import { useEffect, useRef, useState } from "react";
import { Switch } from "./ui/Switch";
import { Modal } from "./ui/Modal";
import type { Dispatch, SetStateAction } from "react";
import { BellIcon } from "@phosphor-icons/react/dist/csr/Bell";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { HeartIcon } from "@phosphor-icons/react/dist/csr/Heart";
import { BrainIcon } from "@phosphor-icons/react/dist/csr/Brain";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { PaletteIcon } from "@phosphor-icons/react/dist/csr/Palette";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { UsersIcon } from "@phosphor-icons/react/dist/csr/Users";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { ScrollIcon } from "@phosphor-icons/react/dist/csr/Scroll";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  AppAppearance,
  AppearanceTheme,
  Project,
  ProviderConfig,
  SessionRecord,
} from "../types";
import type { PetSettings } from "../petSettings";
import type { NotificationSettings } from "../notificationSettings";
import { CollaborationSettingsPanel } from "./CollaborationSettingsPanel";
import { SharedMemorySettingsPanel } from "./SharedMemorySettingsPanel";
import { AppearanceSettingsPanel } from "./AppearanceSettingsPanel";
import { resolveBaseTheme } from "../appearance";
import { QuickCommandCenter } from "./QuickCommandCenter";
import type { QuickContext } from "../quickCommands";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import { AutomationPanel } from "./AutomationPanel";
import { PetSettingsPanel } from "./PetSettingsPanel";
import { NotificationSettingsPanel } from "./NotificationSettingsPanel";
import { AutomationIcon } from "./AutomationIcon";
import { ArchivedSessionsPanel } from "./ArchivedSessionsPanel";
import { GitSettingsPanel } from "./GitSettingsPanel";
import { SkillCenterPanel } from "./SkillCenterPanel";
import { BrowserControlSettingsPanel } from "./BrowserControlSettingsPanel";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import { LockIcon } from "@phosphor-icons/react/dist/csr/Lock";
import { PrivacySettingsPanel } from "./PrivacySettingsPanel";
import type { CollaborationTasksRequest } from "../collaborationTasks";

interface Props {
  quickContext?: QuickContext;
  onSessionsChange?: (sessions: SessionRecord[]) => void;
  sessions: SessionRecord[];
  archivedSessions: SessionRecord[];
  projects: Project[];
  appearance: AppAppearance;
  resolvedAppearanceTheme: AppearanceTheme;
  petSettings: PetSettings;
  notificationSettings: NotificationSettings;
  onAppearanceChange: Dispatch<SetStateAction<AppAppearance>>;
  onPetSettingsChange: (settings: PetSettings) => void;
  onNotificationSettingsChange: (settings: NotificationSettings) => void;
  onRestoreArchivedSession: (session: SessionRecord) => void | Promise<void>;
  onDeleteArchivedSession: (session: SessionRecord) => void | Promise<void>;
  onViewArchivedTranscript: (session: SessionRecord) => void;
  onClose: () => void;
  initialSection?: SettingsSection;
  initialCollaborationTab?: "overview" | "diagnostics";
  initialCollaborationDiagnosticGlobal?: boolean;
  initialCollaborationProjectId?: string;
  initialCollaborationTasksRequest?: CollaborationTasksRequest;
}

export type SettingsSection =
  | "generalSettings"
  | "collaboration"
  | "sharedMemory"
  | "automation"
  | "quickCommands"
  | "git"
  | "browserControl"
  | "skills"
  | "appearance"
  | "notifications"
  | "companionship"
  | "archivedSessions"
  | "privacy";

export function SettingsDialog({
  quickContext,
  onSessionsChange,
  sessions,
  archivedSessions,
  projects,
  appearance,
  resolvedAppearanceTheme,
  petSettings,
  notificationSettings,
  onAppearanceChange,
  onPetSettingsChange,
  onNotificationSettingsChange,
  onRestoreArchivedSession,
  onDeleteArchivedSession,
  onViewArchivedTranscript,
  onClose,
  initialSection = "generalSettings",
  initialCollaborationTab = "overview",
  initialCollaborationDiagnosticGlobal = false,
  initialCollaborationProjectId,
  initialCollaborationTasksRequest,
}: Props) {
  const [activeSection, setActiveSection] = useState<SettingsSection>(
    String(initialSection) === "mcp" ? "generalSettings" : initialSection,
  );
  const [automationEditing, setAutomationEditing] = useState<"create" | "edit" | null>(null);
  const [navQuery, setNavQuery] = useState("");
  const [privacyCollaborationRequested, setPrivacyCollaborationRequested] = useState(false);
  const [taskRequest, setTaskRequest] = useState(initialCollaborationTasksRequest);
  useEffect(() => {
    setTaskRequest(initialCollaborationTasksRequest);
    if (initialCollaborationTasksRequest) { setActiveSection("collaboration"); setNavQuery(""); setPrivacyCollaborationRequested(false); }
  }, [initialCollaborationTasksRequest?.projectId, initialCollaborationTasksRequest?.requestId]);
  const settingsNav = useRef<HTMLElement>(null);
  const privacyNavigationTarget = useRef<SettingsSection | null>(null);
  useEffect(() => {
    if (privacyNavigationTarget.current !== activeSection) return;
    privacyNavigationTarget.current = null;
    settingsNav.current?.querySelector<HTMLButtonElement>('.settings-nav-item[aria-current="page"]')?.focus();
  }, [activeSection]);
  const settingsNavGroups = [
    {
      label: "通用",
      items: [
        {
          id: "generalSettings" as const,
          label: "常规",
          keywords: "通用 常规 general 代理 proxy 网络 network 托盘 tray 休眠 keep awake 终端 字体 font 硬件加速 gpu 证书",
          icon: <GearSixIcon size={16} weight="regular" />,
        },
        {
          id: "git" as const,
          label: "Git",
          keywords: "git 源代码控制 diff stage commit worktree",
          icon: <GitBranchIcon size={16} weight="regular" />,
        },
        {
          id: "notifications" as const,
          label: "通知",
          keywords: "通知 提醒 声音 历史",
          icon: <BellIcon size={16} weight="regular" />,
        },
        {
          id: "appearance" as const,
          label: "外观",
          keywords: "外观 主题 背景 强调色 透明度 会话通信动效 连线",
          icon: <PaletteIcon size={16} weight="regular" />,
        },
        {
          id: "companionship" as const,
          label: "陪伴",
          keywords: "宠物 陪伴",
          icon: <HeartIcon size={16} weight="regular" />,
        },
      ],
    },
    {
      label: "智能体",
      items: [
        {
          id: "collaboration" as const,
          label: "协作中心",
          keywords: "项目 agent 多智能体 协作",
          icon: <UsersIcon size={16} weight="regular" />,
        },
        {
          id: "sharedMemory" as const,
          label: "共享记忆",
          keywords: "共享 记忆 ai-memory handoff 交接 跨智能体",
          icon: <BrainIcon size={16} />,
        },
        {
          id: "automation" as const,
          label: "自动化",
          keywords: "定时 任务 automation",
          icon: <AutomationIcon size={16} />,
        },
        { id: "quickCommands" as const, label: "快捷指令", keywords: "快捷 指令 command 收藏 自定义 范围", icon: <TerminalWindowIcon size={16} /> },
      ],
    },
    {
      label: "集成",
      items: [
        {
          id: "browserControl" as const,
          label: "浏览器控制",
          keywords: "浏览器 browser 控制 网页 自动化 playwright chrome edge 截图 snapshot",
          icon: <GlobeIcon size={16} weight="regular" />,
        },
        {
          id: "skills" as const,
          label: "Skills",
          keywords: "Skill 技能 中央库 分发 项目 Agent",
          icon: <ScrollIcon size={16} weight="regular" />,
        },
      ],
    },
    {
      label: "数据",
      items: [
        {
          id: "archivedSessions" as const,
          label: "归档会话",
          keywords: "归档 会话 恢复 删除",
          icon: <ArchiveIcon size={16} weight="regular" />,
        },
        { id: "privacy" as const, label: "隐私", keywords: "隐私 privacy 数据 目录 存储 外部 服务 CLI 访问", icon: <LockIcon size={16} weight="regular" /> },
      ],
    },
  ];
  const normalizedNavQuery = navQuery.trim().toLocaleLowerCase();
  const selectableProviders: ProviderConfig[] = [];

  const sectionCopy = {
    generalSettings: {
      eyebrow: "GENERAL",
      title: "常规",
      description:
        "配置网络代理、窗口与电源、终端字体和渲染选项。",
      note: "",
    },
    collaboration: {
      eyebrow: "COLLABORATION",
      title: "协作中心",
      description: "管理协作项目、Agent 团队和任务执行状态。",
      note: "",
    },
    sharedMemory: {
      eyebrow: "设置 / 智能体",
      title: "共享记忆",
      description: "跨智能体共享项目上下文，让下一次会话接着做。",
      note: "",
    },
    quickCommands: { eyebrow: "设置 / 智能体", title: "快捷指令", description: "常用操作，选定目标后执行。", note: "" },
    automation: {
      eyebrow: "AUTOMATION",
      title: "自动化",
      description: "创建、启停和审计定时任务与会话事件任务。",
      note: "",
    },
    appearance: {
      eyebrow: "APPEARANCE",
      title: "外观",
      description: "管理界面主题、背景、强调色与透明度。",
      note: "",
    },
    notifications: {
      eyebrow: "NOTIFICATIONS",
      title: "通知",
      description: "管理任务提醒和通知声音，查看通知历史。",
      note: "",
    },
    companionship: {
      eyebrow: "COMPANION",
      title: "陪伴",
      description: "管理桌面宠物的角色、互动和显示。",
      note: "",
    },
    git: {
      eyebrow: "SOURCE CONTROL",
      title: "Git 设置",
      description: "配置分支、比较基准、提交信息和安全同步策略。",
      note: "",
    },
    browserControl: {
      eyebrow: "BROWSER CONTROL",
      title: "浏览器控制",
      description:
        "选择 Agent 使用的浏览器，设置网页操作的运行方式。",
      note: "对已运行的会话不生效，重启会话后可用。",
    },
    skills: {
      eyebrow: "SKILL CENTER",
      title: "Skills",
      description: "统一纳管 Skill，并安全分发给多个 Runtime、项目和 Agent。",
      note: "",
    },
    archivedSessions: {
      eyebrow: "ARCHIVED SESSIONS",
      title: "归档会话",
      description: "查看、恢复或永久删除已归档的会话。",
      note: "",
    },
    privacy: { eyebrow: "PRIVACY", title: "隐私", description: "了解数据存放位置与 Agent 访问范围。", note: "" },
  }[activeSection];
  return (
    <Modal
      overlayClassName="dialog-overlay settings-overlay settings-native-overlay"
      className="settings-dialog settings-native"
      onClose={onClose}
      closeOnOverlayClick
      labelledBy="settings-dialog-title"
    >
        <header className="settings-window-header">
          <span id="settings-dialog-title">设置</span>
          <button type="button" className="dialog-close" aria-label="关闭设置" onClick={onClose}>
            <XIcon size={18} aria-hidden="true" />
          </button>
        </header>
        <aside className="settings-nav" ref={settingsNav}>
          <label className="settings-search">
            <MagnifyingGlassIcon size={15} aria-hidden="true" />
            <input
              type="search"
              value={navQuery}
              onChange={(event) => setNavQuery(event.target.value)}
              placeholder="搜索设置"
              aria-label="搜索设置"
            />
          </label>
          <nav className="settings-nav-list" aria-label="设置分类">
            {settingsNavGroups.map((group) => {
              const items = group.items.filter(
                (item) =>
                  !normalizedNavQuery ||
                  `${item.label} ${item.keywords}`
                    .toLocaleLowerCase()
                    .includes(normalizedNavQuery),
              );
              if (!items.length) return null;
              return (
                <section className="settings-nav-group" key={group.label}>
                  <span className="settings-nav-group-label">
                    {group.label}
                  </span>
                  {items.map((item) => (
                    <button
                      type="button"
                      className={`settings-nav-item${activeSection === item.id ? " active" : ""}`}
                      aria-label={item.label}
                      aria-current={
                        activeSection === item.id ? "page" : undefined
                      }
                      onClick={() => { privacyNavigationTarget.current = null; setPrivacyCollaborationRequested(false); setTaskRequest(undefined); setActiveSection(item.id); }}
                      key={item.id}
                    >
                      <span className="settings-nav-icon" aria-hidden="true">
                        {item.icon}
                      </span>
                      <span className="settings-nav-label">{item.label}</span>
                    </button>
                  ))}
                </section>
              );
            })}
            {normalizedNavQuery &&
              settingsNavGroups.every((group) =>
                group.items.every(
                  (item) =>
                    !`${item.label} ${item.keywords}`
                      .toLocaleLowerCase()
                      .includes(normalizedNavQuery),
                ),
              ) && <p className="settings-search-empty">没有匹配的设置</p>}
          </nav>
        </aside>

        <div className="settings-content" key={activeSection}>
          <div
            className="settings-content-inner"
            data-settings-section={activeSection}
          >
            {activeSection !== "skills" && activeSection !== "git" && activeSection !== "companionship" && (activeSection !== "automation" || automationEditing) && <div className="dialog-heading settings-heading">
                <div>
                  <h2 id="settings-title">{activeSection === "automation" && automationEditing ? automationEditing === "edit" ? "编辑定时任务" : "创建自动化" : sectionCopy.title}</h2>
                  {!(activeSection === "automation" && automationEditing) && <p>{sectionCopy.description}</p>}
                </div>
              <div className="settings-heading-actions">
                {activeSection === "appearance" && (
                    <Switch
                      className="appearance-follow-system"
                      checked={appearance.followSystem}
                      onChange={(checked) =>
                        onAppearanceChange((current) => ({
                          ...current,
                          followSystem: checked,
                        }))
                      }
                      childrenPosition="before"
                    >
                      <span>
                        跟随系统
                        <small>
                          {resolveBaseTheme(resolvedAppearanceTheme) === "light"
                            ? "浅色"
                            : "深色"}
                        </small>
                      </span>
                    </Switch>
                  )}
              </div>
            </div>}
            {sectionCopy.note && <p className="settings-page-note">{sectionCopy.note}</p>}

            {activeSection === "privacy" ? (
              <PrivacySettingsPanel onNavigate={destination => { privacyNavigationTarget.current = destination; setNavQuery(""); setTaskRequest(undefined); setPrivacyCollaborationRequested(destination === "collaboration"); setActiveSection(destination); }} />
            ) : activeSection === "collaboration" ? (
              <CollaborationSettingsPanel onClose={onClose} initialTab={privacyCollaborationRequested ? "records" : initialCollaborationTab} initialDiagnosticGlobal={initialCollaborationDiagnosticGlobal} initialProjectId={initialCollaborationProjectId} taskRequest={taskRequest} />
            ) : activeSection === "sharedMemory" ? (
              <SharedMemorySettingsPanel />
            ) : activeSection === "quickCommands" ? (
              <QuickCommandCenter manage sessions={sessions} providers={selectableProviders} context={quickContext} onSessionsChange={onSessionsChange} />
            ) : activeSection === "automation" ? (
              <AutomationPanel
                onFormOpenChange={setAutomationEditing}
                sessions={sessions}
                providers={selectableProviders}
                embedded
              />
            ) : activeSection === "generalSettings" ? (
              <GeneralSettingsPanel theme={resolveBaseTheme(resolvedAppearanceTheme)} />
            ) : activeSection === "git" ? (
              <GitSettingsPanel />
            ) : activeSection === "browserControl" ? (
              <BrowserControlSettingsPanel />
            ) : activeSection === "skills" ? (
              <SkillCenterPanel projects={projects} />
            ) : activeSection === "notifications" ? (
              <NotificationSettingsPanel
                settings={notificationSettings}
                onChange={onNotificationSettingsChange}
              />
            ) : activeSection === "archivedSessions" ? (
              <ArchivedSessionsPanel
                sessions={archivedSessions}
                projects={projects}
                onRestore={onRestoreArchivedSession}
                onDelete={onDeleteArchivedSession}
                onViewTranscript={onViewArchivedTranscript}
              />
            ) : activeSection === "companionship" ? (
              <PetSettingsPanel settings={petSettings} onChange={onPetSettingsChange} />
            ) : (
              <div className="settings-tabbed-workspace">
                <AppearanceSettingsPanel appearance={appearance} onChange={onAppearanceChange} />
              </div>
            )}
          </div>
        </div>
    </Modal>
  );
}
