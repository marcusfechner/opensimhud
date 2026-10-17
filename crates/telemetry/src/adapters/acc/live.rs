//! The running game: shared memory pages (Windows only) and the broadcasting UDP socket.

use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

use super::pages::{GRAPHICS_SIZE, PHYSICS_SIZE, STATIC_SIZE};
use super::{Raw, Source};

/// Re-check the static page (car, track, session setup) this often; it rarely changes.
const STATIC_EVERY: f64 = 1.0;
/// Retry opening shared memory / the socket this often while the game isn't there.
const RETRY_EVERY: f64 = 1.0;

pub struct Live {
    start: Instant,
    target: SocketAddr,
    udp: Option<UdpSocket>,
    shm: Option<shm::Pages>,
    last_try: f64,
    last_ids: [i32; 2],
    last_static: Vec<u8>,
    last_static_check: f64,
    buf: Vec<u8>,
}

impl Live {
    /// `target` is ACC's broadcasting address, normally 127.0.0.1 with the port from
    /// `broadcasting.json`.
    pub fn new(target: SocketAddr) -> Self {
        Self {
            start: Instant::now(),
            target,
            udp: None,
            shm: None,
            last_try: f64::NEG_INFINITY,
            last_ids: [i32::MIN; 2],
            last_static: Vec::new(),
            last_static_check: f64::NEG_INFINITY,
            buf: vec![0; 64 * 1024],
        }
    }

    /// Whether this platform can read shared memory at all.
    pub fn has_shared_memory() -> bool {
        cfg!(windows)
    }
}

impl Source for Live {
    fn drain(&mut self, out: &mut Vec<Raw>) {
        let now = self.clock();
        if (self.shm.is_none() || self.udp.is_none()) && now - self.last_try >= RETRY_EVERY {
            self.last_try = now;
            if self.shm.is_none() {
                self.shm = shm::Pages::open();
            }
            if self.udp.is_none() {
                self.udp = UdpSocket::bind(("0.0.0.0", 0))
                    .and_then(|s| {
                        s.connect(self.target)
                            .and(s.set_nonblocking(true))
                            .map(|_| s)
                    })
                    .ok();
            }
        }

        if let Some(pages) = &self.shm {
            // only pages that changed: packet ids count up with every game update
            for (k, size) in [(0usize, PHYSICS_SIZE), (1, GRAPHICS_SIZE)] {
                if let Some(page) = pages.read(k, size)
                    && let Some(id) = page
                        .get(0..4)
                        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
                    && id != self.last_ids[k]
                {
                    self.last_ids[k] = id;
                    out.push(if k == 0 {
                        Raw::Physics(page)
                    } else {
                        Raw::Graphics(page)
                    });
                }
            }
            if now - self.last_static_check >= STATIC_EVERY {
                self.last_static_check = now;
                if let Some(page) = pages.read(2, STATIC_SIZE)
                    && page != self.last_static
                {
                    self.last_static = page.clone();
                    out.push(Raw::Static(page));
                }
            }
        }

        if let Some(udp) = &self.udp {
            // stops at WouldBlock (drained) or any other error, e.g. "port unreachable" while
            // ACC is closed on Windows; the next poll simply tries again
            while let Ok(n) = udp.recv(&mut self.buf) {
                out.push(Raw::Udp(self.buf[..n].to_vec()));
            }
        }
    }

    fn send(&mut self, datagram: &[u8]) {
        if let Some(udp) = &self.udp {
            let _ = udp.send(datagram);
        }
    }

    fn clock(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn connected(&self) -> bool {
        self.shm.is_some()
    }
}

#[cfg(windows)]
mod shm {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Memory::{
        FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    };
    use windows::core::PCWSTR;

    const NAMES: [&str; 3] = [
        "Local\\acpmf_physics",
        "Local\\acpmf_graphics",
        "Local\\acpmf_static",
    ];

    struct View {
        handle: HANDLE,
        addr: MEMORY_MAPPED_VIEW_ADDRESS,
    }

    /// The three mapped pages. Opening fails until ACC has created them.
    pub struct Pages {
        views: Vec<View>,
    }

    // The views are only read, by copying; the pointers stay valid until Drop.
    unsafe impl Send for Pages {}

    impl Pages {
        pub fn open() -> Option<Self> {
            let mut views = Vec::new();
            for name in NAMES {
                let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                // SAFETY: plain Win32 calls with a NUL-terminated name; the handle and view are
                // released in Drop.
                unsafe {
                    let handle =
                        OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr())).ok()?;
                    let addr = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
                    if addr.Value.is_null() {
                        let _ = CloseHandle(handle);
                        return None;
                    }
                    views.push(View { handle, addr });
                }
            }
            Some(Self { views })
        }

        /// Copy page `k`. The game writes while we read, so retry until the packet id at the
        /// start is the same before and after the copy (no torn pages).
        pub fn read(&self, k: usize, size: usize) -> Option<Vec<u8>> {
            let ptr = self.views.get(k)?.addr.Value as *const u8;
            for _ in 0..4 {
                // SAFETY: ACC maps at least `size` bytes per page (sizes are the official
                // struct sizes); volatile/unaligned reads because another process writes it.
                unsafe {
                    let before = std::ptr::read_volatile(ptr.cast::<[u8; 4]>());
                    let page = std::slice::from_raw_parts(ptr, size).to_vec();
                    let after = std::ptr::read_volatile(ptr.cast::<[u8; 4]>());
                    if before == after {
                        return Some(page);
                    }
                }
            }
            None
        }
    }

    impl Drop for Pages {
        fn drop(&mut self) {
            for v in &self.views {
                // SAFETY: each view/handle came from MapViewOfFile/OpenFileMappingW above.
                unsafe {
                    let _ = UnmapViewOfFile(v.addr);
                    let _ = CloseHandle(v.handle);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod shm {
    /// ACC only exists on Windows; elsewhere only the broadcast socket works.
    pub struct Pages;

    impl Pages {
        pub fn open() -> Option<Self> {
            None
        }
        pub fn read(&self, _k: usize, _size: usize) -> Option<Vec<u8>> {
            None
        }
    }
}
