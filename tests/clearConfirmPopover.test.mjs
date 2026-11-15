import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const control=readFileSync(new URL('../src/components/MultiAgentControl.tsx',import.meta.url),'utf8');
test('共享会话历史由后端提供，移除只清本地的假清空入口',()=>{
 assert.doesNotMatch(control,/clearConversationConfirmOpen|清空对话|localStorage\.clear/);
 assert.match(control,/ma_conversation_snapshot/);
 assert.match(control,/查看更早的/);
});
test('待执行分工公开命令与写入范围，只有显式执行按钮启动工程任务',()=>{
 assert.match(control,/task\.verificationCommand/); assert.match(control,/task\.writePaths/);
 assert.match(control,/按此分工执行/); assert.match(control,/ma_conversation_execute_plan/);
 assert.match(control,/clientRequestId/);
});
