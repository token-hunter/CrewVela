// Production React, isolated IPC; no CLI/model call or user-project change.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const out = '.scratch/team-chat-task-views-20261117/ui'; mkdirSync(out,{recursive:true});
const browser = await chromium.launch({channel:'chrome'}), contexts=[], errors=[];
const tab=(p,name)=>p.getByRole('tab',{name,exact:true});
const editor=p=>p.getByRole('combobox',{name:'给团队发消息',exact:true});
const calls=(p,command)=>p.evaluate(command=>window.conversationMock.calls.filter(item=>item.command===command),command);
async function fresh(query='',width=680,height=980) {
 const context=await browser.newContext({viewport:{width,height}});contexts.push(context);
 const p=await context.newPage();p.setDefaultTimeout(10000);p.on('pageerror',e=>errors.push(e.message));
 await p.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?dual-views=1&${query}`);
 await p.locator('.conversation-members-trigger:not(:disabled)').waitFor();
 await p.waitForFunction(()=>document.querySelector('.conversation-members-trigger')?.textContent.includes('成员 3'));
 return p;
}
try {
 const attachmentPage=await fresh('natural=1&attachments=1', 420);
 await attachmentPage.getByRole('button',{name:'添加附件',exact:true}).click();
 const staged=attachmentPage.getByLabel('待发送附件',{exact:true});
 await staged.getByRole('button',{name:'移除附件 设计截图.png',exact:true}).waitFor();
 await staged.getByRole('button',{name:'移除附件 需求说明.md',exact:true}).click();
 assert.equal(await staged.getByRole('button').count(),1);
 assert.equal(await editor(attachmentPage).inputValue(),'');
 const attachmentSend=attachmentPage.getByRole('button',{name:'发送消息',exact:true});
 await attachmentSend.click();
 const sentAttachment=attachmentPage.getByLabel('消息附件',{exact:true}).getByRole('button',{name:'设计截图.png',exact:true});
 await sentAttachment.waitFor();
 await sentAttachment.click();
 const attachmentCalls=await calls(attachmentPage,'ma_conversation_send');
 assert.equal(attachmentCalls.length,1);
 assert.equal(attachmentCalls[0].args.input.attachmentIds.length,1);
 assert.equal(attachmentCalls[0].args.input.body,'');
 assert.equal((await calls(attachmentPage,'ma_conversation_open_attachment'))[0].args.attachmentId,attachmentCalls[0].args.input.attachmentIds[0]);
 assert.equal(await staged.count(),0);

 for (const theme of ['dark', 'light']) {
  const p = await fresh(`markdown=1&activity=1&theme=${theme}`, 1800);
  const layout = p.locator('.conversation-delegation-layout');
  const user = p.locator('[data-message-id="a0"]');
  const agent = p.locator('[data-message-id="m90"]');
  await agent.locator('.conversation-message-body').waitFor();
  const typography = (message) => message.evaluate(node => {
   const sender = node.querySelector('.conversation-message-content > header strong');
   const body = node.querySelector('.conversation-message-body');
   return { sender: getComputedStyle(sender).fontSize, body: getComputedStyle(body).fontSize };
  });
  await p.getByRole('button',{name:'查看活动',exact:true}).first().click();
  const surfaces = () => layout.evaluate(node => {
   const sizes = {
    '.conversation-heading select':['fontSize','height'],
    '.conversation-heading > .conversation-icon svg':['width','height'],
    '.conversation-task-link':['fontSize'],
    '.conversation-members-trigger':['fontSize','minHeight'],
    '.conversation-member-preview img':['width','height'],
    '.conversation-modes button':['fontSize','minHeight'],
    '.conversation-avatar':['width','height'],
    '.member-activity-state':['fontSize'],
    '.member-activity-state > svg':['width','height'],
    '.member-activity-toggle':['fontSize'],
    '.member-activity-details li':['fontSize'],
    '.conversation-composer textarea':['fontSize'],
    '.conversation-target':['fontSize','minHeight'],
    '.team-generate-plan':['fontSize'],
    '.conversation-send':['width','height'],
   };
   return Object.fromEntries(Object.entries(sizes).map(([selector,properties]) => {
    const element = node.querySelector(selector);
    if (!element) throw new Error(`Missing measured control: ${selector}`);
    const style = getComputedStyle(element);
    return [selector,Object.fromEntries(properties.map(property => [property,style[property]]))];
   }));
  });
  const wideUser = await typography(user);
  const wideAgent = await typography(agent);
  const wideSurfaces = await surfaces();
  const alignedBounds = () => p.locator('.team-work-views').evaluate(panel => {
   const bounds = node => { const {left, right, width} = node.getBoundingClientRect(); return {left, right, width}; };
   const panelBounds = bounds(panel);
   return {
    panel: panelBounds,
    surfaces: Object.fromEntries([
     '.conversation-heading', '.conversation-tabs-row', '.conversation-message',
     '.conversation-composer', '.conversation-error',
    ].flatMap(selector => {
     const element = panel.querySelector(selector);
     return element ? [[selector, bounds(element)]] : [];
    })),
   };
  });
  const aligned = async width => {
   const {panel, surfaces} = await alignedBounds();
   for (const [selector, rect] of Object.entries(surfaces)) {
    assert.ok(Math.abs(rect.width - 880) <= 2, `${selector} at ${width}px is ${rect.width}px wide`);
    assert.ok(Math.abs((rect.left + rect.right) - (panel.left + panel.right)) <= 2, `${selector} is centered at ${width}px`);
   }
  };
  const replyBody = p.locator('[data-message-id="a1"] .conversation-message-body');
  const wideHeight = await replyBody.evaluate(node => node.offsetHeight);
  for (const width of [320, 440, 500, 620, 680, 760, 899, 900, 941, 999, 1000, 1400, 1800]) {
   await layout.evaluate((node, width) => node.style.width = `${width}px`, width);
   assert.deepEqual(await typography(user), wideUser, `user typography at ${width}px`);
   assert.deepEqual(await typography(agent), wideAgent, `agent typography at ${width}px`);
   const measured = await surfaces();
   for (const [selector, expected] of Object.entries(wideSurfaces))
    assert.deepEqual(measured[selector], expected, `${selector} at ${width}px`);
   if (width >= 1000) await aligned(width);
  }
  await p.screenshot({path:`${out}/chat-centered-${theme}.png`});
  const tabsBefore = (await alignedBounds()).surfaces['.conversation-tabs-row'];
  await tab(p,'任务执行').click();
  assert.deepEqual((await alignedBounds()).surfaces['.conversation-tabs-row'], tabsBefore, 'shared tabs stay in place');
  assert.ok((await p.locator('.team-task-list').boundingBox()).width > 1500, 'wide task list keeps the workspace width');
  await tab(p,'团队聊天').click();
  assert.deepEqual(wideUser, {sender:'14px', body:'14px'});
  assert.deepEqual(wideAgent, wideUser);
  await layout.evaluate(node => node.style.width = '320px');
  assert.ok(await replyBody.evaluate(node => node.offsetHeight) > wideHeight, 'narrow panel wraps text');
  assert.ok(await layout.locator('.conversation-composer-actions').evaluate(node => node.scrollWidth <= node.clientWidth), 'narrow composer controls fit');
  const narrow = await alignedBounds();
  assert.equal(narrow.surfaces['.conversation-heading'].left - narrow.panel.left, 16);
  assert.equal(narrow.surfaces['.conversation-composer'].left - narrow.panel.left, 12);
  await layout.evaluate(node => node.style.width = '1800px');
  await p.evaluate(() => {
   const mock = window.conversationMock;
   mock.chats.A.rounds[0].status = 'completed';
   mock.failNew = true;
  });
  await editor(p).waitFor();
  await p.getByRole('button',{name:'讨论记录操作',exact:true}).click();
  await p.getByRole('menuitem',{name:'新建会话',exact:true}).click();
  await p.getByRole('alert').filter({hasText:'新建会话失败'}).waitFor();
  await aligned(1800);
 }
 const plan=await fresh('plan=pending');
 await plan.getByRole('button',{name:'确认并开始执行',exact:true}).waitFor();
 assert.equal((await calls(plan,'ma_conversation_execute_plan')).length,0);
 assert.equal(await plan.getByRole('region',{name:'待确认分工',exact:true}).count(),1);
 await plan.locator('.team-plan-contract summary').click();
 await plan.getByText('npm test',{exact:true}).waitFor();
 assert.match(await plan.locator('.team-plan-contract').innerText(),/src\/|独立工作树/);
 await plan.locator('.team-plan-contract summary').click();
 await plan.screenshot({path:`${out}/chat-dark.png`});
 await editor(plan).fill('保留当前草稿');
 await tab(plan,'任务执行').click();
 assert.equal(await editor(plan).isVisible(),false,'task view has no composer');
 assert.equal(await plan.getByRole('log').count(),0,'task view has no chat transcript');
 await plan.getByText('暂无工程任务',{exact:true}).waitFor();
 await tab(plan,'团队聊天').click();
 assert.equal(await editor(plan).inputValue(),'保留当前草稿');
 await plan.getByRole('button',{name:'调整分工',exact:true}).click();
 assert.match(await editor(plan).inputValue(),/^请调整分工：/);assert.match(await editor(plan).inputValue(),/保留当前草稿/);
 assert.equal((await calls(plan,'ma_conversation_execute_plan')).length,0,'adjusting only edits the draft');
 await plan.evaluate(()=>window.conversationMock.failApproval=true);
 await plan.getByRole('button',{name:'确认并开始执行',exact:true}).click();
 await plan.getByRole('alert').filter({hasText:'分工确认失败'}).waitFor();
 assert.equal(await tab(plan,'团队聊天').getAttribute('aria-selected'),'true');
 await plan.evaluate(()=>window.conversationMock.failApproval=false);
 await plan.getByRole('button',{name:'确认并开始执行',exact:true}).evaluate(node=>{node.click();node.click();});
 await plan.getByRole('heading',{name:'登录页面',exact:true}).waitFor();
 const approvals=await calls(plan,'ma_conversation_execute_plan');assert.equal(approvals.length,2);
 assert.equal(approvals[0].args.input.clientRequestId,approvals[1].args.input.clientRequestId,'failed confirmation retry keeps operation identity');
 assert.equal(await tab(plan,'任务执行').getAttribute('aria-selected'),'true');
 assert.equal(await plan.locator('.team-task-row').count(),3);
 assert.equal(await plan.getByRole('progressbar').first().getAttribute('aria-valuenow'),null,'no fabricated numeric progress');
 assert.match(await plan.getByRole('region',{name:'执行动态',exact:true}).innerText(),/已确认分工.*Claude.*开始实现登录页面/s);
 await plan.screenshot({path:`${out}/tasks-dark.png`});
 await plan.getByRole('button',{name:'查看记录',exact:true}).click();
 await plan.getByRole('region',{name:'任务执行记录',exact:true}).locator('pre').waitFor();
 assert.match(await plan.getByRole('region',{name:'任务执行记录',exact:true}).innerText(),/login-run.*日志0/s);
 await plan.locator('.team-task-row').nth(2).click();
 const review=plan.getByRole('article',{name:'任务详情 测试与审查',exact:true});await review.waitFor();
 assert.match(await review.innerText(),/Kimi.*接口对接/s);assert.equal(await plan.getByRole('region',{name:'任务执行记录',exact:true}).count(),0);
 await plan.evaluate(()=>window.conversationMock.failExecution=true);
 await plan.getByRole('button',{name:'暂停项目调度',exact:true}).click();
 await plan.getByRole('alert').filter({hasText:'执行控制未确认'}).waitFor();
 assert.equal(await plan.getByRole('button',{name:'继续项目调度',exact:true}).count(),0,'failed pause cannot invent a paused state');
 await plan.evaluate(()=>window.conversationMock.failExecution=false);
 await plan.getByRole('button',{name:'暂停项目调度',exact:true}).click();
 await plan.getByRole('button',{name:'继续项目调度',exact:true}).waitFor();
 await plan.getByText('项目调度已暂停。继续调度会执行已就绪的任务。',{exact:true}).waitFor();
 await plan.getByRole('button',{name:'继续项目调度',exact:true}).click();
 await plan.getByRole('button',{name:'暂停项目调度',exact:true}).waitFor();
 assert.equal((await calls(plan,'ma_conversation_control')).length,0,'engineering controls use project scheduler, not chat pause');
 assert.equal((await calls(plan,'ma_start_project')).length,1);
 await plan.getByRole('button',{name:'回到相关聊天',exact:true}).click();
 await plan.getByRole('region',{name:'项目分工',exact:true}).waitFor();
 assert.match(await editor(plan).inputValue(),/保留当前草稿/);
 await tab(plan,'团队聊天').focus();await plan.keyboard.press('ArrowRight');
 assert.equal(await tab(plan,'任务执行').getAttribute('aria-selected'),'true');
 await plan.waitForFunction(()=>document.activeElement?.getAttribute('aria-label')==='任务执行');
 await plan.keyboard.press('Home');await plan.waitForFunction(()=>document.activeElement?.getAttribute('aria-label')==='团队聊天');assert.equal(await tab(plan,'团队聊天').getAttribute('aria-selected'),'true');
 const chat=await fresh('natural=1&empty=1');
 await editor(chat).fill('@Codex');
 assert.equal(await chat.getByRole('button',{name:'生成分工',exact:true}).isDisabled(),true,'a mention alone cannot start a plan');
 assert.equal(await chat.getByRole('button',{name:'发送消息',exact:true}).isEnabled(),true);
 await tab(chat,'任务执行').click();await tab(chat,'团队聊天').click();
 await editor(chat).fill('你们有哪些功能');await editor(chat).press('Enter');
 await chat.waitForFunction(()=>window.conversationMock.calls.some(c=>c.command==='ma_conversation_send'));
 assert.equal((await calls(chat,'ma_conversation_send')).at(-1).args.input.mode,'discussion');
 await editor(chat).fill('登录页的需求');await chat.getByRole('button',{name:'生成分工',exact:true}).click();
 await chat.waitForFunction(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_send').length===2);
 assert.equal((await calls(chat,'ma_conversation_send')).at(-1).args.input.mode,'execution_plan');
 await editor(chat).fill('hi');await editor(chat).press('Enter');
 await chat.waitForFunction(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_send').length===3);
 assert.equal((await calls(chat,'ma_conversation_send')).at(-1).args.input.mode,'discussion','generate plan never becomes a sticky input mode');
 const history=await fresh('evidence=1&logs-only=1&launch-mode=1');
 await history.evaluate(()=>{
   const mock=window.conversationMock;
   const [old,latest]=mock.snapshot('A').tasks;
   old.status='failed'; latest.status='done'; latest.updatedAt=Date.now();
   const chat=mock.chats.A, failedPlan=chat.rounds[0];
   failedPlan.status='needs_decision'; failedPlan.stopReason='分工格式校验失败';
   chat.executionPlans=[];
   chat.messages.push({...chat.messages[0],id:'recent-topic',body:'最新讨论',createdAt:Date.now()});
   chat.rounds.push({...failedPlan,id:'recent-round',userMessageId:'recent-topic',mode:'discussion',status:'running',stopReason:null,createdAt:Date.now(),updatedAt:Date.now()});
 });
 await history.getByText('分工未生成，开发尚未开始',{exact:true}).waitFor();
 await history.locator('[data-message-id="m1"]').getByText('分工格式校验失败',{exact:true}).waitFor();
 await tab(history,'任务执行').click();
 assert.equal(await history.locator('.team-task-detail').count(),0,'old failures remain collapsed');
 assert.match(await history.locator('.team-task-row').first().innerText(),/接口对接/);
 await history.getByText(/团队聊天 · 正在回复/).waitFor();
 await history.evaluate(()=>window.conversationMock.chats.A.rounds.at(-1).status='completed');
 await history.getByText(/团队聊天 · 回复结束/).waitFor();
 await history.locator('.team-task-row').first().click();
 await history.getByRole('button',{name:'查看记录',exact:true}).click();
 const records=history.getByRole('region',{name:'任务执行记录',exact:true});
 await records.getByText('尝试2日志0',{exact:false}).waitFor();
 await records.locator('.team-run-launch-mode').getByText(/实际：Approve for me.*原因：已授权的可写任务继承管理 Runtime/).waitFor();
 await records.getByLabel('执行尝试').selectOption('evidence-1');
 await records.getByText('尝试1日志0',{exact:false}).waitFor();
 assert.equal(await records.locator('.team-run-launch-mode').count(),0);
 assert.deepEqual((await calls(history,'ma_run_log_history')).map(item=>item.args.runId),['evidence-2','evidence-1']);
 await records.screenshot({path:`${out}/older-run-logs.png`});
 await history.getByRole('button',{name:'查看本轮聊天',exact:true}).click();
 await history.getByText('最新讨论',{exact:true}).waitFor();
 const scroll=await fresh();await scroll.locator('.conversation-transcript').evaluate(node=>node.scrollTop=0);
 const top=await scroll.locator('.conversation-transcript').evaluate(node=>node.scrollTop);
 await tab(scroll,'任务执行').click();await tab(scroll,'团队聊天').click();
 assert.equal(await scroll.locator('.conversation-transcript').evaluate(node=>node.scrollTop),top,'view switching preserves chat position');
 await scroll.getByRole('combobox',{name:'协作项目',exact:true}).selectOption('B');
 await scroll.waitForFunction(()=>document.querySelector('.conversation-members-trigger')?.textContent.includes('成员 1'));
 assert.equal(await tab(scroll,'团队聊天').getAttribute('aria-selected'),'true');
 await tab(scroll,'任务执行').click();assert.equal(await scroll.locator('.team-task-row').count(),0,'no cross-project task leakage');
 for(const theme of ['dark','light'])for(const width of [680,390,320]){
  const p=await fresh(`plan=pending&theme=${theme}`,width,width===680?980:844);
  await p.getByRole('button',{name:'确认并开始执行',exact:true}).waitFor();
  await p.screenshot({path:`${out}/chat-${theme}-${width}.png`});
  assert.equal(await p.evaluate(()=>document.body.scrollWidth>innerWidth),false);
  await p.getByRole('button',{name:'确认并开始执行',exact:true}).click();
  await p.getByRole('heading',{name:'登录页面',exact:true}).waitFor();
  assert.equal(await p.evaluate(()=>[...document.querySelectorAll('.team-tasks-panel,.team-task-row,.team-task-detail')].some(node=>node.scrollWidth>node.clientWidth+1)),false);
  assert.equal(await p.evaluate(()=>[...document.images].every(img=>img.complete&&img.naturalWidth>0)),true);
  await p.screenshot({path:`${out}/tasks-${theme}-${width}.png`});
 }
 const short=await fresh('empty=1',320,420);
 await editor(short).evaluate(node=>node.style.height='999px');
 assert.equal(await short.evaluate(()=>document.querySelector('.conversation-footer').getBoundingClientRect().bottom<=innerHeight+1),true,'resizing in a short window keeps send controls on screen');
 assert.deepEqual(errors,[]);
 console.log('PASS team work views: real tab separation, draft and scroll retention, one-shot planning, full plan contract, explicit/idempotent approval, task selection/dependencies/evidence, truthful progress, scheduler pause/resume/failure, related chat, keyboard, project isolation, dark/light/320px.');
}finally{for(const context of contexts)await context.close();await browser.close();}
