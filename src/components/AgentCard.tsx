import type { AgentPersona, AgentRole, AgentRuntimeKind } from "../types";
import { AGENT_PERSONA_LABELS } from "../types";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { AgentAvatar } from "./AgentAvatar";
import { COLLABORATION_ROLE_LABELS } from "../collaborationRoles";
import { useTranslation } from "../useInterfaceLanguage";

/** 名片座右铭：取职责首行、去空白、截断。 */
export function agentMotto(description: string): string {
  const first = description.split("\n").find((line) => line.trim().length > 0) ?? "";
  return first.trim().slice(0, 60) || "尚未填写职责";
}

export interface AgentCardData {
  name: string;
  role: AgentRole;
  runtimeKind: AgentRuntimeKind;
  avatarId?: string | null;
  persona: AgentPersona;
  description: string;
  model: string | null;
  enabled?: boolean;
}

/** 团队成员名片：协作中心名片墙与编辑器实时预览共用。 */
export function AgentCard({
  agent,
  onClick,
  dimmed,
  reasoning,
}: {
  agent: AgentCardData;
  onClick?: () => void;
  dimmed?: boolean;
  reasoning?: string | null;
}) {
  const t = useTranslation();
  const model = [agent.model || "默认模型", reasoning].filter(Boolean).join(" · ");
  return (
    <div
      className={`collab-agent-card${dimmed ? " dimmed" : ""}${onClick ? " clickable" : ""}`}
      onClick={onClick}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={
        onClick
          ? (event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onClick();
              }
            }
          : undefined
      }
    >
      <div className="collab-agent-card-head">
        <AgentAvatar
          className="collab-agent-card-avatar"
          avatarId={agent.avatarId}
          runtimeKind={agent.runtimeKind}
          decorative
        />
        <div className="collab-agent-card-title">
          <strong>{agent.name || "未命名成员"}</strong>
          <span className="collab-agent-card-tags">
            <i className="collab-tag role">{t(COLLABORATION_ROLE_LABELS[agent.role])}</i>
            <i className="collab-tag persona">{AGENT_PERSONA_LABELS[agent.persona]}</i>
            {agent.enabled === false && <i className="collab-tag off">已停用</i>}
          </span>
        </div>
      </div>
      <p className="collab-agent-card-motto">{agentMotto(agent.description)}</p>
      <div className="collab-agent-card-foot">
        <span className="collab-agent-card-runtime">{RUNTIME_LABELS[agent.runtimeKind]}</span>
        <span className="collab-agent-card-model" title={reasoning ? model : agent.model ?? undefined}>
          {model}
        </span>
      </div>
    </div>
  );
}
