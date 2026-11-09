//! Kimi managed chat_title, using the same public token-refresh wire contract
//! and credential slot as the installed CLI. No Agent turn or tools are run.
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static AUTH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn access_token(value: &Value) -> Result<&str> {
    value["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .context("Kimi OAuth 凭据不可用")
}

// A concurrent native refresh or logout wins. Never overwrite a newer account
// bundle or recreate credentials the user removed while the request ran.
fn commit_if_current(path: &Path, before: &[u8], fresh: &Value) -> Result<Value> {
    let latest = std::fs::read(path).context("Kimi 凭据在刷新期间被移除，未恢复登录")?;
    if latest != before {
        return Ok(serde_json::from_slice(&latest)?);
    }
    let temporary = path.with_file_name(format!(".cc-title-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        std::fs::set_permissions(&temporary, std::fs::metadata(path)?.permissions())?;
        file.write_all(&serde_json::to_vec(fresh)?)?;
        file.sync_all()?;
        drop(file);
        // Recheck immediately before committing; no shared config is changed.
        let latest = std::fs::read(path)?;
        if latest != before {
            return Ok(serde_json::from_slice(&latest)?);
        }
        std::fs::rename(&temporary, path)?;
        Ok(fresh.clone())
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

async fn json_body(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(anyhow!("Kimi 标题接口响应超出上限"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("Kimi 标题接口响应无效")
}

async fn token(client: &reqwest::Client, path: &Path, force: bool) -> Result<String> {
    let before = std::fs::read(path)?;
    let mut bundle: Value = serde_json::from_slice(&before)?;
    if !force && bundle["expires_at"].as_u64().unwrap_or(0) > now() + 30 {
        return Ok(access_token(&bundle)?.to_string());
    }
    let refresh = bundle["refresh_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .context("Kimi 登录已过期，请在 Kimi 中重新登录")?;
    let response = client
        .post("https://auth.kimi.com/api/oauth/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Kimi 登录刷新失败（{}），保留当前名称",
            response.status()
        ));
    }
    let grant = json_body(response).await?;
    access_token(&grant)?;
    if grant["refresh_token"].as_str().is_none_or(str::is_empty) {
        return Err(anyhow!("Kimi 刷新响应缺少凭据"));
    }
    let expires = grant["expires_in"]
        .as_u64()
        .filter(|seconds| *seconds > 0 && *seconds <= 365 * 24 * 3600)
        .context("Kimi 刷新响应期限无效")?;
    for field in ["access_token", "refresh_token", "scope", "token_type"] {
        if let Some(value) = grant.get(field) {
            bundle[field] = value.clone();
        }
    }
    bundle["expires_in"] = Value::from(expires);
    bundle["expires_at"] = Value::from(now() + expires);
    let accepted = commit_if_current(path, &before, &bundle)?;
    Ok(access_token(&accepted)?.to_string())
}

pub(crate) fn summarize(prompt: &str, model: Option<&str>) -> Result<String> {
    let _lease = crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::KimiCode)?;
    let _guard = AUTH_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow!("Kimi 标题账号锁损坏"))?;
    let root = std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or(
            dirs::home_dir()
                .context("无法定位 Kimi")?
                .join(".kimi-code"),
        );
    let config: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("config.toml"))?)?;
    let selected = model
        .or_else(|| config.get("default_model").and_then(toml::Value::as_str))
        .context("Kimi 没有选择模型")?;
    let route = config
        .get("models")
        .and_then(|value| value.get(selected))
        .and_then(|value| value.get("provider"))
        .and_then(toml::Value::as_str);
    if route != Some("managed:kimi-code") {
        return Err(anyhow!("Kimi 当前模型没有原生托管标题接口"));
    }
    let provider = config
        .get("providers")
        .and_then(|value| value.get("managed:kimi-code"))
        .context("Kimi 未配置托管标题 provider")?;
    let base = provider
        .get("base_url")
        .and_then(toml::Value::as_str)
        .context("Kimi 标题服务缺少 URL")?;
    let oauth = provider
        .get("oauth")
        .context("Kimi provider 未配置 OAuth")?;
    if oauth.get("storage").and_then(toml::Value::as_str) != Some("file")
        || oauth.get("key").and_then(toml::Value::as_str) != Some("oauth/kimi-code")
    {
        return Err(anyhow!("Kimi 自定义 OAuth 槽尚不支持标题服务"));
    }
    let oauth_host = oauth
        .get("oauth_host")
        .and_then(toml::Value::as_str)
        .unwrap_or("https://auth.kimi.com");
    if oauth_host.trim_end_matches('/') != "https://auth.kimi.com"
        || std::env::var_os("KIMI_CODE_OAUTH_HOST").is_some()
        || std::env::var_os("KIMI_OAUTH_HOST").is_some()
    {
        return Err(anyhow!("Kimi 自定义 OAuth 服务器尚不支持标题服务"));
    }
    let url = reqwest::Url::parse(base)?;
    if url.scheme() != "https"
        || url.host_str() != Some("api.kimi.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.path().trim_end_matches('/') != "/coding/v1"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(anyhow!("Kimi 标题服务不属于受支持的官方地址，未发送凭据"));
    }
    let path = root.join("credentials/kimi-code.json");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build()?;
        let mut bearer = token(&client, &path, false).await?;
        for attempt in 0..2 {
            let response = client.post(format!("{}/tools", base.trim_end_matches('/'))).bearer_auth(&bearer).json(&serde_json::json!({"method":"chat_title","params":{"chat_content":format!("user: {prompt}")}})).send().await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 { bearer = token(&client, &path, true).await?; continue; }
            if !response.status().is_success() { return Err(anyhow!("Kimi 原生标题服务返回 {}", response.status())); }
            return json_body(response).await?["title"].as_str().and_then(crate::title_summary::sanitize_title).context("Kimi 标题服务未返回有效标题");
        }
        Err(anyhow!("Kimi 标题服务鉴权失败"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_auth_refresh_preserves_concurrent_native_credentials_and_logout() {
        let root = std::env::temp_dir().join(format!("cc-kimi-auth-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("credential.json");
        let old = serde_json::json!({"access_token":"old"});
        let before = serde_json::to_vec(&old).unwrap();
        let fresh = serde_json::json!({"access_token":"fresh"});
        std::fs::write(&path, &before).unwrap();
        assert_eq!(commit_if_current(&path, &before, &fresh).unwrap(), fresh);
        let newer = serde_json::json!({"access_token":"native-newer"});
        std::fs::write(&path, serde_json::to_vec(&newer).unwrap()).unwrap();
        assert_eq!(commit_if_current(&path, &before, &fresh).unwrap(), newer);
        std::fs::remove_file(&path).unwrap();
        assert!(commit_if_current(&path, &before, &fresh).is_err());
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
