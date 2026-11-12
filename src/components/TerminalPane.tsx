import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { CanvasAddon } from "@xterm/addon-canvas";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { CornersInIcon } from "@phosphor-icons/react/dist/csr/CornersIn";
import { CornersOutIcon } from "@phosphor-icons/react/dist/csr/CornersOut";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { ArrowsLeftRightIcon } from "@phosphor-icons/react/dist/csr/ArrowsLeftRight";
import { SessionPet } from "./SessionPet";
import { CliSessionPet, isCliPetRuntimeKind } from "./CliSessionPet";
import {
  getTerminalAppearance,
  subscribeTerminalAppearance,
  FALLBACK_FONT_FAMILY,
} from "../terminalAppearance";
import {
  imagePasteSequence,
  isSupportedImageFile,
  isTerminalPasteShortcut,
  terminalDropIntent,
} from "./terminalPaste";
import { AgentAvatar } from "./AgentAvatar";
import type {
  AgentRuntimeKind,
  AppTheme,
  PtyChunk,
  SessionKind,
  SessionStatus,
  TerminalShell,
} from "../types";
import { terminalPaneHeaderPresentation } from "./terminalPaneHeader";

interface Props {
  sessionId: number;
  title: string;
  providerName: string;
  kind: SessionKind;
  shell: TerminalShell | null;
  agentRuntimeKind?: AgentRuntimeKind | null;
  agentAvatarId?: string | null;
  multiAgentProjectId?: string | null;
  multiAgentAgentId?: string | null;
  cwd: string;
  status: SessionStatus;
  theme: AppTheme;
  terminalTransparent?: boolean;
  preview?: boolean;
  hidden: boolean;
  maximized?: boolean;
  onClose: () => void;
  onToggleMaximize?: () => void;
  onSwitchAgent?: () => void;
  onViewSourceHistory?: () => void;
  onRelayDetails?: () => void;
  relaySession?: boolean;
  dragged?: boolean;
  dropTarget?: boolean;
  onPaneDragStart?: () => void;
  onPaneDragEnd?: () => void;
  onPaneDragOver?: () => void;
  onPaneDragLeave?: () => void;
  onPaneDrop?: () => void;
}

const encoder = new TextEncoder();

interface ClipboardImageFile {
  fileName: string;
  bytes: number[];
}

/**
 * 剪贴板有位图时发 Alt+V（Windows 下 claude 的 chat:imagePaste 绑定）让 claude
 * 自己读系统剪贴板——输入框出现原生 [Image #N] 占位符，随消息直接发给模型。
 * 剪贴板无位图时回落到 readText，保持原文本粘贴行为。
 *
 * 实测结论（diag_paste_image 探针，claude v2.1.220 + ConPTY）：claude 启用了
 * kitty keyboard 增强，legacy \x16（Ctrl+V）不再被识别，\x1bv（Alt+V）可靠触发
 * "Pasting…" → [Image#1]；claude 粘贴时文本优先于图片，故只在无文本冲突时用。
 */
async function pasteClipboard(term: Terminal, sessionId: number) {
  try {
    const hasImage = await invoke<boolean>("clipboard_has_image");
    if (hasImage) {
      const imageFile = await invoke<ClipboardImageFile | null>("clipboard_image_file");
      if (imageFile) {
        const blob = new Blob([new Uint8Array(imageFile.bytes)]);
        const image = await decodeImageBlob(blob);
        await invoke("paste_dropped_image", {
          fileName: imageFile.fileName,
          ...image,
        });
      }
      await invoke("pty_input", {
        sessionId,
        bytes: imagePasteSequence(navigator.platform),
      });
      return;
    }
  } catch (error) {
    console.error("检查剪贴板位图失败:", error);
  }
  try {
    const text = await readText();
    if (text.length > 0) term.paste(text);
  } catch (error) {
    console.error("读取剪贴板文本失败:", error);
  }
}

async function decodeImageBlob(blob: Blob) {
  const bitmap = await createImageBitmap(blob);
  try {
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) throw new Error("无法创建图片解码画布");
    context.drawImage(bitmap, 0, 0);
    const image = context.getImageData(0, 0, bitmap.width, bitmap.height);
    return {
      rgba: Array.from(image.data),
      width: bitmap.width,
      height: bitmap.height,
    };
  } finally {
    bitmap.close();
  }
}

async function pasteDroppedImage(file: File, sessionId: number) {
  const { rgba, width, height } = await decodeImageBlob(file);
  await invoke("paste_dropped_image", {
    fileName: file.name,
    rgba,
    width,
    height,
  });
  await invoke("pty_input", {
    sessionId,
    bytes: imagePasteSequence(navigator.platform),
  });
}

const STATUS_LABELS: Record<SessionStatus, string> = {
  pending: "未执行",
  running: "运行中",
  waiting: "等待确认",
  completed: "执行完毕",
  error: "执行失败",
  stopped: "已停止",
};

function safeFit(fit: FitAddon | null) {
  try {
    fit?.fit();
  } catch {
    /* */
  }
}

const TERMINAL_THEMES = {
  dark: {
    background: "#161615",
    foreground: "#e8e6df",
    cursor: "#d97757",
    selectionBackground: "#5a3a2e",
    black: "#1f1f1e",
    red: "#f06b75",
    green: "#62cf8b",
    yellow: "#d7ab5e",
    blue: "#55aef2",
    magenta: "#ba91e6",
    cyan: "#54c8df",
    white: "#c9d2df",
    brightBlack: "#627286",
    brightRed: "#ff8790",
    brightGreen: "#7de3a2",
    brightYellow: "#ebc071",
    brightBlue: "#73c0f7",
    brightMagenta: "#cca8f1",
    brightCyan: "#74d9ec",
    brightWhite: "#faf9f5",
  },
  light: {
    background: "#ffffff",
    foreground: "#2b2a27",
    cursor: "#c6613f",
    selectionBackground: "#f2dccf",
    black: "#2b2a27",
    red: "#c43d56",
    green: "#4f7d37",
    yellow: "#9a651f",
    blue: "#315fa8",
    magenta: "#7c4d9e",
    cyan: "#187184",
    white: "#5f687b",
    brightBlack: "#687084",
    brightRed: "#dc5870",
    brightGreen: "#66994c",
    brightYellow: "#b77b2d",
    brightBlue: "#4a78bf",
    brightMagenta: "#9365b3",
    brightCyan: "#29899e",
    brightWhite: "#273043",
  },
} as const;

function resolveTerminalTheme(theme: AppTheme, transparent: boolean) {
  if (!transparent) return TERMINAL_THEMES[theme];
  return {
    ...TERMINAL_THEMES[theme],
    background: "rgba(0, 0, 0, 0)",
  };
}

export function TerminalPane({
  sessionId,
  title,
  providerName,
  kind,
  shell,
  agentRuntimeKind = null,
  agentAvatarId,
  multiAgentProjectId,
  multiAgentAgentId,
  cwd,
  status,
  theme,
  terminalTransparent = false,
  preview = false,
  hidden,
  maximized = false,
  onClose,
  onToggleMaximize,
  onSwitchAgent,
  onViewSourceHistory,
  onRelayDetails,
  relaySession = false,
  dragged = false,
  dropTarget = false,
  onPaneDragStart,
  onPaneDragEnd,
  onPaneDragOver,
  onPaneDragLeave,
  onPaneDrop,
}: Props) {
  const header = terminalPaneHeaderPresentation({
    title,
    providerName,
    kind,
    agentRuntimeKind,
    multiAgentProjectId,
    multiAgentAgentId,
    relaySession,
    cwd,
  });
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [imageDropActive, setImageDropActive] = useState(false);
  const [terminalError, setTerminalError] = useState<string | null>(null);
  // 从 pane-head-actions（按钮 / 宠物区）按下时阻断整行 dragstart，避免误触发拖拽。
  const blockDragRef = useRef(false);

  useEffect(() => {
    if (preview) return;
    const container = containerRef.current;
    if (!container) return;
    // Create the terminal before the first poll can deliver output. Settings
    // arrive independently; waiting for them here drops early PTY chunks.
    let disposed = false;
    setTerminalError(null);
      const term = new Terminal({
        fontFamily: FALLBACK_FONT_FAMILY,
        fontSize: 12,
        theme: resolveTerminalTheme(theme, terminalTransparent),
        cursorBlink: true,
        scrollback: 3000,
        // CanvasAddon only clears transparent frames correctly when xterm owns
        // an alpha-enabled backing canvas. Keep this enabled for both themes so
        // switching transparency does not require recreating the PTY terminal.
        allowTransparency: true,
        allowProposedApi: true,
        customGlyphs: true,
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(container);
      try { term.loadAddon(new CanvasAddon()); }
      catch (error) { console.warn("终端 Canvas 不可用，使用默认渲染器:", error); }
      termRef.current = term;
      fitRef.current = fit;

      const doFit = () => safeFit(fit);
      const focusFrame = requestAnimationFrame(() => {
        doFit();
        term.focus();
      });

      term.attachCustomKeyEventHandler((event) => {
        if (!isTerminalPasteShortcut(event)) return true;

        event.preventDefault();
        event.stopPropagation();
        void pasteClipboard(term, sessionId);
        return false;
      });

      term.onData((data: string) => {
        // 过滤 xterm focus reporting(1004)，避免 claude 对话框误退出
        if (data === "\x1b[I" || data === "\x1b[O") return;
        invoke("pty_input", { sessionId, bytes: Array.from(encoder.encode(data)) })
          .catch(error => { if (!disposed) setTerminalError(`终端输入失败：${String(error)}`); });
      });
      term.onResize(({ cols, rows }) => {
        invoke("pty_resize", { sessionId, rows, cols }).catch(() => {});
      });

      const ro = new ResizeObserver(() => doFit());
      ro.observe(container);

      // 监听 MultiTerminal 分发的 poll_all 数据，取本 session 的 chunk 写入
      const onChunk = (e: Event) => {
        const detail = (e as CustomEvent).detail as Record<string, PtyChunk>;
        const c = detail[String(sessionId)];
        if (c && c.data.length > 0) {
          term.write(c.data);
        }
      };
      window.addEventListener("pty:chunk", onChunk as EventListener);

      let appearanceRevision = 0;
      const updateAppearance = (next: { fontFamily: string; fontSize: number }) => {
        if (termRef.current !== term) return;
        term.options.fontFamily = next.fontFamily;
        term.options.fontSize = next.fontSize;
        safeFit(fit);
      };
      const unsubscribeAppearance = subscribeTerminalAppearance((next) => {
        appearanceRevision += 1;
        updateAppearance(next);
      });
      void getTerminalAppearance().then(next => {
        if (!disposed && appearanceRevision === 0) updateAppearance(next);
      }).catch(error => { if (!disposed) setTerminalError(`读取终端外观失败：${String(error)}`); });
    return () => {
        disposed = true;
        window.cancelAnimationFrame(focusFrame);
        ro.disconnect();
        window.removeEventListener("pty:chunk", onChunk as EventListener);
        unsubscribeAppearance();
        term.dispose();
        termRef.current = null;
        fitRef.current = null;
    };
  }, [preview, sessionId]);

  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.theme = resolveTerminalTheme(theme, terminalTransparent);
      // Repaint every row after changing the canvas alpha/background. Without
      // this, the canvas renderer can retain the previous opaque frame and
      // produce black rectangles or overlapping glyph trails.
      termRef.current.refresh(0, termRef.current.rows - 1);
    }
  }, [terminalTransparent, theme]);

  useEffect(() => {
    if (hidden) return;
    const frame = window.requestAnimationFrame(() => {
      safeFit(fitRef.current);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [hidden, maximized]);

  return (
    <div
      className={
        "pane" +
        (kind === "terminal" && header.runtimeKind && isCliPetRuntimeKind(header.runtimeKind) ? " pane-has-cli-pet" : "") +
        (header.runtimeKind === "claude_code" ? " pane-has-session-pet" : "") +
        (hidden ? " pane-hidden" : "") +
        (maximized ? " pane-maximized" : "") +
        (terminalTransparent ? " pane-transparent" : "") +
        (dragged ? " pane-dragging" : "") +
        (dropTarget || imageDropActive ? " pane-drop-target" : "")
      }
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) {
          event.preventDefault();
          event.dataTransfer.dropEffect = "copy";
          setImageDropActive(true);
          return;
        }
        if (!onPaneDragOver) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        onPaneDragOver();
      }}
      onDragLeave={(event) => {
        const relatedTarget = event.relatedTarget;
        if (
          !(relatedTarget instanceof Node) ||
          !event.currentTarget.contains(relatedTarget)
        ) {
          setImageDropActive(false);
          onPaneDragLeave?.();
        }
      }}
      onDrop={(event) => {
        const files = Array.from(event.dataTransfer.files);
        const intent = terminalDropIntent(files);
        if (intent !== "pane") {
          event.preventDefault();
          event.stopPropagation();
          setImageDropActive(false);
          onPaneDragLeave?.();
          if (intent === "reject") {
            console.warn("终端只接受 PNG、JPG、JPEG、GIF 和 WebP 图片拖放");
            return;
          }
          const image = files.find(isSupportedImageFile);
          if (image) {
            void pasteDroppedImage(image, sessionId)
              .then(() => termRef.current?.focus())
              .catch((error) => console.error("拖放图片到终端失败:", error));
          }
          return;
        }
        if (!onPaneDrop) return;
        event.preventDefault();
        onPaneDrop();
      }}
    >
      <div
        className="pane-head"
        draggable={onPaneDragStart !== undefined}
        title="拖动标题栏可调整位置"
        onPointerDownCapture={(event) => {
          const target = event.target;
          blockDragRef.current =
            target instanceof Element &&
            Boolean(target.closest(".pane-head-actions"));
        }}
        onPointerUpCapture={() => {
          blockDragRef.current = false;
        }}
        onPointerCancel={() => {
          blockDragRef.current = false;
        }}
        onDragStart={(event) => {
          if (blockDragRef.current) {
            event.preventDefault();
            blockDragRef.current = false;
            return;
          }
          event.dataTransfer.effectAllowed = "move";
          event.dataTransfer.setData("text/plain", String(sessionId));
          onPaneDragStart?.();
        }}
        onDragEnd={onPaneDragEnd}
      >
        {header.showShellIcon && (
          <span className={`pane-shell-icon ${shell ?? "terminal"}`} aria-hidden="true">
            <TerminalWindowIcon size={19} />
          </span>
        )}
        {header.runtimeKind && (
          <AgentAvatar className="pane-runtime-logo" avatarId={agentAvatarId} runtimeKind={header.runtimeKind} />
        )}
        <span className="pane-id">
          <strong>{header.title}</strong>
          {header.subtitle && <span>· {header.subtitle}</span>}
          {header.agentBadge && <span className="agent-runtime-badge">{header.agentBadge === "relay" ? "接力 Agent" : "协作 Agent"}</span>}
        </span>
        <span className={`pane-status ${status}`} title={STATUS_LABELS[status]}>
          <span className={`status-dot ${status}`} />
          <span className="pane-status-label">{STATUS_LABELS[status]}</span>
        </span>
        <div className="pane-head-actions">
          {header.runtimeKind === "claude_code" && <SessionPet status={status} />}
          {kind === "terminal" && header.runtimeKind && isCliPetRuntimeKind(header.runtimeKind) && (
            <CliSessionPet runtimeKind={header.runtimeKind} status={status} hidden={hidden} />
          )}
          {onViewSourceHistory && <button type="button" className="pane-switch-agent" title="回看接力历史" onClick={onViewSourceHistory}>原历史</button>}
          {onRelayDetails && <button type="button" className="pane-switch-agent" title="接力详情与恢复原会话" onClick={onRelayDetails}>接力详情</button>}
          {onSwitchAgent && <button type="button" className="pane-switch-agent pane-handoff-icon" title="把当前工作交接给其它Agent" aria-label="把当前工作交接给其它Agent" onClick={onSwitchAgent}><ArrowsLeftRightIcon size={16} aria-hidden="true" /></button>}
          {onToggleMaximize && (
            <button
              className="pane-maximize"
              type="button"
              aria-label={maximized ? "还原会话" : "最大化会话"}
              title={maximized ? "还原会话（Esc）" : "最大化会话"}
              aria-pressed={maximized}
              onClick={onToggleMaximize}
            >
              {maximized ? (
                <CornersInIcon size={14} />
              ) : (
                <CornersOutIcon size={14} />
              )}
            </button>
          )}
          <button className="pane-close" type="button" title="停止会话" onClick={onClose}>
            <XIcon size={14} />
          </button>
        </div>
      </div>
      {terminalError && <p role="alert" className="harness-chat-error">{terminalError}</p>}
      <div className="pane-term" onClick={() => termRef.current?.focus()}>
        {preview ? (
          <div className="terminal-preview" aria-label={`${title} 终端预览`}>
            {relaySession ? <><p>{providerName} · 接力会话（界面演示）</p><p>原工作目录：{cwd}</p><p className="terminal-success">继续指令已排队，等待接手回复。</p><p className="terminal-muted">未启动真实 Agent，未读取真实历史。</p></> : kind === "terminal" ? (
              shell === "powershell" ? (
                <>
                  <p>PS D:\projects\crewvela&gt; <span className="terminal-highlight">npm run build</span></p>
                  <p className="terminal-muted">&gt; crewvela@0.1.0 build</p>
                  <p className="terminal-muted">&gt; vite build</p>
                  <p className="terminal-success">✓ 32 modules transformed.</p>
                  <p className="terminal-success">✓ built in 2.41s</p>
                  <p>PS D:\projects\crewvela&gt; <span className="terminal-block-cursor" /></p>
                </>
              ) : (
                <>
                  <p>C:\projects\crewvela&gt; <span className="terminal-highlight">git status</span></p>
                  <p>On branch main</p>
                  <p>Your branch is up to date with 'origin/main'.</p>
                  <p className="terminal-muted">Changes not staged for commit:</p>
                  <p className="terminal-error">  modified: src/App.tsx</p>
                  <p>C:\projects\crewvela&gt; <span className="terminal-block-cursor" /></p>
                </>
              )
            ) : (
              <>
                <p className="terminal-command">&gt; 检查当前项目并继续执行</p>
                <p className="terminal-muted">Thought for 3s</p>
                <p>正在分析工作区结构与现有改动。</p>
                <p className="terminal-muted">＊ Loaded project context</p>
                <p className="terminal-command">&gt; 输出下一步计划</p>
                <p>{providerName} 已完成检查，准备继续处理任务。</p>
                <p className="terminal-muted">＊ Worked for 7s</p>
                <p className="terminal-cursor">&gt; <span /></p>
              </>
            )}
          </div>
        ) : (
          <div ref={containerRef} className="pane-term-surface" />
        )}
      </div>
    </div>
  );
}
