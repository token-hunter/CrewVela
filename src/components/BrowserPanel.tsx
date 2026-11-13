import {
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IS_MAC } from "../platform";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowClockwise";
import { ArrowLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowLeft";
import { ArrowRightIcon } from "@phosphor-icons/react/dist/csr/ArrowRight";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import {
  loadBrowserPanelState,
  normalizeAddressInput,
  saveBrowserPanelState,
} from "../browserPanelState";
import { hasOpenModal } from "./ui/modalStack";

interface Props {
  active: boolean;
  panelId?: string;
}

interface HostRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function hostRectOf(element: HTMLElement | null): HostRect | null {
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return null;
  return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
}

function sameRect(a: HostRect, b: HostRect): boolean {
  return (
    Math.abs(a.x - b.x) < 0.5 &&
    Math.abs(a.y - b.y) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

/**
 * 右侧栏浏览器面板：内容区是 Rust 侧 browser_panel 子 webview 的原生宿主矩形。
 * 原生 HWND 恒在主 webview 之上，CSS 无法遮挡——切走标签页、关栏、全屏弹窗
 * 打开时必须显式 hide，回归时重新对位显示。
 */
export function BrowserPanel({ active, panelId }: Props) {
  const panelInvoke = useCallback(<T,>(command: string, args: Record<string, unknown> = {}) => invoke<T>(command, { ...args, panelId }), [panelId]);
  const persistUrl = (url: string) => { if (!panelId) saveBrowserPanelState({ url }); };
  const [started, setStarted] = useState(() => (panelId ? "" : loadBrowserPanelState().url) !== "");
  const [address, setAddress] = useState(() => (panelId ? "" : loadBrowserPanelState().url));
  const [currentUrl, setCurrentUrl] = useState(() => (panelId ? "" : loadBrowserPanelState().url));
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const hostRef = useRef<HTMLDivElement | null>(null);
  const addressInputRef = useRef<HTMLInputElement | null>(null);
  const addressFocusedRef = useRef(false);
  const activeRef = useRef(active);
  // useRef 初值只在首渲染取一次：与持久化恢复点一致。
  const startedRef = useRef(started);
  const currentUrlRef = useRef(currentUrl);
  const createdRef = useRef(false);
  const openingRef = useRef(false);
  const disposedRef = useRef(false);
  useEffect(() => {
    disposedRef.current = false;
    return () => { disposedRef.current = true; };
  }, []);
  const overlayRef = useRef(false);
  const lastRectRef = useRef<HostRect | null>(null);
  const syncFrameRef = useRef(0);

  const syncBounds = useCallback(() => {
    if (!activeRef.current || !startedRef.current || !createdRef.current || overlayRef.current) {
      return;
    }
    const next = hostRectOf(hostRef.current);
    if (!next) return;
    if (lastRectRef.current && sameRect(lastRectRef.current, next)) return;
    lastRectRef.current = next;
    void panelInvoke("browser_panel_set_bounds", { ...next }).catch(() => {
      // A failed request has not applied this rectangle. Let the periodic
      // synchronizer retry, without invalidating a newer request's rectangle.
      if (lastRectRef.current === next) lastRectRef.current = null;
    });
  }, [panelInvoke]);

  // 激活/失活：失活或全屏弹窗打开 → 隐藏；激活 → 按当前矩形创建或显示。
  // 创建动作抽成可重试函数：首挂载同帧激活时宿主 rect 可能尚未布局（返回
  // null），此后 effect 依赖不再变化，必须由 ResizeObserver 首帧兜底重试，
  // 否则面板永远创建不出来（方案 A 自动激活路径实测踩中）。
  const openOrCreatePanel = useCallback(() => {
    if (disposedRef.current || !startedRef.current) return;
    if (!activeRef.current || overlayRef.current) {
      void panelInvoke("browser_panel_set_visible", { visible: false }).catch(() => {});
      return;
    }
    const rect = hostRectOf(hostRef.current);
    if (!rect) return; // 布局尚未就绪，等 ResizeObserver 首帧重试。
    if (openingRef.current) return;
    openingRef.current = true;
    void panelInvoke("browser_panel_open", { url: currentUrlRef.current, ...rect })
      .then(() => {
        if (disposedRef.current) {
          void panelInvoke("browser_panel_close").catch(() => {});
          return;
        }
        createdRef.current = true;
        setError("");
        // open 在后台创建完会显示；等待期间可能已切走或打开弹窗，按最新状态补隐藏。
        overlayRef.current = hasOpenModal();
        if (!activeRef.current || overlayRef.current) {
          void panelInvoke("browser_panel_set_visible", { visible: false }).catch(() => {});
          return;
        }
        syncBounds();
        // open 对已存在的面板只做 show；若期间地址已被用户改走，补一次导航。
        void panelInvoke<string>("browser_panel_current_url")
          .then((live) => {
            if (disposedRef.current || typeof live !== "string" || !live) return;
            if (live !== currentUrlRef.current) {
              void panelInvoke("browser_panel_navigate", { url: currentUrlRef.current }).catch(() => {});
            }
          })
          .catch(() => {});
      })
      .catch((reason) => {
        if (!disposedRef.current) setError(`打开浏览器面板失败：${String(reason)}`);
      })
      .finally(() => {
        openingRef.current = false;
      });
  }, [syncBounds, panelInvoke]);

  // WKWebView 的显隐仍由前端统一控制，Agent 不得把原生视图显示在设置弹窗上方。
  useEffect(() => {
    if (!IS_MAC || panelId) return;
    let disposed = false;
    const activate = listen<{ url?: string }>("browser-control-panel-needed", (event) => {
      overlayRef.current = hasOpenModal();
      if (!startedRef.current) {
        const url = event.payload?.url;
        if (url) { currentUrlRef.current = url; setCurrentUrl(url); setAddress(url); }
        startedRef.current = true; setStarted(true);
      }
      requestAnimationFrame(() => openOrCreatePanel());
    });
    // 兼容首次激活事件早于组件挂载的时序；查询不会创建页面。
    if (!startedRef.current) void panelInvoke<string>("browser_panel_current_url").then(url => {
      if (disposed || !url || startedRef.current) return;
      currentUrlRef.current = url; setCurrentUrl(url); setAddress(url);
      saveBrowserPanelState({ url }); startedRef.current = true; setStarted(true);
      overlayRef.current = hasOpenModal();
      requestAnimationFrame(() => openOrCreatePanel());
    }).catch(() => {});
    const closed = listen("browser-panel-agent-closed", () => {
      createdRef.current = false;
      startedRef.current = false;
      currentUrlRef.current = "";
      lastRectRef.current = null;
      saveBrowserPanelState({ url: "" });
      setStarted(false); setCurrentUrl(""); setAddress(""); setLoading(false); setError("");
    });
    return () => { disposed = true; void activate.then(dispose => dispose()); void closed.then(dispose => dispose()); };
  }, [openOrCreatePanel, panelInvoke, panelId]);

  useEffect(() => {
    activeRef.current = active;
    if (!started) return;
    const overlayOpen = hasOpenModal();
    overlayRef.current = overlayOpen;
    if (!active || overlayOpen) {
      void panelInvoke("browser_panel_set_visible", { visible: false }).catch(() => {});
      return;
    }
    openOrCreatePanel();
  }, [active, started, openOrCreatePanel]);

  // 尺寸同步：ResizeObserver 抓尺寸变化（拖右栏宽度、窗口缩放）。
  // 面板尚未创建时，首帧回调兼任创建重试（宿主 rect 首次布局完成后）。
  useEffect(() => {
    if (!started) return;
    const host = hostRef.current;
    if (!host || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (syncFrameRef.current) return;
      syncFrameRef.current = requestAnimationFrame(() => {
        syncFrameRef.current = 0;
        if (!createdRef.current) {
          openOrCreatePanel();
          return;
        }
        syncBounds();
      });
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      if (syncFrameRef.current) {
        cancelAnimationFrame(syncFrameRef.current);
        syncFrameRef.current = 0;
      }
    };
  }, [started, syncBounds, openOrCreatePanel]);

  // 低频巡检：全屏弹窗开关 + 位置漂移（左侧栏拖宽只改位置不改尺寸，
  // ResizeObserver 抓不到）。原生 HWND 无法被 CSS 遮罩盖住，弹窗期间必须隐藏。
  // 面板尚未创建时兼任创建重试：冷启动早期 add_child 可能失败（WebView2
  // 环境竞态），open 的失败路径不会自动重来，由巡检兜底直到创建成功。
  useEffect(() => {
    if (!started || !active) return;
    const timer = window.setInterval(() => {
      const overlayOpen = hasOpenModal();
      if (overlayOpen !== overlayRef.current) {
        overlayRef.current = overlayOpen;
        void panelInvoke("browser_panel_set_visible", {
          visible: !overlayOpen && activeRef.current,
        }).catch(() => {});
        return;
      }
      if (!overlayRef.current) {
        if (!createdRef.current) {
          openOrCreatePanel();
          return;
        }
        syncBounds();
      }
    }, 400);
    return () => window.clearInterval(timer);
  }, [started, active, syncBounds, openOrCreatePanel]);

  // 地址栏跟随页面跳转（仅输入框未聚焦时回填），并持久化恢复点。
  useEffect(() => {
    const unlistenNavigated = listen<{ url: string; panelId?: string }>("browser-panel-navigated", (event) => {
      if (event.payload.panelId !== panelId) return;
      const url = event.payload?.url ?? "";
      if (!url) return;
      // 首次工具调用可由后台创建面板；恢复同步、显隐与地址栏状态。
      if (!startedRef.current) {
        startedRef.current = true;
        setStarted(true);
      }
      currentUrlRef.current = url;
      setCurrentUrl(url);
      persistUrl(url);
      if (!addressFocusedRef.current) setAddress(url);
    });
    const unlistenLoad = listen<{ event: string; panelId?: string }>("browser-panel-load", (event) => {
      if (event.payload.panelId !== panelId) return;
      setLoading(event.payload?.event === "started");
    });
    return () => {
      void unlistenNavigated.then((dispose) => dispose());
      void unlistenLoad.then((dispose) => dispose());
    };
  }, [panelInvoke]);

  const runHistory = (action: "back" | "forward" | "reload") => {
    if (!createdRef.current) return;
    void panelInvoke("browser_panel_history", { action }).catch((reason) => {
      setError(`浏览器导航失败：${String(reason)}`);
    });
  };

  const submitAddress = (event: FormEvent) => {
    event.preventDefault();
    const normalized = normalizeAddressInput(address);
    if (!normalized) return;
    setError("");
    currentUrlRef.current = normalized;
    setCurrentUrl(normalized);
    persistUrl(normalized);
    addressInputRef.current?.blur();
    if (!startedRef.current) {
      startedRef.current = true;
      setStarted(true); // 激活路径随后用 currentUrlRef 创建面板。
      return;
    }
    if (createdRef.current) {
      void panelInvoke("browser_panel_navigate", { url: normalized }).catch((reason) => {
        setError(`导航失败：${String(reason)}`);
      });
    }
    // 面板创建进行中：open 完成后的 current_url 对账会补跳。
  };

  // 外部入口（运行时管理"打开 Wiki"等）请求打开指定网址：先落恢复点，
  // 再由激活/创建路径或直接导航完成跳转。
  useEffect(() => {
    const onBrowse = (event: Event) => {
      if (panelId) return;
      const url = (event as CustomEvent<{ url?: string }>).detail?.url;
      if (!url) return;
      setError("");
      currentUrlRef.current = url;
      setCurrentUrl(url);
      setAddress(url);
      persistUrl(url);
      if (!startedRef.current) {
        startedRef.current = true;
        setStarted(true); // App 侧已切到浏览器标签，激活路径用该 URL 创建。
        return;
      }
      if (createdRef.current && activeRef.current) {
        void panelInvoke("browser_panel_navigate", { url }).catch((reason) => {
          setError(`导航失败：${String(reason)}`);
        });
      }
    };
    window.addEventListener("crewvela:browse", onBrowse);
    return () => window.removeEventListener("crewvela:browse", onBrowse);
  }, [panelInvoke]);

  const openExternal = () => {
    const target = currentUrlRef.current || normalizeAddressInput(address);
    if (!target) return;
    void openUrl(target).catch((reason) => {
      setError(`在系统浏览器打开失败：${String(reason)}`);
    });
  };

  const addressKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setAddress(currentUrlRef.current);
      addressInputRef.current?.blur();
    }
  };

  return (
    <div className="browser-panel">
      <div className="browser-toolbar">
        <button
          type="button"
          aria-label="后退"
          title="后退"
          disabled={!started}
          onClick={() => runHistory("back")}
        >
          <ArrowLeftIcon size={16} />
        </button>
        <button
          type="button"
          aria-label="前进"
          title="前进"
          disabled={!started}
          onClick={() => runHistory("forward")}
        >
          <ArrowRightIcon size={16} />
        </button>
        <button
          type="button"
          aria-label="刷新"
          title="刷新"
          disabled={!started}
          onClick={() => runHistory("reload")}
        >
          <ArrowClockwiseIcon size={15} />
        </button>
        <form
          className={`browser-address${loading ? " loading" : ""}`}
          onSubmit={submitAddress}
        >
          <GlobeIcon size={13} className="browser-address-icon" aria-hidden="true" />
          <input
            ref={addressInputRef}
            value={address}
            onChange={(event) => setAddress(event.target.value)}
            onFocus={(event) => {
              addressFocusedRef.current = true;
              event.currentTarget.select();
            }}
            onBlur={() => {
              addressFocusedRef.current = false;
              setAddress(currentUrlRef.current);
            }}
            onKeyDown={addressKeyDown}
            placeholder="搜索或输入网址"
            spellCheck={false}
            aria-label="地址栏"
            type="text"
          />
        </form>
        <button
          type="button"
          aria-label="在系统浏览器打开"
          title="在系统浏览器打开"
          disabled={!currentUrl && !address.trim()}
          onClick={openExternal}
        >
          <ArrowSquareOutIcon size={15} />
        </button>
      </div>
      <div className="browser-host-wrap">
        {!started && (
          <div className="browser-start">
            <GlobeIcon size={34} weight="thin" aria-hidden="true" />
            <p>输入网址或搜索词开始浏览</p>
            <p className="browser-start-hint">登录状态与 Cookie 会跨重启保留</p>
          </div>
        )}
        <div ref={hostRef} className="browser-host" aria-hidden="true" />
      </div>
      {error && (
        <p className="browser-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

export const MemoBrowserPanel = memo(BrowserPanel);
