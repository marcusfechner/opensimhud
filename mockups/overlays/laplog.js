/* Lap log: the player's laps, newest on top, with the running lap live in the first row. */
HUD.register("laplog", {
  title: "Lap log",
  group: "core",
  w: 380,
  hz: 4,
  mount(root) {
    const { h } = HUD;
    const ROWS = 8;

    // icons (built once at mount)
    const icon = (svg) => {
      const t = document.createElement("template");
      t.innerHTML = svg.trim();
      return t.content.firstChild;
    };
    const INVALID = '<svg class="ll-ic invalid" viewBox="0 0 12 12" aria-label="Invalid lap"><circle cx="6" cy="6" r="4.6" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M2.8 9.2l6.4-6.4" stroke="currentColor" stroke-width="1.4"/></svg>';
    const PIT = '<svg class="ll-ic pit" viewBox="0 0 12 12" aria-label="Pit lap"><rect x="0.5" y="0.5" width="11" height="11" rx="3" fill="currentColor"/><path d="M4.3 9V3h2.2a1.8 1.8 0 0 1 0 3.6H4.3" fill="none" stroke="#fff" stroke-width="1.4"/></svg>';

    const bestEl = h("span.num.pb");
    const head = h("div.hud-head", null, h("span.title", { text: "Lap log" }), h("span.spacer"), h("span", { text: "Best" }), bestEl);
    const cols = h("div.hud-row.ll-row.ll-cols", null,
      h("span", { text: "Lap" }), h("span", { text: "Time" }), h("span.r", { text: "Δ" }),
      h("span.r", { text: "S1" }), h("span.r", { text: "S2" }), h("span.r", { text: "S3" }), h("span.r", { text: "Fuel" }));

    function makeRow(live) {
      const lap = h("span.lap.num");
      const pit = icon(PIT);
      const time = h("span.t.num");
      const inv = icon(INVALID);
      const delta = h("span.d.num.r");
      const sec = [h("span.s.num.r"), h("span.s.num.r"), h("span.s.num.r")];
      const fuel = h("span.f.num.r");
      const row = h("div.hud-row.ll-row" + (live ? ".is-live" : ""), null,
        h("span.lapc", null, lap, pit), h("span.tc", null, time, inv), delta, sec[0], sec[1], sec[2], fuel);
      return { row, lap, pit, time, inv, delta, sec, fuel };
    }

    const live = makeRow(true);
    const rows = [];
    const list = h("div.hud-rows", null, live.row);
    for (let i = 0; i < ROWS; i++) {
      const r = makeRow(false);
      rows.push(r);
      list.append(r.row);
    }
    root.append(h("div.hud-panel", null, head, cols, list));

    const fmtSec = (v) => (v > 0 ? v.toFixed(2) : "");
    let lastLen = -1, lastSig = "";
    let pbTime = 0;
    const bestSec = [0, 0, 0];

    function fillCompleted(log) {
      // best valid lap (lap 1 is a standing start, skip it) and best sectors across the log
      pbTime = 0;
      bestSec[0] = bestSec[1] = bestSec[2] = 0;
      for (const e of log) {
        if (!e.valid || e.lap === 1) continue;
        if (!pbTime || e.time < pbTime) pbTime = e.time;
        for (let k = 0; k < 3; k++) if (!bestSec[k] || e.sectors[k] < bestSec[k]) bestSec[k] = e.sectors[k];
      }
      for (let i = 0; i < ROWS; i++) {
        const r = rows[i], e = log[log.length - 1 - i];
        HUD.cls(r.row, "is-empty", !e);
        if (!e) continue;
        const isPb = e.valid && e.time === pbTime;
        HUD.cls(r.row, "is-pb", isPb);
        HUD.cls(r.row, "is-invalid", !e.valid);
        HUD.cls(r.row, "is-pit", !!e.pit);
        HUD.text(r.lap, "" + e.lap);
        HUD.text(r.time, F.lap(e.time));
        const d = e.time - pbTime;
        HUD.text(r.delta, isPb ? "PB" : F.delta(d));
        HUD.cls(r.delta, "pb", isPb);
        HUD.cls(r.delta, "loss", !isPb && d > 0);
        HUD.cls(r.delta, "gain", !isPb && d < 0);
        for (let k = 0; k < 3; k++) {
          HUD.text(r.sec[k], fmtSec(e.sectors[k]));
          HUD.cls(r.sec[k], "sb", e.valid && e.sectors[k] === bestSec[k]);
        }
        HUD.text(r.fuel, e.fuel ? e.fuel.toFixed(2) : "");
      }
      HUD.text(bestEl, F.lap(pbTime));
    }

    return {
      update(s) {
        const p = s.player, lap = p.lap, log = p.log;
        const last = log[log.length - 1];
        const sig = log.length + ":" + (last ? last.time : 0);
        if (sig !== lastSig) {
          lastSig = sig;
          fillCompleted(log);
        }

        // running lap
        HUD.text(live.lap, "" + (p.car.laps + 1));
        const cur = lap.current;
        HUD.text(live.time, F.lap(cur).slice(0, -2)); // tenths while running
        HUD.cls(live.row, "is-invalid", !lap.valid);
        HUD.text(live.delta, F.delta(lap.delta));
        HUD.cls(live.delta, "gain", lap.delta < 0);
        HUD.cls(live.delta, "loss", lap.delta > 0);
        for (let k = 0; k < 3; k++) {
          const done = k < lap.sector;
          HUD.text(live.sec[k], done ? fmtSec(lap.sectors[k]) : k === lap.sector ? "…" : "");
          HUD.cls(live.sec[k], "sb", done && lap.sectorState[k] === "sb");
          HUD.cls(live.sec[k], "pb", done && lap.sectorState[k] === "pb");
        }
        const used = p.fuel.lapStartFuel - p.fuel.liters;
        HUD.text(live.fuel, used > 0 ? used.toFixed(2) : "");
      },
    };
  },
});
