/* Lap log: the player's laps, newest on top, with the running lap live in the first row. */
import "./laplog.css";
import { extraRows, HUD } from "../hud";
import { SIZES } from "../../shared/defaults";
import { F } from "../fmt";
import type { ViewState } from "../view";
import type { LapEntry } from "../../types/telemetry/LapEntry";

interface Row {
  row: HTMLElement;
  lap: HTMLElement;
  time: HTMLElement;
  delta: HTMLElement;
  sec: HTMLElement[];
  fuel: HTMLElement;
}

HUD.register("laplog", {
  title: "Lap log",
  group: "core",
  w: 380,
  hz: 4,
  resize: "height",
  minH: 120,
  mount(root, _state, opts) {
    const { h } = HUD;
    const ROWS = Math.max(1, 8 + extraRows(opts.h, SIZES.laplog.h)); // taller lists more laps

    // icons (built once at mount)
    const icon = (svg: string) => {
      const t = document.createElement("template");
      t.innerHTML = svg.trim();
      return t.content.firstChild as Element;
    };
    const INVALID = '<svg class="ll-ic invalid" viewBox="0 0 12 12" aria-label="Invalid lap"><circle cx="6" cy="6" r="4.6" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M2.8 9.2l6.4-6.4" stroke="currentColor" stroke-width="1.4"/></svg>';
    const PIT = '<svg class="ll-ic pit" viewBox="0 0 12 12" aria-label="Pit lap"><rect x="0.5" y="0.5" width="11" height="11" rx="3" fill="currentColor"/><path d="M4.3 9V3h2.2a1.8 1.8 0 0 1 0 3.6H4.3" fill="none" stroke="#fff" stroke-width="1.4"/></svg>';

    const bestEl = h("span.num.pb");
    const theoEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Lap log" }), h("span.spacer"),
      h("span", { text: "Best" }), bestEl, h("span.ll-theo", { text: "Theoretical" }), theoEl);
    const cols = h("div.hud-row.ll-row.ll-cols", null,
      h("span", { text: "Lap" }), h("span", { text: "Time" }), h("span.r", { text: "Δ" }),
      h("span.r", { text: "S1" }), h("span.r", { text: "S2" }), h("span.r", { text: "S3" }), h("span.r", { text: "Fuel" }));

    function makeRow(live: boolean): Row {
      const lap = h("span.lap.num");
      const pit = icon(PIT);
      const time = h("span.t.num");
      const inv = icon(INVALID);
      const delta = h("span.d.num.r");
      const sec = [h("span.s.num.r"), h("span.s.num.r"), h("span.s.num.r")];
      const fuel = h("span.f.num.r");
      const row = h("div.hud-row.ll-row" + (live ? ".is-live" : ""), null,
        h("span.lapc", null, lap, pit), h("span.tc", null, time, inv), delta, sec[0], sec[1], sec[2], fuel);
      return { row, lap, time, delta, sec, fuel };
    }

    const live = makeRow(true);
    const rows: Row[] = [];
    const list = h("div.hud-rows", null, live.row);
    for (let i = 0; i < ROWS; i++) {
      const r = makeRow(false);
      rows.push(r);
      list.append(r.row);
    }
    root.append(h("div.hud-panel", null, head, cols, list));

    const fmtSec = (v: number | undefined) => (v && v > 0 ? v.toFixed(2) : "");
    let lastSig = "";
    let pbTime = 0;

    function fillCompleted(log: LapEntry[], bestSec: number[]) {
      // best valid lap (lap 1 is a standing start, skip it); best sectors come from core,
      // which only counts sectors driven clean
      pbTime = 0;
      for (const e of log) {
        if (!e.valid || e.pit || e.lap === 1) continue;
        if (!pbTime || e.time < pbTime) pbTime = e.time;
      }
      for (let i = 0; i < ROWS; i++) {
        const r = rows[i], e = log[log.length - 1 - i];
        HUD.cls(r.row, "is-empty", !e);
        if (!e) continue;
        const isPb = e.valid && e.time === pbTime;
        HUD.cls(r.row, "is-pb", isPb);
        HUD.cls(r.row, "is-invalid", !e.valid);
        HUD.cls(r.row, "is-pit", e.pit);
        HUD.text(r.lap, "" + e.lap);
        HUD.text(r.time, F.lap(e.time));
        const d = e.time - pbTime;
        HUD.text(r.delta, isPb ? "PB" : pbTime ? F.delta(d) : "");
        HUD.cls(r.delta, "pb", isPb);
        HUD.cls(r.delta, "loss", !isPb && pbTime > 0 && d > 0);
        HUD.cls(r.delta, "gain", !isPb && pbTime > 0 && d < 0);
        for (let k = 0; k < 3; k++) {
          HUD.text(r.sec[k], fmtSec(e.sectors[k]));
          const clean = e.clean[k] ?? e.valid;
          HUD.cls(r.sec[k], "sb", clean && e.sectors[k] > 0 && e.sectors[k] === bestSec[k]);
          HUD.cls(r.sec[k], "dirty", !clean);
        }
        HUD.text(r.fuel, e.fuel ? e.fuel.toFixed(2) : "");
      }
      HUD.text(bestEl, F.lap(pbTime));
    }

    return {
      update(s: ViewState) {
        const p = s.player, lap = p.lap, log = p.log;
        const last = log[log.length - 1];
        const sig = log.length + ":" + (last ? last.lap + "/" + last.time : 0) + ":" + lap.bestSectors.join();
        if (sig !== lastSig) {
          lastSig = sig;
          fillCompleted(log, lap.bestSectors);
        }
        if (s.game === "evo") HUD.text(bestEl, F.lap(lap.best));
        HUD.text(theoEl, F.lap(lap.theoretical));

        // running lap
        HUD.text(live.lap, "" + (p.car.laps + 1));
        HUD.text(live.time, F.lap(lap.current).slice(0, -2)); // tenths while running
        HUD.cls(live.row, "is-invalid", !lap.valid);
        HUD.text(live.delta, lap.best > 0 ? F.delta(lap.delta) : "");
        HUD.cls(live.delta, "gain", lap.best > 0 && lap.delta < 0);
        HUD.cls(live.delta, "loss", lap.best > 0 && lap.delta > 0);
        for (let k = 0; k < 3; k++) {
          const done = k < lap.sector;
          HUD.text(live.sec[k], done ? fmtSec(lap.sectors[k]) : k === lap.sector ? "…" : "");
          HUD.cls(live.sec[k], "sb", done && lap.sectorState[k] === "sb");
          HUD.cls(live.sec[k], "pb", done && lap.sectorState[k] === "pb");
          HUD.cls(live.sec[k], "dirty", done && lap.sectorState[k] === "invalid");
        }
        const used = p.fuel.usedThisLap;
        HUD.text(live.fuel, used > 0 ? used.toFixed(2) : "");
      },
    };
  },
});
