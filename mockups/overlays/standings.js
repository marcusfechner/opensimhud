/* Standings: multiclass leaderboard with class headers and player context. */
HUD.register("standings", {
  title: "Standings",
  group: "core",
  w: 530,
  hz: 4,
  mount(root, state) {
    const { h } = HUD;
    const TOP = { GT3: 10, GT4: 4 };

    const lapEl = h("span.num");
    const remEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Standings" }), h("span.spacer"), lapEl, remEl);
    const list = h("div.hud-rows.zebra");

    // class start positions (rank of startPos within class), computed once
    const classStart = new Map();
    for (const cl of state.classes) {
      state.cars
        .filter((c) => c.cls === cl)
        .sort((a, b) => a.startPos - b.startPos)
        .forEach((c, i) => classStart.set(c, i + 1));
    }

    function makeHeader(cl) {
      const count = h("span.dim.num");
      const best = h("span.num.sb");
      const row = h("div.std-class", { "data-cls": cl },
        h("span.chip"), h("span.cl-name", { text: cl }), count, h("span.spacer"), h("span.dim", { text: "Best" }), best);
      list.append(row);
      return { row, count, best };
    }
    // status icons (static SVG, built once per row and toggled by row classes)
    const SVG = {
      fastest: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="9.2" r="5.2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M8 9.2V6.4M6.4 2.2h3.2M12.3 4.7l1-1" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>',
      stops: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="5.8" fill="none" stroke="currentColor" stroke-width="1.7"/><circle cx="8" cy="8" r="2.2" fill="currentColor"/></svg>',
    };
    function icon(cls, title, svg) {
      const el = h("span.ico." + cls, { title });
      el.innerHTML = svg;
      return el;
    }
    function makeRow() {
      const pos = h("span.pos.num.r");
      const chg = h("span.chg.num");
      const plate = h("span.badge.num-plate.num");
      const name = h("span.name.trunc");
      const fastest = icon("fastest", "Fastest lap in class", SVG.fastest);
      const stopsN = h("span.num");
      const stops = icon("stops", "Pit stops done", SVG.stops);
      stops.append(stopsN);
      const pit = h("span.badge.pit", { text: "PIT" });
      const out = h("span.badge.out", { text: "OUT" });
      const pen = h("span.badge.pen");
      const cat = h("span.badge.cat");
      const last = h("span.num.r.last");
      const best = h("span.num.r.best");
      const gap = h("span.num.r.gap");
      const int = h("span.num.r.int");
      const row = h("div.hud-row.std-row", null, pos, chg, plate,
        h("span.who", null, name, h("span.status", null, fastest, pen, pit, out, stops)), cat, last, best, gap, int);
      list.append(row);
      return { row, pos, chg, plate, name, stopsN, pen, cat, last, best, gap, int };
    }

    const hdr = {};
    const pools = {};
    hdr.GT3 = makeHeader("GT3");
    pools.GT3 = Array.from({ length: TOP.GT3 }, makeRow);
    const sep = h("div.std-sep", null, h("span", { text: "⋯" }));
    list.append(sep);
    const ctx = Array.from({ length: 3 }, makeRow);
    hdr.GT4 = makeHeader("GT4");
    pools.GT4 = Array.from({ length: TOP.GT4 }, makeRow);

    root.append(h("div.hud-panel", null, head, list));

    const byClass = { GT3: [], GT4: [] };

    function fill(r, c, s) {
      if (!c) { HUD.cls(r.row, "is-empty", true); return; }
      HUD.cls(r.row, "is-empty", false);
      HUD.data(r.row, "cls", c.cls);
      HUD.cls(r.row, "is-player", c.isPlayer);
      HUD.cls(r.row, "in-pit", c.inPit);
      HUD.cls(r.row, "out-lap", c.outLap && !c.inPit);
      HUD.cls(r.row, "pitted", c.pitStops > 0 && !c.inPit && !c.outLap);
      HUD.text(r.stopsN, "" + c.pitStops);
      HUD.cls(r.row, "has-pen", !!c.penalty);
      HUD.text(r.pen, c.penalty);
      HUD.text(r.pos, "" + c.classPos);
      const d = classStart.get(c) - c.classPos;
      HUD.text(r.chg, d > 0 ? "▲" + d : d < 0 ? "▼" + -d : "–");
      HUD.cls(r.chg, "gain", d > 0);
      HUD.cls(r.chg, "loss", d < 0);
      HUD.text(r.plate, "" + c.number);
      HUD.text(r.name, c.short);
      HUD.data(r.cat, "cat", c.cat);
      HUD.text(r.cat, c.cat);
      HUD.text(r.last, F.lap(c.lastLap));
      const b = s.session.best[c.cls];
      const isSb = b.car === c && c.lastLap === c.bestLap;
      HUD.cls(r.last, "sb", isSb);
      HUD.cls(r.last, "pb", !isSb && c.lastLap === c.bestLap);
      HUD.text(r.best, F.lap(c.bestLap));
      HUD.cls(r.best, "sb", b.car === c);
      HUD.cls(r.row, "fastest", b.car === c);
      HUD.text(r.gap, c.classPos === 1 ? "Leader" : F.gap(c.gapLeader, c.lapsDown));
      HUD.cls(r.gap, "dim", c.classPos === 1);
      HUD.text(r.int, c.classPos === 1 ? "" : F.gap(c.gapAhead, c.lapsAhead));
    }

    return {
      update(s) {
        byClass.GT3.length = 0;
        byClass.GT4.length = 0;
        const order = s.order;
        for (let i = 0; i < order.length; i++) byClass[order[i].cls].push(order[i]);

        for (const cl of s.classes) {
          const arr = byClass[cl], pool = pools[cl], H = hdr[cl];
          HUD.text(H.count, arr.length + " cars");
          HUD.text(H.best, F.lap(s.session.best[cl].time));
          for (let i = 0; i < pool.length; i++) fill(pool[i], arr[i], s);
        }

        // player context when outside the GT3 top rows
        const p = s.player.car;
        const arr = byClass[p.cls];
        const shown = TOP[p.cls];
        let start = 0, end = -1;
        if (p.classPos > shown) {
          start = Math.max(shown + 1, p.classPos - 1);
          end = Math.min(arr.length, p.classPos + 1);
        }
        HUD.cls(sep, "is-hidden", !(end >= start && start > shown + 1));
        for (let i = 0; i < ctx.length; i++) {
          const cp = start + i;
          fill(ctx[i], end >= start && cp <= end ? arr[cp - 1] : null, s);
          HUD.cls(ctx[i].row, "is-gone", !(end >= start && cp <= end));
        }

        HUD.text(lapEl, "Lap " + (s.order[0].laps + 1));
        HUD.text(remEl, F.clock(s.session.timeRemaining));
      },
    };
  },
});
