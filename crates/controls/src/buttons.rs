//! Buttons on game controllers: wheels, button boxes, gamepads. Read through DirectInput in
//! background mode, which every wheel supports and which keeps working while the game has
//! the focus. Only presses are reported; the game still gets every button as before.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Buttons per controller, then four directions for each of its hats (d-pads).
pub const BUTTONS: u16 = 128;
pub const HATS: u16 = 4;

/// One button on one controller: what a press reports and what a binding stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct Press {
    /// The controller's instance id, stable on this PC.
    pub device: String,
    /// The controller's name, for showing.
    pub name: String,
    /// 0-based button, or `BUTTONS + hat * 4 + direction` (up, right, down, left) for a hat.
    pub button: u16,
}

pub use imp::Buttons;

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    use windows::Win32::Devices::HumanInterfaceDevice::{
        DI8DEVCLASS_GAMECTRL, DIDATAFORMAT, DIDEVICEINSTANCEW, DIDF_ABSAXIS, DIDFT_ANYINSTANCE,
        DIDFT_BUTTON, DIDFT_POV, DIEDFL_ATTACHEDONLY, DIENUM_CONTINUE, DIOBJECTDATAFORMAT,
        DIRECTINPUT_VERSION, DISCL_BACKGROUND, DISCL_NONEXCLUSIVE, DirectInput8Create, GUID_POV,
        IDirectInput8W, IDirectInputDevice8W,
    };
    use windows::Win32::Foundation::{HINSTANCE, HWND};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::core::{BOOL, GUID, IUnknown, Interface};

    use super::{BUTTONS, HATS, Press};

    const DIDFT_OPTIONAL: u32 = 0x8000_0000;
    /// How often to look for controllers plugged in or out.
    const RESCAN: Duration = Duration::from_secs(3);
    static POV: GUID = GUID_POV;

    /// What we ask DirectInput for: the hats, then the buttons (like DIJOYSTATE2, no axes).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct State {
        hats: [u32; HATS as usize],
        buttons: [u8; BUTTONS as usize],
    }

    struct Device {
        dev: IDirectInputDevice8W,
        guid: GUID,
        id: String,
        name: String,
        /// Which buttons and hat directions are down; `None` until the first reading.
        down: Option<Vec<bool>>,
    }

    pub struct Buttons {
        di: Option<IDirectInput8W>,
        hwnd: HWND,
        objects: Vec<DIOBJECTDATAFORMAT>,
        devices: Vec<Device>,
        next_scan: Instant,
    }

    impl Buttons {
        /// `hwnd` is one of this app's top-level windows; DirectInput wants one, even for
        /// background reading. Call from a thread with COM initialized.
        pub fn new(hwnd: isize) -> Self {
            // SAFETY: plain DirectInput creation; the pointer is only used if it succeeded
            let di = unsafe {
                GetModuleHandleW(None).ok().and_then(|m| {
                    let mut p: *mut c_void = std::ptr::null_mut();
                    DirectInput8Create(
                        HINSTANCE(m.0),
                        DIRECTINPUT_VERSION,
                        &IDirectInput8W::IID,
                        &mut p,
                        None::<&IUnknown>,
                    )
                    .ok()
                    .map(|_| IDirectInput8W::from_raw(p))
                })
            };
            let hats = (0..HATS).map(|k| DIOBJECTDATAFORMAT {
                pguid: &POV,
                dwOfs: k as u32 * 4,
                dwType: DIDFT_POV | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                dwFlags: 0,
            });
            let buttons = (0..BUTTONS).map(|k| DIOBJECTDATAFORMAT {
                pguid: std::ptr::null(),
                dwOfs: (HATS * 4 + k) as u32,
                dwType: DIDFT_BUTTON | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                dwFlags: 0,
            });
            Self {
                di,
                hwnd: HWND(hwnd as *mut c_void),
                objects: hats.chain(buttons).collect(),
                devices: Vec::new(),
                next_scan: Instant::now(),
            }
        }

        /// Buttons pressed since the last call. Call often (every 20 ms or so).
        pub fn poll(&mut self) -> Vec<Press> {
            let now = Instant::now();
            if now >= self.next_scan {
                self.next_scan = now + RESCAN;
                self.scan();
            }
            let mut out = Vec::new();
            for d in &mut self.devices {
                let mut st = State {
                    hats: [u32::MAX; HATS as usize],
                    buttons: [0; BUTTONS as usize],
                };
                // SAFETY: `st` matches the data format set on the device
                let read = unsafe {
                    let _ = d.dev.Poll();
                    d.dev.GetDeviceState(
                        size_of::<State>() as u32,
                        &mut st as *mut State as *mut c_void,
                    )
                };
                if read.is_err() {
                    // lost (another app took it, or it was unplugged): try again next time
                    // SAFETY: plain COM call on a live device
                    let _ = unsafe { d.dev.Acquire() };
                    continue;
                }
                let mut now_down = Vec::with_capacity((BUTTONS + HATS * 4) as usize);
                now_down.extend(st.buttons.iter().map(|b| b & 0x80 != 0));
                for v in st.hats {
                    // centered reads 0xFFFF in the low word; otherwise hundredths of degrees
                    let dir = (v & 0xFFFF != 0xFFFF).then(|| ((v + 4500) / 9000) % 4);
                    now_down.extend((0..4).map(|k| dir == Some(k)));
                }
                if let Some(before) = &d.down {
                    for (k, (&on, &was)) in now_down.iter().zip(before).enumerate() {
                        if on && !was {
                            out.push(Press {
                                device: d.id.clone(),
                                name: d.name.clone(),
                                button: k as u16,
                            });
                        }
                    }
                }
                d.down = Some(now_down);
            }
            out
        }

        /// Open controllers that appeared, drop the ones that are gone.
        fn scan(&mut self) {
            let Some(di) = &self.di else {
                return;
            };
            let mut found: Vec<(GUID, String)> = Vec::new();
            // SAFETY: the callback only runs during EnumDevices and gets `found` back
            let listed = unsafe {
                di.EnumDevices(
                    DI8DEVCLASS_GAMECTRL,
                    Some(on_device),
                    &mut found as *mut Vec<(GUID, String)> as *mut c_void,
                    DIEDFL_ATTACHEDONLY,
                )
            };
            if listed.is_err() {
                return;
            }
            self.devices
                .retain(|d| found.iter().any(|(g, _)| *g == d.guid));
            for (guid, name) in found {
                if self.devices.iter().any(|d| d.guid == guid) {
                    continue;
                }
                if let Some(dev) = self.open(di, &guid) {
                    self.devices.push(Device {
                        dev,
                        guid,
                        id: format!("{guid:?}"),
                        name,
                        down: None,
                    });
                }
            }
        }

        fn open(&self, di: &IDirectInput8W, guid: &GUID) -> Option<IDirectInputDevice8W> {
            let mut format = DIDATAFORMAT {
                dwSize: size_of::<DIDATAFORMAT>() as u32,
                dwObjSize: size_of::<DIOBJECTDATAFORMAT>() as u32,
                dwFlags: DIDF_ABSAXIS,
                dwDataSize: size_of::<State>() as u32,
                dwNumObjs: self.objects.len() as u32,
                rgodf: self.objects.as_ptr() as *mut DIOBJECTDATAFORMAT,
            };
            // SAFETY: DirectInput copies the format; `objects` outlives the call anyway
            unsafe {
                let mut dev: Option<IDirectInputDevice8W> = None;
                di.CreateDevice(guid, &mut dev, None::<&IUnknown>).ok()?;
                let dev = dev?;
                dev.SetDataFormat(&mut format).ok()?;
                // background: buttons still read while the game has the focus; non-exclusive:
                // the game keeps getting them too
                dev.SetCooperativeLevel(self.hwnd, DISCL_BACKGROUND | DISCL_NONEXCLUSIVE)
                    .ok()?;
                let _ = dev.Acquire();
                Some(dev)
            }
        }
    }

    unsafe extern "system" fn on_device(inst: *mut DIDEVICEINSTANCEW, ctx: *mut c_void) -> BOOL {
        // SAFETY: DirectInput passes a valid instance; `ctx` is the Vec given to EnumDevices
        let (inst, found) = unsafe { (&*inst, &mut *(ctx as *mut Vec<(GUID, String)>)) };
        let name = &inst.tszProductName;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        found.push((inst.guidInstance, String::from_utf16_lossy(&name[..len])));
        BOOL(DIENUM_CONTINUE as i32)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Press;

    /// Only Windows has DirectInput; elsewhere no controller buttons are read.
    pub struct Buttons;

    impl Buttons {
        pub fn new(_hwnd: isize) -> Self {
            Self
        }
        pub fn poll(&mut self) -> Vec<Press> {
            Vec::new()
        }
    }
}
