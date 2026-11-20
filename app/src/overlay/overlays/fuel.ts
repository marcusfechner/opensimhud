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
      const label = h("span.k", { text: k });
      return { el: h("div.stat", null, label, v), v, label };
    };
    const perLap = stat("Per lap");
    const lastLap = stat("Last lap");
    const lapsLeft = stat("Laps left");
    const raceLaps = stat("Race laps");
    const toFinish = stat("To finish");
    const empty = stat("Empty in");

    const addEl = h("span.num.fu-add-v");
    const addMsg = h("span.fu-add-msg");
    const add = h("div.fu-add", null, h("div.fu-add-l", null, h("span.k", { text: "Add (½ lap reserve)" }), addMsg), addEl);

    root.append(h("div.hud-panel", null, head,
      h("div.hud-body.fu-body", null,
        h("div.fu-tankrow", null, h("div.fu-lit", null, litersEl, h("span.unit", { text: "L" })), capEl),
        tank,
        h("div.fu-grid", null, perLap.el, lastLap.el, lapsLeft.el, raceLaps.el, toFinish.el, empty.el),
        add)));

    return {
      update(s: ViewState) {
        const f = s.player.fuel;
        const open = !s.track.closed;
        const race = s.session.kind === "race";
        const number = (value: number | null, digits: number) => value == null ? "—" : F.fixed(value, digits);
        HUD.text(perLap.label, f.perLapEstimated ? "Per lap (game est.)" : open ? "Per timed run" : "Per lap");
        HUD.text(lapsLeft.label, open ? "B2G runs left" : "Laps left");
        HUD.text(toFinish.label, race ? "To finish" : "To end");
        HUD.text(litersEl, number(f.liters, 1));
        HUD.text(capEl, f.capacity == null ? "" : "of " + Math.round(f.capacity) + " L");
        tankFill.style.transform = "scaleX(" + (f.capacity != null && f.liters != null ? Math.min(1, f.liters / f.capacity) : 0).toFixed(4) + ")";
        const estimate = f.perLapEstimated ? "≈ " : "";
        HUD.text(lapsLeftEl, f.lapsLeft == null ? "" : estimate + F.fixed(f.lapsLeft, 1) + (open ? " B2G runs" : " laps"));
        HUD.text(perLap.v, f.perLap == null ? number(null, 2) : estimate + number(f.perLap, 2));
        HUD.text(lastLap.v, number(f.lastLapUse, 2));
        HUD.cls(lastLap.v, "gain", !f.perLapEstimated && f.perLap != null && f.lastLapUse != null && f.lastLapUse < f.perLap - 0.01);
        HUD.cls(lastLap.v, "loss", !f.perLapEstimated && f.perLap != null && f.lastLapUse != null && f.lastLapUse > f.perLap + 0.01);
        HUD.text(lapsLeft.v, f.lapsLeft == null ? number(null, 1) : estimate + number(f.lapsLeft, 1));
        const officialLaps = race && s.session.fixedTotalLaps != null && s.session.currentLap != null ? Math.max(0, s.session.fixedTotalLaps - s.session.currentLap + 1 - s.player.car.pct) : null;
        HUD.text(raceLaps.v, number(officialLaps, 1));
        HUD.text(toFinish.v, number(f.toFinish, 1));
        HUD.text(empty.v, f.emptyIn == null ? "—" : F.clock(f.emptyIn));
        const short = f.liters != null && f.toFinish != null && f.liters < f.toFinish;
        const lowReserve = !short && f.toAdd != null && f.toAdd > 0;
        HUD.text(addEl, f.toAdd == null ? "—" : F.fixed(f.toAdd, 1) + " L");
        HUD.text(addMsg, f.perLapEstimated ? "Game estimate is unverified" : f.toFinish == null || f.liters == null ? "" : short ? "Not enough to " + (race ? "finish" : "end") : lowReserve ? "Enough — low reserve" : "Enough to " + (race ? "finish" : "end"));
        HUD.cls(add, "short", short);
        HUD.cls(tank, "low", f.lapsLeft != null && f.lapsLeft < 3);
      },
    };
  },
});
