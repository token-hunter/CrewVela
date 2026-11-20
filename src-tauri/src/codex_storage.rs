//! Storage for native Codex threads created by CrewVela collaboration. A locator
//! records the owner, never a path supplied by a project or a model.
use anyhow::{anyhow, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexStorage {
    #[default]
    Personal,
    CrewVela,
}

impl CodexStorage {
    pub(crate) fn is_personal(&self) -> bool {
        *self == Self::Personal
    }
    pub(crate) fn from_locator(locator: &Value) -> Result<Self> {
        match locator.get("codexStorage") {
            None => Ok(Self::Personal),
            Some(value) => {
                serde_json::from_value(value.clone()).context("无法识别 Codex 会话存储归属")
            }
        }
    }

    pub(crate) fn mark(self, locator: &mut Value) {
        locator["codexStorage"] = match self {
            Self::Personal => "personal",
            Self::CrewVela => "crewvela",
        }
        .into();
    }

    pub(crate) fn home(self) -> Result<PathBuf> {
        match self {
            Self::Personal => Ok(std::env::var_os("CODEX_HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .unwrap_or(
                    dirs::home_dir()
                        .context("无法定位 Codex 用户目录")?
                        .join(".codex"),
                )),
            Self::CrewVela => {
                let root = crate::providers::config_dir()?;
                let home = root.join("codex-home");
                check_local_entry(&home)?;
                Ok(crate::multi_agent::external_command_path(&home))
            }
        }
    }

    pub(crate) fn prepare(self) -> Result<()> {
        if self == Self::Personal {
            return Ok(());
        }
        static INITIALIZE: Mutex<()> = Mutex::new(());
        let _guard = INITIALIZE
            .lock()
            .map_err(|_| anyhow!("Codex 专用目录初始化锁已损坏"))?;
        let home = self.home()?;
        let source = Self::Personal.home()?;
        prepare_at(&home, &source)
    }

    pub(crate) fn verify_device_policy(self, executable: &Path, cwd: &Path) -> Result<()> {
        if self.is_personal() {
            return Ok(());
        }
        let mut command = crate::multi_agent::command_for_path(executable);
        self.configure(&mut command, false)?;
        command.arg("app-server");
        crate::codex_title_summary::verify_sqlite_requirement(command, cwd, &self.home()?)
    }

    pub(crate) fn configure(self, command: &mut Command, acp: bool) -> Result<()> {
        let home = self.home()?;
        // portable-pty also imports the Windows registry environment. Explicit
        // command environment always wins, including for legacy resumes.
        command.env("CODEX_HOME", &home);
        if self == Self::Personal {
            if let Some(value) = std::env::var_os("CODEX_SQLITE_HOME") {
                command.env("CODEX_SQLITE_HOME", value);
            } else {
                command.env_remove("CODEX_SQLITE_HOME");
            }
            return Ok(());
        }
        command.env("CODEX_SQLITE_HOME", &home);
        if acp {
            // The adapter starts app-server before applying thread/start config.
            // Pin both stages to this profile's state database.
            command.env("CREWVELA_CODEX_SQLITE_HOME", &home);
            let existing = command
                .get_envs()
                .find(|(key, _)| *key == "CODEX_CONFIG")
                .and_then(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
                .or_else(|| std::env::var("CODEX_CONFIG").ok());
            let mut config: Value = match existing {
                Some(value) => serde_json::from_str(&value)
                    .map_err(|_| anyhow!("Codex ACP 的 CODEX_CONFIG 不是有效 JSON"))?,
                None => serde_json::json!({}),
            };
            config
                .as_object_mut()
                .context("Codex ACP 的 CODEX_CONFIG 必须是对象")?
                .insert("sqlite_home".into(), serde_json::to_value(&home)?);
            command.env("CODEX_CONFIG", serde_json::to_string(&config)?);
        } else {
            // Codex uses the literal string for a CLI -c value that is not
            // valid TOML. An absolute path avoids embedded quoting in npm.cmd.
            command
                .arg("-c")
                .arg(format!("sqlite_home={}", home.display()));
        }
        Ok(())
    }

    pub(crate) fn login_help(self) -> Result<String> {
        let path = self.home()?;
        #[cfg(windows)]
        let command = format!(
            "$env:CODEX_HOME='{}'; codex login",
            path.display().to_string().replace('\'', "''")
        );
        #[cfg(not(windows))]
        let command = format!(
            "CODEX_HOME='{}' codex login",
            path.display().to_string().replace('\'', "'\\''")
        );
        Ok(format!(
            "如需 ChatGPT 登录，请在新终端执行 {command}，完成后重试"
        ))
    }

    pub(crate) fn needs_login(message: &str) -> bool {
        let message = message.to_ascii_lowercase();
        [
            "sign in with chatgpt",
            "sign in to continue",
            "not logged in",
            "login required",
            "authentication required",
            "not authenticated",
            "unauthorized",
            "401 unauthorized",
        ]
        .iter()
        .any(|needle| message.contains(needle))
    }
}

fn check_local_entry(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let linked = metadata.file_type().is_symlink();
            ensure!(
                !linked,
                "Codex 专用存储中存在重定向，已停止：{}",
                path.display()
            );
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn prepare_at(home: &Path, source: &Path) -> Result<()> {
    check_local_entry(home)?;
    fs::create_dir_all(home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    for entry in fs::read_dir(home)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "sessions"
                | "archived_sessions"
                | "history.jsonl"
                | "session_index.jsonl"
                | "auth.json"
                | "config.toml"
        ) || name.contains(".sqlite")
        {
            check_local_entry(&entry.path())?;
        }
    }
    let marker = home.join(".crewvela-profile-v1");
    if marker.is_file() || !source.is_dir() || source.canonicalize()? == home.canonicalize()? {
        return Ok(());
    }
    seed_config(home, source)?;
    for file in ["AGENTS.md", "AGENTS.override.md", "hooks.json"] {
        let from = source.join(file);
        if from.is_file() {
            // Never follow a personal config symlink into a credential store.
            check_local_entry(&from)?;
            seed_file(&from, &home.join(file))?;
        }
    }
    for name in ["rules", "agents", "skills", "profiles"] {
        let from = source.join(name);
        if from.is_dir() {
            let root = from.canonicalize()?;
            seed_directory(
                &root,
                &root,
                &home.join(name),
                home,
                name == "skills",
                matches!(name, "agents" | "profiles"),
                &mut Vec::new(),
            )?;
        }
    }
    crate::providers::write_atomic(&marker, b"1\n")
}

fn read_config(path: &Path) -> Result<toml::Table> {
    check_local_entry(path)?;
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| anyhow!("Codex 配置无法解析，未启动隔离会话")),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(error) => Err(error).context("读取 Codex 配置失败"),
    }
}

fn seed_config(home: &Path, source: &Path) -> Result<()> {
    let target = home.join("config.toml");
    let mut local = read_config(&target)?;
    let mut inherited = read_config(&source.join("config.toml"))?;
    inherited.remove("sqlite_home");
    inherited.remove("log_dir");
    for key in [
        "model_instructions_file",
        "experimental_compact_prompt_file",
        "model_catalog_json",
    ] {
        if let Some(toml::Value::String(value)) = inherited.get_mut(key) {
            if Path::new(value).is_relative() {
                *value = source.join(&*value).to_string_lossy().into_owned();
            }
        }
    }
    if let Some(entries) = inherited
        .get_mut("skills")
        .and_then(|v| v.get_mut("config"))
        .and_then(toml::Value::as_array_mut)
    {
        for entry in entries {
            if let Some(toml::Value::String(path)) = entry.get_mut("path") {
                if let Ok(relative) = Path::new(path).strip_prefix(source.join("skills")) {
                    *path = home
                        .join("skills")
                        .join(relative)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
    }
    // Login/the TUI can precreate just [tui] and [projects]. Retain every
    // explicit local setting and fill missing settings only on first adoption.
    inherit_missing(&mut local, inherited, &mut Vec::new(), home)?;
    local.insert(
        "sqlite_home".into(),
        toml::Value::String(home.to_str().context("Codex 目录无法编码")?.into()),
    );
    crate::providers::write_atomic(&target, toml::to_string(&local)?.as_bytes())
}

fn inherit_missing(
    local: &mut toml::Table,
    inherited: toml::Table,
    parents: &mut Vec<String>,
    home: &Path,
) -> Result<()> {
    for (key, value) in inherited {
        let lower = key.to_ascii_lowercase();
        match value {
            toml::Value::Table(children) => {
                let entry = local
                    .entry(key)
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if let Some(table) = entry.as_table_mut() {
                    parents.push(lower);
                    inherit_missing(table, children, parents, home)?;
                    parents.pop();
                }
            }
            mut value if !local.contains_key(&key) => {
                let gateway = parents.iter().any(|name| {
                    matches!(
                        name.as_str(),
                        "model_providers" | "mcp_servers" | "gateway" | "gateways" | "auth"
                    )
                });
                let reference = lower == "env_key"
                    || lower.ends_with("_env_var")
                    || parents.iter().any(|name| name == "env_http_headers");
                let credential = matches!(
                    lower.as_str(),
                    "authorization"
                        | "token"
                        | "password"
                        | "secret"
                        | "credentials"
                        | "api_key"
                        | "apikey"
                ) || [
                    "_token",
                    "_secret",
                    "_password",
                    "_api_key",
                    "_access_key",
                    "_private_key",
                ]
                .iter()
                .any(|suffix| lower.ends_with(suffix))
                    || (gateway
                        && parents.iter().any(|name| {
                            matches!(name.as_str(), "http_headers" | "headers" | "query_params")
                        }));
                let unsafe_url = matches!(lower.as_str(), "url" | "base_url" | "chatgpt_base_url")
                    && value
                        .as_str()
                        .and_then(|s| reqwest::Url::parse(s).ok())
                        .is_some_and(|url| {
                            !url.username().is_empty()
                                || url.password().is_some()
                                || url.query().is_some()
                        });
                let secret_arg = gateway
                    && lower == "args"
                    && value.as_array().is_some_and(|args| {
                        args.iter().filter_map(toml::Value::as_str).any(|s| {
                            let s = s.to_ascii_lowercase();
                            [
                                "--token",
                                "--api-key",
                                "--password",
                                "authorization:",
                                "bearer ",
                            ]
                            .iter()
                            .any(|needle| s.contains(needle))
                        })
                    });
                ensure!(!((credential && !reference) || unsafe_url || secret_arg),
                    "个人 Codex 配置含有内嵌凭据或请求参数，未复制。请改用环境变量引用，或在专用 {} 配置相应字段后重试", home.join("config.toml").display());
                if matches!(key.as_str(), "CODEX_HOME" | "CODEX_SQLITE_HOME") {
                    value = toml::Value::String(home.to_string_lossy().into_owned());
                }
                local.insert(key, value);
            }
            _ => {}
        }
    }
    Ok(())
}

fn seed_file(source: &Path, target: &Path) -> Result<()> {
    check_local_entry(target)?;
    if !target.exists() {
        crate::providers::write_atomic(target, &fs::read(source)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(source)?.permissions().mode() & 0o100 != 0 {
                fs::set_permissions(target, fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    Ok(())
}

fn seed_directory(
    source: &Path,
    root: &Path,
    target: &Path,
    home: &Path,
    skill_roots: bool,
    config_files: bool,
    ancestors: &mut Vec<PathBuf>,
) -> Result<()> {
    check_local_entry(target)?;
    let real = source.canonicalize()?;
    ensure!(
        !ancestors.contains(&real) && ancestors.len() < 64,
        "Codex 资源目录含有循环链接"
    );
    ancestors.push(real);
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        // Codex installs its own built-in skills. VCS metadata is not a skill asset.
        if name == ".system"
            || name == ".git"
            || name == ".env"
            || name == "auth.json"
            || name == "credentials.json"
        {
            continue;
        }
        let from = entry.path().canonicalize()?;
        let to = target.join(name);
        let metadata = fs::metadata(&from)?;
        // Skill managers commonly install a top-level SKILL.md package as a
        // junction. Subdirectories of that package must stay inside it.
        let child_root = if skill_roots && metadata.is_dir() && from.join("SKILL.md").is_file() {
            &from
        } else {
            ensure!(
                from.starts_with(root),
                "Codex 资源链接离开配置目录，无法安全导入：{}",
                entry.path().display()
            );
            root
        };
        if metadata.is_dir() {
            seed_directory(&from, child_root, &to, home, false, config_files, ancestors)?;
        } else if metadata.is_file() {
            if config_files && to.extension().is_some_and(|ext| ext == "toml") && !to.exists() {
                let mut safe = toml::Table::new();
                inherit_missing(&mut safe, read_config(&from)?, &mut Vec::new(), home)?;
                crate::providers::write_atomic(&to, toml::to_string(&safe)?.as_bytes())?;
            } else {
                seed_file(&from, &to)?;
            }
        }
    }
    ancestors.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_profile_preserves_config_and_skills_without_copying_personal_state_or_login(
    ) -> Result<()> {
        let root = std::env::temp_dir().join(format!("cv-storage-{}", uuid::Uuid::new_v4()));
        let source = root.join("personal");
        let home = root.join("app/codex-home");
        fs::create_dir_all(source.join("skills/example"))?;
        fs::create_dir_all(source.join("rules"))?;
        fs::create_dir_all(source.join("sessions"))?;
        fs::write(source.join("skills/example/SKILL.md"), "skill")?;
        fs::write(source.join("rules/safe.rules"), "policy")?;
        fs::write(source.join("auth.json"), "personal-only")?;
        fs::write(source.join("session_index.jsonl"), "personal-index")?;
        fs::write(source.join("sessions/old.jsonl"), "history")?;
        fs::write(source.join("state_5.sqlite"), "old-db")?;
        let original = "model_provider='my-provider'\nsqlite_home='elsewhere'\nsandbox_mode='read-only'\napproval_policy='on-request'\n[mcp_servers.known]\nurl='http://localhost:4321'\n[projects.workspace]\ntrust_level='untrusted'\n";
        fs::write(source.join("config.toml"), original)?;
        prepare_at(&home, &source)?;
        let config: toml::Value = toml::from_str(&fs::read_to_string(home.join("config.toml"))?)?;
        assert_eq!(config["sqlite_home"].as_str(), home.to_str());
        assert_eq!(config["model_provider"].as_str(), Some("my-provider"));
        assert_eq!(config["sandbox_mode"].as_str(), Some("read-only"));
        assert_eq!(config["approval_policy"].as_str(), Some("on-request"));
        assert_eq!(
            config["projects"]["workspace"]["trust_level"].as_str(),
            Some("untrusted")
        );
        assert_eq!(
            config["mcp_servers"]["known"]["url"].as_str(),
            Some("http://localhost:4321")
        );
        assert_eq!(fs::read(home.join("rules/safe.rules"))?, b"policy");
        assert_eq!(fs::read(home.join("skills/example/SKILL.md"))?, b"skill");
        for name in [
            "auth.json",
            "session_index.jsonl",
            "sessions",
            "state_5.sqlite",
        ] {
            assert!(!home.join(name).exists());
        }
        fs::write(home.join("auth.json"), "separate-login")?;
        fs::write(home.join("config.toml"), "model='my-local-model'")?;
        prepare_at(&home, &source)?;
        assert_eq!(fs::read(home.join("auth.json"))?, b"separate-login");
        assert_eq!(
            fs::read_to_string(home.join("config.toml"))?,
            "model='my-local-model'"
        );
        assert_eq!(fs::read_to_string(source.join("config.toml"))?, original);
        assert_eq!(fs::read(source.join("state_5.sqlite"))?, b"old-db");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn legacy_owner_does_not_trust_a_locator_supplied_directory() -> Result<()> {
        let old = serde_json::json!({"threadId":"previous","codexHome":"/unrelated"});
        assert_eq!(CodexStorage::from_locator(&old)?, CodexStorage::Personal);
        assert!(
            CodexStorage::from_locator(&serde_json::json!({"codexStorage":"/unrelated"})).is_err()
        );
        let mut value = old;
        CodexStorage::CrewVela.mark(&mut value);
        assert_eq!(CodexStorage::from_locator(&value)?, CodexStorage::CrewVela);
        Ok(())
    }

    #[test]
    fn login_created_config_inherits_missing_settings_once_and_keeps_inline_credentials_private(
    ) -> Result<()> {
        let root = std::env::temp_dir().join(format!("cv-config-{}", uuid::Uuid::new_v4()));
        let source = root.join("personal");
        let home = root.join("isolated");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&home)?;
        let local = "model='local-model'\n[tui]\nanimations=false\n[projects.workspace]\ntrust_level='untrusted'\n";
        fs::write(home.join("config.toml"), local)?;
        let provider = "model='source-model'\nmodel_provider='gateway'\nsandbox_mode='read-only'\n[projects.workspace]\ntrust_level='trusted'\n[model_providers.gateway]\nenv_key='MY_GATEWAY_TOKEN'\n";
        fs::write(
            source.join("config.toml"),
            format!("{provider}experimental_bearer_token='test-only-secret-never-copy'"),
        )?;
        let error = prepare_at(&home, &source).unwrap_err().to_string();
        assert!(!error.contains("test-only-secret") && error.contains("内嵌凭据"));
        assert_eq!(fs::read_to_string(home.join("config.toml"))?, local);
        fs::write(source.join("config.toml"), provider)?;
        prepare_at(&home, &source)?;
        let config = read_config(&home.join("config.toml"))?;
        assert_eq!(config["model"].as_str(), Some("local-model"));
        assert_eq!(
            config["projects"]["workspace"]["trust_level"].as_str(),
            Some("untrusted")
        );
        assert_eq!(config["model_provider"].as_str(), Some("gateway"));
        assert_eq!(
            config["model_providers"]["gateway"]["env_key"].as_str(),
            Some("MY_GATEWAY_TOKEN")
        );
        assert_eq!(config["sandbox_mode"].as_str(), Some("read-only"));
        fs::write(home.join("config.toml"), local)?;
        prepare_at(&home, &source)?;
        assert_eq!(fs::read_to_string(home.join("config.toml"))?, local);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn new_and_legacy_codex_members_reopen_only_their_bound_home_and_native_id() -> Result<()> {
        const CASE: &str = "CREWVELA_CODEX_STORAGE_CASE";
        let Some(root) = std::env::var_os(CASE).map(PathBuf::from) else {
            // The subprocess owns its entire environment. Never change CODEX_HOME
            // in the shared test runner, even temporarily.
            let root = std::env::temp_dir().join(format!("cv-history-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root)?;
            fs::write(root.join("test-owner"), "codex storage fixture")?;
            let result = Command::new(std::env::current_exe()?)
                .args(["--exact","codex_storage::tests::new_and_legacy_codex_members_reopen_only_their_bound_home_and_native_id","--nocapture"])
                .env(CASE,&root).env("CODEX_HOME",root.join("personal"))
                .env("CODEX_SQLITE_HOME",root.join("personal-db")).output()?;
            assert!(
                result.status.success()
                    && String::from_utf8_lossy(&result.stdout).contains("1 passed"),
                "{} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            fs::remove_dir_all(root)?;
            return Ok(());
        };
        ensure!(
            root.starts_with(std::env::temp_dir())
                && fs::read(root.join("test-owner"))? == b"codex storage fixture"
        );
        crate::test_profile::initialize(&root.join("app"))?;
        let cwd = root.join("project");
        fs::create_dir_all(&cwd)?;
        let manager = crate::sessions::SessionManager::open_at(
            root.join("sessions.json"),
            root.join("events"),
        )?;
        let member = manager.create_agent(crate::sessions::NewAgentSession {
            agent_id: "member".into(),
            project_id: "project".into(),
            runtime_kind: "codex".into(),
            agent_name: "Codex".into(),
            avatar_id: None,
            cwd: cwd.display().to_string(),
        })?;
        let native = uuid::Uuid::new_v4().to_string();
        let personal_only = uuid::Uuid::new_v4().to_string();
        assert_eq!(member.codex_storage, CodexStorage::CrewVela);
        manager.bind_codex_thread(&member.id, CodexStorage::CrewVela, Some(&native), &cwd)?;
        drop(manager);
        let manager = crate::sessions::SessionManager::open_at(
            root.join("sessions.json"),
            root.join("events"),
        )?;
        let member = manager.get(&member.id).unwrap();
        let mut old = serde_json::to_value(&member)?;
        old.as_object_mut().unwrap().remove("codexStorage");
        let legacy: crate::sessions::SessionRecord = serde_json::from_value(old)?;
        for (storage, id, reply) in [
            (CodexStorage::Personal, &native, "old"),
            (CodexStorage::CrewVela, &native, "new"),
            (CodexStorage::Personal, &personal_only, "outside"),
        ] {
            let directory = storage.home()?.join("sessions");
            fs::create_dir_all(&directory)?;
            let meta = serde_json::json!({"type":"session_meta","payload":{"id":id,"source":"cli","cwd":cwd}});
            let message = serde_json::json!({"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":reply}]}});
            fs::write(
                directory.join(format!("rollout-{id}.jsonl")),
                format!("{meta}\n{message}\n"),
            )?;
        }
        for (record, expected) in [(&member, "new"), (&legacy, "old")] {
            let history = crate::session_switch_history::read(record, &manager)?;
            assert_eq!(history[0].text, expected);
            let mut command = Command::new("codex");
            record.codex_storage.configure(&mut command, false)?;
            manager.configure_runtime_resume(&mut command, record)?;
            let args = command
                .get_args()
                .map(|v| v.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.ends_with(&["resume".into(), native.clone()]));
            assert_eq!(
                command
                    .get_envs()
                    .find(|(key, _)| *key == "CODEX_HOME")
                    .and_then(|(_, v)| v),
                Some(record.codex_storage.home()?.as_os_str())
            );
            assert_eq!(
                crate::session_switch_history::codex_interactive_identity(
                    record,
                    &personal_only,
                    None,
                    None
                )?,
                record.codex_storage.is_personal()
            );
        }
        let standalone = manager.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "codex".into(),
            cwd: cwd.display().to_string(),
            project_id: None,
        })?;
        assert!(standalone.codex_storage.is_personal());
        assert!(!root.join("personal-db").exists());
        Ok(())
    }

    #[test]
    fn redirected_session_directory_cannot_publish_inside_a_personal_index() -> Result<()> {
        let root = std::env::temp_dir().join(format!("cv-storage-link-{}", uuid::Uuid::new_v4()));
        let personal = root.join("personal");
        let home = root.join("app");
        fs::create_dir_all(&personal)?;
        fs::create_dir_all(&home)?;
        let link = home.join("sessions");
        #[cfg(windows)]
        junction::create(&personal, &link)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&personal, &link)?;
        assert!(prepare_at(&home, &personal).is_err());
        assert!(fs::read_dir(&personal)?.next().is_none());
        #[cfg(windows)]
        junction::delete(&link)?;
        #[cfg(unix)]
        fs::remove_file(&link)?;
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
