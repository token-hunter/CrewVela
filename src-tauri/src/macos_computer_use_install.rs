//! Official macOS runtime installation. No remote script is executed and no TCC
//! permission or unrelated agent configuration is changed by this module.
use crate::macos_computer_use::MacControlTool;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::{HashMap, HashSet, VecDeque}, fs, io::{Read, Write}, os::unix::{fs::OpenOptionsExt, io::AsRawFd}, path::{Component, Path, PathBuf}, process::Command, time::Duration};

const MAX_METADATA: usize = 2 * 1024 * 1024;
const MAX_ARCHIVE: usize = 256 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 1024 * 1024 * 1024;
const KIMI_TEAM: &str = "2J9472RW75";
// Pinned to the publisher of the official app, not to the download transport.
const OCU_TEAM: &str = "J9P29FA5BX";
static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug)]
struct Release {
    version: String,
    url: String,
    sha256: String,
    size: Option<u64>,
    bundle_id: &'static str,
    app_name: &'static str,
    team_id: &'static str,
}

fn version(value: &str) -> Result<String> {
    let value = value.strip_prefix('v').unwrap_or(value);
    if value.len() > 40 || value.split('.').count() != 3 || !value.split('.').all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit())) {
        bail!("官方版本号格式不兼容，未安装");
    }
    Ok(value.into())
}
fn digest(value: &str) -> Result<String> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) { bail!("官方 SHA256 缺失或格式无效"); }
    Ok(value.to_ascii_lowercase())
}
fn allowed_url(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some_and(|p| p != 443) { return false; }
    match url.host_str() {
        Some("api.github.com") => url.path().starts_with("/repos/iFurySt/open-computer-use/"),
        Some("github.com") => url.path().starts_with("/iFurySt/open-computer-use/releases/download/"),
        Some("release-assets.githubusercontent.com") | Some("objects.githubusercontent.com") | Some("github-releases.githubusercontent.com") => true,
        Some("cdn.kimi.com") => url.path().starts_with("/kimi-computer-use/"),
        _ => false,
    }
}
fn official_asset_url(raw: &str, tool: MacControlTool, expected_version: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw).context("官方安装包 URL 无效")?;
    let prefix = match tool {
        MacControlTool::OpenComputerUse => format!("https://github.com/iFurySt/open-computer-use/releases/download/v{expected_version}/"),
        MacControlTool::KimiCu => format!("https://cdn.kimi.com/kimi-computer-use/{expected_version}/"),
    };
    if !allowed_url(&url) || !raw.starts_with(&prefix) || url.query().is_some() || url.fragment().is_some() || !url.path().ends_with(".zip") {
        bail!("安装包地址不属于预期官方版本，拒绝下载");
    }
    Ok(raw.into())
}
fn select_release(tool: MacControlTool, metadata: &Value, silicon: bool) -> Result<Release> {
    match tool {
        MacControlTool::OpenComputerUse => {
            if metadata["draft"] == true || metadata["prerelease"] == true { bail!("不安装草稿或预发布运行时"); }
            let version = version(metadata["tag_name"].as_str().context("官方发布缺少版本")?)?;
            let expected = format!("Open-Computer-Use-{version}-macOS-universal.app.zip");
            let candidates: Vec<_> = metadata["assets"].as_array().context("官方发布缺少资产")?.iter().filter(|asset| asset["name"] == expected).collect();
            if candidates.len() != 1 { bail!("官方发布没有唯一的 macOS Universal 应用包"); }
            let asset = candidates[0];
            let size = asset["size"].as_u64().filter(|s| *s > 0 && *s <= MAX_ARCHIVE as u64).context("官方安装包大小无效")?;
            Ok(Release { url: official_asset_url(asset["browser_download_url"].as_str().context("缺少官方包地址")?, tool, &version)?,
                sha256: digest(asset["digest"].as_str().context("官方发布没有 SHA256，未安装")?)?, version, size: Some(size),
                bundle_id: "com.ifuryst.opencomputeruse", app_name: "Open Computer Use.app", team_id: OCU_TEAM })
        }
        MacControlTool::KimiCu => {
            let version = version(metadata["version"].as_str().context("Kimi 官方清单缺少版本")?)?;
            let asset = &metadata[if silicon { "app" } else { "app_x86_64" }];
            if asset["team_id"].as_str() != Some(KIMI_TEAM) { bail!("Kimi 官方清单签名身份不匹配"); }
            Ok(Release { url: official_asset_url(asset["url"].as_str().context("Kimi 尚未发布当前芯片的安装包")?, tool, &version)?,
                sha256: digest(asset["sha256"].as_str().context("Kimi 官方清单缺少 SHA256")?)?, version, size: None,
                bundle_id: "ai.kimi.cu", app_name: "KimiCU.app", team_id: KIMI_TEAM })
        }
    }
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent("CrewVela-runtime-installer")
        .connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed_url(attempt.url()) { attempt.error("非官方或过多重定向") }
            else { attempt.follow() }
        })).build()?)
}
async fn download(client: &reqwest::Client, url: &str, limit: usize, expected_size: Option<u64>, progress: &(impl Fn(&str) -> Result<()> + Sync)) -> Result<Vec<u8>> {
    if !allowed_url(&reqwest::Url::parse(url)?) { bail!("下载地址不在官方白名单"); }
    let mut response = client.get(url).timeout(Duration::from_secs(10 * 60)).send().await
        .map_err(|_| anyhow!("连接官方服务器失败或超时，请检查网络后重试"))?;
    if !response.status().is_success() { bail!("官方服务器返回 HTTP {}，请稍后重试", response.status().as_u16()); }
    let total = response.content_length();
    if total.is_some_and(|size| size > limit as u64) || expected_size.zip(total).is_some_and(|(expected, actual)| expected != actual) {
        bail!("官方响应大小超限或与清单不一致");
    }
    let mut bytes = Vec::new();
    let mut last_percent = 101;
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!("下载中断或停滞，已有安装未修改，请重试"))? {
        if bytes.len().saturating_add(chunk.len()) > limit { bail!("下载内容超过大小限制"); }
        bytes.extend_from_slice(&chunk);
        if let Some(total) = expected_size.or(total).filter(|size| *size > 0) {
            let percent = ((bytes.len() as u64 * 100 / total).min(100)) as usize;
            if percent / 5 != last_percent / 5 { progress(&format!("正在下载官方运行时 · {percent}%"))?; last_percent = percent; }
        }
    }
    if expected_size.is_some_and(|size| size != bytes.len() as u64) { bail!("安装包下载不完整，已有安装未修改"); }
    Ok(bytes)
}
fn verify_digest(bytes: &[u8], release: &Release) -> Result<()> {
    if format!("{:x}", Sha256::digest(bytes)) != release.sha256 || release.size.is_some_and(|size| size != bytes.len() as u64) {
        bail!("官方安装包 SHA256 或大小校验失败，已有安装未修改");
    }
    Ok(())
}
fn resolve_relative(base: &Path, relative: &Path) -> Option<PathBuf> {
    if relative.is_absolute() { return None; }
    let mut result = base.to_path_buf();
    for part in relative.components() {
        match part { Component::Normal(value) => result.push(value), Component::CurDir => {}, Component::ParentDir => { if !result.pop() { return None; } }, _ => return None }
    }
    Some(result)
}
/// Validate *before* ditto sees the archive. Links may only point within the
/// same top-level app, including framework Versions/Current links.
fn validate_archive(bytes: &[u8]) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("官方安装包不是有效 ZIP")?;
    if zip.len() == 0 || zip.len() > 20000 { bail!("安装包条目数无效或过多"); }
    let mut paths = HashSet::new();
    let mut links = HashMap::new();
    let mut app_name = None;
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_owned();
        if name.contains('\\') || name.contains('\0') { bail!("安装包路径包含非法字符"); }
        let path = entry.enclosed_name().context("安装包含越界路径")?.to_path_buf();
        if path.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) || !paths.insert(path.clone()) { bail!("安装包包含重复或越界路径"); }
        let root = path.components().next().and_then(|c| c.as_os_str().to_str()).context("安装包路径无效")?;
        expanded = expanded.checked_add(entry.size()).context("安装包展开大小溢出")?;
        if expanded > MAX_EXTRACTED { bail!("安装包展开大小超限"); }
        let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
        if !matches!(kind, 0 | 0o040000 | 0o100000 | 0o120000) { bail!("安装包含不支持的文件类型"); }
        if root == "__MACOSX" || root == ".DS_Store" {
            if kind == 0o120000 { bail!("安装包元数据不能包含符号链接"); }
            continue;
        }
        if !root.ends_with(".app") { bail!("安装包含非应用内容"); }
        match &app_name { Some(existing) if existing != root => bail!("安装包含多个应用"), None => app_name = Some(root.to_owned()), _ => {} }
        if kind == 0o120000 {
            if entry.size() > 4096 { bail!("安装包符号链接过长"); }
            let mut target = String::new(); entry.read_to_string(&mut target).context("安装包符号链接编码无效")?;
            let resolved = resolve_relative(path.parent().context("符号链接路径无效")?, Path::new(&target)).context("安装包符号链接越界")?;
            if resolved.components().next().map(|c| c.as_os_str()) != path.components().next().map(|c| c.as_os_str()) { bail!("安装包符号链接逃逸应用目录"); }
            links.insert(path.clone(), PathBuf::from(target));
        }
    }
    let app_name = app_name.context("安装包内没有应用")?;
    for path in &paths {
        if path.ancestors().skip(1).any(|parent| links.contains_key(parent)) {
            bail!("安装包不能向符号链接目录内写入条目");
        }
    }
    for (path, target) in &links {
        validate_link_chain(Path::new(&app_name), &path.parent().context("符号链接路径无效")?.join(target), &links)?;
    }
    Ok(app_name)
}
// Resolve against the *complete* archive link table, not just lexically: a -> .,
// b -> a/.. would escape even though both individual lexical paths look safe.
fn validate_link_chain(root: &Path, candidate: &Path, links: &HashMap<PathBuf, PathBuf>) -> Result<()> {
    let mut remaining: VecDeque<_> = candidate.components().map(|part| part.as_os_str().to_owned()).collect();
    let mut resolved = PathBuf::new();
    let mut expansions = 0;
    while let Some(part) = remaining.pop_front() {
        if part == "." { continue; }
        if part == ".." {
            if resolved == root || !resolved.pop() { bail!("符号链接链逃逸应用目录"); }
            continue;
        }
        resolved.push(part);
        if !resolved.starts_with(root) { bail!("符号链接链逃逸应用目录"); }
        if let Some(target) = links.get(&resolved) {
            expansions += 1;
            if expansions > 128 { bail!("安装包符号链接循环或链过长"); }
            resolved.pop();
            for part in target.components().rev() { remaining.push_front(part.as_os_str().to_owned()); }
        }
    }
    if !resolved.starts_with(root) { bail!("符号链接链逃逸应用目录"); }
    Ok(())
}
fn run(executable: &Path, args: &[&std::ffi::OsStr], timeout: Duration, title: &str) -> Result<String> {
    let mut command = Command::new(executable); command.args(args);
    let output = crate::title_summary::run_cli_with_timeout(command, None, timeout, 128 * 1024, title)?;
    if !output.status.success() { bail!("{title}失败（{}），请检查安装权限或官方应用状态后重试", output.status); }
    let bytes = if output.stdout.is_empty() { output.stderr } else { output.stdout };
    String::from_utf8(bytes).context("官方命令输出编码无效")
}
fn plist(app: &Path) -> Result<Value> {
    let result = run(Path::new("/usr/bin/plutil"), &["-convert".as_ref(), "json".as_ref(), "-o".as_ref(), "-".as_ref(), app.join("Contents/Info.plist").as_os_str()], Duration::from_secs(15), "应用元数据检查")?;
    serde_json::from_str(&result).context("应用元数据不是 JSON")
}
fn bundle_executable(app: &Path, info: &Value, release: &Release) -> Result<PathBuf> {
    if info["CFBundleIdentifier"] != release.bundle_id { bail!("应用标识不匹配，拒绝替换"); }
    let name = info["CFBundleExecutable"].as_str().context("应用缺少执行入口")?;
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." { bail!("应用执行入口无效"); }
    let path = app.join("Contents/MacOS").join(name);
    let canonical = path.canonicalize().context("应用执行入口不存在")?;
    if !canonical.starts_with(app.canonicalize()?) || !canonical.is_file() { bail!("应用执行入口越界或无效"); }
    Ok(path)
}
fn validate_bundle(app: &Path, release: &Release, silicon: bool, exact_version: bool) -> Result<PathBuf> {
    let info = plist(app)?;
    let executable = bundle_executable(app, &info, release)?;
    if exact_version && info["CFBundleShortVersionString"].as_str() != Some(&release.version) { bail!("官方清单与应用版本不一致"); }
    if let Some(minimum) = info["LSMinimumSystemVersion"].as_str() {
        let required: Vec<u32> = minimum.split('.').map(str::parse).collect::<std::result::Result<_,_>>().context("应用最低系统版本格式无效")?;
        let actual = run(Path::new("/usr/bin/sw_vers"), &["-productVersion".as_ref()], Duration::from_secs(15), "系统版本检查")?;
        let mut current: Vec<u32> = actual.trim().split('.').map(str::parse).collect::<std::result::Result<_,_>>()?;
        let mut required = required; current.resize(3,0); required.resize(3,0);
        if current < required { bail!("运行时要求 macOS {minimum} 或更新版本"); }
    }
    use std::os::unix::fs::PermissionsExt;
    if fs::metadata(&executable)?.permissions().mode() & 0o111 == 0 { bail!("应用执行入口不可执行"); }
    let header: Vec<u8> = fs::File::open(&executable)?.take(4096).bytes().collect::<std::io::Result<_>>()?;
    if !supports_arch(&header, silicon) { bail!("运行时不支持本机芯片"); }
    run(Path::new("/usr/bin/codesign"), &["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), app.as_os_str()], Duration::from_secs(30), "应用签名校验")?;
    let signing = run(Path::new("/usr/bin/codesign"), &["-dvvv".as_ref(), app.as_os_str()], Duration::from_secs(15), "签名身份检查")?;
    if !signing.lines().any(|line| line == format!("TeamIdentifier={}", release.team_id)) || !signing.lines().any(|line| line.starts_with("Timestamp=")) {
        bail!("应用签名身份或安全时间戳不匹配，拒绝安装");
    }
    run(Path::new("/usr/sbin/spctl"), &["--assess".as_ref(), "--type".as_ref(), "exec".as_ref(), app.as_os_str()], Duration::from_secs(60), "Gatekeeper 公证检查")?;
    Ok(executable)
}
fn supports_arch(bytes: &[u8], silicon: bool) -> bool {
    let supported = |cpu| if silicon { cpu == 0x0100000c } else { cpu == 0x01000007 };
    if bytes.get(..4) == Some(&[0xcf,0xfa,0xed,0xfe]) { return bytes.get(4..8).is_some_and(|v| supported(u32::from_le_bytes(v.try_into().unwrap()))); }
    if matches!(bytes.get(..4), Some([0xca,0xfe,0xba,0xbe] | [0xca,0xfe,0xba,0xbf])) {
        let count = bytes.get(4..8).map(|v|u32::from_be_bytes(v.try_into().unwrap()) as usize).unwrap_or(0);
        let stride = if bytes[3] == 0xbf { 32 } else { 20 };
        return count > 0 && count <= 32 && bytes.len() >= 8 + count * stride && (0..count).any(|i| supported(u32::from_be_bytes(bytes[8+i*stride..12+i*stride].try_into().unwrap())));
    }
    false
}
fn actual_silicon() -> bool {
    cfg!(target_arch = "aarch64") || run(Path::new("/usr/sbin/sysctl"), &["-n".as_ref(), "hw.optional.arm64".as_ref()], Duration::from_secs(10), "芯片检查").is_ok_and(|value| value.trim() == "1")
}
struct Directory(PathBuf);
impl Drop for Directory { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
struct FileLock(fs::File);
impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = fs::OpenOptions::new().write(true).create(true).mode(0o600).open(path)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 { bail!("另一安装任务正在进行，请等待完成后重试"); }
        Ok(Self(file))
    }
}
impl Drop for FileLock { fn drop(&mut self) { unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN); } } }
fn publish_app(stage: &Path, destination: &Path, verify_existing: impl Fn(&Path) -> Result<()>, verify_new: impl Fn(&Path) -> Result<()>) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok_and(|meta| meta.file_type().is_symlink()) { bail!("目标应用是符号链接，已保留，不能自动替换"); }
    let backup = destination.with_file_name(format!(".crewvela-old-{}.app", uuid::Uuid::new_v4()));
    let existed = destination.exists();
    if existed { verify_existing(destination).context("已有目标不是可替换的官方应用，已保留")?; fs::rename(destination, &backup).context("无法保留旧应用")?; }
    if let Err(error) = fs::rename(stage, destination).and_then(|_| verify_new(destination).map_err(std::io::Error::other)) {
        let _ = fs::remove_dir_all(destination);
        if existed { fs::rename(&backup, destination).context("恢复旧应用失败；旧包仍保存在同目录隐藏备份中")?; }
        return Err(error).context("安装未完成，旧应用已恢复");
    }
    if existed { fs::remove_dir_all(&backup).context("安装成功，但旧应用备份未清理")?; }
    Ok(())
}
fn stage_archive(bytes: &[u8], directory: &Path) -> Result<PathBuf> {
    let app_name = validate_archive(bytes)?;
    let archive = directory.join("official-runtime.zip"); fs::write(&archive, bytes)?;
    let expanded = directory.join("expanded"); fs::create_dir(&expanded)?;
    run(Path::new("/usr/bin/ditto"), &["-x".as_ref(), "-k".as_ref(), archive.as_os_str(), expanded.as_os_str()], Duration::from_secs(90), "官方应用解压")?;
    let app = expanded.join(app_name);
    let canonical = expanded.canonicalize()?;
    let mut pending = vec![app.clone()];
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            if !path.canonicalize().is_ok_and(|target| target.starts_with(&canonical)) { bail!("解压后符号链接越界或无效"); }
        } else if meta.is_dir() { for child in fs::read_dir(path)? { pending.push(child?.path()); } }
    }
    Ok(app)
}

pub async fn install(tool: MacControlTool, progress: &(impl Fn(&str) -> Result<()> + Sync)) -> Result<PathBuf> {
    let _single = INSTALL.try_lock().map_err(|_| anyhow!("已有运行时正在安装，请稍候"))?;
    let config = crate::providers::config_dir()?; fs::create_dir_all(&config)?;
    let _lock = FileLock::acquire(&config.join("computer-use-install.lock"))?;
    let silicon = actual_silicon();
    let client = client()?;
    progress("正在获取官方版本清单")?;
    let metadata_url = match tool { MacControlTool::OpenComputerUse => "https://api.github.com/repos/iFurySt/open-computer-use/releases/latest", MacControlTool::KimiCu => "https://cdn.kimi.com/kimi-computer-use/latest/version.json" };
    let metadata: Value = serde_json::from_slice(&download(&client, metadata_url, MAX_METADATA, None, &|_| Ok(())).await?).context("官方版本清单格式无效")?;
    let release = select_release(tool, &metadata, silicon)?;
    progress(&format!("准备安装官方运行时 {}，已有应用尚未修改", release.version))?;
    let bytes = download(&client, &release.url, MAX_ARCHIVE, release.size, progress).await?;
    progress("下载完成，正在校验 SHA256")?; verify_digest(&bytes, &release)?;
    let parent = match tool { MacControlTool::OpenComputerUse => dirs::home_dir().context("无法定位用户目录")?.join("Applications"), MacControlTool::KimiCu => PathBuf::from("/Applications") };
    fs::create_dir_all(&parent).context("无法创建应用目录，请检查安装权限")?;
    let directory = Directory(parent.join(format!(".crewvela-install-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&directory.0).context("无法写入 Applications，请确认当前用户具备安装权限")?;
    progress("正在安全解压并验证应用签名与公证")?;
    let app = stage_archive(&bytes, &directory.0)?;
    let executable = validate_bundle(&app, &release, silicon, true)?;
    let destination = parent.join(release.app_name);
    if fs::symlink_metadata(&destination).is_ok_and(|m|m.file_type().is_symlink()) { bail!("目标应用为符号链接，已保留"); }
    if destination.exists() { validate_bundle(&destination, &release, silicon, false).context("已有应用身份校验未通过，已保留")?; }
    progress("校验通过，正在安装官方运行时")?;
    match tool {
        MacControlTool::OpenComputerUse => publish_app(&app, &destination,
            |candidate| validate_bundle(candidate, &release, silicon, false).map(|_| ()),
            |candidate| validate_bundle(candidate, &release, silicon, true).map(|_| ()))?,
        MacControlTool::KimiCu => {
            // The verified publisher binary owns the service transaction and
            // rollback. Never emulate it with kill/bootout or a custom plist.
            run(&executable, &["install-bundle".as_ref(), app.as_os_str(), destination.as_os_str(), release.version.as_ref()], Duration::from_secs(180), "KimiCU 官方安装与服务准备")?;
        }
    }
    let installed = validate_bundle(&destination, &release, silicon, true)?;
    progress("运行时安装并校验完成，接下来请完成系统授权")?;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temporary() -> Directory {
        let root = std::env::temp_dir().join(format!("crewvela-install-unit-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); Directory(root)
    }
    fn kimi_manifest() -> Value {
        serde_json::json!({"version":"0.7.6","app":{"url":"https://cdn.kimi.com/kimi-computer-use/0.7.6/KimiCU.app.zip","sha256":"a".repeat(64),"team_id":KIMI_TEAM},"app_x86_64":{"url":"https://cdn.kimi.com/kimi-computer-use/0.7.6/KimiCU-x86_64.app.zip","sha256":"b".repeat(64),"team_id":KIMI_TEAM}})
    }
    #[test]
    fn selects_matching_architecture_and_rejects_manifest_identity_and_url_drift() {
        let mut manifest = kimi_manifest();
        assert_eq!(select_release(MacControlTool::KimiCu,&manifest,true).unwrap().sha256,"a".repeat(64));
        assert_eq!(select_release(MacControlTool::KimiCu,&manifest,false).unwrap().sha256,"b".repeat(64));
        manifest["app_x86_64"] = Value::Null;
        assert!(select_release(MacControlTool::KimiCu,&manifest,false).is_err());
        manifest["app"]["url"] = Value::String("https://untrusted.invalid/KimiCU.app.zip".into());
        assert!(select_release(MacControlTool::KimiCu,&manifest,true).is_err());
        manifest = kimi_manifest(); manifest["app"]["team_id"] = Value::String("UNTRUSTED".into());
        assert!(select_release(MacControlTool::KimiCu,&manifest,true).is_err());
    }
    #[test]
    fn ocu_selection_requires_published_exact_universal_asset_and_digest() {
        let manifest = serde_json::json!({"tag_name":"v1.1.4","assets":[{"name":"Open-Computer-Use-1.1.4-macOS-universal.app.zip","size":123,"digest":format!("sha256:{}","a".repeat(64)),"browser_download_url":"https://github.com/iFurySt/open-computer-use/releases/download/v1.1.4/Open-Computer-Use-1.1.4-macOS-universal.app.zip"}]});
        assert_eq!(select_release(MacControlTool::OpenComputerUse,&manifest,true).unwrap().team_id,OCU_TEAM);
        let mut draft = manifest.clone(); draft["draft"] = Value::Bool(true);
        assert!(select_release(MacControlTool::OpenComputerUse,&draft,true).is_err());
        let mut missing = manifest.clone(); missing["assets"][0]["digest"] = Value::Null;
        assert!(select_release(MacControlTool::OpenComputerUse,&missing,true).is_err());
        let mut wrong = manifest; wrong["assets"][0]["browser_download_url"] = Value::String("https://github.com/iFurySt/open-computer-use/releases/download/v9.0.0/app.zip".into());
        assert!(select_release(MacControlTool::OpenComputerUse,&wrong,true).is_err());
    }
    #[test]
    fn official_transport_rejects_cleartext_credentials_ports_and_other_repositories() {
        for raw in ["http://cdn.kimi.com/kimi-computer-use/x", "https://cdn.kimi.com:8443/kimi-computer-use/x", "https://user@cdn.kimi.com/kimi-computer-use/x", "https://github.com/evil/project/releases/download/x", "https://evil.github.com/iFurySt/open-computer-use/releases/download/x"] {
            assert!(!allowed_url(&reqwest::Url::parse(raw).unwrap()),"{raw}");
        }
        assert!(allowed_url(&reqwest::Url::parse("https://release-assets.githubusercontent.com/github-production-release-asset/file").unwrap()));
    }
    #[test]
    fn checksum_rejects_truncation_and_modified_data() {
        let mut release = select_release(MacControlTool::KimiCu,&kimi_manifest(),true).unwrap();
        release.sha256 = format!("{:x}",Sha256::digest(b"official")); release.size = Some(8);
        assert!(verify_digest(b"official",&release).is_ok());
        assert!(verify_digest(b"officia",&release).is_err());
        assert!(verify_digest(b"Official",&release).is_err());
    }
    fn zip(entries: &[(&str, &str, bool)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (path, contents, symlink) in entries {
            if *symlink { writer.add_symlink(*path,*contents,options).unwrap(); }
            else { writer.start_file(*path,options).unwrap(); writer.write_all(contents.as_bytes()).unwrap(); }
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn validates_framework_links_and_rejects_traversal_metadata_links_and_multiple_apps() {
        let safe = zip(&[("Example.app/Contents/Info.plist","{}",false),("Example.app/Contents/Frameworks/F.framework/Versions/A/F","binary",false),("Example.app/Contents/Frameworks/F.framework/Versions/Current","A",true)]);
        assert_eq!(validate_archive(&safe).unwrap(),"Example.app");
        for entries in [vec![("../outside","x",false)],vec![("Example.app/Contents/link","../../../outside",true)],vec![("Example.app/Contents/link","/tmp/outside",true)],vec![("__MACOSX/link","/tmp/outside",true),("Example.app/Contents/Info.plist","{}",false)],vec![("A.app/file","a",false),("B.app/file","b",false)]] {
            assert!(validate_archive(&zip(&entries)).is_err(),"{entries:?}");
        }
    }
    #[test]
    fn rejects_symlink_ancestors_and_nonlexical_escape_chains() {
        for entries in [
            vec![("Example.app/a",".",true),("Example.app/a/evil","..",true)],
            vec![("Example.app/a",".",true),("Example.app/b","a/..",true)],
            vec![("Example.app/a","b",true),("Example.app/b","a",true)],
            vec![("Example.app/a","Contents",true),("Example.app/a/file","data",false)],
        ] { assert!(validate_archive(&zip(&entries)).is_err(), "{entries:?}"); }
    }
    #[test]
    fn publication_restores_old_application_after_new_verification_failure() {
        let root = temporary(); let target = root.0.join("Official.app"); let stage = root.0.join("Staged.app");
        fs::create_dir(&target).unwrap(); fs::write(target.join("old"),b"old").unwrap();
        fs::create_dir(&stage).unwrap(); fs::write(stage.join("new"),b"new").unwrap();
        let result = publish_app(&stage,&target,|_|Ok(()),|_|bail!("simulated final verification failure"));
        assert!(result.is_err()); assert_eq!(fs::read(target.join("old")).unwrap(),b"old"); assert!(!target.join("new").exists());
    }
    #[test]
    fn publication_refuses_foreign_existing_apps_and_symlinks_without_touching_them() {
        let root = temporary(); let target = root.0.join("Official.app"); let stage = root.0.join("Staged.app");
        fs::create_dir(&target).unwrap(); fs::write(target.join("user"),b"keep").unwrap(); fs::create_dir(&stage).unwrap();
        assert!(publish_app(&stage,&target,|_|bail!("foreign app"),|_|Ok(())).is_err());
        assert_eq!(fs::read(target.join("user")).unwrap(),b"keep"); assert!(stage.exists());
        let link = root.0.join("Link.app"); std::os::unix::fs::symlink(&target,&link).unwrap();
        assert!(publish_app(&stage,&link,|_|Ok(()),|_|Ok(())).is_err()); assert!(stage.exists());
    }
    #[test]
    fn file_lock_rejects_concurrent_install_and_unlocks_on_drop() {
        let root = temporary(); let path = root.0.join("lock");
        let first = FileLock::acquire(&path).unwrap(); assert!(FileLock::acquire(&path).is_err()); drop(first);
        assert!(FileLock::acquire(&path).is_ok());
    }
    #[tokio::test]
    #[ignore = "Downloads and verifies official apps in a temporary directory; never installs or requests permissions"]
    async fn official_downloads_validate_without_installing() -> Result<()> {
        let client = client()?;
        for tool in [MacControlTool::OpenComputerUse,MacControlTool::KimiCu] {
            let url = match tool { MacControlTool::OpenComputerUse => "https://api.github.com/repos/iFurySt/open-computer-use/releases/latest", MacControlTool::KimiCu => "https://cdn.kimi.com/kimi-computer-use/latest/version.json" };
            let metadata: Value = serde_json::from_slice(&download(&client,url,MAX_METADATA,None,&|_|Ok(())).await?)?;
            let release = select_release(tool,&metadata,actual_silicon())?;
            let bytes = download(&client,&release.url,MAX_ARCHIVE,release.size,&|_|Ok(())).await?;
            verify_digest(&bytes,&release)?;
            let directory = temporary(); let app = stage_archive(&bytes,&directory.0)?;
            validate_bundle(&app,&release,actual_silicon(),true)?;
            println!("Verified official {} {} without installation",release.bundle_id,release.version);
        }
        Ok(())
    }
}
