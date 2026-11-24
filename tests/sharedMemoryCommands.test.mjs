import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { sharedMemoryCommands, powerShellLiteral, posixShellLiteral } from "../src/lib/sharedMemoryCommands.ts";

const id = "12345678-1234-5678-9abc-123456789abc";
const status = { executable: "/opt/ai memory", dataDir: "/tmp/memory data", serverUrl: "http://127.0.0.1:47374" };

test("收尾命令必须包含所选项目及准确的 ai-memory UUID", () => {
  for (const options of [{}, { projectDir: "/repo" }, { projectDir: "/repo", sessionId: "latest" }, { projectDir: "/repo", sessionId: "zcode-native-id" }, { projectDir: "relative", sessionId: id }]) {
    assert.equal(sharedMemoryCommands(status, { platform: "posix", ...options }).finalize, null);
  }
  const command = sharedMemoryCommands(status, { platform: "posix", projectDir: "/repo", sessionId: id }).finalize;
  assert.match(command, /--session-id '12345678-1234-5678-9abc-123456789abc'/);
  assert.match(command, /^\(cd '\/repo' && env AI_MEMORY_SERVER_URL='http:\/\/127.0.0.1:47374'/);
  assert.doesNotMatch(command, /--all|--reopen|--server-url/);
});

test("平台生成各自 shell 的字面量与安装命令", () => {
  assert.equal(powerShellLiteral("C:\\O'Brien\\$x` tool"), "'C:\\O''Brien\\$x` tool'");
  assert.equal(posixShellLiteral("/O'Brien/$x` tool"), "'/O'\"'\"'Brien/$x` tool'");
  const windows = sharedMemoryCommands(status, { platform: "windows" });
  const posix = sharedMemoryCommands(status, { platform: "posix" });
  assert.equal(windows.shellLabel, "PowerShell");
  assert.equal(posix.shellLabel, "zsh / bash");
  for (const commands of [windows, posix]) {
    assert.equal(commands.hooks.split("\n").length, 7);
    assert.equal(commands.mcp.split("\n").length, 6);
    assert.match(commands.forAgent("pi").hooks, /install-hooks --agent pi /);
    assert.equal(commands.forAgent("pi").mcp, null);
    assert.match(commands.forAgent("opencode").mcp, /install-mcp --client opencode /);
    assert.match(commands.forAgent("antigravity-cli").hooks, /install-hooks --agent antigravity-cli /);
    assert.match(commands.deepSeek.hooks, /install-hooks --agent dsh --server-url 'http:\/\/127\.0\.0\.1:47374' --apply$/);
    assert.match(commands.deepSeek.mcp, /install-mcp --client dsh --server-url 'http:\/\/127\.0\.0\.1:47374'$/);
    assert.doesNotMatch(`${commands.hooks}\n${commands.mcp}`, /(?:--agent|--client) dsh/);
  }
  assert.match(windows.hooks, /^& '/);
  assert.match(posix.hooks, /^'\/opt\/ai memory'/);
  assert.equal(posix.hooks.split("\n").filter(line => line.includes("--scope project")).length, 1);
});

test("Windows 复制命令传递准确参数且不污染父 PowerShell 的服务地址", { skip: process.platform !== "win32" }, () => {
  const root = mkdtempSync(join(tmpdir(), "shared-memory-command-"));
  try {
    const executable = join(root, "O'Brien $task` exe.ps1");
    const project = join(root, "project O'Brien $task` space");
    mkdirSync(project);
    writeFileSync(executable, "[Console]::WriteLine((@{argv=@($args); cwd=(Get-Location).Path; server=$env:AI_MEMORY_SERVER_URL} | ConvertTo-Json -Compress))\n");
    const command = sharedMemoryCommands({ executable, dataDir: join(root, "data O'Brien"), serverUrl: status.serverUrl }, { platform: "windows", projectDir: project, sessionId: id }).finalize;
    const script = `$env:AI_MEMORY_SERVER_URL = 'http://parent.invalid'; ${command}; [Console]::WriteLine('PARENT=' + $env:AI_MEMORY_SERVER_URL)`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", windowsHide: true });
    assert.equal(result.status, 0, result.stderr);
    const lines = result.stdout.trim().split(/\r?\n/);
    const record = JSON.parse(lines[0]);
    assert.deepEqual(record.argv, ["--data-dir", join(root, "data O'Brien"), "finalize-session", "--agent", "zcode", "--session-id", id]);
    assert.equal(record.cwd, project);
    assert.equal(record.server, status.serverUrl);
    assert.equal(lines[1], "PARENT=http://parent.invalid");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("POSIX 收尾命令在子 shell 中执行并保护带引号和美元符号的参数", { skip: process.platform === "win32" }, () => {
  const root = mkdtempSync(join(tmpdir(), "shared-memory-command-"));
  try {
    const executable = join(root, "memory O'Brien $task");
    const project = join(root, "project O'Brien $task");
    mkdirSync(project);
    writeFileSync(executable, '#!/bin/sh\nprintf "%s\\n" "$AI_MEMORY_SERVER_URL" "$PWD" "$@"\n', { mode: 0o700 });
    const command = sharedMemoryCommands({ executable, dataDir: root, serverUrl: status.serverUrl }, { platform: "posix", projectDir: project, sessionId: id }).finalize;
    const result = spawnSync("/bin/sh", ["-c", `${command}; printf 'PARENT=%s\\n' "$AI_MEMORY_SERVER_URL"`], { encoding: "utf8", env: { ...process.env, AI_MEMORY_SERVER_URL: "http://parent.invalid" } });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.stdout.trim().split("\n"), [status.serverUrl, project, "--data-dir", root, "finalize-session", "--agent", "zcode", "--session-id", id, "PARENT=http://parent.invalid"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
