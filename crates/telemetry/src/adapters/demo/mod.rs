//! Demo adapter: a simulated 60-minute GT3/GT4 race at a Monza-like track.
//!
//! Used for development without a game and shipped as the app's demo mode so people can set
//! up overlays anywhere. It only simulates raw signals (positions, lap times, inputs, fuel,
//! temperatures, flags); everything derived comes from [`crate::core::Session`], exactly as
//! for a real game. Port of `mockups/shared/mock-session.js`.

mod track;

use std::time::Instant;

use crate::adapter::{AdapterEvent, GameAdapter};
use crate::core::Session;
use crate::input::{CarInput, Input, PlayerInput, SessionInput};
use crate::model::{Capabilities, Flag, Game, PitWindow, SessionKind, Track, Tyre, Weather};

use track::{LENGTH, N, SECTORS, lerp_at};

const PIT_SPEED: f64 = 22.2;
const FUEL_PER_LAP: f64 = 2.94;
const START_FUEL: f32 = 71.0;
const REFUEL_TO: f32 = 60.0;
const DURATION: f64 = 3600.0;
/// Race time the overlays start at: long enough for history, pit window about to open.
pub const WARM_UP: f64 = 14.0 * 60.0 + 30.0;

const GT3_MODELS: [&str; 10] = [
    "Ferrari 296 GT3",
    "Porsche 992 GT3 R",
    "BMW M4 GT3",
    "Lamborghini Huracán GT3 EVO2",
    "McLaren 720S GT3 EVO",
    "Audi R8 LMS GT3 EVO II",
    "Mercedes-AMG GT3 EVO",
    "Aston Martin V8 Vantage GT3",
    "Ford Mustang GT3",
    "Honda NSX GT3 EVO",
];
const GT4_MODELS: [&str; 6] = [
    "Alpine A110 GT4",
    "BMW M4 GT4",
    "Porsche 718 Cayman GT4",
    "Aston Martin Vantage GT4",
    "Mercedes-AMG GT4",
    "Maserati MC GT4",
];
const FIRST: [&str; 26] = [
    "Luca", "Jonas", "Mateo", "Emil", "Noah", "Ilya", "Sven", "Tomás", "Kenji", "Oscar", "Rafael",
    "Hugo", "Marek", "Felix", "Arne", "Dario", "Lena", "Mika", "Yuki", "Sara", "Pietro", "Nils",
    "Kacper", "Theo", "Jules", "Bram",
];
const LAST: [&str; 26] = [
    "Marchetti",
    "Lindqvist",
    "Okafor",
    "Brandt",
    "Kowalczyk",
    "Fontaine",
    "Varga",
    "Haddad",
    "Nakamura",
    "Ruiz",
    "Sørensen",
    "Delacroix",
    "Petrov",
    "Ambrosi",
    "Weller",
    "Castell",
    "Moreau",
    "Novak",
    "Aalto",
    "Keane",
    "Bianchi",
    "Duarte",
    "Engström",
    "Valli",
    "Hartmann",
    "Rossi",
];
const NAT: [&str; 26] = [
    "ITA", "SWE", "NGA", "GER", "POL", "FRA", "HUN", "LBN", "JPN", "ESP", "DEN", "FRA", "BUL",
    "ITA", "GBR", "ESP", "FRA", "CZE", "FIN", "IRL", "ITA", "POR", "SWE", "ITA", "GER", "ITA",
];
const TEAMS: [&str; 12] = [
    "Scuderia Nord",
    "Velocity Works",
    "Apex Nine",
    "Redline Union",
    "Kestrel Racing",
    "Halcyon Motorsport",
    "Iron Ridge",
    "Delta Eight",
    "Mistral GT",
    "Lowline Racing",
    "Ostra Competition",
    "Parallax",
];
const PLAYER: usize = 6;
/// Shadows the player so the radar and blind-spot overlays have someone alongside.
const BATTLE: usize = 5;
/// Starts just ahead of the player at almost the same pace.
const AHEAD: usize = 7;

/// Park–Miller minimal standard generator: deterministic runs, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0 * 16807 % 2147483647;
        (self.0 - 1) as f64 / 2147483646.0
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pit {
    Out,
    /// Pit lane before the box.
    Entering,
    /// Stationary, seconds left.
    Stopped(f64),
    /// Pit lane after the box.
    Leaving,
}

struct SimCar {
    pace: f64,
    phase: f64,
    dist: f64,
    lap_start: f64,
    lateral: f64,
    pit: Pit,
    pit_lap: u32,
    stops: u32,
}

/// The simulated race. Call [`Demo::step`] (or `poll`, which uses wall-clock time).
pub struct Demo {
    input: Input,
    cars: Vec<SimCar>,
    rng: Rng,
    shape: track::TrackShape,
    profile: track::Profile,
    last_poll: Option<Instant>,
    lap_pace: f64,
    invalid_at: f64,
    flag_until: f64,
    next_flag_at: f64,
    next_change_at: f64,
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

impl Demo {
    pub fn new() -> Self {
        let shape = track::build();
        let profile = track::profile(&shape);
        let mut rng = Rng(46);
        let mut used = Vec::new();
        let mut inputs = Vec::new();
        let mut cars = Vec::new();
        for i in 0..26 {
            let gt3 = i < 18;
            let mut number = rng.below(98) as u32 + 2;
            while used.contains(&number) {
                number = rng.below(98) as u32 + 2;
            }
            used.push(number);
            let pace_base = if gt3 {
                0.986 + i as f64 * 0.0026
            } else {
                1.075 + (i - 18) as f64 * 0.0024
            };
            let team = TEAMS[rng.below(TEAMS.len())];
            let cat = ["P", "G", "G", "S", "S", "S", "B", "B"][rng.below(8)];
            let pace = pace_base + (rng.next() - 0.5) * 0.002;
            let phase = rng.next() * 100.0;
            let player = i == PLAYER;
            inputs.push(CarInput {
                idx: i,
                number: if player { 46 } else { number },
                driver: format!("{} {}", FIRST[i], LAST[i]),
                short: format!("{}. {}", FIRST[i].chars().next().unwrap(), LAST[i]),
                team: if player {
                    "Scuderia Nord".into()
                } else {
                    team.into()
                },
                model: if player {
                    "Ferrari 296 GT3".into()
                } else if gt3 {
                    GT3_MODELS[i % 10].into()
                } else {
                    GT4_MODELS[i % 6].into()
                },
                cls: if gt3 { "GT3" } else { "GT4" }.into(),
                cat: if player { "S" } else { cat }.into(),
                nat: NAT[i].into(),
                is_player: player,
                penalty: match i {
                    12 => "+5s",
                    21 => "DT",
                    _ => "",
                }
                .into(),
                ..Default::default()
            });
            // rolling start: the field lined up just past the line, ~9 m apart
            let dist = 0.05 - i as f64 * 0.0016;
            cars.push(SimCar {
                pace: if player { 1.0 } else { pace },
                phase,
                dist,
                lap_start: -lerp_at(&profile.t_at[..N], dist) * pace,
                lateral: 0.0,
                pit: Pit::Out,
                pit_lap: 9 + rng.below(10) as u32,
                stops: 0,
            });
        }
        cars[AHEAD].dist = cars[PLAYER].dist + 42.0 / LENGTH;
        cars[AHEAD].pace = 1.0003;
        cars[AHEAD].lap_start = -lerp_at(&profile.t_at[..N], cars[AHEAD].dist) * cars[AHEAD].pace;
        cars[BATTLE].pit_lap = u32::MAX;

        let tyres = ["FL", "FR", "RL", "RR"]
            .iter()
            .enumerate()
            .map(|(k, name)| Tyre {
                name: (*name).into(),
                pressure: 26.8,
                core: 84.0,
                i: Some(86.0),
                m: Some(84.0),
                o: Some(82.0),
                wear: Some(1.0 - k as f32 * 0.004),
                brake: if k < 2 { 420.0 } else { 330.0 },
                compound: "Dry".into(),
            })
            .collect();
        let input = Input {
            t: 0.0,
            track: Track {
                name: "Monza".into(),
                length: LENGTH,
                sectors: SECTORS.to_vec(),
                x: shape.x.iter().map(|&v| v as f32).collect(),
                y: shape.y.iter().map(|&v| v as f32).collect(),
            },
            session: SessionInput {
                kind: SessionKind::Race,
                name: "Race".into(),
                duration: DURATION,
                elapsed: 0.0,
                flag: Flag::Green,
                yellow_sector: 0,
                pit_window: Some(PitWindow {
                    open: 16.0 * 60.0,
                    close: 40.0 * 60.0,
                }),
                mandatory_stops_left: Some(1),
                time_of_day: 14.0 * 3600.0 + 18.0 * 60.0,
            },
            weather: Weather {
                air: 21.4,
                track: 28.6,
                rain_now: 0.0,
                rain10: 0.0,
                rain30: 0.2,
                wetness: 0.0,
                grip: "Optimum".into(),
                wind_speed: 9.0,
                wind_dir: 230.0,
                clouds: 0.35,
            },
            player: PlayerInput {
                car: PLAYER,
                max_rpm: 8000.0,
                shift_rpm: 7600.0,
                tc: 3,
                tc2: 2,
                abs: 3,
                bb: 56.2,
                map: 1,
                fuel: START_FUEL,
                fuel_capacity: 120.0,
                tyres,
                lap_valid: true,
                ..Default::default()
            },
            cars: inputs,
            classes: vec!["GT3".into(), "GT4".into()],
        };
        let mut demo = Self {
            input,
            cars,
            rng,
            shape,
            profile,
            last_poll: None,
            lap_pace: 1.0,
            invalid_at: 2.0,
            flag_until: 0.0,
            next_flag_at: 18.0,
            next_change_at: 15.0,
        };
        for i in 0..demo.cars.len() {
            demo.place(i);
        }
        demo.player_physics(0.0);
        demo
    }

    /// Advance the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let sub = (dt / 0.05).ceil();
        for _ in 0..sub as usize {
            self.tick(dt / sub);
        }
    }

    fn speed_at(&self, pct: f64) -> f64 {
        lerp_at(&self.profile.v, pct)
    }

    fn tick(&mut self, h: f64) {
        self.input.t += h;
        let t = self.input.t;
        let s = &mut self.input.session;
        s.elapsed += h;
        s.time_of_day += h;
        let elapsed = s.elapsed;
        let window_open = s.pit_window.map_or(0.0, |w| w.open);

        for i in 0..self.cars.len() {
            let pct = self.cars[i].dist.rem_euclid(1.0);
            let c = &self.cars[i];
            let mut v = if i == PLAYER {
                self.speed_at(pct) / self.lap_pace
                    * (1.0 + 0.004 * (t / 9.0).sin() + 0.003 * (t / 3.7 + 1.0).sin())
            } else {
                self.speed_at(pct) / c.pace * (1.0 + 0.006 * (t / 7.0 + c.phase).sin())
            };
            let laps = c.dist.floor().max(0.0) as u32;
            let wants_stop = if i == PLAYER {
                (self.input.player.fuel as f64) < FUEL_PER_LAP * 2.2
            } else {
                c.stops == 0 && laps + 1 == c.pit_lap && elapsed > window_open
            };
            let c = &mut self.cars[i];
            match c.pit {
                Pit::Out if pct > 0.95 && wants_stop => {
                    c.pit = Pit::Entering;
                    v = PIT_SPEED;
                }
                Pit::Out => {}
                Pit::Entering => {
                    v = PIT_SPEED;
                    if pct > 0.02 && pct < 0.5 {
                        c.pit = Pit::Stopped(26.0 + self.rng.next() * 4.0);
                    }
                }
                Pit::Stopped(left) => {
                    v = 0.0;
                    c.pit = if left - h <= 0.0 {
                        Pit::Leaving
                    } else {
                        Pit::Stopped(left - h)
                    };
                }
                Pit::Leaving => {
                    v = PIT_SPEED;
                    if pct > 0.05 && pct < 0.5 {
                        c.pit = Pit::Out;
                        c.stops += 1;
                        if i == PLAYER {
                            self.input.session.mandatory_stops_left = Some(0);
                            self.input.player.fuel = REFUEL_TO;
                            self.input
                                .player
                                .tyres
                                .iter_mut()
                                .for_each(|ty| ty.wear = Some(1.0));
                        }
                    }
                }
            }

            let before = self.cars[i].dist;
            if i == BATTLE && self.cars[i].pit == Pit::Out {
                // hover around the player: ahead, alongside, behind
                let player = &self.cars[PLAYER];
                let (pd, pl) = (player.dist, player.lateral);
                let c = &mut self.cars[i];
                c.dist = pd + (6.0 + 24.0 * (t * 0.22).sin()) / LENGTH;
                c.lateral = 3.4 * if (t * 0.05).sin() >= 0.0 { 1.0 } else { -1.0 } + pl;
            } else {
                let c = &mut self.cars[i];
                c.dist += v * h / LENGTH;
                c.lateral = 1.6 * (c.dist.rem_euclid(1.0) * 40.0 + c.phase).sin();
            }

            // crossing the line: close the lap with the exact crossing time
            if self.cars[i].dist.floor() > before.floor() {
                let c = &mut self.cars[i];
                let over = if v > 0.0 {
                    h.min(c.dist.rem_euclid(1.0) * LENGTH / v)
                } else {
                    0.0
                };
                let lap = t - over - c.lap_start;
                c.lap_start = t - over;
                let laps = c.dist.floor() as u32;
                if c.stops == 0 && c.pit == Pit::Out && laps >= c.pit_lap {
                    c.pit_lap = laps + 1 + self.rng.below(6) as u32;
                }
                self.input.cars[i].last_lap = lap;
                if i == PLAYER {
                    self.lap_pace = 1.0 + (self.rng.next() - 0.45) * 0.006;
                    // ~1 lap in 10 gets invalidated somewhere, never two in a row
                    let was_valid = self.input.player.lap_valid;
                    self.invalid_at = if was_valid && self.rng.next() < 0.1 {
                        0.15 + self.rng.next() * 0.75
                    } else {
                        2.0
                    };
                    self.input.player.lap_valid = true;
                    for ty in &mut self.input.player.tyres {
                        ty.wear = ty
                            .wear
                            .map(|w| (w - 0.0045 - self.rng.next() as f32 * 0.001).max(0.5));
                    }
                }
            }

            let c = &self.cars[i];
            let ci = &mut self.input.cars[i];
            ci.laps = c.dist.floor().max(0.0) as u32;
            ci.pct = c.dist.rem_euclid(1.0);
            ci.speed = v as f32;
            ci.in_pit = c.pit != Pit::Out;
            self.place(i);
        }

        self.player_physics(h);
        self.flags();
        let w = &mut self.input.weather;
        w.track += ((29.5 + (t / 200.0).sin()) as f32 - w.track) * h as f32 * 0.01;
        w.rain30 = (0.15 + 0.15 * (t / 90.0).sin()) as f32;
        w.rain10 = (w.rain30 - 0.15).max(0.0);
    }

    /// World position from lap fraction plus a sideways offset (+ = right of travel).
    fn place(&mut self, i: usize) {
        let c = &self.cars[i];
        let (a, b, k) = track::sample(c.dist);
        let h = self.shape.heading[a];
        let (nx, ny) = (-h.sin(), h.cos());
        let ci = &mut self.input.cars[i];
        ci.x = (self.shape.x[a] + (self.shape.x[b] - self.shape.x[a]) * k + nx * c.lateral) as f32;
        ci.y = (self.shape.y[a] + (self.shape.y[b] - self.shape.y[a]) * k + ny * c.lateral) as f32;
        ci.heading = h as f32;
    }

    fn player_physics(&mut self, h: f64) {
        let t = self.input.t;
        let c = &self.cars[PLAYER];
        let pct = c.dist.rem_euclid(1.0);
        let in_pit = c.pit != Pit::Out;
        let speed = self.input.cars[PLAYER].speed as f64;
        let lap_start = c.lap_start;
        let pr = &self.profile;
        let p = &mut self.input.player;

        let thr = lerp_at(&pr.throttle, pct);
        let jitter = if thr > 0.02 && thr < 0.98 {
            0.03 * (t * 13.0).sin()
        } else {
            0.0
        };
        p.throttle = if in_pit {
            if speed > 0.0 { 0.22 } else { 0.0 }
        } else {
            (thr + jitter).clamp(0.0, 1.0)
        } as f32;
        p.brake = if in_pit {
            0.0
        } else {
            lerp_at(&pr.brake, pct).clamp(0.0, 1.0) as f32
        };
        p.steer = (lerp_at(&pr.steer, pct) + 0.015 * (t * 5.3).sin()).clamp(-1.0, 1.0) as f32;
        p.clutch = 0.0;
        p.speed = (speed * 3.6) as f32;
        const TOPS: [f32; 7] = [0.0, 105.0, 142.0, 178.0, 212.0, 246.0, 276.0];
        let mut g = 1;
        while g < 6 && p.speed > TOPS[g] * 0.96 {
            g += 1;
        }
        p.gear = g as i8;
        let lo = if g > 1 { TOPS[g - 1] * 0.72 } else { 0.0 };
        p.rpm = (3200.0 + (p.speed - lo) / (TOPS[g] - lo) * (p.max_rpm - 3200.0))
            .clamp(2800.0, p.max_rpm);
        p.abs_active = p.brake > 0.8 && (t * 40.0).sin() > 0.2;
        p.tc_active = p.throttle > 0.6 && p.steer.abs() > 0.18 && (t * 31.0).sin() > 0.4;

        // lap timing, sector and track limits
        p.lap_time = t - lap_start;
        p.sector = SECTORS.iter().rposition(|&s| pct >= s).unwrap_or(0);
        if p.lap_valid && pct > self.invalid_at {
            p.lap_valid = false;
            self.input.session.flag = Flag::Blackwhite;
            self.flag_until = t + 5.0;
        }

        // fuel
        if !matches!(self.cars[PLAYER].pit, Pit::Stopped(_)) {
            p.fuel = (p.fuel
                - (FUEL_PER_LAP / pr.fuel_integral * (0.12 + p.throttle as f64) * h) as f32)
                .max(0.0);
        }

        // tyres and brakes relax toward a load-dependent temperature
        let (lat, brk, thr, spd) = (p.steer, p.brake, p.throttle, (speed / 80.0) as f32);
        let h32 = h as f32;
        for (k, ty) in p.tyres.iter_mut().enumerate() {
            let (front, left) = (k < 2, k % 2 == 0);
            let outside = if (lat > 0.0 && left) || (lat < 0.0 && !left) {
                lat.abs()
            } else {
                0.0
            };
            let target =
                74.0 + 14.0 * outside + if front { 9.0 * brk + 2.0 } else { 6.0 * thr } + 6.0 * spd;
            ty.core += (target - ty.core) * h32 * 0.08;
            ty.i = Some(ty.core + 3.2 + 1.5 * outside);
            ty.m = Some(ty.core + 0.6);
            ty.o = Some(ty.core - 2.4 + 4.0 * outside);
            ty.pressure = 20.1 + ty.core * 0.0805 + if front { 0.15 } else { 0.0 };
            let b_target = if front {
                260.0 + 620.0 * brk * spd
            } else {
                220.0 + 380.0 * brk * spd
            };
            ty.brake += (b_target - ty.brake) * h32 * if brk > 0.05 { 1.8 } else { 0.35 };
        }

        // a driver tweak every ~25 s
        if t >= self.next_change_at && h > 0.0 {
            self.next_change_at = t + 25.0;
            let up = self.rng.next() < 0.5;
            let p = &mut self.input.player;
            match self.rng.below(5) {
                0 => p.tc = (p.tc as i8 + if up { 1 } else { -1 }).clamp(1, 6) as u8,
                1 => p.tc2 = (p.tc2 as i8 + if up { 1 } else { -1 }).clamp(1, 6) as u8,
                2 => p.abs = (p.abs as i8 + if up { 1 } else { -1 }).clamp(1, 6) as u8,
                3 => p.bb = ((p.bb + if up { 0.2 } else { -0.2 }) * 10.0).round() / 10.0,
                _ => p.map = if p.map == 1 { 2 } else { 1 },
            }
        }
    }

    fn flags(&mut self) {
        let t = self.input.t;
        let s = &mut self.input.session;
        if t > self.flag_until && s.flag != Flag::Green {
            s.flag = Flag::Green;
            s.yellow_sector = 0;
        }
        if t > self.next_flag_at {
            let r = self.rng.next();
            if r < 0.55 {
                s.flag = Flag::Yellow;
                s.yellow_sector = 1 + self.rng.below(3) as u8;
                self.flag_until = t + 9.0;
            } else if r < 0.85 {
                s.flag = Flag::Blue;
                self.flag_until = t + 6.0;
            } else {
                s.flag = Flag::White;
                self.flag_until = t + 5.0;
            }
            self.next_flag_at = t + 30.0 + self.rng.next() * 25.0;
        }
    }
}

impl GameAdapter for Demo {
    fn game(&self) -> Game {
        Game::Demo
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tyre_zones: true,
            tyre_wear: true,
            driver_category: true,
            per_car_laps: true,
            car_positions: true,
        }
    }

    fn poll(&mut self) -> AdapterEvent {
        let now = Instant::now();
        let dt = self
            .last_poll
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64().min(0.25));
        self.last_poll = Some(now);
        self.step(dt);
        AdapterEvent::Updated
    }

    fn input(&self) -> &Input {
        &self.input
    }
}

/// A demo race already [`WARM_UP`] seconds in, with history, ready to render.
pub fn warmed_up() -> (Demo, Session) {
    let mut demo = Demo::new();
    let mut session = Session::new(Game::Demo, demo.capabilities());
    session.update(demo.input());
    let steps = (WARM_UP / 0.1) as usize;
    for _ in 0..steps {
        demo.step(0.1);
        session.update(demo.input());
    }
    (demo, session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmed_up_race_has_history_and_consistent_order() {
        let (_, session) = warmed_up();
        let st = &session.state;
        assert_eq!(st.cars.len(), 26);
        // positions are a permutation of 1..=26, class positions restart per class
        let mut pos: Vec<u32> = st.cars.iter().map(|c| c.pos).collect();
        pos.sort();
        assert_eq!(pos, (1..=26).collect::<Vec<_>>());
        assert_eq!(st.cars[st.order[0]].pos, 1);
        let p = &st.player;
        assert!(p.log.len() >= 6, "laps logged: {}", p.log.len());
        assert!(
            p.lap.best > 100.0 && p.lap.best < 112.0,
            "best {}",
            p.lap.best
        );
        assert!(
            p.fuel.per_lap > 2.5 && p.fuel.per_lap < 3.4,
            "fuel/lap {}",
            p.fuel.per_lap
        );
        assert!(st.session.best["GT3"].time < st.session.best["GT4"].time);
        assert!(st.cars.iter().all(|c| c.lap_times.len() >= 6));
        assert!((st.session.elapsed - WARM_UP).abs() < 1.0);
    }

    #[test]
    fn player_pits_when_fuel_runs_low() {
        let (mut demo, mut session) = warmed_up();
        for _ in 0..(30.0 * 60.0 / 0.1) as usize {
            demo.step(0.1);
            session.update(demo.input());
        }
        let p = &session.state.player;
        assert_eq!(p.pit_stops, 1, "fuel {}", p.fuel.liters);
        let car = &session.state.cars[p.car];
        assert_eq!(car.pit_laps.len(), 2, "in- and out-lap: {:?}", car.pit_laps);
        assert!(p.log.iter().filter(|e| e.pit).count() == 2);
    }
}
