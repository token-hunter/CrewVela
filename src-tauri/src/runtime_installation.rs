//! Verified, per-user CLI installations. Never replace a running user's CLI
//! directory or write its account/model/MCP settings.
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::{collections::HashMap, fs, io::{Read, Write}, path::{Component, Path, PathBuf}, process::Command, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::Duration};
use crate::multi_agent::AgentRuntimeKind;
mod additional;
mod chat;
pub(crate) use chat::ChatDependencySummary;

const CLAUDE_BASE: &str = "https://downloads.claude.ai/claude-code-releases";
const MAX_DOWNLOAD: u64 = 384 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallJob {
    pub id: String, pub kind: AgentRuntimeKind, pub phase: String,
    pub message: String, pub downloaded: u64, pub total: Option<u64>,
    pub version: Option<String>, pub executable: Option<String>, pub error: Option<String>,
}
impl InstallJob { fn running(&self) -> bool { !matches!(self.phase.as_str(), "ready" | "failed" | "cancelled") } }
struct Operation { job: InstallJob, cancel: Arc<AtomicBool>, wake: Arc<tokio::sync::Notify> }
static JOBS: OnceLock<Mutex<HashMap<String, Operation>>> = OnceLock::new();
fn jobs() -> &'static Mutex<HashMap<String, Operation>> { JOBS.get_or_init(Default::default) }
static CLI_INSTALL_LOCKS: OnceLock<HashMap<&'static str, tokio::sync::Mutex<()>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Environment {
    pub kind: AgentRuntimeKind, pub supported: bool, pub executable: Option<String>,
    pub version: Option<String>, pub latest_version: Option<String>, pub source: String,
    pub install_directory: String, pub auth: String, pub error: Option<String>,
    pub update_error: Option<String>,
    pub dependency_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<chat::ChatEnvironment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Installed { kind: AgentRuntimeKind, version: String, directory: String }
#[derive(Clone, Debug)]
struct Release { version: String, url: String, integrity: String, archive: ArchiveFormat }
#[derive(Clone, Copy, Debug)]
enum ArchiveFormat { File, TarGz, Zip }

fn supported_kind(kind: AgentRuntimeKind) -> bool { matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex | AgentRuntimeKind::OpenCode | AgentRuntimeKind::KimiCode | AgentRuntimeKind::Pi) }
fn platform() -> Result<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok(("win32-x64", "x86_64-pc-windows-msvc")),
        ("windows", "aarch64") => Ok(("win32-arm64", "aarch64-pc-windows-msvc")),
        _ => bail!("一键安装支持 Windows x64 / ARM64，请使用该 CLI 的官方安装方式"),
    }
}
fn root() -> Result<PathBuf> { Ok(crate::providers::config_dir()?.join("runtime-installations")) }
fn valid_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.len() < 10 && part.bytes().all(|c| c.is_ascii_digit()))
}
fn relative_executable(kind: AgentRuntimeKind) -> Result<PathBuf> {
    if kind == AgentRuntimeKind::ClaudeCode { return Ok("claude.exe".into()); }
    if kind == AgentRuntimeKind::KimiCode { return Ok("kimi.exe".into()); }
    if kind == AgentRuntimeKind::Pi { return Ok("pi.exe".into()); }
    if kind == AgentRuntimeKind::OpenCode { return Ok("package/bin/opencode.exe".into()); }
    if kind != AgentRuntimeKind::Codex { bail!("此 Runtime 暂不支持一键安装"); }
    Ok(PathBuf::from("package/vendor").join(platform()?.1).join("bin/codex.exe"))
}
fn installed_at(base: &Path, kind: AgentRuntimeKind) -> Result<Option<PathBuf>> {
    if !supported_kind(kind) || platform().is_err() { return Ok(None); }
    let folder = base.join(kind.as_str());
    let bytes = match fs::read(folder.join("current.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let current: Installed = serde_json::from_slice(&bytes).context("CLI 安装记录损坏")?;
    let suffix = current.directory.strip_prefix(&format!("{}-", current.version)).context("CLI 安装目录无效")?;
    if current.kind != kind || !valid_version(&current.version) || uuid::Uuid::parse_str(suffix).is_err() { bail!("CLI 安装记录无效"); }
    let versions = folder.join("versions");
    let executable = versions.join(current.directory).join(relative_executable(kind)?);
    if !executable.is_file() { bail!("CLI 安装文件缺失，请在管理 Runtime 中修复安装"); }
    if !executable.canonicalize()?.starts_with(versions.canonicalize()?) { bail!("CLI 路径越出安装目录"); }
    Ok(Some(executable))
}
pub(crate) fn managed_executable(kind: AgentRuntimeKind) -> Result<Option<PathBuf>> {
    if !supported_kind(kind) || platform().is_err() { return Ok(None); }
    installed_at(&root()?, kind)
}

fn output(executable: &Path, args: &[&str], limit: usize) -> Result<std::process::Output> {
    let mut command = crate::multi_agent::command_for_path(executable);
    command.args(args);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    crate::title_summary::run_cli_with_timeout(command, None, Duration::from_secs(10), limit, "CLI 自检")
}
fn verify(executable: &Path, kind: AgentRuntimeKind, expected: Option<&str>) -> Result<String> {
    let version = output(executable, &["--version"], 16 * 1024)?;
    if !version.status.success() { bail!("CLI 版本自检失败，原安装已保留"); }
    let text = cli_text(&version);
    let found = crate::multi_agent::version_number(&text).context("CLI 未返回有效版本号，原安装已保留")?;
    if !valid_version(&found) || expected.is_some_and(|value| found != value) { bail!("CLI 返回的版本与安装包不符，原安装已保留"); }
    if matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex)
        && !text.to_ascii_lowercase().contains(if kind == AgentRuntimeKind::Codex { "codex" } else { "claude" }) { bail!("CLI 版本身份不符，原安装已保留"); }
    let help = output(executable, &["--help"], 256 * 1024)?;
    if !help.status.success() { bail!("CLI 帮助/启动自检失败，原安装已保留"); }
    additional::verify_help(kind, &cli_text(&help))?;
    Ok(text)
}
fn cli_text(result: &std::process::Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr)).trim().to_string()
}
fn auth_status(executable: &Path, kind: AgentRuntimeKind) -> String {
    // These CLIs do not share a stable, secret-free login status contract.
    // Do not guess account readiness from installed files or a successful help command.
    if !matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex) { return "unverified".into(); }
    let args: &[&str] = if kind == AgentRuntimeKind::Codex { &["login", "status"] } else { &["auth", "status"] };
    let Ok(result) = output(executable, args, 32 * 1024) else { return "unverified".into(); };
    if kind == AgentRuntimeKind::ClaudeCode {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&result.stdout) {
            return match value["loggedIn"].as_bool() { Some(true) => "authenticated", Some(false) => "login_required", None => "unverified" }.into();
        }
    } else {
        let text = format!("{} {}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr)).to_ascii_lowercase();
        if result.status.success() && text.contains("logged in") { return "authenticated".into(); }
        if text.contains("not logged in") { return "login_required".into(); }
    }
    "unverified".into()
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent("CrewVela-runtime-installer/1")
        .connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(600)).build()?)
}
async fn text(client: &reqwest::Client, url: &str) -> Result<String> {
    let mut response = client.get(url).timeout(Duration::from_secs(25)).send().await.map_err(|_| anyhow!("无法连接官方发行服务，请检查网络后重试"))?.error_for_status().map_err(|_| anyhow!("官方发行服务返回错误，请稍后重试"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 1024 * 1024 { bail!("发行信息过大"); }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).context("发行信息编码错误")
}
async fn release(kind: AgentRuntimeKind) -> Result<Release> {
    let (platform, _) = platform()?;
    if !supported_kind(kind) { bail!("此 Runtime 暂不支持一键安装"); }
    let client = client()?;
    if kind == AgentRuntimeKind::ClaudeCode {
        let version = text(&client, &format!("{CLAUDE_BASE}/latest")).await?.trim().to_string();
        if !valid_version(&version) { bail!("官方 Claude Code 版本号无效"); }
        let manifest: serde_json::Value = serde_json::from_str(&text(&client, &format!("{CLAUDE_BASE}/{version}/manifest.json")).await?)?;
        let digest = manifest["platforms"][platform]["checksum"].as_str().context("官方发行缺少此平台校验值")?;
        if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) { bail!("官方校验值无效"); }
        return Ok(Release { version: version.clone(), url: format!("{CLAUDE_BASE}/{version}/{platform}/claude.exe"), integrity: format!("sha256-{}", digest.to_ascii_lowercase()), archive: ArchiveFormat::File });
    }
    if kind != AgentRuntimeKind::Codex { return additional::release(&client, kind, platform).await; }
    let main: serde_json::Value = serde_json::from_str(&text(&client, "https://registry.npmjs.org/@openai/codex/latest").await?)?;
    let version = main["version"].as_str().filter(|value| valid_version(value)).context("官方 Codex 版本号无效")?.to_string();
    let native_version = format!("{version}-{platform}");
    let package: serde_json::Value = serde_json::from_str(&text(&client, &format!("https://registry.npmjs.org/@openai/codex/{native_version}")).await?)?;
    parse_codex_release(&package, &version, platform)
}
fn parse_codex_release(value: &serde_json::Value, version: &str, platform: &str) -> Result<Release> {
    if !matches!(platform, "win32-x64" | "win32-arm64") || !valid_version(version) || value["name"] != "@openai/codex" || value["version"] != format!("{version}-{platform}") { bail!("Codex 官方平台包身份不符"); }
    let expected = format!("https://registry.npmjs.org/@openai/codex/-/codex-{version}-{platform}.tgz");
    if value["dist"]["tarball"] != expected { bail!("Codex 包下载地址不属于预期官方发布"); }
    let integrity = value["dist"]["integrity"].as_str().context("官方包缺少完整性校验")?;
    let digest = integrity.strip_prefix("sha512-").context("官方包缺少 SHA512 校验")?;
    if base64::engine::general_purpose::STANDARD.decode(digest)?.len() != 64 { bail!("官方完整性校验值无效"); }
    Ok(Release { version: version.into(), url: expected, integrity: integrity.into(), archive: ArchiveFormat::TarGz })
}
pub(crate) async fn environment(kind: AgentRuntimeKind) -> Result<Environment, String> {
    crate::blocking_command(move || inspect_environment(kind)).await
}

fn inspect_environment(kind: AgentRuntimeKind) -> Result<Environment, String> {
    if chat::supports(kind) { return chat::environment(kind).map_err(|error| error.to_string()); }
    if !supported_kind(kind) { return Err("此 Runtime 保留现有官方安装方式".into()); }
    let dependency = additional::dependencies(kind);
    let supported = platform().is_ok() && dependency.is_ok();
    let path = crate::multi_agent::resolve_runtime_executable_checked(kind);
    let (executable, version, auth, error, source) = match path {
        Ok(path) => {
            let owned = managed_executable(kind).ok().flatten().as_ref() == Some(&path);
            let source = if owned { "CrewVela 用户级安装" } else if path.to_string_lossy().contains("node_modules") { "本机 npm 安装" } else { "本机 CLI 安装" };
            let inspected = (verify(&path, kind, None), auth_status(&path, kind));
            (Some(path.display().to_string()), inspected.0.as_ref().ok().cloned(), inspected.1, inspected.0.err().map(|e| e.to_string()), source.into())
        }
        Err(error) => (None, None, "unverified".into(), Some(error), "未安装或安装异常".into()),
    };
    Ok(Environment { kind, supported, executable, version, auth, error, source,
        dependency_message: dependency.unwrap_or_else(|error| error.to_string()),
        install_directory: root().map_err(|e| e.to_string())?.join(kind.as_str()).display().to_string(),
        latest_version: None, update_error: None, chat: None })
}

pub(crate) async fn chat_dependencies() -> Result<HashMap<String, ChatDependencySummary>, String> {
    crate::blocking_command(|| Ok(chat::summaries())).await
}

pub(crate) async fn latest_version(kind: AgentRuntimeKind) -> Result<String, String> {
    release(kind).await.map(|release| release.version).map_err(|error| error.to_string())
}

pub(crate) fn status(kind: AgentRuntimeKind) -> Option<InstallJob> { jobs().lock().unwrap().get(kind.as_str()).map(|op| op.job.clone()) }
fn change(id: &str, phase: &str, message: &str, bytes: u64, total: Option<u64>) {
    if let Some(operation) = jobs().lock().unwrap().values_mut().find(|op| op.job.id == id) {
        operation.job.phase = phase.into(); operation.job.message = message.into();
        operation.job.downloaded = bytes; operation.job.total = total;
    }
}
pub(crate) fn cancel(id: &str) -> Result<(), String> {
    let mut state = jobs().lock().unwrap();
    let operation = state.values_mut().find(|op| op.job.id == id).ok_or("安装操作不存在")?;
    if operation.job.running() { operation.cancel.store(true, Ordering::Release); operation.wake.notify_one(); operation.job.message = "正在取消安装，原入口保持不变…".into(); }
    Ok(())
}
pub(crate) fn start(kind: AgentRuntimeKind) -> Result<InstallJob, String> {
    let base = root().map_err(|e| e.to_string())?;
    start_at(base, kind)
}
fn start_at(base: PathBuf, kind: AgentRuntimeKind) -> Result<InstallJob, String> {
    platform().map_err(|e| e.to_string())?;
    if !supported_kind(kind) && !chat::supports(kind) { return Err("此 Runtime 暂不支持一键安装".into()); }
    if supported_kind(kind) { additional::dependencies(kind).map_err(|e| e.to_string())?; }
    let mut state = jobs().lock().unwrap();
    if state.get(kind.as_str()).is_some_and(|op| op.job.running()) { return Err("该 Runtime 已有安装操作，请等待或取消后重试".into()); }
    let job = InstallJob { id: uuid::Uuid::new_v4().to_string(), kind, phase: "checking".into(), message: "正在检查平台与官方发行信息…".into(), downloaded: 0, total: None, version: None, executable: None, error: None };
    let cancel = Arc::new(AtomicBool::new(false)); let wake = Arc::new(tokio::sync::Notify::new());
    state.insert(kind.as_str().into(), Operation { job: job.clone(), cancel: cancel.clone(), wake: wake.clone() });
    let id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        let result = if chat::supports(kind) { chat::install(&base, kind, &id, &cancel, &wake).await }
            else { install_cli(&base, kind, &id, &cancel, &wake, false).await };
        let mut state = jobs().lock().unwrap();
        if let Some(operation) = state.get_mut(kind.as_str()).filter(|op| op.job.id == id) {
            match result {
                Ok((version, executable)) => {
                    operation.job.phase = "ready".into();
                    operation.job.message = if chat::supports(kind) { "Chat 启动协议验证通过" } else { "安装和启动自检通过" }.into();
                    operation.job.version = (!version.is_empty()).then_some(version);
                    operation.job.executable = Some(executable.display().to_string());
                }
                Err(error) => {
                    let cancelled = cancel.load(Ordering::Acquire);
                    operation.job.phase = if cancelled { "cancelled" } else { "failed" }.into();
                    operation.job.message = match (chat::supports(kind), cancelled) {
                        (true, true) => "安装已取消，已验证的组件可在下次继续使用",
                        (true, false) => "安装未完成，请检查缺少的组件后重试",
                        (false, true) => "安装已取消，原入口保持不变",
                        (false, false) => "安装未完成，原入口保持不变",
                    }.into();
                    operation.job.error = (!cancelled).then(|| error.to_string());
                }
            }
        }
    });
    Ok(job)
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> { if cancel.load(Ordering::Acquire) { bail!("安装已取消"); } Ok(()) }
struct Staging(PathBuf);
impl Drop for Staging { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

async fn install_cli(base: &Path, kind: AgentRuntimeKind, id: &str, cancel: &Arc<AtomicBool>, wake: &tokio::sync::Notify, only_if_missing: bool) -> Result<(String, PathBuf)> {
    use AgentRuntimeKind::*;
    let locks = CLI_INSTALL_LOCKS.get_or_init(|| [ClaudeCode, Codex, KimiCode, OpenCode, Pi].into_iter()
        .map(|kind| (kind.as_str(), tokio::sync::Mutex::new(()))).collect());
    let lock = locks.get(kind.as_str()).context("此 CLI 暂不支持安装")?;
    let _guard = tokio::select! { lock = lock.lock() => lock, _ = wake.notified() => bail!("安装已取消") };
    check_cancel(cancel)?;
    if only_if_missing {
        if let Ok(path) = crate::multi_agent::resolve_runtime_executable_checked(kind) { return Ok((String::new(), path)); }
    }
    install(base, kind, id, cancel, wake).await
}

async fn install(base: &Path, kind: AgentRuntimeKind, id: &str, cancel: &Arc<AtomicBool>, wake: &tokio::sync::Notify) -> Result<(String, PathBuf)> {
    let release = tokio::select! { result = release(kind) => result?, _ = wake.notified() => bail!("安装已取消") };
    check_cancel(cancel)?;
    if let Ok(existing) = crate::multi_agent::resolve_runtime_executable_checked(kind) {
        if let Ok(version) = output(&existing, &["--version"], 16 * 1024) {
            if let Some(current) = crate::multi_agent::version_number(&String::from_utf8_lossy(&version.stdout)) {
                if crate::multi_agent::version_is_newer(&current, &release.version) { bail!("本机 CLI 版本高于官方稳定版本，不会自动降级；请保留原安装"); }
            }
        }
    }
    if let Some(operation) = jobs().lock().unwrap().get_mut(kind.as_str()).filter(|op| op.job.id == id) { operation.job.version = Some(release.version.clone()); }
    let folder = base.join(kind.as_str()); fs::create_dir_all(&folder).context("无法创建当前用户安装目录，请检查写入权限和磁盘空间")?;
    let staging = Staging(folder.join(format!(".staging-{id}"))); fs::create_dir(&staging.0)?;
    let archive = staging.0.join("download");
    download(&release, &archive, id, cancel, wake).await?;
    check_cancel(cancel)?;
    change(id, "installing", "完整性校验通过，正在准备官方 CLI…", 0, None);
    let prepared = staging.0.join("prepared"); fs::create_dir(&prepared)?;
    let extract_path = prepared.clone(); let archive_path = archive.clone(); let archive_format = release.archive; let cancel_copy = cancel.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        match archive_format {
            ArchiveFormat::TarGz => extract_archive(&archive_path, &extract_path, &cancel_copy)?,
            ArchiveFormat::Zip => additional::extract_zip(&archive_path, &extract_path, &cancel_copy)?,
            ArchiveFormat::File => fs::rename(&archive_path, extract_path.join("claude.exe"))?,
        }
        Ok(())
    }).await??;
    check_cancel(cancel)?;
    change(id, "verifying", "正在验证版本与 CLI 启动能力…", 0, None);
    let executable = prepared.join(relative_executable(kind)?);
    let copy = executable.clone(); let expected = release.version.clone();
    tauri::async_runtime::spawn_blocking(move || verify(&copy, kind, Some(&expected))).await??;
    check_cancel(cancel)?;
    // Publication and cancellation share the job lock. Cancel either wins
    // before this transaction, or observes the already completed installation.
    let mut state = jobs().lock().unwrap();
    check_cancel(cancel)?;
    let executable = publish(&folder, &prepared, kind, &release.version)?;
    if let Some(operation) = state.get_mut(kind.as_str()).filter(|op| op.job.id == id) { operation.job.phase = "ready".into(); }
    Ok((release.version, executable))
}
async fn download(release: &Release, target: &Path, id: &str, cancel: &AtomicBool, wake: &tokio::sync::Notify) -> Result<()> {
    let client = client()?;
    let request = client.get(&release.url);
    let mut response = tokio::select! { result = request.send() => result.map_err(|_| anyhow!("下载官方安装包失败，请检查网络后重试"))?, _ = wake.notified() => bail!("安装已取消") }
        .error_for_status().map_err(|_| anyhow!("官方安装包暂不可用"))?;
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_DOWNLOAD) { bail!("官方安装包超出大小限制"); }
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(target)?;
    let mut loaded = 0u64; let mut sha256 = Sha256::new(); let mut sha512 = Sha512::new();
    change(id, "downloading", "正在下载并校验官方安装文件…", loaded, total);
    loop {
        let chunk = tokio::select! { result = response.chunk() => result.map_err(|_| anyhow!("下载中断，可重试；原安装未修改"))?, _ = wake.notified() => bail!("安装已取消") };
        let Some(chunk) = chunk else { break; };
        check_cancel(cancel)?;
        loaded += chunk.len() as u64;
        if loaded > MAX_DOWNLOAD { bail!("官方安装包超出大小限制"); }
        file.write_all(&chunk)?; sha256.update(&chunk); sha512.update(&chunk);
        change(id, "downloading", "正在下载并校验官方安装文件…", loaded, total);
    }
    file.sync_all()?;
    let valid = if let Some(expected) = release.integrity.strip_prefix("sha256-") { format!("{:x}", sha256.finalize()) == expected }
        else if let Some(expected) = release.integrity.strip_prefix("sha512-") { base64::engine::general_purpose::STANDARD.encode(sha512.finalize()) == expected }
        else { false };
    if !valid || total.is_some_and(|size| size != loaded) { bail!("安装包完整性校验失败，未切换原安装"); }
    Ok(())
}
fn extract_archive(archive: &Path, directory: &Path, cancel: &AtomicBool) -> Result<()> {
    let gzip = flate2::read::GzDecoder::new(fs::File::open(archive)?);
    let mut tar = tar::Archive::new(gzip); let mut extracted = 0u64;
    for (index, entry) in tar.entries()?.enumerate() {
        check_cancel(cancel)?;
        if index > 4096 { bail!("安装包文件数超出限制"); }
        let mut entry = entry?; let relative = entry.path()?.to_path_buf();
        if relative.components().any(|part| !matches!(part, Component::Normal(_))) || relative.to_string_lossy().contains(':') { bail!("安装包含不安全路径"); }
        let path = directory.join(&relative);
        if entry.header().entry_type().is_dir() { fs::create_dir_all(&path)?; continue; }
        if !entry.header().entry_type().is_file() { bail!("安装包含非普通文件"); }
        extracted = extracted.checked_add(entry.size()).context("安装包大小溢出")?;
        if extracted > MAX_EXTRACTED { bail!("安装包解压大小超出限制"); }
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        let mut output = fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop { check_cancel(cancel)?; let count = entry.read(&mut buffer)?; if count == 0 { break; } output.write_all(&buffer[..count])?; }
    }
    Ok(())
}
fn publish(folder: &Path, prepared: &Path, kind: AgentRuntimeKind, version: &str) -> Result<PathBuf> {
    publish_with(folder, prepared, kind, version, crate::providers::write_atomic)
}
fn publish_with(folder: &Path, prepared: &Path, kind: AgentRuntimeKind, version: &str, write: impl FnOnce(&Path, &[u8]) -> Result<()>) -> Result<PathBuf> {
    if !valid_version(version) { bail!("安装版本号无效"); }
    if !prepared.join(relative_executable(kind)?).is_file() { bail!("CLI 安装入口缺失，原安装未修改"); }
    let directory = format!("{version}-{}", uuid::Uuid::new_v4());
    let versions = folder.join("versions"); fs::create_dir_all(&versions)?;
    let destination = versions.join(&directory);
    rename_for_publication(prepared, &destination).context("无法保存 CLI 版本目录；原安装未修改")?;
    let current = folder.join("current.json");
    if current.exists() { fs::copy(&current, folder.join("previous.json")).context("无法备份旧安装入口；原入口未修改")?; }
    write(&current, &serde_json::to_vec(&Installed { kind, version: version.into(), directory })?)
        .context("无法切换 CLI 入口；旧入口仍保留，新文件保留供排查")?;
    Ok(destination.join(relative_executable(kind)?))
}
fn rename_for_publication(source: &Path, destination: &Path) -> std::io::Result<()> {
    rename_for_publication_with(|| fs::rename(source, destination))
}
fn rename_for_publication_with(mut rename: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    // Windows can briefly retain the freshly verified EXE's image/scan handle.
    // Only retry sharing/access errors; a final failure leaves the old pointer intact.
    let mut delays = [100, 200, 400, 800, 1000].into_iter();
    loop {
        match rename() {
            Ok(()) => return Ok(()),
            Err(error) => {
                if cfg!(windows) && (error.kind() == std::io::ErrorKind::PermissionDenied || matches!(error.raw_os_error(), Some(32 | 33))) {
                    if let Some(delay) = delays.next() { std::thread::sleep(Duration::from_millis(delay)); continue; }
                }
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires CREWVELA_RUN_RUNTIME_LOCAL_PROBE=1; reads current Claude CLI version/help/auth status without installation or model calls"]
    fn local_runtime_environment_returns_without_official_release_lookup() {
        assert_eq!(std::env::var("CREWVELA_RUN_RUNTIME_LOCAL_PROBE").as_deref(), Ok("1"));
        let started=std::time::Instant::now();
        let result=tauri::async_runtime::block_on(super::environment(AgentRuntimeKind::ClaudeCode)).unwrap();
        assert!(result.executable.is_some(), "requires installed Claude CLI");
        assert!(result.version.is_some(), "local CLI self-check must complete");
        assert!(result.latest_version.is_none() && result.update_error.is_none(), "local inspection must be independent of official version lookup");
        eprintln!("LOCAL_RUNTIME_INSPECTION elapsed_ms={} verified={} auth={}",started.elapsed().as_millis(),result.version.is_some(),result.auth);
    }
    use super::*;
    fn temporary(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("crewvela-install-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap(); path
    }
    fn package(version: &str) -> serde_json::Value { serde_json::json!({"name":"@openai/codex","version":format!("{version}-win32-x64"),"dist":{"tarball":format!("https://registry.npmjs.org/@openai/codex/-/codex-{version}-win32-x64.tgz"),"integrity":format!("sha512-{}",base64::engine::general_purpose::STANDARD.encode([1;64]))}}) }
    #[test]
    fn runtime_install_release_rejects_wrong_owner_platform_version_url_or_digest() {
        let original = package("0.169.0");
        assert!(parse_codex_release(&original, "0.169.0", "win32-x64").is_ok());
        for (key, value) in [("name", "@someone/codex"), ("version", "0.169.0-linux-x64")] {
            let mut invalid = original.clone(); invalid[key] = value.into();
            assert!(parse_codex_release(&invalid, "0.169.0", "win32-x64").is_err());
        }
        for (key, value) in [("tarball", "https://example.com/codex.tgz"), ("integrity", "sha1-invalid")] {
            let mut invalid = original.clone(); invalid["dist"][key] = value.into();
            assert!(parse_codex_release(&invalid, "0.169.0", "win32-x64").is_err());
        }
        for version in ["../bad", "1.2.3/evil", "1.2.3-beta", "1.2", "", "1.2.3\n"] { assert!(!valid_version(version)); }
    }
    #[cfg(windows)]
    #[test]
    fn runtime_install_publish_keeps_previous_version_and_rolls_back_a_failed_pointer_write() {
        let base = temporary("transaction"); let folder = base.join("claude_code");
        fs::create_dir_all(&folder).unwrap();
        let prepare = |name: &str| { let p = base.join(name); fs::create_dir(&p).unwrap(); fs::write(p.join("claude.exe"), name).unwrap(); p };
        let first = publish(&folder, &prepare("first"), AgentRuntimeKind::ClaudeCode, "1.2.3").unwrap();
        let before = fs::read(folder.join("current.json")).unwrap();
        assert!(publish_with(&folder, &prepare("failed"), AgentRuntimeKind::ClaudeCode, "1.2.4", |_, _| bail!("fixture disk failure")).is_err());
        assert_eq!(fs::read(folder.join("current.json")).unwrap(), before);
        assert_eq!(installed_at(&base, AgentRuntimeKind::ClaudeCode).unwrap(), Some(first.clone()));
        let second = publish(&folder, &prepare("second"), AgentRuntimeKind::ClaudeCode, "1.2.4").unwrap();
        assert!(first.is_file()); assert_ne!(first, second);
        assert_eq!(fs::read(folder.join("previous.json")).unwrap(), before);
        assert_eq!(installed_at(&base, AgentRuntimeKind::ClaudeCode).unwrap(), Some(second));
        fs::remove_dir_all(base).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn runtime_install_manifest_cannot_select_another_executable_or_escape_versions() {
        let root = temporary("manifest"); let folder = root.join("codex"); fs::create_dir(&folder).unwrap();
        for directory in ["../../other", "C:\\other", "0.169.0-invalid"] {
            fs::write(folder.join("current.json"), serde_json::to_vec(&Installed { kind: AgentRuntimeKind::Codex, version: "0.169.0".into(), directory: directory.into() }).unwrap()).unwrap();
            assert!(installed_at(&root, AgentRuntimeKind::Codex).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn runtime_install_extract_rejects_links_and_cancellation_before_any_write() {
        let root = temporary("archive"); let archive = root.join("package.tgz");
        let gzip = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gzip);
        let mut header = tar::Header::new_gnu(); header.set_entry_type(tar::EntryType::Symlink); header.set_size(0); header.set_cksum();
        builder.append_link(&mut header, "package/escape", "../../outside").unwrap(); builder.into_inner().unwrap().finish().unwrap();
        let destination = root.join("target"); fs::create_dir(&destination).unwrap();
        assert!(extract_archive(&archive, &destination, &AtomicBool::new(false)).is_err());
        assert!(!destination.join("package/escape").exists());
        assert!(extract_archive(&archive, &destination, &AtomicBool::new(true)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn runtime_install_download_rejects_corrupt_content_and_cancel_preserves_previous_manifest() {
        use std::net::TcpListener;
        let root = temporary("download"); let current = root.join("current.json"); fs::write(&current, "unchanged").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || { let (mut stream, _) = listener.accept().unwrap(); let mut request = [0;4096]; let _ = stream.read(&mut request); stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad").unwrap(); });
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let release = Release { version: "1.2.3".into(), url: format!("http://127.0.0.1:{port}"), integrity: format!("sha256-{}", "0".repeat(64)), archive: ArchiveFormat::File };
        assert!(runtime.block_on(download(&release, &root.join("archive"), "fixture", &AtomicBool::new(false), &tokio::sync::Notify::new())).is_err());
        server.join().unwrap();
        assert_eq!(fs::read_to_string(current).unwrap(), "unchanged");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "downloads official Claude/Codex packages into isolated storage; no login or model requests"]
    fn runtime_install_real_official_packages_verify_before_switching_in_isolated_storage() {
        let base = temporary("official-live");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for kind in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex] {
            let id = uuid::Uuid::new_v4().to_string();
            let (version, executable) = runtime.block_on(install(&base, kind, &id, &Arc::new(AtomicBool::new(false)), &Arc::new(tokio::sync::Notify::new()))).unwrap();
            assert_eq!(installed_at(&base, kind).unwrap(), Some(executable.clone()));
            verify(&executable, kind, Some(&version)).unwrap();
            eprintln!("official isolated installation verified: {} {version}", kind.as_str());
        }
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn runtime_install_cancel_interrupts_a_stalled_download_without_switching_the_old_entry() {
        use std::{net::TcpListener, sync::mpsc};
        let root = temporary("cancel-download");
        let current = root.join("current.json"); fs::write(&current, "old-entry").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let port = listener.local_addr().unwrap().port();
        let began = Arc::new(AtomicBool::new(false)); let accepted = began.clone(); let (release, wait) = mpsc::channel();
        let server = std::thread::spawn(move || { let (mut stream, _) = listener.accept().unwrap(); let mut request = [0;4096]; let _ = stream.read(&mut request); stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\na").unwrap(); accepted.store(true, Ordering::Release); let _ = wait.recv_timeout(Duration::from_secs(5)); });
        let cancel = Arc::new(AtomicBool::new(false)); let wake = Arc::new(tokio::sync::Notify::new());
        let release_info = Release { version: "1.2.3".into(), url: format!("http://127.0.0.1:{port}"), integrity: "sha256-not-complete".into(), archive: ArchiveFormat::File };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let partial = root.join("partial");
        let (result, _) = runtime.block_on(async { tokio::join!(
            download(&release_info, &partial, "fixture-cancel", &cancel, &wake),
            async { while !began.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(1)).await; } cancel.store(true, Ordering::Release); wake.notify_one(); }
        ) });
        assert!(result.unwrap_err().to_string().contains("取消"));
        assert_eq!(fs::read_to_string(current).unwrap(), "old-entry");
        release.send(()).unwrap(); server.join().unwrap(); fs::remove_dir_all(root).unwrap();
    }
}

pub(crate) fn open_login(kind: AgentRuntimeKind) -> Result<(), String> {
    let for_chat = chat::supports(kind);
    let kind = chat::cli_kind(kind).unwrap_or(kind);
    if !supported_kind(kind) { return Err("不支持此 CLI 的登录引导".into()); }
    let executable = crate::multi_agent::resolve_runtime_executable_checked(kind)?;
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        let path = executable.display().to_string().replace('\'', "''");
        let script = additional::login_script(kind, &path)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
        let mut command = Command::new("powershell.exe");
        if for_chat { crate::runtime_dependencies::with_managed_node(&mut command); }
        command.args(["-NoLogo", "-NoProfile", "-NoExit", "-EncodedCommand", &encoded]).creation_flags(0x00000010).spawn().map_err(|e| format!("无法打开 CLI 登录窗口：{e}"))?;
        Ok(())
    }
    #[cfg(not(windows))] { let _ = (executable, for_chat); Err("请在原生 CLI 中完成登录".into()) }
}
