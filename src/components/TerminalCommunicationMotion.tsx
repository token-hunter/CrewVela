import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import type { CommunicationDelivery } from "../types";
import "../styles/terminal-communication-motion.css";

const TRAVEL_MS = 680;
const LIFETIME_MS = 1250;

interface Flight {
  id: string;
  path: string;
  target: { x: number; y: number; width: number; height: number };
}

function visiblePane(panes: HTMLElement[], sessionId: string): DOMRect | undefined {
  const pane = panes.find(item => item.dataset.communicationSession === sessionId);
  if (!pane || pane.closest("[hidden], .pane-hidden")) return;
  const bounds = pane.getBoundingClientRect();
  if (bounds.width > 0 && bounds.height > 0 && bounds.bottom > 0 && bounds.right > 0
    && bounds.top < window.innerHeight && bounds.left < window.innerWidth) return bounds;
}

function route(source: DOMRect, target: DOMRect, grid: DOMRect): string {
  const dx = (target.left + target.width / 2) - (source.left + source.width / 2);
  const dy = (target.top + target.height / 2) - (source.top + source.height / 2);
  const horizontal = Math.abs(dx) >= Math.abs(dy);
  const forward = (horizontal ? dx : dy) >= 0;
  const from = forward ? 0.68 : 0.32;
  const to = 1 - from;
  const sx = source.left - grid.left + source.width * (horizontal ? from : 0.5);
  const sy = source.top - grid.top + source.height * (horizontal ? 0.48 : from);
  const tx = target.left - grid.left + target.width * (horizontal ? to : 0.5);
  const ty = target.top - grid.top + target.height * (horizontal ? 0.48 : to);
  const curve = Math.min(96, Math.max(24, Math.hypot(tx - sx, ty - sy) * 0.28));
  const bend = forward ? -curve : curve;
  const x1 = horizontal ? sx + (tx - sx) * 0.35 : sx + bend;
  const x2 = horizontal ? tx - (tx - sx) * 0.35 : tx + bend;
  const y1 = horizontal ? sy + bend : sy + (ty - sy) * 0.35;
  const y2 = horizontal ? ty + bend : ty - (ty - sy) * 0.35;
  return `M ${sx} ${sy} C ${x1} ${y1}, ${x2} ${y2}, ${tx} ${ty}`;
}

function CommunicationFlight({ flight }: { flight: Flight }) {
  const pathRef = useRef<SVGPathElement>(null);
  const lightRef = useRef<SVGGElement>(null);
  useLayoutEffect(() => {
    const path = pathRef.current;
    const light = lightRef.current;
    if (!path || !light) return;
    const length = path.getTotalLength();
    const started = performance.now();
    let frame = 0;
    const draw = (now: number) => {
      const progress = Math.min(1, (now - started) / TRAVEL_MS);
      const { x, y } = path.getPointAtLength(progress * length);
      light.setAttribute("transform", `translate(${x} ${y})`);
      if (progress < 1) frame = window.requestAnimationFrame(draw);
    };
    draw(started);
    return () => window.cancelAnimationFrame(frame);
  }, [flight.path]);

  return <g className="terminal-communication-flight" data-message-id={flight.id}>
    <path ref={pathRef} className="terminal-communication-line" d={flight.path} pathLength={100} />
    <g ref={lightRef} className="terminal-communication-light">
      <circle r={9} className="terminal-communication-halo" />
      <circle r={3.5} className="terminal-communication-dot" />
    </g>
    <rect {...flight.target} rx={8} className="terminal-communication-receiver" />
  </g>;
}

export function TerminalCommunicationMotion() {
  const overlay = useRef<SVGSVGElement>(null);
  const [flights, setFlights] = useState<Flight[]>([]);

  useLayoutEffect(() => {
    const layer = overlay.current;
    const grid = layer?.parentElement;
    if (!layer || !grid) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const seen = new Set<string>();
    const timers = new Set<number>();
    const cancel = () => {
      for (const timer of timers) window.clearTimeout(timer);
      timers.clear();
    };
    const clear = () => { cancel(); setFlights([]); };
    let size = `${grid.clientWidth}:${grid.clientHeight}`;
    const observer = new ResizeObserver(() => {
      const next = `${grid.clientWidth}:${grid.clientHeight}`;
      if (size !== next) { size = next; clear(); }
    });
    observer.observe(grid);

    const receive = (event: Event) => {
      if (reducedMotion.matches || document.hidden || grid.closest("[hidden]")) return;
      const bounds = layer.getBoundingClientRect();
      if (!bounds.width || !bounds.height) return;
      const panes = Array.from(grid.querySelectorAll<HTMLElement>(":scope > [data-communication-session]"));
      const next: Flight[] = [];
      for (const delivery of (event as CustomEvent<CommunicationDelivery[]>).detail) {
        if (seen.has(delivery.id) || delivery.fromSessionId === delivery.toSessionId) continue;
        seen.add(delivery.id);
        if (seen.size > 128) seen.delete(seen.values().next().value!);
        const source = visiblePane(panes, delivery.fromSessionId);
        const target = visiblePane(panes, delivery.toSessionId);
        if (!source || !target) continue;
        next.push({ id: delivery.id, path: route(source, target, bounds), target: {
          x: target.left - bounds.left + 1.5, y: target.top - bounds.top + 1.5,
          width: Math.max(0, target.width - 3), height: Math.max(0, target.height - 3),
        } });
      }
      if (!next.length) return;
      setFlights(current => [...current, ...next]);
      const timer = window.setTimeout(() => {
        timers.delete(timer);
        setFlights(current => current.filter(item => !next.some(flight => flight.id === item.id)));
      }, LIFETIME_MS);
      timers.add(timer);
    };

    window.addEventListener("pty:communication-delivery", receive);
    reducedMotion.addEventListener("change", clear);
    document.addEventListener("visibilitychange", clear);
    return () => {
      window.removeEventListener("pty:communication-delivery", receive);
      reducedMotion.removeEventListener("change", clear);
      document.removeEventListener("visibilitychange", clear);
      observer.disconnect();
      cancel();
    };
  }, []);

  return <svg ref={overlay} className="terminal-communication-motion" aria-hidden="true" style={{ "--communication-travel": `${TRAVEL_MS}ms` } as CSSProperties}>
    {flights.map(flight => <CommunicationFlight key={flight.id} flight={flight} />)}
  </svg>;
}
