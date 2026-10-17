/* Weather: temperatures, rain forecast, track state, wind. */
import "./weather.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

type RainKey = "rainNow" | "rain10" | "rain30";

HUD.register("weather", {
  title: "Weather",
  group: "strategy",
  w: 270,
  hz: 2,
  mount(root) {
    const { h } = HUD;
    const SEG = 5;

    const grip = h("span.wx-grip");
    const head = h("div.hud-head", null, h("span.title", { text: "Weather" }), h("span.spacer"), grip);

    const air = h("span.num");
    const trk = h("span.num");
    const temps = h("div.wx-temps", null,
      h("div.stat", null, h("span.k", { text: "Air" }), h("span.v", null, air, h("small", { text: "°C" }))),
      h("div.stat", null, h("span.k", { text: "Track" }), h("span.v", null, trk, h("small", { text: "°C" }))));

    const slots = ([["rainNow", "Now"], ["rain10", "+10 min"], ["rain30", "+30 min"]] as [RainKey, string][]).map(([key, when]) => {
      const segs: HTMLElement[] = [];
      const meter = h("div.wx-meter");
      for (let i = 0; i < SEG; i++) { const s = h("i"); meter.append(s); segs.push(s); }
      const label = h("span.wx-label");
      const el = h("div.wx-slot", null, h("span.wx-when", { text: when }), meter, label);
      return { key, el, segs, label, lit: -1 };
    });
    const forecast = h("div.wx-forecast", null, ...slots.map((s) => s.el));

    const wet = h("span");
    const windSpd = h("span.num");
    const arrow = h("span.wx-arrow");
    arrow.innerHTML = '<svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M8 1.5 L12.5 12.5 L8 10 L3.5 12.5 Z" fill="currentColor"/></svg>';
    const foot = h("div.wx-foot", null,
      h("span.dim", { text: "Track" }), wet,
      h("span.spacer"),
      h("span.dim", { text: "Wind" }), arrow, windSpd, h("span.dim", { text: "km/h" }));

    const panel = h("div.hud-panel", null, head, h("div.hud-body", null, temps, forecast), foot);
    root.append(panel);

    const rainLabel = (v: number) =>
      v < 0.05 ? "Dry" : v < 0.2 ? "Drizzle" : v < 0.4 ? "Light rain" : v < 0.65 ? "Medium rain" : "Heavy rain";
    const wetLabel = (v: number) => (v < 0.05 ? "Dry" : v < 0.25 ? "Damp" : v < 0.5 ? "Wet" : "Flooded");
    let arrowDeg: number | null = null;

    return {
      update(s: ViewState) {
        const w = s.weather;
        HUD.text(air, F.fixed(w.air, 1));
        HUD.text(trk, F.fixed(w.track, 1));
        HUD.text(grip, w.grip ? w.grip + " grip" : "");
        for (let k = 0; k < slots.length; k++) {
          const sl = slots[k], v = w[sl.key] || 0;
          const lit = v < 0.05 ? 0 : Math.min(SEG, Math.ceil(v * SEG));
          if (lit !== sl.lit) {
            sl.lit = lit;
            for (let i = 0; i < SEG; i++) HUD.cls(sl.segs[i], "on", i < lit);
          }
          HUD.text(sl.label, rainLabel(v));
          HUD.cls(sl.el, "is-rain", v >= 0.05);
        }
        HUD.text(wet, wetLabel(w.wetness || 0));
        HUD.text(windSpd, F.int(w.windSpeed));
        const deg = Math.round(w.windDir);
        if (arrowDeg !== deg) { arrowDeg = deg; arrow.style.transform = "rotate(" + deg + "deg)"; }
      },
    };
  },
});
