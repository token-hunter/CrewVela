use super::*;
use crate::{
    acp::{self, ManagedAcpKind},
    runtime_dependencies as deps,
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
    sync::mpsc,
    time::Instant,
};

use AgentRuntimeKind::*;
const CHATS: [AgentRuntimeKind; 7] = [
    ClaudeChat,
    CodexChat,
    KimiChat,
    OpenCodeChat,
    PiChat,
    DeepSeekHarness,
    ZCode,
];
static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatEnvironment {
    node_required: bool,
    node_version: Option<String>,
    adapter_required: bool,
    adapter_installed: bool,
    cli_label: &'static str,
    verified: bool,
    can_install: bool,
    can_login: bool,
    official_url: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatDependencySummary {
    label: &'static str,
    needs_setup: bool,
}

pub(super) fn supports(kind: AgentRuntimeKind) -> bool {
    CHATS.contains(&kind)
}

pub(super) fn cli_kind(kind: AgentRuntimeKind) -> Option<AgentRuntimeKind> {
    match kind {
        ClaudeChat => Some(ClaudeCode),
        CodexChat => Some(Codex),
        KimiChat => Some(KimiCode),
        OpenCodeChat => Some(OpenCode),
        PiChat => Some(Pi),
        _ => None,
    }
}

fn adapter_kind(kind: AgentRuntimeKind) -> Option<ManagedAcpKind> {
    match kind {
        ClaudeChat => Some(ManagedAcpKind::ClaudeCode),
        CodexChat => Some(ManagedAcpKind::Codex),
        _ => None,
    }
}

fn cli_path(kind: AgentRuntimeKind) -> Result<PathBuf> {
    match kind {
        DeepSeekHarness => acp::deepseek_entry(),
        ZCode => crate::multi_agent::zcode::executable(),
        _ => {
            crate::multi_agent::resolve_runtime_executable_checked(kind).map_err(anyhow::Error::msg)
        }
    }
}

fn launch_path_needs_node(kind: AgentRuntimeKind, path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if kind == DeepSeekHarness && extension == "ts" {
        return true;
    }
    if kind == ZCode && matches!(extension, "js" | "cjs") {
        return !path.ancestors().nth(3).is_some_and(|root| {
            root.join("ZCode.exe").is_file()
                || root
                    .parent()
                    .is_some_and(|root| root.join("MacOS/ZCode").is_file())
        });
    }
    // npm Windows shims contain an explicit node invocation. A desktop shim
    // that runs an embedded engine and a standalone CLI are different paths.
    matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat" | "")
        && fs::File::open(path)
            .ok()
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(8192).read_to_end(&mut bytes).ok()?;
                Some(String::from_utf8_lossy(&bytes).to_ascii_lowercase())
            })
            .is_some_and(|script| {
                script.contains("node.exe")
                    || script.lines().next().is_some_and(|line| {
                        line.starts_with("#!")
                            && line
                                .split_whitespace()
                                .any(|word| word == "node" || word.ends_with("/node"))
                    })
            })
}

fn needs_node(kind: AgentRuntimeKind, path: Option<&Path>) -> bool {
    adapter_kind(kind).is_some()
        || path.is_some_and(|path| launch_path_needs_node(kind, path))
        || (kind == DeepSeekHarness
            && std::env::var_os("CREWVELA_DSH_ROOT").is_some_and(|root| !root.is_empty()))
}

fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn node_version(path: &Path) -> Option<String> {
    let mut command = Command::new(path);
    hidden(&mut command)
        .env_remove("NODE_OPTIONS")
        .arg("--version");
    let output = crate::title_summary::run_cli_with_timeout(
        command,
        None,
        Duration::from_secs(3),
        4096,
        "Node.js 自检",
    )
    .ok()?;
    let version = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .strip_prefix('v')?
        .to_owned();
    (output.status.success()
        && valid_version(&version)
        && version.split('.').next()?.parse::<u32>().ok()? >= 22)
        .then_some(version)
}

fn usable_node() -> Option<(PathBuf, String)> {
    let active = deps::node_executable().ok()?;
    let version = node_version(&active)?;
    Some((active, version))
}

pub(super) fn summaries() -> HashMap<String, ChatDependencySummary> {
    let mut node = None;
    CHATS
        .into_iter()
        .map(|kind| {
            let path = cli_path(kind).ok();
            let required = needs_node(kind, path.as_deref());
            let has_node = !required || *node.get_or_insert_with(|| usable_node().is_some());
            let missing_adapter =
                adapter_kind(kind).is_some_and(|adapter| !acp::chat_adapter_installed(adapter));
            let subtitle = match (path.is_some(), has_node, missing_adapter) {
                (false, false, _) => "缺少运行程序和 Node.js",
                (false, true, _) => "未检测到运行程序",
                (true, false, _) if kind == PiChat => "npm 版 · 缺少兼容的 Node.js",
                (true, false, _) => "缺少兼容的 Node.js",
                (true, true, true) => "缺少聊天适配器",
                (true, true, false) if matches!(kind, DeepSeekHarness | ZCode) && !required => {
                    "使用内置运行环境"
                }
                _ => "已检测到启动依赖",
            };
            (
                kind.as_str().into(),
                ChatDependencySummary {
                    label: subtitle,
                    needs_setup: path.is_none() || !has_node || missing_adapter,
                },
            )
        })
        .collect()
}

fn official(kind: AgentRuntimeKind) -> Option<&'static str> {
    match kind {
        DeepSeekHarness => Some("https://www.deepseek.com/harness/en/"),
        ZCode => Some("https://zcode.z.ai/cn/docs/install"),
        _ => None,
    }
}

pub(super) fn environment(kind: AgentRuntimeKind) -> Result<Environment> {
    let path = cli_path(kind);
    let required = needs_node(kind, path.as_ref().ok().map(PathBuf::as_path));
    let node = required.then(usable_node).flatten();
    let adapter = adapter_kind(kind);
    let adapter_installed = adapter.is_some_and(acp::chat_adapter_installed);
    let ready_to_probe =
        path.is_ok() && (!required || node.is_some()) && (adapter.is_none() || adapter_installed);
    let verification = ready_to_probe.then(|| verify_chat(kind, None)).transpose();
    let verified = matches!(verification, Ok(Some(())));
    let dependency_error = cli_kind(kind)
        .and_then(|cli| additional::dependencies(cli).err())
        .filter(|_| !verified && platform().is_ok());
    let cli_install_allowed = dependency_error.is_none();
    let error = match (verification, dependency_error) {
        (Err(error), _) | (_, Some(error)) => Some(error.to_string()),
        _ => path.as_ref().err().map(ToString::to_string),
    };
    let executable = path.ok();
    let auth = if verified {
        executable
            .as_deref()
            .zip(cli_kind(kind))
            .map(|(path, cli)| auth_status(path, cli))
            .unwrap_or_else(|| "unverified".into())
    } else {
        "unverified".into()
    };
    let version = if verified {
        executable
            .as_deref()
            .filter(|_| cli_kind(kind).is_some())
            .and_then(|path| {
                let mut command = crate::multi_agent::command_for_path(path);
                deps::with_managed_node(&mut command);
                hidden(&mut command).arg("--version");
                let output = crate::title_summary::run_cli_with_timeout(
                    command,
                    None,
                    Duration::from_secs(4),
                    16384,
                    "CLI 版本检查",
                )
                .ok()?;
                output
                    .status
                    .success()
                    .then(|| crate::multi_agent::version_number(&cli_text(&output)))
                    .flatten()
            })
    } else {
        None
    };
    let can_install = platform().is_ok()
        && (executable.is_some() || cli_kind(kind).is_some() && cli_install_allowed)
        && (required && node.is_none()
            || adapter.is_some() && !verified
            || executable.is_none() && cli_kind(kind).is_some());
    let source = match executable.as_deref() {
        Some(path) if launch_path_needs_node(kind, path) => "使用本机 Node.js 的运行入口",
        Some(_) => "本机已安装的运行程序",
        None => "未检测到运行程序",
    };
    let cli_label = match kind {
        ClaudeChat => "Claude Code CLI",
        CodexChat => "Codex CLI",
        KimiChat => "Kimi Code CLI",
        OpenCodeChat => "OpenCode CLI",
        PiChat => "Pi CLI",
        DeepSeekHarness => "DeepSeek Harness",
        _ => "ZCode Desktop",
    };
    let executable = executable.map(|path| path.display().to_string());
    Ok(Environment {
        kind,
        supported: can_install,
        executable,
        version,
        latest_version: None,
        source: source.into(),
        install_directory: deps::chat_install_root()?.display().to_string(),
        auth,
        error,
        update_error: None,
        dependency_message: String::new(),
        chat: Some(ChatEnvironment {
            node_required: required,
            node_version: node.map(|(_, version)| version),
            adapter_required: adapter.is_some(),
            adapter_installed,
            cli_label,
            verified,
            can_install,
            can_login: platform().is_ok() && cli_kind(kind).is_some() && verified,
            official_url: official(kind),
        }),
    })
}

fn node_zip(manifest: &str, platform: &str) -> Result<Release> {
    let suffix = match platform {
        "win32-x64" => "-win-x64.zip",
        "win32-arm64" => "-win-arm64.zip",
        _ => bail!("此平台没有对应的 Node.js ZIP"),
    };
    let mut matches = manifest.lines().filter_map(|line| {
        let (digest, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        let version = file.strip_prefix("node-v")?.strip_suffix(suffix)?;
        if !version.starts_with("24.")
            || !valid_version(version)
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        Some(Release {
            version: version.into(),
            url: format!("https://nodejs.org/dist/v{version}/{file}"),
            integrity: format!("sha256-{}", digest.to_ascii_lowercase()),
            archive: ArchiveFormat::Zip,
        })
    });
    let release = matches
        .next()
        .context("Node.js 24 LTS 官方校验清单缺少此平台的 ZIP")?;
    if matches.next().is_some() {
        bail!("Node.js 24 LTS 校验清单存在重复条目");
    }
    Ok(release)
}

fn component_staging(root: &Path) -> Result<Staging> {
    fs::create_dir_all(root)?;
    let stage = Staging(root.join(format!(".staging-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&stage.0)?;
    Ok(stage)
}

fn publish_component(
    root: &Path,
    prepared: &Path,
    id: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let version = uuid::Uuid::new_v4().to_string();
    let versions = root.join("versions");
    fs::create_dir_all(&versions)?;
    let destination = versions.join(&version);
    rename_for_publication(prepared, &destination)?;
    let _state = jobs().lock().unwrap();
    let published = check_cancel(cancel)
        .and_then(|()| crate::providers::write_atomic(&root.join("current"), version.as_bytes()));
    if let Err(error) = published {
        drop(_state);
        let _ = fs::remove_dir_all(&destination);
        return Err(error).with_context(|| format!("未切换安装入口（{id}）"));
    }
    Ok(destination)
}

fn npm_entry(node: &Path) -> Option<PathBuf> {
    let entry = node.parent()?.join("node_modules/npm/bin/npm-cli.js");
    entry.is_file().then_some(entry)
}

async fn install_node(
    root: &Path,
    id: &str,
    cancel: &Arc<AtomicBool>,
    wake: &tokio::sync::Notify,
) -> Result<PathBuf> {
    let (platform, _) = platform()?;
    let manifest = tokio::select! {
        result = async { text(&client()?, "https://nodejs.org/dist/latest-v24.x/SHASUMS256.txt").await } => result?,
        _ = wake.notified() => bail!("安装已取消"),
    };
    let release = node_zip(&manifest, platform)?;
    let stage = component_staging(root)?;
    let archive = stage.0.join("node.zip");
    download(&release, &archive, id, cancel, wake).await?;
    change(id, "installing", "正在解压 Node.js 24 LTS…", 0, None);
    let folder = stage.0.clone();
    let expected = release.version;
    let version_folder = format!(
        "node-v{expected}-{}",
        if platform == "win32-x64" {
            "win-x64"
        } else {
            "win-arm64"
        }
    );
    let flag = cancel.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf> {
        additional::extract_zip(&archive, &folder, &flag)?;
        let prepared = folder.join(version_folder);
        let node = prepared.join("node.exe");
        check_cancel(&flag)?;
        if node_version(&node).as_deref() != Some(&expected) || npm_entry(&node).is_none() {
            bail!("Node.js 和 npm 自检失败");
        }
        Ok(prepared)
    })
    .await??;
    Ok(publish_component(root, &prepared, id, cancel)?.join("node.exe"))
}

pub(super) async fn install(
    base: &Path,
    kind: AgentRuntimeKind,
    id: &str,
    cancel: &Arc<AtomicBool>,
    wake: &Arc<tokio::sync::Notify>,
) -> Result<(String, PathBuf)> {
    let _guard = tokio::select! { lock = INSTALL.lock() => lock, _ = wake.notified() => bail!("安装已取消") };
    check_cancel(cancel)?;
    if cli_path(kind).is_err() {
        let cli = cli_kind(kind).context("请先按官方指引安装此运行程序")?;
        additional::dependencies(cli)?;
        change(id, "installing", "正在安装缺少的 CLI…", 0, None);
        super::install_cli(base, cli, id, cancel, wake, true).await?;
    }
    let path = cli_path(kind)?;
    let adapter = adapter_kind(kind);
    let root = deps::chat_install_root()?;
    if needs_node(kind, Some(&path)) {
        let node =
            if let Some((node, _)) = tauri::async_runtime::spawn_blocking(usable_node).await? {
                node
            } else {
                install_node(&root.join("nodejs"), id, cancel, wake).await?
            };
        if let Some(adapter) = adapter {
            let flag = cancel.clone();
            let existing = acp::chat_adapter_installed(adapter);
            let valid = if existing {
                tauri::async_runtime::spawn_blocking(move || verify_chat(kind, Some(&flag)).is_ok())
                    .await?
            } else {
                false
            };
            check_cancel(cancel)?;
            if !valid {
                let node = if npm_entry(&node).is_some() {
                    node
                } else {
                    install_node(&root.join("nodejs"), id, cancel, wake).await?
                };
                change(id, "installing", "正在安装聊天适配器…", 0, None);
                let flag = cancel.clone();
                let id = id.to_owned();
                tauri::async_runtime::spawn_blocking(move || {
                    install_adapter(&root, adapter, &node, &id, &flag)
                })
                .await??;
            }
        }
    }
    change(id, "verifying", "正在验证 Chat 启动协议…", 0, None);
    let flag = cancel.clone();
    tauri::async_runtime::spawn_blocking(move || verify_chat(kind, Some(&flag))).await??;
    check_cancel(cancel)?;
    Ok((String::new(), path))
}

fn install_adapter(
    root: &Path,
    adapter: ManagedAcpKind,
    node: &Path,
    id: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    let package = adapter.package().context("该 ACP 使用本机 CLI，无 npm 适配器")?;
    let (name, version) = package
        .rsplit_once('@')
        .context("适配器版本配置无效")?;
    let root = root.join("adapters").join(adapter.binary()).join(version);
    let stage = component_staging(&root)?;
    let prefix = stage.0.join("prepared");
    fs::create_dir(&prefix)?;
    let user_config = stage.0.join("user.npmrc");
    fs::write(&user_config, [])?;
    let global_config = stage.0.join("global.npmrc");
    fs::write(&global_config, [])?;
    let mut command = Command::new(node);
    hidden(&mut command)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .current_dir(&prefix);
    for (key, _) in std::env::vars_os().filter(|(key, _)| {
        key.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("npm_config_")
    }) {
        command.env_remove(key);
    }
    command
        .arg(npm_entry(node).context("未找到 npm")?)
        .arg("--userconfig")
        .arg(&user_config)
        .arg("--globalconfig")
        .arg(&global_config)
        .args([
            "install",
            "--registry=https://registry.npmjs.org",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--no-package-lock",
            "--include=optional",
        ])
        .arg(package);
    let output = crate::title_summary::run_cli_until_cancelled(
        command,
        None,
        Duration::from_secs(120),
        128 * 1024,
        "聊天适配器安装",
        Some(cancel),
    )?;
    if !output.status.success() {
        bail!(
            "聊天适配器安装失败（{}），请检查 npm 官方仓库的网络连接后重试",
            output.status
        );
    }
    let package = prefix.join("node_modules").join(name);
    let metadata: Value = serde_json::from_slice(&fs::read(package.join("package.json"))?)?;
    if metadata["name"] != name || metadata["version"] != version {
        bail!("适配器包身份不符");
    }
    let entry = package.join("dist/index.js");
    probe(
        acp::chat_adapter_probe_command(adapter, node, &entry)?,
        Protocol::Acp,
        Some(cancel),
    )?;
    publish_component(&root, &prefix, id, cancel)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Protocol {
    Acp,
    Pi,
    ZCode,
}

fn verify_chat(kind: AgentRuntimeKind, cancel: Option<&AtomicBool>) -> Result<()> {
    let (command, protocol) = match kind {
        ClaudeChat => (
            acp::adapter_command(ManagedAcpKind::ClaudeCode)?,
            Protocol::Acp,
        ),
        CodexChat => (acp::adapter_command(ManagedAcpKind::Codex)?, Protocol::Acp),
        KimiChat => (acp::kimi_acp_command()?, Protocol::Acp),
        OpenCodeChat => (acp::opencode_acp_command()?, Protocol::Acp),
        DeepSeekHarness => (
            acp::adapter_command(ManagedAcpKind::DeepSeekHarness)?,
            Protocol::Acp,
        ),
        ZCode => (
            crate::multi_agent::zcode::launch(&cli_path(kind)?)?,
            Protocol::ZCode,
        ),
        PiChat => {
            let mut command = crate::multi_agent::command_for_path(&cli_path(kind)?);
            deps::with_managed_node(&mut command);
            command.args(["--mode", "rpc", "--no-session", "--no-extensions"]);
            (command, Protocol::Pi)
        }
        _ => bail!("此 Runtime 不是 Chat"),
    };
    probe(command, protocol, cancel)
}

fn probe(mut command: Command, protocol: Protocol, cancel: Option<&AtomicBool>) -> Result<()> {
    let request = match protocol {
        Protocol::Acp => {
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"crewvela","version":env!("CARGO_PKG_VERSION")}}})
        }
        Protocol::Pi => json!({"id":"crewvela-check","type":"get_state"}),
        Protocol::ZCode => {
            json!({"id":1,"method":"v4/commands/query","params":{"commands":[{"sessionId":null,"commandId":uuid::Uuid::new_v4().to_string()}],"clock":true}})
        }
    };
    hidden(&mut command)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut child, _identity) =
        acp::spawn_owned_command(&mut command).context("无法启动 Chat 自检进程")?;
    let result = (|| -> Result<()> {
        let mut stdin = child.stdin.take().context("无法写入 Chat 自检请求")?;
        let stdout = child.stdout.take().context("无法读取 Chat 自检响应")?;
        serde_json::to_writer(&mut stdin, &request)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        let expected = request["id"].clone();
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut bytes = Vec::new();
            for _ in 0..256 {
                bytes.clear();
                let Ok(count) = (&mut reader)
                    .take(1024 * 1024)
                    .read_until(b'\n', &mut bytes)
                else {
                    break;
                };
                if count == 0 || count >= 1024 * 1024 {
                    break;
                }
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    if value["id"] == expected && value.get("method").is_none() {
                        let ok = match protocol {
                            Protocol::Acp => value["result"]["protocolVersion"].as_u64() == Some(1),
                            Protocol::Pi => value["success"].as_bool() == Some(true),
                            Protocol::ZCode => value["result"]["results"].is_array(),
                        };
                        let _ = send.send(ok);
                        return;
                    }
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(18);
        loop {
            if let Some(cancel) = cancel {
                check_cancel(cancel)?;
            }
            match receive.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => return Ok(()),
                Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("Chat 未返回有效的启动协议响应，请检查运行程序版本和安装状态")
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    bail!("Chat 启动自检超时")
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    acp::stop_child_checked(&mut child)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "checks installed Chat protocols without sending prompts or signing in"]
    fn installed_chat_protocols_answer_without_model_requests() {
        assert_eq!(std::env::var("CREWVELA_RUN_CHAT_PROBE").as_deref(), Ok("1"));
        let mut failed = Vec::new();
        for kind in CHATS {
            match verify_chat(kind, None) {
                Ok(()) => eprintln!("Chat startup passed: {}", kind.as_str()),
                Err(error) => failed.push(format!("{}: {error:#}", kind.as_str())),
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "downloads Node and pinned adapters into isolated storage; no login or model calls"]
    fn official_shared_node_and_adapters_install_and_verify_in_isolation() {
        let root =
            std::env::temp_dir().join(format!("crewvela-chat-installer-{}", uuid::Uuid::new_v4()));
        let _cleanup = Staging(root.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = tokio::sync::Notify::new();
        let node = tauri::async_runtime::block_on(install_node(
            &root.join("nodejs"),
            "isolated",
            &cancel,
            &wake,
        ))
        .unwrap();
        assert!(node_version(&node).unwrap().starts_with("24."));
        let active = deps::active_component(&root.join("nodejs"), Path::new("node.exe")).unwrap();
        assert_eq!(node, active);
        for adapter in [ManagedAcpKind::ClaudeCode, ManagedAcpKind::Codex] {
            install_adapter(&root, adapter, &active, "isolated", &cancel)
                .unwrap_or_else(|error| panic!("{} installation: {error:#}", adapter.binary()));
        }
    }

    #[test]
    fn official_node_zip_is_pinned_to_the_approved_major_architecture_and_digest() {
        let hash = "a".repeat(64);
        let manifest = format!("{hash}  node-v24.12.0-win-x64.zip\n{hash}  node-v24.12.0-win-arm64.zip\n{hash}  node-v26.1.0-win-x64.zip");
        let release = node_zip(&manifest, "win32-arm64").unwrap();
        assert_eq!(
            release.url,
            "https://nodejs.org/dist/v24.12.0/node-v24.12.0-win-arm64.zip"
        );
        assert_eq!(release.integrity, format!("sha256-{hash}"));
        assert!(node_zip(&format!("{hash}  node-v26.1.0-win-x64.zip"), "win32-x64").is_err());
        assert!(node_zip("not-a-digest node-v24.12.0-win-x64.zip", "win32-x64").is_err());
        assert!(node_zip(&format!("{manifest}\n{manifest}"), "win32-x64").is_err());
    }

    #[test]
    fn native_chat_detects_node_for_npm_shim_and_source_but_not_desktop_or_native_exe() {
        let root =
            std::env::temp_dir().join(format!("crewvela-chat-deps-{}", uuid::Uuid::new_v4()));
        let bin = root.join("resources/glm");
        fs::create_dir_all(&bin).unwrap();
        let shim = root.join("pi.cmd");
        fs::write(&shim, b"IF EXIST node.exe SET prog=node").unwrap();
        assert!(needs_node(PiChat, Some(&shim)));
        assert!(!needs_node(PiChat, Some(Path::new("pi.exe"))));
        assert!(launch_path_needs_node(DeepSeekHarness, Path::new("bin.ts")));
        assert!(needs_node(ZCode, Some(&bin.join("zcode.cjs"))));
        fs::write(root.join("ZCode.exe"), []).unwrap();
        assert!(!needs_node(ZCode, Some(&bin.join("zcode.cjs"))));
        assert!(needs_node(ClaudeChat, None));
        assert!(needs_node(CodexChat, None));
        fs::remove_dir_all(root).unwrap();
    }
}
