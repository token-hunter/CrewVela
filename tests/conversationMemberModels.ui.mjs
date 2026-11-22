// Isolated browser regression for the project member model menu.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1432';
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || 'chrome' });
const calls = (page, command) => page.evaluate(command => window.conversationMock.calls.filter(item => item.command === command), command);

try {
  for (const width of [1122, 370]) {
    const settingsPage = await browser.newPage({viewport:{width, height:870}});
    await settingsPage.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&theme=${width === 370 ? 'light' : 'dark'}`, {waitUntil:'domcontentloaded'});
    await settingsPage.getByRole('tab', {name:'团队聊天', exact:true}).waitFor();
    const header = settingsPage.locator('.conversation-members-trigger');
    await header.click();
    const trigger = settingsPage.getByRole('button', {name: 'Codex的模型与推理', exact: true});
    const checkbox = settingsPage.locator('.conversation-member-menu').getByRole('checkbox', {name:'Codex', exact:true});
    const wasChecked = await checkbox.isChecked();
    const firstBounds = await trigger.boundingBox();
    await trigger.click();
    const editor = settingsPage.getByRole('region', {name:'Codex的模型与推理设置'});
    await editor.getByRole('button', {name:/^模型/}).click();
    assert.deepEqual((await calls(settingsPage, 'ma_conversation_member_models'))[0].args, {projectId:'A', agentId:'codex', refresh:false});
    assert.equal((await calls(settingsPage, 'ma_conversation_send')).length, 0);
    const modelBounds = await editor.boundingBox();
    assert.ok(modelBounds.x >= 0 && modelBounds.x + modelBounds.width <= width);
    if (width === 1122) assert.ok(modelBounds.x + modelBounds.width < firstBounds.x);
    await editor.getByRole('button', {name:'测试模型 A', exact:true}).click();
    await editor.getByText('已保存 · 下一轮生效').waitFor();
    await editor.getByRole('button', {name:/^推理级别/}).click();
    await editor.getByRole('button', {name:'高', exact:true}).click();
    await editor.getByText('已保存 · 下一轮生效').waitFor();
    const selections = await calls(settingsPage, 'ma_set_conversation_member_models');
    assert.deepEqual(selections.at(-1).args, {projectId:'A', agentId:'codex', selection:{model:'fixture-a', reasoning:'high'}});
    if (width === 370) await editor.getByRole('button', {name:'返回成员列表', exact:true}).click();
    else await settingsPage.keyboard.press('Escape');
    if (width === 1122) await header.click();
    assert.equal(await checkbox.isChecked(), wasChecked);
    await trigger.click();
    await editor.getByRole('button', {name:'跟随 CLI', exact:true}).click();
    await editor.getByText('已保存 · 下一轮生效').waitFor();
    assert.deepEqual((await calls(settingsPage, 'ma_set_conversation_member_models')).at(-1).args.selection, {model:null, reasoning:null});
  }

  console.log('Team member model menu passed at 1122px and 370px.');
} finally {
  await browser.close();
}
