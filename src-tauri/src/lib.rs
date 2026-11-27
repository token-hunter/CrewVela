mod acp;
mod app_instance;
mod app_help;
mod ai_memory;
mod ai_memory_access;
mod approvals;
mod automation;
mod automation_runtime;
mod runtime_installation;
mod browser_control;
mod browser_panel;
#[cfg(target_os = "macos")]
mod browser_panel_macos;
mod clipboard_image;
mod codex_accounts;
mod codex_storage;
mod codex_communication;
mod session_mcp;
#[cfg(all(test, windows))]
mod settings_frame_native_tests;
mod codex_quota;
mod codex_title_summary;
mod collaboration_support;
mod communication_skill;
mod brand;
pub mod test_profile;
#[cfg(test)]
mod communication_acceptance_tests;
#[cfg(test)]
mod runtime_communication_acceptance_tests;
#[cfg(test)]
mod zcode_communication_acceptance_tests;
mod control;
pub mod general_settings;
mod native_language;
mod terminal_fonts;
mod git_source_control;
mod chat_turn_changes;
mod message_receipts;
mod keep_awake;
mod kimi_title_summary;
mod launcher;
mod mcp;
mod messaging;
mod multi_agent;
mod native_acp_sessions;
mod native_pi_sessions;
mod native_session_titles;
mod native_zcode_sessions;
mod harness_control;
mod notification_history;
mod presets;
mod projects;
mod prompt_optimizer;
mod providers;
mod pty;
mod retired_integrations;
mod runtime_preferences;
mod runtime_launch;
mod runtime_dependencies;
mod runtime_admission;
mod secrets;
mod session_switch;
mod session_switch_native;
mod session_switch_history;
mod session_switch_permissions;
mod sessions;
mod skill_center;
mod telemetry;
mod title_summary;
mod transcript;
mod usage;
#[cfg(target_os = "windows")]
mod windows_notification;
mod workspace_files;
mod workspace_trust;
mod worktree;
mod worktree_lifecycle;
mod worktree_management;
mod worktree_runtime;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, State,
    WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

use acp::{AcpManager, AcpPromptResult, AcpStatus};
use approvals::{collect_approval_cards, ApprovalAutoResponder, ApprovalCard, ApprovalDecision};
use automation::{
    Automation, AutomationAction, AutomationManager, NewAutomation, SessionEventKind,
};
use codex_quota::{
    CheckReceipt as CodexQuotaCheckReceipt, CodexQuotaMonitor,
    MonitorStatus as CodexQuotaMonitorStatus, StoredQuotaNotification,
};
use control::{
    AgentActivity, AgentActivityState, BroadcastReceipt, ControlHistoryMessage, ControlPlan,
    ControlWorkspaceTrustBatch, ControlWorkspaceTrustReceipt, ControlWorkspaceTrustStatus,
    DeliveryRecord, OutboxMessage, PermissionModeReceipt, QueuedMessage, SessionActionQueue,
};
use native_acp_sessions::{HarnessSessionSnapshot, NativeAcpSessions, NativeChatFlavor, NativeClaudeSessions, NativeCodexChatSessions, NativeKimiSessions, NativeOpenCodeChatSessions};
use native_pi_sessions::NativePiSessions;
use native_zcode_sessions::{NativeZCodeSessions, ZCodeSnapshot};
use git_source_control::{GitActionReceipt, GitBranchOption, GitSettings, GitStatusSnapshot};
use multi_agent::{
    cancel_coordinator_request, check_runtime_updates, direct_message_request,
    directed_message_runtimes, directed_sender_id, interpret_collaboration_message_streaming,
    merge_integrator_delivery, probe_all_runtimes, requested_agent_runtimes, explicitly_mentioned_agent_runtimes, upgrade_runtime,
    AddDependencyInput, AgentControlMode, AgentControlReceipt, AgentProfile, AgentRun,
    AgentRuntimeKind, ApplyCoordinatorPlanInput, ApplyCoordinatorPlanReceipt, ClaimTaskInput,
    CollaborationBridge, CollaborationGovernance, CoordinatorCancellationToken,
    CoordinatorHistoryMessage, CoordinatorPlan, CoordinatorStreamUpdate, CreateAgentInput,
    CreateProjectInput, CreateTaskInput, DirectMessageParticipant, DirectMessageRequest,
    EnqueueWakeupInput, HeartbeatReceipt, HeartbeatRunInput, InterventionRecord,
    InterventionResolutionReceipt, KnowledgeIndexReceipt, KnowledgeSearchResult, MemoryEntry,
    MultiAgentProject, MultiAgentSnapshot, MultiAgentStore, MultiAgentSupervisor,
    ProjectExecutionReceipt, PublishArtifactInput, PutMemoryInput, RecoveryReceipt,
    RequestApprovalInput, ResolveInterventionInput, ReturnAgentControlInput, RuntimeEventRecord,
    RuntimeProbe, RuntimeUpgradeReceipt, SearchKnowledgeInput, SearchMemoryInput,
    SendCollaborationMessageInput, TakeoverAgentInput, UpdateAgentInput, UpdateRunInput,
    WakeupRequest,
};
use notification_history::{NotificationHistoryRecord, NotificationHistoryStore};
use presets::LaunchPreset;
use projects::Project;
use providers::ProviderConfig;
use pty::{PtyManager, PtyOpenOptions, WorkspaceTrustState};
use sessions::{
    ApprovalPolicy, NewRuntimeSession, NewSession, NewTerminalSession, PermissionMode, SessionKind,
    SessionManager, SessionRecord, SessionStatus, TerminalShell,
};
use skill_center::{
    DeploySkillInput, DiscoveredSkill, ImportSkillInput, InstallSkillMarketInput,
    SaveSkillMarketRepositoryInput, SaveSkillSetInput, SkillAsset, SkillCenter,
    SkillCenterSnapshot, SkillDeployment, SkillMarketBrowseResult, SkillMarketRepository,
    SkillPreview, SkillSet, SkillsShSearchResult,
};
use telemetry::RuntimeTelemetrySnapshot;
use transcript::TranscriptMessage;
use usage::{SessionUsage, UsageTracker};

pub fn run_session_hook(events_dir: &str, app_session_id: Option<&str>) -> anyhow::Result<()> {
    sessions::run_hook(std::path::Path::new(events_dir), app_session_id)
}

pub fn run_agent_turn_hook(
    events_dir: &str,
    app_session_id: &str,
    payload: &str,
) -> anyhow::Result<()> {
    sessions::run_agent_turn_hook(std::path::Path::new(events_dir), app_session_id, payload)
}

pub fn run_session_statusline(events_dir: &str, app_session_id: &str) -> anyhow::Result<String> {
    use std::io::Read;
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    telemetry::store_claude_statusline(std::path::Path::new(events_dir), app_session_id, &payload)
}

pub use messaging::{parse_send_args, run_message_status, run_send_args, run_send_command, run_sessions_command, run_thread_args};

#[tauri::command]
fn test_profile_info() -> Option<test_profile::Profile> { test_profile::current().cloned() }
pub use session_mcp::run_stdio as run_session_message_mcp;
pub use ai_memory::embedded::run_stdio as run_project_memory_mcp;

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Slow filesystem, database and CLI work must leave both the window thread
/// and Tokio's event-loop workers available for input, cancellation and IPC.
async fn blocking_command<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation).await.map_err(to_err)?
}

#[tauri::command]
fn get_runtime_preferences() -> Result<runtime_preferences::RuntimePreferences, String> {
    runtime_preferences::get().map_err(to_err)
}

#[tauri::command]
fn set_runtime_launch_default(kind: String, mode: String, app: AppHandle) -> Result<runtime_preferences::RuntimePreferences, String> {
    let preferences = runtime_preferences::set_launch_default(&kind, &mode).map_err(to_err)?;
    if let Err(error) = app.emit("runtime-preferences-changed", &preferences) {
        eprintln!("[crewvela] 启动默认值已保存，但窗口通知失败：{error}");
    }
    Ok(preferences)
}

#[tauri::command]
fn set_runtime_enabled(
    kind: AgentRuntimeKind,
    enabled: bool,
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    ptys: State<'_, PtyManager>,
    store: State<'_, MultiAgentStore>,
) -> Result<runtime_preferences::RuntimePreferences, String> {
    let preferences = runtime_preferences::set_enabled(kind, enabled, || {
        for record in sessions.list().into_iter().chain(sessions.list_archived()) {
            if runtime_preferences::session_runtime(&record)? == Some(kind)
                && record.pty_id.is_some_and(|pty_id| ptys.is_running(pty_id))
            {
                return Ok(true);
            }
        }
        for project in store.list_projects()? {
            let snapshot = store.snapshot(&project.id)?;
            if snapshot.runs.iter().any(|run| {
                matches!(
                    run.status,
                    multi_agent::RunStatus::Queued
                        | multi_agent::RunStatus::Starting
                        | multi_agent::RunStatus::Running
                        | multi_agent::RunStatus::WaitingInput
                        | multi_agent::RunStatus::WaitingApproval
                ) && snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.id == run.agent_id && agent.runtime_kind == kind)
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    })
    .map_err(to_err)?;
    app.emit("runtime-preferences-changed", &preferences)
        .map_err(to_err)?;
    Ok(preferences)
}

/// 把首轮对话完成后的摘要请求派发给后台任务，用会话自己的 provider 生成标题。
/// 任何一步失败都静默跳过——截断标题已经就位。
fn flush_title_summaries(app: &AppHandle, session_state: &SessionManager) {
    static LAST_NATIVE_TITLE_READ: AtomicU64 = AtomicU64::new(0);
    let now = automation::now_millis();
    let last = LAST_NATIVE_TITLE_READ.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 3000 && LAST_NATIVE_TITLE_READ.compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        let state = session_state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            for request in state.native_title_candidates() {
                if let Ok(Some(title)) = native_session_titles::read(&request) {
                    if let Err(error) = state.apply_title_request(&request, &title) { eprintln!("[crewvela] 保存原生 CLI 标题失败: {error:#}"); }
                }
            }
        });
    }
    for request in session_state.drain_summary_requests() {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let session_id = request.session_id.clone();
            let apply_request = request.clone();
            let outcome =
                tauri::async_runtime::spawn_blocking(move || summarize_title_request(&request))
                    .await;
            if let Ok(Ok(title)) = outcome {
                eprintln!("[crewvela] 摘要成功 session={session_id} title={title}");
                let state = app_handle.state::<SessionManager>();
                if let Err(error) = state.apply_title_request(&apply_request, &title) {
                    eprintln!("[crewvela] 应用摘要标题失败 session={session_id} err={error:#}");
                }
            } else if let Ok(Err(error)) = outcome {
                eprintln!("[crewvela] 摘要生成失败 session={session_id} err={error:#}");
            } else if let Err(error) = outcome {
                eprintln!("[crewvela] 摘要任务异常 session={session_id} err={error}");
            }
        });
    }
}

fn summarize_title_request(request: &sessions::SummaryRequest) -> anyhow::Result<String> {
    if let Some(kind) = request.runtime_kind.as_deref() {
        if let Ok(Some(title)) = native_session_titles::read(request) { return Ok(title); }
        if matches!(kind, "opencode" | "opencode_chat") {
            // 原生标题任务异步落盘，不另开 Agent，也不把输入截断冒充摘要。
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Ok(Some(title)) = native_session_titles::read(request) { return Ok(title); }
            }
            return Err(anyhow::anyhow!("原生标题任务尚未生成有效标题；保留当前名称"));
        }
        return title_summary::summarize_runtime(kind, &request.prompt, None);
    }
    let provider = providers::get(&request.provider_id)?
        .ok_or_else(|| anyhow::anyhow!("未找到 provider: {}", request.provider_id))?;
    let key = String::new();
    title_summary::summarize_title(&provider, &key, &request.prompt)
}

fn should_stop_sessions_for_window(label: &str) -> bool {
    label == "main"
}

const CONTROL_WINDOW_LABEL: &str = "control";
#[derive(Default)]
struct ControlWindowState {
    dock_requested: AtomicBool,
}

#[tauri::command]
async fn open_control_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(CONTROL_WINDOW_LABEL) {
        window.show().map_err(to_err)?;
        window.set_focus().map_err(to_err)?;
        return Ok(());
    }

    WebviewWindowBuilder::new(
        &app,
        CONTROL_WINDOW_LABEL,
        WebviewUrl::App("index.html?view=control".into()),
    )
    .title("CrewVela · 总控会话")
    .inner_size(440.0, 720.0)
    .min_inner_size(360.0, 520.0)
    .resizable(true)
    .center()
    .build()
    .map_err(to_err)?;
    Ok(())
}

#[tauri::command]
fn close_control_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(CONTROL_WINDOW_LABEL) {
        window.close().map_err(to_err)?;
    }
    Ok(())
}

#[tauri::command]
fn dock_control_window(app: AppHandle, state: State<'_, ControlWindowState>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(CONTROL_WINDOW_LABEL) {
        window.close().map_err(to_err)?;
    }
    state.dock_requested.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
fn take_control_dock_request(state: State<'_, ControlWindowState>) -> bool {
    state.dock_requested.swap(false, Ordering::AcqRel)
}

#[tauri::command]
fn is_control_window_open(app: AppHandle) -> bool {
    app.get_webview_window(CONTROL_WINDOW_LABEL).is_some()
}

const NOTIFICATION_WINDOW_LABEL: &str = "notification-overlay";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationOverlayPayload {
    title: String,
    body: String,
    url: Option<String>,
    revision: u64,
}

#[derive(Default)]
struct NotificationOverlayState {
    current: std::sync::Mutex<Option<NotificationOverlayPayload>>,
    revision: AtomicU64,
}

fn position_notification_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = window
        .app_handle()
        .get_webview_window("main")
        .map(|main| main.current_monitor())
        .transpose()
        .map_err(to_err)?
        .flatten()
        .or(window.primary_monitor().map_err(to_err)?)
        .ok_or_else(|| "无法确定通知显示器".to_string())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let size = &area.size;
    let origin = &area.position;
    let width = (520.0 * scale).round() as i32;
    let height = (176.0 * scale).round() as i32;
    let margin = (22.0 * scale).round() as i32;
    window
        .set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            width as u32,
            height as u32,
        )))
        .map_err(to_err)?;
    let outer = window.outer_size().map_err(to_err)?;
    window
        .set_position(Position::Physical(PhysicalPosition::new(
            origin.x + size.width as i32 - outer.width as i32 - margin,
            origin.y + size.height as i32 - outer.height as i32 - margin,
        )))
        .map_err(to_err)
}

fn notification_initialization_script(
    payload: &NotificationOverlayPayload,
) -> Result<String, String> {
    let json = serde_json::to_string(payload).map_err(to_err)?;
    Ok(format!("window.__CREWVELA_NOTIFICATION__ = {json};"))
}
#[tauri::command]
async fn show_notification_overlay(
    app: AppHandle,
    state: State<'_, NotificationOverlayState>,
    title: String,
    body: String,
    url: Option<String>,
) -> Result<bool, String> {
    let revision = state.revision.fetch_add(1, Ordering::AcqRel) + 1;
    let payload = NotificationOverlayPayload {
        title,
        body,
        url,
        revision,
    };
    *state.current.lock().map_err(to_err)? = Some(payload.clone());

    if let Some(window) = app.get_webview_window(NOTIFICATION_WINDOW_LABEL) {
        window
            .emit("notification-overlay-updated", payload)
            .map_err(to_err)?;
        position_notification_window(&window)?;
        return Ok(true);
    }

    let initialization_script = notification_initialization_script(&payload)?;
    let window = WebviewWindowBuilder::new(
        &app,
        NOTIFICATION_WINDOW_LABEL,
        WebviewUrl::App("index.html#notification".into()),
    )
    .initialization_script(initialization_script)
    .title("通知")
    .inner_size(520.0, 176.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .shadow(true)
    .build()
    .map_err(|error| {
        let message = format!("notification window build failed: {error}");
        eprintln!("[crewvela] {message}");
        let _ = std::fs::write(
            std::env::temp_dir().join("crewvela-notification-error.log"),
            &message,
        );
        message
    })?;
    position_notification_window(&window)?;
    Ok(true)
}

#[tauri::command]
fn notification_overlay_current(
    state: State<'_, NotificationOverlayState>,
) -> Result<Option<NotificationOverlayPayload>, String> {
    state
        .current
        .lock()
        .map(|value| value.clone())
        .map_err(to_err)
}

#[tauri::command]
fn notification_overlay_ready(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(NOTIFICATION_WINDOW_LABEL) {
        position_notification_window(&window)?;
        window.show().map_err(to_err)?;
    }
    Ok(())
}

#[tauri::command]
fn close_notification_overlay(
    app: AppHandle,
    state: State<'_, NotificationOverlayState>,
) -> Result<(), String> {
    *state.current.lock().map_err(to_err)? = None;
    if let Some(window) = app.get_webview_window(NOTIFICATION_WINDOW_LABEL) {
        window.close().map_err(to_err)?;
    }
    Ok(())
}

#[tauri::command]
fn focus_main_from_notification(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(to_err)?;
        window.unminimize().map_err(to_err)?;
        window.set_focus().map_err(to_err)?;
    }
    Ok(())
}

// ---- provider 配置 ----

// Provider config and keychain mutations form one operation. In particular,
// deletion must not race a save that introduces another reference to its key.
static PROVIDER_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[tauri::command]
async fn notification_history_list(
    state: State<'_, NotificationHistoryStore>,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.list().map_err(to_err)
    }).await
}

#[tauri::command]
async fn notification_history_append(
    record: NotificationHistoryRecord,
    state: State<'_, NotificationHistoryStore>,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.append(&record).map_err(to_err)?;
        state.list().map_err(to_err)
    }).await
}

#[tauri::command]
async fn notification_history_import(
    records: Vec<NotificationHistoryRecord>,
    state: State<'_, NotificationHistoryStore>,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.import(&records).map_err(to_err)?;
        state.list().map_err(to_err)
    }).await
}

#[tauri::command]
async fn notification_history_mark_all_read(
    state: State<'_, NotificationHistoryStore>,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.mark_all_read().map_err(to_err)?;
        state.list().map_err(to_err)
    }).await
}

#[tauri::command]
async fn notification_history_clear(state: State<'_, NotificationHistoryStore>) -> Result<(), String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.clear().map_err(to_err)
    }).await
}

#[tauri::command]
fn send_windows_unpacked_notification(
    title: String,
    body: String,
    system_sound: bool,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        return windows_notification::send_if_unpacked(&title, &body, system_sound).map_err(to_err);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, body, system_sound);
        Ok(false)
    }
}
// ---- 项目（会话分组） ----

#[tauri::command]
fn list_projects() -> Result<Vec<Project>, String> {
    projects::list().map_err(to_err)
}

#[tauri::command]
fn create_project(name: Option<String>, path: String) -> Result<Project, String> {
    projects::create(name.as_deref(), &path).map_err(to_err)
}

#[tauri::command]
fn update_project(
    id: String,
    name: Option<String>,
    badge_color: Option<String>,
) -> Result<Project, String> {
    projects::update(&id, name.as_deref(), badge_color.as_deref()).map_err(to_err)
}

/// 删除项目只解除分组：项目下的会话保留并回到未分组。
#[tauri::command]
fn delete_project(id: String, session_state: State<'_, SessionManager>,
    services: State<'_, multi_agent::project_preview::LocalProjectServices>) -> Result<(), String> {
    services.delete(&id).map_err(to_err)?;
    session_state.clear_project(&id).map_err(to_err)?;
    Ok(())
}

#[tauri::command]
async fn edit_local_project(id: String, edit: projects::ProjectEdit, store: State<'_, MultiAgentStore>,
    services: State<'_, multi_agent::project_preview::LocalProjectServices>) -> Result<Project, String> {
    let store = store.inner().clone();
    let services = services.inner().clone();
    blocking_command(move || {
        if let projects::ProjectEdit::Collaboration { ids } = &edit {
            let available = store.list_projects().map_err(to_err)?;
            if ids.iter().any(|id| !available.iter().any(|project| &project.id == id)) {
                return Err("协作项目已变化，请重新选择".into());
            }
        }
        services.edit(&id, edit).map_err(to_err)
    }).await
}

#[tauri::command]
async fn local_project_service_start(id: String, recheck: Option<bool>, app: AppHandle, store: State<'_, MultiAgentStore>,
    services: State<'_, multi_agent::project_preview::LocalProjectServices>) -> Result<multi_agent::project_preview::PreviewStatus, String> {
    let _admission = app.state::<runtime_admission::RuntimeAdmission>().admit().map_err(to_err)?;
    let services = services.inner().clone();
    let store = store.inner().clone();
    blocking_command(move || services.start(&store, &id, recheck.unwrap_or(false)).map_err(to_err)).await
}

#[tauri::command]
async fn local_project_service_stop(id: String,
    services: State<'_, multi_agent::project_preview::LocalProjectServices>) -> Result<multi_agent::project_preview::PreviewStatus, String> {
    let services = services.inner().clone();
    blocking_command(move || Ok(services.stop(&id))).await
}

#[tauri::command]
async fn local_project_service_logs(id: String,
    services: State<'_, multi_agent::project_preview::LocalProjectServices>) -> Result<multi_agent::project_preview::PreviewStatus, String> {
    let services = services.inner().clone();
    blocking_command(move || Ok(services.status(&id))).await
}

#[tauri::command]
async fn local_project_service_statuses(
    services: State<'_, multi_agent::project_preview::LocalProjectServices>,
) -> Result<std::collections::HashMap<String, multi_agent::project_preview::PreviewStatus>, String> {
    let services = services.inner().clone();
    blocking_command(move || Ok(services.statuses())).await
}

// ---- AI 创作工作台（独立于现有会话分组项目） ----


#[tauri::command]
fn list_launch_presets() -> Result<Vec<LaunchPreset>, String> {
    presets::list().map_err(to_err)
}

#[tauri::command]
fn create_launch_preset(
    name: String,
    provider_id: String,
    cwds: Vec<String>,
    yolo: bool,
    auto_mode: bool,
    use_worktree: bool,
    initial_prompt: Option<String>,
) -> Result<LaunchPreset, String> {
    presets::create(presets::NewPreset {
        name,
        provider_id,
        cwds,
        yolo,
        auto_mode,
        use_worktree,
        initial_prompt,
    })
    .map_err(to_err)
}

#[tauri::command]
fn delete_launch_preset(id: String) -> Result<(), String> {
    presets::delete(&id).map_err(to_err)
}

// ---- API key（keychain） ----
// ---- claude 路径 ----

#[tauri::command]
fn claude_path() -> Result<String, String> {
    launcher::resolve_claude()
        .map(|p| p.display().to_string())
        .map_err(to_err)
}

#[tauri::command]
fn paste_clipboard_image(app: AppHandle) -> Result<Option<String>, String> {
    clipboard_image::write_clipboard_image_to_temp(&app).map_err(to_err)
}

#[tauri::command]
fn clipboard_has_image(app: AppHandle) -> bool {
    clipboard_image::clipboard_has_image(&app)
}

/// 剪贴板 CF_HDROP 文件列表（资源管理器复制的原始文件路径），供聊天输入框粘贴附件直引。
#[tauri::command]
fn clipboard_file_paths() -> Vec<String> {
    clipboard_image::clipboard_file_paths()
}

/// WebView 粘贴/拖放给出的文件字节兜底落盘：系统剪贴板直读不可用时由前端整包传入。
#[tauri::command]
fn write_pasted_files_to_temp(
    files: Vec<clipboard_image::PastedFilePayload>,
) -> Result<Vec<String>, String> {
    clipboard_image::write_pasted_files_to_temp(files).map_err(to_err)
}

#[tauri::command]
fn codex_quota_monitor_status(
    state: State<'_, CodexQuotaMonitor>,
) -> Result<CodexQuotaMonitorStatus, String> {
    state.status().map_err(to_err)
}

#[tauri::command]
fn list_codex_quota_notifications(
    state: State<'_, CodexQuotaMonitor>,
) -> Result<Vec<StoredQuotaNotification>, String> {
    state.notifications().map_err(to_err)
}

#[tauri::command]
async fn cid_list_accounts() -> Result<codex_accounts::CodexAccountSnapshot, String> {
    tauri::async_runtime::spawn_blocking(codex_accounts::list)
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn cid_init_accounts() -> Result<codex_accounts::CodexAccountActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(codex_accounts::init)
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn cid_add_account() -> Result<codex_accounts::CodexAccountActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(codex_accounts::add)
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn cid_add_token(token: String) -> Result<codex_accounts::CodexAccountActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || codex_accounts::add_token(token))
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn cid_use_account(
    selector: Option<String>,
) -> Result<codex_accounts::CodexAccountActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || codex_accounts::use_account(selector))
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn cid_remove_account(
    selector: String,
) -> Result<codex_accounts::CodexAccountActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || codex_accounts::remove(selector))
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn restart_codex_app() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(codex_accounts::restart_codex_app)
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn check_codex_quota_updates(
    state: State<'_, CodexQuotaMonitor>,
) -> Result<CodexQuotaCheckReceipt, String> {
    state.check().await.map_err(to_err)
}

#[tauri::command]
fn clipboard_image_file() -> Result<Option<clipboard_image::ClipboardImageFile>, String> {
    clipboard_image::read_clipboard_image_file().map_err(to_err)
}

#[tauri::command]
fn paste_dropped_image(
    app: AppHandle,
    file_name: String,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    clipboard_image::write_dropped_image_to_clipboard(&app, &file_name, rgba, width, height)
        .map_err(to_err)
}

// ---- Multi-Agent 控制平面 ----

#[tauri::command]
async fn ma_probe_runtimes() -> Result<Vec<RuntimeProbe>, String> {
    tauri::async_runtime::spawn_blocking(probe_all_runtimes)
        .await
        .map_err(to_err)
}

#[tauri::command]
async fn ma_check_runtime_updates() -> Result<Vec<RuntimeProbe>, String> {
    tauri::async_runtime::spawn_blocking(check_runtime_updates)
        .await
        .map_err(to_err)
}

#[tauri::command]
async fn ma_upgrade_runtime(kind: AgentRuntimeKind) -> Result<RuntimeUpgradeReceipt, String> {
    if cfg!(windows) && matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex | AgentRuntimeKind::OpenCode | AgentRuntimeKind::KimiCode | AgentRuntimeKind::Pi) {
        let started = runtime_installation::start(kind)?;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let job = runtime_installation::status(kind).filter(|job| job.id == started.id).ok_or("安装状态丢失")?;
            match job.phase.as_str() {
                "ready" => {
                    let runtime = tauri::async_runtime::spawn_blocking(move || probe_all_runtimes().into_iter().find(|probe| probe.kind == kind)).await.map_err(to_err)?.ok_or("Runtime 检测结果缺失")?;
                    return Ok(RuntimeUpgradeReceipt { kind, output: job.message, runtime });
                }
                "failed" | "cancelled" => return Err(job.error.unwrap_or(job.message)),
                _ => {}
            }
        }
    }
    tauri::async_runtime::spawn_blocking(move || upgrade_runtime(kind))
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn runtime_install_environment(kind: AgentRuntimeKind) -> Result<runtime_installation::Environment, String> { runtime_installation::environment(kind).await }
#[tauri::command]
async fn runtime_set_zcode_cli_path(path: Option<std::path::PathBuf>) -> Result<runtime_installation::Environment, String> {
    blocking_command(move || runtime_installation::zcode_cli::configure(path).map_err(to_err)).await
}
#[tauri::command]
async fn runtime_chat_dependencies() -> Result<HashMap<String, runtime_installation::ChatDependencySummary>, String> { runtime_installation::chat_dependencies().await }
#[tauri::command]
async fn runtime_install_latest_version(kind: AgentRuntimeKind) -> Result<String, String> { runtime_installation::latest_version(kind).await }
#[tauri::command]
fn runtime_install_start(kind: AgentRuntimeKind) -> Result<runtime_installation::InstallJob, String> { runtime_installation::start(kind) }
#[tauri::command]
fn runtime_install_status(kind: AgentRuntimeKind) -> Option<runtime_installation::InstallJob> { runtime_installation::status(kind) }
#[tauri::command]
fn runtime_install_cancel(id: String) -> Result<(), String> { runtime_installation::cancel(&id) }
#[tauri::command]
fn runtime_open_login(kind: AgentRuntimeKind) -> Result<(), String> { runtime_installation::open_login(kind) }

#[tauri::command]
async fn ma_list_projects(state: State<'_, MultiAgentStore>) -> Result<Vec<MultiAgentProject>, String> {
    let store = state.inner().clone();
    blocking_command(move || store.list_projects().map_err(to_err)).await
}

#[tauri::command]
async fn ma_list_project_activity(state: State<'_, MultiAgentStore>) -> Result<Vec<multi_agent::CollaborationProjectActivity>, String> {
    let store = state.inner().clone();
    blocking_command(move || store.list_project_activity().map_err(to_err)).await
}

#[tauri::command]
fn ma_list_interventions(
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<InterventionRecord>, String> {
    CollaborationGovernance::new(state.inner().clone())
        .list_interventions()
        .map_err(to_err)
}

#[tauri::command]
fn ma_resolve_intervention(
    input: ResolveInterventionInput,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
    pty: State<'_, PtyManager>,
    sessions: State<'_, SessionManager>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<InterventionResolutionReceipt, String> {
    let mut receipt = CollaborationGovernance::new(store.inner().clone())
        .resolve(input)
        .map_err(to_err)?;
    if receipt.resume_project {
        let execution = supervisor
            .start(
                store.inner().clone(),
                receipt.project_id.clone(),
                multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()),
                collaboration_bridge.inner().clone(),
            )
            .map_err(to_err)?;
        if execution.accepted {
            receipt.message = format!("{}；调度已恢复", receipt.message);
        }
    }
    Ok(receipt)
}

#[tauri::command]
async fn ma_create_project(
    input: CreateProjectInput, git_setup: Option<multi_agent::project_git::ProjectGitPreparation>,
    state: State<'_, MultiAgentStore>,
) -> Result<MultiAgentProject, String> {
    let store = state.inner().clone();
    blocking_command(move || store.create_project_with_git(input, git_setup).map_err(to_err)).await
}

#[tauri::command]
async fn ma_rename_project(project_id: String, name: String, state: State<'_, MultiAgentStore>) -> Result<MultiAgentProject, String> {
    let store = state.inner().clone();
    blocking_command(move || store.rename_project(&project_id, &name).map_err(to_err)).await
}

#[tauri::command]
async fn ma_set_project_reply_preference(project_id: String, agent_id: Option<String>, state: State<'_, MultiAgentStore>) -> Result<(), String> {
    let store = state.inner().clone();
    blocking_command(move || store.set_project_reply_preference(&project_id, agent_id.as_deref()).map_err(to_err)).await
}

#[tauri::command]
async fn ma_project_git_status(root_path: String) -> Result<multi_agent::project_git::ProjectGitStatus, String> {
    blocking_command(move || multi_agent::project_git::inspect(std::path::Path::new(&root_path)).map_err(to_err)).await
}

#[tauri::command]
fn ma_member_library(state: State<'_, MultiAgentStore>) -> Result<multi_agent::member_library::LibrarySnapshot, String> {
    state.library_snapshot().map_err(to_err)
}

#[tauri::command]
fn ma_member_library_import(input: multi_agent::member_library::LibraryImport, state: State<'_, MultiAgentStore>) -> Result<multi_agent::member_library::LibraryApplication, String> {
    let _lifecycle = state.agent_lifecycle_guard();
    state.import_default_team(input).map_err(to_err)
}

#[tauri::command]
fn ma_member_library_apply(project_id: String, state: State<'_, MultiAgentStore>) -> Result<multi_agent::member_library::LibraryApplication, String> {
    state.apply_default_team(&project_id).map_err(to_err)
}

#[tauri::command]
fn ma_member_library_auto_apply(enabled: bool, state: State<'_, MultiAgentStore>) -> Result<(), String> {
    state.set_library_auto_apply(enabled).map_err(to_err)
}

#[tauri::command]
fn ma_member_library_update(
    member_id: String, profile: multi_agent::member_library::LibraryProfile,
    state: State<'_, MultiAgentStore>, sessions: State<'_, SessionManager>,
) -> Result<(), String> {
    let _lifecycle = state.agent_lifecycle_guard();
    for agent_id in state.library_linked_agent_ids(&member_id).map_err(to_err)? {
        ensure_member_terminal_idle(&sessions, &agent_id)?;
    }
    state.update_library_member(&member_id, profile).map_err(to_err)
}

#[tauri::command]
async fn ma_import_agent_avatar(path: String, state: State<'_, MultiAgentStore>) -> Result<String, String> {
    let store = state.inner().clone();
    blocking_command(move || store.import_agent_avatar(std::path::Path::new(&path)).map_err(to_err)).await
}

#[tauri::command]
async fn ma_agent_avatar_image(avatar_id: String, state: State<'_, MultiAgentStore>) -> Result<String, String> {
    let store = state.inner().clone();
    blocking_command(move || store.agent_avatar_image(&avatar_id).map_err(to_err)).await
}

#[tauri::command]
fn ma_create_agent(
    input: CreateAgentInput,
    state: State<'_, MultiAgentStore>,
) -> Result<AgentProfile, String> {
    state.create_agent(input).map_err(to_err)
}

fn ensure_member_terminal_idle(sessions: &SessionManager, agent_id: &str) -> Result<(), String> {
    for session in sessions
        .list()
        .iter()
        .filter(|session| session.multi_agent_agent_id.as_deref() == Some(agent_id))
    {
        if session.pty_id.is_some()
            && !session.runtime_command_host
            && matches!(
                session.status,
                SessionStatus::Running | SessionStatus::Waiting | SessionStatus::Pending
            )
            && !sessions.is_ready_for_prompt(&session.id)
        {
            return Err("成员终端正在执行或尚未就绪，请等待其空闲或停止会话后再编辑".into());
        }
    }
    Ok(())
}

fn stop_member_runtime_ptys(sessions: &SessionManager, pty: &PtyManager, project_id: &str, agent_id: &str) -> Result<(), String> {
    for old in sessions.list().into_iter().filter(|record| record.multi_agent_project_id.as_deref() == Some(project_id) && record.multi_agent_agent_id.as_deref() == Some(agent_id)) {
        let current = sessions.get(&old.id).ok_or_else(|| "成员会话已删除".to_string())?;
        if let Some(id) = current.pty_id { pty.kill(id).map_err(to_err)?; }
        pty.kill_for_app_session(&current.id).map_err(to_err)?;
    }
    Ok(())
}

#[tauri::command]
fn ma_remove_agent(
    project_id: String, agent_id: String, confirm: bool,
    state: State<'_, MultiAgentStore>, sessions: State<'_, SessionManager>,
) -> Result<Vec<String>, String> {
    let _lifecycle = state.agent_lifecycle_guard();
    let blockers = state.remove_agent(&project_id, &agent_id, false).map_err(to_err)?;
    if !blockers.is_empty() { return Ok(blockers); }
    if let Err(reason) = ensure_member_terminal_idle(&sessions, &agent_id) { return Ok(vec![reason]); }
    if confirm { state.remove_agent(&project_id, &agent_id, true).map_err(to_err) }
    else { Ok(blockers) }
}

#[tauri::command]
fn ma_update_agent(
    input: UpdateAgentInput,
    state: State<'_, MultiAgentStore>,
    sessions: State<'_, SessionManager>,
    pty: State<'_, PtyManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<AgentProfile, String> {
    let _lifecycle = state.agent_lifecycle_guard();
    ensure_member_terminal_idle(&sessions, &input.agent_id)?;
    let previous = state.agent_profile(&input.agent_id).map_err(to_err)?;
    if previous.runtime_kind != input.runtime_kind && !input.runtime_kind.is_collaboration_cli() {
        return Err("切换协作成员请选择 CLI 执行引擎；ZCode Desktop 只保留已有成员兼容".into());
    }
    // Hold all old session lifecycle locks through profile mutation/retirement,
    // so a UI resume cannot attach another old-runtime PTY after cleanup.
    let mut old_ids: Vec<_> = if previous.runtime_kind != input.runtime_kind {
        sessions.list().into_iter().filter(|record| record.multi_agent_project_id.as_deref() == Some(&input.project_id) && record.multi_agent_agent_id.as_deref() == Some(&input.agent_id)).map(|record| record.id).collect()
    } else { Vec::new() };
    old_ids.sort();
    let old_lifecycles: Vec<_> = old_ids.iter().map(|id| sessions.lifecycle_lock(id)).collect();
    let _old_guards: Vec<_> = old_lifecycles.iter().map(|lock| lock.lock().unwrap()).collect();
    if previous.runtime_kind != input.runtime_kind {
        input.runtime_kind.ensure_available().map_err(to_err)?;
        stop_member_runtime_ptys(&sessions, &pty, &input.project_id, &input.agent_id)?;
    }
    let profile = state.update_agent(input).map_err(to_err)?;
    if previous.runtime_kind != profile.runtime_kind {
        let switch_result = (|| -> Result<(), String> {
            let retired = sessions
                .retire_agent_runtime_sessions(&profile.project_id, &profile.id)
                .map_err(to_err)?;
            for old in retired {
                action_queue.fail_retired_session(&old.id);
            }
            let project = state.snapshot(&profile.project_id).map_err(to_err)?.project;
            let fresh = sessions
                .create_agent(sessions::NewAgentSession {
                    avatar_id: profile.avatar_id.clone(),
                    project_id: profile.project_id.clone(),
                    agent_id: profile.id.clone(),
                    runtime_kind: profile.runtime_kind.as_str().into(),
                    agent_name: profile.name.clone(),
                    cwd: project.root_path,
                })
                .map_err(to_err)?;
            state
                .link_agent_app_session(&profile.project_id, &profile.id, &fresh.id)
                .map_err(to_err)?;
            Ok(())
        })();
        switch_result.map_err(|error| format!("成员配置已保存，但 Runtime 会话切换未完成：{error}。请停止旧终端后重新启动成员；历史记录保留。"))?;
    }
    sessions
        .sync_agent_profile(&profile)
        .map_err(|reason| format!("成员已保存，但会话信息同步失败：{reason}"))?;
    Ok(profile)
}

#[tauri::command]
fn ma_create_task(
    input: CreateTaskInput,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::MultiAgentTask, String> {
    state.create_task(input).map_err(to_err)
}

#[tauri::command]
fn ma_add_dependency(
    input: AddDependencyInput,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::MultiAgentTask, String> {
    state.add_dependency(input).map_err(to_err)
}

#[tauri::command]
fn ma_claim_task(
    input: ClaimTaskInput,
    state: State<'_, MultiAgentStore>,
) -> Result<AgentRun, String> {
    state.claim_task(input).map_err(to_err)
}

#[tauri::command]
fn ma_confirm_run_stopped(run_id: String, confirmed_stopped: bool, state: State<'_, MultiAgentStore>) -> Result<AgentRun, String> {
    state.confirm_run_stopped(&run_id, confirmed_stopped).map_err(to_err)
}

#[tauri::command]
fn ma_update_run(
    input: UpdateRunInput,
    state: State<'_, MultiAgentStore>,
) -> Result<AgentRun, String> {
    state.update_run(input).map_err(to_err)
}

#[tauri::command]
fn ma_enqueue_wakeup(
    input: EnqueueWakeupInput,
    state: State<'_, MultiAgentStore>,
) -> Result<WakeupRequest, String> {
    state.enqueue_wakeup(input).map_err(to_err)
}

#[tauri::command]
async fn ma_snapshot(
    project_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<MultiAgentSnapshot, String> {
    let store = state.inner().clone();
    blocking_command(move || store.snapshot(&project_id).map_err(to_err)).await
}

#[tauri::command]
fn ma_clear_conversation(
    project_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<usize, String> {
    state
        .clear_coordinator_messages(&project_id)
        .map_err(to_err)
}

#[tauri::command]
fn ma_run_logs(
    project_id: String,
    after_seq: Option<i64>,
    limit: u32,
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<multi_agent::RunLogEntry>, String> {
    state
        .run_logs(&project_id, after_seq, limit)
        .map_err(to_err)
}

#[tauri::command]
async fn ma_run_log_history(project_id: String, run_id: String, limit: u32, state: State<'_, MultiAgentStore>) -> Result<Vec<multi_agent::RunLogEntry>, String> {
    let store = state.inner().clone();
    blocking_command(move || store.logs_for_run(&project_id, &run_id, limit).map_err(to_err)).await
}

#[tauri::command]
fn ma_activity_events(
    project_id: String,
    after_seq: Option<i64>,
    limit: u32,
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<multi_agent::ActivityEvent>, String> {
    state
        .activity_events(&project_id, after_seq, limit)
        .map_err(to_err)
}

#[tauri::command]
fn ma_runtime_events(
    project_id: String,
    after_seq: Option<i64>,
    limit: u32,
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<RuntimeEventRecord>, String> {
    state
        .runtime_events(&project_id, after_seq, limit)
        .map_err(to_err)
}

#[tauri::command]
fn ma_send_message(
    input: SendCollaborationMessageInput,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::CollaborationMessage, String> {
    state.send_collaboration_message(input).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_snapshot(project_id: String, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
async fn ma_conversation_member_models(project_id: String, agent_id: String, refresh: Option<bool>, state: State<'_, MultiAgentStore>) -> Result<multi_agent::conversation_models::MemberOptions, String> {
    let store = state.inner().clone();
    blocking_command(move || store.load_conversation_member_models(&project_id, &agent_id, refresh.unwrap_or(false)).map_err(to_err)).await
}

#[tauri::command]
fn ma_set_conversation_member_models(project_id: String, agent_id: String, selection: multi_agent::conversation_models::Selection, state: State<'_, MultiAgentStore>) -> Result<multi_agent::conversation_models::MemberOptions, String> {
    state.set_conversation_member_models(&project_id, &agent_id, selection).map_err(to_err)
}

#[tauri::command]
async fn ma_conversation_export(
    project_id: String,
    conversation_id: String,
    window: tauri::WebviewWindow,
    state: State<'_, MultiAgentStore>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let store = state.inner().clone();
    let (filename, markdown) = blocking_command(move || {
        store.export_conversation_markdown(&project_id, &conversation_id).map_err(to_err)
    }).await?;
    let (selected, selection) = tokio::sync::oneshot::channel();
    window.dialog().file().set_parent(&window)
        .set_title("导出团队会话")
        .set_file_name(filename)
        .add_filter("Markdown", &["md"])
        .save_file(move |path| { let _ = selected.send(path); });
    let Some(path) = selection.await.map_err(|_| "未能打开会话保存窗口")? else { return Ok(None); };
    let path = path.into_path().map_err(to_err)?;
    blocking_command(move || {
        providers::write_atomic(&path, markdown.as_bytes())
            .map_err(|error| format!("保存文件失败（{}）：{}", path.display(), error.root_cause()))?;
        Ok(Some(path.file_name().unwrap_or_default().to_string_lossy().into_owned()))
    }).await
}

#[tauri::command]
async fn ma_conversation_design_delivery(project_id: String, conversation_id: String, delegation_id: String,
    state: State<'_, MultiAgentStore>) -> Result<multi_agent::DesignDelivery, String> {
    let store = state.inner().clone();
    blocking_command(move || store.design_delivery(&project_id, &conversation_id, &delegation_id).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_run_transcript(project_id: String, run_id: String, state: State<'_, MultiAgentStore>) -> Result<Vec<TranscriptMessage>, String> {
    let store = state.inner().clone();
    blocking_command(move || {
        let locator = store.conversation_run_locator(&project_id, &run_id).map_err(to_err)?;
        session_switch_history::read_conversation_run(&locator).map_err(to_err)
    }).await
}

#[tauri::command]
fn ma_conversation_policy_set(input: multi_agent::conversation::ConversationPolicySetInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    state.conversation_policy_set(input).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_migration_preview(input: multi_agent::conversation::ConversationMigrationPreviewInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::conversation::ConversationMigrationPreview, String> {
    state.conversation_migration_preview(input).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_migration_apply(input: multi_agent::conversation::ConversationMigrationApplyInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    state.conversation_migration_apply(input).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_clear(input: multi_agent::ConversationClearInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    state.clear_conversation(input).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_new(input: multi_agent::ConversationClearInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    state.new_conversation(input).map_err(to_err)
}

#[tauri::command]
async fn ma_conversation_history(project_id: String, state: State<'_, MultiAgentStore>) -> Result<Vec<multi_agent::conversation::ConversationHistoryEntry>, String> {
    let store = state.inner().clone();
    blocking_command(move || store.conversation_history(&project_id).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_switch(input: multi_agent::conversation::ConversationSwitchInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    let store = state.inner().clone();
    blocking_command(move || store.switch_conversation(input).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_rename(input: multi_agent::conversation::ConversationRenameInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    let store = state.inner().clone();
    blocking_command(move || store.rename_conversation(input).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_delete(input: multi_agent::ConversationClearInput, state: State<'_, MultiAgentStore>) -> Result<multi_agent::ConversationSnapshot, String> {
    let store = state.inner().clone();
    blocking_command(move || store.delete_conversation(input).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_plan_preflight(
    input: multi_agent::ConversationSendInput, state: State<'_, MultiAgentStore>,
) -> Result<Vec<multi_agent::conversation::ConversationPlanIssue>, String> {
    let store = state.inner().clone();
    blocking_command(move || {
        let mut issues = store.conversation_plan_preflight(&input).map_err(to_err)?;
        if issues.is_empty() && input.managed {
            for id in &input.participant_agent_ids {
                let member = store.agent_profile(id).map_err(to_err)?;
                if member.runtime_kind == multi_agent::AgentRuntimeKind::ZCodeCli {
                    let project = store.project(&input.project_id).map_err(to_err)?;
                    if let Err(error) = multi_agent::zcode::managed_cli_preflight(std::path::Path::new(&project.root_path)) {
                        issues.push(multi_agent::conversation::ConversationPlanIssue {
                            kind: "member",
                            message: format!("{} 的 ZCode CLI 托管预检失败。{error:#}", member.name),
                        });
                    }
                    break;
                }
            }
        }
        if issues.is_empty() { issues.extend(store.plan_auth_preflight(&input).map_err(to_err)?); }
        Ok(issues)
    }).await
}

#[tauri::command]
async fn ma_conversation_codex_login(project_id: String, expected_conversation_id: String, state: State<'_, MultiAgentStore>) -> Result<(), String> {
    let store = state.inner().clone();
    blocking_command(move || {
        if store.conversation_snapshot(&project_id).map_err(to_err)?.conversation.id != expected_conversation_id {
            return Err("当前对话已切换，请重新选择登录操作".into());
        }
        let root = store.project(&project_id).map_err(to_err)?.root_path;
        codex_storage::login_collaboration(std::path::Path::new(&root)).map_err(to_err)
    }).await
}

#[tauri::command]
async fn ma_conversation_retry_codex_members(input: multi_agent::conversation::CodexRetryInput, app: AppHandle) -> Result<multi_agent::ConversationSnapshot, String> {
    blocking_command(move || {
        let _admission = app.state::<runtime_admission::RuntimeAdmission>().admit().map_err(to_err)?;
        let store = app.state::<MultiAgentStore>();
        let project = input.project_id.clone();
        let host = multi_agent::AgentTerminalHost::new(app.state::<PtyManager>().inner().clone(), app.state::<SessionManager>().inner().clone());
        app.state::<multi_agent::ConversationSupervisor>().retry_codex_members(store.inner().clone(), input, host,
            app.state::<CollaborationBridge>().inner().clone(), app.state::<MultiAgentSupervisor>().inner().clone()).map_err(to_err)?;
        store.conversation_snapshot(&project).map_err(to_err)
    }).await
}

#[tauri::command]
async fn ma_conversation_resume_managed_after_login(project_id: String, expected_conversation_id: String, plan_id: String,
    app: AppHandle) -> Result<multi_agent::ConversationSnapshot, String> {
    blocking_command(move || {
        let _admission = app.state::<runtime_admission::RuntimeAdmission>().admit().map_err(to_err)?;
        let store = app.state::<MultiAgentStore>();
        let host = multi_agent::AgentTerminalHost::new(app.state::<PtyManager>().inner().clone(), app.state::<SessionManager>().inner().clone());
        store.resume_managed_after_login(&project_id, &expected_conversation_id, &plan_id, &app.state::<MultiAgentSupervisor>(), &host,
            &app.state::<CollaborationBridge>()).map_err(to_err)?;
        store.conversation_snapshot(&project_id).map_err(to_err)
    }).await
}

#[tauri::command]
async fn ma_conversation_plan_retry_input(
    project_id: String, expected_conversation_id: String, round_id: String, client_message_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::ConversationSendInput, String> {
    let store = state.inner().clone();
    blocking_command(move || store.conversation_plan_retry_input(
        &project_id, &expected_conversation_id, &round_id, client_message_id,
    ).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_stage_attachments(project_id: String, conversation_id: String, paths: Vec<String>,
    state: State<'_, MultiAgentStore>) -> Result<Vec<multi_agent::ConversationAttachment>, String> {
    let store = state.inner().clone();
    blocking_command(move || store.stage_conversation_attachments(&project_id, &conversation_id, &paths).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_preview_image(source: multi_agent::conversation::ConversationAttachmentSource,
    state: State<'_, MultiAgentStore>) -> Result<tauri::ipc::Response, String> {
    let store = state.inner().clone();
    blocking_command(move || store.conversation_image_bytes(source).map(tauri::ipc::Response::new).map_err(to_err)).await
}

#[tauri::command]
async fn ma_conversation_preview_video(source: multi_agent::conversation::ConversationAttachmentSource,
    app: AppHandle, state: State<'_, MultiAgentStore>) -> Result<String, String> {
    let store = state.inner().clone();
    blocking_command(move || {
        let path = store.conversation_video_path(source).map_err(to_err)?;
        app.asset_protocol_scope().allow_file(&path).map_err(to_err)?;
        Ok(path.display().to_string())
    }).await
}

#[tauri::command]
async fn ma_conversation_open_video(source: multi_agent::conversation::ConversationAttachmentSource,
    state: State<'_, MultiAgentStore>) -> Result<(), String> {
    let store = state.inner().clone();
    blocking_command(move || {
        let path = store.conversation_video_path(source).map_err(to_err)?;
        tauri_plugin_opener::open_path(multi_agent::external_command_path(&path), None::<&str>).map_err(to_err)
    }).await
}

#[tauri::command]
fn ma_conversation_open_attachment(project_id: String, conversation_id: String, message_id: String, attachment_id: String,
    state: State<'_, MultiAgentStore>) -> Result<(), String> {
    let path = state.conversation_attachment_path(&project_id, &conversation_id, &message_id, &attachment_id).map_err(to_err)?;
    tauri_plugin_opener::open_path(multi_agent::external_command_path(&path), None::<&str>).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_send(input: multi_agent::ConversationSendInput, template_trial: Option<bool>,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
    pty: State<'_, PtyManager>, sessions: State<'_, SessionManager>, bridge: State<'_, CollaborationBridge>,
    engineering: State<'_, MultiAgentSupervisor>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    let _admission = admission.admit().map_err(to_err)?;
    let project_id = input.project_id.clone();
    let host = multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone());
    if template_trial == Some(true) {
        supervisor.send_template(state.inner().clone(), input, host, bridge.inner().clone(), false)
    } else {
        supervisor.send(state.inner().clone(), input, host, bridge.inner().clone(), engineering.inner().clone())
    }.map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
async fn ma_project_preview_save(project_id: String, plan_id: String, config: multi_agent::project_preview::PreviewConfig,
    store: State<'_, MultiAgentStore>) -> Result<(), String> {
    let store = store.inner().clone();
    blocking_command(move || store.save_project_preview(&project_id, &plan_id, config).map_err(to_err)).await
}

#[tauri::command]
async fn ma_project_preview_start(app: AppHandle, project_id: String, plan_id: String, confirmed_version: Option<String>) -> Result<multi_agent::project_preview::PreviewStart, String> {
    blocking_command(move || app.state::<multi_agent::project_preview::ProjectPreviews>()
        .start(&app.state::<MultiAgentStore>(), &project_id, &plan_id, confirmed_version).map_err(to_err)).await
}

#[tauri::command]
async fn ma_project_preview_status(app: AppHandle, project_id: String, plan_id: String, logs: bool) -> Result<multi_agent::project_preview::PreviewStatus, String> {
    blocking_command(move || Ok(app.state::<multi_agent::project_preview::ProjectPreviews>().status(&project_id, &plan_id, logs))).await
}

#[tauri::command]
async fn ma_project_preview_stop(app: AppHandle, project_id: String, plan_id: String) -> Result<multi_agent::project_preview::PreviewStatus, String> {
    blocking_command(move || Ok(app.state::<multi_agent::project_preview::ProjectPreviews>().stop(&project_id, &plan_id))).await
}

#[tauri::command]
fn ma_conversation_queue(input: multi_agent::ConversationSendInput,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
    pty: State<'_, PtyManager>, sessions: State<'_, SessionManager>, bridge: State<'_, CollaborationBridge>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    let _admission = admission.admit().map_err(to_err)?;
    let project_id = input.project_id.clone();
    supervisor.queue_chat(state.inner().clone(), input,
        multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()), bridge.inner().clone()).map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_change_queued(project_id: String, conversation_id: String, message_id: String,
    expected_updated_at: i64, input: Option<multi_agent::ConversationSendInput>, hold: Option<bool>,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
    pty: State<'_, PtyManager>, sessions: State<'_, SessionManager>, bridge: State<'_, CollaborationBridge>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    let _admission = admission.admit().map_err(to_err)?;
    supervisor.change_queued_chat(state.inner().clone(), &project_id, &conversation_id, &message_id, expected_updated_at, input, hold,
        multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()), bridge.inner().clone()).map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command(async)]
fn ma_conversation_steer_queued(project_id: String, conversation_id: String, message_id: String,
    expected_updated_at: i64, run_id: String,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    let _admission = admission.admit().map_err(to_err)?;
    supervisor.steer_queued_chat(state.inner(), &project_id, &conversation_id, &message_id, expected_updated_at, &run_id).map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_interrupt(input: multi_agent::ConversationInterruptInput,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
    pty: State<'_, PtyManager>, sessions: State<'_, SessionManager>, bridge: State<'_, CollaborationBridge>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    let _admission = admission.admit().map_err(to_err)?;
    let project_id = input.project_id.clone();
    supervisor.interrupt(state.inner(), input,
        multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()), bridge.inner().clone()).map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
fn ma_conversation_control(project_id: String, action: String, expected_conversation_id: Option<String>,
    state: State<'_, MultiAgentStore>, supervisor: State<'_, multi_agent::ConversationSupervisor>,
) -> Result<multi_agent::ConversationSnapshot, String> {
    supervisor.control(state.inner(), &project_id, &action, expected_conversation_id.as_deref()).map_err(to_err)?;
    state.conversation_snapshot(&project_id).map_err(to_err)
}

#[tauri::command]
async fn ma_conversation_execute_plan(input: multi_agent::ConversationExecutePlanInput, app: AppHandle,
) -> Result<multi_agent::ConversationSnapshot, String> {
    blocking_command(move || {
    let _admission = app.state::<runtime_admission::RuntimeAdmission>().admit().map_err(to_err)?;
    let state = app.state::<MultiAgentStore>();
    if !state.plan_accounts_ready(&input).map_err(to_err)? { return Err(codex_storage::LOGIN_REQUIRED.into()); }
    state.apply_conversation_plan(&input).map_err(to_err)?;
    if state.claim_conversation_plan_start(&input).map_err(to_err)? {
        let result = app.state::<MultiAgentSupervisor>().start(state.inner().clone(), input.project_id.clone(),
            multi_agent::AgentTerminalHost::new(app.state::<PtyManager>().inner().clone(), app.state::<SessionManager>().inner().clone()), app.state::<CollaborationBridge>().inner().clone());
        state.finish_conversation_plan_start(&input, result.as_ref().err().map(ToString::to_string).as_deref()).map_err(to_err)?;
        result.map_err(to_err)?;
    }
    state.conversation_snapshot(&input.project_id).map_err(to_err)
    }).await
}

#[tauri::command]
async fn workspace_list_directory(
    root: String,
    relative_path: String,
) -> Result<Vec<workspace_files::WorkspaceFileEntry>, String> {
    blocking_command(move || {
        workspace_files::list_directory(&root, &relative_path).map_err(to_err)
    }).await
}

#[tauri::command]
async fn workspace_read_text(root: String, relative_path: String) -> Result<String, String> {
    blocking_command(move || {
        workspace_files::read_text(&root, &relative_path).map_err(to_err)
    }).await
}

#[tauri::command]
async fn workspace_search_files(
    root: String,
    query: String,
) -> Result<Vec<workspace_files::WorkspaceFileEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || workspace_files::search(&root, &query))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
fn workspace_media_path(
    app: AppHandle,
    root: String,
    relative_path: String,
) -> Result<String, String> {
    let path = workspace_files::resolve_path(&root, &relative_path).map_err(to_err)?;
    if !path.is_file() {
        return Err("目标不是文件".into());
    }
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(to_err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn workspace_open_path(root: String, relative_path: String) -> Result<(), String> {
    let path = workspace_files::resolve_path(&root, &relative_path).map_err(to_err)?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(to_err)
}

#[tauri::command]
async fn workspace_write_text(
    root: String,
    relative_path: String,
    contents: String,
    expected_contents: String,
) -> Result<u64, String> {
    blocking_command(move || {
        workspace_files::write_text(&root, &relative_path, &contents, &expected_contents).map_err(to_err)
    }).await
}

#[tauri::command]
fn workspace_reveal_path(root: String, relative_path: String) -> Result<(), String> {
    let path = workspace_files::resolve_path(&root, &relative_path).map_err(to_err)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(to_err)
}

// ---- 插件市场 ----

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCollaborationInput {
    message: String,
    project_id: Option<String>,
    #[serde(default)]
    target_agent_ids: Vec<String>,
    #[serde(default)]
    target_session_ids: Vec<String>,
    // None preserves legacy callers; Some(empty) means only the explicit instances count.
    #[serde(default)]
    target_runtime_kinds: Option<Vec<AgentRuntimeKind>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectCollaborationReceipt {
    target_session_ids: Vec<String>,
    target_titles: Vec<String>,
    delivery_task_ids: Vec<String>,
    reply: String,
}

fn session_matches_runtime(session: &SessionRecord, runtime: AgentRuntimeKind) -> bool {
    session.agent_runtime_kind.as_deref() == Some(runtime.as_str())
        || (runtime == AgentRuntimeKind::ClaudeCode
            && session.kind == SessionKind::Claude
            && session.agent_runtime_kind.is_none())
}

fn resolve_direct_collaboration_sessions(
    sessions: &[SessionRecord],
    project_id: Option<&str>,
    runtime_kinds: &[AgentRuntimeKind],
    target_agent_ids: &[String],
    target_session_ids: &[String],
) -> Result<Vec<SessionRecord>, String> {
    fn add_session(
        selected: &mut Vec<SessionRecord>,
        seen: &mut HashSet<String>,
        session: &SessionRecord,
    ) {
        if seen.insert(session.id.clone()) {
            selected.push(session.clone());
        }
    }

    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    for session_id in target_session_ids {
        let session = sessions
            .iter()
            .find(|session| session.id == *session_id)
            .ok_or_else(|| format!("会话不存在或已删除: {session_id}"))?;
        add_session(&mut selected, &mut seen, session);
    }
    for agent_id in target_agent_ids {
        let matches = sessions
            .iter()
            .filter(|session| session.multi_agent_agent_id.as_deref() == Some(agent_id.as_str()))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(format!("Agent {agent_id} 没有可启动的 Runtime 会话"));
        }
        for session in matches {
            add_session(&mut selected, &mut seen, session);
        }
    }
    for runtime in runtime_kinds {
        if selected
            .iter()
            .any(|session| session_matches_runtime(session, *runtime))
        {
            continue;
        }
        let project_candidates = sessions
            .iter()
            .filter(|session| {
                session_matches_runtime(session, *runtime)
                    && project_id
                        .is_some_and(|id| session.multi_agent_project_id.as_deref() == Some(id))
            })
            .collect::<Vec<_>>();
        let fallback_candidates = sessions
            .iter()
            .filter(|session| session_matches_runtime(session, *runtime))
            .collect::<Vec<_>>();
        let matches: Vec<&SessionRecord> = if !project_candidates.is_empty() {
            project_candidates
                .into_iter()
                .min_by_key(|session| {
                    (
                        session.pty_id.is_none(),
                        session.created_at,
                        session.id.clone(),
                    )
                })
                .into_iter()
                .collect()
        } else {
            fallback_candidates
                .into_iter()
                .max_by_key(|session| {
                    (
                        session.pty_id.is_some(),
                        session.updated_at,
                        session.id.clone(),
                    )
                })
                .into_iter()
                .collect()
        };
        if matches.is_empty() {
            return Err(format!("{} 没有可启动的 Runtime 会话", runtime.as_str()));
        }
        for session in matches {
            add_session(&mut selected, &mut seen, session);
        }
    }
    if selected.is_empty() {
        return Err("没有解析到可投递的运行中 Agent 会话".to_string());
    }
    Ok(selected)
}

fn migrate_inactive_member_session(sessions: &SessionManager, ptys: &PtyManager, id: &str) -> Result<bool, String> {
    let lifecycle = sessions.lifecycle_lock(id);
    let _guard = lifecycle.lock().unwrap();
    let mut cleanup_error = None;
    // The session's migration gate checks its starting flag before invoking this
    // callback. Never inspect/kill an unbound PTY during another startup window.
    let migrated = sessions.migrate_inactive_runtime_command_host(id, |pty| {
        if ptys.is_running(pty) { return true; }
        match ptys.kill(pty) {
            Ok(()) => false,
            Err(error) => { cleanup_error = Some(error.to_string()); true }
        }
    }).map_err(to_err)?;
    if let Some(error) = cleanup_error { return Err(error); }
    Ok(migrated)
}

fn prepare_direct_collaboration_sessions(
    store: &MultiAgentStore,
    session_state: &SessionManager,
    pty_state: &PtyManager,
    project_id: Option<&str>,
    runtime_kinds: &[AgentRuntimeKind],
    target_agent_ids: &[String],
    target_session_ids: &[String],
    request: Option<&DirectMessageRequest>,
) -> Result<(Vec<SessionRecord>, Vec<String>), String> {
    let _lifecycle = store.agent_lifecycle_guard();
    let mut resolved_agent_ids = target_agent_ids.to_vec();
    let snapshot = project_id
        .map(|id| store.snapshot(id).map_err(to_err))
        .transpose()?;
    if let Some(snapshot) = &snapshot {
        for runtime in runtime_kinds {
            if snapshot.agents.iter().any(|agent| {
                agent.enabled
                    && resolved_agent_ids.contains(&agent.id)
                    && agent.runtime_kind == *runtime
            }) {
                continue;
            }
            if matches!(request, Some(DirectMessageRequest::Directed { .. })) {
                resolved_agent_ids.extend(
                    snapshot
                        .agents
                        .iter()
                        .filter(|agent| agent.enabled && agent.runtime_kind == *runtime)
                        .map(|agent| agent.id.clone()),
                );
                continue;
            }
            if let Some(agent) = snapshot
                .agents
                .iter()
                .filter(|agent| agent.enabled && agent.runtime_kind == *runtime)
                .min_by_key(|agent| {
                    let role = match agent.role {
                        multi_agent::AgentRole::Orchestrator => 0,
                        multi_agent::AgentRole::Worker => 1,
                        multi_agent::AgentRole::Integrator => 2,
                        multi_agent::AgentRole::Reviewer => 3,
                    };
                    (role, agent.created_at, agent.id.clone())
                })
            {
                resolved_agent_ids.push(agent.id.clone());
            }
        }
        for agent in snapshot
            .agents
            .iter()
            .filter(|agent| agent.enabled && resolved_agent_ids.contains(&agent.id))
        {
            let session = session_state
                .create_agent(sessions::NewAgentSession {
                    avatar_id: agent.avatar_id.clone(),
                    project_id: snapshot.project.id.clone(),
                    agent_id: agent.id.clone(),
                    runtime_kind: agent.runtime_kind.as_str().to_string(),
                    agent_name: agent.name.clone(),
                    cwd: snapshot.project.root_path.clone(),
                })
                .map_err(to_err)?;
            store
                .link_agent_app_session(&snapshot.project.id, &agent.id, &session.id)
                .map_err(to_err)?;
        }
    }

    let cwd = snapshot
        .as_ref()
        .map(|snapshot| snapshot.project.root_path.clone())
        .or_else(|| dirs::home_dir().map(|path| path.display().to_string()))
        .unwrap_or_else(|| ".".to_string());
    for runtime in runtime_kinds {
        if !session_state
            .list()
            .iter()
            .any(|session| session_matches_runtime(session, *runtime))
        {
            session_state
                .create_runtime(NewRuntimeSession {
                    runtime_kind: runtime.as_str().to_string(),
                    cwd: cwd.clone(),
                    project_id: None,
                })
                .map_err(to_err)?;
        }
    }

    let candidates = if matches!(request, Some(DirectMessageRequest::Directed { .. })) {
        resolve_directed_collaboration_sessions(
            &session_state.list(),
            project_id,
            runtime_kinds,
            &resolved_agent_ids,
            target_session_ids,
        )?
    } else {
        resolve_direct_collaboration_sessions(
            &session_state.list(),
            project_id,
            runtime_kinds,
            &resolved_agent_ids,
            target_session_ids,
        )?
    };
    // Ambiguous directed requests must fail before launching either endpoint.
    let direction = resolve_direct_message_direction(request, &candidates)?;
    let mut started_titles = Vec::new();
    let mut targets = Vec::with_capacity(candidates.len());
    for candidate in candidates.iter().filter(|candidate| {
        direction
            .is_none_or(|(sender, recipient)| candidate.id == sender || candidate.id == recipient)
    }) {
        let was_running = candidate
            .pty_id
            .is_some_and(|pty_id| pty_state.is_running(pty_id));
        let background_runtime = matches!(
            candidate.agent_runtime_kind.as_deref(),
            Some(
                "kimi_code"
                    | "pi"
                    | "opencode"
                    | "deepseek_harness"
                    | "zcode"
                    | "zcode_cli"
            )
        ) && candidate.multi_agent_agent_id.is_some();
        let started = if background_runtime && !was_running {
            migrate_inactive_member_session(session_state, pty_state, &candidate.id)?;
            session_state
                .get(&candidate.id)
                .ok_or_else(|| "会话已删除".to_string())?
        } else {
            open_direct_collaboration_session(&candidate, pty_state, session_state, Some(store))?
        };
        if !was_running {
            started_titles.push(started.title.clone());
        }
        targets.push(started);
    }
    Ok((targets, started_titles))
}

#[tauri::command]
async fn ma_send_direct_message(
    input: DirectCollaborationInput,
    app: AppHandle,
) -> Result<Option<DirectCollaborationReceipt>, String> {
    // Session startup and CLI probing may block; keep the UI event loop free so
    // the optimistic user message renders before delivery finishes.
    tauri::async_runtime::spawn_blocking(move || {
        send_direct_collaboration_message(
            input,
            app.state::<MultiAgentStore>(),
            app.state::<SessionManager>(),
            app.state::<PtyManager>(),
            app.state::<SessionActionQueue>(),
        )
    })
    .await
    .map_err(to_err)?
}

fn resolve_directed_collaboration_sessions(
    sessions: &[SessionRecord],
    project_id: Option<&str>,
    runtime_kinds: &[AgentRuntimeKind],
    target_agent_ids: &[String],
    target_session_ids: &[String],
) -> Result<Vec<SessionRecord>, String> {
    if !target_agent_ids.is_empty() || !target_session_ids.is_empty() {
        return resolve_direct_collaboration_sessions(
            sessions,
            project_id,
            &[],
            target_agent_ids,
            target_session_ids,
        );
    }
    let mut candidates = Vec::new();
    for runtime in runtime_kinds {
        let matching = sessions
            .iter()
            .filter(|session| session_matches_runtime(session, *runtime))
            .collect::<Vec<_>>();
        let project_matches = matching
            .iter()
            .filter(|session| {
                project_id.is_some_and(|id| session.multi_agent_project_id.as_deref() == Some(id))
            })
            .copied()
            .collect::<Vec<_>>();
        candidates.extend(
            if project_matches.is_empty() {
                matching
            } else {
                project_matches
            }
            .into_iter()
            .cloned(),
        );
    }
    Ok(candidates)
}

fn resolve_direct_message_direction<'a>(
    request: Option<&DirectMessageRequest>,
    targets: &'a [SessionRecord],
) -> Result<Option<(&'a str, &'a str)>, String> {
    let Some(request @ DirectMessageRequest::Directed { .. }) = request else {
        return Ok(None);
    };
    let participants = targets
        .iter()
        .map(|session| {
            Ok(DirectMessageParticipant {
                id: &session.id,
                title: &session.title,
                runtime: standalone_runtime_kind(session)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    directed_sender_id(request, &participants)
}

fn direct_message_execution_sessions<'a>(
    targets: &'a [SessionRecord],
    direction: Option<(&str, &str)>,
) -> Vec<&'a SessionRecord> {
    targets
        .iter()
        .filter(|target| direction.is_none_or(|(sender, _)| target.id == sender))
        .collect()
}

fn direct_collaboration_runtimes(
    message: &str,
    request: Option<&DirectMessageRequest>,
    has_instance_targets: bool,
    explicit_runtimes: Option<&[AgentRuntimeKind]>,
) -> Vec<AgentRuntimeKind> {
    if let Some(runtimes) = explicit_runtimes {
        return runtimes.iter().copied().fold(Vec::new(), |mut selected, runtime| {
            if !selected.contains(&runtime) { selected.push(runtime); }
            selected
        });
    }
    if has_instance_targets { return Vec::new(); }
    let mentioned = explicitly_mentioned_agent_runtimes(message);
    if !mentioned.is_empty() { return mentioned; }
    match request {
        Some(request @ DirectMessageRequest::Directed { .. }) => {
            directed_message_runtimes(request)
        }
        _ => requested_agent_runtimes(message),
    }
}

fn direct_collaboration_delivery(
    message: &str,
    targets: &[SessionRecord],
    current: &SessionRecord,
    direction: Option<(&str, &str)>,
) -> String {
    let others = targets.iter().filter(|target| target.id != current.id)
        .map(|target| format!("- {}（完整会话 ID：{}）", target.title, target.id))
        .collect::<Vec<_>>().join("\n");
    let instruction = match direction {
        Some((_, recipient)) => format!("这是单向通信指令：你是发送者，只给接收者（完整会话 ID：{recipient}）发送用户要求的消息；不要广播，也不要给接收者创建或规划新任务。"),
        None if targets.len() > 1 && multi_agent::self_introduction_requested(message) => "本次是成员间介绍：每位参与者主动使用原会话通信工具，向名单中每个其他参与者发送一次用户要求的介绍内容；不要只在本地输出介绍。按当前会话的真实身份介绍，不冒充其他参与者，不编造对方回复。收到其他人的介绍后只在本地查看，不再发送确认或问候，不创建工程任务。".into(),
        None => "只联系下列其他参与者，不要向当前会话自身发送。其他参与者收到的是各自的协作指令；不要冒充它们或编造其回复。".into(),
    };
    format!(
        "[CrewVela 多 Agent 协作消息]\n当前会话（你）：{}（完整会话 ID：{}）\n其他参与会话：\n{}\n\n用户要求：{}\n{}\n优先使用已提供的 crewvela_sessions MCP：先调用 sessions 核对目标，再调用 send，只传 to 和 body；发送方身份已绑定当前会话，无需读取 Skill 或执行 shell。没有这些工具时，再读取 crewvela Skill；PowerShell 使用 & $env:CREWVELA_CLI send --to <其他参与者完整会话 ID> --body <消息>。已排队不代表已送达或已回复；只执行用户要求的协作，不创建无关文件。",
        current.title, current.id, others, message, instruction
    )
}

fn send_direct_collaboration_message(
    input: DirectCollaborationInput,
    store: State<'_, MultiAgentStore>,
    session_state: State<'_, SessionManager>,
    pty_state: State<'_, PtyManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<Option<DirectCollaborationReceipt>, String> {
    let message = input.message.trim();
    if message.is_empty() {
        return Err("协作消息不能为空".to_string());
    }
    if message.chars().count() > 32_000 {
        return Err("协作消息不能超过 32000 个字符".to_string());
    }
    let request = direct_message_request(message);
    if input.target_session_ids.is_empty() && request.is_none() {
        return Ok(None);
    }
    let runtime_kinds = direct_collaboration_runtimes(
        message,
        request.as_ref(),
        !input.target_agent_ids.is_empty() || !input.target_session_ids.is_empty(),
        input.target_runtime_kinds.as_deref(),
    );
    // Validate the complete explicit roster before creating or starting any session.
    for runtime in &runtime_kinds { runtime.ensure_available().map_err(to_err)?; }

    session_state.ingest_events().map_err(to_err)?;
    let (targets, started_titles) = prepare_direct_collaboration_sessions(
        &store,
        &session_state,
        &pty_state,
        input.project_id.as_deref(),
        &runtime_kinds,
        &input.target_agent_ids,
        &input.target_session_ids,
        request.as_ref(),
    )?;
    let direction = resolve_direct_message_direction(request.as_ref(), &targets)?;
    let recipients = direct_message_execution_sessions(&targets, direction);

    if let Some(project_id) = input.project_id.as_deref() {
        store.project(project_id).map_err(to_err)?;
        store
            .record_message(project_id, "user", message)
            .map_err(to_err)?;
    }
    let queued_messages: Vec<_> = recipients.iter().map(|target|
        (target.id.clone(), QueuedMessage::new(direct_collaboration_delivery(message, &targets, target, direction), None))
    ).collect();
    // Background members turn durable mailbox entries into tasks with this exact ID.
    // Return only this request's tasks so the UI can report their failures without
    // replaying unrelated historical failures from the project.
    let delivery_task_ids = recipients.iter().zip(&queued_messages)
        .filter(|(target, _)| target.runtime_command_host
            && target.multi_agent_agent_id.is_some()
            && target.multi_agent_project_id.as_deref() == input.project_id.as_deref())
        .map(|(_, (_, message))| format!("cli-message-{}", message.id))
        .collect();
    action_queue.enqueue_messages_durable(queued_messages).map_err(to_err)?;
    for target in &recipients { action_queue.reset_agent_quota_involving(&target.id); }
    action_queue.flush_permission_modes(&session_state, &pty_state);
    action_queue.flush_ready(&session_state, &pty_state);

    let target_titles = recipients
        .iter()
        .map(|session| session.title.clone())
        .collect::<Vec<_>>();
    let mut reply = format!(
        "{}已通过统一协作通道排队发送给 {}；消息已跳过 LLM 规划器。",
        if started_titles.is_empty() {
            String::new()
        } else {
            format!(
                "已按会话原有权限请求启动 {}；启动与收件就绪仍需确认。",
                started_titles.join("、")
            )
        },
        target_titles
            .iter()
            .map(|title| format!("「{title}」"))
            .collect::<Vec<_>>()
            .join("、")
    );
    if let Some(project_id) = input.project_id.as_deref() {
        if let Err(error) = store.record_message(project_id, "coordinator", &reply) {
            // Delivery is already durable. Reporting the whole request as
            // rejected here would invite a retry that duplicates the message.
            reply.push_str(&format!("\n消息已入队，但协作历史保存失败：{error}。请勿重复发送本条消息。"));
            eprintln!("[crewvela] 消息已入队后的协作历史保存失败：{error:#}");
        }
    }
    Ok(Some(DirectCollaborationReceipt {
        target_session_ids: recipients
            .iter()
            .map(|session| session.id.clone())
            .collect(),
        target_titles,
        delivery_task_ids,
        reply,
    }))
}

#[tauri::command]
fn ma_publish_artifact(
    input: PublishArtifactInput,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::ArtifactRecord, String> {
    state.publish_artifact(input).map_err(to_err)
}

#[tauri::command]
fn ma_request_approval(
    input: RequestApprovalInput,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::ApprovalRecord, String> {
    state.request_approval(input).map_err(to_err)
}

#[tauri::command]
fn ma_heartbeat_run(
    input: HeartbeatRunInput,
    state: State<'_, MultiAgentStore>,
) -> Result<HeartbeatReceipt, String> {
    state.heartbeat_run(input).map_err(to_err)
}

#[tauri::command]
fn ma_put_memory(
    input: PutMemoryInput,
    state: State<'_, MultiAgentStore>,
) -> Result<MemoryEntry, String> {
    state.put_memory(input).map_err(to_err)
}

#[tauri::command]
fn ma_search_memory(
    input: SearchMemoryInput,
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<MemoryEntry>, String> {
    state.search_memory(input).map_err(to_err)
}

#[tauri::command]
fn ma_rebuild_knowledge_index(
    project_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<KnowledgeIndexReceipt, String> {
    state.rebuild_knowledge_index(&project_id).map_err(to_err)
}

#[tauri::command]
fn ma_search_knowledge(
    input: SearchKnowledgeInput,
    state: State<'_, MultiAgentStore>,
) -> Result<Vec<KnowledgeSearchResult>, String> {
    state.search_knowledge(input).map_err(to_err)
}

#[tauri::command]
fn ma_recover(state: State<'_, MultiAgentStore>) -> Result<RecoveryReceipt, String> {
    state.recover_interrupted_runs().map_err(to_err)
}

const COORDINATOR_REPLY_STREAM_EVENT: &str = "multi-agent-coordinator-reply-stream";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoordinatorReplyStreamEvent {
    request_id: String,
    text: Option<String>,
    reset: bool,
    done: bool,
}

#[tauri::command]
async fn ma_interpret_message(
    app: AppHandle,
    message: String,
    history: Vec<CoordinatorHistoryMessage>,
    project_id: Option<String>,
    target_agent_ids: Vec<String>,
    request_id: Option<String>,
    state: State<'_, MultiAgentStore>,
) -> Result<CoordinatorPlan, String> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cancellation = CoordinatorCancellationToken::register(request_id.as_deref());
        let stream_app = app.clone();
        let stream_request_id = request_id.clone();
        let snapshot = project_id
            .as_deref()
            .map(|id| store.snapshot(id))
            .transpose()
            .map_err(to_err)?;
        let plan_result = interpret_collaboration_message_streaming(
            &message,
            &history,
            snapshot.as_ref(),
            &target_agent_ids,
            || cancellation.is_cancelled(),
            move |update| {
                let Some(request_id) = stream_request_id.as_ref() else {
                    return;
                };
                let (text, reset) = match update {
                    CoordinatorStreamUpdate::Reply(text) => (Some(text), false),
                    CoordinatorStreamUpdate::Reset => (None, true),
                };
                let _ = stream_app.emit(
                    COORDINATOR_REPLY_STREAM_EVENT,
                    CoordinatorReplyStreamEvent {
                        request_id: request_id.clone(),
                        text,
                        reset,
                        done: false,
                    },
                );
            },
        )
        .map_err(to_err);
        if let Some(request_id) = request_id {
            let _ = app.emit(
                COORDINATOR_REPLY_STREAM_EVENT,
                CoordinatorReplyStreamEvent {
                    request_id,
                    text: None,
                    reset: false,
                    done: true,
                },
            );
        }
        let plan = plan_result?;
        if cancellation.is_cancelled() {
            return Err("Multi-Agent 协调器已取消".to_string());
        }
        if let Some(project_id) = project_id
            .as_deref()
            .filter(|_| plan.intent != multi_agent::CoordinatorIntent::PlanProject)
        {
            let _ = store.record_message(project_id, "user", &message);
            let _ = store.record_message(project_id, "coordinator", &plan.reply);
        }
        Ok(plan)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
fn ma_cancel_interpret_message(request_id: String) -> bool {
    cancel_coordinator_request(&request_id)
}

fn conversation_writes_to_record(
    recorded: &std::collections::HashSet<(String, String)>,
    conversation: &[multi_agent::CoordinatorHistoryMessage],
) -> Vec<(&'static str, String)> {
    conversation
        .iter()
        .filter_map(|message| {
            let sender = match message.role {
                multi_agent::CoordinatorMessageRole::User => "user",
                multi_agent::CoordinatorMessageRole::Assistant => "coordinator",
            };
            let key = (sender.to_string(), message.content.trim().to_string());
            if recorded.contains(&key) {
                None
            } else {
                Some((sender, message.content.clone()))
            }
        })
        .collect()
}

#[tauri::command]
fn ma_apply_plan(
    mut input: ApplyCoordinatorPlanInput,
    state: State<'_, MultiAgentStore>,
) -> Result<ApplyCoordinatorPlanReceipt, String> {
    use multi_agent::CoordinatorIntent;
    if !matches!(
        input.plan.intent,
        CoordinatorIntent::PlanProject | CoordinatorIntent::AddTasks
    ) {
        return Err("当前计划不包含可创建的任务".to_string());
    }
    normalize_coordinator_capabilities(&mut input.plan);
    validate_coordinator_plan(&input.plan)?;
    if input.plan.intent == CoordinatorIntent::AddTasks {
        let project_id = input
            .project_id
            .as_deref()
            .ok_or_else(|| "追加任务计划缺少目标项目".to_string())?;
        let existing = state.snapshot(project_id).map_err(to_err)?;
        for planned in &input.plan.agents {
            if let Some(id) = planned.existing_agent_id.as_deref() {
                let agent = existing
                    .agents
                    .iter()
                    .find(|agent| agent.id == id && agent.enabled)
                    .ok_or_else(|| {
                        format!("计划引用的现有 Agent 不存在、已禁用或不属于当前项目：{id}")
                    })?;
                agent.runtime_kind.ensure_available().map_err(to_err)?;
                if agent.runtime_kind != planned.runtime_kind {
                    return Err(format!("Agent“{}”的 Runtime 与计划不一致", agent.name));
                }
            }
        }
    }
    let conversation = input.conversation.clone();
    let project_id = match input.plan.intent {
        CoordinatorIntent::PlanProject => {
            let name = required_plan_text(input.plan.project_name.as_deref(), "项目名称")?;
            let goal = required_plan_text(input.plan.goal.as_deref(), "项目目标")?;
            state
                .create_project(CreateProjectInput {
                    name,
                    goal,
                    root_path: input.root_path,
                    default_branch: "main".to_string(),
                })
                .map_err(to_err)?
                .id
        }
        CoordinatorIntent::AddTasks => {
            let project_id = input
                .project_id
                .clone()
                .ok_or_else(|| "追加任务计划缺少目标项目".to_string())?;
            state.project(&project_id).map_err(to_err)?;
            project_id
        }
        _ => return Err("当前计划不包含可创建的任务".to_string()),
    };

    let existing = state.snapshot(&project_id).map_err(to_err)?;
    let mut agent_ids = HashMap::<String, String>::new();
    let mut created_agents = 0;
    for planned in &input.plan.agents {
        let id = if let Some(existing_agent_id) = planned.existing_agent_id.as_deref() {
            let agent = existing
                .agents
                .iter()
                .find(|agent| agent.id == existing_agent_id && agent.enabled)
                .ok_or_else(|| {
                    format!(
                        "计划引用的现有 Agent 不存在、已禁用或不属于当前项目：{existing_agent_id}"
                    )
                })?;
            if agent.runtime_kind != planned.runtime_kind {
                return Err(format!("Agent“{}”的 Runtime 与计划不一致", agent.name));
            }
            agent.id.clone()
        } else if let Some(agent) = existing.agents.iter().find(|agent| {
            agent.enabled
                && agent.runtime_kind == planned.runtime_kind
                && agent.name == planned.name
        }) {
            agent.id.clone()
        } else {
            created_agents += 1;
            state
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project_id.clone(),
                    name: planned.name.clone(),
                    runtime_kind: planned.runtime_kind,
                    role: planned.role,
                    description: planned.description.clone(),
                    model: planned.model.clone(),
                    permission_profile: planned.permission_profile,
                    max_concurrency: 1,
                persona: Default::default(),
                persona_custom: String::new(),
                })
                .map_err(to_err)?
                .id
        };
        if agent_ids.insert(planned.key.clone(), id).is_some() {
            return Err(format!("Agent key 重复：{}", planned.key));
        }
    }
    let mut task_ids = HashMap::<String, String>::new();
    for planned in &input.plan.tasks {
        let assignee = agent_ids.get(&planned.assignee_key).ok_or_else(|| {
            format!(
                "任务“{}”引用了未知 Agent：{}",
                planned.title, planned.assignee_key
            )
        })?;
        let reviewer = planned
            .reviewer_key
            .as_ref()
            .map(|key| {
                agent_ids
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("任务“{}”引用了未知审查 Agent：{key}", planned.title))
            })
            .transpose()?;
        let task = state
            .create_task(CreateTaskInput {
                project_id: project_id.clone(),
                parent_id: None,
                stage: planned.stage,
                title: planned.title.clone(),
                description: planned.description.clone(),
                acceptance_criteria: planned.acceptance_criteria.clone(),
                verification_command: planned.verification_command.clone(),
                assignee_agent_id: Some(assignee.clone()),
                reviewer_agent_id: reviewer,
                workspace_mode: planned.workspace_mode,
                priority: planned.priority,
                required_capabilities: planned.required_capabilities.clone(),
                max_attempts: planned.max_attempts,
                retry_backoff_seconds: planned.retry_backoff_seconds,
            })
            .map_err(to_err)?;
        if task_ids.insert(planned.key.clone(), task.id).is_some() {
            return Err(format!("Task key 重复：{}", planned.key));
        }
    }
    for planned in &input.plan.tasks {
        let task_id = task_ids.get(&planned.key).expect("task key was inserted");
        for dependency_key in &planned.depends_on {
            let dependency_id = task_ids.get(dependency_key).ok_or_else(|| {
                format!("任务“{}”引用了未知依赖：{dependency_key}", planned.title)
            })?;
            state
                .add_dependency(AddDependencyInput {
                    task_id: task_id.clone(),
                    depends_on_task_id: dependency_id.clone(),
                })
                .map_err(to_err)?;
        }
    }
    let recorded = state
        .snapshot(&project_id)
        .map(|snapshot| {
            snapshot
                .messages
                .into_iter()
                .map(|message| (message.sender_type.clone(), message.body.trim().to_string()))
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    for (sender, content) in conversation_writes_to_record(&recorded, &conversation) {
        let _ = state.record_message(&project_id, sender, &content);
    }
    let _ = state.record_message(
        &project_id,
        "coordinator",
        &format!(
            "计划已确认：创建 {} 个 Agent、{} 个任务。",
            created_agents,
            input.plan.tasks.len()
        ),
    );
    Ok(ApplyCoordinatorPlanReceipt {
        project_id,
        created_agents,
        created_tasks: input.plan.tasks.len(),
    })
}

fn required_plan_text(value: Option<&str>, label: &str) -> Result<String, String> {
    let normalized = value.unwrap_or_default().trim();
    let placeholder = matches!(
        normalized.to_ascii_lowercase().as_str(),
        "" | "null" | "undefined" | "none" | "n/a"
    ) || matches!(normalized, "未命名" | "未命名项目");
    if placeholder {
        Err(format!("新项目计划缺少有效的{label}"))
    } else {
        Ok(normalized.to_string())
    }
}

fn normalize_coordinator_capabilities(plan: &mut CoordinatorPlan) {
    for task in &mut plan.tasks {
        let contract = format!(
            "{}\n{}\n{}",
            task.title, task.description, task.acceptance_criteria
        )
        .to_lowercase();
        let explicitly_requires_runtime_commands = [
            "slash command",
            "custom command",
            "斜杠命令",
            "自定义命令",
            "内置命令",
        ]
        .iter()
        .any(|marker| contract.contains(marker));
        if !explicitly_requires_runtime_commands {
            task.required_capabilities
                .retain(|capability| !capability.eq_ignore_ascii_case("commands"));
        }
    }
}

fn validate_coordinator_plan(plan: &CoordinatorPlan) -> Result<(), String> {
    if plan.agents.is_empty() || plan.tasks.is_empty() {
        return Err("协作计划至少需要一个 Agent 和一个任务".to_string());
    }
    let mut agent_keys = HashSet::new();
    let mut agent_names = HashSet::new();
    let mut existing_agent_ids = HashSet::new();
    for agent in &plan.agents {
        agent.runtime_kind.ensure_available().map_err(to_err)?;
        if !agent.runtime_kind.is_collaboration_cli()
            && !(agent.runtime_kind == AgentRuntimeKind::ZCode && agent.existing_agent_id.as_deref().is_some_and(|id| !id.trim().is_empty())) {
            return Err("新增协作成员仅支持七个 CLI；ZCode Desktop 仅兼容已有成员，不能自动替换".into());
        }
        if agent.key.trim().is_empty() || !agent_keys.insert(agent.key.clone()) {
            return Err(format!("Agent key 为空或重复：{}", agent.key));
        }
        if let Some(existing_agent_id) = agent.existing_agent_id.as_deref() {
            if plan.intent == multi_agent::CoordinatorIntent::PlanProject {
                return Err("新项目计划不能引用现有 Agent".to_string());
            }
            if !existing_agent_ids.insert(existing_agent_id) {
                return Err(format!("现有 Agent 重复：{existing_agent_id}"));
            }
        } else if !agent_names.insert(agent.name.trim().to_lowercase()) {
            return Err(format!("新建 Agent 名称重复：{}", agent.name));
        }
    }
    let mut task_keys = HashSet::new();
    for task in &plan.tasks {
        if task.key.trim().is_empty() || !task_keys.insert(task.key.clone()) {
            return Err(format!("Task key 为空或重复：{}", task.key));
        }
        if !agent_keys.contains(&task.assignee_key) {
            return Err(format!(
                "任务“{}”引用了未知 Agent：{}",
                task.title, task.assignee_key
            ));
        }
        if task
            .reviewer_key
            .as_ref()
            .is_some_and(|key| !agent_keys.contains(key))
        {
            return Err(format!("任务“{}”引用了未知审查 Agent", task.title));
        }
        if let Some(assignee) = plan
            .agents
            .iter()
            .find(|agent| agent.key == task.assignee_key)
        {
            let available = multi_agent::capabilities_for_runtime(assignee.runtime_kind);
            let missing = task
                .required_capabilities
                .iter()
                .filter(|required| {
                    !available
                        .iter()
                        .any(|capability| capability.eq_ignore_ascii_case(required))
                })
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(format!(
                    "任务“{}”要求的能力不受 {} 支持：{}",
                    task.title,
                    assignee.runtime_kind.as_str(),
                    missing.join("、")
                ));
            }
        }
    }
    let dependencies = plan
        .tasks
        .iter()
        .map(|task| (task.key.clone(), task.depends_on.clone()))
        .collect::<HashMap<_, _>>();
    for task in &plan.tasks {
        for dependency in &task.depends_on {
            if !task_keys.contains(dependency) {
                return Err(format!("任务“{}”引用了未知依赖：{dependency}", task.title));
            }
        }
        if coordinator_dependency_cycle(&task.key, &dependencies, &mut HashSet::new()) {
            return Err(format!("任务依赖存在循环：{}", task.key));
        }
    }
    multi_agent::validate_parallel_review_graph(
        &plan.agents.iter().map(|agent| multi_agent::ReviewGraphAgent {
            key: &agent.key, runtime_kind: agent.runtime_kind, role: agent.role,
        }).collect::<Vec<_>>(),
        &plan.tasks.iter().map(|task| multi_agent::ReviewGraphTask {
            key: &task.key, title: &task.title, assignee_key: &task.assignee_key,
            depends_on: &task.depends_on, workspace_mode: task.workspace_mode,
        }).collect::<Vec<_>>(),
    )
}

fn coordinator_dependency_cycle(
    key: &str,
    dependencies: &HashMap<String, Vec<String>>,
    path: &mut HashSet<String>,
) -> bool {
    if !path.insert(key.to_string()) {
        return true;
    }
    let cyclic = dependencies.get(key).is_some_and(|items| {
        items
            .iter()
            .any(|dependency| coordinator_dependency_cycle(dependency, dependencies, path))
    });
    path.remove(key);
    cyclic
}

#[tauri::command]
fn ma_start_project(
    project_id: String,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
    pty: State<'_, PtyManager>,
    sessions: State<'_, SessionManager>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<ProjectExecutionReceipt, String> {
    let snapshot = store.snapshot(&project_id).map_err(to_err)?;
    let removed_agents = snapshot
        .agents
        .iter()
        .filter(|agent| agent.runtime_kind.is_retired())
        .map(|agent| agent.id.as_str())
        .collect::<HashSet<_>>();
    let retired_task_count = snapshot.tasks.iter().filter(|task| {
        task.assignee_agent_id
            .as_deref()
            .is_some_and(|id| removed_agents.contains(id))
            && !matches!(
                task.status,
                multi_agent::TaskStatus::Done | multi_agent::TaskStatus::Cancelled
            )
    }).count();
    // Historical assignments stay intact. The scheduler excludes retired members
    // and keeps their dependents blocked, while independent work can proceed.
    let mut receipt = supervisor
        .start(
            store.inner().clone(),
            project_id,
            multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()),
            collaboration_bridge.inner().clone(),
        )
        .map_err(to_err)?;
    if receipt.accepted && retired_task_count > 0 {
        receipt.message.push_str(&format!(
            "；另有 {retired_task_count} 个任务仍指派给已移除的 Runtime，已保留且不会执行，请改派后继续；依赖这些任务的工作仍需等待"
        ));
    }
    Ok(receipt)
}

#[tauri::command]
fn ma_pause_project(
    project_id: String,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
) -> Result<ProjectExecutionReceipt, String> {
    supervisor.pause(store.inner(), &project_id).map_err(to_err)
}

#[tauri::command]
fn ma_cancel_project(
    project_id: String,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
) -> Result<ProjectExecutionReceipt, String> {
    supervisor
        .cancel(store.inner(), &project_id)
        .map_err(to_err)
}



#[tauri::command]
fn ma_approve_task(
    task_id: String,
    state: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
    pty: State<'_, PtyManager>,
    sessions: State<'_, SessionManager>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<multi_agent::MultiAgentTask, String> {
    if let Err(error) = merge_integrator_delivery(state.inner(), &task_id) {
        let message = error.to_string();
        let _ = state.request_task_changes(&task_id, &message);
        return Err(format!("{message}；任务已转为“需要修改”，可修复后重试"));
    }
    let task = state.approve_task(&task_id).map_err(to_err)?;
    if task.status == multi_agent::TaskStatus::Done {
        supervisor.wake_active(
            state.inner().clone(), task.project_id.clone(),
            multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()),
            collaboration_bridge.inner().clone(),
        ).map_err(|error| format!("交付已批准，自动调度未启动：{error}。请在协作中心继续项目。"))?;
    }
    Ok(task)
}

#[tauri::command]
async fn ma_update_resumed_delivery(
    task_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::MultiAgentTask, String> {
    let store = state.inner().clone();
    blocking_command(move || store.update_resumed_delivery(&task_id).map_err(to_err)).await
}

#[tauri::command]
fn ma_retry_task(
    task_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::MultiAgentTask, String> {
    state.retry_task(&task_id).map_err(to_err)
}

#[tauri::command]
fn ma_reassign_task(
    task_id: String,
    agent_id: String,
    state: State<'_, MultiAgentStore>,
) -> Result<multi_agent::MultiAgentTask, String> {
    state.reassign_task(&task_id, &agent_id).map_err(to_err)
}

#[tauri::command]
fn ma_takeover_agent(
    input: TakeoverAgentInput,
    state: State<'_, MultiAgentStore>,
) -> Result<AgentControlReceipt, String> {
    let queued_instruction = state
        .set_agent_control(
            &input.project_id,
            &input.agent_id,
            AgentControlMode::Manual,
            input.instruction.as_deref(),
        )
        .map_err(to_err)?;
    Ok(AgentControlReceipt {
        project_id: input.project_id,
        agent_id: input.agent_id,
        control_mode: AgentControlMode::Manual,
        queued_instruction,
        message: "已切换为人工接管；当前 Run 会执行到安全结束，后续任务暂停。补充指令将在该 Agent 下一次执行时送达。".to_string(),
    })
}

#[tauri::command]
fn ma_return_agent_control(
    input: ReturnAgentControlInput,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
    pty: State<'_, PtyManager>,
    sessions: State<'_, SessionManager>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<AgentControlReceipt, String> {
    store
        .set_agent_control(
            &input.project_id,
            &input.agent_id,
            AgentControlMode::Auto,
            None,
        )
        .map_err(to_err)?;
    supervisor
        .start(
            store.inner().clone(),
            input.project_id.clone(),
            multi_agent::AgentTerminalHost::new(pty.inner().clone(), sessions.inner().clone()),
            collaboration_bridge.inner().clone(),
        )
        .map_err(to_err)?;
    Ok(AgentControlReceipt {
        project_id: input.project_id,
        agent_id: input.agent_id,
        control_mode: AgentControlMode::Auto,
        queued_instruction: false,
        message: "已交还总控，调度器会继续领取该 Agent 的待办任务。".to_string(),
    })
}

// ---- 终端会话 ----

fn open_session(
    record: &SessionRecord,
    provider: &ProviderConfig,
    key: &str,
    resume: bool,
    pty_state: &PtyManager,
    session_state: &SessionManager,
) -> Result<SessionRecord, String> {
    let _admission = pty_state.admit_start().map_err(to_err)?;
    worktree_management::verify_start(record, session_state).map_err(to_err)?;
    runtime_preferences::ensure_session_available(record).map_err(to_err)?;
    session_switch::ensure_can_start(&record.id).map_err(to_err)?;
    if let Some(pty_id) = record.pty_id {
        if pty_state.is_running(pty_id) {
            return Ok(record.clone());
        }
        pty_state.kill(pty_id).map_err(to_err)?;
    }
    pty_state.kill_for_app_session(&record.id).map_err(to_err)?;

    let hook_command = session_state.hook_command(&record.id).map_err(to_err)?;
    let status_line_command = session_state
        .status_line_command(&record.id)
        .map_err(to_err)?;
    telemetry::clear_claude_statusline(&record.id).map_err(to_err)?;
    // 从未产生 transcript 的会话（如卡在 trust 对话框被停止，claude 没跑到
    // 写 conversation）没有可恢复的内容，--resume 会直接报
    // "No conversation found with session ID" 退出。退化为 --session-id
    // 全新启动：同一 session id，等价于把会话重新拉起来。
    let resume = resume && crate::usage::transcript_path(record).is_some_and(|path| path.exists());
    // 保留配置中启用的 MCP server 注入本次会话；配置读取失败不阻塞会话启动。
    let mut mcp_servers = crate::mcp::enabled_claude_map().unwrap_or_else(|error| {
        eprintln!("[crewvela] 读取 MCP 配置失败：{error:#}");
        serde_json::Map::new()
    });
    if let Some((name, server)) = ai_memory::embedded::claude_server(record) {
        mcp_servers.insert(name, server);
    }
    let _starting = session_state
        .begin_runtime_start(&record.id)
        .map_err(to_err)?;
    let pty_id = pty_state
        .open(PtyOpenOptions {
            provider,
            key,
            cwd: Some(&record.cwd),
            yolo: record.yolo,
            auto_mode: record.auto_mode,
            app_session_id: &record.id,
            claude_session_id: &record.claude_session_id,
            resume,
            hook_command: &hook_command,
            status_line_command: &status_line_command,
            auto_confirm_trust: record.yolo || record.worktree.is_some(),
            mcp_servers: Some(&mcp_servers),
        })
        .map_err(to_err)?;
    match session_state.attach_runtime(&record.id, pty_id) {
        Ok(updated) => Ok(updated),
        Err(error) => {
            match pty_state.kill(pty_id) {
                Ok(()) => Err(error.to_string()),
                Err(stop) => Err(format!("保存会话失败：{error}；新终端停止未获确认，保留跟踪并禁止替代启动：{stop}")),
            }
        }
    }
}

fn open_terminal_session(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
) -> Result<SessionRecord, String> {
    let _admission = pty_state.admit_start().map_err(to_err)?;
    if let Some(pty_id) = record.pty_id {
        if pty_state.is_running(pty_id) {
            return Ok(record.clone());
        }
        pty_state.kill(pty_id).map_err(to_err)?;
    }
    pty_state.kill_for_app_session(&record.id).map_err(to_err)?;
    let shell = record
        .shell
        .ok_or_else(|| "终端会话缺少 Shell 类型".to_string())?;
    let pty_id = pty_state
        .open_terminal(shell, Some(&record.cwd), &record.id)
        .map_err(to_err)?;
    match session_state.attach_runtime(&record.id, pty_id) {
        Ok(updated) => Ok(updated),
        Err(error) => {
            match pty_state.kill(pty_id) {
                Ok(()) => Err(error.to_string()),
                Err(stop) => Err(format!("保存会话失败：{error}；新终端停止未获确认，保留跟踪并禁止替代启动：{stop}")),
            }
        }
    }
}

#[tauri::command]
async fn skill_center_snapshot(app: AppHandle) -> Result<SkillCenterSnapshot, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.snapshot().map_err(to_err)
    }).await
}

#[tauri::command]
fn skill_center_update_checks(
    state: State<'_, SkillCenter>,
) -> Result<Vec<skill_center::SkillUpdateCheck>, String> {
    state.update_checks().map_err(to_err)
}

#[tauri::command]
async fn skill_center_check_updates(
    app: AppHandle,
    skill_id: Option<String>,
    replacement_archive: Option<String>,
) -> Result<Vec<skill_center::SkillUpdateCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<SkillCenter>();
        match skill_id {
            Some(id) => state
                .check_update(&id, replacement_archive)
                .map(|check| vec![check])
                .map_err(to_err),
            None => state.check_all_updates().map_err(to_err),
        }
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
fn skill_center_versions(
    state: State<'_, SkillCenter>,
    skill_id: String,
) -> Result<Vec<skill_center::SkillVersionInfo>, String> {
    state.versions(&skill_id).map_err(to_err)
}

#[tauri::command]
async fn skill_center_prepare_rollback(
    app: AppHandle,
    skill_id: String,
    version_id: String,
) -> Result<skill_center::SkillUpdateCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SkillCenter>()
            .prepare_rollback(&skill_id, &version_id)
            .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn skill_center_apply_update(
    app: AppHandle,
    skill_id: String,
    check_id: String,
    trust_confirmed: bool,
) -> Result<SkillAsset, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SkillCenter>()
            .apply_update(&skill_id, &check_id, trust_confirmed)
            .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn skill_center_sync_update(
    app: AppHandle,
    skill_id: String,
    deployment_ids: Vec<String>,
    expected_hash: String,
) -> Result<Vec<SkillDeployment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SkillCenter>()
            .sync_update_deployments(&skill_id, deployment_ids, &expected_hash)
            .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn skill_center_import(
    input: ImportSkillInput,
    app: AppHandle,
) -> Result<Vec<SkillAsset>, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.import(input).map_err(to_err)
    }).await
}
#[tauri::command]
fn skill_center_market_repositories(
    state: State<'_, SkillCenter>,
) -> Result<Vec<SkillMarketRepository>, String> {
    state.list_market_repositories().map_err(to_err)
}

#[tauri::command]
fn skill_center_market_repository_save(
    input: SaveSkillMarketRepositoryInput,
    state: State<'_, SkillCenter>,
) -> Result<SkillMarketRepository, String> {
    state.save_market_repository(input).map_err(to_err)
}

#[tauri::command]
fn skill_center_market_repository_delete(
    id: String,
    state: State<'_, SkillCenter>,
) -> Result<(), String> {
    state.delete_market_repository(&id).map_err(to_err)
}

#[tauri::command]
async fn skill_center_market_browse(
    app: AppHandle,
) -> Result<SkillMarketBrowseResult, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.browse_market().map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_skills_sh_search(
    query: String,
    limit: usize,
    offset: usize,
    state: State<'_, SkillCenter>,
) -> Result<SkillsShSearchResult, String> {
    state
        .search_skills_sh(&query, limit, offset)
        .await
        .map_err(to_err)
}

#[tauri::command]
async fn skill_center_market_install(
    input: InstallSkillMarketInput,
    app: AppHandle,
) -> Result<SkillAsset, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.install_market(input).map_err(to_err)
    }).await
}

#[tauri::command]
fn skill_center_set_trust(
    skill_id: String,
    trust_state: String,
    state: State<'_, SkillCenter>,
) -> Result<SkillAsset, String> {
    state.set_trust(&skill_id, &trust_state).map_err(to_err)
}

#[tauri::command]
async fn skill_center_delete_asset(
    skill_id: String,
    app: AppHandle,
) -> Result<(), String> {
    blocking_command(move || app.state::<SkillCenter>().delete_asset(&skill_id).map_err(to_err)).await
}

#[tauri::command]
async fn skill_center_preview(
    skill_id: String,
    app: AppHandle,
) -> Result<SkillPreview, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.preview(&skill_id).map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_read_file(
    skill_id: String,
    relative_path: String,
    app: AppHandle,
) -> Result<String, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.read_file(&skill_id, &relative_path).map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_discover(
    project_paths: Vec<String>,
    app: AppHandle,
) -> Result<Vec<DiscoveredSkill>, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.discover(&project_paths).map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_deploy_batch(
    inputs: Vec<DeploySkillInput>,
    app: AppHandle,
) -> Result<Vec<SkillDeployment>, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.deploy_batch(inputs).map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_undeploy(
    deployment_id: String,
    app: AppHandle,
) -> Result<(), String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.undeploy(&deployment_id).map_err(to_err)
    }).await
}

#[tauri::command]
async fn skill_center_undeploy_batch(
    deployment_ids: Vec<String>,
    app: AppHandle,
) -> Result<(), String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.undeploy_batch(deployment_ids).map_err(to_err)
    }).await
}

#[tauri::command]
fn skill_center_save_set(
    input: SaveSkillSetInput,
    state: State<'_, SkillCenter>,
) -> Result<SkillSet, String> {
    state.save_skill_set(input).map_err(to_err)
}

#[tauri::command]
fn skill_center_delete_set(id: String, state: State<'_, SkillCenter>) -> Result<(), String> {
    state.delete_skill_set(&id).map_err(to_err)
}

#[tauri::command]
async fn skill_center_apply_set(
    id: String,
    trust_confirmed: bool,
    app: AppHandle,
) -> Result<Vec<SkillDeployment>, String> {
    blocking_command(move || {
        let state = app.state::<SkillCenter>();
        state.apply_skill_set(&id, trust_confirmed).map_err(to_err)
    }).await
}

#[tauri::command]
fn git_settings_get() -> Result<GitSettings, String> {
    git_source_control::load_settings().map_err(to_err)
}

#[tauri::command]
fn git_settings_save(settings: GitSettings) -> Result<GitSettings, String> {
    git_source_control::save_settings(settings).map_err(to_err)
}

#[tauri::command]
async fn git_status(
    cwd: String,
    compare_strategy: Option<String>,
) -> Result<GitStatusSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::status(std::path::Path::new(&cwd), compare_strategy.as_deref())
            .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_diff(
    cwd: String,
    path: Option<String>,
    area: String,
    base: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::diff(
            std::path::Path::new(&cwd),
            path.as_deref(),
            &area,
            base.as_deref(),
        )
        .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_stage(cwd: String, paths: Vec<String>) -> Result<GitStatusSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::stage(std::path::Path::new(&cwd), &paths).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_unstage(cwd: String, paths: Vec<String>) -> Result<GitStatusSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::unstage(std::path::Path::new(&cwd), &paths).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_discard(
    cwd: String,
    paths: Vec<String>,
    confirmed: bool,
) -> Result<GitStatusSnapshot, String> {
    if !confirmed {
        return Err("Discard 需要明确确认".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::discard(std::path::Path::new(&cwd), &paths).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_commit(
    cwd: String,
    message: String,
    push_after: bool,
) -> Result<GitActionReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::commit(std::path::Path::new(&cwd), &message, push_after).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_remote(
    cwd: String,
    action: String,
    confirmed: bool,
) -> Result<GitActionReceipt, String> {
    if action == "force_push" && !confirmed {
        return Err("Force Push 需要明确确认".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::remote(std::path::Path::new(&cwd), &action).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}
#[tauri::command]
async fn git_branches(cwd: String) -> Result<Vec<GitBranchOption>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::branches(std::path::Path::new(&cwd)).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_merge(
    cwd: String,
    source: String,
    no_ff: bool,
    confirmed: bool,
) -> Result<GitActionReceipt, String> {
    if !confirmed {
        return Err("Merge 需要明确确认".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::merge(std::path::Path::new(&cwd), &source, no_ff).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_merge_abort(cwd: String, confirmed: bool) -> Result<GitActionReceipt, String> {
    if !confirmed {
        return Err("取消 Merge 需要明确确认".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::merge_abort(std::path::Path::new(&cwd)).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn git_generate_commit_message(
    cwd: String,
    provider_id: String,
    instruction: String,
) -> Result<String, String> {
    let provider = providers::local_cli();
    let key = String::new();
    tauri::async_runtime::spawn_blocking(move || {
        git_source_control::generate_commit_message(
            std::path::Path::new(&cwd),
            &provider,
            &key,
            &instruction,
        )
        .map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

fn standalone_runtime_kind(record: &SessionRecord) -> Result<AgentRuntimeKind, String> {
    match record.agent_runtime_kind.as_deref() {
        Some("claude_code") => Ok(AgentRuntimeKind::ClaudeCode),
        Some("codex") => Ok(AgentRuntimeKind::Codex),
        Some("kimi_code") => Ok(AgentRuntimeKind::KimiCode),
        Some("minimax_code") => Err("MiniMax CLI 已移除，历史会话不能启动或恢复".into()),
        Some("pi") => Ok(AgentRuntimeKind::Pi),
        Some("oh_my_pi") => Err("该 CLI 已移除，历史会话不能启动或恢复".to_string()),
        Some("opencode") => Ok(AgentRuntimeKind::OpenCode),
        Some("deepseek_harness") => Ok(AgentRuntimeKind::DeepSeekHarness),
        Some("zcode") => Ok(AgentRuntimeKind::ZCode),
        Some("zcode_cli") => Ok(AgentRuntimeKind::ZCodeCli),
        Some("antigravity") => Ok(AgentRuntimeKind::Antigravity),
        Some(value) => Err(format!("暂不支持交互式 Runtime 会话: {value}")),
        None => Err("Runtime 会话缺少 Runtime 类型".to_string()),
    }
}

fn configure_yolo_runtime_command(
    kind: AgentRuntimeKind,
    command: &mut Command,
    record: &SessionRecord,
    session_state: &SessionManager,
) -> Result<(), String> {
    match kind {
        AgentRuntimeKind::ClaudeCode => {
            command
                .args(["--permission-mode", "bypassPermissions"])
                .arg("--dangerously-skip-permissions");
        }
        AgentRuntimeKind::ClaudeChat => {}
        AgentRuntimeKind::KimiChat => {}
        AgentRuntimeKind::PiChat => {}
        AgentRuntimeKind::CodexChat => {}
        AgentRuntimeKind::OpenCodeChat => {}
        AgentRuntimeKind::Codex => {
            let notify = session_state
                .agent_turn_hook_command(&record.id)
                .map_err(to_err)?;
            command
                .arg("--no-alt-screen")
                .arg("--dangerously-bypass-hook-trust")
                .arg("--dangerously-bypass-approvals-and-sandbox")
                .arg("-c")
                .arg(format!(
                    "notify={}",
                    serde_json::to_string(&notify).map_err(to_err)?
                ));
        }
        AgentRuntimeKind::KimiCode => {
            command.arg("--yolo");
        }
        AgentRuntimeKind::Unsupported => return Err("该 Runtime 已移除或不受支持，不能启动或恢复".into()),
        AgentRuntimeKind::MiniMaxCode => return Err("MiniMax CLI 已移除，历史会话不能启动或恢复".into()),
        AgentRuntimeKind::Pi => {}
        AgentRuntimeKind::OhMyPi => {
            return Err("该 CLI 已移除，历史会话不能启动或恢复".to_string())
        }
        AgentRuntimeKind::OpenCode => {
            command
                .arg("--dangerously-skip-permissions")
                .arg("--dir")
                .arg(&record.cwd);
        }
        AgentRuntimeKind::ZCode => {
            return Err("ZCode Desktop 通过协作项目原生协议执行，不支持 PTY 会话".into())
        }
        // zcode CLI TUI 在会话内自行管理权限模式，启动无需附加参数。
        AgentRuntimeKind::ZCodeCli => {}
        AgentRuntimeKind::Antigravity => return Err("Antigravity CLI 请在原生会话内管理权限".into()),
        AgentRuntimeKind::DeepSeekHarness => {
            return Err("DeepSeek Harness 通过协作项目 ACP 执行，不支持 PTY 会话".into())
        }
    }
    Ok(())
}

fn configure_runtime_communication(
    kind: AgentRuntimeKind,
    command: &mut Command,
    record: &SessionRecord,
    session_state: &SessionManager,
) -> Result<(), String> {
    // Readiness hooks and tool identity are required in ordinary permission mode too.
    // Keeping them separate from YOLO must never grant additional permissions.
    if kind == AgentRuntimeKind::ClaudeCode {
        let hook = session_state.hook_command(&record.id).map_err(to_err)?;
        let mut identity = HashMap::from([("CREWVELA_SESSION_ID".to_string(), record.id.clone())]);
        let executable = std::env::current_exe().map_err(to_err)?;
        identity.insert("CREWVELA_CLI".to_string(), executable.display().to_string());
        command
            .arg("--settings")
            .arg(pty::build_settings_json(&identity, &hook));
    }
    if kind == AgentRuntimeKind::Codex {
        pty::configure_codex_cli_identity(command, &record.id).map_err(to_err)?;
        let notify = session_state.agent_turn_hook_command(&record.id).map_err(to_err)?;
        command.arg("-c").arg(format!("notify={}", serde_json::to_string(&notify).map_err(to_err)?));
        // 保留配置中启用的 stdio server（浏览器控制等）注入 codex 会话；
        // 读取失败在 mcp 侧静默返回空列表，不阻塞会话启动。
        for override_value in crate::mcp::codex_cli_mcp_overrides() {
            command.arg("-c").arg(override_value);
        }
        session_mcp::configure_codex(command, &record.id).map_err(to_err)?;
    }
    ai_memory::embedded::configure_cli(command, record, kind).map_err(to_err)?;
    if kind == AgentRuntimeKind::Pi {
        session_state
            .configure_pi_readiness(command, &record.id)
            .map_err(to_err)?;
    }
    Ok(())
}

fn open_runtime_session_with_yolo(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    yolo: bool,
) -> Result<SessionRecord, String> {
    open_runtime_session_checked(record, pty_state, session_state, yolo, None, None)
}

fn open_runtime_session_for_switch(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    switch_id: &str,
) -> Result<SessionRecord, String> {
    open_runtime_session_checked(record, pty_state, session_state, false, Some(switch_id), None)
}

fn open_runtime_session_checked(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    yolo: bool,
    switch_id: Option<&str>,
    _member_store: Option<&MultiAgentStore>,
) -> Result<SessionRecord, String> {
    let _admission = pty_state.admit_start().map_err(to_err)?;
    worktree_management::verify_start(record, session_state).map_err(to_err)?;
    let yolo = session_switch::check_runtime_start(&record.id, yolo, switch_id).map_err(to_err)?;
    let kind = standalone_runtime_kind(record)?;
    kind.ensure_available().map_err(to_err)?;
    if matches!(kind, AgentRuntimeKind::DeepSeekHarness | AgentRuntimeKind::ZCode) {
        return Err("该 Runtime 通过协作项目协议执行，不支持独立 PTY 会话".into());
    }
    if let Some(pty_id) = record.pty_id {
        if pty_state.is_running(pty_id) {
            return Ok(record.clone());
        }
        pty_state.kill(pty_id).map_err(to_err)?;
    }
    pty_state.kill_for_app_session(&record.id).map_err(to_err)?;

    let executable = multi_agent::resolve_runtime_executable_checked(kind)?;
    let _starting = session_state
        .begin_runtime_start(&record.id)
        .map_err(to_err)?;
    let codex_handoff = kind == AgentRuntimeKind::Codex && session_switch::uses_codex_handoff(&record.id).map_err(to_err)?;
    // zcode CLI 独立会话直接进入交互式 TUI；cjs 入口需要 node 直启。
    let mut command: Command = if kind == AgentRuntimeKind::ZCodeCli {
        multi_agent::zcode::cli_tui_command().map_err(to_err)?
    } else if codex_handoff {
        multi_agent::codex_handoff_command(&executable)
    } else {
        multi_agent::command_for_path(&executable)
    };
    let runtime_cwd = multi_agent::external_command_path(std::path::Path::new(&record.cwd));
    if kind == AgentRuntimeKind::Antigravity { runtime_installation::configure_managed_antigravity(&mut command, &executable); }
    command.current_dir(&runtime_cwd);
    command.env("TERM", "xterm-256color");
    if kind == AgentRuntimeKind::Codex && record.multi_agent_agent_id.is_some() {
        record.codex_storage.prepare().map_err(to_err)?;
        record.codex_storage.verify_device_policy(&executable, &runtime_cwd).map_err(to_err)?;
        record.codex_storage.configure(&mut command, false).map_err(to_err)?;
    } else if codex_handoff {
        record.codex_storage.configure(&mut command, false).map_err(to_err)?;
    }
    session_state
        .configure_runtime_resume(&mut command, record)
        .map_err(to_err)?;
    configure_runtime_communication(kind, &mut command, record, session_state)?;
    // Handoffs own their permission contract. Never apply a launch preference
    // over the constraints of a transferred session.
    if switch_id.is_none() && !session_switch::is_handoff_target(&record.id).map_err(to_err)? {
        if let Some(mode) = record.cli_launch_mode.as_deref() {
            runtime_launch::configure(&mut command, kind.as_str(), mode).map_err(to_err)?;
        }
    }
    session_switch::configure_target(&mut command, record).map_err(to_err)?;
    if yolo {
        configure_yolo_runtime_command(kind, &mut command, record, session_state)?;
    }
    if kind == AgentRuntimeKind::Codex {
        codex_communication::configure(
            &mut command,
            &executable,
            &runtime_cwd,
            &providers::config_dir().map_err(to_err)?.join("outbox"),
        ).map_err(to_err)?;
    }
    let handoff = if codex_handoff {
        session_switch::prepare_codex_launch(&mut command, &record.id).map_err(to_err)?
    } else { None };
    let pty_id = pty_state
        .open_agent_program_with_handoff(&command, &record.id, yolo, handoff.as_ref())
        .map_err(to_err)?;
    match session_state.attach_runtime(&record.id, pty_id) {
        Ok(updated) => Ok(updated),
        Err(error) => {
            match pty_state.kill(pty_id) {
                Ok(()) => Err(error.to_string()),
                Err(stop) => Err(format!("保存会话失败：{error}；新终端停止未获确认，保留跟踪并禁止替代启动：{stop}")),
            }
        }
    }
}

/// 单个 claude 会话的创建+启动+初始 prompt 排队：launch_multi 与自动化 Launch 动作共用。
#[allow(clippy::too_many_arguments)]
fn launch_one_session(
    provider: &ProviderConfig,
    key: &str,
    cwd: String,
    yolo: bool,
    auto_mode: bool,
    use_worktree: bool,
    initial_prompt: Option<&str>,
    project_id: Option<String>,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
) -> Result<SessionRecord, String> {
    let cwd = if project_id.is_some() {
        runtime_project_cwd(Some(cwd), project_id.as_deref(), |id| projects::get(id).map_err(to_err))?
    } else { cwd };
    let options = use_worktree.then(worktree::WorktreeOptions::default);
    let record = worktree_management::create_session(session_state, &cwd, options.as_ref(),
        |id, actual, tree| session_state.create_claude_in(NewSession {
            provider_id: provider.id.clone(), provider_name: provider.name.clone(),
            cwd: actual, yolo, auto_mode, project_id,
        }, id, tree), |_| Ok(()))?;
    let lifecycle = session_state.lifecycle_lock(&record.id);
    let _guard = lifecycle.lock().unwrap();
    let record = session_state.get(&record.id).ok_or_else(|| "会话已删除".to_string())?;
    match open_session(&record, provider, key, false, pty_state, session_state) {
        Ok(started) => {
            if let Some(prompt) = initial_prompt {
                action_queue.enqueue_many(&[started.id.clone()], prompt.to_string());
            }
            Ok(started)
        }
        Err(error) => {
            let _ = session_state.mark_error(&record.id, error.clone());
            Err(error)
        }
    }
}

#[tauri::command(async)]
fn launch_multi(
    provider_id: String,
    cwds: Vec<String>,
    count: u32,
    yolo: bool,
    auto_mode: bool,
    use_worktree: Option<bool>,
    initial_prompt: Option<String>,
    project_id: Option<String>,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<Vec<SessionRecord>, String> {
    if yolo && auto_mode {
        return Err("Auto 模式与 YOLO 模式不能同时启用".to_string());
    }
    let use_worktree = use_worktree.unwrap_or(false);
    // 归属项目必须真实存在，避免前端传入已删除项目产生孤儿分组。
    if let Some(project_id) = project_id.as_deref() {
        if projects::get(project_id).map_err(to_err)?.is_none() {
            return Err(format!("未找到项目: {project_id}"));
        }
    }
    // 初始 Prompt 逐字保留（与广播同规则），仅过滤空白；排队后由
    // SessionStart hook 置位 ready，pty_poll_all 的 flush_ready 自动派发——
    // readiness 是 hook 驱动的，trust 对话框确认前绝不会发出。
    let initial_prompt = initial_prompt.filter(|prompt| !prompt.trim().is_empty());
    if let Some(prompt) = &initial_prompt {
        if prompt.chars().count() > 32_000 {
            return Err("初始 Prompt 不能超过 32000 个字符".to_string());
        }
    }
    let provider = providers::get(&provider_id)
        .map_err(to_err)?
        .ok_or_else(|| format!("未找到 provider: {provider_id}"))?;
    let key = String::new();
    let mut sessions = Vec::new();
    for i in 0..count {
        let cwd = launcher::pick_cwd(&cwds, i as usize)
            .map(str::to_string)
            .or_else(|| dirs::home_dir().map(|path| path.display().to_string()))
            .unwrap_or_else(|| ".".to_string());
        sessions.push(launch_one_session(
            &provider,
            &key,
            cwd,
            yolo,
            auto_mode,
            use_worktree,
            initial_prompt.as_deref(),
            project_id.clone(),
            &pty_state,
            &session_state,
            &action_queue,
        )?);
    }
    Ok(sessions)
}

#[tauri::command]
fn launch_terminal(
    shell: TerminalShell,
    cwd: Option<String>,
    project_id: Option<String>,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
) -> Result<SessionRecord, String> {
    let cwd = runtime_project_cwd(cwd, project_id.as_deref(), |id| projects::get(id).map_err(to_err))?;
    let record = session_state
        .create_terminal(NewTerminalSession {
            shell,
            cwd,
            project_id,
        })
        .map_err(to_err)?;
    let lifecycle = session_state.lifecycle_lock(&record.id);
    let _guard = lifecycle.lock().unwrap();
    let record = session_state.get(&record.id).ok_or_else(|| "会话已删除".to_string())?;
    match open_terminal_session(&record, &pty_state, &session_state) {
        Ok(started) => Ok(started),
        Err(error) => {
            let _ = session_state.mark_error(&record.id, error.clone());
            Err(error)
        }
    }
}

fn runtime_project_cwd(
    cwd: Option<String>,
    project_id: Option<&str>,
    lookup: impl FnOnce(&str) -> Result<Option<projects::Project>, String>,
) -> Result<String, String> {
    let cwd = cwd.filter(|value| !value.trim().is_empty());
    if let Some(id) = project_id {
        let project = lookup(id)?.ok_or_else(|| format!("未找到项目: {id}"))?;
        let cwd = cwd.unwrap_or_else(|| project.path.clone());
        let project_path = std::fs::canonicalize(&project.path).map_err(|error| format!("项目目录不可访问: {error}"))?;
        let cwd_path = std::fs::canonicalize(&cwd).map_err(|error| format!("工作目录不可访问: {error}"))?;
        if !project_path.is_dir() || !cwd_path.is_dir() || !cwd_path.starts_with(&project_path) {
            return Err("项目会话必须使用该项目目录或其子目录".into());
        }
        return Ok(cwd);
    }
    Ok(cwd.or_else(|| dirs::home_dir().map(|path| path.display().to_string())).unwrap_or_else(|| ".".into()))
}

// Permission probing waits for a child process; keep it off the WebView/UI thread.
#[tauri::command(async)]
fn launch_runtime_session(
    runtime_kind: String,
    cwd: Option<String>,
    project_id: Option<String>,
    launch_mode: Option<String>,
    worktree_options: Option<worktree::WorktreeOptions>,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
) -> Result<SessionRecord, String> {
    let mode = runtime_launch::resolve(&runtime_kind, launch_mode.as_deref(), &runtime_preferences::get().map_err(to_err)?.default_launch_modes).map_err(to_err)?;
    let cwd = runtime_project_cwd(cwd, project_id.as_deref(), |id| projects::get(id).map_err(to_err))?;
    let record = worktree_management::create_session(&session_state, &cwd, worktree_options.as_ref(),
        |id, actual, tree| session_state.create_runtime_in(NewRuntimeSession {
            runtime_kind, cwd: actual, project_id,
        }, id, Some(mode), tree, None), |_| Ok(()))?;
    let lifecycle = session_state.lifecycle_lock(&record.id);
    let _guard = lifecycle.lock().unwrap();
    let record = session_state.get(&record.id).ok_or_else(|| "会话已删除".to_string())?;
    match open_runtime_session(&record, &pty_state, &session_state) {
        Ok(started) => Ok(started),
        Err(error) => {
            let _ = session_state.mark_error(&record.id, error.clone());
            Err(error)
        }
    }
}


#[tauri::command(async)]
fn create_zcode_session(cwd: String, project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>, session_state: State<'_, SessionManager>, native_zcode: State<'_, NativeZCodeSessions>) -> Result<SessionRecord,String> {
    let _lease = runtime_preferences::acquire(AgentRuntimeKind::ZCode).map_err(to_err)?;
    worktree_management::create_chat(&session_state, "zcode", cwd, project_id, worktree_options, source_session_id,
        |record| native_zcode.register(&record.id, &record.cwd))
}
#[tauri::command]
fn zcode_snapshot(session_id:String,native_zcode:State<'_,NativeZCodeSessions>)->Result<ZCodeSnapshot,String> { native_zcode.snapshot(&session_id).map_err(to_err) }
#[tauri::command]
async fn start_zcode_session(app:AppHandle,session_id:String,session_state:State<'_,SessionManager>,native_zcode:State<'_,NativeZCodeSessions>)->Result<ZCodeSnapshot,String> {
    let record=session_state.get(&session_id).ok_or_else(||format!("未找到会话: {session_id}"))?;
    let manager=native_zcode.inner().clone();let sessions=session_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {let lock=sessions.lifecycle_lock(&record.id);let _guard=lock.lock().unwrap();session_switch::ensure_can_start(&record.id)?;manager.start(app,&sessions,&record)}).await.map_err(to_err)?.map_err(to_err)
}
fn zcode_input_admission(
    sessions: &SessionManager,
    id: &str,
    admit: impl FnOnce() -> anyhow::Result<native_zcode_sessions::PendingZCodeInput>,
) -> anyhow::Result<native_zcode_sessions::PendingZCodeInput> {
    let lifecycle = sessions.lifecycle_lock(id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(id)?;
    admit()
}

fn send_zcode_message_blocking(sessions: &SessionManager, manager: &NativeZCodeSessions, id: &str, text: &str, attachments: &[String], plan_enabled: bool) -> anyhow::Result<String> {
    let pending = zcode_input_admission(sessions, id, || manager.admit_input(id, text, attachments, plan_enabled))?;
    pending.wait()
}

#[tauri::command]
async fn send_zcode_message(
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    plan_enabled: Option<bool>,
    session_state: State<'_, SessionManager>,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<String, String> {
    let manager = native_zcode.inner().clone();
    let sessions = session_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        send_zcode_message_blocking(&sessions, &manager, &session_id, &message, &attachments.unwrap_or_default(), plan_enabled.unwrap_or(false))
    })
    .await
    .map_err(to_err)?
    .map_err(to_err)
}
#[tauri::command]
async fn edit_zcode_message(session_id: String, row_id: u64, entity_id: String, message: String, session_state: State<'_, SessionManager>, native_zcode: State<'_, NativeZCodeSessions>) -> Result<String, String> {
    let manager = native_zcode.inner().clone();
    let sessions = session_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pending = zcode_input_admission(&sessions, &session_id, || manager.admit_edit(&session_id, row_id, &entity_id, &message))?;
        pending.wait()
    }).await.map_err(to_err)?.map_err(to_err)
}
#[tauri::command]
async fn zcode_slash_command(
    session_id: String,
    kind: String,
    text: Option<String>,
    display_text: Option<String>,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<(), String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.slash_command(&session_id, &kind, text.as_deref(), display_text.as_deref())
    })
    .await
    .map_err(to_err)?
    .map_err(to_err)
}
#[tauri::command]
async fn zcode_side_session(
    session_id: String,
    first_input: Option<String>,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<ZCodeSnapshot, String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.side_session(&session_id, first_input.as_deref()))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}
#[tauri::command]
async fn zcode_side_send(
    session_id: String,
    message: String,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<ZCodeSnapshot, String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.side_send_message(&session_id, &message))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}
#[tauri::command]
async fn zcode_side_close(
    session_id: String,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<ZCodeSnapshot, String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.side_close(&session_id))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}
#[tauri::command]
async fn zcode_composer_catalog(session_id:String,native_zcode:State<'_,NativeZCodeSessions>)->Result<serde_json::Value,String> {
    let manager=native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.composer_catalog(&session_id)).await.map_err(to_err)?.map_err(to_err)
}
#[tauri::command]
fn zcode_attachment_info(session_id:String,paths:Vec<String>,native_zcode:State<'_,NativeZCodeSessions>)->Result<Vec<serde_json::Value>,String> {native_zcode.attachment_info(&session_id,&paths).map_err(to_err)}
#[tauri::command]
async fn set_zcode_model(session_id:String,model_id:String,native_zcode:State<'_,NativeZCodeSessions>)->Result<ZCodeSnapshot,String> {
    let manager=native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.set_model(&session_id,&model_id)).await.map_err(to_err)?.map_err(to_err)
}
#[tauri::command]
async fn set_zcode_permission_mode(session_id:String,mode:String,native_zcode:State<'_,NativeZCodeSessions>)->Result<ZCodeSnapshot,String> {
    let manager=native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.set_permission_mode(&session_id,&mode)).await.map_err(to_err)?.map_err(to_err)
}
#[tauri::command]
async fn zcode_queue_action(
    session_id: String,
    queue_item_id: Option<String>,
    op: Option<String>,
    new_text: Option<String>,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<ZCodeSnapshot, String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.queue_action_op(&session_id, queue_item_id.as_deref(), op.as_deref(), new_text.as_deref())
    })
    .await
    .map_err(to_err)?
    .map_err(to_err)
}
#[tauri::command]
async fn zcode_child_action(session_id: String, child_session_id: String, request_token: String, open: bool, native_zcode: State<'_, NativeZCodeSessions>) -> Result<(), String> {
    let manager = native_zcode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.child_action(&session_id, &child_session_id, &request_token, open)).await.map_err(to_err)?.map_err(to_err)
}
#[tauri::command]
fn recover_zcode_queue_input(
    session_id: String,
    queue_item_id: String,
    discard: Option<bool>,
    native_zcode: State<'_, NativeZCodeSessions>,
) -> Result<serde_json::Value, String> {
    native_zcode
        .recover_queue_input(&session_id, &queue_item_id, discard.unwrap_or(false))
        .map_err(to_err)
}
#[tauri::command]
fn answer_zcode_permission(session_id:String,request_id:String,allow:bool,native_zcode:State<'_,NativeZCodeSessions>)->Result<(),String> {native_zcode.answer(&session_id,&request_id,allow).map_err(to_err)}
#[tauri::command]
fn cancel_zcode_turn(session_id:String,native_zcode:State<'_,NativeZCodeSessions>)->Result<bool,String> {native_zcode.cancel(&session_id).map_err(to_err)}

#[tauri::command(async)]
fn create_deepseek_harness_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<SessionRecord, String> {
    AgentRuntimeKind::DeepSeekHarness.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "deepseek_harness", cwd, project_id, worktree_options, source_session_id,
        |record| native_acp.register(&record.id, &record.cwd))
}

#[tauri::command]
fn deepseek_harness_snapshot(
    session_id: String,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_acp.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_deepseek_harness_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_deepseek_harness(&record) {
        return Err("会话不是独立 DeepSeek Harness ACP 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_acp = native_acp.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_acp.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_deepseek_harness_model(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_acp = native_acp.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_acp.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn control_deepseek_harness_session(
    app: AppHandle, session_id: String, action: String, value: String,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_acp = native_acp.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_acp.control(&app, &session_id, &action, &value))
        .await.map_err(to_err)?.map_err(to_err)
}

#[tauri::command]
fn send_deepseek_harness_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(&session_id).map_err(to_err)?;

    native_acp
        .send_message_with_attachments(app, &session_state, &session_id, &message, &attachments.unwrap_or_default())
        .map_err(to_err)
}

#[tauri::command]
fn answer_deepseek_harness_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<(), String> {
    native_acp
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_deepseek_harness_turn(
    session_id: String,
    native_acp: State<'_, NativeAcpSessions>,
) -> Result<bool, String> {
    native_acp.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn create_claude_chat_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<SessionRecord, String> {
    AgentRuntimeKind::ClaudeChat.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "claude_chat", cwd, project_id, worktree_options, source_session_id,
        |record| native_claude.0.register(&record.id, &record.cwd))
}

#[tauri::command]
fn claude_chat_snapshot(
    session_id: String,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_claude.0.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_claude_chat_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_claude_chat(&record) {
        return Err("会话不是独立 Claude Code Chat ACP 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_claude = native_claude.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_claude.0.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_claude_chat_config(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_claude = native_claude.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_claude.0.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
fn send_claude_chat_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(&session_id).map_err(to_err)?;

    native_claude
        .0
        .send_message_with_attachments(app, &session_state, &session_id, &message, &attachments.unwrap_or_default())
        .map_err(to_err)
}

#[tauri::command]
fn answer_claude_chat_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<(), String> {
    native_claude
        .0
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_claude_chat_turn(
    session_id: String,
    native_claude: State<'_, NativeClaudeSessions>,
) -> Result<bool, String> {
    native_claude.0.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn create_kimi_chat_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<SessionRecord, String> {
    // kimi_chat 原生聊天持独立启停开关；探测/升级仍复用 kimi_code CLI。
    AgentRuntimeKind::KimiChat.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "kimi_chat", cwd, project_id, worktree_options, source_session_id,
        |record| native_kimi.0.register(&record.id, &record.cwd))
}

#[tauri::command]
fn kimi_chat_snapshot(
    session_id: String,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_kimi.0.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_kimi_chat_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_kimi_chat(&record) {
        return Err("会话不是独立 Kimi Code Chat ACP 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_kimi = native_kimi.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_kimi.0.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_kimi_chat_config(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_kimi = native_kimi.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_kimi.0.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
fn send_kimi_chat_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(&session_id).map_err(to_err)?;

    native_kimi
        .0
        .send_message_with_attachments(app, &session_state, &session_id, &message, &attachments.unwrap_or_default())
        .map_err(to_err)
}

#[tauri::command]
fn answer_kimi_chat_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<(), String> {
    native_kimi
        .0
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_kimi_chat_turn(
    session_id: String,
    native_kimi: State<'_, NativeKimiSessions>,
) -> Result<bool, String> {
    native_kimi.0.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn create_opencode_chat_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<SessionRecord, String> {
    // opencode_chat 原生聊天持独立启停开关；探测/升级仍复用 opencode CLI。
    AgentRuntimeKind::OpenCodeChat.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "opencode_chat", cwd, project_id, worktree_options, source_session_id,
        |record| native_opencode.0.register(&record.id, &record.cwd))
}

#[tauri::command]
fn opencode_chat_snapshot(
    session_id: String,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_opencode.0.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_opencode_chat_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_opencode_chat(&record) {
        return Err("会话不是独立 OpenCode Chat ACP 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_opencode = native_opencode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_opencode.0.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_opencode_chat_config(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_opencode = native_opencode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_opencode.0.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
fn send_opencode_chat_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(&session_id).map_err(to_err)?;

    native_opencode
        .0
        .send_message_with_attachments(app, &session_state, &session_id, &message, &attachments.unwrap_or_default())
        .map_err(to_err)
}

#[tauri::command]
fn answer_opencode_chat_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<(), String> {
    native_opencode
        .0
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_opencode_chat_turn(
    session_id: String,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<bool, String> {
    native_opencode.0.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn create_codex_chat_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<SessionRecord, String> {
    // codex_chat 原生聊天持独立启停开关；适配器仍需本机 codex CLI 已登录。
    AgentRuntimeKind::CodexChat.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "codex_chat", cwd, project_id, worktree_options, source_session_id,
        |record| native_codex.0.register(&record.id, &record.cwd))
}

#[tauri::command]
fn codex_chat_snapshot(
    session_id: String,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_codex.0.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_codex_chat_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_codex_chat(&record) {
        return Err("会话不是独立 Codex Chat ACP 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_codex = native_codex.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_codex.0.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_codex_chat_config(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_codex = native_codex.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_codex.0.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn send_codex_chat_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    plugins: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<(), String> {
    let manager = native_codex.0.clone();
    let sessions = session_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = sessions.lifecycle_lock(&session_id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&session_id)?; manager.send_codex_message_with_plugins(app, &sessions, &session_id, &message, &attachments.unwrap_or_default(), &plugins.unwrap_or_default()) })
        .await.map_err(to_err)?.map_err(to_err)
}

#[tauri::command]
async fn get_codex_chat_plugins(session_id: String, native_codex: State<'_, NativeCodexChatSessions>) -> Result<serde_json::Value, String> {
    let manager = native_codex.0.clone();
    tauri::async_runtime::spawn_blocking(move || manager.codex_plugins(&session_id)).await.map_err(to_err)?.map_err(to_err)
}

#[tauri::command]
fn control_codex_chat_goal(
    app: AppHandle,
    session_id: String,
    action: String,
    objective: Option<String>,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<(), String> {
    native_codex.0.control_codex_goal(app, &session_id, &action, objective.as_deref()).map_err(to_err)
}

#[tauri::command]
fn answer_codex_chat_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<(), String> {
    native_codex
        .0
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_codex_chat_turn(
    session_id: String,
    native_codex: State<'_, NativeCodexChatSessions>,
) -> Result<bool, String> {
    native_codex.0.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn create_pi_chat_session(
    cwd: String,
    project_id: Option<String>, worktree_options: Option<worktree::WorktreeOptions>, source_session_id: Option<String>,
    session_state: State<'_, SessionManager>,
    native_pi: State<'_, NativePiSessions>,
) -> Result<SessionRecord, String> {
    // pi_chat 原生聊天持独立启停开关；探测/升级仍复用 pi CLI。
    AgentRuntimeKind::PiChat.ensure_available().map_err(to_err)?;
    worktree_management::create_chat(&session_state, "pi_chat", cwd, project_id, worktree_options, source_session_id,
        |record| native_pi.register(&record.id, &record.cwd))
}

#[tauri::command]
fn pi_chat_snapshot(
    session_id: String,
    native_pi: State<'_, NativePiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    native_pi.snapshot(&session_id).map_err(to_err)
}

#[tauri::command]
async fn start_pi_chat_session(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    native_pi: State<'_, NativePiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if !sessions::is_standalone_pi_chat(&record) {
        return Err("会话不是独立 Pi Chat 会话".into());
    }
    let app = app.clone();
    let session_state = session_state.inner().clone();
    let native_pi = native_pi.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { let lifecycle = session_state.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap(); session_switch::ensure_can_start(&record.id)?; native_pi.start(app, &session_state, &record) })
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
async fn set_pi_chat_config(
    session_id: String,
    value: String,
    config_id: Option<String>,
    native_pi: State<'_, NativePiSessions>,
) -> Result<HarnessSessionSnapshot, String> {
    let native_pi = native_pi.inner().clone();
    tauri::async_runtime::spawn_blocking(move || native_pi.set_config(&session_id, config_id.as_deref().unwrap_or("model"), &value))
        .await
        .map_err(to_err)?
        .map_err(to_err)
}

#[tauri::command]
fn send_pi_chat_message(
    app: AppHandle,
    session_id: String,
    message: String,
    attachments: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    native_pi: State<'_, NativePiSessions>,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_switch::ensure_can_start(&session_id).map_err(to_err)?;

    native_pi
        .send_message_with_attachments(app, &session_state, &session_id, &message, &attachments.unwrap_or_default())
        .map_err(to_err)
}

#[tauri::command]
fn answer_pi_chat_permission(
    session_id: String,
    request_id: String,
    allow: bool,
    native_pi: State<'_, NativePiSessions>,
) -> Result<(), String> {
    native_pi
        .answer_permission(&session_id, &request_id, allow)
        .map_err(to_err)
}

#[tauri::command]
fn cancel_pi_chat_turn(
    session_id: String,
    native_pi: State<'_, NativePiSessions>,
) -> Result<bool, String> {
    native_pi.cancel_turn(&session_id).map_err(to_err)
}

#[tauri::command]
async fn list_sessions(
    app: AppHandle,
    state: State<'_, SessionManager>,
) -> Result<Vec<SessionRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.ingest_events().map_err(to_err)?;
        flush_title_summaries(&app, &state);
        Ok(state.list())
    }).await
}

#[tauri::command]
async fn list_archived_sessions(
    app: AppHandle,
    state: State<'_, SessionManager>,
) -> Result<Vec<SessionRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        state.ingest_events().map_err(to_err)?;
        flush_title_summaries(&app, &state);
        Ok(state.list_archived())
    }).await
}

#[tauri::command]
async fn session_snapshot(app: AppHandle, state: State<'_, SessionManager>) -> Result<Vec<SessionRecord>, String> {
    let state = state.inner().clone();
    blocking_command(move || {
        flush_title_summaries(&app, &state);
        Ok(state.list())
    }).await
}

/// 全部会话的累计 token 用量（增量解析 transcript；前端低频轮询）。
#[tauri::command]
async fn session_usages(
    session_state: State<'_, SessionManager>,
    usage_state: State<'_, UsageTracker>,
) -> Result<HashMap<String, SessionUsage>, String> {
    let session_state = session_state.inner().clone();
    let usage_state = usage_state.inner().clone();
    blocking_command(move || Ok(usage_state.usage_batch(&session_state.list()))).await
}

#[tauri::command]
async fn session_telemetry(
    session_ids: Option<Vec<String>>,
    session_state: State<'_, SessionManager>,
    pty_state: State<'_, PtyManager>,
    usage_state: State<'_, UsageTracker>,
) -> Result<Vec<RuntimeTelemetrySnapshot>, String> {
    let session_state = session_state.inner().clone();
    let pty_state = pty_state.inner().clone();
    let usage_state = usage_state.inner().clone();
    blocking_command(move || {
        session_state.ingest_events().map_err(to_err)?;
        let requested = session_ids.map(|ids| ids.into_iter().collect::<HashSet<_>>());
        let sessions = session_state
            .list()
            .into_iter()
            .filter(|session| {
                requested
                    .as_ref()
                    .map_or(true, |ids| ids.contains(&session.id))
            })
            .collect::<Vec<_>>();
        Ok(telemetry::snapshot_batch(
            &sessions,
            &pty_state,
            &usage_state,
        ))
    }).await
}

/// 历史会话内容回看：transcript 尾部 200 条消息（user/assistant/tool 摘要）。
#[tauri::command]
async fn session_transcript(
    session_id: String,
    session_state: State<'_, SessionManager>,
) -> Result<Vec<TranscriptMessage>, String> {
    let record = session_state
        .get(&session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    blocking_command(move || transcript::read_transcript(&record, 200).map_err(to_err)).await
}

#[tauri::command]
async fn resume_session(
    app: AppHandle,
    session_id: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<SessionRecord, String> {
    let pty_state = pty_state.inner().clone();
    let session_state = session_state.inner().clone();
    let multi_agent_store = multi_agent_store.inner().clone();
    let native_acp = native_acp.inner().clone();
    let native_zcode = native_zcode.inner().clone();
    let native_claude = native_claude.inner().clone();
    let native_kimi = native_kimi.inner().clone();
    let native_codex = native_codex.inner().clone();
    let native_pi = native_pi.inner().clone();
    let native_opencode = native_opencode.inner().clone();
    // ACP 初始化和恢复可能等待子进程；不得阻塞 Tauri 窗口线程。
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = session_state.lifecycle_lock(&session_id);
        let _lifecycle = lifecycle.lock().unwrap();
        session_state.ingest_events().map_err(to_err)?;
        let record = session_state
            .get(&session_id)
            .ok_or_else(|| format!("未找到会话: {session_id}"))?;
        session_switch::ensure_can_start(&session_id).map_err(to_err)?;
        if sessions::is_standalone_zcode(&record) {
            native_zcode.start(app, &session_state, &record).map_err(to_err)?;
            return session_state.get(&session_id).ok_or_else(|| "ZCode Desktop 会话已删除".into());
        }
        if sessions::is_standalone_deepseek_harness(&record) {
            return match native_acp.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if sessions::is_standalone_claude_chat(&record) {
            return match native_claude.0.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if sessions::is_standalone_kimi_chat(&record) {
            return match native_kimi.0.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if sessions::is_standalone_codex_chat(&record) {
            return match native_codex.0.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if sessions::is_standalone_opencode_chat(&record) {
            return match native_opencode.0.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if sessions::is_standalone_pi_chat(&record) {
            return match native_pi.start(app, &session_state, &record) {
                Ok(_) => session_state.get(&session_id).ok_or_else(|| format!("未找到会话: {session_id}")),
                Err(error) => {
                    let message = to_err(error);
                    let _ = session_state.mark_error(&record.id, message.clone());
                    Err(message)
                }
            };
        }
        if let Some(project_id) = &record.multi_agent_project_id {
            if multi_agent_store
                .handoff_in_progress(project_id)
                .map_err(to_err)?
            {
                return Err("项目正在交接，不能恢复或替换原执行宿主".into());
            }
        }
        if record.retired_agent_runtime {
            return Err("该会话为 Runtime 切换前的只读历史，请选择成员当前会话".into());
        }
        if matches!(record.agent_runtime_kind.as_deref(), Some("oh_my_pi" | "minimax_code")) {
            return Err("该 CLI 已移除，历史会话不能启动或恢复".to_string());
        }
        if record.kind == SessionKind::Terminal {
            let is_standalone_runtime = record.agent_runtime_kind.is_some()
                && record.multi_agent_project_id.is_none()
                && record.multi_agent_agent_id.is_none();
            let opened = if is_standalone_runtime {
                open_runtime_session(&record, &pty_state, &session_state)
            } else {
                open_terminal_session(&record, &pty_state, &session_state)
            };
            return match opened {
                Ok(started) => Ok(started),
                Err(error) => {
                    let _ = session_state.mark_error(&record.id, error.clone());
                    Err(error)
                }
            };
        }
        let provider = providers::get(&record.provider_id)
            .map_err(to_err)?
            .ok_or_else(|| format!("未找到 provider: {}", record.provider_id))?;
        let key = String::new();
        match open_session(&record, &provider, &key, true, &pty_state, &session_state) {
            Ok(started) => Ok(started),
            Err(error) => {
                let _ = session_state.mark_error(&record.id, error.clone());
                Err(error)
            }
        }
    }).await.map_err(to_err)?
}

#[tauri::command]
fn rename_session(
    session_id: String,
    title: String,
    state: State<'_, SessionManager>,
) -> Result<SessionRecord, String> {
    state.rename(&session_id, &title).map_err(to_err)
}

fn stop_zcode_session_blocking<R: tauri::Runtime>(app: &AppHandle<R>, id: &str, sessions: &SessionManager, manager: &NativeZCodeSessions) -> Result<SessionRecord, String> {
    let lifecycle = sessions.lifecycle_lock(id);
    let _guard = lifecycle.lock().unwrap();
    let record = sessions.get(id).ok_or_else(|| "会话已删除".to_string())?;
    if !sessions::is_standalone_zcode(&record) { return Err("会话 Runtime 已变化".into()); }
    manager.stop(id, sessions, app).map_err(to_err)
}

#[tauri::command]
fn stop_session(
    session_id: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
    app: AppHandle,
) -> Result<SessionRecord, String> {
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_zcode(&record)) {
        return stop_zcode_session_blocking(&app, &session_id, &session_state, &native_zcode);
    }
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _lifecycle = lifecycle.lock().unwrap();
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_deepseek_harness(&record)) {
        return native_acp.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_zcode(&record)) {
        return native_zcode.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_claude_chat(&record)) {
        return native_claude.0.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_kimi_chat(&record)) {
        return native_kimi.0.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_codex_chat(&record)) {
        return native_codex.0.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_opencode_chat(&record)) {
        return native_opencode.0.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    if session_state.get(&session_id).is_some_and(|record| sessions::is_standalone_pi_chat(&record)) {
        return native_pi.stop(&session_id, &session_state, &app).map_err(to_err);
    }
    detach_session_runtime(&session_id, &pty_state, &session_state, &action_queue)?;
    session_state.mark_stopped(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn archive_session(
    session_id: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
    app: AppHandle,
) -> Result<SessionRecord, String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _lifecycle = lifecycle.lock().unwrap();
    let archived = (|| -> Result<SessionRecord, String> {
    let record = detach_session_runtime(&session_id, &pty_state, &session_state, &action_queue)?;
    if sessions::is_standalone_zcode(&record) { native_zcode.stop(&session_id,&session_state,&app).map_err(to_err)?; return session_state.archive(&session_id).map_err(to_err); }
    if sessions::is_standalone_deepseek_harness(&record) {
        return native_acp.archive(&session_id, &session_state, &app).map_err(to_err);
    }
    if sessions::is_standalone_claude_chat(&record) {
        return native_claude.0.archive(&session_id, &session_state, &app).map_err(to_err);
    }
    if sessions::is_standalone_kimi_chat(&record) {
        return native_kimi.0.archive(&session_id, &session_state, &app).map_err(to_err);
    }
    if sessions::is_standalone_codex_chat(&record) {
        return native_codex.0.archive(&session_id, &session_state, &app).map_err(to_err);
    }
    if sessions::is_standalone_opencode_chat(&record) {
        return native_opencode.0.archive(&session_id, &session_state, &app).map_err(to_err);
    }
    if sessions::is_standalone_pi_chat(&record) {
        native_pi.stop(&session_id, &session_state, &app).map_err(to_err)?;
        return session_state.archive(&session_id).map_err(to_err);
    }
    if record.pty_id.is_some() {
        session_state.mark_stopped(&session_id).map_err(to_err)?;
    }
    session_state.archive(&session_id).map_err(to_err)
    })()?;
    worktree_management::after_archive(&app, &session_state, &archived)?;
    Ok(archived)
}

#[tauri::command]
fn restore_archived_session(
    session_id: String,
    session_state: State<'_, SessionManager>,
) -> Result<SessionRecord, String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    session_state.restore(&session_id).map_err(to_err)
}

#[tauri::command(async)]
fn archive_project_sessions(
    project_id: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
    app: AppHandle,
) -> Result<usize, String> {
    let sessions = session_state
        .list()
        .into_iter()
        .filter(|session| session.project_id.as_deref() == Some(project_id.as_str()))
        .collect::<Vec<_>>();
    for session in &sessions {
        let lifecycle = session_state.lifecycle_lock(&session.id);
        let _lifecycle = lifecycle.lock().unwrap();
        detach_session_runtime(&session.id, &pty_state, &session_state, &action_queue)?;
        if sessions::is_standalone_zcode(session) { native_zcode.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_claude_chat(session) { native_claude.0.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_kimi_chat(session) { native_kimi.0.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_codex_chat(session) { native_codex.0.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_opencode_chat(session) { native_opencode.0.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_pi_chat(session) { native_pi.stop(&session.id,&session_state,&app).map_err(to_err)?; }
        if sessions::is_standalone_deepseek_harness(session) {
            native_acp.archive(&session.id, &session_state, &app).map_err(to_err)?;
        } else if session.pty_id.is_some() {
            session_state.mark_stopped(&session.id).map_err(to_err)?;
        }
    }
    let count = session_state.archive_project_sessions(&project_id).map_err(to_err)?;
    let mut retained = Vec::new();
    for record in sessions.iter().filter(|record| record.worktree.is_some()) {
        let lifecycle = session_state.lifecycle_lock(&record.id);
        let _guard = lifecycle.lock().unwrap();
        if let Some(archived) = session_state.get(&record.id).filter(|r| r.archived_at.is_some()) {
            if let Err(reason) = worktree_management::after_archive(&app, &session_state, &archived) {
                retained.push(format!("{}：{reason}", record.title));
            }
        }
    }
    if retained.is_empty() { Ok(count) } else { Err(retained.join("；")) }
}

fn detach_session_runtime(
    session_id: &str,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
) -> Result<SessionRecord, String> {
    let record = session_state
        .get(session_id)
        .ok_or_else(|| format!("未找到会话: {session_id}"))?;
    if let Some(pty_id) = record.pty_id {
        pty_state.kill(pty_id).map_err(to_err)?;
    }
    pty_state.kill_for_app_session(session_id).map_err(to_err)?;
    action_queue.discard_on_stop(session_id);
    Ok(record)
}

#[tauri::command(async)]
fn delete_session(
    app: AppHandle,
    session_id: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    usage_state: State<'_, UsageTracker>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
) -> Result<(), String> {
    delete_session_inner(&app, &session_id, &pty_state, &session_state, &action_queue, Some(&usage_state), &native_acp, &native_zcode, &native_claude, &native_kimi, &native_codex, &native_pi, &native_opencode)
}

fn delete_session_inner<R: tauri::Runtime>(
    app: &AppHandle<R>, session_id: &str, pty_state: &PtyManager, session_state: &SessionManager,
    action_queue: &SessionActionQueue, usage_state: Option<&UsageTracker>,
    native_acp: &NativeAcpSessions, native_zcode: &NativeZCodeSessions,
    native_claude: &NativeClaudeSessions, native_kimi: &NativeKimiSessions,
    native_codex: &NativeCodexChatSessions, native_pi: &NativePiSessions,
    native_opencode: &NativeOpenCodeChatSessions,
) -> Result<(), String> {
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _lifecycle = lifecycle.lock().unwrap();
    session_switch::protect_delete(&session_id, &session_state).map_err(to_err)?;
    if let Some(record) = session_state.get(session_id) {
        worktree_management::verify_process_owner(&record).map_err(to_err)?;
    }
    let record = detach_session_runtime(&session_id, &pty_state, &session_state, &action_queue)?;
    if sessions::is_standalone_deepseek_harness(&record) {
        native_acp.stop(&session_id, &session_state, &app).map_err(to_err)?;
    }
    if sessions::is_standalone_claude_chat(&record) { native_claude.0.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    if sessions::is_standalone_kimi_chat(&record) { native_kimi.0.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    if sessions::is_standalone_codex_chat(&record) { native_codex.0.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    if sessions::is_standalone_opencode_chat(&record) { native_opencode.0.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    if sessions::is_standalone_pi_chat(&record) { native_pi.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    if sessions::is_standalone_zcode(&record) { native_zcode.stop(&session_id,&session_state,&app).map_err(to_err)?; }
    // Worktree 有未提交改动时拒绝删除会话，避免静默丢掉 Agent 写入的代码。
    if let Some(worktree) = &record.worktree {
        let store = app.try_state::<MultiAgentStore>();
        if std::path::Path::new(&worktree.path).try_exists().map_err(to_err)? {
            worktree_management::clean_locked(session_state, &record, store.as_deref()).map_err(to_err)?;
        } else {
            session_state.with_worktree_records(|records| {
                worktree_management::unshared(records, session_id, worktree)?;
                worktree::forget_missing_checkout(worktree, session_id)
            }).map_err(to_err)?;
        }
    }
    if let Some(usage) = usage_state { usage.drop_session(session_id); }
    let deleted = if sessions::is_standalone_deepseek_harness(&record) {
        native_acp.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_claude_chat(&record) {
        native_claude.0.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_kimi_chat(&record) {
        native_kimi.0.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_codex_chat(&record) {
        native_codex.0.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_opencode_chat(&record) {
        native_opencode.0.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_zcode(&record) {
        native_zcode.delete_with(session_id, || session_state.delete(session_id))
    } else if sessions::is_standalone_pi_chat(&record) {
        native_pi.delete_with(session_id, || session_state.delete(session_id))
    } else {
        session_state.delete(session_id)
    };
    deleted.map_err(to_err)?;
    if action_queue.has_pending_work(session_id) { action_queue.discard(session_id); }
    action_queue.forget_human_message_receipts(session_id).map_err(to_err)
}

fn delete_session_batch(
    batch: Vec<SessionRecord>, mut delete: impl FnMut(&str) -> Result<(), String>,
) -> Result<usize, String> {
    let mut deleted = 0;
    let mut blocked = Vec::new();
    for record in batch {
        match delete(&record.id) {
            Ok(()) => deleted += 1,
            Err(error) => blocked.push(format!("「{}」：{error}", record.title)),
        }
    }
    if blocked.is_empty() { Ok(deleted) }
    else { Err(format!("已删除 {deleted} 个会话；以下会话因停止/保护检查未通过而保留：{}", blocked.join("；"))) }
}


#[tauri::command(async)]
fn delete_all_sessions(
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    trust_batch: State<'_, ControlWorkspaceTrustBatch>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
    app: AppHandle,
) -> Result<usize, String> {
    // Fixed batch: sessions attached/created after this snapshot belong to the
    // next user action. Each old record uses exactly the single-delete guard.
    let batch = session_state.list();
    let result = delete_session_batch(batch, |id| {
        delete_session_inner(&app, id, &pty_state, &session_state, &action_queue, None, &native_acp, &native_zcode, &native_claude, &native_kimi, &native_codex, &native_pi, &native_opencode)
    });
    trust_batch.replace(trust_batch.session_ids().into_iter().filter(|id| session_state.get(id).is_some()).collect());
    result
}

#[tauri::command]
async fn interpret_control_message(
    message: String,
    history: Vec<ControlHistoryMessage>,
) -> Result<ControlPlan, String> {
    let providers = providers::load().map_err(to_err)?.providers;
    tauri::async_runtime::spawn_blocking(move || {
        control::interpret_with_claude(&message, &history, &providers).map_err(to_err)
    })
    .await
    .map_err(to_err)?
}

fn current_control_workspace_trust_status(
    batch: &ControlWorkspaceTrustBatch,
    pty: &PtyManager,
) -> ControlWorkspaceTrustStatus {
    let session_ids = batch.session_ids();
    let mut status = ControlWorkspaceTrustStatus {
        batch_count: session_ids.len(),
        awaiting_count: 0,
        starting_count: 0,
        missing_count: 0,
    };
    for session_id in session_ids {
        match pty.workspace_trust_state(&session_id) {
            WorkspaceTrustState::Awaiting => status.awaiting_count += 1,
            WorkspaceTrustState::Starting => status.starting_count += 1,
            WorkspaceTrustState::Missing => status.missing_count += 1,
            WorkspaceTrustState::Resolved => {}
        }
    }
    status
}

#[tauri::command]
fn register_control_launch_batch(
    session_ids: Vec<String>,
    batch_state: State<'_, ControlWorkspaceTrustBatch>,
    pty_state: State<'_, PtyManager>,
) -> ControlWorkspaceTrustStatus {
    batch_state.replace(session_ids);
    current_control_workspace_trust_status(batch_state.inner(), pty_state.inner())
}

#[tauri::command]
fn control_workspace_trust_status(
    batch_state: State<'_, ControlWorkspaceTrustBatch>,
    pty_state: State<'_, PtyManager>,
) -> ControlWorkspaceTrustStatus {
    current_control_workspace_trust_status(batch_state.inner(), pty_state.inner())
}

#[tauri::command]
fn confirm_control_workspace_trust(
    batch_state: State<'_, ControlWorkspaceTrustBatch>,
    pty_state: State<'_, PtyManager>,
) -> ControlWorkspaceTrustReceipt {
    let session_ids = batch_state
        .session_ids()
        .into_iter()
        .collect::<HashSet<_>>();
    let confirmation = pty_state.confirm_workspace_trust(&session_ids);
    ControlWorkspaceTrustReceipt {
        confirmed_count: confirmation.confirmed_count,
        skipped_count: confirmation.skipped_count,
        failed_session_ids: confirmation.failed_session_ids,
    }
}

fn start_legacy_acp(manager: &AcpManager, admission: &runtime_admission::RuntimeAdmission, cwd: &str, session_id: Option<&str>, new_session: bool) -> Result<AcpStatus, String> {
    let _admission = admission.admit().map_err(to_err)?;
    if new_session { manager.new_session(cwd) } else { manager.start(cwd, session_id) }.map_err(to_err)
}

#[tauri::command]
async fn acp_start(
    cwd: String,
    session_id: Option<String>,
    state: State<'_, AcpManager>,
    admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<AcpStatus, String> {
    let manager = state.inner().clone();
    let admission = admission.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        start_legacy_acp(&manager, &admission, &cwd, session_id.as_deref(), false)
    })
    .await
    .map_err(to_err)?
}

#[tauri::command]
async fn acp_new_session(cwd: String, state: State<'_, AcpManager>, admission: State<'_, runtime_admission::RuntimeAdmission>) -> Result<AcpStatus, String> {
    let manager = state.inner().clone();
    let admission = admission.inner().clone();
    tauri::async_runtime::spawn_blocking(move || start_legacy_acp(&manager, &admission, &cwd, None, true))
        .await
        .map_err(to_err)?
}

#[tauri::command]
async fn acp_prompt(
    cwd: String,
    message: String,
    state: State<'_, AcpManager>,
) -> Result<AcpPromptResult, String> {
    let manager = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.prompt(&cwd, &message).map_err(to_err))
        .await
        .map_err(to_err)?
}

#[tauri::command]
fn acp_cancel(state: State<'_, AcpManager>) -> Result<bool, String> {
    state.cancel_prompt().map_err(to_err)
}

#[tauri::command]
fn broadcast_prompt(
    prompt: String,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<BroadcastReceipt, String> {
    session_state.ingest_events().map_err(to_err)?;
    let receipt = action_queue
        .enqueue_broadcast(&prompt, &session_state.list())
        .map_err(to_err)?;
    action_queue.flush_permission_modes(&session_state, &pty_state);
    action_queue.flush_ready(&session_state, &pty_state);
    Ok(receipt)
}

/// 人从总控面板 @ 某个会话发定向消息：逐字投递（from=None），
/// 并豁免该会话参与的 agent 互发限流（人工指挥视为已授权）。
fn ensure_session_message_target(target: &SessionRecord) -> Result<(), String> {
    if target.retired_agent_runtime { return Err("目标会话已因 Runtime 切换退役，请重新选择成员当前会话".into()); }
    if target.archived_at.is_some() { return Err("目标会话已归档，请恢复会话或选择当前会话".into()); }
    runtime_preferences::ensure_session_available(target).map_err(to_err)
}

#[tauri::command]
fn send_session_message(
    session_id: String,
    body: String,
    project_id: Option<String>,
    request_id: Option<String>,
    session_state: State<'_, SessionManager>,
    pty_state: State<'_, PtyManager>,
    action_queue: State<'_, SessionActionQueue>,
    multi_agent_store: State<'_, MultiAgentStore>,
) -> Result<String, String> {
    if body.trim().is_empty() {
        return Err("定向消息不能为空".to_string());
    }
    if body.chars().count() > 32_000 {
        return Err("定向消息不能超过 32000 个字符".to_string());
    }
    let request_id = request_id.map(|id| uuid::Uuid::parse_str(&id).map(|id| id.to_string()))
        .transpose().map_err(|_| "消息请求 ID 必须是 UUID".to_string())?;
    session_state.ingest_events().map_err(to_err)?;
    let lifecycle = session_state.lifecycle_lock(&session_id);
    let _guard = lifecycle.lock().unwrap();
    let target = session_state
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在或已删除: {session_id}"))?;
    if let Some(id) = request_id.as_deref() {
        if let Some(title) = action_queue.accepted_human_message(id, &session_id, project_id.as_deref(), &body).map_err(to_err)? {
            return Ok(title);
        }
    }
    ensure_session_message_target(&target)?;
    let mut queued = QueuedMessage::new(body.clone(), None);
    if let Some(id) = request_id.as_deref() { queued.id = id.to_string(); }
    if let Some(project_id) = project_id.as_deref() {
        if target.multi_agent_project_id.as_deref() != Some(project_id) {
            return Err("目标会话不属于当前协作项目，请重新选择".into());
        }
        let agent_id = target
            .multi_agent_agent_id
            .clone()
            .ok_or_else(|| "目标会话没有项目成员关联".to_string())?;
        multi_agent_store
            .send_collaboration_message_with_id(
                SendCollaborationMessageInput {
                    project_id: project_id.to_string(),
                    task_id: None,
                    sender_type: "user".into(),
                    sender_id: None,
                    target_agent_id: Some(agent_id),
                    body,
                },
                &queued.id,
            )
            .map_err(to_err)?;
    }
    action_queue.enqueue_human_message(&session_id, project_id.as_deref(), &target.title, queued, request_id.is_some()).map_err(to_err)?;
    action_queue.reset_agent_quota_involving(&session_id);
    action_queue.flush_permission_modes(&session_state, &pty_state);
    action_queue.flush_ready(&session_state, &pty_state);
    Ok(target.title)
}

/// 即时通道的成功投递记录：协作工作台「协作记录」只读投影的数据源。
/// 内存环形缓冲（重启清空），与 multi_agent 持久通道互不影响。
#[tauri::command]
fn list_delivery_records(state: State<'_, SessionActionQueue>) -> Vec<DeliveryRecord> {
    state.delivery_records()
}

/// 轻量运输证据查询，不探测 Skill 或运行时，也不启动任何 Agent。
#[tauri::command]
fn list_communication_evidence(state: State<'_, SessionActionQueue>) -> Vec<serde_json::Value> {
    state.communication_evidence()
}

/// 群聊视图活跃度开关：按会话 id 批量读取 AgentActivity（未设置过返回默认 Active）。
#[tauri::command]
fn list_agent_activity(
    session_ids: Vec<String>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<Vec<AgentActivityState>, String> {
    Ok(action_queue.agent_activity_states(&session_ids))
}

#[tauri::command]
fn set_agent_activity(
    session_id: String,
    activity: AgentActivity,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<(), String> {
    action_queue.set_activity(&session_id, activity);
    Ok(())
}

/// 执行一条到点的自动化动作，返回 (是否成功, 历史记录描述)。不 panic、不向上抛，
/// 失败全部转化为熔断计数——自动化是无人值守路径，错误必须留在历史里。
/// 时间型自动化出列执行。pty_poll_all 主循环会调用；另外由独立的
/// automation_tick 心跳驱动——没有终端会话时主轮询不跑，调度器不会停摆。
fn busy_automation_ids(automations: &AutomationManager, sessions: &SessionManager, queue: &SessionActionQueue, store: Option<&MultiAgentStore>) -> HashSet<String> {
    automations.list().into_iter().filter(|item| item.recent_runs.iter().any(|r| automation::is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed"))) || item.active_session_ids.iter().any(|id| {
        if queue.has_pending_work(id) { return true; }
        let Some(session) = sessions.get(id) else { return false; };
        if matches!(session.status, SessionStatus::Pending | SessionStatus::Waiting) || (session.status == SessionStatus::Running && !sessions.is_ready_for_prompt(id)) { return true; }
        if let (Some(project), Some(agent), Some(store)) = (&session.multi_agent_project_id, &session.multi_agent_agent_id, store) {
            return store.snapshot(project).map(|snapshot| snapshot.runs.iter().any(|run| run.agent_id == *agent && matches!(run.status, multi_agent::RunStatus::Queued | multi_agent::RunStatus::Starting | multi_agent::RunStatus::Running | multi_agent::RunStatus::WaitingInput | multi_agent::RunStatus::WaitingApproval)) || snapshot.tasks.iter().any(|task| task.assignee_agent_id.as_deref() == Some(agent.as_str()) && matches!(task.status, multi_agent::TaskStatus::Ready | multi_agent::TaskStatus::Claimed | multi_agent::TaskStatus::Running | multi_agent::TaskStatus::WaitingApproval | multi_agent::TaskStatus::InReview))).unwrap_or(true);
        }
        false
    })).map(|item| item.id).collect()
}

fn dispatch_due_automations(
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    automation_state: &AutomationManager,
    multi_agent_store: &MultiAgentStore,
) -> Result<(), String> {
    let now = automation::now_millis();
    automation_state.reconcile(action_queue, session_state, multi_agent_store, now).map_err(to_err)?;
    let mut report_error = None;
    let busy = busy_automation_ids(automation_state, session_state, action_queue, Some(multi_agent_store));
    for (automation_id, execution_id, action) in automation_state.collect_due_for_projects(now, &busy, Some(multi_agent_store)).map_err(to_err)? {
        let mut targets = Vec::new();
        let (ok, detail) = execute_automation_action_tracked(
            &action,
            pty_state,
            session_state,
            action_queue,
            Some(multi_agent_store),
            &mut targets,
            Some((automation_state, &execution_id)),
        );
        if let Err(error) = automation_state.finish_dispatch(&automation_id, &execution_id, ok, detail, now) {
            report_error.get_or_insert_with(|| to_err(error));
        }
    }
    if let Err(error) = dispatch_session_event_automations(
        pty_state, session_state, action_queue, automation_state, Some(multi_agent_store),
    ) {
        report_error.get_or_insert(error);
    }
    report_error.map_or(Ok(()), Err)
}

#[tauri::command]
async fn optimize_prompt(
    draft: String,
    context: Option<String>,
) -> Result<prompt_optimizer::PromptOptimization, String> {
    // CLI 首调可能拉起进程，放后台线程避免阻塞 IPC。
    tauri::async_runtime::spawn_blocking(move || {
        prompt_optimizer::optimize_prompt(&draft, context.as_deref())
    })
    .await
    .map_err(to_err)?
    .map_err(to_err)
}

#[tauri::command(async)]
fn automation_tick(
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    automation_state: State<'_, AutomationManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<(), String> {
    session_state.ingest_events().map_err(to_err)?;
    flush_cli_messages(
        &multi_agent_store,
        &session_state,
        &action_queue,
        &pty_state,
        &supervisor,
        &collaboration_bridge,
    )?;
    dispatch_due_automations(
        &pty_state,
        &session_state,
        &action_queue,
        &automation_state,
        &multi_agent_store,
    )?;
    action_queue.flush_ready(&session_state, &pty_state);
    action_queue.flush_pending_submits(&session_state, &pty_state);
    automation_state.reconcile(&action_queue, &session_state, &multi_agent_store, automation::now_millis()).map_err(to_err)?;
    Ok(())
}

/// 一轮已摄取 hook 并处理 PTY 退出后，按事件种类合并触发一次。
fn dispatch_session_event_automations(
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    automation_state: &AutomationManager,
    multi_agent_store: Option<&MultiAgentStore>,
) -> Result<(), String> {
    let mut pending: Vec<_> = session_state
        .drain_transitions()
        .into_iter()
        .filter(|transition| {
            matches!(
                transition.to,
                SessionStatus::Completed | SessionStatus::Error
            )
        })
        .collect();
    let now = automation::now_millis();
    let mut report_error = None;
    for (event, status) in [
        (SessionEventKind::Completed, SessionStatus::Completed),
        (SessionEventKind::Failed, SessionStatus::Error),
    ] {
        if !pending.iter().any(|transition| transition.to == status) {
            continue;
        }
        let sources = pending.iter().filter(|transition| transition.to == status).filter_map(|transition| session_state.get(&transition.session_id)).collect::<Vec<_>>();
        let busy = busy_automation_ids(automation_state, session_state, action_queue, multi_agent_store);
        let actions = match automation_state.collect_event_filtered(event, now, &sources, &busy) {
            Ok(actions) => actions,
            Err(error) => {
                session_state.restore_transitions(pending);
                return Err(to_err(error));
            }
        };
        pending.retain(|transition| transition.to != status);
        for (automation_id, execution_id, action) in actions {
            let mut targets = Vec::new();
            let (ok, detail) = execute_automation_action_tracked(
                &action,
                pty_state,
                session_state,
                action_queue,
                multi_agent_store,
                &mut targets,
                Some((automation_state, &execution_id)),
            );
            if let Err(error) = automation_state.finish_dispatch(&automation_id, &execution_id, ok, detail, now) {
                report_error.get_or_insert_with(|| to_err(error));
            }
        }
    }
    report_error.map_or(Ok(()), Err)
}

fn process_polled_session_exits(
    chunks: &mut std::collections::HashMap<u32, pty::PtyChunk>,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    automation_state: &AutomationManager,
    multi_agent_store: Option<&MultiAgentStore>,
) -> Result<(), String> {
    for (pty_id, chunk) in chunks {
        if let Some(code) = chunk.code {
            let lifecycle = session_state.lifecycle_lock(&chunk.app_session_id);
            let _guard = lifecycle.lock().unwrap();
            let Some(current) = session_state.get(&chunk.app_session_id) else { continue; };
            if current.pty_id != Some(*pty_id) { continue; }
            // Root exit is only an observation, not proof that descendants are
            // gone. Never clear a restored B binding using a stale A chunk.
            let stopped = pty_state.stop_for_exit(*pty_id).map_err(to_err)?;
            chunk.data.push_str(&stopped.data);
            if let Some(warning) = stopped.warning {
                eprintln!("[crewvela] {warning}");
                chunk.data.push_str(&format!("\r\n[输出警告] {warning}\r\n"));
            }
            if session_state.mark_exit_if_pty(&chunk.app_session_id, *pty_id, code).map_err(to_err)?.is_some() {
                action_queue.discard_on_stop(&chunk.app_session_id);
            }
        }
    }
    dispatch_session_event_automations(
        pty_state,
        session_state,
        action_queue,
        automation_state,
        multi_agent_store,
    )
}

#[cfg(test)]
fn execute_automation_action(action: &AutomationAction, pty: &PtyManager, sessions: &SessionManager, queue: &SessionActionQueue, store: Option<&MultiAgentStore>) -> (bool, String) {
    execute_automation_action_tracked(action, pty, sessions, queue, store, &mut Vec::new(), None)
}

fn execute_automation_action_tracked(
    action: &AutomationAction,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    multi_agent_store: Option<&MultiAgentStore>,
    targets: &mut Vec<String>,
    execution: Option<(&AutomationManager, &str)>,
) -> (bool, String) {
    let enqueue = |deliveries: Vec<(String, QueuedMessage)>| -> Result<(), String> {
        let mut tracked = Vec::new();
        for (session_id, mut message) in deliveries {
            if let Some((manager, run_id)) = execution {
                message.id = format!("automation-{run_id}-{}", message.id);
                let session = session_state.get(&session_id).ok_or("目标会话已删除")?;
                let project_id = session.multi_agent_project_id.clone().filter(|_| session.runtime_command_host);
                manager.register_target(run_id, automation::AutomationTarget {
                    session_id: session_id.clone(), message_id: message.id.clone(),
                    task_id: project_id.as_ref().map(|_| format!("cli-message-{}", message.id)),
                    project_id, run_id: None, stage: "queued".into(), detail: "等待投递".into(),
                }).map_err(to_err)?;
            }
            tracked.push((session_id, message));
        }
        action_queue.enqueue_messages_durable(tracked).map_err(to_err)
    };
    fn target_agent_session(
        project_id: &str,
        agent_id: &str,
        store: &MultiAgentStore,
        session_state: &SessionManager,
        pty_state: &PtyManager,
        expected_runtime: Option<AgentRuntimeKind>,
    ) -> Result<SessionRecord, String> {
        let snapshot = store.snapshot(project_id).map_err(to_err)?;
        let agent = snapshot
            .agents
            .iter()
            .find(|agent| agent.id == agent_id && agent.enabled)
            .ok_or_else(|| "目标 Agent 不存在、已停用或不属于该项目".to_string())?;
        agent.runtime_kind.ensure_available().map_err(to_err)?;
        if expected_runtime.is_some_and(|kind| kind != agent.runtime_kind) { return Err("项目成员的 Agent 类型已变化".into()); }
        if !automation_runtime::project_kind(agent.runtime_kind) { return Err("该 Agent 不支持项目自动化，请使用已打开的原生聊天会话".into()); }
        let session = session_state
            .list()
            .into_iter()
            .find(|session| {
                session.multi_agent_project_id.as_deref() == Some(project_id)
                    && session.multi_agent_agent_id.as_deref() == Some(agent_id)
                    && !session.retired_agent_runtime
                    && automation_runtime::session_runtime(session) == Some(agent.runtime_kind)
                    && session.archived_at.is_none()
            })
            .map(Ok)
            .unwrap_or_else(|| {
                session_state
                    .create_agent(sessions::NewAgentSession {
                        avatar_id: agent.avatar_id.clone(),
                        project_id: project_id.to_string(),
                        agent_id: agent.id.clone(),
                        runtime_kind: agent.runtime_kind.as_str().to_string(),
                        agent_name: agent.name.clone(),
                        cwd: snapshot.project.root_path.clone(),
                    })
                    .map_err(to_err)
            })?;
        store
            .link_agent_app_session(project_id, agent_id, &session.id)
            .map_err(to_err)?;
        if expected_runtime.is_some() {
            let lifecycle = session_state.lifecycle_lock(&session.id);
            let _guard = lifecycle.lock().unwrap();
            // Inactive project members use the supervisor's adapter, model and permission profile.
            // Never convert an already running interactive PTY into a command host.
            session_state.migrate_inactive_runtime_command_host(&session.id, |id| pty_state.is_running(id)).map_err(to_err)?;
            let current = session_state.get(&session.id).ok_or("目标会话已删除")?;
            if current.runtime_command_host { return Ok(current); }
        }
        if session.runtime_command_host || matches!(agent.runtime_kind, AgentRuntimeKind::DeepSeekHarness | AgentRuntimeKind::ZCode) {
            session_state.set_runtime_command_host(&session.id, true).map_err(to_err)?;
            return session_state.get(&session.id).ok_or_else(|| "目标会话已删除".to_string());
        }
        open_direct_collaboration_session(&session, pty_state, session_state, Some(store))
    }

    match action {
        AutomationAction::ConversationTemplate(template) => {
            let result = (|| -> anyhow::Result<()> {
                let (manager, run_id) = execution.ok_or_else(|| anyhow::anyhow!("缺少模板执行编号"))?;
                let store = multi_agent_store.ok_or_else(|| anyhow::anyhow!("协作项目服务不可用"))?;
                manager.dispatch_template(template, run_id, store,
                    multi_agent::AgentTerminalHost::new(pty_state.clone(), session_state.clone()))
            })();
            match result {
                Ok(()) => (true, "模板已提交到原话题，等待成员回复".into()),
                Err(reason) => (false, format!("{reason:#}")),
            }
        }
        AutomationAction::RunAgent { runtime_kind, provider_id, model, cwd, prompt } => {
            let result = (|| -> Result<SessionRecord, String> {
                automation_runtime::validate_launch(*runtime_kind, provider_id.as_deref(), model.as_deref(), cwd).map_err(to_err)?;
                runtime_kind.ensure_available().map_err(to_err)?;
                if !std::path::Path::new(cwd).is_dir() { return Err("工作目录不存在或不是目录".into()); }
                let record = session_state.create_runtime(NewRuntimeSession { runtime_kind: runtime_kind.as_str().into(), cwd: cwd.clone(), project_id: None }).map_err(to_err)?;
                targets.push(record.id.clone());
                let lifecycle = session_state.lifecycle_lock(&record.id);
                let _guard = lifecycle.lock().unwrap();
                match open_runtime_session(&record, pty_state, session_state) {
                    Ok(started) => {
                        enqueue(vec![(started.id.clone(), QueuedMessage::new(prompt.clone(), None))])
                            .map_err(|e| format!("会话已启动但任务排队失败，会话保留：{e:#}"))?;
                        Ok(started)
                    }
                    Err(error) => { let _ = session_state.mark_error(&record.id, error.clone()); Err(error) }
                }
            })();
            match result {
                Ok(started) => { if !targets.contains(&started.id) { targets.push(started.id.clone()); } (true, format!("已启动 {} 会话「{}」，任务已排队；不代表任务完成", runtime_kind.as_str(), started.title)) },
                Err(error) => (false, error),
            }
        }
        AutomationAction::SendAgent { runtime_kind, session_id, body, project_id, agent_id } => {
            let result = (|| -> Result<(), String> {
                runtime_kind.ensure_available().map_err(to_err)?;
                let target = match (project_id, agent_id) {
                    (Some(project), Some(agent)) => target_agent_session(project, agent, multi_agent_store.ok_or("缺少项目存储")?, session_state, pty_state, Some(*runtime_kind))?,
                    (None, None) => session_state.get(session_id).ok_or("目标会话不存在")?,
                    _ => return Err("项目和成员必须同时选择".into()),
                };
                let lifecycle = session_state.lifecycle_lock(&target.id);
                let _guard = lifecycle.lock().unwrap();
                let target = session_state.get(&target.id).ok_or("目标会话已删除")?;
                ensure_session_message_target(&target)?;
                if !automation_runtime::message_candidate(&target, Some(*runtime_kind)) { return Err("目标会话已归档或 Agent 类型变化".into()); }
                if project_id.is_none() && !automation_runtime::live_candidate(&target, *runtime_kind) { return Err("目标会话已停止或异常，请先恢复会话".into()); }
                targets.push(target.id.clone());
                enqueue(vec![(target.id, QueuedMessage::new(body.clone(), None))])
            })();
            match result { Ok(()) => (true, "任务已排队；不代表已投递或任务完成".into()), Err(error) => (false, error) }
        }
        AutomationAction::BroadcastAgent { runtime_kind, body } => {
            if let Err(error) = runtime_kind.ensure_available() { return (false, to_err(error)); }
            let rows = session_state.list().into_iter().filter(|s| automation_runtime::live_candidate(s, *runtime_kind)).collect::<Vec<_>>();
            if rows.is_empty() { return (false, "所选 Agent 没有运行中的会话".into()); }
            let ids = rows.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
            let deliveries = ids.iter().map(|id| (id.clone(), QueuedMessage::new(body.clone(), None))).collect();
            match enqueue(deliveries) {
                Ok(()) => { *targets = ids; (true, format!("已向 {} 个 {} 会话排队；未确认任务完成", targets.len(), runtime_kind.as_str())) },
                Err(error) => (false, to_err(error)),
            }
        }
        AutomationAction::Launch {
            provider_id: _,
            cwd: _,
            prompt,
            project_id: Some(project_id),
            agent_id: Some(agent_id),
        } => {
            let Some(store) = multi_agent_store else {
                return (false, "自动化执行缺少多智能体存储".into());
            };
            match target_agent_session(project_id, agent_id, store, session_state, pty_state, None) {
                Ok(session) => {
                    targets.push(session.id.clone());
                    if let Err(error) = enqueue(vec![(session.id.clone(), QueuedMessage::new(prompt.clone(), None))]) { return (false, to_err(error)); }
                    (
                        true,
                        format!("已启动项目 Agent「{}」并排队 Prompt", session.title),
                    )
                }
                Err(error) => (false, error),
            }
        }
        AutomationAction::Launch {
            provider_id,
            cwd,
            prompt,
            ..
        } => {
            let result = (|| -> Result<SessionRecord, String> {
                let provider = providers::get(provider_id)
                    .map_err(to_err)?
                    .ok_or_else(|| format!("未找到 provider: {provider_id}"))?;
                let key = String::new();
                let cwd = cwd
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| dirs::home_dir().map(|path| path.display().to_string()))
                    .unwrap_or_else(|| ".".to_string());
                let started = launch_one_session(
                    &provider,
                    &key,
                    cwd,
                    false,
                    false,
                    false,
                    None,
                    None,
                    pty_state,
                    session_state,
                    action_queue,
                )?;
                if let Err(error) = enqueue(vec![(started.id.clone(), QueuedMessage::new(prompt.clone(), None))]) { targets.push(started.id.clone()); return Err(format!("会话已启动但任务排队失败：{error:#}")); }
                Ok(started)
            })();
            match result {
                Ok(started) => { targets.push(started.id.clone()); (true, format!("已启动会话「{}」，Prompt 已排队；不代表任务完成", started.title)) },
                Err(error) => (false, error),
            }
        }
        AutomationAction::Message {
            project_id: Some(project_id),
            agent_id: Some(agent_id),
            body,
            ..
        } => {
            let Some(store) = multi_agent_store else {
                return (false, "自动化执行缺少多智能体存储".into());
            };
            match target_agent_session(project_id, agent_id, store, session_state, pty_state, None) {
                Ok(session) => {
                    targets.push(session.id.clone());
                    match enqueue(vec![(session.id.clone(), QueuedMessage::new(body.clone(), None))]) {
                        Ok(()) => (true, format!("已排队发给 Agent「{}」", session.title)),
                        Err(error) => (false, format!("{error:#}")),
                    }
                }
                Err(error) => (false, error),
            }
        }
        AutomationAction::Message {
            session_id, body, ..
        } => {
            let lifecycle = session_state.lifecycle_lock(session_id);
            let _guard = lifecycle.lock().unwrap();
            match session_state.get(session_id) {
            Some(target) => {
                targets.push(target.id.clone());
                if let Err(error) = ensure_session_message_target(&target) {
                    return (false, error);
                }
                match enqueue(vec![(session_id.clone(), QueuedMessage::new(body.clone(), None))]) {
                    Ok(()) => (true, format!("已排队发给「{}」", target.title)),
                    Err(error) => (false, format!("{error:#}")),
                }
            }
            None => (
                false,
                format!("目标会话不存在: {}", session_id.chars().take(8).collect::<String>()),
            ),
            }
        },
        AutomationAction::Broadcast { body } => {
            let ids = session_state.list().iter().filter(|s| s.kind == SessionKind::Claude && s.pty_id.is_some() && runtime_preferences::ensure_session_available(s).is_ok()).map(|s| s.id.clone()).collect::<Vec<_>>();
            if ids.is_empty() { return (false, "没有运行中的 Claude Code 会话".into()); }
            match enqueue(ids.iter().map(|id| (id.clone(), QueuedMessage::with_kind(body.clone(), None, control::MessageKind::Broadcast))).collect()) {
                Ok(()) => { *targets = ids; (true, format!("已排队广播给 {} 个 Claude Code 会话；不代表任务完成", targets.len())) },
                Err(error) => (false, error),
            }
        }
    }
}

#[tauri::command(async)]
fn run_automation_now(id: String, request_id: String, retry_of: Option<String>,
    pty: State<'_, PtyManager>, sessions: State<'_, SessionManager>, queue: State<'_, SessionActionQueue>,
    state: State<'_, AutomationManager>, store: State<'_, MultiAgentStore>, admission: State<'_, runtime_admission::RuntimeAdmission>,
) -> Result<Vec<Automation>, String> {
    let _admission = admission.admit().map_err(to_err)?;
    state.reconcile(&queue, &sessions, &store, automation::now_millis()).map_err(to_err)?;
    if let Some((run_id, action)) = state.begin_manual(&id, &request_id, retry_of.as_deref(), automation::now_millis()).map_err(to_err)? {
        let mut targets = Vec::new();
        let (ok, detail) = execute_automation_action_tracked(&action, &pty, &sessions, &queue, Some(&store), &mut targets, Some((&state, &run_id)));
        state.finish_dispatch(&id, &run_id, ok, detail, automation::now_millis()).map_err(to_err)?;
    }
    Ok(state.list())
}

#[tauri::command(async)]
fn stop_automation_run(id: String, run_id: String, state: State<'_, AutomationManager>, queue: State<'_, SessionActionQueue>, store: State<'_, MultiAgentStore>, supervisor: State<'_, MultiAgentSupervisor>) -> Result<Vec<Automation>, String> {
    if state.stop_template(&id, &run_id, &store).map_err(to_err)? { return Ok(state.list()); }
    if state.cancel_queued_run(&id, &run_id, &queue).is_err() {
        let run = state.list().into_iter().find(|a| a.id == id).and_then(|a| a.recent_runs.into_iter().find(|r| r.id == run_id)).ok_or("执行不存在")?;
        if run.targets.is_empty() || !automation::is_active_stage(run.stage.as_deref().unwrap_or("unconfirmed")) { return Err("本轮没有可停止的活动任务".into()); }
        if store.cancel_queued_automation_tasks(&run.targets).is_ok() {
            state.mark_cancelled(&id,&run_id).map_err(to_err)?;
        } else {
            supervisor.stop_automation_runs(&store, &run.targets).map_err(to_err)?;
            state.mark_stopping(&id, &run_id).map_err(to_err)?;
        }
    }
    Ok(state.list())
}

#[tauri::command]
fn list_automations(state: State<'_, AutomationManager>) -> Vec<Automation> {
    state.list()
}

#[tauri::command(async)]
fn acknowledge_automation_run(id:String,run_id:String,confirmed:bool,state:State<'_,AutomationManager>,queue:State<'_,SessionActionQueue>,store:State<'_,MultiAgentStore>) -> Result<Vec<Automation>,String> {
    if !confirmed {return Err("请先检查原会话，确认原任务不再执行".into());}
    state.acknowledge_unknown(&id,&run_id,&queue,&store).map_err(to_err)?;
    Ok(state.list())
}

#[tauri::command]
fn create_automation(
    input: NewAutomation,
    session_state: State<'_, SessionManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
    automation_state: State<'_, AutomationManager>,
) -> Result<Automation, String> {
    create_automation_inner(input, &session_state, &multi_agent_store, &automation_state)
}

fn create_automation_inner(input: NewAutomation, session_state: &SessionManager, multi_agent_store: &MultiAgentStore, automation_state: &AutomationManager) -> Result<Automation, String> {
    save_automation_inner(None, input, session_state, multi_agent_store, automation_state)
}

#[tauri::command]
fn update_automation(
    id: String,
    input: NewAutomation,
    session_state: State<'_, SessionManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
    automation_state: State<'_, AutomationManager>,
) -> Result<Automation, String> {
    save_automation_inner(Some(&id), input, &session_state, &multi_agent_store, &automation_state)
}

fn save_automation_inner(id: Option<&str>, input: NewAutomation, session_state: &SessionManager, multi_agent_store: &MultiAgentStore, automation_state: &AutomationManager) -> Result<Automation, String> {
    let direct_target_lifecycle = match &input.action {
        AutomationAction::Message { session_id, project_id: None, agent_id: None, .. } | AutomationAction::SendAgent { session_id, project_id: None, agent_id: None, .. } => Some(session_state.lifecycle_lock(session_id)),
        _ => None,
    };
    let _direct_target_guard = direct_target_lifecycle.as_ref().map(|lock| lock.lock().unwrap());
    if let automation::AutomationTrigger::OnSessionEventFiltered { runtime_kind, project_id, session_id, .. } = &input.trigger {
        if let Some(kind) = runtime_kind { kind.ensure_available().map_err(to_err)?; }
        if let Some(id) = project_id { multi_agent_store.snapshot(id).map_err(to_err)?; }
        if let Some(id) = session_id { let source = session_state.get(id).ok_or("事件来源会话不存在")?; ensure_session_message_target(&source)?; }
    }
    // 模块外校验：provider / 目标会话真实存在（模块内只校验自洽性）。
    match &input.action {
        AutomationAction::ConversationTemplate(template) => template.validate(multi_agent_store).map_err(to_err)?,
        AutomationAction::RunAgent { runtime_kind, provider_id, model, cwd, .. } => {
            automation_runtime::validate_launch(*runtime_kind, provider_id.as_deref(), model.as_deref(), cwd).map_err(to_err)?;
            runtime_kind.ensure_available().map_err(to_err)?;
            if !std::path::Path::new(cwd).is_dir() { return Err("工作目录不存在或不是目录".into()); }
            if let Some(id) = provider_id { providers::get(id).map_err(to_err)?.ok_or_else(|| "Provider 不存在".to_string())?; }
        }
        AutomationAction::SendAgent { runtime_kind, session_id, project_id, agent_id, .. } => {
            runtime_kind.ensure_available().map_err(to_err)?;
            if let (Some(project_id), Some(agent_id)) = (project_id, agent_id) {
                if !automation_runtime::project_kind(*runtime_kind) { return Err("该 Agent 不支持项目自动化，请使用已有会话".into()); }
                let snapshot = multi_agent_store.snapshot(project_id).map_err(to_err)?;
                if !snapshot.agents.iter().any(|a| a.id == *agent_id && a.enabled && a.runtime_kind == *runtime_kind) { return Err("项目成员已停用或 Agent 类型不符".into()); }
            } else {
                let target = session_state.get(session_id).ok_or_else(|| "目标会话不存在".to_string())?;
                ensure_session_message_target(&target)?;
                if !automation_runtime::message_candidate(&target, Some(*runtime_kind)) { return Err("目标 Agent 类型不符".into()); }
            }
        }
        AutomationAction::BroadcastAgent { runtime_kind, .. } => { runtime_kind.ensure_available().map_err(to_err)?; }

        AutomationAction::Launch {
            provider_id,
            project_id,
            agent_id,
            ..
        } => match (project_id, agent_id) {
            (Some(project_id), Some(agent_id)) => {
                let snapshot = multi_agent_store.snapshot(project_id).map_err(to_err)?;
                let agent = snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.id == *agent_id && agent.enabled)
                    .ok_or_else(|| "目标 Agent 不存在、已停用或不属于该项目".to_string())?;
                agent.runtime_kind.ensure_available().map_err(to_err)?;
            }
            (None, None) => {
                providers::get(provider_id)
                    .map_err(to_err)?
                    .ok_or_else(|| format!("未找到 provider: {provider_id}"))?;
            }
            _ => return Err("项目和 Agent 必须同时选择".into()),
        },
        AutomationAction::Message {
            session_id,
            project_id,
            agent_id,
            ..
        } => match (project_id, agent_id) {
            (Some(project_id), Some(agent_id)) => {
                let snapshot = multi_agent_store.snapshot(project_id).map_err(to_err)?;
                if !snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.id == *agent_id && agent.enabled)
                {
                    return Err("目标 Agent 不存在、已停用或不属于该项目".into());
                }
            }
            (None, None) => {
                let target = session_state.get(session_id).ok_or_else(|| format!("目标会话不存在: {session_id}"))?;
                ensure_session_message_target(&target)?;
            }
            _ => return Err("项目和 Agent 必须同时选择".into()),
        },
        AutomationAction::Broadcast { .. } => {}
    }
    let now = automation::now_millis();
    match id {
        Some(id) => automation_state.update(id, input, now),
        None => automation_state.create(input, now),
    }.map_err(to_err)
 }

#[tauri::command]
fn set_automation_enabled(
    id: String,
    enabled: bool,
    state: State<'_, AutomationManager>,
) -> Result<Automation, String> {
    state
        .set_enabled(&id, enabled, automation::now_millis())
        .map_err(to_err)
}

#[tauri::command]
fn delete_automation(id: String, state: State<'_, AutomationManager>) -> Result<(), String> {
    state.delete(&id).map_err(to_err)
}

/// cron 高级模式的实时预览：返回接下来 3 个触发点（毫秒），非法表达式报错。
#[tauri::command]
fn preview_cron_trigger(expr: String) -> Result<Vec<u64>, String> {
    automation::cron_next_runs(&expr, automation::now_millis(), 3).map_err(to_err)
}

#[tauri::command]
fn set_all_permission_modes(
    permission_mode: PermissionMode,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<PermissionModeReceipt, String> {
    session_state.ingest_events().map_err(to_err)?;
    let mut receipt = action_queue
        .enqueue_permission_mode(permission_mode, &session_state.list())
        .map_err(to_err)?;
    let dispatch = action_queue.flush_permission_modes(&session_state, &pty_state);
    receipt.already_count = dispatch.already_count;
    receipt.sent_count = dispatch.sent_count;
    receipt.awaiting_confirmation_count = dispatch.awaiting_confirmation_count;
    receipt.queued_count = receipt.target_count.saturating_sub(
        receipt.already_count + receipt.sent_count + receipt.awaiting_confirmation_count,
    );
    Ok(receipt)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PollResult {
    chunks: std::collections::HashMap<u32, pty::PtyChunk>,
    sessions: Vec<SessionRecord>,
    working_cli_pty_ids: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    communication_deliveries: Vec<control::CommunicationDelivery>,
}

fn mark_interactive_runtime_prompts(
    chunks: &std::collections::HashMap<u32, pty::PtyChunk>,
    session_state: &SessionManager,
) {
    for (pty_id, chunk) in chunks.iter().filter(|(_, chunk)| chunk.code.is_none()) {
        let lifecycle = session_state.lifecycle_lock(&chunk.app_session_id);
        let _guard = lifecycle.lock().unwrap();
        let Some(session) = session_state.get(&chunk.app_session_id) else {
            continue;
        };
        if session.pty_id != Some(*pty_id) { continue; }
        if session.runtime_command_host {
            continue;
        }
        if let Ok(kind) = standalone_runtime_kind(&session) {
            // Lifecycle hooks/bridges own readiness for these runtimes; redraws must not revoke it.
            if matches!(
                kind,
                AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Pi
            ) {
                continue;
            }
            session_state.observe_runtime_prompt_output_with_blocker(
                &chunk.app_session_id,
                &chunk.data,
                |output| multi_agent::interactive_runtime_prompt_ready(kind, output),
                multi_agent::interactive_runtime_prompt_blocked,
            );
        }
    }
}

fn open_runtime_session(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
) -> Result<SessionRecord, String> {
    open_runtime_session_with_yolo(record, pty_state, session_state, false)
}

fn open_direct_collaboration_session(
    record: &SessionRecord,
    pty_state: &PtyManager,
    session_state: &SessionManager,
    store: Option<&MultiAgentStore>,
) -> Result<SessionRecord, String> {
    // Auto-wakeup must not cross a stop/transfer/restore with an obsolete snapshot.
    let lifecycle = session_state.lifecycle_lock(&record.id);
    let _lifecycle = lifecycle.lock().unwrap();
    let record = session_state
        .get(&record.id)
        .ok_or_else(|| "会话已删除".to_string())?;
    if record.retired_agent_runtime {
        return Err("该会话为 Runtime 切换前的只读历史，请选择成员当前会话".into());
    }
    if matches!(record.agent_runtime_kind.as_deref(), Some("oh_my_pi" | "minimax_code")) {
        return Err("该 CLI 已移除，历史会话不能启动或恢复".to_string());
    }
    if record.kind == SessionKind::Claude && record.agent_runtime_kind.is_none() {
        let provider = providers::get(&record.provider_id)
            .map_err(to_err)?
            .ok_or_else(|| format!("未找到 provider: {}", record.provider_id))?;
        let key = String::new();
        return open_session(
            &record,
            &provider,
            &key,
            false,
            pty_state,
            session_state,
        );
    }
    open_runtime_session_checked(&record, pty_state, session_state, record.yolo, None, store)
}

fn persist_cli_collaboration_message_with_id(
    store: &MultiAgentStore,
    session_state: &SessionManager,
    message: &OutboxMessage,
    message_id: &str,
    published_at: i64,
) -> anyhow::Result<()> {
    if session_state
        .get(&message.to)
        .is_some_and(|target| target.retired_agent_runtime)
    {
        return Err(anyhow::anyhow!(
            "目标会话已因 Runtime 切换退役，消息未投递；请发送到成员当前会话"
        ));
    }
    let Some(origin) = message.from.as_ref() else {
        return Ok(());
    };
    let Some(sender) = session_state.get(&origin.session_id) else {
        return Ok(());
    };
    let Some(target) = session_state.get(&message.to) else {
        return Ok(());
    };
    if store.record_bound_session_message(&sender, &target, message_id, &message.body, published_at)? {
        return Ok(());
    }
    let (Some(project_id), Some(sender_agent_id), Some(target_project_id), Some(target_agent_id)) = (
        sender.multi_agent_project_id.as_deref(),
        sender.multi_agent_agent_id.as_deref(),
        target.multi_agent_project_id.as_deref(),
        target.multi_agent_agent_id.as_deref(),
    ) else {
        return Ok(());
    };
    if project_id != target_project_id {
        return Ok(());
    }
    store.send_collaboration_message_with_id(
        SendCollaborationMessageInput {
            project_id: project_id.to_string(),
            task_id: None,
            sender_type: "agent".to_string(),
            sender_id: Some(sender_agent_id.to_string()),
            target_agent_id: Some(target_agent_id.to_string()),
            body: message.body.clone(),
        },
        message_id,
    )?;
    Ok(())
}

fn flush_cli_messages(
    multi_agent_store: &MultiAgentStore,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    pty_state: &PtyManager,
    supervisor: &MultiAgentSupervisor,
    collaboration_bridge: &CollaborationBridge,
) -> Result<(), String> {
    flush_cli_messages_with_scheduler(
        multi_agent_store,
        session_state,
        action_queue,
        pty_state,
        |project_id| {
            supervisor
                .start_message_delivery(
                    multi_agent_store.clone(),
                    project_id.to_string(),
                    multi_agent::AgentTerminalHost::new(pty_state.clone(), session_state.clone()),
                    collaboration_bridge.clone(),
                )
                .map_err(to_err)
        },
    )
}

fn flush_cli_messages_with_scheduler(
    multi_agent_store: &MultiAgentStore,
    session_state: &SessionManager,
    action_queue: &SessionActionQueue,
    pty_state: &PtyManager,
    mut start: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    // 旧版成员终端只在其进程已经退出后迁移；不替换正在运行的 TUI。
    for target in session_state.list() {
        if !target.runtime_command_host
            && target.multi_agent_agent_id.is_some()
            && matches!(
                target.agent_runtime_kind.as_deref(),
                Some(
                    "claude_code"
                        | "codex"
                        | "kimi_code"
                            | "pi"
                        | "opencode"
                        | "deepseek_harness"
                        | "zcode"
                        | "zcode_cli"
                )
            )
            && !target
                .pty_id
                .is_some_and(|pty_id| pty_state.is_running(pty_id))
        {
            migrate_inactive_member_session(session_state, pty_state, &target.id)?;
        }
    }
    // 摄取 CLI `send` 写入的会话间消息，再随本轮 flush 投递。
    action_queue
        .ingest_outbox_with_timestamp(&session_state, |message, message_id, published_at| {
            persist_cli_collaboration_message_with_id(
                &multi_agent_store,
                &session_state,
                message,
                message_id,
                published_at,
            )
        })
        .map_err(to_err)?;
    for target in session_state
        .list()
        .into_iter()
        .filter(|session| session.runtime_command_host)
    {
        let _runtime_lease = match runtime_preferences::session_runtime(&target).map_err(to_err)? {
            Some(kind) => match runtime_preferences::acquire_if_available(kind).map_err(to_err)? {
                Some(lease) => Some(lease),
                None => continue,
            },
            None => None,
        };
        let (Some(project_id), Some(agent_id)) = (
            target.multi_agent_project_id.as_deref(),
            target.multi_agent_agent_id.as_deref(),
        ) else {
            continue;
        };
        action_queue.with_delivery_lock(|| -> Result<(), String> {
        let Some(message) = action_queue.peek_deliverable(&target.id) else {
            return Ok(());
        };
        if message.kind == control::MessageKind::Chat
            && message.from.is_some()
            && action_queue.activity(&target.id) == control::AgentActivity::Muted
        {
            return Ok(());
        }
        if multi_agent_store
            .enqueue_cli_message_task(
                project_id,
                agent_id,
                &message.id,
                &control::render_scheduled_delivery(&message),
            )
            .map_err(to_err)?
        {
            action_queue.commit_delivered_message(&target.id, &message.id);
            start(project_id)?;
        }
            Ok(())
        })?;
    }
    // 已入任务表但当前 Agent 忙碌/调度器正在退出时，下轮仍会重试唤醒。
    let mut projects = session_state
        .list()
        .into_iter()
        .filter(|session| session.runtime_command_host)
        .filter_map(|session| session.multi_agent_project_id)
        .collect::<HashSet<_>>();
    projects.extend(multi_agent_store.drain_member_inbox(action_queue).map_err(to_err)?);
    for project_id in projects {
        if multi_agent_store
            .runnable_tasks(&project_id)
            .map_err(to_err)?
            .iter()
            .any(|(task, _)| task.id.starts_with("cli-message-"))
        {
            start(&project_id)?;
        }
    }
    Ok(())
}

#[tauri::command(async)]
fn pty_poll_all(
    app: AppHandle,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    automation_state: State<'_, AutomationManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
    approvals_state: State<'_, ApprovalAutoResponder>,
    supervisor: State<'_, MultiAgentSupervisor>,
    collaboration_bridge: State<'_, CollaborationBridge>,
) -> Result<PollResult, String> {
    session_state.ingest_events().map_err(to_err)?;
    flush_title_summaries(&app, &session_state);
    // Allow/Deny 审批策略的等待会话直接代答；Ask 的出卡片等人工（list_approval_cards）。
    approvals_state.flush(&pty_state, &session_state, &action_queue);
    flush_cli_messages(
        &multi_agent_store,
        &session_state,
        &action_queue,
        &pty_state,
        &supervisor,
        &collaboration_bridge,
    )?;
    // 到点自动化任务出列执行；消息/广播类动作随本轮 flush 一并投递。
    dispatch_due_automations(
        &pty_state,
        &session_state,
        &action_queue,
        &automation_state,
        &multi_agent_store,
    )?;
    let mut chunks = pty_state.poll_all();
    mark_interactive_runtime_prompts(&chunks, &session_state);
    process_polled_session_exits(
        &mut chunks,
        &pty_state,
        &session_state,
        &action_queue,
        &automation_state,
        Some(&multi_agent_store),
    )?;
    action_queue.flush_permission_modes(&session_state, &pty_state);
    action_queue.flush_ready(&session_state, &pty_state);
    // 文本已投递的会话补发提交回车（延迟到达的在本轮生效）。
    action_queue.flush_pending_submits(&session_state, &pty_state);
    let sessions = session_state.list();
    let working_cli_pty_ids = session_state.working_cli_pty_ids(&sessions);
    let communication_deliveries = action_queue.take_communication_deliveries();
    Ok(PollResult { chunks, sessions, working_cli_pty_ids, communication_deliveries })
}

#[tauri::command]
fn pty_input(
    session_id: u32,
    bytes: Vec<u8>,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<(), String> {
    // PermissionRequest 的 hook 文件一定先于终端中的确认操作产生。先摄取它，
    // 避免极快确认时出现“先恢复 running，下一轮又被旧 waiting 事件覆盖”的竞态。
    session_state.ingest_events().map_err(to_err)?;
    let record = session_state
        .list()
        .into_iter()
        .find(|record| record.pty_id == Some(session_id));
    if let Some(record) = record.as_ref() {
        runtime_preferences::ensure_session_available(&record).map_err(to_err)?;
    }
    pty_state.write(session_id, &bytes).map_err(to_err)?;
    if terminal_input_is_submit(&bytes) {
        if let Some(record) = record.as_ref() {
            if record.status == SessionStatus::Waiting || session_state.runtime_prompt_is_blocked(&record.id) {
                action_queue.hold_pending_submit_for_blocker(&record.id);
            } else {
                action_queue.cancel_pending_submit(&record.id);
            }
        }
        session_state
            .mark_terminal_submission(session_id)
            .map_err(to_err)?;
    }
    Ok(())
}

fn terminal_input_is_submit(bytes: &[u8]) -> bool {
    // Enter is a separate xterm input event. Newlines inside pasted text (including
    // bracketed paste) must not confirm an approval or acknowledge queued delivery.
    matches!(bytes, b"\r" | b"\n" | b"\r\n")
}

#[tauri::command]
fn pty_resize(
    session_id: u32,
    rows: u16,
    cols: u16,
    state: State<'_, PtyManager>,
    sessions: State<'_, SessionManager>,
) -> Result<(), String> {
    state.resize(session_id, rows, cols).map_err(to_err)?;
    if let Some(session) = sessions.list().into_iter().find(|session| session.pty_id == Some(session_id)) {
        sessions.resize_runtime_prompt(&session.id, rows, cols);
    }
    Ok(())
}

#[tauri::command]
fn list_approval_cards(
    session_state: State<'_, SessionManager>,
) -> Result<Vec<ApprovalCard>, String> {
    Ok(collect_approval_cards(&session_state))
}

fn respond_session_approval_inner(
    session_id: &str, action: ApprovalDecision, expected_pty_id: u32, expected_waiting_since: u64,
    ptys: &PtyManager, sessions: &SessionManager,
    queue: &SessionActionQueue,
) -> Result<SessionRecord, String> {
    let lifecycle = sessions.lifecycle_lock(session_id);
    let _guard = lifecycle.lock().unwrap();
    sessions.ingest_events().map_err(to_err)?;
    // Acquire queue guards before the session approval gate, matching handoff.
    queue.hold_pending_submit_for_blocker(session_id);
    sessions.respond_waiting_approval(session_id, expected_pty_id, expected_waiting_since, action == ApprovalDecision::Allow, |pty| {
        ptys.write_for_app_session_bounded(pty, session_id, action.keys(), std::time::Duration::from_secs(2))
    }).map_err(to_err)
}

#[tauri::command]
fn respond_session_approval(
    session_id: String,
    action: ApprovalDecision,
    expected_pty_id: u32,
    expected_waiting_since: u64,
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
) -> Result<SessionRecord, String> {
    respond_session_approval_inner(&session_id, action, expected_pty_id, expected_waiting_since, &pty_state, &session_state, &action_queue)
}

#[tauri::command]
fn set_session_approval_policy(
    session_id: String,
    policy: ApprovalPolicy,
    session_state: State<'_, SessionManager>,
) -> Result<SessionRecord, String> {
    session_state
        .set_approval_policy(&session_id, policy)
        .map_err(to_err)
}

#[tauri::command]
fn pty_kill_all(
    pty_state: State<'_, PtyManager>,
    session_state: State<'_, SessionManager>,
    action_queue: State<'_, SessionActionQueue>,
    native_acp: State<'_, NativeAcpSessions>,
    native_zcode: State<'_, NativeZCodeSessions>,
    native_claude: State<'_, NativeClaudeSessions>,
    native_kimi: State<'_, NativeKimiSessions>,
    native_codex: State<'_, NativeCodexChatSessions>,
    native_pi: State<'_, native_pi_sessions::NativePiSessions>,
    native_opencode: State<'_, NativeOpenCodeChatSessions>,
    app: AppHandle,
) -> Result<(), String> {
    pty_state.kill_all().map_err(to_err)?;
    native_acp.stop_all(&session_state, &app).map_err(to_err)?;
    native_zcode.stop_all(&session_state, &app).map_err(to_err)?;
    native_claude.0.stop_all(&session_state, &app).map_err(to_err)?;
    native_kimi.0.stop_all(&session_state, &app).map_err(to_err)?;
    native_codex.0.stop_all(&session_state, &app).map_err(to_err)?;
    native_pi.stop_all(&session_state, &app).map_err(to_err)?;
    native_opencode.0.stop_all(&session_state, &app).map_err(to_err)?;
    action_queue.clear();
    session_state.mark_all_stopped().map_err(to_err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Finder does not inherit the user's terminal PATH. Initialize before any
    // runtime discovery or application worker starts; Windows is a no-op.
    if !test_profile::active() { launcher::initialize_platform_path(); }
    let storage_migration = brand::prepare_app_storage_aliases();
    let mut context=tauri::generate_context!();
    let isolated_windows=test_profile::configure_context(&mut context);
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            storage_migration.map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let config_dir = providers::config_dir()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            match app_instance::AppInstanceGuard::acquire(&config_dir)? {
                Some(guard) => { app.manage(guard); }
                None => {
                    use tauri_plugin_dialog::DialogExt;
                    if let Some(window) = app.get_webview_window("main") { window.hide()?; }
                    app.dialog().message("CrewVela 已在运行，请回到已打开的窗口。重复启动会干扰会话就绪状态和消息投递。升级时请先从旧窗口或托盘退出旧版，再打开新版。")
                        .title("CrewVela 已在运行").show(|_| std::process::exit(0));
                    // No sessions, workers or agents have been opened in this process.
                    return Ok(());
                }
            }
            #[cfg(target_os = "windows")]
            if !test_profile::active() { if let Err(error) = windows_notification::initialize() {
                eprintln!("[crewvela] 初始化 Windows 通知身份失败：{error:#}");
            } }
            if let Err(error) = mcp::retire_removed_integrations_at(&config_dir) {
                eprintln!("[crewvela] 清理已移除集成的旧配置失败：{error:#}");
            }
            runtime_preferences::get()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let runtime_admission = runtime_admission::RuntimeAdmission::default();
            app.manage(PtyManager::with_admission(runtime_admission.clone()));
            app.manage(runtime_admission);
            app.manage(ApprovalAutoResponder::default());
            if !test_profile::active() { browser_control::start_panel_request_service(app.handle(), &config_dir)?; }
            app.manage(
                SessionActionQueue::open_at(
                    config_dir.join("mailbox.json"),
                    config_dir.join("outbox"),
                )
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            app.manage(
                AutomationManager::open_at(config_dir.join("automations.json"))
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            app.manage(ControlWorkspaceTrustBatch::new());
            app.manage(ControlWindowState::default());
            app.manage(NotificationOverlayState::default());
            app.manage(AcpManager::default());
            app.manage(
                CodexQuotaMonitor::new()
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            app.manage(
                NotificationHistoryStore::new()
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            app.manage(
                SkillCenter::new()
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            for warning in retired_integrations::retire_owned_skills(&config_dir, &app.state::<SkillCenter>()).map_err(|e| -> Box<dyn std::error::Error> { e.into() })? { eprintln!("[crewvela] {warning}"); }
            if !test_profile::active() { browser_control::refresh_installed_skill(app.handle()); }
            app.manage(UsageTracker::default());
            let engineering = MultiAgentSupervisor::default();
            app.manage(engineering.previews.clone());
            app.manage(multi_agent::project_preview::LocalProjectServices::default());
            app.manage(engineering);
            app.manage(multi_agent::ConversationSupervisor::default());
            let session_manager = SessionManager::new()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let native_acp = NativeAcpSessions::open_at(config_dir.join("deepseek-harness-sessions.json"))
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let native_claude = NativeClaudeSessions(
                NativeAcpSessions::open_at_flavor(config_dir.join("claude-chat-sessions.json"), NativeChatFlavor::ClaudeChat)
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            let native_kimi = NativeKimiSessions(
                NativeAcpSessions::open_at_flavor(config_dir.join("kimi-chat-sessions.json"), NativeChatFlavor::KimiChat)
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            let native_codex = NativeCodexChatSessions(
                NativeAcpSessions::open_at_flavor(config_dir.join("codex-chat-sessions.json"), NativeChatFlavor::CodexChat)
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            let native_opencode = NativeOpenCodeChatSessions(
                NativeAcpSessions::open_at_flavor(config_dir.join("opencode-chat-sessions.json"), NativeChatFlavor::OpenCodeChat)
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?,
            );
            let native_pi = native_pi_sessions::NativePiSessions::open_at(config_dir.join("pi-chat-sessions.json"))
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            for record in session_manager.list() {
                if (sessions::is_standalone_deepseek_harness(&record) || sessions::is_standalone_zcode(&record) || sessions::is_standalone_claude_chat(&record) || sessions::is_standalone_kimi_chat(&record) || sessions::is_standalone_codex_chat(&record) || sessions::is_standalone_pi_chat(&record) || sessions::is_standalone_opencode_chat(&record))
                    && matches!(record.status, SessionStatus::Pending | SessionStatus::Running | SessionStatus::Waiting)
                {
                    session_manager
                        .mark_stopped(&record.id)
                        .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
                }
            }
            app.manage(NativeZCodeSessions::open_at(config_dir.join("zcode-sessions.json"))
                .map_err(|error| -> Box<dyn std::error::Error> {error.into()})?);
            app.manage(native_acp);
            app.manage(native_claude);
            app.manage(native_kimi);
            app.manage(native_codex);
            app.manage(native_opencode);
            app.manage(native_pi);
            app.manage(session_manager);
            let multi_agent_store = MultiAgentStore::new()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            multi_agent_store
                .recover_interrupted_runs_at_startup()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            multi_agent_store.recover_conversations_at_startup()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let resumable_projects = multi_agent_store.recover_managed_deliveries_at_startup()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if !test_profile::active() {
            let collaboration_bridge = CollaborationBridge::start(multi_agent_store.clone())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            eprintln!(
                "[crewvela] Agent 协作接口已启动：{}",
                collaboration_bridge.endpoint()
            );
            app.manage(collaboration_bridge);
            app.state::<AutomationManager>().template_runtime.set(automation::TemplateRuntime {
                supervisor: app.state::<multi_agent::ConversationSupervisor>().inner().clone(),
                bridge: app.state::<CollaborationBridge>().inner().clone(),
                admission: app.state::<runtime_admission::RuntimeAdmission>().inner().clone(),
            }).map_err(|_| std::io::Error::other("模板执行服务已初始化"))?;
            }
            // Dynamic application listeners must be bound before restoring the
            // embedded engine's saved port so an in-process clash is detectable.
            tauri::async_runtime::spawn(async {
                if let Err(error) = ai_memory::embedded::restore().await {
                    eprintln!("[crewvela] 内置共享记忆恢复失败：{error:#}");
                }
            });
            app.manage(multi_agent_store);
            // 通用设置：代理 env 写进本进程（后续 spawn 的 ACP/PTY/MCP 子进程自动继承）、
            // keep-awake 轮询线程、系统托盘（关闭到托盘）。
            general_settings::apply_process_env(&general_settings::load_cached());
            if !test_profile::active() && !resumable_projects.is_empty() {
                let supervisor = app.state::<MultiAgentSupervisor>();
                let store = app.state::<MultiAgentStore>();
                let host = multi_agent::AgentTerminalHost::new(app.state::<PtyManager>().inner().clone(),
                    app.state::<SessionManager>().inner().clone());
                let bridge = app.state::<CollaborationBridge>();
                for project in resumable_projects {
                    if let Err(error) = supervisor.wake_active(store.inner().clone(),project.clone(),host.clone(),bridge.inner().clone()) {
                        store.fail_recovered_managed_project(&project,&error)?;
                    }
                }
            }
            if !test_profile::active() { communication_skill::sync_installed()?; }
            keep_awake::start(app.handle().clone());
            setup_tray(app).map_err(|error| -> Box<dyn std::error::Error> {
                error.into()
            })?;
            // Isolated macOS windows must receive the store ID on the actual
            // builder, after all command state and the instance guard are ready.
            test_profile::build_isolated_windows(app,&isolated_windows)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == CONTROL_WINDOW_LABEL {
                    return;
                }
                // 通用设置：关闭主窗口 = 隐藏到托盘，Agent 会话继续运行；
                // 从托盘菜单「退出」才会真正结束应用。
                if window.label() == "main" && !test_profile::active() && general_settings::load_cached().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                if !should_stop_sessions_for_window(window.label()) {
                    return;
                }
                if !shutdown_all_sessions(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler({ let handler:fn(tauri::ipc::Invoke<tauri::Wry>)->bool=tauri::generate_handler![
            test_profile_info,
            get_runtime_preferences,
            set_runtime_launch_default,
            set_runtime_enabled,
            skill_center_snapshot,
            skill_center_update_checks,
            skill_center_check_updates,
            skill_center_versions,
            skill_center_prepare_rollback,
            skill_center_apply_update,
            skill_center_sync_update,
            skill_center_import,
            skill_center_set_trust,
            skill_center_delete_asset,
            skill_center_preview,
            skill_center_read_file,
            skill_center_discover,
            skill_center_deploy_batch,
            skill_center_undeploy,
            skill_center_undeploy_batch,
            skill_center_market_repositories,
            skill_center_market_repository_save,
            skill_center_market_repository_delete,
            skill_center_market_browse,
            skill_center_skills_sh_search,
            skill_center_market_install,
            skill_center_save_set,
            skill_center_delete_set,
            skill_center_apply_set,
            native_language::set_interface_language,
            git_settings_get,
            git_settings_save,
            git_status,
            git_diff,
            git_stage,
            git_unstage,
            git_discard,
            git_commit,
            git_branches,
            git_merge,
            git_merge_abort,
            git_remote,
            git_generate_commit_message,
            notification_history_list,
            notification_history_append,
            notification_history_import,
            notification_history_mark_all_read,
            notification_history_clear,
            workspace_list_directory,
            workspace_read_text,
            workspace_search_files,
            workspace_media_path,
            workspace_open_path,
            workspace_write_text,
            workspace_reveal_path,
            worktree_management::worktree_branches,
            worktree_management::worktree_overview,
            worktree_management::worktree_set_root,
            worktree_management::worktree_clean,
            worktree_management::worktree_lifecycle_prepare,
            worktree_management::worktree_lifecycle_restore,
            worktree_lifecycle::worktree_lifecycle_get,
            worktree_lifecycle::worktree_lifecycle_save,
            browser_control::browser_control_get,
            browser_control::browser_control_save,
            general_settings::general_settings_get,
            app_help::app_check_update,
            app_help::app_data_directory,
            general_settings::general_settings_save,
            terminal_fonts::terminal_fonts_list,
            send_windows_unpacked_notification,
            list_projects,
            create_project,
            update_project,
            delete_project,
            edit_local_project,
            local_project_service_start,
            local_project_service_stop,
            local_project_service_logs,
            local_project_service_statuses,
            list_launch_presets,
            create_launch_preset,
            delete_launch_preset,
            codex_quota_monitor_status,
            list_codex_quota_notifications,
            check_codex_quota_updates,
            cid_list_accounts,
            cid_init_accounts,
            cid_add_account,
            cid_add_token,
            cid_use_account,
            cid_remove_account,
            restart_codex_app,
            claude_path,
            paste_clipboard_image,
            clipboard_has_image,
            clipboard_file_paths,
            write_pasted_files_to_temp,
            clipboard_image_file,
            paste_dropped_image,
            ma_probe_runtimes,
            ma_check_runtime_updates,
            ma_upgrade_runtime,
            runtime_install_environment,
            runtime_set_zcode_cli_path,
            runtime_chat_dependencies,
            runtime_install_latest_version,
            runtime_install_start,
            runtime_install_status,
            runtime_install_cancel,
            runtime_open_login,
            ma_list_projects,
            ma_list_project_activity,
            ma_list_interventions,
            ma_resolve_intervention,
            ma_create_project,
            ma_rename_project,
            ma_set_project_reply_preference,
            ma_project_git_status,
            ma_member_library,
            ma_member_library_import,
            ma_member_library_apply,
            ma_member_library_auto_apply,
            ma_member_library_update,
            ma_create_agent,
            ma_import_agent_avatar,
            ma_agent_avatar_image,
            ma_update_agent,
            ma_remove_agent,
            ma_create_task,
            ma_add_dependency,
            ma_claim_task,
            ma_update_run,
            ma_confirm_run_stopped,
            ma_enqueue_wakeup,
            ma_snapshot,
            collaboration_support::collaboration_health,
            collaboration_support::collaboration_handoff_autofill,
            collaboration_support::collaboration_handoff_preview,
            collaboration_support::collaboration_handoff_confirm,
            collaboration_support::collaboration_handoff_cancel,
            ai_memory::ai_memory_status,
            ai_memory::embedded::ai_memory_embedded_status,
            ai_memory::embedded::ai_memory_embedded_set_enabled,
            ai_memory_access::ai_memory_project_access,
            ai_memory::zcode::ai_memory_zcode_sessions,
            ai_memory::ai_memory_install,
            ai_memory::ai_memory_recover_update,
            ai_memory::ai_memory_serve,
            ai_memory::onboarding::ai_memory_quick_info,
            ai_memory::onboarding::ai_memory_quick_start,
            ai_memory::onboarding::ai_memory_quick_status,
            ai_memory::ai_memory_stop,
            ai_memory::ai_memory_setup,
            ai_memory::ai_memory_check,
            ai_memory::ai_memory_open_handoffs,
            ai_memory::ai_memory_session_badges,
            ma_clear_conversation,
            ma_run_logs,
            ma_run_log_history,
            ma_activity_events,
            ma_runtime_events,
            ma_send_message,
            ma_conversation_snapshot,
            multi_agent::conversation_metrics::ma_conversation_round_metrics,
            ma_conversation_member_models,
            ma_set_conversation_member_models,
            ma_conversation_export,
            ma_conversation_design_delivery,
            ma_conversation_run_transcript,
            ma_conversation_policy_set,
            ma_conversation_migration_preview,
            ma_conversation_migration_apply,
            ma_conversation_clear,
            ma_conversation_new,
            ma_conversation_history,
            ma_conversation_switch,
            ma_conversation_rename,
            ma_conversation_delete,
            ma_conversation_send,
            ma_conversation_plan_preflight,
            ma_conversation_codex_login,
            ma_conversation_retry_codex_members,
            ma_conversation_resume_managed_after_login,
            ma_conversation_stage_attachments,
            ma_conversation_preview_image,
            ma_conversation_preview_video,
            ma_conversation_open_video,
            ma_conversation_open_attachment,
            ma_project_preview_save,
            ma_project_preview_start,
            ma_project_preview_status,
            ma_project_preview_stop,
            ma_conversation_plan_retry_input,
            ma_conversation_interrupt,
            ma_conversation_queue,
            ma_conversation_change_queued,
            ma_conversation_steer_queued,
            ma_conversation_control,
            ma_conversation_execute_plan,
            ma_send_direct_message,
            ma_publish_artifact,
            ma_request_approval,
            ma_heartbeat_run,
            ma_put_memory,
            ma_search_memory,
            ma_rebuild_knowledge_index,
            ma_search_knowledge,
            ma_recover,
            ma_interpret_message,
            ma_cancel_interpret_message,
            ma_apply_plan,
            ma_start_project,
            ma_pause_project,
            ma_cancel_project,
            ma_approve_task,
            ma_update_resumed_delivery,
            ma_retry_task,
            ma_reassign_task,
            ma_takeover_agent,
            ma_return_agent_control,
            launch_multi,
            launch_terminal,
            launch_runtime_session,
            create_zcode_session, zcode_snapshot, start_zcode_session, send_zcode_message, edit_zcode_message, zcode_slash_command, zcode_side_session, zcode_side_send, zcode_side_close, zcode_composer_catalog, zcode_attachment_info, set_zcode_model, set_zcode_permission_mode, zcode_queue_action, zcode_child_action, recover_zcode_queue_input, answer_zcode_permission, cancel_zcode_turn,
            create_deepseek_harness_session,
            deepseek_harness_snapshot,
            start_deepseek_harness_session,
            set_deepseek_harness_model,
            control_deepseek_harness_session,
            send_deepseek_harness_message,
            answer_deepseek_harness_permission,
            cancel_deepseek_harness_turn,
            create_claude_chat_session,
            claude_chat_snapshot,
            start_claude_chat_session,
            set_claude_chat_config,
            send_claude_chat_message,
            answer_claude_chat_permission,
            cancel_claude_chat_turn,
            create_kimi_chat_session,
            kimi_chat_snapshot,
            start_kimi_chat_session,
            set_kimi_chat_config,
            send_kimi_chat_message,
            answer_kimi_chat_permission,
            cancel_kimi_chat_turn,
            create_opencode_chat_session,
            opencode_chat_snapshot,
            start_opencode_chat_session,
            set_opencode_chat_config,
            send_opencode_chat_message,
            answer_opencode_chat_permission,
            cancel_opencode_chat_turn,
            create_codex_chat_session,
            codex_chat_snapshot,
            start_codex_chat_session,
            set_codex_chat_config,
            control_codex_chat_goal,
            send_codex_chat_message,
            get_codex_chat_plugins,
            answer_codex_chat_permission,
            cancel_codex_chat_turn,
            create_pi_chat_session,
            pi_chat_snapshot,
            start_pi_chat_session,
            set_pi_chat_config,
            send_pi_chat_message,
            answer_pi_chat_permission,
            cancel_pi_chat_turn,
            list_sessions,
            list_archived_sessions,
            list_approval_cards,
            respond_session_approval,
            set_session_approval_policy,
            list_agent_activity,
            set_agent_activity,
            session_snapshot,
            session_usages,
            session_telemetry,
            session_transcript,
            session_switch::session_switch_list,
            session_switch::session_switch_history,
            session_switch::session_switch_preview,
            session_switch::session_switch_confirm,
            session_switch::session_switch_restore,
            session_switch::session_switch_return_preview,
            session_switch::session_switch_dismiss_recovery,
            resume_session,
            rename_session,
            stop_session,
            archive_session,
            restore_archived_session,
            archive_project_sessions,
            delete_session,
            delete_all_sessions,
            interpret_control_message,
            register_control_launch_batch,
            control_workspace_trust_status,
            confirm_control_workspace_trust,
            acp_start,
            acp_new_session,
            acp_prompt,
            acp_cancel,
            broadcast_prompt,
            send_session_message,
            list_delivery_records,
            list_communication_evidence,
            message_receipts::list_session_message_receipts,
            list_automations,
            run_automation_now,
            stop_automation_run,
            acknowledge_automation_run,
            create_automation,
            update_automation,
            set_automation_enabled,
            delete_automation,
            preview_cron_trigger,
            automation_tick,
            optimize_prompt,
            set_all_permission_modes,
            pty_poll_all,
            pty_input,
            pty_resize,
            pty_kill_all,
            open_control_window,
            close_control_window,
            dock_control_window,
            take_control_dock_request,
            is_control_window_open,
            show_notification_overlay,
            notification_overlay_current,
            notification_overlay_ready,
            close_notification_overlay,
            focus_main_from_notification,
            browser_panel::browser_panel_open,
            browser_panel::browser_panel_close,
            browser_panel::browser_panel_set_bounds,
            browser_panel::browser_panel_set_visible,
            browser_panel::browser_panel_navigate,
            browser_panel::browser_panel_history,
            browser_panel::browser_panel_current_url,
        ]; move |invoke:tauri::ipc::Invoke<tauri::Wry>| {
            if test_profile::active() && !test_profile::allowed_command(invoke.message.command()) {
                invoke.resolver.reject(test_profile::BLOCKED_MESSAGE);true
            } else { handler(invoke) }
        } })
        .run(context)
        .expect("error while running crewvela");
}

fn report_shutdown_failure<R: tauri::Runtime>(app: &AppHandle<R>, message: impl Into<String>) -> bool {
    let message = message.into();
    eprintln!("[crewvela] {message}");
    let _ = app.emit("app-shutdown-error", message);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    false
}

/// 全量停机：停掉所有原生聊天 ACP 会话、PTY与控制面后退出。
/// 真关窗（未开托盘）与托盘「退出」共用；返回 false = 某一步失败已发事件、勿退出。
fn shutdown_all_sessions<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    let runtime_shutdown = if let Some(admission) = app.try_state::<runtime_admission::RuntimeAdmission>() {
        match admission.begin_shutdown(std::time::Duration::from_secs(10)) {
            Ok(guard) => Some(guard),
            Err(error) => return report_shutdown_failure(app, format!("等待会话启动结束失败，未退出应用：{error:#}")),
        }
    } else { None };
    if let Some(services) = app.try_state::<multi_agent::project_preview::LocalProjectServices>() {
        if let Err(error) = services.stop_all() {
            return report_shutdown_failure(app, format!("开发服务停止未确认，未退出应用：{error:#}"));
        }
    }
    if let (Some(supervisor), Some(store)) = (app.try_state::<multi_agent::ConversationSupervisor>(), app.try_state::<MultiAgentStore>()) {
        if let Err(error) = supervisor.stop_all(&store) { return report_shutdown_failure(app, format!("讨论停止未确认，未退出应用：{error:#}")); }
    }
    let shutdown_guard = if let Some(supervisor) = app.try_state::<MultiAgentSupervisor>() {
        match supervisor.begin_shutdown() {
            Ok(guard) => Some(guard),
            Err(error) => return report_shutdown_failure(app, error.to_string()),
        }
    } else { None };
    if let Some(store) = app.try_state::<MultiAgentStore>() {
        let verified = (|| -> anyhow::Result<()> {
            for project in store.list_projects()? {
                for run in store.snapshot(&project.id)?.runs {
                    anyhow::ensure!(!run.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true), "项目 {} 的执行 {} 停止未确认", project.name, run.id);
                }
            }
            Ok(())
        })();
        if let Err(error) = verified {
            return report_shutdown_failure(app, format!("无法确认协作执行停止，未退出应用：{error:#}"));
        }
    }
    if let Some(supervisor) = app.try_state::<MultiAgentSupervisor>() {
        let Some(store) = app.try_state::<MultiAgentStore>() else {
            return report_shutdown_failure(app, "无法核实协作执行状态，未退出应用");
        };
        for project_id in shutdown_guard.as_ref().unwrap().project_ids.iter().cloned() {
            match supervisor.pause(&store, &project_id) {
                Ok(receipt) if receipt.accepted => {}
                Ok(receipt) => return report_shutdown_failure(app, format!("协作 Agent 停止未确认，未退出应用：{}", receipt.message)),
                Err(error) => return report_shutdown_failure(app, format!("停止协作项目失败，未退出应用：{error}")),
            }
        }
    }
    if let (Some(m), Some(sessions)) = (
        app.try_state::<NativeAcpSessions>(),
        app.try_state::<SessionManager>(),
    ) {
        if let Err(error) = m.stop_all(&sessions, app) {
            let _ = app.emit("deepseek-harness-error", format!("停止 DeepSeek Harness 会话失败，未退出应用：{error}"));
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<NativeZCodeSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.stop_all(&sessions, app) {
            let _ = app.emit("zcode-session-error", error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<NativeClaudeSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.0.stop_all(&sessions, app) {
            let _ = app.emit(NativeChatFlavor::ClaudeChat.error_event_name(), error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<NativeKimiSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.0.stop_all(&sessions, app) {
            let _ = app.emit(NativeChatFlavor::KimiChat.error_event_name(), error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<NativeCodexChatSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.0.stop_all(&sessions, app) {
            let _ = app.emit(NativeChatFlavor::CodexChat.error_event_name(), error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<NativeOpenCodeChatSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.0.stop_all(&sessions, app) {
            let _ = app.emit(NativeChatFlavor::OpenCodeChat.error_event_name(), error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let (Some(m), Some(sessions)) = (app.try_state::<native_pi_sessions::NativePiSessions>(), app.try_state::<SessionManager>()) {
        if let Err(error) = m.stop_all(&sessions, app) {
            let _ = app.emit("pi-chat-error", error.to_string());
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let Some(m) = app.try_state::<PtyManager>() {
        if let Err(error) = m.kill_all() {
            let _ = app.emit("pty-stop-error", format!("终端停止未确认，保留窗口和队列：{error}"));
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let Some(m) = app.try_state::<AcpManager>() {
        if let Err(error) = m.stop() {
            let _ = app.emit("acp-error", format!("ACP 停止未确认，未退出应用：{error}"));
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let Some(m) = app.try_state::<SessionManager>() {
        if let Err(error) = m.mark_all_stopped() {
            let _ = app.emit("pty-stop-error", format!("停止状态保存失败，未退出应用：{error}"));
            return report_shutdown_failure(app, format!("停止会话失败，未退出应用：{error}"));
        }
    }
    if let Some(m) = app.try_state::<SessionActionQueue>() {
        m.clear();
    }
    if let Some(guard) = shutdown_guard { guard.commit(); }
    if let Some(guard) = runtime_shutdown { guard.commit(); }
    ai_memory::embedded::stop_at_exit();
    app.exit(0);
    true
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 系统托盘（通用设置「关闭窗口时隐藏到托盘」的载体）：
/// 右键菜单 显示/退出；左键单击直接唤回主窗口。
fn setup_tray(app: &tauri::App) -> anyhow::Result<()> {
    if test_profile::active() { return Ok(()); }
    use tauri::menu::Menu;
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let (show_label, quit_label) = native_language::InterfaceLanguage::load().tray_labels();
    let show = tauri::menu::MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::with_id("crewvela-tray")
        .tooltip("CrewVela")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                if !shutdown_all_sessions(app) {
                    eprintln!("[crewvela] 托盘退出时部分会话停止失败，保留窗口和进程跟踪");
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(native_language::TrayMenuItems { show, quit });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn approving_delivery_wakes_idle_active_scheduler_and_respects_pause() {
        use multi_agent::{ClaimTaskInput, ProjectStatus, RunStatus, TaskStatus, UpdateRunInput};
        for paused in [false, true] {
            let (root, sessions) = lifecycle_fixture();
            let store = MultiAgentStore::open_at(root.join("delivery.db")).unwrap();
            let project = store.create_project(CreateProjectInput {
                name: "Delivery wake".into(), goal: "Test the approval path".into(),
                root_path: root.display().to_string(), default_branch: "main".into(),
            }).unwrap();
            let agent = store.create_agent(serde_json::from_value(serde_json::json!({
                "projectId": project.id, "name": "worker", "runtimeKind": "codex", "role": "worker"
            })).unwrap()).unwrap();
            let create_task = |name: &str| store.create_task(serde_json::from_value(serde_json::json!({
                "projectId": project.id, "title": name, "acceptanceCriteria": "fixture",
                "assigneeAgentId": agent.id, "workspaceMode": "read_only", "maxAttempts": 1
            })).unwrap()).unwrap();
            let delivery = create_task("delivery");
            let downstream = create_task("downstream");
            store.add_dependency(multi_agent::AddDependencyInput {
                task_id: downstream.id.clone(), depends_on_task_id: delivery.id.clone(),
            }).unwrap();
            store.set_project_status(&project.id, ProjectStatus::Active).unwrap();
            let run = store.claim_task(ClaimTaskInput {
                task_id: delivery.id.clone(), agent_id: agent.id, lease_seconds: 120,
            }).unwrap();
            store.update_run(UpdateRunInput {
                run_id: run.id.clone(), status: RunStatus::Running, output_summary: None, error: None, session_locator: None,
            }).unwrap();
            store.finalize_successful_run(&run.id, "fixture", "ready to approve", false, None, None).unwrap();
            if paused {
                store.set_project_status(&project.id, ProjectStatus::Paused).unwrap();
            }
            let app = tauri::test::mock_app();
            app.manage(store.clone());
            app.manage(MultiAgentSupervisor::default());
            app.manage(PtyManager::new());
            app.manage(sessions);
            app.manage(CollaborationBridge::start(store.clone()).unwrap());
            let task = ma_approve_task(delivery.id, app.state(), app.state(), app.state(), app.state(), app.state()).unwrap();
            assert_eq!(task.status, TaskStatus::Done);
            if paused {
                assert!(app.state::<MultiAgentSupervisor>().active_project_ids().is_empty());
                assert_eq!(store.project(&project.id).unwrap().status, ProjectStatus::Paused);
                assert!(!store.snapshot(&project.id).unwrap().runs.iter().any(|r| r.task_id == downstream.id));
            } else {
                // The fixture deliberately has no Git repository, so claiming
                // the dependent terminates before invoking the real Agent CLI.
                let started = std::time::Instant::now();
                loop {
                    let snapshot = store.snapshot(&project.id).unwrap();
                    if let Some(run) = snapshot.runs.iter().find(|r| r.task_id == downstream.id && r.status == RunStatus::Failed) {
                        assert!(run.error.as_deref().is_some_and(|error|error.contains("Git")));
                        break;
                    }
                    assert!(started.elapsed() < std::time::Duration::from_secs(10), "approved task did not wake its dependent");
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                app.state::<MultiAgentSupervisor>().pause(&store, &project.id).unwrap();
            }
        }
    }

    #[test]
    fn project_start_keeps_retired_tasks_without_blocking_available_members() {
        let (root, sessions) = lifecycle_fixture();
        runtime_preferences::with_disabled_for_test(multi_agent::AgentRuntimeKind::Pi, || {
            let database = root.join("control.db");
            let store = MultiAgentStore::open_at(database.clone()).unwrap();
            let project = store.create_project(CreateProjectInput {
                name: "mixed runtime project".into(), goal: "preserve legacy tasks".into(),
                root_path: root.display().to_string(), default_branch: "main".into(),
            }).unwrap();
            let create_agent = |name: &str| store.create_agent(serde_json::from_value(serde_json::json!({
                "projectId": project.id, "name": name, "runtimeKind": "codex", "role": "worker"
            })).unwrap()).unwrap();
            let legacy = create_agent("legacy");
            let current = create_agent("current");
            let create_task = |name: &str, agent: &str| store.create_task(serde_json::from_value(serde_json::json!({
                "projectId": project.id, "title": name, "acceptanceCriteria": "fixture",
                "assigneeAgentId": agent, "workspaceMode": "read_only"
            })).unwrap()).unwrap();
            let old = create_task("legacy work", &legacy.id);
            let dependent = create_task("wait for legacy work", &current.id);
            store.add_dependency(multi_agent::AddDependencyInput {
                task_id: dependent.id.clone(), depends_on_task_id: old.id.clone(),
            }).unwrap();
            let connection = rusqlite::Connection::open(&database).unwrap();
            connection.execute("UPDATE ma_agents SET runtime_kind='removed_runtime' WHERE id=?1", [&legacy.id]).unwrap();
            drop(connection);
            drop(store);
            let store = MultiAgentStore::open_at(database).unwrap();
            let before = store.snapshot(&project.id).unwrap();
            let app = tauri::test::mock_app();
            app.manage(store.clone());
            app.manage(MultiAgentSupervisor::default());
            app.manage(PtyManager::new());
            app.manage(sessions.clone());
            app.manage(CollaborationBridge::start(store.clone()).unwrap());
            let receipt = ma_start_project(project.id.clone(), app.state(), app.state(), app.state(), app.state(), app.state())
                .expect("historical retired work must not prevent starting available members");
            assert!(receipt.accepted);
            assert!(receipt.message.contains("1 个任务"), "{}", receipt.message);
            assert!(app.state::<MultiAgentSupervisor>().pause(&store, &project.id).unwrap().accepted);
            let after = store.snapshot(&project.id).unwrap();
            assert_eq!(after.tasks.len(), before.tasks.len());
            for previous in &before.tasks {
                let task = after.tasks.iter().find(|task| task.id == previous.id).unwrap();
                assert_eq!(task.status, previous.status);
                assert_eq!(task.assignee_agent_id, previous.assignee_agent_id);
            }
            assert!(sessions.list().iter().any(|session| session.multi_agent_agent_id.as_deref() == Some(&current.id)));
            assert!(!sessions.list().iter().any(|session| session.multi_agent_agent_id.as_deref() == Some(&legacy.id)));
            assert!(store.runnable_tasks(&project.id).unwrap().is_empty(), "legacy dependencies must still block dependents");
            assert!(store.claim_task(multi_agent::ClaimTaskInput {
                task_id: old.id, agent_id: legacy.id, lease_seconds: 60,
            }).is_err());
            let independent = store.create_task(serde_json::from_value(serde_json::json!({
                "projectId": project.id, "title": "new independent task", "acceptanceCriteria": "fixture",
                "assigneeAgentId": current.id, "workspaceMode": "read_only"
            })).unwrap()).unwrap();
            store.route_runnable_tasks(&project.id).unwrap();
            assert!(store.runnable_tasks(&project.id).unwrap().iter().any(|(task, _)| task.id == independent.id));
            store.set_project_status(&project.id, multi_agent::ProjectStatus::Active).unwrap();
            let run = store.claim_task(multi_agent::ClaimTaskInput {
                task_id: independent.id.clone(), agent_id: current.id, lease_seconds: 60,
            }).unwrap();
            assert_eq!(run.task_id, independent.id);
        });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn audit_blocking_commands_leave_the_event_loop_responsive_and_preserve_errors() {
        let (release, wait) = std::sync::mpsc::channel();
        let task = super::blocking_command(move || {
            wait.recv_timeout(std::time::Duration::from_secs(2)).map_err(super::to_err)?;
            Ok(42)
        });
        tokio::pin!(task);
        let still_running = tokio::time::timeout(std::time::Duration::from_millis(50), &mut task).await;
        release.send(()).unwrap();
        assert!(still_running.is_err(), "the event-loop timer must fire while the worker waits");
        assert_eq!(task.await.unwrap(), 42);
        assert_eq!(super::blocking_command(|| Err::<(), _>("fixture failure".to_string())).await.unwrap_err(), "fixture failure");
    }

    #[tokio::test]
    async fn audit_workspace_command_roundtrip_preserves_conflicts_and_path_boundaries() {
        let root = std::env::temp_dir().join(format!("crewvela-command-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("test.txt"), "before").unwrap();
        let dir = root.to_string_lossy().into_owned();
        assert_eq!(super::workspace_list_directory(dir.clone(), "".into()).await.unwrap().len(), 1);
        assert_eq!(super::workspace_read_text(dir.clone(), "test.txt".into()).await.unwrap(), "before");
        super::workspace_write_text(dir.clone(), "test.txt".into(), "after".into(), "before".into()).await.unwrap();
        let conflict = super::workspace_write_text(dir.clone(), "test.txt".into(), "stale".into(), "before".into()).await;
        assert!(conflict.unwrap_err().contains("其他程序修改"));
        assert!(super::workspace_read_text(dir.clone(), "../outside.txt".into()).await.is_err());
        assert_eq!(std::fs::read_to_string(root.join("test.txt")).unwrap(), "after");
        std::fs::remove_dir_all(root).unwrap();
    }
    use super::*;

    #[cfg(windows)]
    pub(crate) fn lifecycle_node_pty(ptys: &PtyManager, app_id: &str, exit_root: bool) -> (u32, u32, u32) {
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", if exit_root {
            "const c=require('node:child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore',windowsHide:true});console.log('ROOT_PID='+process.pid+' CHILD_PID='+c.pid);c.unref();process.exit(7)"
        } else { "console.log('ROOT_PID='+process.pid+' CHILD_PID=0');setInterval(()=>{},1000)" }]);
        let id = ptys.open_agent_program(&command, app_id, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut output = String::new();
        loop {
            if let Some(chunk) = ptys.poll_all().remove(&id) { output.push_str(&chunk.data); }
            let number = |label: &str| output.split(label).nth(1).and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next()).and_then(|pid| pid.parse::<u32>().ok());
            if let (Some(root), Some(child)) = (number("ROOT_PID="), number("CHILD_PID=")) {
                if !exit_root || !ptys.is_running(id) { return (id, root, child); }
            }
            assert!(std::time::Instant::now() < deadline, "{output}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(windows)]
    fn lifecycle_process_exists(pid: u32) -> bool {
        let output = std::process::Command::new("tasklist.exe").args(["/FO", "CSV", "/NH"]).output().unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).lines().any(|line| line.split("\",\"").nth(1).and_then(|field| field.trim_matches('"').parse::<u32>().ok()) == Some(pid))
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_stale_exit_chunk_cannot_clear_restored_binding_and_failed_tree_stop_keeps_messages() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let ptys = PtyManager::new();
        let (a, root_pid, child_pid) = lifecycle_node_pty(&ptys, &record.id, true);
        sessions.attach_runtime(&record.id, a).unwrap();
        assert!(lifecycle_process_exists(child_pid), "fixture child must survive its root");
        let mut chunks = ptys.poll_all();
        assert_eq!(chunks[&a].code, Some(7));
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&record.id, QueuedMessage::new("keep before confirmed stop".into(), None));
        let automations = AutomationManager::new();
        let refusal = acp::TestStopFailureGuard::new(root_pid);
        assert!(process_polled_session_exits(&mut chunks, &ptys, &sessions, &queue, &automations, None).is_err());
        assert_eq!(sessions.get(&record.id).unwrap().pty_id, Some(a));
        assert!(queue.peek_next(&record.id).is_some());
        assert!(lifecycle_process_exists(child_pid));
        drop(refusal);
        process_polled_session_exits(&mut chunks, &ptys, &sessions, &queue, &automations, None).unwrap();
        assert!(sessions.get(&record.id).unwrap().pty_id.is_none());
        assert!(queue.peek_next(&record.id).is_none());
        assert!(!lifecycle_process_exists(child_pid));
        let (b, _, _) = lifecycle_node_pty(&ptys, &record.id, false);
        sessions.attach_runtime(&record.id, b).unwrap();
        queue.enqueue_message(&record.id, QueuedMessage::new("restored B message".into(), None));
        process_polled_session_exits(&mut chunks, &ptys, &sessions, &queue, &automations, None).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().pty_id, Some(b));
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        assert_eq!(queue.peek_next(&record.id).unwrap().body, "restored B message");
        assert!(ptys.is_running(b));
        ptys.kill(b).unwrap(); sessions.mark_stopped(&record.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_fixed_delete_batch_preserves_new_attached_session_and_refuses_old_unverified_stop() {
        let (root, sessions) = lifecycle_fixture();
        let old = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let ptys = PtyManager::new();
        let (old_pty, old_pid, _) = lifecycle_node_pty(&ptys, &old.id, false);
        sessions.attach_runtime(&old.id, old_pty).unwrap();
        let batch = sessions.list();
        // Deterministic barrier between snapshot and per-item cleanup.
        let fresh = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let (fresh_pty, _, _) = lifecycle_node_pty(&ptys, &fresh.id, false);
        sessions.attach_runtime(&fresh.id, fresh_pty).unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&old.id, QueuedMessage::new("keep old failure".into(), None));
        queue.enqueue_message(&fresh.id, QueuedMessage::new("keep new session".into(), None));
        let refusal = acp::TestStopFailureGuard::new(old_pid);
        let delete = |id: &str| {
            let lifecycle = sessions.lifecycle_lock(id); let _guard = lifecycle.lock().unwrap();
            detach_session_runtime(id, &ptys, &sessions, &queue)?;
            sessions.delete(id).map_err(to_err)
        };
        assert!(delete_session_batch(batch.clone(), delete).is_err());
        assert_eq!(sessions.get(&old.id).unwrap().pty_id, Some(old_pty));
        assert!(queue.peek_next(&old.id).is_some());
        assert!(sessions.get(&fresh.id).is_some()); assert!(ptys.is_running(fresh_pty));
        drop(refusal);
        assert_eq!(delete_session_batch(batch, delete).unwrap(), 1);
        assert!(sessions.get(&old.id).is_none());
        assert_eq!(sessions.get(&fresh.id).unwrap().pty_id, Some(fresh_pty));
        assert!(queue.peek_next(&fresh.id).is_some());
        assert!(ptys.is_running(fresh_pty));
        ptys.kill(fresh_pty).unwrap(); sessions.mark_stopped(&fresh.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    fn approval_recorder_pty(ptys: &PtyManager, app_id: &str, input_file: &std::path::Path) -> u32 {
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", "const fs=require('node:fs'),file=process.argv[1];if(process.stdin.setRawMode)process.stdin.setRawMode(true);process.stdin.on('data',bytes=>fs.appendFileSync(file,bytes));console.log('RECORDER_READY');setInterval(()=>{},1000)"]).arg(input_file);
        let id = ptys.open_agent_program(&command, app_id, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut output = String::new();
        while !output.contains("RECORDER_READY") {
            output.push_str(&ptys.poll_all().remove(&id).unwrap().data);
            assert!(std::time::Instant::now() < deadline, "{output}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        id
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_old_approval_card_never_writes_new_pty_or_new_waiting_generation() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create(sessions::NewSession { provider_id:"fixture".into(), provider_name:"fixture".into(), cwd:root.display().to_string(), yolo:false, auto_mode:false, project_id:None }).unwrap();
        let ptys = PtyManager::new();
        let first = approval_recorder_pty(&ptys, &record.id, &root.join("input-a"));
        sessions.attach_runtime(&record.id, first).unwrap();
        let notification = serde_json::json!({"session_id":record.claude_session_id,"hook_event_name":"Notification","notification_type":"permission_prompt","message":"fixture tool approval"});
        sessions.apply_hook_value(&notification).unwrap();
        let old = collect_approval_cards(&sessions).pop().unwrap();
        ptys.kill(first).unwrap(); sessions.mark_stopped(&record.id).unwrap();
        let input = root.join("input-b");
        let second = approval_recorder_pty(&ptys, &record.id, &input);
        sessions.attach_runtime(&record.id, second).unwrap(); sessions.apply_hook_value(&notification).unwrap();
        let current = collect_approval_cards(&sessions).pop().unwrap();
        assert_ne!(old.pty_id, current.pty_id);
        assert!(respond_session_approval_inner(&record.id, ApprovalDecision::Allow, old.pty_id, old.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).is_err());
        assert!(!input.exists(), "stale A card must send zero keys to actual B process");
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Waiting);
        respond_session_approval_inner(&record.id, ApprovalDecision::Allow, current.pty_id, current.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let sent = loop {
            if let Ok(bytes) = std::fs::read(&input) { if bytes.contains(&b'1') { break bytes; } }
            assert!(std::time::Instant::now() < deadline, "valid approval did not reach actual PTY child");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(sent.iter().filter(|byte| **byte == b'1').count(), 1);
        assert!(respond_session_approval_inner(&record.id, ApprovalDecision::Allow, current.pty_id, current.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).is_err());
        sessions.apply_hook_value(&notification).unwrap();
        let next = collect_approval_cards(&sessions).pop().unwrap();
        assert!(next.waiting_since > current.waiting_since);
        assert_eq!(next.pty_id, current.pty_id);
        assert!(respond_session_approval_inner(&record.id, ApprovalDecision::Allow, current.pty_id, current.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).is_err());
        assert_eq!(std::fs::read(&input).unwrap(), sent, "old same-PTY waiting generation must send zero keys");
        respond_session_approval_inner(&record.id, ApprovalDecision::Deny, next.pty_id, next.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).unwrap();
        assert!(respond_session_approval_inner(&record.id, ApprovalDecision::Deny, next.pty_id, next.waiting_since, &ptys, &sessions, &SessionActionQueue::new()).is_err());
        ptys.kill(second).unwrap(); sessions.mark_stopped(&record.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_runtime_change_keeps_old_binding_until_descendants_are_confirmed_stopped() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_agent(sessions::NewAgentSession { avatar_id:None, project_id:"fixture-project".into(), agent_id:"fixture-agent".into(), runtime_kind:"codex".into(), agent_name:"fixture".into(), cwd:root.display().to_string() }).unwrap();
        let ptys = PtyManager::new();
        let (pty, root_pid, child_pid) = lifecycle_node_pty(&ptys, &record.id, true);
        sessions.attach_runtime(&record.id, pty).unwrap();
        sessions.set_runtime_command_host(&record.id, true).unwrap();
        let refusal = acp::TestStopFailureGuard::new(root_pid);
        let lifecycle = sessions.lifecycle_lock(&record.id); let _guard = lifecycle.lock().unwrap();
        assert!(stop_member_runtime_ptys(&sessions, &ptys, "fixture-project", "fixture-agent").is_err());
        let old = sessions.get(&record.id).unwrap();
        assert_eq!(old.pty_id, Some(pty)); assert!(!old.retired_agent_runtime); assert!(old.archived_at.is_none());
        assert!(lifecycle_process_exists(child_pid));
        drop(refusal);
        stop_member_runtime_ptys(&sessions, &ptys, "fixture-project", "fixture-agent").unwrap();
        assert!(!lifecycle_process_exists(child_pid));
        sessions.retire_agent_runtime_sessions("fixture-project", "fixture-agent").unwrap();
        let retired = sessions.get(&record.id).unwrap();
        assert!(retired.retired_agent_runtime); assert!(retired.pty_id.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_migration_does_not_kill_an_unbound_pty_during_native_startup() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_agent(sessions::NewAgentSession { avatar_id:None, project_id:"fixture-project".into(), agent_id:"fixture-agent".into(), runtime_kind:"codex".into(), agent_name:"fixture".into(), cwd:root.display().to_string() }).unwrap();
        let ptys = PtyManager::new();
        let starting = sessions.begin_runtime_start(&record.id).unwrap();
        let (pty, _, _) = lifecycle_node_pty(&ptys, &record.id, false);
        assert!(!migrate_inactive_member_session(&sessions, &ptys, &record.id).unwrap());
        assert!(ptys.is_running(pty));
        assert!(!sessions.get(&record.id).unwrap().runtime_command_host);
        sessions.attach_runtime(&record.id, pty).unwrap(); drop(starting);
        assert!(!migrate_inactive_member_session(&sessions, &ptys, &record.id).unwrap());
        assert!(ptys.is_running(pty));
        ptys.kill(pty).unwrap(); sessions.mark_stopped(&record.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_shutdown_refusal_keeps_window_session_binding_and_queue() {
        use tauri::Listener;
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let ptys = PtyManager::new();
        let (pty, root_pid, _) = lifecycle_node_pty(&ptys, &record.id, false);
        sessions.attach_runtime(&record.id, pty).unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&record.id, QueuedMessage::new("keep on refused quit".into(), None));
        let app = tauri::test::mock_app();
        app.manage(sessions.clone()); app.manage(ptys.clone()); app.manage(queue);
        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&errors);
        app.listen("app-shutdown-error", move |event| captured.lock().unwrap().push(event.payload().to_string()));
        let refusal = acp::TestStopFailureGuard::new(root_pid);
        assert!(!shutdown_all_sessions(app.handle()));
        assert_eq!(sessions.get(&record.id).unwrap().pty_id, Some(pty));
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        assert!(app.state::<SessionActionQueue>().peek_next(&record.id).is_some());
        assert!(ptys.is_running(pty)); assert!(!errors.lock().unwrap().is_empty());
        drop(refusal);
        ptys.kill(pty).unwrap(); sessions.mark_stopped(&record.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_shutdown_rechecks_durable_unverified_runs_without_active_scheduler() {
        let (root, sessions) = lifecycle_fixture();
        let store = MultiAgentStore::open_at(root.join("agents.db")).unwrap();
        let project = store.create_project(CreateProjectInput { name:"isolated stop".into(), goal:"retain frozen run".into(), root_path:root.display().to_string(), default_branch:"main".into() }).unwrap();
        let agent = store.create_agent(serde_json::from_value(serde_json::json!({"projectId":project.id,"name":"worker","runtimeKind":"codex","role":"worker","model":null,"permissionProfile":"default","maxConcurrency":1})).unwrap()).unwrap();
        let task = store.create_task(serde_json::from_value(serde_json::json!({"projectId":project.id,"title":"pending stop","assigneeAgentId":agent.id})).unwrap()).unwrap();
        let run = store.claim_task(multi_agent::ClaimTaskInput { task_id:task.id, agent_id:agent.id, lease_seconds:60 }).unwrap();
        store.mark_run_stop_unverified(&run.id, "fixture refused stop").unwrap();
        let supervisor = MultiAgentSupervisor::default();
        assert!(supervisor.active_project_ids().is_empty());
        let admission = runtime_admission::RuntimeAdmission::default();
        let queue = SessionActionQueue::new();
        queue.enqueue_message("frozen", QueuedMessage::new("retain on each refused exit".into(), None));
        let app = tauri::test::mock_app();
        app.manage(store.clone()); app.manage(supervisor.clone()); app.manage(admission.clone()); app.manage(sessions); app.manage(queue);
        for _ in 0..2 {
            assert!(!shutdown_all_sessions(app.handle()));
            assert!(app.state::<SessionActionQueue>().peek_next("frozen").is_some());
            assert!(admission.admit().is_ok(), "refused shutdown must reopen startup admission");
            drop(supervisor.begin_shutdown().unwrap());
        }
        // Mock runtime retains its manager beyond App::drop; release this
        // fixture's SQLite owner explicitly before checking directory cleanup.
        #[allow(deprecated)]
        drop(app.unmanage::<MultiAgentStore>());
        drop(app); drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_closed_runtime_admission_rejects_legacy_acp_entry_points() {
        let admission = runtime_admission::RuntimeAdmission::default();
        admission.begin_shutdown(std::time::Duration::from_secs(1)).unwrap().commit();
        let manager = AcpManager::default();
        for new_session in [false, true] {
            let error = start_legacy_acp(&manager, &admission, "unused-before-spawn", None, new_session).unwrap_err();
            assert!(error.contains("正在退出"));
        }
        manager.stop().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_real_pty_exit_poll_preserves_trailing_output_marker() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let ptys = PtyManager::new();
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", "process.stdout.write('x'.repeat(1024*1024),()=>process.stdout.write('UNIQUE_COMPLETE_TAIL_MARKER',()=>process.exit(0)));"]);
        let pty = ptys.open_agent_program(&command, &record.id, false).unwrap();
        sessions.attach_runtime(&record.id, pty).unwrap();
        let queue = SessionActionQueue::new(); let automations = AutomationManager::new();
        let mut output = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut chunks = ptys.poll_all();
            process_polled_session_exits(&mut chunks, &ptys, &sessions, &queue, &automations, None).unwrap();
            if let Some(chunk) = chunks.get(&pty) { output.push_str(&chunk.data); }
            if sessions.get(&record.id).unwrap().pty_id.is_none() { break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(output.contains("UNIQUE_COMPLETE_TAIL_MARKER"), "real exit polling lost CLI trailing marker ({} bytes observed)", output.len());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_verified_exit_with_missing_reader_eof_warns_and_settles_binding() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let ptys = PtyManager::new();
        PtyManager::suppress_next_fixture_reader_eof();
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", "process.stdout.write('available fixture output',()=>process.exit(0));"]);
        let pty = ptys.open_agent_program(&command, &record.id, false).unwrap();
        sessions.attach_runtime(&record.id, pty).unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&record.id, QueuedMessage::new("discard after verified exit".into(), None));
        let automations = AutomationManager::new(); let mut output = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut chunks = ptys.poll_all();
            process_polled_session_exits(&mut chunks, &ptys, &sessions, &queue, &automations, None).unwrap();
            if let Some(chunk) = chunks.get(&pty) { output.push_str(&chunk.data); }
            if sessions.get(&record.id).unwrap().pty_id.is_none() { break; }
            assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(output.contains("available fixture output"));
        assert!(output.contains("[输出警告]") && output.contains("尾部输出可能不完整"));
        let final_record = sessions.get(&record.id).unwrap();
        assert!(final_record.pty_id.is_none()); assert_ne!(final_record.status, SessionStatus::Running);
        assert!(ptys.poll_all().is_empty()); assert!(queue.peek_next(&record.id).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn lifecycle_fixture() -> (std::path::PathBuf, SessionManager) {
        let root = std::env::temp_dir().join(format!("lifecycle-guards-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        (root, sessions)
    }

    #[test]
    fn lifecycle_delete_failed_native_launches_without_locators() {
        let (root, sessions) = lifecycle_fixture();
        let app = tauri::test::mock_app();
        let ptys = PtyManager::new();
        let queue = SessionActionQueue::new();
        let deepseek = NativeAcpSessions::open_at(root.join("deepseek.json")).unwrap();
        let claude = NativeClaudeSessions(NativeAcpSessions::open_at_flavor(root.join("claude.json"), NativeChatFlavor::ClaudeChat).unwrap());
        let kimi = NativeKimiSessions(NativeAcpSessions::open_at_flavor(root.join("kimi.json"), NativeChatFlavor::KimiChat).unwrap());
        let codex = NativeCodexChatSessions(NativeAcpSessions::open_at_flavor(root.join("codex.json"), NativeChatFlavor::CodexChat).unwrap());
        let opencode = NativeOpenCodeChatSessions(NativeAcpSessions::open_at_flavor(root.join("opencode.json"), NativeChatFlavor::OpenCodeChat).unwrap());
        let pi = NativePiSessions::open_at(root.join("pi.json")).unwrap();
        let zcode = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        let retained = sessions.create_runtime(NewRuntimeSession {
            runtime_kind: "deepseek_harness".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        deepseek.register(&retained.id, &retained.cwd).unwrap();
        let retained_history = std::fs::read(root.join("deepseek.json")).unwrap();
        let mut failures = Vec::new();
        for kind in ["deepseek_harness", "claude_chat", "kimi_chat", "codex_chat", "opencode_chat", "pi_chat"] {
            // A failed launch can persist the outer record before registration.
            let record = sessions.create_runtime(NewRuntimeSession {
                runtime_kind: kind.into(), cwd: root.display().to_string(), project_id: None,
            }).unwrap();
            let result = session_switch::with_isolated_switches(&root, || {
                delete_session_inner(app.handle(), &record.id, &ptys, &sessions, &queue, None,
                    &deepseek, &zcode, &claude, &kimi, &codex, &pi, &opencode)
            });
            if let Err(error) = result { failures.push(format!("{kind}: {error}")); }
            else { assert!(sessions.get(&record.id).is_none(), "{kind}"); }
        }
        assert!(failures.is_empty(), "{}", failures.join("; "));
        assert!(sessions.get(&retained.id).is_some());
        assert!(deepseek.contains(&retained.id));
        assert_eq!(std::fs::read(root.join("deepseek.json")).unwrap(), retained_history);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_automation_direct_target_rechecks_retired_and_archived_at_creation_and_execution() {
        let (root, sessions) = lifecycle_fixture();
        let store = MultiAgentStore::open_at(root.join("agents.db")).unwrap();
        let automations = AutomationManager::new();
        let queue = SessionActionQueue::new();
        for retired in [false, true] {
            let record = if retired {
                sessions.create_agent(sessions::NewAgentSession { avatar_id: None, project_id:"fixture-project".into(), agent_id:"fixture-agent".into(), runtime_kind:"codex".into(), agent_name:"fixture".into(), cwd:root.display().to_string() }).unwrap()
            } else { sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:None }).unwrap() };
            let action = AutomationAction::Message { session_id:record.id.clone(), body:"must not enqueue".into(), project_id:None, agent_id:None };
            // Simulate a definition created before its target became history.
            let input = NewAutomation { name:"old direct target".into(), trigger:automation::AutomationTrigger::Once { at:1 }, action:action.clone(), max_runs:None, max_consecutive_failures:Some(1), overlap_policy: automation::OverlapPolicy::Allow };
            let legacy = automations.create(input.clone(), automation::now_millis()).unwrap();
            if retired { sessions.retire_agent_runtime_sessions("fixture-project", "fixture-agent").unwrap(); }
            else { sessions.archive(&record.id).unwrap(); }
            assert!(create_automation_inner(input.clone(), &sessions, &store, &automations).is_err());
            let before_edit = serde_json::to_value(automations.list()).unwrap();
            assert!(save_automation_inner(Some(&legacy.id), input, &sessions, &store, &automations).is_err());
            assert_eq!(serde_json::to_value(automations.list()).unwrap(), before_edit);
            dispatch_due_automations(&PtyManager::new(), &sessions, &queue, &automations, &store).unwrap();
            assert!(queue.peek_next(&record.id).is_none());
            let task = automations.list().into_iter().find(|task| task.id == legacy.id).unwrap();
            assert_eq!(task.run_count, 1);
            assert_eq!(task.consecutive_failures, 1);
            assert!(!task.enabled);
            assert_eq!(task.recent_runs.len(), 1);
            assert!(!task.recent_runs[0].ok);
            assert!(task.recent_runs[0].detail.contains(if retired {"退役"} else {"归档"}));
        }
        let (ok, detail) = execute_automation_action(&AutomationAction::Message { session_id:"不存在的会话标识".into(), body:"text".into(), project_id:None, agent_id:None }, &PtyManager::new(), &sessions, &queue, None);
        assert!(!ok); assert!(detail.contains("目标会话不存在"));
        drop(store); drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_automation_scopes_delivery_and_tracks_busy_queue_without_running_agents() {
        let (root, sessions) = lifecycle_fixture();
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let ptys = PtyManager::new();
        let mut records = Vec::new();
        for kind in ["codex_chat", "claude_chat", "pi_chat"] {
            let record = sessions.create_runtime(NewRuntimeSession { runtime_kind: kind.into(), cwd:root.display().to_string(), project_id:None }).unwrap();
            sessions.mark_native_runtime_status(&record.id, SessionStatus::Running, None, None).unwrap();
            records.push(record);
        }
        let action = AutomationAction::BroadcastAgent { runtime_kind:AgentRuntimeKind::CodexChat, body:"exact message".into() };
        let mut targets=Vec::new();
        let (ok, detail) = execute_automation_action_tracked(&action, &ptys, &sessions, &queue, None, &mut targets, None);
        assert!(ok, "{detail}"); assert_eq!(targets, vec![records[0].id.clone()]);
        assert_eq!(queue.peek_next(&records[0].id).unwrap().body, "exact message");
        assert!(queue.peek_next(&records[1].id).is_none() && queue.peek_next(&records[2].id).is_none());
        let manager = AutomationManager::new();
        let task = manager.create(NewAutomation {name:"fixture".into(),trigger:automation::AutomationTrigger::Interval {minutes:1},action,max_runs:None,max_consecutive_failures:Some(3),overlap_policy:automation::OverlapPolicy::Skip}, 1).unwrap();
        manager.report_dispatch_result(&task.id, true, detail, 2, targets).unwrap();
        assert!(busy_automation_ids(&manager, &sessions, &queue, None).contains(&task.id));
        let wrong = AutomationAction::SendAgent { runtime_kind:AgentRuntimeKind::ClaudeChat,session_id:records[0].id.clone(),body:"wrong".into(),project_id:None,agent_id:None };
        assert!(!execute_automation_action(&wrong, &ptys, &sessions, &queue, None).0);
        sessions.mark_stopped(&records[0].id).unwrap();
        let stopped = AutomationAction::SendAgent { runtime_kind:AgentRuntimeKind::CodexChat,session_id:records[0].id.clone(),body:"stopped".into(),project_id:None,agent_id:None };
        let (ok,detail) = execute_automation_action(&stopped, &ptys, &sessions, &queue, None);
        assert!(!ok && detail.contains("恢复"), "{detail}");
        assert_eq!(queue.peek_next(&records[0].id).unwrap().body, "exact message");
        drop(queue); drop(sessions);
        let reopened = SessionManager::open_at(root.join("sessions.json"),root.join("events")).unwrap();
        assert!(reopened.get(&records[0].id).is_some());
        drop(reopened); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_automation_native_project_members_use_scheduler_bridge_instead_of_pty() {
        let (root, sessions) = lifecycle_fixture();
        let store = MultiAgentStore::open_at(root.join("project.db")).unwrap();
        let project = store.create_project(CreateProjectInput { name:"automation fixture".into(),goal:"queue only".into(),root_path:root.display().to_string(),default_branch:"main".into() }).unwrap();
        let queue=SessionActionQueue::new();
        let ptys=PtyManager::new();
        for kind in [AgentRuntimeKind::DeepSeekHarness, AgentRuntimeKind::ZCode, AgentRuntimeKind::Codex, AgentRuntimeKind::Pi] {
            let agent = store.create_agent(CreateAgentInput {avatar_id:None,project_id:project.id.clone(),name:kind.as_str().into(),runtime_kind:kind,role:multi_agent::AgentRole::Worker,description:String::new(),persona:Default::default(),persona_custom:String::new(),model:None,permission_profile:multi_agent::PermissionProfile::Default,max_concurrency:1}).unwrap();
            let action = AutomationAction::SendAgent { runtime_kind:kind, session_id:String::new(),body:"native project task".into(),project_id:Some(project.id.clone()),agent_id:Some(agent.id) };
            let mut targets=Vec::new();
            let (ok,detail) = execute_automation_action_tracked(&action,&ptys,&sessions,&queue,Some(&store),&mut targets,None);
            assert!(ok,"{detail}"); assert_eq!(targets.len(),1);
            let target=sessions.get(&targets[0]).unwrap();
            assert!(target.runtime_command_host && target.pty_id.is_none());
            assert_eq!(queue.peek_next(&target.id).unwrap().body,"native project task");
        }
        assert!(ptys.poll_all().is_empty());
        drop(store); drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automation_message_reports_disk_failure_without_publishing_a_delivery() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind: "codex".into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        let mailbox = root.join("mailbox.json");
        let queue = SessionActionQueue::open_at(mailbox.clone(), root.join("outbox")).unwrap();
        std::fs::create_dir(&mailbox).unwrap();
        let action = AutomationAction::Message { session_id: record.id.clone(), body: "test".into(), project_id: None, agent_id: None };
        let (ok, detail) = execute_automation_action(&action, &PtyManager::new(), &sessions, &queue, None);
        assert!(!ok && detail.contains("持久化失败"), "{detail}");
        assert!(queue.peek_next(&record.id).is_none());
        std::fs::remove_dir(&mailbox).unwrap();
        assert!(execute_automation_action(&action, &PtyManager::new(), &sessions, &queue, None).0);
        drop(queue);
        let reopened = SessionActionQueue::open_at(mailbox, root.join("outbox")).unwrap();
        assert_eq!(reopened.peek_next(&record.id).unwrap().body, "test");
        drop(reopened); drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_delivery_keeps_success_receipt_when_history_projection_fails() {
        let (root, sessions) = lifecycle_fixture();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let project = store.create_project(CreateProjectInput { name: "receipt failure fixture".into(), goal: "preserve accepted delivery".into(), root_path: root.display().to_string(), default_branch: "main".into() }).unwrap();
        let agent = store.create_agent(CreateAgentInput {
            project_id: project.id.clone(), name: "receiver".into(), runtime_kind: AgentRuntimeKind::Pi,
            role: multi_agent::AgentRole::Worker, description: String::new(), avatar_id: None,
            persona: Default::default(), persona_custom: String::new(), model: None,
            permission_profile: multi_agent::PermissionProfile::Default, max_concurrency: 1,
        }).unwrap();
        let record = sessions.create_agent(sessions::NewAgentSession { project_id: project.id.clone(), agent_id: agent.id,
            runtime_kind: "pi".into(), agent_name: "receiver".into(), cwd: root.display().to_string(), avatar_id: None }).unwrap();
        let db = rusqlite::Connection::open(root.join("control.db")).unwrap();
        db.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON ma_messages WHEN NEW.sender_type = 'coordinator' BEGIN SELECT RAISE(FAIL, 'fixture history disk failure'); END;").unwrap();
        let app = tauri::test::mock_app();
        app.manage(store); app.manage(sessions); app.manage(PtyManager::new());
        app.manage(SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap());
        let receipt = send_direct_collaboration_message(DirectCollaborationInput {
            message: "仅通知收件成员".into(), project_id: Some(project.id), target_agent_ids: Vec::new(), target_session_ids: vec![record.id.clone()], target_runtime_kinds: None,
        }, app.state(), app.state(), app.state(), app.state()).unwrap().unwrap();
        assert!(receipt.reply.contains("消息已入队") && receipt.reply.contains("历史保存失败") && receipt.reply.contains("请勿重复发送"), "{}", receipt.reply);
        let queue = app.state::<SessionActionQueue>();
        let queued = queue.take_next(&record.id).unwrap();
        assert_eq!(receipt.delivery_task_ids, vec![format!("cli-message-{}", queued.id)]);
        assert!(queue.take_next(&record.id).is_none(), "history failure must not duplicate the message");
        drop(queue); drop(app.unmanage::<MultiAgentStore>()); drop(app); drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_zcode_ui_wrapper_releases_lifecycle_while_waiting_for_native_queue_ack() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"zcode".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let mut manager = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        manager.test_peer = Some((std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-zcode-agent.mjs"), "queue_slow".into()));
        manager.register(&record.id, &record.cwd).unwrap();
        let app = tauri::test::mock_app();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        session_switch::with_isolated_switches(&root, || send_zcode_message_blocking(&sessions, &manager, &record.id, "current", &[], false)).unwrap();
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| session_switch::with_isolated_switches(&root, || send_zcode_message_blocking(&sessions, &manager, &record.id, "delayed", &[], false)));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !manager.snapshot(&record.id).unwrap().recoverable_inputs.iter().any(|input| input["text"] == "delayed") {
                assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(!pending.is_finished());
            let started = std::time::Instant::now();
            stop_zcode_session_blocking(app.handle(), &record.id, &sessions, &manager).unwrap();
            assert!(started.elapsed() < std::time::Duration::from_secs(2), "UI stop must not wait for the native ACK lifecycle lock");
            let error = pending.join().unwrap().unwrap_err();
            assert!(error.to_string().starts_with("ZCODE_INPUT_UNCERTAIN:"), "{error:#}");
        });
        let recovered = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap().snapshot(&record.id).unwrap().recoverable_inputs;
        assert_eq!(recovered.len(), 1); assert_eq!(recovered[0]["text"], "delayed");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_zcode_ui_admission_fences_delete_or_handoff_until_submission_is_admitted() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind:"zcode".into(), cwd:root.display().to_string(), project_id:None }).unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let (root_ref, sessions_ref, id) = (&root, &sessions, &record.id);
            let admitting = scope.spawn(move || session_switch::with_isolated_switches(root_ref, || zcode_input_admission(sessions_ref, id, || {
                entered_tx.send(()).unwrap(); release_rx.recv().unwrap();
                Err(anyhow::anyhow!("fixture admission refused before any send"))
            })));
            entered_rx.recv().unwrap();
            let lifecycle = sessions.lifecycle_lock(&record.id);
            assert!(lifecycle.try_lock().is_err(), "delete/handoff must not cross admission");
            release_tx.send(()).unwrap();
            assert!(admitting.join().unwrap().is_err());
            assert!(lifecycle.try_lock().is_ok());
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_runtime_location_rejects_unknown_project_and_other_directory() {
        let root = std::env::temp_dir().join(format!("crewvela-project-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let project = projects::Project { id: "project".into(), name: "Project".into(),
            path: root.join("project").display().to_string(), badge_color: "blue".into(), created_at: 0, updated_at: 0,
            archived_at: None, last_opened_at: None, launch: None, collaboration_project_ids: Vec::new() };
        assert!(runtime_project_cwd(Some(project.path.clone()), Some("unknown"), |_| Ok(None)).is_err());
        assert!(runtime_project_cwd(Some(root.join("other").display().to_string()), Some("project"), |_| Ok(Some(project.clone()))).is_err());
        assert_eq!(runtime_project_cwd(None, Some("project"), |_| Ok(Some(project.clone()))).unwrap(), project.path);
        assert_eq!(runtime_project_cwd(Some(root.join("project/.").display().to_string()), Some("project"), |_| Ok(Some(project.clone()))).unwrap(), root.join("project/.").display().to_string());
        let cwd = root.join("other").display().to_string();
        assert_eq!(runtime_project_cwd(Some(cwd.clone()), None, |_| panic!("legacy global creation must not look up a project")).unwrap(), cwd);
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let legacy = sessions.create_runtime(NewRuntimeSession { runtime_kind: "codex".into(), cwd, project_id: None }).unwrap();
        assert!(legacy.project_id.is_none());
        for runtime in ["codex", "kimi_code", "pi", "opencode", "deepseek_harness"] {
            let record = sessions.create_runtime(NewRuntimeSession { runtime_kind: runtime.into(), cwd: project.path.clone(), project_id: Some(project.id.clone()) }).unwrap();
            assert_eq!(record.project_id.as_deref(), Some("project"));
            assert_eq!(record.cwd, project.path);
            assert_eq!(record.agent_runtime_kind.as_deref(), Some(runtime));
        }
        std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn runtime_switch_rejects_busy_native_terminal_without_an_active_task() {
        let root =
            std::env::temp_dir().join(format!("crewvela-runtime-busy-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "member".into(),
                runtime_kind: "pi".into(),
                agent_name: "Pi".into(),
                cwd: ".".into(),
            })
            .unwrap();
        sessions.attach_runtime(&record.id, 123).unwrap();
        assert!(ensure_member_terminal_idle(&sessions, "member").is_err());
        sessions.mark_runtime_prompt_ready(&record.id);
        assert!(ensure_member_terminal_idle(&sessions, "member").is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_plan_backfill_skips_messages_already_recorded() {
        let recorded = std::collections::HashSet::from([
            ("user".to_string(), "你好".to_string()),
            ("coordinator".to_string(), "计划如下".to_string()),
        ]);
        let conversation = vec![
            multi_agent::CoordinatorHistoryMessage {
                role: multi_agent::CoordinatorMessageRole::User,
                content: "你好".to_string(),
            },
            multi_agent::CoordinatorHistoryMessage {
                role: multi_agent::CoordinatorMessageRole::Assistant,
                content: "计划如下".to_string(),
            },
            multi_agent::CoordinatorHistoryMessage {
                role: multi_agent::CoordinatorMessageRole::Assistant,
                content: "补充说明".to_string(),
            },
        ];
        let writes = conversation_writes_to_record(&recorded, &conversation);
        assert_eq!(writes, vec![("coordinator", "补充说明".to_string())]);
    }

    #[test]
    fn native_and_member_automation_events_dispatch_without_any_pty() {
        let root = std::env::temp_dir().join(format!("cc-native-automation-events-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let store = MultiAgentStore::open_at(root.join("multi-agent.db")).unwrap();
        let native = sessions.create_runtime(sessions::NewRuntimeSession {
            runtime_kind: "codex_chat".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        let member = sessions.create_agent(sessions::NewAgentSession {
            avatar_id: None, project_id: "project".into(), agent_id: "member".into(),
            runtime_kind: "pi".into(), agent_name: "Pi member".into(), cwd: ".".into(),
        }).unwrap();
        assert!(sessions.list().iter().all(|record| record.pty_id.is_none()));
        let automations = AutomationManager::open_at(root.join("automations.json")).unwrap();
        let completed = automations.create(NewAutomation {
            name: "完成事件".into(),
            trigger: automation::AutomationTrigger::OnSessionEvent { event: SessionEventKind::Completed },
            action: AutomationAction::Broadcast { body: "done".into() }, max_runs: None, max_consecutive_failures: Some(10), overlap_policy: automation::OverlapPolicy::Allow,
        }, automation::now_millis()).unwrap();
        let failed = automations.create(NewAutomation {
            name: "失败事件".into(),
            trigger: automation::AutomationTrigger::OnSessionEvent { event: SessionEventKind::Failed },
            action: AutomationAction::Broadcast { body: "failed".into() }, max_runs: None, max_consecutive_failures: Some(10), overlap_policy: automation::OverlapPolicy::Allow,
        }, automation::now_millis()).unwrap();
        let ptys = PtyManager::new();
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let tick = || dispatch_due_automations(&ptys, &sessions, &queue, &automations, &store).unwrap();
        let count = |id: &str| automations.list().into_iter().find(|item| item.id == id).unwrap().run_count;
        sessions.mark_native_runtime_status(&native.id, SessionStatus::Running, None, None).unwrap();
        sessions.record_native_turn_outcome(&native.id, "turn-1", SessionStatus::Completed).unwrap();
        tick();
        assert_eq!(count(&completed.id), 1, "native completion must dispatch on heartbeat with no PTY");
        assert_eq!(sessions.get(&native.id).unwrap().status, SessionStatus::Running, "completion must preserve live connection status");
        sessions.record_native_turn_outcome(&native.id, "turn-1", SessionStatus::Completed).unwrap();
        tick();
        assert_eq!(count(&completed.id), 1, "repeated status must not re-trigger");
        sessions.mark_agent_activity(&member.id, SessionStatus::Running, None).unwrap();
        sessions.mark_agent_activity(&member.id, SessionStatus::Completed, None).unwrap();
        tick();
        assert_eq!(count(&completed.id), 2, "member completion must dispatch");
        sessions.mark_agent_activity(&member.id, SessionStatus::Error, Some("failed".into())).unwrap();
        tick();
        assert_eq!(count(&failed.id), 1);
        sessions.mark_native_runtime_status(&native.id, SessionStatus::Running, None, None).unwrap();
        sessions.record_native_turn_outcome(&native.id, "cancelled-turn", SessionStatus::Stopped).unwrap();
        sessions.record_native_turn_outcome(&native.id, "cancelled-turn", SessionStatus::Completed).unwrap();
        sessions.mark_native_runtime_status(&native.id, SessionStatus::Stopped, None, None).unwrap();
        sessions.mark_agent_activity(&member.id, SessionStatus::Stopped, None).unwrap();
        tick();
        assert_eq!(count(&completed.id), 2, "cancellation must not trigger completion");
        assert_eq!(count(&failed.id), 1, "cancellation must not trigger failure");
        sessions.mark_native_runtime_status(&native.id, SessionStatus::Running, None, None).unwrap();
        sessions.mark_native_runtime_status(&native.id, SessionStatus::Error, None, Some("failed".into())).unwrap();
        tick();
        assert_eq!(count(&failed.id), 2, "native failures must dispatch");
        assert!(sessions.drain_transitions().is_empty());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn final_session_exit_triggers_failed_automation_in_same_poll() {
        let root =
            std::env::temp_dir().join(format!("crewvela-final-exit-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = sessions
            .create_runtime(sessions::NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        let automations = AutomationManager::new();
        let task = automations
            .create(
                NewAutomation {
                    name: "退出提醒".into(),
                    trigger: automation::AutomationTrigger::OnSessionEvent {
                        event: SessionEventKind::Failed,
                    },
                    action: AutomationAction::Broadcast {
                        body: "已失败".into(),
                    },
                    max_runs: None,
                    max_consecutive_failures: None, overlap_policy: automation::OverlapPolicy::Allow,
                },
                automation::now_millis(),
            )
            .unwrap();

        let ptys = PtyManager::new();
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", "process.exit(1)"]);
        let pty_id = ptys.open_agent_program(&command, &session.id, false).unwrap();
        sessions.attach_runtime(&session.id, pty_id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut chunks = loop {
            let mut chunks = ptys.poll_all();
            if chunks.get(&pty_id).is_some_and(|chunk| chunk.code == Some(1)) { break chunks; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        process_polled_session_exits(&mut chunks, &ptys, &sessions, &SessionActionQueue::new(), &automations, None).unwrap();
        let fired = automations
            .list()
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap();
        assert_eq!(fired.run_count, 1);
        assert_eq!(fired.recent_runs.len(), 1);
        assert!(sessions.drain_transitions().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_event_save_keeps_transition_for_retry() {
        let root =
            std::env::temp_dir().join(format!("crewvela-event-retry-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = sessions
            .create_runtime(sessions::NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        let path = root.join("automations.json");
        let automations = AutomationManager::open_at(path.clone()).unwrap();
        let task = automations
            .create(
                NewAutomation {
                    name: "失败重试".into(),
                    trigger: automation::AutomationTrigger::OnSessionEvent {
                        event: SessionEventKind::Failed,
                    },
                    action: AutomationAction::Broadcast {
                        body: "失败".into(),
                    },
                    max_runs: None,
                    max_consecutive_failures: None, overlap_policy: automation::OverlapPolicy::Allow,
                },
                automation::now_millis(),
            )
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        sessions.mark_exit(&session.id, 1).unwrap();
        assert!(dispatch_session_event_automations(
            &PtyManager::new(),
            &sessions,
            &SessionActionQueue::new(),
            &automations,
            None
        )
        .is_err());
        std::fs::remove_dir(&path).unwrap();
        dispatch_session_event_automations(
            &PtyManager::new(),
            &sessions,
            &SessionActionQueue::new(),
            &automations,
            None,
        )
        .unwrap();
        assert_eq!(
            automations
                .list()
                .into_iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .run_count,
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_hook_readiness_survives_terminal_redraws() {
        let root =
            std::env::temp_dir().join(format!("cc-claude-readiness-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "claude-agent".into(),
                runtime_kind: "claude_code".into(),
                agent_name: "Claude 架构师".into(),
                cwd: ".".into(),
            })
            .unwrap();
        sessions.attach_runtime(&session.id, 7).unwrap();
        let hook = |name: &str| {
            sessions
                .apply_hook_value(&serde_json::json!({
                    "session_id": session.claude_session_id,
                    "_crewvela_app_session_id": session.id,
                    "hook_event_name": name,
                }))
                .unwrap()
        };
        let redraw = |data: &str| {
            mark_interactive_runtime_prompts(
                &std::collections::HashMap::from([(
                    7,
                    pty::PtyChunk {
                        app_session_id: session.id.clone(),
                        data: data.into(),
                        running: true,
                        code: None,
                    },
                )]),
                &sessions,
            )
        };

        hook("SessionStart");
        assert!(sessions.is_ready_for_prompt(&session.id));
        redraw("Claude Code v2.1.285\n> \nbypass permissions on");
        assert!(
            sessions.is_ready_for_prompt(&session.id),
            "启动重绘不能撤销 Hook 就绪状态"
        );
        sessions.mark_prompt_dispatched(&session.id);
        redraw("> ");
        assert!(
            !sessions.is_ready_for_prompt(&session.id),
            "投递后不能因旧输入框再次就绪"
        );
        hook("UserPromptSubmit");
        redraw("Thinking...\nesc to interrupt");
        assert!(!sessions.is_ready_for_prompt(&session.id));
        hook("Stop");
        redraw("Worked for 2s\n> ");
        assert!(
            sessions.is_ready_for_prompt(&session.id),
            "完成重绘不能撤销 Hook 就绪状态"
        );
        sessions
            .apply_hook_value(&serde_json::json!({
                "session_id": session.claude_session_id,
                "_crewvela_app_session_id": session.id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
            }))
            .unwrap();
        redraw("Allow once\n> ");
        assert!(
            !sessions.is_ready_for_prompt(&session.id),
            "审批等待仍由 Hook 阻止投递"
        );
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_runtime_communication_preserves_permissions_and_rejects_retired_cli() {
        let root = std::env::temp_dir().join(format!("cc-runtime-hooks-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        for kind in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex] {
            let record = sessions
                .create_runtime(sessions::NewRuntimeSession {
                    runtime_kind: kind.as_str().into(),
                    cwd: ".".into(),
                    project_id: None,
                })
                .unwrap();
            let mut command = Command::new("runtime");
            configure_runtime_communication(kind, &mut command, &record, &sessions).unwrap();
            let args = command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args
                .iter()
                .any(|value| value.contains("CREWVELA_SESSION_ID")));
            assert!(!args
                .iter()
                .any(|value| value.contains("bypass") || value.contains("skip-permissions")));
            if kind == AgentRuntimeKind::ClaudeCode {
                assert!(args.contains(&"--settings".into()));
                let settings: serde_json::Value =
                    serde_json::from_str(args.last().unwrap()).unwrap();
                assert!(settings["hooks"]["SessionStart"].is_array());
                assert!(settings["hooks"]["Stop"].is_array());
                assert_eq!(settings["env"]["CREWVELA_SESSION_ID"], record.id);
                // Local Claude launch must leave account, endpoint and model
                // selection to the CLI's own settings/environment.
                assert_eq!(record.provider_id, "local-runtime-claude_code");
                assert!(!settings.as_object().unwrap().contains_key("model"));
                assert!(settings["env"].as_object().unwrap().keys().all(|key| key.starts_with("CREWVELA_")));
                assert!(!args.iter().any(|arg| arg == "--model" || arg == "--setting-sources"));
                assert!(!command.get_envs().any(|(key, _)| key.to_string_lossy().starts_with("ANTHROPIC_")));
            }
        }
        assert!(sessions.create_runtime(sessions::NewRuntimeSession {
            runtime_kind: "minimax_code".into(), cwd: ".".into(), project_id: None,
        }).unwrap_err().to_string().contains("已移除"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "launches installed ZCode CLI and executes only local /help; no model request"]
    fn real_zcode_automation_executes_local_help_after_startup() {
        let root = std::env::temp_dir().join(format!("crewvela-zcode-automation-live-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let ptys = PtyManager::new();
        let action = AutomationAction::RunAgent { runtime_kind: AgentRuntimeKind::ZCodeCli, provider_id: None, model: None, cwd: root.display().to_string(), prompt: "/help".into() };
        let (ok, _) = execute_automation_action(&action, &ptys, &sessions, &queue, None);
        assert!(ok, "native ZCode automation startup failed");
        let record = sessions.list().into_iter().next().unwrap();
        let id = record.pty_id.unwrap();
        ptys.resize(id, 50, 180).unwrap();
        sessions.resize_runtime_prompt(&record.id, 50, 180);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(50);
        let mut received_help = false;
        let mut received_followup = false;
        let mut was_ready = false;
        while ptys.is_running(id) && std::time::Instant::now() < deadline {
            let chunks = ptys.poll_all();
            mark_interactive_runtime_prompts(&chunks, &sessions);
            was_ready |= sessions.is_ready_for_prompt(&record.id);
            queue.flush_ready(&sessions, &ptys);
            queue.flush_pending_submits(&sessions, &ptys);
            let output = multi_agent::strip_terminal_control_sequences(&String::from_utf8_lossy(&ptys.transcript_tail(id, 256 * 1024).unwrap()));
            // This text is returned by the CLI's local slash-command handler,
            // so seeing it proves more than writing Enter or draining a queue.
            if !received_help && output.contains("for details.") && output.contains("/help") && !queue.has_pending_work(&record.id) {
                received_help = true;
                queue.enqueue_messages_durable(vec![(record.id.clone(), QueuedMessage::new("/help crewvela-local-probe".into(), None))]).unwrap();
            }
            if received_help && output.contains("Unknown slash command: /crewvela-local-probe.") && !queue.has_pending_work(&record.id) {
                received_followup = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        eprintln!("ZCode native verification: ready={was_ready}, help_response={received_help}, followup_response={received_followup}, pending={}", queue.has_pending_work(&record.id));
        ptys.kill_all().unwrap();
        drop(ptys); drop(queue); drop(sessions);
        // ConPTY's reader can briefly retain the working-directory handle.
        let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match std::fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(_) if std::time::Instant::now() < cleanup_deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(error) => panic!("native test cleanup failed: {error}"),
            }
        }
        assert!(received_help && received_followup, "ZCode automation must return both local command responses in the same session");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "launches installed Codex with real configuration; does not send a model prompt"]
    fn real_codex_startup_becomes_ready_after_update_skip() {
        let root = std::env::temp_dir().join(format!("crewvela-codex-ready-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(sessions::NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let ptys = PtyManager::new();
        let started = open_runtime_session(&record, &ptys, &sessions).unwrap();
        let id = started.pty_id.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        let mut skipped = false;
        while ptys.is_running(id) && std::time::Instant::now() < deadline {
            mark_interactive_runtime_prompts(&ptys.poll_all(), &sessions);
            let output = String::from_utf8_lossy(&ptys.transcript_tail(id, 128 * 1024).unwrap()).into_owned();
            if !skipped && output.contains("Update now") && output.contains("Skip") {
                ptys.write(id, b"\x1b").unwrap();
                skipped = true;
            }
            if sessions.is_ready_for_prompt(&record.id) { break; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let output = String::from_utf8_lossy(&ptys.transcript_tail(id, 128 * 1024).unwrap()).into_owned();
        let ready = sessions.is_ready_for_prompt(&record.id);
        let blocked = sessions.runtime_prompt_is_blocked(&record.id);
        ptys.kill(id).unwrap();
        let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.scratch/codex-delivery-hook");
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::write(evidence.join("codex-startup-raw.log"), &output).unwrap();
        assert!(ready, "installed Codex composer never became input-ready; update_skipped={skipped}, blocked={blocked}; see .scratch/codex-delivery-hook/codex-startup-raw.log");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "launches installed interactive Codex locally, without sending a prompt"]
    fn real_codex_interactive_startup_reaches_tui_without_exit() {
        let root = std::env::temp_dir().join(format!("crewvela-codex-startup-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let cwd = std::env::var("CREWVELA_STARTUP_TEST_CWD").unwrap_or_else(|_| root.display().to_string());
        let record = sessions.create_runtime(sessions::NewRuntimeSession {
            runtime_kind: "codex".into(), cwd, project_id: None,
        }).unwrap();
        let ptys = PtyManager::new();
        let started = open_runtime_session(&record, &ptys, &sessions).unwrap();
        let id = started.pty_id.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let stable_after = std::time::Instant::now() + std::time::Duration::from_secs(6);
        let mut output = String::new();
        loop {
            output = String::from_utf8_lossy(&ptys.transcript_tail(id, 64 * 1024).unwrap()).into_owned();
            if !ptys.is_running(id) || (std::time::Instant::now() >= stable_after
                && (output.contains("OpenAI Codex") || output.contains("Do you trust"))) {
                break;
            }
            if std::time::Instant::now() >= deadline { break; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let running = ptys.is_running(id);
        ptys.kill(id).unwrap();
        assert!(running && (output.contains("OpenAI Codex") || output.contains("Do you trust")),
            "interactive Codex exited or never reached TUI: {output}");
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_communication_group_instruction_identifies_self_and_only_lists_peers() {
        let (root, sessions) = lifecycle_fixture();
        let make = |runtime: &str| sessions.create_runtime(NewRuntimeSession {
            runtime_kind: runtime.into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let claude = make("claude_code"); let codex = make("codex");
        let targets = vec![claude.clone(), codex.clone()];
        for (current, peer) in [(&claude, &codex), (&codex, &claude)] {
            let body = direct_collaboration_delivery("@ClaudeCode @Codex 你两互相打个招呼", &targets, current, None);
            assert!(body.contains(&format!("当前会话（你）：{}（完整会话 ID：{}）", current.title, current.id)));
            let roster = body.split("其他参与会话：\n").nth(1).unwrap().split("\n\n用户要求").next().unwrap();
            assert!(roster.contains(&peer.id)); assert!(!roster.contains(&current.id));
            assert!(body.contains("crewvela_sessions MCP") && body.contains("无需读取 Skill 或执行 shell"));
        }
        let directed = direct_collaboration_delivery("让claude给codex打招呼", &targets, &claude, Some((&claude.id, &codex.id)));
        assert!(directed.contains(&format!("只给接收者（完整会话 ID：{}）", codex.id)));
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_collaboration_prefers_project_sessions_and_falls_back_by_runtime() {
        let root = std::env::temp_dir().join(format!("crewvela-direct-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let project_claude = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project-a".into(),
                agent_id: "claude-a".into(),
                runtime_kind: "claude_code".into(),
                agent_name: "Claude A".into(),
                cwd: ".".into(),
            })
            .unwrap();
        let duplicate_project_claude = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project-a".into(),
                agent_id: "claude-reviewer".into(),
                runtime_kind: "claude_code".into(),
                agent_name: "Claude Reviewer".into(),
                cwd: ".".into(),
            })
            .unwrap();
        let other_claude = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project-b".into(),
                agent_id: "claude-b".into(),
                runtime_kind: "claude_code".into(),
                agent_name: "Claude B".into(),
                cwd: ".".into(),
            })
            .unwrap();
        let codex = sessions
            .create_runtime(sessions::NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        sessions.attach_runtime(&other_claude.id, 2).unwrap();
        sessions.attach_runtime(&codex.id, 3).unwrap();

        let targets = resolve_direct_collaboration_sessions(
            &sessions.list(),
            Some("project-a"),
            &[AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex],
            &["claude-a".into()],
            &[],
        )
        .unwrap();
        let ids = targets
            .iter()
            .map(|session| session.id.as_str())
            .collect::<HashSet<_>>();
        assert!(ids.contains(project_claude.id.as_str()));
        assert!(ids.contains(codex.id.as_str()));
        assert!(!ids.contains(duplicate_project_claude.id.as_str()));
        assert!(!ids.contains(other_claude.id.as_str()));
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .find(|session| session.id == project_claude.id)
            .unwrap()
            .pty_id
            .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn colloquial_three_member_chat_reuses_exact_roster_and_broadcasts_to_each() {
        let (root, sessions) = lifecycle_fixture();
        let create = |runtime: &str, agent: &str, name: &str| {
            sessions.create_agent(sessions::NewAgentSession {
                avatar_id: None, project_id: "project-a".into(), agent_id: agent.into(),
                runtime_kind: runtime.into(), agent_name: name.into(), cwd: root.display().to_string(),
            }).unwrap()
        };
        let claude = create("claude_code", "claude", "模型验收-Claude");
        let codex = create("codex", "codex", "模型验收-Codex");
        let zcode = create("zcode_cli", "zcode", "聊天-ZCodeCLI");
        let reviewer = create("codex", "reviewer", "语义复核员");
        for (message, exact_members) in [
            ("@ClaudeCode @Codex @ZCodeCLI 你们三个聊下", false),
            ("@模型验收-Claude @模型验收-Codex @聊天-ZCodeCLI 你三聊个天", true),
        ] {
            let request = direct_message_request(message).expect("chat must bypass the engineering planner");
            let explicit = [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex, AgentRuntimeKind::ZCodeCli];
            let runtimes = direct_collaboration_runtimes(message, Some(&request), exact_members, (!exact_members).then_some(explicit.as_slice()));
            let ids = if exact_members { vec!["claude".into(), "codex".into(), "zcode".into()] } else { Vec::new() };
            let targets = resolve_direct_collaboration_sessions(&[claude.clone(), codex.clone(), zcode.clone(), reviewer.clone()], Some("project-a"), &runtimes, &ids, &[]).unwrap();
            // Runtime groups can contain several matching members; exact mentions must not expand.
            if exact_members { assert_eq!(targets.len(), 3); assert!(!targets.iter().any(|s| s.id == reviewer.id)); }
            let direction = resolve_direct_message_direction(Some(&request), &targets).unwrap();
            assert_eq!(direction, None);
            let recipients = direct_message_execution_sessions(&targets, direction);
            assert_eq!(recipients.len(), targets.len());
            for current in recipients {
                let delivery = direct_collaboration_delivery(message, &targets, current, direction);
                assert!(delivery.contains(message));
                for other in targets.iter().filter(|s| s.id != current.id) {
                    assert!(delivery.contains(&format!("完整会话 ID：{}", other.id)));
                }
            }
        }
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zcode_cli_mentions_keep_explicit_runtime_identity_and_reuse_live_cli() {
        let message = "@ZCodeCLI @Codex zcode和codex打个招呼";
        let request = direct_message_request(message).unwrap();
        let runtimes = [AgentRuntimeKind::ZCodeCli, AgentRuntimeKind::Codex];
        assert_eq!(direct_collaboration_runtimes(message, Some(&request), false, Some(&runtimes)), runtimes);
        let legacy = direct_collaboration_runtimes(message, Some(&request), false, None);
        assert_eq!(legacy.len(), 2);
        assert!(legacy.contains(&AgentRuntimeKind::ZCodeCli));
        assert!(legacy.contains(&AgentRuntimeKind::Codex));
        assert!(!legacy.contains(&AgentRuntimeKind::ZCode));
        // Explicit instances (including @All) never expand from runtime names in the body.
        assert!(direct_collaboration_runtimes(message, Some(&request), true, None).is_empty());
        assert!(direct_collaboration_runtimes(message, Some(&request), true, Some(&[])).is_empty());
        assert_eq!(direct_collaboration_runtimes("@ZCode @ClaudeCode 打个招呼", Some(&request), true, Some(&runtimes)), runtimes);

        let root = std::env::temp_dir().join(format!("crewvela-cli-routing-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let mut cli = sessions.create_runtime(NewRuntimeSession { runtime_kind: "zcode_cli".into(), cwd: ".".into(), project_id: None }).unwrap();
        cli.pty_id = Some(42); // Selection fixture only: no CLI process is launched.
        let desktop = sessions.create_agent(sessions::NewAgentSession { avatar_id: None, project_id: "project-a".into(), agent_id: "desktop".into(), runtime_kind: "zcode".into(), agent_name: "ZCode Desktop".into(), cwd: ".".into() }).unwrap();
        let codex = sessions.create_agent(sessions::NewAgentSession { avatar_id: None, project_id: "project-a".into(), agent_id: "codex".into(), runtime_kind: "codex".into(), agent_name: "Codex".into(), cwd: ".".into() }).unwrap();
        let candidates = [desktop, cli.clone(), codex.clone()];
        let targets = resolve_direct_collaboration_sessions(&candidates, Some("project-a"), &runtimes, &[], &[]).unwrap();
        assert_eq!(targets.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec![cli.id.as_str(), codex.id.as_str()]);
        // A directional instruction can use the CLI group name or its shorthand.
        for instruction in ["@ZCodeCLI @Codex zcode给codex打个招呼", "@ZCodeCLI @Codex zcodecli给codex打个招呼"] {
            let request = direct_message_request(instruction).unwrap();
            assert_eq!(resolve_direct_message_direction(Some(&request), &targets).unwrap(), Some((cli.id.as_str(), codex.id.as_str())));
        }
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zcode_cli_aliases_keep_legacy_calls_and_identifier_boundaries() {
        for alias in ["@ZCodeCLI", "@zcode_cli", "@zcode-cli"] {
            let text = format!("{alias} @Codex 打个招呼，正文包含 zcode");
            let request = direct_message_request(&text).unwrap();
            assert_eq!(direct_collaboration_runtimes(&text, Some(&request), false, None), vec![AgentRuntimeKind::ZCodeCli, AgentRuntimeKind::Codex]);
        }
        assert!(explicitly_mentioned_agent_runtimes("@ZCodeCLI-2 @ZCodeCLIExtra zcode").is_empty());
        assert_eq!(requested_agent_runtimes("使用 zcode cli"), vec![AgentRuntimeKind::ZCodeCli]);
        let old: DirectCollaborationInput = serde_json::from_value(serde_json::json!({ "message": "hello" })).unwrap();
        assert!(old.target_runtime_kinds.is_none());
        let explicit: DirectCollaborationInput = serde_json::from_value(serde_json::json!({ "message": "hello", "targetRuntimeKinds": [] })).unwrap();
        assert_eq!(explicit.target_runtime_kinds, Some(vec![]));
        // The shared enum intentionally maps unknown protocol values to Unsupported.
        // The dispatch preflight must reject it before creating sessions.
        let unknown: DirectCollaborationInput = serde_json::from_value(serde_json::json!({ "message": "hello", "targetRuntimeKinds": ["typo"] })).unwrap();
        assert!(unknown.target_runtime_kinds.unwrap()[0].ensure_available().is_err());
    }

    #[test]
    fn directed_collaboration_dispatches_only_sender_with_selected_receiver() {
        let root = std::env::temp_dir().join(format!("crewvela-directed-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let create = |runtime: &str, agent: &str, name: &str| {
            sessions
                .create_agent(sessions::NewAgentSession {
                    avatar_id: None,
                    project_id: "project-a".into(),
                    agent_id: agent.into(),
                    runtime_kind: runtime.into(),
                    agent_name: name.into(),
                    cwd: ".".into(),
                })
                .unwrap()
        };
        let pi = create("pi", "pi-agent", "Pi审查工程师");
        let claude = create("claude_code", "claude-agent", "Claude架构师");
        let other = create("claude_code", "other-agent", "未选Claude");
        let codex = create("codex", "codex-agent", "未选Codex");
        let message =
            "@Pi审查工程师 @Claude架构师 让pi给claude code发一句 hello，我是pi！！！codex";
        let request = direct_message_request(message).unwrap();
        let runtimes = direct_collaboration_runtimes(message, Some(&request), true, None);
        assert!(runtimes.is_empty());
        let targets = resolve_direct_collaboration_sessions(
            &sessions.list(),
            Some("project-a"),
            &runtimes,
            &["pi-agent".into(), "claude-agent".into()],
            &[],
        )
        .unwrap();
        assert_eq!(targets.len(), 2);
        assert!(!targets
            .iter()
            .any(|target| target.id == other.id || target.id == codex.id));
        let direction = resolve_direct_message_direction(Some(&request), &targets).unwrap();
        assert_eq!(direction, Some((pi.id.as_str(), claude.id.as_str())));
        let execution = direct_message_execution_sessions(&targets, direction);
        assert_eq!(
            execution
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec![pi.id.as_str()]
        );
        assert_eq!(direct_message_execution_sessions(&targets, None).len(), 2);
        // The shorthand must use direct delivery even when the receiver has no active PTY.
        // Both endpoints are prepared; only the sender receives the execution instruction.
        let shorthand = "@Pi审查工程师 @Claude架构师 pi给claude code发句hi";
        let short_request = direct_message_request(shorthand).unwrap();
        let short_runtimes = direct_collaboration_runtimes(shorthand, Some(&short_request), true, None);
        let short_targets = resolve_direct_collaboration_sessions(
            &sessions.list(),
            Some("project-a"),
            &short_runtimes,
            &["pi-agent".into(), "claude-agent".into()],
            &[],
        )
        .unwrap();
        assert!(short_targets
            .iter()
            .any(|target| target.id == claude.id && target.pty_id.is_none()));
        let short_direction =
            resolve_direct_message_direction(Some(&short_request), &short_targets).unwrap();
        assert_eq!(short_direction, Some((pi.id.as_str(), claude.id.as_str())));
        assert_eq!(
            direct_message_execution_sessions(&short_targets, short_direction)
                .iter()
                .map(|target| target.id.as_str())
                .collect::<Vec<_>>(),
            vec![pi.id.as_str()]
        );
        // With Runtime-group references, only endpoints count; unrelated runtime names in the body do not.
        assert_eq!(
            direct_collaboration_runtimes(message, Some(&request), false, None),
            vec![AgentRuntimeKind::Pi, AgentRuntimeKind::ClaudeCode]
        );
        let missing = direct_message_request("让pi给codex发一句 hello").unwrap();
        assert!(resolve_direct_message_direction(Some(&missing), &targets).is_err());
        // Without explicit instance IDs, never hide another eligible Claude behind min/max selection.
        let full_roster = resolve_directed_collaboration_sessions(
            &sessions.list(),
            Some("project-a"),
            &[AgentRuntimeKind::Pi, AgentRuntimeKind::ClaudeCode],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(full_roster.len(), 3);
        assert!(
            resolve_direct_message_direction(Some(&request), &full_roster)
                .unwrap_err()
                .contains("多个")
        );
        let global_roster = resolve_directed_collaboration_sessions(
            &sessions.list(),
            None,
            &[AgentRuntimeKind::Pi, AgentRuntimeKind::ClaudeCode],
            &[],
            &[],
        )
        .unwrap();
        assert!(
            resolve_direct_message_direction(Some(&request), &global_roster)
                .unwrap_err()
                .contains("多个")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicitly_requested_yolo_runtime_commands_keep_their_supported_flags() {
        let root = std::env::temp_dir().join(format!("crewvela-yolo-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        for (runtime, expected) in [
            (
                AgentRuntimeKind::ClaudeCode,
                "--dangerously-skip-permissions",
            ),
            (
                AgentRuntimeKind::Codex,
                "--dangerously-bypass-approvals-and-sandbox",
            ),
            (AgentRuntimeKind::KimiCode, "--yolo"),
        ] {
            let record = sessions
                .create_runtime(NewRuntimeSession {
                    runtime_kind: runtime.as_str().into(),
                    cwd: ".".into(),
                    project_id: None,
                })
                .unwrap();
            let mut command = Command::new("runtime");
            configure_yolo_runtime_command(runtime, &mut command, &record, &sessions).unwrap();
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            assert!(
                args.iter().any(|arg| arg == expected),
                "{runtime:?}: {args:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_preferences_disabled_pending_cli_mailbox_does_not_break_other_runtime_polling() {
        let root =
            std::env::temp_dir().join(format!("cc-disabled-cli-poll-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(root.join("project.db")).unwrap();
        let project = store
            .create_project(CreateProjectInput {
                name: "runtime filtering".into(),
                goal: "preserve pending messages".into(),
                root_path: root.display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue = SessionActionQueue::new();
        let mut members = Vec::new();
        for kind in [AgentRuntimeKind::Codex, AgentRuntimeKind::Pi] {
            let agent = store
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: kind.as_str().into(),
                    runtime_kind: kind,
                    role: multi_agent::AgentRole::Worker,
                    description: String::new(),
                    model: None,
                    permission_profile: multi_agent::PermissionProfile::Default,
                    max_concurrency: 1,
                persona: Default::default(),
                persona_custom: String::new(),
                })
                .unwrap();
            let session = sessions
                .create_agent(sessions::NewAgentSession {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    agent_id: agent.id,
                    runtime_kind: kind.as_str().into(),
                    agent_name: kind.as_str().into(),
                    cwd: root.display().to_string(),
                })
                .unwrap();
            sessions
                .set_runtime_command_host(&session.id, true)
                .unwrap();
            queue.enqueue_message(
                &session.id,
                QueuedMessage::new("pending message".into(), None),
            );
            members.push(session);
        }
        runtime_preferences::with_disabled_for_test(AgentRuntimeKind::Codex, || {
            let mut starts = 0;
            flush_cli_messages_with_scheduler(
                &store,
                &sessions,
                &queue,
                &PtyManager::new(),
                |_| {
                    starts += 1;
                    Ok(())
                },
            )
            .unwrap();
            assert!(queue.peek_next(&members[0].id).is_some());
            assert!(queue.peek_next(&members[1].id).is_none());
            assert!(starts > 0);
            let snapshot = store.snapshot(&project.id).unwrap();
            assert_eq!(snapshot.tasks.len(), 1);
            assert_eq!(
                snapshot.tasks[0].assignee_agent_id.as_deref(),
                members[1].multi_agent_agent_id.as_deref()
            );
        });
        let agent = store
            .agent_profile(members[0].multi_agent_agent_id.as_deref().unwrap())
            .unwrap();
        store
            .update_agent(UpdateAgentInput {
                project_id: project.id.clone(),
                agent_id: agent.id,
                name: agent.name,
                runtime_kind: agent.runtime_kind,
                role: agent.role,
                description: agent.description,
                model: agent.model,
                avatar_id: agent.avatar_id,
                enabled: false,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        queue.enqueue_message(
            &members[1].id,
            QueuedMessage::new("another enabled runtime turn".into(), None),
        );
        flush_cli_messages_with_scheduler(
            &store,
            &sessions,
            &queue,
            &PtyManager::new(),
            |_| Ok(()),
        )
        .unwrap();
        assert!(queue.peek_next(&members[0].id).is_some());
        assert!(queue.peek_next(&members[1].id).is_none());
        assert_eq!(store.snapshot(&project.id).unwrap().tasks.len(), 2);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn background_cli_flush_does_not_reclassify_a_runtime_during_first_spawn() {
        let root =
            std::env::temp_dir().join(format!("cc-first-spawn-flush-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(root.join("multi-agent.db")).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: "startup-project".into(),
                agent_id: "pi-agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "Pi".into(),
                cwd: root.display().to_string(),
            })
            .unwrap();
        let queue = SessionActionQueue::new();
        let pty = PtyManager::new();
        let starting = sessions.begin_runtime_start(&session.id).unwrap();
        flush_cli_messages_with_scheduler(&store, &sessions, &queue, &pty, |_| {
            panic!("startup must not be rescheduled as a command host")
        })
        .unwrap();
        assert!(!sessions.get(&session.id).unwrap().runtime_command_host);
        assert!(sessions.get(&session.id).unwrap().pty_id.is_none());
        drop(starting);
        // An actual failed launch can migrate later, as long as its project exists.
        assert!(sessions
            .migrate_inactive_runtime_command_host(&session.id, |_| false)
            .unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn background_cli_ingress_without_any_pty_routes_all_active_runtimes() {
        let root = std::env::temp_dir().join(format!(
            "crewvela-headless-messaging-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("project.db")).unwrap();
        let project = store
            .create_project(CreateProjectInput {
                name: "conversation".into(),
                goal: "exchange".into(),
                root_path: root.display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap();
        store
            .set_project_status(&project.id, multi_agent::ProjectStatus::Draft)
            .unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let pty = PtyManager::new();
        let mut members = Vec::new();
        for runtime_kind in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
        ] {
            let agent = store
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: runtime_kind.as_str().into(),
                    runtime_kind,
                    role: multi_agent::AgentRole::Worker,
                    description: String::new(),
                    model: None,
                    permission_profile: multi_agent::PermissionProfile::Default,
                    max_concurrency: 1,
                persona: Default::default(),
                persona_custom: String::new(),
                })
                .unwrap();
            members.push(
                sessions
                    .create_agent(sessions::NewAgentSession {
                        avatar_id: None,
                        project_id: project.id.clone(),
                        agent_id: agent.id,
                        runtime_kind: runtime_kind.as_str().into(),
                        agent_name: runtime_kind.as_str().into(),
                        cwd: root.display().to_string(),
                    })
                    .unwrap(),
            );
        }
        for sender in &members {
            for receiver in &members {
                messaging::write_outbox(
                    &root.join("outbox"),
                    &OutboxMessage {
                        to: receiver.id.clone(),
                        from: Some(control::MessageOrigin {
                            session_id: sender.id.clone(),
                            label: sender.title.clone(),
                        }),
                        body: format!("hello from {}\nreply only when needed", sender.id),
                    },
                )
                .unwrap();
            }
        }
        let mut starts = 0;
        for _ in 0..members.len() {
            flush_cli_messages_with_scheduler(&store, &sessions, &queue, &pty, |id| {
                assert_eq!(id, project.id);
                starts += 1;
                Ok(())
            })
            .unwrap();
        }
        assert!(starts > 0);
        assert!(members
            .iter()
            .all(|member| queue.peek_next(&member.id).is_none()));
        assert!(sessions
            .list()
            .iter()
            .all(|member| member.pty_id.is_none() && member.runtime_command_host));
        let snapshot = store.snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.len(), members.len() * members.len());
        assert_eq!(snapshot.messages.len(), members.len() * members.len());
        assert_eq!(snapshot.project.status, multi_agent::ProjectStatus::Draft);
        assert!(snapshot
            .tasks
            .iter()
            .all(|task| task.id.starts_with("cli-message-")
                && task.status == multi_agent::TaskStatus::Ready));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cli_session_message_is_recorded_in_the_project_store() {
        let root =
            std::env::temp_dir().join(format!("crewvela-cli-message-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(root.join("multi-agent.db")).unwrap();
        let project = store
            .create_project(CreateProjectInput {
                name: "通信测试".into(),
                goal: "验证统一消息源".into(),
                root_path: root.display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap();
        let create_agent = |name: &str, runtime_kind, role| CreateAgentInput {
            avatar_id: None,
            project_id: project.id.clone(),
            name: name.into(),
            runtime_kind,
            role,
            description: String::new(),
            model: None,
            permission_profile: multi_agent::PermissionProfile::Auto,
            max_concurrency: 1,
        persona: Default::default(),
        persona_custom: String::new(),
        };
        let sender_agent = store
            .create_agent(create_agent(
                "Claude",
                AgentRuntimeKind::ClaudeCode,
                multi_agent::AgentRole::Orchestrator,
            ))
            .unwrap();
        let target_agent = store
            .create_agent(create_agent(
                "Codex",
                AgentRuntimeKind::Codex,
                multi_agent::AgentRole::Worker,
            ))
            .unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let sender = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: project.id.clone(),
                agent_id: sender_agent.id.clone(),
                runtime_kind: "claude_code".into(),
                agent_name: "Claude".into(),
                cwd: root.display().to_string(),
            })
            .unwrap();
        let target = sessions
            .create_agent(sessions::NewAgentSession {
                avatar_id: None,
                project_id: project.id.clone(),
                agent_id: target_agent.id.clone(),
                runtime_kind: "codex".into(),
                agent_name: "Codex".into(),
                cwd: root.display().to_string(),
            })
            .unwrap();
        persist_cli_collaboration_message_with_id(
            &store,
            &sessions,
            &OutboxMessage {
                to: target.id,
                from: Some(control::MessageOrigin {
                    session_id: sender.id,
                    label: "Claude".into(),
                }),
                body: "HELLO_CODEX".into(),
            },
            "exact-transport-id",
            chrono::Utc::now().timestamp_millis(),
        )
        .unwrap();

        let messages = store.snapshot(&project.id).unwrap().messages;
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].sender_id.as_deref(),
            Some(sender_agent.id.as_str())
        );
        assert_eq!(
            messages[0].target_agent_id.as_deref(),
            Some(target_agent.id.as_str())
        );
        assert_eq!(messages[0].body, "HELLO_CODEX");
        assert_eq!(messages[0].id, "exact-transport-id");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pi_rendering_does_not_override_extension_readiness() {
        let root = std::env::temp_dir().join(format!("cc-pi-redraw-{}", uuid::Uuid::new_v4()));
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions
            .create_runtime(sessions::NewRuntimeSession {
                runtime_kind: "pi".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        sessions.attach_runtime(&record.id, 42).unwrap();
        assert!(sessions.mark_runtime_prompt_ready(&record.id));
        let chunks = std::collections::HashMap::from([(
            42,
            pty::PtyChunk {
                app_session_id: record.id.clone(),
                data: "arbitrary Pi UI rendering".into(),
                running: true,
                code: None,
            },
        )]);
        mark_interactive_runtime_prompts(&chunks, &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id));
        sessions.mark_prompt_dispatched(&record.id);
        mark_interactive_runtime_prompts(&chunks, &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires the installed Kimi Code CLI"]
    fn real_kimi_prompt_accepts_a_queued_message() {
        let root = std::env::temp_dir().join(format!("crewvela-kimi-pty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let pty = PtyManager::new();
        let queue = SessionActionQueue::new();
        let record = sessions
            .create_runtime(sessions::NewRuntimeSession {
                runtime_kind: "kimi_code".into(),
                cwd: dirs::home_dir().unwrap().display().to_string(),
                project_id: None,
            })
            .unwrap();
        let started = open_runtime_session(&record, &pty, &sessions).unwrap();
        let pty_id = started.pty_id.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !sessions.is_ready_for_prompt(&record.id) && std::time::Instant::now() < deadline {
            let chunks = pty.poll_all();
            mark_interactive_runtime_prompts(&chunks, &sessions);
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        assert!(sessions.is_ready_for_prompt(&record.id));

        let marker = "CREWVELA_KIMI_PTY_OK";
        queue.enqueue_message(&record.id, QueuedMessage::new(marker.into(), None));
        assert_eq!(queue.flush_ready(&sessions, &pty), vec![record.id.clone()]);
        std::thread::sleep(std::time::Duration::from_millis(700));
        queue.flush_pending_submits(&sessions, &pty);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let transcript_bytes = pty.transcript_tail(pty_id, 16_384).unwrap();
        let transcript = String::from_utf8_lossy(&transcript_bytes);
        assert!(transcript.contains(marker));

        let _ = pty.kill(pty_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cli_pets_follow_terminal_and_pi_turn_signals_without_changing_session_status() {
        let (_root, sessions) = lifecycle_fixture();
        for (index, (runtime, idle, working)) in [
            ("codex", "\x1b]7501;state=idle:app=codex\x07", "\x1b]7501;state=working:app=codex\x07"),
            ("kimi_code", "│ > ", "Thinking…\r\nesc to interrupt"),
            ("opencode", "Build · MiniMax M3\r\nctrl+p commands", "Thinking…\r\nesc interrupt"),
            ("zcode_cli", "Type a prompt", "Calling model…\r\nType to queue input\r\nesc to interrupt"),
        ].into_iter().enumerate() {
            let pty_id = index as u32 + 1;
            let record = sessions.create_runtime(sessions::NewRuntimeSession {
                runtime_kind: runtime.into(), cwd: ".".into(), project_id: None,
            }).unwrap();
            sessions.attach_runtime(&record.id, pty_id).unwrap();
            let active = || sessions.working_cli_pty_ids(&sessions.list()).contains(&pty_id);
            let output = |text: &str| mark_interactive_runtime_prompts(
                &[(pty_id, pty::PtyChunk {
                    app_session_id: record.id.clone(), data: format!("\x1b[2J\x1b[H{text}"),
                    running: true, code: None,
                })].into_iter().collect(), &sessions,
            );
            assert!(!active(), "{runtime} PTY attach is idle");
            output(idle);
            assert!(!active(), "{runtime} ready composer is idle");
            output(working);
            assert!(active(), "{runtime} accepted working evidence");
            assert!(active(), "{runtime} remains working without fresh output");
            output(idle);
            assert!(!active(), "{runtime} returned to composer");
            assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
            output(working);
            output("Permission required · Allow once");
            assert!(!active(), "{runtime} waits without playing the work motion");
        }

        let pi = sessions.create_runtime(sessions::NewRuntimeSession {
            runtime_kind: "pi".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        let token = sessions.configure_pi_readiness(&mut Command::new("pi"), &pi.id).unwrap();
        sessions.attach_runtime(&pi.id, 8).unwrap();
        let active = || sessions.working_cli_pty_ids(&sessions.list()).contains(&8);
        let event = |sequence, ready, working| serde_json::json!({
            "_crewvela_app_session_id": pi.id, "session_id": pi.id, "hook_event_name": "RuntimePromptState",
            "runtime_kind": "pi", "runtime_token": token, "runtime_sequence": sequence, "ready": ready,
            "agent_working": working,
        });
        assert!(!active(), "Pi is idle at PTY attachment");
        sessions.apply_hook_value(&event(1, false, false)).unwrap();
        assert!(!active(), "Pi input preparation is not an Agent turn");
        sessions.apply_hook_value(&event(2, false, true)).unwrap();
        assert!(active(), "Pi emitted Agent start");
        sessions.apply_hook_value(&event(3, true, false)).unwrap();
        assert!(!active(), "Pi emitted settlement");
        sessions.apply_hook_value(&event(2, false, true)).unwrap();
        assert!(!active(), "old events cannot restart the work motion");
    }

    #[test]
    #[ignore = "requires installed, signed-in CLIs; submits one short model request per runtime in a temporary workspace"]
    fn cli_pets_verify_live_turns() {
        use std::time::{Duration, Instant};
        let root = std::env::temp_dir().join(format!("crewvela-cli-pet-live-{}", uuid::Uuid::new_v4()));
        test_profile::initialize(&root).unwrap();
        let selection = std::env::var("CREWVELA_CLI_PET_TEST_RUNTIMES").ok();
        let runtimes = ["codex", "kimi_code", "opencode", "zcode_cli", "pi"].into_iter()
            .filter(|runtime| selection.as_deref().is_none_or(|value| value.split(',').any(|item| item == *runtime)))
            .collect::<Vec<_>>();
        assert!(!runtimes.is_empty());
        let mut failures = Vec::new();
        std::thread::scope(|scope| {
            let probes: Vec<_> = runtimes.iter().map(|&runtime| {
                let dir = root.join(runtime);
                (runtime, scope.spawn(move || -> anyhow::Result<()> {
                    std::fs::create_dir_all(&dir)?;
                    let sessions = SessionManager::open_at(dir.join("sessions.json"), dir.join("events"))?;
                    let pty = PtyManager::new();
                    let result = (|| -> anyhow::Result<()> {
                        let cwd = if runtime == "kimi_code" {
                            std::env::var("CREWVELA_CLI_PET_KIMI_CWD").unwrap_or_else(|_| dir.display().to_string())
                        } else { dir.display().to_string() };
                        let record = sessions.create_runtime(sessions::NewRuntimeSession {
                            runtime_kind: runtime.into(), cwd, project_id: None,
                        })?;
                        let started = open_runtime_session(&record, &pty, &sessions).map_err(anyhow::Error::msg)?;
                        let pty_id = started.pty_id.unwrap();
                        pty.resize(pty_id, 38, 115)?;
                        sessions.resize_runtime_prompt(&record.id, 38, 115);
                        let save_screen = || -> anyhow::Result<()> {
                            let tail = pty.transcript_tail(pty_id, 65536)?;
                            let mut screen = vt100::Parser::new(
                                std::num::NonZeroU16::new(38).unwrap(), std::num::NonZeroU16::new(115).unwrap(), 0);
                            screen.process(&tail);
                            std::fs::write(dir.join("screen.txt"), screen.screen().contents())?;
                            Ok(())
                        };
                        let poll = || -> anyhow::Result<bool> {
                            sessions.ingest_events()?;
                            mark_interactive_runtime_prompts(&pty.poll_all(), &sessions);
                            Ok(sessions.working_cli_pty_ids(&sessions.list()).contains(&pty_id))
                        };
                        let startup = Instant::now() + Duration::from_secs(18);
                        while !sessions.is_ready_for_prompt(&record.id) && Instant::now() < startup {
                            poll()?;
                            std::thread::sleep(Duration::from_millis(60));
                        }
                        if !sessions.is_ready_for_prompt(&record.id) {
                            save_screen()?;
                            anyhow::bail!("idle composer was not recognized");
                        }
                        anyhow::ensure!(!poll()?, "the idle composer is animated as working");
                        pty.write(pty_id, b"Reply with PET followed immediately by _OK. Do not use any tools.")?;
                        std::thread::sleep(Duration::from_millis(350));
                        pty.write(pty_id, b"\r")?;
                        sessions.mark_terminal_submission(pty_id)?;
                        let deadline = Instant::now() + Duration::from_secs(40);
                        let mut worked = false;
                        while pty.is_running(pty_id) && Instant::now() < deadline {
                            let working = poll()?;
                            worked |= working;
                            let tail = pty.transcript_tail(pty_id, 65536)?;
                            let replied = multi_agent::strip_terminal_control_sequences(&String::from_utf8_lossy(&tail)).contains("PET_OK");
                            if replied && worked && !working {
                                return Ok(());
                            }
                            std::thread::sleep(Duration::from_millis(45));
                        }
                        let tail = pty.transcript_tail(pty_id, 65536).unwrap_or_default();
                        let replied = multi_agent::strip_terminal_control_sequences(&String::from_utf8_lossy(&tail)).contains("PET_OK");
                        save_screen()?;
                        anyhow::bail!("turn did not settle: observed working={worked}, still working={}, model replied={replied}, composer ready={}",
                            sessions.working_cli_pty_ids(&sessions.list()).contains(&pty_id),
                            sessions.is_ready_for_prompt(&record.id))
                    })();
                    let cleanup = pty.kill_all();
                    result?;
                    cleanup
                }))
            }).collect();
            for (runtime, probe) in probes {
                let result = probe.join().unwrap_or_else(|_| Err(anyhow::anyhow!("probe panicked")));
                eprintln!("{runtime} CLI pet: {result:?}");
                if result.is_err() { failures.push(runtime); }
            }
        });
        if !failures.is_empty() { eprintln!("Isolated terminal screens: {}", root.display()); }
        assert!(failures.is_empty(), "live probes with unverified transitions: {failures:?}");
    }

    #[test]
    fn only_closing_the_main_window_stops_managed_sessions() {
        assert!(should_stop_sessions_for_window("main"));
        assert!(!should_stop_sessions_for_window("control"));
    }

    #[test]
    fn only_enter_acknowledges_terminal_submission_not_multiline_paste() {
        for input in [b"\r".as_slice(), b"\n", b"\r\n"] {
            assert!(terminal_input_is_submit(input));
        }
        for input in [b"".as_slice(), b"hello\nworld", b"hello\r", b"\x1b[200~a\nb\x1b[201~", b"\x1b[A"] {
            assert!(!terminal_input_is_submit(input), "{input:?}");
        }
    }

    #[test]
    fn codex_completed_turn_can_receive_without_a_window_redraw() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_agent(sessions::NewAgentSession {
            avatar_id: None, project_id: "project".into(), agent_id: "codex".into(),
            runtime_kind: "codex".into(), agent_name: "Codex".into(),
            cwd: root.display().to_string(),
        }).unwrap();
        sessions.attach_runtime(&record.id, 42).unwrap();
        sessions.register_agent_prompt(&record.id, "previous turn");
        let frame = |text: &str| HashMap::from([(42, pty::PtyChunk {
            app_session_id: record.id.clone(), data: text.into(), running: true, code: None,
        })]);
        mark_interactive_runtime_prompts(&frame("Working (3s · esc to interrupt)\n› Ask Codex to do anything\n"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id));
        sessions.apply_hook_value(&serde_json::json!({
            "_crewvela_app_session_id": record.id,
            "session_id": record.claude_session_id,
            "hook_event_name": "AgentTurnComplete", "input_prompt": "previous turn",
            "last_assistant_message": "done",
        })).unwrap().expect("owned completion accepted");
        assert!(sessions.is_ready_for_prompt(&record.id), "completed Codex must not retain the previous turn's busy blocker");
        mark_interactive_runtime_prompts(&frame("\x1b[0m\x1b[?25h"), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id), "cursor-only rendering must not revoke confirmed completion");
        mark_interactive_runtime_prompts(&frame("\x1b[H\x1b[1;1H\x1b[0m"), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id), "moving the cursor home does not replace the screen or revoke a completed turn");
        mark_interactive_runtime_prompts(&frame("\x1b[H\x1b[0Junknown replacement screen"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id), "home followed by display erasure must revoke readiness");
        mark_interactive_runtime_prompts(&frame("\x1b]7501;state=done:app=codex\x1b\\"), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id));
        mark_interactive_runtime_prompts(&frame("Working (3s · esc to interrupt) · Running hooks\n› Ask Codex to do anything\n"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id));
        mark_interactive_runtime_prompts(&frame("\x1b]7501;state=done:app=codex\x1b\\"), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id), "native completion must release the final hooks redraw without a resize");
        mark_interactive_runtime_prompts(&frame("\x1b]7501;state=done:app=codex\x07Working (1s · esc to interrupt)\n› Ask Codex to do anything\n"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id), "a later busy frame wins over an old native completion");
        mark_interactive_runtime_prompts(&frame("\x1b]7501;state=do"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id));
        mark_interactive_runtime_prompts(&frame("ne:app=codex\x1b\\"), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id), "split native status must be assembled");
        for dialog in ["Permission required\nAllow once\nenter to confirm", "Update available\nenter continue · esc skip"] {
            mark_interactive_runtime_prompts(&frame(dialog), &sessions);
            sessions.apply_hook_value(&serde_json::json!({
                "_crewvela_app_session_id": record.id, "session_id": record.claude_session_id,
                "hook_event_name": "AgentTurnComplete", "input_prompt": "previous turn",
            })).unwrap();
            mark_interactive_runtime_prompts(&frame("\x1b]7501;state=done:app=codex\x07"), &sessions);
            assert!(!sessions.is_ready_for_prompt(&record.id), "completion cannot dismiss a dialog");
            mark_interactive_runtime_prompts(&frame("Working (1s · esc to interrupt)\n› Ask Codex to do anything\n"), &sessions);
            mark_interactive_runtime_prompts(&frame("\x1b]7501;state=done:app=codex\x07"), &sessions);
            assert!(!sessions.is_ready_for_prompt(&record.id), "busy rendering cannot downgrade a modal blocker");
            mark_interactive_runtime_prompts(&frame("\x1b[2JWorked for 3s\n› Ask Codex to do anything\n"), &sessions);
            assert!(sessions.is_ready_for_prompt(&record.id));
        }
        mark_interactive_runtime_prompts(&frame("\x1b[2J\x1b[HUnknown replacement screen"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id), "completion must not make an unknown replacement screen ready");
        sessions.mark_prompt_dispatched(&record.id);
        mark_interactive_runtime_prompts(&frame("\x1b[0m\x1b[?25h"), &sessions);
        assert!(!sessions.is_ready_for_prompt(&record.id), "rendering cannot revive a dispatched prompt");
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_pty_frames_cannot_change_the_replacement_process_prompt_blocker() {
        let (root, sessions) = lifecycle_fixture();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind: "codex".into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        let ready = ">_ OpenAI Codex (v0.162.1)\nC:\\fixture\n› Ask Codex to do anything\n";
        let blocked = "Update available\n› 1. Update now\n2. Skip\nenter continue · esc skip";
        let frame = |pty_id, text: &str| HashMap::from([(pty_id, pty::PtyChunk {
            app_session_id: record.id.clone(), data: text.into(), running: true, code: None,
        })]);
        sessions.attach_runtime(&record.id, 1).unwrap();
        sessions.attach_runtime(&record.id, 2).unwrap();
        mark_interactive_runtime_prompts(&frame(2, blocked), &sessions);
        assert!(sessions.runtime_prompt_is_blocked(&record.id));
        mark_interactive_runtime_prompts(&frame(1, ready), &sessions);
        assert!(sessions.runtime_prompt_is_blocked(&record.id));
        assert!(!sessions.is_ready_for_prompt(&record.id));
        mark_interactive_runtime_prompts(&frame(2, ready), &sessions);
        assert!(sessions.is_ready_for_prompt(&record.id));
        mark_interactive_runtime_prompts(&frame(1, blocked), &sessions);
        assert!(!sessions.runtime_prompt_is_blocked(&record.id));
        assert!(sessions.is_ready_for_prompt(&record.id));
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dock_request_is_consumed_once() {
        let state = ControlWindowState::default();
        state.dock_requested.store(true, Ordering::Release);

        assert!(state.dock_requested.swap(false, Ordering::AcqRel));
        assert!(!state.dock_requested.swap(false, Ordering::AcqRel));
    }

    #[test]
    fn generated_project_metadata_rejects_null_like_placeholders() {
        assert_eq!(
            required_plan_text(Some(" null "), "项目名称").unwrap_err(),
            "新项目计划缺少有效的项目名称"
        );
        assert_eq!(
            required_plan_text(Some("贪吃蛇"), "项目名称").unwrap(),
            "贪吃蛇"
        );
    }

    #[test]
    fn cli_plan_admission_rejects_new_desktop_but_keeps_explicit_existing_member() {
        for kind in [AgentRuntimeKind::ZCode,AgentRuntimeKind::ClaudeChat,AgentRuntimeKind::CodexChat,AgentRuntimeKind::KimiChat,AgentRuntimeKind::PiChat,AgentRuntimeKind::OpenCodeChat] {
            let mut plan=cross_runtime_plan(true);plan.agents[0].runtime_kind=kind;
            assert!(validate_coordinator_plan(&plan).unwrap_err().contains("CLI"));
        }
        let mut plan=cross_runtime_plan(true);plan.intent=multi_agent::CoordinatorIntent::AddTasks;
        plan.agents[0].runtime_kind=AgentRuntimeKind::ZCode;plan.agents[0].existing_agent_id=Some("original-desktop-member".into());
        assert!(validate_coordinator_plan(&plan).is_ok());
    }

    fn cross_runtime_plan(with_reviews: bool) -> CoordinatorPlan {
        let mut agents = vec![
            serde_json::json!({"key":"frontend","name":"Claude GLM","runtimeKind":"claude_code","role":"worker","description":"前端","model":"glm","permissionProfile":"auto"}),
            serde_json::json!({"key":"backend","name":"Codex","runtimeKind":"codex","role":"worker","description":"后端","model":null,"permissionProfile":"auto"}),
        ];
        let mut tasks = vec![
            serde_json::json!({"key":"frontend_impl","stage":1,"title":"实现前端","description":"前端","acceptanceCriteria":"测试通过","verificationCommand":"npm test","assigneeKey":"frontend","reviewerKey":null,"dependsOn":[],"workspaceMode":"worktree","priority":0}),
            serde_json::json!({"key":"backend_impl","stage":1,"title":"实现后端","description":"后端","acceptanceCriteria":"测试通过","verificationCommand":"cargo test","assigneeKey":"backend","reviewerKey":null,"dependsOn":[],"workspaceMode":"worktree","priority":0}),
        ];
        if with_reviews {
            agents.extend([
                serde_json::json!({"key":"frontend_review","name":"Claude Review","runtimeKind":"claude_code","role":"reviewer","description":"审查后端","model":"glm","permissionProfile":"auto"}),
                serde_json::json!({"key":"backend_review","name":"Codex Review","runtimeKind":"codex","role":"reviewer","description":"审查前端","model":null,"permissionProfile":"auto"}),
                serde_json::json!({"key":"integrator","name":"Codex Integrator","runtimeKind":"codex","role":"integrator","description":"集成","model":null,"permissionProfile":"auto"}),
            ]);
            tasks.extend([
                serde_json::json!({"key":"review_frontend","stage":2,"title":"Review 前端","description":"审查前端","acceptanceCriteria":"给出审查结果","verificationCommand":null,"assigneeKey":"backend_review","reviewerKey":null,"dependsOn":["frontend_impl"],"workspaceMode":"read_only","priority":0}),
                serde_json::json!({"key":"review_backend","stage":2,"title":"Review 后端","description":"审查后端","acceptanceCriteria":"给出审查结果","verificationCommand":null,"assigneeKey":"frontend_review","reviewerKey":null,"dependsOn":["backend_impl"],"workspaceMode":"read_only","priority":0}),
                serde_json::json!({"key":"integrate","stage":3,"title":"集成交付","description":"合并交付","acceptanceCriteria":"全量测试通过","verificationCommand":"npm test","assigneeKey":"integrator","reviewerKey":null,"dependsOn":["review_frontend","review_backend"],"workspaceMode":"worktree","priority":0}),
            ]);
        }
        serde_json::from_value(serde_json::json!({
            "intent":"plan_project","reply":"计划","projectName":"测试","goal":"前后端协作",
            "rootPath":"F:\\project\\demo","targetTaskId":null,"targetAgentId":null,
            "agents":agents,"tasks":tasks,"requiresConfirmation":true
        }))
        .unwrap()
    }

    #[test]
    fn cross_runtime_implementation_requires_cross_reviews_and_integrator() {
        let error = validate_coordinator_plan(&cross_runtime_plan(false)).unwrap_err();
        assert!(error.contains("Review"));
        assert!(validate_coordinator_plan(&cross_runtime_plan(true)).is_ok());
    }

    #[test]
    fn greeting_task_does_not_require_runtime_commands_capability() {
        let mut plan: CoordinatorPlan = serde_json::from_value(serde_json::json!({
            "intent":"plan_project","reply":"计划","projectName":"问候测试","goal":"Agent 互相打招呼",
            "rootPath":"F:\\project\\demo","targetTaskId":null,"targetAgentId":null,
            "agents":[{"key":"claude","name":"Claude 架构师","runtimeKind":"claude_code","role":"worker","description":"发送问候","model":null,"permissionProfile":"auto"}],
            "tasks":[{"key":"greet","stage":0,"title":"Claude 向其他 Agent 打招呼","description":"通过 crewvela Skill 和 CLI 发送问候","acceptanceCriteria":"问候送达","verificationCommand":null,"assigneeKey":"claude","reviewerKey":null,"dependsOn":[],"workspaceMode":"shared_write","priority":0,"requiredCapabilities":["commands"],"maxAttempts":3,"retryBackoffSeconds":5}],
            "requestedRuntimes":["claude_code"],"requiresConfirmation":true
        })).unwrap();

        normalize_coordinator_capabilities(&mut plan);

        assert!(plan.tasks[0].required_capabilities.is_empty());
        assert!(validate_coordinator_plan(&plan).is_ok());
    }
}
