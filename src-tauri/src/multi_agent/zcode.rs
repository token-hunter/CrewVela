//! ZCode native v4 stdio client. The upstream Agent owns execution and persistence.
use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_FRAME: usize = 16 * 1024 * 1024;
#[path = "zcode_account.rs"]
mod account_host;
#[cfg(target_os = "macos")]
#[path = "zcode_macos.rs"]
mod macos;

fn public_workflow_policy(command: &Command) -> Result<Value> {
    let fallback =
        || json!({"enabled":false,"notice":"无法读取原生工作流公开配置，创建入口暂不可用"});
    let builtin = match command
        .get_envs()
        .find(|(key, _)| *key == "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE")
    {
        Some((_, Some(value))) => Some(value.to_owned()),
        Some((_, None)) => None,
        None => std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE"),
    };
    let Some(builtin) = builtin else {
        return Ok(fallback());
    };
    let Some(program) = node_helper_program(command) else {
        return Ok(fallback());
    };
    let mut helper = Command::new(program);
    helper.args([
        "-e",
        include_str!("../../resources/zcode-workflow-policy.cjs"),
    ]);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                helper.env(key, value);
            }
            None => {
                helper.env_remove(key);
            }
        }
    }
    helper
        .env("CREWVELA_ZCODE_WORKFLOW_HELPER", "1")
        .env_remove("NODE_OPTIONS");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        helper.creation_flags(0x0800_0000);
    }
    let result = (|| -> Result<Value> {
        let input = serde_json::to_vec(&json!({"builtin":PathBuf::from(&builtin)}))?;
        let output = crate::title_summary::run_cli_with_timeout(helper, Some(input), Duration::from_secs(20), 16384, "工作流公开配置")?;
        if !output.status.success() { bail!("工作流配置读取失败"); }
        let result: Value = serde_json::from_slice(&output.stdout)?;
        if !result["enabled"].is_boolean() {
            bail!("工作流配置格式不兼容");
        }
        Ok(result)
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) if error.downcast_ref::<crate::acp::UnverifiedStop>().is_some() => Err(error),
        Err(_) => Ok(fallback()),
    }
}

#[derive(Debug)]
pub(crate) struct CommandRejected {
    message: String,
    admission_denied: bool,
}
impl CommandRejected {
    fn local(message: &str) -> Self {
        Self { message: message.to_owned(), admission_denied: false }
    }
}
impl std::fmt::Display for CommandRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CommandRejected {}

#[derive(Debug)]
pub(crate) struct RequestRejected {
    method: String,
    code: Option<i64>,
    resume_lineage_invalid: bool,
}
impl std::fmt::Display for RequestRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ZCode Desktop {} 失败（{}）",
            self.method,
            self.code
                .map_or_else(|| "null".into(), |code| code.to_string())
        )
    }
}
impl std::error::Error for RequestRejected {}

fn sync_workflow_policy(
    client: &mut Client<'_>,
    workspace: &Value,
    enabled: bool,
    stopped: &AtomicBool,
    permission: &mut PermissionHandler<'_>,
) -> Result<bool> {
    match client.request(
        "workspace/updateDynamicWorkflowPolicy",
        json!({"workspace":workspace,"enabled":enabled}),
        stopped,
        permission,
    ) {
        Ok(value) if value["enabled"] == enabled => return Ok(enabled),
        Err(error)
            if error
                .downcast_ref::<RequestRejected>()
                .is_some_and(|failure| failure.code == Some(-32601)) =>
        {
            return Ok(false)
        }
        _ => {}
    }
    // A lost/malformed ACK may follow a successfully applied true policy. The
    // process-owned flag must be confirmed false before a session can be made.
    let reset = client
        .request(
            "workspace/updateDynamicWorkflowPolicy",
            json!({"workspace":workspace,"enabled":false}),
            stopped,
            permission,
        )
        .context("无法确认 ZCode Desktop 工作流安全关闭，连接已停止")?;
    if reset["enabled"] != false {
        bail!("无法确认 ZCode Desktop 工作流安全关闭，连接已停止");
    }
    Ok(false)
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct ModelChoice {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub selection: Value,
}

fn model_choices(settings: &Value) -> Vec<ModelChoice> {
    let mut choices = Vec::new();
    for model in settings
        .pointer("/model/available")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(provider) = model.pointer("/ref/providerId").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = model.pointer("/ref/modelId").and_then(Value::as_str) else {
            continue;
        };
        // Account provider identities are reserved by upstream Built-in Config.
        if provider.is_empty()
            || id.is_empty()
            || model
                .get("disabledReason")
                .is_some_and(|reason| !reason.is_null())
        {
            continue;
        }
        let label = model.get("label").and_then(Value::as_str).unwrap_or(id);
        let provider_label = model
            .get("providerLabel")
            .and_then(Value::as_str)
            .unwrap_or(provider);
        let levels = model.pointer("/reasoning/levels").and_then(Value::as_array);
        let selections: Vec<(Value, String)> = match levels.filter(|levels| !levels.is_empty()) {
            Some(levels) => levels.iter().filter_map(|level| level.get("value").and_then(Value::as_str)).map(|level| (json!({"providerId":provider,"modelId":id,"options":{"reasoningLevel":level}}),format!("{provider_label} · {label} · {level}"))).collect(),
            None => vec![(json!({"providerId":provider,"modelId":id}),format!("{provider_label} · {label}"))],
        };
        for (selection, name) in selections {
            let id = selection.to_string();
            if !choices.iter().any(|choice: &ModelChoice| choice.id == id) {
                choices.push(ModelChoice {
                    id,
                    name,
                    selection,
                });
            }
        }
    }
    choices
}

#[cfg(target_os = "macos")]
pub(crate) fn executable() -> Result<PathBuf> {
    macos::executable()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CREWVELA_ZCODE_EXECUTABLE") {
        let path = PathBuf::from(path);
        if !path.is_absolute() || !path.is_file() {
            bail!("CREWVELA_ZCODE_EXECUTABLE 必须指向现有的绝对 Agent 可执行文件或预构建 cjs 文件");
        }
        return Ok(path);
    }
    if let Some(root) = std::env::var_os("CREWVELA_ZCODE_ROOT") {
        let root = PathBuf::from(root);
        let desktop = root.join("resources/glm/zcode.cjs");
        let path = if desktop.is_file() {
            desktop
        } else {
            root.join("apps/zcode-cli/packages/cli/dist/zcode.cjs")
        };
        if !root.is_absolute() || !path.is_file() {
            bail!("CREWVELA_ZCODE_ROOT 必须指向桌面安装目录或已构建源码目录 apps/zcode-cli/packages/cli/dist/zcode.cjs");
        }
        return Ok(path);
    }
    #[cfg(windows)]
    if let Some(path) = installed_desktop_agent() {
        return Ok(path);
    }
    // Explorer-launched desktop apps can inherit a different PATH than a
    // development shell. A source-build/npm shim lacks the desktop Built-in
    // Config path required by AccountHost, so prefer the installed desktop
    // Agent unless the user explicitly selected an executable/root above.
    if let Ok(path) = which::which("zcode") {
        return Ok(path);
    }
    bail!("未找到 ZCode Desktop Agent；请安装 ZCode Desktop，或指定 CREWVELA_ZCODE_EXECUTABLE / CREWVELA_ZCODE_ROOT")
}

fn cli_override() -> Result<Option<PathBuf>> {
    use crate::runtime_installation::zcode_cli::{stored_executable, OVERRIDE_ENV};
    if let Some(path) = std::env::var_os(OVERRIDE_ENV) {
        let path = PathBuf::from(path);
        if !path.is_absolute() || !path.is_file() {
            bail!("CREWVELA_ZCODE_CLI_EXECUTABLE 必须指向现有的绝对 CLI 可执行文件或预构建 cjs 文件");
        }
        return Ok(Some(path));
    }
    let selected = stored_executable()?;
    if selected.as_ref().is_some_and(|path| !path.is_absolute() || !path.is_file()) {
        bail!("手动选择的 ZCode CLI 已不可用，请重新选择或恢复自动检测");
    }
    Ok(selected)
}

/// zcode CLI runtime：显式 env → CrewVela 选定路径 → PATH 自动检测。
/// 与 ZCode Desktop 是两个独立 Runtime，不共享解析顺序。
pub(crate) fn cli_executable() -> Result<PathBuf> {
    if let Some(path) = cli_override()? { return Ok(path); }
    if let Ok(path) = which::which("zcode") {
        if let Some(resolved) = resolve_path_cli(&path) {
            return Ok(resolved);
        }
        // 无扩展名 sh 脚本在 Windows 不能直接 spawn，优先同目录可执行的
        // cmd/exe shim；仍不可用时保留原样，由 spawn 报出明确错误。
        #[cfg(windows)]
        if path.extension().is_none() {
            let sibling = ["zcode.cmd", "zcode.exe", "zcode.bat"]
                .into_iter()
                .filter_map(|name| path.parent().map(|parent| parent.join(name)))
                .find(|candidate| candidate.is_file());
            if let Some(sibling) = sibling {
                return Ok(sibling);
            }
        }
        return Ok(path);
    }
    bail!("未找到 zcode CLI；请在 PATH 上安装 zcode CLI，或指定 CREWVELA_ZCODE_CLI_EXECUTABLE")
}

/// 两个 zcode runtime 的 Agent 入口：Desktop 走桌面安装，CLI 走本地命令。
pub(crate) fn runtime_executable(kind: super::domain::AgentRuntimeKind) -> Result<PathBuf> {
    match kind {
        super::domain::AgentRuntimeKind::ZCodeCli => cli_executable(),
        _ => executable(),
    }
}

/// zcode CLI 独立终端会话：保留用户原始 shim 的控制台修复；直连
/// cjs 时在 Windows 预置 UTF-8 代码页，再由 Node 正常加载入口。
pub(crate) fn cli_tui_command() -> Result<Command> {
    let mut command = cli_tui_command_inner()?;
    configure_tui_compatibility(
        &mut command,
        std::env::var("__COMPAT_LAYER").ok().as_deref(),
    );
    Ok(command)
}

// Windows 的应用健康检测兼容层会被 GUI 子进程继承，导致 OpenTUI 原生渲染器
// 以 0xC0000005 崩溃。仅过滤已复现的问题层，保留用户显式设置的其他兼容层。
fn configure_tui_compatibility(command: &mut Command, inherited: Option<&str>) {
    let Some(inherited) = inherited else {
        return;
    };
    let layers: Vec<_> = inherited.split_whitespace().collect();
    if !layers
        .iter()
        .any(|layer| layer.eq_ignore_ascii_case("DetectorsAppHealth"))
    {
        return;
    }
    let remaining = layers
        .into_iter()
        .filter(|layer| !layer.eq_ignore_ascii_case("DetectorsAppHealth"))
        .collect::<Vec<_>>()
        .join(" ");
    if remaining.is_empty() {
        command.env_remove("__COMPAT_LAYER");
    } else {
        command.env("__COMPAT_LAYER", remaining);
    }
}

fn cli_tui_command_inner() -> Result<Command> {
    if let Some(path) = cli_override()? {
        return if is_script_entry(&path) { cli_script_tui_command(&path) } else { Ok(super::runtime::command_for_path(&path)) };
    }
    if let Ok(path) = which::which("zcode") {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("exe" | "cmd" | "bat") => return Ok(super::runtime::command_for_path(&path)),
            // 无扩展名 sh 脚本不能直接 spawn，优先同目录可执行 shim。
            #[cfg(windows)]
            None => {
                let sibling = ["zcode.cmd", "zcode.exe", "zcode.bat"]
                    .into_iter()
                    .filter_map(|name| path.parent().map(|parent| parent.join(name)))
                    .find(|candidate| candidate.is_file());
                if let Some(sibling) = sibling {
                    return Ok(super::runtime::command_for_path(&sibling));
                }
            }
            #[cfg(not(windows))]
            None => return Ok(Command::new(&path)),
            _ => {}
        }
    }
    let path = cli_executable()?;
    if is_script_entry(&path) {
        return cli_script_tui_command(&path);
    }
    Ok(super::runtime::command_for_path(&path))
}

/// Keep the entry path as a literal argv item, never shell source. ZCode's
/// current Node runtime supports --import; a fixed preloader sets the shared
/// Windows console code page before OpenTUI is loaded, preserving normal main
/// module/argv semantics and avoiding cmd.exe on every platform.
fn cli_script_tui_command(entry: &Path) -> Result<Command> {
    let node = which::which("node").context("启动 zcode CLI 需要 Node.js")?;
    let mut command = Command::new(node);
    #[cfg(windows)] {
        const UTF8_CONSOLE_PRELOAD: &str = "import{spawnSync}from'node:child_process';const r=spawnSync('chcp.com',['65001'],{stdio:['inherit','ignore','inherit'],windowsHide:true});if((r.error||r.status!==0)&&(process.env.CREWVELA_SESSION_ID||process.stdout.isTTY||process.stdin.isTTY))throw new Error('ZCode CLI UTF-8 console initialization failed');";
        let preload = format!("data:text/javascript;base64,{}", base64::engine::general_purpose::STANDARD.encode(UTF8_CONSOLE_PRELOAD));
        command.arg("--import").arg(preload);
    }
    command.arg(entry);
    Ok(command)
}

/// PATH 上的 zcode 可能是可执行文件、直连 cjs 入口，或指向 node+cjs 的
/// cmd/sh shim。只有解析出真实 cjs 的 shim 才具备与桌面 Agent 相同的
/// builtin/账号能力；exe 若是桌面主程序则回到自带 Agent。
fn resolve_path_cli(path: &Path) -> Option<PathBuf> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("exe") => {
            #[cfg(windows)]
            if let Some(agent) = path.parent().and_then(desktop_agent) {
                return Some(agent);
            }
            Some(path.to_path_buf())
        }
        Some("cjs" | "js") => Some(path.to_path_buf()),
        _ => resolve_shim_entry(path),
    }
}

/// 从 cmd/sh shim 文本中提取真实 js/cjs 入口；带引号的目标优先于裸 token。
fn resolve_shim_entry(shim: &Path) -> Option<PathBuf> {
    let content = std::fs::read(shim).ok()?;
    let text = String::from_utf8_lossy(&content);
    let directory = shim.parent()?;
    for entry in shim_entry_candidates(&text) {
        let entry = expand_shim_path(&entry, directory)?;
        if entry.is_absolute() && entry.is_file() {
            return Some(entry);
        }
    }
    None
}

fn shim_entry_candidates(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut quoted = Vec::new();
    let mut bare = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let mut quoted_token = false;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
            if bytes[index] == b'"' {
                quoted_token = !quoted_token;
            }
            index += 1;
        }
        let token = text[start..index].trim_matches('"');
        if [".cjs", ".js"].into_iter().any(|suffix| {
            token.len() > suffix.len() && token.to_ascii_lowercase().ends_with(suffix)
        }) {
            if quoted_token {
                quoted.push(token.to_owned());
            } else {
                bare.push(token.to_owned());
            }
        }
        index += 1;
    }
    quoted.into_iter().chain(bare).collect()
}

/// 展开 shim 特有路径写法：`%~dp0`（cmd 脚本目录）与 MSYS 盘符路径 `/c/...`。
fn expand_shim_path(token: &str, shim_directory: &Path) -> Option<PathBuf> {
    let token = token.trim();
    if let Some(rest) = token.strip_prefix("%~dp0") {
        let rest = rest.trim_start_matches(['\\', '/']);
        #[cfg(windows)]
        return Some(shim_directory.join(rest.replace('/', "\\")));
        #[cfg(not(windows))]
        return Some(shim_directory.join(rest.replace('\\', "/")));
    }
    #[cfg(windows)]
    {
        let bytes = token.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            let drive = bytes[1].to_ascii_uppercase() as char;
            return Some(PathBuf::from(format!("{drive}:{}", &token[2..])));
        }
    }
    Some(PathBuf::from(token))
}

#[cfg(windows)]
fn desktop_agent(root: &Path) -> Option<PathBuf> {
    let agent = root.join("resources/glm/zcode.cjs");
    (root.is_absolute() && root.join("ZCode.exe").is_file() && agent.is_file()).then_some(agent)
}

#[cfg(windows)]
fn registered_file(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let path = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        // DisplayIcon may contain a trailing icon index.
        value.rsplit_once(',').map_or(value, |(path, index)| {
            if index.trim().parse::<i32>().is_ok() {
                path
            } else {
                value
            }
        })
    };
    let path = PathBuf::from(path.trim());
    path.is_absolute().then_some(path)
}

#[cfg(windows)]
fn installed_desktop_agent() -> Option<PathBuf> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    // NSIS installs can use a custom directory and leave InstallLocation empty.
    for hive in [CURRENT_USER, LOCAL_MACHINE] {
        for registry_path in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(uninstall) = hive.open(registry_path) else {
                continue;
            };
            let Ok(names) = uninstall.keys() else {
                continue;
            };
            for name in names {
                let Ok(entry) = uninstall.open(name) else {
                    continue;
                };
                let Ok(display_name) = entry.get_string("DisplayName") else {
                    continue;
                };
                if !display_name.eq_ignore_ascii_case("ZCode")
                    && !display_name.to_ascii_lowercase().starts_with("zcode ")
                {
                    continue;
                }
                if let Ok(root) = entry.get_string("InstallLocation") {
                    if let Some(agent) = desktop_agent(Path::new(&root)) {
                        return Some(agent);
                    }
                }
                for field in ["DisplayIcon", "UninstallString"] {
                    if let Some(agent) = entry
                        .get_string(field)
                        .ok()
                        .and_then(|value| registered_file(&value))
                        .and_then(|file| file.parent().and_then(desktop_agent))
                    {
                        return Some(agent);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn launch(path: &Path) -> Result<Command> {
    let mut command = command_for_entry(path)?;
    command.args(["app-server", "--stdio"]);
    Ok(command)
}

pub(crate) fn command_for_entry(path: &Path) -> Result<Command> {
    #[cfg(target_os = "macos")]
    let mac_bundle = macos::bundle_root(path)
        .filter(|root| path == *root || path == root.join("Contents/Resources/glm/zcode.cjs"))
        .map(macos::bundle).transpose()?;
    #[cfg(target_os = "macos")]
    let path = mac_bundle.as_ref().map_or(path, |app| app.agent.as_path());
    let desktop_root = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|root| root.join("ZCode.exe").is_file());
    let desktop_program = desktop_root.map(|root| root.join("ZCode.exe"));
    #[cfg(target_os = "macos")]
    let desktop_program = mac_bundle.as_ref().map(|app| app.program.clone()).or(desktop_program);
    let mut command = if path
        .extension()
        .is_some_and(|ext| ext == "cjs" || ext == "js")
    {
        let mut command = Command::new(if let Some(program) = desktop_program.as_ref() {
            program.clone()
        } else {
            crate::runtime_dependencies::node_executable().context("启动 ZCode Desktop cjs Agent 需要 Node.js")?
        });
        if desktop_program.is_some() {
            command
                .env("ELECTRON_RUN_AS_NODE", "1")
                .env_remove("NODE_OPTIONS");
        }
        command.arg(path);
        command
    } else {
        super::runtime::command_for_path(path)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    if std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").is_none() {
        let desktop_config = desktop_root.map(|root| root.join("resources/config/provider/zcode-builtin.json"));
        #[cfg(target_os = "macos")]
        let desktop_config = mac_bundle.as_ref().map(|app| app.builtin.clone()).or(desktop_config);
        let config = desktop_config
            .or_else(|| {
                std::env::var_os("CREWVELA_ZCODE_ROOT")
                    .map(|root| PathBuf::from(root).join("config/provider/zcode-builtin.json"))
            })
            .or_else(|| cli_bundled_builtin(path))
            .or_else(cli_cached_builtin);
        if let Some(config) = config.filter(|config| config.is_file()) {
            command.env("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE", config);
        }
    }
    // The native CLI rewrites a lone bundled path to its active cache path.
    // Supply both native sources so the Account Host and Agent share exactly
    // the same builtin revision instead of silently discarding the overlay.
    if !std::env::var("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE")
        .is_ok_and(|value| !value.trim().is_empty())
        && (command
            .get_envs()
            .any(|(key, _)| key == "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE")
            || std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").is_some())
    {
        let base = zcode_data_base_dir().context("无法定位 ZCode Desktop 本地模型配置目录")?;
        command.env(
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
            base.join(".zcode/v2/provider_config.json"),
        );
    }
    if std::env::var_os("CREWVELA_ZCODE_APP_VERSION").is_none() {
        let version = cli_entry_version(path);
        #[cfg(target_os = "macos")]
        let version = mac_bundle.as_ref().and_then(|app| app.version.clone()).or(version);
        if let Some(version) = version {
            command.env("CREWVELA_ZCODE_APP_VERSION", version);
        }
    }
    Ok(command)
}

/// 账号/工作流辅助脚本用 `-e` 执行，需要一个 node 运行时。优先复用 Agent
/// 命令自身的程序（node，或 ELECTRON_RUN_AS_NODE 下的桌面壳）；shim/exe
/// 场景回退 PATH 上的 node——能跑 cjs CLI 的机器必然装有 node。
fn node_helper_program(command: &Command) -> Option<PathBuf> {
    let program = PathBuf::from(command.get_program());
    if command
        .get_envs()
        .any(|(key, value)| key == "ELECTRON_RUN_AS_NODE" && value.is_some())
    {
        return Some(program);
    }
    let name = program
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if name.contains("node") {
        return Some(program);
    }
    which::which("node").ok()
}

fn zcode_data_base_dir() -> Option<PathBuf> {
    ["ZCODE_DATA_BASE_DIR", "HOME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

fn is_script_entry(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "cjs" || extension == "js")
}

/// 镜像 CLI 自身的 bundled 定位（provider-runtime-env.ts 的候选顺序）：入口
/// 同目录的 provider/zcode-builtin.json，或源码仓库根的 config/provider/
/// zcode-builtin.json（dist 向上 5 级）。Agent 与账号 Host 共享同一文件才共享 revision。
fn cli_bundled_builtin(entry: &Path) -> Option<PathBuf> {
    if !is_script_entry(entry) {
        return None;
    }
    let directory = entry.parent()?;
    // 与 node 的 resolve(dir, "../../../../../config/...") 等价：向上 5 级。
    let repo_root = directory.ancestors().nth(5)?;
    [
        directory.join("provider/zcode-builtin.json"),
        repo_root.join("config/provider/zcode-builtin.json"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// CLI 首次运行后物化的 active builtin cache；bundled 布局缺失时的兜底。
fn cli_cached_builtin() -> Option<PathBuf> {
    zcode_data_base_dir()
        .map(|base| base.join(".zcode/v2/runtime/provider/bundled/zcode-builtin.json"))
        .filter(|candidate| candidate.is_file())
}

/// start-plan 权益查询要求 app_version：桌面布局由 helper 从 app.asar 自行
/// 探测，CLI 入口则从相邻 package.json 读取（dist/../package.json）。
fn cli_entry_version(entry: &Path) -> Option<String> {
    if !is_script_entry(entry) {
        return None;
    }
    let package = entry.parent()?.parent()?.join("package.json");
    let content = std::fs::read(package).ok()?;
    let value: Value = serde_json::from_slice(&content).ok()?;
    value
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
}

pub(super) fn model_selection(model: Option<&str>) -> Result<Option<Value>> {
    model
        .map(|model| {
            let (provider, model) = model
                .trim()
                .split_once('/')
                .context("ZCode Desktop 模型必须使用 provider/model，留空使用 ZCode Desktop 本地默认模型")?;
            if provider.is_empty() || model.is_empty() {
                bail!("ZCode Desktop 模型必须使用非空 provider/model");
            }
            Ok(json!({"providerId": provider, "modelId": model}))
        })
        .transpose()
}

pub(super) struct ZCodeResult {
    pub text: String,
    pub session_id: String,
}

const MANAGED_REVIEW_TOOLS: &[&str] = &["Read", "Glob", "Grep"];

pub(super) struct ManagedTurn {
    pub text: String,
    pub session_id: String,
    pub command_id: String,
    pub turn_id: String,
}

type ManagedCheckpoint<'a> = dyn FnMut(&str, &str, &str, Option<&str>) -> Result<()> + 'a;

pub(super) fn execute_managed_turn(
    cwd: &Path,
    prompt: &str,
    resume: Option<&str>,
    model: Option<&str>,
    text_only: bool,
    stopped: &AtomicBool,
    checkpoint: &mut ManagedCheckpoint<'_>,
) -> Result<ManagedTurn> {
    let command = launch(&runtime_executable(super::AgentRuntimeKind::ZCodeCli)?)?;
    managed_turn_command(command, cwd, prompt, resume, model, text_only, stopped, checkpoint)
}

pub(crate) fn managed_cli_preflight(cwd: &Path) -> Result<()> {
    let command = launch(&cli_executable()?)?;
    check_managed_cli_protocol(command, cwd)
}

fn check_managed_cli_protocol(command: Command, cwd: &Path) -> Result<()> {
    let stopped = AtomicBool::new(false);
    let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
    let mut progress = || Ok(());
    let mut client = Client::spawn_with_account_host(command, cwd, &mut progress, None, "ZCode CLI")?;
    let checked = client.check_v4(&stopped, &mut deny, Duration::from_secs(8));
    match (checked, client.cleanup_checked()) {
        (Ok(()), cleanup) => cleanup,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(error.context(format!("清理 ZCode CLI 预检进程失败：{cleanup:#}"))),
    }
}

#[cfg(test)]
fn execute_managed_review_command(command: Command, cwd: &Path, prompt: &str, stopped: &AtomicBool) -> Result<ZCodeResult> {
    let turn = managed_turn_command(command, cwd, prompt, None, None, false, stopped, &mut |_, _, _, _| Ok(()))?;
    Ok(ZCodeResult { text: turn.text, session_id: turn.session_id })
}

fn managed_turn_command(
    command: Command,
    cwd: &Path,
    prompt: &str,
    resume: Option<&str>,
    model: Option<&str>,
    text_only: bool,
    stopped: &AtomicBool,
    checkpoint: &mut ManagedCheckpoint<'_>,
) -> Result<ManagedTurn> {
    let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
    let mut progress = || Ok(());
    let tools = if text_only { &[][..] } else { MANAGED_REVIEW_TOOLS };
    let mut connection = DesktopConnection::open_with_policy(
        command, cwd, resume, false, Vec::new(), stopped, &mut deny, &mut progress, None, Some(tools),
    )?;
    let result = (|| {
        if let Some(selection) = model_selection(model)? {
            let choice = connection.models.iter().find(|item| item.selection == selection)
                .context("ZCode CLI 本地目录中没有任务指定的模型")?.id.clone();
            if connection.selected_model_id.as_ref() != Some(&choice) {
                connection.set_model(&choice, stopped, &mut deny)?;
            }
        } else if connection.selected_model_id.is_none() {
            let model = connection.models.first().with_context(|| {
                connection.client.account_host.as_ref().and_then(|host| host.unavailable_notice())
                    .unwrap_or_else(|| "ZCode CLI 没有可用的本地审查模型".into())
            })?.id.clone();
            connection.set_model(&model, stopped, &mut deny)?;
        }
        // Plan is a per-turn flag in the v4 protocol. Do not wait for a
        // settings revision when the session was already created in Plan.
        let session_id = connection.session_id.clone();
        let command_id = uuid::Uuid::new_v4().to_string();
        checkpoint("dispatched", &session_id, &command_id, None)?;
        let submission = connection.client.command_id(
            "sendText", Some(&session_id),
            json!({"text":prompt,"attachments":[],"requestedDelivery":"startNow","mode":"build","planEnabled":true}),
            &command_id, None, stopped, &mut deny,
        );
        let (_, ack) = match submission {
            Ok(result) => result,
            Err(error) => {
                if error.downcast_ref::<CommandRejected>().is_some_and(|reason| reason.admission_denied) {
                    checkpoint("rejected", &session_id, &command_id, None)?;
                }
                return Err(error);
            }
        };
        ensure!(ack["status"] == "accepted", "ZCode CLI 未确认接收本轮新输入，请核对原生历史");
        checkpoint("accepted", &session_id, &command_id, None)?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut auth_error = None;
        loop {
            if Instant::now() >= deadline {
                if let Some(reason) = auth_error { bail!("ZCode CLI 托管认证失败，未收到原生轮次终态：{reason}"); }
                bail!("ZCode CLI 托管轮次超时");
            }
            if let Err(error) = connection.poll(stopped, &mut deny) {
                if let Some(reason) = auth_error { return Err(error).context(format!("ZCode CLI 托管认证失败：{reason}")); }
                return Err(error);
            }
            if let Some(reason) = connection.take_auth_error() {
                auth_error = Some(reason);
            }
            let Some(header) = connection.projection.turn_header(&command_id) else { continue; };
            let phase = match header["state"].as_str() {
                Some("running") => continue,
                Some("completedSuccess") => "completed",
                Some("failed") => "failed",
                Some("completedInterrupted") => "interrupted",
                _ => bail!("ZCode CLI 当前轮次返回未知状态"),
            };
            let turn_id = header["turnId"].as_str().filter(|id| !id.is_empty())
                .context("ZCode CLI 轮次缺少原生 turnId")?.to_owned();
            checkpoint(phase, &session_id, &command_id, Some(&turn_id))?;
            if let Some(reason) = auth_error {
                bail!("ZCode CLI 托管轮次认证失败：{reason}");
            }
            ensure!(phase == "completed", "ZCode CLI 原生轮次 {turn_id} 已结束（{phase}）");
            // Tool use can create intermediate assistant messages. A structured delivery
            // belongs to the final assistant message in this turn, not its earlier commentary.
            let final_row = connection.projection.rows.values().rev()
                .find(|row| row["kind"] == "assistantText" && row["turnId"] == turn_id)
                .context("ZCode CLI 成功轮次缺少最终正文")?;
            ensure!(final_row["state"] == "complete", "ZCode CLI 轮次已结束，但最终回复仍未完成");
            let text = final_row["text"].as_str().filter(|text| !text.trim().is_empty())
                .context("ZCode CLI 成功轮次缺少最终正文")?.to_owned();
            return Ok(ManagedTurn { text, session_id, command_id, turn_id });
        }
    })();
    connection.shutdown()?;
    result
}
pub(super) type PermissionHandler<'a> = dyn FnMut(&Value, &AtomicBool) -> Result<bool> + 'a;

struct Client<'a> {
    child: Child,
    _identity: crate::acp::ProcessIdentityGuard,
    tracked_process_id: Option<&'a std::sync::atomic::AtomicU32>,
    #[cfg(test)]
    cleanup_after_stop: Option<Box<dyn FnOnce()>>,
    stdin: ChildStdin,
    receiver: mpsc::Receiver<Result<Value>>,
    startup_diagnostic: mpsc::Receiver<Option<&'static str>>,
    runtime_name: &'static str,
    queued: VecDeque<Value>,
    next_id: u64,
    session: Option<String>,
    /// 辅助对话子会话标识：其轮次会以自己的 sessionId 发起反向请求（鉴权/审批），同样合法。
    side_sessions: Vec<String>,
    on_progress: &'a mut dyn FnMut() -> Result<()>,
    progress_at: Instant,
    permissions: BTreeMap<String, (Value, Value)>,
    account_host: Option<account_host::AccountHost>,
    selected_model: Value,
    auth_error: Option<String>,
}

#[derive(Debug)]
pub(super) struct ProtocolClosed(String);

impl std::fmt::Display for ProtocolClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolClosed {}

fn startup_stderr_diagnostic(bytes: &[u8]) -> Option<&'static str> {
    let line = String::from_utf8_lossy(bytes);
    if line.contains("MODULE_NOT_FOUND") || line.contains("ERR_MODULE_NOT_FOUND") {
        Some("Node.js 找不到 CLI 入口或依赖，请检查「安装与版本」中的启动路径")
    } else if line.contains("ERR_DLOPEN_FAILED") {
        Some("Node.js 无法加载原生依赖，请检查 CLI 与 Node.js 版本")
    } else {
        None
    }
}
impl Drop for Client<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup_checked() {
            eprintln!("[crewvela] ZCode Drop 未能确认进程停止：{error:#}");
        }
    }
}
impl<'a> Client<'a> {
    fn spawn(
        command: Command,
        cwd: &Path,
        on_progress: &'a mut dyn FnMut() -> Result<()>,
    ) -> Result<Self> {
        let account_host = account_host::AccountHost::prepare(&command)?;
        Self::spawn_with_account_host(command, cwd, on_progress, account_host, "ZCode Desktop Agent")
    }

    fn spawn_with_account_host(
        mut command: Command,
        cwd: &Path,
        on_progress: &'a mut dyn FnMut() -> Result<()>,
        account_host: Option<account_host::AccountHost>,
        runtime_name: &'static str,
    ) -> Result<Self> {
        command.current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let (mut child, identity) = crate::acp::spawn_owned_command(&mut command)
            .with_context(|| format!("启动 {runtime_name} 失败"))?;
        let pipes = (|| -> Result<_> {
            Ok((child.stdin.take().context("ZCode Desktop 缺少 stdin")?, child.stdout.take().context("ZCode Desktop 缺少 stdout")?))
        })();
        let (stdin, stdout) = match pipes {
            Ok(pipes) => pipes,
            Err(error) => {
                crate::acp::stop_child_checked(&mut child)?;
                return Err(error);
            }
        };
        // Drain stderr and keep only fixed diagnostics. Raw CLI output can contain credentials.
        let (diagnostic_sender, startup_diagnostic) = mpsc::channel();
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut sink = Vec::new();
                let mut diagnostic = None;
                loop {
                    sink.clear();
                    if (&mut reader).take(4096).read_until(b'\n', &mut sink).unwrap_or(0) == 0 {
                        break;
                    }
                    diagnostic = diagnostic.or_else(|| startup_stderr_diagnostic(&sink));
                }
                let _ = diagnostic_sender.send(diagnostic);
            });
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let count = (&mut reader)
                    .take((MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut bytes);
                let result = match count {
                    Ok(0) => break,
                    Ok(_) if bytes.len() > MAX_FRAME => Err(anyhow!("ZCode Desktop 协议行超出安全上限")),
                    Ok(_) => serde_json::from_slice(&bytes).context("ZCode Desktop 输出不是有效 NDJSON"),
                    Err(error) => Err(error.into()),
                };
                let failed = result.is_err();
                if sender.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            _identity: identity,
            tracked_process_id: None,
            #[cfg(test)]
            cleanup_after_stop: None,
            stdin,
            receiver,
            startup_diagnostic,
            runtime_name,
            queued: VecDeque::new(),
            next_id: 1,
            session: None,
            on_progress,
            progress_at: Instant::now(),
            permissions: BTreeMap::new(),
            side_sessions: Vec::new(),
            account_host,
            selected_model: Value::Null,
            auth_error: None,
        })
    }
    fn send(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.stdin, value)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }
    fn protocol_closed(&mut self) -> anyhow::Error {
        let diagnostic = self.startup_diagnostic.recv_timeout(Duration::from_millis(200)).ok().flatten();
        // On Windows the pipe can report EOF before the process handle is signalled.
        let deadline = Instant::now() + Duration::from_millis(400);
        let status = loop {
            match self.child.try_wait() {
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                state => break state.ok().flatten(),
            }
        };
        let exit = status.map_or_else(
            || "退出状态未取得".to_owned(),
            |status| format!("退出状态 {status}"),
        );
        let detail = diagnostic.map_or_else(String::new, |line| format!("，{line}"));
        ProtocolClosed(format!("{} 协议连接已关闭（{exit}）{detail}", self.runtime_name)).into()
    }

    fn receive(&mut self, deadline: Instant, cancelled: &AtomicBool) -> Result<Value> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                bail!("{} 任务已取消", self.runtime_name);
            }
            if self.progress_at.elapsed() >= Duration::from_secs(20) {
                (self.on_progress)()?;
                self.progress_at = Instant::now();
            }
            if Instant::now() >= deadline {
                bail!("{} 协议等待超时", self.runtime_name);
            }
            match self.receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.protocol_closed()),
            }
        }
    }
    fn reverse(
        &mut self,
        frame: &Value,
        cancelled: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<bool> {
        let (Some(method), Some(id)) =
            (frame.get("method").and_then(Value::as_str), frame.get("id"))
        else {
            return Ok(false);
        };
        if method == "session/requestRuntimePreferences" {
            // 创建 ACK 之前上游已分配新 sessionId；无桌面偏好 Host 使用上游的明确兼容默认值。
            self.send(&json!({"id":id,"error":{"code":-32601,"message":"CrewVela 使用 Agent 默认运行偏好"}}))?;
            return Ok(true);
        }
        if method == "interaction/requestOfficialMcpAuthHeaders" {
            // 官方 MCP 鉴权属于工作区而非会话，创建 ACK 前也可能请求；Personal 模式无桌面鉴权 Host。
            self.send(
                &json!({"id":id,"result":{"ok":false,"reason":"official_auth_unavailable"}}),
            )?;
            return Ok(true);
        }
        if method == "interaction/requestCredentialCipher" {
            // The native contract is process-scoped and has no sessionId. It
            // may run before session/create; do not loosen the checks below.
            let params = &frame["params"];
            let valid = params.as_object().is_some_and(|object| object.len() == 3)
                && params["requestId"].as_str().is_some_and(|id| id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
                && matches!(params["operation"].as_str(), Some("encrypt" | "decrypt"))
                && params["value"].as_str().is_some_and(|value|
                    value.chars().count() <= 1_398_232 && (params["operation"] != "encrypt" || value.len() <= 1_048_576));
            if !valid {
                self.send(&json!({"id":id,"error":{"code":-32602,"message":"凭据请求格式无效"}}))?;
                return Ok(true);
            }
            let unavailable = || json!({"requestId":params["requestId"],"result":{"ok":false,"error":{
                "code":"CREDENTIAL_SECURE_STORAGE_UNAVAILABLE",
                "message":"ZCode Desktop 安全凭据存储不可用，请在原生 ZCode Desktop 检查登录后恢复会话"
            }}});
            let result = match &self.account_host {
                Some(host) => match host.credential_cipher(params) {
                    Ok(result) => result,
                    Err(error) if error.downcast_ref::<crate::acp::UnverifiedStop>().is_some() => return Err(error),
                    Err(_) => unavailable(),
                },
                None => unavailable(),
            };
            self.send(&json!({"id":id,"result":result}))?;
            return Ok(true);
        }
        let requested = frame.pointer("/params/sessionId").and_then(Value::as_str);
        let trusted = (requested == self.session.as_deref() && self.session.is_some())
            || requested.is_some_and(|id| self.side_sessions.iter().any(|side| side == id));
        if !trusted {
            self.send(&json!({"id":id,"error":{"code":-32602,"message":"会话身份不匹配"}}))?;
            bail!("ZCode Desktop 反向请求会话身份不匹配：{method}");
        }
        let result = if method == "session/readReplyLanguage" {
            // The Desktop protocol uses null when the host has no reply-language override.
            json!({"language":null})
        } else if method == "interaction/requestPermission" {
            let params = &frame["params"];
            let request = params["requestId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .context("ZCode Desktop 权限请求缺少业务标识")?;
            // 上游断线恢复用新 RPC id 重发同一 requestId，不能生成重复审批卡。
            if let Some((previous, response)) = self.permissions.get(request) {
                if previous != params {
                    bail!("ZCode Desktop 重复权限请求内容发生变化");
                }
                response.clone()
            } else {
                if self.permissions.len() >= 4096 {
                    bail!("ZCode Desktop 权限请求数量超出安全上限");
                }
                let allow = permission(params, cancelled)?;
                let response = json!({"decision": if allow {"allow"} else {"deny"}, "reason": if allow {"CrewVela 仅批准本次调用"} else {"CrewVela 拒绝本次调用"}});
                self.permissions
                    .insert(request.into(), (params.clone(), response.clone()));
                response
            }
        } else if method == "interaction/requestProviderRuntimeHeaders" {
            match self
                .account_host
                .as_ref()
                .context("ZCode Desktop 账号 Host 不可用")
                .and_then(|host| host.auth(&frame["params"], &self.selected_model))
            {
                Ok(result) => result,
                Err(_) => {
                    let message =
                        "ZCode Desktop 账号模型登录、套餐或身份校验失败，请在原生 ZCode Desktop 登录后恢复会话";
                    self.auth_error = Some(message.into());
                    json!({"headersApplied":false,"errorMessage":message})
                }
            }
        } else {
            self.send(&json!({"id":id,"error":{"code":-32601,"message":"CrewVela 不支持此交互，请在 ZCode Desktop 处理"}}))?;
            bail!("ZCode Desktop 请求不支持的交互：{method}");
        };
        self.send(&json!({"id":id,"result":result}))?;
        Ok(true)
    }
    fn request(
        &mut self,
        method: &str,
        params: Value,
        cancelled: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<Value> {
        self.request_with_timeout(method, params, cancelled, permission, CONTROL_TIMEOUT)
    }

    fn check_v4(&mut self, cancelled: &AtomicBool, permission: &mut PermissionHandler<'_>, timeout: Duration) -> Result<()> {
        let query = self.request_with_timeout("v4/commands/query", json!({"commands":[{"sessionId":null,"commandId":uuid::Uuid::new_v4().to_string()}],"clock":true}), cancelled, permission, timeout)?;
        ensure!(query.get("results").is_some_and(Value::is_array), "{} 未确认支持 v4 协议", self.runtime_name);
        Ok(())
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        cancelled: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
        timeout: Duration,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        if let Err(error) = self.send(&json!({"id":id,"method":method,"params":params})) {
            let broken_pipe = error.chain().any(|cause| {
                cause.downcast_ref::<std::io::Error>().is_some_and(|err| err.kind() == std::io::ErrorKind::BrokenPipe)
                    || cause.downcast_ref::<serde_json::Error>().is_some_and(|err| err.io_error_kind() == Some(std::io::ErrorKind::BrokenPipe))
            });
            return Err(if broken_pipe { self.protocol_closed() } else { error });
        }
        let mut deadline = Instant::now() + timeout;
        loop {
            let frame = self.receive(deadline, cancelled)?;
            // 双向 RPC 的 ID 空间独立，同号权限请求不能当成命令 ACK。
            let reverse_start = Instant::now();
            if self.reverse(&frame, cancelled, permission)? {
                deadline += reverse_start.elapsed();
                continue;
            }
            if frame.get("method").is_none() && frame.get("id") == Some(&json!(id)) {
                if let Some(error) = frame.get("error") {
                    return Err(RequestRejected {
                        method: method.into(),
                        code: error["code"].as_i64(),
                        // Keep only the known machine-readable reason, never raw
                        // provider errors/stacks that may contain local credentials.
                        resume_lineage_invalid: method == "session/resume"
                            && error["code"] == -32603
                            && error["message"] == "project.session-lineage-invalid",
                    }
                    .into());
                }
                return frame
                    .get("result")
                    .cloned()
                    .context("ZCode Desktop 响应缺少 result");
            }
            if self.queued.len() >= 4096 {
                bail!("ZCode Desktop 控制请求积压超出安全上限");
            }
            self.queued.push_back(frame);
        }
    }
    fn command(
        &mut self,
        kind: &str,
        session: Option<&str>,
        payload: Value,
        cancelled: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<(String, Value)> {
        let id = uuid::Uuid::new_v4().to_string();
        self.command_id(kind, session, payload, &id, None, cancelled, permission)
    }
    fn command_id(
        &mut self,
        kind: &str,
        session: Option<&str>,
        payload: Value,
        id: &str,
        base_revision: Option<u64>,
        cancelled: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<(String, Value)> {
        self.command_id_at_epoch(kind, session, payload, id, base_revision, None, cancelled, permission)
    }
    fn command_id_at_epoch(&mut self, kind: &str, session: Option<&str>, payload: Value, id: &str, base_revision: Option<u64>, epoch: Option<&str>, cancelled: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<(String, Value)> {
        let mut envelope = json!({"commandId":id,"clientId":"crewvela","sessionId":session,"type":kind,"payload":payload,"issuedAt":chrono::Utc::now().timestamp_millis()});
        if let Some(epoch) = epoch { envelope["baseLogEpoch"] = json!(epoch); }
        if let Some(revision) = base_revision {
            envelope["baseRevision"] = json!(revision);
        }
        let result = self.request("v4/command", envelope, cancelled, permission)?;
        if result.get("commandId").and_then(Value::as_str) != Some(id) {
            bail!("ZCode Desktop 命令 ACK 标识不匹配");
        }
        // CLI 返回裸 CommandAck；accepted 仅代表接收，不代表模型结束。
        if !matches!(
            result.get("status").and_then(Value::as_str),
            Some("accepted" | "duplicate" | "noop")
        ) {
            let reason = result
                .get("reasonCode")
                .and_then(Value::as_str)
                .filter(|reason| {
                    reason.len() <= 128
                        && reason
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_')
                })
                .unwrap_or("unknown");
            let text = if kind == "sendText"
                && reason == "fault.command.executionFailed"
                && result
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.starts_with("Session model must be provider-qualified:"))
            {
                "ZCode Desktop 会话尚未选择可用模型，请先选择 Personal API 模型".to_owned()
            } else {
                // ACK message 不透传：上游拒绝体可能携带凭据片段（fixture-secret 卫生断言），
                // 诊断需要完整原因时用隔离探针直查 ACK。
                format!("ZCode Desktop 命令 {kind} 被拒绝：{reason}")
            };
            if matches!(
                result.get("status").and_then(Value::as_str),
                Some("failed" | "rejected" | "stale")
            ) {
                return Err(CommandRejected {
                    message: text,
                    admission_denied: matches!(result["status"].as_str(), Some("rejected" | "stale")),
                }.into());
            }
            bail!("ZCode Desktop 命令 ACK 状态未知");
        }
        Ok((id.to_owned(), result))
    }
    fn cleanup_checked(&mut self) -> Result<()> {
        if let Some(pid) = self.tracked_process_id {
            // Both UI stop and worker/error cleanup must use the same Atomic's
            // stop gate. A raw Child stop unregisters the identity before the
            // Live PID is cleared, letting concurrent UI stop see a stale PID.
            let current = pid.load(Ordering::Acquire);
            if current != 0 && current != self.child.id() {
                return Err(crate::acp::UnverifiedStop("ZCode 跟踪身份已改变，拒绝清理其他进程".into()).into());
            }
            crate::acp::terminate_acp_process_checked(pid)?;
        }
        if !self._identity.is_verified() {
            crate::acp::stop_child_checked(&mut self.child)?;
        }
        #[cfg(test)]
        if let Some(after_stop) = self.cleanup_after_stop.take() { after_stop(); }
        self.child.wait().map_err(|error| anyhow!(crate::acp::UnverifiedStop(format!("等待 ZCode 原始进程退出失败：{error}"))))?;
        if let Some(pid) = self.tracked_process_id {
            let _ = pid.compare_exchange(self.child.id(), 0, Ordering::AcqRel, Ordering::Acquire);
        }
        Ok(())
    }
    fn stop_current_checked(&mut self) -> Result<()> {
        if let Some(session) = self.session.clone() { self.stop(&session) }
        else { self.cleanup_checked() }
    }
    fn stop(&mut self, session: &str) -> Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        let _ = self.send(&json!({"id":id,"method":"v4/command","params":{"commandId":uuid::Uuid::new_v4().to_string(),"clientId":"crewvela","sessionId":session,"type":"stop","payload":{},"issuedAt":chrono::Utc::now().timestamp_millis()}}));
        // Native cancellation is followed by explicit checked termination;
        // Drop cannot communicate an unverified stop to its caller.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(frame))
                    if frame.get("method").is_none() && frame.get("id") == Some(&json!(id)) =>
                {
                    break
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        self.cleanup_checked()
    }
}

#[derive(Default)]
struct Wire {
    ordinal: u64,
    assembly: Option<(Value, BTreeMap<u64, Vec<u8>>, usize, Instant)>,
}
impl Wire {
    fn accept(&mut self, wire: &Value, session: &str, subscription: &str) -> Result<Option<Value>> {
        if wire["wireVersion"] != 3
            || wire["topic"] != format!("conversation/{session}")
            || wire["subscriptionId"] != subscription
        {
            bail!("ZCode Desktop wire 版本或订阅身份不匹配");
        }
        if !matches!(
            wire["deliveryKind"].as_str(),
            Some("initial" | "online" | "recovery")
        ) {
            bail!("ZCode Desktop 帧 deliveryKind 无效");
        }
        let ordinal = wire["logicalFrameOrdinal"]
            .as_u64()
            .filter(|n| *n > 0)
            .context("ZCode Desktop 帧 ordinal 无效")?;
        if ordinal <= self.ordinal {
            bail!("ZCode Desktop 帧 ordinal 重复或回退");
        }
        if wire["logicalFrameId"].as_str().is_none_or(str::is_empty) {
            bail!("ZCode Desktop 帧标识无效");
        }
        let frame = match wire["kind"].as_str() {
            Some("complete") => {
                if self.assembly.is_some() {
                    bail!("ZCode Desktop 未完成分片被覆盖");
                }
                wire["frame"].clone()
            }
            Some("fragment") => {
                let count = wire["fragmentCount"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 4096)
                    .context("ZCode Desktop 分片数量无效")?;
                let size = wire["logicalBytes"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= MAX_FRAME as u64)
                    .context("ZCode Desktop 分片字节数无效")? as usize;
                let index = wire["fragmentIndex"]
                    .as_u64()
                    .filter(|n| *n < count)
                    .context("ZCode Desktop 分片下标无效")?;
                let data = base64::engine::general_purpose::STANDARD
                    .decode(wire["dataBase64"].as_str().context("ZCode Desktop 分片无数据")?)
                    .context("ZCode Desktop 分片 base64 无效")?;
                if data.is_empty() || data.len() > size {
                    bail!("ZCode Desktop 分片大小无效");
                }
                if wire.pointer("/checksum/algorithm").and_then(Value::as_str) != Some("crc32") {
                    bail!("ZCode Desktop 分片校验算法不支持");
                }
                // ponytail: 单订阅仅保留一个有界逻辑帧；交错帧拒绝并重试整个任务。
                let (metadata, parts, bytes, started) = self
                    .assembly
                    .get_or_insert_with(|| (wire.clone(), BTreeMap::new(), 0, Instant::now()));
                for key in [
                    "logicalFrameOrdinal",
                    "logicalFrameId",
                    "fragmentCount",
                    "logicalBytes",
                    "checksum",
                    "deliveryKind",
                ] {
                    if metadata[key] != wire[key] {
                        bail!("ZCode Desktop 分片元数据不一致");
                    }
                }
                if started.elapsed() > Duration::from_secs(60) {
                    bail!("ZCode Desktop 分片重组超时");
                }
                if let Some(previous) = parts.get(&index) {
                    if previous != &data {
                        bail!("ZCode Desktop 重复分片内容冲突");
                    }
                } else {
                    *bytes += data.len();
                    parts.insert(index, data);
                }
                if *bytes > size {
                    bail!("ZCode Desktop 分片超出声明大小");
                }
                if parts.len() != count as usize {
                    return Ok(None);
                }
                let joined: Vec<u8> = parts.values().flatten().copied().collect();
                if joined.len() != size
                    || wire.pointer("/checksum/value").and_then(Value::as_str)
                        != Some(format!("{:08x}", crc32fast::hash(&joined)).as_str())
                {
                    bail!("ZCode Desktop 分片校验失败");
                }
                self.assembly = None;
                serde_json::from_slice(&joined).context("ZCode Desktop 分片 JSON 无效")?
            }
            _ => bail!("ZCode Desktop 未知 wire 类型"),
        };
        if frame["topic"] != wire["topic"] || frame["subscriptionId"] != wire["subscriptionId"] {
            bail!("ZCode Desktop 逻辑帧与外壳身份不一致");
        }
        self.ordinal = ordinal;
        Ok(Some(frame))
    }
}

#[derive(Default)]
struct Projection {
    rows: BTreeMap<u64, Value>,
    seq: u64,
    revision: Option<u64>,
    epoch: String,
    initialized: bool,
    title: Option<String>,
    title_source: Option<String>,
    permission_mode: Option<String>,
    mode_revision: u64,
    queue: Option<Value>,
    control: Option<Value>,
    input_routing: Option<String>,
    /// 上下文用量（used/size/cost/cache/breakdown）：runtime.contextUsage 权威，
    /// projection.contextUsed/contextWindow 兜底；state.updated 增量同样可能携带。
    context_usage: Option<Value>,
}
impl Projection {
    fn session_title(&self) -> Option<&str> {
        if !matches!(self.title_source.as_deref(), Some("generated" | "custom")) {
            return None;
        }
        self.title.as_deref().map(str::trim).filter(|title| {
            !title.is_empty() && !title.chars().any(char::is_control)
        })
    }
    fn read_state(&mut self, state: &Value) -> Result<()> {
        if let Some(meta) = state.get("meta") {
            // state.updated carries a partial patch; omitted fields retain their value.
            if let Some(title) = meta.get("title") {
                self.title = title.as_str().map(str::to_owned);
            }
            if let Some(source) = meta.get("titleSource") {
                self.title_source = source.as_str().map(str::to_owned);
            }
        }
        if let Some(revision) = state.get("revision") {
            self.revision = Some(revision.as_u64().context("ZCode Desktop 状态版本无效")?);
        }
        self.read_mode(&state["config"])?;
        if let Some(queue) = state.get("queue") {
            let items = queue["items"].as_array().context("ZCode Desktop 队列格式无效")?;
            if items.len() > 1000 || !queue["autoDrain"].is_boolean() {
                bail!("ZCode Desktop 队列超出限制或缺少执行状态");
            }
            for item in items {
                if item["queueItemId"].as_str().is_none_or(str::is_empty)
                    || item["sourceCommandId"].as_str().is_none_or(str::is_empty)
                    || !item["text"].is_string()
                    || !matches!(
                        item.pointer("/dispatch/state").and_then(Value::as_str),
                        Some("queued" | "reserved" | "promoting")
                    )
                {
                    bail!("ZCode Desktop 队列条目格式无效");
                }
            }
            self.queue = Some(queue.clone());
        }
        if let Some(control) = state.get("control") {
            if !matches!(
                control["phase"].as_str(),
                Some(
                    "draft"
                        | "prewarming"
                        | "running"
                        | "completedSuccess"
                        | "completedInterrupted"
                        | "error"
                )
            ) {
                bail!("ZCode Desktop 运行状态无效");
            }
            self.control = Some(control.clone());
        }
        if let Some(routing) = state.get("inputRouting") {
            let mode = routing["mode"].as_str().context("ZCode Desktop 输入路由无效")?;
            if !matches!(mode, "startNow" | "enqueue" | "guide" | "reject" | "choice") {
                bail!("ZCode Desktop 输入路由未知");
            }
            self.input_routing = Some(mode.into());
        }
        self.read_context_usage(state);
        Ok(())
    }
    /// 上下文用量采集。实际 wire 键位（安装版 3.14.4 实测 + onModelComplete 源码）：
    /// state.updated patch = { usage: { contextWindow: { usedTokens, maxTokens,
    /// cache, breakdown }, cumulative: {...} } }；快照 state 同键。runtime.contextUsage
    /// /projection.contextUsed 是仓库源码的键位，同时探测兼容。
    fn read_context_usage(&mut self, state: &Value) {
        // 首选：安装版 wire 的 usage.contextWindow。
        let window = state.pointer("/usage/contextWindow");
        if let Some(window) = window {
            let used = window.get("usedTokens").and_then(Value::as_u64);
            let size = window.get("maxTokens").and_then(Value::as_u64);
            if size.is_some_and(|size| size > 0) {
                let mut merged = json!({
                    "used": used.unwrap_or(0),
                    "size": size,
                });
                if let Some(cache) = window.get("cache") {
                    merged["cache"] = cache.clone();
                }
                if let Some(breakdown) = window.get("breakdown") {
                    merged["breakdown"] = breakdown.clone();
                }
                self.context_usage = Some(merged);
                return;
            }
        }
        // 兼容：仓库源码键位 runtime.contextUsage。
        if let Some(usage) = state
            .pointer("/runtime/contextUsage")
            .or_else(|| state.get("contextUsage"))
        {
            if usage.is_object() && usage.get("size").and_then(Value::as_u64).unwrap_or(0) > 0 {
                self.context_usage = Some(usage.clone());
                return;
            }
        }
        // 兼容：projection.contextUsed/contextWindow 扁平键（增量续算）。
        let projection = state.get("projection").unwrap_or(state);
        let used = projection.get("contextUsed").and_then(Value::as_u64);
        let size = projection.get("contextWindow").and_then(Value::as_u64);
        if used.is_some() || size.is_some() {
            let current = self.context_usage.get_or_insert_with(|| json!({}));
            if let Some(used) = used {
                current["used"] = json!(used);
            }
            if let Some(size) = size {
                if size > 0 {
                    current["size"] = json!(size);
                }
            }
        }
    }
    /// 当前上下文用量（None = 尚未收到任何 usage 事实）。
    pub(crate) fn context_usage(&self) -> Option<Value> {
        self.context_usage.clone()
    }
    fn confirms_mode_after(&self, mode: &str, previous_seq: u64) -> bool {
        self.mode_revision > previous_seq && self.permission_mode.as_deref() == Some(mode)
    }
    fn read_mode(&mut self, config: &Value) -> Result<()> {
        if config.is_null() {
            return Ok(());
        }
        if config["planEnabled"] == true {
            self.permission_mode = Some("plan".into());
            self.mode_revision += 1;
        } else if let Some(mode) = config["mode"].as_str() {
            if !valid_permission_mode(mode) {
                bail!("ZCode Desktop 返回不支持的权限模式");
            }
            self.permission_mode = Some(mode.into());
            self.mode_revision += 1;
        }
        Ok(())
    }
    fn apply(&mut self, frame: &Value, session: &str, subscription: &str) -> Result<()> {
        if frame.get("topic").and_then(Value::as_str)
            != Some(format!("conversation/{session}").as_str())
            || frame.get("subscriptionId").and_then(Value::as_str) != Some(subscription)
        {
            bail!("ZCode Desktop 收到其他会话或订阅的帧");
        }
        let to = frame
            .get("toSeq")
            .and_then(Value::as_u64)
            .context("ZCode Desktop 帧缺少水位")?;
        let payload = &frame["payload"];
        if to < self.seq && self.initialized {
            bail!("ZCode Desktop 帧水位回退");
        }
        match payload.get("kind").and_then(Value::as_str) {
            Some("snapshot") => {
                let snapshot = &payload["snapshot"];
                if snapshot.get("sessionId").and_then(Value::as_str) != Some(session) {
                    bail!("ZCode Desktop 快照会话标识不匹配");
                }
                if snapshot.get("logEpoch").and_then(Value::as_str) != Some(self.epoch.as_str())
                    || frame["fromSeq"] != 0
                    || snapshot["seq"] != to
                {
                    bail!("ZCode Desktop 快照 epoch 或水位不匹配");
                }
                self.rows.clear();
                self.title = None;
                self.title_source = None;
                self.read_state(snapshot)?;
                for row in snapshot
                    .pointer("/rows/window")
                    .and_then(Value::as_array)
                    .context("ZCode Desktop 快照缺少 rows")?
                {
                    self.row(row.clone())?;
                }
                self.initialized = true;
            }
            Some("deltas") => {
                if !self.initialized
                    || frame.get("fromSeq").and_then(Value::as_u64) != Some(self.seq)
                {
                    bail!("ZCode Desktop 增量水位不连续，拒绝推断任务已完成");
                }
                for delta in payload
                    .get("deltas")
                    .and_then(Value::as_array)
                    .context("ZCode Desktop 缺少 deltas")?
                {
                    match delta.get("op").and_then(Value::as_str) {
                        Some("row.appended" | "row.upserted") => self.row(delta["row"].clone())?,
                        Some("row.removed") => {
                            let from = delta["fromRowId"].as_u64().context("ZCode Desktop 删除行无标识")?;
                            self.rows.retain(|id, _| *id < from);
                        }
                        Some("row.delta") => {
                            let id = delta["rowId"].as_u64().context("ZCode Desktop 流式行无标识")?;
                            if matches!(delta["path"].as_str(), Some("text" | "inputText")) {
                                let field = delta["path"].as_str().unwrap();
                                let row =
                                    self.rows.get_mut(&id).context("ZCode Desktop 流式增量引用未知行")?;
                                let text = row[field]
                                    .as_str()
                                    .context("ZCode Desktop 流式行没有文本")?
                                    .to_owned()
                                    + delta["append"].as_str().context("ZCode Desktop 文本增量无效")?;
                                if text.len() > MAX_FRAME {
                                    bail!("ZCode Desktop 累计流式文本超出安全上限");
                                }
                                row[field] = json!(text);
                            }
                        }
                        Some("state.updated") => self.read_state(&delta["patch"])?,
                        Some("workflowRun.updated" | "workflowRun.removed") => {}
                        _ => bail!("ZCode Desktop 未知增量操作"),
                    }
                }
            }
            _ => bail!("ZCode Desktop 未知帧载荷"),
        }
        self.seq = to;
        if self.rows.len() > 100_000 {
            bail!("ZCode Desktop 会话行数超出安全上限");
        }
        Ok(())
    }
    fn row(&mut self, row: Value) -> Result<()> {
        let id = row["rowId"].as_u64().context("ZCode Desktop 行缺少标识")?;
        self.rows.insert(id, row);
        Ok(())
    }
    fn turn_header(&self, command: &str) -> Option<&Value> {
        self.rows.values()
            .find(|row| row["kind"] == "turnHeader" && row["sourceCommandId"] == command)
    }
    fn completion(&self, command: &str) -> Result<Option<String>> {
        let Some(header) = self.turn_header(command) else {
            return Ok(None);
        };
        match header["state"].as_str() {
            Some("completedSuccess") => {
                let turn = header["turnId"].as_str().filter(|id| !id.is_empty())
                    .context("ZCode Desktop 轮次结束但缺少原生标识")?;
                let text = self
                    .rows
                    .values()
                    .filter(|row| row["kind"] == "assistantText" && row["turnId"].as_str() == Some(turn))
                    .filter_map(|row| row["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    bail!("ZCode Desktop 已结束当前任务但没有可交付文本");
                }
                Ok(Some(text))
            }
            Some("failed" | "completedInterrupted") => bail!("ZCode Desktop 当前任务失败或被中断"),
            Some("running") => Ok(None),
            _ => bail!("ZCode Desktop 当前任务状态未知"),
        }
    }
}

/// Retain the installed CLI's own authentication, tools and settings. Reuse
/// native launcher discovery so a long role/context prompt never goes via cmd.exe.
pub(super) fn native_cli_prompt_command(cwd: &Path, prompt: &str, mode: &str) -> Result<Command> {
    let base = launch(&runtime_executable(super::domain::AgentRuntimeKind::ZCodeCli)?)?;
    let mut args = base.get_args().map(ToOwned::to_owned).collect::<Vec<_>>();
    if args.len() < 2 || args[args.len()-2] != "app-server" || args[args.len()-1] != "--stdio" { bail!("ZCode CLI 原生启动契约已变化，未更换登录或权限配置"); }
    args.truncate(args.len()-2);
    let mut command = Command::new(base.get_program()); command.args(args);
    for (key,value) in base.get_envs() { if let Some(value) = value { command.env(key,value); } else { command.env_remove(key); } }
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    command.arg("--cwd").arg(crate::acp::normalize_harness_path(cwd)?).args(["--mode",mode,"--json","--no-color","--prompt",prompt]);
    Ok(command)
}

pub(super) fn execute(
    kind: super::domain::AgentRuntimeKind,
    cwd: &Path,
    resume: Option<&str>,
    prompt: &str,
    mode: &str,
    model: Option<&str>,
    mcp: Value,
    cancelled: &AtomicBool,
    permission: &mut PermissionHandler<'_>,
    on_progress: &mut dyn FnMut() -> Result<()>,
) -> Result<ZCodeResult> {
    execute_command(
        launch(&runtime_executable(kind)?)?,
        cwd,
        resume,
        prompt,
        mode,
        model,
        mcp,
        cancelled,
        permission,
        on_progress,
    )
}
fn execute_command(
    command: Command,
    cwd: &Path,
    resume: Option<&str>,
    prompt: &str,
    mode: &str,
    model: Option<&str>,
    mcp: Value,
    cancelled: &AtomicBool,
    permission: &mut PermissionHandler<'_>,
    on_progress: &mut dyn FnMut() -> Result<()>,
) -> Result<ZCodeResult> {
    let selection = model_selection(model)?;
    if selection
        .as_ref()
        .and_then(|s| s["providerId"].as_str())
        .is_some_and(|p| p.starts_with("account:"))
    {
        bail!("ZCode Desktop 协作任务暂不支持账号模型，请使用 Personal 模型或独立 ZCode Desktop 会话");
    }
    let mut client = Client::spawn(command, cwd, on_progress)?;
    let outcome = (|| -> Result<ZCodeResult> {
    let handshake = client.request("v4/commands/query", json!({"commands":[{"sessionId":null,"commandId":uuid::Uuid::new_v4().to_string()}],"clock":true}), cancelled, permission)?;
    if !handshake.get("results").is_some_and(Value::is_array) {
        bail!("ZCode Desktop 不支持所需 v4 控制协议");
    }
    let session = if let Some(resume) = resume {
        client.session = Some(resume.to_owned());
        // 显式 resume 在 runtime 激活之前替换每次运行的 MCP token，不能靠订阅的冷恢复旧配置。
        client.request("session/resume", json!({"sessionId":resume,"workspace":{"workspaceKey":cwd.to_string_lossy(),"workspacePath":cwd.to_string_lossy()},"mcpServers":mcp}), cancelled, permission)?;
        resume.to_owned()
    } else {
        let mut config = json!({"mode":mode});
        if let Some(selection) = &selection {
            config["modelSelection"] = selection.clone();
        }
        let (_, ack) = client.command(
            "createSession",
            None,
            json!({"workspaceId":cwd.to_string_lossy(),"config":config,"mcpServers":mcp}),
            cancelled,
            permission,
        )?;
        ack.pointer("/result/sessionId")
            .and_then(Value::as_str)
            .context("ZCode Desktop 创建 ACK 缺少 sessionId")?
            .to_owned()
    };
    client.session = Some(session.clone());
    let connection = uuid::Uuid::new_v4().to_string();
    let subscribe = client.request("v4/conversation/subscribe", json!({"topic":format!("conversation/{session}"),"connectionId":connection,"clientMode":"desktop-continuous","workspace":{"workspaceKey":cwd.to_string_lossy(),"workspacePath":cwd.to_string_lossy()}}), cancelled, permission)?;
    let subscription = subscribe
        .pointer("/ack/subscriptionId")
        .and_then(Value::as_str)
        .context("ZCode Desktop 订阅 ACK 缺少标识")?
        .to_owned();
    let mut payload = json!({"text":prompt,"requestedDelivery":"startNow","mode":mode});
    if let Some(selection) = selection {
        payload["modelSelection"] = selection;
    }
    let (command_id, _) =
        client.command("sendText", Some(&session), payload, cancelled, permission)?;
    let deadline = Instant::now() + Duration::from_secs(60 * 60);
    let mut projection = Projection {
        epoch: subscribe
            .pointer("/ack/logEpoch")
            .and_then(Value::as_str)
            .context("ZCode Desktop 订阅 ACK 缺少 epoch")?
            .into(),
        ..Default::default()
    };
    let mut wire_assembler = Wire::default();
    let outcome = (|| loop {
        if cancelled.load(Ordering::Acquire) {
            bail!("ZCode Desktop 任务已取消");
        }
        let frame = if let Some(frame) = client.queued.pop_front() {
            frame
        } else {
            client.receive(deadline, cancelled)?
        };
        if client.reverse(&frame, cancelled, permission)? {
            continue;
        }
        if frame.get("method").and_then(Value::as_str) != Some("v4/conversation/frame") {
            continue;
        }
        let wire = &frame["params"];
        let Some(frame) = wire_assembler.accept(wire, &session, &subscription)? else {
            continue;
        };
        projection.apply(&frame, &session, &subscription)?;
        if let Some(text) = projection.completion(&command_id)? {
            break Ok(ZCodeResult {
                text,
                session_id: session.clone(),
            });
        }
    })();
    if outcome.is_ok() {
        client.request(
            "session/close",
            json!({"sessionId":session}),
            cancelled,
            permission,
        )?;
    }
    outcome
    })();
    // Handshake/create/subscribe/close failures also require confirmed cleanup
    // before a normal error can release or retry this run's lease.
    if outcome.is_err() { client.stop_current_checked()?; }
    else { client.cleanup_checked()?; }
    outcome
}

/// Persistent desktop connection sharing the collaboration transport and projection.
pub(crate) fn valid_permission_mode(mode: &str) -> bool {
    matches!(mode, "plan" | "build" | "edit" | "yolo")
}

/// ZCode Desktop 原生会话注入的 stdio MCP 条目（浏览器/电脑控制等），
/// 转成 v4 协议 wire 形态。宿主以 `"command" in server` 判定 stdio，env 沿用
/// [{name,value}] 数组（protocolMcpServersToRuntimeMcpConfig 原样 map）。
/// 读取失败静默为空，不阻塞会话启动。
pub(crate) fn zcode_desktop_mcp_servers() -> Vec<Value> {
    let servers = crate::mcp::enabled_native_chat_servers().unwrap_or_else(|error| {
        eprintln!("[crewvela] 读取 ZCode 原生会话 MCP 配置失败：{error:#}");
        Vec::new()
    });
    servers
        .into_iter()
        .map(|server| {
            // v4 wire schema .strict()：stdio 形态不允许 type 字段（以 command 判定）。
            json!({
                "name": server.get("name").cloned().unwrap_or(Value::Null),
                "command": server
                    .get("command")
                    .cloned()
                    .unwrap_or(Value::String(String::new())),
                "args": server.get("args").cloned().unwrap_or_else(|| json!([])),
                "env": server.get("env").cloned().unwrap_or_else(|| json!([])),
                "isolation": "session",
            })
        })
        .collect()
}
pub(crate) struct DesktopConnection<'a> {
    client: Client<'a>,
    pub session_id: String,
    subscription: String,
    projection: Projection,
    wire: Wire,
    pub models: Vec<ModelChoice>,
    pub selected_model_id: Option<String>,
    pub permission_mode: String,
    workflow_notice: Option<String>,
    workflow_enabled: bool,
    child: Option<ChildConversation>,
    side: Option<ChildConversation>,
    workspace: Value,
}
struct ChildConversation {
    owner: String,
    session: String,
    subscription: String,
    connection: String,
    projection: Projection,
    wire: Wire,
    error: Option<String>,
}
impl<'a> DesktopConnection<'a> {
    pub fn open(
        command: Command,
        cwd: &Path,
        resume: Option<&str>,
        allow_empty_recreate: bool,
        mcp_servers: Vec<Value>,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
        progress: &'a mut dyn FnMut() -> Result<()>,
    ) -> Result<Self> {
        Self::open_tracked(command, cwd, resume, allow_empty_recreate, mcp_servers, stopped, permission, progress, None)
    }
    pub(crate) fn open_tracked(
        command: Command,
        cwd: &Path,
        resume: Option<&str>,
        allow_empty_recreate: bool,
        mcp_servers: Vec<Value>,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
        progress: &'a mut dyn FnMut() -> Result<()>,
        process_id: Option<&'a std::sync::atomic::AtomicU32>,
    ) -> Result<Self> {
        Self::open_with_policy(command, cwd, resume, allow_empty_recreate, mcp_servers, stopped, permission, progress, process_id, None)
    }

    fn open_with_policy(
        mut command: Command,
        cwd: &Path,
        resume: Option<&str>,
        allow_empty_recreate: bool,
        mcp_servers: Vec<Value>,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
        progress: &'a mut dyn FnMut() -> Result<()>,
        process_id: Option<&'a std::sync::atomic::AtomicU32>,
        managed_tools: Option<&[&str]>,
    ) -> Result<Self> {
        ensure!(managed_tools.is_none() || (!allow_empty_recreate && mcp_servers.is_empty()), "ZCode CLI 托管会话必须使用固定身份和受限工具");
        if let Some(pid) = process_id {
            crate::acp::terminate_acp_process_checked(pid)?;
        }
        let policy = if managed_tools.is_some() { json!({"enabled":false}) } else { public_workflow_policy(&command)? };
        let mut client_slot = Some(if managed_tools.is_some() {
            let host = match account_host::AccountHost::prepare(&command) {
                Ok(host) => host,
                Err(cli_error) => {
                    // A standalone Node CLI cannot load the Electron keyring from its source tree.
                    // Use the installed ZCode helper for the same local credential store.
                    let host_command = executable().and_then(|path| launch(&path))
                        .context("无法定位 ZCode 的安全账号辅助程序")?;
                    match account_host::AccountHost::prepare(&host_command) {
                        Ok(Some(host)) => {
                            // The provider overlay includes the builtin file path in its revision.
                            for (key, value) in host_command.get_envs() {
                                if matches!(key.to_str(), Some("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE" | "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE")) {
                                    if let Some(value) = value { command.env(key, value); }
                                }
                            }
                            Some(host)
                        }
                        _ => return Err(cli_error).context("ZCode CLI 无法使用本机账号的安全登录存储"),
                    }
                }
            };
            Client::spawn_with_account_host(command, cwd, progress, host, "ZCode CLI")?
        } else {
            Client::spawn(command, cwd, progress)?
        });
        let mut connection_slot: Option<Self> = None;
        let outcome = (|| -> Result<()> {
        let client = client_slot.as_mut().unwrap();
        client.tracked_process_id = process_id;
        if let Some(pid) = process_id { pid.store(client.child.id(), Ordering::Release); }
        if let Some(host) = &client.account_host {
            let config = host.config.clone();
            let revision = config["revision"].clone();
            let result =
                client.request("provider/updateAccountConfig", config, stopped, permission)?;
            if result["receivedRevision"] != revision {
                bail!("ZCode Desktop 账号配置版本确认失败");
            }
        }
        client.check_v4(stopped, permission, CONTROL_TIMEOUT)?;
        let workspace =
            json!({"workspaceKey":cwd.to_string_lossy(),"workspacePath":cwd.to_string_lossy()});
        let mut workflow_enabled = policy["enabled"] == true;
        let mut workflow_notice = policy["notice"].as_str().map(str::to_owned);
        let confirmed = if managed_tools.is_some() { false } else { sync_workflow_policy(
            client,
            &workspace,
            workflow_enabled,
            stopped,
            permission,
        )? };
        if confirmed != workflow_enabled {
            workflow_notice = Some("ZCode Desktop 安装版无法确认工作流开放，创建入口暂不可用".into());
        }
        workflow_enabled = confirmed;
        let mut settings = Value::Null;
        let mut catalog = Vec::new();
        if resume.is_some() {
            // The native read/resume API only returns the current model. A
            // deferred, message-free draft supplies the process-owned catalog.
            let mut params = json!({"workspace":workspace,"mode":"build","persistence":"deferred","titleGenerationEnabled":false,"mcpServers":mcp_servers.clone(),"dynamicWorkflowEnabled":workflow_enabled});
            if let Some(tools) = managed_tools {
                params["toolAllowlist"] = json!(tools);
                params["mode"] = json!("plan");
            }
            let draft = client.request("session/create", params, stopped, permission)?;
            let draft_id = draft
                .pointer("/session/sessionId")
                .and_then(Value::as_str)
                .context("ZCode Desktop 目录草稿缺少标识")?;
            catalog = model_choices(draft.get("settings").unwrap_or(&Value::Null));
            client.request(
                "session/close",
                json!({"sessionId":draft_id}),
                stopped,
                permission,
            )?;
        }
        let resumed = if let Some(sid) = resume {
            client.session = Some(sid.into());
            let mut params = json!({"sessionId":sid,"workspace":workspace,"mcpServers":mcp_servers.clone(),"dynamicWorkflowEnabled":workflow_enabled});
            // A cold resume reconstructs the tool registry. The create-time list is not inherited.
            if let Some(tools) = managed_tools { params["toolAllowlist"] = json!(tools); }
            match client.request(
                "session/resume",
                params,
                stopped,
                permission,
            ) {
                Ok(result) => {
                    settings = result.get("settings").cloned().unwrap_or(Value::Null);
                    let returned = result
                        .get("sessionId")
                        .or_else(|| result.pointer("/session/sessionId"))
                        .and_then(Value::as_str)
                        .context("ZCode Desktop 恢复结果缺少会话标识")?;
                    if returned != sid {
                        bail!("ZCode Desktop 恢复返回其他会话标识");
                    }
                    Some(sid.to_owned())
                }
                Err(error) => {
                    let mut missing = false;
                    if allow_empty_recreate {
                        if let Some(rejected) = error.downcast_ref::<RequestRejected>() {
                            missing = rejected.code == Some(-32004);
                            if rejected.resume_lineage_invalid {
                                // Installed 3.17 checks lineage before reporting not-found.
                                // Query the exact ID without a workspace filter: an empty
                                // filtered list could otherwise conceal an existing history.
                                let listed = client.request("session/list", json!({"sessionIds":[sid]}), stopped, permission)
                                    .context("无法核对 ZCode Desktop 原会话是否存在，未自动重建")?;
                                let sessions = listed["sessions"].as_array()
                                    .context("ZCode Desktop 原会话查询结果不完整，未自动重建")?;
                                missing = sessions.is_empty();
                            }
                        }
                    }
                    if !missing { return Err(error); }
                    client.session = None;
                    None
                }
            }
        } else {
            None
        };
        let session_id = if let Some(sid) = resumed {
            sid
        } else {
            // Upstream discards empty drafts even with immediate persistence.
            // Only confirmed empty drafts may be recreated after not-found.
            let mut create = json!({"workspace":workspace,"mode":"build","persistence":"deferred","titleGenerationEnabled":true,"mcpServers":mcp_servers.clone(),"dynamicWorkflowEnabled":workflow_enabled});
            if let Some(tools) = managed_tools {
                // The strict protocol schema rejects this field on runtimes that cannot enforce it.
                create["toolAllowlist"] = json!(tools);
                create["mode"] = json!("plan");
                create["dynamicWorkflowEnabled"] = json!(false);
                create["titleGenerationEnabled"] = json!(false);
            }
            let result = client.request("session/create", create, stopped, permission)?;
            settings = result.get("settings").cloned().unwrap_or(Value::Null);
            result
                .pointer("/session/sessionId")
                .and_then(Value::as_str)
                .filter(|sid| !sid.is_empty() && sid.len() <= 1024)
                .context("ZCode Desktop 创建结果缺少会话标识")?
                .to_owned()
        };
        client.session = Some(session_id.clone());
        let result = client.request("v4/conversation/subscribe", json!({"topic":format!("conversation/{session_id}"),"connectionId":uuid::Uuid::new_v4().to_string(),"clientMode":"desktop-continuous","workspace":workspace}), stopped, permission)?;
        let subscription = result
            .pointer("/ack/subscriptionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .context("ZCode Desktop 订阅缺少标识")?
            .to_owned();
        let epoch = result
            .pointer("/ack/logEpoch")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .context("ZCode Desktop 订阅缺少 epoch")?
            .to_owned();
        if result.pointer("/ack/mode").and_then(Value::as_str) != Some("snapshot") {
            bail!("ZCode Desktop 新连接必须提供完整快照");
        }
        client.selected_model = settings
            .pointer("/model/current")
            .cloned()
            .unwrap_or(Value::Null);
        connection_slot = Some(Self {
            client: client_slot.take().unwrap(),
            session_id,
            subscription,
            projection: Projection {
                epoch,
                ..Default::default()
            },
            wire: Wire::default(),
            models: if resume.is_some() {
                catalog
            } else {
                model_choices(&settings)
            },
            selected_model_id: settings
                .pointer("/model/current")
                .filter(|v| v.is_object())
                .map(Value::to_string),
            permission_mode: settings
                .pointer("/mode/current")
                .and_then(Value::as_str)
                .filter(|mode| valid_permission_mode(mode))
                .unwrap_or("build")
                .into(),
            workflow_notice,
            workflow_enabled,
            child: None,
            side: None,
            workspace: json!({"workspaceKey":cwd.to_string_lossy(),"workspacePath":cwd.to_string_lossy()}),
        });
        let connection = connection_slot.as_mut().unwrap();
        connection.models.retain(|choice| {
            let provider = choice.selection["providerId"].as_str().unwrap_or("");
            !provider.starts_with("account:")
                || connection
                    .client
                    .account_host
                    .as_ref()
                    .is_some_and(|host| host.admits(provider))
        });
        if !connection
            .models
            .iter()
            .any(|choice| Some(choice.id.as_str()) == connection.selected_model_id.as_deref())
        {
            connection.selected_model_id = None;
            connection.client.selected_model = Value::Null;
        }
        let deadline = Instant::now() + CONTROL_TIMEOUT;
        while !connection.projection.initialized {
            if Instant::now() > deadline {
                bail!("ZCode Desktop 初始快照等待超时");
            }
            connection.poll(stopped, permission)?;
        }
        Ok(())
        })();
        if let Err(error) = outcome {
            if let Some(connection) = connection_slot.as_mut() {
                connection.client.stop_current_checked()?;
            } else if let Some(client) = client_slot.as_mut() {
                client.stop_current_checked()?;
            }
            return Err(error);
        }
        Ok(connection_slot.take().unwrap())
    }
    pub fn rows(&self) -> Vec<Value> {
        self.projection.rows.values().cloned().collect()
    }
    pub(crate) fn session_title(&self) -> Option<&str> {
        self.projection.session_title()
    }
    pub fn child_detail(&self) -> Option<Value> {
        self.child.as_ref().map(|child| json!({"childSessionId":child.session,"rows":child.projection.rows.values().collect::<Vec<_>>(),"loading":!child.projection.initialized,"error":child.error}))
    }
    pub fn open_child(&mut self, child_id: &str, owner: &str, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<()> {
        if !self.projection.rows.values().any(|row| row["kind"] == "subagent" && row["childSessionId"] == child_id) {
            bail!("子会话不属于当前 ZCode Desktop 对话");
        }
        if self.child.as_ref().is_some_and(|child| child.session == child_id && child.owner == owner && child.error.is_none()) { return Ok(()); }
        self.close_child(None, None, stopped, permission)?;
        let connection = uuid::Uuid::new_v4().to_string();
        let result = self.client.request("v4/conversation/subscribe", json!({"topic":format!("conversation/{child_id}"),"connectionId":connection,"clientMode":"desktop-continuous","workspace":self.workspace}), stopped, permission)?;
        let subscription = result.pointer("/ack/subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty()).context("子会话订阅缺少标识")?.to_owned();
        let epoch = result.pointer("/ack/logEpoch").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("").to_owned();
        let valid = !epoch.is_empty() && result.pointer("/ack/mode").and_then(Value::as_str) == Some("snapshot");
        self.child = Some(ChildConversation { owner: owner.into(), session: child_id.into(), subscription, connection, projection: Projection { epoch, ..Default::default() }, wire: Wire::default(), error: None });
        if !valid { self.close_child(None, None, stopped, permission).context("子会话快照确认无效，退订失败")?; bail!("子会话快照确认无效，已退订"); }
        Ok(())
    }
    pub fn close_child(&mut self, expected: Option<&str>, owner: Option<&str>, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<()> {
        if owner.is_some_and(|token| self.child.as_ref().is_none_or(|child| child.owner != token)) { return Ok(()); }
        if expected.is_some_and(|id| self.child.as_ref().is_none_or(|child| child.session != id)) { return Ok(()); }
        if let Some(child) = self.child.take() {
            if let Err(error) = self.client.request("v4/conversation/unsubscribe", json!({"topic":format!("conversation/{}",child.session),"subscriptionId":child.subscription,"connectionId":child.connection}), stopped, permission) {
                self.child = Some(child);
                return Err(error.context("子会话退订失败"));
            }
        }
        Ok(())
    }
    /// 订阅一条辅助会话流（subagent 展开与 side 分栏共用底层逻辑）。
    fn subscribe_conversation(&mut self, session_id: &str, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<(String, String, String)> {
        let connection = uuid::Uuid::new_v4().to_string();
        let result = self.client.request("v4/conversation/subscribe", json!({"topic":format!("conversation/{session_id}"),"connectionId":connection,"clientMode":"desktop-continuous","workspace":self.workspace}), stopped, permission)?;
        let subscription = result.pointer("/ack/subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty()).context("会话订阅缺少标识")?.to_owned();
        let epoch = result.pointer("/ack/logEpoch").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("").to_owned();
        let valid = !epoch.is_empty() && result.pointer("/ack/mode").and_then(Value::as_str) == Some("snapshot");
        if !valid {
            let _ = self.client.request("v4/conversation/unsubscribe", json!({"topic":format!("conversation/{session_id}"),"subscriptionId":subscription,"connectionId":connection}), stopped, permission);
            bail!("会话快照确认无效，已退订");
        }
        Ok((subscription, connection, epoch))
    }
    pub fn side_detail(&self) -> Option<Value> {
        self.side.as_ref().map(|side| json!({"childSessionId":side.session,"rows":side.projection.rows.values().collect::<Vec<_>>(),"loading":!side.projection.initialized,"error":side.error}))
    }
    /// /side|/btw：在父会话进程内派生辅助会话（原生 selection-side-session 同语义），
    /// 首条输入可选；随后订阅其对话流进独立 side 槽，不影响 subagent 展开槽。
    pub fn create_side_session(&mut self, first_input: Option<&str>, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<String> {
        let first_input = first_input.map(str::trim).filter(|value| !value.is_empty());
        if first_input.is_some_and(|text| text.chars().count() > 32000) { bail!("辅助对话消息最多 32000 字符"); }
        // The upstream owns the new ID until this ACK. Do not start a turn which
        // could request auth/permission before we know which identity to trust.
        let (_, ack) = self.client.command_id(
            "createSelectionSideSession", Some(&self.session_id), json!({}),
            &uuid::Uuid::new_v4().to_string(), None, stopped, permission,
        )?;
        let child_id = ack
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .context("createSelectionSideSession 未返回辅助会话标识")?
            .to_owned();
        if self.side.as_ref().is_some_and(|side| side.session == child_id) {
            if let Some(text) = first_input { self.side_send(text, stopped, permission)?; }
            return Ok(child_id);
        }
        self.close_side(stopped, permission)?;
        self.client.side_sessions.push(child_id.clone());
        let (subscription, connection, epoch) = match self.subscribe_conversation(&child_id, stopped, permission) {
            Ok(subscription) => subscription,
            Err(error) => {
                self.client.side_sessions.retain(|id| id != &child_id);
                return Err(error);
            }
        };
        self.side = Some(ChildConversation { owner: "side".into(), session: child_id.clone(), subscription, connection, projection: Projection { epoch, ..Default::default() }, wire: Wire::default(), error: None });
        if let Some(text) = first_input { self.side_send(text, stopped, permission)?; }
        Ok(child_id)
    }
    pub fn close_side(&mut self, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<()> {
        if let Some(side) = self.side.take() {
            let session = side.session.clone();
            if let Err(error) = self.client.request("v4/conversation/unsubscribe", json!({"topic":format!("conversation/{session}"),"subscriptionId":side.subscription,"connectionId":side.connection}), stopped, permission) {
                self.side = Some(side);
                return Err(error.context("辅助对话退订失败"));
            }
            self.client.side_sessions.retain(|id| *id != session);
        }
        Ok(())
    }
    /// 向已打开的辅助会话发送后续输入（原生 side 分栏底部输入框同语义）。
    pub fn side_send(&mut self, text: &str, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<()> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 32000 {
            bail!("辅助对话消息不能为空且最多 32000 字符");
        }
        let child_id = self.side.as_ref().map(|side| side.session.clone()).context("辅助对话未打开")?;
        let id = uuid::Uuid::new_v4().to_string();
        self.client.command_id("sendText", Some(&child_id), json!({"text":text,"attachments":[],"requestedDelivery":"startNow","mode":self.permission_mode}), &id, None, stopped, permission)?;
        // ACK accepted 即权威；等 child 流出现该 turn 或 revision 前进，仅为实现证新鲜度。
        if let Some(side) = self.side.as_ref() {
            let revision_before = side.projection.revision;
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let side = self.side.as_ref().context("辅助对话已关闭")?;
                let evidence = side.projection.rows.values().any(|row| row["kind"] == "turnHeader" && row["sourceCommandId"] == id)
                    || side.projection.revision != revision_before;
                if evidence || Instant::now() > deadline {
                    break;
                }
                self.poll(stopped, permission)?;
            }
        }
        Ok(())
    }
    pub fn queue(&self) -> Vec<Value> {
        self.projection
            .queue
            .as_ref()
            .and_then(|q| q["items"].as_array())
            .cloned()
            .unwrap_or_default()
    }
    pub fn queue_auto_drain(&self) -> bool {
        self.projection
            .queue
            .as_ref()
            .is_some_and(|q| q["autoDrain"] == true)
    }
    pub fn input_routing(&self) -> Option<String> {
        self.projection.input_routing.clone()
    }
    /// 当前上下文用量（None = 尚未收到 usage 事实）。
    pub fn context_usage(&self) -> Option<Value> {
        self.projection.context_usage.clone()
    }
    pub fn can_queue(&self) -> bool {
        self.projection.queue.is_some()
            && self.projection.control.is_some()
            && self.projection.input_routing.is_some()
    }
    pub fn native_busy(&self) -> Option<bool> {
        self.projection
            .control
            .as_ref()
            .map(|c| matches!(c["phase"].as_str(), Some("prewarming" | "running")))
    }
    pub fn queue_command(
        &mut self,
        kind: &str,
        payload: Value,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<()> {
        if !matches!(
            kind,
            "deleteQueueItem" | "setAutoDrain" | "sendQueuedNow" | "editQueueItem"
        ) {
            bail!("ZCode Desktop 队列命令无效");
        }
        let item_id = payload["queueItemId"].as_str().map(str::to_owned);
        let auto_drain = payload["autoDrain"].as_bool();
        // sendQueuedNow/editQueueItem 不改变队列成员资格（promote 成功会移除条目，
        // 但条目在被消费前必然先离开 queued 态；等待循环只针对删除/翻转语义）。
        let wait_membership = matches!(kind, "deleteQueueItem");
        let revision = self
            .projection
            .revision
            .context("ZCode Desktop 队列缺少权威状态版本，请重新加载会话")?;
        self.client.command_id(
            kind,
            Some(&self.session_id),
            payload,
            &uuid::Uuid::new_v4().to_string(),
            Some(revision),
            stopped,
            permission,
        )?;
        let deadline = Instant::now() + CONTROL_TIMEOUT;
        while wait_membership
            && item_id
                .as_ref()
                .is_some_and(|id| self.queue().iter().any(|item| item["queueItemId"] == *id))
            || auto_drain.is_some_and(|value| self.queue_auto_drain() != value)
        {
            if Instant::now() > deadline {
                bail!("ZCode Desktop 队列操作未返回权威状态，请重新加载会话核对");
            }
            self.poll(stopped, permission)?;
        }
        Ok(())
    }
    pub fn model_catalog_notice(&self) -> Option<String> {
        if !self.models.is_empty() {
            return None;
        }
        self.client
            .account_host
            .as_ref()
            .and_then(account_host::AccountHost::unavailable_notice)
            .or_else(|| Some("ZCode Desktop 当前没有可用模型，请重新加载模型；若仍为空，请检查原生 ZCode Desktop 的账号套餐或自定义 API 配置".into()))
    }
    pub fn take_auth_error(&mut self) -> Option<String> {
        self.client.auth_error.take()
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.client.stop(&self.session_id)
    }
    pub fn set_model(
        &mut self,
        id: &str,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<Value> {
        let selection = self
            .models
            .iter()
            .find(|choice| choice.id == id)
            .context("所选模型不在 ZCode Desktop 本地模型目录中")?
            .selection
            .clone();
        let result = self.client.request("session/setModel", json!({"sessionId":self.session_id,"model":selection,"persistAsWorkspaceLastUsed":false}), stopped, permission)?;
        let confirmed = result
            .pointer("/settings/model/current")
            .context("ZCode Desktop 模型更新未返回确认选择")?;
        if confirmed != &selection {
            bail!("ZCode Desktop 模型更新返回不同选择")
        }
        self.selected_model_id = Some(selection.to_string());
        self.client.selected_model = selection.clone();
        Ok(selection)
    }
    pub fn set_permission_mode(
        &mut self,
        mode: &str,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<()> {
        if !valid_permission_mode(mode) {
            bail!("ZCode Desktop 权限模式无效");
        }
        while !self.client.queued.is_empty() {
            self.poll(stopped, permission)?;
        }
        let previous_seq = self.projection.mode_revision;
        let result = self.client.request(
            "session/setMode",
            json!({"sessionId":self.session_id,"mode":mode}),
            stopped,
            permission,
        )?;
        let confirmed = result
            .pointer("/settings/mode/current")
            .and_then(Value::as_str);
        if confirmed != Some(mode) && mode != "plan" {
            bail!("ZCode Desktop 权限更新未确认所选模式");
        }
        // Consume config events queued before this RPC confirmation in order;
        // an older mode update must not undo the confirmed selection afterward.
        while !self.client.queued.is_empty() {
            self.poll(stopped, permission)?;
        }
        // New native versions expose Plan separately; wait for the authoritative
        // config event rather than treating a build/yolo response as Plan.
        if confirmed != Some(mode) {
            let deadline = Instant::now() + CONTROL_TIMEOUT;
            while !self.projection.confirms_mode_after(mode, previous_seq) {
                if Instant::now() > deadline {
                    bail!("ZCode Desktop 计划模式更新未确认");
                }
                self.poll(stopped, permission)?;
            }
        }
        self.permission_mode = mode.into();
        Ok(())
    }
    pub fn send_text(
        &mut self,
        text: &str,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<String> {
        self.send_input(text, &[], stopped, permission)
    }
    pub fn composer_catalog(
        &mut self,
        cwd: &str,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Value {
        let mut result = serde_json::Map::new();
        for (kind, method, field, session_owned) in [
            ("skills", "skills/referenceCatalog", "skills", true),
            ("plugins", "plugins/referenceCatalog", "plugins", true),
            (
                "commands",
                "workspace/readPresentation",
                "slashCommands",
                false,
            ),
        ] {
            let mut params = json!({"workspace":{"workspaceKey":cwd,"workspacePath":cwd}});
            if session_owned {
                params["sessionId"] = json!(self.session_id);
            }
            match self.client.request(method, params, stopped, permission) {
                Ok(value) if value[field].is_array() => {
                    let mut items = value[field].as_array().unwrap().clone();
                    if kind == "commands" && !self.workflow_enabled {
                        items.retain(|item| item["name"] != "workflow");
                    }
                    result.insert(kind.into(), json!({"items":items}));
                }
                Ok(_) => {
                    result.insert(
                        kind.into(),
                        json!({"items":[],"error":format!("ZCode Desktop {kind} 目录返回格式不兼容")}),
                    );
                }
                Err(error) => {
                    result.insert(kind.into(), json!({"items":[],"error":error.to_string()}));
                }
            }
        }
        if let Some(notice) = &self.workflow_notice {
            result.get_mut("commands").unwrap()["notice"] = json!(notice);
        }
        Value::Object(result)
    }
    pub fn send_input(
        &mut self,
        text: &str,
        attachments: &[Value],
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<String> {
        self.send_input_delivery(text, attachments, "startNow", stopped, permission)
    }
    pub fn send_input_delivery(
        &mut self,
        text: &str,
        attachments: &[Value],
        delivery: &str,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<String> {
        self.send_input_delivery_id(
            text,
            attachments,
            delivery,
            &uuid::Uuid::new_v4().to_string(),
            stopped,
            None,
            false,
            permission,
        )
    }
    pub fn send_input_delivery_id(
        &mut self,
        text: &str,
        attachments: &[Value],
        delivery: &str,
        command_id: &str,
        stopped: &AtomicBool,
        cancel: Option<&AtomicBool>,
        plan_enabled: bool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<String> {
        if !matches!(delivery, "startNow" | "queue") {
            bail!("ZCode Desktop 输入发送策略无效");
        }
        let mut payload = json!({"text":text,"attachments":attachments,"requestedDelivery":delivery,"mode":self.permission_mode});
        // /plan 快捷命令与原生一致：本次 Submission 显式声明 planEnabled，不改会话持久模式。
        if plan_enabled {
            payload["planEnabled"] = json!(true);
        }
        let (id, _) = self.client.command_id("sendText", Some(&self.session_id), payload, command_id, None, stopped, permission)?;
        if self.can_queue() {
            let deadline = Instant::now() + CONTROL_TIMEOUT;
            while !self
                .queue()
                .iter()
                .any(|item| item["sourceCommandId"] == id)
                && self.turn_state(&id).is_none()
            {
                if cancel.is_some_and(|flag| flag.swap(false, Ordering::AcqRel)) {
                    self.cancel(stopped, permission)?;
                }
                if Instant::now() > deadline {
                    bail!("ZCode Desktop 已接收输入但队列确认超时，请核对历史后再重发");
                }
                self.poll(stopped, permission)?;
            }
        }
        Ok(id)
    }
    pub fn cancel(
        &mut self,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<()> {
        self.client.command(
            "stop",
            Some(&self.session_id),
            json!({}),
            stopped,
            permission,
        )?;
        Ok(())
    }
    /// 编辑命中稳定实体，使用当前权威版本做 CAS；历史和文件处置由原生 runtime 决定。
    pub fn edit_user_query(&mut self, row_id: u64, entity_id: &str, text: &str, stopped: &AtomicBool, permission: &mut PermissionHandler<'_>) -> Result<String> {
        while !self.client.queued.is_empty() { self.poll(stopped, permission)?; }
        let row = self.projection.rows.get(&row_id).ok_or_else(|| CommandRejected::local("待编辑消息已不在当前历史中"))?;
        if row["kind"] != "userInput" || row["entityId"].as_str() != Some(entity_id) {
            return Err(CommandRejected::local("待编辑消息的实体标识不匹配，请刷新后重试").into());
        }
        let revision = self.projection.revision.ok_or_else(|| CommandRejected::local("ZCode Desktop 缺少权威版本，请刷新后重试"))?;
        let id = uuid::Uuid::new_v4().to_string();
        self.client.command_id_at_epoch("editUserQuery", Some(&self.session_id), json!({"target":{"rowId":row_id,"entityId":entity_id},"newText":text,"workspaceMode":"preserve"}), &id, Some(revision), Some(&self.projection.epoch), stopped, permission)?;
        Ok(id)
    }
    /// 原生桌面端在 UI 层消费的可见斜杠命令（packages/ui v4 slashCommands.ts 同语义）：
    /// /goal|/target → sendGoalCommand，/goal resume → resumeGoal，/compact|/compress → compact。
    /// 这些命令走类型化 v4/command 信封；其余 /xxx（技能、/init 等）仍按 sendText 由 runtime 解析。
    pub fn slash_command(
        &mut self,
        kind: &str,
        text: Option<&str>,
        display_text: Option<&str>,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<()> {
        let (command, payload, needs_revision) = match kind {
            "compact" => ("compact", json!({}), false),
            "sendGoalCommand" => {
                let text = text
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .context("/goal 需要目标文本")?;
                let mut payload = json!({"text": text, "mode": self.permission_mode});
                if let Some(display) = display_text.map(str::trim).filter(|value| !value.is_empty())
                {
                    payload["displayText"] = json!(display);
                }
                ("sendGoalCommand", payload, false)
            }
            "resumeGoal" => ("resumeGoal", json!({}), true),
            _ => bail!("ZCode Desktop 斜杠命令无效"),
        };
        let revision = if needs_revision {
            Some(
                self.projection
                    .revision
                    .context("ZCode Desktop 缺少权威状态版本，请重新加载会话")?,
            )
        } else {
            None
        };
        let id = uuid::Uuid::new_v4().to_string();
        self.client.command_id(
            command,
            Some(&self.session_id),
            payload,
            &id,
            revision,
            stopped,
            permission,
        )?;
        // ACK accepted 即权威结果；等待 turn/队列/revision 证据只为让紧随的快照反映目标状态。
        // plan 模式下的 /goal 只落库不续跑（原生 canContinue=false），revision 前进即代表落库完成。
        if self.can_queue() {
            let revision_before = self.projection.revision;
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.turn_state(&id).is_none()
                && !self.queue().iter().any(|item| item["sourceCommandId"] == id)
                && self.native_busy() != Some(true)
                && self.projection.revision == revision_before
            {
                if Instant::now() > deadline {
                    break;
                }
                self.poll(stopped, permission)?;
            }
        }
        Ok(())
    }
    pub fn turn_state(&self, command: &str) -> Option<&str> {
        self.projection
            .rows
            .values()
            .find(|row| row["kind"] == "turnHeader" && row["sourceCommandId"] == command)
            .and_then(|row| row["state"].as_str())
    }
    pub fn poll(
        &mut self,
        stopped: &AtomicBool,
        permission: &mut PermissionHandler<'_>,
    ) -> Result<bool> {
        if stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 会话已停止");
        }
        let value = if let Some(value) = self.client.queued.pop_front() {
            value
        } else {
            match self.client.receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(value) => value?,
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(false),
                Err(_) => bail!("ZCode Desktop Agent 已退出，原会话可以恢复"),
            }
        };
        if self.client.reverse(&value, stopped, permission)? {
            return Ok(false);
        }
        if value["method"] == "v4/conversation/frame" {
            let params = &value["params"];
            if params["subscriptionId"] != self.subscription {
                if let Some(child) = self.child.as_mut().filter(|child| params["subscriptionId"] == child.subscription) {
                    let outcome = (|| -> Result<()> { if let Some(frame) = child.wire.accept(params, &child.session, &child.subscription)? { child.projection.apply(&frame, &child.session, &child.subscription)?; } Ok(()) })();
                    if let Err(error) = outcome { child.error = Some(error.to_string()); }
                    return Ok(true);
                }
                if let Some(side) = self.side.as_mut().filter(|side| params["subscriptionId"] == side.subscription) {
                    let outcome = (|| -> Result<()> { if let Some(frame) = side.wire.accept(params, &side.session, &side.subscription)? { side.projection.apply(&frame, &side.session, &side.subscription)?; } Ok(()) })();
                    if let Err(error) = outcome { side.error = Some(error.to_string()); }
                    return Ok(true);
                }
                return Ok(false); // Retired child subscriptions may still have in-flight frames.
            }
            if let Some(frame) =
                self.wire
                    .accept(&value["params"], &self.session_id, &self.subscription)?
            {
                self.projection
                    .apply(&frame, &self.session_id, &self.subscription)?;
                if let Some(mode) = &self.projection.permission_mode {
                    self.permission_mode = mode.clone();
                }
                return Ok(true);
            }
        } else if value
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|m| m.contains("resync"))
        {
            if let Some(child) = self.child.as_mut().filter(|child| value["params"]["subscriptionId"] == child.subscription) {
                child.error = Some("子会话要求重新同步，请重新加载详情".into());
                return Ok(true);
            }
            if let Some(side) = self.side.as_mut().filter(|side| value["params"]["subscriptionId"] == side.subscription) {
                side.error = Some("辅助对话要求重新同步，请重新打开分栏".into());
                return Ok(true);
            }
            if value["params"]["subscriptionId"].as_str().is_some_and(|subscription| subscription != self.subscription) {
                return Ok(false);
            }
            // Retain the durable projection; reconnect with a full snapshot, never infer completion.
            bail!("ZCode Desktop 要求重同步，请恢复会话重新读取完整快照");
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_zcode_preflight_checks_protocol_and_redacts_startup_failure() {
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut working = Command::new("node");
        working.args(["-e", r#"
const lines = require('node:readline').createInterface({input: process.stdin});
lines.once('line', line => {
    const {id, method} = JSON.parse(line);
    if (method !== 'v4/commands/query') process.exit(2);
    process.stdout.write(JSON.stringify({id, result: {results: []}}) + '\n');
});
"#]);
        check_managed_cli_protocol(working, cwd).unwrap();

        let mut broken = Command::new("node");
        broken.args(["-e", r#"
const lines = require('node:readline').createInterface({input: process.stdin});
lines.once('line', () => {
    process.stderr.write('Cannot find module private-fixture-path (MODULE_NOT_FOUND)\n', () => process.exit(17));
});
"#]);
        let error = check_managed_cli_protocol(broken, cwd).unwrap_err();
        assert!(error.downcast_ref::<ProtocolClosed>().is_some());
        let message = format!("{error:#}");
        assert!(message.contains("ZCode CLI") && message.contains("17") && message.contains("找不到 CLI 入口"), "{message}");
        assert!(!message.contains("private-fixture-path"), "{message}");
    }

    #[test]
    fn managed_zcode_correlates_ack_terminal_and_reapplies_explicit_tools_on_resume() {
        // A minimal independent v4 peer checks both create and cold-resume payloads.
        let peer = r#"
const assert = require('node:assert/strict');
const { createInterface } = require('node:readline');
const sid = 'fixture-session', topic = 'conversation/' + sid, subscriptionId = 'sub';
const ref = {providerId:'personal-fixture',modelId:'model'};
const settings = {mode:{current:'build'},model:{current:ref,available:[{ref,label:'Fixture'}]}};
const scenario = process.env.SCENARIO, tools = JSON.parse(process.env.FIXTURE_TOOLS);
const emit = value => process.stdout.write(JSON.stringify(value) + '\n');
let seq = 0, command;
function frame(payload) {
  const before = seq++;
  emit({method:'v4/conversation/frame',params:{wireVersion:3,deliveryKind:before?'online':'initial',
    logicalFrameId:String(seq),logicalFrameOrdinal:seq,kind:'complete',topic,subscriptionId,
    frame:{topic,subscriptionId,fromSeq:before,toSeq:seq,payload}}});
}
function rows(items) { frame({kind:'deltas',deltas:items.map(row=>({op:'row.upserted',row}))}); }
createInterface({input:process.stdin}).on('line', input=>{
  const {id,method,params:p,result} = JSON.parse(input);
  const reply = result => emit({id,result});
  if (!method) {
    assert.equal(id,'perm'); assert.equal(result.decision,'deny');
    rows([{kind:'turnHeader',rowId:2,turnId:'this-turn',sourceCommandId:command,state:scenario==='failed'?'failed':scenario==='interrupted'?'completedInterrupted':'completedSuccess'},
      {kind:'assistantText',rowId:3,turnId:'this-turn',text:'reading',state:'complete'},
      {kind:'assistantText',rowId:4,turnId:'this-turn',text:'current final',state:scenario==='incomplete-body'?'streaming':'complete'}]); return;
  }
  if (method==='session/create'||method==='session/resume') {
    assert.deepEqual(p.toolAllowlist,tools); assert.deepEqual(p.mcpServers,[]);
    assert.equal(p.dynamicWorkflowEnabled,false);
    if(method==='session/resume'){assert.equal(p.sessionId,sid);reply({sessionId:scenario==='wrong-resume'?'other':sid,settings});}
    else {assert.equal(p.mode,'plan');assert.equal(p.titleGenerationEnabled,false);reply({session:{sessionId:sid},settings});}
  } else if(method==='session/close') reply({closed:true});
  else if(method==='v4/commands/query') reply({results:[]});
  else if(method==='v4/conversation/subscribe') {
    assert.equal(p.topic,topic); reply({ack:{mode:'snapshot',logEpoch:'epoch',subscriptionId}});
    frame({kind:'snapshot',snapshot:{sessionId:sid,logEpoch:'epoch',seq:1,rows:{window:[
      {kind:'assistantText',rowId:1,turnId:'old-turn',text:'old final',state:'complete'}]}}});
  } else if(method==='v4/command'){
    if(p.type==='stop'){reply({commandId:p.commandId,status:'accepted'});return;}
    assert.equal(p.type,'sendText'); assert.equal(p.payload.planEnabled,true); assert.equal(p.payload.mode,'build');
    command = p.commandId;
    reply({commandId:scenario==='wrong-ack'?'foreign':command,status:['noop','rejected'].includes(scenario)?scenario:'accepted'});
    if(!['noop','wrong-ack','rejected'].includes(scenario))
      emit({id:'perm',method:'interaction/requestPermission',params:{sessionId:sid,requestId:'fixture-request',toolName:'Bash',input:{}}});
  } else assert.fail(method);
});"#;
        for (scenario, text_only, resume, expected) in [
            ("ok", true, None, vec!["dispatched","accepted","completed"]),
            ("ok", false, Some("fixture-session"), vec!["dispatched","accepted","completed"]),
            ("noop", false, None, vec!["dispatched"]),
            ("wrong-ack", true, None, vec!["dispatched"]),
            ("rejected", false, None, vec!["dispatched","rejected"]),
            ("failed", false, None, vec!["dispatched","accepted","failed"]),
            ("interrupted", false, None, vec!["dispatched","accepted","interrupted"]),
            ("incomplete-body", false, None, vec!["dispatched","accepted","completed"]),
            ("wrong-resume", false, Some("fixture-session"), vec![]),
        ] {
            let mut fixture = Command::new("node");
            fixture.args(["-e", peer]).env("SCENARIO", scenario)
                .env("FIXTURE_TOOLS", if text_only { "[]" } else { r#"["Read","Glob","Grep"]"# });
            let mut events = Vec::new();
            let result = managed_turn_command(fixture, Path::new(env!("CARGO_MANIFEST_DIR")), "input", resume,
                Some("personal-fixture/model"), text_only, &AtomicBool::new(false), &mut |phase, session, command, turn| {
                    events.push((phase.to_owned(), session.to_owned(), command.to_owned(), turn.map(str::to_owned))); Ok(())
                });
            assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), expected, "{scenario}: {:?}",result.as_ref().err());
            if scenario == "ok" {
                let result = result.unwrap();
                assert_eq!(result.text,"current final");
                assert_eq!(result.turn_id,"this-turn");
                assert!(events.iter().all(|e| e.1 == result.session_id && e.2 == result.command_id));
                assert_eq!(events.last().unwrap().3.as_deref(),Some("this-turn"));
            } else {
                assert!(result.is_err(), "{scenario}");
            }
        }
    }

    #[test]
    #[ignore = "requires an installed and authenticated ZCode CLI and one live model call"]
    fn installed_zcode_cli_managed_review_reads_an_isolated_fixture() -> Result<()> {
        let cwd = std::env::temp_dir().join(format!("cv-zcode-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&cwd)?;
        let result = (|| {
            let value = uuid::Uuid::new_v4().simple().to_string();
            let source = format!("export const reviewEvidence = '{value}';\n");
            std::fs::write(cwd.join("race.mjs"), &source)?;
            let mut command = launch(&runtime_executable(super::super::AgentRuntimeKind::ZCodeCli)?)?;
            command.env("ZCODE_STORAGE_DIR", cwd.join("storage"));
            let result = execute_managed_review_command(
                command, &cwd,
                "请只用 Read 读取当前目录 race.mjs。最终只返回 reviewEvidence 的值。禁止写入或修改任何文件。",
                &AtomicBool::new(false),
            )?;
            ensure!(result.text.contains(&value), "真实审查未能读取隔离文件");
            ensure!(std::fs::read_to_string(cwd.join("race.mjs"))? == source, "审查修改了只读输入");
            Ok(())
        })();
        std::fs::remove_dir_all(&cwd)?;
        result
    }
    #[test]
    fn zcode_session_title_meta_merges_and_rejects_empty_default_values() {
        let mut projection = Projection::default();
        for meta in [json!({"title":"New Session","titleSource":"default"}),
            json!({"title":"","titleSource":"generated"}),
            json!({"title":"\n","titleSource":"custom"}),
            json!({"title":"bad\nname","titleSource":"generated"})] {
            projection.read_state(&json!({"meta":meta})).unwrap();
            assert_eq!(projection.session_title(), None);
        }
        projection.read_state(&json!({"meta":{"title":" 登录超时修复 ","titleSource":"generated"}})).unwrap();
        assert_eq!(projection.session_title(), Some("登录超时修复"));
        projection.read_state(&json!({"meta":{"title":"原生自定义标题"}})).unwrap();
        assert_eq!(projection.session_title(), Some("原生自定义标题"));
        projection.read_state(&json!({"meta":{"titleSource":"custom"}})).unwrap();
        projection.read_state(&json!({"revision":1})).unwrap();
        assert_eq!(projection.session_title(), Some("原生自定义标题"));
        projection.read_state(&json!({"meta":{"title":null}})).unwrap();
        assert_eq!(projection.session_title(), None);
    }

    #[test]
    fn zcode_session_title_snapshot_and_stale_frames_preserve_authority() {
        let mut projection = Projection { epoch: "epoch".into(), ..Default::default() };
        let snapshot = |seq, meta| json!({"topic":"conversation/main","subscriptionId":"main-sub","fromSeq":0,"toSeq":seq,
            "payload":{"kind":"snapshot","snapshot":{"sessionId":"main","logEpoch":"epoch","seq":seq,"meta":meta,"rows":{"window":[]}}}});
        projection.apply(&snapshot(3, json!({"title":"已生成标题","titleSource":"generated"})), "main", "main-sub").unwrap();
        assert_eq!(projection.session_title(), Some("已生成标题"));
        for (session, subscription) in [("child", "main-sub"), ("main", "old-sub")] {
            let mut frame = snapshot(4, json!({"title":"错误会话标题","titleSource":"generated"}));
            frame["topic"] = json!(format!("conversation/{session}"));
            frame["subscriptionId"] = json!(subscription);
            assert!(projection.apply(&frame, "main", "main-sub").is_err());
            assert_eq!(projection.session_title(), Some("已生成标题"));
        }
        assert!(projection.apply(&snapshot(2, json!({"title":"旧标题","titleSource":"generated"})), "main", "main-sub").is_err());
        assert_eq!(projection.session_title(), Some("已生成标题"));
        projection.apply(&snapshot(4, json!({})), "main", "main-sub").unwrap();
        assert_eq!(projection.session_title(), None);
        let delta = json!({"topic":"conversation/main","subscriptionId":"main-sub","fromSeq":4,"toSeq":5,
            "payload":{"kind":"deltas","deltas":[{"op":"state.updated","patch":{"meta":{"title":"增量摘要","titleSource":"generated"}}}]}});
        projection.apply(&delta, "main", "main-sub").unwrap();
        assert_eq!(projection.session_title(), Some("增量摘要"));
    }
    #[cfg(windows)]
    #[test]
    fn zcode_registry_paths_and_desktop_validation() {
        assert_eq!(
            registered_file(r#""D:\custom path\uninstallerIcon.ico",0"#),
            Some(PathBuf::from(r"D:\custom path\uninstallerIcon.ico"))
        );
        assert_eq!(
            registered_file(r#""D:\custom path\Uninstall ZCode.exe" /currentuser"#),
            Some(PathBuf::from(r"D:\custom path\Uninstall ZCode.exe"))
        );
        assert_eq!(
            registered_file(r"D:\custom path\uninstallerIcon.ico,0"),
            Some(PathBuf::from(r"D:\custom path\uninstallerIcon.ico"))
        );
        assert!(registered_file("relative/icon.ico").is_none());
        let root = std::env::temp_dir().join(format!("zcode-discovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("resources/glm")).unwrap();
        std::fs::write(root.join("resources/glm/zcode.cjs"), "").unwrap();
        assert!(desktop_agent(&root).is_none());
        std::fs::write(root.join("ZCode.exe"), "").unwrap();
        assert_eq!(
            desktop_agent(&root),
            Some(root.join("resources/glm/zcode.cjs"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn zcode_model_catalog_preserves_reasoning_and_structured_identity() {
        let choices = model_choices(
            &json!({"model":{"available":[{"ref":{"providerId":"personal/a","modelId":"model/b"},"label":"Model","reasoning":{"levels":[{"value":"low"},{"value":"max"}],"defaultLevel":"max"}},{"ref":{"providerId":"account:official","modelId":"model"}},{"ref":{"providerId":"personal","modelId":"disabled"},"disabledReason":"unavailable"},{"label":"unknown"}]}}),
        );
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0].selection["providerId"], "personal/a");
        assert_eq!(choices[0].selection["modelId"], "model/b");
        assert_eq!(choices[0].selection["options"]["reasoningLevel"], "low");
        assert_eq!(choices[1].selection["options"]["reasoningLevel"], "max");
        for choice in choices {
            assert_eq!(
                serde_json::from_str::<Value>(&choice.id).unwrap(),
                choice.selection
            );
        }
    }
    #[test]
    fn zcode_model_catalog_accepts_null_disabled_reason() {
        let choices = model_choices(&json!({"model":{"available":[
            {"ref":{"providerId":"personal-fixture","modelId":"enabled"},"disabledReason":null},
            {"ref":{"providerId":"personal-fixture","modelId":"blocked"},"disabledReason":"unavailable"},
            {"ref":{"providerId":"account:official","modelId":"account-model"},"disabledReason":null}
        ]}}));
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].selection["modelId"], "enabled");
    }
    #[test]
    fn zcode_permission_mode_values_and_independent_plan_projection() {
        assert!(["plan", "build", "edit", "yolo"]
            .iter()
            .all(|mode| valid_permission_mode(mode)));
        assert!(!valid_permission_mode("auto"));
        let mut projection = Projection::default();
        projection
            .read_mode(&json!({"mode":"yolo","planEnabled":true}))
            .unwrap();
        assert_eq!(projection.permission_mode.as_deref(), Some("plan"));
        projection.mode_revision = 4;
        assert!(!projection.confirms_mode_after("plan", 4));
        projection.mode_revision = 5;
        assert!(projection.confirms_mode_after("plan", 4));
        projection
            .read_mode(&json!({"mode":"build","planEnabled":false}))
            .unwrap();
        assert_eq!(projection.permission_mode.as_deref(), Some("build"));
        assert!(projection.read_mode(&json!({"mode":"unknown"})).is_err());
    }
    #[test]
    #[ignore = "已安装 ZCode Desktop 隔离控制面权限切换，不发模型消息"]
    fn zcode_installed_permission_modes_control_only() {
        let root =
            std::env::temp_dir().join(format!("zcode-permission-control-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let outcome = (|| -> Result<()> {
            let mut command = launch(&executable()?)?;
            command
                .env("ZCODE_STORAGE_DIR", root.join("storage"))
                .env("ZCODE_DATA_BASE_DIR", &root);
            let stopped = AtomicBool::new(false);
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let mut progress = || Ok(());
            let mut connection = DesktopConnection::open(
                command,
                &root,
                None,
                true,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            for mode in ["plan", "build", "edit", "yolo", "build"] {
                connection.set_permission_mode(mode, &stopped, &mut deny)?;
                assert_eq!(connection.permission_mode, mode);
                for _ in 0..4 {
                    connection.poll(&stopped, &mut deny)?;
                }
                assert_eq!(
                    connection.permission_mode, mode,
                    "queued config reverted the confirmed mode"
                );
            }
            assert!(connection.rows().is_empty());
            assert!(connection.can_queue());
            assert_eq!(connection.native_busy(), Some(false));
            assert!(connection.queue().is_empty());
            for auto_drain in [false, true] {
                connection.queue_command(
                    "setAutoDrain",
                    json!({"autoDrain":auto_drain}),
                    &stopped,
                    &mut deny,
                )?;
                assert_eq!(connection.queue_auto_drain(), auto_drain);
            }
            assert!(connection.rows().is_empty());
            connection.shutdown().unwrap();
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        outcome.unwrap();
    }
    #[test]
    #[ignore = "需要已安装 ZCode Desktop，仅隔离空会话控制面，不调用模型"]
    fn zcode_desktop_empty_session_persists_without_model() {
        let root =
            std::env::temp_dir().join(format!("zcode-desktop-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let result = (|| -> Result<()> {
            let path = executable()?;
            let command = || -> Result<Command> {
                let mut c = launch(&path)?;
                c.env("ZCODE_STORAGE_DIR", root.join("storage"))
                    .env("ZCODE_DATA_BASE_DIR", &root)
                    .env(
                        "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
                        root.join("personal-empty.json"),
                    );
                Ok(c)
            };
            let stopped = AtomicBool::new(false);
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let mut progress = || Ok(());
            let mut connection = DesktopConnection::open(
                command()?,
                &root,
                None,
                true,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            let id = connection.session_id.clone();
            assert!(connection.rows().is_empty());
            // No model selection or personal config: admission must fail before
            // invoking a model. Inspect only the known protocol diagnostic.
            let rejected = connection.client.request(
                "v4/command",
                json!({"commandId":uuid::Uuid::new_v4().to_string(),"clientId":"crewvela","sessionId":id,"type":"sendText","payload":{"text":"isolated admission check","requestedDelivery":"startNow","mode":"build"},"issuedAt":chrono::Utc::now().timestamp_millis()}),
                &stopped,
                &mut deny,
            )?;
            assert_eq!(rejected["reasonCode"], "fault.command.executionFailed");
            assert!(
                rejected.get("message").and_then(Value::as_str).is_some_and(
                    |message| message.starts_with("Session model must be provider-qualified:")
                ),
                "isolated no-model admission diagnostic fields: {:?}",
                rejected.as_object().unwrap().keys().collect::<Vec<_>>()
            );
            connection.shutdown().unwrap();
            drop(connection);
            // An isolated local-only provider exercises the installed catalog
            // and model binding without sending any model request or reading
            // the user's Personal config. The credential is a synthetic value.
            std::fs::write(
                root.join("personal-empty.json"),
                serde_json::to_vec(
                    &json!({"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[{"providerId":"crewvela-offline","providerName":"Offline control fixture","enabled":true,"config":{"group":"standard-personal","access":{"type":"api-key","apiKey":"not-a-real-credential"},"api":{"type":"openai-chat-completions","baseUrl":"http://127.0.0.1:1/v1"},"personalModelIds":["gpt-4.1"]}}]},"modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[]}}}),
                )?,
            )?;
            let mut connection = DesktopConnection::open(
                command()?,
                &root,
                Some(&id),
                true,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            assert!(!connection.session_id.is_empty());
            assert!(connection.rows().is_empty());
            let chosen = connection
                .models
                .iter()
                .find(|choice| choice.selection["providerId"] == "crewvela-offline")
                .context("安装版未返回隔离 Personal 模型目录")?
                .id
                .clone();
            connection.set_model(&chosen, &stopped, &mut deny)?;
            assert_eq!(
                connection.selected_model_id.as_deref(),
                Some(chosen.as_str())
            );
            connection.shutdown().unwrap();
            drop(connection);
            Ok(())
        })();
        let mut cleanup = std::fs::remove_dir_all(&root);
        for _ in 0..10 {
            if !cleanup
                .as_ref()
                .is_err_and(|e| e.raw_os_error() == Some(32))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            cleanup = std::fs::remove_dir_all(&root);
        }
        result.unwrap();
        cleanup.unwrap();
    }
    #[test]
    #[ignore = "读取原生登录并GET账号权益，仅目录/绑定，不调用模型"]
    fn zcode_installed_account_catalog_and_binding() {
        installed_account_check(false, false, "account:bigmodel-individual-coding-plan");
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "需要已安装 ZCode Desktop，隔离 PATH 发现测试，不调用模型"]
    fn zcode_installed_cli_and_desktop_resolve_independently() {
        assert!(std::env::var_os("CREWVELA_ZCODE_EXECUTABLE").is_none());
        assert!(std::env::var_os("CREWVELA_ZCODE_ROOT").is_none());
        // 开发 shell 可能继承 zcode CLI 注入的 provider env；launch 的显式
        // 透传契约会跳过注入，隔离后才能验证 launch 自身的注入结果。
        let inherited: Vec<(&str, Option<std::ffi::OsString>)> = [
            "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE",
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
            "CREWVELA_ZCODE_APP_VERSION",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        for (key, _) in &inherited {
            std::env::remove_var(key);
        }
        let expected = installed_desktop_agent().expect("需要已安装 ZCode Desktop");
        // 场景一：不可解析的裸 shim 回退桌面安装。
        let root = std::env::temp_dir().join(format!("zcode-path-shim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let shim = root.join("zcode.cmd");
        std::fs::write(&shim, "@echo off\r\nexit /b 91\r\n").unwrap();
        let explicit_agent = root.join("resources/glm/zcode.cjs");
        std::fs::create_dir_all(explicit_agent.parent().unwrap()).unwrap();
        std::fs::write(&explicit_agent, "// isolated discovery fixture").unwrap();
        let previous = std::env::var_os("PATH");
        let paths = std::env::join_paths(
            std::iter::once(root.clone())
                .chain(std::env::split_paths(&previous.clone().unwrap_or_default())),
        )
        .unwrap();
        std::env::set_var("PATH", paths);
        let path_match = which::which("zcode");
        let discovered = executable();
        std::env::set_var("CREWVELA_ZCODE_EXECUTABLE", &shim);
        let explicit_executable = executable();
        std::env::remove_var("CREWVELA_ZCODE_EXECUTABLE");
        std::env::set_var("CREWVELA_ZCODE_ROOT", &root);
        let explicit_root = executable();
        std::env::remove_var("CREWVELA_ZCODE_ROOT");
        let catalog = std::panic::catch_unwind(|| {
            let command = launch(discovered.as_ref().unwrap()).unwrap();
            for key in [
                "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE",
                "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
            ] {
                assert!(command
                    .get_envs()
                    .any(|(name, value)| name == key && value.is_some()));
            }
            assert!(account_host::AccountHost::prepare(&command)
                .unwrap()
                .is_some());
        });
        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(path_match.unwrap(), shim);
        assert_eq!(discovered.unwrap(), expected);
        assert_eq!(explicit_executable.unwrap(), shim);
        assert_eq!(explicit_root.unwrap(), explicit_agent);
        catalog.unwrap();
        // 场景二：可解析 shim 指向真实 cjs 时，CLI runtime 解析出该入口并从
        // CLI 布局定位 builtin/版本；Desktop runtime 不受 PATH 影响。
        let cli_root =
            std::env::temp_dir().join(format!("zcode-path-cli-{}", uuid::Uuid::new_v4()));
        let dist = cli_root.join("apps/zcode-cli/packages/cli/dist");
        std::fs::create_dir_all(dist.join("provider")).unwrap();
        std::fs::write(dist.join("zcode.cjs"), "// isolated cli fixture").unwrap();
        std::fs::write(
            dist.join("provider/zcode-builtin.json"),
            "{\"revision\":0,\"config\":{}}",
        )
        .unwrap();
        std::fs::write(
            dist.parent().unwrap().join("package.json"),
            "{\"version\":\"0.0.0-fixture\"}",
        )
        .unwrap();
        let shim_dir = cli_root.join("bin");
        std::fs::create_dir_all(&shim_dir).unwrap();
        let entry = dist.join("zcode.cjs");
        std::fs::write(
            shim_dir.join("zcode.cmd"),
            format!("@echo off\r\nnode \"{}\" %*\r\n", entry.display()),
        )
        .unwrap();
        let previous = std::env::var_os("PATH");
        let paths = std::env::join_paths(
            std::iter::once(shim_dir.clone())
                .chain(std::env::split_paths(&previous.clone().unwrap_or_default())),
        )
        .unwrap();
        std::env::set_var("PATH", paths);
        let resolved_cli = cli_executable();
        let resolved_desktop = executable();
        match previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        // CLI runtime 解析 shim 指向的真实 cjs；Desktop runtime 不受 PATH 影响。
        assert_eq!(resolved_cli.unwrap(), entry);
        assert_eq!(resolved_desktop.unwrap(), expected);
        let command = launch(&entry).unwrap();
        assert_eq!(
            env_value(&command, "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").as_deref(),
            Some(dist.join("provider/zcode-builtin.json").to_str().unwrap())
        );
        assert!(env_value(&command, "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE").is_some());
        assert_eq!(
            env_value(&command, "CREWVELA_ZCODE_APP_VERSION").as_deref(),
            Some("0.0.0-fixture")
        );
        for (key, value) in inherited {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        std::fs::remove_dir_all(&cli_root).unwrap();
    }
    fn env_value(command: &Command, key: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(name, _)| *name == key)
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }
    #[test]
    fn zcode_shim_resolution_extracts_existing_cjs_entries() {
        let root = std::env::temp_dir().join(format!("zcode-shim-parse-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("dist")).unwrap();
        let entry = root.join("dist/zcode.cjs");
        std::fs::write(&entry, "// fixture").unwrap();
        // npm 风格 cmd shim：%~dp0 相对引用包内 cjs。
        let npm = root.join("zcode.cmd");
        std::fs::write(&npm, "@echo off\r\nnode \"%~dp0\\dist\\zcode.cjs\" %*\r\n").unwrap();
        assert_eq!(resolve_shim_entry(&npm).as_deref(), Some(entry.as_path()));
        // Source sh shims use MSYS drive paths on Windows, native paths on Unix.
        #[cfg(windows)]
        fn msys_style(path: &Path) -> String {
            let text = path.to_string_lossy();
            let mut components = text.split(['\\', '/']);
            let drive = components
                .next()
                .unwrap()
                .trim_end_matches(':')
                .to_ascii_lowercase();
            let rest = components.fold(String::new(), |acc, part| format!("{acc}/{part}"));
            format!("/{drive}{rest}")
        }
        #[cfg(not(windows))]
        fn msys_style(path: &Path) -> String {
            path.to_string_lossy().into_owned()
        }
        let standalone = root.join("dist/standalone.cjs");
        std::fs::write(&standalone, "// fixture").unwrap();
        let sh = root.join("zcode");
        std::fs::write(
            &sh,
            format!("#!/bin/sh\nexec node {} \"$@\"\n", msys_style(&standalone)),
        )
        .unwrap();
        assert_eq!(resolve_shim_entry(&sh).as_deref(), Some(standalone.as_path()));
        // 无 cjs 引用或指向不存在文件的 shim 不解析。
        let garbage = root.join("zcode-garbage.cmd");
        std::fs::write(&garbage, "@echo off\r\nexit /b 91\r\n").unwrap();
        assert_eq!(resolve_shim_entry(&garbage), None);
        let stale = root.join("zcode-stale.cmd");
        std::fs::write(
            &stale,
            "@echo off\r\nnode \"C:\\definitely-missing\\zcode.cjs\" %*\r\n",
        )
        .unwrap();
        assert_eq!(resolve_shim_entry(&stale), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shim_path_expansion_keeps_native_roots_and_platform_separators() {
        #[cfg(windows)]
        {
            assert_eq!(expand_shim_path("/c/tools/zcode.cjs", Path::new(r"C:\bin")), Some(PathBuf::from(r"C:\tools\zcode.cjs")));
            assert_eq!(expand_shim_path(r"%~dp0\dist/zcode.cjs", Path::new(r"C:\bin")), Some(PathBuf::from(r"C:\bin\dist\zcode.cjs")));
        }
        #[cfg(not(windows))]
        {
            assert_eq!(expand_shim_path("/c/tools/zcode.cjs", Path::new("/fixture/bin")), Some(PathBuf::from("/c/tools/zcode.cjs")));
            assert_eq!(expand_shim_path(r"%~dp0\dist/zcode.cjs", Path::new("/fixture/bin")), Some(PathBuf::from("/fixture/bin/dist/zcode.cjs")));
        }
    }
    #[test]
    fn zcode_cli_bundled_builtin_mirrors_cli_candidates() {
        let root = std::env::temp_dir().join(format!("zcode-cli-builtin-{}", uuid::Uuid::new_v4()));
        // npm/打包布局：入口同目录 provider/。
        let dist = root.join("package/dist");
        std::fs::create_dir_all(dist.join("provider")).unwrap();
        let entry = dist.join("zcode.cjs");
        std::fs::write(&entry, "// fixture").unwrap();
        let adjacent = dist.join("provider/zcode-builtin.json");
        std::fs::write(&adjacent, "{\"revision\":1}").unwrap();
        assert_eq!(cli_bundled_builtin(&entry).as_deref(), Some(adjacent.as_path()));
        // 源码仓库布局：dist 向上 5 级的 config/provider/。
        let repo = root.join("repo/apps/zcode-cli/packages/cli/dist");
        std::fs::create_dir_all(&repo).unwrap();
        let repo_entry = repo.join("zcode.cjs");
        std::fs::write(&repo_entry, "// fixture").unwrap();
        let repo_config = root.join("repo/config/provider/zcode-builtin.json");
        std::fs::create_dir_all(repo_config.parent().unwrap()).unwrap();
        std::fs::write(&repo_config, "{\"revision\":2}").unwrap();
        assert_eq!(
            cli_bundled_builtin(&repo_entry).as_deref(),
            Some(repo_config.as_path())
        );
        // 非 js 入口不参与 bundled 定位。
        assert_eq!(cli_bundled_builtin(&root.join("zcode.exe")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn zcode_launch_pins_cli_builtin_personal_and_version() {
        let root = std::env::temp_dir().join(format!("zcode-cli-launch-{}", uuid::Uuid::new_v4()));
        let dist = root.join("apps/zcode-cli/packages/cli/dist");
        std::fs::create_dir_all(dist.join("provider")).unwrap();
        let entry = dist.join("zcode.cjs");
        std::fs::write(&entry, "// fixture").unwrap();
        let builtin = dist.join("provider/zcode-builtin.json");
        std::fs::write(&builtin, "{\"revision\":3,\"config\":{}}").unwrap();
        std::fs::write(
            dist.parent().unwrap().join("package.json"),
            "{\"version\":\"7.7.7-fixture\"}",
        )
        .unwrap();
        // 开发 shell 可能继承 zcode CLI 注入的 provider 环境变量；launch 的
        // 显式透传契约意味着它们优先，测试需先清空再恢复。
        let previous: Vec<(&str, Option<std::ffi::OsString>)> = [
            "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE",
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
            "CREWVELA_ZCODE_APP_VERSION",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        for (key, _) in &previous {
            std::env::remove_var(key);
        }
        let outcome = std::panic::catch_unwind(|| {
            let command = launch(&entry).unwrap();
            (
                env_value(&command, "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE"),
                env_value(&command, "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE"),
                env_value(&command, "CREWVELA_ZCODE_APP_VERSION"),
                command
                    .get_args()
                    .map(|argument| argument.to_string_lossy().into_owned())
                    .collect::<Vec<String>>(),
            )
        });
        for (key, value) in previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let (pinned_builtin, personal, version, arguments) = outcome.unwrap();
        assert_eq!(pinned_builtin.as_deref(), Some(builtin.to_str().unwrap()));
        assert!(personal.is_some());
        assert_eq!(version.as_deref(), Some("7.7.7-fixture"));
        assert_eq!(
            arguments[..arguments.len() - 2],
            [entry.to_string_lossy().into_owned()][..]
        );
        assert_eq!(&arguments[arguments.len() - 2..], ["app-server", "--stdio"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn zcode_tui_filters_only_the_crashing_windows_compatibility_layer() {
        let mut command = Command::new("zcode.cmd");
        configure_tui_compatibility(&mut command, Some("DetectorsAppHealth"));
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "__COMPAT_LAYER" && value.is_none()));

        let mut command = Command::new("zcode.cmd");
        configure_tui_compatibility(
            &mut command,
            Some("WIN8RTM detectorsapphealth HIGHDPIAWARE"),
        );
        assert_eq!(
            env_value(&command, "__COMPAT_LAYER").as_deref(),
            Some("WIN8RTM HIGHDPIAWARE")
        );

        let mut command = Command::new("zcode.cmd");
        configure_tui_compatibility(&mut command, Some("WIN8RTM"));
        assert_eq!(command.get_envs().count(), 0);
    }
    #[test]
    fn zcode_cli_tui_command_runs_literal_script_entry_with_node() {
        let root = std::env::temp_dir().join(format!("zcode tui 中文 & (literal) % ! {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("zcode #entry.cjs");
        std::fs::write(&entry, "console.log(JSON.stringify({main:require.main===module,entry:__filename,args:process.argv.slice(2),text:'中文🙂'}));").unwrap();
        let mut command = cli_script_tui_command(&entry).unwrap();
        assert_eq!(PathBuf::from(command.get_program()), which::which("node").unwrap());
        assert_eq!(command.get_args().last().unwrap(), entry.as_os_str());
        let literal = "literal & | <> ^ %PATH% !value! (text) 中文";
        let output = command.arg(literal).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["main"], true);
        assert_eq!(
            Path::new(result["entry"].as_str().unwrap()).canonicalize().unwrap(),
            entry.canonicalize().unwrap()
        );
        assert_eq!(result["args"], json!([literal]));
        assert_eq!(result["text"], "中文🙂");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn zcode_cli_script_uses_utf8_console_in_a_real_pty() {
        let root = std::env::temp_dir().join(format!("zcode pty 中文 & {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("zcode.cjs");
        std::fs::write(&entry, "const r=require('node:child_process').spawnSync('chcp.com',[],{encoding:'utf8'});console.log('UTF8_CLI_FIXTURE 中文🙂 '+r.stdout);").unwrap();
        let command = cli_script_tui_command(&entry).unwrap();
        let manager = crate::pty::PtyManager::new();
        let id = manager.open_agent_program(&command, "zcode-utf8-console-fixture", false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        while Instant::now() < deadline {
            if let Some(chunk) = manager.poll_all().remove(&id) { output.push_str(&chunk.data); }
            if output.contains("UTF8_CLI_FIXTURE") && output.contains("65001") { break; }
            std::thread::sleep(Duration::from_millis(20));
        }
        manager.kill(id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(output.contains("UTF8_CLI_FIXTURE 中文🙂"), "{output}");
        assert!(output.contains("65001"), "{output}");
    }

    const SIDE_RPC_FIXTURE: &str = r#"
        (async()=>{
        const assert = require('node:assert/strict');
        const readline = require('node:readline');
        const scenario = process.env.SIDE_RPC_SCENARIO;
        const child = 'side-fixture';
        const subscription = 'side-fixture-sub';
        const pending = new Map();
        let serial = 0, ordinal = 0, subscribed = false;
        const emit = value => process.stdout.write(JSON.stringify(value)+'\n');
        const reply = (id,result) => emit({id,result});
        function reverse(request, stage, method='interaction/requestPermission') {
            const id = `reverse-${++serial}`;
            pending.set(id,{request,stage,method});
            emit({id,method,params:{sessionId:scenario==='wrong-session'?'foreign-session':child,requestId:`permission-${serial}`,toolName:'Bash',input:{command:'fixture-only'}}});
        }
        function frame(commandId) {
            const topic = `conversation/${child}`, order = ++ordinal;
            const rows = commandId ? [{rowId:1,kind:'turnHeader',turnId:'turn',state:'completedSuccess',sourceCommandId:commandId},{rowId:2,kind:'assistantText',turnId:'turn',text:'side answer',state:'complete'}] : [];
            emit({method:'v4/conversation/frame',params:{wireVersion:3,deliveryKind:order===1?'initial':'online',logicalFrameId:`side-${order}`,logicalFrameOrdinal:order,kind:'complete',topic,subscriptionId:subscription,frame:{topic,subscriptionId:subscription,fromSeq:0,toSeq:order,payload:{kind:'snapshot',snapshot:{sessionId:child,logEpoch:'side-epoch',seq:order,revision:order,rows:{window:rows}}}}}});
        }
        for await (const line of readline.createInterface({input:process.stdin})) {
            const value = JSON.parse(line), p = value.params;
            if (!value.method) {
                const operation = pending.get(value.id); assert.ok(operation); pending.delete(value.id);
                if (scenario==='wrong-session') { assert.equal(value.error.code,-32602); continue; }
                assert.equal(value.error,undefined,'a trusted side request must not be rejected by identity');
                if (operation.method==='interaction/requestProviderRuntimeHeaders') {
                    assert.equal(value.result.headersApplied,false); assert.ok(value.result.errorMessage);
                    reverse(operation.request,'subscribe'); continue;
                }
                assert.equal(value.result.decision,'allow');
                const request=operation.request;
                if (operation.stage==='subscribe') {
                    subscribed = true;
                    reply(request.id,{ack:{subscriptionId:subscription,mode:scenario==='bad-ack'?'online':'snapshot',logEpoch:'side-epoch'}});
                    if (scenario!=='bad-ack') frame();
                } else if (operation.stage==='send') {
                    reply(request.id,{commandId:request.params.commandId,status:'accepted'}); frame(request.params.commandId);
                } else { assert.equal(operation.stage,'unsubscribe'); reply(request.id,{}); }
                continue;
            }
            if (value.method==='v4/command' && p.type==='createSelectionSideSession') {
                assert.equal(p.sessionId,'parent-fixture');
                assert.equal(p.payload.firstInput,undefined,'creation cannot execute before its child ID is trusted');
                reply(value.id,{commandId:p.commandId,status:'accepted',result:{sessionId:child}});
            } else if (value.method==='v4/conversation/subscribe') {
                assert.equal(p.topic,`conversation/${child}`);
                reverse(value,'subscribe',scenario==='auth-during-subscribe'?'interaction/requestProviderRuntimeHeaders':'interaction/requestPermission');
            } else if (value.method==='v4/command' && p.type==='sendText') {
                assert.ok(subscribed); assert.equal(p.sessionId,child); assert.equal(p.payload.text,'first 中文');
                reverse(value,'send');
            } else if (value.method==='v4/conversation/unsubscribe') {
                assert.equal(p.subscriptionId,subscription); reverse(value,'unsubscribe');
            } else { throw new Error(`unexpected method ${value.method}`); }
        }
        })().catch(error=>{console.error(error);process.exitCode=1;});
    "#;

    #[test]
    fn workspace_credential_cipher_unavailable_does_not_disconnect() {
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", r#"
            const rl=require('node:readline').createInterface({input:process.stdin});
            const assert=require('node:assert/strict');
            let original;
            rl.on('line',line=>{
                const frame=JSON.parse(line);
                if(frame.method==='fixture/open') {
                    original=frame.id;
                    console.log(JSON.stringify({id:'credential-rpc',method:'interaction/requestCredentialCipher',params:{requestId:'1db0b641-d36f-4a55-83ce-962fc4bb96c0',operation:'decrypt',value:'enc:v2:os:fixture'}}));
                } else {
                    assert.equal(frame.id,'credential-rpc');
                    assert.equal(frame.error,undefined);
                    assert.equal(frame.result.requestId,'1db0b641-d36f-4a55-83ce-962fc4bb96c0');
                    assert.equal(frame.result.result.ok,false);
                    assert.equal(frame.result.result.error.code,'CREDENTIAL_SECURE_STORAGE_UNAVAILABLE');
                    console.log(JSON.stringify({id:original,result:{connected:true}}));
                }
            });
        "#]);
        let mut progress = || Ok(());
        let mut client = Client::spawn_with_account_host(command, Path::new(env!("CARGO_MANIFEST_DIR")), &mut progress, None, "ZCode Desktop Agent").unwrap();
        assert!(client.session.is_none(), "workspace cipher may be requested before session creation");
        let result = client.request("fixture/open", json!({}), &AtomicBool::new(false), &mut |_, _| panic!("cipher request must never approve a tool"));
        assert_eq!(result.unwrap()["connected"], true);
        client.cleanup_checked().unwrap();
    }

    #[test]
    fn workspace_credential_cipher_roundtrips_through_private_host_before_session_creation() {
        let root = std::env::temp_dir().join(format!("zcode-cipher-rpc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let builtin = root.join("builtin.json");
        std::fs::write(&builtin, r#"{"revision":1,"config":{"providerConfigRules":{"providerRules":[]}}}"#).unwrap();
        let mut command = Command::new(which::which("node").unwrap());
        command.env("ZCODE_DATA_BASE_DIR", &root)
            .env("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE", &builtin)
            .env("ZCODE_CREDENTIAL_SECRET", "synthetic-cipher-test-secret")
            .env_remove("ZCODE_CREDENTIAL_SECRET_FILE")
            .args(["-e", r#"
                const assert=require('node:assert/strict');
                const emit=frame=>console.log(JSON.stringify(frame));
                const requestId='1db0b641-d36f-4a55-83ce-962fc4bb96c0';
                const secret='  synthetic private fixture 中文\n ';
                let original,stage=0;
                require('node:readline').createInterface({input:process.stdin}).on('line',line=>{
                    const frame=JSON.parse(line);
                    if(frame.method==='fixture/open'){
                        original=frame.id;
                        emit({id:'cipher-encrypt',method:'interaction/requestCredentialCipher',params:{requestId,operation:'encrypt',value:secret}});
                    }else{
                        assert.equal(frame.error,undefined);assert.equal(frame.result.requestId,requestId);
                        assert.equal(frame.result.result.ok,true);
                        if(stage++===0){
                            const value=frame.result.result.value;assert.ok(value.startsWith('enc:v2:env:'));
                            emit({id:'cipher-decrypt',method:'interaction/requestCredentialCipher',params:{requestId,operation:'decrypt',value}});
                        }else{
                            assert.equal(frame.result.result.value,secret);
                            emit({id:original,result:{privateRoundtrip:true}});
                        }
                    }
                });
            "#]);
        let mut progress = || Ok(());
        let mut client = Client::spawn(command, &root, &mut progress).unwrap();
        assert!(client.session.is_none());
        let result = client.request("fixture/open", json!({}), &AtomicBool::new(false), &mut |_, _| panic!("cipher must not approve tools"));
        let cleanup = client.cleanup_checked();
        drop(client);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(result.unwrap()["privateRoundtrip"], true);
        cleanup.unwrap();
    }

    fn side_rpc_connection<'a>(scenario: &str, progress: &'a mut dyn FnMut() -> Result<()>) -> DesktopConnection<'a> {
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", SIDE_RPC_FIXTURE]).env("SIDE_RPC_SCENARIO", scenario);
        // Deliberately bypass account discovery: these fixtures never read user
        // credentials or obtain provider headers, even under a configured shell.
        let mut client = Client::spawn_with_account_host(command, Path::new(env!("CARGO_MANIFEST_DIR")), progress, None, "ZCode Desktop Agent").unwrap();
        client.session = Some("parent-fixture".into());
        DesktopConnection {
            client, session_id: "parent-fixture".into(), subscription: "parent-sub".into(),
            projection: Projection::default(), wire: Wire::default(), models: Vec::new(), selected_model_id: None,
            permission_mode: "build".into(), workflow_notice: None, workflow_enabled: false,
            child: None, side: None, workspace: json!({"workspaceKey":"fixture","workspacePath":"fixture"}),
        }
    }

    #[cfg(windows)]
    #[test]
    fn tracked_worker_cleanup_serializes_identity_removal_with_concurrent_ui_stop() {
        let pid = std::sync::atomic::AtomicU32::new(0);
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "setInterval(()=>{},1000)"]);
        let mut progress = || Ok(());
        let mut client = Client::spawn_with_account_host(command, Path::new(env!("CARGO_MANIFEST_DIR")), &mut progress, None, "ZCode Desktop Agent").unwrap();
        pid.store(client.child.id(), Ordering::Release);
        client.tracked_process_id = Some(&pid);
        let stopped = std::sync::Arc::new(std::sync::Barrier::new(2));
        let resumed = std::sync::Arc::new(std::sync::Barrier::new(2));
        let hook_stopped = stopped.clone(); let hook_resumed = resumed.clone();
        // Pause exactly after successful owned cleanup, before Child::wait and
        // the legacy final CAS. Previously this gap had no registry but live PID.
        client.cleanup_after_stop = Some(Box::new(move || { hook_stopped.wait(); hook_resumed.wait(); }));
        std::thread::scope(|scope| {
            let ui = scope.spawn(|| {
                stopped.wait();
                let result = crate::acp::terminate_acp_process_checked(&pid);
                resumed.wait();
                result
            });
            client.cleanup_checked().unwrap();
            ui.join().unwrap().expect("concurrent UI stop must observe an atomically cleared owned identity");
        });
        assert_eq!(pid.load(Ordering::Acquire), 0);
        assert!(client._identity.is_verified());
        assert!(client.child.try_wait().unwrap().is_some());
        client.cleanup_checked().unwrap();
    }

    #[test]
    fn side_identity_is_trusted_during_subscribe_send_and_unsubscribe_reverse_requests() {
        let mut progress = || Ok(());
        let mut connection = side_rpc_connection("success", &mut progress);
        let stopped = AtomicBool::new(false);
        let mut approvals = Vec::new();
        let mut permission = |request: &Value, _: &AtomicBool| { approvals.push(request["sessionId"].as_str().unwrap().to_owned()); Ok(true) };
        assert_eq!(connection.create_side_session(Some("first 中文"), &stopped, &mut permission).unwrap(), "side-fixture");
        assert!(connection.client.side_sessions.iter().any(|id| id == "side-fixture"));
        connection.close_side(&stopped, &mut permission).unwrap();
        assert!(connection.client.side_sessions.is_empty());
        assert!(connection.side_detail().is_none());
        assert_eq!(approvals, ["side-fixture", "side-fixture", "side-fixture"]);
    }

    #[test]
    fn side_subscribe_failure_rolls_back_identity_and_rejects_foreign_reverse_requests() {
        for scenario in ["bad-ack", "wrong-session"] {
            let mut progress = || Ok(());
            let mut connection = side_rpc_connection(scenario, &mut progress);
            let mut approvals = 0;
            let error = connection.create_side_session(Some("first 中文"), &AtomicBool::new(false), &mut |request, _| {
                approvals += 1; assert_eq!(request["sessionId"], "side-fixture"); Ok(true)
            }).unwrap_err();
            assert!(connection.client.side_sessions.is_empty());
            assert!(connection.side_detail().is_none());
            if scenario == "bad-ack" { assert_eq!(approvals, 2); assert!(error.to_string().contains("快照确认无效")); }
            else { assert_eq!(approvals, 0); assert!(error.to_string().contains("身份不匹配")); }
        }
    }

    #[test]
    fn side_auth_request_before_subscribe_ack_is_handled_as_auth_not_identity_failure() {
        let mut progress = || Ok(());
        let mut connection = side_rpc_connection("auth-during-subscribe", &mut progress);
        connection.create_side_session(None, &AtomicBool::new(false), &mut |_, _| Ok(true)).unwrap();
        assert!(connection.take_auth_error().unwrap().contains("账号模型登录"));
        assert!(connection.side_detail().is_some());
        connection.close_side(&AtomicBool::new(false), &mut |_, _| Ok(true)).unwrap();
    }

    #[test]
    #[ignore = "明确授权后单独运行：当前 Individual 模型仅回复 OK 的一轮真实调用"]
    fn zcode_installed_account_model_smoke() {
        installed_account_check(true, false, "account:bigmodel-individual-coding-plan");
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "需要 PATH 上的 zcode CLI 与原生登录，仅目录/绑定，不调用模型"]
    fn zcode_installed_cli_account_catalog_and_binding() {
        installed_account_check(false, true, "account:bigmodel-individual-coding-plan");
    }
    #[test]
    #[ignore = "需要原生安装与账号；确认暂停的 Start Plan 从真实模型目录移除且旧选择无法设置，不调用模型"]
    fn zcode_installed_start_plan_is_excluded() {
        installed_account_check(false, false, "account:bigmodel-start-plan");
    }
    #[test]
    #[ignore = "仅恢复 Start Plan 集成后手动验收：真实调用仅回复 OK；暂停期间不重复触发服务商拦截"]
    fn zcode_installed_start_plan_model_smoke() {
        installed_account_check(true, false, "account:bigmodel-start-plan");
    }
    fn installed_account_check(send: bool, cli: bool, provider: &str) {
        let root = std::env::temp_dir().join(format!(
            "zcode-account-catalog-{}{}",
            if cli { "-cli" } else { "" },
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let result = (|| -> Result<()> {
            let mut command = launch(&runtime_executable(if cli {
                crate::multi_agent::AgentRuntimeKind::ZCodeCli
            } else {
                crate::multi_agent::AgentRuntimeKind::ZCode
            })?)?;
            command.env("ZCODE_STORAGE_DIR", root.join("storage"));
            let mut progress = || Ok(());
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let stopped = AtomicBool::new(false);
            let mut connection = DesktopConnection::open(
                command,
                &root,
                None,
                true,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            if !send && provider == "account:bigmodel-start-plan" {
                assert!(connection.models.iter().all(|choice| choice.selection["providerId"] != provider));
                let old = json!({"providerId":provider,"modelId":"GLM-5.3-Flash","options":{"reasoningLevel":"high"}}).to_string();
                assert!(connection.set_model(&old, &stopped, &mut deny).is_err(), "saved Start Plan selection must remain blocked");
                println!("ZCODE_START_PLAN_EXCLUDED model_count={} old_selection_rejected=true no_model_request=true",connection.models.len());
                connection.shutdown()?;
                return Ok(());
            }
            let id = connection
                .models
                .iter()
                .find(|c| c.selection["providerId"] == provider && (!send || provider != "account:bigmodel-individual-coding-plan" || c.selection["modelId"] == "GLM-5.3"))
                .context("原生目录未返回指定账号 Provider 的可用模型")?
                .id
                .clone();
            let models = connection.models.len();
            connection.set_model(&id, &stopped, &mut deny)?;
            assert_eq!(connection.client.selected_model.to_string(), id);
            assert_eq!(connection.selected_model_id.as_deref(), Some(id.as_str()));
            if send {
                let command =
                    connection.send_text("仅回复 OK，不调用任何工具。", &stopped, &mut deny)?;
                let deadline = Instant::now() + Duration::from_secs(80);
                loop {
                    connection.poll(&stopped, &mut deny)?;
                    if connection.take_auth_error().is_some() {
                        bail!("真实账号模型鉴权失败，未输出服务响应");
                    }
                    match connection.turn_state(&command) {
                        Some("completedSuccess") => break,
                        Some("failed" | "completedInterrupted") => {
                            bail!("真实账号模型未成功完成，未输出服务响应")
                        }
                        _ => {}
                    }
                    if Instant::now() >= deadline {
                        connection.cancel(&stopped, &mut deny)?;
                        bail!("真实账号模型验证超时，已发送取消请求");
                    }
                }
                let reply = connection.rows().iter().any(|row| {
                    row["kind"] == "assistantText"
                        && row["text"].as_str().is_some_and(|s| !s.trim().is_empty())
                });
                assert!(reply, "成功终态应有助手回复");
                println!(
                    "ZCODE_ACCOUNT_MODEL_OK model={} assistant_reply_present=true terminal_success=true", connection.client.selected_model["modelId"]
                );
            } else {
                println!("ZCODE_ACCOUNT_CATALOG_OK model_count={models} binding_confirmed=true no_model_request=true");
            }
            connection.shutdown().unwrap();
            drop(connection);
            Ok(())
        })();
        let mut cleanup = std::fs::remove_dir_all(&root);
        for _ in 0..20 {
            if cleanup.is_ok() {
                break;
            }
            // Native workers can briefly retain SQLite handles after their
            // owning process exits. Retry only this test's private directory.
            std::thread::sleep(Duration::from_millis(100));
            cleanup = std::fs::remove_dir_all(&root);
        }
        result.unwrap();
        cleanup.unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn zcode_startup_and_managed_failures_preserve_unverified_stop_identity() {
        const PEER: &str = r#"
            const readline=require('node:readline');let pending;const sid='cleanup-fixture';
            const emit=x=>process.stdout.write(JSON.stringify(x)+'\n');
            const reply=(id,result)=>emit({id,result});
            readline.createInterface({input:process.stdin}).on('line',line=>{
              const v=JSON.parse(line),p=v.params;
              if(!v.method){
                reply(pending.id,process.env.TEST_STAGE==='pre'?{}:{ack:{subscriptionId:'sub',mode:'snapshot',logEpoch:'epoch'}});
                if(process.env.TEST_STAGE==='post')emit({method:'v4/conversation/frame',params:{wireVersion:3,topic:'conversation/'+sid,subscriptionId:'sub',kind:'complete',logicalFrameId:'invalid-initial',logicalFrameOrdinal:1,deliveryKind:'initial',frame:{topic:'conversation/'+sid,subscriptionId:'sub',fromSeq:0,toSeq:1,payload:{kind:'invalid-fixture'}}}});
                return;
              }
              if(v.method==='v4/commands/query')reply(v.id,{results:[]});
              else if(v.method==='workspace/updateDynamicWorkflowPolicy')reply(v.id,{workspace:p.workspace,enabled:p.enabled});
              else if(v.method==='session/create')reply(v.id,{session:{sessionId:sid},settings:{}});
              else if(v.method==='v4/conversation/subscribe'){
                pending=v;emit({id:'deny-stop',method:'interaction/requestPermission',params:{sessionId:sid,requestId:'fixture-stop',toolName:'Bash',input:{pid:process.pid}}});
              }else if(v.method==='v4/command'){
                reply(v.id,{commandId:p.commandId,status:'accepted',...(p.type==='createSession'?{result:{sessionId:sid}}:{})});
              }else reply(v.id,{});
            });
        "#;
        for (desktop, stage) in [(false,"pre"),(true,"pre"),(true,"post")] {
            let mut command = Command::new(which::which("node").unwrap());
            command.args(["-e", PEER]).env("TEST_STAGE", stage);
            let mut denied = None;
            let mut original_pid = None;
            let mut permission = |params: &Value, _: &AtomicBool| {
                let pid = params["input"]["pid"].as_u64().unwrap() as u32;
                original_pid = Some(pid);
                denied = Some(crate::acp::TestStopFailureGuard::new(pid));
                Ok(false)
            };
            let stopped = AtomicBool::new(false);
            let mut progress = || Ok(());
            let error = if desktop {
                match DesktopConnection::open(command, Path::new(env!("CARGO_MANIFEST_DIR")), None, true, vec![], &stopped, &mut permission, &mut progress) {
                    Err(error) => error,
                    Ok(_) => panic!("invalid startup must fail"),
                }
            } else {
                execute_command(command, Path::new(env!("CARGO_MANIFEST_DIR")), None, "fixture", "build", Some("personal/model"), json!([]), &stopped, &mut permission, &mut progress).err().expect("invalid handshake must fail")
            };
            assert!(error.downcast_ref::<crate::acp::UnverifiedStop>().is_some(), "{error:#}");
            drop(permission);
            drop(denied.take());
            // Cleanup still has its registered spawn-time handle after the
            // failing Client and guard dropped; this cannot reopen a naked PID.
            crate::acp::terminate_process_tree_checked(original_pid.unwrap()).unwrap();
        }
    }

    fn fixture(scenario: &str) -> Command {
        let mut command = Command::new(which::which("node").unwrap());
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/zcode-agent.mjs"))
            .env("ZCODE_TEST_SCENARIO", scenario);
        command
    }
    fn mcp(token: &str) -> Value {
        json!([{"name":"crewvela","type":"http","url":"http://127.0.0.1:1/mcp","headers":[{"name":"Authorization","value":format!("Bearer {token}")}],"isolation":"session"}])
    }
    #[test]
    fn zcode_native_roundtrip_resume_permissions_fragments_and_official_mcp_unavailable() {
        for (scenario, resume) in [
            ("roundtrip", None),
            ("deny", Some("fixture-session")),
            ("official-mcp-startup", None),
            ("official-mcp-after-create", None),
        ] {
            let mut approvals = 0;
            let result = execute_command(
                fixture(scenario),
                Path::new(env!("CARGO_MANIFEST_DIR")),
                resume,
                "fixture",
                "build",
                Some("personal/model"),
                mcp("new-run"),
                &AtomicBool::new(false),
                &mut |params, _| {
                    approvals += 1;
                    assert_eq!(params["sessionId"], "fixture-session");
                    Ok(scenario != "deny")
                },
                &mut || Ok(()),
            )
            .unwrap();
            assert_eq!(result.text, "fixture completed");
            assert_eq!(approvals, 1);
        }
    }
    #[test]
    fn zcode_rejection_diagnostics_and_protocol_boundaries() {
        for scenario in [
            "missing-model",
            "unknown-rejection",
            "wrong-rejected-ack",
            "unknown-status",
        ] {
            let error = execute_command(
                fixture(scenario),
                Path::new(env!("CARGO_MANIFEST_DIR")),
                None,
                "fixture",
                "build",
                None,
                mcp("new-run"),
                &AtomicBool::new(false),
                &mut |_, _| Ok(false),
                &mut || Ok(()),
            )
            .err()
            .unwrap();
            assert_eq!(
                error.downcast_ref::<CommandRejected>().is_some(),
                matches!(scenario, "missing-model" | "unknown-rejection")
            );
            let message = error.to_string();
            assert!(!message.contains("fixture-secret"));
            if scenario == "missing-model" {
                assert!(message.contains("尚未选择"));
            }
        }
    }
    #[test]
    fn zcode_rejects_foreign_session_and_account_auth() {
        for scenario in ["wrong-session", "account", "bad-ack"] {
            let mut approvals = 0;
            let result = execute_command(
                fixture(scenario),
                Path::new(env!("CARGO_MANIFEST_DIR")),
                None,
                "fixture",
                "build",
                None,
                mcp("new-run"),
                &AtomicBool::new(false),
                &mut |_, _| {
                    approvals += 1;
                    Ok(true)
                },
                &mut || Ok(()),
            );
            assert!(result.is_err());
            assert_eq!(approvals, 0);
        }
    }
    #[test]
    fn zcode_cancel_does_not_treat_ack_as_completion() {
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(600));
                cancelled.store(true, Ordering::Release);
            });
            let result = execute_command(
                fixture("cancel"),
                Path::new(env!("CARGO_MANIFEST_DIR")),
                None,
                "fixture",
                "build",
                None,
                mcp("new-run"),
                &cancelled,
                &mut |_, _| Ok(false),
                &mut || Ok(()),
            );
            assert!(result.err().unwrap().to_string().contains("取消"));
        });
    }
    #[test]
    fn zcode_fragment_checksum_fails_closed() {
        let mut wire = Wire::default();
        let frame = json!({"wireVersion":3,"kind":"fragment","deliveryKind":"initial","logicalFrameOrdinal":1,"logicalFrameId":"one","topic":"conversation/s","subscriptionId":"sub","fragmentCount":1,"fragmentIndex":0,"logicalBytes":2,"checksum":{"algorithm":"crc32","value":"00000000"},"dataBase64":"e30="});
        assert!(wire.accept(&frame, "s", "sub").is_err());
        assert!(model_selection(Some("invalid")).is_err());
    }
    #[test]
    fn zcode_workflow_policy_ack_failure_resets_native_flag() {
        for scenario in ["policy_bad_ack", "policy_reset_bad", "policy_unsupported"] {
            let mut command = Command::new(which::which("node").unwrap());
            command
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../tests/fixtures/native-zcode-agent.mjs"),
                )
                .env("ZCODE_NATIVE_SCENARIO", scenario);
            let mut progress = || Ok(());
            let mut permission = |_: &Value, _: &AtomicBool| Ok(false);
            let stopped = AtomicBool::new(false);
            let mut client = Client::spawn(
                command,
                Path::new(env!("CARGO_MANIFEST_DIR")),
                &mut progress,
            )
            .unwrap();
            let workspace = json!({"workspaceKey":"fixture","workspacePath":"fixture"});
            let result =
                sync_workflow_policy(&mut client, &workspace, true, &stopped, &mut permission);
            if scenario == "policy_reset_bad" {
                assert!(result.unwrap_err().to_string().contains("安全关闭"));
            } else {
                assert!(!result.unwrap());
                let state = client
                    .request(
                        "fixture/workflowState",
                        json!({}),
                        &stopped,
                        &mut permission,
                    )
                    .unwrap();
                assert_eq!(state["enabled"], false);
            }
        }
    }
    #[test]
    #[ignore = "需要已安装并已初始化安全凭据的 ZCode Desktop；仅对合成文本做私有加解密，不调用模型或写入凭据"]
    fn zcode_installed_cipher_uses_native_secure_storage_without_model_call() {
        let command = launch(&executable().unwrap()).unwrap();
        let host = account_host::AccountHost::prepare(&command).unwrap().expect("native desktop account host");
        let request_id = uuid::Uuid::new_v4().to_string();
        let encrypted = host.credential_cipher(&json!({"requestId":request_id,"operation":"encrypt","value":"synthetic-native-cipher-probe"})).unwrap();
        assert_eq!(encrypted["result"]["ok"], true, "native secure storage must be available (no response values logged)");
        let decrypted = host.credential_cipher(&json!({"requestId":request_id,"operation":"decrypt","value":encrypted["result"]["value"]})).unwrap();
        assert_eq!(decrypted["result"]["ok"], true);
        assert!(decrypted["result"]["value"] == "synthetic-native-cipher-probe", "native private roundtrip did not preserve synthetic input");
        println!("ZCODE_NATIVE_CIPHER_OK no_model_request=true no_credential_write=true");
    }

    #[test]
    #[ignore = "需要已安装的 ZCode Desktop；隔离存储，验证空草稿跨进程恢复，不调用模型"]
    fn zcode_installed_empty_draft_cross_process_restore() {
        let directory = std::env::temp_dir().join(format!("crewvela-zcode-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = (|| -> Result<()> {
            let command = || -> Result<Command> {
                let mut command = launch(&executable()?)?;
                command.env("ZCODE_STORAGE_DIR", directory.join("storage"))
                    .env("ZCODE_DATA_BASE_DIR", &directory)
                    .env("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE", directory.join("personal-empty.json"));
                Ok(command)
            };
            let stopped = AtomicBool::new(false);
            let mut progress = || Ok(());
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let mut connection = DesktopConnection::open(command()?, &directory, None, false, Vec::new(), &stopped, &mut deny, &mut progress)?;
            assert!(connection.rows().is_empty());
            let original = connection.session_id.clone();
            connection.shutdown()?;
            drop(connection);
            let mut restored = DesktopConnection::open(command()?, &directory, Some(&original), true, Vec::new(), &stopped, &mut deny, &mut progress)?;
            assert!(restored.rows().is_empty());
            assert!(!restored.session_id.is_empty());
            restored.shutdown()?;
            Ok(())
        })();
        if directory.exists() { std::fs::remove_dir_all(&directory).unwrap(); }
        result.unwrap();
    }

    #[test]
    #[ignore = "需要已安装的 ZCode Desktop；隔离存储，只读能力目录，不调用模型"]
    fn zcode_installed_composer_catalog_without_model_call() {
        let directory =
            std::env::temp_dir().join(format!("crewvela-zcode-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = (|| -> Result<()> {
            let mut command = launch(&executable()?)?;
            command
                .env("ZCODE_STORAGE_DIR", directory.join("storage"))
                .env("ZCODE_DATA_BASE_DIR", &directory)
                .env(
                    "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
                    directory.join("personal-empty.json"),
                );
            let stopped = AtomicBool::new(false);
            let mut progress = || Ok(());
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let mut connection = DesktopConnection::open(
                command,
                &directory,
                None,
                false,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            let catalog =
                connection.composer_catalog(directory.to_str().unwrap(), &stopped, &mut deny);
            for kind in ["skills", "plugins", "commands"] {
                assert!(
                    catalog[kind].get("error").is_none(),
                    "{kind} catalog error: {}",
                    catalog[kind]["error"]
                );
                assert!(catalog[kind]["items"].is_array());
                eprintln!(
                    "ZCode Desktop installed {kind} count={}",
                    catalog[kind]["items"].as_array().unwrap().len()
                );
            }
            eprintln!(
                "ZCode Desktop installed command names={:?}",
                catalog["commands"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item["name"].as_str())
                    .collect::<Vec<_>>()
            );
            let has_workflow = catalog["commands"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "workflow");
            assert_eq!(
                has_workflow, connection.workflow_enabled,
                "原生目录必须符合已确认的公开灰度判定"
            );
            eprintln!(
                "ZCode Desktop installed workflow enabled={}",
                connection.workflow_enabled
            );
            connection.shutdown().unwrap();
            Ok(())
        })();
        let mut cleanup = std::fs::remove_dir_all(&directory);
        for _ in 0..10 {
            if !cleanup
                .as_ref()
                .is_err_and(|error| error.raw_os_error() == Some(32))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            cleanup = std::fs::remove_dir_all(&directory);
        }
        cleanup.unwrap();
        result.unwrap();
    }
    #[test]
    #[ignore = "需要已安装的 ZCode Desktop；隔离存储验证类型化斜杠命令信封，不调用模型"]
    fn zcode_desktop_slash_command_control_plane() {
        let directory =
            std::env::temp_dir().join(format!("crewvela-zcode-slash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = (|| -> Result<()> {
            let mut command = launch(&executable()?)?;
            command
                .env("ZCODE_STORAGE_DIR", directory.join("storage"))
                .env("ZCODE_DATA_BASE_DIR", &directory)
                .env(
                    "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
                    directory.join("personal-empty.json"),
                );
            let stopped = AtomicBool::new(false);
            let mut progress = || Ok(());
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let mut connection = DesktopConnection::open(
                command,
                &directory,
                None,
                true,
                Vec::new(),
                &stopped,
                &mut deny,
                &mut progress,
            )?;
            // 未知名与空目标在客户端侧拒绝，不产生线上信封。
            assert!(connection
                .slash_command("setAutoDrain", None, None, &stopped, &mut deny)
                .is_err());
            assert!(connection
                .slash_command("sendGoalCommand", Some("  "), None, &stopped, &mut deny)
                .is_err());
            // plan 模式下 goal 只落库不续跑（原生 canContinue=false）：无模型也能走完整
            // 类型化通路。resume 在无 target 时幂等 noop（原生同语义）。
            connection.set_permission_mode("plan", &stopped, &mut deny)?;
            connection.slash_command("resumeGoal", None, None, &stopped, &mut deny)?;
            connection.slash_command(
                "sendGoalCommand",
                Some("isolated objective"),
                Some("/goal isolated objective"),
                &stopped,
                &mut deny,
            )?;
            connection.shutdown().unwrap();
            Ok(())
        })();
        let mut cleanup = std::fs::remove_dir_all(&directory);
        for _ in 0..10 {
            if !cleanup
                .as_ref()
                .is_err_and(|error| error.raw_os_error() == Some(32))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            cleanup = std::fs::remove_dir_all(&directory);
        }
        cleanup.unwrap();
        result.unwrap();
    }
    #[test]
    #[ignore = "需要已安装的 ZCode Desktop Agent，不调用模型"]
    fn zcode_installed_control_surface_without_model_call() {
        let directory =
            std::env::temp_dir().join(format!("crewvela-zcode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let result = (|| -> Result<()> {
            let path = executable()?;
            let isolated = || -> Result<Command> {
                let mut command = launch(&path)?;
                command
                    .env("ZCODE_STORAGE_DIR", directory.join("storage"))
                    .env("ZCODE_DATA_BASE_DIR", &directory)
                    .env(
                        "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE",
                        directory.join("personal-empty.json"),
                    );
                Ok(command)
            };
            let cancelled = AtomicBool::new(false);
            let mut progress = || Ok(());
            let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
            let workspace = json!({"workspaceKey":directory.to_string_lossy(),"workspacePath":directory.to_string_lossy()});
            let mut client = Client::spawn(isolated()?, &directory, &mut progress)?;
            let (_, ack) = client.command("createSession",None,json!({"workspaceId":directory.to_string_lossy(),"config":{"mode":"build"},"mcpServers":[]}),&cancelled,&mut deny)?;
            let draft = ack
                .pointer("/result/sessionId")
                .and_then(Value::as_str)
                .context("v4 create 缺少 sessionId")?;
            client.session = Some(draft.to_owned());
            let subscribe = client.request("v4/conversation/subscribe",json!({"topic":format!("conversation/{draft}"),"connectionId":"crewvela-draft","clientMode":"desktop-continuous","workspace":workspace}),&cancelled,&mut deny)?;
            let subscription = subscribe
                .pointer("/ack/subscriptionId")
                .and_then(Value::as_str)
                .context("subscribe 缺少 subscriptionId")?;
            let mut wire = Wire::default();
            let mut projection = Projection {
                epoch: subscribe
                    .pointer("/ack/logEpoch")
                    .and_then(Value::as_str)
                    .context("subscribe 缺少 epoch")?
                    .into(),
                ..Default::default()
            };
            while !projection.initialized {
                let frame = client.receive(Instant::now() + CONTROL_TIMEOUT, &cancelled)?;
                if frame["method"] == "v4/conversation/frame" {
                    if let Some(frame) = wire.accept(&frame["params"], draft, subscription)? {
                        projection.apply(&frame, draft, subscription)?;
                    }
                }
            }
            assert!(projection.rows.is_empty());
            let rejected = client.request("v4/command", json!({"commandId":uuid::Uuid::new_v4().to_string(),"clientId":"crewvela","sessionId":draft,"type":"sendText","payload":{"text":"isolated admission check","requestedDelivery":"startNow","mode":"build"},"issuedAt":chrono::Utc::now().timestamp_millis()}), &cancelled, &mut deny)?;
            assert_eq!(rejected["status"], "failed");
            assert!(rejected["message"].as_str().is_some_and(
                |message| message.starts_with("Session model must be provider-qualified:")
            ));
            client.request(
                "session/close",
                json!({"sessionId":draft}),
                &cancelled,
                &mut deny,
            )?;
            // 空草稿没有持久化历史；用上游无模型历史导入构造真实可恢复会话。
            client.session = None;
            let result = client.request("session/create",json!({"workspace":workspace,"mode":"build","persistence":"immediate","titleGenerationEnabled":false,"mcpServers":[],"importedHistory":{"source":"claudeCode","title":"CrewVela control test","messages":[{"role":"user","content":"isolated control fixture"},{"role":"assistant","content":"persisted without model"}]}}),&cancelled,&mut deny)?;
            let sid = result
                .pointer("/session/sessionId")
                .and_then(Value::as_str)
                .context("create 无会话标识")?
                .to_owned();
            client.session = Some(sid.clone());
            let subscribe = client.request("v4/conversation/subscribe",json!({"topic":format!("conversation/{sid}"),"connectionId":"crewvela-test","clientMode":"desktop-continuous","workspace":workspace}),&cancelled,&mut deny)?;
            let subscription = subscribe.pointer("/ack/subscriptionId").and_then(Value::as_str).context("标题订阅缺少标识")?;
            let mut title_wire = Wire::default();
            let mut title_projection = Projection {
                epoch: subscribe.pointer("/ack/logEpoch").and_then(Value::as_str).context("标题订阅缺少 epoch")?.into(),
                ..Default::default()
            };
            // Exercise the installed metadata stream without making any model request.
            client.command("renameSession", Some(&sid), json!({"title":"隔离标题验证"}), &cancelled, &mut deny)?;
            let deadline = Instant::now() + CONTROL_TIMEOUT;
            while title_projection.session_title() != Some("隔离标题验证") {
                let frame = if let Some(frame) = client.queued.pop_front() { frame } else { client.receive(deadline, &cancelled)? };
                if frame["method"] == "v4/conversation/frame" && frame["params"]["subscriptionId"] == subscription {
                    if let Some(frame) = title_wire.accept(&frame["params"], &sid, subscription)? {
                        title_projection.apply(&frame, &sid, subscription)?;
                    }
                }
            }
            assert_eq!(title_projection.title_source.as_deref(), Some("custom"));
            client.request(
                "session/setMode",
                json!({"sessionId":sid,"mode":"build"}),
                &cancelled,
                &mut deny,
            )?;
            client.request(
                "session/close",
                json!({"sessionId":sid}),
                &cancelled,
                &mut deny,
            )?;
            drop(client);
            let mut client = Client::spawn(isolated()?, &directory, &mut progress)?;
            client.session = Some(sid.clone());
            client.request(
                "session/resume",
                json!({"sessionId":sid,"workspace":workspace,"mcpServers":[]}),
                &cancelled,
                &mut deny,
            )?;
            client.request(
                "session/close",
                json!({"sessionId":sid}),
                &cancelled,
                &mut deny,
            )?;
            Ok(())
        })();
        // Windows 文件句柄释放可能稍晚于进程退出；仅对共享冲突有限重试，仍保留清理失败。
        let mut cleanup = std::fs::remove_dir_all(&directory);
        for _ in 0..10 {
            if !cleanup
                .as_ref()
                .is_err_and(|error| error.raw_os_error() == Some(32))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            cleanup = std::fs::remove_dir_all(&directory);
        }
        cleanup.unwrap();
        result.unwrap();
    }
}
