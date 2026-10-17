/* Pit / strategy: pit window on a race timeline, mandatory stops, pit loss,
   estimated class position on rejoin, fuel to add and stint length. */
import "./pit.css";
import { HUD } from "../hud";
import { F } from "../fmt";
import type { ViewCar, ViewState } from "../view";

HUD.register("pit", {
  title: "Pit strategy",
  group: "car",
  w: 290,
  hz: 4,
  mount(root) {
    const { h } = HUD;
    const REQUIRED = 1;
    const STATIONARY = 28, LANE = 24, LOSS = STATIONARY + LANE;

    const stintEl = h("span.num");
    const head = h("div.hud-head", null, h("span.title", { text: "Pit strategy" }), h("span.spacer"), stintEl);

    // window status + race timeline (0 … duration) with the window band and a "now" marker
    const status = h("span.pit-status");
    const countdown = h("span.pit-count.num");
    const band = h("i.pit-band");
    const done = h("i.pit-done");
    const now = h("i.pit-now");
    const timeline = h("div.pit-timeline", null, done, band, now);
    const openLbl = h("span.num"), closeLbl = h("span.num");
    const scale = h("div.pit-scale", null, openLbl, closeLbl);
    const winBox = h("div.pit-window", null,
      h("div.pit-window-top", null, status, countdown),
      timeline,
      scale);

    const stopsEl = h("span.num");
    const lossEl = h("span.num");
    const rejoinEl = h("span.num");
    const rejoinSub = h("span.pit-sub.num");
    const fuelEl = h("span.num");
    const kv = (k: string, ...v: HTMLElement[]) => h("div.kv.pit-kv", null, h("span.dim", { text: k }), h("span.pit-val", null, ...v));
    const list = h("div.pit-list", null,
      kv("Mandatory stops", stopsEl),
      kv("Pit loss", lossEl),
      kv("Rejoin", rejoinEl, rejoinSub),
      kv("Fuel to add", fuelEl));

    root.append(h("div.hud-panel", null, head, h("div.hud-body", null, winBox, list)));

    // window edges rarely change: place their labels only when they do
    let placed = "";

    return {
      update(s: ViewState) {
        const ses = s.session, p = s.player, me = p.car;
        const pw = ses.pitWindow, d = ses.duration, el = ses.elapsed;

        if (!pw || !(d > 0)) {
          HUD.data(winBox, "state", "none");
          HUD.text(status, "No pit window");
          HUD.text(countdown, "");
        } else {
          const { open, close } = pw;
          const key = open + "/" + close + "/" + d;
          if (placed !== key) {
            placed = key;
            openLbl.style.left = ((open / d) * 100).toFixed(2) + "%";
            closeLbl.style.left = ((close / d) * 100).toFixed(2) + "%";
            HUD.text(openLbl, F.clock(open));
            HUD.text(closeLbl, F.clock(close));
            band.style.transform = "translateX(" + ((open / d) * 100).toFixed(2) + "%) scaleX(" + ((close - open) / d).toFixed(4) + ")";
          }
          let state: string, label: string;
          if (el < open) { state = "pending"; label = "Opens in " + F.clock(open - el); }
          else if (el <= close) { state = "open"; label = "Closes in " + F.clock(close - el); }
          else { state = "closed"; label = "Window passed"; }
          HUD.data(winBox, "state", state);
          HUD.text(status, state === "open" ? "Window open" : "Window closed");
          HUD.text(countdown, label);
          done.style.transform = "scaleX(" + Math.min(1, el / d).toFixed(4) + ")";
          now.style.transform = "translateX(" + ((Math.min(el, d) / d) * 100).toFixed(2) + "cqw)";
        }

        HUD.text(stopsEl, p.pitStops + " / " + REQUIRED);
        HUD.cls(stopsEl, "gain", p.pitStops >= REQUIRED);
        HUD.text(lossEl, "~" + LOSS + " s");

        // class position after a stop now: every class car behind within the pit loss passes us
        const myGap = me.gapLeader;
        let passed = 0, nextAhead: ViewCar | null = null, nextGap = Infinity;
        for (let i = 0; i < s.cars.length; i++) {
          const c = s.cars[i];
          if (c === me || c.cls !== me.cls || c.inPit) continue;
          const behind = c.gapLeader - myGap; // seconds the car is behind us (class-relative)
          if (behind > 0 && behind < LOSS && c.lapsDown === me.lapsDown) {
            passed++;
            const ahead = LOSS - behind; // how far ahead of us that car will be on rejoin
            if (ahead < nextGap) { nextGap = ahead; nextAhead = c; }
          }
        }
        HUD.text(rejoinEl, "P" + (me.classPos + passed));
        HUD.cls(rejoinEl, "loss", passed > 0);
        HUD.text(rejoinSub, nextAhead ? F.fixed(nextGap, 1) + " s behind #" + nextAhead.number : "clear air");

        HUD.text(fuelEl, F.fixed(p.fuel.toAdd, 1) + " L");
        HUD.text(stintEl, "Stint " + p.stintLaps + (p.stintLaps === 1 ? " lap" : " laps"));
      },
    };
  },
});
