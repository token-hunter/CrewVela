use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;
use tauri::{AppHandle, Manager};

const MPKG_MAGIC: &str = "PKGM0014";
const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRY_COUNT: u32 = 512;
const MAX_ENTRY_NAME_BYTES: u32 = 1024;
const MAX_CONFIG_BYTES: u64 = 128 * 1024;
const MAX_AUDIO_BYTES: u64 = 512 * 1024 * 1024;
const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["mp3", "ogg", "wav", "m4a", "aac"];

#[derive(Clone, Debug)]
struct MpkgEntry {
    name: String,
    offset: u64,
    length: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDynamicSkin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub video_path: String,
    pub preview_path: Option<String>,
    pub audio_path: Option<String>,
    pub video_has_audio: bool,
    pub imported_at: i64,
    pub source_format: String,
}

fn read_u32(reader: &mut File) -> anyhow::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_string(reader: &mut File, length: u32) -> anyhow::Result<String> {
    anyhow::ensure!(
        length > 0 && length <= MAX_ENTRY_NAME_BYTES,
        "资源名称长度异常"
    );
    let mut bytes = vec![0_u8; length as usize];
    reader.read_exact(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}

fn safe_entry_name(name: &str) -> anyhow::Result<&str> {
    // Resource packages use Windows paths too, even when read on macOS/Linux.
    let normalized = name.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let drive_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let path = Path::new(&normalized);
    anyhow::ensure!(
        !normalized.is_empty()
            && !drive_prefix
            && !path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "资源包包含不安全路径: {name}"
    );
    Ok(name)
}

fn parse_directory_any(
    reader: &mut File,
    file_size: u64,
) -> anyhow::Result<(String, u64, Vec<MpkgEntry>)> {
    let magic_length = read_u32(reader)?;
    anyhow::ensure!(magic_length <= 32, "不支持的 MPKG 文件头");
    let magic = read_string(reader, magic_length)?;
    let count = read_u32(reader)?;
    anyhow::ensure!(count > 0 && count <= MAX_ENTRY_COUNT, "资源包数量异常");

    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name_length = read_u32(reader)?;
        let name = read_string(reader, name_length)?;
        safe_entry_name(&name)?;
        entries.push(MpkgEntry {
            name,
            offset: read_u32(reader)? as u64,
            length: read_u32(reader)? as u64,
        });
    }
    let payload_offset = reader.stream_position()?;
    let payload_length = file_size.saturating_sub(payload_offset);
    for entry in &entries {
        let end = entry
            .offset
            .checked_add(entry.length)
            .ok_or_else(|| anyhow::anyhow!("资源包偏移溢出"))?;
        anyhow::ensure!(
            end <= payload_length,
            "资源包内容越过文件边界: {}",
            entry.name
        );
    }
    Ok((magic, payload_offset, entries))
}

fn parse_directory(reader: &mut File, file_size: u64) -> anyhow::Result<(u64, Vec<MpkgEntry>)> {
    let (magic, payload_offset, entries) = parse_directory_any(reader, file_size)?;
    anyhow::ensure!(magic == MPKG_MAGIC, "不支持的 MPKG 版本: {magic}");
    Ok((payload_offset, entries))
}

fn parse_scene_directory(
    reader: &mut File,
    file_size: u64,
) -> anyhow::Result<(u64, Vec<MpkgEntry>)> {
    let (magic, payload_offset, entries) = parse_directory_any(reader, file_size)?;
    anyhow::ensure!(
        magic.starts_with("PKGV"),
        "不支持的 Wallpaper Engine 场景包: {magic}"
    );
    Ok((payload_offset, entries))
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn read_entry(
    reader: &mut File,
    payload_offset: u64,
    entry: &MpkgEntry,
    limit: u64,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(entry.length <= limit, "资源过大: {}", entry.name);
    reader.seek(SeekFrom::Start(payload_offset + entry.offset))?;
    let mut data = vec![0_u8; entry.length as usize];
    reader.read_exact(&mut data)?;
    Ok(data)
}

fn copy_entry(
    reader: &mut File,
    payload_offset: u64,
    entry: &MpkgEntry,
    destination: &Path,
    detect_audio: bool,
) -> anyhow::Result<bool> {
    reader.seek(SeekFrom::Start(payload_offset + entry.offset))?;
    let mut output = File::create(destination)?;
    let mut remaining = entry.length;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut tail = Vec::<u8>::new();
    let mut has_audio = false;
    while remaining > 0 {
        let take = usize::try_from(remaining.min(buffer.len() as u64))?;
        let read = reader.read(&mut buffer[..take])?;
        anyhow::ensure!(read > 0, "MPKG 资源提前结束: {}", entry.name);
        output.write_all(&buffer[..read])?;
        if detect_audio && !has_audio {
            let mut probe = Vec::with_capacity(tail.len() + read);
            probe.extend_from_slice(&tail);
            probe.extend_from_slice(&buffer[..read]);
            has_audio = probe.windows(4).any(|window| window == b"soun");
            tail.clear();
            tail.extend_from_slice(&probe[probe.len().saturating_sub(3)..]);
        }
        remaining -= read as u64;
    }
    output.sync_all()?;
    Ok(has_audio)
}

fn project_value(project: Option<&serde_json::Value>, key: &str) -> Option<String> {
    project?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn choose_entry<'a>(
    entries: &'a [MpkgEntry],
    requested: Option<&str>,
    extensions: &[&str],
) -> Option<&'a MpkgEntry> {
    requested
        .and_then(|name| {
            entries
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(name)
                    && extensions.contains(&extension(&entry.name).as_str()))
        })
        .or_else(|| {
            entries
                .iter()
                .filter(|entry| extensions.contains(&extension(&entry.name).as_str()))
                .max_by_key(|entry| entry.length)
        })
}

fn skin_root(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join("skins"))
}

fn safe_skin_directory_in_root(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        id.starts_with("dynamic-")
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
        "动态皮肤 ID 无效"
    );
    Ok(root.join(id))
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let candidate = path.to_string_lossy();
    if path.is_dir()
        && !paths.iter().any(|item| {
            item.to_string_lossy()
                .eq_ignore_ascii_case(candidate.as_ref())
        })
    {
        paths.push(path);
    }
}

#[cfg(target_os = "windows")]
fn registry_steam_path(key: &str, value: &str) -> Option<PathBuf> {
    let output = Command::new("reg")
        .args(["query", key, "/v", value])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, value)| value.trim()))?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(path) = std::env::var("STEAM_PATH") {
        push_unique_path(&mut roots, PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    {
        for (key, value) in [
            (r"HKCU\Software\Valve\Steam", "SteamPath"),
            (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
            (r"HKLM\SOFTWARE\Valve\Steam", "InstallPath"),
        ] {
            if let Some(path) = registry_steam_path(key, value) {
                push_unique_path(&mut roots, path);
            }
        }
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(path) = std::env::var(variable) {
                push_unique_path(&mut roots, PathBuf::from(path).join("Steam"));
            }
        }
    }

    let base_roots = roots.clone();
    for root in base_roots {
        let config = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(contents) = fs::read_to_string(config) else {
            continue;
        };
        for line in contents.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("\"path\"") {
                continue;
            }
            let parts = trimmed.split('"').collect::<Vec<_>>();
            if let Some(value) = parts.get(3) {
                push_unique_path(&mut roots, PathBuf::from(value.replace(r"\\", r"\")));
            }
        }
    }
    roots
}

fn find_workshop_scene_package(workshop_id: &str) -> Option<PathBuf> {
    if workshop_id.is_empty() || !workshop_id.chars().all(|value| value.is_ascii_digit()) {
        return None;
    }
    steam_library_roots().into_iter().find_map(|root| {
        let scene = root
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("431960")
            .join(workshop_id)
            .join("scene.pkg");
        scene.is_file().then_some(scene)
    })
}

fn extract_scene_audio(scene: &Path, destination: &Path) -> anyhow::Result<Option<PathBuf>> {
    let metadata = fs::metadata(scene)?;
    anyhow::ensure!(metadata.is_file(), "Wallpaper Engine 场景包不存在");
    anyhow::ensure!(metadata.len() <= MAX_PACKAGE_BYTES, "场景包不能超过 2 GB");
    let mut reader = File::open(scene)?;
    let (payload_offset, entries) = parse_scene_directory(&mut reader, metadata.len())?;
    let Some(audio) = choose_entry(&entries, None, SUPPORTED_AUDIO_EXTENSIONS) else {
        return Ok(None);
    };
    let path = destination.join(format!("sound.{}", extension(&audio.name)));
    copy_entry(&mut reader, payload_offset, audio, &path, false)?;
    Ok(Some(path))
}

pub fn import_mpkg(app: &AppHandle, source_path: &str) -> anyhow::Result<ImportedDynamicSkin> {
    import_mpkg_into_root(Path::new(source_path), &skin_root(app)?)
}

fn import_mpkg_into_root(source: &Path, root: &Path) -> anyhow::Result<ImportedDynamicSkin> {
    anyhow::ensure!(
        source
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("mpkg")),
        "请选择 .mpkg 文件"
    );
    let metadata = fs::metadata(source)?;
    anyhow::ensure!(metadata.is_file(), "MPKG 文件不存在");
    anyhow::ensure!(
        metadata.len() <= MAX_PACKAGE_BYTES,
        "MPKG 文件不能超过 2 GB"
    );

    let mut reader = File::open(source)?;
    let (payload_offset, entries) = parse_directory(&mut reader, metadata.len())?;
    let project_entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("project.json"));
    let project = project_entry
        .map(|entry| read_entry(&mut reader, payload_offset, entry, 2 * 1024 * 1024))
        .transpose()?
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());

    let requested_video = project_value(project.as_ref(), "file");
    let video = choose_entry(
        &entries,
        requested_video.as_deref(),
        &["mp4", "webm", "mov"],
    )
    .ok_or_else(|| anyhow::anyhow!("资源包中没有可播放的 MP4/WebM 视频"))?;
    let requested_preview = project_value(project.as_ref(), "preview");
    let preview = choose_entry(
        &entries,
        requested_preview.as_deref(),
        &["gif", "png", "jpg", "jpeg", "webp"],
    );
    let audio = choose_entry(&entries, None, &["mp3", "ogg", "wav", "m4a", "aac"]);

    let id = format!("dynamic-{}", uuid::Uuid::new_v4());
    fs::create_dir_all(&root)?;
    let staging = root.join(format!("{id}.tmp"));
    let destination = root.join(&id);
    fs::create_dir(&staging)?;

    let result = (|| -> anyhow::Result<ImportedDynamicSkin> {
        let video_extension = extension(&video.name);
        let video_path = staging.join(format!("background.{video_extension}"));
        let video_has_audio = copy_entry(&mut reader, payload_offset, video, &video_path, true)?;

        let preview_path = preview
            .map(|entry| -> anyhow::Result<PathBuf> {
                let path = staging.join(format!("preview.{}", extension(&entry.name)));
                copy_entry(&mut reader, payload_offset, entry, &path, false)?;
                Ok(path)
            })
            .transpose()?;
        let mut audio_path = audio
            .map(|entry| -> anyhow::Result<PathBuf> {
                let path = staging.join(format!("sound.{}", extension(&entry.name)));
                copy_entry(&mut reader, payload_offset, entry, &path, false)?;
                Ok(path)
            })
            .transpose()?;
        if audio_path.is_none() && !video_has_audio {
            let workshop_id = project_value(project.as_ref(), "workshopid").or_else(|| {
                source
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .filter(|value| value.chars().all(|character| character.is_ascii_digit()))
                    .map(str::to_owned)
            });
            audio_path = workshop_id
                .as_deref()
                .and_then(find_workshop_scene_package)
                .and_then(|scene| extract_scene_audio(&scene, &staging).ok().flatten());
        }

        fs::rename(&staging, &destination)?;
        let final_video = destination.join(video_path.file_name().unwrap_or_default());
        let final_preview = preview_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| destination.join(name));
        let final_audio = audio_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| destination.join(name));
        let fallback_name = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("动态皮肤");
        let name =
            project_value(project.as_ref(), "title").unwrap_or_else(|| fallback_name.to_owned());
        let description = project_value(project.as_ref(), "description")
            .unwrap_or_else(|| "从 MPKG 导入的本地动态皮肤".to_owned());
        let receipt = ImportedDynamicSkin {
            id,
            name: name.chars().take(80).collect(),
            description: description.chars().take(180).collect(),
            video_path: final_video.to_string_lossy().into_owned(),
            preview_path: final_preview.map(|path| path.to_string_lossy().into_owned()),
            audio_path: final_audio.map(|path| path.to_string_lossy().into_owned()),
            video_has_audio,
            imported_at: chrono::Utc::now().timestamp_millis(),
            source_format: MPKG_MAGIC.to_owned(),
        };
        fs::write(
            destination.join("manifest.json"),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
        Ok(receipt)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&destination);
    }
    result
}

pub fn delete_dynamic_skin(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    delete_dynamic_skin_from_root(&skin_root(app)?, id)
}

fn delete_dynamic_skin_from_root(root: &Path, id: &str) -> anyhow::Result<()> {
    let directory = safe_skin_directory_in_root(root, id)?;
    if directory.exists() {
        fs::remove_dir_all(directory)?;
    }
    Ok(())
}

pub fn attach_dynamic_skin_audio(
    app: &AppHandle,
    id: &str,
    source_path: &str,
) -> anyhow::Result<String> {
    attach_dynamic_skin_audio_into_root(&skin_root(app)?, id, Path::new(source_path))
}

fn attach_dynamic_skin_audio_into_root(
    root: &Path,
    id: &str,
    source: &Path,
) -> anyhow::Result<String> {
    let directory = safe_skin_directory_in_root(root, id)?;
    anyhow::ensure!(directory.is_dir(), "要修改的动态皮肤不存在");
    let audio_extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    anyhow::ensure!(
        SUPPORTED_AUDIO_EXTENSIONS.contains(&audio_extension.as_str()),
        "请选择 MP3、OGG、WAV、M4A 或 AAC 音频"
    );
    let metadata = fs::metadata(source)?;
    anyhow::ensure!(metadata.is_file(), "BGM 文件不存在");
    anyhow::ensure!(metadata.len() <= MAX_AUDIO_BYTES, "BGM 文件不能超过 512 MB");

    let destination = directory.join(format!("sound.{audio_extension}"));
    let manifest_path = directory.join("manifest.json");
    // Validate the manifest before touching existing BGM. Stage both writes so
    // selecting the current BGM as source is safe and failed writes retain it.
    let manifest = if manifest_path.is_file() {
        let mut manifest = serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest_path)?)?;
        anyhow::ensure!(manifest.is_object(), "皮肤 manifest 必须是 JSON 对象");
        manifest["audioPath"] = serde_json::Value::String(destination.to_string_lossy().into_owned());
        Some(serde_json::to_vec_pretty(&manifest)?)
    } else { None };
    let token = uuid::Uuid::new_v4();
    let staging = directory.join(format!(".audio-{token}.tmp"));
    let manifest_staging = directory.join(format!(".manifest-{token}.tmp"));
    let backup = directory.join(format!(".audio-{token}.backup"));
    let result = (|| -> anyhow::Result<()> {
        fs::copy(source, &staging)?;
        if let Some(contents) = &manifest { fs::write(&manifest_staging, contents)?; }
        let had_previous = destination.exists();
        if had_previous { fs::rename(&destination, &backup)?; }
        let replacement = (|| -> anyhow::Result<()> {
            fs::rename(&staging, &destination)?;
            if manifest.is_some() { fs::rename(&manifest_staging, &manifest_path)?; }
            Ok(())
        })();
        if let Err(error) = replacement {
            // Remove only our replacement; restore the previous file if present.
            if destination.exists() { fs::remove_file(&destination)?; }
            if had_previous { fs::rename(&backup, &destination)?; }
            return Err(error);
        }
        if had_previous { fs::remove_file(&backup)?; }
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path != destination && path.is_file()
                && path.file_stem().and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("sound")) {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    })();
    for temporary in [&staging, &manifest_staging] {
        if temporary.exists() { fs::remove_file(temporary)?; }
    }
    result?;
    Ok(destination.to_string_lossy().into_owned())
}

pub fn read_skin_config(source_path: &str) -> anyhow::Result<String> {
    let source = Path::new(source_path);
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    anyhow::ensure!(
        extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("ccskin"),
        "请选择 .json 或 .ccskin 文件"
    );
    let metadata = fs::metadata(source)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= MAX_CONFIG_BYTES,
        "皮肤配置不能超过 128 KB"
    );
    Ok(fs::read_to_string(source)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_package_with_magic(magic: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&(magic.len() as u32).to_le_bytes());
        output.extend_from_slice(magic.as_bytes());
        output.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        let mut offset = 0_u32;
        for (name, data) in entries {
            output.extend_from_slice(&(name.len() as u32).to_le_bytes());
            output.extend_from_slice(name.as_bytes());
            output.extend_from_slice(&offset.to_le_bytes());
            output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            offset += data.len() as u32;
        }
        for (_, data) in entries {
            output.extend_from_slice(data);
        }
        output
    }

    fn sample_package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        sample_package_with_magic(MPKG_MAGIC, entries)
    }

    #[test]
    fn parses_pkgm0014_directory() {
        let bytes = sample_package(&[("project.json", b"{}"), ("wallpaper.mp4", b"video")]);
        let temp = std::env::temp_dir().join(format!("crewvela-mpkg-{}.mpkg", uuid::Uuid::new_v4()));
        fs::write(&temp, bytes).unwrap();
        let mut file = File::open(&temp).unwrap();
        let size = file.metadata().unwrap().len();
        let (_, entries) = parse_directory(&mut file, size).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].name, "wallpaper.mp4");
        let _ = fs::remove_file(temp);
    }

    #[test]
    fn rejects_parent_directory_entries() {
        for name in ["", "../escape.mp4", "C:\\escape.mp4", "C:/escape.mp4", "C:escape.mp4", "\\\\server\\share\\escape.mp4", "\\escape.mp4", "sounds\\..\\escape.mp4", "sounds/../escape.mp4", "sounds\\../escape.mp4"] {
            assert!(safe_entry_name(name).is_err(), "unsafe package entry: {name}");
        }
        for name in ["wallpaper.mp4", "sounds/music.mp3", "sounds\\music.mp3", "视频/背景.mp4"] {
            assert!(safe_entry_name(name).is_ok(), "valid package entry: {name}");
        }
    }

    #[test]
    fn extracts_bgm_from_wallpaper_engine_scene_package() {
        let root =
            std::env::temp_dir().join(format!("crewvela-scene-audio-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let scene = root.join("scene.pkg");
        fs::write(
            &scene,
            sample_package_with_magic(
                "PKGV0012",
                &[
                    ("sounds/Untitled.mp3", b"wallpaper-engine-bgm"),
                    ("scene.json", b"{}"),
                ],
            ),
        )
        .unwrap();

        let extracted = extract_scene_audio(&scene, &root).unwrap().unwrap();
        assert_eq!(extracted.file_name().unwrap(), "sound.mp3");
        assert_eq!(fs::read(extracted).unwrap(), b"wallpaper-engine-bgm");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_requested_video_falls_back_to_supported_package_entry() {
        let root = std::env::temp_dir().join(format!("skin-video-fallback-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fixture.mpkg");
        fs::write(&source, sample_package(&[
            ("project.json", br#"{"file":"scene.pkg"}"#),
            ("scene.pkg", b"unsupported-scene-package-larger-than-the-video"),
            ("small.mp4", b"small"),
            ("fallback.WEBM", b"supported-video-soun"),
        ])).unwrap();
        let receipt = import_mpkg_into_root(&source, &root.join("skins")).unwrap();
        assert_eq!(Path::new(&receipt.video_path).file_name().unwrap(), "background.webm");
        assert_eq!(fs::read(&receipt.video_path).unwrap(), b"supported-video-soun");
        assert!(!Path::new(&receipt.video_path).parent().unwrap().join("background.pkg").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn requested_non_video_without_fallback_is_rejected_without_import_artifacts() {
        let root = std::env::temp_dir().join(format!("skin-video-rejected-{}", uuid::Uuid::new_v4()));
        let skins = root.join("skins");
        fs::create_dir_all(&skins).unwrap();
        fs::write(skins.join("existing-skin"), b"keep-existing").unwrap();
        let source = root.join("fixture.mpkg");
        fs::write(&source, sample_package(&[
            ("project.json", br#"{"file":"scene.pkg","preview":"preview.png"}"#),
            ("scene.pkg", b"unsupported-scene-package"),
            ("preview.png", b"preview"),
        ])).unwrap();
        let error = import_mpkg_into_root(&source, &skins).unwrap_err();
        assert!(error.to_string().contains("没有可播放"));
        let artifacts: Vec<_> = fs::read_dir(&skins).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(artifacts, vec![std::ffi::OsString::from("existing-skin")]);
        assert_eq!(fs::read(skins.join("existing-skin")).unwrap(), b"keep-existing");
        let absent_root = root.join("not-created");
        assert!(import_mpkg_into_root(&source, &absent_root).is_err());
        assert!(!absent_root.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_requested_entry_obeys_image_whitelist_and_keeps_valid_exact_precedence() {
        let root = std::env::temp_dir().join(format!("skin-preview-types-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fixture.mpkg");
        for preview_extension in [None, Some("GIF"), Some("png"), Some("jpg"), Some("jpeg"), Some("webp")] {
            // An invalid exact preview request must not turn a video/pkg into an
            // image. A valid requested image still wins over a larger fallback.
            let requested = preview_extension.map(|extension| format!("requested.{extension}")).unwrap_or_else(|| "scene.pkg".into());
            let project = serde_json::to_vec(&serde_json::json!({"file":"video.mp4", "preview":requested})).unwrap();
            let mut entries: Vec<(&str, &[u8])> = vec![
                ("project.json", &project), ("video.mp4", b"video-soun"),
                ("scene.pkg", b"not-an-image"),
            ];
            if preview_extension.is_some() {
                entries.push((&requested, b"requested-image"));
                entries.push(("largest.png", b"larger-fallback-image-must-not-override-valid-request"));
            }
            fs::write(&source, sample_package(&entries)).unwrap();
            let receipt = import_mpkg_into_root(&source, &root.join("skins")).unwrap();
            match preview_extension {
                Some(extension) => {
                    let preview = receipt.preview_path.unwrap();
                    assert_eq!(Path::new(&preview).extension().unwrap().to_str().unwrap(), extension.to_ascii_lowercase());
                    assert_eq!(fs::read(preview).unwrap(), b"requested-image");
                }
                None => assert!(receipt.preview_path.is_none()),
            }
        }
        // Invalid exact match with a legal image candidate must use the image.
        fs::write(&source, sample_package(&[
            ("project.json", br#"{"file":"video.mp4","preview":"video.mp4"}"#),
            ("video.mp4", b"video-soun"), ("fallback.PNG", b"fallback-image"),
        ])).unwrap();
        let receipt = import_mpkg_into_root(&source, &root.join("skins")).unwrap();
        let preview = receipt.preview_path.unwrap();
        assert_eq!(Path::new(&preview).file_name().unwrap(), "preview.png");
        assert_eq!(fs::read(preview).unwrap(), b"fallback-image");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_configured_external_package() {
        let Ok(source) = std::env::var("CREWVELA_TEST_MPKG") else {
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "crewvela-dynamic-skin-test-{}",
            uuid::Uuid::new_v4()
        ));
        let receipt = import_mpkg_into_root(Path::new(&source), &root).unwrap();
        eprintln!("{receipt:#?}");
        assert!(Path::new(&receipt.video_path).is_file());
        assert_eq!(receipt.source_format, MPKG_MAGIC);
        if let Some(preview) = receipt.preview_path {
            assert!(Path::new(&preview).is_file());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attaches_replaces_and_deletes_skin_audio() {
        let root = std::env::temp_dir().join(format!(
            "crewvela-dynamic-skin-audio-test-{}",
            uuid::Uuid::new_v4()
        ));
        let id = "dynamic-11111111-1111-1111-1111-111111111111";
        let directory = root.join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("manifest.json"), b"{}").unwrap();
        let first = root.join("first.mp3");
        let second = root.join("second.ogg");
        fs::write(&first, b"first-audio").unwrap();
        fs::write(&second, b"second-audio").unwrap();

        let first_path = attach_dynamic_skin_audio_into_root(&root, id, &first).unwrap();
        assert!(Path::new(&first_path).is_file());
        let repeated = attach_dynamic_skin_audio_into_root(&root, id, Path::new(&first_path)).unwrap();
        assert_eq!(repeated, first_path);
        assert_eq!(fs::read(&first_path).unwrap(), b"first-audio");
        fs::write(directory.join("manifest.json"), b"invalid json").unwrap();
        assert!(attach_dynamic_skin_audio_into_root(&root, id, &second).is_err());
        assert_eq!(fs::read(&first_path).unwrap(), b"first-audio");
        assert_eq!(fs::read(directory.join("manifest.json")).unwrap(), b"invalid json");
        fs::write(directory.join("manifest.json"), b"{}").unwrap();
        let second_path = attach_dynamic_skin_audio_into_root(&root, id, &second).unwrap();
        assert!(!Path::new(&first_path).exists());
        assert_eq!(fs::read(&second_path).unwrap(), b"second-audio");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["audioPath"].as_str(), Some(second_path.as_str()));

        assert!(!fs::read_dir(&directory).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with('.')));
        delete_dynamic_skin_from_root(&root, id).unwrap();
        assert!(!directory.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
