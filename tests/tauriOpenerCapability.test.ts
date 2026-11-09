import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("退休功能不再授予作品路径权限，保留通用外链打开", async () => {
  const capability = JSON.parse(await readFile(new URL("../src-tauri/capabilities/default.json", import.meta.url), "utf8"));
  const config = JSON.parse(await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.ok(capability.permissions.includes("opener:default"));
  assert.doesNotMatch(JSON.stringify([capability, config]), /model-eval-artifacts|media-artifacts|orange-speech|orange-wake/);
});
