import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { resolveTerminalTheme } from "../src/terminalPalette.ts";

const terminalPane = readFileSync(
  new URL("../src/components/TerminalPane.tsx", import.meta.url),
  "utf8",
);

test("xterm enables transparent canvas rendering", () => {
  assert.match(terminalPane, /allowTransparency:\s*true/);
  assert.match(terminalPane, /theme:\s*resolveTerminalTheme\(theme,\s*terminalTransparent\)/);
  for (const theme of ["dark", "light"]) {
    assert.equal(resolveTerminalTheme(theme, true).background, "rgba(0, 0, 0, 0)");
    assert.notEqual(resolveTerminalTheme(theme, false).background, "rgba(0, 0, 0, 0)");
  }
});

test("changing transparency forces the canvas to redraw", () => {
  assert.match(terminalPane, /termRef\.current\.refresh\(0,\s*termRef\.current\.rows\s*-\s*1\)/);
});
