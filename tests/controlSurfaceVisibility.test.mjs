import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
const sidebar=readFileSync(new URL('../src/components/ControlSidebar.tsx',import.meta.url),'utf8');
const control=readFileSync(new URL('../src/components/MultiAgentControl.tsx',import.meta.url),'utf8');
test('多智能体入口只挂载一个公共协作会话，不再混合普通快速总控',()=>{
 assert.equal(sidebar.match(/<MultiAgentControl/g)?.length,1);
 for(const retired of ['resolveUnifiedControlRoute','onQuickCommand','onQuickBroadcast','quickPermissionMode','GroupChatPanel']) assert.ok(!control.includes(retired),retired);
 assert.doesNotMatch(sidebar,/<QuickCommandCenter|executeQuickCommand|enqueue_broadcast/);
});
test('讨论和执行项目显式选择，读写采用独立协作IPC而非总控解释',()=>{
 for(const command of ['ma_conversation_snapshot','ma_conversation_send','ma_conversation_interrupt','ma_conversation_control','ma_conversation_execute_plan']) assert.ok(control.includes(command),command);
 assert.match(control,/role="tablist" aria-label="协作方式"/);
 assert.doesNotMatch(control,/control_interpret|ma_interpret_message|optimize_prompt|自动批准/);
});
test('接收对象仅为本项目具体成员，任务详情和人工处理仍可访问',()=>{
 assert.match(control,/agent\.projectId === projectId && agent\.enabled/);
 assert.match(control,/aria-label="选择参与成员"/);
 assert.match(control,/ariaLabel="协作任务详情"/);
 assert.match(sidebar,/<InterventionCenter/);
 assert.match(control,/<UnverifiedRunNotice/);
 assert.match(control,/ma_pause_project/);
 assert.match(control,/暂停讨论/);
});
