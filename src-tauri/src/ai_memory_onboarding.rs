//! Explicit first-use setup. No agent process is restarted or prompted by this workflow.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub(super) project_dir: String,
    pub(super) agents: Vec<String>,
    pub(super) local_archive: Option<String>,
    #[serde(default)]
    pub(super) target_version: Option<String>,
    #[serde(default)]
    pub(super) update: bool,
    #[serde(default)]
    pub(super) confirm_major: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    id: String,
    label: String,
    state: String,
    detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    id: String,
    project_dir: String,
    agents: Vec<String>,
    state: String,
    steps: Vec<Step>,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

pub(super) fn require_idle() -> Result<(), String> {
    if JOB
        .lock()
        .map_err(|_| "快速启用状态锁异常")?
        .as_ref()
        .is_some_and(|job| job.state == "running")
    {
        return Err("快速启用正在执行，请完成后再操作本地服务或接入配置".into());
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct Environment {
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
    pub(super) codex_home: PathBuf,
    pub(super) existing: Option<PathBuf>,
}

impl Environment {
    fn current() -> Result<Self> {
        let home = dirs::home_dir().context("无法定位用户配置目录")?;
        Ok(Self {
            root: crate::providers::config_dir()?,
            codex_home: std::env::var_os("CODEX_HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            home,
            existing: locate_executable().map(|(path, _)| path),
        })
    }
    pub(super) fn data(&self) -> PathBuf {
        self.root.join("ai-memory-data")
    }
    pub(super) fn config_paths(&self, agent: &str, project: &Path) -> (PathBuf, PathBuf) {
        if agent == "codex" {
            (
                self.codex_home.join("config.toml"),
                self.codex_home.join("hooks.json"),
            )
        } else {
            (
                self.home.join(".claude.json"),
                project.join(".claude/settings.local.json"),
            )
        }
    }
}

#[tauri::command]
pub async fn ai_memory_quick_info() -> Result<Value, String> {
    let mut info = tokio::task::spawn_blocking(|| -> Result<Value> {
        let agents = [("claude-code",crate::multi_agent::AgentRuntimeKind::ClaudeCode),("codex",crate::multi_agent::AgentRuntimeKind::Codex)]
            .map(|(id,kind)| json!({"id":id,"installed":crate::multi_agent::resolve_runtime_executable(kind).is_some()}));
        let package = installation::current_package();
        let current = locate_executable();
        let installed = current.as_ref().map(|(path,_)| installation::version(path)).transpose()?;
        Ok(json!({"supported":package.is_some(),"package":package,"version":null,"agents":agents,"installedVersion":installed,"managed":current.is_some_and(|(_,source)|source=="managed"),"recoveryPending":updates::pending(&crate::providers::config_dir()?)}))
    }).await.map_err(|e|e.to_string())?.map_err(|e|format!("{e:#}"))?;
    if info["supported"] == true {
        match releases::resolve(None).await {
            Ok(release) => {
                info["version"] = json!(release.version);
                info["releaseId"] = json!(release.id);
                info["updateError"] = json!(releases::compatible(&release.version)
                    .err()
                    .map(|e| e.to_string()));
            }
            Err(error) => info["updateError"] = json!(format!("{error:#}")),
        }
    }
    Ok(info)
}

#[tauri::command]
pub fn ai_memory_quick_status() -> Result<Option<Job>, String> {
    let mut state = JOB.lock().map_err(|_| "快速启用状态锁异常")?;
    if state.is_none() {
        let path = crate::providers::config_dir()
            .map_err(|e| e.to_string())?
            .join("ai-memory-quick-setup.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let mut old: Job = serde_json::from_slice(&bytes)
                    .map_err(|_| "上次快速启用记录损坏；请保留记录并检查配置")?;
                if old.state == "running" {
                    old.state = "failed".into();
                    for step in &mut old.steps {
                        if step.state == "running" {
                            step.state = "failed".into();
                            step.detail = "上次运行中断；重试会复核已完成步骤".into();
                        }
                    }
                }
                *state = Some(old);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("读取快速启用记录失败：{error}")),
        }
    }
    Ok(state.clone())
}

fn persist(env: &Environment, job: &Job) -> Result<()> {
    atomic_write(
        &env.root.join("ai-memory-quick-setup.json"),
        &serde_json::to_vec(job)?,
    )
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("文件缺少父目录")?;
    std::fs::create_dir_all(parent)?;
    if std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly()) {
        bail!("配置文件只读，保留原文件：{}", path.display());
    }
    let temp = parent.join(format!(".memory-setup-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        if let Ok(metadata) = std::fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

fn new_job(request: &Request) -> Job {
    let mut steps = vec![
        ("precheck", "检查环境与现有配置"),
        ("install", "安装兼容版本并校验"),
        ("service", "初始化并启动本地服务"),
        ("project", "准备所选项目"),
    ];
    if request.update {
        steps.insert(1, ("backup", "备份安装、记忆和接入配置"));
    }
    for agent in &request.agents {
        steps.push((
            agent.as_str(),
            if agent == "codex" {
                "接入 Codex"
            } else {
                "接入 Claude Code"
            },
        ));
    }
    steps.push(("check", "检查配置与记忆工具"));
    Job {
        id: uuid::Uuid::new_v4().to_string(),
        project_dir: request.project_dir.clone(),
        agents: request.agents.clone(),
        state: "running".into(),
        steps: steps
            .into_iter()
            .map(|(id, label)| Step {
                id: id.into(),
                label: label.into(),
                state: "pending".into(),
                detail: String::new(),
            })
            .collect(),
    }
}

#[tauri::command]
pub fn ai_memory_quick_start(request: Request) -> Result<Job, String> {
    if installation::current_package().is_none() {
        return Err(
            "快速启用支持 Windows x64 和 macOS（Apple Silicon / Intel）；当前平台请使用接入配置"
                .into(),
        );
    }
    validate_request(&request).map_err(|e| format!("{e:#}"))?;
    let env = Environment::current().map_err(|e| e.to_string())?;
    if updates::pending(&env.root) {
        return Err("上次更新未完成，请先恢复升级前快照".into());
    }
    if server_url_override().is_some() {
        return Err("当前使用外部服务地址，快速启用不会修改外部服务；请使用接入配置".into());
    }
    let mut state = JOB.lock().map_err(|_| "快速启用状态锁异常")?;
    if state.as_ref().is_some_and(|job| job.state == "running") {
        return Err("已有快速启用任务正在执行".into());
    }
    let job = new_job(&request);
    persist(&env, &job).map_err(|e| e.to_string())?;
    *state = Some(job.clone());
    drop(state);
    tauri::async_runtime::spawn(async move {
        let update = |id: &str, status: &str, detail: &str| -> Result<()> {
            let mut state = JOB
                .lock()
                .map_err(|_| anyhow::anyhow!("快速启用状态锁异常"))?;
            let job = state.as_mut().context("快速启用状态丢失")?;
            if let Some(step) = job.steps.iter_mut().find(|step| step.id == id) {
                step.state = status.into();
                step.detail = detail.into();
            }
            persist(&env, job)
        };
        let result = execute(&env, &request, &update).await;
        if let Ok(mut state) = JOB.lock() {
            if let Some(job) = state.as_mut() {
                job.state = if result.is_ok() {
                    "awaiting-session"
                } else {
                    "failed"
                }
                .into();
                if let Err(error) = result {
                    let failed = job
                        .steps
                        .iter()
                        .position(|s| s.state == "running")
                        .unwrap_or(job.steps.len() - 1);
                    if let Some(step) = job.steps.get_mut(failed) {
                        step.state = "failed".into();
                        step.detail = format!("{error:#}");
                    }
                }
                if let Err(error) = persist(&env, job) {
                    job.state = "failed".into();
                    if let Some(step) = job.steps.last_mut() {
                        step.state = "failed".into();
                        step.detail = format!("保存启用结果失败：{error}");
                    }
                }
            }
        }
    });
    Ok(job)
}

fn validate_request(request: &Request) -> Result<PathBuf> {
    if request.update && request.target_version.is_none() {
        bail!("请先检查更新并选择目标版本");
    }
    if let Some(version) = &request.target_version {
        releases::compatible(version)?;
    }
    let path = Path::new(&request.project_dir);
    if !path.is_absolute() || !path.is_dir() {
        bail!("请选择存在的绝对项目目录");
    }
    if request.agents.is_empty()
        || request.agents.len() > 2
        || request
            .agents
            .iter()
            .any(|a| !matches!(a.as_str(), "claude-code" | "codex"))
        || request
            .agents
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.agents.len()
    {
        bail!("请选择 Claude Code 或 Codex，不能重复选择");
    }
    Ok(path.canonicalize()?)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("读取配置失败：{}", path.display())),
    }
}

fn precheck(env: &Environment, request: &Request, project: &Path) -> Result<()> {
    let data = env.data();
    if data.join("config.toml").exists() {
        let _ = managed_base(&data)?;
    }
    let owned = data.join("crewvela-quick-setup.json").is_file();
    let mode = read_optional(&data.join("capture-mode"))?;
    let allowlisted = mode
        .as_ref()
        .is_some_and(|mode| mode.as_slice() == b"allowlist\n" || mode.as_slice() == b"allowlist");
    if (mode.is_some() && !allowlisted)
        || (data.join("config.toml").exists() && !owned && !allowlisted)
    {
        bail!("已有记忆数据使用其他捕获策略，已保留。请在接入配置中核对范围后配置；不会自动改变已有全项目捕获行为");
    }
    inspect_marker(project)?;
    for agent in &request.agents {
        let (mcp, hooks) = env.config_paths(agent, project);
        guard_project_registration(&mcp, SetupAction::Mcp)?;
        guard_project_registration(&hooks, SetupAction::Hooks)?;
        // Parse before any download or mutation. Existing foreign ai-memory
        // registrations require an explicit manual choice rather than replacement.
        for path in [&mcp, &hooks] {
            let _ = verification::read_config(path)?;
        }
        if let Some(config) = verification::read_config(&mcp)? {
            if agent == "codex" && config.pointer("/features/hooks") == Some(&Value::Bool(false)) {
                bail!("Codex 的原生 hooks 已被禁用，已保留该选择；请在手动接入中核对后启用");
            }
            let entry = if agent == "codex" {
                config.pointer("/mcp_servers/ai-memory")
            } else {
                config.pointer("/mcpServers/ai-memory")
            };
            if let Some(entry) = entry {
                let standard = entry.as_object().is_some_and(|fields| {
                    fields.iter().all(|(key, value)| match key.as_str() {
                        "url" => value.is_string(),
                        "type" => value == "http",
                        "default_tools_approval_mode" => value == "approve",
                        "headers" | "http_headers" => value.as_object().is_some_and(|headers| {
                            headers.len() == 1
                                && headers
                                    .get("Authorization")
                                    .and_then(Value::as_str)
                                    .is_some_and(|s| s.starts_with("Bearer "))
                        }),
                        // v3 generates explicit prompts for destructive memory tools.
                        // Accept that restrictive default, never arbitrary tool overrides.
                        "tools" if agent == "codex" => value.as_object().is_some_and(|tools| {
                            tools.iter().all(|(name, settings)| {
                                name.starts_with("memory_")
                                    && settings.as_object().is_some_and(|fields| {
                                        fields.len() == 1
                                            && fields.get("approval_mode") == Some(&json!("prompt"))
                                    })
                            })
                        }),
                        _ => false,
                    })
                });
                if !standard {
                    bail!("{agent} 已有自定义或禁用的 ai-memory 配置，已保留；请使用手动接入");
                }
                let existing_base = if data.join("config.toml").exists() {
                    managed_base(&data)?
                } else {
                    DEFAULT_SERVER_URL.into()
                };
                if !entry["url"]
                    .as_str()
                    .is_some_and(|url| verification::endpoint_matches(url, &existing_base))
                {
                    bail!("{agent} 已有其他 ai-memory 接入，已保留；请在接入配置中检查");
                }
            }
        }
    }
    Ok(())
}

fn inspect_marker(project: &Path) -> Result<()> {
    if std::fs::symlink_metadata(project.join(".ai-memory.toml"))
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        bail!("项目标记为符号链接，未修改；请使用普通项目标记");
    }
    if let Some(bytes) = read_optional(&project.join(".ai-memory.toml"))? {
        let value: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)?;
        if !["workspace", "project"].iter().all(|key| {
            value
                .get(*key)
                .and_then(toml::Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
        }) {
            bail!("已有 .ai-memory.toml 缺少明确 workspace/project，未覆盖");
        }
        if value.get("server").is_some() {
            bail!("项目已指定独立记忆服务路由，未覆盖；请使用接入配置");
        }
    }
    Ok(())
}

fn ensure_marker(project: &Path) -> Result<()> {
    let path = project.join(".ai-memory.toml");
    if path.exists() {
        return inspect_marker(project);
    }
    let folder = project.file_name().unwrap_or_default().to_string_lossy();
    let name: String = folder
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .take(48)
        .collect();
    let digest = format!("{:x}", Sha256::digest(project.to_string_lossy().as_bytes()));
    let value = format!(
        "workspace = \"local\"\nproject = {}\n",
        serde_json::to_string(&format!("{name}-{}", &digest[..8]))?
    );
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(value.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn managed_base(data: &Path) -> Result<String> {
    let path = data.join("config.toml");
    managed_url_from_config(&std::fs::read_to_string(path)?)
}

fn initialize_data(executable: &Path, data: &Path) -> Result<String> {
    let fresh = !data.join("config.toml").exists();
    std::fs::create_dir_all(data)?;
    if fresh {
        atomic_write(&data.join("crewvela-quick-setup.json"), b"{\"version\":1}")?;
    }
    let mut command = Command::new(executable);
    command
        .arg("--data-dir")
        .arg(data)
        .arg("init")
        .current_dir(data)
        .creation_flags_no_window();
    run_memory_init(command, Duration::from_secs(10))?;
    if fresh {
        let config = data.join("config.toml");
        let mut value: toml::Value = toml::from_str(&std::fs::read_to_string(&config)?)?;
        // Use an independent local port if the default is occupied; never adopt
        // a listener belonging to another application's data directory.
        if !managed_listener_is_unbound(DEFAULT_SERVER_URL)? {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            value["bind"] = toml::Value::String(format!("127.0.0.1:{port}"));
            drop(listener);
            atomic_write(&config, toml::to_string_pretty(&value)?.as_bytes())?;
        }
    }
    if !data.join("capture-mode").exists() {
        atomic_write(&data.join("capture-mode"), b"allowlist\n")?;
    }
    managed_base(data)
}

pub(super) fn setup_agent(
    env: &Environment,
    executable: &Path,
    project: &Path,
    base: &str,
    agent: &str,
) -> Result<()> {
    setup_agent_with(env, executable, project, base, agent, |command| {
        run_setup_command(command, Duration::from_secs(60))
    })
}

fn setup_agent_with(
    env: &Environment,
    executable: &Path,
    project: &Path,
    base: &str,
    agent: &str,
    mut run: impl FnMut(Command) -> Result<SetupResult>,
) -> Result<()> {
    let _guard = SETUP_LOCK
        .try_lock()
        .map_err(|_| anyhow::anyhow!("另一个接入配置正在执行，请重试"))?;
    let paths = env.config_paths(agent, project);
    let paths = [paths.0, paths.1];
    let before = paths
        .iter()
        .map(|path| read_optional(path))
        .collect::<Result<Vec<_>>>()?;
    let backup = env
        .root
        .join("ai-memory-setup-backups")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&backup)?;
    for (index, bytes) in before.iter().enumerate() {
        if let Some(bytes) = bytes {
            atomic_write(&backup.join(format!("{index}.backup")), bytes)?;
        }
    }
    let mut after = before.clone();
    let result = (|| -> Result<()> {
        for (index, action) in ["install-mcp", "install-hooks"].iter().enumerate() {
            if read_optional(&paths[index])? != before[index] {
                bail!("接入配置在运行前被修改，保留外部修改");
            }
            let mut command = Command::new(executable);
            command
                .arg("--data-dir")
                .arg(env.data())
                .arg(action)
                .arg(if index == 0 { "--client" } else { "--agent" })
                .arg(agent)
                .args(["--server-url", base, "--config-file"])
                .arg(&paths[index])
                .arg("--apply")
                .current_dir(project)
                .creation_flags_no_window();
            if index == 1 {
                command.args(["--capture-mode", "allowlist"]);
            }
            auth::command(&mut command, &env.data(), base)?;
            let output = run(command);
            after[index] = read_optional(&paths[index])?;
            let output = output?;
            if !output.success {
                bail!("{agent} {action} 未完成（退出码 {:?}，超时 {}）；本 Agent 配置将恢复，请查看高级接入诊断",output.exit_code,output.timed_out);
            }
            let _ = verification::read_config(&paths[index])?.context("安装器未生成接入配置")?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        for index in 0..2 {
            if after[index] == before[index] {
                continue;
            }
            if read_optional(&paths[index])? != after[index] {
                bail!(
                    "{error:#}；配置已被其他程序修改，未自动回退；原始备份：{}",
                    backup.display()
                );
            }
            if let Some(bytes) = &before[index] {
                atomic_write(&paths[index], bytes)?;
            } else if paths[index].exists() {
                std::fs::remove_file(&paths[index])?;
            }
        }
        return Err(error).with_context(|| format!("接入未完成；原配置备份：{}", backup.display()));
    }
    Ok(())
}

pub(super) async fn verify_tools(base: &str, data: &Path) -> Result<()> {
    let server = crate::mcp::McpServerConfig {
        id: "memory-quick-check".into(),
        name: "ai-memory".into(),
        transport: crate::mcp::McpTransport::Http,
        command: None,
        args: vec![],
        env: Default::default(),
        url: Some(format!("{base}/mcp")),
        headers: auth::headers(data, base)?,
        enabled: true,
        source_plugin: None,
        created_at: 0,
        updated_at: 0,
    };
    let result = crate::mcp::probe_remote(&server).await;
    if !result.ok || result.tools.is_empty() {
        bail!(
            "记忆工具检查未通过：{}",
            result.error.unwrap_or_else(|| "没有可用工具".into())
        );
    }
    Ok(())
}

pub(super) async fn execute(
    env: &Environment,
    request: &Request,
    update: &(impl Fn(&str, &str, &str) -> Result<()> + Sync),
) -> Result<()> {
    let _operation = if request.update {
        None
    } else {
        Some(updates::operation_lock(&env.root)?)
    };
    if updates::pending(&env.root) {
        bail!("上次更新未完成，请先恢复升级前快照");
    }
    update("precheck", "running", "核对项目、版本和配置范围")?;
    let project = validate_request(request)?;
    precheck(env, request, &project)?;
    update(
        "precheck",
        "done",
        "仅接入所选 Agent；现有项目路由和其他配置将保留",
    )?;
    if request.update {
        let release = releases::resolve(request.target_version.as_deref()).await?;
        return updates::execute(env, request, &release, update).await;
    }
    update("install", "running", "准备所选官方稳定版，校验后才发布")?;
    let executable = if let Some(path) = &env.existing {
        let path = path.clone();
        let found = tokio::task::spawn_blocking(move || installation::version(&path)).await??;
        releases::compatible(&found)?;
        env.existing.clone().unwrap()
    } else {
        let _guard = service_lifecycle_lock(Duration::from_secs(20)).await?;
        let release = releases::resolve(request.target_version.as_deref()).await?;
        installation::install_release(
            &env.root.join("ai-memory"),
            request.local_archive.as_deref().map(Path::new),
            &release,
        )
        .await?
    };
    update("install", "done", "兼容版本已就绪，未覆盖记忆数据")?;
    update("service", "running", "初始化本机目录并检查监听地址")?;
    let base = {
        let _guard = service_lifecycle_lock(Duration::from_secs(20)).await?;
        let exe = executable.clone();
        let data = env.data();
        let base = tokio::task::spawn_blocking(move || initialize_data(&exe, &data)).await??;
        start_service_at(&executable, &env.data(), &base).await?;
        base
    };
    update("service", "done", "本地服务健康；无需模型 API Key")?;
    update("project", "running", "准备项目标记，保留已有项目坐标")?;
    ensure_marker(&project)?;
    update("project", "done", "使用 allowlist，仅捕获带项目标记的目录")?;
    for agent in &request.agents {
        update(agent, "running", "备份并合并 MCP 与钩子配置")?;
        let env = env.clone();
        let exe = executable.clone();
        let project = project.clone();
        let base = base.clone();
        let name = agent.clone();
        tokio::task::spawn_blocking(move || setup_agent(&env, &exe, &project, &base, &name))
            .await??;
        update(agent, "done", "接入配置已完成；现有运行中会话未重启")?;
    }
    update("check", "running", "检查实际记忆工具与项目捕获策略")?;
    verify_tools(&base, &env.data()).await?;
    let exe = executable;
    let data = env.data();
    let cwd = project.display().to_string();
    let scope =
        tokio::task::spawn_blocking(move || inspect_scope_at(&exe, &data, &cwd, &base)).await?;
    if scope.is_none() {
        bail!("项目捕获策略检查未通过，尚不能确认项目接入");
    }
    update(
        "check",
        "done",
        "配置与服务工具已通过；请加载新会话，再检查真实捕获记录",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Environment, Request) {
        let root =
            std::env::temp_dir().join(format!("crewvela-quick-setup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let env = Environment {
            root: root.join("app"),
            home: root.join("home"),
            codex_home: root.join("home/.codex"),
            existing: None,
        };
        let request = Request {
            project_dir: root.join("project").display().to_string(),
            agents: vec!["claude-code".into(), "codex".into()],
            local_archive: None,
            target_version: None,
            update: false,
            confirm_major: false,
        };
        (root, env, request)
    }
    #[test]
    fn marker_is_explicit_repeatable_and_never_overwrites_existing_routes() {
        let (root, _, request) = fixture();
        let project = Path::new(&request.project_dir);
        ensure_marker(project).unwrap();
        let bytes = std::fs::read(project.join(".ai-memory.toml")).unwrap();
        ensure_marker(project).unwrap();
        assert_eq!(
            std::fs::read(project.join(".ai-memory.toml")).unwrap(),
            bytes
        );
        assert!(!String::from_utf8(bytes)
            .unwrap()
            .contains(&request.project_dir));
        std::fs::write(
            project.join(".ai-memory.toml"),
            "workspace='w'\nproject='p'\nserver='custom'",
        )
        .unwrap();
        assert!(ensure_marker(project).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preflight_preserves_legacy_capture_policy_and_invalid_user_configs() {
        let (root, env, request) = fixture();
        let data = env.data();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("config.toml"), "bind='127.0.0.1:49374'").unwrap();
        assert!(precheck(&env, &request, Path::new(&request.project_dir))
            .unwrap_err()
            .to_string()
            .contains("捕获策略"));
        std::fs::write(data.join("capture-mode"), "allowlist").unwrap();
        std::fs::create_dir_all(&env.codex_home).unwrap();
        std::fs::write(env.codex_home.join("config.toml"), "broken = [").unwrap();
        assert!(precheck(&env, &request, Path::new(&request.project_dir)).is_err());
        assert_eq!(
            std::fs::read_to_string(env.codex_home.join("config.toml")).unwrap(),
            "broken = ["
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn requests_reject_duplicates_and_report_selected_steps_only() {
        let (root, _, mut request) = fixture();
        assert_eq!(new_job(&request).steps.len(), 7);
        request.agents = vec!["codex".into(), "codex".into()];
        assert!(validate_request(&request).is_err());
        request.agents = vec!["zcode".into()];
        assert!(validate_request(&request).is_err());
        request.agents = vec!["codex".into()];
        assert_eq!(new_job(&request).steps.len(), 6);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v3_restrictive_tool_approvals_are_retained_but_custom_overrides_stop_setup() {
        let (root, env, mut request) = fixture();
        request.agents = vec!["codex".into()];
        std::fs::create_dir_all(&env.codex_home).unwrap();
        let path = env.codex_home.join("config.toml");
        let base="[mcp_servers.ai-memory]\nurl='http://127.0.0.1:49374/mcp'\ndefault_tools_approval_mode='approve'\n[mcp_servers.ai-memory.http_headers]\nAuthorization='Bearer fixture-only'\n[mcp_servers.ai-memory.tools.memory_delete_page]\napproval_mode='prompt'\n";
        std::fs::write(&path, base).unwrap();
        assert!(precheck(&env, &request, Path::new(&request.project_dir)).is_ok());
        let custom = base.replace("approval_mode='prompt'", "approval_mode='approve'");
        std::fs::write(&path, &custom).unwrap();
        assert!(precheck(&env, &request, Path::new(&request.project_dir)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), custom);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn denied_capture_scope_and_user_changed_policy_are_not_accepted() {
        let report = json!({"server_resolution":"install-default","scope_resolution":"server-derived","policy_state":"inactive","admits_capture":false});
        assert!(scope_from_report(&report, "project").is_none());
        let (root, env, request) = fixture();
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("crewvela-quick-setup.json"), "{}").unwrap();
        std::fs::write(env.data().join("capture-mode"), "denylist").unwrap();
        assert!(precheck(&env, &request, Path::new(&request.project_dir)).is_err());
        assert_eq!(
            std::fs::read_to_string(env.data().join("capture-mode")).unwrap(),
            "denylist"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Uses the official, SHA256-verified release and a completely separate
    /// home/config/project tree. Never reads or writes real Agent configuration.
    #[tokio::test]
    #[ignore = "requires official platform archive: CREWVELA_AI_MEMORY_TEST_RELEASE_ARCHIVE"]
    async fn official_release_first_use_repeat_and_scope_acceptance() {
        let archive = std::env::var("CREWVELA_AI_MEMORY_TEST_RELEASE_ARCHIVE")
            .or_else(|_| std::env::var("CREWVELA_AI_MEMORY_TEST_RELEASE_ZIP"))
            .expect("set official archive path for the current platform");
        let (root, env, mut request) = fixture();
        request.target_version = Some(
            std::env::var("CREWVELA_AI_MEMORY_TEST_RELEASE_VERSION")
                .unwrap_or_else(|_| "2.6.0".into()),
        );
        let project = root.join("记忆测试 项目 O'Brien");
        std::fs::create_dir(&project).unwrap();
        request.project_dir = project.to_string_lossy().into_owned();
        // An unrelated listener on the default port must never be adopted.
        let _foreign_listener = std::net::TcpListener::bind("127.0.0.1:49374").ok();
        request.local_archive = Some(archive);
        std::fs::create_dir_all(&env.codex_home).unwrap();
        std::fs::write(
            env.codex_home.join("config.toml"),
            "model = 'existing-model'\n[mcp_servers.keep]\nurl = 'http://127.0.0.1:9/mcp'\n",
        )
        .unwrap();
        std::fs::create_dir_all(&env.home).unwrap();
        std::fs::write(
            env.home.join(".claude.json"),
            r#"{"other":42,"mcpServers":{"keep":{"command":"keep-me"}}}"#,
        )
        .unwrap();
        let updates = Mutex::new(Vec::new());
        let update = |id: &str, state: &str, _detail: &str| {
            updates
                .lock()
                .unwrap()
                .push((id.to_owned(), state.to_owned()));
            Ok(())
        };
        let result = async {
            execute(&env, &request, &update).await?;
            let base = managed_base(&env.data())?;
            assert_ne!(
                base, DEFAULT_SERVER_URL,
                "fresh data must use an independent port when the default is occupied"
            );
            let codex = verification::read_config(&env.codex_home.join("config.toml"))?.unwrap();
            assert_eq!(codex["model"], "existing-model");
            assert_eq!(
                codex["mcp_servers"]["keep"]["url"],
                "http://127.0.0.1:9/mcp"
            );
            assert!(verification::endpoint_matches(
                codex["mcp_servers"]["ai-memory"]["url"].as_str().unwrap(),
                &base
            ));
            let claude = verification::read_config(&env.home.join(".claude.json"))?.unwrap();
            assert_eq!(claude["other"], 42);
            assert_eq!(claude["mcpServers"]["keep"]["command"], "keep-me");
            for agent in &request.agents {
                let (_, hooks) = env.config_paths(agent, Path::new(&request.project_dir));
                let hooks = verification::read_config(&hooks)?.unwrap();
                assert!(hooks["hooks"].is_object());
            }
            let marker = std::fs::read(Path::new(&request.project_dir).join(".ai-memory.toml"))?;
            let config = std::fs::read(env.data().join("config.toml"))?;
            std::fs::write(env.data().join("keep-user-memory"), "preserve")?;
            execute(&env, &request, &update).await?;
            assert_eq!(
                std::fs::read(Path::new(&request.project_dir).join(".ai-memory.toml"))?,
                marker
            );
            assert_eq!(std::fs::read(env.data().join("config.toml"))?, config);
            assert_eq!(
                std::fs::read_to_string(env.data().join("keep-user-memory"))?,
                "preserve"
            );
            assert_eq!(
                std::fs::read_to_string(env.data().join("capture-mode"))?.trim(),
                "allowlist"
            );
            let exe = env
                .root
                .join("ai-memory")
                .join(installation::current_package().unwrap().executable_name());
            let unmarked = root.join("not-selected");
            std::fs::create_dir(&unmarked)?;
            assert!(
                inspect_scope_at(&exe, &env.data(), unmarked.to_str().unwrap(), &base).is_none(),
                "unmarked directory must not have memory scope"
            );
            assert_eq!(
                updates
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, state)| id == "check" && state == "done")
                    .count(),
                2
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;
        {
            let _guard = service_lifecycle_lock(Duration::from_secs(20))
                .await
                .unwrap();
            let mut service = managed_service().unwrap();
            if service.is_running().unwrap() {
                service.stop().unwrap();
            }
        }
        result.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn partial_setup_rolls_back_both_files_and_retains_external_changes() {
        for concurrent in [false, true] {
            let (root, env, request) = fixture();
            let project = Path::new(&request.project_dir);
            std::fs::create_dir_all(&env.codex_home).unwrap();
            let (mcp, hooks) = env.config_paths("codex", project);
            std::fs::write(&mcp, "model='keep'\n").unwrap();
            std::fs::write(&hooks, "{\"custom\":true}").unwrap();
            let mut index = 0;
            let error = setup_agent_with(
                &env,
                Path::new("fixture.exe"),
                project,
                DEFAULT_SERVER_URL,
                "codex",
                |_| {
                    index += 1;
                    if index == 1 {
                        std::fs::write(&mcp, "model='new'\n")?;
                    } else {
                        std::fs::write(&hooks, "{\"partial\":true}")?;
                        if concurrent {
                            std::fs::write(&mcp, "model='external-change'\n")?;
                        }
                    }
                    Ok(SetupResult {
                        success: index == 1,
                        exit_code: Some(if index == 1 { 0 } else { 1 }),
                        timed_out: false,
                        stdout: String::new(),
                        stderr: String::new(),
                        output_truncated: false,
                    })
                },
            )
            .unwrap_err();
            if concurrent {
                assert!(error.to_string().contains("其他程序修改"));
                assert_eq!(
                    std::fs::read_to_string(&mcp).unwrap(),
                    "model='external-change'\n"
                );
            } else {
                assert_eq!(std::fs::read_to_string(&mcp).unwrap(), "model='keep'\n");
                assert_eq!(
                    std::fs::read_to_string(&hooks).unwrap(),
                    "{\"custom\":true}"
                );
                assert!(format!("{error:#}").contains("原配置备份"));
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "requires isolated official v2.6.0 and v3.4.5 archives; never user data"]
    async fn official_major_upgrade_failure_rollback_and_retry() {
        let old_archive =
            std::env::var("CREWVELA_AI_MEMORY_TEST_OLD_ARCHIVE").expect("set v2.6.0 archive");
        let new_archive =
            std::env::var("CREWVELA_AI_MEMORY_TEST_RELEASE_ARCHIVE").expect("set v3.4.5 archive");
        let (root, mut env, mut request) = fixture();
        request.target_version = Some("2.6.0".into());
        request.local_archive = Some(old_archive);
        let _foreign_listener = std::net::TcpListener::bind("127.0.0.1:49374").ok();
        let update = |_: &str, _: &str, _: &str| Ok(());
        let result = async {
            execute(&env, &request, &update).await?;
            let executable = env.root.join("ai-memory").join(installation::current_package().unwrap().executable_name());
            env.existing = Some(executable.clone());
            let data = env.data();
            std::fs::write(data.join("keep-user-memory"), "preserve")?;
            {
                let db = rusqlite::Connection::open(data.join("db/memory.sqlite"))?;
                db.execute_batch("CREATE TABLE crewvela_upgrade_fixture(value TEXT); INSERT INTO crewvela_upgrade_fixture VALUES('preserve-database');")?;
            }
            let paths = request.agents.iter().flat_map(|agent| {
                let (mcp,hooks)=env.config_paths(agent,Path::new(&request.project_dir)); [mcp,hooks]
            }).collect::<Vec<_>>();
            let before = paths.iter().map(|p|std::fs::read(p)).collect::<std::io::Result<Vec<_>>>()?;
            request.update = true;
            request.target_version = Some("3.4.5".into());
            request.local_archive = Some(new_archive);
            let release = releases::resolve(Some("3.4.5")).await?;
            assert!(updates::execute(&env,&request,&release,&update).await.unwrap_err().to_string().contains("跨大版本"));
            request.confirm_major = true;
            // All real migration, config and tool checks finish before an injected
            // final failure. This exercises rollback of already migrated data.
            let fail = |id: &str, state: &str, _: &str| {
                if id == "check" && state == "done" {
                    std::fs::write(data.join("candidate-only-data"), "retain failed candidate writes")?;
                    bail!("fixture injected final validation failure");
                }
                Ok(())
            };
            let error = updates::execute(&env,&request,&release,&fail).await.unwrap_err();
            assert!(format!("{error:#}").contains("已恢复旧安装"),"{error:#}");
            assert_eq!(installation::version(&executable)?,"2.6.0");
            assert!(!updates::pending(&env.root));
            for (path,bytes) in paths.iter().zip(&before) { assert_eq!(&std::fs::read(path)?,bytes); }
            assert!(!data.join("candidate-only-data").exists());
            let backups=std::fs::read_dir(env.root.join("ai-memory-upgrade-backups"))?.collect::<std::io::Result<Vec<_>>>()?;
            assert!(backups.iter().any(|entry|std::fs::read_dir(entry.path()).unwrap().any(|p|p.unwrap().path().join("candidate-only-data").is_file())));
            let base=managed_base(&data)?;
            assert_eq!(managed_status_for_data(&service_client(&base)?,&base,&data).await?["version"],"2.6.0");
            execute(&env,&request,&update).await?;
            assert_eq!(installation::version(&executable)?,"3.4.5");
            assert_eq!(managed_status_for_data(&service_client(&base)?,&base,&data).await?["version"],"3.4.5");
            assert_eq!(std::fs::read_to_string(data.join("keep-user-memory"))?,"preserve");
            {
                let db=rusqlite::Connection::open(data.join("db/memory.sqlite"))?;
                let value:String=db.query_row("SELECT value FROM crewvela_upgrade_fixture",[],|r|r.get(0))?;
                assert_eq!(value,"preserve-database");
            }
            assert!(!updates::pending(&env.root));
            assert!(updates::execute(&env,&request,&release,&update).await.unwrap_err().to_string().contains("不高于"));
            Ok::<_,anyhow::Error>(())
        }.await;
        {
            let _guard = service_lifecycle_lock(Duration::from_secs(20))
                .await
                .unwrap();
            let mut service = managed_service().unwrap();
            if service.is_running().unwrap() {
                service.stop().unwrap();
            }
        }
        result.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
