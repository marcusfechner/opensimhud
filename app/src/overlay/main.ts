/* Overlay window: one transparent, click-through window per display hosting that display's
   overlays from the active profile.

   Query parameters:
     ?display=1         which display this window covers (set by the app; default 0)
     ?profile=Practice  show this profile instead of the active one (development)
     ?only=relative     show a single overlay at the top left (development)
     ?bg=scene          draw a neutral backdrop instead of transparency
     ?skip=10           start the browser demo replay 10 s in */

import "./styles/tokens.css";
import "./styles/base.css";
import "./host.css";
import "./overlays";
import { applyAppearance, applyOverlayLook } from "../shared/appearance";
import * as app from "../shared/bridge";
import { active, SIZES } from "../shared/defaults";
import type { OverlayConf } from "../types/config/OverlayConf";
import { HUD, type Mounted } from "./hud";
import { apply, createView } from "./view";

const q = new URLSearchParams(location.search);
document.body.dataset.bg = q.get("bg") || (app.inTauri ? "transparent" : "scene");
const DISPLAY = +(q.get("display") || 0);
const SNAP = 8;
const REF_W = 1920;

const stage = document.getElementById("stage")!;
const editLayer = document.getElementById("edit")!;
const banner = document.getElementById("banner")!;
const status = document.getElementById("status")!;
const view = createView();

let config: app.Config | null = null;
let ui: app.Ui = { hidden: false, editing: false };
let displayCount = 1;
let live = false;

interface Item {
  id: string;
  conf: OverlayConf;
  host: HTMLElement;
  frame: HTMLElement;
  inst: Mounted | null;
}
const items = new Map<string, Item>();

/* ---------- geometry ---------- */

/** Reference width of this window: 1080 high, as wide as the display's aspect ratio. */
const stageW = () => innerWidth / (innerHeight / 1080);

/** Overlays keep to their third of the screen: left stays left, right stays at the right
    edge, the middle stays centered, on any aspect ratio. */
function shift(x: number, w: number) {
  const cx = x + w / 2, extra = stageW() - REF_W;
  return cx < REF_W / 3 ? 0 : cx > (REF_W * 2) / 3 ? extra : extra / 2;
}

/** Size of an overlay on screen (reference px), measured once it has rendered. */
function size(it: Item) {
  const k = it.conf.scale / 100;
  const w = it.inst ? it.host.offsetWidth : SIZES[it.id]?.w ?? 300;
  const h = it.inst ? it.host.offsetHeight : SIZES[it.id]?.h ?? 100;
  return { w: w * k, h: h * k };
}

function place(it: Item) {
  const { w, h } = size(it);
  const left = it.conf.x + shift(it.conf.x, w);
  for (const el of [it.host, it.frame]) {
    el.style.left = left + "px";
    el.style.top = it.conf.y + "px";
  }
  it.host.style.transform = it.conf.scale === 100 ? "" : `scale(${it.conf.scale / 100})`;
  it.frame.style.width = w + "px";
  it.frame.style.height = h + "px";
}

function fit() {
  const k = innerHeight / 1080;
  for (const layer of [stage, editLayer]) {
    layer.style.width = stageW() + "px";
    layer.style.transform = `scale(${k})`;
  }
  items.forEach(place);
}

/* ---------- which overlays, mounted where ---------- */

function wanted(): Map<string, OverlayConf> {
  const out = new Map<string, OverlayConf>();
  const only = q.get("only");
  if (only) {
    out.set(only, { on: true, x: 24, y: 24, display: 0, opacity: 100, scale: 100, font: 100 });
    return out;
  }
  if (!config) return out;
  const profile = config.profiles.find((p) => p.name === q.get("profile")) ?? active(config);
  for (const [id, o] of Object.entries(profile.overlays)) {
    if (o.on && HUD.overlays[id] && Math.min(o.display, displayCount - 1) === DISPLAY) out.set(id, o);
  }
  return out;
}

function mountItem(it: Item) {
  if (it.inst || !live) return;
  try {
    it.inst = HUD.mount(it.id, it.host, view);
    it.host.classList.add("ov-host");
  } catch (e) {
    console.error("could not mount", it.id, e);
  }
}

/** Bring the stage in line with the config: add, move, restyle and remove overlays. */
function reconcile() {
  const want = wanted();
  for (const [id, it] of items) {
    if (!want.has(id)) {
      if (it.inst) HUD.unmount(it.inst, it.host);
      it.host.remove();
      it.frame.remove();
      items.delete(id);
    }
  }
  for (const [id, conf] of want) {
    let it = items.get(id);
    if (!it) {
      const frame = HUD.h("div.ov-frame", { "data-id": id }, HUD.h("span.ov-label", { text: HUD.overlays[id].title }));
      frame.addEventListener("pointerdown", (e) => drag(e, id));
      it = { id, conf, host: HUD.h("div.ov-host"), frame, inst: null };
      items.set(id, it);
      stage.append(it.host);
      editLayer.append(frame);
    }
    it.conf = conf;
    mountItem(it);
    applyOverlayLook(it.host, conf);
    place(it);
  }
  // sizes settle after the first paint (fonts, rows filling in)
  requestAnimationFrame(() => items.forEach(place));
  render();
}

/** Show what fits the moment: overlays while the game runs, frames while editing. */
function render() {
  stage.hidden = ui.hidden || !live;
  document.body.classList.toggle("editing", ui.editing);
  editLayer.hidden = !ui.editing;
  banner.hidden = !ui.editing;
  for (const it of items.values()) it.frame.classList.toggle("empty", !it.inst || !live);
  if (ui.editing) requestAnimationFrame(placeBanner);
}

/** Put the edit banner where it covers the fewest overlays: bottom, top or middle. */
function placeBanner() {
  const frames = [...items.values()].map((it) => it.frame.getBoundingClientRect());
  let best = "", least = Infinity;
  for (const spot of ["bottom", "top", "middle"]) {
    banner.dataset.spot = spot;
    const b = banner.getBoundingClientRect();
    let area = 0;
    for (const f of frames) {
      const w = Math.min(b.right, f.right) - Math.max(b.left, f.left);
      const h = Math.min(b.bottom, f.bottom) - Math.max(b.top, f.top);
      if (w > 0 && h > 0) area += w * h;
    }
    if (area < least) [best, least] = [spot, area];
  }
  banner.dataset.spot = best;
}

/* ---------- edit mode: drag with snapping ---------- */

const guideV = HUD.h("div.guide.v"), guideH = HUD.h("div.guide.h");
editLayer.append(guideV, guideH);

function drag(e: PointerEvent, id: string) {
  const it = items.get(id);
  if (!it || !config) return;
  e.preventDefault();
  const frame = it.frame;
  frame.setPointerCapture(e.pointerId);
  frame.classList.add("dragging");
  const k = innerHeight / 1080;
  const { w, h } = size(it);
  const startX = e.clientX, startY = e.clientY;
  // work in on-screen reference px (shift included), convert back on drop
  const ox = it.conf.x + shift(it.conf.x, w), oy = it.conf.y;
  const W = stageW();
  const others = [...items.values()].filter((o) => o !== it).map((o) => {
    const s = size(o);
    return { x: o.conf.x + shift(o.conf.x, s.w), y: o.conf.y, w: s.w, h: s.h };
  });
  let x = ox, y = oy;

  const move = (ev: PointerEvent) => {
    x = ox + (ev.clientX - startX) / k;
    y = oy + (ev.clientY - startY) / k;
    let gx: number | null = null, gy: number | null = null;
    if (!ev.altKey) {
      x = Math.round(x / SNAP) * SNAP;
      y = Math.round(y / SNAP) * SNAP;
      const T = 12;
      if (Math.abs(x + w / 2 - W / 2) < T) { x = W / 2 - w / 2; gx = W / 2; }
      if (Math.abs(y + h / 2 - 540) < T) { y = 540 - h / 2; gy = 540; }
      for (const t of others) {
        if (gx === null) {
          for (const [a, b] of [[x, t.x], [x, t.x + t.w], [x + w, t.x], [x + w, t.x + t.w]]) {
            if (Math.abs(a - b) < T) { x += b - a; gx = b; break; }
          }
        }
        if (gy === null) {
          for (const [a, b] of [[y, t.y], [y, t.y + t.h], [y + h, t.y], [y + h, t.y + t.h]]) {
            if (Math.abs(a - b) < T) { y += b - a; gy = b; break; }
          }
        }
      }
    }
    x = Math.max(0, Math.min(W - w, x));
    y = Math.max(0, Math.min(1080 - h, y));
    for (const el of [it.host, frame]) {
      el.style.left = x + "px";
      el.style.top = y + "px";
    }
    guideV.classList.toggle("show", gx !== null);
    guideH.classList.toggle("show", gy !== null);
    if (gx !== null) guideV.style.left = gx + "px";
    if (gy !== null) guideH.style.top = gy + "px";
  };
  const up = () => {
    frame.classList.remove("dragging");
    guideV.classList.remove("show");
    guideH.classList.remove("show");
    frame.removeEventListener("pointermove", move);
    frame.removeEventListener("pointerup", up);
    frame.removeEventListener("pointercancel", up);
    // back to reference coordinates: the shift of the third it was dropped in
    let rx = x;
    for (let i = 0; i < 2; i++) rx = x - shift(rx, w);
    void save(id, Math.round(rx), Math.round(y));
  };
  frame.addEventListener("pointermove", move);
  frame.addEventListener("pointerup", up);
  frame.addEventListener("pointercancel", up);
}

async function save(id: string, x: number, y: number) {
  if (!config) return;
  const next: app.Config = structuredClone(config);
  const profile = next.profiles.find((p) => p.name === (q.get("profile") ?? next.profile)) ?? active(next);
  const o = profile.overlays[id];
  if (!o || (o.x === x && o.y === y)) return;
  o.x = x;
  o.y = y;
  config = await app.setConfig(next);
  reconcile();
}

addEventListener("keydown", (e) => {
  if (e.key === "Escape" && ui.editing) void app.setEditing(false);
});
document.getElementById("done")!.addEventListener("click", () => void app.setEditing(false));

/* ---------- boot ---------- */

/** WKWebView can run this before the stylesheets apply; canvas overlays need the tokens. */
async function stylesReady() {
  for (let i = 0; i < 120 && !getComputedStyle(document.documentElement).getPropertyValue("--accent").trim(); i++) {
    await new Promise((r) => requestAnimationFrame(r));
  }
}

async function boot() {
  await stylesReady();
  [config, ui, displayCount] = await Promise.all([app.getConfig(), app.getUi(), app.getDisplays().then((d) => d.length || 1)]);
  applyAppearance(config.appearance);
  fit();
  reconcile();
  HUD.start(() => view);

  app.onConfig((c) => {
    const restyle = JSON.stringify(c.appearance) !== JSON.stringify(config?.appearance);
    config = c;
    if (restyle) {
      applyAppearance(c.appearance);
      HUD.restyle();
    }
    reconcile();
  });
  app.onUi((u) => {
    ui = u;
    render();
  });

  // only in a browser: in the app this would sit on the desktop while the game is closed
  if (!app.inTauri) status.textContent = "Waiting for telemetry…";
  await app.telemetry((frame) => {
    apply(view, frame);
    const now = view.ready && view.connection === "connected";
    if (now !== live) {
      live = now;
      status.textContent = "";
      reconcile();
    }
  });
}

addEventListener("resize", fit);
boot().catch((e) => (status.textContent = String(e?.message || e)));
