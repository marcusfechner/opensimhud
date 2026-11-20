/* Tyres: four corners laid out like the car. Inner/middle/outer surface temps,
   pressure against the optimal window, core temp, wear and brake disc temp.
   Games without surface zones (ACC) get one core-temperature block per tyre;
   games without wear hide the wear bar and row. */
import "./tyres.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";
import { tyreTemperaturePosition, tyreTemperatureWindow } from "../tyre-temperature";

type Rgb = [number, number, number];
interface Corner {
  block: HTMLElement; coreBlock: HTMLElement; surface: HTMLElement;
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
    const RAMP_VARS = ["--t-cold", "--t-cool", "--t-ok", "--t-warm", "--t-hot"];
    let ramp: Rgb[] = [];

    const compEl = h("span.badge.comp");
    const stintEl = h("span.num.dim");
    const head = h("div.hud-head", null, h("span.title", { text: "Tyres" }), compEl, h("span.spacer"), stintEl);

    const corners: Corner[] = [];
    const grid = h("div.ty-grid");
    ["FL", "FR", "RL", "RR"].forEach((name, k) => {
      const left = k % 2 === 0;
      const surface = h("div.ty-surface");
      const coreBlock = h("div.ty-core");
      const block = h("div.ty-block", null, coreBlock, surface);
      if (k >= 2) block.classList.add("rear");
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
      corners.push({ block, coreBlock, surface, wearFill, press, core, wearTxt, brake, brakeDot });
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
    function tempColor(t: number, window: ReturnType<typeof tyreTemperatureWindow>): string {
      const position = tyreTemperaturePosition(t, window);
      const i = Math.min(3, Math.floor(position));
      const f = position - i;
      const a = ramp[i], b = ramp[i + 1];
      return rgb([Math.round(a[0] + (b[0] - a[0]) * f), Math.round(a[1] + (b[1] - a[1]) * f), Math.round(a[2] + (b[2] - a[2]) * f)]);
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
        const rearCompound = p.tyres[2].compound;
        const label = (value: string) => value === "DHE" ? "Dry" : value;
        HUD.text(compEl, compound === rearCompound ? label(compound) : "F: " + (label(compound) || "-") + " / R: " + (label(rearCompound) || "-"));
        HUD.cls(compEl, "is-empty", !compound && !rearCompound);
        HUD.text(stintEl, "Stint " + p.stintLaps + (p.stintLaps === 1 ? " lap" : " laps"));
        const hasWear = p.tyres[0].wear != null;
        HUD.cls(panel, "no-wear", !hasWear);
        for (let k = 0; k < 4; k++) {
          const ty = p.tyres[k], c = corners[k];
          const window = tyreTemperatureWindow(s.game, ty.compound, p.car.model);
          const hasSurface = ty.o != null && ty.m != null && ty.i != null;
          HUD.cls(c.block, "has-surface", hasSurface);
          bg(c.coreBlock, tempColor(ty.core, window));
          if (hasSurface) {
            const outer = tempColor(ty.o!, window), middle = tempColor(ty.m!, window), inner = tempColor(ty.i!, window);
            const first = k % 2 === 0 ? outer : inner;
            const last = k % 2 === 0 ? inner : outer;
            bg(c.surface, "linear-gradient(to right, " + first + " 0%, " + middle + " 50%, " + last + " 100%)");
          }
          if (ty.wear != null) {
            c.wearFill.style.transform = "scaleX(" + ty.wear.toFixed(3) + ")";
            HUD.data(c.wearFill, "w", ty.wear < 0.7 ? "low" : "ok");
            HUD.text(c.wearTxt, F.pct(ty.wear));
          }
          HUD.text(c.press, ty.pressure.toFixed(1));
          if (s.game === "evo") c.press.removeAttribute("data-band");
          else HUD.data(c.press, "band", band(ty.pressure, P_LO, P_HI));
          HUD.text(c.core, F.int(ty.core));
          HUD.text(c.brake, F.int(ty.brake));
          HUD.data(c.brakeDot, "band", band(ty.brake, B_LO, B_HI));
        }
      },
      theme() {
        readRamp();
        for (const c of corners) { bgs.delete(c.coreBlock); bgs.delete(c.surface); }
      },
    };
  },
});
