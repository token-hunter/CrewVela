import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { EmptyState } from "./ui/EmptyState";
import {
  ArrowClockwise,
  Check,
  DownloadSimple,
  FileCode,
  FolderOpen,
  GitBranch,
  Package,
  Plus,
  ShieldCheck,
  Trash,
  Warning,
} from "@phosphor-icons/react";
import { SkillMarketPanel } from "./SkillMarketPanel";
import { SkillUpdatesPanel } from "./SkillUpdatesPanel";
import type { Project } from "../types";

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

const STATUS: Record<string, { label: string; tone: string }> = {
  synced: { label: "已同步", tone: "ok" },
  center_newer: { label: "中央较新", tone: "info" },
  runtime_newer: { label: "本地修改", tone: "warn" },
  conflict: { label: "冲突", tone: "danger" },
  failed: { label: "失败", tone: "danger" },
  unverified: { label: "未验证", tone: "muted" },
};

const cellKey = (skillId: string, runtime: string) => `${skillId}::${runtime}`;
const shortHash = (value: string) => value.slice(0, 10);
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
  const [selectedCells, setSelectedCells] = useState<Set<string>>(new Set());
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
  const scopedDeployments = useMemo(() => (snapshot?.deployments ?? []).filter((item) =>
    item.scope === scope && (scope === "global" || item.projectPath === effectiveProject)
    && (scope !== "task" || ((!taskId || item.taskId === taskId) && (!agentId || item.agentId === agentId))),
  ), [snapshot, scope, effectiveProject, taskId, agentId]);

  const deploymentFor = (skillId: string, runtime: string) => scopedDeployments.find((item) => item.skillId === skillId && item.runtime === runtime);

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

  const toggleCell = (key: string) => setSelectedCells((current) => {
    const next = new Set(current); next.has(key) ? next.delete(key) : next.add(key); return next;
  });

  const deploySelected = async () => {
    if (!selectedCells.size) return;
    if (scope === "project" && !projectPath) { setMessage({ tone: "error", text: "请选择项目" }); return; }
    if (scope === "task" && !taskWorkspacePath.trim()) { setMessage({ tone: "error", text: "请选择任务或 Agent 的独立工作目录 / Worktree" }); return; }
    if (scope === "task" && !taskId.trim() && !agentId.trim()) { setMessage({ tone: "error", text: "任务作用域至少填写任务 ID 或 Agent ID" }); return; }
    const inputs = [...selectedCells].map((key) => {
      const [skillId, runtime] = key.split("::");
      return { skillId, runtime, scope, projectPath: effectiveProject, taskId: scope === "task" ? taskId.trim() || null : null, agentId: scope === "task" ? agentId.trim() || null : null, trustConfirmed: true };
    });
    const untrusted = inputs
      .map((input) => snapshot?.assets.find((asset) => asset.id === input.skillId))
      .filter((asset): asset is SkillAsset => Boolean(asset && asset.trustState !== "trusted"));
    if (untrusted.length && !window.confirm(`以下 Skill 尚未信任：\n${untrusted.map((asset) => `• ${asset.name}${asset.riskSummary.length ? `（${asset.riskSummary.join("；")}）` : ""}`).join("\n")}\n\n确认审阅来源并信任后部署？`)) return;
    await run(async () => { await invoke("skill_center_deploy_batch", { inputs }); setSelectedCells(new Set()); }, `已完成 ${inputs.length} 个 Runtime 目标的安全分发`);
  };

  const undeploySelected = async () => {
    const deployments = [...selectedCells]
      .map((key) => { const [skillId, runtime] = key.split("::"); return deploymentFor(skillId, runtime); })
      .filter((item): item is Deployment => Boolean(item));
    if (!deployments.length) return;
    if (!window.confirm(`撤下选中的 ${deployments.length} 个受管部署？任何存在本地修改的副本都会使整批操作取消。`)) return;
    await run(async () => {
      await invoke("skill_center_undeploy_batch", { deploymentIds: deployments.map((item) => item.id) });
      setSelectedCells(new Set());
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
    if (!setName.trim() || !selectedCells.size) { setMessage({ tone: "error", text: "填写配置集名称并选择至少一个分发单元格" }); return; }
    const items = [...selectedCells].map((key) => {
      const [skillId, runtime] = key.split("::");
      return { skillId, runtime, scope, projectPath: effectiveProject, taskId: scope === "task" ? taskId.trim() || null : null, agentId: scope === "task" ? agentId.trim() || null : null };
    });
    await run(async () => { await invoke("skill_center_save_set", { input: { name: setName.trim(), items, projectPath: effectiveProject, agentRole: setAgentRole || null } }); setSetName(""); }, "Skill 配置集已保存");
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
    <section className="skill-center skill-workspace" aria-label="Skill 中心">
      <fieldset disabled={busy} style={{ display: "contents" }}>
      <header className="skill-center-toolbar">
        <div className="skill-center-tabs" role="tablist" aria-label="Skill 管理视图" onKeyDown={(event) => {
          const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
          const index = tabs.indexOf(event.target as HTMLButtonElement);
          if (index < 0 || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
          tabs[next].focus(); tabs[next].click();
        }}>
          {([ ["matrix", "分发矩阵"], ["market", "Skill 市场"], ["updates", "版本更新"], ["sets", "配置集"], ["preview", "文件预览"] ] as const).map(([id, label]) =>
            <button key={id} type="button" role="tab" aria-selected={activeTab === id} tabIndex={activeTab === id ? 0 : -1} className={activeTab === id ? "active" : ""} onClick={() => setActiveTab(id)}>{label}{id === "sets" && <span>{snapshot.skillSets.length}</span>}</button>
          )}
        </div>
        {activeTab !== "market" && <div className="skill-center-actions">
          <button className="btn" onClick={() => void scan()} disabled={busy}><ArrowClockwise />扫描现有 Skill</button>
          <button className="btn" onClick={() => void importPaths("local")} disabled={busy}><FolderOpen />导入目录</button>
          <button className="btn" onClick={() => void importPaths("archive")} disabled={busy}><Package />导入包</button>
          <button className="btn primary" onClick={() => setShowGitImport(true)} disabled={busy}><GitBranch />Git 导入</button>
        </div>}
      </header>

      {message && <div role={message.tone === "error" ? "alert" : "status"} className={`skill-center-message ${message.tone}`}>{message.text}</div>}

      <div className={`skill-center-scopebar${activeTab === "market" || activeTab === "updates" ? " hidden" : ""}`}>
        <label>作用域<select value={scope} onChange={(event) => setScope(event.target.value as typeof scope)}><option value="global">全局</option><option value="project">项目</option><option value="task">任务 / Agent</option></select></label>
        {scope === "project" && <label>项目<select value={projectPath} onChange={(event) => setProjectPath(event.target.value)}><option value="">选择项目</option>{projects.map((project) => <option key={project.id} value={project.path}>{project.name}</option>)}</select></label>}
        {scope === "task" && <><label>独立工作目录<input value={taskWorkspacePath} onChange={(event) => setTaskWorkspacePath(event.target.value)} placeholder="任务 Worktree 绝对路径" /></label><button className="btn" type="button" onClick={() => void run(async () => { const path = await open({ directory: true, title: "选择任务 Worktree" }); if (typeof path === "string") setTaskWorkspacePath(path); })}>选择目录</button><label>任务 ID<input value={taskId} onChange={(event) => setTaskId(event.target.value)} placeholder="可选" /></label><label>Agent ID<input value={agentId} onChange={(event) => setAgentId(event.target.value)} placeholder="可选" /></label></>}
        <div className="skill-center-callout"><ShieldCheck />批量操作先预检全部目标，冲突时不会写入任何 Runtime。</div>
      </div>

      {activeTab === "market" && <SkillMarketPanel onInstalled={load} />}
      {activeTab === "updates" && <SkillUpdatesPanel assets={snapshot.assets} deployments={snapshot.deployments} initialSkillId={selectedAssetId} onChanged={load} />}

      {activeTab === "matrix" && (
        <div className="skill-center-matrix-wrap">
          <table className="skill-center-matrix" style={{ minWidth: 220 + snapshot.runtimes.length * 144 }}>
            <thead><tr><th>中央 Skill</th>{snapshot.runtimes.map((runtime) => <th key={runtime.runtime}><strong>{runtime.label} <i className={runtime.installed ? "runtime-online" : "runtime-offline"}>{runtime.installed ? "已安装" : "未检测"}</i></strong><small>{runtime.projectRoot}</small></th>)}</tr></thead>
            <tbody>{snapshot.assets.map((asset) => (
              <tr key={asset.id} className={asset.id === selectedAssetId ? "selected" : ""}>
                <th scope="row">
                  <button className="skill-asset-cell" title={asset.name} onClick={() => void showPreview(asset)}>
                    <span>{asset.name}</span>
                    <small>{asset.trustState === "trusted" ? "已信任" : asset.trustState === "blocked" ? "已阻止" : "待审阅"} · {shortHash(asset.contentHash)}</small>
                  </button>
                  <button className="skill-asset-update" aria-label={`检查 ${asset.name} 版本与更新`} onClick={() => { setSelectedAssetId(asset.id); setActiveTab("updates"); }}>
                    <ArrowClockwise size={13} aria-hidden="true" />版本更新
                  </button>
                </th>
                {snapshot.runtimes.map((runtime) => {
                  const deployment = deploymentFor(asset.id, runtime.runtime);
                  const key = cellKey(asset.id, runtime.runtime);
                  const status = deployment ? STATUS[deployment.status] ?? STATUS.unverified : null;
                  return <td key={runtime.runtime}><button className={`skill-matrix-cell${selectedCells.has(key) ? " checked" : ""}`} aria-label={`${asset.name} · ${runtime.label} · ${status?.label ?? "未部署"}`} aria-pressed={selectedCells.has(key)} onClick={() => toggleCell(key)} title={deployment?.targetPath ?? "选择后批量部署"}>
                    <i>{selectedCells.has(key) ? <Check weight="bold" /> : null}</i>
                    <span className={`skill-deploy-status ${status?.tone ?? "empty"}`}>{status?.label ?? "未部署"}</span>
                    {deployment && <small>{deployment.mode}</small>}
                  </button>{deployment && <button className="skill-cell-remove" onClick={() => void removeDeployment(deployment)} aria-label="撤下"><Trash /></button>}</td>;
                })}
              </tr>
            ))}</tbody>
          </table>
          {!snapshot.assets.length && <EmptyState className="skill-center-empty" icon={<Package size={28} />} title="中央库还没有 Skill" description="导入目录、压缩包、Git 仓库，或扫描八个 Runtime 的现有目录。" descriptionAs="span" />}
        </div>
      )}

      {activeTab === "sets" && <div className="skill-sets-layout">
        <section className="skill-set-compose"><h3>保存当前选择为配置集</h3><p>配置集是 Skill、Runtime 和作用域的可复用组合；多个配置集可以同时生效。</p><input value={setName} onChange={(event) => setSetName(event.target.value)} placeholder="例如：前端开发、代码审查" /><select value={setAgentRole} onChange={(event) => setSetAgentRole(event.target.value)}><option value="">不绑定 Agent 角色</option><option value="orchestrator">协调者</option><option value="worker">执行者</option><option value="reviewer">审查者</option><option value="integrator">集成者</option></select><button className="btn primary" onClick={() => void saveSet()} disabled={busy || !selectedCells.size}><Plus />保存配置集</button><small>当前选择 {selectedCells.size} 个分发目标</small></section>
        <div className="skill-set-list">{snapshot.skillSets.map((set) => <article key={set.id}><div><strong>{set.name}</strong><p>{set.description || `${set.items.length} 个 Skill / Runtime 目标`}{set.items.some((item) => item.runtime === "oh_my_pi") ? " · 包含 Oh My Pi（已移除）" : ""}</p></div><span>{set.agentRole ? `角色：${set.agentRole}` : set.projectPath ? "项目" : "全局/混合"}</span><button className="btn" disabled={busy || set.items.some((item) => item.runtime === "oh_my_pi")} onClick={() => void applySet(set)}><DownloadSimple />应用</button><button className="icon-btn danger" onClick={() => void run(async () => { await invoke("skill_center_delete_set", { id: set.id }); })}><Trash /></button></article>)}</div>
      </div>}

      {activeTab === "preview" && <div className="skill-preview-layout">
        <aside className="skill-asset-list">{snapshot.assets.map((asset) => <button key={asset.id} className={asset.id === selectedAssetId ? "active" : ""} onClick={() => void showPreview(asset)}><strong>{asset.name}</strong><small>{asset.sourceType} · {asset.fileCount} 文件</small></button>)}</aside>
        <section className="skill-preview-main">{selectedAsset ? <><header><div><h3>{selectedAsset.name}</h3><p>{selectedAsset.description || "未提供描述"}</p></div><div className="skill-trust-actions"><button className={selectedAsset.trustState === "trusted" ? "active" : ""} onClick={() => void trust("trusted")}><ShieldCheck />信任</button><button className={selectedAsset.trustState === "blocked" ? "blocked" : ""} onClick={() => void trust("blocked")}><Warning />阻止</button><button className="icon-btn danger" onClick={() => { if (window.confirm("删除中央库中的 Skill？")) void run(async () => { await invoke("skill_center_delete_asset", { skillId: selectedAsset.id }); }); }}><Trash /></button></div></header><div className="skill-risk-strip"><span>来源：{selectedAsset.sourceRef ?? selectedAsset.centralPath}</span>{selectedAsset.sourceRevision && <span>版本：{selectedAsset.sourceRevision}</span>}<span>SHA-256：{selectedAsset.contentHash}</span><span>{selectedAsset.scriptCount} 个脚本</span></div>{selectedAsset.riskSummary.map((risk) => <div className="skill-risk" key={risk}><Warning />{risk}</div>)}<div className="skill-preview-columns"><div className="skill-preview-code"><strong>{previewFile}</strong><pre>{previewContent || preview?.skillMd || "选择 Skill 读取 SKILL.md"}</pre></div><aside>{preview?.files.map((file) => <button type="button" key={file.path} className={previewFile === file.path ? "active" : ""} onClick={() => void readPreviewFile(file.path)}><FileCode /><span>{file.path}</span><small>{Math.ceil(file.size / 1024)} KB</small></button>)}</aside></div></> : <div className="skill-center-empty">选择一个 Skill</div>}</section>
      </div>}

      {activeTab === "matrix" && <footer className="skill-center-footer"><div><input value={setName} onChange={(event) => setSetName(event.target.value)} placeholder="配置集名称（可选）" /><button className="btn" disabled={!selectedCells.size || !setName.trim()} onClick={() => void saveSet()}>保存配置集</button></div><span>已选择 {selectedCells.size} 个目标</span><button className="btn danger" disabled={busy || ![...selectedCells].some((key) => { const [skillId, runtime] = key.split("::"); return Boolean(deploymentFor(skillId, runtime)); })} onClick={() => void undeploySelected()}><Trash />撤下选中</button><button className="btn primary" disabled={busy || !selectedCells.size} onClick={() => void deploySelected()}><DownloadSimple />部署选中</button></footer>}

      {showGitImport && <div className="skill-center-popover"><div className="skill-center-modal"><h3>从 Git 导入 Skill</h3><label>仓库地址<input value={gitUrl} onChange={(event) => setGitUrl(event.target.value)} placeholder="https://github.com/org/repo.git" /></label><div className="skill-import-grid"><label>分支<input value={gitBranch} onChange={(event) => setGitBranch(event.target.value)} placeholder="可选" /></label><label>子目录<input value={gitSubpath} onChange={(event) => setGitSubpath(event.target.value)} placeholder="skills/example" /></label></div><label>固定 Commit<input value={gitRevision} onChange={(event) => setGitRevision(event.target.value)} placeholder="可选，建议生产环境固定" /></label><footer><button className="btn" onClick={() => setShowGitImport(false)}>取消</button><button className="btn primary" onClick={() => void importGit()} disabled={busy}>导入中央库</button></footer></div></div>}

      {showDiscovery && <div className="skill-center-popover"><div className="skill-center-modal discovery"><h3>Runtime 中发现的 Skill</h3><p>扫描只读取目录；点击“纳管”后才复制进中央库，不会删除原目录。</p><div className="skill-discovery-list">{discovered.map((item) => <article key={`${item.runtime}:${item.path}`}><div><strong>{item.name}</strong><p>{item.runtime} · {item.path}</p></div>{item.alreadyManaged ? <span className="status-chip ok">已纳管</span> : <button className="btn" onClick={() => void adopt(item)}>纳管</button>}</article>)}</div><footer><span>{discovered.length} 个发现结果</span><button className="btn primary" onClick={() => setShowDiscovery(false)}>完成</button></footer></div></div>}
      </fieldset>
    </section>
  );
}
