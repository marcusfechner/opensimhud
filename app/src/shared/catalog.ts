/* What the settings window says about each overlay: group, name, one line, icon. Sizes and
   positions live in config/defaults.json. */

export interface CatalogEntry {
  id: string;
  group: string;
  title: string;
  desc: string;
}

export const CATALOG: CatalogEntry[] = [
  { id: "relative", group: "Core", title: "Relative", desc: "Cars around you on track, with gaps" },
  { id: "standings", group: "Core", title: "Standings", desc: "Classification with status icons" },
  { id: "delta", group: "Core", title: "Delta", desc: "Live delta to your best lap, per sector too" },
  { id: "inputs", group: "Core", title: "Inputs", desc: "Throttle, brake, steering and gear" },
  { id: "fuel", group: "Core", title: "Fuel", desc: "Usage per lap and how much to add" },
  { id: "timer", group: "Core", title: "Session timer", desc: "Time and laps remaining at a glance" },
  { id: "laplog", group: "Core", title: "Lap log", desc: "Every lap with sectors, delta and fuel" },
  { id: "lapgraph", group: "Core", title: "Lap times graph", desc: "Your laps against the cars around you" },
  { id: "radar", group: "Awareness", title: "Radar", desc: "Cars alongside and close behind" },
  { id: "trackmap", group: "Awareness", title: "Track map", desc: "Everyone's position on the circuit" },
  { id: "session", group: "Awareness", title: "Session", desc: "Flags, time remaining and position" },
  { id: "clock", group: "Awareness", title: "Clock", desc: "The time on your PC" },
  { id: "music", group: "Awareness", title: "Music", desc: "What's playing, skip with wheel buttons" },
  { id: "digiflag", group: "Awareness", title: "Digiflag", desc: "Trackside-style LED flag panel" },
  { id: "blindspot", group: "Awareness", title: "Blind spot", desc: "Mirror-style warning lights" },
  { id: "minimap", group: "Awareness", title: "Minimap", desc: "Zoomed map that turns with your car" },
  { id: "flatmap", group: "Awareness", title: "Flat map", desc: "The lap as one line with every car on it" },
  { id: "tyres", group: "Car and strategy", title: "Tyres", desc: "Pressures, temperatures and wear" },
  { id: "weather", group: "Car and strategy", title: "Weather", desc: "Temperatures, rain forecast and grip" },
  { id: "pit", group: "Car and strategy", title: "Pit strategy", desc: "Pit window and stop planning" },
  { id: "electronics", group: "Car and strategy", title: "Electronics", desc: "TC, ABS, brake bias and engine map" },
];

export const byId: Record<string, CatalogEntry> = Object.fromEntries(CATALOG.map((o) => [o.id, o]));

const ICONS: Record<string, string> = {
  relative: '<path d="M3 4h12M3 9h12M3 14h12" /><circle cx="6" cy="9" r="1.6" fill="currentColor"/>',
  standings: '<path d="M3 4h12M3 7.5h12M3 11h12M3 14.5h8"/>',
  delta: '<path d="M2.5 9h13"/><rect x="9" y="6" width="5" height="6" rx="1"/>',
  inputs: '<path d="M2 13l3-6 3 3 3-7 3 9 2-3"/>',
  fuel: '<rect x="3" y="3" width="8" height="12" rx="1.5"/><path d="M11 6l3 2v5a1.5 1.5 0 0 1-3 0M5 6h4"/>',
  radar: '<circle cx="9" cy="9" r="6.5"/><circle cx="9" cy="9" r="3"/><path d="M9 2.5V9l4.5 3"/>',
  trackmap: '<path d="M4 13c-2-2 0-5 2-6s3-4 6-3 4 4 1 6-2 4-4 4-3 0-5-1z"/>',
  session: '<path d="M4 16V2.5M4 3h9l-2 3 2 3H4"/>',
  tyres: '<circle cx="9" cy="9" r="6.5"/><circle cx="9" cy="9" r="2.5"/>',
  weather: '<path d="M5.5 12.5a3 3 0 0 1 .4-6 4 4 0 0 1 7.6 1.2 2.4 2.4 0 0 1-.5 4.8z"/><path d="M7 15l-.6 1.4M10.5 15l-.6 1.4"/>',
  pit: '<path d="M3 15V5l6-2.5L15 5v10M7 15v-4h4v4"/>',
  electronics: '<rect x="3" y="4" width="12" height="10" rx="2"/><path d="M6 7.5v3M9 7.5v3M12 7.5v3"/>',
  timer: '<circle cx="9" cy="10" r="6"/><path d="M9 10V7M7.5 2.5h3"/>',
  clock: '<circle cx="9" cy="9" r="6.5"/><path d="M9 5.5V9l2.5 1.5"/>',
  music: '<path d="M7 13.5V4l8-1.5V12"/><circle cx="5" cy="13.5" r="2"/><circle cx="13" cy="12" r="2"/>',
  laplog: '<path d="M6.5 4.5h8.5M6.5 9h8.5M6.5 13.5h8.5M3 4.5h.01M3 9h.01M3 13.5h.01"/>',
  lapgraph: '<path d="M2.5 15.5h13M3.5 12l3.5-4 3 2.5 4.5-6"/>',
  blindspot: '<path d="M6 5L2.5 9 6 13M12 5l3.5 4-3.5 4"/>',
  minimap: '<rect x="2.5" y="2.5" width="13" height="13" rx="3"/><path d="M9 6l2.5 5.5L9 10.2l-2.5 1.3z"/>',
  flatmap: '<path d="M2.5 9h13M2.5 6.5v5M15.5 6.5v5"/><circle cx="6.5" cy="9" r="1.4" fill="currentColor"/><circle cx="11.5" cy="9" r="1.4" fill="currentColor"/>',
  digiflag: '<path d="M3.5 16V2.5"/><g fill="currentColor" stroke="none"><circle cx="7" cy="4.5" r="1"/><circle cx="10.5" cy="4.5" r="1"/><circle cx="14" cy="4.5" r="1"/><circle cx="7" cy="8" r="1"/><circle cx="10.5" cy="8" r="1"/><circle cx="14" cy="8" r="1"/></g>',
};

export const icon = (id: string) =>
  `<svg viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[id] ?? ""}</svg>`;
