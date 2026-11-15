//! Assetto Corsa EVO shared-memory decoding.
//!
//! The physics and graphics page layouts follow the EVO shared-memory guide. Inputs,
//! lap timing, player data, and world positions were checked against 0.9.1 driving captures.

pub mod live;
pub mod recording;

use crate::adapter::{AdapterEvent, GameAdapter};
use crate::adapters::acc::learn::Learner;
use crate::input::{CarInput, Input, PlayerInput, SessionInput};
use crate::model::{Capabilities, Connection, Flag, Game, SessionKind, Track, Weather};
use std::path::PathBuf;
use std::time::Instant;

/// EVO does not publish official sector boundaries; use equal thirds of lap distance.
const EVO_SECTORS: [f64; 3] = [0.0, 1.0 / 3.0, 2.0 / 3.0];

fn session_kind(name: &str) -> SessionKind {
    let name = name.to_ascii_lowercase();
    if name.contains("qualif") {
        SessionKind::Qualifying
    } else if name.contains("race") {
        SessionKind::Race
    } else if name.contains("hotlap") || name.contains("time attack") || name.contains("hot stint")
    {
        SessionKind::Hotlap
    } else {
        SessionKind::Practice
    }
}

fn decode_flag(value: i32) -> Flag {
    // The capture and the published enum ordering agree on green=2 and checkered=8.
    match value {
        1 => Flag::White,
        2 => Flag::Green,
        4 => Flag::Blue,
        5 => Flag::Yellow,
        6 => Flag::Black,
        7 => Flag::Blackwhite,
        8 => Flag::Chequered,
        _ => Flag::None,
    }
}

fn project_to_track(track: &Track, x: f32, y: f32) -> Option<f64> {
    if track.x.len() < 3 || track.x.len() != track.y.len() {
        return None;
    }
    let (index, distance_squared) = track
        .x
        .iter()
        .zip(&track.y)
        .enumerate()
        .map(|(index, (&tx, &ty))| {
            let dx = x - tx;
            let dy = y - ty;
            (index, dx * dx + dy * dy)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    // Ignore coordinates far from the circuit; they are not useful for on-track spacing.
    (distance_squared < 100.0 * 100.0).then_some(index as f64 / track.x.len() as f64)
}

/// Live EVO integration. Unpublished per-car timing and identity are intentionally not inferred.
pub struct Evo {
    pages: Option<live::Pages>,
    input: Input,
    started: Instant,
    last_packet: Option<i32>,
    last_change: f64,
    last_open: f64,
    connected: bool,
    learner: Learner,
    track_cache: Option<PathBuf>,
    track_name: String,
    last_laps: Option<u32>,
    last_position: Option<(f32, f32)>,
    lap_distance: f64,
}

impl Default for Evo {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Evo {
    pub fn new(track_cache: Option<PathBuf>) -> Self {
        Self {
            pages: None,
            input: Input {
                t: 0.0,
                track: Track {
                    sectors: EVO_SECTORS.to_vec(),
                    ..Track::default()
                },
                session: SessionInput {
                    kind: SessionKind::Practice,
                    name: String::new(),
                    duration: 0.0,
                    elapsed: 0.0,
                    time_remaining: None,
                    lap_remaining: None,
                    flag: Flag::None,
                    yellow_sector: 0,
                    pit_window: None,
                    mandatory_stops_left: None,
                    time_of_day: 0.0,
                },
                weather: Weather::default(),
                player: PlayerInput::default(),
                cars: vec![CarInput {
                    is_player: true,
                    ..Default::default()
                }],
                classes: vec![],
            },
            started: Instant::now(),
            last_packet: None,
            last_change: 0.0,
            last_open: -1.0,
            connected: false,
            learner: Learner::new(EVO_SECTORS.len()),
            track_cache,
            track_name: String::new(),
            last_laps: None,
            last_position: None,
            lap_distance: 0.0,
        }
    }

    fn apply(&mut self, now: f64, p: Physics, g: Graphics, s: Static) {
        self.input.t = now;
        if self.track_name != s.track {
            self.track_name.clone_from(&s.track);
            self.learner = Learner::new(EVO_SECTORS.len());
            self.last_laps = None;
            self.last_position = None;
            self.lap_distance = 0.0;
            self.input.track = Track {
                name: s.track.clone(),
                length: s.track_length as f64,
                sectors: EVO_SECTORS.to_vec(),
                ..Track::default()
            };
            if let Some(dir) = &self.track_cache
                && let Some((mut track, valid)) = Learner::load(dir, &self.input.track.name)
            {
                track.sectors = EVO_SECTORS.to_vec();
                self.learner.adopt(&track, valid);
                self.input.track = track;
            }
        }
        self.input.track.length = s.track_length as f64;
        self.input.session.kind = session_kind(&s.session_name);
        self.input.session.name.clone_from(&s.session_name);
        self.input.session.time_remaining =
            (g.session_time_left_ms >= 0).then_some(g.session_time_left_ms as f64 / 1000.0);
        self.input.session.flag = decode_flag(g.flag);
        self.input.session.time_of_day = if (0..24).contains(&g.time_of_day[0])
            && (0..60).contains(&g.time_of_day[1])
            && (0..60).contains(&g.time_of_day[2])
        {
            g.time_of_day[0] as f64 * 3600.0
                + g.time_of_day[1] as f64 * 60.0
                + g.time_of_day[2] as f64
        } else {
            0.0
        };
        self.input.player.throttle = p.throttle;
        self.input.player.brake = p.brake;
        self.input.player.gear = (p.gear - 1) as i8;
        self.input.player.rpm = p.rpm as f32;
        self.input.player.max_rpm = p.max_rpm as f32;
        // EVO's 0.9.1 captures confirm redline, but not a distinct shift point.
        self.input.player.shift_rpm = 0.0;
        self.input.player.speed = p.speed_kmh;
        self.input.player.steer = g.steering;
        self.input.player.clutch = g.clutch;
        self.input.player.tc_active = p.tc > 0.0 || p.tc_in_action != 0;
        self.input.player.abs_active = p.abs > 0.0 || p.abs_in_action != 0;
        self.input.player.lap_time = g.lap_time_ms.max(0) as f64 / 1000.0;
        self.input.player.best_lap = g.best_lap_time_ms.max(0) as f64 / 1000.0;
        self.input.player.lap_valid = g.is_valid_lap;
        self.input.player.tc = g.electronics.tc.max(0) as u8;
        self.input.player.tc2 = g.electronics.tc_cut.max(0) as u8;
        self.input.player.abs = g.electronics.abs.max(0) as u8;
        self.input.player.bb = if g.electronics.brake_bias > 0.0 {
            g.electronics.brake_bias * 100.0
        } else {
            0.0
        };
        self.input.player.map = if g.electronics.engine_map >= 0 {
            g.electronics.engine_map.saturating_add(1) as u8
        } else {
            0
        };
        self.input.player.fuel = g.fuel_liters;
        self.input.player.fuel_capacity = g.fuel_capacity;
        self.input.player.fuel_per_lap = g.fuel_per_lap;
        self.input.player.fuel_laps_left = g.fuel_laps_left;
        self.input.player.tyres = g
            .tyres
            .iter()
            .enumerate()
            .map(|(index, raw)| {
                let (inner, outer) = if index % 2 == 0 {
                    (raw.temp_right, raw.temp_left)
                } else {
                    (raw.temp_left, raw.temp_right)
                };
                crate::model::Tyre {
                    name: ["FL", "FR", "RL", "RR"][index].into(),
                    pressure: raw.pressure,
                    core: raw.core,
                    i: Some(inner),
                    m: Some(raw.temp_center),
                    o: Some(outer),
                    wear: None,
                    brake: raw.brake,
                    compound: raw.compound.clone(),
                }
            })
            .collect();

        let laps = g.total_lap_count.max(0) as u32;
        if let Some(previous) = self.last_laps {
            if laps > previous {
                if self.learner.lap_done()
                    && let Some((x, y)) = self.learner.outline.clone()
                {
                    self.input.track.x = x;
                    self.input.track.y = y;
                    self.input.track.sectors.clone_from(&self.learner.sectors);
                    if let Some(dir) = &self.track_cache {
                        let _ = Learner::save(dir, &self.input.track, self.learner.outline_valid);
                    }
                }
                self.lap_distance = 0.0;
                self.last_position = None;
            } else if laps < previous {
                let previous_track = self.input.track.clone();
                let previously_valid = self.learner.outline_valid;
                self.learner = Learner::new(EVO_SECTORS.len());
                if !previous_track.x.is_empty() {
                    self.learner.adopt(&previous_track, previously_valid);
                }
                self.lap_distance = 0.0;
                self.last_position = None;
            }
        }
        self.last_laps = Some(laps);

        let count = usize::from(g.active_cars).max(1);
        self.input.cars.clear();
        for slot in 0..count {
            let [raw_x, _, raw_z] = g.car_coordinates[slot];
            let located = slot < usize::from(g.active_cars)
                && [raw_x, raw_z].into_iter().all(f32::is_finite)
                && raw_x.abs() < 100_000.0
                && raw_z.abs() < 100_000.0;
            let (x, z) = if located { (raw_x, raw_z) } else { (0.0, 0.0) };
            let is_player = slot == 0;
            let pct = if is_player {
                0.0
            } else if located {
                project_to_track(&self.input.track, x, z).unwrap_or(0.0)
            } else {
                0.0
            };
            self.input.cars.push(CarInput {
                idx: slot,
                id: slot as u32 + 1,
                is_player,
                x,
                y: z,
                pct,
                heading: if is_player {
                    p.heading + std::f32::consts::FRAC_PI_2
                } else {
                    0.0
                },
                speed: if is_player { p.speed_kmh / 3.6 } else { 0.0 },
                laps: if is_player { laps } else { 0 },
                position: if is_player && (1..=g.total_drivers).contains(&g.current_position) {
                    Some(g.current_position)
                } else {
                    None
                },
                located,
                in_pit: is_player && g.is_in_pit_lane,
                last_lap: if is_player {
                    g.last_lap_time_ms.max(0) as f64 / 1000.0
                } else {
                    0.0
                },
                ..CarInput::default()
            });
        }
        self.input.player.car = 0;

        let [x, _, z] = g.car_coordinates[0];
        let player_located = self.input.cars[0].located;
        if player_located {
            let position = (x, z);
            if let Some(previous) = self.last_position {
                let distance = (position.0 - previous.0).hypot(position.1 - previous.1) as f64;
                if distance.is_finite() && distance < 100.0 {
                    self.lap_distance += distance;
                }
            }
            self.last_position = Some(position);
            // Once the outline is known, project the player's world position onto it. A
            // distance counter started when the adapter attaches can have an arbitrary
            // origin (for example, the car may be in the pit lane), which offsets every
            // lap-fraction map. Keep the accumulated distance as a fallback before learning.
            let fallback_pct =
                (self.lap_distance / s.track_length.max(1.0) as f64).clamp(0.0, 0.999_999);
            let pct = project_to_track(&self.input.track, x, z).unwrap_or(fallback_pct);
            self.input.cars[0].pct = pct;
            self.input.player.sector =
                ((pct * EVO_SECTORS.len() as f64).floor() as usize).min(EVO_SECTORS.len() - 1);
            self.learner.sample(
                pct,
                x,
                z,
                self.input.player.sector,
                !g.is_in_pit_lane,
                g.is_valid_lap,
            );
        } else {
            self.last_position = None;
        }

        self.input.session.lap_remaining = if self.input.session.kind == SessionKind::Race
            && g.total_session_laps > 0
            && g.current_session_lap > 0
        {
            let laps = g.total_session_laps - g.current_session_lap + 1;
            Some((laps as f64 - self.input.cars[0].pct).max(0.0))
        } else {
            None
        };
    }
}

impl GameAdapter for Evo {
    fn game(&self) -> Game {
        Game::Evo
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tyre_zones: true,
            car_positions: true,
            ..Capabilities::default()
        }
    }
    fn input(&self) -> &Input {
        &self.input
    }
    fn poll(&mut self) -> AdapterEvent {
        let now = self.started.elapsed().as_secs_f64();
        if self.pages.is_none() && now - self.last_open >= 1.0 {
            self.last_open = now;
            self.pages = live::Pages::open();
            self.last_packet = None;
        }
        let raw = self
            .pages
            .as_ref()
            .and_then(|p| Some((p.read(0)?, p.read(1)?, p.read(2)?)));
        let decoded = raw.as_ref().and_then(|(p, g, s)| {
            Some((
                Physics::decode(p)?,
                Graphics::decode(g)?,
                Static::decode(s)?,
            ))
        });
        if let Some((p, _, _)) = &decoded
            && self.last_packet != Some(p.packet)
        {
            self.last_packet = Some(p.packet);
            self.last_change = now;
        }
        let active = decoded.as_ref().is_some_and(|(_, g, _)| g.status == 2)
            && self.last_packet.is_some()
            && now - self.last_change < 3.0;
        if !active && now - self.last_change >= 3.0 {
            self.pages = None;
            self.last_packet = None;
        }
        if active != self.connected {
            self.connected = active;
            return AdapterEvent::Connection(if active {
                Connection::Connected
            } else {
                Connection::WaitingForGame
            });
        }
        let Some((p, g, s)) = decoded.filter(|_| active) else {
            return AdapterEvent::Idle;
        };
        self.apply(now, p, g, s);
        AdapterEvent::Updated
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TyreData {
    pub pressure: f32,
    pub core: f32,
    pub brake: f32,
    pub temp_left: f32,
    pub temp_center: f32,
    pub temp_right: f32,
    pub compound: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Electronics {
    pub tc: i8,
    pub tc_cut: i8,
    pub abs: i8,
    pub brake_bias: f32,
    pub engine_map: i8,
}

/// EVO graphics page fields used by the player, session, tyres, and spatial overlays.
#[derive(Debug, Clone, PartialEq)]
pub struct Graphics {
    pub packet: i32,
    pub status: i32,
    pub clutch: f32,
    pub steering: f32,
    pub lap_time_ms: i32,
    pub fuel_liters: f32,
    pub fuel_capacity: f32,
    pub fuel_per_lap: f32,
    pub fuel_laps_left: f32,
    pub total_lap_count: i32,
    /// Most recently completed lap, in milliseconds.
    pub last_lap_time_ms: i32,
    /// Session best lap, in milliseconds.
    pub best_lap_time_ms: i32,
    pub current_position: u32,
    pub total_drivers: u32,
    pub flag: i32,
    pub time_of_day: [i32; 3],
    pub session_time_left_ms: i32,
    pub total_session_laps: i32,
    pub current_session_lap: i32,
    pub electronics: Electronics,
    /// Corners ordered FL, FR, RL, RR.
    pub tyres: [TyreData; 4],
    pub is_in_pit_lane: bool,
    pub is_valid_lap: bool,
    pub active_cars: u8,
    pub car_coordinates: [[f32; 3]; 60],
}

impl Graphics {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        fn i(bytes: &[u8], at: usize) -> Option<i32> {
            Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        }
        fn f(bytes: &[u8], at: usize) -> Option<f32> {
            Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        }
        fn u(bytes: &[u8], at: usize) -> Option<u32> {
            Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        }
        fn string(bytes: &[u8], at: usize, len: usize) -> String {
            bytes
                .get(at..at + len)
                .map(|value| {
                    String::from_utf8_lossy(value)
                        .trim_matches('\0')
                        .trim()
                        .to_owned()
                })
                .unwrap_or_default()
        }
        fn tyre(bytes: &[u8], at: usize) -> TyreData {
            TyreData {
                pressure: f(bytes, at + 8).unwrap_or(0.0),
                core: f(bytes, at + 12).unwrap_or(0.0),
                brake: f(bytes, at + 16).unwrap_or(0.0),
                temp_left: f(bytes, at + 24).unwrap_or(0.0),
                temp_center: f(bytes, at + 28).unwrap_or(0.0),
                temp_right: f(bytes, at + 32).unwrap_or(0.0),
                compound: string(bytes, at + 36, 33),
            }
        }
        let g = Self {
            packet: i(bytes, 0)?,
            status: i(bytes, 4)?,
            clutch: f(bytes, 88)?,
            steering: f(bytes, 92)?,
            lap_time_ms: i(bytes, 188)?,
            fuel_liters: f(bytes, 196)?,
            fuel_capacity: f(bytes, 3928).unwrap_or(0.0),
            fuel_per_lap: f(bytes, 3856).unwrap_or(0.0),
            fuel_laps_left: f(bytes, 3860).unwrap_or(0.0),
            total_lap_count: i(bytes, 2384).unwrap_or(0),
            last_lap_time_ms: i(bytes, 2396).unwrap_or(0),
            best_lap_time_ms: i(bytes, 2400).unwrap_or(0),
            current_position: u(bytes, 2388).unwrap_or(0),
            total_drivers: u(bytes, 2392).unwrap_or(0),
            flag: i(bytes, 2404).unwrap_or(0),
            time_of_day: [
                i(bytes, 172).unwrap_or(-1),
                i(bytes, 176).unwrap_or(-1),
                i(bytes, 180).unwrap_or(-1),
            ],
            session_time_left_ms: i(bytes, 2524).unwrap_or(-1),
            total_session_laps: i(bytes, 2544).unwrap_or(0),
            current_session_lap: i(bytes, 2548).unwrap_or(0),
            electronics: Electronics {
                tc: bytes.get(1872).copied().unwrap_or(0) as i8,
                tc_cut: bytes.get(1873).copied().unwrap_or(0) as i8,
                abs: bytes.get(1874).copied().unwrap_or(0) as i8,
                brake_bias: f(bytes, 1880).unwrap_or(0.0),
                engine_map: bytes.get(1884).copied().unwrap_or(0) as i8,
            },
            tyres: [220, 476, 732, 988].map(|at| tyre(bytes, at)),
            is_in_pit_lane: bytes.get(3120).is_some_and(|v| *v != 0),
            is_valid_lap: bytes.get(3121).is_some_and(|v| *v != 0),
            active_cars: bytes.get(3852).copied().unwrap_or(0),
            car_coordinates: std::array::from_fn(|slot| {
                let at = 3124 + slot * 12;
                [
                    f(bytes, at).unwrap_or(f32::NAN),
                    f(bytes, at + 4).unwrap_or(f32::NAN),
                    f(bytes, at + 8).unwrap_or(f32::NAN),
                ]
            }),
        };
        if !(0..=3).contains(&g.status)
            || !(0.0..=1.0).contains(&g.clutch)
            || !(-1.0..=1.0).contains(&g.steering)
            || !(0.0..=10000.0).contains(&g.fuel_liters)
            || !(0.0..=10000.0).contains(&g.fuel_capacity)
            || !(0.0..=10000.0).contains(&g.fuel_per_lap)
            || !(0.0..=10000.0).contains(&g.fuel_laps_left)
            || !(-1_000_000..=1_000_000).contains(&g.lap_time_ms)
            || !(-1..=1_000_000).contains(&g.total_lap_count)
            || !(-1..=1_000_000).contains(&g.last_lap_time_ms)
            || !(-1..=1_000_000).contains(&g.best_lap_time_ms)
            || g.current_position > 10000
            || g.total_drivers > 10000
            || g.active_cars > 60
        {
            return None;
        }
        Some(g)
    }
}

/// Session metadata in shared-memory format 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Static {
    pub version: String,
    pub game_version: String,
    pub session_name: String,
    pub track: String,
    pub configuration: String,
    /// Track length in meters.
    pub track_length: f32,
}

impl Static {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        fn string(bytes: &[u8], start: usize, len: usize) -> Option<String> {
            let bytes = bytes.get(start..start + len)?;
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(len);
            Some(String::from_utf8_lossy(&bytes[..end]).trim().to_owned())
        }
        let version = string(bytes, 0, 15)?;
        if version != "1.0" {
            return None;
        }
        let length = f32::from_le_bytes(bytes.get(204..208)?.try_into().ok()?);
        if !(0.0..=100_000.0).contains(&length) {
            return None;
        }
        Some(Self {
            version,
            game_version: string(bytes, 15, 15)?,
            session_name: string(bytes, 36, 33)?,
            track: string(bytes, 136, 33)?,
            configuration: string(bytes, 169, 33)?,
            track_length: length,
        })
    }
}

/// A subset of the EVO physics page, in the units supplied by the game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Physics {
    pub packet: i32,
    pub throttle: f32,
    pub brake: f32,
    /// Raw game gear: 0 reverse, 1 neutral, 2 first.
    pub gear: i32,
    pub rpm: i32,
    pub max_rpm: i32,
    pub steer: f32,
    pub speed_kmh: f32,
    /// 0..1 live TC intervention intensity; positive while TC is active in captured EVO 0.9.1.
    pub tc: f32,
    /// 0..1 live ABS intervention intensity; positive while ABS is active in captured EVO 0.9.1.
    pub abs: f32,
    /// Dedicated 0/1 action flag. This remained zero in the 0.9.1 captures.
    pub tc_in_action: i32,
    /// Dedicated 0/1 action flag. This remained zero in the 0.9.1 captures.
    pub abs_in_action: i32,
    /// Vehicle orientation in radians, as reported by the physics page.
    pub heading: f32,
}

impl Physics {
    /// Decode a snapshot. Reject incomplete pages and invalid values instead of publishing them.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        fn int(bytes: &[u8], at: usize) -> Option<i32> {
            Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        }
        fn float(bytes: &[u8], at: usize) -> Option<f32> {
            Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        }
        let p = Self {
            packet: int(bytes, 0)?,
            throttle: float(bytes, 4)?,
            brake: float(bytes, 8)?,
            gear: int(bytes, 16)?,
            rpm: int(bytes, 20)?,
            max_rpm: int(bytes, 588).unwrap_or(0),
            steer: float(bytes, 24)?,
            speed_kmh: float(bytes, 28)?,
            tc: float(bytes, 204).unwrap_or(0.0),
            abs: float(bytes, 252).unwrap_or(0.0),
            tc_in_action: int(bytes, 672).unwrap_or(0),
            abs_in_action: int(bytes, 676).unwrap_or(0),
            heading: float(bytes, 208).unwrap_or(0.0),
        };
        if !(0.0..=1.0).contains(&p.throttle)
            || !(0.0..=1.0).contains(&p.brake)
            || !(0.0..=1.0).contains(&p.tc)
            || !(0.0..=1.0).contains(&p.abs)
            || !(-100.0..=100.0).contains(&p.steer)
            || !(0.0..=1000.0).contains(&p.speed_kmh)
            || !(0..=20).contains(&p.gear)
            || !(0..=50000).contains(&p.rpm)
            || !(0..=50000).contains(&p.max_rpm)
            || !(-std::f32::consts::TAU..=std::f32::consts::TAU).contains(&p.heading)
        {
            return None;
        }
        Some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::{Graphics, Physics, Static};

    #[test]
    fn graphics_inputs_and_fuel() {
        let mut b = [0u8; 200];
        b[4..8].copy_from_slice(&2i32.to_le_bytes());
        b[88..92].copy_from_slice(&0.5f32.to_le_bytes());
        b[92..96].copy_from_slice(&(-0.3f32).to_le_bytes());
        b[188..192].copy_from_slice(&12345i32.to_le_bytes());
        b[196..200].copy_from_slice(&29.8f32.to_le_bytes());
        let g = Graphics::decode(&b).unwrap();
        assert_eq!(
            (g.clutch, g.steering, g.lap_time_ms, g.fuel_liters),
            (0.5, -0.3, 12345, 29.8)
        );
        assert!(Graphics::decode(&b[..199]).is_none());
    }

    #[test]
    fn static_version_and_track() {
        let mut b = [0u8; 208];
        b[..3].copy_from_slice(b"1.0");
        b[15..20].copy_from_slice(b"0.9.1");
        b[36..44].copy_from_slice(b"Practice");
        b[136..139].copy_from_slice(b"Spa");
        b[169..171].copy_from_slice(b"GP");
        b[204..208].copy_from_slice(&7004f32.to_le_bytes());
        let s = Static::decode(&b).unwrap();
        assert_eq!(
            (
                &*s.game_version,
                &*s.session_name,
                &*s.track,
                &*s.configuration
            ),
            ("0.9.1", "Practice", "Spa", "GP")
        );
        assert_eq!(s.track_length, 7004.0);
        b[2] = b'1';
        assert!(Static::decode(&b).is_none());
    }

    #[test]
    fn decode_player_inputs_and_reject_bad_pages() {
        let mut b = [0u8; 32];
        b[0..4].copy_from_slice(&42i32.to_le_bytes());
        b[4..8].copy_from_slice(&0.75f32.to_le_bytes());
        b[8..12].copy_from_slice(&0.25f32.to_le_bytes());
        b[16..20].copy_from_slice(&2i32.to_le_bytes());
        b[20..24].copy_from_slice(&3500i32.to_le_bytes());
        b[24..28].copy_from_slice(&(-0.2f32).to_le_bytes());
        b[28..32].copy_from_slice(&50.0f32.to_le_bytes());
        let p = Physics::decode(&b).unwrap();
        assert_eq!((p.packet, p.gear, p.rpm), (42, 2, 3500));
        assert_eq!(
            (p.throttle, p.brake, p.steer, p.speed_kmh),
            (0.75, 0.25, -0.2, 50.0)
        );
        assert!(Physics::decode(&b[..31]).is_none());
        b[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Physics::decode(&b).is_none());
    }
}
