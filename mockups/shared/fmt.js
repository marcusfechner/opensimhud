/* Formatters. Pure functions, no allocation beyond the returned string. */
(function () {
  const pad2 = (n) => (n < 10 ? "0" + n : "" + n);
  const pad3 = (n) => (n < 10 ? "00" + n : n < 100 ? "0" + n : "" + n);

  const F = {
    /** seconds → "1:47.382" (or "--:--.---" when missing) */
    lap(sec) {
      if (!(sec > 0) || !isFinite(sec)) return "-:--.---";
      const ms = Math.round(sec * 1000);
      const m = Math.floor(ms / 60000);
      const s = Math.floor((ms % 60000) / 1000);
      return m + ":" + pad2(s) + "." + pad3(ms % 1000);
    },
    /** gap in seconds → "+1.234", laps → "+1L" */
    gap(sec, laps) {
      if (laps >= 1) return "+" + laps + "L";
      if (laps <= -1) return laps + "L";
      if (!isFinite(sec)) return "—";
      return (sec >= 0 ? "+" : "-") + Math.abs(sec).toFixed(sec >= 100 || sec <= -100 ? 0 : 1);
    },
    /** relative gap with 1 decimal, sign kept */
    rel(sec) {
      const r = Math.round(sec * 10) / 10;
      return (r > 0 ? "+" : r < 0 ? "-" : "") + Math.abs(r).toFixed(1);
    },
    /** delta seconds → "-0.123" / "+0.45" */
    delta(sec, digits = 2) {
      if (!isFinite(sec)) return "—";
      const k = Math.pow(10, digits), r = Math.round(sec * k) / k;
      return (r > 0 ? "+" : r < 0 ? "-" : "±") + Math.abs(r).toFixed(digits);
    },
    /** seconds → "42:07" or "1:02:07" */
    clock(sec) {
      sec = Math.max(0, Math.floor(sec));
      const h = Math.floor(sec / 3600);
      const m = Math.floor((sec % 3600) / 60);
      const s = sec % 60;
      return h > 0 ? h + ":" + pad2(m) + ":" + pad2(s) : pad2(m) + ":" + pad2(s);
    },
    fixed(v, d = 1) { return isFinite(v) ? v.toFixed(d) : "—"; },
    int(v) { return isFinite(v) ? "" + Math.round(v) : "—"; },
    pct(v) { return Math.round(v * 100) + "%"; },
    /** ordinal-free position "P7" */
    pos(p) { return "P" + p; },
    gear(g) { return g < 0 ? "R" : g === 0 ? "N" : "" + g; },
  };

  window.F = F;
})();
