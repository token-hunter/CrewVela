import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LinkSimple, X } from "@phosphor-icons/react";
import type { AgentProfile, ConversationMemberModelSummary } from "../types";
import { AgentAvatar } from "./AgentAvatar";

type Field = "model" | "reasoning";
interface Choice { value: string; label: string }
export type MemberModelSelection = Record<Field, string | null>;
export interface MemberModels {
  supported: boolean;
  observed: boolean;
  selection: MemberModelSelection;
  model: Choice[];
  reasoning: Choice[];
}
const label: Record<Field, string> = { model: "模型", reasoning: "推理级别" };

export function memberModelSummary(data: MemberModels): ConversationMemberModelSummary | null {
  const display = (field: Field) => {
    const value = data.selection[field];
    return data[field].find(choice => choice.value === value)?.label ?? value;
  };
  const model = display("model");
  const reasoning = display("reasoning");
  return model || reasoning ? { model, reasoning } : null;
}

export function ConversationMemberModels({ id, projectId, member, members, onSelectMember, onClose, onSaved }: {
  id: string;
  projectId: string;
  member: AgentProfile;
  members: AgentProfile[];
  onSelectMember: (id: string) => void;
  onClose: () => void;
  onSaved: (agentId: string, value: ConversationMemberModelSummary | null) => void;
}) {
  const [data, setData] = useState<MemberModels>();
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const memberSelect = useRef<HTMLSelectElement>(null);
  const generation = useRef(0);
  const key = { projectId, agentId: member.id };
  const load = async (refresh = false) => {
    const ticket = ++generation.current;
    setBusy(true);
    setError("");
    try {
      const result = await invoke<MemberModels>("ma_conversation_member_models", { ...key, refresh });
      if (generation.current === ticket) setData(result);
    } catch (error) {
      if (generation.current === ticket) setError(String(error));
    } finally {
      if (generation.current === ticket) setBusy(false);
    }
  };
  useEffect(() => {
    memberSelect.current?.focus();
    void load();
    return () => { generation.current++; };
  }, [projectId, member.id]);
  const save = async (next?: { field: Field; value: string | null }) => {
    if (!data || busy) return;
    const ticket = ++generation.current;
    const selection = next ? { ...data.selection, [next.field]: next.value } : { model: null, reasoning: null };
    if (next?.field === "model" && next.value !== data.selection.model) selection.reasoning = null;
    setBusy(true);
    setError("");
    setSaved(false);
    try {
      const result = await invoke<MemberModels>("ma_set_conversation_member_models", { ...key, selection });
      if (generation.current === ticket) {
        setData(result);
        setSaved(true);
        onSaved(member.id, memberModelSummary(result));
        if (selection.model !== data.selection.model) {
          setData({ ...result, reasoning: [] });
          await load(true);
        }
      }
    } catch (error) {
      if (generation.current === ticket) setError(String(error));
    } finally {
      if (generation.current === ticket) setBusy(false);
    }
  };

  const active = data?.selection;
  return <section id={id} className="conversation-member-models" aria-label={`${member.name}的模型与推理设置`} aria-busy={busy}>
    <div className="conversation-model-bar">
      <label className="conversation-model-member">
        <AgentAvatar avatarId={member.avatarId} runtimeKind={member.runtimeKind} decorative />
        <select ref={memberSelect} aria-label="配置成员" value={member.id} title={member.name} onChange={event => onSelectMember(event.target.value)}>
          {members.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
      </label>
      {(Object.keys(label) as Field[]).map(field => {
        const choices = data?.[field] ?? [];
        const value = active?.[field] ?? "";
        return <label className="conversation-model-field" key={field}>
          <span>{label[field]}</span>
          <select value={value} disabled={busy || !choices.length} onChange={event => void save({ field, value: event.target.value || null })}>
            <option value="">{!data && busy ? "读取中…" : data?.supported && !choices.length ? "未提供" : "跟随 CLI"}</option>
            {value && !choices.some(choice => choice.value === value) && <option value={value} disabled>{value} · 已不可用</option>}
            {choices.map(choice => <option key={choice.value} value={choice.value}>{choice.label}</option>)}
          </select>
        </label>;
      })}
      <div className="conversation-model-actions">
        {(active?.model || active?.reasoning) && <button type="button" className="conversation-model-reset" aria-label="跟随 CLI" title="模型和推理级别都跟随 CLI" disabled={busy} onClick={() => void save()}><LinkSimple size={14} /><span>跟随 CLI</span></button>}
        <small role="status">{busy ? data ? "正在处理…" : "正在读取可用选项…" : saved ? "已保存 · 下一轮聊天生效" : "下一轮聊天生效"}</small>
        <button type="button" className="conversation-model-close" aria-label="收起模型与推理" onClick={onClose}><X size={16} /></button>
      </div>
    </div>
    {((data?.supported && (!data.model.length || !data.reasoning.length)) || error) && <div className="conversation-model-feedback">
      {error && <span role="alert">{error}</span>}
      <button type="button" className="conversation-text-button" disabled={busy} onClick={() => void load(true)}>{error && !data ? "重试" : "刷新可用选项"}</button>
    </div>}
  </section>;
}
