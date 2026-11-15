import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const windows = process.platform === "win32";
const powershell = path.join(process.env.SystemRoot || "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
const repair = path.resolve("tools/repair-codex-plugin-hooks.ps1");
function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), "crewvela hook's repair "));
  const plugin = path.join(root, "plugin cache");
  for (const directory of ["hooks", ".codex-plugin", "skills/impeccable/scripts"]) mkdirSync(path.join(plugin, directory), { recursive: true });
  writeFileSync(path.join(plugin, ".codex-plugin/plugin.json"), JSON.stringify({ name: "impeccable", version: "4.3.1" }));
  const handler = { type: "command", command: '[ ! -f "${PLUGIN_ROOT}/skills/impeccable/scripts/impeccable" ] || "${PLUGIN_ROOT}/skills/impeccable/scripts/impeccable" hook', commandWindows: 'if exist "${PLUGIN_ROOT}/skills/impeccable/scripts/impeccable.cmd" ("${PLUGIN_ROOT}/skills/impeccable/scripts/impeccable.cmd" hook & exit /b)', timeout: 30, statusMessage: "Keep original semantics" };
  const hooks = { hooks: { PostToolUse: [{ matcher: "Edit|Write|apply_patch", hooks: [structuredClone(handler)] }], Stop: [{ hooks: [structuredClone(handler)] }] } };
  const hooksPath = path.join(plugin, "hooks/hooks.json");
  writeFileSync(hooksPath, JSON.stringify(hooks, null, 2));
  const config = path.join(root, "config.toml");
  const raw = '# preserve comments\r\nmodel = "fixture"\r\n[other]\r\nvalue = "keep"\r\n' + ["post_tool_use", "stop"].map(event => `[hooks.state."impeccable@openai-curated-remote:hooks/hooks.json:${event}:0:0"]\r\nenabled = false\r\ntrusted_hash = "sha256:${"a".repeat(64)}"\r\ntrusted_content_hash = "sha256:${"b".repeat(64)}"\r\n\r\n`).join("") + '[hooks.state."unrelated"]\r\ntrusted_content_hash = "keep-this-other-field"\r\n';
  writeFileSync(config, raw);
  const launcher = path.join(plugin, "skills/impeccable/scripts/impeccable.cmd");
  const echo = path.join(root, "echo.mjs");
  writeFileSync(echo, `const chunks=[];for await(const chunk of process.stdin)chunks.push(chunk);process.stdout.write(Buffer.concat(chunks));process.stderr.write("REAL_STDERR_中文");process.exit(37);`);
  writeFileSync(launcher, `@echo off\r\n"${process.execPath}" "${echo}" %*\r\nexit /b %ERRORLEVEL%\r\n`);
  return { root, plugin, hooksPath, config, raw, hooks };
}
function run(fixture, extra = []) {
  return spawnSync(powershell, ["-NoProfile", "-File", repair, "-PluginRoot", fixture.plugin, "-ConfigPath", fixture.config, ...extra], { encoding: "utf8", windowsHide: true, timeout: 15000 });
}
function planned(fixture) {
  const result = run(fixture);
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout.replace(/^\uFEFF/, ""));
}

test("Codex plugin repair changes reviewed commands and preserves modern content trust", { skip: !windows }, () => {
  const f = fixture();
  try {
    const beforeHooks = readFileSync(f.hooksPath);
    const plan = planned(f);
    assert.equal(plan.applied, false);
    assert.deepEqual(readFileSync(f.hooksPath), beforeHooks);
    assert.equal(readFileSync(f.config, "utf8"), f.raw);
    assert.equal(plan.commandChanges.length, 2);
    assert.equal(plan.removedConfigFields.length, 0);
    const applied = run(f, ["-Apply", "-ExpectedHooksHash", plan.expectedHooksHash, "-ExpectedConfigHash", plan.expectedConfigHash]);
    assert.equal(applied.status, 0, applied.stderr);
    const manifest = JSON.parse(applied.stdout.replace(/^\uFEFF/, ""));
    const hooks = JSON.parse(readFileSync(f.hooksPath, "utf8"));
    for (const event of ["PostToolUse", "Stop"]) f.hooks.hooks[event][0].hooks[0].commandWindows = plan.commandChanges[0].after;
    assert.deepEqual(hooks, f.hooks);
    assert.equal(readFileSync(f.config, "utf8"), f.raw);
    assert.deepEqual(readFileSync(path.join(manifest.backupRoot, "impeccable-hooks.json")), beforeHooks);
    assert.equal(manifest.nativeTrustRequired, true);
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});

test("old-reader content-hash compatibility requires an explicit opt-in and keeps unrelated trust", { skip: !windows }, () => {
  const f = fixture();
  try {
    const preview = run(f, ["-RemoveLegacyContentHash"]);
    assert.equal(preview.status, 0, preview.stderr);
    const plan = JSON.parse(preview.stdout.replace(/^\uFEFF/, ""));
    assert.equal(plan.removedConfigFields.length, 2);
    assert.equal(readFileSync(f.config, "utf8"), f.raw);
    const applied = run(f, ["-RemoveLegacyContentHash", "-Apply", "-ExpectedHooksHash", plan.expectedHooksHash, "-ExpectedConfigHash", plan.expectedConfigHash]);
    assert.equal(applied.status, 0, applied.stderr);
    const manifest = JSON.parse(applied.stdout.replace(/^\uFEFF/, ""));
    assert.equal(readFileSync(f.config, "utf8"), f.raw.replace(/^trusted_content_hash = "sha256:b{64}"\r\n/gm, ""));
    assert.equal(readFileSync(path.join(manifest.backupRoot, "config.toml"), "utf8"), f.raw);
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});

test("Codex plugin repair rejects stale review hashes before changing either file", { skip: !windows }, () => {
  const f = fixture();
  try {
    const plan = planned(f);
    const beforeHooks = readFileSync(f.hooksPath);
    const edited = f.raw + "# concurrent user edit\r\n";
    writeFileSync(f.config, edited);
    const result = run(f, ["-Apply", "-ExpectedHooksHash", plan.expectedHooksHash, "-ExpectedConfigHash", plan.expectedConfigHash]);
    assert.notEqual(result.status, 0);
    assert.deepEqual(readFileSync(f.hooksPath), beforeHooks);
    assert.equal(readFileSync(f.config, "utf8"), edited);
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});

test("repaired PowerShell hook preserves binary UTF-8 stdin, stdout, stderr and native exit code", { skip: !windows }, () => {
  const f = fixture();
  try {
    const plan = planned(f);
    const input = Buffer.from('{"cwd":"C:/fixture","message":"中文 Ω \\r\\n"}\n');
    const result = spawnSync(powershell, ["-NoProfile", "-Command", plan.commandChanges[0].after], { input, env: { ...process.env, PLUGIN_ROOT: f.plugin }, windowsHide: true, timeout: 15000 });
    assert.equal(result.status, 37, result.stderr.toString());
    assert.deepEqual(result.stdout, input);
    assert.equal(result.stderr.toString("utf8"), "REAL_STDERR_中文");
  } finally { rmSync(f.root, { recursive: true, force: true }); }
});
