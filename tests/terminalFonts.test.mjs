import test from "node:test";
import assert from "node:assert/strict";
import { missingTerminalFonts, replacePrimaryTerminalFont, splitTerminalFontFamilies } from "../src/terminalFonts.ts";

test("终端字体保留有引号的字体名和现有备用字体", () => {
  const stack = "'Example, Code', Consolas, ui-monospace";
  assert.deepEqual(splitTerminalFontFamilies(stack), ["'Example, Code'", "Consolas", "ui-monospace"]);
  assert.equal(replacePrimaryTerminalFont(stack, "New, Code"), '"New, Code", Consolas, ui-monospace');
  assert.equal(replacePrimaryTerminalFont(stack, ""), "");
});

test("字体可用性匹配本机别名和大小写，并跳过 CSS 通用字体", () => {
  const catalog = [
    { family: "Consolas", aliases: [], monospace: true },
    { family: "Microsoft YaHei", aliases: ["微软雅黑"], monospace: false },
    { family: "Example, Code", aliases: [], monospace: true },
  ];
  assert.deepEqual(missingTerminalFonts('"Example, Code", 微软雅黑, CONSOLAS, ui-monospace', catalog), []);
  assert.deepEqual(missingTerminalFonts("Unknown Font, Consolas, monospace", catalog), ["Unknown Font"]);
});
