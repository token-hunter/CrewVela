import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function useWindowFullscreen(onError: (message: string) => void) {
  const [fullscreen, setFullscreen] = useState(false);
  const [pending, setPending] = useState(false);
  const mounted = useRef(false);
  const changing = useRef(false);
  const restoreMaximized = useRef(false);
  const revision = useRef(0);
  const native = "__TAURI_INTERNALS__" in window;
  const available = native || document.fullscreenEnabled === true;
  const read = useCallback(async () => {
    const request = ++revision.current;
    const value = native ? await getCurrentWindow().isFullscreen() : Boolean(document.fullscreenElement);
    if (mounted.current && request === revision.current) setFullscreen(value);
  }, [native]);

  useEffect(() => {
    mounted.current = true;
    const refresh = () => { void read().catch(error => onError(`读取全屏状态失败：${String(error)}`)); };
    let unlisten: (() => void) | undefined;
    if (native) {
      void getCurrentWindow().onResized(refresh).then(dispose => {
        if (mounted.current) unlisten = dispose;
        else dispose();
      }).catch(error => onError(`监听全屏状态失败：${String(error)}`));
    } else document.addEventListener("fullscreenchange", refresh);
    refresh();
    return () => {
      mounted.current = false;
      revision.current += 1;
      unlisten?.();
      document.removeEventListener("fullscreenchange", refresh);
    };
  }, [native, onError, read]);

  const toggle = useCallback(async () => {
    if (changing.current || !available) return;
    changing.current = true;
    revision.current += 1;
    setPending(true);
    try {
      if (native) {
        const appWindow = getCurrentWindow();
        if (!await appWindow.isFullscreen()) {
          // Windows' borderless maximized NCCALCSIZE path reserves the taskbar
          // even in fullscreen. Leave maximized state before changing fullscreen.
          const wasMaximized = /win/i.test(navigator.platform) && await appWindow.isMaximized();
          if (wasMaximized) await appWindow.unmaximize();
          restoreMaximized.current = wasMaximized;
          try {
            await appWindow.setFullscreen(true);
          } catch (error) {
            // A failed entry must not leave the user's maximized window restored.
            if (restoreMaximized.current) {
              try {
                if (!await appWindow.isFullscreen()) {
                  await appWindow.maximize();
                  restoreMaximized.current = false;
                }
              } catch (restoreError) {
                throw new Error(`${String(error)}；恢复最大化失败：${String(restoreError)}`);
              }
            }
            throw error;
          }
        } else {
          await appWindow.setFullscreen(false);
          if (restoreMaximized.current) {
            await appWindow.maximize();
            restoreMaximized.current = false;
          }
        }
      } else if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
      await read();
    } catch (error) {
      if (mounted.current) onError(`切换全屏失败：${String(error)}`);
    } finally {
      changing.current = false;
      if (mounted.current) setPending(false);
    }
  }, [available, native, onError, read]);
  return { fullscreen, pending, available, toggle };
}
