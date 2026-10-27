//! The music overlay's data and the wheel buttons that control the player. One thread reads
//! controller buttons often and the player about once a second; windows get `media` (what is
//! playing) and `media-cover` (its cover, once per track) events.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use controls::{Buttons, Command, NowPlaying, Player, Press};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

/// How often buttons are read, and how often the player.
const BUTTON_EVERY: Duration = Duration::from_millis(20);
const PLAYER_EVERY: Duration = Duration::from_secs(1);
/// Waiting for a button to assign gives up after this long.
const LEARN_FOR: Duration = Duration::from_secs(15);

/// What the windows can ask for.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct MediaInfo {
    /// This PC can show what's playing and read controller buttons (Windows).
    pub supported: bool,
    pub now: Option<NowPlaying>,
    /// The cover as a data URL.
    pub cover: Option<String>,
}

#[derive(Default)]
pub struct MediaState {
    info: Mutex<MediaInfo>,
    /// The command waiting for a button press to assign, and since when.
    learning: Mutex<Option<(Command, Instant)>>,
    /// Commands from the windows, run on the media thread.
    queue: Mutex<Vec<Command>>,
}

/// A button assigned to a command, sent as `media-learned`.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct Learned {
    pub command: Command,
    pub press: Press,
}

#[tauri::command]
pub fn media_get(state: tauri::State<'_, MediaState>) -> MediaInfo {
    state.info.lock().unwrap().clone()
}

/// Play/pause or skip, as if the button had been pressed.
#[tauri::command]
pub fn media_send(state: tauri::State<'_, MediaState>, command: Command) {
    state.queue.lock().unwrap().push(command);
}

/// Assign the next controller button pressed to `command`; `None` stops waiting.
#[tauri::command]
pub fn media_learn(state: tauri::State<'_, MediaState>, command: Option<Command>) {
    *state.learning.lock().unwrap() = command.map(|c| (c, Instant::now()));
}

/// Start the media thread. `command_for` looks up the command bound to a press; `assign`
/// stores a new binding.
pub fn start(
    app: AppHandle,
    hwnd: isize,
    command_for: impl Fn(&Press) -> Option<Command> + Send + 'static,
    assign: impl Fn(Command, Press) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("media".into())
        .spawn(move || {
            controls::init_thread();
            let mut player = Player::new();
            let mut buttons = Buttons::new(hwnd);
            let supported = player.is_some();
            app.state::<MediaState>().info.lock().unwrap().supported = supported;
            let mut next_read = Instant::now();
            let mut track = None;
            loop {
                let state = app.state::<MediaState>();
                let mut run: Vec<Command> = std::mem::take(&mut *state.queue.lock().unwrap());
                for press in buttons.poll() {
                    let learning = state.learning.lock().unwrap().take();
                    match learning {
                        Some((command, since)) if since.elapsed() < LEARN_FOR => {
                            assign(command, press.clone());
                            let _ = app.emit("media-learned", Learned { command, press });
                        }
                        _ => run.extend(command_for(&press)),
                    }
                }
                if let Some(p) = &player {
                    for c in &run {
                        p.send(*c);
                    }
                }
                if !run.is_empty() {
                    // show the result right away (the player needs a moment to switch)
                    next_read = next_read.min(Instant::now() + Duration::from_millis(150));
                }

                if Instant::now() >= next_read {
                    next_read = Instant::now() + PLAYER_EVERY;
                    if let Some(p) = &mut player {
                        let now = p.now_playing();
                        let id = now.as_ref().map(|n| n.track_id);
                        let cover = (id != track).then(|| {
                            track = id;
                            p.cover().map(|(mime, bytes)| {
                                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                                format!("data:{mime};base64,{b64}")
                            })
                        });
                        let mut info = state.info.lock().unwrap();
                        if info.now != now {
                            info.now = now.clone();
                            let _ = app.emit("media", now);
                        }
                        if let Some(cover) = cover {
                            info.cover = cover.clone();
                            let _ = app.emit("media-cover", cover);
                        }
                    }
                }
                std::thread::sleep(BUTTON_EVERY);
            }
        })
        .expect("start the media thread");
}
