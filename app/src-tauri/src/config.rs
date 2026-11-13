//! The user's settings: profiles (which overlays are on, where, how they look), automatic
//! profile switching, appearance and demo mode. Stored as `config.json` in the app's config
//! folder and pushed to every window when it changes.
//!
//! Defaults and the overlay catalog come from `app/src/config/defaults.json`, which the
//! windows read too.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use controls::{Command, Press};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const DEFAULTS: &str = include_str!("../../src/config/defaults.json");
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../config/")]
pub struct Config {
    pub version: u32,
    /// Name of the profile in use.
    pub profile: String,
    pub profiles: Vec<Profile>,
    pub auto_switch: AutoSwitch,
    pub appearance: Appearance,
    /// Overlays show the demo race instead of the game, for arranging them anywhere.
    pub demo: bool,
    /// Game telemetry source; automatic detection prefers ACC when both games are live.
    pub game: GameSource,
    /// Controller buttons that control the media player.
    pub media_buttons: MediaButtons,
}

/// Game source selected in the app. `Auto` detects ACC and EVO on Windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../config/")]
pub enum GameSource {
    #[default]
    Auto,
    Acc,
    Evo,
}

/// A wheel or button box button for each media player command; `None` is unassigned.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../config/")]
pub struct MediaButtons {
    pub play_pause: Option<Press>,
    pub next: Option<Press>,
    pub previous: Option<Press>,
}

impl MediaButtons {
    pub fn slot(&mut self, c: Command) -> &mut Option<Press> {
        match c {
            Command::PlayPause => &mut self.play_pause,
            Command::Next => &mut self.next,
            Command::Previous => &mut self.previous,
        }
    }

    /// The command bound to a button press.
    pub fn command(&self, p: &Press) -> Option<Command> {
        let is = |b: &Option<Press>| {
            b.as_ref()
                .is_some_and(|b| b.device == p.device && b.button == p.button)
        };
        [
            (Command::PlayPause, &self.play_pause),
            (Command::Next, &self.next),
            (Command::Previous, &self.previous),
        ]
        .into_iter()
        .find(|(_, b)| is(b))
        .map(|(c, _)| c)
    }

    /// Give `c` this button, taking it from any other command.
    pub fn assign(&mut self, c: Command, p: Press) {
        for other in [Command::PlayPause, Command::Next, Command::Previous] {
            let slot = self.slot(other);
            if slot
                .as_ref()
                .is_some_and(|b| b.device == p.device && b.button == p.button)
            {
                *slot = None;
            }
        }
        *self.slot(c) = Some(p);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct Profile {
    pub name: String,
    /// Every overlay of the catalog, keyed by id.
    pub overlays: BTreeMap<String, OverlayConf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../config/")]
pub struct OverlayConf {
    pub on: bool,
    /// Top left corner on a 1920×1080 reference screen.
    pub x: i32,
    pub y: i32,
    /// Which side the overlay keeps to on screens wider (or narrower) than 16:9; `x` is
    /// measured from there. Older configs lack it: then it follows from `x`.
    pub anchor: Option<Anchor>,
    /// Index into the displays, primary first.
    pub display: u32,
    /// Percent.
    pub opacity: u32,
    pub scale: u32,
    pub font: u32,
    /// Size the overlay was resized to, in px at 1080p, for overlays that fit their content
    /// to it (a longer trace, more rows); `None` is the usual size.
    pub w: Option<u32>,
    pub h: Option<u32>,
    /// The overlay's own settings (colors, lengths), as the overlay defines them.
    pub options: BTreeMap<String, String>,
}

impl Default for OverlayConf {
    fn default() -> Self {
        Self {
            on: false,
            x: 0,
            y: 0,
            anchor: None,
            display: 0,
            opacity: 100,
            scale: 100,
            font: 100,
            w: None,
            h: None,
            options: BTreeMap::new(),
        }
    }
}

/// On a screen `extra` reference px wider than 16:9, an overlay sits `extra` further right
/// when anchored right, half that when centered, and stays put when anchored left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../config/")]
pub enum Anchor {
    Left,
    Center,
    Right,
}

impl Anchor {
    /// The side of the 16:9 reference screen an overlay at `x`, `w` wide, is on: by the third
    /// its middle is in.
    pub fn of(x: i32, w: i32) -> Self {
        let cx = x + w / 2;
        if cx < 640 {
            Anchor::Left
        } else if cx > 1280 {
            Anchor::Right
        } else {
            Anchor::Center
        }
    }
}

/// Profile to switch to when a session of this kind starts; `None` keeps the current one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../config/")]
pub struct AutoSwitch {
    pub practice: Option<String>,
    pub qualifying: Option<String>,
    /// Races shorter than 90 minutes.
    pub race: Option<String>,
    /// Races of 90 minutes or more.
    pub endurance: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../config/")]
pub struct Appearance {
    /// `#rrggbb`; `None` is the default violet-to-cyan accent.
    pub accent: Option<String>,
    /// Percent; `None` is the theme's own panel opacity.
    pub panel_opacity: Option<u32>,
}

/* ---------- defaults ---------- */

#[derive(Deserialize)]
struct Defaults {
    overlays: BTreeMap<String, Size>,
    profiles: Vec<DefaultProfile>,
    #[serde(rename = "autoSwitch")]
    auto_switch: AutoSwitch,
}

#[derive(Deserialize)]
struct Size {
    w: i32,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct DefaultProfile {
    name: String,
    overlays: BTreeMap<String, (i32, i32)>,
}

fn defaults() -> Defaults {
    serde_json::from_str(DEFAULTS).expect("defaults.json is valid")
}

/// Overlay ids, sorted.
#[cfg(test)]
pub fn catalog() -> Vec<String> {
    defaults().overlays.into_keys().collect()
}

impl Default for Config {
    fn default() -> Self {
        let d = defaults();
        let profiles = d
            .profiles
            .iter()
            .map(|p| Profile {
                name: p.name.clone(),
                overlays: d
                    .overlays
                    .iter()
                    .map(|(id, s)| {
                        let at = p.overlays.get(id);
                        let (x, y) = at.copied().unwrap_or((s.x, s.y));
                        (
                            id.clone(),
                            OverlayConf {
                                on: at.is_some(),
                                x,
                                y,
                                anchor: Some(Anchor::of(x, s.w)),
                                ..OverlayConf::default()
                            },
                        )
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        Self {
            version: VERSION,
            profile: profiles[0].name.clone(),
            profiles,
            auto_switch: d.auto_switch,
            appearance: Appearance::default(),
            demo: false,
            game: GameSource::Auto,
            media_buttons: MediaButtons::default(),
        }
    }
}

impl Config {
    pub fn active(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.name == self.profile)
            .unwrap_or(&self.profiles[0])
    }

    /// Repair anything a hand edit or an older version could leave behind: at least one
    /// profile, unique names, an existing active profile, every catalog overlay present,
    /// values in range, switch targets that exist.
    pub fn normalize(&mut self) {
        let d = defaults();
        self.version = VERSION;
        if self.profiles.is_empty() {
            self.profiles = Config::default().profiles;
        }
        let mut seen: Vec<String> = Vec::new();
        for p in &mut self.profiles {
            let base = if p.name.trim().is_empty() {
                "Profile".to_owned()
            } else {
                p.name.trim().to_owned()
            };
            let mut name = base.clone();
            let mut n = 2;
            while seen.contains(&name) {
                name = format!("{base} {n}");
                n += 1;
            }
            seen.push(name.clone());
            p.name = name;
            p.overlays.retain(|id, _| d.overlays.contains_key(id));
            for (id, s) in &d.overlays {
                let o = p.overlays.entry(id.clone()).or_insert_with(|| OverlayConf {
                    x: s.x,
                    y: s.y,
                    ..OverlayConf::default()
                });
                o.x = o.x.clamp(-200, 1920);
                o.y = o.y.clamp(-200, 1080);
                o.anchor = o
                    .anchor
                    .or_else(|| Some(Anchor::of(o.x, s.w * o.scale.clamp(60, 200) as i32 / 100)));
                o.display = o.display.min(15);
                o.opacity = o.opacity.clamp(30, 100);
                o.scale = o.scale.clamp(60, 200);
                o.font = o.font.clamp(80, 140);
                o.w = o.w.map(|w| w.clamp(100, 1920));
                o.h = o.h.map(|h| h.clamp(40, 1080));
                // keep what the overlays read small and sane
                o.options
                    .retain(|k, v| k.len() <= 32 && v.len() <= 64 && !k.is_empty());
                while o.options.len() > 32 {
                    o.options.pop_last();
                }
            }
        }
        if !seen.contains(&self.profile) {
            self.profile.clone_from(&seen[0]);
        }
        let a = &mut self.auto_switch;
        for slot in [
            &mut a.practice,
            &mut a.qualifying,
            &mut a.race,
            &mut a.endurance,
        ] {
            if slot.as_ref().is_some_and(|n| !seen.contains(n)) {
                *slot = None;
            }
        }
        let ap = &mut self.appearance;
        if ap
            .accent
            .as_ref()
            .is_some_and(|c| c.len() != 7 || !c.starts_with('#'))
        {
            ap.accent = None;
        }
        ap.panel_opacity = ap.panel_opacity.map(|o| o.clamp(20, 100));
    }

    /// The profile a new session should use, if automatic switching names one.
    pub fn profile_for(&self, kind: telemetry::model::SessionKind, duration: f64) -> Option<&str> {
        use telemetry::model::SessionKind;
        let a = &self.auto_switch;
        match kind {
            SessionKind::Practice | SessionKind::Hotlap => a.practice.as_deref(),
            SessionKind::Qualifying => a.qualifying.as_deref(),
            SessionKind::Race if duration >= 90.0 * 60.0 => a.endurance.as_deref(),
            SessionKind::Race => a.race.as_deref(),
        }
    }
}

/* ---------- file ---------- */

pub fn path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Read the config; a missing file gives the defaults, a broken one is set aside as
/// `config.json.bad` (so nothing the user made is silently lost) and gives the defaults.
pub fn load(dir: &Path) -> Config {
    let file = path(dir);
    let mut c = match std::fs::read_to_string(&file) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("config.json is unreadable ({e}); starting from the defaults");
                let _ = std::fs::rename(&file, file.with_extension("json.bad"));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };
    c.normalize();
    c
}

/// Write via a temporary file so a crash never leaves half a config behind.
pub fn save(dir: &Path, c: &Config) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = path(dir);
    let tmp = file.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(c).map_err(io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use telemetry::model::SessionKind;

    #[test]
    fn a_button_controls_one_command_at_a_time() {
        let press = |button| Press {
            device: "{wheel}".into(),
            name: "Wheel".into(),
            button,
        };
        let mut m = MediaButtons::default();
        m.assign(Command::Next, press(4));
        m.assign(Command::PlayPause, press(5));
        assert_eq!(m.command(&press(4)), Some(Command::Next));
        assert_eq!(m.command(&press(6)), None);
        // the same button on another controller is another button
        let other = Press {
            device: "{box}".into(),
            ..press(4)
        };
        assert_eq!(m.command(&other), None);
        // moving a button to another command frees it from the first
        m.assign(Command::Previous, press(4));
        assert_eq!(m.command(&press(4)), Some(Command::Previous));
        assert!(m.next.is_none());
    }

    #[test]
    fn older_configs_get_the_anchor_their_position_implies() {
        let mut c = Config::default();
        for o in c.profiles[0].overlays.values_mut() {
            o.anchor = None;
        }
        c.normalize();
        let race = Config::default();
        let race = race.profiles.iter().find(|p| p.name == "Race").unwrap();
        assert_eq!(race.overlays["standings"].anchor, Some(Anchor::Left));
        assert_eq!(race.overlays["delta"].anchor, Some(Anchor::Center));
        assert_eq!(race.overlays["relative"].anchor, Some(Anchor::Right));
        assert!(c.profiles[0].overlays.values().all(|o| o.anchor.is_some()));
        // set ones stay
        c.profiles[0].overlays.get_mut("relative").unwrap().anchor = Some(Anchor::Left);
        c.normalize();
        assert_eq!(
            c.profiles[0].overlays["relative"].anchor,
            Some(Anchor::Left)
        );
    }

    #[test]
    fn defaults_hold_every_overlay_in_every_profile() {
        let c = Config::default();
        let ids = catalog();
        assert_eq!(ids.len(), 21);
        for p in &c.profiles {
            assert_eq!(p.overlays.len(), ids.len(), "{}", p.name);
        }
        // a new install starts small: one overlay, no automatic switching
        assert_eq!(c.profile, "My overlays");
        let on: Vec<&str> = c
            .active()
            .overlays
            .iter()
            .filter(|(_, o)| o.on)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(on, ["inputs"]);
        assert_eq!(c.auto_switch, AutoSwitch::default());
        let race = c.profiles.iter().find(|p| p.name == "Race").unwrap();
        assert!(race.overlays["standings"].on && !race.overlays["timer"].on);
        assert_eq!(
            (race.overlays["relative"].x, race.overlays["relative"].y),
            (1516, 739)
        );
        let mut n = c.clone();
        n.normalize();
        assert_eq!(n, c, "defaults are already normal");
    }

    #[test]
    fn normalize_repairs_hand_edits() {
        let mut c = Config::default();
        c.profiles[2].name = "Race".into();
        c.profiles[0].overlays.remove("radar");
        c.profiles[0]
            .overlays
            .insert("retired".into(), OverlayConf::default());
        c.profiles[0].overlays.get_mut("delta").unwrap().opacity = 5;
        c.profile = "Gone".into();
        c.auto_switch.race = Some("Gone".into());
        c.appearance.accent = Some("red".into());
        c.normalize();
        assert_eq!(c.profiles[2].name, "Race 2");
        assert!(c.profiles[0].overlays.contains_key("radar"));
        assert!(!c.profiles[0].overlays.contains_key("retired"));
        assert_eq!(c.profiles[0].overlays["delta"].opacity, 30);
        assert_eq!(c.profile, "My overlays");
        assert_eq!(c.auto_switch.race, None);
        assert_eq!(c.appearance.accent, None);
    }

    #[test]
    fn picks_profiles_by_session() {
        assert_eq!(Config::default().profile_for(SessionKind::Race, 0.0), None);
        let c = Config {
            auto_switch: AutoSwitch {
                practice: Some("Practice".into()),
                qualifying: Some("Qualifying".into()),
                race: Some("Race".into()),
                endurance: Some("Endurance".into()),
            },
            ..Config::default()
        };
        assert_eq!(c.profile_for(SessionKind::Practice, 0.0), Some("Practice"));
        assert_eq!(c.profile_for(SessionKind::Race, 20.0 * 60.0), Some("Race"));
        assert_eq!(
            c.profile_for(SessionKind::Race, 3.0 * 3600.0),
            Some("Endurance")
        );
    }

    #[test]
    fn saves_loads_and_sets_broken_files_aside() {
        let dir = std::env::temp_dir().join(format!("oshud-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), Config::default());
        let c = Config {
            demo: true,
            appearance: Appearance {
                accent: Some("#22d3ee".into()),
                ..Appearance::default()
            },
            ..Config::default()
        };
        save(&dir, &c).unwrap();
        assert_eq!(load(&dir), c);
        std::fs::write(path(&dir), "{ not json").unwrap();
        assert_eq!(load(&dir), Config::default());
        assert!(path(&dir).with_extension("json.bad").exists());
        // a partial file keeps what it has and fills in the rest
        std::fs::write(path(&dir), r#"{"demo": true}"#).unwrap();
        let c = load(&dir);
        assert!(c.demo && c.profiles.len() == 5);
        std::fs::remove_dir_all(dir).ok();
    }
}
