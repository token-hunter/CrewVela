import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircleIcon } from "@phosphor-icons/react/dist/csr/CheckCircle";
import orangeMascot from "../assets/notification-orange-mascot.png";

interface NotificationOverlayPayload {
  title: string;
  body: string;
  url: string | null;
  revision: number;
}

declare global {
  interface Window {
    __CREWVELA_NOTIFICATION__?: NotificationOverlayPayload;
  }
}

const AUTO_DISMISS_MS = 6500;

function readInitialNotice(): NotificationOverlayPayload | null {
  if (window.__CREWVELA_NOTIFICATION__) {
    return window.__CREWVELA_NOTIFICATION__;
  }
  const params = new URLSearchParams(window.location.search);
  if (params.get("view") === "notification-preview") {
    return { title: "任务完成", body: "回复 ok", url: null, revision: 1 };
  }
  if (params.get("view") !== "notification") return null;
  return {
    title: params.get("title") || "通知",
    body: params.get("body") || "",
    url: params.get("url") || null,
    revision: Number(params.get("revision")) || 1,
  };
}

export function NotificationOverlay() {
  const t = useTranslation();
  const preview =
    new URLSearchParams(window.location.search).get("view") ===
    "notification-preview";
  const [notice, setNotice] = useState<NotificationOverlayPayload | null>(
    readInitialNotice,
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const actionInFlight = useRef(false);
  const noticeRef = useRef(notice);
  noticeRef.current = notice;
  const closeOverlay = () => {
    void invoke("close_notification_overlay").catch(reason => setError(`关闭通知失败：${String(reason)}`));
  };
  useEffect(() => { setError(null); }, [notice?.revision]);

  useEffect(() => {
    document.documentElement.dataset.view = "notification";
    if (preview) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let subscribeTimer = 0;
    const subscribe = () => {
      void (async () => {
        try {
          const applyNotice = (next: NotificationOverlayPayload) => {
            if (disposed || (noticeRef.current && next.revision < noticeRef.current.revision)) return;
            // Update the identity before React commits: an old click promise
            // may finish in the same batch as this notification event.
            noticeRef.current = next;
            setNotice(next);
          };
          const dispose = await listen<NotificationOverlayPayload>(
            "notification-overlay-updated",
            (event) => applyNotice(event.payload),
          );
          if (disposed) { dispose(); return; }
          unlisten = dispose;
          const current = await invoke<NotificationOverlayPayload | null>(
            "notification_overlay_current",
          );
          if (current) applyNotice(current);
        } catch {
          // 首屏由初始化脚本提供；订阅失败不阻止已有内容显示。
        }
      })();
    };
    subscribeTimer = window.setTimeout(subscribe, 0);
    return () => {
      disposed = true;
      window.clearTimeout(subscribeTimer);
      unlisten?.();
    };
  }, [preview]);

  useEffect(() => {
    if (!notice || preview) return;
    // 隐藏的 WebView 可能暂停 rAF；React 提交内容后再通知后端定位和显示。
    const timer = window.setTimeout(() => {
      void invoke("notification_overlay_ready").catch(() => undefined);
    }, 0);
    return () => window.clearTimeout(timer);
  }, [notice, preview]);

  useEffect(() => {
    if (!notice || preview || busy || error) return;
    const timer = window.setTimeout(
      closeOverlay,
      AUTO_DISMISS_MS,
    );
    return () => window.clearTimeout(timer);
  }, [notice, preview, busy, error]);

  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !preview)
        closeOverlay();
    };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [preview]);

  if (!notice) return null;

  const view = async () => {
    if (preview || actionInFlight.current) return;
    actionInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await invoke("focus_main_from_notification");
      if (notice.url) await openUrl(notice.url);
      if (noticeRef.current?.revision === notice.revision) await invoke("close_notification_overlay");
    } catch (reason) {
      if (noticeRef.current?.revision === notice.revision) setError(`查看通知失败：${String(reason)}`);
    } finally { actionInFlight.current = false; setBusy(false); }
  };

  return (
    <main
      className="notification-overlay-stage"
      role="alert"
      aria-live="assertive"
    >
      <section className="notification-overlay-card" key={notice.revision}>
        <div className="notification-overlay-copy">
          <div className="notification-overlay-title">
            <h1>{notice.title || t("通知")}</h1>
            <CheckCircleIcon size={20} weight="fill" aria-hidden="true" />
          </div>
          <p>{notice.body}</p>
          {error && <p role="alert">{error}</p>}
          <button type="button" disabled={busy} onClick={() => void view()}>
            {t("查看")}
            <span aria-hidden="true">›</span>
          </button>
        </div>
        <img src={orangeMascot} alt="" aria-hidden="true" />
        <span className="notification-overlay-progress" aria-hidden="true" />
      </section>
    </main>
  );
}
