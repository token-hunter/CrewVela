import assert from 'node:assert/strict';
import test from 'node:test';
import {visibleProjectTasks} from '../src/collaborationTasks.ts';

test('项目任务口径沿用后端讨论任务标识，保留工程任务和原始数据',()=>{
 const tasks=Object.freeze([{id:'normal-task'},{id:'conversation-task-plan'},{id:'cli-message-conversation-turn'},{id:'cli-message-legacy'}].map(Object.freeze));
 assert.deepEqual(visibleProjectTasks(tasks).map(t=>t.id),['normal-task','conversation-task-plan','cli-message-legacy']);
 assert.equal(tasks.length,4);assert.notEqual(visibleProjectTasks(tasks),tasks);
 assert.deepEqual(visibleProjectTasks([]),[]);
});
