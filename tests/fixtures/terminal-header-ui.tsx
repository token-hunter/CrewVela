import { useState } from "react";
import { createRoot } from "react-dom/client";
import { TerminalPane } from "../../src/components/TerminalPane";
import type { AgentRuntimeKind, AppTheme, SessionStatus, SessionKind, TerminalShell } from "../../src/types";
import "../../src/styles.css";

interface HeaderCase {
  id: string;
  title: string;
  providerName: string;
  kind: SessionKind;
  shell: TerminalShell | null;
  agentRuntimeKind?: AgentRuntimeKind;
  multiAgentProjectId?: string;
  multiAgentAgentId?: string;
  relaySession?: boolean;
  cwd: string;
}

const cases: HeaderCase[] = [
  { id: "claude-default", title: "新会话", providerName: "GLM", kind: "claude", shell: null, cwd: "C:\\workspace\\app" },
  { id: "claude-custom", title: "上线前检查", providerName: "GLM", kind: "claude", shell: null, cwd: "C:\\workspace\\app" },
  { id: "codex-ordinary", title: "Codex CLI", providerName: "Codex CLI", kind: "terminal", shell: null, agentRuntimeKind: "codex", cwd: "C:\\workspace\\app" },
  { id: "kimi-ordinary", title: "Kimi Code", providerName: "Kimi Code", kind: "terminal", shell: null, agentRuntimeKind: "kimi_code", cwd: "C:\\workspace\\app" },
  { id: "zcode-ordinary", title: "ZCode CLI", providerName: "ZCode CLI", kind: "terminal", shell: null, agentRuntimeKind: "zcode_cli", cwd: "C:\\workspace\\app" },
  { id: "collaboration", title: "代码审查", providerName: "Codex CLI", kind: "terminal", shell: null, agentRuntimeKind: "codex", multiAgentProjectId: "project-1", multiAgentAgentId: "agent-1", cwd: "C:\\workspace\\app" },
  { id: "relay", title: "接力会话", providerName: "Kimi Code", kind: "terminal", shell: null, agentRuntimeKind: "kimi_code", relaySession: true, cwd: "C:\\workspace\\app" },
  { id: "powershell", title: "PowerShell", providerName: "PowerShell", kind: "terminal", shell: "powershell", cwd: "C:\\workspace\\app" },
];

const theme: AppTheme = "dark";
const status: SessionStatus = "running";

function PreviewCase({ item, sessionId }: { item: HeaderCase; sessionId: number }) {
  const [maximized, setMaximized] = useState(false);
  const [closed, setClosed] = useState(false);

  return (
    <section
      className="terminal-header-case"
      data-case={item.id}
      data-maximized={maximized}
      data-closed={closed}
    >
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
          multiAgentProjectId={item.multiAgentProjectId ?? null}
          multiAgentAgentId={item.multiAgentAgentId ?? null}
          cwd={item.cwd}
          status={status}
          theme={theme}
          preview
          hidden={false}
          maximized={maximized}
          relaySession={item.relaySession}
          onToggleMaximize={() => setMaximized((value) => !value)}
          onClose={() => setClosed(true)}
        />
      )}
      <output aria-live="polite">
        {maximized ? "最大化回调已触发" : "最大化回调待触发"}
      </output>
    </section>
  );
}

function Preview() {
  return (
    <main className="terminal-header-fixture">
      <style>{`
        .terminal-header-fixture { min-height: 100vh; padding: 20px; background: #15171b; color: #e8eaf0; }
        .terminal-header-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(360px, 1fr)); gap: 16px; }
        .terminal-header-case { min-width: 0; padding: 12px; border: 1px solid #383c45; border-radius: 8px; background: #1d2026; }
        .terminal-header-case h2 { margin: 0 0 8px; font: 12px/1.4 monospace; color: #aab2c3; }
        .terminal-header-case .pane { height: 290px; min-width: 0; }
        .terminal-header-case output { display: block; padding-top: 6px; color: #aab2c3; font-size: 11px; }
      `}</style>
      <h1>PTY 标题栏展示检查</h1>
      <div className="terminal-header-grid">
        {cases.map((item, index) => <PreviewCase key={item.id} item={item} sessionId={index + 1} />)}
      </div>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Preview />);
