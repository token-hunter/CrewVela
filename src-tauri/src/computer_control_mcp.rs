//! 电脑控制 MCP server（`crewvela --computer-control-mcp <runtime.ps1>`）。
//! 运行时移植自 orca（stablyai/orca，MIT）：PowerShell + .NET UIA + SendInput，
//! NDJSON 行协议（先 `{"ready":true}`，requestId 回显防失步）。
//! 安全增强：get_state 返回 snapshotId；纯坐标类动作必须携带最新 snapshotId，
//! 元素类动作按 runtimeId 绑定由运行时兜底校验。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

const RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(45);
const RUNTIME_CALL_TIMEOUT: Duration = Duration::from_secs(90);

// ---- PowerShell 运行时客户端 ----

/// 管理常驻 `-Serve` 进程：RemoteSigned 优先，策略拦截时回退 Bypass 一次
/// （对齐 orca 的实践：Bypass 是 Defender 权重最高的命令行标记，仅在必要时使用）。
pub(crate) struct RuntimeClient {
    script: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Option<mpsc::Receiver<String>>,
    next_request_id: u64,
    bypassed: bool,
}

/// Failed startup must reap the local child even before it is published on RuntimeClient.
struct StartupChild(Option<Child>);
impl Drop for StartupChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl RuntimeClient {
    pub(crate) fn new(script: PathBuf) -> Self {
        Self {
            script,
            child: None,
            stdin: None,
            lines: None,
            next_request_id: 1,
            bypassed: false,
        }
    }

    fn spawn_command(script: &PathBuf, bypass: bool) -> Command {
        let mut command = Command::new("powershell.exe");
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg(if bypass { "Bypass" } else { "RemoteSigned" })
            .arg("-File")
            .arg(script)
            .arg("-Serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }

    fn start(&mut self) -> Result<()> {
        self.start_command(Self::spawn_command(&self.script, self.bypassed), RUNTIME_READY_TIMEOUT)
    }

    fn start_command(&mut self, mut command: Command, ready_timeout: Duration) -> Result<()> {
        self.stop();
        let mut owned = StartupChild(Some(command.spawn().context("启动 PowerShell 电脑控制运行时失败")?));
        let child = owned.0.as_mut().expect("startup child");
        let stdin = child
            .stdin
            .take()
            .context("运行时没有可写入的 stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("运行时没有可读取的 stdout")?;
        let stderr = child.stderr.take().context("运行时没有 stderr")?;
        let stderr_tail = capture_stderr_tail(stderr);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        // 等待 {"ready":true} 通告；策略拦截时回退 Bypass 重试一次。
        let deadline = Instant::now() + ready_timeout;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                let tail = stderr_tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let message = format!(
                    "运行时未在 {}s 内就绪{}",
                    ready_timeout.as_secs(),
                    if tail.is_empty() {
                        String::new()
                    } else {
                        format!("；stderr 尾部：{tail}")
                    }
                );
                drop(owned);
                self.stop();
                if !self.bypassed && tail.contains("UnauthorizedAccess") {
                    self.bypassed = true;
                    return self.start();
                }
                bail!("{message}");
            }
            match receiver.recv_timeout(timeout.min(Duration::from_secs(2))) {
                Ok(line) => {
                    let parsed: Value = serde_json::from_str(line.trim())
                        .with_context(|| format!("ready 行不是 JSON：{line}"))?;
                    if parsed.get("ready").and_then(Value::as_bool) == Some(true) {
                        self.child = owned.0.take();
                        self.stdin = Some(stdin);
                        self.lines = Some(receiver);
                        return Ok(());
                    }
                    bail!("运行时首行不是 ready 通告：{line}");
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let tail = stderr_tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    drop(owned);
                    self.stop();
                    if !self.bypassed && tail.contains("UnauthorizedAccess") {
                        self.bypassed = true;
                        return self.start();
                    }
                    bail!("运行时进程在就绪前退出；stderr 尾部：{tail}");
                }
            }
        }
    }

    fn stop(&mut self) {
        self.stdin = None;
        self.lines = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn ensure_started(&mut self) -> Result<()> {
        if self.child.is_none() {
            self.start()?;
        }
        Ok(())
    }

    /// 发送一个操作并等待对应 requestId 的响应；失步（回显了别人的 id 或坏行）
    /// 视为流损坏：杀掉运行时，本次报错，下次调用自动重启。
    pub(crate) fn call(&mut self, mut operation: Value) -> Result<Value> {
        self.ensure_started()?;
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        operation["requestId"] = json!(request_id);
        let line = serde_json::to_string(&operation).context("序列化运行时请求失败")?;
        let write_result = (|| -> Result<()> {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow!("运行时 stdin 已关闭"))?;
            stdin
                .write_all(line.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .and_then(|_| stdin.flush())
                .context("写入运行时请求失败")?;
            Ok(())
        })();
        if let Err(error) = write_result {
            self.stop();
            return Err(error.context("运行时输入通道已关闭，本次操作结果未知；请重新观察后重试"));
        }
        let receiver = self
            .lines
            .as_ref()
            .ok_or_else(|| anyhow!("运行时输出通道已关闭"))?;
        let deadline = Instant::now() + RUNTIME_CALL_TIMEOUT;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                self.stop();
                bail!("运行时响应超时（{}s）", RUNTIME_CALL_TIMEOUT.as_secs());
            }
            match receiver.recv_timeout(timeout) {
                Ok(raw) => {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let parsed: Value = match serde_json::from_str(trimmed) {
                        Ok(value) => value,
                        Err(_) => {
                            self.stop();
                            bail!("运行时输出失步（非 JSON 行），已重启通道");
                        }
                    };
                    match parsed.get("requestId").and_then(Value::as_u64) {
                        Some(id) if id == request_id => {
                            return Ok(parsed);
                        }
                        Some(_) | None => {
                            self.stop();
                            bail!("运行时输出失步（requestId 不匹配），已重启通道");
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.stop();
                    bail!("运行时响应超时（{}s）", RUNTIME_CALL_TIMEOUT.as_secs());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.stop();
                    bail!("运行时进程退出，本次操作结果未知；请重新观察后重试");
                }
            }
        }
    }
}

impl Drop for RuntimeClient {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture_stderr_tail(stderr: impl Read + Send + 'static) -> std::sync::Arc<std::sync::Mutex<String>> {
    let tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let target = std::sync::Arc::clone(&tail);
    std::thread::spawn(move || {
        let mut reader = stderr;
        let mut chunk = [0u8; 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut value) = target.lock() {
                        value.push_str(&String::from_utf8_lossy(&chunk[..read]));
                        if value.len() > 4096 {
                            let mut drain_to = value.len() - 3072;
                            while !value.is_char_boundary(drain_to) { drain_to += 1; }
                            value.drain(..drain_to);
                        }
                    }
                }
            }
        }
    });
    tail
}

// ---- 会话状态（安全门 + 元素索引缓存） ----

#[derive(Default)]
pub(crate) struct SessionState {
    last_snapshot_id: Option<String>,
    snapshot_app: Option<Value>,
    snapshot_window_id: Option<i64>,
    snapshot_app_query: Option<String>,
    elements: Vec<Value>,
}

/// 坐标类动作的 snapshotId 门：不带元素引用、纯靠 x/y 定位的点击/滚动/拖拽
/// 必须携带最近一次观察返回的 snapshotId，防止拿旧图坐标误点已变化的界面。
pub(crate) fn validate_coordinate_gate(
    uses_coordinates: bool,
    snapshot_id: Option<&str>,
    state: &SessionState,
) -> Result<()> {
    if !uses_coordinates {
        return Ok(());
    }
    let expected = state
        .last_snapshot_id
        .as_deref()
        .ok_or_else(|| anyhow!("坐标动作需要先 computer_get_state 观察界面并携带返回的 snapshotId"))?;
    let provided = snapshot_id.ok_or_else(|| {
        anyhow!("坐标动作缺少 snapshotId；先 computer_get_state，并把返回的 snapshotId 一并传入")
    })?;
    if provided != expected {
        bail!("snapshotId 已过期（界面变化或不是最近一次观察）；请重新 computer_get_state 后再试");
    }
    Ok(())
}

fn absorb_snapshot(state: &mut SessionState, response: &Value) {
    if let Some(snapshot) = response.get("snapshot") {
        state.snapshot_app = snapshot.get("app").cloned();
        state.snapshot_window_id = snapshot.get("windowId").and_then(Value::as_i64);
        state.last_snapshot_id = None;
        state.elements.clear();
        if let Some(id) = snapshot.get("snapshotId").and_then(Value::as_str) {
            state.last_snapshot_id = Some(id.to_string());
        }
        if let Some(elements) = snapshot.get("elements").and_then(Value::as_array) {
            state.elements = elements.clone();
        }
    }
}

/// Cached coordinates and UIA elements belong to one observed process/window.
fn bind_snapshot_target(operation: &mut Value, state: &SessionState) -> Result<()> {
    let app = state.snapshot_app.as_ref().context("快照缺少应用身份，请重新 computer_get_state")?;
    let pid = app.get("pid").and_then(Value::as_u64).filter(|pid| *pid > 0)
        .context("快照缺少进程身份，请重新 computer_get_state")?;
    let window = state.snapshot_window_id.filter(|id| *id != 0)
        .context("快照缺少窗口身份，请重新 computer_get_state")?;
    let pid_query = pid.to_string();
    if let Some(provided) = operation.get("app").and_then(Value::as_str) {
        let valid = provided == pid_query
            || state.snapshot_app_query.as_deref().is_some_and(|query| query.eq_ignore_ascii_case(provided))
            || ["name", "bundleIdentifier", "bundleId"].into_iter().any(|key| app.get(key).and_then(Value::as_str).is_some_and(|name| name.eq_ignore_ascii_case(provided)));
        anyhow::ensure!(valid, "目标应用与最近快照不一致，请先观察目标应用");
    }
    if let Some(provided) = operation.get("windowId").and_then(Value::as_i64) {
        anyhow::ensure!(provided == window, "目标窗口与最近快照不一致，请重新观察该窗口");
    }
    operation["app"] = json!(pid_query);
    operation["windowId"] = json!(window);
    Ok(())
}

fn element_record(state: &SessionState, index: i64) -> Result<Value> {
    state
        .elements
        .iter()
        .find(|element| element.get("index").and_then(Value::as_i64) == Some(index))
        .cloned()
        .ok_or_else(|| anyhow!("元素索引 {index} 不在最近一次快照里；请重新 computer_get_state"))
}

// ---- 工具面 ----

pub(crate) struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

pub(crate) fn tool_specs() -> Vec<ToolSpec> {
    let app = || {
        json!({
            "type": "string",
            "description": "目标应用：进程名（如 notepad、chrome）、pid 或窗口标题关键词"
        })
    };
    let element_index = || {
        json!({
            "type": "integer",
            "description": "最近一次快照树行首的元素索引；优先用元素而不是坐标"
        })
    };
    let snapshot_id = || {
        json!({
            "type": "string",
            "description": "纯坐标动作必填：最近一次 computer_get_state 返回的 snapshotId"
        })
    };
    let restore = || {
        json!({
            "type": "boolean",
            "description": "动作前把目标窗口恢复并置前（最小化时需要）",
            "default": true
        })
    };
    vec![
        ToolSpec {
            name: "computer_list_apps",
            description: "列出当前有可见窗口的应用（进程名/pid/标题），作为后续操作的 app 参数来源。",
            schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
        ToolSpec {
            name: "computer_get_state",
            description: "观察目标应用：返回无障碍树（每行首是元素索引）+ 可选窗口截图 + snapshotId。这是所有操作前的第一步。",
            schema: json!({
                "type":"object",
                "required":["app"],
                "properties":{
                    "app": app(),
                    "windowId": {"type":"integer","description":"目标窗口句柄（多窗口时用于校验）"},
                    "screenshot": {"type":"boolean","description":"是否附窗口截图（PNG）；默认包含","default":true},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_click",
            description: "点击元素（优先，走可访问性语义）或窗口坐标（需 snapshotId）。返回动作后的新快照。",
            schema: json!({
                "type":"object",
                "required":["app"],
                "properties":{
                    "app": app(),
                    "elementIndex": element_index(),
                    "x": {"type":"number","description":"窗口相对 X 坐标（elementIndex 缺失时使用）"},
                    "y": {"type":"number","description":"窗口相对 Y 坐标"},
                    "button": {"type":"string","enum":["left","right","middle"],"default":"left"},
                    "clickCount": {"type":"integer","minimum":1,"default":1},
                    "modifiers": {"type":"string","description":"修饰键，如 \"ctrl+shift\""},
                    "snapshotId": snapshot_id(),
                    "screenshot": {"type":"boolean","description":"响应是否附截图，默认否（用树确认即可）","default":false},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_type_text",
            description: "向已聚焦的目标窗口输入文本（键盘要求目标窗口持有焦点）。",
            schema: json!({
                "type":"object",
                "required":["app","text"],
                "properties":{
                    "app": app(),
                    "text": {"type":"string"},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_press_key",
            description: "按单个键（enter/tab/escape/backspace/delete/space/方向键/单字符）。",
            schema: json!({
                "type":"object",
                "required":["app","key"],
                "properties":{
                    "app": app(),
                    "key": {"type":"string"},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_hotkey",
            description: "组合键，如 \"ctrl+c\"、\"ctrl+shift+t\"。",
            schema: json!({
                "type":"object",
                "required":["app","key"],
                "properties":{
                    "app": app(),
                    "key": {"type":"string","description":"如 ctrl+c；修饰键支持 ctrl/shift/alt"},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_paste_text",
            description: "经剪贴板粘贴文本（输入大段内容比逐键快；会临时占用并恢复剪贴板）。",
            schema: json!({
                "type":"object",
                "required":["app","text"],
                "properties":{
                    "app": app(),
                    "text": {"type":"string"},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_scroll",
            description: "滚动：方向 up/down/left/right，pages 为页数。可带元素或坐标（坐标需 snapshotId）。",
            schema: json!({
                "type":"object",
                "required":["app","direction"],
                "properties":{
                    "app": app(),
                    "direction": {"type":"string","enum":["up","down","left","right"]},
                    "pages": {"type":"number","minimum":0.1,"default":1},
                    "elementIndex": element_index(),
                    "x": {"type":"number"},
                    "y": {"type":"number"},
                    "snapshotId": snapshot_id(),
                    "screenshot": {"type":"boolean","default":false},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_drag",
            description: "从某元素/坐标拖拽到另一元素/坐标（纯坐标需 snapshotId）。",
            schema: json!({
                "type":"object",
                "required":["app"],
                "properties":{
                    "app": app(),
                    "fromElementIndex": element_index(),
                    "fromX": {"type":"number"},
                    "fromY": {"type":"number"},
                    "toElementIndex": element_index(),
                    "toX": {"type":"number"},
                    "toY": {"type":"number"},
                    "snapshotId": snapshot_id(),
                    "screenshot": {"type":"boolean","default":false},
                    "restoreWindow": restore(),
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_set_value",
            description: "用可访问性 ValuePattern 直接设置元素值（文本框首选，不经键盘）。",
            schema: json!({
                "type":"object",
                "required":["app","elementIndex","value"],
                "properties":{
                    "app": app(),
                    "elementIndex": element_index(),
                    "value": {"type":"string"},
                    "screenshot": {"type":"boolean","default":false},
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "computer_perform_action",
            description: "对元素执行具名可访问性动作：invoke/select/toggle（快照行会标注可用动作）。",
            schema: json!({
                "type":"object",
                "required":["app","elementIndex","action"],
                "properties":{
                    "app": app(),
                    "elementIndex": element_index(),
                    "action": {"type":"string","enum":["invoke","select","toggle"]},
                    "screenshot": {"type":"boolean","default":false},
                },
                "additionalProperties": false
            }),
        },
    ]
}

// ---- 工具调用 → 运行时操作 ----

fn bool_param(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

fn number_param(params: &Value, key: &str) -> Option<f64> {
    params.get(key).and_then(Value::as_f64)
}

fn string_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn dispatch_tool(
    runtime: &mut RuntimeClient,
    state: &mut SessionState,
    name: &str,
    params: &Value,
) -> Result<Vec<Value>> {
    let screenshot_wanted = bool_param(params, "screenshot");
    let mut operation = json!({ "tool": tool_field(name) });
    if let Some(app) = string_param(params, "app") {
        operation["app"] = json!(app);
    }
    if let Some(id) = params.get("windowId").and_then(Value::as_i64) {
        operation["windowId"] = json!(id);
    }
    if let Some(restore) = bool_param(params, "restoreWindow") {
        operation["restoreWindow"] = json!(restore);
    }
    match name {
        "computer_list_apps" => {}
        "computer_get_state" => {
            // 观察默认带截图；后续动作用树确认即可（省 token）。
            operation["noScreenshot"] = json!(screenshot_wanted == Some(false));
        }
        "computer_click" => {
            let element = params.get("elementIndex").and_then(Value::as_i64);
            match element {
                Some(index) => operation["element"] = element_record(state, index)?,
                None => {
                    validate_coordinate_gate(
                        true,
                        string_param(params, "snapshotId").as_deref(),
                        state,
                    )?;
                    operation["x"] = json!(number_param(params, "x").context("坐标点击缺少 x")?);
                    operation["y"] = json!(number_param(params, "y").context("坐标点击缺少 y")?);
                }
            }
            if let Some(button) = string_param(params, "button") {
                operation["mouse_button"] = json!(button);
            }
            if let Some(count) = params.get("clickCount").and_then(Value::as_i64) {
                operation["click_count"] = json!(count);
            }
            if let Some(modifiers) = string_param(params, "modifiers") {
                operation["modifiers"] = json!(modifiers);
            }
            operation["noScreenshot"] = json!(screenshot_wanted != Some(true));
        }
        "computer_type_text" => {
            operation["text"] = json!(string_param(params, "text").context("缺少 text 参数")?);
        }
        "computer_press_key" | "computer_hotkey" => {
            operation["key"] = json!(string_param(params, "key").context("缺少 key 参数")?);
        }
        "computer_paste_text" => {
            operation["text"] = json!(string_param(params, "text").context("缺少 text 参数")?);
        }
        "computer_scroll" => {
            operation["direction"] =
                json!(string_param(params, "direction").context("缺少 direction 参数")?);
            if let Some(pages) = number_param(params, "pages") {
                operation["pages"] = json!(pages);
            }
            let element = params.get("elementIndex").and_then(Value::as_i64);
            match element {
                Some(index) => operation["element"] = element_record(state, index)?,
                None => {
                    validate_coordinate_gate(
                        true,
                        string_param(params, "snapshotId").as_deref(),
                        state,
                    )?;
                    operation["x"] = json!(number_param(params, "x").context("坐标滚动缺少 x")?);
                    operation["y"] = json!(number_param(params, "y").context("坐标滚动缺少 y")?);
                }
            }
            operation["noScreenshot"] = json!(screenshot_wanted != Some(true));
        }
        "computer_drag" => {
            let from_element = params.get("fromElementIndex").and_then(Value::as_i64);
            let to_element = params.get("toElementIndex").and_then(Value::as_i64);
            match from_element {
                Some(index) => operation["fromElement"] = element_record(state, index)?,
                None => {
                    operation["from_x"] =
                        json!(number_param(params, "fromX").context("拖拽缺少 fromX")?);
                    operation["from_y"] =
                        json!(number_param(params, "fromY").context("拖拽缺少 fromY")?);
                }
            }
            match to_element {
                Some(index) => operation["toElement"] = element_record(state, index)?,
                None => {
                    operation["to_x"] = json!(number_param(params, "toX").context("拖拽缺少 toX")?);
                    operation["to_y"] = json!(number_param(params, "toY").context("拖拽缺少 toY")?);
                }
            }
            if from_element.is_none() || to_element.is_none() {
                validate_coordinate_gate(
                    true,
                    string_param(params, "snapshotId").as_deref(),
                    state,
                )?;
            }
            operation["noScreenshot"] = json!(screenshot_wanted != Some(true));
        }
        "computer_set_value" => {
            let index = params
                .get("elementIndex")
                .and_then(Value::as_i64)
                .context("缺少 elementIndex 参数")?;
            operation["element"] = element_record(state, index)?;
            operation["value"] = json!(string_param(params, "value").context("缺少 value 参数")?);
            operation["noScreenshot"] = json!(screenshot_wanted != Some(true));
        }
        "computer_perform_action" => {
            let index = params
                .get("elementIndex")
                .and_then(Value::as_i64)
                .context("缺少 elementIndex 参数")?;
            operation["element"] = element_record(state, index)?;
            operation["action"] =
                json!(string_param(params, "action").context("缺少 action 参数")?);
            operation["noScreenshot"] = json!(screenshot_wanted != Some(true));
        }
        _ => bail!("未知工具：{name}"),
    }
    if matches!(name, "computer_click" | "computer_scroll" | "computer_drag" | "computer_set_value" | "computer_perform_action") {
        bind_snapshot_target(&mut operation, state)?;
    }
    let app_query = operation.get("app").and_then(Value::as_str).map(str::to_owned);
    let response = runtime.call(operation)?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = response.get("error").and_then(Value::as_str).unwrap_or("未知错误");
        bail!("运行时错误：{error}");
    }
    if response.get("snapshot").is_some() { state.snapshot_app_query = app_query; }
    absorb_snapshot(state, &response);
    Ok(format_response(name, &response))
}

fn tool_field(name: &str) -> &'static str {
    match name {
        "computer_list_apps" => "list_apps",
        "computer_get_state" => "get_app_state",
        "computer_click" => "click",
        "computer_type_text" => "type_text",
        "computer_press_key" => "press_key",
        "computer_hotkey" => "hotkey",
        "computer_paste_text" => "paste_text",
        "computer_scroll" => "scroll",
        "computer_drag" => "drag",
        "computer_set_value" => "set_value",
        "computer_perform_action" => "perform_secondary_action",
        _ => "unsupported",
    }
}

/// 响应 → MCP content 块。截图块放最前（实测 text 在前时模型容易忽略图片）。
pub(crate) fn format_response(name: &str, response: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(snapshot) = response.get("snapshot") {
        if let Some(image) = snapshot_image_block(snapshot) {
            blocks.push(image);
        }
    }
    blocks.push(json!({"type": "text", "text": response_text(name, response)}));
    blocks
}

fn snapshot_image_block(snapshot: &Value) -> Option<Value> {
    let base64 = snapshot
        .get("screenshotPngBase64")
        .and_then(Value::as_str)
        .filter(|data| !data.is_empty())?;
    Some(json!({
        "type": "image",
        "data": base64,
        "mimeType": "image/png",
    }))
}

pub(crate) fn response_text(name: &str, response: &Value) -> String {
    if name == "computer_list_apps" {
        let apps = response
            .get("apps")
            .and_then(Value::as_array)
            .map(|apps| {
                apps.iter()
                    .filter_map(|app| {
                        let pid = app.get("pid").and_then(Value::as_u64)?;
                        let process_name = app.get("bundleIdentifier").and_then(Value::as_str)?;
                        let title = app.get("name").and_then(Value::as_str).unwrap_or("");
                        Some(format!("pid={pid} process={process_name} title={title}"))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut text = format!("共 {} 个应用：\n", apps.len());
        text.push_str(&apps.join("\n"));
        return text;
    }
    let mut parts = Vec::new();
    if name == "computer_list_windows" {
        let windows = response
            .pointer("/windows")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        parts.push(format!(
            "窗口：{}",
            serde_json::to_string(&windows).unwrap_or_default()
        ));
        return parts.join("\n");
    }
    if let Some(action) = response.get("action") {
        let path = action.get("path").and_then(Value::as_str).unwrap_or("?");
        let action_name = action.get("actionName").and_then(Value::as_str).unwrap_or("?");
        let mut line = format!("已执行 {name}（path={path}, action={action_name}");
        if let Some(reason) = action
            .pointer("/verification/reason")
            .and_then(Value::as_str)
        {
            line.push_str(&format!(", 未验证原因={reason}"));
        }
        if let Some(reason) = action.get("fallbackReason").and_then(Value::as_str) {
            line.push_str(&format!(", 兜底原因={reason}"));
        }
        line.push('）');
        parts.push(line);
    }
    if let Some(snapshot) = response.get("snapshot") {
        if let Some(id) = snapshot.get("snapshotId").and_then(Value::as_str) {
            parts.push(format!("snapshotId: {id}"));
        }
        if let Some(title) = snapshot.get("windowTitle").and_then(Value::as_str) {
            if !title.is_empty() {
                parts.push(format!("窗口：{title}"));
            }
        }
        if let Some(lines) = snapshot.get("treeLines").and_then(Value::as_array) {
            let rendered = lines
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            if !rendered.is_empty() {
                parts.push(format!("无障碍树（行首数字为元素索引）：\n{rendered}"));
            }
        }
        if let Some(truncated) = snapshot.pointer("/truncation/truncated").and_then(Value::as_bool) {
            if truncated {
                parts.push("（树已截断：元素过多或层级过深，可改用截图观察）".into());
            }
        }
        if snapshot.get("screenshotError").is_some() {
            parts.push("（截图失败或超限：可加 noScreenshot 或换更小的窗口）".into());
        } else if snapshot.get("screenshotPngBase64").and_then(Value::as_str).is_none() {
            parts.push("（本次未包含截图）".into());
        }
    }
    parts.join("\n")
}

// ---- stdio MCP 循环 ----

pub fn run_stdio(script: PathBuf) -> i32 {
    if !cfg!(target_os = "windows") {
        eprintln!("[computer-control] 仅支持 Windows");
        return 1;
    }
    if !script.is_file() {
        eprintln!("[computer-control] 运行时脚本不存在：{}", script.display());
        return 2;
    }
    let stdin = std::io::stdin();
    let mut runtime = RuntimeClient::new(script);
    let mut state = SessionState::default();
    for line in BufReader::new(stdin.lock()).lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(id) = frame.get("id").cloned() else {
            continue; // 通知帧无需响应
        };
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        let params = frame.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "computer-control",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tool_specs()
                    .into_iter()
                    .map(|spec| json!({
                        "name": spec.name,
                        "description": spec.description,
                        "inputSchema": spec.schema,
                    }))
                    .collect::<Vec<_>>(),
            })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                match dispatch_tool(&mut runtime, &mut state, name, &params.get("arguments").cloned().unwrap_or(Value::Null)) {
                    Ok(content) => Ok(json!({ "content": content })),
                    Err(error) => Ok(json!({
                        "content": [{ "type": "text", "text": format!("{error:#}") }],
                        "isError": true,
                    })),
                }
            }
            other => Err(json!({
                "code": -32601,
                "message": format!("未知方法：{other}"),
            })),
        };
        let response = match reply {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        if let Ok(encoded) = serde_json::to_string(&response) {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{encoded}");
            let _ = stdout.flush();
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn broken_stdin_retires_runtime_and_next_call_restarts_without_replaying() {
        let directory = std::env::temp_dir().join(format!("computer-restart-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("restart-fixture.ps1");
        std::fs::write(&script, r#"param([switch]$Serve)
$marker = Join-Path $PSScriptRoot 'started'
$first = -not (Test-Path -LiteralPath $marker)
[IO.File]::WriteAllText($marker, 'started')
[Console]::Out.WriteLine('{"ready":true}')
[Console]::Out.Flush()
if ($first) { exit 0 }
while ($null -ne ($line = [Console]::ReadLine())) {
  $request = $line | ConvertFrom-Json
  [Console]::Out.WriteLine((@{requestId=$request.requestId; tool=$request.tool} | ConvertTo-Json -Compress))
  [Console]::Out.Flush()
}
"#).unwrap();
        let mut runtime = RuntimeClient::new(script);
        runtime.start().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while runtime.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "fixture must exit before the request");
            std::thread::sleep(Duration::from_millis(10));
        }
        let error = runtime.call(json!({"tool":"not-replayed"})).unwrap_err();
        assert!(format!("{error:#}").contains("操作结果未知"));
        assert!(runtime.child.is_none());
        assert!(runtime.stdin.is_none());
        assert!(runtime.lines.is_none());
        let reply = runtime.call(json!({"tool":"observe-again"})).unwrap();
        assert_eq!(reply["tool"], "observe-again");
        assert_eq!(reply["requestId"], 2);
        runtime.stop();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_handshakes_reap_the_unpublished_child() {
        let directory = std::env::temp_dir().join(format!("computer-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("startup-fixture.ps1");
        std::fs::write(&script, r#"param([string]$Marker, [string]$Mode)
$held = [System.IO.File]::Open($Marker, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
if ($Mode -eq 'json') { [Console]::Out.WriteLine('not-json') }
if ($Mode -eq 'not-ready') { [Console]::Out.WriteLine('{"ready":false}') }
[Console]::Out.Flush()
Start-Sleep -Seconds 10
"#).unwrap();
        for mode in ["json", "not-ready", "timeout"] {
            let marker = directory.join(mode);
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-ExecutionPolicy", "RemoteSigned", "-File"])
                .arg(&script).arg(&marker).arg(mode)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            let mut runtime = RuntimeClient::new(PathBuf::from("unused.ps1"));
            assert!(runtime.start_command(command, Duration::from_secs(2)).is_err());
            assert!(runtime.child.is_none());
            assert!(marker.exists(), "fixture must have actually started");
            // The fixture keeps this file exclusive for its entire lifetime.
            // Opening it proves startup failure terminated and reaped that process.
            std::fs::OpenOptions::new().write(true).open(&marker).expect("failed-start child still running");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn state_with(id: &str) -> SessionState {
        SessionState {
            last_snapshot_id: Some(id.into()),
            ..SessionState::default()
        }
    }

    #[test]
    fn stderr_tail_handles_non_ascii_chunk_boundaries() {
        let bytes = "中文错误🛠️".repeat(2000).into_bytes();
        let tail = capture_stderr_tail(std::io::Cursor::new(bytes));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let value = tail.lock().expect("Unicode truncation must not poison stderr lock");
            if !value.is_empty() { assert!(value.len() <= 4096); }
            drop(value);
            if std::sync::Arc::strong_count(&tail) == 1 { break; }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!tail.lock().unwrap().is_empty());
    }

    #[test]
    fn cached_snapshot_target_cannot_be_reused_for_another_app_or_window() {
        let mut state = SessionState::default();
        absorb_snapshot(&mut state, &json!({"snapshot":{"snapshotId":"snap", "app":{"pid":123,"name":"notepad"},"windowId":456,"elements":[]}}));
        let mut operation = json!({"app":"notepad", "x":10,"y":10});
        bind_snapshot_target(&mut operation, &state).unwrap();
        assert_eq!(operation["app"], "123");
        assert_eq!(operation["windowId"], 456);
        assert!(bind_snapshot_target(&mut json!({"app":"calculator"}), &state).is_err());
        assert!(bind_snapshot_target(&mut json!({"app":"notepad", "windowId":789}), &state).is_err());
        absorb_snapshot(&mut state, &json!({"snapshot":{"snapshotId":"incomplete"}}));
        assert!(state.elements.is_empty());
        assert!(bind_snapshot_target(&mut operation, &state).is_err());
    }

    #[test]
    fn coordinate_gate_requires_fresh_snapshot_id() {
        let state = state_with("snap-1");
        assert!(validate_coordinate_gate(false, None, &state).is_ok());
        assert!(validate_coordinate_gate(true, None, &state).is_err());
        assert!(validate_coordinate_gate(true, Some("stale"), &state).is_err());
        assert!(validate_coordinate_gate(true, Some("snap-1"), &state).is_ok());
        let empty = SessionState::default();
        assert!(validate_coordinate_gate(true, Some("any"), &empty).is_err());
    }

    #[test]
    fn absorb_snapshot_updates_state_and_element_lookup() {
        let mut state = SessionState::default();
        absorb_snapshot(
            &mut state,
            &json!({
                "ok": true,
                "snapshot": {
                    "snapshotId": "snap-2",
                    "elements": [
                        { "index": 0, "runtimeId": [42] },
                        { "index": 7, "runtimeId": [7, 8] },
                    ]
                }
            }),
        );
        assert_eq!(state.last_snapshot_id.as_deref(), Some("snap-2"));
        let record = element_record(&state, 7).unwrap();
        assert_eq!(record["runtimeId"][1], 8);
        assert!(element_record(&state, 9).is_err());
    }

    #[test]
    fn tool_specs_cover_full_surface_with_schemas() {
        let specs = tool_specs();
        let names: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
        for expected in [
            "computer_list_apps",
            "computer_get_state",
            "computer_click",
            "computer_type_text",
            "computer_press_key",
            "computer_hotkey",
            "computer_paste_text",
            "computer_scroll",
            "computer_drag",
            "computer_set_value",
            "computer_perform_action",
        ] {
            assert!(names.contains(&expected), "缺少工具 {expected}");
        }
        for spec in &specs {
            assert_eq!(spec.schema["type"], "object");
            assert!(spec.schema.get("properties").is_some());
        }
        // 工具名与运行时 op 字段一一对应。
        assert_eq!(tool_field("computer_perform_action"), "perform_secondary_action");
        assert_eq!(tool_field("computer_get_state"), "get_app_state");
    }

    #[test]
    fn response_text_renders_tree_and_snapshot_id() {
        let text = response_text(
            "computer_click",
            &json!({
                "ok": true,
                "action": { "path": "accessibility", "actionName": "primaryAction" },
                "snapshot": {
                    "snapshotId": "snap-3",
                    "windowTitle": "记事本",
                    "treeLines": ["0 pane 无标题", "1 edit 编辑"],
                }
            }),
        );
        assert!(text.contains("snapshotId: snap-3"));
        assert!(text.contains("1 edit 编辑"));
        assert!(text.contains("已执行"));
        let apps = response_text(
            "computer_list_apps",
            &json!({ "ok": true, "apps": [{ "pid": 12, "bundleIdentifier": "notepad", "name": "记事本" }] }),
        );
        assert!(apps.contains("pid=12 process=notepad"));
    }

    #[test]
    fn format_response_places_image_before_text() {
        let blocks = format_response(
            "computer_get_state",
            &json!({
                "ok": true,
                "snapshot": {
                    "snapshotId": "s",
                    "screenshotPngBase64": "aGk=",
                    "treeLines": ["0 pane x"],
                }
            }),
        );
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["mimeType"], "image/png");
        assert_eq!(blocks[1]["type"], "text");
    }

    /// 真机集成：物化的运行时脚本 handshake + list_apps（零输入副作用）。
    #[test]
    #[cfg(target_os = "windows")]
    fn runtime_handshake_and_list_apps_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cc-computer-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("runtime.ps1");
        std::fs::write(
            &script,
            include_str!("../resources/computer-use-runtime.ps1"),
        )
        .unwrap();
        let mut runtime = RuntimeClient::new(script);
        let handshake = runtime.call(json!({ "tool": "handshake" })).unwrap();
        assert_eq!(
            handshake.pointer("/capabilities/provider").and_then(Value::as_str),
            Some("orca-computer-use-windows")
        );
        let apps = runtime.call(json!({ "tool": "list_apps" })).unwrap();
        assert!(apps.get("apps").and_then(Value::as_array).is_some_and(|list| !list.is_empty()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
