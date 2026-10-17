/* HUD runtime: overlay registry, single render loop, theme handling.

   Overlay contract (mirrors what a Tauri webview overlay will do later):
     HUD.register(id, {
       title, group, w, h,            // default size in px at 1080p
       hz,                            // update rate (render loop calls update at most this often)
       mount(root, state) -> { update(state, dt), theme?(), destroy?() }
     })

   - mount() builds the DOM once and caches node refs.
   - update() only writes textContent / transform / classes / data-* on cached nodes.
   - theme() is called after HUD.restyle() (canvas overlays re-read colors there). */
(function () {
  const HUD = (window.HUD = window.HUD || {});
  HUD.overlays = HUD.overlays || {};

  HUD.register = function (id, def) {
    def.id = id;
    HUD.overlays[id] = def;
  };

  /* ---------- restyle ---------- */
  /* Call after design tokens are overridden at runtime (e.g. the accent a user picks in the app)
     so overlays that cache colors — canvas ones — re-read them. */
  const themeSubs = new Set();
  HUD.restyle = function () {
    cssCache.clear();
    // wait one frame so computed styles reflect the new tokens
    requestAnimationFrame(() => themeSubs.forEach((fn) => fn()));
  };
  HUD.onTheme = (fn) => (themeSubs.add(fn), () => themeSubs.delete(fn));

  /** Read a CSS custom property (cached until the next restyle). Use in canvas overlays. */
  const cssCache = new Map();
  HUD.css = function (name, el) {
    const key = name;
    let v = cssCache.get(key);
    if (v === undefined) {
      v = getComputedStyle(el || document.documentElement).getPropertyValue(name).trim();
      cssCache.set(key, v);
    }
    return v;
  };

  /* ---------- render loop ---------- */
  const subs = [];
  HUD.speed = 1;
  HUD.paused = false;

  HUD.subscribe = function (fn, hz) {
    const s = { fn, interval: hz ? 1000 / hz : 0, acc: 0 };
    subs.push(s);
    return () => {
      const i = subs.indexOf(s);
      if (i >= 0) subs.splice(i, 1);
    };
  };

  let last = 0;
  function frame(now) {
    const realDt = last ? Math.min(100, now - last) : 16.7;
    last = now;
    if (!HUD.paused) {
      const simDt = (realDt / 1000) * HUD.speed;
      if (HUD.sim) HUD.sim.step(simDt);
      for (let i = 0; i < subs.length; i++) {
        const s = subs[i];
        s.acc += realDt;
        if (s.acc >= s.interval - 1) {
          const dt = s.acc / 1000;
          s.acc = s.interval ? s.acc % s.interval : 0;
          s.fn(HUD.state, dt);
        }
      }
    }
    requestAnimationFrame(frame);
  }
  HUD.start = function () {
    if (!HUD._started) {
      HUD._started = true;
      requestAnimationFrame(frame);
    }
  };

  /* ---------- mounting ---------- */
  /** Mount overlay `id` into container `el`. Returns the instance. */
  HUD.mount = function (id, el) {
    const def = HUD.overlays[id];
    if (!def) throw new Error("Unknown overlay: " + id);
    el.classList.add("hud", "ov-" + id);
    el.style.width = def.w + "px";
    if (def.h) el.style.height = def.h + "px";
    const inst = def.mount(el, HUD.state) || {};
    if (inst.update) {
      inst.update(HUD.state, 0);
      inst._unsub = HUD.subscribe(inst.update, def.hz || 10);
    }
    if (inst.theme) inst._unTheme = HUD.onTheme(inst.theme);
    return inst;
  };

  /* ---------- small DOM helpers for mount() ---------- */
  /** h("div.hud-row.foo", {style:..., data-x:...}, ...children) */
  HUD.h = function (tag, attrs, ...kids) {
    const parts = tag.split(".");
    const el = document.createElement(parts[0] || "div");
    for (let i = 1; i < parts.length; i++) el.classList.add(parts[i]);
    if (attrs) {
      for (const k in attrs) {
        const v = attrs[k];
        if (k === "text") el.textContent = v;
        else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
        else el.setAttribute(k, v);
      }
    }
    for (const k of kids) if (k != null) el.append(k);
    return el;
  };

  /** Set text only if changed (avoids needless style/layout work). */
  HUD.text = function (el, v) {
    if (el._t !== v) {
      el._t = v;
      el.textContent = v;
    }
  };
  /** Toggle a class only if changed. */
  HUD.cls = function (el, name, on) {
    const k = "_c_" + name;
    if (el[k] !== on) {
      el[k] = on;
      el.classList.toggle(name, on);
    }
  };
  /** Set a data attribute only if changed. */
  HUD.data = function (el, name, v) {
    const k = "_d_" + name;
    if (el[k] !== v) {
      el[k] = v;
      el.dataset[name] = v;
    }
  };
  /** Restart a one-shot CSS animation class. */
  HUD.flash = function (el) {
    el.classList.remove("flash");
    void el.offsetWidth; // rare, only on value change
    el.classList.add("flash");
  };

  /** Size a canvas for crisp rendering at devicePixelRatio. Returns 2d ctx. */
  HUD.canvas = function (canvas, w, h) {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = w + "px";
    canvas.style.height = h + "px";
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return ctx;
  };
})();
