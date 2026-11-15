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


/// 探测 claude 可执行文件路径，优先返回原生 claude.exe。
///
/// `which claude` 在 Windows 上通常返回 npm 生成的 shim（claude.cmd / claude.ps1
/// / 无扩展名 sh）。真正的原生入口在同级 node_modules/@anthropic-ai/claude-code/bin/
/// 下的 claude.exe。直接 spawn 该 .exe 可绕过 cmd.exe 中间层，避免 conpty 下
/// claude 的 TUI 不渲染、启动后卡住的问题。
pub fn resolve_claude() -> Result<PathBuf> {
    if let Some(path) = crate::runtime_installation::managed_executable(crate::multi_agent::AgentRuntimeKind::ClaudeCode)? { return Ok(path); }
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

/// 从 cwds 取第 i 个有效目录；越界或空串返回 None（调用方据此用 home）。
pub fn pick_cwd<'a>(cwds: &'a [String], i: usize) -> Option<&'a str> {
    cwds.get(i).map(|s| s.as_str()).filter(|s| !s.is_empty())
}

pub fn build_env(_provider: &ProviderConfig, _key: &str) -> HashMap<String,String> { HashMap::new() }

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
