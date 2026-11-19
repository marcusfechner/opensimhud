//! Game-independent, bounded selection of observed native outlines.
//!
//! Comparisons use equal-distance copies; selected points retain their original indexing.

use serde::{Deserialize, Serialize};

pub const POINT_COUNT: usize = 1000;
const COMPARISON_COUNT: usize = 128;
const MAX_SESSIONS: usize = 8;
const PER_SESSION: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteTopology {
    Closed,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SamplingDomain {
    GameProgress,
    MeasuredDistance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutlineCandidate {
    pub track_key: String,
    pub topology: RouteTopology,
    pub domain: SamplingDomain,
    pub valid: bool,
    pub session_id: u64,
    pub points: Vec<(f32, f32)>,
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f64 {
    f64::from((a.0 - b.0).hypot(a.1 - b.1))
}

impl OutlineCandidate {
    pub fn measured_length(&self) -> Option<f64> {
        if self.points.len() != POINT_COUNT {
            return None;
        }
        let count = if self.topology == RouteTopology::Closed {
            POINT_COUNT
        } else {
            POINT_COUNT - 1
        };
        let mut total = 0.0;
        for index in 0..count {
            let segment = distance(self.points[index], self.points[(index + 1) % POINT_COUNT]);
            if !segment.is_finite() || segment > 100.0 {
                return None;
            }
            total += segment;
        }
        Some(total)
    }
    /// Produce a comparison copy without changing the native path.
    fn comparison(&self) -> Option<Vec<(f32, f32)>> {
        if self.points.len() != POINT_COUNT
            || self.track_key.is_empty()
            || self.points.iter().any(|p| {
                !p.0.is_finite()
                    || !p.1.is_finite()
                    || p.0.abs() > 100_000.0
                    || p.1.abs() > 100_000.0
            })
        {
            return None;
        }
        let closed = self.topology == RouteTopology::Closed;
        if closed && distance(self.points[0], self.points[POINT_COUNT - 1]) > 60.0 {
            return None;
        }
        let count = if closed { POINT_COUNT } else { POINT_COUNT - 1 };
        let mut cumulative = Vec::with_capacity(count + 1);
        cumulative.push(0.0);
        for index in 0..count {
            let length = distance(self.points[index], self.points[(index + 1) % POINT_COUNT]);
            if !length.is_finite() || length > 100.0 {
                return None;
            }
            cumulative.push(cumulative[index] + length);
        }
        let total = *cumulative.last()?;
        if total <= 0.0 {
            return None;
        }
        let divisor = if closed {
            COMPARISON_COUNT
        } else {
            COMPARISON_COUNT - 1
        };
        let mut segment = 0;
        let mut result = Vec::with_capacity(COMPARISON_COUNT);
        for index in 0..COMPARISON_COUNT {
            let target = total * index as f64 / divisor as f64;
            while segment + 1 < count && cumulative[segment + 1] < target {
                segment += 1;
            }
            let a = self.points[segment];
            let b = self.points[(segment + 1) % POINT_COUNT];
            let length = cumulative[segment + 1] - cumulative[segment];
            let t = if length > 0.0 {
                ((target - cumulative[segment]) / length) as f32
            } else {
                0.0
            };
            result.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
        Some(result)
    }
}

fn tangent(points: &[(f32, f32)], index: usize, closed: bool) -> (f32, f32) {
    let next = if closed {
        (index + 1) % points.len()
    } else {
        (index + 1).min(points.len() - 1)
    };
    let previous = if next == index {
        index.saturating_sub(1)
    } else {
        index
    };
    let dx = points[next].0 - points[previous].0;
    let dy = points[next].1 - points[previous].1;
    let length = dx.hypot(dy);
    if length > 0.0 {
        (dx / length, dy / length)
    } else {
        (0.0, 0.0)
    }
}

fn dot(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

/// Compare ordered samples after aligning the timing line, never by nearest-point projection.
fn disagreement(a: &OutlineCandidate, b: &OutlineCandidate) -> Option<(f64, f64)> {
    if a.track_key != b.track_key || a.topology != b.topology || a.domain != b.domain {
        return None;
    }
    let x = a.comparison()?;
    let y = b.comparison()?;
    let closed = a.topology == RouteTopology::Closed;
    let first_direction = tangent(&x, 0, closed);
    let shift = if closed {
        (0..y.len())
            .filter(|&i| {
                distance(x[0], y[i]) <= 60.0 && dot(first_direction, tangent(&y, i, true)) >= 0.5
            })
            .min_by(|&i, &j| distance(x[0], y[i]).total_cmp(&distance(x[0], y[j])))?
    } else {
        if distance(x[0], y[0]) > 60.0
            || distance(*x.last()?, *y.last()?) > 60.0
            || dot(first_direction, tangent(&y, 0, false)) < 0.5
            || dot(
                tangent(&x, x.len() - 1, false),
                tangent(&y, y.len() - 1, false),
            ) < 0.5
        {
            return None;
        }
        0
    };
    let mut values: Vec<_> = x
        .iter()
        .enumerate()
        .map(|(i, &point)| distance(point, y[(i + shift) % y.len()]))
        .collect();
    values.sort_unstable_by(f64::total_cmp);
    Some((values[values.len() / 2], values[values.len() * 9 / 10]))
}

fn agrees(a: &OutlineCandidate, b: &OutlineCandidate) -> bool {
    disagreement(a, b).is_some_and(|(median, p90)| median <= 10.0 && p90 <= 20.0)
}

fn group_score(group: &[OutlineCandidate]) -> f64 {
    let mut score = 0.0;
    for i in 0..group.len() {
        for j in i + 1..group.len() {
            let Some((median, p90)) = disagreement(&group[i], &group[j]) else {
                return f64::INFINITY;
            };
            score += median + 0.5 * p90;
        }
    }
    score
}

fn roughness(candidate: &OutlineCandidate) -> f64 {
    let Some(points) = candidate.comparison() else {
        return f64::INFINITY;
    };
    let closed = candidate.topology == RouteTopology::Closed;
    let count = if closed {
        points.len()
    } else {
        points.len().saturating_sub(4)
    };
    let mut ratio = 0.0;
    let mut windows = 0;
    for index in 0..count {
        let mut arc = 0.0;
        for step in 0..4 {
            arc += distance(
                points[(index + step) % points.len()],
                points[(index + step + 1) % points.len()],
            );
        }
        let chord = distance(points[index], points[(index + 4) % points.len()]);
        if chord > 1.0 {
            ratio += arc / chord;
            windows += 1;
        }
    }
    if windows == 0 {
        f64::INFINITY
    } else {
        ratio / windows as f64
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionEvidence {
    id: u64,
    representatives: Vec<OutlineCandidate>,
    challengers: Vec<OutlineCandidate>,
}

/// Established representatives carry one total vote per contributing session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateHistory {
    sessions: Vec<SessionEvidence>,
}

impl CandidateHistory {
    fn weighted_score(&self, candidate: &OutlineCandidate) -> f64 {
        self.sessions
            .iter()
            .map(|session| {
                let total: f64 = session
                    .representatives
                    .iter()
                    .map(|observation| {
                        disagreement(candidate, observation)
                            .map_or(1_000.0, |(median, p90)| median + 0.5 * p90)
                    })
                    .sum();
                total / session.representatives.len().max(1) as f64
            })
            .sum()
    }
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn last_session_id(&self) -> u64 {
        self.sessions.last().map_or(0, |session| session.id)
    }

    pub fn representatives(&self) -> impl Iterator<Item = &OutlineCandidate> {
        self.sessions
            .iter()
            .flat_map(|session| session.representatives.iter())
    }

    pub fn candidates(&self) -> impl Iterator<Item = &OutlineCandidate> {
        self.sessions
            .iter()
            .flat_map(|session| session.representatives.iter().chain(&session.challengers))
    }

    #[cfg(test)]
    pub(crate) fn representative_count(&self) -> usize {
        self.sessions
            .last()
            .map_or(0, |session| session.representatives.len())
    }
    pub fn is_valid(&self, key: &str, topology: RouteTopology, domain: SamplingDomain) -> bool {
        if self.sessions.len() > MAX_SESSIONS {
            return false;
        }
        let mut previous = 0;
        for (index, session) in self.sessions.iter().enumerate() {
            if session.id == 0
                || session.id <= previous
                || session.representatives.is_empty()
                || session.representatives.len() > PER_SESSION
                || session.challengers.len() > PER_SESSION
                || (index + 1 != self.sessions.len() && !session.challengers.is_empty())
            {
                return false;
            }
            previous = session.id;
            if session
                .representatives
                .iter()
                .chain(&session.challengers)
                .any(|candidate| {
                    !candidate.valid
                        || candidate.session_id != session.id
                        || candidate.track_key != key
                        || candidate.topology != topology
                        || candidate.domain != domain
                        || candidate.comparison().is_none()
                })
            {
                return false;
            }
        }
        true
    }
    pub fn observe(&mut self, candidate: OutlineCandidate) -> bool {
        if !candidate.valid || candidate.comparison().is_none() {
            return false;
        }
        if let Some(first) = self
            .sessions
            .first()
            .and_then(|s| s.representatives.first())
            && (first.track_key != candidate.track_key
                || first.topology != candidate.topology
                || first.domain != candidate.domain)
        {
            return false;
        }
        if self
            .sessions
            .last()
            .is_none_or(|s| s.id != candidate.session_id)
        {
            if let Some(previous) = self.sessions.last_mut() {
                if candidate.session_id <= previous.id {
                    return false;
                }
                previous.challengers.clear();
            }
            self.sessions.push(SessionEvidence {
                id: candidate.session_id,
                ..Default::default()
            });
            if self.sessions.len() > MAX_SESSIONS {
                self.sessions.remove(0);
            }
        }
        let session = self.sessions.last_mut().expect("session was inserted");
        session.challengers.push(candidate.clone());
        if session.challengers.len() > PER_SESSION {
            session.challengers.remove(0);
        }
        if session.representatives.len() < PER_SESSION {
            session.representatives.push(candidate);
        }
        if session.representatives.len() == PER_SESSION && session.challengers.len() == PER_SESSION
        {
            let new_score = group_score(&session.challengers);
            let old_score = group_score(&session.representatives);
            let smoother = || {
                session.challengers.iter().map(roughness).sum::<f64>()
                    < session.representatives.iter().map(roughness).sum::<f64>()
            };
            if new_score < old_score || ((new_score - old_score).abs() < 0.1 && smoother()) {
                session.representatives.clone_from(&session.challengers);
            }
        }
        true
    }

    /// Select an observed path. Call only at a session boundary and apply game-specific gates.
    pub fn select(
        &self,
        active: &OutlineCandidate,
        initial_session: bool,
    ) -> Option<OutlineCandidate> {
        let pool: Vec<_> = self
            .sessions
            .iter()
            .flat_map(|s| s.representatives.iter())
            .collect();
        let best = pool
            .iter()
            .copied()
            .min_by(|a, b| self.weighted_score(a).total_cmp(&self.weighted_score(b)))?;
        let supporters: Vec<_> = pool.iter().copied().filter(|p| agrees(best, p)).collect();
        let mut quorum = false;
        for i in 0..supporters.len() {
            for j in i + 1..supporters.len() {
                for k in j + 1..supporters.len() {
                    let group = [supporters[i], supporters[j], supporters[k]];
                    if agrees(group[0], group[1])
                        && agrees(group[0], group[2])
                        && agrees(group[1], group[2])
                        && (initial_session
                            || group
                                .iter()
                                .any(|candidate| candidate.session_id != group[0].session_id))
                    {
                        quorum = true;
                    }
                }
            }
        }
        if !quorum {
            return None;
        }
        let old_score = self.weighted_score(active);
        let new_score = self.weighted_score(best);
        let averages = |candidate: &OutlineCandidate| -> Option<(f64, f64)> {
            let mut medians = 0.0;
            let mut p90s = 0.0;
            for session in &self.sessions {
                let count = session.representatives.len() as f64;
                for observation in &session.representatives {
                    let (median, p90) = disagreement(candidate, observation)?;
                    medians += median / count;
                    p90s += p90 / count;
                }
            }
            Some((
                medians / self.sessions.len() as f64,
                p90s / self.sessions.len() as f64,
            ))
        };
        let (old_median, old_p90) = averages(active)?;
        let (new_median, new_p90) = averages(best)?;
        if new_score > old_score * 0.8 || (old_median - new_median < 2.0 && old_p90 - new_p90 < 5.0)
        {
            return None;
        }
        Some(best.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(session_id: u64, radius: f32, domain: SamplingDomain) -> OutlineCandidate {
        OutlineCandidate {
            track_key: "test".into(),
            topology: RouteTopology::Closed,
            domain,
            valid: true,
            session_id,
            points: (0..POINT_COUNT)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / POINT_COUNT as f32;
                    (radius * angle.cos(), radius * angle.sin())
                })
                .collect(),
        }
    }

    #[test]
    fn three_agreeing_paths_can_replace_an_outlying_bootstrap() {
        let old = circle(1, 500.0, SamplingDomain::GameProgress);
        let mut history = CandidateHistory::default();
        for radius in [520.0, 521.0, 519.0] {
            assert!(history.observe(circle(1, radius, SamplingDomain::GameProgress)));
        }
        let selected = history.select(&old, true).unwrap();
        assert!(selected.points[0].0 > 518.0);
        assert_eq!(selected.points.len(), POINT_COUNT);
        assert!(history.select(&old, false).is_none());
    }

    #[test]
    fn invalid_or_mixed_domain_candidates_do_not_vote() {
        let mut history = CandidateHistory::default();
        let mut invalid = circle(1, 500.0, SamplingDomain::GameProgress);
        invalid.valid = false;
        assert!(!history.observe(invalid));
        assert!(history.observe(circle(1, 500.0, SamplingDomain::GameProgress)));
        assert!(!history.observe(circle(1, 500.0, SamplingDomain::MeasuredDistance)));
    }

    #[test]
    fn challenger_window_retains_three_clean_laps_after_outlier() {
        let old = circle(1, 500.0, SamplingDomain::MeasuredDistance);
        let mut history = CandidateHistory::default();
        for radius in [500.0, 520.0, 521.0, 519.0] {
            assert!(history.observe(circle(1, radius, SamplingDomain::MeasuredDistance)));
        }
        assert!(history.select(&old, true).is_some());
    }

    #[test]
    fn acc_selection_preserves_nonuniform_native_progress_bins() {
        let old = circle(1, 500.0, SamplingDomain::GameProgress);
        let mut history = CandidateHistory::default();
        let mut driven = circle(1, 520.0, SamplingDomain::GameProgress);
        for (index, point) in driven.points.iter_mut().enumerate() {
            let pct = index as f64 / POINT_COUNT as f64;
            let angle = (pct + 0.04 * (pct * std::f64::consts::TAU).sin()) * std::f64::consts::TAU;
            *point = ((angle.cos() * 520.0) as f32, (angle.sin() * 520.0) as f32);
        }
        for _ in 0..3 {
            assert!(history.observe(driven.clone()));
        }
        let selected = history.select(&old, true).unwrap();
        assert_eq!(selected.points[300], driven.points[300]);
        assert_eq!(selected.points, driven.points);
    }

    #[test]
    fn long_session_stays_bounded_and_does_not_gain_votes() {
        let mut history = CandidateHistory::default();
        for _ in 0..40 {
            assert!(history.observe(circle(1, 500.0, SamplingDomain::GameProgress)));
        }
        assert!(history.observe(circle(2, 510.0, SamplingDomain::GameProgress)));
        assert_eq!(history.sessions.len(), 2);
        assert_eq!(history.sessions[0].representatives.len(), 3);
        assert!(history.sessions[0].challengers.is_empty());
        assert_eq!(history.sessions[1].representatives.len(), 1);
        let long_session = circle(1, 500.0, SamplingDomain::GameProgress);
        let short_session = circle(2, 510.0, SamplingDomain::GameProgress);
        assert!(
            (history.weighted_score(&long_session) - history.weighted_score(&short_session)).abs()
                < 0.1
        );
    }
}
