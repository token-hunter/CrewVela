import type { Dispatch, SetStateAction } from "react";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { Switch } from "./ui/Switch";
import { SettingsSection } from "./ui/SettingsSection";
import { ArrowCounterClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowCounterClockwise";
import type {
  AppAppearance,
  AppearanceAccent,
  AppearanceBackground,
  AppearanceTheme,
} from "../types";
import { DEFAULT_APPEARANCE } from "../appearance";

interface Props {
  appearance: AppAppearance;
  onChange: Dispatch<SetStateAction<AppAppearance>>;
}

const THEME_OPTIONS: Array<{ id: AppearanceTheme; label: string }> = [
  { id: "dark", label: "深色" },
  { id: "light", label: "浅色" },
  { id: "ocean", label: "深海" },
  { id: "purple", label: "暮紫" },
  { id: "sand", label: "暖沙" },
  { id: "aurora", label: "极光" },
];

const BACKGROUND_OPTIONS: Array<{
  id: AppearanceBackground;
  label: string;
}> = [
  { id: "solid", label: "纯色" },
  { id: "soft", label: "柔光" },
  { id: "texture", label: "微纹理" },
];

const ACCENT_OPTIONS: Array<{ id: AppearanceAccent; label: string }> = [
  { id: "clay", label: "陶土" },
  { id: "blue", label: "蓝色" },
  { id: "purple", label: "紫色" },
  { id: "cyan", label: "青色" },
  { id: "green", label: "绿色" },
  { id: "orange", label: "橙色" },
  { id: "pink", label: "粉色" },
  { id: "coral", label: "珊瑚色" },
  { id: "gray", label: "灰色" },
];

export function AppearanceSettingsPanel({ appearance, onChange }: Props) {
  const selectTheme = (theme: AppearanceTheme) => {
    onChange((current) => ({ ...current, theme, followSystem: false }));
  };

  return (
    <div className="appearance-settings-panel">
      <SettingsSection className="appearance-settings-section" title="主题">
        <div className="appearance-theme-grid">
          {THEME_OPTIONS.map((option) => {
            const selected =
              !appearance.followSystem && appearance.theme === option.id;
            return (
              <button
                className={`appearance-theme-option${selected ? " selected" : ""}`}
                type="button"
                key={option.id}
                aria-pressed={selected}
                onClick={() => selectTheme(option.id)}
              >
                <span
                  className="appearance-theme-preview"
                  data-preview-theme={option.id}
                  aria-hidden="true"
                >
                  <span className="appearance-preview-sidebar" />
                  <span className="appearance-preview-main">
                    <i />
                    <i />
                    <i />
                    <b />
                    <b />
                    <b />
                  </span>
                  {selected && (
                    <span className="appearance-option-check">
                      <CheckIcon size={13} weight="bold" />
                    </span>
                  )}
                </span>
                <strong>{option.label}</strong>
              </button>
            );
          })}
        </div>
      </SettingsSection>

      <SettingsSection className="appearance-settings-section" title="背景">
        <div className="appearance-background-grid">
          {BACKGROUND_OPTIONS.map((option) => {
            const selected = appearance.background === option.id;
            return (
              <button
                className={`appearance-background-option ${option.id}${selected ? " selected" : ""}`}
                type="button"
                key={option.id}
                aria-pressed={selected}
                onClick={() =>
                  onChange((current) => ({ ...current, background: option.id }))
                }
              >
                <span aria-hidden="true">
                  {selected && (
                    <i className="appearance-option-check">
                      <CheckIcon size={13} weight="bold" />
                    </i>
                  )}
                </span>
                <strong>{option.label}</strong>
              </button>
            );
          })}
        </div>
      </SettingsSection>

      <section className="appearance-settings-section appearance-terminal-section">
        <Switch
          className="appearance-terminal-toggle"
          checked={appearance.terminalTransparent === true}
          onChange={(checked) =>
            onChange((current) => ({
              ...current,
              terminalTransparent: checked,
            }))
          }
          childrenPosition="before"
        >
          <span>
            <strong>全局透明模式</strong>
            <small>让皮肤场景透过整个界面显示，文字、图标和边框保持清晰。</small>
          </span>
        </Switch>

        <div
          className={`appearance-terminal-opacity${
            appearance.terminalTransparent ? " enabled" : ""
          }`}
        >
          <div className="appearance-terminal-opacity-copy">
            <span>页面底板不透明度</span>
            <div className="appearance-terminal-opacity-actions">
              <output>
                {Math.round((appearance.terminalOpacity ?? 0.65) * 100)}%
                {(appearance.terminalOpacity ?? 0.65) === 0 ? " · 完全透明" : ""}
              </output>
              <button
                type="button"
                disabled={!appearance.terminalTransparent}
                onClick={() =>
                  onChange((current) => ({
                    ...current,
                    terminalTransparent: true,
                    terminalOpacity: 0,
                  }))
                }
              >
                完全透明
              </button>
            </div>
          </div>
          <input
            type="range"
            min="0"
            max="90"
            step="5"
            value={Math.round((appearance.terminalOpacity ?? 0.65) * 100)}
            disabled={!appearance.terminalTransparent}
            aria-label="页面底板不透明度"
            onChange={(event) =>
              onChange((current) => ({
                ...current,
                terminalOpacity: Number(event.target.value) / 100,
              }))
            }
          />
          <div className="appearance-terminal-preview" aria-hidden="true">
            <span>&gt; claude</span>
            <span>正在分析项目文件...</span>
            <i />
          </div>
        </div>
      </section>

      <SettingsSection className="appearance-settings-section appearance-accent-section" title="强调色">
        <div className="appearance-accent-list" role="radiogroup" aria-label="强调色">
          {ACCENT_OPTIONS.map((option) => (
            <button
              type="button"
              role="radio"
              key={option.id}
              className={appearance.accent === option.id ? "selected" : ""}
              data-accent={option.id}
              aria-label={option.label}
              aria-checked={appearance.accent === option.id}
              title={option.label}
              onClick={() =>
                onChange((current) => ({ ...current, accent: option.id }))
              }
            />
          ))}
        </div>
        <button
          className="appearance-reset"
          type="button"
          onClick={() => onChange(DEFAULT_APPEARANCE)}
        >
          <ArrowCounterClockwiseIcon size={14} />
          恢复默认
        </button>
      </SettingsSection>
    </div>
  );
}
