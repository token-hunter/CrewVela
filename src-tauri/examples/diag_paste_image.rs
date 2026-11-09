//! Claude Code 图片粘贴诊断探针。
//!
//! 前置条件：Windows 剪贴板中已有图片。本程序只启动本机已登录的 Claude Code，
//! 依次发送候选按键序列，不提交消息，也不发起模型请求。

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_CLAUDE_EXE: &str = if cfg!(windows) { "claude.exe" } else { "claude" };

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for cc in chars.by_ref() {
                    if cc.is_ascii_alphabetic() || cc == '~' {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn main() {
    let claude_exe =
        std::env::var("CREWVELA_CLAUDE_EXE").unwrap_or_else(|_| DEFAULT_CLAUDE_EXE.into());
    let candidate =
        std::env::var("CREWVELA_PASTE_CANDIDATE").unwrap_or_else(|_| "kitty-alt-v".into());
    let (candidate_name, sequence): (&str, &[u8]) = match candidate.as_str() {
        "ctrl-v" => ("legacy Ctrl+V", b"\x16"),
        "kitty-ctrl-v" => ("kitty Ctrl+V", b"\x1b[118;5u"),
        "kitty-alt-v" => ("kitty Alt+V", b"\x1b[118;3u"),
        _ => ("legacy Alt+V", b"\x1bv"),
    };

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("无法创建 PTY");

    let mut cmd = CommandBuilder::new(claude_exe);
    cmd.arg("--dangerously-skip-permissions");
    cmd.cwd(dirs::home_dir().expect("无法获取用户目录"));

    let mut child = pair.slave.spawn_command(cmd).expect("无法启动 Claude Code");
    drop(pair.slave);
    let writer = Arc::new(Mutex::new(
        pair.master.take_writer().expect("无法获取 PTY writer"),
    ));
    let mut reader = pair.master.try_clone_reader().expect("无法获取 PTY reader");
    let collected = Arc::new(Mutex::new(Vec::<u8>::new()));
    let collected_reader = Arc::clone(&collected);
    let reader_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => collected_reader
                    .lock()
                    .unwrap()
                    .extend_from_slice(&buf[..n]),
            }
        }
    });

    eprintln!("[probe] 等待 Claude Code 就绪");
    thread::sleep(Duration::from_secs(8));
    let before = collected.lock().unwrap().len();
    {
        let mut writer = writer.lock().unwrap();
        writer.write_all(sequence).expect("发送按键失败");
        writer.flush().expect("刷新按键失败");
    }
    eprintln!("[probe] 已发送 {candidate_name}: {sequence:?}");
    thread::sleep(Duration::from_secs(7));
    let after = collected.lock().unwrap().len();

    let _ = child.kill();
    drop(reader_thread);
    let data = collected.lock().unwrap().clone();
    let output_dir = std::env::temp_dir().join("crewvela-paste-image-probe");
    std::fs::create_dir_all(&output_dir).expect("无法创建诊断输出目录");
    let raw_path = output_dir.join("paste_probe_raw.bin");
    let clean_path = output_dir.join("paste_probe_clean.txt");
    std::fs::write(&raw_path, &data).unwrap();
    let clean = strip_ansi(&String::from_utf8_lossy(&data));
    std::fs::write(&clean_path, &clean).unwrap();

    let segment = strip_ansi(&String::from_utf8_lossy(&data[before..after]));
    let collapsed = segment.split_whitespace().collect::<Vec<_>>().join(" ");
    eprintln!("[probe] [Image] 次数: {}", clean.matches("[Image").count());
    eprintln!(
        "[probe] 增量输出: {}",
        collapsed.chars().take(500).collect::<String>()
    );
    eprintln!("[probe] 输出文件: {} / {}", raw_path.display(), clean_path.display());
}
