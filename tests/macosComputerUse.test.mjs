import test from "node:test";
import assert from "node:assert/strict";
import { macControlLabel, permissionLabel } from "../src/macosComputerUse.ts";

const installed = { supported: true, installed: true, executable: "/Applications/Fixture.app/Contents/MacOS/tool", mcpRegistered: true, accessibility: null, screenRecording: null, serviceRunning: null, detail: "fixture" };
test("安装和 MCP 注册不能冒充已授权或已连接", () => {
  assert.equal(macControlLabel(installed), "已安装 · 权限待检查");
  assert.equal(macControlLabel({ ...installed, accessibility: true, screenRecording: true }), "权限已授予 · 工具连接待验证");
});
test("缺失与不支持当前架构分别显示", () => {
  assert.equal(macControlLabel(undefined), "尚未检测");
  assert.equal(macControlLabel({ ...installed, installed: false }), "未安装或安装无效");
  assert.equal(macControlLabel({ ...installed, supported: false }), "不支持当前系统或架构");
});
test("拒绝、未知和后台服务停止保留真实状态", () => {
  assert.equal(macControlLabel({ ...installed, accessibility: false, screenRecording: true }), "待授权");
  assert.equal(macControlLabel({ ...installed, accessibility: true, screenRecording: true, serviceRunning: false }), "后台服务未运行");
  assert.equal(permissionLabel(undefined), "待检查");
  assert.equal(permissionLabel(null), "待检查");
  assert.equal(permissionLabel(false), "待授权");
  assert.equal(permissionLabel(true), "已授予");
});
