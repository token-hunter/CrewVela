use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccount {
    pub index: usize,
    pub account: String,
    pub plan: Option<String>,
    pub next_used_percent: Option<f64>,
    pub next_reset: Option<String>,
    pub total_used_percent: Option<f64>,
    pub total_reset: Option<String>,
    pub current: bool,
    pub local_status: Option<String>,
    pub refresh_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountSnapshot {
    pub installed: bool,
    pub accounts: Vec<CodexAccount>,
    pub warnings: Vec<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountActionReceipt {
    pub message: String,
    pub snapshot: CodexAccountSnapshot,
}

fn resolve_cid() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CID_BIN").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    resolve_cid_on_path()
}

#[cfg(not(windows))]
fn resolve_cid_on_path() -> Option<PathBuf> {
    which::which("cid").ok()
}

#[cfg(windows)]
fn resolve_cid_on_path() -> Option<PathBuf> {
    let mut where_command = Command::new("where.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        where_command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(output) = where_command.arg("cid.cmd").output() {
        if output.status.success() {
            if let Some(line) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
            {
                let path = PathBuf::from(line);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }

    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("npm").join("cid.cmd"))
        .filter(|path| path.is_file())
}

pub fn is_installed() -> bool {
    resolve_cid().is_some()
}

fn validate_selector(selector: &str) -> Result<(), String> {
    let selector = selector.trim();
    if selector.is_empty() || selector.len() > 160 {
        return Err("账户标识无效".into());
    }
    if !selector
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '@' | '.' | '_' | '-' | '+'))
    {
        return Err("账户标识包含不支持的字符".into());
    }
    Ok(())
}

fn run_cid(args: &[&str]) -> Result<(String, String), String> {
    let cid =
        resolve_cid().ok_or_else(|| "未找到 cid，请先安装 codex-id 并执行 cid init".to_string())?;
    let output = cid_command(&cid, args)
        .output()
        .map_err(|error| format!("启动 cid 失败：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok((stdout, stderr))
}

fn cid_command(cid: &std::path::Path, args: &[&str]) -> Command {
    #[cfg(windows)]
    let mut command = Command::new("cmd.exe");
    #[cfg(not(windows))]
    let mut command = Command::new(cid);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
        command.args(["/D", "/C"]).arg(cid);
    }
    command.args(args).env("NO_COLOR", "1");
    command
}

fn parse_percent(value: &str) -> Option<f64> {
    value.strip_suffix('%')?.parse::<f64>().ok()
}

fn parse_column(tokens: &[&str], cursor: &mut usize) -> (Option<f64>, Option<String>) {
    let Some(value) = tokens.get(*cursor) else {
        return (None, None);
    };
    *cursor += 1;
    if *value == "-" {
        return (None, None);
    }
    let percent = parse_percent(value);
    let reset = tokens.get(*cursor).and_then(|candidate| {
        if candidate.starts_with('(') && candidate.ends_with(')') {
            *cursor += 1;
            Some(candidate.trim_matches(['(', ')']).to_string())
        } else {
            None
        }
    });
    (percent, reset)
}

fn warning_account(line: &str) -> Option<String> {
    let failure = line.find("续期失败")?;
    let remainder = line[failure + "续期失败".len()..].trim_start_matches(['：', ':', ' ']);
    remainder
        .split(['：', ':'])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_snapshot(stdout: &str, stderr: &str) -> CodexAccountSnapshot {
    let warnings = stderr
        .lines()
        .flat_map(|line| line.split('\r'))
        .map(str::trim)
        .filter(|line| line.contains("续期失败"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let failed_accounts = warnings
        .iter()
        .filter_map(|line| warning_account(line))
        .collect::<Vec<_>>();

    let usage_columns = stdout.lines().any(|line| {
        let line = line.trim();
        line.starts_with("No.") && line.contains("Plan") && line.contains("Total")
    });
    let mut accounts = Vec::new();
    for line in stdout.lines() {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.len() < 3 {
            continue;
        }
        let raw_index = tokens[0];
        let current = raw_index.ends_with('*');
        let Ok(index) = raw_index.trim_end_matches('*').parse::<usize>() else {
            continue;
        };
        let account = tokens[1].to_string();
        let (plan, next_used_percent, next_reset, total_used_percent, total_reset, local_status) =
            if usage_columns && tokens.len() >= 5 {
                let plan = (tokens[2] != "-").then(|| tokens[2].to_string());
                let mut cursor = 3;
                let (next_used_percent, next_reset) = parse_column(&tokens, &mut cursor);
                let (total_used_percent, total_reset) = parse_column(&tokens, &mut cursor);
                (
                    plan,
                    next_used_percent,
                    next_reset,
                    total_used_percent,
                    total_reset,
                    None,
                )
            } else {
                (None, None, None, None, None, Some(tokens[2..].join(" ")))
            };
        let refresh_error = failed_accounts
            .iter()
            .find(|failed| failed.as_str() == account)
            .and_then(|_| {
                warnings
                    .iter()
                    .find(|line| line.contains(&account))
                    .cloned()
            });
        accounts.push(CodexAccount {
            index,
            account,
            plan,
            next_used_percent,
            next_reset,
            total_used_percent,
            total_reset,
            current,
            local_status,
            refresh_error,
        });
    }

    CodexAccountSnapshot {
        installed: true,
        accounts,
        warnings,
        message: None,
    }
}

pub fn list() -> Result<CodexAccountSnapshot, String> {
    if resolve_cid().is_none() {
        return Ok(CodexAccountSnapshot {
            installed: false,
            accounts: Vec::new(),
            warnings: Vec::new(),
            message: Some("未找到 cid，请先安装 codex-id".into()),
        });
    }
    let (stdout, stderr) = run_cid(&["list"])?;
    Ok(parse_snapshot(&stdout, &stderr))
}

pub fn init() -> Result<CodexAccountActionReceipt, String> {
    let (message, _) = run_cid(&["init"])?;
    Ok(CodexAccountActionReceipt {
        message,
        snapshot: list()?,
    })
}

pub fn add() -> Result<CodexAccountActionReceipt, String> {
    let cid = resolve_cid().ok_or_else(|| "未找到 cid，请先安装 codex-id".to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let command_line = format!(
            "title CrewVela - Codex 登录 && call \"{}\" add",
            cid.display()
        );
        Command::new("cmd.exe")
            .args(["/D", "/K"])
            .arg(command_line)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|error| format!("打开 Codex 登录窗口失败：{error}"))?;
        return Ok(CodexAccountActionReceipt {
            message: "已打开 Codex 登录窗口；登录完成后点击“刷新登录态”。".into(),
            snapshot: list()?,
        });
    }
    #[cfg(not(windows))]
    {
        let _ = cid;
        Err("当前仅支持在 Windows 中打开 cid 登录窗口".into())
    }
}

pub fn add_token(token: String) -> Result<CodexAccountActionReceipt, String> {
    let token = token.trim();
    if token.len() < 16 || token.len() > 4096
        || !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+/=".contains(&byte)) {
        return Err("访问令牌格式无效".into());
    }
    let (message, _) = run_cid(&["add", token])?;
    Ok(CodexAccountActionReceipt {
        message,
        snapshot: list()?,
    })
}

pub fn use_account(selector: Option<String>) -> Result<CodexAccountActionReceipt, String> {
    if let Some(value) = selector.as_deref() {
        validate_selector(value)?;
        let (message, _) = run_cid(&["use", value])?;
        return Ok(CodexAccountActionReceipt {
            message,
            snapshot: list()?,
        });
    }

    let snapshot = list()?;
    let now = chrono::Local::now().naive_local();
    let selected = select_best_account(&snapshot.accounts, now)
        .ok_or_else(|| "没有找到具备剩余额度的可用账户，未切换账号".to_string())?;
    if selected.current {
        return Ok(CodexAccountActionReceipt {
            message: "当前账户仍有剩余额度，无需切换。".into(),
            snapshot,
        });
    }
    let selected_account = selected.account.clone();
    validate_selector(&selected_account)?;
    let (message, _) = run_cid(&["use", &selected_account])?;
    Ok(CodexAccountActionReceipt {
        message,
        snapshot: list()?,
    })
}

fn select_best_account(accounts: &[CodexAccount], now: NaiveDateTime) -> Option<&CodexAccount> {
    if let Some(current) = accounts
        .iter()
        .find(|account| account.current && account_is_selectable(account))
    {
        return Some(current);
    }
    accounts
        .iter()
        .filter(|account| account_is_selectable(account))
        .min_by(|left, right| {
            let left_reset = account_reset_distance_minutes(left, now).unwrap_or(i64::MAX);
            let right_reset = account_reset_distance_minutes(right, now).unwrap_or(i64::MAX);
            let left_remaining = account_remaining(left).unwrap_or_default();
            let right_remaining = account_remaining(right).unwrap_or_default();
            left_reset
                .cmp(&right_reset)
                .then_with(|| right_remaining.total_cmp(&left_remaining))
                .then_with(|| left.index.cmp(&right.index))
        })
}

fn account_is_selectable(account: &CodexAccount) -> bool {
    if account.refresh_error.is_some()
        || matches!(
            account.local_status.as_deref(),
            Some("离线" | "未登录" | "未知")
        )
    {
        return false;
    }
    account_remaining(account).is_some_and(|remaining| remaining > 0.0)
}

pub(crate) fn account_remaining(account: &CodexAccount) -> Option<f64> {
    // 历史字段名 used_percent 实际保存 cid 表格的剩余额度；任一窗口耗尽都不可用。
    let mut remaining: Option<f64> = None;
    for value in [account.total_used_percent, account.next_used_percent].into_iter().flatten() {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return None; // 已提供的窗口无效时，不能凭另一窗口推断账户可用。
        }
        remaining = Some(remaining.map_or(value, |current| current.min(value)));
    }
    remaining
}

fn account_reset_distance_minutes(account: &CodexAccount, now: NaiveDateTime) -> Option<i64> {
    let value = account
        .total_reset
        .as_deref()
        .or(account.next_reset.as_deref())?
        .trim();
    if let Ok(time) = NaiveTime::parse_from_str(value, "%H:%M") {
        let mut candidate = now.date().and_time(time);
        if candidate < now {
            candidate += chrono::Duration::days(1);
        }
        return Some((candidate - now).num_minutes().max(0));
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y/%m/%d"))
    {
        return Some((date - now.date()).num_days().max(0) * 24 * 60);
    }
    let (month, day) = value.split_once('-')?;
    let month = month.parse::<u32>().ok()?;
    let day = day.parse::<u32>().ok()?;
    let mut date = NaiveDate::from_ymd_opt(now.year(), month, day)?;
    if date < now.date() {
        date = NaiveDate::from_ymd_opt(now.year() + 1, month, day)?;
    }
    Some((date - now.date()).num_days().max(0) * 24 * 60)
}

pub fn remove(selector: String) -> Result<CodexAccountActionReceipt, String> {
    validate_selector(&selector)?;
    let (message, _) = run_cid(&["remove", selector.trim()])?;
    Ok(CodexAccountActionReceipt {
        message,
        snapshot: list()?,
    })
}

fn parse_codex_desktop_processes(output: &str) -> Vec<u32> {
    output
        .lines()
        .filter_map(|line| {
            let (pid, path) = line.trim().split_once('\t')?;
            let normalized = path.trim().replace('/', "\\").to_ascii_lowercase();
            let is_codex_desktop = normalized.contains("\\windowsapps\\openai.codex_")
                && normalized.ends_with("\\app\\chatgpt.exe");
            is_codex_desktop
                .then(|| pid.trim().parse::<u32>().ok())
                .flatten()
        })
        .collect()
}

// Pure script construction stays testable on Unix; only the Windows restart
// entry point below may execute it.
#[cfg(any(windows, test))]
fn restart_codex_helper_script() -> &'static str {
    r#"$processIds = @($env:CREWVELA_CODEX_PROCESS_IDS -split ',' | Where-Object { $_ -match '^\d+$' } | ForEach-Object { [int]$_ })
if ($processIds.Count -gt 0) {
  Start-Sleep -Milliseconds 350
  foreach ($processId in $processIds) {
    Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
  }
  Wait-Process -Id $processIds -Timeout 5 -ErrorAction SilentlyContinue
}
Start-Process -FilePath 'codex://'"#
}

#[cfg(windows)]
pub fn restart_codex_app() -> Result<String, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let discovery_script = r#"Get-Process -Name ChatGPT -ErrorAction SilentlyContinue | ForEach-Object {
  try {
    $path = $_.Path
    if ($path -and $path -match '\\WindowsApps\\OpenAI\.Codex_[^\\]+\\app\\ChatGPT\.exe$') {
      Write-Output ("{0}`t{1}" -f $_.Id, $path)
    }
  } catch {}
}"#;
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            discovery_script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("检查 Codex App 进程失败：{error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            "检查 Codex App 进程失败".into()
        } else {
            format!("检查 Codex App 进程失败：{error}")
        });
    }

    let process_ids = parse_codex_desktop_processes(&String::from_utf8_lossy(&output.stdout));
    let process_ids_env = process_ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            restart_codex_helper_script(),
        ])
        .env("CREWVELA_CODEX_PROCESS_IDS", &process_ids_env)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 Codex 重启程序失败：{error}"))?;

    Ok(if process_ids.is_empty() {
        "未检测到正在运行的 Codex App，正在启动…".into()
    } else {
        "正在重启 Codex App…".into()
    })
}

#[cfg(not(windows))]
pub fn restart_codex_app() -> Result<String, String> {
    Err("重启 Codex App 当前仅支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_command_preserves_platform_launch_and_literal_arguments() {
        let path = std::path::Path::new("/fixture with spaces/cid");
        let command = cid_command(path, &["use", "user+test@example.com"]);
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        if cfg!(windows) {
            assert_eq!(command.get_program(), "cmd.exe");
            assert_eq!(args, ["/D", "/C", "/fixture with spaces/cid", "use", "user+test@example.com"]);
        } else {
            assert_eq!(command.get_program(), path.as_os_str());
            assert_eq!(args, ["use", "user+test@example.com"]);
        }
    }

    #[cfg(unix)]
    #[test]
    fn cid_command_executes_unix_script_in_path_with_spaces() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("crewvela cid {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cid");
        std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$NO_COLOR\" \"$@\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = cid_command(&path, &["use", "user+test@example.com"]).output().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\nuse\nuser+test@example.com\n");
    }

    fn account(
        index: usize,
        remaining: Option<f64>,
        reset: Option<&str>,
        current: bool,
    ) -> CodexAccount {
        CodexAccount {
            index,
            account: format!("account-{index}@example.com"),
            plan: Some("Team".into()),
            next_used_percent: None,
            next_reset: None,
            total_used_percent: remaining,
            total_reset: reset.map(str::to_string),
            current,
            local_status: None,
            refresh_error: None,
        }
    }

    #[test]
    fn token_shell_payloads_are_rejected_before_running_cid() {
        for payload in ["abcdefghijklmnop&calc", "abcdefghijklmnop|calc", "abcdefghijklmnop%PATH%", "abcdefghijklmnop^calc", "abcdefghijklmnop>file", "abcdefghijklmnop(abc)", "abcdefghijklmnop\"", "abcdefghijklmnop\ncalc"] {
            assert_eq!(add_token(payload.into()).unwrap_err(), "访问令牌格式无效");
        }
    }

    #[test]
    fn auto_select_requires_remaining_quota_in_every_reported_window() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap().and_hms_opt(12, 0, 0).unwrap();
        for (next, total) in [(0.0, 70.0), (20.0, 0.0)] {
            let mut exhausted = account(1, Some(total), None, true);
            exhausted.next_used_percent = Some(next);
            let accounts = vec![exhausted, account(2, Some(30.0), None, false)];
            assert!(!account_is_selectable(&accounts[0]));
            assert_eq!(select_best_account(&accounts, now).unwrap().index, 2);
        }
        let mut candidate = account(1, Some(70.0), None, false);
        candidate.next_used_percent = Some(20.0);
        assert_eq!(account_remaining(&candidate), Some(20.0));
        candidate.total_used_percent = None;
        assert_eq!(account_remaining(&candidate), Some(20.0));
        candidate.next_used_percent = None;
        assert_eq!(account_remaining(&candidate), None);
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            candidate.total_used_percent = Some(invalid);
            candidate.next_used_percent = Some(20.0);
            assert!(!account_is_selectable(&candidate));
        }
    }

    #[test]
    fn auto_select_prefers_nearest_reset_and_rejects_zero_remaining() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 8, 19)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let accounts = vec![
            account(1, Some(1.0), Some("08-23"), false),
            account(2, Some(32.0), Some("08-21"), false),
            account(3, Some(0.0), Some("08-24"), false),
            account(4, Some(0.0), Some("08-24"), true),
            account(5, Some(35.0), Some("08-25"), false),
            account(6, Some(100.0), Some("08-26"), false),
        ];
        let selected = select_best_account(&accounts, now).unwrap();
        assert_eq!(selected.index, 2);
        assert!(!selected.current);
    }

    #[test]
    fn auto_select_uses_remaining_as_tiebreaker_for_same_reset() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 8, 19)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let accounts = vec![
            account(1, Some(20.0), Some("08-21"), false),
            account(2, Some(80.0), Some("08-21"), false),
        ];
        assert_eq!(select_best_account(&accounts, now).unwrap().index, 2);
    }

    #[test]
    fn auto_select_keeps_current_account_while_it_has_remaining_quota() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 8, 19)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let accounts = vec![
            account(2, Some(32.0), Some("08-21"), false),
            account(5, Some(27.0), Some("08-25"), true),
            account(6, Some(100.0), Some("08-26"), false),
        ];
        let selected = select_best_account(&accounts, now).unwrap();
        assert_eq!(selected.index, 5);
        assert!(selected.current);
    }

    #[test]
    fn parses_account_table_and_refresh_failures() {
        let stdout = "No. Account Plan Next Total\n1 first@example.com Team - 0% (08-18)\n2 second@example.com - - -\n3* third@example.com Team - 55% (08-18)";
        let stderr = "续期失败：second@example.com：刷新接口返回 401";
        let snapshot = parse_snapshot(stdout, stderr);
        assert_eq!(snapshot.accounts.len(), 3);
        assert_eq!(
            snapshot.accounts.iter().map(|account| account.account.as_str()).collect::<Vec<_>>(),
            ["first@example.com", "second@example.com", "third@example.com"]
        );
        assert!(snapshot.accounts[0].refresh_error.is_none());
        assert_eq!(snapshot.accounts[1].refresh_error.as_deref(), Some(stderr));
        assert!(snapshot.accounts[2].refresh_error.is_none());
        assert!(snapshot.accounts[2].current);
        assert_eq!(snapshot.accounts[2].total_used_percent, Some(55.0));
        assert_eq!(snapshot.accounts[2].total_reset.as_deref(), Some("08-18"));
        assert!(snapshot.accounts[1]
            .refresh_error
            .as_deref()
            .unwrap()
            .contains("401"));
    }

    #[test]
    fn parses_five_hour_and_weekly_quota_columns() {
        let stdout = "No. Account Plan Next Total\n1 first@example.com - - -\n2* second@example.com Plus 24% (16:18) 70% (09-01)";
        let snapshot = parse_snapshot(stdout, "");
        let account = &snapshot.accounts[1];

        assert_eq!(snapshot.accounts[0].account, "first@example.com");
        assert_eq!(account.account, "second@example.com");
        assert!(account.current);
        assert_eq!(account.next_used_percent, Some(24.0));
        assert_eq!(account.next_reset.as_deref(), Some("16:18"));
        assert_eq!(account.total_used_percent, Some(70.0));
        assert_eq!(account.total_reset.as_deref(), Some("09-01"));
    }

    #[test]
    fn parses_status_table_when_usage_is_unavailable() {
        let stdout = "No. Account Status\n1 first@example.com 未知\n2* current@example.com 已登录";
        let snapshot = parse_snapshot(stdout, "续期失败：first@example.com：fetch failed");
        assert_eq!(snapshot.accounts.len(), 2);
        assert_eq!(snapshot.accounts[0].local_status.as_deref(), Some("未知"));
        assert_eq!(snapshot.accounts[1].local_status.as_deref(), Some("已登录"));
        assert!(snapshot.accounts[1].current);
        assert!(snapshot.accounts[0].refresh_error.is_some());
    }

    #[test]
    fn parses_only_codex_desktop_processes() {
        let output = concat!(
            "12520\tC:\\Program Files\\WindowsApps\\OpenAI.Codex_26.803.10989.0_x64__2p2nqsd0c76g0\\app\\ChatGPT.exe\n",
            "50412\tC:\\Program Files\\WindowsApps\\OpenAI.Codex_26.803.10989.0_x64__2p2nqsd0c76g0\\app\\ChatGPT.exe\n",
            "57728\tC:\\Program Files\\WindowsApps\\OpenAI.Codex_26.803.10989.0_x64__2p2nqsd0c76g0\\app\\resources\\codex.exe\n",
            "12\tC:\\Program Files\\ChatGPT\\ChatGPT.exe\n",
            "not-a-pid\tC:\\Program Files\\WindowsApps\\OpenAI.Codex_fake\\app\\ChatGPT.exe\n",
        );

        assert_eq!(parse_codex_desktop_processes(output), vec![12520, 50412]);
    }

    #[test]
    fn restart_helper_stops_exact_processes_before_opening_codex_protocol() {
        let script = restart_codex_helper_script();
        let stop = script.find("Stop-Process -Id $processId").unwrap();
        let wait = script.find("Wait-Process -Id $processIds").unwrap();
        let reopen = script.find("codex://").unwrap();

        assert!(script.contains("$env:CREWVELA_CODEX_PROCESS_IDS"));
        assert!(stop < wait && wait < reopen);
        assert!(!script.contains("Stop-Process -Name"));
    }

    #[test]
    fn restart_helper_opens_codex_when_process_list_is_empty() {
        let script = restart_codex_helper_script();
        let guarded_stop = script.find("if ($processIds.Count -gt 0)").unwrap();
        let stop = script.find("Stop-Process -Id $processId").unwrap();
        let reopen = script.find("Start-Process -FilePath 'codex://'").unwrap();

        assert!(script.contains("Where-Object { $_ -match '^\\d+$' }"));
        assert!(guarded_stop < stop && stop < reopen);
        assert!(script[..reopen].trim_end().ends_with('}'));
    }

    #[cfg(not(windows))]
    #[test]
    fn restart_helper_reports_unsupported_platform_without_starting_processes() {
        assert_eq!(restart_codex_app().unwrap_err(), "重启 Codex App 当前仅支持 Windows");
    }
}
