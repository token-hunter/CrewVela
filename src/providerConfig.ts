import {
  createDefaultProviderMeta,
  providerMeta,
  type ProviderApiKeyField,
  type ProviderConfig,
  type ProviderMeta,
} from "./types";

export const MODEL_ROLE_FIELDS = [
  {
    role: "Sonnet",
    model: "ANTHROPIC_DEFAULT_SONNET_MODEL",
    display: "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    supportsOneM: true,
  },
  {
    role: "Opus",
    model: "ANTHROPIC_DEFAULT_OPUS_MODEL",
    display: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    supportsOneM: true,
  },
  {
    role: "Fable",
    model: "ANTHROPIC_DEFAULT_FABLE_MODEL",
    display: "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    supportsOneM: true,
  },
  {
    role: "Haiku",
    model: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    display: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    supportsOneM: false,
  },
  {
    role: "Subagent",
    model: "CLAUDE_CODE_SUBAGENT_MODEL",
    display: null,
    supportsOneM: true,
  },
] as const;

export type ProviderSettingToggleKey =
  | "hideAttribution"
  | "teammates"
  | "enableToolSearch"
  | "effortMax"
  | "disableAutoUpgrade";

export interface ProviderSettingToggles {
  hideAttribution: boolean;
  teammates: boolean;
  enableToolSearch: boolean;
  effortMax: boolean;
  disableAutoUpgrade: boolean;
}

const CONTROL_CHARACTERS = /[\x00-\x08\x0a-\x1f\x7f]/;
const HEADER_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

export const PROTECTED_REQUEST_HEADERS = new Set([
  "host",
  "content-length",
  "transfer-encoding",
  "connection",
  "proxy-authorization",
  "proxy-authenticate",
  "authorization",
  "x-api-key",
  "x-goog-api-key",
  "content-type",
  "accept-encoding",
  "x-forwarded-host",
  "x-forwarded-port",
  "x-forwarded-proto",
  "forwarded",
]);

function cloneObject(value: Record<string, unknown>): Record<string, unknown> {
  return structuredClone(value);
}

export function resolvedProviderMeta(provider: ProviderConfig): ProviderMeta {
  const meta = providerMeta(provider);
  return {
    ...meta,
    endpointCandidates: [...meta.endpointCandidates],
    requestHeaders: { ...meta.requestHeaders },
    requestBody: cloneObject(meta.requestBody),
    settings: cloneObject(meta.settings),
  };
}

export function cloneProviderConfig(provider: ProviderConfig): ProviderConfig {
  return {
    ...provider,
    env: { ...provider.env },
    meta: resolvedProviderMeta(provider),
  };
}

function toEnvValue(value: unknown): string | null {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return null;
}

export function providerToSettingsConfig(
  provider: ProviderConfig,
): Record<string, unknown> {
  const meta = resolvedProviderMeta(provider);
  const env: Record<string, string | number> = {
    ANTHROPIC_BASE_URL: provider.baseUrl,
    [meta.apiKeyField]: "",
    ...provider.env,
  };
  if (provider.timeoutMs != null) env.API_TIMEOUT_MS = String(provider.timeoutMs);
  if (provider.disableNonessentialTraffic) {
    env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = 1;
  }
  return { ...cloneObject(meta.settings), env };
}

export function applySettingsConfig(
  provider: ProviderConfig,
  config: Record<string, unknown>,
): ProviderConfig {
  const meta = resolvedProviderMeta(provider);
  const sourceEnv =
    config.env && typeof config.env === "object" && !Array.isArray(config.env)
      ? (config.env as Record<string, unknown>)
      : {};
  const apiKeyField: ProviderApiKeyField = Object.prototype.hasOwnProperty.call(
    sourceEnv,
    "ANTHROPIC_API_KEY",
  )
    ? "ANTHROPIC_API_KEY"
    : meta.apiKeyField;
  const baseUrl =
    typeof sourceEnv.ANTHROPIC_BASE_URL === "string"
      ? sourceEnv.ANTHROPIC_BASE_URL
      : provider.baseUrl;
  const timeoutValue = toEnvValue(sourceEnv.API_TIMEOUT_MS);
  const timeoutMs = timeoutValue
    ? Number.parseInt(timeoutValue.replace(/[^\d]/g, ""), 10) || null
    : null;
  const disableValue = sourceEnv.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC;
  const disableNonessentialTraffic =
    disableValue === 1 ||
    disableValue === true ||
    disableValue === "1" ||
    disableValue === "true";

  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(sourceEnv)) {
    if (
      key === "ANTHROPIC_BASE_URL" ||
      key === "ANTHROPIC_AUTH_TOKEN" ||
      key === "ANTHROPIC_API_KEY" ||
      key === "API_TIMEOUT_MS" ||
      key === "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
    ) {
      continue;
    }
    const normalized = toEnvValue(value);
    if (normalized != null) env[key] = normalized;
  }

  const settings = cloneObject(config);
  delete settings.env;
  return {
    ...provider,
    baseUrl,
    env,
    timeoutMs,
    disableNonessentialTraffic,
    meta: createDefaultProviderMeta({
      ...meta,
      apiKeyField,
      settings,
    }),
  };
}

export function settingToggles(provider: ProviderConfig): ProviderSettingToggles {
  const config = providerToSettingsConfig(provider);
  const env = config.env as Record<string, unknown>;
  const attribution = config.attribution as Record<string, unknown> | undefined;
  return {
    hideAttribution: attribution?.commit === "" && attribution?.pr === "",
    teammates:
      env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === "1" ||
      env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === 1,
    enableToolSearch:
      env.ENABLE_TOOL_SEARCH === "true" || env.ENABLE_TOOL_SEARCH === "1",
    effortMax: env.CLAUDE_CODE_EFFORT_LEVEL === "max",
    disableAutoUpgrade:
      env.DISABLE_AUTOUPDATER === "1" || env.DISABLE_AUTOUPDATER === 1,
  };
}

export function updateSettingToggle(
  provider: ProviderConfig,
  key: ProviderSettingToggleKey,
  checked: boolean,
): ProviderConfig {
  const next = cloneProviderConfig(provider);
  const meta = resolvedProviderMeta(next);
  if (key === "hideAttribution") {
    const settings = cloneObject(meta.settings);
    if (checked) settings.attribution = { commit: "", pr: "" };
    else delete settings.attribution;
    return { ...next, meta: { ...meta, settings } };
  }

  const envKey = {
    teammates: "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
    enableToolSearch: "ENABLE_TOOL_SEARCH",
    effortMax: "CLAUDE_CODE_EFFORT_LEVEL",
    disableAutoUpgrade: "DISABLE_AUTOUPDATER",
  }[key];
  const enabledValue = {
    teammates: "1",
    enableToolSearch: "true",
    effortMax: "max",
    disableAutoUpgrade: "1",
  }[key];
  if (checked) next.env[envKey] = enabledValue;
  else delete next.env[envKey];
  return next;
}

export function hasOneM(value: string): boolean {
  return /\[1m\]$/i.test(value.trim());
}

export function stripOneM(value: string): string {
  return value.trim().replace(/\[1m\]$/i, "");
}

export function setOneM(value: string, enabled: boolean): string {
  const base = stripOneM(value);
  return enabled && base ? `${base}[1m]` : base;
}

export function parseJsonObject(
  raw: string,
  label: string,
): { value?: Record<string, unknown>; error?: string } {
  if (!raw.trim()) return { value: {} };
  try {
    const value = JSON.parse(raw) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return { error: `${label}必须是 JSON 对象。` };
    }
    return { value: value as Record<string, unknown> };
  } catch (error) {
    return { error: `${label}格式错误：${String(error)}` };
  }
}

export function validateUserAgent(value: string): string | null {
  return CONTROL_CHARACTERS.test(value.trim())
    ? "User-Agent 不能包含换行等控制字符。"
    : null;
}

export function validateRequestHeaders(
  headers: Record<string, unknown>,
): { value?: Record<string, string>; error?: string } {
  const normalized: Record<string, string> = {};
  for (const [rawName, rawValue] of Object.entries(headers)) {
    const name = rawName.trim().toLowerCase();
    if (!name || !HEADER_NAME.test(name)) {
      return { error: `请求头名称“${rawName}”无效。` };
    }
    if (PROTECTED_REQUEST_HEADERS.has(name)) {
      return { error: `请求头“${rawName}”由 CrewVela 管理，不能覆盖。` };
    }
    if (typeof rawValue !== "string") {
      return { error: `请求头“${rawName}”的值必须是字符串。` };
    }
    if (CONTROL_CHARACTERS.test(rawValue)) {
      return { error: `请求头“${rawName}”包含控制字符。` };
    }
    if (Object.prototype.hasOwnProperty.call(normalized, name)) {
      return { error: `请求头“${rawName}”规范化后重复。` };
    }
    normalized[name] = rawValue;
  }
  return { value: normalized };
}
