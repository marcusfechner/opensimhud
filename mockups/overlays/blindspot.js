/* Blind spot indicator: two small mirror-style lights. Amber = car in the blind spot
   (just behind and beside you), red = fully alongside. */
HUD.register("blindspot", {
  title: "Blind spot",
  group: "awareness",
  w: 640,
  h: 60,
  hz: 30,
  mount(root) {
    const { h } = HUD;
    const CAR = 4.6;
    const LAT_MAX = 6;
    const SVG = "http://www.w3.org/2000/svg";

    function chevron(dir) {
      const svg = document.createElementNS(SVG, "svg");
      svg.setAttribute("viewBox", "0 0 24 24");
      svg.setAttribute("aria-hidden", "true");
      const p = document.createElementNS(SVG, "path");
      p.setAttribute("d", dir === "l" ? "M14.5 5 7.5 12l7 7" : "M9.5 5l7 7-7 7");
      svg.append(p);
      return svg;
    }
    function light(name) {
      const label = h("span.bs-label.num");
      const el = h("div.bs-light." + name, null, name === "l" ? chevron("l") : label, name === "l" ? label : chevron("r"));
      return { el, label, level: -1 };
    }
    const L = light("l"), R = light("r");
    root.append(h("div.bs-strip", null, L.el, R.el));

    function set(s, level, along) {
      if (level !== s.level) { s.level = level; HUD.data(s.el, "level", "" + level); }
      if (level === 2) HUD.text(s.label, "Alongside");
      else if (level === 1) HUD.text(s.label, Math.max(1, Math.round(-along)) + " m");
    }

    return {
      update(st) {
        const me = st.player.car;
        let l = 0, r = 0, la = 0, ra = 0;
        const cars = st.cars;
        for (let i = 0; i < cars.length; i++) {
          const c = cars[i];
          if (c.isPlayer || c.inPit) continue;
          const along = c.relMeters;
          if (along < -8 || along > CAR) continue;
          const lat = c.lateral - me.lateral;
          if (lat > LAT_MAX || lat < -LAT_MAX || Math.abs(lat) < 1.2) continue;
          // 2 = at least half alongside (same threshold as radar bars), 1 = in the blind spot behind-alongside
          const level = Math.abs(along) < CAR * 0.5 ? 2 : along <= 2 ? 1 : 0;
          if (lat < 0) { if (level > l) { l = level; la = along; } }
          else if (level > r) { r = level; ra = along; }
        }
        set(L, l, la);
        set(R, r, ra);
      },
    };
  },
});
