/* The overlay catalog's sizes and the default profiles, from config/defaults.json (the app's
   Rust side builds its defaults from the same file). */

import defaults from "../config/defaults.json";
import type { Config } from "../types/config/Config";
import type { OverlayConf } from "../types/config/OverlayConf";
import type { Profile } from "../types/config/Profile";
import { anchorAt } from "./placement";

export interface Box {
  w: number;
  h: number;
  x: number;
  y: number;
}

/** Every overlay's usual size and fallback position on the 1920×1080 reference screen. */
export const SIZES: Record<string, Box> = defaults.overlays;

const overlayConf = (on: boolean, x: number, y: number, w: number): OverlayConf => ({
  on, x, y, anchor: anchorAt(x, w), display: 0, opacity: 100, scale: 100, font: 100, w: null, h: null, options: {},
});

/** A built-in profile by name, or every overlay off at its fallback position. */
export function defaultProfile(name: string): Profile {
  const d = defaults.profiles.find((p) => p.name === name);
  const at = (d?.overlays ?? {}) as Record<string, number[]>;
  const overlays: Record<string, OverlayConf> = {};
  for (const [id, s] of Object.entries(SIZES)) {
    const p = at[id];
    overlays[id] = overlayConf(!!p, p ? p[0] : s.x, p ? p[1] : s.y, s.w);
  }
  return { name, overlays };
}

export const BUILT_IN = defaults.profiles.map((p) => p.name);

export function defaultConfig(): Config {
  return {
    version: 1,
    profile: BUILT_IN[0],
    profiles: BUILT_IN.map(defaultProfile),
    autoSwitch: { ...defaults.autoSwitch },
    appearance: { accent: null, panelOpacity: null },
    demo: false,
    game: "auto",
    mediaButtons: { playPause: null, next: null, previous: null },
  };
}

/** The profile in use. */
export function active(c: Config): Profile {
  return c.profiles.find((p) => p.name === c.profile) ?? c.profiles[0];
}
