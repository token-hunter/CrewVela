import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CustomSkin } from "../customSkins";

interface Props {
  skin: CustomSkin | undefined;
  motionEnabled: boolean;
  soundEnabled: boolean;
  soundVolume: number;
}

function localAsset(path: string | null | undefined): string | undefined {
  return path ? convertFileSrc(path) : undefined;
}

export function DynamicSkinLayer({ skin, motionEnabled, soundEnabled, soundVolume }: Props) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [suspended, setSuspended] = useState(() => document.hidden || !document.hasFocus());
  const media = skin?.media;
  const videoUrl = useMemo(() => localAsset(media?.videoPath), [media?.videoPath]);
  const previewUrl = useMemo(() => localAsset(media?.previewPath), [media?.previewPath]);
  const audioUrl = useMemo(() => localAsset(media?.audioPath), [media?.audioPath]);
  const audibleVideo = Boolean(soundEnabled && media?.videoHasAudio && !audioUrl);
  const volume = Math.max(0, Math.min(1, soundVolume / 100));

  useEffect(() => {
    const update = () => setSuspended(document.hidden || !document.hasFocus());
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
    const video = videoRef.current;
    const audio = audioRef.current;
    if (video) {
      video.muted = !audibleVideo;
      video.volume = volume;
    }
    if (audio) audio.volume = volume;

    const shouldPlayVideo = Boolean(media && !suspended && (motionEnabled || audibleVideo));
    const shouldPlayAudio = Boolean(media && audioUrl && soundEnabled && !suspended);
    const play = () => {
      if (shouldPlayVideo) {
        void video?.play().catch(() => undefined);
      } else video?.pause();
      if (shouldPlayAudio) void audio?.play().catch(() => undefined);
      else audio?.pause();
    };
    play();

    window.addEventListener("pointerdown", play, { once: true });
    return () => window.removeEventListener("pointerdown", play);
  }, [audioUrl, audibleVideo, media, motionEnabled, soundEnabled, suspended, volume]);

  if (!media || !videoUrl) return null;
  return (
    <div className={`dynamic-skin-layer${suspended ? " paused" : ""}`} aria-hidden="true">
      <video
        ref={videoRef}
        key={videoUrl}
        src={videoUrl}
        poster={previewUrl}
        autoPlay={motionEnabled}
        loop
        playsInline
        muted={!audibleVideo}
        preload="auto"
      />
      {audioUrl && (
        <audio
          ref={audioRef}
          key={audioUrl}
          src={audioUrl}
          autoPlay={motionEnabled && soundEnabled}
          loop
          preload="auto"
        />
      )}
    </div>
  );
}
