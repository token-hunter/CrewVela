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
  ArchivedSessionsSettingsStyle,
  AutomationSettingsStyle,
  BrowserControlSettingsStyle,
  GeneralSettingsStyle,
  GitSettingsStyle,
  NotificationSettingsStyle,
  PrivacySettingsStyle,
  ProjectManagementStyle,
  RightSidebarStartStyle,
  WorktreeSettingsStyle,
} from "../types";
import {
  DEFAULT_APPEARANCE,
  resolveArchivedSessionsSettingsStyle,
  resolveAutomationSettingsStyle,
  resolveBrowserControlSettingsStyle,
  resolveGeneralSettingsStyle,
  resolveGitSettingsStyle,
  resolveNotificationSettingsStyle,
  resolvePrivacySettingsStyle,
  resolveProjectManagementStyle,
  resolveRightSidebarStartStyle,
  resolveWorktreeSettingsStyle,
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

const PROJECT_STYLE_OPTIONS: Array<{
  id: ProjectManagementStyle;
  label: string;
  tag: string;
  hint: string;
}> = [
  { id: "classic", label: "经典列表", tag: "默认", hint: "经典手风琴卡片，信息完整直达" },
  { id: "gallery", label: "精选画廊", tag: "可缩放", hint: "纯净流光封面网格，点开毛玻璃详情" },
  { id: "vinyl", label: "黑胶唱片架", tag: "可旋转", hint: "启动服务滑出旋转，底部唱机操控" },
  { id: "pinboard", label: "相纸灵感墙", tag: "可对齐", hint: "图钉相纸软木墙，运行亮起状态灯" },
  { id: "studio", label: "亮灯工作室", tag: "光影联动", hint: "启动服务窗格亮灯，总览园区灯火" },
];

function ProjectStylePreview({ style }: { style: ProjectManagementStyle }) {
  if (style === "classic") {
    return (
      <span className="appearance-project-preview classic" aria-hidden="true">
        <span className="app-classic-row expanded">
          <i className="dot" />
          <span className="lines"><b /><small /></span>
          <em />
        </span>
        <span className="app-classic-row">
          <i className="dot" />
          <span className="lines"><b /></span>
        </span>
      </span>
    );
  }
  if (style === "gallery") {
    return (
      <span className="appearance-project-preview gallery" aria-hidden="true">
        <i className="g1" />
        <i className="g2" />
        <i className="g3" />
        <i className="g4" />
        <i className="g5" />
      </span>
    );
  }
  if (style === "vinyl") {
    return (
      <span className="appearance-project-preview vinyl" aria-hidden="true">
        <span className="vn-item active">
          <i className="disc" />
          <i className="sleeve s1" />
        </span>
        <span className="vn-item">
          <i className="disc" />
          <i className="sleeve s2" />
        </span>
      </span>
    );
  }
  if (style === "pinboard") {
    return (
      <span className="appearance-project-preview pinboard" aria-hidden="true">
        <span className="pb-card c1"><i className="pin" /><b /></span>
        <span className="pb-card c2"><i className="pin" /><b /></span>
        <span className="pb-card c3"><i className="pin" /><b /></span>
      </span>
    );
  }
  return (
    <span className="appearance-project-preview studio" aria-hidden="true">
      <span className="st-block lit">
        <i className="on" /><i className="on" /><i /><i className="on" />
        <i className="on" /><i /><i className="on" /><i className="on" />
      </span>
      <span className="st-block">
        <i /><i /><i /><i />
        <i /><i /><i /><i />
      </span>
    </span>
  );
}

function StartStylePillSwatch({ style }: { style: RightSidebarStartStyle }) {
  if (style === "minimal") {
    return (
      <span className="appearance-pm-swatch is-classic" aria-hidden="true">
        <i />
        <i className="short" />
      </span>
    );
  }
  if (style === "lamp") {
    return (
      <span className="appearance-pm-swatch is-start-lamp" aria-hidden="true">
        <i className="shade" />
        <i className="beam" />
      </span>
    );
  }
  if (style === "clothesline") {
    return (
      <span className="appearance-pm-swatch is-start-line" aria-hidden="true">
        <i className="wire" />
        <b />
        <b className="accent" />
      </span>
    );
  }
  return (
    <span className="appearance-pm-swatch is-start-tape" aria-hidden="true">
      <i className="reel" />
      <i className="reel" />
    </span>
  );
}

function ProjectStylePillSwatch({ style }: { style: ProjectManagementStyle }) {
  if (style === "classic") {
    return (
      <span className="appearance-pm-swatch is-classic" aria-hidden="true">
        <i />
        <i className="short" />
      </span>
    );
  }
  if (style === "gallery") {
    return <span className="appearance-pm-swatch is-gallery" aria-hidden="true" />;
  }
  if (style === "vinyl") {
    return (
      <span className="appearance-pm-swatch is-vinyl" aria-hidden="true">
        <i className="disc" />
        <i className="sleeve" />
      </span>
    );
  }
  if (style === "pinboard") {
    return (
      <span className="appearance-pm-swatch is-pinboard" aria-hidden="true">
        <i />
        <b />
      </span>
    );
  }
  return (
    <span className="appearance-pm-swatch is-studio" aria-hidden="true">
      <i className="on" />
      <i />
      <i />
      <i className="on" />
    </span>
  );
}

type ExtraStyleModuleId =
  | "general"
  | "git"
  | "worktrees"
  | "notifications"
  | "automation"
  | "browserControl"
  | "archivedSessions"
  | "privacy";

function ExtraStylePreview({ module, mode }: { module: ExtraStyleModuleId; mode: string }) {
  if (mode === "classic") {
    return (
      <span className="appearance-more-preview is-classic" aria-hidden="true">
        <span className="amp-classic-lines">
          <i className="w70" />
          <i className="w92" />
          <i className="w54" />
        </span>
      </span>
    );
  }
  if (module === "general") {
    return (
      <span className="appearance-more-preview is-coffee" aria-hidden="true">
        <svg viewBox="0 0 64 48" width="54" height="40">
          <path d="M23 14 C21 9, 26 7, 24 3" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" />
          <path d="M31 13 C29 8, 34 6, 32 2" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" />
          <rect x="15" y="18" width="28" height="21" rx="6" fill="var(--bg-elev)" stroke="var(--accent)" strokeWidth="1.6" />
          <path d="M43 22 C50 22, 50 34, 43 34" fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" />
          <ellipse cx="29" cy="42" rx="19" ry="3" fill="var(--border-strong)" />
        </svg>
      </span>
    );
  }
  if (module === "git") {
    return (
      <span className="appearance-more-preview is-railway" aria-hidden="true">
        <svg viewBox="0 0 88 44" width="74" height="38">
          <path d="M6 16 L82 16" stroke="var(--border-strong)" strokeWidth="2" fill="none" />
          <path d="M24 16 C36 16, 38 32, 48 32 L60 32 C68 32, 70 16, 78 16" stroke="var(--accent)" strokeWidth="2.2" fill="none" />
          <circle cx="16" cy="16" r="3.5" fill="var(--text-dim)" />
          <circle cx="48" cy="32" r="3.5" fill="var(--accent)" />
          <circle cx="76" cy="16" r="4" fill="var(--success)" />
        </svg>
      </span>
    );
  }
  if (module === "worktrees") {
    return (
      <span className="appearance-more-preview is-greenhouse" aria-hidden="true">
        <svg viewBox="0 0 64 48" width="52" height="40">
          <path d="M20 33 L44 33 L40 43 L24 43 Z" fill="var(--accent)" opacity="0.86" />
          <rect x="18" y="30" width="28" height="4" rx="1.8" fill="var(--accent)" />
          <path d="M32 30 L32 16" stroke="var(--text-dim)" strokeWidth="2" strokeLinecap="round" />
          <path d="M32 23 C24 21, 21 14, 26 11 C30 12, 32 17, 32 23 Z" fill="var(--success)" />
          <path d="M32 20 C40 18, 43 11, 38 8 C34 9, 32 14, 32 20 Z" fill="var(--success)" opacity="0.78" />
        </svg>
      </span>
    );
  }
  if (module === "notifications") {
    return (
      <span className="appearance-more-preview is-mixer" aria-hidden="true">
        <span className="amp-eq">
          <i style={{ height: "45%" }} />
          <i style={{ height: "85%" }} />
          <i style={{ height: "62%" }} />
          <i style={{ height: "96%" }} className="accent" />
          <i style={{ height: "52%" }} />
          <i style={{ height: "74%" }} />
        </span>
      </span>
    );
  }
  if (module === "automation") {
    return (
      <span className="appearance-more-preview is-sundial" aria-hidden="true">
        <svg viewBox="0 0 52 52" width="44" height="44">
          <circle cx="26" cy="26" r="19" fill="none" stroke="var(--border-strong)" strokeWidth="1.5" />
          <circle cx="26" cy="26" r="11" fill="none" stroke="var(--border)" strokeDasharray="2 3" />
          <line x1="26" y1="26" x2="36" y2="18" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" />
          <circle cx="26" cy="7" r="3" fill="var(--accent)" />
          <circle cx="43" cy="31" r="2.8" fill="var(--success)" />
          <circle cx="15" cy="19" r="2.6" fill="#6aa6f8" />
        </svg>
      </span>
    );
  }
  if (module === "browserControl") {
    return (
      <span className="appearance-more-preview is-patchbay" aria-hidden="true">
        <svg viewBox="0 0 80 46" width="68" height="39">
          <circle cx="16" cy="23" r="5" fill="var(--bg-elev)" stroke="var(--accent)" strokeWidth="1.8" />
          <circle cx="40" cy="23" r="3.8" fill="var(--bg-elev)" stroke="var(--warn, #e5a94c)" strokeWidth="1.5" />
          <circle cx="64" cy="14" r="4.6" fill="var(--bg-elev)" stroke="var(--accent)" strokeWidth="1.8" />
          <circle cx="64" cy="33" r="4.4" fill="var(--bg-elev)" stroke="var(--border-strong)" strokeWidth="1.5" />
          <path d="M40 23 C50 23, 52 33, 60 33" fill="none" stroke="var(--border-strong)" strokeWidth="1.3" strokeDasharray="2.5 2.5" />
          <path d="M16 23 C28 23, 30 23, 40 23 C50 23, 52 14, 64 14" fill="none" stroke="var(--accent)" strokeWidth="2.4" strokeLinecap="round" />
        </svg>
      </span>
    );
  }
  if (module === "archivedSessions") {
    return (
      <span className="appearance-more-preview is-tea" aria-hidden="true">
        <svg viewBox="0 0 64 48" width="54" height="40">
          <path d="M37 15 C39 11, 36 8, 39 4" fill="none" stroke="var(--accent)" strokeWidth="1.6" strokeLinecap="round" />
          <path d="M43 16 C45 12, 42 9, 45 6" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" opacity="0.82" />
          <rect x="20" y="18" width="24" height="24" rx="5" fill="var(--bg-elev)" stroke="var(--accent)" strokeWidth="1.5" />
          <rect x="28" y="21" width="8" height="17" rx="2" fill="var(--warn, #e5a94c)" opacity="0.72" />
          <circle cx="32" cy="34" r="1.8" fill="var(--accent)" />
          <g transform="translate(-4, -3) rotate(-11 32 16)">
            <rect x="22" y="13" width="20" height="5" rx="2.4" fill="var(--warn, #e5a94c)" opacity="0.85" />
            <circle cx="32" cy="11.5" r="2.2" fill="var(--accent)" />
          </g>
        </svg>
      </span>
    );
  }
  return (
    <span className="appearance-more-preview is-vault" aria-hidden="true">
      <svg viewBox="0 0 52 52" width="44" height="44">
        <circle cx="26" cy="26" r="19" fill="var(--bg-elev)" stroke="var(--border-strong)" strokeWidth="1.6" />
        <circle cx="26" cy="26" r="11" fill="none" stroke="var(--accent)" strokeWidth="1.8" />
        <line x1="26" y1="15" x2="26" y2="21" stroke="var(--accent)" strokeWidth="2.2" strokeLinecap="round" />
        <circle cx="26" cy="26" r="3" fill="var(--accent)" />
      </svg>
    </span>
  );
}

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
  const activeProjectStyle = resolveProjectManagementStyle(appearance);
  const activeGeneralStyle = resolveGeneralSettingsStyle(appearance);
  const activeGitStyle = resolveGitSettingsStyle(appearance);
  const activeWorktreeStyle = resolveWorktreeSettingsStyle(appearance);
  const activeNotificationStyle = resolveNotificationSettingsStyle(appearance);
  const activeAutomationStyle = resolveAutomationSettingsStyle(appearance);
  const activeBrowserControlStyle = resolveBrowserControlSettingsStyle(appearance);
  const activeArchivedSessionsStyle = resolveArchivedSessionsSettingsStyle(appearance);
  const activePrivacyStyle = resolvePrivacySettingsStyle(appearance);

  const currentStartStyleOption =
    START_STYLE_OPTIONS.find((item) => item.id === activeStartStyle) ?? START_STYLE_OPTIONS[0];
  const isStartStyleCreative = activeStartStyle !== "minimal";

  const currentProjectStyleOption =
    PROJECT_STYLE_OPTIONS.find((item) => item.id === activeProjectStyle) ??
    PROJECT_STYLE_OPTIONS[0];
  const isProjectStyleCreative = activeProjectStyle !== "classic";

  const extraCreativeCount = [
    isStartStyleCreative,
    isProjectStyleCreative,
    activeGeneralStyle !== "classic",
    activeGitStyle !== "classic",
    activeWorktreeStyle !== "classic",
    activeNotificationStyle !== "classic",
    activeAutomationStyle !== "classic",
    activeBrowserControlStyle !== "classic",
    activeArchivedSessionsStyle !== "classic",
    activePrivacyStyle !== "classic",
  ].filter(Boolean).length;
  const totalStyleModules = 10;

  const extraStyleModules: Array<{
    id: ExtraStyleModuleId;
    title: string;
    activeMode: string;
    options: Array<{ id: string; label: string; hint: string }>;
    onSelect: (mode: string) => void;
  }> = [
    {
      id: "general",
      title: "常规",
      activeMode: activeGeneralStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有分组表单布局" },
        { id: "coffee", label: "防休眠咖啡桌", hint: "咖啡杯防休眠开关与活字试字台" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({ ...current, generalSettingsStyle: mode as GeneralSettingsStyle })),
    },
    {
      id: "git",
      title: "Git",
      activeMode: activeGitStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有极简全宽配置布局" },
        { id: "railway", label: "铁道道岔", hint: "动态道岔图解 Pull 策略与分支站牌" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({ ...current, gitSettingsStyle: mode as GitSettingsStyle })),
    },
    {
      id: "worktrees",
      title: "Worktrees",
      activeMode: activeWorktreeStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有极简细线流工作树清单" },
        { id: "greenhouse", label: "温室苗圃", hint: "按状态生长的盆栽树苗与种子快照库" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({ ...current, worktreeSettingsStyle: mode as WorktreeSettingsStyle })),
    },
    {
      id: "notifications",
      title: "通知",
      activeMode: activeNotificationStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有分标签通知开关与历史列表" },
        { id: "mixer", label: "迷你调音台", hint: "实时声波柱、音量推子与通道指示灯" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({
          ...current,
          notificationSettingsStyle: mode as NotificationSettingsStyle,
        })),
    },
    {
      id: "automation",
      title: "自动化",
      activeMode: activeAutomationStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有自动化任务卡片与执行记录" },
        { id: "sundial", label: "24 小时星盘", hint: "时刻刻度星点环与上发条快捷试跑" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({
          ...current,
          automationSettingsStyle: mode as AutomationSettingsStyle,
        })),
    },
    {
      id: "browserControl",
      title: "浏览器控制",
      activeMode: activeBrowserControlStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有单栏表单与运行状态布局" },
        { id: "patchbay", label: "网页跳线总机", hint: "实体弯曲跳线直连内置面板与外部浏览器渠道" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({
          ...current,
          browserControlSettingsStyle: mode as BrowserControlSettingsStyle,
        })),
    },
    {
      id: "archivedSessions",
      title: "归档会话",
      activeMode: activeArchivedSessionsStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有分组归档列表与快捷操作栏" },
        { id: "tea", label: "岁茶藏罐架", hint: "项目木架密封茶罐与顶部开盖查香恢复台" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({
          ...current,
          archivedSessionsSettingsStyle: mode as ArchivedSessionsSettingsStyle,
        })),
    },
    {
      id: "privacy",
      title: "隐私",
      activeMode: activePrivacyStyle,
      options: [
        { id: "classic", label: "经典", hint: "现有三大基石与数据入口列表" },
        { id: "vault", label: "本机保险箱", hint: "可转动锁芯密码盘与钥匙数据抽屉" },
      ],
      onSelect: (mode) =>
        onChange((current) => ({ ...current, privacySettingsStyle: mode as PrivacySettingsStyle })),
    },
  ];

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
        className="appearance-settings-section appearance-more-styles-section"
        title="更多页面风格"
        description="为工作区开始页、项目管理、常规、Git、Worktrees、通知、自动化、浏览器控制、归档会话与隐私开启可选创意模式，默认均保持现有设计。"
      >
        <div className="appearance-more-styles-bar">
          <span className="appearance-more-styles-summary">
            已开启创意模式：<strong>{extraCreativeCount} / {totalStyleModules}</strong>
          </span>
          <div className="appearance-more-styles-actions">
            <button
              type="button"
              className="btn secondary"
              disabled={extraCreativeCount === totalStyleModules}
              onClick={() =>
                onChange((current) => ({
                  ...current,
                  rightSidebarStartStyle:
                    resolveRightSidebarStartStyle(current) === "minimal"
                      ? "lamp"
                      : resolveRightSidebarStartStyle(current),
                  projectManagementStyle:
                    resolveProjectManagementStyle(current) === "classic"
                      ? "vinyl"
                      : resolveProjectManagementStyle(current),
                  generalSettingsStyle: "coffee",
                  gitSettingsStyle: "railway",
                  worktreeSettingsStyle: "greenhouse",
                  notificationSettingsStyle: "mixer",
                  automationSettingsStyle: "sundial",
                  browserControlSettingsStyle: "patchbay",
                  archivedSessionsSettingsStyle: "tea",
                  privacySettingsStyle: "vault",
                }))
              }
            >
              一键全部创意
            </button>
            <button
              type="button"
              className="btn secondary"
              disabled={extraCreativeCount === 0}
              onClick={() =>
                onChange((current) => ({
                  ...current,
                  rightSidebarStartStyle: "minimal",
                  projectManagementStyle: "classic",
                  generalSettingsStyle: "classic",
                  gitSettingsStyle: "classic",
                  worktreeSettingsStyle: "classic",
                  notificationSettingsStyle: "classic",
                  automationSettingsStyle: "classic",
                  browserControlSettingsStyle: "classic",
                  archivedSessionsSettingsStyle: "classic",
                  privacySettingsStyle: "classic",
                }))
              }
            >
              全部恢复默认
            </button>
          </div>
        </div>

        <div className="appearance-more-styles-grid">
          <article
            className={`appearance-more-style-card is-featured${isStartStyleCreative ? " is-creative" : ""}`}
          >
            <div className="appearance-more-style-top">
              <span className="appearance-more-preview is-featured-pm" aria-hidden="true">
                <StartStylePreview style={activeStartStyle} />
              </span>
              <div className="appearance-more-style-copy">
                <strong>
                  工作区开始页
                  {isStartStyleCreative && <em>创意中</em>}
                </strong>
                <small>{currentStartStyleOption.hint}</small>
              </div>
              <div className="appearance-more-style-top-end">
                <span className="appearance-more-mode-tag">
                  {currentStartStyleOption.label} · {currentStartStyleOption.tag}
                </span>
              </div>
            </div>
            <div className="appearance-pm-pills is-four-cols" role="group" aria-label="工作区开始页模式">
              {START_STYLE_OPTIONS.map((option) => {
                const selected = activeStartStyle === option.id;
                return (
                  <button
                    key={option.id}
                    type="button"
                    className={`appearance-pm-pill-btn${selected ? " selected" : ""}`}
                    aria-pressed={selected}
                    onClick={() =>
                      onChange((current) => ({
                        ...current,
                        rightSidebarStartStyle: option.id,
                      }))
                    }
                  >
                    <StartStylePillSwatch style={option.id} />
                    <span>{option.label}</span>
                  </button>
                );
              })}
            </div>
          </article>

          <article
            className={`appearance-more-style-card is-featured${isProjectStyleCreative ? " is-creative" : ""}`}
          >
            <div className="appearance-more-style-top">
              <span className="appearance-more-preview is-featured-pm" aria-hidden="true">
                <ProjectStylePreview style={activeProjectStyle} />
              </span>
              <div className="appearance-more-style-copy">
                <strong>
                  项目管理
                  {isProjectStyleCreative && <em>创意中</em>}
                </strong>
                <small>{currentProjectStyleOption.hint}</small>
              </div>
              <div className="appearance-more-style-top-end">
                <span className="appearance-more-mode-tag">
                  {currentProjectStyleOption.label} · {currentProjectStyleOption.tag}
                </span>
              </div>
            </div>
            <div className="appearance-pm-pills" role="group" aria-label="项目管理模式">
              {PROJECT_STYLE_OPTIONS.map((option) => {
                const selected = activeProjectStyle === option.id;
                return (
                  <button
                    key={option.id}
                    type="button"
                    className={`appearance-pm-pill-btn${selected ? " selected" : ""}`}
                    aria-pressed={selected}
                    onClick={() =>
                      onChange((current) => ({
                        ...current,
                        projectManagementStyle: option.id,
                      }))
                    }
                  >
                    <ProjectStylePillSwatch style={option.id} />
                    <span>{option.label}</span>
                  </button>
                );
              })}
            </div>
          </article>

          {extraStyleModules.map((mod) => {
            const currentOpt = mod.options.find((o) => o.id === mod.activeMode) ?? mod.options[0];
            const isCreative = mod.activeMode !== "classic";
            return (
              <article
                key={mod.id}
                className={`appearance-more-style-card${isCreative ? " is-creative" : ""}`}
              >
                <div className="appearance-more-style-top">
                  <ExtraStylePreview module={mod.id} mode={mod.activeMode} />
                  <div className="appearance-more-style-copy">
                    <strong>
                      {mod.title}
                      {isCreative && <em>创意中</em>}
                    </strong>
                    <small>{currentOpt.hint}</small>
                  </div>
                </div>
                <div
                  className="appearance-more-style-pills"
                  role="group"
                  aria-label={`${mod.title}页面风格`}
                >
                  {mod.options.map((opt) => {
                    const selected = mod.activeMode === opt.id;
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        className={selected ? "selected" : ""}
                        aria-pressed={selected}
                        onClick={() => mod.onSelect(opt.id)}
                      >
                        {opt.label}
                      </button>
                    );
                  })}
                </div>
              </article>
            );
          })}
        </div>
      </SettingsSection>
    </div>
  );
}
