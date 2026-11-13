//! Assetto Corsa EVO shared-memory decoding.
//!
//! The physics and graphics page layouts follow the EVO shared-memory guide. Inputs and
//! world positions were checked against 0.9.1 driving captures; session data still needs work.

pub mod live;
pub mod recording;

use crate::adapter::{AdapterEvent, GameAdapter};
use crate::adapters::acc::learn::Learner;
use crate::input::{CarInput, Input, PlayerInput, SessionInput};
use crate::model::{Capabilities, Connection, Flag, Game, SessionKind, Track, Weather};
use std::path::PathBuf;
use std::time::Instant;

/// Live EVO integration. Player inputs and world positions are supported initially.
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
                track: Track::default(),
                session: SessionInput {
                    kind: SessionKind::Practice,
                    name: String::new(),
                    duration: 0.0,
                    elapsed: 0.0,
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
            learner: Learner::new(1),
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
            self.learner = Learner::new(1);
            self.last_laps = None;
            self.last_position = None;
            self.lap_distance = 0.0;
            self.input.track = Track {
                name: s.track.clone(),
                length: s.track_length as f64,
                sectors: vec![0.0],
                ..Track::default()
            };
            if let Some(dir) = &self.track_cache
                && let Some((track, valid)) = Learner::load(dir, &self.input.track.name)
            {
                self.learner.adopt(&track, valid);
                self.input.track = track;
            }
        }
        self.input.track.length = s.track_length as f64;
        self.input.session.name = s.session_name;
        self.input.player.throttle = p.throttle;
        self.input.player.brake = p.brake;
        self.input.player.gear = (p.gear - 1) as i8;
        self.input.player.rpm = p.rpm as f32;
        self.input.player.speed = p.speed_kmh;
        self.input.player.steer = g.steering;
        self.input.player.clutch = g.clutch;
        self.input.player.lap_time = g.lap_time_ms.max(0) as f64 / 1000.0;
        self.input.player.lap_valid = g.is_valid_lap;

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
                self.learner = Learner::new(1);
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
            self.input.cars.push(CarInput {
                idx: slot,
                id: slot as u32 + 1,
                is_player,
                x,
                y: z,
                heading: if is_player {
                    p.heading + std::f32::consts::FRAC_PI_2
                } else {
                    0.0
                },
                speed: if is_player { p.speed_kmh / 3.6 } else { 0.0 },
                laps: if is_player { laps } else { 0 },
                located,
                in_pit: is_player && g.is_in_pit_lane,
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
            let pct = (self.lap_distance / s.track_length.max(1.0) as f64).clamp(0.0, 0.999_999);
            self.input.cars[0].pct = pct;
            self.learner
                .sample(pct, x, z, 0, !g.is_in_pit_lane, g.is_valid_lap);
        } else {
            self.last_position = None;
        }
    }
}

impl GameAdapter for Evo {
    fn game(&self) -> Game {
        Game::Evo
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
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

/// EVO graphics page fields used for the player car and world-position map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Graphics {
    pub packet: i32,
    pub status: i32,
    pub clutch: f32,
    pub steering: f32,
    pub lap_time_ms: i32,
    pub fuel_liters: f32,
    pub total_lap_count: i32,
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
        let g = Self {
            packet: i(bytes, 0)?,
            status: i(bytes, 4)?,
            clutch: f(bytes, 88)?,
            steering: f(bytes, 92)?,
            lap_time_ms: i(bytes, 188)?,
            fuel_liters: f(bytes, 196)?,
            total_lap_count: i(bytes, 2384).unwrap_or(0),
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
            || !(-1_000_000..=1_000_000).contains(&g.lap_time_ms)
            || !(-1..=1_000_000).contains(&g.total_lap_count)
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
    pub steer: f32,
    pub speed_kmh: f32,
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
            steer: float(bytes, 24)?,
            speed_kmh: float(bytes, 28)?,
            heading: float(bytes, 208).unwrap_or(0.0),
        };
        if !(0.0..=1.0).contains(&p.throttle)
            || !(0.0..=1.0).contains(&p.brake)
            || !(-100.0..=100.0).contains(&p.steer)
            || !(0.0..=1000.0).contains(&p.speed_kmh)
            || !(0..=20).contains(&p.gear)
            || !(0..=50000).contains(&p.rpm)
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
