// PTY 探针：用 portable-pty 启动指定程序，采集 N 秒输出后打印。
// 用于诊断 claude.exe 在 conpty 下到底有没有输出。
// 用法: cargo run --example pty_probe -- <program> [args...]
//       pty_probe_env <program> <env_file> [args...]   (env_file: 每行 KEY=VALUE)

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match mode {
        "env" => {
            // pty_probe env <program> <env_file> [args...]
            let program = args.get(2).expect("need program");
            let env_file = args.get(3).expect("need env_file");
            let rest = &args[4..];
            run_with_env(program, rest, env_file, 8);
        }
        _ => {
            // pty_probe <program> [args...]
            let program = args.get(1).expect("need program");
            let rest = &args[2..];
            run(program, rest, &[], 5);
        }
    }
}

fn run_with_env(program: &str, rest: &[String], env_file: &str, secs: u64) {
    let envs: Vec<(String, String)> = std::fs::read_to_string(env_file)
        .expect("read env_file")
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    run(program, rest, &envs, secs);
}

fn run(program: &str, rest: &[String], envs: &[(String, String)], secs: u64) {
    eprintln!("spawn: {} {:?} (envs: {})", program, rest, envs.len());

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(program);
    for a in rest {
        cmd.arg(a);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }

    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("reader");
    let collected = Arc::new(Mutex::new(Vec::<u8>::new()));
    let collected_c = Arc::clone(&collected);
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => collected_c.lock().unwrap().extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
    });

    thread::sleep(Duration::from_secs(secs));
    let _ = child.kill();

    let data = collected.lock().unwrap().clone();
    eprintln!("=== captured {} bytes in {}s ===", data.len(), secs);
    println!("{}", String::from_utf8_lossy(&data));
    eprintln!("=== end (first 200 bytes hex) ===");
    let head: String = data.iter().take(200).map(|b| format!("{:02x} ", b)).collect();
    eprintln!("{}", head);
    let _ = handle.join();
}
