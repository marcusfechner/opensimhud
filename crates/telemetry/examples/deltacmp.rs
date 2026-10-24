//! Print core's delta and sector deltas next to the values ACC reports, for checking the
//! delta against the game.
//!
//! cargo run --release -p telemetry --example deltacmp -- FILE [EVERY_SECONDS]
use std::fs::File;

use telemetry::adapters::acc::pages::Graphics;
use telemetry::adapters::acc::recording::{Reader, Replay};
use telemetry::adapters::acc::{Acc, Options, Raw, Source};
use telemetry::{AdapterEvent, GameAdapter, Session};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let every: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let mut acc = Acc::new(
        Replay::stepped(Reader::new(File::open(path).unwrap()).unwrap(), 1.0 / 120.0),
        Options::default(),
    );
    let mut session = Session::new(acc.game(), acc.capabilities());
    let mut raw = Reader::new(File::open(path).unwrap()).unwrap();
    let mut g: Option<Graphics> = None;
    let mut pending: Option<(f64, Raw)> = None;
    let mut next = 0.0;
    while !acc.source().finished {
        if acc.poll() == AdapterEvent::Updated {
            session.update(acc.input());
        }
        let at = acc.source().clock();
        loop {
            let rec = pending.take().or_else(|| raw.next_record());
            let Some((t, r)) = rec else { break };
            if t > at {
                pending = Some((t, r));
                break;
            }
            if let Raw::Graphics(b) = r {
                g = Graphics::from_bytes(&b);
            }
        }
        if at < next {
            continue;
        }
        next = at + every;
        let (Some(g), false) = (&g, session.state.cars.is_empty()) else {
            continue;
        };
        let l = &session.state.player.lap;
        println!(
            "{at:8.1} {:?} lap {:>2} cur {:7.3} best {:7.3} ours {:+7.3} acc {:+7} pos {} est {:>7} sec {} pct {:.3} split {}",
            l.sector_deltas
                .iter()
                .map(|d| d.map(|d| (d * 1000.0).round() as i32))
                .collect::<Vec<_>>(),
            g.completed_laps,
            l.current,
            l.best,
            l.delta,
            g.i_delta_lap_time,
            g.is_delta_positive,
            g.i_estimated_lap_time,
            g.current_sector_index,
            g.normalized_car_position,
            g.i_split
        );
    }
}
