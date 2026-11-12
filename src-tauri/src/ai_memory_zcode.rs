//! Read-only selection of exact ai-memory session IDs for ZCode finalization.
use super::*;
use std::collections::HashSet;

const PAGE_SIZE: usize = 100;
const MAX_ROWS: usize = 10_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZcodeMemorySession {
    pub id: String,
    pub started_at: String,
    pub cwd: String,
}

fn sessions_url(base: &str, workspace: &str, project: &str, offset: usize) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base).context("记忆服务地址无效")?;
    if url.query().is_some() || url.fragment().is_some() {
        bail!("记忆服务基础地址不能包含查询参数或片段");
    }
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("记忆服务地址不能追加项目路径"))?
        .pop_if_empty()
        .extend([
            "api",
            "v1",
            "workspaces",
            workspace,
            "projects",
            project,
            "sessions",
        ]);
    url.query_pairs_mut()
        .append_pair("include_open", "true")
        .append_pair("limit", &PAGE_SIZE.to_string())
        .append_pair("offset", &offset.to_string());
    Ok(url)
}

fn same_project_directory(root: &Path, cwd: &str) -> bool {
    let Ok(cwd) = Path::new(cwd).canonicalize() else {
        return false;
    };
    // canonicalize resolves junctions/symlinks before checking the boundary.
    cwd == root || cwd.starts_with(root)
}

fn selected_sessions(value: &Value, project: &Path) -> Result<Vec<ZcodeMemorySession>> {
    let rows = value
        .get("sessions")
        .and_then(Value::as_array)
        .context("记忆服务没有返回有效的会话列表")?;
    let mut result = Vec::new();
    for row in rows {
        if row.get("agent_kind").and_then(Value::as_str) != Some("zcode")
            || !row.get("ended_at").is_some_and(Value::is_null)
        {
            continue;
        }
        let id = row
            .get("session_id")
            .and_then(Value::as_str)
            .context("ZCode 记忆会话缺少 ID")?;
        uuid::Uuid::parse_str(id).context("记忆服务返回了无效会话 ID")?;
        let cwd = row
            .get("cwd")
            .and_then(Value::as_str)
            .context("ZCode 记忆会话缺少工作目录")?;
        if !same_project_directory(project, cwd) {
            continue;
        }
        let started_at = row
            .get("started_at")
            .and_then(Value::as_str)
            .context("ZCode 记忆会话缺少开始时间")?;
        result.push(ZcodeMemorySession {
            id: id.to_owned(),
            started_at: started_at.to_owned(),
            cwd: cwd.to_owned(),
        });
    }
    Ok(result)
}

async fn list_sessions(project_dir: String) -> Result<Vec<ZcodeMemorySession>> {
    let requested = PathBuf::from(&project_dir);
    if !requested.is_absolute() || !requested.is_dir() {
        bail!("请选择存在的绝对项目目录");
    }
    let root = requested.canonicalize().context("无法解析项目目录")?;
    let (executable, _) = locate_executable().context("未找到 ai-memory，请先安装")?;
    let data = data_dir()?;
    let cwd = project_dir.clone();
    let scope = tokio::task::spawn_blocking(move || inspect_scope(&executable, &data, &cwd))
        .await?
        .context("无法确认所选项目的记忆作用域，请检查项目标记、捕获策略及服务地址")?;
    query_sessions(
        &reqwest::Client::new(),
        &resolved_server_url()?,
        &scope.0,
        &scope.1,
        &root,
    )
    .await
}

async fn query_sessions(
    client: &reqwest::Client,
    base: &str,
    workspace: &str,
    project: &str,
    root: &Path,
) -> Result<Vec<ZcodeMemorySession>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for offset in (0..MAX_ROWS).step_by(PAGE_SIZE) {
        let url = sessions_url(base, workspace, project, offset)?;
        let body = service_json(
            authenticated_request(client, url.to_string())?.timeout(Duration::from_secs(5)),
        )
        .await
        .map_err(|error| anyhow::anyhow!("查询 ZCode 记忆会话失败：{error}"))?;
        let rows = body
            .get("sessions")
            .and_then(Value::as_array)
            .context("记忆服务没有返回有效的会话列表")?;
        if rows.len() > PAGE_SIZE {
            bail!("记忆服务未遵守会话分页限制");
        }
        for session in selected_sessions(&body, root)? {
            if seen.insert(session.id.clone()) {
                result.push(session);
            }
        }
        if rows.len() < PAGE_SIZE {
            return Ok(result);
        }
    }
    bail!("项目会话超过本次查询的 10000 条限制，请先在 Wiki 中缩小并核实目标会话");
}

#[tauri::command]
pub async fn ai_memory_zcode_sessions(
    project_dir: String,
) -> Result<Vec<ZcodeMemorySession>, String> {
    list_sessions(project_dir)
        .await
        .map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_url_keeps_scope_components_separate_and_requests_open_sessions() {
        let url = sessions_url("http://127.0.0.1:47374/prefix", "w space", "p/other", 100).unwrap();
        assert_eq!(
            url.path(),
            "/prefix/api/v1/workspaces/w%20space/projects/p%2Fother/sessions"
        );
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs.get("include_open").map(|v| v.as_ref()), Some("true"));
        assert_eq!(pairs.get("offset").map(|v| v.as_ref()), Some("100"));
        assert!(sessions_url("http://127.0.0.1:47374/?token=x", "w", "p", 0).is_err());
    }

    #[test]
    fn selects_only_open_zcode_sessions_with_exact_memory_ids_in_the_selected_directory() {
        let root = std::env::temp_dir().join(format!("zcode-memory-list-{}", uuid::Uuid::new_v4()));
        let project = root.join("project");
        let other = root.join("project-other");
        std::fs::create_dir_all(project.join("child")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let make = |cwd: &Path, agent: &str, ended: Value, id: String| {
            json!({
                "session_id": id, "cwd": cwd, "agent_kind": agent, "started_at": "2026-11-04T03:26:37Z", "ended_at": ended
            })
        };
        let id = uuid::Uuid::new_v4().to_string();
        let body = json!({"sessions": [
            make(&project, "zcode", Value::Null, id.clone()),
            make(&project.join("child"), "zcode", Value::Null, uuid::Uuid::new_v4().to_string()),
            make(&other, "zcode", Value::Null, uuid::Uuid::new_v4().to_string()),
            make(&project, "codex", Value::Null, uuid::Uuid::new_v4().to_string()),
            make(&project, "zcode", json!("2026-11-04T04:00:00Z"), uuid::Uuid::new_v4().to_string())
        ]});
        let selected = selected_sessions(&body, &project.canonicalize().unwrap()).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, id);
        assert!(selected_sessions(&json!({}), &project).is_err());
        let native_id =
            json!({"sessions": [make(&project, "zcode", Value::Null, "native-session-1".into())]});
        assert!(selected_sessions(&native_id, &project.canonicalize().unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn read_pagination_request(socket: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        // Accepted Winsock sockets inherit the nonblocking accept-polling mode.
        socket.set_nonblocking(false).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = socket.read(&mut chunk).unwrap();
            assert!(count > 0, "分页请求头未完整接收，连接已关闭");
            request.extend_from_slice(&chunk[..count]);
            assert!(request.len() <= 16 * 1024, "分页请求头超出测试上限");
        }
        String::from_utf8(request).unwrap()
    }

    #[test]
    fn pagination_fixture_reads_fragmented_request_headers() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, connected) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => std::thread::yield_now(),
                    Err(error) => panic!("分页测试连接失败：{error}"),
                }
            };
            accepted.send(()).unwrap();
            read_pagination_request(&mut socket)
        });
        let mut client = std::net::TcpStream::connect(address).unwrap();
        connected.recv_timeout(Duration::from_secs(2)).unwrap();
        let prefix = b"GET /sessions?include_open=true&offset=";
        let suffix = b"100 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        client.write_all(prefix).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let sent = client.write_all(suffix);
        let request = server.join().unwrap();
        sent.unwrap();
        assert_eq!(request.as_bytes(), [prefix.as_slice(), suffix.as_slice()].concat());
    }

    #[tokio::test]
    async fn pagination_reads_later_open_sessions_and_rejects_invalid_responses() {
        use std::io::Write;
        let root =
            std::env::temp_dir().join(format!("zcode-memory-pages-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let page_one = json!({"sessions": (0..PAGE_SIZE).map(|_| json!({"agent_kind": "codex", "ended_at": null})).collect::<Vec<_>>()});
        let page_two = json!({"sessions": [{"agent_kind": "zcode", "ended_at": null, "session_id": id, "cwd": root, "started_at": "2026-11-04T03:26:37Z"}]});
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (offset, body) in [(0, page_one), (100, page_two), (0, json!({"wrong": []}))] {
                let start = std::time::Instant::now();
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && start.elapsed() < Duration::from_secs(5) =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(error) => panic!("未收到分页测试请求：{error}"),
                    }
                };
                let request = read_pagination_request(&mut socket);
                assert!(request.contains("include_open=true"));
                assert!(request.contains(&format!("offset={offset} ")));
                let body = body.to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = query_sessions(&client, &base, "w", "p", &root.canonicalize().unwrap())
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, id);
        assert!(query_sessions(&client, &base, "w", "p", &root)
            .await
            .is_err());
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
