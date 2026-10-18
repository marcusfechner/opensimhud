//! Runs the telemetry pipelines on their own threads and streams frames to subscribed windows.
//!
//! adapter.poll() → core::Session::update() → stream::Publisher → every window's ipc::Channel
//!
//! - live: the game (or the demo race in demo mode) for the overlay windows;
//! - preview: always the demo race, for the settings window's previews; it only runs while
//!   that window is subscribed.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use telemetry::adapters::acc::live::Live;
use telemetry::adapters::acc::recording::{Reader, Replay};
use telemetry::adapters::acc::{Acc, Options, setup};
use telemetry::adapters::demo;
use telemetry::model::{Connection, Game, SessionKind};
use telemetry::stream::{Frame, Publisher};
use telemetry::{AdapterEvent, GameAdapter, Session};
use ts_rs::TS;

/// Polls per second; the fastest stream is 60 Hz, so this leaves headroom.
const POLL_HZ: f64 = 120.0;
/// Previews don't need the full rate.
const PREVIEW_HZ: f64 = 60.0;

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

    /// Stop streaming to a window that was closed or hidden.
    pub fn unsubscribe(&self, label: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.live.retain(|(l, _)| l != label);
        inner.new.retain(|(l, _)| l != label);
    }

    fn is_empty(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.live.is_empty() && inner.new.is_empty()
    }

    /// Everyone gets a full snapshot on the next tick (the source changed).
    fn resend_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        let live = std::mem::take(&mut inner.live);
        inner.new.extend(live);
    }

    /// Send this tick's frames; newcomers get a snapshot instead.
    fn send(&self, frames: &[Frame], snapshot: impl Fn() -> Vec<Frame>) {
        let mut inner = self.inner.lock().unwrap();
        let fresh = std::mem::take(&mut inner.new);
        // a failed send means the window is gone: drop its channel
        inner
            .live
            .retain(|(_, ch)| frames.iter().all(|f| ch.send(f.clone()).is_ok()));
        for (label, ch) in fresh {
            if snapshot().into_iter().all(|f| ch.send(f).is_ok()) {
                inner.live.push((label, ch));
            }
        }
    }
}

/// The overlay windows' feed, and the settings window's preview feed.
#[derive(Default, Clone)]
pub struct Hubs {
    pub live: Hub,
    pub preview: Hub,
}

/// Called by each overlay window once its page has loaded.
#[tauri::command]
pub fn telemetry_subscribe(
    channel: Channel<Frame>,
    window: tauri::WebviewWindow,
    hubs: tauri::State<'_, Hubs>,
) {
    hubs.live.subscribe(window.label().to_owned(), channel);
}

/// The settings window's previews: the demo race, while the window is visible.
#[tauri::command]
pub fn preview_subscribe(
    channel: Channel<Frame>,
    window: tauri::WebviewWindow,
    hubs: tauri::State<'_, Hubs>,
) {
    hubs.preview.subscribe(window.label().to_owned(), channel);
}

#[tauri::command]
pub fn preview_unsubscribe(window: tauri::WebviewWindow, hubs: tauri::State<'_, Hubs>) {
    hubs.preview.unsubscribe(window.label());
}

/// What the settings window shows about the game.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct Status {
    pub game: Game,
    pub connection: Connection,
    /// The overlays show the demo race.
    pub demo: bool,
    pub track: String,
    pub session: Option<SessionKind>,
}

/// What the live pipeline reports to the app.
pub enum Event {
    Status(Status),
    /// A session of this kind and planned length (seconds) started.
    Session(SessionKind, f64),
}

/// Changes to the live pipeline.
pub enum Command {
    Demo(bool),
}

/// Where telemetry comes from. `OPENSIMHUD_SOURCE=demo|acc|replay:<file.ovrec>` overrides the
/// default: ACC on Windows, the demo race elsewhere. Demo mode always wins.
enum Choice {
    Demo,
    Acc,
    Replay(PathBuf),
}

fn choose(demo: bool) -> Choice {
    if demo {
        return Choice::Demo;
    }
    match std::env::var("OPENSIMHUD_SOURCE").as_deref() {
        Ok("demo") => Choice::Demo,
        Ok("acc") => Choice::Acc,
        Ok(s) if s.starts_with("replay:") => Choice::Replay(s["replay:".len()..].into()),
        _ if cfg!(windows) => Choice::Acc,
        _ => Choice::Demo,
    }
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

fn waiting(adapter: Box<dyn GameAdapter>) -> (Box<dyn GameAdapter>, Session) {
    let mut session = Session::new(adapter.game(), adapter.capabilities());
    session.set_connection(Connection::WaitingForGame);
    (adapter, session)
}

fn make(choice: Choice, data_dir: Option<PathBuf>) -> (Box<dyn GameAdapter>, Session) {
    match choice {
        Choice::Demo => {
            let (demo, session) = demo::warmed_up();
            (Box::new(demo), session)
        }
        Choice::Acc => {
            let (opts, port) = acc_options(data_dir);
            waiting(Box::new(Acc::new(
                Live::new(([127, 0, 0, 1], port).into()),
                opts,
            )))
        }
        Choice::Replay(path) => match std::fs::File::open(&path).and_then(Reader::new) {
            Ok(reader) => waiting(Box::new(Acc::new(
                Replay::realtime(reader),
                acc_options(data_dir).0,
            ))),
            Err(e) => {
                eprintln!(
                    "could not open {}: {e}; using the demo race",
                    path.display()
                );
                make(Choice::Demo, data_dir)
            }
        },
    }
}

/// Start both pipelines. `data_dir` holds learned track maps; `demo` is the initial mode.
pub fn start(
    hubs: Hubs,
    data_dir: Option<PathBuf>,
    demo: bool,
    events: impl Fn(Event) + Send + 'static,
) -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    let live = hubs.live.clone();
    thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || run_live(live, data_dir, demo, rx, events))
        .expect("could not start the telemetry thread");
    thread::Builder::new()
        .name("preview".into())
        .spawn(move || run_preview(hubs.preview))
        .expect("could not start the preview thread");
    tx
}

/// Sleep until the next tick at a fixed rate.
struct Ticker {
    tick: Duration,
    next: Instant,
}

impl Ticker {
    fn new(hz: f64) -> Self {
        Self {
            tick: Duration::from_secs_f64(1.0 / hz),
            next: Instant::now(),
        }
    }
    fn wait(&mut self) {
        self.next += self.tick;
        let now = Instant::now();
        if self.next > now {
            thread::sleep(self.next - now);
        } else {
            self.next = now;
        }
    }
}

fn run_live(
    hub: Hub,
    data_dir: Option<PathBuf>,
    mut demo: bool,
    commands: Receiver<Command>,
    events: impl Fn(Event),
) {
    let (mut adapter, mut session) = make(choose(demo), data_dir.clone());
    let mut publisher = Publisher::new();
    let start = Instant::now();
    let mut ticker = Ticker::new(POLL_HZ);
    let mut status: Option<Status> = None;
    let mut session_key: Option<(SessionKind, bool)> = None;
    loop {
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                Command::Demo(on) => {
                    if on != demo {
                        demo = on;
                        (adapter, session) = make(choose(demo), data_dir.clone());
                        // windows must forget the old source's cars and track
                        publisher = Publisher::new();
                        hub.resend_all();
                        session_key = None;
                    }
                }
            }
        }
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
        let st = &session.state;
        let connected = st.connection == Connection::Connected;

        let now = Status {
            game: st.game,
            connection: st.connection,
            demo,
            track: if connected {
                st.track.name.clone()
            } else {
                String::new()
            },
            session: connected.then_some(st.session.kind),
        };
        if status.as_ref() != Some(&now) {
            events(Event::Status(now.clone()));
            status = Some(now);
        }
        // a session starts, or turns out to be an endurance race once its length is known
        if connected {
            let key = (st.session.kind, st.session.duration >= 90.0 * 60.0);
            if session_key != Some(key) {
                session_key = Some(key);
                events(Event::Session(key.0, st.session.duration));
            }
        } else {
            session_key = None;
        }

        // while the game isn't there, windows only learn that (no empty data to render)
        let frames = if connected {
            publisher.poll(st, start.elapsed().as_secs_f64())
        } else {
            publisher.poll_meta(st).into_iter().collect()
        };
        hub.send(&frames, || {
            if connected {
                Publisher::full(st)
            } else {
                vec![Publisher::meta_only(st)]
            }
        });
        ticker.wait();
    }
}

type Race = (Box<dyn GameAdapter>, Session, Publisher);

fn run_preview(hub: Hub) {
    let mut race: Option<Race> = None;
    let start = Instant::now();
    let mut ticker = Ticker::new(PREVIEW_HZ);
    loop {
        if hub.is_empty() {
            // nobody looking: costs nothing until the settings window shows again
            thread::sleep(Duration::from_millis(200));
            ticker = Ticker::new(PREVIEW_HZ);
            continue;
        }
        let (adapter, session, publisher) = race.get_or_insert_with(|| {
            let (demo, session) = demo::warmed_up();
            (Box::new(demo), session, Publisher::new())
        });
        if adapter.poll() == AdapterEvent::Updated {
            session.update(adapter.input());
        }
        let frames = publisher.poll(&session.state, start.elapsed().as_secs_f64());
        hub.send(&frames, || Publisher::full(&session.state));
        ticker.wait();
    }
}
