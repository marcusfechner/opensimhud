/* Electronics: shift lights, gear + speed, and TC / TC2 / ABS / BB / Map tiles.
   A setting the driver just changed flashes and keeps an accent outline briefly;
   the ABS and TC tiles light up while the system is intervening. */
import "./electronics.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

type Key = "tc" | "tc2" | "abs" | "bb" | "map";

HUD.register("electronics", {
  title: "Electronics",
  group: "car",
  w: 360,
  hz: 20,
  mount(root, state) {
    const { h } = HUD;
    const SEGS = 12, RECENT = 2.5;

    const segs: HTMLElement[] = [];
    const strip = h("div.el-strip");
    for (let i = 0; i < SEGS; i++) {
      const seg = h("i.el-seg");
      seg.dataset.zone = i < 5 ? "g" : i < 9 ? "y" : "r";
      strip.append(seg);
      segs.push(seg);
    }

    const gear = h("span.el-gear.num");
    const speed = h("span.el-speed.num");
    const drive = h("div.el-drive", null, gear, h("div.el-speed-box", null, speed, h("small", { text: "km/h" })));

    const TILES: [Key, string][] = [["tc", "TC"], ["tc2", "TC2"], ["abs", "ABS"], ["bb", "BB"], ["map", "Map"]];
    const tiles = {} as Record<Key, { tile: HTMLElement; v: HTMLElement }>;
    const tileRow = h("div.el-tiles");
    for (const [key, label] of TILES) {
      const v = h("span.el-val.num");
      const tile = h("div.el-tile.flash-host", { "data-k": key }, h("span.el-lbl", { text: label }), v);
      tileRow.append(tile);
      tiles[key] = { tile, v };
    }

    root.append(h("div.hud-panel", null, strip, h("div.el-body", null, drive, tileRow)));

    let seenChange = state.player.changedAt;

    return {
      update(s: ViewState) {
        const p = s.player;

        // shift lights
        const start = p.maxRpm * 0.7;
        const frac = (p.rpm - start) / (p.shiftRpm - start);
        const lit = Math.max(0, Math.min(SEGS, Math.ceil(frac * SEGS)));
        const shift = p.shiftRpm > 0 && p.rpm >= p.shiftRpm * 0.985;
        HUD.cls(strip, "shift", shift);
        for (let i = 0; i < SEGS; i++) HUD.cls(segs[i], "on", i < lit);

        HUD.text(gear, F.gear(p.gear));
        HUD.text(speed, F.int(p.speed));

        HUD.text(tiles.tc.v, "" + p.tc);
        HUD.text(tiles.tc2.v, "" + p.tc2);
        HUD.text(tiles.abs.v, "" + p.abs);
        HUD.text(tiles.bb.v, p.bb.toFixed(1));
        HUD.text(tiles.map.v, "" + p.map);
        HUD.cls(tiles.abs.tile, "active", p.absActive);
        HUD.cls(tiles.tc.tile, "active", p.tcActive);

        if (p.changedAt !== seenChange) {
          seenChange = p.changedAt;
          const t = tiles[p.changed as Key];
          if (t) HUD.flash(t.tile);
        }
        const recent = s.t - p.changedAt < RECENT;
        for (const [key] of TILES) HUD.cls(tiles[key].tile, "recent", recent && p.changed === key);
      },
    };
  },
});
