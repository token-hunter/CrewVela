use serde::Serialize;
use tauri::{ipc::Channel, Manager, WebviewWindow};

pub(crate) fn embedded_key() -> Option<&'static str> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        use tauri::utils::{config::BundleType, platform::bundle_type};
        if bundle_type() == Some(BundleType::Nsis) {
            return option_env!("CREWVELA_UPDATER_PUBLIC_KEY")
                .map(str::trim)
                .filter(|key| !key.is_empty());
        }
    }
    None
}

#[cfg(windows)]
pub(crate) fn configure_context<R: tauri::Runtime>(
    context: &mut tauri::Context<R>,
    pubkey: &str,
) {
    // Tauri deserializes the plugin configuration before running its setup hook.
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({ "pubkey": pubkey }),
    );
}

#[cfg(windows)]
pub(crate) fn register<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    app.plugin(tauri_plugin_updater::Builder::new().build())?;
    app.manage(PendingUpdate::default());
    Ok(())
}

pub(crate) fn is_ready<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    #[cfg(windows)]
    return !crate::test_profile::active() && app.try_state::<PendingUpdate>().is_some();
    #[cfg(not(windows))]
    {
        let _ = app;
        false
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn updater_startup_registers_only_after_configuration_is_available() {
        let missing_config = mock_builder().build(mock_context(noop_assets())).unwrap();
        let error = register(missing_config.handle()).unwrap_err();
        assert!(error.to_string().contains("plugins.updater"));
        assert!(!is_ready(missing_config.handle()));

        let mut context = mock_context(noop_assets());
        configure_context(&mut context, "test-updater-public-key");
        let configured = mock_builder().build(context).unwrap();
        assert!(!is_ready(configured.handle()));
        register(configured.handle()).unwrap();
        assert!(is_ready(configured.handle()));
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub(crate) enum DownloadProgress {
    #[cfg(windows)]
    Downloading { received: u64, total: u64 },
    #[cfg(windows)]
    Verifying,
}

#[tauri::command]
pub(crate) async fn app_download_update(window: WebviewWindow, version: String, progress: Channel<DownloadProgress>) -> Result<(), String> {
    if window.label() != "main" || !is_ready(window.app_handle()) {
        return Err("当前应用请通过官方发布页更新。".into());
    }
    #[cfg(windows)]
    return windows::download(window, version, progress).await;
    #[cfg(not(windows))]
    {
        let _ = (version, progress);
        Err("当前应用请通过官方发布页更新。".into())
    }
}

#[tauri::command]
pub(crate) async fn app_install_update(window: WebviewWindow, version: String) -> Result<bool, String> {
    if window.label() != "main" || !is_ready(window.app_handle()) {
        return Err("当前应用请通过官方发布页更新。".into());
    }
    #[cfg(windows)]
    return crate::blocking_command(move || windows::install(window, &version)).await;
    #[cfg(not(windows))]
    {
        let _ = version;
        Err("当前应用请通过官方发布页更新。".into())
    }
}

#[cfg(windows)]
pub(crate) use windows::PendingUpdate;

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::{app_help, sessions::SessionStatus};
    use sha2::{Digest, Sha256};
    use std::time::{Duration, Instant};
    use tauri::Url;
    use tauri_plugin_updater::{Update, UpdaterExt};
    use tokio::sync::Mutex;

    const DOWNLOAD_ROOT: &str = "https://github.com/token-hunter/CrewVela/releases/download";

    #[derive(Default)]
    pub(crate) struct PendingUpdate(Mutex<Option<VerifiedUpdate>>);

    struct VerifiedUpdate {
        update: Update,
        bytes: Vec<u8>,
    }

    pub(super) async fn download(window: WebviewWindow, version: String, progress: Channel<DownloadProgress>) -> Result<(), String> {
        let app = window.app_handle();
        let pending = app.state::<PendingUpdate>();
        let mut pending = pending.0.try_lock().map_err(|_| "更新已在进行。")?;
        if pending.as_ref().is_some_and(|ready| ready.update.version == version) {
            return Ok(());
        }
        let check = app_help::app_check_update(app.clone()).await?;
        let release = check.signed_update
            .filter(|_| check.status == "available" && check.latest_version.as_deref() == Some(version.as_str()))
            .ok_or("可用版本已变化，请重新检查更新。")?;
        let root = format!("{DOWNLOAD_ROOT}/{}", release.release_tag);
        let manifest = Url::parse(&format!("{root}/{}", app_help::UPDATE_MANIFEST))
            .map_err(|_| "更新清单地址无效。")?;
        let installer = Url::parse(&format!("{root}/{}", release.asset_name))
            .map_err(|_| "安装包地址无效。")?;
        let cleanup = app.clone();
        let mut builder = app.updater_builder()
            .endpoints(vec![manifest]).map_err(|_| "更新清单地址无效。")?
            .timeout(Duration::from_secs(300))
            .on_before_exit(move || {
                crate::ai_memory::embedded::stop_at_exit();
                cleanup.cleanup_before_exit();
            });
        let settings = crate::general_settings::load_cached();
        if !settings.http_proxy.trim().is_empty() {
            let proxy = Url::parse(settings.http_proxy.trim()).map_err(|_| "更新代理地址无效。")?;
            builder = builder.proxy(proxy);
        }
        let update = tokio::time::timeout(
            Duration::from_secs(20),
            builder.build().map_err(|_| "无法初始化更新服务。")?.check(),
        ).await.map_err(|_| "读取更新清单超时，请重试。")?
            .map_err(|_| "无法读取更新清单，请重试或前往官方发布页。")?
            .filter(|update| update.version == version && update.download_url == installer)
            .ok_or("更新清单与官方发布附件不一致。")?;

        let mut received = 0u64;
        let mut previous = Instant::now();
        let bytes = update.download(
            |chunk, _| {
                received = received.saturating_add(chunk as u64);
                if previous.elapsed() >= Duration::from_millis(80) || received == release.size {
                    let _ = progress.send(DownloadProgress::Downloading { received, total: release.size });
                    previous = Instant::now();
                }
            },
            || { let _ = progress.send(DownloadProgress::Verifying); },
        ).await.map_err(|_| "下载或签名校验失败，请重试或前往官方发布页。")?;
        if bytes.len() as u64 != release.size || format!("{:x}", Sha256::digest(&bytes)) != release.sha256 {
            return Err("更新包与官方发布附件的校验值不一致。".into());
        }
        *pending = Some(VerifiedUpdate { update, bytes });
        Ok(())
    }

    pub(super) fn install(window: WebviewWindow, version: &str) -> Result<bool, String> {
        let app = window.app_handle();
        let pending = app.state::<PendingUpdate>();
        let pending = pending.0.try_lock().map_err(|_| "更新已在进行。")?;
        let verified = pending.as_ref().filter(|ready| ready.update.version == version)
            .ok_or("请先下载并校验更新包。")?;

        // Refuse restarts during activity and hold both admission guards until the
        // installer starts. A failed launch drops them so that work can continue.
        let admission = app.state::<crate::runtime_admission::RuntimeAdmission>();
        let Ok(_admission) = admission.begin_shutdown(Duration::ZERO) else { return Ok(false) };
        let supervisor = app.state::<crate::MultiAgentSupervisor>();
        let Ok(supervisor) = supervisor.begin_shutdown() else { return Ok(false) };
        let sessions = app.state::<crate::SessionManager>();
        let agent_busy = sessions.with_worktree_records(|records| Ok(records.iter().any(|session| {
            matches!(session.status, SessionStatus::Running | SessionStatus::Waiting)
        }))).map_err(|_| "无法确认会话已结束，请重试。")?;
        if agent_busy || !supervisor.project_ids.is_empty()
            || app.state::<crate::PtyManager>().has_open_terminals()
            || app.state::<crate::multi_agent::ConversationSupervisor>().has_active_rounds()
            || app.state::<crate::multi_agent::project_preview::LocalProjectServices>().has_active_services()
            || app.state::<crate::automation::AutomationManager>().list().iter().any(|automation| {
                automation.recent_runs.iter().any(|run| run.stage.as_deref().is_some_and(crate::automation::is_active_stage))
            })
        {
            return Ok(false);
        }
        verified.update.install(&verified.bytes).map_err(|_| "无法启动安装程序，请重试。")?;
        Ok(true)
    }
}
