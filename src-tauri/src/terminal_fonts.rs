use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledFont {
    family: String,
    aliases: Vec<String>,
    monospace: bool,
}

#[tauri::command]
pub(crate) async fn terminal_fonts_list() -> Result<Vec<InstalledFont>, String> {
    crate::blocking_command(|| {
        let fonts = installed_fonts().map_err(crate::to_err)?;
        if fonts.is_empty() {
            return Err("系统没有返回可用字体".into());
        }
        Ok(fonts)
    })
    .await
}

#[cfg(windows)]
fn installed_fonts() -> Result<Vec<InstalledFont>> {
    use windows::{core::Interface, Win32::Graphics::DirectWrite::*};

    // DirectWrite reports font families for system and per-user installations.
    // COM objects and UTF-16 buffers stay inside this background invocation.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection = None;
        factory.GetSystemFontCollection(&mut collection, true)?;
        let collection = collection.context("DirectWrite 未返回系统字体集合")?;
        let mut fonts = Vec::with_capacity(collection.GetFontFamilyCount() as usize);
        for index in 0..collection.GetFontFamilyCount() {
            let family = collection.GetFontFamily(index)?;
            let names = family.GetFamilyNames()?;
            let mut preferred = 0;
            let mut english_exists = Default::default();
            names.FindLocaleName(windows::core::w!("en-us"), &mut preferred, &mut english_exists)?;
            if !english_exists.as_bool() {
                preferred = 0;
            }
            let mut entries = Vec::new();
            for i in 0..names.GetCount() {
                let mut buffer = vec![0; names.GetStringLength(i)? as usize + 1];
                names.GetString(i, &mut buffer)?;
                buffer.pop();
                entries.push(String::from_utf16_lossy(&buffer));
            }
            if entries.is_empty() {
                continue;
            }
            let name = entries.swap_remove(preferred as usize);
            entries.retain(|alias| *alias != name && !alias.is_empty());
            entries.sort();
            entries.dedup();
            let monospace = family
                .GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)
                .and_then(|font| font.cast::<IDWriteFont1>())
                .is_ok_and(|font| font.IsMonospacedFont().as_bool());
            fonts.push(InstalledFont { family: name, aliases: entries, monospace });
        }
        Ok(fonts)
    }
}

#[cfg(target_os = "macos")]
fn installed_fonts() -> Result<Vec<InstalledFont>> {
    use objc2_core_foundation::{CFArray, CFRetained, CFString};
    use objc2_core_text::{CTFont, CTFontManagerCopyAvailableFontFamilyNames, CTFontSymbolicTraits};

    // Core Text documents an array of CFString font family names.
    let families = unsafe {
        CFRetained::cast_unchecked::<CFArray<CFString>>(CTFontManagerCopyAvailableFontFamilyNames())
    };
    Ok(families.iter().map(|family| {
        let font = unsafe { CTFont::with_name(&family, 12.0, std::ptr::null()) };
        let monospace = unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitMonoSpace);
        InstalledFont { family: family.to_string(), aliases: Vec::new(), monospace }
    }).collect())
}

#[cfg(target_os = "linux")]
fn installed_fonts() -> Result<Vec<InstalledFont>> {
    use std::{collections::BTreeMap, process::Command};
    let output = Command::new("fc-list")
        .arg("--format=%{family}\t%{spacing}\n")
        .output()
        .context("无法运行系统字体工具 fc-list")?;
    anyhow::ensure!(output.status.success(), "系统字体工具 fc-list 返回失败");
    let mut fonts = BTreeMap::<String, InstalledFont>::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((names, spacing)) = line.split_once('\t') else { continue; };
        let mut names = names.split(',').map(str::trim).filter(|name| !name.is_empty());
        let Some(family) = names.next() else { continue; };
        let font = fonts.entry(family.to_lowercase()).or_insert_with(|| InstalledFont {
            family: family.to_owned(), aliases: Vec::new(), monospace: false,
        });
        font.aliases.extend(names.map(str::to_owned));
        font.monospace |= matches!(spacing.trim(), "100" | "110");
    }
    Ok(fonts.into_values().map(|mut font| {
        font.aliases.sort();
        font.aliases.dedup();
        font
    }).collect())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn installed_fonts() -> Result<Vec<InstalledFont>> {
    anyhow::bail!("当前平台暂不支持枚举本机字体")
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn directwrite_reads_local_font_families_and_monospace_traits() {
        let fonts = super::installed_fonts().unwrap();
        assert!(!fonts.is_empty());
        assert!(fonts.iter().all(|font| !font.family.trim().is_empty()));
        if let Some(consolas) = fonts.iter().find(|font| font.family == "Consolas") {
            assert!(consolas.monospace);
        }
        eprintln!("DirectWrite: {} families, {} monospaced", fonts.len(), fonts.iter().filter(|font| font.monospace).count());
    }
}
