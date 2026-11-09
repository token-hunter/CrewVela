//! Process-local handoff policies; never rewrite native user configuration.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{path::PathBuf, process::Command};
#[cfg(test)]
thread_local! { pub(crate) static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }

fn root(variable: &str, fallback: &str) -> Result<PathBuf> {
    Ok(std::env::var_os(variable)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(
            dirs::home_dir()
                .context("无法定位 Agent 配置目录")?
                .join(fallback),
        ))
}
// Only known top-level scalar syntax is accepted. Merge keys/indirections
// fail closed rather than interpreting a partial TOML/YAML document.
fn scalar(source: &str, key: &str, separator: char) -> Result<Option<String>> {
    let mut result = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if separator == ':' && trimmed.starts_with("<<:") {
            bail!("权限配置使用 YAML 合并，无法安全核验");
        }
        let Some((left, right)) = trimmed.split_once(separator) else {
            continue;
        };
        if left.trim().trim_matches(['\'', '"']) != key {
            continue;
        }
        if line.starts_with(char::is_whitespace) || result.is_some() {
            bail!("权限配置格式不明确，无法安全核验");
        }
        let right = right.trim();
        let value = if let Some(quote @ ('\'' | '"')) = right.chars().next() {
            let end = right[1..].find(quote).context("权限配置字符串未结束")? + 1;
            let trailing = right[end + 1..].trim();
            if !trailing.is_empty() && !trailing.starts_with('#') {
                bail!("权限配置包含未支持的表达式");
            }
            right[1..end].to_owned()
        } else {
            right
                .split('#')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        };
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            bail!("权限配置包含未支持的值");
        }
        result = Some(value);
    }
    Ok(result)
}
pub(crate) fn preflight(kind: &str) -> Result<()> {
    if matches!(kind, "minimax_code" | "oh_my_pi") { bail!("该 CLI 已移除，不能启动或恢复"); }
    if kind != "kimi_code" { return Ok(()); }
    #[cfg(test)]
    if let Some(home) = TEST_HOME.with(|slot| slot.borrow().clone()) {
        return validate(&std::fs::read_to_string(home.join("kimi-config.toml")).unwrap_or_default());
    }
    let path = root("KIMI_CODE_HOME", ".kimi-code")?.join("config.toml");
    let source = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => bail!("无法读取目标 Agent 权限配置；来源保持不变"),
    };
    validate(&source)
}
fn validate(source: &str) -> Result<()> {
    let mode = scalar(source, "default_permission_mode", '=')?.unwrap_or_else(|| "manual".into());
    let legacy = scalar(source, "default_yolo", '=')?;
    if mode != "manual" || legacy.as_deref().is_some_and(|v| v != "false") {
        bail!("Kimi 已配置自动执行权限，请先把原生 default_permission_mode 设为 manual（旧版 default_yolo=false）；未停止来源");
    }
    Ok(())
}
pub(crate) fn opencode(command: &mut Command, id: &str, cli: bool) -> Result<()> {
    let mut config: Value = std::env::var("OPENCODE_CONFIG_CONTENT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("OpenCode 进程配置不是有效 JSON")?
        .unwrap_or_else(|| serde_json::json!({}));
    let name = format!("crewvela-handoff-{id}");
    let object = config
        .as_object_mut()
        .context("OpenCode 进程配置必须为对象")?;
    let agents = object
        .entry("agent")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("OpenCode Agent 配置必须为对象")?;
    agents.insert(
        name.clone(),
        serde_json::json!({"mode":"primary","permission":"ask","description":"CrewVela 接力任务"}),
    );
    object.insert("default_agent".into(), Value::String(name.clone()));
    command
        .env("OPENCODE_CONFIG_CONTENT", config.to_string())
        .env_remove("OPENCODE_PERMISSION");
    if cli {
        command.arg("--agent").arg(name);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_native_config_is_rejected_without_mutating_it() {
        assert!(validate("default_permission_mode = 'auto'").is_err());
        assert!(validate("default_yolo = true").is_err());
        assert!(validate("default_permission_mode = 'manual' # comment").is_ok());
    }
}
