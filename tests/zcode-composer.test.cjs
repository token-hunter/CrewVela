const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/components/zcodeComposer.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);
const catalog = {skills:{items:[
  {id:'user',name:'same',path:'C:\\Users\\demo\\skill space (x)\\SKILL.md',scope:'user',enabled:true},
  {id:'workspace',name:'same',path:'F:\\project\\SKILL.md',scope:'workspace',enabled:true},
  {id:'label',name:'中文[技能]',path:'C:\\skill space (x)\\SKILL.md',scope:'user',enabled:true},
  {id:'relative',name:'bad',path:'relative/SKILL.md',scope:'user',enabled:true}
]},plugins:{items:[
  {pluginId:'valid@market',name:'中文[插件]',enabled:true,conflictingPluginIds:[]},
  {pluginId:'conflict@market',name:'conflict',enabled:true,conflictingPluginIds:['other']},
  {pluginId:'bad)@market',name:'bad',enabled:true,conflictingPluginIds:[]},
  {pluginId:'off@market',name:'off',enabled:false,conflictingPluginIds:[]}
]},commands:{items:[{name:'workflow'},{name:'compact'},{name:'custom'}]}};
test('submission failure preserves newer drafts and treats unknown acceptance as review, never automatic replay', async()=>{
  const {rejectedSubmissionDisposition}=await modulePromise;
  assert.equal(rejectedSubmissionDisposition('ZCODE_INPUT_REJECTED: invalid attachment',false),'restore');
  assert.equal(rejectedSubmissionDisposition('ZCODE_INPUT_REJECTED: route changed',true),'preserve');
  for(const hasDraft of [false,true]) {
    assert.equal(rejectedSubmissionDisposition('ZCODE_INPUT_UNCERTAIN: ACK timed out',hasDraft),'review');
    assert.equal(rejectedSubmissionDisposition('IPC closed',hasDraft),'review');
  }
});
test('native catalog selection preserves identity, priority, conflicts and canonical escaping',async()=>{
  const {composerItems}=await modulePromise;
  const items=composerItems(catalog);
  assert.equal(items.filter(x=>x.name==='same').length,1);
  assert.equal(items.find(x=>x.name==='same').id,'workspace');
  assert.equal(items.find(x=>x.id==='label').canonical,'[$中文\\[技能\\]](C:\\\\skill space (x)\\\\SKILL.md)');
  assert.equal(items.find(x=>x.id==='valid@market').canonical,'[@中文\\[插件\\]](plugin://valid@market)');
  assert.equal(items.find(x=>x.id==='conflict@market').disabled,true);
  assert.ok(!items.some(x=>['bad','off','side','btw'].includes(x.name)));
  // 内置命令(goal/compact/plan 等)不再被过滤:随消息文本交由 runtime 执行。
  assert.ok(items.some(x=>x.name==='compact'));
});
test('workflow remains first for selected and typed commands, without losing task text',async()=>{
  const {composerItems,composeMessage}=await modulePromise;
  const items=composerItems(catalog),skill=items.find(x=>x.name==='same'),workflow=items.find(x=>x.name==='workflow');
  assert.equal(composeMessage('任务',[skill,workflow]),`/workflow ${skill.canonical} 任务`);
  assert.equal(composeMessage('/workflow 任务',[skill]),`/workflow ${skill.canonical} 任务`);
  assert.equal(composeMessage('',[]),'');
});
test('malformed catalogs fail explicitly; per-kind errors stay visible',async()=>{
  const {normalizeCatalog}=await modulePromise;
  assert.throws(()=>normalizeCatalog(null),/格式/);
  assert.throws(()=>normalizeCatalog({...catalog,skills:{items:[null]}}),/skills/);
  assert.equal(normalizeCatalog({...catalog,plugins:{items:[],error:'query failed'}}).plugins.error,'query failed');
});
test('disconnected send failures restore drafts; accepted input is never queued twice',async()=>{
  const {submissionOutcome}=await modulePromise;
  const previous=new Set([1]), rows=[{rowId:1,kind:'userInput'}];
  assert.equal(submissionOutcome({connected:false,busy:false,error:'Agent exited',rows},previous),'restore');
  assert.equal(submissionOutcome({connected:true,busy:false,error:'command rejected',rows},previous),'restore');
  assert.equal(submissionOutcome({connected:false,busy:false,error:'model failed',rows:[...rows,{rowId:2,kind:'userInput'}]},previous),'accepted');
  assert.equal(submissionOutcome({connected:true,busy:true,error:null,rows},previous),'pending');
});
test('visible slash commands route to typed v4 commands; everything else stays sendText',async()=>{
  const {parseVisibleSlashCommand}=await modulePromise;
  // goal/target → sendGoalCommand,objective 去掉 /goal 前缀与 replace 动作。
  assert.deepEqual(parseVisibleSlashCommand('/goal 回复1-10'),{kind:'sendGoalCommand',objective:'回复1-10',displayText:'/goal 回复1-10'});
  assert.deepEqual(parseVisibleSlashCommand('/TARGET  ship it  '),{kind:'sendGoalCommand',objective:'ship it',displayText:'/TARGET  ship it'});
  assert.deepEqual(parseVisibleSlashCommand('/goal replace 新目标'),{kind:'sendGoalCommand',objective:'新目标',displayText:'/goal replace 新目标'});
  assert.deepEqual(parseVisibleSlashCommand('/goal resume'),{kind:'resumeGoal',displayText:'/goal resume'});
  assert.deepEqual(parseVisibleSlashCommand('/goal'),{kind:'emptyGoal',displayText:'/goal'});
  assert.deepEqual(parseVisibleSlashCommand('/goal pause'),{kind:'unsupportedGoal',action:'pause',displayText:'/goal pause'});
  // compact/compress → compact 命令;大小写不敏感。
  assert.deepEqual(parseVisibleSlashCommand('/compact'),{kind:'compact',displayText:'/compact'});
  assert.deepEqual(parseVisibleSlashCommand('/Compress 上下文'),{kind:'compact',displayText:'/Compress 上下文'});
  // /plan 快捷方式:空任务只切模式,带任务随 sendText 声明 planEnabled。
  assert.deepEqual(parseVisibleSlashCommand('/plan 重构登录'),{kind:'planShortcut',task:'重构登录',displayText:'/plan 重构登录'});
  assert.deepEqual(parseVisibleSlashCommand('/plan'),{kind:'planShortcut',task:'',displayText:'/plan'});
  assert.equal(parseVisibleSlashCommand('/plan 任务',1)?.kind,'unsupportedPlanShortcut');
  // 附件/引用上下文使 goal/compact 退回字面 sendText(无附件语义)。
  assert.equal(parseVisibleSlashCommand('/goal x',1),null);
  assert.equal(parseVisibleSlashCommand('/compact',1),null);
  // 普通命令与技能、非命令文本、非法形态都保持 sendText。
  assert.equal(parseVisibleSlashCommand('/init'),null);
  assert.equal(parseVisibleSlashCommand('/workflow 全量检查'),null);
  assert.equal(parseVisibleSlashCommand('回复1-10'),null);
  assert.equal(parseVisibleSlashCommand('/'),null);
});
