use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::pty::PtyManager;
use crate::sessions::{SessionKind, SessionRecord};
use crate::usage::{transcript_path, SessionUsage, UsageTracker};

const TELEMETRY_TRANSCRIPT_TAIL_BYTES: u64 = 1024 * 1024;
const TERMINAL_CONTEXT_TAIL_BYTES: usize = 96 * 1024;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextTelemetry {
    pub used_tokens: Option<u64>,
    pub max_tokens: Option<u64>,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub resets_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTelemetrySnapshot {
    pub session_id: String,
    pub runtime: String,
    pub context: Option<ContextTelemetry>,
    pub last_turn: Option<TokenBreakdown>,
    pub cumulative: Option<TokenBreakdown>,
    pub cache_hit_percent: Option<f64>,
    pub quota_windows: Vec<QuotaWindow>,
    pub captured_at: u64,
    pub source: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredClaudeStatusline {
    context: ContextTelemetry,
    last_turn: Option<TokenBreakdown>,
    quota_windows: Vec<QuotaWindow>,
    captured_at: u64,
}

struct ParsedClaudeStatusline {
    context: ContextTelemetry,
    last_turn: Option<TokenBreakdown>,
    quota_windows: Vec<QuotaWindow>,
}

fn parse_claude_statusline_payload(payload: &str) -> Option<ParsedClaudeStatusline> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let window = value.get("context_window")?;
    let number = |name: &str| window.get(name).and_then(Value::as_u64);
    let percent = |name: &str| {
        window
            .get(name)
            .and_then(Value::as_f64)
            .map(clamp_percent)
            .map(round_one)
    };
    let current = window.get("current_usage").filter(|value| !value.is_null());
    let current_number = |name: &str| {
        current
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let last_turn = current.map(|_| TokenBreakdown {
        input_tokens: current_number("input_tokens"),
        output_tokens: current_number("output_tokens")
            .max(number("total_output_tokens").unwrap_or(0)),
        cache_read_tokens: current_number("cache_read_input_tokens"),
        cache_write_tokens: current_number("cache_creation_input_tokens"),
        reasoning_tokens: None,
    });
    let used_tokens =
        number("total_input_tokens").or_else(|| last_turn.as_ref().map(context_tokens));
    let max_tokens = number("context_window_size");
    let used_percent = percent("used_percentage").or_else(|| {
        Some(used_tokens? as f64 / max_tokens? as f64 * 100.0)
            .map(clamp_percent)
            .map(round_one)
    });
    let remaining_percent = percent("remaining_percentage")
        .or_else(|| used_percent.map(|used| round_one(clamp_percent(100.0 - used))));
    let quota_windows = [("five_hour", "5 小时"), ("seven_day", "每周")]
        .into_iter()
        .filter_map(|(key, label)| {
            let quota = value.pointer(&format!("/rate_limits/{key}"))?;
            let used = quota
                .get("used_percentage")
                .and_then(Value::as_f64)
                .map(clamp_percent)
                .map(round_one);
            Some(QuotaWindow {
                label: label.to_string(),
                used_percent: used,
                remaining_percent: used.map(|value| round_one(clamp_percent(100.0 - value))),
                resets_at: quota.get("resets_at").and_then(Value::as_u64),
            })
        })
        .collect();
    Some(ParsedClaudeStatusline {
        context: ContextTelemetry {
            used_tokens,
            max_tokens,
            used_percent,
            remaining_percent,
        },
        last_turn,
        quota_windows,
    })
}

fn telemetry_source(statusline: bool, terminal: bool, transcript: bool) -> &'static str {
    match (statusline, terminal, transcript) {
        (true, _, true) => "statusline+transcript",
        (true, _, false) => "statusline",
        (false, true, true) => "terminal+transcript",
        (false, true, false) => "terminal_status",
        (false, false, true) => "transcript",
        (false, false, false) => "unavailable",
    }
}

fn safe_session_telemetry_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn telemetry_dir_from_events(events_dir: &Path) -> PathBuf {
    events_dir
        .parent()
        .unwrap_or(events_dir)
        .join("session-telemetry")
}

fn telemetry_file(dir: &Path, app_session_id: &str) -> Option<PathBuf> {
    safe_session_telemetry_id(app_session_id).then(|| dir.join(format!("{app_session_id}.json")))
}

pub fn store_claude_statusline(
    events_dir: &Path,
    app_session_id: &str,
    payload: &str,
) -> anyhow::Result<String> {
    let parsed = parse_claude_statusline_payload(payload)
        .ok_or_else(|| anyhow::anyhow!("Claude statusLine payload 缺少有效 context_window"))?;
    let dir = telemetry_dir_from_events(events_dir);
    let path = telemetry_file(&dir, app_session_id)
        .ok_or_else(|| anyhow::anyhow!("无效的 CrewVela 会话 ID"))?;
    fs::create_dir_all(&dir)?;
    let stored = StoredClaudeStatusline {
        context: parsed.context.clone(),
        last_turn: parsed.last_turn,
        quota_windows: parsed.quota_windows,
        captured_at: now_millis(),
    };
    fs::write(path, serde_json::to_vec(&stored)?)?;
    let used = stored
        .context
        .used_percent
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| "--".to_string());
    let tokens = match (stored.context.used_tokens, stored.context.max_tokens) {
        (Some(current), Some(max)) => format!(" ({}/{})", human_tokens(current), human_tokens(max)),
        _ => String::new(),
    };
    Ok(format!("Context {used}{tokens}"))
}

fn read_claude_statusline(app_session_id: &str) -> Option<StoredClaudeStatusline> {
    let dir = crate::providers::config_dir()
        .ok()?
        .join("session-telemetry");
    let path = telemetry_file(&dir, app_session_id)?;
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub fn clear_claude_statusline(app_session_id: &str) -> anyhow::Result<()> {
    let dir = crate::providers::config_dir()?.join("session-telemetry");
    let Some(path) = telemetry_file(&dir, app_session_id) else {
        anyhow::bail!("无效的 CrewVela 会话 ID");
    };
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn human_tokens(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub fn snapshot_batch(
    sessions: &[SessionRecord],
    pty: &PtyManager,
    usage: &UsageTracker,
) -> Vec<RuntimeTelemetrySnapshot> {
    sessions
        .iter()
        .filter_map(|session| snapshot_for(session, pty, usage))
        .collect()
}

fn snapshot_for(
    session: &SessionRecord,
    pty: &PtyManager,
    usage_tracker: &UsageTracker,
) -> Option<RuntimeTelemetrySnapshot> {
    let runtime = runtime_kind(session)?;
    let terminal_context = session
        .pty_id
        .and_then(|id| pty.transcript_tail(id, TERMINAL_CONTEXT_TAIL_BYTES).ok())
        .and_then(|tail| parse_terminal_context(&tail));
    let terminal_reported = terminal_context.is_some();

    let (context, last_turn, cumulative, cache_hit, quota_windows, source, confidence) = if runtime
        == "claude_code"
    {
        let statusline = read_claude_statusline(&session.id);
        let statusline_reported = statusline.is_some();
        let transcript_turn = latest_claude_usage(session);
        let transcript_reported = transcript_turn.is_some();
        let last_turn = statusline
            .as_ref()
            .and_then(|snapshot| snapshot.last_turn.clone())
            .or(transcript_turn);
        let cumulative = non_empty_usage(usage_tracker.usage_for(session));
        let context = statusline
            .as_ref()
            .map(|snapshot| snapshot.context.clone())
            .or_else(|| merge_context(terminal_context, last_turn.as_ref()));
        let cache_hit = last_turn.as_ref().and_then(cache_hit_percent);
        let quota_windows = statusline
            .as_ref()
            .map(|snapshot| snapshot.quota_windows.clone())
            .unwrap_or_default();
        let source = telemetry_source(statusline_reported, terminal_reported, transcript_reported);
        let confidence = if source == "unavailable" {
            "unavailable"
        } else {
            "provider_reported"
        };
        (
            context,
            last_turn,
            cumulative,
            cache_hit,
            quota_windows,
            source,
            confidence,
        )
    } else {
        let source = if terminal_context.is_some() {
            "terminal_status"
        } else {
            "unavailable"
        };
        let confidence = if terminal_context.is_some() {
            "provider_reported"
        } else {
            "unavailable"
        };
        (
            terminal_context,
            None,
            None,
            None,
            Vec::new(),
            source,
            confidence,
        )
    };

    Some(RuntimeTelemetrySnapshot {
        session_id: session.id.clone(),
        runtime: runtime.to_string(),
        context,
        last_turn,
        cumulative,
        cache_hit_percent: cache_hit,
        quota_windows,
        captured_at: now_millis(),
        source: source.to_string(),
        confidence: confidence.to_string(),
    })
}

fn runtime_kind(session: &SessionRecord) -> Option<&'static str> {
    match session.agent_runtime_kind.as_deref() {
        Some("codex") => Some("codex"),
        Some("kimi_code") => Some("kimi_code"),
        Some("claude_code") => Some("claude_code"),
        _ if session.kind == SessionKind::Claude => Some("claude_code"),
        _ => None,
    }
}

fn latest_claude_usage(session: &SessionRecord) -> Option<TokenBreakdown> {
    latest_claude_usage_at(&transcript_path(session)?)
}

fn latest_claude_usage_at(path: &Path) -> Option<TokenBreakdown> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TELEMETRY_TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut reader = BufReader::new(file);
    if start > 0 {
        // The byte tail can start inside a UTF-8 character; discard the partial line as bytes.
        let mut partial = Vec::new();
        reader.read_until(b'\n', &mut partial).ok()?;
    }
    let mut latest = None;
    for line in reader.lines().map_while(Result::ok) {
        if let Some(usage) = parse_claude_usage_line(&line) {
            latest = Some(usage);
        }
    }
    latest
}

fn parse_claude_usage_line(line: &str) -> Option<TokenBreakdown> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let usage = value.pointer("/message/usage")?;
    let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    Some(TokenBreakdown {
        input_tokens: number("input_tokens"),
        output_tokens: number("output_tokens"),
        cache_read_tokens: number("cache_read_input_tokens"),
        cache_write_tokens: number("cache_creation_input_tokens"),
        reasoning_tokens: None,
    })
}

fn non_empty_usage(usage: SessionUsage) -> Option<TokenBreakdown> {
    (usage.total() > 0).then_some(TokenBreakdown {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_creation_tokens,
        reasoning_tokens: None,
    })
}

fn merge_context(
    terminal: Option<ContextTelemetry>,
    last_turn: Option<&TokenBreakdown>,
) -> Option<ContextTelemetry> {
    let last_turn_tokens = last_turn.map(context_tokens);
    match (terminal, last_turn_tokens) {
        (Some(mut context), Some(tokens)) => {
            context.used_tokens = Some(tokens);
            Some(context)
        }
        (None, Some(tokens)) => Some(ContextTelemetry {
            used_tokens: Some(tokens),
            max_tokens: None,
            used_percent: None,
            remaining_percent: None,
        }),
        (Some(context), None) => Some(context),
        (None, None) => None,
    }
}

fn context_tokens(usage: &TokenBreakdown) -> u64 {
    usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens
}

fn parse_terminal_context(raw: &[u8]) -> Option<ContextTelemetry> {
    let visible = strip_terminal_sequences(raw);
    let lower = visible.to_ascii_lowercase();
    let context_index = lower.rfind("context")?;
    let mut end = (context_index + 180).min(lower.len());
    while !lower.is_char_boundary(end) { end -= 1; }
    let segment = &lower[context_index..end];
    let percent_index = segment.find('%')?;
    let percent = parse_number_before(&segment[..percent_index])?;
    let percent_tail = &segment[percent_index + 1..];
    let reports_remaining = percent_tail
        .chars()
        .take(24)
        .collect::<String>()
        .contains("left")
        || segment[..percent_index].contains("remaining");
    let used_percent = clamp_percent(if reports_remaining {
        100.0 - percent
    } else {
        percent
    });
    let remaining_percent = clamp_percent(100.0 - used_percent);
    let (used_tokens, max_tokens) = parse_token_ratio(segment).unwrap_or((None, None));
    Some(ContextTelemetry {
        used_tokens,
        max_tokens,
        used_percent: Some(round_one(used_percent)),
        remaining_percent: Some(round_one(remaining_percent)),
    })
}

fn strip_terminal_sequences(raw: &[u8]) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }
    let mut state = State::Text;
    let mut output = Vec::with_capacity(raw.len());
    for byte in raw.iter().copied() {
        match state {
            State::Text if byte == 0x1b => state = State::Escape,
            State::Text if byte == b'\r' || byte == b'\n' => output.push(b'\n'),
            State::Text if byte == 0x08 => {
                output.pop();
            }
            State::Text if byte >= 0x20 => output.push(byte),
            State::Text => {}
            State::Escape if byte == b'[' => state = State::Csi,
            State::Escape if byte == b']' => state = State::Osc,
            State::Escape => state = State::Text,
            State::Csi if (0x40..=0x7e).contains(&byte) => state = State::Text,
            State::Csi => {}
            State::Osc if byte == 0x07 => state = State::Text,
            State::Osc if byte == 0x1b => state = State::OscEscape,
            State::Osc => {}
            State::OscEscape if byte == b'\\' => state = State::Text,
            State::OscEscape => state = State::Osc,
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn parse_number_before(value: &str) -> Option<f64> {
    let number = value
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    number.parse().ok()
}

fn parse_token_ratio(segment: &str) -> Option<(Option<u64>, Option<u64>)> {
    let slash = segment.find('/')?;
    let left = segment[..slash]
        .rsplit(|ch: char| ch == '(' || ch.is_whitespace())
        .find(|value| !value.is_empty())?;
    let right = segment[slash + 1..]
        .split(|ch: char| ch == ')' || ch.is_whitespace())
        .find(|value| !value.is_empty())?;
    Some((parse_human_tokens(left), parse_human_tokens(right)))
}

fn parse_human_tokens(value: &str) -> Option<u64> {
    let value = value.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.');
    let (number, multiplier) = match value.chars().last()?.to_ascii_lowercase() {
        'k' => (&value[..value.len() - 1], 1_000.0),
        'm' => (&value[..value.len() - 1], 1_000_000.0),
        'b' => (&value[..value.len() - 1], 1_000_000_000.0),
        _ => (value, 1.0),
    };
    number
        .parse::<f64>()
        .ok()
        .map(|tokens| (tokens * multiplier).round() as u64)
}

fn cache_hit_percent(usage: &TokenBreakdown) -> Option<f64> {
    let denominator = usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens;
    (denominator > 0).then_some(round_one(
        usage.cache_read_tokens as f64 / denominator as f64 * 100.0,
    ))
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn round_one(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kimi_used_and_max_context() {
        let context = parse_terminal_context(b"K3 thinking: high  context: 42.0% (420k/1M)")
            .expect("Kimi context should parse");
        assert_eq!(context.used_tokens, Some(420_000));
        assert_eq!(context.max_tokens, Some(1_000_000));
        assert_eq!(context.used_percent, Some(42.0));
        assert_eq!(context.remaining_percent, Some(58.0));
    }

    #[test]
    fn parses_codex_remaining_context_without_inverting_it() {
        let context = parse_terminal_context(b"gpt-5.6-sol medium  Context 83% left")
            .expect("Codex context should parse");
        assert_eq!(context.used_percent, Some(17.0));
        assert_eq!(context.remaining_percent, Some(83.0));
    }

    #[test]
    fn cache_hit_uses_mutually_exclusive_input_categories() {
        let usage = TokenBreakdown {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 800,
            cache_write_tokens: 100,
            reasoning_tokens: None,
        };
        assert_eq!(cache_hit_percent(&usage), Some(80.0));
    }

    #[test]
    fn parses_claude_current_usage_without_counting_output_as_context() {
        let usage = parse_claude_usage_line(
            r#"{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":800,"cache_creation_input_tokens":100}}}"#,
        )
        .unwrap();
        assert_eq!(context_tokens(&usage), 1_000);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn parses_official_claude_statusline_context_payload() {
        let payload = r#"{
            "context_window": {
                "total_input_tokens": 22528,
                "total_output_tokens": 87,
                "context_window_size": 262144,
                "used_percentage": 8.6,
                "remaining_percentage": 91.4,
                "current_usage": {
                    "input_tokens": 22400,
                    "output_tokens": 87,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 128
                }
            }
        }"#;
        let parsed = parse_claude_statusline_payload(payload)
            .expect("official statusLine JSON should parse");
        assert_eq!(parsed.context.used_tokens, Some(22_528));
        assert_eq!(parsed.context.max_tokens, Some(262_144));
        assert_eq!(parsed.context.used_percent, Some(8.6));
        assert_eq!(parsed.context.remaining_percent, Some(91.4));
        let last_turn = parsed.last_turn.expect("current_usage should be preserved");
        assert_eq!(last_turn.input_tokens, 22_400);
        assert_eq!(last_turn.output_tokens, 87);
        assert_eq!(last_turn.cache_read_tokens, 128);
    }

    #[test]
    fn telemetry_source_only_claims_sources_that_really_reported() {
        assert_eq!(telemetry_source(true, false, true), "statusline+transcript");
        assert_eq!(telemetry_source(false, false, true), "transcript");
        assert_eq!(telemetry_source(false, true, true), "terminal+transcript");
        assert_eq!(telemetry_source(false, false, false), "unavailable");
    }

    #[test]
    fn statusline_capture_persists_only_structured_telemetry() {
        let root =
            std::env::temp_dir().join(format!("crewvela-statusline-test-{}", uuid::Uuid::new_v4()));
        let events = root.join("session-events");
        fs::create_dir_all(&events).unwrap();
        let payload = r#"{
            "cwd":"C:/secret/project",
            "context_window":{
                "total_input_tokens":1000,
                "total_output_tokens":20,
                "context_window_size":200000,
                "used_percentage":0.5,
                "remaining_percentage":99.5,
                "current_usage":{
                    "input_tokens":100,
                    "output_tokens":20,
                    "cache_creation_input_tokens":100,
                    "cache_read_input_tokens":800
                }
            }
        }"#;
        let display = store_claude_statusline(&events, "app-session-123", payload).unwrap();
        assert_eq!(display, "Context 0.5% (1.0k/200.0k)");
        let path = root.join("session-telemetry/app-session-123.json");
        let stored = fs::read_to_string(path).unwrap();
        assert!(!stored.contains("secret/project"));
        let parsed: StoredClaudeStatusline = serde_json::from_str(&stored).unwrap();
        assert_eq!(parsed.context.max_tokens, Some(200_000));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn context_window_truncation_preserves_utf8_boundaries() {
        let output = format!("context 25% x{}", "中".repeat(100));
        assert!(!output.is_char_boundary(180));
        assert_eq!(parse_terminal_context(output.as_bytes()).unwrap().used_percent, Some(25.0));
    }

    #[test]
    fn transcript_tail_starting_inside_utf8_still_reads_latest_usage() {
        let path = std::env::temp_dir().join(format!("cc-telemetry-tail-{}.jsonl", uuid::Uuid::new_v4()));
        let line = r#"{"type":"assistant","message":{"usage":{"input_tokens":42,"output_tokens":7}}}"#;
        let mut transcript = format!("{}\n{line}\n", "中".repeat(400_000));
        while transcript.is_char_boundary(transcript.len() - TELEMETRY_TRANSCRIPT_TAIL_BYTES as usize) {
            transcript.push('\n');
        }
        fs::write(&path, transcript).unwrap();
        let usage = latest_claude_usage_at(&path).unwrap();
        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 7);
        fs::remove_file(path).unwrap();
    }

}
