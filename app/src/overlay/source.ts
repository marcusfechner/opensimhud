/* Where frames come from: the Tauri telemetry stream inside the app, or — in a plain browser
   during development — a recording of the demo race made by `npm run demo:frames`. */

import type { Frame } from "../types/telemetry/Frame";

export async function connect(onFrame: (f: Frame) => void): Promise<void> {
  if ("__TAURI_INTERNALS__" in window) {
    const { Channel, invoke } = await import("@tauri-apps/api/core");
    const channel = new Channel<Frame>();
    channel.onmessage = onFrame;
    await invoke("telemetry_subscribe", { channel });
  } else {
    await replay(onFrame);
  }
}

/** Replay `public/dev/demo-frames.jsonl` ({at, frame} per line) in real time, looping. */
async function replay(onFrame: (f: Frame) => void) {
  const res = await fetch("/dev/demo-frames.jsonl");
  if (!res.ok) throw new Error("No demo recording. Run `npm run demo:frames` first.");
  const lines = (await res.text()).trim().split("\n").map((l) => JSON.parse(l) as { at: number; frame: Frame });
  const end = lines[lines.length - 1].at;
  // ?skip=10 starts the replay 10 s in (handy for screenshots)
  const skip = +(new URLSearchParams(location.search).get("skip") || 0);
  let i = 0;
  while (i < lines.length && lines[i].at <= skip) onFrame(lines[i++].frame);
  let t0 = performance.now() - skip * 1000;
  const tick = () => {
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
  void end;
}
