//! Pinned, verified installation. Runtime data is never part of an install swap.
use super::*;
use sha2::{Digest, Sha256};

pub(super) const VERSION: &str = "2.6.0";
const SHA256: &str = "9517b818f0ea561c7a97e700f9c5f260f729ada7d026ba04f864ff11c6a2961f";
const URL: &str = "https://github.com/akitaonrails/ai-memory/releases/download/v2.6.0/ai-memory-windows-x86_64.zip";
const MAX_ARCHIVE: usize = 32 * 1024 * 1024;

pub(super) fn version(executable: &Path) -> Result<String> {
    let mut command = Command::new(executable);
    command.arg("--version").creation_flags_no_window();
    let output = run_setup_command(command, Duration::from_secs(10))?;
    if !output.success {
        bail!("无法读取已有 ai-memory 版本，保留原安装");
    }
    let version = output
        .stdout
        .trim()
        .strip_prefix("ai-memory ")
        .context("无法识别 ai-memory 版本")?
        .to_owned();
    Ok(version)
}

pub(super) async fn install(target: &Path, archive: Option<&Path>) -> Result<PathBuf> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        bail!("快速安装暂支持 Windows x64；其他平台请使用高级手动接入");
    }
    let installed = target.join("ai-memory.exe");
    if installed.is_file() {
        let found = version(&installed)?;
        if found != VERSION {
            bail!("已有 ai-memory {found}，已保留；快速安装仅验证了 {VERSION}，不会自动升级或降级");
        }
        return Ok(installed);
    }
    let bytes = if let Some(archive) = archive {
        if !archive.is_file() || std::fs::metadata(archive)?.len() > MAX_ARCHIVE as u64 {
            bail!("请选择不超过 32MB 的官方 v{VERSION} Windows x64 ZIP 包");
        }
        std::fs::read(archive).context("无法读取本地安装包")?
    } else {
        let mut response = reqwest::Client::new()
            .get(URL)
            // GitHub redirects to an expiring asset URL. Request a fresh
            // redirect instead of reusing a stale intermediary cache entry.
            .query(&[("crewvela_download", uuid::Uuid::new_v4().to_string())])
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .header(reqwest::header::USER_AGENT, "CrewVela shared-memory setup")
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .context("下载失败，可重试或选择本地官方安装包")?
            .error_for_status()
            .context("官方安装包下载返回错误")?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.context("下载中断，请重试")? {
            if bytes.len() + chunk.len() > MAX_ARCHIVE {
                bail!("安装包超过允许大小，未安装");
            }
            bytes.extend_from_slice(&chunk);
        }
        bytes
    };
    if format!("{:x}", Sha256::digest(&bytes)) != SHA256 {
        bail!("安装包 SHA256 校验失败；仅接受官方 v{VERSION} Windows x64 完整包，已有安装未修改");
    }
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || publish(&target, &bytes))
        .await
        .context("安装任务中断")?
}

fn extract(bytes: &[u8], stage: &Path) -> Result<()> {
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("安装包不是有效 ZIP")?;
    if zip.len() > 5000 {
        bail!("安装包条目过多");
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let relative = entry.enclosed_name().context("安装包含目录越界路径")?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            bail!("安装包路径无效");
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("安装包不允许符号链接");
        }
        total = total.checked_add(entry.size()).context("安装包大小溢出")?;
        if total > 160 * 1024 * 1024 {
            bail!("安装包解压内容过大");
        }
        let output = stage.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(output)?;
            continue;
        }
        std::fs::create_dir_all(output.parent().context("安装包路径缺少父目录")?)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
    }
    Ok(())
}

fn publish(target: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let parent = target.parent().context("安装目录缺少父目录")?;
    std::fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".ai-memory-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage)?;
    let prepare = (|| -> Result<()> {
        extract(bytes, &stage)?;
        if version(&stage.join("ai-memory.exe"))? != VERSION {
            bail!("发行包版本与固定版本不符");
        }
        if !stage.join("hooks").is_dir() {
            bail!("发行包缺少 hooks 模板");
        }
        Ok(())
    })();
    if let Err(error) = prepare {
        // Keep the isolated failed stage for diagnosis; never touch the live install.
        return Err(error).context("安装包准备失败，已有安装保持不变");
    }
    let backup = parent.join(format!("ai-memory-install-backup-{}", uuid::Uuid::new_v4()));
    let existing = target.exists();
    if existing {
        std::fs::rename(target, &backup).context("无法备份已有安装目录")?;
    }
    if let Err(error) = std::fs::rename(&stage, target) {
        if existing {
            std::fs::rename(&backup, target).with_context(|| {
                format!(
                    "发布失败且恢复失败：{error}；备份保留于 {}",
                    backup.display()
                )
            })?;
        }
        return Err(error).context("发布安装失败，原目录已恢复");
    }
    Ok(target.join("ai-memory.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("crewvela-memory-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        p
    }
    fn archive(name: &str, mode: Option<u32>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        if let Some(mode) = mode {
            zip.add_symlink(
                name,
                "outside",
                zip::write::SimpleFileOptions::default().unix_permissions(mode),
            )
            .unwrap();
        } else {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn extraction_rejects_paths_and_links_and_keeps_files_within_stage() {
        for (name, mode) in [
            ("../escape", None),
            ("/absolute", None),
            ("link", Some(0o777)),
        ] {
            let root = root();
            assert!(extract(&archive(name, mode), &root).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
        let root = root();
        extract(&archive("hooks/test.txt", None), &root).unwrap();
        assert_eq!(
            std::fs::read(root.join("hooks/test.txt")).unwrap(),
            b"fixture"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn invalid_local_archive_does_not_publish_or_replace_data() {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return;
        }
        let root = root();
        let target = root.join("ai-memory");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), "original").unwrap();
        let file = root.join("bad.zip");
        std::fs::write(&file, archive("ai-memory.exe", None)).unwrap();
        assert!(install(&target, Some(&file))
            .await
            .unwrap_err()
            .to_string()
            .contains("SHA256"));
        assert_eq!(
            std::fs::read_to_string(target.join("keep")).unwrap(),
            "original"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    #[ignore = "downloads official release; explicit network acceptance only"]
    async fn official_online_installation() {
        let root = root();
        let result = install(&root.join("ai-memory"), None).await;
        if let Ok(path) = &result {
            assert_eq!(version(path).unwrap(), VERSION);
        }
        if result.is_ok() {
            std::fs::remove_dir_all(root).unwrap();
        }
        result.unwrap();
    }
}
