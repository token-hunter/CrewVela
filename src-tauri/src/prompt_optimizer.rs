//! 提示词优化：发送前用 LLM 把用户草稿改写得清晰完整。
//!
//! 复用协调器的非交互 claude CLI 管道（-p + stream-json + json-schema），
//! 但输出是纯文本优化稿；不带任何工具与项目上下文，单轮、可取消。

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

const OPTIMIZER_TIMEOUT: Duration = Duration::from_secs(45);

/// 优化结果：改写稿 + 一句话说明改动点。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptOptimization {
    pub optimized: String,
    pub notes: String,
}

struct OptimizerTransport {
    arguments: Vec<String>,
    stdin_payload: String,
}

fn optimizer_prompt_transport(prompt: String) -> OptimizerTransport {
    OptimizerTransport {
        arguments: Vec::new(),
        stdin_payload: prompt,
    }
}

const SYSTEM_PROMPT: &str = r#"你是 CrewVela 的提示词优化器。用户会给你一段准备发送给 AI 编程 Agent 的指令草稿；你的任务是把草稿改写得清晰、完整、可直接执行，而不是回答或执行它。

改写规则：
1. 保持用户的原始意图、范围与语气；不新增用户没提的需求，不删减已有要求。
2. 补全明显的缺省信息缺失项（交付物形式、目标文件/模块、验收方式），缺失且无法推断的信息用「（请补充：…）」占位，不要编造。
3. 消除歧义：把"它""这个""那个"等指代替换为草稿中的具体对象；模糊动词（优化一下、处理一下）改为可验证的动词。
4. 结构化：多条要求分行或编号；超过 3 条要求时按「目标/要求/验收」组织。
5. 保留草稿中的技术名词、路径、命令、@提及 原样不动。
6. notes 用一句话说明你主要改了什么（如"补齐了验收标准，拆分了 3 条并列要求"）；没有实质改动时 optimized 原样返回，notes 写"无实质改动"。
7. optimized 用草稿的原文语言；notes 用中文。"#;

const SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "optimized": {"type": "string"},
    "notes": {"type": "string"}
  },
  "required": ["optimized", "notes"]
}"#;

fn run_optimizer_attempt(claude: &Path, user_prompt: String) -> Result<PromptOptimization> {
    let _runtime_lease =
        crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::ClaudeCode)?;
    let mut command = crate::title_summary::claude_command(claude)?;
    let prompt = optimizer_prompt_transport(user_prompt);
    command
        .arg("-p")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--json-schema")
        .arg(SCHEMA)
        .arg("--no-session-persistence")
        .arg("--safe-mode")
        .arg("--tools")
        .arg("")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--max-turns")
        .arg("1")
        .arg("--system-prompt")
        .arg(SYSTEM_PROMPT)
        .args(&prompt.arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let output = crate::title_summary::run_cli_with_timeout(
        command, Some(prompt.stdin_payload.into_bytes()), OPTIMIZER_TIMEOUT,
        8 * 1024 * 1024, "提示词优化",
    )?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr).chars().take(400).collect();
        return Err(anyhow!("提示词优化器退出码 {}（{detail}）", output.status));
    }
    let raw = String::from_utf8(output.stdout).context("优化器输出不是有效 UTF-8")?;
    optimization_from_stream(&raw)
}

fn optimization_from_stream(raw: &str) -> Result<PromptOptimization> {
    let envelope = raw.lines().rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value.get("type").and_then(serde_json::Value::as_str) == Some("result"))
        .ok_or_else(|| anyhow!("优化器流式输出缺少最终结果"))?;
    if envelope.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        let detail: String = envelope.get("result").and_then(serde_json::Value::as_str)
            .unwrap_or("未知错误").chars().take(400).collect();
        return Err(anyhow!("提示词优化器返回错误：{detail}"));
    }
    // Claude's schema response lives beside `result` on the final envelope.
    // Older adapters embed the JSON (or its wrapper) inside the result string.
    let root = if let Some(structured) = envelope.get("structured_output") {
        structured.clone()
    } else {
        let result = envelope.get("result").and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("优化器输出缺少 result 字段"))?;
        serde_json::from_str::<serde_json::Value>(result.trim())
            .context("解析优化器 JSON 输出失败")?
    };
    let value = root.get("structured_output").unwrap_or(&root);
    let optimized = value
        .get("optimized")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("优化器输出缺少 optimized 字段"))?;
    let notes = value
        .get("notes")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if optimized.trim().is_empty() { return Err(anyhow!("优化器返回了空的优化稿")); }
    Ok(PromptOptimization {
        optimized: optimized.trim().to_string(),
        notes: notes.trim().to_string(),
    })
}

/// 优化提示词草稿。draft 为空返回错误；context 是可选的目标会话上下文。
pub fn optimize_prompt(draft: &str, context: Option<&str>) -> Result<PromptOptimization> {
    let _runtime_lease =
        crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::ClaudeCode)?;
    let draft = draft.trim();
    if draft.is_empty() {
        return Err(anyhow!("草稿不能为空"));
    }
    if draft.chars().count() > 8_000 {
        return Err(anyhow!("草稿过长（最多 8000 字）"));
    }
    let context = context.map(str::trim).filter(|value| !value.is_empty());
    let user_prompt = if let Some(context) = context {
        format!(
            "目标会话/项目上下文（仅供理解，不抄进结果）：\n{}\n\n指令草稿：\n{}",
            context, draft
        )
    } else {
        format!("指令草稿：\n{draft}")
    };
    let claude = crate::launcher::resolve_claude()?;
    run_optimizer_attempt(&claude, user_prompt)
}

#[cfg(test)]
mod tests {
    use super::{optimization_from_stream, optimize_prompt, PromptOptimization};

    #[test]
    fn empty_draft_is_rejected() {
        assert!(optimize_prompt("   ", None).is_err());
    }

    #[test]
    fn oversized_draft_is_rejected() {
        let draft = "长".repeat(8_001);
        assert!(optimize_prompt(&draft, None).is_err());
    }

    #[test]
    fn result_line_is_parsed_from_stream_footer() {
        let raw = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"result\",\"result\":\"{\\\"optimized\\\":\\\"实现登录页\\\",\\\"notes\\\":\\\"补齐交付物\\\"}\"}\n"
        );
        let parsed = optimization_from_stream(raw).unwrap();
        assert_eq!(parsed.optimized, "实现登录页");
        assert_eq!(parsed.notes, "补齐交付物");
    }

    #[test]
    fn structured_output_wrapper_is_unwrapped() {
        let raw = concat!(
            "{\"type\":\"result\",\"result\":\"{\\\"structured_output\\\":{\\\"optimized\\\":\\\"A\\\",\\\"notes\\\":\\\"B\\\"}}\"}\n"
        );
        let parsed = optimization_from_stream(raw).unwrap();
        assert_eq!(parsed.optimized, "A");
        assert_eq!(parsed.notes, "B");
    }

    #[test]
    fn structured_output_on_final_envelope_is_used_even_when_result_is_empty() {
        let raw = r#"{"type":"result","result":"","structured_output":{"optimized":"完善登录页","notes":"明确验收"}}"#;
        let parsed = optimization_from_stream(raw).unwrap();
        assert_eq!(parsed.optimized, "完善登录页");
        assert_eq!(parsed.notes, "明确验收");
    }

    #[test]
    fn error_envelope_cannot_fall_back_to_an_earlier_success_or_structured_output() {
        let raw = concat!(
            "{\"type\":\"result\",\"result\":\"{\\\"optimized\\\":\\\"old\\\"}\"}\n",
            "{\"type\":\"result\",\"is_error\":true,\"result\":\"API unavailable\",\"structured_output\":{\"optimized\":\"invalid\"}}\n"
        );
        assert!(optimization_from_stream(raw).unwrap_err().to_string().contains("API unavailable"));
        assert!(optimization_from_stream(r#"{"type":"result","structured_output":{"optimized":"  "}}"#).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn optimizer_reads_large_diagnostics_before_stdin_and_parses_schema_footer() {
        let root = std::env::temp_dir().join(format!("cc optimizer fixture {}", uuid::Uuid::new_v4()));
        let entry = root.join("node_modules/@anthropic-ai/claude-code/cli.js");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, r#"
            const fs = require('node:fs');
            fs.writeSync(1, ('{"type":"assistant","text":"progress"}\n').repeat(10000));
            fs.writeSync(2, 'diagnostic'.repeat(100000));
            let input = '';
            process.stdin.setEncoding('utf8');
            process.stdin.on('data', part => input += part);
            process.stdin.on('end', () => console.log(JSON.stringify({type:'result',result:'',structured_output:{optimized:input,notes:'fixture'}})));
        "#).unwrap();
        let wrapper = root.join("claude.cmd");
        std::fs::write(&wrapper, "@echo off\nnode \"%~dp0%node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\n").unwrap();
        let literal = "中文 & | <> ^ %变量% \"引号\"\n".repeat(4096);
        let result = super::run_optimizer_attempt(&wrapper, literal.clone()).unwrap();
        assert_eq!(result.optimized, literal.trim());
        assert_eq!(result.notes, "fixture");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_result_line_is_an_error() {
        assert!(optimization_from_stream("{\"type\":\"system\"}\n").is_err());
    }

    #[test]
    fn camel_case_serialization_matches_frontend_contract() {
        let value = serde_json::to_value(PromptOptimization {
            optimized: "x".into(),
            notes: "y".into(),
        })
        .unwrap();
        assert!(value.get("optimized").is_some());
        assert!(value.get("notes").is_some());
    }
}
