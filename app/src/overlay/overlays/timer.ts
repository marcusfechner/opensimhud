/* Session timer: time and laps remaining, session progress, time of day. */
import "./timer.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

HUD.register("timer", {
  title: "Session timer",
  group: "awareness",
  w: 240,
  hz: 4,
  mount(root) {
    const { h } = HUD;

    const name = h("span.tm2-name");
    const tod = h("span.tm2-tod.num");
    const left = h("span.tm2-left.num");
    const laps = h("span.tm2-laps.num");
    const fill = h("i");
    const elapsed = h("span.num");
    const total = h("span.num");

    root.append(h("div.hud-panel.tm2-panel", null,
      h("div.tm2-top", null, name, h("span.spacer"), tod),
      h("div.tm2-main", null, left, laps),
      h("div.tm2-prog", null,
        h("div.bar.tm2-bar", null, fill),
        h("div.tm2-scale", null, elapsed, total))));

    const pad2 = (v: number) => (v < 10 ? "0" : "") + v;

    return {
      update(s: ViewState) {
        const ses = s.session;
        HUD.text(name, ses.name);
        const t = Math.floor(ses.timeOfDay) % 86400;
        HUD.text(tod, pad2(Math.floor(t / 3600)) + ":" + pad2(Math.floor((t % 3600) / 60)));
        HUD.text(left, F.clock(ses.timeRemaining));
        const lapsLeft = Math.ceil(ses.lapRemaining || 0);
        HUD.text(laps, lapsLeft <= 0 ? "" : lapsLeft === 1 ? "Last lap" : "≈ " + lapsLeft + " laps");
        fill.style.transform = "scaleX(" + (ses.duration > 0 ? Math.min(1, ses.elapsed / ses.duration) : 0) + ")";
        HUD.text(elapsed, F.clock(ses.elapsed));
        HUD.text(total, ses.duration > 0 ? F.clock(ses.duration) : "");
      },
    };
  },
});
