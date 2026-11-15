//! ACC shared memory pages (`Local\acpmf_physics`, `acpmf_graphics`, `acpmf_static`).
//!
//! Exact mirrors of Kunos' `SPageFilePhysics`, `SPageFileGraphic` and `SPageFileStatic`
//! (ACC 1.8+). `#[repr(C)]` reproduces the C layout, including the 2-byte padding after
//! odd-length UTF-16 strings; the tests pin the page sizes and the offsets of every field we
//! use. All fields are plain numbers, so any byte pattern is a valid value.

#![allow(dead_code)] // the full layouts are kept even where we don't read a field

/// Copy a page out of raw bytes (shared memory or a recording).
fn read<T: Copy>(bytes: &[u8]) -> Option<T> {
    if bytes.len() < std::mem::size_of::<T>() {
        return None;
    }
    // SAFETY: T is a #[repr(C)] struct of i32/f32/u16 fields, so every bit pattern is valid,
    // and the length was checked; read_unaligned copes with any alignment of `bytes`.
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

/// Decode a fixed-size, NUL-terminated UTF-16 string.
pub fn wstr(chars: &[u16]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Physics {
    pub packet_id: i32,
    pub gas: f32,
    pub brake: f32,
    pub fuel: f32,
    /// 0 = reverse, 1 = neutral, 2 = first gear, …
    pub gear: i32,
    pub rpms: i32,
    /// -1 (full left) .. 1 (full right).
    pub steer_angle: f32,
    pub speed_kmh: f32,
    pub velocity: [f32; 3],
    pub acc_g: [f32; 3],
    pub wheel_slip: [f32; 4],
    pub wheel_load: [f32; 4],
    /// psi, FL FR RL RR.
    pub wheels_pressure: [f32; 4],
    pub wheel_angular_speed: [f32; 4],
    pub tyre_wear: [f32; 4],
    pub tyre_dirty_level: [f32; 4],
    pub tyre_core_temperature: [f32; 4],
    pub camber_rad: [f32; 4],
    pub suspension_travel: [f32; 4],
    pub drs: f32,
    /// Traction control slip; > 0 while TC is cutting.
    pub tc: f32,
    /// Car yaw, radians.
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub cg_height: f32,
    pub car_damage: [f32; 5],
    pub number_of_tyres_out: i32,
    pub pit_limiter_on: i32,
    /// ABS slip; > 0 while ABS is working.
    pub abs: f32,
    pub kers_charge: f32,
    pub kers_input: f32,
    pub auto_shifter_on: i32,
    pub ride_height: [f32; 2],
    pub turbo_boost: f32,
    pub ballast: f32,
    pub air_density: f32,
    pub air_temp: f32,
    pub road_temp: f32,
    pub local_angular_vel: [f32; 3],
    pub final_ff: f32,
    pub performance_meter: f32,
    pub engine_brake: i32,
    pub ers_recovery_level: i32,
    pub ers_power_level: i32,
    pub ers_heat_charging: i32,
    pub ers_is_charging: i32,
    pub kers_current_kj: f32,
    pub drs_available: i32,
    pub drs_enabled: i32,
    /// °C, FL FR RL RR.
    pub brake_temp: [f32; 4],
    pub clutch: f32,
    pub tyre_temp_i: [f32; 4],
    pub tyre_temp_m: [f32; 4],
    pub tyre_temp_o: [f32; 4],
    pub is_ai_controlled: i32,
    pub tyre_contact_point: [[f32; 3]; 4],
    pub tyre_contact_normal: [[f32; 3]; 4],
    pub tyre_contact_heading: [[f32; 3]; 4],
    /// Raw front share 0..1; the in-car display adds a per-car offset.
    pub brake_bias: f32,
    pub local_velocity: [f32; 3],
    pub p2p_activations: i32,
    pub p2p_status: i32,
    pub current_max_rpm: i32,
    pub mz: [f32; 4],
    pub fz: [f32; 4],
    pub my: [f32; 4],
    pub slip_ratio: [f32; 4],
    /// Native signed tyre slip angles in radians, ordered FL, FR, RL, RR.
    pub slip_angle: [f32; 4],
    pub tc_in_action: i32,
    pub abs_in_action: i32,
    pub suspension_damage: [f32; 4],
    pub tyre_temp: [f32; 4],
    pub water_temp: f32,
    pub brake_pressure: [f32; 4],
    pub front_brake_compound: i32,
    pub rear_brake_compound: i32,
    pub pad_life: [f32; 4],
    pub disc_life: [f32; 4],
    pub ignition_on: i32,
    pub starter_engine_on: i32,
    pub is_engine_running: i32,
    pub kerb_vibration: f32,
    pub slip_vibrations: f32,
    pub g_vibrations: f32,
    pub abs_vibrations: f32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Graphics {
    pub packet_id: i32,
    /// 0 off, 1 replay, 2 live, 3 pause.
    pub status: i32,
    /// -1 unknown, 0 practice, 1 qualifying, 2 race, 3 hotlap, 4 time attack, 5 drift,
    /// 6 drag, 7 hotstint, 8 hotlap superpole.
    pub session: i32,
    pub current_time: [u16; 15],
    pub last_time: [u16; 15],
    pub best_time: [u16; 15],
    pub split: [u16; 15],
    pub completed_laps: i32,
    pub position: i32,
    /// Milliseconds.
    pub i_current_time: i32,
    pub i_last_time: i32,
    pub i_best_time: i32,
    /// Milliseconds.
    pub session_time_left: f32,
    pub distance_traveled: f32,
    pub is_in_pit: i32,
    pub current_sector_index: i32,
    pub last_sector_time: i32,
    pub number_of_laps: i32,
    pub tyre_compound: [u16; 33],
    pub replay_time_multiplier: f32,
    pub normalized_car_position: f32,
    pub active_cars: i32,
    /// World x, y (up), z of every car on track; matched to `car_id`.
    pub car_coordinates: [[f32; 3]; 60],
    pub car_id: [i32; 60],
    pub player_car_id: i32,
    pub penalty_time: f32,
    /// 0 none, 1 blue, 2 yellow, 3 black, 4 white, 5 chequered, 6 penalty, 7 green, 8 orange.
    pub flag: i32,
    pub penalty: i32,
    pub ideal_line_on: i32,
    pub is_in_pit_lane: i32,
    pub surface_grip: f32,
    pub mandatory_pit_done: i32,
    pub wind_speed: f32,
    pub wind_direction: f32,
    pub is_setup_menu_visible: i32,
    pub main_display_index: i32,
    pub secondary_display_index: i32,
    pub tc: i32,
    pub tc_cut: i32,
    /// 0-based; the car shows it 1-based.
    pub engine_map: i32,
    pub abs: i32,
    pub fuel_x_lap: f32,
    pub rain_lights: i32,
    pub flashing_lights: i32,
    pub lights_stage: i32,
    pub exhaust_temperature: f32,
    pub wiper_lv: i32,
    pub driver_stint_total_time_left: i32,
    pub driver_stint_time_left: i32,
    pub rain_tyres: i32,
    pub session_index: i32,
    pub used_fuel: f32,
    pub delta_lap_time: [u16; 15],
    pub i_delta_lap_time: i32,
    pub estimated_lap_time: [u16; 15],
    pub i_estimated_lap_time: i32,
    pub is_delta_positive: i32,
    pub i_split: i32,
    pub is_valid_lap: i32,
    pub fuel_estimated_laps: f32,
    pub track_status: [u16; 33],
    pub missing_mandatory_pits: i32,
    /// Time of day at the track, seconds since midnight.
    pub clock: f32,
    pub direction_lights_left: i32,
    pub direction_lights_right: i32,
    pub global_yellow: i32,
    pub global_yellow1: i32,
    pub global_yellow2: i32,
    pub global_yellow3: i32,
    pub global_white: i32,
    pub global_green: i32,
    pub global_chequered: i32,
    pub global_red: i32,
    pub mfd_tyre_set: i32,
    pub mfd_fuel_to_add: f32,
    pub mfd_tyre_pressure_lf: f32,
    pub mfd_tyre_pressure_rf: f32,
    pub mfd_tyre_pressure_lr: f32,
    pub mfd_tyre_pressure_rr: f32,
    /// 0 green, 1 fast, 2 optimum, 3 greasy, 4 damp, 5 wet, 6 flooded.
    pub track_grip_status: i32,
    /// 0 none, 1 drizzle, 2 light, 3 medium, 4 heavy, 5 thunderstorm.
    pub rain_intensity: i32,
    pub rain_intensity_in_10min: i32,
    pub rain_intensity_in_30min: i32,
    pub current_tyre_set: i32,
    pub strategy_tyre_set: i32,
    pub gap_ahead: i32,
    pub gap_behind: i32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Static {
    pub sm_version: [u16; 15],
    pub ac_version: [u16; 15],
    pub number_of_sessions: i32,
    pub num_cars: i32,
    pub car_model: [u16; 33],
    pub track: [u16; 33],
    pub player_name: [u16; 33],
    pub player_surname: [u16; 33],
    pub player_nick: [u16; 33],
    pub sector_count: i32,
    pub max_torque: f32,
    pub max_power: f32,
    pub max_rpm: i32,
    pub max_fuel: f32,
    pub suspension_max_travel: [f32; 4],
    pub tyre_radius: [f32; 4],
    pub max_turbo_boost: f32,
    pub deprecated_1: f32,
    pub deprecated_2: f32,
    pub penalties_enabled: i32,
    pub aid_fuel_rate: f32,
    pub aid_tire_rate: f32,
    pub aid_mechanical_damage: f32,
    pub aid_allow_tyre_blankets: f32,
    pub aid_stability: f32,
    pub aid_auto_clutch: i32,
    pub aid_auto_blip: i32,
    pub has_drs: i32,
    pub has_ers: i32,
    pub has_kers: i32,
    pub kers_max_j: f32,
    pub engine_brake_settings_count: i32,
    pub ers_power_controller_count: i32,
    pub track_spline_length: f32,
    pub track_configuration: [u16; 33],
    pub ers_max_j: f32,
    pub is_timed_race: i32,
    pub has_extra_lap: i32,
    pub car_skin: [u16; 33],
    pub reversed_grid_positions: i32,
    /// Pit window start / end, milliseconds of session time.
    pub pit_window_start: i32,
    pub pit_window_end: i32,
    pub is_online: i32,
    pub dry_tyres_name: [u16; 33],
    pub wet_tyres_name: [u16; 33],
}

pub const PHYSICS_SIZE: usize = std::mem::size_of::<Physics>();
pub const GRAPHICS_SIZE: usize = std::mem::size_of::<Graphics>();
pub const STATIC_SIZE: usize = std::mem::size_of::<Static>();

impl Physics {
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        read(b)
    }
}
impl Graphics {
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        read(b)
    }
}
impl Static {
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        read(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    // Sizes and offsets cross-checked against the field-by-field reader of PyAccSharedMemory
    // and Kunos' SharedFileOut.h.
    #[test]
    fn page_sizes_match_acc() {
        assert_eq!(PHYSICS_SIZE, 800);
        assert_eq!(GRAPHICS_SIZE, 1588);
        assert_eq!(STATIC_SIZE, 820);
    }

    #[test]
    fn physics_offsets() {
        assert_eq!(offset_of!(Physics, fuel), 12);
        assert_eq!(offset_of!(Physics, gear), 16);
        assert_eq!(offset_of!(Physics, speed_kmh), 28);
        assert_eq!(offset_of!(Physics, wheels_pressure), 88);
        assert_eq!(offset_of!(Physics, tyre_core_temperature), 152);
        assert_eq!(offset_of!(Physics, tc), 204);
        assert_eq!(offset_of!(Physics, abs), 252);
        assert_eq!(offset_of!(Physics, air_temp), 288);
        assert_eq!(offset_of!(Physics, road_temp), 292);
        assert_eq!(offset_of!(Physics, brake_temp), 348);
        assert_eq!(offset_of!(Physics, clutch), 364);
        assert_eq!(offset_of!(Physics, brake_bias), 564);
        assert_eq!(offset_of!(Physics, slip_angle), 656);
        assert_eq!(offset_of!(Physics, pad_life), 740);
        assert_eq!(offset_of!(Physics, disc_life), 756);
    }

    #[test]
    fn graphics_offsets() {
        assert_eq!(offset_of!(Graphics, completed_laps), 132);
        assert_eq!(offset_of!(Graphics, i_current_time), 140);
        assert_eq!(offset_of!(Graphics, session_time_left), 152);
        assert_eq!(offset_of!(Graphics, current_sector_index), 164);
        assert_eq!(offset_of!(Graphics, tyre_compound), 176);
        assert_eq!(offset_of!(Graphics, normalized_car_position), 248);
        assert_eq!(offset_of!(Graphics, car_coordinates), 256);
        assert_eq!(offset_of!(Graphics, car_id), 976);
        assert_eq!(offset_of!(Graphics, player_car_id), 1216);
        assert_eq!(offset_of!(Graphics, flag), 1224);
        assert_eq!(offset_of!(Graphics, penalty), 1228);
        assert_eq!(offset_of!(Graphics, is_in_pit_lane), 1236);
        assert_eq!(offset_of!(Graphics, wind_speed), 1248);
        assert_eq!(offset_of!(Graphics, tc), 1268);
        assert_eq!(offset_of!(Graphics, abs), 1280);
        assert_eq!(offset_of!(Graphics, fuel_x_lap), 1284);
        assert_eq!(offset_of!(Graphics, i_delta_lap_time), 1360);
        assert_eq!(offset_of!(Graphics, is_valid_lap), 1408);
        assert_eq!(offset_of!(Graphics, clock), 1488);
        assert_eq!(offset_of!(Graphics, global_yellow), 1500);
        assert_eq!(offset_of!(Graphics, global_red), 1528);
        assert_eq!(offset_of!(Graphics, track_grip_status), 1556);
        assert_eq!(offset_of!(Graphics, rain_intensity_in_30min), 1568);
        assert_eq!(offset_of!(Graphics, gap_behind), 1584);
    }

    #[test]
    fn static_offsets() {
        assert_eq!(offset_of!(Static, num_cars), 64);
        assert_eq!(offset_of!(Static, car_model), 68);
        assert_eq!(offset_of!(Static, track), 134);
        assert_eq!(offset_of!(Static, sector_count), 400);
        assert_eq!(offset_of!(Static, max_rpm), 412);
        assert_eq!(offset_of!(Static, max_fuel), 416);
        assert_eq!(offset_of!(Static, pit_window_start), 676);
        assert_eq!(offset_of!(Static, is_online), 684);
        assert_eq!(offset_of!(Static, dry_tyres_name), 688);
    }

    #[test]
    fn reads_values_and_strings_from_raw_bytes() {
        let mut b = vec![0u8; GRAPHICS_SIZE];
        b[132..136].copy_from_slice(&7i32.to_le_bytes());
        let name: Vec<u8> = "dry_compound"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        b[176..176 + name.len()].copy_from_slice(&name);
        let g = Graphics::from_bytes(&b).unwrap();
        assert_eq!(g.completed_laps, 7);
        assert_eq!(wstr(&g.tyre_compound), "dry_compound");
        assert!(Graphics::from_bytes(&b[..100]).is_none());
    }
}
