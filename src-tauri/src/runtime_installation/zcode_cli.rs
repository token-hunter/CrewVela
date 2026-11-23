use super::{root, valid_version, AgentRuntimeKind, Environment};
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use std::{fs, path::{Path, PathBuf}, time::Duration};

pub(crate) const OVERRIDE_ENV: &str = "CREWVELA_ZCODE_CLI_EXECUTABLE";
const SELECTED_ENTRY: &str = "selected.json";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Details {
    can_select: bool,
    has_selection: bool,
    diagnosis: Option<String>,
}

fn directory() -> Result<PathBuf> { Ok(root()?.join(AgentRuntimeKind::ZCodeCli.as_str())) }

pub(crate) fn stored_executable() -> Result<Option<PathBuf>> {
    read_selection(&directory()?)
}

fn read_selection(directory: &Path) -> Result<Option<PathBuf>> {
    match fs::read(directory.join(SELECTED_ENTRY)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("ZCode CLI 路径设置损坏，请恢复自动检测"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("读取 ZCode CLI 路径设置失败"),
    }
}

fn validate_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute() && path.is_file(), "请选择现有的 ZCode CLI 文件");
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
    let shim = cfg!(windows) && (extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"));
    ensure!(shim || extension.eq_ignore_ascii_case("exe") || matches!(extension, "js" | "cjs") || cfg!(unix) && extension.is_empty(),
        "此文件类型不能用于启动 ZCode CLI");
    // cmd.exe expands these characters even when given a quoted program path.
    if shim && path.to_string_lossy().contains(['%', '!', '"', '^', '\r', '\n', '&', '|', '<', '>', '(', ')']) {
        bail!("此批处理路径包含 cmd.exe 特殊字符，请选择 CLI 的 .exe 或 .cjs 入口");
    }
    Ok(())
}

fn output(path: &Path, arguments: &[&str]) -> Result<std::process::Output> {
    let mut command = crate::multi_agent::zcode::command_for_entry(path)?;
    command.args(arguments);
    crate::title_summary::run_cli_with_timeout(command, None, Duration::from_secs(10), 64 * 1024, "ZCode CLI 自检")
}

fn version(path: &Path) -> Result<String> {
    let result = output(path, &["--version"])?;
    ensure!(result.status.success(), "ZCode CLI 版本检测失败");
    let output = String::from_utf8_lossy(&result.stdout);
    let version = output.trim().split_whitespace().last().unwrap_or_default();
    ensure!(valid_version(version), "未读取到有效的 ZCode CLI 版本");
    Ok(version.into())
}

fn diagnose(path: &Path, version: &str) -> Result<String> {
    let result = output(path, &["doctor", "--json"])?;
    ensure!(result.status.success(), "ZCode CLI 的 doctor 自检失败");
    summarize_diagnosis(&result.stdout, version)
}

fn summarize_diagnosis(stdout: &[u8], version: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(stdout).context("ZCode CLI 未返回有效的诊断数据")?;
    ensure!(value["cli"]["name"] == "zcode" && value["cli"]["version"] == version,
        "该文件的诊断身份或版本与 ZCode CLI 不符");
    let runtime = &value["runtime"];
    let platform = runtime["platform"].as_str().filter(|platform| matches!(*platform, "win32" | "darwin" | "linux"));
    let arch = runtime["arch"].as_str().filter(|arch| matches!(*arch, "x64" | "arm64" | "ia32"));
    let node = runtime["node"].as_str().filter(|node| node.strip_prefix('v').is_some_and(valid_version));
    let mut summary = String::from("CLI 自检通过");
    if let (Some(platform), Some(arch)) = (platform, arch) {
        summary.push_str(&format!(" · {platform} {arch}"));
    }
    if let Some(node) = node {
        summary.push_str(&format!(" · Node.js {node}"));
    }
    Ok(summary)
}

pub(super) fn inspect() -> Result<Environment> {
    let directory = directory()?;
    let can_select = std::env::var_os(OVERRIDE_ENV).is_none();
    let has_selection = read_selection(&directory).map_or(true, |choice| choice.is_some());
    let (executable, version, diagnosis, error) = match crate::multi_agent::zcode::cli_executable() {
        Ok(path) => match version(&path) {
            Ok(version) => {
                let checked = diagnose(&path, &version);
                (Some(path), Some(version), checked.as_ref().ok().cloned(), checked.err())
            }
            Err(error) => (Some(path), None, None, Some(error)),
        },
        Err(error) => (None, None, None, Some(error)),
    };
    let source = if !can_select { "环境变量指定" } else if has_selection { "手动选择" }
        else if executable.is_some() { "本机 PATH" } else { "未检测到 CLI" };
    Ok(Environment { kind: AgentRuntimeKind::ZCodeCli, supported: false,
        executable: executable.map(|path| path.display().to_string()), version, latest_version: None,
        source: source.into(), install_directory: directory.display().to_string(), auth: "unverified".into(),
        error: error.map(|error| error.to_string()), update_error: None, dependency_message: String::new(),
        chat: None, local_cli: Some(Details { can_select, has_selection, diagnosis }) })
}

fn save_selection(directory: &Path, path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        validate_path(path)?;
        diagnose(path, &version(path)?)?;
    }
    fs::create_dir_all(directory)?;
    crate::providers::write_atomic(&directory.join(SELECTED_ENTRY), &serde_json::to_vec(&path)?)
}

pub(crate) fn configure(path: Option<PathBuf>) -> Result<Environment> {
    ensure!(std::env::var_os(OVERRIDE_ENV).is_none(), "ZCode CLI 由 CREWVELA_ZCODE_CLI_EXECUTABLE 指定，请先移除该环境变量并重启 CrewVela");
    save_selection(&directory()?, path.as_deref())?;
    inspect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_requires_matching_identity_and_version() {
        let report = br#"{"cli":{"name":"zcode","version":"0.16.9"},"runtime":{"platform":"win32","arch":"x64","node":"v24.14.0"}}"#;
        assert_eq!(summarize_diagnosis(report, "0.16.9").unwrap(), "CLI 自检通过 · win32 x64 · Node.js v24.14.0");
        assert!(summarize_diagnosis(report, "0.16.8").is_err());
        assert!(summarize_diagnosis(br#"{"cli":{"name":"other","version":"0.16.9"}}"#, "0.16.9").is_err());
    }
}
