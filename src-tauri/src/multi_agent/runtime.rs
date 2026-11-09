use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use super::AgentRuntimeKind;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProbe {
    pub kind: AgentRuntimeKind,
    pub installed: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub protocol: String,
    pub capabilities: Vec<String>,
    pub error: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub update_status: String,
    pub update_command: String,
    pub can_auto_update: bool,
    pub update_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpgradeReceipt {
    pub kind: AgentRuntimeKind,
    pub output: String,
    pub runtime: RuntimeProbe,
}

struct UpdateSpec {
    package: Option<&'static str>,
    args: &'static [&'static str],
    display: &'static str,
    automatic: bool,
}

struct RuntimeDefinition {
    kind: AgentRuntimeKind,
    command: &'static str,
    protocol: &'static str,
    capabilities: &'static [&'static str],
}

const DEFINITIONS: [RuntimeDefinition; 8] = [
    RuntimeDefinition {
        kind: AgentRuntimeKind::ZCode,
        command: "zcode",
        protocol: "zcode_v4_stdio",
        capabilities: &["resume", "interrupt", "approvals", "mcp"],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::ZCodeCli,
        command: "zcode",
        protocol: "zcode_v4_stdio",
        capabilities: &["resume", "interrupt", "approvals", "mcp"],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::DeepSeekHarness,
        command: "dsh",
        protocol: "acp_v1",
        capabilities: &["resume", "interrupt", "approvals", "mcp", "acp"],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::ClaudeCode,
        command: "claude",
        protocol: "acp_v1+pty_fallback",
        capabilities: &["resume", "interrupt", "permission_modes", "mcp", "acp"],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::Codex,
        command: "codex",
        protocol: "acp_v1+pty_fallback",
        capabilities: &[
            "resume",
            "interrupt",
            "approvals",
            "usage",
            "diff",
            "mcp",
            "acp",
        ],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::KimiCode,
        command: "kimi",
        protocol: "cli_stream_json",
        capabilities: &[
            "resume",
            "interrupt",
            "prompt_auto",
            "commands",
            "usage",
            "mcp",
            "subagents",
        ],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::Pi,
        command: "pi",
        protocol: "cli_jsonl_session",
        capabilities: &["resume", "interrupt", "commands", "usage"],
    },
    RuntimeDefinition {
        kind: AgentRuntimeKind::OpenCode,
        command: "opencode",
        protocol: "cli_jsonl",
        capabilities: &[
            "resume",
            "interrupt",
            "approvals",
            "commands",
            "diff",
            "mcp",
        ],
    },
];

pub(crate) fn capabilities_for_runtime(kind: AgentRuntimeKind) -> Vec<String> {
    if kind.is_retired() {
        return Vec::new();
    }
    let mut capabilities = DEFINITIONS
        .iter()
        .find(|definition| definition.kind == kind)
        .map(|definition| {
            definition
                .capabilities
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    capabilities.extend(["filesystem".to_string(), "shell".to_string()]);
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

pub fn probe_all_runtimes() -> Vec<RuntimeProbe> {
    DEFINITIONS.iter().map(probe_runtime).collect()
}

pub fn check_runtime_updates() -> Vec<RuntimeProbe> {
    probe_all_runtimes()
        .into_iter()
        .map(|mut probe| {
            if !probe.installed {
                return probe;
            }
            let spec = update_spec(probe.kind);
            let result = {
                spec.package
                    .ok_or_else(|| "该 Runtime 不支持在线版本查询".to_string())
                    .and_then(npm_latest_version)
                    .map(|latest| {
                        let available = probe
                            .version
                            .as_deref()
                            .and_then(version_number)
                            .is_some_and(|current| version_is_newer(&latest, &current));
                        (Some(latest), available)
                    })
            };
            match result {
                Ok((latest, available)) => {
                    probe.latest_version = latest;
                    probe.update_available = available;
                    probe.update_status =
                        if available { "available" } else { "up_to_date" }.to_string();
                }
                Err(error) => {
                    probe.update_status = "error".to_string();
                    probe.update_error = Some(error);
                }
            }
            probe
        })
        .collect()
}

pub fn upgrade_runtime(kind: AgentRuntimeKind) -> Result<RuntimeUpgradeReceipt, String> {
    kind.ensure_available().map_err(|error| error.to_string())?;
    let definition = DEFINITIONS
        .iter()
        .find(|item| item.kind == kind)
        .ok_or_else(|| "未知 Runtime".to_string())?;
    let mut probe = probe_runtime(definition);
    if !probe.installed {
        return Err(probe
            .error
            .take()
            .unwrap_or_else(|| "Runtime 未安装".to_string()));
    }
    let spec = update_spec(kind);
    if !spec.automatic {
        return Err(format!("该安装方式需要手动升级：{}", spec.display));
    }
    let executable = probe
        .executable
        .as_ref()
        .ok_or_else(|| "Runtime 路径不可用".to_string())?;
    let output = run_command(std::path::Path::new(executable), spec.args, 300)?;
    Ok(RuntimeUpgradeReceipt {
        kind,
        output,
        runtime: probe_runtime(definition),
    })
}

fn probe_runtime(definition: &RuntimeDefinition) -> RuntimeProbe {
    let (executable, resolution_error) = if definition.kind == AgentRuntimeKind::Codex {
        match resolve_codex_definition(definition) {
            Ok(resolved) => (Some(resolved), None),
            Err(error) => (None, Some(error)),
        }
    } else { (resolve_definition_executable(definition), None) };
    let Some((executable, validated_version)) = executable else {
        return RuntimeProbe {
            kind: definition.kind,
            installed: false,
            executable: None,
            version: None,
            protocol: definition.protocol.to_string(),
            capabilities: capabilities_for_runtime(definition.kind),
            error: Some(if let Some(error) = resolution_error { error }
            else if definition.kind == AgentRuntimeKind::ZCode {
                super::zcode::executable()
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "ZCode Desktop Agent 启动入口不可用".into())
            } else if definition.kind == AgentRuntimeKind::ZCodeCli {
                super::zcode::cli_executable()
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "zcode CLI 启动入口不可用".into())
            } else if definition.kind == AgentRuntimeKind::DeepSeekHarness {
                crate::acp::deepseek_executable()
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "DeepSeek Harness 启动入口不可用".into())
            } else {
                format!("未找到 {} 命令", definition.command)
            }),
            latest_version: None,
            update_available: false,
            update_status: "unchecked".to_string(),
            update_command: update_spec(definition.kind).display.to_string(),
            can_auto_update: update_spec(definition.kind).automatic,
            update_error: None,
        };
    };

    let (version, error) = match validated_version {
        Some(version) => (Some(version), None),
        None if matches!(
            definition.kind,
            AgentRuntimeKind::DeepSeekHarness
                | AgentRuntimeKind::ZCode
                | AgentRuntimeKind::ZCodeCli
        ) =>
        {
            (None, None)
        }
        None => read_version(&executable),
    };
    RuntimeProbe {
        kind: definition.kind,
        installed: true,
        executable: Some(executable.display().to_string()),
        version,
        protocol: definition.protocol.to_string(),
        capabilities: capabilities_for_runtime(definition.kind),
        error,
        latest_version: None,
        update_available: false,
        update_status: "unchecked".to_string(),
        update_command: update_spec(definition.kind).display.to_string(),
        can_auto_update: update_spec(definition.kind).automatic,
        update_error: None,
    }
}

fn update_spec(kind: AgentRuntimeKind) -> UpdateSpec {
    match kind {
        AgentRuntimeKind::ZCode => UpdateSpec {
            package: None,
            args: &[],
            display: "通过 ZCode Desktop 或原安装方式手动升级",
            automatic: false,
        },
        AgentRuntimeKind::ZCodeCli => UpdateSpec {
            package: None,
            args: &[],
            display: "通过 zcode CLI 官方渠道或原安装方式手动升级",
            automatic: false,
        },
        AgentRuntimeKind::ClaudeChat => UpdateSpec {
            package: None,
            args: &[],
            display: "Claude Code Chat 适配器随 crewvela 内置，随应用更新",
            automatic: false,
        },
        AgentRuntimeKind::KimiChat => UpdateSpec {
            package: None,
            args: &[],
            display: "直连本机 kimi CLI（原生 ACP），通过 kimi upgrade 手动升级",
            automatic: false,
        },
        AgentRuntimeKind::PiChat => UpdateSpec {
            package: None,
            args: &[],
            display: "直连本机 pi CLI（pi --mode rpc 原生聊天），通过 pi update --self 手动升级",
            automatic: false,
        },
        AgentRuntimeKind::CodexChat => UpdateSpec {
            package: None,
            args: &[],
            display: "codex-acp 适配器随 crewvela 内置并驱动本机 codex CLI，通过 codex 自身方式手动升级",
            automatic: false,
        },
        AgentRuntimeKind::OpenCodeChat => UpdateSpec {
            package: None,
            args: &[],
            display: "直连本机 opencode CLI（原生 ACP），通过 opencode upgrade 手动升级",
            automatic: false,
        },
        AgentRuntimeKind::ClaudeCode => UpdateSpec {
            package: Some("@anthropic-ai/claude-code"),
            args: &["update"],
            display: "claude update",
            automatic: true,
        },
        AgentRuntimeKind::Codex => UpdateSpec {
            package: Some("@openai/codex"),
            args: &["update"],
            display: "codex update",
            automatic: true,
        },
        AgentRuntimeKind::KimiCode => UpdateSpec {
            package: Some("@moonshot-ai/kimi-code"),
            args: &["upgrade"],
            display: "kimi upgrade",
            automatic: false,
        },
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => unreachable!("已移除的 CLI 不支持更新"),
        AgentRuntimeKind::Pi => UpdateSpec {
            package: Some("@earendil-works/pi-coding-agent"),
            args: &["update", "--self", "--approve"],
            display: "pi update --self",
            automatic: true,
        },
        AgentRuntimeKind::OhMyPi => unreachable!("已移除的 Runtime 不在可用清单中"),
        AgentRuntimeKind::OpenCode => UpdateSpec {
            package: Some("opencode-ai"),
            args: &["upgrade"],
            display: "opencode upgrade",
            automatic: true,
        },
        AgentRuntimeKind::DeepSeekHarness => UpdateSpec {
            package: None,
            args: &[],
            display: "按 DeepSeek Harness 原安装方式手动升级",
            automatic: false,
        },
    }
}

fn npm_latest_version(package: &str) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/c", "npm.cmd", "view", package, "version"]);
        command
    };
    #[cfg(not(target_os = "windows"))]
    let command = {
        let mut command = Command::new("npm");
        command.args(["view", package, "version"]);
        command
    };
    run_spawned_command(command, 20).and_then(|output| {
        version_number(&output).ok_or_else(|| format!("无法解析 {package} 的最新版本"))
    })
}

fn run_command(
    path: &std::path::Path,
    args: &[&str],
    timeout_seconds: u64,
) -> Result<String, String> {
    let mut command = command_for_path(path);
    command.args(args);
    run_spawned_command(command, timeout_seconds)
}

fn run_spawned_command(mut command: Command, timeout_seconds: u64) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = crate::title_summary::run_cli_with_timeout(command, None,
        Duration::from_secs(timeout_seconds), 2 * 1024 * 1024, "Runtime 命令")
        .map_err(|error| format!("{error:#}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stdout.is_empty() { stderr } else if stderr.is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
    if output.status.success() { Ok(detail) }
    else { Err(if detail.is_empty() { output.status.to_string() } else { detail }) }
}

fn version_number(value: &str) -> Option<String> {
    value
        .split(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .find(|part| part.contains('.') && part.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))
        .map(str::to_string)
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    parse(latest) > parse(current)
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn runtime_command_drains_both_pipes_before_waiting_for_exit() {
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "process.stdout.write('cli 1.2.3\\n'+'x'.repeat(200000));process.stderr.write('diagnostic '.repeat(20000));"]);
        let output = run_spawned_command(command, 5).unwrap();
        assert!(output.starts_with("cli 1.2.3"));
        assert!(output.contains("diagnostic"));
    }


    #[test]
    fn claude_and_codex_advertise_acp_with_pty_fallback() {
        for kind in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex] {
            let definition = DEFINITIONS
                .iter()
                .find(|definition| definition.kind == kind)
                .unwrap();
            assert_eq!(definition.protocol, "acp_v1+pty_fallback");
            assert!(capabilities_for_runtime(kind)
                .iter()
                .any(|value| value == "acp"));
        }
    }

    #[test]
    fn compares_versions_from_all_runtime_output_shapes() {
        assert_eq!(
            version_number("codex-cli 0.153.1").as_deref(),
            Some("0.153.1")
        );
        assert!(version_is_newer("18.1.10", "17.3.5"));
        assert!(!version_is_newer("1.18.16", "1.18.16"));
    }

    #[test]
    fn removed_runtime_has_no_probe_upgrade_or_executable() {
        let kinds = DEFINITIONS
            .iter()
            .map(|definition| definition.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                AgentRuntimeKind::ZCode,
                AgentRuntimeKind::ZCodeCli,
                AgentRuntimeKind::DeepSeekHarness,
                AgentRuntimeKind::ClaudeCode,
                AgentRuntimeKind::Codex,
                AgentRuntimeKind::KimiCode,
                AgentRuntimeKind::Pi,
                AgentRuntimeKind::OpenCode,
            ]
        );
        assert!(resolve_runtime_executable(AgentRuntimeKind::OhMyPi).is_none());
        assert!(resolve_runtime_executable(AgentRuntimeKind::MiniMaxCode).is_none());
        assert!(capabilities_for_runtime(AgentRuntimeKind::MiniMaxCode).is_empty());
        assert!(upgrade_runtime(AgentRuntimeKind::MiniMaxCode).unwrap_err().contains("已移除"));
        assert!(capabilities_for_runtime(AgentRuntimeKind::OhMyPi).is_empty());
        assert!(upgrade_runtime(AgentRuntimeKind::OhMyPi)
            .unwrap_err()
            .contains("已移除"));
    }

    #[test]
    #[ignore = "需要访问 npm registry 和各 Runtime 更新源"]
    fn checks_real_installed_runtime_updates() {
        let probes = check_runtime_updates();
        assert_eq!(probes.len(), DEFINITIONS.len());
        for probe in probes {
            println!(
                "{} current={:?} latest={:?} status={}",
                probe.kind.as_str(),
                probe.version,
                probe.latest_version,
                probe.update_status
            );
            if probe.installed {
                assert_ne!(probe.update_status, "unchecked");
            }
        }
    }
}

pub(crate) fn resolve_runtime_executable(kind: AgentRuntimeKind) -> Option<PathBuf> {
    resolve_runtime_executable_checked(kind).ok()
}

pub(crate) fn resolve_runtime_executable_checked(kind: AgentRuntimeKind) -> Result<PathBuf, String> {
    let definition = executable_definition(kind)
        .ok_or_else(|| format!("Runtime {} 不可用", kind.as_str()))?;
    if definition.kind == AgentRuntimeKind::Codex {
        return resolve_codex_definition(definition).map(|(path, _)| path);
    }
    resolve_definition_executable(definition).map(|(path, _)| path)
        .ok_or_else(|| format!("未检测到 {} Runtime；请确认对应 CLI 已安装且在应用 PATH 中", kind.as_str()))
}

fn executable_definition(kind: AgentRuntimeKind) -> Option<&'static RuntimeDefinition> {
    // Native chats own separate settings/availability, but execute their CLI's
    // installed binary. This mapping must not change the session runtime kind.
    let executable_kind = match kind {
        AgentRuntimeKind::ClaudeChat => AgentRuntimeKind::ClaudeCode,
        AgentRuntimeKind::CodexChat => AgentRuntimeKind::Codex,
        AgentRuntimeKind::KimiChat => AgentRuntimeKind::KimiCode,
        AgentRuntimeKind::PiChat => AgentRuntimeKind::Pi,
        AgentRuntimeKind::OpenCodeChat => AgentRuntimeKind::OpenCode,
        other => other,
    };
    DEFINITIONS
        .iter()
        .find(|definition| definition.kind == executable_kind)
}

fn resolve_definition_executable(
    definition: &RuntimeDefinition,
) -> Option<(PathBuf, Option<String>)> {
    if definition.kind == AgentRuntimeKind::ZCode {
        return super::zcode::executable().ok().map(|path| (path, None));
    }
    if definition.kind == AgentRuntimeKind::ZCodeCli {
        return super::zcode::cli_executable().ok().map(|path| (path, None));
    }
    if definition.kind == AgentRuntimeKind::DeepSeekHarness {
        return crate::acp::deepseek_executable()
            .ok()
            .map(|path| (path, None));
    }
    if definition.kind == AgentRuntimeKind::ClaudeCode {
        if let Ok(path) = crate::launcher::resolve_claude() {
            return Some((path, None));
        }
    }

    if definition.kind == AgentRuntimeKind::Codex {
        return resolve_codex_definition(definition).ok();
    }


    if let Ok(path) = which::which(definition.command) {
        return Some((path, None));
    }
    None
}

fn resolve_codex_definition(definition: &RuntimeDefinition) -> Result<(PathBuf, Option<String>), String> {
    let mut candidates = Vec::new();
    if let Ok(path) = which::which(definition.command) { candidates.push(path); }
    for prefix in codex_npm_prefixes() { candidates.extend(codex_npm_candidates(&prefix)); }
    resolve_codex_candidates_checked(candidates).map(|(path, version)| (path, Some(version)))
}

fn codex_npm_prefixes() -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    if let Some(prefix) = std::env::var_os("NPM_CONFIG_PREFIX") {
        prefixes.push(PathBuf::from(prefix));
    }
    #[cfg(target_os = "windows")]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        prefixes.push(PathBuf::from(app_data).join("npm"));
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(home) = dirs::home_dir() {
        prefixes.push(home.join(".npm-global"));
        prefixes.push(home.join(".local"));
    }
    #[cfg(target_os = "macos")]
    prefixes.push(PathBuf::from("/opt/homebrew"));
    #[cfg(not(target_os = "windows"))]
    prefixes.push(PathBuf::from("/usr/local"));
    if let Some(data_dir) = dirs::data_dir() {
        prefixes.push(data_dir.join("npm"));
    }
    prefixes.into_iter().fold(Vec::new(), |mut unique, prefix| {
        if !unique.contains(&prefix) {
            unique.push(prefix);
        }
        unique
    })
}

#[cfg(test)]
fn resolve_codex_from_npm_prefixes<P>(
    prefixes: impl IntoIterator<Item = P>,
) -> Option<(PathBuf, String)>
where
    P: AsRef<std::path::Path>,
{
    let mut candidates = Vec::new();
    for prefix in prefixes {
        candidates.extend(codex_npm_candidates(prefix.as_ref()));
    }
    resolve_codex_candidates(candidates)
}

fn resolve_codex_candidates_checked(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<(PathBuf, String), String> {
    let mut inspected = Vec::new();
    let mut failures = Vec::new();
    for candidate in candidates {
        if inspected.contains(&candidate) { continue; }
        inspected.push(candidate.clone());
        if !candidate.is_file() { continue; }
        let (version, error) = read_version(&candidate);
        if let Some(version) = version.filter(|value| value.to_ascii_lowercase().contains("codex")) {
            return Ok((candidate, version));
        }
        let detail = error.unwrap_or_else(|| "命令输出不是 Codex 版本".into());
        failures.push(format!("{}：{detail}", candidate.display()));
    }
    if failures.is_empty() {
        Err("未找到 codex 命令；请安装 Codex CLI，并确认应用可继承其安装路径".into())
    } else {
        // Keep version/process errors distinct from a missing installation.
        Err(format!("已找到 Codex 文件，但版本检查失败（不是未安装）。请检查 Node.js、CLI 安装和进程检查错误：{}", failures.join("；")))
    }
}

#[cfg(test)]
fn resolve_codex_candidates(candidates: impl IntoIterator<Item = PathBuf>) -> Option<(PathBuf, String)> {
    resolve_codex_candidates_checked(candidates).ok()
}

fn codex_npm_candidates(prefix: &std::path::Path) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    let mut candidates = vec![
        prefix.join("codex.cmd"),
        prefix.join("node_modules").join(".bin").join("codex.cmd"),
    ];
    #[cfg(not(target_os = "windows"))]
    let mut candidates = vec![
        prefix.join("bin").join("codex"),
        prefix.join("node_modules").join(".bin").join("codex"),
    ];

    if let Some((package, target, executable)) = native_codex_package() {
        let mut package_roots = vec![prefix.join("node_modules").join("@openai")];
        package_roots.push(
            prefix
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("node_modules")
                .join("@openai"),
        );
        #[cfg(not(target_os = "windows"))]
        {
            package_roots.push(prefix.join("lib").join("node_modules").join("@openai"));
            package_roots.push(
                prefix
                    .join("lib")
                    .join("node_modules")
                    .join("@openai")
                    .join("codex")
                    .join("node_modules")
                    .join("@openai"),
            );
        }
        for package_root in package_roots {
            candidates.push(
                package_root
                    .join(package)
                    .join("vendor")
                    .join(target)
                    .join("bin")
                    .join(executable),
            );
        }
    }
    candidates
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-win32-x64", "x86_64-pc-windows-msvc", "codex.exe"))
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-win32-arm64", "aarch64-pc-windows-msvc", "codex.exe"))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-linux-x64", "x86_64-unknown-linux-musl", "codex"))
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-linux-arm64", "aarch64-unknown-linux-musl", "codex"))
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-darwin-x64", "x86_64-apple-darwin", "codex"))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    Some(("codex-darwin-arm64", "aarch64-apple-darwin", "codex"))
}

#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
fn native_codex_package() -> Option<(&'static str, &'static str, &'static str)> {
    None
}


fn read_version(executable: &PathBuf) -> (Option<String>, Option<String>) {
    let mut command = command_for_path(executable);
    command.arg("--version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = match crate::title_summary::run_cli_with_timeout(command, None, Duration::from_secs(3),
        2 * 1024 * 1024, "读取 Runtime 版本") {
        Ok(output) => output,
        Err(error) => return (None, Some(format!("读取版本失败: {error:#}"))),
    };
    let raw = if output.stdout.is_empty() { &output.stderr } else { &output.stdout };
    let version = String::from_utf8_lossy(raw).trim().to_string();
    if !output.status.success() {
        return (None, Some(format!("读取版本失败: {}", if version.is_empty() { output.status.to_string() } else { version })));
    }
    if version.is_empty() { (None, Some("命令未返回版本".into())) } else { (Some(version), None) }
}

pub(crate) fn command_for_path(path: &std::path::Path) -> Command {
    if cfg!(target_os = "windows")
        && path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
    {
        let mut command = Command::new("cmd.exe");
        command.arg("/d").arg("/c").arg(path);
        command
    } else {
        Command::new(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "需要已安装 Codex CLI；只读检查原生 Chat 的交接前可执行文件探测"]
    fn handoff_preflight_codex_chat_reuses_installed_cli() {
        let cli = resolve_runtime_executable(AgentRuntimeKind::Codex).expect("installed Codex CLI");
        let chat = resolve_runtime_executable(AgentRuntimeKind::CodexChat).expect("Codex Chat must share the installed CLI");
        assert_eq!(chat, cli);
    }

    #[test]
    fn native_chat_executable_ownership_matches_cli_without_merging_runtime_preferences() {
        for (chat, cli) in [(AgentRuntimeKind::ClaudeChat, AgentRuntimeKind::ClaudeCode), (AgentRuntimeKind::CodexChat, AgentRuntimeKind::Codex), (AgentRuntimeKind::KimiChat, AgentRuntimeKind::KimiCode), (AgentRuntimeKind::PiChat, AgentRuntimeKind::Pi), (AgentRuntimeKind::OpenCodeChat, AgentRuntimeKind::OpenCode)] {
            assert_eq!(executable_definition(chat).unwrap().kind, cli);
            assert_ne!(chat.as_str(), cli.as_str());
        }
        for runtime in [AgentRuntimeKind::ZCode, AgentRuntimeKind::ZCodeCli, AgentRuntimeKind::DeepSeekHarness] {
            assert_eq!(executable_definition(runtime).unwrap().kind, runtime);
        }
        for retired in [AgentRuntimeKind::Unsupported, AgentRuntimeKind::OhMyPi, AgentRuntimeKind::MiniMaxCode] {
            assert!(executable_definition(retired).is_none());
        }
    }


    #[test]
    fn codex_npm_fallback_skips_a_broken_wrapper() {
        let root = std::env::temp_dir().join(format!(
            "crewvela-codex-broken-wrapper-{}",
            uuid::Uuid::new_v4()
        ));
        let broken_prefix = root.join("broken");
        let valid_prefix = root.join("valid");
        fs::create_dir_all(&broken_prefix).unwrap();
        fs::create_dir_all(&valid_prefix).unwrap();
        #[cfg(windows)]
        {
            fs::write(broken_prefix.join("codex.cmd"), b"@exit /b 1\r\n").unwrap();
            fs::write(valid_prefix.join("codex.cmd"), b"@echo codex-cli test-version\r\n").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for (prefix, script) in [(&broken_prefix, "#!/bin/sh\nexit 1\n"),
                (&valid_prefix, "#!/bin/sh\nprintf 'codex-cli test-version\\n'\n")] {
                fs::create_dir_all(prefix.join("bin")).unwrap();
                let entry = prefix.join("bin/codex");
                fs::write(&entry, script).unwrap();
                fs::set_permissions(entry, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        let resolved =
            resolve_codex_from_npm_prefixes([broken_prefix.as_path(), valid_prefix.as_path()]);

        let expected = valid_prefix.join(if cfg!(windows) { "codex.cmd" } else { "bin/codex" });
        assert_eq!(
            resolved.as_ref().map(|(path, _)| path.as_path()),
            Some(expected.as_path())
        );
        assert_eq!(
            resolved.as_ref().map(|(_, version)| version.as_str()),
            Some("codex-cli test-version")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_npm_candidates_include_the_current_platform_native_binary() {
        let Some((package, target, executable)) = native_codex_package() else {
            return;
        };
        let prefix = PathBuf::from("npm-prefix");
        let expected = prefix
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join(package)
            .join("vendor")
            .join(target)
            .join("bin")
            .join(executable);

        assert!(codex_npm_candidates(&prefix).contains(&expected));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn codex_npm_fallback_uses_the_standard_global_wrapper() {
        let prefix =
            std::env::temp_dir().join(format!("crewvela-codex-wrapper-{}", uuid::Uuid::new_v4()));
        let wrapper = prefix.join("codex.cmd");
        fs::create_dir_all(&prefix).unwrap();
        fs::write(&wrapper, b"@echo codex-cli test-version\r\n").unwrap();

        let resolved = resolve_codex_from_npm_prefixes([prefix.as_path()]);

        assert_eq!(
            resolved.as_ref().map(|(path, _)| path.as_path()),
            Some(wrapper.as_path())
        );
        assert_eq!(
            resolved.as_ref().map(|(_, version)| version.as_str()),
            Some("codex-cli test-version")
        );
        fs::remove_dir_all(prefix).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn codex_probe_preserves_failure_instead_of_reporting_not_installed() {
        let root = std::env::temp_dir().join(format!("codex-probe-error-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let wrapper = root.join("codex.cmd");
        fs::write(&wrapper, b"@echo fixture-version-probe-failed 1>&2\r\n@exit /b 23\r\n").unwrap();
        let error = resolve_codex_candidates_checked([wrapper]).unwrap_err();
        fs::remove_dir_all(root).unwrap();
        assert!(error.contains("fixture-version-probe-failed"), "lost actual probe error: {error}");
        assert!(!error.contains("未找到"), "existing executable was reported as absent");
    }

    #[cfg(unix)]
    #[test]
    fn codex_probe_preserves_unix_failure_instead_of_reporting_not_installed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("codex-probe-error-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let wrapper = root.join("codex");
        fs::write(&wrapper, "#!/bin/sh\nprintf 'fixture-version-probe-failed\\n' >&2\nexit 23\n").unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let error = resolve_codex_candidates_checked([wrapper]).unwrap_err();
        fs::remove_dir_all(root).unwrap();
        assert!(error.contains("fixture-version-probe-failed"), "lost actual probe error: {error}");
        assert!(!error.contains("未找到"));
    }

}
