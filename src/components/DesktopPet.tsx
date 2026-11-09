import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PetSettings } from "../petSettings";
import { resolvePet } from "../petSettings";
import {
  clampPetPoint,
  createMovementTarget,
  createPetActionDeck,
  interpolatePetPosition,
  isMovingPetAction,
  PET_ANIMATION_COLUMNS,
  PET_ANIMATION_ROWS,
  petAnimationFrame,
  petActionDuration,
  type PetAction,
  type PetDirection,
  type PetPoint,
} from "../petBehavior";
import type { SessionRecord } from "../types";

interface Props {
  settings: PetSettings;
  sessions: SessionRecord[];
  onPositionChange: (position: PetPoint) => void;
}

const ACTION_LABELS: Record<PetAction, string> = {
  idle: "发呆", blink: "眨眼", sleep: "睡觉", stretch: "伸懒腰", walk: "散步", run: "跑步",
  jump: "跳跃", chase: "追逐", read: "看书", coffee: "喝咖啡", computer: "玩电脑", sing: "唱歌",
  eat: "吃饭", music: "听音乐", celebrate: "庆祝", think: "思考", remind: "提醒", tumble: "摔倒后爬起",
};

function viewportBounds() {
  return { width: window.innerWidth, height: window.innerHeight };
}

export function DesktopPet({ settings, sessions, onPositionChange }: Props) {
  const pet = resolvePet(settings);
  const dragging = useRef<{ dx: number; dy: number } | null>(null);
  const movementFrame = useRef<number | null>(null);
  const actionDeck = useRef<PetAction[]>([]);
  const actionCursor = useRef(0);
  const onPositionChangeRef = useRef(onPositionChange);
  const defaultPosition = useMemo(
    () => ({
      x: Math.max(12, window.innerWidth - settings.size - 26),
      y: Math.max(12, window.innerHeight - settings.size - 34),
    }),
    [settings.size],
  );
  const [position, setPosition] = useState(settings.position ?? defaultPosition);
  const positionRef = useRef(position);
  const [action, setAction] = useState<PetAction>("idle");
  const [actionTick, setActionTick] = useState(0);
  const [animationFrame, setAnimationFrame] = useState(0);
  const [direction, setDirection] = useState<PetDirection>("right");
  const needsAttention = sessions.some((session) => session.status === "waiting" || session.status === "error");

  useEffect(() => { positionRef.current = position; }, [position]);
  useEffect(() => { onPositionChangeRef.current = onPositionChange; }, [onPositionChange]);

  const stopMovement = useCallback(() => {
    if (movementFrame.current !== null) cancelAnimationFrame(movementFrame.current);
    movementFrame.current = null;
  }, []);

  const chooseNextAction = useCallback(() => {
    if (needsAttention) return "remind" as const;
    if (actionDeck.current.length === 0 || actionCursor.current >= actionDeck.current.length) {
      actionDeck.current = createPetActionDeck();
      actionCursor.current = 0;
    }
    const next = actionDeck.current[actionCursor.current];
    actionCursor.current += 1;
    return next;
  }, [needsAttention]);

  useEffect(() => {
    if (!settings.enabled) return;
    const timer = window.setTimeout(() => {
      setAction(chooseNextAction());
      setActionTick((current) => current + 1);
    }, petActionDuration(action));
    return () => window.clearTimeout(timer);
  }, [action, actionTick, chooseNextAction, settings.enabled]);

  useEffect(() => {
    setAnimationFrame(0);
    if (!settings.enabled || !pet.actionAtlas) return;
    const { durationMs } = petAnimationFrame(action, 0);
    const timer = window.setInterval(() => {
      setAnimationFrame((current) => (current + 1) % 3);
    }, durationMs);
    return () => window.clearInterval(timer);
  }, [action, actionTick, pet.actionAtlas, settings.enabled]);

  useEffect(() => {
    if (needsAttention) setAction("remind");
  }, [needsAttention]);

  useEffect(() => {
    stopMovement();
    if (!settings.enabled || dragging.current || !isMovingPetAction(action)) return;
    const start = positionRef.current;
    const target = createMovementTarget(action, start, viewportBounds(), settings.size);
    setDirection(target.x < start.x ? "left" : "right");
    const startedAt = performance.now();
    const duration = petActionDuration(action);

    const tick = (now: number) => {
      if (dragging.current) return;
      const progress = Math.min(1, (now - startedAt) / duration);
      const next = clampPetPoint(
        interpolatePetPosition(action, start, target, progress, settings.size),
        viewportBounds(),
        settings.size,
      );
      positionRef.current = next;
      setPosition(next);
      if (progress < 1) movementFrame.current = requestAnimationFrame(tick);
      else {
        movementFrame.current = null;
        onPositionChangeRef.current(next);
      }
    };
    movementFrame.current = requestAnimationFrame(tick);
    return stopMovement;
  }, [action, actionTick, settings.enabled, settings.size, stopMovement]);

  useEffect(() => {
    const clamp = () => {
      const next = clampPetPoint(positionRef.current, viewportBounds(), settings.size);
      positionRef.current = next;
      setPosition(next);
    };
    window.addEventListener("resize", clamp);
    clamp();
    return () => window.removeEventListener("resize", clamp);
  }, [settings.size]);

  useEffect(() => {
    actionDeck.current = [];
    actionCursor.current = 0;
    setAction("idle");
  }, [pet.id]);

  if (!settings.enabled) return null;

  const atlasFrame = petAnimationFrame(action, animationFrame);
  const atlasColumnPosition = atlasFrame.column * (100 / (PET_ANIMATION_COLUMNS - 1));
  const atlasRowPosition = atlasFrame.row * (100 / (PET_ANIMATION_ROWS - 1));
  const spriteStyle = pet.actionAtlas
    ? {
        backgroundImage: `url(${pet.actionAtlas})`,
        backgroundPosition: `${atlasColumnPosition}% ${atlasRowPosition}%`,
      }
    : undefined;

  return (
    <div
      className={`desktop-pet desktop-pet-action-${action} desktop-pet-facing-${direction}`}
      data-pet-action={action}
      data-pet-frame={animationFrame}
      style={{ left: position.x, top: position.y, width: settings.size, height: settings.size }}
      role="img"
      aria-label={`${pet.name}正在${ACTION_LABELS[action]}`}
      title={`${pet.name} · ${ACTION_LABELS[action]} · 拖动可调整位置`}
      onPointerDown={(event) => {
        stopMovement();
        dragging.current = { dx: event.clientX - positionRef.current.x, dy: event.clientY - positionRef.current.y };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!dragging.current) return;
        const next = clampPetPoint(
          { x: event.clientX - dragging.current.dx, y: event.clientY - dragging.current.dy },
          viewportBounds(),
          settings.size,
        );
        positionRef.current = next;
        setPosition(next);
      }}
      onPointerUp={(event) => {
        if (!dragging.current) return;
        dragging.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
        onPositionChangeRef.current(positionRef.current);
      }}
      onPointerCancel={() => { dragging.current = null; }}
    >
      {pet.actionAtlas ? (
        <span className="desktop-pet-sprite" style={spriteStyle} aria-hidden="true" />
      ) : (
        <img src={pet.src} alt="" draggable={false} />
      )}
    </div>
  );
}
