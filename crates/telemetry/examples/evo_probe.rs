//! Read-only EVO shared-memory probe. Build without dependencies with
//! `rustc --edition 2024 crates/telemetry/examples/evo_probe.rs -o target/evo-probe.exe`.
//! Run in a driving session as `target/evo-probe.exe [output.jsonl]`.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::c_void;
    use std::io::Write;
    use std::time::{Duration, Instant};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        fn MapViewOfFile(
            handle: *mut c_void,
            access: u32,
            hi: u32,
            lo: u32,
            size: usize,
        ) -> *const u8;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let mut views = Vec::new();
    for (name, size) in [("physics", 800), ("graphics", 8192), ("static", 2048)] {
        let name: Vec<u16> = format!("Local\\acevo_pmf_{name}\0")
            .encode_utf16()
            .collect();
        // SAFETY: Open an existing mapping read-only, then request a bounded view.
        let handle = unsafe { OpenFileMappingW(4, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("EVO mapping unavailable; start a driving session".into());
        }
        let view = unsafe { MapViewOfFile(handle, 4, 0, 0, size) };
        if view.is_null() {
            unsafe { CloseHandle(handle) };
            return Err("Could not map EVO page at the requested size".into());
        }
        views.push((view, size));
    }
    let mut output = std::env::args()
        .nth(1)
        .map(std::fs::File::create)
        .transpose()?;
    let start = Instant::now();
    loop {
        // SAFETY: each view maps the requested number of readable bytes until process exit.
        let pages: Vec<Vec<u8>> = views
            .iter()
            .map(|(view, size)| unsafe { std::slice::from_raw_parts(*view, *size).to_vec() })
            .collect();
        let bytes = &pages[0];
        let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let i = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let elapsed = start.elapsed().as_secs_f64();
        if let Some(file) = output.as_mut() {
            let hex =
                |bytes: &[u8]| -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() };
            let physics = hex(&pages[0]);
            let graphics = hex(&pages[1]);
            let statics = hex(&pages[2]);
            writeln!(
                file,
                "{{\"format\":\"opensimhud-evo-1\",\"seconds\":{elapsed:.3},\"physics_hex\":\"{physics}\",\"graphics_hex\":\"{graphics}\",\"static_hex\":\"{statics}\"}}"
            )?;
            file.flush()?;
        }
        println!(
            "{elapsed:6.1}s packet={} throttle={:.2} brake={:.2} gear={} rpm={} steer={:.2} speed={:.1}",
            i(0),
            f(4),
            f(8),
            i(16),
            i(20),
            f(24),
            f(28)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("EVO shared memory is only available on Windows");
}
