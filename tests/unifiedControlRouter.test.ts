import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveUnifiedControlRoute,
  stripUnifiedControlPrefix,
} from "../src/unifiedControlRouter.ts";

test("普通指令默认走快速指挥", () => {
  assert.equal(resolveUnifiedControlRoute("打开两个 Kimi 会话"), "quick");
  assert.equal(resolveUnifiedControlRoute("检查当前目录并给出建议"), "quick");
});

test("@目标、已选目标和明确项目操作走项目协作", () => {
  assert.equal(resolveUnifiedControlRoute("@ClaudeCode 实现登录页"), "multi_agent");
  assert.equal(resolveUnifiedControlRoute("检查代码", 2), "multi_agent");
  assert.equal(resolveUnifiedControlRoute("暂停当前项目"), "multi_agent");
  assert.equal(resolveUnifiedControlRoute("创建两个协作任务"), "multi_agent");
});

test("显式前缀可以覆盖默认路由并从执行文本中移除", () => {
  assert.equal(resolveUnifiedControlRoute("项目协作：检查状态"), "multi_agent");
  assert.equal(resolveUnifiedControlRoute("快速指挥，暂停当前项目"), "quick");
  assert.equal(stripUnifiedControlPrefix("项目协作：检查状态"), "检查状态");
  assert.equal(stripUnifiedControlPrefix("快速指挥，打开一个会话"), "打开一个会话");
});

