// 保持电脑运行：有 Agent 会话运行、多智能体控制面存活或存在已启用的
// 自动化任务时，阻止系统因空闲进入休眠。语义对齐 Electron
// powerSaveBlocker("prevent-app-suspension")：防不了手动睡眠/合盖休眠。
//
// Windows 用 SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)。
// 该状态绑定调用线程：轮询线程常驻并独占调用，线程退出时自动释放，
// 因此应用退出无需显式清理（仍会在 ExitRequested 里清一次做卫生）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::sessions::SessionStatus;

static ENABLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// 会话记录是否算「活动」（keep-awake 判定；Completed/Error/Stopped 不算）。
pub(crate) fn is_active_status(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Pending | SessionStatus::Running | SessionStatus::Waiting
    )
}

fn evaluate(app: &AppHandle) -> bool {
    let sessions_active = app
        .try_state::<crate::sessions::SessionManager>()
        .map(|manager| manager.list().iter().any(|record| is_active_status(&record.status)))
        .unwrap_or(false);
    let control_plane_active = app
        .try_state::<crate::acp::AcpManager>()
        .map(|manager| manager.is_running())
        .unwrap_or(false);
    let automation_armed = app
        .try_state::<crate::automation::AutomationManager>()
        .map(|manager| manager.list().iter().any(|automation| automation.enabled))
        .unwrap_or(false);
    sessions_active || control_plane_active || automation_armed
}

#[cfg(target_os = "windows")]
fn apply() {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };
    let flags = if ENABLED.load(Ordering::Relaxed) && ACTIVE.load(Ordering::Relaxed) {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    // SetThreadExecutionState 失败（NULL）仅意味着本次未生效，下一轮会重试。
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(target_os = "windows"))]
fn apply() {}

/// 启动常驻轮询线程：每 30s 重估活跃状态并刷新执行状态（幂等 syscall）。
pub fn start(app: AppHandle) {
    ENABLED.store(
        crate::general_settings::load_cached().keep_awake,
        Ordering::Relaxed,
    );
    let _ = std::thread::Builder::new()
        .name("cc-keep-awake".into())
        .spawn(move || loop {
            ACTIVE.store(evaluate(&app), Ordering::Relaxed);
            apply();
            std::thread::sleep(POLL_INTERVAL);
        });
}

/// 设置保存路径：立即生效一次，不等下一轮轮询。
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    apply();
}

/// 面板状态展示：当前是否实际在阻止休眠；非 Windows 恒为 None。
pub fn current_state() -> Option<bool> {
    if cfg!(target_os = "windows") {
        Some(ENABLED.load(Ordering::Relaxed) && ACTIVE.load(Ordering::Relaxed))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unfinished_statuses_count_as_active() {
        for status in [
            SessionStatus::Pending,
            SessionStatus::Running,
            SessionStatus::Waiting,
        ] {
            assert!(is_active_status(&status), "{status:?} 应算活动");
        }
        for status in [
            SessionStatus::Completed,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ] {
            assert!(!is_active_status(&status), "{status:?} 不应算活动");
        }
    }
}
