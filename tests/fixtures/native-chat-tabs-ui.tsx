import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { NativeChatTabs } from "../../src/components/NativeChatTabs";
import { RUNTIME_LABELS } from "../../src/runtimeLogos";
import type { NativeAgentKind } from "../../src/nativeAgentSwitch";
import type { SessionRecord } from "../../src/types";
import "../../src/styles/tokens.css";
import "../../src/styles/native-agent.css";
import "../../src/components/native-chat-tab-sort.css";

const kinds: NativeAgentKind[] = [
  "zcode",
  "codex_chat",
  "claude_chat",
  "kimi_chat",
  "opencode_chat",
  "pi_chat",
  "deepseek_harness",
];
const enabledAgents = Object.fromEntries(kinds.map(kind => [kind, true])) as Record<NativeAgentKind, boolean>;

function mockSession(id: string, kind: NativeAgentKind, title: string, status: SessionRecord["status"] = "running"): SessionRecord {
  return {
    id,
    kind: "terminal",
    agentRuntimeKind: kind,
    providerId: "local-runtime-" + kind,
    providerName: RUNTIME_LABELS[kind],
    cwd: "C:/example/crewvela",
    title,
    status,
  } as SessionRecord;
}

const initialSessions = kinds.map((kind, index) => mockSession(
  "agent-" + kind,
  kind,
  kind === "zcode" ? "ZCode" : RUNTIME_LABELS[kind],
  index === 1 ? "waiting" : index === 6 ? "error" : "running",
));

function NativeChatTabsFixture() {
  const [sessions, setSessions] = useState(initialSessions);
  const [selectedId, setSelectedId] = useState<string | null>("agent-zcode");
  const [duplicateProvider, setDuplicateProvider] = useState(false);
  const [longTitle, setLongTitle] = useState(false);
  const [narrow, setNarrow] = useState(false);
  const [light, setLight] = useState(false);
  const [notice, setNotice] = useState("当前选中 ZCode Desktop；原型仅展示顶部标签栏。");

  useEffect(() => {
    document.documentElement.dataset.theme = light ? "light" : "dark";
  }, [light]);

  const toggleDuplicateProvider = (checked: boolean) => {
    setDuplicateProvider(checked);
    setSessions(current => checked
      ? [...current.filter(session => session.id !== "codex-review"), mockSession("codex-review", "codex_chat", "Codex Chat · Review", "waiting")]
      : current.filter(session => session.id !== "codex-review"));
  };
  const toggleLongTitle = (checked: boolean) => {
    setLongTitle(checked);
    setSessions(current => current.map(session => session.id === "agent-zcode"
      ? { ...session, title: checked ? "ZCode Desktop · 正在整理跨 Agent 协作记录与会话路由" : "ZCode" }
      : session));
  };
  const closeSession = (id: string) => {
    setSessions(current => {
      const index = current.findIndex(session => session.id === id);
      const next = current.filter(session => session.id !== id);
      if (selectedId === id) setSelectedId(next[Math.min(index, next.length - 1)]?.id ?? null);
      return next;
    });
  };
  const reorderSession = (source: string, target: string, after: boolean) => {
    setSessions(current => {
      const next = [...current];
      const sourceIndex = next.findIndex(session => session.id === source);
      if (sourceIndex < 0) return current;
      const [moved] = next.splice(sourceIndex, 1);
      const targetIndex = next.findIndex(session => session.id === target);
      if (targetIndex < 0) return current;
      next.splice(targetIndex + (after ? 1 : 0), 0, moved);
      return next;
    });
  };

  return <main className="fixture-page">
    <header className="fixture-header">
      <div className="fixture-heading">
        <p>CrewVela · NativeChatTabs</p>
        <h1>顶部标签栏原型</h1>
        <p>{sessions.length} 个打开会话 · ZCode 默认选中</p>
      </div>
      <div className="fixture-controls" aria-label="原型查看选项">
        <label className="fixture-option"><input type="checkbox" checked={duplicateProvider} onChange={event => toggleDuplicateProvider(event.target.checked)} />重复 provider</label>
        <label className="fixture-option"><input type="checkbox" checked={longTitle} onChange={event => toggleLongTitle(event.target.checked)} />长标题</label>
        <label className="fixture-option"><input type="checkbox" checked={narrow} onChange={event => setNarrow(event.target.checked)} />窄宽</label>
        <label className="fixture-option"><input type="checkbox" checked={light} onChange={event => setLight(event.target.checked)} />浅色主题</label>
      </div>
    </header>
    <section
      className={"fixture-stage" + (narrow ? " narrow" : "")}
      aria-label="NativeChatTabs 七会话预览"
    >
      <NativeChatTabs
        sessions={sessions}
        selectedId={selectedId}
        creating={false}
        enabledAgents={enabledAgents}
        canCreate
        onSelect={setSelectedId}
        onClose={closeSession}
        onCreate={kind => setSessions(current => [...current, mockSession("new-" + Date.now(), kind, RUNTIME_LABELS[kind] + " · 新会话")])}
        onReorder={reorderSession}
        onHandoff={() => setNotice("交接按钮已触发；fixture 不启动真实 Agent。")}
      />
      <div className="fixture-context"><p>{notice}</p><p className="fixture-note">悬停标签或用键盘聚焦可查看完整名称；方向键、Home、End、关闭和拖动均沿用生产组件。</p></div>
    </section>
  </main>;
}

createRoot(document.getElementById("root")!).render(<NativeChatTabsFixture />);
