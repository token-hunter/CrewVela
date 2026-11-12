import { initializeStorage } from "./initializeStorage";
import ReactDOM from "react-dom/client";
import "./styles.css";
// Cowork 风格层：紧跟全局样式导入，所有窗口共用；与组件样式冲突时靠更高优先级取胜。
import "./styles/cowork.css";

// 不使用 StrictMode：dev 下它会双重 mount/unmount useEffect，
// 导致 Tauri event 的 listen 注册/注销时序错乱，丢事件。
const view =
  new URLSearchParams(window.location.search).get("view") ??
  (window.location.hash === "#notification" ? "notification" : null);
document.documentElement.dataset.view = view ?? "main";
void initializeStorage().then(async () => {
  const [{ default: App }, { ControlWindow }, { NotificationOverlay }] = await Promise.all([
    import("./App"), import("./components/ControlWindow"), import("./components/NotificationOverlay"),
  ]);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    view === "notification" || view === "notification-preview" ? (
      <NotificationOverlay />
    ) : view === "control" ? (
      <ControlWindow />
    ) : (
      <App />
    ),
  );
}).catch((error: unknown) => {
  console.error("CrewVela 加载失败", error);
  const root = document.getElementById("root");
  if (root) {
    const message = document.createElement("p");
    message.setAttribute("role", "alert");
    message.textContent = "CrewVela 加载失败。原有数据已保留，请检查本地存储后重试。";
    const retry = document.createElement("button");
    retry.textContent = "重试加载";
    retry.addEventListener("click", () => window.location.reload());
    root.replaceChildren(message, retry);
  }
});
