//! `ovtool`: inspect telemetry without the app.
//!
//! `ovtool dump [--seconds N] [--compact]` prints the derived overlay state of the demo race
//! as JSON, N simulated seconds after the warm-up.
//! `ovtool frames [--seconds N]` prints the frames the overlay window would receive, one JSON
//! object `{"at": seconds, "frame": …}` per line; the overlay's browser dev mode replays them.
//! `ovtool setup`, `record` and `replay` work with Assetto Corsa Competizione (see acc.rs).

mod acc;
mod evo;

use std::io::Write;
use std::process::ExitCode;

use telemetry::GameAdapter;
use telemetry::adapters::demo;
use telemetry::stream::Publisher;

const USAGE: &str = "usage: ovtool <command>

commands:
  dump [--seconds N] [--compact]   print the demo race state as JSON, N seconds after warm-up
  frames [--seconds N]             print N seconds of overlay frames as JSON lines (default 30)

  ACC (Assetto Corsa Competizione):
  setup [--enable] [--port P] [--password PW]
                                   show or turn on broadcasting in broadcasting.json
  record [--out FILE] [--port P] [--password PW] [--host H]
                                   record a session to an .ovrec file (Windows, Ctrl+C stops)
  replay FILE [--frames]           summarize a recording, or print its overlay frames as JSON lines

  EVO (Assetto Corsa EVO):
  evo-record FILE.jsonl            record raw EVO shared memory (Windows, Ctrl+C stops)
  replay FILE.jsonl [--frames]     replay a versioned EVO recording

  help                             show this message";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("dump") => dump(&args[1..]),
        Some("frames") => frames(&args[1..]),
        Some("setup") => acc::setup(&args[1..]),
        Some("record") => acc::record(&args[1..]),
        Some("replay") if args.get(1).is_some_and(|s| s.ends_with(".jsonl")) => {
            evo::replay(&args[1..])
        }
        Some("replay") => acc::replay(&args[1..]),
        Some("evo-record") => evo::record(&args[1..]),
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn dump(args: &[String]) -> ExitCode {
    let mut seconds = 0.0;
    let mut compact = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--seconds" => match it.next().and_then(|v| v.parse().ok()) {
                Some(v) => seconds = v,
                None => {
                    eprintln!("--seconds needs a number");
                    return ExitCode::FAILURE;
                }
            },
            "--compact" => compact = true,
            other => {
                eprintln!("unknown option: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (mut demo, mut session) = demo::warmed_up();
    for _ in 0..(seconds / 0.1) as usize {
        demo.step(0.1);
        session.update(demo.input());
    }
    let json = if compact {
        serde_json::to_string(&session.state)
    } else {
        serde_json::to_string_pretty(&session.state)
    };
    match json {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not serialize the state: {e}");
            ExitCode::FAILURE
        }
    }
}

fn frames(args: &[String]) -> ExitCode {
    let seconds = match args {
        [] => 30.0,
        [flag, v] if flag == "--seconds" => match v.parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("--seconds needs a number");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: ovtool frames [--seconds N]");
            return ExitCode::FAILURE;
        }
    };
    let (mut demo, mut session) = demo::warmed_up();
    let mut publisher = Publisher::new();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    const TICK: f64 = 1.0 / 120.0;
    for k in 0..=(seconds / TICK) as usize {
        let at = k as f64 * TICK;
        if k > 0 {
            demo.step(TICK);
            session.update(demo.input());
        }
        for frame in publisher.poll(&session.state, at) {
            // serialize the frame itself (json! would widen every f32 to a long f64)
            let Ok(frame) = serde_json::to_string(&frame) else {
                return ExitCode::FAILURE;
            };
            if writeln!(out, r#"{{"at":{at:.4},"frame":{frame}}}"#).is_err() {
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
