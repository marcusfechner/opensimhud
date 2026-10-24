//! Updates from GitHub Releases: the app checks once at start, the user installs with one
//! click. Releases are signed in CI (.github/workflows/release.yml); the public key is in
//! tauri.conf.json, so only builds signed with the matching private key are installed.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use ts_rs::TS;

/// The update found by the last check, kept for `update_install`.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

/// A newer version than the running one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct UpdateInfo {
    pub version: String,
    /// The release notes, if the release has any.
    pub notes: Option<String>,
}

/// Download progress, sent as the `update-progress` event.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct UpdateProgress {
    #[ts(type = "number")]
    pub done: u64,
    /// The download size, when the server says.
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}

/// Ask GitHub for a newer release. `None` when this is the latest.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
    });
    *app.state::<Pending>().0.lock().unwrap() = update;
    Ok(info)
}

/// Download and install the update found by `update_check`, then restart into it.
/// On Windows the installer closes the app itself and starts the new version.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = app.state::<Pending>().0.lock().unwrap().take();
    let Some(update) = update else {
        return Err("No update to install. Check again.".into());
    };
    let mut done = 0u64;
    let result = update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                let _ = app.emit("update-progress", UpdateProgress { done, total });
            },
            || {},
        )
        .await;
    if let Err(e) = result {
        // keep it, so the user can try again
        *app.state::<Pending>().0.lock().unwrap() = Some(update);
        return Err(e.to_string());
    }
    app.restart()
}
