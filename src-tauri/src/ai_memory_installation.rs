//! Verified installation selected from an official release. Data stays independent.
use super::*;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE: usize = 128 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Package {
    pub platform: &'static str,
    pub asset: &'static str,
    pub extensions: &'static [&'static str],
    #[serde(skip)]
    executable: &'static str,
}

impl Package {
    pub(super) fn executable_name(self) -> &'static str {
        self.executable
    }
}

// Platform asset names are a contract, versions and hashes come from GitHub.
fn package_for(os: &str, arch: &str) -> Option<Package> {
    let (platform, asset, executable, extensions): (_, _, _, &'static [&'static str]) =
        match (os, arch) {
            ("windows", "x86_64") => (
                "Windows x64",
                "ai-memory-windows-x86_64.zip",
                "ai-memory.exe",
                &["zip"],
            ),
            ("macos", "aarch64") => (
                "macOS Apple Silicon",
                "ai-memory-macos-aarch64.tar.gz",
                "ai-memory",
                &["gz"],
            ),
            ("macos", "x86_64") => (
                "macOS Intel",
                "ai-memory-macos-x86_64.tar.gz",
                "ai-memory",
                &["gz"],
            ),
            _ => return None,
        };
    Some(Package {
        platform,
        asset,
        executable,
        extensions,
    })
}

pub(super) fn current_package() -> Option<Package> {
    package_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn verify_archive(bytes: &[u8], release: &releases::Release) -> Result<()> {
    if bytes.len() as u64 != release.size
        || format!("{:x}", Sha256::digest(bytes)) != release.sha256
    {
        bail!(
            "安装包 SHA256 或大小校验失败；仅接受官方 {} 完整包（{}），已有安装未修改",
            release.tag,
            release.asset
        );
    }
    Ok(())
}

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

fn capabilities(executable: &Path) -> Result<()> {
    for (verb, flag) in [
        ("hook", "--check-capture"),
        ("install-hooks", "--capture-mode"),
        ("install-mcp", "--config-file"),
        ("serve", "--enable-web"),
        ("backup", "--to"),
    ] {
        let mut command = Command::new(executable);
        command.args([verb, "--help"]).creation_flags_no_window();
        let output = run_setup_command(command, Duration::from_secs(10))?;
        if !output.success || !output.stdout.contains(flag) {
            bail!("新版 ai-memory 缺少 {verb} {flag} 能力，未替换现有安装");
        }
    }
    Ok(())
}

pub(super) async fn install(target: &Path, archive: Option<&Path>) -> Result<PathBuf> {
    let release = releases::resolve(None).await?;
    install_release(target, archive, &release).await
}

pub(super) async fn install_release(
    target: &Path,
    archive: Option<&Path>,
    release: &releases::Release,
) -> Result<PathBuf> {
    install_release_with_progress(target, archive, release, &|_| Ok(())).await
}

pub(super) async fn install_release_with_progress(
    target: &Path,
    archive: Option<&Path>,
    release: &releases::Release,
    progress: &(impl Fn(&str) -> Result<()> + Sync),
) -> Result<PathBuf> {
    releases::compatible(&release.version)?;
    let package = current_package().context(
        "快速安装支持 Windows x64 和 macOS（Apple Silicon / Intel）；当前平台请使用高级手动接入",
    )?;
    let installed = target.join(package.executable_name());
    if installed.is_file() {
        let found = version(&installed)?;
        if found != release.version {
            bail!(
                "已有 ai-memory {found}，已保留；请通过检查更新显式升级到 {}",
                release.version
            );
        }
        return Ok(installed);
    }
    let bytes = if let Some(archive) = archive {
        if !archive.is_file() || std::fs::metadata(archive)?.len() > MAX_ARCHIVE as u64 {
            bail!(
                "请选择不超过 128MB 的官方 {} {} 安装包（{}）",
                release.tag,
                package.platform,
                package.asset
            );
        }
        std::fs::read(archive).context("无法读取本地安装包")?
    } else {
        download_archive(release, progress).await?
    };
    progress("安装包下载完成，正在校验官方 SHA256 与完整大小")?;
    verify_archive(&bytes, release)?;
    let target = target.to_path_buf();
    let release = release.clone();
    tokio::task::spawn_blocking(move || publish(&target, &bytes, package, &release.version))
        .await
        .context("安装任务中断")?
}

#[derive(Clone, Copy)]
struct DownloadPolicy {
    total: Duration,
    connect: Duration,
    idle: Duration,
    retry_delay: Duration,
    attempts: usize,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            total: Duration::from_secs(15 * 60),
            connect: Duration::from_secs(20),
            idle: Duration::from_secs(45),
            retry_delay: Duration::from_secs(1),
            attempts: 3,
        }
    }
}

enum DownloadFailure {
    Retry(anyhow::Error),
    Stop(anyhow::Error),
}

async fn download_archive(
    release: &releases::Release,
    progress: &(impl Fn(&str) -> Result<()> + Sync),
) -> Result<Vec<u8>> {
    download_with_policy(release, DownloadPolicy::default(), progress).await
}

async fn download_with_policy(
    release: &releases::Release,
    policy: DownloadPolicy,
    progress: &(impl Fn(&str) -> Result<()> + Sync),
) -> Result<Vec<u8>> {
    if release.size == 0 || release.size > MAX_ARCHIVE as u64 {
        bail!("官方安装包大小超出允许范围，未开始下载");
    }
    let client = reqwest::Client::builder()
        .connect_timeout(policy.connect)
        .read_timeout(policy.idle)
        // Binary archive bytes must match the official digest exactly.
        .no_gzip().no_brotli().no_zstd().no_deflate()
        .build()?;
    let deadline = tokio::time::Instant::now() + policy.total;
    let mut last_error = anyhow::anyhow!("下载总时限已到");
    for attempt in 1..=policy.attempts {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        progress(&format!("{} · 第 {attempt}/{} 次下载 · 0%（旧服务与数据尚未改变）", release.asset, policy.attempts))?;
        match download_once(&client, release, remaining, progress).await {
            Ok(bytes) => return Ok(bytes),
            Err(DownloadFailure::Stop(error)) => return Err(error),
            Err(DownloadFailure::Retry(error)) => last_error = error,
        }
        if attempt < policy.attempts {
            let delay = policy.retry_delay.saturating_mul(attempt as u32);
            if deadline.saturating_duration_since(tokio::time::Instant::now()) <= delay { break; }
            progress(&format!("网络中断或停滞，正在重试（{attempt}/{}）；将重新获取官方地址并从头下载", policy.attempts))?;
            tokio::time::sleep(delay).await;
        }
    }
    Err(last_error).context("官方安装包下载未完成：已达到重试次数或总时限；旧服务与数据未改变。可稍后重试，或在上方“下载较慢？使用本地官方安装包”中导入对应版本完整包")
}

async fn download_once(
    client: &reqwest::Client,
    release: &releases::Release,
    remaining: Duration,
    progress: &(impl Fn(&str) -> Result<()> + Sync),
) -> std::result::Result<Vec<u8>, DownloadFailure> {
    // Renew the expiring signed asset redirect on every attempt. Never expose its URL.
    let mut response = client.get(&release.url)
        .query(&[("crewvela_download", uuid::Uuid::new_v4().to_string())])
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::USER_AGENT, "CrewVela shared-memory setup")
        .timeout(remaining).send().await
        .map_err(|e| DownloadFailure::Retry(e.without_url().into()))?;
    let status = response.status();
    if !status.is_success() {
        let error = anyhow::anyhow!("官方安装包下载返回 HTTP {status}");
        return Err(if status.is_server_error() || matches!(status.as_u16(), 403 | 408 | 429) {
            DownloadFailure::Retry(error)
        } else { DownloadFailure::Stop(error) });
    }
    let mut bytes = Vec::new();
    let mut last_progress = tokio::time::Instant::now();
    while let Some(chunk) = response.chunk().await
        .map_err(|e| DownloadFailure::Retry(e.without_url().into()))? {
        if bytes.len() + chunk.len() > MAX_ARCHIVE || (bytes.len() + chunk.len()) as u64 > release.size {
            return Err(DownloadFailure::Stop(anyhow::anyhow!("安装包超过官方声明大小，未安装")));
        }
        bytes.extend_from_slice(&chunk);
        if last_progress.elapsed() >= Duration::from_secs(1) || bytes.len() as u64 == release.size {
            progress(&format!("已下载 {:.1} / {:.1} MiB（{}%）", bytes.len() as f64 / 1048576.0,
                release.size as f64 / 1048576.0, bytes.len() as u64 * 100 / release.size))
                .map_err(DownloadFailure::Stop)?;
            last_progress = tokio::time::Instant::now();
        }
    }
    if bytes.len() as u64 != release.size {
        return Err(DownloadFailure::Retry(anyhow::anyhow!("安装包未完整传输")));
    }
    // A digest mismatch is not a transient network error; never weaken or retry it.
    verify_archive(&bytes, release).map_err(DownloadFailure::Stop)?;
    Ok(bytes)
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
        if total > MAX_EXTRACTED {
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

fn extract_tar(bytes: &[u8], stage: &Path) -> Result<()> {
    use std::io::Read;
    // Cap decompression itself, including headers and padding, not just file sizes.
    let decoder = flate2::read::GzDecoder::new(bytes).take(MAX_EXTRACTED + 1);
    let mut archive = tar::Archive::new(decoder);
    let mut total = 0u64;
    for (count, entry) in archive
        .entries()
        .context("安装包不是有效 TAR.GZ")?
        .enumerate()
    {
        if count >= 5000 {
            bail!("安装包条目过多");
        }
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        // macOS releases use ./ prefixes and a ./ root directory.
        let relative: PathBuf = path
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect();
        if !path.is_relative()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || path.as_os_str().to_string_lossy().contains(['\\', ':'])
        {
            bail!("安装包含目录越界路径");
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            bail!("安装包不允许链接或特殊文件");
        }
        if relative.as_os_str().is_empty() {
            if kind.is_dir() {
                continue;
            }
            bail!("安装包路径无效");
        }
        total = total.checked_add(entry.size()).context("安装包大小溢出")?;
        if total > MAX_EXTRACTED {
            bail!("安装包解压内容过大");
        }
        let output = stage.join(relative);
        if kind.is_dir() {
            std::fs::create_dir_all(output)?;
            continue;
        }
        std::fs::create_dir_all(output.parent().context("安装包路径缺少父目录")?)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)?;
        let copied = std::io::copy(&mut entry, &mut file)?;
        if copied != entry.size() {
            bail!("安装包内容不完整");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if entry.header().mode()? & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        file.sync_all()?;
    }
    // Read through the gzip trailer so truncation/CRC failures remain errors.
    let mut decoder = archive.into_inner();
    std::io::copy(&mut decoder, &mut std::io::sink())?;
    if decoder.limit() == 0 {
        bail!("安装包解压内容过大");
    }
    Ok(())
}

fn publish(
    target: &Path,
    bytes: &[u8],
    package: Package,
    expected_version: &str,
) -> Result<PathBuf> {
    let parent = target.parent().context("安装目录缺少父目录")?;
    std::fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".ai-memory-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage)?;
    let prepare = (|| -> Result<()> {
        if package.asset.ends_with(".zip") {
            extract(bytes, &stage)?;
        } else {
            extract_tar(bytes, &stage)?;
        }
        let executable = stage.join(package.executable_name());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        }
        if version(&executable)? != expected_version {
            bail!("发行包版本与所选发行版不符");
        }
        capabilities(&executable)?;
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
    Ok(target.join(package.executable_name()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn interrupted_download_retries_with_fresh_redirect_and_reports_progress() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut requests = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !stopped.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else { std::thread::sleep(Duration::from_millis(2)); continue; };
                stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut request = Vec::new();
                let mut byte = [0u8];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                    if stream.read(&mut byte).unwrap_or(0) == 0 { break; }
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n").unwrap();
                // The first connection drops in the middle of the body.
                stream.write_all(if requests.len() == 1 { b"abc" } else { b"abcdefgh" }).unwrap();
            }
            requests
        });
        let package = current_package().unwrap();
        let mut release = legacy_release(package);
        release.url = format!("http://{address}/fixture.zip");
        release.size = 8;
        release.sha256 = format!("{:x}", Sha256::digest(b"abcdefgh"));
        let messages = std::sync::Mutex::new(Vec::new());
        let result = download_archive(&release, &|message| { messages.lock().unwrap().push(message.to_owned()); Ok(()) }).await;
        stop.store(true, Ordering::Relaxed);
        let requests = server.join().unwrap();
        assert_eq!(result.unwrap(), b"abcdefgh");
        assert_eq!(requests.len(), 2);
        assert_ne!(requests[0].lines().next(), requests[1].lines().next(), "retry must renew the expiring GitHub redirect");
        let messages = messages.lock().unwrap();
        assert!(messages.iter().any(|m| m.contains("重试")));
        assert!(messages.iter().any(|m| m.contains("100%")));
    }

    fn download_server(
        handler: impl Fn(usize, std::net::TcpStream) + Send + Sync + 'static,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<Vec<String>>) {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture.zip", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handler = Arc::new(handler);
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            let mut workers = Vec::new();
            while !stopped.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else { std::thread::sleep(Duration::from_millis(2)); continue; };
                stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut request = Vec::new();
                let mut byte = [0u8];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                    if stream.read(&mut byte).unwrap_or(0) == 0 { break; }
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let handler = handler.clone();
                let attempt = requests.len();
                workers.push(std::thread::spawn(move || handler(attempt, stream)));
            }
            for worker in workers { worker.join().unwrap(); }
            requests
        });
        (url, stop, thread)
    }

    fn download_fixture(url: String) -> releases::Release {
        let mut release = legacy_release(current_package().unwrap());
        release.url = url;
        release.size = 8;
        release.sha256 = format!("{:x}", Sha256::digest(b"abcdefgh"));
        release
    }

    #[tokio::test]
    async fn slow_continuous_download_resets_idle_timeout_and_still_checks_digest() {
        use std::io::Write;
        let (url, stop, server) = download_server(|_, mut stream| {
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n").unwrap();
            for byte in b"abcdefgh" {
                std::thread::sleep(Duration::from_millis(70));
                stream.write_all(&[*byte]).unwrap();
            }
        });
        let release = download_fixture(url);
        let policy = DownloadPolicy { total: Duration::from_secs(3), connect: Duration::from_secs(1), idle: Duration::from_millis(300), retry_delay: Duration::ZERO, attempts: 1 };
        let result = download_with_policy(&release, policy, &|_| Ok(())).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(server.join().unwrap().len(), 1);
        let bytes = result.unwrap();
        verify_archive(&bytes, &release).unwrap();
        assert_eq!(bytes, b"abcdefgh");
    }

    #[tokio::test]
    async fn stalled_download_has_bounded_retries_and_actionable_error() {
        use std::io::Write;
        let (url, stop, server) = download_server(|_, mut stream| {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n");
            std::thread::sleep(Duration::from_millis(250));
        });
        let policy = DownloadPolicy { total: Duration::from_secs(2), connect: Duration::from_secs(1), idle: Duration::from_millis(50), retry_delay: Duration::from_millis(5), attempts: 3 };
        let result = download_with_policy(&download_fixture(url), policy, &|_| Ok(())).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(server.join().unwrap().len(), 3);
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("本地官方安装包"));
        assert!(error.contains("旧服务与数据未改变"));
        assert!(!error.contains("http://"), "transport errors must not expose signed asset URLs");
    }

    #[tokio::test]
    async fn invalid_download_digest_and_oversized_body_never_retry_or_publish() {
        use std::io::Write;
        for oversized in [false, true] {
            let (url, stop, server) = download_server(move |_, mut stream| {
                let body: &[u8] = if oversized { b"abcdefghi" } else { b"xxxxxxxx" };
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(body);
            });
            let release = download_fixture(url);
            let result = download_archive(&release, &|_| Ok(())).await;
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(server.join().unwrap().len(), 1);
            let error = result.unwrap_err().to_string();
            assert!(error.contains(if oversized { "超过官方声明大小" } else { "SHA256" }), "{error}");
        }
    }
    const VERSION: &str = "2.6.0"; // Historical package fixtures, never an install default.
    fn legacy_release(package: Package) -> releases::Release {
        let (sha, size) = match package.asset {
            "ai-memory-macos-aarch64.tar.gz" => (
                "3c23c80b07f7cba0045f13e50e92c98a53fb5f741b50560a67ce3b9739e1bff9",
                16200873,
            ),
            "ai-memory-macos-x86_64.tar.gz" => (
                "7523b656739ac75989e7a15ac05523fc15112bc915f83cff96e3da90e7c73b5e",
                17579460,
            ),
            _ => (
                "9517b818f0ea561c7a97e700f9c5f260f729ada7d026ba04f864ff11c6a2961f",
                19978755,
            ),
        };
        releases::Release {
            version: VERSION.into(),
            tag: format!("v{VERSION}"),
            asset: package.asset.into(),
            sha256: sha.into(),
            size,
            url: String::new(),
            id: 1,
        }
    }
    #[test]
    fn supported_platforms_have_distinct_pinned_archives() {
        let windows = package_for("windows", "x86_64").unwrap();
        let arm = package_for("macos", "aarch64").unwrap();
        let intel = package_for("macos", "x86_64").unwrap();
        assert_eq!(windows.executable, "ai-memory.exe");
        assert_eq!(arm.executable, "ai-memory");
        assert_eq!(intel.executable, "ai-memory");
        assert_ne!(arm.asset, intel.asset);
        assert!(
            verify_archive(b"wrong platform or modified package", &legacy_release(arm)).is_err()
        );
        for (os, arch) in [
            ("linux", "aarch64"),
            ("windows", "aarch64"),
            ("macos", "arm"),
            ("macos", "unknown"),
        ] {
            assert!(package_for(os, arch).is_none());
        }
    }

    fn tar_archive(name: &str, kind: u8, mode: u32, content: &[u8]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(mode);
        header.set_entry_type(tar::EntryType::new(kind));
        // Raw path injection deliberately permits malicious names for regression tests.
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_cksum();
        builder.append(&header, content).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn tar_rejects_escape_links_special_files_duplicates_and_truncation() {
        for (name, kind) in [
            ("../escape", b'0'),
            ("/absolute", b'0'),
            ("a/../../escape", b'0'),
            ("C:\\escape", b'0'),
            ("link", b'2'),
            ("hardlink", b'1'),
            ("fifo", b'6'),
        ] {
            let root = root();
            assert!(
                extract_tar(&tar_archive(name, kind, 0o777, b""), &root).is_err(),
                "{name} {kind}"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
        let root = root();
        extract_tar(&tar_archive("./", b'5', 0o755, b""), &root).unwrap();
        let bytes = tar_archive("./hooks/test.sh", b'0', 0o4755, b"fixture");
        extract_tar(&bytes, &root).unwrap();
        assert_eq!(
            std::fs::read(root.join("hooks/test.sh")).unwrap(),
            b"fixture"
        );
        assert!(
            extract_tar(&bytes, &root).is_err(),
            "never overwrite a duplicate or existing entry"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.join("hooks/test.sh"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o755
            );
        }
        let empty = root.join("truncated");
        std::fs::create_dir(&empty).unwrap();
        assert!(extract_tar(&bytes[..bytes.len() - 8], &empty).is_err());
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut encoder = encoder;
        let mut header = tar::Header::new_gnu();
        header.set_path("oversized").unwrap();
        header.set_size(MAX_EXTRACTED + 1);
        header.set_mode(0o644);
        header.set_cksum();
        encoder.write_all(header.as_bytes()).unwrap();
        assert!(extract_tar(&encoder.finish().unwrap(), &empty).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_publication_restores_execute_permission_and_preserves_data_on_failure() {
        let root = root();
        let target = root.join("安装 目录");
        let data = root.join("ai-memory-data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(data.join("keep"), "memory").unwrap();
        let package = package_for("macos", "aarch64").unwrap();
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let script = b"#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'ai-memory 2.6.0\\n'; else printf '%s\\n' --check-capture --capture-mode --config-file --enable-web --to; fi\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o644); // Publication must make the binary executable.
        header.set_cksum();
        builder
            .append_data(&mut header, "ai-memory", &script[..])
            .unwrap();
        let mut directory = tar::Header::new_gnu();
        directory.set_size(0);
        directory.set_mode(0o755);
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_cksum();
        builder
            .append_data(&mut directory, "hooks", std::io::empty())
            .unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();
        let executable = publish(&target, &bytes, package, VERSION).unwrap();
        assert_eq!(version(&executable).unwrap(), VERSION);
        let before = std::fs::read(&executable).unwrap();
        assert!(publish(
            &target,
            &tar_archive("ai-memory", b'0', 0o755, b"not executable"),
            package,
            VERSION
        )
        .is_err());
        assert_eq!(std::fs::read(executable).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(data.join("keep")).unwrap(),
            "memory"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires both official Mac archives in CREWVELA_AI_MEMORY_TEST_ARCHIVES; never executes a Mac binary on Windows"]
    fn official_mac_archives_verify_and_extract() {
        let archives = PathBuf::from(
            std::env::var_os("CREWVELA_AI_MEMORY_TEST_ARCHIVES").expect("set archive directory"),
        );
        for arch in ["aarch64", "x86_64"] {
            let package = package_for("macos", arch).unwrap();
            let bytes = std::fs::read(archives.join(package.asset)).unwrap();
            let release = |package: Package| {
                if let Some(path) = std::env::var_os("CREWVELA_AI_MEMORY_TEST_MAC_RELEASE") {
                    let metadata: Value =
                        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                    releases::parse(&metadata, package.asset).unwrap()
                } else {
                    legacy_release(package)
                }
            };
            verify_archive(&bytes, &release(package)).unwrap();
            let other = package_for(
                "macos",
                if arch == "aarch64" {
                    "x86_64"
                } else {
                    "aarch64"
                },
            )
            .unwrap();
            assert!(verify_archive(&bytes, &release(other)).is_err());
            let stage = root();
            extract_tar(&bytes, &stage).unwrap();
            assert!(stage.join(package.executable).is_file());
            assert!(stage.join("hooks/claude-code").is_dir());
            assert!(stage.join("hooks/codex").is_dir());
            std::fs::remove_dir_all(stage).unwrap();
        }
    }
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
        if current_package().is_none() {
            return;
        }
        let root = root();
        let target = root.join("ai-memory");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), "original").unwrap();
        let file = root.join("bad.zip");
        std::fs::write(&file, archive("ai-memory.exe", None)).unwrap();
        assert!(install_release(
            &target,
            Some(&file),
            &legacy_release(current_package().unwrap())
        )
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
    #[ignore = "downloads official release for the current Windows/Mac architecture; explicit network acceptance only"]
    async fn official_online_installation() {
        let root = root();
        let result = install(&root.join("ai-memory"), None).await;
        if let Ok(path) = &result {
            releases::compatible(&version(path).unwrap()).unwrap();
        }
        if result.is_ok() {
            std::fs::remove_dir_all(root).unwrap();
        }
        result.unwrap();
    }
}
