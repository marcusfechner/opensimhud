/* HUD runtime: overlay registry, single render loop, restyle hook and DOM helpers.
   Port of mockups/shared/hud.js; overlays keep calling HUD.text / HUD.cls / … as before.

   Overlay contract:
     HUD.register(id, { title, group, w, h?, hz?, mount(root, state) -> { update?, theme?, destroy? } })
   - mount() builds the DOM once and caches node refs.
   - update() only writes textContent / transform / classes / data-* on cached nodes.
   - theme() runs after HUD.restyle() (canvas overlays re-read colors there). */

import type { ViewState } from "./view";

export interface OverlayInstance {
  update?(s: ViewState, dt: number): void;
  theme?(): void;
  destroy?(): void;
}

export interface OverlayDef {
  id?: string;
  title: string;
  group: string;
  /** Default size in px at 1080p. */
  w: number;
  h?: number;
  /** Update rate; the render loop calls update at most this often. */
  hz?: number;
  mount(root: HTMLElement, state: ViewState): OverlayInstance | void;
}

interface Mounted extends OverlayInstance {
  _unsub?: () => void;
  _unTheme?: () => void;
}

type Attrs = Record<string, string | number | Partial<CSSStyleDeclaration>>;
// per-element caches so unchanged values never touch the DOM
type Cached = HTMLElement & Record<string, unknown>;

const overlays: Record<string, OverlayDef> = {};
const themeSubs = new Set<() => void>();
const cssCache = new Map<string, string>();
const subs: { fn: (s: ViewState, dt: number) => void; interval: number; acc: number }[] = [];
let started = false;
let last = 0;
let getState: () => ViewState;

export const HUD = {
  overlays,
  paused: false,

  register(id: string, def: OverlayDef) {
    def.id = id;
    overlays[id] = def;
  },

  /* ---------- restyle ---------- */
  /** Call after design tokens change at runtime so canvas overlays re-read colors. */
  restyle() {
    cssCache.clear();
    requestAnimationFrame(() => themeSubs.forEach((fn) => fn()));
  },
  onTheme(fn: () => void) {
    themeSubs.add(fn);
    return () => void themeSubs.delete(fn);
  },
  /** A CSS custom property, cached until the next restyle. Use in canvas overlays. */
  css(name: string, el?: Element): string {
    let v = cssCache.get(name);
    if (v === undefined) {
      v = getComputedStyle(el || document.documentElement).getPropertyValue(name).trim();
      // never cache "": the stylesheet may simply not be applied yet
      if (v) cssCache.set(name, v);
    }
    return v;
  },

  /** Color of a car class (`--c-gt3`, `--c-gt4`, …); unknown classes get the dim text color. */
  classColor(cls: string): string {
    return (cls && HUD.css("--c-" + cls.toLowerCase())) || HUD.css("--text-dim");
  },

  /* ---------- render loop ---------- */
  subscribe(fn: (s: ViewState, dt: number) => void, hz?: number) {
    const s = { fn, interval: hz ? 1000 / hz : 0, acc: 0 };
    subs.push(s);
    return () => {
      const i = subs.indexOf(s);
      if (i >= 0) subs.splice(i, 1);
    };
  },
  start(state: () => ViewState) {
    getState = state;
    if (started) return;
    started = true;
    requestAnimationFrame(frame);
  },

  /** Mount overlay `id` into `el`. */
  mount(id: string, el: HTMLElement, state: ViewState): Mounted {
    const def = overlays[id];
    if (!def) throw new Error("Unknown overlay: " + id);
    el.classList.add("hud", "ov-" + id);
    el.style.width = def.w + "px";
    if (def.h) el.style.height = def.h + "px";
    const inst: Mounted = def.mount(el, state) || {};
    if (inst.update) {
      inst.update(state, 0);
      inst._unsub = HUD.subscribe(inst.update, def.hz || 10);
    }
    if (inst.theme) inst._unTheme = HUD.onTheme(inst.theme);
    return inst;
  },

  /* ---------- DOM helpers for mount() ---------- */
  /** h("div.hud-row.foo", { text, style, "data-x": … }, ...children) */
  h(tag: string, attrs?: Attrs | null, ...kids: (Node | string | null | undefined)[]): HTMLElement {
    const parts = tag.split(".");
    const el = document.createElement(parts[0] || "div");
    for (let i = 1; i < parts.length; i++) el.classList.add(parts[i]);
    if (attrs) {
      for (const k in attrs) {
        const v = attrs[k];
        if (k === "text") el.textContent = String(v);
        else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
        else el.setAttribute(k, String(v));
      }
    }
    for (const k of kids) if (k != null) el.append(k);
    return el;
  },
  /** Set text only if changed. */
  text(el: HTMLElement, v: string) {
    const c = el as Cached;
    if (c._t !== v) {
      c._t = v;
      el.textContent = v;
    }
  },
  /** Toggle a class only if changed. */
  cls(el: Element, name: string, on: boolean) {
    const c = el as Cached, k = "_c_" + name;
    if (c[k] !== on) {
      c[k] = on;
      el.classList.toggle(name, on);
    }
  },
  /** Set a data attribute only if changed. */
  data(el: HTMLElement, name: string, v: string) {
    const c = el as Cached, k = "_d_" + name;
    if (c[k] !== v) {
      c[k] = v;
      el.dataset[name] = v;
    }
  },
  /** Restart a one-shot CSS animation class. */
  flash(el: HTMLElement) {
    el.classList.remove("flash");
    void el.offsetWidth; // rare, only on value change
    el.classList.add("flash");
  },
  /** Size a canvas for crisp rendering at devicePixelRatio. Returns its 2d context. */
  canvas(canvas: HTMLCanvasElement, w: number, h: number): CanvasRenderingContext2D {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = w + "px";
    canvas.style.height = h + "px";
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return ctx;
  },
};

function frame(now: number) {
  const realDt = last ? Math.min(100, now - last) : 16.7;
  last = now;
  if (!HUD.paused) {
    const state = getState();
    for (let i = 0; i < subs.length; i++) {
      const s = subs[i];
      s.acc += realDt;
      if (s.acc >= s.interval - 1) {
        const dt = s.acc / 1000;
        s.acc = s.interval ? s.acc % s.interval : 0;
        s.fn(state, dt);
      }
    }
  }
  requestAnimationFrame(frame);
}
