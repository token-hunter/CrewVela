import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { EmptyState } from "./ui/EmptyState";
import { Modal } from "./ui/Modal";
import {
  ArrowClockwise,
  ArrowSquareOut,
  Check,
  DownloadSimple,
  GearSix,
  MagnifyingGlass,
  Plus,
  Trash,
  Warning,
  X,
} from "@phosphor-icons/react";

interface SkillMarketRepository {
  id: string;
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
  builtin: boolean;
  updatedAt: number;
}

interface SkillMarketItem {
  key: string;
  name: string;
  description: string;
  sourceType: "market" | "skills_sh";
  sourceUrl: string;
  repositoryId: string;
  repositoryLabel: string;
  branch: string;
  subpath: string;
  readmeUrl: string;
  installs: number | null;
  contentHash: string | null;
  installedAssetId: string | null;
}

interface BrowseResult {
  skills: SkillMarketItem[];
  errors: string[];
}

interface SearchResult {
  skills: SkillMarketItem[];
  totalCount: number;
  query: string;
  hasMore: boolean;
}

interface Props {
  onInstalled: () => Promise<void>;
}

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
const PAGE_SIZE = 20;
const PREVIEW_REPOSITORIES: SkillMarketRepository[] = [
  { id: "anthropics", owner: "anthropics", name: "skills", branch: "main", enabled: true, builtin: true, updatedAt: 0 },
  { id: "composio", owner: "ComposioHQ", name: "awesome-claude-skills", branch: "master", enabled: true, builtin: true, updatedAt: 0 },
  { id: "baoyu", owner: "JimLiu", name: "baoyu-skills", branch: "main", enabled: true, builtin: true, updatedAt: 0 },
];
const PREVIEW_MARKET_SKILLS: SkillMarketItem[] = [
  { key: "risk", name: "21risk-automation", description: "Automate risk tasks and validate current tool schemas.", sourceType: "market", sourceUrl: "https://github.com/ComposioHQ/awesome-claude-skills.git", repositoryId: "composio", repositoryLabel: "ComposioHQ/awesome-claude-skills", branch: "master", subpath: "skills/21risk-automation", readmeUrl: "https://github.com/ComposioHQ/awesome-claude-skills", installs: null, contentHash: "preview-risk", installedAssetId: null },
  { key: "chat", name: "2chat-automation", description: "Automate chat tasks with reusable MCP workflows.", sourceType: "market", sourceUrl: "https://github.com/ComposioHQ/awesome-claude-skills.git", repositoryId: "composio", repositoryLabel: "ComposioHQ/awesome-claude-skills", branch: "master", subpath: "skills/2chat-automation", readmeUrl: "https://github.com/ComposioHQ/awesome-claude-skills", installs: null, contentHash: "preview-chat", installedAssetId: null },
  { key: "frontend", name: "frontend-design", description: "Create distinctive production-grade frontend interfaces.", sourceType: "market", sourceUrl: "https://github.com/anthropics/skills.git", repositoryId: "anthropics", repositoryLabel: "anthropics/skills", branch: "main", subpath: "skills/frontend-design", readmeUrl: "https://github.com/anthropics/skills", installs: null, contentHash: "preview-frontend", installedAssetId: "frontend" },
  { key: "docs", name: "documents", description: "Create and verify document artifacts with visual QA.", sourceType: "market", sourceUrl: "https://github.com/anthropics/skills.git", repositoryId: "anthropics", repositoryLabel: "anthropics/skills", branch: "main", subpath: "skills/documents", readmeUrl: "https://github.com/anthropics/skills", installs: null, contentHash: "preview-docs", installedAssetId: null },
];
const PREVIEW_SKILLS_SH: SkillMarketItem[] = [
  { ...PREVIEW_MARKET_SKILLS[2], key: "design-taste", name: "design-taste-frontend", sourceType: "skills_sh", repositoryLabel: "leonxlnx/taste-skill", sourceUrl: "https://github.com/leonxlnx/taste-skill.git", subpath: "design-taste-frontend", installs: 386134, installedAssetId: null },
  { ...PREVIEW_MARKET_SKILLS[2], key: "stitch-taste", name: "stitch-design-taste", sourceType: "skills_sh", repositoryLabel: "leonxlnx/taste-skill", sourceUrl: "https://github.com/leonxlnx/taste-skill.git", subpath: "stitch-design-taste", installs: 244064, installedAssetId: null },
  { ...PREVIEW_MARKET_SKILLS[2], key: "gpt-taste", name: "gpt-taste", sourceType: "skills_sh", repositoryLabel: "leonxlnx/taste-skill", sourceUrl: "https://github.com/leonxlnx/taste-skill.git", subpath: "gpt-taste", installs: 243020, installedAssetId: null },
  { ...PREVIEW_MARKET_SKILLS[2], key: "visual", name: "high-end-visual-design", sourceType: "skills_sh", repositoryLabel: "leonxlnx/taste-skill", sourceUrl: "https://github.com/leonxlnx/taste-skill.git", subpath: "high-end-visual-design", installs: 288822, installedAssetId: null },
];

const installsLabel = (value: number | null) =>
  value === null ? null : new Intl.NumberFormat("zh-CN", { notation: "compact", maximumFractionDigits: 1 }).format(value);

export function SkillMarketPanel({ onInstalled }: Props) {
  const [source, setSource] = useState<"repos" | "skills_sh">("repos");
  const [repositories, setRepositories] = useState<SkillMarketRepository[]>([]);
  const [browseResult, setBrowseResult] = useState<BrowseResult>({ skills: [], errors: [] });
  const [skillsShItems, setSkillsShItems] = useState<SkillMarketItem[]>([]);
  const [skillsShHasMore, setSkillsShHasMore] = useState(false);
  const [skillsShInput, setSkillsShInput] = useState("");
  const [skillsShQuery, setSkillsShQuery] = useState("");
  const [skillsShOffset, setSkillsShOffset] = useState(0);
  const [repoSearch, setRepoSearch] = useState("");
  const [repoFilter, setRepoFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<"all" | "installed" | "uninstalled">("all");
  const [managerOpen, setManagerOpen] = useState(false);
  const [draftOwner, setDraftOwner] = useState("");
  const [draftName, setDraftName] = useState("");
  const [draftBranch, setDraftBranch] = useState("main");
  const [busy, setBusy] = useState(false);
  const operationInFlight = useRef(false);
  const [browsing, setBrowsing] = useState(false);
  const [searching, setSearching] = useState(false);
  const loading = browsing || searching;
  const browseSequence = useRef(0);
  const repositorySequence = useRef(0);
  const searchSequence = useRef(0);
  const activeSearch = useRef<{ query: string; offset: number } | null>(null);
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(null);

  const loadRepositories = useCallback(async () => {
    const sequence = ++repositorySequence.current;
    const items = UI_PREVIEW
      ? PREVIEW_REPOSITORIES
      : await invoke<SkillMarketRepository[]>("skill_center_market_repositories");
    if (sequence === repositorySequence.current) setRepositories(items);
  }, []);

  const browse = useCallback(async () => {
    const sequence = ++browseSequence.current;
    setBrowsing(true);
    setMessage(null);
    try {
      const result = UI_PREVIEW
        ? { skills: PREVIEW_MARKET_SKILLS, errors: [] }
        : await invoke<BrowseResult>("skill_center_market_browse");
      if (sequence === browseSequence.current) setBrowseResult(result);
      return true;
    } catch (error) {
      if (sequence === browseSequence.current) setMessage({ tone: "error", text: String(error) });
      return false;
    } finally {
      if (sequence === browseSequence.current) setBrowsing(false);
    }
  }, []);

  useEffect(() => {
    void Promise.all([loadRepositories(), browse()]).catch((error) =>
      setMessage({ tone: "error", text: String(error) }),
    );
  }, [browse, loadRepositories]);

  const filteredRepositorySkills = useMemo(() => {
    const query = repoSearch.trim().toLocaleLowerCase();
    return browseResult.skills.filter((skill) => {
      const matchesQuery = !query
        || skill.name.toLocaleLowerCase().includes(query)
        || skill.description.toLocaleLowerCase().includes(query)
        || skill.repositoryLabel.toLocaleLowerCase().includes(query);
      const matchesRepo = repoFilter === "all" || skill.repositoryId === repoFilter;
      const installed = Boolean(skill.installedAssetId);
      const matchesStatus = statusFilter === "all"
        || (statusFilter === "installed" ? installed : !installed);
      return matchesQuery && matchesRepo && matchesStatus;
    });
  }, [browseResult.skills, repoFilter, repoSearch, statusFilter]);

  const runSearch = async (offset = 0) => {
    if (operationInFlight.current) return;
    const query = offset > 0 ? skillsShQuery : skillsShInput.trim();
    if (query.length < 2) {
      setMessage({ tone: "error", text: "skills.sh 搜索至少输入 2 个字符" });
      return;
    }
    if (activeSearch.current?.query === query && activeSearch.current.offset === offset) return;
    const sequence = ++searchSequence.current;
    activeSearch.current = { query, offset };
    setSearching(true);
    setMessage(null);
    try {
      const result = UI_PREVIEW
        ? { skills: PREVIEW_SKILLS_SH, totalCount: PREVIEW_SKILLS_SH.length, query, hasMore: false }
        : await invoke<SearchResult>("skill_center_skills_sh_search", {
            query,
            limit: PAGE_SIZE,
            offset,
          });
      if (sequence !== searchSequence.current) return;
      setSkillsShQuery(query);
      setSkillsShHasMore(result.hasMore);
      setSkillsShOffset(offset + result.skills.length);
      setSkillsShItems((current) => offset === 0 ? result.skills : [...current, ...result.skills.filter(item => !current.some(old => old.key === item.key))]);
    } catch (error) {
      if (sequence === searchSequence.current) setMessage({ tone: "error", text: String(error) });
    } finally {
      if (sequence === searchSequence.current) { activeSearch.current = null; setSearching(false); }
    }
  };

  const install = async (skill: SkillMarketItem) => {
    if (skill.installedAssetId || operationInFlight.current) return;
    operationInFlight.current = true;
    searchSequence.current++; activeSearch.current = null; setSearching(false);
    browseSequence.current++; setBrowsing(false);
    setBusy(true);
    setMessage(null);
    try {
      const asset = UI_PREVIEW
        ? { id: `preview-${skill.key}` }
        : await invoke<{ id: string }>("skill_center_market_install", {
            input: {
              sourceType: skill.sourceType,
              sourceUrl: skill.sourceUrl,
              branch: skill.branch,
              subpath: skill.subpath,
              name: skill.name,
              key: skill.key,
            },
          });
      const markInstalled = (items: SkillMarketItem[]) =>
        items.map((item) => item.key === skill.key ? { ...item, installedAssetId: asset.id } : item);
      setBrowseResult((current) => ({ ...current, skills: markInstalled(current.skills) }));
      setSkillsShItems(markInstalled);
      await onInstalled();
      setMessage({ tone: "success", text: `${skill.name} 已进入中央库，可在分发矩阵中部署到 Runtime` });
    } catch (error) {
      setMessage({ tone: "error", text: String(error) });
    } finally {
      operationInFlight.current = false;
      setBusy(false);
    }
  };

  const resetRepositoryDraft = () => {
    setDraftOwner("");
    setDraftName("");
    setDraftBranch("main");
  };

  const closeManager = () => {
    resetRepositoryDraft();
    setManagerOpen(false);
    setMessage(null);
  };

  const saveRepository = async () => {
    if (operationInFlight.current) return;
    if (!draftOwner.trim() || !draftName.trim()) {
      setMessage({ tone: "error", text: "请填写 GitHub Owner 和仓库名称" });
      return;
    }
    operationInFlight.current = true; setBusy(true);
    try {
      if (!UI_PREVIEW) {
        await invoke("skill_center_market_repository_save", {
          input: {
            owner: draftOwner.trim(),
            name: draftName.trim(),
            branch: draftBranch.trim() || "main",
            enabled: true,
          },
        });
      }
      resetRepositoryDraft();
      await loadRepositories();
      const refreshed = await browse();
      if (refreshed) setMessage({ tone: "success", text: "Skill 仓库已保存" });
    } catch (error) {
      setMessage({ tone: "error", text: String(error) });
    } finally {
      operationInFlight.current = false;
      setBusy(false);
    }
  };

  const toggleRepository = async (repository: SkillMarketRepository) => {
    if (operationInFlight.current) return;
    if (UI_PREVIEW) {
      setRepositories((current) => current.map((item) =>
        item.id === repository.id ? { ...item, enabled: !item.enabled } : item,
      ));
      return;
    }
    operationInFlight.current = true; setBusy(true);
    try {
      await invoke("skill_center_market_repository_save", {
        input: { ...repository, enabled: !repository.enabled },
      });
      await loadRepositories();
      await browse();
    } catch (error) {
      setMessage({ tone: "error", text: String(error) });
    } finally {
      operationInFlight.current = false;
      setBusy(false);
    }
  };

  const deleteRepository = async (repository: SkillMarketRepository) => {
    if (operationInFlight.current) return;
    if (repository.builtin || !window.confirm(`删除仓库 ${repository.owner}/${repository.name}？`)) return;
    operationInFlight.current = true; setBusy(true);
    try {
      if (!UI_PREVIEW) await invoke("skill_center_market_repository_delete", { id: repository.id });
      await loadRepositories();
      await browse();
    } catch (error) {
      setMessage({ tone: "error", text: String(error) });
    } finally {
      operationInFlight.current = false;
      setBusy(false);
    }
  };

  const openExternal = (url: string) => {
    if (UI_PREVIEW) window.open(url, "_blank", "noopener,noreferrer");
    else void openUrl(url).catch((error) => setMessage({ tone: "error", text: String(error) }));
  };

  const visibleSkills = source === "repos" ? filteredRepositorySkills : skillsShItems;
  const feedback = message && (
    <div className={`skill-market-message ${message.tone}`} role={message.tone === "error" ? "alert" : "status"}>
      {message.tone === "error" ? <Warning /> : <Check />}{message.text}
    </div>
  );

  return (
    <section className="skill-market" aria-label="Skill 市场">
      <header className="skill-market-toolbar">
        <div className="skill-market-source-tabs" role="tablist" aria-label="Skill 来源">
          <button className={source === "repos" ? "active" : ""} onClick={() => setSource("repos")}>仓库</button>
          <button className={source === "skills_sh" ? "active" : ""} onClick={() => setSource("skills_sh")}>skills.sh</button>
        </div>
        {source === "repos" ? (
          <>
            <label className="skill-market-search"><MagnifyingGlass /><input value={repoSearch} onChange={(event) => setRepoSearch(event.target.value)} placeholder="搜索技能名称或仓库名称…" /></label>
            <select value={repoFilter} onChange={(event) => setRepoFilter(event.target.value)} aria-label="筛选 Skill 仓库">
              <option value="all">全部仓库</option>
              {repositories.filter((item) => item.enabled).map((item) => <option key={item.id} value={item.id}>{item.owner}/{item.name}</option>)}
            </select>
            <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as typeof statusFilter)} aria-label="筛选安装状态">
              <option value="all">全部</option>
              <option value="installed">已安装</option>
              <option value="uninstalled">未安装</option>
            </select>
          </>
        ) : (
          <>
            <label className="skill-market-search"><MagnifyingGlass /><input value={skillsShInput} onChange={(event) => setSkillsShInput(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void runSearch(0); }} placeholder="搜索 skills.sh（至少 2 个字符）…" /></label>
            <button className="btn primary" onClick={() => void runSearch(0)} disabled={busy || loading || skillsShInput.trim().length < 2}><MagnifyingGlass />搜索技能</button>
          </>
        )}
        <div className="skill-market-toolbar-actions">
          <button className="icon-btn" title="刷新" aria-label="刷新 Skill 市场" onClick={() => void (source === "repos" ? browse() : runSearch(0))} disabled={busy || loading}><ArrowClockwise className={loading ? "spin" : ""} /></button>
          <button className="btn" onClick={() => { setMessage(null); setManagerOpen(true); }}><GearSix />仓库管理</button>
        </div>
      </header>

      {!managerOpen && feedback}
      {!!browseResult.errors.length && source === "repos" && <div className="skill-market-partial"><Warning />部分仓库读取失败：{browseResult.errors.join("；")}</div>}

      <div className="skill-market-grid">
        {visibleSkills.map((skill) => {
          const installed = Boolean(skill.installedAssetId);
          return (
            <article className="skill-market-card" key={skill.key}>
              <div className="skill-market-card-body">
                <h3>{skill.name}</h3>
                <div className="skill-market-meta">
                  <span>{skill.repositoryLabel}</span>
                  {installsLabel(skill.installs) && <span><DownloadSimple />{installsLabel(skill.installs)}</span>}
                </div>
                {skill.description && <p>{skill.description}</p>}
              </div>
              <footer>
                <button type="button" onClick={() => openExternal(skill.readmeUrl)}><ArrowSquareOut />查看</button>
                <button type="button" className={installed ? "installed" : "install"} onClick={() => void install(skill)} disabled={busy || installed}>
                  {installed ? <Check /> : <DownloadSimple />}{installed ? "已安装" : "安装"}
                </button>
              </footer>
            </article>
          );
        })}
        {!loading && visibleSkills.length === 0 && (
          <EmptyState
            className="skill-market-empty"
            icon={<MagnifyingGlass size={28} />}
            title={source === "skills_sh" && !skillsShQuery ? "搜索 skills.sh 中的社区 Skill" : "没有匹配的 Skill"}
            description={source === "repos" ? "调整搜索或仓库过滤条件。" : "输入至少 2 个字符开始搜索。"}
            descriptionAs="span"
          />
        )}
      </div>

      {source === "skills_sh" && skillsShHasMore && (
        <footer className="skill-market-load-more">
          <button className="btn" onClick={() => void runSearch(skillsShOffset)} disabled={loading || busy}>加载更多</button>
          <span>已显示 {skillsShItems.length} 个结果</span>
        </footer>
      )}

      {managerOpen && createPortal(
        <Modal className="skill-center-modal skill-market-manager" overlayClassName="skill-center-popover" ariaLabel="Skill 仓库管理" onClose={closeManager}>
          <header>
            <div><h3>Skill 仓库管理</h3><p>添加、启用、停用或删除仓库后会立即保存。</p></div>
            <button type="button" className="icon-btn" aria-label="关闭仓库管理" onClick={closeManager}><X size={16} /></button>
          </header>
          <div className="skill-market-repository-list">
            {repositories.map((repository) => (
              <article key={repository.id}>
                <div><strong title={`${repository.owner}/${repository.name}`}>{repository.owner}/{repository.name}</strong><small>{repository.branch}{repository.builtin ? " · 内置" : " · 自定义"}</small></div>
                <label><input type="checkbox" checked={repository.enabled} disabled={busy} onChange={() => void toggleRepository(repository)} />启用</label>
                <button type="button" className="icon-btn danger" disabled={busy || repository.builtin} title={repository.builtin ? "内置仓库可停用但不能删除" : "删除仓库"} aria-label={`删除仓库 ${repository.owner}/${repository.name}`} onClick={() => void deleteRepository(repository)}><Trash size={15} /></button>
              </article>
            ))}
          </div>
          <form className="skill-market-repository-form" onSubmit={(event) => { event.preventDefault(); void saveRepository(); }}>
            <label>GitHub 所有者<input value={draftOwner} disabled={busy} onChange={(event) => setDraftOwner(event.target.value)} placeholder="anthropics" /></label>
            <label>仓库名称<input value={draftName} disabled={busy} onChange={(event) => setDraftName(event.target.value)} placeholder="skills" /></label>
            <label>分支<input value={draftBranch} disabled={busy} onChange={(event) => setDraftBranch(event.target.value)} placeholder="main" /></label>
            <button type="submit" className="btn" disabled={busy}><Plus size={15} />添加仓库</button>
          </form>
          {feedback}
          <footer>
            <span>{repositories.filter((item) => item.enabled).length} 个来源已启用</span>
            <button type="button" className="btn" onClick={closeManager}>取消</button>
            <button type="button" className="btn primary" onClick={closeManager}>完成</button>
          </footer>
        </Modal>, document.body,
      )}
    </section>
  );
}
