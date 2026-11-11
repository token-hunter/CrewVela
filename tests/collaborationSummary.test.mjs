import assert from "node:assert/strict";
import test from "node:test";
import { summarizeDiagnosticChecks } from "../src/collaborationDiagnostics.ts";

test("精简诊断仍显示全部非零状态，异常优先且不冒充正常", () => {
  assert.equal(summarizeDiagnosticChecks([
    { state: "正常" }, { state: "未验证" }, { state: "异常" }, { state: "正常" },
    { state: "已停止" }, { state: "忙碌" }, { state: "等待授权" },
  ]), "1 项异常 · 1 项等待授权 · 1 项已停止 · 1 项未验证 · 1 项忙碌 · 2 项正常");
});

test("无检查时不显示正常；全部未验证时保留未验证语义", () => {
  assert.equal(summarizeDiagnosticChecks([]), "0 项检查");
  assert.equal(summarizeDiagnosticChecks([{ state: "未验证" }]), "1 项未验证");
});
