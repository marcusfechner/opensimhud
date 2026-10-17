/* Lap time graph: the player's laps plus the cars directly ahead and behind in class.
   Redraws only when one of the plotted lap lists changes. */
import "./lapgraph.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { LapEntry } from "../../types/telemetry/LapEntry";
import type { ViewCar, ViewState } from "../view";

interface Point {
  lap: number;
  time: number;
  pit: boolean;
  invalid: boolean;
  /** Counts for the y range, the mean and the line (not lap 1, not pit or invalid). */
  ok: boolean;
}
type Series = Point[];

HUD.register("lapgraph", {
  title: "Lap time graph",
  group: "core",
  w: 420,
  hz: 1,
  mount(root) {
    const { h } = HUD;
    const CW = 392, CH = 150;
    const M = { l: 44, r: 50, t: 8, b: 18 };   // plot margins (right gutter holds the mean label)
    const SHOW = 20;                            // laps on the x axis

    const legYou = h("span.lg-key.you", null, h("i"), h("span", { text: "You" }));
    const legAhead = h("span.lg-key.ahead", null, h("i"), h("span"));
    const legBehind = h("span.lg-key.behind", null, h("i"), h("span"));
    const canvas = h("canvas.lg-canvas") as HTMLCanvasElement;
    const ctx = HUD.canvas(canvas, CW, CH);

    root.append(h("div.hud-panel", null,
      h("div.hud-head", null, h("span.title", { text: "Lap times" }), h("span.spacer"), legYou, legAhead, legBehind),
      h("div.hud-body.lg-body", null, canvas)));

    let col: Record<string, string> = {};
    function readColors() {
      col = {
        you: HUD.css("--accent"),
        you2: HUD.css("--accent-2"),
        ahead: HUD.css("--warn"),
        behind: HUD.css("--text-dim"),
        pit: HUD.css("--info"),
        pb: HUD.css("--pb"),
        grid: HUD.css("--track-line"),
        label: HUD.css("--text-faint"),
        text: HUD.css("--text-dim"),
        font: HUD.css("--font-num"),
      };
    }
    readColors();

    function playerSeries(log: LapEntry[]): Series {
      return log.map((e) => ({ lap: e.lap, time: e.time, pit: !!e.pit, invalid: !e.valid, ok: e.valid && !e.pit && e.lap > 1 }));
    }
    function carSeries(c: ViewCar | null): Series | null {
      if (!c) return null;
      const first = c.laps - c.lapTimes.length + 1;
      return c.lapTimes.map((time, i) => {
        const lap = first + i;
        const pit = c.pitLaps.includes(lap);
        return { lap, time, pit, invalid: false, ok: !pit && lap > 1 };
      });
    }

    const fmtAxis = (sec: number) => {
      const m = Math.floor(sec / 60), s = sec - m * 60;
      return m + ":" + (s < 10 ? "0" : "") + s.toFixed(1);
    };

    function draw(you: Series, ahead: Series | null, behind: Series | null) {
      ctx.clearRect(0, 0, CW, CH);
      const lastLap = you.length ? you[you.length - 1].lap : 1;
      const x0 = Math.max(2, lastLap - SHOW + 1), x1 = Math.max(x0 + 4, lastLap);
      const inView = (p: Point) => p.lap >= x0 && p.lap <= x1;

      // y range from clean laps only (player first, others inside a sane window)
      let lo = Infinity, hi = -Infinity;
      for (const p of you) if (p.ok && inView(p)) { lo = Math.min(lo, p.time); hi = Math.max(hi, p.time); }
      if (!isFinite(lo)) { lo = 100; hi = 110; }
      for (const s of [ahead, behind]) {
        if (!s) continue;
        for (const p of s) if (p.ok && inView(p) && Math.abs(p.time - lo) < 3) { lo = Math.min(lo, p.time); hi = Math.max(hi, p.time); }
      }
      const pad = Math.max(0.25, (hi - lo) * 0.18);
      lo -= pad; hi += pad;

      const X = (lap: number) => M.l + ((lap - x0) / (x1 - x0)) * (CW - M.l - M.r);
      const Y = (t: number) => M.t + (1 - (t - lo) / (hi - lo)) * (CH - M.t - M.b);
      const inY = (t: number) => t >= lo && t <= hi;

      // grid: 3–4 "nice" steps
      const span = hi - lo;
      const step = [0.1, 0.2, 0.25, 0.5, 1, 2, 5].find((st) => span / st <= 4) || 10;
      ctx.font = "600 10px " + col.font;
      ctx.textBaseline = "middle";
      ctx.textAlign = "right";
      ctx.lineWidth = 1;
      for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) {
        const y = Math.round(Y(v)) + 0.5;
        ctx.strokeStyle = col.grid;
        ctx.globalAlpha = 0.6;
        ctx.setLineDash([2, 4]);
        ctx.beginPath(); ctx.moveTo(M.l, y); ctx.lineTo(CW - M.r, y); ctx.stroke();
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
        ctx.fillStyle = col.label;
        ctx.fillText(fmtAxis(v), M.l - 6, y);
      }
      // x labels every 5 laps
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      for (let l = x0; l <= x1; l++) {
        // every 5th lap, plus the first one if it isn't crowding a 5th
        if (l % 5 && !(l === x0 && 5 - (l % 5) >= 3)) continue;
        ctx.fillStyle = col.label;
        ctx.fillText("L" + l, X(l), CH - 4);
      }

      // mean of your clean laps in view: the graph's middle line
      let sum = 0, n = 0;
      let avgLabel: { text: string; y: number } | null = null;
      for (const p of you) if (p.ok && inView(p)) { sum += p.time; n++; }
      if (n) {
        const mean = sum / n, y = Math.round(Y(mean)) + 0.5;
        ctx.strokeStyle = col.text;
        ctx.globalAlpha = 0.45;
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(M.l, y); ctx.lineTo(CW - M.r, y); ctx.stroke();
        ctx.globalAlpha = 1;
        avgLabel = { text: F.lap(mean).slice(0, -1), y }; // 1:47.70
      }

      /* Smooth curve through points (monotone cubic, Fritsch–Carlson):
         no overshoot, so the line never suggests a lap time that wasn't driven. */
      function smooth(pts: [number, number][]) {
        const n = pts.length;
        ctx.moveTo(pts[0][0], pts[0][1]);
        if (n === 1) return;
        const d: number[] = [], m: number[] = new Array(n);
        for (let i = 0; i < n - 1; i++) d.push((pts[i + 1][1] - pts[i][1]) / (pts[i + 1][0] - pts[i][0]));
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for (let i = 1; i < n - 1; i++) m[i] = d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2;
        for (let i = 0; i < n - 1; i++) {
          if (d[i] === 0) { m[i] = m[i + 1] = 0; continue; }
          const a = m[i] / d[i], b = m[i + 1] / d[i], h = a * a + b * b;
          if (h > 9) { const t = 3 / Math.sqrt(h); m[i] = t * a * d[i]; m[i + 1] = t * b * d[i]; }
        }
        for (let i = 0; i < n - 1; i++) {
          const [x0, y0] = pts[i], [x1, y1] = pts[i + 1], dx = (x1 - x0) / 3;
          ctx.bezierCurveTo(x0 + dx, y0 + m[i] * dx, x1 - dx, y1 - m[i + 1] * dx, x1, y1);
        }
      }
      // one series as a smooth line through clean laps; pit laps break the line
      function line(series: Series | null, color: string | CanvasGradient, width: number, alpha: number) {
        if (!series) return;
        ctx.strokeStyle = color;
        ctx.lineWidth = width;
        ctx.globalAlpha = alpha;
        ctx.lineJoin = ctx.lineCap = "round";
        ctx.beginPath();
        let run: [number, number][] = [];
        const flush = () => { if (run.length) smooth(run); run = []; };
        for (const p of series) {
          if (!inView(p)) continue;
          if (!p.ok || !inY(p.time)) { if (p.pit) flush(); continue; }
          run.push([X(p.lap), Y(p.time)]);
        }
        flush();
        ctx.stroke();
        ctx.globalAlpha = 1;
      }
      line(ahead, col.ahead, 1.25, 0.7);
      line(behind, col.behind, 1.25, 0.7);

      const g = ctx.createLinearGradient(M.l, 0, CW - M.r, 0);
      g.addColorStop(0, col.you);
      g.addColorStop(1, col.you2);
      line(you, g, 2, 1);

      // player dots: filled = clean, hollow = invalid, P marker = pit
      let pb: Point | null = null;
      for (const p of you) if (p.ok && (!pb || p.time < pb.time)) pb = p;
      for (const p of you) {
        if (!inView(p)) continue;
        const x = X(p.lap);
        if (p.pit) {
          ctx.fillStyle = col.pit;
          ctx.font = "700 9px " + col.font;
          ctx.fillText("P", x, CH - M.b - 3);
          continue;
        }
        if (!inY(p.time)) continue;
        const y = Y(p.time);
        ctx.beginPath();
        ctx.arc(x, y, p === pb ? 4 : 2.6, 0, Math.PI * 2);
        if (p.invalid) {
          ctx.strokeStyle = col.text; ctx.lineWidth = 1.2; ctx.stroke();
        } else {
          ctx.fillStyle = p === pb ? col.pb : col.you; ctx.fill();
        }
      }
      if (pb && inView(pb)) {
        const x = X(pb.lap), y = Y(pb.time);
        ctx.strokeStyle = col.pb;
        ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.arc(x, y, 7, 0, Math.PI * 2); ctx.stroke();
        ctx.fillStyle = col.pb;
        ctx.font = "700 10px " + col.font;
        ctx.textAlign = "center";
        ctx.fillText("PB", x, y + (y > CH / 2 ? -11 : 19));
      }

      // mean label in the right gutter, at the end of the mean line (never covers data)
      if (avgLabel) {
        const { text, y } = avgLabel, x = CW - M.r + 6;
        ctx.textAlign = "left";
        ctx.textBaseline = "middle";
        ctx.fillStyle = col.label;
        ctx.font = "600 9px " + col.font;
        ctx.fillText("Avg", x, y - 7);
        ctx.fillStyle = col.text;
        ctx.font = "700 10px " + col.font;
        ctx.fillText(text, x, y + 5);
      }
    }

    let lastSig = "";
    let cache: [Series, Series | null, Series | null] | null = null;
    function classNeighbours(s: ViewState) {
      const me = s.player.car;
      let ahead: ViewCar | null = null, behind: ViewCar | null = null;
      for (const c of s.order) {
        if (c.cls !== me.cls) continue;
        if (c.classPos === me.classPos - 1) ahead = c;
        if (c.classPos === me.classPos + 1) behind = c;
      }
      return { ahead, behind };
    }
    function legend(el: HTMLElement, label: string, c: ViewCar | null) {
      HUD.cls(el, "is-empty", !c);
      if (c) HUD.text(el.lastChild as HTMLElement, label + " #" + c.number);
    }

    function render(s: ViewState, force: boolean) {
      const { ahead, behind } = classNeighbours(s);
      const log = s.player.log;
      const sig = log.length + "|" + (ahead ? ahead.idx + ":" + ahead.lapTimes.length : "-") + "|" + (behind ? behind.idx + ":" + behind.lapTimes.length : "-");
      legend(legAhead, "Ahead", ahead);
      legend(legBehind, "Behind", behind);
      if (!force && sig === lastSig) return;
      lastSig = sig;
      cache = [playerSeries(log), carSeries(ahead), carSeries(behind)];
      draw(cache[0], cache[1], cache[2]);
    }

    return {
      update(s: ViewState) { render(s, false); },
      theme() {
        readColors();
        if (cache) draw(cache[0], cache[1], cache[2]);
      },
    };
  },
});
