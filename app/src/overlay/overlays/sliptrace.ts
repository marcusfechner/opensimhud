/** Shared timestamped ring buffer for the slip-angle and handling-balance traces. */
export const TRACE_GAP_SECONDS = 0.4;
export const TRACE_MAX_SECONDS = 20;
export const TRACE_SAMPLE_CAPACITY_HZ = 90;

export const TRACE_CANVAS_HEIGHT = 93;
export const TRACE_AXIS_LEFT = 38;
export const TRACE_AXIS_RIGHT = 8;

const PANEL_BORDER = 2;
const PANEL_PADDING_X = 10;
const BASE_OVERLAY_WIDTH = 420;

export const TRACE_BASE_PLOT_WIDTH =
  BASE_OVERLAY_WIDTH - PANEL_BORDER - 2 * PANEL_PADDING_X - TRACE_AXIS_LEFT - TRACE_AXIS_RIGHT;

const MAX_PLOT_WIDTH =
  1920 - PANEL_BORDER - 2 * PANEL_PADDING_X - TRACE_AXIS_LEFT - TRACE_AXIS_RIGHT;
export const TRACE_CAPACITY = Math.ceil(
  TRACE_MAX_SECONDS * TRACE_SAMPLE_CAPACITY_HZ * (MAX_PLOT_WIDTH / TRACE_BASE_PLOT_WIDTH),
) + 16;

/** Width of the canvas inside the panel's border and horizontal padding. */
export function traceCanvasWidth(overlayWidth: number): number {
  return overlayWidth - PANEL_BORDER - 2 * PANEL_PADDING_X;
}

/** A wider overlay keeps the same pixel speed and therefore shows more history. */
export function traceWindowSeconds(secondsAtBaseWidth: number, plotWidth: number): number {
  return secondsAtBaseWidth * plotWidth / TRACE_BASE_PLOT_WIDTH;
}

/**
 * Append results: 0 = ignored (invalid or duplicate timestamp), 1 = continuous, 2 =
 * discontinuity (rewind or long pause). Invalid samples are kept as explicit gaps.
 */
export class TraceBuffer {
  readonly time: Float64Array;
  readonly values: Float32Array;
  readonly valid: Uint8Array;
  readonly breakBefore: Uint8Array;
  readonly channels: number;
  readonly capacity: number;

  head = 0;
  count = 0;
  lastTime = -Infinity;

  constructor(channels: number, capacity = TRACE_CAPACITY) {
    this.channels = channels;
    this.capacity = capacity;
    this.time = new Float64Array(capacity);
    this.values = new Float32Array(capacity * channels);
    this.valid = new Uint8Array(capacity);
    this.breakBefore = new Uint8Array(capacity);
  }

  append(timestamp: number, sample: ArrayLike<number> | null): 0 | 1 | 2 {
    if (!Number.isFinite(timestamp) || timestamp === this.lastTime) return 0;

    let discontinuity = false;
    if (timestamp < this.lastTime) {
      this.clear();
      discontinuity = true;
    } else if (timestamp - this.lastTime > TRACE_GAP_SECONDS) {
      discontinuity = true;
    }

    const slot = this.head;
    const valueAt = slot * this.channels;
    let usable = sample !== null && sample.length >= this.channels;
    for (let channel = 0; channel < this.channels; channel++) {
      const value = usable ? sample![channel] : 0;
      if (!Number.isFinite(value)) usable = false;
      this.values[valueAt + channel] = Number.isFinite(value) ? value : 0;
    }

    this.time[slot] = timestamp;
    this.valid[slot] = usable ? 1 : 0;
    this.breakBefore[slot] = this.count === 0 || discontinuity ? 1 : 0;
    this.head = (slot + 1) % this.capacity;
    this.count = Math.min(this.capacity, this.count + 1);
    this.lastTime = timestamp;
    return discontinuity ? 2 : 1;
  }

  clear() {
    this.head = 0;
    this.count = 0;
    this.lastTime = -Infinity;
  }

  /** Physical ring slot for a chronological index (oldest first). */
  slot(index: number): number {
    return (this.head - this.count + index + this.capacity) % this.capacity;
  }

  timeAt(index: number): number {
    return this.time[this.slot(index)];
  }

  valueAt(index: number, channel: number): number {
    return this.values[this.slot(index) * this.channels + channel];
  }

  validAt(index: number): boolean {
    return this.valid[this.slot(index)] !== 0;
  }

  breaksAt(index: number): boolean {
    return this.breakBefore[this.slot(index)] !== 0;
  }
}

/** Start at the newest sample just outside the visible window, when one is available. */
export function traceStart(trace: TraceBuffer, windowSeconds: number): number {
  if (trace.count < 2) return 0;
  const newestTime = trace.lastTime;
  let index = trace.count - 1;
  while (index > 0 && newestTime - trace.timeAt(index) <= windowSeconds) index--;
  return index;
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
