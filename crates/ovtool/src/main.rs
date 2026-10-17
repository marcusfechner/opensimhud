//! `ovtool`: inspect telemetry without the app.
//!
//! `ovtool dump [--seconds N] [--compact]` prints the derived overlay state of the demo race
//! as JSON, N simulated seconds after the warm-up. `record` and `replay` arrive with the ACC
//! adapter.

use std::process::ExitCode;

use telemetry::GameAdapter;
use telemetry::adapters::demo;

const USAGE: &str = "usage: ovtool <command>

commands:
  dump [--seconds N] [--compact]   print the demo race state as JSON, N seconds after warm-up
  help                             show this message";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("dump") => dump(&args[1..]),
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
