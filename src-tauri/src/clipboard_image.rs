use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// 临时文件名前缀，用于启动时清理残留（当前未启用清理，仅预留可识别命名）。
const FILENAME_PREFIX: &str = "crewvela-clip-";

fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        })
        .unwrap_or(false)
}

fn validate_dropped_rgba(file_name: &str, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    if !is_supported_image_path(Path::new(file_name)) {
        return Err(anyhow!("只支持 PNG、JPG、JPEG、GIF 和 WebP 图片"));
    }
    if width == 0 || height == 0 {
        return Err(anyhow!("图片尺寸无效"));
    }
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow!("图片尺寸过大"))?;
    if rgba.len() != expected_len {
        return Err(anyhow!(
            "图片像素数据长度无效：期望 {expected_len}，实际 {}",
            rgba.len()
        ));
    }
    Ok(())
}

/// 读取剪贴板 CF_HDROP 文件列表（Windows；其余平台恒为空）。
#[cfg(windows)]
fn clipboard_files() -> Vec<PathBuf> {
    use clipboard_win::formats::FileList;
    use clipboard_win::{Clipboard, Getter};

    let Ok(_clipboard) = Clipboard::new_attempts(10) else {
        return Vec::new();
    };
    let mut paths = Vec::<PathBuf>::new();
    if FileList.read_clipboard(&mut paths).is_err() {
        return Vec::new();
    }
    paths
}

#[cfg(not(windows))]
fn clipboard_files() -> Vec<PathBuf> {
    Vec::new()
}

fn clipboard_supported_image_files() -> Vec<PathBuf> {
    clipboard_files()
        .into_iter()
        .filter(|path| is_supported_image_path(path))
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImageFile {
    file_name: String,
    bytes: Vec<u8>,
}

/// 给定毫秒时间戳与一次性 nonce，生成形如 `crewvela-clip-<ts_ms>-<nonce>.png` 的文件名。
///
/// 抽出来是为便于单测；nonce 由调用方保证唯一性（随机串）。
fn clip_image_filename(ts_ms: u64, nonce: &str) -> String {
    format!("{FILENAME_PREFIX}{ts_ms}-{nonce}.png")
}

/// 把 RGBA 像素缓冲编码成 PNG 字节。
fn encode_rgba_as_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut buf), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("PNG 写头失败")?;
        writer.write_image_data(rgba).context("PNG 写像素失败")?;
    }
    Ok(buf)
}

/// PNG 文件签名，用于识别 plugin 返回的字节是否已经是 PNG 编码。
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 从 `tauri-plugin-clipboard-manager` 返回的 `Image` 字段提取合法 PNG 字节。
///
/// plugin 2.0.0 的 `read_image` 把 PNG 编码字节错误地塞进 `Image::rgba` 字段
/// （违反 tauri `Image` 契约——`rgba` 本应是 RGBA 像素数据）。这里通过签名识别做兼容：
/// - 字段以 PNG 签名开头 → 当作 PNG 字节直接返回（plugin bug 形态）
/// - 否则视作 RGBA → 编码成 PNG（plugin 修复后的形态）
/// - 既不是 PNG 也不是合法 RGBA → 报错
fn extract_png_bytes(rgba_field: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    if rgba_field.starts_with(PNG_SIGNATURE) {
        return Ok(rgba_field.to_vec());
    }
    if width == 0 || height == 0 {
        return Err(anyhow!("剪贴板图像尺寸无效: {width}x{height}"));
    }
    encode_rgba_as_png(rgba_field, width, height)
}

/// 轻量检查剪贴板是否含位图（供前端决定走原生图片粘贴还是文本粘贴）。
///
/// 用 `read_image` 探测：无图或解码失败都视为 false。成功返回的 Image 数据被丢弃，
/// 真正读取由 claude 自己完成（前端发 Alt+V 让 claude 读系统剪贴板）。
pub fn clipboard_has_image(app: &AppHandle) -> bool {
    app.clipboard()
        .read_image()
        .map(|image| image.width() > 0 && image.height() > 0)
        .unwrap_or(false)
        || !clipboard_supported_image_files().is_empty()
}

pub fn read_clipboard_image_file() -> Result<Option<ClipboardImageFile>> {
    let Some(path) = clipboard_supported_image_files().into_iter().next() else {
        return Ok(None);
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("剪贴板图片文件名无效"))?
        .to_owned();
    let bytes = std::fs::read(&path)
        .with_context(|| format!("读取剪贴板图片文件失败: {}", path.display()))?;
    Ok(Some(ClipboardImageFile { file_name, bytes }))
}

pub fn write_dropped_image_to_clipboard(
    app: &AppHandle,
    file_name: &str,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<()> {
    validate_dropped_rgba(file_name, &rgba, width, height)?;
    let image = tauri::image::Image::new_owned(rgba, width, height);
    app.clipboard()
        .write_image(&image)
        .context("写入拖放图片到剪贴板失败")
}

/// 把剪贴板里的位图（若有）写成系统临时目录下的 PNG，返回其绝对路径。
///
/// 剪贴板没有位图时返回 `Ok(None)`，调用方应回落到 `read_text`。
/// 任意环节出错返回 `Err`，调用方仍可自行决定是否回落。
pub fn write_clipboard_image_to_temp(app: &AppHandle) -> Result<Option<String>> {
    let clipboard = app.clipboard();
    let image = match clipboard.read_image() {
        Ok(image) => image,
        Err(_) => return Ok(None),
    };
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let rgba = image.rgba();
    let png_bytes = extract_png_bytes(&rgba, width, height)?;
    let ts_ms = now_ms();
    let nonce = random_nonce();
    let filename = clip_image_filename(ts_ms, &nonce);
    let path: PathBuf = std::env::temp_dir().join(filename);
    std::fs::write(&path, &png_bytes)
        .with_context(|| format!("写入剪贴板临时图片失败: {}", path.display()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 与 `native_zcode_sessions::prepare_attachments` 的 composer 限额保持一致，粘贴入口提前校验。
pub const PASTED_FILE_COUNT_LIMIT: usize = 8;
pub const PASTED_FILE_SIZE_LIMIT: u64 = 20 * 1024 * 1024;

/// WebView 粘贴/拖放事件给出的文件（名字 + 字节），由前端整包传入。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedFilePayload {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// 粘贴文件名净化：只取 basename（WebView 可能给出带路径的名字），剔除控制字符与
/// 首尾空白；其余字符（含中文、空格、括号）原样保留。无法得到可用名字时返回 None。
fn sanitize_pasted_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        None
    } else {
        Some(cleaned)
    }
}

/// 剪贴板 CF_HDROP 里的全部文件路径（资源管理器“复制”后粘贴的原始文件，
/// 直接引用原路径，不落临时副本）。非 Windows 恒为空。
pub fn clipboard_file_paths() -> Vec<String> {
    clipboard_files()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 把 WebView 给出的文件字节写入系统临时目录，返回绝对路径列表（供附件通路引用）。
///
/// 这是系统剪贴板直读（位图/CF_HDROP）都拿不到时的兜底，例如 WebView 合成文件。
pub fn write_pasted_files_to_temp(files: Vec<PastedFilePayload>) -> Result<Vec<String>> {
    write_pasted_files_in(&std::env::temp_dir(), files)
}

fn write_pasted_files_in(dir: &Path, files: Vec<PastedFilePayload>) -> Result<Vec<String>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if files.len() > PASTED_FILE_COUNT_LIMIT {
        bail!("每次最多添加 {} 个附件", PASTED_FILE_COUNT_LIMIT);
    }
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let name =
            sanitize_pasted_file_name(&file.name).ok_or_else(|| anyhow!("附件文件名不可用"))?;
        if file.bytes.len() as u64 > PASTED_FILE_SIZE_LIMIT {
            bail!("CrewVela 附件上限为每个 20 MiB");
        }
        let path = dir.join(format!(
            "{FILENAME_PREFIX}{}-{}-{name}",
            now_ms(),
            random_nonce()
        ));
        std::fs::write(&path, &file.bytes)
            .with_context(|| format!("写入粘贴附件失败: {}", path.display()))?;
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}


/// 6 位 base32 随机串，足够避免同毫秒内冲突。
fn random_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mixed = (nanos as u64)
        .wrapping_mul(2654435761)
        .wrapping_add(now_ms());
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut s = String::with_capacity(6);
    let mut x = mixed;
    for _ in 0..6 {
        s.push(alphabet[(x % 32) as usize] as char);
        x /= 32;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_image_paths_cover_terminal_drop_contract() {
        for name in [
            "screen.png",
            "photo.JPG",
            "shot.jpeg",
            "anim.gif",
            "card.webp",
        ] {
            assert!(
                is_supported_image_path(Path::new(name)),
                "{name} should be accepted"
            );
        }
        for name in ["notes.txt", "archive.zip", "image.svg", "no-extension"] {
            assert!(
                !is_supported_image_path(Path::new(name)),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn dropped_rgba_validation_rejects_invalid_payloads() {
        assert!(validate_dropped_rgba("image.png", &[0, 0, 0, 255], 1, 1).is_ok());
        assert!(validate_dropped_rgba("notes.txt", &[0, 0, 0, 255], 1, 1).is_err());
        assert!(validate_dropped_rgba("image.png", &[0, 0, 0], 1, 1).is_err());
        assert!(validate_dropped_rgba("image.png", &[], 0, 0).is_err());
    }

    #[test]
    fn clip_filename_matches_contract() {
        assert_eq!(
            clip_image_filename(1_700_000_000_000, "abc123"),
            "crewvela-clip-1700000000000-abc123.png"
        );
    }

    #[test]
    fn encode_rgba_roundtrips_as_valid_png() {
        // 2×2 全不透明红色 RGBA
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 255, 255, 0, 255,
        ];
        let bytes = encode_rgba_as_png(&rgba, 2, 2).unwrap();
        let decoder = png::Decoder::new(Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        let mut out = vec![0u8; rgba.len()];
        reader.next_frame(&mut out).unwrap();
        assert_eq!(out, rgba);
    }

    #[test]
    fn encode_rgba_rejects_size_mismatch() {
        // 像素数据长度与 width*height*4 不符：write_image_data 应失败。
        let rgba = [255, 0, 0, 255]; // 仅 1 像素
        let result = encode_rgba_as_png(&rgba, 2, 2);
        assert!(result.is_err());
    }

    #[test]
    fn extract_passthrough_when_field_already_png() {
        // 模拟 plugin 2.0.0 bug：rgba 字段里塞的是 PNG 字节。
        // 先造一张真 PNG（encode 一张 1×1 红），喂回 extract 应原样返回。
        let one_pixel_rgba = [255, 0, 0, 255];
        let png = encode_rgba_as_png(&one_pixel_rgba, 1, 1).unwrap();
        assert!(png.starts_with(PNG_SIGNATURE));
        let out = extract_png_bytes(&png, 1, 1).unwrap();
        assert_eq!(out, png);
    }

    #[test]
    fn extract_encodes_when_field_is_raw_rgba() {
        // 真 RGBA 数据（无 PNG 签名）→ 编码成 PNG。
        let rgba = [255, 0, 0, 255];
        let out = extract_png_bytes(&rgba, 1, 1).unwrap();
        assert!(out.starts_with(PNG_SIGNATURE));
        // 解码回来像素一致
        let decoder = png::Decoder::new(Cursor::new(&out));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = [0u8; 4];
        reader.next_frame(&mut pixels).unwrap();
        assert_eq!(pixels, rgba);
    }

    #[test]
    fn extract_rejects_invalid_size_without_png_signature() {
        // 非 PNG 签名 + 尺寸 0 → 报错
        assert!(extract_png_bytes(&[1, 2, 3, 4], 0, 0).is_err());
        assert!(extract_png_bytes(&[1, 2, 3, 4], 0, 10).is_err());
    }

    #[test]
    fn random_nonce_is_six_chars_and_alphanumeric() {
        let nonce = random_nonce();
        assert_eq!(nonce.len(), 6);
        assert!(nonce.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn pasted_names_strip_paths_and_control_chars() {
        assert_eq!(
            sanitize_pasted_file_name("..\\..\\evil.png").as_deref(),
            Some("evil.png")
        );
        assert_eq!(sanitize_pasted_file_name("a/b/c.txt").as_deref(), Some("c.txt"));
        assert_eq!(
            sanitize_pasted_file_name(" 截图 (1).png ").as_deref(),
            Some("截图 (1).png")
        );
        assert_eq!(sanitize_pasted_file_name("bad\u{7}.png").as_deref(), Some("bad.png"));
        assert_eq!(sanitize_pasted_file_name(".."), None);
        assert_eq!(sanitize_pasted_file_name("   "), None);
        assert_eq!(sanitize_pasted_file_name(""), None);
    }

    #[test]
    fn pasted_files_write_sanitized_and_enforce_composer_limits() {
        let dir = std::env::temp_dir().join(format!("crewvela-paste-test-{}", random_nonce()));
        std::fs::create_dir_all(&dir).unwrap();

        let paths = write_pasted_files_in(
            &dir,
            vec![
                PastedFilePayload {
                    name: "..\\..\\shot.png".into(),
                    bytes: vec![1, 2, 3],
                },
                PastedFilePayload {
                    name: "笔记.txt".into(),
                    bytes: vec![4],
                },
            ],
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(Path::new(&paths[0]).is_absolute());
        assert!(paths[0].contains(FILENAME_PREFIX) && paths[0].ends_with("shot.png"));
        assert!(paths[1].ends_with("笔记.txt"));
        assert_eq!(std::fs::read(&paths[0]).unwrap(), vec![1, 2, 3]);

        let too_many: Vec<PastedFilePayload> = (0..=PASTED_FILE_COUNT_LIMIT)
            .map(|index| PastedFilePayload {
                name: format!("f{index}.txt"),
                bytes: vec![0],
            })
            .collect();
        assert!(write_pasted_files_in(&dir, too_many).is_err());

        let too_big = vec![PastedFilePayload {
            name: "big.png".into(),
            bytes: vec![0u8; PASTED_FILE_SIZE_LIMIT as usize + 1],
        }];
        assert!(write_pasted_files_in(&dir, too_big).is_err());

        let bad_name = vec![PastedFilePayload {
            name: "..".into(),
            bytes: vec![0],
        }];
        assert!(write_pasted_files_in(&dir, bad_name).is_err());

        assert!(write_pasted_files_in(&dir, Vec::new()).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
