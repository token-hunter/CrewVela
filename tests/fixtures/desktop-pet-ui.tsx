import { useCallback, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { DesktopPet } from "../../src/components/DesktopPet";
import { BUILTIN_PETS, DEFAULT_PET_SETTINGS, type PetSettings } from "../../src/petSettings";
import type { PetPoint } from "../../src/petBehavior";
import type { SessionRecord, SessionStatus } from "../../src/types";
import "../../src/styles.css";

type SessionInput = Pick<SessionRecord, "id" | "status">;
declare global {
  interface Window {
    desktopPetFixture: {
      setSessions: (sessions: SessionInput[]) => void;
      patchSettings: (patch: Partial<PetSettings>) => void;
      unmount: () => void;
      positions: PetPoint[];
      pets: string[];
    };
  }
}
document.documentElement.dataset.theme = "dark";
const query = new URLSearchParams(location.search);
const initialStatus = query.get("status") as SessionStatus | null;

function Fixture() {
  const [settings, setSettings] = useState<PetSettings>({ ...DEFAULT_PET_SETTINGS, enabled: true, selectedId: query.get("pet") ?? "orange-coder" });
  const [sessions, setSessions] = useState<SessionInput[]>(initialStatus ? [{ id: "fixture-session", status: initialStatus }] : []);
  const [shown, setShown] = useState(true);
  const positions = useRef<PetPoint[]>([]);
  const onPositionChange = useCallback((position: PetPoint) => { positions.current.push(position); }, []);
  window.desktopPetFixture = {
    setSessions,
    patchSettings: (patch) => setSettings((current) => ({ ...current, ...patch })),
    unmount: () => setShown(false),
    positions: positions.current,
    pets: BUILTIN_PETS.map((pet) => pet.id),
  };
  return <>
    <p>桌宠动作隔离测试，不连接真实会话。</p>
    {shown && <DesktopPet settings={settings} sessions={sessions as SessionRecord[]} onPositionChange={onPositionChange} />}
  </>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
