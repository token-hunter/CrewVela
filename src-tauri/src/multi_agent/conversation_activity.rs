//! Public activity metadata, never reasoning text or arbitrary command/tool output.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use super::RuntimeEventRecord;
use std::io::{Read, Seek, SeekFrom};

pub(super) fn signal(event_type: &str, payload: &Value) -> Option<Value> {
    match event_type {
        "analysis_activity" => Some(json!({"kind":"analysis"})),
        "assistant_delta" if payload["text"].as_str().is_some_and(|text| !text.is_empty()) => Some(json!({"kind":"reply"})),
        "tool_protocol" => {
            let id = payload["toolCallId"].as_str()?;
            if id.is_empty() || id.len() > 512 { return None; }
            let mut activity = json!({"kind":"tool","toolId":id});
            if let Some(status) = match payload["status"].as_str() { Some("completed")=>Some("completed"),Some("failed"|"error")=>Some("failed"),Some("pending")=>Some("pending"),Some("in_progress"|"running")=>Some("running"),_=>None } {activity["status"]=json!(status);}
            let kind = payload["kind"].as_str().unwrap_or("").to_lowercase();
            let title = payload["title"].as_str().unwrap_or("").to_lowercase();
            let name = if kind.is_empty() || kind == "other" {title.as_str()} else {kind.as_str()};
            // ACP search also covers local grep/glob. Only a fetch/web signal
            // or a recognized web tool name justifies claiming web access.
            let operation = if name == "read" || name.starts_with("read file") {Some("read")} else if kind == "fetch" || ["web_search","web search","websearch","fetch_url"].iter().any(|part| title.contains(part)) {Some("web")} else if name.contains("search") {Some("search")} else if name == "execute" || name.contains("command") || name == "bash" {Some("command")} else if name == "edit" {Some("edit")} else {None};
            if let Some(operation) = operation { activity["operation"] = json!(operation); }
            let path = payload.pointer("/locations/0/path").or_else(|| payload.pointer("/rawInput/file_path")).or_else(|| payload.pointer("/rawInput/path")).and_then(Value::as_str);
            if let Some(path) = path.filter(|path| path.len() <= 512 && !path.contains(['\n','\r','\0']) && !path.contains("://")) { activity["path"] = json!(path); }
            Some(activity)
        }
        _ => None,
    }
}

/// Only recognized JSON stdout protocol signals count as Agent activity. Startup
/// stderr, transport heartbeats and arbitrary prose do not imply model progress.
pub(super) fn stream_signal(value: &Value) -> Option<Value> {
    match value["type"].as_str()? {
        "message_update" => match value.pointer("/assistantMessageEvent/type").and_then(Value::as_str) {
            Some("thinking_start"|"thinking_delta") => Some(json!({"kind":"analysis"})),
            Some("text_start"|"text_delta") => Some(json!({"kind":"reply"})),
            _ => None,
        },
        "tool_execution_start"|"tool_execution_end" => {
            let name = value["toolName"].as_str().unwrap_or("");
            signal("tool_protocol", &json!({"toolCallId":value["toolCallId"],"title":name,"kind":if name=="read" {"read"} else {name},"status":if value["type"]=="tool_execution_end" {if value["isError"]==true {"failed"} else {"completed"}} else {"in_progress"},"rawInput":value["args"]}))
        }
        _ => None,
    }
}

/// Follow the existing UTF-8 JSONL capture, not terminal escape sequences or
/// visually wrapped PTY text. A bounded partial line survives split writes.
#[derive(Default)]
pub(super) struct CapturedActivity {
    offset: u64,
    partial: Vec<u8>,
    dropping_line: bool,
    disabled: bool,
}
impl CapturedActivity {
    pub(super) fn poll(&mut self, path: &std::path::Path, store: &super::MultiAgentStore, run: &str) -> Result<()> {
        if self.disabled { return Ok(()); }
        let mut file = match std::fs::File::open(path) {Ok(file)=>file,Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(()),Err(error)=>{self.disabled=true;return Err(error.into());}};
        let length = file.metadata()?.len();
        if length < self.offset {
            // The existing capture rotates after 5 MiB. Replaying its retained
            // prefix would falsely reset last-activity time, so stop observing.
            self.disabled = true; self.partial.clear();
            return Err(anyhow::anyhow!("活动捕获文件已轮转，后续状态不再推断；原执行不受影响"));
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut chunk = [0_u8;65536];
        let mut available = file.take(256 * 1024);
        loop {
            let count = available.read(&mut chunk)?;
            if count == 0 { break; }
            self.offset += count as u64;
            for payload in self.feed(&chunk[..count]) {
                store.append_runtime_event(super::AppendRuntimeEventInput {run_id:run.into(),event_type:"agent_activity".into(),payload})?;
            }
        }
        Ok(())
    }
    fn feed(&mut self, bytes: &[u8]) -> Vec<Value> {
        let mut signals = Vec::new();
        for byte in bytes {
            if *byte == b'\n' {
                if !self.dropping_line {
                    if let Ok(value) = serde_json::from_slice::<Value>(&self.partial) {
                        if let Some(signal) = stream_signal(&value) { signals.push(signal); }
                    }
                }
                self.partial.clear(); self.dropping_line=false;
            } else if !self.dropping_line {
                if self.partial.len() >= 1024*1024 { self.partial.clear(); self.dropping_line=true; }
                else { self.partial.push(*byte); }
            }
        }
        signals
    }
}

pub(super) fn persist(connection: &Connection, event: &RuntimeEventRecord, activity: &Value) -> Result<()> {
    // Fast chunks update genuine activity at most once per second. Tool state
    // transitions remain lossless; no heartbeat is ever recorded as activity.
    let previous: Option<(String,i64)> = connection.query_row("SELECT payload_json,created_at FROM ma_runtime_events WHERE run_id=?1 AND event_type='agent_activity' ORDER BY seq DESC LIMIT 1", [&event.run_id], |row| Ok((row.get(0)?,row.get(1)?))).optional()?;
    let encoded = activity.to_string();
    if previous.is_some_and(|(raw,at)| raw == encoded && event.created_at.saturating_sub(at) < 1000) { return Ok(()); }
    connection.execute("INSERT INTO ma_runtime_events(project_id,run_id,task_id,agent_id,event_type,payload_json,created_at) VALUES(?1,?2,?3,?4,'agent_activity',?5,?6)", params![event.project_id,event.run_id,event.task_id,event.agent_id,encoded,event.created_at])?;
    Ok(())
}

/// Dedicated per-run projection survives unrelated project chatter and output
/// floods. Keep latest tool transitions independently from reply chunk traffic.
pub(super) fn events(connection: &Connection, project: &str, run_ids: &[String]) -> Result<Vec<RuntimeEventRecord>> {
    let mut result = Vec::new();
    let mut statement = connection.prepare("SELECT seq,project_id,run_id,task_id,agent_id,event_type,payload_json,created_at FROM ma_runtime_events WHERE project_id=?1 AND run_id=?2 AND event_type='agent_activity' AND (seq IN (SELECT seq FROM ma_runtime_events WHERE run_id=?2 AND event_type='agent_activity' AND json_extract(payload_json,'$.kind')='tool' ORDER BY seq DESC LIMIT 40) OR seq IN (SELECT seq FROM ma_runtime_events WHERE run_id=?2 AND event_type='agent_activity' ORDER BY seq DESC LIMIT 12)) ORDER BY seq")?;
    for run in run_ids.iter().rev().take(100) {
        result.extend(statement.query_map(params![project,run], super::store::runtime_event_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_excludes_thoughts_commands_and_tool_results() {
        let thought = json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"private reasoning"}});
        assert_eq!(stream_signal(&thought),Some(json!({"kind":"analysis"})));
        assert_eq!(signal("output", &json!({"content":"thinking…"})),None);
        let tool = signal("tool_protocol", &json!({"toolCallId":"t1","kind":"execute","status":"in_progress","rawInput":{"command":"curl -H secret"},"content":["private result"]})).unwrap();
        assert_eq!(tool,json!({"kind":"tool","toolId":"t1","operation":"command","status":"running"}));
        assert!(!tool.to_string().contains("secret"));
        assert_eq!(stream_signal(&json!({"type":"heartbeat"})),None);
    }
    #[test]
    fn native_pi_tool_lifecycle_is_recognized() {
        let start = stream_signal(&json!({"type":"tool_execution_start","toolCallId":"t","toolName":"read","args":{"path":"src/api/auth.ts"}})).unwrap();
        assert_eq!(start["path"],"src/api/auth.ts"); assert_eq!(start["operation"],"read");
        let end = stream_signal(&json!({"type":"tool_execution_end","toolCallId":"t","toolName":"read","isError":true,"result":{"content":"secret"}})).unwrap();
        assert_eq!(end["status"],"failed"); assert!(!end.to_string().contains("secret"));
    }
    #[test]
    fn local_search_is_not_reported_as_web_access() {
        let local=signal("tool_protocol", &json!({"toolCallId":"s","kind":"search","title":"Search files","status":"in_progress"})).unwrap();
        assert_eq!(local["operation"],"search");
        let web=signal("tool_protocol", &json!({"toolCallId":"w","kind":"other","title":"mcp__web_search","status":"in_progress"})).unwrap();
        assert_eq!(web["operation"],"web");
    }
    #[test]
    fn captured_jsonl_handles_split_utf8_and_oversized_lines_without_replay() {
        let mut capture=CapturedActivity::default();
        let line="{\"type\":\"tool_execution_start\",\"toolCallId\":\"t\",\"toolName\":\"read\",\"args\":{\"path\":\"目录/auth.ts\"}}\n".as_bytes();
        let split=line.iter().position(|byte| !byte.is_ascii()).unwrap()+1;
        assert!(capture.feed(&line[..split]).is_empty());
        let signals=capture.feed(&line[split..]);assert_eq!(signals.len(),1);assert_eq!(signals[0]["path"],"目录/auth.ts");
        assert!(capture.feed(&vec![b'x';1024*1024+1]).is_empty());assert!(capture.partial.len()<1024);
        assert!(capture.feed(b"\n").is_empty());
        assert_eq!(capture.feed(line).len(),1);
    }
}
