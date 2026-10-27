/* Where an overlay sits on a screen of any aspect ratio.

   Positions are stored for a 1920 × 1080 reference screen. Screens are scaled to 1080 high,
   so a wider screen is wider than 1920 in reference px. Each overlay keeps to a side: left
   ones stay at the left, right ones move with the right edge, centered ones stay centered.
   The side is stored with the position, so an overlay can sit anywhere on any screen. */

import type { Anchor } from "../types/config/Anchor";
import type { OverlayConf } from "../types/config/OverlayConf";

export const REF_W = 1920;
export const REF_H = 1080;

/** The side an overlay at `x`, `w` wide, is on: the third of `W` its middle is in. */
export function anchorAt(x: number, w: number, W = REF_W): Anchor {
  const cx = x + w / 2;
  return cx < W / 3 ? "left" : cx > (W * 2) / 3 ? "right" : "center";
}

/** How far right of its stored `x` an overlay with this anchor sits on a screen `W` wide. */
function offset(anchor: Anchor, W: number) {
  const extra = W - REF_W;
  return anchor === "left" ? 0 : anchor === "right" ? extra : extra / 2;
}

/** Left edge on a screen `W` reference px wide, kept on screen. */
export function screenX(o: Pick<OverlayConf, "x" | "anchor">, w: number, W: number): number {
  const x = o.x + offset(o.anchor ?? anchorAt(o.x, w), W);
  return Math.max(0, Math.min(Math.max(0, W - w), x));
}

/** The stored position for a left edge at `x` on a screen `W` wide. */
export function fromScreen(x: number, w: number, W: number): { x: number; anchor: Anchor } {
  const anchor = anchorAt(x, w, W);
  return { x: Math.round(x - offset(anchor, W)), anchor };
}
