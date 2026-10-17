/* Tyres: four corners laid out like the car. Inner/middle/outer surface temps,
   pressure against the optimal window, core temp, wear and brake disc temp.
   Games without surface zones (ACC) get one core-temperature block per tyre;
   games without wear hide the wear bar and row. */
import "./tyres.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

type Rgb = [number, number, number];
interface Corner {
  zO: HTMLElement; zM: HTMLElement; zI: HTMLElement;
  wearFill: HTMLElement;
  press: HTMLElement; core: HTMLElement; wearTxt: HTMLElement; brake: HTMLElement; brakeDot: HTMLElement;
}

HUD.register("tyres", {
  title: "Tyres",
  group: "car",
  w: 270,
  hz: 4,
  mount(root) {
    const { h } = HUD;
    const P_LO = 26.0, P_HI = 27.0;       // optimal hot pressure window (psi)
    const B_LO = 300, B_HI = 650;         // brake disc window (°C)
    const STOPS = [60, 75, 86, 97, 108];  // temps mapped onto the ramp tokens
    const RAMP_VARS = ["--t-cold", "--t-cool", "--t-ok", "--t-warm", "--t-hot"];
    let ramp: Rgb[] = [];

    const compEl = h("span.badge.comp");
    const stintEl = h("span.num.dim");
    const head = h("div.hud-head", null, h("span.title", { text: "Tyres" }), compEl, h("span.spacer"), stintEl);

    const corners: Corner[] = [];
    const grid = h("div.ty-grid");
    ["FL", "FR", "RL", "RR"].forEach((name, k) => {
      const left = k % 2 === 0;
      // zone order left→right inside the block: left tyres have outer on the left
      const zO = h("i.zone"), zM = h("i.zone"), zI = h("i.zone");
      const block = h("div.ty-block", null, ...(left ? [zO, zM, zI] : [zI, zM, zO]));
      const wearFill = h("i");
      const wear = h("div.bar.ty-wear", null, wearFill);
      const tyre = h("div.ty-tyre", null, block, wear);

      const press = h("span.ty-press.num");
      const core = h("span.num");
      const wearTxt = h("span.num");
      const brake = h("span.num");
      const brakeDot = h("i.ty-dot");
      const info = h("div.ty-info", null,
        h("div.ty-press-row", null, press, h("small", { text: "psi" })),
        h("div.ty-kv", null, h("span.dim", { text: "Core" }), h("span", null, core, h("small", { text: "°" }))),
        h("div.ty-kv.ty-wear-row", null, h("span.dim", { text: "Wear" }), wearTxt),
        h("div.ty-kv", null, h("span.dim", { text: "Brake" }), h("span.ty-brake", null, brakeDot, brake)));

      const cell = h("div.ty-corner", null, ...(left ? [tyre, info] : [info, tyre]));
      cell.classList.add(left ? "left" : "right");
      cell.append(h("span.ty-name", { text: name }));
      grid.append(cell);
      corners.push({ zO, zM, zI, wearFill, press, core, wearTxt, brake, brakeDot });
    });

    const panel = h("div.hud-panel", null, head, h("div.hud-body", null, grid));
    root.append(panel);

    function readRamp() {
      ramp = RAMP_VARS.map((v) => {
        const hex = HUD.css(v).replace("#", "");
        const n = parseInt(hex.length === 3 ? hex.replace(/./g, "$&$&") : hex, 16);
        return [(n >> 16) & 255, (n >> 8) & 255, n & 255] as Rgb;
      });
    }
    const rgb = (c: Rgb) => "rgb(" + c[0] + "," + c[1] + "," + c[2] + ")";
    function tempColor(t: number): string {
      if (t <= STOPS[0]) return rgb(ramp[0]);
      for (let i = 1; i < STOPS.length; i++) {
        if (t <= STOPS[i]) {
          const f = (t - STOPS[i - 1]) / (STOPS[i] - STOPS[i - 1]);
          const a = ramp[i - 1], b = ramp[i];
          return "rgb(" + Math.round(a[0] + (b[0] - a[0]) * f) + "," + Math.round(a[1] + (b[1] - a[1]) * f) + "," + Math.round(a[2] + (b[2] - a[2]) * f) + ")";
        }
      }
      return rgb(ramp[ramp.length - 1]);
    }
    // background only when the color changed
    const bgs = new WeakMap<HTMLElement, string>();
    const bg = (el: HTMLElement, v: string) => { if (bgs.get(el) !== v) { bgs.set(el, v); el.style.background = v; } };
    const band = (v: number, lo: number, hi: number) => (v < lo ? "low" : v > hi ? "high" : "ok");

    readRamp();

    return {
      update(s: ViewState) {
        const p = s.player;
        if (p.tyres.length < 4) return;
        const compound = p.tyres[0].compound;
        HUD.text(compEl, compound === "DHE" ? "Dry" : compound);
        HUD.cls(compEl, "is-empty", !compound);
        HUD.text(stintEl, "Stint " + p.stintLaps + (p.stintLaps === 1 ? " lap" : " laps"));
        const hasWear = p.tyres[0].wear != null;
        HUD.cls(panel, "no-wear", !hasWear);
        for (let k = 0; k < 4; k++) {
          const ty = p.tyres[k], c = corners[k];
          // no surface zones (e.g. ACC): the whole block shows the core temperature
          bg(c.zO, tempColor(ty.o ?? ty.core));
          bg(c.zM, tempColor(ty.m ?? ty.core));
          bg(c.zI, tempColor(ty.i ?? ty.core));
          if (ty.wear != null) {
            c.wearFill.style.transform = "scaleX(" + ty.wear.toFixed(3) + ")";
            HUD.data(c.wearFill, "w", ty.wear < 0.7 ? "low" : "ok");
            HUD.text(c.wearTxt, F.pct(ty.wear));
          }
          HUD.text(c.press, ty.pressure.toFixed(1));
          HUD.data(c.press, "band", band(ty.pressure, P_LO, P_HI));
          HUD.text(c.core, F.int(ty.core));
          HUD.text(c.brake, F.int(ty.brake));
          HUD.data(c.brakeDot, "band", band(ty.brake, B_LO, B_HI));
        }
      },
      theme() {
        readRamp();
        for (const c of corners) { bgs.delete(c.zO); bgs.delete(c.zM); bgs.delete(c.zI); }
      },
    };
  },
});
