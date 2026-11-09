import test from "node:test";
import assert from "node:assert/strict";
import { needsSwitchRecovery, projectSwitchSessions, retainedSwitchSource, sessionSwitchReason, switchedHistory, switchSlotId, switchSelectionAfterSnapshot, switchWorkspaceAssignment, switchTargets, CLI_SWITCH_RUNTIMES, CHAT_SWITCH_RUNTIMES } from "../src/sessionSwitch.ts";
import type { SessionRecord } from "../src/types.ts";
import type { SessionSwitchRecord } from "../src/sessionSwitch.ts";

const source = (id: string): SessionRecord => ({
  id, claudeSessionId: "native-" + id, kind: "claude", shell: null, providerId: "fixture", providerName: "Claude",
  cwd: "C:/fixture/worktree", title: id, status: "running", ptyId: 1, yolo: false, autoMode: false,
  permissionMode: null, titleLocked: false, exitCode: null, lastError: null, createdAt: 1, updatedAt: 1,
});
const record = (original: SessionRecord, targetId: string, sourcePosition = 0): SessionSwitchRecord => ({
  id: "switch-" + original.id, source: original, sourcePosition, sourcePty: original.ptyId, targetId,
  threadId: "thread-" + targetId, state: "queued", historyMode: "official", context: "fixture", error: null,
});
const target = (id: string) => ({ ...source(id), kind: "terminal", agentRuntimeKind: "codex", claudeSessionId: "" }) as SessionRecord;

test("entry permits ordinary supported agents and stopped sessions, excludes all collaboration/host/shell/retired variants", () => {
  assert.equal(sessionSwitchReason(source("a")), null);
  assert.equal(sessionSwitchReason({ ...source("a"), ptyId: null, status: "stopped" }), null);
  for (const patch of [{ multiAgentProjectId: "p" }, { multiAgentAgentId: "a" }, { runtimeCommandHost: true },
    { retiredAgentRuntime: true }, { archivedAt: 123 }, { kind: "terminal" }]) {
    assert.ok(sessionSwitchReason({ ...source("a"), ...patch } as SessionRecord));
  }
});
test("source logical slot and original history remain; all operations retain target physical id", () => {
  const original = source("a"); const relay = record(original, "codex");
  const unrelated = source("b"); const projected = projectSwitchSessions([unrelated, target("codex")], [relay]);
  assert.deepEqual(projected.map((item) => item.id), ["codex", "b"]);
  assert.equal(switchSlotId("codex", [relay]), "a");
  assert.equal(switchedHistory("codex", [relay])?.claudeSessionId, "native-a");
  assert.equal(switchedHistory("b", [relay]), null);
  const favorites = new Set(["a"]);
  assert.equal(favorites.has(switchSlotId(projected[0].id, [relay])), true);
  assert.equal(projected[0].agentRuntimeKind, "codex");
});
test("two successive switches preserve source positions even when native source records are already archived", () => {
  const a = record(source("a"), "codex-a", 0); const b = record(source("b"), "codex-b", 1);
  assert.deepEqual(projectSwitchSessions([target("codex-b"), source("shell"), target("codex-a")], [a, b]).map((item) => item.id),
    ["codex-a", "codex-b", "shell"]);
});
test("cancelled relation stops projecting; unrelated original sessions retain exact references", () => {
  const original = source("a"); const other = source("b"); const cancelled = { ...record(original, "codex"), state: "cancelled" };
  const projected = projectSwitchSessions([original, other], [cancelled]);
  assert.strictEqual(projected[0], original); assert.strictEqual(projected[1], other);
  assert.equal(switchSlotId("codex", [cancelled]), "codex");
  assert.equal(switchedHistory("codex", [cancelled])?.id, "a");
});
test("every interrupted stage is recoverable after reload without an error field", () => {
  for (const state of ["stopping", "stopped", "importing", "imported", "launching"]) {
    assert.equal(needsSwitchRecovery({ ...record(source("a"), "codex"), state }, []), true);
  }
  assert.equal(needsSwitchRecovery(record(source("a"), "codex"), [target("codex")]), false);
  assert.equal(needsSwitchRecovery(record(source("a"), "codex"), []), true);
  assert.equal(needsSwitchRecovery({ ...record(source("a"), "codex"), state: "cancelled" }, []), false);
});

test("source archival during import retains focus and original tab even if user changes current tab", () => {
  const relay = { ...record(source("a"), "codex"), state: "importing", targetId: null };
  assert.equal(switchSelectionAfterSnapshot("a", [source("b")], [relay], "a"), "a");
  assert.equal(switchSelectionAfterSnapshot("b", [source("b")], [relay], "a"), "b");
  const done = { ...relay, state: "queued", targetId: "codex" };
  assert.equal(switchSelectionAfterSnapshot("a", [target("codex")], [done], "a"), "codex");
  assert.equal(switchWorkspaceAssignment("codex", { a: "original-tab", b: "new-current-tab" }, [done], "new-current-tab"), "original-tab");
  assert.equal(switchSelectionAfterSnapshot("a", [], [{ ...relay, state: "preview" }], null), null);
});

test("both physical records deleted leaves no dead recovery entry", () => {
  assert.equal(needsSwitchRecovery(record(source("a"), "codex"), [], []), false);
  assert.equal(needsSwitchRecovery(record(source("a"), "codex"), [], [source("a")]), true);
});


test("all seven CLI and seven Chat select only other members of their own family", () => {
  for (const group of [CLI_SWITCH_RUNTIMES, CHAT_SWITCH_RUNTIMES]) for (const runtime of group) {
    const item = { ...source(runtime), kind: "terminal", agentRuntimeKind: runtime } as SessionRecord;
    assert.equal(sessionSwitchReason(item), null);
    assert.deepEqual(switchTargets(item), group.filter(kind => kind !== runtime));
  }
  assert.ok(sessionSwitchReason({ ...source("retired"), agentRuntimeKind: "oh_my_pi" }));
});
test("A to B to C to A agent type keeps one leaf, original location, favorites and all original history", () => {
  const a = source("a"); const b = target("b"); const c = { ...target("c"), agentRuntimeKind: "kimi_code" } as SessionRecord;
  const a2 = { ...source("a2"), agentRuntimeKind: "claude_code", createdAt: 99 } as SessionRecord;
  const records = [record(a, "b"), record(b, "c"), record(c, "a2")];
  assert.equal(switchSlotId("a2", records), "a");
  assert.equal(switchedHistory("a2", records)?.id, "a");
  const projected = projectSwitchSessions([a, b, c, a2, source("other")], records);
  assert.deepEqual(projected.map(item => item.id), ["a2", "other"]);
  assert.equal(projected[0].createdAt, a.createdAt);
  assert.equal(switchWorkspaceAssignment("a2", { a: "original" }, records, "changed"), "original");
});
test("sending interrupted before persistence is recoverable and cancelled middle hop is excluded", () => {
  assert.equal(needsSwitchRecovery({ ...record(source("a"), "b"), state: "sending" }, [target("b")]), true);
  const records = [record(source("a"), "b"), { ...record(target("b"), "c"), state: "cancelled" }];
  assert.deepEqual(projectSwitchSessions([target("b"), source("other")], records).map(item => item.id), ["b", "other"]);
});

test("new handoffs retain the stopped source and give the receiver a separate slot in the same workspace", () => {
  const a={...source('a'),status:'stopped',ptyId:null} as SessionRecord,b=target('b');
  const relay={...record(a,'b'),preserveSource:true};
  const projected=projectSwitchSessions([b,a,source('other')],[relay]);
  assert.deepEqual(projected.map(item=>item.id),['a','b','other']);
  assert.strictEqual(projected[0],a);
  assert.equal(switchSlotId('a',[relay]),'a');assert.equal(switchSlotId('b',[relay]),'b');
  assert.equal(switchWorkspaceAssignment('b',{a:'work'},[relay],'other'),'work');
  assert.equal(switchWorkspaceAssignment('b',{a:'work',b:'moved'},[relay],'other'),'moved');
  assert.equal(retainedSwitchSource('a',[relay])?.id,relay.id);
  assert.equal(retainedSwitchSource('b',[relay]),null);
});

test("separate handoff chains survive reload, cancellation, and an earlier legacy relay",()=>{
  const a=source('a'),b=target('b'),c=target('c');
  const records=[{...record(a,'b'),preserveSource:true},{...record(b,'c'),preserveSource:true}];
  const loaded=JSON.parse(JSON.stringify(records));
  assert.deepEqual(projectSwitchSessions([c,b,a],loaded).map(item=>item.id),['a','b','c']);
  assert.equal(switchSlotId('c',loaded),'c');
  assert.equal(switchWorkspaceAssignment('c',{a:'work'},loaded,'other'),'work');
  assert.equal(retainedSwitchSource('b',loaded)?.targetId,'c');
  const cancelled=[loaded[0],{...loaded[1],state:'cancelled'}];
  assert.equal(retainedSwitchSource('b',cancelled),null);
  assert.deepEqual(projectSwitchSessions([b,a],cancelled).map(item=>item.id),['a','b']);
  const mixed=[record(a,'b'),loaded[1]];
  assert.deepEqual(projectSwitchSessions([c,b],mixed).map(item=>item.id),['b','c']);
  assert.equal(switchSlotId('b',mixed),'a');assert.equal(switchSlotId('c',mixed),'c');
  assert.equal(retainedSwitchSource('a',[{...loaded[0],state:'preview'}]),null);
});
