import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { hasMentionToken } from "../src/multiAgentMention.ts";

const source = readFileSync(new URL("../src/components/MultiAgentControl.tsx", import.meta.url), "utf8");
const ast = ts.createSourceFile("control.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const names = new Set(["resolveMentionTargetRuntimeKinds", "resolveExplicitMentionTargetAgentIds", "resolveSessionMentionTargets", "isMentionBindingAvailable", "countMentionTargets"]);
const selected = ast.statements.filter(node => ts.isFunctionDeclaration(node) && names.has(node.name?.text)).map(node => node.getText(ast)).join("\n");
const ctx = vm.createContext({ hasMentionToken });
vm.runInContext(ts.transpileModule(selected, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText + "\nglobalThis.routing = { resolveMentionTargetRuntimeKinds, resolveExplicitMentionTargetAgentIds, resolveSessionMentionTargets, countMentionTargets };", ctx);
const options = [
  { kind: "runtime", token: "@ZCodeCLI", runtimeKind: "zcode_cli", targetCount: 0 },
  { kind: "runtime", token: "@ZCode", runtimeKind: "zcode" },
  { kind: "runtime", token: "@Codex", runtimeKind: "codex" },
  { kind: "session", token: "@ZCodeCLI-2", runtimeKind: "zcode_cli", sessionId: "live-cli" },
  { kind: "agent", token: "@审查员", runtimeKind: "codex", agentId: "reviewer" },
  { kind: "all", token: "@All" },
];
const runtimes = value => Array.from(ctx.routing.resolveMentionTargetRuntimeKinds(value, options));

test("截图原句保留 CLI 群组身份，正文不能追加 Desktop", () => {
  assert.deepEqual(runtimes("@ZCodeCLI @Codex zcode和codex打个招呼"), ["zcode_cli", "codex"]);
  assert.deepEqual(runtimes("@zcodecli @Codex 你们互相打个招呼"), ["zcode_cli", "codex"]);
  assert.deepEqual(runtimes("@ZCode @ZCodeCLI @ZCodeCLI 打个招呼"), ["zcode_cli", "zcode"]);
});

test("实例和 All 保持精确身份，实例后缀不误识别为群组", () => {
  assert.deepEqual(runtimes("@ZCodeCLI-2 @审查员 zcode和codex打个招呼"), []);
  const snapshot = { agents: [{ id: "reviewer", enabled: true }, { id: "disabled", enabled: false }] };
  assert.deepEqual(Array.from(ctx.routing.resolveExplicitMentionTargetAgentIds("@All 打个招呼", options, snapshot, [])), ["reviewer"]);
  assert.deepEqual(Array.from(ctx.routing.resolveSessionMentionTargets("@ZCodeCLI-2 发消息", options, [{ id: "live-cli" }]), item => item.session.id), ["live-cli"]);
});

test("通信 IPC 包含 Runtime 枚举，未把它仅留在候选项中", () => {
  assert.match(source, /targetRuntimeKinds:\s*resolveMentionTargetRuntimeKinds\(\s*message,\s*mentionOptions,?\s*\)/);
});

test("发送提示统计尚无项目成员的 CLI 群组，实例和群组不重复计数", () => {
  const snapshot = { agents: [{id: "codex", runtimeKind: "codex"}] };
  assert.equal(ctx.routing.countMentionTargets(["codex"], ["zcode_cli", "codex"], [], snapshot), 2);
  assert.equal(ctx.routing.countMentionTargets(["codex"], ["zcode_cli", "codex"], [{id: "cli", agentRuntimeKind: "zcode_cli"}, {id: "project-codex", agentRuntimeKind: "codex", multiAgentAgentId: "codex"}], snapshot), 2);
});

test("通信后台失败只反馈本次投递，保留原因且不重复播放历史失败", () => {
  let effect;
  const visit = node => {
    if (ts.isCallExpression(node) && node.expression.getText(ast) === "useEffect"
      && node.arguments[0]?.getText(ast).includes("const directTasks = directDeliveryTasks")) effect = node.arguments[0];
    ts.forEachChild(node, visit);
  };
  visit(ast);
  assert.ok(effect);
  const replies = [];
  const direct = new Map([["p", new Set(["delivery-1"])]]);
  const state = {
    snapshot: { project: { id: "p", status: "completed" }, tasks: [
      { id: "old", title: "旧任务", status: "failed", updatedAt: 1 },
      { id: "delivery-1", title: "ZCode CLI 消息", status: "failed", updatedAt: 2 },
    ], runs: [{ taskId: "delivery-1", createdAt: 2, error: "测试：CLI 启动失败" }] },
    autoReportProjects: { current: new Set() }, directDeliveryTasks: { current: direct },
    reportedFailedTasks: { current: new Map() }, reportedApprovalTasks: { current: new Map() },
    failedCollaborationMessages: { current: new Map() }, MAX_COORDINATOR_HISTORY: 200,
    appendCoordinatorMessage: (_role, text) => replies.push(text), persistAutoReportState() {},
    collaborationResultMessage() { throw new Error("通信失败不能宣布整个项目已完成"); },
  };
  const context = vm.createContext(state);
  vm.runInContext(ts.transpileModule(`globalThis.report = ${effect.getText(ast)}`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  context.report();
  context.report();
  assert.equal(replies.length, 1);
  assert.match(replies[0], /测试：CLI 启动失败/);
  assert.doesNotMatch(replies[0], /旧任务/);
  assert.equal(direct.size, 0);
  assert.match(state.failedCollaborationMessages.current.get("p")[0].content, /后台|启动失败/, "快照轮询不能擦除本次失败提示");
});
