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
 const draftImage=staged.getByRole('button',{name:'预览图片 设计截图.png',exact:true});
 const checkImage = locator => locator.evaluate(img => img.complete && img.naturalWidth === 360);
 await draftImage.locator('img').waitFor();
 await attachmentPage.waitForFunction(()=>document.querySelector('.conversation-draft-attachments img')?.naturalWidth === 360);
 await attachmentPage.locator('.conversation-composer').screenshot({path:`${out}/draft-image-attachment.png`});
 await draftImage.click();
 const imagePreview=attachmentPage.getByRole('dialog',{name:'图片预览 设计截图.png',exact:true});
 await imagePreview.getByRole('img',{name:'设计截图.png'}).waitFor();
 await attachmentPage.waitForFunction(()=>document.querySelector('.conversation-image-dialog img')?.naturalWidth === 360);
 assert.ok(await checkImage(draftImage.locator('img')));
 assert.ok((await imagePreview.locator('img').boundingBox()).width > (await draftImage.boundingBox()).width);
 await imagePreview.screenshot({path:`${out}/image-preview-dialog.png`});
 await attachmentPage.keyboard.press('Escape');
 await imagePreview.waitFor({state:'hidden'});
 assert.equal(await draftImage.evaluate(button => button === document.activeElement),true);
 await staged.getByRole('button',{name:'移除附件 设计截图.png',exact:true}).click();
 assert.equal(await staged.getByRole('button',{name:'预览图片 设计截图.png',exact:true}).count(),0);
 await attachmentPage.getByRole('button',{name:'添加附件',exact:true}).click();
 await staged.getByRole('button',{name:'移除附件 需求说明.md',exact:true}).click();
 assert.equal(await staged.getByRole('button').count(),2);
 assert.equal(await editor(attachmentPage).inputValue(),'');
 const attachmentSend=attachmentPage.getByRole('button',{name:'发送消息',exact:true});
 await attachmentSend.click();
 const sentAttachment=attachmentPage.getByLabel('消息附件',{exact:true}).getByRole('button',{name:'预览图片 设计截图.png',exact:true});
 await sentAttachment.waitFor();
 await sentAttachment.click();
 const attachmentCalls=await calls(attachmentPage,'ma_conversation_send');
 assert.equal(attachmentCalls.length,1);
 assert.equal(attachmentCalls[0].args.input.attachmentIds.length,1);
 assert.equal(attachmentCalls[0].args.input.body,'');
 await imagePreview.getByRole('img',{name:'设计截图.png'}).waitFor();
 await imagePreview.getByRole('button',{name:'关闭图片预览',exact:true}).click();
 const sentPreview=(await calls(attachmentPage,'ma_conversation_preview_image')).find(item=>item.args.source.kind==='message');
 assert.equal(sentPreview.args.source.attachmentId,attachmentCalls[0].args.input.attachmentIds[0]);
 assert.equal((await calls(attachmentPage,'ma_conversation_open_attachment')).length,0);
 assert.equal(await staged.count(),0);

 await attachmentPage.getByRole('button',{name:'添加附件',exact:true}).click();
 await staged.getByRole('button',{name:'移除附件 设计截图.png',exact:true}).click();
 await attachmentSend.click();
 await attachmentPage.getByLabel('消息附件',{exact:true}).getByRole('button',{name:'需求说明.md',exact:true}).click();
 assert.equal((await calls(attachmentPage,'ma_conversation_open_attachment')).length,1);

 const videoPage=await fresh('natural=1&video=1', 420);
 const pickVideo=videoPage.getByRole('button',{name:'添加附件',exact:true});
 await pickVideo.hover();
 const formats=videoPage.getByRole('tooltip').filter({hasText:'视频 · MP4、MOV、WEBM'});
 await formats.waitFor();
 assert.match(await formats.innerText(),/图片 · PNG、JPG、JPEG、WEBP、GIF/);
 assert.match(await formats.innerText(),/文档 · PDF、DOCX、TXT、MD、CSV/);
 const hintBounds=await formats.boundingBox();
 assert.ok(hintBounds.x>=0 && hintBounds.x+hintBounds.width<=420);
 await pickVideo.click();
 const videoDraft=videoPage.getByRole('button',{name:'移除附件 操作演示.mp4',exact:true});
 await videoDraft.waitFor();
 const [picker]=await calls(videoPage,'plugin:dialog|open');
 assert.deepEqual(picker.args.options.filters.find(filter=>filter.name==='视频').extensions,['mp4','mov','webm']);
 assert.ok(picker.args.options.filters[0].extensions.includes('mp4'));
 const draftPreview=videoPage.getByLabel('待发送附件',{exact:true}).getByRole('button',{name:'预览视频 操作演示.mp4'});
 await draftPreview.click();
 const player=videoPage.getByRole('dialog',{name:'视频预览 操作演示.mp4'});
 const video=player.locator('video[controls]');
 await video.waitFor();
 assert.deepEqual((await calls(videoPage,'ma_conversation_preview_video'))[0].args.source,{kind:'draft',path:'/fixture/操作演示.mp4'});
 assert.equal((await calls(videoPage,'ma_conversation_stage_attachments')).length,0);
 await video.evaluate(element=>{element.muted=true;return element.play();});
 await videoPage.waitForFunction(()=>{const v=document.querySelector('.conversation-video-dialog video');return v?.videoWidth===64 && v.currentTime>0.05;});
 await video.evaluate(element=>{element.pause();element.currentTime=0.7;});
 await videoPage.waitForFunction(()=>{const v=document.querySelector('.conversation-video-dialog video');return v && !v.seeking && Math.abs(v.currentTime-0.7)<0.05;});
 await player.screenshot({path:`${out}/draft-video-preview.png`});
 await videoPage.keyboard.press('Escape');
 await player.waitFor({state:'detached'});
 assert.ok(await draftPreview.evaluate(element=>element===document.activeElement));
 await videoDraft.click();
 await videoDraft.waitFor({state:'detached'});
 await pickVideo.click();
 await videoDraft.waitFor();
 await videoPage.getByRole('button',{name:'发送消息',exact:true}).click();
 await videoPage.getByLabel('消息附件',{exact:true}).getByRole('button',{name:'预览视频 操作演示.mp4',exact:true}).click();
 await video.waitFor();
 const [videoSend]=await calls(videoPage,'ma_conversation_send');
 const [,videoPreview]=await calls(videoPage,'ma_conversation_preview_video');
 assert.equal(videoSend.args.input.body,'');
 assert.equal(videoSend.args.input.attachmentIds.length,1);
 assert.equal(videoPreview.args.source.kind,'message');
 assert.equal(videoPreview.args.source.attachmentId,videoSend.args.input.attachmentIds[0]);
 await player.getByRole('button',{name:'用系统播放器打开',exact:true}).click();
 assert.deepEqual((await calls(videoPage,'ma_conversation_open_video'))[0].args.source,videoPreview.args.source);
 assert.equal((await calls(videoPage,'ma_conversation_preview_image')).length,0);
 await player.getByRole('button',{name:'关闭视频预览',exact:true}).click();

 const brokenVideo=await fresh('natural=1&video=1&broken-video=1', 320);
 await brokenVideo.getByRole('button',{name:'添加附件',exact:true}).click();
 await brokenVideo.getByRole('button',{name:'预览视频 操作演示.mp4',exact:true}).click();
 await brokenVideo.getByRole('alert').getByText(/内置播放器无法播放/).waitFor();
 await brokenVideo.getByRole('button',{name:'用系统播放器打开',exact:true}).click();
 assert.equal((await calls(brokenVideo,'ma_conversation_open_video'))[0].args.source.kind,'draft');

 const brokenPage=await fresh('natural=1&attachments=1&broken-image=1', 360);
 await brokenPage.getByRole('button',{name:'添加附件',exact:true}).click();
 await brokenPage.getByRole('button',{name:'无法预览 设计截图.png',exact:true}).waitFor();
 await brokenPage.getByRole('button',{name:'移除附件 设计截图.png',exact:true}).click();

 for (const theme of ['dark', 'light']) {
  const p = await fresh(`markdown=1&activity=1&theme=${theme}`, 1800);
  const layout = p.locator('.conversation-delegation-layout');
  const user = p.locator('[data-message-id="a0"]');
  const agent = p.locator('[data-message-id="m90"]');
  await agent.locator('.conversation-message-body').waitFor();
   const messageMetrics = (message) => message.evaluate(node => {
    const body = getComputedStyle(node.querySelector('.conversation-message-body')).fontSize;
    const avatar = node.querySelector('.conversation-user-avatar');
    if (avatar) {
     const { width, height } = getComputedStyle(avatar);
     return { avatar: [width, height], body };
    }
    const sender = node.querySelector('.conversation-message-content > header strong');
    return { sender: getComputedStyle(sender).fontSize, body };
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
   const wideUser = await messageMetrics(user);
   const wideAgent = await messageMetrics(agent);
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
    assert.deepEqual(await messageMetrics(user), wideUser, `user message dimensions at ${width}px`);
    assert.deepEqual(await messageMetrics(agent), wideAgent, `agent typography at ${width}px`);
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
   assert.deepEqual(wideUser, {avatar:['28px','28px'], body:'14px'});
   assert.deepEqual(wideAgent, {sender:'14px', body:'14px'});
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
 const managed = await fresh('natural=1&empty=1&managed-test=1');
 await editor(managed).fill('开发并交付可运行的贪吃蛇游戏');
 const startManaged = managed.getByRole('button',{name:'全自动托管',exact:true});
 managed.once('dialog',dialog=>dialog.dismiss());
 await startManaged.click();
 assert.equal((await calls(managed,'ma_conversation_send')).length,0);
 await Promise.all([managed.waitForEvent('dialog').then(async confirmation=>{
   const message = confirmation.message();
   await confirmation.accept();
   assert.match(message, /main.*\/fixture\/todo-app/s);
 }),startManaged.click()]);
 await managed.getByText('全自动托管 · 正在生成分工',{exact:true}).waitFor();
 const sent = (await calls(managed,'ma_conversation_send')).at(-1).args.input;
 assert.equal(sent.managed,true);
 assert.equal(sent.nativeCapabilities,false);
 assert.equal(sent.mode,'execution_plan');
 assert.equal(sent.chatDispatch,undefined);
 assert.deepEqual(sent.participantAgentIds,['claude','codex','kimi']);
 await managed.evaluate(()=>{
   const chat = window.conversationMock.chats.A;
   const round = chat.rounds.at(-1);
   round.status = 'completed';
   const delivery = {id:'managed-result',projectId:'A',roundId:round.id,messageId:'managed-proposal',
     summary:'贪吃蛇交付',tasks:[],status:'applied',taskIds:['t-build'],createdAt:Date.now(),
     managed:{target:round.managedTarget,status:'running',summary:null,commit:null,verificationCommand:null}};
   chat.messages.push({...chat.messages[0],id:delivery.messageId,roundId:round.id,senderType:'agent',agentId:'claude',body:'',kind:'plan',phase:'plan',createdAt:Date.now()});
   chat.executionPlans.push(delivery);
 });
 const outcome = managed.getByRole('region',{name:'全自动托管结果',exact:true});
 await outcome.getByRole('heading',{name:'全自动托管中',exact:true}).waitFor();
 await managed.evaluate(()=>Object.assign(window.conversationMock.chats.A.executionPlans[0].managed,{
   status:'completed',summary:'成品通过启动和计分测试',commit:'c4cb7a9e',verificationCommand:'node --test tests/acceptance.test.js',
 }));
 await outcome.getByRole('heading',{name:'成品已交付',exact:true}).waitFor();
 assert.equal(await outcome.getByText('成品通过启动和计分测试',{exact:true}).count(),0);
 assert.equal(await outcome.getByText('node --test tests/acceptance.test.js',{exact:true}).count(),0);
 await outcome.getByRole('button',{name:'查看启动说明',exact:true}).click();
 assert.deepEqual((await calls(managed,'workspace_reveal_path')).at(-1).args,{root:'/fixture/todo-app',relativePath:'README.md'});
 await outcome.getByRole('button',{name:'启动项目',exact:true}).click();
 const previewConfig = managed.getByRole('dialog',{name:'配置项目启动',exact:true});
 await previewConfig.getByLabel('启动程序').fill('node');
 await previewConfig.getByLabel('启动参数（每行一个）').fill('server.js');
 await previewConfig.getByRole('checkbox',{name:/PORT=0/}).check();
 await previewConfig.getByRole('button',{name:'保存并启动',exact:true}).click();
 await outcome.getByText('正在等待本地服务响应…',{exact:true}).waitFor();
 assert.deepEqual((await calls(managed,'ma_project_preview_save')).at(-1).args.config,{program:'node',args:['server.js'],workingDirectory:'.',url:null});
 await managed.evaluate(()=>Object.assign(window.conversationMock.preview,{state:'running',url:'http://127.0.0.1:49201/'}));
 await outcome.getByRole('button',{name:'打开网页',exact:true}).waitFor();
 await managed.waitForFunction(()=>window.conversationMock.previewBrowsed.length === 1);
 await outcome.getByRole('button',{name:'打开网页',exact:true}).click();
 assert.deepEqual(await managed.evaluate(()=>window.conversationMock.previewBrowsed),['http://127.0.0.1:49201/','http://127.0.0.1:49201/']);
 await outcome.getByRole('button',{name:'停止项目',exact:true}).click();
 await outcome.getByRole('button',{name:'启动项目',exact:true}).waitFor();
 await managed.evaluate(()=>window.conversationMock.previewVersion='current-version');
 managed.once('dialog', dialog=>dialog.dismiss());
 await outcome.getByRole('button',{name:'启动项目',exact:true}).click();
 await managed.waitForFunction(()=>window.conversationMock.calls.filter(call=>call.command==='ma_project_preview_start').length === 2);
 managed.once('dialog', dialog=>dialog.accept());
 await outcome.getByRole('button',{name:'启动项目',exact:true}).click();
 await outcome.getByText('正在等待本地服务响应…',{exact:true}).waitFor();
 assert.equal((await calls(managed,'ma_project_preview_start')).at(-1).args.confirmedVersion,'current-version');
 await managed.evaluate(()=>Object.assign(window.conversationMock.preview,{state:'failed',error:'项目进程已退出（7）',logs:'测试启动日志'}));
 await outcome.getByText('项目进程已退出（7）',{exact:true}).waitFor();
 await outcome.getByText('启动日志',{exact:true}).click();
 await outcome.getByText('测试启动日志',{exact:true}).waitFor();
 assert.equal((await calls(managed,'ma_project_preview_save')).length,1,'saved project can restart without reconfiguration');
 await managed.evaluate(() => {
   const mock = window.conversationMock;
   const original = mock.chats.A.executionPlans[0];
   mock.chats.A.executionPlans[0] = { ...original, id: 'managed-demo-result', managed: { ...original.managed, demo: { state: 'preparing' } } };
   mock.preview = { state: 'starting' };
   mock.previewVersion = '';
 });
 await outcome.getByRole('button',{name:'正在准备 Demo…',exact:true}).waitFor();
 assert.equal(await outcome.getByRole('button',{name:'打开 Demo',exact:true}).count(),0);
 await managed.evaluate(() => {
   const mock = window.conversationMock;
   mock.chats.A.executionPlans[0].managed.demo = { state: 'ready' };
   mock.preview = { state: 'running', url: 'http://127.0.0.1:49202/' };
 });
 const openDemo = outcome.getByRole('button',{name:'打开 Demo',exact:true});
 const stopDemo = outcome.getByRole('button',{name:'停止 Demo',exact:true});
 await openDemo.waitFor();
 await outcome.getByRole('status').getByText('运行中',{exact:true}).waitFor();
 assert.equal(await outcome.locator('.project-preview-logs').getAttribute('open'),null);
 for (const width of [680,480,320]) {
   await managed.setViewportSize({width,height:980});
   const [open, stop] = await Promise.all([openDemo.boundingBox(),stopDemo.boundingBox()]);
   assert.ok(Math.abs(open.width-stop.width)<1 && Math.abs(open.height-stop.height)<1 && open.y===stop.y,`Demo controls share equal cells at ${width}px`);
   assert.ok(await outcome.evaluate(card=>card.scrollWidth<=card.clientWidth),'delivery card fits its container');
   if(width===320) {
     const [address, logs] = await Promise.all([outcome.locator('.project-preview-url').boundingBox(),outcome.locator('.project-preview-logs').boundingBox()]);
     assert.ok(address.height<22 && logs.y>=address.y+address.height,'narrow footer keeps the address on one line');
   }
 }
 await outcome.screenshot({path:`${out}/managed-delivery-demo-narrow.png`});
 await managed.setViewportSize({width:680,height:980});
 const actionColors = () => outcome.locator('.project-preview-buttons').evaluate(actions => Array.from(actions.querySelectorAll('button'), button => {
   const style=getComputedStyle(button);
   return [style.backgroundColor,style.borderTopColor,style.color];
 }));
 const clay = await actionColors();
 await managed.evaluate(()=>{document.documentElement.dataset.appearanceAccent='purple';});
 const purple = await actionColors();
 assert.notEqual(purple[0][0],clay[0][0],'primary fill follows accent changes');
 assert.notEqual(purple[1][1],clay[1][1],'stop outline follows accent changes');
 assert.equal(purple[0][0],purple[1][1],'fill and outline use the same active accent');
 await managed.evaluate(()=>{document.documentElement.dataset.theme='light';});
 const light = await actionColors();
 assert.notEqual(light[0][0],purple[0][0],'light theme uses the matching light accent');
 assert.equal(light[0][0],light[1][1],'light fill and outline stay synchronized');
 await managed.evaluate(()=>Object.assign(document.documentElement.dataset,{theme:'dark',appearanceAccent:'clay'}));
 await outcome.screenshot({path:`${out}/managed-delivery-demo.png`});
 await openDemo.click();
 await managed.waitForFunction(() => window.conversationMock.previewBrowsed.at(-1) === 'http://127.0.0.1:49202/');
 await stopDemo.click();
 assert.equal((await calls(managed,'ma_project_preview_stop')).at(-1).args.planId,'managed-demo-result');
 await openDemo.click();
 await outcome.getByRole('button',{name:'正在准备 Demo…',exact:true}).waitFor();
 assert.equal((await calls(managed,'ma_project_preview_start')).at(-1).args.planId,'managed-demo-result');
 await managed.evaluate(() => { window.conversationMock.preview = { state: 'running', url: 'http://127.0.0.1:49203/' }; });
 await managed.waitForFunction(() => window.conversationMock.previewBrowsed.at(-1) === 'http://127.0.0.1:49203/');
 assert.equal(await managed.getByRole('dialog',{name:'配置项目启动',exact:true}).count(),0);
 assert.equal((await calls(managed,'ma_project_preview_save')).length,1,'automatic preview never saves a manual configuration');
 assert.equal((await calls(managed,'ma_conversation_execute_plan')).length,0);
 await managed.evaluate(() => {
   const mock = window.conversationMock;
   const { agents, tasks } = mock.snapshot('A');
   const { runHistory } = mock;
   const build = tasks.find(task => task.id === 't-build');
   const review = tasks.find(task => task.id === 't-review');
   Object.assign(agents.find(agent => agent.id === 'codex'), { name: 'OpenCode 开发', runtimeKind: 'opencode' });
   Object.assign(agents.find(agent => agent.id === 'kimi'), { name: 'Pi 审查', runtimeKind: 'pi' });
   Object.assign(build, { status: 'done', reviewerAgentId: null });
   Object.assign(review, { status: 'done' });
   const integration = { ...build, id: 't-delivery', title: '联调 Demo', assigneeAgentId: 'pi-integrator' };
   agents.push({ ...agents.find(agent => agent.id === 'kimi'), id: 'pi-integrator', name: 'Pi 联调', role: 'integrator' });
   tasks.push(integration);
   const managedPlan = mock.chats.A.executionPlans[0];
   managedPlan.taskIds = [build.id, review.id, integration.id];
   managedPlan.tasks = [build, review, integration];
   const at = Date.now();
   for (const attempt of [2, 1]) runHistory.push({
     id: `managed-attempt-${attempt}`, projectId: 'A', taskId: 't-build', agentId: 'codex', attempt,
     status: attempt === 2 ? 'succeeded' : 'failed', outputSummary: `运行 ${attempt}`, error: null,
     createdAt: at + attempt, startedAt: at, finishedAt: at + attempt,
     sessionLocator: { sessionId: 'native-opencode', turnId: `turn-${attempt}`, managedTurn: 'completed' },
   });
   const successfulBuild = runHistory.find(run => run.id === 'managed-attempt-2');
   for (const task of [review, integration]) runHistory.push({
     ...successfulBuild, id: `managed-run-${task.id}`, taskId: task.id, agentId: task.assigneeAgentId, attempt: 1,
     sessionLocator: { sessionId: `native-${task.id}`, managedTurn: 'completed' },
   });
 });
 await outcome.getByRole('button',{name:'查看任务执行',exact:true}).click();
 const deliveryDetails = managed.getByRole('region',{name:'交付详情',exact:true});
 const developmentRecord = deliveryDetails.getByRole('button',{name:/OpenCode 开发.*接口对接/});
 const reviewRecord = deliveryDetails.getByRole('button',{name:/Pi 审查.*测试与审查/});
 const integrationRecord = deliveryDetails.getByRole('button',{name:/Pi 联调.*联调 Demo/});
 await developmentRecord.waitFor();
 const acceptanceSummary = deliveryDetails.getByText('成品通过启动和计分测试',{exact:true});
 assert.equal(await acceptanceSummary.isVisible(),false,'long summary starts collapsed');
 await deliveryDetails.getByText('查看验收详情',{exact:true}).click();
 await acceptanceSummary.waitFor();
 await deliveryDetails.getByText('查看验收详情',{exact:true}).click();
 const memberDeliveries = deliveryDetails.getByRole('group',{name:'成员交付明细',exact:true});
 assert.equal(await memberDeliveries.getByRole('button').count(),3,'只显示这轮托管绑定的任务');
 assert.match(await developmentRecord.innerText(),/验收通过/);
 assert.match(await developmentRecord.innerText(),/首轮失败.*重试成功/s);
 assert.match(await reviewRecord.innerText(),/验收通过/);
 assert.match(await integrationRecord.innerText(),/验收通过/);
 await managed.setViewportSize({width:320,height:980});
 assert.equal(await deliveryDetails.evaluate(card => card.scrollWidth <= card.clientWidth),true,'交付详情适应窄面板');
 await deliveryDetails.screenshot({path:`${out}/managed-delivery-members-narrow.png`});
 await managed.setViewportSize({width:680,height:980});
 await deliveryDetails.screenshot({path:`${out}/managed-delivery-members.png`});
 await developmentRecord.click();
 assert.equal(await tab(managed,'任务执行').getAttribute('aria-selected'),'true');
 await managed.getByRole('article',{name:'任务详情 接口对接',exact:true}).waitFor();
 const runRecords = managed.getByRole('region',{name:'任务执行记录',exact:true});
 await managed.waitForFunction(() => {
   const heading = document.querySelector('.team-task-records h3');
   const panel = document.querySelector('.team-tasks-panel');
   if (!heading || !panel) return false;
   const recordHeading = heading.getBoundingClientRect();
   const taskPanel = panel.getBoundingClientRect();
   return recordHeading.top >= taskPanel.top - 1 && recordHeading.bottom <= Math.min(taskPanel.bottom, innerHeight);
 });
 const openRun = runRecords.getByRole('button',{name:'查看原会话记录',exact:true});
 await openRun.click();
 await runRecords.getByLabel('执行尝试').selectOption('managed-attempt-1');
 await openRun.click();
 assert.deepEqual((await calls(managed,'fixture_open_session')).map(({args})=>[args.sessionId,args.original.projectId,args.original.runId,args.original.source]),
   [[null,'A','managed-attempt-2','managed_task'],[null,'A','managed-attempt-1','managed_task']]);
 await reviewRecord.click();
 await managed.getByRole('article',{name:'任务详情 测试与审查',exact:true}).waitFor();
 assert.match(await runRecords.innerText(),/managed-run-t-review/);
 await integrationRecord.click();
 await managed.getByRole('article',{name:'任务详情 联调 Demo',exact:true}).waitFor();
 assert.match(await runRecords.innerText(),/managed-run-t-delivery/);
 await managed.evaluate(() => {
   const mock = window.conversationMock;
   const agents = mock.snapshot('A').agents;
   agents.splice(agents.findIndex(agent => agent.id === 'pi-integrator'),1);
   mock.chats.A.executionPlans[0].managed.target.members = { 'pi-integrator': { runtime: 'pi', role: 'integrator' } };
 });
 const historicalRecord = deliveryDetails.getByRole('button',{name:/Pi.*历史成员.*联调 Demo/});
 await historicalRecord.click();
 const integrationDetail = managed.getByRole('article',{name:'任务详情 联调 Demo',exact:true});
 await integrationDetail.getByText('Pi（历史成员）',{exact:true}).waitFor();
 assert.match(await runRecords.innerText(),/managed-run-t-delivery/);
 await tab(managed,'团队聊天').click();
 await managed.evaluate(() => {
   const mock = window.conversationMock;
   Object.assign(mock.chats.A.executionPlans[0].managed, {
     status: 'failed', summary: '最终验收连续三次未通过', commit: null, verificationCommand: null,
   });
   mock.snapshot('A').tasks.find(task => task.id === 't-delivery').status = 'failed';
 });
 await outcome.getByText('最终验收连续三次未通过',{exact:true}).waitFor();
 assert.doesNotMatch(await outcome.getByRole('button',{name:/Pi.*历史成员.*联调 Demo/}).innerText(),/验收通过/,'进程回执成功不足以证明任务通过验收');
 assert.equal(await outcome.getByRole('button',{name:'启动项目',exact:true}).count(),0);
 await managed.evaluate(()=>{
   const chat = window.conversationMock.chats.A;
   chat.messages.push(
     {...chat.messages[0],id:'retry-1',taskId:'t-build',senderType:'system',kind:'status',status:'failed',body:'托管运行第一次失败'},
     {...chat.messages[0],id:'retry-2',taskId:'t-build',senderType:'system',kind:'status',status:'failed',body:'托管运行第二次失败'},
     {...chat.messages[0],id:'manual-error',taskId:'t-design',senderType:'system',kind:'status',status:'failed',body:'人工流程仍显示执行失败'},
   );
   chat.executionPlans[0].managed.summary = 'MCP config file not found: path/{mcpServers:{Authorization:Bearer fixture-secret}}';
 });
 await outcome.getByText(/Windows 未能正确传递 MCP 配置/).waitFor();
 await managed.getByText('人工流程仍显示执行失败',{exact:true}).waitFor();
 assert.equal(await managed.getByText(/托管运行第.次失败|fixture-secret/).count(),0);
 const managedBefore=await managed.evaluate(()=>structuredClone(window.conversationMock.chats.A));
 const expectedManagedIds=managedBefore.messages.map(message=>message.id).sort();
 const recordsMenu=managed.getByRole('button',{name:'讨论记录操作',exact:true});
 await recordsMenu.click();
 await managed.getByRole('menuitem',{name:'新建会话',exact:true}).click();
 await managed.waitForFunction(oldId=>window.conversationMock.chats.A.conversation.id!==oldId
   || document.querySelector('.conversation-error')?.textContent.includes('讨论记录已变化'),managedBefore.conversation.id);
 const newCall=(await calls(managed,'ma_conversation_new')).at(-1);
 assert.deepEqual([...newCall.args.input.expectedMessageIds].sort(),expectedManagedIds,'new chat validates hidden managed messages too');
 await outcome.waitFor({state:'detached'});
 assert.equal(await managed.locator('.conversation-message').count(),0);
 assert.deepEqual(await managed.evaluate(()=>window.conversationMock.archivedConversations[0]),managedBefore);
 await recordsMenu.click();
 await managed.getByRole('menuitem',{name:'历史会话',exact:true}).click();
 const archived=managed.getByRole('dialog',{name:'历史会话',exact:true});
 await archived.getByRole('button',{name:/开发并交付可运行的贪吃蛇游戏/}).click();
 await outcome.waitFor();
 assert.equal(await managed.getByText(/托管运行第.次失败/).count(),0);
 await recordsMenu.click();
 await managed.getByRole('menuitem',{name:'清空讨论记录',exact:true}).click();
 const clearManaged=managed.getByRole('dialog',{name:'清空讨论记录',exact:true});
 await clearManaged.getByRole('button',{name:'清空记录',exact:true}).click();
 await clearManaged.waitFor({state:'detached'});
 const clearCall=(await calls(managed,'ma_conversation_clear')).at(-1);
 assert.deepEqual([...clearCall.args.input.expectedMessageIds].sort(),expectedManagedIds,'clear uses the same snapshot');
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
 console.log('PASS team work views: draft and sent image previews, document attachments, real tab separation, draft and scroll retention, one-shot planning, full plan contract, explicit/idempotent approval, task selection/dependencies/evidence, truthful progress, scheduler pause/resume/failure, related chat, keyboard, project isolation, dark/light/320px.');
}finally{for(const context of contexts)await context.close();await browser.close();}
