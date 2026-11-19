//! Splits the derived [`State`] into frames sent to the overlay windows, each at the rate its
//! content changes, so a window never parses the whole state 60 times a second.
//!
//! | frame       | rate          | content                                                  |
//! |-------------|---------------|----------------------------------------------------------|
//! | `fast`      | 60 Hz         | player inputs, lap timing, every car's position/relative |
//! | `standings` | 10 Hz         | order, positions, gaps, lap times                        |
//! | `slow`      | 2 Hz          | session, weather, fuel, tyres                            |
//! | `meta`, `track`, `entries`, `history` | on change | game, track, entry list, lap logs  |

use serde::Serialize;
use ts_rs::TS;

use crate::model::{
    Capabilities, Connection, Fuel, Game, Lap, LapEntry, SessionInfo, State, Track, Tyre, Weather,
};

const FAST_HZ: f64 = 60.0;
const STANDINGS_HZ: f64 = 10.0;
const SLOW_HZ: f64 = 2.0;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum Frame {
    Meta(MetaFrame),
    Track(Track),
    Entries(EntriesFrame),
    Fast(FastFrame),
    Standings(StandingsFrame),
    Slow(SlowFrame),
    History(HistoryFrame),
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetaFrame {
    pub game: Game,
    pub connection: Connection,
    pub capabilities: Capabilities,
    pub classes: Vec<String>,
}

/// Who is in each car; changes rarely (driver swaps, cars joining).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Entry {
    pub idx: usize,
    pub number: u32,
    pub driver: String,
    pub short: String,
    pub team: String,
    pub model: String,
    pub cls: String,
    pub cat: String,
    pub nat: String,
    pub is_player: bool,
    /// The car's world position is known; maps leave it out until then. Here rather than
    /// in the fast frame because it rarely changes.
    pub located: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EntriesFrame {
    pub cars: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerFast {
    pub car: usize,
    pub speed: f32,
    pub gear: i8,
    pub rpm: f32,
    pub max_rpm: f32,
    pub shift_rpm: f32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steer: f32,
    pub slip_angles_deg: Option<[f32; 4]>,
    pub slip_balance_deg: Option<f32>,
    pub abs_active: bool,
    pub tc_active: bool,
    pub tc: u8,
    pub tc2: u8,
    pub abs: u8,
    pub bb: f32,
    pub map: u8,
    pub changed: String,
    pub changed_at: f64,
    pub lap: Lap,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CarFast {
    // f32 keeps this 60 Hz frame small; centimeters and milliseconds are plenty here
    pub pct: f32,
    pub dist: f64,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub speed: f32,
    pub rel_meters: f32,
    pub rel_lateral: f32,
    pub rel_gap: f32,
    pub in_pit: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FastFrame {
    pub t: f64,
    pub player: PlayerFast,
    /// Indexed like the entry list.
    pub cars: Vec<CarFast>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Standing {
    pub laps: u32,
    pub pos: u32,
    pub class_pos: u32,
    pub start_pos: u32,
    pub last_lap: f64,
    pub best_lap: f64,
    pub gap_leader: f64,
    pub gap_ahead: f64,
    pub laps_down: u32,
    pub laps_ahead: u32,
    pub rel_laps: i32,
    pub out_lap: bool,
    pub pit_stops: u32,
    pub penalty: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StandingsFrame {
    /// Car indices by overall position.
    pub order: Vec<usize>,
    pub cars: Vec<Standing>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SlowFrame {
    pub session: SessionInfo,
    pub weather: Weather,
    pub fuel: Fuel,
    pub tyres: Vec<Tyre>,
    pub stint_laps: u32,
    pub pit_stops: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CarHistory {
    pub lap_times: Vec<f64>,
    pub pit_laps: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryFrame {
    pub log: Vec<LapEntry>,
    pub cars: Vec<CarHistory>,
}

/// Decides which frames are due. `now` is monotonic wall-clock seconds, so rates hold even
/// while the sim is paused.
#[derive(Default)]
pub struct Publisher {
    next_fast: f64,
    next_standings: f64,
    next_slow: f64,
    meta: Option<MetaFrame>,
    track: Option<(String, usize, u64, u64, bool, u32)>,
    entries: Option<EntriesFrame>,
    history: Option<(usize, u32, usize, usize)>,
}

impl Publisher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every frame, for a window that just subscribed.
    pub fn full(state: &State) -> Vec<Frame> {
        vec![
            Frame::Meta(meta(state)),
            Frame::Track(state.track.clone()),
            Frame::Entries(entries(state)),
            Frame::Slow(slow(state)),
            Frame::History(history(state)),
            Frame::Standings(standings(state)),
            Frame::Fast(fast(state)),
        ]
    }

    /// Only the meta frame, when it changed: used while the game is not connected, so windows
    /// learn the connection state without receiving empty data.
    pub fn poll_meta(&mut self, state: &State) -> Option<Frame> {
        let m = meta(state);
        (self.meta.as_ref() != Some(&m)).then(|| {
            self.meta = Some(m.clone());
            Frame::Meta(m)
        })
    }

    /// The meta frame alone, for a window that subscribed while the game is not connected.
    pub fn meta_only(state: &State) -> Frame {
        Frame::Meta(meta(state))
    }

    /// The frames due at `now`.
    pub fn poll(&mut self, state: &State, now: f64) -> Vec<Frame> {
        let mut out = Vec::new();
        let m = meta(state);
        if self.meta.as_ref() != Some(&m) {
            self.meta = Some(m.clone());
            out.push(Frame::Meta(m));
        }
        let t = &state.track;
        let track_sig = (
            t.name.clone(),
            t.x.len(),
            t.length.to_bits(),
            sectors_signature(&t.sectors),
            t.closed,
            t.geometry_revision,
        );
        if self.track.as_ref() != Some(&track_sig) {
            self.track = Some(track_sig);
            out.push(Frame::Track(t.clone()));
        }
        let e = entries(state);
        if self.entries.as_ref() != Some(&e) {
            self.entries = Some(e.clone());
            out.push(Frame::Entries(e));
        }
        let h_sig = history_sig(state);
        if self.history != Some(h_sig) {
            self.history = Some(h_sig);
            out.push(Frame::History(history(state)));
        }
        if now >= self.next_slow {
            self.next_slow = advance(self.next_slow, now, SLOW_HZ);
            out.push(Frame::Slow(slow(state)));
        }
        if now >= self.next_standings {
            self.next_standings = advance(self.next_standings, now, STANDINGS_HZ);
            out.push(Frame::Standings(standings(state)));
        }
        if now >= self.next_fast {
            self.next_fast = advance(self.next_fast, now, FAST_HZ);
            out.push(Frame::Fast(fast(state)));
        }
        out
    }
}

fn sectors_signature(sectors: &[f64]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64 ^ sectors.len() as u64;
    for sector in sectors {
        hash ^= sector.to_bits();
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Next due time on a fixed grid, skipping ticks that were missed.
fn advance(due: f64, now: f64, hz: f64) -> f64 {
    let step = 1.0 / hz;
    let next = due + step;
    if next <= now { now + step } else { next }
}

fn meta(s: &State) -> MetaFrame {
    MetaFrame {
        game: s.game,
        connection: s.connection,
        capabilities: s.capabilities,
        classes: s.classes.clone(),
    }
}

fn entries(s: &State) -> EntriesFrame {
    EntriesFrame {
        cars: s
            .cars
            .iter()
            .map(|c| Entry {
                idx: c.idx,
                number: c.number,
                driver: c.driver.clone(),
                short: c.short.clone(),
                team: c.team.clone(),
                model: c.model.clone(),
                cls: c.cls.clone(),
                cat: c.cat.clone(),
                nat: c.nat.clone(),
                is_player: c.is_player,
                located: c.located,
            })
            .collect(),
    }
}

fn fast(s: &State) -> FastFrame {
    let p = &s.player;
    FastFrame {
        t: s.t,
        player: PlayerFast {
            car: p.car,
            speed: p.speed,
            gear: p.gear,
            rpm: p.rpm,
            max_rpm: p.max_rpm,
            shift_rpm: p.shift_rpm,
            throttle: p.throttle,
            brake: p.brake,
            clutch: p.clutch,
            steer: p.steer,
            slip_angles_deg: p.slip_angles_deg,
            slip_balance_deg: p.slip_balance_deg,
            abs_active: p.abs_active,
            tc_active: p.tc_active,
            tc: p.tc,
            tc2: p.tc2,
            abs: p.abs,
            bb: p.bb,
            map: p.map,
            changed: p.changed.clone(),
            changed_at: p.changed_at,
            lap: p.lap.clone(),
        },
        cars: s
            .cars
            .iter()
            .map(|c| CarFast {
                pct: c.pct as f32,
                dist: c.dist,
                x: c.x,
                y: c.y,
                heading: c.heading,
                speed: c.speed,
                rel_meters: c.rel_meters as f32,
                rel_lateral: c.rel_lateral as f32,
                rel_gap: c.rel_gap as f32,
                in_pit: c.in_pit,
            })
            .collect(),
    }
}

fn standings(s: &State) -> StandingsFrame {
    StandingsFrame {
        order: s.order.clone(),
        cars: s
            .cars
            .iter()
            .map(|c| Standing {
                laps: c.laps,
                pos: c.pos,
                class_pos: c.class_pos,
                start_pos: c.start_pos,
                last_lap: c.last_lap,
                best_lap: c.best_lap,
                gap_leader: c.gap_leader,
                gap_ahead: c.gap_ahead,
                laps_down: c.laps_down,
                laps_ahead: c.laps_ahead,
                rel_laps: c.rel_laps,
                out_lap: c.out_lap,
                pit_stops: c.pit_stops,
                penalty: c.penalty.clone(),
            })
            .collect(),
    }
}

fn slow(s: &State) -> SlowFrame {
    let p = &s.player;
    SlowFrame {
        session: s.session.clone(),
        weather: s.weather.clone(),
        fuel: p.fuel.clone(),
        tyres: p.tyres.clone(),
        stint_laps: p.stint_laps,
        pit_stops: p.pit_stops,
    }
}

fn history(s: &State) -> HistoryFrame {
    HistoryFrame {
        log: s.player.log.clone(),
        cars: s
            .cars
            .iter()
            .map(|c| CarHistory {
                lap_times: c.lap_times.clone(),
                pit_laps: c.pit_laps.clone(),
            })
            .collect(),
    }
}

/// Changes whenever any car completes a lap or a pit lap is recorded.
fn history_sig(s: &State) -> (usize, u32, usize, usize) {
    (
        s.player.log.len(),
        s.player.log.last().map_or(0, |e| e.lap),
        s.cars.iter().map(|c| c.lap_times.len()).sum(),
        s.cars.iter().map(|c| c.pit_laps.len()).sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameAdapter;
    use crate::adapters::demo;

    fn kinds(frames: &[Frame]) -> Vec<&'static str> {
        frames
            .iter()
            .map(|f| match f {
                Frame::Meta(_) => "meta",
                Frame::Track(_) => "track",
                Frame::Entries(_) => "entries",
                Frame::Fast(_) => "fast",
                Frame::Standings(_) => "standings",
                Frame::Slow(_) => "slow",
                Frame::History(_) => "history",
            })
            .collect()
    }

    #[test]
    fn sends_each_stream_at_its_rate() {
        let (mut demo, mut session) = demo::warmed_up();
        let mut publisher = Publisher::new();
        let first = publisher.poll(&session.state, 0.0);
        assert_eq!(
            kinds(&first),
            [
                "meta",
                "track",
                "entries",
                "history",
                "slow",
                "standings",
                "fast"
            ]
        );
        // one simulated second at 120 polls per second
        let mut counts = std::collections::BTreeMap::new();
        for k in 1..=120 {
            demo.step(1.0 / 120.0);
            session.update(demo.input());
            for kind in kinds(&publisher.poll(&session.state, k as f64 / 120.0)) {
                *counts.entry(kind).or_insert(0) += 1;
            }
        }
        assert!((59..=61).contains(&counts["fast"]), "{counts:?}");
        assert!((9..=11).contains(&counts["standings"]), "{counts:?}");
        assert!((1..=3).contains(&counts["slow"]), "{counts:?}");
        assert!(
            !counts.contains_key("track") && !counts.contains_key("entries"),
            "{counts:?}"
        );
    }

    #[test]
    fn track_frame_is_sent_when_only_sector_boundaries_change() {
        let (_, mut session) = demo::warmed_up();
        let mut publisher = Publisher::new();
        publisher.poll(&session.state, 0.0);
        session.state.track.sectors = vec![0.0, 0.31, 0.69];
        let frames = publisher.poll(&session.state, 0.01);
        assert!(frames.iter().any(
            |frame| matches!(frame, Frame::Track(track) if track.sectors == [0.0, 0.31, 0.69])
        ));
    }

    #[test]
    fn track_frame_is_sent_for_revision_and_topology_changes() {
        let (_, mut session) = demo::warmed_up();
        let mut publisher = Publisher::new();
        publisher.poll(&session.state, 0.0);
        assert!(!kinds(&publisher.poll(&session.state, 0.01)).contains(&"track"));
        session.state.track.geometry_revision += 1;
        assert!(kinds(&publisher.poll(&session.state, 0.02)).contains(&"track"));
        assert!(!kinds(&publisher.poll(&session.state, 0.03)).contains(&"track"));
        session.state.track.closed = false;
        assert!(kinds(&publisher.poll(&session.state, 0.04)).contains(&"track"));
    }

    #[test]
    fn frames_serialize_tagged_and_camel_case() {
        let (_, session) = demo::warmed_up();
        let json = serde_json::to_string(&Publisher::full(&session.state)).unwrap();
        assert!(json.contains(r#""kind":"fast""#));
        assert!(json.contains(r#""relLateral""#));
        // the fast frame stays small: it is sent 60 times a second
        let fast = serde_json::to_string(&Frame::Fast(super::fast(&session.state))).unwrap();
        assert!(fast.len() < 6_000, "fast frame is {} bytes", fast.len());
    }

    #[test]
    fn slip_angles_and_balance_are_sent_on_fast_only() {
        let (_, mut session) = demo::warmed_up();
        session.state.player.slip_angles_deg = Some([1.0, 2.0, 3.0, 4.0]);
        session.state.player.slip_balance_deg = Some(2.0);

        let fast = serde_json::to_value(Frame::Fast(super::fast(&session.state))).unwrap();
        assert_eq!(
            fast["player"]["slipAnglesDeg"],
            serde_json::json!([1.0, 2.0, 3.0, 4.0])
        );
        assert_eq!(fast["player"]["slipBalanceDeg"], 2.0);

        let slow = serde_json::to_value(super::slow(&session.state)).unwrap();
        assert!(slow.get("slipAnglesDeg").is_none());
        assert!(slow.get("slipBalanceDeg").is_none());
    }
}
