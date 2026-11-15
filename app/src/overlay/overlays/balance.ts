import "./balance.css";
import { HUD } from "../hud";
import type { ViewState } from "../view";
import {
  clamp,
  TRACE_AXIS_LEFT,
  TRACE_AXIS_RIGHT,
  TRACE_CANVAS_HEIGHT,
  TRACE_CAPACITY,
  TRACE_GAP_SECONDS,
  traceCanvasWidth,
  traceStart,
  traceWindowSeconds,
  TraceBuffer,
} from "./sliptrace";

const MIN_SPEED_KMH = 20;
const NEUTRAL_DEG = 0.3;
const TAU_SECONDS: Record<string, number> = { off: 0, light: 0.15, smooth: 0.3 };

HUD.register("balance", {
  title: "Handling balance",
  group: "core",
  w: 420,
  h: 128,
  hz: 60,
  resize: "width",
  minW: 320,
  options: [
    {
      key: "seconds", label: "Trace length", type: "range", min: 2, max: 20, step: 1, default: 6, unit: " s",
      hint: "Seconds shown at the usual width; a wider trace shows more",
    },
    { key: "rangeDeg", label: "Scale", type: "range", min: 2, max: 20, step: 1, default: 6, unit: "°" },
    {
      key: "smoothing", label: "Smoothing", type: "choice", default: "light",
      choices: [["off", "Off"], ["light", "Light"], ["smooth", "Smooth"]],
      hint: "Time-based smoothing for the displayed trace",
    },
    { key: "under", label: "Understeer", type: "color", default: "#4c9dff" },
    { key: "over", label: "Oversteer", type: "color", default: "#f6c445" },
  ],
  mount(root, _state, opts) {
    const options = opts.options;
    const rangeDeg = options.rangeDeg as number;
    const tauSeconds = TAU_SECONDS[options.smoothing as string] ?? TAU_SECONDS.light;
    const underColor = options.under as string;
    const overColor = options.over as string;
    const canvasWidth = traceCanvasWidth(opts.w);
    const plotLeft = TRACE_AXIS_LEFT;
    const plotRight = canvasWidth - TRACE_AXIS_RIGHT;
    const plotWidth = plotRight - plotLeft;
    const plotTop = 15;
    const plotBottom = TRACE_CANVAS_HEIGHT - 10;
    const plotHeight = plotBottom - plotTop;
    const centerY = plotTop + plotHeight / 2;
    const windowSeconds = traceWindowSeconds(options.seconds as number, plotWidth);

    const panel = HUD.h("div.hud-panel.balance-panel");
    panel.style.setProperty("--balance-over", overColor);
    panel.style.setProperty("--balance-under", underColor);
    const title = HUD.h("span.balance-title", { text: "HANDLING BALANCE" });
    const readout = HUD.h("span.balance-readout", {
      role: "status",
      "aria-live": "polite",
      "data-state": "no-data",
      text: "NO DATA —",
    });
    const header = HUD.h("div.balance-header", null, title, readout);
    const canvas = HUD.h("canvas.balance-canvas", {
      role: "img",
      "aria-label": "Rear minus front average tyre slip-angle magnitude. Positive is oversteer tendency; negative is understeer tendency.",
    }) as HTMLCanvasElement;
    panel.append(header, canvas);
    root.append(panel);

    const ctx = HUD.canvas(canvas, canvasWidth, TRACE_CANVAS_HEIGHT);
    const background = document.createElement("canvas");
    const bg = HUD.canvas(background, canvasWidth, TRACE_CANVAS_HEIGHT);
    const trace = new TraceBuffer(1, TRACE_CAPACITY);
    const sample = new Float32Array(1);

    let grid = "";
    let zero = "";
    let dim = "";
    let faint = "";

    function valueY(value: number): number {
      return centerY - clamp(value, -rangeDeg, rangeDeg) / rangeDeg * (plotHeight / 2);
    }

    function axisLabel(value: number): string {
      if (value === 0) return "0°";
      return (value > 0 ? "+" : "−") + Math.abs(value).toFixed(0) + "°";
    }

    function readTheme() {
      grid = HUD.css("--track-line");
      zero = HUD.css("--track-edge");
      dim = HUD.css("--text-dim");
      faint = HUD.css("--text-faint");

      bg.clearRect(0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      bg.font = "10px " + HUD.css("--font-ui");
      bg.textAlign = "right";
      bg.textBaseline = "middle";
      bg.lineWidth = 1;
      for (const level of [rangeDeg, 0, -rangeDeg]) {
        const y = valueY(level);
        const strong = level === 0;
        bg.strokeStyle = strong ? zero : grid;
        bg.setLineDash(strong ? [] : [2, 4]);
        bg.beginPath();
        bg.moveTo(plotLeft + 0.5, Math.round(y) + 0.5);
        bg.lineTo(plotRight + 0.5, Math.round(y) + 0.5);
        bg.stroke();
        bg.fillStyle = strong ? dim : faint;
        bg.fillText(axisLabel(level), plotLeft - 5, y);
      }
      bg.setLineDash([]);
    }

    function strokeSegment(x1: number, y1: number, x2: number, y2: number, color: string) {
      ctx.strokeStyle = color;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.stroke();
    }

    function draw() {
      ctx.clearRect(0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      ctx.drawImage(background, 0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      if (!trace.count) return;

      const newestTime = trace.lastTime;
      const start = traceStart(trace, windowSeconds);
      let previous = -1;
      let latestX = 0;
      let latestY = centerY;
      let latestValue = 0;

      ctx.save();
      ctx.beginPath();
      ctx.rect(plotLeft, plotTop, plotWidth, plotHeight);
      ctx.clip();
      ctx.lineWidth = 1.8;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";

      for (let i = start; i < trace.count; i++) {
        if (!trace.validAt(i)) {
          previous = -1;
          continue;
        }
        const value = trace.valueAt(i, 0);
        const x = plotRight - (newestTime - trace.timeAt(i)) / windowSeconds * plotWidth;
        const y = valueY(value);
        if (previous >= 0 && !trace.breaksAt(i)) {
          const previousValue = trace.valueAt(previous, 0);
          const previousX = plotRight - (newestTime - trace.timeAt(previous)) / windowSeconds * plotWidth;
          const previousY = valueY(previousValue);
          if (previousValue * value < 0) {
            const zeroAt = previousValue / (previousValue - value);
            const zeroX = previousX + (x - previousX) * zeroAt;
            strokeSegment(previousX, previousY, zeroX, centerY, previousValue > 0 ? overColor : underColor);
            strokeSegment(zeroX, centerY, x, y, value > 0 ? overColor : underColor);
          } else {
            const color = (previousValue + value) >= 0 ? overColor : underColor;
            strokeSegment(previousX, previousY, x, y, color);
          }
        }
        previous = i;
        latestX = x;
        latestY = y;
        latestValue = value;
      }

      if (previous >= 0) {
        ctx.fillStyle = latestValue > 0 ? overColor : latestValue < 0 ? underColor : dim;
        ctx.beginPath();
        ctx.arc(latestX, latestY, 2.2, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.restore();
    }

    function setReadout(text: string, state: string) {
      HUD.text(readout, text);
      HUD.data(readout, "state", state);
    }

    function setValue(value: number) {
      const sign = value > 0 ? "+" : value < 0 ? "−" : "";
      const degrees = sign + Math.abs(value).toFixed(1) + "°";
      if (Math.abs(value) <= NEUTRAL_DEG) setReadout("NEUTRAL " + degrees, "neutral");
      else if (value > 0) setReadout("OVER " + degrees, "over");
      else setReadout("UNDER " + degrees, "under");
    }

    let hasContext = false;
    let game = "";
    let connection = "";
    let trackName = "";
    let sessionName = "";
    let sessionKind = "";
    let previousElapsed = NaN;
    let filtered: number | null = null;

    function resetForContext(s: ViewState): boolean {
      const elapsedRewound = Number.isFinite(previousElapsed) && Number.isFinite(s.session.elapsed) &&
        s.session.elapsed < previousElapsed - 0.5;
      const changed = hasContext && (
        game !== s.game || connection !== s.connection || trackName !== s.track.name ||
        sessionName !== s.session.name || sessionKind !== s.session.kind || elapsedRewound
      );
      if (changed) {
        trace.clear();
        filtered = null;
      }
      hasContext = true;
      game = s.game;
      connection = s.connection;
      trackName = s.track.name;
      sessionName = s.session.name;
      sessionKind = s.session.kind;
      previousElapsed = s.session.elapsed;
      return changed;
    }

    readTheme();

    return {
      update(s: ViewState) {
        const changed = resetForContext(s);
        if (changed) setReadout("NO DATA —", "no-data");
        if (s.connection !== "connected") {
          const hadTrace = trace.count > 0;
          if (hadTrace) trace.clear();
          filtered = null;
          setReadout("NO DATA —", "no-data");
          if (changed || hadTrace) draw();
          return;
        }

        const timestamp = s.t;
        if (!Number.isFinite(timestamp) || timestamp === trace.lastTime) {
          if (changed) draw();
          return;
        }
        const dt = Number.isFinite(trace.lastTime) ? timestamp - trace.lastTime : 0;
        if (dt < 0 || dt > TRACE_GAP_SECONDS) filtered = null;

        const raw = s.player.slipBalanceDeg;
        const speedIsLow = Number.isFinite(s.player.speed) && s.player.speed < MIN_SPEED_KMH;
        const usable = !speedIsLow && typeof raw === "number" && Number.isFinite(raw);
        let sampleValue: number | null = null;
        if (usable && raw !== null) {
          if (tauSeconds === 0 || filtered === null || dt <= 0 || dt > TRACE_GAP_SECONDS) {
            filtered = raw;
          } else {
            const alpha = 1 - Math.exp(-dt / tauSeconds);
            filtered += alpha * (raw - filtered);
          }
          sample[0] = filtered;
          sampleValue = filtered;
        } else {
          filtered = null;
        }

        const accepted = trace.append(timestamp, sampleValue === null ? null : sample);
        if (!accepted) return;

        if (sampleValue === null) {
          setReadout(speedIsLow ? "LOW SPEED —" : "NO DATA —", speedIsLow ? "low-speed" : "no-data");
        } else {
          setValue(sampleValue);
        }
        draw();
      },
      theme() {
        readTheme();
        draw();
      },
    };
  },
});
