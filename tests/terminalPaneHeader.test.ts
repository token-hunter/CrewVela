import assert from "node:assert/strict";
import test from "node:test";

import { terminalPaneHeaderPresentation } from "../src/components/terminalPaneHeader.ts";

test("ordinary CLI sessions keep their runtime logo and hide a duplicate runtime name", () => {
  for (const [runtime, title] of [["codex", "Codex CLI"], ["kimi_code", "Kimi Code"]] as const) {
    assert.deepEqual(terminalPaneHeaderPresentation({
      title,
      providerName: title,
      kind: "terminal",
      agentRuntimeKind: runtime,
      cwd: "C:\\workspace",
    }), {
      title,
      runtimeKind: runtime,
      showShellIcon: false,
      subtitle: null,
      agentBadge: null,
    });
  }
});

test("ordinary Claude uses its runtime avatar and replaces only the default title", () => {
  assert.deepEqual(terminalPaneHeaderPresentation({
    title: "新会话",
    providerName: "GLM",
    kind: "claude",
    cwd: "C:\\workspace",
  }), {
    title: "Claude Code",
    runtimeKind: "claude_code",
    showShellIcon: false,
    subtitle: "GLM",
    agentBadge: null,
  });

  assert.deepEqual(terminalPaneHeaderPresentation({
    title: "上线前检查",
    providerName: "GLM",
    kind: "claude",
    cwd: "C:\\workspace",
  }), {
    title: "上线前检查",
    runtimeKind: "claude_code",
    showShellIcon: false,
    subtitle: "GLM",
    agentBadge: null,
  });
});

test("ordinary terminals retain their terminal icon and working-directory subtitle", () => {
  assert.deepEqual(terminalPaneHeaderPresentation({
    title: "PowerShell",
    providerName: "PowerShell",
    kind: "terminal",
    cwd: "C:\\workspace",
  }), {
    title: "PowerShell",
    runtimeKind: null,
    showShellIcon: true,
    subtitle: "C:\\workspace",
    agentBadge: null,
  });
});

test("only sessions with both collaboration identifiers show the collaboration badge", () => {
  assert.deepEqual(terminalPaneHeaderPresentation({
    title: "代码审查",
    providerName: "Codex CLI",
    kind: "terminal",
    agentRuntimeKind: "codex",
    multiAgentProjectId: "project-1",
    multiAgentAgentId: "agent-1",
    cwd: "C:\\workspace",
  }), {
    title: "代码审查",
    runtimeKind: "codex",
    showShellIcon: false,
    subtitle: "Codex CLI",
    agentBadge: "collaboration",
  });

  for (const association of [
    { multiAgentProjectId: "project-1", multiAgentAgentId: null },
    { multiAgentProjectId: null, multiAgentAgentId: "agent-1" },
  ]) {
    assert.equal(terminalPaneHeaderPresentation({
      title: "Codex CLI",
      providerName: "Codex CLI",
      kind: "terminal",
      agentRuntimeKind: "codex",
      ...association,
      cwd: "C:\\workspace",
    }).agentBadge, null);
  }
});

test("relay label takes precedence and does not require collaboration identifiers", () => {
  assert.equal(terminalPaneHeaderPresentation({
    title: "接力会话",
    providerName: "Kimi Code",
    kind: "terminal",
    agentRuntimeKind: "kimi_code",
    relaySession: true,
    cwd: "C:\\workspace",
  }).agentBadge, "relay");

  assert.equal(terminalPaneHeaderPresentation({
    title: "接力会话",
    providerName: "Kimi Code",
    kind: "terminal",
    agentRuntimeKind: "kimi_code",
    multiAgentProjectId: "project-1",
    multiAgentAgentId: "agent-1",
    relaySession: true,
    cwd: "C:\\workspace",
  }).agentBadge, "relay");
});
