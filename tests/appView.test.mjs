import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { existingViewPanel } from "../src/appView.ts";

test("视图入口优先当前实例，再复用最近或已有同类实例", () => {
  const tabs=["start","files:first","files:second","git","browser:one","terminal:owned"];
  assert.equal(existingViewPanel("files",tabs,"files:second","files:first"),"files:second");
  assert.equal(existingViewPanel("files",tabs,null,"files:second"),"files:second");
  assert.equal(existingViewPanel("files",tabs,"git","git"),"files:first");
  assert.equal(existingViewPanel("terminal",tabs,"git","git"),"terminal:owned");
});
test("已关闭或无同类实例时才允许创建，不能选择其他类型", () => {
  assert.equal(existingViewPanel("files",["start","files:current"],"files:closed","files:closed"),"files:current");
  assert.equal(existingViewPanel("terminal",["start","files:one"],null,"start"),undefined);
  assert.equal(existingViewPanel("browser",["start","multi_agent:one"],null,"start"),undefined);
});
test("全屏权限仅授予主窗口，不扩大到控制窗口或通知窗口", () => {
  const capability=JSON.parse(readFileSync(new URL("../src-tauri/capabilities/main-view.json",import.meta.url)));
  assert.deepEqual(capability.windows,["main"]);
  assert.deepEqual(new Set(capability.permissions),new Set(["core:window:allow-is-fullscreen","core:window:allow-set-fullscreen","core:window:allow-is-maximized","core:window:allow-unmaximize","core:window:allow-maximize"]));
  const schema=readFileSync(new URL("../src-tauri/gen/schemas/desktop-schema.json",import.meta.url),"utf8");
  for(const permission of capability.permissions)assert.ok(schema.includes(JSON.stringify(permission)),`${permission}: supported by installed Tauri`);
});
