//! ACC commands: `setup`, `record` and `replay`.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use telemetry::adapters::acc::live::Live;
use telemetry::adapters::acc::pages::wstr;
use telemetry::adapters::acc::recording::{Reader, Recording, Replay, Writer};
use telemetry::adapters::acc::{Acc, Options, Source, setup};
use telemetry::model::Connection;
use telemetry::stream::Publisher;
use telemetry::{AdapterEvent, GameAdapter, Session};

/// `--name value` options after the command.
struct Args(Vec<String>);

impl Args {
    fn value(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .position(|a| a == name)
            .and_then(|i| self.0.get(i + 1))
            .map(String::as_str)
    }
    fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
    fn positional(&self) -> Option<&str> {
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if a.starts_with("--") {
                if !matches!(a.as_str(), "--frames" | "--realtime" | "--enable") {
                    it.next();
                }
            } else {
                return Some(a);
            }
        }
        None
    }
}

/// Port and password: command line first, then broadcasting.json, then ACC's usual defaults.
fn connection(args: &Args) -> (u16, String) {
    let file = setup::config_path().and_then(|p| setup::read(&p).ok());
    let port = args
        .value("--port")
        .and_then(|p| p.parse().ok())
        .or(file.as_ref().filter(|b| b.enabled()).map(|b| b.port))
        .unwrap_or(setup::DEFAULT_PORT);
    let password = args
        .value("--password")
        .map(str::to_owned)
        .or(file.map(|b| b.password))
        .unwrap_or_else(|| setup::DEFAULT_PASSWORD.into());
    (port, password)
}

pub fn setup(args: &[String]) -> ExitCode {
    let args = Args(args.to_vec());
    let Some(path) = setup::config_path() else {
        eprintln!("Could not find your Documents folder.");
        return ExitCode::FAILURE;
    };
    println!("broadcasting.json: {}", path.display());
    let current = setup::read(&path);
    match &current {
        Ok(b) if b.enabled() => println!(
            "Broadcasting is on: port {}, password \"{}\".",
            b.port, b.password
        ),
        Ok(_) => println!("Broadcasting is off (port 0)."),
        Err(e) => println!("Could not read it: {e}"),
    }
    if !args.flag("--enable") {
        if !matches!(current, Ok(ref b) if b.enabled()) {
            println!("\nRun `ovtool setup --enable` to turn it on, then restart ACC.");
        }
        return ExitCode::SUCCESS;
    }
    let port = args
        .value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(setup::DEFAULT_PORT);
    let password = args.value("--password").unwrap_or(setup::DEFAULT_PASSWORD);
    match setup::enable(&path, port, password) {
        Ok(()) => {
            println!(
                "Turned broadcasting on: port {port}, password \"{password}\". The old file is kept as broadcasting.json.bak."
            );
            println!("Restart ACC so it picks this up.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Could not write it: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn record(args: &[String]) -> ExitCode {
    let args = Args(args.to_vec());
    if !Live::has_shared_memory() {
        eprintln!(
            "Recording needs Windows: ACC shares the player's data through Windows shared memory."
        );
        return ExitCode::FAILURE;
    }
    let (port, password) = connection(&args);
    let host = args.value("--host").unwrap_or("127.0.0.1");
    let Some(target) = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
    else {
        eprintln!("Could not resolve {host}:{port}");
        return ExitCode::FAILURE;
    };
    let out = args
        .value("--out")
        .map(PathBuf::from)
        .unwrap_or_else(default_name);
    let file = match File::create(&out) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Could not create {}: {e}", out.display());
            return ExitCode::FAILURE;
        }
    };
    let writer = match Writer::new(file) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Could not write {}: {e}", out.display());
            return ExitCode::FAILURE;
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        if ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst)).is_err() {
            eprintln!(
                "Could not install the Ctrl+C handler; stop with Ctrl+Break instead (the file stays readable)."
            );
        }
    }

    println!("Recording to {}", out.display());
    println!("Broadcast: {target}, password \"{password}\". Press Ctrl+C to stop.\n");
    let opts = Options {
        password,
        ..Options::default()
    };
    let mut acc = Acc::new(Recording::new(Live::new(target), writer), opts);
    let tick = Duration::from_secs_f64(1.0 / 120.0);
    let mut last_status = Instant::now() - Duration::from_secs(10);
    let mut connected = false;
    while !stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        if let AdapterEvent::Connection(c) = acc.poll() {
            connected = c == Connection::Connected;
            println!(
                "{}",
                if connected {
                    "\nACC found."
                } else {
                    "\nWaiting for ACC (start a session; menus don't send data)…"
                }
            );
        }
        if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            print_status(&acc, connected);
        }
        if let Some(e) = acc.source_mut().error.take() {
            eprintln!("\nWriting the recording failed: {e}");
            return ExitCode::FAILURE;
        }
        if let Some(rest) = tick.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let records = acc.source().writer.as_ref().map_or(0, |w| w.records);
    match acc.source_mut().finish() {
        Ok(()) => {
            let size = std::fs::metadata(&out).map_or(0, |m| m.len());
            println!(
                "\n\nSaved {} ({} records, {:.1} MB).",
                out.display(),
                records,
                size as f64 / 1e6
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("\nCould not finish the recording: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One status line, rewritten in place: live values to compare with the game's own HUD.
fn print_status<S: Source, W: Write + Send>(acc: &Acc<Recording<S, W>>, connected: bool) {
    let mb = acc
        .source()
        .writer
        .as_ref()
        .map_or(0.0, |w| w.bytes as f64 / 1e6);
    let line = match (
        connected,
        acc.physics_page(),
        acc.graphics_page(),
        acc.static_page(),
    ) {
        (true, Some(p), Some(g), st) => {
            let car = st.map(|s| wstr(&s.car_model)).unwrap_or_default();
            let track = st.map(|s| wstr(&s.track)).unwrap_or_default();
            let inp = acc.input();
            format!(
                "{track} {car} | lap {} {:.0}% | gear {} {:>3.0} km/h | fuel {:.1} L | TC {} ABS {} BB {:.1} map {} | broadcast {} | {:.1} MB raw",
                g.completed_laps + 1,
                g.normalized_car_position * 100.0,
                inp.player.gear,
                p.speed_kmh,
                p.fuel,
                g.tc,
                g.abs,
                inp.player.bb,
                g.engine_map + 1,
                if acc.broadcast_registered() {
                    format!("{} cars", acc.broadcast_cars())
                } else {
                    "not connected".into()
                },
                mb
            )
        }
        _ => format!("waiting… {mb:.1} MB raw"),
    };
    print!("\r{line:<150}");
    let _ = std::io::stdout().flush();
}

pub fn replay(args: &[String]) -> ExitCode {
    let args = Args(args.to_vec());
    let Some(path) = args.positional().map(Path::new) else {
        eprintln!("usage: ovtool replay <file.ovrec> [--frames]");
        return ExitCode::FAILURE;
    };
    let reader = match File::open(path).and_then(Reader::new) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Could not open {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    const TICK: f64 = 1.0 / 120.0;
    let mut acc = Acc::new(Replay::stepped(reader, TICK), Options::default());
    let mut session = Session::new(acc.game(), acc.capabilities());
    let frames = args.flag("--frames");
    let mut publisher = Publisher::new();
    let mut out = BufWriter::new(std::io::stdout().lock());
    let mut next_report = 0.0;
    while !acc.source().finished {
        let ev = acc.poll();
        if ev == AdapterEvent::Updated {
            session.update(acc.input());
        }
        let at = acc.source().clock();
        if frames {
            if session.state.cars.is_empty() {
                continue;
            }
            for frame in publisher.poll(&session.state, at) {
                let Ok(json) = serde_json::to_string(&frame) else {
                    return ExitCode::FAILURE;
                };
                if writeln!(out, r#"{{"at":{at:.4},"frame":{json}}}"#).is_err() {
                    return ExitCode::FAILURE;
                }
            }
        } else if at >= next_report && !session.state.cars.is_empty() {
            next_report = at + 60.0;
            report(&session, at);
        }
    }
    if !frames {
        report(&session, acc.source().clock());
        let st = &session.state;
        println!("\nLap log:");
        for e in &st.player.log {
            println!(
                "  lap {:>3}  {:>9.3} s  {}{}  fuel {:.2} L",
                e.lap,
                e.time,
                if e.valid { "" } else { "invalid " },
                if e.pit { "pit " } else { "" },
                e.fuel
            );
        }
        println!(
            "\nTrack outline learned: {}",
            if st.track.x.is_empty() { "no" } else { "yes" }
        );
    }
    ExitCode::SUCCESS
}

fn report(session: &Session, at: f64) {
    let st = &session.state;
    let p = &st.player;
    let me = &st.cars[p.car];
    println!(
        "[{:>6.0} s] {} {} | {} cars in {:?} | P{} (class P{}) lap {} | last {:.3} best {:.3} | fuel {:.1} L ({:.2}/lap)",
        at,
        st.track.name,
        st.session.name,
        st.cars.len(),
        st.classes,
        me.pos,
        me.class_pos,
        me.laps + 1,
        p.lap.last,
        p.lap.best,
        p.fuel.liters,
        p.fuel.per_lap
    );
}

/// `acc-2026-10-17-2215.ovrec` in the current folder (UTC).
fn default_name() -> PathBuf {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    PathBuf::from(format!(
        "acc-{y:04}-{m:02}-{d:02}-{:02}{:02}.ovrec",
        rem / 3600,
        rem % 3600 / 60
    ))
}
