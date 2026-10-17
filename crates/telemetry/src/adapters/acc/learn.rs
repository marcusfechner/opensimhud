//! ACC publishes no track map: learn the outline from the player's position over one clean
//! lap, and the sector boundaries from where the sector index changes. Learned tracks can be
//! cached on disk so the map is there from the first lap next time.

use std::path::{Path, PathBuf};

use crate::model::Track;

/// Samples along the lap.
const BINS: usize = 1000;
/// Share of bins a lap must cover to become the outline.
const MIN_COVERAGE: f64 = 0.97;

pub struct Learner {
    bins: Vec<Option<(f32, f32)>>,
    clean: bool,
    last_bin: Option<usize>,
    last_point: (f32, f32),
    /// Lap fraction where each sector starts, learned from sector index changes.
    pub sectors: Vec<f64>,
    last_sector: Option<usize>,
    pub outline: Option<(Vec<f32>, Vec<f32>)>,
}

impl Learner {
    pub fn new(sector_count: usize) -> Self {
        let n = sector_count.max(1);
        Self {
            bins: vec![None; BINS],
            clean: true,
            last_bin: None,
            last_point: (0.0, 0.0),
            sectors: (0..n).map(|k| k as f64 / n as f64).collect(),
            last_sector: None,
            outline: None,
        }
    }

    /// Feed the player's position. `clean` = on track, lap valid.
    pub fn sample(&mut self, pct: f64, x: f32, y: f32, sector: usize, clean: bool) {
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
        if self.outline.is_some() {
            return;
        }
        self.clean &= clean;
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
        let built =
            self.outline.is_none() && self.clean && filled as f64 >= BINS as f64 * MIN_COVERAGE;
        if built {
            self.outline = Some(build(&self.bins));
        }
        self.bins.iter_mut().for_each(|b| *b = None);
        self.clean = true;
        self.last_bin = None;
        built
    }

    pub fn load(dir: &Path, name: &str) -> Option<Track> {
        let text = std::fs::read_to_string(cache_path(dir, name)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(dir: &Path, track: &Track) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string(track).map_err(std::io::Error::other)?;
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
            l.sample(pct, x, y, sector, true);
        }
        assert!(l.lap_done());
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

    #[test]
    fn ignores_invalid_or_partial_laps() {
        let mut l = Learner::new(3);
        for k in 0..3000 {
            let pct = k as f64 / 3000.0;
            let (x, y) = circle(pct);
            l.sample(pct, x, y, 0, !(1000..=1200).contains(&k)); // off track for a while
        }
        assert!(!l.lap_done());
        for k in 1500..3000 {
            let pct = k as f64 / 3000.0;
            let (x, y) = circle(pct);
            l.sample(pct, x, y, 0, true); // joined mid-lap
        }
        assert!(!l.lap_done());
        assert!(l.outline.is_none());
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
        Learner::save(&dir, &t).unwrap();
        assert_eq!(Learner::load(&dir, "monza"), Some(t));
        std::fs::remove_dir_all(dir).ok();
    }
}
