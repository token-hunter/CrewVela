use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const PROJECTS_VERSION: u32 = 1;

/// 项目徽标配色板：按创建顺序循环取用，用户无需关心配色。
pub const BADGE_PALETTE: [&str; 8] = [
    "#5b8def", "#9b6dd6", "#4caf7d", "#e0a458", "#e06c75", "#56b6c2", "#d65f9e", "#7c8cff",
];

/// 项目 = 一个本地目录（参考 orca 的 Project 裁剪而来：砍掉 GitHub 集成、
/// 多 host、worktree 管理，只保留「目录 + 名称 + 徽标色」）。
/// 会话通过 SessionRecord.project_id 归属项目，侧栏按项目分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub badge_color: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectsFile {
    version: u32,
    projects: Vec<Project>,
}

impl Default for ProjectsFile {
    fn default() -> Self {
        Self {
            version: PROJECTS_VERSION,
            projects: Vec::new(),
        }
    }
}

pub fn projects_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("projects.json"))
}

fn load_from(path: &std::path::Path) -> Result<ProjectsFile> {
    if !path.exists() {
        return Ok(ProjectsFile::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("读取项目配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("解析项目配置失败: {}", path.display()))
}

fn save_to(path: &std::path::Path, file: &ProjectsFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file)?;
    fs::write(path, json).with_context(|| format!("写入项目配置失败: {}", path.display()))
}

pub fn list() -> Result<Vec<Project>> {
    Ok(load_from(&projects_path()?)?.projects)
}

pub fn get(id: &str) -> Result<Option<Project>> {
    Ok(list()?.into_iter().find(|project| project.id == id))
}

/// 目录 basename 作为默认项目名；取不到（如盘符根目录）回退完整路径。
fn default_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn create_at(path: &std::path::Path, name: Option<&str>, dir: &str) -> Result<Project> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Err(anyhow!("项目目录不能为空"));
    }
    let dir_path = std::path::Path::new(dir);
    if !dir_path.is_dir() {
        return Err(anyhow!("项目目录不存在: {dir}"));
    }
    let name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_name(dir));

    let mut file = load_from(path)?;
    if file.projects.iter().any(|project| project.path == dir) {
        return Err(anyhow!("该目录已添加为项目: {dir}"));
    }
    let now = now_millis();
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: dir.to_string(),
        badge_color: BADGE_PALETTE[file.projects.len() % BADGE_PALETTE.len()].to_string(),
        created_at: now,
        updated_at: now,
    };
    file.projects.push(project.clone());
    save_to(path, &file)?;
    Ok(project)
}

pub fn create(name: Option<&str>, dir: &str) -> Result<Project> {
    create_at(&projects_path()?, name, dir)
}

fn update_at(
    path: &std::path::Path,
    id: &str,
    name: Option<&str>,
    badge_color: Option<&str>,
) -> Result<Project> {
    let mut file = load_from(path)?;
    let project = file
        .projects
        .iter_mut()
        .find(|project| project.id == id)
        .ok_or_else(|| anyhow!("未找到项目: {id}"))?;
    if let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) {
        project.name = name.to_string();
    }
    if let Some(color) = badge_color.map(str::trim).filter(|value| !value.is_empty()) {
        project.badge_color = color.to_string();
    }
    project.updated_at = now_millis();
    let updated = project.clone();
    save_to(path, &file)?;
    Ok(updated)
}

pub fn update(id: &str, name: Option<&str>, badge_color: Option<&str>) -> Result<Project> {
    update_at(&projects_path()?, id, name, badge_color)
}

fn delete_at(path: &std::path::Path, id: &str) -> Result<()> {
    let mut file = load_from(path)?;
    let before = file.projects.len();
    file.projects.retain(|project| project.id != id);
    if file.projects.len() == before {
        return Err(anyhow!("未找到项目: {id}"));
    }
    save_to(path, &file)
}

/// 删除项目只解除分组归属，不触碰目录与会话本体
/// （会话的 project_id 由调用方经 SessionManager::clear_project 清理）。
pub fn delete(id: &str) -> Result<()> {
    delete_at(&projects_path()?, id)
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

    fn test_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "crewvela-projects-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn create_uses_dir_basename_and_cycles_palette() {
        let root = test_dir("create");
        let store = root.join("projects.json");
        let dir_a = root.join("alpha");
        let dir_b = root.join("beta");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();

        let first = create_at(&store, None, dir_a.to_str().unwrap()).unwrap();
        assert_eq!(first.name, "alpha");
        assert_eq!(first.badge_color, BADGE_PALETTE[0]);

        let second = create_at(&store, Some(" 自定义名字 "), dir_b.to_str().unwrap()).unwrap();
        assert_eq!(second.name, "自定义名字");
        assert_eq!(second.badge_color, BADGE_PALETTE[1]);

        let listed = load_from(&store).unwrap().projects;
        assert_eq!(listed.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_rejects_duplicate_and_missing_dirs() {
        let root = test_dir("reject");
        let store = root.join("projects.json");
        let dir = root.join("demo");
        fs::create_dir_all(&dir).unwrap();

        create_at(&store, None, dir.to_str().unwrap()).unwrap();
        let duplicated = create_at(&store, None, dir.to_str().unwrap()).unwrap_err();
        assert!(duplicated.to_string().contains("已添加为项目"));

        let missing = root.join("not-exists");
        let err = create_at(&store, None, missing.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("不存在"));

        assert!(create_at(&store, None, "   ").is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_renames_and_recolors() {
        let root = test_dir("update");
        let store = root.join("projects.json");
        let dir = root.join("demo");
        fs::create_dir_all(&dir).unwrap();
        let project = create_at(&store, None, dir.to_str().unwrap()).unwrap();

        let updated = update_at(&store, &project.id, Some("新名字"), Some("#123456")).unwrap();
        assert_eq!(updated.name, "新名字");
        assert_eq!(updated.badge_color, "#123456");
        assert!(updated.updated_at >= project.updated_at);

        // 空值不覆盖既有字段
        let untouched = update_at(&store, &project.id, Some("  "), None).unwrap();
        assert_eq!(untouched.name, "新名字");
        assert_eq!(untouched.badge_color, "#123456");

        assert!(update_at(&store, "unknown-id", Some("x"), None).is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_project_and_survives_reopen() {
        let root = test_dir("delete");
        let store = root.join("projects.json");
        let dir = root.join("demo");
        fs::create_dir_all(&dir).unwrap();
        let project = create_at(&store, None, dir.to_str().unwrap()).unwrap();

        delete_at(&store, &project.id).unwrap();
        assert!(load_from(&store).unwrap().projects.is_empty());
        assert!(delete_at(&store, &project.id).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}
