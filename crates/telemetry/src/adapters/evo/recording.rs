//! Replay the versioned JSONL snapshots produced by the EVO research probe.
use super::{Evo, Graphics, Physics, Static};
use crate::adapter::{AdapterEvent, GameAdapter};
use crate::input::Input;
use crate::model::{Capabilities, Connection, Game};
use std::io::{self, BufRead, BufReader, Read};
use std::time::Instant;

#[derive(serde::Deserialize)]
struct Record {
    format: String,
    seconds: f64,
    physics_hex: String,
    graphics_hex: String,
    static_hex: String,
}

fn bytes(hex: &str, size: usize) -> io::Result<Vec<u8>> {
    if hex.len() != size * 2 || !hex.is_ascii() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid page length",
        ));
    }
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let s = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(s, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid page hex"))
        })
        .collect()
}

/// Real-time playback of versioned EVO snapshots. Malformed records stop playback.
pub struct Replay<R: Read + Send> {
    reader: BufReader<R>,
    next: Option<Record>,
    last_record_time: f64,
    start: Instant,
    clock: f64,
    step: Option<f64>,
    evo: Evo,
    connected: bool,
    last_seen: f64,
    finished: bool,
    pending_update: bool,
    pub error: Option<String>,
}

impl<R: Read + Send> Replay<R> {
    pub fn new(reader: R) -> io::Result<Self> {
        let mut result = Self {
            reader: BufReader::new(reader),
            next: None,
            last_record_time: 0.0,
            start: Instant::now(),
            clock: 0.0,
            step: None,
            evo: Evo::default(),
            connected: false,
            last_seen: f64::NEG_INFINITY,
            finished: false,
            pending_update: false,
            error: None,
        };
        result.next = result.read_record()?;
        if result.next.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty EVO recording",
            ));
        }
        Ok(result)
    }

    /// Advance by a fixed interval on each poll for offline tools.
    pub fn stepped(reader: R, step: f64) -> io::Result<Self> {
        if !step.is_finite() || step <= 0.0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid step"));
        }
        let mut result = Self::new(reader)?;
        result.step = Some(step);
        Ok(result)
    }

    pub fn finished(&self) -> bool {
        self.finished && !self.pending_update
    }
    pub fn clock(&self) -> f64 {
        self.clock
    }

    fn read_record(&mut self) -> io::Result<Option<Record>> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.len() > 64_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "EVO record too large",
            ));
        }
        let record: Record = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if record.format != "opensimhud-evo-1"
            || !record.seconds.is_finite()
            || record.seconds < self.last_record_time
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid EVO record header",
            ));
        }
        self.last_record_time = record.seconds;
        Ok(Some(record))
    }
}

impl<R: Read + Send> GameAdapter for Replay<R> {
    fn game(&self) -> Game {
        Game::Evo
    }
    fn capabilities(&self) -> Capabilities {
        self.evo.capabilities()
    }
    fn input(&self) -> &Input {
        &self.evo.input
    }
    fn poll(&mut self) -> AdapterEvent {
        if self.pending_update {
            self.pending_update = false;
            return AdapterEvent::Updated;
        }
        self.clock = self.step.map_or_else(
            || self.start.elapsed().as_secs_f64(),
            |step| self.clock + step,
        );
        let now = self.clock;
        let mut updated = false;
        while self.next.as_ref().is_some_and(|r| r.seconds <= now) {
            let r = self.next.take().unwrap();
            let decoded = (|| -> io::Result<_> {
                let p = bytes(&r.physics_hex, 800)?;
                let g = bytes(&r.graphics_hex, 8192)?;
                let s = bytes(&r.static_hex, 2048)?;
                Ok((
                    Physics::decode(&p),
                    Graphics::decode(&g),
                    Static::decode(&s),
                ))
            })();
            match decoded {
                Ok((_, Some(g), _)) if g.status != 2 => {
                    self.last_seen = r.seconds;
                    self.connected = false;
                    updated = false;
                }
                Ok((Some(p), Some(g), Some(s))) => {
                    self.last_seen = r.seconds;
                    self.connected = g.status == 2;
                    if self.connected {
                        self.evo.apply(r.seconds, p, g, s);
                        updated = true;
                    }
                }
                _ => {
                    self.error = Some("invalid EVO snapshot".into());
                    self.finished = true;
                    break;
                }
            }
            match self.read_record() {
                Ok(next) => self.next = next,
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.finished = true;
                    break;
                }
            }
        }
        // Keep the final snapshot visible until the next poll.
        if self.next.is_none() && !updated {
            self.finished = true;
        }
        let connected = self.connected && !self.finished && now - self.last_seen < 3.0;
        if connected != self.evo.connected {
            self.evo.connected = connected;
            self.pending_update = connected && updated;
            return AdapterEvent::Connection(if connected {
                Connection::Connected
            } else {
                Connection::WaitingForGame
            });
        }
        if updated && connected {
            AdapterEvent::Updated
        } else {
            AdapterEvent::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outline::{OutlineCandidate, RouteTopology, SamplingDomain};

    #[test]
    #[ignore = "requires the locally supplied Touristenfahrten recording"]
    fn tourist_recording_learns_open_outline_after_pit_exit() {
        let path = std::env::var_os("OPENSIMHUD_TOURIST_RECORDING").expect("set recording path");
        let mut replay = Replay::stepped(std::fs::File::open(path).unwrap(), 0.1).unwrap();
        let mut learned_lap = None;
        while !replay.finished() {
            replay.poll();
            if learned_lap.is_none() && !replay.evo.input.track.x.is_empty() {
                learned_lap = replay.evo.last_laps;
                assert!(!replay.evo.input.track.closed);
            }
        }
        assert!(replay.error.is_none());
        assert_eq!(learned_lap, Some(1));
        assert_eq!(replay.evo.last_laps, Some(2));
    }

    #[test]
    #[ignore = "requires the locally supplied Suzuka recording"]
    fn suzuka_recording_rejects_spin_and_refines_at_simulated_boundary() {
        let path = std::env::var_os("OPENSIMHUD_SUZUKA_RECORDING").expect("set recording path");
        let mut replay = Replay::stepped(std::fs::File::open(&path).unwrap(), 0.1).unwrap();
        let mut first = None;
        let mut after_spin = None;
        while !replay.finished() {
            replay.poll();
            if after_spin.is_none() && replay.evo.last_laps == Some(3) {
                after_spin = Some(replay.evo.outline_history.representative_count());
            }
            let track = &replay.evo.input.track;
            if first.is_none() && !track.x.is_empty() {
                first = Some(track.clone());
            }
        }
        assert!(replay.error.is_none());
        assert_eq!(
            after_spin,
            Some(1),
            "spin lap must not enter the candidate history"
        );
        let track = first.expect("first complete lap produced an outline");
        assert_eq!(track.geometry_revision, 1);
        assert_eq!(
            replay.evo.input.track.x, track.x,
            "valid outline changed during the session"
        );
        let key = replay.evo.track_key.as_ref().unwrap();
        let reference = OutlineCandidate {
            track_key: format!("{:016x}", key.hash()),
            topology: RouteTopology::Closed,
            domain: SamplingDomain::MeasuredDistance,
            valid: true,
            session_id: 0,
            points: track.x.into_iter().zip(track.y).collect(),
        };
        let selected = replay
            .evo
            .outline_history
            .select(&reference, true)
            .expect("clean laps should corroborate a later reference");
        assert_ne!(selected.points, reference.points);
        let (seconds, physics, mut graphics, mut statics) =
            BufReader::new(std::fs::File::open(path).unwrap())
                .lines()
                .filter_map(|line| {
                    let record: Record = serde_json::from_str(&line.ok()?).ok()?;
                    Some((
                        record.seconds,
                        Physics::decode(&bytes(&record.physics_hex, 800).ok()?)?,
                        Graphics::decode(&bytes(&record.graphics_hex, 8192).ok()?)?,
                        Static::decode(&bytes(&record.static_hex, 2048).ok()?)?,
                    ))
                })
                .last()
                .unwrap();
        graphics.total_lap_count = 0;
        graphics.current_session_lap = 0;
        graphics.lap_time_ms = 0;
        graphics.is_in_pit_lane = false;
        statics.session_name = "Qualifying".into();
        replay.evo.apply(seconds + 1.0, physics, graphics, statics);
        assert_eq!(replay.evo.input.track.geometry_revision, 2);
        assert_eq!(
            replay.evo.input.track.x,
            selected.points.iter().map(|p| p.0).collect::<Vec<_>>()
        );
        assert_eq!(
            replay.evo.input.track.sectors,
            super::super::EVO_FALLBACK_SECTORS
        );
    }

    #[test]
    #[ignore = "requires the locally supplied EVO edge-case recording"]
    fn edge_case_recording_keeps_complete_revisioned_outlines() {
        let path = std::env::var_os("OPENSIMHUD_EVO_EDGE_RECORDING").expect("set recording path");
        let mut replay = Replay::stepped(std::fs::File::open(path).unwrap(), 0.1).unwrap();
        let mut saw_outline = false;
        while !replay.finished() {
            replay.poll();
            let track = &replay.evo.input.track;
            if !track.x.is_empty() {
                saw_outline = true;
                assert_eq!(track.x.len(), 1000);
                assert_eq!(track.y.len(), 1000);
                assert!(track.geometry_revision > 0);
                assert_eq!(track.sectors, super::super::EVO_FALLBACK_SECTORS);
            }
        }
        assert!(replay.error.is_none());
        assert!(saw_outline, "recording should produce at least one outline");
    }

    #[test]
    #[ignore = "requires the locally supplied Brands Hatch recording"]
    fn brands_recording_preserves_measured_length_at_the_seam() {
        let path = std::env::var_os("OPENSIMHUD_EVO_BRANDS_RECORDING").expect("set recording path");
        let mut replay = Replay::stepped(std::fs::File::open(path).unwrap(), 0.1).unwrap();
        let mut outline = None;
        while !replay.finished() {
            replay.poll();
            if !replay.evo.input.track.x.is_empty() {
                outline = Some(replay.evo.input.track.clone());
                break;
            }
        }
        let track = outline.expect("recording should produce an outline");
        assert!((track.length - 1944.0).abs() < 2.0);
        let length: f64 = (0..track.x.len())
            .map(|i| {
                let next = (i + 1) % track.x.len();
                f64::from((track.x[next] - track.x[i]).hypot(track.y[next] - track.y[i]))
            })
            .sum();
        assert!(
            (1906.0..=1913.0).contains(&length),
            "measured length {length}"
        );
        let segment = |i: usize| {
            let next = (i + 1) % track.x.len();
            (track.x[next] - track.x[i]).hypot(track.y[next] - track.y[i])
        };
        assert!((segment(999) - segment(500)).abs() < 2.0);
    }

    #[test]
    fn playback_delivers_snapshot_then_disconnects() {
        let mut physics = vec![0u8; 800];
        physics[4..8].copy_from_slice(&0.75f32.to_le_bytes());
        physics[16..20].copy_from_slice(&2i32.to_le_bytes());
        physics[28..32].copy_from_slice(&42f32.to_le_bytes());
        let mut graphics = vec![0u8; 8192];
        graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
        let mut statics = vec![0u8; 2048];
        statics[..3].copy_from_slice(b"1.0");
        statics[136..139].copy_from_slice(b"Spa");
        let hex = |b: &[u8]| -> String { b.iter().map(|n| format!("{n:02x}")).collect() };
        let line = serde_json::json!({"format":"opensimhud-evo-1", "seconds":0.0,
            "physics_hex":hex(&physics), "graphics_hex":hex(&graphics), "static_hex":hex(&statics)})
        .to_string();
        let mut replay = Replay::new(line.as_bytes()).unwrap();
        assert_eq!(
            replay.poll(),
            AdapterEvent::Connection(Connection::Connected)
        );
        assert_eq!(replay.poll(), AdapterEvent::Updated);
        assert_eq!(replay.input().player.speed, 42.0);
        assert_eq!(replay.input().player.throttle, 0.75);
        assert_eq!(replay.input().track.name, "Spa");
        assert_eq!(
            replay.poll(),
            AdapterEvent::Connection(Connection::WaitingForGame)
        );
    }

    #[test]
    fn rejects_other_formats_and_bad_lengths() {
        assert!(Replay::new(&b"{}\n"[..]).is_err());
        assert!(bytes("00", 2).is_err());
    }

    #[test]
    fn menu_snapshot_with_empty_static_page_is_not_an_error() {
        let line = serde_json::json!({"format":"opensimhud-evo-1", "seconds":0.0,
            "physics_hex":"00".repeat(800), "graphics_hex":"00".repeat(8192),
            "static_hex":"00".repeat(2048)})
        .to_string();
        let mut replay = Replay::stepped(line.as_bytes(), 0.1).unwrap();
        assert_eq!(replay.poll(), AdapterEvent::Idle);
        assert!(replay.finished());
        assert!(replay.error.is_none());
    }
}
