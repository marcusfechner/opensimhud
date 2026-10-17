/* Track map: circuit outline (cached), cars as class-colored dots, player emphasized. */
HUD.register("trackmap", {
  title: "Track map",
  group: "awareness",
  w: 300,
  h: 210,
  hz: 20,
  mount(root, state) {
    const { h } = HUD;
    const W = 300, H = 210, PAD = 20;
    const track = state.session.track;
    const n = track.n;

    const canvas = h("canvas");
    const name = h("span.tm-name", { text: track.name });
    const len = h("span.tm-len.num", { text: (track.length / 1000).toFixed(2) + " km" });
    const label = h("div.tm-label", null, name, len);
    const panel = h("div.hud-panel.tm-panel", null, canvas, label);
    root.append(panel);

    const ctx = HUD.canvas(canvas, W, H);
    const off = document.createElement("canvas");

    /* fit track into the box (leave room for the label at the top) */
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (let i = 0; i < n; i++) {
      const x = track.x[i], y = track.y[i];
      if (x < minX) minX = x; if (x > maxX) maxX = x;
      if (y < minY) minY = y; if (y > maxY) maxY = y;
    }
    const top = PAD + 12;
    const sc = Math.min((W - PAD * 2) / (maxX - minX), (H - top - PAD) / (maxY - minY));
    const ox = (W - (maxX - minX) * sc) / 2 - minX * sc;
    const oy = top + (H - top - PAD - (maxY - minY) * sc) / 2 - minY * sc;
    const sx = new Float32Array(n), sy = new Float32Array(n);
    for (let i = 0; i < n; i++) { sx[i] = track.x[i] * sc + ox; sy[i] = track.y[i] * sc + oy; }

    const pathRange = (a, b) => {
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
    const sectorPaths = [pathRange(sec[0], sec[1]), pathRange(sec[1], sec[2]), pathRange(sec[2], 1)];

    // pit lane: offset to the inside of the main straight (positive normal = right of travel = inside here)
    const pit = new Path2D();
    {
      const from = 0.955, to = 1.045, steps = 60, offPx = 7;
      for (let k = 0; k <= steps; k++) {
        const pct = (from + ((to - from) * k) / steps) % 1;
        const i = Math.floor(pct * n);
        const hd = track.heading[i];
        const x = sx[i] - Math.sin(hd) * offPx, y = sy[i] + Math.cos(hd) * offPx;
        k ? pit.lineTo(x, y) : pit.moveTo(x, y);
      }
    }

    function normalAt(pct) {
      const i = Math.floor(pct * n) % n;
      const hd = track.heading[i];
      return { x: sx[i], y: sy[i], nx: -Math.sin(hd), ny: Math.cos(hd) };
    }

    let col = {};
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
        gt3: HUD.css("--c-gt3"),
        gt4: HUD.css("--c-gt4"),
        cup: HUD.css("--c-cup"),
        tcx: HUD.css("--c-tcx"),
        font: HUD.css("--font-num"),
      };
    }

    function drawStatic() {
      const c = HUD.canvas(off, W, H);
      c.clearRect(0, 0, W, H);
      c.lineJoin = c.lineCap = "round";

      // pit lane
      c.strokeStyle = col.line;
      c.lineWidth = 2;
      c.setLineDash([3, 3]);
      c.stroke(pit);
      c.setLineDash([]);

      // soft accent glow under a crisp edge line
      const g = c.createLinearGradient(0, 0, W, H);
      g.addColorStop(0, col.accent);
      g.addColorStop(1, col.accent2);
      c.globalAlpha = 0.25; c.strokeStyle = g; c.lineWidth = 10; c.stroke(full);
      c.globalAlpha = 1; c.strokeStyle = col.edge; c.lineWidth = 3; c.stroke(full);

      // sector boundary ticks
      c.strokeStyle = col.accent;
      c.lineWidth = 2;
      for (let k = 1; k < sec.length; k++) {
        const p = normalAt(sec[k]);
        c.beginPath();
        c.moveTo(p.x - p.nx * 7, p.y - p.ny * 7);
        c.lineTo(p.x + p.nx * 7, p.y + p.ny * 7);
        c.stroke();
      }
      // start / finish: chequered bar
      const sf = normalAt(0);
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
      for (let k = 0; k < 3; k++) {
        const mid = (sec[k] + (k < 2 ? sec[k + 1] : 1)) / 2;
        const p = normalAt(mid);
        c.fillText("S" + (k + 1), p.x - p.nx * 16, p.y - p.ny * 16);
      }
    }

    const clsColor = (cls) => (cls === "GT3" ? col.gt3 : cls === "GT4" ? col.gt4 : cls === "CUP" ? col.cup : col.tcx);

    // interpolate along the screen-space polyline (lateral offset is too small to matter at this scale)
    let PX = 0, PY = 0;
    function posOf(c) {
      const f = c.pct * n, i = Math.floor(f) % n, j = (i + 1) % n, t = f - Math.floor(f);
      PX = sx[i] + (sx[j] - sx[i]) * t;
      PY = sy[i] + (sy[j] - sy[i]) * t;
    }

    let lastState = null;
    function render(s) {
      ctx.clearRect(0, 0, W, H);
      ctx.drawImage(off, 0, 0, W, H);

      const ses = s.session;
      if (ses.flag === "yellow" && ses.yellowSector) {
        ctx.lineJoin = ctx.lineCap = "round";
        ctx.strokeStyle = col.yellow;
        ctx.globalAlpha = 0.85;
        ctx.lineWidth = 5;
        ctx.stroke(sectorPaths[ses.yellowSector - 1]);
        ctx.globalAlpha = 1;
      }

      const cars = s.cars;
      const r = 4;
      for (let k = 0; k < cars.length; k++) {
        const c = cars[k];
        if (c.isPlayer) continue;
        posOf(c);
        const x = PX, y = PY;
        ctx.globalAlpha = c.inPit ? 0.35 : 1;
        ctx.beginPath();
        ctx.arc(x, y, r, 0, Math.PI * 2);
        ctx.fillStyle = clsColor(c.cls);
        ctx.fill();
        ctx.lineWidth = 1.2;
        ctx.strokeStyle = "rgba(0,0,0,0.65)";
        ctx.stroke();
      }
      ctx.globalAlpha = 1;

      // player last so it sits on top
      const me = s.player.car;
      posOf(me);
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
      ctx.fillText("" + me.number, px, py + 0.5);
    }

    readColors();
    drawStatic();

    return {
      update(s) { lastState = s; render(s); },
      theme() {
        readColors();
        drawStatic();
        if (lastState) render(lastState);
      },
    };
  },
});
