import type { OptionDef } from "../hud";

export const framelessOption: OptionDef = {
  key: "frameless",
  label: "Frameless",
  type: "choice",
  default: "off",
  choices: [["off", "Off"], ["on", "On"]],
  hint: "Remove the background and frame",
};
