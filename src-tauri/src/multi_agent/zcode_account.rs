//! Private ZCode Account Host. Secrets travel only over child-process pipes.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub(super) struct AccountHost {
    program: PathBuf,
    env: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
    builtin: PathBuf,
    active_catalog: Option<PathBuf>,
    version: Option<String>,
    pub config: Value,
    scopes: Value,
    start_plan_unavailable: bool,
}
impl AccountHost {
    pub(super) fn active_catalog(&self) -> Option<&Path> {
        self.active_catalog.as_deref()
    }
    pub fn admits(&self, provider: &str) -> bool {
        self.scopes.get(provider).is_some()
    }
    pub fn unavailable_notice(&self) -> Option<String> {
        self.config["states"].as_object().and_then(|states| {
            if states
                .values()
                .any(|state| state["unavailableReason"] == "credential-failed")
            {
                return Some(
                    "ZCode Desktop 账号权益查询暂未成功，请重新加载模型；当前未开放未校验的账号模型".into(),
                );
            }
            if self.start_plan_unavailable {
                return Some("ZCode Desktop Start Plan 当前暂不支持在 CrewVela 中使用，相关模型已隐藏；请选择其他可用模型，或在原生 ZCode Desktop 检查套餐状态".into());
            }
            (!states.is_empty()
                && states
                    .values()
                    .all(|state| state["unavailableReason"] == "not-authenticated"))
            .then(|| "未读取到 ZCode Desktop 当前登录账号，请在原生 ZCode Desktop 登录后重新加载模型".into())
        })
    }
    pub fn prepare(command: &Command) -> Result<Option<Self>> {
        Self::prepare_with_desktop_catalog(command, false)
    }
    pub(super) fn prepare_with_desktop_catalog(command: &Command, allow_desktop_catalog: bool) -> Result<Option<Self>> {
        let env: Vec<_> = command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
            .collect();
        let builtin = command
            .get_envs()
            .find(|(k, _)| *k == "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE")
            .and_then(|(_, v)| v.map(PathBuf::from))
            .or_else(|| std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").map(PathBuf::from));
        let Some(builtin) = builtin else {
            return Ok(None);
        };
        // 辅助脚本以 node -e 执行；shim/exe 启动的 Agent 命令本身不能当 node 用。
        let Some(program) = super::node_helper_program(command) else {
            return Ok(None);
        };
        // start-plan 权益查询要求 app_version；CLI 入口由 launch() 从相邻
        // package.json 注入，桌面布局留空让 helper 从 app.asar 自行探测。
        let version = command
            .get_envs()
            .find(|(key, _)| *key == "CREWVELA_ZCODE_APP_VERSION")
            .and_then(|(_, value)| value)
            .and_then(|value| value.to_str().map(str::to_owned));
        let windows_bundle = cfg!(windows) && program.parent().is_some_and(|root| {
            builtin == root.join("resources/config/provider/zcode-builtin.json")
                && root.join("resources/app.asar").is_file()
        });
        let mac_bundle = cfg!(target_os = "macos") && program.ancestors()
            .find(|path| path.ends_with("Contents/Frameworks"))
            .and_then(Path::parent)
            .is_some_and(|contents| {
                builtin == contents.join("Resources/config/provider/zcode-builtin.json")
                    && contents.join("Resources/app.asar").is_file()
            });
        let use_desktop_catalog = allow_desktop_catalog
            && std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").is_none()
            && (windows_bundle || mac_bundle);
        let mut host = Self {
            program,
            env,
            builtin,
            active_catalog: None,
            version,
            config: Value::Null,
            scopes: Value::Null,
            start_plan_unavailable: false,
        };
        let mut snapshot = json!({"operation":"snapshot","useDesktopCatalog":use_desktop_catalog});
        if let Some(version) = &host.version {
            snapshot["version"] = json!(version);
        }
        let result = host.call(snapshot)?;
        host.active_catalog = result["catalog"].as_str().map(PathBuf::from)
            .filter(|path| path != &host.builtin);
        host.config = result["config"].clone();
        host.scopes = result["scopes"].clone();
        host.start_plan_unavailable = result["startPlanUnavailable"].as_bool().unwrap_or(false);
        Ok(Some(host))
    }
    fn call(&self, mut input: Value) -> Result<Value> {
        // Decrypted control characters can expand to six JSON bytes each.
        let output_limit: u64 = if input["operation"] == "credentialCipher" { 8 * 1024 * 1024 } else { 1024 * 1024 };
        input["builtin"] = json!(self.builtin);
        if let Some(catalog) = &self.active_catalog {
            input["catalog"] = json!(catalog);
        }
        let mut command = Command::new(&self.program);
        command.args(["-e", include_str!("../../resources/zcode-account-host.cjs")]);
        for (key, value) in &self.env {
            match value {
                Some(value) => {
                    command.env(key, value);
                }
                None => {
                    command.env_remove(key);
                }
            }
        }
        command
            .env("CREWVELA_ZCODE_ACCOUNT_HELPER", "1")
            .env_remove("NODE_OPTIONS");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("启动 ZCode Desktop 账号 Host 失败")?;
        let mut stdin = child.stdin.take().context("ZCode Desktop 账号 Host 缺少输入管道")?;
        stdin.write_all(&serde_json::to_vec(&input)?)?;
        drop(stdin);
        let (tx, rx) = mpsc::channel();
        let stdout = child
            .stdout
            .take()
            .context("ZCode Desktop 账号 Host 缺少输出管道")?;
        std::thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            let result = stdout.take(output_limit + 1).read_to_end(&mut bytes);
            let _ = tx.send((result, bytes));
        });
        let received = rx.recv_timeout(Duration::from_secs(45));
        if received.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("ZCode Desktop 账号权益查询超时，请恢复会话重试");
        }
        let (read, bytes) = received.unwrap();
        read.context("读取 ZCode Desktop 账号 Host 失败")?;
        let status = child.wait()?;
        if !status.success() || bytes.len() as u64 > output_limit {
            bail!("ZCode Desktop 账号 Host 响应无效");
        }
        // Never include parsing input or raw helper output in error diagnostics.
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("ZCode Desktop 账号 Host 响应无效"))?;
        if let Some(error) = value["error"].as_str() {
            bail!("{error}");
        }
        value
            .get("result")
            .cloned()
            .context("ZCode Desktop 账号 Host 缺少结果")
    }
    fn grant_for_request<'a>(
        &'a self,
        params: &Value,
        selected: &Value,
    ) -> Result<(String, &'a Value)> {
        let provider = params["providerId"]
            .as_str()
            .context("ZCode Desktop 账号请求缺少 Provider")?;
        if params["modelSelection"]["providerId"] != selected["providerId"]
            || params["modelSelection"]["modelId"] != selected["modelId"]
            || selected["providerId"] != provider
            || params["reason"] != "model-request"
        {
            bail!("ZCode Desktop 账号请求模型身份不匹配");
        }
        let grant = self
            .scopes
            .get(provider)
            .context("ZCode Desktop 账号模型尚未通过权益校验，请恢复会话")?;
        if params["accountAccess"] != grant["access"] {
            bail!("ZCode Desktop 账号请求套餐身份不匹配");
        }
        Ok((provider.to_owned(), grant))
    }
    pub fn auth(&self, params: &Value, selected: &Value) -> Result<Value> {
        let (provider, grant) = self.grant_for_request(params, selected)?;
        self.call(json!({"operation":"auth","providerId":provider,"grant":grant}))
    }
    pub fn credential_cipher(&self, params: &Value) -> Result<Value> {
        self.call(json!({"operation":"credentialCipher","request":params}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zcode_account_query_failure_has_retry_notice_without_grant() {
        let mut host = AccountHost {
            program: PathBuf::new(),
            env: Vec::new(),
            builtin: PathBuf::new(),
            active_catalog: None,
            version: None,
            config: json!({"states":{"account:fixture":{"unavailableReason":"credential-failed"}}}),
            scopes: json!({}),
            start_plan_unavailable: false,
        };
        assert!(host.unavailable_notice().unwrap().contains("重新加载模型"));
        assert!(!host.admits("account:fixture"));
        host.config["states"]["account:fixture"]["unavailableReason"] = json!("not-entitled");
        assert!(host.unavailable_notice().is_none());
        host.config["states"]["account:fixture"]["unavailableReason"] = json!("not-authenticated");
        assert!(host
            .unavailable_notice()
            .unwrap()
            .contains("原生 ZCode Desktop 登录"));
        host.config["states"]["account:other"] = json!({"unavailableReason":"not-entitled"});
        assert!(host.unavailable_notice().is_none());
        host.config["states"]["account:other"]["unavailableReason"] = json!("credential-failed");
        assert!(host.unavailable_notice().unwrap().contains("权益查询"));
        host.config["states"] = json!({});
        assert!(host.unavailable_notice().is_none());
        host.config["states"] = json!({"account:start":{"unavailableReason":"not-entitled"}});
        host.start_plan_unavailable = true;
        assert!(host.unavailable_notice().unwrap().contains("Start Plan 当前暂不支持"));
        assert!(!host.admits("account:start"));
    }
    #[test]
    fn zcode_account_request_matches_model_identity_without_reasoning_options() {
        let access = json!({"type":"zhipu-account","accountType":"bigmodel","mode":"individual-coding-plan","entitled":true});
        let host = AccountHost {
            program: PathBuf::new(),
            env: Vec::new(),
            builtin: PathBuf::new(),
            active_catalog: None,
            version: None,
            config: Value::Null,
            scopes: json!({"account:fixture":{"access":access,"scope":"fixture"}}),
            start_plan_unavailable: false,
        };
        let selection = json!({"providerId":"account:fixture","modelId":"GLM-4.7","options":{"reasoningLevel":"high"}});
        let params = json!({"providerId":"account:fixture","modelSelection":{"providerId":"account:fixture","modelId":"GLM-4.7"},"accountAccess":access,"reason":"model-request"});
        assert!(host.grant_for_request(&params, &selection).is_ok());
        for pointer in [
            "/providerId",
            "/modelSelection/modelId",
            "/accountAccess/mode",
        ] {
            let mut wrong = params.clone();
            *wrong.pointer_mut(pointer).unwrap() = json!("foreign");
            assert!(host.grant_for_request(&wrong, &selection).is_err());
        }
        assert!(host.grant_for_request(&params, &Value::Null).is_err());
    }
}
