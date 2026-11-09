import { useEffect, useRef, useState } from "react";
import type { NativeAgentKind } from "../nativeAgentSwitch";
import { RUNTIME_LOGOS } from "../runtimeLogos";
import "../styles/neutral-agent-visuals.css";

export type NativeAgentMascotKind = NativeAgentKind;
type Motion = "intro" | "idle" | "story";
const stageTiming: Record<NativeAgentMascotKind, { intro: number; wait: number; story: number }> = {
  claude_chat: { intro: 4400, wait: 19000, story: 5600 },
  codex_chat: { intro: 4200, wait: 22000, story: 5600 },
  kimi_chat: { intro: 4600, wait: 17000, story: 6000 },
  opencode_chat: { intro: 4100, wait: 24000, story: 5400 },
  pi_chat: { intro: 4300, wait: 15000, story: 6200 },
  zcode: { intro: 4400, wait: 20000, story: 5600 },
  deepseek_harness: { intro: 4200, wait: 21000, story: 5600 },
};

export function NativeAgentMascot({ kind, active }: { kind: NativeAgentMascotKind; active: boolean }) {
  const [motion, setMotion] = useState<Motion>("intro");
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [reacting, setReacting] = useState(false);
  const [sceneRun, setSceneRun] = useState(0);
  const lastIntroKind = useRef<NativeAgentMascotKind | null>(null);
  const reactionKind = useRef(kind);
  const phaseTimer = useRef<number | undefined>(undefined);
  const scheduleStoryRef = useRef<() => void>(() => undefined);
  const reactionTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(preference.matches);
    update();
    preference.addEventListener("change", update);
    return () => preference.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    if (!active || reduced) return;
    const timing = stageTiming[kind];
    let live = true;
    const scheduleStory = () => {
      if (!live) return;
      phaseTimer.current = window.setTimeout(() => {
        if (!live) return;
        setMotion("story");
        setSceneRun(run => run + 1);
        phaseTimer.current = window.setTimeout(() => {
          if (!live) return;
          setMotion("idle");
          scheduleStory();
        }, timing.story);
      }, timing.wait);
    };
    scheduleStoryRef.current = scheduleStory;
    if (lastIntroKind.current !== kind) {
      lastIntroKind.current = kind;
      setMotion("intro");
      phaseTimer.current = window.setTimeout(() => {
        if (!live) return;
        setMotion("idle");
        scheduleStory();
      }, timing.intro);
    } else {
      setMotion("idle");
      scheduleStory();
    }
    return () => {
      live = false;
      if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
      if (scheduleStoryRef.current === scheduleStory) scheduleStoryRef.current = () => undefined;
    };
  }, [active, kind, reduced]);

  useEffect(() => () => {
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
  }, []);

  useEffect(() => {
    if (active && !reduced) return;
    if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    setReacting(false);
  }, [active, reduced]);

  useEffect(() => {
    if (reactionKind.current === kind) return;
    reactionKind.current = kind;
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    setReacting(false);
  }, [kind]);

  const celebrate = () => {
    if (!active || reduced) return;
    const timing = stageTiming[kind];
    if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
    setMotion("story");
    setSceneRun(run => run + 1);
    setReacting(true);
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    reactionTimer.current = window.setTimeout(() => setReacting(false), timing.story);
    phaseTimer.current = window.setTimeout(() => {
      setReacting(false);
      setMotion("idle");
      scheduleStoryRef.current();
    }, timing.story);
  };
  const artwork = <span className="native-neutral-mark" key={sceneRun}>
    <img src={RUNTIME_LOGOS[kind]} alt="" draggable={false} />
  </span>;
  const labels: Record<NativeAgentMascotKind, string> = {
    claude_chat: "Claude Code 会话图形，点击播放动画",
    codex_chat: "Codex 会话图形，点击播放动画",
    kimi_chat: "Kimi Code 会话图形，点击播放动画",
    opencode_chat: "OpenCode 会话图形，点击播放动画",
    pi_chat: "Pi 会话图形，点击播放动画",
    zcode: "ZCode Desktop 会话图形，点击播放动画",
    deepseek_harness: "DeepSeek Harness 会话图形，点击播放动画",
  };
  return <button type="button" className="native-mascot native-mascot-neutral" data-mascot-kind={kind.replace("_chat", "")} data-motion={motion} data-reaction={reacting ? "active" : ""} data-reduced-motion={reduced ? "true" : "false"} data-scene-run={sceneRun} aria-label={labels[kind]} title="点击触发一个小彩蛋" onClick={celebrate}>
    {artwork}
  </button>;
}
