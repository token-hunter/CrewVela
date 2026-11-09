//! Reviewed, immutable Skill versions. Checking never mutates a deployed directory.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileChange {
    pub path: String,
    pub kind: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateCheck {
    pub id: String,
    pub skill_id: String,
    pub status: String,
    pub message: String,
    pub checked_at: i64,
    pub base_hash: String,
    pub candidate: Option<SkillAsset>,
    pub changes: Vec<SkillFileChange>,
    pub omitted_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersionInfo {
    pub id: String,
    pub saved_at: i64,
    pub asset: SkillAsset,
}

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_update_tracking (
      skill_id TEXT PRIMARY KEY, baseline_hash TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS skill_update_checks (
      skill_id TEXT PRIMARY KEY, record TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS skill_versions (
      id TEXT PRIMARY KEY, skill_id TEXT NOT NULL, saved_at INTEGER NOT NULL, asset TEXT NOT NULL);
      INSERT OR IGNORE INTO skill_update_tracking(skill_id,baseline_hash)
      SELECT id,content_hash FROM skill_assets;",
    )?;
    Ok(())
}

impl SkillCenter {
    pub(super) fn register_update_baseline(&self, asset: &SkillAsset, pinned: bool) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR IGNORE INTO skill_update_tracking(skill_id,baseline_hash,pinned) VALUES(?1,?2,?3)",
            params![asset.id, asset.content_hash, pinned],
        )?;
        Ok(())
    }

    pub fn update_checks(&self) -> Result<Vec<SkillUpdateCheck>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT record FROM skill_update_checks WHERE skill_id IN (SELECT id FROM skill_assets)")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter()
            .map(|raw| serde_json::from_str(raw).map_err(Into::into))
            .collect()
    }

    fn save_check(&self, check: &SkillUpdateCheck) -> Result<()> {
        let old: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT record FROM skill_update_checks WHERE skill_id=?1",
                [&check.skill_id],
                |row| row.get(0),
            )
            .optional()?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO skill_update_checks(skill_id,record) VALUES(?1,?2) ON CONFLICT(skill_id) DO UPDATE SET record=excluded.record",
            params![check.skill_id, serde_json::to_string(check)?],
        )?;
        // Discard only an unchanged, superseded candidate, never an active or archived version.
        if let Some(old) = old
            .and_then(|raw| serde_json::from_str::<SkillUpdateCheck>(&raw).ok())
            .and_then(|c| c.candidate)
        {
            let path = Path::new(&old.central_path);
            if check
                .candidate
                .as_ref()
                .is_none_or(|a| a.central_path != old.central_path)
                && self
                    .asset(&check.skill_id)?
                    .is_none_or(|a| a.central_path != old.central_path)
                && !self
                    .versions(&check.skill_id)?
                    .iter()
                    .any(|v| v.asset.central_path == old.central_path)
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("version-"))
                && path.is_dir()
                && hash_directory(path).is_ok_and(|hash| hash == old.content_hash)
            {
                ensure_within(path, &self.root.join("assets"))?;
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }

    fn assert_clean_center(&self, asset: &SkillAsset) -> Result<()> {
        let baseline: String = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT baseline_hash FROM skill_update_tracking WHERE skill_id=?1",
                [&asset.id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_else(|| asset.content_hash.clone());
        if hash_directory(Path::new(&asset.central_path))? != baseline
            || asset.content_hash != baseline
        {
            bail!("中央 Skill 存在本地修改，请先另行保存或重新导入；更新未执行");
        }
        Ok(())
    }

    pub fn check_update(
        &self,
        skill_id: &str,
        replacement_archive: Option<String>,
    ) -> Result<SkillUpdateCheck> {
        let _guard = self.import_lock.lock().unwrap();
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        self.register_update_baseline(&asset, false)?;
        let mut check = SkillUpdateCheck {
            id: Uuid::new_v4().to_string(),
            skill_id: skill_id.into(),
            status: "error".into(),
            message: String::new(),
            checked_at: now_ts(),
            base_hash: asset.content_hash.clone(),
            candidate: None,
            changes: Vec::new(),
            omitted_files: 0,
        };
        let staging = self.root.join("tmp").join(format!("update-{}", check.id));
        fs::create_dir_all(&staging)?;
        let result = (|| -> Result<()> {
            self.assert_clean_center(&asset)?;
            let pinned: bool = self.conn.lock().unwrap().query_row(
                "SELECT pinned FROM skill_update_tracking WHERE skill_id=?1",
                [skill_id],
                |row| row.get(0),
            )?;
            if pinned {
                check.status = "pinned".into();
                check.message =
                    "已固定 Commit，不自动追踪分支；如需更换版本请重新导入指定 Commit".into();
                return Ok(());
            }
            let mut candidate = asset.clone();
            let source = match asset.source_type.as_str() {
                "git" | "market" | "skills_sh" => {
                    let input = ImportSkillInput {
                        source_type: "git".into(),
                        source: asset
                            .source_ref
                            .clone()
                            .ok_or_else(|| anyhow!("缺少 Git 来源"))?,
                        branch: asset.source_branch.clone(),
                        subpath: asset.source_subpath.clone(),
                        revision: None,
                    };
                    clone_git_source(&input, &staging)?;
                    let repo = staging.join("repository");
                    candidate.source_revision = Some(git_head_revision(&repo)?);
                    let root =
                        select_source_skill(&repo, asset.source_subpath.as_deref(), &asset.name)?;
                    candidate.source_subpath = Some(
                        root.strip_prefix(&repo)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                    root
                }
                "local" | "adopted" => {
                    let root = PathBuf::from(
                        asset
                            .source_ref
                            .as_deref()
                            .ok_or_else(|| anyhow!("缺少本地来源"))?,
                    );
                    select_source_skill(&root, asset.source_subpath.as_deref(), &asset.name)?
                }
                "archive" => {
                    let Some(path) = replacement_archive else {
                        check.status = "manual".into();
                        check.message =
                            "压缩包没有远程更新源，请选择新版 .zip / .skill 文件进行比较".into();
                        return Ok(());
                    };
                    extract_archive(Path::new(&path), &staging)?;
                    candidate.source_ref = Some(path);
                    select_source_skill(&staging, None, &asset.name)?
                }
                _ => {
                    check.status = "manual".into();
                    check.message = "该来源暂不能自动检测，请重新导入新版 Skill".into();
                    return Ok(());
                }
            };
            let candidate_path = self
                .root
                .join("assets")
                .join(format!("version-{}", check.id));
            ensure_non_overlapping(&source, &candidate_path)?;
            copy_directory(&source, &candidate_path, false)?;
            candidate.content_hash = hash_directory(&candidate_path)?;
            if candidate.content_hash == asset.content_hash {
                fs::remove_dir_all(&candidate_path)?;
                check.status = "up_to_date".into();
                check.message = "Skill 内容与来源一致".into();
                return Ok(());
            }
            candidate.central_path = candidate_path.display().to_string();
            // Keep the stable asset name: it determines existing deployment directory names.
            candidate.description = read_skill_metadata(&candidate_path)?.1;
            let (files, scripts, risks) = inspect_skill(&candidate_path)?;
            candidate.file_count = files;
            candidate.script_count = scripts;
            candidate.risk_summary = risks;
            candidate.trust_state = "unknown".into();
            candidate.updated_at = now_ts();
            let (changes, omitted) = version_diff(Path::new(&asset.central_path), &candidate_path)?;
            check.changes = changes;
            check.omitted_files = omitted;
            check.candidate = Some(candidate);
            check.status = "available".into();
            check.message = "发现新内容，确认差异后更新中央库；Runtime 不会自动更新".into();
            Ok(())
        })();
        let _ = fs::remove_dir_all(&staging);
        if let Err(error) = result {
            check.message = error.to_string();
            check.status = "error".into();
        }
        self.save_check(&check)?;
        Ok(check)
    }

    pub fn check_all_updates(&self) -> Result<Vec<SkillUpdateCheck>> {
        self.list_assets()?
            .iter()
            .map(|asset| self.check_update(&asset.id, None))
            .collect()
    }

    pub fn versions(&self, skill_id: &str) -> Result<Vec<SkillVersionInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id,saved_at,asset FROM skill_versions WHERE skill_id=?1 ORDER BY saved_at DESC,rowid DESC")?;
        let rows = stmt
            .query_map([skill_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, saved_at, raw)| {
                Ok(SkillVersionInfo {
                    id,
                    saved_at,
                    asset: serde_json::from_str(&raw)?,
                })
            })
            .collect()
    }

    pub fn prepare_rollback(&self, skill_id: &str, version_id: &str) -> Result<SkillUpdateCheck> {
        let _guard = self.import_lock.lock().unwrap();
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        self.assert_clean_center(&asset)?;
        let previous = self
            .versions(skill_id)?
            .into_iter()
            .find(|v| v.id == version_id)
            .ok_or_else(|| anyhow!("版本不存在"))?;
        if hash_directory(Path::new(&previous.asset.central_path))? != previous.asset.content_hash {
            bail!("历史版本已被修改，拒绝回滚");
        }
        let (changes, omitted_files) = version_diff(
            Path::new(&asset.central_path),
            Path::new(&previous.asset.central_path),
        )?;
        let check = SkillUpdateCheck {
            id: Uuid::new_v4().to_string(),
            skill_id: skill_id.into(),
            status: "available".into(),
            message: "回滚预览：仅更改中央库，Runtime 需要另行选择同步".into(),
            checked_at: now_ts(),
            base_hash: asset.content_hash,
            candidate: Some(previous.asset),
            changes,
            omitted_files,
        };
        self.save_check(&check)?;
        Ok(check)
    }

    pub fn apply_update(
        &self,
        skill_id: &str,
        check_id: &str,
        trust_confirmed: bool,
    ) -> Result<SkillAsset> {
        let _import = self.import_lock.lock().unwrap();
        let _deployment = self.deployment_lock.lock().unwrap();
        if !trust_confirmed {
            bail!("请审阅差异与风险后确认更新");
        }
        let current = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        if current.trust_state == "blocked" {
            bail!("该 Skill 已被阻止，请先解除阻止");
        }
        self.assert_clean_center(&current)?;
        let mut check = self
            .update_checks()?
            .into_iter()
            .find(|c| c.skill_id == skill_id && c.id == check_id)
            .ok_or_else(|| anyhow!("检查结果已失效，请重新检查"))?;
        if check.status != "available" || check.base_hash != current.content_hash {
            bail!("中央版本已变化，请重新检查");
        }
        let mut candidate = check
            .candidate
            .clone()
            .ok_or_else(|| anyhow!("候选版本不存在"))?;
        ensure_within(
            Path::new(&candidate.central_path),
            &self.root.join("assets"),
        )?;
        if hash_directory(Path::new(&candidate.central_path))? != candidate.content_hash {
            bail!("候选版本已变化，请重新检查");
        }
        candidate.trust_state = "trusted".into();
        candidate.updated_at = now_ts();
        check.status = "applied".into();
        check.message = "中央版本已切换，请选择需要同步的 Runtime".into();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO skill_versions(id,skill_id,saved_at,asset) VALUES(?1,?2,?3,?4)",
            params![
                Uuid::new_v4().to_string(),
                skill_id,
                now_ts(),
                serde_json::to_string(&current)?
            ],
        )?;
        tx.execute("UPDATE skill_assets SET description=?1,source_ref=?2,source_subpath=?3,source_revision=?4,
            central_path=?5,content_hash=?6,trust_state='trusted',file_count=?7,script_count=?8,risk_summary=?9,updated_at=?10 WHERE id=?11",
            params![candidate.description,candidate.source_ref,candidate.source_subpath,candidate.source_revision,candidate.central_path,
                candidate.content_hash,candidate.file_count,candidate.script_count,serde_json::to_string(&candidate.risk_summary)?,candidate.updated_at,skill_id])?;
        tx.execute(
            "UPDATE skill_update_tracking SET baseline_hash=?1 WHERE skill_id=?2",
            params![candidate.content_hash, skill_id],
        )?;
        tx.execute(
            "UPDATE skill_update_checks SET record=?1 WHERE skill_id=?2",
            params![serde_json::to_string(&check)?, skill_id],
        )?;
        tx.commit()?;
        Ok(candidate)
    }

    pub(super) fn deployment_version_asset(
        &self,
        deployment: &SkillDeployment,
    ) -> Result<SkillAsset> {
        let current = self
            .asset(&deployment.skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        let mut versions = vec![current];
        versions.extend(
            self.versions(&deployment.skill_id)?
                .into_iter()
                .map(|v| v.asset),
        );
        versions
            .into_iter()
            .find(|a| {
                a.content_hash == deployment.source_hash
                    && (deployment.mode == "copy"
                        || link_points_to(
                            Path::new(&deployment.target_path),
                            Path::new(&a.central_path),
                        ))
            })
            .ok_or_else(|| anyhow!("无法验证部署版本或链接目标，请保留本地内容后重新部署"))
    }

    /// Stage copies first; shared physical targets must be selected as a group.
    pub fn sync_update_deployments(
        &self,
        skill_id: &str,
        deployment_ids: Vec<String>,
        expected_hash: &str,
    ) -> Result<Vec<SkillDeployment>> {
        let _guard = self.deployment_lock.lock().unwrap();
        let asset = self
            .asset(skill_id)?
            .ok_or_else(|| anyhow!("Skill 不存在"))?;
        self.assert_clean_center(&asset)?;
        if asset.content_hash != expected_hash || asset.trust_state != "trusted" {
            bail!("中央版本或信任状态已变化，请刷新后重试");
        }
        let all = self.list_deployments()?;
        let ids: HashSet<_> = deployment_ids.into_iter().collect();
        let selected: Vec<_> = all
            .iter()
            .filter(|d| ids.contains(&d.id) && d.skill_id == skill_id)
            .cloned()
            .collect();
        if selected.len() != ids.len() {
            bail!("部署选择已失效");
        }
        if selected.iter().any(|deployment| deployment.runtime == "oh_my_pi") {
            bail!("Oh My Pi 已移除，历史 Skill 部署不可同步");
        }
        let mut targets = HashSet::new();
        let mut swaps = Vec::new();
        for record in &selected {
            if all
                .iter()
                .any(|other| other.target_path == record.target_path && !ids.contains(&other.id))
            {
                bail!(
                    "多个 Runtime 共用目录，必须一起选择：{}",
                    record.target_path
                );
            }
            let old = self.deployment_version_asset(record)?;
            let target = PathBuf::from(&record.target_path);
            preflight_target(&old, &target, Some(record))?;
            if hash_directory(&target)? != record.source_hash {
                bail!("Runtime 存在本地修改：{}", record.target_path);
            }
            ensure_non_overlapping(Path::new(&asset.central_path), &target)?;
            if targets.insert(target.clone()) {
                swaps.push(VersionSwap::prepare(&asset, &target, old, record.clone())?);
            }
        }
        // Repeat preflight after staging, before any destination is changed.
        for swap in &swaps {
            preflight_target(&swap.old_asset, &swap.target, Some(&swap.record))?;
            if hash_directory(&swap.target)? != swap.record.source_hash {
                bail!("部署在准备期间发生变化");
            }
        }
        for swap in &mut swaps {
            swap.activate()?;
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for record in &selected {
            tx.execute("UPDATE skill_deployments SET mode='copy',status='synced',source_hash=?1,target_hash=?1,last_error=NULL,updated_at=?2 WHERE id=?3",
                params![asset.content_hash,now_ts(),record.id])?;
        }
        tx.commit()?;
        drop(conn);
        for swap in &mut swaps {
            swap.committed = true;
        }
        Ok(self
            .list_deployments()?
            .into_iter()
            .filter(|d| ids.contains(&d.id))
            .collect())
    }
}

struct VersionSwap {
    target: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    old_asset: SkillAsset,
    record: SkillDeployment,
    new_hash: String,
    moved_old: bool,
    installed: bool,
    committed: bool,
}
impl VersionSwap {
    fn prepare(
        asset: &SkillAsset,
        target: &Path,
        old_asset: SkillAsset,
        record: SkillDeployment,
    ) -> Result<Self> {
        let parent = target.parent().ok_or_else(|| anyhow!("目标路径无父目录"))?;
        let nonce = Uuid::new_v4();
        let staged = parent.join(format!(".crewvela-update-{nonce}"));
        let backup = parent.join(format!(".crewvela-backup-{nonce}"));
        copy_directory(Path::new(&asset.central_path), &staged, false)?;
        let swap = Self {
            target: target.into(),
            staged,
            backup,
            old_asset,
            record,
            new_hash: asset.content_hash.clone(),
            moved_old: false,
            installed: false,
            committed: false,
        };
        fs::write(
            swap.staged.join(COPY_MARKER),
            serde_json::to_vec(
                &serde_json::json!({"managedBy":"crewvela","skillId":asset.id,"sourceHash":asset.content_hash}),
            )?,
        )?;
        if hash_directory(&swap.staged)? != asset.content_hash {
            bail!("版本复制校验失败");
        }
        Ok(swap)
    }
    fn activate(&mut self) -> Result<()> {
        fs::rename(&self.target, &self.backup)?;
        self.moved_old = true;
        fs::rename(&self.staged, &self.target)?;
        self.installed = true;
        Ok(())
    }
}
impl Drop for VersionSwap {
    fn drop(&mut self) {
        if self.committed {
            if self.record.mode == "copy" {
                let _ = remove_owned_copy(
                    &self.backup,
                    &self.record.skill_id,
                    &self.record.source_hash,
                );
            } else {
                let _ = remove_owned_link(&self.backup, Path::new(&self.old_asset.central_path));
            }
        } else {
            if self.installed
                && remove_owned_copy(&self.target, &self.record.skill_id, &self.new_hash).is_err()
            {
                eprintln!(
                    "Skill 同步回滚时目标被修改，保留现场与备份：{}",
                    self.backup.display()
                );
                return;
            }
            if self.moved_old {
                if let Err(error) = fs::rename(&self.backup, &self.target) {
                    eprintln!(
                        "Skill 同步回滚失败，原内容保留于 {}：{error}",
                        self.backup.display()
                    );
                }
            }
        }
        if self.staged.exists() {
            let _ = fs::remove_dir_all(&self.staged);
        }
    }
}

fn select_source_skill(root: &Path, subpath: Option<&str>, name: &str) -> Result<PathBuf> {
    let selected = match subpath {
        Some(path) if !path.is_empty() => safe_join(root, path)?,
        _ => root.into(),
    };
    ensure_within(&selected, root)?;
    if locate_skill_md(&selected).is_ok() {
        return Ok(selected);
    }
    let matches: Vec<_> = find_skill_roots_recursive(&selected)?
        .into_iter()
        .filter(|path| read_skill_metadata(path).is_ok_and(|(n, _)| n == name))
        .collect();
    if matches.len() != 1 {
        bail!("来源中无法唯一定位该 Skill，请重新导入并指定准确子目录");
    }
    Ok(matches[0].clone())
}

fn preview_text(path: &Path) -> (Option<String>, bool) {
    if !path.is_file() {
        return (None, false);
    }
    let Ok(metadata) = fs::metadata(path) else {
        return (None, true);
    };
    if metadata.len() > 8192 {
        return (
            Some("文件超过 8 KiB；请通过文件预览审阅完整内容".into()),
            true,
        );
    }
    match fs::read_to_string(path) {
        Ok(text) => (Some(text), false),
        Err(_) => (Some("二进制文件或非 UTF-8 内容".into()), true),
    }
}

fn version_diff(before: &Path, after: &Path) -> Result<(Vec<SkillFileChange>, usize)> {
    let mut old = Vec::new();
    let mut new = Vec::new();
    collect_hash_files(before, before, &mut old)?;
    collect_hash_files(after, after, &mut new)?;
    let paths: BTreeSet<_> = old
        .into_iter()
        .chain(new)
        .filter(|p| p != Path::new(COPY_MARKER))
        .collect();
    let mut result = Vec::new();
    let mut omitted = 0;
    for path in paths {
        let a = before.join(&path);
        let b = after.join(&path);
        // Streaming digest avoids allocating arbitrary-sized scripts or resources.
        let digest = |p: &Path| -> Result<Option<Vec<u8>>> {
            if !p.exists() {
                return Ok(None);
            }
            let mut h = Sha256::new();
            let mut f = File::open(p)?;
            let mut buf = [0u8; 65536];
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(Some(h.finalize().to_vec()))
        };
        if digest(&a)? == digest(&b)? {
            continue;
        }
        if result.len() >= 64 {
            omitted += 1;
            continue;
        }
        let (old_text, x) = preview_text(&a);
        let (new_text, y) = preview_text(&b);
        result.push(SkillFileChange {
            path: path.to_string_lossy().replace('\\', "/"),
            kind: if !a.exists() {
                "added"
            } else if !b.exists() {
                "deleted"
            } else {
                "modified"
            }
            .into(),
            before: old_text,
            after: new_text,
            truncated: x || y,
        });
    }
    Ok((result, omitted))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        center: SkillCenter,
        asset: SkillAsset,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("crewvela-update-test-{}", Uuid::new_v4()));
            let source = root.join("source");
            fs::create_dir_all(&source).unwrap();
            fs::write(
                source.join("SKILL.md"),
                "---\nname: version-test\n---\nVersion one",
            )
            .unwrap();
            let center = SkillCenter::open_at(root.join("center")).unwrap();
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
            Self {
                root,
                source,
                center,
                asset,
            }
        }
        fn change(&self) {
            fs::write(
                self.source.join("SKILL.md"),
                "---\nname: version-test\n---\nVersion two",
            )
            .unwrap();
        }
        fn deploy(&self, runtime: &str) -> SkillDeployment {
            let project = self.root.join(format!("project-{runtime}"));
            fs::create_dir_all(&project).unwrap();
            self.center
                .deploy_batch(vec![DeploySkillInput {
                    skill_id: self.asset.id.clone(),
                    runtime: runtime.into(),
                    scope: "project".into(),
                    project_path: Some(project.display().to_string()),
                    task_id: None,
                    agent_id: None,
                    trust_confirmed: true,
                }])
                .unwrap()
                .remove(0)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn corrupted_runtime_target_is_not_reported_as_synced() {
        let f = Fixture::new();
        let deployed = f.deploy("claude_code");
        let target = Path::new(&deployed.target_path);
        if deployed.mode == "copy" { fs::remove_dir_all(target).unwrap(); }
        else { remove_owned_link(target, Path::new(&f.asset.central_path)).unwrap(); }
        fs::write(target, "external replacement file").unwrap();
        let snapshot = f.center.snapshot().unwrap();
        let state = snapshot.deployments.iter().find(|item| item.id == deployed.id).unwrap();
        assert_eq!(state.status, "failed");
        assert!(state.last_error.as_deref().is_some_and(|message| message.contains("无法校验")));
        assert!(f.center.undeploy(&deployed.id).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "external replacement file");
    }

    #[test]
    fn check_apply_selective_sync_and_rollback_form_a_real_loop() {
        let f = Fixture::new();
        let claude = f.deploy("claude_code");
        let codex = f.deploy("codex");
        assert_eq!(
            f.center.check_update(&f.asset.id, None).unwrap().status,
            "up_to_date"
        );
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        assert_eq!(check.status, "available");
        assert_eq!(check.changes.len(), 1);
        assert!(check.changes[0].before.as_ref().unwrap().contains("one"));
        assert_eq!(
            hash_directory(Path::new(&f.asset.central_path)).unwrap(),
            f.asset.content_hash
        );
        assert!(f
            .center
            .apply_update(&f.asset.id, &check.id, false)
            .is_err());
        let next = f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        assert_ne!(next.central_path, f.asset.central_path);
        assert_eq!(
            hash_directory(Path::new(&claude.target_path)).unwrap(),
            f.asset.content_hash
        );
        assert_eq!(
            hash_directory(Path::new(&codex.target_path)).unwrap(),
            f.asset.content_hash
        );
        let snapshot = f.center.snapshot().unwrap();
        assert!(snapshot
            .deployments
            .iter()
            .all(|d| d.status == "center_newer"));
        f.center
            .sync_update_deployments(&f.asset.id, vec![claude.id.clone()], &next.content_hash)
            .unwrap();
        assert_eq!(
            hash_directory(Path::new(&claude.target_path)).unwrap(),
            next.content_hash
        );
        assert_eq!(
            hash_directory(Path::new(&codex.target_path)).unwrap(),
            f.asset.content_hash
        );
        let versions = f.center.versions(&f.asset.id).unwrap();
        assert_eq!(versions.len(), 1);
        let rollback = f
            .center
            .prepare_rollback(&f.asset.id, &versions[0].id)
            .unwrap();
        let previous = f
            .center
            .apply_update(&f.asset.id, &rollback.id, true)
            .unwrap();
        assert_eq!(previous.content_hash, f.asset.content_hash);
        // Rollback is central-only too.
        assert_eq!(
            hash_directory(Path::new(&claude.target_path)).unwrap(),
            next.content_hash
        );
        f.center
            .sync_update_deployments(&f.asset.id, vec![claude.id.clone()], &previous.content_hash)
            .unwrap();
        assert_eq!(
            hash_directory(Path::new(&claude.target_path)).unwrap(),
            previous.content_hash
        );
        f.center.undeploy_batch(vec![claude.id, codex.id]).unwrap();
    }

    #[test]
    fn removed_runtime_deployment_cannot_be_synced_or_rewritten() {
        let f = Fixture::new();
        let deployed = f.deploy("claude_code");
        let target = Path::new(&deployed.target_path);
        let original_hash = hash_directory(target).unwrap();
        f.center.conn.lock().unwrap().execute(
            "UPDATE skill_deployments SET runtime='oh_my_pi' WHERE id=?1", [&deployed.id],
        ).unwrap();
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        let next = f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        let error = f.center.sync_update_deployments(&f.asset.id, vec![deployed.id.clone()], &next.content_hash).unwrap_err();
        assert!(error.to_string().contains("已移除"));
        assert_eq!(hash_directory(target).unwrap(), original_hash);
        assert_eq!(f.center.list_deployments().unwrap().iter().find(|item| item.id == deployed.id).unwrap().runtime, "oh_my_pi");
    }

    #[test]
    fn runtime_drift_blocks_entire_selected_batch() {
        let f = Fixture::new();
        let a = f.deploy("claude_code");
        let b = f.deploy("codex");
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        let next = f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        // Force a managed copy for one target so modifying it does not alter the other old link.
        if b.mode != "copy" {
            remove_owned_link(Path::new(&b.target_path), Path::new(&f.asset.central_path)).unwrap();
            copy_directory(
                Path::new(&f.asset.central_path),
                Path::new(&b.target_path),
                false,
            )
            .unwrap();
            fs::write(
                Path::new(&b.target_path).join(COPY_MARKER),
                serde_json::to_vec(
                    &serde_json::json!({"skillId":f.asset.id,"managedBy":"crewvela"}),
                )
                .unwrap(),
            )
            .unwrap();
            f.center
                .conn
                .lock()
                .unwrap()
                .execute(
                    "UPDATE skill_deployments SET mode='copy' WHERE id=?1",
                    [&b.id],
                )
                .unwrap();
        }
        fs::write(Path::new(&b.target_path).join("user.txt"), "keep me").unwrap();
        assert!(f
            .center
            .sync_update_deployments(&f.asset.id, vec![a.id, b.id], &next.content_hash)
            .is_err());
        assert_eq!(
            hash_directory(Path::new(&a.target_path)).unwrap(),
            f.asset.content_hash
        );
        assert_eq!(
            fs::read_to_string(Path::new(&b.target_path).join("user.txt")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn stale_check_and_tampered_candidate_are_rejected() {
        let f = Fixture::new();
        f.change();
        let first = f.center.check_update(&f.asset.id, None).unwrap();
        let second = f.center.check_update(&f.asset.id, None).unwrap();
        assert!(f.center.apply_update(&f.asset.id, &first.id, true).is_err());
        fs::write(
            Path::new(&second.candidate.unwrap().central_path).join("injected.sh"),
            "echo injected",
        )
        .unwrap();
        assert!(f
            .center
            .apply_update(&f.asset.id, &second.id, true)
            .is_err());
    }

    #[test]
    fn center_drift_survives_snapshot_refresh_and_blocks_update() {
        let f = Fixture::new();
        fs::write(
            Path::new(&f.asset.central_path).join("mine.txt"),
            "local modification",
        )
        .unwrap();
        f.center.snapshot().unwrap();
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        assert_eq!(check.status, "error");
        assert!(check.message.contains("本地修改"));
    }

    #[test]
    fn checks_and_history_survive_reopening() {
        let f = Fixture::new();
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        let reopened = SkillCenter::open_at(f.root.join("center")).unwrap();
        assert_eq!(reopened.update_checks().unwrap()[0].status, "applied");
        assert_eq!(reopened.versions(&f.asset.id).unwrap().len(), 1);
    }

    #[test]
    fn archive_without_replacement_is_explicitly_manual() {
        let f = Fixture::new();
        f.center
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE skill_assets SET source_type='archive' WHERE id=?1",
                [&f.asset.id],
            )
            .unwrap();
        assert_eq!(
            f.center.check_update(&f.asset.id, None).unwrap().status,
            "manual"
        );
    }

    #[test]
    fn replacement_archive_updates_same_asset_and_preserves_old_directory() {
        use std::io::Write;
        let f = Fixture::new();
        f.center
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE skill_assets SET source_type='archive' WHERE id=?1",
                [&f.asset.id],
            )
            .unwrap();
        let path = f.root.join("new.skill");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("bundle/SKILL.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"---\nname: version-test\n---\narchive version two")
            .unwrap();
        zip.finish().unwrap();
        let check = f
            .center
            .check_update(&f.asset.id, Some(path.display().to_string()))
            .unwrap();
        assert_eq!(check.status, "available");
        let next = f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        assert_eq!(next.id, f.asset.id);
        assert_eq!(next.source_ref, Some(path.display().to_string()));
        assert!(Path::new(&f.asset.central_path).exists());
    }

    #[test]
    fn old_version_links_can_be_undeployed_after_central_update() {
        let f = Fixture::new();
        let a = f.deploy("claude_code");
        let b = f.deploy("codex");
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        f.center.undeploy_batch(vec![a.id, b.id]).unwrap();
        assert!(!path_exists_no_follow(Path::new(&a.target_path)));
        assert!(!path_exists_no_follow(Path::new(&b.target_path)));
        assert!(Path::new(&f.asset.central_path).exists());
    }

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = background_command("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().into()
    }

    #[test]
    fn git_subdirectory_tracks_content_not_unrelated_commits_and_pin_is_preserved() {
        let f = Fixture::new();
        let repo = f.root.join("repo");
        fs::create_dir_all(repo.join("skills/nested")).unwrap();
        fs::write(
            repo.join("skills/nested/SKILL.md"),
            "---\nname: git-version-test\n---\none",
        )
        .unwrap();
        git(&repo, &["init"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "user.name", "test"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "one"]);
        let revision = git(&repo, &["rev-parse", "HEAD"]);
        let input = ImportSkillInput {
            source_type: "git".into(),
            source: repo.display().to_string(),
            branch: None,
            subpath: None,
            revision: None,
        };
        let asset = f.center.import(input.clone()).unwrap().remove(0);
        assert_eq!(asset.source_subpath.as_deref(), Some("skills/nested"));
        fs::write(repo.join("README.md"), "unrelated").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "unrelated"]);
        assert_eq!(
            f.center.check_update(&asset.id, None).unwrap().status,
            "up_to_date"
        );
        fs::write(
            repo.join("skills/nested/SKILL.md"),
            "---\nname: git-version-test\n---\ntwo",
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "two"]);
        let check = f.center.check_update(&asset.id, None).unwrap();
        assert_eq!(check.status, "available");
        assert_eq!(
            check.candidate.unwrap().source_revision,
            Some(git(&repo, &["rev-parse", "HEAD"]))
        );
        let pinned_center = SkillCenter::open_at(f.root.join("pinned-center")).unwrap();
        let pinned = pinned_center
            .import(ImportSkillInput {
                revision: Some(revision),
                ..input
            })
            .unwrap()
            .remove(0);
        assert_eq!(
            pinned_center.check_update(&pinned.id, None).unwrap().status,
            "pinned"
        );
    }

    #[test]
    fn shared_target_requires_every_reference_selected() {
        let f = Fixture::new();
        let a = f.deploy("claude_code");
        let mut duplicate = a.clone();
        duplicate.id = Uuid::new_v4().to_string();
        duplicate.runtime = "codex".into();
        f.center.conn.lock().unwrap().execute("INSERT INTO skill_deployments SELECT ?1,?2,skill_id,?3,scope,project_path,task_id,agent_id,target_path,mode,status,source_hash,target_hash,last_error,updated_at FROM skill_deployments WHERE id=?4",
            params![duplicate.id, "shared-test-identity", duplicate.runtime,a.id]).unwrap();
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        let next = f.center.apply_update(&f.asset.id, &check.id, true).unwrap();
        assert!(f
            .center
            .sync_update_deployments(&f.asset.id, vec![a.id.clone()], &next.content_hash)
            .is_err());
        assert_eq!(
            hash_directory(Path::new(&a.target_path)).unwrap(),
            f.asset.content_hash
        );
        f.center
            .sync_update_deployments(&f.asset.id, vec![a.id, duplicate.id], &next.content_hash)
            .unwrap();
        assert_eq!(
            hash_directory(Path::new(&a.target_path)).unwrap(),
            next.content_hash
        );
    }

    #[test]
    fn superseded_candidates_are_cleaned_but_history_is_kept() {
        let f = Fixture::new();
        f.change();
        let check = f.center.check_update(&f.asset.id, None).unwrap();
        let obsolete = check.candidate.unwrap().central_path;
        let next = f.center.check_update(&f.asset.id, None).unwrap();
        assert!(!Path::new(&obsolete).exists());
        f.center.apply_update(&f.asset.id, &next.id, true).unwrap();
        f.center.check_update(&f.asset.id, None).unwrap();
        assert!(Path::new(&f.asset.central_path).exists());
    }

    #[test]
    fn partial_swap_failure_restores_original_target() {
        let f = Fixture::new();
        let d = f.deploy("claude_code");
        let mut swap = VersionSwap::prepare(
            &f.asset,
            Path::new(&d.target_path),
            f.asset.clone(),
            d.clone(),
        )
        .unwrap();
        fs::remove_dir_all(&swap.staged).unwrap();
        assert!(swap.activate().is_err());
        drop(swap);
        assert_eq!(
            hash_directory(Path::new(&d.target_path)).unwrap(),
            f.asset.content_hash
        );
    }
}
