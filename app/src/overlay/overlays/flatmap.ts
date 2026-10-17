/* Flat map: the lap unrolled into one line (start/finish at both ends), every car at its lap
   position. Markers are cached nodes moved with translate only; they are rebuilt when the
   entry list or the track's sectors change. */
import "./flatmap.css";
import { HUD } from "../hud";
import type { ViewCar, ViewState, ViewTrack } from "../view";

interface Mark {
  c: ViewCar;
  el: HTMLElement;
  x: number;
  pit: boolean | null;
}

HUD.register("flatmap", {
  title: "Flat map",
  group: "awareness",
  w: 720,
  h: 56,
  hz: 10,
  mount(root, state) {
    const { h } = HUD;
    const X0 = 22, X1 = 698, SPAN = X1 - X0;

    const rail = h("div.fm-rail");
    const tickLayer = h("div");
    const ends = [h("div.fm-sf.a"), h("div.fm-sf.b")];
    const layer = h("div.fm-layer");
    const panel = h("div.hud-panel.fm-panel", null, rail, ...ends, tickLayer, layer);
    root.append(panel);
    // number tags for the cars directly ahead / behind on track
    const tagA = h("div.fm-tag.a.num"), tagB = h("div.fm-tag.b.num");

    function place(el: HTMLElement, x: number, y: number) {
      el.style.transform = "translate(" + x.toFixed(1) + "px," + y + "px)";
    }

    let track: ViewTrack | null = null;
    function setTrack(t: ViewTrack) {
      track = t;
      tickLayer.replaceChildren(
        ...t.sectors.slice(1).map((pct, k) =>
          h("div.fm-tick", { style: { transform: "translateX(" + (X0 + pct * SPAN) + "px)" } }, h("span", { text: "S" + (k + 2) }))),
      );
    }

    // one marker per car, built once per entry list
    let cars: ViewCar[] = [];
    let marks: Mark[] = [];
    function setCars(list: ViewCar[]) {
      cars = list.slice();
      marks = list.map((c) => {
        const el = h("div.fm-car" + (c.isPlayer ? ".me" : ""), { "data-cls": c.cls });
        if (c.isPlayer) el.append(h("span.fm-num.num", { text: "" + c.number }));
        return { c, el, x: -1, pit: null };
      });
      layer.replaceChildren(...marks.map((m) => m.el), tagA, tagB);
      // player marker on top
      const me = marks.find((m) => m.c.isPlayer);
      if (me) layer.append(me.el);
    }
    const changed = (list: ViewCar[]) =>
      list.length !== cars.length || list.some((c, i) => c !== cars[i] || c.cls !== marks[i].el.dataset.cls);

    setTrack(state.track);
    setCars(state.cars);

    return {
      update(s: ViewState) {
        if (s.track !== track) setTrack(s.track);
        if (changed(s.cars)) setCars(s.cars);
        let ahead: ViewCar | null = null, behind: ViewCar | null = null;
        for (let i = 0; i < marks.length; i++) {
          const m = marks[i], c = m.c;
          const x = Math.round((X0 + c.pct * SPAN) * 2) / 2;
          if (c.inPit !== m.pit) { m.pit = c.inPit; HUD.cls(m.el, "pit", c.inPit); }
          if (x !== m.x || m.pit) { m.x = x; place(m.el, x, c.inPit ? 9 : 0); }
          if (c.isPlayer || c.inPit) continue;
          if (c.relMeters > 0 && (!ahead || c.relMeters < ahead.relMeters)) ahead = c;
          if (c.relMeters < 0 && (!behind || c.relMeters > behind.relMeters)) behind = c;
        }
        for (const [tag, c] of [[tagA, ahead], [tagB, behind]] as const) {
          HUD.cls(tag, "on", !!c);
          if (!c) continue;
          HUD.text(tag, "" + c.number);
          HUD.data(tag, "cls", c.cls);
          place(tag, X0 + c.pct * SPAN, 0);
        }
      },
    };
  },
});
