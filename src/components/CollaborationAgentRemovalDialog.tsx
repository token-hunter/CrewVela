import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { AgentProfile } from "../types";
import { Modal } from "./ui/Modal";

export function CollaborationAgentRemovalDialog({ agent, previewBlockers, onClose, onRemoved }: {
  agent: AgentProfile;
  previewBlockers?: string[];
  onClose: () => void;
  onRemoved: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  const submitting = useRef(false);
  const [busy, setBusy] = useState(false);
  const [blockers, setBlockers] = useState<string[] | null>(previewBlockers ?? null);
  const [error, setError] = useState("");

  useEffect(() => {
    if (previewBlockers) { setBlockers(previewBlockers); return; }
    let alive = true;
    void invoke<string[]>("ma_remove_agent", { projectId: agent.projectId, agentId: agent.id, confirm: false })
      .then(result => { if (alive) setBlockers(result); })
      .catch(reason => { if (alive) setError(String(reason)); });
    return () => { alive = false; };
  }, [agent.id, agent.projectId, previewBlockers]);

  const remove = async () => {
    if (submitting.current || blockers?.length !== 0) return;
    submitting.current = true;
    setBusy(true);
    setError("");
    try {
      const remaining = previewBlockers ?? await invoke<string[]>("ma_remove_agent", {
        projectId: agent.projectId, agentId: agent.id, confirm: true,
      });
      if (remaining.length) setBlockers(remaining);
      else onRemoved();
    } catch (reason) {
      setError(String(reason));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };

  return createPortal(<Modal className="collab-member-removal-dialog" ariaLabel="从团队移除成员"
    initialFocusRef={cancel} onClose={() => { if (!submitting.current) onClose(); }}>
    <h2>从团队移除「{agent.name}」？</h2>
    <p>历史聊天、任务和审查记录将保留。</p>
    {!blockers && !error && <p role="status">正在检查未完成的任务…</p>}
    {!!blockers?.length && <div role="status">
      <p>请先转派或结束以下任务和活动，再移除成员。</p>
      <ul>{blockers.map((title, index) => <li key={index}>{title}</li>)}</ul>
    </div>}
    {error && <p role="alert">{error}</p>}
    <footer>
      <button ref={cancel} type="button" className="btn secondary" disabled={busy} onClick={onClose}>取消</button>
      <button type="button" className="btn secondary collab-member-remove" disabled={busy || blockers?.length !== 0}
        onClick={() => void remove()}>{busy ? "正在移除…" : "确认移除"}</button>
    </footer>
  </Modal>, document.body);
}
