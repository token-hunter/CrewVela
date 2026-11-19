//! Opt-in acceptance against real installed runtimes. Uses a disposable unit-converter clone and a separate CrewVela profile.
use super::*;
use crate::multi_agent::{AgentTerminalHost, CollaborationBridge, MultiAgentSupervisor};
use crate::{pty::PtyManager, sessions::SessionManager};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    ensure!(output.status.success(), "isolated Git command failed ({})", output.status);
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn verify_in_browser(url: &str, image: &Path) -> Result<()> {
    let script = r#"
        import assert from 'node:assert/strict';
        import { chromium } from 'playwright';
        const url = new URL(process.argv[1]);
        assert.equal(url.hostname, '127.0.0.1');
        const browser = await chromium.launch({ channel: 'chrome' });
        try {
          const page = await browser.newPage();
          page.setDefaultTimeout(10000);
          await page.goto(url.href);
          await page.getByRole('button', { name: '一键填入 25', exact: true }).click();
          assert.equal(await page.locator('#value-input').inputValue(), '25');
          assert.equal((await page.locator('#result').innerText()).trim(), '');
          await page.getByRole('button', { name: '换算', exact: true }).click();
          await page.locator('#result').filter({ hasText: /77/ }).waitFor();
          await page.screenshot({ path: process.argv[2] });
        } finally {
          await browser.close();
        }
    "#;
    let status = Command::new("node")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .args(["--input-type=module", "--eval", script, url]).arg(image).status()?;
    ensure!(status.success(), "unit-converter browser acceptance failed ({status})");
    Ok(())
}

fn select_runtimes() -> Result<[AgentRuntimeKind; 3]> {
    let settings = std::env::var("CREWVELA_UNIFIED_LIVE_RUNTIMES").unwrap_or_else(|_| "codex,claude_code,codex".into());
    let selected = settings.split(',').map(|key| match key.trim() {
        "claude_code" => Ok(AgentRuntimeKind::ClaudeCode),
        "codex" => Ok(AgentRuntimeKind::Codex),
        "kimi_code" => Ok(AgentRuntimeKind::KimiCode),
        "pi" => Ok(AgentRuntimeKind::Pi),
        "opencode" => Ok(AgentRuntimeKind::OpenCode),
        "zcode_cli" => Ok(AgentRuntimeKind::ZCodeCli),
        _ => bail!("unsupported interactive runtime in acceptance case"),
    }).collect::<Result<Vec<_>>>()?;
    selected.try_into().map_err(|_| anyhow!("provide three runtimes in worker, reviewer, integrator order"))
}

#[test]
#[ignore = "opt-in: isolated real runtimes, requires CREWVELA_RUN_REAL_UNIFIED_MANAGED=1, a unit-converter seed repository, CrewVela hook executable and Chrome"]
fn real_unified_managed_unit_converter() -> Result<()> {
    ensure!(std::env::var("CREWVELA_RUN_REAL_UNIFIED_MANAGED").as_deref() == Ok("1"), "explicit acceptance opt-in required");
    ensure!(std::env::var_os("CREWVELA_UNIFIED_LIVE_HOOK_EXE").is_some_and(|value| {
        let path = Path::new(&value);
        path.is_absolute() && path.is_file()
    }), "set CREWVELA_UNIFIED_LIVE_HOOK_EXE to a built CrewVela executable for native CLI callbacks");
    let seed = std::env::var("CREWVELA_UNIFIED_UNIT_SOURCE").context("set the committed unit-converter seed repository")?;
    let seed = crate::acp::canonical_harness_directory(&seed)?;
    let kinds = select_runtimes()?;
    let short_id = id();
    let root = std::env::temp_dir().join(format!("cvu-{}", &short_id[..8]));
    std::fs::create_dir(&root)?;
    crate::test_profile::initialize(&root.join("profile"))?;
    let working = root.join("project");
    let status = Command::new("git").args(["clone", "--quiet", "--no-local", "--branch", "main", "--"])
        .arg(seed).arg(&working).status()?;
    ensure!(status.success(), "could not create the isolated unit converter clone");
    git(&working, &["remote", "remove", "origin"])?;
    git(&working, &["config", "user.name", "CrewVela Acceptance"])?;
    git(&working, &["config", "user.email", "acceptance@example.invalid"])?;
    let initial_head = git(&working, &["rev-parse", "HEAD"])?;
    eprintln!("UNIFIED_LIVE_ROOT={} RUNTIMES={kinds:?}", root.display());

    // Defaults are written only to the new isolated profile above. The real
    // CLI installations retain their login and model settings.
    for kind in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex] {
        crate::runtime_preferences::set_launch_default(kind.as_str(), "auto")?;
    }
    let store = MultiAgentStore::open_at(root.join("store.db"))?;
    let project = store.create_project(CreateProjectInput {
        name: "单位换算器 · 托管验收".into(), goal: "验证待交付版本能够真实运行与换算".into(),
        root_path: working.display().to_string(), default_branch: "main".into(),
    })?;
    let mut participants = Vec::new();
    for (kind, (name, role)) in kinds.into_iter().zip([
        ("换算器开发", AgentRole::Worker), ("独立审查", AgentRole::Reviewer), ("联调验收", AgentRole::Integrator),
    ]) {
        let profile = store.create_agent(CreateAgentInput {
            avatar_id: None, project_id: project.id.clone(), name: name.into(), runtime_kind: kind, role,
            description: name.into(), persona: Default::default(), persona_custom: String::new(), model: None,
            permission_profile: PermissionProfile::Auto, max_concurrency: 1,
        })?;
        participants.push(profile.id);
    }
    let terminals = AgentTerminalHost::new(PtyManager::new(), SessionManager::open_at(root.join("sessions.json"), root.join("session-events"))?);
    let bridge = CollaborationBridge::start(store.clone())?;
    let chat = ConversationSupervisor::default();
    let engineering = MultiAgentSupervisor::default();
    let body = r#"在隔离的单位换算器中交付一个输入快捷操作。点击「一键填入 25」按钮后把 25 填进现有输入框，保持手动点击「换算」。
只安排三项任务，任务的 reviewerAgentId 一律为 null。换算器开发只改 public/index.html 与 tests/frontend.test.js，验证命令 node --test tests/frontend.test.js。独立审查直接依赖开发任务，只读检查实际提交及测试证据，返回审查结论，禁写且不运行命令。联调验收直接依赖该审查，在 README.md、crewvela.launch.json 和 tests/integration.test.js 中补充用法、Demo 清单和覆盖实际 HTTP 换算的测试，验证命令 node --test tests/integration.test.js。
联调从提交版本本机启动 node server.js，验证首页、快捷按钮和 25°C 到 77°F，保留现有换算。Demo 清单使用 {"program":"node","args":["server.js"],"workingDirectory":".","url":null}，自动发现 PORT=0 的回环地址。只操作本测试项目，留在本地，不推送。"#;
    chat.send(store.clone(), ConversationSendInput {
        native_capabilities: false, managed: true, expected_conversation_id: None, chat_dispatch: None,
        project_id: project.id.clone(), client_message_id: id(), body: body.into(), attachment_ids: Vec::new(),
        participant_agent_ids: participants, mode: "execution_plan".into(), intent: "chat".into(),
        max_rounds: 1, max_duration_seconds: 900, task_rules: None,
    }, terminals, bridge, engineering.clone())?;

    let mut preview_plan = None;
    let outcome = (|| -> Result<()> {
        let started = Instant::now();
        let mut last_state = String::new();
        while started.elapsed() < Duration::from_secs(1200) {
            let snapshot = store.conversation_snapshot(&project.id)?;
            let round = snapshot.rounds.last().context("missing live planning round")?;
            let plan = snapshot.execution_plans.iter().find(|plan| plan.round_id == round.id);
            let delivery = plan.and_then(|plan| plan.managed.as_ref());
            let tasks = store.snapshot(&project.id)?.tasks.into_iter()
                .filter(|task| plan.is_some_and(|plan| plan.task_ids.contains(&task.id)))
                .map(|task| task.status).collect::<Vec<_>>();
            let state = format!("planning={} delivery={} tasks={tasks:?}", round.status, delivery.map_or("pending", |value| value.status.as_str()));
            if state != last_state {
                eprintln!("{state}");
                last_state = state;
            }
            ensure!(!matches!(round.status.as_str(), "failed" | "unconfirmed" | "needs_decision"),
                "live planning ended without delivery; inspect the isolated store at {}", root.display());
            if let (Some(plan), Some(delivery)) = (plan, delivery) {
                preview_plan = Some(plan.id.clone());
                ensure!(delivery.status != "failed", "live execution failed; inspect the isolated store at {}", root.display());
                if delivery.status == "completed" {
                    let hash = delivery.commit.as_deref().context("delivery is missing its accepted SHA")?;
                    ensure!(hash != initial_head && hash == git(&working, &["rev-parse", "HEAD"])?, "accepted SHA must be the new main HEAD");
                    let demo = serde_json::to_value(&delivery.demo)?;
                    ensure!(demo["state"] != "unavailable", "automatic Demo was unavailable; inspect the isolated store at {}", root.display());
                    let preview = serde_json::to_value(engineering.previews.status(&project.id, &plan.id, false))?;
                    if preview["state"] == "running" {
                        let url = preview["url"].as_str().context("running Demo has no URL")?;
                        verify_in_browser(url, &root.join("browser.png"))?;
                        eprintln!("DELIVERED_SHA={hash} BROWSER_SCREENSHOT={}", root.join("browser.png").display());
                        return Ok(());
                    }
                    ensure!(!matches!(preview["state"].as_str(), Some("failed" | "error")), "automatic Demo failed; inspect the isolated store at {}", root.display());
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("real acceptance timed out; inspect the isolated store at {}", root.display())
    })();
    if outcome.is_err() {
        let _ = chat.control(&store, &project.id, "cancel", None);
        let _ = engineering.cancel(&store, &project.id);
    }
    if let Some(plan) = preview_plan { engineering.previews.stop(&project.id, &plan); }
    outcome
}

#[test]
#[ignore = "opt-in: existing DeepSeek Harness production team chat over the installed, logged-in ACP runtime"]
fn real_unified_deepseek_acp_chat() -> Result<()> {
    ensure!(std::env::var("CREWVELA_RUN_REAL_UNIFIED_MANAGED").as_deref() == Ok("1"), "explicit acceptance opt-in required");
    let root = std::env::temp_dir().join(format!("cvd-{}", id()));
    std::fs::create_dir(&root)?;
    crate::test_profile::initialize(&root.join("profile"))?;
    let fixture = Fixture::at(root);
    let agent = fixture.store.create_agent(CreateAgentInput {
        avatar_id: None, project_id: fixture.project.clone(), name: "ACP Probe".into(),
        runtime_kind: AgentRuntimeKind::DeepSeekHarness, role: AgentRole::Reviewer,
        description: "Reply to the current user message; this is an isolated chat test.".into(),
        persona: Default::default(), persona_custom: String::new(), model: None,
        permission_profile: PermissionProfile::Default, max_concurrency: 1,
    })?;
    let pty = PtyManager::new();
    let sessions = SessionManager::open_at(fixture.root.join("sessions.json"), fixture.root.join("session-events"))?;
    let host = AgentTerminalHost::new(pty, sessions);
    let bridge = CollaborationBridge::start(fixture.store.clone())?;
    let mut input = fixture.input();
    let marker = format!("ACP_OK_{}", &id()[..8]);
    input.body = format!("请在团队聊天中只回复这个标记，不调用任何工具：{marker}");
    input.native_capabilities = true;
    input.max_duration_seconds = 120;
    input.participant_agent_ids = vec![agent.id.clone()];
    let (round, _) = fixture.store.begin_conversation(input)?;
    run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &ExistingRunExecutor { host, bridge })?;
    let snapshot = fixture.store.conversation_snapshot(&fixture.project)?;
    ensure!(snapshot.rounds.last().is_some_and(|value| value.status == "completed"), "DeepSeek ACP team chat did not complete; isolated store: {}", fixture.root.display());
    ensure!(snapshot.messages.iter().any(|message| message.agent_id.as_deref() == Some(agent.id.as_str()) && message.body.trim() == marker),
        "DeepSeek ACP team chat did not return the marker; isolated store: {}", fixture.root.display());
    let state = fixture.store.snapshot(&fixture.project)?;
    ensure!(state.runs.iter().any(|run| run.agent_id == agent.id && run.session_locator.as_ref().is_some_and(|locator| locator["sessionId"].as_str().is_some())),
        "missing real ACP session evidence");
    Ok(())
}
