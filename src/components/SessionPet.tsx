import { useEffect, useState } from "react";
import claudePetSleep from "../assets/claude-code-pet-sleep.png";
import claudePetWorkingBoth from "../assets/claude-code-pet-working-both.png";
import claudePetWorkingLeft from "../assets/claude-code-pet-working-left.png";
import claudePetWorkingRight from "../assets/claude-code-pet-working-right.png";
import claudePetSmokeOne from "../assets/claude-code-pet-working-smoke-1.png";
import claudePetSmokeTwo from "../assets/claude-code-pet-working-smoke-2.png";
import claudePetSmokeThree from "../assets/claude-code-pet-working-smoke-3.png";
import type { SessionStatus } from "../types";
import { getPetWorkPhase } from "./sessionPetState";

type PetMode = "working" | "idle";

interface Props {
  status: SessionStatus;
}

export function getPetMode(status: SessionStatus): PetMode {
  return status === "running" ? "working" : "idle";
}

export function SessionPet({ status }: Props) {
  const mode = getPetMode(status);
  const isWorking = mode === "working";
  const [elapsedMs, setElapsedMs] = useState(0);

  useEffect(() => {
    if (!isWorking) {
      setElapsedMs(0);
      return;
    }

    const startedAt = Date.now();
    setElapsedMs(0);
    const timer = window.setInterval(() => {
      setElapsedMs(Date.now() - startedAt);
    }, 1_000);

    return () => window.clearInterval(timer);
  }, [isWorking]);

  const phase = getPetWorkPhase(status, elapsedMs);
  const label = isWorking ? "干活中" : "摸鱼中";
  const description =
    phase === "overheated"
      ? "Claude 工作时间较长，正在敲键盘并冒烟"
      : phase === "busy"
        ? "Claude 正在快速敲键盘工作"
        : isWorking
          ? "Claude 正在敲键盘工作"
          : "Claude 正在睡觉摸鱼";

  return (
    <div
      className={`session-pet session-pet-${mode} session-pet-phase-${phase}`}
      role="status"
      aria-label={description}
      title={description}
    >
      <span className="session-pet-stage" aria-hidden="true">
        {isWorking ? (
          <span className="session-pet-working-frames">
            <img
              className="session-pet-working-frame session-pet-working-left"
              src={claudePetWorkingLeft}
              alt=""
              draggable={false}
            />
            <img
              className="session-pet-working-frame session-pet-working-right"
              src={claudePetWorkingRight}
              alt=""
              draggable={false}
            />
            <img
              className="session-pet-working-frame session-pet-working-both"
              src={claudePetWorkingBoth}
              alt=""
              draggable={false}
            />
            {phase === "overheated" && (
              <>
                <img className="session-pet-smoke session-pet-smoke-one" src={claudePetSmokeOne} alt="" draggable={false} />
                <img className="session-pet-smoke session-pet-smoke-two" src={claudePetSmokeTwo} alt="" draggable={false} />
                <img className="session-pet-smoke session-pet-smoke-three" src={claudePetSmokeThree} alt="" draggable={false} />
              </>
            )}
          </span>
        ) : (
          <>
            <img className="session-pet-sleep-image" src={claudePetSleep} alt="" draggable={false} />
            <span className="session-pet-bubbles">
              <span className="session-pet-bubble" />
              <span className="session-pet-bubble" />
              <span className="session-pet-bubble" />
            </span>
          </>
        )}
      </span>
      <span className="session-pet-label">{label}</span>
    </div>
  );
}
