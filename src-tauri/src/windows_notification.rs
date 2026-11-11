use std::path::Path;

use anyhow::{Context, Result};
use tauri_winrt_notification::{Sound, Toast};
use windows_registry::CURRENT_USER;
use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

const APP_ID: &str = crate::brand::APP_ID;
const APP_NAME: &str = "CrewVela";

pub fn is_unpacked_build(exe: &Path) -> bool {
    let Some(parent) = exe.parent() else {
        return false;
    };
    let components = parent
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    matches!(
        components.as_slice(),
        [.., target, profile]
            if target.eq_ignore_ascii_case("target")
                && (profile.eq_ignore_ascii_case("debug")
                    || profile.eq_ignore_ascii_case("release"))
    )
}

pub fn initialize() -> Result<()> {
    let exe = std::env::current_exe().context("读取 CrewVela 可执行文件路径失败")?;
    let key = CURRENT_USER
        .create(format!(r"SOFTWARE\Classes\AppUserModelId\{APP_ID}"))
        .context("注册 CrewVela AppUserModelID 失败")?;
    key.set_string("DisplayName", APP_NAME)?;
    key.set_string("IconBackgroundColor", "0")?;
    key.set_hstring("IconUri", &exe.as_path().into())?;

    let wide = APP_ID
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if result < 0 {
        anyhow::bail!("设置 CrewVela 进程 AppUserModelID 失败：HRESULT {result:#x}");
    }
    Ok(())
}

pub fn send_if_unpacked(title: &str, body: &str, system_sound: bool) -> Result<bool> {
    let exe = std::env::current_exe().context("读取 CrewVela 可执行文件路径失败")?;
    if !is_unpacked_build(&exe) {
        return Ok(false);
    }
    initialize()?;
    let mut toast = Toast::new(APP_ID).title(title).text1(body);
    toast = if system_sound {
        toast.sound(Some(Sound::Default))
    } else {
        toast.sound(None)
    };
    toast.show().context("发送 CrewVela 原生通知失败")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_target_debug_and_release_use_unpacked_transport() {
        assert!(is_unpacked_build(Path::new(
            r"C:\example\crewvela\src-tauri\target\debug\crewvela.exe"
        )));
        assert!(is_unpacked_build(Path::new(
            r"C:\example\crewvela\src-tauri\target\release\crewvela.exe"
        )));
        assert!(!is_unpacked_build(Path::new(
            r"C:\Program Files\CrewVela\crewvela.exe"
        )));
        assert!(!is_unpacked_build(Path::new(
            r"F:\portable\crewvela.exe"
        )));
    }
}
