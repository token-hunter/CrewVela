use super::*;
use serde_json::Value;

/// npm's Windows cmd shim strips inner double quotes from table literals. One
/// TOML scalar per argument preserves the provider's actual routing fields.
fn config_scalar(command: &mut Command, key: &str, value: &Value) -> Result<()> {
    let literal = match value {
        Value::String(text)
            if !text.contains(['\'', '"', '\r', '\n', '\0', '%', '^', '&', '|', '<', '>']) =>
        {
            format!("'{text}'")
        }
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        _ => bail!("Codex 规划无法安全传递供应商配置项 {key}"),
    };
    command.arg("-c").arg(format!("{key}={literal}"));
    Ok(())
}

pub(super) fn codex_planner(
    request: &RuntimeRequest,
    cancelled: &AtomicBool,
) -> Result<RuntimeResult> {
    let mut command = runtime_command(AgentRuntimeKind::Codex)?;
    configure_codex_unattended_tools(&mut command);
    request.codex_storage.configure(&mut command, false)?;
    let mut probe = runtime_command(AgentRuntimeKind::Codex)?;
    request.codex_storage.configure(&mut probe, false)?;
    probe.arg("app-server");
    let effective = crate::codex_title_summary::read_effective_config(probe, &request.cwd)?;
    // Retain the actual provider and model without forwarding user MCPs, hooks,
    // credentials embedded in arguments, or executable provider customizations.
    if let Some(model) = effective["model"].as_str() {
        command.arg("--model").arg(model);
    }
    if let Some(provider) = effective["model_provider"]
        .as_str()
        .filter(|provider| *provider != "openai")
    {
        let route = effective["model_providers"][provider]
            .as_object()
            .context("无法核实 Codex 规划使用的供应商配置")?;
        anyhow::ensure!(
            provider
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                && route.iter().all(|(key, value)| value.is_null()
                    || matches!(
                        key.as_str(),
                        "name"
                            | "base_url"
                            | "env_key"
                            | "env_key_instructions"
                            | "env_http_headers"
                            | "wire_api"
                            | "requires_openai_auth"
                            | "supports_websockets"
                            | "supports_standalone_web_search"
                            | "request_max_retries"
                            | "stream_max_retries"
                            | "stream_idle_timeout_ms"
                            | "websocket_connect_timeout_ms"
                    )),
            "Codex 规划无法安全继承此自定义供应商的凭据或执行配置，未切换供应商"
        );
        command
            .arg("-c")
            .arg(format!("model_provider='{provider}'"));
        for (key, value) in route.iter().filter(|(_, value)| !value.is_null()) {
            if key == "env_key_instructions" {
                continue;
            }
            let field = format!("model_providers.{provider}.{key}");
            if key == "base_url" {
                let url = reqwest::Url::parse(value.as_str().context("Codex 供应商地址格式无效")?)?;
                anyhow::ensure!(
                    matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none(),
                    "Codex 规划不会在命令行传递包含凭据或查询参数的供应商地址"
                );
            }
            if key == "env_http_headers" {
                for (header, variable) in value
                    .as_object()
                    .context("Codex 供应商的环境请求头格式无效")?
                {
                    anyhow::ensure!(
                        header
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                        "Codex 供应商请求头名称无效"
                    );
                    config_scalar(
                        &mut command,
                        &format!("{field}.{header}"),
                        variable,
                    )?;
                }
            } else {
                config_scalar(&mut command, &field, value)?;
            }
        }
    }
    for (key, value) in crate::codex_title_summary::no_tools_config(&serde_json::json!({}))
        .as_object()
        .unwrap()
    {
        if key != "mcp_servers" {
            command.arg("-c").arg(format!("{key}={value}"));
        }
    }
    command.args([
        "--sandbox",
        "read-only",
        "--ask-for-approval",
        "never",
        "exec",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
        "--json",
    ]);
    append_codex_images(&mut command, request)?;
    command.arg("-");
    request.log_launch("无工具只读规划", "Codex exec")?;
    execute_command(
        AgentRuntimeKind::Codex,
        command,
        Some(render_task_prompt(request)),
        request,
        cancelled,
    )
}

/// For a one-shot CLI, the successful process exit is checked by the caller.
/// Only accept this invocation's native session and assistant terminal event.
pub(super) fn managed_command_reply(
    kind: AgentRuntimeKind,
    raw: &str,
) -> Result<(String, &'static str, String)> {
    let mut id = None;
    let mut text = None;
    let mut started = false;
    let mut completed = false;
    let key = if kind == AgentRuntimeKind::Codex {
        "threadId"
    } else {
        "sessionId"
    };
    if kind == AgentRuntimeKind::ClaudeCode {
        let mut result = raw.trim().trim_start_matches('\u{feff}');
        // The CLI writes this model-catalog diagnostic to stderr even for a
        // successful custom-provider run. The visible PTY captures both streams.
        while let Some(diagnostic) = result.strip_prefix("[claude-code:unrecognized_model] ") {
            let (line, rest) = diagnostic
                .split_once('\n')
                .context("Claude Code 模型提示后缺少原生结果")?;
            let notice: Value = serde_json::from_str(line.trim_end_matches('\r'))?;
            anyhow::ensure!(
                notice["model"].is_string() && notice["query_source"] == "sdk",
                "Claude Code 模型提示格式无效"
            );
            result = rest.trim_start();
        }
        let value: Value =
            serde_json::from_str(result).context("Claude Code 缺少已确认的审查结果")?;
        anyhow::ensure!(
            value["type"] == "result"
                && value["subtype"] == "success"
                && value["is_error"] == false
                && value
                    .get("stop_reason")
                    .is_none_or(|reason| reason == "end_turn"),
            "Claude Code 未报告原生成功终态"
        );
        id = value["session_id"].as_str().map(str::to_owned);
        text = value["result"].as_str().map(str::to_owned);
        completed = true;
    } else {
        for event in raw
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            match (kind, event["type"].as_str()) {
                (AgentRuntimeKind::Codex, Some("thread.started"))
                | (AgentRuntimeKind::Pi, Some("session")) => {
                    anyhow::ensure!(id.is_none() && !started, "原生输出包含多个会话");
                    id = event[if kind == AgentRuntimeKind::Codex {
                        "thread_id"
                    } else {
                        "id"
                    }]
                    .as_str()
                    .map(str::to_owned);
                }
                (AgentRuntimeKind::Codex, Some("turn.started"))
                | (AgentRuntimeKind::Pi, Some("agent_start")) => {
                    anyhow::ensure!(id.is_some() && !started, "原生轮次缺少唯一会话起点");
                    started = true;
                }
                (AgentRuntimeKind::Codex, Some("item.completed"))
                    if event["item"]["type"] == "agent_message" && started && !completed =>
                {
                    text = event["item"]["text"].as_str().map(str::to_owned);
                }
                (AgentRuntimeKind::Pi, Some("message_end"))
                    if event["message"]["role"] == "assistant" && started && !completed =>
                {
                    text = if event["message"]["stopReason"] == "stop" {
                        event["message"]["content"].as_array().map(|items| {
                            items
                                .iter()
                                .filter(|item| item["type"] == "text")
                                .filter_map(|item| item["text"].as_str())
                                .collect::<Vec<_>>()
                                .join("")
                        })
                    } else {
                        None
                    };
                }
                (AgentRuntimeKind::Codex, Some("turn.completed"))
                | (AgentRuntimeKind::Pi, Some("agent_end")) => {
                    anyhow::ensure!(started, "原生终态缺少受理起点");
                    completed = true;
                }
                (AgentRuntimeKind::Codex, Some("error" | "turn.failed")) => {
                    bail!("Codex 原生执行未成功")
                }
                _ => {}
            }
        }
    }
    let id = id
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .context("缺少可核实的原生会话标识")?;
    let text = text
        .filter(|text| !text.trim().is_empty())
        .context("原生执行未返回已确认的助手正文")?;
    anyhow::ensure!(completed, "原生执行缺少完成回执");
    Ok((text, key, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_command_receipts_require_the_native_session_and_terminal_event() {
        let id = uuid::Uuid::new_v4();
        let codex = format!(
            r#"{{"type":"thread.started","thread_id":"{id}"}}
{{"type":"turn.started"}}
{{"type":"item.completed","item":{{"type":"agent_message","text":"done"}}}}
{{"type":"turn.completed"}}"#
        );
        assert_eq!(
            managed_command_reply(AgentRuntimeKind::Codex, &codex)
                .unwrap()
                .0,
            "done"
        );
        assert!(managed_command_reply(
            AgentRuntimeKind::Codex,
            codex
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join("\n")
                .as_str()
        )
        .is_err());
        let pi = format!(
            r#"{{"type":"session","id":"{id}"}}
{{"type":"agent_start"}}
{{"type":"message_end","message":{{"role":"assistant","stopReason":"stop","content":[{{"type":"text","text":"done"}}]}}}}
{{"type":"agent_end"}}"#
        );
        assert_eq!(
            managed_command_reply(AgentRuntimeKind::Pi, &pi).unwrap().0,
            "done"
        );
        assert!(
            managed_command_reply(AgentRuntimeKind::Pi, &pi.replace("\"stop\"", "\"length\""))
                .is_err()
        );
        let claude = format!(
            r#"{{"type":"result","subtype":"success","is_error":false,"session_id":"{id}","result":"done"}}"#
        );
        let notice =
            "[claude-code:unrecognized_model] {\"model\":\"custom\",\"query_source\":\"sdk\"}\r\n";
        assert!(
            managed_command_reply(AgentRuntimeKind::ClaudeCode, &format!("{notice}{claude}"))
                .is_ok()
        );
        assert!(managed_command_reply(
            AgentRuntimeKind::ClaudeCode,
            &format!("untrusted\n{claude}")
        )
        .is_err());
        assert!(managed_command_reply(
            AgentRuntimeKind::ClaudeCode,
            &format!("{notice}{claude}\n{claude}")
        )
        .is_err());
        assert_eq!(
            managed_command_reply(AgentRuntimeKind::ClaudeCode, &claude)
                .unwrap()
                .0,
            "done"
        );
        assert!(managed_command_reply(
            AgentRuntimeKind::ClaudeCode,
            &claude.replace("false", "true")
        )
        .is_err());
    }
}
