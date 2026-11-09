import { useEffect, useRef, useState } from "react";
import type { DeepSkinDefinition } from "../deepSkins";

interface Props {
  skin: DeepSkinDefinition | undefined;
  motionEnabled: boolean;
  soundEnabled: boolean;
  soundVolume: number;
}

interface AudioGraph {
  context: AudioContext;
  gain: GainNode;
  stop: () => void;
}

interface RainDrop {
  x: number;
  y: number;
  length: number;
  speed: number;
  opacity: number;
  width: number;
  drift: number;
  depth: number;
  shimmer: number;
}

function createRainDrop(width: number, height: number, randomY = true): RainDrop {
  // Bias most drops towards the distant rain curtain while retaining a few
  // long, sharp foreground streaks. This reads as heavy rain without turning
  // the window into a flat layer of identical white lines.
  const depth = Math.pow(Math.random(), 1.55);
  return {
    x: Math.random() * (width + 80) + 18,
    y: randomY ? Math.random() * height : -60 - Math.random() * height * 0.35,
    length: 10 + depth * 48,
    speed: 190 + depth * 540,
    opacity: 0.10 + depth * 0.46,
    width: 0.42 + depth * 1.25,
    drift: -38 - depth * 76,
    depth,
    shimmer: Math.random() * Math.PI * 2,
  };
}

function RainWindowCanvas({ paused }: { paused: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    let frame = 0;
    let previousTime = performance.now();
    let drops: RainDrop[] = [];

    const resize = () => {
      const bounds = canvas.getBoundingClientRect();
      const ratio = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.max(1, Math.round(bounds.width * ratio));
      canvas.height = Math.max(1, Math.round(bounds.height * ratio));
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      // A storm needs a continuous rain curtain. Density scales with the
      // visible window but is capped to keep the animation inexpensive.
      const count = Math.min(720, Math.max(120, Math.round(bounds.width * bounds.height / 720)));
      drops = Array.from({ length: count }, () => createRainDrop(bounds.width, bounds.height));
    };

    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const draw = (time: number) => {
      const bounds = canvas.getBoundingClientRect();
      const elapsed = Math.min(0.035, (time - previousTime) / 1000);
      previousTime = time;
      context.clearRect(0, 0, bounds.width, bounds.height);
      if (!paused) {
        context.lineCap = "round";
        context.globalCompositeOperation = "lighter";
        const gust = 0.88 + Math.sin(time / 1350) * 0.12 + Math.sin(time / 4300) * 0.07;
        for (let index = 0; index < drops.length; index += 1) {
          let drop = drops[index];
          drop.y += drop.speed * gust * elapsed;
          drop.x += drop.drift * gust * elapsed;
          if (drop.y - drop.length > bounds.height || drop.x < -80) {
            drop = createRainDrop(bounds.width, bounds.height, false);
            drops[index] = drop;
          }
          const tailX = drop.x - drop.drift * (drop.length / drop.speed);
          const shimmer = 0.9 + Math.sin(time / 180 + drop.shimmer) * 0.1;
          const alpha = drop.opacity * shimmer;
          const gradient = context.createLinearGradient(tailX, drop.y - drop.length, drop.x, drop.y);
          gradient.addColorStop(0, "rgba(187,215,230,0)");
          gradient.addColorStop(0.34, `rgba(196,224,238,${alpha * 0.34})`);
          gradient.addColorStop(0.82, `rgba(220,239,248,${alpha * 0.78})`);
          gradient.addColorStop(1, `rgba(242,250,253,${alpha})`);
          context.strokeStyle = gradient;
          context.lineWidth = drop.width;
          context.shadowBlur = drop.depth > 0.72 ? 1.4 : 0;
          context.shadowColor = "rgba(208,235,248,.28)";
          context.beginPath();
          context.moveTo(tailX, drop.y - drop.length);
          context.lineTo(drop.x, drop.y);
          context.stroke();
        }
        context.shadowBlur = 0;
        context.globalCompositeOperation = "source-over";
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [paused]);

  return <canvas ref={canvasRef} className="rain-window-canvas" />;
}

function noiseBuffer(context: AudioContext, seconds: number, crackle = false): AudioBuffer {
  const length = Math.ceil(context.sampleRate * seconds);
  const buffer = context.createBuffer(1, length, context.sampleRate);
  const data = buffer.getChannelData(0);
  for (let index = 0; index < length; index += 1) {
    const base = Math.random() * 2 - 1;
    const impulse = crackle && Math.random() < 0.00065 ? (Math.random() * 2 - 1) * 4 : 0;
    data[index] = Math.max(-1, Math.min(1, base * (crackle ? 0.14 : 0.75) + impulse));
  }
  return buffer;
}

function startAtmosphere(kind: NonNullable<DeepSkinDefinition["atmosphere"]>): AudioGraph {
  const context = new AudioContext();
  const gain = context.createGain();
  gain.gain.value = 0;
  gain.connect(context.destination);
  const nodes: AudioScheduledSourceNode[] = [];

  const addNoise = (filterType: BiquadFilterType, frequency: number, q: number, level: number, crackle = false) => {
    const source = context.createBufferSource();
    const filter = context.createBiquadFilter();
    const localGain = context.createGain();
    source.buffer = noiseBuffer(context, 4, crackle);
    source.loop = true;
    filter.type = filterType;
    filter.frequency.value = frequency;
    filter.Q.value = q;
    localGain.gain.value = level;
    source.connect(filter).connect(localGain).connect(gain);
    source.start();
    nodes.push(source);
  };

  if (kind === "rain") {
    addNoise("lowpass", 3300, 0.4, 0.62);
    addNoise("highpass", 900, 0.2, 0.16);
  } else if (kind === "navigation") {
    addNoise("lowpass", 180, 0.7, 0.16);
    const hum = context.createOscillator();
    const humGain = context.createGain();
    hum.type = "sine";
    hum.frequency.value = 54;
    humGain.gain.value = 0.18;
    hum.connect(humGain).connect(gain);
    hum.start();
    nodes.push(hum);
  } else {
    addNoise("bandpass", 2100, 0.45, 0.24, true);
    addNoise("lowpass", 320, 0.5, 0.08);
  }

  const resume = () => void context.resume();
  window.addEventListener("pointerdown", resume, { once: true });
  void context.resume();
  return {
    context,
    gain,
    stop: () => {
      window.removeEventListener("pointerdown", resume);
      nodes.forEach((node) => {
        try { node.stop(); } catch { /* source already stopped */ }
      });
      void context.close();
    },
  };
}

export function SkinAtmosphere({ skin, motionEnabled, soundEnabled, soundVolume }: Props) {
  const audioRef = useRef<AudioGraph | null>(null);
  const [paused, setPaused] = useState(() => document.hidden || !document.hasFocus());
  const atmosphere = skin?.atmosphere;

  useEffect(() => {
    const update = () => setPaused(document.hidden || !document.hasFocus());
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
    };
  }, []);

  useEffect(() => {
    audioRef.current?.stop();
    audioRef.current = null;
    if (!atmosphere || !soundEnabled) return;
    const graph = startAtmosphere(atmosphere);
    graph.gain.gain.setTargetAtTime(Math.min(0.34, soundVolume / 290), graph.context.currentTime, 0.2);
    audioRef.current = graph;
    return () => {
      graph.stop();
      if (audioRef.current === graph) audioRef.current = null;
    };
  }, [atmosphere, soundEnabled]);

  useEffect(() => {
    const graph = audioRef.current;
    if (!graph) return;
    graph.gain.gain.setTargetAtTime(paused ? 0 : Math.min(0.34, soundVolume / 290), graph.context.currentTime, 0.08);
    if (!paused) void graph.context.resume();
  }, [paused, soundVolume]);

  if (!atmosphere || !motionEnabled) return null;
  return (
    <div className={`skin-atmosphere skin-atmosphere-${atmosphere}${paused ? " paused" : ""}`} aria-hidden="true">
      {atmosphere === "rain" && <><RainWindowCanvas paused={paused} /><i className="rain-window-glass" /></>}
      {atmosphere === "navigation" && <><i className="navigation-stars" /><i className="navigation-radar" /><i className="navigation-lights" /></>}
      {atmosphere === "records" && <><i className="records-disc-glint" /><i className="records-light" /><i className="records-dust" /></>}
    </div>
  );
}
