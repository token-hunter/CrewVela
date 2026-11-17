// Production components with isolated IPC; no user projects or real CLI calls.
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1442';
const output='.scratch/conversation-live-activity';mkdirSync(output,{recursive:true});
const browser=await chromium.launch({channel:'chrome'});
const errors=[];
try {
 const page=await browser.newPage({viewport:{width:1122,height:1402}});
 page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?activity=1`);
 const claude=page.locator('[data-message-id="a1"]'), codex=page.locator('[data-message-id="a2"]'), pi=page.locator('[data-message-id="a3"]');
 await claude.getByText(/正在读取文件/).waitFor();
 assert.equal(await claude.locator('.member-activity-details').count(),0);
 await claude.getByRole('button',{name:'查看活动',exact:true}).click();
 assert.equal(await claude.getByRole('button',{name:'收起活动',exact:true}).getAttribute('aria-expanded'),'true');
 await claude.getByText('读取 src/api/auth.ts',{exact:true}).waitFor();
 await claude.getByText(/调度心跳正常/).waitFor();
 await codex.getByText('45秒未收到新活动，需核实').waitFor();
 assert.equal(await codex.locator('.member-activity-spin').count(),0);
 assert.equal(await pi.locator('.member-activity-spin').count(),0);
 await claude.getByRole('button',{name:'查看Claude的原会话'}).click();
 assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='fixture_open_session').at(-1).args.sessionId),'session-claude');
 // Screenshot the actual selected surface, excluding fixture-only window chrome.
 await page.addStyleTag({content:'.conversation-fixture-resource{display:none}'});
 await page.mouse.move(0,0);await page.locator('.conversation-transcript').evaluate(el=>el.scrollTop=0);
 await page.screenshot({path:`${output}/desktop.png`});
 // Normal snapshots preserve expansion and scroll state; actual activity drives stages.
 await page.evaluate(()=>{const m=window.conversationMock;m.activityEvents.push({seq:6,projectId:'A',runId:'run-claude',taskId:'activity-task-claude',agentId:'claude',eventType:'agent_activity',payload:{kind:'reply'},createdAt:Date.now()});});
 await claude.getByText(/正在生成回复/).first().waitFor();
 assert.equal(await claude.getByRole('button',{name:'收起活动'}).count(),1);
 await page.evaluate(()=>{window.conversationMock.activityApprovals.push({id:'approval-1',projectId:'A',runId:'run-claude',status:'pending'});});
 await claude.getByText(/等待你确认/).waitFor();
 assert.equal(await claude.locator('.member-activity-state .member-activity-spin').count(),0);
 await page.evaluate(()=>{const m=window.conversationMock;m.activityApprovals.length=0;const message=m.chats.A.messages.find(x=>x.id==='a1');message.status='completed';message.updatedAt=Date.now();m.runHistory.find(x=>x.id==='run-claude').status='succeeded';m.runHistory.find(x=>x.id==='run-claude').finishedAt=Date.now();});
 await claude.locator('.member-activity-state').getByText('已完成',{exact:true}).waitFor();
 assert.equal(await claude.locator('.member-activity-spin').count(),0);
 assert.equal(await claude.getByRole('button',{name:'停止本轮团队回复'}).count(),0);
 // Keyboard collapse, reduced-motion, wrapping, composer visibility.
 await claude.getByRole('button',{name:'收起活动'}).focus();await page.keyboard.press('Enter');
 assert.equal(await claude.locator('.member-activity-details').count(),0);
 await page.emulateMedia({reducedMotion:'reduce'});
 await page.setViewportSize({width:390,height:844});
 await claude.getByRole('button',{name:'查看活动'}).click();
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
 const box=await page.locator('.conversation-composer').boundingBox();assert.ok(box.y+box.height<=844);
 await page.screenshot({path:`${output}/narrow.png`});
 await page.reload();
 await page.locator('[data-message-id="a1"] .member-activity-toggle').click();
 const icon=page.locator('[data-message-id="a1"] .member-activity-state .member-activity-spin');
 assert.equal(await icon.evaluate(el=>getComputedStyle(el).animationName),'none');
 await page.locator('[data-message-id="a1"]').getByRole('button',{name:'停止本轮团队回复'}).click();
 assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_control').at(-1).args.action),'cancel');
 await page.locator('[data-message-id="a1"] .member-activity-state').getByText('已取消',{exact:true}).waitFor();
 assert.equal(await page.locator('[data-message-id="a1"] .member-activity-spin').count(),0);
 for(const width of [640,320]) {
   const light=await browser.newPage({viewport:{width,height:900}});light.on('pageerror',e=>errors.push(e.message));
   await light.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?activity=1&theme=light`);
   const member=light.locator('[data-message-id="a1"]');await member.getByRole('button',{name:'查看活动'}).click();
   await member.getByRole('button',{name:'停止本轮团队回复'}).scrollIntoViewIfNeeded();
   assert.equal(await light.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   const composer=await light.locator('.conversation-composer').boundingBox();assert.ok(composer.y+composer.height<=900);
   assert.equal(await light.locator('[data-message-id="a2"] .member-activity-recency').evaluate(e=>getComputedStyle(e).color),'rgb(154, 103, 0)');
   if(width===640)await light.screenshot({path:`${output}/light.png`});
   await light.close();
 }
 assert.deepEqual(errors,[]);
 console.log('PASS activity UI: real event stages, timeline, original session, terminal, stop scope, keyboard, reduced motion, narrow viewport');
} finally {await browser.close();}
