//! ACC Broadcasting API (UDP, protocol version 4): message encoding and decoding.
//!
//! Every car's position, laps and entry-list data come from here; ACC only sends it after a
//! client registers with the port and password from `broadcasting.json`. Little-endian;
//! strings are UTF-8 with a u16 byte length.

pub const PROTOCOL_VERSION: u8 = 4;
/// Lap times are `i32::MAX` when there is no time.
const NO_TIME: i32 = i32::MAX;

// outbound message types
const REGISTER: u8 = 1;
const UNREGISTER: u8 = 9;
const REQUEST_ENTRY_LIST: u8 = 10;
const REQUEST_TRACK_DATA: u8 = 11;

// inbound message types
const REGISTRATION_RESULT: u8 = 1;
const REALTIME_UPDATE: u8 = 2;
const REALTIME_CAR_UPDATE: u8 = 3;
const ENTRY_LIST: u8 = 4;
const TRACK_DATA: u8 = 5;
const ENTRY_LIST_CAR: u8 = 6;
const BROADCASTING_EVENT: u8 = 7;

/* ---------- outbound ---------- */

fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&b[..n]);
}

/// Register as a client; ACC answers with a `Registration` and then streams updates every
/// `interval_ms`.
pub fn register(
    display_name: &str,
    password: &str,
    interval_ms: u32,
    command_password: &str,
) -> Vec<u8> {
    let mut out = vec![REGISTER, PROTOCOL_VERSION];
    put_str(&mut out, display_name);
    put_str(&mut out, password);
    out.extend_from_slice(&interval_ms.to_le_bytes());
    put_str(&mut out, command_password);
    out
}

fn with_connection(kind: u8, connection_id: i32) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend_from_slice(&connection_id.to_le_bytes());
    out
}

pub fn unregister(connection_id: i32) -> Vec<u8> {
    with_connection(UNREGISTER, connection_id)
}
pub fn request_entry_list(connection_id: i32) -> Vec<u8> {
    with_connection(REQUEST_ENTRY_LIST, connection_id)
}
pub fn request_track_data(connection_id: i32) -> Vec<u8> {
    with_connection(REQUEST_TRACK_DATA, connection_id)
}

/* ---------- inbound ---------- */

#[derive(Debug, Clone, PartialEq)]
pub struct Lap {
    /// Milliseconds, `None` when there is no time.
    pub time_ms: Option<i32>,
    pub car: u16,
    pub driver: u16,
    pub splits: Vec<Option<i32>>,
    pub invalid: bool,
    pub valid_for_best: bool,
    pub out_lap: bool,
    pub in_lap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RealtimeUpdate {
    pub event_index: u16,
    pub session_index: u16,
    /// 0 practice, 4 qualifying, 9 superpole, 10 race, 11 hotlap, 12 hotstint, 13 hotlap superpole, 14 replay.
    pub session_type: u8,
    /// 0 none, 1 starting, 2 pre-formation, 3 formation lap, 4 pre-session, 5 session, 6 session over, 7 post-session, 8 result UI.
    pub phase: u8,
    /// Milliseconds since the session started.
    pub session_time: f32,
    pub session_end_time: f32,
    pub focused_car: i32,
    pub camera_set: String,
    pub camera: String,
    pub hud_page: String,
    pub replay: Option<(f32, f32)>,
    /// Milliseconds since midnight.
    pub time_of_day: f32,
    pub ambient_temp: i8,
    pub track_temp: i8,
    /// 0..1.
    pub clouds: f32,
    pub rain_level: f32,
    pub wetness: f32,
    pub best_session_lap: Lap,
}

/// Where a car is: 1 track, 2 pit lane, 3 pit entry, 4 pit exit (0 unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarLocation(pub u8);

impl CarLocation {
    pub fn in_pit(self) -> bool {
        matches!(self.0, 2..=4)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarUpdate {
    pub car: u16,
    pub driver: u16,
    pub driver_count: u8,
    /// -1 reverse, 0 neutral, 1.. gears.
    pub gear: i8,
    pub x: f32,
    pub y: f32,
    pub yaw: f32,
    pub location: CarLocation,
    pub kmh: u16,
    pub position: u16,
    pub cup_position: u16,
    pub track_position: u16,
    /// Lap fraction 0..1.
    pub spline: f32,
    pub laps: u16,
    /// Live delta to the car's best lap, milliseconds.
    pub delta: i32,
    pub best_session_lap: Lap,
    pub last_lap: Lap,
    pub current_lap: Lap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Driver {
    pub first_name: String,
    pub last_name: String,
    pub short_name: String,
    /// 3 platinum, 2 gold, 1 silver, 0 bronze, 255 unknown.
    pub category: u8,
    pub nationality: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryCar {
    pub car: u16,
    pub model: u8,
    pub team: String,
    pub race_number: i32,
    /// Cup category: 0 overall/pro, 1 pro-am, 2 am, 3 silver, 4 national.
    pub cup_category: u8,
    pub current_driver: u8,
    pub nationality: u16,
    pub drivers: Vec<Driver>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackData {
    pub name: String,
    pub id: i32,
    pub meters: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// 1 green flag, 2 session over, 3 penalty, 4 accident, 5 lap completed, 6 best session lap, 7 best personal lap.
    pub kind: u8,
    pub message: String,
    pub time_ms: i32,
    pub car: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Registration {
        connection_id: i32,
        success: bool,
        read_only: bool,
        error: String,
    },
    Realtime(RealtimeUpdate),
    Car(CarUpdate),
    /// Car indices currently in the session; details follow as `EntryCar` messages.
    EntryList(Vec<u16>),
    EntryCar(EntryCar),
    Track(TrackData),
    Event(Event),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    UnknownType(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "message ended early"),
            Self::UnknownType(t) => write!(f, "unknown message type {t}"),
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let s = self
            .b
            .get(self.pos..self.pos + n)
            .ok_or(DecodeError::Truncated)?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.u8()? as i8)
    }
    fn bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.u8()? != 0)
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn time(&mut self) -> Result<Option<i32>, DecodeError> {
        let t = self.i32()?;
        Ok((t != NO_TIME).then_some(t))
    }
    fn lap(&mut self) -> Result<Lap, DecodeError> {
        let time_ms = self.time()?;
        let car = self.u16()?;
        let driver = self.u16()?;
        let n = self.u8()?;
        let splits = (0..n).map(|_| self.time()).collect::<Result<_, _>>()?;
        Ok(Lap {
            time_ms,
            car,
            driver,
            splits,
            invalid: self.bool()?,
            valid_for_best: self.bool()?,
            out_lap: self.bool()?,
            in_lap: self.bool()?,
        })
    }
}

pub fn decode(bytes: &[u8]) -> Result<Message, DecodeError> {
    let mut r = Reader { b: bytes, pos: 0 };
    match r.u8()? {
        REGISTRATION_RESULT => Ok(Message::Registration {
            connection_id: r.i32()?,
            success: r.bool()?,
            read_only: r.bool()?,
            error: r.str()?,
        }),
        REALTIME_UPDATE => {
            let event_index = r.u16()?;
            let session_index = r.u16()?;
            let session_type = r.u8()?;
            let phase = r.u8()?;
            let session_time = r.f32()?;
            let session_end_time = r.f32()?;
            let focused_car = r.i32()?;
            let camera_set = r.str()?;
            let camera = r.str()?;
            let hud_page = r.str()?;
            let replay = if r.bool()? {
                Some((r.f32()?, r.f32()?))
            } else {
                None
            };
            Ok(Message::Realtime(RealtimeUpdate {
                event_index,
                session_index,
                session_type,
                phase,
                session_time,
                session_end_time,
                focused_car,
                camera_set,
                camera,
                hud_page,
                replay,
                time_of_day: r.f32()?,
                ambient_temp: r.i8()?,
                track_temp: r.i8()?,
                clouds: r.u8()? as f32 / 10.0,
                rain_level: r.u8()? as f32 / 10.0,
                wetness: r.u8()? as f32 / 10.0,
                best_session_lap: r.lap()?,
            }))
        }
        REALTIME_CAR_UPDATE => Ok(Message::Car(CarUpdate {
            car: r.u16()?,
            driver: r.u16()?,
            driver_count: r.u8()?,
            // sent offset by two so reverse fits in a byte
            gear: r.u8()?.wrapping_sub(2) as i8,
            x: r.f32()?,
            y: r.f32()?,
            yaw: r.f32()?,
            location: CarLocation(r.u8()?),
            kmh: r.u16()?,
            position: r.u16()?,
            cup_position: r.u16()?,
            track_position: r.u16()?,
            spline: r.f32()?,
            laps: r.u16()?,
            delta: r.i32()?,
            best_session_lap: r.lap()?,
            last_lap: r.lap()?,
            current_lap: r.lap()?,
        })),
        ENTRY_LIST => {
            let _connection_id = r.i32()?;
            let n = r.u16()?;
            Ok(Message::EntryList(
                (0..n).map(|_| r.u16()).collect::<Result<_, _>>()?,
            ))
        }
        ENTRY_LIST_CAR => {
            let car = r.u16()?;
            let model = r.u8()?;
            let team = r.str()?;
            let race_number = r.i32()?;
            let cup_category = r.u8()?;
            let current_driver = r.u8()?;
            let nationality = r.u16()?;
            let n = r.u8()?;
            let drivers = (0..n)
                .map(|_| {
                    Ok(Driver {
                        first_name: r.str()?,
                        last_name: r.str()?,
                        short_name: r.str()?,
                        category: r.u8()?,
                        nationality: r.u16()?,
                    })
                })
                .collect::<Result<_, DecodeError>>()?;
            Ok(Message::EntryCar(EntryCar {
                car,
                model,
                team,
                race_number,
                cup_category,
                current_driver,
                nationality,
                drivers,
            }))
        }
        TRACK_DATA => {
            let _connection_id = r.i32()?;
            // camera sets and HUD pages follow; we don't need them
            Ok(Message::Track(TrackData {
                name: r.str()?,
                id: r.i32()?,
                meters: r.i32()?,
            }))
        }
        BROADCASTING_EVENT => Ok(Message::Event(Event {
            kind: r.u8()?,
            message: r.str()?,
            time_ms: r.i32()?,
            car: r.i32()?,
        })),
        other => Err(DecodeError::UnknownType(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds inbound packets the way ACC writes them.
    #[derive(Default)]
    struct W(Vec<u8>);
    impl W {
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
            put_str(&mut self.0, v);
            self
        }
        fn lap(self, ms: i32, car: u16, splits: &[i32], invalid: bool, out_lap: bool) -> Self {
            let mut w = self.i32(ms).u16(car).u16(0).u8(splits.len() as u8);
            for &s in splits {
                w = w.i32(s);
            }
            w.u8(invalid as u8)
                .u8(!invalid as u8)
                .u8(out_lap as u8)
                .u8(0)
        }
    }

    #[test]
    fn encodes_registration_like_the_sdk() {
        // byte layout from the official SDK's registration request
        let expected = [
            0x01, 0x04, 0x09, 0x00, b'Y', b'o', b'u', b'r', b' ', b'n', b'a', b'm', b'e', 0x03,
            0x00, b'a', b's', b'd', 0xfa, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(register("Your name", "asd", 250, ""), expected);
        assert_eq!(request_entry_list(7), [10, 7, 0, 0, 0]);
    }

    #[test]
    fn decodes_registration_result() {
        let b = W::default().u8(1).i32(3).u8(1).u8(0).s("").0;
        assert_eq!(
            decode(&b).unwrap(),
            Message::Registration {
                connection_id: 3,
                success: true,
                read_only: false,
                error: String::new()
            }
        );
    }

    #[test]
    fn decodes_car_update_with_laps() {
        let b = W::default()
            .u8(3)
            .u16(12) // car
            .u16(0)
            .u8(1)
            .u8(5) // gear byte → 3rd
            .f32(-120.5)
            .f32(45.25)
            .f32(1.2)
            .u8(2) // pit lane
            .u16(143)
            .u16(4)
            .u16(2)
            .u16(4)
            .f32(0.25)
            .u16(9)
            .i32(-350)
            .lap(107_355, 12, &[34_000, 44_200, 29_155], false, false)
            .lap(107_812, 12, &[34_100, 44_400, 29_312], true, false)
            .lap(NO_TIME, 12, &[], false, true)
            .0;
        let Message::Car(c) = decode(&b).unwrap() else {
            panic!("not a car update")
        };
        assert_eq!(
            (c.car, c.gear, c.kmh, c.position, c.laps, c.delta),
            (12, 3, 143, 4, 9, -350)
        );
        assert!(c.location.in_pit());
        assert_eq!(c.best_session_lap.time_ms, Some(107_355));
        assert_eq!(
            c.last_lap.splits,
            vec![Some(34_100), Some(44_400), Some(29_312)]
        );
        assert!(c.last_lap.invalid);
        assert_eq!(c.current_lap.time_ms, None);
        assert!(c.current_lap.out_lap);
        assert!((c.spline - 0.25).abs() < 1e-6);
    }

    #[test]
    fn decodes_realtime_update_live_and_replay() {
        let base = |replay: bool| {
            let w = W::default()
                .u8(2)
                .u16(1)
                .u16(2)
                .u8(10)
                .u8(5)
                .f32(900_000.0)
                .f32(2_700_000.0)
                .i32(12)
                .s("Drivable")
                .s("Chase")
                .s("Basic HUD");
            let w = if replay {
                w.u8(1).f32(1.0).f32(2.0)
            } else {
                w.u8(0)
            };
            w.f32(52_320_000.0)
                .u8(21)
                .u8(29)
                .u8(3)
                .u8(0)
                .u8(1)
                .lap(105_928, 0, &[], false, false)
                .0
        };
        for replay in [false, true] {
            let Message::Realtime(u) = decode(&base(replay)).unwrap() else {
                panic!()
            };
            assert_eq!((u.session_type, u.phase, u.focused_car), (10, 5, 12));
            assert_eq!(u.replay.is_some(), replay);
            assert_eq!((u.ambient_temp, u.track_temp), (21, 29));
            assert!((u.clouds - 0.3).abs() < 1e-6 && (u.wetness - 0.1).abs() < 1e-6);
            assert_eq!(u.best_session_lap.time_ms, Some(105_928));
        }
    }

    #[test]
    fn decodes_entry_list_car_with_drivers() {
        let b = W::default()
            .u8(6)
            .u16(12)
            .u8(32)
            .s("Scuderia Nord")
            .i32(46)
            .u8(0)
            .u8(0)
            .u16(1)
            .u8(1)
            .s("Sven")
            .s("Varga")
            .s("VAR")
            .u8(1)
            .u16(1)
            .0;
        let Message::EntryCar(e) = decode(&b).unwrap() else {
            panic!()
        };
        assert_eq!((e.car, e.model, e.race_number), (12, 32, 46));
        assert_eq!(e.drivers[0].last_name, "Varga");
        assert_eq!(e.drivers[0].category, 1);
    }

    #[test]
    fn decodes_entry_list_track_and_event() {
        let b = W::default().u8(4).i32(3).u16(2).u16(0).u16(12).0;
        assert_eq!(decode(&b).unwrap(), Message::EntryList(vec![0, 12]));
        let b = W::default()
            .u8(5)
            .i32(3)
            .s("monza")
            .i32(11)
            .i32(5793)
            .u8(0)
            .u8(0)
            .0;
        assert_eq!(
            decode(&b).unwrap(),
            Message::Track(TrackData {
                name: "monza".into(),
                id: 11,
                meters: 5793
            })
        );
        let b = W::default()
            .u8(7)
            .u8(3)
            .s("Drive through for cutting")
            .i32(1000)
            .i32(12)
            .0;
        let Message::Event(e) = decode(&b).unwrap() else {
            panic!()
        };
        assert_eq!((e.kind, e.car), (3, 12));
    }

    #[test]
    fn rejects_truncated_and_unknown() {
        assert_eq!(decode(&[3, 1]), Err(DecodeError::Truncated));
        assert_eq!(decode(&[42]), Err(DecodeError::UnknownType(42)));
        assert_eq!(decode(&[]), Err(DecodeError::Truncated));
    }
}
