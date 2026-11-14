//! Assetto Corsa Competizione adapter.
//!
//! Two feeds are merged into one [`Input`]:
//! - shared memory (Windows): the player's car at the game's update rate — inputs, lap
//!   timing, fuel, tyres, flags, weather, plus every car's world position;
//! - the Broadcasting API (UDP): every car's entry, position, laps and lap times (~10 Hz).
//!
//! Raw data comes from a [`Source`]: the live game ([`live::Live`]) or a recording
//! ([`recording::Replay`]); the decoding below is the same for both, so recordings made on
//! Windows replay anywhere.

pub mod broadcast;
pub mod cars;
pub mod learn;
pub mod live;
pub mod pages;
pub mod recording;
pub mod setup;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::adapter::{AdapterEvent, GameAdapter};
use crate::input::{CarInput, Input, PlayerInput, SessionInput};
use crate::model::{
    Capabilities, Connection, Flag, Game, PitWindow, SessionKind, Track, Tyre, Weather,
};
use broadcast::{CarUpdate, EntryCar, Message};
use learn::Learner;
use pages::{Graphics, Physics, Static, wstr};

/// One piece of raw game data, as read or recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Physics(Vec<u8>),
    Graphics(Vec<u8>),
    Static(Vec<u8>),
    Udp(Vec<u8>),
}

/// Where raw data comes from: the running game or a recording.
pub trait Source: Send {
    /// Append everything that arrived since the last call, in order.
    fn drain(&mut self, out: &mut Vec<Raw>);
    /// Send a datagram to ACC's broadcasting port (ignored when replaying).
    fn send(&mut self, datagram: &[u8]);
    /// Seconds since the source started (recording time when replaying).
    fn clock(&self) -> f64;
    /// Shared memory is open (live) / data remains (replay).
    fn connected(&self) -> bool;
}

/// Settings from `broadcasting.json` plus where to cache learned tracks.
#[derive(Debug, Clone)]
pub struct Options {
    pub password: String,
    /// How often ACC sends car updates, milliseconds.
    pub update_ms: u32,
    pub track_cache: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            password: setup::DEFAULT_PASSWORD.into(),
            update_ms: 100,
            track_cache: None,
        }
    }
}

/// Graphics status values.
const STATUS_OFF: i32 = 0;
/// ACC reports neutral for ~70-80 ms during every shift; a neutral this short while moving
/// keeps showing the previous gear.
const SHIFT_NEUTRAL: f64 = 0.15;
/// Shared memory that stops updating this long is treated as a closed game.
const STALE_AFTER: f64 = 3.0;
/// ACC can take several seconds to answer a registration while a session loads, and it
/// accepts every request it queued: retrying sooner leaves duplicate connections.
const REGISTER_EVERY: f64 = 10.0;
/// No broadcast data this long while the game is live → register again (game restarted).
const BROADCAST_TIMEOUT: f64 = 10.0;
/// A car without broadcast updates this long (while others get theirs) has left the server.
/// Cars normally get one every second or faster.
const CAR_GONE: f64 = 10.0;

struct CarState {
    id: u16,
    entry: Option<EntryCar>,
    update: Option<CarUpdate>,
    /// Clock time of the last broadcast update, for extrapolating the position.
    updated_at: f64,
    last_lap: f64,
    /// Last world position used to derive the heading.
    prev: Option<(f32, f32)>,
    heading: f32,
}

pub struct Acc<S: Source> {
    source: S,
    opts: Options,
    raw: Vec<Raw>,
    input: Input,
    physics: Option<Physics>,
    graphics: Option<Graphics>,
    statics: Option<Static>,
    last_physics_at: f64,
    last_physics_id: i32,
    connection: Option<i32>,
    last_register: f64,
    last_broadcast: f64,
    last_entry_request: f64,
    realtime: Option<broadcast::RealtimeUpdate>,
    track_meters: f64,
    cars: Vec<CarState>,
    by_id: HashMap<u16, usize>,
    learner: Learner,
    learned_for: String,
    status: Connection,
    player_laps: Option<u32>,
    first_remaining: Option<f64>,
    /// Gear shown before the current neutral, and since when it is neutral.
    gear: i8,
    neutral_since: Option<f64>,
    /// The last delta ACC reported with a lap estimate.
    delta: Option<f64>,
}

impl<S: Source> Acc<S> {
    pub fn new(source: S, opts: Options) -> Self {
        Self {
            source,
            opts,
            raw: Vec::new(),
            input: empty_input(),
            physics: None,
            graphics: None,
            statics: None,
            last_physics_at: f64::NEG_INFINITY,
            last_physics_id: i32::MIN,
            connection: None,
            last_register: f64::NEG_INFINITY,
            last_broadcast: f64::NEG_INFINITY,
            last_entry_request: f64::NEG_INFINITY,
            realtime: None,
            track_meters: 0.0,
            cars: Vec::new(),
            by_id: HashMap::new(),
            learner: Learner::new(3),
            learned_for: String::new(),
            status: Connection::WaitingForGame,
            player_laps: None,
            first_remaining: None,
            gear: 0,
            neutral_since: None,
            delta: None,
        }
    }

    pub fn source(&self) -> &S {
        &self.source
    }
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    /// For status output: car count from the broadcast feed, and whether it is registered.
    pub fn broadcast_cars(&self) -> usize {
        self.cars.iter().filter(|c| c.entry.is_some()).count()
    }
    pub fn broadcast_registered(&self) -> bool {
        self.connection.is_some()
    }
    pub fn static_page(&self) -> Option<&Static> {
        self.statics.as_ref()
    }
    pub fn graphics_page(&self) -> Option<&Graphics> {
        self.graphics.as_ref()
    }
    pub fn physics_page(&self) -> Option<&Physics> {
        self.physics.as_ref()
    }

    fn handle(&mut self, raw: Raw, now: f64) {
        match raw {
            Raw::Physics(b) => {
                if let Some(p) = Physics::from_bytes(&b) {
                    if p.packet_id != self.last_physics_id {
                        self.last_physics_id = p.packet_id;
                        self.last_physics_at = now;
                    }
                    self.physics = Some(p);
                }
            }
            Raw::Graphics(b) => self.graphics = Graphics::from_bytes(&b).or(self.graphics),
            Raw::Static(b) => self.statics = Static::from_bytes(&b).or(self.statics),
            Raw::Udp(b) => {
                if let Ok(msg) = broadcast::decode(&b) {
                    self.last_broadcast = now;
                    self.on_message(msg, now);
                }
            }
        }
    }

    fn on_message(&mut self, msg: Message, now: f64) {
        match msg {
            Message::Registration {
                connection_id,
                success,
                ..
            } => {
                if !success {
                    return;
                }
                if self.connection.is_some() {
                    // a late answer to an earlier request: keep one connection
                    self.source.send(&broadcast::unregister(connection_id));
                } else {
                    self.connection = Some(connection_id);
                    self.source
                        .send(&broadcast::request_entry_list(connection_id));
                    self.source
                        .send(&broadcast::request_track_data(connection_id));
                    self.last_entry_request = now;
                }
            }
            Message::Realtime(u) => {
                // a new session: forget lap bookkeeping tied to the old one
                if self
                    .realtime
                    .as_ref()
                    .is_some_and(|r| r.session_index != u.session_index)
                {
                    self.cars.clear();
                    self.by_id.clear();
                    self.first_remaining = None;
                    self.request_entries(now, true);
                }
                self.realtime = Some(u);
            }
            Message::Car(u) => {
                let i = match self.by_id.get(&u.car) {
                    Some(&i) => i,
                    None => {
                        self.request_entries(now, false);
                        self.add_car(u.car)
                    }
                };
                let c = &mut self.cars[i];
                if let Some(t) = u.last_lap.time_ms {
                    c.last_lap = t as f64 / 1000.0;
                }
                c.update = Some(u);
                c.updated_at = now;
            }
            Message::EntryList(ids) => {
                // the list is complete: whoever isn't on it has left
                let player = self.graphics.map(|g| g.player_car_id);
                self.remove_cars(|c| !ids.contains(&c.id) && Some(c.id as i32) != player);
                for id in ids {
                    if !self.by_id.contains_key(&id) {
                        self.add_car(id);
                    }
                }
            }
            Message::EntryCar(e) => {
                let i = match self.by_id.get(&e.car) {
                    Some(&i) => i,
                    None => self.add_car(e.car),
                };
                self.cars[i].entry = Some(e);
            }
            Message::Track(t) => self.track_meters = t.meters as f64,
            Message::Event(_) => {}
        }
    }

    fn add_car(&mut self, id: u16) -> usize {
        self.cars.push(CarState {
            id,
            entry: None,
            update: None,
            updated_at: 0.0,
            last_lap: 0.0,
            prev: None,
            heading: 0.0,
        });
        let i = self.cars.len() - 1;
        self.by_id.insert(id, i);
        i
    }

    fn remove_cars(&mut self, gone: impl Fn(&CarState) -> bool) {
        if !self.cars.iter().any(&gone) {
            return;
        }
        self.cars.retain(|c| !gone(c));
        self.by_id = self
            .cars
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        // the inputs were filled per index; start them over for the new order
        self.input.cars.clear();
    }

    /// Back from the menus: another session, perhaps on another server. Forget its cars.
    fn forget_session(&mut self, now: f64) {
        self.cars.clear();
        self.by_id.clear();
        self.input.cars.clear();
        self.realtime = None;
        self.first_remaining = None;
        self.player_laps = None;
        self.delta = None;
        self.request_entries(now, true);
    }

    fn request_entries(&mut self, now: f64, force: bool) {
        if let Some(conn) = self.connection
            && (force || now - self.last_entry_request > 1.0)
        {
            self.last_entry_request = now;
            self.source.send(&broadcast::request_entry_list(conn));
        }
    }

    fn keep_registered(&mut self, now: f64, live: bool) {
        // ACC sends nothing in menus and while loading, so only silence during a session
        // means the connection is gone (the game restarted)
        let silent = now - self.last_broadcast.max(self.last_register) > BROADCAST_TIMEOUT;
        if live
            && silent
            && let Some(old) = self.connection.take()
        {
            self.source.send(&broadcast::unregister(old));
        }
        if self.connection.is_none() && now - self.last_register >= REGISTER_EVERY {
            self.last_register = now;
            self.source.send(&broadcast::register(
                "OpenSimHUD",
                &self.opts.password,
                self.opts.update_ms,
                "",
            ));
        }
    }

    /// Rebuild the adapter input from the latest pages and broadcast state.
    fn build(&mut self, now: f64) {
        let (Some(p), Some(g)) = (self.physics, self.graphics) else {
            return;
        };
        let st = self.statics;
        let player_id = g.player_car_id.clamp(0, u16::MAX as i32) as u16;
        if now - self.last_broadcast < 1.0 {
            self.remove_cars(|c| {
                c.id != player_id && c.update.is_some() && now - c.updated_at > CAR_GONE
            });
        }
        if !self.by_id.contains_key(&player_id) {
            // no broadcast (yet): at least the player's own car
            self.add_car(player_id);
        }
        let inp = &mut self.input;
        inp.t = now;

        // ---------- track ----------
        let track_name = st.map(|s| wstr(&s.track)).unwrap_or_default();
        let sector_count = st.map_or(3, |s| s.sector_count.clamp(1, 10) as usize);
        if track_name != self.learned_for {
            self.learned_for = track_name.clone();
            self.learner = Learner::new(sector_count);
            inp.track = Track {
                name: pretty(&track_name),
                length: 0.0,
                sectors: self.learner.sectors.clone(),
                x: vec![],
                y: vec![],
            };
            if let Some(dir) = &self.opts.track_cache
                && let Some((t, valid)) = Learner::load(dir, &inp.track.name)
            {
                self.learner.adopt(&t, valid);
                inp.track = t;
            }
        }
        if self.track_meters > 0.0 {
            inp.track.length = self.track_meters;
        } else if let Some(s) = st
            && s.track_spline_length > 0.0
        {
            inp.track.length = s.track_spline_length as f64;
        }
        if inp.track.x.is_empty() && inp.track.sectors != self.learner.sectors {
            inp.track.sectors.clone_from(&self.learner.sectors);
        }

        // ---------- session ----------
        // sessions without a time limit report -1 left (and -1 elapsed over broadcast)
        let timed = g.session_time_left >= 0.0;
        let remaining = (g.session_time_left as f64 / 1000.0).max(0.0);
        let elapsed = match &self.realtime {
            Some(r) if r.session_time >= 0.0 => r.session_time as f64 / 1000.0,
            _ => {
                let first =
                    *self
                        .first_remaining
                        .get_or_insert(if timed { remaining } else { -now });
                if timed {
                    (first - remaining).max(0.0)
                } else {
                    now + first
                }
            }
        };
        let s = &mut inp.session;
        s.kind = match g.session {
            0 => SessionKind::Practice,
            1 | 8 => SessionKind::Qualifying,
            2 => SessionKind::Race,
            _ => SessionKind::Hotlap,
        };
        s.name = match s.kind {
            SessionKind::Practice => "Practice",
            SessionKind::Qualifying => "Qualifying",
            SessionKind::Race => "Race",
            SessionKind::Hotlap => "Hotlap",
        }
        .into();
        s.elapsed = elapsed;
        // 0 = no time limit. The broadcast's elapsed and remaining share one clock; shared
        // memory's remaining time runs up to a second apart from it.
        s.duration = match &self.realtime {
            _ if !timed => 0.0,
            Some(r) if r.session_time >= 0.0 && r.session_end_time >= 0.0 => {
                (r.session_time + r.session_end_time) as f64 / 1000.0
            }
            _ => elapsed + remaining,
        };
        (s.flag, s.yellow_sector) = flag(&g);
        s.pit_window = st.and_then(|s| {
            (s.pit_window_end > s.pit_window_start && s.pit_window_start > 0).then(|| PitWindow {
                open: s.pit_window_start as f64 / 1000.0,
                close: s.pit_window_end as f64 / 1000.0,
            })
        });
        // 255 outside races and when the session has no mandatory stops
        s.mandatory_stops_left = (0..100)
            .contains(&g.missing_mandatory_pits)
            .then_some(g.missing_mandatory_pits as u32);
        s.time_of_day = g.clock as f64;

        // ---------- weather ----------
        let w = &mut inp.weather;
        w.air = p.air_temp;
        w.track = p.road_temp;
        w.rain_now = g.rain_intensity as f32 / 5.0;
        w.rain10 = g.rain_intensity_in_10min as f32 / 5.0;
        w.rain30 = g.rain_intensity_in_30min as f32 / 5.0;
        w.wetness = match g.track_grip_status {
            4 => 0.15, // damp
            5 => 0.4,  // wet
            6 => 1.0,  // flooded
            _ => 0.0,
        };
        w.grip = [
            "Green", "Fast", "Optimum", "Greasy", "Damp", "Wet", "Flooded",
        ]
        .get(g.track_grip_status as usize)
        .copied()
        .unwrap_or("")
        .into();
        // ACC reports m/s and radians
        w.wind_speed = g.wind_speed * 3.6;
        w.wind_dir = g.wind_direction.to_degrees().rem_euclid(360.0);
        // the broadcast's clouds / rain / wetness read 0 even in heavy rain; shared memory
        // has no cloud cover, so it stays unknown
        w.clouds = 0.0;

        // ---------- cars ----------
        let coords: HashMap<u16, (f32, f32)> = (0..(g.active_cars.clamp(0, 60) as usize))
            .map(|k| {
                (
                    g.car_id[k].clamp(0, u16::MAX as i32) as u16,
                    (g.car_coordinates[k][0], g.car_coordinates[k][2]),
                )
            })
            .collect();
        let length = inp.track.length.max(1.0);
        let player_name = st
            .map(|s| {
                format!("{} {}", wstr(&s.player_name), wstr(&s.player_surname))
                    .trim()
                    .to_owned()
            })
            .unwrap_or_default();
        inp.cars.resize_with(self.cars.len(), CarInput::default);
        let mut classes: Vec<String> = Vec::new();
        for (i, c) in self.cars.iter_mut().enumerate() {
            let ci = &mut inp.cars[i];
            let is_player = c.id == player_id;
            ci.idx = i;
            ci.id = c.id as u32;
            ci.is_player = is_player;
            match &c.entry {
                Some(e) => {
                    let m = cars::model(e.model);
                    ci.cls = m.as_ref().map_or("GT3", |m| m.class).into();
                    ci.model = m
                        .as_ref()
                        .map_or_else(|| format!("Car {}", e.model), |m| m.name.into());
                    ci.number = e.race_number.max(0) as u32;
                    ci.team.clone_from(&e.team);
                    if let Some(d) = e
                        .drivers
                        .get(e.current_driver as usize)
                        .or(e.drivers.first())
                    {
                        ci.driver = format!("{} {}", d.first_name, d.last_name)
                            .trim()
                            .to_owned();
                        let initial = d
                            .first_name
                            .chars()
                            .next()
                            .map(|c| format!("{c}. "))
                            .unwrap_or_default();
                        ci.short = format!("{initial}{}", d.last_name);
                        ci.cat = cars::category(d.category).into();
                    }
                }
                None if is_player => {
                    ci.cls = "GT3".into();
                    ci.driver.clone_from(&player_name);
                    ci.short.clone_from(&player_name);
                    ci.model = st.map(|s| wstr(&s.car_model)).unwrap_or_default();
                }
                // the entry list hasn't arrived yet for this car
                None if ci.cls.is_empty() => ci.cls = "GT3".into(),
                None => {}
            }
            if !classes.contains(&ci.cls) {
                classes.push(ci.cls.clone());
            }

            if is_player {
                ci.laps = g.completed_laps.max(0) as u32;
                ci.pct = (g.normalized_car_position as f64).rem_euclid(1.0);
                ci.speed = p.speed_kmh / 3.6;
                ci.position = (g.position > 0).then_some(g.position as u32);
                ci.last_lap = g.i_last_time as f64 / 1000.0;
                ci.in_pit = g.is_in_pit_lane != 0 || g.is_in_pit != 0;
                ci.penalty = penalty(g.penalty);
            } else if let Some(u) = &c.update {
                // extrapolate between ~10 Hz broadcast updates for smooth motion
                let dt = (now - c.updated_at).clamp(0.0, 0.5);
                let pct = u.spline as f64 + u.kmh as f64 / 3.6 * dt / length;
                ci.laps = u.laps as u32 + pct.floor() as u32;
                ci.pct = pct.rem_euclid(1.0);
                ci.speed = u.kmh as f32 / 3.6;
                ci.position = (u.position > 0).then_some(u.position as u32);
                ci.last_lap = c.last_lap;
                ci.in_pit = u.location.in_pit();
            } else {
                // on the entry list but not out yet (in the garage)
                ci.in_pit = true;
            }
            if let Some(&(x, y)) =
                coords
                    .get(&c.id)
                    .or(c.update.as_ref().map(|u| (u.x, u.y)).as_ref())
            {
                if let Some((px, py)) = c.prev
                    && (x - px).hypot(y - py) > 0.5
                {
                    c.heading = (y - py).atan2(x - px);
                    c.prev = Some((x, y));
                } else if c.prev.is_none() {
                    c.prev = Some((x, y));
                }
                ci.x = x;
                ci.y = y;
                ci.heading = c.heading;
                ci.located = true;
            } else {
                ci.located = false;
            }
        }
        // only rank by the game's positions when every car has one
        if inp.cars.iter().any(|c| c.position.is_none()) {
            inp.cars.iter_mut().for_each(|c| c.position = None);
        }
        classes.sort_by_key(|c| cars::CLASS_ORDER.iter().position(|o| o == c).unwrap_or(99));
        inp.classes = classes;

        // ---------- player ----------
        let pi = &mut inp.player;
        pi.car = self.by_id[&player_id];
        pi.speed = p.speed_kmh;
        let gear = (p.gear - 1).clamp(-1, 10) as i8;
        if gear == 0 && p.speed_kmh > 10.0 && self.gear > 0 {
            let since = *self.neutral_since.get_or_insert(now);
            if now - since >= SHIFT_NEUTRAL {
                self.gear = 0;
            }
        } else {
            self.neutral_since = None;
            self.gear = gear;
        }
        pi.gear = self.gear;
        pi.rpm = p.rpms as f32;
        pi.max_rpm = st.map_or(8000.0, |s| s.max_rpm.max(1) as f32);
        pi.shift_rpm = pi.max_rpm * 0.96;
        pi.throttle = p.gas;
        pi.brake = p.brake;
        pi.clutch = p.clutch;
        pi.steer = p.steer_angle;
        pi.abs_active = p.abs > 0.0;
        pi.tc_active = p.tc > 0.0;
        pi.tc = g.tc.clamp(0, 255) as u8;
        pi.tc2 = g.tc_cut.clamp(0, 255) as u8;
        pi.abs = g.abs.clamp(0, 255) as u8;
        // shown like the car's own display: raw front share plus a per-car offset (unknown
        // cars keep the raw share)
        let offset = st
            .and_then(|s| cars::brake_bias_offset(&wstr(&s.car_model)))
            .unwrap_or(0.0);
        pi.bb = if p.brake_bias > 0.0 {
            (p.brake_bias * 1000.0 + offset * 10.0).round() / 10.0
        } else {
            0.0
        };
        pi.map = (g.engine_map + 1).clamp(0, 255) as u8;
        pi.fuel = p.fuel;
        pi.fuel_capacity = st.map_or(0.0, |s| s.max_fuel);
        let compound = match wstr(&g.tyre_compound).as_str() {
            "dry_compound" => "Dry".to_owned(),
            "wet_compound" => "Wet".to_owned(),
            other => other.to_owned(),
        };
        pi.tyres.resize_with(4, Tyre::default);
        for (k, (ty, name)) in pi
            .tyres
            .iter_mut()
            .zip(["FL", "FR", "RL", "RR"])
            .enumerate()
        {
            if ty.name != name {
                ty.name = name.into();
            }
            ty.pressure = p.wheels_pressure[k];
            ty.core = p.tyre_core_temperature[k];
            ty.brake = p.brake_temp[k];
            ty.i = None;
            ty.m = None;
            ty.o = None;
            ty.wear = None;
            if ty.compound != compound {
                ty.compound.clone_from(&compound);
            }
        }
        pi.lap_time = g.i_current_time.max(0) as f64 / 1000.0;
        pi.sector = g.current_sector_index.max(0) as usize;
        pi.lap_valid = g.is_valid_lap != 0;
        // signed milliseconds, against the best lap of the session (none yet: i32::MAX).
        // Just before the line ACC drops the estimate and reports 0; keep the last delta then.
        if g.i_best_time <= 0 || g.i_best_time == i32::MAX {
            self.delta = None;
        } else if g.i_estimated_lap_time != i32::MAX {
            self.delta = Some(g.i_delta_lap_time as f64 / 1000.0);
        }
        pi.delta = self.delta;

        // ---------- learn the track ----------
        let pc = &inp.cars[pi.car];
        self.learner
            .sample(pc.pct, pc.x, pc.y, pi.sector, !pc.in_pit, pi.lap_valid);
        if self.player_laps.is_some_and(|l| pc.laps > l)
            && self.learner.lap_done()
            && let Some((x, y)) = self.learner.outline.clone()
        {
            inp.track.x = x;
            inp.track.y = y;
            inp.track.sectors.clone_from(&self.learner.sectors);
            if let Some(dir) = &self.opts.track_cache {
                let _ = Learner::save(dir, &inp.track, self.learner.outline_valid);
            }
        }
        self.player_laps = Some(pc.laps);
    }
}

impl<S: Source> GameAdapter for Acc<S> {
    fn game(&self) -> Game {
        Game::Acc
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            session_data: true,
            tyre_zones: false,
            tyre_wear: false,
            driver_category: true,
            per_car_laps: true,
            car_positions: true,
        }
    }

    fn poll(&mut self) -> AdapterEvent {
        let now = self.source.clock();
        let mut raw = std::mem::take(&mut self.raw);
        self.source.drain(&mut raw);
        let got = !raw.is_empty();
        for r in raw.drain(..) {
            self.handle(r, now);
        }
        self.raw = raw;

        let live = self.source.connected()
            && now - self.last_physics_at < STALE_AFTER
            && self.graphics.is_some_and(|g| g.status != STATUS_OFF);
        self.keep_registered(now, live);
        let status = if live {
            Connection::Connected
        } else {
            Connection::WaitingForGame
        };
        if status != self.status {
            self.status = status;
            if live {
                self.forget_session(now);
            }
            return AdapterEvent::Connection(status);
        }
        if live && got {
            self.build(now);
            AdapterEvent::Updated
        } else {
            AdapterEvent::Idle
        }
    }

    fn input(&self) -> &Input {
        &self.input
    }
}

/// The flag to show and the yellow sector (1-based, 0 = none or unknown).
///
/// The player's own flag only covers what concerns this car and is shown briefly (green for
/// 20 s at the start, chequered when this car finishes); the global flags carry the track
/// state for the whole session: red before the start, green, yellow per sector, white once
/// the clock has run out (last lap) and chequered once the leader finishes.
fn flag(g: &Graphics) -> (Flag, u8) {
    let sector = [g.global_yellow1, g.global_yellow2, g.global_yellow3]
        .iter()
        .position(|&y| y != 0)
        .map_or(0, |k| k as u8 + 1);
    let own = match g.flag {
        1 => Flag::Blue,
        2 => Flag::Yellow,
        3 | 8 => Flag::Black,
        4 => Flag::White,
        5 => Flag::Chequered,
        6 => Flag::Blackwhite,
        _ => Flag::None,
    };
    let f = if own != Flag::None {
        own
    } else if g.global_chequered != 0 {
        Flag::Chequered
    } else if g.global_yellow != 0 {
        Flag::Yellow
    } else if g.global_white != 0 {
        Flag::White
    } else if g.global_green != 0 || g.flag == 7 {
        Flag::Green
    } else {
        Flag::None
    };
    (f, if f == Flag::Yellow { sector } else { 0 })
}

/// "monza" → "Monza", "spa_2019" → "Spa 2019".
fn pretty(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// ACC penalty codes repeat a pattern of six per reason (cutting, pit speeding, …).
fn penalty(code: i32) -> String {
    match code {
        0 => String::new(),
        1..=18 => ["DT", "SG10", "SG20", "SG30", "DSQ", "Lap"][((code - 1) % 6) as usize].into(),
        _ => "Penalty".into(),
    }
}

fn empty_input() -> Input {
    Input {
        t: 0.0,
        track: Track::default(),
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
        player: PlayerInput {
            lap_valid: true,
            ..Default::default()
        },
        cars: Vec::new(),
        classes: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
