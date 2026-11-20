use super::*;
use std::io::{BufRead, BufReader};
use std::process::Stdio;

#[test]
#[ignore = "requires installed Kimi Code 2.6.3 and Node; isolated loopback model"]
fn installed_kimi_acp_confines_file_tools_and_resumes_delivery() -> Result<()> {
    if !super::managed_native::isolated_cli_contract(
        "installed_kimi_acp_confines_file_tools_and_resumes_delivery",
    )? {
        return Ok(());
    }
    verify(AgentRuntimeKind::KimiCode)
}

#[test]
#[ignore = "requires installed OpenCode 1.19.3 and Node; isolated loopback model"]
fn installed_opencode_acp_confines_file_tools_and_resumes_delivery() -> Result<()> {
    if !super::managed_native::isolated_cli_contract(
        "installed_opencode_acp_confines_file_tools_and_resumes_delivery",
    )? {
        return Ok(());
    }
    verify(AgentRuntimeKind::OpenCode)
}

fn verify(kind: AgentRuntimeKind) -> Result<()> {
    let root = std::env::temp_dir().join(format!("cva-{}", &id()[..8]));
    std::fs::create_dir(&root)?;
    std::fs::write(root.join("canary.txt"), "untouched")?;
    let mut command = crate::multi_agent::executor::hidden_command("node");
    command
        .args(["-e", PROVIDER])
        .arg(&root)
        .arg(kind.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut server, _identity) = crate::acp::spawn_owned_command(&mut command)?;
    let result = (|| -> Result<()> {
        let mut port = String::new();
        BufReader::new(server.stdout.take().unwrap()).read_line(&mut port)?;
        let url = format!("http://127.0.0.1:{}/v1", port.trim().parse::<u16>()?);
        let profile = root.join("original");
        std::fs::create_dir_all(&profile)?;
        if kind == AgentRuntimeKind::KimiCode {
            std::env::set_var("KIMI_CODE_HOME", &profile);
            std::env::remove_var("KIMI_MODEL_NAME");
            std::fs::write(profile.join("config.toml"), format!("default_model='fixture'\n[providers.fixture]\ntype='openai'\nbase_url='{url}'\napi_key='fixture'\n[models.fixture]\nprovider='fixture'\nmodel='fixture'\nmax_context_size=32768\n"))?;
            std::fs::write(
                profile.join("mcp.json"),
                r#"{"mcpServers":{"untrusted":{"command":"unavailable"}}}"#,
            )?;
        } else {
            for key in [
                "OPENCODE_CONFIG",
                "OPENCODE_CONFIG_DIR",
                "OPENCODE_CONFIG_CONTENT",
            ] {
                std::env::remove_var(key);
            }
            std::env::set_var("XDG_CONFIG_HOME", &profile);
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
            std::env::set_var("OPENCODE_AUTH_CONTENT", "{}");
            std::env::set_var("OPENCODE_DISABLE_MODELS_FETCH", "1");
            let dir = profile.join("opencode");
            std::fs::create_dir(&dir)?;
            std::fs::write(
                dir.join("opencode.json"),
                serde_json::to_vec(&json!({"model":"fixture/fixture",
                "provider":{"fixture":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":url,"apiKey":"fixture"},
                    "models":{"fixture":{"name":"Fixture"}}}},"mcp":{"untrusted":{"type":"local","command":["unavailable"]}}}))?,
            )?;
        }
        super::managed_pi_native::verify_delivery(&root, kind)?;
        ensure!(std::fs::read(root.join("canary.txt"))? == b"untouched");
        ensure!(
            !std::fs::read_to_string(root.join("provider.log"))?
                .lines()
                .any(|line| line.starts_with("Error:")),
            "回环模型遇到了未预期的请求，见 {}",
            root.display()
        );
        Ok(())
    })();
    crate::acp::stop_child_checked(&mut server)?;
    result
}

// Predetermined model responses exercise the real CLI, MCP connection, live run grants,
// exact native session retry, read-only review, Git settlement and host-run acceptance.
const PROVIDER: &str = r#"
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const root=process.argv[1], kimi=process.argv[2]==='kimi_code', prefix=kimi?'mcp__crewvela__':'crewvela_';
const allowed=['cc_research_read','cc_research_list','cc_research_search','cc_workspace_edit_read','cc_workspace_write_file'].map(s=>prefix+s);
const [read,,,edit,write]=allowed, canary=path.join(root,'canary.txt');
const log=(v)=>fs.appendFileSync(path.join(root,'provider.log'),v+'\n');
let count=0;
function next(body) {
  const msgs=body.messages||[], user=msgs.findLastIndex(m=>m.role==='user' && JSON.stringify(m.content).includes('CrewVela 托管轮次'));
  if(user<0 || msgs.some(m=>(m.role==='system'&&JSON.stringify(m.content).includes('You are a title generator'))
       || (m.role==='user'&&typeof m.content==='string'&&m.content.startsWith('Generate a title for this conversation:'))))return 'Fixture title';
  const prompt=JSON.stringify(msgs[user].content), available=(body.tools||[]).map(t=>t.function.name).sort();
  const replies=msgs.slice(user+1).filter(m=>m.role==='tool').map(m=>JSON.stringify(m.content)), nr=replies.length;
  const last=replies.at(-1)||'';
  log(JSON.stringify({available,nr,result:last.slice(0,220)}));
  if (prompt.includes('本轮工具执行已禁用')) {
    if (!prompt.includes('+module.exports = 25')) throw Error('Reviewer did not receive delivered commit');
    return '{"verdict":"approved","summary":"Verified value 25 in the delivered commit."}';
  }
  const advertised=kimi?available.filter(n=>n.startsWith('mcp__')):available;
  if (advertised.join()!==[...allowed].sort().join()) throw Error('Unexpected file tool catalog '+available);
  const call=(name,args)=>({name,args});
  if (prompt.includes('当前任务：integration')) {
    if(!nr)return call(write,{path:'README.md',text:'Run node --test tests/acceptance.test.js to verify value 25.\n'});
    if(nr===1)return call(write,{path:'tests/acceptance.test.js',text:"const test=require('node:test');test('assembled',()=>require('node:assert/strict').equal(require('../src/owned/value'),25));\n"});
    return 'Integrated the accepted value.';
  }
  const resumed=msgs.slice(0,user).some(m=>m.role==='user' && JSON.stringify(m.content).includes('CrewVela 托管轮次'));
  if(resumed) {
    if(!nr)return call(edit,{path:'src/owned/value.js'});
    const revision=replies[0].match(/\b[0-9a-f]{64}\b/)?.[0];
    if(!revision||!replies[0].includes('= 24')) throw Error('missing original text and revision');
    if(nr===1)return call(write,{path:'src/owned/value.js',text:'changed',revision:'stale'});
    if(nr===2) {
      if(!/发生变化/.test(last))throw Error('stale edit accepted');
      return call(write,{path:'src/owned/value.js',text:'module.exports = 25;\n',revision});
    }
    if(nr===3)return call(write,{path:'src/owned/check.test.js',text:"const test=require('node:test');test('value',()=>require('node:assert/strict').equal(require('./value'),25));\n"});
    return 'Revised value and wrote regression test.';
  }
  if(kimi && nr<4) {
    const attempts=[['Write',{path:canary,content:'changed'}],['Edit',{path:canary,old_string:'untouched',new_string:'changed'}],
      ['Bash',{command:'echo CREWVELA_LOCAL_TEST'}],['mcp__untrusted__probe',{}]];
    return call(...attempts[nr]);
  }
  if(kimi && !replies.slice(0,4).every(r=>/disabled by the active tool policy|denied by permission rule|not found/.test(r)))throw Error('unapproved tools were not denied');
  const step=nr-(kimi?4:0);
  if(!step)return call(write,{path:'outside.js',text:'changed'});
  if(step===1) {
    if(!/范围/.test(last))throw Error('write outside grant not denied');
    return call(read,{path:canary});
  }
  if(step===2) {
    if(!/超出/.test(last))throw Error('read outside worktree not denied');
    return call(write,{path:'src/owned/value.js',text:'module.exports = 24;\n'});
  }
  if(!last.includes('src/owned/value.js')||last.includes('ERROR'))throw Error('scoped write failed');
  return 'Initial value without its required test.';
}
http.createServer(async(req,res)=>{
  try{
    const chunks=[];for await(const c of req)chunks.push(c);
    if(!chunks.length){res.writeHead(200,{'content-type':'application/json'});return res.end('{"data":[]}');}
    const result=next(JSON.parse(Buffer.concat(chunks))), id='fixture'+(++count);
    res.writeHead(200,{'content-type':'text/event-stream'});
    const emit=(delta,finish=null)=>res.write('data: '+JSON.stringify({id,object:'chat.completion.chunk',created:1,model:'fixture',choices:[{index:0,delta,finish_reason:finish}]})+'\n\n');
    if(typeof result==='string'){emit({role:'assistant',content:result});emit({},'stop');}
    else{emit({role:'assistant',tool_calls:[{index:0,id:'call'+count,type:'function',function:{name:result.name,arguments:JSON.stringify(result.args)}}]});emit({},'tool_calls');}
    res.end('data: [DONE]\n\n');
  }catch(e){log(String(e));res.writeHead(400);res.end(String(e));}
}).listen(0,'127.0.0.1',function(){console.log(this.address().port);});
"#;
