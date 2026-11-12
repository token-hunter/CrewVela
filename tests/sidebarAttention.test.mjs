import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { attentionKey, attentionOpenMode, attentionSummary, readDismissedAttention, sessionAttentionItems } from "../src/sidebarAttention.ts";

const session = (id, extra = {}) => ({ id, title: id, kind: "claude", providerId: "fixture", providerName: "Fixture", status: "running", ptyId: 1, archivedAt: null, updatedAt: 100, lastError: null, ...extra });
test("普通等待、运行、完成、停止及归档均不产生待处理提醒", () => {
  const sessions = ["running", "pending", "completed", "stopped", "waiting"].map(status => session(status, { status }));
  sessions.push(session("idle", { status: "waiting", waitingReason: "等待用户输入" }), session("archive", { status: "error", archivedAt: 0 }), session("retired", { status: "error", retiredAgentRuntime: true }));
  assert.equal(sessionAttentionItems(sessions).length, 0);
  assert.equal(attentionSummary([]), "");
});
test("跨 Agent 类型显示当前传入作用域，授权优先且按更新时间排序", () => {
  const a = session("claude", { status: "waiting", waitingReason: "Claude needs your permission to use Bash" });
  const b = session("codex", { kind: "terminal", agentRuntimeKind: "codex_chat", providerId: "local-runtime-codex_chat", ptyId: null, status: "error", updatedAt: 300 });
  const c = session("kimi", { status: "waiting", waitingReason: "请求审批", updatedAt: 200 });
  assert.deepEqual(sessionAttentionItems([b, a, c, b]).map(i => i.session.id), ["kimi", "claude", "codex"]);
  assert.equal(attentionSummary(sessionAttentionItems([b, a, c])), "待授权 2 · 执行失败 1");
  assert.deepEqual(sessionAttentionItems([b]).map(i => i.session.id), ["codex"]);
});
test("失去进程的 PTY 不继续提示待授权，原生运行中的待授权仍可显示", () => {
  const stale = session("stale", { ptyId: null, status: "waiting", waitingReason: "授权" });
  const runtimes = ["claude_chat", "codex_chat", "kimi_chat", "opencode_chat", "pi_chat", "deepseek_harness", "zcode"];
  const native = runtimes.map(runtime => session(runtime, { ptyId: null, kind: "terminal", providerId: `local-runtime-${runtime}`, agentRuntimeKind: runtime, status: "waiting", waitingReason: "Read file" }));
  assert.deepEqual(sessionAttentionItems([stale, ...native]).map(i => i.session.id), runtimes);
});
test("忽略仅影响同一轮失败，新失败重新提醒，不忽略授权", () => {
  const failed = session("failed", { status: "error", lastError: "secret-fixture-error" });
  const key = attentionKey(failed, "error");
  assert.ok(!key.includes("secret-fixture-error"));
  assert.equal(sessionAttentionItems([failed], [key]).length, 0);
  assert.equal(sessionAttentionItems([{ ...failed, updatedAt: 101 }], [key]).length, 1);
  const approval = session("approval", { status: "waiting", waitingReason: "approval" });
  assert.equal(sessionAttentionItems([approval], [attentionKey(approval, "approval")]).length, 1);
});
test("错误查看和失效授权走只读记录，不把查看变成任务重试", () => {
  assert.equal(attentionOpenMode(session("a", { status: "error" }), "error"), "history");
  assert.equal(attentionOpenMode(session("b", { status: "stopped", ptyId: null }), "approval"), "history");
  assert.equal(attentionOpenMode(session("c", { status: "running" }), "approval"), "history");
  assert.equal(attentionOpenMode(session("d", { status: "waiting" }), "approval"), "session");
});
test("忽略偏好格式校验和数量上限", () => {
  assert.deepEqual(readDismissedAttention(null), []);
  assert.deepEqual(readDismissedAttention('["a","a","b"]'), ["a", "b"]);
  for (const raw of ["{", "{}", "[1]"]) assert.throws(() => readDismissedAttention(raw));
  assert.equal(readDismissedAttention(JSON.stringify(Array.from({length: 600}, (_, i) => String(i)))).length, 500);
});
test("首页移除旧额度请求链路，并把当前工作区会话交给待处理入口", () => {
  const sidebar = readFileSync(new URL("../src/components/AppSidebar.tsx", import.meta.url), "utf8");
  const attention = readFileSync(new URL("../src/components/SidebarAttention.tsx", import.meta.url), "utf8");
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(sidebar + attention + app, /CodexQuotaSummary|onOpenCodexAccounts|codexAccountsOpen|loadCodexAccountSnapshot/);
  assert.match(sidebar, /<SidebarAttention sessions=\{props.sessions\}/);
  assert.match(app, /sessions=\{tabSessions\}/);
  assert.match(app, /attentionOpenMode\(current, kind\).*\{ setTranscriptFor\(current\); return; \}/);
  assert.doesNotMatch(attention, /\binvoke\(|setInterval\(|window.addEventListener\("focus"/);
});
