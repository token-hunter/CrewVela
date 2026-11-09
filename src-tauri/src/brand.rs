//! New identities and deliberately retained, read-compatible legacy aliases.
use anyhow::{bail, Context, Result};
use std::{fs, path::{Path, PathBuf}, sync::Mutex};

pub(crate) const CONFIG_NAME: &str = "CrewVela";
pub(crate) const APP_ID: &str = "com.crewvela.app";
pub(crate) const LEGACY_NAME: &str = "cc-launcher";
pub(crate) const LEGACY_APP_ID: &str = "com.cclauncher.app";
pub(crate) const LEGACY_APP_SESSION: &str = "_cc_launcher_app_session_id";
pub(crate) const LEGACY_QUEUED_AT: &str = "_cc_launcher_queued_at";
pub(crate) const APP_SESSION: &str = "_crewvela_app_session_id";
pub(crate) const QUEUED_AT: &str = "_crewvela_queued_at";
pub(crate) fn is_launcher_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    stem == "crewvela" || stem == LEGACY_NAME
}
static DIRECTORY_GATE: Mutex<()> = Mutex::new(());

/// Share existing files, live SQLite/WAL and mailbox locks rather than copying
/// a moving snapshot. The old physical root is retained only for compatibility.
pub(crate) fn directory_at(base: &Path, previous: &str, current: &str) -> Result<PathBuf> {
    let _gate = DIRECTORY_GATE.lock().map_err(|_| anyhow::anyhow!("数据迁移锁损坏"))?;
    fs::create_dir_all(base)?;
    let old = base.join(previous);
    let new = base.join(current);
    let exists = |path: &Path| -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).context("读取迁移目录失败，未创建空配置"),
        }
    };
    let old_exists = exists(&old)?;
    if exists(&new)? {
        if !new.is_dir() { bail!("CrewVela 数据路径不是可用目录：{}", new.display()); }
        if old_exists && old.canonicalize()? != new.canonicalize()? {
            bail!("新旧数据目录同时存在且指向不同数据，未合并或覆盖。请先备份并核对：{}；{}", old.display(), new.display());
        }
        return Ok(new);
    }
    if !old_exists { fs::create_dir_all(&new)?; return Ok(new); }
    if !old.is_dir() { bail!("旧数据目录不可用，未创建空配置：{}", old.display()); }
    let target = old.canonicalize()?;
    #[cfg(windows)]
    let created = junction::create(&target, &new);
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(&target, &new);
    if let Err(error) = created {
        // A second process can publish the same alias first.
        if new.canonicalize().ok().as_ref() != Some(&target) {
            return Err(error).context("建立 CrewVela 数据兼容入口失败，旧数据未改动");
        }
    }
    if new.canonicalize()? != target { bail!("数据迁移入口复核失败，旧数据未改动"); }
    Ok(new)
}

pub(crate) fn config_dir() -> Result<PathBuf> {
    if let Some(profile)=crate::test_profile::current() { return Ok(profile.config_dir.clone()); }
    directory_at(&dirs::config_dir().context("无法确定配置目录")?, LEGACY_NAME, CONFIG_NAME)
}

/// Must run before Tauri creates WebView profiles for the new application ID.
pub(crate) fn prepare_app_storage_aliases() -> Result<()> {
    if crate::test_profile::active() { return Ok(()); }
    // Rebuildable caches belong to each application ID independently. Do not
    // alias, merge or reject them when both old and new cache directories exist.
    // Keep every data root even if a platform shares it with its cache root.
    prepare_app_storage_aliases_at(
        [dirs::config_dir(), dirs::data_dir(), dirs::data_local_dir()].into_iter().flatten(),
    )
}

fn prepare_app_storage_aliases_at(data_roots: impl IntoIterator<Item = PathBuf>) -> Result<()> {
    let mut bases = std::collections::HashSet::new();
    for base in data_roots {
        if bases.insert(base.clone()) && base.join(LEGACY_APP_ID).exists() {
            directory_at(&base, LEGACY_APP_ID, APP_ID)?;
        }
    }
    Ok(())
}

/// New fields win only when consistent. Never accept contradictory identities.
pub(crate) fn normalize_hook(event: &serde_json::Value) -> Result<serde_json::Value> {
    let mut normalized = event.clone();
    if let Some(object) = normalized.as_object_mut() {
        for (current, old) in [(APP_SESSION, LEGACY_APP_SESSION), (QUEUED_AT, LEGACY_QUEUED_AT)] {
            match (object.get(current), object.get(old)) {
                (Some(a), Some(b)) if a != b => bail!("新旧 hook 字段冲突，事件已保留"),
                (None, Some(value)) => { object.insert(current.into(), value.clone()); }
                _ => {}
            }
        }
    }
    Ok(normalized)
}

pub(crate) fn compatible_hook(event: &serde_json::Value) -> Result<serde_json::Value> {
    let mut event = normalize_hook(event)?;
    if let Some(object) = event.as_object_mut() {
        for (current, old) in [(APP_SESSION, LEGACY_APP_SESSION), (QUEUED_AT, LEGACY_QUEUED_AT)] {
            if let Some(value) = object.get(current).cloned() { object.insert(old.into(), value); }
        }
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_storage_preparation_keeps_independent_caches_and_migrates_only_data_roots() {
        let root = std::env::temp_dir().join(format!("crewvela-app-cache-{}", uuid::Uuid::new_v4()));
        let cache = root.join("cache");
        let data = root.join("data");
        for name in [LEGACY_APP_ID, APP_ID] {
            fs::create_dir_all(cache.join(name)).unwrap();
            fs::write(cache.join(name).join("synthetic-cache"), name).unwrap();
        }
        fs::create_dir_all(data.join(LEGACY_APP_ID)).unwrap();
        fs::write(data.join(LEGACY_APP_ID).join("synthetic-data"), "keep data").unwrap();

        // Cache is deliberately absent from the data/config candidate roots.
        // Repeated data/local-data roots must still resolve to the same alias.
        prepare_app_storage_aliases_at([root.join("config"), data.clone(), data.clone()]).unwrap();
        assert_eq!(data.join(APP_ID).canonicalize().unwrap(), data.join(LEGACY_APP_ID).canonicalize().unwrap());
        assert_eq!(fs::read_to_string(data.join(APP_ID).join("synthetic-data")).unwrap(), "keep data");
        for name in [LEGACY_APP_ID, APP_ID] {
            assert!(!fs::symlink_metadata(cache.join(name)).unwrap().file_type().is_symlink());
            assert_eq!(fs::read_to_string(cache.join(name).join("synthetic-cache")).unwrap(), name);
            assert_eq!(fs::read_dir(cache.join(name)).unwrap().count(), 1);
        }
        assert_ne!(cache.join(LEGACY_APP_ID).canonicalize().unwrap(), cache.join(APP_ID).canonicalize().unwrap());
        #[cfg(windows)] junction::delete(data.join(APP_ID)).unwrap();
        #[cfg(unix)] fs::remove_file(data.join(APP_ID)).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_storage_preparation_still_rejects_conflicting_data_even_when_shared_with_cache() {
        let root = std::env::temp_dir().join(format!("crewvela-app-data-conflict-{}", uuid::Uuid::new_v4()));
        // Windows may use the same physical root for local data and caches.
        // Being a cache location must not exempt this data root from protection.
        for name in [LEGACY_APP_ID, APP_ID] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(root.join(name).join("synthetic-data"), name).unwrap();
        }
        let error = prepare_app_storage_aliases_at([root.clone(), root.clone()]).unwrap_err();
        assert!(error.to_string().contains("新旧数据目录同时存在"));
        for name in [LEGACY_APP_ID, APP_ID] {
            assert_eq!(fs::read_to_string(root.join(name).join("synthetic-data")).unwrap(), name);
            assert_eq!(fs::read_dir(root.join(name)).unwrap().count(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn brand_migration_shares_live_data_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!("crewvela-brand-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join(LEGACY_NAME)).unwrap();
        fs::write(root.join(LEGACY_NAME).join("mailbox.json"), "original").unwrap();
        let current = directory_at(&root, LEGACY_NAME, CONFIG_NAME).unwrap();
        assert_eq!(fs::read_to_string(current.join("mailbox.json")).unwrap(), "original");
        fs::write(current.join("mailbox.json"), "updated").unwrap();
        assert_eq!(fs::read_to_string(root.join(LEGACY_NAME).join("mailbox.json")).unwrap(), "updated");
        assert_eq!(directory_at(&root, LEGACY_NAME, CONFIG_NAME).unwrap(), current);
        let guard = crate::app_instance::AppInstanceGuard::acquire(&current).unwrap().unwrap();
        assert!(crate::app_instance::AppInstanceGuard::acquire(&root.join(LEGACY_NAME)).unwrap().is_none());
        drop(guard);
        #[cfg(windows)] junction::delete(&current).unwrap();
        #[cfg(unix)] fs::remove_file(&current).unwrap();
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn brand_migration_never_merges_conflicting_roots_or_follows_broken_aliases() {
        let root = std::env::temp_dir().join(format!("crewvela-brand-conflict-{}", uuid::Uuid::new_v4()));
        for name in [LEGACY_NAME, CONFIG_NAME] { fs::create_dir_all(root.join(name)).unwrap(); fs::write(root.join(name).join("keep"), name).unwrap(); }
        assert!(directory_at(&root, LEGACY_NAME, CONFIG_NAME).is_err());
        assert_eq!(fs::read_to_string(root.join(LEGACY_NAME).join("keep")).unwrap(), LEGACY_NAME);
        assert_eq!(fs::read_to_string(root.join(CONFIG_NAME).join("keep")).unwrap(), CONFIG_NAME);
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn brand_migration_keeps_old_hook_identity_and_order_and_rejects_conflicts() {
        let old = serde_json::json!({LEGACY_APP_SESSION: "original", LEGACY_QUEUED_AT: 17});
        let new = compatible_hook(&old).unwrap();
        assert_eq!(new[APP_SESSION], "original"); assert_eq!(new[QUEUED_AT], 17);
        assert_eq!(new[LEGACY_APP_SESSION], "original");
        assert_eq!(compatible_hook(&new).unwrap(), new);
        assert!(normalize_hook(&serde_json::json!({APP_SESSION: "one", LEGACY_APP_SESSION: "another"})).is_err());
    }
}
