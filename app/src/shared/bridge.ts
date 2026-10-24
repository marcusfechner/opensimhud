/* Everything the windows need from the app, in one place.

   Inside the app this calls the Rust side (invoke / events / channels). In a plain browser
   during development it stands in: the config lives in localStorage and is shared between
   tabs (open the settings page and an overlay page side by side), and telemetry is the demo
   race recorded by `npm run demo:frames`. */

import type { AccSetup } from "../types/config/AccSetup";
import type { Config } from "../types/config/Config";
import type { Display } from "../types/config/Display";
import type { Status } from "../types/config/Status";
import type { Ui } from "../types/config/Ui";
import type { UpdateInfo } from "../types/config/UpdateInfo";
import type { UpdateProgress } from "../types/config/UpdateProgress";
import type { Frame } from "../types/telemetry/Frame";
import { defaultConfig } from "./defaults";

export type { AccSetup, Config, Display, Status, Ui, UpdateInfo, UpdateProgress };

export const inTauri = "__TAURI_INTERNALS__" in window;

const tauri = () => import("@tauri-apps/api/core");
const events = () => import("@tauri-apps/api/event");

/* ---------- browser stand-in ---------- */

const KEY = "opensimhud-config";
const UI_KEY = "opensimhud-ui";
const bus = inTauri ? null : new BroadcastChannel("opensimhud");
const local = new Map<string, Set<(v: unknown) => void>>();

function emitLocal(name: string, payload: unknown) {
  local.get(name)?.forEach((fn) => fn(payload));
  bus?.postMessage({ name, payload });
}
bus?.addEventListener("message", (e) => local.get(e.data.name)?.forEach((fn) => fn(e.data.payload)));

function readJson<T>(key: string): T | null {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : null;
  } catch {
    return null;
  }
}
function writeJson(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // private mode: works for this page only
  }
}

/** Listen to an app event; returns the unsubscribe function. */
function on<T>(name: string, fn: (v: T) => void): () => void {
  if (inTauri) {
    let off: (() => void) | null = null;
    let dead = false;
    void events().then(({ listen }) =>
      listen<T>(name, (e) => fn(e.payload)).then((u) => (dead ? u() : (off = u))),
    );
    return () => {
      dead = true;
      off?.();
    };
  }
  const set = local.get(name) ?? new Set();
  local.set(name, set);
  const wrapped = fn as (v: unknown) => void;
  set.add(wrapped);
  return () => void set.delete(wrapped);
}

/* ---------- config ---------- */

export async function getConfig(): Promise<Config> {
  if (inTauri) return (await tauri()).invoke<Config>("config_get");
  return readJson<Config>(KEY) ?? defaultConfig();
}

/** Save and apply; resolves to the config as the app stored it (repaired if needed). */
export async function setConfig(c: Config): Promise<Config> {
  if (inTauri) return (await tauri()).invoke<Config>("config_set", { config: c });
  writeJson(KEY, c);
  emitLocal("config", c);
  return c;
}

export const onConfig = (fn: (c: Config) => void) => on<Config>("config", fn);

/* ---------- session state: hidden, editing ---------- */

export async function getUi(): Promise<Ui> {
  if (inTauri) return (await tauri()).invoke<Ui>("ui_get");
  return readJson<Ui>(UI_KEY) ?? { hidden: false, editing: false };
}

async function setUi(change: Partial<Ui>) {
  const ui = { ...(await getUi()), ...change };
  if (ui.editing) ui.hidden = false;
  writeJson(UI_KEY, ui);
  emitLocal("ui", ui);
}

export async function setHidden(hidden: boolean) {
  if (inTauri) return (await tauri()).invoke("set_hidden", { hidden });
  return setUi({ hidden });
}

export async function setEditing(editing: boolean) {
  if (inTauri) return (await tauri()).invoke("set_editing", { editing });
  return setUi({ editing });
}

export const onUi = (fn: (u: Ui) => void) => on<Ui>("ui", fn);

/* ---------- game status, displays, ACC setup ---------- */

const browserStatus: Status = { game: "demo", connection: "connected", demo: false, track: "Monza", session: "race" };

export async function getStatus(): Promise<Status | null> {
  if (inTauri) return (await tauri()).invoke<Status | null>("status_get");
  return browserStatus;
}

export const onStatus = (fn: (s: Status) => void) => on<Status>("status", fn);

export async function getDisplays(): Promise<Display[]> {
  if (inTauri) return (await tauri()).invoke<Display[]>("displays");
  const k = devicePixelRatio || 1;
  return [{ index: 0, name: "This screen", width: Math.round(screen.width * k), height: Math.round(screen.height * k), primary: true }];
}

export async function accSetup(): Promise<AccSetup> {
  if (inTauri) return (await tauri()).invoke<AccSetup>("acc_setup");
  return { path: null, enabled: false, port: 0, error: "Only available in the app on Windows." };
}

export async function accEnable(): Promise<AccSetup> {
  if (inTauri) return (await tauri()).invoke<AccSetup>("acc_enable");
  return accSetup();
}

/* ---------- updates ---------- */

/** A newer release on GitHub, or `null` when this is the latest. Rejects when offline. */
export async function checkUpdate(): Promise<UpdateInfo | null> {
  if (inTauri) return (await tauri()).invoke<UpdateInfo | null>("update_check");
  return null;
}

/** Download and install the update `checkUpdate` found; the app restarts into it. */
export async function installUpdate(): Promise<void> {
  if (inTauri) await (await tauri()).invoke("update_install");
}

export const onUpdateProgress = (fn: (p: UpdateProgress) => void) => on<UpdateProgress>("update-progress", fn);

/* ---------- telemetry ---------- */

/** The overlay windows' feed: the game, or the demo race in demo mode. */
export async function telemetry(onFrame: (f: Frame) => void): Promise<void> {
  if (inTauri) {
    const { Channel, invoke } = await tauri();
    const channel = new Channel<Frame>();
    channel.onmessage = onFrame;
    await invoke("telemetry_subscribe", { channel });
  } else {
    replay(onFrame);
  }
}

/** The settings window's previews: always the demo race. Returns a stop function. */
export function preview(onFrame: (f: Frame) => void): () => void {
  if (inTauri) {
    let stopped = false;
    void tauri().then(({ Channel, invoke }) => {
      if (stopped) return;
      const channel = new Channel<Frame>();
      channel.onmessage = (f) => !stopped && onFrame(f);
      void invoke("preview_subscribe", { channel });
    });
    return () => {
      stopped = true;
      void tauri().then(({ invoke }) => invoke("preview_unsubscribe"));
    };
  }
  return replay(onFrame);
}

type Line = { at: number; frame: Frame };
let recording: Promise<Line[]> | null = null;

/** Replay `public/dev/demo-frames.jsonl` in real time, looping. Returns a stop function. */
function replay(onFrame: (f: Frame) => void): () => void {
  recording ??= fetch("/dev/demo-frames.jsonl").then(async (res) => {
    if (!res.ok) throw new Error("No demo recording. Run `npm run demo:frames` first.");
    return (await res.text()).trim().split("\n").map((l) => JSON.parse(l) as Line);
  });
  let stopped = false;
  void recording.then((lines) => {
    // ?skip=10 starts the replay 10 s in (handy for screenshots)
    const skip = +(new URLSearchParams(location.search).get("skip") || 0);
    let i = 0;
    while (i < lines.length && lines[i].at <= skip) onFrame(lines[i++].frame);
    let t0 = performance.now() - skip * 1000;
    const tick = () => {
      if (stopped) return;
      const at = (performance.now() - t0) / 1000;
      while (i < lines.length && lines[i].at <= at) onFrame(lines[i++].frame);
      if (i >= lines.length) {
        // loop: lap times jump at the seam, which is fine for a dev preview
        i = 0;
        t0 = performance.now();
      }
      setTimeout(tick, 1000 / 120);
    };
    tick();
  });
  return () => void (stopped = true);
}
