use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::Url;

use super::executor::{external_command_path, git_head, git_repository_layout, valid_commit_hash};
use super::managed_delivery::{git, ManagedDelivery};
use super::MultiAgentStore;

pub(super) const MANIFEST: &str = "crewvela.launch.json";
pub(super) const MANIFEST_INSTRUCTIONS: &str = r#"网页成品交付必须包含已实测的 crewvela.launch.json。请在独立工作树中启动，验证页面及核心操作，在项目根目录提交启动配置。Demo 会从干净的交付提交启动，启动步骤不能依赖本轮 Agent 留下的进程或未提交文件。JSON 示例 {"program":"node","args":["server.js"],"workingDirectory":".","url":null}。服务器须仅监听回环地址。优先支持 PORT=0 并在标准输出单独打印完整的本机 HTTP 地址；否则在 url 填写固定本地地址。核心操作的验收命令仍单独登记。"#;
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const START_TIMEOUT: Duration = Duration::from_secs(30);
const LOG_LIMIT: usize = 16_384;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(super) enum DeliveryDemo {
    Preparing,
    Ready,
    Unavailable { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewConfig {
    pub(super) program: String,
    #[serde(default)]
    pub(super) args: Vec<String>,
    pub(super) working_directory: String,
    pub(super) url: Option<String>,
}

impl PreviewConfig {
    pub(super) fn from_delivery(root: &Path, commit: &str) -> Result<Option<Self>> {
        let entry = git(root, &["ls-tree", commit, "--", MANIFEST])?;
        if entry.is_empty() {
            return Ok(None);
        }
        // Resolve the tree entry first. Git for Windows tries to stat a `sha:./path`
        // argument in the current directory and can reject a valid long worktree.
        let (metadata, name) = entry.split_once('\t').with_context(|| format!("无法辨认提交中的 {MANIFEST}"))?;
        let mut fields = metadata.split_whitespace();
        let (Some("100644" | "100755"), Some("blob"), Some(object), None) =
            (fields.next(), fields.next(), fields.next(), fields.next()) else {
            anyhow::bail!("提交中的 {MANIFEST} 必须是普通文件");
        };
        ensure!(name == MANIFEST && valid_commit_hash(object), "提交中的 {MANIFEST} 缺少有效对象");
        let size = git(root, &["cat-file", "-s", object])?.parse::<usize>()?;
        ensure!(size <= 8192, "{MANIFEST} 超过 8 KB");
        let source = git(root, &["cat-file", "blob", object])?;
        let config: Self = serde_json::from_str(&source).with_context(|| format!("无法读取 {MANIFEST}"))?;
        config.validate(root)?;
        Ok(Some(config))
    }

    pub(crate) fn validate(&self, root: &Path) -> Result<PathBuf> {
        ensure!(
            !self.program.trim().is_empty()
                && self.program.len() <= 512
                && !self.program.chars().any(char::is_control),
            "请填写启动程序，例如 node"
        );
        ensure!(
            self.args.len() <= 40
                && self
                    .args
                    .iter()
                    .all(|arg| arg.len() <= 1024 && !arg.chars().any(char::is_control)),
            "启动参数过多、过长或包含控制字符"
        );
        let relative = Path::new(&self.working_directory);
        ensure!(
            !self.working_directory.is_empty()
                && relative.components().all(|part| match part {
                    Component::CurDir => true,
                    Component::Normal(name) => !name.to_string_lossy().eq_ignore_ascii_case(".git"),
                    _ => false,
                }),
            "工作目录应为项目内的相对目录"
        );
        let root = root.canonicalize().context("项目目录不可访问")?;
        let cwd = root
            .join(relative)
            .canonicalize()
            .context("启动工作目录不存在")?;
        ensure!(
            cwd.is_dir() && cwd.starts_with(&root),
            "工作目录应位于项目目录内"
        );
        if let Some(url) = &self.url {
            local_url(url)?;
        }
        Ok(cwd)
    }

    fn executable(&self, cwd: &Path) -> Result<PathBuf> {
        let path = Path::new(self.program.trim());
        let executable = if path.is_absolute() {
            path.to_path_buf()
        } else if path.components().count() > 1 {
            cwd.join(path)
        } else {
            which::which(path).with_context(|| format!("找不到启动程序 {}", self.program))?
        };
        ensure!(executable.is_file(), "找不到启动程序 {}", self.program);
        Ok(external_command_path(&executable))
    }
}

fn local_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).context("请输入完整的本地 HTTP 地址")?;
    ensure!(
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.port() != Some(0),
        "启动地址应为本机 HTTP 地址，例如 http://127.0.0.1:3000/"
    );
    Ok(url)
}

fn local_addresses(url: &Url) -> Vec<SocketAddr> {
    let port = url.port_or_known_default().unwrap_or(80);
    match url.host_str() {
        Some("[::1]") => vec![SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            port,
        )],
        Some("localhost") => vec![
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), port),
        ],
        _ => vec![SocketAddr::from(([127, 0, 0, 1], port))],
    }
}

fn readiness(url: &Url) -> Option<u16> {
    let mut stream = local_addresses(url)
        .into_iter()
        .find_map(|addr| TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok())?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
        url.host_str()?,
        url.port_or_known_default()?
    )
    .ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut bytes = [0; 512];
    let mut len = 0;
    let line = loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        stream.set_read_timeout(Some(remaining)).ok()?;
        let read = stream.read(&mut bytes[len..]).ok()?;
        if read == 0 {
            return None;
        }
        len += read;
        if let Some(end) = bytes[..len].iter().position(|byte| *byte == b'\n') {
            break std::str::from_utf8(&bytes[..end]).ok()?;
        }
    };
    let mut parts = line.split_whitespace();
    parts.next()?.starts_with("HTTP/").then_some(())?;
    parts.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn wait_until(previews: &ProjectPreviews, project: &str, expected: &str) -> PreviewStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = previews.status(project, "plan", true);
            if status.state == expected {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected}, got {}: {:?}\n{:?}",
                status.state,
                status.error,
                status.logs
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn project_preview_rejects_external_urls_and_directories() {
        assert!(local_url("https://127.0.0.1:3000").is_err());
        assert!(local_url("http://example.com:3000").is_err());
        assert!(local_url("http://localhost:0/").is_err());
        assert!(local_url("http://localhost:4567/app?lang=zh").is_ok());
        let config = PreviewConfig {
            program: "node".into(),
            args: vec![],
            working_directory: "..".into(),
            url: None,
        };
        assert!(config.validate(Path::new(".")).is_err());
    }

    #[test]
    fn local_project_services_run_and_stop_independently() {
        if which::which("node").is_err() { return; }
        let root = std::env::temp_dir().join(format!("crewvela-local-services-{}", uuid::Uuid::new_v4()));
        let script = r#"const response = require('node:fs').readFileSync('marker', 'utf8'); const s = require('node:http').createServer((q,r) => r.end(response)); s.listen(0, '127.0.0.1', () => console.log('http://127.0.0.1:' + s.address().port));"#;
        let registered = |id: &str, cwd: &str| {
            let dir = root.join(id);
            std::fs::create_dir_all(dir.join(cwd)).unwrap();
            std::fs::write(dir.join(cwd).join("marker"), id).unwrap();
            crate::projects::Project {
                id: id.into(), name: id.into(), path: dir.display().to_string(), badge_color: String::new(),
                created_at: 0, updated_at: 0, archived_at: None, last_opened_at: None,
                launch: Some(PreviewConfig { program: "node".into(), args: vec!["-e".into(), script.into()],
                    working_directory: cwd.into(), url: None }),
                collaboration_project_ids: Vec::new(),
            }
        };
        let first = registered("converter", "apps/web");
        let second = registered("media", "server");
        let services = LocalProjectServices::default();
        {
            let mut values = services.instances.lock().unwrap();
            LocalProjectServices::start_locked(&mut values, &first, false).unwrap();
            LocalProjectServices::start_locked(&mut values, &second, false).unwrap();
        }
        let await_url = |id: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = services.status(id);
                if let Some(url) = status.url { break url; }
                assert!(Instant::now() < deadline, "{id}: {:?} {:?}", status.error, status.logs);
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        let first_url = await_url(&first.id);
        let second_url = await_url(&second.id);
        assert_ne!(first_url, second_url);
        assert_eq!(services.stop(&first.id).state, "idle");
        assert_eq!(readiness(&local_url(&first_url).unwrap()), None);
        assert_eq!(readiness(&local_url(&second_url).unwrap()), Some(200));
        services.stop_all().unwrap();
        assert_eq!(readiness(&local_url(&second_url).unwrap()), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn delivery_manifest_is_read_from_its_blob_in_a_long_windows_worktree() {
        if which::which("git").is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("cv-preview-{}", uuid::Uuid::new_v4()));
        let directory = root.join("w".repeat(207_usize.saturating_sub(root.as_os_str().len()).max(16)));
        std::fs::create_dir_all(&directory).unwrap();
        git(&directory, &["init", "-b", "main"]).unwrap();
        git(&directory, &["config", "core.longpaths", "true"]).unwrap();
        let manifest = directory.join(MANIFEST);
        std::fs::write(&manifest, r#"{"program":"node","args":["server.js"],"workingDirectory":"."}"#).unwrap();
        GitFixture::commit(&directory, "delivery with manifest");
        let hash = git_head(&directory).unwrap();
        std::fs::write(&manifest, "uncommitted working tree contents").unwrap();
        let config = PreviewConfig::from_delivery(&directory, &hash).unwrap().unwrap();
        assert_eq!(config.args, ["server.js"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_preview_starts_reuses_stops_and_checks_delivery_version() {
        if which::which("node").is_err() || which::which("git").is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("crewvela-preview-{}", uuid::Uuid::new_v4()));
        let project_path = root.join("app");
        std::fs::create_dir_all(&project_path).unwrap();
        let converter = std::env::var_os("CREWVELA_PREVIEW_TEST_CONVERTER").map(PathBuf::from);
        if let Some(converter) = &converter {
            std::fs::create_dir_all(project_path.join("public")).unwrap();
            std::fs::copy(converter.join("server.js"), project_path.join("server.js")).unwrap();
            std::fs::copy(
                converter.join("public/index.html"),
                project_path.join("public/index.html"),
            )
            .unwrap();
        } else {
            std::fs::write(
                project_path.join("server.js"),
                r#"
                const server = require('node:http').createServer((_req, res) => res.end('preview'));
                server.listen(Number(process.env.PORT), '127.0.0.1', () =>
                  console.log('http://127.0.0.1:' + server.address().port));
            "#,
            )
            .unwrap();
        }
        git(&project_path, &["init", "-b", "main"]).unwrap();
        GitFixture::commit(&project_path, "initial delivery");
        let hash = git_head(&project_path).unwrap();
        let store = MultiAgentStore::open_at(root.join("test.db")).unwrap();
        let project = store
            .create_project(super::super::CreateProjectInput {
                name: "Preview fixture".into(),
                goal: "Start a local HTTP service".into(),
                root_path: project_path.display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap();
        let config = PreviewConfig {
            program: "node".into(),
            args: vec!["server.js".into()],
            working_directory: ".".into(),
            url: None,
        };
        let target = super::super::managed_delivery::ManagedTarget {
            root: project.root_path.clone(),
            branch: "main".into(),
            base: hash.clone(),
            planner: "test".into(),
            mode: Default::default(),
            integration_owner: None,
            members: Default::default(),
        };
        let mut delivery = ManagedDelivery::new(target);
        delivery.status = "completed".into();
        delivery.commit = Some(hash.clone());
        let plan = super::super::ConversationExecutionPlan {
            id: "plan".into(),
            project_id: project.id.clone(),
            round_id: "round".into(),
            message_id: "message".into(),
            summary: String::new(),
            tasks: vec![],
            final_task_key: None,
            status: "applied".into(),
            task_ids: vec![],
            created_at: 1,
            managed: Some(delivery),
        };
        store.connection.lock().unwrap().execute("INSERT INTO ma_conversation_plans(id,project_id,round_id,data_json) VALUES(?1,?2,?3,?4)",
            params![&plan.id, &project.id, &plan.round_id, serde_json::to_string(&plan).unwrap()]).unwrap();
        let previews = ProjectPreviews::default();
        assert!(previews.start(&store, &project.id, "plan", None).is_err());
        store
            .save_project_preview(&project.id, "plan", config.clone())
            .unwrap();
        assert!(store
            .preview_delivery(&project.id, "plan", None)
            .unwrap()
            .preview
            .is_some());
        previews.start(&store, &project.id, "plan", None).unwrap();
        let status = wait_until(&previews, &project.id, "running");
        let url = status.url.unwrap();
        assert!(status.logs.unwrap().contains(url.trim_end_matches('/')));
        assert_eq!(readiness(&local_url(&url).unwrap()), Some(200));
        if converter.is_some() {
            let api = format!("{url}api/convert?value=25&from=celsius&to=fahrenheit");
            assert_eq!(readiness(&local_url(&api).unwrap()), Some(200));
            eprintln!("单位换算器由交付启动服务启动，首页与换算接口返回 200");
        }
        let PreviewStart::Status { status: repeated } =
            previews.start(&store, &project.id, "plan", None).unwrap()
        else {
            panic!("expected existing process")
        };
        assert_eq!(repeated.url.as_deref(), Some(url.as_str()));
        assert_eq!(previews.stop(&project.id, "plan").state, "idle");
        assert!(readiness(&local_url(&url).unwrap()).is_none());

        std::fs::write(
            project_path.join(MANIFEST),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        GitFixture::commit(&project_path, "delivery with launch manifest");
        let new_hash = git_head(&project_path).unwrap();
        let recorded = PreviewConfig::from_delivery(&project_path, &new_hash).unwrap().unwrap();
        assert_eq!(recorded.args, ["server.js"]);
        assert!(PreviewConfig::from_delivery(&project_path, &hash).unwrap().is_none());
        let PreviewStart::Confirm { version, .. } =
            previews.start(&store, &project.id, "plan", None).unwrap()
        else {
            panic!("expected version confirmation")
        };
        previews
            .start(&store, &project.id, "plan", Some(version.clone()))
            .unwrap();
        wait_until(&previews, &project.id, "running");
        assert_eq!(previews.stop(&project.id, "plan").state, "idle");
        let mut failing = config;
        failing.args = vec![
            "-e".into(),
            "console.error('preview test failure');process.exit(7)".into(),
        ];
        store
            .save_project_preview(&project.id, "plan", failing)
            .unwrap();
        previews
            .start(&store, &project.id, "plan", Some(version))
            .unwrap();
        let failed = wait_until(&previews, &project.id, "failed");
        assert!(failed.error.unwrap().contains('7'));
        assert!(wait_until(&previews, &project.id, "failed")
            .logs
            .unwrap()
            .contains("preview test failure"));
        let nested = project_path.join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("index.html"), "nested demo").unwrap();
        GitFixture::commit(&project_path, "nested project");
        let checkout = PreviewCheckout::create(&nested, &git_head(&nested).unwrap()).unwrap();
        assert_eq!(checkout.project_path, checkout.path.join("nested"));
        assert_eq!(std::fs::read_to_string(checkout.project_path.join("index.html")).unwrap(), "nested demo");
        drop(checkout);
        drop(store);
        drop(previews);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_manifest_starts_local_project_twice_without_changing_the_delivered_demo() {
        if which::which("node").is_err() || which::which("git").is_err() { return; }
        let root = std::env::temp_dir().join(format!("crewvela-local-manifest-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        std::fs::create_dir_all(checkout.join("public")).unwrap();
        let sample = std::env::var_os("CREWVELA_PREVIEW_TEST_CONVERTER").map(PathBuf::from);
        if let Some(sample) = &sample {
            for file in ["server.js", "public/index.html", MANIFEST] {
                std::fs::copy(sample.join(file), checkout.join(file)).unwrap();
            }
        } else {
            std::fs::write(checkout.join(MANIFEST), r#"{"program":"node","args":["server.js"],"workingDirectory":".","url":null}"#).unwrap();
            std::fs::write(checkout.join("public/index.html"), "delivery version").unwrap();
            std::fs::write(checkout.join("server.js"), r#"
                const http = require('node:http'), fs = require('node:fs');
                const server = http.createServer((q,r) => r.end(fs.readFileSync('public/index.html')));
                server.listen(Number(process.env.PORT), '127.0.0.1', () => console.log('http://127.0.0.1:' + server.address().port));
            "#).unwrap();
        }
        git(&checkout, &["init", "-b", "main"]).unwrap();
        GitFixture::commit(&checkout, "Agent registers launch configuration");
        let commit = git_head(&checkout).unwrap();
        let stored_config = PreviewConfig::from_delivery(&checkout, &commit).unwrap().unwrap();
        let marker = "CREWVELA_CURRENT_CHECKOUT_ONLY";
        let html = checkout.join("public/index.html");
        let current = std::fs::read_to_string(&html).unwrap() + marker;
        std::fs::write(&html, current).unwrap();
        let store = MultiAgentStore::open_at(root.join("team.db")).unwrap();
        let team = store.create_project(super::super::CreateProjectInput { name: "Converter team".into(), goal: "deliver".into(),
            root_path: checkout.display().to_string(), default_branch: "main".into() }).unwrap();
        let mut delivery = ManagedDelivery::new(super::super::managed_delivery::ManagedTarget {
            root: team.root_path.clone(), branch: "main".into(), base: commit.clone(),
            planner: "fixture".into(), mode: Default::default(), integration_owner: None, members: Default::default(),
        });
        delivery.status = "completed".into();
        delivery.commit = Some(commit);
        delivery.preview = Some(stored_config.clone());
        delivery.demo = Some(DeliveryDemo::Preparing);
        let plan = super::super::ConversationExecutionPlan {
            id: "plan".into(), project_id: team.id.clone(), round_id: "round".into(), message_id: "message".into(),
            summary: String::new(), tasks: vec![], final_task_key: None, status: "applied".into(), task_ids: vec![], created_at: 1, managed: Some(delivery),
        };
        store.connection.lock().unwrap().execute("INSERT INTO ma_conversation_plans(id,project_id,round_id,data_json) VALUES(?1,?2,?3,?4)",
            params![plan.id, team.id, plan.round_id, serde_json::to_string(&plan).unwrap()]).unwrap();
        let previews = ProjectPreviews::default();
        previews.start(&store, &team.id, &plan.id, None).unwrap();
        let demo_url = wait_until(&previews, &team.id, "running").url.unwrap();
        let projects_file = root.join("projects.json");
        std::fs::write(&projects_file, serde_json::to_vec(&serde_json::json!({"version": 1, "projects": [{
            "id": "converter", "name": "单位换算器", "path": checkout, "badgeColor": "", "createdAt": 0, "updatedAt": 0
        }]})).unwrap()).unwrap();
        let get_page = |url: &str| {
            let url = local_url(url).unwrap();
            let mut conn = TcpStream::connect(local_addresses(&url)[0]).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let path = url.query().map_or_else(|| url.path().to_owned(), |query| format!("{}?{query}", url.path()));
            write!(conn, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
            let mut page = String::new();
            conn.read_to_string(&mut page).unwrap();
            page
        };
        let local = LocalProjectServices::default();
        let mut previous_save = None;
        for _ in 0..2 {
            local.start_at(&store, &projects_file, "converter", false).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let local_url = loop {
                let status = local.status("converter");
                if status.state == "running" { break status.url.unwrap(); }
                assert!(Instant::now() < deadline, "{:?} {:?}", status.error, status.logs);
                std::thread::sleep(Duration::from_millis(20));
            };
            let registered = crate::projects::get_at(&projects_file, "converter").unwrap().unwrap();
            assert_eq!(registered.launch.as_ref(), Some(&stored_config));
            if let Some(previous) = previous_save { assert_eq!(previous, registered.updated_at); }
            previous_save = Some(registered.updated_at);
            assert!(get_page(&local_url).contains(marker));
            assert!(!get_page(&demo_url).contains(marker));
            if sample.is_some() {
                let api = format!("{local_url}api/convert?value=25&from=celsius&to=fahrenheit");
                assert!(get_page(&api).contains(r#"{"ok":true,"result":77}"#));
            }
            assert_eq!(local.stop("converter").state, "idle");
            assert!(readiness(&super::local_url(&local_url).unwrap()).is_none());
            assert_eq!(readiness(&super::local_url(&demo_url).unwrap()), Some(200));
        }
        assert_eq!(previews.stop(&team.id, &plan.id).state, "idle");
        let restart = previews.start(&store, &team.id, &plan.id, None).unwrap();
        assert!(matches!(restart, PreviewStart::Status { .. }));
        let restarted_demo = wait_until(&previews, &team.id, "running").url.unwrap();
        assert!(!get_page(&restarted_demo).contains(marker));
        previews.stop(&team.id, &plan.id);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    struct GitFixture;
    impl GitFixture {
        fn commit(project: &Path, message: &str) {
            git(project, &["add", "."]).unwrap();
            git(
                project,
                &[
                    "-c",
                    "user.name=Preview test",
                    "-c",
                    "user.email=preview@example.invalid",
                    "commit",
                    "-m",
                    message,
                ],
            )
            .unwrap();
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStatus {
    state: &'static str,
    url: Option<String>,
    error: Option<String>,
    logs: Option<String>,
}

impl Default for PreviewStatus {
    fn default() -> Self {
        Self {
            state: "idle",
            url: None,
            error: None,
            logs: None,
        }
    }
}

#[derive(Default)]
struct Output {
    tail: VecDeque<u8>,
    discovered_url: Option<Url>,
}

fn capture(reader: impl Read + Send + 'static, output: Arc<Mutex<Output>>, discover_url: bool) {
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0; 2048];
        let mut pending = Vec::new();
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let mut output = output.lock().unwrap();
            output.tail.extend(&buffer[..n]);
            let extra = output.tail.len().saturating_sub(LOG_LIMIT);
            output.tail.drain(..extra);
            if discover_url && output.discovered_url.is_none() {
                pending.extend_from_slice(&buffer[..n]);
                let end = pending
                    .iter()
                    .rposition(|byte| byte.is_ascii_whitespace())
                    .map_or(0, |pos| pos + 1);
                output.discovered_url = String::from_utf8_lossy(&pending[..end])
                    .split_whitespace()
                    .filter_map(|word| local_url(word).ok())
                    .find(|url| url.port().is_some());
                pending.drain(..end);
                if pending.len() > 2048 {
                    pending.clear();
                }
            }
        }
    });
}

struct PreviewProcess {
    child: Child,
    _ownership: crate::acp::ProcessIdentityGuard,
    _checkout: Option<PreviewCheckout>,
}

struct PreviewCheckout {
    root: PathBuf,
    path: PathBuf,
    project_path: PathBuf,
}

impl PreviewCheckout {
    fn create(root: &Path, commit: &str) -> Result<Self> {
        ensure!(valid_commit_hash(commit), "交付提交无效");
        let (root, project_relative) = git_repository_layout(root)?;
        let path = std::env::temp_dir().join(format!("crewvela-demo-{}", uuid::Uuid::new_v4()));
        git(&root, &["worktree", "add", "--detach", "--quiet", &path.to_string_lossy(), commit])
            .context("无法检出 Demo 的交付版本")?;
        Ok(Self { root, project_path: path.join(project_relative), path })
    }
}

impl Drop for PreviewCheckout {
    fn drop(&mut self) {
        if let Err(error) = git(&self.root, &["worktree", "remove", "--force", &self.path.to_string_lossy()]) {
            eprintln!("[crewvela] 清理 Demo 工作树失败：{error:#}");
        }
    }
}

struct Instance {
    process: Option<PreviewProcess>,
    output: Arc<Mutex<Output>>,
    fixed_url: Option<Url>,
    status: PreviewStatus,
    started: Instant,
    last_http_status: Option<u16>,
    local: bool,
    dynamic_port: bool,
}

impl Instance {
    fn launch(config: &PreviewConfig, root: &Path, checkout: Option<PreviewCheckout>, local: bool, dynamic_port: bool) -> Result<Self> {
        let cwd = config.validate(checkout.as_ref().map_or(root, |checkout| checkout.project_path.as_path()))?;
        let fixed_url = config.url.as_deref().map(local_url).transpose()?;
        if let Some(url) = &fixed_url {
            ensure!(
                !local_addresses(url).into_iter()
                    .any(|addr| TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()),
                "启动端口已被占用，请停止原服务或修改启动配置"
            );
        }
        let executable = config.executable(&cwd)?;
        let mut command = super::command_for_path(&executable);
        command.args(&config.args).current_dir(external_command_path(&cwd))
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if fixed_url.is_none() && dynamic_port {
            command.env("PORT", "0");
        }
        let (mut child, ownership) =
            crate::acp::spawn_owned_command(&mut command).context("启动项目失败")?;
        let output = Arc::new(Mutex::new(Output::default()));
        capture(child.stdout.take().expect("piped stdout"), output.clone(), fixed_url.is_none());
        capture(child.stderr.take().expect("piped stderr"), output.clone(), false);
        Ok(Self {
            process: Some(PreviewProcess { child, _ownership: ownership, _checkout: checkout }),
            output, fixed_url,
            status: PreviewStatus { state: "starting", ..Default::default() },
            started: Instant::now(), last_http_status: None, local, dynamic_port,
        })
    }

    fn status(&self, logs: bool) -> PreviewStatus {
        let mut status = self.status.clone();
        if logs {
            let bytes: Vec<_> = self.output.lock().unwrap().tail.iter().copied().collect();
            status.logs = Some(String::from_utf8_lossy(&bytes).into_owned());
        }
        status
    }

    fn finish(&mut self, error: Option<String>) {
        if let Some(process) = &mut self.process {
            if let Err(reason) = crate::acp::stop_child_checked(&mut process.child) {
                let previous = error.as_deref().unwrap_or("停止项目失败");
                self.status = PreviewStatus {
                    state: "stopFailed",
                    error: Some(format!("{previous}。{reason:#}")),
                    ..Default::default()
                };
                return;
            }
        }
        self.process = None;
        self.status = PreviewStatus {
            state: if error.is_some() { "failed" } else { "idle" },
            error,
            ..Default::default()
        };
    }

    fn poll(&mut self) -> bool {
        let Some(process) = self.process.as_mut() else {
            return false;
        };
        if self.status.state == "stopFailed" {
            return false;
        }
        match process.child.try_wait() {
            Ok(Some(exit)) if self.local && !self.dynamic_port && self.fixed_url.is_none() && exit.success() => {
                self.finish(None);
                if self.status.state == "idle" { self.status.state = "completed"; }
                return false;
            }
            Ok(Some(exit)) => {
                self.finish(Some(format!("项目进程已退出（{exit}）")));
                return false;
            }
            Err(error) => {
                self.finish(Some(format!("无法检查项目进程：{error}")));
                return false;
            }
            Ok(None) => {}
        }
        if self.local && self.fixed_url.is_none() && !self.dynamic_port {
            self.status.state = "running";
            if self.status.url.is_none() {
                let url = self.output.lock().unwrap().discovered_url.clone();
                if let Some(url) = url.filter(|url| readiness(url).is_some_and(|code| (200..400).contains(&code))) {
                    self.status.url = Some(url.into());
                }
            }
            return true;
        }
        if self.status.state == "running" {
            return true;
        }
        let url = self
            .fixed_url
            .clone()
            .or_else(|| self.output.lock().unwrap().discovered_url.clone());
        if let Some(url) = url {
            self.last_http_status = readiness(&url).or(self.last_http_status);
            if self
                .last_http_status
                .is_some_and(|code| (200..if self.local { 400 } else { 300 }).contains(&code))
            {
                self.status = PreviewStatus {
                    state: "running",
                    url: Some(url.into()),
                    ..Default::default()
                };
                return true;
            }
        }
        if self.started.elapsed() >= START_TIMEOUT {
            let detail = self
                .last_http_status
                .map_or("未收到成功的 HTTP 响应".into(), |code| {
                    format!("页面返回 HTTP {code}")
                });
            self.finish(Some(format!("启动超时：{detail}。")));
            return false;
        }
        true
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(process) = &mut self.process {
            let _ = crate::acp::stop_child_checked(&mut process.child);
        }
    }
}

type PreviewInstances = HashMap<(String, String), Arc<Mutex<Instance>>>;

#[derive(Clone, Default)]
pub(crate) struct ProjectPreviews(Arc<Mutex<PreviewInstances>>);

#[derive(Clone, Default)]
pub(crate) struct LocalProjectServices {
    instances: Arc<Mutex<HashMap<String, Arc<Mutex<Instance>>>>>,
    preparing: Arc<Mutex<HashSet<String>>>,
}

struct PreparingProject<'a>(&'a Mutex<HashSet<String>>, &'a str);
impl Drop for PreparingProject<'_> {
    fn drop(&mut self) { self.0.lock().unwrap().remove(self.1); }
}

impl LocalProjectServices {
    pub(crate) fn start(&self, store: &MultiAgentStore, id: &str, recheck: bool) -> Result<PreviewStatus> {
        self.start_at(store, &crate::projects::projects_path()?, id, recheck)
    }

    fn start_at(&self, store: &MultiAgentStore, projects_file: &Path, id: &str, recheck: bool) -> Result<PreviewStatus> {
        let reservation = {
            let mut preparing = self.preparing.lock().unwrap();
            let instances = self.instances.lock().unwrap();
            if let Some(entry) = instances.get(id) {
                let entry = entry.lock().unwrap();
                if entry.process.is_some() { return Ok(entry.status(false)); }
            }
            ensure!(preparing.insert(id.into()), "正在识别本项目的启动方式");
            PreparingProject(&self.preparing, id)
        };
        let mut project = crate::projects::get_at(projects_file, id)?.context("找不到本地项目")?;
        ensure!(project.archived_at.is_none(), "请先恢复已归档的项目");
        let (config, from_manifest) = super::project_launch::resolve(&project, recheck, store)?;
        let mut instances = self.instances.lock().unwrap();
        if project.launch.as_ref() != Some(&config) {
            project = crate::projects::edit_at(projects_file, id, crate::projects::ProjectEdit::Launch { config: Some(config) })?;
        }
        let result = Self::start_locked(&mut instances, &project, from_manifest);
        drop(instances);
        drop(reservation);
        result
    }

    fn start_locked(instances: &mut HashMap<String, Arc<Mutex<Instance>>>, project: &crate::projects::Project, dynamic_port: bool) -> Result<PreviewStatus> {
        ensure!(project.archived_at.is_none(), "请先恢复已归档的项目");
        if let Some(entry) = instances.get(&project.id) {
            let entry = entry.lock().unwrap();
            if entry.process.is_some() { return Ok(entry.status(false)); }
        }
        let config = project.launch.as_ref().context("请先保存启动配置")?;
        let entry = Arc::new(Mutex::new(Instance::launch(config, Path::new(&project.path), None, true, dynamic_port)?));
        let status = entry.lock().unwrap().status(false);
        let observer = Arc::downgrade(&entry);
        instances.insert(project.id.clone(), entry);
        std::thread::spawn(move || {
            while let Some(entry) = observer.upgrade() {
                if !entry.lock().unwrap().poll() { break; }
                drop(entry);
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        Ok(status)
    }

    pub(crate) fn statuses(&self) -> HashMap<String, PreviewStatus> {
        self.instances.lock().unwrap().iter()
            .map(|(id, entry)| (id.clone(), entry.lock().unwrap().status(false)))
            .collect()
    }

    #[cfg(windows)]
    pub(crate) fn has_active_services(&self) -> bool {
        self.instances.lock().unwrap().values()
            .any(|entry| matches!(entry.lock().unwrap().status.state, "starting" | "running"))
    }

    pub(crate) fn status(&self, id: &str) -> PreviewStatus {
        self.instances.lock().unwrap().get(id)
            .map_or_else(PreviewStatus::default, |entry| entry.lock().unwrap().status(true))
    }

    pub(crate) fn stop(&self, id: &str) -> PreviewStatus {
        self.instances.lock().unwrap().get(id).map_or_else(PreviewStatus::default, |entry| {
            let mut entry = entry.lock().unwrap();
            entry.finish(None);
            entry.status(false)
        })
    }

    pub(crate) fn edit(&self, id: &str, edit: crate::projects::ProjectEdit) -> Result<crate::projects::Project> {
        use crate::projects::ProjectEdit;
        let affects_launch = matches!(edit, ProjectEdit::Directory { .. } | ProjectEdit::Archive { .. } | ProjectEdit::Launch { .. });
        let preparing = self.preparing.lock().unwrap();
        ensure!(!preparing.contains(id) || matches!(edit, ProjectEdit::Opened), "请等待本项目完成启动识别");
        let mut instances = self.instances.lock().unwrap();
        if affects_launch {
            ensure!(instances.get(id).is_none_or(|entry| entry.lock().unwrap().process.is_none()),
                "请先停止本项目的开发服务");
        }
        let result = crate::projects::edit(id, edit)?;
        if affects_launch { instances.remove(id); }
        Ok(result)
    }

    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        let preparing = self.preparing.lock().unwrap();
        ensure!(!preparing.contains(id), "请等待本项目完成启动识别");
        let mut instances = self.instances.lock().unwrap();
        ensure!(instances.get(id).is_none_or(|entry| entry.lock().unwrap().process.is_none()),
            "请先停止本项目的开发服务");
        crate::projects::delete(id)?;
        instances.remove(id);
        Ok(())
    }

    pub(crate) fn stop_all(&self) -> Result<()> {
        for (id, entry) in self.instances.lock().unwrap().iter() {
            let mut entry = entry.lock().unwrap();
            if entry.process.is_none() { continue; }
            entry.finish(None);
            ensure!(entry.process.is_none(), "项目 {id}：{}", entry.status.error.as_deref().unwrap_or("停止开发服务失败"));
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum PreviewStart {
    Status { status: PreviewStatus },
    Confirm { version: String, message: String },
}

impl ProjectPreviews {
    pub(crate) fn status(&self, project: &str, plan: &str, logs: bool) -> PreviewStatus {
        let entry = self.0.lock().unwrap().get(&(project.into(), plan.into())).cloned();
        entry.map_or_else(PreviewStatus::default, |entry| {
            entry.lock().unwrap().status(logs)
        })
    }

    pub(crate) fn stop(&self, project: &str, plan: &str) -> PreviewStatus {
        let entry = self.0.lock().unwrap().get(&(project.into(), plan.into())).cloned();
        entry.map_or_else(PreviewStatus::default, |entry| {
            let mut entry = entry.lock().unwrap();
            entry.finish(None);
            entry.status(false)
        })
    }

    pub(crate) fn start(
        &self,
        store: &MultiAgentStore,
        project: &str,
        plan: &str,
        confirmed: Option<String>,
    ) -> Result<PreviewStart> {
        let delivery = store.preview_delivery(project, plan, None)?;
        let config = delivery.preview.context("请先配置项目启动方式")?;
        let root = Path::new(&delivery.target.root);
        let automatic = delivery.demo.is_some();
        let key = (project.to_owned(), plan.to_owned());
        let mut instances = self.0.lock().unwrap();
        if let Some(entry) = instances.get(&key) {
            let mut entry = entry.lock().unwrap();
            if automatic {
                entry.poll();
                if entry.status.state == "running" && !entry.status.url.as_deref().and_then(|url| local_url(url).ok())
                    .as_ref().and_then(readiness).is_some_and(|code| (200..300).contains(&code))
                {
                    entry.finish(Some("Demo 服务已停止响应".into()));
                }
            }
            if entry.process.is_some() {
                return Ok(PreviewStart::Status {
                    status: entry.status(false),
                });
            }
        }
        let delivered = delivery.commit.as_deref().context("缺少交付提交")?;
        let checkout = if automatic {
            Some(PreviewCheckout::create(root, delivered)?)
        } else {
            let head = git_head(root)?;
            let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
            let dirty = !git(root, &["status", "--porcelain"])?.is_empty();
            let version = format!("{branch}:{head}:{dirty}");
            if (head != delivered || branch != delivery.target.branch || dirty)
                && confirmed.as_deref() != Some(&version)
            {
                return Ok(PreviewStart::Confirm {
                    version,
                    message: format!(
                        "交付版本为 {}（{}）。当前目录为 {}（{}）{}。启动当前目录的版本？",
                        delivery.target.branch, delivered.chars().take(8).collect::<String>(), branch,
                        head.chars().take(8).collect::<String>(), if dirty { "，有未提交改动" } else { "" }
                    ),
                });
            }
            None
        };
        let entry = Arc::new(Mutex::new(Instance::launch(&config, root, checkout, false, true)?));
        let status = entry.lock().unwrap().status(false);
        let observer = Arc::downgrade(&entry);
        instances.insert(key.clone(), entry);
        let registry = Arc::downgrade(&self.0);
        let demo_store = automatic.then(|| store.clone());
        std::thread::spawn(move || {
            let mut demo_store = demo_store;
            while let Some(entry) = observer.upgrade() {
                let (alive, outcome) = {
                    let mut entry = entry.lock().unwrap();
                    let alive = entry.poll();
                    let outcome = match entry.status.state {
                        _ if demo_store.is_none() => None,
                        "starting" => None,
                        "running" => Some(DeliveryDemo::Ready),
                        _ => Some(DeliveryDemo::Unavailable {
                            error: entry.status.error.clone().unwrap_or_else(|| "Demo 启动已停止".into()),
                        }),
                    };
                    (alive, outcome)
                };
                if let (Some(outcome), Some(registry)) = (outcome, registry.upgrade()) {
                    let store = demo_store.take().expect("outcome requires a delivery store");
                    let instances = registry.lock().unwrap();
                    if instances.get(&key).is_some_and(|current| Arc::ptr_eq(current, &entry)) {
                        if let Err(error) = store.record_demo(&key.0, &key.1, outcome) {
                            eprintln!("[crewvela] 保存 Demo 状态失败：{error:#}");
                        }
                    }
                }
                drop(entry);
                if !alive { break; }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        Ok(PreviewStart::Status { status })
    }

    pub(super) fn prepare_delivery(&self, store: &MultiAgentStore, project: &str, plan: &str) -> Result<()> {
        let delivery = store.preview_delivery(project, plan, None)?;
        if matches!(delivery.demo, Some(DeliveryDemo::Preparing)) {
            if let Err(error) = self.start(store, project, plan, None) {
                store.record_demo(project, plan, DeliveryDemo::Unavailable { error: format!("{error:#}") })?;
            }
        }
        Ok(())
    }
}

impl MultiAgentStore {
    pub(super) fn preview_delivery(
        &self,
        project: &str,
        plan: &str,
        update: Option<PreviewConfig>,
    ) -> Result<ManagedDelivery> {
        use rusqlite::params;
        let connection = self.connection.lock().unwrap();
        let raw: String = connection
            .query_row(
                "SELECT data_json FROM ma_conversation_plans WHERE id=?1 AND project_id=?2",
                params![plan, project],
                |row| row.get(0),
            )
            .context("找不到本项目的交付记录")?;
        let mut plan_data: super::ConversationExecutionPlan = serde_json::from_str(&raw)?;
        let delivery = plan_data
            .managed
            .as_mut()
            .context("此任务没有托管交付记录")?;
        ensure!(delivery.status == "completed", "项目完成交付后才能启动");
        if let Some(config) = update {
            ensure!(delivery.demo.is_none(), "Demo 启动方式由交付提交提供");
            config.validate(Path::new(&delivery.target.root))?;
            delivery.preview = Some(config);
            connection.execute(
                "UPDATE ma_conversation_plans SET data_json=?1 WHERE id=?2 AND project_id=?3",
                params![serde_json::to_string(&plan_data)?, plan, project],
            )?;
        }
        Ok(plan_data.managed.expect("validated delivery"))
    }

    fn record_demo(&self, project: &str, plan: &str, status: DeliveryDemo) -> Result<()> {
        use rusqlite::{params, TransactionBehavior};
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data_json FROM ma_conversation_plans WHERE id=?1 AND project_id=?2", params![plan, project], |row| row.get(0))?;
        let mut plan_data: super::ConversationExecutionPlan = serde_json::from_str(&raw)?;
        if let Some(delivery) = plan_data.managed.as_mut().filter(|delivery| delivery.demo.is_some()) {
            delivery.demo = Some(status);
            tx.execute("UPDATE ma_conversation_plans SET data_json=?1 WHERE id=?2 AND project_id=?3", params![serde_json::to_string(&plan_data)?, plan, project])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn save_project_preview(
        &self,
        project: &str,
        plan: &str,
        config: PreviewConfig,
    ) -> Result<()> {
        self.preview_delivery(project, plan, Some(config))
            .map(|_| ())
    }
}
