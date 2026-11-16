import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {PRIVACY_DATA_ROWS,PRIVACY_EXPLANATIONS,privacySections} from '../src/privacySettings.ts';
test('privacy data actions keep exact existing destinations and a separate directory view',()=>{
 assert.equal(PRIVACY_DATA_ROWS.length,5);assert.equal(PRIVACY_DATA_ROWS[0].destination,undefined);
 assert.deepEqual(PRIVACY_DATA_ROWS.slice(1).map(row=>row.destination),['archivedSessions','collaboration','sharedMemory','browserControl']);
 assert.equal(PRIVACY_DATA_ROWS.filter(row=>row.title==='共享记忆').length,1);
});
test('privacy explanations read current bundled policy sections including CRLF',()=>{
 const policy=readFileSync('docs/privacy.md','utf8').replace(/\r?\n/g,'\r\n');
 for(const detail of Object.values(PRIVACY_EXPLANATIONS)){
  const sections=privacySections(policy,detail.headings);assert.deepEqual(sections.map(s=>s.title),[...detail.headings]);assert.ok(sections.every(s=>s.paragraphs.length));
 }
 const services=privacySections(policy,PRIVACY_EXPLANATIONS.services.headings).flatMap(s=>s.paragraphs).join('\n');
 assert.match(services,/记忆服务/);assert.match(services,/没有统一控制/);assert.deepEqual(privacySections(policy,['missing']),[]);
});
