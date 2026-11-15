import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EmptyState } from "./ui/EmptyState";
import { useRuntimeAvailability } from "../runtimeAvailability";
import {
  ArrowLeft,
  ArrowClockwise,
  CaretDown,
  Check,
  CloudArrowDown,
  CloudArrowUp,
  DotsThree,
  GitBranch,
  MagicWand,
  MagnifyingGlass,
  Minus,
  Plus,
  Trash,
  Warning,
  X,
} from "@phosphor-icons/react";
import type {
  GitActionReceipt,
  GitBranchOption,
  GitCommittedEntry,
  GitFileEntry,
  GitSettings,
  GitStatusSnapshot,
  MultiAgentProject,
  MultiAgentSnapshot,
  Project,
  ProviderConfig,
  SessionRecord,
} from "../types";
import {
  GIT_SIDEBAR_ENTRY_LIMIT,
  limitGroupedEntries,
} from "../gitSidebarLimit";

interface Props {
  sessions: SessionRecord[];
  projects: Project[];
  providers: ProviderConfig[];
  keyStates: Record<string, boolean>;
  variant?: "workbench" | "sidebar";
  onClose?: () => void;
  preview?: boolean;
}

interface GitWorkspaceOption {
  id: string;
  kind: "project" | "session" | "agent";
  label: string;
  detail: string;
  cwd: string;
}

interface DiffSelection {
  key: string;
  path: string | null;
  area: "staged" | "unstaged" | "untracked" | "committed";
  label: string;
}

const DEFAULT_SETTINGS: GitSettings = {
  branchPrefix: "crewvela/",
  compareStrategy: "repository_default",
  pullStrategy: "ff_only",
  commitInstruction: "使用 Conventional Commits，准确概括已暂存变更。",
};


const PREVIEW_GIT_STATUS: GitStatusSnapshot = {
  cwd: "C:\\example\\crewvela",
  repoRoot: "C:\\example\\crewvela",
  branch: "feature/git-workbench",
  detached: false,
  head: "a84f5c2",
  upstream: "origin/feature/git-workbench",
  ahead: 3,
  behind: 0,
  mergeInProgress: false,
  defaultBase: "origin/main",
  compareBase: "origin/main",
  entries: [
    { path: "src/components/GitSourceControl.tsx", originalPath: null, status: ".M", staged: false, unstaged: true, untracked: false, conflicted: false, additions: 126, deletions: 18 },
    { path: "src/components/SettingsDialog.tsx", originalPath: null, status: "M.", staged: true, unstaged: false, untracked: false, conflicted: false, additions: 22, deletions: 11 },
    { path: "src/styles.css", originalPath: null, status: ".M", staged: false, unstaged: true, untracked: false, conflicted: false, additions: 84, deletions: 9 },
    { path: "tests/gitSourceControlPanel.test.mjs", originalPath: null, status: "??", staged: false, unstaged: false, untracked: true, conflicted: false, additions: 46, deletions: 0 },
  ],
  committedEntries: [],
};
const workspaceKey = (value: string) => {
  const normalized = value.replace(/\\/g, "/");
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith("//")
    ? normalized.toLowerCase()
    : normalized;
};

const changeSummary = (status: GitStatusSnapshot | null) => {
  if (!status) return "尚未读取";
  const conflicts = status.entries.filter((entry) => entry.conflicted).length;
  const staged = status.entries.filter((entry) => entry.staged).length;
  const pending = status.entries.filter((entry) => entry.unstaged || entry.untracked).length;
  if (conflicts) return `${conflicts} 个冲突`;
  if (pending) return `${pending} 个未提交`;
  if (staged) return `${staged} 个已暂存`;
  if (status.ahead) return `${status.ahead} 个提交待推送`;
  return "工作区干净";
};

const fileStats = (entry: { additions: number | null; deletions: number | null }) => (
  <span className="git-file-stats" aria-label="文件增删行数">
    {entry.additions !== null && <em>+{entry.additions}</em>}
    {entry.deletions !== null && <i>-{entry.deletions}</i>}
  </span>
);

const diffLineKind = (line: string) => {
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("+++") || line.startsWith("---")) return "header";
  if (line.startsWith("+")) return "addition";
  if (line.startsWith("-")) return "deletion";
  if (line.startsWith("diff ") || line.startsWith("index ")) return "meta";
  return "context";
};

export function GitSourceControlPanel({
  sessions,
  projects,
  providers,
  keyStates,
  variant = "workbench",
  onClose,
  preview = false,
}: Props) {
  const { isRuntimeEnabled, error: runtimeError } = useRuntimeAvailability();
  const [agentProjects, setAgentProjects] = useState<MultiAgentProject[]>([]);
  const [agentSnapshots, setAgentSnapshots] = useState<MultiAgentSnapshot[]>([]);
  const [settings, setSettings] = useState<GitSettings>(DEFAULT_SETTINGS);
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [status, setStatus] = useState<GitStatusSnapshot | null>(null);
  const [statusScope, setStatusScope] = useState("");
  const [workspaceStatuses, setWorkspaceStatuses] = useState<Record<string, GitStatusSnapshot | null>>({});
  const [selection, setSelection] = useState<DiffSelection | null>(null);
  const [diff, setDiff] = useState("");
  const [workspaceMessages, setWorkspaceMessages] = useState<Record<string, string>>({});
  const providerId = "local";
  const [loading, setLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const busy = loading || actionBusy;
  const actionInFlight = useRef(false);
  const [settingsReady, setSettingsReady] = useState(preview);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [filterQuery, setFilterQuery] = useState("");
  const [diffOpen, setDiffOpen] = useState(false);
  const [branches, setBranches] = useState<GitBranchOption[]>([]);
  const [mergeSource, setMergeSource] = useState("");
  const [mergeNoFf, setMergeNoFf] = useState(false);
  const [mergeDialogOpen, setMergeDialogOpen] = useState(false);
  const refreshRequestRef = useRef(0);
  const branchRequestRef = useRef(0);
  const scopeRef = useRef({ key: "", generation: 0 });

  const aiProviders = useMemo(
    () => providers.filter((provider) => keyStates[provider.id]),
    [keyStates, providers],
  );


  useEffect(() => {
    if (preview) {
      setSettings(DEFAULT_SETTINGS);
      return;
    }
    let cancelled = false;
    Promise.all([
      invoke<GitSettings>("git_settings_get"),
      invoke<MultiAgentProject[]>("ma_list_projects"),
    ])
      .then(async ([nextSettings, nextProjects]) => {
        const snapshots = await Promise.all(
          nextProjects.map((project) =>
            invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: project.id }).catch(() => null),
          ),
        );
        if (cancelled) return;
        setSettings(nextSettings);
        setSettingsReady(true);
        setAgentProjects(nextProjects);
        setAgentSnapshots(snapshots.filter((item): item is MultiAgentSnapshot => item !== null));
      })
      .catch((reason) => !cancelled && setError(String(reason)));
    return () => { cancelled = true; };
  }, [preview]);

  const workspaces = useMemo(() => {
    const items: GitWorkspaceOption[] = [];
    const seen = new Set<string>();
    const append = (item: GitWorkspaceOption) => {
      const key = workspaceKey(item.cwd);
      if (!item.cwd || seen.has(key)) return;
      seen.add(key);
      items.push(item);
    };
    projects.forEach((project) => append({
      id: `project:${project.id}`,
      kind: "project",
      label: project.name,
      detail: "项目仓库",
      cwd: project.path,
    }));
    agentProjects.forEach((project) => append({
      id: `agent-project:${project.id}`,
      kind: "project",
      label: project.name,
      detail: "协作项目仓库",
      cwd: project.rootPath,
    }));
    sessions.forEach((session) => {
      if (!session.worktree) return;
      append({
        id: `session:${session.id}`,
        kind: "session",
        label: session.title,
        detail: `会话 Worktree · ${session.worktree.branch}`,
        cwd: session.worktree.path,
      });
    });
    agentSnapshots.forEach((snapshot) => {
      snapshot.runs.forEach((run) => {
        const cwd = typeof run.sessionLocator?.cwd === "string" ? run.sessionLocator.cwd : null;
        if (!cwd || workspaceKey(cwd) === workspaceKey(snapshot.project.rootPath)) return;
        const task = snapshot.tasks.find((item) => item.id === run.taskId);
        const agent = snapshot.agents.find((item) => item.id === run.agentId);
        append({
          id: `agent-run:${run.id}`,
          kind: "agent",
          label: task?.title ?? `任务 ${run.attempt}`,
          detail: `${agent?.name ?? "Agent"} · Run ${run.attempt}`,
          cwd,
        });
      });
    });
    return items;
  }, [agentProjects, agentSnapshots, projects, sessions]);

  useEffect(() => {
    if (!selectedWorkspaceId && workspaces[0]) setSelectedWorkspaceId(workspaces[0].id);
    if (selectedWorkspaceId && !workspaces.some((item) => item.id === selectedWorkspaceId)) {
      setSelectedWorkspaceId(workspaces[0]?.id ?? null);
    }
  }, [selectedWorkspaceId, workspaces]);

  const selectedWorkspace = workspaces.find((item) => item.id === selectedWorkspaceId) ?? null;
  const selectedWorkspaceCwd = selectedWorkspace?.cwd ?? null;
  const scopeKey = JSON.stringify([selectedWorkspaceId, selectedWorkspaceCwd]);
  const message = workspaceMessages[scopeKey] ?? "";
  const setMessage = (value: string | ((current: string) => string)) => {
    setWorkspaceMessages(current => ({ ...current, [scopeKey]: typeof value === "function" ? value(current[scopeKey] ?? "") : value }));
  };
  if (scopeRef.current.key !== scopeKey) {
    scopeRef.current = { key: scopeKey, generation: scopeRef.current.generation + 1 };
  }
  useEffect(() => {
    setStatus(null);
    setSelection(null);
    setDiff("");
    setBranches([]);
    setMergeSource("");
    setMergeDialogOpen(false);
    setError(null);
    setNotice(null);
  }, [scopeKey]);
  const workspaceStatusKey = useMemo(
    () => workspaces.map((workspace) => `${workspace.id}:${workspaceKey(workspace.cwd)}`).join("|"),
    [workspaces],
  );
  const refreshBranches = useCallback(async () => {
    const generation = scopeRef.current.generation;
    const request = ++branchRequestRef.current;
    if (!selectedWorkspaceCwd) {
      setBranches([]);
      setMergeSource("");
      return;
    }
    try {
      const next = preview
        ? [
            { name: "feature/runtime-telemetry", remote: false },
            { name: "feature/session-navigation", remote: false },
            { name: "origin/main", remote: true },
          ]
        : await invoke<GitBranchOption[]>("git_branches", { cwd: selectedWorkspaceCwd });
      if (generation !== scopeRef.current.generation || request !== branchRequestRef.current) return;
      setBranches(next);
      setMergeSource((current) => (
        current && next.some((branch) => branch.name === current)
          ? current
          : next[0]?.name ?? ""
      ));
    } catch (reason) {
      if (generation !== scopeRef.current.generation || request !== branchRequestRef.current) return;
      setBranches([]);
      setMergeSource("");
      setError(String(reason));
    }
  }, [preview, selectedWorkspaceCwd]);

  useEffect(() => {
    void refreshBranches();
  }, [refreshBranches]);



  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const next: Record<string, GitStatusSnapshot | null> = {};
      for (const workspace of workspaces) {
        if (cancelled) return;
        next[workspace.id] = await invoke<GitStatusSnapshot>("git_status", {
          cwd: workspace.cwd,
          compareStrategy: settings.compareStrategy,
        }).catch(() => null);
      }
      if (!cancelled) setWorkspaceStatuses(next);
    };
    void load();
    return () => { cancelled = true; };
  }, [settings.compareStrategy, workspaceStatusKey]);

  const refresh = useCallback(async () => {
    const generation = scopeRef.current.generation;
    const requestId = refreshRequestRef.current + 1;
    refreshRequestRef.current = requestId;
    if (!selectedWorkspaceCwd || !selectedWorkspaceId) {
      setStatus(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const next = preview
        ? { ...PREVIEW_GIT_STATUS, cwd: selectedWorkspaceCwd, repoRoot: selectedWorkspaceCwd }
        : await invoke<GitStatusSnapshot>("git_status", {
            cwd: selectedWorkspaceCwd,
            compareStrategy: settings.compareStrategy,
          });
      if (requestId !== refreshRequestRef.current || generation !== scopeRef.current.generation) return;
      setStatus(next);
      setStatusScope(JSON.stringify([selectedWorkspaceId, selectedWorkspaceCwd]));
      setWorkspaceStatuses((current) => ({ ...current, [selectedWorkspaceId]: next }));
      setSelection((current) => {
        if (current) return current;
        const first = next.entries.find((entry) => entry.conflicted || entry.unstaged)
          ?? next.entries.find((entry) => entry.staged)
          ?? next.entries.find((entry) => entry.untracked);
        if (first) {
          const area = first.untracked ? "untracked" : first.unstaged || first.conflicted ? "unstaged" : "staged";
          return { key: `${area}:${first.path}`, path: first.path, area, label: first.path };
        }
        const committed = next.committedEntries[0];
        return committed ? { key: `committed:${committed.path}`, path: committed.path, area: "committed", label: committed.path } : null;
      });
    } catch (reason) {
      if (requestId !== refreshRequestRef.current || generation !== scopeRef.current.generation) return;
      setStatus(null);
      setSelection(null);
      setDiff("");
      setError(String(reason));
    } finally {
      if (requestId === refreshRequestRef.current) setLoading(false);
    }
  }, [preview, selectedWorkspaceCwd, selectedWorkspaceId, settings.compareStrategy]);

  useEffect(() => {
    void refresh();
    return () => { refreshRequestRef.current += 1; };
  }, [refresh]);

  useEffect(() => {
    if (!selectedWorkspaceCwd || !selection) {
      setDiff("");
      return;
    }
    if (preview) {
      setDiff([
        "diff --git a/src/components/GitSourceControl.tsx b/src/components/GitSourceControl.tsx",
        "@@ -42,6 +42,9 @@",
        "+  const stagedCount = entries.filter((entry) => entry.staged).length;",
        "+  const pendingCount = entries.filter((entry) => entry.unstaged).length;",
        "   return renderSourceControl();",
      ].join("\n"));
      return;
    }
    let cancelled = false;
    setDiff("");
    invoke<string>("git_diff", {
      cwd: selectedWorkspaceCwd,
      path: selection.path,
      area: selection.area,
      base: selection.area === "committed" ? status?.compareBase ?? null : null,
    })
      .then((value) => !cancelled && setDiff(value || "该范围没有文本差异。"))
      .catch((reason) => !cancelled && setDiff(`无法读取 Diff：${String(reason)}`));
    return () => { cancelled = true; };
  }, [preview, selectedWorkspaceCwd, selection, status?.compareBase]);

  // Keep a single mutation in flight, including while its workspace is hidden.
  // Only the initiating view may consume its receipt or error.
  const runAction = async <T,>(work: () => Promise<T>, apply: (value: T) => void) => {
    if (actionInFlight.current || loading) return;
    const generation = scopeRef.current.generation;
    actionInFlight.current = true;
    setActionBusy(true);
    setError(null);
    setNotice(null);
    ++refreshRequestRef.current;
    try {
      const value = await work();
      if (generation === scopeRef.current.generation) apply(value);
    } catch (reason) {
      if (generation === scopeRef.current.generation) setError(String(reason));
    } finally {
      actionInFlight.current = false;
      setActionBusy(false);
    }
  };

  const applyStatus = (next: GitStatusSnapshot) => {
    setStatus(next);
    setStatusScope(scopeKey);
    if (selectedWorkspace) {
      setWorkspaceStatuses((current) => ({ ...current, [selectedWorkspace.id]: next }));
    }
    setSelection(null);
    setDiff("");
  };
  const applyReceipt = (receipt: GitActionReceipt) => {
    applyStatus(receipt.status);
    if (receipt.warning) setError(receipt.message);
    else setNotice(receipt.message);
  };
  const workspaceReady = !!selectedWorkspace && !!status && statusScope === scopeKey;

  const mutate = async (command: "git_stage" | "git_unstage", paths: string[]) => {
    if (!selectedWorkspace || !workspaceReady || !paths.length) return;
    await runAction(
      () => invoke<GitStatusSnapshot>(command, { cwd: selectedWorkspace.cwd, paths }),
      next => { applyStatus(next); setNotice("操作完成"); },
    );
  };

  const discardPaths = async (paths: string[]) => {
    if (!selectedWorkspace || !workspaceReady || !paths.length || actionInFlight.current) return;
    if (!window.confirm(`永久丢弃 ${paths.length} 个文件的工作区变更？此操作不可撤销。`)) return;
    await runAction(
      () => invoke<GitStatusSnapshot>("git_discard", { cwd: selectedWorkspace.cwd, paths, confirmed: true }),
      next => { applyStatus(next); setNotice("操作完成"); },
    );
  };

  const runRemote = async (action: "fetch" | "pull" | "push" | "publish" | "force_push") => {
    if (!selectedWorkspace || !workspaceReady || actionInFlight.current) return;
    const confirmed = action !== "force_push" || window.confirm("使用 --force-with-lease 推送当前分支？请确认远端提交不会被意外覆盖。");
    if (!confirmed) return;
    await runAction(
      () => invoke<GitActionReceipt>("git_remote", { cwd: selectedWorkspace.cwd, action, confirmed }),
      applyReceipt,
    );
  };
  const runMerge = async () => {
    if (!selectedWorkspace || !workspaceReady || !mergeSource || status?.mergeInProgress || actionInFlight.current) return;
    if (status && status.entries.length > 0) {
      setError("工作区存在未提交变更，请先提交或清理后再合并。");
      return;
    }
    const currentBranch = status?.branch ?? "当前分支";
    if (!window.confirm(`将 ${mergeSource} 合并到 ${currentBranch}${mergeNoFf ? "（--no-ff）" : ""}？`)) return;
    if (preview) {
      setNotice(`已模拟将 ${mergeSource} 合并到 ${currentBranch}`);
      setMergeDialogOpen(false);
      return;
    }
    await runAction(
      () => invoke<GitActionReceipt>("git_merge", { cwd: selectedWorkspace.cwd, source: mergeSource, noFf: mergeNoFf, confirmed: true }),
      receipt => { applyReceipt(receipt); setMergeDialogOpen(false); void refreshBranches(); },
    );
  };

  const abortMerge = async () => {
    if (!selectedWorkspace || !workspaceReady || !status?.mergeInProgress || actionInFlight.current) return;
    if (!window.confirm("取消当前 Merge，并恢复到合并前状态？")) return;
    await runAction(
      () => invoke<GitActionReceipt>("git_merge_abort", { cwd: selectedWorkspace.cwd, confirmed: true }),
      receipt => { applyReceipt(receipt); void refreshBranches(); },
    );
  };

  const commit = async (pushAfter: boolean) => {
    if (!selectedWorkspace || !workspaceReady || !message.trim()) return;
    const submittedMessage = message;
    await runAction(
      () => invoke<GitActionReceipt>("git_commit", { cwd: selectedWorkspace.cwd, message: submittedMessage, pushAfter }),
      receipt => {
        applyReceipt(receipt);
        setMessage(current => current === submittedMessage ? "" : current);
      },
    );
  };

  const generateMessage = async () => {
    if (!isRuntimeEnabled("claude_code")) { setError(runtimeError ?? "请先启用 Claude Code 再生成提交信息"); return; }
    if (!selectedWorkspace || !workspaceReady || !providerId) return;
    const submittedMessage = message;
    await runAction(
      () => invoke<string>("git_generate_commit_message", { cwd: selectedWorkspace.cwd, providerId, instruction: settings.commitInstruction }),
      generated => setMessage(current => current === submittedMessage ? generated : current),
    );
  };

  const saveSettings = async () => {
    if (!settingsReady) return;
    await runAction(
      () => invoke<GitSettings>("git_settings_save", { settings }),
      saved => { setSettings(saved); setNotice("Git 设置已保存"); void refresh(); },
    );
  };

  const groups = useMemo(() => {
    if (!status) return [];
    return [
      { id: "conflicts", label: "冲突", area: "unstaged" as const, entries: status.entries.filter((entry) => entry.conflicted) },
      { id: "changes", label: "更改", area: "unstaged" as const, entries: status.entries.filter((entry) => entry.unstaged && !entry.conflicted) },
      { id: "staged", label: "已暂存", area: "staged" as const, entries: status.entries.filter((entry) => entry.staged && !entry.conflicted) },
      { id: "untracked", label: "未跟踪", area: "untracked" as const, entries: status.entries.filter((entry) => entry.untracked) },
    ].filter((group) => group.entries.length > 0);
  }, [status]);

  const selectFile = (entry: GitFileEntry | GitCommittedEntry, area: DiffSelection["area"]) => {
    setSelection({ key: `${area}:${entry.path}`, path: entry.path, area, label: entry.path });
    if (variant === "sidebar") setDiffOpen(true);
  };

  if (variant === "sidebar") {
    const normalizedFilter = filterQuery.trim().toLocaleLowerCase();
    const filteredSidebarGroups = groups
      .map((group) => ({
        ...group,
        entries: normalizedFilter
          ? group.entries.filter((entry) => entry.path.toLocaleLowerCase().includes(normalizedFilter))
          : group.entries,
      }))
      .filter((group) => group.entries.length > 0);
    const visibleEntries = limitGroupedEntries(
      filteredSidebarGroups.map((group) => group.entries),
      GIT_SIDEBAR_ENTRY_LIMIT,
    );
    const sidebarGroups = filteredSidebarGroups
      .map((group, index) => ({
        ...group,
        allEntries: group.entries,
        entries: visibleEntries[index],
      }))
      .filter((group) => group.entries.length > 0);
    const sidebarEntryCount = filteredSidebarGroups.reduce(
      (total, group) => total + group.entries.length,
      0,
    );
    const hiddenSidebarEntryCount = Math.max(
      0,
      sidebarEntryCount - GIT_SIDEBAR_ENTRY_LIMIT,
    );
    const additions = status?.entries.reduce((total, entry) => total + (entry.additions ?? 0), 0) ?? 0;
    const deletions = status?.entries.reduce((total, entry) => total + (entry.deletions ?? 0), 0) ?? 0;
    const stagedPaths = status?.entries.filter((entry) => entry.staged).map((entry) => entry.path) ?? [];
    const pendingPaths = status?.entries
      .filter((entry) => entry.unstaged || entry.untracked || entry.conflicted)
      .map((entry) => entry.path) ?? [];
    const primaryStages = stagedPaths.length === 0;
    const primaryDisabled = busy || (primaryStages ? pendingPaths.length === 0 : !message.trim());

    return (
      <aside className="git-source-sidebar" aria-label="Git 源代码控制">
        <header className="git-source-sidebar-header">
          <div className="git-source-sidebar-title">
            <GitBranch size={16} weight="bold" />
            <strong>源代码控制</strong>
          </div>
          <div className="git-source-sidebar-header-actions">
            <button type="button" aria-label="刷新 Git 状态" title="刷新 Git 状态" onClick={() => void refresh()} disabled={busy || !workspaceReady}>
              <ArrowClockwise size={15} className={busy ? "spin" : ""} />
            </button>
            {onClose && (
              <button type="button" aria-label="关闭源代码控制" title="关闭" onClick={onClose}>
                <X size={15} />
              </button>
            )}
          </div>
        </header>

        <div className="git-source-sidebar-toolbar">
          <label className="git-source-sidebar-search">
            <MagnifyingGlass size={14} />
            <input
              value={filterQuery}
              onChange={(event) => setFilterQuery(event.target.value)}
              placeholder="筛选文件"
              aria-label="筛选 Git 变更文件"
            />
          </label>
          <details className="git-source-sidebar-overflow">
            <summary aria-label="更多 Git 操作" title="更多 Git 操作"><DotsThree size={17} weight="bold" /></summary>
            <div>
              <button type="button" onClick={() => void runRemote("fetch")} disabled={busy || !workspaceReady}>Fetch</button>
              <button type="button" onClick={() => void runRemote("pull")} disabled={busy || !workspaceReady}>Pull</button>
              <button type="button" onClick={() => setMergeDialogOpen(true)} disabled={busy || status?.mergeInProgress || branches.length === 0}>
                Merge…
              </button>
              {status?.mergeInProgress && <button type="button" className="danger-text" onClick={() => void abortMerge()} disabled={busy || !workspaceReady}>取消合并</button>}
              <button type="button" onClick={() => void runRemote(status?.upstream ? "push" : "publish")} disabled={busy || !workspaceReady}>
                {status?.upstream ? "Push" : "Publish Branch"}
              </button>
              <button type="button" className="danger-text" onClick={() => void runRemote("force_push")} disabled={busy || !status?.upstream}>
                Force Push
              </button>
            </div>
          </details>
        </div>

        <section className="git-source-sidebar-context">
          <select
            value={selectedWorkspaceId ?? ""}
            onChange={(event) => {
              setSelectedWorkspaceId(event.target.value);
              setDiffOpen(false);
            }}
            aria-label="选择 Git 工作区"
          >
            {workspaces.map((workspace) => (
              <option value={workspace.id} key={workspace.id}>{workspace.label} · {workspace.detail}</option>
            ))}
          </select>
          <div className="git-source-sidebar-repo">
            <strong>{selectedWorkspace?.label ?? "未选择工作区"}</strong>
            <span className="git-source-sidebar-stats"><em>+{additions}</em><i>-{deletions}</i></span>
          </div>
          <div className="git-source-sidebar-branch">
            <GitBranch size={13} />
            <strong>{status?.branch ?? "Detached HEAD"}</strong>
            <span>↑{status?.ahead ?? 0} ↓{status?.behind ?? 0}</span>
          </div>
        </section>
        {status?.mergeInProgress && (
          <div className="git-source-sidebar-merge-state">
            <Warning size={15} />
            <span><strong>合并冲突待处理</strong><small>解决冲突后提交，或取消本次合并。</small></span>
            <button type="button" onClick={() => void abortMerge()} disabled={busy || !workspaceReady}>取消合并</button>
          </div>
        )}

        {error && <div className="git-source-sidebar-banner error"><Warning size={14} /><span>{error}</span></div>}
        {notice && !error && <div className="git-source-sidebar-banner notice"><Check size={14} /><span>{notice}</span></div>}

        <section className="git-source-sidebar-composer">
          <div className="git-source-sidebar-message">
            <textarea
              value={message}
              onChange={(event) => setMessage(event.target.value)}
              placeholder="提交信息"
              rows={3}
            />
            <button
              type="button"
              aria-label="AI 生成提交信息"
              title="AI 生成提交信息"
              onClick={() => void generateMessage()}
              disabled={!isRuntimeEnabled("claude_code") || busy || !providerId || stagedPaths.length === 0}
            >
              <MagicWand size={15} />
            </button>
          </div>
          <div className="git-source-sidebar-primary">
            <button
              type="button"
              onClick={() => {
                if (primaryStages) void mutate("git_stage", pendingPaths);
                else void commit(false);
              }}
              disabled={primaryDisabled}
            >
              {primaryStages ? <Plus size={15} /> : <Check size={15} />}
              {primaryStages ? "暂存全部" : "提交"}
            </button>
            <details>
              <summary aria-label="更多提交与同步操作"><CaretDown size={14} /></summary>
              <div>
                <button type="button" onClick={() => void commit(false)} disabled={busy || !workspaceReady || !message.trim()}>提交</button>
                <button type="button" onClick={() => void commit(true)} disabled={busy || !workspaceReady || !message.trim()}>提交并推送</button>
                <button type="button" onClick={() => void runRemote(status?.upstream ? "push" : "publish")} disabled={busy || !workspaceReady}>
                  {status?.upstream ? "Push" : "Publish Branch"}
                </button>
                <button type="button" onClick={() => void runRemote("pull")} disabled={busy || !workspaceReady}>Pull</button>
                <button type="button" onClick={() => void runRemote("fetch")} disabled={busy || !workspaceReady}>Fetch</button>
                <button type="button" onClick={() => setMergeDialogOpen(true)} disabled={busy || status?.mergeInProgress || branches.length === 0}>Merge…</button>
                {status?.mergeInProgress && <button type="button" className="danger-text" onClick={() => void abortMerge()} disabled={busy || !workspaceReady}>取消合并</button>}
                <button type="button" className="danger-text" onClick={() => void runRemote("force_push")} disabled={busy || !status?.upstream}>Force Push</button>
              </div>
            </details>
          </div>
        </section>

        <div className="git-source-sidebar-list">
          {hiddenSidebarEntryCount > 0 && (
            <div className="git-source-sidebar-limit">
              <Warning size={14} />
              <span>检测到 {sidebarEntryCount} 项，仅显示前 {GIT_SIDEBAR_ENTRY_LIMIT} 项。使用筛选可缩小范围。</span>
            </div>
          )}
          {sidebarGroups.map((group) => (
            <section className={`git-source-sidebar-group ${group.id}`} key={group.id}>
              <header>
                <strong>{group.label}<em>{group.allEntries.length}</em></strong>
                <span>
                  {group.id === "staged" ? (
                    <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_unstage", group.allEntries.map((entry) => entry.path))} title="全部取消暂存"><Minus size={13} /></button>
                  ) : (
                    <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_stage", group.allEntries.map((entry) => entry.path))} title="全部暂存"><Plus size={13} /></button>
                  )}
                  {(group.id === "changes" || group.id === "untracked") && (
                    <button type="button" className="danger-text" disabled={busy || !workspaceReady} onClick={() => void discardPaths(group.allEntries.map((entry) => entry.path))} title="全部丢弃"><Trash size={13} /></button>
                  )}
                </span>
              </header>
              {group.entries.map((entry) => (
                <div className="git-source-sidebar-file" key={`${group.id}:${entry.path}`}>
                  <button type="button" className="git-source-sidebar-file-main" onClick={() => selectFile(entry, group.area)}>
                    <i>{entry.status.replace(/\./g, "") || "M"}</i>
                    <span><strong>{entry.path.split("/").pop()}</strong><small>{entry.path}</small></span>
                    {fileStats(entry)}
                  </button>
                  <span className="git-source-sidebar-file-actions">
                    {group.id === "staged" ? (
                      <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_unstage", [entry.path])} title="取消暂存"><Minus size={12} /></button>
                    ) : (
                      <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_stage", [entry.path])} title="暂存"><Plus size={12} /></button>
                    )}
                  </span>
                </div>
              ))}
            </section>
          ))}
          {status && sidebarGroups.length === 0 && (
            <EmptyState
              className="git-source-sidebar-empty"
              icon={<Check size={22} />}
              title={normalizedFilter ? "没有匹配文件" : "工作区干净"}
              description={normalizedFilter ? "换个关键词继续筛选。" : "当前没有待处理的变更。"}
              descriptionAs="span"
            />
          )}
          {!status && !error && <EmptyState className="git-source-sidebar-empty" title="正在读取 Git 状态…" titleAs="span" />}
        </div>

        {mergeDialogOpen && (
          <section className="git-source-sidebar-merge-dialog" role="dialog" aria-modal="true" aria-label="合并分支">
            <header>
              <div><GitBranch size={16} /><span><strong>合并分支</strong><small>合并到 {status?.branch ?? "当前分支"}</small></span></div>
              <button type="button" onClick={() => setMergeDialogOpen(false)} aria-label="关闭合并分支"><X size={15} /></button>
            </header>
            <div className="git-source-sidebar-merge-form">
              <label>
                <span>来源分支</span>
                <select value={mergeSource} onChange={(event) => setMergeSource(event.target.value)}>
                  {branches.map((branch) => (
                    <option key={branch.name} value={branch.name}>{branch.name}{branch.remote ? "（远程）" : ""}</option>
                  ))}
                </select>
              </label>
              <label className="git-source-sidebar-merge-mode">
                <input type="checkbox" checked={mergeNoFf} onChange={(event) => setMergeNoFf(event.target.checked)} />
                <span><strong>创建 Merge Commit</strong><small>使用 --no-ff 保留独立分支历史。</small></span>
              </label>
              {!!status?.entries.length && (
                <div className="git-source-sidebar-merge-warning">
                  <Warning size={15} />
                  <span>工作区存在未提交变更，提交或清理后才能合并。</span>
                </div>
              )}
              <footer>
                <button type="button" className="btn" onClick={() => setMergeDialogOpen(false)}>取消</button>
                <button
                  type="button"
                  className="btn primary"
                  onClick={() => void runMerge()}
                  disabled={busy || !mergeSource || !!status?.entries.length || status?.mergeInProgress}
                >
                  {mergeNoFf ? "使用 --no-ff 合并" : "合并"}
                </button>
              </footer>
            </div>
          </section>
        )}


        {diffOpen && (
          <section className="git-source-sidebar-diff">
            <header>
              <button type="button" onClick={() => setDiffOpen(false)} aria-label="返回变更列表"><ArrowLeft size={15} /></button>
              <div><strong>{selection?.label ?? "Diff"}</strong><small>{selection?.area}</small></div>
            </header>
            <div className="git-source-sidebar-diff-content" tabIndex={0} role="document" aria-label="Git Diff">
              {(diff || "正在读取 Diff…").split("\n").map((line, index) => (
                <code className={diffLineKind(line)} key={`${index}:${line.slice(0, 24)}`}>{line || " "}</code>
              ))}
            </div>
          </section>
        )}
      </aside>
    );
  }


  return (
    <div className="git-workbench">
      <div className="git-command-bar">
        <label>
          <span>工作区</span>
          <select value={selectedWorkspaceId ?? ""} onChange={(event) => setSelectedWorkspaceId(event.target.value)}>
            {workspaces.map((workspace) => <option value={workspace.id} key={workspace.id}>{workspace.label} · {workspace.detail}</option>)}
          </select>
        </label>
        <label>
          <span>比较基准</span>
          <select
            disabled={busy || !settingsReady} value={settings.compareStrategy}
            onChange={(event) => setSettings((current) => ({ ...current, compareStrategy: event.target.value as GitSettings["compareStrategy"] }))}
          >
            <option value="repository_default">仓库默认分支</option>
            <option value="branch_upstream">当前分支 Upstream</option>
          </select>
        </label>
        <label className="git-prefix-field">
          <span>Worktree 分支前缀</span>
          <input disabled={busy || !settingsReady} value={settings.branchPrefix} onChange={(event) => setSettings((current) => ({ ...current, branchPrefix: event.target.value }))} />
        </label>
        <button type="button" className="btn" onClick={() => void saveSettings()} disabled={busy || !settingsReady}>保存设置</button>
        <button type="button" className="icon-button" aria-label="刷新 Git 状态" onClick={() => void refresh()} disabled={busy || !workspaceReady}>
          <ArrowClockwise size={17} className={busy ? "spin" : ""} />
        </button>
      </div>

      {error && <div className="git-error-banner"><Warning size={16} /> <span>{error}</span></div>}
      {notice && !error && <div className="git-notice-banner"><Check size={16} /> <span>{notice}</span></div>}

      <div className="git-workbench-grid">
        <aside className="git-workspace-rail">
          <header><span>工作区</span><em>{workspaces.length}</em></header>
          <div className="git-workspace-list">
            {workspaces.map((workspace) => (
              <button
                type="button"
                key={workspace.id}
                className={workspace.id === selectedWorkspaceId ? "active" : ""}
                onClick={() => setSelectedWorkspaceId(workspace.id)}
              >
                <GitBranch size={16} />
                <span><strong>{workspace.label}</strong><small>{workspace.detail}</small></span>
                <i>{changeSummary(workspace.id === selectedWorkspaceId ? status : workspaceStatuses[workspace.id] ?? null)}</i>
              </button>
            ))}
            {!workspaces.length && <p>暂无项目或 Worktree。</p>}
          </div>
          {status && (
            <footer>
              <strong>{status.branch ?? "Detached HEAD"}</strong>
              <span>{status.upstream ?? "尚未发布"}</span>
              <div><em>↑ {status.ahead}</em><i>↓ {status.behind}</i></div>
            </footer>
          )}
        </aside>

        <section className="git-change-pane">
          <header>
            <div><strong>源代码变更</strong><small>{status?.compareBase ? `比较 ${status.compareBase}` : "仅显示工作区变更"}</small></div>
            <div className="git-remote-actions">
              <button type="button" title="Fetch" onClick={() => void runRemote("fetch")} disabled={busy || !workspaceReady}><CloudArrowDown size={16} /></button>
              <button type="button" title="Pull --ff-only" onClick={() => void runRemote("pull")} disabled={busy || !workspaceReady}>Pull</button>
              <button type="button" title="Push" onClick={() => void runRemote(status?.upstream ? "push" : "publish")} disabled={busy || !workspaceReady}><CloudArrowUp size={16} />{status?.upstream ? "Push" : "Publish"}</button>
              <button type="button" className="danger-text" title="Force Push with lease" onClick={() => void runRemote("force_push")} disabled={busy || !status?.upstream}>Force</button>
            </div>
          </header>
          <div className="git-change-list">
            {groups.map((group) => (
              <section className={`git-change-group ${group.id}`} key={group.id}>
                <header>
                  <strong>{group.label}<em>{group.entries.length}</em></strong>
                  <span>
                    {(group.id === "staged") ? (
                      <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_unstage", group.entries.map((entry) => entry.path))} title="全部取消暂存"><Minus size={14} /></button>
                    ) : (
                      <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_stage", group.entries.map((entry) => entry.path))} title="全部暂存"><Plus size={14} /></button>
                    )}
                    {(group.id === "changes" || group.id === "untracked") && (
                      <button type="button" className="danger-text" disabled={busy || !workspaceReady} onClick={() => void discardPaths(group.entries.map((entry) => entry.path))} title="全部丢弃"><Trash size={14} /></button>
                    )}
                  </span>
                </header>
                {group.entries.map((entry) => (
                  <div className={`git-file-row${selection?.key === `${group.area}:${entry.path}` ? " active" : ""}`} key={`${group.id}:${entry.path}`}>
                    <button type="button" className="git-file-main" onClick={() => selectFile(entry, group.area)}>
                      <i>{entry.status.replace(/\./g, "") || "M"}</i>
                      <span><strong>{entry.path.split("/").pop()}</strong><small>{entry.path}</small></span>
                      {fileStats(entry)}
                    </button>
                    <span className="git-file-actions">
                      {group.id === "staged" ? (
                        <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_unstage", [entry.path])} title="取消暂存"><Minus size={13} /></button>
                      ) : (
                        <button type="button" disabled={busy || !workspaceReady} onClick={() => void mutate("git_stage", [entry.path])} title="暂存"><Plus size={13} /></button>
                      )}
                      {(group.id === "changes" || group.id === "untracked") && (
                        <button type="button" className="danger-text" disabled={busy || !workspaceReady} onClick={() => void discardPaths([entry.path])} title="丢弃"><Trash size={13} /></button>
                      )}
                    </span>
                  </div>
                ))}
              </section>
            ))}
            {!!status?.committedEntries.length && (
              <section className="git-change-group committed">
                <header><strong>已提交变更<em>{status.committedEntries.length}</em></strong><small>{status.compareBase}…HEAD</small></header>
                {status.committedEntries.map((entry) => (
                  <div className={`git-file-row${selection?.key === `committed:${entry.path}` ? " active" : ""}`} key={`committed:${entry.path}`}>
                    <button type="button" className="git-file-main" onClick={() => selectFile(entry, "committed")}>
                      <i>{entry.status}</i>
                      <span><strong>{entry.path.split("/").pop()}</strong><small>{entry.path}</small></span>
                      {fileStats(entry)}
                    </button>
                  </div>
                ))}
              </section>
            )}
            {status && !groups.length && !status.committedEntries.length && <div className="git-clean-state"><Check size={22} /><strong>没有待处理变更</strong><span>当前工作区与比较基准一致。</span></div>}
          </div>
        </section>

        <section className="git-detail-pane">
          <header>
            <div><strong>{selection?.label ?? "Diff"}</strong><small>{selection ? selection.area : "选择左侧文件查看差异"}</small></div>
          </header>
          <div className="git-diff-view" tabIndex={0} role="document" aria-label="Git Diff">
            {(diff || "选择一个变更查看完整 Diff。").split("\n").map((line, index) => (
              <code className={diffLineKind(line)} key={`${index}:${line.slice(0, 24)}`}>{line || " "}</code>
            ))}
          </div>
          <div className="git-commit-composer">
            <div className="git-ai-row">
              <span>Claude Code · 本地配置</span>
              <button type="button" onClick={() => void generateMessage()} disabled={!isRuntimeEnabled("claude_code") || busy || !providerId || !status?.entries.some((entry) => entry.staged)}>
                <MagicWand size={15} />AI 生成
              </button>
            </div>
            <textarea
              value={message}
              onChange={(event) => setMessage(event.target.value)}
              placeholder="输入提交信息，生成后仍可编辑…"
              rows={3}
            />
            <details>
              <summary>提交信息指令</summary>
              <textarea
                disabled={busy || !settingsReady} value={settings.commitInstruction}
                onChange={(event) => setSettings((current) => ({ ...current, commitInstruction: event.target.value }))}
                rows={2}
              />
            </details>
            <div className="git-commit-actions">
              <button type="button" className="btn" onClick={() => void commit(false)} disabled={busy || !workspaceReady || !message.trim()}>提交</button>
              <button type="button" className="btn primary" onClick={() => void commit(true)} disabled={busy || !workspaceReady || !message.trim()}>提交并推送</button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

export function GitSettingsPanel() {
  const [settings, setSettings] = useState<GitSettings>(DEFAULT_SETTINGS);
  const [savedSnapshot, setSavedSnapshot] = useState<GitSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [reload, setReload] = useState(0);
  const saveInFlight = useRef(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<GitSettings>("git_settings_get")
      .then((value) => {
        if (!cancelled) {
          setSettings(value);
          setSavedSnapshot(value);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => {
      cancelled = true;
    };
  }, [reload]);

  const save = async () => {
    if (saveInFlight.current || loading || !savedSnapshot) return;
    saveInFlight.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const saved = await invoke<GitSettings>("git_settings_save", { settings });
      setSettings(saved);
      setSavedSnapshot(saved);
      setNotice("Git 设置已保存");
    } catch (reason) {
      setError(String(reason));
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  const dirty = savedSnapshot !== null
    && JSON.stringify(settings) !== JSON.stringify(savedSnapshot);

  return (
    <div className="git-settings-only">
      <section className="git-settings-card">
        <header>
          <div><GitBranch size={18} /><span><strong>分支与比较</strong><small>控制 CrewVela 创建 Worktree 分支和审查变更时使用的默认规则。</small></span></div>
        </header>
        <label className="git-settings-row">
          <span><strong>Worktree 分支前缀</strong><small>新建会话或 Agent Worktree 时使用。</small></span>
          <input
            disabled={loading || busy || !savedSnapshot}
            value={settings.branchPrefix}
            onChange={(event) => setSettings((current) => ({ ...current, branchPrefix: event.target.value }))}
            placeholder="crewvela/"
          />
        </label>
        <label className="git-settings-row">
          <span><strong>默认比较基准</strong><small>只影响 Diff 和变更统计，不改变合并目标。</small></span>
          <select
            disabled={loading || busy || !savedSnapshot}
            value={settings.compareStrategy}
            onChange={(event) => setSettings((current) => ({ ...current, compareStrategy: event.target.value as GitSettings["compareStrategy"] }))}
          >
            <option value="repository_default">仓库默认分支</option>
            <option value="branch_upstream">当前分支 Upstream</option>
          </select>
        </label>
        <label className="git-settings-row">
          <span><strong>Pull 策略</strong><small>同步远端时使用的合并方式；发生分叉时仅快进会停止并显示原因。</small></span>
          <select
            disabled={loading || busy || !savedSnapshot}
            value={settings.pullStrategy}
            onChange={(event) => setSettings((current) => ({ ...current, pullStrategy: event.target.value as GitSettings["pullStrategy"] }))}
          >
            <option value="ff_only">仅快进合并（--ff-only）</option>
            <option value="rebase">变基合并（--rebase）</option>
          </select>
        </label>
      </section>

      <section className="git-settings-card">
        <header>
          <div><MagicWand size={18} /><span><strong>提交信息</strong><small>作为 Agent 生成提交说明时的固定要求，生成后仍可在 Git 面板中编辑。</small></span></div>
        </header>
        <label className="git-settings-block">
          <span>提交信息指令</span>
          <textarea
            disabled={loading || busy || !savedSnapshot}
            value={settings.commitInstruction}
            onChange={(event) => setSettings((current) => ({ ...current, commitInstruction: event.target.value }))}
            rows={5}
          />
        </label>
      </section>

      <section className="git-settings-card git-settings-safety">
        <header>
          <div><Warning size={18} /><span><strong>安全保证</strong><small>这些底线由 CrewVela 固定执行，无需也无法修改。</small></span></div>
        </header>
        <ul className="git-settings-guarantees">
          <li><Check size={14} weight="bold" /><span><strong>推送保护</strong>强制推送必须手动确认，并始终使用 <code>--force-with-lease</code>，不会覆盖远端新增提交。</span></li>
          <li><Check size={14} weight="bold" /><span><strong>同步策略</strong>仅快进模式遇到分叉会停止；选择变基模式时，Pull 会重新应用本地提交，冲突需要手动处理。</span></li>
        </ul>
      </section>

      {error && <div className="git-settings-feedback error"><Warning size={15} />{error}</div>}
      {notice && !error && <div className="git-settings-feedback success"><Check size={15} />{notice}</div>}
      <div className="git-settings-actions">
        {!loading && !savedSnapshot && <button type="button" className="btn" onClick={() => setReload(value => value + 1)}>重新读取</button>}
        {dirty && <span className="git-settings-dirty">有未保存更改</span>}
        <button type="button" className="btn primary" onClick={() => void save()} disabled={busy || loading || !savedSnapshot}>
          {busy ? "保存中…" : "保存设置"}
        </button>
      </div>
    </div>
  );
}
