//! Explicit first-run UI profile. No legacy migration or external integrations.
use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::{Path, PathBuf}, sync::OnceLock};

const MARKER: &str = ".crewvela-test-profile";
const MARKER_CONTENT: &str = "crewvela-isolated-test-v1\n";
static PROFILE: OnceLock<Profile> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub webview_dir: PathBuf,
}

pub fn current() -> Option<&'static Profile> { PROFILE.get() }
pub fn active() -> bool { current().is_some() }

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))] { metadata.file_type().is_symlink() }
}

fn reject_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) => bail!("隔离测试目录不能经过符号链接或目录联接：{}", ancestor.display()),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error).context("无法核对隔离测试目录"),
        }
    }
    Ok(())
}

fn validate_existing_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry=entry?;
        let metadata=fs::symlink_metadata(entry.path())?;
        if is_link(&metadata) { bail!("隔离测试数据中存在链接，未加载任何旧数据"); }
        if metadata.is_dir() { validate_existing_tree(&entry.path())?; }
    }
    Ok(())
}

fn prepare(root: &Path, protected: &[PathBuf]) -> Result<Profile> {
    if !root.is_absolute() || root.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        bail!("隔离测试数据目录必须是没有 .. 的绝对路径");
    }
    #[cfg(windows)]
    if root.components().any(|part| matches!(part, std::path::Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::Verbatim(_) | std::path::Prefix::VerbatimDisk(_) | std::path::Prefix::VerbatimUNC(..) | std::path::Prefix::DeviceNS(_)))) {
        bail!("隔离测试目录不接受设备路径或长路径前缀");
    }
    let cleaned:PathBuf=root.components().collect();
    let root=cleaned.as_path();
    // Compare path components without opening any real history/config files.
    let normalized=root.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    for path in protected {
        let other=path.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
        if normalized==other || normalized.starts_with(&(other.clone()+"/")) || other.starts_with(&(normalized.clone()+"/")) {
            bail!("隔离测试目录不能包含、复用或位于正式用户数据目录中");
        }
    }
    reject_links(root)?;
    if root.exists() {
        if !root.is_dir() { bail!("隔离测试路径不是目录"); }
        let marker=root.join(MARKER);
        if marker.exists() {
            reject_links(&marker)?;
            if fs::read_to_string(&marker)?!=MARKER_CONTENT { bail!("隔离测试目录标记无效，未加载已有数据"); }
        } else if fs::read_dir(root)?.next().is_some() {
            bail!("隔离测试只接受空目录或已验证的测试目录，不会接管已有文件");
        }
        validate_existing_tree(root)?;
    }
    fs::create_dir_all(root)?;
    fs::write(root.join(MARKER),MARKER_CONTENT)?;
    let profile=Profile { data_dir:root.canonicalize()?, config_dir:root.join("config"), webview_dir:root.join("webview") };
    fs::create_dir_all(&profile.config_dir)?;
    fs::create_dir_all(&profile.webview_dir)?;
    Ok(profile)
}

pub fn initialize(root: &Path) -> Result<()> {
    if active() { bail!("隔离测试配置已经初始化"); }
    // WKWebView ignores data_directory. Fail before touching any profile or
    // creating a WebView if the dedicated persistent store API is unavailable.
    #[cfg(target_os = "macos")]
    check_macos_support()?;
    let mut protected=Vec::new();
    for base in [dirs::config_dir(),dirs::data_dir(),dirs::data_local_dir(),dirs::cache_dir()].into_iter().flatten() {
        for name in [crate::brand::CONFIG_NAME,crate::brand::LEGACY_NAME,crate::brand::APP_ID,crate::brand::LEGACY_APP_ID] {
            protected.push(base.join(name));
        }
    }
    if let Some(home)=dirs::home_dir() {
        for name in [".claude",".codex",".zcode",".kimi",".agents",".config"] { protected.push(home.join(name)); }
    }
    let profile=prepare(root,&protected)?;
    // WebView2 supports process-level profile overrides. Do not inherit a
    // parent process's real user-data folder or browser arguments in this mode.
    #[cfg(windows)] {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER",&profile.webview_dir);
        std::env::remove_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS");
    }
    PROFILE.set(profile).map_err(|_|anyhow::anyhow!("隔离测试初始化冲突"))
}

/// Defer isolated macOS windows so no default WebKit store can open first.
/// Tauri 2.11's WindowConfig conversion drops data_store_identifier; it must
/// also be applied explicitly to the runtime builder, after setup is ready.
pub fn configure_context(context: &mut tauri::Context<tauri::Wry>) -> Vec<tauri::utils::config::WindowConfig> {
    if let Some(profile)=current() {
        configure_windows(context.config_mut(),profile);
        return defer_macos_windows(context.config_mut(),cfg!(target_os = "macos"));
    }
    Vec::new()
}

fn defer_macos_windows(config: &mut tauri::Config, macos: bool) -> Vec<tauri::utils::config::WindowConfig> {
    if !macos { return Vec::new(); }
    config.app.windows.iter_mut().filter_map(|window| {
        if !window.create { return None; }
        let deferred=window.clone();
        window.create=false;
        Some(deferred)
    }).collect()
}

/// The same explicit builder path is exercised with MockRuntime in regression
/// tests; actual WKWebView store separation still requires native acceptance.
pub(crate) fn build_isolated_windows<R: tauri::Runtime>(
    app: &tauri::App<R>, windows: &[tauri::utils::config::WindowConfig],
) -> Result<()> {
    // Validate the entire plan before creating even one WebView. Never silently
    // fall back to the default store if an isolated window loses its identity.
    if windows.iter().any(|window| window.data_store_identifier.is_none()) {
        bail!("隔离测试窗口缺少专属 WebKit 存储身份，已拒绝创建窗口");
    }
    for window in windows {
        let identifier=window.data_store_identifier.context("隔离测试窗口缺少专属 WebKit 存储身份")?;
        tauri::WebviewWindowBuilder::from_config(app,window)?
            .data_store_identifier(identifier)
            .incognito(false)
            .build()?;
    }
    Ok(())
}

fn configure_windows(config: &mut tauri::Config, profile: &Profile) {
    configure_windows_for_platform(config, profile, cfg!(target_os = "macos"));
}

#[cfg(any(target_os = "macos", test))]
fn validate_macos_version(version: &str) -> Result<()> {
    let parts=version.trim().split('.').map(str::parse::<u32>).collect::<std::result::Result<Vec<_>,_>>();
    match parts {
        Ok(parts) if (1..=3).contains(&parts.len()) && parts[0]>=14 => Ok(()),
        Ok(parts) if (1..=3).contains(&parts.len()) => bail!("新用户隔离测试需要 macOS 14 或更高版本；未启动，也未加载正式数据"),
        _ => bail!("无法确认 macOS 版本，已拒绝启动隔离测试；未加载正式数据"),
    }
}

#[cfg(target_os = "macos")]
fn check_macos_support() -> Result<()> {
    let output=std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion").output().context("无法检测 macOS 版本，已拒绝启动隔离测试")?;
    if !output.status.success() { bail!("macOS 版本检测失败，已拒绝启动隔离测试"); }
    let version=std::str::from_utf8(&output.stdout).context("macOS 版本格式无效，已拒绝启动隔离测试")?;
    validate_macos_version(version)
}

// Keep the platform policy testable on Windows without pretending to run WebKit.
fn configure_windows_for_platform(config: &mut tauri::Config, profile: &Profile, macos: bool) {
    let digest=Sha256::digest(profile.data_dir.to_string_lossy().as_bytes());
    let hash=format!("{digest:x}");
    let mut store_identifier=[0_u8;16];
    store_identifier.copy_from_slice(&digest[..16]);
    // A stable, non-nil UUID-shaped identifier belongs exclusively to this root.
    store_identifier[6]=(store_identifier[6]&0x0f)|0x80;
    store_identifier[8]=(store_identifier[8]&0x3f)|0x80;
    config.identifier=format!("com.crewvela.test.{}",&hash[..16]);
    for window in &mut config.app.windows {
        if macos {
            window.data_directory=None;
            window.data_store_identifier=Some(store_identifier);
            window.incognito=false;
        } else {
            window.data_directory=Some(profile.webview_dir.clone());
            window.data_store_identifier=None;
        }
        if window.label=="main" { window.title="CrewVela · 新用户隔离测试".into(); }
    }
}

/// First-run test mode is deliberately limited to local configuration UI.
/// Keep an allowlist: new commands cannot silently gain access to real accounts.
pub(crate) fn allowed_command(command: &str) -> bool {
    matches!(command,
        "test_profile_info" | "get_runtime_preferences" | "set_runtime_enabled" |
        "list_providers" | "get_provider" | "get_key_state" | "set_key" | "delete_key" |
        "save_provider" | "delete_provider" | "set_provider_hidden" | "restore_builtin_provider" | "reorder_providers" |
        "list_sessions" | "list_archived_sessions" | "session_snapshot" | "session_switch_list" |
        "session_usages" | "session_telemetry" | "list_agent_activity" | "list_approval_cards" |
        "list_projects" | "create_project" | "update_project" | "delete_project" | "list_launch_presets" |
        "ma_list_projects" | "ma_snapshot" | "ma_list_interventions" | "ma_activity_events" | "ma_runtime_events" |
        "general_settings_get" | "general_settings_save" | "list_automations" |
        "notification_history_list" | "notification_history_append" | "notification_history_mark_all_read" | "notification_history_clear" |
        "is_control_window_open" | "take_control_dock_request"
    )
}

pub(crate) const BLOCKED_MESSAGE: &str = "新用户隔离测试模式未开放真实 Agent、账号及全局集成操作，以免读取或修改原有数据；请退出测试模式后使用正式功能。";

#[cfg(test)]
mod tests {
    use super::*;
    fn temporary() -> PathBuf {
        let root=std::env::temp_dir().join(format!("crewvela-profile-test-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        // macOS /var may alias /private/var. Windows canonicalization instead
        // adds a verbatim prefix which prepare deliberately refuses.
        #[cfg(not(windows))]
        let root=root.canonicalize().unwrap();
        root
    }
    #[test]
    fn empty_profile_is_independent_and_reopens_without_seeding_legacy_data() {
        let base=temporary();let legacy=base.join("legacy");fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("sessions.json"),"original history").unwrap();
        let root=base.join("isolated 中文 test");let profile=prepare(&root,&[legacy.clone()]).unwrap();
        assert!(fs::read_dir(&profile.config_dir).unwrap().next().is_none());
        fs::write(profile.config_dir.join("sessions.json"),"test history").unwrap();
        assert_eq!(prepare(&root,&[legacy.clone()]).unwrap().config_dir,profile.config_dir);
        assert_eq!(fs::read_to_string(legacy.join("sessions.json")).unwrap(),"original history");
        assert!(prepare(&legacy,&[legacy.clone()]).is_err());
        assert!(prepare(&legacy.join("."),&[legacy.clone()]).unwrap_err().to_string().contains("正式用户数据目录"));
        assert!(prepare(&legacy.join("nested"),&[legacy]).is_err());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn rejects_existing_files_bad_markers_relative_paths_and_links() {
        let base=temporary();let data=base.join("data");fs::create_dir(&data).unwrap();
        fs::write(data.join("history"),"keep").unwrap();
        assert!(prepare(&data,&[]).is_err());assert_eq!(fs::read_to_string(data.join("history")).unwrap(),"keep");
        fs::write(data.join(MARKER),"invalid").unwrap();assert!(prepare(&data,&[]).is_err());
        assert!(prepare(Path::new("relative"),&[]).is_err());
        #[cfg(windows)] {
            let link=base.join("link");junction::create(&data,&link).unwrap();
            assert!(prepare(&link,&[]).is_err());
            junction::delete(&link).unwrap();
        }
        #[cfg(unix)] {
            let link=base.join("link");std::os::unix::fs::symlink(&data,&link).unwrap();assert!(prepare(&link,&[]).is_err());fs::remove_file(link).unwrap();
        }
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn test_webview_profile_and_identity_are_separate() {
        let profile=Profile {data_dir:PathBuf::from("isolated"),config_dir:PathBuf::from("isolated/config"),webview_dir:PathBuf::from("isolated/webview")};
        let mut config=tauri::Config::default();config.app.windows.push(Default::default());
        configure_windows(&mut config,&profile);
        assert_ne!(config.identifier,crate::brand::APP_ID);
        if cfg!(target_os = "macos") {
            assert!(config.app.windows[0].data_directory.is_none());
            assert!(config.app.windows[0].data_store_identifier.is_some());
        } else {
            assert_eq!(config.app.windows[0].data_directory.as_ref(),Some(&profile.webview_dir));
            assert!(config.app.windows[0].data_store_identifier.is_none());
        }
        assert!(config.app.windows[0].title.contains("隔离测试"));
    }
    #[test]
    fn macos_webview_store_is_persistent_stable_and_unique_per_test_root() {
        let profile=Profile {data_dir:PathBuf::from("isolated 中文 one"),config_dir:PathBuf::from("one/config"),webview_dir:PathBuf::from("one/webview")};
        let mut first=tauri::Config::default();
        let mut window=tauri::utils::config::WindowConfig::default();
        window.data_directory=Some(PathBuf::from("real-profile-must-not-be-used"));
        window.data_store_identifier=Some([0;16]);
        window.incognito=true;
        first.app.windows.push(window);
        configure_windows_for_platform(&mut first,&profile,true);
        let store=first.app.windows[0].data_store_identifier.unwrap();
        assert_ne!(store,[0;16]);
        assert!(first.app.windows[0].data_directory.is_none());
        assert!(!first.app.windows[0].incognito);
        assert_ne!(first.identifier,crate::brand::APP_ID);
        let serialized=serde_json::to_value(&first.app.windows[0]).unwrap();
        assert_eq!(serialized["dataStoreIdentifier"],serde_json::json!(store));
        configure_windows_for_platform(&mut first,&profile,true);
        assert_eq!(first.app.windows[0].data_store_identifier,Some(store));
        let other=Profile {data_dir:PathBuf::from("isolated 中文 two"),..profile.clone()};
        configure_windows_for_platform(&mut first,&other,true);
        assert_ne!(first.app.windows[0].data_store_identifier,Some(store));
        // Switching the policy back keeps WebView2 using the explicit test path.
        configure_windows_for_platform(&mut first,&profile,false);
        assert_eq!(first.app.windows[0].data_directory,Some(profile.webview_dir));
        assert!(first.app.windows[0].data_store_identifier.is_none());
    }
    #[test]
    fn macos_runtime_window_plan_defers_only_automatic_windows_and_fails_closed() {
        use tauri::Manager;
        let profile=Profile {data_dir:PathBuf::from("isolated runtime"),config_dir:PathBuf::from("runtime/config"),webview_dir:PathBuf::from("runtime/webview")};
        let mut config=tauri::Config::default();
        config.app.windows.push(tauri::utils::config::WindowConfig::default());
        let mut template=tauri::utils::config::WindowConfig::default();
        template.label="template".into();template.create=false;
        config.app.windows.push(template);
        configure_windows_for_platform(&mut config,&profile,true);
        let windows=defer_macos_windows(&mut config,true);
        assert_eq!(windows.len(),1);
        assert_eq!(windows[0].label,"main");
        assert!(config.app.windows.iter().all(|window|!window.create));
        let store=windows[0].data_store_identifier.unwrap();
        assert_ne!(store,[0;16]);

        let mut context=tauri::test::mock_context(tauri::test::noop_assets());
        *context.config_mut()=config;
        let app=tauri::test::mock_builder().build(context).unwrap();
        assert!(app.webview_windows().is_empty(),"默认存储窗口不能在 setup 前创建");
        let mut invalid=windows[0].clone();
        invalid.label="missing-store".into();invalid.data_store_identifier=None;
        assert!(build_isolated_windows(&app,&[windows[0].clone(),invalid]).is_err());
        assert!(app.webview_windows().is_empty(),"缺失身份时必须在创建任何窗口前拒绝");
        build_isolated_windows(&app,&windows).unwrap();
        assert!(app.get_webview_window("main").is_some());
        assert!(app.get_webview_window("template").is_none());
        assert_eq!(app.webview_windows().len(),1);

        let mut windows_config=tauri::Config::default();
        windows_config.app.windows.push(Default::default());
        configure_windows_for_platform(&mut windows_config,&profile,false);
        assert!(defer_macos_windows(&mut windows_config,false).is_empty());
        assert!(windows_config.app.windows[0].create,"Windows 仍使用原自动创建路径");
    }
    #[test]
    fn macos_without_dedicated_store_support_fails_closed() {
        for version in ["14.0","14.7.1\n","15.0","26.1"] { validate_macos_version(version).unwrap(); }
        for version in ["10.15.7","12.7","13.6.9"] {
            assert!(validate_macos_version(version).unwrap_err().to_string().contains("macOS 14"));
        }
        for version in ["","unknown","14.x","14..1","14.0.0.1","-1","14.0 unexpected"] {
            assert!(validate_macos_version(version).is_err(),"{version}");
        }
    }
    #[test]
    fn external_account_history_tools_and_unknown_commands_are_blocked() {
        for command in ["cid_list_accounts","cid_init_accounts","launch_multi","launch_terminal","launch_runtime_session","resume_session","ai_memory_status","ai_memory_quick_start","ai_memory_recover_update","skill_center_snapshot","plugin_market_snapshot","automation_tick","check_codex_quota_updates","send_session_message","unknown_command"] {
            assert!(!allowed_command(command),"{command}");
        }
        for command in ["test_profile_info","list_sessions","list_projects","list_providers","get_key_state","general_settings_get","session_snapshot"] { assert!(allowed_command(command)); }
    }
    #[test]
    fn isolated_backend_initializes_real_empty_stores_without_real_credentials() {
        const VARIABLE:&str="CREWVELA_PROFILE_TEST_ROOT";
        if let Some(root)=std::env::var_os(VARIABLE) {
            initialize(Path::new(&root)).unwrap();
            assert!(active());
            assert_eq!(crate::brand::config_dir().unwrap(),current().unwrap().config_dir);
            crate::brand::prepare_app_storage_aliases().unwrap();
            assert!(crate::secrets::get("anthropic").unwrap().is_none());
            crate::secrets::set("isolated","synthetic-memory-only").unwrap();
            assert_eq!(crate::secrets::get("isolated").unwrap().as_deref(),Some("synthetic-memory-only"));
            crate::secrets::delete("isolated").unwrap();
            assert!(crate::sessions::SessionManager::new().unwrap().list().is_empty());
            assert!(crate::projects::list().unwrap().is_empty());
            assert!(crate::multi_agent::MultiAgentStore::new().unwrap().list_projects().unwrap().is_empty());
            assert!(!crate::providers::load().unwrap().providers.is_empty());
            return;
        }
        // A child process keeps the explicit global profile from leaking into
        // unrelated normal-startup regression tests in this process.
        let root=temporary();
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","test_profile::tests::isolated_backend_initializes_real_empty_stores_without_real_credentials","--nocapture"])
            .env(VARIABLE,&root).output().unwrap();
        assert!(output.status.success(),"{}\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        assert!(!root.join("config/providers.json").exists(), "local-only startup must not create model-service configuration");
        fs::remove_dir_all(root).unwrap();
    }
}
