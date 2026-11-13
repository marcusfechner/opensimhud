//! Read-only access to EVO's named mappings on Windows.

#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Memory::{
        FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    };
    use windows::core::PCWSTR;

    struct View {
        handle: HANDLE,
        addr: MEMORY_MAPPED_VIEW_ADDRESS,
        len: usize,
    }

    impl Drop for View {
        fn drop(&mut self) {
            // SAFETY: The handle and view were acquired together in open().
            unsafe {
                let _ = UnmapViewOfFile(self.addr);
                let _ = CloseHandle(self.handle);
            }
        }
    }

    pub struct Pages(Vec<View>);

    // Views are read only and owned by the telemetry thread.
    unsafe impl Send for Pages {}

    impl Pages {
        pub fn open() -> Option<Self> {
            let mut views = Vec::new();
            for (name, len) in [("physics", 800), ("graphics", 8192), ("static", 2048)] {
                let wide: Vec<u16> = format!("Local\\acevo_pmf_{name}")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                // SAFETY: Names are NUL terminated. Each view is closed by its owner.
                unsafe {
                    let handle =
                        OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr())).ok()?;
                    let addr = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, len);
                    if addr.Value.is_null() {
                        let _ = CloseHandle(handle);
                        return None;
                    }
                    views.push(View { handle, addr, len });
                }
            }
            Some(Self(views))
        }

        pub fn read(&self, index: usize) -> Option<Vec<u8>> {
            let view = self.0.get(index)?;
            let ptr = view.addr.Value.cast::<u8>();
            for _ in 0..4 {
                // SAFETY: View is mapped for len bytes. Packet reads guard copies of the
                // physics and graphics pages; static has no packet counter and is copied once.
                unsafe {
                    let before = std::ptr::read_volatile(ptr.cast::<[u8; 4]>());
                    let bytes = std::slice::from_raw_parts(ptr, view.len).to_vec();
                    let after = std::ptr::read_volatile(ptr.cast::<[u8; 4]>());
                    if before == after {
                        return Some(bytes);
                    }
                }
            }
            None
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub struct Pages;
    impl Pages {
        pub fn open() -> Option<Self> {
            None
        }
        pub fn read(&self, _: usize) -> Option<Vec<u8>> {
            None
        }
    }
}

pub use platform::Pages;
