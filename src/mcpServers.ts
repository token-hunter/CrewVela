export type McpTransport = "stdio" | "sse" | "http";

export interface McpServerConfig {
  id: string;
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  env: Record<string, string>;
  url: string | null;
  headers: Record<string, string>;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface McpUpsertInput {
  id: string | null;
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  env: Record<string, string>;
  url: string | null;
  headers: Record<string, string>;
  enabled: boolean;
}

export interface McpToolInfo {
  name: string;
  description: string | null;
}

export interface McpProbeReport {
  id: string;
  ok: boolean;
  status: "connected" | "failed";
  tools: McpToolInfo[];
  error: string | null;
  probedAt: number;
}

export const TRANSPORT_LABELS: Record<McpTransport, string> = {
  stdio: "STDIO",
  sse: "SSE",
  http: "HTTP",
};

export const TRANSPORT_OPTIONS: Array<{ id: McpTransport; label: string; hint: string }> = [
  { id: "stdio", label: "STDIO", hint: "本地进程，npx / uvx 等命令启动" },
  { id: "http", label: "HTTP", hint: "Streamable HTTP 端点" },
  { id: "sse", label: "SSE", hint: "Server-Sent Events 端点" },
];

/** 表单草稿：文本域与结构化字段之间的转换都在这里，便于单测。 */
export interface McpServerDraft {
  id: string | null;
  name: string;
  transport: McpTransport;
  command: string;
  argsText: string;
  envText: string;
  url: string;
  headersText: string;
  enabled: boolean;
}

export const EMPTY_DRAFT: McpServerDraft = {
  id: null,
  name: "",
  transport: "stdio",
  command: "",
  argsText: "",
  envText: "",
  url: "",
  headersText: "",
  enabled: true,
};

export function draftFromServer(server: McpServerConfig): McpServerDraft {
  return {
    id: server.id,
    name: server.name,
    transport: server.transport,
    command: server.command ?? "",
    argsText: server.args.map((arg) => arg && arg.trim() === arg && !/[\r\n]/.test(arg) && !arg.startsWith('"') ? arg : JSON.stringify(arg)).join("\n"),
    envText: formatEntries(server.env, "="),
    url: server.url ?? "",
    headersText: formatEntries(server.headers, ": "),
    enabled: server.enabled,
  };
}

export function formatEntries(
  record: Record<string, string>,
  separator: string,
): string {
  return Object.entries(record)
    .map(([key, value]) => `${key}${separator}${value}`)
    .join("\n");
}

/** KEY=VALUE（env）或 KEY: VALUE（headers）逐行解析；分隔符取首个，值里允许再出现。 */
export function parseEntries(
  text: string,
  separator: string,
): { entries: Record<string, string>; error: string | null } {
  const entries: Record<string, string> = {};
  for (const [index, rawLine] of text.split(/\r?\n/).entries()) {
    const line = rawLine.trim();
    if (!line) continue;
    const at = line.indexOf(separator.trimEnd());
    if (at <= 0) {
      return {
        entries,
        error: `第 ${index + 1} 行格式无效，应形如 KEY${separator}VALUE：${line}`,
      };
    }
    const key = line.slice(0, at).trim();
    const value = line.slice(at + 1).trim();
    if (!key) {
      return { entries, error: `第 ${index + 1} 行的名称为空` };
    }
    entries[key] = value;
  }
  return { entries, error: null };
}

export function parseArgsText(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      if (line.startsWith('"')) {
        try { const value: unknown = JSON.parse(line); if (typeof value === "string") return value; }
        catch { /* Existing literal arguments remain valid. */ }
      }
      return line;
    });
}

/** 保存前的校验；返回中文错误文案或 null。 */
export function validateDraft(
  draft: McpServerDraft,
  existingServers: McpServerConfig[],
): string | null {
  const name = draft.name.trim();
  if (!name) return "服务器名称不能为空";
  if (name.length > 64) return "服务器名称不能超过 64 个字符";
  if (
    existingServers.some(
      (server) => server.name === name && server.id !== draft.id,
    )
  ) {
    return `已存在同名 MCP 服务器：${name}`;
  }
  if (draft.transport === "stdio") {
    if (!draft.command.trim()) return "STDIO 服务器必须填写启动命令";
  } else {
    const url = draft.url.trim();
    if (!url) return `${TRANSPORT_LABELS[draft.transport]} 服务器必须填写 URL`;
    if (!/^https?:\/\//.test(url)) return "URL 必须以 http:// 或 https:// 开头";
  }
  if (draft.transport === "stdio" && parseEntries(draft.envText, "=").error) return parseEntries(draft.envText, "=").error;
  if (draft.transport !== "stdio" && parseEntries(draft.headersText, ": ").error) {
    return parseEntries(draft.headersText, ": ").error;
  }
  return null;
}

export function draftToUpsertInput(draft: McpServerDraft): McpUpsertInput {
  const env = parseEntries(draft.envText, "=").entries;
  const headers = parseEntries(draft.headersText, ": ").entries;
  return {
    id: draft.id,
    name: draft.name.trim(),
    transport: draft.transport,
    command: draft.transport === "stdio" ? draft.command.trim() : null,
    args: draft.transport === "stdio" ? parseArgsText(draft.argsText) : [],
    env: draft.transport === "stdio" ? env : {},
    url: draft.transport === "stdio" ? null : draft.url.trim(),
    headers: draft.transport === "stdio" ? {} : headers,
    enabled: draft.enabled,
  };
}

/** UI 预览（?ui-preview=1）用的假数据。 */
export const MCP_PREVIEW_SERVERS: McpServerConfig[] = [
  {
    id: "preview-1",
    name: "document-knowledge-base",
    transport: "http",
    command: null,
    args: [],
    env: {},
    url: "https://mcp.example.com/knowledge",
    headers: { Authorization: "Bearer preview" },
    enabled: true,
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: "preview-2",
    name: "filesystem",
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", "."],
    env: {},
    url: null,
    headers: {},
    enabled: true,
    createdAt: 2,
    updatedAt: 2,
  },
  {
    id: "preview-3",
    name: "legacy-sse",
    transport: "sse",
    command: null,
    args: [],
    env: {},
    url: "https://mcp.example.com/sse",
    headers: {},
    enabled: false,
    createdAt: 3,
    updatedAt: 3,
  },
];

export const MCP_PREVIEW_REPORTS: McpProbeReport[] = [
  {
    id: "preview-1",
    ok: true,
    status: "connected",
    tools: [
      { name: "search", description: "全文检索知识库文档" },
      { name: "read_document", description: "按 ID 读取文档正文" },
    ],
    error: null,
    probedAt: 4,
  },
  {
    id: "preview-2",
    ok: false,
    status: "failed",
    tools: [],
    error: "连接超时（45 秒内未完成握手）",
    probedAt: 5,
  },
];
