//! Update only recognized bundled communication guides; never overwrite custom skills.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::Write, path::Path};

const BUNDLED: &str = include_str!("../../skills/crewvela/SKILL.md");
// Exact previously shipped guides, including the shared guide installed before rebranding.
const PREVIOUS: &[&str] = &[
    "95a418c70d0e7ef9a2d7fead6809cbef6297ae5a82b63dfe31bc5c45098f8384",
    "e417793d4858b5e15f9b2353f977d719667c1ab721d6738eca9ca8a30d1b61e4",
    "cec02c1e531dbc8449a42babe6aef2d344e4d7cacb560ebded0411adfd7ff2be",
    "6aa6a621aaaa802ce8380bc4cdc3790418ea1eb225ae7e36646aaad3123e3edf",
    "346ba587667be5b65dd53af4cf869fc912070bbc506784ecbd800a0bef1eb633",
    "c0dc6d6011d52046a34c148e917b42466e50cb940ca0dbce055d43bdea6061e9",
];

#[derive(Debug, PartialEq)]
enum Status { Current, Updated, Installed, PreservedCustom }

fn fingerprint(bytes: &[u8]) -> String {
    let normalized = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

pub(crate) fn sync_installed() -> Result<()> {
    let home = dirs::home_dir().context("无法确定通信 Skill 安装目录")?;
    sync_installed_at(&home)
}

fn sync_installed_at(home: &Path) -> Result<()> {
    let mut seen = HashSet::new();
    for directory in [".claude", ".agents", ".codex"] {
        let parent = home.join(directory).join("skills/crewvela");
        fs::create_dir_all(&parent)?;
        // Shared .agents/skills or skill directories may be junctions. Update their
        // actual target once, without replacing the user's discovery roots.
        let parent = parent.canonicalize()?;
        if !seen.insert(parent.clone()) { continue; }
        let path = parent.join("SKILL.md");
        if sync_file(&path, PREVIOUS)? == Status::PreservedCustom {
            eprintln!("[crewvela] 保留自定义通信 Skill，请手动同步新版指南：{}", path.display());
        }
        let previous = home.join(directory).join("skills").join(crate::brand::LEGACY_NAME).join("SKILL.md");
        if previous.exists() {
            let alias = BUNDLED.replacen("name: crewvela", &format!("name: {}", crate::brand::LEGACY_NAME), 1);
            match sync_content(&previous, PREVIOUS, &alias) {
                Ok(Status::PreservedCustom) => eprintln!("[CrewVela] 保留自定义旧通信 Skill；新版 crewvela 已单独安装"),
                Err(error) => eprintln!("[CrewVela] 旧 Skill 兼容入口未更新，原文件保留，新版独立安装不受影响：{error:#}"),
                _ => {}
            }
        }
    }
    Ok(())
}

fn sync_file(path: &Path, previous: &[&str]) -> Result<Status> {
    sync_content(path, previous, BUNDLED)
}

fn sync_content(path: &Path, previous: &[&str], content: &str) -> Result<Status> {
    if path.with_file_name(".crewvela-skill.json").exists() {
        // Skill-center deployments have their own hashes and update transactions.
        return Ok(Status::PreservedCustom);
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Ok(Status::PreservedCustom);
    }
    let old = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("读取已有通信 Skill 失败"),
    };
    if let Some(old) = &old {
        let hash = fingerprint(old);
        if hash == fingerprint(content.as_bytes()) { return Ok(Status::Current); }
        if !previous.contains(&hash.as_str()) { return Ok(Status::PreservedCustom); }
        if fs::metadata(path)?.permissions().readonly() { bail!("通信 Skill 为只读文件，保留原文：{}", path.display()); }
        let backup = path.with_file_name(format!("SKILL.md.crewvela-backup-{hash}"));
        match fs::OpenOptions::new().create_new(true).write(true).open(&backup) {
            Ok(mut file) => { file.write_all(old)?; file.sync_all()?; }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&backup)? != *old { bail!("通信 Skill 备份内容不一致，保留原文"); }
            }
            Err(error) => return Err(error).context("备份通信 Skill 失败"),
        }
    }
    let temp = path.with_file_name(format!(".crewvela-skill-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&temp)?;
        if old.is_some() { file.set_permissions(fs::metadata(path)?.permissions())?; }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        let latest = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("复核通信 Skill 失败"),
        };
        if latest != old { bail!("通信 Skill 同步期间被修改，保留当前文件"); }
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result.context("发布通信 Skill 失败，已有指南及备份保留")?;
    Ok(if old.is_some() { Status::Updated } else { Status::Installed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_migration_installs_new_skill_preserves_custom_legacy_and_updates_known_aliases() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-brand-{}", uuid::Uuid::new_v4()));
        let old = root.join(".claude/skills").join(crate::brand::LEGACY_NAME).join("SKILL.md");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, "custom guide must remain").unwrap();
        sync_installed_at(&root).unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "custom guide must remain");
        assert_eq!(fs::read_to_string(root.join(".claude/skills/crewvela/SKILL.md")).unwrap(), BUNDLED);
        sync_installed_at(&root).unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "custom guide must remain");
        let known = BUNDLED.replacen("name: crewvela", &format!("name: {}", crate::brand::LEGACY_NAME), 1);
        fs::write(&old, &known).unwrap();
        sync_installed_at(&root).unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), known);
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognized_guide_upgrades_once_with_exact_backup_and_preserves_custom_edits() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-sync-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("SKILL.md");
        let old = b"previous fixture\r\n";
        fs::write(&path, old).unwrap();
        let hash = fingerprint(old);
        assert_eq!(sync_file(&path, &[&hash]).unwrap(), Status::Updated);
        assert_eq!(fs::read(root.join(format!("SKILL.md.crewvela-backup-{hash}"))).unwrap(), old);
        assert_eq!(fs::read_to_string(&path).unwrap(), BUNDLED);
        assert_eq!(sync_file(&path, &[&hash]).unwrap(), Status::Current);
        fs::write(&path, "user customized guide").unwrap();
        assert_eq!(sync_file(&path, &[&hash]).unwrap(), Status::PreservedCustom);
        assert_eq!(fs::read_to_string(&path).unwrap(), "user customized guide");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_guide_installs_and_conflicting_backup_never_overwrites_existing_data() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-sync-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("SKILL.md");
        assert_eq!(sync_file(&path, &[]).unwrap(), Status::Installed);
        assert_eq!(fs::read_to_string(&path).unwrap(), BUNDLED);
        let old = b"previous fixture"; let hash = fingerprint(old);
        fs::write(&path, old).unwrap();
        fs::write(root.join(format!("SKILL.md.crewvela-backup-{hash}")), "different backup").unwrap();
        assert!(sync_file(&path, &[&hash]).is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skill_center_managed_guides_keep_their_content_and_ownership_record() {
        let root = std::env::temp_dir().join(format!("crewvela-skill-sync-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("SKILL.md"); let old = b"previous fixture";
        fs::write(&path, old).unwrap();
        fs::write(root.join(".crewvela-skill.json"), "managed by skill center").unwrap();
        assert_eq!(sync_file(&path, &[&fingerprint(old)]).unwrap(), Status::PreservedCustom);
        assert_eq!(fs::read(&path).unwrap(), old);
        fs::remove_dir_all(root).unwrap();
    }
}
