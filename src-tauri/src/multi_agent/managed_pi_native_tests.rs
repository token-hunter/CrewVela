use super::*;
use crate::multi_agent::{
    executor::execute_run, AgentTerminalHost, ClaimTaskInput, CollaborationBridge,
};
use crate::sessions::SessionManager;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

/// The loopback provider supplies predetermined tool requests. This exercises the installed
/// Pi CLI, its real extension discovery, PTY receipt hooks and actual files without a model.
#[test]
#[ignore = "requires installed Pi 1.5.0 and Node; local loopback provider"]
fn installed_pi_confines_native_edits_and_resumes_its_exact_session() -> Result<()> {
    if !super::managed_native::isolated_cli_contract(
        "installed_pi_confines_native_edits_and_resumes_its_exact_session",
    )? {
        return Ok(());
    }
    let root = std::env::temp_dir().join(format!("cvp-{}", &id()[..8]));
    let profile = root.join("pi-profile");
    std::fs::create_dir_all(&profile)?;
    let server_file = root.join("provider.cjs");
    std::fs::write(&server_file, LOOPBACK_PROVIDER)?;
    struct Provider(Child);
    impl Drop for Provider {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Provider(
        Command::new("node")
            .arg(server_file)
            .arg(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?,
    );
    use std::io::{BufRead, BufReader};
    let mut base = String::new();
    BufReader::new(child.0.stdout.take().unwrap()).read_line(&mut base)?;
    ensure!(
        base.trim().starts_with("http://127.0.0.1:"),
        "local test provider did not start"
    );
    std::fs::write(
        profile.join("models.json"),
        serde_json::to_vec(&json!({"providers":{"test":{
        "baseUrl":base.trim(),"api":"openai-completions","apiKey":"local-contract",
        "models":[{"id":"contract","name":"Local contract"}]}}}))?,
    )?;
    std::fs::write(
        profile.join("settings.json"),
        br#"{"defaultProvider":"test","defaultModel":"contract","defaultThinkingLevel":"off"}"#,
    )?;
    std::env::set_var("PI_CODING_AGENT_DIR", &profile);
    std::env::set_var("PI_OFFLINE", "1");
    verify_delivery(&root, AgentRuntimeKind::Pi)
}

pub(super) fn verify_delivery(root: &Path, kind: AgentRuntimeKind) -> Result<()> {
    crate::test_profile::initialize(&root.join("crewvela"))?;
    let (fixture, input, raw_plan) = managed_plan_fixture();
    let worker = &fixture.agents[0];
    let draft = serde_json::from_str::<Value>(&raw_plan)?;
    let reviewer = draft["tasks"][1]["assigneeAgentId"]
        .as_str()
        .unwrap()
        .to_owned();
    let integrator = draft["tasks"][2]["assigneeAgentId"]
        .as_str()
        .unwrap()
        .to_owned();
    for member in [worker, &reviewer, &integrator] {
        fixture.store.connection.lock().unwrap().execute(
            "UPDATE ma_agents SET runtime_kind=?2 WHERE id=?1",
            [member.as_str(), kind.as_str()],
        )?;
    }
    let chat = fixture.run_input(input, &ScriptedReplies::new(vec![raw_plan]));
    let plan = chat
        .execution_plans
        .first()
        .context("test plan was not accepted")?;
    let store = &fixture.store;
    let ids = store.apply_conversation_plan_in(
        &ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: plan.id.clone(),
            client_request_id: format!("managed-{}", plan.id),
        },
        true,
    )?;
    store.set_project_status(&fixture.project, ProjectStatus::Active)?;
    store.connection.lock().unwrap().execute(
        "UPDATE ma_tasks SET max_attempts=3,retry_backoff_seconds=0 WHERE id=?1",
        [&ids[0]],
    )?;
    let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events"))?;
    let pty = crate::pty::PtyManager::new();
    let host = AgentTerminalHost::new(pty.clone(), sessions);
    let bridge = CollaborationBridge::start(store.clone())?;
    let run = |task_id: &str, member: &str| -> Result<()> {
        let task = store
            .snapshot(&fixture.project)?
            .tasks
            .into_iter()
            .find(|t| t.id == task_id)
            .unwrap();
        let record = store.claim_task(ClaimTaskInput {
            task_id: task.id.clone(),
            agent_id: member.into(),
            lease_seconds: 120,
        })?;
        execute_run(
            store,
            store.project(&fixture.project)?,
            task,
            store.agent_profile(member)?,
            record.id,
            &AtomicBool::new(false),
            host.clone(),
            bridge.clone(),
        )
    };
    let first = run(&ids[0], worker).unwrap_err();
    let diagnostic = || std::fs::read_to_string(root.join("provider.log")).unwrap_or_default();
    ensure!(
        format!("{first:#}").contains("验收测试文件不存在"),
        "first turn: {first:#} {} {}",
        root.display(),
        diagnostic()
    );
    let old = store
        .latest_session_locator(&ids[0])?
        .context("Missing native session")?;
    let dir = Path::new(old["cwd"].as_str().unwrap());
    ensure!(
        dir.join("src/owned/value.js").is_file() && !dir.join("outside.js").exists(),
        "write confinement failed"
    );
    run(&ids[0], worker)
        .with_context(|| format!("resumed turn: {} {}", root.display(), diagnostic()))?;
    let last = store.latest_session_locator(&ids[0])?.unwrap();
    for key in ["sessionId", "sessionPath", "cwd", "managedBaseline"] {
        ensure!(old[key] == last[key], "{key} changed across retry");
    }
    ensure!(
        last["managedTurn"] == "completed" && old["messageId"] != last["messageId"],
        "missing native turn evidence"
    );
    ensure!(
        std::fs::read_to_string(dir.join("src/owned/value.js"))?.contains("= 25"),
        "native edit did not apply"
    );
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
    ensure!(reopened.recover_managed_deliveries_at_startup()? == [fixture.project.clone()],
        "settled commit and original grant did not permit continuation after reopening the store");
    run(&ids[1], &reviewer)
        .with_context(|| format!("review: {} {}", root.display(), diagnostic()))?;
    let reviewed = store.latest_session_locator(&ids[1])?.unwrap();
    ensure!(
        reviewed["managedTurn"] == "completed" && reviewed["sessionId"] != last["sessionId"],
        "review did not provide an independent turn"
    );
    run(&ids[2], &integrator)
        .with_context(|| format!("integration: {} {}", root.display(), diagnostic()))?;
    let integrated = store.latest_session_locator(&ids[2])?.unwrap();
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
    ensure!(
        reopened.recover_managed_deliveries_at_startup()? == [fixture.project.clone()],
        "fully accepted, already merged candidate was not eligible for settlement after restart"
    );
    let persisted = reopened.managed_plan_running(&fixture.project)?.unwrap();
    let snapshot = reopened.snapshot(&fixture.project)?;
    for task_id in &ids {
        for run in snapshot.runs.iter().filter(|run| &run.task_id == task_id) {
            let source = reopened.conversation_run_locator(&fixture.project, &run.id)?;
            let messages = crate::session_switch_history::read_conversation_run(&source)?;
            let users: Vec<_> = messages.iter().filter(|message| message.role == "user").collect();
            ensure!(users.len() == 1 && messages.iter().any(|message| message.role == "assistant" && !message.text.trim().is_empty()),
                "托管原生记录未精确读出本轮用户输入和答复");
            if kind != AgentRuntimeKind::Pi {
                ensure!(users[0].text.contains(&run.id), "原生记录展示了其他 ACP 轮次的输入");
            }
        }
    }
    let sha = crate::multi_agent::executor::git_head(Path::new(&snapshot.project.root_path))?;
    reopened.settle_idle_managed_plan(
        &snapshot,
        &persisted,
        &crate::multi_agent::project_preview::ProjectPreviews::default(),
        &AtomicBool::new(false),
    )?;
    let completed = reopened
        .conversation_snapshot(&fixture.project)?
        .execution_plans
        .remove(0)
        .managed
        .unwrap();
    ensure!(completed.status == "completed" && completed.commit.as_deref() == Some(sha.as_str()));
    for locator in [&last, &reviewed, &integrated] {
        if let Some(id) = locator["ptyId"].as_u64() {
            if pty.is_running(id as u32) {
                pty.kill(id as u32)?;
            }
        }
    }
    eprintln!("{kind:?} native scopes, receipts, independent review and same-session retry confirmed in {}",root.display());
    Ok(())
}

const LOOPBACK_PROVIDER: &str = r#"
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const log=path.join(process.argv[2],'provider.log');
const all=['cc_research_list','cc_research_read','cc_research_search'],write=[...all,'crewvela_write','crewvela_edit'];
function next(body) {
  const available=(body.tools||[]).map(t=>t.function.name).sort();
  const writing=available.includes('crewvela_write');
  if (available.join() !== [...(writing?write:all)].sort().join()) throw Error('unexpected tools '+available);
  const user=body.messages.findLastIndex(m=>m.role==='user');
  const reply=body.messages.slice(user+1).filter(m=>m.role==='tool');
  const nr=reply.length;
  if (!writing) {
    if (!nr) return ['cc_research_read',{path:'src/owned/value.js'}];
    if (!JSON.stringify(reply.at(-1)).includes('25')) throw Error('review did not see the actual delivery');
    return '{"verdict":"approved","summary":"Read value.js from the delivered worktree and confirmed 25."}';
  }
  if (JSON.stringify(body.messages[user].content).includes('当前任务：integration')) {
    if (!nr) return ['crewvela_write',{path:'README.md',content:'Run node --test tests/acceptance.test.js to verify value 25.\n'}];
    if (nr===1) return ['crewvela_write',{path:'tests/acceptance.test.js',content:"const test=require('node:test');test('assembled value',()=>require('node:assert/strict').equal(require('../src/owned/value'),25));\n"}];
    return 'Integration completed.';
  }
  const retry=body.messages.slice(0,user).some(m=>m.role==='user' && JSON.stringify(m.content).includes('CrewVela message'));
  if (retry) {
    if (!nr) return ['crewvela_edit',{path:'src/owned/value.js',edits:[{oldText:'= 24',newText:'= 25'}]}];
    if (nr===1) return ['crewvela_write',{path:'src/owned/check.test.js',content:"const test = require('node:test'); test('value', () => require('node:assert/strict').equal(require('./value'), 25));\n"}];
    return 'Completed the native edit and its scoped regression test.';
  }
  if (!nr) return ['crewvela_write',{path:'outside.js',content:'unexpected'}];
  if (nr===1) {
    if (!JSON.stringify(reply[0]).match(/范围|scope|denied|拒绝/i)) throw Error('unauthorized write not denied');
    return ['crewvela_write',{path:'src/owned/value.js',content:'module.exports = 24;\n'}];
  }
  return 'Initial delivery intentionally has no test.';
}
let count=0;
http.createServer(async(req,res)=>{
  try {
    const chunks=[];for await(const c of req) chunks.push(c);
    const body=JSON.parse(Buffer.concat(chunks)), result=next(body), id='turn'+(++count);
    fs.appendFileSync(log,JSON.stringify({id,result:typeof result==='string'?'text':result[0]})+'\n');
    res.writeHead(200,{'Content-Type':'text/event-stream'});
    const delta=(d,finish=null)=>res.write('data: '+JSON.stringify({id,object:'chat.completion.chunk',created:1,model:'contract',choices:[{index:0,delta:d,finish_reason:finish}]})+'\n\n');
    if (typeof result==='string') { delta({role:'assistant',content:result});delta({},'stop'); }
    else {delta({role:'assistant',tool_calls:[{index:0,id:'call'+count,type:'function',function:{name:result[0],arguments:JSON.stringify(result[1])}}]});delta({},'tool_calls');}
    res.end('data: [DONE]\n\n');
  } catch(error) { fs.appendFileSync(log,String(error)+'\n');res.writeHead(400);res.end(String(error)); }
}).listen(0,'127.0.0.1',function(){console.log('http://127.0.0.1:'+this.address().port+'/v1')});
"#;
