const { test } = require('node:test');
const assert = require('node:assert/strict');
const workspaceTabs = import('../src/workspaceTabs.ts');
const sessionSwitch = import('../src/sessionSwitch.ts');

test('resolves the current physical session from its remembered logical slot and workspace', async () => {
  const [{ findWorkspaceSessionBySlot }, { switchSlotId, switchWorkspaceAssignment }] = await Promise.all([
    workspaceTabs,
    sessionSwitch,
  ]);
  const relay = { source: { id: 'source-a' }, targetId: 'target-a', logicalId: 'source-a', state: 'queued' };
  const records = [relay];
  const assignments = { 'source-a': 'workspace-a', 'source-b': 'workspace-b' };
  const sessionTab = (id) => switchWorkspaceAssignment(id, assignments, records, 'workspace-a');
  const sessionSlot = (id) => switchSlotId(id, records);
  const visible = [{ id: 'target-a' }, { id: 'source-b' }];

  assert.equal(
    findWorkspaceSessionBySlot(visible, 'workspace-a', sessionSlot('source-a'), sessionTab, sessionSlot)?.id,
    'target-a',
    'a replaced physical ID must restore through its stable logical slot',
  );
  assert.equal(
    findWorkspaceSessionBySlot(visible, 'workspace-b', sessionSlot('source-a'), sessionTab, sessionSlot),
    null,
    'a matching logical slot assigned to another workspace must not restore',
  );
  assert.equal(findWorkspaceSessionBySlot(visible, 'workspace-a', null, sessionTab, sessionSlot), null);

  const cancelledRecords = [{ ...relay, state: 'cancelled' }];
  const cancelledTab = (id) => switchWorkspaceAssignment(id, assignments, cancelledRecords, 'workspace-a');
  const cancelledSlot = (id) => switchSlotId(id, cancelledRecords);
  assert.equal(
    findWorkspaceSessionBySlot(
      [{ id: 'source-a' }, { id: 'target-a' }],
      'workspace-a',
      'source-a',
      cancelledTab,
      cancelledSlot,
    )?.id,
    'source-a',
    'cancelling a replacement must fall back to the original session slot',
  );
});
