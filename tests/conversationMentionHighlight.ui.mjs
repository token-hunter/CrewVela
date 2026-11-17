import assert from 'node:assert/strict';
import {chromium} from 'playwright';
import {mkdirSync} from 'node:fs';
const base=process.env.CREWVELA_UI_URL || 'http://127.0.0.1:1443';
mkdirSync('.scratch/conversation-mention-highlight',{recursive:true});
const browser=await chromium.launch({channel:'chrome'});
const errors=[];
try {
 for(const theme of ['dark','light']) {
  const page=await browser.newPage({viewport:{width:1000,height:1000}});page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&native=1&header=1&theme=${theme}`);
  const editor=page.getByRole('combobox',{name:'给团队发消息',exact:true});
  await editor.fill('@模型验收-Claude @聊天-ZCodeCLI 检查接口\n保留换行');
  await editor.press('Escape');
  const marks=page.locator('.conversation-mention-highlight mark');
  await marks.first().waitFor();assert.deepEqual(await marks.allTextContents(),['@模型验收-Claude','@聊天-ZCodeCLI']);
  assert.equal(await editor.evaluate(e=>getComputedStyle(e).webkitTextFillColor),'rgba(0, 0, 0, 0)');
  assert.equal(await marks.first().evaluate(e=>getComputedStyle(e).backgroundColor==='rgba(0, 0, 0, 0)'),false);
  const metrics=await page.evaluate(()=>{const t=document.querySelector('.conversation-composer textarea'),m=document.querySelector('.conversation-mention-highlight > div'),a=getComputedStyle(t),b=getComputedStyle(m);return[a.font===b.font,a.lineHeight===b.lineHeight,a.padding===b.padding,t.clientWidth===m.getBoundingClientRect().width]});
  assert.ok(metrics.every(Boolean),JSON.stringify(metrics));
  await page.screenshot({path:`.scratch/conversation-mention-highlight/${theme}.png`});
  // Keep native IME underlines/candidate editing visible during composition.
  await editor.dispatchEvent('compositionstart',{data:''});
  assert.equal(await page.locator('.conversation-editor').getAttribute('data-highlighted'),'false');
  assert.notEqual(await editor.evaluate(e=>getComputedStyle(e).webkitTextFillColor),'rgba(0, 0, 0, 0)');
  await editor.dispatchEvent('keydown',{key:'Enter',keyCode:229,isComposing:true});
  assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_send').length),0);
  await editor.dispatchEvent('compositionend',{data:'中文'});await marks.first().waitFor();
  await editor.fill('@模型验收-Claude');await editor.press('End');await editor.press('Backspace');
  assert.equal(await marks.count(),0);
  await editor.press('e');await marks.first().waitFor();
  await editor.press('ControlOrMeta+A');await editor.press('Backspace');
  assert.equal(await editor.inputValue(),'');assert.equal(await marks.count(),0);
  await editor.fill('@模型验收-Claude');await page.emulateMedia({forcedColors:'active'});
  assert.notEqual(await editor.evaluate(e=>getComputedStyle(e).webkitTextFillColor),'rgba(0, 0, 0, 0)');
  await page.emulateMedia({forcedColors:'none'});
  await editor.fill('mail@模型验收-Claude @模型验收-Claude-extra @未知成员');
  assert.equal(await marks.count(),0);
  await editor.fill('@模型验收-');assert.equal(await marks.count(),0);
  await page.getByRole('option',{name:/@模型验收-Claude/}).click();
  await marks.first().waitFor();
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  await editor.press('Shift+Enter');await editor.pressSequentially('test');
  assert.ok((await editor.inputValue()).includes('\ntest'));
  // Long scrolling content: the mirror follows the exact native scroll offset.
  const long=Array.from({length:35},(_,i)=>`第${i}行 @模型验收-Claude ${'文字'.repeat(10)}`).join('\n');
  await editor.fill(long);await editor.press('Escape');
  await editor.evaluate(e=>{e.scrollTop=e.scrollHeight;e.dispatchEvent(new Event('scroll'));});
  const scroll=await page.evaluate(()=>{const e=document.querySelector('.conversation-composer textarea'),m=document.querySelector('.conversation-mention-highlight > div');return{top:e.scrollTop,transform:getComputedStyle(m).transform}});
  assert.ok(scroll.top>0);assert.ok(scroll.transform.includes(String(-scroll.top)));
  await page.setViewportSize({width:390,height:844});
  await editor.fill('@模型验收-Claude @聊天-ZCodeCLI\n检查接口');await editor.press('Escape');
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  const align=await page.evaluate(()=>{const e=document.querySelector('.conversation-composer textarea'),h=document.querySelector('.conversation-mention-highlight');return{input:e.clientWidth,mirror:h.clientWidth}});assert.equal(align.input,align.mirror);
  if(theme==='dark')await page.screenshot({path:'.scratch/conversation-mention-highlight/narrow.png'});
  await editor.fill('@模型验收-Claude 检查接口\n保留换行');await editor.press('Escape');
  const value=await editor.inputValue();await page.getByRole('button',{name:'发送消息',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.conversationMock.calls.filter(c=>c.command==='ma_conversation_send').at(-1).args.input.body),value);
  await page.close();
 }
 assert.deepEqual(errors,[]);
 console.log('PASS mention highlighting: exact members, native editing/IME, line breaks, scroll/resize alignment, dark/light/narrow, unchanged send payload');
} finally {await browser.close();}
