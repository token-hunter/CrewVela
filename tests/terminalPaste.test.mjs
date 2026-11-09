import test from "node:test";
import assert from "node:assert/strict";

import {
  isSupportedImageFile,
  terminalDropIntent,
} from "../src/components/terminalPaste.ts";

test("accepts the supported terminal image formats", () => {
  for (const file of [
    { name: "screen.png", type: "image/png" },
    { name: "photo.JPG", type: "image/jpeg" },
    { name: "animation.gif", type: "image/gif" },
    { name: "card.webp", type: "image/webp" },
  ]) {
    assert.equal(isSupportedImageFile(file), true, file.name);
    assert.equal(terminalDropIntent([file]), "image", file.name);
  }
});

test("rejects non-image file drops", () => {
  assert.equal(
    terminalDropIntent([{ name: "notes.txt", type: "text/plain" }]),
    "reject",
  );
});

test("keeps empty-file drags available for terminal pane ordering", () => {
  assert.equal(terminalDropIntent([]), "pane");
});
