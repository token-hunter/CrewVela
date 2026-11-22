//! Session-scoped messaging over stdio, outside the model's shell sandbox.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{collections::VecDeque, io::{BufRead, Read, Write}, path::PathBuf, process::Command};

const FRAME_LIMIT: u64 = 256 * 1024;
const SERVER: &str = "crewvela_sessions";

pub(crate) fn configure_codex(command: &mut Command, sender: &str) -> Result<()> {
    let executable = std::env::current_exe()?;
    for (key, value) in [
        ("command", json!(executable)),
        ("args", json!(["--session-message-mcp", sender])),
        ("enabled", json!(true)),
    ] {
        command.arg("-c").arg(format!("mcp_servers.{SERVER}.{key}={}", serde_json::to_string(&value)?));
    }
    Ok(())
}

pub fn run_stdio(sender: &str) -> Result<()> {
    uuid::Uuid::parse_str(sender).context("会话通信身份格式无效")?;
    let root = crate::providers::config_dir()?;
    let endpoint = Endpoint { sessions: root.join("sessions.json"), outbox: root.join("outbox"), sender: sender.into() };
    endpoint.check_sender(false)?;
    serve(std::io::stdin().lock(), std::io::stdout().lock(), &endpoint)
}

struct Endpoint {
    sessions: PathBuf,
    outbox: PathBuf,
    sender: String,
}

impl Endpoint {
    fn check_sender(&self, require_running: bool) -> Result<()> {
        let records = crate::sessions::load_records(&self.sessions)?;
        let sender = records.iter().find(|record| record.id == self.sender
            && !record.retired_agent_runtime && record.archived_at.is_none())
            .context("会话通信身份已失效；未更换发送方或新建会话")?;
        if require_running && (sender.pty_id.is_none() || sender.runtime_command_host) {
            bail!("发送方原交互会话已停止，未发送消息");
        }
        crate::runtime_preferences::ensure_session_available(sender)
    }

    fn call(&self, name: &str, arguments: &Value) -> Result<Value> {
        self.check_sender(true)?;
        let args = arguments.as_object().context("工具参数必须是对象")?;
        let data = match name {
            "sessions" => {
                if !args.is_empty() { bail!("sessions 不接受参数"); }
                json!({"sessions":crate::messaging::session_view_at(&self.sessions)?})
            }
            "send" => {
                if args.len() != 2 || !args.contains_key("to") || !args.contains_key("body") {
                    bail!("send 仅接受 to 和 body；发送方由原会话绑定");
                }
                let to = args["to"].as_str().context("to 必须是字符串")?;
                let body = args["body"].as_str().context("body 必须是字符串")?;
                let receipt = crate::messaging::send_at(&self.sessions, &self.outbox, to, body,
                    Some(&self.sender), crate::runtime_preferences::ensure_session_available)?;
                json!({"queued":true,"receipt":receipt})
            }
            _ => bail!("未知会话通信工具"),
        };
        Ok(json!({"content":[{"type":"text","text":serde_json::to_string(&data)?}],"structuredContent":data,"isError":false}))
    }
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"sessions","description":"列出 CrewVela 会话。给已打开的 Agent 发消息前先刷新列表，选择 preferredForRuntime=true 的原交互会话。",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
         "annotations":{"readOnlyHint":true,"openWorldHint":false}},
        {"name":"send","description":"以当前绑定会话身份给目标原会话发送消息。成功只表示已持久化排队，不表示已送达或收到回复；不得编造对话。无需 shell 或项目写权限。",
         "inputSchema":{"type":"object","properties":{"to":{"type":"string","minLength":1},"body":{"type":"string","minLength":1,"maxLength":32000}},"required":["to","body"],"additionalProperties":false},
         "annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}
    ]})
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn emit(writer: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn serve(reader: impl BufRead, writer: impl Write, endpoint: &Endpoint) -> Result<()> {
    serve_tools(reader, writer, "crewvela-sessions",
        "通过 sessions/send 给用户已打开的原会话通信，发送方身份固定；不要通过 shell 写队列，不要请求扩大项目沙箱，排队不等于对方回复。",
        tools(), |name, arguments| endpoint.call(name, arguments), "send")
}

pub(crate) fn serve_tools(
    mut reader: impl BufRead,
    mut writer: impl Write,
    server_name: &str,
    instructions: &str,
    tools: Value,
    call: impl Fn(&str, &Value) -> Result<Value>,
    write_tool: &str,
) -> Result<()> {
    let (mut initialized, mut ready) = (false, false);
    // A retried send request must never create a second message in this process.
    let mut sent: VecDeque<(Value, Value, Value)> = VecDeque::new();
    loop {
        let mut frame = Vec::new();
        if (&mut reader).take(FRAME_LIMIT + 1).read_until(b'\n', &mut frame)? == 0 { return Ok(()); }
        if frame.len() as u64 > FRAME_LIMIT {
            emit(&mut writer, &rpc_error(Value::Null, -32600, "会话通信请求超过大小限制"))?;
            bail!("本机会话工具请求超过大小限制");
        }
        if frame.iter().all(u8::is_ascii_whitespace) { continue; }
        let request: Value = match serde_json::from_slice(&frame) {
            Ok(value) => value,
            Err(_) => { emit(&mut writer, &rpc_error(Value::Null, -32700, "JSON 请求无效"))?; continue; }
        };
        let id = request.get("id").cloned();
        let method = request["method"].as_str().unwrap_or("");
        if request["jsonrpc"] != "2.0" || method.is_empty() || id.as_ref().is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64())) {
            emit(&mut writer, &rpc_error(Value::Null, -32600, "JSON-RPC 请求无效"))?;
            continue;
        }
        let Some(id) = id else {
            if method == "notifications/initialized" && initialized { ready = true; }
            continue;
        };
        let response = match method {
            "initialize" if !initialized => {
                initialized = true;
                let version = request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05");
                let version = if matches!(version, "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25") { version } else { "2024-11-05" };
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":server_name,"version":"1.0.0"},
                    "instructions":instructions}})
            }
            "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
            _ if !ready => rpc_error(id, -32002, "请先完成 initialize 握手"),
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":tools}),
            "tools/call" => {
                let params = &request["params"];
                if let Some((_, previous, response)) = sent.iter().find(|(previous, _, _)| previous == &id) {
                    let response = if previous == params { response.clone() } else { rpc_error(id, -32600, "不能复用发送请求 ID 修改消息") };
                    emit(&mut writer, &response)?;
                    continue;
                }
                let name = params["name"].as_str().unwrap_or("");
                let result = call(name, params.get("arguments").unwrap_or(&json!({})))
                    .unwrap_or_else(|error| json!({"content":[{"type":"text","text":format!("{error:#}")}],"isError":true}));
                let response = json!({"jsonrpc":"2.0","id":id,"result":result});
                if name == write_tool {
                    if sent.len() == 32 { sent.pop_front(); }
                    sent.push_back((id, params.clone(), response.clone()));
                }
                response
            }
            _ => rpc_error(id, -32601, "会话通信不支持该方法"),
        };
        emit(&mut writer, &response)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control::SessionActionQueue, sessions::{NewRuntimeSession, SessionManager}};

    fn fixture() -> (PathBuf, SessionManager, Endpoint, String) {
        let root = std::env::temp_dir().join(format!("crewvela-session-mcp-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let make = |runtime: &str| sessions.create_runtime(NewRuntimeSession {
            runtime_kind: runtime.into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let sender = make("codex");
        let target = make("claude_code");
        sessions.attach_runtime(&sender.id, 1).unwrap();
        sessions.attach_runtime(&target.id, 2).unwrap();
        let endpoint = Endpoint { sessions: root.join("sessions.json"), outbox: root.join("outbox"), sender: sender.id };
        (root, sessions, endpoint, target.id)
    }

    #[test]
    fn direct_communication_rejects_self_send_without_queuing() {
        let (root, sessions, endpoint, _) = fixture();
        let result = endpoint.call("send", &json!({"to":endpoint.sender,"body":"hello"}));
        assert!(result.is_err(), "self-message was accepted: {result:?}");
        assert!(result.unwrap_err().to_string().contains("自己"));
        assert!(!root.join("outbox").exists() || std::fs::read_dir(root.join("outbox")).unwrap().next().is_none());
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn session_tool_sends_as_bound_identity_through_the_existing_mailbox() {
        let (root, sessions, endpoint, target) = fixture();
        let listed = endpoint.call("sessions", &json!({})).unwrap();
        assert!(listed.to_string().contains(&target));
        let body = "MCP 原会话消息\n保留引号 ' 与 $变量";
        let result = endpoint.call("send", &json!({"to":target,"body":body})).unwrap();
        assert_eq!(result["isError"], false);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        let message = queue.peek_next(&target).unwrap();
        assert_eq!(message.body, body);
        assert_eq!(message.from.unwrap().session_id, endpoint.sender);
        drop(queue); drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn session_tool_rejects_sender_spoofing_unknown_tools_and_stale_identity() {
        let (root, sessions, endpoint, target) = fixture();
        for (name, args) in [
            ("send", json!({"to":target,"body":"hello","from":target})),
            ("send", json!({"to":target,"body":""})),
            ("sessions", json!({"path":"unrelated"})),
            ("shell", json!({"command":"must not execute"})),
        ] { assert!(endpoint.call(name, &args).is_err()); }
        assert!(!root.join("outbox").exists());
        sessions.mark_stopped(&endpoint.sender).unwrap();
        assert!(endpoint.call("send", &json!({"to":target,"body":"stopped"})).is_err());
        assert!(!root.join("outbox").exists());
        sessions.delete(&endpoint.sender).unwrap();
        assert!(endpoint.call("send", &json!({"to":target,"body":"stale"})).is_err());
        assert!(!root.join("outbox").exists());
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stdio_handshake_and_duplicate_request_deliver_only_once() {
        let (root, sessions, endpoint, target) = fixture();
        let send = json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"send","arguments":{"to":target,"body":"one message"}}});
        let requests = [
            json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            send.clone(), send,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"send","arguments":{"to":target,"body":"different"}}}),
        ];
        let input = requests.iter().map(|request| format!("{request}\n")).collect::<String>();
        let mut output = Vec::new();
        serve(std::io::Cursor::new(input), &mut output, &endpoint).unwrap();
        let replies = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>();
        assert_eq!(replies.len(), 6);
        assert_eq!(replies[0]["error"]["code"], -32002);
        assert_eq!(replies[1]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(replies[2]["result"]["tools"].as_array().unwrap().len(), 2);
        assert_eq!(replies[3], replies[4]);
        assert_eq!(replies[5]["error"]["code"], -32600);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        assert_eq!(queue.peek_next(&target).unwrap().body, "one message");
        drop(queue); drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_and_oversize_frames_cannot_send_messages() {
        let (root, sessions, endpoint, _) = fixture();
        let mut output = Vec::new();
        serve(std::io::Cursor::new(b"invalid json\n[]\n"), &mut output, &endpoint).unwrap();
        let replies = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>();
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[1]["error"]["code"], -32600);
        assert!(serve(std::io::Cursor::new(vec![b'x'; FRAME_LIMIT as usize + 1]), Vec::new(), &endpoint).is_err());
        assert!(!root.join("outbox").exists());
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_configuration_only_adds_the_bound_session_server() {
        let mut command = Command::new("codex");
        command.args(["--sandbox", "read-only", "-c", "mcp_servers.custom.enabled=true"]);
        let original = command.get_args().map(|arg| arg.to_owned()).collect::<Vec<_>>();
        configure_codex(&mut command, "fixture-original-id").unwrap();
        let args = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(&args[..original.len()], original.iter().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>());
        assert_eq!(args.len(), original.len() + 6);
        assert!(args.iter().any(|arg| arg.contains("--session-message-mcp") && arg.contains("fixture-original-id")));
        assert!(!args.iter().any(|arg| arg.contains("default_permissions") || arg.contains("--add-dir")));
    }
}
