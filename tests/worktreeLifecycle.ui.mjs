import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright";
import { createServer } from "vite";

// Serve a fixture with the production component and a controlled Tauri IPC surface.
const fixturePath = "/worktree-lifecycle-fixture.js";
const cacheDir = await mkdtemp(join(tmpdir(), "crewvela-lifecycle-vite-"));
const server = await createServer({
  cacheDir,
  configFile: false,
  logLevel: "error",
  esbuild: { jsx: "automatic" },
  server: { host: "127.0.0.1", port: 0, watch: null, hmr: false },
  plugins: [{
    name: "worktree-lifecycle-fixture",
    resolveId(id) { if (id === fixturePath) return fixturePath; },
    load(id) {
      if (id !== fixturePath) return;
      return [
        'import React, {useState} from "react";',
        'import {createRoot} from "react-dom/client";',
        'import {WorktreeLifecycleControls} from "/src/components/WorktreeLifecycleControls.tsx";',
        'import "/src/styles.css";',
        'function Fixture() {',
        '  const [revision, setRevision] = useState(0);',
        '  window.refreshOverview = () => setRevision(r => r + 1);',
        '  return React.createElement(WorktreeLifecycleControls, {repoRoot: "D:/fixture", sessionId: "session", refreshKey: revision});',
        '}',
        'createRoot(document.getElementById("root")).render(React.createElement(Fixture));',
      ].join("\n");
    },
  }],
});

let browser;
try {
  await server.listen();
  browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
  const page = await browser.newPage({ viewport: { width: 1150, height: 850 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/worktree-lifecycle-fixture.html", (route) => route.fulfill({
    contentType: "text/html",
    body: '<html lang="zh"><meta charset="utf-8"><body><main id="root"></main><script type="module" src="/worktree-lifecycle-fixture.js"></script></body></html>',
  }));
  await page.addInitScript(() => {
    const state = {
      policy: { command: "", files: [], autoCleanup: false },
      snapshots: [], run: null, warning: null,
    };
    window.fixture = state;
    window.calls = [];
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
      window.calls.push([cmd, args]);
      switch (cmd) {
        case "worktree_lifecycle_get": return structuredClone(state);
        case "worktree_lifecycle_save": state.policy = structuredClone(args.policy); return;
        case "worktree_lifecycle_prepare":
          if (!state.run) {
            state.run = { state: "failed", updatedAtMs: 1, lines: ["fixture failure"] };
            throw new Error("fixture failure");
          }
          state.run = { state: "ready", updatedAtMs: 2, lines: ["fixture success"] };
          return;
        case "worktree_lifecycle_restore": return;
        default: throw new Error(cmd);
      }
    } };
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/worktree-lifecycle-fixture.html`);
  await page.getByText("环境准备与恢复", { exact: true }).click();
  const command = page.getByRole("textbox", { name: "准备命令", exact: true });
  const cleanup = page.getByRole("checkbox", { name: "归档时自动回收" });
  await command.waitFor();
  assert.equal(await cleanup.isChecked(), false);
  await command.fill("echo ready");
  await page.getByRole("textbox", { name: "所需文件" }).fill("config/local.env\n.env.local");
  await cleanup.check();
  await page.getByRole("button", { name: "保存项目配置" }).click();
  await page.getByText("已保存此项目的配置").waitFor();
  assert.deepEqual(await page.evaluate(() => window.fixture.policy), {
    command: "echo ready", files: ["config/local.env", ".env.local"], autoCleanup: true,
  });
  await page.getByRole("button", { name: "执行准备" }).click();
  await page.getByRole("alert").filter({ hasText: "fixture failure" }).waitFor();
  await page.getByRole("button", { name: "重试准备" }).click();
  await page.getByText("准备日志 · 准备完成").waitFor();
  await command.fill("unsaved draft");
  await page.evaluate(() => {
    window.fixture.snapshots = [{
      id: "snapshot-one", sessionId: "session", branch: "crewvela/session", path: "D:/managed/session",
      commit: "12345678901", gitRef: "refs/crewvela/worktree-snapshots/mock/head", archivePath: "D:/state/checkout.tar.gz", createdAtMs: 2,
    }];
    window.refreshOverview();
  });
  await page.getByText("恢复快照（1）").click();
  assert.equal(await command.inputValue(), "unsaved draft");
  await page.getByRole("button", { name: "恢复到原路径", exact: true }).click();
  await page.getByText("已恢复到原 Worktree 路径").waitFor();
  assert.deepEqual(await page.evaluate(() => window.calls.filter(([cmd]) => cmd === "worktree_lifecycle_restore").map(([, args]) => args)),
    [{ repoRoot: "D:/fixture", snapshotId: "snapshot-one" }]);
  assert.deepEqual(errors, []);
  console.log("PASS worktree policy save, failed preparation/retry, snapshot refresh, unsaved draft and restore");
} finally {
  await browser?.close();
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
