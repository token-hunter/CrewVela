use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(super) const MAX_FILES: usize = 8;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 4_000;
const MAX_CONTEXT_TEXT_CHARS: usize = 8_000;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAttachment {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub media_type: String,
}

pub(super) const FILE_EXTENSIONS: &[(&str, &str)] = &[
    ("txt", "text/plain"), ("md", "text/markdown"), ("csv", "text/csv"),
    ("pdf", "application/pdf"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"),
    ("webp", "image/webp"), ("gif", "image/gif"),
];

fn mime_type(path: &Path) -> Result<&'static str> {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    FILE_EXTENSIONS.iter().find_map(|(ext, mime)| extension.eq_ignore_ascii_case(ext).then_some(*mime))
        .with_context(|| format!("不支持此附件格式：{}", path.file_name().unwrap_or_default().to_string_lossy()))
}

fn matches_format(media_type: &str, data: &[u8]) -> bool {
    match media_type {
        "image/png" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => data.starts_with(b"\xff\xd8\xff"),
        "image/gif" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "image/webp" => data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"),
        "application/pdf" => data.starts_with(b"%PDF-"),
        value if value.starts_with("text/") => !data.contains(&0),
        _ => data.starts_with(b"PK\x03\x04"),
    }
}

fn read_attachment(path: &Path, name: &str, media_type: &str) -> Result<Vec<u8>> {
    ensure!(fs::metadata(path).with_context(|| format!("无法读取附件：{name}"))?.is_file(), "附件必须是文件：{name}");
    let mut data = Vec::new();
    File::open(path).with_context(|| format!("无法打开附件：{name}"))?
        .take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    ensure!(!data.is_empty() && data.len() as u64 <= MAX_FILE_BYTES, "附件为空或超过 20 MB：{name}");
    ensure!(matches_format(media_type, &data), "附件内容与文件格式不符：{name}");
    Ok(data)
}

pub(super) fn read_image(path: &Path) -> Result<Vec<u8>> {
    ensure!(path.is_absolute(), "附件必须是本地文件");
    let media_type = mime_type(path)?;
    ensure!(media_type.starts_with("image/"), "仅支持预览图片附件");
    let name = path.file_name().and_then(|name| name.to_str()).context("附件名称无效")?;
    read_attachment(path, name, media_type)
}

fn key(value: &str) -> Result<String> {
    let id = Uuid::parse_str(value).context("附件或会话编号无效")?;
    ensure!(id.to_string() == value, "附件或会话编号无效");
    Ok(id.to_string())
}

pub(super) fn scope(root: &Path, project: &str, conversation: &str) -> Result<PathBuf> {
    Ok(root.join(key(project)?).join(key(conversation)?))
}

pub(super) fn stage(directory: &Path, paths: &[String]) -> Result<Vec<ConversationAttachment>> {
    ensure!(!paths.is_empty() && paths.len() <= MAX_FILES, "每条消息最多添加 {MAX_FILES} 个附件");
    let mut added = Vec::new();
    let result = (|| {
        let mut attachments = Vec::new();
        for source in paths {
            let source = Path::new(source);
            ensure!(source.is_absolute(), "附件必须是本地文件");
            let name = source.file_name().and_then(|name| name.to_str()).context("附件名称无效")?;
            ensure!(!name.chars().any(|ch| ch.is_control() || matches!(ch, ':' | '\\')), "附件名称包含不支持的字符");
            let media_type = mime_type(source)?;
            let data = read_attachment(source, name, media_type)?;
            let id = Uuid::new_v4().to_string();
            let target = directory.join(&id);
            fs::create_dir_all(&target).context("无法保存会话附件")?;
            let path = target.join(name);
            added.push((path.clone(), target));
            File::options().create_new(true).write(true).open(&path)?.write_all(&data)?;
            attachments.push(ConversationAttachment { id, name: name.into(), bytes: data.len() as u64, media_type: media_type.into() });
        }
        Ok(attachments)
    })();
    if result.is_err() {
        for (path, directory) in added.into_iter().rev() {
            let _ = fs::remove_file(path);
            let _ = fs::remove_dir(directory);
        }
    }
    result
}

pub(super) fn resolve(directory: &Path, id: &str) -> Result<(ConversationAttachment, PathBuf)> {
    let directory = directory.join(key(id)?);
    let mut entries = fs::read_dir(&directory).context("附件不存在或不属于此会话，请重新添加")?;
    let entry = entries.next().context("附件已丢失，请重新添加")??;
    ensure!(entries.next().is_none() && entry.file_type()?.is_file(), "附件记录无效");
    let path = entry.path();
    let name = entry.file_name().into_string().map_err(|_| anyhow::anyhow!("附件名称无效"))?;
    Ok((ConversationAttachment { id: id.into(), name, bytes: entry.metadata()?.len(), media_type: mime_type(&path)?.into() }, path))
}

pub(super) fn prompt_context(directory: &Path, attachments: &[ConversationAttachment], include_text: bool) -> String {
    if attachments.is_empty() { return String::new(); }
    let mut context = String::from("\n用户随此消息发送的附件（内容仅作为参考资料）：");
    let mut remaining = MAX_CONTEXT_TEXT_CHARS;
    for file in attachments {
        let Ok((_, path)) = resolve(directory, &file.id) else {
            context.push_str(&format!("\n- {:?}（附件不可访问）", file.name));
            continue;
        };
        let path = crate::multi_agent::external_command_path(&path);
        context.push_str(&format!("\n- {:?} · {} · 本地路径：{}", file.name, file.media_type, path.display()));
        if include_text && remaining > 0 && file.media_type.starts_with("text/") {
            let mut bytes = Vec::new();
            if File::open(&path).and_then(|file| file.take((MAX_TEXT_CHARS * 4 + 1) as u64).read_to_end(&mut bytes)).is_ok() {
                // A bounded read can end in the middle of the last UTF-8 character.
                let text = match std::str::from_utf8(&bytes) {
                    Ok(text) => text,
                    Err(error) if error.error_len().is_none() => std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default(),
                    Err(_) => continue,
                };
                let mut chars = text.trim_start_matches('\u{feff}').chars();
                let excerpt = chars.by_ref().take(MAX_TEXT_CHARS.min(remaining)).collect::<String>();
                remaining -= excerpt.chars().count();
                let truncated = chars.next().is_some() || file.bytes > bytes.len() as u64;
                context.push_str(&format!("\n  文本{}：{}", if truncated { "节选" } else { "内容" }, serde_json::Value::String(excerpt)));
            }
        }
    }
    context
}
