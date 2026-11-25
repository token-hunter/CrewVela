use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager};

use crate::{providers, test_profile};

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub(crate) enum InterfaceLanguage {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

impl InterfaceLanguage {
    fn preference_path() -> anyhow::Result<PathBuf> {
        Ok(providers::config_dir()?.join("interface-language"))
    }

    pub(crate) fn load() -> Self {
        Self::preference_path().map(|path| Self::read_at(&path)).unwrap_or_default()
    }

    fn read_at(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(value) if value.trim() == "en" => Self::English,
            _ => Self::Chinese,
        }
    }

    fn save(self) -> anyhow::Result<()> {
        self.write_at(&Self::preference_path()?)
    }

    fn write_at(self, path: &Path) -> anyhow::Result<()> {
        let value = match self {
            Self::Chinese => "zh-CN",
            Self::English => "en",
        };
        match std::fs::read_to_string(path) {
            Ok(saved) if saved.trim() == value => return Ok(()),
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        providers::write_atomic(path, value.as_bytes())
    }

    pub(crate) fn tray_labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Chinese => ("显示 CrewVela", "退出"),
            Self::English => ("Show CrewVela", "Quit"),
        }
    }
}

pub(crate) struct TrayMenuItems {
    pub(crate) show: MenuItem<tauri::Wry>,
    pub(crate) quit: MenuItem<tauri::Wry>,
}

#[tauri::command]
pub(crate) async fn set_interface_language(
    app: AppHandle,
    language: InterfaceLanguage,
) -> Result<(), String> {
    if test_profile::active() {
        return Ok(());
    }
    language.save().map_err(|error| error.to_string())?;
    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let (show, quit) = language.tray_labels();
        items.show.set_text(show).map_err(|error| error.to_string())?;
        items.quit.set_text(quit).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::InterfaceLanguage;

    #[test]
    fn tray_content_uses_the_last_saved_language() {
        let path = std::env::temp_dir().join(format!("crewvela-language-{}", uuid::Uuid::new_v4()));
        assert_eq!(InterfaceLanguage::read_at(&path).tray_labels(), ("显示 CrewVela", "退出"));
        InterfaceLanguage::English.write_at(&path).unwrap();
        assert_eq!(InterfaceLanguage::read_at(&path).tray_labels(), ("Show CrewVela", "Quit"));
        InterfaceLanguage::Chinese.write_at(&path).unwrap();
        assert_eq!(InterfaceLanguage::read_at(&path).tray_labels(), ("显示 CrewVela", "退出"));
        std::fs::remove_file(path).unwrap();
    }
}
