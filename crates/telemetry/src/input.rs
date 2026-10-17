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
    pub flag: Flag,
    pub yellow_sector: u8,
    pub pit_window: Option<PitWindow>,
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
    pub tyres: Vec<Tyre>,
    /// Running time of the current lap.
    pub lap_time: f64,
    /// 0-based sector the player is in.
    pub sector: usize,
    /// False once the current lap is invalid. Core latches it per lap.
    pub lap_valid: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CarInput {
    pub idx: usize,
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
    /// Position as reported by the game; `None` lets core rank by distance.
    pub position: Option<u32>,
    /// Time of the most recently completed lap; must be set when `laps` increments.
    pub last_lap: f64,
    pub in_pit: bool,
    pub penalty: String,
}
