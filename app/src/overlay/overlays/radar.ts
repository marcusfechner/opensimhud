/* Radar: top-down proximity view around the player, player pointing up. */
import "./radar.css";
import { HUD } from "../hud";
import type { ViewState } from "../view";

HUD.register("radar", {
  title: "Radar",
  group: "awareness",
  w: 220,
  h: 220,
  hz: 60,
  mount(root) {
    const { h } = HUD;
    const W = 220, H = 220, CX = W / 2, CY = H / 2;
    const RANGE = 40;            // metres along track that count as "in range"
    const PX = 3.1;              // px per metre along track
    const EX = 1.35;             // lateral + car-size exaggeration for legibility
    const CAR_W = 2.0 * PX * EX, CAR_L = 4.6 * PX * EX;

    const canvas = h("canvas") as HTMLCanvasElement;
    const sideL = h("div.radar-side.l");
    const sideR = h("div.radar-side.r");
    const panel = h("div.hud-panel.radar-panel", null, canvas, sideL, sideR);
    root.append(panel);

    const ctx = HUD.canvas(canvas, W, H);
    const bg = document.createElement("canvas");
    let col: Record<string, string> = {};

    function readColors() {
      col = {
        text: HUD.css("--text"),
        dim: HUD.css("--text-dim"),
        faint: HUD.css("--text-faint"),
        accent: HUD.css("--accent"),
        warn: HUD.css("--warn"),
        danger: HUD.css("--danger"),
        ring: HUD.css("--track-line"),
        font: HUD.css("--font-num"),
      };
    }

    function rr(c: CanvasRenderingContext2D, x: number, y: number, w: number, hh: number, r: number) {
      c.beginPath();
      c.moveTo(x + r, y);
      c.arcTo(x + w, y, x + w, y + hh, r);
      c.arcTo(x + w, y + hh, x, y + hh, r);
      c.arcTo(x, y + hh, x, y, r);
      c.arcTo(x, y, x + w, y, r);
      c.closePath();
    }

    /* static layer: range rings, lane guides, distance labels */
    function drawBg() {
      const bgCtx = HUD.canvas(bg, W, H);
      bgCtx.clearRect(0, 0, W, H);
      bgCtx.strokeStyle = col.ring;
      bgCtx.lineWidth = 1;
      for (const m of [10, 20, 35]) {
        bgCtx.globalAlpha = 0.55;
        bgCtx.setLineDash(m === 35 ? [3, 4] : []);
        bgCtx.beginPath();
        bgCtx.arc(CX, CY, m * PX, 0, Math.PI * 2);
        bgCtx.stroke();
      }
      bgCtx.setLineDash([]);
      // lane guides beside the player
      bgCtx.globalAlpha = 0.5;
      bgCtx.setLineDash([2, 5]);
      bgCtx.beginPath();
      bgCtx.moveTo(CX - 2.4 * PX * EX, 10); bgCtx.lineTo(CX - 2.4 * PX * EX, H - 10);
      bgCtx.moveTo(CX + 2.4 * PX * EX, 10); bgCtx.lineTo(CX + 2.4 * PX * EX, H - 10);
      bgCtx.stroke();
      bgCtx.setLineDash([]);
      bgCtx.globalAlpha = 1;
      bgCtx.fillStyle = col.faint;
      bgCtx.font = "500 9px " + col.font;
      bgCtx.textAlign = "left";
      bgCtx.fillText("10 m", CX + 4, CY - 10 * PX - 3);
      bgCtx.fillText("20 m", CX + 4, CY - 20 * PX - 3);
    }

    function carRect(x: number, y: number, fill: string, alpha: number) {
      ctx.globalAlpha = alpha;
      ctx.fillStyle = fill;
      rr(ctx, x - CAR_W / 2, y - CAR_L / 2, CAR_W, CAR_L, 3);
      ctx.fill();
      // windscreen hint so direction reads at a glance
      ctx.globalAlpha = alpha * 0.45;
      ctx.fillStyle = "#000";
      rr(ctx, x - CAR_W / 2 + 2, y - CAR_L / 2 + CAR_L * 0.22, CAR_W - 4, CAR_L * 0.2, 1.5);
      ctx.fill();
    }

    let visible: boolean | null = null, leftOn: boolean | null = null, rightOn: boolean | null = null;

    function render(s: ViewState) {
      const me = s.player.car;
      ctx.clearRect(0, 0, W, H);
      ctx.drawImage(bg, 0, 0, W, H);

      let any = false, left = 0, right = 0;
      const cars = s.cars;
      for (let i = 0; i < cars.length; i++) {
        const c = cars[i];
        if (c.isPlayer || c.inPit || !c.located) continue;
        const along = c.relMeters;
        if (along > RANGE || along < -RANGE) continue;
        const lat = c.lateral - me.lateral;
        if (lat > 9 || lat < -9) continue;
        any = true;
        const overlap = Math.abs(along) < 5;
        const close = Math.abs(along) < 12;
        if (overlap) {
          if (lat < 0) left = Math.max(left, 1); else right = Math.max(right, 1);
        }
        const fill = overlap ? col.danger : close ? col.warn : col.dim;
        // fade cars near the edge of the range
        const edge = Math.abs(along) / RANGE;
        const alpha = edge > 0.75 ? 1 - ((edge - 0.75) / 0.25) * 0.8 : 1;
        carRect(CX + lat * PX * EX, CY - along * PX, fill, alpha);
      }
      ctx.globalAlpha = 1;
      // player in neutral white so it never reads as a warning color
      carRect(CX, CY, col.text, 1);

      if (any !== visible) { visible = any; HUD.cls(panel, "is-idle", !any); }
      if ((left > 0) !== leftOn) { leftOn = left > 0; HUD.cls(sideL, "on", leftOn); }
      if ((right > 0) !== rightOn) { rightOn = right > 0; HUD.cls(sideR, "on", rightOn); }
    }

    readColors();
    drawBg();
    let lastState: ViewState | null = null;

    return {
      update(s: ViewState) { lastState = s; render(s); },
      theme() {
        readColors();
        drawBg();
        if (lastState) render(lastState);
      },
    };
  },
});
