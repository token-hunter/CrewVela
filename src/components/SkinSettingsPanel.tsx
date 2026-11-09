import { useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import type { AppAppearance } from "../types";
import type { CompanionSettings } from "../companionSettings";
import { Switch } from "./ui/Switch";
import { DEFAULT_APPEARANCE } from "../appearance";
import {
  loadCustomSkins,
  parseCustomSkin,
  saveCustomSkins,
  type CustomSkin,
} from "../customSkins";

interface Props {
  settings: CompanionSettings;
  appearance: AppAppearance;
  onChange: (settings: CompanionSettings) => void;
  onAppearanceChange: (appearance: AppAppearance) => void;
}

function customPreview(appearance: AppAppearance) {
  const backgrounds: Record<AppAppearance["theme"], string> = {
    dark: "linear-gradient(145deg,#282828,#141414)",
    light: "linear-gradient(145deg,#fff,#e7e9ed)",
    ocean: "linear-gradient(145deg,#173345,#0d161d)",
    purple: "linear-gradient(145deg,#332845,#17131e)",
    sand: "linear-gradient(145deg,#fffaf2,#eadcc7)",
    aurora: "linear-gradient(145deg,#173b35,#101923)",
  };
  return { background: backgrounds[appearance.theme] };
}

interface ImportedDynamicSkin {
  id: string;
  name: string;
  description: string;
  videoPath: string;
  previewPath: string | null;
  audioPath: string | null;
  videoHasAudio: boolean;
  importedAt: number;
  sourceFormat: string;
}

export function SkinSettingsPanel({ settings, appearance, onChange, onAppearanceChange }: Props) {
  const [customSkins, setCustomSkins] = useState<CustomSkin[]>(loadCustomSkins);
  const [message, setMessage] = useState<{ kind: "error" | "success"; text: string } | null>(null);
  const [importing, setImporting] = useState(false);
  const [mutating, setMutating] = useState(false);
  const mutationInFlight = useRef(false);
  const latest = useRef({ settings, appearance });
  latest.current = { settings, appearance };
  const [importProgress, setImportProgress] = useState<{ current: number; total: number } | null>(null);
  const activeCustomSkin = customSkins.find((skin) => skin.id === settings.customSkinId);
  const hasAtmosphere = Boolean(activeCustomSkin?.media);
  const hasSound = Boolean(
    activeCustomSkin?.media?.audioPath
      || activeCustomSkin?.media?.videoHasAudio,
  );

  const applySkin = (skin: CustomSkin) => {
    const { settings, appearance } = latest.current;
    const skinHasSound = Boolean(skin.media?.audioPath || skin.media?.videoHasAudio);
    onChange({
      ...settings,
      customSkinId: skin.id,
      skinSoundEnabled: skinHasSound ? true : settings.skinSoundEnabled,
      appearanceBeforeSkin: settings.customSkinId
        ? settings.appearanceBeforeSkin
        : appearance,
    });
    onAppearanceChange({
      ...skin.appearance,
      terminalTransparent: appearance.terminalTransparent,
      terminalOpacity: appearance.terminalOpacity,
    });
    setMessage({ kind: "success", text: `已应用“${skin.name}”` });
  };

  const clearSkin = () => {
    const { settings, appearance } = latest.current;
    if (!settings.customSkinId) return;
    onAppearanceChange({
      ...(settings.appearanceBeforeSkin ?? DEFAULT_APPEARANCE),
      terminalTransparent: appearance.terminalTransparent,
      terminalOpacity: appearance.terminalOpacity,
    });
    onChange({
      ...settings,
      customSkinId: null,
      appearanceBeforeSkin: null,
    });
    setMessage({ kind: "success", text: "已取消皮肤，恢复原有外观" });
  };

  const importSkinFile = async (selected: string): Promise<CustomSkin> => {
    const extension = selected.split(".").pop()?.toLowerCase();
    if (extension === "mpkg") {
      const imported = await invoke<ImportedDynamicSkin>("import_dynamic_skin", { sourcePath: selected });
      return {
        id: imported.id,
        name: imported.name,
        description: imported.description,
        appearance: {
          theme: "dark",
          background: "texture",
          accent: "orange",
          followSystem: false,
        },
        importedAt: imported.importedAt,
        media: {
          kind: "video",
          videoPath: imported.videoPath,
          previewPath: imported.previewPath,
          audioPath: imported.audioPath,
          videoHasAudio: imported.videoHasAudio,
          sourceFormat: imported.sourceFormat,
        },
      };
    }
    if (extension === "json" || extension === "ccskin") {
      const raw = await invoke<string>("read_skin_config_file", { sourcePath: selected });
      const fallbackName = selected.split(/[\\/]/).pop()?.replace(/\.(json|ccskin)$/i, "") ?? "自定义皮肤";
      return parseCustomSkin(raw, fallbackName);
    }
    throw new Error("请选择 .mpkg、.json 或 .ccskin 文件");
  };

  const importSkins = async () => {
    const selected = await open({
      multiple: true,
      title: "导入 CrewVela 皮肤",
      filters: [{ name: "CrewVela 皮肤", extensions: ["mpkg", "json", "ccskin"] }],
    });
    const paths = typeof selected === "string" ? [selected] : selected ?? [];
    if (paths.length === 0) return;
    if (paths.length > 20) throw new Error("单次最多导入 20 个皮肤包");

    const importedSkins: CustomSkin[] = [];
    const failed: string[] = [];
    setImportProgress({ current: 0, total: paths.length });
    for (let index = 0; index < paths.length; index += 1) {
      setImportProgress({ current: index + 1, total: paths.length });
      try {
        importedSkins.push(await importSkinFile(paths[index]));
      } catch (error) {
        const filename = paths[index].split(/[\\/]/).pop() ?? paths[index];
        failed.push(`${filename}：${error instanceof Error ? error.message : String(error)}`);
      }
    }

    if (importedSkins.length === 0) {
      throw new Error(failed.join("；") || "没有成功导入任何皮肤");
    }
    const next = [...importedSkins, ...customSkins];
    saveCustomSkins(next);
    setCustomSkins(next);
    applySkin(importedSkins[0]);
    setMessage({
      kind: failed.length > 0 ? "error" : "success",
      text: failed.length > 0
        ? `成功导入 ${importedSkins.length} 个，失败 ${failed.length} 个：${failed.join("；")}`
        : `已成功导入 ${importedSkins.length} 个皮肤，并应用第一个`,
    });
  };

  const removeCustom = async (skin: CustomSkin) => {
    const wasActive = settings.customSkinId === skin.id;
    if (wasActive) {
      clearSkin();
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
    }
    if (skin.media) await invoke("delete_dynamic_skin", { id: skin.id });
    const next = customSkins.filter((item) => item.id !== skin.id);
    saveCustomSkins(next);
    setCustomSkins(next);
    if (!wasActive) {
      setMessage(null);
    }
  };

  const attachBgm = async (skin: CustomSkin) => {
    if (!skin.media) throw new Error("这个皮肤不支持背景音乐");
    const selected = await open({
      multiple: false,
      title: `为“${skin.name}”选择背景音乐`,
      filters: [{ name: "音频文件", extensions: ["mp3", "ogg", "wav", "m4a", "aac"] }],
    });
    if (typeof selected !== "string") return;
    const audioPath = await invoke<string>("attach_dynamic_skin_audio", {
      id: skin.id,
      sourcePath: selected,
    });
    const next = customSkins.map((item) => item.id === skin.id && item.media
      ? { ...item, media: { ...item.media, audioPath } }
      : item);
    saveCustomSkins(next);
    setCustomSkins(next);
    if (latest.current.settings.customSkinId === skin.id) {
      onChange({ ...latest.current.settings, skinSoundEnabled: true });
    }
    setMessage({ kind: "success", text: `已为“${skin.name}”添加背景音乐` });
  };

  const mutate = async (action: () => Promise<void>, importingSkin = false) => {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true;
    setMutating(true); setImporting(importingSkin); setMessage(null);
    try { await action(); }
    catch (error) { setMessage({ kind: "error", text: error instanceof Error ? error.message : String(error) }); }
    finally { mutationInFlight.current = false; setMutating(false); setImporting(false); setImportProgress(null); }
  };

  return (
    <div className="skin-settings-panel">
      <div className="skin-library-toolbar">
        <div>
          <strong>我的皮肤</strong>
          <span>{customSkins.length} 个</span>
        </div>
        <div className="skin-library-actions">
          <button
            type="button"
            className="skin-clear-button"
            disabled={mutating || !settings.customSkinId}
            onClick={clearSkin}
          >
            取消皮肤
          </button>
          <button
            type="button"
            disabled={mutating}
            onClick={() => void mutate(importSkins, true)}
          >
            {importing
              ? importProgress
                ? `正在导入 ${importProgress.current}/${importProgress.total}…`
                : "正在选择…"
              : "导入皮肤"}
          </button>
        </div>
      </div>

      {message && <div className={`skin-message ${message.kind}`} role="status">{message.text}</div>}

      <section className={`skin-atmosphere-controls${hasAtmosphere ? " available" : ""}`}>
        <div>
          <strong>动态场景</strong>
          <small>{activeCustomSkin?.media ? "循环播放导入的本地高清视频。" : hasAtmosphere ? "为当前皮肤播放局部环境动效。" : "当前皮肤没有动态场景。"}</small>
        </div>
        <Switch
          className="settings-switch"
          disabled={!hasAtmosphere}
          checked={hasAtmosphere && settings.skinMotionEnabled}
          onChange={(checked) => onChange({ ...settings, skinMotionEnabled: checked })}
        />
        <div>
          <strong>环境音</strong>
          <small>{activeCustomSkin?.media && !hasSound ? "这个资源包没有内嵌音轨。" : "本地播放，可随时关闭；默认不播放。"}</small>
        </div>
        <Switch
          className="settings-switch"
          disabled={!hasSound}
          checked={hasSound && settings.skinSoundEnabled}
          onChange={(checked) => onChange({ ...settings, skinSoundEnabled: checked })}
        />
        <label className="skin-atmosphere-volume">
          <span>音量</span>
          <input
            type="range"
            min="0"
            max="100"
            disabled={!hasSound || !settings.skinSoundEnabled}
            value={settings.skinSoundVolume}
            onChange={(event) => onChange({ ...settings, skinSoundVolume: Number(event.target.value) })}
          />
          <output>{settings.skinSoundVolume}%</output>
        </label>
      </section>

      <div className="skin-gallery">
        {customSkins.map((skin) => {
          const selected = settings.customSkinId === skin.id;
          const previewStyle = skin.media?.previewPath
            ? { backgroundImage: `url("${convertFileSrc(skin.media.previewPath)}")` }
            : customPreview(skin.appearance);
          return (
            <article key={skin.id} className={`skin-preset-card custom${selected ? " selected" : ""}`}>
              <button type="button" className="skin-custom-apply" disabled={mutating} onClick={() => applySkin(skin)}>
                <span className="skin-preset-preview" style={previewStyle}><i /><b /><em /></span>
                <span className="skin-preset-copy">
                  <strong>{skin.name}</strong>
                  <small>{skin.description}</small>
                  {skin.media && (
                    <small className={`skin-audio-status${skin.media.audioPath || skin.media.videoHasAudio ? " available" : ""}`}>
                      {skin.media.audioPath || skin.media.videoHasAudio ? "含背景音乐" : "未包含背景音乐"}
                    </small>
                  )}
                </span>
              </button>
              {selected && <i className="skin-selected"><CheckIcon size={13} weight="bold" /></i>}
              <div className="skin-card-actions">
                {skin.media && (
                  <button type="button" disabled={mutating} onClick={() => void mutate(() => attachBgm(skin))}>
                    {skin.media.audioPath ? "更换 BGM" : "添加 BGM"}
                  </button>
                )}
                <button
                  type="button"
                  className="skin-custom-delete"
                  disabled={mutating}
                  onClick={() => {
                    if (!window.confirm(`确定删除皮肤“${skin.name}”吗？`)) return;
                    void mutate(() => removeCustom(skin));
                  }}
                >
                  <TrashIcon size={14} />删除皮肤
                </button>
              </div>
            </article>
          );
        })}
      </div>


    </div>
  );
}
