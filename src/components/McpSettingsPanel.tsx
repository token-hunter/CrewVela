import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "./ui/Switch";
import { EmptyState } from "./ui/EmptyState";
import {
  ArrowsClockwiseIcon,
  CaretDownIcon,
  CaretRightIcon,
  PencilIcon,
  PlugsIcon,
  PlusIcon,
  TrashIcon,
  WarningCircleIcon,
  XIcon,
} from "@phosphor-icons/react";
import {
  draftFromServer,
  draftToUpsertInput,
  EMPTY_DRAFT,
  MCP_PREVIEW_REPORTS,
  MCP_PREVIEW_SERVERS,
  TRANSPORT_LABELS,
  TRANSPORT_OPTIONS,
  validateDraft,
  type McpProbeReport,
  type McpServerConfig,
  type McpServerDraft,
} from "../mcpServers";

const UI_PREVIEW =
  new URLSearchParams(window.location.search).get("ui-preview") === "1";

type StatusView = "unknown" | "connected" | "failed" | "disabled";

function serverDetailView(server: McpServerConfig): string {
  if (server.transport === "stdio") {
    return [server.command, ...server.args].filter(Boolean).join(" ");
  }
  return server.url ?? "";
}

export function McpSettingsPanel() {
  const [servers, setServers] = useState<McpServerConfig[]>(
    UI_PREVIEW ? MCP_PREVIEW_SERVERS : [],
  );
  const [reports, setReports] = useState<Record<string, McpProbeReport>>(
    UI_PREVIEW
      ? Object.fromEntries(MCP_PREVIEW_REPORTS.map((report) => [report.id, report]))
      : {},
  );
  const [draft, setDraft] = useState<McpServerDraft | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [probing, setProbing] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!UI_PREVIEW);
  const [saving, setSaving] = useState(false);
  const operation = useRef(!UI_PREVIEW);
  const busy = loading || probing || saving || busyId !== null;
  const beginOperation = () => {
    if (operation.current) return false;
    operation.current = true;
    return true;
  };
  const clearReport = (id: string) => setReports((current) => {
    const next = { ...current }; delete next[id]; return next;
  });

  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    operation.current = true;
    void invoke<McpServerConfig[]>("mcp_list_servers")
      .then((latest) => { if (!disposed) setServers(latest); })
      .catch((reason) => { if (!disposed) setNotice(`读取失败：${String(reason)}`); })
      .finally(() => { if (!disposed) { operation.current = false; setLoading(false); } });
    return () => { disposed = true; };
  }, []);

  const refresh = async () => {
    if (!beginOperation()) return;
    setProbing(true);
    setNotice(null);
    try {
      const [latest, fresh] = await Promise.all([
        invoke<McpServerConfig[]>("mcp_list_servers"),
        invoke<McpProbeReport[]>("mcp_probe_servers"),
      ]);
      setServers(latest);
      setReports(Object.fromEntries(fresh.map((report) => [report.id, report])));
    } catch (reason) {
      setNotice(`检测失败：${String(reason)}`);
    } finally {
      operation.current = false;
      setProbing(false);
    }
  };

  const toggleEnabled = async (server: McpServerConfig) => {
    if (!beginOperation()) return;
    setBusyId(server.id);
    setNotice(null);
    try {
      const updated = await invoke<McpServerConfig>("mcp_set_server_enabled", {
        id: server.id,
        enabled: !server.enabled,
      });
      setServers((current) =>
        current.map((item) => (item.id === updated.id ? updated : item)),
      );
      clearReport(server.id);
    } catch (reason) {
      setNotice(String(reason));
    } finally {
      operation.current = false;
      setBusyId(null);
    }
  };

  const remove = async (server: McpServerConfig) => {
    if (!beginOperation()) return;
    setBusyId(server.id);
    setNotice(null);
    try {
      await invoke<void>("mcp_delete_server", { id: server.id });
      setServers((current) => current.filter((item) => item.id !== server.id));
      clearReport(server.id);
      setNotice(`已删除 ${server.name}`);
    } catch (reason) {
      setNotice(String(reason));
    } finally {
      operation.current = false;
      setBusyId(null);
    }
  };

  const submitDraft = async () => {
    if (!draft) return;
    const error = validateDraft(draft, servers);
    if (error) {
      setFormError(error);
      return;
    }
    if (!beginOperation()) return;
    setSaving(true);
    setFormError(null);
    try {
      const saved = await invoke<McpServerConfig>("mcp_save_server", {
        input: draftToUpsertInput(draft),
      });
      setServers((current) => {
        const exists = current.some((item) => item.id === saved.id);
        return exists
          ? current.map((item) => (item.id === saved.id ? saved : item))
          : [...current, saved];
      });
      clearReport(saved.id);
      setDraft(null);
      setNotice(`已保存 ${saved.name}，新会话启动时生效`);
    } catch (reason) {
      setFormError(String(reason));
    } finally {
      operation.current = false;
      setSaving(false);
    }
  };

  const update = (patch: Partial<McpServerDraft>) =>
    setDraft((current) => (current ? { ...current, ...patch } : current));

  const statusOf = (server: McpServerConfig): StatusView => {
    if (!server.enabled) return "disabled";
    const report = reports[server.id];
    if (!report) return "unknown";
    return report.ok ? "connected" : "failed";
  };

  const enabledCount = servers.filter((server) => server.enabled).length;
  const toolTotal = Object.values(reports)
    .filter((report) => report.ok)
    .reduce((total, report) => total + report.tools.length, 0);

  return (
    <div className="mcp-settings-panel">
      <div className="mcp-toolbar">
        <button
          type="button"
          className="mcp-add-button"
          disabled={busy}
          onClick={() => {
            setDraft({ ...EMPTY_DRAFT });
            setFormError(null);
          }}
        >
          <PlusIcon size={15} weight="bold" /> 新增服务器
        </button>
        <button
          type="button"
          className="mcp-refresh-button"
          disabled={busy}
          onClick={() => void refresh()}
        >
          <ArrowsClockwiseIcon size={15} weight="bold" />
          {probing ? "检测中…" : "检测连接"}
        </button>
        <span className="mcp-summary">
          {enabledCount}/{servers.length} 启用 · {toolTotal} 个工具
        </span>
      </div>
      {notice && <p className="mcp-notice">{notice}</p>}
      {loading && <p role="status">正在读取 MCP 服务器…</p>}

      {draft && (
        <fieldset className="mcp-server-form" disabled={busy} style={{ minWidth: 0 }}>
          <header>
            <PlugsIcon size={17} weight="duotone" />
            <strong>{draft.id ? "编辑服务器" : "新增服务器"}</strong>
            <button
              type="button"
              aria-label="取消编辑"
              onClick={() => setDraft(null)}
            >
              <XIcon size={15} />
            </button>
          </header>
          <label className="mcp-form-row">
            <span>传输类型</span>
            <div className="mcp-transport-options" role="radiogroup" aria-label="传输类型">
              {TRANSPORT_OPTIONS.map((option) => (
                <button
                  key={option.id}
                  type="button"
                  role="radio"
                  aria-checked={draft.transport === option.id}
                  className={draft.transport === option.id ? "selected" : ""}
                  onClick={() => update({ transport: option.id })}
                  title={option.hint}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </label>
          <label className="mcp-form-row">
            <span>名称</span>
            <input
              value={draft.name}
              placeholder="如 filesystem"
              onChange={(event) => update({ name: event.target.value })}
            />
          </label>
          {draft.transport === "stdio" ? (
            <>
              <label className="mcp-form-row">
                <span>命令</span>
                <input
                  value={draft.command}
                  placeholder="npx"
                  onChange={(event) => update({ command: event.target.value })}
                />
              </label>
              <label className="mcp-form-row mcp-form-tall">
                <span title={'每行一个参数；空参数或首尾空格可使用 JSON 字符串，如 "" 或 " text "'}>参数（每行一个）</span>
                <textarea
                  value={draft.argsText}
                  placeholder={"-y\n@modelcontextprotocol/server-filesystem\n."}
                  onChange={(event) => update({ argsText: event.target.value })}
                />
              </label>
              <label className="mcp-form-row mcp-form-tall">
                <span>环境变量（KEY=VALUE 每行一条）</span>
                <textarea
                  value={draft.envText}
                  placeholder="API_KEY=secret"
                  onChange={(event) => update({ envText: event.target.value })}
                />
              </label>
            </>
          ) : (
            <>
              <label className="mcp-form-row">
                <span>URL</span>
                <input
                  value={draft.url}
                  placeholder="https://mcp.example.com/mcp"
                  onChange={(event) => update({ url: event.target.value })}
                />
              </label>
              {draft.transport === "http" && (
                <label className="mcp-form-row mcp-form-tall">
                  <span>请求头（KEY: VALUE 每行一条）</span>
                  <textarea
                    value={draft.headersText}
                    placeholder="Authorization: Bearer token"
                    onChange={(event) =>
                      update({ headersText: event.target.value })
                    }
                  />
                </label>
              )}
            </>
          )}
          <label className="mcp-form-check">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(event) => update({ enabled: event.target.checked })}
            />
            <span>启用（注入新启动的 Claude 会话与多 Agent 任务）</span>
          </label>
          {formError && (
            <p className="mcp-form-error">
              <WarningCircleIcon size={14} weight="bold" />
              {formError}
            </p>
          )}
          <footer className="mcp-form-actions">
            <button type="button" onClick={() => setDraft(null)}>
              取消
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => void submitDraft()}
            >
              {saving ? "保存中…" : "保存"}
            </button>
          </footer>
        </fieldset>
      )}

      {servers.length === 0 && !draft && !loading && (
        <EmptyState
          className="mcp-empty"
          icon={<PlugsIcon size={28} weight="duotone" />}
          title="还没有配置 MCP 服务器"
          titleAs="p"
          description="添加后，启用的服务器会自动注入新启动的 Claude 会话与多 Agent 任务。"
        />
      )}

      <div className="mcp-server-list">
        {servers.map((server) => {
          const status = statusOf(server);
          const report = reports[server.id];
          const isOpen = expanded[server.id] ?? false;
          return (
            <article key={server.id} className="mcp-server-card" data-status={status}>
              <header>
                <span className="mcp-status-dot" data-status={status} aria-hidden="true" />
                <strong title={server.name}>{server.name}</strong>
                <span className="mcp-transport-badge">
                  {TRANSPORT_LABELS[server.transport]}
                </span>
                <span className="mcp-card-actions">
                  <button
                    type="button"
                    className="icon"
                    aria-label={`编辑 ${server.name}`}
                    disabled={busy}
                    onClick={() => {
                      setDraft(draftFromServer(server));
                      setFormError(null);
                    }}
                  >
                    <PencilIcon size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon danger"
                    aria-label={`删除 ${server.name}`}
                    disabled={busy}
                    onClick={() => void remove(server)}
                  >
                    <TrashIcon size={14} />
                  </button>
                  <Switch
                    className="mcp-toggle"
                    title={server.enabled ? "已启用" : "已停用"}
                    checked={server.enabled}
                    disabled={busy}
                    onChange={() => void toggleEnabled(server)}
                    withKnob={false}
                  />
                </span>
              </header>
              <p className="mcp-card-detail">{serverDetailView(server)}</p>
              <div className="mcp-card-sub">
                {status === "disabled" ? (
                  <span className="mcp-status-label">已停用</span>
                ) : status === "failed" ? (
                  <span className="mcp-status-label failed">
                    连接失败{report?.error ? `：${report.error}` : ""}
                  </span>
                ) : status === "connected" ? (
                  <span className="mcp-status-label">
                    已连接 · {report?.tools.length ?? 0} 个工具
                  </span>
                ) : (
                  <span className="mcp-status-label muted">未检测</span>
                )}
                {report && report.tools.length > 0 && (
                  <button
                    type="button"
                    className="mcp-tools-toggle"
                    aria-expanded={isOpen}
                    onClick={() =>
                      setExpanded((current) => ({
                        ...current,
                        [server.id]: !current[server.id],
                      }))
                    }
                  >
                    {isOpen ? <CaretDownIcon size={13} /> : <CaretRightIcon size={13} />}
                    工具列表
                  </button>
                )}
              </div>
              {isOpen && report && (
                <ul className="mcp-tool-list">
                  {report.tools.map((tool) => (
                    <li key={tool.name}>
                      <code>{tool.name}</code>
                      {tool.description && <small>{tool.description}</small>}
                    </li>
                  ))}
                </ul>
              )}
            </article>
          );
        })}
      </div>
    </div>
  );
}
