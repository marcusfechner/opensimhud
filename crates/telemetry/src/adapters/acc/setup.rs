//! `Documents\Assetto Corsa Competizione\Config\broadcasting.json`: ACC only streams other
//! cars' data after a UDP port is set there (the key really is spelled "updListenerPort").
//! ACC writes the file as UTF-16; it reads UTF-8 too, but we keep UTF-16 to match.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub const DEFAULT_PORT: u16 = 9000;
pub const DEFAULT_PASSWORD: &str = "asd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broadcasting {
    /// 0 = broadcasting disabled.
    pub port: u16,
    pub password: String,
}

impl Broadcasting {
    pub fn enabled(&self) -> bool {
        self.port != 0
    }
}

/// Where ACC keeps the file for the current user, if a Documents folder exists.
pub fn config_path() -> Option<PathBuf> {
    Some(
        dirs::document_dir()?
            .join("Assetto Corsa Competizione")
            .join("Config")
            .join("broadcasting.json"),
    )
}

fn decode(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        // UTF-16LE without a BOM: every other byte of ASCII text is zero
        [_, 0, ..] => utf16(bytes, true),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn parse(text: &str) -> io::Result<Map<String, Value>> {
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "broadcasting.json is not a JSON object",
        )),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

pub fn read(path: &Path) -> io::Result<Broadcasting> {
    let map = parse(&decode(&std::fs::read(path)?))?;
    Ok(Broadcasting {
        port: map
            .get("updListenerPort")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u16,
        password: map
            .get("connectionPassword")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

/// Set port and password, keeping any other keys. The old file is kept as `.bak`.
/// ACC has to be restarted to pick the change up.
pub fn enable(path: &Path, port: u16, password: &str) -> io::Result<()> {
    let mut map = match std::fs::read(path) {
        Ok(bytes) => {
            std::fs::write(path.with_extension("json.bak"), &bytes)?;
            parse(&decode(&bytes))?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(e),
    };
    map.insert("updListenerPort".into(), port.into());
    map.insert("connectionPassword".into(), password.into());
    map.entry("commandPassword").or_insert_with(|| "".into());
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(io::Error::other)?;
    let mut out = vec![0xFF, 0xFE];
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ovsetup-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("broadcasting.json")
    }

    #[test]
    fn reads_acc_utf16_file_with_disabled_broadcasting() {
        let p = temp("read");
        let text = "{\n    \"updListenerPort\": 0,\n    \"connectionPassword\": \"\",\n    \"commandPassword\": \"\"\n}";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&p, bytes).unwrap();
        let b = read(&p).unwrap();
        assert!(!b.enabled());
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn enables_broadcasting_and_keeps_other_keys() {
        let p = temp("enable");
        std::fs::write(&p, r#"{"updListenerPort": 0, "connectionPassword": "", "commandPassword": "secret", "extra": 1}"#).unwrap();
        enable(&p, DEFAULT_PORT, DEFAULT_PASSWORD).unwrap();
        let b = read(&p).unwrap();
        assert_eq!(
            b,
            Broadcasting {
                port: 9000,
                password: "asd".into()
            }
        );
        let map = parse(&decode(&std::fs::read(&p).unwrap())).unwrap();
        assert_eq!(map["commandPassword"], "secret");
        assert_eq!(map["extra"], 1);
        assert!(p.with_extension("json.bak").exists());
        assert_eq!(&std::fs::read(&p).unwrap()[..2], &[0xFF, 0xFE]);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn creates_a_missing_file() {
        let p = temp("missing");
        enable(&p, 9001, "pw").unwrap();
        assert_eq!(read(&p).unwrap().port, 9001);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }
}
