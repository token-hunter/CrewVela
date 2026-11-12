//! Local service credentials never cross endpoint boundaries or enter IPC output.
use super::*;

pub(super) fn token(data: &Path, endpoint: &str) -> Result<Option<String>> {
    if let Ok(value) = std::env::var("AI_MEMORY_AUTH_TOKEN") {
        if !value.trim().is_empty() {
            return Ok(Some(value));
        }
    }
    local_token(data, endpoint)
}

fn local_token(data: &Path, endpoint: &str) -> Result<Option<String>> {
    if loopback_socket_address(endpoint).is_none() {
        return Ok(None);
    }
    let config = data.join("config.toml");
    let text = match std::fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法读取本地记忆认证配置"),
    };
    // A token from a local data directory must never authenticate a remote service.
    let expected = managed_url_from_config(&text)
        .map_err(|_| anyhow::anyhow!("本地记忆认证配置的监听地址无效"))?;
    if loopback_socket_address(endpoint) != loopback_socket_address(&expected)
        || loopback_socket_address(endpoint).is_none()
    {
        return Ok(None);
    }
    let value: toml::Value =
        toml::from_str(&text).map_err(|_| anyhow::anyhow!("本地记忆认证配置无效"))?;
    if let Some(inline) = value
        .get("auth")
        .and_then(|a| a.get("bearer_token"))
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Ok(Some(inline.into()));
    }
    let Some(file) = value
        .get("auth")
        .and_then(|a| a.get("bearer_token_file"))
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let file = PathBuf::from(file);
    let file = if file.is_absolute() {
        file
    } else {
        data.join(file)
    };
    let root = data.canonicalize().context("本地记忆数据目录不可访问")?;
    let path = file
        .canonicalize()
        .context("本地记忆访问令牌文件不可访问")?;
    if !path.starts_with(&root) || std::fs::symlink_metadata(&file)?.file_type().is_symlink() {
        bail!("本地记忆访问令牌路径越界或为链接，请手动核对认证配置");
    }
    let meta = std::fs::metadata(&path)?;
    if !meta.is_file() || meta.len() > 4096 {
        bail!("本地记忆访问令牌文件无效");
    }
    let token = std::fs::read_to_string(path).context("本地记忆访问令牌无法读取")?;
    let token = token.trim();
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("本地记忆访问令牌格式无效");
    }
    Ok(Some(token.into()))
}

pub(super) fn headers(
    data: &Path,
    endpoint: &str,
) -> Result<std::collections::BTreeMap<String, String>> {
    Ok(token(data, endpoint)?
        .map(|token| ("Authorization".into(), format!("Bearer {token}")))
        .into_iter()
        .collect())
}

pub(super) fn command(command: &mut Command, data: &Path, endpoint: &str) -> Result<()> {
    if let Some(token) = token(data, endpoint)? {
        command.env("AI_MEMORY_AUTH_TOKEN", token);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_tokens_do_not_escape_to_remote_or_other_local_services() {
        let root = std::env::temp_dir().join(format!("memory-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("auth-tokens")).unwrap();
        std::fs::write(
            root.join("config.toml"),
            "bind='127.0.0.1:12345'\n[auth]\nbearer_token_file='auth-tokens/default'\n",
        )
        .unwrap();
        std::fs::write(root.join("auth-tokens/default"), "fixture-service-token\n").unwrap();
        assert_eq!(
            local_token(&root, "http://127.0.0.1:12345/mcp")
                .unwrap()
                .as_deref(),
            Some("fixture-service-token")
        );
        for endpoint in [
            "https://example.com",
            "http://127.0.0.1:12346",
            "https://127.0.0.1:12345",
        ] {
            assert!(local_token(&root, endpoint).unwrap().is_none());
        }
        std::fs::write(
            root.join("config.toml"),
            "bind='127.0.0.1:12345'\n[auth]\nbearer_token_file='../outside'\n",
        )
        .unwrap();
        assert!(local_token(&root, "http://127.0.0.1:12345").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
