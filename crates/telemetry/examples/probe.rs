//! Print the raw ACC data in a recording, for checking the decoding against the game.
//!
//! cargo run --release -p telemetry --example probe -- FILE [EVERY_SECONDS]

use std::collections::BTreeMap;
use std::fs::File;

use telemetry::adapters::acc::Raw;
use telemetry::adapters::acc::broadcast::{self, Message};
use telemetry::adapters::acc::pages::{Graphics, Physics, Static, wstr};
use telemetry::adapters::acc::recording::Reader;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: probe FILE [EVERY_SECONDS]");
    let every: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let mut reader = Reader::new(File::open(path).unwrap()).unwrap();

    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    let mut udp_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut physics: Option<Physics> = None;
    let mut graphics: Option<Graphics> = None;
    let mut statics_seen = 0;
    let mut next = 0.0;
    let mut shown_entries = 0;
    let mut last_valid = None;
    let mut last_laps = None;
    let mut last_flags = None;
    let mut last_t = 0.0;
    while let Some((t, raw)) = reader.next_record() {
        last_t = t;
        match raw {
            Raw::Physics(b) => {
                *counts.entry("physics").or_default() += 1;
                physics = Physics::from_bytes(&b);
            }
            Raw::Graphics(b) => {
                *counts.entry("graphics").or_default() += 1;
                graphics = Graphics::from_bytes(&b);
                if let Some(g) = &graphics {
                    let flags = (
                        g.flag,
                        g.penalty,
                        g.global_yellow,
                        g.global_yellow1,
                        g.global_yellow2,
                        g.global_yellow3,
                        g.global_white,
                        g.global_green,
                        g.global_chequered,
                        g.global_red,
                    );
                    if last_flags != Some(flags) {
                        println!(
                            "  [{t:8.2}] FLAGS flag {} pen {} yellow {} ({} {} {}) white {} green {} chequered {} red {}",
                            flags.0,
                            flags.1,
                            flags.2,
                            flags.3,
                            flags.4,
                            flags.5,
                            flags.6,
                            flags.7,
                            flags.8,
                            flags.9
                        );
                        last_flags = Some(flags);
                    }
                    if last_valid != Some(g.is_valid_lap) || last_laps != Some(g.completed_laps) {
                        println!(
                            "  [{t:8.2}] laps {} valid {} cur {} last {} best {} sector {} pct {:.3} pit {}/{} status {}",
                            g.completed_laps,
                            g.is_valid_lap,
                            g.i_current_time,
                            g.i_last_time,
                            g.i_best_time,
                            g.current_sector_index,
                            g.normalized_car_position,
                            g.is_in_pit,
                            g.is_in_pit_lane,
                            g.status
                        );
                        last_valid = Some(g.is_valid_lap);
                        last_laps = Some(g.completed_laps);
                    }
                }
            }
            Raw::Static(b) => {
                *counts.entry("static").or_default() += 1;
                if let Some(s) = Static::from_bytes(&b)
                    && statics_seen < 3
                {
                    statics_seen += 1;
                    println!(
                        "[{t:8.2}] STATIC sm {} ac {} sessions {} cars {} car {:?} track {:?} player {:?} {:?} sectors {} maxrpm {} maxfuel {} spline {} pitwin {}..{} online {} timed {} dry {:?} wet {:?} cfg {:?}",
                        wstr(&s.sm_version),
                        wstr(&s.ac_version),
                        s.number_of_sessions,
                        s.num_cars,
                        wstr(&s.car_model),
                        wstr(&s.track),
                        wstr(&s.player_name),
                        wstr(&s.player_surname),
                        s.sector_count,
                        s.max_rpm,
                        s.max_fuel,
                        s.track_spline_length,
                        s.pit_window_start,
                        s.pit_window_end,
                        s.is_online,
                        s.is_timed_race,
                        wstr(&s.dry_tyres_name),
                        wstr(&s.wet_tyres_name),
                        wstr(&s.track_configuration),
                    );
                }
            }
            Raw::Udp(b) => {
                *counts.entry("udp").or_default() += 1;
                match broadcast::decode(&b) {
                    Ok(m) => {
                        let k = match &m {
                            Message::Registration { .. } => "registration",
                            Message::Realtime(_) => "realtime",
                            Message::Car(_) => "car",
                            Message::EntryList(_) => "entrylist",
                            Message::EntryCar(_) => "entrycar",
                            Message::Track(_) => "track",
                            Message::Event(_) => "event",
                        };
                        *udp_kinds.entry(k.into()).or_default() += 1;
                        match &m {
                            Message::Registration { .. }
                            | Message::Track(_)
                            | Message::EntryList(_)
                            | Message::Event(_) => println!("[{t:8.2}] UDP {m:?}"),
                            Message::EntryCar(e) if shown_entries < 40 => {
                                shown_entries += 1;
                                let d = e.drivers.first();
                                println!(
                                    "[{t:8.2}] ENTRY car {} model {} #{} team {:?} cup {} driver {:?} {:?} cat {:?} nat {}",
                                    e.car,
                                    e.model,
                                    e.race_number,
                                    e.team,
                                    e.cup_category,
                                    d.map(|d| &d.first_name),
                                    d.map(|d| &d.last_name),
                                    d.map(|d| d.category),
                                    e.nationality
                                );
                            }
                            Message::Realtime(r) if t >= next - 0.05 => println!(
                                "[{t:8.2}] RT session {} type {} phase {} time {:.0} end {:.0} tod {:.0} amb {} trk {} clouds {:.2} rain {:.2} wet {:.2} focused {} replay {:?} best {:?}",
                                r.session_index,
                                r.session_type,
                                r.phase,
                                r.session_time,
                                r.session_end_time,
                                r.time_of_day,
                                r.ambient_temp,
                                r.track_temp,
                                r.clouds,
                                r.rain_level,
                                r.wetness,
                                r.focused_car,
                                r.replay,
                                r.best_session_lap.time_ms
                            ),
                            Message::Car(c) if t >= next - 0.05 => println!(
                                "[{t:8.2}] CAR {} pos {} cup {} trk {} laps {} spline {:.3} kmh {} gear {} loc {} xy ({:.1},{:.1}) yaw {:.2} delta {} last {:?} inv {} best {:?} cur {:?} curinv {} out {} in {}",
                                c.car,
                                c.position,
                                c.cup_position,
                                c.track_position,
                                c.laps,
                                c.spline,
                                c.kmh,
                                c.gear,
                                c.location.0,
                                c.x,
                                c.y,
                                c.yaw,
                                c.delta,
                                c.last_lap.time_ms,
                                c.last_lap.invalid,
                                c.best_session_lap.time_ms,
                                c.current_lap.time_ms,
                                c.current_lap.invalid,
                                c.current_lap.out_lap,
                                c.current_lap.in_lap
                            ),
                            _ => {}
                        }
                    }
                    Err(e) => {
                        *udp_kinds
                            .entry(format!(
                                "error {e} (type {})",
                                b.first().copied().unwrap_or(0)
                            ))
                            .or_default() += 1
                    }
                }
            }
        }
        if t >= next {
            next = t + every;
            if let (Some(p), Some(g)) = (&physics, &graphics) {
                println!(
                    "[{t:8.2}] PHYS id {} gear {} rpm {} kmh {:.1} fuel {:.2} gas {:.2} brk {:.2} clutch {:.2} steer {:.2} bb {:.4} air {:.1} road {:.1} heading {:.2} press {:?} core {:?} brakeT {:?} tc {:.2} abs {:.2} pad {:?} disc {:?} curmaxrpm {} tcact {} absact {} ign {} run {}",
                    p.packet_id,
                    p.gear,
                    p.rpms,
                    p.speed_kmh,
                    p.fuel,
                    p.gas,
                    p.brake,
                    p.clutch,
                    p.steer_angle,
                    p.brake_bias,
                    p.air_temp,
                    p.road_temp,
                    p.heading,
                    p.wheels_pressure,
                    p.tyre_core_temperature,
                    p.brake_temp,
                    p.tc,
                    p.abs,
                    p.pad_life,
                    p.disc_life,
                    p.current_max_rpm,
                    p.tc_in_action,
                    p.abs_in_action,
                    p.ignition_on,
                    p.is_engine_running
                );
                let n = g.active_cars.clamp(0, 60) as usize;
                let me = (0..n).find(|&k| g.car_id[k] == g.player_car_id);
                println!(
                    "[{t:8.2}] GFX id {} status {} session {} laps {} pos {} cur {} last {} best {} left {:.0} dist {:.0} sector {} lastsec {} nlaps {} compound {:?} pct {:.4} active {} ids {:?} player {} me {:?} flag {} pen {} penT {:.1} pitlane {} grip {:.3} mand {} wind {:.2}/{:.2} tc {} tccut {} map {} abs {} fxl {:.3} used {:.2} sessidx {} delta {} est {} valid {} fuelest {:.2} trackstatus {:?} clock {:.0} gy {} {} {} {} gw {} gg {} gc {} gr {} gripst {} rain {} {} {} tyreset {} gap {} {} stint {} {} mfdfuel {:.1}",
                    g.packet_id,
                    g.status,
                    g.session,
                    g.completed_laps,
                    g.position,
                    g.i_current_time,
                    g.i_last_time,
                    g.i_best_time,
                    g.session_time_left,
                    g.distance_traveled,
                    g.current_sector_index,
                    g.last_sector_time,
                    g.number_of_laps,
                    wstr(&g.tyre_compound),
                    g.normalized_car_position,
                    g.active_cars,
                    &g.car_id[..n.min(8)],
                    g.player_car_id,
                    me.map(|k| g.car_coordinates[k]),
                    g.flag,
                    g.penalty,
                    g.penalty_time,
                    g.is_in_pit_lane,
                    g.surface_grip,
                    g.mandatory_pit_done,
                    g.wind_speed,
                    g.wind_direction,
                    g.tc,
                    g.tc_cut,
                    g.engine_map,
                    g.abs,
                    g.fuel_x_lap,
                    g.used_fuel,
                    g.session_index,
                    g.i_delta_lap_time,
                    g.i_estimated_lap_time,
                    g.is_valid_lap,
                    g.fuel_estimated_laps,
                    wstr(&g.track_status),
                    g.clock,
                    g.global_yellow,
                    g.global_yellow1,
                    g.global_yellow2,
                    g.global_yellow3,
                    g.global_white,
                    g.global_green,
                    g.global_chequered,
                    g.global_red,
                    g.track_grip_status,
                    g.rain_intensity,
                    g.rain_intensity_in_10min,
                    g.rain_intensity_in_30min,
                    g.current_tyre_set,
                    g.gap_ahead,
                    g.gap_behind,
                    g.driver_stint_total_time_left,
                    g.driver_stint_time_left,
                    g.mfd_fuel_to_add
                );
            }
        }
    }
    println!("\nduration {last_t:.1} s");
    println!("records {counts:?}");
    println!("udp {udp_kinds:?}");
}
