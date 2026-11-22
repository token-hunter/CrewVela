// Isolated browser regression for the project member model bar.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432';
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || 'chrome' });
const calls = (page, command) => page.evaluate(command => window.conversationMock.calls.filter(item => item.command === command), command);

try {
  for (const width of [1122, 370]) {
    const settingsPage = await browser.newPage({viewport:{width, height:870}});
    await settingsPage.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?theme=${width === 370 ? 'light' : 'dark'}`, {waitUntil:'domcontentloaded'});
    await settingsPage.getByRole('tab', {name:'团队聊天', exact:true}).waitFor();
    assert.equal((await calls(settingsPage, 'ma_conversation_member_models')).length, 0);
    const header = settingsPage.locator('.conversation-members-trigger');
    await header.click();
    const trigger = settingsPage.getByRole('button', {name: 'Codex的模型与推理', exact: true});
    const checkbox = settingsPage.locator('.conversation-member-menu').getByRole('checkbox', {name:'Codex', exact:true});
    const wasChecked = await checkbox.isChecked();
    await trigger.click();
    const editor = settingsPage.getByRole('region', {name:'Codex的模型与推理设置'});
    const model = editor.getByRole('combobox', {name:'模型', exact:true});
    const reasoning = editor.getByRole('combobox', {name:'推理级别', exact:true});
    await model.waitFor();
    assert.deepEqual((await calls(settingsPage, 'ma_conversation_member_models'))[0].args, {projectId:'A', agentId:'codex', refresh:false});
    assert.equal((await calls(settingsPage, 'ma_conversation_send')).length, 0);
    const modelBounds = await editor.boundingBox();
    assert.ok(modelBounds.x >= 0 && modelBounds.x + modelBounds.width <= width);
    const composer = settingsPage.locator('.multi-agent-composer');
    const composerBounds = await composer.boundingBox();
    assert.ok(modelBounds.y + modelBounds.height <= composerBounds.y);
    const modelField = await model.boundingBox();
    const reasoningField = await reasoning.boundingBox();
    assert.ok(reasoningField.x > modelField.x);
    assert.ok(Math.abs(reasoningField.y - modelField.y) <= 1);
    await model.selectOption('fixture-a');
    await editor.getByText('已保存 · 下一轮聊天生效').waitFor();
    await reasoning.selectOption('high');
    await editor.getByText('已保存 · 下一轮聊天生效').waitFor();
    const selections = await calls(settingsPage, 'ma_set_conversation_member_models');
    assert.deepEqual(selections.at(-1).args, {projectId:'A', agentId:'codex', selection:{model:'fixture-a', reasoning:'high'}});
    const memberChips = settingsPage.locator('[data-message-id="m5"] .conversation-member-model-summary');
    await memberChips.getByText('测试模型 A', {exact:true}).waitFor();
    await memberChips.getByText('高', {exact:true}).waitFor();
    assert.equal(await editor.locator('.conversation-model-actions').evaluate(el => el.scrollWidth <= el.clientWidth), true);
    assert.equal(await settingsPage.locator('[data-message-id="m2"] .conversation-member-model-summary').count(), 0);
    await editor.getByRole('button', {name:'收起模型与推理', exact:true}).click();
    await editor.waitFor({state:'hidden'});
    const lower = settingsPage.getByRole('button', {name:'模型与推理', exact:true});
    assert.equal(await lower.evaluate(el => el === document.activeElement), true);
    await header.click();
    assert.equal(await checkbox.isChecked(), wasChecked);
    await header.click();
    await lower.click();
    await settingsPage.getByRole('combobox', {name:'配置成员', exact:true}).selectOption('codex');
    await model.locator('option[value="fixture-a"]').waitFor({state:'attached'});
    assert.equal(await model.inputValue(), 'fixture-a');
    assert.equal(await reasoning.inputValue(), 'high');
    await editor.getByRole('button', {name:'跟随 CLI', exact:true}).click();
    await editor.getByText('已保存 · 下一轮聊天生效').waitFor();
    await memberChips.waitFor({state:'hidden'});
    assert.deepEqual((await calls(settingsPage, 'ma_set_conversation_member_models')).at(-1).args.selection, {model:null, reasoning:null});
    await settingsPage.close();
  }

  console.log('Member model bar, saved labels, member selection, and fallbacks passed at 1122px and 370px.');
} finally {
  await browser.close();
}
