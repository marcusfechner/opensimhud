//! The contract every game integration implements.

use crate::input::Input;
use crate::model::{Capabilities, Connection, Game};

/// Something an adapter observed during a poll.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterEvent {
    /// Nothing new.
    Idle,
    /// The connection state changed.
    Connection(Connection),
    /// `input()` holds new data.
    Updated,
}

/// A source of telemetry for one game.
pub trait GameAdapter: Send {
    fn game(&self) -> Game;
    fn capabilities(&self) -> Capabilities;
    /// Read whatever is new; never blocks for long.
    fn poll(&mut self) -> AdapterEvent;
    /// The latest signals. Only meaningful after a poll returned `Updated`.
    fn input(&self) -> &Input;
}
