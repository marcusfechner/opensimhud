//! Print decoded EVO input while a session is running.
use std::time::{Duration, Instant};
use telemetry::adapters::evo::Evo;
use telemetry::{AdapterEvent, GameAdapter, Session};

fn main() {
    let mut game = Evo::default();
    let mut session = Session::new(game.game(), game.capabilities());
    let start = Instant::now();
    let mut last = 0;
    loop {
        match game.poll() {
            AdapterEvent::Connection(c) => {
                session.set_connection(c);
                println!("Connection: {c:?}");
            }
            AdapterEvent::Updated => {
                session.update(game.input());
                if start.elapsed().as_secs() != last {
                    last = start.elapsed().as_secs();
                    let p = &session.state.player;
                    println!(
                        "{}: gear {} speed {:.1} throttle {:.2} brake {:.2} steer {:.2}",
                        session.state.track.name, p.gear, p.speed, p.throttle, p.brake, p.steer
                    );
                }
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
