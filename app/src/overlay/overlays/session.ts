/* Session: what session, how long is left, where am I, and the current flag. The clock in
   the corner shows the time of day in the game, or on this PC. */
import "./session.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewState } from "../view";

const LABEL: Record<string, string> = {
  none: "No flag",
  green: "Green, track clear",
  blue: "Blue flag, faster car behind",
  white: "White flag, slow car ahead",
  blackwhite: "Track limits warning",
  black: "Black flag, return to the pits",
  chequered: "Chequered flag",
};

HUD.register("session", {
  title: "Session",
  group: "awareness",
  w: 380,
  hz: 4,
  options: [
    { key: "clock", label: "Clock", type: "choice", default: "game", choices: [["game", "In the game"], ["pc", "On this PC"]] },
  ],
  mount(root, state, opts) {
    const pcClock = opts.options.clock === "pc";
    const { h } = HUD;

    const name = h("span.ses-name");
    const track = h("span.ses-track");
    const tod = h("span.ses-tod.num");
    const left = h("span.ses-left.num");
    const laps = h("span.ses-laps.num");
    const pos = h("span.ses-pos.num");
    const posOf = h("span.ses-of.num");
    const clsBadge = h("span.badge.cls");
    const clsPos = h("span.ses-clspos.num");

    const flagText = h("span.flag-text");
    const flag = h("div.ses-flag.flash-host", null, h("span.flag-swatch"), flagText);

    const panel = h("div.hud-panel.ses-panel", null,
      h("div.ses-main", null,
        h("div.ses-time", null,
          h("div.ses-line", null, name, track),
          h("div.ses-line", null, left, laps)),
        h("div.ses-place", null,
          h("div.ses-line.big", null, pos, posOf),
          h("div.ses-line", null, clsBadge, clsPos))),
      h("div.ses-foot", null, flag, tod));
    root.append(panel);

    // cars per class; recounted when the entry list changes size
    const classCount: Record<string, number> = {};
    let counted = -1;
    const count = (s: ViewState) => {
      if (counted === s.cars.length) return;
      counted = s.cars.length;
      for (const k in classCount) delete classCount[k];
      for (const c of s.cars) classCount[c.cls] = (classCount[c.cls] || 0) + 1;
    };
    count(state);

    const pad2 = (v: number) => (v < 10 ? "0" : "") + v;
    let lastFlag = "";

    return {
      update(s: ViewState) {
        count(s);
        const ses = s.session, me = s.player.car;
        HUD.text(name, ses.name);
        HUD.text(track, ses.track.name);
        // no time limit (duration 0): count up instead
        const timed = ses.duration > 0;
        HUD.text(left, F.clock(timed ? ses.timeRemaining : ses.elapsed));
        const best = s.player.lap.best || 110;
        const lapsLeft = ses.lapRemaining > 0 ? Math.ceil(ses.lapRemaining) : Math.ceil(ses.timeRemaining / best) + 1;
        HUD.text(laps, ses.kind === "race" && timed ? "≈ " + lapsLeft + " laps left" : "");
        HUD.text(pos, "P" + me.pos);
        HUD.text(posOf, "/" + s.cars.length);
        HUD.data(clsBadge, "cls", me.cls);
        HUD.text(clsBadge, me.cls);
        HUD.text(clsPos, "P" + me.classPos + "/" + (classCount[me.cls] || 0));
        if (pcClock) {
          const d = new Date();
          HUD.text(tod, pad2(d.getHours()) + ":" + pad2(d.getMinutes()));
        } else {
          const t = Math.floor(ses.timeOfDay) % 86400;
          HUD.text(tod, pad2(Math.floor(t / 3600)) + ":" + pad2(Math.floor((t % 3600) / 60)));
        }

        const f = ses.flag;
        HUD.text(flagText,
          f === "yellow" ? (ses.yellowSector ? "Yellow flag in sector " + ses.yellowSector : "Yellow flag")
          // in a timed race the white flag comes out once the clock has run out
          : f === "white" && timed && ses.timeRemaining <= 0 ? "White flag, last lap"
          : LABEL[f] || "");
        HUD.data(panel, "flag", f);
        if (f !== lastFlag) {
          if (lastFlag && f !== "green" && f !== "none") HUD.flash(flag);
          lastFlag = f;
        }
      },
    };
  },
});
