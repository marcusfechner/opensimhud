/* Inputs: scrolling throttle/brake/steer trace, pedal bars, gear, speed and wheel angle. */
HUD.register("inputs", {
  title: "Inputs",
  group: "core",
  w: 420,
  hz: 60,
  mount(root) {
    const { h } = HUD;
    const CW = 250, CH = 88, LEN = 360; // ~6 s at 60 Hz
    const PAD = 4;

    const canvas = h("canvas.in-trace");
    const ctx = HUD.canvas(canvas, CW, CH);
    const bg = document.createElement("canvas");
    const bgCtx = HUD.canvas(bg, CW, CH);

    const mkBar = (cls) => {
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

    root.append(h("div.hud-panel", null,
      h("div.hud-body.in-body", null,
        canvas,
        h("div.in-bars", null, cluBar.el, brkBar.el, thrBar.el),
        h("div.in-box", null,
          h("div.in-gearrow", null, gearEl, wheel),
          h("div.in-speedrow", null, speedEl, h("span.unit", { text: "km/h" })),
          h("div.in-flags", null, tcEl, absEl)))));

    // ring buffers
    const thr = new Float32Array(LEN), brk = new Float32Array(LEN), steer = new Float32Array(LEN);
    const abs = new Uint8Array(LEN), tc = new Uint8Array(LEN);
    let head = 0;

    let col = {};
    function readColors() {
      col = {
        thr: HUD.css("--in-throttle"),
        brk: HUD.css("--in-brake"),
        steer: HUD.css("--in-steer"),
        abs: HUD.css("--in-abs"),
        tc: HUD.css("--in-tc"),
        grid: HUD.css("--track-line"),
      };
      // area shading under the traces: built once per theme, reused every frame
      col.thrFill = shade(col.thr);
      col.brkFill = shade(col.brk);
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
    const yOf = (v) => PAD + (CH - 2 * PAD) * (1 - v);
    function rgba(hex, a) {
      const m = /^#?([0-9a-f]{6})$/i.exec(hex);
      if (!m) return hex;
      const n = parseInt(m[1], 16);
      return "rgba(" + (n >> 16) + "," + ((n >> 8) & 255) + "," + (n & 255) + "," + a + ")";
    }
    function shade(color) {
      const g = ctx.createLinearGradient(0, yOf(1), 0, yOf(0));
      g.addColorStop(0, rgba(color, 0.42));
      g.addColorStop(1, rgba(color, 0.04));
      return g;
    }
    function area(buf, fill) {
      ctx.fillStyle = fill;
      ctx.beginPath();
      ctx.moveTo(0, yOf(0));
      for (let i = 0; i < LEN; i++) ctx.lineTo((i / (LEN - 1)) * CW, yOf(buf[(head + i) % LEN]));
      ctx.lineTo(CW, yOf(0));
      ctx.closePath();
      ctx.fill();
    }
    function line(buf, color, width, alpha) {
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.globalAlpha = alpha;
      ctx.beginPath();
      for (let i = 0; i < LEN; i++) {
        const v = buf[(head + i) % LEN];
        const x = (i / (LEN - 1)) * CW;
        if (i === 0) ctx.moveTo(x, yOf(v)); else ctx.lineTo(x, yOf(v));
      }
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    function ticks(buf, color, y) {
      ctx.fillStyle = color;
      for (let i = 0; i < LEN; i++) {
        if (buf[(head + i) % LEN]) ctx.fillRect((i / (LEN - 1)) * CW, y, 1.5, 3);
      }
    }

    readColors();

    function draw() {
      ctx.clearRect(0, 0, CW, CH);
      ctx.drawImage(bg, 0, 0, CW, CH);
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
      update(s) {
        const p = s.player;
        thr[head] = p.throttle;
        brk[head] = p.brake;
        steer[head] = 0.5 + p.steer * 0.5;
        abs[head] = p.absActive ? 1 : 0;
        tc[head] = p.tcActive ? 1 : 0;
        head = (head + 1) % LEN;
        draw();

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
