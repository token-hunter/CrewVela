//! Standalone CLI launch choices. Do not apply project or chat permission profiles here.
use anyhow::{bail, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, process::Command, sync::OnceLock};

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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(catalog().len(), 6);
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
