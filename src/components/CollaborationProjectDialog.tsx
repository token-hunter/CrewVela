import { useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "./ui/Modal";

export interface CollaborationProjectDraft {
  name: string;
  goal: string;
  rootPath: string;
}

export function CollaborationProjectDialog({ onCreate, onClose }: {
  onCreate: (draft: CollaborationProjectDraft) => Promise<void>;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<CollaborationProjectDraft>({ name: "", goal: "", rootPath: "" });
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [picking, setPicking] = useState(false);
  const inFlight = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const busy = creating || picking;

  const pickDirectory = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setPicking(true);
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择协作项目目录" });
      if (typeof selected === "string") setDraft(current => ({
        ...current,
        rootPath: selected,
        name: current.name || selected.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "",
      }));
    } catch (reason) { setError(`选择目录失败：${String(reason)}`); }
    finally { inFlight.current = false; setPicking(false); }
  };

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (inFlight.current) return;
    const input = { name: draft.name.trim(), goal: draft.goal.trim(), rootPath: draft.rootPath.trim() };
    if (!input.name || !input.goal || !input.rootPath) {
      setError("请填写项目名称、项目目录和项目目标。");
      return;
    }
    if (!/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(input.rootPath)) {
      setError("项目目录请填写本地绝对路径，或点击「选择目录」。");
      return;
    }
    inFlight.current = true;
    setCreating(true);
    setError(null);
    try { await onCreate(input); onClose(); }
    catch (reason) { setError(`创建项目失败：${String(reason)}`); }
    finally { inFlight.current = false; setCreating(false); }
  };

  return createPortal(<Modal className="collab-agent-editor collab-project-editor" overlayClassName="dialog-overlay collab-agent-editor-overlay" ariaLabel="创建协作项目" initialFocusRef={nameRef} onClose={() => { if (!inFlight.current) onClose(); }}>
    <header><h3>创建协作项目</h3><button type="button" aria-label="关闭创建项目" disabled={busy} onClick={onClose}>×</button></header>
    <p>选择项目目录并填写目标，创建后即可在这里添加团队成员。</p>
    <form onSubmit={event => void create(event)} noValidate>
      <label>项目名称<input ref={nameRef} value={draft.name} disabled={busy} onChange={event => setDraft({ ...draft, name: event.target.value })} placeholder="例如：CrewVela" /></label>
      <label>项目目录<input value={draft.rootPath} disabled={busy} onChange={event => setDraft({ ...draft, rootPath: event.target.value })} placeholder="输入项目的本地绝对路径，或选择目录" /></label>
      <button className="btn secondary" type="button" disabled={busy} onClick={() => void pickDirectory()}>{picking ? "选择中…" : "选择目录"}</button>
      <label>项目目标<textarea aria-label="项目目标" rows={3} value={draft.goal} disabled={busy} onChange={event => setDraft({ ...draft, goal: event.target.value })} placeholder="描述团队要完成的工作和验收要求" /></label>
      {error && <div className="collab-agent-error" role="alert">{error}</div>}
      <footer><button className="btn secondary" type="button" disabled={busy} onClick={onClose}>取消</button><button className="btn primary" type="submit" disabled={busy}>{creating ? "创建中…" : "创建项目"}</button></footer>
    </form>
  </Modal>, document.body);
}
