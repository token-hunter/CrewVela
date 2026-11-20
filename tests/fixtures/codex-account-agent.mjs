import assert from "node:assert/strict";
import readline from "node:readline";

const scenario = process.argv[2];
const emit = value => process.stdout.write(JSON.stringify(value) + "\n");
let logins = 0;
for await (const line of readline.createInterface({ input: process.stdin })) {
  const { id, method, params } = JSON.parse(line);
  if (method === "initialized") continue;
  if (method === "initialize") { emit({ id, result: {} }); continue; }
  if (method === "account/read") {
    assert.deepEqual(params, { refreshToken: false });
    const account = scenario === "key" ? { type: "apiKey" }
      : (logins && scenario === "login") ? { type: "chatgpt", email: "private@example.invalid" } : null;
    const result = scenario === "unknown" ? {} : { account, requiresOpenaiAuth: scenario !== "gateway" };
    emit({ id, result });
    continue;
  }
  assert.equal(method, "account/login/start");
  assert.deepEqual(params, { type: "chatgpt" });
  assert.equal(++logins, 1);
  const loginId = "fixture-login";
  emit({ method: "account/login/completed", params: { loginId: "unrelated", success: false } });
  const authUrl = scenario === "foreign-url" ? "https://invalid.example/oauth" : "https://auth.openai.com/oauth/authorize";
  emit({ id, result: { type: "chatgpt", loginId, authUrl } });
  emit({ method: "account/login/completed", params: { loginId, success: scenario !== "cancelled" } });
}
