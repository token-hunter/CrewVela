use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    version: u8,
    roots: Vec<PathBuf>,
}

fn read_policy(path: &Path) -> Result<Policy> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Policy {
                version: 1,
                roots: Vec::new(),
            })
        }
        Err(error) => return Err(error.into()),
    };
    let policy: Policy = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if policy.version != 1 || policy.roots.iter().any(|p| !p.is_absolute()) {
        bail!("项目授权文件格式无效");
    }
    Ok(policy)
}

// Short-lived writer lock also serializes concurrent CrewVela instances.
struct Writer(PathBuf);
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn access(file: &Path, selected: &Path, grant: Option<bool>) -> Result<bool> {
    if !selected.is_absolute() || !selected.is_dir() {
        bail!("请选择存在的绝对项目目录");
    }
    let root = selected.canonicalize()?;
    let same = |path: &PathBuf| path == &root || path.canonicalize().ok().as_ref() == Some(&root);
    let Some(grant) = grant else {
        return Ok(read_policy(file)?.roots.iter().any(same));
    };
    if grant {
        let marker = root.join(".ai-memory.toml");
        if !fs::symlink_metadata(&marker)
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            bail!("项目根目录需要自己的 .ai-memory.toml 文件");
        }
        let value: toml::Value = toml::from_str(&fs::read_to_string(marker)?)?;
        if !["workspace", "project"].iter().all(|key| {
            value
                .get(*key)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty())
        }) {
            bail!("项目标记需要明确的 workspace 和 project");
        }
    }
    let folder = file.parent().context("缺少授权目录")?;
    fs::create_dir_all(folder)?;
    let lock = file.with_extension("lock");
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)
        .with_context(|| {
            format!(
                "无法锁定项目授权；若此前异常退出，请核对 {}",
                lock.display()
            )
        })?;
    let _writer = Writer(lock);
    let mut policy = read_policy(file)?;
    let was_granted = policy.roots.iter().any(same);
    if grant == was_granted {
        return Ok(grant);
    }
    if grant {
        // Pin the physical directory so repointing a junction cannot transfer consent.
        #[cfg(windows)]
        let root = {
            let path = root.to_str().context("项目目录不是有效的 Unicode 路径")?;
            if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
                PathBuf::from(format!(r"\\{unc}"))
            } else {
                PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(path))
            }
        };
        policy.roots.push(root);
    } else {
        policy.roots.retain(|p| !same(p));
    }
    let temp = folder.join(format!(".project-roots-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        output.write_all(&serde_json::to_vec_pretty(&policy)?)?;
        output.sync_all()?;
        drop(output);
        fs::rename(&temp, file)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result?;
    Ok(grant)
}

#[tauri::command]
pub fn ai_memory_project_access(project_dir: String, grant: Option<bool>) -> Result<bool, String> {
    let file = crate::providers::config_dir()
        .map_err(|e| e.to_string())?
        .join("ai-memory/project-roots.json");
    access(&file, Path::new(&project_dir), grant).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_only_marked_projects_and_preserves_invalid_policy() {
        let root = std::env::temp_dir().join(format!("memory-policy-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let file = root.join("policy.json");
        assert!(!access(&file, &root, None).unwrap());
        assert!(access(&file, &root, Some(true)).is_err());
        fs::write(root.join(".ai-memory.toml"), "workspace='w'\nproject='p'").unwrap();
        assert!(access(&file, &root, Some(true)).unwrap());
        assert!(access(&file, &root, None).unwrap());
        assert!(!access(&file, &root, Some(false)).unwrap());
        fs::write(&file, b"invalid").unwrap();
        assert!(access(&file, &root, Some(true)).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"invalid");
        fs::remove_dir_all(root).unwrap();
    }
}
