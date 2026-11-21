// Real App + actual dialogs and isolated IPC. No CLI/model/configuration is touched.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const base = process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1420';
const output = '.scratch/cli-launch-modes-20261116/ui'; mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome' });
const errors = [];
async function choose(page, kind) {
  await page.getByRole('button', {name:'新建会话',exact:true}).first().click();
  const labels = {
    claude_code:'Claude Code', codex:'Codex CLI', kimi_code:'Kimi Code', opencode:'OpenCode', pi:'Pi Agent', zcode_cli:'ZCode CLI',
    claude_chat:'Claude Code Chat', codex_chat:'Codex Chat', kimi_chat:'Kimi Code Chat', opencode_chat:'OpenCode Chat',
    pi_chat:'Pi Chat', deepseek_harness:'DeepSeek Harness', zcode:'ZCode Desktop',
  };
  await page.getByRole('menu',{name:'新建 Runtime 会话'}).getByRole('menuitem',{name:labels[kind],exact:true}).click();
  const dialog = page.getByRole('dialog', {name:'新建会话',exact:true});
  await dialog.waitFor(); return dialog;
}
try {
  for (const theme of ['dark', 'light']) {
    const page = await browser.newPage({viewport:{width:1440,height:1000}}); page.setDefaultTimeout(12000);
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(theme => localStorage.setItem('crewvela-appearance', JSON.stringify({theme})),theme);
    await page.goto(`${base}/tests/fixtures/app-session-async-ui.html?cli-launch=1`);
    await page.waitForFunction(()=>window.appSessionMock);
    await page.waitForFunction(theme => document.documentElement.dataset.appearanceTheme === theme,theme);
    let dialog = await choose(page,'codex');
    assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).inputValue(),'read_only');
    await dialog.screenshot({path:`${output}/launch-${theme}.png`});
    await dialog.getByRole('combobox',{name:'本次启动模式'}).selectOption('workspace');
    await dialog.getByRole('button',{name:'启动',exact:true}).click(); await dialog.waitFor({state:'detached'});
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.defaults.codex),'read_only');
    let call=await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').at(-1));
    assert.equal(call.args.launchMode,'workspace'); assert.equal(call.args.cwd,'C:\\fixture');
    await page.locator('.pane-launch-mode').filter({hasText:'工作区写入'}).waitFor();
    dialog=await choose(page,'claude_code');
    await dialog.getByRole('button',{name:'管理默认模式'}).click();
    const management=page.getByRole('dialog',{name:'管理 Runtime',exact:true});
    assert.equal(await management.locator('.runtime-default-mode').count(),6);
    await management.getByRole('combobox',{name:'默认启动模式 · Claude Code'}).selectOption('manual');
    await page.waitForFunction(()=>window.appSessionMock.cliLaunch.defaults.claude_code==='manual');
    await management.screenshot({path:`${output}/management-${theme}.png`});
    assert.equal(await management.evaluate(el=>el.scrollWidth>el.clientWidth),false);
    await page.setViewportSize({width:390,height:850});
    await management.screenshot({path:`${output}/management-narrow-${theme}.png`});
    assert.equal(await management.evaluate(el=>el.scrollWidth>el.clientWidth),false);
    await page.setViewportSize({width:1440,height:1000});
    await page.evaluate(()=>window.appSessionMock.cliLaunch.failSave=true);
    await management.getByRole('combobox',{name:'默认启动模式 · Claude Code'}).selectOption('yolo');
    await management.getByRole('alert').filter({hasText:'写入失败'}).waitFor();
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.defaults.claude_code),'manual');
    await page.evaluate(()=>window.appSessionMock.cliLaunch.failSave=false);
    await management.getByRole('button',{name:'关闭',exact:true}).click();
    assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).inputValue(),'manual');
    await dialog.getByRole('combobox',{name:'启动 Agent'}).selectOption('claude_chat');
    assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).count(),0);
    assert.equal(await dialog.getByRole('button',{name:'管理默认模式'}).count(),0);
    await dialog.getByRole('combobox',{name:'启动 Agent'}).selectOption('pi');
    assert.equal(await dialog.getByRole('button',{name:'管理默认模式'}).isVisible(),true);
    assert.equal(await dialog.locator('option[value="yolo"]').count(),0);
    await dialog.getByRole('combobox',{name:'启动 Agent'}).selectOption('kimi_code');
    await dialog.getByRole('combobox',{name:'本次启动模式'}).selectOption('yolo');
    await dialog.getByRole('checkbox',{name:'设为 Kimi Code 的默认模式',exact:true}).check();
    await dialog.getByRole('button',{name:'启动',exact:true}).click(); await dialog.waitFor({state:'detached'});
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.defaults.kimi_code),'yolo');
    dialog=await choose(page,'opencode');
    await dialog.getByRole('combobox',{name:'本次启动模式'}).selectOption('auto');
    await dialog.getByRole('checkbox',{name:'设为 OpenCode 的默认模式',exact:true}).check();
    await page.evaluate(()=>window.appSessionMock.cliLaunch.failSave=true);
    const prior=await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').length);
    await dialog.getByRole('button',{name:'启动',exact:true}).click(); await dialog.getByRole('alert').filter({hasText:'写入失败'}).waitFor();
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').length),prior);
    await page.evaluate(()=>{window.appSessionMock.cliLaunch.failSave=false;window.appSessionMock.cliLaunch.hold=true;});
    await dialog.getByRole('checkbox',{name:'设为 OpenCode 的默认模式',exact:true}).uncheck();
    await dialog.getByRole('button',{name:'启动',exact:true}).click();
    await page.waitForFunction(()=>window.appSessionMock.cliLaunch.pending.length===1);
    await dialog.locator('form').dispatchEvent('submit'); await page.keyboard.press('Escape');
    assert.equal(await dialog.count(),1);
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').length),prior+1);
    await page.evaluate(()=>window.appSessionMock.cliLaunch.release()); await dialog.waitFor({state:'detached'});
    await page.reload();
    dialog=await choose(page,'kimi_code');
    assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).inputValue(),'yolo');
    await page.setViewportSize({width:390,height:850});
    assert.equal(await dialog.evaluate(el=>el.scrollWidth>el.clientWidth),false);
    await dialog.screenshot({path:`${output}/launch-narrow-${theme}.png`});
    await page.keyboard.press('Escape'); await dialog.waitFor({state:'detached'});
    await page.setViewportSize({width:1440,height:1000});
    await page.evaluate(()=>window.appSessionMock.cliLaunch.hold=false);
    for (const [kind,mode] of [['claude_code','plan'],['pi','read_tools'],['zcode_cli','guarded']]) {
      dialog=await choose(page,kind);
      await dialog.getByRole('combobox',{name:'本次启动模式'}).selectOption(mode);
      await dialog.getByRole('button',{name:'启动',exact:true}).click();await dialog.waitFor({state:'detached'});
      const actual=await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').at(-1).args);
      assert.equal(actual.runtimeKind,kind);assert.equal(actual.launchMode,mode);
    }
    dialog=await choose(page,'codex');
    await dialog.getByRole('combobox',{name:'本次启动模式'}).selectOption('yolo');
    await page.evaluate(()=>window.appSessionMock.cliLaunch.failLaunch=true);
    await dialog.getByRole('button',{name:'启动',exact:true}).click();
    await dialog.getByRole('alert').filter({hasText:'CLI 拒绝启动模式'}).waitFor();
    assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).inputValue(),'yolo');
    assert.equal(await page.evaluate(()=>window.appSessionMock.cliLaunch.defaults.codex),'read_only');
    await page.evaluate(()=>window.appSessionMock.cliLaunch.failLaunch=false);
    await page.keyboard.press('Escape');await dialog.waitFor({state:'detached'});
    await page.getByRole('button',{name:'新增页签',exact:true}).click();
    await page.locator('.ews-card').waitFor();
    await page.locator('.ews-card .ews-row').filter({hasText:'Fixture project'}).click();
    await page.getByRole('button',{name:'选择 Agent',exact:true}).click();
    await page.getByRole('button',{name:'Codex',exact:true}).click();
    dialog=page.getByRole('dialog',{name:'新建会话',exact:true});
    await dialog.getByRole('button',{name:'启动',exact:true}).click();await dialog.waitFor({state:'detached'});
    const projectLaunch=await page.evaluate(()=>window.appSessionMock.cliLaunch.calls.filter(item=>item.command==='launch_runtime_session').at(-1).args);
    assert.equal(projectLaunch.projectId,'fixture-project');assert.equal(projectLaunch.cwd,'C:\\fixture');assert.equal(projectLaunch.launchMode,'read_only');

    const chatCommands = {
      claude_chat:'create_claude_chat_session', codex_chat:'create_codex_chat_session', kimi_chat:'create_kimi_chat_session',
      opencode_chat:'create_opencode_chat_session', pi_chat:'create_pi_chat_session',
      deepseek_harness:'create_deepseek_harness_session', zcode:'create_zcode_session',
    };
    const chatActivity = () => page.evaluate(() => ({
      launches: window.appSessionMock.cliLaunch.calls.filter(item => item.command.startsWith('create_')),
      picks: window.appSessionMock.cliLaunch.pickerCalls.length,
    }));
    const initial = await chatActivity();
    let expectedPicks = initial.picks;
    let expectedLaunches = initial.launches.length;
    for (const [kind, command] of Object.entries(chatCommands)) {
      dialog = await choose(page,kind);
      assert.equal(await dialog.getByRole('combobox',{name:'启动 Agent'}).inputValue(),kind);
      assert.equal(await dialog.getByRole('combobox',{name:'本次启动模式'}).count(),0);
      assert.equal(await dialog.getByRole('button',{name:'管理默认模式'}).count(),0);
      const activity = await chatActivity();
      assert.equal(activity.picks,expectedPicks,`${kind}: no automatic directory picker`);
      assert.equal(activity.launches.length,expectedLaunches,`${kind}: no creation before confirmation`);

      if (kind === 'claude_chat') {
        await dialog.getByRole('button',{name:'取消',exact:true}).click();
        await dialog.waitFor({state:'detached'});
        assert.deepEqual(await chatActivity(),activity);
        await page.locator('.session-project-head').filter({hasText:'Fixture project'}).hover();
        await page.getByTitle('在「Fixture project」下新建会话').click();
        await dialog.getByRole('combobox',{name:'启动 Agent'}).selectOption(kind);
        assert.equal(await dialog.getByRole('combobox',{name:'工作目录'}).inputValue(),'fixture-project');
        await dialog.screenshot({path:`${output}/chat-launch-${theme}.png`});
        await page.setViewportSize({width:390,height:850});
        assert.equal(await dialog.evaluate(el=>el.scrollWidth>el.clientWidth),false);
        await dialog.screenshot({path:`${output}/chat-launch-narrow-${theme}.png`});
        await page.setViewportSize({width:1440,height:1000});
      }
      await dialog.getByRole('combobox',{name:'工作目录'}).selectOption('second-project');
      let expected = { cwd:'D:\\Second 项目', projectId:'second-project' };
      if (kind === 'pi_chat') {
        expected = { cwd:'G:\\临时目录', projectId:null };
        await page.evaluate(path => window.appSessionMock.cliLaunch.pickerValue = path,expected.cwd);
        await dialog.getByRole('button',{name:'浏览其他目录…'}).click();
        await dialog.getByText('临时使用此目录，会话归入未分组').waitFor();
        expectedPicks++;
      }
      await dialog.getByRole('button',{name:'启动',exact:true}).click();
      await dialog.waitFor({state:'detached'});
      const after = await chatActivity();
      assert.equal(after.picks,expectedPicks);
      assert.equal(after.launches.length,++expectedLaunches);
      assert.deepEqual(after.launches.at(-1),{command,args:expected});
    }
    await page.close();
  }
  const firstRun = await browser.newPage();
  await firstRun.goto(`${base}/tests/fixtures/app-session-async-ui.html?cli-launch=1&chat-setup=1`);
  const launch = await choose(firstRun, 'claude_chat');
  await launch.getByText('缺少兼容的 Node.js', { exact: true }).waitFor();
  await launch.getByRole('button', { name: '安装与版本', exact: true }).click();
  const setup = firstRun.getByRole('dialog', { name: '管理 Runtime', exact: true });
  await setup.getByRole('heading', { name: '管理 Claude Code Chat', exact: true }).waitFor();
  await setup.getByRole('button', { name: '关闭', exact: true }).click();
  assert.equal(await launch.getByRole('combobox', { name: '启动 Agent' }).inputValue(), 'claude_chat');
  await firstRun.close();
  assert.deepEqual(errors,[]);
  console.log('PASS: real App launch routes for six CLIs and seven Chats, directory selection, mode controls, pending dedup/close guard, saved defaults, dark/light and narrow layouts');
} finally { await browser.close(); }
