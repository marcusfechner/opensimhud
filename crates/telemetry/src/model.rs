//! Normalized telemetry model: what every overlay reads, whatever the game.
//!
//! The shape mirrors `HUD.state` from the mockups (`mockups/shared/mock-session.js`).
//! Adapters never fill this directly: they report [`crate::input`] frames and
//! [`crate::core::Session`] derives everything here. Times are seconds, distances meters,
//! temperatures °C, pressures psi, speeds km/h unless a field says otherwise.
//!
//! TypeScript bindings are generated into `app/src/types/telemetry/` by `cargo test`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which game a snapshot comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Game {
    Demo,
    Acc,
}

/// Connection state of an adapter, shown in the app sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Connection {
    /// The game isn't running or isn't sending data.
    WaitingForGame,
    /// Data is flowing.
    Connected,
}

/// What a game can provide. Overlays hide or adapt what isn't available
/// (e.g. ACC only reports one core temperature per tyre).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SessionKind {
    Practice,
    Qualifying,
    Race,
    Hotlap,
}

/// The flag currently shown to the player.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Flag {
    #[default]
    None,
    Green,
    Yellow,
    Blue,
    White,
    /// Black and white diagonal: unsportsmanlike / track limits warning.
    Blackwhite,
    Black,
    Chequered,
}

/// Sector result colors: session best (purple), personal best (green) or slower (yellow).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SectorState {
    /// Not driven yet on this lap.
    #[default]
    #[serde(rename = "")]
    Pending,
    Sb,
    Pb,
    Slower,
}

/// The whole state the overlays render.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct State {
    pub game: Game,
    pub connection: Connection,
    pub capabilities: Capabilities,
    /// Sim time in seconds since the adapter connected.
    pub t: f64,
    pub track: Track,
    pub session: SessionInfo,
    pub weather: Weather,
    pub player: Player,
    /// Every car, indexed by `Car::idx` (stable for the session).
    pub cars: Vec<Car>,
    /// Car indices sorted by overall position.
    pub order: Vec<usize>,
    /// Classes in display order, e.g. `["GT3", "GT4"]`.
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Track {
    pub name: String,
    /// Lap length in meters.
    pub length: f64,
    /// Lap fraction where each sector starts; the first is always 0.
    pub sectors: Vec<f64>,
    /// Outline sampled at equal distances (meters, world space). Empty until known
    /// (ACC has no track map; it's learned from a lap).
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BestLap {
    /// Lap time in seconds, 0 when nobody has set one.
    pub time: f64,
    /// Index of the car holding it.
    pub car: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PitWindow {
    /// Session time (s since start) when the window opens / closes.
    pub open: f64,
    pub close: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionInfo {
    pub kind: SessionKind,
    pub name: String,
    /// Planned duration in seconds.
    pub duration: f64,
    pub elapsed: f64,
    pub time_remaining: f64,
    /// Laps the player still has to drive, fractional (current lap included).
    pub lap_remaining: f64,
    pub flag: Flag,
    /// 1-based sector with a local yellow, 0 when none.
    pub yellow_sector: u8,
    pub pit_window: Option<PitWindow>,
    /// Mandatory pit stops the player still has to make; `None` when the game doesn't say.
    pub mandatory_stops_left: Option<u32>,
    /// Local time of day at the track, seconds since midnight.
    pub time_of_day: f64,
    /// Best lap per class, keyed by class name.
    pub best: std::collections::BTreeMap<String, BestLap>,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Weather {
    pub air: f32,
    pub track: f32,
    /// Rain intensity 0..1 now, in 10 and in 30 minutes.
    pub rain_now: f32,
    pub rain10: f32,
    pub rain30: f32,
    /// Standing water 0..1.
    pub wetness: f32,
    pub grip: String,
    /// km/h and degrees (direction the wind comes from).
    pub wind_speed: f32,
    pub wind_dir: f32,
    pub clouds: f32,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Fuel {
    pub liters: f32,
    pub capacity: f32,
    /// Average use per lap over the last laps.
    pub per_lap: f32,
    pub laps_left: f32,
    /// Liters needed to the finish.
    pub to_finish: f32,
    /// Liters to add at the next stop (with half a lap margin).
    pub to_add: f32,
    pub last_lap_use: f32,
    /// Burnt so far on the current lap.
    pub used_this_lap: f32,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Tyre {
    /// "FL", "FR", "RL", "RR".
    pub name: String,
    pub pressure: f32,
    pub core: f32,
    /// Inner / middle / outer surface temperatures (only with `Capabilities::tyre_zones`).
    pub i: Option<f32>,
    pub m: Option<f32>,
    pub o: Option<f32>,
    /// Remaining tread 0..1 (only with `Capabilities::tyre_wear`).
    pub wear: Option<f32>,
    pub brake: f32,
    pub compound: String,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Lap {
    /// Running time of the current lap.
    pub current: f64,
    pub last: f64,
    /// Best valid lap.
    pub best: f64,
    /// Current lap against the best / last lap at the same track position.
    pub delta: f64,
    pub delta_last: f64,
    pub predicted: f64,
    /// 0-based sector the player is in.
    pub sector: usize,
    pub sectors: Vec<f64>,
    pub sector_state: Vec<SectorState>,
    pub best_sectors: Vec<f64>,
    pub last_sectors: Vec<f64>,
    /// Time gained (negative) or lost per sector against the best lap: final for completed
    /// sectors, running for the current one, `None` ahead of it or without a best lap.
    /// They add up to `delta`.
    pub sector_deltas: Vec<Option<f64>>,
    /// The completed lap's sector states and deltas, kept for a moment after the line.
    pub last_sector_state: Vec<SectorState>,
    pub last_sector_deltas: Vec<Option<f64>>,
    /// False once the current lap got invalidated (track limits, …).
    pub valid: bool,
}

/// One completed lap of the player.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LapEntry {
    /// 1-based lap number.
    pub lap: u32,
    pub time: f64,
    pub sectors: Vec<f64>,
    pub valid: bool,
    pub fuel: f32,
    /// In-lap or out-lap.
    pub pit: bool,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Player {
    /// Index into `State::cars`.
    pub car: usize,
    pub speed: f32,
    /// -1 reverse, 0 neutral.
    pub gear: i8,
    pub rpm: f32,
    pub max_rpm: f32,
    pub shift_rpm: f32,
    /// Pedals 0..1, steering -1 (full left) .. 1 (full right).
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steer: f32,
    pub abs_active: bool,
    pub tc_active: bool,
    pub tc: u8,
    pub tc2: u8,
    pub abs: u8,
    /// Brake bias as shown in the car, percent front.
    pub bb: f32,
    pub map: u8,
    /// Setting changed most recently ("tc", "abs", "bb", "map", "tc2") and when.
    pub changed: String,
    pub changed_at: f64,
    pub fuel: Fuel,
    pub tyres: Vec<Tyre>,
    pub lap: Lap,
    pub stint_laps: u32,
    pub pit_stops: u32,
    pub log: Vec<LapEntry>,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Car {
    pub idx: usize,
    pub number: u32,
    pub driver: String,
    /// "L. Marchetti".
    pub short: String,
    pub team: String,
    pub model: String,
    pub cls: String,
    /// Driver category: "P", "G", "S", "B" or "" when unknown.
    pub cat: String,
    /// Three-letter nationality code or "".
    pub nat: String,
    pub is_player: bool,

    /// Completed laps and fraction of the current lap.
    pub laps: u32,
    pub pct: f64,
    /// `laps + pct`.
    pub dist: f64,
    pub speed: f32,
    /// World position (meters) and heading (radians).
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    /// `x` / `y` hold a real position; maps leave the car out until then.
    pub located: bool,

    pub pos: u32,
    pub class_pos: u32,
    pub start_pos: u32,
    pub last_lap: f64,
    pub best_lap: f64,
    /// Completed lap times, oldest first (last 80).
    pub lap_times: Vec<f64>,
    /// Lap numbers that were in- or out-laps.
    pub pit_laps: Vec<u32>,

    /// Gap to the class leader and to the class car ahead, seconds.
    pub gap_leader: f64,
    pub gap_ahead: f64,
    pub laps_down: u32,
    pub laps_ahead: u32,
    /// On-track gap to the player (+ ahead) in seconds and meters; laps between the two.
    pub rel_gap: f64,
    pub rel_meters: f64,
    pub rel_laps: i32,
    /// Sideways offset from the player in meters, + = to the player's right.
    pub rel_lateral: f64,

    pub in_pit: bool,
    pub out_lap: bool,
    pub pit_stops: u32,
    /// Pending penalty, e.g. "+5s", "DT", or "".
    pub penalty: String,
}
