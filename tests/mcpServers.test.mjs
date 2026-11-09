import assert from "node:assert/strict";
import test from "node:test";
import {
  draftFromServer,
  draftToUpsertInput,
  EMPTY_DRAFT,
  formatEntries,
  parseArgsText,
  parseEntries,
  validateDraft,
  MCP_PREVIEW_SERVERS,
} from "../src/mcpServers.ts";

test("parseEntries 按 KEY=VALUE 逐行解析，分隔符只取首个", () => {
  const { entries, error } = parseEntries("API_KEY=abc=def\r\nEMPTY=\n", "=");
  assert.equal(error, null);
  assert.deepEqual(entries, { API_KEY: "abc=def", EMPTY: "" });

  const missing = parseEntries("没有分隔符的一行", "=");
  assert.match(missing.error, /第 1 行格式无效/);
});

test("parseEntries 用冒号分隔请求头，坏行返回行号错误", () => {
  const ok = parseEntries("Authorization: Bearer token\nX-Trace: 1", ": ");
  assert.equal(ok.error, null);
  assert.deepEqual(ok.entries, { Authorization: "Bearer token", "X-Trace": "1" });

  const bad = parseEntries("Authorization=token", ": ");
  assert.match(bad.error, /第 1 行格式无效/);
});

test("parseArgsText 去空行去首尾空白", () => {
  assert.deepEqual(parseArgsText("  -y  \n\n @modelcontextprotocol/server-filesystem \n. \n"), [
    "-y",
    "@modelcontextprotocol/server-filesystem",
    ".",
  ]);
});

test("draft 与 server 配置互转保持无损", () => {
  const server = MCP_PREVIEW_SERVERS[1]; // stdio filesystem
  const draft = draftFromServer(server);
  assert.equal(draft.transport, "stdio");
  assert.equal(draft.argsText, "-y\n@modelcontextprotocol/server-filesystem\n.");
  const input = draftToUpsertInput(draft);
  assert.equal(input.id, server.id);
  assert.deepEqual(input.args, server.args);
});

test("draftToUpsertInput 按传输类型裁剪无关字段", () => {
  const stdio = draftToUpsertInput({
    ...EMPTY_DRAFT,
    name: "fs",
    command: "npx",
    argsText: "-y\nserver",
    envText: "K=V",
    url: "https://should-drop.example.com",
    headersText: "Authorization: none",
  });
  assert.equal(stdio.command, "npx");
  assert.deepEqual(stdio.args, ["-y", "server"]);
  assert.deepEqual(stdio.env, { K: "V" });
  assert.equal(stdio.url, null);
  assert.deepEqual(stdio.headers, {});

  const http = draftToUpsertInput({
    ...EMPTY_DRAFT,
    transport: "http",
    name: "kb",
    url: "https://example.com/mcp",
    headersText: "Authorization: Bearer t",
    command: "should-drop",
    envText: "K=V",
  });
  assert.equal(http.command, null);
  assert.deepEqual(http.args, []);
  assert.deepEqual(http.env, {});
  assert.equal(http.url, "https://example.com/mcp");
  assert.deepEqual(http.headers, { Authorization: "Bearer t" });
});

test("validateDraft 校验名称、必填项与 URL 协议", () => {
  assert.match(validateDraft(EMPTY_DRAFT, []), /名称不能为空/);

  const noCommand = validateDraft({ ...EMPTY_DRAFT, name: "fs" }, []);
  assert.match(noCommand, /STDIO 服务器必须填写启动命令/);

  const badUrl = validateDraft({
    ...EMPTY_DRAFT,
    transport: "http",
    name: "kb",
    url: "ftp://bad",
  }, []);
  assert.match(badUrl, /http:\/\/ 或 https:\/\//);

  const good = validateDraft({
    ...EMPTY_DRAFT,
    name: "kb",
    transport: "http",
    url: "https://example.com/mcp",
  }, []);
  assert.equal(good, null);
});

test("validateDraft 拒绝同名服务器但允许编辑自身", () => {
  const existing = MCP_PREVIEW_SERVERS;
  const duplicate = validateDraft(
    { ...EMPTY_DRAFT, name: "filesystem", command: "npx" },
    existing,
  );
  assert.match(duplicate, /已存在同名/);

  const editing = validateDraft(
    { ...EMPTY_DRAFT, id: existing[1].id, name: "filesystem", command: "npx" },
    existing,
  );
  assert.equal(editing, null);
});

test("formatEntries 与 parseEntries 互逆", () => {
  const text = "A=1\nB=2";
  assert.equal(formatEntries({ A: "1", B: "2" }, "="), text);
  assert.deepEqual(parseEntries(text, "=").entries, { A: "1", B: "2" });
});

test("编辑保存保留空参数、首尾空格、换行及字面引号", () => {
  const server = { ...MCP_PREVIEW_SERVERS[1], args: ["", " spaced ", "line\nbreak", '"quoted"', "normal", "C:\\directory"] };
  assert.deepEqual(draftToUpsertInput(draftFromServer(server)).args, server.args);
});

test("切换传输类型不被隐藏且将被裁剪的字段阻挡", () => {
  const http = { ...EMPTY_DRAFT, name: "remote", transport: "http", url: "https://example.com/mcp", command: "npx", envText: "old invalid env" };
  assert.equal(validateDraft(http, []), null);
  assert.equal(draftToUpsertInput(http).command, null);
  const stdio = { ...http, transport: "stdio", envText: "", headersText: "old invalid header" };
  assert.equal(validateDraft(stdio, []), null);
  assert.equal(draftToUpsertInput(stdio).url, null);
});
