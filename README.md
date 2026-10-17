# Overlays

Open-source HUD overlays for sim racing. Assetto Corsa Competizione comes first, and other sims can be added later. The plan is a Tauri app: a Rust core reads the game's telemetry, and each overlay is an HTML/CSS/JS page in its own transparent window.

## Development

You need Rust (via [rustup](https://rustup.rs)) and Node.js 22 or newer. Tauri's [prerequisites](https://tauri.app/start/prerequisites/) cover the rest. On Windows, that's WebView2, which is already installed on Windows 10 and 11.

```sh
cd app
npm install
npm run tauri dev        # run the app with hot reload
cargo test --workspace   # from the repo root: Rust tests
```

| Path | Purpose |
|---|---|
| `crates/telemetry` | Game-agnostic telemetry: the normalized model, derived calculations and game adapters. It doesn't depend on Tauri. |
| `crates/ovtool` | Command-line helper to record, replay and dump telemetry. |
| `app/src-tauri` | Tauri shell: windows, config, hotkeys, and moving telemetry to the overlay window. |
| `app/src/overlay` | Overlay window: one transparent, click-through window per monitor that holds every overlay. |
| `app/src/app` | Settings window. |

You can develop most features on any OS against the built-in demo data. Testing with Assetto Corsa Competizione itself needs Windows, because ACC shares its telemetry through Windows shared memory.

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
