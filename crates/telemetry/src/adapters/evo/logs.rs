//! Bounded, read-only tailing and parsing for EVO's local `log-*.txt` files.
//!
//! The split `id` is an ordinal (0, 1, 2), not a car ID. Car identifiers in `New lap` lines
//! are deliberately parsed only as a delimiter and are never retained or exposed.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoggedLap {
    /// Official sector durations, in milliseconds.
    pub split_ms: Vec<u32>,
    /// Completed-lap duration, in milliseconds.
    pub lap_ms: u32,
    /// Stable, nonpersonal fingerprint used only to suppress duplicate log events.
    pub fingerprint: u64,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Split {
        ordinal: u8,
        ms: u32,
        final_flags: Option<bool>,
    },
    SplitsComplete {
        ordinal: u8,
    },
    NewLap {
        ms: u32,
        fingerprint: u64,
    },
    UnsupportedSplit,
    Other,
}

#[derive(Default)]
struct PendingSplits {
    parts: Vec<u32>,
    final_seen: bool,
    complete_marker: bool,
}

#[derive(Default)]
pub(super) struct SplitParser {
    pending: Option<PendingSplits>,
}

impl SplitParser {
    /// Add one complete UTF-8 line. Only an ordered, closed split group followed by a
    /// completed-lap line can produce a result.
    pub fn push_line(&mut self, line: &str) -> Option<LoggedLap> {
        match parse_event(line) {
            Event::Split {
                ordinal,
                ms,
                final_flags,
            } => {
                if ordinal == 0 && self.pending.is_none() {
                    self.pending = Some(PendingSplits::default());
                }
                let group = self.pending.as_mut()?;
                if usize::from(ordinal) != group.parts.len()
                    || ordinal >= 10
                    || group.final_seen
                    || final_flags.is_none()
                    || ms == 0
                {
                    self.pending = None;
                    return None;
                }
                group.parts.push(ms);
                group.final_seen = final_flags == Some(true);
            }
            Event::SplitsComplete { ordinal } => {
                let group = self.pending.as_mut()?;
                if !(1..=10).contains(&group.parts.len())
                    || usize::from(ordinal) + 1 != group.parts.len()
                    || !group.final_seen
                    || group.complete_marker
                {
                    self.pending = None;
                } else {
                    group.complete_marker = true;
                }
            }
            Event::NewLap { ms, fingerprint } => {
                let group = self.pending.take()?;
                if !group.complete_marker || ms == 0 {
                    return None;
                }
                let split_ms = group.parts;
                let sum = split_ms.iter().map(|&v| u64::from(v)).sum::<u64>();
                if sum.abs_diff(u64::from(ms)) > 500 {
                    return None;
                }
                return Some(LoggedLap {
                    fingerprint: with_splits(fingerprint, &split_ms),
                    split_ms,
                    lap_ms: ms,
                });
            }
            Event::UnsupportedSplit => self.pending = None,
            Event::Other => {}
        }
        None
    }

    /// Forget an incomplete group after a file rotation or truncation.
    pub fn reset(&mut self) {
        self.pending = None;
    }

    /// Detect a split-like line whose format or ordinal is outside the supported
    /// grammar, so the worker can report the fallback without exposing its contents.
    pub fn is_unsupported_split_line(line: &str) -> bool {
        matches!(parse_event(line), Event::UnsupportedSplit)
            || (line.contains("On Split") && matches!(parse_event(line), Event::Other))
    }
}

fn parse_event(line: &str) -> Event {
    if let Some(at) = line.find("On Split start ") {
        let fields: Vec<_> = line[at..].split_whitespace().take(10).collect();
        if fields.len() == 10 && fields[0] == "On" && fields[1] == "Split" && fields[2] == "start" {
            if fields[4] != "end" || fields[6] != "id" || fields[8] != "splittime" {
                return Event::UnsupportedSplit;
            }
            let start = fields[3];
            let end = fields[5];
            // Tokens: On Split start <bool> end <bool> id <n> splittime <ms>.
            let ordinal = fields[7].parse::<u8>().ok();
            let ms = fields[9].parse::<u32>().ok();
            if let (Some(ordinal), Some(ms)) = (ordinal, ms) {
                if ordinal >= 10 {
                    return Event::UnsupportedSplit;
                }
                return Event::Split {
                    ordinal,
                    ms,
                    final_flags: match (start, end) {
                        ("false", "false") => Some(false),
                        ("true", "true") => Some(true),
                        _ => None,
                    },
                };
            }
            return Event::Split {
                ordinal: u8::MAX,
                ms: 0,
                final_flags: None,
            };
        }
    }

    if let Some(at) = line.find("On Split end with all splits, id ") {
        let rest = &line[at + "On Split end with all splits, id ".len()..];
        if let Some(value) = rest.split(|c: char| !c.is_ascii_digit()).next()
            && let Ok(ordinal) = value.parse::<u8>()
        {
            return Event::SplitsComplete { ordinal };
        }
        return Event::SplitsComplete { ordinal: u8::MAX };
    }

    if let Some(at) = line.find("New lap carId ") {
        let rest = &line[at + "New lap carId ".len()..];
        // Do not retain or emit the car ID. It is only used to locate the time after the colon.
        if let Some((car, time)) = rest.split_once(':')
            && !car.trim().is_empty()
        {
            let token = time.split_whitespace().next().unwrap_or_default();
            if let Some(ms) = parse_lap_time(token) {
                return Event::NewLap {
                    ms,
                    fingerprint: fingerprint(line[..at].trim(), ms),
                };
            }
        }
    }

    Event::Other
}

/// Completion timing alone is usable for open-run correlation, never for sector learning.
pub(super) fn completion_time_ms(line: &str) -> Option<u32> {
    match parse_event(line) {
        Event::NewLap { ms, .. } if ms > 0 => Some(ms),
        _ => None,
    }
}

fn parse_lap_time(value: &str) -> Option<u32> {
    let (minutes, seconds) = value.split_once(':')?;
    let (seconds, millis) = seconds.split_once('.')?;
    if millis.len() != 3 || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes = minutes.parse::<u32>().ok()?;
    let seconds = seconds.parse::<u32>().ok()?;
    let millis = millis.parse::<u32>().ok()?;
    if seconds >= 60 {
        return None;
    }
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1_000)?
        .checked_add(millis)
}

fn fingerprint(prefix: &str, lap_ms: u32) -> u64 {
    // Hash only the event clock and nonpersonal timing values, never the rest of the log line.
    let event_clock = clock_ms(prefix).unwrap_or(0);
    let mut hash = 0xcbf29ce484222325u64;
    for byte in event_clock
        .to_le_bytes()
        .into_iter()
        .chain(lap_ms.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn with_splits(fingerprint: u64, splits: &[u32]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in fingerprint
        .to_le_bytes()
        .into_iter()
        .chain((splits.len() as u32).to_le_bytes())
        .chain(splits.iter().flat_map(|value| value.to_le_bytes()))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn clock_ms(prefix: &str) -> Option<u32> {
    for field in prefix.split_whitespace() {
        let token = field.trim_matches(|c: char| !c.is_ascii_digit() && c != ':' && c != '.');
        let mut parts = token.split([':', '.']);
        let (Some(h), Some(m), Some(s), Some(ms), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        if ms.len() != 3 {
            continue;
        }
        let (Ok(h), Ok(m), Ok(s), Ok(ms)) = (
            h.parse::<u32>(),
            m.parse::<u32>(),
            s.parse::<u32>(),
            ms.parse::<u32>(),
        ) else {
            continue;
        };
        if h < 24 && m < 60 && s < 60 {
            return Some(((h * 60 + m) * 60 + s) * 1_000 + ms);
        }
    }
    None
}

/// Finds the newest EVO log and incrementally consumes only appended bytes.
pub(super) struct LogTailer {
    dir: PathBuf,
    current_path: Option<PathBuf>,
    file: Option<File>,
    offset: u64,
    partial: Vec<u8>,
    discarding_line: bool,
    generation: u64,
}

impl LogTailer {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            current_path: None,
            file: None,
            offset: 0,
            partial: Vec::with_capacity(1024),
            discarding_line: false,
            generation: 0,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Ignore existing history and begin at the current end of the newest log.
    pub fn seek_to_latest_end(&mut self) {
        self.reset_file();
        let _ = self.select_latest();
    }

    pub fn read_appended(&mut self) -> io::Result<Vec<String>> {
        self.select_latest()?;
        let Some(file) = self.file.as_mut() else {
            return Ok(Vec::new());
        };
        let path = self.current_path.as_ref().expect("file always has a path");
        let path_metadata = match fs::metadata(path) {
            Ok(value) => value,
            Err(error) => {
                self.reset_file();
                return Err(error);
            }
        };
        let handle_metadata = file.metadata()?;
        let same_file = match (path_metadata.created(), handle_metadata.created()) {
            (Ok(path_created), Ok(handle_created)) => path_created == handle_created,
            _ => true,
        };
        if path_metadata.len() < self.offset || handle_metadata.len() < self.offset || !same_file {
            self.reset_file();
            self.select_latest()?;
            return Ok(Vec::new());
        }
        let available = path_metadata
            .len()
            .max(handle_metadata.len())
            .saturating_sub(self.offset);
        if available == 0 {
            return Ok(Vec::new());
        }
        let take = available.min(MAX_READ_BYTES as u64) as usize;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; take];
        let read = file.read(&mut bytes)?;
        bytes.truncate(read);
        self.offset = self.offset.saturating_add(read as u64);
        Ok(self.consume(&bytes))
    }

    fn select_latest(&mut self) -> io::Result<()> {
        let latest = newest_log(&self.dir)?;
        if latest == self.current_path && self.file.is_some() {
            return Ok(());
        }
        self.reset_file();
        let Some(path) = latest else {
            return Ok(());
        };
        let mut file = File::open(&path)?;
        // Newly discovered or rotated logs may contain an entire earlier session. Begin at EOF
        // so only later appends can become evidence for the active session.
        let offset = file.seek(SeekFrom::End(0))?;
        self.current_path = Some(path);
        self.offset = offset;
        self.file = Some(file);
        Ok(())
    }

    fn consume(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if !self.discarding_line {
                    if self.partial.last() == Some(&b'\r') {
                        self.partial.pop();
                    }
                    if let Ok(line) = std::str::from_utf8(&self.partial) {
                        lines.push(line.to_owned());
                    }
                }
                self.partial.clear();
                self.discarding_line = false;
            } else if !self.discarding_line {
                if self.partial.len() == MAX_LINE_BYTES {
                    self.partial.clear();
                    self.discarding_line = true;
                } else {
                    self.partial.push(byte);
                }
            }
        }
        lines
    }

    fn reset_file(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.file = None;
        self.current_path = None;
        self.offset = 0;
        self.partial.clear();
        self.discarding_line = false;
    }
}

fn newest_log(dir: &Path) -> io::Result<Option<PathBuf>> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| name.starts_with("log-") && name.ends_with(".txt"))
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, path));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    const COMPLETE: &str = include_str!("testdata/complete-solo.log");
    const MALFORMED: &str = include_str!("testdata/malformed-partial.log");
    const INTERLEAVED: &str = include_str!("testdata/interleaved.log");

    #[test]
    fn parses_the_observed_three_duration_log_grammar() {
        let mut parser = SplitParser::default();
        let result = COMPLETE.lines().find_map(|line| parser.push_line(line));
        assert_eq!(
            result,
            Some(LoggedLap {
                split_ms: vec![24_018, 42_198, 29_898],
                lap_ms: 96_114,
                fingerprint: result.as_ref().unwrap().fingerprint,
            })
        );
    }

    #[test]
    fn rejects_partial_reordered_and_interleaved_groups() {
        for fixture in [MALFORMED, INTERLEAVED] {
            let mut parser = SplitParser::default();
            assert!(fixture.lines().all(|line| parser.push_line(line).is_none()));
        }
    }

    #[test]
    fn rejects_split_total_that_does_not_match_completed_lap() {
        let mut parser = SplitParser::default();
        let fixture = COMPLETE.replace("01:36.114", "01:42.114");
        assert!(fixture.lines().all(|line| parser.push_line(line).is_none()));
    }

    #[test]
    fn parses_only_complete_ordered_two_to_ten_sector_groups() {
        for count in [1, 2, 3, 4, 10] {
            let mut parser = SplitParser::default();
            for index in 0..count {
                let last = index + 1 == count;
                let line = format!("On Split start {last} end {last} id {index} splittime 5000");
                assert!(parser.push_line(&line).is_none());
            }
            assert!(
                parser
                    .push_line(&format!("On Split end with all splits, id {}", count - 1))
                    .is_none()
            );
            let seconds = count * 5;
            let line = format!(
                "New lap carId 1: {:02}:{:02}.000",
                seconds / 60,
                seconds % 60
            );
            let lap = parser.push_line(&line).expect("valid group");
            assert_eq!(lap.split_ms, vec![5000; count]);
        }
    }

    #[test]
    fn detects_unvalidated_split_ordinals_and_grammar() {
        assert!(SplitParser::is_unsupported_split_line(
            "On Split start false end false id 10 splittime 1000"
        ));
        assert!(SplitParser::is_unsupported_split_line(
            "On Split start false id 0 splittime 1000"
        ));
        assert!(!SplitParser::is_unsupported_split_line(
            "On Split start false end false id 0 splittime 1000"
        ));
    }

    #[test]
    fn fingerprint_includes_split_durations_without_retaining_car_identity() {
        let parse = |fixture: &str| {
            let mut parser = SplitParser::default();
            fixture
                .lines()
                .find_map(|line| parser.push_line(line))
                .unwrap()
        };
        let original = parse(COMPLETE);
        let changed = parse(
            &COMPLETE
                .replace("splittime 24018", "splittime 24019")
                .replace("splittime 29898", "splittime 29897"),
        );
        assert_ne!(original.fingerprint, changed.fingerprint);
    }

    #[test]
    fn tailer_handles_partial_crlf_utf8_and_oversize_lines() {
        let dir = temporary_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log-test.txt");
        fs::write(&path, b"old history\r\n").unwrap();
        let mut tailer = LogTailer::new(dir.clone());
        tailer.seek_to_latest_end();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"one\r").unwrap();
        assert!(tailer.read_appended().unwrap().is_empty());
        file.write_all(b"\ntwo\n\xff\n").unwrap();
        let mut big = vec![b'x'; MAX_LINE_BYTES + 5];
        big.push(b'\n');
        file.write_all(&big).unwrap();
        file.write_all(b"last\n").unwrap();
        let lines = tailer.read_appended().unwrap();
        assert_eq!(lines, ["one", "two", "last"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rotation_skips_old_file_contents_and_truncation_discards_partial_state() {
        let dir = temporary_dir();
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("log-first.txt");
        fs::write(&first, b"old\n").unwrap();
        let mut tailer = LogTailer::new(dir.clone());
        tailer.seek_to_latest_end();
        let initial_generation = tailer.generation();

        let rotated = dir.join("log-rotated.txt");
        let mut new_log = fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&rotated)
            .unwrap();
        new_log.write_all(b"old rotated history\n").unwrap();
        new_log
            .set_times(
                fs::FileTimes::new()
                    .set_modified(SystemTime::now() + std::time::Duration::from_secs(2)),
            )
            .unwrap();
        assert!(tailer.read_appended().unwrap().is_empty());
        assert!(tailer.generation() > initial_generation);
        new_log.write_all(b"new line\n").unwrap();
        assert_eq!(tailer.read_appended().unwrap(), ["new line"]);
        drop(new_log);

        fs::write(&rotated, b"x\n").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&rotated)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(SystemTime::now() + std::time::Duration::from_secs(3)),
            )
            .unwrap();
        let before_truncation = tailer.generation();
        assert!(tailer.read_appended().unwrap().is_empty());
        assert!(tailer.generation() > before_truncation);
        fs::remove_dir_all(dir).unwrap();
    }

    fn temporary_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("opensimhud-evo-log-test-{nonce}"))
    }
}
