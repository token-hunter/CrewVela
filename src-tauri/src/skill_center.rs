use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod updates;
pub use updates::{SkillUpdateCheck, SkillVersionInfo};

const COPY_MARKER: &str = ".crewvela-skill.json";
const LEGACY_COPY_MARKER_SHA256: &str = "aa9f74d1a53acaff83168d6f4bbf77dea367ebf3a3b9d9bc4f82e5ff8d81c15e";
const LEGACY_MANAGED_BY_SHA256: &str = "e3946634d41c620eba19d52341225a0f0a2c3c58ee487584f654bc4ca4a1c341";
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 2_048;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSkillAdapter {
    pub runtime: String,
    pub label: String,
    pub global_root: String,
    pub project_root: String,
    pub discovery_roots: Vec<String>,
    pub recursive_scan: bool,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillAsset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_branch: Option<String>,
    pub source_subpath: Option<String>,
    pub source_revision: Option<String>,
    pub central_path: String,
    pub content_hash: String,
    pub trust_state: String,
    pub file_count: u32,
    pub script_count: u32,
    pub risk_summary: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDeployment {
    pub id: String,
    pub skill_id: String,
    pub runtime: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub target_path: String,
    pub mode: String,
    pub status: String,
    pub source_hash: String,
    pub target_hash: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSetItem {
    pub skill_id: String,
    pub runtime: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSet {
    pub id: String,
    pub name: String,
    pub description: String,
    pub project_path: Option<String>,
    pub agent_role: Option<String>,
    pub items: Vec<SkillSetItem>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCenterSnapshot {
    pub assets: Vec<SkillAsset>,
    pub deployments: Vec<SkillDeployment>,
    pub skill_sets: Vec<SkillSet>,
    pub runtimes: Vec<RuntimeSkillAdapter>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSkill {
    pub runtime: String,
    pub path: String,
    pub name: String,
    pub description: String,
    pub content_hash: String,
    pub already_managed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    pub path: String,
    pub size: u64,
    pub is_script: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPreview {
    pub skill_id: String,
    pub skill_md: String,
    pub files: Vec<SkillFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkillInput {
    pub source_type: String,
    pub source: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub subpath: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploySkillInput {
    pub skill_id: String,
    pub runtime: String,
    pub scope: String,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub trust_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSkillSetInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub agent_role: Option<String>,
    pub items: Vec<SkillSetItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketRepository {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub enabled: bool,
    pub builtin: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSkillMarketRepositoryInput {
    #[serde(default)]
    pub id: Option<String>,
    pub owner: String,
    pub name: String,
    #[serde(default = "default_market_branch")]
    pub branch: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketItem {
    pub key: String,
    pub name: String,
    pub description: String,
    pub source_type: String,
    pub source_url: String,
    pub repository_id: String,
    pub repository_label: String,
    pub branch: String,
    pub subpath: String,
    pub readme_url: String,
    pub installs: Option<u64>,
    pub content_hash: Option<String>,
    pub installed_asset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarketBrowseResult {
    pub skills: Vec<SkillMarketItem>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiResponse {
    pub query: String,
    pub skills: Vec<SkillsShApiSkill>,
    pub count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct SkillsShApiSkill {
    pub id: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    pub installs: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShSearchResult {
    pub skills: Vec<SkillMarketItem>,
    pub total_count: usize,
    pub query: String,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSkillMarketInput {
    pub source_type: String,
    pub source_url: String,
    pub branch: String,
    pub subpath: String,
    pub name: String,
    #[serde(default)]
    pub key: Option<String>,
}

pub struct SkillCenter {
    conn: Mutex<Connection>,
    import_lock: Mutex<()>,
    deployment_lock: Mutex<()>,
    root: PathBuf,
}

impl SkillCenter {
    pub fn new() -> Result<Self> {
        let root = crate::providers::config_dir()?.join("skill-center");
        Self::open_at(root)
    }

    pub(crate) fn open_at(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("assets"))?;
        fs::create_dir_all(root.join("tmp"))?;
        let conn = Connection::open(root.join("skill-center.db"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS skill_assets (
               id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
               source_type TEXT NOT NULL, source_ref TEXT, source_branch TEXT, source_subpath TEXT,
               source_revision TEXT, central_path TEXT NOT NULL UNIQUE, content_hash TEXT NOT NULL,
               trust_state TEXT NOT NULL DEFAULT 'unknown', file_count INTEGER NOT NULL DEFAULT 0,
               script_count INTEGER NOT NULL DEFAULT 0, risk_summary TEXT NOT NULL DEFAULT '[]',
               created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS skill_deployments (
               id TEXT PRIMARY KEY, identity_key TEXT NOT NULL UNIQUE,
               skill_id TEXT NOT NULL REFERENCES skill_assets(id) ON DELETE CASCADE,
               runtime TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT, task_id TEXT, agent_id TEXT,
               target_path TEXT NOT NULL, mode TEXT NOT NULL, status TEXT NOT NULL,
               source_hash TEXT NOT NULL, target_hash TEXT, last_error TEXT, updated_at INTEGER NOT NULL,
               UNIQUE(skill_id, runtime, scope, project_path, task_id, agent_id)
             );
             CREATE TABLE IF NOT EXISTS skill_sets (
               id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
               project_path TEXT, agent_role TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS skill_set_items (
               set_id TEXT NOT NULL REFERENCES skill_sets(id) ON DELETE CASCADE,
               skill_id TEXT NOT NULL REFERENCES skill_assets(id) ON DELETE CASCADE,
               runtime TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT, task_id TEXT, agent_id TEXT,
               PRIMARY KEY(set_id, skill_id, runtime, scope, project_path, task_id, agent_id)
             );",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skill_market_repositories (
               id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL, branch TEXT NOT NULL,
               enabled INTEGER NOT NULL DEFAULT 1, builtin INTEGER NOT NULL DEFAULT 0,
               created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
               UNIQUE(owner, name, branch)
             );",
        )?;
        let seeded_at = now_ts();
        for repository in default_market_repositories() {
            conn.execute(
                "INSERT OR IGNORE INTO skill_market_repositories(
                   id,owner,name,branch,enabled,builtin,created_at,updated_at
                 ) VALUES(?1,?2,?3,?4,1,1,?5,?5)",
                params![
                    repository.id,
                    repository.owner,
                    repository.name,
                    repository.branch,
                    seeded_at
                ],
            )?;
        }

        let has_identity_key = {
            let mut statement = conn.prepare("PRAGMA table_info(skill_deployments)")?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            columns.iter().any(|column| column == "identity_key")
        };
        if !has_identity_key {
            conn.execute(
                "ALTER TABLE skill_deployments ADD COLUMN identity_key TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        conn.execute(
            "UPDATE skill_deployments SET identity_key = skill_id || '|' || runtime || '|' || scope || '|' ||
             COALESCE(project_path,'') || '|' || COALESCE(task_id,'') || '|' || COALESCE(agent_id,'')
             WHERE identity_key=''",
            [],
        )?;
        conn.execute(
            "DELETE FROM skill_deployments WHERE rowid NOT IN (
               SELECT MAX(rowid) FROM skill_deployments GROUP BY identity_key
             )",
            [],
        )?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_skill_assets_content_hash;
             CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_deployment_identity ON skill_deployments(identity_key);",
        )?;
        updates::initialize(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            import_lock: Mutex::new(()),
            deployment_lock: Mutex::new(()),
            root,
        })
    }

    pub fn snapshot(&self) -> Result<SkillCenterSnapshot> {
        let _guard = self.deployment_lock.lock().unwrap();
        self.refresh_asset_hashes()?;
        self.refresh_deployment_statuses()?;
        Ok(SkillCenterSnapshot {
            assets: self.list_assets()?,
            deployments: self.list_deployments()?,
            skill_sets: self.list_skill_sets()?,
            runtimes: runtime_adapters()?,
        })
    }

    pub fn list_market_repositories(&self) -> Result<Vec<SkillMarketRepository>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT id,owner,name,branch,enabled,builtin,updated_at
             FROM skill_market_repositories
             ORDER BY builtin DESC, owner COLLATE NOCASE, name COLLATE NOCASE",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(SkillMarketRepository {
                    id: row.get(0)?,
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    builtin: row.get::<_, i64>(5)? != 0,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_market_repository(
        &self,
        input: SaveSkillMarketRepositoryInput,
    ) -> Result<SkillMarketRepository> {
        let owner = input.owner.trim();
        let name = input.name.trim().trim_end_matches(".git");
        let branch = input.branch.trim();
        validate_market_repo_coordinate(owner, name, branch)?;
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_ts();
        let builtin = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT builtin FROM skill_market_repositories WHERE id=?1",
                [&id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        self.conn.lock().unwrap().execute(
            "INSERT INTO skill_market_repositories(id,owner,name,branch,enabled,builtin,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(id) DO UPDATE SET
               owner=excluded.owner,name=excluded.name,branch=excluded.branch,
               enabled=excluded.enabled,updated_at=excluded.updated_at",
            params![
                id,
                owner,
                name,
                branch,
                if input.enabled { 1 } else { 0 },
                builtin,
                now
            ],
        )?;
        self.market_repository(&id)?
            .ok_or_else(|| anyhow!("保存 Skill 仓库失败"))
    }

    pub fn delete_market_repository(&self, id: &str) -> Result<()> {
        let repository = self
            .market_repository(id)?
            .ok_or_else(|| anyhow!("Skill 仓库不存在"))?;
        if repository.builtin {
            bail!("内置仓库不能删除，可以在仓库管理中停用");
        }
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM skill_market_repositories WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn browse_market(&self) -> Result<SkillMarketBrowseResult> {
        let repositories = self
            .list_market_repositories()?
            .into_iter()
            .filter(|repository| repository.enabled)
            .collect::<Vec<_>>();
        let assets = self.list_assets()?;
        let mut skills = Vec::new();
        let mut errors = Vec::new();
        for repository in repositories {
            let staging = self.root.join("tmp").join(Uuid::new_v4().to_string());
            if let Err(error) = fs::create_dir_all(&staging) {
                errors.push(format!("{}/{}：{error}", repository.owner, repository.name));
                continue;
            }
            let source_url = market_repository_url(&repository.owner, &repository.name);
            let input = ImportSkillInput {
                source_type: "git".into(),
                source: source_url.clone(),
                branch: Some(repository.branch.clone()),
                subpath: None,
                revision: None,
            };
            let result = (|| -> Result<Vec<SkillMarketItem>> {
                clone_git_source(&input, &staging)?;
                let root = staging.join("repository");
                let mut roots = find_skill_roots_recursive(&root)?;
                roots.sort();
                let mut discovered = Vec::new();
                for skill_root in roots.into_iter().take(500) {
                    let (name, description) = read_skill_metadata(&skill_root)?;
                    let content_hash = hash_directory(&skill_root)?;
                    let relative = skill_root
                        .strip_prefix(&root)?
                        .to_string_lossy()
                        .replace('\\', "/");
                    let installed_asset_id = assets
                        .iter()
                        .find(|asset| asset.content_hash == content_hash)
                        .map(|asset| asset.id.clone());
                    discovered.push(SkillMarketItem {
                        key: format!("{}:{relative}", repository.id),
                        name,
                        description,
                        source_type: "market".into(),
                        source_url: source_url.clone(),
                        repository_id: repository.id.clone(),
                        repository_label: format!("{}/{}", repository.owner, repository.name),
                        branch: repository.branch.clone(),
                        subpath: relative.clone(),
                        readme_url: format!(
                            "https://github.com/{}/{}/tree/{}/{}",
                            repository.owner, repository.name, repository.branch, relative
                        ),
                        installs: None,
                        content_hash: Some(content_hash),
                        installed_asset_id,
                    });
                }
                Ok(discovered)
            })();
            match result {
                Ok(mut items) => skills.append(&mut items),
                Err(error) => errors.push(format!(
                    "{}/{}：{}",
                    repository.owner, repository.name, error
                )),
            }
            let _ = fs::remove_dir_all(staging);
        }
        skills.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.repository_label.cmp(&right.repository_label))
        });
        Ok(SkillMarketBrowseResult { skills, errors })
    }

    pub async fn search_skills_sh(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SkillsShSearchResult> {
        let query = query.trim();
        if query.chars().count() < 2 {
            bail!("skills.sh 搜索至少输入 2 个字符");
        }
        let limit = limit.clamp(1, 50);
        let offset = offset.min(5_000);
        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .build()?
            .get("https://skills.sh/api/search")
            .query(&[
                ("q", query.to_string()),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<SkillsShApiResponse>()
            .await?;
        let has_more = response.skills.len() == limit;
        let assets = self.list_assets()?;
        let mut skills = Vec::new();
        for skill in response.skills {
            let Some((owner, name)) = skill.source.split_once('/') else {
                continue;
            };
            if name.contains('/') || validate_market_repo_coordinate(owner, name, "main").is_err() {
                continue;
            }
            let source_url = market_repository_url(owner, name);
            let installed_asset_id = assets
                .iter()
                .find(|asset| {
                    asset.source_type == "skills_sh"
                        && asset.source_ref.as_deref().is_some_and(|value| {
                            normalize_git_source(value) == normalize_git_source(&source_url)
                        })
                        && (asset.name.eq_ignore_ascii_case(&skill.name)
                            || asset.source_subpath.as_deref().is_some_and(|value| {
                                Path::new(value)
                                    .file_name()
                                    .and_then(|part| part.to_str())
                                    .is_some_and(|part| part.eq_ignore_ascii_case(&skill.skill_id))
                            }))
                })
                .map(|asset| asset.id.clone());
            skills.push(SkillMarketItem {
                key: skill.id,
                name: skill.name,
                description: String::new(),
                source_type: "skills_sh".into(),
                source_url,
                repository_id: format!("{owner}/{name}"),
                repository_label: format!("{owner}/{name}"),
                branch: "main".into(),
                subpath: skill.skill_id,
                readme_url: format!("https://github.com/{owner}/{name}"),
                installs: Some(skill.installs),
                content_hash: None,
                installed_asset_id,
            });
        }
        Ok(SkillsShSearchResult {
            skills,
            total_count: response.count,
            query: response.query,
            has_more,
        })
    }

    pub fn install_market(&self, input: InstallSkillMarketInput) -> Result<SkillAsset> {
        if !matches!(input.source_type.as_str(), "market" | "skills_sh") {
            bail!("不支持的市场来源：{}", input.source_type);
        }
        if input.source_type == "skills_sh"
            && input
                .key
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("skills.sh 安装缺少唯一 key");
        }
        validate_github_source_url(&input.source_url)?;
        validate_git_ref(&input.branch)?;
        let _import_guard = self.import_lock.lock().unwrap();
        let staging = self.root.join("tmp").join(Uuid::new_v4().to_string());
        fs::create_dir_all(&staging)?;
        let result = (|| {
            let clone_input = ImportSkillInput {
                source_type: "git".into(),
                source: input.source_url.clone(),
                branch: if input.source_type == "skills_sh" {
                    None
                } else {
                    Some(input.branch.clone())
                },
                subpath: None,
                revision: None,
            };
            clone_git_source(&clone_input, &staging)?;
            let repository = staging.join("repository");
            let effective_branch = if input.source_type == "skills_sh" {
                git_current_branch(&repository).unwrap_or_else(|| "main".into())
            } else {
                input.branch.clone()
            };
            let revision = git_head_revision(&repository)?;
            let requested = input.subpath.trim().replace('\\', "/");
            let skill_root = resolve_market_skill_root(&repository, &requested, &input.name)?;
            let actual_subpath = skill_root
                .strip_prefix(&repository)?
                .to_string_lossy()
                .replace('\\', "/");
            let import_input = ImportSkillInput {
                source_type: input.source_type,
                source: input.source_url,
                branch: Some(effective_branch),
                subpath: Some(actual_subpath),
                revision: Some(revision),
            };
            let asset = self.import_one(&import_input, &skill_root)?;
            self.register_update_baseline(&asset, false)?;
            Ok(asset)
        })();
        let _ = fs::remove_dir_all(staging);
        result
    }

    fn market_repository(&self, id: &str) -> Result<Option<SkillMarketRepository>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,owner,name,branch,enabled,builtin,updated_at
                 FROM skill_market_repositories WHERE id=?1",
                [id],
                |row| {
                    Ok(SkillMarketRepository {
                        id: row.get(0)?,
                        owner: row.get(1)?,
                        name: row.get(2)?,
                        branch: row.get(3)?,
                        enabled: row.get::<_, i64>(4)? != 0,
                        builtin: row.get::<_, i64>(5)? != 0,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn import(&self, input: ImportSkillInput) -> Result<Vec<SkillAsset>> {
        let _import_guard = self.import_lock.lock().unwrap();
        let staging = self.root.join("tmp").join(Uuid::new_v4().to_string());
        fs::create_dir_all(&staging)?;
        let result = (|| {
            let mut effective_input = input.clone();
            let source_root = match input.source_type.as_str() {
                "local" | "adopted" => PathBuf::from(&input.source),
                "archive" => {
                    extract_archive(Path::new(&input.source), &staging)?;
                    staging.clone()
                }
                "git" => {
                    clone_git_source(&input, &staging)?;
                    let repository = staging.join("repository");
                    effective_input.revision = Some(git_head_revision(&repository)?);
                    repository
                }
                other => bail!("不支持的 Skill 来源类型：{other}"),
            };
            let source_boundary = source_root.clone();
            let selected = if let Some(subpath) = input.subpath.as_deref() {
                safe_join(&source_root, subpath)?
            } else {
                source_root
            };
            if matches!(input.source_type.as_str(), "archive" | "git") {
                ensure_within(&selected, &source_boundary)?;
            }
            let roots = if matches!(input.source_type.as_str(), "archive" | "git") {
                find_skill_roots_recursive(&selected)?
            } else {
                find_skill_roots(&selected)?
            };
            if roots.is_empty() {
                bail!("未找到包含 SKILL.md 的目录");
            }
            let mut imported = Vec::new();
            for skill_root in roots {
                let mut skill_input = effective_input.clone();
                if input.source_type == "git" {
                    skill_input.subpath = Some(
                        skill_root
                            .strip_prefix(staging.join("repository"))?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
                let asset = self.import_one(&skill_input, &skill_root)?;
                self.register_update_baseline(&asset, input.revision.is_some())?;
                imported.push(asset);
            }
            Ok(imported)
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    pub fn set_trust(&self, skill_id: &str, trust_state: &str) -> Result<SkillAsset> {
        if !matches!(trust_state, "unknown" | "trusted" | "blocked") {
            bail!("无效的信任状态");
        }
        let now = now_ts();
        self.conn.lock().unwrap().execute(
            "UPDATE skill_assets SET trust_state=?1, updated_at=?2 WHERE id=?3",
            params![trust_state, now, skill_id],
        )?;
        self.asset(skill_id)?.ok_or_else(|| anyhow!("Skill 不存在"))
    }

    pub fn delete_asset(&self, skill_id: &str) -> Result<()> {
        let _deployment_guard = self.deployment_lock.lock().unwrap();
        if self.conn.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM skill_deployments WHERE skill_id=?1",
            [skill_id],
            |row| row.get::<_, i64>(0),
        )? > 0
        {
            bail!("该 Skill 仍有 Runtime 部署，请先撤下");
        }
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        let path = PathBuf::from(asset.central_path);
        ensure_within(&path, &self.root.join("assets"))?;
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM skill_assets WHERE id=?1", [skill_id])?;
        Ok(())
    }

    /// 按来源路径反查资产 id（updated_at 新的在前）。内置功能（浏览器控制等）
    /// 用它定位自己物化的 skill 及历次版本残留。
    pub fn asset_ids_by_source_ref(&self, source_ref: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut statement =
            conn.prepare("SELECT id FROM skill_assets WHERE source_ref=?1 ORDER BY updated_at DESC")?;
        let ids = statement
            .query_map([source_ref], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    /// 某资产的全部部署记录 id（撤下内置功能的 skill 用）。
    pub fn deployment_ids_for_skill(&self, skill_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut statement =
            conn.prepare("SELECT id FROM skill_deployments WHERE skill_id=?1")?;
        let ids = statement
            .query_map([skill_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    pub fn preview(&self, skill_id: &str) -> Result<SkillPreview> {
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        let root = PathBuf::from(asset.central_path);
        let skill_md_path = locate_skill_md(&root)?;
        let skill_md = fs::read_to_string(skill_md_path)?;
        let mut files = Vec::new();
        collect_file_entries(&root, &root, &mut files)?;
        Ok(SkillPreview {
            skill_id: skill_id.into(),
            skill_md,
            files,
        })
    }

    pub fn read_file(&self, skill_id: &str, relative_path: &str) -> Result<String> {
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        let root = PathBuf::from(asset.central_path);
        let path = safe_join(&root, relative_path)?;
        ensure_within(&path, &root)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_PREVIEW_BYTES {
            bail!("文件过大，无法预览");
        }
        fs::read_to_string(path).context("该文件不是可预览的 UTF-8 文本")
    }

    pub fn discover(&self, project_paths: &[String]) -> Result<Vec<DiscoveredSkill>> {
        let adapters = runtime_adapters()?;
        let managed: HashSet<String> = self
            .list_assets()?
            .into_iter()
            .map(|item| item.content_hash)
            .collect();
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for adapter in adapters {
            let mut roots = vec![PathBuf::from(&adapter.global_root)];
            roots.extend(adapter.discovery_roots.iter().map(PathBuf::from));
            roots.extend(
                project_paths
                    .iter()
                    .map(|project| Path::new(project).join(&adapter.project_root)),
            );
            for root in roots {
                if !root.exists() {
                    continue;
                }
                for skill_root in scan_skill_root(&root, adapter.recursive_scan)? {
                    let canonical = skill_root
                        .canonicalize()
                        .unwrap_or(skill_root.clone())
                        .display()
                        .to_string();
                    if !seen.insert((adapter.runtime.clone(), canonical.clone())) {
                        continue;
                    }
                    let (name, description) = read_skill_metadata(&skill_root)?;
                    let hash = hash_directory(&skill_root)?;
                    result.push(DiscoveredSkill {
                        runtime: adapter.runtime.clone(),
                        path: canonical,
                        name,
                        description,
                        already_managed: managed.contains(&hash),
                        content_hash: hash,
                    });
                }
            }
        }
        result.sort_by(|a, b| a.runtime.cmp(&b.runtime).then(a.name.cmp(&b.name)));
        Ok(result)
    }

    pub fn deploy_batch(&self, inputs: Vec<DeploySkillInput>) -> Result<Vec<SkillDeployment>> {
        Ok(self.deploy_batch_with_ownership(inputs)?.into_iter().map(|(deployment, _)| deployment).collect())
    }

    /// Whether each returned record was created in this locked operation.
    /// Plugin removal must not claim an existing manual deployment.
    pub(crate) fn deploy_batch_with_ownership(&self, inputs: Vec<DeploySkillInput>) -> Result<Vec<(SkillDeployment, bool)>> {
        let _deployment_guard = self.deployment_lock.lock().unwrap();
        if inputs.iter().any(|input| matches!(input.runtime.as_str(), "oh_my_pi" | "minimax_code")) {
            bail!("该 CLI 已移除，历史 Skill 部署不可重新部署");
        }
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let prepared = inputs
            .iter()
            .map(|input| self.prepare_deployment(input))
            .collect::<Result<Vec<_>>>()?;
        for item in &prepared {
            preflight_target(&item.asset, &item.target, item.existing.as_ref())?;
        }
        let mut deployed = Vec::new();
        for item in prepared {
            let created = item.existing.is_none();
            deployed.push((self.materialize(item)?, created));
        }
        Ok(deployed)
    }

    pub fn undeploy(&self, deployment_id: &str) -> Result<()> {
        let _deployment_guard = self.deployment_lock.lock().unwrap();
        self.undeploy_unlocked(deployment_id)
    }

    fn undeploy_unlocked(&self, deployment_id: &str) -> Result<()> {
        let deployment = self
            .deployment(deployment_id)?
            .ok_or_else(|| anyhow!("部署记录不存在"))?;
        if matches!(deployment.runtime.as_str(), "oh_my_pi" | "minimax_code") {
            bail!("该 CLI 已移除，历史 Skill 部署记录与外部文件需保留");
        }
        let target = PathBuf::from(&deployment.target_path);
        let shared = self.conn.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM skill_deployments WHERE target_path=?1 AND id<>?2",
            params![deployment.target_path, deployment_id],
            |row| row.get::<_, i64>(0),
        )?;
        if shared == 0 && path_exists_no_follow(&target) {
            match deployment.mode.as_str() {
                "symlink" | "junction" => remove_owned_link(
                    &target,
                    &PathBuf::from(self.deployment_version_asset(&deployment)?.central_path),
                )?,
                "copy" => {
                    remove_owned_copy(&target, &deployment.skill_id, &deployment.source_hash)?
                }
                _ => bail!("未知部署模式，拒绝删除"),
            }
        }
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM skill_deployments WHERE id=?1", [deployment_id])?;
        Ok(())
    }

    pub fn undeploy_batch(&self, deployment_ids: Vec<String>) -> Result<()> {
        let _deployment_guard = self.deployment_lock.lock().unwrap();
        let mut unique = HashSet::new();
        let deployments = deployment_ids
            .into_iter()
            .filter(|id| unique.insert(id.clone()))
            .map(|id| {
                self.deployment(&id)?
                    .ok_or_else(|| anyhow!("部署记录不存在：{id}"))
            })
            .collect::<Result<Vec<_>>>()?;
        if deployments
            .iter()
            .any(|deployment| matches!(deployment.runtime.as_str(), "oh_my_pi" | "minimax_code"))
        {
            bail!("该 CLI 已移除，历史 Skill 部署记录与外部文件需保留");
        }
        for deployment in &deployments {
            let target = PathBuf::from(&deployment.target_path);
            let shared = self.conn.lock().unwrap().query_row(
                "SELECT COUNT(*) FROM skill_deployments WHERE target_path=?1 AND id<>?2",
                params![deployment.target_path, deployment.id],
                |row| row.get::<_, i64>(0),
            )?;
            if shared > 0 || !path_exists_no_follow(&target) {
                continue;
            }
            let asset = self.deployment_version_asset(deployment)?;
            match deployment.mode.as_str() {
                "symlink" | "junction"
                    if link_points_to(&target, Path::new(&asset.central_path)) => {}
                "copy"
                    if is_owned_copy(&target, &deployment.skill_id, &deployment.source_hash)? => {}
                _ => bail!(
                    "{} 的所有权或内容已变化，批量撤下未执行",
                    deployment.target_path
                ),
            }
        }
        for deployment in deployments {
            self.undeploy_unlocked(&deployment.id)?;
        }
        Ok(())
    }

    pub fn save_skill_set(&self, input: SaveSkillSetInput) -> Result<SkillSet> {
        let name = input.name.trim();
        if name.is_empty() {
            bail!("配置集名称不能为空");
        }
        let mut unique_items = HashSet::new();
        for item in &input.items {
            validate_deploy_identity(item)?;
            self.asset(&item.skill_id)?
                .ok_or_else(|| anyhow!("配置集包含不存在的 Skill"))?;
            if !unique_items.insert(deploy_identity(
                &item.skill_id,
                &item.runtime,
                &item.scope,
                item.project_path.as_deref(),
                item.task_id.as_deref(),
                item.agent_id.as_deref(),
            )) {
                bail!("配置集包含重复的 Skill / Runtime / 作用域目标");
            }
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_ts();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO skill_sets(id,name,description,project_path,agent_role,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?6)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,
               project_path=excluded.project_path,agent_role=excluded.agent_role,updated_at=excluded.updated_at",
            params![id, name, input.description, input.project_path, input.agent_role, now],
        )?;
        tx.execute("DELETE FROM skill_set_items WHERE set_id=?1", [&id])?;
        for item in input.items {
            tx.execute(
                "INSERT INTO skill_set_items(set_id,skill_id,runtime,scope,project_path,task_id,agent_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![id, item.skill_id, item.runtime, item.scope, item.project_path, item.task_id, item.agent_id],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.skill_set(&id)?
            .ok_or_else(|| anyhow!("保存配置集失败"))
    }

    pub fn delete_skill_set(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM skill_sets WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn apply_skill_set(&self, id: &str, trust_confirmed: bool) -> Result<Vec<SkillDeployment>> {
        let set = self.skill_set(id)?.ok_or_else(|| anyhow!("配置集不存在"))?;
        let inputs = set
            .items
            .into_iter()
            .map(|item| DeploySkillInput {
                skill_id: item.skill_id,
                runtime: item.runtime,
                scope: item.scope,
                project_path: item.project_path.or_else(|| set.project_path.clone()),
                task_id: item.task_id,
                agent_id: item.agent_id,
                trust_confirmed,
            })
            .collect();
        self.deploy_batch(inputs)
    }
}

struct PreparedDeployment {
    asset: SkillAsset,
    input: DeploySkillInput,
    target: PathBuf,
    existing: Option<SkillDeployment>,
}

impl SkillCenter {
    fn refresh_asset_hashes(&self) -> Result<()> {
        for asset in self.list_assets()? {
            let path = PathBuf::from(&asset.central_path);
            if !path.is_dir() {
                continue;
            }
            let content_hash = hash_directory(&path)?;
            if content_hash == asset.content_hash {
                continue;
            }
            let (file_count, script_count, risks) = inspect_skill(&path)?;
            self.conn.lock().unwrap().execute(
                "UPDATE skill_assets SET content_hash=?1,file_count=?2,script_count=?3,
                 risk_summary=?4,updated_at=?5 WHERE id=?6",
                params![
                    content_hash,
                    file_count,
                    script_count,
                    serde_json::to_string(&risks)?,
                    now_ts(),
                    asset.id
                ],
            )?;
        }
        Ok(())
    }

    fn import_one(&self, input: &ImportSkillInput, source: &Path) -> Result<SkillAsset> {
        locate_skill_md(source)?;
        let hash = hash_directory(source)?;
        // 注意锁作用域：if let 条件里的临时 MutexGuard 会活到块尾，
        // 块内再调 asset() 重入 conn 锁会死锁，必须先显式释放。
        let existing_id = {
            let guard = self.conn.lock().unwrap();
            guard
                .query_row(
                    "SELECT id FROM skill_assets WHERE content_hash=?1",
                    [&hash],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        };
        if let Some(existing) = existing_id {
            return self
                .asset(&existing)?
                .ok_or_else(|| anyhow!("读取已有 Skill 失败"));
        }
        let (name, description) = read_skill_metadata(source)?;
        let id = Uuid::new_v4().to_string();
        let slug = safe_slug(&name);
        let target = self
            .root
            .join("assets")
            .join(format!("{slug}-{}", &id[..8]));
        copy_directory(source, &target, false)?;
        let (file_count, script_count, risk_summary) = inspect_skill(&target)?;
        let now = now_ts();
        let insert_result = self.conn.lock().unwrap().execute(
            "INSERT INTO skill_assets(id,name,description,source_type,source_ref,source_branch,source_subpath,
             source_revision,central_path,content_hash,trust_state,file_count,script_count,risk_summary,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'unknown',?11,?12,?13,?14,?14)",
            params![id, name, description, input.source_type, input.source, input.branch, input.subpath,
                input.revision, target.display().to_string(), hash, file_count, script_count,
                serde_json::to_string(&risk_summary)?, now],
        );
        if let Err(error) = insert_result {
            let _ = fs::remove_dir_all(&target);
            let existing_id = {
                let guard = self.conn.lock().unwrap();
                guard
                    .query_row(
                        "SELECT id FROM skill_assets WHERE content_hash=?1",
                        [&hash],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
            };
            if let Some(existing) = existing_id {
                return self
                    .asset(&existing)?
                    .ok_or_else(|| anyhow!("读取并发导入的 Skill 失败"));
            }
            return Err(error.into());
        }
        self.asset(&id)?.ok_or_else(|| anyhow!("导入 Skill 失败"))
    }

    fn prepare_deployment(&self, input: &DeploySkillInput) -> Result<PreparedDeployment> {
        if matches!(input.runtime.as_str(), "oh_my_pi" | "minimax_code") {
            bail!("该 CLI 已移除，历史 Skill 部署不可重新部署");
        }
        validate_deploy_identity(input)?;
        // install-cli「版本兼容检查」的对应物：runtime 命令探测不到时不阻断部署
        // （文件级部署支持在装 runtime 前预留 skill），但显式记录，
        // 避免「部署了却不生效」被静默吞掉。
        if let Ok(adapters) = runtime_adapters() {
            if let Some(adapter) = adapters.iter().find(|item| item.runtime == input.runtime) {
                if !adapter.installed {
                    eprintln!(
                        "[crewvela] 警告：runtime「{}」未在 PATH 探测到，\
                         本次部署的 skill 在该 runtime 安装前不会生效",
                        adapter.label
                    );
                }
            }
        }
        let mut asset = self
            .asset(&input.skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        if asset.trust_state == "blocked" {
            bail!("该 Skill 已被阻止部署");
        }
        if asset.trust_state != "trusted" {
            if !input.trust_confirmed {
                bail!("Skill 尚未信任，请审阅内容后确认部署");
            }
            asset = self.set_trust(&input.skill_id, "trusted")?;
        }
        let target = deployment_target(input, &asset)?;
        ensure_non_overlapping(Path::new(&asset.central_path), &target)?;
        let existing = self.find_deployment(input)?;
        Ok(PreparedDeployment {
            asset,
            input: input.clone(),
            target,
            existing,
        })
    }

    fn materialize(&self, item: PreparedDeployment) -> Result<SkillDeployment> {
        let source = PathBuf::from(&item.asset.central_path);
        if let Some(parent) = item.target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mode =
            create_managed_target(&source, &item.target, item.existing.as_ref(), &item.asset)?;
        // 以 runtime 的实际发现行为验证部署结果（install-cli「终端说激活完成不算数」
        // 的同义约束）：runtime 靠目标根部的 SKILL.md 发现 skill，文件没到位就是没装上。
        // 校验失败时清理刚创建的目标并报错，不落 synced 部署记录。
        if locate_skill_md(&item.target).is_err() {
            let _ = fs::remove_dir_all(&item.target);
            bail!(
                "部署校验失败：目标根部缺少 SKILL.md（{}），runtime 无法发现该 skill",
                item.target.display()
            );
        }
        let target_hash = hash_directory(&item.target).ok();
        let id = item
            .existing
            .as_ref()
            .map(|d| d.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_ts();
        self.conn.lock().unwrap().execute(
            "INSERT INTO skill_deployments(id,identity_key,skill_id,runtime,scope,project_path,task_id,agent_id,target_path,
             mode,status,source_hash,target_hash,last_error,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'synced',?11,?12,NULL,?13)
             ON CONFLICT(id) DO UPDATE SET
               target_path=excluded.target_path,mode=excluded.mode,status='synced',source_hash=excluded.source_hash,
               target_hash=excluded.target_hash,last_error=NULL,updated_at=excluded.updated_at",
            params![id,deployment_identity(&item.input),item.asset.id,item.input.runtime,item.input.scope,item.input.project_path,item.input.task_id,
                item.input.agent_id,item.target.display().to_string(),mode,item.asset.content_hash,target_hash,now],
        )?;
        self.find_deployment(&item.input)?
            .ok_or_else(|| anyhow!("写入部署记录失败"))
    }

    fn refresh_deployment_statuses(&self) -> Result<()> {
        for deployment in self.list_deployments()? {
            let asset = match self.asset(&deployment.skill_id)? {
                Some(asset) => asset,
                None => continue,
            };
            let target = PathBuf::from(&deployment.target_path);
            if matches!(deployment.mode.as_str(), "symlink" | "junction")
                && link_points_to(&target, Path::new(&asset.central_path))
            {
                self.conn.lock().unwrap().execute(
                    "UPDATE skill_deployments SET status='synced',source_hash=?1,target_hash=?1,last_error=NULL,updated_at=?2 WHERE id=?3",
                    params![asset.content_hash,now_ts(),deployment.id],
                )?;
                continue;
            }
            let (target_hash, target_error) = if path_exists_no_follow(&target) {
                let checked_hash = (|| -> Result<String> {
                    if deployment.mode == "copy" {
                        is_owned_copy(&target, &asset.id, &deployment.source_hash)?;
                    }
                    hash_directory(&target)
                })();
                match checked_hash {
                    Ok(hash) => (Some(hash), None),
                    Err(error) => (None, Some(format!("无法校验 Runtime 部署：{error:#}"))),
                }
            } else {
                (None, Some("Runtime 部署目标已不存在".into()))
            };
            let source_changed = asset.content_hash != deployment.source_hash;
            let target_changed = target_hash
                .as_deref()
                .is_some_and(|hash| hash != deployment.source_hash);
            let status = if target_error.is_some() {
                "failed"
            } else if source_changed && target_changed {
                "conflict"
            } else if source_changed {
                "center_newer"
            } else if target_changed {
                "runtime_newer"
            } else {
                "synced"
            };
            self.conn.lock().unwrap().execute(
                "UPDATE skill_deployments SET status=?1,target_hash=?2,updated_at=?3,last_error=?5 WHERE id=?4",
                params![status, target_hash, now_ts(), deployment.id, target_error],
            )?;
        }
        Ok(())
    }

    fn asset(&self, id: &str) -> Result<Option<SkillAsset>> {
        self.conn.lock().unwrap().query_row(
            "SELECT id,name,description,source_type,source_ref,source_branch,source_subpath,source_revision,
             central_path,content_hash,trust_state,file_count,script_count,risk_summary,created_at,updated_at
             FROM skill_assets WHERE id=?1", [id], map_asset,
        ).optional().map_err(Into::into)
    }

    fn list_assets(&self) -> Result<Vec<SkillAsset>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT id,name,description,source_type,source_ref,source_branch,source_subpath,source_revision,
             central_path,content_hash,trust_state,file_count,script_count,risk_summary,created_at,updated_at
             FROM skill_assets ORDER BY name COLLATE NOCASE",
        )?;
        let rows = statement
            .query_map([], map_asset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn deployment(&self, id: &str) -> Result<Option<SkillDeployment>> {
        self.conn.lock().unwrap().query_row(
            "SELECT id,skill_id,runtime,scope,project_path,task_id,agent_id,target_path,mode,status,
             source_hash,target_hash,last_error,updated_at FROM skill_deployments WHERE id=?1",
            [id], map_deployment,
        ).optional().map_err(Into::into)
    }

    fn find_deployment(&self, input: &DeploySkillInput) -> Result<Option<SkillDeployment>> {
        self.conn.lock().unwrap().query_row(
            "SELECT id,skill_id,runtime,scope,project_path,task_id,agent_id,target_path,mode,status,
             source_hash,target_hash,last_error,updated_at FROM skill_deployments
             WHERE identity_key=?1",
            [deployment_identity(input)], map_deployment,
        ).optional().map_err(Into::into)
    }

    fn list_deployments(&self) -> Result<Vec<SkillDeployment>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT id,skill_id,runtime,scope,project_path,task_id,agent_id,target_path,mode,status,
             source_hash,target_hash,last_error,updated_at FROM skill_deployments ORDER BY updated_at DESC",
        )?;
        let rows = statement
            .query_map([], map_deployment)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn skill_set(&self, id: &str) -> Result<Option<SkillSet>> {
        let header = self.conn.lock().unwrap().query_row(
            "SELECT id,name,description,project_path,agent_role,created_at,updated_at FROM skill_sets WHERE id=?1",
            [id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,
                row.get::<_,Option<String>>(3)?,row.get::<_,Option<String>>(4)?,row.get::<_,i64>(5)?,row.get::<_,i64>(6)?)),
        ).optional()?;
        let Some((id, name, description, project_path, agent_role, created_at, updated_at)) =
            header
        else {
            return Ok(None);
        };
        let items = self.list_skill_set_items(&id)?;
        Ok(Some(SkillSet {
            id,
            name,
            description,
            project_path,
            agent_role,
            items,
            created_at,
            updated_at,
        }))
    }

    fn list_skill_sets(&self) -> Result<Vec<SkillSet>> {
        let ids = {
            let conn = self.conn.lock().unwrap();
            let mut statement =
                conn.prepare("SELECT id FROM skill_sets ORDER BY updated_at DESC")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        ids.into_iter()
            .map(|id| {
                self.skill_set(&id)?
                    .ok_or_else(|| anyhow!("配置集读取失败"))
            })
            .collect()
    }

    fn list_skill_set_items(&self, id: &str) -> Result<Vec<SkillSetItem>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT skill_id,runtime,scope,project_path,task_id,agent_id FROM skill_set_items WHERE set_id=?1 ORDER BY runtime,skill_id",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok(SkillSetItem {
                    skill_id: row.get(0)?,
                    runtime: row.get(1)?,
                    scope: row.get(2)?,
                    project_path: row.get(3)?,
                    task_id: row.get(4)?,
                    agent_id: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

fn map_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillAsset> {
    let risks: String = row.get(13)?;
    Ok(SkillAsset {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        source_type: row.get(3)?,
        source_ref: row.get(4)?,
        source_branch: row.get(5)?,
        source_subpath: row.get(6)?,
        source_revision: row.get(7)?,
        central_path: row.get(8)?,
        content_hash: row.get(9)?,
        trust_state: row.get(10)?,
        file_count: row.get(11)?,
        script_count: row.get(12)?,
        risk_summary: serde_json::from_str(&risks).unwrap_or_default(),
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn map_deployment(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillDeployment> {
    Ok(SkillDeployment {
        id: row.get(0)?,
        skill_id: row.get(1)?,
        runtime: row.get(2)?,
        scope: row.get(3)?,
        project_path: row.get(4)?,
        task_id: row.get(5)?,
        agent_id: row.get(6)?,
        target_path: row.get(7)?,
        mode: row.get(8)?,
        status: row.get(9)?,
        source_hash: row.get(10)?,
        target_hash: row.get(11)?,
        last_error: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

pub(crate) fn runtime_adapters() -> Result<Vec<RuntimeSkillAdapter>> {
    let home = dirs::home_dir().context("无法确定用户主目录")?;
    let kimi_home = std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kimi-code"));
    let path = |root: &Path, relative: &str| root.join(relative).display().to_string();
    Ok(vec![
        adapter(
            "claude_code",
            "Claude Code",
            path(&home, ".claude/skills"),
            ".claude/skills",
            vec![],
            false,
        ),
        adapter(
            "codex",
            "Codex",
            path(&home, ".codex/skills"),
            ".codex/skills",
            vec![path(&home, ".agents/skills")],
            false,
        ),
        adapter(
            "kimi_code",
            "Kimi Code",
            path(&kimi_home, "skills"),
            ".kimi-code/skills",
            vec![
                path(&home, ".agents/skills"),
                path(&home, ".config/agents/skills"),
            ],
            false,
        ),
        adapter(
            "pi",
            "Pi",
            path(&home, ".pi/agent/skills"),
            ".pi/skills",
            vec![path(&home, ".agents/skills")],
            false,
        ),
        adapter(
            "opencode",
            "OpenCode",
            path(&home, ".config/opencode/skills"),
            ".opencode/skills",
            vec![],
            false,
        ),
    ])
}

fn adapter(
    runtime: &str,
    label: &str,
    global_root: String,
    project_root: &str,
    discovery_roots: Vec<String>,
    recursive_scan: bool,
) -> RuntimeSkillAdapter {
    let command = match runtime {
        "claude_code" => "claude",
        "codex" => "codex",
        "kimi_code" => "kimi",
        "pi" => "pi",
        "opencode" => "opencode",
        _ => "",
    };
    RuntimeSkillAdapter {
        runtime: runtime.into(),
        label: label.into(),
        global_root,
        project_root: project_root.into(),
        discovery_roots,
        recursive_scan,
        installed: !command.is_empty() && which::which(command).is_ok(),
    }
}

fn deployment_target(input: &DeploySkillInput, asset: &SkillAsset) -> Result<PathBuf> {
    let adapter = runtime_adapters()?
        .into_iter()
        .find(|item| item.runtime == input.runtime)
        .ok_or_else(|| anyhow!("不支持的 Runtime：{}", input.runtime))?;
    let root = match input.scope.as_str() {
        "global" => PathBuf::from(adapter.global_root),
        "project" | "task" => {
            let project = input
                .project_path
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("项目或任务作用域必须指定项目目录"))?;
            Path::new(project).join(adapter.project_root)
        }
        _ => bail!("不支持的作用域：{}", input.scope),
    };
    Ok(root.join(asset_directory_name(asset)))
}

fn asset_directory_name(asset: &SkillAsset) -> String {
    let slug = safe_slug(&asset.name);
    if slug == "skill" {
        asset.id[..8].to_string()
    } else {
        slug
    }
}

fn validate_deploy_identity(item: &impl DeployIdentity) -> Result<()> {
    if !matches!(item.scope(), "global" | "project" | "task") {
        bail!("无效的部署作用域");
    }
    if !runtime_adapters()?
        .iter()
        .any(|adapter| adapter.runtime == item.runtime())
    {
        bail!("无效的 Runtime");
    }
    if matches!(item.scope(), "project" | "task") {
        let project_path = item
            .project_path()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("项目或任务作用域缺少项目目录"))?;
        if !Path::new(project_path).is_absolute() {
            bail!("项目或任务作用域必须使用绝对工作目录");
        }
    }
    if item.scope() == "task"
        && item.task_id().is_none_or(|value| value.trim().is_empty())
        && item.agent_id().is_none_or(|value| value.trim().is_empty())
    {
        bail!("任务作用域至少需要任务或 Agent 标识");
    }
    Ok(())
}

trait DeployIdentity {
    fn runtime(&self) -> &str;
    fn scope(&self) -> &str;
    fn project_path(&self) -> Option<&str>;
    fn task_id(&self) -> Option<&str>;
    fn agent_id(&self) -> Option<&str>;
}
impl DeployIdentity for DeploySkillInput {
    fn runtime(&self) -> &str {
        &self.runtime
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    fn project_path(&self) -> Option<&str> {
        self.project_path.as_deref()
    }
    fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }
}
impl DeployIdentity for SkillSetItem {
    fn runtime(&self) -> &str {
        &self.runtime
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    fn project_path(&self) -> Option<&str> {
        self.project_path.as_deref()
    }
    fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }
}

fn deploy_identity(
    skill_id: &str,
    runtime: &str,
    scope: &str,
    project_path: Option<&str>,
    task_id: Option<&str>,
    agent_id: Option<&str>,
) -> String {
    format!(
        "{skill_id}|{runtime}|{scope}|{}|{}|{}",
        project_path.unwrap_or_default(),
        task_id.unwrap_or_default(),
        agent_id.unwrap_or_default()
    )
}

fn deployment_identity(input: &DeploySkillInput) -> String {
    deploy_identity(
        &input.skill_id,
        &input.runtime,
        &input.scope,
        input.project_path.as_deref(),
        input.task_id.as_deref(),
        input.agent_id.as_deref(),
    )
}

fn path_exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn preflight_target(
    asset: &SkillAsset,
    target: &Path,
    existing: Option<&SkillDeployment>,
) -> Result<()> {
    if !path_exists_no_follow(target) {
        return Ok(());
    }
    let source = Path::new(&asset.central_path);
    if link_points_to(target, source) {
        return Ok(());
    }
    let Some(record) = existing else {
        bail!("目标已存在且不属于 CrewVela：{}", target.display());
    };
    match record.mode.as_str() {
        "copy" if is_owned_copy(target, &asset.id, &record.source_hash)? => Ok(()),
        "symlink" | "junction" => {
            bail!("原受管链接已被外部内容替换，拒绝覆盖：{}", target.display())
        }
        _ => bail!("无法证明目标所有权，拒绝覆盖：{}", target.display()),
    }
}

fn create_managed_target(
    source: &Path,
    target: &Path,
    existing: Option<&SkillDeployment>,
    asset: &SkillAsset,
) -> Result<String> {
    if path_exists_no_follow(target) {
        if link_points_to(target, source) {
            return Ok("symlink".into());
        }
        let record = existing.ok_or_else(|| anyhow!("目标不属于 CrewVela"))?;
        if record.mode == "copy" && is_owned_copy(target, &asset.id, &record.source_hash)? {
            fs::remove_dir_all(target)?;
        } else {
            bail!("目标已被修改，拒绝覆盖：{}", target.display());
        }
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(source, target).is_ok() {
            return Ok("symlink".into());
        }
        if junction::create(source, target).is_ok() {
            return Ok("junction".into());
        }
    }
    #[cfg(unix)]
    if std::os::unix::fs::symlink(source, target).is_ok() {
        return Ok("symlink".into());
    }
    copy_directory(source, target, false)?;
    let marker = serde_json::json!({"skillId":asset.id,"sourceHash":asset.content_hash,"managedBy":"crewvela"});
    fs::write(
        target.join(COPY_MARKER),
        serde_json::to_vec_pretty(&marker)?,
    )?;
    Ok("copy".into())
}

fn remove_owned_link(target: &Path, source: &Path) -> Result<()> {
    if !link_points_to(target, source) {
        bail!("链接目标已变化，拒绝撤下");
    }
    #[cfg(windows)]
    {
        fs::remove_dir(target).or_else(|_| fs::remove_file(target))?;
    }
    #[cfg(not(windows))]
    {
        fs::remove_file(target)?;
    }
    Ok(())
}

fn remove_owned_copy(target: &Path, skill_id: &str, source_hash: &str) -> Result<()> {
    if !is_owned_copy(target, skill_id, source_hash)? {
        bail!("Runtime 副本已被修改，拒绝删除");
    }
    fs::remove_dir_all(target)?;
    Ok(())
}

fn is_owned_copy(target: &Path, skill_id: &str, source_hash: &str) -> Result<bool> {
    let current_marker = target.join(COPY_MARKER);
    let marker_path = if path_exists_no_follow(&current_marker) {
        current_marker
    } else {
        let mut historical = None;
        for entry in fs::read_dir(target)? {
            let entry = entry?;
            if is_legacy_marker_name(&entry.file_name()) {
                if historical.is_some() { return Ok(false); }
                historical = Some(entry.path());
            }
        }
        let Some(path) = historical else { return Ok(false); };
        path
    };
    let Some((mut marker, raw)) = read_copy_marker(&marker_path)? else { return Ok(false); };
    if marker.get("skillId").and_then(|v| v.as_str()) != Some(skill_id)
        || marker.get("sourceHash").and_then(|v| v.as_str()) != Some(source_hash) {
        return Ok(false);
    }
    let current = hash_directory(target)?;
    if current != source_hash { return Ok(false); }
    if marker_path.file_name() != Some(std::ffi::OsStr::new(COPY_MARKER)) {
        marker["managedBy"] = serde_json::json!("crewvela");
        publish_migrated_copy_marker(target, &marker_path, &raw, &marker)?;
    } else {
        // A prior publication may have succeeded while deleting the old marker
        // failed. Retry only cleanup of a still-valid, matching owned marker.
        for entry in fs::read_dir(target)? {
            let entry = entry?;
            if !is_legacy_marker_name(&entry.file_name()) { continue; }
            if let Some((old, _)) = read_copy_marker(&entry.path())? {
                if old.get("skillId").and_then(|value| value.as_str()) == Some(skill_id)
                    && old.get("sourceHash").and_then(|value| value.as_str()) == Some(source_hash) {
                    fs::remove_file(entry.path()).context("清理 Skill 旧所有权标记失败")?;
                }
            }
        }
    }
    Ok(true)
}

fn is_legacy_marker_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name|
        format!("{:x}", Sha256::digest(name.as_bytes())) == LEGACY_COPY_MARKER_SHA256)
}

// Marker metadata is excluded from the content hash only when its name,
// manager and required fields prove it is an application marker. An unrelated
// or tampered same-name user file remains ordinary hashed content.
fn read_copy_marker(path: &Path) -> Result<Option<(serde_json::Value, Vec<u8>)>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("读取 Skill 所有权标记失败"),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() { return Ok(None); }
    let Some(name) = path.file_name() else { return Ok(None); };
    let legacy = is_legacy_marker_name(name);
    if name != COPY_MARKER && !legacy { return Ok(None); }
    let raw = fs::read(path).context("读取 Skill 所有权标记失败")?;
    let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&raw) else { return Ok(None); };
    let manager = marker.get("managedBy").and_then(|value| value.as_str()).unwrap_or_default();
    let expected_manager = if legacy {
        format!("{:x}", Sha256::digest(manager.as_bytes())) == LEGACY_MANAGED_BY_SHA256
    } else { manager == "crewvela" };
    let valid_id = marker.get("skillId").and_then(|value| value.as_str()).is_some_and(|id| !id.is_empty());
    let valid_hash = marker.get("sourceHash").and_then(|value| value.as_str())
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    Ok((expected_manager && valid_id && valid_hash).then_some((marker, raw)))
}

fn publish_migrated_copy_marker(
    target: &Path,
    previous_path: &Path,
    previous_raw: &[u8],
    marker: &serde_json::Value,
) -> Result<()> {
    let temporary = target.join(format!(".crewvela-marker-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(marker)?)?;
        file.sync_all()?;
        drop(file);
        // Publish without replacing a marker created concurrently by another
        // process. Unsupported filesystems fail safely with the old marker kept.
        fs::hard_link(&temporary, target.join(COPY_MARKER)).context("发布 Skill 所有权新标记失败")?;
        fs::remove_file(&temporary)?;
        if fs::read(previous_path)? != previous_raw {
            bail!("Skill 所有权标记在迁移期间被修改，原标记已保留");
        }
        fs::remove_file(previous_path).context("清理 Skill 旧所有权标记失败")?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result.context("迁移 Skill 所有权标记失败")
}

fn link_points_to(target: &Path, source: &Path) -> bool {
    if let Ok(link) = fs::read_link(target) {
        let resolved = if link.is_absolute() {
            link
        } else {
            target.parent().unwrap_or(Path::new("")).join(link)
        };
        if resolved == source {
            return true;
        }
        if let (Ok(left), Ok(right)) = (resolved.canonicalize(), source.canonicalize()) {
            return left == right;
        }
    }
    // Windows Junction may be followed by canonicalize even when read_link behavior differs.
    if let (Ok(left), Ok(right)) = (target.canonicalize(), source.canonicalize()) {
        return left == right;
    }
    false
}

fn ensure_non_overlapping(source: &Path, target: &Path) -> Result<()> {
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let target = normalize_absolute(target)?;
    if source.starts_with(&target) || target.starts_with(&source) {
        bail!("中央 Skill 与部署目标路径重叠");
    }
    Ok(())
}

fn ensure_within(path: &Path, root: &Path) -> Result<()> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path.starts_with(root) {
        bail!("路径越界");
    }
    Ok(())
}

fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("路径包含越界片段");
    }
    Ok(root.join(path))
}

fn locate_skill_md(root: &Path) -> Result<PathBuf> {
    for name in ["SKILL.md", "skill.md"] {
        let path = root.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("{} 缺少 SKILL.md", root.display())
}

fn find_skill_roots(root: &Path) -> Result<Vec<PathBuf>> {
    if locate_skill_md(root).is_ok() {
        return Ok(vec![root.to_path_buf()]);
    }
    let mut roots = Vec::new();
    if root.is_dir() {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() && locate_skill_md(&path).is_ok() {
                roots.push(path);
            }
        }
    }
    Ok(roots)
}

fn scan_skill_root(root: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if recursive {
        // Runtime discovery intentionally follows installed links, but visits each real directory once.
        return find_skill_roots_recursive_with_boundary(root, false);
    }
    find_skill_roots(root)
}

fn find_skill_roots_recursive(root: &Path) -> Result<Vec<PathBuf>> {
    find_skill_roots_recursive_with_boundary(root, true)
}

fn find_skill_roots_recursive_with_boundary(root: &Path, restrict_to_root: bool) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    fn walk(path: &Path, boundary: &Path, restrict: bool, seen: &mut HashSet<PathBuf>, result: &mut Vec<PathBuf>) -> Result<()> {
        let canonical = path.canonicalize()?;
        if restrict && !canonical.starts_with(boundary) { bail!("Skill 来源路径通过链接越出了仓库范围：{}", path.display()); }
        if !seen.insert(canonical) { return Ok(()); }
        if locate_skill_md(path).is_ok() { result.push(path.to_path_buf()); return Ok(()); }
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() && !matches!(path.file_name().and_then(|value| value.to_str()), Some(".git" | "node_modules")) {
                walk(&path, boundary, restrict, seen, result)?;
            }
        }
        Ok(())
    }
    if root.exists() { walk(root, &root.canonicalize()?, restrict_to_root, &mut seen, &mut result)?; }
    Ok(result)
}

fn read_skill_metadata(root: &Path) -> Result<(String, String)> {
    let raw = fs::read_to_string(locate_skill_md(root)?)?;
    let mut name = None;
    let mut description = None;
    if raw.starts_with("---") {
        for line in raw.lines().skip(1).take_while(|line| line.trim() != "---") {
            if let Some(value) = line.strip_prefix("name:") {
                name = Some(unquote(value));
            }
            if let Some(value) = line.strip_prefix("description:") {
                description = Some(unquote(value));
            }
        }
    }
    let fallback = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_string();
    Ok((
        name.filter(|value| !value.is_empty()).unwrap_or(fallback),
        description.unwrap_or_default(),
    ))
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches(['\'', '"']).trim().to_string()
}

fn safe_slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "skill".into()
    } else {
        slug.chars().take(48).collect()
    }
}

fn hash_directory(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_hash_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        if relative.components().count() == 1 && read_copy_marker(&root.join(&relative))?.is_some() {
            continue;
        }
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0]);
        let mut file = File::open(root.join(&relative))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_hash_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if matches!(name.to_str(), Some(".git" | "node_modules" | ".DS_Store")) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            bail!("Skill 中包含符号链接：{}", path.display());
        }
        if metadata.is_dir() {
            collect_hash_files(root, &path, files)?;
        } else if metadata.is_file() {
            files.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path, include_marker: bool) -> Result<()> {
    if path_exists_no_follow(target) {
        bail!("目标目录已存在：{}", target.display());
    }
    fs::create_dir_all(target)?;
    fn walk(source_root: &Path, current: &Path, target: &Path, include_marker: bool) -> Result<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let source_path = entry.path();
            let name = entry.file_name();
            if matches!(name.to_str(), Some(".git" | "node_modules" | ".DS_Store"))
                || (!include_marker && current == source_root && read_copy_marker(&source_path)?.is_some())
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&source_path)?;
            if metadata.file_type().is_symlink() {
                bail!("Skill 中包含符号链接：{}", source_path.display());
            }
            let relative = source_path.strip_prefix(source_root)?;
            let target_path = target.join(relative);
            if metadata.is_dir() {
                fs::create_dir_all(&target_path)?;
                walk(source_root, &source_path, target, include_marker)?;
            } else if metadata.is_file() {
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(source_path, target_path)?;
            }
        }
        Ok(())
    }
    if let Err(error) = walk(source, source, target, include_marker) {
        let _ = fs::remove_dir_all(target);
        return Err(error);
    }
    Ok(())
}

fn inspect_skill(root: &Path) -> Result<(u32, u32, Vec<String>)> {
    let mut files = Vec::new();
    collect_file_entries(root, root, &mut files)?;
    let script_count = files.iter().filter(|file| file.is_script).count() as u32;
    let mut risks = Vec::new();
    if script_count > 0 {
        risks.push(format!("包含 {script_count} 个脚本或可执行文件"));
    }
    for file in &files {
        if file.size > MAX_PREVIEW_BYTES || !file.is_script {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(root.join(&file.path)) {
            let lower = raw.to_ascii_lowercase();
            if [
                "rm -rf",
                "remove-item -recurse",
                "invoke-expression",
                "powershell -enc",
                "curl | sh",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            {
                risks.push(format!("{} 包含高风险命令片段", file.path));
            }
        }
    }
    risks.sort();
    risks.dedup();
    Ok((files.len() as u32, script_count, risks))
}

fn collect_file_entries(
    root: &Path,
    current: &Path,
    result: &mut Vec<SkillFileEntry>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if matches!(
            name.to_str(),
            Some(".git" | "node_modules" | ".DS_Store")
        ) || (current == root && read_copy_marker(&path)?.is_some()) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_file_entries(root, &path, result)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            result.push(SkillFileEntry {
                is_script: is_script_path(&relative),
                path: relative,
                size: metadata.len(),
            });
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

fn is_script_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("ps1" | "bat" | "cmd" | "sh" | "py" | "js" | "mjs" | "ts" | "exe")
    )
}

fn extract_archive(path: &Path, target: &Path) -> Result<()> {
    let archive_size = fs::metadata(path)?.len();
    if archive_size > MAX_ARCHIVE_BYTES {
        bail!("Skill 压缩包超过 256 MB 限制");
    }
    let file = File::open(path).with_context(|| format!("打开压缩包失败：{}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        bail!("Skill 压缩包文件数量超过限制");
    }
    let mut extracted = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
            bail!("压缩包内单个文件超过 64 MB 限制");
        }
        extracted = extracted.saturating_add(entry.size());
        if extracted > MAX_ARCHIVE_EXTRACTED_BYTES {
            bail!("Skill 压缩包解压后超过 512 MB 限制");
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("压缩包包含不安全路径"))?
            .to_path_buf();
        let output = target.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(output)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn clone_git_source(input: &ImportSkillInput, staging: &Path) -> Result<()> {
    let destination = staging.join("repository");
    let mut command = background_command("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(branch) = input
        .branch
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        validate_git_ref(branch)?;
        command.args(["--branch", branch]);
    }
    let output = command
        .arg("--")
        .arg(&input.source)
        .arg(&destination)
        .output()?;
    if !output.status.success() {
        bail!("Git 克隆失败：{}", String::from_utf8_lossy(&output.stderr));
    }
    if let Some(revision) = input
        .revision
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        validate_git_ref(revision)?;
        let fetch = background_command("git")
            .arg("-C")
            .arg(&destination)
            .args(["fetch", "--depth", "1", "origin", revision])
            .output()?;
        if !fetch.status.success() {
            bail!(
                "获取固定 Commit 失败：{}",
                String::from_utf8_lossy(&fetch.stderr)
            );
        }
        let checkout = background_command("git")
            .arg("-C")
            .arg(&destination)
            .args(["checkout", "--detach", revision])
            .output()?;
        if !checkout.status.success() {
            bail!(
                "切换固定 Commit 失败：{}",
                String::from_utf8_lossy(&checkout.stderr)
            );
        }
    }
    Ok(())
}

fn resolve_market_skill_root(repository: &Path, requested: &str, name: &str) -> Result<PathBuf> {
    let requested_basename = Path::new(requested)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested);
    let mut candidates = find_skill_roots_recursive(repository)?
        .into_iter()
        .filter_map(|root| {
            let relative = root
                .strip_prefix(repository)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let metadata_name = read_skill_metadata(&root)
                .map(|(value, _)| value)
                .unwrap_or_default();
            let basename = root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let rank = if relative.eq_ignore_ascii_case(requested) {
                0
            } else if metadata_name.eq_ignore_ascii_case(name) {
                1
            } else if basename.eq_ignore_ascii_case(requested_basename) {
                2
            } else {
                return None;
            };
            Some((rank, root.components().count(), root))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    candidates
        .into_iter()
        .next()
        .map(|(_, _, root)| root)
        .ok_or_else(|| anyhow!("仓库中未找到 Skill：{name}"))
}

fn git_head_revision(repository: &Path) -> Result<String> {
    let output = background_command("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        bail!(
            "读取 Git Commit 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let revision = String::from_utf8(output.stdout)?.trim().to_string();
    validate_git_ref(&revision)?;
    Ok(revision)
}

fn git_current_branch(repository: &Path) -> Option<String> {
    let output = background_command("git")
        .arg("-C")
        .arg(repository)
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

fn default_market_branch() -> String {
    "main".into()
}

fn default_true() -> bool {
    true
}

fn default_market_repositories() -> Vec<SkillMarketRepository> {
    vec![
        SkillMarketRepository {
            id: "builtin-anthropics-skills".into(),
            owner: "anthropics".into(),
            name: "skills".into(),
            branch: "main".into(),
            enabled: true,
            builtin: true,
            updated_at: 0,
        },
        SkillMarketRepository {
            id: "builtin-composio-awesome".into(),
            owner: "ComposioHQ".into(),
            name: "awesome-claude-skills".into(),
            branch: "master".into(),
            enabled: true,
            builtin: true,
            updated_at: 0,
        },
        SkillMarketRepository {
            id: "builtin-cexll-myclaude".into(),
            owner: "cexll".into(),
            name: "myclaude".into(),
            branch: "master".into(),
            enabled: true,
            builtin: true,
            updated_at: 0,
        },
        SkillMarketRepository {
            id: "builtin-jimliu-baoyu".into(),
            owner: "JimLiu".into(),
            name: "baoyu-skills".into(),
            branch: "main".into(),
            enabled: true,
            builtin: true,
            updated_at: 0,
        },
    ]
}

fn validate_market_repo_coordinate(owner: &str, name: &str, branch: &str) -> Result<()> {
    let valid_owner = !owner.is_empty()
        && owner.len() <= 39
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && owner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    let valid_name = !name.is_empty()
        && name.len() <= 100
        && !name.contains("..")
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if !valid_owner || !valid_name {
        bail!("GitHub Skill 仓库坐标无效");
    }
    validate_git_ref(branch)
}

fn market_repository_url(owner: &str, name: &str) -> String {
    format!("https://github.com/{owner}/{name}.git")
}

fn validate_github_source_url(value: &str) -> Result<()> {
    let normalized = value
        .trim()
        .strip_prefix("https://github.com/")
        .ok_or_else(|| anyhow!("Skill 市场仅允许 GitHub HTTPS 仓库"))?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut parts = normalized.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        bail!("GitHub Skill 仓库地址无效");
    }
    validate_market_repo_coordinate(owner, name, "main")
}

fn normalize_git_source(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn validate_git_ref(value: &str) -> Result<()> {
    if value.starts_with('-')
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || ".-_/".contains(ch)))
    {
        bail!("Git 分支或 Commit 格式无效");
    }
    Ok(())
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn historical_copy_marker(target: &Path, skill_id: &str, source_hash: &str) -> PathBuf {
        let manager = String::from_utf8(vec![99, 99, 45, 100, 111, 99, 107]).unwrap();
        let path = target.join(format!(".{manager}-skill.json"));
        fs::write(&path, serde_json::to_vec(&serde_json::json!({
            "managedBy": manager, "skillId": skill_id, "sourceHash": source_hash,
        })).unwrap()).unwrap();
        path
    }

    #[test]
    fn brand_migration_proves_copy_ownership_before_replacing_the_marker() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-brand-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("SKILL.md"), "# skill content stays byte-for-byte").unwrap();
        let source_hash = hash_directory(&root).unwrap();
        let previous = historical_copy_marker(&root, "owned-id", &source_hash);
        assert_eq!(hash_directory(&root).unwrap(), source_hash);
        assert!(!is_owned_copy(&root, "another-id", &source_hash).unwrap());
        assert!(!is_owned_copy(&root, "owned-id", &"0".repeat(64)).unwrap());
        assert!(previous.exists());
        assert!(!root.join(COPY_MARKER).exists());
        assert!(is_owned_copy(&root, "owned-id", &source_hash).unwrap());
        assert!(!previous.exists());
        assert_eq!(hash_directory(&root).unwrap(), source_hash);
        let marker: serde_json::Value = serde_json::from_slice(&fs::read(root.join(COPY_MARKER)).unwrap()).unwrap();
        assert_eq!(marker["managedBy"], "crewvela");
        assert_eq!(marker["sourceHash"], source_hash);
        // Interrupted cleanup can safely resume after the new marker exists.
        let previous = historical_copy_marker(&root, "owned-id", &source_hash);
        assert!(is_owned_copy(&root, "owned-id", &source_hash).unwrap());
        assert!(!previous.exists());
        remove_owned_copy(&root, "owned-id", &source_hash).unwrap();
        assert!(!root.exists());
    }

    #[test]
    fn brand_migration_preserves_modified_copies_and_unrecognized_marker_files() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-brand-conflict-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("SKILL.md"), "original").unwrap();
        let source_hash = hash_directory(&root).unwrap();
        let previous = historical_copy_marker(&root, "owned-id", &source_hash);
        fs::write(root.join("SKILL.md"), "user modification").unwrap();
        assert!(!is_owned_copy(&root, "owned-id", &source_hash).unwrap());
        assert!(previous.exists());
        assert!(!root.join(COPY_MARKER).exists());
        fs::write(root.join("SKILL.md"), "original").unwrap();
        let mut invalid: serde_json::Value = serde_json::from_slice(&fs::read(&previous).unwrap()).unwrap();
        invalid["managedBy"] = serde_json::json!("user-owned");
        fs::write(&previous, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert_ne!(hash_directory(&root).unwrap(), source_hash);
        assert!(!is_owned_copy(&root, "owned-id", &source_hash).unwrap());
        fs::write(root.join(COPY_MARKER), "unrelated user file").unwrap();
        assert!(!is_owned_copy(&root, "owned-id", &source_hash).unwrap());
        assert_eq!(fs::read_to_string(root.join(COPY_MARKER)).unwrap(), "unrelated user file");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn brand_marker_publication_never_overwrites_a_concurrent_user_file() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-brand-publish-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("SKILL.md"), "original").unwrap();
        let hash = hash_directory(&root).unwrap();
        let previous = historical_copy_marker(&root, "owned-id", &hash);
        let (mut marker, raw) = read_copy_marker(&previous).unwrap().unwrap();
        marker["managedBy"] = serde_json::json!("crewvela");
        fs::write(root.join(COPY_MARKER), "concurrent user content").unwrap();
        assert!(publish_migrated_copy_marker(&root, &previous, &raw, &marker).is_err());
        assert_eq!(fs::read_to_string(root.join(COPY_MARKER)).unwrap(), "concurrent user content");
        assert_eq!(fs::read(&previous).unwrap(), raw);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn brand_migration_refreshes_a_persisted_copy_and_retains_its_source_hash() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-brand-snapshot-{}", Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: brand-copy\n---\n# Source").unwrap();
        let center = SkillCenter::open_at(root.join("center")).unwrap();
        let asset = center.import(ImportSkillInput {
            source_type: "local".into(), source: source.display().to_string(),
            branch: None, subpath: None, revision: None,
        }).unwrap().remove(0);
        let target = root.join("runtime-copy");
        copy_directory(Path::new(&asset.central_path), &target, false).unwrap();
        let previous = historical_copy_marker(&target, &asset.id, &asset.content_hash);
        let input = DeploySkillInput {
            skill_id: asset.id.clone(), runtime: "claude_code".into(), scope: "project".into(),
            project_path: Some(root.display().to_string()), task_id: None, agent_id: None, trust_confirmed: true,
        };
        assert!(preflight_target(&asset, &target, None).is_err(), "marker alone must not claim a user directory");
        center.conn.lock().unwrap().execute(
            "INSERT INTO skill_deployments(id,identity_key,skill_id,runtime,scope,project_path,
             target_path,mode,status,source_hash,updated_at) VALUES(?1,?2,?3,'claude_code','project',?4,?5,'copy','synced',?6,0)",
            params!["deployment", deployment_identity(&input), asset.id, root.display().to_string(),
                target.display().to_string(), asset.content_hash],
        ).unwrap();
        let snapshot = center.snapshot().unwrap();
        assert_eq!(snapshot.deployments[0].status, "synced");
        assert_eq!(snapshot.deployments[0].source_hash, asset.content_hash);
        assert_eq!(snapshot.deployments[0].target_hash.as_deref(), Some(asset.content_hash.as_str()));
        assert!(!previous.exists());
        assert!(target.join(COPY_MARKER).is_file());
        drop(center);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repository_scanning_rejects_link_escape_while_runtime_discovery_handles_links_and_cycles() {
        let sandbox = std::env::temp_dir().join(format!("skill-source-links-{}", Uuid::new_v4()));
        let repository = sandbox.join("repository"); let external = sandbox.join("external");
        fs::create_dir_all(&repository).unwrap(); fs::create_dir_all(&external).unwrap();
        fs::write(external.join("SKILL.md"), "---\nname: outside\n---\nprivate skill").unwrap();
        #[cfg(windows)]
        { junction::create(&external, repository.join("external")).unwrap(); junction::create(&repository, repository.join("cycle")).unwrap(); }
        #[cfg(unix)]
        { std::os::unix::fs::symlink(&external, repository.join("external")).unwrap(); std::os::unix::fs::symlink(&repository, repository.join("cycle")).unwrap(); }
        assert!(find_skill_roots_recursive(&repository).is_err());
        let discovered = scan_skill_root(&repository, true).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].canonicalize().unwrap(), external.canonicalize().unwrap());
        #[cfg(windows)]
        { junction::delete(repository.join("external")).unwrap(); junction::delete(repository.join("cycle")).unwrap(); }
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn slug_is_safe() {
        assert_eq!(safe_slug("Frontend Design / 高级"), "frontend-design");
    }

    #[test]
    fn safe_join_rejects_parent() {
        assert!(safe_join(Path::new("C:/safe"), "../escape").is_err());
    }

    #[test]
    fn runtime_matrix_excludes_retired_cli() {
        let adapters = runtime_adapters().unwrap();
        assert_eq!(adapters.iter().map(|item| item.runtime.as_str()).collect::<Vec<_>>(), vec!["claude_code", "codex", "kimi_code", "pi", "opencode"]);
        assert!(!adapters.iter().any(|item| item.runtime == "minimax_code"));
    }

    #[test]
    fn import_and_project_deploy_are_real_and_scoped() {
        let sandbox = std::env::temp_dir().join(format!("crewvela-skill-test-{}", Uuid::new_v4()));
        let source = sandbox.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: frontend-test\ndescription: test\n---\n# Test",
        )
        .unwrap();
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let asset = center
            .import(ImportSkillInput {
                source_type: "local".into(),
                source: source.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap()
            .remove(0);
        let project = sandbox.join("project");
        fs::create_dir_all(&project).unwrap();
        let deployed = center
            .deploy_batch(vec![DeploySkillInput {
                skill_id: asset.id.clone(),
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            }])
            .unwrap();
        assert_eq!(deployed.len(), 1);
        assert!(Path::new(&deployed[0].target_path).exists());
        center
            .deploy_batch(vec![DeploySkillInput {
                skill_id: asset.id,
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            }])
            .unwrap();
        assert_eq!(center.list_deployments().unwrap().len(), 1);
        center.undeploy_batch(vec![deployed[0].id.clone()]).unwrap();
        assert!(!Path::new(&deployed[0].target_path).exists());
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn deploy_verifies_skill_md_reaches_target_root() {
        let sandbox = std::env::temp_dir().join(format!("crewvela-skill-verify-{}", Uuid::new_v4()));
        let source = sandbox.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: verify-test\ndescription: test\n---\n# Test",
        )
        .unwrap();
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let asset = center
            .import(ImportSkillInput {
                source_type: "local".into(),
                source: source.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap()
            .remove(0);
        // 中央库里的 SKILL.md 消失（数据损坏）：部署必须在 runtime 发现层面失败，
        // 不落 synced 记录、不留无法被 runtime 发现的空壳目标目录。
        fs::remove_file(Path::new(&asset.central_path).join("SKILL.md")).unwrap();
        let project = sandbox.join("project");
        fs::create_dir_all(&project).unwrap();
        let result = center.deploy_batch(vec![DeploySkillInput {
            skill_id: asset.id,
            runtime: "claude_code".into(),
            scope: "project".into(),
            project_path: Some(project.display().to_string()),
            task_id: None,
            agent_id: None,
            trust_confirmed: true,
        }]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SKILL.md"));
        assert!(center.list_deployments().unwrap().is_empty());
        assert!(!project.join(".claude/skills").join("verify-test").exists());
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn deployment_refuses_unowned_directory() {
        let sandbox =
            std::env::temp_dir().join(format!("crewvela-skill-conflict-{}", Uuid::new_v4()));
        let source = sandbox.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: conflict-test\n---\n# Test",
        )
        .unwrap();
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let asset = center
            .import(ImportSkillInput {
                source_type: "local".into(),
                source: source.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap()
            .remove(0);
        let clean_source = sandbox.join("clean-source");
        fs::create_dir_all(&clean_source).unwrap();
        fs::write(
            clean_source.join("SKILL.md"),
            "---\nname: clean-test\n---\n# Test",
        )
        .unwrap();
        let clean_asset = center
            .import(ImportSkillInput {
                source_type: "local".into(),
                source: clean_source.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap()
            .remove(0);
        let project = sandbox.join("project");
        let target = project.join(".claude/skills/conflict-test");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("mine.txt"), "user").unwrap();
        let result = center.deploy_batch(vec![
            DeploySkillInput {
                skill_id: clean_asset.id,
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            },
            DeploySkillInput {
                skill_id: asset.id,
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            },
        ]);
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(target.join("mine.txt")).unwrap(), "user");
        assert!(!project.join(".claude/skills/clean-test").exists());
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn archive_import_finds_nested_skill_and_hashes_it() {
        let sandbox =
            std::env::temp_dir().join(format!("crewvela-skill-archive-{}", Uuid::new_v4()));
        fs::create_dir_all(&sandbox).unwrap();
        let archive_path = sandbox.join("bundle.skill");
        let file = File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "bundle/nested/SKILL.md",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(b"---\nname: nested-skill\n---\n# Test")
            .unwrap();
        archive.finish().unwrap();
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let imported = center
            .import(ImportSkillInput {
                source_type: "archive".into(),
                source: archive_path.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].name, "nested-skill");
        assert_eq!(imported[0].content_hash.len(), 64);
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn git_import_records_actual_checked_out_revision() {
        let sandbox = std::env::temp_dir().join(format!("crewvela-skill-git-{}", Uuid::new_v4()));
        let repository = sandbox.join("repository");
        fs::create_dir_all(repository.join("skills/example")).unwrap();
        fs::write(
            repository.join("skills/example/SKILL.md"),
            "---\nname: git-skill\n---\n# Test",
        )
        .unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.name", "CrewVela Test"],
            vec!["config", "user.email", "test@crewvela.local"],
            vec!["add", "."],
            vec!["commit", "-m", "test"],
        ] {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let expected = git_head_revision(&repository).unwrap();
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let imported = center
            .import(ImportSkillInput {
                source_type: "git".into(),
                source: repository.display().to_string(),
                branch: None,
                subpath: Some("skills/example".into()),
                revision: None,
            })
            .unwrap();
        assert_eq!(
            imported[0].source_revision.as_deref(),
            Some(expected.as_str())
        );
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn market_repositories_seed_and_custom_sources_are_managed() {
        let sandbox = std::env::temp_dir().join(format!("crewvela-skill-market-{}", Uuid::new_v4()));
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let defaults = center.list_market_repositories().unwrap();
        assert_eq!(defaults.len(), 4);
        assert!(defaults.iter().all(|repository| repository.builtin));

        let custom = center
            .save_market_repository(SaveSkillMarketRepositoryInput {
                id: None,
                owner: "example-org".into(),
                name: "agent-skills".into(),
                branch: "main".into(),
                enabled: true,
            })
            .unwrap();
        assert!(!custom.builtin);
        assert_eq!(center.list_market_repositories().unwrap().len(), 5);

        let disabled = center
            .save_market_repository(SaveSkillMarketRepositoryInput {
                id: Some(custom.id.clone()),
                owner: custom.owner.clone(),
                name: custom.name.clone(),
                branch: custom.branch.clone(),
                enabled: false,
            })
            .unwrap();
        assert!(!disabled.enabled);
        center.delete_market_repository(&custom.id).unwrap();
        assert_eq!(center.list_market_repositories().unwrap().len(), 4);
        assert!(center
            .delete_market_repository("builtin-anthropics-skills")
            .is_err());
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn market_repository_and_registry_inputs_are_hardened() {
        assert!(validate_market_repo_coordinate("anthropics", "skills", "main").is_ok());
        assert!(validate_market_repo_coordinate("../escape", "skills", "main").is_err());
        assert!(validate_market_repo_coordinate("owner", "repo/name", "main").is_err());
        assert!(validate_github_source_url("https://github.com/anthropics/skills.git").is_ok());
        assert!(validate_github_source_url("https://example.com/anthropics/skills.git").is_err());

        let sandbox =
            std::env::temp_dir().join(format!("crewvela-skill-market-key-{}", Uuid::new_v4()));
        let center = SkillCenter::open_at(sandbox.join("center")).unwrap();
        let error = center
            .install_market(InstallSkillMarketInput {
                source_type: "skills_sh".into(),
                source_url: "https://github.com/anthropics/skills.git".into(),
                branch: "main".into(),
                subpath: "frontend-design".into(),
                name: "frontend-design".into(),
                key: None,
            })
            .unwrap_err();
        assert!(error.to_string().contains("唯一 key"));
        let _ = fs::remove_dir_all(sandbox);
    }

    #[test]
    fn market_resolver_finds_nested_skills_by_actual_path_or_metadata_name() {
        let sandbox =
            std::env::temp_dir().join(format!("crewvela-skill-market-nested-{}", Uuid::new_v4()));
        let nested = sandbox.join("wrapper/skills/frontend");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: design-taste-frontend\ndescription: test\n---\n# Skill",
        )
        .unwrap();

        let exact =
            resolve_market_skill_root(&sandbox, "wrapper/skills/frontend", "design-taste-frontend")
                .unwrap();
        assert_eq!(exact, nested);

        let by_name =
            resolve_market_skill_root(&sandbox, "design-taste-frontend", "design-taste-frontend")
                .unwrap();
        assert_eq!(by_name, nested);
        let _ = fs::remove_dir_all(sandbox);
    }
}
