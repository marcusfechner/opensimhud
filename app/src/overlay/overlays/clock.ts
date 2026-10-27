/* Clock: the time on this PC, so you know how late it got. */
import "./clock.css";
import { HUD } from "../hud";

HUD.register("clock", {
  title: "Clock",
  group: "awareness",
  w: 130,
  hz: 2,
  options: [
    { key: "format", label: "Format", type: "choice", default: "24", choices: [["24", "24 hour"], ["12", "12 hour"]] },
    { key: "seconds", label: "Seconds", type: "choice", default: "off", choices: [["off", "Hide"], ["on", "Show"]] },
  ],
  mount(root, _state, opts) {
    const { h } = HUD;
    const h12 = opts.options.format === "12", secs = opts.options.seconds === "on";
    const time = h("span.ck-time.num");
    const ampm = h("span.ck-ampm");
    root.append(h("div.hud-panel.ck-panel", null, time, ampm));
    const pad2 = (v: number) => (v < 10 ? "0" : "") + v;

    return {
      update() {
        const d = new Date();
        const hr = d.getHours();
        const shown = h12 ? hr % 12 || 12 : hr;
        HUD.text(time, (h12 ? "" + shown : pad2(shown)) + ":" + pad2(d.getMinutes()) + (secs ? ":" + pad2(d.getSeconds()) : ""));
        HUD.text(ampm, h12 ? (hr < 12 ? "AM" : "PM") : "");
      },
    };
  },
});
