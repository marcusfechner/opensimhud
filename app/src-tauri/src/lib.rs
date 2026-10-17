//! Tauri shell: windows, config, hotkeys and the telemetry transport live here;
//! game data handling lives in the `telemetry` crate.

mod transport;
mod windows;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let hub = transport::Hub::default();
    transport::start(hub.clone());
    tauri::Builder::default()
        .manage(hub)
        .invoke_handler(tauri::generate_handler![transport::telemetry_subscribe])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                window.state::<transport::Hub>().unsubscribe(window.label());
            }
        })
        .setup(|app| {
            windows::open_overlay(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
