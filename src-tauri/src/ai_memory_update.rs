//! Explicit managed upgrades. Keep cold data/config snapshots, never kill by PID.
use super::onboarding::{Environment, Request};
use super::*;
use sha2::{Digest, Sha256};

const JOURNAL: &str = "ai-memory-upgrade-pending.json";

#[derive(Serialize, Deserialize)]
struct ConfigCopy {
    path: PathBuf,
    existed: bool,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    backup: PathBuf,
    old_version: String,
    was_running: bool,
    configs: Vec<ConfigCopy>,
}

pub(super) fn pending(root: &Path) -> bool {
    root.join(JOURNAL).exists()
}

// OS file lock is released on process exit, unlike a stale PID/lock-file marker.
// Share it with managed setup/start/stop so another CrewVela process cannot
// restart the service while a cold snapshot or migration is in progress.
pub(super) fn operation_lock(root: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(root)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(root.join(".ai-memory-operation.lock"))?;
    // std File::try_lock needs Rust 1.89; preserve this project's Rust 1.85 API floor.
    #[cfg(unix)]
    let locked = {
        use std::os::fd::AsRawFd;
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
    };
    #[cfg(windows)]
    let locked = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        let mut overlapped = unsafe { std::mem::zeroed() };
        unsafe {
            LockFileEx(
                file.as_raw_handle().cast(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            ) != 0
        }
    };
    #[cfg(not(any(unix, windows)))]
    let locked = false;
    if !locked {
        bail!(
            "无法取得记忆服务操作锁（可能有其他实例正在操作）：{}",
            std::io::Error::last_os_error()
        );
    }
    Ok(file)
}
fn digest(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(format!("{:x}", Sha256::digest(bytes)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// Copy only physical files/directories. Backups include auth and raw ledgers too,
// which the upstream wiki/DB backup alone intentionally excludes.
fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            bail!("备份源包含链接或目录联接，未继续更新");
        }
    }
    if meta.file_type().is_symlink() {
        bail!("备份源包含符号链接，未继续更新");
    }
    if meta.is_dir() {
        std::fs::create_dir(target)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700))?;
        }
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        std::fs::copy(source, target)?;
    } else {
        bail!("备份源包含特殊文件，未继续更新");
    }
    Ok(())
}

fn save(root: &Path, journal: &Journal) -> Result<()> {
    onboarding::atomic_write(&root.join(JOURNAL), &serde_json::to_vec(journal)?)
}

async fn restore(root: &Path, journal: &Journal) -> Result<()> {
    let data = root.join("ai-memory-data");
    let managed = root.join("ai-memory");
    let executable = managed.join(
        installation::current_package()
            .context("当前平台不支持托管安装")?
            .executable_name(),
    );
    let backup_parent = root.join("ai-memory-upgrade-backups").canonicalize()?;
    let backup = journal.backup.canonicalize()?;
    if backup.parent() != Some(backup_parent.as_path()) {
        bail!("更新恢复记录的备份位置无效");
    }
    if managed_service()?.is_running()? {
        managed_service()?.stop()?;
    }
    if let Ok(base) = data_config_base(&data) {
        if !managed_listener_is_unbound(&base)? {
            bail!("记忆服务仍由外部进程运行，请先手动停止后恢复；未终止外部进程");
        }
    }
    let old_install = backup.join("install");
    if old_install.exists() {
        if managed.exists() {
            std::fs::rename(
                &managed,
                backup.join(format!("failed-install-{}", uuid::Uuid::new_v4())),
            )?;
        }
        std::fs::rename(old_install, &managed)?;
    } else if installation::version(&executable)? != journal.old_version {
        bail!("未找到可验证的旧安装，保留所有快照，请手动恢复");
    }
    // Never discard data written by the candidate, even when validation failed.
    if data.exists() {
        std::fs::rename(
            &data,
            backup.join(format!("failed-data-{}", uuid::Uuid::new_v4())),
        )?;
    }
    copy_tree(&backup.join("data"), &data)?;
    for (i, entry) in journal.configs.iter().enumerate() {
        let current = digest(&entry.path)?;
        if current == entry.before {
            continue;
        }
        if current != entry.after {
            bail!(
                "Agent 配置已被外部程序修改，未覆盖；备份保留于 {}",
                backup.display()
            );
        }
        if entry.existed {
            onboarding::atomic_write(
                &entry.path,
                &std::fs::read(backup.join(format!("agent-{i}")))?,
            )?;
        } else if entry.path.exists() {
            std::fs::remove_file(&entry.path)?;
        }
    }
    if journal.was_running {
        start_service_at(&executable, &data, &data_config_base(&data)?).await?;
    }
    std::fs::remove_file(root.join(JOURNAL))?;
    Ok(())
}

fn data_config_base(data: &Path) -> Result<String> {
    managed_url_from_config(&std::fs::read_to_string(data.join("config.toml"))?)
}

pub(super) async fn recover_update(confirmed: bool) -> Result<(), String> {
    onboarding::require_idle()?;
    if !confirmed {
        return Err("恢复旧快照前需确认；未合并升级后的新数据，新数据会保留在备份目录".into());
    }
    let root = crate::providers::config_dir().map_err(|e| e.to_string())?;
    let _operation = operation_lock(&root).map_err(|e| e.to_string())?;
    let _guard = service_lifecycle_lock(Duration::from_secs(20))
        .await
        .map_err(|e| e.to_string())?;
    let journal: Journal =
        serde_json::from_slice(&std::fs::read(root.join(JOURNAL)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    restore(&root, &journal).await.map_err(|e| format!("{e:#}"))
}

pub(super) async fn execute(
    env: &Environment,
    request: &Request,
    release: &releases::Release,
    update: &(impl Fn(&str, &str, &str) -> Result<()> + Sync),
) -> Result<()> {
    let _operation = operation_lock(&env.root)?;
    if pending(&env.root) {
        bail!("上次更新未完成，请先恢复升级前快照");
    }
    let managed = env.root.join("ai-memory");
    let executable = managed.join(
        installation::current_package()
            .context("当前平台不支持更新")?
            .executable_name(),
    );
    let existing = env
        .existing
        .as_ref()
        .context("尚未安装记忆服务，请先安装")?;
    if existing.canonicalize()? != executable.canonicalize()? {
        bail!("只更新 CrewVela 托管安装；Homebrew、Scoop、PATH 或自定义安装请使用原安装方式更新");
    }
    let old_version = installation::version(&executable)?;
    releases::compatible(&old_version)?;
    let old = releases::numbers(&old_version)?;
    let new = releases::numbers(&release.version)?;
    if new <= old {
        bail!("目标版本不高于当前版本，未降级或重复升级");
    }
    if new.0 != old.0 && !request.confirm_major {
        bail!("跨大版本更新需确认数据迁移与备份，不能仅替换旧程序回退");
    }
    update(
        "install",
        "running",
        &format!("下载并校验官方 {}，现有服务暂不改变", release.tag),
    )?;
    let candidate = env
        .root
        .join(format!(".ai-memory-candidate-{}", uuid::Uuid::new_v4()));
    installation::install_release(
        &candidate,
        request.local_archive.as_deref().map(Path::new),
        release,
    )
    .await?;
    update(
        "install",
        "done",
        "新版本和所需命令已通过检查，尚未替换运行版本",
    )?;
    let _guard = service_lifecycle_lock(Duration::from_secs(20)).await?;
    let data = env.data();
    let base = data_config_base(&data)?;
    let was_running = managed_service()?.is_running()?;
    if !was_running && !managed_listener_is_unbound(&base)? {
        bail!("当前服务由外部进程或上次应用持有，请先手动停止后更新；未终止该进程");
    }
    update(
        "backup",
        "running",
        "暂停记忆服务并备份完整数据与所选 Agent 配置；请保持依赖会话暂停",
    )?;
    if was_running {
        managed_service()?.stop()?;
    }
    let backup = env
        .root
        .join("ai-memory-upgrade-backups")
        .join(uuid::Uuid::new_v4().to_string());
    let snapshot = (|| -> Result<Journal> {
        std::fs::create_dir_all(&backup)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o700))?;
        }
        copy_tree(&data, &backup.join("data"))?;
        let project = Path::new(&request.project_dir);
        let mut configs = Vec::new();
        for agent in &request.agents {
            let paths = env.config_paths(agent, project);
            for path in [paths.0, paths.1] {
                let before = digest(&path)?;
                if before.is_some() {
                    copy_tree(&path, &backup.join(format!("agent-{}", configs.len())))?;
                }
                configs.push(ConfigCopy {
                    path,
                    existed: before.is_some(),
                    after: before.clone(),
                    before,
                });
            }
        }
        let journal = Journal {
            backup: backup.clone(),
            old_version,
            was_running,
            configs,
        };
        save(&env.root, &journal)?;
        Ok(journal)
    })();
    let mut journal = match snapshot {
        Ok(journal) => journal,
        Err(error) => {
            if was_running {
                start_service_at(&executable, &data, &base)
                    .await
                    .context("备份失败且旧服务重启失败")?;
            }
            return Err(error).context("备份未完成，未替换原安装或数据");
        }
    };
    update(
        "backup",
        "done",
        &format!("升级前快照保留于 {}", backup.display()),
    )?;
    let outcome = async {
        std::fs::rename(&managed, backup.join("install"))?;
        std::fs::rename(&candidate, &managed)?;
        update("service", "running", "启动新版并迁移已备份的记忆数据库")?;
        start_service_at(&executable, &data, &base).await?;
        if !managed_service()?.is_running()? {
            bail!("新版服务未由本次更新持有，未继续配置外部服务");
        }
        let status = managed_status_for_data(&service_client(&base)?, &base, &data).await?;
        if status["version"] != release.version {
            bail!("运行中的服务版本与目标版本不一致");
        }
        update("service", "done", "新版服务健康、认证已接入")?;
        for (index, agent) in request.agents.iter().enumerate() {
            update(agent, "running", "备份后更新 MCP 与钩子，保留其他服务")?;
            onboarding::setup_agent(
                env,
                &executable,
                Path::new(&request.project_dir),
                &base,
                agent,
            )?;
            for i in index * 2..index * 2 + 2 {
                journal.configs[i].after = digest(&journal.configs[i].path)?;
            }
            save(&env.root, &journal)?;
            update(agent, "done", "新版接入配置已完成；现有会话需重新加载")?;
        }
        update(
            "project",
            "done",
            "沿用原项目标记和 allowlist，不导入其他项目历史",
        )?;
        update("check", "running", "验证记忆工具和项目作用域")?;
        onboarding::verify_tools(&base, &data).await?;
        if inspect_scope_at(&executable, &data, &request.project_dir, &base).is_none() {
            bail!("升级后项目作用域检查未通过");
        }
        update(
            "check",
            "done",
            "更新已完成，请加载新会话验证真实捕获；旧快照已保留",
        )?;
        std::fs::remove_file(env.root.join(JOURNAL))?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = outcome {
        match restore(&env.root,&journal).await {
            Ok(())=>bail!("更新未完成，已恢复旧安装和升级前快照。候选数据未丢弃，保留于 {}。原因：{error:#}",backup.display()),
            Err(recovery)=>bail!("更新失败且恢复未完成；请先恢复升级前快照。备份：{}；更新原因：{error:#}；恢复原因：{recovery:#}",backup.display()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn interrupted_upgrade_restores_snapshot_but_never_overwrites_external_config() {
        for external in [false, true] {
            let root =
                std::env::temp_dir().join(format!("memory-recovery-{}", uuid::Uuid::new_v4()));
            let backup = root.join("ai-memory-upgrade-backups/fixture");
            let data = root.join("ai-memory-data");
            for dir in [
                &data,
                &root.join("ai-memory"),
                &backup.join("install"),
                &backup.join("data"),
            ] {
                std::fs::create_dir_all(dir).unwrap();
            }
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let config = format!("bind='127.0.0.1:{port}'");
            std::fs::write(data.join("config.toml"), &config).unwrap();
            std::fs::write(backup.join("data/config.toml"), &config).unwrap();
            std::fs::write(data.join("candidate"), "keep failed writes").unwrap();
            std::fs::write(backup.join("data/old-memory"), "original").unwrap();
            std::fs::write(backup.join("install/old-program"), "original").unwrap();
            let path = root.join("agent-config");
            std::fs::write(backup.join("agent-0"), "before").unwrap();
            std::fs::write(&path, "before").unwrap();
            let before = digest(&path).unwrap();
            std::fs::write(&path, "after").unwrap();
            let after = digest(&path).unwrap();
            if external {
                std::fs::write(&path, "external").unwrap();
            }
            let journal = Journal {
                backup: backup.clone(),
                old_version: "2.6.0".into(),
                was_running: false,
                configs: vec![ConfigCopy {
                    path: path.clone(),
                    existed: true,
                    before,
                    after,
                }],
            };
            save(&root, &journal).unwrap();
            let result = restore(&root, &journal).await;
            assert_eq!(result.is_err(), external);
            assert_eq!(pending(&root), external);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                if external { "external" } else { "before" }
            );
            assert_eq!(
                std::fs::read_to_string(data.join("old-memory")).unwrap(),
                "original"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("ai-memory/old-program")).unwrap(),
                "original"
            );
            assert!(std::fs::read_dir(&backup).unwrap().any(|e| e
                .unwrap()
                .path()
                .join("candidate")
                .is_file()));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn operation_lock_refuses_overlapping_instances_and_releases_on_drop() {
        let root = std::env::temp_dir().join(format!("memory-lock-{}", uuid::Uuid::new_v4()));
        let lock = operation_lock(&root).unwrap();
        assert!(operation_lock(&root).is_err());
        drop(lock);
        drop(operation_lock(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snapshots_preserve_files_and_refuse_links() {
        let root = std::env::temp_dir().join(format!("memory-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("data/db")).unwrap();
        std::fs::write(root.join("data/db/keep"), "memory").unwrap();
        copy_tree(&root.join("data"), &root.join("backup")).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("backup/db/keep")).unwrap(),
            "memory"
        );
        assert!(copy_tree(&root.join("data"), &root.join("backup")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("data"), root.join("link")).unwrap();
            assert!(copy_tree(&root.join("link"), &root.join("bad")).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
