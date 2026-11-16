// Actual React/Markdown with fixture-only IPC. Never posts to real Agents or user projects.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL||'http://127.0.0.1:1420';
const browser=await chromium.launch({channel:'chrome'}),errors=[];
async function fresh(query='',width=780){const p=await browser.newPage({viewport:{width,height:1100}});p.on('pageerror',e=>errors.push(e.message));await p.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?${query}`);await p.getByRole('button',{name:/选择参与成员/}).waitFor();return p;}
const send=async(p,body)=>{await p.getByRole('combobox',{name:/给团队/}).fill(body);await p.locator('.conversation-composer').evaluate(form=>form.requestSubmit());};
const call=(p,command)=>p.evaluate(command=>window.conversationMock.calls.filter(c=>c.command===command),command);
try{
 for(const [body,ids,intent,mode] of [['hi',['claude','codex'],'chat','discussion'],['@Codex 解释这个函数',['codex'],'chat','discussion'],['@Claude Code @Codex 你们聊一下这个方案',['claude','codex'],'exchange','discussion'],['你们分工实现这个项目',['claude','codex'],'chat','execution_plan']]){
  const p=await fresh('empty=1&natural=1&unsupported=1');await send(p,body);
  await p.waitForFunction(()=>window.conversationMock.calls.some(c=>c.command==='ma_conversation_send'));
  const input=(await call(p,'ma_conversation_send'))[0].args.input;
  assert.deepEqual(input.participantAgentIds,ids);assert.equal(input.intent,intent);assert.equal(input.mode,mode);
  assert.equal(await p.locator('.conversation-message.agent').count(),ids.length);
  assert.equal(await p.locator('.conversation-message header small').count(),0);
  assert.equal((await call(p,'ma_conversation_execute_plan')).length,0,'Planning never grants execution permission');if(body==='hi'){mkdirSync('.scratch/natural-team-chat-20261117/ui',{recursive:true});await p.screenshot({path:'.scratch/natural-team-chat-20261117/ui/hello.png'});}await p.close();
 }
 const interrupt=await fresh('running=1');await send(interrupt,'@Codex 先回答这个问题');
 await interrupt.waitForFunction(()=>window.conversationMock.calls.some(c=>c.command==='ma_conversation_interrupt'));
 const targeted=(await call(interrupt,'ma_conversation_interrupt'))[0].args.input;
 assert.deepEqual(targeted.participantAgentIds,['codex']);assert.equal(targeted.mode,'discussion');assert.equal(targeted.intent,'chat');assert.ok(targeted.clientMessageId);await interrupt.close();
 const queued=await fresh('queued=1&natural=1');await queued.getByRole('button',{name:'继续发送',exact:true}).click();
 await queued.waitForFunction(()=>window.conversationMock.calls.some(c=>c.command==='ma_conversation_send'));
 const resumed=(await call(queued,'ma_conversation_send'))[0].args.input;assert.equal(resumed.clientMessageId,'pending-original');assert.deepEqual(resumed.participantAgentIds,['codex']);
 assert.equal(await queued.locator('.conversation-message.user').filter({hasText:'先回答这个问题'}).count(),1);await queued.close();
 const frozen=await fresh('queued=1&unconfirmed=1');assert.equal(await frozen.getByRole('button',{name:'继续发送',exact:true}).isDisabled(),true);assert.equal((await call(frozen,'ma_conversation_send')).length,0);await frozen.close();
 for(const theme of ['dark','light'])for(const width of [780,320]){
  const p=await fresh(`markdown=1&theme=${theme}`,width);const tracked=[];p.on('request',r=>{if(r.url().includes('example.invalid'))tracked.push(r.url());});
  const message=p.locator('[data-message-id="m90"]');await message.locator('strong',{hasText:'已经确认'}).waitFor();
  assert.equal(await message.locator('h1').innerText(),'方案');assert.equal(await message.locator('li').count(),2);assert.match(await message.locator('pre code').innerText(),/<safe>/);assert.equal(await message.locator('table tbody tr').count(),1);
  assert.equal(await message.locator('.conversation-message-body script,.conversation-message-body img').count(),0);assert.equal(await message.locator('a[href^="javascript:"]').count(),0);assert.equal(await p.evaluate(()=>window.unsafeExecuted),undefined);assert.deepEqual(tracked,[]);
  await message.getByRole('link',{name:'安全链接',exact:true}).click();assert.equal((await call(p,'plugin:opener|open_url')).length,1);
  assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  mkdirSync('.scratch/natural-team-chat-20261117/ui',{recursive:true});await p.screenshot({path:`.scratch/natural-team-chat-20261117/ui/${theme}-${width}.png`});await p.close();
 }
 assert.deepEqual(errors,[]);console.log('PASS natural chat UI: scoped delivery, chat/exchange/plan intent, targeted interrupt, no stage labels, real Markdown/GFM, no script/remote-image loading, dark/light/narrow');
}finally{await browser.close();}
