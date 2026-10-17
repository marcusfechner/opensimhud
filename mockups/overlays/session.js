/* Session: what session, how long is left, where am I, and the current flag. */
HUD.register("session", {
  title: "Session",
  group: "awareness",
  w: 380,
  hz: 4,
  mount(root, state) {
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

    const classCount = {};
    for (const c of state.cars) classCount[c.cls] = (classCount[c.cls] || 0) + 1;

    const pad2 = (v) => (v < 10 ? "0" : "") + v;
    let lastFlag = "";

    return {
      update(s) {
        const ses = s.session, me = s.player.car;
        HUD.text(name, ses.name);
        HUD.text(track, ses.track.name);
        HUD.text(left, F.clock(ses.timeRemaining));
        const best = s.player.lap.best || 110;
        const lapsLeft = ses.lapRemaining > 0 ? Math.ceil(ses.lapRemaining) : Math.ceil(ses.timeRemaining / best) + 1;
        HUD.text(laps, "≈ " + lapsLeft + " laps left");
        HUD.text(pos, "P" + me.pos);
        HUD.text(posOf, "/" + s.cars.length);
        HUD.data(clsBadge, "cls", me.cls);
        HUD.text(clsBadge, me.cls);
        HUD.text(clsPos, "P" + me.classPos + "/" + classCount[me.cls]);
        const t = Math.floor(ses.timeOfDay) % 86400;
        HUD.text(tod, pad2(Math.floor(t / 3600)) + ":" + pad2(Math.floor((t % 3600) / 60)));

        const f = ses.flag;
        const label =
          f === "yellow" ? "Yellow flag in sector " + ses.yellowSector :
          f === "blue" ? "Blue flag, faster car behind" :
          f === "white" ? "White flag, slow car ahead" :
          f === "chequered" ? "Chequered flag" : "Green, track clear";
        HUD.text(flagText, label);
        HUD.data(panel, "flag", f);
        if (f !== lastFlag) {
          if (lastFlag && f !== "green") HUD.flash(flag);
          lastFlag = f;
        }
      },
    };
  },
});
