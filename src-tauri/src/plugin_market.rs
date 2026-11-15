// 插件市场（方案 B：CrewVela 自管）：把 Claude Code / Codex 生态的 plugin
// marketplace.json 作为上游目录，安装到 CrewVela 自己的缓存后按组件路由——
// skills/ 进 Skill 中心（可分发到全部 runtime），.mcp.json 转成 MCP 管理条目，
// hooks/agents 仅识别展示（claude/codex 专属组件，暂不接线）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp;
use crate::skill_center::{ImportSkillInput, SkillCenter};
mod targets;
mod catalog;
mod native;
mod curation;
mod compatibility;
mod codex_native;
mod codex_wrapper;
mod official;
#[cfg(test)]
mod long_path_tests;
pub use official::add_official;
pub use curation::{candidate_snapshot, save_selection, PluginCandidateSnapshot};
pub use targets::{install_targets, remove_target, snapshot_with_targets, PluginInstallInput, PluginTarget};

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// 全部读写集中在显式路径下，测试用临时目录隔离。
#[derive(Clone)]
struct Paths {
    data_file: PathBuf,
    market_root: PathBuf,
    cache_root: PathBuf,
    mcp_file: PathBuf,
}

impl Paths {
    fn from_config_dir(dir: PathBuf) -> Self {
        Self {
            data_file: dir.join("plugin-market.json"),
            market_root: dir.join("plugins").join("marketplaces"),
            cache_root: dir.join("plugins").join("cache"),
            mcp_file: dir.join("mcp-servers.json"),
        }
    }

    fn standard() -> Result<Self> {
        Ok(Self::from_config_dir(crate::providers::config_dir()?))
    }
}

// ---- 数据模型 ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRecord {
    pub id: String,
    pub source: String,
    /// git | local
    pub source_kind: String,
    pub owner_name: String,
    pub path: String,
    pub url: Option<String>,
    pub added_at: u64,
    pub last_refreshed_at: Option<u64>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub pinned_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    /// "{plugin}@{marketplace}"
    pub id: String,
    pub marketplace_id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub cache_path: String,
    pub skill_asset_ids: Vec<String>,
    pub skill_names: Vec<String>,
    pub mcp_server_names: Vec<String>,
    pub has_hooks: bool,
    pub has_agents: bool,
    pub installed_at: u64,
    #[serde(default)]
    pub targets: Vec<PluginTarget>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketPluginView {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub source_display: String,
    pub installable: bool,
    pub note: Option<String>,
    pub compatibility: Option<compatibility::PluginCompatibility>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceView {
    pub id: String,
    pub source: String,
    pub source_kind: String,
    pub owner_name: String,
    pub path: String,
    pub url: Option<String>,
    pub last_refreshed_at: Option<u64>,
    pub last_error: Option<String>,
    pub plugins: Vec<MarketPluginView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketSnapshot {
    pub marketplaces: Vec<MarketplaceView>,
    pub installed: Vec<InstalledPlugin>,
    pub selection_configured: bool,
    pub selected_plugin_ids: Vec<String>,
    pub installed_catalog: Vec<InstalledCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledCatalogEntry {
    pub market: MarketplaceView,
    pub plugin: MarketPluginView,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct PluginMarketFile {
    version: u32,
    #[serde(default)]
    marketplaces: Vec<MarketplaceRecord>,
    #[serde(default)]
    installed: Vec<InstalledPlugin>,
    #[serde(default)]
    selected_plugin_ids: Option<Vec<String>>,
}

/// 插件来源（marketplace.json 条目解析结果）。
#[derive(Debug, Clone)]
enum PluginSourceRef {
    /// 市场仓库内的相对路径
    Relative(PathBuf),
    /// 独立 git 仓库（可选 monorepo 子目录 / ref）
    Git {
        url: String,
        subpath: Option<String>,
        reference: Option<String>,
    },
    Unsupported(&'static str),
}

// ---- 存取 ----

fn load_file_at(path: &Path) -> Result<PluginMarketFile> {
    if !path.exists() {
        return Ok(PluginMarketFile {
            version: 1,
            ..Default::default()
        });
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取插件市场配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 plugin-market.json 失败")
}

fn save_file_at(path: &Path, file: &PluginMarketFile) -> Result<()> {
    let mut file = file.clone();
    file.version = 1;
    let bytes = serde_json::to_vec_pretty(&file).context("序列化插件市场配置失败")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn mutate_at<T>(
    paths: &Paths,
    skill_center: &SkillCenter,
    mutation: impl FnOnce(&mut PluginMarketFile, &Paths, &SkillCenter) -> Result<T>,
) -> Result<T> {
    let mut file = load_file_at(&paths.data_file)?;
    let result = mutation(&mut file, paths, skill_center)?;
    save_file_at(&paths.data_file, &file)?;
    Ok(result)
}

// ---- marketplace.json 解析 ----

const MARKETPLACE_JSON_LOCATIONS: [&str; 2] = [
    ".claude-plugin/marketplace.json",
    ".agents/plugins/marketplace.json",
];

pub fn read_marketplace_json(root: &Path) -> Result<Value> {
    for relative in MARKETPLACE_JSON_LOCATIONS {
        let path = root.join(relative);
        if path.is_file() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("读取 marketplace.json 失败: {}", path.display()))?;
            return serde_json::from_str(&raw)
                .with_context(|| format!("解析 marketplace.json 失败: {}", path.display()));
        }
    }
    bail!(
        "未找到 marketplace.json（支持 {}）",
        MARKETPLACE_JSON_LOCATIONS.join(" 或 ")
    );
}

fn parse_entry_source(value: &Value, plugin_root: Option<&str>) -> PluginSourceRef {
    match value {
        Value::String(relative) => {
            if relative.starts_with("./") || relative.starts_with("../") {
                PluginSourceRef::Relative(PathBuf::from(relative))
            } else if relative.starts_with("https://") || relative.starts_with("http://") {
                PluginSourceRef::Git {
                    url: relative.clone(),
                    subpath: None,
                    reference: None,
                }
            } else if plugin_root.is_some() {
                PluginSourceRef::Relative(
                    PathBuf::from(plugin_root.expect("已检查")).join(relative),
                )
            } else {
                PluginSourceRef::Unsupported("裸名称来源缺少 metadata.pluginRoot")
            }
        }
        Value::Object(object) => {
            let reference = object
                .get("ref")
                .and_then(Value::as_str)
                .map(str::to_string);
            match object.get("source").and_then(Value::as_str).unwrap_or("") {
                "local" => object.get("path").and_then(Value::as_str)
                    .filter(|path| !path.is_empty()).map(|path| PluginSourceRef::Relative(PathBuf::from(path)))
                    .unwrap_or(PluginSourceRef::Unsupported("local 来源缺少 path")),
                "github" => {
                    let Some(repo) = object.get("repo").and_then(Value::as_str) else {
                        return PluginSourceRef::Unsupported("github 来源缺少 repo");
                    };
                    PluginSourceRef::Git {
                        url: format!("https://github.com/{repo}.git"),
                        subpath: None,
                        reference,
                    }
                }
                "url" => {
                    let Some(url) = object.get("url").and_then(Value::as_str) else {
                        return PluginSourceRef::Unsupported("url 来源缺少 url");
                    };
                    PluginSourceRef::Git {
                        url: url.to_string(),
                        subpath: None,
                        reference,
                    }
                }
                "git-subdir" => {
                    let Some(url) = object.get("url").and_then(Value::as_str) else {
                        return PluginSourceRef::Unsupported("git-subdir 来源缺少 url");
                    };
                    PluginSourceRef::Git {
                        url: url.to_string(),
                        subpath: object
                            .get("path")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        reference,
                    }
                }
                other => PluginSourceRef::Unsupported(match other {
                    "npm" | "archive" | "command" => "暂不支持的来源类型",
                    _ => "未知来源类型",
                }),
            }
        }
        _ => PluginSourceRef::Unsupported("来源字段格式无效"),
    }
}

fn source_display(source: &PluginSourceRef) -> String {
    match source {
        PluginSourceRef::Relative(path) => path.display().to_string(),
        PluginSourceRef::Git {
            url,
            subpath,
            reference,
        } => {
            let mut display = url.clone();
            if let Some(subpath) = subpath {
                display.push_str(&format!("#{subpath}"));
            }
            if let Some(reference) = reference {
                display.push_str(&format!("@{reference}"));
            }
            display
        }
        PluginSourceRef::Unsupported(reason) => reason.to_string(),
    }
}

fn entry_string(entry: &Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 解析市场目录为结构化视图；解析失败返回 Err（含原因），调用方记录 last_error。
fn parse_marketplace(
    root: &Path,
) -> Result<(String, String, Vec<(MarketPluginView, PluginSourceRef)>)> {
    parse_marketplace_filtered(root, None)
}

fn parse_marketplace_filtered(
    root: &Path,
    names: Option<&std::collections::HashSet<String>>,
) -> Result<(String, String, Vec<(MarketPluginView, PluginSourceRef)>)> {
    let json = read_marketplace_json(root)?;
    let id = entry_string(&json, "name").ok_or_else(|| anyhow!("marketplace.json 缺少 name"))?;
    let owner_name = json
        .pointer("/owner/name")
        .and_then(Value::as_str)
        .unwrap_or("未知来源")
        .to_string();
    let plugin_root = json
        .pointer("/metadata/pluginRoot")
        .and_then(Value::as_str)
        .map(str::to_string);
    let plugins = json
        .get("plugins")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| names.is_none_or(|names| entry.get("name").and_then(Value::as_str).is_some_and(|name| names.contains(name))))
                .map(|entry| {
                    let name =
                        entry_string(entry, "name").ok_or_else(|| anyhow!("市场条目缺少 name"))?;
                    let source_ref = parse_entry_source(
                        entry.get("source").unwrap_or(&Value::Null),
                        plugin_root.as_deref(),
                    );
                    let unsupported = matches!(source_ref, PluginSourceRef::Unsupported(_));
                    let view = MarketPluginView {
                        name: name.clone(),
                        display_name: entry_string(entry, "displayName"),
                        description: entry_string(entry, "description").unwrap_or_default(),
                        version: entry_string(entry, "version"),
                        author: entry
                            .pointer("/author/name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        category: entry_string(entry, "category"),
                        source_display: source_display(&source_ref),
                        installable: !unsupported,
                        note: match &source_ref {
                            PluginSourceRef::Unsupported(reason) => Some(reason.to_string()),
                            _ => None,
                        },
                        compatibility: None,
                    };
                    let mut view = view;
                    compatibility::enrich_view(root, &source_ref, &mut view);
                    Ok((view, source_ref))
                })
                .collect::<Result<Vec<_>>>()
        })
        .unwrap_or_else(|| Ok(Vec::new()))?;
    Ok((id, owner_name, plugins))
}

fn view_of(record: &MarketplaceRecord) -> Result<MarketplaceView> {
    let mut view = MarketplaceView {
        id: record.id.clone(),
        source: record.source.clone(),
        source_kind: record.source_kind.clone(),
        owner_name: record.owner_name.clone(),
        path: record.path.clone(),
        url: record.url.clone(),
        last_refreshed_at: record.last_refreshed_at,
        last_error: record.last_error.clone(),
        plugins: Vec::new(),
    };
    match parse_marketplace(Path::new(&record.path)) {
        Ok((_, _, plugins)) => {
            view.plugins = plugins.into_iter().map(|(plugin, _)| plugin).collect();
        }
        Err(error) => {
            view.last_error = Some(format!("{error:#}"));
        }
    }
    Ok(view)
}

// ---- git 工具 ----

fn run_git(args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    #[cfg(target_os = "windows")]
    command.args(["-c", "core.longpaths=true"]);
    command.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .with_context(|| format!("启动 git 失败（{}）", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} 失败：{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Prepare complete replacements beside the destination. Clone/copy failures
/// must leave a currently installed marketplace or plugin cache intact.
fn replace_directory(target: &Path, prepare: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let parent = target.parent().ok_or_else(|| anyhow!("目标目录缺少父目录"))?;
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".plugin-staging-{}", uuid::Uuid::new_v4()));
    let backup = parent.join(format!(".plugin-backup-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        prepare(&staging)?;
        let previous = match std::fs::symlink_metadata(target) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    bail!("目标不是普通受管目录，拒绝替换：{}", target.display());
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if previous { std::fs::rename(target, &backup)?; }
        if let Err(error) = std::fs::rename(&staging, target) {
            if previous { std::fs::rename(&backup, target).context("替换失败且恢复旧目录失败")?; }
            return Err(error.into());
        }
        if previous { std::fs::remove_dir_all(&backup).context("新版本已发布，但清理旧缓存失败")?; }
        Ok(())
    })();
    if staging.exists() { std::fs::remove_dir_all(&staging).context("清理插件准备目录失败")?; }
    result
}

fn clone_repo(url: &str, reference: Option<&str>, target: &Path) -> Result<()> {
    replace_directory(target, |staging| clone_repo_into(url, reference, staging))
}

fn clone_repo_into(url: &str, reference: Option<&str>, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    let target_display = target.display().to_string();
    let mut args: Vec<&str> = vec!["clone", "--depth", "1"];
    let pinned_commit = reference.filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if pinned_commit.is_some() {
        // Fetch the requested commit before touching the worktree. The current
        // default branch may contain unrelated files with much longer paths.
        args.push("--no-checkout");
    }
    if let Some(reference) = reference.filter(|value| !value.is_empty() && pinned_commit.is_none()) {
        args.push("--branch");
        args.push(reference);
    }
    args.push("--");
    args.push(url);
    args.push(&target_display);
    run_git(&args)?;
    #[cfg(target_os = "windows")]
    run_git(&["-C", &target_display, "config", "--local", "core.longpaths", "true"])?;
    if let Some(revision) = pinned_commit {
        run_git(&["-C", &target_display, "fetch", "--depth", "1", "origin", revision])?;
        run_git(&["-C", &target_display, "checkout", "--detach", revision])?;
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            bail!("插件目录包含符号链接，拒绝复制外部内容：{}", entry.path().display());
        }
        if metadata.is_dir() {
            copy_dir_recursive(&entry.path(), &destination)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), destination)?;
        } else {
            bail!("插件目录包含不支持的文件类型：{}", entry.path().display());
        }
    }
    Ok(())
}

/// 解析用户输入的市场源：本地绝对路径直接引用，owner/repo 补全 GitHub，
/// 其余按 git URL 处理。
fn classify_source(source: &str) -> Result<(String, PathBuf, Option<String>)> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        bail!("市场源不能为空");
    }
    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() && candidate.is_dir() {
        return Ok(("local".into(), candidate, None));
    }
    if trimmed.contains("://") || trimmed.starts_with("git@") {
        return Ok(("git".into(), PathBuf::new(), Some(trimmed.to_string())));
    }
    let shorthand = regex_lite(trimmed);
    if let Some((owner, repo)) = shorthand {
        return Ok((
            "git".into(),
            PathBuf::new(),
            Some(format!("https://github.com/{owner}/{repo}.git")),
        ));
    }
    bail!("无法识别的市场源：支持本地绝对路径、git URL 或 owner/repo")
}

/// owner/repo 简写校验（不含协议、单层斜杠、字符受限）。
fn regex_lite(value: &str) -> Option<(&str, &str)> {
    let (owner, repo) = value.split_once('/')?;
    if owner.is_empty()
        || repo.is_empty()
        || repo.contains('/')
        || value.contains("://")
        || !owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || !repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return None;
    }
    Some((owner, repo))
}

fn ensure_within(root: &Path, target: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let canonical = target
        .canonicalize()
        .with_context(|| format!("市场内路径不存在: {}", target.display()))?;
    if canonical.starts_with(&root) {
        Ok(canonical)
    } else {
        bail!("插件来源路径越出了市场仓库范围")
    }
}

// ---- 市场管理 ----

fn add_marketplace_at(paths: &Paths, source: &str) -> Result<MarketplaceView> {
    let (kind, local_path, url) = classify_source(source)?;
    std::fs::create_dir_all(&paths.market_root)?;
    std::fs::create_dir_all(&paths.cache_root)?;

    let (record_path, resolved_url): (PathBuf, Option<String>) = if kind == "local" {
        (local_path.clone(), None)
    } else {
        let url = url.expect("git 源必有 url");
        let temp = paths
            .market_root
            .join(format!("tmp-{}", uuid::Uuid::new_v4()));
        clone_repo(&url, None, &temp).with_context(|| format!("克隆市场仓库失败：{url}"))?;
        // 先克隆到临时目录解析出市场 id，再落到正式位置。
        let (id, _, _) =
            parse_marketplace(&temp).with_context(|| format!("解析市场失败：{url}"))?;
        let final_path = paths.market_root.join(safe_dir_name(&id));
        replace_directory(&final_path, |staging| {
            std::fs::rename(&temp, staging)?;
            Ok(())
        })?;
        (final_path, Some(url))
    };

    let (id, owner_name, _) = parse_marketplace(&record_path)?;
    let now = now_millis();
    let mut file = load_file_at(&paths.data_file)?;
    let existing = file.marketplaces.iter_mut().find(|record| record.id == id);
    let record = match existing {
        Some(record) => {
            record.source = source.trim().to_string();
            record.source_kind = kind.clone();
            record.owner_name = owner_name.clone();
            record.path = record_path.display().to_string();
            record.url = resolved_url.clone();
            record.last_refreshed_at = Some(now);
            record.last_error = None;
            record.clone()
        }
        None => {
            let record = MarketplaceRecord {
                id: id.clone(),
                source: source.trim().to_string(),
                source_kind: kind.clone(),
                owner_name,
                path: record_path.display().to_string(),
                url: resolved_url,
                added_at: now,
                last_refreshed_at: Some(now),
                last_error: None,
                pinned_revision: None,
            };
            file.marketplaces.push(record.clone());
            record
        }
    };
    save_file_at(&paths.data_file, &file)?;
    view_of(&record)
}

fn refresh_marketplace_at(paths: &Paths, id: &str) -> Result<MarketplaceView> {
    let mut file = load_file_at(&paths.data_file)?;
    let record = file
        .marketplaces
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| anyhow!("未找到市场：{id}"))?
        .clone();
    drop(file);
    if record.pinned_revision.is_some() {
        // Pinned official sources do not follow upstream HEAD during refresh.
        return view_of(&record);
    }

    if record.source_kind == "git" {
        let url = record.url.clone().unwrap_or_default();
        let path = PathBuf::from(&record.path);
        replace_directory(&path, |staging| {
            clone_repo_into(&url, None, staging).with_context(|| format!("刷新市场失败：{url}"))?;
            let (actual_id, _, _) = parse_marketplace(staging)?;
            if actual_id != record.id { bail!("市场标识已变化，保留原市场：{}", record.id); }
            Ok(())
        })?;
    }

    let mut file = load_file_at(&paths.data_file)?;
    if let Some(record) = file.marketplaces.iter_mut().find(|record| record.id == id) {
        match parse_marketplace(Path::new(&record.path)) {
            Ok((_, owner_name, _)) => {
                record.owner_name = owner_name;
                record.last_error = None;
            }
            Err(error) => {
                record.last_error = Some(format!("{error:#}"));
            }
        }
        record.last_refreshed_at = Some(now_millis());
        let updated = record.clone();
        save_file_at(&paths.data_file, &file)?;
        return view_of(&updated);
    }
    bail!("市场记录已不存在：{id}")
}

fn remove_marketplace_at(paths: &Paths, skill_center: &SkillCenter, id: &str) -> Result<()> {
    let installed = load_file_at(&paths.data_file)?.installed.into_iter()
        .filter(|plugin| plugin.marketplace_id == id).collect::<Vec<_>>();
    for plugin in installed { uninstall_plugin_at(paths, skill_center, &plugin.id)?; }
    mutate_at(paths, skill_center, |file, paths, skill_center| {
        let record = file
            .marketplaces
            .iter()
            .find(|record| record.id == id)
            .ok_or_else(|| anyhow!("未找到市场：{id}"))?
            .clone();
        let installed: Vec<InstalledPlugin> = file
            .installed
            .iter()
            .filter(|plugin| plugin.marketplace_id == id)
            .cloned()
            .collect();
        for plugin in &installed {
            uninstall_routes(paths, skill_center, plugin)?;
        }
        file.installed.retain(|plugin| plugin.marketplace_id != id);
        file.marketplaces.retain(|record| record.id != id);
        if record.source_kind == "git" {
            let _ = std::fs::remove_dir_all(Path::new(&record.path));
        }
        Ok(())
    })
}

// ---- 插件安装 / 卸载 ----

fn safe_dir_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." || cleaned != value {
        use sha2::{Digest, Sha256};
        let prefix = if cleaned.is_empty() || cleaned == "." || cleaned == ".." { "market" } else { &cleaned };
        format!("{prefix}-{:x}", Sha256::digest(value.as_bytes()))
    } else {
        cleaned
    }
}

/// 识别插件目录组件：manifest、skills、.mcp.json / mcp.json、hooks、agents。
struct PluginComponents {
    version: Option<String>,
    description: Option<String>,
    skill_dirs: Vec<PathBuf>,
    mcp_servers: Option<serde_json::Map<String, Value>>,
    has_hooks: bool,
    has_agents: bool,
}

fn scan_plugin_components(root: &Path) -> Result<PluginComponents> {
    let (_, manifest) = compatibility::read_manifest(root)?;
    let version = manifest
        .as_ref()
        .and_then(|value| value.get("version"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let description = manifest
        .as_ref()
        .and_then(|value| value.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut skill_dirs = Vec::new();
    let skills_root = root.join("skills");
    if locate_skill(&root).is_some() {
        // 单 skill 插件：SKILL.md 直接在根目录。
        skill_dirs.push(root.to_path_buf());
    } else if skills_root.is_dir() {
        for entry in std::fs::read_dir(&skills_root)? {
            let path = entry?.path();
            if path.is_dir() && locate_skill(&path).is_some() {
                skill_dirs.push(path);
            }
        }
    }

    skill_dirs.sort();
    let mut mcp_servers: Option<serde_json::Map<String, Value>> = None;
    for name in [".mcp.json", "mcp.json"] {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        if let Ok(value) = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::new)
            .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(anyhow::Error::new))
        {
            mcp_servers = value.get("mcpServers").and_then(Value::as_object).cloned();
            break;
        }
    }

    let hooks_dir = root.join("hooks");
    let has_hooks = hooks_dir.join("hooks.json").is_file()
        || (hooks_dir.is_dir()
            && std::fs::read_dir(&hooks_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false));
    let agents_dir = root.join("agents");
    let has_agents = agents_dir.is_dir()
        && std::fs::read_dir(&agents_dir)
            .map(|entries| {
                entries.filter_map(|entry| entry.ok()).any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                })
            })
            .unwrap_or(false);

    Ok(PluginComponents {
        version,
        description,
        skill_dirs,
        mcp_servers,
        has_hooks,
        has_agents,
    })
}

fn locate_skill(root: &Path) -> Option<PathBuf> {
    ["SKILL.md", "skill.md"]
        .iter()
        .find(|name| root.join(name).is_file())
        .map(|name| root.join(name))
}

/// 物化插件到缓存目录并返回缓存路径。
fn materialize_plugin(
    paths: &Paths,
    market_path: &Path,
    plugin_name: &str,
    source: &PluginSourceRef,
) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let market_identity = market_path.canonicalize().unwrap_or_else(|_| market_path.to_path_buf());
    let digest = format!("{:x}", Sha256::digest(market_identity.to_string_lossy().as_bytes()));
    let market_name = market_path.file_name().and_then(|value| value.to_str()).unwrap_or("market");
    let target = paths.cache_root.join(format!("{}-{}", safe_dir_name(market_name), digest))
        .join(safe_dir_name(plugin_name));
    let source_dir: PathBuf = match source {
        PluginSourceRef::Relative(relative) => {
            let joined = market_path.join(relative);
            ensure_within(market_path, &joined)?
        }
        PluginSourceRef::Git {
            url,
            subpath,
            reference,
        } => {
            let temp = paths
                .cache_root
                .join(format!("tmp-{}", uuid::Uuid::new_v4()));
            clone_repo(url, reference.as_deref(), &temp)
                .with_context(|| format!("克隆插件仓库失败：{url}"))?;
            let picked = match subpath {
                Some(subpath) if !subpath.is_empty() => {
                    let joined = temp.join(subpath);
                    ensure_within(&temp, &joined)?
                }
                _ => temp.clone(),
            };
            let result = (|| {
                replace_directory(&target, |staging| copy_dir_recursive(&picked, staging))?;
                Ok(target.clone())
            })();
            let _ = std::fs::remove_dir_all(&temp);
            return result;
        }
        PluginSourceRef::Unsupported(reason) => bail!("该来源类型暂不支持安装：{reason}"),
    };
    if !source_dir.is_dir() {
        bail!("插件目录不存在: {}", source_dir.display());
    }
    let absolute_target = if target.exists() { target.canonicalize()? } else { target.clone() };
    let absolute_source = source_dir.canonicalize()?;
    if absolute_source.starts_with(&absolute_target) || absolute_target.starts_with(&absolute_source) {
        bail!("插件来源与缓存目录重叠，拒绝替换");
    }
    replace_directory(&target, |staging| copy_dir_recursive(&source_dir, staging))?;
    Ok(target)
}

/// 卸载路由：清 MCP 条目 + 撤下并删除 Skill 资产（部署过的先撤下）。
fn uninstall_routes(
    paths: &Paths,
    skill_center: &SkillCenter,
    plugin: &InstalledPlugin,
) -> Result<()> {
    mcp::remove_plugin_servers_at(&paths.mcp_file, &plugin.id)?;
    // Historical records have no deployment ownership. Preserve central assets
    // and manually deployed copies rather than deleting all matching skills.
    let _ = skill_center;
    Ok(())
}

fn install_plugin_at(
    paths: &Paths,
    skill_center: &SkillCenter,
    marketplace_id: &str,
    plugin_name: &str,
) -> Result<InstalledPlugin> {
    let file = load_file_at(&paths.data_file)?;
    let record = file
        .marketplaces
        .iter()
        .find(|record| record.id == marketplace_id)
        .ok_or_else(|| anyhow!("未找到市场：{marketplace_id}"))?
        .clone();
    let market_path = PathBuf::from(&record.path);
    let (_, _, entries) = parse_marketplace(&market_path)?;
    let (_, source) = entries
        .iter()
        .find(|(view, _)| view.name == plugin_name)
        .ok_or_else(|| anyhow!("市场中不存在插件：{plugin_name}"))?;

    let cache_path = materialize_plugin(paths, &market_path, plugin_name, source)?;
    let components = scan_plugin_components(&cache_path)?;
    let plugin_id = format!("{plugin_name}@{marketplace_id}");

    let mut skill_asset_ids = Vec::new();
    let mut skill_names = Vec::new();
    for skill_dir in &components.skill_dirs {
        let assets = skill_center.import(ImportSkillInput {
            source_type: "local".into(),
            source: skill_dir.display().to_string(),
            branch: None,
            subpath: None,
            revision: None,
        })?;
        if let Some(asset) = assets.last() {
            skill_asset_ids.push(asset.id.clone());
            skill_names.push(asset.name.clone());
        }
    }

    let mcp_server_names = components
        .mcp_servers
        .as_ref()
        .map(|servers| {
            mcp::import_plugin_servers_at(&paths.mcp_file, &plugin_id, plugin_name, servers)
        })
        .transpose()?
        .map(|imported| imported.into_iter().map(|server| server.name).collect())
        .unwrap_or_default();

    let installed = InstalledPlugin {
        id: plugin_id,
        marketplace_id: marketplace_id.to_string(),
        name: plugin_name.to_string(),
        version: components.version,
        description: components.description.unwrap_or_default(),
        cache_path: cache_path.display().to_string(),
        skill_asset_ids,
        skill_names,
        mcp_server_names,
        has_hooks: components.has_hooks,
        has_agents: components.has_agents,
        installed_at: now_millis(),
        targets: Vec::new(),
    };

    let mut file = load_file_at(&paths.data_file)?;
    file.installed.retain(|plugin| plugin.id != installed.id);
    file.installed.push(installed.clone());
    save_file_at(&paths.data_file, &file)?;
    Ok(installed)
}

fn uninstall_plugin_at(
    paths: &Paths,
    skill_center: &SkillCenter,
    installed_id: &str,
) -> Result<()> {
    let plugin = load_file_at(&paths.data_file)?.installed.into_iter()
        .find(|plugin| plugin.id == installed_id).ok_or_else(|| anyhow!("未找到已安装插件：{installed_id}"))?;
    for target in &plugin.targets {
        targets::remove_at(paths, skill_center, installed_id, &target.id)?;
    }
    mutate_at(paths, skill_center, |file, paths, skill_center| {
        let plugin = file
            .installed
            .iter()
            .find(|plugin| plugin.id == installed_id)
            .ok_or_else(|| anyhow!("未找到已安装插件：{installed_id}"))?
            .clone();
        uninstall_routes(paths, skill_center, &plugin)?;
        file.installed.retain(|plugin| plugin.id != installed_id);
        let cache = Path::new(&plugin.cache_path);
        if cache.exists() {
            ensure_within(&paths.cache_root, cache)?;
            std::fs::remove_dir_all(cache)?;
        }
        Ok(())
    })
}

fn snapshot_at(paths: &Paths) -> Result<PluginMarketSnapshot> {
    let file = load_file_at(&paths.data_file)?;
    Ok(PluginMarketSnapshot {
        marketplaces: file
            .marketplaces
            .iter()
            .map(view_of)
            .collect::<Result<Vec<_>>>()?,
        installed: file.installed,
        selection_configured: file.selected_plugin_ids.is_some(),
        selected_plugin_ids: file.selected_plugin_ids.unwrap_or_default(),
        installed_catalog: Vec::new(),
    })
}

// ---- 对外接口（默认路径） ----

pub fn add_marketplace(source: &str) -> Result<MarketplaceView> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    add_marketplace_at(&Paths::standard()?, source)
}

pub fn refresh_marketplace(id: &str) -> Result<MarketplaceView> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    refresh_marketplace_at(&Paths::standard()?, id)
}

pub fn remove_marketplace(skill_center: &SkillCenter, id: &str) -> Result<()> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    remove_marketplace_at(&Paths::standard()?, skill_center, id)
}

pub fn install_plugin(
    skill_center: &SkillCenter,
    marketplace_id: &str,
    plugin_name: &str,
) -> Result<InstalledPlugin> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    install_plugin_at(
        &Paths::standard()?,
        skill_center,
        marketplace_id,
        plugin_name,
    )
}

pub fn uninstall_plugin(skill_center: &SkillCenter, installed_id: &str) -> Result<()> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    uninstall_plugin_at(&Paths::standard()?, skill_center, installed_id)
}

pub fn snapshot() -> Result<PluginMarketSnapshot> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    snapshot_at(&Paths::standard()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_marketplaces_with_identical_basenames_do_not_share_cache() {
        let root = std::env::temp_dir().join(format!("plugin-cache-identity-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_config_dir(root.join("config"));
        let first = root.join("first/catalog"); let second = root.join("second/catalog");
        for market in [&first, &second] { std::fs::create_dir_all(market.join("plugin")).unwrap(); }
        std::fs::write(first.join("plugin/content.txt"), "first source").unwrap();
        std::fs::write(second.join("plugin/content.txt"), "second source").unwrap();
        let source = PluginSourceRef::Relative(PathBuf::from("./plugin"));
        let first_cache = materialize_plugin(&paths, &first, "demo", &source).unwrap();
        let second_cache = materialize_plugin(&paths, &second, "demo", &source).unwrap();
        assert_ne!(first_cache, second_cache);
        assert_eq!(std::fs::read_to_string(first_cache.join("content.txt")).unwrap(), "first source");
        assert_eq!(std::fs::read_to_string(second_cache.join("content.txt")).unwrap(), "second source");
        assert_ne!(safe_dir_name("插件甲"), safe_dir_name("插件乙"));
        assert_ne!(safe_dir_name("a/b"), safe_dir_name("a-b"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_clone_or_copy_preserves_the_previous_directory() {
        let root = std::env::temp_dir().join(format!("plugin-replacement-{}", uuid::Uuid::new_v4()));
        let target = root.join("installed");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), "previous version").unwrap();
        assert!(clone_repo(root.join("missing-repository").to_str().unwrap(), None, &target).is_err());
        assert_eq!(std::fs::read_to_string(target.join("keep.txt")).unwrap(), "previous version");
        assert!(replace_directory(&target, |staging| {
            std::fs::create_dir_all(staging)?;
            std::fs::write(staging.join("partial.txt"), "partial")?;
            bail!("fixture copy failure")
        }).is_err());
        assert_eq!(std::fs::read_to_string(target.join("keep.txt")).unwrap(), "previous version");
        assert!(!target.join("partial.txt").exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "staging files must be cleaned");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_copy_rejects_external_directory_links_without_losing_cache() {
        let root = std::env::temp_dir().join(format!("plugin-external-link-{}", uuid::Uuid::new_v4()));
        let source = root.join("source"); let outside = root.join("outside"); let target = root.join("cache");
        for directory in [&source, &outside, &target] { std::fs::create_dir_all(directory).unwrap(); }
        std::fs::write(outside.join("private.txt"), "private content").unwrap();
        std::fs::write(target.join("keep.txt"), "installed content").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, source.join("outside")).unwrap();
        #[cfg(windows)]
        junction::create(&outside, source.join("outside")).unwrap();
        assert!(replace_directory(&target, |staging| copy_dir_recursive(&source, staging)).is_err());
        assert_eq!(std::fs::read_to_string(target.join("keep.txt")).unwrap(), "installed content");
        assert!(!target.join("outside/private.txt").exists());
        #[cfg(windows)]
        junction::delete(source.join("outside")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }


    struct Fixture {
        root: PathBuf,
        paths: Paths,
        center: SkillCenter,
        market: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "crewvela-plugin-market-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            let paths = Paths::from_config_dir(root.clone());
            std::fs::create_dir_all(&paths.market_root).unwrap();
            std::fs::create_dir_all(&paths.cache_root).unwrap();

            // 本地市场（.claude-plugin/marketplace.json + 一个多组件插件）。
            let market = root.join("source-market");
            let marketplace_json = r#"{
                "name": "fixture-market",
                "owner": { "name": "Fixture 团队" },
                "metadata": { "pluginRoot": "./plugins" },
                "plugins": [
                    { "name": "demo", "source": "./plugins/demo",
                      "description": "多组件演示插件", "version": "1.2.0",
                      "author": { "name": "Demo 作者" }, "category": "productivity" },
                    { "name": "remote", "source": { "source": "git-subdir",
                      "url": "https://github.com/example/mono.git", "path": "plugins/remote" } },
                    { "name": "npm-one", "source": { "source": "npm", "package": "x" } }
                ]
            }"#;
            std::fs::create_dir_all(market.join(".claude-plugin")).unwrap();
            std::fs::write(
                market.join(".claude-plugin").join("marketplace.json"),
                marketplace_json,
            )
            .unwrap();
            let demo = market.join("plugins").join("demo");
            std::fs::create_dir_all(demo.join(".claude-plugin")).unwrap();
            std::fs::write(
                demo.join(".claude-plugin").join("plugin.json"),
                r#"{ "name": "demo", "version": "1.2.0", "description": "多组件演示插件" }"#,
            )
            .unwrap();
            std::fs::create_dir_all(demo.join("skills").join("greet")).unwrap();
            std::fs::write(
                demo.join("skills").join("greet").join("SKILL.md"),
                "---\nname: greet\ndescription: 打招呼\n---\n你好",
            )
            .unwrap();
            std::fs::create_dir_all(demo.join("skills").join("review")).unwrap();
            std::fs::write(
                demo.join("skills").join("review").join("SKILL.md"),
                "---\nname: review\ndescription: 审查\n---\n审查",
            )
            .unwrap();
            std::fs::write(
                demo.join(".mcp.json"),
                r#"{ "mcpServers": {
                    "fetch": { "command": "npx", "args": ["-y", "mcp-fetch"] },
                    "kb": { "type": "http", "url": "https://example.com/mcp" } } }"#,
            )
            .unwrap();
            std::fs::create_dir_all(demo.join("hooks")).unwrap();
            std::fs::write(demo.join("hooks").join("hooks.json"), "{}").unwrap();
            std::fs::create_dir_all(demo.join("agents")).unwrap();
            std::fs::write(demo.join("agents").join("helper.md"), "# helper").unwrap();

            let center = SkillCenter::open_at(root.join("skill-center")).unwrap();
            Self {
                root,
                paths,
                center,
                market,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn parses_marketplace_in_both_locations_with_source_variants() {
        let fixture = Fixture::new();
        let (id, owner, entries) = parse_marketplace(&fixture.market).unwrap();
        assert_eq!(id, "fixture-market");
        assert_eq!(owner, "Fixture 团队");
        assert_eq!(entries.len(), 3);

        let (demo, demo_source) = &entries[0];
        assert_eq!(demo.name, "demo");
        assert_eq!(demo.version.as_deref(), Some("1.2.0"));
        assert!(!demo.installable, "连接器需授权，候选不能宣称可直接安装");
        assert_eq!(demo.compatibility.as_ref().unwrap().status, "needs_auth");
        assert!(matches!(demo_source, PluginSourceRef::Relative(_)));

        let (remote, remote_source) = &entries[1];
        assert!(remote.installable);
        match remote_source {
            PluginSourceRef::Git { url, subpath, .. } => {
                assert_eq!(url, "https://github.com/example/mono.git");
                assert_eq!(subpath.as_deref(), Some("plugins/remote"));
            }
            _ => panic!("remote 应解析为 Git 来源"),
        }

        let (npm, npm_source) = &entries[2];
        assert!(!npm.installable);
        assert!(npm.note.is_some());
        assert!(matches!(npm_source, PluginSourceRef::Unsupported(_)));
    }

    #[test]
    fn agents_marketplace_location_is_supported() {
        let fixture = Fixture::new();
        let alt = fixture.root.join("alt-market");
        std::fs::create_dir_all(alt.join(".agents").join("plugins")).unwrap();
        std::fs::write(
            alt.join(".agents").join("plugins").join("marketplace.json"),
            r#"{ "name": "codex-style", "owner": {"name": "Codex"}, "plugins": [] }"#,
        )
        .unwrap();
        let (id, _, entries) = parse_marketplace(&alt).unwrap();
        assert_eq!(id, "codex-style");
        assert!(entries.is_empty());
    }

    #[test]
    fn bare_source_requires_plugin_root() {
        let value = serde_json::json!("demo");
        assert!(matches!(
            parse_entry_source(&value, None),
            PluginSourceRef::Unsupported(_)
        ));
        assert!(matches!(
            parse_entry_source(&value, Some("./plugins")),
            PluginSourceRef::Relative(_)
        ));
    }

    #[test]
    fn classify_source_supports_local_url_and_shorthand() {
        let fixture = Fixture::new();
        let (kind, path, url) =
            classify_source(fixture.market.display().to_string().as_str()).unwrap();
        assert_eq!(kind, "local");
        assert!(url.is_none());
        assert!(path.is_dir());

        let (kind, _, url) = classify_source("anthropics/claude-plugins-community").unwrap();
        assert_eq!(kind, "git");
        assert_eq!(
            url.as_deref(),
            Some("https://github.com/anthropics/claude-plugins-community.git")
        );

        let (kind, _, url) = classify_source("https://gitlab.com/x.git").unwrap();
        assert_eq!(kind, "git");
        assert_eq!(url.as_deref(), Some("https://gitlab.com/x.git"));

        assert!(classify_source("not a source").is_err());
    }

    #[test]
    fn install_routes_components_and_uninstall_cleans_up() {
        let fixture = Fixture::new();
        let view = add_marketplace_at(
            &fixture.paths,
            fixture.market.display().to_string().as_str(),
        )
        .unwrap();
        assert_eq!(view.id, "fixture-market");
        assert_eq!(view.plugins.len(), 3);
        assert!(view.last_error.is_none());

        let installed =
            install_plugin_at(&fixture.paths, &fixture.center, "fixture-market", "demo").unwrap();
        assert_eq!(installed.id, "demo@fixture-market");
        assert_eq!(
            installed.skill_names,
            vec!["greet".to_string(), "review".to_string()]
        );
        assert_eq!(installed.mcp_server_names.len(), 2);
        assert!(installed.has_hooks && installed.has_agents);
        assert!(Path::new(&installed.cache_path).join(".mcp.json").is_file());

        // skills 进入 Skill 中心。
        let center_snapshot = fixture.center.snapshot().unwrap();
        assert_eq!(
            center_snapshot
                .assets
                .iter()
                .filter(|asset| installed.skill_asset_ids.contains(&asset.id))
                .count(),
            2
        );
        // MCP 条目带插件前缀且可溯源。
        let servers = mcp::load_at(&fixture.paths.mcp_file).unwrap();
        assert_eq!(servers.len(), 2);
        assert!(servers
            .iter()
            .all(|server| server.source_plugin.as_deref() == Some("demo@fixture-market")));

        // 重装不产生重复路由。
        let reinstalled =
            install_plugin_at(&fixture.paths, &fixture.center, "fixture-market", "demo").unwrap();
        let servers = mcp::load_at(&fixture.paths.mcp_file).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(
            reinstalled.skill_asset_ids.len(),
            installed.skill_asset_ids.len()
        );

        // 卸载反向清理。
        uninstall_plugin_at(&fixture.paths, &fixture.center, &installed.id).unwrap();
        let after = snapshot_at(&fixture.paths).unwrap();
        assert!(after.installed.is_empty());
        assert!(mcp::load_at(&fixture.paths.mcp_file).unwrap().is_empty());
        let center_after = fixture.center.snapshot().unwrap();
        assert_eq!(center_after.assets.len(), 2, "历史记录缺少所有权，中央资产须保留");
        assert!(!Path::new(&installed.cache_path).exists());
    }

    #[test]
    fn legacy_uninstall_preserves_manual_deployments_and_central_assets() {
        let fixture = Fixture::new();
        add_marketplace_at(
            &fixture.paths,
            fixture.market.display().to_string().as_str(),
        )
        .unwrap();
        let installed =
            install_plugin_at(&fixture.paths, &fixture.center, "fixture-market", "demo").unwrap();
        let project = fixture.root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let deploy = fixture
            .center
            .deploy_batch(vec![crate::skill_center::DeploySkillInput {
                skill_id: installed.skill_asset_ids[0].clone(),
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            }])
            .unwrap();
        assert_eq!(deploy.len(), 1);

        uninstall_plugin_at(&fixture.paths, &fixture.center, &installed.id).unwrap();
        let center_after = fixture.center.snapshot().unwrap();
        assert_eq!(center_after.assets.len(), 2);
        assert_eq!(center_after.deployments.len(), 1);
        assert!(Path::new(&deploy[0].target_path).join("SKILL.md").is_file());
    }

    #[test]
    fn removing_marketplace_uninstalls_its_plugins() {
        let fixture = Fixture::new();
        add_marketplace_at(
            &fixture.paths,
            fixture.market.display().to_string().as_str(),
        )
        .unwrap();
        install_plugin_at(&fixture.paths, &fixture.center, "fixture-market", "demo").unwrap();

        remove_marketplace_at(&fixture.paths, &fixture.center, "fixture-market").unwrap();

        let after = snapshot_at(&fixture.paths).unwrap();
        assert!(after.marketplaces.is_empty());
        assert!(after.installed.is_empty());
        assert!(mcp::load_at(&fixture.paths.mcp_file).unwrap().is_empty());
        // 本地源市场的原始目录不被删除。
        assert!(fixture.market.is_dir());
    }

    #[test]
    fn unsupported_source_rejected_on_install() {
        let fixture = Fixture::new();
        add_marketplace_at(
            &fixture.paths,
            fixture.market.display().to_string().as_str(),
        )
        .unwrap();
        let error = install_plugin_at(&fixture.paths, &fixture.center, "fixture-market", "npm-one")
            .unwrap_err();
        assert!(error.to_string().contains("暂不支持"));
    }

}
