//! Tauri shell: windows, config, hotkeys and the telemetry transport live here;
//! game data handling lives in the `telemetry` crate.

mod config;
mod transport;
mod updates;
mod windows;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use telemetry::adapters::acc::setup;
use ts_rs::TS;

use config::Config;
use transport::{Command, Event, Hubs, Status};

/// Everything the windows can ask about or change.
struct AppState {
    dir: Option<PathBuf>,
    config: Mutex<Config>,
    ui: Mutex<Ui>,
    status: Mutex<Option<Status>>,
    control: Mutex<Option<Sender<Command>>>,
}

/// Session-only state shared with every window.
#[derive(Debug, Clone, Copy, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
struct Ui {
    /// All overlays hidden (Ctrl+Shift+H).
    hidden: bool,
    /// Overlays can be dragged in game (Ctrl+Shift+L).
    editing: bool,
}

#[tauri::command]
fn config_get(state: tauri::State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

/// Replace the config: repaired, saved, applied, and sent to every window.
#[tauri::command]
fn config_set(app: AppHandle, config: Config) -> Result<Config, String> {
    apply_config(&app, config).map_err(|e| e.to_string())
}

fn apply_config(app: &AppHandle, mut config: Config) -> tauri::Result<Config> {
    config.normalize();
    let state = app.state::<AppState>();
    let old = std::mem::replace(&mut *state.config.lock().unwrap(), config.clone());
    if old.demo != config.demo
        && let Some(tx) = &*state.control.lock().unwrap()
    {
        let _ = tx.send(Command::Demo(config.demo));
    }
    if let Some(dir) = &state.dir
        && let Err(e) = config::save(dir, &config)
    {
        eprintln!("could not save the config: {e}");
    }
    let ui = *state.ui.lock().unwrap();
    windows::sync(app, &config, ui.hidden, ui.editing)?;
    app.emit("config", &config)?;
    Ok(config)
}

#[tauri::command]
fn status_get(state: tauri::State<'_, AppState>) -> Option<Status> {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
fn ui_get(state: tauri::State<'_, AppState>) -> Ui {
    *state.ui.lock().unwrap()
}

#[tauri::command]
fn set_hidden(app: AppHandle, hidden: bool) -> Result<(), String> {
    update_ui(&app, |ui| ui.hidden = hidden).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_editing(app: AppHandle, editing: bool) -> Result<(), String> {
    update_ui(&app, |ui| ui.editing = editing).map_err(|e| e.to_string())
}

fn update_ui(app: &AppHandle, change: impl FnOnce(&mut Ui)) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let ui = {
        let mut ui = state.ui.lock().unwrap();
        change(&mut ui);
        // editing needs the overlays visible
        if ui.editing {
            ui.hidden = false;
        }
        *ui
    };
    let config = state.config.lock().unwrap().clone();
    windows::sync(app, &config, ui.hidden, ui.editing)?;
    windows::set_editing(app, ui.editing)?;
    app.emit("ui", ui)
}

#[tauri::command]
fn displays(app: AppHandle) -> Vec<windows::Display> {
    windows::displays(&app)
}

/// ACC's `broadcasting.json`, which has to name a port before ACC sends other cars' data.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
struct AccSetup {
    /// Where the file is (or would be); `None` without a Documents folder.
    path: Option<String>,
    enabled: bool,
    port: u16,
    /// Why the file couldn't be read or written, for the user.
    error: Option<String>,
}

fn acc_setup_read() -> AccSetup {
    let Some(path) = setup::config_path() else {
        return AccSetup {
            path: None,
            enabled: false,
            port: 0,
            error: Some("No Documents folder found.".into()),
        };
    };
    let shown = Some(path.display().to_string());
    match setup::read(&path) {
        Ok(b) => AccSetup {
            path: shown,
            enabled: b.enabled(),
            port: b.port,
            error: None,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AccSetup {
            path: shown,
            enabled: false,
            port: 0,
            error: None,
        },
        Err(e) => AccSetup {
            path: shown,
            enabled: false,
            port: 0,
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
fn acc_setup() -> AccSetup {
    acc_setup_read()
}

/// Turn broadcasting on with the usual port and password; ACC needs a restart afterwards.
#[tauri::command]
fn acc_enable() -> AccSetup {
    let Some(path) = setup::config_path() else {
        return acc_setup_read();
    };
    match setup::enable(&path, setup::DEFAULT_PORT, setup::DEFAULT_PASSWORD) {
        Ok(()) => acc_setup_read(),
        Err(e) => AccSetup {
            error: Some(e.to_string()),
            ..acc_setup_read()
        },
    }
}

/// Telemetry pipeline → app: keep the status, switch profiles when a session starts.
fn on_event(app: &AppHandle, event: Event) {
    let state = app.state::<AppState>();
    match event {
        Event::Status(s) => {
            *state.status.lock().unwrap() = Some(s.clone());
            let _ = app.emit("status", s);
        }
        Event::Session(kind, duration) => {
            let config = state.config.lock().unwrap().clone();
            if let Some(name) = config.profile_for(kind, duration)
                && name != config.profile
            {
                let mut next = config.clone();
                next.profile = name.to_owned();
                let app = app.clone();
                // window changes must happen on the main thread
                let _ = app.clone().run_on_main_thread(move || {
                    let _ = apply_config(&app, next);
                });
            }
        }
    }
}

fn shortcuts() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let hide = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyH);
    let edit = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyL);
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let ui = *app.state::<AppState>().ui.lock().unwrap();
            let _ = if *shortcut == hide {
                update_ui(app, |u| u.hidden = !ui.hidden)
            } else if *shortcut == edit {
                update_ui(app, |u| u.editing = !ui.editing)
            } else {
                Ok(())
            };
        })
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(shortcuts())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Hubs::default())
        .manage(updates::Pending::default())
        .invoke_handler(tauri::generate_handler![
            transport::telemetry_subscribe,
            transport::preview_subscribe,
            transport::preview_unsubscribe,
            config_get,
            config_set,
            status_get,
            ui_get,
            set_hidden,
            set_editing,
            displays,
            acc_setup,
            acc_enable,
            updates::update_check,
            updates::update_install,
        ])
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Destroyed => {
                let hubs = window.state::<Hubs>();
                hubs.live.unsubscribe(window.label());
                hubs.preview.unsubscribe(window.label());
            }
            // the settings window is the app: closing it quits (a tray icon comes later)
            tauri::WindowEvent::CloseRequested { .. } if window.label() == "main" => {
                window.app_handle().exit(0);
            }
            _ => {}
        })
        .setup(|app| {
            let dir = app.path().app_config_dir().ok();
            let config = dir.as_deref().map(config::load).unwrap_or_default();
            app.manage(AppState {
                dir,
                config: Mutex::new(config.clone()),
                ui: Mutex::new(Ui::default()),
                status: Mutex::new(None),
                control: Mutex::new(None),
            });

            let handle = app.handle().clone();
            let hubs = app.state::<Hubs>().inner().clone();
            let tx = transport::start(
                hubs,
                app.path().app_data_dir().ok(),
                config.demo,
                move |e| on_event(&handle, e),
            );
            *app.state::<AppState>().control.lock().unwrap() = Some(tx);

            windows::sync(app.handle(), &config, false, false)?;
            let gs = app.global_shortcut();
            for s in [
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyH),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyL),
            ] {
                // another app may own the combination; the buttons in the app still work
                if let Err(e) = gs.register(s) {
                    eprintln!("could not register a hotkey: {e}");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
