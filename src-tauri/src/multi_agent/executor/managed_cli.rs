use super::super::conversation::research::secret_component;
use super::super::managed_delivery::ManagedUse;
use super::{
    external_command_path, hidden_command, managed_static_review, render_task_prompt, run_piped_command,
    runtime_command, spawn_passive_stream_reader, AgentRuntimeKind, RuntimeRequest, RuntimeResult,
};
use crate::acp::{AcpConnection, ManagedAcpKind};
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLICY: &str = "text-only-v1";
const TRANSPORT: &str = "managed-acp";
const KIMI_AGENT: &str = "---\nname: agent\ndescription: CrewVela isolated text analysis\noverride: true\ntools: []\nsubagents: []\n---\nWork from the text supplied by CrewVela.\n";
const DIFF_LIMIT: usize = 256 * 1024;

pub(super) fn supports(kind: AgentRuntimeKind, usage: ManagedUse) -> bool {
    matches!(kind, AgentRuntimeKind::KimiCode | AgentRuntimeKind::OpenCode)
        && matches!(usage, ManagedUse::Plan | ManagedUse::Review)
}

pub(super) fn execute(request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
    ensure!(request.managed, "缺少托管任务授权");
    if request.managed_planner() {
        ensure!(
            request.store.conversation_task_images(&request.task.id)?.is_empty(),
            "该受限规划入口仅支持文本，图片附件未投递"
        );
        request.log_launch("无工具执行", "原生 ACP 规划")?;
        return run(request, cancelled, &render_task_prompt(request));
    }
    ensure!(
        request.task.workspace_mode == super::WorkspaceMode::ReadOnly
            && request.agent.role == super::AgentRole::Reviewer,
        "Kimi Code 和 OpenCode 的托管运行仅支持规划和静态审查"
    );
    request.log_launch("受限静态审查", "原生 ACP 读取 Git 交付差异")?;
    managed_static_review(request, cancelled, |instructions| {
        let context = review_context(request, cancelled)?;
        run(request, cancelled, &format!("{instructions}\n\n本轮工具执行已禁用。下面是宿主从实际交付 Git 对象取得的变更，基于这些内容评审。证据不足时说明缺失内容并请求补充。\n{context}"))
    })
}

fn review_context(request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<String> {
    let mut context = String::new();
    let mut seen = std::collections::HashSet::new();
    let snapshot = request.store.snapshot(&request.project.id)?;
    let discussion = request.store.conversation_snapshot(&request.project.id)?;
    let restricted: Vec<&str> = std::iter::once(&discussion.project_policy)
        .chain(discussion.member_policies.iter().filter(|p| p.agent_id.as_deref() == Some(&request.agent.id)))
        .flat_map(|p| p.boundary.denied_read_paths.iter().map(String::as_str))
        .collect();
    for artifact in request.dependency_artifacts.iter().filter(|a| a.artifact_type == "git_commit") {
        let commit = artifact.content_hash.as_deref().context("交付缺少 Git 哈希")?;
        let locator = snapshot
            .runs
            .iter()
            .find(|run| run.id == artifact.run_id)
            .and_then(|run| run.session_locator.as_ref())
            .context("交付缺少经确认的开发轮次")?;
        let base = locator["managedBaseline"].as_str().context("交付缺少已验证的起始提交")?;
        ensure!(
            [base, commit]
                .iter()
                .all(|h| matches!(h.len(), 40 | 64) && h.bytes().all(|b| b.is_ascii_hexdigit())),
            "交付范围不是完整的 Git 对象标识"
        );
        if !seen.insert((base, commit)) {
            continue;
        }
        let stats = super::delivery_git(
            &request.cwd,
            &[
                "diff",
                "--relative",
                "--numstat",
                "-z",
                "--no-textconv",
                "--no-ext-diff",
                "--no-renames",
                base,
                commit,
                "--",
                ".",
            ],
        )?;
        validate_diff_paths(&stats, Path::new(&request.project.root_path), &restricted)?;
        let mut command = hidden_command("git");
        command.current_dir(&request.cwd).args([
            "--no-pager",
            "diff",
            "--relative",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--unified=6",
            base,
            commit,
            "--",
            ".",
        ]);
        let output = run_piped_command(
            command,
            None,
            cancelled,
            Duration::from_secs(30),
            "读取实际交付差异",
            |out, err| (spawn_passive_stream_reader(out), spawn_passive_stream_reader(err)),
        )?;
        ensure!(output.status.success(), "Git 无法读取已登记的交付提交");
        ensure!(
            context.len() + output.stdout.len() + 1 <= DIFF_LIMIT,
            "交付差异超过 256 KiB 静态审查上限，未裁剪差异或自动批准"
        );
        context.push_str(&output.stdout);
        context.push('\n');
    }
    ensure!(!seen.is_empty() && context.contains("diff --git "), "交付没有可验证的文件差异");
    Ok(context)
}

fn validate_diff_paths(stats: &[u8], root: &Path, restricted: &[&str]) -> Result<()> {
    fn normalized(path: &Path) -> PathBuf {
        let text = external_command_path(path).to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            PathBuf::from(text.to_lowercase())
        } else {
            PathBuf::from(text)
        }
    }
    for item in std::str::from_utf8(stats)?.split('\0').filter(|item| !item.is_empty()) {
        let mut columns = item.splitn(3, '\t');
        let added = columns.next().unwrap_or_default();
        let removed = columns.next().context("Git 变更记录缺少删除行数")?;
        ensure!(
            added.parse::<u64>().is_ok() && removed.parse::<u64>().is_ok(),
            "交付包含二进制文件，无法完成文本静态审查"
        );
        let path = columns.next().context("Git 变更记录缺少路径")?;
        ensure!(!path.split(['/', '\\']).any(secret_component), "交付包含凭据或密钥路径，未向模型发送差异");
        let candidate = normalized(&root.join(path));
        ensure!(
            !restricted.iter().any(|denied| candidate.starts_with(normalized(&root.join(denied)))),
            "交付涉及项目或该成员禁止读取的路径，未向模型发送差异"
        );
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    let mut options = fs::DirBuilder::new();
    options.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        options.mode(0o700);
    }
    options.create(path)?;
    Ok(())
}

fn home_path(key: &str, default: &str) -> Result<PathBuf> {
    Ok(std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(default)))
        .context("无法定位原生 Runtime 配置目录")?)
}

// The session and index must survive an acceptance retry. Only inert provider
// configuration is imported. Never mount the user's plugins, MCP or hooks.
struct IsolatedProfile {
    kind: AgentRuntimeKind,
    root: PathBuf,
    work: PathBuf,
    env: Vec<(OsString, OsString)>,
    temporary_config: Option<PathBuf>,
    agent: String,
    expected_model: Option<String>,
}

impl IsolatedProfile {
    fn prepare(request: &RuntimeRequest) -> Result<Self> {
        let kind = request.agent.runtime_kind;
        let key =
            format!("{}\0{}\0{}\0{}", request.project.id, request.task.id, request.agent.id, kind.as_str());
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        let root = request.store.conversation_attachment_root.with_extension("managed-runtime").join(&hash);
        Self::at(kind, root, format!("crewvela-managed-{}", &hash[..16]), request.agent.model.as_deref())
    }

    fn at(kind: AgentRuntimeKind, root: PathBuf, agent: String, model: Option<&str>) -> Result<Self> {
        let work = root.join("input");
        private_dir(&work)?;
        let user = root.join("user");
        private_dir(&user)?;
        let mut profile = Self {
            kind,
            root,
            work,
            env: Vec::new(),
            temporary_config: None,
            agent,
            expected_model: model.filter(|s| !s.trim().is_empty()).map(str::to_owned),
        };
        for key in ["HOME", "USERPROFILE", "OPENCODE_TEST_HOME"] {
            profile.set(key, &user);
        }
        // Prevent generic cross-tool discovery under the real OS home as well.
        let xdg = [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
        ];
        for (name, folder) in xdg {
            profile.set(name, profile.root.join(folder));
        }
        match kind {
            AgentRuntimeKind::KimiCode => profile.kimi()?,
            AgentRuntimeKind::OpenCode => profile.opencode(model)?,
            _ => bail!("该 Runtime 不属于 Kimi/OpenCode 托管协议"),
        }
        Ok(profile)
    }

    fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
        self.env.push((OsString::from(key), value.as_ref().to_owned()));
    }

    fn kimi(&mut self) -> Result<()> {
        let original = home_path("KIMI_CODE_HOME", ".kimi-code")?;
        let native = self.root.join("kimi");
        private_dir(&native)?;
        let path = original.join("config.toml");
        let mut config: toml::Table = match fs::read_to_string(path) {
            Ok(contents) => toml::from_str(contents.trim_start_matches('\u{feff}'))
                .map_err(|_| anyhow!("Kimi 原生模型配置无法解析"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(error) => return Err(error).context("无法读取 Kimi 原生模型配置"),
        };
        config.retain(|key, _| matches!(key, "default_model" | "models" | "providers" | "thinking"));
        config.insert("default_permission_mode".into(), "manual".into());
        let rule: toml::Table = toml::from_str("[[rules]]\ndecision = 'deny'\npattern = '*'")?;
        config.insert("permission".into(), toml::Value::Table(rule));
        let config_path = native.join("config.toml");
        let mut file_config = fs::OpenOptions::new();
        file_config.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file_config.mode(0o600);
        }
        let mut file = file_config.open(&config_path)?;
        self.temporary_config = Some(config_path);
        file.write_all(toml::to_string(&config)?.as_bytes())?;
        drop(file);
        let credentials = original.join("credentials");
        if credentials.is_dir() {
            let link = native.join("credentials");
            match fs::symlink_metadata(&link) {
                Ok(_) => ensure!(
                    link.canonicalize()? == credentials.canonicalize()?,
                    "Kimi 凭据目录与现有账号不一致"
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    #[cfg(windows)]
                    junction::create(&credentials, &link)?;
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(credentials.canonicalize()?, &link)?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        let agents = native.join("agents");
        private_dir(&agents)?;
        fs::write(agents.join("agent.md"), KIMI_AGENT)?;
        self.set("KIMI_CODE_HOME", native);
        self.set("KIMI_DISABLE_TELEMETRY", "1");
        Ok(())
    }

    fn opencode(&mut self, model: Option<&str>) -> Result<()> {
        ensure_opencode_managed_config_is_clear()?;
        let mut routes = serde_json::Map::new();
        let config = home_path("XDG_CONFIG_HOME", ".config")?.join("opencode");
        for file in ["config.json", "opencode.json", "opencode.jsonc"] {
            merge_opencode_file(&mut routes, &config.join(file))?;
        }
        if let Some(path) = std::env::var_os("OPENCODE_CONFIG").filter(|v| !v.is_empty()) {
            merge_opencode_file(&mut routes, Path::new(&path))?;
        }
        if let Some(path) = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            for file in ["opencode.json", "opencode.jsonc"] {
                merge_opencode_file(&mut routes, &Path::new(&path).join(file))?;
            }
        }
        if let Ok(text) = std::env::var("OPENCODE_CONFIG_CONTENT") {
            if !text.trim().is_empty() {
                merge_routes(&mut routes, &parse_opencode_config(&text)?)?;
            }
        }
        if let Some(model) = model.filter(|v| !v.trim().is_empty()) {
            routes.insert("model".into(), model.into());
        }
        self.expected_model = routes.get("model").and_then(Value::as_str).map(str::to_owned);
        let selected = routes
            .get("model")
            .and_then(Value::as_str)
            .and_then(|m| m.split_once('/'))
            .map(|(id, _)| id.to_owned());
        restrict_provider_modules(&mut routes, selected.as_deref())?;
        let auth_file = home_path("XDG_DATA_HOME", ".local/share")?.join("opencode/auth.json");
        let raw_auth = match std::env::var("OPENCODE_AUTH_CONTENT") {
            Ok(text) if !text.trim().is_empty() => Some(text),
            _ => match fs::read_to_string(&auth_file) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e).context("无法读取 OpenCode 原生认证"),
            },
        };
        let mut auth = serde_json::Map::new();
        if let Some(text) = raw_auth {
            let source: serde_json::Map<String, Value> =
                serde_json::from_str(&text).map_err(|_| anyhow!("OpenCode 原生认证格式无效"))?;
            for (id, entry) in source {
                if selected.as_ref().is_some_and(|s| s != &id) {
                    continue;
                }
                match entry["type"].as_str() {
                    Some("api") => {
                        auth.insert(id, entry);
                    }
                    Some("oauth" | "wellknown") if selected.as_deref() == Some(id.as_str()) => {
                        bail!("OpenCode 此模型依赖账号插件或托管配置，隔离会话无法安全复用其登录流程")
                    }
                    _ => {}
                }
            }
        }
        self.set("OPENCODE_AUTH_CONTENT", serde_json::to_string(&auth)?);
        let mut config = Value::Object(routes);
        config["agent"] = json!({(&self.agent): {"mode":"primary", "permission":{"*":"deny"}}});
        config["default_agent"] = json!(&self.agent);
        config["permission"] = json!({"*":"deny"});
        config["snapshot"] = false.into();
        config["formatter"] = false.into();
        config["lsp"] = false.into();
        self.set("OPENCODE_CONFIG_CONTENT", serde_json::to_string(&config)?);
        self.set("OPENCODE_DISABLE_PROJECT_CONFIG", "1");
        self.set("OPENCODE_PURE", "1");
        self.set("OPENCODE_SERVER_PASSWORD", uuid::Uuid::new_v4().to_string());
        Ok(())
    }

    fn command(&self, debug: bool) -> Result<Command> {
        let mut cmd = if debug {
            let mut cmd = runtime_command(self.kind)?;
            cmd.args(["debug", "agent", &self.agent, "--pure"]);
            cmd
        } else if self.kind == AgentRuntimeKind::KimiCode {
            crate::acp::kimi_acp_command()?
        } else {
            let mut cmd = crate::acp::opencode_acp_command()?;
            cmd.arg("--pure");
            cmd
        };
        self.configure_command(&mut cmd);
        Ok(cmd)
    }

    fn configure_command(&self, cmd: &mut Command) {
        for key in [
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_PERMISSION",
            "OPENCODE_TEST_MANAGED_CONFIG_DIR",
            "OPENCODE_DB",
            "OPENCODE_PLUGIN_META_FILE",
            "OPENCODE_MODELS_PATH",
            "OPENCODE_MODELS_URL",
            "KIMI_CODE_TRUST_WORKSPACE",
            "NODE_OPTIONS",
            "BUN_OPTIONS",
        ] {
            cmd.env_remove(key);
        }
        cmd.envs(self.env.iter().map(|(k, v)| (k, v))).current_dir(&self.work);
    }
}

// Administrative configuration is loaded after inline config and may install
// MCP servers or override the selected agent. Never bypass an existing policy.
fn ensure_opencode_managed_config_is_clear() -> Result<()> {
    #[cfg(windows)]
    let system = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/ProgramData"))
        .join("opencode");
    #[cfg(target_os = "macos")]
    let system = PathBuf::from("/Library/Application Support/opencode");
    #[cfg(not(any(windows, target_os = "macos")))]
    let system = PathBuf::from("/etc/opencode");
    for dir in
        std::iter::once(system).chain(std::env::var_os("OPENCODE_TEST_MANAGED_CONFIG_DIR").map(PathBuf::from))
    {
        for file in ["opencode.json", "opencode.jsonc"] {
            ensure!(!dir.join(file).try_exists()?, "OpenCode 有设备级托管配置，无法使用隔离 Agent");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let base = Path::new("/Library/Managed Preferences");
        if base.is_dir() {
            ensure!(!base.join("ai.opencode.managed.plist").try_exists()?, "OpenCode 有设备级托管配置");
            for entry in fs::read_dir(base)? {
                let entry = entry?;
                ensure!(
                    !entry.file_type()?.is_dir()
                        || !entry.path().join("ai.opencode.managed.plist").try_exists()?,
                    "OpenCode 有系统账号托管配置，无法使用隔离 Agent"
                );
            }
        }
    }
    Ok(())
}

impl Drop for IsolatedProfile {
    fn drop(&mut self) {
        if let Some(path) = &self.temporary_config {
            let _ = fs::remove_file(path);
        }
    }
}

fn parse_opencode_config(source: &str) -> Result<Value> {
    // OpenCode accepts JSONC. Preserve string bytes (URLs and escaped quotes
    // included) while removing comments and trailing commas outside strings.
    let mut text = source.trim_start_matches('\u{feff}').as_bytes().to_vec();
    let mut quoted = false;
    let mut escaped = false;
    let mut previous = None;
    let mut i = 0;
    while i < text.len() {
        let ch = text[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else if ch == b'"' {
                quoted = false;
            }
        } else if ch == b'/' && text.get(i + 1) == Some(&b'/') {
            while i < text.len() && text[i] != b'\n' {
                text[i] = b' ';
                i += 1;
            }
            continue;
        } else if ch == b'/' && text.get(i + 1) == Some(&b'*') {
            let end = text[i + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .context("OpenCode 配置含未闭合注释")?
                + i
                + 4;
            text[i..end].fill(b' ');
            i = end;
            continue;
        } else {
            if matches!(ch, b'}' | b']') {
                if let Some(index) = previous {
                    if text[index] == b',' {
                        text[index] = b' ';
                    }
                }
            }
            if ch == b'"' {
                quoted = true;
            }
        }
        if !ch.is_ascii_whitespace() {
            previous = Some(i);
        }
        i += 1;
    }
    serde_json::from_slice(&text).map_err(|_| anyhow!("OpenCode 原生模型配置不是有效 JSON/JSONC"))
}

fn merge_routes(routes: &mut serde_json::Map<String, Value>, data: &Value) -> Result<()> {
    fn overlay(target: &mut Value, source: &Value) {
        if let (Some(left), Some(right)) = (target.as_object_mut(), source.as_object()) {
            for (key, value) in right {
                overlay(left.entry(key.clone()).or_insert(Value::Null), value);
            }
        } else {
            *target = source.clone();
        }
    }
    let obj = data.as_object().context("OpenCode 配置必须为对象")?;
    for key in ["model", "provider", "enabled_providers", "disabled_providers"] {
        if let Some(value) = obj.get(key) {
            overlay(routes.entry(key).or_insert(Value::Null), value);
        }
    }
    Ok(())
}

fn merge_opencode_file(routes: &mut serde_json::Map<String, Value>, path: &Path) -> Result<()> {
    match fs::read_to_string(path) {
        Ok(text) => merge_routes(routes, &parse_opencode_config(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("无法读取 OpenCode 模型配置"),
    }
}

fn restrict_provider_modules(
    routes: &mut serde_json::Map<String, Value>,
    selected: Option<&str>,
) -> Result<()> {
    fn check(value: &Value) -> Result<()> {
        match value {
            Value::Object(obj) => {
                if let Some(npm) = obj.get("npm") {
                    ensure!(
                        matches!(
                            npm.as_str(),
                            Some(
                                "@ai-sdk/openai"
                                    | "@ai-sdk/openai-compatible"
                                    | "@ai-sdk/anthropic"
                                    | "@ai-sdk/google"
                                    | "@ai-sdk/azure"
                                    | "@ai-sdk/xai"
                                    | "@ai-sdk/mistral"
                                    | "@ai-sdk/groq"
                                    | "@ai-sdk/deepinfra"
                                    | "@ai-sdk/cerebras"
                                    | "@ai-sdk/cohere"
                                    | "@ai-sdk/gateway"
                                    | "@ai-sdk/togetherai"
                                    | "@ai-sdk/perplexity"
                                    | "@ai-sdk/vercel"
                                    | "@ai-sdk/alibaba"
                                    | "@openrouter/ai-sdk-provider"
                            )
                        ),
                        "OpenCode 的选定模型需要加载未验证的外部 Provider 模块"
                    );
                }
                for value in obj.values() {
                    check(value)?;
                }
            }
            Value::Array(items) => {
                for item in items {
                    check(item)?;
                }
            }
            Value::String(s) => ensure!(!s.contains("{file:"), "隔离的 OpenCode 模型配置不允许宿主文件插值"),
            _ => {}
        }
        Ok(())
    }
    if let Some(providers) = routes.get_mut("provider") {
        let entries = providers.as_object_mut().context("OpenCode provider 配置无效")?;
        if let Some(id) = selected {
            entries.retain(|key, _| key == id);
        }
    }
    for v in routes.values() {
        check(v)?;
    }
    Ok(())
}

fn certify_opencode(profile: &IsolatedProfile, cancelled: &AtomicBool) -> Result<()> {
    let output = run_piped_command(
        profile.command(true)?,
        None,
        cancelled,
        Duration::from_secs(45),
        "核对 OpenCode 有效工具权限",
        |out, err| (spawn_passive_stream_reader(out), spawn_passive_stream_reader(err)),
    )?;
    ensure!(output.status.success(), "OpenCode 无法返回当前受限 Agent 的有效权限，未投递任务");
    let effective: Value = serde_json::from_str(&output.stdout).context("无法解析 OpenCode 有效 Agent")?;
    let tools = effective["tools"].as_object().context("OpenCode 未返回工具权限")?;
    ensure!(
        effective["name"] == profile.agent
            && effective["mode"] == "primary"
            && ["read", "write", "edit", "bash", "task"]
                .iter()
                .all(|key| tools.get(*key) == Some(&Value::Bool(false)))
            && tools.values().all(|value| value.as_bool() == Some(false)),
        "OpenCode 有效 Agent 仍可执行工具，未投递任务"
    );
    let rules = effective["permission"].as_array().context("OpenCode 未返回原生权限规则")?;
    let global = rules
        .iter()
        .rposition(|r| r["permission"] == "*" && r["pattern"] == "*" && r["action"] == "deny")
        .context("OpenCode 未强制拒绝未知工具")?;
    // v1 adds access to its own truncation-output directory. No tool remains
    // enabled, and this narrow rule cannot authorize an MCP, shell or edit.
    ensure!(
        rules[global + 1..].iter().all(|r| r["action"] == "deny"
            || (r["permission"] == "external_directory"
                && r["pattern"]
                    .as_str()
                    .is_some_and(|p| p.replace('\\', "/").ends_with("/opencode/tool-output/*")))),
        "OpenCode 有效授权覆盖了全局工具拒绝规则"
    );
    Ok(())
}

fn kimi_wire(profile: &IsolatedProfile, session: &str) -> Result<PathBuf> {
    ensure!(
        !session.is_empty() && session.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "Kimi 返回了无效会话标识"
    );
    let mut matches = Vec::new();
    for bucket in fs::read_dir(profile.root.join("kimi/sessions"))? {
        let bucket = bucket?;
        if !bucket.file_type()?.is_dir() {
            continue;
        }
        let directory = bucket.path().join(session);
        let wire = directory.join("agents/main/wire.jsonl");
        if !wire.is_file() {
            continue;
        }
        let state: Value = serde_json::from_reader(fs::File::open(directory.join("state.json"))?)?;
        let expected = profile.work.canonicalize()?;
        let recorded = state["cwd"].as_str().context("Kimi 未记录原生工作目录")?;
        ensure!(
            state["id"] == session && Path::new(recorded).canonicalize()? == expected,
            "Kimi 原生会话记录的工作目录或标识不匹配"
        );
        matches.push(wire);
    }
    ensure!(matches.len() == 1, "Kimi 未记录唯一的绑定会话");
    Ok(matches.remove(0))
}

// Kimi 2.6 maps some upstream failures to ACP end_turn. Read native evidence
// for the exact prompt and turn ID appended AFTER the per-attempt file offset.
fn kimi_receipt(path: &Path, offset: u64, prompt: &str) -> Result<Option<(String, String)>> {
    let mut input = fs::File::open(path)?;
    ensure!(input.metadata()?.len() >= offset, "Kimi 原生记录在本轮被截断");
    input.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(input);
    let mut line = String::new();
    let mut turn = None;
    let mut message = None;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || !line.ends_with('\n') {
            return Ok(None);
        }
        let data: Value = serde_json::from_str(&line)?;
        let event = data.get("event").unwrap_or(&data);
        if event["type"] == "turn.prompt" {
            let matches =
                event["input"].as_array().is_some_and(|parts| parts.len() == 1 && parts[0]["text"] == prompt);
            ensure!(matches && turn.is_none(), "Kimi 原生记录含不同输入，无法归属本轮");
            turn = event["turnId"].as_u64();
            message = event["promptId"].as_str().filter(|id| !id.is_empty()).map(str::to_owned);
            ensure!(turn.is_some() && message.is_some(), "Kimi 原生记录缺少消息或轮次标识");
        }
        if event["type"] == "turn.ended" && turn.is_some() && event["turnId"].as_u64() == turn {
            ensure!(event["reason"] == "completed", "Kimi 已结束但未成功完成本轮（{}）", event["reason"]);
            return Ok(message.zip(turn.map(|n| n.to_string())));
        }
    }
}

fn opencode_db(root: &Path) -> Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(
        root.join("data/opencode/opencode.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("无法核对 OpenCode 托管会话的本地回执")
}

fn opencode_original_session(profile: &IsolatedProfile, id: &str, cancelled: &AtomicBool) -> Result<()> {
    // Let the native CLI reopen its own database after the previous process's
    // shutdown. Looking up a missing ID must not create a replacement session.
    let mut cmd = runtime_command(AgentRuntimeKind::OpenCode)?;
    cmd.args(["session", "list", "--format", "json", "--pure"]);
    profile.configure_command(&mut cmd);
    let list = run_piped_command(cmd, None, cancelled, Duration::from_secs(45), "核对原 OpenCode 会话",
        |out,err| (spawn_passive_stream_reader(out),spawn_passive_stream_reader(err)))?;
    ensure!(list.status.success(), "OpenCode 无法列出任务私有会话");
    let items: Vec<Value> = serde_json::from_str(&list.stdout).context("OpenCode 私有会话清单格式不受支持")?;
    let mut matches = items.iter().filter(|row| row["id"]==id);
    let original = matches.next().context("找不到原 OpenCode 托管会话")?;
    let dir = original["directory"].as_str().context("OpenCode 原会话未返回工作目录")?;
    ensure!(matches.next().is_none() && Path::new(dir).canonicalize()? == profile.work.canonicalize()?,
        "原 OpenCode 会话的工作目录或标识不匹配");
    Ok(())
}

fn opencode_receipt(root: &Path, session: &str, prompt: &str) -> Result<(String, String)> {
    let db = opencode_db(root)?;
    let mut query = db.prepare(
        "SELECT DISTINCT u.id, a.id FROM message u JOIN part p ON p.message_id=u.id
           JOIN message a ON a.session_id=u.session_id AND json_extract(a.data,'$.parentID')=u.id
         WHERE u.session_id=?1 AND json_extract(u.data,'$.role')='user'
           AND json_extract(p.data,'$.type')='text' AND json_extract(p.data,'$.text')=?2
           AND json_extract(a.data,'$.role')='assistant' AND json_extract(a.data,'$.error') IS NULL
           AND json_extract(a.data,'$.time.completed') IS NOT NULL AND json_extract(a.data,'$.finish')='stop'")?;
    let mut receipts = query
        .query_map(rusqlite::params![session, prompt], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
    ensure!(receipts.len() == 1, "OpenCode 未持久化此轮唯一的用户消息与成功答复");
    Ok(receipts.remove(0))
}

fn run(request: &RuntimeRequest, cancelled: &AtomicBool, instructions: &str) -> Result<RuntimeResult> {
    ensure!(!cancelled.load(Ordering::Acquire), "托管任务已取消");
    let profile = IsolatedProfile::prepare(request)?;
    let output = protocol_turn(
        &profile,
        request.resume_locator.as_ref(),
        &request.run_id,
        instructions,
        request.agent.model.as_deref(),
        cancelled,
        |phase, evidence| request.checkpoint_managed_turn(phase, evidence),
    )?;
    let locator = request.store.latest_session_locator(&request.task.id)?.context("缺少本轮托管回执")?;
    ensure!(
        locator["sessionId"] == output.0
            && locator["commandId"] == request.run_id
            && locator["managedTurn"] == "completed",
        "本次任务与保存的原生回执不一致"
    );
    Ok(RuntimeResult { summary: output.1, session_locator: locator })
}

fn protocol_turn(
    profile: &IsolatedProfile,
    resume: Option<&Value>,
    run_id: &str,
    instructions: &str,
    model: Option<&str>,
    cancelled: &AtomicBool,
    mut checkpoint: impl FnMut(&str, Value) -> Result<()>,
) -> Result<(String, String)> {
    let kind = if profile.kind == AgentRuntimeKind::KimiCode {
        ManagedAcpKind::Kimi
    } else {
        ManagedAcpKind::OpenCode
    };
    let previous = resume.and_then(|loc| loc["sessionId"].as_str());
    let work = external_command_path(&profile.work);
    let cwd = work.to_str().context("托管运行目录无法转为 UTF-8")?;
    let engine = if kind == ManagedAcpKind::Kimi { "kimi-code-acp" } else { "opencode-acp" };
    if let (Some(previous), Some(locator)) = (previous, resume) {
        ensure!(
            !previous.is_empty()
                && locator["engine"] == engine
                && locator["toolPolicy"] == POLICY
                && locator["sessionPath"] == cwd,
            "原会话缺少托管引擎、工作目录或工具边界，拒绝切换到普通会话"
        );
        if kind == ManagedAcpKind::Kimi {
            kimi_wire(profile, previous)?;
        } else {
            opencode_original_session(profile, previous, cancelled)?;
        }
    }
    if kind == ManagedAcpKind::OpenCode {
        certify_opencode(profile, cancelled)?;
    }
    let mut connection = AcpConnection::spawn_for_with_cli_identity(
        kind,
        cwd,
        previous,
        vec![],
        &[],
        false,
        false,
        Arc::new(AtomicU32::new(0)),
        None,
        Some(profile.command(false)?),
        None,
    )?;
    let result = (|| -> Result<(String, String)> {
        let id = connection.session_id().to_owned();
        ensure!(
            previous.is_none_or(|p| connection.resumed() && p == id),
            "原生 Runtime 未恢复指定会话，未投递任务"
        );
        if kind == ManagedAcpKind::OpenCode {
            let config = connection.set_config_option("mode", &profile.agent)?;
            ensure!(
                config.iter().any(|value| value["id"] == "mode" && value["currentValue"] == profile.agent),
                "OpenCode 未确认使用已核实的受限 Agent"
            );
        }
        if let Some(model) = model.filter(|s| !s.trim().is_empty()).or(profile.expected_model.as_deref()) {
            let options = connection.set_config_option("model", model)?;
            ensure!(
                options.iter().any(|o| o["id"] == "model" && o["currentValue"] == model),
                "Runtime 未确认所选模型，未投递任务"
            );
        }
        let wire = if kind == ManagedAcpKind::Kimi {
            let wire = kimi_wire(profile, &id)?;
            let offset = fs::metadata(&wire)?.len();
            Some((wire, offset))
        } else {
            None
        };
        let key = format!("CrewVela 托管轮次 {run_id}。");
        let prompt = format!("{key}\n{}", instructions.trim_end());
        let mut evidence = json!({"sessionId":id,"transport":TRANSPORT,"engine":engine,"sessionPath":cwd,
            "toolPolicy":POLICY,"permissionMode":"text-only","commandId":run_id});
        ensure!(!cancelled.load(Ordering::Acquire), "原生会话就绪前托管任务已取消");
        checkpoint("dispatched", evidence.clone())?;
        let reply = prompt_with_native_cancel(&mut connection, &prompt, cancelled)?;
        ensure!(!reply.cancelled, "原生会话 {id} 已返回取消回执 cancelled");
        ensure!(!cancelled.load(Ordering::Acquire), "取消期间原生轮次已结束，未交付结果");
        ensure!(
            reply.stop_reason.as_deref() == Some("end_turn"),
            "原生托管轮次未正常结束（{}）",
            reply.stop_reason.as_deref().unwrap_or("无结束回执")
        );
        ensure!(!reply.text.trim().is_empty(), "原生托管轮次没有产生答复");
        if let Some((wire, offset)) = wire {
            let deadline = Instant::now() + Duration::from_secs(3);
            let (message, turn_id) = loop {
                if let Some(id) = kimi_receipt(&wire, offset, &prompt)? {
                    break id;
                }
                ensure!(
                    !cancelled.load(Ordering::Acquire) && Instant::now() < deadline,
                    "Kimi 未持久化匹配输入的完成回执"
                );
                std::thread::sleep(Duration::from_millis(30));
            };
            evidence["messageId"] = message.into();
            evidence["turnId"] = turn_id.into();
        } else {
            let (user, assistant) = opencode_receipt(&profile.root, &id, &prompt)?;
            evidence["messageId"] = user.into();
            evidence["turnId"] = assistant.into();
        }
        // The matching successful JSON-RPC reply proves acceptance even when
        // the runtime sent no user-message echo. A bare stdin write cannot.
        checkpoint("accepted", evidence.clone())?;
        checkpoint("completed", evidence)?;
        Ok((id, reply.text))
    })();
    connection.stop_checked()?;
    result
}

fn prompt_with_native_cancel(
    connection: &mut AcpConnection,
    prompt: &str,
    cancelled: &AtomicBool,
) -> Result<crate::acp::AcpPromptResult> {
    // ACP cancel is a notification. Its acknowledgement is the pending
    // prompt's cancelled result. Give it time to arrive, then stop the owned
    // process if the native runtime does not respond.
    let cancel = connection.cancel_handle();
    let fallback = AtomicBool::new(false);
    let (finished, wait) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let flag = &fallback;
        let watcher = scope.spawn(move || loop {
            if cancelled.load(Ordering::Acquire) {
                if cancel.cancel().is_err()
                    || matches!(
                        wait.recv_timeout(Duration::from_secs(2)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    )
                {
                    flag.store(true, Ordering::Release);
                }
                break;
            }
            if !matches!(
                wait.recv_timeout(Duration::from_millis(40)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                break;
            }
        });
        let result = connection.prompt_interactive(prompt, &fallback);
        drop(finished);
        watcher.join().map_err(|_| anyhow!("原生取消监视线程异常"))?;
        if fallback.load(Ordering::Acquire) && cancelled.load(Ordering::Acquire) {
            bail!("原生会话未确认取消，正在回收本轮进程");
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn policies_drop_native_hooks_and_external_modules_and_never_authorize_writes() -> Result<()> {
        for kind in [AgentRuntimeKind::KimiCode, AgentRuntimeKind::OpenCode] {
            assert!(supports(kind, ManagedUse::Plan) && supports(kind, ManagedUse::Review));
            assert!(!supports(kind, ManagedUse::Write));
        }
        let original = parse_opencode_config(
            r#"{
            // fixture with JSONC comments and trailing commas
            "model":"local/example", "provider":{"local":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://localhost/"}},},
            "mcp":{"extra":{}}, "plugin":["untrusted"], "tools":{"bash":true}
        }"#,
        )?;
        let mut config = serde_json::Map::new();
        merge_routes(&mut config, &original)?;
        restrict_provider_modules(&mut config, Some("local"))?;
        assert_eq!(config.len(), 2);
        config.get_mut("provider").unwrap()["local"]["npm"] = "./dynamic.js".into();
        assert!(restrict_provider_modules(&mut config, Some("local")).is_err());
        let root = std::env::temp_dir();
        assert!(validate_diff_paths(b"2\t0\tsrc/lib.rs\0", &root, &[]).is_ok());
        assert!(validate_diff_paths(b"2\t0\tkeys/.env\0", &root, &[]).is_err());
        assert!(validate_diff_paths(b"-\t-\tpicture.png\0", &root, &[]).is_err());
        assert!(validate_diff_paths(b"0\t2\tprivate/deleted.txt\0", &root, &["private"]).is_err());
        assert!(validate_diff_paths(b"1\t2\tprivate-other/file\0", &root, &["private"]).is_ok());
        Ok(())
    }

    #[test]
    fn kimi_requires_the_matching_turn_input_and_a_successful_native_reason() -> Result<()> {
        let file = std::env::temp_dir().join(format!("cv-kimi-{}.jsonl", uuid::Uuid::new_v4()));
        let prefix = b"{\"type\":\"turn.ended\",\"turnId\":3,\"reason\":\"completed\"}\n";
        let input = json!({"type":"turn.prompt","input":[{"type":"text","text":"actual run"}],"turnId":4,"promptId":"message-four"});
        let run = || -> Result<()> {
            for reason in ["failed", "cancelled", "completed"] {
                let mut out = fs::File::create(&file)?;
                out.write_all(prefix)?;
                writeln!(out, "{}", json!({"event":input}))?;
                writeln!(out, "{}", json!({"type":"turn.ended","turnId":4,"reason":reason}))?;
                assert!(kimi_receipt(&file, prefix.len() as u64, "other input").is_err());
                let receipt = kimi_receipt(&file, prefix.len() as u64, "actual run");
                if reason == "completed" {
                    assert_eq!(receipt?, Some(("message-four".into(), "4".into())));
                } else {
                    assert!(receipt.is_err());
                }
            }
            Ok(())
        }();
        fs::remove_file(file)?;
        run
    }

    // This server emulates only a streaming model API. The executables,
    // permission enforcement, persistence, cancel and ACP replies are native.
    const LOOPBACK: &str = r#"
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const root=process.argv[1]; const server=http.createServer(async(req,res)=>{
  let raw='';for await(const c of req)raw+=c;
  if(!raw){res.writeHead(200,{'content-type':'application/json'});return res.end(JSON.stringify({data:[]}));}
  const body=JSON.parse(raw),msgs=body.messages||[];
  const task=msgs.findLastIndex(m=>m.role==='user'&&JSON.stringify(m.content).includes('CrewVela 托管轮次'));
  const q=JSON.stringify(msgs[task]?.content)||'';
  const names=(body.tools||[]).map(t=>t.function?.name);
  const results=msgs.slice(task+1).filter(m=>m.role==='tool').map(m=>JSON.stringify(m.content));
  const wantsTools=q.includes('crewvela-deny-read-write')&&q.includes('kimi_code')&&results.length===0;
  fs.appendFileSync(path.join(root,'fixture-requests'),JSON.stringify({foundTask:task>=0,tools:names,
    attempted:wantsTools,results:results.map(r=>r.slice(0,180))})+'\n');
  if(q.includes('opencode')&&names.length) fs.writeFileSync(path.join(root,'open-tools'),names.join(','));
  if(results.length===5&&['Read','Write','Edit','Bash','mcp__unsafe__probe'].every(name=>results.some(x=>x.includes(name)&&x.includes('ERROR: Tool execution failed')
      &&(x.includes('disabled by the active tool policy')||x.includes('denied by permission rule')||x.includes('not found')))))
    fs.writeFileSync(path.join(root,'denied'),String(results.length));
  if(q.includes('crewvela-wait-cancel')) {fs.writeFileSync(path.join(root,'running'),'yes'); return;}
  res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
  const emit=(delta,reason)=>res.write('data: '+JSON.stringify({id:'chatcmpl-probe',created:1,object:'chat.completion.chunk',model:'probe',choices:[{index:0,delta,finish_reason:reason}]})+'\n\n');
  if(wantsTools){
    const canary=path.join(root,'canary.txt');
    const calls=[['Read',{path:canary}],['Write',{path:canary,content:'changed'}],
      ['Edit',{path:canary,old_string:'untouched',new_string:'changed'}],['Bash',{command:'echo CREWVELA_FIXTURE_SHELL'}],
      ['mcp__unsafe__probe',{}]].map(([name,args],index)=>({index,id:'tool-'+index,type:'function',function:{name,arguments:JSON.stringify(args)}}));
    emit({role:'assistant',tool_calls:calls},null);emit({},'tool_calls');
  } else {emit({role:'assistant',content:'CREWVELA_PROTOCOL_PROOF'},null);emit({},'stop');}
  res.end('data: [DONE]\n\n');
}); server.listen(0,'127.0.0.1',()=>console.log(server.address().port));
"#;

    #[test]
    #[ignore = "需要本机安装 Kimi Code、OpenCode、Node；模型 API 由私有回环测试服务模拟"]
    fn installed_kimi_and_opencode_enforce_text_only_and_resume_exact_acp_sessions() -> Result<()> {
        const PROBE: &str = "CREWVELA_MANAGED_ACP_PROBE";
        if let Some(value) = std::env::var_os(PROBE) {
            let root = PathBuf::from(value);
            ensure!(
                root.starts_with(std::env::temp_dir())
                    && fs::read(root.join("marker"))? == b"isolated-managed-acp",
                "验收目录不正确"
            );
            return native_test(&root);
        }
        let root = std::env::temp_dir().join(format!("cv-acp-{}", uuid::Uuid::new_v4()));
        private_dir(&root)?;
        fs::write(root.join("marker"), b"isolated-managed-acp")?;
        let result = hidden_command(std::env::current_exe()?)
            .args(["--exact","multi_agent::executor::managed_cli::tests::installed_kimi_and_opencode_enforce_text_only_and_resume_exact_acp_sessions",
                "--ignored","--nocapture"]).env(PROBE,&root)
            .env("KIMI_CODE_HOME",root.join("original-kimi")).env("XDG_CONFIG_HOME",root.join("original-config"))
            .env("XDG_DATA_HOME",root.join("original-data")).env("OPENCODE_AUTH_CONTENT","{}")
            .env_remove("OPENCODE_CONFIG_CONTENT").env_remove("OPENCODE_CONFIG").env_remove("OPENCODE_CONFIG_DIR")
            .env_remove("KIMI_MODEL_NAME").env("OPENCODE_DISABLE_MODELS_FETCH","1").status()?;
        ensure!(
            root.canonicalize()?.parent() == Some(std::env::temp_dir().canonicalize()?.as_path())
                && fs::read(root.join("marker"))? == b"isolated-managed-acp",
            "测试临时目录与创建位置不一致"
        );
        fs::remove_dir_all(&root)?;
        ensure!(result.success(), "原生 Runtime 回环验收失败");
        Ok(())
    }

    fn native_test(root: &Path) -> Result<()> {
        crate::test_profile::initialize(&root.join("app"))?;
        let mut command = hidden_command("node");
        command.args(["-e", LOOPBACK]).arg(root).stdout(Stdio::piped()).stderr(Stdio::null());
        let (mut server, _identity) = crate::acp::spawn_owned_command(&mut command)?;
        let mut port = String::new();
        BufReader::new(server.stdout.take().unwrap()).read_line(&mut port)?;
        let url = format!("http://127.0.0.1:{}/v1", port.trim().parse::<u16>()?);
        let original = root.join("original-kimi");
        private_dir(&original)?;
        fs::write(original.join("config.toml"),format!("default_model='probe'\n[providers.probe]\ntype='openai'\nbase_url='{url}'\napi_key='fixture'\n[models.probe]\nprovider='probe'\nmodel='probe'\nmax_context_size=8192\n"))?;
        fs::write(
            original.join("mcp.json"),
            json!({"mcpServers":{"unsafe":{"command":"node","args":["-e","process.exit(1)"]}}}).to_string(),
        )?;
        let oc = root.join("original-config/opencode");
        private_dir(&oc)?;
        fs::write(oc.join("opencode.json"), json!({"model":"probe/probe","provider":{"probe":{
            "npm":"@ai-sdk/openai-compatible","options":{"baseURL":url,"apiKey":"fixture"},"models":{"probe":{"name":"probe"}}}},
            "agent":{"crewvela-managed-probe":{"mode":"primary","permission":{"*":"allow"}}},
            "mcp":{"unsafe":{"type":"local","command":["node","-e","process.exit(1)"]}}}).to_string())?;
        fs::write(root.join("canary.txt"), "untouched")?;
        let result = (|| -> Result<()> {
            for kind in [AgentRuntimeKind::KimiCode, AgentRuntimeKind::OpenCode] {
                let directory = root.join(kind.as_str());
                let mut locator = None;
                for stage in ["first-crewvela-deny-read-write", "resumed-crewvela-deny-read-write"] {
                    let _ = fs::remove_file(root.join("denied"));
                    let profile =
                        IsolatedProfile::at(kind, directory.clone(), "crewvela-managed-probe".into(), None)?;
                    let mut phases = Vec::new();
                    let mut saved = None;
                    let input = format!("{stage} {}", kind.as_str());
                    let (id, text) = protocol_turn(
                        &profile,
                        locator.as_ref(),
                        stage,
                        &input,
                        None,
                        &AtomicBool::new(false),
                        |phase, record| {
                            phases.push(phase.to_owned());
                            saved = Some(record);
                            Ok(())
                        },
                    )?;
                    assert_eq!(phases, ["dispatched", "accepted", "completed"]);
                    assert_eq!(text, "CREWVELA_PROTOCOL_PROOF");
                    if kind == AgentRuntimeKind::KimiCode {
                        ensure!(
                            fs::read_to_string(root.join("denied")).ok().as_deref() == Some("5"),
                            "Kimi 未返回完整工具拒绝记录：{}",
                            fs::read_to_string(root.join("fixture-requests"))?
                        );
                    } else {
                        ensure!(!root.join("open-tools").exists(), "OpenCode 仍向模型暴露工具");
                    }
                    if let Some(old) = locator {
                        assert_eq!(old["sessionId"], id);
                    }
                    locator = saved;
                }
                assert_eq!(fs::read(root.join("canary.txt"))?, b"untouched");
                let profile = IsolatedProfile::at(kind, directory, "crewvela-managed-probe".into(), None)?;
                let mut missing = locator.clone().unwrap();
                missing["sessionId"] = format!("nosuch_{}", uuid::Uuid::new_v4().simple()).into();
                let mut dispatched = false;
                let rejected = protocol_turn(
                    &profile,
                    Some(&missing),
                    "cannot-resume",
                    "never submit",
                    None,
                    &AtomicBool::new(false),
                    |_, _| {
                        dispatched = true;
                        Ok(())
                    },
                );
                ensure!(rejected.is_err() && !dispatched, "原会话丢失时提交了新轮次");
                let flag = AtomicBool::new(false);
                let marker = root.join("running");
                let _ = fs::remove_file(&marker);
                let outcome = std::thread::scope(|scope| {
                    scope.spawn(|| {
                        let deadline = Instant::now() + Duration::from_secs(15);
                        while !marker.is_file() && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        flag.store(true, Ordering::Release);
                    });
                    protocol_turn(
                        &profile,
                        locator.as_ref(),
                        "cancel",
                        "crewvela-wait-cancel",
                        None,
                        &flag,
                        |phase, _| {
                            ensure!(phase == "dispatched", "取消轮次未完成，不应登记完成");
                            Ok(())
                        },
                    )
                });
                let message = format!("{:#}", outcome.unwrap_err());
                ensure!(message.contains("取消回执 cancelled"), "{kind:?} 没有返回精确取消回执：{message}");
            }
            Ok(())
        })();
        crate::acp::stop_child_checked(&mut server)?;
        result
    }
}
