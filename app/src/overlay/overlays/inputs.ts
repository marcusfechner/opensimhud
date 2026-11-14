/* Inputs: scrolling throttle/brake/steer trace, pedal bars, gear, speed and wheel angle.

   Resizing makes the trace wider (everything else stays as it is), and a wider trace shows
   more seconds at the same speed. The colors and the trace length are options.

   The trace is placed by telemetry time, not by how often it gets drawn: the overlay window
   can get fewer frames while the game is busy, and the trace must still cover the same
   seconds at the same speed (and stand still while the game is paused). */
import "./inputs.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

interface Colors {
  thr: string;
  brk: string;
  steer: string;
  abs: string;
  tc: string;
  grid: string;
  thrFill: CanvasGradient | string;
  brkFill: CanvasGradient | string;
}

HUD.register("inputs", {
  title: "Inputs",
  group: "core",
  w: 420,
  hz: 60,
  resize: "width",
  minW: 320,
  options: [
    { key: "seconds", label: "Trace length", type: "range", min: 2, max: 20, step: 1, default: 6, unit: " s",
      hint: "Seconds shown at the usual width; a wider trace shows more" },
    { key: "throttle", label: "Throttle", type: "color", default: "#3ddc84" },
    { key: "brake", label: "Brake", type: "color", default: "#ff5a5f" },
    { key: "clutch", label: "Clutch", type: "color", default: "#4c9dff" },
    { key: "steer", label: "Steering", type: "color", default: "#e9edf2" },
  ],
  mount(root, _state, opts) {
    const { h } = HUD;
    const o = opts.options;
    // the trace takes whatever width the rest leaves
    const BASE_CW = 250, CW = BASE_CW + opts.w - 420, CH = 88;
    const WINDOW = ((o.seconds as number) * CW) / BASE_CW; // seconds shown, at a steady speed
    const LEN = Math.ceil(WINDOW * 90) + 16; // samples kept: telemetry comes at about 60 Hz
    const PAD = 4;
    for (const [v, key] of [["--in-throttle", "throttle"], ["--in-brake", "brake"], ["--in-clutch", "clutch"], ["--in-steer", "steer"]]) {
      root.style.setProperty(v, o[key] as string);
    }

    const canvas = h("canvas.in-trace") as HTMLCanvasElement;
    const ctx = HUD.canvas(canvas, CW, CH);
    const bg = document.createElement("canvas");
    const bgCtx = HUD.canvas(bg, CW, CH);

    const mkBar = (cls: string) => {
      const fill = h("i");
      const el = h("div.bar.v." + cls, null, fill);
      return { el, fill };
    };
    const thrBar = mkBar("thr"), brkBar = mkBar("brk"), cluBar = mkBar("clu");

    const gearEl = h("span.in-gear.num");
    const speedEl = h("span.in-speed.num");
    const wheel = h("div.in-wheel", null, h("i"));
    const absEl = h("span.in-flag.abs", { text: "ABS" });
    const tcEl = h("span.in-flag.tc", { text: "TC" });
    const flags = h("div.in-flags", null, tcEl, absEl);

    root.append(h("div.hud-panel", null,
      h("div.hud-body.in-body", null,
        canvas,
        h("div.in-bars", null, cluBar.el, brkBar.el, thrBar.el),
        h("div.in-box", null,
          h("div.in-gearrow", null, gearEl, wheel),
          h("div.in-speedrow", null, speedEl, h("span.unit", { text: "km/h" })),
          flags))));

    // ring buffers, oldest sample at `head` once full
    const time = new Float64Array(LEN);
    const thr = new Float32Array(LEN), brk = new Float32Array(LEN), steer = new Float32Array(LEN);
    const abs = new Uint8Array(LEN), tc = new Uint8Array(LEN);
    let head = 0, count = 0, lastT = -Infinity;
    // x of every kept sample for this draw (NaN = older than the window)
    const xs = new Float32Array(LEN);
    let first = 0; // index (0..count) of the first sample to draw

    const yOf = (v: number) => PAD + (CH - 2 * PAD) * (1 - v);
    function rgba(hex: string, a: number) {
      const m = /^#?([0-9a-f]{6})$/i.exec(hex);
      if (!m) return hex;
      const n = parseInt(m[1], 16);
      return "rgba(" + (n >> 16) + "," + ((n >> 8) & 255) + "," + (n & 255) + "," + a + ")";
    }
    function shade(color: string) {
      const g = ctx.createLinearGradient(0, yOf(1), 0, yOf(0));
      g.addColorStop(0, rgba(color, 0.42));
      g.addColorStop(1, rgba(color, 0.04));
      return g;
    }

    let col: Colors;
    function readColors() {
      const c = {
        thr: o.throttle as string,
        brk: o.brake as string,
        steer: o.steer as string,
        abs: HUD.css("--in-abs"),
        tc: HUD.css("--in-tc"),
        grid: HUD.css("--track-line"),
      };
      // area shading under the traces: built once per restyle, reused every frame
      col = { ...c, thrFill: shade(c.thr), brkFill: shade(c.brk) };
      bgCtx.clearRect(0, 0, CW, CH);
      bgCtx.strokeStyle = col.grid;
      bgCtx.lineWidth = 1;
      bgCtx.globalAlpha = 0.5;
      for (const f of [0.25, 0.5, 0.75]) {
        const y = Math.round(PAD + (CH - 2 * PAD) * f) + 0.5;
        bgCtx.beginPath();
        bgCtx.setLineDash(f === 0.5 ? [] : [2, 4]);
        bgCtx.moveTo(0, y);
        bgCtx.lineTo(CW, y);
        bgCtx.stroke();
      }
      bgCtx.globalAlpha = 1;
    }
    /** Ring index of the i-th kept sample, oldest first. */
    const at = (i: number) => (head - count + i + LEN) % LEN;

    /** x of each sample from its age; the newest sits at the right edge. */
    function layout() {
      const now = time[at(count - 1)];
      first = count;
      for (let i = count - 1; i >= 0; i--) {
        const age = now - time[at(i)];
        xs[i] = CW * (1 - age / WINDOW);
        first = i;
        if (age > WINDOW) break; // one sample past the edge keeps the line running to x = 0
      }
    }
    function area(buf: Float32Array, fill: CanvasGradient | string) {
      if (count - first < 2) return;
      ctx.fillStyle = fill;
      ctx.beginPath();
      ctx.moveTo(xs[first], yOf(0));
      for (let i = first; i < count; i++) ctx.lineTo(xs[i], yOf(buf[at(i)]));
      ctx.lineTo(xs[count - 1], yOf(0));
      ctx.closePath();
      ctx.fill();
    }
    function line(buf: Float32Array, color: string, width: number, alpha: number) {
      if (count - first < 2) return;
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.globalAlpha = alpha;
      ctx.beginPath();
      for (let i = first; i < count; i++) {
        const y = yOf(buf[at(i)]);
        if (i === first) ctx.moveTo(xs[i], y); else ctx.lineTo(xs[i], y);
      }
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    function ticks(buf: Uint8Array, color: string, y: number) {
      ctx.fillStyle = color;
      for (let i = first; i < count; i++) {
        if (buf[at(i)] && xs[i] >= 0) ctx.fillRect(xs[i], y, 1.5, 3);
      }
    }

    readColors();

    function draw() {
      ctx.clearRect(0, 0, CW, CH);
      ctx.drawImage(bg, 0, 0, CW, CH);
      if (!count) return;
      layout();
      ctx.lineJoin = "round";
      area(brk, col.brkFill);
      area(thr, col.thrFill);
      line(steer, col.steer, 1, 0.45);
      line(thr, col.thr, 2, 1);
      line(brk, col.brk, 2, 1);
      ticks(tc, col.tc, 0);
      ticks(abs, col.abs, CH - 3);
    }

    const STEER_LOCK = 240; // degrees shown at full lock (visual only)

    return {
      update(s: ViewState) {
        const p = s.player;
        // a sample per new telemetry frame; time going back means a new session or source
        if (s.t < lastT - 1) count = 0;
        if (s.t !== lastT) {
          lastT = s.t;
          time[head] = s.t;
          thr[head] = p.throttle;
          brk[head] = p.brake;
          steer[head] = 0.5 + p.steer * 0.5;
          abs[head] = p.absActive ? 1 : 0;
          tc[head] = p.tcActive ? 1 : 0;
          head = (head + 1) % LEN;
          count = Math.min(LEN, count + 1);
          draw();
        }

        thrBar.fill.style.transform = "scaleY(" + p.throttle + ")";
        brkBar.fill.style.transform = "scaleY(" + p.brake + ")";
        cluBar.fill.style.transform = "scaleY(" + p.clutch + ")";
        wheel.style.transform = "rotate(" + (p.steer * STEER_LOCK).toFixed(1) + "deg)";
        HUD.text(gearEl, F.gear(p.gear));
        HUD.text(speedEl, F.int(p.speed));
        HUD.cls(absEl, "on", p.absActive);
        HUD.cls(tcEl, "on", p.tcActive);
      },
      theme() {
        readColors();
        draw();
      },
    };
  },
});
