import { useEffect, useRef, useState } from "react";
import atlas from "../assets/deepseek-whale-girl-atlas.png";
import { WHALE_GIRL_MOTIONS, whaleGirlFrame } from "../whaleGirlMotion";
import "../styles/native-whale-girl.css";

export function NativeWhaleGirl({ active = true }: { active?: boolean }) {
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
    // Resume the current gesture from its first frame, without accumulating hidden time.
    started.current = performance.now();
    setPose({ motion: current.current, frame: 0 });
    const timer = window.setInterval(() => {
      let elapsed = performance.now() - started.current;
      if (elapsed >= WHALE_GIRL_MOTIONS[current.current].duration) {
        current.current = (current.current + 1) % WHALE_GIRL_MOTIONS.length;
        started.current = performance.now(); elapsed = 0;
      }
      const motion = current.current, frame = whaleGirlFrame(motion, elapsed);
      setPose(previous => previous.motion === motion && previous.frame === frame ? previous : { motion, frame });
    }, 80);
    return () => window.clearInterval(timer);
  }, [active, reduced]);
  const row = reduced ? 0 : pose.motion, frame = reduced ? 0 : pose.frame;
  return <button type="button" className="native-whale-girl" data-motion={WHALE_GIRL_MOTIONS[row].name}
    data-frame={frame} data-motion-paused={!active || reduced ? "true" : "false"}
    aria-label={`鲸鱼娘：${WHALE_GIRL_MOTIONS[row].name}，点击切换动作`} title="点击换个动作"
    disabled={!active || reduced} onClick={() => {
      current.current = (current.current + 1) % WHALE_GIRL_MOTIONS.length;
      started.current = performance.now(); setPose({ motion: current.current, frame: 0 });
    }}>
    <span className="native-whale-girl-sprite" aria-hidden="true" style={{ backgroundImage: `url(${atlas})`, backgroundPosition: `${frame / 3 * 100}% ${row / 7 * 100}%` }} />
  </button>;
}
