//! What the overlays need from the PC beyond the game: the media player that is playing
//! (title, artist, play/pause, skip), and game controller buttons, so a button on the wheel
//! can skip a song. Both work on Windows; elsewhere they report nothing.

pub mod buttons;
pub mod player;

pub use buttons::{Buttons, Press};
pub use player::{Command, NowPlaying, Player};

/// Prepare the calling thread for [`Player`] and [`Buttons`]: both need COM on Windows.
pub fn init_thread() {
    #[cfg(windows)]
    // SAFETY: initializing COM for this thread; a second call is harmless
    unsafe {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}
