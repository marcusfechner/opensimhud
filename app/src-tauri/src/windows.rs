//! Overlay windows: one transparent, click-through, always-on-top window per monitor that
//! hosts every overlay. Milestone 3 opens one on the primary monitor.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub fn open_overlay(app: &AppHandle) -> tauri::Result<()> {
    let Some(monitor) = app.primary_monitor()? else {
        return Ok(());
    };
    let label = "overlay-0";
    if app.get_webview_window(label).is_some() {
        return Ok(());
    }
    let pos = *monitor.position();
    let size = *monitor.size();
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay.html".into()))
        .title("OpenSimHUD")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    window.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    window.set_size(PhysicalSize::new(size.width, size.height))?;
    // mouse and keyboard go straight to the game underneath
    window.set_ignore_cursor_events(true)?;
    window.show()?;
    Ok(())
}
