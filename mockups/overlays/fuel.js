/* Fuel: tank level, consumption and how much to add at the next stop. */
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

    const stat = (k) => {
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
      update(s) {
        const f = s.player.fuel;
        const lap = s.player.lap;
        HUD.text(litersEl, F.fixed(f.liters, 1));
        HUD.text(capEl, "of " + f.capacity + " L");
        tankFill.style.transform = "scaleX(" + (f.liters / f.capacity).toFixed(4) + ")";
        HUD.text(lapsLeftEl, F.fixed(f.lapsLeft, 1) + " laps");

        HUD.text(perLap.v, F.fixed(f.perLap, 2));
        HUD.text(lastLap.v, F.fixed(f.lastLapUse, 2));
        HUD.cls(lastLap.v, "gain", f.lastLapUse < f.perLap - 0.01);
        HUD.cls(lastLap.v, "loss", f.lastLapUse > f.perLap + 0.01);
        HUD.text(lapsLeft.v, F.fixed(f.lapsLeft, 1));
        HUD.text(raceLaps.v, F.fixed(s.session.lapRemaining, 1));
        HUD.text(toFinish.v, F.fixed(f.toFinish, 1));
        HUD.text(empty.v, F.clock(f.lapsLeft * (lap.best || 1)));

        const short = f.liters < f.toFinish;
        HUD.text(addEl, short ? "+" + F.fixed(f.toAdd, 1) + " L" : "—");
        HUD.text(addMsg, short ? "Not enough to finish" : "Enough to finish");
        HUD.cls(add, "short", short);
        HUD.cls(tank, "low", f.lapsLeft < 3);
      },
    };
  },
});
