/* Relative: cars physically around the player on track, ahead on top. */
HUD.register("relative", {
  title: "Relative",
  group: "core",
  w: 380,
  hz: 10,
  mount(root) {
    const { h } = HUD;
    const AHEAD = 4, BEHIND = 4, ROWS = AHEAD + 1 + BEHIND;

    const lapEl = h("span.num");
    const remEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Relative" }), h("span.spacer"), lapEl, remEl);

    const rows = [];
    const list = h("div.hud-rows.zebra");
    for (let i = 0; i < ROWS; i++) {
      const pos = h("span.pos.num");
      const plate = h("span.badge.num-plate.num");
      const name = h("span.name.trunc");
      const cat = h("span.badge.cat");
      const pit = h("span.badge.pit", { text: "PIT" });
      const last = h("span.last.num.r");
      const gap = h("span.gap.num.r");
      const row = h("div.hud-row.rel-row", null, pos, plate, h("span.who", null, name, pit), cat, last, gap);
      list.append(row);
      rows.push({ row, pos, plate, name, cat, pit, last, gap });
    }

    const fLast = h("span.num");
    const fBest = h("span.num.pb");
    const fDelta = h("span.num");
    const foot = h("div.rel-foot", null,
      h("span.dim", { text: "Last" }), fLast,
      h("span.dim", { text: "Best" }), fBest,
      h("span.dim", { text: "Δ" }), fDelta);

    const panel = h("div.hud-panel", null, head, list, foot);
    root.append(panel);

    const ahead = [], behind = [];
    const byGapAsc = (a, b) => a.relGap - b.relGap;

    function fill(r, c, isPlayer) {
      if (!c) { HUD.cls(r.row, "is-empty", true); return; }
      HUD.cls(r.row, "is-empty", false);
      HUD.cls(r.row, "is-player", isPlayer);
      HUD.data(r.row, "cls", c.cls);
      HUD.cls(r.row, "lapping", !isPlayer && c.relLaps > 0);
      HUD.cls(r.row, "lapped", !isPlayer && c.relLaps < 0);
      HUD.cls(r.row, "in-pit", c.inPit);
      HUD.text(r.pos, "" + c.classPos);
      HUD.text(r.plate, "" + c.number);
      HUD.text(r.name, c.short);
      HUD.data(r.cat, "cat", c.cat);
      HUD.text(r.cat, c.cat);
      HUD.text(r.last, F.lap(c.lastLap));
      HUD.text(r.gap, isPlayer ? "" : F.rel(c.relGap));
    }

    return {
      update(s) {
        ahead.length = 0;
        behind.length = 0;
        const cars = s.cars;
        for (let i = 0; i < cars.length; i++) {
          const c = cars[i];
          if (c.isPlayer) continue;
          if (c.relGap >= 0) ahead.push(c); else behind.push(c);
        }
        ahead.sort(byGapAsc);   // closest ahead first
        behind.sort(byGapAsc);  // furthest behind first
        // rows 0..3: ahead, furthest at top
        for (let i = 0; i < AHEAD; i++) fill(rows[AHEAD - 1 - i], ahead[i], false);
        fill(rows[AHEAD], s.player.car, true);
        for (let i = 0; i < BEHIND; i++) fill(rows[AHEAD + 1 + i], behind[behind.length - 1 - i], false);

        const p = s.player;
        HUD.text(lapEl, "Lap " + (p.car.laps + 1));
        HUD.text(remEl, F.clock(s.session.timeRemaining));
        HUD.text(fLast, F.lap(p.lap.last));
        HUD.text(fBest, F.lap(p.lap.best));
        HUD.text(fDelta, F.delta(p.lap.delta));
        HUD.cls(fDelta, "gain", p.lap.delta < 0);
        HUD.cls(fDelta, "loss", p.lap.delta > 0);
      },
    };
  },
});
