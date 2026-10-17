# Overlays

Open-source HUD overlays for sim racing. Assetto Corsa Competizione comes first, and other sims can be added later. The plan is a Tauri app: a Rust core reads the game's telemetry, and each overlay is an HTML/CSS/JS page in its own transparent window.

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
