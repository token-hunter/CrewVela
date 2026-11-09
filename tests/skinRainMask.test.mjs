import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readStyles } from "./helpers/loadStyles.mjs";

const component = readFileSync(new URL("../src/components/SkinAtmosphere.tsx", import.meta.url), "utf8");
const styles = readStyles();

test("rain animation uses a dedicated canvas limited to the exterior window", () => {
  assert.match(component, /className="rain-window-canvas"/);
  assert.match(styles, /\.rain-window-canvas\s*\{[^}]*width:\s*21%/s);
  assert.match(styles, /\.rain-window-canvas\s*\{[^}]*height:\s*69%/s);
});

test("removed deep-rain-office skin leaves no selectors behind", () => {
  assert.doesNotMatch(styles, /data-deep-skin="deep-rain-office"/);
});
