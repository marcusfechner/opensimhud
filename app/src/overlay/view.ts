/* The state overlays render, rebuilt from telemetry frames.

   Rust sends the state as separate streams (see crates/telemetry/src/stream.rs). This merges
   them into one long-lived object with the shape the overlays were designed against
   (mockups/shared/mock-session.js): every car is one stable object, and references are
   resolved (player.car, order, session.best[cls].car are car objects). Frames are merged in
   place, so overlays can cache nothing and still allocate nothing per update. */

import type { Capabilities } from "../types/telemetry/Capabilities";
import type { CarFast } from "../types/telemetry/CarFast";
import type { CarHistory } from "../types/telemetry/CarHistory";
import type { Connection } from "../types/telemetry/Connection";
import type { Entry } from "../types/telemetry/Entry";
import type { Frame } from "../types/telemetry/Frame";
import type { Fuel } from "../types/telemetry/Fuel";
import type { Game } from "../types/telemetry/Game";
import type { LapEntry } from "../types/telemetry/LapEntry";
import type { PlayerFast } from "../types/telemetry/PlayerFast";
import type { SessionInfo } from "../types/telemetry/SessionInfo";
import type { Standing } from "../types/telemetry/Standing";
import type { Track } from "../types/telemetry/Track";
import type { Tyre } from "../types/telemetry/Tyre";
import type { Weather } from "../types/telemetry/Weather";

export type ViewCar = Entry &
  CarFast &
  Standing &
  CarHistory & {
    /** Sideways offset from the player (= relLateral); the radar subtracts the player's 0. */
    lateral: number;
  };

export type ViewTrack = Omit<Track, "x" | "y"> & {
  x: Float32Array;
  y: Float32Array;
  /** Samples in x / y. */
  n: number;
  /** Direction of travel at each sample, radians. */
  heading: Float32Array;
};

export type ViewSession = Omit<SessionInfo, "best"> & {
  track: ViewTrack;
  best: Record<string, { time: number; car: ViewCar | null }>;
};

export type ViewPlayer = Omit<PlayerFast, "car"> & {
  car: ViewCar;
  /** `lapStartFuel` = liters at the start of this lap (liters + used this lap). */
  fuel: Fuel & { lapStartFuel: number };
  tyres: Tyre[];
  stintLaps: number;
  pitStops: number;
  log: LapEntry[];
};

export interface ViewState {
  /** True once a full snapshot arrived; overlays mount after that. */
  ready: boolean;
  t: number;
  game: Game;
  connection: Connection;
  capabilities: Capabilities;
  classes: string[];
  track: ViewTrack;
  session: ViewSession;
  weather: Weather;
  player: ViewPlayer;
  cars: ViewCar[];
  order: ViewCar[];
}

function blankCar(idx: number): ViewCar {
  return {
    idx, number: 0, driver: "", short: "", team: "", model: "", cls: "", cat: "", nat: "", isPlayer: false,
    pct: 0, dist: 0, x: 0, y: 0, heading: 0, speed: 0, relMeters: 0, relLateral: 0, relGap: 0, inPit: false,
    laps: 0, pos: 0, classPos: 0, startPos: 0, lastLap: 0, bestLap: 0, gapLeader: 0, gapAhead: 0,
    lapsDown: 0, lapsAhead: 0, relLaps: 0, outLap: false, pitStops: 0, penalty: "",
    lapTimes: [], pitLaps: [], lateral: 0,
  };
}

function buildTrack(t: Track): ViewTrack {
  const n = t.x.length;
  const x = Float32Array.from(t.x), y = Float32Array.from(t.y);
  const heading = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const a = (i - 1 + n) % n, b = (i + 1) % n;
    heading[i] = Math.atan2(y[b] - y[a], x[b] - x[a]);
  }
  return { name: t.name, length: t.length, sectors: t.sectors, x, y, n, heading };
}

export function createView(): ViewState {
  const track = buildTrack({ name: "", length: 1, sectors: [0], x: [], y: [] });
  const car = blankCar(0);
  return {
    ready: false,
    t: 0,
    game: "demo",
    connection: "waitingForGame",
    capabilities: { tyreZones: false, tyreWear: false, driverCategory: false, perCarLaps: false, carPositions: false },
    classes: [],
    track,
    session: {
      kind: "practice", name: "", duration: 0, elapsed: 0, timeRemaining: 0, lapRemaining: 0, flag: "none",
      yellowSector: 0, pitWindow: null, mandatoryStopsLeft: null, timeOfDay: 0, best: {}, track,
    },
    weather: { air: 0, track: 0, rainNow: 0, rain10: 0, rain30: 0, wetness: 0, grip: "", windSpeed: 0, windDir: 0, clouds: 0 },
    player: {
      car, speed: 0, gear: 0, rpm: 0, maxRpm: 0, shiftRpm: 0, throttle: 0, brake: 0, clutch: 0, steer: 0,
      absActive: false, tcActive: false, tc: 0, tc2: 0, abs: 0, bb: 0, map: 0, changed: "", changedAt: -1e9,
      lap: { current: 0, last: 0, best: 0, delta: 0, deltaLast: 0, predicted: 0, sector: 0, sectors: [], sectorState: [], bestSectors: [], lastSectors: [], sectorDeltas: [], lastSectorState: [], lastSectorDeltas: [], valid: true },
      fuel: { liters: 0, capacity: 0, perLap: 0, lapsLeft: 0, toFinish: 0, toAdd: 0, lastLapUse: 0, usedThisLap: 0, lapStartFuel: 0 },
      tyres: [], stintLaps: 0, pitStops: 0, log: [],
    },
    cars: [car],
    order: [car],
  };
}

function ensureCars(v: ViewState, n: number) {
  while (v.cars.length < n) v.cars.push(blankCar(v.cars.length));
  v.cars.length = n;
}

const seen = new Set<Frame["kind"]>();

/** Merge one frame into the view. */
export function apply(v: ViewState, f: Frame) {
  switch (f.kind) {
    case "meta":
      v.game = f.game;
      v.connection = f.connection;
      v.capabilities = f.capabilities;
      v.classes = f.classes;
      break;
    case "track":
      v.track = v.session.track = buildTrack(f);
      break;
    case "entries":
      ensureCars(v, f.cars.length);
      for (const e of f.cars) Object.assign(v.cars[e.idx], e);
      break;
    case "fast": {
      v.t = f.t;
      ensureCars(v, f.cars.length);
      const { car, ...player } = f.player;
      Object.assign(v.player, player);
      v.player.car = v.cars[car] ?? v.player.car;
      for (let i = 0; i < f.cars.length; i++) {
        const c = v.cars[i];
        Object.assign(c, f.cars[i]);
        c.lateral = c.relLateral;
      }
      break;
    }
    case "standings": {
      ensureCars(v, f.cars.length);
      for (let i = 0; i < f.cars.length; i++) Object.assign(v.cars[i], f.cars[i]);
      v.order.length = 0;
      for (const i of f.order) v.order.push(v.cars[i]);
      break;
    }
    case "slow": {
      const { best, ...session } = f.session;
      Object.assign(v.session, session);
      const out: ViewSession["best"] = {};
      for (const cls in best) {
        const b = best[cls]!;
        out[cls] = { time: b.time, car: b.car == null ? null : v.cars[b.car] ?? null };
      }
      v.session.best = out;
      v.weather = f.weather;
      v.player.fuel = { ...f.fuel, lapStartFuel: f.fuel.liters + f.fuel.usedThisLap };
      v.player.tyres = f.tyres;
      v.player.stintLaps = f.stintLaps;
      v.player.pitStops = f.pitStops;
      break;
    }
    case "history":
      v.player.log = f.log;
      ensureCars(v, f.cars.length);
      for (let i = 0; i < f.cars.length; i++) {
        v.cars[i].lapTimes = f.cars[i].lapTimes;
        v.cars[i].pitLaps = f.cars[i].pitLaps;
      }
      break;
  }
  if (!v.ready) {
    seen.add(f.kind);
    v.ready = ["meta", "track", "entries", "fast", "standings", "slow", "history"].every((k) => seen.has(k as Frame["kind"]));
  }
}
