//! macOS external adapters only. Never borrow the host's TCC permissions as
//! evidence of another application's authorization, and never grant them here.
use anyhow::Result;
#[cfg(any(target_os = "macos", test))]
use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MacControlTool { KimiCu }

#[cfg(any(target_os = "macos", test))]
impl MacControlTool {
    fn source(self) -> &'static str { match self {
        Self::KimiCu => "builtin@kimi-computer-use",
    } }
    fn name(self) -> &'static str { match self {
        Self::KimiCu => "kimi-cu",
    } }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacControlStatus {
    pub supported: bool,
    pub installed: bool,
    pub executable: Option<String>,
    pub mcp_registered: Option<bool>,
    pub accessibility: Option<bool>,
    pub screen_recording: Option<bool>,
    pub service_running: Option<bool>,
    pub detail: String,
}

#[cfg(any(target_os = "macos", test))]
fn platform_supported(_tool: MacControlTool, version: &str, _apple_silicon: bool) -> bool {
    version.trim().split('.').next().and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|major| major >= 14)
}

#[cfg(any(target_os = "macos", test))]
fn mac_version(raw: &str) -> Option<Vec<u32>> {
    let mut parts: Vec<u32> = raw.trim().split('.').map(str::parse).collect::<std::result::Result<_, _>>().ok()?;
    if parts.is_empty() || parts.len() > 3 { return None; }
    parts.resize(3, 0); Some(parts)
}

#[cfg(any(target_os = "macos", test))]
fn binary_supports(bytes: &[u8], apple_silicon: bool) -> bool {
    let cpu_supported = |cpu: u32| cpu == 0x01000007 || (apple_silicon && cpu == 0x0100000c);
    let Some(magic) = bytes.get(..4) else { return false; };
    if magic == [0xcf, 0xfa, 0xed, 0xfe] {
        return bytes.get(4..8).is_some_and(|cpu| cpu_supported(u32::from_le_bytes(cpu.try_into().unwrap())));
    }
    if magic == [0xca, 0xfe, 0xba, 0xbe] || magic == [0xca, 0xfe, 0xba, 0xbf] {
        let Some(count) = bytes.get(4..8).map(|n| u32::from_be_bytes(n.try_into().unwrap()) as usize) else { return false; };
        let stride = if magic[3] == 0xbf { 32 } else { 20 };
        if count == 0 || count > 32 || bytes.len() < 8 + stride * count { return false; }
        return (0..count).any(|i| cpu_supported(u32::from_be_bytes(bytes[8+i*stride..12+i*stride].try_into().unwrap())));
    }
    false
}

/// Bundle metadata comes from macOS plutil, including binary Info.plist files.
#[cfg(any(target_os = "macos", test))]
fn bundle_executable(app: &Path, info: &serde_json::Value, id: &str) -> Result<PathBuf> {
    if info["CFBundleIdentifier"].as_str() != Some(id) { bail!("应用标识不匹配：{}", app.display()); }
    let name = info["CFBundleExecutable"].as_str().filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("应用缺少 CFBundleExecutable"))?;
    if name.contains(['/', '\\']) || matches!(name, "." | "..") { bail!("应用执行入口无效"); }
    let exe = app.join("Contents/MacOS").join(name);
    if !exe.is_file() { bail!("应用执行入口不存在：{}", exe.display()); }
    Ok(exe)
}

#[cfg(any(target_os = "macos", test))]
fn parse_kimi_permissions(raw: &str) -> Result<(bool, bool)> {
    // These are the official CLI's explicit permission fields, not generic
    // occurrences of true/false elsewhere in a diagnostic log.
    for line in raw.lines() {
        let Some(fields) = line.split_once("permissions:").or_else(|| line.split_once("permissionStatus:")).map(|(_, rest)| rest) else { continue; };
        let flag = |key: &str| fields.split_whitespace().find_map(|word| word.strip_prefix(key))
            .and_then(|value| match value { "true" => Some(true), "false" => Some(false), _ => None });
        if let (Some(a), Some(s)) = (flag("accessibility="), flag("screenRecording=")) { return Ok((a, s)); }
    }
    bail!("KimiCU 未返回可识别的权限状态；请在官方应用中检查授权")
}

#[cfg(any(target_os = "macos", test))]
fn registration_matches(entry: &crate::mcp::McpServerConfig, exe: &Path, path: &str) -> bool {
    entry.enabled && entry.transport == crate::mcp::McpTransport::Stdio
        && entry.command.as_deref() == exe.to_str()
        && entry.args == ["mcp"] && entry.env.get("PATH").map(String::as_str) == Some(path)
}

#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod native {
    use super::*;
    use std::{process::Command, time::Duration};
    static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn capture(exe: &Path, args: &[&str]) -> Result<std::process::Output> {
        let mut command = Command::new(exe); command.args(args);
        let result = crate::title_summary::run_cli_with_timeout(command, None, Duration::from_secs(15), 64 * 1024, "电脑控制检查")?;
        // Do not surface raw external diagnostics: they can contain local data.
        if !result.status.success() { bail!("电脑控制命令失败（{}）；请在官方应用中检查安装或权限", result.status); }
        Ok(result)
    }
    fn output(exe: &Path, args: &[&str]) -> Result<String> {
        let result = capture(exe, args)?;
        let mut raw = result.stdout;
        if raw.is_empty() { raw = result.stderr; }
        String::from_utf8(raw).context("电脑控制检查未返回 UTF-8")
    }
    fn app_binary(app: &Path, id: &str) -> Result<PathBuf> {
        let plist = app.join("Contents/Info.plist");
        let text = output(Path::new("/usr/bin/plutil"), &["-convert", "json", "-o", "-", plist.to_str().context("应用路径编码无效")?])?;
        let info = serde_json::from_str(&text).context("应用元数据无效")?;
        let exe = bundle_executable(app, &info, id)?;
        executable(&exe)?;
        if let Some(required) = info["LSMinimumSystemVersion"].as_str() {
            let current = output(Path::new("/usr/bin/sw_vers"), &["-productVersion"])?;
            if mac_version(&current).zip(mac_version(required)).is_none_or(|(current, required)| current < required) {
                bail!("应用要求 macOS {required} 或更新版本");
            }
        }
        let silicon = apple_silicon();
        use std::io::Read;
        let mut header = Vec::new(); std::fs::File::open(&exe)?.take(4096).read_to_end(&mut header)?;
        if !binary_supports(&header, silicon) { bail!("应用执行入口不支持当前架构"); }
        Ok(exe)
    }
    fn executable(exe: &Path) -> Result<()> {
        if !exe.is_file() { bail!("文件不存在：{}", exe.display()); }
        #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(exe)?.permissions().mode() & 0o111 == 0 {
            bail!("文件不可执行：{}", exe.display());
        }
        }
        Ok(())
    }
    fn app_for_exe(exe: &Path) -> Option<PathBuf> {
        exe.ancestors().find(|p| p.extension().is_some_and(|ext| ext == "app")).map(Path::to_path_buf)
    }
    pub(super) fn discover(tool: MacControlTool) -> Result<PathBuf> {
        // Prefer a valid user-level app before inspecting PATH and system Applications.
        let name = "KimiCU.app";
        let id = "ai.kimi.cu";
        if let Some(home) = dirs::home_dir() {
            let app = home.join("Applications").join(name);
            if app.is_dir() {
                if let Ok(executable) = app_binary(&app, id) { return Ok(executable); }
            }
        }
        // lib::run initializes Finder PATH once, before any runtime discovery.
        // Resolve symlinks for a CLI in a relocated .app, never infer a user name.
        let cli = which::which(tool.name());
        if let Ok(cli) = cli {
            let canonical = std::fs::canonicalize(&cli)?;
            if let Some(app) = app_for_exe(&canonical) {
                return app_binary(&app, id);
            }
        }
        let mut apps = vec![];
        if let Some(home) = dirs::home_dir() { apps.push(home.join("Applications").join(name)); }
        apps.push(Path::new("/Applications").join(name));
        let mut errors = vec![];
        for app in apps.into_iter().filter(|app| app.exists()) {
            match app_binary(&app, id) { Ok(exe) => return Ok(exe), Err(e) => errors.push(e.to_string()) }
        }
        if !errors.is_empty() { bail!("{}", errors.join("；")); }
        bail!("未找到 {}；请先安装官方运行时，再重新检测。支持 PATH、用户 Applications 和系统 Applications", tool.name())
    }
    pub(super) fn supported(tool: MacControlTool) -> Result<()> {
        let version = output(Path::new("/usr/bin/sw_vers"), &["-productVersion"])?;
        if !platform_supported(tool, &version, apple_silicon()) {
            bail!("{} 需要 macOS 14+ 和兼容当前芯片的官方运行时", tool.name());
        }
        Ok(())
    }
    fn apple_silicon() -> bool {
        cfg!(target_arch = "aarch64") || output(Path::new("/usr/sbin/sysctl"), &["-n", "hw.optional.arm64"]).is_ok_and(|s| s.trim() == "1")
    }
    fn config(tool: MacControlTool, exe: &Path) -> Result<crate::mcp::McpServerConfig> {
        let mut config = match tool {
            MacControlTool::KimiCu => crate::kimi_computer_use::build_mcp_config(&crate::kimi_computer_use::KimiComputerUseSettings { enabled: true }, exe),
        };
        config.env.insert("PATH".into(), std::env::var("PATH").context("PATH 编码无效")?);
        Ok(config)
    }
    pub(crate) fn inspect(tool: MacControlTool, dir: &Path, enabled: bool) -> MacControlStatus {
        let mut result = MacControlStatus::default();
        if let Err(e) = supported(tool) { result.detail = e.to_string(); return result; }
        result.supported = true;
        match discover(tool) {
            Err(e) => { result.detail = e.to_string(); },
            Ok(exe) => {
                result.installed = true; result.executable = Some(exe.display().to_string());
                result.detail = "运行时已检测到；辅助功能、屏幕录制与 MCP 连接尚未验证。点击检查权限或打开授权。".into();
                let path = std::env::var("PATH").unwrap_or_default();
                match crate::mcp::load_at(&dir.join("mcp-servers.json")) {
                    Err(e) => result.detail = format!("读取 MCP 配置失败：{e:#}"),
                    Ok(entries) => {
                        result.mcp_registered = Some(entries.iter().any(|e| e.source_plugin.as_deref() == Some(tool.source()) && registration_matches(e, &exe, &path)));
                        if enabled && result.mcp_registered == Some(false) {
                            result.detail = "已启用的 MCP 配置与本机运行时不一致，请点击重新连接后重开 Agent 会话。".into();
                        }
                    }
                }
            }
        }
        result
    }
    pub(crate) fn save(tool: MacControlTool, dir: &Path, settings_file: &Path, enabled: bool) -> Result<()> {
        let _write = WRITE.lock().map_err(|_| anyhow!("电脑控制设置锁异常"))?;
        let file = dir.join("mcp-servers.json");
        let previous = crate::mcp::load_at(&file)?.into_iter().find(|e| e.source_plugin.as_deref() == Some(tool.source()));
        if enabled {
            supported(tool)?;
            let exe = discover(tool)?;
            crate::mcp::upsert_builtin_server_at(&file, tool.source(), config(tool, &exe)?)?;
        } else { crate::mcp::remove_plugin_servers_at(&file, tool.source())?; }
        let persist = (|| -> Result<()> {
            std::fs::create_dir_all(dir)?;
            let temp = settings_file.with_extension("tmp");
            std::fs::write(&temp, serde_json::to_vec_pretty(&serde_json::json!({"enabled": enabled}))?)?;
            std::fs::rename(temp, settings_file)?; Ok(())
        })();
        if let Err(e) = persist {
            let rollback = match previous { Some(entry) => crate::mcp::upsert_builtin_server_at(&file, tool.source(), entry).map(|_| ()), None => crate::mcp::remove_plugin_servers_at(&file, tool.source()).map(|_| ()) };
            rollback.context("保存设置失败，且 MCP 恢复失败；请重新检测")?;
            return Err(e.context("保存设置失败，已恢复 MCP 配置"));
        }
        Ok(())
    }
    pub(super) fn action(tool: MacControlTool, prepare: bool) -> Result<MacControlStatus> {
        supported(tool)?;
        let exe = discover(tool)?;
        if prepare {
            match tool {
                MacControlTool::KimiCu => {
                    // Official install is idempotent. Do not uninstall/bootout
                    // an existing service or edit the user's LaunchAgents.
                    let running = output(&exe, &["service-status"]).is_ok_and(|service| service.split_whitespace().any(|part| part == "status=1"));
                    if !running { output(&exe, &["install"])?; }
                    let app = app_for_exe(&exe).context("未找到 KimiCU 应用包")?;
                    output(Path::new("/usr/bin/open"), &["-a", app.to_str().context("应用路径编码无效")?])?;
                }
            }
        }
        let dir = crate::providers::config_dir()?;
        let mut result = inspect(tool, &dir, false);
        let probe = match tool {
            MacControlTool::KimiCu => {
                let service = output(&exe, &["service-status"]);
                result.service_running = service.ok().map(|s| s.split_whitespace().any(|part| part == "status=1"));
                capture(&exe, &["xpc-ping"]).and_then(|out| parse_kimi_permissions(&format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))
            }
        };
        match probe {
            Ok((a, s)) => {
                result.accessibility = Some(a); result.screen_recording = Some(s);
                result.detail = if a && s { "权限已授予；MCP 配置不等于工具已加载，请重开 Agent 会话验证。" } else { "待授权：请在官方应用和系统设置中开启辅助功能、屏幕录制，再检查权限。" }.into();
            }
            Err(e) => result.detail = format!("权限尚未确认：{e:#}"),
        }
        Ok(result)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn disable_is_scoped_and_save_failure_restores_owned_registration() {
            let dir = std::env::temp_dir().join(format!("mac-control-save-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("mcp-servers.json");
            let tool = MacControlTool::KimiCu;
            let owned = crate::kimi_computer_use::build_mcp_config(&crate::kimi_computer_use::KimiComputerUseSettings { enabled: true }, Path::new("/fixture/tool"));
            let mut user = owned.clone(); user.id = "user".into(); user.name = "user-tool".into(); user.source_plugin = None;
            std::fs::write(&file, serde_json::to_vec(&serde_json::json!({"version":1,"servers":[owned.clone(), user.clone()]})).unwrap()).unwrap();
            let blocked = dir.join("blocked.json"); std::fs::create_dir(&blocked).unwrap();
            assert!(save(tool, &dir, &blocked, false).is_err());
            let entries = crate::mcp::load_at(&file).unwrap();
            assert!(entries.iter().any(|e| e.id == owned.id && e.enabled));
            assert!(entries.iter().any(|e| e.id == user.id));
            let settings = dir.join("settings.json");
            save(tool, &dir, &settings, false).unwrap();
            assert_eq!(crate::mcp::load_at(&file).unwrap().len(), 1);
            assert_eq!(serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&settings).unwrap()).unwrap()["enabled"], false);
            std::fs::write(&file, "invalid registry").unwrap();
            assert!(save(tool, &dir, &settings, false).is_err());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), "invalid registry");
            std::fs::remove_dir_all(dir).unwrap();
        }
        #[test]
        fn bounded_probe_preserves_json_and_does_not_disclose_raw_failure_output() {
            let node = which::which("node").unwrap();
            assert_eq!(output(&node, &["-e", "process.stdout.write('{}')"]).unwrap(), "{}");
            let error = output(&node, &["-e", "process.stderr.write('sensitive-fixture');process.exit(3)"]).unwrap_err().to_string();
            assert!(error.contains("失败")); assert!(!error.contains("sensitive-fixture"));
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use native::{inspect, save};

#[tauri::command]
pub async fn macos_computer_use_action(tool: MacControlTool, prepare: bool) -> Result<MacControlStatus, String> {
    if crate::test_profile::active() { return Err("隔离测试模式禁用真实电脑控制".into()); }
    #[cfg(target_os = "macos")]
    { tauri::async_runtime::spawn_blocking(move || native::action(tool, prepare)).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}")) }
    #[cfg(not(target_os = "macos"))]
    { let _ = (tool, prepare); Err("此入口仅用于 macOS".into()) }
}

#[cfg(target_os = "macos")]
static INSTALL_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Explicit user action only: installing also enables this CrewVela-managed MCP.
/// Existing user MCP entries are retained.
#[tauri::command]
pub async fn macos_computer_use_install(app: tauri::AppHandle, tool: MacControlTool, request_id: String) -> Result<MacControlStatus, String> {
    if crate::test_profile::active() { return Err("隔离测试模式禁用真实电脑控制安装".into()); }
    #[cfg(target_os = "macos")]
    {
        use tauri::Emitter;
        use std::sync::atomic::Ordering;
        uuid::Uuid::parse_str(&request_id).map_err(|_| "安装请求标识无效".to_string())?;
        if INSTALL_IN_FLIGHT.swap(true, Ordering::AcqRel) { return Err("已有电脑控制安装任务，请等待完成".into()); }
        struct Guard;
        impl Drop for Guard { fn drop(&mut self) { INSTALL_IN_FLIGHT.store(false, Ordering::Release); } }
        let _guard = Guard;
        native::supported(tool).map_err(|error| error.to_string())?;
        let progress = |message: &str| -> anyhow::Result<()> {
            app.emit("macos-computer-use-install-progress", serde_json::json!({ "tool": tool, "requestId": request_id, "message": message }))?;
            Ok(())
        };
        let installed_executable = crate::macos_computer_use_install::install(tool, &progress).await.map_err(|error| format!("{error:#}"))?;
        progress("运行时已安装，正在配置 MCP 与打开授权引导…").map_err(|error| error.to_string())?;
        tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<MacControlStatus> {
            let dir = crate::providers::config_dir()?;
            let discovered = native::discover(tool)?;
            if std::fs::canonicalize(&discovered)? != std::fs::canonicalize(&installed_executable)? {
                bail!("官方运行时已安装，但检测到另一执行入口；已有连接未修改，请检查本机重复安装后重新连接");
            }
            let file = dir.join("kimi-computer-use.json");
            native::save(tool, &dir, &file, true).context("运行时已安装，但 MCP 配置未完成；请在设置中启用并保存")?;
            let mut status = match native::action(tool, true) {
                Ok(status) => status,
                Err(error) => {
                    let mut status = native::inspect(tool, &dir, true);
                    status.detail = format!("运行时与 MCP 已设置，授权引导未完成：{error:#}。请点击打开授权后继续检查。");
                    return Ok(status);
                }
            };
            status.detail = format!("安装并启用完成。{} 重开 Agent 会话后验证工具连接。", status.detail);
            Ok(status)
        }).await.map_err(|error| error.to_string())?.map_err(|error| format!("{error:#}"))
    }
    #[cfg(not(target_os = "macos"))]
    { let _ = (app, tool, request_id); Err("此安装入口仅用于 macOS".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn supported_platforms_and_unknown_version() {
        assert!(platform_supported(MacControlTool::KimiCu, "14.0\n", false));
        assert!(platform_supported(MacControlTool::KimiCu, "26.0", true));
        assert!(platform_supported(MacControlTool::KimiCu, "14.0", false));
        assert!(!platform_supported(MacControlTool::KimiCu, "13.6", true));
        assert!(!platform_supported(MacControlTool::KimiCu, "unknown", true));
    }
    #[test] fn native_headers_and_minimum_system_versions() {
        let arm = [0xcf,0xfa,0xed,0xfe,0x0c,0,0,1];
        assert!(binary_supports(&arm, true)); assert!(!binary_supports(&arm, false));
        assert!(binary_supports(&[0xcf,0xfa,0xed,0xfe,7,0,0,1], false));
        let mut fat = vec![0xca,0xfe,0xba,0xbe,0,0,0,1];
        fat.extend([1,0,0,0x0c]); fat.resize(28, 0);
        assert!(binary_supports(&fat, true)); assert!(!binary_supports(&fat, false));
        assert!(!binary_supports(&fat[..12], true));
        assert!(!binary_supports(b"MZwrong-platform", true));
        assert!(mac_version("14.10").unwrap() > mac_version("14.2.1").unwrap());
        assert_eq!(mac_version("14.0"), mac_version("14"));
        assert!(mac_version("unknown").is_none());
    }
    #[test] fn permission_results_are_strict_and_unknown_is_not_granted() {
        for (accessibility, screen) in [(false, false), (true, false), (false, true), (true, true)] {
            let raw = format!("permissions: accessibility={accessibility} screenRecording={screen}");
            assert_eq!(parse_kimi_permissions(&raw).unwrap(), (accessibility, screen));
        }
        assert_eq!(parse_kimi_permissions("permissionStatus: accessibility=false screenRecording=true\n").unwrap(), (false, true));
        assert!(parse_kimi_permissions("unrelated accessibility=true screenRecording=true").is_err());
        assert!(parse_kimi_permissions("permissions: accessibility=unknown screenRecording=true").is_err());
    }
    #[test] fn removed_tool_is_rejected_by_ipc_deserialization() {
        assert!(serde_json::from_str::<MacControlTool>(r#""kimiCu""#).is_ok());
        assert!(serde_json::from_str::<MacControlTool>(r#""openComputerUse""#).is_err());
    }
    #[test] fn bundle_metadata_handles_spaces_unicode_and_rejects_traversal() {
        let app = std::env::temp_dir().join(format!("电脑控制 {}.app", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/kimi-cu"), b"fixture").unwrap();
        let mut info = serde_json::json!({"CFBundleIdentifier":"ai.kimi.cu","CFBundleExecutable":"kimi-cu"});
        assert_eq!(bundle_executable(&app, &info, "ai.kimi.cu").unwrap(), app.join("Contents/MacOS/kimi-cu"));
        assert!(bundle_executable(&app, &info, "wrong.id").is_err());
        for name in ["../kimi-cu", "..", "missing", "", "a\\b"] { info["CFBundleExecutable"] = name.into(); assert!(bundle_executable(&app, &info, "ai.kimi.cu").is_err()); }
        std::fs::remove_dir_all(app).unwrap();
    }
    #[test] fn mcp_path_drift_is_detected_without_adopting_other_flags() {
        let exe = Path::new("/Applications/工具.app/Contents/MacOS/tool");
        let mut config = crate::kimi_computer_use::build_mcp_config(&crate::kimi_computer_use::KimiComputerUseSettings { enabled: true }, exe);
        config.env.insert("PATH".into(), "/opt/homebrew/bin:/usr/bin".into());
        assert!(registration_matches(&config, exe, "/opt/homebrew/bin:/usr/bin"));
        assert!(!registration_matches(&config, exe, "/usr/bin"));
        config.args.push("--allow-global-input".into());
        assert!(!registration_matches(&config, exe, "/opt/homebrew/bin:/usr/bin"));
    }
}
