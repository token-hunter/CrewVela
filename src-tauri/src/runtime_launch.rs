//! Native CLI launch choices shared by the standalone and collaboration launchers.
use anyhow::{bail, ensure, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, process::Command, sync::OnceLock};
use crate::multi_agent::conversation_models::Selection;

#[derive(Debug, Deserialize)]
pub struct Choice { pub id: String, pub label: String, pub description: String }

pub fn catalog() -> &'static BTreeMap<String, Vec<Choice>> {
    static CATALOG: OnceLock<BTreeMap<String, Vec<Choice>>> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(include_str!("../../src/runtimeLaunchModes.json")).expect("bundled CLI launch catalog"))
}

pub fn validate(runtime: &str, mode: &str) -> Result<()> {
    if !catalog().get(runtime).is_some_and(|modes| modes.iter().any(|item| item.id == mode)) {
        bail!("不支持的 CLI 启动模式：{runtime} / {mode}");
    }
    Ok(())
}

pub(crate) fn label(runtime: &str, mode: &str) -> Option<&'static str> {
    catalog().get(runtime)?.iter().find(|choice| choice.id == mode).map(|choice| choice.label.as_str())
}

pub fn resolve(runtime: &str, requested: Option<&str>, defaults: &BTreeMap<String, String>) -> Result<String> {
    let mode = requested.or_else(|| defaults.get(runtime).map(String::as_str)).unwrap_or("cli_default");
    validate(runtime, mode)?;
    Ok(mode.into())
}

pub fn configure(command: &mut Command, runtime: &str, mode: &str) -> Result<()> {
    validate(runtime, mode)?;
    let args: Vec<&str> = match (runtime, mode) {
        (_, "cli_default") => vec![],
        ("claude_code", "manual") => vec!["--permission-mode", "default"],
        ("claude_code", "accept_edits") => vec!["--permission-mode", "acceptEdits"],
        ("claude_code", "auto") => vec!["--permission-mode", "auto"],
        ("claude_code", "plan") => vec!["--permission-mode", "plan"],
        ("claude_code", "yolo") => vec!["--permission-mode", "bypassPermissions", "--dangerously-skip-permissions"],
        ("codex", "workspace") => vec!["--sandbox", "workspace-write", "--ask-for-approval", "on-request"],
        ("codex", "read_only") => vec!["--sandbox", "read-only", "--ask-for-approval", "on-request"],
        ("codex", "auto") => vec!["--approve-for-me"],
        ("codex", "yolo") => vec!["--dangerously-bypass-approvals-and-sandbox"],
        ("kimi_code", "yolo") => vec!["--yolo"],
        ("kimi_code", "auto") => vec!["--auto"],
        ("kimi_code", "plan") => vec!["--plan"],
        ("zcode_cli", "guarded" | "build" | "edit" | "plan" | "yolo") => vec!["--mode", mode],
        ("opencode", "build" | "plan") => vec!["--agent", mode],
        ("opencode", "auto") => vec!["--auto"],
        ("pi", "read_tools") => vec!["--no-extensions", "--no-mcp", "--tools", "read,grep,find,ls"],
        _ => bail!("CLI 启动模式尚未接通：{runtime} / {mode}"),
    };
    command.args(args);
    Ok(())
}

pub(crate) fn valid_model_id(value: &str) -> bool {
    value.len() <= 256 && !value.starts_with('-') && !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:/+@-".contains(&b))
}

pub(crate) fn supports_reasoning(runtime: &str) -> bool {
    matches!(runtime, "claude_code" | "codex" | "pi" | "antigravity")
}

pub(crate) fn valid_reasoning(runtime: &str, value: &str) -> bool {
    match runtime {
        "claude_code" => matches!(value, "low" | "medium" | "high" | "xhigh" | "max" | "ultracode"),
        "codex" => matches!(value, "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"),
        "pi" => matches!(value, "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"),
        "antigravity" => matches!(value, "low" | "medium" | "high" | "xhigh" | "max"),
        _ => false,
    }
}

pub(crate) fn validate_models(runtime: &str, selection: &Selection) -> Result<()> {
    ensure!(supports_reasoning(runtime) || matches!(runtime, "kimi_code" | "opencode"), "该 Agent 不支持启动时选择模型或推理级别");
    ensure!(selection.model.as_deref().is_none_or(valid_model_id), "模型标识无法用于 CLI 启动");
    ensure!(selection.reasoning.as_deref().is_none_or(|value| valid_reasoning(runtime, value)), "推理级别无法用于 CLI 启动");
    Ok(())
}

pub(crate) fn configure_models(command: &mut Command, runtime: &str, selection: &Selection) -> Result<()> {
    validate_models(runtime, selection)?;
    if let Some(model) = &selection.model {
        command.args(["--model", model]);
    }
    if let Some(reasoning) = &selection.reasoning {
        match runtime {
            "claude_code" | "antigravity" => { command.args(["--effort", reasoning]); }
            "pi" => { command.args(["--thinking", reasoning]); }
            "codex" => { command.args(["--config", &format!("model_reasoning_effort=\"{reasoning}\"")]); }
            _ => bail!("该 Agent 不支持启动时选择推理级别"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_model_settings_reach_the_selected_cli_without_changing_launch_mode() {
        let models = Selection { model: Some("provider/model-v2".into()), reasoning: Some("high".into()) };
        let model_only = Selection { model: models.model.clone(), reasoning: None };
        for (kind, mode, selection, expected) in [
            ("claude_code", "plan", &models, vec!["--permission-mode", "plan", "--model", "provider/model-v2", "--effort", "high"]),
            ("codex", "read_only", &models, vec!["--sandbox", "read-only", "--ask-for-approval", "on-request",
                "--model", "provider/model-v2", "--config", "model_reasoning_effort=\"high\""]),
            ("pi", "read_tools", &models, vec!["--no-extensions", "--no-mcp", "--tools", "read,grep,find,ls",
                "--model", "provider/model-v2", "--thinking", "high"]),
            ("antigravity", "cli_default", &models, vec!["--model", "provider/model-v2", "--effort", "high"]),
            ("kimi_code", "plan", &model_only, vec!["--plan", "--model", "provider/model-v2"]),
            ("opencode", "build", &model_only, vec!["--agent", "build", "--model", "provider/model-v2"]),
        ] {
            let mut command = Command::new("fixture-cli");
            configure(&mut command, kind, mode).unwrap();
            configure_models(&mut command, kind, selection).unwrap();
            assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
            let mut defaults = Command::new("fixture-cli");
            configure_models(&mut defaults, kind, &Selection::default()).unwrap();
            assert_eq!(defaults.get_args().count(), 0);
        }
        assert!(validate_models("kimi_code", &models).is_err());
        assert!(validate_models("opencode", &models).is_err());
        assert!(validate_models("zcode_cli", &model_only).is_err());
        assert!(validate_models("claude_code", &Selection { model: Some("--help".into()), reasoning: None }).is_err());
        assert!(validate_models("codex", &Selection { model: None, reasoning: Some("ultracode".into()) }).is_err());
    }
    #[test]
    fn explicit_native_default_and_override_win_without_affecting_saved_defaults() {
        let defaults = BTreeMap::from([("codex".into(), "yolo".into()), ("pi".into(), "read_tools".into())]);
        assert_eq!(resolve("codex", None, &defaults).unwrap(), "yolo");
        assert_eq!(resolve("codex", Some("cli_default"), &defaults).unwrap(), "cli_default");
        assert_eq!(resolve("codex", Some("read_only"), &defaults).unwrap(), "read_only");
        assert_eq!(resolve("pi", None, &defaults).unwrap(), "read_tools");
        assert_eq!(resolve("claude_code", None, &defaults).unwrap(), "cli_default");
        assert!(resolve("pi", Some("yolo"), &defaults).is_err());
        assert_eq!(defaults["codex"], "yolo");
    }
    #[test]
    fn every_advertised_choice_is_wired_and_defaults_preserve_native_configuration() {
        assert_eq!(catalog().len(), 7);
        for (kind, choices) in catalog() {
            for choice in choices {
                assert!(!choice.label.is_empty() && !choice.description.is_empty());
                let mut command = Command::new("fixture-cli");
                command.arg("existing-resume-argument");
                configure(&mut command, kind, &choice.id).unwrap();
                let args = command.get_args().collect::<Vec<_>>();
                assert_eq!(args[0], "existing-resume-argument");
                assert_eq!(args.len() == 1, choice.id == "cli_default");
            }
        }
        for (kind, mode) in [("pi", "yolo"), ("opencode_chat", "auto"), ("codex", "unknown"), ("oh_my_pi", "cli_default")] {
            let mut command = Command::new("fixture-cli");
            assert!(configure(&mut command, kind, mode).is_err());
            assert_eq!(command.get_args().count(), 0);
        }
    }
    #[test]
    fn native_mode_meanings_do_not_get_conflated() {
        for (kind, mode, expected) in [
            ("kimi_code", "yolo", vec!["--yolo"]),
            ("kimi_code", "auto", vec!["--auto"]),
            ("opencode", "auto", vec!["--auto"]),
            ("opencode", "plan", vec!["--agent", "plan"]),
            ("zcode_cli", "guarded", vec!["--mode", "guarded"]),
            ("pi", "read_tools", vec!["--no-extensions", "--no-mcp", "--tools", "read,grep,find,ls"]),
        ] {
            let mut command = Command::new("fixture-cli"); configure(&mut command, kind, mode).unwrap();
            assert_eq!(command.get_args().map(|arg| arg.to_str().unwrap()).collect::<Vec<_>>(), expected);
        }
    }
}
