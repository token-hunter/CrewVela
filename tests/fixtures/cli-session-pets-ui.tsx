import { useState } from "react";
import { createRoot } from "react-dom/client";
import { TerminalPane } from "../../src/components/TerminalPane";
import type { AgentRuntimeKind, AppTheme, SessionKind, SessionStatus, TerminalShell } from "../../src/types";
import "../../src/styles.css";
import { usePtyPolling } from "../../src/ptyPolling";
import { DEFAULT_GENERAL_SETTINGS } from "../../src/generalSettings";

const usePolledWork = new URLSearchParams(location.search).has("polled-work");
const workFixture = { ids: [] as number[] };
if (usePolledWork) Object.assign(window, {
  cliWorkFixture: workFixture,
  __TAURI_INTERNALS__: {
    transformCallback: () => 1,
    invoke: async (command: string) => {
      if (command === "pty_poll_all") return { sessions: [], chunks: {}, workingCliPtyIds: workFixture.ids };
      if (command === "general_settings_get") return { settings: DEFAULT_GENERAL_SETTINGS };
      return 1;
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
});
const avoidFixtureSessionUpdates = () => {};

interface HeaderCase {
  id: string;
  title: string;
  providerName: string;
  kind: SessionKind;
  shell: TerminalShell | null;
  agentRuntimeKind?: AgentRuntimeKind | null;
  multiAgentProjectId?: string;
  multiAgentAgentId?: string;
}

const cases: HeaderCase[] = [
  { id: "codex", title: "Codex CLI", providerName: "Codex CLI", kind: "terminal", shell: null, agentRuntimeKind: "codex" },
  { id: "kimi-code", title: "Kimi Code", providerName: "Kimi Code", kind: "terminal", shell: null, agentRuntimeKind: "kimi_code" },
  { id: "zcode-cli", title: "ZCode CLI", providerName: "ZCode CLI", kind: "terminal", shell: null, agentRuntimeKind: "zcode_cli" },
  { id: "opencode", title: "OpenCode", providerName: "OpenCode", kind: "terminal", shell: null, agentRuntimeKind: "opencode" },
  { id: "pi", title: "Pi", providerName: "Pi", kind: "terminal", shell: null, agentRuntimeKind: "pi" },
  { id: "legacy-claude", title: "Claude Code", providerName: "GLM", kind: "claude", shell: null },
  { id: "collaboration-claude", title: "Claude Code · Claude 架构师", providerName: "Claude Code", kind: "terminal", shell: null, agentRuntimeKind: "claude_code", multiAgentProjectId: "fixture-project", multiAgentAgentId: "fixture-claude" },
  { id: "zcode-native", title: "ZCode", providerName: "ZCode", kind: "terminal", shell: null, agentRuntimeKind: "zcode" },
  { id: "unsupported-runtime", title: "已停用 CLI", providerName: "已停用 CLI", kind: "terminal", shell: null, agentRuntimeKind: "removed_runtime" as AgentRuntimeKind },
  { id: "powershell", title: "PowerShell", providerName: "PowerShell", kind: "terminal", shell: "powershell" },
];

function PreviewCase({
  item,
  sessionId,
  status,
  hidden,
}: {
  item: HeaderCase;
  sessionId: number;
  status: SessionStatus;
  hidden: boolean;
}) {
  const [maximized, setMaximized] = useState(false);
  const [closed, setClosed] = useState(false);

  return (
    <section className="cli-pets-case" data-case={item.id} data-maximized={maximized} data-closed={closed}>
      <h2>{item.id}</h2>
      {closed ? (
        <output aria-live="polite">已触发关闭回调</output>
      ) : (
        <TerminalPane
          sessionId={sessionId}
          title={item.title}
          providerName={item.providerName}
          kind={item.kind}
          shell={item.shell}
          agentRuntimeKind={item.agentRuntimeKind ?? null}
          multiAgentProjectId={item.multiAgentProjectId}
          multiAgentAgentId={item.multiAgentAgentId}
          cwd="C:\\workspace\\app"
          status={status}
          theme={"dark" satisfies AppTheme}
          preview={!usePolledWork}
          hidden={hidden}
          maximized={maximized}
          onSwitchAgent={() => {}}
          onToggleMaximize={() => setMaximized((value) => !value)}
          onClose={() => setClosed(true)}
        />
      )}
    </section>
  );
}

function Preview() {
  usePtyPolling(avoidFixtureSessionUpdates, usePolledWork);
  const [status, setStatus] = useState<SessionStatus>("running");
  const [hidden, setHidden] = useState(false);

  return (
    <main className="cli-pets-fixture">
      <style>{`
        .cli-pets-fixture { min-height: 100vh; padding: 16px; background: #15171b; color: #e8eaf0; }
        .cli-pets-fixture h1 { margin: 0 0 10px; font-size: 18px; }
        .cli-pets-controls { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; }
        .cli-pets-controls button { padding: 5px 10px; color: #e8eaf0; border: 1px solid #474d59; border-radius: 5px; background: #252932; cursor: pointer; }
        .cli-pets-controls output { margin-left: 8px; color: #aab2c3; font: 12px monospace; }
        .cli-pets-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
        .cli-pets-case { min-width: 0; padding: 8px; border: 1px solid #383c45; border-radius: 8px; background: #1d2026; }
        .cli-pets-case h2 { margin: 0 0 6px; color: #aab2c3; font: 11px/1.3 monospace; }
        .cli-pets-case .pane { height: 150px; min-width: 0; }
        .cli-pets-case output { display: block; height: 150px; color: #aab2c3; font-size: 12px; }
        @media (max-width: 1240px) { .cli-pets-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
        @media (max-width: 760px) { .cli-pets-grid { grid-template-columns: 1fr; } }
      `}</style>
      <h1>CLI 执行动效 · 标题栏预览</h1>
      <div className="cli-pets-controls">
        <button type="button" data-testid="set-running" onClick={() => setStatus("running")}>运行中</button>
        <button type="button" data-testid="set-idle" onClick={() => setStatus("completed")}>停止</button>
        <button type="button" data-testid="toggle-hidden" onClick={() => setHidden((value) => !value)}>
          {hidden ? "显示面板" : "隐藏面板"}
        </button>
        <output data-testid="fixture-state">status={status}; hidden={String(hidden)}</output>
      </div>
      <div className="cli-pets-grid">
        {cases.map((item, index) => (
          <PreviewCase key={item.id} item={item} sessionId={index + 1} status={status} hidden={hidden} />
        ))}
      </div>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Preview />);
