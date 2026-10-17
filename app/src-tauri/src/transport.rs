//! Runs the telemetry pipeline on its own thread and streams frames to subscribed windows.
//!
//! adapter.poll() → core::Session::update() → stream::Publisher → every window's ipc::Channel

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use telemetry::GameAdapter;
use telemetry::adapters::demo;
use telemetry::stream::{Frame, Publisher};

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

/// Start the pipeline. For now always the demo race; game detection comes with the ACC adapter.
pub fn start(hub: Hub) {
    thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || run(hub))
        .expect("could not start the telemetry thread");
}

fn run(hub: Hub) {
    let (mut adapter, mut session) = demo::warmed_up();
    let mut publisher = Publisher::new();
    let start = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / POLL_HZ);
    let mut next = Instant::now();
    loop {
        adapter.poll();
        session.update(adapter.input());
        let frames = publisher.poll(&session.state, start.elapsed().as_secs_f64());

        let mut inner = hub.inner.lock().unwrap();
        let fresh = std::mem::take(&mut inner.new);
        // a failed send means the window is gone: drop its channel
        inner
            .live
            .retain(|(_, ch)| frames.iter().all(|f| ch.send(f.clone()).is_ok()));
        for (label, ch) in fresh {
            if Publisher::full(&session.state)
                .into_iter()
                .all(|f| ch.send(f).is_ok())
            {
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
