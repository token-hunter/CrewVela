use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use super::MultiAgentStore;

const PREFIX: &str = "imported:";
const MAX_SOURCE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_SOURCE_EDGE: u32 = 4096;
const AVATAR_EDGE: u32 = 192;

pub(super) fn is_imported_avatar(id: &str) -> bool {
    id.strip_prefix(PREFIX).is_some_and(|hash| {
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn prepare_avatar(path: &Path) -> Result<Vec<u8>> {
    ensure!(path.is_absolute() && path.is_file(), "请选择本地图片文件");
    let file = File::open(path).context("无法读取头像文件")?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).context("无法读取头像文件")?;
    ensure!(bytes.len() as u64 <= MAX_SOURCE_BYTES, "头像图片不能超过 10 MB");
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    ensure!(matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)),
        "请选择 PNG、JPG 或 WebP 图片");
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().context("无法读取图片，头像宽高最多为 4096 像素")?;
    let (width, height) = decoder.dimensions();
    ensure!((1..=MAX_SOURCE_EDGE).contains(&width) && (1..=MAX_SOURCE_EDGE).contains(&height),
        "头像宽高最多为 4096 像素");
    let orientation = decoder.orientation().context("无法读取图片方向")?;
    let mut avatar = DynamicImage::from_decoder(decoder).context("无法解码头像图片")?;
    avatar.apply_orientation(orientation);
    let avatar = avatar.resize_to_fill(AVATAR_EDGE, AVATAR_EDGE, image::imageops::FilterType::Triangle).into_rgba8();
    let mut png = Cursor::new(Vec::new());
    avatar.write_to(&mut png, ImageFormat::Png).context("无法生成头像预览")?;
    Ok(png.into_inner())
}

impl MultiAgentStore {
    pub(crate) fn import_agent_avatar(&self, path: &Path) -> Result<String> {
        let png = prepare_avatar(path)?;
        let id = format!("{PREFIX}{:x}", Sha256::digest(&png));
        self.connection.lock().unwrap().execute(
            "INSERT INTO ma_agent_avatar_images (id, png) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
            params![id, png],
        )?;
        Ok(id)
    }

    pub(crate) fn agent_avatar_image(&self, id: &str) -> Result<String> {
        ensure!(is_imported_avatar(id), "无效的自定义头像");
        let png = self.connection.lock().unwrap().query_row(
            "SELECT png FROM ma_agent_avatar_images WHERE id = ?1", [id], |row| row.get::<_, Vec<u8>>(0),
        ).optional()?.context("自定义头像不存在，请重新导入")?;
        Ok(format!("data:image/png;base64,{}", STANDARD.encode(png)))
    }

    pub(super) fn check_agent_avatar(&self, id: Option<&str>) -> Result<()> {
        super::validate_agent_avatar(id)?;
        if let Some(id) = id.filter(|id| is_imported_avatar(id)) {
            let exists: bool = self.connection.lock().unwrap().query_row(
                "SELECT EXISTS(SELECT 1 FROM ma_agent_avatar_images WHERE id = ?1)", [id], |row| row.get(0),
            )?;
            ensure!(exists, "自定义头像不存在，请重新导入");
        }
        Ok(())
    }
}
