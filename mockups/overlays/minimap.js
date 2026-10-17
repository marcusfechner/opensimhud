/* Minimap: heading-up, zoomed local map around the player (~300 m). The full track path is
   built once in world metres; each frame only the canvas transform changes. */
HUD.register("minimap", {
  title: "Minimap",
  group: "awareness",
  w: 220,
  h: 220,
  hz: 30,
  mount(root, state) {
    const { h } = HUD;
    const W = 220, H = 220;
    const CX = W / 2, CY = H * 0.64;   // player sits in the lower third, more road ahead
    const SCALE = 0.46;                // px per metre → ~300 m visible ahead
    const track = state.session.track;
    const n = track.n;

    const canvas = h("canvas");
    const panel = h("div.hud-panel.mm-panel", null, canvas);
    root.append(panel);
    const ctx = HUD.canvas(canvas, W, H);
    const dpr = Math.min(2, window.devicePixelRatio || 1);

    // world-space paths, built once
    const road = new Path2D();
    road.moveTo(track.x[0], track.y[0]);
    for (let i = 1; i < n; i++) road.lineTo(track.x[i], track.y[i]);
    road.closePath();
    function crossLine(pct, half) {
      const i = Math.floor(pct * n) % n, hd = track.heading[i];
      const nx = -Math.sin(hd), ny = Math.cos(hd);
      const p = new Path2D();
      p.moveTo(track.x[i] - nx * half, track.y[i] - ny * half);
      p.lineTo(track.x[i] + nx * half, track.y[i] + ny * half);
      return p;
    }
    const startLine = crossLine(0, 8);
    const sectorLines = track.sectors.slice(1).map((pct) => crossLine(pct, 7));

    let col = {};
    function readColors() {
      col = {
        text: HUD.css("--text"),
        faint: HUD.css("--text-faint"),
        edge: HUD.css("--track-edge"),
        line: HUD.css("--track-line"),
        accent: HUD.css("--accent"),
        accent2: HUD.css("--accent-2"),
        GT3: HUD.css("--c-gt3"),
        GT4: HUD.css("--c-gt4"),
        CUP: HUD.css("--c-cup"),
        TCX: HUD.css("--c-tcx"),
        font: HUD.css("--font-num"),
      };
    }
    readColors();

    function render(s) {
      const me = s.player.car;
      const rot = -Math.PI / 2 - me.heading;
      const cos = Math.cos(rot), sin = Math.sin(rot);

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);

      // distance rings (screen space)
      ctx.strokeStyle = col.line;
      ctx.lineWidth = 1;
      ctx.setLineDash([2, 4]);
      for (const m of [100, 200]) {
        ctx.beginPath();
        ctx.arc(CX, CY, m * SCALE, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      ctx.fillStyle = col.faint;
      ctx.font = "500 9px " + col.font;
      ctx.fillText("100 m", CX + 4, CY - 100 * SCALE - 3);

      // world → screen: translate to player, rotate heading-up, scale
      ctx.save();
      ctx.translate(CX, CY);
      ctx.rotate(rot);
      ctx.scale(SCALE, SCALE);
      ctx.translate(-me.x, -me.y);
      ctx.lineJoin = ctx.lineCap = "round";
      ctx.globalAlpha = 0.22;
      ctx.strokeStyle = col.accent;
      ctx.lineWidth = 30;
      ctx.stroke(road);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = col.edge;
      ctx.lineWidth = 14;
      ctx.stroke(road);
      ctx.strokeStyle = "rgba(10, 13, 19, 0.9)";
      ctx.lineWidth = 10;
      ctx.stroke(road);
      ctx.strokeStyle = col.accent;
      ctx.lineWidth = 3;
      for (const p of sectorLines) ctx.stroke(p);
      ctx.strokeStyle = col.text;
      ctx.lineWidth = 3;
      ctx.stroke(startLine);
      ctx.restore();

      // cars in screen space (constant size regardless of zoom)
      const cars = s.cars;
      for (let i = 0; i < cars.length; i++) {
        const c = cars[i];
        if (c.isPlayer) continue;
        const dx = c.x - me.x, dy = c.y - me.y;
        const x = CX + (dx * cos - dy * sin) * SCALE;
        const y = CY + (dx * sin + dy * cos) * SCALE;
        if (x < -6 || x > W + 6 || y < -6 || y > H + 6) continue;
        ctx.globalAlpha = c.inPit ? 0.35 : 1;
        ctx.fillStyle = col[c.cls] || col.text;
        ctx.beginPath();
        ctx.arc(x, y, 4.5, 0, Math.PI * 2);
        ctx.fill();
        ctx.lineWidth = 1.5;
        ctx.strokeStyle = "rgba(10, 13, 19, 0.85)";
        ctx.stroke();
      }
      ctx.globalAlpha = 1;

      // player arrow, always pointing up
      ctx.beginPath();
      ctx.moveTo(CX, CY - 9);
      ctx.lineTo(CX + 6.5, CY + 7);
      ctx.lineTo(CX, CY + 3.5);
      ctx.lineTo(CX - 6.5, CY + 7);
      ctx.closePath();
      ctx.fillStyle = col.text;
      ctx.shadowColor = col.accent;
      ctx.shadowBlur = 10;
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = col.accent;
      ctx.stroke();
    }

    let last = null;
    return {
      update(s) { last = s; render(s); },
      theme() { readColors(); if (last) render(last); },
    };
  },
});
