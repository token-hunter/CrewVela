import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import "../styles/file-explorer-editing.css";
import { EmptyState } from "./ui/EmptyState";
import { ArrowClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowClockwise";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { FileIcon } from "@phosphor-icons/react/dist/csr/File";
import { FloppyDiskIcon } from "@phosphor-icons/react/dist/csr/FloppyDisk";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { GearIcon } from "@phosphor-icons/react/dist/csr/Gear";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import {
  DEFAULT_FILE_PREVIEW_SETTINGS,
  loadFilePreviewSettings,
  saveFilePreviewSettings,
  type FilePreviewKind,
  type FilePreviewSettings,
} from "../filePreviewSettings";

interface WorkspaceFileEntry {
  name: string;
  relativePath: string;
  directory: boolean;
  size: number;
  modifiedAt: number | null;
  extension: string;
}

interface Props {
  root: string | null;
  active: boolean;
}

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "webm", "mov", "m4v", "ogv"]);
const BINARY_EXTENSIONS = new Set(["doc", "docx", "xls", "xlsx", "ppt", "pptx", "zip", "7z", "rar"]);
/** 可进入编辑保存的文本类预览（html 走 iframe 预览，进入编辑时再取原文）。 */
const EDITABLE_KINDS: ReadonlySet<FilePreviewKind> = new Set(["code", "markdown", "html"]);
const FILE_TREE_WIDTH_STORAGE_KEY = "crewvela-file-tree-width";
const DEFAULT_FILE_TREE_WIDTH = 260;
const PREVIEW_LABELS: Record<FilePreviewKind, string> = {
  image: "图片",
  pdf: "PDF",
  markdown: "Markdown",
  html: "HTML",
  code: "代码 / 文本",
  binary: "二进制文件",
  video: "视频",
};

function previewKind(entry: WorkspaceFileEntry): FilePreviewKind {
  if (IMAGE_EXTENSIONS.has(entry.extension)) return "image";
  if (VIDEO_EXTENSIONS.has(entry.extension)) return "video";
  if (entry.extension === "pdf") return "pdf";
  if (entry.extension === "md" || entry.extension === "markdown") return "markdown";
  if (entry.extension === "html" || entry.extension === "htm") return "html";
  if (BINARY_EXTENSIONS.has(entry.extension)) return "binary";
  return "code";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function loadFileTreeWidth(): number {
  try {
    const raw = window.localStorage.getItem(FILE_TREE_WIDTH_STORAGE_KEY);
    if (raw === null) return DEFAULT_FILE_TREE_WIDTH;
    const width = Number(raw);
    return Number.isFinite(width) && width >= 0 ? width : DEFAULT_FILE_TREE_WIDTH;
  } catch {
    return DEFAULT_FILE_TREE_WIDTH;
  }
}

function highlightedLine(line: string): ReactNode[] {
  const parts = line.split(/("[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*'|\/\/.*$|#.*$|\b(?:const|let|var|function|fn|class|interface|type|return|if|else|for|while|match|use|import|from|export|pub|async|await|true|false|null|None|Some)\b|\b\d+(?:\.\d+)?\b)/g);
  return parts.map((part, index) => {
    let className = "";
    if (/^['"]/.test(part)) className = "string";
    else if (/^(\/\/|#)/.test(part)) className = "comment";
    else if (/^\d/.test(part)) className = "number";
    else if (/^[A-Za-z_]+$/.test(part)) className = "keyword";
    return className ? <span className={className} key={index}>{part}</span> : part;
  });
}

function MarkdownPreview({ content }: { content: string }) {
  const nodes: ReactNode[] = [];
  let code: string[] | null = null;
  for (const [index, line] of content.split(/\r?\n/).entries()) {
    if (line.startsWith("```")) {
      if (code) {
        nodes.push(<pre key={`code-${index}`}><code>{code.join("\n")}</code></pre>);
        code = null;
      } else code = [];
      continue;
    }
    if (code) {
      code.push(line);
      continue;
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      const Tag = `h${heading[1].length}` as keyof JSX.IntrinsicElements;
      nodes.push(<Tag key={index}>{heading[2]}</Tag>);
    } else if (/^[-*]\s+/.test(line)) {
      nodes.push(<li key={index}>{line.replace(/^[-*]\s+/, "")}</li>);
    } else if (line.trim()) {
      nodes.push(<p key={index}>{line}</p>);
    }
  }
  if (code) nodes.push(<pre key="code-tail"><code>{code.join("\n")}</code></pre>);
  return <article className="file-preview-markdown">{nodes}</article>;
}

export function FileExplorerPanel({ root, active }: Props) {
  const [children, setChildren] = useState<Record<string, WorkspaceFileEntry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set([""]));
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<WorkspaceFileEntry | null>(null);
  const [content, setContent] = useState("");
  const [mediaUrl, setMediaUrl] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [treeError, setTreeError] = useState<string | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [searchResults, setSearchResults] = useState<WorkspaceFileEntry[]>([]);
  const [codeQuery, setCodeQuery] = useState("");
  const [zoom, setZoom] = useState(1);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [mediaVersion, setMediaVersion] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<FilePreviewSettings>(loadFilePreviewSettings);
  const settingsRef = useRef(settings);
  const [fileTreeWidth, setFileTreeWidth] = useState(loadFileTreeWidth);
  const fileTreeResizeRef = useRef<{ startX: number; startWidth: number; panelWidth: number } | null>(null);
  const selectedRef = useRef(selected);
  const rootRef = useRef(root);
  const previewLoadRef = useRef(0);
  const rootGenerationRef = useRef(0);
  const directoryRequestsRef = useRef(new Map<string, number>());
  const draftsRef = useRef(new Map<string, { content: string; draft: string; error?: string }>());
  const selectionsRef = useRef(new Map<string, WorkspaceFileEntry>());
  const pendingSavesRef = useRef(new Set<string>());
  const selectedRootRef = useRef(root);
  const fileKey = (workspace: string, path: string) => JSON.stringify([workspace, path]);
  const autoLoadedRootRef = useRef<string | null>(null);
  const lastRootRef = useRef(root);
  selectedRef.current = selected;
  if (rootRef.current !== root) {
    rootRef.current = root;
    ++rootGenerationRef.current;
    ++previewLoadRef.current;
  }
  const panelStyle = {
    "--file-tree-width": `${fileTreeWidth}px`,
  } as CSSProperties;

  const loadDirectory = useCallback(async (relativePath: string, force = false) => {
    if (!root || (children[relativePath] && !force)) return;
    const generation = rootGenerationRef.current;
    const request = (directoryRequestsRef.current.get(relativePath) ?? 0) + 1;
    directoryRequestsRef.current.set(relativePath, request);
    const currentRequest = () => rootRef.current === root && generation === rootGenerationRef.current && directoryRequestsRef.current.get(relativePath) === request;
    setLoading((current) => new Set(current).add(relativePath));
    setTreeError(null);
    try {
      const entries = await invoke<WorkspaceFileEntry[]>("workspace_list_directory", {
        root,
        relativePath,
      });
      if (currentRequest()) setChildren((current) => ({ ...current, [relativePath]: entries }));
    } catch (reason) {
      if (currentRequest()) setTreeError(String(reason));
    } finally {
      if (!currentRequest()) return;
      setLoading((current) => {
        const next = new Set(current);
        next.delete(relativePath);
        return next;
      });
    }
  }, [children, root]);

  useEffect(() => {
    if (lastRootRef.current !== root) {
      lastRootRef.current = root;
      autoLoadedRootRef.current = null;
    }
    setChildren({});
    setExpanded(new Set([""]));
    const restored = root ? selectionsRef.current.get(root) ?? null : null;
    selectedRootRef.current = root;
    selectedRef.current = restored;
    setSelected(restored);
    setLoading(new Set());
    setSearchResults([]);
    setQuery("");
    setEditing(false);
    setDraft("");
    setSaving(Boolean(root && restored && pendingSavesRef.current.has(fileKey(root, restored.relativePath))));
    setContent("");
    setMediaUrl("");
    setError(null);
    setTreeError(null);
    if (root && active && autoLoadedRootRef.current !== root) {
      autoLoadedRootRef.current = root;
      void loadDirectory("", true);
    }
  }, [root]);

  useEffect(() => {
    if (root && active && autoLoadedRootRef.current !== root && !children[""] && !loading.has("")) {
      autoLoadedRootRef.current = root;
      void loadDirectory("");
    }
  }, [active, children, loadDirectory, loading, root]);

  useEffect(() => {
    if (!root || !selected || selectedRootRef.current !== root || selectedRef.current !== selected) return;
    let cancelled = false;
    const loadId = ++previewLoadRef.current;
    const kind = previewKind(selected);
    setContent("");
    setMediaUrl("");
    setCodeQuery("");
    setZoom(1);
    const key = fileKey(root, selected.relativePath);
    const savedDraft = draftsRef.current.get(key);
    setEditing(Boolean(savedDraft));
    setDraft(savedDraft?.draft ?? "");
    setSaving(pendingSavesRef.current.has(key));
    setError(savedDraft?.error ?? null);
    if (savedDraft) { setContent(savedDraft.content); return; }
    if (!settings[kind]) return;
    const load = async () => {
      try {
        if (["image", "pdf", "html", "video"].includes(kind)) {
          const path = await invoke<string>("workspace_media_path", {
            root,
            relativePath: selected.relativePath,
          });
          if (!cancelled && loadId === previewLoadRef.current) setMediaUrl(convertFileSrc(path));
        } else if (kind !== "binary") {
          const text = await invoke<string>("workspace_read_text", {
            root,
            relativePath: selected.relativePath,
          });
          if (!cancelled && loadId === previewLoadRef.current) setContent(text);
        }
      } catch (reason) {
        if (!cancelled && loadId === previewLoadRef.current) setError(String(reason));
      }
    };
    void load();
    return () => { cancelled = true; };
  }, [root, selected, settings]);

  useEffect(() => {
    const needle = query.trim();
    setTreeError(null);
    if (!root || needle.length < 2) {
      setSearchResults([]);
      return;
    }
    let cancelled = false;
    const generation = rootGenerationRef.current;
    setSearchResults([]);
    const currentRequest = () => !cancelled && rootRef.current === root && generation === rootGenerationRef.current;
    const timer = window.setTimeout(() => {
      void invoke<WorkspaceFileEntry[]>("workspace_search_files", { root, query: needle })
        .then((entries) => { if (currentRequest()) setSearchResults(entries); })
        .catch((reason) => { if (currentRequest()) setTreeError(String(reason)); });
    }, 180);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [query, root]);

  const visibleChildren = useMemo(() => children, [children]);

  const toggleDirectory = (entry: WorkspaceFileEntry) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(entry.relativePath)) next.delete(entry.relativePath);
      else next.add(entry.relativePath);
      return next;
    });
    if (!expanded.has(entry.relativePath)) void loadDirectory(entry.relativePath);
  };

  const selectFile = (entry: WorkspaceFileEntry) => {
    if (!root || selected?.relativePath === entry.relativePath) return;
    ++previewLoadRef.current;
    selectedRootRef.current = root;
    selectedRef.current = entry;
    selectionsRef.current.set(root, entry);
    setSelected(entry);
  };

  const renderEntries = (relativePath: string, depth = 0): ReactNode => (
    (visibleChildren[relativePath] ?? []).map((entry) => (
      <div key={entry.relativePath}>
        <button
          type="button"
          className={`file-tree-row${selected?.relativePath === entry.relativePath ? " selected" : ""}`}
          style={{ paddingLeft: 10 + depth * 14 }}
          onClick={() => entry.directory ? toggleDirectory(entry) : selectFile(entry)}
          title={entry.relativePath}
        >
          {entry.directory ? <CaretRightIcon className={expanded.has(entry.relativePath) ? "expanded" : ""} size={12} /> : <span className="file-tree-spacer" />}
          {entry.directory ? <FolderIcon size={15} weight="fill" /> : <FileIcon size={15} />}
          <span>{entry.name}</span>
        </button>
        {entry.directory && expanded.has(entry.relativePath) && (
          <div>{loading.has(entry.relativePath) ? <p className="file-tree-loading">读取中…</p> : renderEntries(entry.relativePath, depth + 1)}</div>
        )}
      </div>
    ))
  );

  const updateSetting = (kind: FilePreviewKind) => {
    const current = settingsRef.current;
    const next = { ...current, [kind]: !current[kind] };
    settingsRef.current = next;
    setSettings(next);
    setSettingsError(null);
    try {
      saveFilePreviewSettings(next);
    } catch (reason) {
      setSettingsError(`预览设置未保存：${String(reason)}`);
    }
  };

  const openSelected = async () => {
    if (!root || !selected) return;
    try {
      await invoke("workspace_open_path", { root, relativePath: selected.relativePath });
    } catch (reason) {
      setError(String(reason));
    }
  };

  const revealSelected = async () => {
    if (!root || !selected) return;
    try {
      await invoke("workspace_reveal_path", { root, relativePath: selected.relativePath });
    } catch (reason) {
      setError(String(reason));
    }
  };

  const closePreview = () => {
    if (saving) return;
    if (editing && draft !== content && !window.confirm("关闭文件预览？未保存的修改将丢失。")) return;

    if (root && selected) draftsRef.current.delete(fileKey(root, selected.relativePath));
    if (root) selectionsRef.current.delete(root);
    // Invalidate pending preview and edit-entry reads before clearing state.
    ++previewLoadRef.current;
    selectedRef.current = null;
    setSelected(null);
    setContent("");
    setMediaUrl("");
    setError(null);
    setCodeQuery("");
    setZoom(1);
    setEditing(false);
    setDraft("");
    setMediaVersion(0);
  };

  const enterEdit = async () => {
    if (!root || !selected || !kind || !EDITABLE_KINDS.has(kind)) return;
    setError(null);
    const loadId = ++previewLoadRef.current;
    try {
      const text = await invoke<string>("workspace_read_text", {
        root,
        relativePath: selected.relativePath,
      });
      if (selectedRef.current === selected && rootRef.current === root && loadId === previewLoadRef.current) {
        draftsRef.current.set(fileKey(root, selected.relativePath), { content: text, draft: text });
        setContent(text);
        setDraft(text);
        setEditing(true);
      }
    } catch (reason) {
      if (selectedRef.current === selected && rootRef.current === root && loadId === previewLoadRef.current) setError(String(reason));
    }
  };

  const cancelEdit = () => {
    if (saving || (draft !== content && !window.confirm("放弃未保存的修改？"))) return;
    if (root && selected) draftsRef.current.delete(fileKey(root, selected.relativePath));
    setEditing(false);
    setDraft("");
  };

  const saveDraft = async () => {
    if (!root || !selected || !editing || saving) return;
    const key = fileKey(root, selected.relativePath);
    if (pendingSavesRef.current.has(key)) return;
    pendingSavesRef.current.add(key);
    const savedText = draft;
    const currentFile = () => rootRef.current === root && selectedRootRef.current === root && selectedRef.current?.relativePath === selected.relativePath;
    setSaving(true);
    setError(null);
    try {
      const size = await invoke<number>("workspace_write_text", {
        root,
        relativePath: selected.relativePath,
        contents: savedText,
        expectedContents: content,
      });
      draftsRef.current.delete(key);
      const updated = { ...selected, size, modifiedAt: Date.now() };
      if (!currentFile()) return;
      selectionsRef.current.set(root, updated);
      setContent(savedText);
      selectedRef.current = updated;
      setSelected(updated);
      setEditing(false);
      setDraft("");
      setMediaVersion((version) => version + 1);
    } catch (reason) {
      const pending = draftsRef.current.get(key);
      if (pending) pending.error = String(reason);
      if (currentFile()) setError(String(reason));
    } finally {
      pendingSavesRef.current.delete(key);
      if (currentFile()) setSaving(false);
    }
  };

  const editorKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveDraft();
    }
  };

  const clampFileTreeWidth = (width: number, panelWidth: number) => {
    return Math.max(0, Math.min(panelWidth, width));
  };

  const persistFileTreeWidth = (width: number, panelWidth: number) => {
    const next = clampFileTreeWidth(width, panelWidth);
    setFileTreeWidth(next);
    try {
      window.localStorage.setItem(FILE_TREE_WIDTH_STORAGE_KEY, String(next));
    } catch {
      // 当前窗口仍可继续使用调整后的宽度。
    }
  };

  const startFileTreeResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const panelWidth = event.currentTarget.parentElement?.clientWidth ?? 760;
    fileTreeResizeRef.current = { startX: event.clientX, startWidth: fileTreeWidth, panelWidth };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveFileTreeResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = fileTreeResizeRef.current;
    if (!drag) return;
    setFileTreeWidth(clampFileTreeWidth(drag.startWidth + drag.startX - event.clientX, drag.panelWidth));
  };

  const stopFileTreeResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = fileTreeResizeRef.current;
    if (!drag) return;
    fileTreeResizeRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    persistFileTreeWidth(drag.startWidth + drag.startX - event.clientX, drag.panelWidth);
  };

  const resizeFileTreeWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const panelWidth = event.currentTarget.parentElement?.clientWidth ?? 760;
    persistFileTreeWidth(fileTreeWidth + (event.key === "ArrowLeft" ? 24 : -24), panelWidth);
  };

  const kind = selected ? previewKind(selected) : null;
  const codeLines = content.split(/\r?\n/);

  return (
    <section className="file-explorer-panel" aria-label="项目文件" style={panelStyle}>
      <div className="file-preview-pane">
        {!selected || !kind ? (
          <EmptyState className="file-preview-empty" icon={<FileIcon size={28} />} title="选择文件进行预览" titleAs="span" />
        ) : (
          <>
            <header>
              <div className="file-preview-file-line">
                <FileIcon size={15} />
                <strong title={selected.name}>{selected.name}</strong>
                <span>{formatBytes(selected.size)} · {PREVIEW_LABELS[kind]}</span>
              </div>
              <div className="file-preview-header-actions">
                {EDITABLE_KINDS.has(kind) && !editing && (
                  <button type="button" aria-label="编辑文件" title="编辑并保存" onClick={() => void enterEdit()}><PencilSimpleIcon size={15} /></button>
                )}
                <button type="button" aria-label="打开所在目录" title="在资源管理器中显示" onClick={() => void revealSelected()}><FolderOpenIcon size={15} /></button>
                <button type="button" aria-label="使用系统应用打开" title="使用系统应用打开" onClick={() => void openSelected()}><ArrowSquareOutIcon size={15} /></button>
                <button type="button" aria-label="关闭文件" title="关闭文件" onClick={closePreview} disabled={saving}><XIcon size={15} /></button>
              </div>
            </header>
            {editing && (
              <div className="file-preview-edit-bar">
                <span className={draft !== content ? "dirty" : ""}>{draft !== content ? "未保存" : "无改动"}</span>
                <button type="button" onClick={() => void saveDraft()} disabled={saving || draft === content}>
                  <FloppyDiskIcon size={14} /> {saving ? "保存中…" : "保存"}
                </button>
                <button type="button" onClick={cancelEdit} disabled={saving}><XIcon size={14} /> 取消</button>
              </div>
            )}
            {editing && error && <div className="file-preview-error file-preview-edit-error" role="alert">{error}</div>}
            {editing ? (
              <textarea
                className="file-preview-editor"
                value={draft}
                disabled={saving}
                onChange={(event) => {
                  const next = event.target.value;
                  setDraft(next);
                  if (root) draftsRef.current.set(fileKey(root, selected.relativePath), { content, draft: next });
                }}
                onKeyDown={editorKeyDown}
                spellCheck={false}
                aria-label={`编辑 ${selected.name}`}
              />
            ) : !settings[kind] ? (
              <EmptyState
                className="file-preview-empty"
                title={`${PREVIEW_LABELS[kind]}预览已关闭`}
                titleAs="span"
                action={<button type="button" onClick={() => updateSetting(kind)}>启用预览</button>}
              />
            ) : error ? (
              <div className="file-preview-error">{error}</div>
            ) : kind === "image" ? (
              <div className="file-preview-image"><div><button type="button" onClick={() => setZoom((value) => Math.max(.25, value - .25))}>−</button><span>{Math.round(zoom * 100)}%</span><button type="button" onClick={() => setZoom((value) => Math.min(4, value + .25))}>＋</button></div>{mediaUrl && <img src={mediaUrl} alt={selected.name} style={{ width: `${zoom * 100}%` }} />}</div>
            ) : kind === "video" ? (
              <div className="file-preview-media">{mediaUrl && <video src={mediaUrl} controls playsInline preload="metadata" />}</div>
            ) : kind === "pdf" ? (
              <iframe className="file-preview-frame" title={selected.name} src={mediaUrl} />
            ) : kind === "html" ? (
              <iframe key={mediaVersion} className="file-preview-frame" title={selected.name} src={mediaUrl} sandbox="" />
            ) : kind === "markdown" ? (
              <MarkdownPreview content={content} />
            ) : kind === "binary" ? (
              <EmptyState
                className="file-preview-empty"
                icon={<FileIcon size={28} />}
                title="此文件需要使用系统应用打开"
                titleAs="span"
                action={<button type="button" onClick={() => void openSelected()}>打开文件</button>}
              />
            ) : (
              <div className="file-preview-code"><label><MagnifyingGlassIcon size={13} /><input value={codeQuery} onChange={(event) => setCodeQuery(event.target.value)} placeholder="在文件中查找" /></label><pre>{codeLines.map((line, index) => <div className={codeQuery && line.toLocaleLowerCase().includes(codeQuery.toLocaleLowerCase()) ? "match" : ""} key={index}><span>{index + 1}</span><code>{highlightedLine(line)}</code></div>)}</pre></div>
            )}
          </>
        )}
      </div>
      <div
        className="file-preview-divider"
        role="separator"
        aria-label="调整文件树宽度"
        aria-orientation="vertical"
        aria-valuemin={0}
        aria-valuemax={window.innerWidth}
        aria-valuenow={Math.round(fileTreeWidth)}
        tabIndex={0}
        onPointerDown={startFileTreeResize}
        onPointerMove={moveFileTreeResize}
        onPointerUp={stopFileTreeResize}
        onPointerCancel={stopFileTreeResize}
        onDoubleClick={(event) => persistFileTreeWidth(DEFAULT_FILE_TREE_WIDTH, event.currentTarget.parentElement?.clientWidth ?? 760)}
        onKeyDown={resizeFileTreeWithKeyboard}
      />
      <aside className="file-explorer-browser" aria-label="项目文件树">
        <header className="file-explorer-header">
          <div><FolderIcon size={17} weight="fill" /><strong>项目文件</strong></div>
          <div>
            <button type="button" aria-label="刷新文件树" title="刷新文件树" onClick={() => void loadDirectory("", true)} disabled={!root}><ArrowClockwiseIcon size={15} /></button>
            <button type="button" aria-label="文件预览设置" title="文件预览设置" onClick={() => setSettingsOpen((value) => !value)}><GearIcon size={15} /></button>
          </div>
        </header>
        {settingsOpen && (
          <div className="file-preview-settings">
            {Object.entries(PREVIEW_LABELS).map(([value, label]) => (
              <label key={value}><span>{label}</span><input type="checkbox" checked={settings[value as FilePreviewKind]} onChange={() => updateSetting(value as FilePreviewKind)} /></label>
            ))}
          </div>
        )}
        <label className="file-explorer-search">
          <MagnifyingGlassIcon size={14} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="筛选文件…" disabled={!root} />
        </label>
        <div className="file-explorer-tree">
          {(treeError || settingsError) && <div role="alert" style={{ padding: 8, color: "var(--danger)", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{[treeError, settingsError].filter(Boolean).join("\n")}</div>}
          {!root ? <p className="file-explorer-empty">当前工作区没有可浏览的目录</p> : query.trim().length >= 2 ? searchResults.map((entry) => <button type="button" className="file-tree-row file-search-result" key={entry.relativePath} onClick={() => entry.directory ? toggleDirectory(entry) : selectFile(entry)}><span className="file-tree-spacer" />{entry.directory ? <FolderIcon size={15} weight="fill" /> : <FileIcon size={15} />}<span>{entry.relativePath}</span></button>) : loading.has("") && !children[""] ? <p className="file-explorer-empty">正在读取文件…</p> : children[""]?.length === 0 ? <p className="file-explorer-empty">目录为空</p> : renderEntries("")}
        </div>
      </aside>
    </section>
  );
}
