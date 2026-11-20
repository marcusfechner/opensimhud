//! Assetto Corsa EVO shared-memory decoding.
//!
//! The physics and graphics page layouts follow the EVO shared-memory guide. Inputs,
//! lap timing, player data, and world positions were checked against 0.9.1 driving captures.

pub mod live;
mod logs;
pub(crate) mod outline;
pub mod recording;
mod sector_learning;

use crate::adapter::{AdapterEvent, GameAdapter};
use crate::adapters::outline::{CandidateHistory, OutlineCandidate, RouteTopology, SamplingDomain};
use crate::input::{CarInput, Input, PlayerInput, SessionInput};
use crate::model::{Capabilities, Connection, Flag, Game, SessionKind, Track, Weather};
use outline::ObservedLearner;
use sector_learning::{
    EvoLearningWorker, EvoTrackKey, FALLBACK_SECTORS as EVO_FALLBACK_SECTORS, PendingOpenRun,
    SessionRequest, TelemetrySample, WorkerResult, default_log_dir,
};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::time::Instant;

const SLIP_ANGLE_OFFSET: usize = 656;

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
    // Keep the display projection behavior unchanged; the learner uses the segment projection
    // below for sub-bin boundary interpolation and a tighter distance threshold.
    let divisor = if track.closed {
        track.x.len()
    } else {
        track.x.len().saturating_sub(1)
    };
    (distance_squared < 100.0 * 100.0 && divisor > 0).then_some(index as f64 / divisor as f64)
}

fn signed_progress_delta(from: f64, to: f64) -> f64 {
    (to - from + 0.5).rem_euclid(1.0) - 0.5
}

fn project_to_track_quality(
    track: &Track,
    x: f32,
    y: f32,
    previous_pct: Option<f64>,
    maximum_progress_delta: f64,
) -> Option<(f64, f64)> {
    if track.x.len() < 3 || track.x.len() != track.y.len() {
        return None;
    }
    let n = track.x.len();
    let count = if track.closed { n } else { n - 1 };
    let mut best: Option<(f64, f64)> = None;
    for index in 0..count {
        let next = (index + 1) % n;
        let (ax, ay) = (track.x[index], track.y[index]);
        let (bx, by) = (track.x[next], track.y[next]);
        let (dx, dy) = (bx - ax, by - ay);
        let length_squared = dx * dx + dy * dy;
        if !length_squared.is_finite() || length_squared <= f32::EPSILON {
            continue;
        }
        let along = (((x - ax) * dx + (y - ay) * dy) / length_squared).clamp(0.0, 1.0);
        let px = ax + along * dx;
        let py = ay + along * dy;
        let distance_squared = (x - px).powi(2) + (y - py).powi(2);
        let pct = (index as f64 + f64::from(along)) / count as f64;
        if previous_pct.is_some_and(|previous| {
            (if track.closed {
                signed_progress_delta(previous, pct)
            } else {
                pct - previous
            })
            .abs()
                > maximum_progress_delta
        }) {
            continue;
        }
        if best.is_none_or(|(_, distance)| (distance_squared as f64) < distance) {
            best = Some((pct, f64::from(distance_squared)));
        }
    }
    let (pct, distance_squared) = best?;
    (distance_squared < 100.0 * 100.0).then_some((pct, distance_squared.sqrt()))
}

fn sector_at(pct: f64, starts: &[f64]) -> usize {
    if !pct.is_finite() {
        return 0;
    }
    let pct = pct.clamp(0.0, 1.0);
    starts
        .partition_point(|start| *start <= pct)
        .saturating_sub(1)
        .min(starts.len() - 1)
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
    learner: ObservedLearner,
    outline_history: CandidateHistory,
    pending_outline: Option<(EvoTrackKey, Track, bool)>,
    pending_candidate: Option<(u64, u32, OutlineCandidate)>,
    pending_open: Option<PendingOpenRun>,
    track_cache: Option<PathBuf>,
    live_learning: bool,
    selected_active: bool,
    track_key: Option<EvoTrackKey>,
    track_name: String,
    track_configuration: String,
    session_name: String,
    session_kind: SessionKind,
    session_epoch: u64,
    session_started: bool,
    selection_open: bool,
    active_sectors: Vec<f64>,
    last_quality_pct: Option<f64>,
    last_quality_seconds: f64,
    reverse_progress: f64,
    outline_sample_at: f64,
    outline_started: bool,
    outline_outlap: bool,
    outline_last_pit: bool,
    outline_all_solo: bool,
    open_route: bool,
    last_open_clock: i32,
    last_open_completed: i32,
    outline_previous_sample: Option<(f64, i32, f32, f32)>,
    outline_previous_packet: Option<i32>,
    outline_stalled_seconds: f64,
    outline_stationary_seconds: f64,
    worker: Option<EvoLearningWorker>,
    retired_workers: Vec<EvoLearningWorker>,
    last_worker_sample: f64,
    last_laps: Option<u32>,
    last_session_lap: Option<u32>,
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
        Self::build(track_cache, false, false)
    }

    /// Create the live-only adapter. Replay and preview paths use `new` and never inspect EVO
    /// logs or touch the user's persistent cache.
    pub fn new_live(track_cache: Option<PathBuf>) -> Self {
        Self::build(track_cache, cfg!(windows), true)
    }

    /// A live source used for auto-detection; it must be selected before it starts background I/O.
    pub fn new_monitor(track_cache: Option<PathBuf>) -> Self {
        Self::build(track_cache, cfg!(windows), false)
    }

    fn build(track_cache: Option<PathBuf>, live_learning: bool, selected_active: bool) -> Self {
        Self {
            pages: None,
            input: Input {
                t: 0.0,
                track: Track {
                    sectors: EVO_FALLBACK_SECTORS.to_vec(),
                    ..Track::default()
                },
                session: SessionInput {
                    generation: 0,
                    kind: SessionKind::Practice,
                    name: String::new(),
                    duration: 0.0,
                    elapsed: 0.0,
                    time_remaining: None,
                    lap_remaining: None,
                    current_lap: None,
                    fixed_total_laps: None,
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
            learner: ObservedLearner::new(),
            outline_history: CandidateHistory::default(),
            pending_outline: None,
            pending_candidate: None,
            pending_open: None,
            track_cache,
            live_learning,
            selected_active,
            track_key: None,
            track_name: String::new(),
            track_configuration: String::new(),
            session_name: String::new(),
            session_kind: SessionKind::Practice,
            session_epoch: 0,
            session_started: false,
            selection_open: false,
            active_sectors: EVO_FALLBACK_SECTORS.to_vec(),
            last_quality_pct: None,
            last_quality_seconds: 0.0,
            reverse_progress: 0.0,
            outline_sample_at: -1.0,
            outline_started: false,
            outline_outlap: false,
            outline_last_pit: false,
            outline_all_solo: true,
            open_route: false,
            last_open_clock: 0,
            last_open_completed: 0,
            outline_previous_sample: None,
            outline_previous_packet: None,
            outline_stalled_seconds: 0.0,
            outline_stationary_seconds: 0.0,
            worker: None,
            retired_workers: Vec::new(),
            last_worker_sample: -1.0,
            last_laps: None,
            last_session_lap: None,
            last_position: None,
            lap_distance: 0.0,
        }
    }

    fn ensure_worker(&mut self) -> bool {
        self.reap_retired_workers();
        if self
            .worker
            .as_ref()
            .is_some_and(EvoLearningWorker::is_finished)
            && let Some(mut worker) = self.worker.take()
        {
            worker.join();
            self.retired_workers.push(worker);
            self.pending_outline = None;
            self.pending_candidate = None;
            self.pending_open = None;
            self.learner.reject_current_lap();
            self.last_quality_pct = None;
        }
        if self.worker.is_some() {
            return false;
        }
        self.worker = Some(EvoLearningWorker::spawn(
            default_log_dir(),
            self.track_cache.clone(),
        ));
        self.last_worker_sample = -1.0;
        true
    }

    fn stop_worker(&mut self) {
        self.pending_outline = None;
        self.pending_candidate = None;
        self.pending_open = None;
        self.learner.reject_current_lap();
        self.last_quality_pct = None;
        if let Some(mut worker) = self.worker.take() {
            worker.stop();
            self.retired_workers.push(worker);
        }
        self.reap_retired_workers();
    }

    fn reap_retired_workers(&mut self) {
        let mut index = 0;
        while index < self.retired_workers.len() {
            if self.retired_workers[index].is_finished() {
                let mut worker = self.retired_workers.remove(index);
                worker.join();
            } else {
                index += 1;
            }
        }
    }

    fn start_worker_session(&self, key: EvoTrackKey, track_length_m: f64) {
        if let Some(worker) = &self.worker {
            worker.start_session(SessionRequest {
                epoch: self.session_epoch,
                key,
                track_name: self.track_name.clone(),
                track_length_m,
            });
        }
    }

    fn apply_worker_results(&mut self) {
        loop {
            let result = self.worker.as_ref().map(EvoLearningWorker::try_result);
            match result {
                Some(Ok(WorkerResult::SessionLoaded {
                    epoch,
                    sectors,
                    outline,
                })) if epoch == self.session_epoch => {
                    if self.selection_open
                        && let Some((track, valid)) = outline
                        && (self.input.track.x.is_empty()
                            || track.geometry_revision > self.input.track.geometry_revision
                            || (track.geometry_revision == self.input.track.geometry_revision
                                && track.x == self.input.track.x
                                && track.y == self.input.track.y))
                    {
                        self.active_sectors = if self.open_route {
                            vec![0.0]
                        } else {
                            sectors.unwrap_or_else(|| EVO_FALLBACK_SECTORS.to_vec())
                        };
                        self.learner.adopt(&track, valid);
                        self.input.track.geometry_revision = track.geometry_revision.max(1);
                        self.input.track.x = track.x;
                        self.input.track.y = track.y;
                        self.input.track.sectors = self.active_sectors.to_vec();
                        self.last_quality_pct = None;
                        self.last_quality_seconds = self.input.t;
                    }
                }
                Some(Ok(WorkerResult::SessionLoaded { .. })) => {}
                Some(Ok(WorkerResult::OpenBootstrap {
                    epoch,
                    source_revision,
                    track,
                    valid,
                })) if epoch == self.session_epoch
                    && source_revision == self.input.track.geometry_revision
                    && (self.input.track.x.is_empty()
                        || (!self.learner.outline_valid && valid)) =>
                {
                    self.learner.adopt(&track, valid);
                    self.input.track = track;
                    self.active_sectors = if self.open_route {
                        vec![0.0]
                    } else {
                        EVO_FALLBACK_SECTORS.to_vec()
                    };
                    self.input.track.sectors.clone_from(&self.active_sectors);
                    self.last_quality_pct = None;
                }
                Some(Ok(WorkerResult::OpenBootstrap { .. })) => {}
                Some(Err(TryRecvError::Empty | TryRecvError::Disconnected)) | None => break,
            }
        }
    }

    fn flush_outline_messages(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        if let Some((key, track, valid)) = self.pending_outline.take()
            && let Err(pending) = worker.save_outline(key, track, valid)
        {
            self.pending_outline = Some(*pending);
            return;
        }
        if let Some((epoch, revision, candidate)) = self.pending_candidate.take()
            && let Err(candidate) = worker.outline_candidate(epoch, revision, candidate)
        {
            self.pending_candidate = Some((epoch, revision, candidate));
        }
        if let Some(run) = self.pending_open.take()
            && let Err(run) = worker.open_run(run)
        {
            self.pending_open = Some(*run);
        }
    }

    fn apply(&mut self, now: f64, p: Physics, g: Graphics, s: Static) {
        self.input.t = now;
        let laps = g.total_lap_count.max(0) as u32;
        let session_lap = g.current_session_lap.max(0) as u32;
        let kind = session_kind(&s.session_name);
        let key = EvoTrackKey::new(&s.track, &s.configuration, f64::from(s.track_length));
        let track_changed = self.track_name != s.track
            || self.track_configuration != s.configuration
            || self.track_key != key;
        let lap_counter_reset = self.last_laps.is_some_and(|previous| laps < previous);
        let session_lap_reset = self
            .last_session_lap
            .is_some_and(|previous| previous > session_lap && session_lap <= 1);
        let new_session = !self.session_started
            || track_changed
            || self.session_name != s.session_name
            || self.session_kind != kind
            || lap_counter_reset
            || session_lap_reset;

        if new_session {
            self.pending_outline = None;
            self.pending_candidate = None;
            self.pending_open = None;
            let mut previous_outline = (!track_changed && !self.input.track.x.is_empty())
                .then(|| (self.input.track.clone(), self.learner.outline_valid));
            if !self.live_learning
                && let Some((track, true)) = previous_outline.as_mut()
                && let Some(key) = &self.track_key
            {
                let reference = OutlineCandidate {
                    track_key: format!("{:016x}", key.hash()),
                    topology: if track.closed {
                        RouteTopology::Closed
                    } else {
                        RouteTopology::Open
                    },
                    domain: SamplingDomain::MeasuredDistance,
                    valid: true,
                    session_id: 0,
                    points: track
                        .x
                        .iter()
                        .copied()
                        .zip(track.y.iter().copied())
                        .collect(),
                };
                if track.geometry_revision < u32::MAX - 1
                    && let Some(selected) = self
                        .outline_history
                        .select(&reference, self.outline_history.session_count() == 1)
                {
                    track.x = selected.points.iter().map(|point| point.0).collect();
                    track.y = selected.points.iter().map(|point| point.1).collect();
                    track.geometry_revision = track.geometry_revision.saturating_add(1);
                }
            }
            if track_changed {
                self.outline_history = CandidateHistory::default();
            }
            self.session_epoch = self.session_epoch.wrapping_add(1);
            self.session_started = true;
            self.track_key.clone_from(&key);
            self.track_name.clone_from(&s.track);
            self.track_configuration.clone_from(&s.configuration);
            self.session_name.clone_from(&s.session_name);
            self.session_kind = kind;
            let configuration = s.configuration.to_ascii_lowercase();
            self.open_route =
                configuration.contains("touristenfahrten") || configuration.contains("tourist");
            if track_changed {
                self.active_sectors = if self.open_route {
                    vec![0.0]
                } else {
                    EVO_FALLBACK_SECTORS.to_vec()
                };
            }
            self.selection_open = laps == 0 && g.lap_time_ms <= 1_000;
            self.learner = ObservedLearner::new();
            self.last_laps = None;
            self.last_session_lap = None;
            self.last_position = None;
            self.last_quality_pct = None;
            self.last_quality_seconds = now;
            self.reverse_progress = 0.0;
            self.outline_sample_at = -1.0;
            self.outline_started = g.lap_time_ms <= 1_000 && !g.is_in_pit_lane;
            self.outline_outlap = g.is_in_pit_lane;
            self.outline_last_pit = g.is_in_pit_lane;
            self.outline_all_solo = g.active_cars == 1;
            self.last_open_clock = g.lap_time_ms;
            self.last_open_completed = g.last_lap_time_ms;
            self.outline_previous_sample = None;
            self.outline_previous_packet = None;
            self.outline_stalled_seconds = 0.0;
            self.outline_stationary_seconds = 0.0;
            self.lap_distance = 0.0;
            self.input.track = Track {
                name: s.track.clone(),
                length: f64::from(s.track_length),
                sectors: self.active_sectors.to_vec(),
                closed: !self.open_route,
                ..Track::default()
            };
            if let Some((track, valid)) = previous_outline {
                self.learner.adopt(&track, valid);
                self.input.track = track;
            }
            self.input.session.generation = self.session_epoch;
            if self.live_learning && self.selected_active {
                if let Some(key) = key.as_ref() {
                    self.ensure_worker();
                    self.start_worker_session(key.clone(), f64::from(s.track_length));
                } else {
                    self.stop_worker();
                }
            }
        } else if self.selection_open && (laps > 0 || g.lap_time_ms > 1_000) {
            self.selection_open = false;
        }
        if !new_session
            && self.live_learning
            && self.selected_active
            && let Some(key) = key.as_ref()
            && self.ensure_worker()
        {
            self.start_worker_session(key.clone(), f64::from(s.track_length));
        }
        self.input.track.length = s.track_length as f64;
        self.input.session.generation = self.session_epoch;
        self.input.session.kind = kind;
        // The session lap follows the leader in races; total_lap_count follows the player.
        self.input.session.current_lap = if kind == SessionKind::Race {
            (g.current_session_lap > 0 && g.total_lap_count >= 0).then_some(laps.saturating_add(1))
        } else {
            u32::try_from(g.current_session_lap)
                .ok()
                .filter(|lap| *lap > 0)
        };
        // Recorded lap-limited races report a positive target; timed races report zero.
        self.input.session.fixed_total_laps = (kind == SessionKind::Race)
            .then(|| {
                u32::try_from(g.total_session_laps)
                    .ok()
                    .filter(|laps| *laps > 0)
            })
            .flatten();
        self.input.session.name.clone_from(&s.session_name);
        self.apply_worker_results();
        self.flush_outline_messages();
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
        self.input.player.slip_angles_deg = p.slip_angles_deg;
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

        if self.open_route
            && !new_session
            && self.last_laps == Some(laps)
            && self.last_open_clock > 5_000
            && (0..=1_000).contains(&g.lap_time_ms)
        {
            let [x, _, z] = g.car_coordinates[0];
            if !self.open_route || !g.is_in_pit_lane {
                self.learner.sample_observed_path(
                    (g.active_cars > 0).then_some((x, z)),
                    !g.is_in_pit_lane && !self.outline_outlap && self.outline_started,
                    g.is_valid_lap,
                );
            }
            let fresh_timing = g.last_lap_time_ms > 0
                && g.last_lap_time_ms != self.last_open_completed
                && g.last_lap_time_ms.abs_diff(self.last_open_clock) <= 500;
            let run = self.learner.finish_pending_open(f64::from(s.track_length));
            if fresh_timing
                && self.outline_all_solo
                && let Some(((x, y), valid)) = run
                && self.worker.is_some()
            {
                let mut track = self.input.track.clone();
                track.x = x;
                track.y = y;
                track.closed = false;
                self.pending_open = Some(PendingOpenRun {
                    epoch: self.session_epoch,
                    source_revision: self.input.track.geometry_revision,
                    track,
                    valid,
                    lap_ms: g.last_lap_time_ms as u32,
                });
                self.flush_outline_messages();
            }
            // On Touristenfahrten the timer also resets at the route entrance,
            // several seconds after leaving the pit lane, without a new lap time.
            self.outline_started = !g.is_in_pit_lane;
            self.outline_outlap = g.is_in_pit_lane;
            self.outline_all_solo = g.active_cars == 1;
            self.outline_sample_at = -1.0;
            self.outline_previous_sample = None;
            self.outline_previous_packet = None;
            self.outline_stalled_seconds = 0.0;
            self.outline_stationary_seconds = 0.0;
            self.reverse_progress = 0.0;
            self.last_quality_pct = None;
            self.last_position = None;
        }
        if let Some(previous) = self.last_laps {
            if laps > previous {
                if self.outline_previous_packet == Some(p.packet) {
                    self.learner.reject_current_lap();
                }
                let [x, _, z] = g.car_coordinates[0];
                if !self.open_route || !g.is_in_pit_lane {
                    self.learner.sample_observed_path(
                        (g.active_cars > 0).then_some((x, z)),
                        !g.is_in_pit_lane && !self.outline_outlap && self.outline_started,
                        g.is_valid_lap,
                    );
                }
                if g.last_lap_time_ms <= 0 {
                    self.learner.reject_current_lap();
                }
                let built = if self.open_route {
                    self.learner.lap_done_open(f64::from(s.track_length))
                } else {
                    self.learner.lap_done_normalized(f64::from(s.track_length))
                };
                let candidate = self.learner.take_candidate().and_then(|(x, y)| {
                    self.track_key.as_ref().map(|key| OutlineCandidate {
                        track_key: format!("{:016x}", key.hash()),
                        topology: if self.open_route {
                            RouteTopology::Open
                        } else {
                            RouteTopology::Closed
                        },
                        domain: SamplingDomain::MeasuredDistance,
                        valid: true,
                        session_id: self.session_epoch,
                        points: x.into_iter().zip(y).collect(),
                    })
                });
                if !self.live_learning
                    && let Some(candidate) = &candidate
                {
                    self.outline_history.observe(candidate.clone());
                }
                if built && let Some((x, y)) = self.learner.outline.clone() {
                    if !self.input.track.x.is_empty()
                        && let Some(worker) = &self.worker
                    {
                        worker.invalidate_lap(self.session_epoch);
                    }
                    if self.input.track.x != x || self.input.track.y != y {
                        self.input.track.geometry_revision =
                            self.input.track.geometry_revision.saturating_add(1);
                    }
                    self.input.track.x = x;
                    self.input.track.y = y;
                    self.input.track.sectors = self.active_sectors.to_vec();
                    if self.worker.is_some()
                        && let Some(key) = &self.track_key
                    {
                        self.pending_outline = Some((
                            key.clone(),
                            self.input.track.clone(),
                            self.learner.outline_valid,
                        ));
                    }
                    self.last_quality_pct = None;
                    self.last_quality_seconds = now;
                }
                if let Some(candidate) = candidate
                    && self.worker.is_some()
                {
                    self.pending_candidate = Some((
                        self.session_epoch,
                        self.input.track.geometry_revision,
                        candidate,
                    ));
                }
                self.flush_outline_messages();
                self.lap_distance = 0.0;
                self.reverse_progress = 0.0;
                self.outline_started = g.last_lap_time_ms > 0 && !g.is_in_pit_lane;
                self.outline_outlap = g.is_in_pit_lane;
                self.outline_all_solo = g.active_cars == 1;
                self.outline_sample_at = -1.0;
                self.outline_previous_sample = None;
                self.outline_previous_packet = None;
                self.outline_stalled_seconds = 0.0;
                self.outline_stationary_seconds = 0.0;
                self.last_position = None;
            } else if laps < previous {
                self.learner = ObservedLearner::new();
                self.lap_distance = 0.0;
                self.last_position = None;
                self.last_quality_pct = None;
                self.last_quality_seconds = now;
            }
        }
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
                model: if is_player {
                    g.car_model.clone()
                } else {
                    String::new()
                },
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
                position: if is_player && g.current_position < g.total_drivers {
                    Some(g.current_position + 1)
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
        let fallback_pct =
            (self.lap_distance / s.track_length.max(1.0) as f64).clamp(0.0, 0.999_999);
        let quality_projection = if self.learner.outline_valid && player_located {
            let dt = (now - self.last_quality_seconds).clamp(0.0, 0.25);
            let maximum_progress_delta = 0.004
                + f64::from(p.speed_kmh / 3.6) * dt * 3.0 / f64::from(s.track_length.max(1.0));
            project_to_track_quality(
                &self.input.track,
                x,
                z,
                self.last_quality_pct,
                maximum_progress_delta,
            )
        } else {
            None
        };
        if let (Some(previous), Some((current, distance))) =
            (self.last_quality_pct, quality_projection)
            && distance <= 30.0
        {
            let delta = if self.open_route {
                current - previous
            } else {
                signed_progress_delta(previous, current)
            };
            self.reverse_progress = if delta < 0.0 {
                self.reverse_progress - delta
            } else {
                0.0
            };
            if self.reverse_progress > 0.005 {
                self.learner.reject_current_lap();
            }
        }
        if self.open_route && self.outline_last_pit && !g.is_in_pit_lane {
            self.learner.reset_capture();
            self.outline_outlap = false;
            self.outline_started = (0..=1_000).contains(&g.lap_time_ms);
            self.outline_sample_at = -1.0;
            self.outline_previous_sample = None;
            self.outline_previous_packet = None;
            self.outline_stalled_seconds = 0.0;
            self.outline_stationary_seconds = 0.0;
        }
        self.outline_last_pit = g.is_in_pit_lane;
        if g.is_in_pit_lane && !self.open_route {
            self.outline_outlap = true;
            self.learner.reject_current_lap();
        }
        self.outline_all_solo &= g.active_cars == 1;
        if (!self.open_route || (!g.is_in_pit_lane && self.outline_started && g.lap_time_ms > 0))
            && (self.outline_sample_at < 0.0 || now - self.outline_sample_at >= 0.1)
        {
            if self.outline_previous_packet == Some(p.packet) {
                self.learner.reject_current_lap();
            }
            let speed = p.speed_kmh / 3.6;
            if !speed.is_finite() || !(0.0..=120.0).contains(&speed) || !player_located {
                self.learner.reject_current_lap();
            }
            if let Some((previous_time, previous_clock, previous_x, previous_z)) =
                self.outline_previous_sample
            {
                let dt = now - previous_time;
                let distance = (x - previous_x).hypot(z - previous_z);
                if !dt.is_finite()
                    || dt <= 0.0
                    || dt > 0.35
                    || !distance.is_finite()
                    || distance > 100.0
                    || f64::from(distance) > (3.0 * f64::from(speed) * dt + 15.0).max(30.0)
                    || g.lap_time_ms < previous_clock
                {
                    self.learner.reject_current_lap();
                }
                if dt > 0.0 {
                    self.outline_stalled_seconds = if g.lap_time_ms == previous_clock {
                        self.outline_stalled_seconds + dt
                    } else {
                        0.0
                    };
                    self.outline_stationary_seconds = if distance < 0.1 {
                        self.outline_stationary_seconds + dt
                    } else {
                        0.0
                    };
                    if self.outline_stalled_seconds > 0.75 || self.outline_stationary_seconds > 0.75
                    {
                        self.learner.reject_current_lap();
                    }
                }
            }
            self.learner.sample_observed_path(
                player_located.then_some((x, z)),
                !g.is_in_pit_lane && !self.outline_outlap && self.outline_started,
                g.is_valid_lap,
            );
            self.outline_sample_at = now;
            self.outline_previous_sample = Some((now, g.lap_time_ms, x, z));
            self.outline_previous_packet = Some(p.packet);
        }
        if player_located {
            let position = (x, z);
            if let Some(previous) = self.last_position {
                let distance = (position.0 - previous.0).hypot(position.1 - previous.1) as f64;
                if distance.is_finite() && distance < 100.0 {
                    self.lap_distance += distance;
                }
            }
            self.last_position = Some(position);
            let map_pct = project_to_track(&self.input.track, x, z);
            let pct = quality_projection
                .map(|(projected_pct, _)| projected_pct)
                .or(self.last_quality_pct)
                .or(map_pct)
                .unwrap_or(fallback_pct);
            if let Some((projected_pct, _)) = quality_projection {
                self.last_quality_pct = Some(projected_pct);
                self.last_quality_seconds = now;
            } else {
                self.last_quality_pct = None;
                self.last_quality_seconds = now;
            }
            self.input.cars[0].pct = pct;
            self.input.player.sector = sector_at(pct, &self.active_sectors);
        } else {
            self.last_position = None;
            self.last_quality_pct = None;
            self.last_quality_seconds = now;
        }

        if self.live_learning
            && self.selected_active
            && now - self.last_worker_sample >= 0.05
            && let Some(worker) = &self.worker
        {
            worker.sample(TelemetrySample {
                epoch: self.session_epoch,
                reference_revision: self.input.track.geometry_revision,
                closed: self.input.track.closed,
                seconds: now,
                lap_counter: laps,
                lap_clock_ms: g.lap_time_ms.max(0) as u32,
                last_lap_ms: g.last_lap_time_ms.max(0) as u32,
                lap_valid: g.is_valid_lap,
                in_pit: g.is_in_pit_lane,
                active_cars: g.active_cars,
                speed_mps: p.speed_kmh / 3.6,
                track_length_m: s.track_length,
                position: player_located.then_some((x, z)),
                projected_pct: quality_projection.map(|(pct, _)| pct),
                projection_distance_m: quality_projection.map(|(_, distance)| distance),
            });
            self.last_worker_sample = now;
        }
        self.last_laps = Some(laps);
        self.last_session_lap = Some(session_lap);
        self.last_open_clock = g.lap_time_ms;
        self.last_open_completed = g.last_lap_time_ms;

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
            slip_angles: true,
            ..Capabilities::default()
        }
    }
    fn set_selected(&mut self, selected: bool) {
        self.selected_active = selected;
        if !selected {
            self.stop_worker();
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
            if !active {
                self.stop_worker();
            }
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

impl Drop for Evo {
    fn drop(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.join();
        }
        for worker in &mut self.retired_workers {
            worker.join();
        }
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
    pub car_model: String,
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
            car_model: string(bytes, 3086, 33),
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
            || !(-1_000_000..=2_400_000).contains(&g.lap_time_ms)
            || !(-1..=1_000_000).contains(&g.total_lap_count)
            || !(-1..=2_400_000).contains(&g.last_lap_time_ms)
            || !(-1..=2_400_000).contains(&g.best_lap_time_ms)
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
    /// Signed per-wheel slip angles normalized to degrees, ordered FL, FR, RL, RR.
    /// The shared-memory source stores these four values in radians at byte 656.
    pub slip_angles_deg: Option<[f32; 4]>,
}

fn decode_slip_angles_degrees(bytes: &[u8]) -> Option<[f32; 4]> {
    let raw = bytes.get(SLIP_ANGLE_OFFSET..SLIP_ANGLE_OFFSET + 16)?;
    let mut radians = [0.0; 4];
    for (index, angle) in radians.iter_mut().enumerate() {
        let at = index * 4;
        *angle = f32::from_le_bytes(raw.get(at..at + 4)?.try_into().ok()?);
    }
    if !radians.iter().all(|angle| angle.is_finite()) {
        return None;
    }
    let degrees = radians.map(f32::to_degrees);
    degrees
        .iter()
        .all(|angle| angle.is_finite())
        .then_some(degrees)
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
            slip_angles_deg: decode_slip_angles_degrees(bytes),
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
    use super::*;

    #[test]
    fn graphics_decodes_player_car_model() {
        let mut bytes = [0; 8192];
        bytes[3086..3098].copy_from_slice(b"Ferrari SF25");
        assert_eq!(Graphics::decode(&bytes).unwrap().car_model, "Ferrari SF25");
    }

    #[test]
    fn sector_lookup_changes_at_the_ordered_boundaries() {
        let starts = [0.0, 0.31, 0.69];
        assert_eq!(sector_at(0.0, &starts), 0);
        assert_eq!(sector_at(0.309_999, &starts), 0);
        assert_eq!(sector_at(0.31, &starts), 1);
        assert_eq!(sector_at(0.689_999, &starts), 1);
        assert_eq!(sector_at(0.69, &starts), 2);
        assert_eq!(sector_at(0.999_999, &starts), 2);
        assert_eq!(sector_at(1.0, &starts), 2);
    }

    #[test]
    fn quality_projection_prefers_the_continuous_branch_at_a_track_crossing() {
        let track = Track {
            x: vec![-2.0, -1.0, 0.0, 1.0, 2.0, 1.0, 0.0, -1.0],
            y: vec![0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, -1.0],
            ..Track::default()
        };
        let projection = project_to_track_quality(&track, 0.0, 0.0, Some(0.68), 0.1).unwrap();
        assert!(
            (projection.0 - 0.75).abs() < 1e-6,
            "expected the exact crossing on the continuous branch, got {projection:?}"
        );
        assert_eq!(projection.1, 0.0);
        assert!((signed_progress_delta(0.999, 0.001) - 0.002).abs() < 1e-9);
        assert!((signed_progress_delta(0.001, 0.999) + 0.002).abs() < 1e-9);
    }

    #[test]
    fn open_projection_clamps_finish_and_never_wraps_to_start() {
        let track = Track {
            closed: false,
            x: vec![0.0, 100.0, 200.0],
            y: vec![0.0; 3],
            ..Track::default()
        };
        let (finish, _) = project_to_track_quality(&track, 200.0, 0.0, Some(0.99), 0.05).unwrap();
        assert_eq!(finish, 1.0);
        assert!(project_to_track_quality(&track, 0.0, 0.0, Some(0.99), 0.05).is_none());
        assert_eq!(project_to_track(&track, 200.0, 0.0), Some(1.0));
    }

    #[test]
    fn open_route_timer_reset_after_pit_exit_starts_capture() {
        let mut evo = Evo::new(None);
        let mut physics = Physics::decode(&[0; 800]).unwrap();
        let mut graphics = Graphics::decode(&[0; 8192]).unwrap();
        let metadata = Static {
            version: "1.0".into(),
            game_version: "0.9.1".into(),
            session_name: "Practice".into(),
            track: "Nurburgring".into(),
            configuration: "Touristenfahrten".into(),
            track_length: 20_000.0,
        };
        graphics.active_cars = 1;
        graphics.is_in_pit_lane = true;
        graphics.lap_time_ms = 30_000;
        evo.apply(0.0, physics, graphics.clone(), metadata.clone());
        graphics.is_in_pit_lane = false;
        graphics.lap_time_ms = 39_000;
        physics.packet += 1;
        evo.apply(0.1, physics, graphics.clone(), metadata.clone());
        assert!(!evo.outline_started);
        graphics.lap_time_ms = 50;
        graphics.is_valid_lap = true;
        physics.packet += 1;
        evo.apply(0.2, physics, graphics, metadata);
        assert!(evo.outline_started);
    }

    #[test]
    fn evo_outline_learner_does_not_invent_sector_boundaries() {
        let mut evo = Evo::new(None);
        for index in 0..3_000 {
            let pct = index as f64 / 3_000.0;
            let angle = pct * std::f64::consts::TAU;
            evo.learner.sample_observed_path(
                Some(((angle.cos() * 500.0) as f32, (angle.sin() * 500.0) as f32)),
                true,
                true,
            );
        }
        assert_eq!(evo.input.track.sectors, EVO_FALLBACK_SECTORS);
    }

    #[test]
    fn race_session_lap_tracks_player_and_fixed_target() {
        let mut evo = Evo::new(None);
        let physics = Physics::decode(&[0; 800]).unwrap();
        let mut graphics = Graphics::decode(&[0; 8192]).unwrap();
        let mut metadata = Static {
            version: "1.0".into(),
            game_version: "0.9.1".into(),
            session_name: "Race".into(),
            track: "Test".into(),
            configuration: "GP".into(),
            track_length: 1000.0,
        };
        graphics.total_session_laps = 20;
        for (lap, expected) in [
            (1, Some(1)),
            (7, Some(1)),
            (0, None),
            (-1, None),
            (1, Some(1)),
        ] {
            graphics.current_session_lap = lap;
            evo.apply(0.0, physics, graphics.clone(), metadata.clone());
            assert_eq!(evo.input.session.current_lap, expected);
            assert_eq!(evo.input.session.fixed_total_laps, Some(20));
        }
        graphics.current_session_lap = 8;
        graphics.total_lap_count = 6;
        evo.apply(0.5, physics, graphics.clone(), metadata.clone());
        assert_eq!(evo.input.session.current_lap, Some(7));
        graphics.total_session_laps = 0;
        evo.apply(0.6, physics, graphics.clone(), metadata.clone());
        assert_eq!(evo.input.session.fixed_total_laps, None);
        graphics.total_session_laps = -1;
        evo.apply(0.7, physics, graphics.clone(), metadata.clone());
        assert_eq!(evo.input.session.fixed_total_laps, None);
        metadata.session_name = "Practice".into();
        graphics.total_session_laps = 20;
        graphics.current_session_lap = 3;
        evo.apply(1.0, physics, graphics, metadata);
        assert_eq!(evo.input.session.current_lap, Some(3));
        assert_eq!(evo.input.session.fixed_total_laps, None);
    }

    #[test]
    fn player_position_is_converted_from_zero_based() {
        let mut evo = Evo::new(None);
        let physics = Physics::decode(&[0; 800]).unwrap();
        let mut graphics = Graphics::decode(&[0; 8192]).unwrap();
        let metadata = Static {
            version: "1.0".into(),
            game_version: "0.9.1".into(),
            session_name: "Race".into(),
            track: "Test".into(),
            configuration: "GP".into(),
            track_length: 1000.0,
        };
        for (position, drivers, expected) in [
            (0, 1, Some(1)),
            (19, 20, Some(20)),
            (20, 20, None),
            (0, 0, None),
        ] {
            graphics.current_position = position;
            graphics.total_drivers = drivers;
            evo.apply(0.0, physics, graphics.clone(), metadata.clone());
            assert_eq!(evo.input.cars[0].position, expected);
        }
    }

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

    #[test]
    fn decodes_slip_angles_from_version_1_physics_offset() {
        let mut b = vec![0u8; 800];
        let radians = [0.1f32, -0.2, 0.3, -0.4];
        for (index, angle) in radians.into_iter().enumerate() {
            let at = SLIP_ANGLE_OFFSET + index * 4;
            b[at..at + 4].copy_from_slice(&angle.to_le_bytes());
        }

        let decoded = Physics::decode(&b).unwrap();
        let actual = decoded.slip_angles_deg.unwrap();
        let expected = radians.map(f32::to_degrees);
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
        }

        b[SLIP_ANGLE_OFFSET..SLIP_ANGLE_OFFSET + 16].fill(0);
        assert_eq!(Physics::decode(&b).unwrap().slip_angles_deg, Some([0.0; 4]));

        assert_eq!(
            Physics::decode(&b[..SLIP_ANGLE_OFFSET + 15])
                .unwrap()
                .slip_angles_deg,
            None,
            "a short slip-angle block is unavailable without invalidating earlier page fields"
        );

        b[SLIP_ANGLE_OFFSET + 8..SLIP_ANGLE_OFFSET + 12]
            .copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert_eq!(Physics::decode(&b).unwrap().slip_angles_deg, None);
    }
}
