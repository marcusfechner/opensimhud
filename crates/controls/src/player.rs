//! The media player Windows shows in its own media controls (Spotify, a browser tab, …):
//! what is playing, and play/pause and skip. No login or API key: any player that reports to
//! Windows works.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What is playing right now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The player, as Windows names it ("Spotify.exe", "Chrome", …).
    pub app: String,
    pub playing: bool,
    /// Seconds into the track and its length; 0 when the player doesn't say.
    pub position: f64,
    pub duration: f64,
    /// Changes whenever the track does, so the cover is fetched once per track.
    pub track_id: u32,
}

/// What a button can ask the player to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../config/")]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

pub use imp::Player;

#[cfg(windows)]
mod imp {
    use std::time::{SystemTime, UNIX_EPOCH};

    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;

    use super::{Command, NowPlaying};

    /// Windows counts time in 100 ns ticks; DateTime from 1601.
    const TICK: f64 = 1e-7;
    const UNIX_TO_1601: i64 = 116_444_736_000_000_000;

    pub struct Player {
        manager: Manager,
        last: Option<(String, String, String)>,
        track_id: u32,
    }

    impl Player {
        /// `None` when Windows offers no media controls (very old versions). Call from a
        /// thread with COM initialized (multithreaded).
        pub fn new() -> Option<Self> {
            let manager = Manager::RequestAsync().ok()?.join().ok()?;
            Some(Self {
                manager,
                last: None,
                track_id: 0,
            })
        }

        fn session(&self) -> Option<Session> {
            self.manager.GetCurrentSession().ok()
        }

        /// What is playing; `None` when no player is open.
        pub fn now_playing(&mut self) -> Option<NowPlaying> {
            let s = self.session()?;
            let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
            let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
            let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
            let album = props
                .AlbumTitle()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let key = (title.clone(), artist.clone(), album.clone());
            if self.last.as_ref() != Some(&key) {
                self.last = Some(key);
                self.track_id = self.track_id.wrapping_add(1);
            }
            let playing = s
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .is_ok_and(|st| st == Status::Playing);
            let (mut position, mut duration) = (0.0, 0.0);
            if let Ok(tl) = s.GetTimelineProperties() {
                let start = tl.StartTime().map_or(0, |t| t.Duration);
                let end = tl.EndTime().map_or(0, |t| t.Duration);
                let pos = tl.Position().map_or(0, |t| t.Duration);
                duration = (end - start).max(0) as f64 * TICK;
                position = (pos - start).max(0) as f64 * TICK;
                // players report the position now and then; carry it on while playing
                if playing && let Ok(at) = tl.LastUpdatedTime() {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| (d.as_nanos() / 100) as i64)
                        + UNIX_TO_1601;
                    let since = (now - at.UniversalTime) as f64 * TICK;
                    if (0.0..86_400.0).contains(&since) {
                        position += since;
                    }
                }
                if duration > 0.0 {
                    position = position.min(duration);
                }
            }
            Some(NowPlaying {
                title,
                artist,
                album,
                app: s
                    .SourceAppUserModelId()
                    .map(|h| h.to_string())
                    .unwrap_or_default(),
                playing,
                position,
                duration,
                track_id: self.track_id,
            })
        }

        /// The cover of the current track: its media type and bytes.
        pub fn cover(&self) -> Option<(String, Vec<u8>)> {
            let s = self.session()?;
            let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
            let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
            let size = u32::try_from(stream.Size().ok()?).ok()?;
            if size == 0 || size > 4 << 20 {
                return None;
            }
            let mime = stream
                .ContentType()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let reader = DataReader::CreateDataReader(&stream).ok()?;
            reader.LoadAsync(size).ok()?.join().ok()?;
            let mut bytes = vec![0u8; size as usize];
            reader.ReadBytes(&mut bytes).ok()?;
            Some((
                if mime.is_empty() {
                    "image/jpeg".into()
                } else {
                    mime
                },
                bytes,
            ))
        }

        /// Ask the player to play/pause or skip. False when it can't (or nothing is open).
        pub fn send(&self, c: Command) -> bool {
            let Some(s) = self.session() else {
                return false;
            };
            let op = match c {
                Command::PlayPause => s.TryTogglePlayPauseAsync(),
                Command::Next => s.TrySkipNextAsync(),
                Command::Previous => s.TrySkipPreviousAsync(),
            };
            op.and_then(|o| o.join()).unwrap_or(false)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Command, NowPlaying};

    /// Only Windows has the media controls this reads.
    pub struct Player;

    impl Player {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn now_playing(&mut self) -> Option<NowPlaying> {
            None
        }
        pub fn cover(&self) -> Option<(String, Vec<u8>)> {
            None
        }
        pub fn send(&self, _: Command) -> bool {
            false
        }
    }
}
