/* Track map: circuit outline (cached), cars as class-colored dots, player emphasized.
   The outline can be unknown at first (ACC and EVO learn it from a lap): then a calm note
   shows, and everything is rebuilt as soon as a new track object arrives. */
import "./trackmap.css";
import { HUD } from "../hud";
import type { ViewCar, ViewState, ViewTrack } from "../view";

interface Geometry {
  n: number;
  sx: Float32Array;
  sy: Float32Array;
  heading: Float32Array;
  sec: number[];
  full: Path2D;
  sectorPaths: Path2D[];
  pit: Path2D;
}

HUD.register("trackmap", {
  title: "Track map",
  group: "awareness",
  w: 300,
  h: 210,
  hz: 20,
  resize: "both",
  minW: 200,
  minH: 150,
  mount(root, state, opts) {
    const { h } = HUD;
    const W = opts.w, H = opts.h ?? 210, PAD = 20;

    const canvas = h("canvas") as HTMLCanvasElement;
    const name = h("span.tm-name");
    const len = h("span.tm-len.num");
    const label = h("div.tm-label", null, name, len);
    const empty = h("div.tm-empty", { text: "Track map appears after one lap" });
    const panel = h("div.hud-panel.tm-panel", null, canvas, label, empty);
    root.append(panel);

    const ctx = HUD.canvas(canvas, W, H);
    const off = document.createElement("canvas");

    /* fit the track into the box (leave room for the label at the top) */
    function build(track: ViewTrack): Geometry | null {
      const n = track.n;
      if (n < 3) return null;
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (let i = 0; i < n; i++) {
        const x = track.x[i], y = track.y[i];
        if (x < minX) minX = x; if (x > maxX) maxX = x;
        if (y < minY) minY = y; if (y > maxY) maxY = y;
      }
      const top = PAD + 12;
      const sc = Math.min((W - PAD * 2) / (maxX - minX || 1), (H - top - PAD) / (maxY - minY || 1));
      const ox = (W - (maxX - minX) * sc) / 2 - minX * sc;
      const oy = top + (H - top - PAD - (maxY - minY) * sc) / 2 - minY * sc;
      const sx = new Float32Array(n), sy = new Float32Array(n);
      for (let i = 0; i < n; i++) { sx[i] = track.x[i] * sc + ox; sy[i] = track.y[i] * sc + oy; }

      const pathRange = (a: number, b: number) => {
        // a, b: pct (0..1), wraps around
        const p = new Path2D();
        const i0 = Math.floor(a * n), i1 = Math.floor(b * n);
        const count = ((i1 - i0 + n) % n) || n;
        p.moveTo(sx[i0 % n], sy[i0 % n]);
        for (let k = 1; k <= count; k++) { const i = (i0 + k) % n; p.lineTo(sx[i], sy[i]); }
        return p;
      };
      const full = pathRange(0, 0);
      full.closePath();
      const sec = track.sectors;
      const sectorPaths = sec.map((s, k) => pathRange(s, k + 1 < sec.length ? sec[k + 1] : 1));

      // pit lane: offset to the inside of the main straight (positive normal = right of travel)
      const pit = new Path2D();
      const from = 0.955, to = 1.045, steps = 60, offPx = 7;
      for (let k = 0; k <= steps; k++) {
        const pct = (from + ((to - from) * k) / steps) % 1;
        const i = Math.floor(pct * n);
        const hd = track.heading[i];
        const x = sx[i] - Math.sin(hd) * offPx, y = sy[i] + Math.cos(hd) * offPx;
        if (k) pit.lineTo(x, y); else pit.moveTo(x, y);
      }
      return { n, sx, sy, heading: track.heading, sec, full, sectorPaths, pit };
    }

    function normalAt(g: Geometry, pct: number) {
      const i = Math.floor(pct * g.n) % g.n;
      const hd = g.heading[i];
      return { x: g.sx[i], y: g.sy[i], nx: -Math.sin(hd), ny: Math.cos(hd) };
    }

    let col: Record<string, string> = {};
    function readColors() {
      col = {
        text: HUD.css("--text"),
        dim: HUD.css("--text-dim"),
        faint: HUD.css("--text-faint"),
        edge: HUD.css("--track-edge"),
        line: HUD.css("--track-line"),
        accent: HUD.css("--accent"),
        accent2: HUD.css("--accent-2"),
        accentText: HUD.css("--accent-text"),
        yellow: HUD.css("--flag-yellow"),
        panel: HUD.css("--panel-bg"),
        font: HUD.css("--font-num"),
      };
    }

    function drawStatic(g: Geometry | null) {
      const c = HUD.canvas(off, W, H);
      c.clearRect(0, 0, W, H);
      if (!g) return;
      c.lineJoin = c.lineCap = "round";

      // pit lane
      c.strokeStyle = col.line;
      c.lineWidth = 2;
      c.setLineDash([3, 3]);
      c.stroke(g.pit);
      c.setLineDash([]);

      // soft accent glow under a crisp edge line
      const grad = c.createLinearGradient(0, 0, W, H);
      grad.addColorStop(0, col.accent);
      grad.addColorStop(1, col.accent2);
      c.globalAlpha = 0.25; c.strokeStyle = grad; c.lineWidth = 10; c.stroke(g.full);
      c.globalAlpha = 1; c.strokeStyle = col.edge; c.lineWidth = 3; c.stroke(g.full);

      // sector boundary ticks
      c.strokeStyle = col.accent;
      c.lineWidth = 2;
      for (let k = 1; k < g.sec.length; k++) {
        const p = normalAt(g, g.sec[k]);
        c.beginPath();
        c.moveTo(p.x - p.nx * 7, p.y - p.ny * 7);
        c.lineTo(p.x + p.nx * 7, p.y + p.ny * 7);
        c.stroke();
      }
      // start / finish: chequered bar
      const sf = normalAt(g, 0);
      for (let k = -4; k < 4; k++) {
        c.fillStyle = k % 2 ? "#fff" : "#111";
        c.beginPath();
        const x0 = sf.x + sf.nx * k * 1.8, y0 = sf.y + sf.ny * k * 1.8;
        c.arc(x0, y0, 1.3, 0, Math.PI * 2);
        c.fill();
      }
      c.strokeStyle = "#fff";
      c.lineWidth = 2;
      c.beginPath();
      c.moveTo(sf.x - sf.nx * 8, sf.y - sf.ny * 8);
      c.lineTo(sf.x + sf.nx * 8, sf.y + sf.ny * 8);
      c.stroke();

      // sector numbers, placed outside the track
      c.font = "600 10px " + col.font;
      c.fillStyle = col.faint;
      c.textAlign = "center";
      c.textBaseline = "middle";
      for (let k = 0; k < g.sec.length; k++) {
        const mid = (g.sec[k] + (k + 1 < g.sec.length ? g.sec[k + 1] : 1)) / 2;
        const p = normalAt(g, mid);
        c.fillText("S" + (k + 1), p.x - p.nx * 16, p.y - p.ny * 16);
      }
    }

    const clsColor = (cls: string) => HUD.classColor(cls);

    // interpolate along the screen-space polyline (lateral offset is too small to matter at this scale)
    let PX = 0, PY = 0;
    function posOf(g: Geometry, c: ViewCar) {
      const f = c.pct * g.n, i = Math.floor(f) % g.n, j = (i + 1) % g.n, t = f - Math.floor(f);
      PX = g.sx[i] + (g.sx[j] - g.sx[i]) * t;
      PY = g.sy[i] + (g.sy[j] - g.sy[i]) * t;
    }

    let track: ViewTrack | null = null;
    let geo: Geometry | null = null;
    function setTrack(t: ViewTrack) {
      track = t;
      geo = build(t);
      HUD.text(name, t.name);
      HUD.text(len, t.length > 1 ? (t.length / 1000).toFixed(2) + " km" : "");
      empty.hidden = !!geo;
      drawStatic(geo);
    }

    function render(s: ViewState) {
      if (s.track !== track) setTrack(s.track);
      ctx.clearRect(0, 0, W, H);
      const g = geo;
      if (!g) return;
      ctx.drawImage(off, 0, 0, W, H);

      const ses = s.session;
      const yellow = g.sectorPaths[ses.yellowSector - 1];
      if (ses.flag === "yellow" && yellow) {
        ctx.lineJoin = ctx.lineCap = "round";
        ctx.strokeStyle = col.yellow;
        ctx.globalAlpha = 0.85;
        ctx.lineWidth = 5;
        ctx.stroke(yellow);
        ctx.globalAlpha = 1;
      }

      const cars = s.cars;
      const r = 4;
      for (let k = 0; k < cars.length; k++) {
        const c = cars[k];
        if (c.isPlayer || !c.located) continue;
        posOf(g, c);
        ctx.globalAlpha = c.inPit ? 0.35 : 1;
        ctx.beginPath();
        ctx.arc(PX, PY, r, 0, Math.PI * 2);
        ctx.fillStyle = clsColor(c.cls);
        ctx.fill();
        ctx.lineWidth = 1.2;
        ctx.strokeStyle = "rgba(0,0,0,0.65)";
        ctx.stroke();
      }
      ctx.globalAlpha = 1;

      // player last so it sits on top
      const me = s.player.car;
      posOf(g, me);
      const px = PX, py = PY;
      ctx.beginPath();
      ctx.arc(px, py, 9, 0, Math.PI * 2);
      ctx.fillStyle = col.accent;
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = "rgba(0,0,0,0.6)";
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(px, py, 12.5, 0, Math.PI * 2);
      ctx.strokeStyle = col.accent;
      ctx.globalAlpha = 0.45;
      ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.fillStyle = col.accentText;
      ctx.font = "700 9px " + col.font;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      if (me.number > 0) ctx.fillText("" + me.number, px, py + 0.5);
    }

    readColors();
    setTrack(state.track);
    let lastState: ViewState | null = null;

    return {
      update(s: ViewState) { lastState = s; render(s); },
      theme() {
        readColors();
        drawStatic(geo);
        if (lastState) render(lastState);
      },
    };
  },
});
