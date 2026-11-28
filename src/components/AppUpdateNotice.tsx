import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { X } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { trustedReleaseUrl, type AppUpdateCheck } from "../appHelp";
import { useTranslation } from "../useInterfaceLanguage";
import "../styles/app-update-notice.css";

type Offer = Exclude<AppUpdateCheck, { status: "unavailable" }>;
type Progress = { phase: "downloading"; received: number; total: number } | { phase: "verifying" };
const restartBlockerMessage = {
  runtimeTransition: "会话正在启动或退出，请稍后重试。",
  agentSession: "请结束运行中或等待确认的 Agent 会话后再安装。",
  terminal: "请关闭仍打开的终端后再安装。",
  projectExecution: "请结束正在执行的项目任务后再安装。",
  teamChat: "请等待团队聊天的当前回复完成后再安装。",
  previewService: "请停止仍在运行的项目预览后再安装。",
  automation: "请等待当前自动化任务结束后再安装。",
} as const;
type RestartBlocker = keyof typeof restartBlockerMessage;
type Notice = { offer: Offer } & (
  | { step: "available" | "retry" | "verifying" | "installing" }
  | { step: "downloading"; percent: number }
  | { step: "ready"; reason: (typeof restartBlockerMessage)[RestartBlocker] | "无法启动安装程序，请重试。" }
);

const CHECK_DELAY = 5000;
const CHECK_INTERVAL = 6 * 60 * 60 * 1000;

export function AppUpdateNotice() {
  const t = useTranslation();
  const [notice, setNotice] = useState<Notice>();
  const dismissed = useRef(new Set<string>());
  const operating = useRef(false);

  useEffect(() => {
    let disposed = false;
    async function check() {
      if (disposed || operating.current) return;
      try {
        const offer = await invoke<AppUpdateCheck>("app_check_update");
        if (disposed || operating.current || offer.status !== "available"
          || !trustedReleaseUrl(offer.releaseUrl) || dismissed.current.has(offer.latestVersion)) return;
        setNotice(previous => previous && previous.step !== "available" ? previous : { offer, step: "available" });
      } catch {
        // Background checks are silent. The Help menu shows network errors on demand.
      }
    }
    const initial = window.setTimeout(() => void check(), CHECK_DELAY);
    const interval = window.setInterval(() => void check(), CHECK_INTERVAL);
    return () => {
      disposed = true;
      window.clearTimeout(initial);
      window.clearInterval(interval);
    };
  }, []);

  if (!notice) return null;
  const { offer, step } = notice;
  const version = offer.latestVersion;
  function dismiss() {
    dismissed.current.add(version);
    setNotice(undefined);
  }
  async function install() {
    setNotice({ offer, step: "installing" });
    try {
      const blocker = await invoke<RestartBlocker | null>("app_install_update", { version });
      if (blocker) setNotice({ offer, step: "ready", reason: restartBlockerMessage[blocker] });
    } catch {
      setNotice({ offer, step: "ready", reason: "无法启动安装程序，请重试。" });
    }
  }
  async function update() {
    if (operating.current) return;
    operating.current = true;
    try {
      if (step === "ready") {
        await install();
        return;
      }
      if (step === "retry") {
        const latest = await invoke<AppUpdateCheck>("app_check_update");
        if (latest.status !== "available" || !trustedReleaseUrl(latest.releaseUrl)) {
          dismiss();
          return;
        }
        if (latest.latestVersion !== version || latest.canInstall !== offer.canInstall) {
          setNotice({ offer: latest, step: "available" });
          return;
        }
      }
      if (!offer.canInstall) {
        await openUrl(offer.releaseUrl);
        dismiss();
        return;
      }
      setNotice({ offer, step: "downloading", percent: 0 });
      const channel = new Channel<Progress>();
      let listening = true;
      channel.onmessage = (event) => {
        if (!listening) return;
        if (event.phase === "verifying") {
          setNotice({ offer, step: "verifying" });
        } else if (event.total > 0) {
          const percent = Math.min(100, Math.floor(event.received * 100 / event.total));
          setNotice({ offer, step: "downloading", percent });
        }
      };
      try {
        await invoke("app_download_update", { version, progress: channel });
      } finally {
        listening = false;
      }
      await install();
    } catch {
      setNotice({ offer, step: "retry" });
    } finally {
      operating.current = false;
    }
  }

  const downloading = step === "downloading";
  const waiting = step === "verifying" || step === "installing";
  const progress = downloading || waiting;
  const heading = step === "downloading" ? "下载中" : step === "verifying" ? "正在校验更新"
    : step === "installing" ? "正在重启安装" : step === "ready" ? "更新已下载"
    : step === "retry" ? "更新未完成" : "有新版本";
  const action = step === "ready" ? "重启安装" : step === "retry" ? "重试"
    : offer.canInstall ? "更新" : "发布页";

  return (
    <section className="app-update-notice" aria-label={t("CrewVela 更新提醒")}>
      <div className="app-update-notice-row">
        <strong role="status">{t(heading)}</strong>
        {downloading && <span className="app-update-notice-percent" aria-hidden="true">{notice.percent}%</span>}
        {!progress && <button type="button" className="app-update-notice-dismiss" aria-label={t("稍后更新")} onClick={dismiss}><X size={15} aria-hidden="true" /></button>}
      </div>
      {progress ? (
        <progress className="app-update-notice-progress" max={100} value={downloading ? notice.percent : undefined} aria-label={t(heading)} />
      ) : (
        <div className="app-update-notice-row">
          <span className="app-update-notice-version" title={version}>v{version}</span>
          <button type="button" className="app-update-notice-action" onClick={() => void update()}>{t(action)}</button>
        </div>
      )}
      {step === "ready" && <p role="status">{t(notice.reason)}</p>}
      {step === "retry" && <button className="app-update-notice-release" type="button" onClick={() => void openUrl(offer.releaseUrl).then(dismiss).catch(() => { setNotice({ offer, step: "retry" }); })}>{t("查看官方发布页")}</button>}
    </section>
  );
}
