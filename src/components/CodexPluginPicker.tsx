import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PuzzlePiece, X } from "@phosphor-icons/react";
import { parseCodexPlugins, previewCodexPlugins, type CodexPlugin } from "../codexPlugins";
import "../styles/codex-plugins.css";

export function CodexPluginPicker({ sessionId, preview, open, selected, onClose, onSelect }: {
  sessionId: string; preview: boolean; open: boolean; selected: CodexPlugin[];
  onClose: () => void; onSelect: (plugin: CodexPlugin) => void;
}) {
  const [plugins, setPlugins] = useState<CodexPlugin[]>([]);
  const [search, setSearch] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [reload, setReload] = useState(0);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!open) return;
    let disposed = false;
    setSearch(""); setLoading(true); setError(""); setPlugins([]); input.current?.focus();
    const load = preview ? Promise.resolve({ plugins: previewCodexPlugins, errors: [] }) : invoke("get_codex_chat_plugins", { sessionId }).then(parseCodexPlugins);
    void load.then(result => { if (!disposed) { setPlugins(result.plugins); setError(result.errors.join("；")); } }).catch(reason => { if (!disposed) setError(String(reason)); }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [open, sessionId, preview, reload]);
  if (!open) return null;
  const filtered = plugins.filter(plugin => `${plugin.displayName} ${plugin.name} ${plugin.description}`.toLowerCase().includes(search.toLowerCase()));
  return <div className="codex-goal-overlay" onMouseDown={event => { if (event.currentTarget === event.target) onClose(); }}>
    <div ref={dialog} className="codex-goal-dialog harness-goal-dialog codex-plugin-dialog" role="dialog" aria-modal="true" aria-label="Codex 插件" onKeyDown={event => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
      if (event.key === "Tab") {
        const items = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)');
        if (!items?.length) return;
        const first = items[0], last = items[items.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    }}>
      <header><h2><PuzzlePiece size={22} />插件</h2><button type="button" aria-label="关闭插件选择" onClick={onClose}><X size={18} /></button></header>
      <p>选择已安装且已启用的插件，随下一条消息发送给 Codex。</p>
      <input ref={input} aria-label="搜索 Codex 插件" placeholder="搜索插件…" value={search} onChange={event => setSearch(event.target.value)} />
      {loading && <p role="status">正在读取 Codex 插件…</p>}
      {error && <p role="alert">{error}</p>}
      <div className="codex-plugin-list">{filtered.map(plugin => <button type="button" key={plugin.id} disabled={selected.some(item => item.id === plugin.id)} onClick={() => { onSelect(plugin); onClose(); }}><PuzzlePiece size={20} /><span><strong>{plugin.displayName}</strong><small>{plugin.description || plugin.marketplace}</small></span><small>{selected.some(item => item.id === plugin.id) ? "已选择" : "添加"}</small></button>)}</div>
      {!loading && !error && filtered.length === 0 && <p role="status">{plugins.length ? "没有匹配的插件" : "当前 Codex 没有已安装且可用的插件"}</p>}
      <footer><button type="button" disabled={loading} onClick={() => setReload(value => value + 1)}>刷新</button><button type="button" onClick={onClose}>完成</button></footer>
    </div>
  </div>;
}
