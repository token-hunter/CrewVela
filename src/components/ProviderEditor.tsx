import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModelCombobox } from "./ui/ModelCombobox";
import { Switch } from "./ui/Switch";
import { Modal } from "./ui/Modal";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowLeft";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CheckCircleIcon } from "@phosphor-icons/react/dist/csr/CheckCircle";
import { DownloadSimpleIcon } from "@phosphor-icons/react/dist/csr/DownloadSimple";
import { EyeIcon } from "@phosphor-icons/react/dist/csr/Eye";
import { EyeSlashIcon } from "@phosphor-icons/react/dist/csr/EyeSlash";
import { GaugeIcon } from "@phosphor-icons/react/dist/csr/Gauge";
import { KeyIcon } from "@phosphor-icons/react/dist/csr/Key";
import { SlidersHorizontalIcon } from "@phosphor-icons/react/dist/csr/SlidersHorizontal";
import { WarningCircleIcon } from "@phosphor-icons/react/dist/csr/WarningCircle";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import {
  applySettingsConfig,
  cloneProviderConfig,
  hasOneM,
  MODEL_ROLE_FIELDS,
  parseJsonObject,
  providerToSettingsConfig,
  resolvedProviderMeta,
  setOneM,
  settingToggles,
  stripOneM,
  updateSettingToggle,
  validateRequestHeaders,
  validateUserAgent,
} from "../providerConfig";
import {
  buildProviderPresets,
  createProviderFromPreset,
  providerPresetId,
  type ProviderPreset,
  type ProviderPresetId,
} from "../providerPresets";
import type { ProviderConfig, ProviderMeta } from "../types";

interface Props {
  provider: ProviderConfig;
  providers: ProviderConfig[];
  hasKey: boolean;
  onSave: (provider: ProviderConfig, key?: string) => void | Promise<void>;
  onClose: () => void;
}

interface EndpointProbe {
  url: string;
  latencyMs: number | null;
  status: number | null;
  reachable: boolean;
  error: string | null;
}

interface FetchedModel {
  id: string;
  ownedBy: string | null;
}

interface StatusMessage {
  kind: "ok" | "err";
  text: string;
}

const USER_AGENT_PRESETS = [
  { label: "跟随 Claude Code", value: "" },
  { label: "Claude Code", value: "claude-cli/2.1.0" },
  { label: "浏览器 Chrome", value: "Mozilla/5.0 Chrome/131.0" },
] as const;

const KNOWN_ENV_KEYS = new Set([
  "ANTHROPIC_MODEL",
  ...MODEL_ROLE_FIELDS.flatMap((field) =>
    field.display ? [field.model, field.display] : [field.model],
  ),
  "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
  "ENABLE_TOOL_SEARCH",
  "CLAUDE_CODE_EFFORT_LEVEL",
  "DISABLE_AUTOUPDATER",
]);

function isValidEndpoint(value: string) {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function prettyJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}

function initialDraft(provider: ProviderConfig) {
  return cloneProviderConfig(provider);
}

export function ProviderEditor({
  provider,
  providers,
  hasKey,
  onSave,
  onClose,
}: Props) {
  const isNewProvider = provider.id.trim().length === 0;
  const presets = useMemo(() => buildProviderPresets(), []);
  const [selectedPresetId, setSelectedPresetId] =
    useState<ProviderPresetId | null>(null);
  const [draft, setDraft] = useState<ProviderConfig>(() => initialDraft(provider));
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(!isNewProvider);
  const [rawSettings, setRawSettings] = useState(() =>
    prettyJson(providerToSettingsConfig(initialDraft(provider))),
  );
  const [rawSettingsDirty, setRawSettingsDirty] = useState(false);
  const [headersJson, setHeadersJson] = useState(() =>
    prettyJson(resolvedProviderMeta(initialDraft(provider)).requestHeaders),
  );
  const [bodyJson, setBodyJson] = useState(() =>
    prettyJson(resolvedProviderMeta(initialDraft(provider)).requestBody),
  );
  const [models, setModels] = useState<FetchedModel[]>([]);
  const [networkBusy, setNetworkBusy] = useState<"probe" | "models" | null>(null);
  const [networkMessage, setNetworkMessage] = useState<StatusMessage | null>(null);
  const [modelsMessage, setModelsMessage] = useState<StatusMessage | null>(null);
  const [saving, setSaving] = useState(false);
  const saveInFlight = useRef(false);
  const networkRequest = useRef(0);
  const networkInputs = useRef({ draft, apiKey, rawSettings, headersJson });
  networkInputs.current = { draft, apiKey, rawSettings, headersJson };
  useEffect(() => () => { ++networkRequest.current; }, []);
  const networkContext = () => {
    const request = ++networkRequest.current;
    const inputs = networkInputs.current;
    return () => request === networkRequest.current && networkInputs.current.draft === inputs.draft
      && networkInputs.current.apiKey === inputs.apiKey && networkInputs.current.rawSettings === inputs.rawSettings
      && networkInputs.current.headersJson === inputs.headersJson;
  };
  const [error, setError] = useState<string | null>(null);

  const selectedPreset =
    presets.find((preset) => preset.id === selectedPresetId) ?? null;
  const activePresetId = selectedPreset?.id ?? providerPresetId(draft);
  const shouldChoosePreset = isNewProvider && selectedPreset === null;
  const meta = resolvedProviderMeta(draft);
  const toggles = settingToggles(draft);
  const otherEnvEntries = Object.entries(draft.env).filter(
    ([key]) => !KNOWN_ENV_KEYS.has(key),
  );
  const modelOptions = useMemo(
    () => models.map((model) => ({ value: model.id, hint: model.ownedBy })),
    [models],
  );
  // 内置预设的默认模板：编辑既有服务时对比“用户层 vs 预设默认”，差异计数实时跟随草稿。
  const presetDefault = useMemo(() => {
    if (isNewProvider) return null;
    const presetId = providerPresetId(provider);
    if (presetId === "custom") return null;
    return presets.find((preset) => preset.id === presetId) ?? null;
  }, [isNewProvider, provider, presets]);
  const customizationCount = useMemo(() => {
    if (!presetDefault) return null;
    let count = 0;
    if (draft.baseUrl.trim() !== presetDefault.template.baseUrl) count++;
    const envKeys = new Set([
      "ANTHROPIC_MODEL",
      ...MODEL_ROLE_FIELDS.flatMap((field) =>
        field.display ? [field.model, field.display] : [field.model],
      ),
    ]);
    for (const key of envKeys) {
      if ((draft.env[key] ?? "") !== (presetDefault.template.env[key] ?? "")) count++;
    }
    if ((draft.timeoutMs ?? null) !== (presetDefault.template.timeoutMs ?? null)) count++;
    return count;
  }, [draft, presetDefault]);
  const effectiveModel = stripOneM(draft.env.ANTHROPIC_MODEL ?? "");

  const replaceDraft = (next: ProviderConfig, syncRaw = true) => {
    setError(null);
    setDraft(next);
    if (syncRaw) {
      setRawSettings(prettyJson(providerToSettingsConfig(next)));
      setRawSettingsDirty(false);
    }
  };

  const update = (patch: Partial<ProviderConfig>) =>
    replaceDraft({ ...draft, ...patch });

  const updateMeta = (patch: Partial<ProviderMeta>) =>
    replaceDraft({ ...draft, meta: { ...meta, ...patch } });

  const updateEnv = (key: string, value: string) => {
    const env = { ...draft.env };
    if (value) env[key] = value;
    else delete env[key];
    replaceDraft({ ...draft, env });
  };

  const choosePreset = (preset: ProviderPreset) => {
    const next = createProviderFromPreset(preset, providers);
    const nextMeta = resolvedProviderMeta(next);
    setSelectedPresetId(preset.id);
    replaceDraft(next);
    setApiKey("");
    setHeadersJson(prettyJson(nextMeta.requestHeaders));
    setBodyJson(prettyJson(nextMeta.requestBody));
    setModels([]);
    setNetworkMessage(null);
    setModelsMessage(null);
    setAdvancedOpen(preset.id === "custom");
  };

  const returnToPresets = () => {
    const next = initialDraft(provider);
    setSelectedPresetId(null);
    replaceDraft(next);
    setHeadersJson(prettyJson(resolvedProviderMeta(next).requestHeaders));
    setBodyJson(prettyJson(resolvedProviderMeta(next).requestBody));
    setApiKey("");
    setModels([]);
    setNetworkMessage(null);
    setModelsMessage(null);
    setAdvancedOpen(false);
  };

  const applyRawSettings = () => {
    const parsed = parseJsonObject(rawSettings, "完整配置");
    if (parsed.error || !parsed.value) {
      setError(parsed.error ?? "完整配置不能为空。");
      return false;
    }
    const next = applySettingsConfig(draft, parsed.value);
    setDraft(next);
    setRawSettings(prettyJson(providerToSettingsConfig(next)));
    setRawSettingsDirty(false);
    setError(null);
    return true;
  };

  const openExternal = (url: string) => {
    if (url) void openUrl(url).catch((openError) => setError(String(openError)));
  };

  const testEndpoints = async () => {
    const current = networkContext();
    setNetworkBusy("probe");
    setNetworkMessage(null);
    setError(null);
    try {
      const probes = await invoke<EndpointProbe[]>("test_provider_endpoints", {
        provider: draft,
        timeoutSecs: 8,
      });
      if (!current()) return;
      const reachable = probes.filter((probe) => probe.reachable);
      if (reachable.length === 0) {
        throw new Error(probes.map((probe) => probe.error).filter(Boolean).join("；") || "端点不可达");
      }
      const fastest = reachable.reduce((best, probe) =>
        (probe.latencyMs ?? Number.MAX_SAFE_INTEGER) <
        (best.latencyMs ?? Number.MAX_SAFE_INTEGER)
          ? probe
          : best,
      );
      setNetworkMessage({
        kind: "ok",
        text: `连接成功 · HTTP ${fastest.status ?? "—"} · ${fastest.latencyMs ?? "—"} ms`,
      });
      if (meta.endpointAutoSelect && fastest.url !== draft.baseUrl) {
        update({ baseUrl: fastest.url });
      }
    } catch (probeError) {
      if (current()) setNetworkMessage({ kind: "err", text: `连接失败：${String(probeError)}` });
    } finally {
      setNetworkBusy(null);
    }
  };

  const fetchModels = async () => {
    const current = networkContext();
    setNetworkBusy("models");
    setModelsMessage(null);
    setError(null);
    try {
      const nextModels = await invoke<FetchedModel[]>("fetch_provider_models", {
        provider: draft,
        key: apiKey.trim() || null,
      });
      if (!current()) return;
      setModels(nextModels);
      setModelsMessage(
        nextModels.length > 0
          ? { kind: "ok", text: `已获取 ${nextModels.length} 个模型，点击模型输入框即可从列表中选择。` }
          : { kind: "err", text: "模型接口返回成功，但列表为空。" },
      );
    } catch (fetchError) {
      if (current()) setModelsMessage({ kind: "err", text: `获取模型失败：${String(fetchError)}` });
    } finally {
      setNetworkBusy(null);
    }
  };

  const addEndpoint = () =>
    updateMeta({ endpointCandidates: [...meta.endpointCandidates, ""] });

  const updateEndpoint = (index: number, value: string) =>
    updateMeta({
      endpointCandidates: meta.endpointCandidates.map((item, itemIndex) =>
        itemIndex === index ? value : item,
      ),
    });

  const removeEndpoint = (index: number) =>
    updateMeta({
      endpointCandidates: meta.endpointCandidates.filter(
        (_, itemIndex) => itemIndex !== index,
      ),
    });

  const save = async () => {
    if (saveInFlight.current || networkBusy !== null) return;
    let nextDraft = draft;
    if (rawSettingsDirty) {
      const parsed = parseJsonObject(rawSettings, "完整配置");
      if (parsed.error || !parsed.value) {
        setError(parsed.error ?? "完整配置不能为空。");
        return;
      }
      nextDraft = applySettingsConfig(nextDraft, parsed.value);
    }

    const parsedHeaders = parseJsonObject(headersJson, "Header 覆盖");
    if (parsedHeaders.error || !parsedHeaders.value) {
      setError(parsedHeaders.error ?? "Header 覆盖不能为空。");
      return;
    }
    const headers = validateRequestHeaders(parsedHeaders.value);
    if (headers.error || !headers.value) {
      setError(headers.error ?? "Header 覆盖无效。");
      return;
    }
    const parsedBody = parseJsonObject(bodyJson, "Body 覆盖");
    if (parsedBody.error || !parsedBody.value) {
      setError(parsedBody.error ?? "Body 覆盖不能为空。");
      return;
    }
    if (Object.prototype.hasOwnProperty.call(parsedBody.value, "stream")) {
      setError("Body 覆盖不能包含由 CrewVela 管理的 stream 字段。");
      return;
    }

    const name = nextDraft.name.trim();
    const baseUrl = nextDraft.baseUrl.trim();
    // Existing IDs are referenced by sessions, presets and credentials. Editing
    // a service must keep that identity until a full migration is supported.
    const id = isNewProvider
      ? nextDraft.id.trim() || name.toLowerCase().replace(/[^a-z0-9]+/g, "-")
      : provider.id;
    const currentMeta = resolvedProviderMeta(nextDraft);
    const userAgentError = validateUserAgent(currentMeta.customUserAgent);
    if (userAgentError) {
      setError(userAgentError);
      return;
    }
    if (!name) {
      setError("请填写模型服务名称。");
      return;
    }
    if (!id) {
      setError("无法生成唯一标识，请修改模型服务名称。");
      return;
    }
    if (
      providers.some(
        (existing) => existing.id === id && existing.id !== provider.id,
      )
    ) {
      setError(`唯一标识“${id}”已存在，请修改后再保存。`);
      return;
    }
    if (!baseUrl || !isValidEndpoint(baseUrl)) {
      setError("请填写以 http:// 或 https:// 开头的有效 API 端点。");
      return;
    }
    if (!apiKey.trim() && !hasKey) {
      setError("请填写 API Key 后再保存模型服务。");
      return;
    }

    const finalProvider: ProviderConfig = {
      ...nextDraft,
      id,
      name,
      baseUrl,
      authTokenRef: isNewProvider ? nextDraft.authTokenRef.trim() || id : provider.authTokenRef,
      meta: {
        ...currentMeta,
        requestHeaders: headers.value,
        requestBody: parsedBody.value,
      },
    };

    saveInFlight.current = true;
    setSaving(true);
    setError(null);
    try {
      await onSave(finalProvider, apiKey.trim() || undefined);
    } catch (saveError) {
      setError(`保存失败：${String(saveError)}`);
    } finally {
      saveInFlight.current = false;
      setSaving(false);
    }
  };

  return (
    <Modal
      overlayClassName="editor-overlay"
      className={`editor provider-editor${shouldChoosePreset ? " choosing-preset" : ""}`}
      onClose={() => { if (!saveInFlight.current) onClose(); }}
      closeOnOverlayClick
      labelledBy="provider-editor-title"
    >
        {shouldChoosePreset ? (
          <>
            <header className="provider-editor-header">
              <div>
                <span className="provider-editor-eyebrow">模型服务</span>
                <h2 id="provider-editor-title">添加模型服务</h2>
                <p>选择预设后会自动填入官方端点、模型映射和高级参数。</p>
              </div>
              <button className="dialog-close" type="button" aria-label="关闭" onClick={onClose} disabled={saving}>
                <XIcon size={16} aria-hidden="true" />
              </button>
            </header>
            <div className="provider-preset-intro">
              <KeyIcon size={17} aria-hidden="true" />
              <span>内置预设通常只需粘贴 API Key；敏感信息仍保存在系统凭据管理器。</span>
            </div>
            <div className="provider-preset-grid">
              {presets.map((preset) => (
                <button
                  className="provider-preset-card"
                  type="button"
                  key={preset.id}
                  onClick={() => choosePreset(preset)}
                >
                  <span className="provider-preset-icon" data-provider-tone={preset.tone}>
                    {preset.label.slice(0, 1).toUpperCase()}
                  </span>
                  <span className="provider-preset-copy">
                    <strong>{preset.label}</strong>
                    <small>{preset.description}</small>
                  </span>
                  <span className="provider-preset-action">配置</span>
                </button>
              ))}
            </div>
            <footer className="provider-editor-footer">
              <span>当前只支持这 5 类模型服务。</span>
              <button className="btn" type="button" onClick={onClose}>取消</button>
            </footer>
          </>
        ) : (
          <>
            <header className="provider-editor-header compact">
              <div className="provider-editor-title-row">
                {isNewProvider && (
                  <button className="provider-editor-back" type="button" aria-label="返回预设" onClick={returnToPresets}>
                    <ArrowLeftIcon size={17} aria-hidden="true" />
                  </button>
                )}
                <div>
                  <span className="provider-editor-eyebrow">{isNewProvider ? "新建模型服务" : "模型服务设置"}</span>
                  <h2 id="provider-editor-title">{isNewProvider ? selectedPreset?.label : draft.name}</h2>
                  <p>{isNewProvider ? selectedPreset?.description : "修改会在下一次启动或恢复会话时生效。"}</p>
                </div>
              </div>
              <button className="dialog-close" type="button" aria-label="关闭" onClick={onClose} disabled={saving}>
                <XIcon size={16} aria-hidden="true" />
              </button>
            </header>

            <fieldset className="provider-editor-scroll" disabled={saving} style={{ border: 0, margin: 0, minWidth: 0 }}>
              {!isNewProvider && (
                <div className="provider-effective-summary">
                  <span className="provider-effective-cell">
                    <label>当前生效端点</label>
                    <b title={draft.baseUrl || undefined}>{draft.baseUrl || "未配置"}</b>
                    {meta.endpointAutoSelect && <i>测速后自动选最快</i>}
                  </span>
                  <span className="provider-effective-cell">
                    <label>兜底模型</label>
                    <b>{effectiveModel || "未配置"}</b>
                    {hasOneM(draft.env.ANTHROPIC_MODEL ?? "") && <i>1M 上下文</i>}
                  </span>
                  {customizationCount !== null && (
                    <span className="provider-effective-cell">
                      <label>与预设默认</label>
                      <b>{customizationCount === 0 ? "完全一致" : `已自定义 ${customizationCount} 项`}</b>
                      <i>{customizationCount === 0 ? "跟随官方推荐配置" : "保存后以本页配置为准"}</i>
                    </span>
                  )}
                </div>
              )}
              <section className="provider-form-section provider-identity-section">
                <div className="provider-section-heading">
                  <div><strong>基础信息</strong><span>服务名称、官网和备注</span></div>
                  {isNewProvider && selectedPreset && (
                    <span className="provider-inline-status"><CheckCircleIcon size={15} weight="fill" /> 已应用预设</span>
                  )}
                </div>
                <div className="provider-identity-grid">
                  <div className="provider-live-icon" data-provider-tone={activePresetId}>
                    {(draft.name || selectedPreset?.label || "P").slice(0, 1).toUpperCase()}
                  </div>
                  <div className="field"><label htmlFor="provider-name">显示名称</label><input id="provider-name" value={draft.name} onChange={(event) => update({ name: event.target.value })} /></div>
                  <div className="field"><label htmlFor="provider-website">服务商官网</label><div className="provider-linked-input"><input id="provider-website" value={meta.websiteUrl} onChange={(event) => updateMeta({ websiteUrl: event.target.value })} placeholder="https://example.com" /><button type="button" disabled={!meta.websiteUrl} onClick={() => openExternal(meta.websiteUrl)} aria-label="打开官网"><ArrowSquareOutIcon size={16} /></button></div></div>
                </div>
                <div className="field"><label htmlFor="provider-notes">备注</label><textarea id="provider-notes" value={meta.notes} onChange={(event) => updateMeta({ notes: event.target.value })} placeholder="例如：团队账号、计费说明或使用范围" rows={2} /></div>
              </section>

              <section className="provider-form-section">
                <div className="provider-section-heading"><div><strong>连接与凭据</strong><span>密钥不会写入 providers.json</span></div><button className="btn compact" type="button" onClick={() => void testEndpoints()} disabled={networkBusy !== null}><GaugeIcon size={15} />{networkBusy === "probe" ? "检测中…" : "测试端点"}</button></div>
                <div className="field"><label htmlFor="provider-api-key">API Key {hasKey && !isNewProvider && <span className="badge ok">已设置</span>}</label><div className="provider-key-line"><div className="provider-secret-input"><input id="provider-api-key" type={showApiKey ? "text" : "password"} value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={hasKey ? "留空保持现有密钥" : "粘贴 API Key"} autoComplete="off" /><button type="button" onClick={() => setShowApiKey((visible) => !visible)} aria-label={showApiKey ? "隐藏密钥" : "显示密钥"}>{showApiKey ? <EyeSlashIcon size={17} /> : <EyeIcon size={17} />}</button></div><button className="btn provider-help-link" type="button" disabled={!meta.apiKeyUrl} onClick={() => openExternal(meta.apiKeyUrl)}><KeyIcon size={15} /> 获取 API Key</button></div></div>
                <div className="provider-endpoint-row"><div className="field"><label htmlFor="provider-base-url">API 端点</label><input id="provider-base-url" value={draft.baseUrl} onChange={(event) => update({ baseUrl: event.target.value })} placeholder="https://api.example.com/anthropic" /></div><Switch className="provider-mini-toggle" checked={meta.isFullUrl} onChange={(checked) => updateMeta({ isFullUrl: checked })} withKnob={false}><span>完整 URL</span></Switch></div>
                {meta.endpointCandidates.length > 0 && <div className="provider-candidate-list"><span>候选端点</span>{meta.endpointCandidates.map((endpoint, index) => <div className="provider-candidate-row" key={index}><input value={endpoint} onChange={(event) => updateEndpoint(index, event.target.value)} /><button type="button" aria-label="删除候选端点" onClick={() => removeEndpoint(index)}>×</button></div>)}</div>}
                <div className="provider-inline-actions"><button className="btn compact" type="button" onClick={addEndpoint}>+ 候选端点</button><Switch className="provider-mini-toggle" checked={meta.endpointAutoSelect} onChange={(checked) => updateMeta({ endpointAutoSelect: checked })} withKnob={false}><span>测速后自动选最快端点</span></Switch></div>
                {networkMessage && <div className={`provider-network-message ${networkMessage.kind}`}>{networkMessage.kind === "ok" ? <CheckCircleIcon size={15} weight="fill" /> : <WarningCircleIcon size={15} weight="fill" />}{networkMessage.text}</div>}
              </section>

              <section className="provider-form-section">
                <div className="provider-section-heading"><div><strong>API 与模型映射</strong><span>对应 Claude Code 的各角色模型</span></div><button className="btn compact" type="button" onClick={() => void fetchModels()} disabled={networkBusy !== null}><DownloadSimpleIcon size={15} />{networkBusy === "models" ? "获取中…" : "获取模型"}</button></div>
                <div className="field-row provider-api-format-row"><div className="field"><label htmlFor="provider-api-format">API 格式</label><select id="provider-api-format" value={meta.apiFormat} onChange={(event) => updateMeta({ apiFormat: event.target.value as ProviderMeta["apiFormat"] })}><option value="anthropic">Anthropic</option><option value="openai_chat">OpenAI Chat（需要协议转译）</option><option value="openai_responses">OpenAI Responses（需要协议转译）</option><option value="gemini_native">Gemini Native（需要协议转译）</option></select>{meta.apiFormat !== "anthropic" && <div className="hint">该格式会随模型服务保存；Claude Code 使用前仍需启用对应的协议转译。</div>}</div><div className="field"><label htmlFor="provider-key-field">API Key 字段</label><select id="provider-key-field" value={meta.apiKeyField} onChange={(event) => updateMeta({ apiKeyField: event.target.value as ProviderMeta["apiKeyField"] })}><option value="ANTHROPIC_AUTH_TOKEN">ANTHROPIC_AUTH_TOKEN</option><option value="ANTHROPIC_API_KEY">ANTHROPIC_API_KEY</option></select></div></div>
                {modelsMessage && <div className={`provider-network-message ${modelsMessage.kind}`}>{modelsMessage.kind === "ok" ? <CheckCircleIcon size={15} weight="fill" /> : <WarningCircleIcon size={15} weight="fill" />}{modelsMessage.text}</div>}
                <div className="provider-model-table"><div className="provider-model-table-head"><span>角色</span><span>显示名称</span><span>上游模型</span><span>1M</span></div>{MODEL_ROLE_FIELDS.map((field) => { const model = draft.env[field.model] ?? ""; return <div className="provider-model-row" key={field.role}><strong>{field.role}</strong><input value={field.display ? draft.env[field.display] ?? "" : ""} disabled={!field.display} onChange={(event) => field.display && updateEnv(field.display, event.target.value)} placeholder={field.display ? "可选显示名" : "—"} /><ModelCombobox options={modelOptions} value={stripOneM(model)} onChange={(value) => updateEnv(field.model, setOneM(value, hasOneM(model)))} placeholder="模型 ID" /><label><input type="checkbox" disabled={!field.supportsOneM || !stripOneM(model)} checked={field.supportsOneM && hasOneM(model)} onChange={(event) => updateEnv(field.model, setOneM(model, event.target.checked))} /></label></div>; })}</div>
                <div className="provider-quick-set"><span>快速设置</span><button type="button" onClick={() => { const model = stripOneM(draft.env.ANTHROPIC_MODEL ?? ""); if (!model) return; const env = { ...draft.env }; MODEL_ROLE_FIELDS.forEach((field) => { env[field.model] = model; }); replaceDraft({ ...draft, env }); }}>将兜底模型应用到全部角色</button></div>
                <div className="field provider-fallback-field"><label htmlFor="provider-fallback">默认兜底模型</label><div className="provider-fallback-line"><ModelCombobox id="provider-fallback" options={modelOptions} value={stripOneM(draft.env.ANTHROPIC_MODEL ?? "")} onChange={(value) => updateEnv("ANTHROPIC_MODEL", setOneM(value, hasOneM(draft.env.ANTHROPIC_MODEL ?? "")))} placeholder="未匹配角色时使用的模型" /><Switch className="provider-mini-toggle" disabled={!stripOneM(draft.env.ANTHROPIC_MODEL ?? "")} checked={hasOneM(draft.env.ANTHROPIC_MODEL ?? "")} onChange={(checked) => updateEnv("ANTHROPIC_MODEL", setOneM(draft.env.ANTHROPIC_MODEL ?? "", checked))} withKnob={false}><span>1M</span></Switch></div></div>
              </section>

              <section className={`provider-advanced${advancedOpen ? " open" : ""}`}>
                <button className="provider-advanced-toggle" type="button" aria-expanded={advancedOpen} onClick={() => setAdvancedOpen((open) => !open)}><SlidersHorizontalIcon size={17} /><span><strong>高级配置</strong><small>User-Agent、请求覆盖、行为开关和完整 JSON</small></span><i>⌄</i></button>
                {advancedOpen && <div className="provider-advanced-body">
                  <div className="provider-advanced-grid"><div className="field"><label htmlFor="provider-id">唯一标识</label><input id="provider-id" value={draft.id} readOnly title="唯一标识自动生成，创建后保持不变" /></div><div className="field"><label htmlFor="provider-timeout">请求超时（毫秒）</label><input id="provider-timeout" type="number" value={draft.timeoutMs ?? ""} onChange={(event) => update({ timeoutMs: event.target.value ? Number(event.target.value) : null })} placeholder="使用默认值" /></div><div className="field"><label htmlFor="provider-cost">成本倍率</label><input id="provider-cost" type="number" min="0" step="0.01" value={meta.costMultiplier ?? ""} onChange={(event) => updateMeta({ costMultiplier: event.target.value ? Number(event.target.value) : null })} placeholder="1.00" /></div><div className="field"><label htmlFor="provider-pricing-source">计费模型来源</label><select id="provider-pricing-source" value={meta.pricingModelSource} onChange={(event) => updateMeta({ pricingModelSource: event.target.value as ProviderMeta["pricingModelSource"] })}><option value="inherit">继承请求模型</option><option value="request">请求体</option><option value="response">响应体</option></select></div></div>
                  <div className="field"><label htmlFor="provider-user-agent">自定义 User-Agent</label><div className="provider-user-agent-line"><select aria-label="User-Agent 预设" value={USER_AGENT_PRESETS.some((item) => item.value === meta.customUserAgent) ? meta.customUserAgent : "custom"} onChange={(event) => event.target.value !== "custom" && updateMeta({ customUserAgent: event.target.value })}><option value="custom">自定义</option>{USER_AGENT_PRESETS.map((item) => <option value={item.value} key={item.label}>{item.label}</option>)}</select><input id="provider-user-agent" value={meta.customUserAgent} onChange={(event) => updateMeta({ customUserAgent: event.target.value })} placeholder="留空则跟随 Claude Code" /></div></div>
                  <div className="provider-json-grid"><div className="field"><label htmlFor="provider-headers">自定义请求 Header</label><textarea id="provider-headers" rows={6} spellCheck={false} value={headersJson} onChange={(event) => setHeadersJson(event.target.value)} /><div className="hint">通过 Claude Code 官方 ANTHROPIC_CUSTOM_HEADERS 注入。</div></div><div className="field provider-disabled-field"><label htmlFor="provider-body">请求 Body 覆盖</label><textarea id="provider-body" rows={6} spellCheck={false} value={bodyJson} disabled /><div className="hint">当前 5 个 Anthropic 兼容服务不启用协议代理，因此不提供 Body 改写。</div></div></div>
                  <div className="provider-setting-toggles"><label><input type="checkbox" checked={toggles.hideAttribution} onChange={(event) => replaceDraft(updateSettingToggle(draft, "hideAttribution", event.target.checked))} />隐藏 AI 署名</label><label><input type="checkbox" checked={toggles.teammates} onChange={(event) => replaceDraft(updateSettingToggle(draft, "teammates", event.target.checked))} />启用 Teammates</label><label><input type="checkbox" checked={toggles.enableToolSearch} onChange={(event) => replaceDraft(updateSettingToggle(draft, "enableToolSearch", event.target.checked))} />启用 Tool Search</label><label><input type="checkbox" checked={toggles.effortMax} onChange={(event) => replaceDraft(updateSettingToggle(draft, "effortMax", event.target.checked))} />最大 Effort</label><label><input type="checkbox" checked={toggles.disableAutoUpgrade} onChange={(event) => replaceDraft(updateSettingToggle(draft, "disableAutoUpgrade", event.target.checked))} />关闭自动更新</label><label><input type="checkbox" checked={draft.disableNonessentialTraffic} onChange={(event) => update({ disableNonessentialTraffic: event.target.checked })} />关闭非必要流量</label></div>
                  {otherEnvEntries.length > 0 && <div className="provider-extra-env"><strong>其他预设环境变量</strong>{otherEnvEntries.map(([key, value]) => <span key={key}><code>{key}</code><em>{value}</em></span>)}</div>}
                  <div className="field provider-raw-settings"><div className="provider-field-label-row"><label htmlFor="provider-settings-json">完整配置 JSON</label><button type="button" onClick={applyRawSettings} disabled={!rawSettingsDirty}>应用 JSON</button></div><textarea id="provider-settings-json" rows={10} spellCheck={false} value={rawSettings} onChange={(event) => { setRawSettings(event.target.value); setRawSettingsDirty(true); }} /></div>
                </div>}
              </section>
              {error && <div className="provider-editor-error">{error}</div>}
            </fieldset>

            <footer className="provider-editor-footer form-actions"><span>保存后可在快速指挥和项目协作中选择。</span><div><button className="btn" type="button" onClick={onClose} disabled={saving}>取消</button><button className="btn primary" type="button" onClick={() => void save()} disabled={saving || networkBusy !== null}>{saving ? "保存中…" : isNewProvider ? "添加模型服务" : "保存修改"}</button></div></footer>
          </>
        )}
    </Modal>
  );
}
