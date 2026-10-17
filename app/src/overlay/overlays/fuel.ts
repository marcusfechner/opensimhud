/* Fuel: tank level, consumption and how much to add at the next stop. */
import "./fuel.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

HUD.register("fuel", {
  title: "Fuel",
  group: "core",
  w: 270,
  hz: 4,
  mount(root) {
    const { h } = HUD;

    const lapsLeftEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Fuel" }), h("span.spacer"), lapsLeftEl);

    const litersEl = h("span.num.fu-liters");
    const capEl = h("span.dim.num");
    const tankFill = h("i");
    const tank = h("div.bar.fu-tank", null, tankFill);

    const stat = (k: string) => {
      const v = h("span.v.num");
      return { el: h("div.stat", null, h("span.k", { text: k }), v), v };
    };
    const perLap = stat("Per lap");
    const lastLap = stat("Last lap");
    const lapsLeft = stat("Laps left");
    const raceLaps = stat("Race laps");
    const toFinish = stat("To finish");
    const empty = stat("Empty in");

    const addEl = h("span.num.fu-add-v");
    const addMsg = h("span.fu-add-msg");
    const add = h("div.fu-add", null, h("div.fu-add-l", null, h("span.k", { text: "Add at next stop" }), addMsg), addEl);

    root.append(h("div.hud-panel", null, head,
      h("div.hud-body.fu-body", null,
        h("div.fu-tankrow", null, h("div.fu-lit", null, litersEl, h("span.unit", { text: "L" })), capEl),
        tank,
        h("div.fu-grid", null, perLap.el, lastLap.el, lapsLeft.el, raceLaps.el, toFinish.el, empty.el),
        add)));

    return {
      update(s: ViewState) {
        const f = s.player.fuel;
        const lap = s.player.lap;
        // no consumption figures until a lap has been completed
        const known = f.perLap > 0;
        HUD.text(litersEl, F.fixed(f.liters, 1));
        HUD.text(capEl, f.capacity > 0 ? "of " + Math.round(f.capacity) + " L" : "");
        tankFill.style.transform = "scaleX(" + (f.capacity > 0 ? Math.min(1, f.liters / f.capacity) : 0).toFixed(4) + ")";
        HUD.text(lapsLeftEl, known ? F.fixed(f.lapsLeft, 1) + " laps" : "");

        HUD.text(perLap.v, known ? F.fixed(f.perLap, 2) : "—");
        HUD.text(lastLap.v, f.lastLapUse > 0 ? F.fixed(f.lastLapUse, 2) : "—");
        HUD.cls(lastLap.v, "gain", known && f.lastLapUse > 0 && f.lastLapUse < f.perLap - 0.01);
        HUD.cls(lastLap.v, "loss", known && f.lastLapUse > f.perLap + 0.01);
        HUD.text(lapsLeft.v, known ? F.fixed(f.lapsLeft, 1) : "—");
        HUD.text(raceLaps.v, s.session.lapRemaining > 0 ? F.fixed(s.session.lapRemaining, 1) : "—");
        HUD.text(toFinish.v, known && s.session.lapRemaining > 0 ? F.fixed(f.toFinish, 1) : "—");
        HUD.text(empty.v, known ? F.clock(f.lapsLeft * (lap.best || lap.last || 1)) : "—");

        const short = known && s.session.lapRemaining > 0 && f.liters < f.toFinish;
        HUD.text(addEl, short ? "+" + F.fixed(f.toAdd, 1) + " L" : "—");
        HUD.text(addMsg, !known ? "Needs one full lap" : short ? "Not enough to finish" : "Enough to finish");
        HUD.cls(add, "short", short);
        HUD.cls(tank, "low", known && f.lapsLeft < 3);
      },
    };
  },
});
