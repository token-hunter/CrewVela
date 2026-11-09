import test from "node:test";
import assert from "node:assert/strict";
import { quotaFailure, quotaPercent, quotaReset, quotaUpdated, quotaPreview } from "../src/providerQuota.ts";

test("额度百分比明确保留零值，异常值不制造用量", () => {
  assert.equal(quotaPercent(0), "0%");
  assert.equal(quotaPercent(58.123), "58.1%");
  assert.equal(quotaPercent(null), "—");
  assert.equal(quotaPercent(NaN), "—");
});
test("重置倒计时区分分钟、天数和已过期", () => {
  assert.equal(quotaReset(60_000, 0), "1 分钟后重置");
  assert.equal(quotaReset(90_000_000, 0), "1 天 1 小时后重置");
  assert.equal(quotaReset(1, 100), "待重置");
  assert.equal(quotaReset(null), "");
});
test("查询时间和余额展示不混淆本地Token数据", () => {
  assert.equal(quotaUpdated(null), "尚未查询");
  assert.equal(quotaUpdated(0, 0), "刚刚更新");
  assert.equal(quotaUpdated(0, 180_000), "3 分钟前更新");
  assert.equal(quotaPreview("https://api.deepseek.com").balances[0].amount, -0.1);
  assert.equal(quotaPreview("https://proxy.test").status, "unsupported");
});
test("IPC刷新失败保留同账号上次成功数据，未配置不伪造余额", () => {
  const previous = quotaPreview("https://api.deepseek.com");
  const failure = quotaFailure(previous);
  assert.equal(failure.status, "error");
  assert.equal(failure.updatedAt, previous.updatedAt);
  assert.deepEqual(failure.balances, previous.balances);
  assert.deepEqual(quotaFailure(null).balances, []);
  assert.deepEqual(quotaPreview("https://api.kimi.com/coding/", "unconfigured").windows, []);
  assert.equal(quotaPreview("https://api.kimi.com/coding/", "error").status, "error");
});
