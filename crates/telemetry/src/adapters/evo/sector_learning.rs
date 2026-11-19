//! Conservative EVO sector learning from the game log and projected telemetry.
//!
//! File access, parsing, correlation and persistence live on the worker thread. The adapter only
//! sends bounded samples and receives small session-start results without waiting for I/O.

use super::logs::{LogTailer, LoggedLap, SplitParser, completion_time_ms};
use crate::adapters::outline::{CandidateHistory, OutlineCandidate, RouteTopology, SamplingDomain};
use crate::model::Track;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) const FALLBACK_SECTORS: [f64; 3] = [0.0, 1.0 / 3.0, 2.0 / 3.0];

#[cfg(test)]
const SCHEMA_VERSION: u32 = 2;
const MIN_CONFIRM_LAPS: usize = 3;
const MAX_OBSERVATIONS: usize = 64;
const MAX_QUARANTINED: usize = 8;
const MAX_TELEMETRY_LAPS: usize = 8;
const MAX_PENDING_LOG_LAPS: usize = 8;
const MAX_LAP_SAMPLES: usize = 24_000;
const MAX_LAP_HISTORY: Duration = Duration::from_secs(40 * 60);
const SAMPLE_GAP_MAX: f64 = 0.25;
const MATCH_TOLERANCE_MS: u32 = 500;
const MAX_CONFIRMED_DEVIATION: f64 = 0.005;
const MAX_PROMOTION_MAD: f64 = 0.002;
const MIN_SECTOR_WIDTH: f64 = 0.002;
const MAX_PROJECTION_DISTANCE_M: f64 = 30.0;
const MAX_FINISH_POSITION_GAP_M: f64 = 60.0;
const MAX_FINISH_PROGRESS_GAP: f64 = 0.05;
const MAX_SPEED_MPS: f32 = 120.0;
const CHANNEL_CAPACITY: usize = 64;
#[cfg(test)]
const MAX_SIDECAR_BYTES: u64 = 64 * 1024;
#[cfg(test)]
const MAX_LAYOUT_BYTES: u64 = 512 * 1024;
const MAX_DIAGNOSTIC_BYTES: u64 = 128 * 1024;
const MAX_LEARNED_BYTES: u64 = 2 * 1024 * 1024;
const LEARNED_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct EvoTrackKey {
    track: String,
    configuration: String,
    length_m: u32,
    hash: u64,
}

impl EvoTrackKey {
    pub fn new(track: &str, configuration: &str, length_m: f64) -> Option<Self> {
        if track.trim().is_empty()
            || configuration.trim().is_empty()
            || !length_m.is_finite()
            || !(100.0..=100_000.0).contains(&length_m)
        {
            return None;
        }
        let track = canonical(track);
        let configuration = canonical(configuration);
        let length_m = length_m.round() as u32;
        let material = format!("evo\0{track}\0{configuration}\0{length_m}");
        let hash = stable_hash(material.as_bytes());
        Some(Self {
            track,
            configuration,
            length_m,
            hash,
        })
    }

    fn is_valid(&self) -> bool {
        Self::new(&self.track, &self.configuration, f64::from(self.length_m))
            .is_some_and(|recomputed| recomputed == *self)
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    fn topology(&self) -> RouteTopology {
        if self.configuration.contains("touristenfahrten") || self.configuration.contains("tourist")
        {
            RouteTopology::Open
        } else {
            RouteTopology::Closed
        }
    }

    fn outline_key(&self) -> String {
        format!("{:016x}", self.hash)
    }
}

fn canonical(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone)]
pub(super) struct TelemetrySample {
    pub epoch: u64,
    pub reference_revision: u32,
    pub closed: bool,
    pub seconds: f64,
    pub lap_counter: u32,
    pub lap_clock_ms: u32,
    pub last_lap_ms: u32,
    pub lap_valid: bool,
    pub in_pit: bool,
    pub active_cars: u8,
    pub speed_mps: f32,
    pub track_length_m: f32,
    pub position: Option<(f32, f32)>,
    pub projected_pct: Option<f64>,
    pub projection_distance_m: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct SessionRequest {
    pub epoch: u64,
    pub key: EvoTrackKey,
    pub track_name: String,
    pub track_length_m: f64,
}

#[derive(Debug, Clone)]
pub(super) enum WorkerResult {
    SessionLoaded {
        epoch: u64,
        sectors: Option<Vec<f64>>,
        outline: Option<(Track, bool)>,
    },
    OpenBootstrap {
        epoch: u64,
        source_revision: u32,
        track: Track,
        valid: bool,
    },
}

#[derive(Debug, Clone)]
pub(super) struct PendingOpenRun {
    pub epoch: u64,
    pub source_revision: u32,
    pub track: Track,
    pub valid: bool,
    pub lap_ms: u32,
}

enum WorkerMessage {
    Session(SessionRequest),
    Sample(TelemetrySample),
    InvalidateLap {
        epoch: u64,
    },
    OutlineCandidate {
        epoch: u64,
        source_revision: u32,
        candidate: OutlineCandidate,
    },
    OpenRun(PendingOpenRun),
    SaveOutline {
        key: EvoTrackKey,
        track: Track,
        valid: bool,
    },
    Stop,
}

/// One background reader/learner. `try_send` keeps all adapter calls nonblocking.
pub(super) struct EvoLearningWorker {
    tx: SyncSender<WorkerMessage>,
    rx: Receiver<WorkerResult>,
    overflow: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl EvoLearningWorker {
    pub fn spawn(log_dir: PathBuf, track_cache: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let (result_tx, result_rx) = mpsc::sync_channel(8);
        let overflow = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let thread_overflow = Arc::clone(&overflow);
        let thread_running = Arc::clone(&running);
        let join = thread::Builder::new()
            .name("evo-sector-learning".into())
            .spawn(move || {
                worker_loop(
                    rx,
                    result_tx,
                    thread_overflow,
                    thread_running,
                    log_dir,
                    track_cache,
                )
            })
            .ok();
        Self {
            tx,
            rx: result_rx,
            overflow,
            running,
            join,
        }
    }

    pub fn start_session(&self, request: SessionRequest) {
        self.try_send(WorkerMessage::Session(request));
    }

    pub fn sample(&self, sample: TelemetrySample) {
        self.try_send(WorkerMessage::Sample(sample));
    }

    pub fn invalidate_lap(&self, epoch: u64) {
        self.try_send(WorkerMessage::InvalidateLap { epoch });
    }

    pub fn outline_candidate(
        &self,
        epoch: u64,
        source_revision: u32,
        candidate: OutlineCandidate,
    ) -> Result<(), OutlineCandidate> {
        match self.tx.try_send(WorkerMessage::OutlineCandidate {
            epoch,
            source_revision,
            candidate,
        }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(WorkerMessage::OutlineCandidate { candidate, .. }))
            | Err(TrySendError::Disconnected(WorkerMessage::OutlineCandidate {
                candidate, ..
            })) => Err(candidate),
            Err(_) => unreachable!("only an outline message was sent"),
        }
    }

    pub fn open_run(&self, run: PendingOpenRun) -> Result<(), Box<PendingOpenRun>> {
        match self.tx.try_send(WorkerMessage::OpenRun(run)) {
            Ok(()) => Ok(()),
            Err(
                TrySendError::Full(WorkerMessage::OpenRun(run))
                | TrySendError::Disconnected(WorkerMessage::OpenRun(run)),
            ) => Err(Box::new(run)),
            Err(_) => unreachable!("only an open run was sent"),
        }
    }

    pub fn save_outline(
        &self,
        key: EvoTrackKey,
        track: Track,
        valid: bool,
    ) -> Result<(), Box<(EvoTrackKey, Track, bool)>> {
        match self
            .tx
            .try_send(WorkerMessage::SaveOutline { key, track, valid })
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(WorkerMessage::SaveOutline { key, track, valid }))
            | Err(TrySendError::Disconnected(WorkerMessage::SaveOutline { key, track, valid })) => {
                Err(Box::new((key, track, valid)))
            }
            Err(_) => unreachable!("only an outline message was sent"),
        }
    }

    fn try_send(&self, message: WorkerMessage) {
        match self.tx.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.overflow.store(true, Ordering::Release),
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn try_result(&self) -> Result<WorkerResult, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        let _ = self.tx.try_send(WorkerMessage::Stop);
    }

    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_some_and(JoinHandle::is_finished)
    }

    pub fn join(&mut self) {
        self.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EvoLearningWorker {
    fn drop(&mut self) {
        self.stop();
        if self.is_finished()
            && let Some(join) = self.join.take()
        {
            let _ = join.join();
        }
        // If a local filesystem call is in progress, dropping the handle detaches the thread;
        // `running` is already false and the worker exits immediately after that call returns.
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Provisional,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    boundaries: Vec<f64>,
    anchors: Vec<BoundaryAnchor>,
    reference_revision: u32,
    fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryAnchor {
    x_m: f32,
    y_m: f32,
    dir_x: f32,
    dir_y: f32,
    pct_on_revision: f64,
}

/// Reproject only near the old fraction, with direction and ambiguity guards at crossings.
fn project_anchor_on_outline(anchor: &BoundaryAnchor, outline: &Track) -> Option<f64> {
    let n = outline.x.len();
    if n < 2 || outline.y.len() != n || !anchor.pct_on_revision.is_finite() {
        return None;
    }
    let count = if outline.closed { n } else { n - 1 };
    let mut matches = Vec::new();
    for index in 0..count {
        let next = (index + 1) % n;
        let (ax, ay) = (outline.x[index], outline.y[index]);
        let (dx, dy) = (outline.x[next] - ax, outline.y[next] - ay);
        let length_squared = dx * dx + dy * dy;
        if !length_squared.is_finite() || length_squared <= f32::EPSILON {
            continue;
        }
        let length = length_squared.sqrt();
        if (dx * anchor.dir_x + dy * anchor.dir_y) / length < 0.35 {
            continue;
        }
        let along =
            (((anchor.x_m - ax) * dx + (anchor.y_m - ay) * dy) / length_squared).clamp(0.0, 1.0);
        let pct = (index as f64 + f64::from(along)) / count as f64;
        let progress_distance = if outline.closed {
            cyclic_progress_distance(anchor.pct_on_revision, pct)
        } else {
            (anchor.pct_on_revision - pct).abs()
        };
        if progress_distance > 0.05 {
            continue;
        }
        let lateral = (anchor.x_m - (ax + along * dx)).hypot(anchor.y_m - (ay + along * dy));
        if lateral.is_finite() && lateral <= 30.0 {
            matches.push((pct, lateral));
        }
    }
    matches.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    let &(best_pct, best_distance) = matches.first()?;
    if matches.iter().skip(1).any(|&(pct, distance)| {
        let difference = if outline.closed {
            cyclic_progress_distance(best_pct, pct)
        } else {
            (best_pct - pct).abs()
        };
        difference > 0.015 && distance - best_distance <= 2.0
    }) {
        return None;
    }
    Some(if outline.closed {
        best_pct.rem_euclid(1.0)
    } else {
        best_pct.clamp(0.0, 1.0)
    })
}

fn candidates_project_continuously(history: &CandidateHistory, outline: &Track) -> bool {
    history.representatives().all(|candidate| {
        let count = candidate.points.len();
        if count != 1000 {
            return false;
        }
        let closed = candidate.topology == RouteTopology::Closed;
        let segments = if closed { count } else { count - 1 };
        let mut previous: Option<f64> = None;
        for sample in 0..128 {
            let index = if closed {
                sample * count / 128
            } else {
                sample * (count - 1) / 127
            };
            let next = if index + 1 < count {
                index + 1
            } else if closed {
                0
            } else {
                index
            };
            let prior = if next == index { index - 1 } else { index };
            let a = candidate.points[prior];
            let b = candidate.points[next];
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            let length = dx.hypot(dy);
            if !length.is_finite() || length <= f32::EPSILON {
                return false;
            }
            let point = candidate.points[index];
            let anchor = BoundaryAnchor {
                x_m: point.0,
                y_m: point.1,
                dir_x: dx / length,
                dir_y: dy / length,
                pct_on_revision: index as f64 / segments as f64,
            };
            let Some(pct) = project_anchor_on_outline(&anchor, outline) else {
                return false;
            };
            if let Some(previous) = previous {
                let delta = if closed {
                    (pct - previous + 0.5_f64).rem_euclid(1.0) - 0.5
                } else {
                    pct - previous
                };
                if !(-0.005..=0.05).contains(&delta) {
                    return false;
                }
            }
            previous = Some(pct);
        }
        true
    })
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectorFile {
    schema_version: u32,
    key: EvoTrackKey,
    track_length_m: f64,
    status: Status,
    sector_starts: Vec<f64>,
    observations: Vec<Observation>,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutFile {
    schema_version: u32,
    key: EvoTrackKey,
    track: Track,
    from_valid_lap: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvoLearnedFile {
    schema_version: u32,
    key: EvoTrackKey,
    track_length_m: f64,
    topology: RouteTopology,
    sampling_domain: SamplingDomain,
    active_outline: Option<Track>,
    outline_valid: bool,
    next_session_id: u64,
    candidate_pool: CandidateHistory,
    observations: Vec<Observation>,
}

impl EvoLearnedFile {
    fn blank(request: &SessionRequest) -> Self {
        Self {
            schema_version: LEARNED_SCHEMA_VERSION,
            key: request.key.clone(),
            track_length_m: request.track_length_m,
            topology: request.key.topology(),
            sampling_domain: SamplingDomain::MeasuredDistance,
            active_outline: None,
            outline_valid: false,
            next_session_id: 1,
            candidate_pool: CandidateHistory::default(),
            observations: Vec::new(),
        }
    }

    fn valid(&self, request: &SessionRequest) -> bool {
        if self.schema_version != LEARNED_SCHEMA_VERSION
            || self.key != request.key
            || !self.key.is_valid()
            || !self.track_length_m.is_finite()
            || (self.track_length_m - request.track_length_m).abs() > 1.0
            || self.topology != self.key.topology()
            || self.sampling_domain != SamplingDomain::MeasuredDistance
            || self.next_session_id == 0
            || self.next_session_id <= self.candidate_pool.last_session_id()
            || self.observations.len() > MAX_OBSERVATIONS
            || !self.candidate_pool.is_valid(
                &self.key.outline_key(),
                self.topology,
                self.sampling_domain,
            )
            || self.candidate_pool.candidates().any(|candidate| {
                candidate.measured_length().is_none_or(|length| {
                    !(request.track_length_m * 0.7..=request.track_length_m * 1.3).contains(&length)
                })
            })
        {
            return false;
        }
        let Some(track) = &self.active_outline else {
            return self.observations.is_empty() && self.candidate_pool.session_count() == 0;
        };
        if track.name != request.track_name
            || (track.length - request.track_length_m).abs() > 1.0
            || track.geometry_revision == 0
            || track.geometry_revision == u32::MAX
            || track.closed != (self.topology == RouteTopology::Closed)
            || track.x.len() != 1000
            || track.y.len() != 1000
            || !(1..=10).contains(&track.sectors.len())
            || track.sectors.first() != Some(&0.0)
            || track
                .sectors
                .iter()
                .any(|v| !v.is_finite() || !(0.0..1.0).contains(v))
            || track.sectors.windows(2).any(|pair| pair[0] >= pair[1])
            || track
                .x
                .iter()
                .chain(&track.y)
                .any(|v| !v.is_finite() || v.abs() > 100_000.0)
        {
            return false;
        }
        let segments = if track.closed {
            track.x.len()
        } else {
            track.x.len() - 1
        };
        let mut length = 0.0;
        for index in 0..segments {
            let next = (index + 1) % track.x.len();
            let segment =
                f64::from((track.x[next] - track.x[index]).hypot(track.y[next] - track.y[index]));
            if !segment.is_finite() || segment > 100.0 {
                return false;
            }
            length += segment;
        }
        if !(request.track_length_m * 0.7..=request.track_length_m * 1.3).contains(&length) {
            return false;
        }
        let mut fingerprints = HashSet::new();
        self.observations.iter().all(|observation| {
            observation.reference_revision == track.geometry_revision
                && fingerprints.insert(observation.fingerprint)
                && observation.anchors.len() == observation.boundaries.len()
                && valid_sector_boundaries_for_length(&observation.boundaries, self.track_length_m)
                && observation
                    .anchors
                    .iter()
                    .zip(&observation.boundaries)
                    .all(|(anchor, &pct)| {
                        anchor.pct_on_revision == pct
                            && anchor.x_m.is_finite()
                            && anchor.y_m.is_finite()
                            && anchor.dir_x.is_finite()
                            && anchor.dir_y.is_finite()
                            && (anchor.dir_x.hypot(anchor.dir_y) - 1.0).abs() <= 0.01
                            && project_anchor_on_outline(anchor, track).is_some_and(|projected| {
                                let delta = if track.closed {
                                    cyclic_progress_distance(projected, pct)
                                } else {
                                    (projected - pct).abs()
                                };
                                delta <= 0.002
                            })
                    })
        })
    }

    fn load(root: &Path, request: &SessionRequest) -> Option<Self> {
        let path = learned_path(root, &request.key);
        [path.clone(), backup_path(&path)].iter().find_map(|path| {
            if fs::metadata(path).ok()?.len() > MAX_LEARNED_BYTES {
                return None;
            }
            let file: Self = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
            file.valid(request).then_some(file)
        })
    }

    fn save(&self, root: &Path, request: &SessionRequest) -> io::Result<()> {
        if !self.valid(request) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid EVO learned state",
            ));
        }
        let path = learned_path(root, &request.key);
        fs::create_dir_all(path.parent().unwrap_or(root))?;
        for existing in [&path, &backup_path(&path)] {
            if fs::metadata(existing).is_ok_and(|metadata| metadata.len() > MAX_LEARNED_BYTES) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized EVO cache cannot be safely replaced",
                ));
            }
            if read_schema_version(existing, MAX_LEARNED_BYTES)
                .is_some_and(|version| version > u64::from(LEARNED_SCHEMA_VERSION))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "newer EVO learned state",
                ));
            }
            if fs::metadata(existing).is_ok_and(|metadata| metadata.len() <= MAX_LEARNED_BYTES)
                && fs::read(existing)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
                    .is_some_and(|other| other.key != self.key)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "EVO learned cache key collision",
                ));
            }
        }
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_LEARNED_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "EVO learned state too large",
            ));
        }
        let backup = backup_path(&path);
        let read_valid = |source: &Path| {
            fs::metadata(source).is_ok_and(|metadata| metadata.len() <= MAX_LEARNED_BYTES)
                && fs::read(source)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
                    .is_some_and(|file| file.valid(request))
        };
        if path.exists() && !read_valid(&path) && read_valid(&backup) {
            fs::remove_file(&path)?;
        }
        atomic_replace(&path, &bytes)
    }
}

fn learned_path(root: &Path, key: &EvoTrackKey) -> PathBuf {
    root.join("learned")
        .join(format!("{:016x}.json", key.hash()))
}

#[derive(Clone, Default)]
struct SectorEstimator {
    observations: VecDeque<Observation>,
    quarantined: VecDeque<Observation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObserveResult {
    Accepted,
    Duplicate,
    Invalid,
    Quarantined { new_cluster: bool },
}

impl SectorEstimator {
    fn migrate_to(&self, outline: &Track, revision: u32, track_length_m: f64) -> Option<Self> {
        if revision == 0 {
            return None;
        }
        let old_confirmed = self.confirmed_starts();
        let mut migrated = Self::default();
        for observation in &self.observations {
            let mut updated = observation.clone();
            let mut fractions = Vec::with_capacity(updated.anchors.len());
            let mut failed = false;
            for anchor in &mut updated.anchors {
                if let Some(pct) = project_anchor_on_outline(anchor, outline) {
                    anchor.pct_on_revision = pct;
                    fractions.push(pct);
                } else {
                    failed = true;
                    break;
                }
            }
            if failed || !valid_sector_boundaries_for_length(&fractions, track_length_m) {
                continue;
            }
            updated.boundaries = fractions;
            updated.reference_revision = revision;
            migrated.observations.push_back(updated);
        }
        if let Some(old) = old_confirmed {
            let new = migrated.confirmed_starts()?;
            if old.len() != new.len() {
                return None;
            }
        }
        Some(migrated)
    }

    #[cfg(test)]
    fn from_file(path: &Path, key: &EvoTrackKey, actual_length: f64) -> Self {
        let file = read_sector_file(path)
            .filter(|file| valid_sector_file(file, key, actual_length))
            .or_else(|| {
                read_sector_file(&backup_path(path))
                    .filter(|file| valid_sector_file(file, key, actual_length))
            });
        let Some(file) = file else {
            return Self::default();
        };
        Self {
            observations: file.observations.into(),
            quarantined: VecDeque::new(),
        }
    }

    #[cfg(test)]
    fn observe(&mut self, s2: f64, s3: f64, fingerprint: u64) -> bool {
        self.observe_detailed(s2, s3, fingerprint) == ObserveResult::Accepted
    }

    #[cfg(test)]
    fn observe_detailed(&mut self, s2: f64, s3: f64, fingerprint: u64) -> ObserveResult {
        self.observe_boundaries(vec![s2, s3], fingerprint)
    }

    #[cfg(test)]
    fn observe_boundaries(&mut self, boundaries: Vec<f64>, fingerprint: u64) -> ObserveResult {
        let anchors = boundaries
            .iter()
            .map(|&pct| BoundaryAnchor {
                x_m: (pct * 100.0) as f32,
                y_m: 0.0,
                dir_x: 1.0,
                dir_y: 0.0,
                pct_on_revision: pct,
            })
            .collect();
        self.observe_anchors(anchors, 1, fingerprint)
    }

    fn observe_anchors(
        &mut self,
        anchors: Vec<BoundaryAnchor>,
        reference_revision: u32,
        fingerprint: u64,
    ) -> ObserveResult {
        let boundaries: Vec<_> = anchors
            .iter()
            .map(|anchor| anchor.pct_on_revision)
            .collect();
        if !valid_sector_boundaries(&boundaries, MIN_SECTOR_WIDTH)
            || reference_revision == 0
            || self
                .observations
                .front()
                .is_some_and(|item| item.reference_revision != reference_revision)
            || anchors.iter().any(|a| {
                !a.x_m.is_finite()
                    || !a.y_m.is_finite()
                    || !a.dir_x.is_finite()
                    || !a.dir_y.is_finite()
                    || (a.dir_x.hypot(a.dir_y) - 1.0).abs() > 0.01
            })
        {
            return ObserveResult::Invalid;
        }
        if self
            .observations
            .iter()
            .any(|item| item.fingerprint == fingerprint)
            || self
                .quarantined
                .iter()
                .any(|item| item.fingerprint == fingerprint)
        {
            return ObserveResult::Duplicate;
        }
        if let Some(starts) = self.confirmed_starts()
            && (starts.len() != boundaries.len() + 1
                || boundaries
                    .iter()
                    .zip(&starts[1..])
                    .any(|(a, b)| (a - b).abs() > MAX_CONFIRMED_DEVIATION)
                || self.observations.iter().any(|old| {
                    old.anchors.len() == anchors.len()
                        && old
                            .anchors
                            .iter()
                            .zip(&anchors)
                            .any(|(a, b)| (a.x_m - b.x_m).hypot(a.y_m - b.y_m) > 35.0)
                }))
        {
            self.quarantined.push_back(Observation {
                boundaries,
                anchors,
                reference_revision,
                fingerprint,
            });
            while self.quarantined.len() > MAX_QUARANTINED {
                self.quarantined.pop_front();
            }
            return ObserveResult::Quarantined {
                new_cluster: stable_cluster(self.quarantined.iter().collect()).1,
            };
        }
        self.observations.push_back(Observation {
            boundaries,
            anchors,
            reference_revision,
            fingerprint,
        });
        while self.observations.len() > MAX_OBSERVATIONS {
            self.observations.pop_front();
        }
        ObserveResult::Accepted
    }

    fn status_and_starts(&self) -> (Status, Vec<f64>) {
        let (cluster, is_confirmed) = self.stable_cluster();
        if cluster.is_empty() {
            return (Status::Provisional, FALLBACK_SECTORS.to_vec());
        }
        let mut starts = vec![0.0];
        for index in 0..cluster[0].boundaries.len() {
            starts.push(median(
                cluster.iter().map(|item| item.boundaries[index]).collect(),
            ));
        }
        (
            if is_confirmed {
                Status::Confirmed
            } else {
                Status::Provisional
            },
            starts,
        )
    }

    fn confirmed_starts(&self) -> Option<Vec<f64>> {
        let (status, starts) = self.status_and_starts();
        (status == Status::Confirmed).then_some(starts)
    }

    fn stable_cluster(&self) -> (Vec<&Observation>, bool) {
        stable_cluster(self.observations.iter().collect())
    }
}

fn stable_cluster(items: Vec<&Observation>) -> (Vec<&Observation>, bool) {
    if items.is_empty() {
        return (Vec::new(), false);
    }
    let mut best = Vec::new();
    for center in &items {
        let cluster: Vec<_> = items
            .iter()
            .copied()
            .filter(|item| {
                item.boundaries.len() == center.boundaries.len()
                    && item.reference_revision == center.reference_revision
                    && item
                        .boundaries
                        .iter()
                        .zip(&center.boundaries)
                        .all(|(a, b)| (a - b).abs() <= MAX_CONFIRMED_DEVIATION)
                    && item
                        .anchors
                        .iter()
                        .zip(&center.anchors)
                        .all(|(a, b)| (a.x_m - b.x_m).hypot(a.y_m - b.y_m) <= 35.0)
            })
            .collect();
        if cluster.len() > best.len() {
            best = cluster;
        }
    }
    if best.is_empty() {
        return (Vec::new(), false);
    }
    let confirmed = best.len() >= MIN_CONFIRM_LAPS
        && (0..best[0].boundaries.len()).all(|index| {
            let center = median(best.iter().map(|item| item.boundaries[index]).collect());
            let deviations: Vec<_> = best
                .iter()
                .map(|item| (item.boundaries[index] - center).abs())
                .collect();
            deviations.iter().copied().fold(0.0, f64::max) <= MAX_CONFIRMED_DEVIATION
                && median(deviations) <= MAX_PROMOTION_MAD
        });
    (best, confirmed)
}

#[cfg(test)]
impl SectorEstimator {
    fn save(&self, path: &Path, key: EvoTrackKey, actual_length: f64) -> io::Result<()> {
        fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
        let backup = backup_path(path);
        for existing in [path, backup.as_path()] {
            if read_sector_file(existing).is_some_and(|file| file.key != key) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "EVO sector cache key collision",
                ));
            }
            if read_schema_version(existing, MAX_SIDECAR_BYTES)
                .is_some_and(|version| version > u64::from(SCHEMA_VERSION))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported EVO sector cache version",
                ));
            }
        }
        let primary_valid = read_sector_file(path)
            .is_some_and(|file| valid_sector_file(&file, &key, actual_length));
        let backup_valid = read_sector_file(&backup)
            .is_some_and(|file| valid_sector_file(&file, &key, actual_length));
        if path.exists() && !primary_valid && backup_valid {
            // Keep a recoverable confirmed copy out of the atomic-replace backup rotation.
            fs::remove_file(path)?;
        }
        let (status, starts) = self.status_and_starts();
        let file = SectorFile {
            schema_version: SCHEMA_VERSION,
            key,
            track_length_m: actual_length,
            status,
            sector_starts: starts,
            observations: self.observations.iter().cloned().collect(),
        };
        let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
        atomic_replace(path, &bytes)
    }
}

#[cfg(test)]
fn valid_sector_file(file: &SectorFile, key: &EvoTrackKey, actual_length: f64) -> bool {
    if file.schema_version != SCHEMA_VERSION
        || !file.key.is_valid()
        || &file.key != key
        || !file.track_length_m.is_finite()
        || (file.track_length_m - actual_length).abs() > 1.0
        || file.observations.len() > MAX_OBSERVATIONS
        || file.sector_starts.first() != Some(&0.0)
        || !valid_sector_boundaries_for_length(
            file.sector_starts.get(1..).unwrap_or_default(),
            actual_length,
        )
    {
        return false;
    }
    let mut estimator = SectorEstimator::default();
    for observation in &file.observations {
        if !valid_sector_boundaries_for_length(&observation.boundaries, actual_length)
            || observation.reference_revision == 0
            || estimator
                .observations
                .front()
                .is_some_and(|known| known.reference_revision != observation.reference_revision)
            || observation.anchors.len() != observation.boundaries.len()
            || observation
                .anchors
                .iter()
                .zip(&observation.boundaries)
                .any(|(anchor, &pct)| {
                    !anchor.x_m.is_finite()
                        || !anchor.y_m.is_finite()
                        || !anchor.dir_x.is_finite()
                        || !anchor.dir_y.is_finite()
                        || (anchor.dir_x.hypot(anchor.dir_y) - 1.0).abs() > 0.01
                        || anchor.pct_on_revision != pct
                })
            || estimator
                .observations
                .iter()
                .any(|known| known.fingerprint == observation.fingerprint)
        {
            return false;
        }
        estimator.observations.push_back(observation.clone());
    }
    let (computed_status, computed_starts) = estimator.status_and_starts();
    computed_status == file.status
        && file.sector_starts.len() == computed_starts.len()
        && file
            .sector_starts
            .iter()
            .zip(computed_starts)
            .all(|(left, right)| (left - right).abs() <= 1e-9)
}

fn valid_sector_boundaries(boundaries: &[f64], width: f64) -> bool {
    if boundaries.len() > 9 {
        return false;
    }
    let mut previous = 0.0;
    for &boundary in boundaries {
        if !boundary.is_finite() || boundary - previous < width {
            return false;
        }
        previous = boundary;
    }
    1.0 - previous >= width
}

fn valid_sector_boundaries_for_length(boundaries: &[f64], length_m: f64) -> bool {
    if !length_m.is_finite() || !(100.0..=100_000.0).contains(&length_m) {
        return false;
    }
    let width = (20.0 / length_m).max(MIN_SECTOR_WIDTH);
    valid_sector_boundaries(boundaries, width)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) * 0.5
    } else {
        values[mid]
    }
}

#[cfg(test)]
fn sidecar_path(root: &Path, key: &EvoTrackKey) -> PathBuf {
    root.join("sectors")
        .join(format!("{:016x}.json", key.hash()))
}

#[cfg(test)]
fn layout_path(root: &Path, key: &EvoTrackKey) -> PathBuf {
    root.join("layouts")
        .join(format!("{:016x}.json", key.hash()))
}

#[cfg(test)]
fn load_layout(
    path: &Path,
    key: &EvoTrackKey,
    track_name: &str,
    track_length_m: f64,
) -> Option<(Track, bool)> {
    [path.to_path_buf(), backup_path(path)]
        .iter()
        .find_map(|candidate| {
            let file = read_layout_file(candidate)?;
            valid_layout_file(&file, key, track_name, track_length_m)
        })
}

#[cfg(test)]
fn save_layout(
    path: &Path,
    key: &EvoTrackKey,
    track: &Track,
    from_valid_lap: bool,
) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let backup = backup_path(path);
    for existing in [path, backup.as_path()] {
        if read_layout_file(existing).is_some_and(|file| file.key != *key) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "EVO layout cache key collision",
            ));
        }
        if read_schema_version(existing, MAX_LAYOUT_BYTES)
            .is_some_and(|version| version > u64::from(SCHEMA_VERSION))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported EVO layout cache version",
            ));
        }
    }
    let primary_valid = read_layout_file(path)
        .is_some_and(|file| valid_layout_file(&file, key, &track.name, track.length).is_some());
    let backup_valid = read_layout_file(&backup)
        .is_some_and(|file| valid_layout_file(&file, key, &track.name, track.length).is_some());
    if path.exists() && !primary_valid && backup_valid {
        fs::remove_file(path)?;
    }
    let file = LayoutFile {
        schema_version: SCHEMA_VERSION,
        key: key.clone(),
        track: track.clone(),
        from_valid_lap,
    };
    let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_LAYOUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "EVO layout cache exceeds size limit",
        ));
    }
    atomic_replace(path, &bytes)
}

#[cfg(test)]
fn read_layout_file(path: &Path) -> Option<LayoutFile> {
    if fs::metadata(path).ok()?.len() > MAX_LAYOUT_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn read_schema_version(path: &Path, max_bytes: u64) -> Option<u64> {
    if fs::metadata(path).ok()?.len() > max_bytes {
        return None;
    }
    serde_json::from_slice::<serde_json::Value>(&fs::read(path).ok()?)
        .ok()?
        .get("schemaVersion")?
        .as_u64()
}

#[cfg(test)]
fn valid_layout_file(
    file: &LayoutFile,
    key: &EvoTrackKey,
    track_name: &str,
    track_length_m: f64,
) -> Option<(Track, bool)> {
    let geometry_valid = file.schema_version == SCHEMA_VERSION
        && file.key.is_valid()
        && file.key == *key
        && canonical(track_name) == key.track
        && canonical(&file.track.name) == key.track
        && file.track.length.is_finite()
        && (file.track.length - track_length_m).abs() <= 1.0
        && file.track.x.len() >= 100
        && file.track.x.len() <= 2_000
        && file.track.x.len() == file.track.y.len()
        && file
            .track
            .x
            .iter()
            .chain(&file.track.y)
            .all(|value| value.is_finite());
    geometry_valid.then(|| (file.track.clone(), file.from_valid_lap))
}

#[cfg(test)]
fn read_sector_file(path: &Path) -> Option<SectorFile> {
    if fs::metadata(path).ok()?.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("cache");
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let backup = backup_path(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);

    let had_original = path.exists();
    if had_original {
        // The destination remains recoverable if the second rename fails or the process exits.
        if backup.exists()
            && let Err(error) = fs::remove_file(&backup)
        {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
    }
    if let Err(error) = fs::rename(&temp, path) {
        if had_original {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[derive(Clone, Default)]
struct LapSample {
    lap_time: f64,
    pct: f64,
    absolute_pct: f64,
    position: (f32, f32),
    reference_revision: u32,
}

struct CurrentLap {
    epoch: u64,
    reference_revision: u32,
    closed: bool,
    lap_counter: u32,
    track_length_m: f64,
    started_at_zero: bool,
    valid: bool,
    in_pit: bool,
    all_solo: bool,
    out_lap: bool,
    invalid: bool,
    invalid_reasons: Vec<&'static str>,
    stagnant_clock: f64,
    slow_time: f64,
    last_clock_ms: u32,
    previous_completed_ms: u32,
    last_seconds: f64,
    last_position: Option<(f32, f32)>,
    last_pct: Option<f64>,
    unwrapped_pct: f64,
    samples: Vec<LapSample>,
}

impl CurrentLap {
    fn new(sample: &TelemetrySample, out_lap: bool) -> Self {
        let mut result = Self {
            epoch: sample.epoch,
            reference_revision: sample.reference_revision,
            closed: sample.closed,
            lap_counter: sample.lap_counter,
            track_length_m: f64::from(sample.track_length_m),
            started_at_zero: sample.lap_clock_ms <= 1_000,
            valid: sample.lap_valid,
            in_pit: sample.in_pit,
            all_solo: sample.active_cars == 1,
            out_lap,
            invalid: false,
            invalid_reasons: Vec::new(),
            stagnant_clock: 0.0,
            slow_time: 0.0,
            last_clock_ms: sample.lap_clock_ms,
            previous_completed_ms: sample.last_lap_ms,
            last_seconds: sample.seconds,
            last_position: None,
            last_pct: None,
            unwrapped_pct: 0.0,
            samples: Vec::new(),
        };
        result.add_sample(sample, false);
        result
    }

    fn mark_invalid(&mut self, reason: &'static str) {
        self.invalid = true;
        if !self.invalid_reasons.contains(&reason) {
            self.invalid_reasons.push(reason);
        }
    }

    fn add_sample(&mut self, sample: &TelemetrySample, gap: bool) {
        if sample.reference_revision == 0
            || sample.reference_revision != self.reference_revision
            || sample.closed != self.closed
        {
            self.mark_invalid("outline_revision_changed_or_unavailable");
        }
        let first = self.samples.is_empty();
        let dt = sample.seconds - self.last_seconds;
        if gap || (!first && (!dt.is_finite() || dt <= 0.0 || dt > SAMPLE_GAP_MAX)) {
            self.mark_invalid("sample_interval_or_queue_gap");
        }
        if sample.lap_clock_ms < self.last_clock_ms {
            self.mark_invalid("lap_clock_moved_backwards");
        } else if sample.lap_clock_ms == self.last_clock_ms {
            self.stagnant_clock += dt.max(0.0);
            if self.stagnant_clock > SAMPLE_GAP_MAX {
                self.mark_invalid("lap_clock_stalled");
            }
        } else {
            self.stagnant_clock = 0.0;
        }
        self.valid &= sample.lap_valid;
        self.in_pit |= sample.in_pit;
        self.all_solo &= sample.active_cars == 1;
        let speed_valid =
            sample.speed_mps.is_finite() && (0.0..=MAX_SPEED_MPS).contains(&sample.speed_mps);
        if !speed_valid {
            self.mark_invalid("invalid_speed");
        }
        if !sample.track_length_m.is_finite()
            || !(100.0..=100_000.0).contains(&sample.track_length_m)
        {
            self.mark_invalid("invalid_track_length");
        }
        if speed_valid && sample.speed_mps < 1.5 {
            self.slow_time += dt.max(0.0);
        } else {
            self.slow_time = 0.0;
        }
        if self.slow_time > 0.75 {
            self.mark_invalid("stationary_over_750ms");
        }

        let Some((x, y)) = sample
            .position
            .filter(|(x, y)| x.is_finite() && y.is_finite())
        else {
            self.mark_invalid("missing_player_position");
            self.last_clock_ms = sample.lap_clock_ms;
            self.last_seconds = sample.seconds;
            self.last_position = None;
            self.last_pct = None;
            return;
        };
        let Some(pct) = sample.projected_pct.filter(|pct| {
            pct.is_finite()
                && if self.closed {
                    (0.0..1.0).contains(pct)
                } else {
                    (0.0..=1.0).contains(pct)
                }
        }) else {
            self.mark_invalid("missing_track_projection");
            self.last_clock_ms = sample.lap_clock_ms;
            self.last_seconds = sample.seconds;
            self.last_position = Some((x, y));
            self.last_pct = None;
            return;
        };
        if sample
            .projection_distance_m
            .is_none_or(|distance| !distance.is_finite() || distance > MAX_PROJECTION_DISTANCE_M)
        {
            self.mark_invalid("projection_over_30m_or_unavailable");
        }
        if let Some((px, py)) = self.last_position {
            let moved = (x - px).hypot(y - py);
            let speed_mps = if speed_valid {
                f64::from(sample.speed_mps)
            } else {
                0.0
            };
            let expected = speed_mps * dt.max(0.0) * 3.0 + 15.0;
            if !moved.is_finite() || f64::from(moved) > expected.max(30.0) || moved > 100.0 {
                self.mark_invalid("world_position_jump");
            }
        }

        let unwrapped;
        if let Some(previous) = self.last_pct {
            let delta = pct - previous;
            if self.closed && delta < -0.5 && previous > 0.8 && pct < 0.2 {
                self.unwrapped_pct += 1.0 + delta;
            } else if delta < -0.005 {
                self.mark_invalid("projected_progress_reversed");
                self.unwrapped_pct += delta;
            } else {
                let speed_mps = if speed_valid {
                    f64::from(sample.speed_mps)
                } else {
                    0.0
                };
                let maximum = 0.004
                    + speed_mps * dt.max(0.0) * 3.0 / f64::from(sample.track_length_m.max(1.0));
                if delta > maximum.max(0.006) {
                    self.mark_invalid("projected_progress_jump");
                }
                self.unwrapped_pct += delta.max(0.0);
            }
            unwrapped = self.unwrapped_pct;
        } else if self.samples.is_empty() {
            // Progress is local to this lap. The first projected sample can fall just before
            // the map's 0/1 seam when EVO publishes its lap counter, so do not seed it with an
            // absolute track fraction.
            self.unwrapped_pct = 0.0;
            unwrapped = 0.0;
        } else {
            // Projection may have been unavailable temporarily. The lap is already invalid;
            // preserve the last continuous progress instead of jumping to an absolute fraction.
            unwrapped = self.unwrapped_pct;
        }
        if unwrapped.is_finite()
            && (0.0..=1.1).contains(&unwrapped)
            && self.samples.len() < MAX_LAP_SAMPLES
        {
            self.samples.push(LapSample {
                lap_time: f64::from(sample.lap_clock_ms) / 1_000.0,
                pct: unwrapped,
                absolute_pct: pct,
                position: (x, y),
                reference_revision: sample.reference_revision,
            });
        } else {
            self.mark_invalid("invalid_progress_or_sample_limit");
        }
        self.last_clock_ms = sample.lap_clock_ms;
        self.last_seconds = sample.seconds;
        self.last_position = Some((x, y));
        self.last_pct = Some(pct);
    }

    fn finish(
        self,
        last_lap_ms: u32,
        completed_at: f64,
        next_out_lap: bool,
        finish_gap_valid: bool,
        finish_position_valid: bool,
    ) -> TelemetryLap {
        let full_course_coverage = self
            .samples
            .first()
            .zip(self.samples.last())
            .is_some_and(|(first, last)| last.pct - first.pct >= 0.9);
        TelemetryLap {
            epoch: self.epoch,
            reference_revision: self.reference_revision,
            closed: self.closed,
            track_length_m: self.track_length_m,
            completed_at,
            lap_ms: last_lap_ms,
            started_at_zero: self.started_at_zero,
            valid: self.valid,
            in_pit: self.in_pit,
            all_solo: self.all_solo,
            out_lap: self.out_lap || next_out_lap,
            invalid: self.invalid,
            invalid_reasons: self.invalid_reasons,
            finish_gap_valid,
            finish_position_valid,
            full_course_coverage,
            samples: self.samples,
        }
    }
}

struct TelemetryLap {
    epoch: u64,
    reference_revision: u32,
    closed: bool,
    track_length_m: f64,
    completed_at: f64,
    lap_ms: u32,
    started_at_zero: bool,
    valid: bool,
    in_pit: bool,
    all_solo: bool,
    out_lap: bool,
    invalid: bool,
    invalid_reasons: Vec<&'static str>,
    finish_gap_valid: bool,
    finish_position_valid: bool,
    full_course_coverage: bool,
    samples: Vec<LapSample>,
}

impl TelemetryLap {
    fn rejection_reasons(&self) -> Vec<&'static str> {
        let mut reasons = self.invalid_reasons.clone();
        if self.lap_ms == 0 {
            reasons.push("zero_lap_time");
        }
        if !self.started_at_zero {
            reasons.push("lap_did_not_start_near_zero");
        }
        if !self.valid {
            reasons.push("game_marked_lap_invalid");
        }
        if self.in_pit {
            reasons.push("lap_entered_pits");
        }
        if !self.all_solo {
            reasons.push("active_car_count_was_not_one");
        }
        if self.out_lap {
            reasons.push("lap_marked_as_outlap");
        }
        if self.invalid && self.invalid_reasons.is_empty() {
            reasons.push("telemetry_quality_failed_unspecified");
        }
        if !self.finish_gap_valid {
            reasons.push("finish_sample_gap_over_250ms");
        }
        if !self.finish_position_valid {
            reasons.push("finish_line_projection_mismatch");
        }
        if !self.full_course_coverage {
            reasons.push("projected_course_coverage_under_90pct");
        }
        if self.samples.len() < 20 {
            reasons.push("fewer_than_20_progress_samples");
        }
        reasons.sort_unstable();
        reasons.dedup();
        reasons
    }

    fn candidate(&self, logged: &LoggedLap) -> Result<Vec<BoundaryAnchor>, String> {
        let reasons = self.rejection_reasons();
        if !reasons.is_empty() {
            return Err(reasons.join(","));
        }
        if !(1..=10).contains(&logged.split_ms.len()) {
            return Err("unsupported_sector_count".into());
        }
        let mut elapsed_ms = 0u64;
        let mut boundaries = Vec::with_capacity(logged.split_ms.len() - 1);
        for (index, &split_ms) in logged.split_ms[..logged.split_ms.len() - 1]
            .iter()
            .enumerate()
        {
            if split_ms == 0 {
                return Err("split_clock_order_or_duration_invalid".into());
            }
            elapsed_ms += u64::from(split_ms);
            if elapsed_ms >= u64::from(self.lap_ms) {
                return Err("split_clock_order_or_duration_invalid".into());
            }
            boundaries.push(
                anchor_at_lap_time(
                    &self.samples,
                    elapsed_ms as f64 / 1_000.0,
                    self.reference_revision,
                    self.closed,
                )
                .map_err(|reason| format!("split{}_{reason}", index + 1))?,
            );
        }
        let fractions: Vec<_> = boundaries
            .iter()
            .map(|anchor| anchor.pct_on_revision)
            .collect();
        if !valid_sector_boundaries_for_length(&fractions, self.track_length_m) {
            return Err("inferred_boundaries_out_of_order_or_too_close".into());
        }
        Ok(boundaries)
    }
}

fn anchor_at_lap_time(
    samples: &[LapSample],
    time: f64,
    revision: u32,
    closed: bool,
) -> Result<BoundaryAnchor, &'static str> {
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.lap_time <= time && time <= b.lap_time {
            let gap = b.lap_time - a.lap_time;
            if gap <= 0.0 {
                return Err("nonincreasing_sample_clock");
            }
            if gap > SAMPLE_GAP_MAX {
                return Err("sample_gap_over_250ms");
            }
            if revision == 0 || a.reference_revision != revision || b.reference_revision != revision
            {
                return Err("outline_revision_changed_or_unavailable");
            }
            let dx = b.position.0 - a.position.0;
            let dy = b.position.1 - a.position.1;
            let length = dx.hypot(dy);
            if !length.is_finite() || length <= f32::EPSILON {
                return Err("missing_travel_tangent");
            }
            let t = (time - a.lap_time) / gap;
            let delta = if closed {
                (b.absolute_pct - a.absolute_pct + 0.5).rem_euclid(1.0) - 0.5
            } else {
                b.absolute_pct - a.absolute_pct
            };
            return Ok(BoundaryAnchor {
                x_m: a.position.0 + dx * t as f32,
                y_m: a.position.1 + dy * t as f32,
                dir_x: dx / length,
                dir_y: dy / length,
                pct_on_revision: if closed {
                    (a.absolute_pct + delta * t).rem_euclid(1.0)
                } else {
                    (a.absolute_pct + delta * t).clamp(0.0, 1.0)
                },
            });
        }
    }
    Err("split_time_outside_sample_range")
}

#[cfg(test)]
fn position_at_lap_time(samples: &[LapSample], time: f64) -> Result<f64, &'static str> {
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.lap_time <= time && time <= b.lap_time {
            let gap = b.lap_time - a.lap_time;
            if gap <= 0.0 {
                return Err("nonincreasing_sample_clock");
            }
            if gap > SAMPLE_GAP_MAX {
                return Err("sample_gap_over_250ms");
            }
            let fraction = (time - a.lap_time) / gap;
            return Ok(a.pct + (b.pct - a.pct) * fraction);
        }
    }
    Err("split_time_outside_sample_range")
}

fn cyclic_progress_distance(a: f64, b: f64) -> f64 {
    let forward = (b - a).rem_euclid(1.0);
    forward.min(1.0 - forward)
}

fn finish_position_matches(
    last_position: Option<(f32, f32)>,
    next_position: Option<(f32, f32)>,
    last_pct: Option<f64>,
    next_pct: Option<f64>,
    closed: bool,
) -> bool {
    last_position
        .zip(next_position)
        .zip(last_pct.zip(next_pct))
        .is_some_and(|(((x1, y1), (x2, y2)), (last_pct, next_pct))| {
            let moved = f64::from(x2 - x1).hypot(f64::from(y2 - y1));
            moved <= MAX_FINISH_POSITION_GAP_M
                && (if closed {
                    cyclic_progress_distance(last_pct, next_pct)
                } else {
                    (last_pct - next_pct).abs()
                }) <= MAX_FINISH_PROGRESS_GAP
        })
}

struct PendingLog {
    epoch: u64,
    received_at: f64,
    lap: LoggedLap,
}

struct MatchContext<'a> {
    session: &'a SessionRequest,
    estimator: Option<&'a mut SectorEstimator>,
    learned: Option<&'a mut EvoLearnedFile>,
    track_cache: Option<&'a Path>,
    fingerprints: &'a mut HashSet<u64>,
    fingerprint_order: &'a mut VecDeque<u64>,
    diagnostics: &'a mut DiagnosticLog,
    current_seconds: f64,
}

struct DiagnosticLog {
    reported: HashSet<&'static str>,
    file: Option<fs::File>,
    path: Option<PathBuf>,
    bytes: u64,
}

impl DiagnosticLog {
    fn new(root: Option<&Path>) -> Self {
        let mut diagnostic = Self {
            reported: HashSet::new(),
            file: None,
            path: None,
            bytes: 0,
        };
        let Some(root) = root else {
            return diagnostic;
        };
        let path = root.join("sector-learning.log");
        let opened = fs::create_dir_all(root)
            .and_then(|()| OpenOptions::new().create(true).append(true).open(&path));
        let file = match opened {
            Ok(file) => file,
            Err(error) => {
                eprintln!(
                    "EVO sector learner diagnostics unavailable ({:?})",
                    error.kind()
                );
                return diagnostic;
            }
        };
        let bytes = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                eprintln!(
                    "EVO sector learner diagnostics unavailable ({:?})",
                    error.kind()
                );
                return diagnostic;
            }
        };
        let file = if bytes > MAX_DIAGNOSTIC_BYTES {
            drop(file);
            match OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) => {
                    eprintln!(
                        "EVO sector learner diagnostics unavailable ({:?})",
                        error.kind()
                    );
                    return diagnostic;
                }
            }
        } else {
            file
        };
        diagnostic.bytes = if bytes > MAX_DIAGNOSTIC_BYTES {
            0
        } else {
            bytes
        };
        diagnostic.file = Some(file);
        diagnostic.path = Some(path);
        diagnostic
    }

    fn log(&mut self, message: String) {
        #[cfg(not(test))]
        eprintln!("{message}");
        let message = if message.len() as u64 > MAX_DIAGNOSTIC_BYTES - 64 {
            "EVO sector learner diagnostic event omitted (too large)".to_owned()
        } else {
            message
        };
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format!("{stamp} {message}\n");
        if self.file.is_none() {
            return;
        }
        let truncate = self.bytes.saturating_add(line.len() as u64) > MAX_DIAGNOSTIC_BYTES;
        if truncate {
            self.file = None;
            let Some(path) = self.path.as_ref() else {
                return;
            };
            match OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
            {
                Ok(file) => self.file = Some(file),
                Err(error) => {
                    eprintln!(
                        "EVO sector learner diagnostics unavailable ({:?})",
                        error.kind()
                    );
                    return;
                }
            }
            self.bytes = 0;
        }
        let result = {
            let file = self.file.as_mut().unwrap();
            file.write_all(line.as_bytes()).and_then(|()| file.flush())
        };
        match result {
            Ok(()) => self.bytes = self.bytes.saturating_add(line.len() as u64),
            Err(error) => {
                self.file = None;
                eprintln!(
                    "EVO sector learner diagnostics unavailable ({:?})",
                    error.kind()
                );
            }
        }
    }

    fn report_once(&mut self, category: &'static str, message: String) {
        if self.reported.insert(category) {
            self.log(message);
        }
    }
}

fn worker_loop(
    rx: Receiver<WorkerMessage>,
    results: SyncSender<WorkerResult>,
    overflow: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    log_dir: PathBuf,
    track_cache: Option<PathBuf>,
) {
    let mut tailer = LogTailer::new(log_dir);
    let mut parser = SplitParser::default();
    let mut active: Option<SessionRequest> = None;
    let mut estimator: Option<SectorEstimator> = None;
    let mut learned: Option<EvoLearnedFile> = None;
    let mut contributing_session: Option<u64> = None;
    let mut current_lap: Option<CurrentLap> = None;
    let mut completed = VecDeque::<TelemetryLap>::new();
    let mut pending_logs = VecDeque::<PendingLog>::new();
    let mut pending_open = VecDeque::<(Instant, PendingOpenRun)>::new();
    let mut open_completions = VecDeque::<(Instant, u32)>::new();
    let mut pending_result: Option<WorkerResult> = None;
    let mut fingerprints = HashSet::<u64>::new();
    let mut fingerprint_order = VecDeque::<u64>::new();
    let mut diagnostics = DiagnosticLog::new(track_cache.as_deref());
    let mut current_seconds = 0.0;
    let mut last_log_poll = Instant::now() - Duration::from_secs(1);
    let mut last_reported_error = None::<io::ErrorKind>;
    let mut tail_generation = tailer.generation();

    while running.load(Ordering::Acquire) {
        if let Some(result) = pending_result.take() {
            match results.try_send(result) {
                Ok(()) => {}
                Err(TrySendError::Full(result)) => pending_result = Some(result),
                Err(TrySendError::Disconnected(_)) => break,
            }
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(WorkerMessage::Stop) => break,
            Ok(WorkerMessage::Session(request)) => {
                if !pending_logs.is_empty() {
                    diagnostics.log(format!(
                        "EVO sector learner: session reset discarded {} pending parsed log laps",
                        pending_logs.len()
                    ));
                }
                current_seconds = 0.0;
                current_lap = None;
                completed.clear();
                pending_logs.clear();
                pending_open.clear();
                open_completions.clear();
                pending_result = None;
                parser.reset();
                tailer.seek_to_latest_end();
                tail_generation = tailer.generation();
                let same_track = active.as_ref().is_some_and(|old| old.key == request.key);
                let mut state = if same_track {
                    learned
                        .take()
                        .unwrap_or_else(|| EvoLearnedFile::blank(&request))
                } else {
                    track_cache
                        .as_ref()
                        .and_then(|root| EvoLearnedFile::load(root, &request))
                        .unwrap_or_else(|| EvoLearnedFile::blank(&request))
                };
                let mut sector_estimator = SectorEstimator {
                    observations: state.observations.clone().into(),
                    quarantined: VecDeque::new(),
                };
                if state.outline_valid
                    && let Some(track) = &state.active_outline
                    && track.geometry_revision < u32::MAX - 1
                {
                    let reference = OutlineCandidate {
                        track_key: request.key.outline_key(),
                        topology: state.topology,
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
                    if let Some(candidate) = state
                        .candidate_pool
                        .select(&reference, state.candidate_pool.session_count() == 1)
                    {
                        let mut proposed_track = track.clone();
                        proposed_track.x = candidate.points.iter().map(|p| p.0).collect();
                        proposed_track.y = candidate.points.iter().map(|p| p.1).collect();
                        proposed_track.geometry_revision =
                            track.geometry_revision.saturating_add(1);
                        if candidates_project_continuously(&state.candidate_pool, &proposed_track)
                            && let Some(migrated) = sector_estimator.migrate_to(
                                &proposed_track,
                                proposed_track.geometry_revision,
                                request.track_length_m,
                            )
                        {
                            let mut proposed = state.clone();
                            proposed.active_outline = Some(proposed_track);
                            proposed.observations = migrated.observations.iter().cloned().collect();
                            if proposed.valid(&request)
                                && track_cache
                                    .as_ref()
                                    .is_none_or(|root| proposed.save(root, &request).is_ok())
                            {
                                state = proposed;
                                sector_estimator = migrated;
                                diagnostics.log(format!(
                                    "EVO learner {:016x}: promoted outline",
                                    request.key.hash()
                                ));
                            } else {
                                diagnostics.log(format!(
                                    "EVO learner {:016x}: promotion validation or write failed",
                                    request.key.hash()
                                ));
                            }
                        } else {
                            diagnostics.log(format!(
                                "EVO learner {:016x}: sector anchor migration rejected promotion",
                                request.key.hash()
                            ));
                        }
                    }
                }
                let sectors = sector_estimator.confirmed_starts();
                diagnostics.log(format!(
                    "EVO sector learner {:016x}: session selection {}",
                    request.key.hash(),
                    if sectors.is_some() {
                        "confirmed"
                    } else {
                        "fallback"
                    }
                ));
                let outline = state.active_outline.clone().map(|mut track| {
                    track.sectors = sectors.clone().unwrap_or_else(|| FALLBACK_SECTORS.to_vec());
                    (track, state.outline_valid)
                });
                contributing_session = None;
                estimator = Some(sector_estimator);
                learned = Some(state);
                active = Some(request.clone());
                let result = WorkerResult::SessionLoaded {
                    epoch: request.epoch,
                    sectors,
                    outline,
                };
                if let Err(TrySendError::Full(result)) = results.try_send(result) {
                    pending_result = Some(result);
                }
            }
            Ok(WorkerMessage::InvalidateLap { epoch }) => {
                if active
                    .as_ref()
                    .is_some_and(|session| session.epoch == epoch)
                {
                    current_lap = None;
                    completed.clear();
                    pending_logs.clear();
                }
            }
            Ok(WorkerMessage::Sample(sample)) => {
                current_seconds = sample.seconds;
                if overflow.swap(false, Ordering::AcqRel)
                    && let Some(lap) = current_lap.as_mut()
                {
                    lap.mark_invalid("worker_sample_queue_overflow");
                }
                let Some(session) = active.as_ref() else {
                    continue;
                };
                if sample.epoch != session.epoch {
                    continue;
                }
                if let Some(lap) = current_lap.as_mut() {
                    let open_clock_completion = !sample.closed
                        && sample.lap_counter == lap.lap_counter
                        && lap.last_clock_ms > 5_000
                        && sample.lap_clock_ms <= 1_000
                        && sample.last_lap_ms > 0
                        && sample.last_lap_ms != lap.previous_completed_ms
                        && sample.last_lap_ms.abs_diff(lap.last_clock_ms) <= MATCH_TOLERANCE_MS;
                    if sample.lap_counter > lap.lap_counter || open_clock_completion {
                        let finished = current_lap.take().unwrap();
                        let previous_lap_was_in_pit = finished.in_pit;
                        let finish_gap = sample.seconds - finished.last_seconds;
                        let finish_gap_valid = finish_gap.is_finite()
                            && finish_gap > 0.0
                            && finish_gap <= SAMPLE_GAP_MAX;
                        let finish_position_valid = finish_position_matches(
                            finished.last_position,
                            sample.position,
                            finished.last_pct,
                            sample.projected_pct,
                            finished.closed,
                        );
                        if sample.last_lap_ms > 0 {
                            completed.push_back(finished.finish(
                                sample.last_lap_ms,
                                sample.seconds,
                                false,
                                finish_gap_valid,
                                finish_position_valid,
                            ));
                        }
                        while completed.len() > MAX_TELEMETRY_LAPS
                            || completed.front().is_some_and(|old| {
                                sample.seconds - old.completed_at > MAX_LAP_HISTORY.as_secs_f64()
                            })
                        {
                            completed.pop_front();
                        }
                        current_lap = Some(CurrentLap::new(&sample, previous_lap_was_in_pit));
                    } else if sample.lap_counter < lap.lap_counter {
                        current_lap = Some(CurrentLap::new(&sample, true));
                        completed.clear();
                        pending_logs.clear();
                    } else {
                        lap.add_sample(&sample, false);
                    }
                } else {
                    current_lap = Some(CurrentLap::new(&sample, true));
                }
                try_match_laps(
                    &mut pending_logs,
                    &mut completed,
                    MatchContext {
                        session,
                        estimator: estimator.as_mut(),
                        learned: learned.as_mut(),
                        track_cache: track_cache.as_deref(),
                        fingerprints: &mut fingerprints,
                        fingerprint_order: &mut fingerprint_order,
                        diagnostics: &mut diagnostics,
                        current_seconds,
                    },
                );
            }
            Ok(WorkerMessage::SaveOutline { key, track, valid }) => {
                if let (Some(request), Some(state)) = (active.as_ref(), learned.as_mut())
                    && request.key == key
                {
                    if state.outline_valid && state.active_outline.is_some() {
                        continue;
                    }
                    if state
                        .active_outline
                        .as_ref()
                        .is_some_and(|old| old.geometry_revision != track.geometry_revision)
                    {
                        state.observations.clear();
                        estimator = Some(SectorEstimator::default());
                    }
                    state.active_outline = Some(track);
                    state.outline_valid = valid;
                    if let Some(root) = track_cache.as_ref()
                        && let Err(error) = state.save(root, request)
                    {
                        diagnostics.log(format!(
                            "EVO learner {:016x}: outline save failed ({:?})",
                            key.hash(),
                            error.kind()
                        ));
                    }
                }
            }
            Ok(WorkerMessage::OutlineCandidate {
                epoch,
                source_revision,
                mut candidate,
            }) => {
                if let (Some(request), Some(state)) = (active.as_ref(), learned.as_mut())
                    && request.epoch == epoch
                    && candidate.domain == SamplingDomain::MeasuredDistance
                    && candidate.topology == request.key.topology()
                    && state
                        .active_outline
                        .as_ref()
                        .is_some_and(|track| track.geometry_revision == source_revision)
                {
                    let session_id = contributing_session.unwrap_or(state.next_session_id);
                    candidate.session_id = session_id;
                    candidate.track_key = request.key.outline_key();
                    let length_ratio =
                        candidate.measured_length().unwrap_or(0.0) / request.track_length_m;
                    if state.candidate_pool.observe(candidate) {
                        diagnostics.log(format!("EVO learner {:016x}: accepted outline candidate, length ratio {length_ratio:.3}", request.key.hash()));
                        if contributing_session.is_none() {
                            contributing_session = Some(session_id);
                            state.next_session_id = session_id.saturating_add(1);
                        }
                        if let Some(root) = track_cache.as_ref()
                            && let Err(error) = state.save(root, request)
                        {
                            diagnostics.log(format!(
                                "EVO learner {:016x}: candidate save failed ({:?})",
                                request.key.hash(),
                                error.kind()
                            ));
                        }
                    } else {
                        diagnostics.log(format!(
                            "EVO learner {:016x}: rejected invalid outline candidate",
                            request.key.hash()
                        ));
                    }
                } else {
                    diagnostics.log(
                        "EVO learner: rejected stale or incompatible outline candidate".into(),
                    );
                }
            }
            Ok(WorkerMessage::OpenRun(run)) => {
                if active.as_ref().is_some_and(|session| {
                    session.epoch == run.epoch && session.key.topology() == RouteTopology::Open
                }) {
                    pending_open.push_back((Instant::now(), run));
                    while pending_open.len() > 8 {
                        pending_open.pop_front();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        if !running.load(Ordering::Acquire) {
            break;
        }
        if last_log_poll.elapsed() >= Duration::from_secs(1) {
            last_log_poll = Instant::now();
            match tailer.read_appended() {
                Ok(lines) => {
                    last_reported_error = None;
                    if tailer.generation() != tail_generation {
                        parser.reset();
                        pending_open.clear();
                        open_completions.clear();
                        if !pending_logs.is_empty() {
                            diagnostics.log(format!(
                                "EVO sector learner: log rotation discarded {} pending parsed log laps",
                                pending_logs.len()
                            ));
                        }
                        pending_logs.clear();
                        tail_generation = tailer.generation();
                        diagnostics.report_once(
                            "log-rotation",
                            "EVO sector learner: log rotated or was truncated; split parser reset"
                                .into(),
                        );
                    }
                    if let Some(session) = active.as_ref() {
                        for line in lines {
                            if session.key.topology() == RouteTopology::Open
                                && let Some(ms) = completion_time_ms(&line)
                            {
                                open_completions.push_back((Instant::now(), ms));
                                while open_completions.len() > 8 {
                                    open_completions.pop_front();
                                }
                            }
                            if SplitParser::is_unsupported_split_line(&line) {
                                diagnostics.report_once(
                                    "unsupported-splits",
                                    "EVO sector learner: unsupported split grammar/count; using thirds"
                                        .into(),
                                );
                            }
                            if let Some(lap) = parser.push_line(&line) {
                                diagnostics.log(format!(
                                    "EVO sector learner: parsed complete log lap {} ms",
                                    lap.lap_ms
                                ));
                                if fingerprints.contains(&lap.fingerprint) {
                                    diagnostics.log(format!(
                                        "EVO sector learner: ignored duplicate log lap {} ms",
                                        lap.lap_ms
                                    ));
                                    continue;
                                }
                                pending_logs.push_back(PendingLog {
                                    epoch: session.epoch,
                                    received_at: current_seconds,
                                    lap,
                                });
                                while pending_logs.len() > MAX_PENDING_LOG_LAPS {
                                    if let Some(dropped) = pending_logs.pop_front() {
                                        diagnostics.log(format!(
                                            "EVO sector learner: pending log queue overflow dropped lap {} ms",
                                            dropped.lap.lap_ms
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    if last_reported_error != Some(error.kind()) {
                        // Never include the log line or a car/player identifier in diagnostics.
                        diagnostics.log(format!("EVO log tail unavailable ({:?})", error.kind()));
                        last_reported_error = Some(error.kind());
                    }
                }
            }
            if let Some(session) = active.as_ref() {
                try_match_laps(
                    &mut pending_logs,
                    &mut completed,
                    MatchContext {
                        session,
                        estimator: estimator.as_mut(),
                        learned: learned.as_mut(),
                        track_cache: track_cache.as_deref(),
                        fingerprints: &mut fingerprints,
                        fingerprint_order: &mut fingerprint_order,
                        diagnostics: &mut diagnostics,
                        current_seconds,
                    },
                );
            }
        }
        while pending_open
            .front()
            .is_some_and(|(at, _)| at.elapsed() > Duration::from_secs(10))
        {
            pending_open.pop_front();
        }
        while open_completions
            .front()
            .is_some_and(|(at, _)| at.elapsed() > Duration::from_secs(10))
        {
            open_completions.pop_front();
        }
        if let (Some(request), Some(state), Some(sector_estimator)) =
            (active.as_ref(), learned.as_mut(), estimator.as_mut())
            && let Some((_, run)) = pending_open.front()
            && let Some((index, _)) = open_completions
                .iter()
                .enumerate()
                .filter(|(_, (_, ms))| ms.abs_diff(run.lap_ms) <= MATCH_TOLERANCE_MS)
                .min_by_key(|(index, (_, ms))| (ms.abs_diff(run.lap_ms), *index))
        {
            open_completions.remove(index);
            let (_, run) = pending_open.pop_front().expect("pending run exists");
            let current_revision = state
                .active_outline
                .as_ref()
                .map_or(0, |track| track.geometry_revision);
            if run.source_revision == current_revision {
                let mut proposed = state.clone();
                let mut next_estimator = sector_estimator.clone();
                let mut new_contributing_session = None;
                let bootstrap =
                    proposed.active_outline.is_none() || (!proposed.outline_valid && run.valid);
                if bootstrap {
                    let mut track = run.track.clone();
                    track.geometry_revision = current_revision.saturating_add(1);
                    track.sectors = FALLBACK_SECTORS.to_vec();
                    proposed.active_outline = Some(track);
                    proposed.outline_valid = run.valid;
                    proposed.observations.clear();
                    next_estimator = SectorEstimator::default();
                }
                if run.valid {
                    let session_id = contributing_session.unwrap_or(proposed.next_session_id);
                    let candidate = OutlineCandidate {
                        track_key: request.key.outline_key(),
                        topology: RouteTopology::Open,
                        domain: SamplingDomain::MeasuredDistance,
                        valid: true,
                        session_id,
                        points: run
                            .track
                            .x
                            .iter()
                            .copied()
                            .zip(run.track.y.iter().copied())
                            .collect(),
                    };
                    if proposed.candidate_pool.observe(candidate) && contributing_session.is_none()
                    {
                        new_contributing_session = Some(session_id);
                        proposed.next_session_id = session_id.saturating_add(1);
                    }
                }
                if proposed.valid(request)
                    && track_cache
                        .as_ref()
                        .is_none_or(|root| proposed.save(root, request).is_ok())
                {
                    *state = proposed;
                    *sector_estimator = next_estimator;
                    if new_contributing_session.is_some() {
                        contributing_session = new_contributing_session;
                    }
                    if bootstrap && let Some(track) = state.active_outline.clone() {
                        current_lap = None;
                        completed.clear();
                        pending_logs.clear();
                        let result = WorkerResult::OpenBootstrap {
                            epoch: run.epoch,
                            source_revision: run.source_revision,
                            track,
                            valid: run.valid,
                        };
                        if let Err(TrySendError::Full(result)) = results.try_send(result) {
                            pending_result = Some(result);
                        }
                    }
                    diagnostics.log(format!(
                        "EVO learner {:016x}: matched open timing event",
                        request.key.hash()
                    ));
                }
            }
        }
    }
    if !pending_logs.is_empty() {
        diagnostics.log(format!(
            "EVO sector learner: worker stopped with {} pending parsed log laps",
            pending_logs.len()
        ));
    }
}

fn nearest_telemetry_candidate(candidates: &[(usize, u32)]) -> Option<(usize, u32)> {
    candidates
        .iter()
        .copied()
        .min_by_key(|(index, difference)| (*difference, *index))
}

fn try_match_laps(
    pending_logs: &mut VecDeque<PendingLog>,
    completed: &mut VecDeque<TelemetryLap>,
    context: MatchContext<'_>,
) {
    let MatchContext {
        session,
        estimator,
        mut learned,
        track_cache,
        fingerprints,
        fingerprint_order,
        diagnostics,
        current_seconds,
    } = context;
    let Some(estimator) = estimator else {
        return;
    };
    while let Some(pending) = pending_logs.front() {
        if pending.epoch != session.epoch {
            diagnostics.log(format!(
                "EVO sector learner {:016x}: discarded stale parsed log lap {} ms",
                session.key.hash(),
                pending.lap.lap_ms
            ));
            pending_logs.pop_front();
            continue;
        }
        let candidates: Vec<_> = completed
            .iter()
            .enumerate()
            .filter(|(_, lap)| {
                lap.epoch == pending.epoch
                    && lap.lap_ms.abs_diff(pending.lap.lap_ms) <= MATCH_TOLERANCE_MS
            })
            .map(|(index, lap)| (index, lap.lap_ms.abs_diff(pending.lap.lap_ms)))
            .collect();
        if candidates.is_empty() {
            let too_old = current_seconds - pending.received_at > MAX_LAP_HISTORY.as_secs_f64();
            if too_old {
                diagnostics.log(format!(
                    "EVO sector learner {:016x}: no telemetry match for log lap {} ms",
                    session.key.hash(),
                    pending.lap.lap_ms
                ));
                pending_logs.pop_front();
            } else {
                break;
            }
            continue;
        }
        // The game log and telemetry queues are chronological. When adjacent lap times are
        // within the broad safety tolerance, choose the closest duration; equal-distance ties
        // stay with the older telemetry lap, preserving that order.
        let (index, difference) = nearest_telemetry_candidate(&candidates).unwrap();
        if candidates.len() > 1 {
            diagnostics.log(format!(
                "EVO sector learner {:016x}: selected nearest telemetry lap for log lap {} ms ({} candidates, {} ms difference)",
                session.key.hash(),
                pending.lap.lap_ms,
                candidates.len(),
                difference
            ));
        }
        let lap = completed.remove(index).unwrap();
        let pending = pending_logs.pop_front().unwrap();
        if fingerprints.contains(&pending.lap.fingerprint) {
            diagnostics.log(format!(
                "EVO sector learner {:016x}: ignored duplicate matched lap {} ms",
                session.key.hash(),
                pending.lap.lap_ms
            ));
            continue;
        }
        let logged_lap_ms = pending.lap.lap_ms;
        let boundaries = match lap.candidate(&pending.lap) {
            Ok(boundaries) => boundaries,
            Err(reason) => {
                diagnostics.log(format!(
                    "EVO sector learner {:016x}: rejected matched lap {} ms ({reason})",
                    session.key.hash(),
                    logged_lap_ms
                ));
                remember_fingerprint(pending.lap.fingerprint, fingerprints, fingerprint_order);
                continue;
            }
        };
        let was_confirmed = estimator.confirmed_starts().is_some();
        match estimator.observe_anchors(
            boundaries.clone(),
            lap.reference_revision,
            pending.lap.fingerprint,
        ) {
            ObserveResult::Accepted => {}
            ObserveResult::Duplicate | ObserveResult::Invalid => {
                diagnostics.report_once(
                    "candidate-reject",
                    "EVO sector learner: ignored a duplicate or invalid sector candidate".into(),
                );
                continue;
            }
            ObserveResult::Quarantined { new_cluster } => {
                if new_cluster {
                    diagnostics.report_once(
                        "geometry-shift",
                        format!(
                            "EVO sector learner {:016x}: consistent candidates differ from the confirmed map; quarantined for track/layout review",
                            session.key.hash()
                        ),
                    );
                } else {
                    diagnostics.report_once(
                        "candidate-reject",
                        "EVO sector learner: ignored an outlying sector candidate".into(),
                    );
                }
                continue;
            }
        }
        remember_fingerprint(pending.lap.fingerprint, fingerprints, fingerprint_order);
        let (status, starts) = estimator.status_and_starts();
        diagnostics.log(format!(
            "EVO sector learner {:016x}: accepted lap {} ms with {} sectors; {} stored, {}",
            session.key.hash(),
            logged_lap_ms,
            boundaries.len() + 1,
            estimator.observations.len(),
            if status == Status::Confirmed {
                "confirmed"
            } else {
                "provisional"
            }
        ));
        if !was_confirmed && status == Status::Confirmed {
            diagnostics.report_once(
                "promotion",
                format!(
                    "EVO sector learner {:016x}: sector map confirmed with {} sectors",
                    session.key.hash(),
                    starts.len()
                ),
            );
        }
        if let Some(state) = learned.as_deref_mut() {
            let previous = state.observations.clone();
            state.observations = estimator.observations.iter().cloned().collect();
            if !state.valid(session) {
                state.observations = previous.clone();
                estimator.observations = previous.into();
                diagnostics.log(format!(
                    "EVO learner {:016x}: sector anchor validation rejected observation",
                    session.key.hash()
                ));
            } else if let Some(root) = track_cache
                && let Err(error) = state.save(root, session)
            {
                diagnostics.log(format!(
                    "EVO learner {:016x}: sector save failed ({:?})",
                    session.key.hash(),
                    error.kind()
                ));
            }
        }
    }
}

fn remember_fingerprint(value: u64, fingerprints: &mut HashSet<u64>, order: &mut VecDeque<u64>) {
    fingerprints.insert(value);
    order.push_back(value);
    while order.len() > MAX_OBSERVATIONS {
        if let Some(old) = order.pop_front() {
            fingerprints.remove(&old);
        }
    }
}

pub(super) fn default_log_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENSIMHUD_EVO_LOG_DIR") {
        return PathBuf::from(path);
    }
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    profile.join("Saved Games").join("ACE").join("Logs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn key() -> EvoTrackKey {
        EvoTrackKey::new("Red Bull Ring", "GP", 4318.0).unwrap()
    }

    #[test]
    fn track_key_is_stable_and_layout_specific() {
        let a = key();
        assert_eq!(
            a.hash(),
            EvoTrackKey::new(" red  bull ring ", "gp", 4318.2)
                .unwrap()
                .hash()
        );
        assert_ne!(
            a.hash(),
            EvoTrackKey::new("Red Bull Ring", "National", 4318.0)
                .unwrap()
                .hash()
        );
        assert_ne!(
            a.hash(),
            EvoTrackKey::new("Red Bull Ring", "GP", 4324.0)
                .unwrap()
                .hash()
        );
    }

    #[test]
    fn open_clock_reset_requires_matching_log_completion() {
        let dir = temporary_dir();
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let log = logs.join("log-open.txt");
        fs::write(&log, b"start\n").unwrap();
        let key = EvoTrackKey::new("Nordschleife", "Touristenfahrten", 1000.0).unwrap();
        let request = SessionRequest {
            epoch: 1,
            key,
            track_name: "Nordschleife".into(),
            track_length_m: 1000.0,
        };
        let mut worker = EvoLearningWorker::spawn(logs, Some(dir.join("cache")));
        worker.start_session(request);
        let wait_result = |worker: &EvoLearningWorker| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(result) = worker.try_result() {
                    return Some(result);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        assert!(matches!(
            wait_result(&worker),
            Some(WorkerResult::SessionLoaded { .. })
        ));
        let track = Track {
            name: "Nordschleife".into(),
            length: 1000.0,
            closed: false,
            x: (0..1000).map(|i| i as f32 * 1000.0 / 999.0).collect(),
            y: vec![0.0; 1000],
            sectors: FALLBACK_SECTORS.to_vec(),
            ..Track::default()
        };
        worker
            .open_run(PendingOpenRun {
                epoch: 1,
                source_revision: 0,
                track,
                valid: true,
                lap_ms: 60_000,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        assert!(matches!(worker.try_result(), Err(TryRecvError::Empty)));
        let mut file = OpenOptions::new().append(true).open(log).unwrap();
        writeln!(file, "12:00:00.000 New lap carId 1: 01:00.000").unwrap();
        assert!(
            matches!(wait_result(&worker), Some(WorkerResult::OpenBootstrap { track, .. }) if !track.closed && track.geometry_revision == 1)
        );
        worker.join();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn learned_cache_keeps_geometry_and_anchored_sectors_together() {
        let dir = temporary_dir();
        let request = SessionRequest {
            epoch: 1,
            key: key(),
            track_name: "Red Bull Ring".into(),
            track_length_m: 4318.0,
        };
        let radius = 4318.0 / std::f64::consts::TAU;
        let track = Track {
            name: request.track_name.clone(),
            length: request.track_length_m,
            x: (0..1000)
                .map(|i| (i as f64 * std::f64::consts::TAU / 1000.0).cos() as f32 * radius as f32)
                .collect(),
            y: (0..1000)
                .map(|i| (i as f64 * std::f64::consts::TAU / 1000.0).sin() as f32 * radius as f32)
                .collect(),
            sectors: FALLBACK_SECTORS.to_vec(),
            geometry_revision: 1,
            ..Track::default()
        };
        let anchors: Vec<_> = [0.25, 0.65]
            .into_iter()
            .map(|pct| {
                let angle = pct * std::f64::consts::TAU;
                BoundaryAnchor {
                    x_m: (angle.cos() * radius) as f32,
                    y_m: (angle.sin() * radius) as f32,
                    dir_x: -angle.sin() as f32,
                    dir_y: angle.cos() as f32,
                    pct_on_revision: pct,
                }
            })
            .collect();
        let mut estimator = SectorEstimator::default();
        for id in 1..=3 {
            assert_eq!(
                estimator.observe_anchors(anchors.clone(), 1, id),
                ObserveResult::Accepted
            );
        }
        let mut state = EvoLearnedFile::blank(&request);
        state.active_outline = Some(track);
        state.outline_valid = true;
        state.observations = estimator.observations.iter().cloned().collect();
        assert!(state.valid(&request));
        state.save(&dir, &request).unwrap();
        state.save(&dir, &request).unwrap();
        let path = learned_path(&dir, &request.key);
        fs::write(&path, b"corrupt").unwrap();
        let recovered = EvoLearnedFile::load(&dir, &request).unwrap();
        assert_eq!(recovered.observations.len(), 3);
        recovered.save(&dir, &request).unwrap();
        assert!(EvoLearnedFile::load(&dir, &request).is_some());
        let mut mismatched = recovered.clone();
        mismatched.observations[0].reference_revision = 2;
        assert!(!mismatched.valid(&request));
        let newer = br#"{"schemaVersion":99}"#;
        fs::write(&path, newer).unwrap();
        assert!(recovered.save(&dir, &request).is_err());
        assert_eq!(fs::read(&path).unwrap(), newer);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_selection_promotes_only_with_migrated_confirmed_sectors() {
        let dir = temporary_dir();
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let length = std::f64::consts::TAU * 520.0;
        let key = EvoTrackKey::new("Test", "GP", length).unwrap();
        let request = SessionRequest {
            epoch: 2,
            key,
            track_name: "Test".into(),
            track_length_m: length,
        };
        let points = |radius: f32| -> Vec<(f32, f32)> {
            (0..1000)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / 1000.0;
                    (radius * angle.cos(), radius * angle.sin())
                })
                .collect()
        };
        let old = points(500.0);
        let track = Track {
            name: "Test".into(),
            length,
            x: old.iter().map(|p| p.0).collect(),
            y: old.iter().map(|p| p.1).collect(),
            sectors: FALLBACK_SECTORS.to_vec(),
            geometry_revision: 1,
            ..Track::default()
        };
        let anchors: Vec<_> = [0.25, 0.65]
            .into_iter()
            .map(|pct| {
                let angle = pct * std::f64::consts::TAU;
                BoundaryAnchor {
                    x_m: (angle.cos() * 500.0) as f32,
                    y_m: (angle.sin() * 500.0) as f32,
                    dir_x: -angle.sin() as f32,
                    dir_y: angle.cos() as f32,
                    pct_on_revision: pct,
                }
            })
            .collect();
        let mut estimator = SectorEstimator::default();
        for id in 1..=3 {
            assert_eq!(
                estimator.observe_anchors(anchors.clone(), 1, id),
                ObserveResult::Accepted
            );
        }
        let mut state = EvoLearnedFile::blank(&request);
        state.active_outline = Some(track);
        state.outline_valid = true;
        state.observations = estimator.observations.into_iter().collect();
        for _ in 0..3 {
            assert!(state.candidate_pool.observe(OutlineCandidate {
                track_key: request.key.outline_key(),
                topology: RouteTopology::Closed,
                domain: SamplingDomain::MeasuredDistance,
                valid: true,
                session_id: 1,
                points: points(520.0)
            }));
        }
        state.next_session_id = 2;
        state.save(&dir, &request).unwrap();
        let mut worker = EvoLearningWorker::spawn(logs, Some(dir.clone()));
        worker.start_session(request.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        let loaded = loop {
            if let Ok(result) = worker.try_result() {
                break result;
            }
            assert!(Instant::now() < deadline, "worker selection timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        match loaded {
            WorkerResult::SessionLoaded {
                outline: Some((selected, true)),
                sectors: Some(sectors),
                ..
            } => {
                assert_eq!(selected.geometry_revision, 2);
                assert!((selected.x[0] - 520.0).abs() < 0.1);
                assert_eq!(sectors.len(), 3);
            }
            _ => panic!("expected migrated selection"),
        }
        let saved = EvoLearnedFile::load(&dir, &request).unwrap();
        assert!(
            saved
                .observations
                .iter()
                .all(|observation| observation.reference_revision == 2)
        );
        assert_eq!(saved.observations[0].anchors[0].x_m, anchors[0].x_m);
        worker.join();
        let veto_dir = dir.join("veto");
        state.candidate_pool = CandidateHistory::default();
        for _ in 0..3 {
            assert!(state.candidate_pool.observe(OutlineCandidate {
                track_key: request.key.outline_key(),
                topology: RouteTopology::Closed,
                domain: SamplingDomain::MeasuredDistance,
                valid: true,
                session_id: 1,
                points: points(550.0)
            }));
        }
        state.save(&veto_dir, &request).unwrap();
        let mut worker = EvoLearningWorker::spawn(dir.join("logs"), Some(veto_dir.clone()));
        worker.start_session(request.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        let loaded = loop {
            if let Ok(result) = worker.try_result() {
                break result;
            }
            assert!(Instant::now() < deadline, "worker veto timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        match loaded {
            WorkerResult::SessionLoaded {
                outline: Some((selected, true)),
                sectors: Some(sectors),
                ..
            } => {
                assert_eq!(selected.geometry_revision, 1);
                assert!((selected.x[0] - 500.0).abs() < 0.1);
                assert_eq!(sectors.len(), 3);
            }
            _ => panic!("expected old consistent reference"),
        }
        worker.join();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn diagnostics_file_is_bounded_and_keeps_recent_events() {
        let dir = temporary_dir();
        let mut diagnostics = DiagnosticLog::new(Some(&dir));
        for index in 0..2_000 {
            diagnostics.log(format!("safe event {index:04} {}", "x".repeat(96)));
        }
        let path = dir.join("sector-learning.log");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(fs::metadata(&path).unwrap().len() <= MAX_DIAGNOSTIC_BYTES);
        assert!(contents.contains("safe event 1999"));
        assert!(!contents.contains("safe event 0000"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn three_consistent_independent_laps_confirm_and_outliers_do_not_move_them() {
        let mut estimator = SectorEstimator::default();
        assert!(estimator.observe(0.327, 0.687, 1));
        assert!(estimator.observe(0.3274, 0.6881, 2));
        assert!(estimator.confirmed_starts().is_none());
        assert!(estimator.observe(0.3268, 0.6874, 3));
        let confirmed = estimator.confirmed_starts().unwrap();
        assert!((confirmed[1] - 0.327).abs() < 0.001);
        assert!((confirmed[2] - 0.6874).abs() < 0.001);
        assert!(!estimator.observe(0.5, 0.8, 4));
        assert_eq!(estimator.confirmed_starts(), Some(confirmed));
    }

    #[test]
    fn monza_like_boundaries_confirm_after_three_coherent_observations() {
        let mut estimator = SectorEstimator::default();
        for (id, s2, s3) in [
            (1, 0.33647, 0.65591),
            (2, 0.33657, 0.65581),
            (3, 0.33637, 0.65601),
        ] {
            assert!(estimator.observe(s2, s3, id));
        }
        let starts = estimator.confirmed_starts().unwrap();
        assert!((starts[1] - 0.33647).abs() < 0.0002);
        assert!((starts[2] - 0.65591).abs() < 0.0002);
    }

    #[test]
    fn sector_width_uses_track_length_and_accepts_short_sectors() {
        assert!(valid_sector_boundaries_for_length(&[0.51, 0.62], 1_944.0));
        assert!(valid_sector_boundaries_for_length(&[0.30, 0.315], 4_318.0));
        assert!(!valid_sector_boundaries_for_length(&[0.30, 0.315], 1_000.0));
        assert!(!valid_sector_boundaries_for_length(
            &[0.30, 0.315],
            f64::NAN
        ));
    }

    #[test]
    fn anchor_projection_stays_near_its_previous_progress_and_direction() {
        let track = Track {
            x: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).cos() * 500.0)
                .collect(),
            y: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).sin() * 500.0)
                .collect(),
            ..Track::default()
        };
        let anchor = BoundaryAnchor {
            x_m: 0.0,
            y_m: 500.0,
            dir_x: -1.0,
            dir_y: 0.0,
            pct_on_revision: 0.25,
        };
        assert!((project_anchor_on_outline(&anchor, &track).unwrap() - 0.25).abs() < 0.001);
        let wrong_direction = BoundaryAnchor {
            dir_x: 1.0,
            ..anchor.clone()
        };
        assert!(project_anchor_on_outline(&wrong_direction, &track).is_none());
        let wrong_progress = BoundaryAnchor {
            pct_on_revision: 0.75,
            ..anchor
        };
        assert!(project_anchor_on_outline(&wrong_progress, &track).is_none());
    }

    #[test]
    fn ambiguous_nearby_crossing_vetoes_anchor_projection() {
        let mut track = Track {
            x: vec![1000.0; 1000],
            y: vec![1000.0; 1000],
            ..Track::default()
        };
        for index in [100, 130] {
            track.x[index] = -10.0;
            track.y[index] = 0.0;
            track.x[index + 1] = 10.0;
            track.y[index + 1] = 0.0;
        }
        let anchor = BoundaryAnchor {
            x_m: 0.0,
            y_m: 0.0,
            dir_x: 1.0,
            dir_y: 0.0,
            pct_on_revision: 0.115,
        };
        assert!(project_anchor_on_outline(&anchor, &track).is_none());
    }

    #[test]
    fn migrated_anchors_preserve_confirmed_physical_boundaries() {
        let offset = 0.02;
        let track = Track {
            x: (0..1000)
                .map(|i| {
                    ((i as f64 / 1000.0 - offset) * std::f64::consts::TAU).cos() as f32 * 500.0
                })
                .collect(),
            y: (0..1000)
                .map(|i| {
                    ((i as f64 / 1000.0 - offset) * std::f64::consts::TAU).sin() as f32 * 500.0
                })
                .collect(),
            ..Track::default()
        };
        let anchors = [0.25, 0.65].map(|pct| {
            let angle = pct * std::f64::consts::TAU;
            BoundaryAnchor {
                x_m: (angle.cos() * 500.0) as f32,
                y_m: (angle.sin() * 500.0) as f32,
                dir_x: -angle.sin() as f32,
                dir_y: angle.cos() as f32,
                pct_on_revision: pct,
            }
        });
        let mut estimator = SectorEstimator::default();
        for fingerprint in 1..=3 {
            assert_eq!(
                estimator.observe_anchors(anchors.to_vec(), 1, fingerprint),
                ObserveResult::Accepted
            );
        }
        let migrated = estimator.migrate_to(&track, 2, 3_142.0).unwrap();
        let starts = migrated.confirmed_starts().unwrap();
        assert!((starts[1] - 0.27).abs() < 0.001 && (starts[2] - 0.67).abs() < 0.001);
        assert!(
            migrated
                .observations
                .iter()
                .all(|item| item.reference_revision == 2 && item.anchors[0].x_m == anchors[0].x_m)
        );
        let wrong_track = Track {
            x: track.x.iter().map(|x| x + 200.0).collect(),
            ..track
        };
        assert!(estimator.migrate_to(&wrong_track, 2, 3_142.0).is_none());
    }

    #[test]
    fn confirms_variable_counts_and_quarantines_count_changes() {
        for count in [1, 2, 3, 4, 10] {
            let boundaries: Vec<_> = (1..count)
                .map(|index| index as f64 / count as f64)
                .collect();
            let mut estimator = SectorEstimator::default();
            for fingerprint in 1..=3 {
                assert_eq!(
                    estimator.observe_boundaries(boundaries.clone(), fingerprint),
                    ObserveResult::Accepted
                );
            }
            let confirmed = estimator.confirmed_starts().unwrap();
            assert_eq!(confirmed.len(), count);
            let different = if count == 2 {
                vec![0.3, 0.7]
            } else {
                vec![0.5]
            };
            assert!(matches!(
                estimator.observe_boundaries(different, 4),
                ObserveResult::Quarantined { .. }
            ));
            assert_eq!(estimator.confirmed_starts(), Some(confirmed));
        }
    }

    #[test]
    fn repeated_outliers_are_quarantined_and_report_a_separate_cluster() {
        let mut estimator = SectorEstimator::default();
        estimator.observe(0.327, 0.687, 1);
        estimator.observe(0.3274, 0.6881, 2);
        estimator.observe(0.3268, 0.6874, 3);
        let confirmed = estimator.confirmed_starts().unwrap();
        assert_eq!(
            estimator.observe_detailed(0.401, 0.721, 4),
            ObserveResult::Quarantined { new_cluster: false }
        );
        assert_eq!(
            estimator.observe_detailed(0.400, 0.720, 5),
            ObserveResult::Quarantined { new_cluster: false }
        );
        assert_eq!(
            estimator.observe_detailed(0.402, 0.722, 6),
            ObserveResult::Quarantined { new_cluster: true }
        );
        assert_eq!(estimator.confirmed_starts(), Some(confirmed));
        assert_eq!(estimator.observations.len(), 3);
        assert_eq!(estimator.quarantined.len(), 3);
    }

    #[test]
    fn lap_quality_latches_implausible_speed_and_track_metadata() {
        let mut lap = CurrentLap::new(&telemetry_sample(0.0, 0, 0.1, 30.0), false);
        let mut bad_speed = telemetry_sample(0.05, 50, 0.101, f32::NAN);
        lap.add_sample(&bad_speed, false);
        assert!(lap.invalid);
        assert!(lap.invalid_reasons.contains(&"invalid_speed"));

        bad_speed = telemetry_sample(0.10, 100, 0.102, 30.0);
        bad_speed.track_length_m = f32::NAN;
        lap.add_sample(&bad_speed, false);
        assert!(lap.invalid);
        assert!(lap.invalid_reasons.contains(&"invalid_track_length"));
    }

    #[test]
    fn lap_progress_is_relative_to_its_first_sample_and_wraps_at_zero() {
        let mut first = telemetry_sample(0.0, 0, 0.999, 30.0);
        first.position = Some((-132.0, -393.0));
        let mut lap = CurrentLap::new(&first, false);
        assert_eq!(lap.samples[0].pct, 0.0);

        let mut after_wrap = telemetry_sample(0.05, 50, 0.001, 30.0);
        after_wrap.position = Some((-130.0, -392.0));
        lap.add_sample(&after_wrap, false);

        assert!((lap.samples[1].pct - 0.002).abs() < 1e-9);
        assert!(!lap.invalid_reasons.contains(&"projected_progress_reversed"));
        assert!(
            !lap.invalid_reasons
                .contains(&"invalid_progress_or_sample_limit")
        );
    }

    #[test]
    fn finish_position_accepts_small_cyclic_progress_offsets_at_the_line() {
        assert!(finish_position_matches(
            Some((-136.7, -391.2)),
            Some((-132.4, -393.1)),
            Some(0.9827),
            Some(0.9992),
            true,
        ));
        assert!(finish_position_matches(
            Some((-135.2, -391.0)),
            Some((-130.9, -392.9)),
            Some(0.9840),
            Some(0.0006),
            true,
        ));
        assert!(!finish_position_matches(
            Some((0.0, 0.0)),
            Some((100.0, 0.0)),
            Some(0.98),
            Some(0.01),
            true,
        ));
    }

    #[test]
    fn nearest_lap_match_resolves_close_durations_and_keeps_oldest_ties() {
        assert_eq!(
            nearest_telemetry_candidate(&[(0, 42), (1, 0)]),
            Some((1, 0))
        );
        assert_eq!(
            nearest_telemetry_candidate(&[(0, 10), (1, 10)]),
            Some((0, 10))
        );
    }

    #[test]
    fn duplicate_fingerprints_are_ignored_and_observation_history_is_bounded() {
        let mut estimator = SectorEstimator::default();
        for fingerprint in 1..=100 {
            assert!(estimator.observe(0.3, 0.7, fingerprint));
        }
        assert_eq!(estimator.observations.len(), MAX_OBSERVATIONS);
        assert!(!estimator.observe(0.3, 0.7, 100));
        assert!(estimator.confirmed_starts().is_some());
    }

    #[test]
    fn widely_disagreeing_observations_stay_provisional_until_a_cluster_forms() {
        let mut estimator = SectorEstimator::default();
        assert!(estimator.observe(0.25, 0.60, 1));
        assert!(estimator.observe(0.40, 0.72, 2));
        assert!(estimator.observe(0.30, 0.66, 3));
        assert!(estimator.confirmed_starts().is_none());
        assert!(estimator.observe(0.301, 0.661, 4));
        assert!(estimator.observe(0.299, 0.659, 5));
        assert!(estimator.confirmed_starts().is_some());
    }

    #[test]
    fn sidecar_round_trips_and_rejects_corrupt_or_mismatched_data() {
        let dir = temporary_dir();
        let path = sidecar_path(&dir, &key());
        let mut estimator = SectorEstimator::default();
        estimator.observe(0.327, 0.687, 1);
        estimator.observe(0.3275, 0.688, 2);
        estimator.observe(0.3265, 0.6875, 3);
        estimator.save(&path, key(), 4318.0).unwrap();
        let loaded = SectorEstimator::from_file(&path, &key(), 4318.0);
        assert!(loaded.confirmed_starts().is_some());
        assert!(
            SectorEstimator::from_file(&path, &key(), 5100.0)
                .confirmed_starts()
                .is_none()
        );
        fs::write(&path, b"not json").unwrap();
        assert!(
            SectorEstimator::from_file(&path, &key(), 4318.0)
                .confirmed_starts()
                .is_none()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn layout_cache_checks_identity_and_recovers_from_its_backup() {
        let dir = temporary_dir();
        let key = key();
        let path = layout_path(&dir, &key);
        let track = Track {
            name: "Red Bull Ring".into(),
            length: 4318.0,
            x: (0..100)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 100.0;
                    angle.cos() * 500.0
                })
                .collect(),
            y: (0..100)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 100.0;
                    angle.sin() * 500.0
                })
                .collect(),
            sectors: FALLBACK_SECTORS.to_vec(),
            ..Track::default()
        };
        save_layout(&path, &key, &track, true).unwrap();
        assert!(load_layout(&path, &key, "Red Bull Ring", 4318.0).is_some());
        assert!(load_layout(&path, &key, "Red Bull Ring", 5000.0).is_none());
        assert!(
            load_layout(
                &path,
                &EvoTrackKey::new("Red Bull Ring", "National", 4318.0).unwrap(),
                "Red Bull Ring",
                4318.0
            )
            .is_none()
        );

        let backup = backup_path(&path);
        fs::copy(&path, &backup).unwrap();
        fs::write(&path, b"corrupt primary").unwrap();
        assert!(load_layout(&path, &key, "Red Bull Ring", 4318.0).is_some());
        save_layout(&path, &key, &track, true).unwrap();
        assert!(read_layout_file(&backup).is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_layout_schema_is_ignored_and_replaced() {
        let dir = temporary_dir();
        let key = key();
        let path = layout_path(&dir, &key);
        let track = Track {
            name: "Red Bull Ring".into(),
            length: 4318.0,
            x: (0..100)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 100.0;
                    angle.cos() * 500.0
                })
                .collect(),
            y: (0..100)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 100.0;
                    angle.sin() * 500.0
                })
                .collect(),
            sectors: FALLBACK_SECTORS.to_vec(),
            ..Track::default()
        };
        save_layout(&path, &key, &track, true).unwrap();
        let mut old = read_layout_file(&path).unwrap();
        old.schema_version = 1;
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(load_layout(&path, &key, "Red Bull Ring", 4318.0).is_none());

        save_layout(&path, &key, &track, true).unwrap();
        assert!(load_layout(&path, &key, "Red Bull Ring", 4318.0).is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_sector_schema_is_ignored_and_replaced() {
        let dir = temporary_dir();
        let path = sidecar_path(&dir, &key());
        let mut estimator = SectorEstimator::default();
        estimator.observe(0.327, 0.687, 1);
        estimator.observe(0.3275, 0.688, 2);
        estimator.observe(0.3265, 0.6875, 3);
        estimator.save(&path, key(), 4318.0).unwrap();
        let mut old = read_sector_file(&path).unwrap();
        old.schema_version = 1;
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(
            SectorEstimator::from_file(&path, &key(), 4318.0)
                .confirmed_starts()
                .is_none()
        );

        estimator.save(&path, key(), 4318.0).unwrap();
        assert!(
            SectorEstimator::from_file(&path, &key(), 4318.0)
                .confirmed_starts()
                .is_some()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sector_save_keeps_a_valid_backup_when_the_primary_is_corrupt() {
        let dir = temporary_dir();
        let path = sidecar_path(&dir, &key());
        let mut estimator = SectorEstimator::default();
        estimator.observe(0.327, 0.687, 1);
        estimator.observe(0.3275, 0.688, 2);
        estimator.observe(0.3265, 0.6875, 3);
        estimator.save(&path, key(), 4318.0).unwrap();
        let backup = backup_path(&path);
        fs::copy(&path, &backup).unwrap();
        fs::write(&path, b"corrupt primary").unwrap();
        assert!(
            SectorEstimator::from_file(&path, &key(), 4318.0)
                .confirmed_starts()
                .is_some()
        );
        estimator.observe(0.3271, 0.6872, 4);
        estimator.save(&path, key(), 4318.0).unwrap();
        assert!(read_sector_file(&backup).is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_does_not_overwrite_a_newer_sidecar_schema() {
        let dir = temporary_dir();
        let path = sidecar_path(&dir, &key());
        let mut estimator = SectorEstimator::default();
        estimator.observe(0.327, 0.687, 1);
        estimator.observe(0.3275, 0.688, 2);
        estimator.observe(0.3265, 0.6875, 3);
        estimator.save(&path, key(), 4318.0).unwrap();
        let mut newer = read_sector_file(&path).unwrap();
        newer.schema_version += 1;
        let newer_bytes = serde_json::to_vec(&newer).unwrap();
        fs::write(&path, &newer_bytes).unwrap();
        assert!(estimator.save(&path, key(), 4318.0).is_err());
        assert_eq!(fs::read(&path).unwrap(), newer_bytes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_replacement_leaves_a_recoverable_previous_file() {
        let dir = temporary_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        fs::write(&path, b"old-good").unwrap();
        let backup = backup_path(&path);
        fs::rename(&path, &backup).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"old-good");
        atomic_replace(&path, b"new-good").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new-good");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interpolation_uses_distance_progress_not_elapsed_time_ratio() {
        let samples = [
            LapSample {
                lap_time: 35.95,
                pct: 0.2995,
                ..Default::default()
            },
            LapSample {
                lap_time: 36.05,
                pct: 0.30,
                ..Default::default()
            },
            LapSample {
                lap_time: 79.95,
                pct: 0.7195,
                ..Default::default()
            },
            LapSample {
                lap_time: 80.05,
                pct: 0.72,
                ..Default::default()
            },
        ];
        assert!((position_at_lap_time(&samples, 36.0).unwrap() - 0.29975).abs() < 1e-12);
        assert!((position_at_lap_time(&samples, 80.0).unwrap() - 0.71975).abs() < 1e-12);
        assert_eq!(36.0 / 120.0, 0.30);
        assert_ne!(80.0 / 120.0, 0.72);
        assert_eq!(
            position_at_lap_time(&samples, 50.0),
            Err("sample_gap_over_250ms")
        );

        let wide_gap = [
            LapSample {
                lap_time: 10.0,
                pct: 0.1,
                ..Default::default()
            },
            LapSample {
                lap_time: 10.8,
                pct: 0.2,
                ..Default::default()
            },
        ];
        assert_eq!(
            position_at_lap_time(&wide_gap, 10.4),
            Err("sample_gap_over_250ms")
        );
    }

    #[test]
    fn candidate_uses_the_logged_split_clock_to_find_distance_boundaries() {
        let mut samples = Vec::new();
        for step in 0..=2_400 {
            let time = step as f64 * 0.05;
            let pct = if time <= 36.0 {
                0.30 * time / 36.0
            } else if time <= 80.0 {
                0.30 + 0.42 * (time - 36.0) / 44.0
            } else {
                0.72 + 0.28 * (time - 80.0) / 40.0
            };
            samples.push(LapSample {
                lap_time: time,
                pct,
                absolute_pct: pct,
                position: ((pct * 4_318.0) as f32, 0.0),
                reference_revision: 1,
            });
        }
        let mut lap = TelemetryLap {
            epoch: 7,
            reference_revision: 1,
            closed: true,
            track_length_m: 4_318.0,
            completed_at: 120.0,
            lap_ms: 120_000,
            started_at_zero: true,
            valid: true,
            in_pit: false,
            all_solo: true,
            out_lap: false,
            invalid: false,
            invalid_reasons: Vec::new(),
            finish_gap_valid: true,
            finish_position_valid: true,
            full_course_coverage: true,
            samples,
        };
        let log = LoggedLap {
            split_ms: vec![36_000, 44_000, 40_000],
            lap_ms: 120_000,
            fingerprint: 91,
        };
        let boundaries = lap.candidate(&log).unwrap();
        let (s2, s3) = (boundaries[0].pct_on_revision, boundaries[1].pct_on_revision);
        assert!((s2 - 0.30).abs() < 1e-9);
        assert!((s3 - 0.72).abs() < 1e-9);
        assert_ne!(s3, 80.0 / 120.0);
        lap.finish_position_valid = false;
        assert_eq!(
            lap.candidate(&log).unwrap_err(),
            "finish_line_projection_mismatch"
        );
    }

    #[test]
    fn variable_splits_produce_complete_anchored_sector_vectors() {
        let samples: Vec<_> = (0..=2_400)
            .map(|step| {
                let time = step as f64 * 0.05;
                let pct = time / 120.0;
                LapSample {
                    lap_time: time,
                    pct,
                    absolute_pct: pct,
                    position: ((pct * 4318.0) as f32, 0.0),
                    reference_revision: 1,
                }
            })
            .collect();
        let lap = TelemetryLap {
            epoch: 1,
            reference_revision: 1,
            closed: false,
            track_length_m: 4318.0,
            completed_at: 120.0,
            lap_ms: 120_000,
            started_at_zero: true,
            valid: true,
            in_pit: false,
            all_solo: true,
            out_lap: false,
            invalid: false,
            invalid_reasons: Vec::new(),
            finish_gap_valid: true,
            finish_position_valid: true,
            full_course_coverage: true,
            samples,
        };
        for count in [2, 3, 4, 10] {
            let logged = LoggedLap {
                split_ms: vec![120_000 / count as u32; count],
                lap_ms: 120_000,
                fingerprint: 1,
            };
            let anchors = lap.candidate(&logged).unwrap();
            assert_eq!(anchors.len(), count - 1);
            let mut estimator = SectorEstimator::default();
            for id in 1..=3 {
                assert_eq!(
                    estimator.observe_anchors(anchors.clone(), 1, id),
                    ObserveResult::Accepted
                );
            }
            assert_eq!(estimator.confirmed_starts().unwrap().len(), count);
        }
    }

    #[test]
    fn three_subsequent_matched_laps_confirm_without_changing_active_sectors() {
        let request = SessionRequest {
            epoch: 1,
            key: key(),
            track_name: "Red Bull Ring".into(),
            track_length_m: 4318.0,
        };
        let mut estimator = SectorEstimator::default();
        let mut completed = VecDeque::new();
        let mut logs = VecDeque::new();
        let mut fingerprints = HashSet::new();
        let mut fingerprint_order = VecDeque::new();
        let mut diagnostics = DiagnosticLog::new(None);
        let active_sectors = FALLBACK_SECTORS;
        let radius = 4318.0 / std::f64::consts::TAU;
        let mut learned = EvoLearnedFile::blank(&request);
        learned.active_outline = Some(Track {
            name: request.track_name.clone(),
            length: 4318.0,
            x: (0..1000)
                .map(|index| {
                    (radius * (index as f64 * std::f64::consts::TAU / 1000.0).cos()) as f32
                })
                .collect(),
            y: (0..1000)
                .map(|index| {
                    (radius * (index as f64 * std::f64::consts::TAU / 1000.0).sin()) as f32
                })
                .collect(),
            sectors: active_sectors.to_vec(),
            geometry_revision: 1,
            ..Track::default()
        });
        learned.outline_valid = true;
        for lap_index in 0..3 {
            let sample = |step: usize| {
                let pct = step as f64 / 2400.0;
                let angle = pct * std::f64::consts::TAU;
                let radius = 4318.0 / std::f64::consts::TAU;
                TelemetrySample {
                    epoch: 1,
                    reference_revision: 1,
                    closed: true,
                    seconds: lap_index as f64 * 120.0 + step as f64 * 0.05,
                    lap_counter: lap_index,
                    lap_clock_ms: (step * 50) as u32,
                    last_lap_ms: 120_000,
                    lap_valid: true,
                    in_pit: false,
                    active_cars: 1,
                    speed_mps: 36.0,
                    track_length_m: 4318.0,
                    position: Some(((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)),
                    projected_pct: Some(pct),
                    projection_distance_m: Some(0.0),
                }
            };
            let mut current = CurrentLap::new(&sample(0), false);
            for step in 1..2400 {
                current.add_sample(&sample(step), false);
            }
            completed.push_back(current.finish(
                120_000,
                (lap_index + 1) as f64 * 120.0,
                false,
                true,
                true,
            ));
            logs.push_back(PendingLog {
                epoch: 1,
                received_at: (lap_index + 1) as f64 * 120.0,
                lap: LoggedLap {
                    split_ms: vec![36_000, 44_000, 40_000],
                    lap_ms: 120_000,
                    fingerprint: u64::from(lap_index + 1),
                },
            });
            try_match_laps(
                &mut logs,
                &mut completed,
                MatchContext {
                    session: &request,
                    estimator: Some(&mut estimator),
                    learned: Some(&mut learned),
                    track_cache: None,
                    fingerprints: &mut fingerprints,
                    fingerprint_order: &mut fingerprint_order,
                    diagnostics: &mut diagnostics,
                    current_seconds: (lap_index + 1) as f64 * 120.0,
                },
            );
            assert_eq!(active_sectors, FALLBACK_SECTORS);
            assert_eq!(estimator.confirmed_starts().is_some(), lap_index == 2);
            assert_eq!(learned.observations.len(), lap_index as usize + 1);
        }
    }

    fn temporary_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("opensimhud-evo-sector-test-{nonce}"))
    }

    fn telemetry_sample(
        seconds: f64,
        lap_clock_ms: u32,
        pct: f64,
        speed_mps: f32,
    ) -> TelemetrySample {
        TelemetrySample {
            epoch: 1,
            reference_revision: 1,
            closed: true,
            seconds,
            lap_counter: 1,
            lap_clock_ms,
            last_lap_ms: 0,
            lap_valid: true,
            in_pit: false,
            active_cars: 1,
            speed_mps,
            track_length_m: 4_318.0,
            position: Some(((pct * 4_318.0) as f32, 0.0)),
            projected_pct: Some(pct),
            projection_distance_m: Some(0.0),
        }
    }
}
