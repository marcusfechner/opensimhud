/* Settings window: overlays, layout, appearance, profiles and app settings, all editing the
   one config the app keeps (see src-tauri/src/config.rs). Changes apply in game at once.
   Ported from mockups/app.js. */

import "../overlay/styles/tokens.css";
import "../overlay/styles/base.css";
import "./app.css";
import "../overlay/overlays";
import { HUD, mountKey, resolveOptions, type Mounted } from "../overlay/hud";
import { apply, createView } from "../overlay/view";
import { applyAppearance, applyOverlayLook } from "../shared/appearance";
import * as app from "../shared/bridge";
import { active, BUILT_IN, defaultProfile, SIZES } from "../shared/defaults";
import type { OverlayConf } from "../types/config/OverlayConf";
import type { Profile } from "../types/config/Profile";
import { byId, CATALOG, icon } from "../shared/catalog";
import { fromScreen, REF_H, REF_W, screenX } from "../shared/placement";

const $ = <T extends HTMLElement = HTMLElement>(s: string, el: ParentNode = document) => el.querySelector(s) as T;
const $$ = <T extends HTMLElement = HTMLElement>(s: string, el: ParentNode = document) => Array.from(el.querySelectorAll(s)) as T[];
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

let cfg!: app.Config;
let ui: app.Ui = { hidden: false, editing: false };
let status: app.Status | null = null;
let displays: app.Display[] = [];

/* ---------- config: edit locally, save shortly after ---------- */

let saving: ReturnType<typeof setTimeout> | null = null;
let inFlight = 0;

/** Change the config: `fn` edits a copy; the page re-renders, the app saves soon after. */
function commit(fn: (c: app.Config) => void, rerender = true) {
  const next = structuredClone(cfg);
  fn(next);
  cfg = next;
  if (rerender) renderAll();
  if (saving) clearTimeout(saving);
  saving = setTimeout(save, 120);
}

async function save() {
  saving = null;
  inFlight++;
  try {
    const stored = await app.setConfig(cfg);
    // the app may have repaired something (e.g. a duplicate profile name)
    if (!saving && inFlight === 1 && JSON.stringify(stored) !== JSON.stringify(cfg)) {
      cfg = stored;
      renderAll();
    }
  } finally {
    inFlight--;
  }
}

/** Changes made elsewhere (dragging in game, automatic profile switch). */
function external(c: app.Config) {
  if (saving || inFlight) return; // our own save is on its way; its result wins
  if (JSON.stringify(c) === JSON.stringify(cfg)) return;
  cfg = c;
  renderAll();
}

const profile = (): Profile => active(cfg);
const conf = (id: string): OverlayConf => profile().overlays[id];

/* ---------- small controls ---------- */

function rangeFill(r: HTMLInputElement) {
  r.style.setProperty("--fill", ((+r.value - +r.min) / (+r.max - +r.min)) * 100 + "%");
}
function setSwitch(b: HTMLElement, on: boolean) {
  b.setAttribute("aria-checked", String(on));
}
function onSwitch(b: HTMLElement, fn: (on: boolean) => void) {
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    const on = b.getAttribute("aria-checked") !== "true";
    setSwitch(b, on);
    fn(on);
  });
}

/* ---------- previews: the demo race, while this window is visible ---------- */

const pview = createView();
let stopPreview: (() => void) | null = null;
const previewReady: (() => void)[] = [];

function startPreview() {
  if (stopPreview) return;
  stopPreview = app.preview((f) => {
    apply(pview, f);
    if (pview.ready) previewReady.splice(0).forEach((fn) => fn());
  });
}
function whenPreviewReady(fn: () => void) {
  if (pview.ready) fn();
  else previewReady.push(fn);
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    stopPreview?.();
    stopPreview = null;
  } else startPreview();
});

function mountPreview(stage: HTMLElement, id: string): { el: HTMLElement; inst: Mounted } | null {
  if (!HUD.overlays[id]) return null;
  const el = document.createElement("div");
  stage.append(el);
  try {
    return { el, inst: HUD.mount(id, el, pview, conf(id)) };
  } catch (e) {
    console.warn("preview failed", id, e);
    el.remove();
    return null;
  }
}

/* ---------- sidebar ---------- */

const GAMES: [string, string, boolean][] = [
  ["ACC", "Assetto Corsa Competizione", true],
  ["iR", "iRacing", false],
  ["LMU", "Le Mans Ultimate", false],
  ["EVO", "Assetto Corsa EVO", false],
  ["rF2", "rFactor 2", false],
];
$("#gameMenu").innerHTML = GAMES.map(
  ([k, n, on]) => `<div class="game-opt${on ? "" : " disabled"}" role="option" aria-selected="${on}" aria-disabled="${!on}">
    <span class="game-logo">${k}</span><span>${n}</span>
    ${on ? '<svg class="check" viewBox="0 0 14 14"><path d="M3 7.5l2.5 2.5L11 4.5" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>' : '<span class="soon">Coming soon</span>'}
  </div>`,
).join("");
$("#gameBtn").addEventListener("click", (e) => {
  e.stopPropagation();
  const open = $("#game").classList.toggle("open");
  $("#gameBtn").setAttribute("aria-expanded", String(open));
});
document.addEventListener("click", () => $("#game").classList.remove("open"));

const SESSION: Record<string, string> = { practice: "practice", qualifying: "qualifying", race: "race", hotlap: "hotlap" };

function renderStatus() {
  const s = status;
  const connected = s?.connection === "connected";
  const demo = s ? s.game === "demo" : cfg.demo;
  $("#game").dataset.status = connected ? "connected" : "waiting";
  $("#gameLogo").textContent = demo ? "DEMO" : "ACC";
  $("#gameLogo").classList.toggle("small", demo);
  $("#gameName").textContent = demo ? "Demo race" : "Assetto Corsa Competizione";
  $("#gameStatus").textContent = demo
    ? "Overlays show a sample race"
    : connected
      ? [s!.track, s!.session && SESSION[s!.session]].filter(Boolean).join(", ") || "Connected"
      : "Waiting for the game";
}

function renderSidebar() {
  const sel = $<HTMLSelectElement>("#profileSel");
  sel.innerHTML = cfg.profiles.map((p) => `<option${p.name === cfg.profile ? " selected" : ""}>${esc(p.name)}</option>`).join("");
  const n = Object.values(profile().overlays).filter((o) => o.on).length;
  $("#navCount").textContent = String(n);
  renderStatus();
}
$<HTMLSelectElement>("#profileSel").addEventListener("change", (e) => {
  const name = (e.target as HTMLSelectElement).value;
  commit((c) => (c.profile = name));
});

/* ---------- navigation ---------- */

let page = "overlays";
const pageRender: Record<string, () => void> = {};
function go(p: string) {
  page = p;
  $$(".page").forEach((el) => el.classList.toggle("active", el.dataset.page === p));
  $$("#nav button").forEach((b) => (b.dataset.page === p ? b.setAttribute("aria-current", "page") : b.removeAttribute("aria-current")));
  pageRender[p]?.();
}
$$("#nav button").forEach((b) => b.addEventListener("click", () => go(b.dataset.page!)));

function renderAll() {
  applyAppearance(cfg.appearance);
  HUD.restyle();
  renderSidebar();
  pageRender[page]?.();
}

/* ---------- overlays page ---------- */

let selected = "relative";
let preview: { el: HTMLElement; inst: Mounted } | null = null;
let previewFor = "";
let previewBuilt = "";

function renderOverlays() {
  const p = profile();
  const n = Object.values(p.overlays).filter((o) => o.on).length;
  $("#ovSummary").textContent = `${n} of ${CATALOG.length} shown in ${p.name}`;
  $("#hideAll span").textContent = ui.hidden ? "Show overlays" : "Hide overlays";

  const groups = [...new Set(CATALOG.map((o) => o.group))];
  $("#ovList").innerHTML = groups
    .map(
      (g) => `<div class="ov-group"><h2>${g}</h2>${CATALOG.filter((o) => o.group === g)
        .map((o) => {
          const on = p.overlays[o.id]?.on;
          return `<div class="ov-item${on ? " on" : ""}${o.id === selected ? " selected" : ""}" data-id="${o.id}" tabindex="0">
            <span class="ov-icon">${icon(o.id)}</span>
            <span class="txt"><div class="t">${o.title}</div><div class="d">${o.desc}</div></span>
            <span></span>
            <button class="switch" role="switch" aria-checked="${!!on}" aria-label="Show ${o.title}"></button>
          </div>`;
        })
        .join("")}</div>`,
    )
    .join("");
  $$("#ovList .ov-item").forEach((item) => {
    const id = item.dataset.id!;
    onSwitch($(".switch", item), (on) => commit((c) => (active(c).overlays[id].on = on)));
    item.addEventListener("click", () => select(id));
    item.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        select(id);
      }
    });
  });
  renderDetail();
}

function select(id: string) {
  selected = id;
  $$("#ovList .ov-item").forEach((i) => i.classList.toggle("selected", i.dataset.id === id));
  renderDetail();
}

const sliders: [string, string, keyof OverlayConf][] = [
  ["#sOpacity", "#vOpacity", "opacity"],
  ["#sScale", "#vScale", "scale"],
  ["#sFont", "#vFont", "font"],
];

function renderDetail() {
  const o = conf(selected);
  $("#dTitle").textContent = byId[selected].title;
  $("#dState").textContent = o.on ? "Shown in game" : "Off";
  $("#dProfile").textContent = profile().name;
  setSwitch($("#dToggle"), o.on);
  for (const [s, v, key] of sliders) {
    const r = $<HTMLInputElement>(s);
    r.value = String(o[key]);
    rangeFill(r);
    $(v).textContent = o[key] + "%";
  }
  renderOptions();
  refreshPreview();
}

/** The overlay's own options, and the size it was resized to in game. */
function renderOptions() {
  const def = HUD.overlays[selected];
  const o = conf(selected);
  const defs = def?.options ?? [];
  const sized = !!def?.resize && (o.w != null || o.h != null);
  $("#optTitle").hidden = $("#optRows").hidden = !defs.length && !sized;
  if (!def) return;
  const vals = resolveOptions(def, o.options);
  const reset = (key: string, show: boolean) =>
    `<button class="btn small" data-reset="${key}"${show ? "" : " hidden"}>Reset</button>`;
  const rows = defs.map((d) => {
    const v = vals[d.key];
    if (d.type === "choice") {
      const seg = d.choices
        .map(([c, label]) => `<button data-choice="${d.key}" data-value="${c}" aria-pressed="${c === v}">${esc(label)}</button>`)
        .join("");
      return `<div class="set-row"><span class="lbl">${esc(d.label)}${d.hint ? `<small>${esc(d.hint)}</small>` : ""}</span>
        <span><span class="seg">${seg}</span></span><span></span></div>`;
    }
    if (d.type === "color") {
      return `<div class="set-row"><span class="lbl">${esc(d.label)}</span>
        <label class="color-pick"><input type="color" data-key="${d.key}" value="${v}" /><span class="num">${v}</span></label>
        ${reset(d.key, v !== d.default)}</div>`;
    }
    return `<div class="set-row"><span class="lbl">${esc(d.label)}${d.hint ? `<small>${esc(d.hint)}</small>` : ""}</span>
      <input type="range" data-key="${d.key}" min="${d.min}" max="${d.max}" step="${d.step}" value="${v}" />
      <span class="val">${v}${d.unit ?? ""}</span></div>`;
  });
  if (sized) {
    const size = [o.w ?? "usual", o.h ?? "usual"].join(" × ");
    rows.push(`<div class="set-row"><span class="lbl">Resized in game<small>Width × height, px at 1080p</small></span>
      <span class="val-l num">${size}</span>${reset("@size", true)}</div>`);
  }
  $("#optRows").innerHTML = rows.join("");
  $$<HTMLInputElement>('#optRows input[type="range"]').forEach(rangeFill);
}
$("#optRows").addEventListener("input", (e) => {
  const input = e.target as HTMLInputElement;
  const key = input.dataset.key;
  if (!key) return;
  const def = HUD.overlays[selected]?.options?.find((d) => d.key === key);
  if (input.type === "range") {
    rangeFill(input);
    input.nextElementSibling!.textContent = input.value + (def?.type === "range" ? def.unit ?? "" : "");
  } else {
    input.nextElementSibling!.textContent = input.value;
    const r = input.closest(".set-row")!.querySelector<HTMLElement>("[data-reset]");
    if (r) r.hidden = input.value === def?.default;
  }
  commit((c) => (active(c).overlays[selected].options[key] = input.value), false);
  refreshPreview();
});
$("#optRows").addEventListener("click", (e) => {
  const choice = (e.target as HTMLElement).closest<HTMLElement>("[data-choice]");
  if (choice) {
    const { choice: k, value } = choice.dataset;
    commit((c) => (active(c).overlays[selected].options[k!] = value!));
    return;
  }
  const key = (e.target as HTMLElement).closest<HTMLElement>("[data-reset]")?.dataset.reset;
  if (!key) return;
  commit((c) => {
    const o = active(c).overlays[selected];
    if (key === "@size") o.w = o.h = null;
    else delete o.options[key];
  });
});

/** Mount the selected overlay's preview again when it or its options changed. */
function refreshPreview() {
  const built = mountKey(conf(selected));
  if (previewFor !== selected || previewBuilt !== built) {
    const same = previewFor === selected && preview;
    previewFor = selected;
    previewBuilt = built;
    if (preview) HUD.unmount(preview.inst, preview.el);
    preview?.el.remove();
    preview = null;
    const stage = $("#previewStage");
    if (!same) stage.innerHTML = '<span class="preview-missing">Starting the demo race…</span>';
    const id = selected;
    whenPreviewReady(() => {
      if (previewFor !== id) return;
      stage.innerHTML = "";
      preview = mountPreview(stage, id);
      if (!preview) stage.innerHTML = '<span class="preview-missing">No preview for this overlay</span>';
      else {
        previewRO.observe(preview.el);
        lookPreview();
      }
    });
  }
  lookPreview();
}

function lookPreview() {
  if (!preview) return;
  applyOverlayLook(preview.el, conf(selected));
  fitPreview();
}

function fitPreview() {
  if (!preview) return;
  const stage = $("#previewStage");
  const box = $("#preview").getBoundingClientRect();
  const el = preview.el;
  const w = Math.max(el.offsetWidth, el.scrollWidth), h = Math.max(el.offsetHeight, el.scrollHeight);
  const want = conf(selected).scale / 100;
  const k = Math.min(want, (box.width - 40) / w, (box.height - 44) / h);
  stage.style.transform = `translate(-50%, -50%) scale(${k})`;
}
const previewRO = new ResizeObserver(() => fitPreview());

onSwitch($("#dToggle"), (on) => commit((c) => (active(c).overlays[selected].on = on)));
for (const [s, v, key] of sliders) {
  $<HTMLInputElement>(s).addEventListener("input", (e) => {
    const r = e.target as HTMLInputElement;
    rangeFill(r);
    $(v).textContent = r.value + "%";
    commit((c) => ((active(c).overlays[selected][key] as number) = +r.value), false);
    lookPreview();
  });
}
$("#hideAll").addEventListener("click", () => void app.setHidden(!ui.hidden));

/* ---------- layout page ---------- */

const SNAP = 8;
let scaleK = 1;
/** Width of the shown display in reference px (1080 high). */
let screenW = REF_W;
let selBox: string | null = null;
let shownDisplay = 0;

function boxSize(id: string) {
  const o = conf(id), s = SIZES[id];
  return { w: Math.round((s.w * o.scale) / 100), h: Math.round((s.h * o.scale) / 100) };
}

function renderLayout() {
  const screen = $("#screen");
  $$(".lbox", screen).forEach((b) => b.remove());
  const d = displays[shownDisplay];
  screenW = d && d.height > 0 ? (REF_H * d.width) / d.height : REF_W;
  screen.style.aspectRatio = `${screenW} / ${REF_H}`;
  scaleK = screen.clientWidth / screenW || 1;
  screen.style.setProperty("--grid", SNAP * 2 * scaleK + "px");
  const p = profile();
  const last = Math.max(0, displays.length - 1);
  const onHere = (o: OverlayConf) => o.on && Math.min(o.display, last) === shownDisplay;
  for (const o of CATALOG) {
    const c = p.overlays[o.id];
    if (!onHere(c)) continue;
    const b = document.createElement("div");
    b.className = "lbox";
    b.dataset.id = o.id;
    const { w, h } = boxSize(o.id);
    b.innerHTML = icon(o.id) + `<span>${o.title}</span><span class="size">${w}×${h}</span>`;
    placeBox(b, screenX(c, w, screenW), c.y, w, h);
    screen.append(b);
    b.addEventListener("pointerdown", (e) => startDrag(e, b, o.id));
  }
  selectBox(selBox && onHere(p.overlays[selBox]) ? selBox : null);

  const off = CATALOG.filter((o) => !p.overlays[o.id].on);
  $("#hiddenList").innerHTML = off.length
    ? off.map((o) => `<div class="hidden-item">${icon(o.id)}<span>${o.title}</span><span class="spacer"></span><button class="btn" data-id="${o.id}">Show</button></div>`).join("")
    : '<span class="hint">Every overlay is on.</span>';

  $("#displayInfo").textContent = d ? `${d.name}, ${d.width} × ${d.height}` : "";
  $("#displayCard").hidden = displays.length < 2;
  $("#displaySel").innerHTML = displays.map((x) => `<option value="${x.index}"${x.index === shownDisplay ? " selected" : ""}>${esc(displayName(x))}</option>`).join("");
}

const displayName = (d: app.Display) => `Display ${d.index + 1}${d.primary ? " (main)" : ""}, ${d.width} × ${d.height}`;

function placeBox(b: HTMLElement, x: number, y: number, w: number, h: number) {
  b.style.left = x * scaleK + "px";
  b.style.top = y * scaleK + "px";
  b.style.width = w * scaleK + "px";
  b.style.height = h * scaleK + "px";
}

function selectBox(id: string | null) {
  selBox = id;
  $$(".lbox").forEach((b) => b.classList.toggle("selected", b.dataset.id === id));
  const o = id ? conf(id) : null;
  const size = id ? boxSize(id) : null;
  $("#selName").textContent = id ? byId[id].title : "Select an overlay";
  $("#selX").textContent = o && size ? String(Math.round(screenX(o, size.w, screenW))) : "–";
  $("#selY").textContent = o ? String(o.y) : "–";
  $("#selW").textContent = size ? String(size.w) : "–";
  $("#selH").textContent = size ? String(size.h) : "–";
  $("#moveDisplay").hidden = !id || displays.length < 2;
  if (id && o) {
    $("#selDisplay").innerHTML = displays.map((x) => `<option value="${x.index}"${x.index === Math.min(o.display, displays.length - 1) ? " selected" : ""}>${esc(displayName(x))}</option>`).join("");
  }
}

function startDrag(e: PointerEvent, b: HTMLElement, id: string) {
  e.preventDefault();
  b.setPointerCapture(e.pointerId);
  selectBox(id);
  b.classList.add("dragging");
  const o = conf(id);
  const { w, h } = boxSize(id);
  const W = screenW;
  const sx = e.clientX, sy = e.clientY, ox = screenX(o, w, W), oy = o.y;
  const snap = $<HTMLInputElement>("#snapChk").checked;
  const others = CATALOG.filter((x) => x.id !== id && conf(x.id).on).map((x) => {
    const s = boxSize(x.id), c = conf(x.id);
    return { x: screenX(c, s.w, W), y: c.y, ...s };
  });
  const gV = $("#gV"), gH = $("#gH");
  let x = ox, y = oy;
  const move = (ev: PointerEvent) => {
    x = ox + (ev.clientX - sx) / scaleK;
    y = oy + (ev.clientY - sy) / scaleK;
    let gx: number | null = null, gy: number | null = null;
    if (snap) {
      x = Math.round(x / SNAP) * SNAP;
      y = Math.round(y / SNAP) * SNAP;
      const T = 12;
      if (Math.abs(x + w / 2 - W / 2) < T) { x = W / 2 - w / 2; gx = W / 2; }
      if (Math.abs(y + h / 2 - 540) < T) { y = 540 - h / 2; gy = 540; }
      for (const t of others) {
        if (gx === null) {
          for (const [a, bx] of [[x, t.x], [x, t.x + t.w], [x + w, t.x], [x + w, t.x + t.w]]) {
            if (Math.abs(a - bx) < T) { x += bx - a; gx = bx; break; }
          }
        }
        if (gy === null) {
          for (const [a, by] of [[y, t.y], [y, t.y + t.h], [y + h, t.y], [y + h, t.y + t.h]]) {
            if (Math.abs(a - by) < T) { y += by - a; gy = by; break; }
          }
        }
      }
    }
    x = Math.round(Math.max(0, Math.min(W - w, x)));
    y = Math.round(Math.max(0, Math.min(1080 - h, y)));
    placeBox(b, x, y, w, h);
    gV.classList.toggle("show", gx !== null);
    gH.classList.toggle("show", gy !== null);
    if (gx !== null) gV.style.left = gx * scaleK + "px";
    if (gy !== null) gH.style.top = gy * scaleK + "px";
    $("#selX").textContent = String(x);
    $("#selY").textContent = String(y);
  };
  const up = () => {
    b.classList.remove("dragging");
    gV.classList.remove("show");
    gH.classList.remove("show");
    b.removeEventListener("pointermove", move);
    b.removeEventListener("pointerup", up);
    b.removeEventListener("pointercancel", up);
    if (x !== ox || y !== oy) {
      // stored from the side of the screen it was dropped on
      commit((c) => Object.assign(active(c).overlays[id], { ...fromScreen(x, w, W), y }), false);
    }
  };
  b.addEventListener("pointermove", move);
  b.addEventListener("pointerup", up);
  b.addEventListener("pointercancel", up);
}

$("#screen").addEventListener("pointerdown", (e) => e.target === e.currentTarget && selectBox(null));
$("#hiddenList").addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLElement>("button[data-id]");
  if (!b) return;
  const id = b.dataset.id!;
  selBox = id;
  commit((c) => Object.assign(active(c).overlays[id], { on: true, display: shownDisplay }));
});
$("#resetLayout").addEventListener("click", () => {
  const p = profile();
  // built-in profiles go back to their own layout, others to each overlay's usual place
  const base = defaultProfile(BUILT_IN.includes(p.name) ? p.name : "");
  commit((c) => {
    for (const [id, o] of Object.entries(active(c).overlays)) {
      o.x = base.overlays[id].x;
      o.y = base.overlays[id].y;
      o.anchor = base.overlays[id].anchor;
    }
  });
});
$("#editInGame").addEventListener("click", () => void app.setEditing(true));
$("#displaySel").addEventListener("change", (e) => {
  shownDisplay = +(e.target as HTMLSelectElement).value;
  renderLayout();
});
$("#selDisplay").addEventListener("change", (e) => {
  const id = selBox;
  if (!id) return;
  const d = +(e.target as HTMLSelectElement).value;
  commit((c) => (active(c).overlays[id].display = d));
});
addEventListener("resize", () => page === "layout" && renderLayout());

/* ---------- appearance page ---------- */

const DEFAULT_ACCENT = "#a78bfa";
const SWATCHES = ["#a78bfa", "#4c8dff", "#22d3ee", "#3ddc84", "#ffb020", "#ff5a5f", "#f472b6"];
const DEFAULT_PANEL = 66;
let lookMounted = false;

function renderAppearance() {
  const cur = cfg.appearance.accent ?? DEFAULT_ACCENT;
  $("#swatches").innerHTML =
    SWATCHES.map((c) => `<button class="swatch${c === cur ? " on" : ""}" style="background:${c}" data-c="${c}" aria-label="Accent ${c}"></button>`).join("") +
    `<input type="color" class="swatch-input" id="accentPick" value="${cur}" aria-label="Any accent color">`;
  const r = $<HTMLInputElement>("#tOpacity");
  r.value = String(cfg.appearance.panelOpacity ?? DEFAULT_PANEL);
  rangeFill(r);
  $("#vtOpacity").textContent = r.value + "%";
  if (!lookMounted) {
    lookMounted = true;
    whenPreviewReady(() => {
      mountPreview($("#lookStage"), "relative");
      mountPreview($("#lookStage"), "delta");
    });
  }
}
$("#swatches").addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLElement>(".swatch");
  if (!b) return;
  const c = b.dataset.c!;
  commit((x) => (x.appearance.accent = c === DEFAULT_ACCENT ? null : c));
});
$("#swatches").addEventListener("change", (e) => {
  const t = e.target as HTMLInputElement;
  if (t.id === "accentPick") commit((x) => (x.appearance.accent = t.value));
});
$("#tOpacity").addEventListener("input", (e) => {
  const r = e.target as HTMLInputElement;
  rangeFill(r);
  $("#vtOpacity").textContent = r.value + "%";
  commit((x) => (x.appearance.panelOpacity = +r.value === DEFAULT_PANEL ? null : +r.value), false);
  applyAppearance(cfg.appearance);
  HUD.restyle();
});

/* ---------- profiles page ---------- */

let renaming: string | null = null;

function uniqueName(base: string) {
  let n = 1, name = base;
  while (cfg.profiles.some((p) => p.name === name)) name = `${base} ${++n}`;
  return name;
}

function renderProfiles() {
  $("#profGrid").innerHTML = cfg.profiles
    .map((p) => {
      const isActive = p.name === cfg.profile;
      const on = Object.entries(p.overlays).filter(([, o]) => o.on);
      const mini = on
        .map(([id, o]) => {
          const s = SIZES[id];
          return `<i style="left:${(o.x / 1920) * 100}%;top:${(o.y / 1080) * 100}%;width:${((s.w * o.scale) / 100 / 1920) * 100}%;height:${((s.h * o.scale) / 100 / 1080) * 100}%"></i>`;
        })
        .join("");
      const name = renaming === p.name
        ? `<input class="nm-input" value="${esc(p.name)}" aria-label="Profile name">`
        : `<span class="nm">${esc(p.name)}</span>`;
      return `<div class="card prof${isActive ? " active" : ""}" data-name="${esc(p.name)}">
        <div class="top">${name}<span class="spacer"></span>${isActive ? '<span class="pill">In use</span>' : ""}</div>
        <div class="mini">${mini}</div>
        <div class="meta">${on.length} overlay${on.length === 1 ? "" : "s"}</div>
        <div class="actions">
          ${isActive ? "" : '<button class="btn" data-act="use">Use</button>'}
          <button class="btn" data-act="rename">Rename</button>
          <button class="btn" data-act="duplicate">Duplicate</button>
          ${cfg.profiles.length > 1 ? '<button class="btn danger" data-act="delete">Delete</button>' : ""}
        </div>
      </div>`;
    })
    .join("");
  const input = $<HTMLInputElement>("#profGrid .nm-input");
  if (input) {
    input.focus();
    input.select();
    const done = (keep: boolean) => {
      const old = renaming!;
      renaming = null;
      const name = input.value.trim();
      if (!keep || !name || name === old) return renderProfiles();
      const next = cfg.profiles.some((p) => p.name === name) ? uniqueName(name) : name;
      commit((c) => {
        const p = c.profiles.find((x) => x.name === old)!;
        p.name = next;
        if (c.profile === old) c.profile = next;
        for (const k of ["practice", "qualifying", "race", "endurance"] as const) if (c.autoSwitch[k] === old) c.autoSwitch[k] = next;
      });
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") done(true);
      if (e.key === "Escape") done(false);
    });
    input.addEventListener("blur", () => renaming && done(true));
  }

  const opts = (cur: string | null) =>
    `<option value=""${cur == null ? " selected" : ""}>Keep current</option>` +
    cfg.profiles.map((p) => `<option${p.name === cur ? " selected" : ""}>${esc(p.name)}</option>`).join("");
  const a = cfg.autoSwitch;
  const rows: [keyof typeof a, string][] = [
    ["practice", "Practice"],
    ["qualifying", "Qualifying"],
    ["race", "Race shorter than 90 min"],
    ["endurance", "Race of 90 min or longer"],
  ];
  $("#autoSwitch").innerHTML = rows
    .map(([k, label]) => `<div class="set-row"><span class="lbl">${label}</span><select class="select" data-k="${k}">${opts(a[k])}</select></div>`)
    .join("");
}

$("#profGrid").addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLElement>("button[data-act]");
  if (!b) return;
  const name = b.closest<HTMLElement>(".prof")!.dataset.name!;
  const act = b.dataset.act;
  if (act === "use") commit((c) => (c.profile = name));
  if (act === "rename") {
    renaming = name;
    renderProfiles();
  }
  if (act === "duplicate") {
    const copy = structuredClone(cfg.profiles.find((p) => p.name === name)!);
    copy.name = uniqueName(name + " copy");
    commit((c) => c.profiles.splice(c.profiles.findIndex((p) => p.name === name) + 1, 0, copy));
  }
  if (act === "delete") {
    commit((c) => {
      c.profiles = c.profiles.filter((p) => p.name !== name);
      if (c.profile === name) c.profile = c.profiles[0].name;
      for (const k of ["practice", "qualifying", "race", "endurance"] as const) if (c.autoSwitch[k] === name) c.autoSwitch[k] = null;
    });
  }
});
$("#autoSwitch").addEventListener("change", (e) => {
  const s = e.target as HTMLSelectElement;
  const k = s.dataset.k as keyof app.Config["autoSwitch"];
  commit((c) => (c.autoSwitch[k] = s.value || null), false);
});
$("#newProfile").addEventListener("click", () => {
  const p = structuredClone(profile());
  p.name = uniqueName("New profile");
  commit((c) => {
    c.profiles.push(p);
    c.profile = p.name;
  });
  renaming = p.name;
  renderProfiles();
});

/* ---------- settings page ---------- */

let acc: app.AccSetup | null = null;

function renderSettings() {
  setSwitch($("#demoToggle"), cfg.demo);
  const card = $("#accCard");
  if (!acc) {
    card.innerHTML = '<span class="empty-note">Checking broadcasting.json…</span>';
    void app.accSetup().then((a) => {
      acc = a;
      renderSettings();
    });
    return;
  }
  const lines: string[] = [];
  if (!acc.path) {
    card.innerHTML = `<span class="empty-note">${esc(acc.error ?? "ACC's settings folder wasn't found.")}</span>`;
    return;
  }
  if (acc.enabled) {
    lines.push(`<div class="row"><span class="ok">●</span><span>Broadcasting is on (port ${acc.port}). Other cars' data comes through.</span></div>`);
  } else {
    lines.push(`<div class="row"><span class="warn">●</span><span>Broadcasting is off, so only your own car shows. Turn it on, then restart ACC.</span></div>`);
    lines.push(`<div><button class="btn primary" id="accEnable">Turn on broadcasting</button></div>`);
  }
  if (acc.error) lines.push(`<div class="err">${esc(acc.error)}</div>`);
  lines.push(`<div class="path">${esc(acc.path)}</div>`);
  card.innerHTML = lines.join("");
  $("#accEnable")?.addEventListener("click", async () => {
    acc = await app.accEnable();
    renderSettings();
  });
}
onSwitch($("#demoToggle"), (on) => commit((c) => (c.demo = on)));

/* ---------- updates ---------- */

type UpdateState =
  | { k: "checking" }
  | { k: "latest" }
  | { k: "offline"; error: string }
  | { k: "available"; info: app.UpdateInfo; error?: string }
  | { k: "installing"; info: app.UpdateInfo; done: number; total: number | null };
let update: UpdateState = { k: "checking" };

async function checkUpdate() {
  update = { k: "checking" };
  renderUpdate();
  try {
    const info = await app.checkUpdate();
    update = info ? { k: "available", info } : { k: "latest" };
  } catch (e) {
    update = { k: "offline", error: String(e) };
  }
  renderUpdate();
}

async function installUpdate() {
  if (update.k !== "available") return;
  update = { k: "installing", info: update.info, done: 0, total: null };
  renderUpdate();
  try {
    await app.installUpdate(); // restarts the app when it works
  } catch (e) {
    update = { k: "available", info: (update as { info: app.UpdateInfo }).info, error: String(e) };
    renderUpdate();
  }
}

function renderUpdate() {
  const u = update;
  const note = $("#updateNote");
  note.hidden = u.k !== "available" && u.k !== "installing";
  const card = $("#updateCard");
  if (!app.inTauri) {
    card.innerHTML = "<span>Updates are checked in the app.</span>";
    return;
  }
  const pct = u.k === "installing" && u.total ? ` ${Math.round((100 * u.done) / u.total)}%` : "";
  switch (u.k) {
    case "checking":
      card.innerHTML = "<span>Checking for updates…</span>";
      break;
    case "latest":
      card.innerHTML = '<span>You have the latest version.</span><button class="btn" id="updateCheck">Check again</button>';
      break;
    case "offline":
      card.innerHTML = `<span>Couldn't check for updates. <span class="err">${esc(u.error)}</span></span><button class="btn" id="updateCheck">Try again</button>`;
      break;
    case "available":
      card.innerHTML = `<span>Version ${esc(u.info.version)} is available.${u.error ? ` <span class="err">The install failed: ${esc(u.error)}</span>` : ""}</span><button class="btn primary" id="updateInstall">Install and restart</button>`;
      break;
    case "installing":
      card.innerHTML = `<span>Downloading version ${esc(u.info.version)}…${pct}</span>`;
      break;
  }
  if (u.k === "available" || u.k === "installing") {
    $("#updateNoteText").textContent =
      u.k === "installing" ? `Downloading version ${u.info.version}…${pct}` : `Version ${u.info.version} is available.`;
    $<HTMLButtonElement>("#updateNoteBtn").disabled = u.k === "installing";
  }
  $("#updateCheck")?.addEventListener("click", () => void checkUpdate());
  $("#updateInstall")?.addEventListener("click", () => void installUpdate());
}
$("#updateNoteBtn").addEventListener("click", () => void installUpdate());
app.onUpdateProgress((p) => {
  if (update.k !== "installing") return;
  update.done = p.done;
  update.total = p.total;
  renderUpdate();
});

/* ---------- settings page: music and wheel buttons ---------- */

const MEDIA: [app.MediaCommand, string][] = [["playPause", "Play / pause"], ["next", "Next track"], ["previous", "Previous track"]];
const SLOT: Record<app.MediaCommand, keyof app.Config["mediaButtons"]> = { playPause: "playPause", next: "next", previous: "previous" };
const DIRS = ["up", "right", "down", "left"];
let media: app.MediaInfo | null = null;
let learning: app.MediaCommand | null = null;
let learnTimer: ReturnType<typeof setTimeout> | null = null;

/** "Button 5" or "Hat up", as the controller numbers them (from 1). */
function buttonName(p: app.Press) {
  if (p.button < 128) return `Button ${p.button + 1}`;
  const k = p.button - 128, hat = Math.floor(k / 4);
  return `${hat ? `Hat ${hat + 1}` : "D-pad"} ${DIRS[k % 4]}`;
}

function learn(c: app.MediaCommand | null) {
  learning = c;
  if (learnTimer) clearTimeout(learnTimer);
  learnTimer = c ? setTimeout(() => learn(null), 15000) : null;
  void app.mediaLearn(c);
  renderMusic();
}

function renderMusic() {
  const rows = $("#musicRows"), hint = $("#musicHint");
  if (!media) {
    rows.innerHTML = '<div class="set-row wide"><span class="empty-note">Looking for a media player…</span></div>';
    void app.getMedia().then((m) => {
      media = m;
      renderMusic();
    });
    return;
  }
  const n = media.supported ? media.now : null;
  const playing = n ? `${esc(n.title || "Unknown title")}${n.artist ? ` <span class="dim">by ${esc(n.artist)}</span>` : ""}` : '<span class="dim">Nothing playing</span>';
  const out = [`<div class="set-row music-now"><span class="lbl">Now playing<small>${n ? esc(n.app) : "Spotify or any player Windows shows in its media controls"}</small></span>
    <span class="trunc-line">${playing}</span>
    <span class="music-ctl">${MEDIA.map(([c, label]) => `<button class="btn small" data-send="${c}" title="${label}" aria-label="${label}">${{ playPause: "⏯", next: "⏭", previous: "⏮" }[c]}</button>`).join("")}</span></div>`];
  for (const [c, label] of MEDIA) {
    const b = cfg.mediaButtons?.[SLOT[c]]; // configs from before music had none
    const what = learning === c ? '<span class="learning">Press a button on your wheel…</span>'
      : b ? `${esc(buttonName(b))} <span class="dim">on ${esc(b.name)}</span>` : '<span class="dim">Not assigned</span>';
    out.push(`<div class="set-row music-bind"><span class="lbl">${label}</span><span class="trunc-line">${what}</span>
      <span class="music-ctl">${b && learning !== c ? `<button class="btn small" data-clear="${c}">Clear</button>` : ""}
      <button class="btn small${learning === c ? "" : " primary"}" data-learn="${c}">${learning === c ? "Cancel" : b ? "Change" : "Assign"}</button></span></div>`);
  }
  rows.innerHTML = out.join("");
  rows.querySelectorAll<HTMLButtonElement>("button").forEach((btn) => (btn.disabled = !media!.supported));
  hint.textContent = media.supported
    ? "Wheel buttons work while you drive; the game still gets them too, so pick ones it doesn't use."
    : "Showing what's playing and controlling it with wheel buttons needs Windows.";
}
$("#musicRows").addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLElement>("button");
  if (!b) return;
  if (b.dataset.send) void app.mediaSend(b.dataset.send as app.MediaCommand);
  else if (b.dataset.clear) {
    const c = b.dataset.clear as app.MediaCommand;
    commit((cf) => cf.mediaButtons && (cf.mediaButtons[SLOT[c]] = null));
  } else if (b.dataset.learn) {
    const c = b.dataset.learn as app.MediaCommand;
    learn(learning === c ? null : c);
  }
});
app.onMedia((n) => {
  if (media) media = { ...media, now: n };
  if (page === "settings") renderMusic();
});
app.onMediaLearned(() => learn(null));

/* ---------- boot ---------- */

pageRender.overlays = renderOverlays;
pageRender.layout = () => requestAnimationFrame(renderLayout);
pageRender.appearance = renderAppearance;
pageRender.profiles = renderProfiles;
pageRender.settings = () => {
  renderSettings();
  renderMusic();
};

async function boot() {
  [cfg, ui, status, displays] = await Promise.all([app.getConfig(), app.getUi(), app.getStatus(), app.getDisplays()]);
  if (app.inTauri) {
    const { getVersion } = await import("@tauri-apps/api/app");
    $("#version").textContent = await getVersion();
  } else $("#version").textContent = "dev";
  app.onConfig(external);
  app.onUi((u) => {
    ui = u;
    if (page === "overlays") renderOverlays();
  });
  app.onStatus((s) => {
    status = s;
    renderStatus();
  });
  void checkUpdate();
  HUD.start(() => pview);
  startPreview();
  const q = new URLSearchParams(location.search);
  if (q.get("overlay") && byId[q.get("overlay")!]) selected = q.get("overlay")!;
  applyAppearance(cfg.appearance);
  renderSidebar();
  go(q.get("page") || "overlays");
  if (document.fonts) void document.fonts.ready.then(fitPreview);
  HUD.onTheme(() => requestAnimationFrame(fitPreview));
}

void boot();
