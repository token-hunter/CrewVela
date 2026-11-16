import assert from "node:assert/strict";
import { chromium } from "playwright";
import { mkdirSync, writeFileSync } from "node:fs";

const base=process.env.CREWVELA_UI_URL||"http://127.0.0.1:1437";
const out=".scratch/chat-connection-copy-20261116";mkdirSync(out,{recursive:true});
const browser=await chromium.launch({channel:"chrome",headless:true});
const errors=[],results=[];
const chats=[
 ["claude","start_claude_chat_session","send_claude_chat_message"],["codex","start_codex_chat_session","send_codex_chat_message"],
 ["kimi","start_kimi_chat_session","send_kimi_chat_message"],["opencode","start_opencode_chat_session","send_opencode_chat_message"],
 ["pi","start_pi_chat_session","send_pi_chat_message"],["harness","start_deepseek_harness_session","send_deepseek_harness_message"],["zcode","start_zcode_session","send_zcode_message"],
];
const protocol=/\b(?:ACP|RPC|RCP)\b/i;
async function checkChrome(page,kind) {
  const section=page.locator("section.native-agent-chat");
  assert.doesNotMatch(await section.getAttribute("aria-label"),protocol,`${kind}: accessible session name`);
  assert.doesNotMatch(await page.locator(".native-agent-welcome").innerText(),protocol,`${kind}: welcome copy`);
  assert.equal(await page.locator(".native-agent-welcome > p").count(),0,`${kind}: omit the welcome subtitle entirely`);
  assert.equal(await page.locator(".native-agent-welcome > h1").count(),1,`${kind}: keep the Agent name`);
  assert.equal(await page.locator(".native-agent-welcome > button.native-agent-skills").count(),1,`${kind}: keep the commands/skills entry`);
  assert.equal(await page.locator(".harness-chat-info").filter({hasText:/正在连接/}).count(),0,`${kind}: remove the duplicate connection banner`);
}
try {
 for(const theme of ["dark","light"])for(const [kind,command,sendCommand] of chats) {
  const page=await browser.newPage({viewport:{width:1200,height:900}});page.setDefaultTimeout(10000);
  page.on("pageerror",error=>errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/native-chat-connection-ui.html?kind=${kind}&connection=hold`);
  await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
  await page.locator("section.native-agent-chat").waitFor();
  if(kind==="zcode")await page.getByRole("button",{name:"恢复会话",exact:true}).click();
  await page.waitForFunction(command=>window.asyncChatMock.pending.some(item=>item.command===command),command);
  // Exact symptom: no standalone connection implementation banner during startup.
  assert.equal(await page.locator(".harness-chat-info").filter({hasText:/正在连接/}).count(),0,`${kind}: startup should not display connection implementation details`);
  await checkChrome(page,kind);
  assert.equal(await page.locator("textarea[aria-label^='发送给']").isDisabled(),true,`${kind}: connection state still controls input`);
  if(kind==="harness")await page.screenshot({path:`${out}/connecting-${theme}.png`});
  await page.evaluate(command=>window.asyncChatMock.release(command,"登录状态已失效，请登录后重试"),command);
  await page.getByRole("alert").filter({hasText:"登录状态已失效"}).waitFor();
  assert.equal(await page.getByRole("button",{name:"恢复会话",exact:true}).isEnabled(),true);
  assert.doesNotMatch(await page.locator(".harness-chat-info").allTextContents().then(text=>text.join(" ")),protocol);
  await page.getByRole("button",{name:"恢复会话",exact:true}).click();
  await page.waitForFunction(command=>window.asyncChatMock.pending.some(item=>item.command===command),command);
  await page.evaluate(command=>window.asyncChatMock.release(command),command);
  await page.waitForFunction(()=>!document.querySelector("textarea[aria-label^='发送给']").disabled);
  assert.equal(await page.getByRole("alert").filter({hasText:"登录状态已失效"}).count(),0);
  await checkChrome(page,kind);
  assert.equal(await page.getByRole("button",{name:"停止会话",exact:true}).isEnabled(),true);
  const editor=page.locator("textarea[aria-label^='发送给']"),send=page.getByRole("button",{name:"发送",exact:true});
  await editor.fill("保留这条消息");await send.click();
  await page.waitForFunction(command=>window.asyncChatMock.pending.some(item=>item.command===command),sendCommand);
  await page.locator("form.harness-chat-composer").evaluate(form=>{form.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true}));form.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true}));});
  assert.equal(await page.evaluate(command=>window.asyncChatMock.calls.filter(item=>item.command===command).length,sendCommand),1,`${kind}: in-flight send remains deduplicated`);
  // ZCode restores drafts only for an authoritative pre-dispatch rejection;
  // an unknown IPC failure must remain reviewable to avoid duplicate delivery.
  await page.evaluate(({command,failure})=>window.asyncChatMock.release(command,failure),{command:sendCommand,failure:kind==="zcode"?"ZCODE_INPUT_REJECTED: 发送失败，请重试":"发送失败，请重试"});
  await page.getByRole("alert").filter({hasText:"发送失败，请重试"}).waitFor();
  assert.equal(await editor.inputValue(),"保留这条消息",`${kind}: send failure retains the draft`);
  assert.equal(await send.isEnabled(),true);
  await page.goto(`${base}/tests/fixtures/native-chat-connection-ui.html?kind=${kind}&preview=1`);
  await page.locator("section.native-agent-chat").waitFor();
  const chrome=await page.locator(".harness-chat-preview-banner,.native-agent-welcome").allTextContents();
  assert.doesNotMatch(chrome.join(" "),protocol,`${kind}: previews also avoid protocol jargon`);
  results.push({kind,theme,connecting:true,errorAndRecovery:true,sendDedupAndDraft:true,preview:true});await page.close();
 }
 assert.deepEqual(errors,[]);writeFileSync(`${out}/ui-results.json`,JSON.stringify({results,errors},null,2));
 console.log("PASS: 7 chats × dark/light, hidden connection implementation copy, accessible names/welcome/preview, connection gating, visible failure and recovery");
}catch(error){console.error(JSON.stringify({pageErrors:errors}));throw error;}finally{await browser.close();}
