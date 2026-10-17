/* Delta: live delta to best lap with a centered bar, predicted lap and sector chips. */
import "./delta.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

HUD.register("delta", {
  title: "Delta",
  group: "core",
  w: 520,
  hz: 30,
  mount(root) {
    const { h } = HUD;
    const RANGE = 1.0; // seconds at full bar

    const predEl = h("span.num.dl-pred");
    const deltaEl = h("span.num.dl-value");
    const lastEl = h("span.num");
    const bestEl = h("span.num.pb");

    const fillL = h("i.dl-fill.neg");
    const fillR = h("i.dl-fill.pos");
    const bar = h("div.dl-bar", null,
      h("div.dl-half.l", null, fillL),
      h("div.dl-half.r", null, fillR),
      h("span.dl-tick", { style: { left: "25%" } }),
      h("span.dl-tick.zero", { style: { left: "50%" } }),
      h("span.dl-tick", { style: { left: "75%" } }));

    const sectors: { chip: HTMLElement; t: HTMLElement }[] = [];
    const secRow = h("div.dl-sectors");
    for (let k = 0; k < 3; k++) {
      const t = h("span.num");
      const chip = h("div.dl-sec", null, h("span.lbl", { text: "S" + (k + 1) }), t);
      secRow.append(chip);
      sectors.push({ chip, t });
    }

    const top = h("div.dl-top", null,
      h("div.dl-col", null, h("span.k", { text: "Predicted" }), predEl),
      h("div.dl-center", null, deltaEl),
      h("div.dl-col.r", null,
        h("div.kv", null, h("span.k", { text: "Last" }), lastEl),
        h("div.kv", null, h("span.k", { text: "Best" }), bestEl)));

    root.append(h("div.hud-panel", null, h("div.hud-body", null, top, bar, secRow)));

    return {
      update(s: ViewState) {
        const lap = s.player.lap;
        const d = Math.abs(lap.delta) < 0.005 ? 0 : lap.delta; // avoid "-0.00"
        const k = Math.min(1, Math.abs(d) / RANGE);
        fillL.style.transform = "scaleX(" + (d < 0 ? k : 0) + ")";
        fillR.style.transform = "scaleX(" + (d > 0 ? k : 0) + ")";
        HUD.text(deltaEl, F.delta(d));
        HUD.cls(deltaEl, "gain", d < 0);
        HUD.cls(deltaEl, "loss", d > 0);
        HUD.text(predEl, F.lap(lap.predicted));
        HUD.cls(predEl, "pb", lap.predicted > 0 && lap.predicted < lap.best);
        HUD.text(lastEl, F.lap(lap.last));
        HUD.text(bestEl, F.lap(lap.best));

        for (let i = 0; i < 3; i++) {
          const sc = sectors[i];
          const st = lap.sectorState[i] || "";
          HUD.data(sc.chip, "state", st || (i === lap.sector ? "live" : "next"));
          const secs = lap.sectors;
          let txt: string;
          if (st) txt = F.fixed(secs[i] ?? 0, 3);
          else if (i === lap.sector) txt = F.fixed(lap.current - (i > 0 ? secs[0] ?? 0 : 0) - (i > 1 ? secs[1] ?? 0 : 0), 1);
          else txt = lap.bestSectors[i] ? F.fixed(lap.bestSectors[i], 3) : "—";
          HUD.text(sc.t, txt);
        }
      },
    };
  },
});
