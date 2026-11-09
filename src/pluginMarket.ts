export interface MarketPluginView {
  name: string;
  displayName: string | null;
  description: string;
  version: string | null;
  author: string | null;
  category: string | null;
  sourceDisplay: string;
  installable: boolean;
  note: string | null;
  compatibility?: PluginCompatibility | null;
}

export interface PluginCompatibility {
  format: string;
  status: "native" | "skill_compatible" | "needs_auth" | "unsupported";
  nativeCodex: boolean;
  nativeClaude: boolean;
  skillCompatible: boolean;
  components: string[];
  note: string;
  dependencies?: string[];
  license?: string | null;
}

export function compatibilityLabel(plugin: MarketPluginView): string {
  const compatibility = plugin.compatibility;
  if (!compatibility) return "组件未知 · 安装前检查";
  if (compatibility.status === "needs_auth") return "待授权 · 仅展示";
  if (compatibility.nativeCodex) return "Codex 原生整包";
  if (compatibility.nativeClaude) return "Claude 原生整包";
  if (compatibility.skillCompatible) return "Skill 兼容分发";
  return "尚未适配";
}

export function pluginModes(plugin: MarketPluginView, builtin: boolean): string[] {
  if (builtin) return ["skills"];
  if (!plugin.compatibility || plugin.compatibility.status === "needs_auth") return [];
  return [plugin.compatibility.skillCompatible && "skills", plugin.compatibility.nativeClaude && "native_claude", plugin.compatibility.nativeCodex && "native_codex"].filter((mode): mode is string => Boolean(mode));
}

export function pluginSupportsAgent(plugin: MarketPluginView, builtin: boolean, runtime: string): boolean {
  const modes = pluginModes(plugin, builtin);
  return (modes.includes("skills") && Boolean(PLUGIN_AGENTS.find((agent) => agent.id === runtime)?.support)) ||
    (modes.includes("native_codex") && runtime === "codex") || (modes.includes("native_claude") && runtime === "claude_code");
}

export interface MarketplaceView {
  id: string;
  source: string;
  sourceKind: "git" | "local" | string;
  ownerName: string;
  path: string;
  url: string | null;
  lastRefreshedAt: number | null;
  lastError: string | null;
  plugins: MarketPluginView[];
}

export interface InstalledPlugin {
  id: string;
  marketplaceId: string;
  name: string;
  version: string | null;
  description: string;
  cachePath: string;
  skillAssetIds: string[];
  skillNames: string[];
  mcpServerNames: string[];
  hasHooks: boolean;
  hasAgents: boolean;
  installedAt: number;
  targets?: PluginTarget[];
}

export interface PluginTarget {
  id: string;
  runtime: string;
  scope: string;
  projectPath: string | null;
  deploymentIds: string[];
  status: "installed" | "failed" | "needs_attention";
  lastError: string | null;
  native?: { selector: string; scope: string; installPath: string | null; verified: boolean; runtime?: string | null } | null;
}

export const PLUGIN_AGENTS = [
  { id: "claude_code", label: "Claude Code", support: true, kind: "原生插件", hint: "本次分发 Skill；原生插件组件未安装" },
  { id: "codex", label: "Codex", support: true, kind: "原生插件", hint: "本次分发 Skill；原生插件组件未安装" },
  { id: "kimi_code", label: "Kimi Code", support: true, kind: "原生插件", hint: "本次分发 Skill；需核对插件格式" },
  { id: "pi", label: "Pi", support: true, kind: "扩展包", hint: "仅分发 Skill，不转换 Pi 扩展" },
  { id: "opencode", label: "OpenCode", support: true, kind: "JS / TS 插件", hint: "仅分发 Skill，不转换 JS / TS 插件" },
  { id: "zcode", label: "ZCode", support: false, kind: "原生插件", hint: "当前安装适配待验证" },
  { id: "deepseek_harness", label: "DeepSeek Harness", support: false, kind: "独立插件体系", hint: "当前安装适配待验证" },
];

export const PLUGIN_CATEGORIES = ["全部", "编程与开发", "办公与文档", "研究与检索", "设计与创作", "数据与自动化"];
export function pluginCategory(value: string | null): string {
  if (PLUGIN_CATEGORIES.includes(value ?? "")) return value!;
  if (/design|creative/.test(value ?? "")) return "设计与创作";
  if (/productivity|document|office/.test(value ?? "")) return "办公与文档";
  if (/database|automation|monitoring|data/.test(value ?? "")) return "数据与自动化";
  if (/research|search/.test(value ?? "")) return "研究与检索";
  return "编程与开发";
}

export function agentLabel(runtime: string): string {
  return PLUGIN_AGENTS.find((agent) => agent.id === runtime)?.label ?? runtime;
}

export interface PluginMarketSnapshot {
  marketplaces: MarketplaceView[];
  installed: InstalledPlugin[];
  selectionConfigured?: boolean;
  selectedPluginIds?: string[];
  installedCatalog?: MarketPluginEntry[];
}

export interface PluginCandidateSnapshot {
  marketplaces: MarketplaceView[];
  selectionConfigured: boolean;
  selectedPluginIds: string[];
  componentHints: Record<string, string>;
}

export function selectedCatalog(markets: MarketplaceView[], ids: string[]): MarketplaceView[] {
  const selected = new Set(ids);
  return markets.map((market) => ({ ...market, plugins: market.plugins.filter((plugin) => selected.has(installedKey(market.id, plugin.name))) }));
}

export function validSelection(markets: MarketplaceView[], ids: string[]): string[] {
  const valid = new Set(filterMarketPlugins(markets, null, "").map(({ market, plugin }) => installedKey(market.id, plugin.name)));
  return [...new Set(ids)].filter((id) => valid.has(id));
}

export interface ComponentBadge {
  label: string;
  tone: "skill" | "mcp" | "claude-only";
}

/** 已安装插件的组件徽章（hooks/agents 是 claude/codex 专属组件，仅识别展示）。 */
export function componentBadges(plugin: {
  skillNames: string[];
  mcpServerNames: string[];
  hasHooks: boolean;
  hasAgents: boolean;
}): ComponentBadge[] {
  const badges: ComponentBadge[] = [];
  if (plugin.skillNames.length > 0) {
    badges.push({ label: `Skills ×${plugin.skillNames.length}`, tone: "skill" });
  }
  if (plugin.mcpServerNames.length > 0) {
    badges.push({ label: `MCP ×${plugin.mcpServerNames.length}`, tone: "mcp" });
  }
  if (plugin.hasHooks || plugin.hasAgents) {
    const parts = [
      plugin.hasHooks ? "Hooks" : null,
      plugin.hasAgents ? "Agents" : null,
    ].filter(Boolean);
    badges.push({ label: `${parts.join("/")}（未分发）`, tone: "claude-only" });
  }
  return badges;
}

/** 市场源输入校验；返回中文错误或 null（与后端 classify_source 规则一致）。 */
export function validateMarketSource(raw: string): string | null {
  const source = raw.trim();
  if (!source) return "市场源不能为空";
  if (/^[A-Za-z]:[\\/]/.test(source) || source.startsWith("\\\\") || source.startsWith("/")) return null;
  if (source.includes("://") || source.startsWith("git@")) return null;
  if (/^[\w.-]+\/[\w.-]+$/.test(source)) return null;
  return "无法识别的市场源：支持本地绝对路径、git URL 或 owner/repo";
}

/** 插件显示名：displayName 优先，回退 name。 */
export function pluginTitle(plugin: MarketPluginView): string {
  return plugin.displayName?.trim() || plugin.name;
}

export function installedKey(marketplaceId: string, pluginName: string): string {
  return `${pluginName}@${marketplaceId}`;
}

export interface MarketPluginEntry {
  market: MarketplaceView;
  plugin: MarketPluginView;
}

/**
 * 按选中市场与关键词过滤插件。关键词不区分大小写，匹配
 * 标题/名称/描述/分类/作者；结果保持市场与市场内的原有顺序。
 */
export function filterMarketPlugins(
  marketplaces: MarketplaceView[],
  selectedMarketId: string | null,
  query: string,
): MarketPluginEntry[] {
  const keyword = query.trim().toLowerCase();
  const entries: MarketPluginEntry[] = [];
  for (const market of marketplaces) {
    if (selectedMarketId && market.id !== selectedMarketId) continue;
    for (const plugin of market.plugins) {
      if (!keyword) {
        entries.push({ market, plugin });
        continue;
      }
      const haystack = [
        pluginTitle(plugin),
        plugin.name,
        plugin.description,
        plugin.category ?? "",
        plugin.author ?? "",
      ]
        .join("\n")
        .toLowerCase();
      if (haystack.includes(keyword)) entries.push({ market, plugin });
    }
  }
  return entries;
}

/** UI 预览（?ui-preview=1）假数据。 */
export const PLUGIN_MARKET_PREVIEW: PluginMarketSnapshot = {
  marketplaces: [
    {
      id: "claude-community",
      source: "anthropics/claude-plugins-community",
      sourceKind: "git",
      ownerName: "Anthropic",
      path: "…/plugins/marketplaces/claude-community",
      url: "https://github.com/anthropics/claude-plugins-community.git",
      lastRefreshedAt: 12,
      lastError: null,
      plugins: [
        {
          name: "pdf",
          displayName: "PDF 工具箱",
          description: "读取、编辑和处理 PDF 文档的技能集",
          version: "1.4.0",
          author: "Anthropic",
          category: "productivity",
          sourceDisplay: "./plugins/pdf",
          installable: true,
          note: null,
        },
        {
          name: "frontend-design",
          displayName: null,
          description: "生成高质量前端界面并回归检查",
          version: "2.0.1",
          author: "Community",
          category: "web-dev",
          sourceDisplay: "https://github.com/example/skills.git@main",
          installable: true,
          note: null,
        },
        {
          name: "cloud-ops",
          displayName: null,
          description: "云资源操作（npm 分发）",
          version: null,
          author: null,
          category: null,
          sourceDisplay: "npm",
          installable: false,
          note: "暂不支持的来源类型",
        },
      ],
    },
    {
      id: "zcode-official",
      source: "F:\\plugins\\zcode-official",
      sourceKind: "local",
      ownerName: "ZCode Desktop",
      path: "…/plugins/marketplaces/zcode-official",
      url: null,
      lastRefreshedAt: 40,
      lastError: null,
      plugins: [
        {
          name: "browser-use",
          displayName: "浏览器自动化",
          description: "接管浏览器完成网页操作、巡检与回归测试",
          version: "0.9.3",
          author: "ZCode Desktop",
          category: "automation",
          sourceDisplay: "./plugins/browser-use",
          installable: true,
          note: null,
        },
        {
          name: "postgres-helper",
          displayName: null,
          description: "PostgreSQL 查询编写与性能优化建议",
          version: "1.1.0",
          author: "Community",
          category: "database",
          sourceDisplay: "./plugins/postgres-helper",
          installable: true,
          note: null,
        },
        {
          name: "deploy-notify",
          displayName: null,
          description: "部署完成后向多渠道推送结果通知",
          version: null,
          author: "ZCode Desktop",
          category: "devops",
          sourceDisplay: "./plugins/deploy-notify",
          installable: true,
          note: null,
        },
      ],
    },
  ],
  installed: [
    {
      id: "pdf@claude-community",
      marketplaceId: "claude-community",
      name: "pdf",
      version: "1.4.0",
      description: "读取、编辑和处理 PDF 文档的技能集",
      cachePath: "…/plugins/cache/claude-community/pdf",
      skillAssetIds: ["asset-1", "asset-2"],
      skillNames: ["pdf-read", "pdf-edit"],
      mcpServerNames: ["pdf-server"],
      hasHooks: false,
      hasAgents: true,
      installedAt: 20,
    },
    {
      id: "frontend-design@claude-community",
      marketplaceId: "claude-community",
      name: "frontend-design",
      version: "2.0.1",
      description: "生成高质量前端界面并回归检查",
      cachePath: "…/plugins/cache/claude-community/frontend-design",
      skillAssetIds: ["asset-3"],
      skillNames: ["frontend-design"],
      mcpServerNames: [],
      hasHooks: false,
      hasAgents: false,
      installedAt: 30,
    },
  ],
};

/** Read-only preview metadata matches the bundled, pinned catalogue. */
export const CURATED_PLUGIN_PREVIEW: PluginMarketSnapshot = {
  selectionConfigured: true,
  selectedPluginIds: ["frontend-design", "webapp-testing", "pdf", "docx", "pptx", "xlsx"].map((name) => name + "@crewvela精选"),
  marketplaces: [{ id: "crewvela精选", source: "https://github.com/anthropics/skills", sourceKind: "builtin", ownerName: "Anthropic", path: "", url: "https://github.com/anthropics/skills", lastRefreshedAt: null, lastError: null,
    plugins: [
      ["frontend-design", "前端设计", "构建有辨识度的网页与组件，提供设计和实现方法。", "设计与创作"],
      ["webapp-testing", "网页测试", "使用 Playwright 检查本地网页交互、截图与浏览器日志。", "编程与开发"],
      ["pdf", "PDF 处理", "读取、合并、拆分和生成 PDF，处理表格与表单。", "办公与文档"],
      ["docx", "Word 文档", "创建、编辑和分析 Word 文档，保留内容与格式。", "办公与文档"],
      ["pptx", "演示文稿", "创建、调整和分析 PowerPoint 演示文稿。", "办公与文档"],
      ["xlsx", "电子表格", "创建、编辑和分析表格，处理公式与数据。", "数据与自动化"],
    ].map(([name, displayName, description, category]) => ({ name, displayName, description, category, author: "Anthropic", version: "dbd4588f9e10", sourceDisplay: `anthropics/skills / skills/${name} @ dbd4588f9e1033efb41dad4bef2f7947c8993d44`, installable: true, note: "精选 Skill 包" })) }],
  installed: ["frontend-design", "pdf", "webapp-testing"].map((name, index) => ({ id: `${name}@crewvela精选`, marketplaceId: "crewvela精选", name, version: "dbd4588f9e10", description: "只读预览示例", cachePath: "", skillAssetIds: [name], skillNames: [name], mcpServerNames: [], hasHooks: false, hasAgents: false, installedAt: 0,
    targets: [{ id: `${index === 0 ? "claude_code" : "codex"}|global|`, runtime: index === 0 ? "claude_code" : "codex", scope: "global", projectPath: null, deploymentIds: [name], status: index === 2 ? "needs_attention" : "installed", lastError: index === 2 ? "示例：Skill 文件需要检查" : null }] })),
};
