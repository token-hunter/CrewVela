use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_VERSION: u32 = 1;
// Reads can initialize or migrate the file, so they participate in the same
// transaction lock as explicit mutations, including background quota readers.
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn lock_store() -> Result<MutexGuard<'static, ()>> {
    STORE_LOCK.lock().map_err(|_| anyhow::anyhow!("模型服务配置锁不可用"))
}

/// 单个 provider 的完整配置，camelCase 序列化以匹配前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// keychain 条目的 account 名（一般等于 id）
    pub auth_token_ref: String,
    /// 额外环境变量（model 映射等）
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub disable_nonessential_traffic: bool,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub meta: ProviderMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeta {
    #[serde(default)]
    pub preset_id: String,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub website_url: String,
    #[serde(default)]
    pub api_key_url: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub icon_color: String,
    #[serde(default)]
    pub category: String,
    #[serde(default = "default_api_format")]
    pub api_format: String,
    #[serde(default = "default_api_key_field")]
    pub api_key_field: String,
    #[serde(default)]
    pub is_full_url: bool,
    #[serde(default)]
    pub models_url: String,
    #[serde(default)]
    pub endpoint_candidates: Vec<String>,
    #[serde(default = "default_true")]
    pub endpoint_auto_select: bool,
    #[serde(default)]
    pub custom_user_agent: String,
    #[serde(default)]
    pub request_headers: HashMap<String, String>,
    #[serde(default)]
    pub request_body: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub settings: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub cost_multiplier: Option<f64>,
    #[serde(default = "default_pricing_model_source")]
    pub pricing_model_source: String,
}

impl Default for ProviderMeta {
    fn default() -> Self {
        Self {
            preset_id: String::new(),
            schema_version: default_schema_version(),
            website_url: String::new(),
            api_key_url: String::new(),
            notes: String::new(),
            icon: String::new(),
            icon_color: String::new(),
            category: "custom".into(),
            api_format: default_api_format(),
            api_key_field: default_api_key_field(),
            is_full_url: false,
            models_url: String::new(),
            endpoint_candidates: Vec::new(),
            endpoint_auto_select: true,
            custom_user_agent: String::new(),
            request_headers: HashMap::new(),
            request_body: serde_json::Map::new(),
            settings: serde_json::Map::new(),
            cost_multiplier: None,
            pricing_model_source: default_pricing_model_source(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_schema_version() -> u32 {
    1
}

fn default_api_format() -> String {
    "anthropic".into()
}

fn default_api_key_field() -> String {
    "ANTHROPIC_AUTH_TOKEN".into()
}

fn default_pricing_model_source() -> String {
    "inherit".into()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersFile {
    pub version: u32,
    pub providers: Vec<ProviderConfig>,
}

/// Canonical CrewVela entry, sharing legacy data through a migration alias.
pub fn config_dir() -> Result<PathBuf> {
    crate::brand::config_dir()
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("providers.json"))
}

/// 加载配置；首次运行写入预置模板；已存在则补齐新增的内置 provider
/// （让后续新增的 builtin，如 DeepSeek，自动出现在老配置里）。
pub fn load() -> Result<ProvidersFile> {
    load_at(&config_path()?)
}

fn load_at(path: &Path) -> Result<ProvidersFile> {
    let _guard = lock_store()?;
    load_unlocked(path)
}

fn load_unlocked(path: &Path) -> Result<ProvidersFile> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let seeded = ProvidersFile {
                version: CONFIG_VERSION,
                providers: default_providers(),
            };
            save_at(path, &seeded)?;
            return Ok(seeded);
        }
        Err(error) => return Err(error).with_context(|| format!("读取配置失败: {}", path.display())),
    };
    let mut file: ProvidersFile = serde_json::from_str(&raw).context("解析 providers.json 失败")?;
    // 迁移：补齐缺失的内置 provider，并给旧内置记录补显式 presetId。
    let mut changed = false;
    let defaults = default_providers();
    for provider in &mut file.providers {
        if provider.meta.preset_id.is_empty() && defaults.iter().any(|item| item.id == provider.id)
        {
            provider.meta.preset_id = provider.id.clone();
            provider.meta.schema_version = default_schema_version();
            changed = true;
        }
    }
    for dp in defaults {
        if !file.providers.iter().any(|p| p.id == dp.id) {
            file.providers.push(dp);
            changed = true;
        }
    }
    if changed {
        save_at(path, &file)?;
    }
    Ok(file)
}

// Caller holds STORE_LOCK across its complete read/modify/write transaction.
fn save_at(path: &Path, file: &ProvidersFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file)?;
    write_atomic(&path, json.as_bytes())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    write_atomic_with(path, contents, |source, destination| fs::rename(source, destination))
}

fn write_atomic_with(
    path: &Path,
    contents: &[u8],
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let previous_permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("读取模型服务配置权限失败"),
    };
    if previous_permissions.as_ref().is_some_and(|permissions| permissions.readonly()) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "模型服务配置为只读文件").into());
    }
    let nonce = format!("{}.{}", std::process::id(), unix_timestamp_nanos());
    let temp = path.with_extension(format!("json.{nonce}.tmp"));

    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut output = options.open(&temp)
        .with_context(|| format!("创建临时配置失败: {}", temp.display()))?;
    if let Some(permissions) = previous_permissions {
        if let Err(error) = output.set_permissions(permissions) {
            drop(output);
            let _ = fs::remove_file(&temp);
            return Err(error).context("保留模型服务配置权限失败");
        }
    }
    if let Err(error) = output.write_all(contents).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&temp);
        return Err(error).context("写入并同步模型服务临时配置失败");
    }
    drop(output);

    // A same-directory rename replaces the old file in one publication step.
    // Moving it to a backup first exposes a missing path to concurrent readers.
    if let Err(error) = rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(error).with_context(|| format!("替换配置失败: {}", path.display()));
    }
    Ok(())
}

fn unix_timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

pub fn get(id: &str) -> Result<Option<ProviderConfig>> {
    Ok(load()?.providers.into_iter().find(|p| p.id == id))
}

pub fn upsert(provider: ProviderConfig) -> Result<()> {
    let provider = normalize_provider(provider)?;
    update_at(&config_path()?, |file| {
        if let Some(existing) = file.providers.iter_mut().find(|p| p.id == provider.id) {
            *existing = provider;
        } else {
            file.providers.push(provider);
        }
        Ok(())
    })
}

fn update_at(path: &Path, update: impl FnOnce(&mut ProvidersFile) -> Result<()>) -> Result<()> {
    let _guard = lock_store()?;
    let mut file = load_unlocked(path)?;
    update(&mut file)?;
    save_at(path, &file)
}

pub fn normalize_provider(mut provider: ProviderConfig) -> Result<ProviderConfig> {
    provider.id = provider.id.trim().to_string();
    provider.name = provider.name.trim().to_string();
    provider.base_url = provider.base_url.trim().to_string();
    provider.auth_token_ref = provider.auth_token_ref.trim().to_string();

    let mut normalized_env = HashMap::with_capacity(provider.env.len());
    for (key, value) in provider.env {
        let key = key.trim().to_string();
        if key.is_empty() {
            anyhow::bail!("环境变量名称不能为空");
        }
        if normalized_env
            .insert(key.clone(), value.trim().to_string())
            .is_some()
        {
            anyhow::bail!("环境变量名称重复: {key}");
        }
    }
    provider.env = normalized_env;

    provider.meta.website_url = provider.meta.website_url.trim().to_string();
    provider.meta.preset_id = provider.meta.preset_id.trim().to_string();
    provider.meta.api_key_url = provider.meta.api_key_url.trim().to_string();
    provider.meta.notes = provider.meta.notes.trim().to_string();
    provider.meta.icon = provider.meta.icon.trim().to_string();
    provider.meta.icon_color = provider.meta.icon_color.trim().to_string();
    provider.meta.category = provider.meta.category.trim().to_string();
    provider.meta.api_format = provider.meta.api_format.trim().to_string();
    provider.meta.api_key_field = provider.meta.api_key_field.trim().to_string();
    provider.meta.models_url = provider.meta.models_url.trim().to_string();
    provider.meta.custom_user_agent = provider.meta.custom_user_agent.trim().to_string();
    provider.meta.pricing_model_source = provider.meta.pricing_model_source.trim().to_string();

    let mut seen_endpoints = HashSet::new();
    provider.meta.endpoint_candidates = provider
        .meta
        .endpoint_candidates
        .into_iter()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty() && seen_endpoints.insert(value.clone()))
        .collect();

    let mut normalized_headers = HashMap::new();
    for (name, value) in provider.meta.request_headers {
        let name = name.trim().to_ascii_lowercase();
        if normalized_headers.insert(name.clone(), value).is_some() {
            anyhow::bail!("请求头名称重复: {name}");
        }
    }
    provider.meta.request_headers = normalized_headers;

    validate_provider(&provider)?;
    Ok(provider)
}

fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    if provider.id.is_empty() {
        anyhow::bail!("模型服务唯一标识不能为空");
    }
    if !provider
        .id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        anyhow::bail!("模型服务唯一标识只能包含字母、数字、-、_ 和 .");
    }
    if provider.name.is_empty() {
        anyhow::bail!("模型服务名称不能为空");
    }
    if provider.auth_token_ref.is_empty() {
        anyhow::bail!("模型服务凭据引用不能为空");
    }
    if provider.meta.schema_version == 0 {
        anyhow::bail!("模型服务配置版本必须大于 0");
    }

    let endpoint = provider.base_url.as_str();
    let endpoint_body = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"));
    if endpoint_body.is_none_or(|body| body.is_empty() || body.chars().any(char::is_whitespace)) {
        anyhow::bail!("API 端点必须是有效的 http:// 或 https:// 地址");
    }

    if provider.timeout_ms == Some(0) {
        anyhow::bail!("请求超时必须是大于 0 的毫秒数");
    }

    if !matches!(
        provider.meta.api_format.as_str(),
        "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"
    ) {
        anyhow::bail!("不支持的 API 格式: {}", provider.meta.api_format);
    }
    if !matches!(
        provider.meta.api_key_field.as_str(),
        "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY"
    ) {
        anyhow::bail!("不支持的 API Key 字段: {}", provider.meta.api_key_field);
    }
    if !provider.meta.website_url.is_empty() {
        validate_optional_http_url("官网链接", &provider.meta.website_url)?;
    }
    if !provider.meta.api_key_url.is_empty() {
        validate_optional_http_url("API Key 链接", &provider.meta.api_key_url)?;
    }
    if !provider.meta.models_url.is_empty() {
        validate_optional_http_url("模型列表地址", &provider.meta.models_url)?;
    }
    for endpoint in &provider.meta.endpoint_candidates {
        validate_optional_http_url("候选端点", endpoint)?;
    }
    if provider
        .meta
        .custom_user_agent
        .chars()
        .any(|character| (character.is_control() && character != '\t') || character == '\u{7f}')
    {
        anyhow::bail!("自定义 User-Agent 不能包含控制字符");
    }
    for (name, value) in &provider.meta.request_headers {
        if is_protected_request_header(name) {
            anyhow::bail!("请求头 {name} 由 CrewVela 管理，不能覆盖");
        }
        if name.is_empty()
            || !name.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(
                        character,
                        '!' | '#'
                            | '$'
                            | '%'
                            | '&'
                            | '\''
                            | '*'
                            | '+'
                            | '-'
                            | '.'
                            | '^'
                            | '_'
                            | '`'
                            | '|'
                            | '~'
                    )
            })
        {
            anyhow::bail!("无效的请求头名称: {name}");
        }
        if value
            .chars()
            .any(|character| (character.is_control() && character != '\t') || character == '\u{7f}')
        {
            anyhow::bail!("请求头 {name} 包含控制字符");
        }
    }
    if provider.meta.request_body.contains_key("stream") {
        anyhow::bail!("Body 覆盖不能包含协议字段 stream");
    }
    if provider
        .meta
        .cost_multiplier
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        anyhow::bail!("成本倍率必须是非负有限数值");
    }
    if !matches!(
        provider.meta.pricing_model_source.as_str(),
        "inherit" | "request" | "response"
    ) {
        anyhow::bail!(
            "不支持的计费模型来源: {}",
            provider.meta.pricing_model_source
        );
    }
    Ok(())
}

fn is_protected_request_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "content-type"
            | "accept-encoding"
            | "x-forwarded-host"
            | "x-forwarded-port"
            | "x-forwarded-proto"
            | "forwarded"
    )
}

fn validate_optional_http_url(label: &str, value: &str) -> Result<()> {
    let body = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    if body.is_none_or(|body| body.is_empty() || body.chars().any(char::is_whitespace)) {
        anyhow::bail!("{label}必须是有效的 http:// 或 https:// 地址");
    }
    Ok(())
}

pub fn delete(id: &str) -> Result<()> {
    let _guard = lock_store()?;
    let path = config_path()?;
    delete_with_credentials(load_unlocked(&path)?, id, |file| save_at(&path, file), crate::secrets::get, crate::secrets::delete, crate::secrets::set)
}

fn delete_with_credentials(
    mut file: ProvidersFile,
    id: &str,
    persist: impl FnOnce(&ProvidersFile) -> Result<()>,
    get_key: impl FnOnce(&str) -> Result<Option<String>>,
    delete_key: impl FnOnce(&str) -> Result<()>,
    restore_key: impl FnOnce(&str, &str) -> Result<()>,
) -> Result<()> {
    let Some(provider) = file.providers.iter().find(|provider| provider.id == id).cloned() else {
        return Ok(());
    };
    // Reject before touching the keychain: restoring a deliberately deleted
    // builtin key could itself fail, even though the provider was never removed.
    if provider.builtin {
        anyhow::bail!("内置模型不能删除，可以隐藏或恢复默认");
    }
    let account = provider.auth_token_ref;
    file.providers.retain(|provider| provider.id != id);
    if file.providers.iter().any(|provider| provider.auth_token_ref == account) {
        // Older configurations may legitimately share credentials. Removing a
        // single service must not invalidate any remaining service's key.
        return persist(&file);
    }
    let previous_key = get_key(&account)?;
    if previous_key.is_some() {
        delete_key(&account)?;
    }
    if let Err(error) = persist(&file) {
        if let Some(previous_key) = previous_key {
            if let Err(rollback_error) = restore_key(&account, &previous_key) {
                anyhow::bail!("删除模型服务失败: {error:#}；恢复原凭据失败: {rollback_error:#}");
            }
        }
        return Err(error);
    }
    Ok(())
}

pub fn set_hidden(id: &str, hidden: bool) -> Result<()> {
    update_at(&config_path()?, |file| {
        let provider = file
            .providers
            .iter_mut()
            .find(|provider| provider.id == id)
            .with_context(|| format!("未找到模型服务: {id}"))?;
        if !provider.builtin {
            anyhow::bail!("自定义模型不能隐藏，请直接删除");
        }
        provider.hidden = hidden;
        Ok(())
    })
}

pub fn restore_builtin(id: &str) -> Result<()> {
    update_at(&config_path()?, |file| {
        let index = file
            .providers
            .iter()
            .position(|provider| provider.id == id)
            .with_context(|| format!("未找到模型服务: {id}"))?;
        if !file.providers[index].builtin {
            anyhow::bail!("只有内置模型支持恢复默认");
        }
        let auth_token_ref = file.providers[index].auth_token_ref.clone();
        let mut restored = default_providers()
            .into_iter()
            .find(|provider| provider.id == id)
            .with_context(|| format!("未找到内置模型默认配置: {id}"))?;
        restored.auth_token_ref = auth_token_ref;
        restored.hidden = false;
        file.providers[index] = restored;
        Ok(())
    })
}

pub fn reorder(provider_ids: &[String]) -> Result<()> {
    update_at(&config_path()?, |file| {
        file.providers = reorder_providers(std::mem::take(&mut file.providers), provider_ids)?;
        Ok(())
    })
}

fn reorder_providers(
    providers: Vec<ProviderConfig>,
    provider_ids: &[String],
) -> Result<Vec<ProviderConfig>> {
    if providers.len() != provider_ids.len() {
        anyhow::bail!("Provider 排序列表与现有 Provider 数量不一致");
    }

    let mut providers_by_id: HashMap<String, ProviderConfig> = providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect();
    let mut reordered = Vec::with_capacity(provider_ids.len());

    for provider_id in provider_ids {
        let provider = providers_by_id
            .remove(provider_id)
            .with_context(|| format!("Provider 排序列表包含未知或重复 ID: {provider_id}"))?;
        reordered.push(provider);
    }

    if !providers_by_id.is_empty() {
        anyhow::bail!("Provider 排序列表缺少现有 Provider");
    }

    Ok(reordered)
}

/// 预置模板，配置取自 Reddit 原文（ThreeKiloZero）。
fn default_providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            id: "glm".into(),
            name: "智谱 GLM".into(),
            base_url: "https://open.bigmodel.cn/api/anthropic".into(),
            auth_token_ref: "glm".into(),
            env: map(&[
                ("ANTHROPIC_MODEL", "glm-5.1"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "glm-5.1"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "glm-5.1"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "glm-5.1"),
            ]),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
            hidden: false,
            meta: ProviderMeta {
                preset_id: "glm".into(),
                website_url: "https://open.bigmodel.cn".into(),
                api_key_url: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII".into(),
                icon: "zhipu".into(),
                icon_color: "#0F62FE".into(),
                category: "cn_official".into(),
                endpoint_candidates: vec!["https://open.bigmodel.cn/api/anthropic".into()],
                ..ProviderMeta::default()
            },
        },
        ProviderConfig {
            id: "kimi".into(),
            name: "Kimi For Coding".into(),
            base_url: "https://api.kimi.com/coding/".into(),
            auth_token_ref: "kimi".into(),
            env: map(&[
                ("ANTHROPIC_MODEL", "kimi-for-coding"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "kimi-for-coding"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "kimi-for-coding"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "kimi-for-coding"),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "262144"),
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "262144"),
            ]),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
            hidden: false,
            meta: ProviderMeta {
                preset_id: "kimi".into(),
                website_url: "https://www.kimi.com/code/?aff=cc-switch".into(),
                api_key_url: "https://www.kimi.com/code/?aff=cc-switch".into(),
                icon: "kimi".into(),
                icon_color: "#6366F1".into(),
                category: "cn_official".into(),
                endpoint_candidates: vec!["https://api.kimi.com/coding".into()],
                ..ProviderMeta::default()
            },
        },
        ProviderConfig {
            id: "minimax".into(),
            name: "MiniMax".into(),
            base_url: "https://api.minimaxi.com/anthropic".into(),
            auth_token_ref: "minimax".into(),
            env: map(&[
                ("ANTHROPIC_MODEL", "MiniMax-M2.7"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "MiniMax-M2.7"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "MiniMax-M2.7"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "MiniMax-M2.7"),
            ]),
            timeout_ms: Some(3_000_000),
            disable_nonessential_traffic: true,
            builtin: true,
            hidden: false,
            meta: ProviderMeta {
                preset_id: "minimax".into(),
                website_url: "https://platform.minimaxi.com".into(),
                api_key_url: "https://platform.minimaxi.com/subscribe/coding-plan".into(),
                icon: "minimax".into(),
                icon_color: "#FF6B6B".into(),
                category: "cn_official".into(),
                endpoint_candidates: vec!["https://api.minimaxi.com/anthropic".into()],
                ..ProviderMeta::default()
            },
        },
        // 配置取自 DeepSeek 官方 Claude Code 接入文档
        // https://api-docs.deepseek.com/zh-cn/quick_start/agent_integrations/claude_code
        ProviderConfig {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/anthropic".into(),
            auth_token_ref: "deepseek".into(),
            env: map(&[
                ("ANTHROPIC_MODEL", "deepseek-v4-pro"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "deepseek-v4-pro"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "deepseek-v4-pro"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "deepseek-v4-flash"),
            ]),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
            hidden: false,
            meta: ProviderMeta {
                preset_id: "deepseek".into(),
                website_url: "https://platform.deepseek.com".into(),
                api_key_url: "https://platform.deepseek.com/api_keys".into(),
                icon: "deepseek".into(),
                icon_color: "#1E88E5".into(),
                category: "cn_official".into(),
                models_url: "https://api.deepseek.com/models".into(),
                endpoint_candidates: vec!["https://api.deepseek.com/anthropic".into()],
                ..ProviderMeta::default()
            },
        },
        // 空白自定义模板：用户编辑填自己的 API 端点
        ProviderConfig {
            id: "custom".into(),
            name: "自定义".into(),
            base_url: String::new(),
            auth_token_ref: "custom".into(),
            env: HashMap::new(),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
            hidden: false,
            meta: ProviderMeta {
                preset_id: "custom".into(),
                ..ProviderMeta::default()
            },
        },
    ]
}

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_store() -> (PathBuf, PathBuf) {
        let directory = std::env::temp_dir().join(format!("crewvela-provider-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("providers.json");
        let mut file = load_at(&path).unwrap();
        let mut custom = file.providers[0].clone();
        custom.id = "kept-custom".into();
        custom.name = "original custom".into();
        custom.builtin = false;
        file.providers.push(custom);
        save_at(&path, &file).unwrap();
        (directory, path)
    }

    #[test]
    fn provider_atomic_publish_never_exposes_a_missing_file_to_load() {
        let (directory, path) = isolated_store();
        let mut file = load_at(&path).unwrap();
        file.providers.iter_mut().find(|provider| provider.id == "kept-custom").unwrap().name = "updated custom".into();
        let updated = serde_json::to_vec(&file).unwrap();
        let mut observed_custom = None;
        write_atomic_with(&path, &updated, |source, destination| {
            if destination == path && source.extension().is_some_and(|ext| ext == "tmp") {
                // Interleave the real initializing/migrating reader at publication.
                let observed = load_at(&path).unwrap();
                observed_custom = observed.providers.into_iter().find(|provider| provider.id == "kept-custom").map(|provider| provider.name);
            }
            fs::rename(source, destination)
        }).unwrap();
        let persisted = load_at(&path).unwrap();
        fs::remove_dir_all(directory).unwrap();
        assert_eq!(observed_custom.as_deref(), Some("original custom"), "a reader must keep seeing the previous complete config until replacement");
        assert_eq!(persisted.providers.iter().find(|provider| provider.id == "kept-custom").unwrap().name, "updated custom");
    }

    #[test]
    fn provider_load_waits_for_an_in_flight_read_modify_write() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (directory, path) = isolated_store();
        let writer_path = path.clone();
        let (loaded_tx, loaded_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || update_at(&writer_path, |file| {
            file.providers.iter_mut().find(|provider| provider.id == "kept-custom").unwrap().name = "committed edit".into();
            loaded_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        }));
        loaded_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (reader_started_tx, reader_started_rx) = mpsc::channel();
        let (read_tx, read_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            reader_started_tx.send(()).unwrap();
            read_tx.send(load_at(&path)).unwrap();
        });
        reader_started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = read_rx.recv_timeout(Duration::from_millis(150)).ok();
        release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        let was_early = early.is_some();
        let observed = early.unwrap_or_else(|| read_rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        reader.join().unwrap();
        fs::remove_dir_all(directory).unwrap();
        assert!(!was_early, "load must share the storage transaction lock");
        assert_eq!(observed.providers.iter().find(|provider| provider.id == "kept-custom").unwrap().name, "committed edit");
    }

    #[test]
    fn provider_atomic_publish_failure_keeps_original_file_and_permissions() {
        let (directory, path) = isolated_store();
        let original = fs::read(&path).unwrap();
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let error = write_atomic_with(&path, b"replacement", |_, destination| {
            assert_eq!(destination, path, "the original must never be moved aside");
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "injected publish denial"))
        }).unwrap_err();
        assert!(format!("{error:#}").contains("injected publish denial"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::metadata(&path).unwrap().permissions(), original_permissions);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1, "failed publish must clean up its temporary file");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn provider_atomic_publish_preserves_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (directory, path) = isolated_store();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let original = fs::read(&path).unwrap();
        write_atomic(&path, &original).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn provider_atomic_publish_readonly_destination_preserves_original() {
        let (directory, path) = isolated_store();
        let original = fs::read(&path).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        let result = write_atomic(&path, b"replacement");
        let retained = fs::read(&path).unwrap();
        let retained_permissions = fs::metadata(&path).unwrap().permissions();
        let remaining_count = fs::read_dir(&directory).unwrap().count();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_dir_all(directory).unwrap();
        assert!(result.is_err(), "read-only destination must reject replacement");
        assert_eq!(retained, original);
        assert!(retained_permissions.readonly());
        assert_eq!(remaining_count, 1);
    }

    #[test]
    fn provider_load_migrates_custom_configuration_and_does_not_overwrite_invalid_data() {
        let directory = std::env::temp_dir().join(format!("crewvela-provider-migration-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("providers.json");
        let legacy = deletion_fixture(false, false);
        save_at(&path, &legacy).unwrap();
        let migrated = load_at(&path).unwrap();
        assert!(migrated.providers.iter().any(|provider| provider.id == "first"));
        assert_eq!(migrated.providers.len(), default_providers().len() + 1);
        assert_eq!(load_at(&path).unwrap().providers.len(), migrated.providers.len());
        fs::write(&path, "invalid config remains visible").unwrap();
        assert!(load_at(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid config remains visible");
        fs::remove_dir_all(directory).unwrap();
    }

    fn deletion_fixture(shared: bool, builtin: bool) -> ProvidersFile {
        let mut first = default_providers().remove(0);
        first.id = "first".into();
        first.auth_token_ref = "shared-account".into();
        first.builtin = builtin;
        let mut providers = vec![first.clone()];
        if shared {
            first.id = "second".into();
            providers.push(first);
        }
        ProvidersFile { version: CONFIG_VERSION, providers }
    }

    #[test]
    fn provider_delete_preserves_shared_credentials_without_accessing_keychain() {
        let file = deletion_fixture(true, false);
        delete_with_credentials(file, "first", |remaining| {
            assert_eq!(remaining.providers.len(), 1);
            assert_eq!(remaining.providers[0].id, "second");
            Ok(())
        }, |_| panic!("shared credentials must not be read"),
           |_| panic!("shared credentials must not be deleted"),
           |_, _| panic!("shared credentials must not be changed")).unwrap();
    }

    #[test]
    fn provider_delete_rejects_builtins_before_accessing_credentials() {
        let error = delete_with_credentials(deletion_fixture(false, true), "first",
            |_| panic!("builtin deletion must not persist"),
            |_| panic!("builtin deletion must not read credentials"),
            |_| panic!("builtin deletion must not delete credentials"),
            |_, _| panic!("builtin deletion must not change credentials")).unwrap_err();
        assert!(error.to_string().contains("内置模型不能删除"));
    }

    #[test]
    fn provider_delete_exclusive_key_is_removed_only_with_the_provider() {
        let events = std::cell::RefCell::new(Vec::new());
        delete_with_credentials(deletion_fixture(false, false), "first", |remaining| {
            assert!(remaining.providers.is_empty());
            events.borrow_mut().push("persist");
            Ok(())
        }, |account| {
            assert_eq!(account, "shared-account");
            events.borrow_mut().push("read");
            Ok(Some("isolated test key".into()))
        }, |_| { events.borrow_mut().push("delete"); Ok(()) },
           |_, _| panic!("successful deletion must not restore credentials")).unwrap();
        assert_eq!(*events.borrow(), ["read", "delete", "persist"]);
    }

    #[test]
    fn provider_delete_persist_failure_restores_key_and_reports_rollback_failure() {
        for fail_restore in [false, true] {
            let key = std::cell::RefCell::new(Some("isolated test key".to_string()));
            let error = delete_with_credentials(deletion_fixture(false, false), "first",
                |_| anyhow::bail!("config write denied"),
                |_| Ok(key.borrow().clone()),
                |_| { key.replace(None); Ok(()) },
                |_, previous| {
                    assert_eq!(previous, "isolated test key");
                    if fail_restore { anyhow::bail!("key restore denied"); }
                    key.replace(Some(previous.to_string()));
                    Ok(())
                }).unwrap_err();
            assert!(error.to_string().contains("config write denied"));
            if fail_restore {
                assert!(error.to_string().contains("key restore denied"));
                assert!(key.borrow().is_none());
            } else {
                assert_eq!(key.borrow().as_deref(), Some("isolated test key"));
            }
        }
    }

    #[test]
    fn provider_delete_key_failure_does_not_persist_or_hide_the_error() {
        let error = delete_with_credentials(deletion_fixture(false, false), "first",
            |_| panic!("failed key removal must leave provider configuration intact"),
            |_| Ok(Some("isolated test key".into())),
            |_| anyhow::bail!("key delete denied"),
            |_, _| panic!("rejected deletion has not changed credentials")).unwrap_err();
        assert_eq!(error.to_string(), "key delete denied");
    }

    #[test]
    fn provider_delete_shared_persist_failure_never_touches_credentials() {
        let error = delete_with_credentials(deletion_fixture(true, false), "first",
            |_| anyhow::bail!("config write denied"),
            |_| panic!("shared credentials must not be read"),
            |_| panic!("shared credentials must not be deleted"),
            |_, _| panic!("shared credentials must not be restored")).unwrap_err();
        assert_eq!(error.to_string(), "config write denied");
    }

    #[test]
    fn provider_delete_missing_provider_and_missing_key_remain_supported() {
        delete_with_credentials(deletion_fixture(false, false), "missing",
            |_| panic!("missing provider must not persist"),
            |_| panic!("missing provider must not read credentials"),
            |_| panic!("missing provider must not delete credentials"),
            |_, _| panic!("missing provider must not restore credentials")).unwrap();
        delete_with_credentials(deletion_fixture(false, false), "first",
            |remaining| { assert!(remaining.providers.is_empty()); Ok(()) },
            |_| Ok(None),
            |_| panic!("missing key must not be deleted"),
            |_, _| panic!("missing key must not be restored")).unwrap();
    }

    #[test]
    fn atomic_write_replaces_existing_file_without_leaving_sidecars() {
        let directory = std::env::temp_dir().join(format!(
            "crewvela-provider-atomic-{}-{}",
            std::process::id(),
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("providers.json");
        fs::write(&path, "old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let remaining = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec![std::ffi::OsString::from("providers.json")]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn default_providers_include_five_builtins() {
        let providers = default_providers();
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        for id in ["glm", "kimi", "minimax", "deepseek", "custom"] {
            assert!(ids.contains(&id), "缺少内置 provider: {id}");
        }
        assert!(providers.iter().all(|p| p.builtin));
        assert!(providers.iter().all(|p| !p.hidden));
    }

    #[test]
    fn custom_provider_is_empty_template() {
        let custom = default_providers()
            .into_iter()
            .find(|p| p.id == "custom")
            .expect("应有 custom");
        assert!(custom.base_url.is_empty(), "custom base_url 应为空");
        assert!(custom.env.is_empty(), "custom env 应为空");
        assert!(custom.builtin);
        assert_eq!(custom.auth_token_ref, "custom");
    }

    #[test]
    fn deepseek_config_matches_official_docs() {
        let ds = default_providers()
            .into_iter()
            .find(|p| p.id == "deepseek")
            .expect("应有 deepseek");
        assert_eq!(ds.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(ds.auth_token_ref, "deepseek");
        assert_eq!(ds.env.get("ANTHROPIC_MODEL").unwrap(), "deepseek-v4-pro");
        assert_eq!(
            ds.env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").unwrap(),
            "deepseek-v4-flash"
        );
        assert_eq!(ds.meta.models_url, "https://api.deepseek.com/models");
    }

    #[test]
    fn reorder_providers_uses_requested_order() {
        let providers = default_providers();
        let ids = ["custom", "deepseek", "minimax", "kimi", "glm"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        let reordered = reorder_providers(providers, &ids).expect("排序应成功");
        let reordered_ids = reordered
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            reordered_ids,
            ids.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reorder_providers_rejects_incomplete_or_duplicate_ids() {
        let incomplete = ["glm", "kimi"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(reorder_providers(default_providers(), &incomplete).is_err());

        let duplicate = ["glm", "kimi", "minimax", "deepseek", "glm"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(reorder_providers(default_providers(), &duplicate).is_err());
    }

    #[test]
    fn provider_normalization_trims_fields_and_env() {
        let mut provider = default_providers()
            .into_iter()
            .find(|provider| provider.id == "kimi")
            .expect("应有 kimi");
        provider.id = " kimi-work ".into();
        provider.name = " Kimi Work ".into();
        provider.base_url = " https://api.kimi.com/coding/ ".into();
        provider.auth_token_ref = " kimi-work ".into();
        provider.env = [(" ANTHROPIC_MODEL ".into(), " kimi-for-coding ".into())]
            .into_iter()
            .collect();

        let normalized = normalize_provider(provider).expect("合法配置应可规范化");
        assert_eq!(normalized.id, "kimi-work");
        assert_eq!(normalized.name, "Kimi Work");
        assert_eq!(normalized.base_url, "https://api.kimi.com/coding/");
        assert_eq!(normalized.auth_token_ref, "kimi-work");
        assert_eq!(
            normalized.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("kimi-for-coding")
        );
    }

    #[test]
    fn provider_validation_rejects_invalid_endpoint_id_and_timeout() {
        let base = default_providers()
            .into_iter()
            .find(|provider| provider.id == "kimi")
            .expect("应有 kimi");

        let mut invalid_endpoint = base.clone();
        invalid_endpoint.base_url = "api.example.com".into();
        assert!(normalize_provider(invalid_endpoint).is_err());

        let mut invalid_id = base.clone();
        invalid_id.id = "kimi work".into();
        assert!(normalize_provider(invalid_id).is_err());

        let mut invalid_timeout = base;
        invalid_timeout.timeout_ms = Some(0);
        assert!(normalize_provider(invalid_timeout).is_err());
    }

    #[test]
    fn legacy_provider_json_loads_complete_provider_metadata_defaults() {
        let legacy = r#"{
            "id": "kimi",
            "name": "Kimi",
            "baseUrl": "https://api.kimi.com/coding/",
            "authTokenRef": "kimi",
            "env": {},
            "timeoutMs": null,
            "disableNonessentialTraffic": false,
            "builtin": true
        }"#;

        let provider: ProviderConfig = serde_json::from_str(legacy).expect("旧配置必须继续可读");

        assert_eq!(provider.meta.api_format, "anthropic");
        assert_eq!(provider.meta.api_key_field, "ANTHROPIC_AUTH_TOKEN");
        assert!(provider.meta.endpoint_auto_select);
        assert!(provider.meta.endpoint_candidates.is_empty());
        assert!(provider.meta.settings.is_empty());
        assert!(!provider.hidden);
    }
}
