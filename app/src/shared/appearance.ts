/* Apply the user's appearance (accent, panel opacity) and per-overlay look to the HUD design
   tokens. Used by the overlay windows and the settings window's previews. (No blur: over a
   game the transparent overlay window has nothing behind its panels to blur.) */

import type { Appearance } from "../types/config/Appearance";
import type { OverlayConf } from "../types/config/OverlayConf";

const ACCENT_VARS = ["--accent", "--accent-2", "--accent-grad", "--accent-text", "--row-player"];
const FONT_VARS = ["--fs-xs", "--fs-sm", "--fs-md", "--fs-lg", "--fs-xl", "--fs-xxl"];

/** Dark or light text, whichever reads on the accent. */
function textOn(hex: string) {
  const n = parseInt(hex.slice(1), 16);
  const l = (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255;
  return l > 0.6 ? "#14100a" : "#ffffff";
}

export function applyAppearance(a: Appearance, root: HTMLElement = document.documentElement) {
  if (a.accent) {
    root.style.setProperty("--accent", a.accent);
    root.style.setProperty("--accent-2", a.accent);
    root.style.setProperty("--accent-grad", a.accent);
    root.style.setProperty("--accent-text", textOn(a.accent));
    root.style.setProperty("--row-player", `color-mix(in srgb, ${a.accent} 30%, transparent)`);
  } else {
    for (const v of ACCENT_VARS) root.style.removeProperty(v);
  }
  // panel opacity: rewrite the alpha of the theme's own --panel-bg
  root.style.removeProperty("--panel-bg");
  if (a.panelOpacity != null) {
    const v = getComputedStyle(root).getPropertyValue("--panel-bg").trim();
    const m = v.match(/rgba?\(([^,]+),([^,]+),([^,)]+)/);
    if (m) root.style.setProperty("--panel-bg", `rgba(${m[1]},${m[2]},${m[3]},${a.panelOpacity / 100})`);
  }
}

/** Opacity and font size of one overlay's host element (scale is placement's business). */
export function applyOverlayLook(el: HTMLElement, o: Pick<OverlayConf, "opacity" | "font">) {
  el.style.opacity = o.opacity === 100 ? "" : String(o.opacity / 100);
  for (const v of FONT_VARS) el.style.removeProperty(v);
  if (o.font !== 100) {
    const cs = getComputedStyle(el);
    for (const v of FONT_VARS) {
      const px = parseFloat(cs.getPropertyValue(v));
      if (px) el.style.setProperty(v, (px * o.font) / 100 + "px");
    }
  }
}
