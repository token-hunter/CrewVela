// Runs unchanged in Chromium and the system WKWebView diagnostic runner.
export async function checkSidebarStartLayout({ fullApp = false, cycles = 10 } = {}) {
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const wait = async (predicate, message) => {
    const deadline = Date.now() + 10000;
    while (!predicate()) {
      if (Date.now() > deadline) throw new Error(message);
      await pause(10);
    }
  };
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const cards = () => [...document.querySelectorAll(".right-sidebar-start-tools > button")];
  const toggle = () => document.querySelector(fullApp ? '[aria-label="切换右侧栏"]' : '[data-testid="sidebar-toggle"]');
  await wait(() => toggle(), "sidebar toggle missing");
  const measurements = [];
  for (let cycle = 0; cycle < cycles; cycle++) {
    toggle().click();
    await wait(() => cards().length === 5 && document.querySelector(".unified-right-sidebar-content").getBoundingClientRect().width > 300, "sidebar did not expand");
    await pause(200); // Include completion of the real App's 160ms expansion transition.
    const grid = document.querySelector(".right-sidebar-start-tools");
    const heights = cards().map(button => button.getBoundingClientRect().height);
    assert(heights.every(height => height >= 44 && height <= 48), `cycle ${cycle}: incorrect menu rows ${heights}`);
    assert(grid.getBoundingClientRect().height <= 240, `cycle ${cycle}: inflated entry list`);
    const hostRect = document.querySelector(".right-sidebar-start").getBoundingClientRect();
    const menuRect = grid.getBoundingClientRect();
    if (hostRect.height >= 500) {
      const center = (menuRect.top + menuRect.height / 2 - hostRect.top) / hostRect.height;
      assert(center >= 0.44 && center <= 0.49, `cycle ${cycle}: menu not centered slightly above midpoint (${center})`);
    }
    assert(cards().every(button => {
      const text = button.querySelector("span");
      return text.getBoundingClientRect().height <= button.clientHeight;
    }), `cycle ${cycle}: clipped text`);
    measurements.push(heights);
    toggle().click();
    await wait(() => !document.querySelector(".right-sidebar-start"), "start page did not unmount on collapse");
  }
  // Also exercise the five real component callbacks and the no-workspace state.
  if (!fullApp) {
    toggle().click();
    await wait(() => cards().length === 5 && document.querySelector(".unified-right-sidebar-content").getBoundingClientRect().width > 300, "fixture did not reopen");
    const expected = ["files", /mac/i.test(navigator.platform) ? "zsh" : "powershell", "browser", "git", "multi_agent"];
    for (let index = 0; index < expected.length; index++) {
      cards()[index].click();
      await wait(() => document.querySelector('[data-testid="last-tool"]').textContent === expected[index], `entry ${index} callback failed`);
    }
    document.querySelector('[aria-label="有工作区"]').click();
    await wait(() => cards()[0].disabled && cards()[1].disabled, "workspace-dependent entries not disabled");
    assert(cards().slice(2).every(button => !button.disabled), "independent entries disabled");
    const label = cards()[0].querySelector("span");
    label.textContent = "较长的工作区入口名称，应按内容增高而不是裁切。".repeat(12);
    await pause(50);
    assert(cards()[0].getBoundingClientRect().height > 48, "long label cannot expand row");
    assert(label.getBoundingClientRect().height <= cards()[0].clientHeight, "long label clipped");
  }
  return { cycles, measurements, callbacks: !fullApp, adaptiveHeight: !fullApp };
}
