//! `ovtool`: record, replay and dump telemetry without the app.
//! Subcommands arrive with the demo adapter (dump) and the ACC adapter (record, replay).

use std::process::ExitCode;

const USAGE: &str = "usage: ovtool <command>

commands:
  help    show this message

coming next: dump, record, replay";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None | Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
