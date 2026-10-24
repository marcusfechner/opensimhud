//! Print each completed lap with core's last and best next to ACC's, to spot laps one of
//! them doesn't count.
//!
//! cargo run --release -p telemetry --example bestcmp -- FILE
use std::fs::File;

use telemetry::adapters::acc::pages::Graphics;
use telemetry::adapters::acc::recording::{Reader, Replay};
use telemetry::adapters::acc::{Acc, Options, Raw, Source};
use telemetry::{AdapterEvent, GameAdapter, Session};

fn main() {
    let path = std::env::args().nth(1).expect("usage: bestcmp FILE");
    let mut acc = Acc::new(
        Replay::stepped(
            Reader::new(File::open(&path).unwrap()).unwrap(),
            1.0 / 120.0,
        ),
        Options::default(),
    );
    let mut session = Session::new(acc.game(), acc.capabilities());
    let mut raw = Reader::new(File::open(&path).unwrap()).unwrap();
    let mut pending: Option<(f64, Raw)> = None;
    let mut logged = 0;
    let mut acc_laps = -1;
    let mut valid_log: Vec<String> = Vec::new();
    while !acc.source().finished {
        if acc.poll() == AdapterEvent::Updated {
            session.update(acc.input());
        }
        let at = acc.source().clock();
        while let Some((t, r)) = pending.take().or_else(|| raw.next_record()) {
            if t > at {
                pending = Some((t, r));
                break;
            }
            if let Raw::Graphics(b) = r
                && let Some(g) = Graphics::from_bytes(&b)
            {
                if g.completed_laps != acc_laps {
                    acc_laps = g.completed_laps;
                    println!(
                        "{t:8.2}   ACC lap {:>2} done: last {:>7} best {:>10} valid now {} cur {}",
                        g.completed_laps,
                        g.i_last_time,
                        g.i_best_time,
                        g.is_valid_lap,
                        g.i_current_time
                    );
                }
                let v = format!("{}", g.is_valid_lap);
                if valid_log.last() != Some(&v) {
                    println!(
                        "{t:8.2}     valid -> {v} cur {} pct {:.3} laps {}",
                        g.i_current_time, g.normalized_car_position, g.completed_laps
                    );
                    valid_log.push(v);
                }
            }
        }
        let log = &session.state.player.log;
        if log.len() != logged {
            logged = log.len();
            let e = log.last().unwrap();
            let l = &session.state.player.lap;
            println!(
                "{at:8.2} core lap {:>2} done: time {:>8.3} valid {} pit {} -> last {:.3} best {:.3}",
                e.lap, e.time, e.valid, e.pit, l.last, l.best
            );
        }
    }
}
