import { useEffect, useRef, useState } from "react";
import { SpriteMascotTimeline, type SpriteMascotMotion } from "../spriteMascotMotion";
import { createSpriteMascotRenderer, type SpriteFlow } from "../spriteMascotRenderer";
import "../styles/native-sprite-mascot.css";

/** Continuous, aligned in-between poses from the authored character artwork. */
export function NativeSpriteMascot({ active = true, atlas, flow, motions, className, label }: {
  active?: boolean;
  atlas: string;
  flow: SpriteFlow;
  motions: readonly SpriteMascotMotion[];
  className: string;
  label: string;
}) {
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [motion, setMotion] = useState(0);
  const [ready, setReady] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [restore, setRestore] = useState(0);
  const canvas = useRef<HTMLCanvasElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const sprite = useRef<HTMLSpanElement>(null);
  const timeline = useRef(new SpriteMascotTimeline(motions));
  const renderer = useRef<Awaited<ReturnType<typeof createSpriteMascotRenderer>> | null>(null);
  const size = useRef({ width: 300, height: 300 });
  const draws = useRef(0);
  const failed = useRef(false);
  const lastDraw = useRef("");
  const reducedRef = useRef(reduced);
  reducedRef.current = reduced;
  const fallback = (error: unknown) => {
    failed.current = true; setReady(false); lastDraw.current = "";
    setFailure(error instanceof Error ? error.message : String(error));
    renderer.current?.dispose(); renderer.current = null;
    if (button.current) button.current.dataset.renderer = "static-fallback";
    console.warn("Continuous mascot animation unavailable; keeping the original static artwork.", error);
  };
  const draw = () => {
    const pose = reducedRef.current ? { from: 0, to: 0, mix: 0 } : timeline.current.sample();
    const row = Math.floor(pose.from / 4), frame = pose.from % 4;
    if (sprite.current) sprite.current.style.backgroundPosition = flow.actions ? "center" : `${frame / 3 * 100}% ${row / (motions.length - 1) * 100}%`;
    if (button.current) button.current.dataset.frame = String(frame);
    if (!renderer.current) return;
    renderer.current.prepare(pose, timeline.current.motion);
    const stats = renderer.current.stats;
    if (button.current && stats) {
      button.current.dataset.actionTextures = String(stats.resident);
      button.current.dataset.actionPending = String(stats.pending);
      button.current.dataset.actionBytes = String(stats.bytes);
    }
    const picture = pose.from === pose.to || pose.mix <= 0 ? `static:${pose.from}` : pose.mix >= 1 ? `static:${pose.to}` : `${pose.from}:${pose.to}:${pose.mix}`;
    const key = `${picture}:${size.current.width}:${size.current.height}:${window.devicePixelRatio}`;
    if (key === lastDraw.current) return;
    try { if (!renderer.current.draw(pose, size.current.width, size.current.height)) return; }
    catch (error) { fallback(error); return; }
    lastDraw.current = key;
    if (button.current) {
      button.current.dataset.interpolation = `${pose.from}:${pose.to}:${pose.mix.toFixed(5)}`;
      button.current.dataset.renderCount = String(++draws.current);
    }
  };
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const change = () => setReduced(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  useEffect(() => {
    // A suspended tab can also lose its context. Keep restoration listeners
    // attached while inactive, even though its GPU textures have been released.
    const node = canvas.current!;
    const lost = (event: Event) => { event.preventDefault(); fallback(new Error("Mascot WebGL context lost")); };
    const restored = () => { failed.current = false; setRestore(value => value + 1); };
    node.addEventListener("webglcontextlost", lost); node.addEventListener("webglcontextrestored", restored);
    return () => { node.removeEventListener("webglcontextlost", lost); node.removeEventListener("webglcontextrestored", restored); };
  }, []);
  useEffect(() => {
    // Retain the frozen canvas, but release GPU textures while a tab is inactive.
    // Initial inactive/reduced-motion welcomes do not load displacement textures.
    if (!active || reduced) return;
    const node = canvas.current!;
    let cancelled = false;
    const observer = new ResizeObserver(([entry]) => {
      size.current = { width: entry.contentRect.width, height: entry.contentRect.height };
      if (!failed.current) draw();
    });
    observer.observe(node);
    createSpriteMascotRenderer(node, atlas, flow).then(value => {
      if (cancelled) { value.dispose(); return; }
      renderer.current = value; failed.current = false; setFailure(null); lastDraw.current = "";
      draw(); if (!failed.current) setReady(true);
    }).catch(error => { if (!cancelled) fallback(error); });
    return () => {
      cancelled = true; observer.disconnect();
      renderer.current?.dispose(); renderer.current = null;
    };
  }, [active, reduced, atlas, flow, restore]);
  useEffect(() => {
    draw();
    if (!active || reduced || !ready) return;
    let request = 0, previous: number | undefined;
    const tick = (time: number) => {
      if (failed.current) return;
      // If a local action texture is still decoding, keep its rest pose and
      // pause the timeline instead of skipping the first part of the gesture.
      try { if (previous !== undefined && renderer.current?.readyFor(timeline.current.sample())) timeline.current.advance(time - previous); }
      catch (error) { fallback(error); return; }
      previous = time;
      draw();
      const nextMotion = timeline.current.motion;
      if (button.current?.dataset.motion !== motions[nextMotion].name) setMotion(nextMotion);
      request = window.requestAnimationFrame(tick);
    };
    request = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(request);
  }, [active, reduced, ready, motions]);
  const row = reduced ? 0 : motion;
  return <button ref={button} type="button" className={`${className} native-sprite-mascot`} data-motion={motions[row].name}
    data-artwork-layout={flow.actions ? "single" : "atlas"}
    data-renderer={ready ? "flow" : failed.current ? "static-fallback" : !active || reduced ? "static-paused" : "loading"}
    data-motion-paused={!active || reduced || !!failure ? "true" : "false"}
    aria-label={`${label}：${motions[row].name}${failure ? "（静态预览）" : "，点击切换动作"}`} title={failure ? "动画暂不可用，正在显示静态形象" : "点击换个动作"}
    disabled={!active || reduced || !!failure} onClick={() => {
      timeline.current.next(); setMotion(timeline.current.motion); draw();
    }}>
    <span ref={sprite} className={`${className}-sprite`} aria-hidden="true" style={{ backgroundImage: `url(${atlas})`, backgroundSize: ready && !reduced ? "0 0" : flow.actions ? "contain" : undefined }}>
      <canvas ref={canvas} className="native-sprite-canvas" style={{ visibility: ready && !reduced ? "visible" : "hidden" }} />
    </span>
  </button>;
}
