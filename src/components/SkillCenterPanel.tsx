import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { EmptyState } from "./ui/EmptyState";
import {
  ArrowClockwise,
  CaretDown,
  DotsThree,
  DownloadSimple,
  FileCode,
  FolderOpen,
  GitBranch,
  Package,
  Plus,
  ShieldCheck,
  Trash,
  Warning,
  MagnifyingGlass,
  Check,
  Code,
  Browser,
  FileText,
  Funnel,
} from "@phosphor-icons/react";
import { SkillMarketPanel } from "./SkillMarketPanel";
import { SkillUpdatesPanel } from "./SkillUpdatesPanel";
import type { AgentRuntimeKind, Project } from "../types";
import { RUNTIME_LOGOS } from "../runtimeLogos";
import { Modal } from "./ui/Modal";
import { distributionAssets, distributionSelection, scopedDistribution, type DistributionFilter } from "../skillDistribution";

interface Props { projects: Project[]; }

interface RuntimeAdapter {
  runtime: string; label: string; globalRoot: string; projectRoot: string;
  discoveryRoots: string[]; recursiveScan: boolean; installed: boolean;
}
interface SkillAsset {
  id: string; name: string; description: string; sourceType: string; sourceRef: string | null;
  sourceRevision: string | null; centralPath: string; contentHash: string; trustState: string;
  fileCount: number; scriptCount: number; riskSummary: string[]; updatedAt: number;
}
interface Deployment {
  id: string; skillId: string; runtime: string; scope: string; projectPath: string | null;
  taskId: string | null; agentId: string | null; targetPath: string; mode: string; status: string;
  sourceHash: string; targetHash: string | null; lastError: string | null; updatedAt: number;
}
interface SkillSetItem {
  skillId: string; runtime: string; scope: string; projectPath: string | null;
  taskId: string | null; agentId: string | null;
}
interface SkillSetRecord {
  id: string; name: string; description: string; projectPath: string | null; agentRole: string | null;
  items: SkillSetItem[]; updatedAt: number;
}
interface Snapshot { assets: SkillAsset[]; deployments: Deployment[]; skillSets: SkillSetRecord[]; runtimes: RuntimeAdapter[]; }
interface DiscoveredSkill {
  runtime: string; path: string; name: string; description: string; contentHash: string; alreadyManaged: boolean;
}
interface SkillPreview { skillId: string; skillMd: string; files: Array<{ path: string; size: number; isScript: boolean }>; }

const SOURCE_LABELS: Record<string, string> = { git: "Git 仓库", local: "本地导入", archive: "压缩包", adopted: "Agent 扫描" };
function skillIcon(name: string) {
  if (/browser/i.test(name)) return Browser;
  if (/commit|git/i.test(name)) return GitBranch;
  if (/research|search/i.test(name)) return MagnifyingGlass;
  if (/review/i.test(name)) return FileText;
  if (/code|frontend|design/i.test(name)) return Code;
  return FileText;
}

const STATUS: Record<string, { label: string; tone: string }> = {
  synced: { label: "已同步", tone: "ok" },
  center_newer: { label: "中央较新", tone: "info" },
  runtime_newer: { label: "本地修改", tone: "warn" },
  conflict: { label: "冲突", tone: "danger" },
  failed: { label: "失败", tone: "danger" },
  unverified: { label: "未验证", tone: "muted" },
};

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
const previewRuntimes = [
  ["claude_code", "Claude Code", ".claude/skills"], ["codex", "Codex", ".codex/skills"],
  ["kimi_code", "Kimi Code", ".config/agents/skills"],
  ["pi", "Pi", ".pi/skills"],
  ["opencode", "OpenCode", ".opencode/skills"],
] as const;
const PREVIEW_SNAPSHOT: Snapshot = {
  assets: [
    { id: "frontend", name: "frontend-design", description: "高质量前端界面设计", sourceType: "git", sourceRef: "github.com/example/skills", sourceRevision: "f34a9c1", centralPath: "…/skill-center/assets/frontend-design", contentHash: "63c81c4417f13d2c", trustState: "trusted", fileCount: 8, scriptCount: 1, riskSummary: [], updatedAt: 0 },
    { id: "review", name: "code-review", description: "代码审查与回归检查", sourceType: "adopted", sourceRef: "~/.codex/skills/code-review", sourceRevision: null, centralPath: "…/skill-center/assets/code-review", contentHash: "1f9b782ec81e973a", trustState: "trusted", fileCount: 5, scriptCount: 0, riskSummary: [], updatedAt: 0 },
    { id: "research", name: "research", description: "基于一手资料的研究工作流", sourceType: "local", sourceRef: "D:/skills/research", sourceRevision: null, centralPath: "…/skill-center/assets/research", contentHash: "ae732f0bc13b802e", trustState: "unknown", fileCount: 12, scriptCount: 2, riskSummary: ["包含 2 个脚本或可执行文件"], updatedAt: 0 },
  ],
  deployments: [
    { id: "d1", skillId: "frontend", runtime: "claude_code", scope: "global", projectPath: null, taskId: null, agentId: null, targetPath: "~/.claude/skills/frontend-design", mode: "junction", status: "synced", sourceHash: "63c81c4417f13d2c", targetHash: "63c81c4417f13d2c", lastError: null, updatedAt: 0 },
    { id: "d2", skillId: "frontend", runtime: "codex", scope: "global", projectPath: null, taskId: null, agentId: null, targetPath: "~/.codex/skills/frontend-design", mode: "junction", status: "synced", sourceHash: "63c81c4417f13d2c", targetHash: "63c81c4417f13d2c", lastError: null, updatedAt: 0 },
    { id: "d3", skillId: "review", runtime: "codex", scope: "global", projectPath: null, taskId: null, agentId: null, targetPath: "~/.codex/skills/code-review", mode: "copy", status: "runtime_newer", sourceHash: "1f9b782ec81e973a", targetHash: "changed", lastError: null, updatedAt: 0 },
  ],
  skillSets: [{ id: "set1", name: "前端开发", description: "Claude + Codex 前端协作", projectPath: null, agentRole: null, items: [], updatedAt: 0 }],
  runtimes: previewRuntimes.map(([runtime, label, projectRoot]) => ({ runtime, label, projectRoot, globalRoot: `~/${projectRoot}`, discoveryRoots: [], recursiveScan: false, installed: true })),
};

export function SkillCenterPanel({ projects }: Props) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [selectedAssetId, setSelectedAssetId] = useState<string | null>(null);
  const [selectedSkillIds, setSelectedSkillIds] = useState<Set<string>>(new Set());
  const [runtimeIds, setRuntimeIds] = useState<string[] | null>(null);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<DistributionFilter>("all");
  const [rowActionsId, setRowActionsId] = useState<string | null>(null);
  const [showSaveSet, setShowSaveSet] = useState(false);
  const importMenu = useRef<HTMLDetailsElement>(null);
  const toolsMenu = useRef<HTMLDetailsElement>(null);
  const [showRuntimeDetails, setShowRuntimeDetails] = useState(false);
  const [scope, setScope] = useState<"global" | "project" | "task">("global");
  const [projectPath, setProjectPath] = useState(projects[0]?.path ?? "");
  const [taskWorkspacePath, setTaskWorkspacePath] = useState("");
  const [taskId, setTaskId] = useState("");
  const [agentId, setAgentId] = useState("");
  const [preview, setPreview] = useState<SkillPreview | null>(null);
  const [previewFile, setPreviewFile] = useState("SKILL.md");
  const [previewContent, setPreviewContent] = useState("");
  const [activeTab, setActiveTab] = useState<"matrix" | "market" | "sets" | "preview" | "updates">("matrix");
  const [discovered, setDiscovered] = useState<DiscoveredSkill[]>([]);
  const [showDiscovery, setShowDiscovery] = useState(false);
  const [showGitImport, setShowGitImport] = useState(false);
  const [gitUrl, setGitUrl] = useState("");
  const [gitBranch, setGitBranch] = useState("");
  const [gitSubpath, setGitSubpath] = useState("");
  const [gitRevision, setGitRevision] = useState("");
  const [setName, setSetName] = useState("");
  const [setAgentRole, setSetAgentRole] = useState("");
  const [busy, setBusy] = useState(false);
  const operationInFlight = useRef(false);
  const loadSequence = useRef(0);
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(null);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    const next = UI_PREVIEW ? PREVIEW_SNAPSHOT : await invoke<Snapshot>("skill_center_snapshot");
    if (sequence !== loadSequence.current) return;
    setSnapshot(next);
    setSelectedAssetId((current) => current && next.assets.some((asset) => asset.id === current) ? current : next.assets[0]?.id ?? null);
  }, []);

  useEffect(() => { let disposed = false; void load().catch((error) => { if (!disposed) setMessage({ tone: "error", text: String(error) }); }); return () => { disposed = true; loadSequence.current++; }; }, [load]);

  const selectedAsset = snapshot?.assets.find((asset) => asset.id === selectedAssetId) ?? null;
  const effectiveProject = scope === "global" ? null : scope === "task" ? taskWorkspacePath || null : projectPath || null;
  const scopedDeployments = useMemo(() => scopedDistribution(snapshot?.deployments ?? [], scope, effectiveProject, taskId, agentId), [snapshot, scope, effectiveProject, taskId, agentId]);

  const deploymentFor = (skillId: string, runtime: string) => scopedDeployments.find((item) => item.skillId === skillId && item.runtime === runtime);
  const defaultRuntime = snapshot?.runtimes.find(runtime => runtime.runtime === "claude_code") ?? snapshot?.runtimes[0];
  const selectedRuntimes = snapshot?.runtimes.filter(runtime => (runtimeIds ?? [defaultRuntime?.runtime]).includes(runtime.runtime)) ?? [];
  const visibleAssets = distributionAssets(snapshot?.assets ?? [], scopedDeployments, search, statusFilter, selectedRuntimes.map(runtime => runtime.runtime));
  const selectedSkills = visibleAssets.filter(asset => selectedSkillIds.has(asset.id));
  const selectedKeys = distributionSelection(selectedSkillIds, selectedRuntimes.map(runtime => runtime.runtime), visibleAssets);
  const targetLabel = selectedRuntimes.map(runtime => runtime.label).join("、") || "未选择 Agent";
  const rowAsset = snapshot?.assets.find(asset => asset.id === rowActionsId);
  useEffect(() => { setSelectedSkillIds(new Set()); }, [scope, effectiveProject, taskId, agentId, search, statusFilter]);

  const run = async (work: () => Promise<void>, success?: string) => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    setBusy(true); setMessage(null);
    let workCompleted = false;
    try { await work(); workCompleted = true; await load(); if (success) setMessage({ tone: "success", text: success }); }
    catch (error) {
      const failures = [String(error)];
      if (!workCompleted) { try { await load(); } catch (refreshError) { failures.push(`刷新状态失败：${String(refreshError)}`); } }
      setMessage({ tone: "error", text: failures.join("；") });
    }
    finally { operationInFlight.current = false; setBusy(false); }
  };

  const importPaths = async (sourceType: "local" | "archive") => {
    await run(async () => {
      const selected = await open(sourceType === "local"
        ? { directory: true, multiple: true, title: "选择 Skill 目录" }
        : { multiple: true, title: "选择 Skill 压缩包", filters: [{ name: "Skill 包", extensions: ["zip", "skill"] }] });
      const paths = typeof selected === "string" ? [selected] : selected ?? [];
      const failures: string[] = [];
      let imported = 0;
      for (const source of paths) {
        try { await invoke("skill_center_import", { input: { sourceType, source } }); imported++; }
        catch (error) { failures.push(`${source}：${String(error)}`); }
      }
      if (failures.length) throw new Error(`已导入 ${imported} 个来源；失败 ${failures.length} 个：${failures.join("；")}`);
      if (paths.length) setMessage({ tone: "success", text: `已导入 ${imported} 个来源` });
    });
  };

  const importGit = async () => {
    if (!gitUrl.trim()) { setMessage({ tone: "error", text: "请输入 Git 仓库地址" }); return; }
    await run(async () => {
      await invoke("skill_center_import", { input: {
        sourceType: "git", source: gitUrl.trim(), branch: gitBranch.trim() || null,
        subpath: gitSubpath.trim() || null, revision: gitRevision.trim() || null,
      } });
      setShowGitImport(false); setGitUrl(""); setGitBranch(""); setGitSubpath(""); setGitRevision("");
    }, "Git Skill 已进入中央库");
  };

  const scan = async () => {
    await run(async () => {
      setDiscovered(await invoke<DiscoveredSkill[]>("skill_center_discover", { projectPaths: projects.map((project) => project.path) }));
      setShowDiscovery(true);
    });
  };

  const adopt = async (item: DiscoveredSkill) => {
    await run(async () => { await invoke("skill_center_import", { input: { sourceType: "adopted", source: item.path } }); }, `已纳管 ${item.name}`);
  };

  const toggleSkill = (key: string) => setSelectedSkillIds((current) => {
    const next = new Set(current); next.has(key) ? next.delete(key) : next.add(key); return next;
  });

  const toggleRuntime = (runtime: string) => setRuntimeIds(value => {
    const current = value ?? selectedRuntimes.map(item => item.runtime);
    return current.includes(runtime) ? current.filter(item => item !== runtime) : [...current, runtime];
  });

  const deploySelected = async () => {
    if (!selectedKeys.length) return;
    if (scope === "project" && !projectPath) { setMessage({ tone: "error", text: "请选择项目" }); return; }
    if (scope === "task" && !taskWorkspacePath.trim()) { setMessage({ tone: "error", text: "请选择任务或 Agent 的独立工作目录 / Worktree" }); return; }
    if (scope === "task" && !taskId.trim() && !agentId.trim()) { setMessage({ tone: "error", text: "任务作用域至少填写任务 ID 或 Agent ID" }); return; }
    const inputs = selectedKeys.map((key) => {
      const [skillId, runtime] = key.split("::");
      return { skillId, runtime, scope, projectPath: effectiveProject, taskId: scope === "task" ? taskId.trim() || null : null, agentId: scope === "task" ? agentId.trim() || null : null, trustConfirmed: true };
    });
    const untrusted = selectedSkills.filter(asset => asset.trustState !== "trusted");
    if (untrusted.length && !window.confirm(`以下 Skill 尚未信任：\n${untrusted.map((asset) => `• ${asset.name}${asset.riskSummary.length ? `（${asset.riskSummary.join("；")}）` : ""}`).join("\n")}\n\n确认审阅来源并信任后部署？`)) return;
    await run(async () => { await invoke("skill_center_deploy_batch", { inputs }); setSelectedSkillIds(new Set()); }, `已完成 ${selectedSkills.length} 个 Skill 向 ${selectedRuntimes.length} 个 Agent 的安全分发`);
  };

  const undeploySelected = async () => {
    const deployments = selectedKeys
      .map((key) => { const [skillId, runtime] = key.split("::"); return deploymentFor(skillId, runtime); })
      .filter((item): item is Deployment => Boolean(item));
    if (!deployments.length) return;
    if (!window.confirm(`撤下选中的 ${deployments.length} 个受管部署？任何存在本地修改的副本都会使整批操作取消。`)) return;
    await run(async () => {
      await invoke("skill_center_undeploy_batch", { deploymentIds: deployments.map((item) => item.id) });
      setSelectedSkillIds(new Set());
    }, `已安全撤下 ${deployments.length} 个部署`);
  };

  const removeDeployment = async (deployment: Deployment) => {
    if (!window.confirm(`撤下 ${deployment.targetPath}？Runtime 中被修改的副本不会被删除。`)) return;
    await run(async () => { await invoke("skill_center_undeploy", { deploymentId: deployment.id }); }, "已撤下受管部署");
  };

  const trust = async (state: "trusted" | "blocked" | "unknown") => {
    if (!selectedAsset) return;
    await run(async () => { await invoke("skill_center_set_trust", { skillId: selectedAsset.id, trustState: state }); }, "信任状态已更新");
  };

  const showPreview = async (asset: SkillAsset) => {
    if (operationInFlight.current) return;
    setSelectedAssetId(asset.id); setActiveTab("preview");
    setPreview(null); setPreviewContent("");
    await run(async () => {
      const next = await invoke<SkillPreview>("skill_center_preview", { skillId: asset.id });
      setPreview(next);
      setPreviewFile("SKILL.md");
      setPreviewContent(next.skillMd);
    });
  };

  const readPreviewFile = async (path: string) => {
    if (!selectedAsset) return;
    await run(async () => {
      const content = await invoke<string>("skill_center_read_file", {
        skillId: selectedAsset.id,
        relativePath: path,
      });
      setPreviewFile(path);
      setPreviewContent(content);
    });
  };

  const saveSet = async () => {
    if (!setName.trim() || !selectedKeys.length) { setMessage({ tone: "error", text: "填写配置集名称并选择至少一个 Skill" }); return; }
    const items = selectedKeys.map((key) => {
      const [skillId, runtime] = key.split("::");
      return { skillId, runtime, scope, projectPath: effectiveProject, taskId: scope === "task" ? taskId.trim() || null : null, agentId: scope === "task" ? agentId.trim() || null : null };
    });
    await run(async () => { await invoke("skill_center_save_set", { input: { name: setName.trim(), items, projectPath: effectiveProject, agentRole: setAgentRole || null } }); setSetName(""); setShowSaveSet(false); }, "Skill 配置集已保存");
  };

  const applySet = async (set: SkillSetRecord) => {
    const untrusted = set.items
      .map((item) => snapshot?.assets.find((asset) => asset.id === item.skillId))
      .filter((asset): asset is SkillAsset => Boolean(asset && asset.trustState !== "trusted"));
    if (untrusted.length && !window.confirm(`配置集包含尚未信任的 Skill：${untrusted.map((asset) => asset.name).join("、")}。确认审阅并信任后应用？`)) return;
    await run(async () => { await invoke("skill_center_apply_set", { id: set.id, trustConfirmed: true }); }, `已应用“${set.name}”`);
  };

  if (!snapshot) return <div className="skill-center-loading">{message?.tone === "error" ? <><p role="alert">{message.text}</p><button className="btn" disabled={busy} onClick={() => void run(async () => {})}>重新读取中央库</button></> : <><ArrowClockwise className="spin" /> 正在读取 Skill 中央库…</>}</div>;

  return (
    <section className="skill-center skill-workspace skill-runtime-first skill-card-library" data-skill-view={activeTab} aria-label="Skills">
      <fieldset disabled={busy} style={{ display: "contents" }}>
      <header className="skill-center-toolbar">
        <h2 id="settings-title">Skills</h2>
        <div className="skill-center-tabs" role="tablist" aria-label="Skill 管理视图" onKeyDown={event => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
          const index = tabs.indexOf(event.target as HTMLButtonElement);
          if (index < 0) return;
          event.preventDefault();
          const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
          tabs[next].focus(); tabs[next].click();
        }}>
          <button type="button" role="tab" aria-selected={activeTab !== "market"} tabIndex={activeTab !== "market" ? 0 : -1} className={activeTab !== "market" ? "active" : ""} onClick={() => setActiveTab("matrix")}>我的</button>
          <button type="button" role="tab" title="Skill 市场" aria-selected={activeTab === "market"} tabIndex={activeTab === "market" ? 0 : -1} className={activeTab === "market" ? "active" : ""} onClick={() => setActiveTab("market")}>发现</button>
        </div>
        {activeTab === "matrix" && <div className="skill-library-search-tools"><label className="skill-filter-search"><MagnifyingGlass aria-hidden="true" /><input type="search" value={search} onChange={event => setSearch(event.target.value)} placeholder="搜索 Skill" aria-label="搜索 Skill" /></label>
          <details className="skill-filter-menu" onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }}><summary aria-label="筛选 Skills" title="分发状态筛选"><Funnel size={18} />{statusFilter !== "all" && <span className="sr-only">筛选已启用</span>}</summary><div><label>分发状态<select aria-label="分发状态筛选" value={statusFilter} onChange={event => setStatusFilter(event.target.value as DistributionFilter)}><option value="all">全部状态</option><option value="undeployed">未部署</option><option value="synced">已同步</option><option value="attention">需要处理</option><option value="untrusted">待审阅 / 已阻止</option></select></label></div></details>
        </div>}
        {activeTab !== "market" && <details className="skill-import-menu" ref={importMenu} onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }}>
          <summary className="btn primary"><DownloadSimple />导入 Skill<CaretDown /></summary>
          <div className="skill-center-actions" onClick={() => { if (importMenu.current) importMenu.current.open = false; }}>
            <button className="btn" onClick={() => void importPaths("local")} disabled={busy}><FolderOpen />导入目录</button>
            <button className="btn" onClick={() => void importPaths("archive")} disabled={busy}><Package />导入包</button>
            <button className="btn" onClick={() => setShowGitImport(true)} disabled={busy}><GitBranch />Git 导入</button>
          </div>
        </details>}
      </header>

      {message && <div role={message.tone === "error" ? "alert" : "status"} className={`skill-center-message ${message.tone}`}>{message.text}</div>}

      {activeTab !== "matrix" && activeTab !== "market" && <div className="skill-library-subview"><button type="button" onClick={() => setActiveTab("matrix")}>返回我的 Skills</button><h3>{activeTab === "updates" ? "版本更新" : activeTab === "sets" ? "配置集" : "文件预览与信任审阅"}</h3></div>}
      <div className={activeTab === "matrix" ? "skill-distribution-body" : "skill-secondary-body"}>
      {activeTab === "matrix" && <div className="skill-library-targets"><span>部署到 Agent</span><div className="skill-runtime-tabs" role="group" aria-label="分发 Runtime" onKeyDown={event => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button"));
        const index = buttons.indexOf(event.target as HTMLButtonElement);
        if (index < 0) return;
        event.preventDefault();
        const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }}>{snapshot.runtimes.map(runtime => <button type="button" data-runtime={runtime.runtime} className={selectedRuntimes.includes(runtime) ? "active" : ""} aria-pressed={selectedRuntimes.includes(runtime)} key={runtime.runtime} disabled={busy} onClick={() => toggleRuntime(runtime.runtime)}>
        {RUNTIME_LOGOS[runtime.runtime as AgentRuntimeKind] ? <img src={RUNTIME_LOGOS[runtime.runtime as AgentRuntimeKind]} alt="" /> : <Package />}<span>{runtime.label}</span><Check className="skill-runtime-check" size={14} aria-hidden="true" />
      </button>)}</div></div>}
      <div className={`skill-center-scopebar${activeTab === "market" || activeTab === "updates" ? " hidden" : ""}`}>
        <label>作用域<select aria-label="作用域" value={scope} onChange={(event) => setScope(event.target.value as typeof scope)}><option value="global">全局</option><option value="project">项目</option><option value="task">任务 / Agent</option></select></label>
        {scope === "project" && <label>项目<select value={projectPath} onChange={(event) => setProjectPath(event.target.value)}><option value="">选择项目</option>{projects.map((project) => <option key={project.id} value={project.path}>{project.name}</option>)}</select></label>}
        {scope === "task" && <><label>独立工作目录<input value={taskWorkspacePath} onChange={(event) => setTaskWorkspacePath(event.target.value)} placeholder="任务 Worktree 绝对路径" /></label><button className="btn" type="button" onClick={() => void run(async () => { const path = await open({ directory: true, title: "选择任务 Worktree" }); if (typeof path === "string") setTaskWorkspacePath(path); })}>选择目录</button><label>任务 ID<input value={taskId} onChange={(event) => setTaskId(event.target.value)} placeholder="可选" /></label><label>Agent ID<input value={agentId} onChange={(event) => setAgentId(event.target.value)} placeholder="可选" /></label></>}
        {activeTab === "matrix" && <><span className="skill-library-count">{visibleAssets.length === snapshot.assets.length ? `${snapshot.assets.length} 个 Skills` : `${visibleAssets.length} / ${snapshot.assets.length} 个 Skills`}</span><label className="skill-library-select-all"><input type="checkbox" aria-label="选择当前列表全部 Skill" disabled={!visibleAssets.length} checked={!!visibleAssets.length && selectedSkills.length === visibleAssets.length} ref={input => { if (input) input.indeterminate = selectedSkills.length > 0 && selectedSkills.length < visibleAssets.length; }} onChange={event => setSelectedSkillIds(new Set(event.target.checked ? visibleAssets.map(asset => asset.id) : []))} />全选</label></>}
        <details className="skill-tools-menu" ref={toolsMenu} onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }}>
          <summary aria-label="更多 Skill 操作">更多<CaretDown size={14} /></summary>
          <div className="skill-center-actions" onClick={() => { if (toolsMenu.current) toolsMenu.current.open = false; }}>
            <button className="btn" onClick={() => setActiveTab("updates")}><ArrowClockwise />版本更新</button>
            <button className="btn" onClick={() => setActiveTab("sets")}><Package />配置集{snapshot.skillSets.length > 0 && <span>{snapshot.skillSets.length}</span>}</button>
            <button className="btn" onClick={() => selectedAsset ? void showPreview(selectedAsset) : setActiveTab("preview")}><FileCode />文件预览</button>
            <button className="btn" onClick={() => void scan()}><ArrowClockwise />扫描现有 Skill</button>
            <button className="btn" disabled={!selectedRuntimes.length} onClick={() => setShowRuntimeDetails(true)}><FolderOpen />目标详情</button>
            <button className="btn" disabled={!selectedKeys.some(key => { const [skillId, runtime] = key.split("::"); return Boolean(deploymentFor(skillId, runtime)); })} onClick={() => void undeploySelected()}><Trash />撤下选中</button>
          </div>
        </details>
      </div>

      {activeTab === "market" && <SkillMarketPanel onInstalled={load} />}
      {activeTab === "updates" && <SkillUpdatesPanel assets={snapshot.assets} deployments={snapshot.deployments} initialSkillId={selectedAssetId} onChanged={load} />}

      {activeTab === "matrix" && (
        <>
        <div className="skill-library-grid" role="list" aria-label="我的 Skills">
          {visibleAssets.map(asset => {
            const Icon = skillIcon(asset.name);
            return <article key={asset.id} role="listitem" className={`skill-library-card${selectedSkillIds.has(asset.id) ? " selected" : ""}`} aria-label={asset.name}>
              <div className="skill-library-icon" aria-hidden="true"><Icon size={32} weight="light" /></div>
              <input className="skill-card-checkbox" type="checkbox" aria-label={`选择 ${asset.name}`} checked={selectedSkillIds.has(asset.id)} onChange={() => toggleSkill(asset.id)} />
              <div className="skill-library-card-copy">
                <button className="skill-card-title" title={`查看 ${asset.name} 文件与信任状态`} onClick={() => void showPreview(asset)}>{asset.name}</button>
                <p className="skill-card-description" title={asset.description}>{asset.description || "未提供描述，可打开文件预览查看 SKILL.md。"}</p>
                <p className="skill-card-metadata"><span title={asset.sourceRef ?? undefined}>{SOURCE_LABELS[asset.sourceType] ?? "其他来源"}</span><span aria-hidden="true">·</span><span className={`skill-trust-label ${asset.trustState === "trusted" ? "ok" : asset.trustState === "blocked" ? "danger" : "warn"}`}>{asset.trustState === "trusted" ? "已信任" : asset.trustState === "blocked" ? "已阻止" : "待审阅"}</span></p>
                <div className="skill-card-deployments" aria-label={`${asset.name} 分发状态`}>{selectedRuntimes.length ? selectedRuntimes.map(runtime => {
                  const deployment = deploymentFor(asset.id, runtime.runtime);
                  const status = deployment ? STATUS[deployment.status] ?? STATUS.unverified : null;
                  return <span key={runtime.runtime} className={`skill-card-deployment ${status?.tone ?? "empty"}`} title={`${runtime.label}：${status?.label ?? "未部署"}${deployment?.lastError ? ` · ${deployment.lastError}` : ""}`}><span>{runtime.label}</span>{status?.label === "已同步" ? <Check size={13} aria-label="已同步" /> : <small>{status?.label ?? "未部署"}</small>}</span>;
                }) : <span className="skill-card-no-target">选择 Agent 查看分发状态</span>}</div>
              </div>
              <button type="button" className="skill-card-more icon-btn" aria-label={`${asset.name} 更多操作`} onClick={() => setRowActionsId(asset.id)}><DotsThree size={21} /></button>
            </article>;
          })}
        </div>
        {!snapshot.assets.length && <EmptyState className="skill-center-empty" icon={<Package size={28} />} title="还没有 Skills" description="导入目录、压缩包或 Git 仓库，也可以从更多菜单扫描已有 Skills。" descriptionAs="span" />}
        {!!snapshot.assets.length && !visibleAssets.length && <p className="skill-center-empty">没有匹配的 Skill，请调整搜索或状态筛选。</p>}
        </>
      )}

      {activeTab === "sets" && <div className="skill-sets-layout">
        <section className="skill-set-compose"><h3>保存当前选择为配置集</h3><p>配置集是 Skill、Runtime 和作用域的可复用组合；多个配置集可以同时生效。</p><input value={setName} onChange={(event) => setSetName(event.target.value)} placeholder="例如：前端开发、代码审查" /><select value={setAgentRole} onChange={(event) => setSetAgentRole(event.target.value)}><option value="">不绑定 Agent 角色</option><option value="orchestrator">协调者</option><option value="worker">执行者</option><option value="reviewer">审查者</option><option value="integrator">集成者</option></select><button className="btn primary" onClick={() => void saveSet()} disabled={busy || !selectedKeys.length}><Plus />保存配置集</button><small>当前选择 {selectedKeys.length} 个分发目标</small></section>
        <div className="skill-set-list">{snapshot.skillSets.map((set) => <article key={set.id}><div><strong>{set.name}</strong><p>{set.description || `${set.items.length} 个 Skill / Runtime 目标`}{set.items.some((item) => item.runtime === "oh_my_pi") ? " · 包含 Oh My Pi（已移除）" : ""}</p></div><span>{set.agentRole ? `角色：${set.agentRole}` : set.projectPath ? "项目" : "全局/混合"}</span><button className="btn" disabled={busy || set.items.some((item) => item.runtime === "oh_my_pi")} onClick={() => void applySet(set)}><DownloadSimple />应用</button><button className="icon-btn danger" onClick={() => void run(async () => { await invoke("skill_center_delete_set", { id: set.id }); })}><Trash /></button></article>)}</div>
      </div>}

      {activeTab === "preview" && <div className="skill-preview-layout">
        <aside className="skill-asset-list">{snapshot.assets.map((asset) => <button key={asset.id} className={asset.id === selectedAssetId ? "active" : ""} onClick={() => void showPreview(asset)}><strong>{asset.name}</strong><small>{asset.sourceType} · {asset.fileCount} 文件</small></button>)}</aside>
        <section className="skill-preview-main">{selectedAsset ? <><header><div><h3>{selectedAsset.name}</h3><p>{selectedAsset.description || "未提供描述"}</p></div><div className="skill-trust-actions"><button className={selectedAsset.trustState === "trusted" ? "active" : ""} onClick={() => void trust("trusted")}><ShieldCheck />信任</button><button className={selectedAsset.trustState === "blocked" ? "blocked" : ""} onClick={() => void trust("blocked")}><Warning />阻止</button><button className="icon-btn danger" onClick={() => { if (window.confirm("删除中央库中的 Skill？")) void run(async () => { await invoke("skill_center_delete_asset", { skillId: selectedAsset.id }); }); }}><Trash /></button></div></header><div className="skill-risk-strip"><span>来源：{selectedAsset.sourceRef ?? selectedAsset.centralPath}</span>{selectedAsset.sourceRevision && <span>版本：{selectedAsset.sourceRevision}</span>}<span>SHA-256：{selectedAsset.contentHash}</span><span>{selectedAsset.scriptCount} 个脚本</span></div>{selectedAsset.riskSummary.map((risk) => <div className="skill-risk" key={risk}><Warning />{risk}</div>)}<div className="skill-preview-columns"><div className="skill-preview-code"><strong>{previewFile}</strong><pre>{previewContent || preview?.skillMd || "选择 Skill 读取 SKILL.md"}</pre></div><aside>{preview?.files.map((file) => <button type="button" key={file.path} className={previewFile === file.path ? "active" : ""} onClick={() => void readPreviewFile(file.path)}><FileCode /><span>{file.path}</span><small>{Math.ceil(file.size / 1024)} KB</small></button>)}</aside></div></> : <div className="skill-center-empty">选择一个 Skill</div>}</section>
      </div>}

      </div>
      {activeTab === "matrix" && <footer className="skill-center-footer">
        <div className="skill-selection-summary"><strong>已选择 {selectedSkills.length} 个 Skill · {selectedRuntimes.length} 个 Agent</strong><small><ShieldCheck />冲突检查通过后才会写入目标。</small></div>
        <button className="btn" disabled={busy || !selectedKeys.length} onClick={() => { setMessage(null); setShowSaveSet(true); }}>保存配置集</button>
        <button className="btn primary" disabled={busy || !selectedKeys.length} onClick={() => void deploySelected()}>部署到 {selectedRuntimes.length === 1 ? targetLabel : `${selectedRuntimes.length} 个 Agent`}</button>
      </footer>}

      {showRuntimeDetails && createPortal(<Modal className="skill-center-modal skill-runtime-modal skill-runtime-details" ariaLabel="Skill 部署目标详情" onClose={() => { if (!busy) setShowRuntimeDetails(false); }}>
        <h3>部署目标详情</h3><div>{selectedRuntimes.map(runtime => <dl key={runtime.runtime}><div><dt>Agent</dt><dd>{runtime.label}</dd></div><div><dt>安装状态</dt><dd>{runtime.installed ? "已检测到 Runtime" : "未检测到 Runtime，请检查安装"}</dd></div><div><dt>全局目录</dt><dd>{runtime.globalRoot}</dd></div><div><dt>项目相对目录</dt><dd>{runtime.projectRoot}</dd></div><div><dt>发现目录</dt><dd>{runtime.discoveryRoots.join("、") || "使用默认目录"}</dd></div></dl>)}</div><footer><button className="btn" onClick={() => setShowRuntimeDetails(false)}>关闭</button></footer>
      </Modal>, document.body)}

      {showSaveSet && createPortal(<Modal className="skill-center-modal skill-runtime-modal" ariaLabel="保存 Skill 配置集" onClose={() => { if (!busy) setShowSaveSet(false); }}>
        <h3>保存配置集</h3><p>将选中的 {selectedSkills.length} 个 Skill 与 {selectedRuntimes.length} 个 Agent、当前作用域保存为可复用组合。</p>
        <label>配置集名称<input value={setName} disabled={busy} onChange={event => setSetName(event.target.value)} placeholder="例如：前端开发" /></label>
        {message?.tone === "error" && <p role="alert">{message.text}</p>}
        <footer><button className="btn" disabled={busy} onClick={() => setShowSaveSet(false)}>取消</button><button className="btn primary" disabled={busy || !setName.trim() || !selectedKeys.length} onClick={() => void saveSet()}>保存</button></footer>
      </Modal>, document.body)}

      {rowAsset && createPortal(<Modal className="skill-center-modal skill-runtime-modal" ariaLabel={`${rowAsset.name} 操作`} onClose={() => { if (!busy) setRowActionsId(null); }}>
        <h3>{rowAsset.name}</h3><p>{rowAsset.description || "未提供描述"}</p>
        <dl className="skill-row-facts"><div><dt>内容版本</dt><dd>{rowAsset.contentHash}</dd></div>{selectedRuntimes.map(runtime => { const deployment = deploymentFor(rowAsset.id, runtime.runtime); return deployment && <div key={runtime.runtime}><dt>{runtime.label}</dt><dd>{deployment.targetPath} · {deployment.mode}{deployment.lastError && <p role="alert">{deployment.lastError}</p>}</dd></div>; })}</dl>
        <button className="btn" disabled={busy} onClick={() => { setRowActionsId(null); void showPreview(rowAsset); }}><FileCode />文件预览与信任审阅</button>
        <button className="btn" disabled={busy} onClick={() => { setSelectedAssetId(rowAsset.id); setRowActionsId(null); setActiveTab("updates"); }}><ArrowClockwise />版本更新</button>
        {selectedRuntimes.map(runtime => { const deployment = deploymentFor(rowAsset.id, runtime.runtime); return deployment && <button key={runtime.runtime} className="btn danger" disabled={busy} onClick={() => { setRowActionsId(null); void removeDeployment(deployment); }}><Trash />撤下 {runtime.label} 部署</button>; })}
        <footer><button className="btn" disabled={busy} onClick={() => setRowActionsId(null)}>关闭</button></footer>
      </Modal>, document.body)}

      {showGitImport && createPortal(<Modal className="skill-center-modal skill-runtime-modal" ariaLabel="从 Git 导入 Skill" onClose={() => { if (!busy) setShowGitImport(false); }}>
        <h3>从 Git 导入 Skill</h3><fieldset disabled={busy} className="skill-modal-fields"><label>仓库地址<input value={gitUrl} onChange={(event) => setGitUrl(event.target.value)} placeholder="https://github.com/org/repo.git" /></label><div className="skill-import-grid"><label>分支<input value={gitBranch} onChange={(event) => setGitBranch(event.target.value)} placeholder="可选" /></label><label>子目录<input value={gitSubpath} onChange={(event) => setGitSubpath(event.target.value)} placeholder="skills/example" /></label></div><label>固定 Commit<input value={gitRevision} onChange={(event) => setGitRevision(event.target.value)} placeholder="可选，建议生产环境固定" /></label></fieldset>
        {message?.tone === "error" && <p role="alert">{message.text}</p>}
        <footer><button className="btn" disabled={busy} onClick={() => setShowGitImport(false)}>取消</button><button className="btn primary" onClick={() => void importGit()} disabled={busy}>导入中央库</button></footer>
      </Modal>, document.body)}

      {showDiscovery && createPortal(<Modal className="skill-center-modal skill-runtime-modal discovery" ariaLabel="Runtime 中发现的 Skill" onClose={() => { if (!busy) setShowDiscovery(false); }}>
        <h3>Runtime 中发现的 Skill</h3><p>扫描只读取目录；点击“纳管”后才复制进中央库，不会删除原目录。</p>
        {message?.tone === "error" && <p role="alert">{message.text}</p>}
        <div className="skill-discovery-list">{discovered.map((item) => <article key={`${item.runtime}:${item.path}`}><div><strong>{item.name}</strong><p>{item.runtime} · {item.path}</p></div>{item.alreadyManaged ? <span className="status-chip ok">已纳管</span> : <button className="btn" disabled={busy} onClick={() => void adopt(item)}>纳管</button>}</article>)}</div><footer><span>{discovered.length} 个发现结果</span><button className="btn primary" disabled={busy} onClick={() => setShowDiscovery(false)}>完成</button></footer>
      </Modal>, document.body)}
      </fieldset>
    </section>
  );
}
