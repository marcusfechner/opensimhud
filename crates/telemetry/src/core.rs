//! Derives the overlay [`State`] from adapter [`Input`]. Game-agnostic: every adapter gets
//! ranking, gaps, the relative, lap history, the lap log, sectors, delta, fuel figures,
//! best laps and pit-stop bookkeeping from here.

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::input::{CarInput, Input};
use crate::model::{
    BestLap, Capabilities, Car, Connection, Fuel, Game, Lap, LapEntry, Player, SectorState,
    SessionInfo, SessionKind, State, Weather,
};

/// Bins of a time-at-position trace over one lap.
const BINS: usize = 1000;
/// Lap times kept per car, and laps kept in the player's log.
const HISTORY: usize = 80;
/// Laps averaged for fuel use per lap.
const FUEL_LAPS: usize = 5;
/// Lap time assumed before anyone has completed a lap.
const FALLBACK_LAP: f64 = 120.0;
/// Seconds into a restarted lap clock to wait for the lap count before taking the restart
/// as a reset rather than the line.
const NEXT_LAP_WAIT: f64 = 2.0;

/// Time at each position of one lap (`t[BINS]` is the lap time).
#[derive(Debug, Clone)]
struct Trace {
    t: Vec<f64>,
}

impl Trace {
    /// Constant speed: the fallback reference before a real lap exists.
    fn linear(lap: f64) -> Self {
        Self {
            t: (0..=BINS).map(|i| lap * i as f64 / BINS as f64).collect(),
        }
    }

    fn lap_time(&self) -> f64 {
        self.t[BINS]
    }

    /// Time at lap fraction `pct` (0..1).
    fn at(&self, pct: f64) -> f64 {
        let f = pct.clamp(0.0, 1.0) * BINS as f64;
        let i = (f as usize).min(BINS - 1);
        let k = f - i as f64;
        self.t[i] + (self.t[i + 1] - self.t[i]) * k
    }

    /// Time from distance 0 to `dist` (laps + fraction) at this pace.
    fn time_to(&self, dist: f64) -> f64 {
        let laps = dist.floor();
        laps * self.lap_time() + self.at(dist - laps)
    }
}

/// Per-car bookkeeping between updates.
#[derive(Debug, Clone, Default)]
struct CarMemo {
    seen: bool,
    laps: u32,
    in_pit: bool,
}

/// Player bookkeeping between updates.
#[derive(Debug, Clone)]
struct PlayerMemo {
    /// Set by `update_cars` when the player's lap count went up this update.
    lap_done: bool,
    valid: bool,
    /// Validity of the next lap, while the game's lap clock has restarted but the lap count
    /// hasn't caught up yet (ACC: a frame or two).
    next_valid: Option<bool>,
    lap_time: f64,
    sector: usize,
    sectors: Vec<f64>,
    /// Per sector of this lap: driven clean (see `Lap::best_sectors`).
    clean: Vec<bool>,
    /// The car was in the pit lane during the current sector.
    sector_pit: bool,
    /// The delta when the current sector began.
    sector_start: f64,
    trace: Vec<f64>,
    last_bin: usize,
    best_trace: Option<Trace>,
    last_trace: Option<Trace>,
    fuel_prev: f32,
    fuel_used: f32,
    fuel_hist: VecDeque<f32>,
    electronics: Option<(u8, u8, u8, f32, u8)>,
}

impl Default for PlayerMemo {
    fn default() -> Self {
        Self {
            lap_done: false,
            valid: true,
            next_valid: None,
            lap_time: 0.0,
            sector: 0,
            sectors: Vec::new(),
            clean: Vec::new(),
            sector_pit: false,
            sector_start: 0.0,
            trace: vec![0.0; BINS + 1],
            last_bin: 0,
            best_trace: None,
            last_trace: None,
            fuel_prev: 0.0,
            fuel_used: 0.0,
            fuel_hist: VecDeque::new(),
            electronics: None,
        }
    }
}

/// Owns the derived [`State`] and updates it from each new [`Input`].
pub struct Session {
    pub state: State,
    cars: Vec<CarMemo>,
    player: PlayerMemo,
    /// Cars in the last ranking, to notice the field arriving after the player.
    ranked: usize,
    /// `CarInput::id` of each car, in index order.
    ids: Vec<u32>,
    /// Cars that left, by id, kept in case they come back.
    gone: HashMap<u32, (Car, CarMemo)>,
}

impl Session {
    pub fn new(game: Game, capabilities: Capabilities) -> Self {
        Self {
            state: State {
                game,
                connection: Connection::Connected,
                capabilities,
                t: 0.0,
                track: Default::default(),
                session: SessionInfo {
                    kind: SessionKind::Practice,
                    name: String::new(),
                    duration: 0.0,
                    elapsed: 0.0,
                    time_remaining: 0.0,
                    lap_remaining: 0.0,
                    flag: Default::default(),
                    yellow_sector: 0,
                    pit_window: None,
                    mandatory_stops_left: None,
                    time_of_day: 0.0,
                    best: BTreeMap::new(),
                },
                weather: Weather::default(),
                player: Player::default(),
                cars: Vec::new(),
                order: Vec::new(),
                classes: Vec::new(),
            },
            cars: Vec::new(),
            player: PlayerMemo::default(),
            ranked: 0,
            ids: Vec::new(),
            gone: HashMap::new(),
        }
    }

    /// Whether the game is connected; overlays hide while it isn't.
    pub fn set_connection(&mut self, connection: Connection) {
        self.state.connection = connection;
    }

    /// Apply the adapter's latest signals.
    pub fn update(&mut self, input: &Input) {
        // a new session (practice → race, restart): start the bookkeeping over
        let s = &self.state.session;
        let track_changed = self.state.track.name != input.track.name;
        let session_name_changed = s.name != input.session.name;
        let player_lap_counter_reset = input
            .cars
            .get(input.player.car)
            .and_then(|player| {
                self.state
                    .cars
                    .iter()
                    .find(|car| car.is_player)
                    .map(|previous| {
                        player.laps < previous.laps
                            && input.player.best_lap <= 0.0
                            && self.state.player.lap.best > 0.0
                    })
            })
            .unwrap_or(false);
        if !self.cars.is_empty()
            && (s.kind != input.session.kind
                || input.session.elapsed + 5.0 < s.elapsed
                || track_changed
                || session_name_changed
                || player_lap_counter_reset)
        {
            let (game, caps, connection) = (
                self.state.game,
                self.state.capabilities,
                self.state.connection,
            );
            *self = Session::new(game, caps);
            self.state.connection = connection;
        }
        self.copy_session(input);
        self.update_cars(input);
        self.update_player(input);
        self.rank(input);
        self.gaps();
        self.relative();
        self.best_laps();
        self.lap_remaining(input);
    }

    fn copy_session(&mut self, input: &Input) {
        let s = &mut self.state;
        s.t = input.t;
        if s.track.name != input.track.name || s.track.x.len() != input.track.x.len() {
            s.track = input.track.clone();
        }
        if s.classes != input.classes {
            s.classes = input.classes.clone();
        }
        s.weather.clone_from(&input.weather);
        let si = &input.session;
        let so = &mut s.session;
        so.kind = si.kind;
        if so.name != si.name {
            so.name.clone_from(&si.name);
        }
        so.duration = si.duration;
        so.elapsed = si.elapsed;
        so.time_remaining = si
            .time_remaining
            .unwrap_or_else(|| (si.duration - si.elapsed).max(0.0));
        so.lap_remaining = si.lap_remaining.unwrap_or(0.0).max(0.0);
        so.flag = si.flag;
        so.yellow_sector = si.yellow_sector;
        so.pit_window = si.pit_window;
        so.mandatory_stops_left = si.mandatory_stops_left;
        so.time_of_day = si.time_of_day;
    }

    fn update_cars(&mut self, input: &Input) {
        if self.ids.len() != input.cars.len()
            || self.ids.iter().zip(&input.cars).any(|(&id, c)| id != c.id)
        {
            self.rekey(input);
        }
        let n = input.cars.len();
        self.state.cars.resize_with(n, Car::default);
        self.cars.resize_with(n, CarMemo::default);
        for (i, ci) in input.cars.iter().enumerate() {
            let car = &mut self.state.cars[i];
            let memo = &mut self.cars[i];
            if !memo.seen || car.number != ci.number || car.driver != ci.driver {
                copy_identity(car, ci);
            }
            car.idx = i;
            car.laps = ci.laps;
            car.pct = ci.pct;
            car.dist = ci.laps as f64 + ci.pct;
            car.speed = ci.speed;
            car.x = ci.x;
            car.y = ci.y;
            car.heading = ci.heading;
            car.located = ci.located;
            car.in_pit = ci.in_pit;
            if car.penalty != ci.penalty {
                car.penalty.clone_from(&ci.penalty);
            }

            if memo.seen {
                // pit lane: entering marks the in-lap, leaving completes a stop and starts the out-lap
                if ci.in_pit && !memo.in_pit {
                    car.pit_laps.push(ci.laps + 1);
                } else if !ci.in_pit && memo.in_pit {
                    car.pit_stops += 1;
                    car.out_lap = true;
                }
                if ci.laps > memo.laps {
                    if ci.last_lap > 0.0 {
                        car.lap_times.push(ci.last_lap);
                        if car.lap_times.len() > HISTORY {
                            car.lap_times.remove(0);
                        }
                        car.last_lap = ci.last_lap;
                        // the player's best only counts valid laps; see `update_player`
                        if !ci.is_player && (car.best_lap == 0.0 || ci.last_lap < car.best_lap) {
                            car.best_lap = ci.last_lap;
                        }
                    }
                    if car.out_lap {
                        car.pit_laps.push(ci.laps);
                        car.out_lap = false;
                    }
                    if ci.is_player && ci.last_lap > 0.0 {
                        self.player.lap_done = true;
                    }
                }
            }
            memo.seen = true;
            memo.laps = ci.laps;
            memo.in_pit = ci.in_pit;
        }
    }

    /// Cars joined or left, so indices moved: keep each car's history with its id.
    fn rekey(&mut self, input: &Input) {
        let mut by_id = std::mem::take(&mut self.gone);
        let old = self.state.cars.drain(..).zip(self.cars.drain(..));
        for (&id, (car, memo)) in self.ids.iter().zip(old) {
            by_id.insert(id, (car, memo));
        }
        for ci in &input.cars {
            let (car, memo) = by_id.remove(&ci.id).unwrap_or_default();
            self.state.cars.push(car);
            self.cars.push(memo);
        }
        self.ids = input.cars.iter().map(|c| c.id).collect();
        self.gone = by_id;
    }

    fn update_player(&mut self, input: &Input) {
        let pi = &input.player;
        let idx = pi.car;
        let sectors = input.track.sectors.len().max(1);
        let m = &mut self.player;
        let p = &mut self.state.player;
        let t = input.t;
        let first = m.electronics.is_none();
        if m.sectors.len() != sectors {
            m.sectors = vec![0.0; sectors];
            m.clean = vec![false; sectors];
            p.lap.theoretical = 0.0;
            p.lap.sectors = vec![0.0; sectors];
            p.lap.sector_state = vec![SectorState::Pending; sectors];
            p.lap.best_sectors = vec![0.0; sectors];
            p.lap.last_sectors = vec![0.0; sectors];
            p.lap.sector_deltas = vec![None; sectors];
            p.lap.last_sector_state = vec![SectorState::Pending; sectors];
            p.lap.last_sector_deltas = vec![None; sectors];
        }

        // physics and electronics, straight through
        p.car = idx;
        p.speed = pi.speed;
        p.gear = pi.gear;
        p.rpm = pi.rpm;
        p.max_rpm = pi.max_rpm;
        p.shift_rpm = pi.shift_rpm;
        p.throttle = pi.throttle;
        p.brake = pi.brake;
        p.clutch = pi.clutch;
        p.steer = pi.steer;
        p.abs_active = pi.abs_active;
        p.tc_active = pi.tc_active;
        let now = (pi.tc, pi.tc2, pi.abs, pi.bb, pi.map);
        if let Some(prev) = m.electronics
            && prev != now
        {
            p.changed = if prev.0 != now.0 {
                "tc"
            } else if prev.1 != now.1 {
                "tc2"
            } else if prev.2 != now.2 {
                "abs"
            } else if prev.3 != now.3 {
                "bb"
            } else {
                "map"
            }
            .into();
            p.changed_at = t;
        }
        m.electronics = Some(now);
        (p.tc, p.tc2, p.abs, p.bb, p.map) = now;
        p.tyres.clone_from(&pi.tyres);

        // fuel burnt this lap (refuelling never counts as negative use)
        if !first && pi.fuel < m.fuel_prev {
            m.fuel_used += m.fuel_prev - pi.fuel;
        }
        m.fuel_prev = pi.fuel;

        let Some(car_in) = input.cars.get(idx) else {
            return;
        };
        let car = &mut self.state.cars[idx];
        let lap = &mut p.lap;
        // something to measure the delta against: the game's own delta or a best lap trace
        let reference = pi.delta.is_some() || m.best_trace.is_some();
        m.sector_pit |= car_in.in_pit;

        // lap completed: log it, then reset the per-lap bookkeeping
        // the lap clock restarting means the line too: what the game says about validity from
        // here belongs to the new lap, even before the lap count goes up
        if pi.lap_time + 1.0 < m.lap_time {
            m.next_valid = Some(true);
        }
        m.lap_time = pi.lap_time;

        let lap_completed = std::mem::take(&mut m.lap_done);
        if lap_completed {
            let lt = car.last_lap;
            let pit = car.pit_laps.contains(&car.laps);
            let done: f64 = m.sectors[..sectors - 1].iter().sum();
            m.sectors[sectors - 1] = lt - done;
            let clean = m.valid && !m.sector_pit && !car.out_lap && !pit;
            m.clean[sectors - 1] = clean;
            close_sector(
                lap,
                sectors - 1,
                m.sectors[sectors - 1],
                clean,
                &self.state.session.best,
                &car.cls,
            );
            m.trace[BINS] = lt;
            // The last sampled position can be a little short of the line because the game
            // resets lap time and increments its lap counter before the adapter's position
            // wraps. Interpolate that small unobserved tail to the measured lap time instead
            // of leaving zeroes there (which produce a lap-sized delta near the finish).
            let last = m.last_bin.min(BINS - 1);
            let from = m.trace[last];
            for (bin, trace_time) in m.trace.iter_mut().enumerate().take(BINS + 1).skip(last + 1) {
                let fraction = (bin - last) as f64 / (BINS - last) as f64;
                *trace_time = from + (lt - from) * fraction;
            }
            let trace = Trace {
                t: std::mem::replace(&mut m.trace, vec![0.0; BINS + 1]),
            };
            if m.valid {
                // laps into or out of the pits don't set a best lap (as in ACC)
                let is_best = lap.best == 0.0 || lt < lap.best;
                let reported_best_matches = pi.best_lap > 0.0 && (pi.best_lap - lt).abs() <= 0.005;
                if !pit && (is_best || (m.best_trace.is_none() && reported_best_matches)) {
                    lap.best = lt;
                    m.best_trace = Some(trace.clone());
                }
            }
            m.last_trace = Some(trace);
            lap.last = lt;
            lap.last_sectors.clone_from(&m.sectors);
            car.best_lap = lap.best;
            p.log.push(LapEntry {
                lap: car.laps,
                time: lt,
                sectors: m.sectors.clone(),
                clean: m.clean.clone(),
                valid: m.valid,
                fuel: m.fuel_used,
                pit,
            });
            if p.log.len() > HISTORY {
                p.log.remove(0);
            }
            if !pit && m.fuel_used > 0.0 {
                m.fuel_hist.push_back(m.fuel_used);
                if m.fuel_hist.len() > FUEL_LAPS {
                    m.fuel_hist.pop_front();
                }
            }
            p.fuel.last_lap_use = m.fuel_used;
            p.stint_laps += 1;
            m.fuel_used = 0.0;
            m.valid = m.next_valid.take().unwrap_or(true);
            m.sector = 0;
            m.last_bin = 0;
            m.sectors.iter_mut().for_each(|s| *s = 0.0);
            m.clean.iter_mut().for_each(|c| *c = false);
            m.sector_pit = car_in.in_pit;
            // the last sector's delta is the running one from just before the line
            lap.last_sector_state.clone_from(&lap.sector_state);
            lap.last_sector_deltas.clone_from(&lap.sector_deltas);
            m.sector_start = 0.0;
            lap.sector_deltas.iter_mut().for_each(|d| *d = None);
            lap.sector_state
                .iter_mut()
                .for_each(|s| *s = SectorState::Pending);
        }

        // Apply a game-reported best after processing a just-completed lap so that the core
        // still recognizes a new personal best and captures its delta trace.
        if pi.best_lap > 0.0 && (lap.best == 0.0 || pi.best_lap < lap.best) {
            lap.best = pi.best_lap;
            car.best_lap = lap.best;
        }

        // validity is latched: once invalid, the lap stays invalid
        let mut restarted_without_line = false;
        match &mut m.next_valid {
            // no lap counted for a while: the clock restarted without crossing the line
            // (back to the pits, session reset), so the new lap is the one running
            Some(v) if pi.lap_time > NEXT_LAP_WAIT => {
                m.valid = *v && pi.lap_valid;
                m.next_valid = None;
                restarted_without_line = true;
                m.sector = pi.sector.min(sectors - 1);
                m.sectors.fill(0.0);
                m.clean.fill(false);
                m.sector_pit = car_in.in_pit;
                m.trace.fill(0.0);
                m.last_bin = 0;
                lap.sectors.fill(0.0);
                lap.sector_state.fill(SectorState::Pending);
                lap.sector_deltas.fill(None);
            }
            Some(v) => *v &= pi.lap_valid,
            None => m.valid &= pi.lap_valid,
        }
        lap.valid = m.next_valid.unwrap_or(m.valid);

        // back to an earlier sector with no time in the splits so far: the car stood behind
        // the line (grid, standing start) and has now crossed it, so the lap starts here.
        // At a normal line crossing the sector index can drop a frame before the lap counts;
        // those splits hold real times and are logged with the lap.
        if pi.sector < m.sector && m.sectors[..m.sector].iter().sum::<f64>() < 1.0 {
            m.sector = pi.sector;
            m.sectors.iter_mut().for_each(|s| *s = 0.0);
            m.clean.iter_mut().for_each(|c| *c = false);
            m.sector_pit = car_in.in_pit;
            lap.sector_state
                .iter_mut()
                .for_each(|s| *s = SectorState::Pending);
            m.trace.iter_mut().for_each(|t| *t = 0.0);
            m.last_bin = 0;
            m.sector_start = 0.0;
            lap.sector_deltas.iter_mut().for_each(|d| *d = None);
        }

        // time-at-position trace, and the delta: the game's when it reports one
        // EVO can report the completed lap count before its projected position wraps from
        // the end of the track to the start. Treat that one stale end-of-lap position as 0
        // for the new lap; otherwise it fills every trace bin with the reset lap clock.
        let pct = if lap_completed && car_in.pct > 0.5 {
            0.0
        } else {
            car_in.pct
        };
        let bin = ((pct * BINS as f64) as usize).min(BINS - 1);
        if bin >= m.last_bin {
            for b in m.last_bin..=bin {
                m.trace[b] = pi.lap_time;
            }
            m.last_bin = bin;
        }
        lap.current = pi.lap_time;
        lap.delta = match (pi.delta, &m.best_trace) {
            (Some(d), _) => d,
            (None, Some(best)) => pi.lap_time - best.at(pct),
            (None, None) => 0.0,
        };
        lap.predicted = if reference && lap.best > 0.0 {
            lap.best + lap.delta
        } else {
            0.0
        };
        lap.delta_last = m
            .last_trace
            .as_ref()
            .map_or(0.0, |tr| pi.lap_time - tr.at(pct));
        if restarted_without_line {
            m.sector_start = lap.delta;
        }
        // sector splits, measured from the running lap time
        if !lap_completed && pi.sector > m.sector && pi.sector < sectors {
            for k in m.sector..pi.sector {
                let before: f64 = m.sectors[..k].iter().sum();
                m.sectors[k] = pi.lap_time - before;
                // a skipped split (a sector index jump) has no real time of its own
                let clean = m.valid && !m.sector_pit && !car.out_lap && k == m.sector;
                m.clean[k] = clean;
                close_sector(
                    lap,
                    k,
                    m.sectors[k],
                    clean,
                    &self.state.session.best,
                    &car.cls,
                );
                // time gained since the last split goes to the sector just left
                lap.sector_deltas[k] =
                    (reference && k == m.sector).then_some(lap.delta - m.sector_start);
            }
            m.sector = pi.sector;
            m.sector_start = lap.delta;
            m.sector_pit = car_in.in_pit;
        }
        // the running sector (not on the frame the index already dropped at the line)
        if lap_completed && m.sector < sectors {
            // The new lap begins in sector 0 even if the adapter's position is still at
            // the end of the previous lap. Keep its initial delta available to the HUD.
            lap.sector_deltas[m.sector] = reference.then_some(lap.delta - m.sector_start);
        } else if pi.sector == m.sector && m.sector < sectors {
            lap.sector_deltas[m.sector] = reference.then_some(lap.delta - m.sector_start);
        }
        lap.sector = if lap_completed { m.sector } else { pi.sector };
        lap.sectors.clone_from(&m.sectors);

        // pit stops: a completed stop starts a new stint
        if car.pit_stops > p.pit_stops {
            p.pit_stops = car.pit_stops;
            p.stint_laps = 0;
        }

        // fuel figures
        let f = &mut p.fuel;
        f.liters = pi.fuel;
        f.capacity = pi.fuel_capacity;
        f.per_lap = if pi.fuel_per_lap > 0.0 {
            pi.fuel_per_lap
        } else if m.fuel_hist.is_empty() {
            0.0
        } else {
            m.fuel_hist.iter().sum::<f32>() / m.fuel_hist.len() as f32
        };
        f.laps_left = if pi.fuel_laps_left > 0.0 {
            pi.fuel_laps_left
        } else if f.per_lap > 0.0 {
            f.liters / f.per_lap
        } else {
            0.0
        };
        f.used_this_lap = m.fuel_used;
    }

    fn rank(&mut self, input: &Input) {
        // start positions follow the grid until the race starts, and are taken again when
        // the field shows up after the player alone (other cars' data can arrive later)
        let ses = &self.state.session;
        let grid = ses.kind == SessionKind::Race && ses.elapsed <= 0.0;
        let field_arrived = self.ranked <= 1 && input.cars.len() > 1;
        self.ranked = input.cars.len();
        let game = self.state.game;
        let cars = &mut self.state.cars;
        let order = &mut self.state.order;
        order.clear();
        order.extend(0..cars.len());
        if input.cars.iter().all(|c| c.position.is_some()) && !input.cars.is_empty() {
            order.sort_by_key(|&i| input.cars[i].position);
        } else {
            order.sort_by(|&a, &b| cars[b].dist.total_cmp(&cars[a].dist));
        }
        let mut class_count: BTreeMap<&str, u32> = BTreeMap::new();
        for (k, &i) in order.iter().enumerate() {
            let car = &mut cars[i];
            car.pos = input.cars[i].position.unwrap_or_else(|| {
                if game == Game::Evo && input.cars[i].is_player {
                    0
                } else {
                    k as u32 + 1
                }
            });
            let n = class_count.entry(input.cars[i].cls.as_str()).or_insert(0);
            *n += 1;
            car.class_pos = *n;
            if car.start_pos == 0 || grid || field_arrived {
                car.start_pos = car.pos;
            }
        }
    }

    /// The pace trace used to turn distances into time: the player's best lap when known.
    fn reference(&self) -> Trace {
        if let Some(t) = self
            .player
            .best_trace
            .as_ref()
            .or(self.player.last_trace.as_ref())
        {
            return t.clone();
        }
        let mut laps: Vec<f64> = self
            .state
            .cars
            .iter()
            .map(|c| c.best_lap)
            .filter(|&t| t > 0.0)
            .collect();
        laps.sort_by(f64::total_cmp);
        Trace::linear(laps.get(laps.len() / 2).copied().unwrap_or(FALLBACK_LAP))
    }

    fn gaps(&mut self) {
        let reference = self.reference();
        let ref_lap = reference.lap_time();
        let cars = &mut self.state.cars;
        let mut leader: BTreeMap<String, usize> = BTreeMap::new();
        let mut ahead: BTreeMap<String, usize> = BTreeMap::new();
        for &i in &self.state.order {
            let (cls, dist, best) = (cars[i].cls.clone(), cars[i].dist, cars[i].best_lap);
            // time gaps at this car's own pace
            let scale = if best > 0.0 { best / ref_lap } else { 1.0 };
            let gap = |other: f64| (reference.time_to(other) - reference.time_to(dist)) * scale;
            let l = *leader.entry(cls.clone()).or_insert(i);
            let lead_dist = cars[l].dist;
            let ahead_dist = ahead.get(&cls).map(|&a| cars[a].dist);
            let car = &mut cars[i];
            car.gap_leader = if l == i { 0.0 } else { gap(lead_dist) };
            car.laps_down = (lead_dist - dist + 1e-9).floor().max(0.0) as u32;
            match ahead_dist {
                Some(d) => {
                    car.gap_ahead = gap(d);
                    car.laps_ahead = (d - dist + 1e-9).floor().max(0.0) as u32;
                }
                None => {
                    car.gap_ahead = 0.0;
                    car.laps_ahead = 0;
                }
            }
            ahead.insert(cls, i);
        }
    }

    fn relative(&mut self) {
        let reference = self.reference();
        let length = self.state.track.length;
        let pi = self.state.player.car;
        let Some(p) = self.state.cars.get(pi).cloned() else {
            return;
        };
        // player's right-hand side in world space
        let (rx, ry) = (-p.heading.sin() as f64, p.heading.cos() as f64);
        for car in &mut self.state.cars {
            let diff = car.dist - p.dist;
            let w = diff - diff.round();
            car.rel_meters = w * length;
            car.rel_gap = reference.time_to(p.dist + w) - reference.time_to(p.dist);
            car.rel_laps = (diff - w).round() as i32;
            car.rel_lateral = (car.x - p.x) as f64 * rx + (car.y - p.y) as f64 * ry;
        }
    }

    fn best_laps(&mut self) {
        let best = &mut self.state.session.best;
        best.values_mut().for_each(|b| *b = BestLap::default());
        for car in &self.state.cars {
            if car.best_lap <= 0.0 {
                continue;
            }
            let b = best.entry(car.cls.clone()).or_default();
            if b.time == 0.0 || car.best_lap < b.time {
                *b = BestLap {
                    time: car.best_lap,
                    car: Some(car.idx),
                };
            }
        }
    }

    fn lap_remaining(&mut self, input: &Input) {
        let s = &mut self.state;
        let Some(car) = s.cars.get(s.player.car) else {
            return;
        };
        if let Some(laps) = input.session.lap_remaining {
            s.session.lap_remaining = laps.max(0.0);
        } else {
            let lap = &s.player.lap;
            let reference = [lap.best, lap.last, car.best_lap]
                .into_iter()
                .find(|&t| t > 0.0)
                .unwrap_or(FALLBACK_LAP);
            let rest = 1.0 - car.pct;
            s.session.lap_remaining = if s.session.kind == SessionKind::Race
                && s.session.duration > 0.0
            {
                rest + ((s.session.time_remaining - rest * reference).max(0.0) / reference).ceil()
            } else {
                0.0
            };
        }
        let f: &mut Fuel = &mut s.player.fuel;
        f.to_finish = s.session.lap_remaining as f32 * f.per_lap;
        f.to_add = (f.to_finish - f.liters + f.per_lap * 0.5).max(0.0);
    }
}

fn copy_identity(car: &mut Car, ci: &CarInput) {
    car.number = ci.number;
    car.driver.clone_from(&ci.driver);
    car.short.clone_from(&ci.short);
    car.team.clone_from(&ci.team);
    car.model.clone_from(&ci.model);
    car.cls.clone_from(&ci.cls);
    car.cat.clone_from(&ci.cat);
    car.nat.clone_from(&ci.nat);
    car.is_player = ci.is_player;
}

/// Finish sector `k` in `time`: its color, and a new best sector if it was driven clean.
fn close_sector(
    lap: &mut Lap,
    k: usize,
    time: f64,
    clean: bool,
    best: &BTreeMap<String, BestLap>,
    cls: &str,
) {
    if !clean || !time.is_finite() || time <= 0.0 {
        lap.sector_state[k] = SectorState::Invalid;
        return;
    }
    lap.sector_state[k] = sector_state(lap, time, k, best, cls);
    if lap.best_sectors[k] == 0.0 || time < lap.best_sectors[k] {
        lap.best_sectors[k] = time;
        lap.theoretical = if lap.best_sectors.iter().all(|&s| s > 0.0) {
            lap.best_sectors.iter().sum()
        } else {
            0.0
        };
    }
}

/// Session best (purple) when faster than the class best lap's share for this sector,
/// personal best (green) when faster than the player's best sector, otherwise slower.
fn sector_state(
    lap: &Lap,
    time: f64,
    k: usize,
    best: &BTreeMap<String, BestLap>,
    cls: &str,
) -> SectorState {
    let total: f64 = lap.best_sectors.iter().sum();
    let class_best = best.get(cls).map_or(0.0, |b| b.time);
    if total > 0.0 && class_best > 0.0 && time < class_best * lap.best_sectors[k] / total {
        SectorState::Sb
    } else if lap.best_sectors[k] == 0.0 || time < lap.best_sectors[k] {
        SectorState::Pb
    } else {
        SectorState::Slower
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{PlayerInput, SessionInput};
    use crate::model::{Flag, Track};

    fn input(cars: Vec<CarInput>) -> Input {
        Input {
            t: 0.0,
            track: Track {
                name: "Test".into(),
                length: 1000.0,
                sectors: vec![0.0, 0.5],
                x: vec![],
                y: vec![],
            },
            session: SessionInput {
                kind: SessionKind::Race,
                name: "Race".into(),
                duration: 600.0,
                elapsed: 0.0,
                time_remaining: None,
                lap_remaining: None,
                flag: Flag::Green,
                yellow_sector: 0,
                pit_window: None,
                mandatory_stops_left: None,
                time_of_day: 0.0,
            },
            weather: Weather::default(),
            player: PlayerInput {
                car: 0,
                fuel: 50.0,
                fuel_capacity: 100.0,
                lap_valid: true,
                ..Default::default()
            },
            cars,
            classes: vec!["GT3".into()],
        }
    }

    fn car(idx: usize, laps: u32, pct: f64) -> CarInput {
        CarInput {
            idx,
            id: idx as u32,
            number: idx as u32 + 1,
            cls: "GT3".into(),
            is_player: idx == 0,
            laps,
            pct,
            ..Default::default()
        }
    }

    /// Drive the player round `laps` laps of `lap_time` seconds in `steps` updates per lap.
    fn drive(s: &mut Session, inp: &mut Input, laps: u32, lap_time: f64, steps: u32, burn: f32) {
        for _ in 0..laps {
            for k in 1..=steps {
                let pct = k as f64 / steps as f64;
                inp.t += lap_time / steps as f64;
                inp.player.lap_time = lap_time * pct;
                inp.player.sector = if pct >= 0.5 { 1 } else { 0 };
                inp.player.fuel -= burn / steps as f32;
                let c = &mut inp.cars[0];
                if k == steps {
                    c.laps += 1;
                    c.pct = 0.0;
                    c.last_lap = lap_time;
                    inp.player.lap_time = 0.0;
                    inp.player.sector = 0;
                } else {
                    c.pct = pct;
                }
                s.update(inp);
            }
        }
    }

    /// Finish a lap when the lap counter advances before position wraps to the start.
    fn drive_lap_with_stale_end_position(s: &mut Session, inp: &mut Input, lap_time: f64) {
        let steps = 200;
        for k in 1..steps {
            let pct = k as f64 / steps as f64;
            inp.t += lap_time / steps as f64;
            inp.player.lap_time = lap_time * pct;
            inp.player.sector = if pct >= 0.5 { 1 } else { 0 };
            inp.player.lap_valid = true;
            inp.cars[0].pct = pct;
            s.update(inp);
        }
        inp.t += 0.1;
        inp.cars[0].laps += 1;
        inp.cars[0].pct = 0.999;
        inp.cars[0].last_lap = lap_time;
        inp.player.lap_time = 0.1;
        inp.player.sector = 1;
        s.update(inp);
    }

    #[test]
    fn trace_time_to_wraps_laps() {
        let t = Trace::linear(100.0);
        assert!((t.time_to(2.25) - 225.0).abs() < 1e-9);
        assert!((t.at(0.5) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn ranks_by_distance_and_measures_gaps() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 3, 0.5), car(1, 3, 0.25), car(2, 2, 0.75)]);
        for c in &mut inp.cars {
            c.last_lap = 100.0;
        }
        s.update(&inp);
        let st = &s.state;
        assert_eq!(st.order, vec![0, 1, 2]);
        assert_eq!(st.cars[2].class_pos, 3);
        // linear 120 s fallback reference until a lap is known
        assert!(
            (st.cars[1].gap_ahead - 30.0).abs() < 1e-6,
            "{}",
            st.cars[1].gap_ahead
        );
        assert!((st.cars[2].gap_leader - 90.0).abs() < 1e-6);
        // relative: car 1 is a quarter lap behind on track; car 2 is a lap down but a quarter
        // lap *ahead* on track (the nearest way round), so it counts as lapped
        assert!((st.cars[1].rel_meters + 250.0).abs() < 1e-6);
        assert!(st.cars[1].rel_gap < 0.0);
        assert!((st.cars[2].rel_meters - 250.0).abs() < 1e-6);
        assert!(st.cars[2].rel_gap > 0.0);
        assert_eq!(st.cars[2].rel_laps, -1);
    }

    #[test]
    fn logs_laps_latches_validity_and_keeps_best_valid() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 200, 3.0);
        // second lap is faster but invalid for a moment mid-lap
        for k in 1..=200u32 {
            let pct = k as f64 / 200.0;
            inp.t += 0.49;
            inp.player.lap_time = 98.0 * pct;
            inp.player.lap_valid = !(80..90).contains(&k);
            inp.player.sector = if pct >= 0.5 { 1 } else { 0 };
            let c = &mut inp.cars[0];
            if k == 200 {
                c.laps += 1;
                c.pct = 0.0;
                c.last_lap = 98.0;
                inp.player.lap_time = 0.0;
                inp.player.sector = 0;
            } else {
                c.pct = pct;
            }
            s.update(&inp);
        }
        let p = &s.state.player;
        assert_eq!(p.log.len(), 2);
        assert!(p.log[0].valid && !p.log[1].valid);
        assert_eq!(p.lap.best, 100.0, "invalid 98 s lap must not become best");
        assert_eq!(p.lap.last, 98.0);
        assert!((p.log[0].sectors[0] - 50.0).abs() < 0.6);
        assert!((p.log[0].sectors.iter().sum::<f64>() - 100.0).abs() < 1e-9);
        assert!(p.lap.valid, "validity resets on the new lap");
    }

    #[test]
    fn delta_against_best_lap() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 400, 3.0);
        // halfway through the next lap, one second down
        inp.cars[0].pct = 0.5;
        inp.player.lap_time = 51.0;
        s.update(&inp);
        assert!(
            (s.state.player.lap.delta - 1.0).abs() < 0.3,
            "{}",
            s.state.player.lap.delta
        );
        assert!((s.state.player.lap.predicted - 101.0).abs() < 0.3);
    }

    #[test]
    fn best_trace_interpolates_the_unsampled_finish_segment() {
        let mut s = Session::new(Game::Evo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 200, 3.0);

        // The final sampled pct was 0.995; EVO can reset the lap clock before a sample at
        // 0.998 arrives on the next lap, so the stored reference must cover this short tail.
        inp.t += 0.1;
        inp.cars[0].pct = 0.998;
        inp.player.sector = 1;
        inp.player.lap_time = 99.8;
        s.update(&inp);
        assert!(
            s.state.player.lap.delta.abs() < 0.1,
            "unexpected finish-line delta: {}",
            s.state.player.lap.delta
        );
    }

    #[test]
    fn stale_end_position_does_not_poison_the_next_lap_trace_or_sectors() {
        let mut s = Session::new(Game::Evo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 200, 3.0);

        drive_lap_with_stale_end_position(&mut s, &mut inp, 99.0);
        assert_eq!(s.state.player.lap.best_sectors, [49.5, 49.5]);
        assert_eq!(s.state.player.lap.theoretical, 99.0);
        assert_eq!(s.state.player.lap.sector, 0);
        assert!(s.state.player.lap.delta.abs() < 1.0);

        // A later PB must still build a usable trace; the stale 0.999 position from the
        // previous lap must not have filled the new trace with its reset 0.1-second clock.
        drive_lap_with_stale_end_position(&mut s, &mut inp, 98.0);
        inp.t += 0.1;
        inp.cars[0].pct = 0.5;
        inp.player.sector = 1;
        inp.player.lap_time = 49.4;
        s.update(&inp);
        assert!(
            s.state.player.lap.delta.abs() < 1.0,
            "unexpected delta from the recorded best trace: {}",
            s.state.player.lap.delta
        );
    }

    #[test]
    fn next_lap_invalid_at_the_line_keeps_the_finished_lap_valid() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 400, 3.0);
        assert_eq!(s.state.player.lap.best, 100.0);
        // a clean, faster lap up to the line
        for k in 1..400 {
            inp.t += 0.24;
            inp.cars[0].pct = k as f64 / 400.0;
            inp.player.lap_time = 0.24 * k as f64;
            inp.player.sector = if k >= 200 { 1 } else { 0 };
            s.update(&inp);
        }
        // ACC: the clock restarts and the new lap is already invalid (a cut just before
        // the line) a frame before the lap count goes up
        inp.t += 0.02;
        inp.cars[0].pct = 0.0;
        inp.player.lap_time = 0.01;
        inp.player.sector = 0;
        inp.player.lap_valid = false;
        s.update(&inp);
        inp.t += 0.02;
        inp.cars[0].laps += 1;
        inp.cars[0].last_lap = 96.0;
        inp.player.lap_time = 0.03;
        s.update(&inp);
        let p = &s.state.player;
        assert!(p.log.last().unwrap().valid);
        assert_eq!(p.lap.best, 96.0);
        assert_eq!(p.lap.last, 96.0);
        assert!(!p.lap.valid, "the new lap is the invalid one");
        // that lap stays invalid even once the flag comes back
        inp.player.lap_valid = true;
        inp.player.lap_time = 5.0;
        inp.cars[0].pct = 0.05;
        s.update(&inp);
        assert!(!s.state.player.lap.valid);
    }

    /// One lap with two sectors of `s1` and `s2` seconds; ACC invalidates the lap at `cut`
    /// (lap fraction), and the car starts it in the pit lane if `pit_out`.
    fn sector_lap(
        s: &mut Session,
        inp: &mut Input,
        s1: f64,
        s2: f64,
        cut: Option<f64>,
        pit_out: bool,
    ) {
        let steps = 200;
        for k in 1..=steps {
            let pct = k as f64 / steps as f64;
            let time = if pct <= 0.5 {
                s1 * pct * 2.0
            } else {
                s1 + s2 * (pct - 0.5) * 2.0
            };
            inp.t += (s1 + s2) / steps as f64;
            inp.player.lap_time = time;
            inp.player.sector = if pct >= 0.5 { 1 } else { 0 };
            if cut.is_some_and(|c| pct >= c) {
                inp.player.lap_valid = false;
            }
            let c = &mut inp.cars[0];
            c.in_pit = pit_out && pct < 0.1;
            if k == steps {
                c.laps += 1;
                c.pct = 0.0;
                c.last_lap = s1 + s2;
                inp.player.lap_time = 0.0;
                inp.player.sector = 0;
                inp.player.lap_valid = true;
            } else {
                c.pct = pct;
            }
            s.update(inp);
        }
    }

    #[test]
    fn sectors_before_a_cut_count_and_the_rest_dont() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        sector_lap(&mut s, &mut inp, 50.0, 50.0, None, false);
        let lap = &s.state.player.lap;
        assert_eq!(lap.best_sectors, [50.0, 50.0]);
        assert_eq!(lap.theoretical, 100.0);

        // faster in both, but cut in the second sector: only the first counts
        sector_lap(&mut s, &mut inp, 45.0, 44.0, Some(0.75), false);
        let p = &s.state.player;
        assert_eq!(p.lap.best_sectors, [45.0, 50.0]);
        assert_eq!(p.lap.theoretical, 95.0);
        assert_eq!(
            p.lap.last_sector_state,
            [SectorState::Sb, SectorState::Invalid]
        );
        assert_eq!(p.log.last().unwrap().clean, [true, false]);
        assert_eq!(p.lap.best, 100.0, "the lap itself stays invalid");

        // cut in the first sector: nothing after it counts either
        sector_lap(&mut s, &mut inp, 44.0, 43.0, Some(0.2), false);
        let lap = &s.state.player.lap;
        assert_eq!(lap.best_sectors, [45.0, 50.0]);
        assert_eq!(
            lap.last_sector_state,
            [SectorState::Invalid, SectorState::Invalid]
        );

        // an out-lap doesn't contribute sectors to the session bests
        sector_lap(&mut s, &mut inp, 40.0, 49.0, None, true);
        let lap = &s.state.player.lap;
        assert_eq!(lap.best_sectors, [45.0, 50.0]);
        assert_eq!(lap.theoretical, 95.0);
    }

    #[test]
    fn nonpositive_sector_times_do_not_become_best_sectors() {
        let mut lap = Lap {
            best_sectors: vec![0.0],
            sector_state: vec![SectorState::Pending],
            ..Lap::default()
        };
        close_sector(&mut lap, 0, -20.0, true, &BTreeMap::new(), "");
        assert_eq!(lap.best_sectors, [0.0]);
        assert_eq!(lap.theoretical, 0.0);
        assert_eq!(lap.sector_state, [SectorState::Invalid]);
    }

    #[test]
    fn pit_out_lap_sectors_do_not_set_the_best_sector_baseline() {
        let mut s = Session::new(Game::Evo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        inp.cars[0].in_pit = true;
        s.update(&inp);
        inp.cars[0].in_pit = false;
        s.update(&inp);
        assert!(s.state.cars[0].out_lap);

        drive(&mut s, &mut inp, 1, 96.0, 200, 3.0);
        let lap = s.state.player.log.last().unwrap();
        assert!(lap.pit);
        assert!(lap.clean.iter().all(|clean| !clean));
        assert_eq!(s.state.player.lap.best_sectors, [0.0, 0.0]);
        assert_eq!(s.state.player.lap.theoretical, 0.0);
    }

    #[test]
    fn lap_clock_reset_without_the_line_starts_a_fresh_lap() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        // A split from the abandoned lap must not carry into the new lap.
        inp.cars[0].pct = 0.6;
        inp.player.lap_time = 60.0;
        inp.player.sector = 1;
        s.update(&inp);
        assert_eq!(s.state.player.lap.sectors, [60.0, 0.0]);

        // An invalid lap abandoned halfway (back to the pits): the clock restarts.
        inp.cars[0].pct = 0.5;
        inp.player.lap_time = 50.0;
        inp.player.lap_valid = false;
        s.update(&inp);
        assert!(!s.state.player.lap.valid);
        inp.player.lap_valid = true;
        for k in 0..30 {
            inp.player.lap_time = k as f64 * 0.1;
            s.update(&inp);
        }
        assert!(s.state.player.lap.valid);
        assert_eq!(s.state.player.lap.sectors, [0.0, 0.0]);
        assert_eq!(
            s.state.player.lap.sector_state,
            [SectorState::Pending, SectorState::Pending]
        );

        // Complete the new lap and ensure its sectors, not the abandoned lap's split,
        // drive the theoretical time.
        drive(&mut s, &mut inp, 1, 100.0, 200, 3.0);
        assert_eq!(s.state.player.lap.best_sectors, [50.0, 50.0]);
        assert_eq!(s.state.player.lap.theoretical, 100.0);
    }

    #[test]
    fn standing_start_restarts_the_delta_trace() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        // on the grid behind the line, in the last sector, with the clock stopped
        let mut inp = input(vec![car(0, 0, 0.8)]);
        inp.player.sector = 1;
        for _ in 0..10 {
            inp.t += 0.1;
            s.update(&inp);
        }
        // across the line (which doesn't count as a lap), then one full lap
        drive(&mut s, &mut inp, 1, 100.0, 400, 3.0);
        inp.cars[0].pct = 0.5;
        inp.player.lap_time = 51.0;
        s.update(&inp);
        let d = s.state.player.lap.delta;
        assert!((d - 1.0).abs() < 0.3, "{d}");
    }

    #[test]
    fn game_delta_wins_and_sector_deltas_add_up() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 400, 3.0);
        assert_eq!(s.state.player.lap.sector_deltas, vec![None, None]);
        let at = |s: &mut Session, inp: &mut Input, pct: f64, sector: usize, d: f64| {
            inp.t += 1.0;
            inp.cars[0].pct = pct;
            inp.player.lap_time = pct * 100.0;
            inp.player.sector = sector;
            inp.player.delta = Some(d);
            s.update(inp);
        };
        at(&mut s, &mut inp, 0.25, 0, -0.2);
        assert_eq!(s.state.player.lap.sector_deltas, vec![Some(-0.2), None]);
        at(&mut s, &mut inp, 0.5, 1, -0.3);
        at(&mut s, &mut inp, 0.75, 1, -0.1);
        let lap = &s.state.player.lap;
        assert_eq!(lap.delta, -0.1);
        assert!((lap.predicted - 99.9).abs() < 1e-9);
        let [Some(a), Some(b)] = lap.sector_deltas[..] else {
            panic!("{:?}", lap.sector_deltas)
        };
        assert!((a + 0.3).abs() < 1e-9 && (b - 0.2).abs() < 1e-9, "{a} {b}");
        // over the line: the finished lap's deltas are kept, the new lap starts empty
        inp.t += 1.0;
        inp.cars[0].laps += 1;
        inp.cars[0].pct = 0.0;
        inp.cars[0].last_lap = 99.9;
        inp.player.lap_time = 0.0;
        inp.player.sector = 0;
        inp.player.delta = Some(0.0);
        s.update(&inp);
        let lap = &s.state.player.lap;
        assert_eq!(lap.last_sector_deltas.len(), 2);
        assert!(lap.last_sector_deltas.iter().all(Option::is_some));
        assert_eq!(lap.sector_deltas, vec![Some(0.0), None]);
    }

    #[test]
    fn fuel_average_ignores_refuel_and_estimates_to_add() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.0)]);
        s.update(&inp);
        drive(&mut s, &mut inp, 3, 100.0, 100, 3.0);
        let f = &s.state.player.fuel;
        assert!((f.per_lap - 3.0).abs() < 1e-3, "{}", f.per_lap);
        assert!((f.laps_left - f.liters / 3.0).abs() < 1e-3);
        // refuel (while stationary): liters jump up, the lap's use stays the fuel actually burnt
        inp.player.fuel += 40.0;
        s.update(&inp);
        drive(&mut s, &mut inp, 1, 100.0, 100, 3.0);
        assert!((s.state.player.fuel.last_lap_use - 3.0).abs() < 1e-3);
        let f = &s.state.player.fuel;
        assert!(f.to_finish > 0.0 && f.to_add >= 0.0);
    }

    #[test]
    fn pit_lane_marks_in_and_out_laps_and_counts_stops() {
        let mut s = Session::new(Game::Demo, Capabilities::default());
        let mut inp = input(vec![car(0, 0, 0.5), car(1, 4, 0.96)]);
        s.update(&inp);
        inp.cars[1].in_pit = true;
        s.update(&inp);
        inp.cars[1].laps = 5;
        inp.cars[1].pct = 0.03;
        inp.cars[1].last_lap = 130.0;
        s.update(&inp);
        inp.cars[1].in_pit = false;
        s.update(&inp);
        assert!(s.state.cars[1].out_lap);
        inp.cars[1].laps = 6;
        inp.cars[1].pct = 0.0;
        inp.cars[1].last_lap = 140.0;
        s.update(&inp);
        let c = &s.state.cars[1];
        assert_eq!(c.pit_stops, 1);
        assert_eq!(c.pit_laps, vec![5, 6]);
        assert!(!c.out_lap);
        assert_eq!(c.lap_times, vec![130.0, 140.0]);
    }
}
