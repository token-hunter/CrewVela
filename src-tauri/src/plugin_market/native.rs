use super::*;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePluginReceipt {
    pub selector: String,
    pub scope: String,
    pub install_path: Option<String>,
    pub verified: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installed_at: Option<Value>,
    #[serde(default)]
    pub last_updated: Option<Value>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub source: Option<Value>,
    #[serde(default)]
    pub marketplace_source: Option<Value>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub config_root: Option<String>,
}

pub(super) fn run_claude(args: &[String]) -> Result<Value> {
    let executable = crate::launcher::resolve_claude()?;
    let mut command = crate::title_summary::claude_command(&executable)?;
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    run_json_command(command, Duration::from_secs(90))
}

fn terminate(child: &mut std::process::Child) {
    // Do not issue taskkill against an exited PID which may have been reused.
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn run_json_command(mut command: Command, timeout: Duration) -> Result<Value> {
    let mut child = command.spawn().context("启动 原生 CLI 插件命令失败")?;
    let mut stdout = child.stdout.take().context("读取 原生 CLI 输出失败")?;
    let mut stderr = child.stderr.take().context("读取 原生 CLI 错误失败")?;
    let (out_tx, out_rx) = std::sync::mpsc::sync_channel(1);
    let (err_tx, err_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = (&mut stdout)
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .and_then(|_| std::io::copy(&mut stdout, &mut std::io::sink()))
            .map(|_| bytes);
        let _ = out_tx.send(result);
    });
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = (&mut stderr)
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .and_then(|_| std::io::copy(&mut stderr, &mut std::io::sink()))
            .map(|_| bytes);
        let _ = err_tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            // Readers own their pipes and are detached on timeout. A surviving
            // descendant cannot turn the timeout into another unbounded wait.
            bail!("原生 CLI 插件命令超时；可能已产生部分安装，请重新检测后处理");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = out_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            terminate(&mut child);
            anyhow!("原生 CLI 标准输出等待超时，安装结果未核验")
        })??;
    let stderr = err_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            terminate(&mut child);
            anyhow!("原生 CLI 错误输出等待超时，安装结果未核验")
        })??;
    if !status.success() {
        bail!("原生 CLI 插件命令失败（退出码 {}）。请在 原生 CLI 插件管理中查看诊断；应用未保存命令输出。", status.code().map(|code| code.to_string()).unwrap_or_else(|| "未知".into()));
    }
    let _ = stderr;
    parse_json_result(&stdout)
}

fn parse_json_result(stdout: &[u8]) -> Result<Value> {
    if let Ok(value) = serde_json::from_slice(stdout) {
        return Ok(value);
    }
    String::from_utf8_lossy(stdout)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .ok_or_else(|| anyhow!("Claude 插件命令未返回有效 JSON"))
}

pub(super) fn list_args() -> Vec<String> {
    vec!["plugin".into(), "list".into(), "--json".into()]
}
pub(super) fn find_receipt(list: &Value, selector: &str) -> Result<Option<NativePluginReceipt>> {
    let entries = list
        .as_array()
        .ok_or_else(|| anyhow!("Claude 插件列表格式不兼容"))?;
    let matching = entries
        .iter()
        .filter(|entry| {
            entry.get("id").and_then(Value::as_str) == Some(selector)
                && entry.get("scope").and_then(Value::as_str) == Some("user")
        })
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        bail!("Claude 返回重复安装记录，拒绝认领");
    }
    let Some(entry) = matching.first() else {
        return Ok(None);
    };
    let path = entry
        .get("installPath")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| anyhow!("Claude 安装记录缺少路径，无法核验所有权"))?;
    Ok(Some(NativePluginReceipt {
        selector: selector.into(),
        scope: "user".into(),
        install_path: Some(path.into()),
        verified: true,
        version: entry
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
        installed_at: entry.get("installedAt").cloned(),
        last_updated: entry.get("lastUpdated").cloned(),
        runtime: None, source: None, marketplace_source: None, content_hash: None, config_root: None,
    }))
}

pub(super) fn install_args(name: &str, market: &Path) -> Vec<String> {
    vec![
        "plugin".into(),
        "install".into(),
        name.into(),
        "--marketplace".into(),
        market.display().to_string(),
        "--scope".into(),
        "user".into(),
        "--json".into(),
    ]
}
pub(super) fn remove_args(receipt: &NativePluginReceipt) -> Vec<String> {
    vec![
        "plugin".into(),
        "uninstall".into(),
        receipt.selector.clone(),
        "--scope".into(),
        "user".into(),
        "--keep-data".into(),
        "--json".into(),
    ]
}

pub(super) fn verify_owned(
    receipt: &NativePluginReceipt,
    list: &Value,
) -> Result<Option<NativePluginReceipt>> {
    let found = find_receipt(list, &receipt.selector)?;
    if !receipt.verified && found.is_some() {
        bail!("安装结果未核验，不可自动卸载或认领。请在 Claude 插件管理中检查并处理");
    }
    if let Some(current) = &found {
        if current.install_path != receipt.install_path
            || current.scope != receipt.scope
            || current.version != receipt.version
            || current.installed_at != receipt.installed_at
            || current.last_updated != receipt.last_updated
        {
            bail!("原生插件路径、版本或安装记录已变化，拒绝卸载外部安装");
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_requires_exact_selector_scope_path_and_keep_data() {
        let list = serde_json::json!([{ "id": "demo@market", "scope": "user", "installPath": "C:/cache/demo" }, { "id": "demo@other", "scope": "user", "installPath": "C:/cache/other" }]);
        let receipt = find_receipt(&list, "demo@market").unwrap().unwrap();
        assert!(verify_owned(&receipt, &list).unwrap().is_some());
        assert!(verify_owned(&receipt, &serde_json::json!([{ "id": "demo@market", "scope": "user", "installPath": "C:/external" }])).is_err());
        assert!(remove_args(&receipt).contains(&"--keep-data".into()));
        assert_eq!(
            install_args("demo", Path::new("C:/market with spaces"))[4],
            "C:/market with spaces"
        );
        assert!(find_receipt(&serde_json::json!([{ "id": "demo@market", "scope": "project", "installPath": "C:/project" }]), "demo@market").unwrap().is_none());
    }

    #[test]
    fn json_result_accepts_machine_readable_final_line() {
        assert_eq!(
            parse_json_result(b"Downloading...\n{\"success\":true}\n").unwrap()["success"],
            true
        );
        assert!(parse_json_result(b"No result JSON").is_err());
    }

    #[test]
    fn unverified_absent_receipt_can_be_cleared_but_existing_cannot_be_claimed() {
        let receipt = NativePluginReceipt {
            selector: "demo@market".into(),
            scope: "user".into(),
            install_path: None,
            verified: false,
            version: None,
            installed_at: None,
            last_updated: None,
            runtime: None, source: None, marketplace_source: None, content_hash: None, config_root: None,
        };
        assert!(verify_owned(&receipt, &serde_json::json!([]))
            .unwrap()
            .is_none());
        assert!(verify_owned(
            &receipt,
            &serde_json::json!([{ "id":"demo@market", "scope":"user", "installPath":"C:/unknown" }])
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn command_output_is_bounded_and_timeout_stops_the_child() {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Write-Output 'progress'; Write-Output '{\"success\":true}'",
            ])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        assert_eq!(
            run_json_command(command, Duration::from_secs(10)).unwrap()["success"],
            true
        );
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 20",
            ])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        assert!(run_json_command(command, Duration::from_millis(150))
            .unwrap_err()
            .to_string()
            .contains("超时"));
        assert!(start.elapsed() < Duration::from_secs(8));
    }
}
