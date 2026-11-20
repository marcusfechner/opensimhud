import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const source = await readFile(new URL("../src/overlay/tyre-temperature.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { tyreTemperatureWindow, tyreTemperaturePosition } = await import(`data:text/javascript,${encodeURIComponent(js)}`);

test("EVO compound windows and SF25 wet exceptions", () => {
  for (const [compound, model, low, high] of [
    ["Slick (S)", "Ferrari 296 GT3", 70, 105],
    ["Slick (S)", "Ferrari F2004", 70, 105],
    ["Intermediate (I)", "Ferrari F2004", 30, 80],
    ["Wet (W)", "Ferrari 296 GT3", 30, 80],
    ["Wet (W)", "Ferrari F2004", 30, 80],
    ["Slick Soft (S)", "", 95, 110],
    ["Slick Medium (M)", "", 100, 125],
    ["Slick Hard (H)", "", 105, 140],
    ["Intermediate (I)", "Ferrari SF25", 95, 110],
    ["Wet (W)", "Ferrari SF25", 95, 110],
  ]) {
    assert.deepEqual(tyreTemperatureWindow("evo", compound, model), { low, high });
    assert.equal(tyreTemperaturePosition(low, { low, high }), 2);
    assert.equal(tyreTemperaturePosition(high, { low, high }), 2);
  }
  assert.deepEqual(tyreTemperatureWindow("acc", "Wet (W)", "Ferrari SF25"), { low: 40, high: 65 });
  assert.deepEqual(tyreTemperatureWindow("acc", "DHE"), { low: 80, high: 105 });
  assert.deepEqual(tyreTemperatureWindow("acc", "Slick Soft (S)"), { low: 80, high: 105 });
  assert.equal(tyreTemperaturePosition(80, tyreTemperatureWindow("acc", "DHE")), 2);
  assert.equal(tyreTemperaturePosition(105, tyreTemperatureWindow("acc", "DHE")), 2);
});
