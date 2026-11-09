use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct HarnessControl {
    inner: std::sync::Arc<ControlDirectory>,
}

struct ControlDirectory {
    root: PathBuf,
    token: String,
}

impl HarnessControl {
    pub fn configure(command: &mut Command) -> Result<Self> {
        let root = std::env::temp_dir().join(format!("crewvela-harness-control-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root)?;
        let control = Self { inner: std::sync::Arc::new(ControlDirectory { root, token: uuid::Uuid::new_v4().to_string() }) };
        let plugin = control.inner.root.join("bridge.mjs");
        fs::write(&plugin, include_str!("../resources/harness-control.mjs"))?;
        let plugin_uri = tauri::Url::from_file_path(crate::acp::normalize_harness_path(&plugin.canonicalize()?)?).map_err(|_| anyhow!("无法转换控制插件路径"))?;
        let patch = control.inner.root.join("patch.json");
        fs::write(&patch, serde_json::to_vec(&json!([
            {"id":"session-title-llm", "disabled":false},
            { "insert":[{"id":"crewvela-control-bridge", "name":plugin_uri.as_str()}] }
        ]))?)?;
        command.arg("--patch").arg(patch)
            .env("CREWVELA_HARNESS_CONTROL_ENDPOINT", control.inner.root.join("endpoint.json"))
            .env("CREWVELA_HARNESS_CONTROL_TOKEN", &control.inner.token);
        Ok(control)
    }

    fn connect(&self) -> Result<TcpStream> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let endpoint = loop {
            match fs::read(self.inner.root.join("endpoint.json")) {
                Ok(bytes) => break serde_json::from_slice::<Value>(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(error).context("Harness 控制插件未启动"),
            }
        };
        let port = endpoint["port"].as_u64().filter(|port| *port > 0 && *port <= 65535).ok_or_else(|| anyhow!("Harness 控制端口无效"))?;
        let socket = TcpStream::connect_timeout(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, port as u16).into(), Duration::from_secs(3))?;
        Ok(socket)
    }

    pub fn subscribe(&self, session_id: &str) -> Result<BufReader<TcpStream>> {
        let mut socket = self.connect()?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        let nonce = uuid::Uuid::new_v4().to_string();
        serde_json::to_writer(&mut socket, &json!({"token":self.inner.token,"nonce":nonce,"sessionId":session_id,"action":"stream"}))?;
        socket.write_all(b"\n")?;
        let mut reader = BufReader::new(socket);
        let mut response = String::new();
        (&mut reader).take(1024 * 1024).read_line(&mut response)?;
        let response: Value = serde_json::from_str(&response)?;
        if response["nonce"] != nonce || response["ready"] != true { return Err(anyhow!("Harness 实时流订阅失败：{response}")); }
        reader.get_ref().set_read_timeout(Some(Duration::from_millis(250)))?;
        Ok(reader)
    }

    pub fn request(&self, session_id: &str, action: &str, value: &str) -> Result<Value> {
        let mut socket = self.connect()?;
        socket.set_read_timeout(Some(Duration::from_secs(if action == "command" { 300 } else { 5 })))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        let nonce = uuid::Uuid::new_v4().to_string();
        serde_json::to_writer(&mut socket, &json!({"token":self.inner.token,"nonce":nonce,"sessionId":session_id,"action":action,"value":value}))?;
        socket.write_all(b"\n")?;
        let mut response = String::new();
        BufReader::new(socket).take(1024 * 1024).read_line(&mut response).context("Harness 控制连接中断或超时；上游操作可能已部分执行，请查看当前会话状态")?;
        let response: Value = serde_json::from_str(&response).context("Harness 控制响应无效")?;
        if response["nonce"] != nonce { return Err(anyhow!("Harness 控制响应标识不匹配")); }
        if let Some(error) = response["error"].as_str() { return Err(anyhow!("{error}")); }
        response.get("result").cloned().ok_or_else(|| anyhow!("Harness 控制响应缺少结果"))
    }
}

impl Drop for ControlDirectory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); }
}
