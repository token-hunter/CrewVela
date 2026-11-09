use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use which::which;

use crate::providers::ProviderConfig;

/// Finder starts applications without the user's terminal PATH. Call once on
/// startup, before spawning runtimes or background workers. Other OSes keep the
/// inherited environment unchanged.
pub fn initialize_platform_path() {
    #[cfg(target_os = "macos")]
    {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var("SHELL").ok()
            .filter(|shell| matches!(shell.as_str(), "/bin/zsh" | "/bin/bash"))
            .unwrap_or_else(|| "/bin/zsh".into());
        // Interactive login config covers Homebrew and version managers. Bound
        // the wait: a broken shell profile must never prevent the app opening.
        let mut login_path = None;
        if let Ok(mut child) = Command::new(shell)
            .args(["-ilc", "printf '\\0CREWVELA_PATH\\0%s\\0' \"$PATH\""])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        {
            let stdout = child.stdout.take().expect("piped stdout");
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = stdout.take(1024 * 1024).read_to_end(&mut bytes);
                let _ = sender.send(bytes);
            });
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            if let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(100)) {
                                login_path = bytes.split(|byte| *byte == 0)
                                    .skip_while(|part| *part != b"CREWVELA_PATH")
                                    .nth(1).map(|part| {
                                        use std::os::unix::ffi::OsStringExt;
                                        std::ffi::OsString::from_vec(part.to_vec())
                                    });
                            }
                        }
                        break;
                    }
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                    _ => { let _ = child.kill(); let _ = child.wait(); break; }
                }
            }
        }
        let mut paths: Vec<PathBuf> = Vec::new();
        if let Some(login_path) = login_path {
            append_absolute_paths(&mut paths, std::env::split_paths(&login_path));
        }
        append_absolute_paths(&mut paths, std::env::split_paths(&inherited));
        append_absolute_paths(&mut paths, ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
        if let Some(home) = dirs::home_dir() {
            append_absolute_paths(&mut paths, [home.join(".local/bin"), home.join(".cargo/bin"), home.join(".npm-global/bin"), home.join(".bun/bin")]);
        }
        if let Ok(path) = std::env::join_paths(paths) { std::env::set_var("PATH", path); }
    }
}

#[cfg(any(target_os = "macos", test))]
fn append_absolute_paths(paths: &mut Vec<PathBuf>, candidates: impl IntoIterator<Item = PathBuf>) {
    for candidate in candidates {
        if candidate.is_absolute() && !paths.contains(&candidate) { paths.push(candidate); }
    }
}

/// 启动 Claude Code 前必须从继承环境移除的变量。
/// Provider 路由会由当前会话重新注入，NO_COLOR 会关闭终端颜色。
pub const CLEAR_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_CUSTOM_HEADERS",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "NO_COLOR",
];

/// 探测 claude 可执行文件路径，优先返回原生 claude.exe。
///
/// `which claude` 在 Windows 上通常返回 npm 生成的 shim（claude.cmd / claude.ps1
/// / 无扩展名 sh）。真正的原生入口在同级 node_modules/@anthropic-ai/claude-code/bin/
/// 下的 claude.exe。直接 spawn 该 .exe 可绕过 cmd.exe 中间层，避免 conpty 下
/// claude 的 TUI 不渲染、启动后卡住的问题。
pub fn resolve_claude() -> Result<PathBuf> {
    let raw = which("claude")
        .map_err(|_| anyhow!("未在 PATH 中找到 claude，请先安装 Claude Code CLI"))?;

    // 直接是 .exe 就用它
    if raw
        .extension()
        .map_or(false, |e| e.eq_ignore_ascii_case("exe"))
    {
        return Ok(raw);
    }

    if let Some(dir) = raw.parent() {
        let exe = dir
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe");
        if exe.exists() {
            return Ok(exe);
        }
    }

    // fallback：返回 shim 原路径（后续 spawn 时 .cmd 会经 cmd.exe /c 包装）
    Ok(raw)
}

/// 组装 provider 所需的环境变量（不含 CLEAR_VARS）。
pub fn build_env(provider: &ProviderConfig, key: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("ANTHROPIC_BASE_URL".into(), provider.base_url.clone());

    for (k, v) in &provider.env {
        if k != "ANTHROPIC_API_KEY" && k != "ANTHROPIC_AUTH_TOKEN" {
            env.insert(k.clone(), v.clone());
        }
    }

    env.insert(provider.meta.api_key_field.clone(), key.to_string());

    let mut custom_headers = Vec::new();
    if !provider.meta.custom_user_agent.is_empty() {
        custom_headers.push(format!("User-Agent: {}", provider.meta.custom_user_agent));
    }
    let mut header_entries = provider.meta.request_headers.iter().collect::<Vec<_>>();
    header_entries.sort_by(|left, right| left.0.cmp(right.0));
    custom_headers.extend(
        header_entries
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}")),
    );
    if !custom_headers.is_empty() {
        env.insert("ANTHROPIC_CUSTOM_HEADERS".into(), custom_headers.join("\n"));
    }

    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());

    if !env.contains_key("ANTHROPIC_MODEL") {
        let primary_model = env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .or_else(|| env.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
            .cloned();
        if let Some(primary_model) = primary_model {
            env.insert("ANTHROPIC_MODEL".into(), primary_model);
        }
    }

    if let Some(t) = provider.timeout_ms {
        env.insert("API_TIMEOUT_MS".into(), t.to_string());
    }
    if provider.disable_nonessential_traffic {
        env.insert(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
            "1".into(),
        );
    }
    env
}

/// 启动前校验 provider 配置。空 base_url 直接拒绝，避免连到无效端点。
pub fn validate_provider(provider: &ProviderConfig) -> Result<(), String> {
    if provider.base_url.trim().is_empty() {
        return Err(format!(
            "请先编辑 {} provider，填写 API 端点（base_url）",
            provider.id
        ));
    }
    Ok(())
}

/// 从 cwds 取第 i 个有效目录；越界或空串返回 None（调用方据此用 home）。
pub fn pick_cwd<'a>(cwds: &'a [String], i: usize) -> Option<&'a str> {
    cwds.get(i).map(|s| s.as_str()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_path_preserves_precedence_and_rejects_relative_entries() {
        let root = std::env::temp_dir();
        let first = root.join("crewvela-path-first");
        let second = root.join("crewvela-path-second");
        let mut paths = vec![first.clone()];
        append_absolute_paths(&mut paths, [second.clone(), first.clone(), PathBuf::new(), PathBuf::from("relative/bin")]);
        assert_eq!(paths, vec![first, second]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn platform_path_does_not_change_windows_or_linux_environment() {
        let before = std::env::var_os("PATH");
        initialize_platform_path();
        assert_eq!(std::env::var_os("PATH"), before);
    }

    fn sample() -> ProviderConfig {
        ProviderConfig {
            id: "x".into(),
            name: "X".into(),
            base_url: "https://example.com".into(),
            auth_token_ref: "x".into(),
            env: [("ANTHROPIC_MODEL".to_string(), "m1".to_string())]
                .into_iter()
                .collect(),
            timeout_ms: Some(1000),
            disable_nonessential_traffic: true,
            builtin: false,
            hidden: false,
            meta: crate::providers::ProviderMeta::default(),
        }
    }

    #[test]
    fn build_env_sets_base_token_and_models() {
        let env = build_env(&sample(), "k");
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").unwrap(),
            "https://example.com"
        );
        assert_eq!(env.get("ANTHROPIC_AUTH_TOKEN").unwrap(), "k");
        assert_eq!(env.get("ANTHROPIC_MODEL").unwrap(), "m1");
        assert_eq!(env.get("API_TIMEOUT_MS").unwrap(), "1000");
        assert_eq!(
            env.get("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC").unwrap(),
            "1"
        );
        // 不应包含需清除的变量
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn build_env_honors_provider_api_key_field() {
        let mut provider = sample();
        provider.meta.api_key_field = "ANTHROPIC_API_KEY".into();

        let env = build_env(&provider, "provider-key");

        assert_eq!(
            env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("provider-key")
        );
        assert!(!env.contains_key("ANTHROPIC_AUTH_TOKEN"));
        assert!(CLEAR_VARS.contains(&"ANTHROPIC_AUTH_TOKEN"));
    }

    #[test]
    fn inherited_provider_routing_is_cleared_before_launch() {
        for variable in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        ] {
            assert!(CLEAR_VARS.contains(&variable), "missing {variable}");
        }
    }

    #[test]
    fn build_env_applies_supported_custom_headers_without_a_proxy() {
        let mut provider = sample();
        provider
            .meta
            .request_headers
            .insert("x-team".into(), "dock".into());
        provider.meta.custom_user_agent = "crewvela/1.0".into();

        let env = build_env(&provider, "provider-key");

        assert_eq!(
            env.get("ANTHROPIC_CUSTOM_HEADERS").map(String::as_str),
            Some("User-Agent: crewvela/1.0\nx-team: dock")
        );
    }

    #[test]
    fn build_env_uses_provider_sonnet_model_when_primary_model_is_missing() {
        let mut provider = sample();
        provider.id = "glm".into();
        provider.env = [
            (
                "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
                "glm-4.6".to_string(),
            ),
            (
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                "glm-4.6".to_string(),
            ),
            (
                "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                "glm-4.5-air".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let env = build_env(&provider, "glm-key");

        assert_eq!(
            env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("glm-4.6")
        );
    }

    #[test]
    fn build_env_advertises_truecolor_terminal_capabilities() {
        let env = build_env(&sample(), "k");

        assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
        assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
        assert!(CLEAR_VARS.contains(&"NO_COLOR"));
    }

    #[test]
    fn validate_provider_rejects_empty_base_url() {
        let mut p = sample();
        p.base_url = String::new();
        let err = validate_provider(&p).unwrap_err();
        assert!(err.contains("base_url"), "错误应提及 base_url: {err}");
    }

    #[test]
    fn validate_provider_accepts_filled_base_url() {
        let p = sample();
        assert!(validate_provider(&p).is_ok());
    }

    #[test]
    fn pick_cwd_returns_some_for_valid_index() {
        let cwds = vec!["a".to_string(), "b".to_string()];
        assert_eq!(pick_cwd(&cwds, 0), Some("a"));
        assert_eq!(pick_cwd(&cwds, 1), Some("b"));
    }

    #[test]
    fn pick_cwd_out_of_bounds_is_none() {
        let cwds = vec!["a".to_string()];
        assert_eq!(pick_cwd(&cwds, 5), None);
    }

    #[test]
    fn pick_cwd_empty_string_is_none() {
        let cwds = vec!["".to_string(), "b".to_string()];
        assert_eq!(pick_cwd(&cwds, 0), None);
        assert_eq!(pick_cwd(&cwds, 1), Some("b"));
    }
}
