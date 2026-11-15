import "./slipangles.css";
import { HUD } from "../hud";
import type { ViewState } from "../view";
import {
  clamp,
  TRACE_AXIS_LEFT,
  TRACE_AXIS_RIGHT,
  TRACE_CANVAS_HEIGHT,
  TRACE_CAPACITY,
  traceCanvasWidth,
  traceStart,
  traceWindowSeconds,
  TraceBuffer,
} from "./sliptrace";

const WHEELS = ["FL", "FR", "RL", "RR"] as const;
const COLORS = ["#3ddc84", "#22d3ee", "#f6c445", "#b46bff"] as const;
const MIN_SPEED_KMH = 5;

HUD.register("slipangles", {
  title: "Tyre slip angles",
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
    { key: "rangeDeg", label: "Scale", type: "range", min: 2, max: 30, step: 1, default: 12, unit: "°" },
    {
      key: "mode", label: "Display", type: "choice", default: "magnitude",
      choices: [["magnitude", "Magnitude"], ["signed", "Signed"]],
      hint: "Magnitude compares turn directions; signed keeps the game's sign",
    },
    { key: "fl", label: "Front left", type: "color", default: COLORS[0] },
    { key: "fr", label: "Front right", type: "color", default: COLORS[1] },
    { key: "rl", label: "Rear left", type: "color", default: COLORS[2] },
    { key: "rr", label: "Rear right", type: "color", default: COLORS[3] },
  ],
  mount(root, _state, opts) {
    const options = opts.options;
    const rangeDeg = options.rangeDeg as number;
    const mode = options.mode as "magnitude" | "signed";
    const colors = [options.fl as string, options.fr as string, options.rl as string, options.rr as string];
    const canvasWidth = traceCanvasWidth(opts.w);
    const plotLeft = TRACE_AXIS_LEFT;
    const plotRight = canvasWidth - TRACE_AXIS_RIGHT;
    const plotWidth = plotRight - plotLeft;
    const plotTop = 15;
    const plotBottom = TRACE_CANVAS_HEIGHT - 10;
    const plotHeight = plotBottom - plotTop;
    const centerY = plotTop + plotHeight / 2;
    const windowSeconds = traceWindowSeconds(options.seconds as number, plotWidth);

    const panel = HUD.h("div.hud-panel.sliptrace-panel");
    const values = HUD.h("div.sliptrace-values", { role: "list", "aria-label": "Tyre slip angles" });
    const readouts: HTMLSpanElement[] = [];
    for (let i = 0; i < WHEELS.length; i++) {
      const label = HUD.h("span.sliptrace-value", {
        role: "listitem",
        "aria-label": ["Front left", "Front right", "Rear left", "Rear right"][i],
        style: { color: colors[i] },
        text: WHEELS[i] + " —",
      }) as HTMLSpanElement;
      readouts.push(label);
      values.append(label);
    }
    const modeLabel = HUD.h("span.sliptrace-mode", { text: mode === "magnitude" ? "MAGNITUDE" : "SIGNED" });
    const header = HUD.h("div.sliptrace-header", null, values, modeLabel);
    const canvas = HUD.h("canvas.sliptrace-canvas", {
      role: "img",
      "aria-label": "Four tyre slip-angle history, newest sample at the right",
    }) as HTMLCanvasElement;
    panel.append(header, canvas);
    root.append(panel);

    const ctx = HUD.canvas(canvas, canvasWidth, TRACE_CANVAS_HEIGHT);
    const background = document.createElement("canvas");
    const bg = HUD.canvas(background, canvasWidth, TRACE_CANVAS_HEIGHT);
    const trace = new TraceBuffer(4, TRACE_CAPACITY);

    let grid = "";
    let axis = "";
    let dim = "";

    function valueY(value: number): number {
      if (mode === "magnitude") {
        return plotBottom - clamp(Math.abs(value), 0, rangeDeg) / rangeDeg * plotHeight;
      }
      return centerY - clamp(value, -rangeDeg, rangeDeg) / rangeDeg * (plotHeight / 2);
    }

    function axisLabel(value: number): string {
      if (value === 0) return "0°";
      const sign = value > 0 ? "+" : "−";
      return sign + Math.abs(value).toFixed(0) + "°";
    }

    function readTheme() {
      grid = HUD.css("--track-line");
      axis = HUD.css("--track-edge");
      dim = HUD.css("--text-dim");

      bg.clearRect(0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      bg.font = "10px " + HUD.css("--font-ui");
      bg.textAlign = "right";
      bg.textBaseline = "middle";
      bg.lineWidth = 1;

      const levels = mode === "magnitude"
        ? [rangeDeg, rangeDeg / 2, 0]
        : [rangeDeg, 0, -rangeDeg];
      for (let i = 0; i < levels.length; i++) {
        const level = levels[i];
        const y = mode === "magnitude"
          ? plotBottom - level / rangeDeg * plotHeight
          : centerY - level / rangeDeg * (plotHeight / 2);
        const strong = mode === "magnitude" ? i === 2 : i === 1;
        bg.strokeStyle = strong ? axis : grid;
        bg.setLineDash(strong ? [] : [2, 4]);
        bg.beginPath();
        bg.moveTo(plotLeft + 0.5, Math.round(y) + 0.5);
        bg.lineTo(plotRight + 0.5, Math.round(y) + 0.5);
        bg.stroke();
        bg.fillStyle = strong ? dim : HUD.css("--text-faint");
        bg.fillText(axisLabel(level), plotLeft - 5, y);
      }
      bg.setLineDash([]);
    }

    function draw() {
      ctx.clearRect(0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      ctx.drawImage(background, 0, 0, canvasWidth, TRACE_CANVAS_HEIGHT);
      if (trace.count < 2) return;

      const newestTime = trace.lastTime;
      const start = traceStart(trace, windowSeconds);
      ctx.save();
      ctx.beginPath();
      ctx.rect(plotLeft, plotTop, plotWidth, plotHeight);
      ctx.clip();
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      for (let channel = 0; channel < 4; channel++) {
        ctx.strokeStyle = colors[channel];
        ctx.lineWidth = 1.7;
        ctx.beginPath();
        let drawing = false;
        for (let i = start; i < trace.count; i++) {
          if (!trace.validAt(i)) {
            drawing = false;
            continue;
          }
          if (trace.breaksAt(i)) drawing = false;
          const age = newestTime - trace.timeAt(i);
          const x = plotRight - age / windowSeconds * plotWidth;
          const y = valueY(trace.valueAt(i, channel));
          if (drawing) ctx.lineTo(x, y);
          else ctx.moveTo(x, y);
          drawing = true;
        }
        ctx.stroke();
      }
      ctx.restore();
    }

    let hasContext = false;
    let game = "";
    let connection = "";
    let trackName = "";
    let sessionName = "";
    let sessionKind = "";
    let previousElapsed = NaN;

    function resetForContext(s: ViewState): boolean {
      const elapsedRewound = Number.isFinite(previousElapsed) && Number.isFinite(s.session.elapsed) &&
        s.session.elapsed < previousElapsed - 0.5;
      const changed = hasContext && (
        game !== s.game || connection !== s.connection || trackName !== s.track.name ||
        sessionName !== s.session.name || sessionKind !== s.session.kind || elapsedRewound
      );
      if (changed) trace.clear();
      hasContext = true;
      game = s.game;
      connection = s.connection;
      trackName = s.track.name;
      sessionName = s.session.name;
      sessionKind = s.session.kind;
      previousElapsed = s.session.elapsed;
      return changed;
    }

    function setUnavailable() {
      for (let i = 0; i < readouts.length; i++) HUD.text(readouts[i], WHEELS[i] + " —");
    }

    function formatAngle(angle: number): string {
      const shown = mode === "magnitude" ? Math.abs(angle) : angle;
      if (mode === "signed" && shown > 0) return "+" + shown.toFixed(1) + "°";
      if (mode === "signed" && shown < 0) return "−" + Math.abs(shown).toFixed(1) + "°";
      return Math.abs(shown).toFixed(1) + "°";
    }

    readTheme();

    return {
      update(s: ViewState) {
        const changed = resetForContext(s);
        if (changed) setUnavailable();
        if (s.connection !== "connected") {
          const hadTrace = trace.count > 0;
          if (hadTrace) trace.clear();
          setUnavailable();
          if (changed || hadTrace) draw();
          return;
        }

        const angles = s.player.slipAnglesDeg;
        const usable = Number.isFinite(s.player.speed) && s.player.speed >= MIN_SPEED_KMH && angles !== null;
        const accepted = trace.append(s.t, usable ? angles : null);
        if (!accepted) {
          if (changed) draw();
          return;
        }

        if (usable && angles) {
          for (let i = 0; i < readouts.length; i++) {
            HUD.text(readouts[i], WHEELS[i] + " " + formatAngle(angles[i]));
          }
        } else {
          setUnavailable();
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
