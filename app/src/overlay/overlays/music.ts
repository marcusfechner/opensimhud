/* Music: what's playing on this PC (Spotify or any player Windows knows about), with the
   cover and how far into the track. Wheel buttons for play/pause and skip are assigned in
   the app's settings. */
import "./music.css";
import { HUD } from "../hud";
import * as app from "../../shared/bridge";

HUD.register("music", {
  title: "Music",
  group: "awareness",
  w: 320,
  hz: 4,
  mount(root) {
    const { h } = HUD;
    const cover = h("div.mu-cover");
    const title = h("span.mu-title.trunc");
    const artist = h("span.mu-artist.trunc");
    const state = h("span.mu-state");
    const fill = h("i");
    const pos = h("span.num");
    const len = h("span.num");
    root.append(h("div.hud-panel.mu-panel", null,
      cover,
      h("div.mu-text", null,
        h("div.mu-line", null, title, state),
        artist,
        h("div.mu-prog", null, h("div.bar.mu-bar", null, fill), h("div.mu-times", null, pos, len)))));

    // the player reports about once a second; the progress runs on in between
    let now: app.NowPlaying | null = null, at = 0;
    const set = (n: app.NowPlaying | null) => {
      now = n;
      at = performance.now();
    };
    const setCover = (url: string | null) => {
      cover.style.backgroundImage = url ? `url("${url}")` : "";
      HUD.cls(cover, "has-art", !!url);
    };
    void app.getMedia().then((m) => {
      set(m.now);
      setCover(m.cover);
    });
    const offs = [app.onMedia(set), app.onMediaCover(setCover)];
    const clock = (s: number) => Math.floor(s / 60) + ":" + String(Math.floor(s % 60)).padStart(2, "0");

    return {
      update() {
        HUD.cls(root, "is-idle", !now);
        if (!now) {
          HUD.text(title, "Nothing playing");
          HUD.text(artist, "Start music in Spotify or any player");
          HUD.text(pos, "");
          HUD.text(len, "");
          fill.style.transform = "scaleX(0)";
          HUD.data(state, "state", "");
          return;
        }
        HUD.text(title, now.title || "Unknown title");
        HUD.text(artist, [now.artist, now.album].filter(Boolean).join(" · "));
        HUD.data(state, "state", now.playing ? "playing" : "paused");
        const p = Math.min(now.duration || Infinity, now.position + (now.playing ? (performance.now() - at) / 1000 : 0));
        const known = now.duration > 0;
        HUD.text(pos, known ? clock(p) : "");
        HUD.text(len, known ? clock(now.duration) : "");
        fill.style.transform = "scaleX(" + (known ? p / now.duration : 0) + ")";
      },
      destroy() {
        offs.forEach((off) => off());
      },
    };
  },
});
