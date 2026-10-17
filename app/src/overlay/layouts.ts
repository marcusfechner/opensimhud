/* Built-in layouts on a 1920×1080 reference screen; anchors scale to any display.
   They become the default Race and Practice profiles once config lands (milestone 5). */

export type Anchor = Partial<Record<"left" | "right" | "top" | "bottom", number>>;

export const LAYOUTS: Record<string, Record<string, Anchor>> = {
  race: {
    standings: { left: 24, top: 24 },
    relative: { right: 24, bottom: 24 },
    delta: { left: 700, top: 136 },
    inputs: { left: 750, bottom: 24 },
    fuel: { right: 24, top: 300 },
    radar: { left: 850, top: 470 },
    trackmap: { right: 24, top: 24 },
    session: { left: 770, top: 24 },
    weather: { right: 340, top: 24 },
    tyres: { right: 310, top: 300 },
    pit: { left: 24, bottom: 24 },
    electronics: { left: 780, bottom: 170 },
  },
  practice: {
    timer: { left: 24, top: 24 },
    laplog: { left: 24, top: 190 },
    lapgraph: { left: 24, bottom: 24 },
    digiflag: { left: 840, top: 24 },
    flatmap: { left: 600, top: 236 },
    blindspot: { left: 640, top: 400 },
    delta: { left: 700, bottom: 150 },
    inputs: { left: 750, bottom: 24 },
    minimap: { right: 24, top: 24 },
    fuel: { right: 24, top: 270 },
    relative: { right: 24, bottom: 24 },
  },
};
