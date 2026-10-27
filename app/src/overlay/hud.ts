/* HUD runtime: overlay registry, single render loop, restyle hook and DOM helpers.
   Port of mockups/shared/hud.js; overlays keep calling HUD.text / HUD.cls / … as before.

   Overlay contract:
     HUD.register(id, { title, group, w, h?, hz?, options?, resize?,
                        mount(root, state, opts) -> { update?, theme?, destroy? } })
   - mount() builds the DOM once and caches node refs. `opts` holds the overlay's options and
     the size it was resized to; when either changes, the overlay is mounted again.
   - update() only writes textContent / transform / classes / data-* on cached nodes.
   - theme() runs after HUD.restyle() (canvas overlays re-read colors there). */

import type { ViewState } from "./view";

export interface OverlayInstance {
  update?(s: ViewState, dt: number): void;
  theme?(): void;
  destroy?(): void;
}

/** A setting the user can change per overlay, shown in the app. Values are stored as text. */
export type OptionDef =
  | { key: string; label: string; type: "color"; default: string }
  | { key: string; label: string; type: "range"; default: number; min: number; max: number; step: number; unit?: string; hint?: string }
  | { key: string; label: string; type: "choice"; default: string; choices: [value: string, label: string][]; hint?: string };

export interface MountOpts {
  /** Every option of the overlay, defaults filled in: colors as `#rrggbb`, ranges as numbers. */
  options: Record<string, string | number>;
  /** Width in px at 1080p: the resized width for overlays that resize by width, else `w`. */
  w: number;
  /** Resized height in px at 1080p for overlays that resize by height; `null` is the usual. */
  h: number | null;
}

/** What the overlay window passes to `HUD.mount` from the overlay's config. */
export interface MountConf {
  options?: Record<string, string>;
  w?: number | null;
  h?: number | null;
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
  options?: OptionDef[];
  /** What resizing changes: the content's width and/or height. Without it, the whole
      overlay scales. */
  resize?: "width" | "height" | "both";
  /** Smallest size it can be resized to, px at 1080p (defaults: `w` / its usual height). */
  minW?: number;
  minH?: number;
  mount(root: HTMLElement, state: ViewState, opts: MountOpts): OverlayInstance | void;
}

/** The overlay's options with defaults for anything missing or malformed. */
export function resolveOptions(def: OverlayDef, stored: Record<string, string> = {}): Record<string, string | number> {
  const out: Record<string, string | number> = {};
  for (const o of def.options ?? []) {
    const v = stored[o.key];
    if (o.type === "color") out[o.key] = v && /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : o.default;
    else if (o.type === "choice") out[o.key] = o.choices.some(([c]) => c === v) ? v : o.default;
    else {
      const n = v === undefined || v === "" ? NaN : Number(v);
      out[o.key] = Number.isFinite(n) ? Math.min(o.max, Math.max(o.min, n)) : o.default;
    }
  }
  return out;
}

/** Rows to add (or remove) to fit a list overlay to the height it was resized to: whole
    rows of `--row-h` against its usual height. */
export function extraRows(h: number | null, usual: number): number {
  if (!h) return 0;
  const row = parseFloat(HUD.css("--row-h")) || 28;
  return Math.round((h - usual) / row);
}

/** Changes when what an overlay is built from changes: then it is mounted again. */
export const mountKey = (c: MountConf) => JSON.stringify([c.options ?? {}, c.w ?? null, c.h ?? null]);

export const resizesWidth = (def: OverlayDef) => def.resize === "width" || def.resize === "both";
export const resizesHeight = (def: OverlayDef) => def.resize === "height" || def.resize === "both";

export interface Mounted extends OverlayInstance {
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
  mount(id: string, el: HTMLElement, state: ViewState, conf: MountConf = {}): Mounted {
    const def = overlays[id];
    if (!def) throw new Error("Unknown overlay: " + id);
    el.classList.add("hud", "ov-" + id);
    const w = resizesWidth(def) && conf.w ? Math.max(def.minW ?? def.w, conf.w) : def.w;
    const h = resizesHeight(def) && conf.h ? Math.max(def.minH ?? 0, conf.h) : null;
    el.style.width = w + "px";
    if (def.h) el.style.height = (h ?? def.h) + "px";
    const inst: Mounted = def.mount(el, state, { options: resolveOptions(def, conf.options), w, h }) || {};
    if (inst.update) {
      inst.update(state, 0);
      inst._unsub = HUD.subscribe(inst.update, def.hz || 10);
    }
    if (inst.theme) inst._unTheme = HUD.onTheme(inst.theme);
    return inst;
  },

  /** Undo mount(): stop updates, let the overlay clean up, empty `el`. */
  unmount(inst: Mounted, el: HTMLElement) {
    inst._unsub?.();
    inst._unTheme?.();
    inst.destroy?.();
    el.replaceChildren();
    el.removeAttribute("style");
    el.className = "";
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
