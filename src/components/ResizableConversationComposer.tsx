import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import "../styles/conversation-composer-resize.css";

type Edge = "n" | "s" | "w" | "e" | "nw" | "ne" | "sw" | "se";
interface Size { left: number; bottom: number; width: number; height: number }
interface Limits { width: number; height: number; minWidth: number; minHeight: number }
const EDGES: Record<Edge, string> = { n: "上边缘", s: "下边缘", w: "左边缘", e: "右边缘", nw: "左上角", ne: "右上角", sw: "左下角", se: "右下角" };
const CURSORS: Record<Edge, string> = { n: "ns-resize", s: "ns-resize", w: "ew-resize", e: "ew-resize", nw: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", sw: "nesw-resize" };
const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), Math.max(min, max));
function fit(size: Size, limits: Limits): Size {
  const width = clamp(size.width, limits.minWidth, limits.width);
  const height = clamp(size.height, limits.minHeight, limits.height);
  return { width, height, left: clamp(size.left, 0, limits.width - width), bottom: clamp(size.bottom, 0, limits.height - height) };
}
function resized(start: Size, edge: Edge, dx: number, dy: number, limits: Limits): Size {
  const next = fit(start, limits);
  if (edge.includes("w")) {
    const right = next.left + next.width;
    next.left = clamp(next.left + dx, 0, right - limits.minWidth); next.width = right - next.left;
  } else if (edge.includes("e")) next.width = clamp(next.width + dx, limits.minWidth, limits.width - next.left);
  if (edge.includes("n")) next.height = clamp(next.height - dy, limits.minHeight, limits.height - next.bottom);
  else if (edge.includes("s")) {
    const top = next.bottom + next.height;
    next.bottom = clamp(next.bottom - dy, 0, top - limits.minHeight); next.height = top - next.bottom;
  }
  return fit(next, limits);
}

/** Resize the entire form while preserving its opposite edge and the panel's bounds. */
export function ResizableConversationComposer({ children, onSubmit }: { children: ReactNode; onSubmit: (event: FormEvent<HTMLFormElement>) => void }) {
  const slot = useRef<HTMLDivElement>(null);
  const form = useRef<HTMLFormElement>(null);
  const [size, setSize] = useState<Size | null>(null);
  const sizeRef = useRef(size); sizeRef.current = size;
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ edge: Edge; x: number; y: number; start: Size; handle: HTMLElement; pointerId: number; cursor: string; selection: string } | null>(null);
  const frame = useRef(0);
  const pending = useRef<{ x: number; y: number } | null>(null);

  const measure = useCallback((): Limits | null => {
    const node = form.current, host = slot.current;
    const panel = host?.closest<HTMLElement>(".team-chat-panel"), footer = host?.closest<HTMLElement>(".conversation-footer");
    if (!node || !host || !panel || !footer || !host.clientWidth || !panel.clientHeight) return null;
    const formStyle = getComputedStyle(node);
    const extra = footer.getBoundingClientRect().height - host.getBoundingClientRect().height;
    // Keep some conversation visible even when the editor is expanded to its limit.
    const height = Math.max(1, panel.clientHeight - extra - Math.min(80, panel.clientHeight * .2));
    const padding = [formStyle.paddingTop, formStyle.paddingBottom, formStyle.borderTopWidth, formStyle.borderBottomWidth].reduce((sum, value) => sum + (parseFloat(value) || 0), 0);
    const actions = node.querySelector<HTMLElement>(".conversation-composer-actions")?.getBoundingClientRect().height ?? 34;
    const attachments = node.querySelector<HTMLElement>(".conversation-draft-attachments");
    const attachmentHeight = attachments ? attachments.offsetHeight + parseFloat(getComputedStyle(attachments).marginBlockStart) + parseFloat(getComputedStyle(attachments).marginBlockEnd) : 0;
    return { width: host.clientWidth, height, minWidth: Math.min(280, host.clientWidth), minHeight: Math.min(height, padding + actions + attachmentHeight + 48) };
  }, []);
  const currentSize = (): Size => {
    const rect = form.current!.getBoundingClientRect();
    return sizeRef.current ?? { left: 0, bottom: 0, width: rect.width, height: rect.height };
  };
  const flush = useCallback(() => {
    frame.current = 0;
    const point = pending.current, active = drag.current, limits = measure(); pending.current = null;
    if (point && active && limits) setSize(resized(active.start, active.edge, point.x - active.x, point.y - active.y, limits));
  }, [measure]);
  const finish = useCallback(() => {
    if (frame.current) cancelAnimationFrame(frame.current);
    flush();
    const active = drag.current; drag.current = null;
    if (!active) return;
    document.body.style.cursor = active.cursor; document.body.style.userSelect = active.selection;
    if (active.handle.hasPointerCapture(active.pointerId)) active.handle.releasePointerCapture(active.pointerId);
    setDragging(false);
  }, [flush]);
  useEffect(() => {
    window.addEventListener("blur", finish);
    const escape = (event: globalThis.KeyboardEvent) => { if (event.key === "Escape" && drag.current) { event.preventDefault(); finish(); } };
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("blur", finish); window.removeEventListener("keydown", escape);
      if (frame.current) cancelAnimationFrame(frame.current);
      const active = drag.current; drag.current = null; pending.current = null;
      if (active) { document.body.style.cursor = active.cursor; document.body.style.userSelect = active.selection; }
    };
  }, [finish]);
  useLayoutEffect(() => {
    const host = slot.current, node = form.current;
    if (!host || !node) return;
    const update = () => {
      const limits = measure(); if (!limits) return;
      setSize(previous => {
        if (!previous && node.getBoundingClientRect().height <= limits.height) return null;
        const next = fit(previous ?? { width: host.clientWidth, height: node.getBoundingClientRect().height, left: 0, bottom: 0 }, limits);
        return previous && Object.keys(next).every(key => next[key as keyof Size] === previous[key as keyof Size]) ? previous : next;
      });
    };
    const observer = new ResizeObserver(update);
    [host, host.closest(".team-chat-panel"), host.closest(".conversation-footer"), node.querySelector(".conversation-composer-actions"), node.querySelector(".conversation-editor")].forEach(target => { if (target) observer.observe(target); });
    update(); return () => observer.disconnect();
  }, [measure]);
  const start = (event: PointerEvent<HTMLButtonElement>, edge: Edge) => {
    if (event.button !== 0 || drag.current || !measure()) return;
    event.preventDefault(); event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { edge, x: event.clientX, y: event.clientY, start: currentSize(), handle: event.currentTarget, pointerId: event.pointerId, cursor: document.body.style.cursor, selection: document.body.style.userSelect };
    document.body.style.cursor = CURSORS[edge]; document.body.style.userSelect = "none"; setDragging(true);
  };
  const keyboard = (event: KeyboardEvent<HTMLButtonElement>, edge: Edge) => {
    if (event.key === "Home") { event.preventDefault(); setSize(null); return; }
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    const limits = measure(); if (!limits) return;
    event.preventDefault(); event.stopPropagation();
    const step = event.shiftKey ? 24 : 8;
    setSize(resized(currentSize(), edge, event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0, event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0, limits));
  };
  const endPointer = (event: PointerEvent<HTMLButtonElement>) => { if (drag.current?.pointerId === event.pointerId) finish(); };
  return <div className="conversation-composer-slot" ref={slot}>
    <form ref={form} className="conversation-composer multi-agent-composer resizable-conversation-composer" data-sized={!!size} data-resizing={dragging} onSubmit={onSubmit} style={size ? { width: size.width, height: size.height, marginLeft: size.left, marginBottom: size.bottom } : undefined}>
      {children}
      {(Object.keys(EDGES) as Edge[]).map(edge => <button key={edge} type="button" className="conversation-composer-resizer" data-edge={edge} tabIndex={edge === "nw" ? 0 : -1} aria-label={`拖动${EDGES[edge]}调整输入区域`} title="拖动调整大小；方向键微调，Home 或双击边缘恢复默认" style={{ cursor: CURSORS[edge] }} onPointerDown={event => start(event, edge)} onPointerMove={event => {
        if (drag.current?.pointerId !== event.pointerId) return;
        pending.current = { x: event.clientX, y: event.clientY }; if (!frame.current) frame.current = requestAnimationFrame(flush);
      }} onPointerUp={endPointer} onPointerCancel={endPointer} onLostPointerCapture={endPointer} onKeyDown={event => keyboard(event, edge)} onDoubleClick={() => setSize(null)} />)}
    </form>
  </div>;
}
