import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { BrainIcon } from "@phosphor-icons/react/dist/csr/Brain";
import { BookBookmarkIcon } from "@phosphor-icons/react/dist/csr/BookBookmark";
import { ChatCircleDotsIcon } from "@phosphor-icons/react/dist/csr/ChatCircleDots";
import { CheckSquareOffsetIcon } from "@phosphor-icons/react/dist/csr/CheckSquareOffset";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { PackageIcon } from "@phosphor-icons/react/dist/csr/Package";
import { SealCheckIcon } from "@phosphor-icons/react/dist/csr/SealCheck";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  KnowledgeSearchResult,
  MultiAgentProject,
  MultiAgentSnapshot,
} from "../types";

interface Props {
  onClose?: () => void;
  embedded?: boolean;
  projectId?: string | null;
  snapshot?: MultiAgentSnapshot | null;
  onOpenRecord?: (result: KnowledgeSearchResult) => void;
}

const SOURCE_LABELS: Record<string, string> = {
  project: "项目",
  task: "任务",
  conversation: "对话",
  run: "运行",
  artifact: "交付",
  review: "审核",
  memory: "经验",
};

function formatTime(value: number): string {
  return new Date(value).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function sourceIcon(sourceType: string) {
  const props = { size: 18, weight: "regular" as const, "aria-hidden": true as const };
  switch (sourceType) {
    case "project":
      return <FolderOpenIcon {...props} />;
    case "task":
      return <CheckSquareOffsetIcon {...props} />;
    case "conversation":
      return <ChatCircleDotsIcon {...props} />;
    case "run":
      return <TerminalWindowIcon {...props} />;
    case "artifact":
      return <PackageIcon {...props} />;
    case "review":
      return <SealCheckIcon {...props} />;
    case "memory":
      return <BrainIcon {...props} />;
    default:
      return <FileTextIcon {...props} />;
  }
}

export function KnowledgeBaseDialog({ onClose, embedded = false, projectId: projectIdProp, snapshot, onOpenRecord }: Props) {
  const [projects, setProjects] = useState<MultiAgentProject[]>([]);
  const [projectId, setProjectId] = useState(projectIdProp ?? "");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<KnowledgeSearchResult[]>([]);
  const [loading, setLoading] = useState(true);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(0);
  const [sourceType, setSourceType] = useState("");
  const [hasMore, setHasMore] = useState(false);
  const [searchedQuery, setSearchedQuery] = useState("");
  const preview = new URLSearchParams(window.location.search).get("ui-preview") === "1";
  const previewSnapshot = useRef(snapshot);
  previewSnapshot.current = snapshot;

  const search = useCallback(async (nextProjectId: string, nextQuery: string, type = "", offset = 0) => {
    const sequence = ++requestSequence.current;
    if (!offset) { setResults([]); setHasMore(false); setSearchedQuery(nextQuery); }
    if (!nextProjectId) { setSearching(false); return; }
    setSearching(true);
    setError(null);
    try {
      const next = preview ? Array.from({ length: 75 }, (_, index): KnowledgeSearchResult => {
        const task = previewSnapshot.current?.tasks[index % (previewSnapshot.current?.tasks.length || 1)];
        const sourceType = index % 3 === 0 ? "task" : index % 3 === 1 ? "conversation" : "artifact";
        return { id: `preview-record-${index}`, projectId: nextProjectId, sourceType, sourceId: sourceType === "task" ? task?.id ?? String(index) : `preview-${sourceType}-${index}`, taskId: task?.id ?? null, runId: null,
          title: `${sourceType === "task" ? "任务" : sourceType === "conversation" ? "对话" : "交付"} · ${task?.title ?? "协作演示"} ${index + 1}`, snippet: "协作记录演示：可筛选、加载更多及查看关联上下文。", content: `演示记录 ${index + 1}\n${task?.description ?? "协作讨论与交付检查"}`, sourceUri: `preview/${sourceType}/${index}`, relevance: 0, createdAt: Date.now(), updatedAt: Date.now() - index * 1000 };
      }).filter((record) => (!type || record.sourceType === type) && (!nextQuery.trim() || `${record.title} ${record.content}`.includes(nextQuery.trim()))).slice(offset, offset + 61)
        : await invoke<KnowledgeSearchResult[]>("ma_search_knowledge", { input: { projectId: nextProjectId, query: nextQuery.trim(), sourceTypes: type ? [type] : [], taskId: null, limit: 61, offset } });
      if (sequence === requestSequence.current) {
        setHasMore(next.length > 60);
        setResults((current) => offset ? [...current, ...next.slice(0, 60)] : next.slice(0, 60));
      }
    } catch (reason) {
      if (sequence === requestSequence.current) setError(String(reason));
    } finally {
      if (sequence === requestSequence.current) setSearching(false);
    }
  }, [preview]);

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    const load = preview ? Promise.resolve(snapshot ? [snapshot.project] : []) : invoke<MultiAgentProject[]>("ma_list_projects");
    load.then((nextProjects) => {
      if (disposed) return;
      setProjects(nextProjects);
      const id = projectIdProp !== undefined ? projectIdProp ?? "" : nextProjects[0]?.id ?? "";
      setProjectId(id);
    }).catch((reason) => { if (!disposed) setError(String(reason)); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; requestSequence.current += 1; };
  }, [projectIdProp, preview, snapshot?.project.id]);
  useEffect(() => { setQuery(""); void search(projectId, "", sourceType); }, [projectId, search]);

  const panel = (
      <section
        className={`knowledge-dialog${embedded ? " embedded" : ""}`}
        role={embedded ? undefined : "dialog"}
        aria-modal={embedded ? undefined : "true"}
        aria-labelledby="knowledge-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="knowledge-dialog-heading">
          <div>
            <BookBookmarkIcon size={22} aria-hidden="true" />
            <span>
              <h2 id="knowledge-dialog-title">项目档案</h2>
              <p>搜索多智能体项目中的任务、对话、交付和审核记录。</p>
            </span>
          </div>
          {onClose && <button type="button" aria-label="关闭项目档案" title="关闭" onClick={onClose}>
            <XIcon size={16} />
          </button>}
        </header>

        <div className="knowledge-dialog-toolbar">
          {projectIdProp === undefined && <label className="knowledge-project-field">
            <span className="knowledge-toolbar-label">项目</span>
            <select
              aria-label="选择项目"
              value={projectId}
              disabled={projects.length === 0}
              onChange={(event) => {
                const nextProjectId = event.target.value;
                setProjectId(nextProjectId);

              }}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>{project.name}</option>
              ))}
            </select>
          </label>}
          <label className="knowledge-project-field"><span className="knowledge-toolbar-label">类型</span><select aria-label="档案类型" value={sourceType} onChange={(event) => { const type = event.target.value; setSourceType(type); void search(projectId, searchedQuery, type); }}><option value="">全部类型</option>{Object.entries(SOURCE_LABELS).map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select></label>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void search(projectId, query, sourceType);
            }}
          >
            <MagnifyingGlassIcon size={15} aria-hidden="true" />
            <input
              value={query}
              disabled={!projectId}
              placeholder="搜索需求、对话、任务或审核…"
              aria-label="搜索项目档案"
              onChange={(event) => setQuery(event.target.value)}
            />
            <button type="submit" disabled={!projectId || searching}>
              {searching ? "检索中" : "搜索"}
            </button>
          </form>
        </div>

        <div className="knowledge-dialog-meta">
          <span>项目记录</span>
          <span>已显示 {results.length} 条{hasMore ? "，还有更多" : ""}</span>
        </div>

        <div className="knowledge-dialog-content">
          {error ? (
            <p className="knowledge-dialog-state error" role="alert">{error}</p>
          ) : loading ? (
            <p className="knowledge-dialog-state">正在读取项目…</p>
          ) : !projectId ? (
            <p className="knowledge-dialog-state">暂无选中的协作项目，请先选择或创建项目。</p>
          ) : searching && results.length === 0 ? (
            <p className="knowledge-dialog-state">正在检索项目记录…</p>
          ) : results.length === 0 && !searching ? (
            <p className="knowledge-dialog-state">
              {query.trim() ? "没有找到相关记录" : "当前项目还没有可检索记录"}
            </p>
          ) : (
            <div className="knowledge-dialog-results">
              {results.map((result) => (
                <article className="knowledge-result" key={result.id}>
                  <div className="knowledge-result-type" aria-hidden="true">
                    <span className="knowledge-result-icon">
                      {sourceIcon(result.sourceType)}
                    </span>
                    <span>{SOURCE_LABELS[result.sourceType] ?? result.sourceType}</span>
                  </div>
                  <div className="knowledge-result-main">
                    <div className="knowledge-result-heading">
                      <h3>{result.title}</h3>
                      <time>{formatTime(result.updatedAt)}</time>
                    </div>
                    <p>{result.snippet}</p>
                    {onOpenRecord && <button className="btn secondary" onClick={() => onOpenRecord(result)}>{result.sourceType === "task" ? "查看关联任务" : "查看" + (SOURCE_LABELS[result.sourceType] ?? "记录") + "详情"}</button>}
                    <details>
                      <summary>查看原文与引用</summary>
                      <pre>{result.content}</pre>
                      <code title={result.sourceUri}>{result.sourceUri}</code>
                    </details>
                  </div>
                </article>
              ))}
              {hasMore && <button className="btn secondary" disabled={searching} onClick={() => void search(projectId, searchedQuery, sourceType, results.length)}>{searching ? "加载中…" : "加载更多"}</button>}
            </div>
          )}
        </div>
      </section>
  );

  if (embedded) return panel;

  return createPortal(
    <div className="dialog-overlay" onClick={onClose}>
      {panel}
    </div>,
    document.body,
  );
}
