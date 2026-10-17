//! Normalized telemetry model shared by every game adapter and the overlays.
//!
//! Mirrors `HUD.state` from the mockups (`mockups/shared/mock-session.js`); the full model
//! lands with the demo adapter.

use serde::Serialize;

/// Which game a snapshot comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Game {
    Demo,
    Acc,
}

/// Connection state of an adapter, shown in the app sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    /// The game isn't running or isn't sending data.
    WaitingForGame,
    /// Data is flowing.
    Connected,
}
