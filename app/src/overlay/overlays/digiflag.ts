/* Digiflag: a trackside LED flag panel. Dot matrix on canvas; redraws only when the pattern changes. */
import "./digiflag.css";
import { HUD } from "../hud";
import type { ViewState } from "../view";

type Led = "green" | "yellow" | "blue" | "white";
/** A session flag, or "idle" when the panel is dark. */
type Shown = ViewState["session"]["flag"] | "idle";

HUD.register("digiflag", {
  title: "Digiflag",
  group: "awareness",
  w: 240,
  hz: 10,
  mount(root) {
    const { h } = HUD;
    const COLS = 27, ROWS = 13, P = 8;          // LEDs, pitch in px
    const CW = COLS * P, CH = ROWS * P;
    const GREEN_HOLD = 4000;                    // ms of green after a flag clears

    const canvas = h("canvas.df-matrix") as HTMLCanvasElement;
    const ctx = HUD.canvas(canvas, CW, CH);
    const caption = h("span.df-caption");
    const panel = h("div.hud-panel.df-panel", null,
      h("div.df-body", null, canvas),
      h("div.df-foot", null, h("span.df-dot"), caption));
    root.append(panel);

    // 5×7 glyphs for the yellow sector number
    const GLYPH: Record<string, string[]> = {
      S: ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
      1: ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
      2: ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
      3: ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
    };

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const unlit = document.createElement("canvas");
    const sprites = {} as Record<Led, HTMLCanvasElement>;

    function sprite(color: string): HTMLCanvasElement {
      // one lit LED with a soft glow, pre-rendered once per color
      const s = P * 2, c = document.createElement("canvas");
      c.width = c.height = Math.round(s * dpr);
      const g = c.getContext("2d")!;
      g.scale(dpr, dpr);
      const grd = g.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
      grd.addColorStop(0, color);
      grd.addColorStop(0.3, color);
      grd.addColorStop(0.32, "rgba(0,0,0,0)");
      g.fillStyle = grd;
      g.globalAlpha = 0.35;
      g.beginPath(); g.arc(s / 2, s / 2, s / 2, 0, Math.PI * 2); g.fill();
      g.globalAlpha = 1;
      g.fillStyle = color;
      g.beginPath(); g.arc(s / 2, s / 2, P * 0.36, 0, Math.PI * 2); g.fill();
      // hot center
      g.fillStyle = "rgba(255,255,255,0.35)";
      g.beginPath(); g.arc(s / 2 - 0.8, s / 2 - 0.8, P * 0.13, 0, Math.PI * 2); g.fill();
      return c;
    }

    function readColors() {
      const col: Record<Led, string> = {
        green: HUD.css("--flag-green"),
        yellow: HUD.css("--flag-yellow"),
        blue: HUD.css("--flag-blue"),
        white: HUD.css("--flag-white"),
      };
      for (const k in col) sprites[k as Led] = sprite(col[k as Led]);
      // static layer: every LED unlit
      unlit.width = Math.round(CW * dpr);
      unlit.height = Math.round(CH * dpr);
      const g = unlit.getContext("2d")!;
      g.scale(dpr, dpr);
      g.fillStyle = "rgba(255,255,255,0.07)";
      for (let r = 0; r < ROWS; r++) {
        for (let c = 0; c < COLS; c++) {
          g.beginPath();
          g.arc(c * P + P / 2, r * P + P / 2, P * 0.32, 0, Math.PI * 2);
          g.fill();
        }
      }
      lastKey = "";
    }

    /* pattern: per LED, a sprite name or null (unlit) */
    const grid: (Led | null)[] = new Array(COLS * ROWS);
    function pattern(flag: Shown, sector: number) {
      for (let r = 0; r < ROWS; r++) {
        for (let c = 0; c < COLS; c++) {
          let v: Led | null = null;
          if (flag === "green") v = "green";
          else if (flag === "yellow") v = "yellow";
          else if (flag === "blue") v = "blue";
          else if (flag === "white") v = "white";
          else if (flag === "blackwhite") v = c / COLS + r / ROWS > 1 ? "white" : null; // lower right white
          else if (flag === "chequered") v = (Math.floor(c / 3) + Math.floor(r / 3)) % 2 ? "white" : null;
          // LEDs can't show black: a white frame around a dark panel
          else if (flag === "black") v = r === 0 || r === ROWS - 1 || c === 0 || c === COLS - 1 ? "white" : null;
          grid[r * COLS + c] = v;
        }
      }
      if (flag === "yellow" && sector) {
        // sector number punched out in dark LEDs, centered
        const text = "S" + sector;
        const w = text.length * 6 - 1, x0 = Math.floor((COLS - w) / 2), y0 = Math.floor((ROWS - 7) / 2);
        for (let k = 0; k < text.length; k++) {
          const g = GLYPH[text[k]];
          if (!g) continue;
          for (let gy = 0; gy < 7; gy++)
            for (let gx = 0; gx < 5; gx++)
              if (g[gy][gx] === "1") grid[(y0 + gy) * COLS + x0 + k * 6 + gx] = null;
        }
      }
    }

    function draw(flag: Shown, sector: number, on: boolean) {
      ctx.clearRect(0, 0, CW, CH);
      ctx.drawImage(unlit, 0, 0, CW, CH);
      if (!on || flag === "idle") return;
      pattern(flag, sector);
      for (let i = 0; i < grid.length; i++) {
        const v = grid[i];
        if (!v) continue;
        const c = i % COLS, r = (i - c) / COLS;
        ctx.drawImage(sprites[v], c * P - P / 2, r * P - P / 2, P * 2, P * 2);
      }
    }

    const LABEL: Partial<Record<Shown, string>> = {
      idle: "No flag",
      green: "Green, track clear",
      blue: "Blue, let the faster car by",
      white: "White, slow car ahead",
      blackwhite: "Track limits warning",
      black: "Black flag, return to the pits",
      chequered: "Chequered flag",
    };
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let lastFlag: Shown = "green", greenUntil = 0, lastKey = "";
    readColors();

    return {
      update(s: ViewState) {
        const now = performance.now();
        const ses = s.session;
        let flag: Shown = ses.flag === "none" ? "idle" : ses.flag;
        if (flag === "green") {
          // green only shows briefly after another flag clears
          if (lastFlag !== "green") greenUntil = now + GREEN_HOLD;
          if (now > greenUntil) flag = "idle";
        }
        lastFlag = ses.flag === "none" ? "green" : ses.flag;
        const flashing = !reduced && (flag === "yellow" || flag === "blue");
        const on = !flashing || Math.floor(now / 250) % 2 === 0;
        const sector = flag === "yellow" ? ses.yellowSector : 0;
        const key = flag + sector + on;
        if (key !== lastKey) {
          lastKey = key;
          draw(flag, sector, on);
        }
        HUD.data(panel, "flag", flag);
        HUD.cls(panel, "is-idle", flag === "idle");
        HUD.text(caption, flag === "yellow" ? "Yellow in sector " + sector : LABEL[flag] || "");
      },
      theme() {
        readColors();
      },
    };
  },
});
