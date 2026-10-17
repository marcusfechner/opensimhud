/* Formatters. Pure functions, no allocation beyond the returned string. */

const pad2 = (n: number) => (n < 10 ? "0" + n : "" + n);
const pad3 = (n: number) => (n < 10 ? "00" + n : n < 100 ? "0" + n : "" + n);

export const F = {
  /** seconds → "1:47.382" (or "-:--.---" when missing) */
  lap(sec: number): string {
    if (!(sec > 0) || !isFinite(sec)) return "-:--.---";
    const ms = Math.round(sec * 1000);
    const m = Math.floor(ms / 60000);
    const s = Math.floor((ms % 60000) / 1000);
    return m + ":" + pad2(s) + "." + pad3(ms % 1000);
  },
  /** gap in seconds → "+1.2", laps → "+1L" */
  gap(sec: number, laps?: number): string {
    if (laps !== undefined && laps >= 1) return "+" + laps + "L";
    if (laps !== undefined && laps <= -1) return laps + "L";
    if (!isFinite(sec)) return "—";
    return (sec >= 0 ? "+" : "-") + Math.abs(sec).toFixed(sec >= 100 || sec <= -100 ? 0 : 1);
  },
  /** relative gap with 1 decimal, sign kept */
  rel(sec: number): string {
    const r = Math.round(sec * 10) / 10;
    return (r > 0 ? "+" : r < 0 ? "-" : "") + Math.abs(r).toFixed(1);
  },
  /** delta seconds → "-0.12" / "+0.45" / "±0.00" */
  delta(sec: number, digits = 2): string {
    if (!isFinite(sec)) return "—";
    const k = Math.pow(10, digits), r = Math.round(sec * k) / k;
    return (r > 0 ? "+" : r < 0 ? "-" : "±") + Math.abs(r).toFixed(digits);
  },
  /** seconds → "42:07" or "1:02:07" */
  clock(sec: number): string {
    sec = Math.max(0, Math.floor(sec));
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    return h > 0 ? h + ":" + pad2(m) + ":" + pad2(s) : pad2(m) + ":" + pad2(s);
  },
  fixed(v: number, d = 1): string { return isFinite(v) ? v.toFixed(d) : "—"; },
  int(v: number): string { return isFinite(v) ? "" + Math.round(v) : "—"; },
  pct(v: number): string { return Math.round(v * 100) + "%"; },
  pos(p: number): string { return "P" + p; },
  gear(g: number): string { return g < 0 ? "R" : g === 0 ? "N" : "" + g; },
};
