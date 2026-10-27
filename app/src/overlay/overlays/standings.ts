/* Standings: multiclass leaderboard with class headers and player context. */
import "./standings.css";
import { extraRows, HUD } from "../hud";
import { SIZES } from "../../shared/defaults";
import { F } from "../fmt";
import type { ViewCar, ViewState } from "../view";

interface Row {
  row: HTMLElement;
  pos: HTMLElement;
  chg: HTMLElement;
  plate: HTMLElement;
  name: HTMLElement;
  stopsN: HTMLElement;
  pen: HTMLElement;
  cat: HTMLElement;
  last: HTMLElement;
  best: HTMLElement;
  gap: HTMLElement;
  int: HTMLElement;
}

interface Section {
  cls: string;
  header: { row: HTMLElement; count: HTMLElement; best: HTMLElement };
  pool: Row[];
  cars: ViewCar[];
}

// status icons (static SVG, built once per row and toggled by row classes)
const SVG = {
  fastest: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="9.2" r="5.2" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M8 9.2V6.4M6.4 2.2h3.2M12.3 4.7l1-1" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>',
  stops: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="5.8" fill="none" stroke="currentColor" stroke-width="1.7"/><circle cx="8" cy="8" r="2.2" fill="currentColor"/></svg>',
};

HUD.register("standings", {
  title: "Standings",
  group: "core",
  w: 530,
  hz: 4,
  resize: "height",
  minH: 160,
  mount(root, _state, opts) {
    const { h } = HUD;
    // rows per class: the first class gets the most room; a single-class field gets more.
    // Resizing the height adds or removes rows from the first class.
    const extra = extraRows(opts.h, SIZES.standings.h);
    const topRows = (k: number, classes: number) => (k > 0 ? 4 : Math.max(3, (classes === 1 ? 14 : 10) + extra));

    const lapEl = h("span.num");
    const remEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Standings" }), h("span.spacer"), lapEl, remEl);
    const list = h("div.hud-rows.zebra");
    root.append(h("div.hud-panel", null, head, list));

    function icon(cls: string, title: string, svg: string) {
      const el = h("span.ico." + cls, { title });
      el.innerHTML = svg;
      return el;
    }
    function makeHeader(cl: string) {
      const count = h("span.dim.num");
      const best = h("span.num.sb");
      const row = h("div.std-class", { "data-cls": cl },
        h("span.chip"), h("span.cl-name", { text: cl }), count, h("span.spacer"), h("span.dim", { text: "Best" }), best);
      list.append(row);
      return { row, count, best };
    }
    function makeRow(): Row {
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

    // Sections are rebuilt only when the classes or the player's class change (rare).
    let sections: Section[] = [];
    let ctx: Row[] = [];
    let sep: HTMLElement = h("div.std-sep");
    let built = "";
    function build(s: ViewState) {
      list.replaceChildren();
      const classes = s.classes.length ? s.classes : [s.player.car.cls];
      sections = classes.map((cls, k) => {
        const header = makeHeader(cls);
        const pool = Array.from({ length: topRows(k, classes.length) }, makeRow);
        if (cls === s.player.car.cls) {
          sep = h("div.std-sep", null, h("span", { text: "⋯" }));
          list.append(sep);
          ctx = Array.from({ length: 3 }, makeRow);
        }
        return { cls, header, pool, cars: [] };
      });
    }

    /** Position in class at the start (rank of startPos within the class). */
    function classStart(c: ViewCar, s: ViewState) {
      let n = 1;
      for (const o of s.cars) if (o.cls === c.cls && o.startPos > 0 && o.startPos < c.startPos) n++;
      return n;
    }

    function fill(r: Row, c: ViewCar | undefined, s: ViewState) {
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
      const d = classStart(c, s) - c.classPos;
      HUD.text(r.chg, d > 0 ? "▲" + d : d < 0 ? "▼" + -d : "–");
      HUD.cls(r.chg, "gain", d > 0);
      HUD.cls(r.chg, "loss", d < 0);
      HUD.text(r.plate, "" + c.number);
      HUD.text(r.name, c.short);
      HUD.data(r.cat, "cat", c.cat);
      HUD.text(r.cat, c.cat);
      HUD.cls(r.cat, "is-hidden", !c.cat);
      HUD.text(r.last, F.lap(c.lastLap));
      const holder = s.session.best[c.cls]?.car === c;
      const isPb = c.lastLap > 0 && c.lastLap === c.bestLap;
      HUD.cls(r.last, "sb", holder && isPb);
      HUD.cls(r.last, "pb", !holder && isPb);
      HUD.text(r.best, F.lap(c.bestLap));
      HUD.cls(r.best, "sb", holder);
      HUD.cls(r.row, "fastest", holder);
      HUD.text(r.gap, c.classPos === 1 ? "Leader" : F.gap(c.gapLeader, c.lapsDown));
      HUD.cls(r.gap, "dim", c.classPos === 1);
      HUD.text(r.int, c.classPos === 1 ? "" : F.gap(c.gapAhead, c.lapsAhead));
    }

    return {
      update(s: ViewState) {
        const sig = s.classes.join("|") + "/" + s.player.car.cls;
        if (sig !== built) {
          built = sig;
          build(s);
        }
        for (const sec of sections) sec.cars.length = 0;
        for (const c of s.order) sections.find((sec) => sec.cls === c.cls)?.cars.push(c);

        for (const sec of sections) {
          HUD.text(sec.header.count, sec.cars.length + " cars");
          HUD.text(sec.header.best, F.lap(s.session.best[sec.cls]?.time ?? 0));
          for (let i = 0; i < sec.pool.length; i++) fill(sec.pool[i], sec.cars[i], s);
        }

        // player context when outside their class's top rows
        const p = s.player.car;
        const own = sections.find((sec) => sec.cls === p.cls);
        const arr = own ? own.cars : [];
        const shown = own ? own.pool.length : 0;
        let start = 0, end = -1;
        if (p.classPos > shown) {
          start = Math.max(shown + 1, p.classPos - 1);
          end = Math.min(arr.length, p.classPos + 1);
        }
        HUD.cls(sep, "is-hidden", !(end >= start && start > shown + 1));
        for (let i = 0; i < ctx.length; i++) {
          const cp = start + i;
          fill(ctx[i], end >= start && cp <= end ? arr[cp - 1] : undefined, s);
          HUD.cls(ctx[i].row, "is-gone", !(end >= start && cp <= end));
        }

        HUD.text(lapEl, s.order.length ? "Lap " + (s.order[0].laps + 1) : "");
        HUD.text(remEl, F.clock(s.session.timeRemaining));
      },
    };
  },
});
