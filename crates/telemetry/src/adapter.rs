//! The contract every game integration implements.

use serde::Serialize;

use crate::model::{Connection, Game};

/// What a game can provide. Overlays hide or adapt what isn't available
/// (e.g. ACC only reports one core temperature per tyre).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Inner / middle / outer tyre temperatures.
    pub tyre_zones: bool,
    /// Tyre wear as a fraction.
    pub tyre_wear: bool,
    /// Driver category (ACC: Platinum / Gold / Silver / Bronze).
    pub driver_category: bool,
    /// Lap times for every car, not just the player.
    pub per_car_laps: bool,
    /// World positions for every car (radar, maps).
    pub car_positions: bool,
}

/// Something an adapter observed since the last poll.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterEvent {
    /// Nothing new.
    Idle,
    /// The connection state changed.
    Connection(Connection),
    /// New data was applied to the adapter's state.
    Updated,
}

/// A source of telemetry for one game.
pub trait GameAdapter {
    fn game(&self) -> Game;
    fn capabilities(&self) -> Capabilities;
    /// Read whatever is new; never blocks for long.
    fn poll(&mut self) -> AdapterEvent;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Idle;
    impl GameAdapter for Idle {
        fn game(&self) -> Game {
            Game::Demo
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn poll(&mut self) -> AdapterEvent {
            AdapterEvent::Idle
        }
    }

    #[test]
    fn adapters_are_object_safe() {
        let mut a: Box<dyn GameAdapter> = Box::new(Idle);
        assert_eq!(a.poll(), AdapterEvent::Idle);
        assert_eq!(a.game(), Game::Demo);
    }
}
