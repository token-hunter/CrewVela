import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { WorktreeLifecycleControls } from "./WorktreeLifecycleControls";
import { RUNTIME_LABELS } from "../runtimeLogos";
import type { AgentRuntimeKind } from "../types";
import "../styles/worktree-settings.css";

interface Entry {
  sessionId: string | null;
  title: string;
  project: string | null;
  repoRoot: string;
  path: string;
  cwd: string;
  branch: string | null;
  baseRef: string | null;
  runtime: AgentRuntimeKind | null;
  archived: boolean;
  present: boolean;
  blocked: string | null;
}
interface Overview { root: string; entries: Entry[] }

export function WorktreeSettingsPanel() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [repository, setRepository] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const sequence = useRef(0);
  const action = useRef(false);
  const reload = useCallback(async () => {
    const current = ++sequence.current;
    try {
      const data = await invoke<Overview>("worktree_overview");
      if (!data || !Array.isArray(data.entries)) throw Error("应用未返回工作树列表");
      if (current === sequence.current) { setOverview(data); setError(""); }
    } catch (reason) {
      if (current === sequence.current) setError(String(reason));
    }
  }, []);
  useEffect(() => { void reload(); return () => { sequence.current++; }; }, [reload]);
  const act = async (task: () => Promise<void>) => {
    if (action.current) return;
    action.current = true; setBusy(true); setError(""); setNotice("");
    try { await task(); } catch (reason) { setError(String(reason)); }
    finally { action.current = false; setBusy(false); }
  };
  const chooseRoot = (reset = false) => act(async () => {
    const root = reset ? null : await open({ directory: true, multiple: false, title: "选择工作树根目录" });
    if (!reset && typeof root !== "string") return;
    await invoke("worktree_set_root", { root });
    await reload(); setNotice("已更新新建工作树的位置。");
  });
  const inspect = (entry: Entry) => { setSelected(entry.sessionId); setRepository(entry.repoRoot); setConfirming(null); };
  const clean = (entry: Entry) => act(async () => {
    await invoke("worktree_clean", { sessionId: entry.sessionId });
    setConfirming(null); await reload(); setNotice("已保存恢复快照并清理工作树，原分支和会话历史已保留。");
  });
  return <div className="worktree-settings">
    <section>
      <h3>工作树根目录</h3>
      <p>新建的会话工作树存放在此目录。更改后，已有工作树继续使用原位置。</p>
      {overview && <code>{overview.root}</code>}
      <div className="worktree-actions">
        <button type="button" className="btn" disabled={busy} onClick={() => void chooseRoot()}>更改根目录</button>
        <button type="button" className="btn secondary" disabled={busy} onClick={() => void chooseRoot(true)}>恢复默认目录</button>
      </div>
    </section>
    <section>
      <div className="worktree-section-heading"><h3>托管工作树</h3><button type="button" className="btn secondary" disabled={busy} onClick={() => void reload()}>刷新</button></div>
      {!overview && !error && <p role="status">正在读取工作树…</p>}
      {overview && !overview.entries.length && <p>暂无托管工作树。新建会话时可勾选「使用独立工作树」。</p>}
      {overview?.entries.map(entry => <article key={entry.sessionId || entry.path}>
        <strong><GitBranchIcon size={16} aria-hidden="true" />{entry.title}</strong>
        <p>{[entry.project || "未分组", entry.runtime ? RUNTIME_LABELS[entry.runtime] : "协作任务", entry.archived && "已归档", entry.branch].filter(Boolean).join(" · ")}</p>
        <code>{entry.path}</code>
        {entry.cwd !== entry.path && <p>会话工作目录 <code>{entry.cwd}</code></p>}
        {entry.baseRef && <p>基准分支 {entry.baseRef.replace(/^refs\/(heads|remotes)\//, "")}</p>}
        {entry.blocked && <p>{entry.blocked}</p>}
        <div className="worktree-actions">
          <button type="button" className="btn secondary" disabled={busy || !entry.present} onClick={() => void act(async () => {
            await invoke("workspace_open_path", { root: entry.path, relativePath: "." });
          })}><FolderOpenIcon size={15} aria-hidden="true" />打开目录</button>
          {entry.sessionId && <>
            <button type="button" className="btn secondary" disabled={busy} onClick={() => inspect(entry)}>管理环境与恢复</button>
            <button type="button" className="btn secondary" disabled={busy || !!entry.blocked || !entry.present} onClick={() => { setConfirming(entry.sessionId); setNotice(""); }}>清理工作树</button>
          </>}
        </div>
        {confirming === entry.sessionId && entry.sessionId && <div className="worktree-confirm">
          <p>清理「{entry.title}」的检出目录？将先保存恢复快照，并保留 Git 分支与会话记录。</p>
          <div className="worktree-actions"><button type="button" className="btn" disabled={busy} onClick={() => void clean(entry)}>确认清理</button>
            <button type="button" className="btn secondary" disabled={busy} onClick={() => setConfirming(null)}>取消</button></div>
        </div>}
      </article>)}
    </section>
    <section>
      <div className="worktree-section-heading"><h3>仓库环境</h3><button type="button" className="btn secondary" disabled={busy} onClick={() => void act(async () => {
        const path = await open({ directory: true, multiple: false, title: "选择 Git 仓库根目录" });
        if (typeof path === "string") { setRepository(path); setSelected(null); }
      })}>选择仓库</button></div>
      {repository ? <><code>{repository}</code><WorktreeLifecycleControls key={JSON.stringify([repository, selected])} repoRoot={repository} sessionId={selected ?? undefined} refreshKey={overview?.entries.find(entry => entry.sessionId === selected)?.present} onChange={reload} /></>
        : <p>选择仓库可配置新工作树的环境准备，或查看已保存的恢复快照。</p>}
    </section>
    {error && <p role="alert" className="error-text">{error}</p>}
    {notice && <p role="status">{notice}</p>}
  </div>;
}
