import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  parseBridgeArgs,
  stripLaunchOnlyArgs,
  resolveMode,
  composeSpawn,
} from "../src-tauri/resources/browser-control-bridge.mjs";

test("parseBridgeArgs 以 -- 切分引擎与浏览器参数", () => {
  const parsed = parseBridgeArgs([
    "node.exe",
    "C:\\mcp\\cli.js",
    "--",
    "--browser",
    "chrome",
    "--isolated",
  ]);
  assert.deepEqual(parsed, {
    engineCommand: "node.exe",
    engineArgs: ["C:\\mcp\\cli.js"],
    browserArgs: ["--browser", "chrome", "--isolated"],
  });
  assert.equal(parseBridgeArgs(["node.exe"]), null);
  assert.equal(parseBridgeArgs(["--", "--browser", "chrome"]), null);
});

test("stripLaunchOnlyArgs 移除启动语义参数（分体与连体值）", () => {
  assert.deepEqual(
    stripLaunchOnlyArgs(["--browser", "msedge", "--isolated", "--viewport-size", "1280x720", "--headless"]),
    ["--viewport-size", "1280x720"],
  );
  assert.deepEqual(
    stripLaunchOnlyArgs(["--executable-path", "C:\\chrome.exe", "--browser=chrome", "--keep"]),
    ["--keep"],
  );
});

test("composeSpawn 面板模式改写为 cdp-endpoint，外部模式原样透传", () => {
  const base = {
    engineCommand: "node.exe",
    engineArgs: ["C:\\mcp\\cli.js"],
    browserArgs: ["--browser", "msedge", "--isolated"],
  };
  const panel = composeSpawn({ ...base, mode: "panel", endpoint: "http://127.0.0.1:52341" });
  assert.equal(panel.command, "node.exe");
  assert.deepEqual(panel.args, [
    "C:\\mcp\\cli.js",
    "--cdp-endpoint",
    "http://127.0.0.1:52341",
  ]);
  assert.equal(panel.useShell, false);

  const external = composeSpawn({ ...base, mode: "external" });
  assert.deepEqual(external.args, ["C:\\mcp\\cli.js", "--browser", "msedge", "--isolated"]);
});

test("Windows npm wrapper 通过真实 Node 子进程保留自定义浏览器路径和参数字面值", () => {
  const root = mkdtempSync(path.join(tmpdir(), "browser bridge 中文 & () % ! "));
  try {
    const npmBin = path.join(root, "node_modules", "npm", "bin");
    mkdirSync(npmBin, { recursive: true });
    const cli = path.join(npmBin, "npx-cli.js");
    writeFileSync(cli, "console.log(JSON.stringify(process.argv.slice(2)))");
    const customBrowser = path.join(root, "Browser 中文 & (custom) %UNCHANGED% !literal!", "chrome.exe");
    mkdirSync(path.dirname(customBrowser));
    writeFileSync(customBrowser, "fixture path only; never executed");
    const browserArgs = ["--executable-path", customBrowser, "--isolated", "--literal=中文 & % ! ()"];
    for (const extension of ["cmd", "bat"]) {
      const wrapper = path.join(root, `npx.${extension}`);
      writeFileSync(wrapper, '@echo off\r\n"%~dp0\\node.exe" "%~dp0\\node_modules\\npm\\bin\\npx-cli.js" %*\r\n');
      const config = { engineCommand: wrapper, engineArgs: ["--yes", "@playwright/mcp@0.0.83"], browserArgs, platform: "win32" };
      const external = composeSpawn({ ...config, mode: "external" });
      const child = spawnSync(external.command, external.args, { shell: external.useShell, encoding: "utf8", windowsHide: true, timeout: 5000 });
      assert.ifError(child.error);
      assert.equal(child.status, 0, child.stderr);
      assert.deepEqual(JSON.parse(child.stdout), [...config.engineArgs, ...browserArgs]);
      const panel = composeSpawn({ ...config, mode: "panel", endpoint: "http://127.0.0.1:52341" });
      const panelChild = spawnSync(panel.command, panel.args, { shell: panel.useShell, encoding: "utf8", windowsHide: true, timeout: 5000 });
      assert.ifError(panelChild.error);
      assert.equal(panelChild.status, 0, panelChild.stderr);
      assert.deepEqual(JSON.parse(panelChild.stdout), [...config.engineArgs, "--cdp-endpoint", "http://127.0.0.1:52341", "--literal=中文 & % ! ()"]);
    }
    const unsupported = path.join(root, "custom.cmd");
    writeFileSync(unsupported, "@echo unsupported fixture");
    assert.throws(() => composeSpawn({ engineCommand: unsupported, engineArgs: [], browserArgs, mode: "external", platform: "win32" }), /不支持安全启动/);
    const missing = path.join(root, "missing.cmd");
    writeFileSync(missing, '@node "%~dp0\\node_modules\\npm\\bin\\missing.js" %*');
    assert.throws(() => composeSpawn({ engineCommand: missing, engineArgs: [], browserArgs, mode: "external", platform: "win32" }), /不支持安全启动/);
    const incomplete = path.join(root, "incomplete install");
    mkdirSync(incomplete);
    const incompleteWrapper = path.join(incomplete, "npx.cmd");
    writeFileSync(incompleteWrapper, '@node "%~dp0\\node_modules\\npm\\bin\\npx-cli.js" %*');
    assert.throws(() => composeSpawn({ engineCommand: incompleteWrapper, engineArgs: [], browserArgs, mode: "external", platform: "win32" }), /缺少 JavaScript 入口/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

async function closeFixtureServer(server) {
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("fixture HTTP server failed to close within 1000ms")), 1000);
    server.close((error) => {
      clearTimeout(timer);
      if (error && error.code !== "ERR_SERVER_NOT_RUNNING") reject(error);
      else resolve();
    });
    // fetch may retain keep-alive connections even when an assertion failed.
    server.closeAllConnections?.();
  });
}

test("resolveMode 探活成功才选面板，端口文件缺失/超时/坏格式都退外部", { timeout: 5000 }, async (t) => {
  const servers = [];
  const dir = mkdtempSync(path.join(tmpdir(), "cc-bridge-test-"));
  t.after(async () => {
    const cleanup = await Promise.allSettled(servers.map(closeFixtureServer));
    rmSync(dir, { recursive: true, force: true });
    const errors = cleanup.filter((result) => result.status === "rejected").map((result) => result.reason);
    if (errors.length) throw new AggregateError(errors, "fixture cleanup failed");
  });
  // 1) 活的假 CDP 端点。
  const server = createServer((request, response) => {
    if (request.url === "/json/version") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ Browser: "Edg/fake" }));
    } else {
      response.writeHead(404);
      response.end();
    }
  });
  servers.push(server);
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const port = server.address().port;

  const configFile = path.join(dir, "panel-cdp.json");

  writeFileSync(configFile, JSON.stringify({ port, startedAt: 1 }));
  assert.deepEqual(await resolveMode(configFile), {
    mode: "panel",
    endpoint: `http://127.0.0.1:${port}`,
  });

  // 2) 非 /json/version 语义之外：404 → 外部。
  const notFound = createServer((request, response) => {
    response.writeHead(404);
    response.end();
  });
  servers.push(notFound);
  await new Promise((resolve, reject) => { notFound.once("error", reject); notFound.listen(0, "127.0.0.1", resolve); });
  writeFileSync(configFile, JSON.stringify({ port: notFound.address().port, startedAt: 1 }));
  assert.deepEqual(await resolveMode(configFile), { mode: "external" });

  // 3) 端口无人监听 → 外部。
  writeFileSync(configFile, JSON.stringify({ port: 1, startedAt: 1 }));
  assert.deepEqual(await resolveMode(configFile), { mode: "external" });

  // 4) 文件缺失 / 坏 JSON / 坏端口 → 外部。
  assert.deepEqual(await resolveMode(path.join(dir, "missing.json")), { mode: "external" });
  writeFileSync(configFile, "{broken");
  assert.deepEqual(await resolveMode(configFile), { mode: "external" });
  // 5) waitMs>0：连不上按冷启动窗口退避等待，耗尽后外部。
  const start = Date.now();
  writeFileSync(configFile, JSON.stringify({ port: 1, startedAt: 1 }));
  assert.deepEqual(await resolveMode(configFile, { waitMs: 700 }), { mode: "external" });
  assert.ok(Date.now() - start >= 650, "等待期应覆盖 waitMs");
  const quick = Date.now();
  assert.deepEqual(await resolveMode(configFile, { waitMs: 0 }), { mode: "external" });
  assert.ok(Date.now() - quick < 400, "waitMs=0 应立即外部");
  writeFileSync(configFile, JSON.stringify({ port: "not-a-number" }));
  assert.deepEqual(await resolveMode(configFile), { mode: "external" });

});
