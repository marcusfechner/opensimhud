//! `.ovrec` recordings: the raw ACC data (shared memory pages and broadcast datagrams) with
//! timestamps, so a real session can be replayed through the exact same decoder anywhere.
//!
//! Format: the magic line `OVREC1\n`, then a raw-deflate stream of records
//! `[kind: u8][time: u64 µs][len: u32][payload]`, all little-endian. Pages are stored XORed
//! with the previous page of the same kind (mostly zeros, so they compress very well). The
//! stream is flushed every second, so a recording cut short (crash, killed process) still
//! replays up to that point.

use std::io::{self, BufReader, BufWriter, Read, Write};
use std::time::Instant;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use super::{Raw, Source};

const MAGIC: &[u8; 7] = b"OVREC1\n";
const PHYSICS: u8 = 1;
const GRAPHICS: u8 = 2;
const STATIC: u8 = 3;
const UDP: u8 = 4;

/// Writes raw events as they pass through.
pub struct Writer<W: Write> {
    out: DeflateEncoder<BufWriter<W>>,
    prev: [Vec<u8>; 3],
    last_flush: f64,
    /// Uncompressed bytes written, for progress output.
    pub bytes: u64,
    pub records: u64,
}

impl<W: Write> Writer<W> {
    pub fn new(inner: W) -> io::Result<Self> {
        let mut buf = BufWriter::new(inner);
        buf.write_all(MAGIC)?;
        Ok(Self {
            out: DeflateEncoder::new(buf, Compression::new(5)),
            prev: Default::default(),
            last_flush: 0.0,
            bytes: 0,
            records: 0,
        })
    }

    pub fn write(&mut self, t: f64, raw: &Raw) -> io::Result<()> {
        let (kind, payload): (u8, std::borrow::Cow<[u8]>) = match raw {
            Raw::Physics(b) => (PHYSICS, self.delta(0, b).into()),
            Raw::Graphics(b) => (GRAPHICS, self.delta(1, b).into()),
            Raw::Static(b) => (STATIC, self.delta(2, b).into()),
            Raw::Udp(b) => (UDP, b.as_slice().into()),
        };
        self.out.write_all(&[kind])?;
        self.out
            .write_all(&((t.max(0.0) * 1e6) as u64).to_le_bytes())?;
        self.out.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.out.write_all(&payload)?;
        self.bytes += 13 + payload.len() as u64;
        self.records += 1;
        if t - self.last_flush >= 1.0 {
            self.last_flush = t;
            self.out.flush()?;
        }
        Ok(())
    }

    fn delta(&mut self, k: usize, page: &[u8]) -> Vec<u8> {
        let prev = &mut self.prev[k];
        let d = if prev.len() == page.len() {
            page.iter().zip(prev.iter()).map(|(a, b)| a ^ b).collect()
        } else {
            page.to_vec()
        };
        prev.clear();
        prev.extend_from_slice(page);
        d
    }

    /// Finish the stream; call before dropping for a complete file.
    pub fn finish(self) -> io::Result<()> {
        self.out.finish()?.flush()
    }
}

/// Reads records back in order.
pub struct Reader<R: Read> {
    input: DeflateDecoder<BufReader<R>>,
    prev: [Vec<u8>; 3],
}

impl<R: Read> Reader<R> {
    pub fn new(inner: R) -> io::Result<Self> {
        let mut buf = BufReader::new(inner);
        let mut magic = [0u8; 7];
        buf.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not an .ovrec recording",
            ));
        }
        Ok(Self {
            input: DeflateDecoder::new(buf),
            prev: Default::default(),
        })
    }

    /// The next record, or `None` at the end (including a recording that was cut short).
    pub fn next_record(&mut self) -> Option<(f64, Raw)> {
        let mut head = [0u8; 13];
        self.input.read_exact(&mut head).ok()?;
        let kind = head[0];
        let t = u64::from_le_bytes(head[1..9].try_into().unwrap()) as f64 / 1e6;
        let len = u32::from_le_bytes(head[9..13].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; len];
        self.input.read_exact(&mut payload).ok()?;
        let mut undelta = |k: usize, mut d: Vec<u8>| {
            let prev = &mut self.prev[k];
            if prev.len() == d.len() {
                d.iter_mut().zip(prev.iter()).for_each(|(a, b)| *a ^= b);
            }
            prev.clear();
            prev.extend_from_slice(&d);
            d
        };
        let raw = match kind {
            PHYSICS => Raw::Physics(undelta(0, payload)),
            GRAPHICS => Raw::Graphics(undelta(1, payload)),
            STATIC => Raw::Static(undelta(2, payload)),
            UDP => Raw::Udp(payload),
            _ => return None,
        };
        Some((t, raw))
    }
}

/// Wraps a source and records everything it delivers.
pub struct Recording<S: Source, W: Write> {
    pub inner: S,
    pub writer: Option<Writer<W>>,
    pub error: Option<io::Error>,
}

impl<S: Source, W: Write> Recording<S, W> {
    pub fn new(inner: S, writer: Writer<W>) -> Self {
        Self {
            inner,
            writer: Some(writer),
            error: None,
        }
    }

    pub fn finish(&mut self) -> io::Result<()> {
        match self.writer.take() {
            Some(w) => w.finish(),
            None => Ok(()),
        }
    }
}

impl<S: Source, W: Write + Send> Source for Recording<S, W> {
    fn drain(&mut self, out: &mut Vec<Raw>) {
        let from = out.len();
        self.inner.drain(out);
        let t = self.inner.clock();
        if let Some(w) = &mut self.writer {
            for raw in &out[from..] {
                if let Err(e) = w.write(t, raw) {
                    self.error = Some(e);
                    self.writer = None;
                    break;
                }
            }
        }
    }
    fn send(&mut self, datagram: &[u8]) {
        self.inner.send(datagram);
    }
    fn clock(&self) -> f64 {
        self.inner.clock()
    }
    fn connected(&self) -> bool {
        self.inner.connected()
    }
}

/// Plays a recording back as a source, in real time or as fast as it is polled.
pub struct Replay<R: Read> {
    reader: Reader<R>,
    next: Option<(f64, Raw)>,
    start: Instant,
    /// `None` = deliver everything recorded up to `clock`, advanced by `step` per drain.
    realtime: bool,
    clock: f64,
    step: f64,
    pub finished: bool,
}

impl<R: Read> Replay<R> {
    /// Real time: plays at the speed it was recorded.
    pub fn realtime(reader: Reader<R>) -> Self {
        Self::new(reader, true, 0.0)
    }

    /// Fast: each drain advances the clock by `step` seconds (for tools and tests).
    pub fn stepped(reader: Reader<R>, step: f64) -> Self {
        Self::new(reader, false, step)
    }

    fn new(mut reader: Reader<R>, realtime: bool, step: f64) -> Self {
        let next = reader.next_record();
        Self {
            reader,
            next,
            start: Instant::now(),
            realtime,
            clock: 0.0,
            step,
            finished: false,
        }
    }
}

impl<R: Read + Send> Source for Replay<R> {
    fn drain(&mut self, out: &mut Vec<Raw>) {
        // finished only once a drain finds nothing left, so the last records still count as
        // connected data
        self.finished = self.next.is_none();
        self.clock = if self.realtime {
            self.start.elapsed().as_secs_f64()
        } else {
            self.clock + self.step
        };
        while let Some((t, _)) = &self.next {
            if *t > self.clock {
                return;
            }
            let (_, raw) = self.next.take().unwrap();
            out.push(raw);
            self.next = self.reader.next_record();
        }
    }
    fn send(&mut self, _datagram: &[u8]) {}
    fn clock(&self) -> f64 {
        self.clock
    }
    fn connected(&self) -> bool {
        !self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_pages_and_datagrams() {
        let mut buf = Vec::new();
        let mut w = Writer::new(&mut buf).unwrap();
        let mut page = vec![0u8; 800];
        for k in 0..50u32 {
            page[0..4].copy_from_slice(&k.to_le_bytes());
            w.write(k as f64 * 0.01, &Raw::Physics(page.clone()))
                .unwrap();
        }
        w.write(0.6, &Raw::Udp(vec![3, 1, 2])).unwrap();
        w.finish().unwrap();
        assert!(
            buf.len() < 2_000,
            "50 pages compress to {} bytes",
            buf.len()
        );

        let mut r = Reader::new(buf.as_slice()).unwrap();
        for k in 0..50u32 {
            let (t, Raw::Physics(p)) = r.next_record().unwrap() else {
                panic!("expected a physics page")
            };
            assert!((t - k as f64 * 0.01).abs() < 1e-6);
            assert_eq!(u32::from_le_bytes(p[0..4].try_into().unwrap()), k);
            assert_eq!(p.len(), 800);
        }
        assert_eq!(
            r.next_record().map(|(_, raw)| raw),
            Some(Raw::Udp(vec![3, 1, 2]))
        );
        assert!(r.next_record().is_none());
    }

    #[test]
    fn a_recording_cut_short_replays_up_to_the_last_flush() {
        let mut buf = Vec::new();
        let mut w = Writer::new(&mut buf).unwrap();
        for k in 0..30 {
            w.write(k as f64 * 0.1, &Raw::Udp(vec![k as u8; 40]))
                .unwrap();
        }
        w.out.flush().unwrap();
        drop(w); // no finish(): like a killed process
        let mut r = Reader::new(buf.as_slice()).unwrap();
        let mut n = 0;
        while r.next_record().is_some() {
            n += 1;
        }
        assert!(n >= 20, "only {n} records readable");
    }

    #[test]
    fn rejects_other_files() {
        assert!(Reader::new(&b"hello world"[..]).is_err());
    }
}
