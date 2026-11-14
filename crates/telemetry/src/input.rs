//! What an adapter reports: raw, game-normalized signals.
//!
//! Adapters keep one [`Input`] up to date; [`crate::core::Session`] reads it after every poll
//! and derives the overlay [`crate::model::State`] (order, gaps, lap history, log, sectors,
//! delta, fuel figures, best laps, pit stops). Anything a game reports directly (e.g. ACC's
//! race position) goes here; anything that can be computed from these signals does not.

use crate::model::{Flag, PitWindow, SessionKind, Track, Tyre, Weather};

#[derive(Debug, Clone)]
pub struct Input {
    /// Sim time in seconds; must only move forward.
    pub t: f64,
    pub track: Track,
    pub session: SessionInput,
    pub weather: Weather,
    pub player: PlayerInput,
    pub cars: Vec<CarInput>,
    /// Classes in display order.
    pub classes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionInput {
    pub kind: SessionKind,
    pub name: String,
    pub duration: f64,
    pub elapsed: f64,
    /// Direct game-reported session countdown, if available.
    pub time_remaining: Option<f64>,
    /// Direct game-reported laps remaining, including the current partial lap.
    pub lap_remaining: Option<f64>,
    pub flag: Flag,
    pub yellow_sector: u8,
    pub pit_window: Option<PitWindow>,
    /// Mandatory pit stops the player still has to make; `None` when the game doesn't say.
    pub mandatory_stops_left: Option<u32>,
    pub time_of_day: f64,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerInput {
    /// Index of the player's car in `Input::cars`.
    pub car: usize,
    pub speed: f32,
    pub gear: i8,
    pub rpm: f32,
    pub max_rpm: f32,
    pub shift_rpm: f32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steer: f32,
    pub abs_active: bool,
    pub tc_active: bool,
    pub tc: u8,
    pub tc2: u8,
    pub abs: u8,
    pub bb: f32,
    pub map: u8,
    pub fuel: f32,
    pub fuel_capacity: f32,
    /// Game-reported average use per lap, when available.
    pub fuel_per_lap: f32,
    /// Game-reported fuel range at the current pace, when available.
    pub fuel_laps_left: f32,
    pub tyres: Vec<Tyre>,
    /// Running time of the current lap.
    pub lap_time: f64,
    /// Current best valid lap time in seconds, reported by the game when available.
    pub best_lap: f64,
    /// 0-based sector the player is in.
    pub sector: usize,
    /// False once the current lap is invalid. Core latches it per lap.
    pub lap_valid: bool,
    /// The game's own live delta to the best lap, seconds, when it has one. Core uses it
    /// instead of its own so the overlay matches the in-game delta.
    pub delta: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct CarInput {
    pub idx: usize,
    /// The game's own id for the car, unique and stable while it stays in the session, so
    /// core can keep a car's history when others join or leave and the indices shift.
    pub id: u32,
    pub number: u32,
    pub driver: String,
    pub short: String,
    pub team: String,
    pub model: String,
    pub cls: String,
    pub cat: String,
    pub nat: String,
    pub is_player: bool,
    /// Completed laps and fraction of the current lap.
    pub laps: u32,
    pub pct: f64,
    /// m/s.
    pub speed: f32,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    /// `x` / `y` hold a real position (the game has reported where the car is).
    pub located: bool,
    /// Position as reported by the game; `None` lets core rank by distance.
    pub position: Option<u32>,
    /// Time of the most recently completed lap; must be set when `laps` increments.
    pub last_lap: f64,
    pub in_pit: bool,
    pub penalty: String,
}
