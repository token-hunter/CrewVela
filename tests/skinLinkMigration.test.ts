import test from "node:test";
import assert from "node:assert/strict";

import {
  DEEP_SKIN_IDS,
  isDeepSkinId,
  migrateLegacySkinId,
} from "../src/skinIds.ts";

test("all six current skin ids are accepted without rewriting", () => {
  assert.equal(DEEP_SKIN_IDS.length, 6);
  for (const id of DEEP_SKIN_IDS) {
    assert.equal(isDeepSkinId(id), true);
    assert.equal(migrateLegacySkinId(id), id);
  }
});

test("legacy color-only companion presets migrate to deep skins", () => {
  assert.equal(migrateLegacySkinId("warm-orange"), "deep-orange-machine");
  assert.equal(migrateLegacySkinId("city-rain"), "deep-rain-office");
  assert.equal(migrateLegacySkinId("moon-study"), "deep-paper-lab");
  assert.equal(migrateLegacySkinId("retro-terminal"), "deep-midnight-records");
  assert.equal(migrateLegacySkinId("morning-light"), "deep-snow-station");
  assert.equal(migrateLegacySkinId("unknown-skin"), undefined);
});
