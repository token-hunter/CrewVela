//! Manual application metadata and privacy-directory access. No download or install path.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, time::Duration};
use std::path::Path;

const REPOSITORY: &str = "https://github.com/token-hunter/CrewVela";
const RELEASE_API: &str = "https://api.github.com/repos/token-hunter/CrewVela/releases/latest";
const MAX_METADATA_BYTES: usize = 1024 * 1024;

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
    latest_version: String,
    status: &'static str,
    release_url: String,
    release_notes: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
}

fn parse_release(bytes: &[u8], current: &str) -> Result<AppUpdateCheck, String> {
    if bytes.len() > MAX_METADATA_BYTES { return Err("发行信息过大，未完成检查。".into()); }
    let release: Release = serde_json::from_slice(bytes).map_err(|_| "发行信息格式无效，未完成检查。")?;
    if release.draft || release.prerelease { return Err("发行源未返回公开稳定版，未完成检查。".into()); }
    let latest = Version::parse(release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name))
        .map_err(|_| "发行版本号不符合语义版本规范，未完成检查。")?;
    if !latest.pre.is_empty() { return Err("发行源返回了预发布版本，未完成检查。".into()); }
    let current_version = Version::parse(current).map_err(|_| "当前程序版本无效，无法比较更新。")?;
    let status = match latest.cmp_precedence(&current_version) {
        Ordering::Greater => "available", Ordering::Equal => "current", Ordering::Less => "ahead",
    };
    Ok(AppUpdateCheck {
        current_version: current_version.to_string(), latest_version: latest.to_string(), status,
        // Build this URL from our pinned repository and validated SemVer tag; never open metadata HTML URLs.
        release_url: format!("{REPOSITORY}/releases/tag/{}", release.tag_name),
        release_notes: release.body.unwrap_or_default().chars().take(6000).collect(),
    })
}

fn response_error(status: u16) -> String {
    match status {
        404 => "发布源不可用（HTTP 404）：仓库尚未公开、地址不可访问，或还没有公开稳定发行版。".into(),
        403 | 429 => format!("GitHub 访问受限或已达到请求频率限制（HTTP {status}），请稍后重试。"),
        _ => format!("发行服务返回 HTTP {status}，未完成检查，请稍后重试。"),
    }
}
async fn fetch_release(client: reqwest::Client, endpoint: &str, current: &str) -> Result<AppUpdateCheck, String> {
    let mut response = client.get(endpoint).header(reqwest::header::ACCEPT,"application/vnd.github+json")
        .header(reqwest::header::USER_AGENT,format!("CrewVela/{current}"))
        .send().await.map_err(|error| if error.is_timeout() { "检查更新超时，请检查网络后重试。" } else { "无法连接发行服务，请检查通用设置中的网络代理或稍后重试。" })?;
    if !response.status().is_success() { return Err(response_error(response.status().as_u16())); }
    if response.content_length().is_some_and(|size| size > MAX_METADATA_BYTES as u64) { return Err("发行信息过大，未完成检查。".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取发行信息失败，未完成检查。")? {
        if bytes.len() + chunk.len() > MAX_METADATA_BYTES { return Err("发行信息过大，未完成检查。".into()); }
        bytes.extend_from_slice(&chunk);
    }
    parse_release(&bytes,current)
}

#[tauri::command]
pub async fn app_check_update(app: tauri::AppHandle) -> Result<AppUpdateCheck, String> {
    if crate::test_profile::active() { return Err("隔离测试模式不访问在线发布源。".into()); }
    let client = crate::general_settings::configure_http_client(reqwest::Client::builder())
        .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "无法创建更新客户端，请检查网络设置。")?;
    fetch_release(client,RELEASE_API,&app.package_info().version.to_string()).await
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
    fn release(tag: &str) -> Vec<u8> { serde_json::to_vec(&serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false,"body":"更新说明","html_url":"https://untrusted.invalid/download.exe"})).unwrap() }
    #[test]
    fn version_precedence_and_build_metadata() {
        for (current,latest,status) in [("0.1.0","v0.2.0","available"),("1.2.0","v1.10.0","available"),("1.2.3+local","v1.2.3+release","current"),("2.0.0","v1.9.9","ahead"),("1.2.3-beta.1","v1.2.3","available")] {
            assert_eq!(parse_release(&release(latest),current).unwrap().status,status);
        }
    }
    #[test]
    fn metadata_never_controls_download_origin() {
        let check=parse_release(&release("v1.2.3"),"0.1.0").unwrap();
        assert_eq!(check.release_url,"https://github.com/token-hunter/CrewVela/releases/tag/v1.2.3");
    }
    #[test]
    fn rejects_invalid_and_nonstable_releases() {
        for tag in ["../bad","v1","1.02.3","v1.2.3-rc.1","v1.2.3/evil"] {assert!(parse_release(&release(tag),"0.1.0").is_err());}
        for flag in ["draft","prerelease"] {let mut value:serde_json::Value=serde_json::from_slice(&release("v1.2.3")).unwrap();value[flag]=true.into();assert!(parse_release(&serde_json::to_vec(&value).unwrap(),"0.1.0").is_err());}
        for bytes in [b"invalid".to_vec(),b"{}".to_vec(),vec![0;MAX_METADATA_BYTES+1]] {assert!(parse_release(&bytes,"0.1.0").is_err());}
        assert!(parse_release(&release("v1.2.3"),"invalid").is_err());
    }
    #[test]
    fn notes_are_bounded_plain_text() {
        let mut value:serde_json::Value=serde_json::from_slice(&release("v1.2.3")).unwrap();value["body"]="中".repeat(7000).into();
        assert_eq!(parse_release(&serde_json::to_vec(&value).unwrap(),"0.1.0").unwrap().release_notes.chars().count(),6000);
    }
    fn server(status: &str, headers: &str, body: Vec<u8>) -> (String,std::thread::JoinHandle<()>) {
        use std::io::{Read,Write};
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let url=format!("http://{}",listener.local_addr().unwrap());
        let response=format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",body.len());
        let thread=std::thread::spawn(move||{let(mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();let mut request=[0;4096];let n=stream.read(&mut request).unwrap();let text=String::from_utf8_lossy(&request[..n]).to_ascii_lowercase();assert!(text.starts_with("get /"));assert!(text.contains("user-agent: crewvela/"));assert!(!text.contains("authorization:"));assert!(!text.contains("cookie:"));let _=stream.write_all(response.as_bytes());let _=stream.write_all(&body);});
        (url,thread)
    }
    fn client() -> reqwest::Client {reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).redirect(reqwest::redirect::Policy::none()).build().unwrap()}
    #[tokio::test]
    async fn successful_http_metadata_check() {
        let(url,thread)=server("200 OK","",release("v0.2.0"));let check=fetch_release(client(),&url,"0.1.0").await.unwrap();assert_eq!(check.status,"available");thread.join().unwrap();
    }
    #[tokio::test]
    async fn http_errors_are_never_current() {
        for status in ["404 Not Found","403 Forbidden","429 Too Many Requests","500 Internal Server Error"] {let(url,thread)=server(status,"",b"private server body".to_vec());let error=fetch_release(client(),&url,"0.1.0").await.unwrap_err();assert!(error.contains(&status[..3]));assert!(!error.contains("private server body"));thread.join().unwrap();}
    }
    #[tokio::test]
    async fn oversized_and_redirected_responses_fail_closed() {
        let(url,thread)=server("200 OK","",vec![b'x';MAX_METADATA_BYTES+1]);assert!(fetch_release(client(),&url,"0.1.0").await.unwrap_err().contains("过大"));thread.join().unwrap();
        let(url,thread)=server("302 Found","Location: https://untrusted.invalid/\r\n",vec![]);assert!(fetch_release(client(),&url,"0.1.0").await.unwrap_err().contains("302"));thread.join().unwrap();
    }
}
