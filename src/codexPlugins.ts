export interface CodexPlugin {
  id: string;
  name: string;
  displayName: string;
  description: string;
  marketplace: string;
}
const pluginId = /^[A-Za-z0-9._-]+@[A-Za-z0-9._-]+$/;
export function parseCodexPlugins(value: unknown): { plugins: CodexPlugin[]; errors: string[] } {
  const catalog = value as { marketplaces?: { name?: string; plugins?: Record<string, unknown>[] }[]; marketplaceLoadErrors?: { message?: string }[] } | null;
  const plugins: CodexPlugin[] = [];
  for (const market of Array.isArray(catalog?.marketplaces) ? catalog.marketplaces : []) {
    for (const plugin of Array.isArray(market.plugins) ? market.plugins : []) {
      if (typeof plugin.id !== "string" || !pluginId.test(plugin.id) || plugin.installed !== true || plugin.enabled !== true || plugin.availability !== "AVAILABLE") continue;
      const ui = plugin.interface as { displayName?: string; shortDescription?: string } | undefined;
      plugins.push({ id: plugin.id, name: typeof plugin.name === "string" ? plugin.name : plugin.id.split("@")[0], displayName: typeof ui?.displayName === "string" ? ui.displayName : String(plugin.name || plugin.id), description: typeof ui?.shortDescription === "string" ? ui.shortDescription : "", marketplace: typeof market.name === "string" ? market.name : "" });
    }
  }
  return { plugins: [...new Map(plugins.map(plugin => [plugin.id, plugin])).values()], errors: (Array.isArray(catalog?.marketplaceLoadErrors) ? catalog.marketplaceLoadErrors : []).map(error => error.message || "部分插件目录加载失败") };
}
export const previewCodexPlugins: CodexPlugin[] = [
  { id: "openai-developers@openai-curated-remote", name: "openai-developers", displayName: "OpenAI Developers", description: "构建 AI 应用与智能体", marketplace: "openai-curated-remote" },
  { id: "figma@openai-curated-remote", name: "figma", displayName: "Figma", description: "创建设计并交付代码", marketplace: "openai-curated-remote" },
  { id: "product-design@openai-curated-remote", name: "product-design", displayName: "Product Design", description: "探索产品想法并制作原型", marketplace: "openai-curated-remote" },
];
