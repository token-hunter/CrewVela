import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SkinSettingsPanel } from "../../src/components/SkinSettingsPanel";
import { PetSettingsPanel } from "../../src/components/PetSettingsPanel";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_COMPANION_SETTINGS } from "../../src/companionSettings";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { CUSTOM_SKINS_STORAGE_KEY } from "../../src/customSkins";
import "../../src/styles.css";

const pending: Array<{ command: string; args: unknown; resolve: (value: any) => void; reject: (error: Error) => void }> = [];
const calls: Array<{ command: string; args: unknown }> = [];
const reads: Array<() => void> = [];
const mock = { pending, calls, reads, completedReads: 0, invoke(command: string, args: unknown) { calls.push({ command, args }); return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject })); }, release(command: string, value: unknown, error?: string) {
  const index = pending.findIndex(item => item.command === command); if (index < 0) throw Error(`No pending ${command}`);
  const item = pending.splice(index, 1)[0]; if (error) item.reject(Error(error)); else item.resolve(value);
}, readPet() { const read = reads.shift(); if (!read) throw Error("No pending pet image"); read(); } };
(window as any).skinPetMock = mock;
const Reader = FileReader;
window.FileReader = class extends Reader { readAsDataURL(file: Blob) { reads.push(() => { this.addEventListener("loadend", () => { mock.completedReads++; }, { once: true }); super.readAsDataURL(file); }); } } as typeof FileReader;
const image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
localStorage.setItem(CUSTOM_SKINS_STORAGE_KEY, JSON.stringify([{ id: "old-skin", name: "Original skin", description: "fixture", importedAt: 1, appearance: DEFAULT_APPEARANCE, media: { kind: "video", videoPath: "/fixture.mp4", previewPath: null, audioPath: "/fixture.mp3", videoHasAudio: true, sourceFormat: "fixture" } }]));
function Fixture() {
  const [settings, setSettings] = useState({ ...DEFAULT_COMPANION_SETTINGS, customSkinId: "old-skin", skinSoundEnabled: true });
  const [appearance, setAppearance] = useState(DEFAULT_APPEARANCE);
  const [pet, setPet] = useState({ ...DEFAULT_PET_SETTINGS, enabled: true, customPets: [{ id: "old-pet", name: "Old pet", description: "fixture", src: image, custom: true }] });
  const [shown, setShown] = useState(true);
  const mode = new URLSearchParams(location.search).get("mode");
  return mode === "pet" ? <><button onClick={() => setShown(false)}>关闭桌宠面板</button>{shown && <PetSettingsPanel settings={pet} onChange={setPet} />}<output id="pet-state">{JSON.stringify(pet)}</output></> : <><SkinSettingsPanel settings={settings} appearance={appearance} onChange={setSettings} onAppearanceChange={setAppearance} /><output id="skin-state">{JSON.stringify(settings)}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
