import { useEffect, useRef, useState } from "react";
import { spriteMascotFrame, type SpriteMascotMotion } from "../spriteMascotMotion";

/** Shared four-column sprite playback for welcome characters. */
export function NativeSpriteMascot({ active = true, atlas, motions, className, label }: {
  active?: boolean;
  atlas: string;
  motions: readonly SpriteMascotMotion[];
  className: string;
  label: string;
}) {
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [pose, setPose] = useState({ motion: 0, frame: 0 });
  const current = useRef(0);
  const started = useRef(0);
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const change = () => setReduced(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  useEffect(() => {
    if (!active || reduced) return;
    // Hidden/inactive time never advances the current gesture.
    started.current = performance.now();
    setPose({ motion: current.current, frame: 0 });
    const timer = window.setInterval(() => {
      let elapsed = performance.now() - started.current;
      if (elapsed >= motions[current.current].duration) {
        current.current = (current.current + 1) % motions.length;
        started.current = performance.now(); elapsed = 0;
      }
      const motion = current.current, frame = spriteMascotFrame(motions[motion], elapsed);
      setPose(previous => previous.motion === motion && previous.frame === frame ? previous : { motion, frame });
    }, 80);
    return () => window.clearInterval(timer);
  }, [active, reduced, motions]);
  const row = reduced ? 0 : pose.motion, frame = reduced ? 0 : pose.frame;
  return <button type="button" className={className} data-motion={motions[row].name}
    data-frame={frame} data-motion-paused={!active || reduced ? "true" : "false"}
    aria-label={`${label}：${motions[row].name}，点击切换动作`} title="点击换个动作"
    disabled={!active || reduced} onClick={() => {
      current.current = (current.current + 1) % motions.length;
      started.current = performance.now(); setPose({ motion: current.current, frame: 0 });
    }}>
    <span className={`${className}-sprite`} aria-hidden="true" style={{ backgroundImage: `url(${atlas})`, backgroundPosition: `${frame / 3 * 100}% ${row / (motions.length - 1) * 100}%` }} />
  </button>;
}
