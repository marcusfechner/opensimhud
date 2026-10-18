# OpenSimHUD

Open-source HUD overlays for sim racing. Assetto Corsa Competizione comes first, and other sims can be added later. It's a Tauri app: a Rust core reads the game's telemetry, and the overlays are drawn in one transparent, click-through window per display.

## Development

You need Rust (via [rustup](https://rustup.rs)) and Node.js 22 or newer. Tauri's [prerequisites](https://tauri.app/start/prerequisites/) cover the rest. On Windows, that's WebView2, which is already installed on Windows 10 and 11.

```sh
cd app
npm install
npm run tauri dev        # run the app with hot reload
cargo test --workspace   # from the repo root: Rust tests
```

To work in a normal browser, run `npm run demo:frames` once to record a minute of the demo race, then `npm run dev`:

- `http://localhost:1420/` is the settings window.
- `http://localhost:1420/overlay.html` is an overlay window. `?profile=Practice`, `?only=relative` and `?skip=20` choose what you see.

Outside the app, the settings are kept in the browser and shared between tabs. With both pages open side by side, a change in one shows in the other.

## Using the app

- **Settings window:** turns overlays on and off, places them, and sets their look. Changes show in game straight away.
- **Profiles:** each profile keeps which overlays are on, where they sit and how they look. The app can switch profiles when a practice, qualifying or race session starts.
- **Hotkeys:** <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>H</kbd> shows or hides all overlays. <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> lets you drag them around in game; <kbd>Esc</kbd> finishes.
- **Demo mode:** shows a sample race, so you can arrange overlays without the game.
- **Config file:** settings are saved to `config.json` in the app's config folder (`%APPDATA%\io.github.marcusfechner.opensimhud` on Windows). Positions are stored for a 1920 × 1080 screen and scaled to yours.

| Path | Purpose |
|---|---|
| `crates/telemetry` | Game-agnostic telemetry: the normalized model, derived calculations and game adapters. It doesn't depend on Tauri. |
| `crates/ovtool` | Command-line helper to record, replay and dump telemetry. |
| `app/src-tauri` | Tauri shell: config, windows, hotkeys, and moving telemetry to the windows. |
| `app/src/overlay` | Overlay window: one transparent, click-through window per display that holds its overlays. |
| `app/src/shared` | What both windows share: the bridge to the app, defaults, appearance. |
| `app/src/config/defaults.json` | The overlay catalog and the default profiles, read by both Rust and TypeScript. |
| `app/src/app` | Settings window. |

You can develop most features on any OS against the built-in demo data. Testing with Assetto Corsa Competizione itself needs Windows, because ACC shares its telemetry through Windows shared memory.

## Recording ACC sessions

Real sessions are recorded on Windows and can be replayed anywhere, so most ACC work can happen on any OS. A recording (`.ovrec`) holds ACC's raw data: shared memory and the Broadcasting API.

On the Windows PC with ACC (needs [Rust](https://rustup.rs) and this repository):

```powershell
cargo build --release -p ovtool
.\target\release\ovtool setup              # is ACC's Broadcasting API on?
.\target\release\ovtool setup --enable     # if not: turn it on, then restart ACC
.\target\release\ovtool record             # start ACC, drive, press Ctrl+C when done
```

`record` prints one line of live values every second: lap, gear, speed, fuel, TC, ABS, brake bias and the number of cars from the broadcast. Compare them with the game's own HUD; a mismatch means the data is being read wrong. Good first recordings:

- a practice session of a few clean laps, so the track map can be learned
- a race with other cars and a pit stop
- a session in the rain, if you can

Back on any machine:

```sh
cargo run -p ovtool -- replay acc-2026-10-18-2015.ovrec            # summary and lap log
cargo run -p ovtool -- replay acc-….ovrec --frames > app/public/dev/demo-frames.jsonl
```

The second line replaces the browser preview's demo race with the recording. To check the decoding itself, `cargo run --release -p telemetry --example probe -- FILE [SECONDS]` prints the raw shared memory and broadcast values every few seconds, plus every lap, validity and flag change. The app can also play it directly with `OPENSIMHUD_SOURCE=replay:/path/to/file.ovrec npm run tauri dev`.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).

## Mockups

`mockups/` holds clickable HTML mockups of every overlay and of the app window. You don't need a build step or a server. Open the files straight from disk:

- `mockups/index.html` is the gallery. It shows every overlay over a stand-in game screen ("In game", with a Race and a Practice example screen) or as individual cards ("All overlays"). You can pause or speed up the fake race.
- `mockups/overlay.html?o=relative&bg=transparent` opens one overlay on its own, the way it will sit in its own window. `bg` can be `transparent`, `checker` or `scene`, and `scale` resizes the overlay.
- `mockups/app.html` is the settings and launcher window.

### Structure

| Path | Purpose |
|---|---|
| `shared/tokens.css` | Design tokens for the one visual style. Users customize the look (accent color, panel opacity) by overriding tokens. |
| `shared/base.css` | Panel, row, badge and bar primitives that only use tokens. |
| `shared/hud.js` | Overlay registry, the single render loop (each overlay sets its update rate), `HUD.restyle()` for runtime token changes, and DOM helpers. |
| `shared/mock-session.js` | Fake race that fills the normalized telemetry model (`HUD.state`). This model is the contract the real game adapters will provide. |
| `shared/fmt.js` | Formatting for lap times, gaps and deltas. |
| `overlays/<id>.js/.css` | One overlay each. Registered with `HUD.register(id, { w, hz, mount })`. |

### Performance rules the mockups follow

- Each overlay builds its DOM once. Updates only change `textContent`, transforms, classes or `data-*` on cached nodes, and only when a value actually changed.
- Everything that is plotted (inputs trace, radar, track map) is drawn on a canvas. Static layers are cached.
- Animations only use `transform` and `opacity`. `backdrop-filter` is opt-in because it is expensive to draw over a game.
