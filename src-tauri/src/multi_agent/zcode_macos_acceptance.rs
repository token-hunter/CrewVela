//! Opt-in installed-runtime checks. Never run real-model tests implicitly.
use super::super::*;
use std::collections::HashSet;

struct IsolatedRoot(PathBuf);
impl IsolatedRoot {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("crewvela-zcode-dock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
    fn command(&self, account: bool) -> Result<Command> {
        let mut command = launch(&executable()?)?;
        command.env("ZCODE_STORAGE_DIR", self.0.join("storage"));
        if !account {
            command.env("ZCODE_DATA_BASE_DIR", &self.0)
                .env("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE", self.0.join("personal-empty.json"));
        }
        Ok(command)
    }
}
impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        // Keep evidence on cleanup failure instead of silently deleting elsewhere.
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("isolated ZCode test cleanup failed: {error}");
        }
    }
}

fn process_tree(root: u32) -> Result<HashSet<u32>> {
    let output = Command::new("/bin/ps").args(["-axo", "pid=,ppid="]).output()?;
    anyhow::ensure!(output.status.success(), "cannot inspect test process tree");
    let pairs: Vec<(u32, u32)> = String::from_utf8(output.stdout)?.lines().filter_map(|line| {
        let mut values = line.split_whitespace();
        Some((values.next()?.parse().ok()?, values.next()?.parse().ok()?))
    }).collect();
    let mut owned = HashSet::from([root]);
    loop {
        let before = owned.len();
        for (pid, parent) in &pairs {
            if owned.contains(parent) { owned.insert(*pid); }
        }
        if owned.len() == before { return Ok(owned); }
    }
}

fn check_dock(root: u32) -> Result<(usize, usize)> {
    let owned = process_tree(root)?;
    let output = Command::new("/usr/bin/lsappinfo").arg("list").output()?;
    anyhow::ensure!(output.status.success(), "cannot inspect LaunchServices");
    let entries = String::from_utf8_lossy(&output.stdout);
    let mut registered = 0;
    for line in entries.lines() {
        let Some((_, pid)) = line.split_once("pid = ") else { continue; };
        let Some(pid) = pid.split_whitespace().next().and_then(|value| value.parse::<u32>().ok()) else { continue; };
        if !owned.contains(&pid) { continue; }
        registered += 1;
        anyhow::ensure!(!line.contains("type=\"Foreground\""), "test ZCode process {pid} registered as Foreground");
        anyhow::ensure!(line.contains("type=\"UIElement\"") || line.contains("type=\"BackgroundOnly\""), "test ZCode process has unknown activation type");
    }
    anyhow::ensure!(registered > 0, "no test process registered; Dock check is inconclusive");
    Ok((owned.len(), registered))
}

#[test]
#[ignore = "installed macOS ZCode; isolated account/storage, real control plane and Dock check, no model request"]
fn zcode_macos_installed_sessions_stay_off_dock() -> Result<()> {
    let root = IsolatedRoot::new()?;
    let stopped = AtomicBool::new(false);
    let mut progress = || Ok(());
    let mut deny = |_: &Value, _: &AtomicBool| Ok(false);
    let mut connection = DesktopConnection::open(root.command(false)?, &root.0, None, false, vec![], &stopped, &mut deny, &mut progress)?;
    let pid = connection.client.child.id();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut largest = (0, 0);
    while Instant::now() < deadline {
        connection.poll(&stopped, &mut deny)?;
        let counts = check_dock(pid)?;
        if counts.0 > largest.0 { largest = counts; }
    }
    anyhow::ensure!(largest.0 > 1, "native workers were not observed");
    let original = connection.session_id.clone();
    connection.shutdown()?;
    drop(connection);
    let mut restored = DesktopConnection::open(root.command(false)?, &root.0, Some(&original), true, vec![], &stopped, &mut deny, &mut progress)?;
    check_dock(restored.client.child.id())?;
    restored.shutdown()?;
    drop(restored);
    println!("ZCODE_DOCK_CONTROL_OK processes={} registered={} foreground=0 create_restore=true no_model_request=true", largest.0, largest.1);
    Ok(())
}

#[test]
#[ignore = "requires explicit per-run user authorization; one real model prompt with native node_repl tool"]
fn zcode_macos_installed_tool_turn_stays_off_dock() -> Result<()> {
    anyhow::ensure!(std::env::var("CREWVELA_RUN_ZCODE_DOCK_MODEL").as_deref() == Ok("1"), "explicit one-request model opt-in required");
    let root = IsolatedRoot::new()?;
    let stopped = AtomicBool::new(false);
    let mut progress = || Ok(());
    // The only tool authorized for this fixture is a native JS calculation.
    let mut permission = |request: &Value, _: &AtomicBool| {
        let name = request["toolName"].as_str().or_else(|| request["tool"]["name"].as_str()).unwrap_or("");
        Ok(matches!(name, "mcp__node_repl__js" | "node_repl.js"))
    };
    let mut connection = DesktopConnection::open(root.command(true)?, &root.0, None, false, vec![], &stopped, &mut permission, &mut progress)?;
    let choice = connection.models.iter().find(|model| model.selection["providerId"] == "account:bigmodel-individual-coding-plan" && model.selection["modelId"] == "GLM-5.3").context("authorized account model unavailable")?.id.clone();
    connection.set_model(&choice, &stopped, &mut permission)?;
    check_dock(connection.client.child.id())?;
    println!("ZCODE_DOCK_MODEL_REQUEST_START");
    let id = connection.send_text("隔离工具验收：只调用一次内置 node_repl.js 工具执行 console.log(17 * 19)，然后回复工具返回的数值。不要读取、写入或列出任何文件，不要访问网络，不要调用其他工具。", &stopped, &mut permission)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        connection.poll(&stopped, &mut permission)?;
        check_dock(connection.client.child.id())?;
        anyhow::ensure!(connection.take_auth_error().is_none(), "model authentication failed; response omitted");
        match connection.turn_state(&id) {
            Some("completedSuccess") => break,
            Some("failed" | "completedInterrupted") => bail!("tool turn failed; response omitted"),
            _ => {}
        }
        if Instant::now() >= deadline {
            connection.cancel(&stopped, &mut permission)?;
            bail!("tool turn timed out and was cancelled");
        }
    }
    let rows = connection.rows();
    anyhow::ensure!(rows.iter().any(|row| row["kind"] == "toolCall" && row["status"] == "success" && (row["toolName"].as_str().is_some_and(|name| name.contains("node_repl")) || (row["display"]["serverName"] == "node_repl" && row["display"]["toolName"] == "js"))), "no successful native node_repl tool row");
    anyhow::ensure!(rows.iter().any(|row| row["kind"] == "assistantText" && row["text"].as_str().is_some_and(|text| text.contains("323"))), "model did not report tool result");
    let session = connection.session_id.clone();
    connection.shutdown()?;
    drop(connection);
    let mut restored = DesktopConnection::open(root.command(true)?, &root.0, Some(&session), false, vec![], &stopped, &mut permission, &mut progress)?;
    anyhow::ensure!(restored.rows().iter().any(|row| row["kind"] == "assistantText" && row["text"].as_str().is_some_and(|text| text.contains("323"))), "restored session lost tool turn");
    check_dock(restored.client.child.id())?;
    restored.shutdown()?;
    println!("ZCODE_DOCK_TOOL_OK model=GLM-5.3 native_tool=node_repl result=323 terminal_success=true restored=true foreground=0 model_prompts=1");
    Ok(())
}


fn mcp_request(client: &mut Client<'_>, id: u32, method: &str, params: Value) -> Result<Value> {
    client.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let frame = client.receive(deadline, &AtomicBool::new(false))?;
        if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
            anyhow::ensure!(frame.get("error").is_none(), "native MCP request rejected: {method}");
            return frame.get("result").cloned().context("native MCP response missing result");
        }
    }
}

#[test]
#[ignore = "installed macOS ZCode native node_repl MCP; isolated JS calculation and Dock check, no model request"]
fn zcode_macos_installed_native_js_tool_stays_off_dock() -> Result<()> {
    let root = IsolatedRoot::new()?;
    let runtime = root.command(false)?;
    let agent = executable()?;
    let server = agent.parent().context("missing Agent directory")?.join("packages/node-repl-host/dist/mcp/server.js");
    anyhow::ensure!(server.is_file(), "installed native node_repl service missing");
    let mut command = Command::new(runtime.get_program());
    command.arg(&server);
    for (key, value) in runtime.get_envs() {
        match value {
            Some(value) => { command.env(key, value); }
            None => { command.env_remove(key); }
        }
    }
    let mut progress = || Ok(());
    let mut client = Client::spawn_with_account_host(command, &root.0, &mut progress, None)?;
    // Current installed service is modern-only MCP: each request carries its envelope.
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientInfo":{"name":"crewvela-dock-acceptance","version":"1"},
        "io.modelcontextprotocol/clientCapabilities":{}
    });
    let catalog = mcp_request(&mut client, 1, "tools/list", json!({"_meta":meta}))?;
    anyhow::ensure!(catalog["tools"].as_array().is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "js")), "native js tool not advertised");
    let result = mcp_request(&mut client, 2, "tools/call", json!({"_meta":meta,"name":"js","arguments":{"code":"console.log(17 * 19)","title":"验证隔离计算"}}))?;
    anyhow::ensure!(result["isError"] != true && result["content"].as_array().is_some_and(|content| content.iter().any(|item| item["text"].as_str().is_some_and(|text| text.contains("323")))), "native JS worker did not return 323");
    check_dock(client.child.id())?;
    client.cleanup_checked()?;
    println!("ZCODE_DOCK_NATIVE_TOOL_OK mcp_tool=js result=323 foreground=0 no_model_request=true");
    Ok(())
}
