import { useEffect, useRef, useState } from "react";
import { CheckIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import type { PetDefinition, PetSettings } from "../petSettings";
import { BUILTIN_PETS, clampPetSize } from "../petSettings";
import { Switch } from "./ui/Switch";

interface Props {
  settings: PetSettings;
  onChange: (next: PetSettings) => void;
}

const ACCEPTED_TYPES = new Set(["image/png", "image/apng", "image/gif", "image/webp"]);
const MAX_CUSTOM_PET_BYTES = 2 * 1024 * 1024;

function readDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error("读取宠物图片失败"));
    reader.readAsDataURL(file);
  });
}

export function PetSettingsPanel({ settings, onChange }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [error, setError] = useState("");
  const [importing, setImporting] = useState(false);
  const importInFlight = useRef(false);
  const latestSettings = useRef(settings);
  latestSettings.current = settings;
  const selectionRevision = useRef(0);
  const enabledRevision = useRef(0);
  const disposed = useRef(false);
  useEffect(() => { disposed.current = false; return () => { disposed.current = true; }; }, []);
  const pets = [...BUILTIN_PETS, ...settings.customPets];

  const selectPet = (pet: PetDefinition) => {
    selectionRevision.current += 1; enabledRevision.current += 1;
    onChange({ ...settings, enabled: true, selectedId: pet.id });
  };

  const importPet = async (file: File | undefined) => {
    if (!file || importInFlight.current) return;
    setError("");
    if (!ACCEPTED_TYPES.has(file.type)) {
      setError("请选择 PNG、APNG、GIF 或 WebP 图片。");
      if (inputRef.current) inputRef.current.value = "";
      return;
    }
    if (file.size > MAX_CUSTOM_PET_BYTES) {
      setError("自定义宠物图片不能超过 2 MB。");
      if (inputRef.current) inputRef.current.value = "";
      return;
    }
    importInFlight.current = true; setImporting(true);
    const selectedAtStart = selectionRevision.current;
    const enabledAtStart = enabledRevision.current;
    try {
      const src = await readDataUrl(file);
      if (disposed.current) return;
      const id = `custom-${crypto.randomUUID()}`;
      const name = file.name.replace(/\.[^.]+$/, "").trim().slice(0, 30) || "自定义宠物";
      const pet: PetDefinition = {
        id,
        name,
        description: "我的自定义桌宠",
        src,
        custom: true,
      };
      onChange({
        ...latestSettings.current,
        enabled: enabledRevision.current === enabledAtStart ? true : latestSettings.current.enabled,
        selectedId: selectionRevision.current === selectedAtStart ? id : latestSettings.current.selectedId,
        customPets: [...latestSettings.current.customPets, pet],
      });
    } catch (reason) {
      if (!disposed.current) setError(reason instanceof Error ? reason.message : "导入宠物失败");
    } finally {
      importInFlight.current = false;
      if (!disposed.current) setImporting(false);
      if (inputRef.current) inputRef.current.value = "";
    }
  };

  const removeCustomPet = (petId: string) => {
    if (settings.selectedId === petId) selectionRevision.current += 1;
    const remaining = settings.customPets.filter((pet) => pet.id !== petId);
    onChange({
      ...settings,
      customPets: remaining,
      selectedId: settings.selectedId === petId ? BUILTIN_PETS[0].id : settings.selectedId,
    });
  };

  return (
    <div className="pet-settings-panel">
      <section className="pet-settings-card pet-settings-master">
        <div>
          <h3>显示桌宠</h3>
          <p>在工作区右下角显示水果伙伴，并根据 Agent 状态做出动作。</p>
        </div>
        <Switch
          className="pet-switch"
          label="显示桌宠"
          checked={settings.enabled}
          onChange={(checked) => { enabledRevision.current += 1; onChange({ ...settings, enabled: checked }); }}
        />
      </section>

      <section className="pet-settings-section">
        <div className="pet-settings-section-heading">
          <div>
            <h3>选择水果伙伴</h3>
            <p>四款橙子与四种水果，可随时切换。</p>
          </div>
          <button className="btn secondary pet-import" disabled={importing} onClick={() => inputRef.current?.click()}>
            <PlusIcon size={15} weight="bold" />
            {importing ? "正在导入…" : "导入自定义"}
          </button>
          <input
            ref={inputRef}
            className="pet-file-input"
            type="file"
            disabled={importing}
            accept="image/png,image/apng,image/gif,image/webp"
            onChange={(event) => void importPet(event.target.files?.[0])}
          />
        </div>
        {error && <p className="pet-import-error" role="alert">{error}</p>}
        <div className="pet-choice-grid">
          {pets.map((pet) => {
            const selected = settings.selectedId === pet.id;
            return (
              <article className={`pet-choice${selected ? " selected" : ""}`} key={pet.id}>
                <button className="pet-choice-main" onClick={() => selectPet(pet)}>
                  <span className="pet-choice-preview">
                    <img src={pet.src} alt="" draggable={false} />
                  </span>
                  <span className="pet-choice-copy">
                    <strong>{pet.name}</strong>
                    <small>{pet.description}</small>
                  </span>
                  {selected && <span className="pet-choice-check"><CheckIcon size={13} weight="bold" /></span>}
                </button>
                {pet.custom && (
                  <button
                    className="pet-choice-delete"
                    aria-label={`删除 ${pet.name}`}
                    onClick={() => removeCustomPet(pet.id)}
                  >
                    <TrashIcon size={14} />
                  </button>
                )}
              </article>
            );
          })}
        </div>
      </section>

      <section className="pet-settings-card pet-size-card">
        <div>
          <h3>宠物大小</h3>
          <p>可以直接拖动桌宠改变位置。</p>
        </div>
        <div className="pet-size-control">
          <input
            type="range"
            min={72}
            max={192}
            step={4}
            value={settings.size}
            onChange={(event) => onChange({ ...settings, size: clampPetSize(Number(event.target.value)) })}
          />
          <span>{settings.size}px</span>
        </div>
      </section>
    </div>
  );
}
