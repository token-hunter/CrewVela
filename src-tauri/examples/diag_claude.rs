// YOLO 诊断：模拟 crewvela 完整 YOLO 启动（--settings 覆盖 user env +
// --dangerously-skip-permissions + reader 检测 trust 自动确认），验证跳过 Security guide。
use keyring::Entry;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::json;
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
                    if cc.is_ascii_alphabetic() {
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
    let key = Entry::new("crewvela", "glm")
        .ok()
        .and_then(|e| e.get_password().ok())
        .expect("无 glm key");

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    // 构造 --settings env（覆盖 user settings.json 的 env 块）
    let mut env = serde_json::Map::new();
    env.insert(
        "ANTHROPIC_BASE_URL".into(),
        json!("https://api.z.ai/api/anthropic"),
    );
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(key));
    env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), json!("glm-4.6"));
    env.insert(
        "ANTHROPIC_DEFAULT_SONNET_MODEL".into(),
        json!("glm-4.6"),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_HAIKU_MODEL".into(),
        json!("glm-4.5-air"),
    );
    let settings = json!({ "env": serde_json::Value::Object(env) }).to_string();

    let mut cmd = CommandBuilder::new(claude_exe);
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd.arg("--settings");
    cmd.arg(&settings);
    cmd.arg("--dangerously-skip-permissions");
    cmd.cwd(dirs::home_dir().unwrap());

    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let mut reader = pair.master.try_clone_reader().unwrap();
    let collected = Arc::new(Mutex::new(Vec::<u8>::new()));
    let c2 = Arc::clone(&collected);
    let writer_clone = Arc::clone(&writer);
    let h = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut auto_trust = true;
        let mut accumulated = String::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    c2.lock().unwrap().extend_from_slice(&buf[..n]);
                    accumulated += &String::from_utf8_lossy(&buf[..n]);
                    // 单词级检测（ANSI 不切断单词）：trust + folder 都在即 trust 对话框
                    if auto_trust && accumulated.contains("trust") && accumulated.contains("folder") {
                        std::thread::sleep(Duration::from_millis(800));
                        if let Ok(mut w) = writer_clone.lock() {
                            let wr = w.write_all(b"1\r");
                            let fl = w.flush();
                            eprintln!("[diag] auto-trust write={:?} flush={:?}", wr, fl);
                        }
                        auto_trust = false;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if h.is_finished() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let still_running = !h.is_finished();
    let _ = child.kill();
    let data = collected.lock().unwrap().clone();
    eprintln!(
        "[diag] {} bytes, still_running={}",
        data.len(),
        still_running
    );
    print!("{}", String::from_utf8_lossy(&data));
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(0);
}
