import { useEffect, useRef, useState } from "react";
import { ArrowCounterClockwiseIcon, CheckIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import type { PetDefinition, PetSettings } from "../petSettings";
import { BUILTIN_PETS, clampPetSize } from "../petSettings";
import {
  PET_ANIMATION_COLUMNS,
  PET_ANIMATION_FRAME_COUNT,
  PET_ANIMATION_ROWS,
  petAnimationFrame,
  type PetAction,
} from "../petBehavior";
import { Switch } from "./ui/Switch";

interface Props {
  settings: PetSettings;
  onChange: (next: PetSettings) => void;
}

const ACCEPTED_TYPES = new Set(["image/png", "image/apng", "image/gif", "image/webp"]);
const MAX_CUSTOM_PET_BYTES = 2 * 1024 * 1024;

const PREVIEW_ACTIONS: ReadonlyArray<{ id: PetAction; label: string }> = [
  { id: "idle", label: "发呆" },
  { id: "computer", label: "玩电脑" },
  { id: "coffee", label: "喝咖啡" },
  { id: "music", label: "听音乐" },
  { id: "celebrate", label: "庆祝" },
  { id: "sleep", label: "睡觉" },
];

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
  const [previewAction, setPreviewAction] = useState<PetAction>("idle");
  const [animationFrame, setAnimationFrame] = useState(0);
  const [positionResetNotice, setPositionResetNotice] = useState(false);
  const resetNoticeTimer = useRef<number | null>(null);
  const importInFlight = useRef(false);
  const latestSettings = useRef(settings);
  latestSettings.current = settings;
  const selectionRevision = useRef(0);
  const enabledRevision = useRef(0);
  const disposed = useRef(false);

  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      if (resetNoticeTimer.current !== null) window.clearTimeout(resetNoticeTimer.current);
    };
  }, []);

  const pets = [...BUILTIN_PETS, ...settings.customPets];
  const activePet = pets.find((pet) => pet.id === settings.selectedId) ?? pets[0];

  useEffect(() => {
    setAnimationFrame(0);
    if (!settings.enabled || !activePet?.actionAtlas) return;
    const { durationMs } = petAnimationFrame(previewAction, 0);
    const timer = window.setInterval(() => {
      setAnimationFrame((current) => (current + 1) % PET_ANIMATION_FRAME_COUNT);
    }, durationMs);
    return () => window.clearInterval(timer);
  }, [activePet?.actionAtlas, activePet?.id, previewAction, settings.enabled]);

  const selectPet = (pet: PetDefinition) => {
    selectionRevision.current += 1;
    enabledRevision.current += 1;
    onChange({ ...settings, enabled: true, selectedId: pet.id });
  };

  const resetPosition = () => {
    onChange({ ...settings, position: null });
    setPositionResetNotice(true);
    if (resetNoticeTimer.current !== null) window.clearTimeout(resetNoticeTimer.current);
    resetNoticeTimer.current = window.setTimeout(() => {
      if (!disposed.current) setPositionResetNotice(false);
    }, 1800);
  };

  const cyclePreviewAction = () => {
    if (!activePet?.actionAtlas) return;
    const index = PREVIEW_ACTIONS.findIndex((item) => item.id === previewAction);
    const next = PREVIEW_ACTIONS[(index + 1) % PREVIEW_ACTIONS.length];
    setPreviewAction(next.id);
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
    importInFlight.current = true;
    setImporting(true);
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

  const stageImgSize = Math.round(48 + ((settings.size - 72) / (192 - 72)) * 42);
  const currentActionMeta = PREVIEW_ACTIONS.find((item) => item.id === previewAction) ?? PREVIEW_ACTIONS[0];
  const atlasFrame = petAnimationFrame(previewAction, animationFrame);
  const atlasColumnPosition = atlasFrame.column * (100 / (PET_ANIMATION_COLUMNS - 1));
  const atlasRowPosition = atlasFrame.row * (100 / (PET_ANIMATION_ROWS - 1));

  return (
    <div className="pet-settings-panel">
      <div className="dialog-heading settings-heading pet-settings-heading">
        <div>
          <h2 id="settings-title">陪伴</h2>
          <p>管理桌面宠物的角色、互动和显示。</p>
        </div>
        <div className="settings-heading-actions pet-settings-master-inline">
          <span className="pet-switch-caption" aria-hidden="true">显示桌宠</span>
          <Switch
            className="pet-switch"
            label="显示桌宠"
            checked={settings.enabled}
            onChange={(checked) => {
              enabledRevision.current += 1;
              onChange({ ...settings, enabled: checked });
            }}
          />
        </div>
      </div>

      {activePet && (
        <section className="pet-settings-card pet-showcase-banner">
          <button
            type="button"
            className={`pet-live-stage${settings.enabled ? "" : " disabled"}${activePet.actionAtlas ? " interactive" : ""}`}
            onClick={cyclePreviewAction}
            title={activePet.actionAtlas ? "点击切换试播动作" : activePet.name}
            aria-label={activePet.actionAtlas ? `预览舞台：${activePet.name}，点击切换试播动作` : `预览舞台：${activePet.name}`}
          >
            <span className="pet-live-stage-header">
              <span className="pet-stage-tag">LIVE PREVIEW</span>
              <span className="pet-stage-status">
                {settings.enabled ? (activePet.actionAtlas ? currentActionMeta.label : "互动陪伴中") : "已隐藏"}
              </span>
            </span>
            <span
              className={`pet-live-stage-canvas${settings.enabled && activePet.actionAtlas ? ` desktop-pet-action-${previewAction}` : ""}`}
            >
              {activePet.actionAtlas ? (
                <span
                  className="desktop-pet-sprite pet-stage-sprite"
                  aria-hidden="true"
                  style={{
                    width: `${stageImgSize}px`,
                    height: `${stageImgSize}px`,
                    backgroundImage: `url(${activePet.actionAtlas})`,
                    backgroundPosition: `${atlasColumnPosition}% ${atlasRowPosition}%`,
                  }}
                />
              ) : (
                <img
                  src={activePet.src}
                  alt=""
                  draggable={false}
                  style={{ width: `${stageImgSize}px`, height: `${stageImgSize}px` }}
                />
              )}
            </span>
          </button>

          <div className="pet-showcase-controls">
            <div className="pet-showcase-top">
              <div className="pet-showcase-identity">
                <h3>{activePet.name}</h3>
                <span>{activePet.description}</span>
              </div>
              <button
                type="button"
                className="btn secondary pet-reset-position"
                onClick={resetPosition}
                title="将桌宠恢复到工作区默认右下角位置"
              >
                {positionResetNotice ? <CheckIcon size={13} weight="bold" aria-hidden="true" /> : <ArrowCounterClockwiseIcon size={13} aria-hidden="true" />}
                <span>{positionResetNotice ? "已归位" : "重置位置"}</span>
              </button>
            </div>

            <div className="pet-size-row">
              <div className="pet-size-label">
                <strong>宠物大小</strong>
                <small>可直接拖动桌宠改变位置</small>
                <span className="pet-size-value">{settings.size}px</span>
              </div>
              <div className="pet-size-control">
                <input
                  type="range"
                  aria-label="宠物大小"
                  min={72}
                  max={192}
                  step={4}
                  value={settings.size}
                  onChange={(event) => onChange({ ...settings, size: clampPetSize(Number(event.target.value)) })}
                />
              </div>
            </div>

            <div className="pet-action-bar">
              <span className="pet-action-label">试播动作</span>
              {activePet.actionAtlas ? (
                <div className="pet-action-pills" role="group" aria-label="试播动作">
                  {PREVIEW_ACTIONS.map((action) => {
                    const active = previewAction === action.id;
                    return (
                      <button
                        key={action.id}
                        type="button"
                        className={`pet-action-pill${active ? " active" : ""}`}
                        aria-pressed={active}
                        onClick={() => setPreviewAction(action.id)}
                      >
                        {action.label}
                      </button>
                    );
                  })}
                </div>
              ) : (
                <span className="pet-action-hint">自定义形象保持原图展示</span>
              )}
            </div>
          </div>
        </section>
      )}

      <section className="pet-settings-section pet-gallery-section">
        <div className="pet-settings-section-heading">
          <div className="pet-gallery-heading-copy">
            <h3>水果伙伴陈列柜</h3>
            <p>四款橙子与四种水果，点击即刻切换上场。</p>
          </div>
          <button
            type="button"
            className="btn secondary pet-import"
            disabled={importing}
            onClick={() => inputRef.current?.click()}
          >
            <PlusIcon size={15} />
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
                <button type="button" className="pet-choice-main" onClick={() => selectPet(pet)}>
                  {selected && (
                    <span className="pet-choice-check" aria-hidden="true">
                      <CheckIcon size={12} weight="bold" />
                    </span>
                  )}
                  <span className="pet-choice-preview">
                    <img src={pet.src} alt="" draggable={false} />
                  </span>
                  <span className="pet-choice-copy">
                    <strong>{pet.name}</strong>
                    <small>{pet.description}</small>
                  </span>
                </button>
                {pet.custom && (
                  <button
                    type="button"
                    className="pet-choice-delete"
                    aria-label={`删除 ${pet.name}`}
                    title={`删除 ${pet.name}`}
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
    </div>
  );
}
