import test from "node:test";
import assert from "node:assert/strict";
import { communicationSessions, communicationMembers, communicationExample, communicationGuideSeen, markCommunicationGuideSeen, resolveCommunicationGuideRequest, COMMUNICATION_SEEN_EVENT, COMMUNICATION_SEEN_KEY } from "../src/communicationGuide.ts";

test("通信入口沿用工作台项目，但显式空项目和独立场景不会串入其他项目", () => {
  assert.deepEqual(resolveCommunicationGuideRequest({}, "a"), { scope: "project", projectId: "a" });
  assert.deepEqual(resolveCommunicationGuideRequest({ scope: "project", projectId: null }, "a"), { scope: "project", projectId: null });
  assert.deepEqual(resolveCommunicationGuideRequest({ scope: "sessions" }, "a"), { scope: "sessions", projectId: null });
  assert.deepEqual(resolveCommunicationGuideRequest({ scope: "project", projectId: "b" }, "a"), { scope: "project", projectId: "b" });
  assert.deepEqual(resolveCommunicationGuideRequest({}, null), { scope: "sessions", projectId: null });
});

const claude = { id: "claude", title: "Claude Code", kind: "claude", providerId: "fixture", status: "running", ptyId: 1 };
const codex = { ...claude, id: "codex", title: "Codex CLI", kind: "terminal", providerId: "local-runtime-codex", agentRuntimeKind: "codex", ptyId: 2 };
test("通信示例只统计运行中的交互 Agent，不把 Shell、项目后台成员或旧记录算进去", () => {
  const records = [claude, codex,
    { ...codex, id: "shell", agentRuntimeKind: undefined },
    { ...codex, id: "stopped", status: "stopped" },
    { ...codex, id: "pending", status: "pending" },
    { ...codex, id: "archived", archivedAt: 1 },
    { ...codex, id: "retired", retiredAgentRuntime: true },
    { ...codex, id: "background", multiAgentProjectId: "project" },
    { ...codex, id: "command", runtimeCommandHost: true },
    { ...codex, id: "no-process", ptyId: null },
  ];
  const active = communicationSessions(records);
  assert.deepEqual(active.map(item => item.id), ["claude", "codex"]);
  assert.equal(communicationSessions([{ ...codex, ptyId: null, agentRuntimeKind: "codex_chat", providerId: "local-runtime-codex_chat", status: "waiting" }]).length, 1);
});
test("项目示例只包含当前项目已启用的成员，同 Runtime 的两个成员仍可交流", () => {
  const agents = [{ id: "a", name: "实现者", projectId: "p", runtimeKind: "codex", enabled: true },
    { id: "b", name: "审查员", projectId: "p", runtimeKind: "codex", enabled: true },
    { id: "foreign", name: "其他项目", projectId: "q", runtimeKind: "claude_code", enabled: true },
    { id: "disabled", projectId: "p", runtimeKind: "claude_code", enabled: false },
    { id: "removed", projectId: "p", runtimeKind: "unsupported", enabled: true }];
  const members = communicationMembers(agents, "p");
  assert.deepEqual(members.map(item => item.id), ["a", "b"]);
  assert.match(communicationExample("project", members, "a", "b"), /本项目的「审查员」/);
  assert.equal(communicationExample("project", members, "a", "foreign"), null);
  assert.equal(communicationExample("sessions", members, "a", "a"), null);
});
test("同名对象的复制示例包含明确 ID，不广播且不替换接收方", () => {
  const participants = communicationSessions([claude, codex, { ...codex, id: "another-codex" }]);
  const prompt = communicationExample("sessions", participants, "claude", "another-codex");
  assert.match(prompt, /会话 ID：another-codex/);
  assert.match(prompt, /只联系这个会话/);
});
test("首次提示不依赖会话，关闭或查看教程后沿用已读记录，重新启动仍保持已读", t => {
  const previous = globalThis.window;
  t.after(() => { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; });
  const stored = new Map(), events = [];
  const reopen = () => { globalThis.window = { localStorage: { getItem: key => stored.get(key) ?? null, setItem: (key, value) => stored.set(key, value) }, dispatchEvent: event => events.push(event.type) }; };
  reopen();
  assert.equal(communicationGuideSeen(), false);
  markCommunicationGuideSeen();
  assert.equal(stored.get(COMMUNICATION_SEEN_KEY), "1");
  reopen();
  assert.equal(communicationGuideSeen(), true);
  assert.deepEqual(events, [COMMUNICATION_SEEN_EVENT]);
  // Existing installations retain the same preference key; the change does not reset dismissal.
  stored.set(COMMUNICATION_SEEN_KEY, "1");
  assert.equal(communicationGuideSeen(), true);
});
test("无法保存首次提示偏好时仍通知当前窗口收起提示，并暴露保存失败", t => {
  const previous = globalThis.window;
  t.after(() => { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; });
  const events = [];
  globalThis.window = { localStorage: { getItem() { return null; }, setItem() { throw Error("storage unavailable"); } }, dispatchEvent(event) { events.push(event.type); } };
  assert.equal(communicationGuideSeen(), false);
  assert.throws(markCommunicationGuideSeen, /storage unavailable/);
  assert.deepEqual(events, [COMMUNICATION_SEEN_EVENT]);
});
