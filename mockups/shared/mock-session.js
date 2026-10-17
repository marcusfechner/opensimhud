/* Fake ACC race session producing the normalized telemetry model.
   Everything is updated in place on HUD.state (no per-frame allocation).

   HUD.state = {
     t,                       // sim seconds since page load
     session: { type, name, track, duration, elapsed, timeRemaining, lapsRemaining,
                flag, yellowSector, pitWindow:{open,close}, timeOfDay, best:{GT3:{time,car},…} },
     weather: { air, track, rainNow, rain10, rain30, wetness, grip, windSpeed, windDir, clouds },
     player:  { car, speed, gear, rpm, maxRpm, shiftRpm, throttle, brake, clutch, steer, absActive, tcActive,
                tc, tc2, abs, bb, map, fuel:{liters,capacity,perLap,lapsLeft,toFinish,toAdd,lastLapUse},
                tyres:[{name,pressure,core,i,m,o,wear,brake}×4], lap:{current,last,best,delta,deltaLast,predicted,
                sector,sectors:[…],bestSectors:[…],valid}, stintLaps, pitStops,
                log:[{lap,time,sectors:[3],valid,fuel,pit}] },
     cars:  [ { idx, number, driver, short, team, model, cls, cat, nat, pace, dist, laps, pct, pos, classPos, startPos,
                lastLap, bestLap, lapTimes:[…], pitLaps:[lap#…], outLap, penalty,
                gapLeader, gapAhead, lapsDown, relGap, relLaps, relMeters, inPit, pitStops,
                isPlayer, x, y, heading, lateral } ],
     order: [cars sorted by overall position],
     classes: ["GT3","GT4"],
   } */
(function () {
  const HUD = (window.HUD = window.HUD || {});

  /* ---------- seeded random ---------- */
  let seed = 46;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;
  const pick = (arr) => arr[Math.floor(rnd() * arr.length)];
  const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
  const lerp = (a, b, t) => a + (b - a) * t;

  /* ---------- track: Monza-like outline, Catmull-Rom → equal arc-length samples ---------- */
  const RAW = [
    [820, 800], [600, 800], [380, 800], [300, 800], [275, 798], [262, 782], [248, 776], [225, 772],
    [170, 750], [130, 700], [115, 630], [118, 560], [125, 500], [115, 485], [110, 470], [122, 455],
    [135, 420], [150, 360], [175, 320], [210, 305], [260, 300], [300, 310], [330, 335], [420, 390],
    [520, 440], [560, 455], [580, 475], [605, 478], [628, 462], [655, 470], [760, 490], [880, 500],
    [960, 510], [1030, 540], [1065, 620], [1055, 710], [1000, 775], [920, 800],
  ];
  const TRACK_LEN = 5793;
  const N = 1200;

  function buildTrack() {
    // dense Catmull-Rom sampling
    const dense = [];
    const n = RAW.length;
    for (let i = 0; i < n; i++) {
      const p0 = RAW[(i - 1 + n) % n], p1 = RAW[i], p2 = RAW[(i + 1) % n], p3 = RAW[(i + 2) % n];
      for (let k = 0; k < 40; k++) {
        const t = k / 40, t2 = t * t, t3 = t2 * t;
        const f = (a, b, c, d) => 0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
        dense.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
      }
    }
    // cumulative length
    const cum = [0];
    for (let i = 1; i <= dense.length; i++) {
      const a = dense[i - 1], b = dense[i % dense.length];
      cum.push(cum[i - 1] + Math.hypot(b[0] - a[0], b[1] - a[1]));
    }
    const rawLen = cum[cum.length - 1];
    const scale = TRACK_LEN / rawLen;
    // resample at equal arc length
    const x = new Float32Array(N), y = new Float32Array(N);
    let j = 0;
    for (let i = 0; i < N; i++) {
      const target = (i / N) * rawLen;
      while (cum[j + 1] < target) j++;
      const a = dense[j], b = dense[(j + 1) % dense.length];
      const t = (target - cum[j]) / (cum[j + 1] - cum[j] || 1);
      x[i] = lerp(a[0], b[0], t) * scale;
      y[i] = lerp(a[1], b[1], t) * scale;
    }
    // heading + signed curvature (smoothed)
    const heading = new Float32Array(N), curv = new Float32Array(N);
    const ds = TRACK_LEN / N;
    for (let i = 0; i < N; i++) {
      const b = (i + 1) % N, a = (i - 1 + N) % N;
      heading[i] = Math.atan2(y[b] - y[a], x[b] - x[a]);
    }
    const raw = new Float32Array(N);
    for (let i = 0; i < N; i++) {
      let d = heading[(i + 1) % N] - heading[i];
      while (d > Math.PI) d -= 2 * Math.PI;
      while (d < -Math.PI) d += 2 * Math.PI;
      raw[i] = d / ds;
    }
    const W = 6;
    for (let i = 0; i < N; i++) {
      let s = 0;
      for (let k = -W; k <= W; k++) s += raw[(i + k + N) % N];
      curv[i] = s / (2 * W + 1);
    }
    return { name: "Monza", country: "Italy", length: TRACK_LEN, n: N, x, y, heading, curv, sectors: [0, 0.345, 0.69] };
  }

  /* ---------- reference speed profile (player car) ---------- */
  function buildProfile(track) {
    const n = track.n, ds = track.length / n;
    const VMAX = 79, ALAT = 19, ABRAKE = 15;
    const v = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const k = Math.abs(track.curv[i]);
      v[i] = k > 1e-5 ? Math.min(VMAX, Math.sqrt(ALAT / k)) : VMAX;
    }
    // backward pass (braking) and forward pass (acceleration), twice for wraparound
    for (let pass = 0; pass < 2; pass++) {
      for (let i = n - 1; i >= 0; i--) {
        const nx = v[(i + 1) % n];
        v[i] = Math.min(v[i], Math.sqrt(nx * nx + 2 * ABRAKE * ds));
      }
      for (let i = 0; i < n; i++) {
        const pv = v[(i - 1 + n) % n];
        const acc = Math.max(1.2, 9.5 * (1 - pv / 86));
        v[i] = Math.min(v[i], Math.sqrt(pv * pv + 2 * acc * ds));
      }
    }
    // inputs from profile
    const thr = new Float32Array(n), brk = new Float32Array(n), steer = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const a = v[i], b = v[(i + 1) % n];
      const accel = (b * b - a * a) / (2 * ds);
      if (accel < -4) {
        brk[i] = clamp(-accel / ABRAKE, 0, 1);
        thr[i] = 0;
      } else if (accel > 0.6) {
        thr[i] = 1;
      } else {
        thr[i] = a > VMAX - 1 ? 1 : 0.55 + 0.3 * (a / VMAX);
      }
      steer[i] = clamp(track.curv[i] * 55, -1, 1);
    }
    // smooth inputs a little so traces look human
    const sm = (arr, w) => {
      const out = new Float32Array(n);
      for (let i = 0; i < n; i++) {
        let s = 0;
        for (let k = -w; k <= w; k++) s += arr[(i + k + n) % n];
        out[i] = s / (2 * w + 1);
      }
      return out;
    };
    const thrS = sm(thr, 3), brkS = sm(brk, 2), steerS = sm(steer, 4);
    // time at each sample
    const tAt = new Float32Array(n + 1);
    for (let i = 0; i < n; i++) tAt[i + 1] = tAt[i] + ds / ((v[i] + v[(i + 1) % n]) / 2);
    // scale speeds so the lap is ~1:47.6
    const TARGET = 107.6;
    const k = tAt[n] / TARGET;
    for (let i = 0; i < n; i++) v[i] *= k;
    for (let i = 0; i <= n; i++) tAt[i] /= k;
    // fuel integral
    let fuelInt = 0;
    for (let i = 0; i < n; i++) fuelInt += (0.12 + thrS[i]) * (tAt[i + 1] - tAt[i]);
    return { v, thr: thrS, brk: brkS, steer: steerS, tAt, lapTime: tAt[n], fuelInt };
  }

  /* ---------- entry list ---------- */
  const GT3_MODELS = [
    "Ferrari 296 GT3", "Porsche 992 GT3 R", "BMW M4 GT3", "Lamborghini Huracán GT3 EVO2", "McLaren 720S GT3 EVO",
    "Audi R8 LMS GT3 EVO II", "Mercedes-AMG GT3 EVO", "Aston Martin V8 Vantage GT3", "Ford Mustang GT3", "Honda NSX GT3 EVO",
  ];
  const GT4_MODELS = ["Alpine A110 GT4", "BMW M4 GT4", "Porsche 718 Cayman GT4", "Aston Martin Vantage GT4", "Mercedes-AMG GT4", "Maserati MC GT4"];
  const FIRST = ["Luca", "Jonas", "Mateo", "Emil", "Noah", "Ilya", "Sven", "Tomás", "Kenji", "Oscar", "Rafael", "Hugo", "Marek", "Felix", "Arne", "Dario",
    "Lena", "Mika", "Yuki", "Sara", "Pietro", "Nils", "Kacper", "Theo", "Jules", "Bram", "Anton"];
  const LAST = ["Marchetti", "Lindqvist", "Okafor", "Brandt", "Kowalczyk", "Fontaine", "Varga", "Haddad", "Nakamura", "Ruiz", "Sørensen", "Delacroix",
    "Petrov", "Ambrosi", "Weller", "Castell", "Moreau", "Novak", "Aalto", "Keane", "Bianchi", "Duarte", "Engström", "Valli", "Hartmann", "Rossi", "Kaya"];
  const NAT = ["ITA", "SWE", "NGA", "GER", "POL", "FRA", "HUN", "LBN", "JPN", "ESP", "DEN", "FRA", "BUL", "ITA", "GBR", "ESP", "FRA", "CZE", "FIN", "IRL", "ITA", "POR", "SWE", "ITA", "GER", "ITA", "TUR"];
  const TEAMS = ["Scuderia Nord", "Velocity Works", "Apex Nine", "Redline Union", "Kestrel Racing", "Halcyon Motorsport", "Iron Ridge", "Delta Eight",
    "Mistral GT", "Lowline Racing", "Ostra Competition", "Parallax"];

  function buildCars(profile) {
    const cars = [];
    const T0 = profile.lapTime;
    const used = new Set();
    for (let i = 0; i < 26; i++) {
      const cls = i < 18 ? "GT3" : "GT4";
      let number;
      do number = Math.floor(rnd() * 98) + 2; while (used.has(number));
      used.add(number);
      const first = FIRST[i], last = LAST[i];
      const paceBase = cls === "GT3" ? 0.986 + i * 0.0026 : 1.075 + (i - 18) * 0.0024;
      cars.push({
        idx: i,
        number,
        driver: first + " " + last,
        short: first[0] + ". " + last,
        nat: NAT[i],
        team: pick(TEAMS),
        model: cls === "GT3" ? GT3_MODELS[i % GT3_MODELS.length] : GT4_MODELS[i % GT4_MODELS.length],
        cls,
        cat: pick(["P", "G", "G", "S", "S", "S", "B", "B"]),
        pace: paceBase + (rnd() - 0.5) * 0.002,
        noisePhase: rnd() * 100,
        dist: 0, laps: 0, pct: 0,
        pos: 0, classPos: 0, startPos: 0, prevPos: 0,
        lastLap: 0, bestLap: 0, lapStart: 0,
        gapLeader: 0, gapAhead: 0, lapsDown: 0,
        relGap: 0, relLaps: 0, relMeters: 0,
        inPit: false, pitStops: 0, pitLap: 0, pitStop: 0,
        isPlayer: false,
        x: 0, y: 0, heading: 0, lateral: 0,
        speed: 0,
      });
    }
    // player = 7th fastest GT3
    const player = cars[6];
    player.isPlayer = true;
    player.number = 46 === player.number ? 47 : 46;
    player.model = "Ferrari 296 GT3";
    player.pace = 1.0;
    player.team = "Scuderia Nord";
    player.cat = "S";
    return { cars, player, T0 };
  }

  /* ---------- simulation ---------- */
  function create() {
    const track = buildTrack();
    const prof = buildProfile(track);
    const { cars, player, T0 } = buildCars(prof);
    const L = track.length;
    const START_ELAPSED = 14 * 60 + 30;
    const DURATION = 3600;

    // initial distances: elapsed race time divided by each car's pace, plus small grid scatter
    cars.forEach((c, i) => {
      const lapsDone = START_ELAPSED / (T0 * c.pace) - i * 0.0035 + (rnd() - 0.5) * 0.01;
      c.dist = lapsDone;
      c.laps = Math.floor(lapsDone);
      c.pct = lapsDone - c.laps;
      c.lastLap = T0 * c.pace * (1 + rnd() * 0.004);
      c.bestLap = c.lastLap - rnd() * 0.35;
      c.lateral = (rnd() - 0.5) * 4;
      c.startPos = i + 1;
      c.pitLap = c.laps + 1 + Math.floor(rnd() * 10);
      c.pitStops = 0;
    });
    // battle partners for radar / relative: one oscillating alongside, one close ahead
    const battle = cars[5];
    battle.battle = true;
    const ahead = cars[7];
    ahead.dist = player.dist + 42 / L;
    ahead.laps = Math.floor(ahead.dist);
    ahead.pct = ahead.dist - ahead.laps;
    ahead.pace = 1.0003;

    // lap start times from the real time-at-position curve (not linear in distance)
    const tAtPct = (pct) => {
      const f = pct * N, i = Math.floor(f);
      return lerp(prof.tAt[i], prof.tAt[i + 1], f - i);
    };
    cars.forEach((c) => (c.lapStart = -tAtPct(c.pct) * c.pace));
    const SHARE = [tAtPct(track.sectors[1]) / T0, 0, 0];
    SHARE[1] = tAtPct(track.sectors[2]) / T0 - SHARE[0];
    SHARE[2] = 1 - SHARE[0] - SHARE[1];

    // lap history so log/graph overlays have data from the first frame (lap 1 = standing start)
    cars.forEach((c) => {
      c.lapTimes = [];
      c.pitLaps = [];
      c.outLap = false;
      c.penalty = "";
      for (let k = 0; k < c.laps; k++) c.lapTimes.push(T0 * c.pace * (1 + (rnd() - 0.3) * 0.006) + (k === 0 ? 7 + rnd() * 3 : 0));
      c.lastLap = c.lapTimes[c.lapTimes.length - 1];
      c.bestLap = Math.min(...c.lapTimes.slice(1));
    });
    cars[12].penalty = "+5s";
    cars[21].penalty = "DT";
    // one of the player's earlier laps was invalid (track limits) and doesn't count as best
    const pTimes = player.lapTimes;
    let invalidIdx = 2 + Math.floor(rnd() * (pTimes.length - 3));
    if (pTimes[invalidIdx] === player.bestLap) invalidIdx = Math.max(1, invalidIdx - 1);
    player.bestLap = Math.min(...pTimes.filter((tm, k) => k > 0 && k !== invalidIdx));
    const seedLog = pTimes.map((time, k) => {
      const s1 = time * SHARE[0] + (rnd() - 0.5) * 0.12, s2 = time * SHARE[1] + (rnd() - 0.5) * 0.12;
      return { lap: k + 1, time, sectors: [s1, s2, time - s1 - s2], valid: k !== invalidIdx, fuel: 2.86 + rnd() * 0.12, pit: false };
    });

    const best = player.bestLap;
    const state = {
      t: 0,
      session: {
        type: "race", name: "Race", track, duration: DURATION,
        elapsed: START_ELAPSED, timeRemaining: DURATION - START_ELAPSED, lapRemaining: 0,
        flag: "green", yellowSector: 0, flagUntil: 0, nextFlagAt: 18,
        pitWindow: { open: 16 * 60, close: 40 * 60 },
        timeOfDay: 14 * 3600 + 32 * 60,
        best: { GT3: { time: 0, car: null }, GT4: { time: 0, car: null } },
        server: "ACC · Public Race",
      },
      weather: {
        air: 21.4, track: 28.6, rainNow: 0, rain10: 0, rain30: 0.2, wetness: 0, grip: "Optimum",
        windSpeed: 9, windDir: 230, clouds: 0.35,
      },
      player: {
        car: player, speed: 0, gear: 3, rpm: 0, maxRpm: 8000, shiftRpm: 7600,
        throttle: 0, brake: 0, clutch: 0, steer: 0, absActive: false, tcActive: false,
        tc: 3, tc2: 2, abs: 3, bb: 56.2, map: 1, changed: "", changedAt: -10,
        fuel: { liters: 47.3, capacity: 120, perLap: 2.94, lapsLeft: 0, toFinish: 0, toAdd: 0, lastLapUse: 2.94, lapStartFuel: 47.3, history: [2.96, 2.93, 2.95, 2.92, 2.94] },
        tyres: ["FL", "FR", "RL", "RR"].map((name, k) => ({
          name, pressure: 26.8, core: 84, i: 86, m: 84, o: 82, wear: 0.94 - k * 0.004, brake: k < 2 ? 420 : 330, compound: "DHE",
        })),
        lap: {
          current: 0, last: player.lastLap, best, delta: 0, deltaLast: 0, predicted: best,
          sector: 0, sectors: [0, 0, 0], sectorState: ["", "", ""],
          bestSectors: [best * SHARE[0], best * SHARE[1], best * SHARE[2]], lastSectors: [0, 0, 0],
          bestTrace: null, valid: true, invalidAt: 2,
        },
        stintLaps: 0, pitStops: 0, inputs: { thr: prof.thr, brk: prof.brk, steer: prof.steer },
        log: seedLog,
      },
      cars, order: cars.slice(), classes: ["GT3", "GT4"],
      profile: prof,
    };

    // best lap reference trace: time-at-position scaled to the best lap
    const bt = new Float32Array(N + 1);
    for (let i = 0; i <= N; i++) bt[i] = (prof.tAt[i] / prof.lapTime) * best;
    state.player.lap.bestTrace = bt;
    const lastTrace = new Float32Array(N + 1);
    for (let i = 0; i <= N; i++) lastTrace[i] = (prof.tAt[i] / prof.lapTime) * player.lastLap;
    state.player.lap.lastTrace = lastTrace;
    state.player.lap.curTrace = Float32Array.from(lastTrace);
    // pre-fill sectors already driven on the current (partial) lap
    {
      const lap = state.player.lap, sb = track.sectors;
      lap.sector = player.pct < sb[1] ? 0 : player.pct < sb[2] ? 1 : 2;
      for (let k = 0; k < lap.sector; k++) lap.sectors[k] = lap.bestSectors[k] + 0.05 + rnd() * 0.2;
    }
    battle.pitLap = 1e9;

    const sampleIdx = (pct) => {
      const f = pct * N;
      const i = Math.floor(f) % N;
      return { i, j: (i + 1) % N, t: f - Math.floor(f) };
    };

    function placeCar(c) {
      const { i, j, t } = sampleIdx(c.pct);
      const hx = track.heading[i];
      const nx = -Math.sin(hx), ny = Math.cos(hx);
      c.x = lerp(track.x[i], track.x[j], t) + nx * c.lateral;
      c.y = lerp(track.y[i], track.y[j], t) + ny * c.lateral;
      c.heading = hx;
    }

    function speedAt(pct) {
      const { i, j, t } = sampleIdx(pct);
      return lerp(prof.v[i], prof.v[j], t);
    }

    function step(dt) {
      if (dt <= 0) return;
      // sub-step for stability at high sim speeds
      const sub = Math.ceil(dt / 0.05);
      const h = dt / sub;
      for (let k = 0; k < sub; k++) tick(h);
      rank();
    }

    function tick(dt) {
      state.t += dt;
      const t = state.t;
      const s = state.session;
      s.elapsed += dt;
      s.timeRemaining = Math.max(0, s.duration - s.elapsed);
      s.timeOfDay += dt;

      for (let ci = 0; ci < cars.length; ci++) {
        const c = cars[ci];
        let v;
        if (c.isPlayer) {
          v = (speedAt(c.pct) / (c.lapPace || 1)) * (1 + 0.004 * Math.sin(t / 9) + 0.003 * Math.sin(t / 3.7 + 1));
        } else {
          v = (speedAt(c.pct) / c.pace) * (1 + 0.006 * Math.sin(t / 7 + c.noisePhase));
        }
        // pit stop
        if (c.inPit) {
          if (c.pitStop > 0) {
            c.pitStop -= dt;
            v = 0;
            if (c.pitStop <= 0) c.pitStop = -1;
          } else {
            v = 22.2;
            if (c.pct > 0.02 && c.pct < 0.5 && c.pitStop === 0) c.pitStop = 26 + rnd() * 4;
            if (c.pct > 0.05 && c.pct < 0.5 && c.pitStop === -1) {
              c.inPit = false; c.pitStop = 0; c.pitStops++; c.outLap = true;
              if (c.isPlayer) playerStopDone();
            }
          }
        } else if (c.pct > 0.95 && (c.isPlayer
            ? state.player.fuel.liters < state.player.fuel.perLap * 2.2 // player boxes when fuel runs low
            : c.pitStops === 0 && c.laps + 1 === c.pitLap && s.elapsed > s.pitWindow.open)) {
          c.inPit = true;
          c.pitStop = 0;
          c.pitLaps.push(c.laps + 1); // in-lap
          v = 22.2;
        }
        c.speed = v;
        const prevLaps = c.laps;
        if (c.battle && !c.inPit) {
          // oscillate around the player so the radar shows a car alongside
          const off = (6 + 24 * Math.sin(t * 0.22)) / L;
          c.dist = player.dist + off;
          c.lateral = 3.4 * Math.sign(Math.sin(t * 0.05) || 1) + player.lateral;
        } else {
          c.dist += (v * dt) / L;
        }
        c.laps = Math.floor(c.dist);
        c.pct = c.dist - c.laps;
        if (!c.battle) c.lateral = 1.6 * Math.sin(c.pct * 40 + c.noisePhase);
        if (c.laps > prevLaps) {
          // interpolate the exact crossing time inside this tick
          const over = v > 0 ? Math.min(dt, (c.pct * L) / v) : 0;
          const lt = t - over - c.lapStart;
          if (lt > 60) {
            c.lastLap = lt;
            const counts = !c.isPlayer || state.player.lap.valid;
            if (counts && (!c.bestLap || lt < c.bestLap)) c.bestLap = lt;
            c.lapTimes.push(lt);
            if (c.lapTimes.length > 80) c.lapTimes.shift();
          }
          if (c.outLap) { c.pitLaps.push(c.laps); c.outLap = false; }
          c.lapStart = t - over;
          // missed the stop because the window was closed → reschedule
          if (c.laps >= c.pitLap && c.pitStops === 0 && !c.inPit) c.pitLap = c.laps + 1 + Math.floor(rnd() * 6);
          if (c.isPlayer) onPlayerLap(lt);
        }
        placeCar(c);
      }

      playerPhysics(dt);
      flags(dt);
      weather(dt);
    }

    function onPlayerLap(lt) {
      const p = state.player, lap = p.lap;
      lap.last = lt;
      // final sector closes with the lap
      const s3 = lt - lap.sectors[0] - lap.sectors[1];
      lap.sectors[2] = s3;
      lap.sectorState[2] = sectorState(2, s3);
      lap.curTrace[N] = lt;
      // swap traces: last ← current
      lap.lastTrace.set(lap.curTrace);
      if (lap.valid && lt < lap.best) {
        lap.best = lt;
        lap.bestTrace.set(lap.curTrace);
      }
      lap.lastSectors = lap.sectors.slice();
      lap.sectors[0] = lap.sectors[1] = lap.sectors[2] = 0;
      lap.sectorState = ["", "", ""];
      lap.sector = 0;
      const f = p.fuel;
      f.lastLapUse = f.lapStartFuel - f.liters;
      f.history.push(f.lastLapUse);
      if (f.history.length > 5) f.history.shift();
      f.perLap = f.history.reduce((a, b) => a + b, 0) / f.history.length;
      f.lapStartFuel = f.liters;
      p.stintLaps++;
      player.lapPace = 1 + (rnd() - 0.45) * 0.006;
      p.tyres.forEach((ty) => (ty.wear = Math.max(0.5, ty.wear - 0.0045 - rnd() * 0.001)));
      p.log.push({ lap: player.laps, time: lt, sectors: lap.lastSectors, valid: lap.valid, fuel: f.lastLapUse, pit: player.pitLaps.includes(player.laps) });
      if (p.log.length > 80) p.log.shift();
      // next lap: ~1 in 10 gets invalidated somewhere (track limits), never two in a row
      lap.invalidAt = lap.valid && rnd() < 0.1 ? 0.15 + rnd() * 0.75 : 2;
      lap.valid = true;
    }

    function playerStopDone() {
      const p = state.player, f = p.fuel;
      const added = 60 - f.liters;
      f.liters = 60;
      f.lapStartFuel += added; // so this lap's usage stays the fuel burnt, not the fuel added
      p.stintLaps = 0;
      p.pitStops++;
      p.tyres.forEach((ty) => (ty.wear = 1));
    }

    function sectorState(k, sv) {
      const lap = state.player.lap;
      const sb = state.session.best.GT3.time * SHARE[k];
      const r = sv < sb ? "sb" : sv < lap.bestSectors[k] ? "pb" : "slower";
      if (sv < lap.bestSectors[k]) lap.bestSectors[k] = sv;
      return r;
    }

    function playerPhysics(dt) {
      const p = state.player, c = player, lap = p.lap;
      const { i, j, t } = sampleIdx(c.pct);
      const pr = prof;
      const jitter = 0.03 * Math.sin(state.t * 13);
      p.throttle = c.inPit ? (c.speed > 0 ? 0.22 : 0) : clamp(lerp(pr.thr[i], pr.thr[j], t) + (pr.thr[i] < 0.98 && pr.thr[i] > 0.02 ? jitter : 0), 0, 1);
      p.brake = c.inPit ? 0 : clamp(lerp(pr.brk[i], pr.brk[j], t), 0, 1);
      p.steer = clamp(lerp(pr.steer[i], pr.steer[j], t) + 0.015 * Math.sin(state.t * 5.3), -1, 1);
      p.clutch = 0;
      p.speed = c.speed * 3.6;
      const tops = [0, 105, 142, 178, 212, 246, 276];
      let g = 1;
      while (g < 6 && p.speed > tops[g] * 0.96) g++;
      p.gear = g;
      const lo = g > 1 ? tops[g - 1] * 0.72 : 0;
      p.rpm = clamp(3200 + ((p.speed - lo) / (tops[g] - lo)) * (p.maxRpm - 3200), 2800, p.maxRpm);
      p.absActive = p.brake > 0.8 && Math.sin(state.t * 40) > 0.2;
      p.tcActive = p.throttle > 0.6 && Math.abs(p.steer) > 0.18 && Math.sin(state.t * 31) > 0.4;

      // lap timing + delta
      lap.current = state.t - c.lapStart;
      if (lap.valid && c.pct > lap.invalidAt) {
        lap.valid = false;
        const ses = state.session;
        ses.flag = "blackwhite"; // track limits warning
        ses.flagUntil = state.t + 5;
      }
      const fi = c.pct * N, ii = Math.floor(fi), ft = fi - ii;
      lap.curTrace[ii] = lap.current;
      const bestAt = lerp(lap.bestTrace[ii], lap.bestTrace[ii + 1], ft);
      const lastAt = lerp(lap.lastTrace[ii], lap.lastTrace[ii + 1], ft);
      lap.delta = lap.current - bestAt;
      lap.deltaLast = lap.current - lastAt;
      lap.predicted = lap.best + lap.delta;
      const sec = c.pct < track.sectors[1] ? 0 : c.pct < track.sectors[2] ? 1 : 2;
      if (sec > lap.sector) {
        const k = lap.sector;
        const sv = lap.current - (k === 1 ? lap.sectors[0] : 0);
        lap.sectors[k] = sv;
        lap.sectorState[k] = sectorState(k, sv);
        lap.sector = sec;
      }

      // fuel
      const f = p.fuel;
      f.liters = Math.max(0, f.liters - (pr.fuelInt ? (2.94 / pr.fuelInt) * (0.12 + p.throttle) * dt : 0));
      f.lapsLeft = f.liters / f.perLap;
      // fractional laps still to drive: rest of this lap + whole laps until the clock runs out (+ the finishing lap)
      const rest = 1 - c.pct;
      const lapsToGo = rest + Math.ceil(Math.max(0, state.session.timeRemaining - rest * lap.best) / lap.best);
      state.session.lapRemaining = lapsToGo;
      f.toFinish = lapsToGo * f.perLap;
      f.toAdd = Math.max(0, f.toFinish - f.liters + f.perLap * 0.5);

      // tyres + brakes: relax toward a load-dependent equilibrium
      const lat = p.steer, brk = p.brake, thr = p.throttle, spd = c.speed / 80;
      for (let k = 0; k < 4; k++) {
        const ty = p.tyres[k];
        const front = k < 2, left = k % 2 === 0;
        const outside = (lat > 0 && left) || (lat < 0 && !left) ? Math.abs(lat) : 0;
        const target = 74 + 14 * outside + (front ? 9 * brk : 6 * thr) + 6 * spd + (front ? 2 : 0);
        ty.core += (target - ty.core) * dt * 0.08;
        ty.i = ty.core + 3.2 + 1.5 * outside;
        ty.m = ty.core + 0.6;
        ty.o = ty.core - 2.4 + 4 * outside;
        ty.pressure = 20.1 + ty.core * 0.0805 + (front ? 0.15 : 0);
        const bTarget = front ? 260 + 620 * brk * spd : 220 + 380 * brk * spd;
        ty.brake += (bTarget - ty.brake) * dt * (brk > 0.05 ? 1.8 : 0.35);
      }

      // electronics: a driver tweak every ~25 s
      if (state.t - p.changedAt > 25) {
        p.changedAt = state.t;
        const which = pick(["tc", "abs", "bb", "map", "tc2"]);
        if (which === "bb") p.bb = Math.round((p.bb + (rnd() < 0.5 ? -0.2 : 0.2)) * 10) / 10;
        else if (which === "map") p.map = p.map === 1 ? 2 : 1;
        else p[which] = clamp(p[which] + (rnd() < 0.5 ? -1 : 1), 1, 6);
        p.changed = which;
      }
    }

    function flags() {
      const s = state.session;
      if (state.t > s.flagUntil && s.flag !== "green") { s.flag = "green"; s.yellowSector = 0; }
      if (state.t > s.nextFlagAt) {
        const r = rnd();
        if (r < 0.55) { s.flag = "yellow"; s.yellowSector = 1 + Math.floor(rnd() * 3); s.flagUntil = state.t + 9; }
        else if (r < 0.85) { s.flag = "blue"; s.flagUntil = state.t + 6; }
        else { s.flag = "white"; s.flagUntil = state.t + 5; }
        s.nextFlagAt = state.t + 30 + rnd() * 25;
      }
    }

    function weather(dt) {
      const w = state.weather;
      w.track += (29.5 + Math.sin(state.t / 200) - w.track) * dt * 0.01;
      w.rain30 = 0.15 + 0.15 * Math.sin(state.t / 90);
      w.rain10 = Math.max(0, w.rain30 - 0.15);
    }

    function rank() {
      const order = state.order;
      order.sort((a, b) => b.dist - a.dist);
      const leaderDist = order[0].dist;
      const classLeader = {}, classAhead = {}, classCount = {};
      for (let k = 0; k < order.length; k++) {
        const c = order[k];
        c.prevPos = c.pos;
        c.pos = k + 1;
        const cl = c.cls;
        classCount[cl] = (classCount[cl] || 0) + 1;
        c.classPos = classCount[cl];
        if (!classLeader[cl]) classLeader[cl] = c;
        const lead = classLeader[cl];
        const ref = T0 * lead.pace;
        const d = lead.dist - c.dist;
        c.lapsDown = Math.floor(d + 1e-6);
        c.gapLeader = d * ref;
        const ah = classAhead[cl];
        c.gapAhead = ah ? (ah.dist - c.dist) * ref : 0;
        c.lapsAhead = ah ? Math.floor(ah.dist - c.dist) : 0;
        classAhead[cl] = c;
        // relative to player
        const diff = c.dist - player.dist;
        let w = diff - Math.round(diff);
        c.relMeters = w * L;
        c.relGap = w * T0;
        c.relLaps = Math.round(diff - w);
        // best laps
        const b = state.session.best[cl];
        if (c.bestLap && (!b.time || c.bestLap < b.time)) { b.time = c.bestLap; b.car = c; }
      }
      state.leaderDist = leaderDist;
    }

    state.step = step;
    rank();
    for (const c of cars) placeCar(c);
    playerPhysics(0); // derived values (fuel laps, race laps, inputs) valid before the first frame
    return state;
  }

  HUD.state = create();
  HUD.sim = { step: HUD.state.step };
})();
