/* Overlay window: one transparent, click-through window per monitor hosting every overlay.

   Query parameters (useful in a browser during development):
     ?layout=race|practice   which built-in layout to show (default race)
     ?only=relative          show a single overlay at the top left
     ?bg=scene               draw a neutral backdrop instead of transparency
     ?skip=10                start the demo replay 10 s in */

import "./styles/tokens.css";
import "./styles/base.css";
import "./host.css";
import "./overlays";
import { HUD } from "./hud";
import { LAYOUTS, type Anchor } from "./layouts";
import { connect } from "./source";
import { apply, createView } from "./view";

const q = new URLSearchParams(location.search);
const inTauri = "__TAURI_INTERNALS__" in window;
document.body.dataset.bg = q.get("bg") || (inTauri ? "transparent" : "scene");

const stage = document.getElementById("stage")!;
const status = document.getElementById("status")!;
const view = createView();

function layout(): Record<string, Anchor> {
  const only = q.get("only");
  if (only) return { [only]: { left: 24, top: 24 } };
  return LAYOUTS[q.get("layout") || "race"] || LAYOUTS.race;
}

/** Scale the 1080p-reference stage to the window; anchors keep working on any aspect ratio. */
function fit() {
  const k = innerHeight / 1080;
  stage.style.width = innerWidth / k + "px";
  stage.style.transform = `scale(${k})`;
}

/** WKWebView can run this before the stylesheets apply; canvas overlays need the tokens. */
async function stylesReady() {
  for (let i = 0; i < 120 && !getComputedStyle(document.documentElement).getPropertyValue("--accent").trim(); i++) {
    await new Promise((r) => requestAnimationFrame(r));
  }
}

async function mountAll() {
  await stylesReady();
  for (const [id, anchor] of Object.entries(layout())) {
    if (!HUD.overlays[id]) continue;
    const host = HUD.h("div.ov-host");
    for (const k in anchor) host.style[k as keyof Anchor] = anchor[k as keyof Anchor] + "px";
    stage.append(host);
    try {
      HUD.mount(id, host, view);
    } catch (e) {
      console.error("could not mount", id, e);
    }
  }
  HUD.start(() => view);
}

fit();
addEventListener("resize", fit);
status.textContent = "Waiting for telemetry…";
let mounted = false;
connect((frame) => {
  apply(view, frame);
  if (!mounted && view.ready) {
    mounted = true;
    status.textContent = "";
    void mountAll();
  }
}).catch((e) => (status.textContent = String(e.message || e)));
