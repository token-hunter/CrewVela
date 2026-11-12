//! Discover official stable releases. A job resolves one immutable tag/digest tuple.
use super::*;

const API: &str = "https://api.github.com/repos/akitaonrails/ai-memory/releases";
const DOWNLOADS: &str = "https://github.com/akitaonrails/ai-memory/releases/download/";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Release {
    pub version: String,
    pub tag: String,
    pub asset: String,
    pub sha256: String,
    pub size: u64,
    pub url: String,
    pub id: u64,
}

pub(super) fn numbers(value: &str) -> Result<(u64, u64, u64)> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|s| {
            s.is_empty()
                || !s.bytes().all(|b| b.is_ascii_digit())
                || (s.len() > 1 && s.starts_with('0'))
        })
    {
        bail!("无效的稳定版本号");
    }
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}

pub(super) fn compatible(version: &str) -> Result<()> {
    let n = numbers(version)?;
    if n < (2, 6, 0) || n.0 > 3 {
        bail!("ai-memory {version} 尚未适配，请保留当前版本并更新 CrewVela");
    }
    Ok(())
}

pub(super) fn parse(value: &Value, asset: &str) -> Result<Release> {
    if value["draft"] != false || value["prerelease"] != false {
        bail!("只接受官方稳定发行版");
    }
    let tag = value["tag_name"].as_str().context("发行版缺少版本号")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    numbers(version)?;
    let assets = value["assets"].as_array().context("发行版缺少安装包列表")?;
    let matches = assets
        .iter()
        .filter(|a| a["name"] == asset)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!("此版本未提供唯一的当前平台安装包：{asset}");
    }
    let a = matches[0];
    let url = a["browser_download_url"]
        .as_str()
        .context("安装包缺少下载地址")?;
    if url != format!("{DOWNLOADS}{tag}/{asset}") {
        bail!("安装包地址不是固定官方发行版地址");
    }
    let sha256 = a["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("官方发行包缺少 SHA256，未安装未校验的文件")?;
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("官方校验值无效");
    }
    let size = a["size"].as_u64().context("安装包缺少大小信息")?;
    if size == 0 || size > 128 * 1024 * 1024 {
        bail!("安装包大小不在支持范围内");
    }
    Ok(Release {
        version: version.into(),
        tag: tag.into(),
        asset: asset.into(),
        sha256: sha256.to_ascii_lowercase(),
        size,
        url: url.into(),
        id: value["id"].as_u64().context("发行版缺少 ID")?,
    })
}

pub(super) async fn resolve(tag: Option<&str>) -> Result<Release> {
    let package = installation::current_package().context("当前平台不支持快速安装")?;
    let endpoint = if let Some(tag) = tag {
        numbers(tag)?;
        format!("{API}/tags/v{}", tag.strip_prefix('v').unwrap_or(tag))
    } else {
        format!("{API}/latest")
    };
    let response = reqwest::Client::new()
        .get(endpoint)
        .header(
            reqwest::header::USER_AGENT,
            "CrewVela shared-memory updater",
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("无法检查 GitHub 版本；当前安装保持不变，请稍后重试")?
        .error_for_status()
        .context("GitHub 版本检查失败（可能达到限流），当前安装保持不变")?;
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            bail!("GitHub 发行版元数据过大");
        }
        bytes.extend_from_slice(&chunk);
    }
    let release = parse(&serde_json::from_slice(&bytes)?, package.asset)?;
    if let Some(tag) = tag {
        if release.version != tag.trim_start_matches('v') {
            bail!("发行版与所选版本不一致");
        }
    }
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"id":1,"tag_name":"v3.4.5","draft":false,"prerelease":false,"assets":[{"name":"ai-memory-windows-x86_64.zip","size":20,"digest":format!("sha256:{}","a".repeat(64)),"browser_download_url":format!("{DOWNLOADS}v3.4.5/ai-memory-windows-x86_64.zip")}]})
    }
    #[test]
    fn discovery_rejects_prereleases_bad_checksums_ambiguous_assets_and_foreign_urls() {
        let asset = "ai-memory-windows-x86_64.zip";
        assert_eq!(parse(&fixture(), asset).unwrap().version, "3.4.5");
        for (pointer, invalid) in [
            ("/draft", serde_json::json!(true)),
            ("/prerelease", serde_json::json!(true)),
            ("/tag_name", serde_json::json!("v3.4.5-rc1")),
            ("/assets/0/digest", Value::Null),
            (
                "/assets/0/browser_download_url",
                serde_json::json!("https://evil.invalid/file.zip"),
            ),
            ("/assets/0/size", serde_json::json!(129 * 1024 * 1024)),
        ] {
            let mut value = fixture();
            *value.pointer_mut(pointer).unwrap() = invalid;
            assert!(parse(&value, asset).is_err(), "{pointer}");
        }
        let mut value = fixture();
        let a = value["assets"][0].clone();
        value["assets"].as_array_mut().unwrap().push(a);
        assert!(parse(&value, asset).is_err());
        assert!(parse(&fixture(), "missing.tar.gz").is_err());
    }
    #[test]
    fn semantic_versions_compare_numerically_and_gate_unadapted_majors() {
        assert!(numbers("3.10.0").unwrap() > numbers("3.9.9").unwrap());
        assert!(compatible("3.4.5").is_ok());
        assert!(compatible("2.6.0").is_ok());
        for v in [
            "4.0.0",
            "2.5.9",
            "3.04.0",
            "3.4.5-rc1",
            "latest",
            "../../../main",
        ] {
            assert!(compatible(v).is_err());
        }
    }
}
