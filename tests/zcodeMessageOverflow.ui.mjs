// Run against Vite: CREWVELA_UI_URL=http://127.0.0.1:1420 node tests/zcodeMessageOverflow.ui.mjs
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const baseUrl = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
const page = await browser.newPage({ viewport: { width: 820, height: 900 } });
page.setDefaultTimeout(10000);

try {
  await page.goto(new URL("/tests/fixtures/zcode-message-overflow.html", baseUrl).href);
  const messages = page.locator(".harness-chat-messages");
  const summary = page.locator(".zcode-tool > summary");
  const details = page.locator(".zcode-tool-details");
  const output = page.locator("#tool-output");
  await output.evaluate((element, text) => { element.textContent = text; }, JSON.stringify({
    snapshot: "accessibility-node-".repeat(90),
    items: ["可访问元素", "按钮", "文本", "选择条目"],
  }, null, 2));
  if ((await output.textContent()).length < 1500) {
    throw new Error("fixture must exercise a long tool output");
  }

  const measure = () => page.evaluate(() => {
    const messages = document.querySelector(".harness-chat-messages");
    const column = document.querySelector(".zcode-message-column");
    const summary = document.querySelector(".zcode-tool > summary");
    const output = document.querySelector("#tool-output");
    const title = summary.querySelector("strong");
    const children = [...summary.children].map((child) => ({
      name: child.tagName,
      text: child.textContent,
      width: Math.round(child.getBoundingClientRect().width),
      topOffset: Math.round(child.getBoundingClientRect().top - summary.getBoundingClientRect().top),
      scrollWidth: child.scrollWidth,
      whiteSpace: getComputedStyle(child).whiteSpace,
      overflowWrap: getComputedStyle(child).overflowWrap,
    }));
    return {
      viewportWidth: document.documentElement.clientWidth,
      messagesClientWidth: messages.clientWidth,
      messagesScrollWidth: messages.scrollWidth,
      pageClientWidth: document.documentElement.clientWidth,
      pageScrollWidth: document.documentElement.scrollWidth,
      columnClientWidth: column.clientWidth,
      columnScrollWidth: column.scrollWidth,
      summaryClientWidth: summary.clientWidth,
      summaryScrollWidth: summary.scrollWidth,
      titleTopOffset: Math.round(title.getBoundingClientRect().top - summary.getBoundingClientRect().top),
      summaryPaddingTop: Number.parseFloat(getComputedStyle(summary).paddingTop),
      summaryChildren: children,
      outputClientWidth: output.clientWidth,
      outputScrollWidth: output.scrollWidth,
      outputClientHeight: output.clientHeight,
      outputScrollHeight: output.scrollHeight,
      outputOverflowY: getComputedStyle(output).overflowY,
    };
  });

  const expectNoHorizontalOverflow = async (width, phase) => {
    await page.setViewportSize({ width, height: 900 });
    const layout = await measure();
    console.log(`Layout at ${width}px (${phase}):`, JSON.stringify(layout));
    if (layout.pageScrollWidth > layout.pageClientWidth) {
      throw new Error(`page has horizontal overflow at ${width}px: ${layout.pageScrollWidth}px > ${layout.pageClientWidth}px`);
    }
    if (layout.messagesScrollWidth > layout.messagesClientWidth) {
      throw new Error(`message area has horizontal overflow at ${width}px: ${layout.messagesScrollWidth}px > ${layout.messagesClientWidth}px`);
    }
    if (layout.columnScrollWidth > layout.columnClientWidth || layout.summaryScrollWidth > layout.summaryClientWidth) {
      throw new Error(`message content has horizontal overflow at ${width}px: ${JSON.stringify(layout)}`);
    }
    if (layout.titleTopOffset > layout.summaryPaddingTop + 8) {
      throw new Error(`tool disclosure arrow occupies a separate row at ${width}px: ${layout.titleTopOffset}px offset`);
    }
    if (layout.summaryChildren.some((child) => child.scrollWidth > child.width + 1)) {
      throw new Error(`tool title, status, or MCP label is clipped at ${width}px: ${JSON.stringify(layout.summaryChildren)}`);
    }
    if (layout.summaryChildren[1].topOffset > layout.summaryChildren[0].topOffset + 2) {
      throw new Error(`tool status is detached from the title row at ${width}px: ${JSON.stringify(layout.summaryChildren)}`);
    }
    return layout;
  };

  await expectNoHorizontalOverflow(820, "collapsed");

  await summary.click();
  if (await page.locator(".zcode-tool").getAttribute("open") === null) {
    throw new Error("tool details did not open from its summary");
  }
  await details.waitFor({ state: "visible" });
  const afterOpen = await expectNoHorizontalOverflow(820, "expanded");
  if (afterOpen.outputScrollWidth > afterOpen.outputClientWidth) {
    throw new Error(`long tool output is not wrapped: ${afterOpen.outputScrollWidth}px > ${afterOpen.outputClientWidth}px`);
  }
  if (afterOpen.outputScrollHeight <= afterOpen.outputClientHeight || !["auto", "scroll"].includes(afterOpen.outputOverflowY)) {
    throw new Error(`long tool output is not vertically scrollable: ${JSON.stringify(afterOpen)}`);
  }

  mkdirSync("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/zcode-message-overflow-fixed.png", fullPage: true });
  await expectNoHorizontalOverflow(1440, "wide desktop");
  await expectNoHorizontalOverflow(1024, "intermediate desktop");
  await expectNoHorizontalOverflow(600, "container at or below 650px");
  console.log("PASS: tool summary, expanded details, and long output stay within the ZCode message area");
} finally {
  await browser.close();
}
