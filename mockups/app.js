/* App window mockup: navigation, overlay list + live preview, layout editor, appearance, profiles. */
(function () {
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));
  const q = new URLSearchParams(location.search);
  if (q.has("debug")) addEventListener("error", (e) => (document.documentElement.dataset.errors = (document.documentElement.dataset.errors || "") + e.message + " | "));

  /* ---------- catalog (the app's own metadata; size is the 1080p default box) ---------- */
  const ICONS = {
    relative: '<path d="M3 4h12M3 9h12M3 14h12" /><circle cx="6" cy="9" r="1.6" fill="currentColor"/>',
    standings: '<path d="M3 4h12M3 7.5h12M3 11h12M3 14.5h8"/>',
    delta: '<path d="M2.5 9h13"/><rect x="9" y="6" width="5" height="6" rx="1"/>',
    inputs: '<path d="M2 13l3-6 3 3 3-7 3 9 2-3"/>',
    fuel: '<rect x="3" y="3" width="8" height="12" rx="1.5"/><path d="M11 6l3 2v5a1.5 1.5 0 0 1-3 0M5 6h4"/>',
    radar: '<circle cx="9" cy="9" r="6.5"/><circle cx="9" cy="9" r="3"/><path d="M9 2.5V9l4.5 3"/>',
    trackmap: '<path d="M4 13c-2-2 0-5 2-6s3-4 6-3 4 4 1 6-2 4-4 4-3 0-5-1z"/>',
    session: '<path d="M4 16V2.5M4 3h9l-2 3 2 3H4"/>',
    tyres: '<circle cx="9" cy="9" r="6.5"/><circle cx="9" cy="9" r="2.5"/>',
    weather: '<path d="M5.5 12.5a3 3 0 0 1 .4-6 4 4 0 0 1 7.6 1.2 2.4 2.4 0 0 1-.5 4.8z"/><path d="M7 15l-.6 1.4M10.5 15l-.6 1.4"/>',
    pit: '<path d="M3 15V5l6-2.5L15 5v10M7 15v-4h4v4"/>',
    electronics: '<rect x="3" y="4" width="12" height="10" rx="2"/><path d="M6 7.5v3M9 7.5v3M12 7.5v3"/>',
    timer: '<circle cx="9" cy="10" r="6"/><path d="M9 10V7M7.5 2.5h3"/>',
    laplog: '<path d="M6.5 4.5h8.5M6.5 9h8.5M6.5 13.5h8.5M3 4.5h.01M3 9h.01M3 13.5h.01"/>',
    lapgraph: '<path d="M2.5 15.5h13M3.5 12l3.5-4 3 2.5 4.5-6"/>',
    blindspot: '<path d="M6 5L2.5 9 6 13M12 5l3.5 4-3.5 4"/>',
    minimap: '<rect x="2.5" y="2.5" width="13" height="13" rx="3"/><path d="M9 6l2.5 5.5L9 10.2l-2.5 1.3z"/>',
    flatmap: '<path d="M2.5 9h13M2.5 6.5v5M15.5 6.5v5"/><circle cx="6.5" cy="9" r="1.4" fill="currentColor"/><circle cx="11.5" cy="9" r="1.4" fill="currentColor"/>',
    digiflag: '<path d="M3.5 16V2.5"/><g fill="currentColor" stroke="none"><circle cx="7" cy="4.5" r="1"/><circle cx="10.5" cy="4.5" r="1"/><circle cx="14" cy="4.5" r="1"/><circle cx="7" cy="8" r="1"/><circle cx="10.5" cy="8" r="1"/><circle cx="14" cy="8" r="1"/></g>',
  };
  const CATALOG = [
    { id: "relative", group: "Core", title: "Relative", desc: "Cars around you on track, with gaps", w: 380, h: 290, x: 1480, y: 560, on: true },
    { id: "standings", group: "Core", title: "Standings", desc: "Classification with status icons", w: 530, h: 520, x: 24, y: 120, on: true },
    { id: "delta", group: "Core", title: "Delta", desc: "Live delta to your best lap", w: 520, h: 72, x: 700, y: 96, on: true },
    { id: "inputs", group: "Core", title: "Inputs", desc: "Throttle, brake, steering and gear", w: 420, h: 120, x: 752, y: 936, on: true },
    { id: "fuel", group: "Core", title: "Fuel", desc: "Usage per lap and how much to add", w: 260, h: 170, x: 1640, y: 880, on: true },
    { id: "timer", group: "Core", title: "Session timer", desc: "Time and laps remaining at a glance", w: 240, h: 110, x: 24, y: 24, on: false },
    { id: "laplog", group: "Core", title: "Lap log", desc: "Every lap with sectors, delta and fuel", w: 380, h: 310, x: 24, y: 680, on: false },
    { id: "lapgraph", group: "Core", title: "Lap times graph", desc: "Your laps against the cars around you", w: 420, h: 200, x: 1476, y: 340, on: false },
    { id: "radar", group: "Awareness", title: "Radar", desc: "Cars alongside and close behind", w: 220, h: 220, x: 848, y: 472, on: true },
    { id: "trackmap", group: "Awareness", title: "Track map", desc: "Everyone's position on the circuit", w: 300, h: 260, x: 1600, y: 24, on: false },
    { id: "session", group: "Awareness", title: "Session", desc: "Flags, time remaining and position", w: 360, h: 60, x: 784, y: 24, on: true },
    { id: "digiflag", group: "Awareness", title: "Digiflag", desc: "Trackside-style LED flag panel", w: 240, h: 150, x: 1180, y: 24, on: false },
    { id: "blindspot", group: "Awareness", title: "Blind spot", desc: "Mirror-style warning lights", w: 640, h: 60, x: 640, y: 360, on: false },
    { id: "minimap", group: "Awareness", title: "Minimap", desc: "Zoomed map that turns with your car", w: 220, h: 220, x: 1680, y: 300, on: false },
    { id: "flatmap", group: "Awareness", title: "Flat map", desc: "The lap as one line with every car on it", w: 720, h: 56, x: 600, y: 210, on: false },
    { id: "tyres", group: "Car and strategy", title: "Tyres", desc: "Pressures, temperatures and wear", w: 260, h: 220, x: 24, y: 696, on: true },
    { id: "weather", group: "Car and strategy", title: "Weather", desc: "Temperatures, rain forecast and grip", w: 280, h: 150, x: 1296, y: 24, on: false },
    { id: "pit", group: "Car and strategy", title: "Pit strategy", desc: "Pit window and stop planning", w: 280, h: 170, x: 304, y: 880, on: false },
    { id: "electronics", group: "Car and strategy", title: "Electronics", desc: "TC, ABS, brake bias and engine map", w: 340, h: 72, x: 792, y: 856, on: true },
  ];
  const DEFAULT_POS = CATALOG.map((o) => ({ x: o.x, y: o.y }));
  const byId = Object.fromEntries(CATALOG.map((o) => [o.id, o]));
  const icon = (id) => `<svg viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${ICONS[id]}</svg>`;

  /* contextual content options */
  const CONTENT = {
    relative: { steppers: [["Cars ahead", 4], ["Cars behind", 4]], cols: ["Class position", "Car number", "Driver category", "Last lap", "Gap"] },
    standings: { steppers: [["Cars per class", 12]], cols: ["Position change", "Car number", "Status icons", "Last lap", "Best lap", "Gap", "Interval", "Pit stops"] },
    delta: { cols: ["Delta bar", "Predicted lap", "Last lap", "Best lap"], select: ["Compare to", ["Best lap", "Last lap", "Session best"]] },
    inputs: { cols: ["Trace", "Pedal bars", "Steering", "Gear and speed", "ABS and TC marks"], select: ["Trace length", ["5 seconds", "3 seconds", "8 seconds"]] },
    fuel: { cols: ["Laps left", "Per lap", "To finish", "To add", "Time to empty"], select: ["Safety margin", ["Half a lap", "One lap", "None"]] },
    radar: { cols: ["Cars behind", "Distance labels"], select: ["Range", ["20 m", "10 m", "40 m"]] },
    trackmap: { cols: ["Car numbers", "Pit lane", "Sector markers", "Other classes"] },
    session: { cols: ["Flags", "Time remaining", "Laps remaining", "Position", "Clock"] },
    tyres: { cols: ["Pressure", "Temperatures", "Wear", "Brake temperature"], select: ["Temperature view", ["Inner, middle, outer", "Core only"]] },
    weather: { cols: ["Air temperature", "Track temperature", "Forecast", "Wind"] },
    pit: { cols: ["Pit window", "Fuel to add", "Position after stop", "Stops done"] },
    electronics: { cols: ["TC", "TC cut", "ABS", "Brake bias", "Engine map", "Shift lights"] },
    timer: { cols: ["Session name", "Laps remaining", "Progress bar", "Clock"] },
    laplog: { steppers: [["Laps shown", 8]], cols: ["Delta to best", "Sectors", "Fuel used", "Current lap"] },
    lapgraph: { steppers: [["Laps shown", 20]], cols: ["Car ahead", "Car behind", "Personal best marker"] },
    digiflag: { cols: ["Flash yellow and blue", "Show green after a flag clears", "Caption"] },
    blindspot: { cols: ["Distance", "Red when alongside"] },
    minimap: { select: ["Zoom", ["300 m", "150 m", "500 m"]], cols: ["Heading up", "Distance rings", "Other classes"] },
    flatmap: { cols: ["Sector markers", "Numbers for cars ahead and behind", "Cars in pit lane"] },
  };

  /* ---------- load all overlay files, tolerate missing ones ---------- */
  function loadOverlays() {
    return Promise.all(
      CATALOG.map(
        (o) =>
          new Promise((resolve) => {
            const css = document.createElement("link");
            css.rel = "stylesheet";
            css.href = "overlays/" + o.id + ".css";
            document.head.append(css);
            const js = document.createElement("script");
            js.src = "overlays/" + o.id + ".js";
            js.onload = js.onerror = () => resolve();
            document.body.append(js);
          })
      )
    );
  }

  /** Mount an overlay into a fresh element; returns {el, inst, def} or null. */
  function mountInto(stage, id) {
    if (!HUD.overlays[id]) return null;
    const el = document.createElement("div");
    stage.append(el);
    try {
      const inst = HUD.mount(id, el);
      return { el, inst, def: HUD.overlays[id] };
    } catch (e) {
      console.warn("mount failed", id, e);
      el.remove();
      return null;
    }
  }
  function unmount(m) {
    if (!m) return;
    m.inst._unsub && m.inst._unsub();
    m.inst._unTheme && m.inst._unTheme();
    m.inst.destroy && m.inst.destroy();
    m.el.remove();
  }

  /* ---------- small controls ---------- */
  function wireSwitch(btn, onChange) {
    if (!btn.hasAttribute("aria-checked")) btn.setAttribute("aria-checked", "false");
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const on = btn.getAttribute("aria-checked") !== "true";
      btn.setAttribute("aria-checked", on);
      onChange && onChange(on);
    });
  }
  function rangeFill(r) {
    r.style.setProperty("--fill", ((r.value - r.min) / (r.max - r.min)) * 100 + "%");
  }
  $$("input[type=range]").forEach((r) => {
    rangeFill(r);
    r.addEventListener("input", () => rangeFill(r));
  });
  $$(".switch").forEach((b) => {
    if (!b.id) wireSwitch(b);
  });
  $$(".seg").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      $$("button", seg).forEach((x) => x.setAttribute("aria-pressed", x === b));
    })
  );

  /* ---------- navigation ---------- */
  const pages = $$(".page");
  const navBtns = $$("#nav button");
  const pageInit = {};
  function go(page) {
    pages.forEach((p) => p.classList.toggle("active", p.dataset.page === page));
    navBtns.forEach((b) => (b.dataset.page === page ? b.setAttribute("aria-current", "page") : b.removeAttribute("aria-current")));
    if (pageInit[page]) pageInit[page]();
  }
  navBtns.forEach((b) => b.addEventListener("click", () => go(b.dataset.page)));

  /* ---------- game selector ---------- */
  const GAMES = [
    ["ACC", "Assetto Corsa Competizione", true],
    ["iR", "iRacing", false],
    ["LMU", "Le Mans Ultimate", false],
    ["EVO", "Assetto Corsa EVO", false],
    ["rF2", "rFactor 2", false],
  ];
  const game = $("#game");
  $("#gameMenu").innerHTML = GAMES.map(
    ([k, n, on]) =>
      `<div class="game-opt${on ? "" : " disabled"}" role="option" aria-selected="${on}" aria-disabled="${!on}">
        <span class="game-logo">${k}</span><span>${n}</span>
        ${on ? '<svg class="check" viewBox="0 0 14 14"><path d="M3 7.5l2.5 2.5L11 4.5" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>' : '<span class="soon">Coming soon</span>'}
      </div>`
  ).join("");
  $("#gameBtn").addEventListener("click", (e) => {
    e.stopPropagation();
    const open = game.classList.toggle("open");
    $("#gameBtn").setAttribute("aria-expanded", open);
  });
  document.addEventListener("click", () => game.classList.remove("open"));
  if (q.get("status") === "waiting") {
    game.dataset.status = "waiting";
    $("#gameStatus").textContent = "Waiting for game";
  }

  /* ---------- overlays page ---------- */
  let selected = q.get("overlay") || "relative";
  let preview = null;
  const look = { opacity: 100, scale: 100, font: 100 };

  function summary() {
    const n = CATALOG.filter((o) => o.on).length;
    $("#ovSummary").textContent = n + " of " + CATALOG.length + " shown";
    $("#navCount").textContent = n;
  }

  function renderList() {
    const groups = [...new Set(CATALOG.map((o) => o.group))];
    $("#ovList").innerHTML = groups
      .map(
        (g) => `<div class="ov-group"><h2>${g}</h2>${CATALOG.filter((o) => o.group === g)
          .map(
            (o, i) => `<div class="ov-item${o.on ? " on" : ""}${o.id === selected ? " selected" : ""}" data-id="${o.id}" tabindex="0">
              <span class="ov-icon">${icon(o.id)}</span>
              <span class="txt"><div class="t">${o.title}</div><div class="d">${o.desc}</div></span>
              <kbd>Alt ${CATALOG.indexOf(o) + 1 > 9 ? "F" + (CATALOG.indexOf(o) - 8) : CATALOG.indexOf(o) + 1}</kbd>
              <button class="switch" role="switch" aria-checked="${o.on}" aria-label="Show ${o.title}"></button>
            </div>`
          )
          .join("")}</div>`
      )
      .join("");
    $$("#ovList .ov-item").forEach((item) => {
      const o = byId[item.dataset.id];
      wireSwitch($(".switch", item), (on) => {
        o.on = on;
        item.classList.toggle("on", on);
        summary();
        if (o.id === selected) syncDetailToggle();
      });
      item.addEventListener("click", () => select(o.id));
      item.addEventListener("keydown", (e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), select(o.id)));
    });
    summary();
  }

  const dToggle = $("#dToggle");
  wireSwitch(dToggle, (on) => {
    byId[selected].on = on;
    const item = $(`.ov-item[data-id="${selected}"]`);
    item.classList.toggle("on", on);
    $(".switch", item).setAttribute("aria-checked", on);
    summary();
    syncDetailToggle();
  });
  function syncDetailToggle() {
    const o = byId[selected];
    dToggle.setAttribute("aria-checked", o.on);
    $("#dState").textContent = o.on ? "Shown in game" : "Hidden";
  }

  function renderContent(id) {
    const c = CONTENT[id] || { cols: [] };
    let html = "";
    (c.steppers || []).forEach(([label, v]) => {
      html += `<div class="set-row wide"><span class="lbl">${label}</span><span><span class="stepper"><button aria-label="Fewer">−</button><span>${v}</span><button aria-label="More">+</button></span></span></div>`;
    });
    if (c.select) {
      html += `<div class="set-row wide"><span class="lbl">${c.select[0]}</span><span><select class="select" style="width:200px">${c.select[1].map((x) => `<option>${x}</option>`).join("")}</select></span></div>`;
    }
    html += `<div class="set-row wide"><span class="lbl">Show</span><span class="checks">${c.cols
      .map((x) => `<label><input type="checkbox" checked> ${x}</label>`)
      .join("")}</span></div>`;
    const box = $("#contentSettings");
    box.innerHTML = html;
    $$(".stepper", box).forEach((s) => {
      const span = $("span", s);
      s.addEventListener("click", (e) => {
        const b = e.target.closest("button");
        if (!b) return;
        const v = +span.textContent + (b.getAttribute("aria-label") === "More" ? 1 : -1);
        span.textContent = Math.max(1, Math.min(30, v));
      });
    });
  }

  /* font size: scale the --fs-* tokens on the preview root */
  const FS = ["--fs-xs", "--fs-sm", "--fs-md", "--fs-lg", "--fs-xl", "--fs-xxl"];
  function applyLook() {
    $("#vOpacity").textContent = look.opacity + "%";
    $("#vScale").textContent = look.scale + "%";
    $("#vFont").textContent = look.font + "%";
    if (!preview) return;
    const el = preview.el;
    el.style.opacity = look.opacity / 100;
    FS.forEach((v) => el.style.removeProperty(v));
    if (look.font !== 100) {
      const cs = getComputedStyle(el);
      FS.forEach((v) => el.style.setProperty(v, (parseFloat(cs.getPropertyValue(v)) * look.font) / 100 + "px"));
    }
    fitPreview();
  }
  function fitPreview() {
    if (!preview) return;
    const stage = $("#previewStage");
    stage.style.transform = "translate(-50%, -50%)";
    const box = $("#preview").getBoundingClientRect();
    // content can be taller than the overlay's default box (e.g. standings with many cars)
    const el = preview.el, inner = el.firstElementChild;
    const w = Math.max(el.offsetWidth, el.scrollWidth);
    const h = Math.max(el.offsetHeight, el.scrollHeight, inner ? inner.offsetHeight : 0);
    const want = look.scale / 100;
    const fit = Math.min(want, (box.width - 40) / w, (box.height - 44) / h);
    stage.style.transform = `translate(-50%, -50%) scale(${fit})`;
  }
  [["#sOpacity", "opacity"], ["#sScale", "scale"], ["#sFont", "font"]].forEach(([sel, key]) =>
    $(sel).addEventListener("input", (e) => {
      look[key] = +e.target.value;
      applyLook();
    })
  );

  const previewRO = new ResizeObserver(() => fitPreview());
  function select(id) {
    previewRO.disconnect();
    selected = id;
    $$("#ovList .ov-item").forEach((i) => i.classList.toggle("selected", i.dataset.id === id));
    const o = byId[id];
    $("#dTitle").textContent = o.title;
    syncDetailToggle();
    renderContent(id);
    unmount(preview);
    const stage = $("#previewStage");
    stage.innerHTML = "";
    preview = mountInto(stage, id);
    if (!preview) stage.innerHTML = '<span class="preview-missing">Preview not available yet</span>';
    // overlays may resize themselves later (fonts, rows appearing): keep the preview fitted
    applyLook();
    if (preview) {
      previewRO.observe(preview.el);
      if (preview.el.firstElementChild) previewRO.observe(preview.el.firstElementChild);
    }
  }

  $("#hideAll").addEventListener("click", () => {
    const anyOn = CATALOG.some((o) => o.on);
    CATALOG.forEach((o) => (o.on = !anyOn));
    renderList();
    syncDetailToggle();
    $("#hideAll span").textContent = anyOn ? "Show all" : "Hide all";
  });

  /* ---------- layout page ---------- */
  const screen = $("#screen");
  const SNAP = 8;
  let layoutBuilt = false, scaleK = 1, selBox = "relative";
  const gV = $("#gV"), gH = $("#gH");

  function buildLayout() {
    $$(".lbox", screen).forEach((b) => b.remove());
    scaleK = screen.clientWidth / 1920;
    screen.style.setProperty("--grid", SNAP * 2 * scaleK + "px");
    CATALOG.filter((o) => o.on).forEach((o) => {
      const b = document.createElement("div");
      b.className = "lbox";
      b.dataset.id = o.id;
      b.innerHTML = icon(o.id) + `<span>${o.title}</span><span class="size">${o.w}×${o.h}</span>`;
      place(b, o);
      screen.append(b);
      b.addEventListener("pointerdown", (e) => startDrag(e, b, o));
    });
    if (selBox && byId[selBox] && byId[selBox].on) selectBox(selBox);
    else selectBox(null);
    const off = CATALOG.filter((o) => !o.on);
    const hl = $("#hiddenList");
    hl.innerHTML = off.length
      ? off.map((o) => `<div class="hidden-item">${icon(o.id)}<span>${o.title}</span><span class="spacer"></span><button class="btn" data-id="${o.id}">Show</button></div>`).join("")
      : '<span class="hint">Every overlay is on screen.</span>';
  }
  $("#hiddenList").addEventListener("click", (e) => {
    const b = e.target.closest("button[data-id]");
    if (!b) return;
    byId[b.dataset.id].on = true;
    selBox = b.dataset.id;
    renderList();
    syncDetailToggle();
    buildLayout();
  });
  function place(b, o) {
    b.style.left = o.x * scaleK + "px";
    b.style.top = o.y * scaleK + "px";
    b.style.width = o.w * scaleK + "px";
    b.style.height = o.h * scaleK + "px";
  }
  function selectBox(id) {
    selBox = id;
    $$(".lbox", screen).forEach((b) => b.classList.toggle("selected", b.dataset.id === id));
    const o = id && byId[id];
    $("#selName").textContent = o ? o.title : "Select an overlay";
    $("#selX").textContent = o ? o.x : "—";
    $("#selY").textContent = o ? o.y : "—";
    $("#selW").textContent = o ? o.w : "—";
    $("#selH").textContent = o ? o.h : "—";
  }
  function startDrag(e, b, o) {
    e.preventDefault();
    b.setPointerCapture(e.pointerId);
    selectBox(o.id);
    b.classList.add("dragging");
    const sx = e.clientX, sy = e.clientY, ox = o.x, oy = o.y;
    const snapOn = () => $("#snapChk").checked;
    const others = CATALOG.filter((x) => x.on && x !== o);
    function move(ev) {
      let x = ox + (ev.clientX - sx) / scaleK;
      let y = oy + (ev.clientY - sy) / scaleK;
      let gx = null, gy = null;
      if (snapOn()) {
        x = Math.round(x / SNAP) * SNAP;
        y = Math.round(y / SNAP) * SNAP;
        const T = 12;
        // screen center
        if (Math.abs(x + o.w / 2 - 960) < T) { x = 960 - o.w / 2; gx = 960; }
        if (Math.abs(y + o.h / 2 - 540) < T) { y = 540 - o.h / 2; gy = 540; }
        // edges of other overlays
        for (const t of others) {
          if (gx === null) {
            for (const [a, bx] of [[x, t.x], [x, t.x + t.w], [x + o.w, t.x], [x + o.w, t.x + t.w]]) {
              if (Math.abs(a - bx) < T) { x += bx - a; gx = bx; break; }
            }
          }
          if (gy === null) {
            for (const [a, by] of [[y, t.y], [y, t.y + t.h], [y + o.h, t.y], [y + o.h, t.y + t.h]]) {
              if (Math.abs(a - by) < T) { y += by - a; gy = by; break; }
            }
          }
        }
      }
      o.x = Math.round(Math.max(0, Math.min(1920 - o.w, x)));
      o.y = Math.round(Math.max(0, Math.min(1080 - o.h, y)));
      place(b, o);
      gV.classList.toggle("show", gx !== null);
      gH.classList.toggle("show", gy !== null);
      if (gx !== null) gV.style.left = gx * scaleK + "px";
      if (gy !== null) gH.style.top = gy * scaleK + "px";
      selectBox(o.id);
    }
    function up() {
      b.classList.remove("dragging");
      gV.classList.remove("show");
      gH.classList.remove("show");
      b.removeEventListener("pointermove", move);
      b.removeEventListener("pointerup", up);
      b.removeEventListener("pointercancel", up);
    }
    b.addEventListener("pointermove", move);
    b.addEventListener("pointerup", up);
    b.addEventListener("pointercancel", up);
  }
  screen.addEventListener("pointerdown", (e) => e.target === screen && selectBox(null));
  $("#resetLayout").addEventListener("click", () => {
    CATALOG.forEach((o, i) => Object.assign(o, DEFAULT_POS[i]));
    buildLayout();
  });
  $("#editInGame").addEventListener("click", (e) => {
    const b = e.currentTarget;
    const old = b.innerHTML;
    b.textContent = "Edit mode is on in game";
    setTimeout(() => (b.innerHTML = old), 1800);
  });
  pageInit.layout = () => {
    requestAnimationFrame(buildLayout);
    if (!layoutBuilt) {
      layoutBuilt = true;
      window.addEventListener("resize", () => $(".page.active").dataset.page === "layout" && buildLayout());
    }
  };

  /* ---------- appearance page ---------- */
  const DEFAULT_ACCENT = "#a78bfa";
  const SWATCHES = ["#a78bfa", "#4c8dff", "#22d3ee", "#3ddc84", "#ffb020", "#ff5a5f", "#f472b6"];
  const ACCENT_VARS = ["--accent", "--accent-2", "--accent-grad", "--accent-text", "--row-player"];
  let appearanceBuilt = false;
  const custom = { accent: null, opacity: null };

  function accentText(hex) {
    const n = parseInt(hex.slice(1), 16);
    const l = (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255;
    return l > 0.6 ? "#14100a" : "#ffffff";
  }
  function applyCustom() {
    const root = document.documentElement;
    if (custom.accent) {
      root.style.setProperty("--accent", custom.accent);
      root.style.setProperty("--accent-2", custom.accent);
      root.style.setProperty("--accent-grad", custom.accent);
      root.style.setProperty("--accent-text", accentText(custom.accent));
      root.style.setProperty("--row-player", `color-mix(in srgb, ${custom.accent} 30%, transparent)`);
    } else {
      ACCENT_VARS.forEach((v) => root.style.removeProperty(v));
    }
    // panel opacity: rewrite the alpha of the token's own --panel-bg
    root.style.removeProperty("--panel-bg");
    if (custom.opacity != null) {
      const v = getComputedStyle(root).getPropertyValue("--panel-bg").trim();
      const m = v.match(/rgba?\(([^,]+),([^,]+),([^,)]+)/);
      if (m) root.style.setProperty("--panel-bg", `rgba(${m[1]},${m[2]},${m[3]},${custom.opacity / 100})`);
    }
    $("#vtOpacity").textContent = $("#tOpacity").value + "%";
    renderSwatches();
    HUD.restyle(); // canvas overlays (track map, radar, …) re-read the accent
  }
  function renderSwatches() {
    const cur = custom.accent || DEFAULT_ACCENT;
    $("#swatches").innerHTML =
      SWATCHES.map((c) => `<button class="swatch${c === cur ? " on" : ""}" style="background:${c}" data-c="${c}" aria-label="Accent ${c}"></button>`).join("") +
      `<input type="color" class="swatch-input" id="accentPick" value="${cur}" aria-label="Custom accent color">`;
  }
  $("#swatches").addEventListener("click", (e) => {
    const b = e.target.closest(".swatch");
    if (!b) return;
    custom.accent = b.dataset.c === DEFAULT_ACCENT ? null : b.dataset.c;
    applyCustom();
  });
  $("#swatches").addEventListener("input", (e) => {
    if (e.target.id !== "accentPick") return;
    custom.accent = e.target.value;
    applyCustom();
  });
  $("#tOpacity").addEventListener("input", (e) => {
    custom.opacity = +e.target.value;
    applyCustom();
  });
  wireSwitch($("#blurToggle"), (on) => $("#lookPrev").classList.toggle("glass-on", on));

  pageInit.appearance = () => {
    if (appearanceBuilt) return;
    appearanceBuilt = true;
    const stage = $("#lookPrev .stage");
    mountInto(stage, "relative");
    mountInto(stage, "delta");
    rangeFill($("#tOpacity"));
    $("#vtOpacity").textContent = $("#tOpacity").value + "%";
    renderSwatches();
  };

  /* ---------- profiles page ---------- */
  const PROFILES = [
    ["Race", "8 overlays", ["standings", "relative", "delta", "inputs", "fuel", "radar", "session", "tyres"]],
    ["Endurance", "10 overlays", ["standings", "relative", "delta", "fuel", "radar", "session", "tyres", "weather", "pit", "trackmap"]],
    ["Qualifying", "4 overlays", ["delta", "inputs", "session", "trackmap"]],
  ];
  let profilesBuilt = false;
  pageInit.profiles = () => {
    if (profilesBuilt) return;
    profilesBuilt = true;
    const cur = $("#profileSel").value;
    $("#profGrid").innerHTML = PROFILES.map(
      ([n, meta, ids]) => `<div class="card prof${n === cur ? " active" : ""}">
        <div class="top"><span class="nm">${n}</span><span class="spacer"></span>${n === cur ? '<span class="pill">In use</span>' : ""}</div>
        <div class="mini">${ids
          .map((id) => {
            const o = byId[id];
            return `<i style="left:${(o.x / 1920) * 100}%;top:${(o.y / 1080) * 100}%;width:${(o.w / 1920) * 100}%;height:${(o.h / 1080) * 100}%"></i>`;
          })
          .join("")}</div>
        <div class="meta">${meta}</div>
        <div class="actions">${n === cur ? "" : '<button class="btn">Use</button>'}<button class="btn">Duplicate</button><button class="btn">Export</button></div>
      </div>`
    ).join("");
  };

  /* ---------- fit the window into small viewports ---------- */
  function fitWindow() {
    const k = Math.min(1, (innerWidth - 24) / 1200, (innerHeight - 24) / 780);
    $("#win").style.transform = k < 1 ? `scale(${k})` : "";
  }
  addEventListener("resize", fitWindow);
  fitWindow();

  /* ---------- boot ---------- */
  renderList();
  loadOverlays().then(() => {
    HUD.start();
    select(selected);
    go(q.get("page") || "overlays");
    if (document.fonts) document.fonts.ready.then(fitPreview);
  });
  // token changes (accent, opacity) can change sizes: refit the preview
  HUD.onTheme(() => requestAnimationFrame(fitPreview));
})();
