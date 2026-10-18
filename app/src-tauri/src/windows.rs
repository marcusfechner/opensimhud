//! Overlay windows: one transparent, click-through, always-on-top window per display that has
//! overlays in the active profile. Each hosts all of that display's overlays.
//!
//! Edit mode makes them take the mouse so overlays can be dragged; hiding hides them all.

use std::collections::BTreeSet;

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindowBuilder,
};
use ts_rs::TS;

use crate::config::Config;

const PREFIX: &str = "overlay-";

/// A display as the settings window lists it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct Display {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Displays, primary first, then left to right.
pub fn monitors(app: &AppHandle) -> Vec<Monitor> {
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let mut all = app.available_monitors().unwrap_or_default();
    all.sort_by_key(|m| {
        (
            Some(*m.position()) != primary,
            m.position().x,
            m.position().y,
        )
    });
    all
}

pub fn displays(app: &AppHandle) -> Vec<Display> {
    monitors(app)
        .iter()
        .enumerate()
        .map(|(i, m)| Display {
            index: i as u32,
            name: m
                .name()
                .cloned()
                .unwrap_or_else(|| format!("Display {}", i + 1)),
            width: m.size().width,
            height: m.size().height,
            primary: i == 0,
        })
        .collect()
}

/// Open a window for every display that has overlays on (every display while editing, so
/// overlays can be added anywhere); close the others. Overlays on a display that is gone
/// show on the primary one.
pub fn sync(app: &AppHandle, config: &Config, hidden: bool, editing: bool) -> tauri::Result<()> {
    let monitors = monitors(app);
    let wanted: BTreeSet<usize> = config
        .active()
        .overlays
        .values()
        .filter(|o| o.on)
        .map(|o| (o.display as usize).min(monitors.len().saturating_sub(1)))
        .chain(if editing { 0..monitors.len() } else { 0..0 })
        .collect();
    for (label, w) in app.webview_windows() {
        if let Some(i) = label
            .strip_prefix(PREFIX)
            .and_then(|i| i.parse::<usize>().ok())
            && !wanted.contains(&i)
        {
            w.close()?;
        }
    }
    for i in wanted {
        let Some(m) = monitors.get(i) else { continue };
        open(app, i, m, hidden, editing)?;
    }
    Ok(())
}

fn open(app: &AppHandle, i: usize, m: &Monitor, hidden: bool, editing: bool) -> tauri::Result<()> {
    let label = format!("{PREFIX}{i}");
    let window = match app.get_webview_window(&label) {
        Some(w) => w,
        None => WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App(format!("overlay.html?display={i}").into()),
        )
        .title("OpenSimHUD overlay")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?,
    };
    let (pos, size) = (*m.position(), *m.size());
    window.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    window.set_size(PhysicalSize::new(size.width, size.height))?;
    // mouse and keyboard go straight to the game underneath, except while editing
    window.set_ignore_cursor_events(!editing)?;
    if hidden {
        window.hide()?;
    } else {
        window.show()?;
    }
    Ok(())
}

/// Let the overlay windows take the mouse (edit mode) or pass it through to the game.
pub fn set_editing(app: &AppHandle, editing: bool) -> tauri::Result<()> {
    for (label, w) in app.webview_windows() {
        if label.starts_with(PREFIX) {
            w.set_ignore_cursor_events(!editing)?;
            if editing {
                w.show()?;
                w.set_focus()?;
            }
        }
    }
    app.emit("edit-mode", editing)
}
