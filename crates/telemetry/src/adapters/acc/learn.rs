//! ACC publishes no track map: learn the outline from the player's position over one full
//! lap, and the sector boundaries from where the sector index changes. Learned tracks can be
//! cached on disk so the map is there from the first lap next time.
//!
//! Any complete lap outside the pit lane gives an outline, even one ACC invalidated for track
//! limits (a cut corner barely shows at map scale, and some drivers never do a valid lap). A
//! later valid lap replaces such an outline once.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Track;

/// Samples along the lap.
const BINS: usize = 1000;
/// Share of bins a lap must cover to become the outline.
const MIN_COVERAGE: f64 = 0.97;

pub struct Learner {
    bins: Vec<Option<(f32, f32)>>,
    /// This lap so far: continuous forward progress, never in the pit lane.
    clean: bool,
    /// This lap so far: valid by the game's rules.
    valid: bool,
    last_bin: Option<usize>,
    last_point: (f32, f32),
    /// Lap fraction where each sector starts, learned from sector index changes.
    pub sectors: Vec<f64>,
    last_sector: Option<usize>,
    pub outline: Option<(Vec<f32>, Vec<f32>)>,
    /// The outline came from a valid lap; nothing left to learn.
    pub outline_valid: bool,
}

/// A learned track as cached on disk.
#[derive(Serialize, Deserialize)]
struct Cached {
    #[serde(flatten)]
    track: Track,
    /// Older caches lack this; they were only written for valid laps.
    #[serde(default = "yes")]
    from_valid_lap: bool,
}

fn yes() -> bool {
    true
}

impl Learner {
    pub fn new(sector_count: usize) -> Self {
        let n = sector_count.max(1);
        Self {
            bins: vec![None; BINS],
            clean: true,
            valid: true,
            last_bin: None,
            last_point: (0.0, 0.0),
            sectors: (0..n).map(|k| k as f64 / n as f64).collect(),
            last_sector: None,
            outline: None,
            outline_valid: false,
        }
    }

    /// Start from a cached outline; a valid one ends learning.
    pub fn adopt(&mut self, track: &Track, from_valid_lap: bool) {
        self.sectors.clone_from(&track.sectors);
        if !track.x.is_empty() {
            self.outline = Some((track.x.clone(), track.y.clone()));
            self.outline_valid = from_valid_lap;
        }
    }

    /// Feed the player's position. `on_track` = out of the pit lane; `valid` = the game still
    /// counts this lap.
    pub fn sample(&mut self, pct: f64, x: f32, y: f32, sector: usize, on_track: bool, valid: bool) {
        if !(0.0..1.0).contains(&pct) {
            return;
        }
        if let Some(prev) = self.last_sector
            && sector == prev + 1
            && sector < self.sectors.len()
        {
            self.sectors[sector] = pct;
        }
        self.last_sector = Some(sector);
        if self.outline_valid {
            return;
        }
        self.clean &= on_track;
        self.valid &= valid;
        let b = ((pct * BINS as f64) as usize).min(BINS - 1);
        // only accept forward progress within the lap (no reversing, no teleports); bins
        // skipped between two samples are interpolated
        if self.last_bin.is_none_or(|l| b >= l && b - l < 50) {
            if let Some(l) = self.last_bin {
                let (px, py) = self.last_point;
                for k in l + 1..b {
                    let t = (k - l) as f32 / (b - l) as f32;
                    self.bins[k] = Some((px + (x - px) * t, py + (y - py) * t));
                }
            }
            self.bins[b] = Some((x, y));
            self.last_bin = Some(b);
            self.last_point = (x, y);
        } else if self.last_bin.is_some_and(|l| b < l) && l_near_end(self.last_bin) {
            self.last_bin = Some(b);
        } else {
            self.clean = false;
        }
    }

    /// The player crossed the line. Returns true when this lap produced the outline.
    pub fn lap_done(&mut self) -> bool {
        let filled = self.bins.iter().filter(|b| b.is_some()).count();
        let complete = self.clean && filled as f64 >= BINS as f64 * MIN_COVERAGE;
        let built = complete && (self.outline.is_none() || (self.valid && !self.outline_valid));
        if built {
            self.outline = Some(build(&self.bins));
            self.outline_valid = self.valid;
        }
        self.bins.iter_mut().for_each(|b| *b = None);
        self.clean = true;
        self.valid = true;
        self.last_bin = None;
        built
    }

    /// A cached track, and whether its outline came from a valid lap.
    pub fn load(dir: &Path, name: &str) -> Option<(Track, bool)> {
        let text = std::fs::read_to_string(cache_path(dir, name)).ok()?;
        let c: Cached = serde_json::from_str(&text).ok()?;
        Some((c.track, c.from_valid_lap))
    }

    pub fn save(dir: &Path, track: &Track, from_valid_lap: bool) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let c = Cached {
            track: track.clone(),
            from_valid_lap,
        };
        let json = serde_json::to_string(&c).map_err(std::io::Error::other)?;
        std::fs::write(cache_path(dir, &track.name), json)
    }
}

fn l_near_end(last: Option<usize>) -> bool {
    last.is_some_and(|l| l > BINS - 50)
}

fn cache_path(dir: &Path, name: &str) -> PathBuf {
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
    dir.join(format!("{safe}.json"))
}

/// Fill gaps by interpolating between known samples (wrapping), then smooth lightly.
fn build(bins: &[Option<(f32, f32)>]) -> (Vec<f32>, Vec<f32>) {
    let n = bins.len();
    let known: Vec<usize> = (0..n).filter(|&i| bins[i].is_some()).collect();
    let mut pts = vec![(0f32, 0f32); n];
    for (k, &i) in known.iter().enumerate() {
        let j = known[(k + 1) % known.len()];
        let (a, b) = (bins[i].unwrap(), bins[j].unwrap());
        let gap = (j + n - i) % n;
        let gap = if gap == 0 { n } else { gap };
        for s in 0..gap {
            let t = s as f32 / gap as f32;
            pts[(i + s) % n] = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        }
    }
    const W: usize = 3;
    let avg = |f: &dyn Fn((f32, f32)) -> f32, i: usize| {
        (0..=2 * W)
            .map(|k| f(pts[(i + n + k - W) % n]))
            .sum::<f32>()
            / (2 * W + 1) as f32
    };
    let x = (0..n).map(|i| avg(&|p| p.0, i)).collect();
    let y = (0..n).map(|i| avg(&|p| p.1, i)).collect();
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(pct: f64) -> (f32, f32) {
        let a = pct * std::f64::consts::TAU;
        ((a.cos() * 500.0) as f32, (a.sin() * 500.0) as f32)
    }

    #[test]
    fn learns_outline_and_sectors_from_a_clean_lap() {
        let mut l = Learner::new(3);
        for k in 0..3000 {
            let pct = k as f64 / 3000.0;
            let (x, y) = circle(pct);
            let sector = if pct < 0.3 {
                0
            } else if pct < 0.7 {
                1
            } else {
                2
            };
            l.sample(pct, x, y, sector, true, true);
        }
        assert!(l.lap_done());
        assert!(l.outline_valid);
        let (x, y) = l.outline.clone().unwrap();
        assert_eq!(x.len(), BINS);
        let r = (x[250].hypot(y[250]) - 500.0).abs();
        assert!(r < 2.0, "outline off the circle by {r} m");
        assert!(
            (l.sectors[1] - 0.3).abs() < 0.002 && (l.sectors[2] - 0.7).abs() < 0.002,
            "{:?}",
            l.sectors
        );
    }

    fn lap(l: &mut Learner, ks: std::ops::Range<usize>, on_track: bool, valid: bool) -> bool {
        for k in ks {
            let pct = k as f64 / 3000.0;
            let (x, y) = circle(pct);
            l.sample(pct, x, y, 0, on_track, valid);
        }
        l.lap_done()
    }

    #[test]
    fn ignores_pit_and_partial_laps() {
        let mut l = Learner::new(3);
        for k in 0..3000 {
            let pct = k as f64 / 3000.0;
            let (x, y) = circle(pct);
            l.sample(pct, x, y, 0, !(1000..=1200).contains(&k), true); // through the pit lane
        }
        assert!(!l.lap_done());
        assert!(!lap(&mut l, 1500..3000, true, true)); // joined mid-lap
        assert!(l.outline.is_none());
    }

    #[test]
    fn learns_from_an_invalid_lap_until_a_valid_one_comes() {
        let mut l = Learner::new(3);
        assert!(lap(&mut l, 0..3000, true, false));
        assert!(l.outline.is_some() && !l.outline_valid);
        assert!(!lap(&mut l, 0..3000, true, false)); // another cut lap changes nothing
        assert!(lap(&mut l, 0..3000, true, true));
        assert!(l.outline_valid);
        assert!(!lap(&mut l, 0..3000, true, true));
    }

    #[test]
    fn caches_tracks_on_disk() {
        let dir = std::env::temp_dir().join(format!("ovtrack-{}", std::process::id()));
        let t = Track {
            name: "monza".into(),
            length: 5793.0,
            sectors: vec![0.0, 0.33, 0.66],
            x: vec![1.0, 2.0],
            y: vec![3.0, 4.0],
        };
        Learner::save(&dir, &t, false).unwrap();
        assert_eq!(Learner::load(&dir, "monza"), Some((t.clone(), false)));
        // caches written before the flag existed came from valid laps
        let old = serde_json::to_string(&t).unwrap();
        std::fs::write(cache_path(&dir, "monza"), old).unwrap();
        assert_eq!(Learner::load(&dir, "monza"), Some((t, true)));
        std::fs::remove_dir_all(dir).ok();
    }
}
