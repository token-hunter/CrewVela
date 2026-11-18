import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "./ui/Modal";

export interface CollaborationProjectDraft {
  name: string;
  goal: string;
  rootPath: string;
}

interface GitStatus {
  state: "ready" | "missingRepository" | "missingCommit";
  repositoryPath: string | null;
  hasFiles: boolean;
  stagedFiles: string[];
  stagedToken: string | null;
}

interface GitSetup {
  confirmExistingFiles: boolean;
  stagedToken?: string;
}

const ABSOLUTE_PATH = /^(?:\/|[A-Za-z]:[\\/]|\\\\)/;

export function CollaborationProjectDialog({ onCreate, onClose, preview = false }: {
  onCreate: (draft: CollaborationProjectDraft, gitSetup?: GitSetup) => Promise<void>;
  onClose: () => void;
  preview?: boolean;
}) {
  const [draft, setDraft] = useState<CollaborationProjectDraft>({ name: "", goal: "", rootPath: "" });
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [picking, setPicking] = useState(false);
  const [gitCheck, setGitCheck] = useState<{ path: string; status?: GitStatus; error?: string } | null>(null);
  const [initialize, setInitialize] = useState<boolean | null>(null);
  const [includeStaged, setIncludeStaged] = useState(false);
  const [checkAttempt, setCheckAttempt] = useState(0);
  const inFlight = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const busy = creating || picking;
  const rootPath = draft.rootPath.trim();
  const currentCheck = gitCheck?.path === rootPath ? gitCheck : null;
  const gitStatus = currentCheck?.status;
  const checkingGit = ABSOLUTE_PATH.test(rootPath) && !currentCheck;
  const initializeGit = !!gitStatus && gitStatus.state !== "ready" && (initialize ?? !gitStatus.hasFiles);

  useEffect(() => {
    if (!ABSOLUTE_PATH.test(rootPath)) return;
    let current = true;
    const timeout = window.setTimeout(() => {
      const request = preview ? Promise.resolve<GitStatus>({ state: "missingRepository", repositoryPath: null, hasFiles: false, stagedFiles: [], stagedToken: null })
        : invoke<GitStatus>("ma_project_git_status", { rootPath });
      void request.then(status => { if (current) setGitCheck({ path: rootPath, status }); },
        reason => { if (current) setGitCheck({ path: rootPath, error: String(reason) }); });
    }, 160);
    return () => { current = false; window.clearTimeout(timeout); };
  }, [rootPath, checkAttempt, preview]);

  const resetGitChoice = () => { setGitCheck(null); setInitialize(null); setIncludeStaged(false); };

  const pickDirectory = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setPicking(true);
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择协作项目目录" });
      if (typeof selected === "string") {
        if (selected.trim() !== rootPath) resetGitChoice();
        setDraft(current => ({ ...current, rootPath: selected,
          name: current.name || selected.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "" }));
      }
    } catch (reason) { setError(`选择目录失败：${String(reason)}`); }
    finally { inFlight.current = false; setPicking(false); }
  };

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (inFlight.current || checkingGit) return;
    const input = { name: draft.name.trim(), goal: draft.goal.trim(), rootPath: draft.rootPath.trim() };
    if (!input.name || !input.goal || !input.rootPath) {
      setError("请填写项目名称、项目目录和项目目标。");
      return;
    }
    if (!ABSOLUTE_PATH.test(input.rootPath)) {
      setError("项目目录请填写本地绝对路径，或点击「选择目录」。");
      return;
    }
    inFlight.current = true;
    setCreating(true);
    setError(null);
    const stagedToken = includeStaged ? gitStatus?.stagedToken : null;
    const gitSetup = initializeGit ? { confirmExistingFiles: !!gitStatus?.hasFiles,
      ...(stagedToken ? { stagedToken } : {}) } : undefined;
    try { await onCreate(input, gitSetup); onClose(); }
    catch (reason) { setError(`创建项目失败：${String(reason)}`); }
    finally { inFlight.current = false; setCreating(false); }
  };

  return createPortal(<Modal className="collab-agent-editor collab-project-editor" overlayClassName="dialog-overlay collab-agent-editor-overlay" ariaLabel="创建协作项目" initialFocusRef={nameRef} onClose={() => { if (!inFlight.current) onClose(); }}>
    <header><h3>创建协作项目</h3><button type="button" aria-label="关闭创建项目" disabled={busy} onClick={onClose}>×</button></header>
    <p>选择项目目录并填写目标，创建后即可在这里添加团队成员。</p>
    <form onSubmit={event => void create(event)} noValidate>
      <label>项目名称<input ref={nameRef} value={draft.name} disabled={busy} onChange={event => setDraft({ ...draft, name: event.target.value })} placeholder="例如：CrewVela" /></label>
      <label>项目目录<input value={draft.rootPath} disabled={busy} onChange={event => {
        if (event.target.value.trim() !== rootPath) resetGitChoice();
        setDraft({ ...draft, rootPath: event.target.value });
      }} placeholder="输入项目的本地绝对路径，或选择目录" /></label>
      <button className="btn secondary" type="button" disabled={busy} onClick={() => void pickDirectory()}>{picking ? "选择中…" : "选择目录"}</button>
      {ABSOLUTE_PATH.test(rootPath) && <div className="collab-project-git">
        {checkingGit && <p role="status">正在检查 Git 工作区…</p>}
        {gitStatus?.state === "ready" && <p role="status">将复用现有 Git 仓库 <span>{gitStatus.repositoryPath}</span></p>}
        {gitStatus && gitStatus.state !== "ready" && <>
          <label><input type="checkbox" checked={initializeGit} disabled={busy} onChange={event => setInitialize(event.target.checked)} />
            {gitStatus.state === "missingRepository" ? "初始化 Git 并创建首次提交" : "创建首次 Git 提交"}</label>
          {gitStatus.repositoryPath && <p>仓库目录 <span>{gitStatus.repositoryPath}</span></p>}
          <p>{gitStatus.hasFiles ? "目录已有内容。默认创建空提交，未暂存的文件不会纳入分工工作树。" : "创建一条空提交，用于团队分工的工作树。"}</p>
          {gitStatus.stagedFiles.length > 0 && <>
            <label><input type="checkbox" checked={includeStaged} disabled={busy || !initializeGit} onChange={event => setIncludeStaged(event.target.checked)} />将已暂存文件纳入首次提交</label>
            <ul aria-label="已暂存文件">{gitStatus.stagedFiles.map((file, index) => <li key={index}>{file}</li>)}</ul>
          </>}
        </>}
        {currentCheck?.error && <p role="status">{currentCheck.error} 可先创建项目用于团队聊天。</p>}
        {(currentCheck?.error || (error && initializeGit)) && <button type="button" className="btn secondary" disabled={busy} onClick={() => {
          resetGitChoice(); setError(null); setCheckAttempt(attempt => attempt + 1);
        }}>重新检查 Git</button>}
      </div>}
      <label>项目目标<textarea aria-label="项目目标" rows={3} value={draft.goal} disabled={busy} onChange={event => setDraft({ ...draft, goal: event.target.value })} placeholder="描述团队要完成的工作和验收要求" /></label>
      {error && <div className="collab-agent-error" role="alert">{error}</div>}
      <footer><button className="btn secondary" type="button" disabled={busy} onClick={onClose}>取消</button><button className="btn primary" type="submit" disabled={busy || checkingGit}>{creating ? "创建中…" : "创建项目"}</button></footer>
    </form>
  </Modal>, document.body);
}
