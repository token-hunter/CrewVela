//! Official native distributions for the existing OpenCode 1.x adapter, Kimi and Pi.
use super::*;

pub(super) async fn release(client: &reqwest::Client, kind: AgentRuntimeKind, platform: &str) -> Result<Release> {
    if kind == AgentRuntimeKind::Antigravity {
        let (manifest, _, _) = antigravity_platform(platform)?;
        let url = format!("https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/{manifest}.json");
        let metadata = serde_json::from_str(&text(client, &url).await?)?;
        return parse_antigravity_release(&metadata, platform);
    }
    if kind == AgentRuntimeKind::OpenCode {
        // Stay on the same upstream channel as the existing run --format json / ACP adapters.
        let main: serde_json::Value = serde_json::from_str(&text(client, "https://registry.npmjs.org/opencode-ai/latest").await?)?;
        let version = main["version"].as_str().filter(|value| valid_version(value) && value.starts_with("1.")).context("OpenCode 1.x 官方版本无效，请检查兼容渠道")?;
        let package = opencode_package(platform)?;
        if main["name"] != "opencode-ai" || main["optionalDependencies"][package] != version { bail!("OpenCode 官方平台依赖身份不符"); }
        let metadata: serde_json::Value = serde_json::from_str(&text(client, &format!("https://registry.npmjs.org/{package}/{version}")).await?)?;
        return parse_opencode_release(&metadata, version, platform);
    }
    let (repo, _) = github_spec(kind, platform)?;
    let metadata: serde_json::Value = serde_json::from_str(&text(client, &format!("https://api.github.com/repos/{repo}/releases/latest")).await?)?;
    parse_github_release(&metadata, kind, platform)
}

fn antigravity_platform(platform: &str) -> Result<(&'static str, &'static str, &'static str)> {
    match platform {
        "win32-x64" => Ok(("windows_amd64", "windows-x64", "cli_windows_x64.exe")),
        "win32-arm64" => Ok(("windows_arm64", "windows-arm", "cli_windows_arm64.exe")),
        _ => bail!("Antigravity CLI 未提供此平台安装包"),
    }
}

fn parse_antigravity_release(value: &serde_json::Value, platform: &str) -> Result<Release> {
    let (_, directory, filename) = antigravity_platform(platform)?;
    let version = value["version"].as_str().filter(|version| valid_version(version)).context("Antigravity CLI 官方版本号无效")?;
    let url = value["url"].as_str().context("官方发行缺少下载地址")?;
    let parsed = reqwest::Url::parse(url)?;
    let segments: Vec<_> = parsed.path_segments().context("Antigravity CLI 下载路径无效")?.collect();
    let valid_path = matches!(segments.as_slice(), ["antigravity-public", "antigravity-cli", build, dir, name]
        if *dir == directory && *name == filename && build.strip_prefix(version)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())));
    if parsed.scheme() != "https" || parsed.host_str() != Some("storage.googleapis.com")
        || !parsed.username().is_empty() || parsed.password().is_some() || parsed.port().is_some()
        || parsed.query().is_some() || parsed.fragment().is_some() || !valid_path {
        bail!("下载地址不属于预期 Antigravity CLI 官方平台包");
    }
    let digest = value["sha512"].as_str().filter(|digest| digest.len() == 128 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("Antigravity CLI 官方发行缺少有效 SHA512")?;
    let bytes = (0..digest.len()).step_by(2).map(|index| u8::from_str_radix(&digest[index..index + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Release { version: version.into(), url: url.into(),
        integrity: format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(bytes)), archive: ArchiveFormat::File })
}

fn opencode_package(platform: &str) -> Result<&'static str> {
    match platform {
        // Baseline avoids requiring AVX2 on older Windows x64 CPUs.
        "win32-x64" => Ok("opencode-windows-x64-baseline"),
        "win32-arm64" => Ok("opencode-windows-arm64"),
        _ => bail!("OpenCode 未提供此平台安装包"),
    }
}
fn parse_opencode_release(value: &serde_json::Value, version: &str, platform: &str) -> Result<Release> {
    let package = opencode_package(platform)?;
    if !valid_version(version) || !version.starts_with("1.") || value["name"] != package || value["version"] != version { bail!("OpenCode 官方平台包身份不符"); }
    let expected = format!("https://registry.npmjs.org/{package}/-/{package}-{version}.tgz");
    if value["dist"]["tarball"] != expected { bail!("OpenCode 下载地址不属于预期官方发布"); }
    let integrity = value["dist"]["integrity"].as_str().context("官方包缺少完整性校验")?;
    let digest = integrity.strip_prefix("sha512-").context("官方包缺少 SHA512 校验")?;
    if base64::engine::general_purpose::STANDARD.decode(digest)?.len() != 64 { bail!("官方完整性校验值无效"); }
    Ok(Release { version: version.into(), url: expected, integrity: integrity.into(), archive: ArchiveFormat::TarGz })
}
fn github_spec(kind: AgentRuntimeKind, platform: &str) -> Result<(&'static str, String)> {
    let arch = match platform { "win32-x64" => "x64", "win32-arm64" => "arm64", _ => bail!("此平台没有原生发行包") };
    match kind {
        AgentRuntimeKind::KimiCode => Ok(("MoonshotAI/kimi-code", format!("kimi-code-win32-{arch}.tar.gz"))),
        AgentRuntimeKind::Pi => Ok(("earendil-works/pi", format!("pi-windows-{arch}.zip"))),
        _ => bail!("此 Runtime 不使用此发行渠道"),
    }
}
fn parse_github_release(value: &serde_json::Value, kind: AgentRuntimeKind, platform: &str) -> Result<Release> {
    let (repo, name) = github_spec(kind, platform)?;
    if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) { bail!("官方发行不是稳定版本"); }
    let tag = value["tag_name"].as_str().context("官方发行缺少版本标记")?;
    let prefix = if kind == AgentRuntimeKind::KimiCode { "@moonshot-ai/kimi-code@" } else { "v" };
    let version = tag.strip_prefix(prefix).filter(|version| valid_version(version)).context("官方发行版本标记无效")?;
    let asset = value["assets"].as_array().and_then(|assets| assets.iter().find(|asset| asset["name"] == name)).context("官方发行缺少此平台安装包")?;
    let url = asset["browser_download_url"].as_str().context("官方发行缺少下载地址")?;
    let parsed = reqwest::Url::parse(url)?;
    let expected_prefix = format!("/{repo}/releases/download/");
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") || !parsed.username().is_empty() || parsed.password().is_some()
        || parsed.port().is_some() || parsed.query().is_some() || parsed.fragment().is_some()
        || !parsed.path().starts_with(&expected_prefix) || !parsed.path().ends_with(&format!("/{name}")) {
        bail!("下载地址不属于预期官方仓库");
    }
    let digest = asset["digest"].as_str().and_then(|value| value.strip_prefix("sha256:")).filter(|value| value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())).context("官方发行缺少有效 SHA256，不执行安装")?;
    Ok(Release { version: version.into(), url: url.into(), integrity: format!("sha256-{}", digest.to_ascii_lowercase()), archive: if kind == AgentRuntimeKind::Pi { ArchiveFormat::Zip } else { ArchiveFormat::TarGz } })
}

pub(super) fn dependencies(kind: AgentRuntimeKind) -> Result<String> {
    platform()?;
    #[cfg(windows)]
    if kind == AgentRuntimeKind::KimiCode {
        let configured = std::env::var_os("KIMI_SHELL_PATH").map(PathBuf::from);
        let bash = if let Some(path) = configured {
            if !path.is_absolute() || !path.is_file() { bail!("KIMI_SHELL_PATH 无效，请指定 Git Bash 的绝对路径"); }
            Some(path)
        } else { crate::multi_agent::git_bash_executable() };
        if bash.is_none() { bail!("Kimi Code 需要 Git for Windows / Git Bash；安装后重新检查"); }
        return Ok("已检测到 Git Bash，不需要 Node/npm".into());
    }
    let _ = kind;
    Ok("当前平台支持，不需要 Node/npm".into())
}

pub(super) fn verify_help(kind: AgentRuntimeKind, help: &str) -> Result<()> {
    let lower = help.to_ascii_lowercase();
    let valid = match kind {
        AgentRuntimeKind::ClaudeCode => lower.contains("claude"),
        AgentRuntimeKind::Codex => lower.contains("codex"),
        AgentRuntimeKind::OpenCode => lower.contains("opencode") && lower.contains("run") && lower.contains("acp"),
        AgentRuntimeKind::KimiCode => lower.contains("kimi") && lower.contains("--output-format") && lower.contains("--prompt") && lower.contains("acp"),
        AgentRuntimeKind::Pi => lower.contains("pi") && lower.contains("--mode") && lower.contains("rpc"),
        AgentRuntimeKind::Antigravity => lower.contains("agy") && lower.contains("--print") && lower.split_whitespace().any(|word| word == "acp"),
        _ => false,
    };
    if !valid { bail!("CLI 帮助与现有 Runtime 协议不匹配，原安装已保留"); }
    Ok(())
}

pub(super) fn login_script(kind: AgentRuntimeKind, quoted_path: &str) -> Result<String, String> {
    let args = match kind {
        AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::OpenCode => "auth login",
        AgentRuntimeKind::Codex | AgentRuntimeKind::KimiCode => "login",
        AgentRuntimeKind::Pi | AgentRuntimeKind::Antigravity => "",
        _ => return Err("不支持此 CLI 的登录引导".into()),
    };
    let hint = if kind == AgentRuntimeKind::Pi { "Write-Host '进入 Pi 后输入 /login，选择需要登录的 Provider。'\n" } else { "" };
    Ok(format!("{hint}& '{quoted_path}' {args}"))
}

pub(super) fn extract_zip(archive: &Path, directory: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    if zip.len() > 4096 { bail!("安装包文件数超出限制"); }
    let mut extracted = 0u64;
    for index in 0..zip.len() {
        check_cancel(cancel)?;
        let mut entry = zip.by_index(index)?;
        let relative = PathBuf::from(entry.name());
        if relative.as_os_str().is_empty() || relative.components().any(|part| !matches!(part, Component::Normal(_))) || entry.name().contains([':', '\\'])
            || entry.unix_mode().is_some_and(|mode| { let kind = mode & 0o170000; kind != 0 && kind != 0o100000 && kind != 0o040000 }) {
            bail!("安装包含不安全路径或非普通文件");
        }
        let path = directory.join(&relative);
        if entry.is_dir() { fs::create_dir_all(&path)?; continue; }
        extracted = extracted.checked_add(entry.size()).context("安装包大小溢出")?;
        if extracted > MAX_EXTRACTED { bail!("安装包解压大小超出限制"); }
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        let mut output = fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
        let mut buffer = vec![0; 1024 * 1024];
        loop { check_cancel(cancel)?; let count = entry.read(&mut buffer)?; if count == 0 { break; } output.write_all(&buffer[..count])?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
