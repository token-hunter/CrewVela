import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const terminalPane = readFileSync(
  new URL("../src/components/TerminalPane.tsx", import.meta.url),
  "utf8",
);

test("xterm enables transparent canvas rendering", () => {
  assert.match(terminalPane, /allowTransparency:\s*true/);
  assert.match(terminalPane, /background:\s*"rgba\(0, 0, 0, 0\)"/);
});

test("changing transparency forces the canvas to redraw", () => {
  assert.match(terminalPane, /termRef\.current\.refresh\(0,\s*termRef\.current\.rows\s*-\s*1\)/);
});
