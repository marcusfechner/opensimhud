/** Temperature windows in °C. See docs/tyre-temperatures.md for sources and estimates. */
export interface TemperatureWindow { low: number; high: number }

const DEFAULT: TemperatureWindow = { low: 70, high: 105 };
const ACC_SLICK: TemperatureWindow = { low: 80, high: 105 };
const ACC_WET: TemperatureWindow = { low: 40, high: 65 };
const EVO: Record<string, TemperatureWindow> = {
  vintage: { low: 50, high: 75 }, eco: { low: 40, high: 70 },
  road: { low: 35, high: 70 }, supercar: { low: 60, high: 95 },
  hypercar: { low: 60, high: 105 }, slick: { low: 70, high: 105 },
  wet: { low: 30, high: 80 },
};

/** Match whole words/parenthesized codes, not substrings in unrelated compound names. */
export function tyreTemperatureWindow(game: string, compound: string, model = ""): TemperatureWindow {
  const name = compound.toLowerCase();
  if (game === "acc") return /\bwet\b/.test(name) ? ACC_WET : ACC_SLICK;
  if (game !== "evo") return DEFAULT;
  if (name === "slick soft (s)") return { low: 95, high: 110 };
  if (name === "slick medium (m)") return { low: 100, high: 125 };
  if (name === "slick hard (h)") return { low: 105, high: 140 };
  if (/\b(intermediate|inter|wet|rain|w|i)\b/.test(name)) {
    return model === "Ferrari SF25" ? { low: 95, high: 110 } : EVO.wet;
  }
  const types: [RegExp, string][] = [
    [/\b(vintage|v)\b/, "vintage"], [/\b(eco|e)\b/, "eco"],
    [/\b(road|street|rd)\b/, "road"], [/\b(supercar|super car|sc|sport)\b/, "supercar"],
    [/\b(hypercar|hyper car|hc)\b/, "hypercar"], [/\b(wet|rain|w)\b/, "wet"],
    [/\b(slick|slicks|s)\b/, "slick"],
  ];
  return EVO[types.find(([pattern]) => pattern.test(name))?.[1] ?? ""] ?? DEFAULT;
}

/** Five colour stops with a green plateau across the operating window. */
export function tyreTemperaturePosition(t: number, window: TemperatureWindow): number {
  const margin = 20;
  if (t <= window.low) return Math.max(0, 2 * (t - (window.low - margin)) / margin);
  if (t <= window.high) return 2;
  return Math.min(4, 2 + 2 * (t - window.high) / margin);
}
