//! End-to-end tests of the ACC adapter with a scripted fake game: shared memory pages and
//! broadcast datagrams go in, the normalized input (and core's derived state) comes out.

use std::collections::VecDeque;

use super::pages::{Graphics, Physics, Static};
use super::*;
use crate::core::Session;

/// Plays scripted raw data; records what the adapter sends.
#[derive(Default)]
struct Fake {
    queue: VecDeque<Raw>,
    clock: f64,
    sent: Vec<Vec<u8>>,
    connected: bool,
}

impl Source for Fake {
    fn drain(&mut self, out: &mut Vec<Raw>) {
        out.extend(self.queue.drain(..));
    }
    fn send(&mut self, d: &[u8]) {
        self.sent.push(d.to_vec());
    }
    fn clock(&self) -> f64 {
        self.clock
    }
    fn connected(&self) -> bool {
        self.connected
    }
}

fn bytes<T: Copy>(v: &T) -> Vec<u8> {
    // SAFETY: the page structs are plain #[repr(C)] numbers; viewing them as bytes is fine.
    unsafe {
        std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()).to_vec()
    }
}

fn zeroed<T: Copy>() -> T {
    // SAFETY: all-zero is a valid value for every page field (numbers and u16 strings).
    unsafe { std::mem::zeroed() }
}

fn wide<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (o, c) in out.iter_mut().zip(s.encode_utf16()) {
        *o = c;
    }
    out
}

/// Minimal writer for inbound broadcast packets.
#[derive(Default)]
struct Pkt(Vec<u8>);
impl Pkt {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(mut self, v: i32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(mut self, v: f32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn s(mut self, v: &str) -> Self {
        self.0.extend_from_slice(&(v.len() as u16).to_le_bytes());
        self.0.extend_from_slice(v.as_bytes());
        self
    }
    fn lap(self, ms: i32) -> Self {
        self.i32(ms).u16(0).u16(0).u8(0).u8(0).u8(1).u8(0).u8(0)
    }
}

fn entry_car(id: u16, model: u8, number: i32, first: &str, last: &str, cat: u8) -> Raw {
    Raw::Udp(
        Pkt::default()
            .u8(6)
            .u16(id)
            .u8(model)
            .s("Team")
            .i32(number)
            .u8(0)
            .u8(0)
            .u16(1)
            .u8(1)
            .s(first)
            .s(last)
            .s("XXX")
            .u8(cat)
            .u16(1)
            .0,
    )
}

fn car_update(id: u16, pos: u16, spline: f32, laps: u16, last_ms: i32, pit: bool) -> Raw {
    Raw::Udp(
        Pkt::default()
            .u8(3)
            .u16(id)
            .u16(0)
            .u8(1)
            .u8(5)
            .f32(0.0)
            .f32(0.0)
            .f32(0.0)
            .u8(if pit { 2 } else { 1 })
            .u16(180)
            .u16(pos)
            .u16(pos)
            .u16(pos)
            .f32(spline)
            .u16(laps)
            .i32(0)
            .lap(i32::MAX)
            .lap(last_ms)
            .lap(i32::MAX)
            .0,
    )
}

const RADIUS: f32 = 500.0;

/// A session at a circular test track: the player (car id 1) drives laps of `lap` seconds.
struct Script {
    acc: Acc<Fake>,
    phys: Physics,
    gfx: Graphics,
    t: f64,
    radius: f32,
}

impl Script {
    fn new() -> Self {
        let mut st: Static = zeroed();
        st.track = wide("test_ring");
        st.car_model = wide("ferrari_296_gt3");
        st.player_name = wide("Sven");
        st.player_surname = wide("Varga");
        st.sector_count = 3;
        st.max_rpm = 8000;
        st.max_fuel = 120.0;
        st.track_spline_length = 2.0 * std::f32::consts::PI * RADIUS;
        let mut gfx: Graphics = zeroed();
        gfx.status = 2;
        gfx.session = 2;
        gfx.player_car_id = 1;
        gfx.active_cars = 2;
        gfx.car_id[0] = 1;
        gfx.car_id[1] = 7;
        gfx.is_valid_lap = 1;
        gfx.session_time_left = 1_800_000.0;
        gfx.tyre_compound = wide("dry_compound");
        gfx.tc = 3;
        gfx.engine_map = 0;
        let mut phys: Physics = zeroed();
        phys.fuel = 50.0;
        phys.gear = 4; // third gear
        phys.brake_bias = 0.562;
        let fake = Fake {
            connected: true,
            ..Default::default()
        };
        let mut acc = Acc::new(fake, Options::default());
        acc.source_mut().queue.push_back(Raw::Static(bytes(&st)));
        Self {
            acc,
            phys,
            gfx,
            t: 0.0,
            radius: RADIUS,
        }
    }

    /// Advance one step: the player moves to `pct` of lap `laps`.
    fn step(
        &mut self,
        dt: f64,
        laps: i32,
        pct: f32,
        lap_time: f64,
        extra: Vec<Raw>,
    ) -> AdapterEvent {
        self.t += dt;
        let a = pct * std::f32::consts::TAU;
        self.phys.packet_id += 1;
        self.phys.speed_kmh = 180.0;
        self.phys.fuel -= 0.001;
        self.gfx.packet_id += 1;
        self.gfx.completed_laps = laps;
        self.gfx.normalized_car_position = pct;
        self.gfx.i_current_time = (lap_time * 1000.0) as i32;
        self.gfx.current_sector_index = (pct * 3.0).min(2.0) as i32;
        self.gfx.session_time_left -= (dt * 1000.0) as f32;
        self.gfx.car_coordinates[0] = [a.cos() * self.radius, 0.0, a.sin() * self.radius];
        // car 7 drives 20 m behind on the inside
        let b = a - 20.0 / RADIUS;
        self.gfx.car_coordinates[1] = [b.cos() * (RADIUS - 3.0), 0.0, b.sin() * (RADIUS - 3.0)];
        let src = self.acc.source_mut();
        src.clock = self.t;
        src.queue.push_back(Raw::Physics(bytes(&self.phys)));
        src.queue.push_back(Raw::Graphics(bytes(&self.gfx)));
        src.queue.extend(extra);
        self.acc.poll()
    }
}

#[test]
fn waits_for_the_game_then_connects() {
    let mut g = Script::new();
    g.acc.source_mut().connected = false;
    assert_eq!(g.step(0.01, 0, 0.1, 1.0, vec![]), AdapterEvent::Idle);
    g.acc.source_mut().connected = true;
    assert_eq!(
        g.step(0.01, 0, 0.1, 1.0, vec![]),
        AdapterEvent::Connection(Connection::Connected)
    );
    assert_eq!(g.step(0.01, 0, 0.1, 1.0, vec![]), AdapterEvent::Updated);
    // shared memory stops updating: the game was closed
    g.t += 5.0;
    g.acc.source_mut().clock = g.t;
    assert_eq!(
        g.acc.poll(),
        AdapterEvent::Connection(Connection::WaitingForGame)
    );
}

#[test]
fn player_only_without_broadcast() {
    let mut g = Script::new();
    for _ in 0..3 {
        g.step(0.01, 2, 0.4, 40.0, vec![]);
    }
    let inp = g.acc.input();
    assert_eq!(inp.cars.len(), 1);
    let p = &inp.player;
    assert_eq!(inp.cars[p.car].driver, "Sven Varga");
    assert_eq!(inp.track.name, "Test Ring");
    assert_eq!((p.gear, p.tc, p.map), (3, 3, 1));
    assert!(
        (p.bb - 51.2).abs() < 1e-3,
        "raw 56.2, Ferrari 296 offset -5: {}",
        p.bb
    );
    assert_eq!(p.tyres[0].compound, "Dry");
    assert!(p.tyres[0].i.is_none() && p.tyres[0].wear.is_none());
    assert_eq!(inp.session.kind, SessionKind::Race);
    assert_eq!(p.sector, 1);
}

#[test]
fn normalizes_all_four_slip_angles_and_rejects_nonfinite_samples() {
    let mut g = Script::new();
    g.phys.slip_angle = [0.1, -0.2, 0.3, -0.4];
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    let angles = g.acc.input().player.slip_angles_deg.unwrap();
    let expected = [0.1f32, -0.2, 0.3, -0.4].map(f32::to_degrees);
    for (actual, expected) in angles.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }

    g.phys.slip_angle[2] = f32::NAN;
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    assert_eq!(g.acc.input().player.slip_angles_deg, None);
}

#[test]
fn registers_and_merges_the_broadcast_field() {
    let mut g = Script::new();
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    // the adapter asked to register with the default password
    let sent = &g.acc.source().sent;
    assert_eq!(sent[0][..2], [1, 4]);
    let reg = Raw::Udp(Pkt::default().u8(1).i32(5).u8(1).u8(0).s("").0);
    g.step(0.01, 0, 0.1, 1.0, vec![reg]);
    let sent = &g.acc.source().sent;
    assert!(
        sent.iter().any(|d| d == &broadcast::request_entry_list(5)),
        "requests the entry list"
    );

    let field = vec![
        Raw::Udp(Pkt::default().u8(4).i32(5).u16(2).u16(1).u16(7).0),
        entry_car(1, 32, 46, "Sven", "Varga", 1),
        entry_car(7, 61, 12, "Lena", "Moreau", 2),
        Raw::Udp(
            Pkt::default()
                .u8(5)
                .i32(5)
                .s("test_ring")
                .i32(1)
                .i32(3142)
                .u8(0)
                .u8(0)
                .0,
        ),
        car_update(1, 1, 0.1, 0, i32::MAX, false),
        car_update(7, 2, 0.094, 0, i32::MAX, false),
    ];
    g.step(0.01, 0, 0.1, 1.0, field);
    let inp = g.acc.input();
    assert_eq!(inp.cars.len(), 2);
    let me = &inp.cars[inp.player.car];
    let other = inp.cars.iter().find(|c| !c.is_player).unwrap();
    assert_eq!(
        (me.number, me.cls.as_str(), me.model.as_str()),
        (46, "GT3", "Ferrari 296 GT3")
    );
    assert_eq!(
        (other.short.as_str(), other.cls.as_str(), other.cat.as_str()),
        ("L. Moreau", "GT4", "G")
    );
    assert_eq!(inp.classes, ["GT3", "GT4"]);
    assert_eq!(inp.track.length, 3142.0);

    // core: the other car is ~20 m behind and on the player's side
    let mut session = Session::new(Game::Acc, g.acc.capabilities());
    session.update(g.acc.input());
    let o = session.state.cars.iter().find(|c| !c.is_player).unwrap();
    assert!(
        o.rel_meters < -5.0 && o.rel_meters > -40.0,
        "rel {} m",
        o.rel_meters
    );
    assert_eq!(o.class_pos, 1);
}

#[test]
fn late_registration_answers_keep_one_connection() {
    let mut g = Script::new();
    let registers = |g: &Script| g.acc.source().sent.iter().filter(|d| d[0] == 1).count();
    // a loading session: ACC answers nothing for a while
    for _ in 0..250 {
        g.step(0.1, 0, 0.1, 1.0, vec![]);
    }
    assert_eq!(registers(&g), 3, "retries every 10 s, not faster");
    // then it answers every queued request
    let reg = |id: i32| Raw::Udp(Pkt::default().u8(1).i32(id).u8(1).u8(0).s("").0);
    g.step(0.1, 0, 0.1, 1.0, vec![reg(5), reg(6), reg(7)]);
    let sent = &g.acc.source().sent;
    assert!(sent.contains(&broadcast::request_entry_list(5)));
    assert!(sent.contains(&broadcast::unregister(6)) && sent.contains(&broadcast::unregister(7)));
    assert!(!sent.contains(&broadcast::request_entry_list(6)));
    assert!(g.acc.broadcast_registered());
}

#[test]
fn shifts_do_not_flash_neutral() {
    let mut g = Script::new();
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    assert_eq!(g.acc.input().player.gear, 3);
    g.phys.gear = 1; // neutral mid-shift, ~70 ms
    for _ in 0..7 {
        g.step(0.01, 0, 0.1, 1.0, vec![]);
        assert_eq!(g.acc.input().player.gear, 3);
    }
    g.phys.gear = 5;
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    assert_eq!(g.acc.input().player.gear, 4);
    // a real neutral shows after a moment
    g.phys.gear = 1;
    for _ in 0..20 {
        g.step(0.01, 0, 0.1, 1.0, vec![]);
    }
    assert_eq!(g.acc.input().player.gear, 0);
}

#[test]
fn global_flags_drive_the_session_flag() {
    let mut g = Script::new();
    let mut flag = |set: &dyn Fn(&mut Graphics)| {
        set(&mut g.gfx);
        g.step(0.01, 1, 0.2, 10.0, vec![]);
        let s = &g.acc.input().session;
        (s.flag, s.yellow_sector)
    };
    assert_eq!(flag(&|x| x.global_red = 1), (Flag::None, 0));
    assert_eq!(
        flag(&|x| {
            x.global_red = 0;
            x.global_green = 1;
            x.flag = 7;
        }),
        (Flag::Green, 0)
    );
    assert_eq!(flag(&|x| x.flag = 0), (Flag::Green, 0));
    assert_eq!(
        flag(&|x| {
            x.global_yellow = 1;
            x.global_yellow2 = 1;
        }),
        (Flag::Yellow, 2)
    );
    assert_eq!(
        flag(&|x| x.flag = 1),
        (Flag::Blue, 0),
        "the car's own flag first"
    );
    assert_eq!(
        flag(&|x| {
            x.flag = 0;
            x.global_yellow = 0;
            x.global_yellow2 = 0;
            x.global_white = 1;
        }),
        (Flag::White, 0)
    );
    assert_eq!(flag(&|x| x.global_chequered = 1), (Flag::Chequered, 0));
}

#[test]
fn sessions_without_a_time_limit() {
    let mut g = Script::new();
    g.gfx.session = 0;
    g.gfx.session_time_left = -1.0;
    for _ in 0..100 {
        g.step(0.1, 0, 0.1, 1.0, vec![]);
        g.gfx.session_time_left = -1.0;
    }
    let s = &g.acc.input().session;
    assert_eq!(s.duration, 0.0);
    // counted from the first update (the first poll only reports the connection)
    assert!((s.elapsed - 9.8).abs() < 0.05, "elapsed {}", s.elapsed);
}

/// Drive `laps` laps of 60 s in steps, feeding the core.
fn drive(g: &mut Script, session: &mut Session, laps: i32) {
    let steps = 400;
    for lap in 0..laps {
        for k in 0..steps {
            let pct = k as f32 / steps as f32;
            if lap > 0 && k == 0 {
                g.gfx.i_last_time = 60_000 + lap * 250;
            }
            if g.step(60.0 / steps as f64, lap, pct, 60.0 * pct as f64, vec![])
                == AdapterEvent::Updated
            {
                session.update(g.acc.input());
            }
        }
    }
}

#[test]
fn standing_start_from_behind_the_line_splits_the_first_lap() {
    let mut g = Script::new();
    let mut session = Session::new(Game::Acc, g.acc.capabilities());
    let mut feed = |g: &mut Script, laps: i32, pct: f32, lap_time: f64| {
        if g.step(0.15, laps, pct, lap_time, vec![]) == AdapterEvent::Updated {
            session.update(g.acc.input());
        }
    };
    // on the grid: last sector, no lap time yet
    for _ in 0..20 {
        feed(&mut g, 0, 0.97, 0.0);
    }
    for k in 0..400 {
        let pct = k as f32 / 400.0;
        feed(&mut g, 0, pct, 60.0 * pct as f64);
    }
    g.gfx.i_last_time = 60_000;
    feed(&mut g, 1, 0.0, 0.0);
    let log = &session.state.player.log;
    assert_eq!(log.len(), 1);
    assert!(
        (log[0].sectors[0] - 20.0).abs() < 0.5,
        "{:?}",
        log[0].sectors
    );
    assert!(
        (log[0].sectors[1] - 20.0).abs() < 0.5,
        "{:?}",
        log[0].sectors
    );
}

#[test]
fn invalid_laps_still_teach_the_track() {
    let mut g = Script::new();
    g.gfx.is_valid_lap = 0;
    let mut session = Session::new(Game::Acc, g.acc.capabilities());
    drive(&mut g, &mut session, 2);
    assert_eq!(session.state.track.x.len(), 1000);
}

#[test]
fn laps_feed_the_core_and_teach_the_track() {
    let mut g = Script::new();
    let mut session = Session::new(Game::Acc, g.acc.capabilities());
    let steps = 400;
    for lap in 0..3 {
        for k in 0..steps {
            let pct = k as f32 / steps as f32;
            if lap > 0 && k == 0 {
                g.gfx.i_last_time = 60_000 + lap * 250;
            }
            if g.step(60.0 / steps as f64, lap, pct, 60.0 * pct as f64, vec![])
                == AdapterEvent::Updated
            {
                session.update(g.acc.input());
            }
        }
    }
    let st = &session.state;
    assert_eq!(st.player.log.len(), 2, "two laps completed");
    assert_eq!(st.player.lap.best, 60.25);
    // the first full lap taught the outline: a circle of the test radius
    assert_eq!(st.track.x.len(), 1000);
    let r = st.track.x[100].hypot(st.track.y[100]);
    assert!((r - RADIUS).abs() < 3.0, "outline radius {r}");
    assert!(
        (st.track.sectors[1] - 1.0 / 3.0).abs() < 0.01,
        "{:?}",
        st.track.sectors
    );
}

#[test]
fn same_track_session_promotes_consensus_without_moving_sectors() {
    let mut game = Script::new();
    for lap in 0..5 {
        game.radius = if lap == 0 { 500.0 } else { 520.0 };
        for step in 0..400 {
            let pct = step as f32 / 400.0;
            game.step(0.15, lap, pct, 60.0 * pct as f64, vec![]);
        }
    }
    let old = game.acc.input().track.clone();
    assert_eq!(old.geometry_revision, 1);
    assert!((old.x[250].hypot(old.y[250]) - 500.0).abs() < 3.0);
    game.gfx.session_index += 1;
    game.gfx.session = 1;
    game.radius = 520.0;
    game.step(0.15, 0, 0.0, 0.0, vec![]);
    let promoted = &game.acc.input().track;
    assert_eq!(promoted.geometry_revision, 2);
    assert!((promoted.x[250].hypot(promoted.y[250]) - 520.0).abs() < 3.0);
    assert_eq!(promoted.sectors, old.sectors);
}

#[test]
fn cached_history_promotes_on_a_fresh_adapter_without_changing_sectors() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("opensimhud-acc-session-{nonce}"));
    let mut first = Script::new();
    first.acc.opts.track_cache = Some(dir.clone());
    first.acc.cache_writer = CacheWriter::new(dir.clone());
    for lap in 0..5 {
        first.radius = if lap == 0 { 500.0 } else { 520.0 };
        for step in 0..400 {
            let pct = step as f32 / 400.0;
            first.step(0.15, lap, pct, 60.0 * pct as f64, vec![]);
        }
    }
    let original = first.acc.input().track.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let ready =
            AccLearnedFile::load(&dir, &original.name, original.length).is_some_and(|state| {
                let reference = OutlineCandidate {
                    track_key: state.track_name.clone(),
                    topology: RouteTopology::Closed,
                    domain: SamplingDomain::GameProgress,
                    valid: true,
                    session_id: 0,
                    points: state
                        .track
                        .x
                        .iter()
                        .copied()
                        .zip(state.track.y.iter().copied())
                        .collect(),
                };
                state.candidate_pool.select(&reference, true).is_some()
            });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "candidate history was not saved"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(first);
    let mut second = Script::new();
    second.acc.opts.track_cache = Some(dir.clone());
    second.acc.cache_writer = CacheWriter::new(dir.clone());
    second.step(0.15, 0, 0.0, 0.0, vec![]);
    second.step(0.15, 0, 0.001, 0.1, vec![]);
    let selected = second.acc.input().track.clone();
    assert_eq!(selected.geometry_revision, 2);
    assert!((selected.x[250].hypot(selected.y[250]) - 520.0).abs() < 3.0);
    assert_eq!(selected.sectors, original.sectors);
    drop(second);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while AccLearnedFile::load(&dir, &original.name, original.length)
        .is_none_or(|state| state.track.geometry_revision != 2)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "promoted cache was not saved"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recordings_replay_to_the_same_input() {
    use super::recording::{Reader, Recording, Replay, Writer};
    let mut buf = Vec::new();
    {
        let mut g = Script::new();
        let src = std::mem::take(g.acc.source_mut());
        let mut rec = Acc::new(
            Recording::new(src, Writer::new(&mut buf).unwrap()),
            Options::default(),
        );
        // replay the same script through the recording wrapper
        for k in 0..50 {
            g.t += 0.02;
            g.phys.packet_id += 1;
            g.gfx.packet_id += 1;
            g.gfx.normalized_car_position = k as f32 / 100.0;
            let s = &mut rec.source_mut().inner;
            s.connected = true;
            s.clock = g.t;
            s.queue.push_back(Raw::Physics(bytes(&g.phys)));
            s.queue.push_back(Raw::Graphics(bytes(&g.gfx)));
            rec.poll();
        }
        rec.source_mut().finish().unwrap();
        assert_eq!(rec.input().cars[0].driver, "Sven Varga");
    }
    let mut replay = Acc::new(
        Replay::stepped(Reader::new(buf.as_slice()).unwrap(), 0.02),
        Options::default(),
    );
    let mut updated = 0;
    while !replay.source().finished {
        if replay.poll() == AdapterEvent::Updated {
            updated += 1;
        }
    }
    assert!(updated > 40, "{updated} updates replayed");
    let inp = replay.input();
    assert_eq!(inp.cars[inp.player.car].driver, "Sven Varga");
    assert!((inp.cars[0].pct - 0.49).abs() < 1e-6);
}

/// Registered, with the player (1) and cars 7 and 9 on the entry list.
fn with_field() -> Script {
    let mut g = Script::new();
    g.step(0.01, 0, 0.1, 1.0, vec![]);
    let reg = Raw::Udp(Pkt::default().u8(1).i32(5).u8(1).u8(0).s("").0);
    g.step(0.01, 0, 0.1, 1.0, vec![reg]);
    let field = vec![
        Raw::Udp(Pkt::default().u8(4).i32(5).u16(3).u16(1).u16(7).u16(9).0),
        entry_car(1, 32, 46, "Sven", "Varga", 1),
        entry_car(7, 61, 12, "Lena", "Moreau", 2),
        entry_car(9, 32, 3, "Ola", "Berg", 1),
    ];
    g.step(0.01, 0, 0.1, 1.0, field);
    g
}

fn drivers(g: &Script) -> Vec<&str> {
    g.acc
        .input()
        .cars
        .iter()
        .map(|c| c.driver.as_str())
        .collect()
}

#[test]
fn cars_not_out_yet_count_as_in_the_pits_and_off_the_map() {
    let mut g = with_field();
    g.step(
        0.01,
        0,
        0.1,
        1.0,
        vec![car_update(7, 2, 0.094, 0, i32::MAX, false)],
    );
    let inp = g.acc.input();
    let car = |n: &str| inp.cars.iter().find(|c| c.driver == n).unwrap();
    assert!(car("Lena Moreau").located && !car("Lena Moreau").in_pit);
    assert!(!car("Ola Berg").located && car("Ola Berg").in_pit);
}

#[test]
fn cars_that_leave_the_server_go_and_keep_their_history_if_they_return() {
    let mut g = with_field();
    let mut session = Session::new(Game::Acc, g.acc.capabilities());
    let updates = |nine: bool, laps: u16, last: i32| {
        let mut v = vec![car_update(7, 2, 0.5, laps, last, false)];
        if nine {
            v.push(car_update(9, 3, 0.4, laps, last, false));
        }
        v
    };
    g.step(0.1, 0, 0.5, 10.0, updates(true, 0, i32::MAX));
    session.update(g.acc.input());
    g.step(0.1, 0, 0.5, 10.0, updates(true, 1, 90_000));
    session.update(g.acc.input());
    let berg = |s: &Session| {
        s.state
            .cars
            .iter()
            .find(|c| c.driver == "Ola Berg")
            .cloned()
    };
    assert_eq!(berg(&session).unwrap().best_lap, 90.0);

    // car 9 goes quiet while 7 keeps reporting
    for _ in 0..105 {
        g.step(0.1, 0, 0.5, 10.0, updates(false, 1, 90_000));
        session.update(g.acc.input());
    }
    assert_eq!(drivers(&g), ["Sven Varga", "Lena Moreau"]);
    assert!(berg(&session).is_none());

    // it comes back: the entry list names it again, its best lap is still known
    g.step(0.1, 0, 0.5, 10.0, updates(true, 1, 90_000));
    let list = Raw::Udp(Pkt::default().u8(4).i32(5).u16(3).u16(1).u16(7).u16(9).0);
    g.step(
        0.1,
        0,
        0.5,
        10.0,
        vec![list, entry_car(9, 32, 3, "Ola", "Berg", 1)],
    );
    session.update(g.acc.input());
    assert_eq!(berg(&session).unwrap().best_lap, 90.0);
}

#[test]
fn the_entry_list_drops_cars_no_longer_on_it() {
    let mut g = with_field();
    let list = Raw::Udp(Pkt::default().u8(4).i32(5).u16(2).u16(1).u16(9).0);
    g.step(0.01, 0, 0.1, 1.0, vec![list]);
    assert_eq!(drivers(&g), ["Sven Varga", "Ola Berg"]);
}

#[test]
fn back_from_the_menu_forgets_the_old_lobby() {
    let mut g = with_field();
    assert_eq!(g.acc.input().cars.len(), 3);
    // leave to the menu, join another server
    g.gfx.status = 0;
    assert_eq!(
        g.step(0.1, 0, 0.1, 1.0, vec![]),
        AdapterEvent::Connection(Connection::WaitingForGame)
    );
    g.gfx.status = 2;
    assert_eq!(
        g.step(0.1, 0, 0.1, 1.0, vec![]),
        AdapterEvent::Connection(Connection::Connected)
    );
    g.step(0.1, 0, 0.1, 1.0, vec![]);
    assert_eq!(drivers(&g), ["Sven Varga"]);
    let sent = &g.acc.source().sent;
    assert_eq!(
        sent.last().unwrap(),
        &broadcast::request_entry_list(5),
        "asks for the new entry list"
    );
}
