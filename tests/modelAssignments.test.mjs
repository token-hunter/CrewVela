import assert from 'node:assert/strict';
import test from 'node:test';
import { assignmentDraft, assignmentPreview, assignmentSource, connectionLabel, filterAssignments, modelSuggestions } from '../src/modelAssignments.ts';

test('作用域与搜索可组合，默认配置保留在项目视图，成员和会话来源不混淆', () => {
  const data = assignmentPreview();
  assert.equal(filterAssignments(data.rows, 'preview-project', 'all', '').length, 8);
  assert.equal(filterAssignments(data.rows, 'other-project', 'all', '').length, 5);
  const member = filterAssignments(data.rows, '', 'member', 'TERRA');
  assert.equal(member.length, 1); assert.equal(member[0].name, '前端工程师');
  assert.equal(assignmentSource(member[0]), '成员覆盖');
  const session = filterAssignments(data.rows, '', 'session', '前端页面');
  assert.equal(assignmentSource(session[0]), '会话覆盖'); assert.equal(session[0].editable, false);
});
test('模型连接、账号本机配置、缺失连接与已有模型候选分别展示', () => {
  const provider = { id:'glm',name:'智谱服务',env:{ ANTHROPIC_MODEL:'glm-real',ANTHROPIC_DEFAULT_OPUS_MODEL_NAME:'display-name' } };
  assert.equal(connectionLabel('glm','claude_code',[provider]), '智谱服务');
  assert.equal(connectionLabel('local','codex',[provider]), 'Codex 本机配置');
  assert.match(connectionLabel('missing','codex',[]), /连接已缺失/);
  const suggestions = modelSuggestions(assignmentPreview().rows, [provider], 'claude_code', 'glm');
  assert.ok(suggestions.includes('glm-real')); assert.ok(!suggestions.includes('display-name'));
  assert.ok(!suggestions.includes('gpt-6.1-sol'));
});
test('编辑历史服务名配置会拆成真实连接和模型，不把服务名另存为模型 ID', () => {
  const row={...assignmentPreview().rows[0],raw:{connectionId:null,model:'glm'},effective:{connectionId:'glm',model:'glm-5.3',source:'legacy_connection'}};
  assert.deepEqual(assignmentDraft(row),{connectionId:'glm',model:'glm-5.3'});
  assert.deepEqual(row.raw,{connectionId:null,model:'glm'});
});
