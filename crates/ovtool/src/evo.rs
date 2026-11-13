//! EVO recording and replay commands.
use std::io::{BufWriter, Write};
use std::process::ExitCode;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use telemetry::adapters::evo::{live::Pages, recording::Replay};
use telemetry::stream::Publisher;
use telemetry::{AdapterEvent, GameAdapter, Session};

pub fn record(args: &[String]) -> ExitCode {
    let [path] = args else {
        eprintln!("usage: ovtool evo-record FILE.jsonl");
        return ExitCode::FAILURE;
    };
    if !path.ends_with(".jsonl") {
        eprintln!("EVO recordings must use the .jsonl extension");
        return ExitCode::FAILURE;
    }
    let Some(initial_pages) = Pages::open() else {
        eprintln!("Could not open EVO pages; start a driving session first.");
        return ExitCode::FAILURE;
    };
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Could not create {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut out = BufWriter::new(file);
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    if let Err(e) = ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)) {
        eprintln!("Could not install Ctrl+C handler: {e}");
        return ExitCode::FAILURE;
    }
    println!("Recording to {path}. Press Ctrl+C to stop.");
    let start = Instant::now();
    let mut pages = Some(initial_pages);
    let mut last_packet = None;
    let mut changed_at = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if pages.is_none() {
            pages = Pages::open();
            last_packet = None;
            changed_at = Instant::now();
        }
        let Some((p, g, s)) = pages
            .as_ref()
            .and_then(|pages| Some((pages.read(0)?, pages.read(1)?, pages.read(2)?)))
        else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let packet = i32::from_le_bytes(p[..4].try_into().unwrap());
        if last_packet != Some(packet) {
            last_packet = Some(packet);
            changed_at = Instant::now();
        } else if changed_at.elapsed() >= Duration::from_secs(3) {
            pages = None;
            continue;
        }
        let hex = |b: &[u8]| -> String { b.iter().map(|n| format!("{n:02x}")).collect() };
        let row = serde_json::json!({"format":"opensimhud-evo-1", "seconds":start.elapsed().as_secs_f64(),
            "physics_hex":hex(&p), "graphics_hex":hex(&g), "static_hex":hex(&s)});
        if let Err(e) = writeln!(out, "{row}").and_then(|_| out.flush()) {
            eprintln!("Recording failed: {e}");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    ExitCode::SUCCESS
}

pub fn replay(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("usage: ovtool replay FILE.jsonl [--frames]");
        return ExitCode::FAILURE;
    };
    if args.len() > 2 || args.get(1).is_some_and(|s| s != "--frames") {
        eprintln!("usage: ovtool replay FILE.jsonl [--frames]");
        return ExitCode::FAILURE;
    }
    let mut source = match std::fs::File::open(path).and_then(|f| Replay::stepped(f, 1.0 / 120.0)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Could not open {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let frames = args.get(1).is_some();
    let mut session = Session::new(source.game(), source.capabilities());
    let mut publisher = Publisher::new();
    let mut out = BufWriter::new(std::io::stdout().lock());
    let mut next_report = 0.0;
    while !source.finished() {
        match source.poll() {
            AdapterEvent::Connection(c) => session.set_connection(c),
            AdapterEvent::Updated => session.update(source.input()),
            AdapterEvent::Idle => {}
        }
        let at = source.clock();
        if session.state.cars.is_empty() {
            continue;
        }
        if frames {
            for frame in publisher.poll(&session.state, at) {
                let Ok(json) = serde_json::to_string(&frame) else {
                    return ExitCode::FAILURE;
                };
                if writeln!(out, r#"{{"at":{at:.4},"frame":{json}}}"#).is_err() {
                    return ExitCode::FAILURE;
                }
            }
        } else if at >= next_report {
            next_report = at + 60.0;
            let p = &session.state.player;
            println!(
                "[{at:.0}s] {} | gear {} speed {:.1} km/h throttle {:.2} brake {:.2}",
                session.state.track.name, p.gear, p.speed, p.throttle, p.brake
            );
        }
    }
    if let Some(e) = source.error {
        eprintln!("Replay failed: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
