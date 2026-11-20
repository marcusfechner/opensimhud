//! Fuel measurements are independent of lap timing and sector bookkeeping.
use crate::input::Input;
use crate::model::{Fuel, Game, SessionKind};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Segment {
    Timed,
    Outlap,
    Transfer,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FuelTracker {
    previous: Option<Sample>,
    segment: Option<Segment>,
    used: f32,
    distance: f64,
    start_time: f64,
    refueled: bool,
    pit_touched: bool,
    garage_observed: bool,
    history: VecDeque<(f32, f64)>,
    last: Option<f32>,
    boundary_at: Option<f64>,
    log_fuel: Option<(f64, Option<f32>)>,
    /// Clock-only EVO boundary awaiting its corresponding completed-lap update.
    pending_counter: Option<(u32, f64)>,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f64,
    fuel: f32,
    clock: f64,
    laps: u32,
    last_lap: f64,
    pit: bool,
    position: Option<(f32, f32)>,
}

impl FuelTracker {
    pub(crate) fn disconnect(&mut self) {
        self.previous = None;
        self.segment = None;
        self.garage_observed = false;
        self.pending_counter = None;
    }

    fn begin(&mut self, kind: Segment, t: f64) {
        self.segment = Some(kind);
        self.used = 0.0;
        self.distance = 0.0;
        self.start_time = t;
        self.refueled = false;
        self.pit_touched = false;
    }

    fn finish(&mut self, t: f64, display: bool, average: bool) -> Option<f32> {
        let value = self.segment.map(|_| self.used);
        if display && let Some(value) = value {
            self.last = Some(value);
        }
        if average && let Some(value) = value {
            let duration = t - self.start_time;
            if duration.is_finite() && duration > 0.0 {
                self.history.push_back((value, duration));
                if self.history.len() > 5 {
                    self.history.pop_front();
                }
            }
        }
        self.segment = None;
        value
    }

    pub(crate) fn update(&mut self, input: &Input, game: Game) -> Option<f32> {
        let Some(car) = input.cars.get(input.player.car) else {
            self.disconnect();
            return None;
        };
        let fuel = input.player.fuel;
        if !fuel.is_finite() || fuel < 0.0 {
            self.disconnect();
            return None;
        }
        let position =
            (car.located && car.x.is_finite() && car.y.is_finite()).then_some((car.x, car.y));
        let sample = Sample {
            t: input.t,
            fuel,
            clock: input.player.lap_time,
            laps: car.laps,
            last_lap: car.last_lap,
            pit: car.in_pit,
            position,
        };
        let Some(prev) = self.previous.replace(sample) else {
            self.garage_observed = input.track.closed && sample.pit && car.speed.abs() < 1.0;
            return None;
        };
        let dt = sample.t - prev.t;
        let distance = match (prev.position, sample.position) {
            (Some((x, y)), Some((nx, ny))) => Some(f64::from((nx - x).hypot(ny - y))),
            _ => None,
        };
        let paused = dt > 5.0
            && distance.is_some_and(|distance| distance < 1.0)
            && (sample.clock - prev.clock).abs() < 0.5
            && (sample.fuel - prev.fuel).abs() < 0.01;
        if paused && self.segment.is_some() {
            self.start_time += dt;
        }
        let jump = !dt.is_finite()
            || dt < 0.0
            || (dt > 5.0 && !paused)
            || distance.is_some_and(|d| d > (dt * 150.0 + 50.0));
        let fuel_jump = prev.fuel - fuel > (dt.max(0.0) as f32 * 1.5 + 2.0);
        if jump || fuel_jump {
            self.segment = None;
            self.garage_observed = false;
            if game == Game::Evo && input.track.closed {
                self.pending_counter = None;
                self.boundary_at = None;
                self.log_fuel = None;
            }
            return None;
        }
        let entering = !prev.pit && sample.pit;
        let leaving = prev.pit && !sample.pit;
        if input.track.closed && self.segment.is_none() && sample.pit && car.speed.abs() < 1.0 {
            self.garage_observed = true;
        }
        if input.track.closed
            && self.segment.is_none()
            && self.garage_observed
            && prev.pit
            && (leaving || distance.is_some_and(|d| d > 0.5))
        {
            self.begin(Segment::Outlap, prev.t);
            self.garage_observed = false;
        }
        if self.segment.is_some() {
            self.used += (prev.fuel - fuel).max(0.0);
            self.refueled |= fuel > prev.fuel + 0.05;
            self.pit_touched |= prev.pit || sample.pit;
            if let Some(distance) = distance {
                self.distance += distance;
            }
        }
        let counter = sample.laps > prev.laps;
        let clock = if game == Game::Evo && input.track.closed {
            prev.clock.is_finite()
                && sample.clock.is_finite()
                && prev.clock > sample.clock
                && sample.clock >= 0.0
        } else {
            prev.clock > 2.0 && sample.clock + 1.0 < prev.clock
        };
        let mut logged = None;
        if !input.track.closed {
            if entering {
                self.segment = None;
            } else if leaving {
                self.begin(Segment::Transfer, sample.t);
            }
        }
        if input.track.closed {
            let paired_counter = game == Game::Evo
                && counter
                && !clock
                && self.pending_counter.is_some_and(|(laps, old_clock)| {
                    // A fresh lap time matching the current clock rather than the
                    // pending clock identifies a new counter-first crossing.
                    let new_lap = sample.last_lap > 0.0
                        && (sample.last_lap - prev.clock).abs() < 1.0
                        && (sample.last_lap - old_clock).abs() > 1.0;
                    prev.laps == laps && !new_lap
                });
            let recent = self.boundary_at.is_some_and(|at| sample.t - at < 3.0);
            let duplicate = if game == Game::Evo {
                paired_counter || (clock && !counter && self.pending_counter.is_none() && recent)
            } else {
                recent
            };
            if counter {
                self.pending_counter = None;
            }
            // A moving EVO clock reset can mark a pit-lane line even without a lap
            // counter update. Require both credible speed and continuous displacement.
            let moving = car.speed.is_finite()
                && car.speed.abs() > 0.5
                && dt > 0.0
                && distance
                    .is_some_and(|d| d > 0.05 && d <= f64::from(car.speed.abs()) * dt * 3.0 + 5.0);
            let evo_clock = game == Game::Evo && clock && !counter && !duplicate;
            if evo_clock && !moving {
                // The reset cannot be attributed to a line: discard the interrupted
                // segment and wait for a witnessed start rather than merging laps.
                self.segment = None;
                self.garage_observed = false;
                self.boundary_at = Some(sample.t);
                self.pending_counter = Some((prev.laps, prev.clock));
                self.log_fuel = Some((sample.t, None));
                return None;
            }
            let boundary = counter
                || (clock
                    && (game == Game::Evo || !sample.pit || self.segment == Some(Segment::Timed)));
            if boundary && !duplicate {
                let representative = self.segment == Some(Segment::Timed)
                    && !self.pit_touched
                    && !self.refueled
                    && input.track.length.is_finite()
                    && input.track.length > 0.0
                    && self.distance >= input.track.length * 0.7;
                logged = self.finish(sample.t, true, representative);
                self.log_fuel = Some((sample.t, logged));
                self.begin(Segment::Timed, sample.t);
                self.pit_touched = sample.pit;
                self.garage_observed = false;
                self.boundary_at = Some(sample.t);
                self.pending_counter =
                    (game == Game::Evo && clock && !counter).then_some((prev.laps, prev.clock));
            }
            if counter && duplicate {
                logged = self
                    .log_fuel
                    .filter(|(at, _)| paired_counter || sample.t - at < 3.0)
                    .and_then(|(_, value)| value);
            }
        } else {
            // At the Tourist gantry the counter and last-run time advance without a clock reset.
            // At the bridge the clock resets without a fresh completed time.
            let fresh_finish = sample.last_lap > 0.0
                && sample.last_lap != prev.last_lap
                && (sample.last_lap - prev.clock).abs() < 2.0;
            if !sample.pit && !entering && fresh_finish && self.segment == Some(Segment::Timed) {
                let complete = self.segment == Some(Segment::Timed)
                    && !self.refueled
                    && input.track.length > 0.0
                    && self.distance >= input.track.length * 0.7;
                if complete {
                    logged = self.finish(sample.t, true, true);
                } else {
                    self.segment = None;
                }
                self.log_fuel = Some((sample.t, logged));
                self.begin(Segment::Transfer, sample.t);
            } else if clock && !sample.pit && !entering {
                self.begin(Segment::Timed, sample.t);
            }
        }
        logged
    }

    pub(crate) fn publish(&self, input: &Input, fuel: &mut Fuel, acc: bool) {
        fuel.liters = (input.player.fuel.is_finite() && input.player.fuel >= 0.0)
            .then_some(input.player.fuel);
        fuel.capacity = (input.player.fuel_capacity.is_finite()
            && input.player.fuel_capacity > 0.0)
            .then_some(input.player.fuel_capacity);
        fuel.last_lap_use = self.last;
        fuel.used_this_lap = self.segment.map(|_| self.used);
        let measured = (!self.history.is_empty())
            .then(|| self.history.iter().map(|(f, _)| f).sum::<f32>() / self.history.len() as f32);
        // ACC's setup estimate is useful for planning, but is not a measured lap.
        // Reject implausible values, and never use it for session projections.
        let native = input.player.fuel_per_lap;
        let provisional =
            (acc && native.is_finite() && (0.1..=100.0).contains(&native)).then_some(native);
        fuel.per_lap = measured.or(provisional);
        fuel.per_lap_estimated = measured.is_none() && provisional.is_some();
        fuel.laps_left = fuel
            .liters
            .zip(fuel.per_lap.filter(|f| *f > 0.0))
            .map(|(liters, per)| liters / per);
        let total_fuel: f64 = self.history.iter().map(|(f, _)| f64::from(*f)).sum();
        let total_time: f64 = self.history.iter().map(|(_, t)| t).sum();
        let rate =
            (!self.history.is_empty() && total_time > 0.0).then_some(total_fuel / total_time);
        fuel.empty_in = fuel
            .liters
            .zip(rate.filter(|rate| *rate > 0.0))
            .map(|(liters, rate)| f64::from(liters) / rate);
        let target = if !input.track.closed {
            None
        } else if input.session.kind == SessionKind::Race {
            match (
                input.session.fixed_total_laps,
                input.session.current_lap,
                input.cars.get(input.player.car),
                measured,
            ) {
                (Some(total), Some(current), Some(car), Some(per))
                    if total >= current && car.pct.is_finite() =>
                {
                    Some(
                        (f64::from(total - current) + 1.0 - car.pct.clamp(0.0, 1.0))
                            * f64::from(per),
                    )
                }
                (Some(_), _, _, _) => None,
                _ => input
                    .session
                    .time_remaining
                    .filter(|t| t.is_finite() && *t >= 0.0)
                    .zip(rate)
                    .zip(measured)
                    .map(|((seconds, rate), per)| seconds * rate + f64::from(per)),
            }
        } else {
            input
                .session
                .time_remaining
                .filter(|t| t.is_finite() && *t >= 0.0)
                .zip(rate)
                .map(|(seconds, rate)| seconds * rate)
        };
        fuel.to_finish = target.map(|v| v as f32);
        fuel.to_add = fuel
            .to_finish
            .zip(fuel.liters)
            .zip(measured)
            .map(|((needed, liters), per)| (needed - liters + per * 0.5).max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{CarInput, PlayerInput, SessionInput};
    use crate::model::{Flag, Track, Weather};

    #[test]
    fn acc_provisional_estimate_is_separate_from_qualified_consumption() {
        let mut tracker = FuelTracker::default();
        let mut inp = input();
        inp.session.kind = SessionKind::Race;
        inp.session.fixed_total_laps = Some(10);
        inp.session.current_lap = Some(1);
        inp.player.fuel_per_lap = 2.8;
        inp.player.fuel_laps_left = 999.0;
        let mut fuel = Fuel::default();
        tracker.publish(&inp, &mut fuel, true);
        assert_eq!(fuel.per_lap, Some(2.8));
        assert!(fuel.per_lap_estimated);
        assert!((fuel.laps_left.unwrap() - 50.0 / 2.8).abs() < 1e-4);
        assert_eq!(
            (fuel.to_finish, fuel.to_add, fuel.empty_in),
            (None, None, None)
        );
        inp.player.fuel = f32::NAN;
        tracker.publish(&inp, &mut fuel, true);
        assert_eq!(fuel.laps_left, None);
        inp.player.fuel = 0.0;
        tracker.publish(&inp, &mut fuel, true);
        assert_eq!(fuel.laps_left, Some(0.0));
        inp.player.fuel = 50.0;
        tracker.history.push_back((2.0, 90.0));
        tracker.publish(&inp, &mut fuel, true);
        assert_eq!(fuel.per_lap, Some(2.0));
        assert!(!fuel.per_lap_estimated);
        assert_eq!(fuel.laps_left, Some(25.0));
        assert_eq!(fuel.to_finish, Some(20.0));
        inp.player.fuel_per_lap = 9.0;
        tracker.publish(&inp, &mut fuel, true);
        assert_eq!(fuel.per_lap, Some(2.0));
    }

    #[test]
    fn provisional_estimate_rejects_unusable_values_and_other_games() {
        let tracker = FuelTracker::default();
        let mut inp = input();
        let mut fuel = Fuel::default();
        for value in [0.0, -1.0, 0.01, 101.0, f32::NAN, f32::INFINITY] {
            inp.player.fuel_per_lap = value;
            tracker.publish(&inp, &mut fuel, true);
            assert_eq!(fuel.per_lap, None);
            assert!(!fuel.per_lap_estimated);
        }
        inp.player.fuel_per_lap = 3.0;
        tracker.publish(&inp, &mut fuel, false);
        assert_eq!(fuel.per_lap, None);
        inp.track.closed = false;
        tracker.publish(&inp, &mut fuel, false);
        assert_eq!(fuel.per_lap, None);
    }

    fn input() -> Input {
        Input {
            t: 0.0,
            track: Track {
                length: 1000.0,
                ..Track::default()
            },
            session: SessionInput {
                generation: 0,
                kind: SessionKind::Practice,
                name: String::new(),
                duration: 0.0,
                elapsed: 0.0,
                time_remaining: Some(900.0),
                lap_remaining: None,
                current_lap: None,
                fixed_total_laps: None,
                flag: Flag::Green,
                yellow_sector: 0,
                pit_window: None,
                mandatory_stops_left: None,
                time_of_day: 0.0,
            },
            weather: Weather::default(),
            player: PlayerInput {
                fuel: 50.0,
                fuel_capacity: 100.0,
                ..PlayerInput::default()
            },
            cars: vec![CarInput {
                located: true,
                ..CarInput::default()
            }],
            classes: vec![],
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn sample(
        tracker: &mut FuelTracker,
        input: &mut Input,
        t: f64,
        clock: f64,
        laps: u32,
        fuel: f32,
        x: f32,
        pit: bool,
    ) -> Fuel {
        input.t = t;
        input.player.lap_time = clock;
        input.player.fuel = fuel;
        input.cars[0].laps = laps;
        input.cars[0].x = x;
        input.cars[0].in_pit = pit;
        tracker.update(input, Game::Demo);
        let mut f = Fuel::default();
        tracker.publish(input, &mut f, false);
        f
    }

    #[allow(clippy::too_many_arguments)]
    fn evo_sample(
        tracker: &mut FuelTracker,
        input: &mut Input,
        t: f64,
        clock: f64,
        laps: u32,
        fuel: f32,
        x: f32,
        pit: bool,
    ) -> Fuel {
        let dt = t - input.t;
        input.cars[0].speed = if dt > 0.0 {
            (x - input.cars[0].x).abs() / dt as f32
        } else {
            0.0
        };
        input.t = t;
        input.player.lap_time = clock;
        input.player.fuel = fuel;
        input.cars[0].laps = laps;
        input.cars[0].x = x;
        input.cars[0].in_pit = pit;
        tracker.update(input, Game::Evo);
        let mut fuel = Fuel::default();
        tracker.publish(input, &mut fuel, false);
        fuel
    }

    #[test]
    fn evo_clock_only_pit_line_splits_suzuka_and_brands_outlaps() {
        for (first_use, out_use) in [(0.051, 0.815), (4.462, 4.618)] {
            let mut tr = FuelTracker::default();
            let mut i = input();
            evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
            evo_sample(
                &mut tr,
                &mut i,
                1.0,
                8.0,
                0,
                50.0 - first_use / 2.0,
                3.0,
                true,
            );
            let f = evo_sample(&mut tr, &mut i, 2.0, 0.1, 0, 50.0 - first_use, 6.0, true);
            assert!((f.last_lap_use.unwrap() - first_use).abs() < 1e-3);
            assert_eq!(f.per_lap, None);
            let f = evo_sample(&mut tr, &mut i, 3.0, 1.1, 0, 50.0 - first_use, 9.0, false);
            assert!((f.last_lap_use.unwrap() - first_use).abs() < 1e-3);
            for k in 1..=10 {
                evo_sample(
                    &mut tr,
                    &mut i,
                    3.0 + f64::from(k),
                    1.1 + f64::from(k),
                    0,
                    50.0 - first_use - out_use * k as f32 / 10.0,
                    9.0 + k as f32 * 80.0,
                    false,
                );
            }
            let f = evo_sample(
                &mut tr,
                &mut i,
                14.0,
                0.0,
                1,
                50.0 - first_use - out_use,
                889.0,
                false,
            );
            assert!((f.last_lap_use.unwrap() - out_use).abs() < 1e-3);
            assert_eq!(f.per_lap, None);
            for k in 1..=10 {
                evo_sample(
                    &mut tr,
                    &mut i,
                    14.0 + f64::from(k),
                    f64::from(k),
                    1,
                    50.0 - first_use - out_use - k as f32 * 0.2,
                    889.0 + k as f32 * 80.0,
                    false,
                );
            }
            let f = evo_sample(
                &mut tr,
                &mut i,
                25.0,
                0.0,
                2,
                48.0 - first_use - out_use,
                1769.0,
                false,
            );
            assert!((f.per_lap.unwrap() - 2.0).abs() < 1e-3);
        }
    }

    #[test]
    fn evo_delayed_counter_does_not_finalize_clock_boundary_twice() {
        for counter_first in [false, true] {
            let mut tr = FuelTracker::default();
            let mut i = input();
            evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
            evo_sample(&mut tr, &mut i, 1.0, 20.0, 0, 49.9, 5.0, true);
            let f = if counter_first {
                evo_sample(&mut tr, &mut i, 2.0, 21.0, 1, 49.8, 10.0, true)
            } else {
                evo_sample(&mut tr, &mut i, 2.0, 0.1, 0, 49.8, 10.0, true)
            };
            let last = f.last_lap_use;
            assert!((last.unwrap() - 0.2).abs() < 1e-3);
            if counter_first {
                let f = evo_sample(&mut tr, &mut i, 3.0, 0.1, 1, 49.7, 15.0, true);
                assert_eq!(f.last_lap_use, last);
            } else {
                evo_sample(&mut tr, &mut i, 5.0, 3.1, 0, 49.7, 15.0, true);
                let f = evo_sample(&mut tr, &mut i, 6.0, 4.1, 1, 49.6, 20.0, true);
                assert_eq!(f.last_lap_use, last);
                assert!((f.used_this_lap.unwrap() - 0.2).abs() < 1e-3);
            }
            assert!(tr.history.is_empty());
        }
    }

    #[test]
    fn evo_short_established_clock_does_not_require_minimum_lap_time() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
        evo_sample(&mut tr, &mut i, 0.2, 0.8, 0, 49.9, 2.0, true);
        let f = evo_sample(&mut tr, &mut i, 0.4, 0.1, 0, 49.8, 4.0, true);
        assert!((f.last_lap_use.unwrap() - 0.2).abs() < 1e-3);
        assert_eq!(f.per_lap, None);
    }

    #[test]
    fn evo_clock_reset_is_seen_after_a_sampling_gap() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
        evo_sample(&mut tr, &mut i, 1.0, 20.0, 0, 49.9, 2.0, true);
        let f = evo_sample(&mut tr, &mut i, 4.0, 2.1, 0, 49.8, 20.0, true);
        assert!((f.last_lap_use.unwrap() - 0.2).abs() < 1e-3);
        assert_eq!(f.per_lap, None);
    }

    #[test]
    fn evo_short_successive_clock_boundaries_are_distinct() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
        evo_sample(&mut tr, &mut i, 0.2, 0.8, 0, 49.9, 2.0, true);
        evo_sample(&mut tr, &mut i, 0.4, 0.1, 0, 49.8, 4.0, true);
        evo_sample(&mut tr, &mut i, 0.6, 0.8, 0, 49.7, 6.0, false);
        let f = evo_sample(&mut tr, &mut i, 0.8, 0.1, 0, 49.6, 8.0, false);
        assert!((f.last_lap_use.unwrap() - 0.2).abs() < 1e-3);
        assert!((f.used_this_lap.unwrap()).abs() < 1e-3);
        assert_eq!(f.per_lap, None);
    }

    #[test]
    fn evo_counter_can_follow_short_clock_boundary_after_three_seconds() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
        evo_sample(&mut tr, &mut i, 1.0, 2.0, 0, 49.9, 2.0, true);
        let f = evo_sample(&mut tr, &mut i, 2.0, 0.1, 0, 49.8, 4.0, true);
        let last = f.last_lap_use;
        evo_sample(&mut tr, &mut i, 5.0, 3.1, 0, 49.7, 6.0, true);
        let f = evo_sample(&mut tr, &mut i, 6.0, 4.1, 1, 49.6, 8.0, true);
        assert_eq!(f.last_lap_use, last);
        assert!((f.used_this_lap.unwrap() - 0.2).abs() < 1e-3);
    }

    #[test]
    fn ambiguous_evo_reset_discards_segment_without_merging() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        evo_sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, true);
        evo_sample(&mut tr, &mut i, 1.0, 10.0, 0, 49.8, 5.0, true);
        let f = evo_sample(&mut tr, &mut i, 2.0, 0.0, 0, 49.7, 5.0, true);
        assert_eq!(f.last_lap_use, None);
        assert_eq!(f.used_this_lap, None);
        let f = evo_sample(&mut tr, &mut i, 6.0, 4.0, 1, 49.6, 15.0, true);
        assert_eq!(f.last_lap_use, None);
        assert_eq!(f.used_this_lap, None);
        let f = evo_sample(&mut tr, &mut i, 7.0, 5.0, 1, 49.5, 5000.0, false);
        assert_eq!(f.last_lap_use, None);
        assert_eq!(f.used_this_lap, None);
    }
    #[test]
    fn witnessed_outlap_then_first_complete_lap_and_timed_practice() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 20.0, 0, 50.0, 0.0, true);
        sample(&mut tr, &mut i, 1.0, 21.0, 0, 49.9, 0.0, false);
        sample(&mut tr, &mut i, 2.0, 22.0, 0, 49.5, 50.0, false);
        let f = sample(&mut tr, &mut i, 3.0, 0.0, 0, 49.0, 100.0, false);
        assert!((f.last_lap_use.unwrap() - 1.0).abs() < 0.001);
        assert_eq!(f.per_lap, None);
        for k in 1..=10 {
            sample(
                &mut tr,
                &mut i,
                3.0 + k as f64,
                k as f64,
                0,
                49.0 - k as f32 * 0.2,
                100.0 + k as f32 * 80.0,
                false,
            );
        }
        let f = sample(&mut tr, &mut i, 14.0, 0.0, 1, 46.8, 980.0, false);
        assert!((f.per_lap.unwrap() - 2.2).abs() < 0.001);
        assert!(f.laps_left.is_some());
        assert!((f.to_finish.unwrap() - 180.0).abs() < 1.0);
    }

    #[test]
    fn garage_departure_observed_after_connection_starts_initial_outlap() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 0.0, 0, 50.0, 0.0, false);
        sample(&mut tr, &mut i, 1.0, 1.0, 0, 50.0, 0.0, true);
        sample(&mut tr, &mut i, 2.0, 4.0, 0, 49.9, 20.0, true);
        let f = sample(&mut tr, &mut i, 3.0, 0.0, 0, 49.8, 40.0, true);
        assert_eq!(f.last_lap_use, None);
        sample(&mut tr, &mut i, 4.0, 1.0, 0, 49.7, 50.0, false);
        let f = sample(&mut tr, &mut i, 7.0, 0.0, 1, 49.5, 60.0, false);
        assert!((f.last_lap_use.unwrap() - 0.5).abs() < 0.001);
        assert_eq!(f.per_lap, None);
    }
    #[test]
    fn tourist_finish_survives_return_and_next_approach() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        i.track.closed = false;
        sample(&mut tr, &mut i, 0.0, 10.0, 0, 50.0, 0.0, true);
        sample(&mut tr, &mut i, 1.0, 11.0, 0, 49.9, 0.0, false);
        sample(&mut tr, &mut i, 2.0, 0.0, 0, 49.5, 20.0, false);
        for k in 1..=10 {
            sample(
                &mut tr,
                &mut i,
                2.0 + k as f64,
                k as f64,
                0,
                49.5 - k as f32 * 0.2,
                20.0 + k as f32 * 80.0,
                false,
            );
        }
        i.cars[0].last_lap = 10.0;
        let f = sample(&mut tr, &mut i, 13.0, 11.0, 1, 47.3, 900.0, false);
        let last = f.last_lap_use.unwrap();
        assert!((last - 2.2).abs() < 0.001);
        assert_eq!(f.to_finish, None);
        let f = sample(&mut tr, &mut i, 14.0, 12.0, 1, 47.0, 950.0, true);
        assert_eq!(f.last_lap_use, Some(last));
        sample(&mut tr, &mut i, 15.0, 13.0, 1, 60.0, 950.0, true);
        sample(&mut tr, &mut i, 16.0, 14.0, 1, 59.9, 950.0, false);
        let f = sample(&mut tr, &mut i, 17.0, 0.0, 1, 59.5, 950.0, false);
        assert_eq!(f.last_lap_use, Some(last));
        let f = sample(&mut tr, &mut i, 18.0, 1.0, 1, 59.3, 1000.0, false);
        assert_eq!(f.last_lap_use, Some(last));
        for k in 2..=10 {
            let f = sample(
                &mut tr,
                &mut i,
                17.0 + f64::from(k),
                f64::from(k),
                1,
                59.5 - k as f32 * 0.3,
                950.0 + k as f32 * 80.0,
                false,
            );
            assert_eq!(f.last_lap_use, Some(last));
        }
        i.cars[0].last_lap = 10.5;
        let f = sample(&mut tr, &mut i, 28.0, 11.0, 2, 56.2, 1830.0, false);
        assert!((f.last_lap_use.unwrap() - 3.3).abs() < 0.001);
        assert!((f.per_lap.unwrap() - 2.75).abs() < 0.001);
        assert_eq!(f.to_finish, None);
    }
    #[test]
    fn partial_connection_shortcut_and_following_full_lap() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        let f = sample(&mut tr, &mut i, 0.0, 30.0, 0, 50.0, 0.0, false);
        assert_eq!(f.last_lap_use, None);
        assert_eq!(f.per_lap, None);
        sample(&mut tr, &mut i, 1.0, 0.0, 1, 49.9, 0.0, false);
        for k in 1..=4 {
            sample(
                &mut tr,
                &mut i,
                1.0 + k as f64,
                k as f64,
                1,
                49.9 - k as f32 * 0.1,
                k as f32 * 75.0,
                false,
            );
        }
        let f = sample(&mut tr, &mut i, 6.0, 0.0, 2, 49.4, 350.0, false);
        assert!((f.last_lap_use.unwrap() - 0.5).abs() < 0.001);
        assert_eq!(f.per_lap, None);
        for k in 1..=10 {
            sample(
                &mut tr,
                &mut i,
                6.0 + k as f64,
                k as f64,
                2,
                49.4 - k as f32 * 0.2,
                350.0 + k as f32 * 80.0,
                false,
            );
        }
        i.player.lap_valid = false;
        let f = sample(&mut tr, &mut i, 17.0, 0.0, 3, 47.2, 1230.0, false);
        assert!((f.per_lap.unwrap() - 2.2).abs() < 0.001);
    }
    #[test]
    fn asynchronous_counter_and_clock_do_not_create_second_segment() {
        for counter_first in [false, true] {
            let mut tr = FuelTracker::default();
            let mut i = input();
            sample(&mut tr, &mut i, 0.0, 40.0, 0, 50.0, 0.0, false);
            sample(&mut tr, &mut i, 1.0, 0.0, 1, 49.9, 0.0, false);
            for k in 1..=10 {
                sample(
                    &mut tr,
                    &mut i,
                    1.0 + k as f64,
                    k as f64,
                    1,
                    49.9 - k as f32 * 0.1,
                    k as f32 * 80.0,
                    false,
                );
            }
            let f = if counter_first {
                sample(&mut tr, &mut i, 12.0, 11.0, 2, 48.8, 880.0, false)
            } else {
                sample(&mut tr, &mut i, 12.0, 0.0, 1, 48.8, 880.0, false)
            };
            let last = f.last_lap_use;
            assert!(last.is_some());
            let f = sample(&mut tr, &mut i, 13.0, 0.1, 2, 48.7, 900.0, false);
            assert_eq!(f.last_lap_use, last);
            assert!((f.used_this_lap.unwrap() - 0.1).abs() < 0.001);
            assert_eq!(tr.history.len(), 1);
        }
    }
    #[test]
    fn pit_entry_refuel_and_teleport_preserve_average() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 30.0, 0, 50.0, 0.0, false);
        sample(&mut tr, &mut i, 1.0, 0.0, 1, 49.9, 0.0, false);
        for k in 1..=10 {
            sample(
                &mut tr,
                &mut i,
                1.0 + k as f64,
                k as f64,
                1,
                49.9 - k as f32 * 0.1,
                k as f32 * 80.0,
                false,
            );
        }
        let f = sample(&mut tr, &mut i, 12.0, 0.0, 2, 48.8, 880.0, false);
        let avg = f.per_lap;
        let last = f.last_lap_use;
        let f = sample(&mut tr, &mut i, 13.0, 1.0, 2, 48.5, 900.0, true);
        assert_eq!(f.last_lap_use, last);
        assert_eq!(f.per_lap, avg);
        sample(&mut tr, &mut i, 14.0, 2.0, 2, 60.0, 900.0, true);
        sample(&mut tr, &mut i, 15.0, 3.0, 2, 59.9, 900.0, false);
        let f = sample(&mut tr, &mut i, 16.0, 4.0, 2, 59.8, 5000.0, false);
        assert_eq!(f.used_this_lap, None);
        assert_eq!(f.per_lap, avg);
    }
    #[test]
    fn empty_tank_zero_measurement_and_targets_remain_distinct() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 30.0, 0, 0.0, 0.0, false);
        let f = sample(&mut tr, &mut i, 1.0, 0.0, 1, 0.0, 0.0, false);
        assert_eq!(f.liters, Some(0.0));
        assert_eq!(f.last_lap_use, None);
        assert_eq!(f.to_finish, None);
        for k in 1..=10 {
            sample(
                &mut tr,
                &mut i,
                1.0 + k as f64,
                k as f64,
                1,
                0.0,
                k as f32 * 80.0,
                false,
            );
        }
        let f = sample(&mut tr, &mut i, 12.0, 0.0, 2, 0.0, 880.0, false);
        assert_eq!(f.last_lap_use, Some(0.0));
        assert_eq!(f.per_lap, Some(0.0));
        assert_eq!(f.laps_left, None);
    }

    #[test]
    fn rolling_history_excludes_shortcuts_and_keeps_only_five() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 20.0, 0, 100.0, 0.0, false);
        sample(&mut tr, &mut i, 1.0, 0.0, 1, 100.0, 0.0, false);
        let mut t = 1.0;
        let mut x = 0.0;
        let mut fuel = 100.0;
        for lap in 1..=6 {
            for k in 1..=10 {
                t += 1.0;
                x += 80.0;
                fuel -= lap as f32 * 0.1;
                sample(&mut tr, &mut i, t, k as f64, lap, fuel, x, false);
            }
            t += 1.0;
            x += 80.0;
            let f = sample(&mut tr, &mut i, t, 0.0, lap + 1, fuel, x, false);
            assert_eq!(tr.history.len(), (lap as usize).min(5));
            if lap == 6 {
                assert!((f.per_lap.unwrap() - 4.0).abs() < 0.001);
            }
        }
    }
    #[test]
    fn fixed_lap_and_timed_race_require_qualified_evidence() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        i.session.kind = SessionKind::Race;
        i.session.time_remaining = None;
        let mut f = Fuel::default();
        tr.publish(&i, &mut f, false);
        assert_eq!(f.to_finish, None);
        tr.history.push_back((2.0, 90.0));
        tr.publish(&i, &mut f, false);
        assert_eq!(f.to_finish, None);
        i.session.time_remaining = Some(900.0);
        tr.publish(&i, &mut f, false);
        assert!((f.to_finish.unwrap() - 22.0).abs() < 0.001);
        i.session.fixed_total_laps = Some(10);
        i.session.current_lap = Some(8);
        i.cars[0].pct = 0.5;
        tr.publish(&i, &mut f, false);
        assert!((f.to_finish.unwrap() - 5.0).abs() < 0.001);
    }
    #[test]
    fn practice_requirement_zero_rate_and_unknown_countdown() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        i.player.fuel = 25.0;
        tr.history.push_back((2.0, 90.0));
        let mut f = Fuel::default();
        tr.publish(&i, &mut f, false);
        assert!((f.to_finish.unwrap() - 20.0).abs() < 0.001);
        assert_eq!(f.to_add, Some(0.0));
        i.session.time_remaining = None;
        tr.publish(&i, &mut f, false);
        assert_eq!(f.to_finish, None);
        tr.history.clear();
        tr.history.push_back((0.0, 90.0));
        i.session.time_remaining = Some(900.0);
        tr.publish(&i, &mut f, false);
        assert_eq!(f.to_finish, Some(0.0));
        assert_eq!(f.empty_in, None);
    }
    #[test]
    fn timing_lines_bound_pit_laps_across_refuel() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        sample(&mut tr, &mut i, 0.0, 30.0, 0, 50.0, 0.0, false);
        sample(&mut tr, &mut i, 1.0, 0.0, 1, 50.0, 0.0, false);
        let f = sample(&mut tr, &mut i, 5.0, 4.0, 1, 49.8, 20.0, true);
        assert_eq!(f.last_lap_use, None);
        let f = sample(&mut tr, &mut i, 6.0, 0.0, 2, 49.7, 30.0, true);
        assert!((f.last_lap_use.unwrap() - 0.3).abs() < 0.001);
        sample(&mut tr, &mut i, 7.0, 1.0, 2, 49.6, 40.0, true);
        sample(&mut tr, &mut i, 8.0, 2.0, 2, 60.0, 50.0, true);
        let f = sample(&mut tr, &mut i, 9.0, 3.0, 2, 59.8, 60.0, false);
        assert!((f.last_lap_use.unwrap() - 0.3).abs() < 0.001);
        let f = sample(&mut tr, &mut i, 13.0, 0.0, 3, 59.7, 80.0, false);
        assert!((f.last_lap_use.unwrap() - 0.4).abs() < 0.001);
        assert_eq!(f.per_lap, None);
    }

    #[test]
    fn pit_crossings_coalesce_delayed_signals_and_next_lap_is_eligible() {
        for counter_first in [false, true] {
            for refuel in [false, true] {
                let mut tr = FuelTracker::default();
                let mut i = input();
                tr.history.push_back((2.0, 90.0));
                sample(&mut tr, &mut i, 0.0, 20.0, 0, 50.0, 0.0, false);
                sample(&mut tr, &mut i, 1.0, 0.0, 1, 50.0, 0.0, false);
                for k in 1..=10 {
                    sample(
                        &mut tr,
                        &mut i,
                        1.0 + f64::from(k),
                        f64::from(k),
                        1,
                        50.0 - k as f32 * 0.1,
                        k as f32 * 80.0,
                        false,
                    );
                }
                let f = sample(&mut tr, &mut i, 12.0, 11.0, 1, 48.9, 880.0, true);
                assert_eq!(f.last_lap_use, None);
                let f = if counter_first {
                    sample(&mut tr, &mut i, 13.0, 12.0, 2, 48.8, 900.0, true)
                } else {
                    sample(&mut tr, &mut i, 13.0, 0.0, 1, 48.8, 900.0, true)
                };
                let inlap = f.last_lap_use.unwrap();
                assert!((inlap - 1.2).abs() < 0.001);
                let f = sample(&mut tr, &mut i, 14.0, 0.1, 2, 48.7, 920.0, true);
                assert_eq!(f.last_lap_use, Some(inlap));
                let added = if refuel { 10.0 } else { 0.0 };
                sample(&mut tr, &mut i, 15.0, 1.1, 2, 48.7 + added, 930.0, true);
                let f = sample(&mut tr, &mut i, 16.0, 2.1, 2, 48.6 + added, 950.0, false);
                assert_eq!(f.last_lap_use, Some(inlap));
                for k in 1..=10 {
                    sample(
                        &mut tr,
                        &mut i,
                        16.0 + f64::from(k),
                        2.1 + f64::from(k),
                        2,
                        48.6 + added - k as f32 * 0.1,
                        950.0 + k as f32 * 80.0,
                        false,
                    );
                }
                let f = sample(&mut tr, &mut i, 27.0, 0.0, 3, 47.5 + added, 1830.0, false);
                assert!((f.last_lap_use.unwrap() - 1.3).abs() < 0.001);
                assert_eq!(tr.history.len(), 1);
                for k in 1..=10 {
                    sample(
                        &mut tr,
                        &mut i,
                        27.0 + f64::from(k),
                        f64::from(k),
                        3,
                        47.5 + added - k as f32 * 0.1,
                        1830.0 + k as f32 * 80.0,
                        false,
                    );
                }
                sample(&mut tr, &mut i, 38.0, 0.0, 4, 46.4 + added, 2710.0, false);
                assert_eq!(tr.history.len(), 2);
            }
        }
    }
    #[test]
    fn pause_and_disconnection_preserve_qualified_history() {
        let mut tr = FuelTracker::default();
        let mut i = input();
        tr.history.push_back((2.0, 90.0));
        sample(&mut tr, &mut i, 0.0, 20.0, 0, 50.0, 0.0, false);
        sample(&mut tr, &mut i, 1.0, 0.0, 1, 50.0, 0.0, false);
        sample(&mut tr, &mut i, 2.0, 1.0, 1, 49.9, 50.0, false);
        let f = sample(&mut tr, &mut i, 22.0, 1.0, 1, 49.9, 50.0, false);
        assert!(f.used_this_lap.is_some());
        assert_eq!(f.per_lap, Some(2.0));
        tr.disconnect();
        let f = sample(&mut tr, &mut i, 23.0, 2.0, 1, 49.8, 100.0, false);
        assert_eq!(f.used_this_lap, None);
        assert_eq!(f.per_lap, Some(2.0));
    }
}
