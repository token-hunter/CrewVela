//! Manual application metadata and privacy-directory access. No download or install path.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, time::Duration};
use std::path::Path;

const REPOSITORY: &str = "https://github.com/token-hunter/CrewVela";
const RELEASE_API: &str = "https://api.github.com/repos/token-hunter/CrewVela/releases";
const RELEASE_PAGE_SIZE: usize = 30;
const MAX_METADATA_BYTES: usize = 1024 * 1024;
pub(crate) const UPDATE_MANIFEST: &str = "crewvela-update.json";

fn checked_application_directory(path: &Path) -> Result<String, String> {
    if !path.is_absolute() || !path.is_dir() { return Err("应用数据目录不可用，请检查目录或文件系统权限。".into()); }
    path.to_str().map(str::to_owned).ok_or_else(|| "应用数据目录无法表示为有效文本。".into())
}

#[tauri::command]
pub async fn app_data_directory() -> Result<String, String> {
    crate::blocking_command(|| {
        let directory = crate::providers::config_dir().map_err(|_| "无法读取当前应用数据目录。".to_string())?;
        checked_application_directory(&directory)
    }).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateCheck {
    current_version: String,
    pub(crate) latest_version: Option<String>,
    pub(crate) status: &'static str,
    release_url: Option<String>,
    release_notes: String,
    can_install: bool,
    #[serde(skip)]
    pub(crate) signed_update: Option<SignedUpdate>,
}

#[derive(Debug)]
pub(crate) struct SignedUpdate {
    pub(crate) release_tag: String,
    pub(crate) asset_name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

fn find_signed_update(release: &Release, os: &str, arch: &str) -> Option<SignedUpdate> {
    if (os, arch) != ("windows", "x86_64") {
        return None;
    }
    let uploaded = |name: &str| {
        release.assets.iter().any(|asset| asset.name == name && asset.state == "uploaded" && asset.size > 0)
    };
    if !uploaded(UPDATE_MANIFEST) {
        return None;
    }
    release.assets.iter().find_map(|asset| {
        let sha256 = asset.digest.as_deref()?.strip_prefix("sha256:")?;
        (sha256.len() == 64 && sha256.bytes().all(|c| c.is_ascii_hexdigit())
            && asset.name.ends_with("-setup.exe")
            && asset.name.bytes().all(|c| c.is_ascii_alphanumeric() || b".-+_".contains(&c))
            && asset.size <= 512 * 1024 * 1024
            && is_compatible_installer(asset, os, arch)
            && uploaded(&format!("{}.sig", asset.name)))
            .then(|| SignedUpdate {
                release_tag: release.tag_name.clone(), asset_name: asset.name.clone(),
                size: asset.size, sha256: sha256.to_ascii_lowercase(),
            })
    })
}

fn is_beta(version: &Version) -> bool {
    version.pre.as_str().split('.').next() == Some("beta")
}

fn is_compatible_installer(asset: &ReleaseAsset, os: &str, arch: &str) -> bool {
    if asset.state != "uploaded" || asset.size == 0 {
        return false;
    }
    let name = asset.name.to_ascii_lowercase();
    if !name.starts_with("crewvela-") && !name.starts_with("crewvela_") {
        return false;
    }
    let architectures: &[&str] = match (os, arch) {
        ("windows", "x86") => &["x86", "i686"],
        ("windows", "x86_64") => &["x64", "x86_64", "amd64"],
        ("windows", "aarch64") => &["arm64", "aarch64"],
        ("macos", "x86_64") => &["x64", "x86_64", "universal"],
        ("macos", "aarch64") => &["arm64", "aarch64", "universal"],
        ("linux", "x86_64") => &["x64", "x86_64", "amd64"],
        ("linux", "aarch64") => &["arm64", "aarch64"],
        _ => return false,
    };
    let extensions: &[&str] = match os {
        "windows" => &["-setup.exe", "_en-us.msi", ".msi"],
        "macos" => &[".dmg"],
        "linux" => &[".appimage", ".deb", ".rpm"],
        _ => return false,
    };
    let stem = extensions.iter().find_map(|ext| name.strip_suffix(*ext));
    stem.is_some_and(|stem| {
        architectures.iter().any(|arch| {
            stem.strip_suffix(arch)
                .is_some_and(|prefix| prefix.ends_with(['-', '_', '.']))
        })
    })
}

fn eligible_version(release: &Release, allow_beta: bool, os: &str, arch: &str) -> Option<Version> {
    if release.draft {
        return None;
    }
    let tag = &release.tag_name;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    let eligible_channel = if release.prerelease {
        allow_beta && is_beta(&version)
    } else {
        version.pre.is_empty()
    };
    (eligible_channel && release.assets.iter().any(|asset| is_compatible_installer(asset, os, arch)))
        .then_some(version)
}

fn response_error(status: u16) -> String {
    match status {
        404 => "发布源不可用（HTTP 404）：无法访问 CrewVela 的 GitHub 发布列表。".into(),
        403 | 429 => format!("GitHub 访问受限或已达到请求频率限制（HTTP {status}），请稍后重试。"),
        _ => format!("发行服务返回 HTTP {status}，未完成检查，请稍后重试。"),
    }
}
async fn fetch_release_page(client: &reqwest::Client, endpoint: &str, current: &str, page: usize) -> Result<Vec<Release>, String> {
    let mut response = client.get(endpoint)
        .query(&[("per_page", RELEASE_PAGE_SIZE), ("page", page)])
        .header(reqwest::header::ACCEPT,"application/vnd.github+json")
        .header(reqwest::header::USER_AGENT,format!("CrewVela/{current}"))
        .send().await.map_err(|error| if error.is_timeout() { "检查更新超时，请检查网络后重试。" } else { "无法连接发行服务，请检查通用设置中的网络代理或稍后重试。" })?;
    if !response.status().is_success() { return Err(response_error(response.status().as_u16())); }
    if response.content_length().is_some_and(|size| size > MAX_METADATA_BYTES as u64) { return Err("发行信息过大，未完成检查。".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取发行信息失败，未完成检查。")? {
        if bytes.len() + chunk.len() > MAX_METADATA_BYTES { return Err("发行信息过大，未完成检查。".into()); }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "发行信息格式无效，未完成检查。".into())
}

async fn fetch_update(client: reqwest::Client, endpoint: &str, current: &str, os: &str, arch: &str) -> Result<AppUpdateCheck, String> {
    let current_version = Version::parse(current).map_err(|_| "当前程序版本无效，无法比较更新。")?;
    let allow_beta = is_beta(&current_version);
    let mut newest: Option<(Version, Release)> = None;
    for page in 1.. {
        let releases = fetch_release_page(&client, endpoint, current, page).await?;
        let has_more = releases.len() == RELEASE_PAGE_SIZE;
        for release in releases {
            if let Some(version) = eligible_version(&release, allow_beta, os, arch) {
                if newest.as_ref().is_none_or(|(best, _)| version.cmp_precedence(best).is_gt()) {
                    newest = Some((version, release));
                }
            }
        }
        if !has_more {
            break;
        }
    }
    let Some((latest, release)) = newest else {
        return Ok(AppUpdateCheck {
            current_version: current_version.to_string(),
            latest_version: None,
            status: "unavailable",
            release_url: None,
            release_notes: String::new(),
            can_install: false,
            signed_update: None,
        });
    };
    let status = match latest.cmp_precedence(&current_version) {
        Ordering::Greater => "available",
        Ordering::Equal => "current",
        Ordering::Less => "ahead",
    };
    let signed_update = find_signed_update(&release, os, arch);
    Ok(AppUpdateCheck {
        current_version: current_version.to_string(),
        latest_version: Some(latest.to_string()),
        status,
        // Construct the link from the pinned repository and validated SemVer tag.
        release_url: Some(format!("{REPOSITORY}/releases/tag/{}", release.tag_name)),
        release_notes: release.body.unwrap_or_default().chars().take(6000).collect(),
        can_install: false,
        signed_update,
    })
}

#[tauri::command]
pub async fn app_check_update(app: tauri::AppHandle) -> Result<AppUpdateCheck, String> {
    if crate::test_profile::active() { return Err("隔离测试模式不访问在线发布源。".into()); }
    let client = crate::general_settings::configure_http_client(reqwest::Client::builder())
        .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "无法创建更新客户端，请检查网络设置。")?;
    let current = app.package_info().version.to_string();
    let mut result = tokio::time::timeout(
        Duration::from_secs(15),
        fetch_update(client, RELEASE_API, &current, std::env::consts::OS, std::env::consts::ARCH),
    )
    .await
    .map_err(|_| "检查更新超时，请检查网络后重试。")??;
    result.can_install = result.status == "available" && result.signed_update.is_some()
        && crate::app_updater::embedded_key().is_some();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_directory_is_absolute_existing_and_read_only() {
        let root=std::env::temp_dir().join(format!("crewvela-privacy-directory-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        assert_eq!(checked_application_directory(&root).unwrap(),root.to_str().unwrap());
        assert_eq!(crate::workspace_files::resolve_path(root.to_str().unwrap(),".").unwrap(),root.canonicalize().unwrap());
        assert!(checked_application_directory(Path::new("relative-folder")).is_err());
        let missing=root.join("missing");assert!(checked_application_directory(&missing).is_err());assert!(!missing.exists());
        let file=root.join("record.txt");std::fs::write(&file,"fixture").unwrap();assert!(checked_application_directory(&file).is_err());assert_eq!(std::fs::read_to_string(&file).unwrap(),"fixture");
        std::fs::remove_file(file).unwrap();std::fs::remove_dir(root).unwrap();
    }
    const WINDOWS_INSTALLER: &str = "CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe";
    const MAC_INSTALLER: &str = "CrewVela_0.1.0_macos_aarch64.dmg";

    #[test]
    fn in_app_update_requires_the_exact_signed_windows_asset_and_manifest() {
        let mut value = release("v0.1.1-beta.20261128", true, WINDOWS_INSTALLER);
        let digest = "a".repeat(64);
        value["assets"][0]["digest"] = format!("sha256:{digest}").into();
        let unsigned: Release = serde_json::from_value(value.clone()).unwrap();
        assert!(find_signed_update(&unsigned, "windows", "x86_64").is_none());
        value["assets"].as_array_mut().unwrap().extend([
            serde_json::json!({"name": UPDATE_MANIFEST, "state": "uploaded", "size": 128}),
            serde_json::json!({"name": format!("{WINDOWS_INSTALLER}.sig"), "state": "uploaded", "size": 128}),
        ]);
        let release: Release = serde_json::from_value(value.clone()).unwrap();
        let update = find_signed_update(&release, "windows", "x86_64").unwrap();
        assert_eq!(update.asset_name, WINDOWS_INSTALLER);
        assert_eq!(update.sha256, digest);
        assert_eq!(update.release_tag, "v0.1.1-beta.20261128");
        assert!(find_signed_update(&release, "windows", "aarch64").is_none());
        value["assets"][2]["name"] = "AnotherVersion.exe.sig".into();
        let wrong_signature: Release = serde_json::from_value(value).unwrap();
        assert!(find_signed_update(&wrong_signature, "windows", "x86_64").is_none());
    }

    fn release(tag: &str, prerelease: bool, asset: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag, "draft": false, "prerelease": prerelease,
            "body": "更新说明", "html_url": "https://untrusted.invalid/download.exe",
            "assets": [{"name": asset, "state": "uploaded", "size": 123}]
        })
    }
    #[tokio::test]
    async fn version_precedence_and_build_metadata() {
        for (current, latest, status) in [
            ("0.1.0", "v0.2.0", "available"),
            ("1.2.0", "v1.10.0", "available"),
            ("1.2.3+local", "v1.2.3+release", "current"),
            ("2.0.0", "v1.9.9", "ahead"),
            ("1.2.3-beta.1", "v1.2.3", "available"),
        ] {
            let check = check_releases(&[release(latest, false, WINDOWS_INSTALLER)], current, "windows", "x86_64").await;
            assert_eq!(check.status, status);
        }
    }
    #[tokio::test]
    async fn channels_select_highest_compatible_version_and_pin_the_release_url() {
        let mut draft = release("v9.0.0", false, WINDOWS_INSTALLER);
        draft["draft"] = true.into();
        let mut empty_asset = release("v8.0.0", false, WINDOWS_INSTALLER);
        empty_asset["assets"][0]["size"] = 0.into();
        let mut pending_asset = release("v7.0.0", false, WINDOWS_INSTALLER);
        pending_asset["assets"][0]["state"] = "new".into();
        let releases = [
            release("v0.1.1-beta.20261127", true, WINDOWS_INSTALLER),
            draft, empty_asset, pending_asset,
            release("v4.0.0-rc.1", true, WINDOWS_INSTALLER),
            release("v3.0.0-beta.1", false, WINDOWS_INSTALLER),
            release("v2.0.0", false, MAC_INSTALLER),
            release("v1.0.0", false, "SHA256SUMS.txt"),
            release("v0.1.1-beta.20261128", true, WINDOWS_INSTALLER),
            release("v0.1.0", false, WINDOWS_INSTALLER),
            release("v0.2.0/extra", false, WINDOWS_INSTALLER),
        ];
        let beta = check_releases(&releases, "0.1.1-beta.20261127", "windows", "x86_64").await;
        assert_eq!(beta.status, "available");
        assert_eq!(beta.latest_version.as_deref(), Some("0.1.1-beta.20261128"));
        assert_eq!(beta.release_url.as_deref(), Some("https://github.com/token-hunter/CrewVela/releases/tag/v0.1.1-beta.20261128"));
        let stable = check_releases(&releases, "0.1.0", "windows", "x86_64").await;
        assert_eq!(stable.status, "current");
        assert_eq!(stable.latest_version.as_deref(), Some("0.1.0"));
        let mac = check_releases(&releases, "0.1.0-beta.1", "macos", "aarch64").await;
        assert_eq!(mac.latest_version.as_deref(), Some("2.0.0"));
    }
    #[tokio::test]
    async fn missing_channel_or_platform_has_no_download() {
        let releases = [release("v0.1.1-beta.20261128", true, WINDOWS_INSTALLER)];
        for (current, os, arch) in [
            ("0.1.0", "windows", "x86_64"),
            ("0.1.0-beta.1", "windows", "aarch64"),
            ("0.1.0-beta.1", "macos", "aarch64"),
        ] {
            let check = check_releases(&releases, current, os, arch).await;
            assert_eq!(check.status, "unavailable");
            assert_eq!(check.latest_version, None);
            assert_eq!(check.release_url, None);
        }
    }
    #[test]
    fn matches_package_format_and_architecture() {
        for (name, os, arch, expected) in [
            (WINDOWS_INSTALLER, "windows", "x86_64", true),
            ("CrewVela_1.2.3_x64-setup.exe", "windows", "x86_64", true),
            ("CrewVela_1.2.3_x64_en-US.msi", "windows", "x86_64", true),
            ("CrewVela_1.2.3_arm64-setup.exe", "windows", "x86_64", false),
            ("CrewVela_x64.exe", "windows", "x86_64", false),
            (MAC_INSTALLER, "macos", "aarch64", true),
            (MAC_INSTALLER, "macos", "x86_64", false),
            ("CrewVela_1.2.3_universal.dmg", "macos", "x86_64", true),
            ("CrewVela_1.2.3_aarch64.AppImage", "linux", "aarch64", true),
            ("CrewVela_1.2.3_amd64.deb", "linux", "x86_64", true),
            ("CrewVela_1.2.3.x86_64.rpm", "linux", "x86_64", true),
            ("CrewVela_1.2.3_x64-setup.exe.sha256", "windows", "x86_64", false),
        ] {
            let asset = ReleaseAsset { name: name.into(), state: "uploaded".into(), size: 123, digest: None };
            assert_eq!(is_compatible_installer(&asset, os, arch), expected, "{name} {os} {arch}");
        }
    }
    #[tokio::test]
    async fn notes_are_bounded_plain_text() {
        let mut value = release("v1.2.3", false, WINDOWS_INSTALLER);
        value["body"] = "中".repeat(7000).into();
        let check = check_releases(&[value], "0.1.0", "windows", "x86_64").await;
        assert_eq!(check.release_notes.chars().count(), 6000);
    }
    fn server(status: &str, headers: &str, body: Vec<u8>) -> (String,std::thread::JoinHandle<()>) {
        server_responses(vec![(status.to_owned(), headers.to_owned(), body)])
    }

    fn server_responses(responses: Vec<(String, String, Vec<u8>)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            for (index, (status, headers, body)) in responses.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                let request = request.to_ascii_lowercase();
                assert!(request.starts_with(&format!("get /?per_page={RELEASE_PAGE_SIZE}&page={} ", index + 1)));
                assert!(request.contains("user-agent: crewvela/"));
                assert!(!request.contains("authorization:"));
                assert!(!request.contains("cookie:"));
                let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n", body.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (url, thread)
    }
    fn client() -> reqwest::Client {reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).redirect(reqwest::redirect::Policy::none()).build().unwrap()}
    async fn check_releases(releases: &[serde_json::Value], current: &str, os: &str, arch: &str) -> AppUpdateCheck {
        let body = serde_json::to_vec(releases).unwrap();
        let (url, thread) = server("200 OK", "", body);
        let result = fetch_update(client(), &url, current, os, arch).await.unwrap();
        thread.join().unwrap();
        result
    }

    #[tokio::test]
    async fn searches_following_pages_and_rejects_malformed_metadata() {
        let first_page = vec![release("v3.0.0", false, MAC_INSTALLER); RELEASE_PAGE_SIZE];
        let second_page = [release("v2.0.0", false, WINDOWS_INSTALLER)];
        let (url, thread) = server_responses(vec![
            ("200 OK".into(), String::new(), serde_json::to_vec(&first_page).unwrap()),
            ("200 OK".into(), String::new(), serde_json::to_vec(&second_page).unwrap()),
        ]);
        let result = fetch_update(client(), &url, "0.1.0", "windows", "x86_64").await.unwrap();
        assert_eq!(result.latest_version.as_deref(), Some("2.0.0"));
        thread.join().unwrap();

        let (url, thread) = server("200 OK", "", b"{}".to_vec());
        assert!(fetch_update(client(), &url, "0.1.0", "windows", "x86_64").await.unwrap_err().contains("格式无效"));
        thread.join().unwrap();
    }
    #[tokio::test]
    async fn http_errors_are_never_current() {
        for status in ["404 Not Found","403 Forbidden","429 Too Many Requests","500 Internal Server Error"] {let(url,thread)=server(status,"",b"private server body".to_vec());let error=fetch_update(client(),&url,"0.1.0","windows","x86_64").await.unwrap_err();assert!(error.contains(&status[..3]));assert!(!error.contains("private server body"));thread.join().unwrap();}
    }
    #[tokio::test]
    async fn oversized_and_redirected_responses_fail_closed() {
        let(url,thread)=server("200 OK","",vec![b'x';MAX_METADATA_BYTES+1]);assert!(fetch_update(client(),&url,"0.1.0","windows","x86_64").await.unwrap_err().contains("过大"));thread.join().unwrap();
        let(url,thread)=server("302 Found","Location: https://untrusted.invalid/\r\n",vec![]);assert!(fetch_update(client(),&url,"0.1.0","windows","x86_64").await.unwrap_err().contains("302"));thread.join().unwrap();
    }
}
