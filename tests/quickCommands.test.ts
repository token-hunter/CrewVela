import test from "node:test";
import assert from "node:assert/strict";
import { emptyQuickSettings, isQuickSessionLive, quickCommandCatalog, readQuickSettings, scopeSessions, validateQuickCommand, writeQuickSettings, type QuickCommand } from "../src/quickCommands.ts";
import { applyDeliveryEvidence, executeQuickPlan } from "../src/quickCommandExecution.ts";
import { resolveUnifiedControlRoute } from "../src/unifiedControlRouter.ts";
import type { RuntimeProbe, SessionRecord, ProviderConfig } from "../src/types.ts";
const session = (id: string, changes: Partial<SessionRecord> = {}): SessionRecord => ({ id, title: id, kind: "terminal", agentRuntimeKind: "codex_chat", status: "completed", ptyId: null, cwd: "/fixture/project", providerId: "local", providerName: "Codex Chat", projectId: "project-a", ...changes } as SessionRecord);
const command: QuickCommand = { id: "builtin:send", title: "发送", action: "send", scope: "workspace", body: "检查当前项目并给出下一步建议" };
test("范围依据工作区与项目身份筛选，不会把相同目录的其他会话算进来", () => {
  const rows = [session("a"), session("b", { projectId: "other" }), session("stopped", { status: "stopped", ptyId: 7 }), session("shell", { agentRuntimeKind: null }), session("archived", { archivedAt: 1 }), session("host", { runtimeCommandHost: true })];
  assert.deepEqual(scopeSessions(rows, "workspace", { workspaceSessionIds: ["a"] }, "send").map(s => s.id), ["a"]);
  assert.deepEqual(scopeSessions(rows, "project", { projectId: "other" }, "send").map(s => s.id), ["b"]);
  assert.deepEqual(scopeSessions(rows, "all", {}, "send").map(s => s.id), ["a", "b"]);
  assert.equal(scopeSessions(rows, "workspace", {}, "send").length, 0);
  assert.equal(scopeSessions(rows, "project", {}, "send").length, 0);
  assert.equal(scopeSessions(rows, "all", {}, "delete").length, 3);
});
test("原生空闲会话可以投递；失败、停止和退役会话不能伪装为运行中", () => {
  assert.ok(isQuickSessionLive(session("native")));
  for (const changes of [{ status: "error" }, { status: "stopped", ptyId: 2 }, { retiredAgentRuntime: true }, { agentRuntimeKind: "minimax_code" }]) assert.equal(isQuickSessionLive(session("bad", changes as Partial<SessionRecord>)), false);
});
test("目录只推荐本机已安装且启用的 CLI；Provider 还须配置凭据", () => {
  const probes = ["claude_code", "codex", "pi"].map(kind => ({ kind, installed: true, error: null })) as RuntimeProbe[];
  const providers = [{ id: "configured", name: "示例" }, { id: "empty", name: "未配置" }, { id: "hidden", name: "隐藏", hidden: true }] as ProviderConfig[];
  const catalog = quickCommandCatalog(probes, providers, { configured: true, hidden: true }, kind => kind !== "pi");
  assert.ok(catalog.some(c => c.id === "runtime:claude_code"));
  assert.ok(catalog.some(c => c.id === "provider:configured"));
  assert.ok(catalog.some(c => c.id === "builtin:review"));
  assert.ok(!catalog.some(c => c.id === "runtime:pi" || c.id === "provider:empty" || c.id === "provider:hidden"));
  assert.ok(!catalog.some(c => c.runtimeKind === "opencode"));
});
test("自定义、收藏、最近、默认范围可往返保存；损坏或写入失败不覆盖原数据", () => {
  let raw: string | null = null;
  const storage = { getItem: () => raw, setItem: (_: string, value: string) => { raw = value; } };
  const settings = { ...emptyQuickSettings(), commands: [{ ...command, id: "custom:a" }], favorites: ["custom:a"], recent: ["custom:a"], defaultScope: "selected" as const };
  writeQuickSettings(storage, settings);
  assert.deepEqual(readQuickSettings(storage), settings);
  const launch = { ...settings, commands: [{ ...command, id: "custom:launch", action: "launch" as const, runtimeKind: "claude_code" as const, providerId: "configured", cwd: "/fixture/project with spaces", count: 2 }] };
  writeQuickSettings(storage, launch);
  assert.deepEqual(readQuickSettings(storage), launch);
  assert.throws(() => validateQuickCommand({ ...launch.commands[0], count: 1.5 }));
  assert.throws(() => validateQuickCommand({ ...command, action: "toString" }));
  assert.throws(() => validateQuickCommand({ ...command, scope: "__proto__" }));
  for (const broken of ["null", "{", '{"version":2}']) { raw = broken; assert.throws(() => readQuickSettings(storage)); assert.equal(raw, broken); }
  assert.throws(() => writeQuickSettings({ setItem: () => { throw Error("quota"); } }, settings), /quota/);
});
test("结构化发送逐目标投递，正文里的项目操作不触发解析；部分失败不中断其余目标", async () => {
  const rows = [session("a"), session("b"), session("c")];
  const calls: Array<{ method: string; args: any }> = [];
  let uuid = 0;
  const receipts = await executeQuickPlan({ command, targets: rows, cwd: "", count: 1 }, {
    invoke: async <T>(method: string, args?: Record<string, unknown>) => {
      calls.push({ method, args });
      if (method === "session_snapshot") return rows as T;
      assert.equal(method, "send_session_message");
      if (args?.sessionId === "b") throw Error("拒绝投递");
      return "accepted" as T;
    }, enabled: () => true, uuid: () => `request-${++uuid}`, progress: () => {},
  });
  assert.deepEqual(receipts.map(r => r.state), ["queued", "failed", "queued"]);
  assert.deepEqual(calls.filter(c => c.method === "send_session_message").map(c => c.args.body), Array(3).fill(command.body));
  assert.equal(new Set(receipts.filter(r => r.requestId).map(r => r.requestId)).size, 2);
  assert.equal(resolveUnifiedControlRoute('把“检查当前项目并给出下一步建议”发送给所有会话'), "quick");
});
test("执行时重新核对冻结目标，不扩大范围；身份变化、消失、禁用和停止均拒绝", async () => {
  const original = session("original");
  for (const changed of [[], [session("other")], [session("original", { cwd: "/changed" })], [session("original", { status: "stopped" })]]) {
    const receipts = await executeQuickPlan({ command, targets: [original], cwd: "", count: 1 }, {
      invoke: async <T>(method: string) => { assert.equal(method, "session_snapshot"); return changed as T; }, enabled: () => true, uuid: () => "unused", progress: () => {},
    });
    assert.equal(receipts[0].state, "failed");
  }
  const disabled = await executeQuickPlan({ command, targets: [original], cwd: "", count: 1 }, { invoke: async <T>() => [original] as T, enabled: () => false, uuid: () => "unused", progress: () => {} });
  assert.equal(disabled[0].state, "failed");
});
test("启动沿用本地配置；任务投递失败时明确保留已启动会话", async () => {
  const calls: string[] = [];
  const receipts = await executeQuickPlan({ command: { ...command, action: "launch", runtimeKind: "codex" }, targets: [], cwd: "/fixture/project", count: 2 }, {
    invoke: async <T>(method: string, args?: Record<string, unknown>) => {
      calls.push(method);
      if (method === "launch_runtime_session") { assert.equal(args?.runtimeKind, "codex"); assert.equal(args?.cwd, "/fixture/project"); return session(`created-${calls.length}`) as T; }
      throw Error("投递失败");
    }, enabled: () => true, uuid: () => "request", progress: () => {},
  });
  assert.equal(receipts.length, 2);
  assert.ok(receipts.every(r => r.state === "failed" && r.detail?.includes("会话仍保留")));
  assert.ok(!calls.some(c => c === "delete_session" || c === "interpret_control_message"));
});
test("停止与删除使用逐会话接口；停止失败不会错误显示成功", async () => {
  for (const action of ["stop", "delete"] as const) {
    const rows = [session("a"), session("b")];
    const receipts = await executeQuickPlan({ command: { ...command, action }, targets: rows, cwd: "", count: 1 }, {
      invoke: async <T>(method: string, args?: Record<string, unknown>) => {
        if (method === "session_snapshot") return rows as T;
        assert.equal(method, action === "stop" ? "stop_session" : "delete_session");
        if (args?.sessionId === "b") throw Error("保存历史失败");
        return undefined as T;
      }, enabled: () => true, uuid: () => "unused", progress: () => {},
    });
    assert.deepEqual(receipts.map(r => r.state), [action === "stop" ? "stopped" : "removed", "failed"]);
  }
});
test("运输证据必须同时匹配请求与目标；写入不等于已读或任务完成", () => {
  const receipt = [{ targetId: "a", title: "a", requestId: "r", state: "queued" as const }];
  assert.deepEqual(applyDeliveryEvidence(receipt, [{ id: "r", toSessionId: "other", stage: "text_written" }]), receipt);
  assert.equal(applyDeliveryEvidence(receipt, [{ id: "r", toSessionId: "a", stage: "text_written" }])[0].state, "text_written");
  const retry = applyDeliveryEvidence(receipt, [{ id: "r", toSessionId: "a", stage: "queued", error: "busy" }])[0];
  assert.equal(retry.state, "queued"); assert.match(retry.detail!, /重试/);
});
