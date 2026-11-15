import assert from 'node:assert/strict';
import test from 'node:test';
import { activeRun, currentRun, matchesRunFilter, planState, runStage } from '../src/automationExecution.ts';
const item = runs => ({ enabled:true, maxRuns:null, runCount:2, consecutiveFailures:0, maxConsecutiveFailures:3, trigger:{kind:'daily',hour:9,minute:0}, recentRuns:runs });
test('Old dispatch receipts and missing results never imply success', () => {
  assert.equal(runStage({ok:true}), 'unconfirmed'); assert.equal(runStage({ok:false}), 'failed');
  for (const stage of ['queued','submitted','received','unconfirmed']) assert.equal(activeRun({stage}), true);
});
test('Daily plan remains enabled after a completed run; pause is independent of active work', () => {
  const a = item([{stage:'completed'}]); assert.equal(planState(a), '计划已启用');
  assert.equal(matchesRunFilter(a,'completed'),true);
  a.enabled=false; a.recentRuns.push({stage:'running'});
  assert.equal(planState(a),'计划已暂停'); assert.equal(matchesRunFilter(a,'active'),true);
});
test('Skipped ticks do not replace unresolved execution; attention excludes successful tasks', () => {
  const a=item([{id:'old',stage:'unconfirmed'},{id:'new',stage:'completed'},{stage:'skipped'}]);
  assert.equal(currentRun(a).id,'old'); assert.equal(matchesRunFilter(a,'attention'),true);
  assert.equal(matchesRunFilter(a,'completed'),false); assert.equal(matchesRunFilter(a,'failed'),false);
});
test('Once and exhausted schedule do not invent task completion', () => {
  const a=item([{stage:'queued'}]); a.maxRuns=2;
  assert.equal(planState(a),'触发次数已用完'); assert.equal(matchesRunFilter(a,'completed'),false);
});
