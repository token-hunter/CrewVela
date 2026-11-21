import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  repoRoot: string;
  sessionId?: string;
  refreshKey?: string | number | boolean;
  onChange?: () => void | Promise<void>;
}

interface Policy {
  command: string;
  files: string[];
  autoCleanup: boolean;
}

interface Snapshot {
  id: string;
  sessionId: string;
  branch: string;
  path: string;
  commit: string;
  gitRef: string;
  archivePath: string;
  createdAtMs: number;
}

interface Preparation {
  state: "running" | "ready" | "failed" | "unverified";
  updatedAtMs: number;
  lines: string[];
}

interface LifecycleView {
  policy: Policy;
  run: Preparation | null;
  snapshots: Snapshot[];
  warning: string | null;
}

const STATE_LABEL: Record<Preparation["state"], string> = {
  running: "准备中或上次执行中断",
  ready: "准备完成",
  failed: "准备失败",
  unverified: "准备进程尚未确认停止",
};

function LifecycleEditor({ repoRoot, sessionId, refreshKey, onChange }: Props) {
  const [view, setView] = useState<LifecycleView | null>(null);
  const [command, setCommand] = useState("");
  const [files, setFiles] = useState("");
  const [autoCleanup, setAutoCleanup] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const latest = useRef(0);
  const initialized = useRef(false);
  const lastRefresh = useRef(refreshKey);
  const load = useCallback(async (reset: boolean) => {
    const request = ++latest.current;
    try {
      const data = await invoke<LifecycleView>("worktree_lifecycle_get", { repoRoot, sessionId: sessionId ?? null });
      if (request !== latest.current) return;
      setView(data);
      if (reset || !initialized.current) {
        initialized.current = true;
        setCommand(data.policy.command);
        setFiles(data.policy.files.join("\n"));
        setAutoCleanup(data.policy.autoCleanup);
      }
    } catch (reason) {
      if (request === latest.current) setError(String(reason));
    }
  }, [repoRoot, sessionId]);

  useEffect(() => {
    void load(true);
    return () => { ++latest.current; };
  }, [load]);

  useEffect(() => {
    if (lastRefresh.current !== refreshKey) {
      lastRefresh.current = refreshKey;
      void load(false);
    }
  }, [refreshKey, load]);

  const entries = files.split(/\r?\n/).map((name) => name.trim()).filter(Boolean);
  const changed = !!view && (command !== view.policy.command || autoCleanup !== view.policy.autoCleanup ||
    JSON.stringify(entries) !== JSON.stringify(view.policy.files));

  async function action(name: string, args: Record<string, unknown>, success: string) {
    if (busy) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await invoke(name, args);
      setNotice(success);
      await load(name === "worktree_lifecycle_save");
      await onChange?.();
    } catch (reason) {
      setError(String(reason));
      await load(false);
    } finally {
      setBusy(false);
    }
  }

  const run = view?.run;
  const canPrepare = !!sessionId && refreshKey !== false && !!view && !changed && !busy &&
    (view.policy.command.trim() || view.policy.files.length > 0 || run?.state === "failed") &&
    run?.state !== "unverified" && run?.state !== "running";

  return (
    <div className="git-settings-rows" aria-busy={busy}>
      {!view && !error && <p role="status">正在读取项目配置…</p>}
      {view?.warning && <p role="status">{view.warning}</p>}
      {view && (
        <>
          <label className="git-settings-row">
            <span><strong>准备命令</strong><small>保存后，新建的 Worktree 会在启动会话前执行此命令。Windows 使用 cmd，macOS 和 Linux 使用 sh。</small></span>
            <input aria-label="准备命令" value={command} onChange={(e) => setCommand(e.target.value)} disabled={busy} />
          </label>
          <label className="git-settings-row">
            <span><strong>所需文件</strong><small>每行填写一个项目相对路径。从原仓库复制到 Worktree 中的相同位置。</small></span>
            <textarea rows={3} aria-label="所需文件" placeholder=".env.local" value={files} onChange={(e) => setFiles(e.target.value)} disabled={busy} />
          </label>
          <label className="git-settings-row">
            <span><strong>归档时自动回收</strong><small>回收无占用的干净 Worktree 前会保存恢复快照。含本地改动或忽略文件时保留目录。</small></span>
            <input type="checkbox" aria-label="归档时自动回收" checked={autoCleanup} onChange={(e) => setAutoCleanup(e.target.checked)} disabled={busy} />
          </label>
          <div className="settings-heading-actions">
            <button className="btn primary" type="button" disabled={!changed || busy} onClick={() => void action(
              "worktree_lifecycle_save", { repoRoot, policy: { command, files: entries, autoCleanup } }, "已保存此项目的配置",
            )}>保存项目配置</button>
            {sessionId && <button className="btn" type="button" disabled={!canPrepare} onClick={() => void action(
              "worktree_lifecycle_prepare", { sessionId }, "环境准备完成",
            )}>{run?.state === "failed" ? "重试准备" : "执行准备"}</button>}
          </div>
          {run && (
            <details key={run.updatedAtMs} open={run.state !== "ready"}>
              <summary>准备日志 · {STATE_LABEL[run.state]}</summary>
              <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "15rem", overflow: "auto" }}>{run.lines.join("\n")}</pre>
            </details>
          )}
          {view.snapshots.length > 0 && (
            <details>
              <summary>恢复快照（{view.snapshots.length}）</summary>
              {view.snapshots.map((snapshot) => (
                <div className="git-settings-row" key={snapshot.id}>
                  <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                    <strong>{snapshot.branch} · {snapshot.commit.slice(0, 10)}</strong>
                    <small>{new Date(snapshot.createdAtMs).toLocaleString()} · {snapshot.path}</small>
                    <small>{snapshot.gitRef}</small>
                    <small title={snapshot.archivePath}>文件快照 · {snapshot.archivePath}</small>
                  </span>
                  <button className="btn" type="button" disabled={busy} onClick={() => void action(
                    "worktree_lifecycle_restore", { repoRoot, snapshotId: snapshot.id }, "已恢复到原 Worktree 路径",
                  )}>恢复到原路径</button>
                </div>
              ))}
            </details>
          )}
        </>
      )}
      {notice && <p role="status">{notice}</p>}
      {error && <div className="git-settings-feedback error" role="alert">{error}</div>}
      {!view && error && <button className="btn" type="button" onClick={() => { setError(""); void load(true); }}>重新读取配置</button>}
    </div>
  );
}

export function WorktreeLifecycleControls(props: Props) {
  const [expanded, setExpanded] = useState(false);
  return (
    <details className="git-settings-section" onToggle={(event) => setExpanded(event.currentTarget.open)}>
      <summary>环境准备与恢复</summary>
      {expanded && <LifecycleEditor key={JSON.stringify([props.repoRoot, props.sessionId])} {...props} />}
    </details>
  );
}
