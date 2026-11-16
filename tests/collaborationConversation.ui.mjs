// Fixture-only IPC: no real CLI, model account or user project is touched.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432';
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || 'chrome' });
const contexts = [], errors = [];
const url = extra => `${base}/tests/fixtures/collaboration-conversation-ui.html?${extra}`;
async function fresh(extra = '', viewport = { width: 1122, height: 1402 }) {
  const context = await browser.newContext({ viewport }); contexts.push(context);
  const page = await context.newPage(); page.setDefaultTimeout(10000);
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(url(extra)); await page.locator('.conversation-members button').first().waitFor();
  await page.waitForFunction(() => document.querySelector('.conversation-modes [aria-selected="true"]'));
  return page;
}
const calls = (page, command) => page.evaluate(command => window.conversationMock.calls.filter(item => item.command === command), command);
try {
  const page = await fresh();
  assert.equal(await page.getByRole('tab', { name: '执行项目', exact: true }).getAttribute('aria-selected'), 'true');
  await page.getByRole('button', { name: '查看任务 · 3', exact: true }).click();
  const settings = page.getByRole('dialog', { name: '设置', exact: true });
  await settings.locator('.collab-task-row').first().waitFor(); assert.equal(await settings.locator('.collab-task-row').count(), 3);
  assert.equal(await settings.getByRole('button', { name: '全部', exact: true }).getAttribute('aria-pressed'), 'true');
  await settings.getByRole('button', { name: '暂停项目', exact: true }).click();
  await page.waitForFunction(() => window.conversationMock.projects[0].status === 'paused');
  assert.equal((await calls(page, 'ma_pause_project')).length, 1);
  await settings.getByRole('button', { name: '关闭设置', exact: true }).click();
  await settings.waitFor({ state: 'detached' });
  await page.getByRole('button', { name: '查看Codex的原会话', exact: true }).click();
  assert.deepEqual((await calls(page, 'fixture_open_session')).at(-1).args, { sessionId: 'session-codex' });
  const editor = page.getByRole('combobox', { name: '给团队发消息', exact: true });
  await page.getByRole('tab', { name: '讨论', exact: true }).click();
  await editor.fill('@Cod');
  await page.getByRole('option', { name: /@Codex/ }).waitFor();
  await page.waitForFunction(() => document.querySelector('.multi-agent-mention-results [role="option"]')?.textContent.includes('@Codex'));
  await editor.press('Enter'); assert.equal(await editor.inputValue(), '@Codex ');
  assert.equal((await calls(page, 'ma_conversation_send')).length, 0, 'choosing a mention is not submission');
  await editor.fill('@'); await page.locator('.multi-agent-mention-menu').getByRole('option').first().waitFor();
  await editor.press('Escape'); await editor.dispatchEvent('select');
  assert.equal(await page.locator('.multi-agent-mention-menu').getByRole('option').count(), 0, 'the same caret selection must not reopen a dismissed menu');
  await editor.fill('@Claude Code @Codex 检查接口');
  await page.locator('.conversation-composer').evaluate(form => { form.requestSubmit(); form.requestSubmit(); });
  await page.getByRole('button', { name: /暂停讨论/ }).waitFor();
  const sent = await calls(page, 'ma_conversation_send');
  assert.equal(sent.length, 1); assert.deepEqual(sent[0].args.input.participantAgentIds, ['claude', 'codex']);
  await page.getByRole('combobox', { name: '给团队插话', exact: true }).fill('先核对数据格式');
  await page.getByRole('button', { name: '发送插话', exact: true }).click();
  await page.getByRole('button', { name: '继续讨论', exact: true }).waitFor();
  assert.equal((await calls(page, 'ma_conversation_interrupt')).length, 1);
  assert.equal((await calls(page, 'ma_conversation_send')).length, 1, 'interjection is not an unbounded extra dispatch');

  const uncertain = await fresh('empty=1&fail=send-before-save');
  await uncertain.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('核对网络恢复');
  await uncertain.getByRole('button', { name: '发送消息', exact: true }).click();
  await uncertain.getByRole('button', { name: '核对并重试', exact: true }).waitFor();
  await uncertain.waitForTimeout(1700);
  assert.match(await uncertain.getByRole('alert').innerText(), /响应丢失/);
  await uncertain.getByRole('button', { name: '核对并重试', exact: true }).click();
  await uncertain.getByRole('combobox', { name: '给团队插话', exact: true }).waitFor();
  const attempts = await calls(uncertain, 'ma_conversation_send');
  assert.equal(attempts.length, 2); assert.equal(attempts[0].args.input.clientMessageId, attempts[1].args.input.clientMessageId);
  assert.equal(await uncertain.getByRole('log').getByText('核对网络恢复', { exact: true }).count(), 1, 'an accepted message must not be duplicated after a lost response');
  assert.equal(await uncertain.getByRole('button', { name: '核对并重试', exact: true }).count(), 0);

  const accepted = await fresh('empty=1&fail=send');
  await accepted.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('已保存但响应丢失');
  await accepted.getByRole('button', { name: '发送消息', exact: true }).click();
  await accepted.getByRole('button', { name: '核对并重试', exact: true }).waitFor();
  await accepted.getByRole('button', { name: '核对并重试', exact: true }).waitFor({ state: 'detached' });
  assert.equal((await calls(accepted, 'ma_conversation_send')).length, 1, 'an exact persisted receipt settles uncertainty without resending');
  assert.equal(await accepted.getByRole('log').getByText('已保存但响应丢失', { exact: true }).count(), 1);

  const scope = await fresh();
  await scope.evaluate(() => { window.conversationMock.holdProject = 'B'; });
  await scope.getByLabel('协作项目', { exact: true }).selectOption('B');
  await scope.waitForFunction(() => window.conversationMock.pending.filter(item => item.args.projectId === 'B').length >= 2);
  assert.equal(await scope.getByRole('log').getByText('按这个分工执行。', { exact: true }).count(), 0, 'new projects never display the old transcript while loading');
  await scope.getByLabel('协作项目', { exact: true }).selectOption('A');
  await scope.getByRole('log').getByText('按这个分工执行。', { exact: true }).waitFor();
  await scope.evaluate(() => { window.conversationMock.holdProject = ''; while (window.conversationMock.pending.length) window.conversationMock.release(window.conversationMock.pending[0].command); });
  assert.equal(await scope.getByLabel('协作项目', { exact: true }).inputValue(), 'A');
  await scope.getByRole('log').getByText('按这个分工执行。', { exact: true }).waitFor();

  const editing = await fresh('empty=1');
  await editing.evaluate(() => { window.conversationMock.holdSend = true; });
  await editing.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('第一个消息');
  await editing.getByRole('button', { name: '发送消息', exact: true }).click();
  await editing.waitForFunction(() => window.conversationMock.pending.some(item => item.command === 'ma_conversation_send'));
  await editing.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('新的未发送草稿');
  await editing.evaluate(() => window.conversationMock.release('ma_conversation_send'));
  assert.equal(await editing.getByRole('combobox', { name: '给团队发消息', exact: true }).inputValue(), '新的未发送草稿');

  const planning = await fresh('plan=pending');
  await planning.getByText('npm test', { exact: true }).waitFor();
  await planning.getByRole('button', { name: '按此分工执行', exact: true }).evaluate(button => { button.click(); button.click(); });
  await planning.getByRole('heading', { name: '分工已确认', exact: true }).waitFor();
  assert.equal((await calls(planning, 'ma_conversation_execute_plan')).length, 1);
  assert.equal((await calls(planning, 'ma_apply_plan')).length, 0);

  const stopping = await fresh('running=1&stopping=1');
  await stopping.getByRole('button', { name: /暂停讨论/ }).click();
  await stopping.getByText('正在停止', { exact: false }).first().waitFor();
  assert.equal(await stopping.getByRole('button', { name: '继续讨论', exact: true }).count(), 0, 'unverified stops cannot unlock a new round');
  assert.equal(await stopping.getByRole('button', { name: /暂停讨论/ }).isDisabled(), true);

  const unsupported = await fresh('unsupported=1');
  await unsupported.locator('.conversation-members-trigger').click();
  assert.equal(await unsupported.locator('.conversation-member-menu').getByRole('checkbox', { name: 'Kimi Code', exact: true }).isDisabled(), true);
  await unsupported.keyboard.press('Escape');
  await unsupported.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('@Kimi Code 检查');
  await unsupported.getByRole('button', { name: '发送消息', exact: true }).click();
  await unsupported.getByRole('alert').filter({ hasText: '暂不支持受限讨论' }).waitFor();
  assert.equal((await calls(unsupported, 'ma_conversation_send')).length, 0);

  const disabled = await fresh('disabled=codex');
  await disabled.locator('.conversation-members-trigger').click();
  assert.equal(await disabled.locator('.conversation-member-menu').getByRole('checkbox', { name: 'Codex', exact: true }).isDisabled(), true);
  await disabled.keyboard.press('Escape');
  await disabled.getByRole('combobox', { name: '给团队发消息', exact: true }).fill('@Codex 检查');
  await disabled.getByRole('button', { name: '发送消息', exact: true }).click();
  await disabled.getByRole('alert').filter({ hasText: 'Runtime 已停用' }).waitFor();
  assert.equal((await calls(disabled, 'ma_conversation_send')).length, 0, 'disabled Runtime preferences also constrain explicit mentions');

  mkdirSync('.scratch/collaboration-conversation', { recursive: true });
  for (const [theme, size] of ['dark', 'light'].flatMap(theme => [{ width: 1122, height: 1402 }, { width: 780, height: 540 }, { width: 640, height: 700 }, { width: 390, height: 844 }, { width: 300, height: 420 }].map(size => [theme, size]))) {
    const visual = await fresh(`theme=${theme}`, size);
    await visual.locator('.conversation-transcript').evaluate(node => { node.scrollTop = 0; });
    const geometry = await visual.evaluate(() => {
      const footer = document.querySelector('.conversation-footer').getBoundingClientRect();
      const editor = document.querySelector('.conversation-composer textarea');
      const computed = getComputedStyle(editor);
      const area = editor.getBoundingClientRect(), wrapper = editor.parentElement.getBoundingClientRect(), actions = document.querySelector('.conversation-composer-actions').getBoundingClientRect();
      return { overflow: document.body.scrollWidth > innerWidth, bottom: footer.bottom, area: { width: area.width, height: area.height, bottom: area.bottom }, wrapper: { width: wrapper.width, height: wrapper.height }, actionsTop: actions.top, fill: computed.webkitTextFillColor, color: computed.color, placeholder: getComputedStyle(editor, '::placeholder').webkitTextFillColor, images: [...document.querySelectorAll('img')].every(img => img.complete && img.naturalWidth > 0) };
    });
    assert.equal(geometry.overflow, false); assert.ok(geometry.bottom <= size.height + 1, JSON.stringify({ theme, size, geometry }));
    assert.ok(Math.abs(geometry.area.width - geometry.wrapper.width) < 1, 'typing fills the editor width');
    assert.ok(Math.abs(geometry.area.height - geometry.wrapper.height) < 1, 'no unused strip below the textarea');
    assert.ok(geometry.area.height >= 96, 'multiline input has sufficient default space');
    assert.ok(geometry.actionsTop >= geometry.area.bottom - 1, 'recipient and send actions stay below the full-width editor');
    assert.notEqual(geometry.fill, 'rgba(0, 0, 0, 0)', 'typed text remains visible without the old transparent highlight overlay');
    assert.equal(geometry.fill, geometry.color); assert.notEqual(geometry.placeholder, 'rgba(0, 0, 0, 0)');
    assert.equal(geometry.images, true);
    await visual.screenshot({ path: `.scratch/collaboration-conversation/${theme}-${size.width}.jpg`, fullPage: false });
    await visual.locator('.conversation-target').click();
    const recipients = await visual.locator('.conversation-recipient-menu').boundingBox();
    assert.ok(recipients.y >= 0 && recipients.y + recipients.height <= size.height + 1, 'member popup remains visible above the expanded editor');
    await visual.keyboard.press('Escape');
  }
  const resizing = await fresh('empty=1&members=many', { width: 780, height: 540 });
  const resizeEditor = resizing.getByRole('combobox', { name: '给团队发消息', exact: true });
  const original = await resizeEditor.boundingBox();
  await resizing.mouse.move(original.x + original.width - 4, original.y + original.height - 4); await resizing.mouse.down();
  await resizing.mouse.move(original.x + original.width - 4, original.y + original.height + 60, { steps: 10 }); await resizing.mouse.up();
  const expanded = await resizeEditor.boundingBox(); assert.ok(expanded.height > original.height + 30, 'manual resizing changes the actual textarea, not only an ignored inline height');
  await resizing.locator('.conversation-target').click();
  const resizedRecipients = await resizing.locator('.conversation-recipient-menu').boundingBox(); assert.ok(resizedRecipients.y >= 0 && resizedRecipients.y + resizedRecipients.height <= 541);
  assert.deepEqual(errors, []);
  console.log('PASS: scoped chat, explicit mentions, duplicate guards, durable retry identity, project isolation, new drafts, plan execution, pause semantics, source sessions, supported runtimes, dark/light and narrow layouts');
} finally { for (const context of contexts) await context.close(); await browser.close(); }
