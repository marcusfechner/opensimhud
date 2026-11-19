//! Bounded background persistence for ACC's native-progress geometry history.

use crate::{
    adapters::outline::{CandidateHistory, RouteTopology, SamplingDomain},
    model::Track,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
};

const SCHEMA: u32 = 3;
const MAX_BYTES: u64 = 2 * 1024 * 1024;
static NEXT_SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static LAST_SAVE_SEQUENCE: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();

pub(super) fn next_sequence() -> u64 {
    NEXT_SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccLearnedFile {
    schema_version: u32,
    pub track_name: String,
    pub track: Track,
    pub outline_valid: bool,
    pub next_session_id: u64,
    pub candidate_pool: CandidateHistory,
}

impl AccLearnedFile {
    pub fn new(track_name: String, track: Track) -> Self {
        Self {
            schema_version: SCHEMA,
            track_name,
            track,
            outline_valid: false,
            next_session_id: 1,
            candidate_pool: CandidateHistory::default(),
        }
    }

    fn valid(&self, key: &str, length: f64) -> bool {
        let t = &self.track;
        let length_matches =
            !length.is_finite() || length <= 0.0 || (t.length - length).abs() <= 1.0;
        self.schema_version == SCHEMA
            && self.track_name == key
            && t.name == key
            && t.length.is_finite()
            && t.length > 0.0
            && length_matches
            && t.closed
            && t.x.len() == 1000
            && t.y.len() == 1000
            && t.geometry_revision > 0
            && t.geometry_revision < u32::MAX
            && t.x
                .iter()
                .chain(&t.y)
                .all(|v| v.is_finite() && v.abs() <= 100_000.0)
            && !t.sectors.is_empty()
            && t.sectors[0] == 0.0
            && t.sectors
                .iter()
                .all(|v| v.is_finite() && (0.0..1.0).contains(v))
            && t.sectors.windows(2).all(|w| w[0] < w[1])
            && self.next_session_id > self.candidate_pool.last_session_id()
            && self.candidate_pool.is_valid(
                key,
                RouteTopology::Closed,
                SamplingDomain::GameProgress,
            )
    }

    pub fn load(root: &Path, key: &str, length: f64) -> Option<Self> {
        let path = path_for(root, key);
        [path.clone(), backup(&path)].iter().find_map(|path| {
            let file = read(path)?;
            file.valid(key, length).then_some(file)
        })
    }
}

fn path_for(root: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(format!("{safe}.json"))
}
fn backup(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}
fn read(path: &Path) -> Option<AccLearnedFile> {
    if fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn save(root: &Path, state: &AccLearnedFile) -> io::Result<()> {
    if !state.valid(&state.track_name, state.track.length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid ACC learned state",
        ));
    }
    let path = path_for(root, &state.track_name);
    let backup = backup(&path);
    fs::create_dir_all(root)?;
    for old in [&path, &backup] {
        if fs::metadata(old).is_ok_and(|metadata| metadata.len() > MAX_BYTES) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized ACC cache cannot be safely replaced",
            ));
        }
        if fs::metadata(old).is_ok_and(|m| m.len() <= MAX_BYTES)
            && let Ok(bytes) = fs::read(old)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            if value
                .get("schemaVersion")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v > u64::from(SCHEMA))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "newer ACC cache version",
                ));
            }
            if read(old).is_some_and(|file| file.track_name != state.track_name) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ACC cache key collision",
                ));
            }
        }
    }
    let bytes = serde_json::to_vec(state).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ACC cache too large",
        ));
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    let primary_valid =
        read(&path).is_some_and(|file| file.valid(&state.track_name, state.track.length));
    let backup_valid =
        read(&backup).is_some_and(|file| file.valid(&state.track_name, state.track.length));
    if path.exists() {
        if !primary_valid && backup_valid {
            fs::remove_file(&path)?;
        } else {
            if backup.exists() {
                fs::remove_file(&backup)?;
            }
            fs::rename(&path, &backup)?;
        }
    }
    if let Err(error) = fs::rename(&temp, &path) {
        if !path.exists() && backup.exists() {
            let _ = fs::copy(&backup, &path);
        }
        return Err(error);
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub sequence: u64,
    pub state: AccLearnedFile,
}

pub(super) struct CacheWriter {
    tx: SyncSender<Snapshot>,
}

impl CacheWriter {
    pub fn new(root: PathBuf) -> Option<Self> {
        let (tx, rx) = mpsc::sync_channel::<Snapshot>(8);
        thread::Builder::new()
            .name("acc-outline-cache".into())
            .spawn(move || {
                while let Ok(snapshot) = rx.recv() {
                    let key = path_for(&root, &snapshot.state.track_name);
                    let Ok(mut latest) = LAST_SAVE_SEQUENCE
                        .get_or_init(|| Mutex::new(HashMap::new()))
                        .lock()
                    else {
                        continue;
                    };
                    if latest
                        .get(&key)
                        .is_some_and(|seq| *seq >= snapshot.sequence)
                    {
                        continue;
                    }
                    latest.insert(key, snapshot.sequence);
                    let _ = save(&root, &snapshot.state);
                }
            })
            .ok()?;
        Some(Self { tx })
    }

    pub fn try_send(&self, snapshot: Snapshot) -> Result<(), Box<Snapshot>> {
        match self.tx.try_send(snapshot) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(snapshot) | TrySendError::Disconnected(snapshot)) => {
                Err(Box::new(snapshot))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn versioned_cache_recovers_backup_and_rejects_mismatched_key() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opensimhud-acc-cache-{nonce}"));
        let track = Track {
            name: "Monza".into(),
            length: 5793.0,
            sectors: vec![0.0, 0.3, 0.7],
            x: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).cos() * 500.0)
                .collect(),
            y: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).sin() * 500.0)
                .collect(),
            geometry_revision: 1,
            ..Track::default()
        };
        let mut state = AccLearnedFile::new("Monza".into(), track);
        state.outline_valid = true;
        save(&dir, &state).unwrap();
        save(&dir, &state).unwrap();
        assert!(AccLearnedFile::load(&dir, "Monza", 5793.0).is_some());
        assert!(AccLearnedFile::load(&dir, "Monza", 6000.0).is_none());
        fs::write(path_for(&dir, "Monza"), b"corrupt").unwrap();
        assert!(AccLearnedFile::load(&dir, "Monza", 5793.0).is_some());
        let newer = br#"{"schemaVersion":99}"#;
        fs::write(path_for(&dir, "Monza"), newer).unwrap();
        assert!(save(&dir, &state).is_err());
        assert_eq!(fs::read(path_for(&dir, "Monza")).unwrap(), newer);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_snapshot_from_another_writer_cannot_overwrite_newer_state() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opensimhud-acc-order-{nonce}"));
        let track = Track {
            name: "Test".into(),
            length: 1000.0,
            sectors: vec![0.0, 0.5],
            x: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).cos() * 150.0)
                .collect(),
            y: (0..1000)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1000.0).sin() * 150.0)
                .collect(),
            geometry_revision: 1,
            ..Track::default()
        };
        let old = AccLearnedFile::new("Test".into(), track);
        let mut new = old.clone();
        new.track.geometry_revision = 2;
        let old_snapshot = Snapshot {
            sequence: next_sequence(),
            state: old,
        };
        let new_snapshot = Snapshot {
            sequence: next_sequence(),
            state: new,
        };
        let older_writer = CacheWriter::new(dir.clone()).unwrap();
        let newer_writer = CacheWriter::new(dir.clone()).unwrap();
        assert!(newer_writer.try_send(new_snapshot).is_ok());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while AccLearnedFile::load(&dir, "Test", 1000.0).is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(older_writer.try_send(old_snapshot).is_ok());
        drop(older_writer);
        drop(newer_writer);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            AccLearnedFile::load(&dir, "Test", 1000.0)
                .unwrap()
                .track
                .geometry_revision,
            2
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
