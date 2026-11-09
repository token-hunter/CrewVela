import { useEffect, useRef, useState } from "react";
import type { AnimationEvent as ReactAnimationEvent, PointerEvent as ReactPointerEvent } from "react";
import zcodeOpen from "../assets/native-agent-zcode-open.png";
import zcodeClosed from "../assets/native-agent-zcode-closed.png";
import "../styles/native-zcode-mascot.css";

const particles = ["one", "two", "three", "four", "five", "six", "seven"] as const;
const transferParticles = ["one", "two", "three", "four", "five"] as const;

export function NativeZCodeMascot({ active }: { active: boolean }) {
  const [reducedMotion, setReducedMotion] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [sceneRun, setSceneRun] = useState(0);
  const [playback, setPlayback] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReducedMotion(preference.matches);
    update();
    preference.addEventListener("change", update);
    return () => preference.removeEventListener("change", update);
  }, []);

  const tiltTowardPointer = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (!active || reducedMotion || event.pointerType !== "mouse") return;
    const stage = stageRef.current;
    if (!stage) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - bounds.left) / bounds.width - 0.5) * 2;
    const y = ((event.clientY - bounds.top) / bounds.height - 0.5) * 2;
    stage.style.setProperty("--native-zcode-tilt-x", `${Math.max(-2, Math.min(2, -y * 2))}deg`);
    stage.style.setProperty("--native-zcode-tilt-y", `${Math.max(-2, Math.min(2, x * 2))}deg`);
  };

  const resetTilt = () => {
    stageRef.current?.style.removeProperty("--native-zcode-tilt-x");
    stageRef.current?.style.removeProperty("--native-zcode-tilt-y");
  };

  const finishPlayback = (event: ReactAnimationEvent<HTMLSpanElement>) => {
    if (event.target === event.currentTarget && event.animationName === "native-zcode-story-end") setPlayback(false);
  };

  useEffect(() => {
    if (!active || reducedMotion) {
      stageRef.current?.style.removeProperty("--native-zcode-tilt-x");
      stageRef.current?.style.removeProperty("--native-zcode-tilt-y");
    }
    if (reducedMotion) setPlayback(false);
  }, [active, reducedMotion]);

  return <button
    type="button"
    className="native-zcode-mascot"
    aria-label="ZCode 代码核心；点击重播扫描、代码编排与完成光波"
    title="点击重播 ZCode 的代码扫描动画"
    disabled={!active || reducedMotion}
    data-scene-run={sceneRun}
    data-reduced-motion={reducedMotion ? "true" : "false"}
    data-playback={playback ? "story" : "cycle"}
    onPointerMove={tiltTowardPointer}
    onPointerLeave={resetTilt}
    onClick={() => { resetTilt(); setPlayback(true); setSceneRun(run => run + 1); }}>
    <div className="native-zcode-stage" ref={stageRef} key={sceneRun}>
      {playback && <span className="native-zcode-story-sentinel" aria-hidden="true" onAnimationEnd={finishPlayback} />}
      <svg className="native-zcode-effects" viewBox="0 0 380 280" aria-hidden="true">
        <path className="native-zcode-scan" pathLength="100" d="M61 140 C68 77 123 34 190 32 C262 32 316 80 319 140 C316 201 260 248 190 248 C121 248 65 202 61 140" />
        <ellipse className="native-zcode-wave" cx="190" cy="140" rx="62" ry="46" />
      </svg>
      <div className="native-zcode-orbit" aria-hidden="true">
        {particles.map(particle => <span key={particle} className={`native-zcode-particle native-zcode-particle--${particle}`} />)}
      </div>
      <div className="native-zcode-codefield" aria-hidden="true">
        <div className="native-zcode-code-row native-zcode-code-row--one"><i /><i /><i /></div>
        <div className="native-zcode-code-row native-zcode-code-row--two"><i /><i /></div>
        <div className="native-zcode-code-row native-zcode-code-row--three"><i /><i /><i /></div>
      </div>
      <div className="native-zcode-transfers" aria-hidden="true">
        {transferParticles.map(particle => <i key={particle} className={`native-zcode-transfer native-zcode-transfer--${particle}`} />)}
      </div>
      <div className="native-zcode-body">
        <img className="native-zcode-open" src={zcodeOpen} alt="" />
        <img className="native-zcode-blink" src={zcodeClosed} alt="" />
      </div>
    </div>
  </button>;
}
