//! Read-only configuration and capture diagnostics; never infer live hooks from history.
use super::*;
use crate::mcp::{McpServerConfig, McpTransport};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub state: &'static str,
    pub detail: String,
}
fn item(state: &'static str, detail: impl Into<String>) -> CheckItem {
    CheckItem {
        state,
        detail: detail.into(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub agent: String,
    pub project_dir: String,
    pub mcp: CheckItem,
    pub hooks: CheckItem,
    pub tools: CheckItem,
    pub capture: CheckItem,
}

pub(super) fn read_config(path: &Path) -> Result<Option<Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("无法读取 {}", path.display())),
    };
    let text = text.trim_start_matches('\u{feff}');
    if path.extension().is_some_and(|ext| ext == "toml") {
        let value: toml::Value =
            toml::from_str(text).with_context(|| format!("TOML 格式错误：{}", path.display()))?;
        Ok(Some(serde_json::to_value(value)?))
    } else {
        Ok(Some(serde_json::from_str(text).with_context(|| {
            format!("JSON 格式错误：{}", path.display())
        })?))
    }
}

fn agent_home(home: &Path, variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(default))
}

fn opencode_root(home: &Path) -> PathBuf {
    let default = agent_home(home, "XDG_CONFIG_HOME", ".config").join("opencode");
    std::env::var_os("OPENCODE_CONFIG_DIR").filter(|p| !p.is_empty()).map(PathBuf::from).unwrap_or(default)
}

fn plugin_paths(root: &Path) -> [PathBuf; 2] {
    [root.join("plugins/ai-memory.ts"), root.join("plugins/ai-memory-opencode2.ts")]
}

pub(super) fn opencode_plugins() -> Result<[PathBuf; 2]> {
    Ok(plugin_paths(&opencode_root(&dirs::home_dir().context("无法定位用户目录")?)))
}

pub(super) fn default_opencode_plugins() -> Result<[PathBuf; 2]> {
    Ok(plugin_paths(&dirs::home_dir().context("无法定位用户目录")?.join(".config/opencode")))
}

pub(super) fn config_paths(agent: SetupAgent, project: &Path) -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("无法定位用户目录")?;
    Ok(match agent {
        SetupAgent::ClaudeCode => (
            home.join(".claude.json"),
            project.join(".claude/settings.local.json"),
        ),
        SetupAgent::Codex => {
            let root = agent_home(&home, "CODEX_HOME", ".codex");
            (root.join("config.toml"), root.join("hooks.json"))
        }
        SetupAgent::KimiCode => {
            let root = agent_home(&home, "KIMI_CODE_HOME", ".kimi-code");
            (root.join("mcp.json"), root.join("config.toml"))
        }
        SetupAgent::Zcode => {
            let path = home.join(".zcode/cli/config.json");
            (path.clone(), path)
        }
        SetupAgent::Pi => {
            let extension = agent_home(&home, "PI_CODING_AGENT_DIR", ".pi/agent").join("extensions/ai-memory-pi.ts");
            (extension.clone(), extension)
        }
        SetupAgent::Opencode => {
            let root = opencode_root(&home);
            let config = std::env::var_os("OPENCODE_CONFIG").filter(|p| !p.is_empty()).map(PathBuf::from)
                .unwrap_or_else(|| {
                    let jsonc = root.join("opencode.jsonc");
                    if jsonc.exists() { jsonc } else { root.join("opencode.json") }
                });
            let [v1, v2] = plugin_paths(&root);
            (config, if v2.exists() { v2 } else { v1 })
        }
        SetupAgent::AntigravityCli => {
            let root = home.join(".gemini/config");
            (root.join("mcp_config.json"), root.join("hooks.json"))
        }
    })
}

fn mcp_entry<'a>(config: &'a Value, agent: SetupAgent) -> Option<&'a Value> {
    match agent {
        SetupAgent::Codex => config.pointer("/mcp_servers/ai-memory"),
        SetupAgent::Zcode => config.pointer("/mcp/servers/ai-memory"),
        SetupAgent::Opencode => config.pointer("/mcp/servers/ai-memory").or_else(|| config.pointer("/mcp/ai-memory")),
        _ => config.pointer("/mcpServers/ai-memory"),
    }
}

pub(super) fn endpoint_matches(actual: &str, base: &str) -> bool {
    let Ok(actual) = reqwest::Url::parse(actual) else {
        return false;
    };
    let Ok(mut expected) = reqwest::Url::parse(base) else {
        return false;
    };
    expected.set_path(&format!("{}/mcp", expected.path().trim_end_matches('/')));
    actual.scheme() == expected.scheme()
        && actual.host_str() == expected.host_str()
        && actual.port_or_known_default() == expected.port_or_known_default()
        && actual.path().trim_end_matches('/') == expected.path()
}

fn registration(config: Option<&Value>, agent: SetupAgent, url: &str) -> CheckItem {
    let Some(config) = config else {
        return item("missing", "未找到 MCP 配置文件");
    };
    let Some(entry) = mcp_entry(config, agent) else {
        return item("missing", "未注册 ai-memory MCP");
    };
    if entry["enabled"] == Value::Bool(false) || entry["disabled"] == Value::Bool(true) {
        return item("failed", "ai-memory MCP 配置已禁用");
    }
    if let Some(endpoint) = entry_url(entry, agent) {
        return if endpoint_matches(endpoint, url) {
            item("ok", "已注册当前记忆服务（仅检查配置文件）")
        } else {
            item("mismatch", "MCP 地址与当前记忆服务不一致")
        };
    }
    // Custom stdio bridges may route/authenticate dynamically; do not execute them.
    item(
        "unknown",
        "发现 stdio/自定义 MCP 配置；未执行桥接命令，需在智能体会话中验证",
    )
}

fn entry_url(entry: &Value, agent: SetupAgent) -> Option<&str> {
    if matches!(agent, SetupAgent::AntigravityCli) { entry["serverUrl"].as_str() } else { entry["url"].as_str() }
}

pub(super) fn guard_existing_registration(path: &Path, agent: SetupAgent, url: &str) -> Result<()> {
    let Some(config) = read_config(path)? else { return Ok(()) };
    let entries = [mcp_entry(&config, agent), matches!(agent, SetupAgent::Opencode).then(|| config.pointer("/mcp/ai-memory")).flatten()];
    for entry in entries.into_iter().flatten() {
        if entry["enabled"] == false || entry["disabled"] == true || !entry_url(entry, agent).is_some_and(|endpoint| endpoint_matches(endpoint, url)) {
            bail!("{} 中已有其他 ai-memory 注册或该注册已禁用，保留原配置", path.display());
        }
    }
    Ok(())
}

fn generated_hook_state(path: &Path, agent: SetupAgent, url: &str) -> Result<CheckItem> {
    let source = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(item("missing", "未找到 ai-memory 扩展或插件")),
        Err(error) => return Err(error).with_context(|| format!("无法读取 {}", path.display())),
    };
    let source = source.trim_start_matches('\u{feff}');
    let agent_marker = if matches!(agent, SetupAgent::Pi) { "pi" } else if path.ends_with("ai-memory-opencode2.ts") { "opencode2" } else { "opencode" };
    let header = format!("// Auto-generated by `ai-memory install-hooks --agent {agent_marker} --apply`");
    let endpoint = source.lines().find_map(|line| line.trim_start().strip_prefix("const SERVER = "))
        .and_then(|literal| literal.split_once(".replace("))
        .and_then(|(literal, _)| serde_json::from_str::<String>(literal).ok());
    Ok(if !source.starts_with(&header) || endpoint.is_none() {
        item("unknown", "发现自定义扩展或插件，需要确认其来源")
    } else if endpoint.as_deref().map(|value| value.trim_end_matches('/')) != Some(url.trim_end_matches('/')) {
        item("mismatch", "扩展或插件指向其他记忆服务")
    } else {
        item("ok", "已找到当前服务的扩展或插件；需在新会话验证加载")
    })
}

pub(super) fn guard_generated_hook(path: &Path, agent: SetupAgent, url: &str) -> Result<()> {
    let state = generated_hook_state(path, agent, url)?;
    if !matches!(state.state, "missing" | "ok") {
        bail!("{}：{}；保留原配置", path.display(), state.detail);
    }
    Ok(())
}

pub(super) fn guard_antigravity_hooks(path: &Path, url: &str) -> Result<()> {
    if let Some(group) = read_config(path)?.as_ref().and_then(|value| value.get("ai-memory")) {
        if !contains_hook(group, url) {
            bail!("{} 中已有自定义或指向其他服务的 ai-memory 钩子，保留原配置", path.display());
        }
    }
    Ok(())
}

fn contains_hook(value: &Value, url: &str) -> bool {
    match value {
        Value::Object(fields) => {
            if fields.get("enabled") == Some(&Value::Bool(false)) {
                return false;
            }
            let command = fields
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = fields.get("args").map(Value::to_string).unwrap_or_default();
            ((command.contains("ai-memory") || args.contains("ai-memory-project-hook.ps1"))
                && (has_service_url(command, url) || has_service_url(&args, url)))
                || fields.values().any(|value| contains_hook(value, url))
        }
        Value::Array(values) => values.iter().any(|value| contains_hook(value, url)),
        _ => false,
    }
}

fn hooks_configuration(config: Option<&Value>, url: &str) -> CheckItem {
    let Some(config) = config else {
        return item("missing", "未找到钩子配置文件");
    };
    let Some(hooks) = config.get("hooks") else {
        return item("missing", "未找到钩子配置");
    };
    if hooks.get("enabled") == Some(&Value::Bool(false)) {
        return item("failed", "钩子配置已禁用");
    }
    if contains_hook(hooks, url) {
        item(
            "ok",
            "发现指向当前服务的钩子配置；事件完整性、信任与实际运行待验证",
        )
    } else {
        item(
            "unknown",
            "未发现指向当前服务的 ai-memory 钩子；自定义脚本需手动核实",
        )
    }
}

fn has_service_url(text: &str, expected: &str) -> bool {
    let Ok(expected) = reqwest::Url::parse(expected.trim_end_matches('/')) else {
        return false;
    };
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '\\' | '[' | ']' | ','))
        .filter_map(|token| reqwest::Url::parse(token.trim_end_matches('/')).ok())
        .any(|actual| actual == expected)
}

fn kimi_hooks_configuration(config: Option<&Value>, url: &str) -> CheckItem {
    let base = hooks_configuration(config, url);
    if base.state != "ok" { return base; }
    let Some(entries) = config.and_then(|v| v["hooks"].as_array()) else {
        return item("unknown", "无法识别 Kimi 事件配置，请在新会话中验证捕获");
    };
    let missing: Vec<_> = ["SessionStart", "PostToolUse", "SessionEnd"].into_iter()
        .filter(|event| !entries.iter().any(|entry| entry["event"] == *event && contains_hook(entry, url)))
        .collect();
    if missing.is_empty() { base } else {
        item("failed", format!("Kimi 缺少当前服务的捕获事件：{}", missing.join("、")))
    }
}

fn capture_report(report: &Value, agent: SetupAgent, url: &str) -> CheckItem {
    if report["server"].as_str().map(|s| s.trim_end_matches('/')) != Some(url.trim_end_matches('/'))
    {
        return item("mismatch", "项目诊断指向其他记忆服务，未使用其捕获结果");
    }
    for key in ["capture_coverage_problem", "marker_capture_problem"] {
        if !report.get(key).is_some_and(Value::is_null) {
            return item(
                "failed",
                "项目捕获诊断异常或不完整，请检查项目标记及服务配置",
            );
        }
    }
    if !report
        .pointer("/project_coordinate/server_problem")
        .is_some_and(Value::is_null)
    {
        return item("failed", "项目坐标诊断异常或不完整");
    }
    let Some(row) = report["rows"]
        .as_array()
        .and_then(|rows| rows.iter().filter(|row| row["agent"] == agent.id() ||
            (matches!(agent, SetupAgent::Opencode) && matches!(row["agent"].as_str(), Some("open-code" | "opencode2"))))
            .max_by_key(|row| row["captured"].as_u64().unwrap_or(0)))
    else {
        return item("unknown", "诊断未返回此智能体的捕获信息");
    };
    if row["uncaptured"] == Value::Bool(true) {
        return item(
            "failed",
            "此项目存在近期会话但没有捕获记录；请核实钩子与捕获策略",
        );
    }
    match row["captured"].as_u64() {
        Some(n) if n > 0 => item(
            "pending",
            format!("发现历史捕获记录（{n}）；可能含回填，当前新会话钩子仍待验证"),
        ),
        Some(_) => item("pending", "尚无捕获记录；请在此项目完成一次新会话后检查"),
        None => item("unknown", "诊断未返回有效捕获计数"),
    }
}

pub async fn ai_memory_check(
    agent: SetupAgent,
    project_dir: String,
) -> Result<CheckReport, String> {
    check(agent, project_dir)
        .await
        .map_err(|error| format!("{error:#}"))
}

async fn check(agent: SetupAgent, project_dir: String) -> Result<CheckReport> {
    let selected = PathBuf::from(&project_dir);
    if !selected.is_absolute() || !selected.is_dir() {
        bail!("请选择存在的绝对项目目录");
    }
    let (executable, _) = locate_executable().context("未找到 ai-memory，请先安装")?;
    let data = data_dir()?;
    let url = resolved_server_url()?;
    let config_url = url.clone();
    let checks = tokio::task::spawn_blocking(move || -> Result<_> {
        let (mcp_path, hooks_path) = config_paths(agent, &selected)?;
        let config_check = |path: &Path, hooks: bool| {
            if matches!(agent, SetupAgent::Pi) || (hooks && matches!(agent, SetupAgent::Opencode)) {
                return generated_hook_state(path, agent, &config_url)
                    .unwrap_or_else(|error| item("failed", format!("{error:#}")));
            }
            match read_config(path) {
                Ok(value) => {
                    if hooks && matches!(agent, SetupAgent::KimiCode) {
                        kimi_hooks_configuration(value.as_ref(), &config_url)
                    } else if hooks && matches!(agent, SetupAgent::AntigravityCli) {
                        match value.as_ref().and_then(|config| config.get("ai-memory")) {
                            Some(group) if contains_hook(group, &config_url) => item("ok", "发现指向当前服务的 Antigravity 钩子；实际运行待验证"),
                            Some(_) => item("mismatch", "Antigravity 的 ai-memory 钩子没有指向当前服务"),
                            None => item("missing", "未找到 Antigravity 的 ai-memory 钩子"),
                        }
                    } else if hooks {
                        hooks_configuration(value.as_ref(), &config_url)
                    } else {
                        registration(value.as_ref(), agent, &config_url)
                    }
                }
                Err(error) => item("failed", format!("{error:#}")),
            }
        };
        let mcp = config_check(&mcp_path, false);
        let hooks = config_check(&hooks_path, true);
        let mut command = Command::new(executable);
        command
            .arg("--data-dir")
            .arg(data)
            .args(["doctor", "--json"])
            .env("AI_MEMORY_SERVER_URL", &config_url)
            .current_dir(selected)
            .creation_flags_no_window();
        let capture = match run_setup_command(command, Duration::from_secs(20)) {
            Ok(result) if result.success && !result.output_truncated => {
                match serde_json::from_str(&result.stdout) {
                    Ok(value) => capture_report(&value, agent, &config_url),
                    Err(_) => item("failed", "项目诊断返回无效 JSON"),
                }
            }
            Ok(result) => item(
                "failed",
                if result.timed_out {
                    "项目诊断超时".into()
                } else {
                    format!(
                        "项目诊断失败（退出码 {:?}）：{}",
                        result.exit_code,
                        result.stderr.chars().take(1000).collect::<String>()
                    )
                },
            ),
            Err(error) => item("failed", format!("无法运行项目诊断：{error:#}")),
        };
        Ok((mcp, hooks, capture))
    })
    .await??;
    let server = McpServerConfig {
        id: "ai-memory-check".into(),
        name: "ai-memory".into(),
        transport: McpTransport::Http,
        command: None,
        args: vec![],
        env: Default::default(),
        url: Some(format!("{}/mcp", url.trim_end_matches('/'))),
        headers: auth::headers(&data_dir()?, &url)?,
        enabled: true,
        source_plugin: None,
        created_at: 0,
        updated_at: 0,
    };
    let probe = crate::mcp::probe_remote(&server).await;
    let tools = if probe.ok && !probe.tools.is_empty() {
        item(
            "ok",
            format!(
                "记忆服务工具可用（{} 个）；智能体会话加载仍待验证",
                probe.tools.len()
            ),
        )
    } else {
        item(
            "failed",
            probe
                .error
                .unwrap_or_else(|| "服务未返回工具；启用认证时需核实认证配置".into()),
        )
    };
    Ok(CheckReport {
        agent: agent.id().into(),
        project_dir,
        mcp: checks.0,
        hooks: checks.1,
        capture: checks.2,
        tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_integrations_preserve_custom_hooks_and_other_memory_registrations() {
        let directory = std::env::temp_dir().join(format!("crewvela-memory-target-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let url = "http://127.0.0.1:47374";
        let pi = directory.join("ai-memory-pi.ts");
        assert!(guard_generated_hook(&pi, SetupAgent::Pi, url).is_ok());
        let custom = "// user extension\nexport default () => {};\n";
        std::fs::write(&pi, custom).unwrap();
        assert!(guard_generated_hook(&pi, SetupAgent::Pi, url).is_err());
        assert_eq!(std::fs::read_to_string(&pi).unwrap(), custom);
        let generated = format!("// Auto-generated by `ai-memory install-hooks --agent pi --apply`.\nconst SERVER = {url:?}.replace(/\\/+$/, \"\");\n");
        std::fs::write(&pi, generated).unwrap();
        assert_eq!(generated_hook_state(&pi, SetupAgent::Pi, url).unwrap().state, "ok");
        assert!(guard_generated_hook(&pi, SetupAgent::Pi, "http://127.0.0.1:12345").is_err());

        let agy = directory.join("mcp_config.json");
        let registration = json!({"mcpServers":{"other":{"command":"custom"}, "ai-memory":{"serverUrl":format!("{url}/mcp")}}});
        std::fs::write(&agy, registration.to_string()).unwrap();
        assert!(guard_existing_registration(&agy, SetupAgent::AntigravityCli, url).is_ok());
        assert!(guard_existing_registration(&agy, SetupAgent::AntigravityCli, "http://127.0.0.1:12345").is_err());
        assert_eq!(read_config(&agy).unwrap(), Some(registration));

        let oc = directory.join("opencode.json");
        let mixed = json!({"mcp":{"ai-memory":{"url":"https://custom.invalid/mcp"}, "servers":{"ai-memory":{"url":format!("{url}/mcp")}}}});
        std::fs::write(&oc, mixed.to_string()).unwrap();
        assert!(guard_existing_registration(&oc, SetupAgent::Opencode, url).is_err());
        assert_eq!(read_config(&oc).unwrap(), Some(mixed));
        for file in [pi, agy, oc] { std::fs::remove_file(file).unwrap(); }
        std::fs::remove_dir(directory).unwrap();
    }
    use serde_json::json;
    #[test]
    fn kimi_requires_start_tools_and_end_for_the_current_service() {
        let url = "http://127.0.0.1:47374";
        let hook = |event: &str| json!({"event":event,"command":format!("ai-memory hook --server-url {url}")});
        let mut config = json!({"hooks":[hook("SessionStart"),hook("PostToolUse")]});
        assert_eq!(kimi_hooks_configuration(Some(&config),url).state, "failed");
        config["hooks"].as_array_mut().unwrap().push(hook("SessionEnd"));
        assert_eq!(kimi_hooks_configuration(Some(&config),url).state, "ok");
    }
    #[test]
    fn registration_distinguishes_missing_disabled_and_wrong_endpoint() {
        let url = "http://127.0.0.1:49374";
        assert_eq!(registration(None, SetupAgent::Codex, url).state, "missing");
        let config =
            json!({"mcp_servers":{"ai-memory":{"url":format!("{url}/mcp?flavor=moonshot")}}});
        assert_eq!(
            registration(Some(&config), SetupAgent::Codex, url).state,
            "ok"
        );
        assert_eq!(
            registration(Some(&config), SetupAgent::Codex, "http://127.0.0.1:9").state,
            "mismatch"
        );
        let disabled =
            json!({"mcpServers":{"ai-memory":{"url":format!("{url}/mcp"),"disabled":true}}});
        assert_eq!(
            registration(Some(&disabled), SetupAgent::KimiCode, url).state,
            "failed"
        );
    }
    #[test]
    fn doctor_history_never_proves_live_hooks_and_other_servers_are_rejected() {
        let mut report = json!({"server":"http://localhost:1", "capture_coverage_problem":null,"marker_capture_problem":null,
            "project_coordinate":{"server_problem":null},"rows":[{"agent":"codex","captured":24,"uncaptured":false}]});
        assert_eq!(
            capture_report(&report, SetupAgent::Codex, "http://localhost:2").state,
            "mismatch"
        );
        assert_eq!(
            capture_report(&report, SetupAgent::Codex, "http://localhost:1").state,
            "pending"
        );
        report["rows"][0]["uncaptured"] = true.into();
        assert_eq!(
            capture_report(&report, SetupAgent::Codex, "http://localhost:1").state,
            "failed"
        );
        report["rows"] = json!([]);
        assert_eq!(
            capture_report(&report, SetupAgent::Codex, "http://localhost:1").state,
            "unknown"
        );
    }
    #[test]
    fn config_parsing_reports_corruption_instead_of_absence() {
        let root = std::env::temp_dir().join(format!("crewvela-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        assert!(read_config(&path).unwrap().is_none());
        std::fs::write(
            &path,
            "[mcp_servers.ai-memory]\nurl = 'http://localhost:1/mcp'",
        )
        .unwrap();
        assert_eq!(
            registration(
                read_config(&path).unwrap().as_ref(),
                SetupAgent::Codex,
                "http://localhost:1"
            )
            .state,
            "ok"
        );
        std::fs::write(
            &path,
            "\u{feff}[mcp_servers.ai-memory]\nurl = 'http://localhost:1/mcp'",
        )
        .unwrap();
        assert_eq!(
            registration(
                read_config(&path).unwrap().as_ref(),
                SetupAgent::Codex,
                "http://localhost:1"
            )
            .state,
            "ok"
        );
        std::fs::write(&path, "[invalid").unwrap();
        assert!(read_config(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
    #[test]
    fn hook_config_is_not_inferred_from_unrelated_or_disabled_commands() {
        assert!(!has_service_url(
            "ai-memory hook --server-url http://localhost:10",
            "http://localhost:1"
        ));
        assert!(!has_service_url(
            "ai-memory hook --server-url http://localhost:1/other",
            "http://localhost:1"
        ));
        assert!(has_service_url(
            "[\"--server-url\",\"http://localhost:1\"]",
            "http://localhost:1"
        ));
        let url = "http://localhost:1";
        let config = json!({"hooks":{"SessionStart":[{"command":"ai-memory hook --server-url http://localhost:1"}]}});
        assert_eq!(hooks_configuration(Some(&config), url).state, "ok");
        assert_eq!(
            hooks_configuration(Some(&config), "http://other:2").state,
            "unknown"
        );
        let disabled = json!({"hooks":{"enabled":false,"events":config["hooks"].clone()}});
        assert_eq!(hooks_configuration(Some(&disabled), url).state, "failed");
        assert_eq!(
            hooks_configuration(Some(&json!({"other":"ai-memory"})), url).state,
            "missing"
        );
    }
    #[tokio::test]
    async fn streamable_http_probe_sends_required_accept_header() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let fixture = std::thread::spawn(move || {
            for (method, id) in [("initialize", Some(1)), ("notifications/initialized", None), ("tools/list", Some(2))] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut header = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    header.push_str(&line.to_lowercase());
                }
                assert!(header.contains("accept: application/json, text/event-stream"));
                let length = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse::<usize>().unwrap();
                let mut request = vec![0; length];
                reader.read_exact(&mut request).unwrap();
                let request: Value = serde_json::from_slice(&request).unwrap();
                assert_eq!(request["method"].as_str(), Some(method));
                assert_eq!(request.get("id").and_then(Value::as_i64), id);
                if id.is_none() {
                    write!(stream, "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    continue;
                }
                let body = if id == Some(1) { json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}) }
                    else { json!({"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"memory_search"}]}}) }.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let config = McpServerConfig {
            id: "fixture".into(),
            name: "fixture".into(),
            transport: McpTransport::Http,
            command: None,
            args: vec![],
            env: Default::default(),
            url: Some(format!("http://{addr}/mcp")),
            headers: Default::default(),
            enabled: true,
            source_plugin: None,
            created_at: 0,
            updated_at: 0,
        };
        let report = crate::mcp::probe_remote(&config).await;
        assert!(report.ok, "{:?}", report.error);
        assert_eq!(report.tools[0].name, "memory_search");
        fixture.join().unwrap();
    }
    #[tokio::test]
    #[ignore = "requires the locally installed ai-memory service; read-only"]
    async fn live_read_only_check() {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let report = check(SetupAgent::Codex, project).await.unwrap();
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        assert_eq!(report.mcp.state, "ok");
        assert_eq!(report.tools.state, "ok");
        assert_ne!(
            report.capture.state, "ok",
            "history must not mark current hooks verified"
        );
    }
}
