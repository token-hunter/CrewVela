import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));

test("公开发布规则隔离本地敏感文件并保留源码、测试、许可与素材", (t) => {
  const directory = mkdtempSync(path.join(tmpdir(), "crewvela-publication-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  writeFileSync(path.join(directory, ".gitignore"), readFileSync(path.join(root, ".gitignore")));
  writeFileSync(path.join(directory, "empty-global-ignore"), "");
  execFileSync("git", ["init", "--quiet", directory]);
  const privatePaths = [
    ".env", ".env.local", ".env.production", "nested/.env", "nested/.env.production.local",
    ".env.example.local", ".envrc", ".npmrc", "secrets/service.json", "export/credentials.json",
    "export/auth.json", "signing/release.key", "signing/release.p12", "signing/release.pem",
    "cookies.json", "storage-state-account.json", "storageState.json", "state/accounts.sqlite3",
    "state/cache.db-wal", ".claude/settings.local.json", ".claude/mailbox/messages.json",
    ".codex/sessions/session.jsonl", ".impeccable/hook.cache.json", ".scratch/audit/report.json",
    "src-tauri/target/release/crewvela.exe", "dist/index.html", "node_modules/package/index.js",
    "downloads/model.onnx", "downloads/model.gguf", "downloads/model.safetensors",
    "qa-local-probe.mjs", "qa-local-capture.png", "baidu-home.png", "task_plan.md", "$dest/copy.txt",
  ];
  const publicPaths = [
    ".env.example", "nested/.env.example", ".gitleaks.toml", "README.md", "LICENSE",
    "package-lock.json", "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json",
    "src/App.tsx", "src/assets/agent-codex.png", "src-tauri/icons/icon.ico",
    "src-tauri/vendor/portable-pty/src/lib.rs", "THIRD_PARTY_NOTICES/provider-quota.md",
    "tests/fixtures/agent.mjs", "tests/browserControlLazyBridge.test.mjs",
    "qa-general-settings.mjs", "qa-kimi-computer-use.mjs", "pet-preview.html",
    ".claude/commands/review.md", ".impeccable/config.json", "docs/plans/example/task_plan.md",
  ];
  const ignored = execFileSync("git", [
    "-c", `core.excludesFile=${path.join(directory, "empty-global-ignore")}`,
    "check-ignore", "--no-index", "--stdin",
  ], { cwd: directory, input: [...privatePaths, ...publicPaths].join("\n") + "\n", encoding: "utf8" });
  assert.deepEqual(new Set(ignored.trim().split(/\r?\n/)), new Set(privatePaths));
});

test("环境示例不携带默认秘密，也不声明向前端公开的变量", () => {
  const example = readFileSync(path.join(root, ".env.example"), "utf8");
  const assignments = example.split(/\r?\n/)
    .map((line) => line.replace(/^\s*#\s?/, "").trim())
    .filter((line) => /^[A-Z][A-Z0-9_]*\s*=/.test(line));
  assert.ok(assignments.some((line) => line.startsWith("AI_MEMORY_AUTH_TOKEN=")));
  for (const line of assignments) {
    assert.match(line, /^[A-Z][A-Z0-9_]*=$/);
    assert.ok(!line.startsWith("VITE_"), "秘密不能通过 Vite 注入前端");
  }
});
