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
  RightSidebarStartStyle,
} from "../types";
import {
  DEFAULT_APPEARANCE,
  resolveRightSidebarStartStyle,
} from "../appearance";

interface Props {
  appearance: AppAppearance;
  onChange: Dispatch<SetStateAction<AppAppearance>>;
}

const THEME_OPTIONS: Array<{ id: AppearanceTheme; label: string; modeLabel: string }> = [
  { id: "dark", label: "深色", modeLabel: "Dark" },
  { id: "ocean", label: "暖夜黑", modeLabel: "Dark" },
  { id: "purple", label: "石墨黑", modeLabel: "Dark" },
  { id: "light", label: "浅色", modeLabel: "Light" },
  { id: "sand", label: "暖沙", modeLabel: "Light" },
  { id: "aurora", label: "暖瓷白", modeLabel: "Light" },
];

const BACKGROUND_OPTIONS: Array<{
  id: AppearanceBackground;
  label: string;
  hint: string;
}> = [
  { id: "solid", label: "纯色", hint: "纯净无干扰" },
  { id: "soft", label: "柔光", hint: "边缘环境微光" },
  { id: "texture", label: "微纹理", hint: "精密斜线网格" },
];

const ACCENT_OPTIONS: Array<{ id: AppearanceAccent; label: string }> = [
  { id: "none", label: "无强调色" },
  { id: "clay", label: "陶土" },
  { id: "purple", label: "紫色" },
  { id: "cyan", label: "青色" },
  { id: "orange", label: "橙色" },
  { id: "pink", label: "粉色" },
  { id: "coral", label: "珊瑚色" },
  { id: "gray", label: "灰色" },
];

const START_STYLE_OPTIONS: Array<{
  id: RightSidebarStartStyle;
  label: string;
  tag: string;
  hint: string;
}> = [
  { id: "minimal", label: "极简列表", tag: "默认", hint: "纯净单列入口，零装饰干扰" },
  { id: "lamp", label: "深夜拉绳台灯", tag: "默认关灯", hint: "下拉开灯照亮，关灯暗光探照" },
  { id: "clothesline", label: "微风晾衣架", tag: "可吹风", hint: "双层挂绳轻摆，顺手摘下开工" },
  { id: "cassette", label: "复古磁带架", tag: "可播放", hint: "横向抽拉卡带，装入双轴播放仓" },
];

function StartStylePreview({ style }: { style: RightSidebarStartStyle }) {
  if (style === "minimal") {
    return (
      <span className="appearance-start-preview minimal" aria-hidden="true">
        <span className="asp-minimal-list">
          <i />
          <i />
          <i />
          <i />
        </span>
      </span>
    );
  }
  if (style === "lamp") {
    return (
      <span className="appearance-start-preview lamp" aria-hidden="true">
        <i className="asp-lamp-wire" />
        <i className="asp-lamp-shade" />
        <i className="asp-lamp-beam" />
        <i className="asp-lamp-cord" />
        <span className="asp-lamp-lines">
          <i />
          <i />
          <i />
        </span>
      </span>
    );
  }
  if (style === "clothesline") {
    return (
      <span className="appearance-start-preview clothesline" aria-hidden="true">
        <span className="asp-line-row">
          <i className="asp-wire" />
          <span className="asp-tags">
            <i />
            <i className="accent" />
            <i />
          </span>
        </span>
        <span className="asp-line-row">
          <i className="asp-wire" />
          <span className="asp-tags short">
            <i />
            <i />
          </span>
        </span>
      </span>
    );
  }
  return (
    <span className="appearance-start-preview cassette" aria-hidden="true">
      <span className="asp-tapes">
        <i className="t1" />
        <i className="t2" />
        <i className="t3" />
      </span>
      <span className="asp-deck">
        <i />
        <i />
        <b />
      </span>
    </span>
  );
}

export function AppearanceSettingsPanel({ appearance, onChange }: Props) {
  const selectTheme = (theme: AppearanceTheme) => {
    onChange((current) => ({ ...current, theme, followSystem: false }));
  };
  const currentAccent = ACCENT_OPTIONS.find((item) => item.id === appearance.accent) ?? ACCENT_OPTIONS[1];
  const activeStartStyle = resolveRightSidebarStartStyle(appearance);

  return (
    <div className="appearance-settings-panel">
      <SettingsSection className="appearance-settings-section appearance-theme-section" title="主题" description="选择深色或浅色界面调色板，切换后立即应用至全局工作区。">
        <div className="appearance-theme-grid">
          {THEME_OPTIONS.map((option) => {
            const selected =
              !appearance.followSystem && appearance.theme === option.id;
            return (
              <button
                className={`appearance-theme-option${selected ? " selected" : ""}`}
                type="button"
                key={option.id}
                aria-label={option.label}
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
                  <em className="appearance-theme-mode-tag">{option.modeLabel}</em>
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

      <div className="appearance-dual-sections">
        <SettingsSection className="appearance-settings-section appearance-background-section" title="背景" description="调整主工作区画布的光学质感。">
          <div className="appearance-background-grid">
            {BACKGROUND_OPTIONS.map((option) => {
              const selected = appearance.background === option.id;
              return (
                <button
                  className={`appearance-background-option ${option.id}${selected ? " selected" : ""}`}
                  type="button"
                  key={option.id}
                  aria-label={option.label}
                  aria-pressed={selected}
                  onClick={() =>
                    onChange((current) => ({ ...current, background: option.id }))
                  }
                >
                  <span aria-hidden="true">
                    <small className="appearance-bg-hint">{option.hint}</small>
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

        <SettingsSection
          className="appearance-settings-section appearance-accent-section"
          title={<span className="appearance-accent-heading"><span>强调色</span><em className="appearance-accent-current" aria-hidden="true">{currentAccent.label}</em></span>}
          description="用于主按钮、焦点环、激活导航与状态高亮。"
        >
          <div className="appearance-accent-footer-row">
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
          </div>
        </SettingsSection>
      </div>

      <section className="appearance-settings-section">
        <Switch
          className="appearance-terminal-toggle"
          checked={appearance.communicationMotion === true}
          onChange={(checked) => onChange((current) => ({ ...current, communicationMotion: checked }))}
          childrenPosition="before"
        >
          <span>
            <strong>会话通信动效</strong>
            <small>两个 Agent 终端同屏通信时，用强调色连线显示消息方向。</small>
          </span>
        </Switch>
      </section>

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
            <small>调节界面各区域的背景透明度，文字、图标和边框保持清晰。</small>
          </span>
        </Switch>

        <div
          className={`appearance-terminal-opacity${
            appearance.terminalTransparent ? " enabled" : ""
          }`}
        >
          <div className="appearance-opacity-controls">
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
          </div>
          <div className="appearance-terminal-preview" aria-hidden="true">
            <span>&gt; claude</span>
            <span>正在分析项目文件...</span>
            <i />
          </div>
        </div>
      </section>

      <SettingsSection
        className="appearance-settings-section appearance-start-style-section"
        title="工作区开始页"
        description="自定义右侧栏「开始」页的展示模式与交互玩法，切换后即时生效。"
      >
        <div className="appearance-start-style-grid" role="group" aria-label="工作区开始页模式">
          {START_STYLE_OPTIONS.map((option) => {
            const selected = activeStartStyle === option.id;
            return (
              <button
                key={option.id}
                type="button"
                className={`appearance-start-style-option${selected ? " selected" : ""}`}
                aria-label={option.label}
                aria-pressed={selected}
                onClick={() =>
                  onChange((current) => ({
                    ...current,
                    rightSidebarStartStyle: option.id,
                  }))
                }
              >
                <span className="appearance-start-style-stage">
                  <StartStylePreview style={option.id} />
                  {selected && (
                    <span className="appearance-option-check">
                      <CheckIcon size={13} weight="bold" />
                    </span>
                  )}
                </span>
                <span className="appearance-start-style-meta">
                  <strong>
                    {option.label}
                    <em>{option.tag}</em>
                  </strong>
                  <small>{option.hint}</small>
                </span>
              </button>
            );
          })}
        </div>
      </SettingsSection>
    </div>
  );
}
