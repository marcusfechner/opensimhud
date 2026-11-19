//! Bounded measured-distance outline collection for EVO, without log or filesystem access.

use crate::model::Track;

const BINS: usize = 1000;
const MIN_POINTS: usize = 100;
const MAX_POINTS: usize = 24_000;
type NativePath = (Vec<f32>, Vec<f32>);

pub(crate) struct ObservedLearner {
    clean: bool,
    valid: bool,
    path: Vec<(f32, f32)>,
    last: Option<(f32, f32)>,
    pub outline: Option<(Vec<f32>, Vec<f32>)>,
    pub outline_valid: bool,
    candidate: Option<(Vec<f32>, Vec<f32>)>,
}

impl ObservedLearner {
    pub fn new() -> Self {
        Self {
            clean: true,
            valid: true,
            path: Vec::with_capacity(MAX_POINTS),
            last: None,
            outline: None,
            outline_valid: false,
            candidate: None,
        }
    }

    pub fn adopt(&mut self, track: &Track, valid: bool) {
        if !track.x.is_empty() {
            self.outline = Some((track.x.clone(), track.y.clone()));
            self.outline_valid = valid;
        }
    }

    pub fn reject_current_lap(&mut self) {
        self.clean = false;
    }

    pub fn reset_capture(&mut self) {
        self.path.clear();
        self.last = None;
        self.clean = true;
        self.valid = true;
        self.candidate = None;
    }

    pub fn sample_observed_path(
        &mut self,
        position: Option<(f32, f32)>,
        on_track: bool,
        valid: bool,
    ) {
        self.clean &= on_track;
        self.valid &= valid;
        let Some((x, y)) = position.filter(|(x, y)| {
            x.is_finite() && y.is_finite() && x.abs() <= 100_000.0 && y.abs() <= 100_000.0
        }) else {
            self.clean = false;
            self.last = None;
            return;
        };
        if self
            .last
            .is_some_and(|(px, py)| (x - px).hypot(y - py) > 100.0)
        {
            self.clean = false;
        }
        if self.path.len() < MAX_POINTS {
            self.path.push((x, y));
        } else {
            self.clean = false;
        }
        self.last = Some((x, y));
    }

    pub fn lap_done_normalized(&mut self, length: f64) -> bool {
        self.finish(length, true)
    }
    pub fn lap_done_open(&mut self, length: f64) -> bool {
        self.finish(length, false)
    }
    pub fn take_candidate(&mut self) -> Option<(Vec<f32>, Vec<f32>)> {
        self.candidate.take()
    }

    /// Hold an open run without displaying it until the worker confirms a timing event.
    pub fn finish_pending_open(&mut self, length: f64) -> Option<(NativePath, bool)> {
        let path = if self.clean {
            resample(&self.path, length, false)
        } else {
            None
        };
        let valid = self.valid;
        self.path.clear();
        self.last = None;
        self.clean = true;
        self.valid = true;
        self.candidate = None;
        path.map(|path| (path, valid))
    }

    fn finish(&mut self, length: f64, closed: bool) -> bool {
        self.candidate = None;
        let path = self
            .clean
            .then(|| resample(&self.path, length, closed))
            .flatten();
        let mut built = false;
        if let Some(path) = path {
            if self.valid {
                self.candidate = Some(path.clone());
            }
            if self.outline.is_none() || (self.valid && !self.outline_valid) {
                self.outline = Some(path);
                self.outline_valid = self.valid;
                built = true;
            }
        }
        self.path.clear();
        self.last = None;
        self.clean = true;
        self.valid = true;
        built
    }
}

fn resample(
    path: &[(f32, f32)],
    reported_length: f64,
    closed: bool,
) -> Option<(Vec<f32>, Vec<f32>)> {
    if path.len() < MIN_POINTS
        || !reported_length.is_finite()
        || !(100.0..=100_000.0).contains(&reported_length)
    {
        return None;
    }
    let closure = (path[0].0 - path.last()?.0).hypot(path[0].1 - path.last()?.1);
    if closed && (!closure.is_finite() || closure > 60.0) {
        return None;
    }
    let mut distances = Vec::with_capacity(path.len() + 1);
    distances.push(0.0f64);
    for pair in path.windows(2) {
        let distance = f64::from((pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1));
        if !distance.is_finite() || distance > 100.0 {
            return None;
        }
        distances.push(distances.last().copied()? + distance);
    }
    let total = distances.last().copied()? + if closed { f64::from(closure) } else { 0.0 };
    if !(reported_length * 0.7..=reported_length * 1.3).contains(&total) {
        return None;
    }
    if closed {
        distances.push(total);
    }
    let mut output = Vec::with_capacity(BINS);
    let mut segment = 0;
    for index in 0..BINS {
        let divisor = if closed { BINS } else { BINS - 1 };
        let target = total * index as f64 / divisor as f64;
        while segment + 1 < path.len() - usize::from(!closed) && distances[segment + 1] < target {
            segment += 1;
        }
        let a = path[segment];
        let b = if segment + 1 < path.len() {
            path[segment + 1]
        } else {
            path[0]
        };
        let width = distances[segment + 1] - distances[segment];
        let t = if width > 0.0 {
            ((target - distances[segment]) / width) as f32
        } else {
            0.0
        };
        output.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
    }
    if closed {
        let smoothed = (0..BINS)
            .map(|i| {
                let mut x = 0.0;
                let mut y = 0.0;
                for k in 0..=6 {
                    let p = output[(i + BINS + k - 3) % BINS];
                    x += p.0;
                    y += p.1;
                }
                (x / 7.0, y / 7.0)
            })
            .collect::<Vec<_>>();
        Some(smoothed.into_iter().unzip())
    } else {
        Some(output.into_iter().unzip())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_distance_keeps_closed_seam_and_open_endpoints() {
        let circle: Vec<_> = (0..3000)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU * 0.982 / 3000.0;
                (500.0 * angle.cos(), 500.0 * angle.sin())
            })
            .collect();
        let (x, y) = resample(&circle, 3300.0, true).unwrap();
        assert!((x[500] + 500.0).hypot(y[500]) < 6.0);
        let line: Vec<_> = (0..=1000).map(|i| (i as f32, 0.0)).collect();
        let (x, _) = resample(&line, 1000.0, false).unwrap();
        assert_eq!(x[0], 0.0);
        assert!((x[BINS - 1] - 1000.0).abs() < 0.01);
    }

    #[test]
    fn brands_indy_driven_length_does_not_compress_the_finish_seam() {
        let radius = 1_910.0 / std::f64::consts::TAU;
        let path: Vec<_> = (0..3000)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 3000.0;
                ((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)
            })
            .collect();
        let (x, y) = resample(&path, 1944.0, true).unwrap();
        let distance = |i: usize| {
            let next = (i + 1) % BINS;
            (x[next] - x[i]).hypot(y[next] - y[i])
        };
        assert!((distance(BINS - 1) - distance(500)).abs() < 0.1);
        assert!((x[500] + radius as f32).abs() < 1.0);
    }

    #[test]
    fn thirty_minute_closed_lap_fits_the_sample_cap() {
        let mut learner = ObservedLearner::new();
        let radius = 4_000.0f64;
        for index in 0..18_000 {
            let angle = index as f64 * std::f64::consts::TAU / 18_000.0;
            learner.sample_observed_path(
                Some(((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)),
                true,
                true,
            );
        }
        assert!(learner.lap_done_normalized(radius * std::f64::consts::TAU));
        assert_eq!(learner.outline.as_ref().unwrap().0.len(), BINS);
    }

    #[test]
    fn exceeding_sample_cap_rejects_the_entire_run() {
        let mut learner = ObservedLearner::new();
        for index in 0..=MAX_POINTS {
            let angle = index as f64 * std::f64::consts::TAU / MAX_POINTS as f64;
            learner.sample_observed_path(
                Some(((500.0 * angle.cos()) as f32, (500.0 * angle.sin()) as f32)),
                true,
                true,
            );
        }
        assert!(!learner.lap_done_normalized(3141.6));
        assert!(learner.outline.is_none());
    }

    #[test]
    fn invalid_bootstrap_is_display_only_and_valid_lap_replaces_it_once() {
        let mut learner = ObservedLearner::new();
        let feed = |learner: &mut ObservedLearner, radius: f64, valid: bool| {
            for index in 0..3000 {
                let angle = index as f64 * std::f64::consts::TAU / 3000.0;
                learner.sample_observed_path(
                    Some(((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)),
                    true,
                    valid,
                );
            }
            learner.lap_done_normalized(3141.6)
        };
        assert!(feed(&mut learner, 500.0, false));
        assert!(!learner.outline_valid && learner.take_candidate().is_none());
        assert!(feed(&mut learner, 510.0, true));
        assert!(learner.outline_valid && learner.take_candidate().is_some());
        let selected = learner.outline.clone();
        assert!(!feed(&mut learner, 520.0, true));
        assert_eq!(learner.outline, selected);
        assert!(learner.take_candidate().is_some());
    }
}
