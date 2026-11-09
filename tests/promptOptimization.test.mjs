import assert from "node:assert/strict";
import test from "node:test";
import {
  optimizationIsUseful,
  validatePromptDraft,
  PROMPT_OPTIMIZER_MAX_CHARS,
  PROMPT_OPTIMIZATION_PREVIEW,
} from "../src/promptOptimization.ts";

test("validatePromptDraft 校验空草稿与超长草稿", () => {
  assert.match(validatePromptDraft("   "), /草稿为空/);
  assert.equal(validatePromptDraft("实现登录页"), null);
  assert.match(
    validatePromptDraft("长".repeat(PROMPT_OPTIMIZER_MAX_CHARS + 1)),
    /草稿过长/,
  );
  assert.equal(validatePromptDraft("长".repeat(PROMPT_OPTIMIZER_MAX_CHARS)), null);
});

test("optimizationIsUseful 过滤无效优化结果", () => {
  assert.equal(optimizationIsUseful("   ", { optimized: "x", notes: "y" }), false);
  assert.equal(
    optimizationIsUseful("实现登录页", { optimized: "  ", notes: "y" }),
    false,
  );
  assert.equal(
    optimizationIsUseful("实现登录页", { optimized: "实现登录页", notes: "补齐" }),
    false,
  );
  assert.equal(
    optimizationIsUseful("实现登录页", { optimized: "实现登录页。", notes: "无实质改动" }),
    false,
  );
  assert.equal(
    optimizationIsUseful("实现登录页", { optimized: "实现一个可用的登录页…", notes: "补齐交付物" }),
    true,
  );
});

test("预览数据本身通过校验", () => {
  assert.equal(validatePromptDraft(PROMPT_OPTIMIZATION_PREVIEW.optimized), null);
  assert.equal(
    optimizationIsUseful("帮登录页加校验", PROMPT_OPTIMIZATION_PREVIEW),
    true,
  );
});
