//! 一键启动预设：把「provider + 目录组合 + YOLO/Auto + worktree + 初始 Prompt」
//! 存成具名预设，rail 新建菜单一键拉起。无状态函数式 API（仿 projects.rs），
//! 持久化 `%APPDATA%\crewvela\presets.json`。
//! 预设只存参数不存 key——启动时仍走 launch_multi 的 keychain 读取路径。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const PRESETS_VERSION: u32 = 1;
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPreset {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    /// 每个元素对应一个会话的目录（空串 = home 默认）；会话数 = 数组长度。
    pub cwds: Vec<String>,
    pub yolo: bool,
    pub auto_mode: bool,
    pub use_worktree: bool,
    #[serde(default)]
    pub initial_prompt: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetsFile {
    version: u32,
    presets: Vec<LaunchPreset>,
}

impl Default for PresetsFile {
    fn default() -> Self {
        Self {
            version: PRESETS_VERSION,
            presets: Vec::new(),
        }
    }
}

pub fn presets_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("presets.json"))
}

fn load_from(path: &std::path::Path) -> Result<PresetsFile> {
    if !path.exists() {
        return Ok(PresetsFile::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("读取预设配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("解析预设配置失败: {}", path.display()))
}

fn save_to(path: &std::path::Path, file: &PresetsFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file)?;
    crate::providers::write_atomic(path, json.as_bytes())
        .with_context(|| format!("写入预设配置失败: {}", path.display()))
}

pub fn list() -> Result<Vec<LaunchPreset>> {
    Ok(load_from(&presets_path()?)?.presets)
}

/// 新建预设的参数（id/时间戳由 create 生成）。
pub struct NewPreset {
    pub name: String,
    pub provider_id: String,
    pub cwds: Vec<String>,
    pub yolo: bool,
    pub auto_mode: bool,
    pub use_worktree: bool,
    pub initial_prompt: Option<String>,
}

fn create_at(path: &std::path::Path, input: NewPreset) -> Result<LaunchPreset> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(anyhow!("预设名称不能为空"));
    }
    if name.chars().count() > 40 {
        return Err(anyhow!("预设名称不能超过 40 个字符"));
    }
    if crate::providers::get(&input.provider_id)?.is_none() {
        return Err(anyhow!("未找到 provider: {}", input.provider_id));
    }
    if input.yolo && input.auto_mode {
        return Err(anyhow!("Auto 模式与 YOLO 模式不能同时启用"));
    }
    let cwds: Vec<String> = input
        .cwds
        .iter()
        .map(|cwd| cwd.trim().to_string())
        .collect();
    if cwds.is_empty() {
        return Err(anyhow!("预设至少需要一个会话目录"));
    }
    let initial_prompt = input
        .initial_prompt
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty());

    let _guard = STORE_LOCK.lock().map_err(|_| anyhow!("预设配置锁不可用"))?;
    let mut file = load_from(path)?;
    if file.presets.iter().any(|preset| preset.name == name) {
        return Err(anyhow!("已存在同名预设: {name}"));
    }
    let now = now_millis();
    let preset = LaunchPreset {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        provider_id: input.provider_id,
        cwds,
        yolo: input.yolo,
        auto_mode: input.auto_mode,
        use_worktree: input.use_worktree,
        initial_prompt,
        created_at: now,
        updated_at: now,
    };
    file.presets.push(preset.clone());
    save_to(path, &file)?;
    Ok(preset)
}

pub fn create(input: NewPreset) -> Result<LaunchPreset> {
    create_at(&presets_path()?, input)
}

fn delete_at(path: &std::path::Path, id: &str) -> Result<()> {
    let _guard = STORE_LOCK.lock().map_err(|_| anyhow!("预设配置锁不可用"))?;
    let mut file = load_from(path)?;
    let before = file.presets.len();
    file.presets.retain(|preset| preset.id != id);
    if file.presets.len() == before {
        return Err(anyhow!("未找到预设: {id}"));
    }
    save_to(path, &file)
}

pub fn delete(id: &str) -> Result<()> {
    delete_at(&presets_path()?, id)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_concurrent_preset_creates_preserve_every_successful_write() {
        let (root, store) = test_store("concurrent");
        let barrier = std::sync::Barrier::new(16);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16).map(|i| {
                let barrier = &barrier;
                let store = &store;
                scope.spawn(move || { barrier.wait(); create_at(store, new_preset(&format!("preset-{i}"))) })
            }).collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        let stored = load_from(&store);
        fs::remove_dir_all(root).unwrap();
        assert!(results.iter().all(Result::is_ok), "concurrent saves failed: {results:?}");
        assert_eq!(stored.unwrap().presets.len(), 16, "successful saves must not overwrite other presets");
    }

    fn test_store(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "crewvela-presets-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        (root.clone(), root.join("presets.json"))
    }

    fn new_preset(name: &str) -> NewPreset {
        NewPreset {
            name: name.to_string(),
            provider_id: "glm".to_string(), // 内置 provider，一定存在
            cwds: vec!["F:/a".to_string(), "F:/b".to_string()],
            yolo: false,
            auto_mode: true,
            use_worktree: false,
            initial_prompt: None,
        }
    }

    #[test]
    fn create_and_list_roundtrip() {
        let (root, store) = test_store("roundtrip");
        let preset = create_at(&store, new_preset("晨会巡检")).unwrap();
        assert_eq!(preset.name, "晨会巡检");
        assert_eq!(preset.cwds.len(), 2);

        // 空白 Prompt 归一为 None；空白目录 trim 后保留（空串 = home）。
        let with_prompt = create_at(
            &store,
            NewPreset {
                initial_prompt: Some("  ".to_string()),
                cwds: vec!["  ".to_string()],
                ..new_preset("home 会话")
            },
        )
        .unwrap();
        assert_eq!(with_prompt.initial_prompt, None);
        assert_eq!(with_prompt.cwds, vec![String::new()]);

        let listed = load_from(&store).unwrap().presets;
        assert_eq!(listed.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_rejects_invalid_input() {
        let (root, store) = test_store("reject");
        assert!(create_at(&store, new_preset("  ")).is_err());
        assert!(create_at(
            &store,
            NewPreset {
                provider_id: "not-exists".to_string(),
                ..new_preset("x")
            }
        )
        .unwrap_err()
        .to_string()
        .contains("未找到 provider"));
        assert!(create_at(
            &store,
            NewPreset {
                yolo: true,
                auto_mode: true,
                ..new_preset("x")
            }
        )
        .is_err());
        assert!(create_at(
            &store,
            NewPreset {
                cwds: vec![],
                ..new_preset("x")
            }
        )
        .is_err());

        create_at(&store, new_preset("dup")).unwrap();
        assert!(create_at(&store, new_preset("dup"))
            .unwrap_err()
            .to_string()
            .contains("同名预设"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_preset() {
        let (root, store) = test_store("delete");
        let preset = create_at(&store, new_preset("a")).unwrap();
        delete_at(&store, &preset.id).unwrap();
        assert!(load_from(&store).unwrap().presets.is_empty());
        assert!(delete_at(&store, &preset.id).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}
