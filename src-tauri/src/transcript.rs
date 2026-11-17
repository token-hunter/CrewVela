//! 历史会话内容回看：把 claude transcript（jsonl）解析成前端可渲染的消息流。
//! user/assistant 提取文本块；tool_use/tool_result 折叠成一行摘要（工具输出可能
//! 巨大，只留头部）；thinking/system/progress 等非对话内容跳过。

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};

use anyhow::{anyhow, Context, Result};
use serde::{Serialize, Deserialize};

use crate::sessions::SessionRecord;
use crate::usage::transcript_path;

/// 单条消息文本上限：超过截断，避免一条巨型输出撑爆对话框。
const MAX_MESSAGE_CHARS: usize = 4_000;
/// 工具调用/结果摘要行上限。
const MAX_TOOL_SUMMARY_CHARS: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    /// user / assistant / tool
    pub role: String,
    pub text: String,
    /// transcript 行的 ISO 时间戳（没有则 None）。
    pub at: Option<String>,
}

fn cap(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let truncated: String = text.chars().take(max).collect();
    format!("{truncated}…")
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    cap(line.to_string(), max)
}

/// content 可能是纯字符串，或块数组（text / tool_use / tool_result / thinking）。
/// 返回若干条消息（一个块一条），无有效内容返回空。
fn parse_content(content: &serde_json::Value, is_user: bool) -> Vec<(String, String)> {
    if let Some(text) = content.as_str() {
        let trimmed = text.trim();
        return if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![(
                (if is_user { "user" } else { "assistant" }).to_string(),
                if is_user { trimmed.to_string() } else { cap(trimmed.to_string(), MAX_MESSAGE_CHARS) },
            )]
        };
    }
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    let mut text_buffer = String::new();
    let flush_text = |buffer: &mut String, messages: &mut Vec<(String, String)>| {
        let text = buffer.trim();
        if !text.is_empty() {
            messages.push((
                (if is_user { "user" } else { "assistant" }).to_string(),
                if is_user { text.to_string() } else { cap(text.to_string(), MAX_MESSAGE_CHARS) },
            ));
        }
        buffer.clear();
    };
    for block in blocks {
        match block.get("type").and_then(|value| value.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|value| value.as_str()) {
                    if !text_buffer.is_empty() {
                        text_buffer.push_str("\n");
                    }
                    text_buffer.push_str(text);
                }
            }
            Some("tool_use") => {
                flush_text(&mut text_buffer, &mut messages);
                let name = block
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("tool");
                let input = block
                    .get("input")
                    .map(|input| {
                        let raw = serde_json::to_string(input).unwrap_or_default();
                        first_line(&raw, MAX_TOOL_SUMMARY_CHARS)
                    })
                    .unwrap_or_default();
                messages.push(("tool".to_string(), format!("⚙ {name}  {input}")));
            }
            Some("tool_result") => {
                flush_text(&mut text_buffer, &mut messages);
                let summary = match block.get("content") {
                    Some(serde_json::Value::String(text)) => {
                        first_line(text, MAX_TOOL_SUMMARY_CHARS)
                    }
                    Some(serde_json::Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                        .next()
                        .map(|text| first_line(text, MAX_TOOL_SUMMARY_CHARS))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if !summary.is_empty() {
                    messages.push(("tool".to_string(), format!("⤷ {summary}")));
                }
            }
            _ => {}
        }
    }
    flush_text(&mut text_buffer, &mut messages);
    messages
}

/// 读取会话 transcript 的尾部消息（最多 max_messages 条）。
pub fn read_transcript(
    session: &SessionRecord,
    max_messages: usize,
) -> Result<Vec<TranscriptMessage>> {
    let path = transcript_path(session)
        .ok_or_else(|| anyhow!("该会话没有 claude transcript（终端会话不支持回看）"))?;
    read_transcript_file(&path, max_messages)
}

pub(crate) fn read_transcript_file(path: &std::path::Path, max_messages: usize) -> Result<Vec<TranscriptMessage>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("找不到会话记录文件: {}", path.display()))?;
    read_tail(file, max_messages)
}

fn read_tail(mut reader: impl Read + Seek, max_messages: usize) -> Result<Vec<TranscriptMessage>> {
    let mut messages: VecDeque<TranscriptMessage> = VecDeque::new();
    if max_messages == 0 { return Ok(Vec::new()); }
    // Read backwards in fixed blocks until enough *rendered messages* are found.
    // Bytes are assembled before JSON decoding, preserving multi-byte UTF-8 and
    // arbitrarily long user prompts across block boundaries.
    let mut position = reader.seek(SeekFrom::End(0))?;
    let mut block = [0u8; 64 * 1024];
    let mut reversed_line = Vec::new();
    while position > 0 {
        let length = position.min(block.len() as u64) as usize;
        position -= length as u64;
        reader.seek(SeekFrom::Start(position))?;
        reader.read_exact(&mut block[..length])?;
        for byte in block[..length].iter().rev() {
            if *byte == b'\n' {
                prepend_line(&mut reversed_line, &mut messages, max_messages);
                if messages.len() == max_messages {
                    return Ok(messages.into_iter().collect());
                }
            } else {
                reversed_line.push(*byte);
            }
        }
    }
    prepend_line(&mut reversed_line, &mut messages, max_messages);
    Ok(messages.into_iter().collect())
}

fn prepend_line(reversed: &mut Vec<u8>, messages: &mut VecDeque<TranscriptMessage>, limit: usize) {
    reversed.reverse();
    let parsed = serde_json::from_slice::<serde_json::Value>(reversed);
    reversed.clear();
    let Ok(value) = parsed else { return };
    let is_user = match value.get("type").and_then(|kind| kind.as_str()) {
        Some("user") => true,
        Some("assistant") => false,
        _ => return,
    };
    let at = value.get("timestamp").and_then(|stamp| stamp.as_str()).map(str::to_string);
    let Some(content) = value.pointer("/message/content") else { return };
    for (role, text) in parse_content(content, is_user).into_iter().rev() {
        if messages.len() == limit { break; }
        messages.push_front(TranscriptMessage { role, text, at: at.clone() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_tail_preserves_unicode_multiblock_prompts_and_block_order() {
        let long = "跨块🙂原文".repeat(20_000);
        let rows = [
            serde_json::json!({"type":"user", "timestamp":"before", "message":{"content":long}}),
            serde_json::json!({"type":"assistant", "timestamp":"after", "message":{"content":[
                {"type":"text", "text":"第一段"},
                {"type":"tool_use", "name":"Read", "input":{"file_path":"a"}},
                {"type":"text", "text":"最后一段"}
            ]}}),
        ];
        let data = rows.iter().map(|row| row.to_string()).collect::<Vec<_>>().join("\r\n");
        for suffix in ["", "\r\n", "\nnot-json\n{\"unfinished\":"] {
            let bytes = format!("{data}{suffix}").into_bytes();
            let messages = read_tail(std::io::Cursor::new(&bytes), 10).unwrap();
            assert_eq!(messages.len(), 4);
            assert_eq!(messages[0].text, long);
            assert_eq!(messages[0].at.as_deref(), Some("before"));
            assert_eq!(messages[1].text, "第一段");
            assert_eq!(messages[2].role, "tool");
            assert_eq!(messages[3].text, "最后一段");
            let last = read_tail(std::io::Cursor::new(&bytes), 2).unwrap();
            assert_eq!(last[0].text, messages[2].text);
            assert_eq!(last[1].text, messages[3].text);
            assert!(read_tail(std::io::Cursor::new(&bytes), 0).unwrap().is_empty());
        }
    }

    #[test]
    fn audit_tail_skips_corrupt_lines_but_reports_io_errors() {
        let mut bytes = b"\xff\xfe\n".to_vec();
        bytes.extend_from_slice(b"{\"type\":\"user\",\"message\":{\"content\":\"valid\"}}\n");
        let messages = read_tail(std::io::Cursor::new(bytes), 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid");
        struct Broken;
        impl Read for Broken { fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> { Err(std::io::ErrorKind::PermissionDenied.into()) } }
        impl Seek for Broken { fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> { Ok(10) } }
        assert!(read_tail(Broken, 1).is_err());
    }

    #[test]
    fn audit_transcript_tail_does_not_read_old_history() {
        struct CountingReader { inner: std::io::Cursor<Vec<u8>>, bytes: std::rc::Rc<std::cell::Cell<usize>> }
        impl std::io::Read for CountingReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = std::io::Read::read(&mut self.inner, buf)?;
                self.bytes.set(self.bytes.get() + n);
                Ok(n)
            }
        }
        impl std::io::Seek for CountingReader {
            fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> { std::io::Seek::seek(&mut self.inner, pos) }
        }
        let line = "{\"type\":\"user\",\"message\":{\"content\":\"历史消息\"}}\n";
        let mut history = line.repeat(100_000);
        history.push_str("{\"type\":\"assistant\",\"message\":{\"content\":\"最后一条\"}}\n");
        let bytes = std::rc::Rc::new(std::cell::Cell::new(0));
        let started = std::time::Instant::now();
        let messages = read_tail(CountingReader { inner: std::io::Cursor::new(history.into_bytes()), bytes: bytes.clone() }, 2).unwrap();
        eprintln!("transcript tail: bytes={} elapsed={:?}", bytes.get(), started.elapsed());
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "历史消息");
        assert_eq!(messages[1].text, "最后一条");
        assert!(bytes.get() <= 128 * 1024, "tail reads must not scale with the entire transcript: {} bytes", bytes.get());
    }

    #[test]
    fn parses_user_string_and_assistant_blocks() {
        let user = parse_content(&serde_json::json!(" 你好 "), true);
        assert_eq!(user, vec![("user".to_string(), "你好".to_string())]);

        let assistant = parse_content(
            &serde_json::json!([
                {"type": "text", "text": "先看一下代码"},
                {"type": "tool_use", "name": "Read", "input": {"file_path": "F:\\a.rs"}},
                {"type": "thinking", "thinking": "..."}
            ]),
            false,
        );
        assert_eq!(assistant.len(), 2);
        assert_eq!(assistant[0].0, "assistant");
        assert_eq!(assistant[0].1, "先看一下代码");
        assert_eq!(assistant[1].0, "tool");
        assert!(assistant[1].1.contains("Read"));
        assert!(assistant[1].1.contains(r#"F:\\a.rs"#));
    }

    #[test]
    fn tool_result_folds_into_summary_line() {
        let parsed = parse_content(
            &serde_json::json!([
                {"type": "tool_result", "content": [{"type": "text", "text": "第一行输出\n第二行输出"}]}
            ]),
            true,
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "tool");
        assert!(parsed[0].1.contains("第一行输出"));
        assert!(!parsed[0].1.contains("第二行输出"));
    }

    #[test]
    fn caps_oversized_text() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 10);
        // user 消息不截断（接力需要完整原文）；assistant 维持截断。
        let user = parse_content(&serde_json::json!(long), true);
        assert_eq!(user[0].1.chars().count(), MAX_MESSAGE_CHARS + 10);
        let assistant = parse_content(&serde_json::json!(long), false);
        assert!(assistant[0].1.chars().count() <= MAX_MESSAGE_CHARS + 1);
        assert!(assistant[0].1.ends_with('…'));
    }
}
