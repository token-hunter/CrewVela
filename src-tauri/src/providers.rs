//! Shared app-data path/atomic I/O and legacy session descriptors. No model-service configuration.
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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


pub fn config_dir() -> Result<PathBuf> { crate::brand::config_dir() }
pub fn local_cli() -> ProviderConfig {
 ProviderConfig { id:"local".into(),name:"Claude Code".into(),base_url:String::new(),auth_token_ref:String::new(),env:HashMap::new(),timeout_ms:None,disable_nonessential_traffic:false,builtin:true,hidden:false,meta:ProviderMeta::default() }
}
// Legacy provider IDs are historical labels, never a source of credentials or routing.
pub fn get(_id: &str) -> Result<Option<ProviderConfig>> { Ok(Some(local_cli())) }
pub fn load() -> Result<ProvidersFile> { Ok(ProvidersFile {version:1,providers:vec![local_cli()]}) }
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
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
