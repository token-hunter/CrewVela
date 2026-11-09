import {
  createDefaultProviderMeta,
  type ProviderConfig,
  type ProviderMeta,
} from "./types";

export type ProviderPresetId =
  | "kimi"
  | "glm"
  | "deepseek"
  | "minimax"
  | "custom";

export interface ProviderPreset {
  id: ProviderPresetId;
  label: string;
  description: string;
  aliases: string[];
  tone: ProviderPresetId;
  template: ProviderConfig;
}

const PRESET_ORDER: ProviderPresetId[] = [
  "kimi",
  "glm",
  "deepseek",
  "minimax",
  "custom",
];

const PRESET_COPY: Record<
  ProviderPresetId,
  Pick<ProviderPreset, "label" | "description" | "aliases">
> = {
  kimi: {
    label: "Kimi For Coding",
    description: "Kimi Coding Plan，已配置 256K 上下文和角色模型映射",
    aliases: ["moonshot", "kimi", "月之暗面"],
  },
  glm: {
    label: "智谱 GLM",
    description: "智谱 Claude Code 兼容端点和 GLM 角色模型映射",
    aliases: ["zhipu", "glm", "bigmodel", "智谱"],
  },
  deepseek: {
    label: "DeepSeek",
    description: "DeepSeek Anthropic 兼容端点及独立模型列表地址",
    aliases: ["deepseek", "深度求索"],
  },
  minimax: {
    label: "MiniMax",
    description: "MiniMax Coding Plan，已配置长超时与非必要流量开关",
    aliases: ["minimax", "稀宇"],
  },
  custom: {
    label: "自定义配置",
    description: "手动配置其他 Claude Code 兼容模型服务",
    aliases: ["custom", "自定义"],
  },
};

function meta(patch: Partial<ProviderMeta>): ProviderMeta {
  return createDefaultProviderMeta(patch);
}

const PRESET_TEMPLATES: Record<ProviderPresetId, ProviderConfig> = {
  kimi: {
    id: "kimi",
    name: "Kimi For Coding",
    baseUrl: "https://api.kimi.com/coding/",
    authTokenRef: "kimi",
    env: {
      ANTHROPIC_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-for-coding",
      CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
      CLAUDE_CODE_AUTO_COMPACT_WINDOW: "262144",
    },
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
    meta: meta({
      presetId: "kimi",
      schemaVersion: 1,
      websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
      apiKeyUrl: "https://www.kimi.com/code/?aff=cc-switch",
      icon: "kimi",
      iconColor: "#6366F1",
      category: "cn_official",
      endpointCandidates: ["https://api.kimi.com/coding"],
    }),
  },
  glm: {
    id: "glm",
    name: "智谱 GLM",
    baseUrl: "https://open.bigmodel.cn/api/anthropic",
    authTokenRef: "glm",
    env: {
      ANTHROPIC_MODEL: "glm-5.1",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5.1",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.1",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5.1",
    },
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
    meta: meta({
      presetId: "glm",
      schemaVersion: 1,
      websiteUrl: "https://open.bigmodel.cn",
      apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
      icon: "zhipu",
      iconColor: "#0F62FE",
      category: "cn_official",
      endpointCandidates: ["https://open.bigmodel.cn/api/anthropic"],
    }),
  },
  deepseek: {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com/anthropic",
    authTokenRef: "deepseek",
    env: {
      ANTHROPIC_MODEL: "deepseek-v4-pro",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-flash",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-pro",
    },
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
    meta: meta({
      presetId: "deepseek",
      schemaVersion: 1,
      websiteUrl: "https://platform.deepseek.com",
      apiKeyUrl: "https://platform.deepseek.com/api_keys",
      icon: "deepseek",
      iconColor: "#1E88E5",
      category: "cn_official",
      modelsUrl: "https://api.deepseek.com/models",
      endpointCandidates: ["https://api.deepseek.com/anthropic"],
    }),
  },
  minimax: {
    id: "minimax",
    name: "MiniMax",
    baseUrl: "https://api.minimaxi.com/anthropic",
    authTokenRef: "minimax",
    env: {
      ANTHROPIC_MODEL: "MiniMax-M2.7",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M2.7",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M2.7",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M2.7",
    },
    timeoutMs: 3_000_000,
    disableNonessentialTraffic: true,
    builtin: true,
    meta: meta({
      presetId: "minimax",
      schemaVersion: 1,
      websiteUrl: "https://platform.minimaxi.com",
      apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
      icon: "minimax",
      iconColor: "#FF6B6B",
      category: "cn_official",
      endpointCandidates: ["https://api.minimaxi.com/anthropic"],
    }),
  },
  custom: {
    id: "custom",
    name: "自定义模型服务",
    baseUrl: "",
    authTokenRef: "custom",
    env: {},
    timeoutMs: null,
    disableNonessentialTraffic: false,
    builtin: true,
    meta: meta({ presetId: "custom", schemaVersion: 1, category: "custom" }),
  },
};

function cloneMeta(value: ProviderMeta | undefined): ProviderMeta {
  const current = createDefaultProviderMeta(value ?? {});
  return {
    ...current,
    endpointCandidates: [...current.endpointCandidates],
    requestHeaders: { ...current.requestHeaders },
    requestBody: structuredClone(current.requestBody),
    settings: structuredClone(current.settings),
  };
}

function cloneProvider(provider: ProviderConfig): ProviderConfig {
  return {
    ...provider,
    env: { ...provider.env },
    meta: cloneMeta(provider.meta),
  };
}

export function providerPresetId(
  provider: Pick<ProviderConfig, "id" | "name" | "meta">,
): ProviderPresetId {
  const presetId = provider.meta?.presetId;
  if (PRESET_ORDER.includes(presetId as ProviderPresetId)) {
    return presetId as ProviderPresetId;
  }
  const identity = `${provider.id} ${provider.name}`.toLowerCase();
  if (identity.includes("kimi")) return "kimi";
  if (identity.includes("glm") || identity.includes("zhipu")) return "glm";
  if (identity.includes("deepseek")) return "deepseek";
  if (identity.includes("minimax")) return "minimax";
  return "custom";
}

/** 新建配置只读取稳定预设，不读取或覆盖用户已经编辑过的实例。 */
export function buildProviderPresets(): ProviderPreset[] {
  return PRESET_ORDER.map((id) => ({
    id,
    ...PRESET_COPY[id],
    tone: id,
    template: cloneProvider(PRESET_TEMPLATES[id]),
  }));
}

function nextProviderId(baseId: ProviderPresetId, providers: ProviderConfig[]) {
  const ids = new Set(providers.map((provider) => provider.id));
  if (!ids.has(baseId)) return baseId;
  let suffix = 2;
  while (ids.has(`${baseId}-${suffix}`)) suffix += 1;
  return `${baseId}-${suffix}`;
}

export function createProviderFromPreset(
  preset: ProviderPreset,
  providers: ProviderConfig[],
): ProviderConfig {
  const id = nextProviderId(preset.id, providers);
  const sameKindCount = providers.filter(
    (provider) => providerPresetId(provider) === preset.id,
  ).length;
  const template = cloneProvider(preset.template);
  const name =
    preset.id === "custom"
      ? "自定义模型服务"
      : sameKindCount > 0
        ? `${template.name} ${sameKindCount + 1}`
        : template.name;

  return {
    ...template,
    id,
    name,
    authTokenRef: id,
    builtin: false,
  };
}
