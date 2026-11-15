import { useState } from "react";
import { createRoot } from "react-dom/client";
import { PetSettingsPanel } from "../../src/components/PetSettingsPanel";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import "../../src/styles.css";
const reads: Array<() => void> = [];
const mock = { reads, completedReads: 0, readPet() { const read = reads.shift(); if (!read) throw Error("No pending pet image"); read(); } };
(window as any).petMock = mock;
const Reader = FileReader;
window.FileReader = class extends Reader { readAsDataURL(file: Blob) { reads.push(() => { this.addEventListener("loadend", () => { mock.completedReads++; }, { once: true }); super.readAsDataURL(file); }); } } as typeof FileReader;
const image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
function Fixture() {
  const [pet, setPet] = useState({ ...DEFAULT_PET_SETTINGS, enabled: true, customPets: [{ id: "old-pet", name: "Old pet", description: "fixture", src: image, custom: true }] });
  const [shown, setShown] = useState(true);
  return <><button onClick={() => setShown(false)}>关闭桌宠面板</button>{shown && <PetSettingsPanel settings={pet} onChange={setPet} />}<output id="pet-state">{JSON.stringify(pet)}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
