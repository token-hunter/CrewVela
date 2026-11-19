use super::*;
use crate::multi_agent::executor::{execute_run, AgentTerminalHost};
use crate::multi_agent::{ClaimTaskInput, CollaborationBridge};
use crate::sessions::{NewAgentSession, SessionManager};
use std::path::Path;
use std::process::Command;

pub(super) fn isolated_cli_contract(test: &str) -> Result<bool> {
    const CHILD: &str = "CREWVELA_ISOLATED_CLI_CONTRACT";
    if std::env::var(CHILD).ok().as_deref() == Some(test) { return Ok(true); }
    let output = Command::new(std::env::current_exe()?).args([test,"--ignored","--nocapture","--test-threads=1"])
        .env(CHILD,test).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(output.status.success() && stdout.contains("1 passed"), "{stdout}\n{stderr}");
    eprint!("{stderr}");
    Ok(false)
}

#[test]
#[ignore = "requires installed Codex CLI and Node; local loopback provider"]
fn installed_codex_planner_retains_provider_and_offers_no_tools() -> Result<()> {
    if !isolated_cli_contract("installed_codex_planner_retains_provider_and_offers_no_tools")? { return Ok(()); }
    let root = std::env::temp_dir().join(format!("cvc-{}", &id()[..8]));
    let home = root.join("codex");
    std::fs::create_dir_all(&home)?;
    crate::test_profile::initialize(&root.join("profile"))?;
    let server_file = root.join("fixture.cjs");
    std::fs::write(&server_file,r#"
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
http.createServer(async(req,res)=>{
  if (req.method !== 'POST') { res.writeHead(404);res.end();return; }
  const chunks=[];for await(const c of req) chunks.push(c);
  const payload=JSON.parse(Buffer.concat(chunks)), tools=(payload.tools||[]).map(t=>t.name||t.type);
  fs.writeFileSync(path.join(process.argv[2],'request.json'),JSON.stringify({tools,model:payload.model}));
  if(tools.length || payload.model!=='gpt-5.4') {res.writeHead(400);res.end('unexpected tool or model');return;}
  const item={id:'msg_contract',type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:'Native planning receipt',annotations:[]}]};
  res.writeHead(200,{'Content-Type':'text/event-stream'});
  const write=(type, fields)=>res.write('event: '+type+'\ndata: '+JSON.stringify({type,...fields})+'\n\n');
  write('response.created',{response:{id:'resp_contract',created_at:1,status:'in_progress',output:[]}});
  write('response.output_item.added',{output_index:0,item:{...item,content:[],status:'in_progress'}});
  write('response.output_item.done',{output_index:0,item});
  write('response.completed',{response:{id:'resp_contract',status:'completed',created_at:1,output:[item],usage:{input_tokens:1,output_tokens:1,total_tokens:2}}});
  res.end();
}).listen(0,'127.0.0.1',function(){console.log('http://127.0.0.1:'+this.address().port+'/v1')});
"#)?;
    struct Provider(std::process::Child);
    impl Drop for Provider { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let mut provider = Provider(Command::new("node").arg(server_file).arg(&root)
        .stdout(std::process::Stdio::piped()).spawn()?);
    use std::io::{BufRead, BufReader};
    let mut url = String::new();
    BufReader::new(provider.0.stdout.take().unwrap()).read_line(&mut url)?;
    let url = url.trim();
    ensure!(url.starts_with("http://127.0.0.1:"));
    std::fs::write(home.join("config.toml"),format!("model = 'gpt-5.4'\nmodel_provider = 'localcontract'\n[model_providers.localcontract]\nname = 'Local contract'\nbase_url = '{url}'\nwire_api = 'responses'\nenv_key='CREWVELA_ROUTE_AUTH'\n"))?;
    std::env::set_var("CODEX_HOME",&home);
    std::env::set_var("OPENAI_BASE_URL",url);
    std::env::set_var("CREWVELA_ROUTE_AUTH","dummy-local-only");
    let (fixture,mut input,_) = managed_plan_fixture();
    let member = fixture.store.create_agent(serde_json::from_value(json!({"projectId":fixture.project,
        "name":"规划","runtimeKind":"codex","role":"orchestrator"}))?)?;
    input.participant_agent_ids.push(member.id.clone());
    let store = &fixture.store;
    let (round,_) = store.begin_conversation(input)?;
    let turn = prepare_turn(store,&round.id,0,"plan",1,&member.id)?;
    let task = store.snapshot(&fixture.project)?.tasks.into_iter().find(|t| t.id == turn.task_id).unwrap();
    let run = store.claim_task(ClaimTaskInput {task_id:task.id.clone(),agent_id:member.id.clone(),lease_seconds:120})?;
    let host=AgentTerminalHost::new(crate::pty::PtyManager::new(),SessionManager::open_at(root.join("sessions.json"),root.join("events"))?);
    let result=execute_run(store,store.project(&fixture.project)?,task,member,run.id,&AtomicBool::new(false),host,CollaborationBridge::start(store.clone())?);
    let request=std::fs::read_to_string(root.join("request.json")).unwrap_or_default();
    result.with_context(||format!("isolated Codex sample {}, local dispatch {request}",root.display()))?;
    let observed: Value=serde_json::from_str(&request)?;
    ensure!(observed["tools"]==json!([]) && observed["model"]=="gpt-5.4");
    let locator=store.latest_session_locator(&turn.task_id)?.unwrap();
    ensure!(locator["managedTurn"]=="completed" && locator["threadId"].is_string());
    Ok(())
}

// Exercise production adapters and OS PTYs without using a model, credentials
// or a live application. A subprocess owns the test profile and fake PATH.
#[test]
fn managed_native_adapters_resume_the_exact_turn_after_acceptance_and_startup_failures() {
    const ENV: &str = "CREWVELA_MANAGED_NATIVE_TEST_DIR";
    if let Some(root) = std::env::var_os(ENV) {
        let root = Path::new(&root);
        assert!(root.starts_with(std::env::temp_dir()));
        assert_eq!(std::fs::read(root.join("fixture")).unwrap(), b"managed-native-test");
        crate::test_profile::initialize(&root.join("app profile")).unwrap();
        for kind in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex] {
            crate::runtime_preferences::set_launch_default(kind.as_str(), "auto").unwrap();
        }
        run_member(root, AgentRuntimeKind::ClaudeCode, RetryCase::Complete);
        run_member(root, AgentRuntimeKind::Codex, RetryCase::Complete);
        run_member(root, AgentRuntimeKind::ClaudeCode, RetryCase::UserBusy);
        run_member(root, AgentRuntimeKind::Codex, RetryCase::PriorCommitOutsideScope);
        run_member(root, AgentRuntimeKind::Codex, RetryCase::ApprovalBeforeSubmit);
        return;
    }
    let root = std::env::temp_dir().join(format!("cvn-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(root.join("fixture"), b"managed-native-test").unwrap();
    let script = bin.join("fixture.cjs");
    std::fs::write(&script, FIXTURE_CLI).unwrap();
    let node = which::which("node").unwrap();
    for (name, kind) in [("claude", "claude_code"), ("codex", "codex")] {
        #[cfg(windows)]
        std::fs::write(bin.join(format!("{name}.cmd")), format!("@\"{}\" \"{}\" {kind} %*\r\n", node.display(), script.display())).unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\nexec \"{}\" \"{}\" {kind} \"$@\"\n", node.display(), script.display())).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let outcome = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "multi_agent::conversation::tests::managed_native::managed_native_adapters_resume_the_exact_turn_after_acceptance_and_startup_failures", "--nocapture"])
        .env(ENV, &root).env("PATH", path).env("CODEX_HOME", root.join("codex-home"))
        .env("CODEX_WINDOWS_REGISTERED_CORE", "1")
        .env("CLAUDE_CONFIG_DIR", root.join("claude-home")).output().unwrap();
    assert!(outcome.status.success() && String::from_utf8_lossy(&outcome.stdout).contains("1 passed"),
        "{}\n{}", String::from_utf8_lossy(&outcome.stdout), String::from_utf8_lossy(&outcome.stderr));
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Clone, Copy, PartialEq)]
enum RetryCase { Complete, UserBusy, PriorCommitOutsideScope, ApprovalBeforeSubmit }

fn run_member(root: &Path, kind: AgentRuntimeKind, case: RetryCase) {
    let (fixture, input, plan_json) = managed_plan_fixture();
    let store = &fixture.store;
    let worker_id = &fixture.agents[0];
    store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind=?2 WHERE id=?1", [worker_id.as_str(), kind.as_str()]).unwrap();
    let chat = fixture.run_input(input, &ScriptedReplies::new(vec![plan_json]));
    let plan = &chat.execution_plans[0];
    let ids = store.apply_conversation_plan_in(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id.clone(), client_request_id: format!("managed-{}", plan.id),
    }, true).unwrap();
    let task_id = &ids[0];
    store.connection.lock().unwrap().execute("UPDATE ma_tasks SET max_attempts=3,retry_backoff_seconds=0 WHERE id=?1", [task_id]).unwrap();
    if kind == AgentRuntimeKind::Codex {
        store.connection.lock().unwrap().execute("UPDATE ma_tasks SET description=description || ?2 WHERE id=?1", [task_id.as_str(), "\n收到的原字符必须是 →°×🐍"]).unwrap();
    }
    let project = store.project(&fixture.project).unwrap();
    let agent = store.agent_profile(worker_id).unwrap();
    let sessions = SessionManager::open_at(root.join(format!("sessions-{}.json", fixture.project)), root.join("event queue")).unwrap();
    let app_session = sessions.create_agent(NewAgentSession {
        project_id: project.id.clone(), agent_id: worker_id.clone(), cwd: project.root_path.clone(),
        runtime_kind: kind.as_str().into(), agent_name: agent.name.clone(), avatar_id: None,
    }).unwrap();
    let pty = crate::pty::PtyManager::new();
    let host = AgentTerminalHost::new(pty.clone(), sessions.clone());
    let bridge = CollaborationBridge::start(store.clone()).unwrap();
    let mut first = None;
    let key = if kind == AgentRuntimeKind::Codex { "threadId" } else { "sessionId" };
    for stage in ["first", "startup-failure", "finish"] {
        let runtime_stage = match (case, stage) {
            (RetryCase::PriorCommitOutsideScope, "first") => "first-outside",
            (RetryCase::ApprovalBeforeSubmit, "finish") => "approval",
            _ => stage,
        };
        std::fs::write(root.join("stage"), runtime_stage).unwrap();
        let task = store.snapshot(&project.id).unwrap().tasks.into_iter().find(|task| &task.id == task_id).unwrap();
        let run = store.claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: worker_id.clone(), lease_seconds: 120 }).unwrap();
        let outcome = execute_run(store, project.clone(), task, agent.clone(), run.id, &AtomicBool::new(false), host.clone(), bridge.clone());
        let locator = store.latest_session_locator(task_id).unwrap().unwrap_or_else(|| panic!("missing {kind:?} {stage} locator: {outcome:?}"));
        if stage == "first" {
            let error = format!("{:#}", outcome.unwrap_err());
            assert!(error.contains("验收测试文件不存在"), "{kind:?}: {error}");
            assert!(uuid::Uuid::parse_str(locator[key].as_str().unwrap()).is_ok());
            assert_eq!(locator["managedLaunchMode"], "auto");
            assert_eq!(std::path::PathBuf::from(locator["cwd"].as_str().unwrap()).canonicalize().unwrap(),
                Path::new(&sessions.get(&app_session.id).unwrap().cwd).canonicalize().unwrap());
            if case == RetryCase::UserBusy { sessions.register_agent_prompt(&app_session.id, "用户在当前终端处理其他事情"); }
            first = Some(locator);
        } else {
            let original = first.as_ref().unwrap();
            for field in [key, "cwd", "managedBaseline", "managedLaunchMode"] { assert_eq!(locator[field], original[field], "{field}"); }
            if case == RetryCase::UserBusy {
                assert!(outcome.unwrap_err().to_string().contains("保留当前会话"));
                let original_pty = original["ptyId"].as_u64().unwrap() as u32;
                assert!(pty.is_running(original_pty));
                pty.kill(original_pty).unwrap();
                break;
            }
            if stage == "startup-failure" { assert!(outcome.unwrap_err().to_string().contains("原生会话启动失败")); }
            else if case == RetryCase::ApprovalBeforeSubmit {
                assert!(format!("{:#}", outcome.unwrap_err()).contains("审批面板"));
                assert!(!root.join("unexpected-approval").exists());
                let run = store.snapshot(&project.id).unwrap().runs.into_iter().find(|run| run.task_id == *task_id && run.attempt == 3).unwrap();
                assert!(run.error.unwrap().contains("审批面板"));
            }
            else {
                if case == RetryCase::PriorCommitOutsideScope {
                    let error = outcome.unwrap_err().to_string();
                    assert!(error.contains("超出用户确认目录范围") && error.contains("outside.txt"), "{error}");
                } else { outcome.unwrap(); }
                let history: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("resume.json")).unwrap()).unwrap();
                assert_eq!(history["resumed"], original[key]);
                let cwd = Path::new(history["cwd"].as_str().unwrap()).canonicalize().unwrap();
                assert_eq!(cwd, Path::new(original["cwd"].as_str().unwrap()).canonicalize().unwrap());
                pty.kill(locator["ptyId"].as_u64().unwrap() as u32).unwrap();
            }
        }
    }
}

const FIXTURE_CLI: &str = r#"
const fs = require('node:fs'), path = require('node:path'), { randomUUID } = require('node:crypto');
const [kind, ...args] = process.argv.slice(2), codex = kind === 'codex';
if (args.includes('--version')) { console.log(kind + ' fixture 1.0'); process.exit(0); }
const root = process.env.CREWVELA_MANAGED_NATIVE_TEST_DIR, stage = fs.readFileSync(path.join(root, 'stage'), 'utf8');
const after = key => args.includes(key) ? args[args.indexOf(key) + 1] : undefined;
if (!process.stdin.isTTY || args.includes('exec') || args.includes('--print')) throw Error('expected native TUI');
const resumed = after(codex ? 'resume' : '--resume'), id = resumed || after('--session-id') || randomUUID();
let events;
if (codex) {
  if (!args.includes('--approve-for-me') || args.includes('--dangerously-bypass-hook-trust')) throw Error('wrong Codex permissions');
  if (process.platform === 'win32' && process.env.CODEX_WINDOWS_REGISTERED_CORE === '1') throw Error('standalone Codex inherited registered Core');
  const notify = JSON.parse(args.find(v => v.startsWith('notify=')).slice(7));
  if (notify[3] !== process.env.CREWVELA_SESSION_ID) throw Error('incorrect app session');
  events = notify[2];
  const sessions = path.join(root, 'codex-home', 'sessions');
  if (path.resolve(process.env.CODEX_HOME) !== path.dirname(sessions)) throw Error('PTY changed the isolated CODEX_HOME');
  fs.mkdirSync(sessions, {recursive:true});
  fs.writeFileSync(path.join(sessions, 'rollout-' + id + '.jsonl'),
    JSON.stringify({type:'session_meta', payload:{id, source:'cli', cwd:process.cwd()}}) + '\n');
} else {
  if (after('--permission-mode') !== 'auto' || !args.includes('--strict-mcp-config')) throw Error('wrong Claude permissions');
  const mcp = JSON.parse(fs.readFileSync(after('--mcp-config'), 'utf8')).mcpServers;
  if (Object.keys(mcp).join() !== 'crewvela' || !mcp.crewvela.headers.Authorization.startsWith('Bearer ')) throw Error('unexpected scoped MCP');
  const settings = JSON.parse(fs.readFileSync(after('--settings'), 'utf8'));
  events = settings.hooks.SessionStart[0].hooks[0].command.match(/--session-hook "([^"]+)"/)[1];
}
if (!stage.startsWith('first') && !resumed) throw Error('fresh session on retry');
if (stage === 'startup-failure') { console.error('fixture startup failure'); process.exit(3); }
if (resumed) fs.writeFileSync(path.join(root, 'resume.json'), JSON.stringify({resumed, cwd:process.cwd()}));
function event(name, extra={}) {
  const to = path.join(events, randomUUID() + '.json');
  fs.writeFileSync(to + '.tmp', JSON.stringify({session_id:id, hook_event_name:name, _crewvela_app_session_id:process.env.CREWVELA_SESSION_ID, ...extra}));
  fs.renameSync(to + '.tmp', to);
}
process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
setTimeout(() => process.exit(7), 30_000).unref();
const ready = () => {
  process.stdout.write('\x1b[2J\x1b[H');
  if (codex) process.stdout.write('OpenAI Codex (v0.169.0)\r\n\r\n› \x1b[2mAsk Codex to do anything\x1b[22m\r\n  ? for shortcuts\x1b[3;3H\x1b]7501;state=idle:app=codex\x07');
  else {
    process.stdout.write('Claude Code\r\n\r\n>\u00a0Try a command\x1b[3;3H');
    event('SessionStart');
  }
};
const paintClaudeTrust = (selected) => process.stdout.write('\x1b[2J\x1b[HAccessing workspace:\r\n' +
  (selected ? '  No, exit\r\n> Yes, I trust this folder' : '> No, exit\r\n  Yes, I trust this folder') +
  '\r\nEnter to confirm · Esc to cancel');
let startup = stage.startsWith('first'), selected = codex;
if (!startup) ready();
else if (codex) {
  if (!args.some(arg => arg.startsWith('projects={') && /trust_level\s*=\s*"?untrusted"?/.test(arg))) throw Error('missing per-process restricted workspace');
  process.stdout.write('Folder access\r\nConfig, hooks, and exec policies from untrusted folders stay disabled.\r\n' +
    'Opening will not change saved trust.\r\n› 1. Open restricted\r\n  2. Back to Agent Command Center\r\nenter continue · esc back');
} else paintClaudeTrust(false);
let buffer = '', prompt = '', codexRepainted = false, lateMenuShown = false;
process.stdin.on('data', chunk => {
  if (chunk.includes('\x03')) process.exit(0);
  buffer += chunk;
  if (startup) {
    if (!selected && buffer === '\x1b[B') {
      selected = true;
      buffer = '';
      paintClaudeTrust(true);
    } else if (/[\r\n]/.test(buffer)) {
      if (!selected || !/^\r?\n?$/.test(buffer)) throw Error('submitted the startup menu without the expected selection');
      startup = false;
      buffer = '';
      ready();
    }
    return;
  }
  const start = buffer.indexOf('\x1b[200~'), end = buffer.indexOf('\x1b[201~');
  if (start >= 0 && end >= start) {
    prompt = buffer.slice(start + 6, end).replace(/\r\n?/g, '\n');
    buffer = buffer.slice(end + 6);
    if (codex) {
      const pointer = 'Do task: env CV_TASK';
      if (prompt !== pointer) throw Error('expected only the task file pointer in the composer');
      const taskFile = process.env.CV_TASK;
      const envKey = 'shell_environment_policy.set.CV_TASK=';
      const configured = args.find(arg => arg.startsWith(envKey))?.slice(envKey.length);
      if (configured !== JSON.stringify(taskFile) && configured !== taskFile) throw Error('task file not forwarded to Codex tools');
      const brief = fs.readFileSync(taskFile, 'utf8');
      if (!brief.includes('收到的原字符必须是 →°×🐍')) throw Error('managed Codex task file lost Unicode characters');
      const repaint = () => {
        process.stdout.write('\x1b[3;3H' + pointer + '\x1b[K');
        codexRepainted = true;
      };
      if (stage === 'approval') {
        repaint();
        setTimeout(() => process.stdout.write('\x1b[H\x1b[2JWould you like to run the following command?\r\n1. Yes, proceed (y)\r\n2. No, and tell Codex what to do differently (esc)'), 25);
      } else {
        process.stdout.write('\x1b]7501;state=working:app=codex\x07');
        setTimeout(() => process.stdout.write('\x1b]7501;state=idle:app=codex\x07'), 35);
        setTimeout(repaint, 90);
      }
    } else {
      const variable = /^Read \$(CV_[0-9a-f]{5})$/.exec(prompt)?.[1];
      if (!variable) throw Error('expected a run-specific Claude task instruction');
      const taskFile = process.env[variable];
      if (!after('--append-system-prompt')?.includes(taskFile)) throw Error('no executable task-file instruction for the current run');
      const brief = fs.readFileSync(taskFile, 'utf8');
      if (!brief.includes('当前任务：') || !brief.includes('本轮为全自动托管')) throw Error('Claude prompt lost the full task');
      process.stdout.write('\x1b[3;3H' + prompt + '\x1b[K');
    }
  }
  if (!prompt || !/[\r\n]/.test(buffer)) return;
  if (codex && !codexRepainted) throw Error('Enter before idle and a fresh composer echo');
  if (codex && stage === 'finish' && !lateMenuShown) {
    lateMenuShown = true;
    buffer = '';
    process.stdout.write('\x1b[2J\x1b[HFolder access\r\nConfig, hooks, and exec policies from untrusted folders stay disabled.\r\n' +
      'Opening will not change saved trust.\r\n› 1. Open restricted\r\n  2. Back to Agent Command Center\r\nenter continue · esc back');
    return;
  }
  if (lateMenuShown && !/^\r?\n?$/.test(buffer)) throw Error('replayed task during folder-access continuation');
  if (stage === 'approval') fs.writeFileSync(path.join(root, 'unexpected-approval'), 'unexpected Enter');
  if (stage === 'first-outside') {
    fs.writeFileSync('outside.txt', 'outside assigned paths');
    const git = (...argv) => require('node:child_process').execFileSync('git', argv);
    git('add', 'outside.txt');
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@localhost', 'commit', '-m', 'fixture outside scope');
  }
  if (stage === 'finish') {
    fs.mkdirSync('src/owned',{recursive:true});
    fs.writeFileSync('src/owned/check.test.js', "require('node:test')('managed fixture', () => {})");
  }
  event(codex ? 'AgentTurnComplete' : 'Stop', {input_prompt:prompt, last_assistant_message:'fixture done'});
  prompt = ''; buffer = '';
});
"#;
