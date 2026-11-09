import { useEffect, useState } from "react";
import { ArrowRight, ArrowsLeftRight } from "@phosphor-icons/react";
import { RUNTIME_LOGOS, RUNTIME_LABELS } from "../runtimeLogos";
import type { NativeAgentKind, NativeAgentSwitchProps } from "../nativeAgentSwitch";
import { NativeAgentMascot } from "./NativeAgentMascot";

export function NativeAgentSelector({ kind, switching, enabledAgents, onSwitchAgent, onHandoffAgent, managedToolbar }: NativeAgentSwitchProps & { kind: NativeAgentKind }) {
  if (managedToolbar) return null;
  return <div className="native-agent-toolbar"><nav className="native-agent-selector" aria-label="打开 Agent 聊天">{(["claude_chat", "codex_chat", "kimi_chat", "zcode", "deepseek_harness", "opencode_chat", "pi_chat"] as const).map(agent => <button
    type="button" key={agent} className={agent === kind ? "selected" : ""}
    aria-label={`打开 ${RUNTIME_LABELS[agent]} 聊天`} aria-pressed={agent === kind}
    title={enabledAgents?.[agent] === false ? `${RUNTIME_LABELS[agent]} 未启用` : `打开 ${RUNTIME_LABELS[agent]} 聊天`}
    disabled={switching || enabledAgents?.[agent] === false || !onSwitchAgent}
    onClick={() => { if (agent !== kind) onSwitchAgent?.(agent); }}>
    <img src={RUNTIME_LOGOS[agent]} data-runtime={agent} alt="" />
  </button>)}</nav>{onHandoffAgent && <button type="button" className="btn secondary native-agent-handoff" title="把当前工作交接给其它Agent" aria-label="把当前工作交接给其它Agent" disabled={switching} onClick={onHandoffAgent}><ArrowsLeftRight size={17} aria-hidden="true" /></button>}</div>
}

export function NativeAgentWelcome({ kind, active = true, skillsDisabled, onSkills }: { kind: NativeAgentKind; active?: boolean; skillsDisabled: boolean; onSkills: () => void }) {
  const [visible, setVisible] = useState(document.visibilityState !== "hidden");
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  const tagline = kind === "deepseek_harness" ? "代码理解 · 功能开发 · 问题排查"
    : kind === "claude_chat" ? "本机 Claude Code · 原生 ACP 会话"
    : kind === "codex_chat" ? "本机 Codex · 原生 ACP 会话"
    : kind === "kimi_chat" ? "本机 Kimi Code · 原生 ACP 会话"
    : kind === "opencode_chat" ? "本机 OpenCode · 原生 ACP 会话"
    : kind === "pi_chat" ? "本机 pi · 原生 RPC 会话" : "技能 · 插件 · 工作流";
  return <div className={`native-agent-welcome ${kind === "zcode" ? "native-agent-zcode" : "native-agent-dolphin"}${!active || !visible ? " motion-paused" : ""}`}>
    <div className="native-agent-character">
      <NativeAgentMascot kind={kind} active={active && visible} />
    </div>
    <h1>{RUNTIME_LABELS[kind]}</h1>
    <p>{tagline}</p>
    <button type="button" className="native-agent-skills" disabled={skillsDisabled} onClick={onSkills}>{kind === "zcode" ? "查看技能与插件" : kind === "claude_chat" || kind === "codex_chat" || kind === "kimi_chat" || kind === "opencode_chat" || kind === "pi_chat" ? "查看命令" : "查看技能"}<ArrowRight size={17} /></button>
  </div>
}
