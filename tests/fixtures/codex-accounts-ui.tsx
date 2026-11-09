import { createRoot } from "react-dom/client";
import { CodexAccountsPanel } from "../../src/components/CodexAccountsPanel";
import "../../src/styles.css";

const accounts = [[90, 0], [0, 90], [80, 7], [null, null], [80, 50]].map(([weekly, fiveHour], index) => ({
  index: index + 1, account: `fixture-${index}@example.com`, plan: "Team",
  nextUsedPercent: fiveHour, totalUsedPercent: weekly, nextReset: null, totalReset: null,
  current: false, localStatus: null, refreshError: null,
}));
let switchCount = 0;
(window as any).__accountSwitchCount = () => switchCount;
(window as any).__TAURI_INTERNALS__ = { invoke: (command: string, args: { selector?: string } = {}) => {
  const snapshot = () => ({ installed: true, warnings: [], message: null, accounts });
  if (command === "cid_list_accounts") return Promise.resolve(snapshot());
  if (command === "cid_use_account") {
    switchCount += 1;
    accounts.forEach(account => { account.current = account.account === args.selector; });
    return Promise.resolve({ snapshot: snapshot(), message: null });
  }
  if (command === "restart_codex_app") return Promise.resolve("Codex 已重新打开。");
  return Promise.reject(new Error(`Unexpected fixture command: ${command}`));
} };
createRoot(document.getElementById("root")!).render(<CodexAccountsPanel />);
