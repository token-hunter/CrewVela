// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

const CLI_USAGE: &str = "CrewVela — 多Agent协作工作台\n会话通信\n用法：CrewVela sessions\n      CrewVela send --to <会话ID> --body <消息> [--request-id <UUID>] [--reply-to <消息ID>]\n      CrewVela message-status <消息ID>\n新用户隔离测试：CrewVela --test-data-dir <独立测试目录绝对路径>\n无参数仅用于从桌面启动应用。";

fn may_start_gui(first: Option<&str>, agent_identity: bool) -> bool {
    first.is_none() && !agent_identity
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut first = args.next();
    if first.as_deref()==Some("--test-data-dir") {
        let root=args.next().filter(|_|args.next().is_none());
        let result=root.as_deref().ok_or_else(||anyhow::anyhow!("--test-data-dir 需要唯一的测试目录绝对路径"))
            .and_then(|path|crewvela_lib::test_profile::initialize(std::path::Path::new(path)));
        if let Err(error)=result { eprintln!("{error:#}");std::process::exit(2); }
        first=None;
    }
    match first.as_deref() {
        Some("--project-memory-mcp") => {
            let result = (|| -> anyhow::Result<()> {
                let identity = args.next().ok_or_else(|| anyhow::anyhow!("共享记忆工具缺少会话身份"))?;
                let token = args.next().ok_or_else(|| anyhow::anyhow!("共享记忆工具缺少会话授权"))?;
                if let Some(root) = args.next() {
                    if args.next().is_some() { anyhow::bail!("共享记忆工具参数无效"); }
                    crewvela_lib::test_profile::initialize(std::path::Path::new(&root))?;
                }
                crewvela_lib::run_project_memory_mcp(&identity, &token)
            })();
            if let Err(error) = result {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("--session-message-mcp") => {
            let Some(identity) = args.next().filter(|_| args.next().is_none()) else {
                eprintln!("会话通信 MCP 需要唯一的原会话身份");
                std::process::exit(2);
            };
            if let Err(error) = crewvela_lib::run_session_message_mcp(&identity) {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("--help" | "-h" | "help") => {
            println!("{CLI_USAGE}");
            return;
        }
        Some("--version" | "-V") => {
            println!("CrewVela {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--session-hook") => {
            let Some(events_dir) = args.next() else {
                eprintln!("缺少 --session-hook 事件目录");
                std::process::exit(2);
            };
            let app_session_id = args.next();
            if let Err(error) =
                crewvela_lib::run_session_hook(&events_dir, app_session_id.as_deref())
            {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("--agent-turn-hook") => {
            let (Some(events_dir), Some(app_session_id), Some(payload)) =
                (args.next(), args.next(), args.next())
            else {
                eprintln!("缺少 --agent-turn-hook 参数");
                std::process::exit(2);
            };
            if let Err(error) =
                crewvela_lib::run_agent_turn_hook(&events_dir, &app_session_id, &payload)
            {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("--session-statusline") => {
            let (Some(events_dir), Some(app_session_id)) = (args.next(), args.next()) else {
                eprintln!("缺少 --session-statusline 参数");
                std::process::exit(2);
            };
            match crewvela_lib::run_session_statusline(&events_dir, &app_session_id) {
                Ok(line) => println!("{line}"),
                Err(error) => {
                    eprintln!("{error:#}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("send") => {
            if let Err(error) = crewvela_lib::run_send_args(args) {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("message-status") => {
            let Some(id) = args.next() else {
                eprintln!("用法: crewvela message-status <消息 ID>");
                std::process::exit(2);
            };
            if args.next().is_some() { eprintln!("message-status 只接受一个消息 ID"); std::process::exit(2); }
            if let Err(error) = crewvela_lib::run_message_status(&id) {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("sessions") => {
            if args.next().is_some() {
                eprintln!("sessions 不接受参数。\n{CLI_USAGE}");
                std::process::exit(2);
            }
            if let Err(error) = crewvela_lib::run_sessions_command() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
            return;
        }
        _ if may_start_gui(
            first.as_deref(),
            std::env::var_os("CREWVELA_SESSION_ID").is_some() && !crewvela_lib::test_profile::active(),
        ) => {}
        _ => {
            eprintln!("无效或缺失的 CLI 子命令。\n{CLI_USAGE}");
            std::process::exit(2);
        }
    }
    // WebView2 硬件加速开关必须在任何 WebView 创建前生效；
    // main.rs 的 GUI 启动路径是进程内最早的确定性时机。
    crewvela_lib::general_settings::apply_webview2_env_overrides();
    crewvela_lib::run()
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_desktop_empty_invocation_can_start_gui() {
        assert!(super::may_start_gui(None, false));
        assert!(!super::may_start_gui(None, true));
        for arg in [
            "--help",
            "-h",
            "help",
            "--version",
            "sessions",
            "send",
            "studio",
            "invalid",
        ] {
            assert!(!super::may_start_gui(Some(arg), false));
            assert!(!super::may_start_gui(Some(arg), true));
        }
    }
}
