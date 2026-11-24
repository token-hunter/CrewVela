import { useTranslation } from "../useInterfaceLanguage";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowsClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowsClockwise";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { CopyIcon } from "@phosphor-icons/react/dist/csr/Copy";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { WorktreeLifecycleControls } from "./WorktreeLifecycleControls";
import { RUNTIME_LABELS } from "../runtimeLogos";
import type { AgentRuntimeKind, WorktreeSettingsStyle } from "../types";
import "../styles/worktree-settings.css";
import "../styles/settings-creative-modes.css";

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
type FilterKind = "all" | "session" | "collab";

function normalizePath(value: string): string {
  return value.trim().replace(/[\\/]+$/, "");
}

function resolveSubdirectory(basePath: string, cwd: string): string | null {
  const normalizedBase = normalizePath(basePath);
  const normalizedCwd = normalizePath(cwd);
  if (!normalizedCwd || normalizedBase.toLowerCase() === normalizedCwd.toLowerCase()) return null;
  const lowerBase = normalizedBase.toLowerCase();
  const lowerCwd = normalizedCwd.toLowerCase();
  if (lowerCwd.startsWith(`${lowerBase}\\`) || lowerCwd.startsWith(`${lowerBase}/`)) {
    return `./${normalizedCwd.slice(normalizedBase.length + 1).replace(/\\/g, "/")}`;
  }
  return normalizedCwd;
}

function formatBaseRef(baseRef: string | null): string | null {
  if (!baseRef) return null;
  return baseRef.replace(/^refs\/(heads|remotes)\//, "");
}

function formatEntryMeta(entry: Entry): string {
  const base = formatBaseRef(entry.baseRef);
  const branchText = entry.branch ? (base ? `${entry.branch} ← ${base}` : entry.branch) : (base ? `基准 ${base}` : null);
  return [
    entry.project || "未分组",
    entry.runtime ? RUNTIME_LABELS[entry.runtime] : "协作任务",
    entry.archived && "已归档",
    !entry.present && "已清理",
    branchText,
  ].filter(Boolean).join(" · ");
}

function greenhouseState(entry: Entry): {
  toneClass: "is-active" | "is-protected" | "is-autumn" | "is-dormant";
  leafColor: string;
  label: string;
} {
  if (!entry.present) {
    return { toneClass: "is-dormant", leafColor: "var(--text-dim)", label: "已清理休眠" };
  }
  if (entry.blocked || !entry.sessionId) {
    return { toneClass: "is-protected", leafColor: "#6aa6f8", label: !entry.sessionId ? "协作保护" : "受保护" };
  }
  if (entry.archived) {
    return { toneClass: "is-autumn", leafColor: "var(--warn, #e5a94c)", label: "秋叶可修剪" };
  }
  return { toneClass: "is-active", leafColor: "var(--success)", label: "活跃生长" };
}

export function WorktreeSettingsPanel({
  viewStyle = "classic",
}: {
  viewStyle?: WorktreeSettingsStyle;
} = {}) {
  const t = useTranslation();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [repository, setRepository] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [filter, setFilter] = useState<FilterKind>("all");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const sequence = useRef(0);
  const action = useRef(false);
  const copyTimer = useRef<number | null>(null);
  const repoSectionRef = useRef<HTMLElement>(null);

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

  useEffect(() => {
    void reload();
    return () => {
      sequence.current++;
      if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    };
  }, [reload]);

  const act = async (task: () => Promise<void>) => {
    if (action.current) return;
    action.current = true; setBusy(true); setError(""); setNotice("");
    try { await task(); } catch (reason) { setError(String(reason)); }
    finally { action.current = false; setBusy(false); }
  };

  const copyPath = (key: string, value: string) => {
    if (!value) return;
    void navigator.clipboard?.writeText(value).catch(() => {});
    setCopiedKey(key);
    if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => {
      setCopiedKey((current) => (current === key ? null : current));
    }, 1500);
  };

  const chooseRoot = (reset = false) => act(async () => {
    const root = reset ? null : await open({ directory: true, multiple: false, title: "选择工作树根目录" });
    if (!reset && typeof root !== "string") return;
    await invoke("worktree_set_root", { root });
    await reload();
    setNotice("已更新新建工作树的位置。");
  });

  const inspect = (entry: Entry) => {
    setSelected(entry.sessionId);
    setRepository(entry.repoRoot);
    setConfirming(null);
    window.requestAnimationFrame(() => {
      repoSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });
  };

  const clean = (entry: Entry) => act(async () => {
    await invoke("worktree_clean", { sessionId: entry.sessionId });
    setConfirming(null);
    await reload();
    setNotice("已保存恢复快照并清理工作树，原分支和会话历史已保留。");
  });

  const allEntries = overview?.entries ?? [];
  const sessionCount = allEntries.filter((entry) => !!entry.sessionId).length;
  const collabCount = allEntries.length - sessionCount;
  const visibleEntries = allEntries.filter((entry) => {
    if (filter === "session") return !!entry.sessionId;
    if (filter === "collab") return !entry.sessionId;
    return true;
  });
  const selectedEntry = selected ? allEntries.find((entry) => entry.sessionId === selected) ?? null : null;

  return (
    <div className="worktree-settings">
      {error && <p role="alert" className="worktree-banner error-text">{error}</p>}
      {notice && !error && (
        <p role="status" className="worktree-banner worktree-notice">
          <CheckIcon size={13} weight="bold" aria-hidden="true" />
          <span>{notice}</span>
        </p>
      )}

      <section className="worktree-section worktree-root-section">
        <div className="worktree-root-header">
          <div className="worktree-section-copy">
            <h3>工作树根目录</h3>
            <p>新建的会话工作树存放在此目录。更改后，已有工作树继续使用原位置。</p>
          </div>
          <div className="worktree-actions">
            <button type="button" className="btn" disabled={busy} onClick={() => void chooseRoot()}>更改根目录</button>
            <button type="button" className="btn secondary worktree-btn-quiet" disabled={busy} onClick={() => void chooseRoot(true)}>恢复默认目录</button>
          </div>
        </div>
        {overview && (
          <div className="worktree-path-row">
            <button
              type="button"
              className={`worktree-path-copy${copiedKey === "root" ? " copied" : ""}`}
              title={`${overview.root}（点击复制）`}
              onClick={() => copyPath("root", overview.root)}
            >
              <code>{overview.root}</code>
              <span className="worktree-copy-hint" aria-hidden="true">
                {copiedKey === "root" ? <><CheckIcon size={12} weight="bold" /> 已复制</> : <><CopyIcon size={12} /> {t("复制")}</>}
              </span>
            </button>
          </div>
        )}
      </section>

      <section className="worktree-section worktree-list-section">
        <div className="worktree-section-heading">
          <div className="worktree-list-heading-left">
            <h3>托管工作树</h3>
            {allEntries.length > 0 && (
              <div className="worktree-filter-tabs" role="tablist" aria-label="筛选托管工作树">
                <button
                  type="button"
                  role="tab"
                  aria-selected={filter === "all"}
                  className={`worktree-filter-tab${filter === "all" ? " active" : ""}`}
                  onClick={() => setFilter("all")}
                >
                  {t("全部")} {allEntries.length}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={filter === "session"}
                  className={`worktree-filter-tab${filter === "session" ? " active" : ""}`}
                  onClick={() => setFilter("session")}
                >
                  {t("会话")} {sessionCount}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={filter === "collab"}
                  className={`worktree-filter-tab${filter === "collab" ? " active" : ""}`}
                  onClick={() => setFilter("collab")}
                >
                  协作任务 {collabCount}
                </button>
              </div>
            )}
          </div>
          <button type="button" className="btn secondary worktree-btn-quiet" disabled={busy} onClick={() => void reload()}>
            <ArrowsClockwiseIcon size={13} aria-hidden="true" />
            <span>{t("刷新")}</span>
          </button>
        </div>

        {!overview && !error && <p role="status" className="worktree-empty">正在读取工作树…</p>}
        {overview && !allEntries.length && (
          <p className="worktree-empty">暂无托管工作树。新建会话时可勾选「使用独立工作树」。</p>
        )}
        {overview && allEntries.length > 0 && !visibleEntries.length && (
          <p className="worktree-empty">当前筛选下没有工作树。</p>
        )}

        {visibleEntries.length > 0 && viewStyle === "greenhouse" && (
          <div className="wt-greenhouse-grid" aria-label="工作树温室苗圃架">
            {visibleEntries.map((entry) => {
              const entryKey = entry.sessionId || entry.path;
              const subdir = resolveSubdirectory(entry.path, entry.cwd);
              const isSelected = !!entry.sessionId && selected === entry.sessionId;
              const isCopied = copiedKey === entryKey;
              const pot = greenhouseState(entry);
              const branchLabel = entry.branch || formatBaseRef(entry.baseRef) || "未命名分支";

              return (
                <article
                  key={entryKey}
                  className={`wt-pot-card${isSelected ? " selected" : ""}`}
                >
                  <div className="wt-pot-top">
                    <span className={`wt-pot-pill ${pot.toneClass}`}>{pot.label}</span>
                    <span className="wt-pot-kind">
                      {entry.project || t("未分组")} · {entry.runtime ? RUNTIME_LABELS[entry.runtime] : "协作任务"}
                    </span>
                  </div>

                  <div className="wt-pot-main">
                    <svg viewBox="0 0 72 72" className="wt-pot-svg" aria-hidden="true">
                      <path d="M22 52 L50 52 L45 66 L27 66 Z" fill="var(--accent)" opacity="0.82" />
                      <rect x="20" y="48" width="32" height="5" rx="2" fill="var(--accent)" />
                      <path d="M36 48 L36 27" stroke="var(--text-dim)" strokeWidth="2.5" strokeLinecap="round" />
                      {entry.present ? (
                        <>
                          <path d="M36 37 C26 35, 22 25, 28 20 C34 22, 36 29, 36 37 Z" fill={pot.leafColor} opacity="0.92" />
                          <path d="M36 32 C46 30, 50 20, 44 15 C38 17, 36 24, 36 32 Z" fill={pot.leafColor} opacity="0.74" />
                          <circle cx="36" cy="18" r="5.5" fill={pot.leafColor} />
                        </>
                      ) : (
                        <circle cx="36" cy="27" r="4" fill="var(--border-strong)" />
                      )}
                    </svg>
                    <div className="wt-pot-copy">
                      <strong title={entry.title}>{entry.title}</strong>
                      <span className="wt-pot-branch" title={branchLabel}>{branchLabel}</span>
                      {entry.blocked && <p className="wt-pot-blocked">{entry.blocked}</p>}
                    </div>
                  </div>

                  <div className="wt-pot-footer">
                    <div className="worktree-path-row">
                      <button
                        type="button"
                        className={`worktree-path-copy${isCopied ? " copied" : ""}`}
                        title={`${entry.path}（点击复制）`}
                        onClick={() => copyPath(entryKey, entry.path)}
                      >
                        <code>{entry.path}</code>
                        <span className="worktree-copy-hint" aria-hidden="true">
                          {isCopied ? <><CheckIcon size={12} weight="bold" /> 已复制</> : <><CopyIcon size={12} /> {t("复制")}</>}
                        </span>
                      </button>
                      {subdir && (
                        <span className="worktree-subdir" title={`会话工作目录：${entry.cwd}`}>
                          子目录 <code>{subdir}</code>
                        </span>
                      )}
                    </div>

                    <div className="wt-pot-actions">
                      <button
                        type="button"
                        className="btn secondary worktree-btn-quiet"
                        disabled={busy || !entry.present}
                        onClick={() => void act(async () => {
                          await invoke("workspace_open_path", { root: entry.path, relativePath: "." });
                        })}
                      >
                        <FolderOpenIcon size={13} aria-hidden="true" />
                        <span>打开目录</span>
                      </button>
                      {entry.sessionId && (
                        <>
                          <button
                            type="button"
                            className="btn secondary worktree-btn-quiet"
                            disabled={busy}
                            onClick={() => inspect(entry)}
                          >
                            种子与恢复
                          </button>
                          <button
                            type="button"
                            className="btn secondary worktree-btn-quiet worktree-btn-danger"
                            disabled={busy || !!entry.blocked || !entry.present}
                            onClick={() => { setConfirming(entry.sessionId); setNotice(""); }}
                          >
                            修剪回收
                          </button>
                        </>
                      )}
                    </div>

                    {confirming === entry.sessionId && entry.sessionId && (
                      <div className="worktree-confirm">
                        <p>清理「{entry.title}」的检出目录？将先保存恢复快照，并保留 Git 分支与会话记录。</p>
                        <div className="worktree-actions">
                          <button type="button" className="btn primary" disabled={busy} onClick={() => void clean(entry)}>确认清理</button>
                          <button type="button" className="btn secondary worktree-btn-quiet" disabled={busy} onClick={() => setConfirming(null)}>{t("取消")}</button>
                        </div>
                      </div>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        )}

        {visibleEntries.length > 0 && viewStyle !== "greenhouse" && (
          <div className="worktree-entries">
            {visibleEntries.map((entry) => {
              const entryKey = entry.sessionId || entry.path;
              const subdir = resolveSubdirectory(entry.path, entry.cwd);
              const isSelected = !!entry.sessionId && selected === entry.sessionId;
              const isCopied = copiedKey === entryKey;

              return (
                <article
                  key={entryKey}
                  className={`worktree-entry${isSelected ? " selected" : ""}${!entry.present ? " missing" : ""}`}
                >
                  <strong className="worktree-entry-title">
                    <GitBranchIcon size={15} aria-hidden="true" />
                    <span>{entry.title}</span>
                  </strong>

                  <div className="worktree-entry-body">
                    <p className="worktree-entry-meta">{formatEntryMeta(entry)}</p>
                    <div className="worktree-path-row">
                      <button
                        type="button"
                        className={`worktree-path-copy${isCopied ? " copied" : ""}`}
                        title={`${entry.path}（点击复制）`}
                        onClick={() => copyPath(entryKey, entry.path)}
                      >
                        <code>{entry.path}</code>
                        <span className="worktree-copy-hint" aria-hidden="true">
                          {isCopied ? <><CheckIcon size={12} weight="bold" /> 已复制</> : <><CopyIcon size={12} /> {t("复制")}</>}
                        </span>
                      </button>
                      {subdir && (
                        <span className="worktree-subdir" title={`会话工作目录：${entry.cwd}`}>
                          子目录 <code>{subdir}</code>
                        </span>
                      )}
                    </div>
                    {entry.blocked && <p className="worktree-entry-blocked">{entry.blocked}</p>}
                  </div>

                  <div className="worktree-actions worktree-entry-actions">
                    <button
                      type="button"
                      className="btn secondary worktree-btn-quiet"
                      disabled={busy || !entry.present}
                      onClick={() => void act(async () => {
                        await invoke("workspace_open_path", { root: entry.path, relativePath: "." });
                      })}
                    >
                      <FolderOpenIcon size={14} aria-hidden="true" />
                      <span>打开目录</span>
                    </button>
                    {entry.sessionId && (
                      <>
                        <button
                          type="button"
                          className="btn secondary worktree-btn-quiet"
                          disabled={busy}
                          onClick={() => inspect(entry)}
                        >
                          管理环境与恢复
                        </button>
                        <button
                          type="button"
                          className="btn secondary worktree-btn-quiet worktree-btn-danger"
                          disabled={busy || !!entry.blocked || !entry.present}
                          onClick={() => { setConfirming(entry.sessionId); setNotice(""); }}
                        >
                          清理工作树
                        </button>
                      </>
                    )}
                  </div>

                  {confirming === entry.sessionId && entry.sessionId && (
                    <div className="worktree-confirm">
                      <p>清理「{entry.title}」的检出目录？将先保存恢复快照，并保留 Git 分支与会话记录。</p>
                      <div className="worktree-actions">
                        <button type="button" className="btn primary" disabled={busy} onClick={() => void clean(entry)}>确认清理</button>
                        <button type="button" className="btn secondary worktree-btn-quiet" disabled={busy} onClick={() => setConfirming(null)}>{t("取消")}</button>
                      </div>
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        )}
      </section>

      <section className="worktree-section worktree-repo-section" ref={repoSectionRef}>
        <div className="worktree-section-heading">
          <div className="worktree-section-copy">
            <h3>仓库环境</h3>
            {!repository && <p>选择仓库可配置新工作树的环境准备，或查看已保存的恢复快照。</p>}
            {repository && (
              <p className="worktree-repo-context">
                <span>当前仓库</span>
                <code className="worktree-inline-code">{repository}</code>
                {selectedEntry && <span className="worktree-repo-selected">· 已关联「{selectedEntry.title}」</span>}
              </p>
            )}
          </div>
          <button
            type="button"
            className="btn secondary"
            disabled={busy}
            onClick={() => void act(async () => {
              const path = await open({ directory: true, multiple: false, title: "选择 Git 仓库根目录" });
              if (typeof path === "string") { setRepository(path); setSelected(null); }
            })}
          >
            选择仓库
          </button>
        </div>
        {repository && (
          <WorktreeLifecycleControls
            key={JSON.stringify([repository, selected])}
            repoRoot={repository}
            sessionId={selected ?? undefined}
            refreshKey={overview?.entries.find((entry) => entry.sessionId === selected)?.present}
            onChange={reload}
          />
        )}
      </section>
    </div>
  );
}
