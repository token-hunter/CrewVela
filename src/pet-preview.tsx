import React from "react";
import ReactDOM from "react-dom/client";
import { TerminalPane } from "./components/TerminalPane";
import type { AppTheme, SessionStatus } from "./types";
import "./styles.css";

const query = new URLSearchParams(window.location.search);
const theme: AppTheme = query.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;

const demos: Array<{ id: number; title: string; status: SessionStatus }> = [
  { id: 101, title: "重构会话状态", status: "running" },
  { id: 102, title: "整理发布说明", status: "waiting" },
  { id: 103, title: "修复终端配色", status: "completed" },
  { id: 104, title: "准备新任务", status: "pending" },
];

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <main
      className="grid"
      style={{
        gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
        height: "100vh",
        borderRadius: 0,
      }}
    >
      {demos.map((demo) => (
        <TerminalPane
          key={demo.id}
          sessionId={demo.id}
          title={demo.title}
          providerName="GLM"
          kind="claude"
          shell={null}
          cwd="C:\Users\demo"
          status={demo.status}
          theme={theme}
          hidden={false}
          onClose={() => {}}
        />
      ))}
    </main>
  </React.StrictMode>,
);
