//! Runs the telemetry pipeline on its own thread and streams frames to subscribed windows.
//!
//! adapter.poll() → core::Session::update() → stream::Publisher → every window's ipc::Channel

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use telemetry::adapters::acc::live::Live;
use telemetry::adapters::acc::recording::{Reader, Replay};
use telemetry::adapters::acc::{Acc, Options, setup};
use telemetry::adapters::demo;
use telemetry::model::Connection;
use telemetry::stream::{Frame, Publisher};
use telemetry::{AdapterEvent, GameAdapter, Session};

/// Polls per second; the fastest stream is 60 Hz, so this leaves headroom.
const POLL_HZ: f64 = 120.0;

/// Windows receiving frames, keyed by window label. New subscribers get a full snapshot on
/// the next tick.
#[derive(Default, Clone)]
pub struct Hub {
    inner: Arc<Mutex<HubInner>>,
}

#[derive(Default)]
struct HubInner {
    live: Vec<(String, Channel<Frame>)>,
    new: Vec<(String, Channel<Frame>)>,
}

impl Hub {
    /// A reloaded page subscribes again. Its old channel is dead but sends to it still
    /// "succeed", so it is replaced rather than kept.
    fn subscribe(&self, label: String, channel: Channel<Frame>) {
        let mut inner = self.inner.lock().unwrap();
        inner.live.retain(|(l, _)| *l != label);
        inner.new.retain(|(l, _)| *l != label);
        inner.new.push((label, channel));
    }

    /// Stop streaming to a window that was closed.
    pub fn unsubscribe(&self, label: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.live.retain(|(l, _)| l != label);
        inner.new.retain(|(l, _)| l != label);
    }
}

/// Called by each overlay window once its page has loaded.
#[tauri::command]
pub fn telemetry_subscribe(
    channel: Channel<Frame>,
    window: tauri::WebviewWindow,
    hub: tauri::State<'_, Hub>,
) {
    hub.subscribe(window.label().to_owned(), channel);
}

/// Where telemetry comes from. `OPENSIMHUD_SOURCE=demo|acc|replay:<file.ovrec>` overrides the
/// default: ACC on Windows, the demo race elsewhere.
enum Choice {
    Demo,
    Acc,
    Replay(PathBuf),
}

fn choose() -> Choice {
    match std::env::var("OPENSIMHUD_SOURCE").as_deref() {
        Ok("demo") => Choice::Demo,
        Ok("acc") => Choice::Acc,
        Ok(s) if s.starts_with("replay:") => Choice::Replay(s["replay:".len()..].into()),
        _ if cfg!(windows) => Choice::Acc,
        _ => Choice::Demo,
    }
}

/// Start the pipeline. `data_dir` holds learned track maps.
pub fn start(hub: Hub, data_dir: Option<PathBuf>) {
    thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || run(hub, data_dir))
        .expect("could not start the telemetry thread");
}

fn acc_options(data_dir: Option<PathBuf>) -> (Options, u16) {
    let file = setup::config_path().and_then(|p| setup::read(&p).ok());
    let port = file
        .as_ref()
        .filter(|b| b.enabled())
        .map_or(setup::DEFAULT_PORT, |b| b.port);
    let password = file.map_or_else(|| setup::DEFAULT_PASSWORD.into(), |b| b.password);
    let opts = Options {
        password,
        track_cache: data_dir.map(|d| d.join("tracks")),
        ..Options::default()
    };
    (opts, port)
}

fn run(hub: Hub, data_dir: Option<PathBuf>) {
    let (mut adapter, mut session): (Box<dyn GameAdapter>, Session) = match choose() {
        Choice::Demo => {
            let (demo, session) = demo::warmed_up();
            (Box::new(demo), session)
        }
        Choice::Acc => {
            let (opts, port) = acc_options(data_dir);
            let acc = Acc::new(Live::new(([127, 0, 0, 1], port).into()), opts);
            let session = Session::new(acc.game(), acc.capabilities());
            (Box::new(acc), session)
        }
        Choice::Replay(path) => match std::fs::File::open(&path).and_then(Reader::new) {
            Ok(reader) => {
                let (opts, _) = acc_options(data_dir);
                let acc = Acc::new(Replay::realtime(reader), opts);
                let session = Session::new(acc.game(), acc.capabilities());
                (Box::new(acc), session)
            }
            Err(e) => {
                eprintln!(
                    "could not open {}: {e}; using the demo race",
                    path.display()
                );
                let (demo, session) = demo::warmed_up();
                (Box::new(demo), session)
            }
        },
    };
    if adapter.game() != telemetry::model::Game::Demo {
        session.set_connection(Connection::WaitingForGame);
    }

    let mut publisher = Publisher::new();
    let start = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / POLL_HZ);
    let mut next = Instant::now();
    loop {
        match adapter.poll() {
            AdapterEvent::Connection(c) => {
                if c == Connection::Connected {
                    // a fresh start: new bookkeeping, and windows get everything again
                    session = Session::new(adapter.game(), adapter.capabilities());
                    publisher = Publisher::new();
                }
                session.set_connection(c);
            }
            AdapterEvent::Updated => session.update(adapter.input()),
            AdapterEvent::Idle => {}
        }
        let connected = session.state.connection == Connection::Connected;
        // while the game isn't there, windows only learn that (no empty data to render)
        let frames = if connected {
            publisher.poll(&session.state, start.elapsed().as_secs_f64())
        } else {
            publisher.poll_meta(&session.state).into_iter().collect()
        };

        let mut inner = hub.inner.lock().unwrap();
        let fresh = std::mem::take(&mut inner.new);
        // a failed send means the window is gone: drop its channel
        inner
            .live
            .retain(|(_, ch)| frames.iter().all(|f| ch.send(f.clone()).is_ok()));
        for (label, ch) in fresh {
            let snapshot = if connected {
                Publisher::full(&session.state)
            } else {
                vec![Publisher::meta_only(&session.state)]
            };
            if snapshot.into_iter().all(|f| ch.send(f).is_ok()) {
                inner.live.push((label, ch));
            }
        }
        drop(inner);

        next += tick;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}
