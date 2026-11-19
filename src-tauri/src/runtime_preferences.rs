use std::io::Write;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::multi_agent::AgentRuntimeKind;
use crate::sessions::{SessionKind, SessionRecord};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePreferences {
    #[serde(default)]
    pub disabled_runtimes: Vec<AgentRuntimeKind>,
    #[serde(default)]
    pub default_launch_modes: BTreeMap<String, String>,
}

struct PolicyState {
    preferences: RuntimePreferences,
    active: Vec<(AgentRuntimeKind, usize)>,
}

struct RuntimePolicy {
    path: PathBuf,
    state: Arc<Mutex<PolicyState>>,
}

/// A lease closes the check/spawn race and keeps a runtime enabled until its work exits.
pub(crate) struct RuntimeLease {
    state: Arc<Mutex<PolicyState>>,
    kind: AgentRuntimeKind,
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some((_, count)) = state.active.iter_mut().find(|(kind, _)| *kind == self.kind) {
                *count = count.saturating_sub(1);
            }
        }
    }
}

fn validate(kind: AgentRuntimeKind, preferences: &RuntimePreferences) -> Result<()> {
    if kind.is_retired() {
        return Err(anyhow!("该 CLI 已移除，请更换 Runtime 后重试"));
    }
    if preferences.disabled_runtimes.contains(&kind) {
        return Err(anyhow!(
            "{} 已全局禁用，请先在协作中心启用 Runtime",
            kind.as_str()
        ));
    }
    Ok(())
}

impl RuntimePolicy {
    fn open_at(path: PathBuf) -> Result<Self> {
        let preferences = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("解析 Runtime 设置失败")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                RuntimePreferences::default()
            }
            Err(error) => return Err(error).context("读取 Runtime 设置失败"),
        };
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(PolicyState {
                preferences,
                active: Vec::new(),
            })),
        })
    }

    fn preferences(&self) -> Result<RuntimePreferences> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow!("Runtime 设置锁已损坏"))?
            .preferences
            .clone())
    }

    fn acquire(&self, kind: AgentRuntimeKind) -> Result<RuntimeLease> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("Runtime 设置锁已损坏"))?;
        validate(kind, &state.preferences)?;
        if let Some((_, count)) = state
            .active
            .iter_mut()
            .find(|(runtime, _)| *runtime == kind)
        {
            *count += 1;
        } else {
            state.active.push((kind, 1));
        }
        Ok(RuntimeLease {
            state: self.state.clone(),
            kind,
        })
    }

    fn acquire_if_available(&self, kind: AgentRuntimeKind) -> Result<Option<RuntimeLease>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("Runtime 设置锁已损坏"))?;
        if kind.is_retired() || state.preferences.disabled_runtimes.contains(&kind) {
            return Ok(None);
        }
        if let Some((_, count)) = state
            .active
            .iter_mut()
            .find(|(runtime, _)| *runtime == kind)
        {
            *count += 1;
        } else {
            state.active.push((kind, 1));
        }
        Ok(Some(RuntimeLease {
            state: self.state.clone(),
            kind,
        }))
    }

    fn set_enabled(
        &self,
        kind: AgentRuntimeKind,
        enabled: bool,
        is_busy: impl FnOnce() -> Result<bool>,
    ) -> Result<RuntimePreferences> {
        if enabled && kind.is_retired() {
            validate(kind, &RuntimePreferences::default())?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("Runtime 设置锁已损坏"))?;
        if enabled == !state.preferences.disabled_runtimes.contains(&kind) {
            return Ok(state.preferences.clone());
        }
        if !enabled
            && (state
                .active
                .iter()
                .any(|(runtime, count)| *runtime == kind && *count > 0)
                || is_busy()?)
        {
            return Err(anyhow!(
                "{} 有运行中的会话、任务或调度器，请先停止后再禁用",
                kind.as_str()
            ));
        }
        let mut next = state.preferences.clone();
        next.disabled_runtimes.retain(|runtime| *runtime != kind);
        if !enabled {
            next.disabled_runtimes.push(kind);
        }
        self.save(&next)?;
        state.preferences = next.clone();
        Ok(next)
    }

    fn set_launch_default(&self, kind: &str, mode: &str) -> Result<RuntimePreferences> {
        crate::runtime_launch::validate(kind, mode)?;
        let mut state = self.state.lock().map_err(|_| anyhow!("Runtime 设置锁已损坏"))?;
        let mut next = state.preferences.clone();
        next.default_launch_modes.insert(kind.into(), mode.into());
        self.save(&next)?;
        state.preferences = next.clone();
        Ok(next)
    }

    fn save(&self, next: &RuntimePreferences) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).context("创建 Runtime 设置目录失败")?;
        }
        let bytes = serde_json::to_vec_pretty(&next).context("序列化 Runtime 设置失败")?;
        let temp = self
            .path
            .with_extension(format!("crewvela-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .context("创建 Runtime 设置临时文件失败")?;
            file.write_all(&bytes).context("写入 Runtime 设置失败")?;
            file.sync_all().context("同步 Runtime 设置失败")?;
            drop(file);
            std::fs::rename(&temp, &self.path).context("保存 Runtime 设置失败")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result?;
        Ok(())
    }
}

fn policy() -> Result<&'static RuntimePolicy> {
    static POLICY: OnceLock<Result<RuntimePolicy, String>> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            #[cfg(test)]
            return RuntimePolicy::open_at(
                std::env::temp_dir()
                    .join(format!("cc-runtime-tests-{}", uuid::Uuid::new_v4()))
                    .join("preferences.json"),
            )
            .map_err(|error| format!("{error:#}"));
            #[cfg(not(test))]
            crate::providers::config_dir()
                .and_then(|directory| {
                    RuntimePolicy::open_at(directory.join("runtime-preferences.json"))
                })
                .map_err(|error| format!("{error:#}"))
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

#[cfg(test)]
thread_local! { static TEST_POLICY: std::cell::RefCell<Option<Arc<RuntimePolicy>>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
pub(crate) fn with_disabled_for_test<T>(kind: AgentRuntimeKind, run: impl FnOnce() -> T) -> T {
    with_test_policy(|policy| { policy.set_enabled(kind, false, || Ok(false)).unwrap(); }, run)
}

#[cfg(test)]
pub(crate) fn with_launch_default_for_test<T>(kind: &str, mode: &str, run: impl FnOnce() -> T) -> T {
    with_test_policy(|policy| { policy.set_launch_default(kind, mode).unwrap(); }, run)
}

#[cfg(test)]
fn with_test_policy<T>(configure: impl FnOnce(&RuntimePolicy), run: impl FnOnce() -> T) -> T {
    let path = std::env::temp_dir()
        .join(format!("cc-disabled-runtime-{}", uuid::Uuid::new_v4()))
        .join("preferences.json");
    let policy = Arc::new(RuntimePolicy::open_at(path.clone()).unwrap());
    configure(&policy);
    struct Reset {
        previous: Option<Arc<RuntimePolicy>>,
        path: PathBuf,
    }
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_POLICY.with(|slot| *slot.borrow_mut() = self.previous.take());
            let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
        }
    }
    let previous = TEST_POLICY.with(|slot| slot.replace(Some(policy)));
    let _reset = Reset { previous, path };
    run()
}

pub(crate) fn get() -> Result<RuntimePreferences> {
    #[cfg(test)]
    if let Some(result) =
        TEST_POLICY.with(|policy| policy.borrow().as_ref().map(|policy| policy.preferences()))
    {
        return result;
    }
    policy()?.preferences()
}
pub(crate) fn set_launch_default(kind: &str, mode: &str) -> Result<RuntimePreferences> {
    policy()?.set_launch_default(kind, mode)
}
pub(crate) fn ensure_available(kind: AgentRuntimeKind) -> Result<()> {
    validate(kind, &get()?)
}
pub(crate) fn acquire(kind: AgentRuntimeKind) -> Result<RuntimeLease> {
    #[cfg(test)]
    if let Some(result) =
        TEST_POLICY.with(|policy| policy.borrow().as_ref().map(|policy| policy.acquire(kind)))
    {
        return result;
    }
    policy()?.acquire(kind)
}
pub(crate) fn acquire_if_available(kind: AgentRuntimeKind) -> Result<Option<RuntimeLease>> {
    #[cfg(test)]
    if let Some(result) = TEST_POLICY.with(|policy| {
        policy
            .borrow()
            .as_ref()
            .map(|policy| policy.acquire_if_available(kind))
    }) {
        return result;
    }
    policy()?.acquire_if_available(kind)
}
pub(crate) fn set_enabled(
    kind: AgentRuntimeKind,
    enabled: bool,
    is_busy: impl FnOnce() -> Result<bool>,
) -> Result<RuntimePreferences> {
    policy()?.set_enabled(kind, enabled, is_busy)
}

pub(crate) fn session_runtime(record: &SessionRecord) -> Result<Option<AgentRuntimeKind>> {
    match record.agent_runtime_kind.as_deref() {
        Some(kind) => Ok(Some(
            serde_json::from_value(serde_json::Value::String(kind.into()))
                .context("会话 Runtime 类型无效")?,
        )),
        None if record.kind == SessionKind::Claude => Ok(Some(AgentRuntimeKind::ClaudeCode)),
        None => Ok(None),
    }
}

pub(crate) fn ensure_session_available(record: &SessionRecord) -> Result<()> {
    if let Some(kind) = session_runtime(record)? {
        ensure_available(kind)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temporary_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("cc-runtime-policy-{}", uuid::Uuid::new_v4()))
            .join("preferences.json")
    }

    #[test]
    fn runtime_preferences_default_persist_reenable_and_reject_removed_runtime() {
        let path = temporary_path();
        let policy = RuntimePolicy::open_at(path.clone()).unwrap();
        assert!(policy.preferences().unwrap().disabled_runtimes.is_empty());
        assert!(policy.acquire(AgentRuntimeKind::OhMyPi).is_err());
        assert!(policy.acquire(AgentRuntimeKind::MiniMaxCode).is_err());
        assert!(policy.set_enabled(AgentRuntimeKind::MiniMaxCode, true, || Ok(false)).is_err());
        assert!(policy
            .set_enabled(AgentRuntimeKind::OhMyPi, true, || Ok(false))
            .is_err());
        policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(false))
            .unwrap();
        assert!(policy.acquire(AgentRuntimeKind::Codex).is_err());
        assert!(RuntimePolicy::open_at(path.clone())
            .unwrap()
            .preferences()
            .unwrap()
            .disabled_runtimes
            .contains(&AgentRuntimeKind::Codex));
        policy
            .set_enabled(AgentRuntimeKind::Codex, true, || Ok(false))
            .unwrap();
        assert!(policy.acquire(AgentRuntimeKind::Codex).is_ok());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_runtime_preferences_preserve_retained_runtime_policy_and_refuse_launch() {
        let path = temporary_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"disabledRuntimes":["removed_runtime","codex"]}"#).unwrap();
        let policy = RuntimePolicy::open_at(path.clone()).unwrap();
        assert!(policy.acquire(AgentRuntimeKind::Unsupported).is_err());
        assert!(policy.set_enabled(AgentRuntimeKind::Unsupported, true, || Ok(false)).is_err());
        assert!(policy.acquire(AgentRuntimeKind::Codex).is_err());
        assert!(policy.acquire(AgentRuntimeKind::Pi).is_ok());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_preferences_busy_lease_and_process_block_disable() {
        let path = temporary_path();
        let policy = RuntimePolicy::open_at(path.clone()).unwrap();
        let lease = policy.acquire(AgentRuntimeKind::Codex).unwrap();
        assert!(policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(false))
            .is_err());
        drop(lease);
        assert!(policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(true))
            .is_err());
        assert!(policy.preferences().unwrap().disabled_runtimes.is_empty());
        policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(false))
            .unwrap();
        assert!(policy.acquire(AgentRuntimeKind::ClaudeCode).is_ok());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_preferences_failed_persistence_preserves_previous_policy() {
        let path = temporary_path();
        let policy = RuntimePolicy::open_at(path.clone()).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        assert!(policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(false))
            .is_err());
        assert!(policy.preferences().unwrap().disabled_runtimes.is_empty());
        assert!(policy.acquire(AgentRuntimeKind::Codex).is_ok());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn launch_defaults_survive_reload_and_runtime_switches_without_partial_writes() {
        let path = temporary_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"disabledRuntimes":["codex"]}"#).unwrap();
        let policy = RuntimePolicy::open_at(path.clone()).unwrap();
        assert!(policy.preferences().unwrap().default_launch_modes.is_empty());
        policy.set_launch_default("claude_code", "auto").unwrap();
        policy.set_launch_default("pi", "read_tools").unwrap();
        assert!(policy.set_launch_default("pi", "yolo").is_err());
        policy.set_enabled(AgentRuntimeKind::Codex, true, || Ok(false)).unwrap();
        let reloaded = RuntimePolicy::open_at(path.clone()).unwrap().preferences().unwrap();
        assert_eq!(reloaded.default_launch_modes["claude_code"], "auto");
        assert_eq!(reloaded.default_launch_modes["pi"], "read_tools");
        assert!(reloaded.disabled_runtimes.is_empty());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(policy.set_launch_default("claude_code", "yolo").is_err());
        assert_eq!(policy.preferences().unwrap().default_launch_modes["claude_code"], "auto");
        assert!(path.parent().unwrap().canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn runtime_preferences_disabled_session_rejects_creation_spawn_and_delivery_preserves_history_and_terminal(
    ) {
        use crate::sessions::{
            NewAgentSession, NewRuntimeSession, NewTerminalSession, SessionManager, TerminalShell,
        };
        let path = temporary_path();
        let policy = Arc::new(RuntimePolicy::open_at(path.clone()).unwrap());
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_POLICY.with(|policy| *policy.borrow_mut() = None);
            }
        }
        TEST_POLICY.with(|slot| *slot.borrow_mut() = Some(policy.clone()));
        let _reset = Reset;
        let root = path.parent().unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let history = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        policy
            .set_enabled(AgentRuntimeKind::Codex, false, || Ok(false))
            .unwrap();
        assert!(sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: ".".into(),
                project_id: None
            })
            .is_err());
        assert!(sessions
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "codex".into(),
                agent_name: "member".into(),
                cwd: ".".into()
            })
            .is_err());
        assert!(sessions.begin_runtime_start(&history.id).is_err());
        assert!(sessions
            .queue_runtime_delivery(&history.id, "message", "hello")
            .is_err());
        assert!(sessions.get(&history.id).is_some());
        assert_eq!(sessions.list().len(), 1);
        let terminal = sessions
            .create_terminal(NewTerminalSession {
                shell: TerminalShell::Powershell,
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        assert!(sessions.begin_runtime_start(&terminal.id).is_ok());
        assert!(session_runtime(&terminal).unwrap().is_none());
        assert!(sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "pi".into(),
                cwd: ".".into(),
                project_id: None
            })
            .is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_preferences_disabled_claude_rejects_auxiliary_processes_before_launch() {
        with_disabled_for_test(AgentRuntimeKind::ClaudeCode, || {
            let provider: crate::providers::ProviderConfig = serde_json::from_value(serde_json::json!({
                "id": "test", "name": "test", "baseUrl": "http://localhost", "authTokenRef": "test"
            })).unwrap();
            for error in [
                crate::title_summary::summarize_title(&provider, "unused", "title").unwrap_err(),
                crate::prompt_optimizer::optimize_prompt("draft", None).unwrap_err(),
                crate::git_source_control::generate_commit_message(
                    std::path::Path::new("missing"),
                    &provider,
                    "unused",
                    "中文",
                )
                .unwrap_err(),
                crate::acp::AcpManager::default()
                    .start(".", None)
                    .unwrap_err(),
            ] {
                assert!(error.to_string().contains("已全局禁用"), "{error:#}");
            }
        });
    }
}
