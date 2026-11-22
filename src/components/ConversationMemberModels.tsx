import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CaretLeft, CaretRight, Check, LinkSimple } from "@phosphor-icons/react";
import type { AgentProfile } from "../types";

type Field = "model" | "reasoning";
interface Choice { value: string; label: string }
interface MemberModels {
  supported: boolean;
  observed: boolean;
  selection: Record<Field, string | null>;
  model: Choice[];
  reasoning: Choice[];
}
const label: Record<Field, string> = { model: "模型", reasoning: "推理级别" };

export function ConversationMemberModels({ projectId, member, compact, onBack }: {
  projectId: string; member: AgentProfile; compact: boolean; onBack: () => void;
}) {
  const [data, setData] = useState<MemberModels>();
  const [field, setField] = useState<Field>();
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const heading = useRef<HTMLElement>(null);
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
    heading.current?.focus();
    void load();
    return () => { generation.current++; };
  }, [projectId, member.id]);
  useEffect(() => {
    if (field) heading.current?.focus();
  }, [field]);

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
        setField(undefined);
        setSaved(true);
        heading.current?.focus();
      }
    } catch (error) {
      if (generation.current === ticket) setError(String(error));
    } finally {
      if (generation.current === ticket) setBusy(false);
    }
  };

  const active = data?.selection;
  return <section className="conversation-member-models" aria-label={`${member.name}的模型与推理设置`} aria-busy={busy}>
    <header ref={heading} tabIndex={-1}>
      {(compact || field) && <button type="button" aria-label={field ? "返回模型与推理" : "返回成员列表"} onClick={() => field ? setField(undefined) : onBack()}><CaretLeft size={16} /></button>}
      <strong>{field ? label[field] : "模型与推理"}</strong>
      <small title={member.name}>{member.name}</small>
    </header>
    {data?.supported && field ? <div className="conversation-model-values">
      {[{ value: null, label: "跟随 CLI" }, ...data[field]].map(choice => <button type="button" key={choice.value ?? ""} disabled={busy} aria-pressed={active?.[field] === choice.value} onClick={() => void save({ field, value:choice.value })}>
        <span>{choice.label}</span>{active?.[field] === choice.value && <Check size={15} />}
      </button>)}
    </div> : data?.supported && data.observed ? <>
      {(Object.keys(label) as Field[]).map(item => <button type="button" key={item} className="conversation-model-row" disabled={busy || !data[item].length} onClick={() => setField(item)}>
        <span>{label[item]}</span><span className="conversation-model-value">{data[item].find(choice => choice.value === active?.[item])?.label ?? active?.[item] ?? (data[item].length ? "跟随 CLI" : "未提供")}</span><CaretRight size={14} />
      </button>)}
      <footer><button type="button" disabled={busy || (!active?.model && !active?.reasoning)} onClick={() => void save()}><LinkSimple size={15} />跟随 CLI</button><small role="status">{saved ? "已保存 · 下一轮生效" : "下一轮生效"}</small></footer>
    </> : data && !data.supported ? <p>此 Runtime 使用 CLI 配置。</p> : !error && <p role="status">正在读取可用选项…</p>}
    {data?.supported && !field && (!data.model.length || !data.reasoning.length) && <button type="button" className="conversation-model-refresh" disabled={busy} onClick={() => void load(true)}>刷新可用选项</button>}
    {error && <p role="alert">{error}{!data && <button type="button" disabled={busy} onClick={() => void load(true)}>重试</button>}</p>}
  </section>;
}
