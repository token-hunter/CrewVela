import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { useRef, useState } from "react";
import { Switch } from "./ui/Switch";
import { Modal } from "./ui/Modal";
import type { Dispatch, SetStateAction } from "react";
import { DotsSixVertical } from "@phosphor-icons/react";
import { stripOneM } from "../providerConfig";
import { BellIcon } from "@phosphor-icons/react/dist/csr/Bell";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { ArrowLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowLeft";
import { HeartIcon } from "@phosphor-icons/react/dist/csr/Heart";
import { CubeIcon } from "@phosphor-icons/react/dist/csr/Cube";
import { BrainIcon } from "@phosphor-icons/react/dist/csr/Brain";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { PaletteIcon } from "@phosphor-icons/react/dist/csr/Palette";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { EyeIcon } from "@phosphor-icons/react/dist/csr/Eye";
import { EyeSlashIcon } from "@phosphor-icons/react/dist/csr/EyeSlash";
import { ArrowCounterClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowCounterClockwise";
import { UsersIcon } from "@phosphor-icons/react/dist/csr/Users";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { MonitorIcon } from "@phosphor-icons/react/dist/csr/Monitor";
import { BooksIcon } from "@phosphor-icons/react/dist/csr/Books";
import { PlugsIcon } from "@phosphor-icons/react/dist/csr/Plugs";
import { PackageIcon } from "@phosphor-icons/react/dist/csr/Package";
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
import type { CompanionSettings } from "../companionSettings";
import { formatTokens } from "../format";
import { CollaborationSettingsPanel } from "./CollaborationSettingsPanel";
import { SharedMemorySettingsPanel } from "./SharedMemorySettingsPanel";
import { AppearanceSettingsPanel } from "./AppearanceSettingsPanel";
import { AutomationPanel } from "./AutomationPanel";
import { PetSettingsPanel } from "./PetSettingsPanel";
import { NotificationSettingsPanel } from "./NotificationSettingsPanel";
import { SkinSettingsPanel } from "./SkinSettingsPanel";
import { AutomationIcon } from "./AutomationIcon";
import { ArchivedSessionsPanel } from "./ArchivedSessionsPanel";
import { PROVIDER_LOGOS, providerTone } from "../providerLogos";
import { GitSettingsPanel } from "./GitSettingsPanel";
import { SkillCenterPanel } from "./SkillCenterPanel";
import { McpSettingsPanel } from "./McpSettingsPanel";
import { PluginMarketPanel } from "./PluginMarketPanel";
import { BrowserControlSettingsPanel } from "./BrowserControlSettingsPanel";
import { ComputerControlSettingsPanel } from "./ComputerControlSettingsPanel";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import { ProviderQuotaCell } from "./ProviderQuotaCell";

interface Props {
  sessions: SessionRecord[];
  archivedSessions: SessionRecord[];
  projects: Project[];
  providers: ProviderConfig[];
  keyStates: Record<string, boolean>;
  /** 按 provider 汇总的累计 token（全部会话，含已停止）。 */
  usageByProvider: Record<string, number>;
  appearance: AppAppearance;
  resolvedAppearanceTheme: AppearanceTheme;
  petSettings: PetSettings;
  notificationSettings: NotificationSettings;
  companionSettings: CompanionSettings;
  onAppearanceChange: Dispatch<SetStateAction<AppAppearance>>;
  onPetSettingsChange: (settings: PetSettings) => void;
  onNotificationSettingsChange: (settings: NotificationSettings) => void;
  onCompanionSettingsChange: (settings: CompanionSettings) => void;
  onEdit: (provider: ProviderConfig) => void;
  onNew: () => void;
  onDelete: (providerId: string) => void;
  onSetHidden: (providerId: string, hidden: boolean) => void;
  onRestoreDefault: (providerId: string) => void;
  onReorder: (providerIds: string[]) => Promise<void>;
  onRestoreArchivedSession: (session: SessionRecord) => void | Promise<void>;
  onDeleteArchivedSession: (session: SessionRecord) => void | Promise<void>;
  onViewArchivedTranscript: (session: SessionRecord) => void;
  onClose: () => void;
  initialSection?: SettingsSection;
  initialCollaborationTab?: "overview" | "diagnostics";
  initialCollaborationDiagnosticGlobal?: boolean;
  initialCollaborationProjectId?: string;
}

export type SettingsSection =
  | "generalSettings"
  | "providers"
  | "collaboration"
  | "sharedMemory"
  | "automation"
  | "git"
  | "browserControl"
  | "computerControl"
  | "mcp"
  | "pluginMarket"
  | "skills"
  | "appearance"
  | "notifications"
  | "companionship"
  | "archivedSessions";

export function SettingsDialog({
  sessions,
  archivedSessions,
  projects,
  providers,
  keyStates,
  usageByProvider,
  appearance,
  resolvedAppearanceTheme,
  petSettings,
  notificationSettings,
  companionSettings,
  onAppearanceChange,
  onPetSettingsChange,
  onNotificationSettingsChange,
  onCompanionSettingsChange,
  onEdit,
  onNew,
  onDelete,
  onSetHidden,
  onRestoreDefault,
  onReorder,
  onRestoreArchivedSession,
  onDeleteArchivedSession,
  onViewArchivedTranscript,
  onClose,
  initialSection = "providers",
  initialCollaborationTab = "overview",
  initialCollaborationDiagnosticGlobal = false,
  initialCollaborationProjectId,
}: Props) {
  const [draggedProviderId, setDraggedProviderId] = useState<string | null>(
    null,
  );
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [savingOrder, setSavingOrder] = useState(false);
  const [activeSection, setActiveSection] = useState<SettingsSection>(initialSection);
  const [appearanceTab, setAppearanceTab] = useState<"appearance" | "skins">(
    "appearance",
  );
  const [navQuery, setNavQuery] = useState("");
  const blockRowDragRef = useRef(false);

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
          keywords: "外观 主题 皮肤 背景",
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
          id: "providers" as const,
          label: "模型设置",
          keywords: "模型 provider 服务",
          icon: <CubeIcon size={16} weight="regular" />,
        },
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
          icon: <BrainIcon size={16} weight="duotone" />,
        },
        {
          id: "automation" as const,
          label: "自动化",
          keywords: "定时 任务 automation",
          icon: <AutomationIcon size={16} />,
        },
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
          id: "computerControl" as const,
          label: "电脑控制",
          keywords:
            "电脑 computer 控制 桌面 应用 窗口 点击 输入 uia 自动化 截图 kimi kimicu kimi-cu kimi-computer-use open-computer-use ocu 开源 codex",
          icon: <MonitorIcon size={16} weight="regular" />,
        },
        {
          id: "pluginMarket" as const,
          label: "插件市场",
          keywords: "插件 plugin marketplace 市场 安装 工具",
          icon: <PackageIcon size={16} weight="regular" />,
        },
        {
          id: "skills" as const,
          label: "Skill 中心",
          keywords: "Skill 技能 中央库 分发 项目 Agent",
          icon: <BooksIcon size={16} weight="regular" />,
        },
        {
          id: "mcp" as const,
          label: "MCP",
          keywords: "MCP 服务器 server 工具源 Model Context Protocol",
          icon: <PlugsIcon size={16} weight="regular" />,
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
      ],
    },
  ];
  const normalizedNavQuery = navQuery.trim().toLocaleLowerCase();
  const selectableProviders = providers.filter((provider) => !provider.hidden);

  const resetDragState = () => {
    setDraggedProviderId(null);
    setDropTargetId(null);
  };

  const moveProvider = async (targetProviderId: string) => {
    if (
      savingOrder ||
      !draggedProviderId ||
      draggedProviderId === targetProviderId
    ) {
      resetDragState();
      return;
    }

    const nextIds = providers.map((provider) => provider.id);
    const sourceIndex = nextIds.indexOf(draggedProviderId);
    const targetIndex = nextIds.indexOf(targetProviderId);
    if (sourceIndex < 0 || targetIndex < 0) {
      resetDragState();
      return;
    }

    nextIds.splice(sourceIndex, 1);
    nextIds.splice(targetIndex, 0, draggedProviderId);
    resetDragState();
    setSavingOrder(true);
    try {
      await onReorder(nextIds);
    } finally {
      setSavingOrder(false);
    }
  };

  const sectionCopy = {
    generalSettings: {
      eyebrow: "GENERAL",
      title: "常规",
      description:
        "配置网络代理、窗口与电源、终端字体和渲染选项。",
      note: "",
    },
    providers: {
      eyebrow: "CONNECTIONS",
      title: "模型设置",
      description: "配置模型端点、模型映射与 API Key。",
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
    automation: {
      eyebrow: "AUTOMATION",
      title: "自动化",
      description: "创建、启停和审计定时任务与会话事件任务。",
      note: "",
    },
    appearance: {
      eyebrow: "APPEARANCE",
      title: "外观",
      description: "管理界面主题、背景和完整皮肤。",
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
    computerControl: {
      eyebrow: "COMPUTER USE",
      title: "电脑控制",
      description:
        "管理 Agent 操作本机 Windows 应用时可用的控制方式。",
      note: "",
    },
    mcp: {
      eyebrow: "MCP SERVERS",
      title: "MCP 服务器",
      description:
        "接入 Model Context Protocol 工具源，启用的服务器会注入 Claude 会话与多 Agent 任务。",
      note: "对已运行的会话不生效，重启会话后可用。",
    },
    pluginMarket: {
      eyebrow: "PLUGIN MARKET",
      title: "插件市场",
      description:
        "浏览精选插件，管理已安装的插件和市场来源。",
      note: "",
    },
    skills: {
      eyebrow: "SKILL CENTER",
      title: "Skill 中心",
      description: "统一纳管 Skill，并安全分发给多个 Runtime、项目和 Agent。",
      note: "",
    },
    archivedSessions: {
      eyebrow: "ARCHIVED SESSIONS",
      title: "归档会话",
      description: "查看、恢复或永久删除已归档的会话。",
      note: "",
    },
  }[activeSection];
  const compactSectionHeading = false;
  const workspaceSection = ["generalSettings", "browserControl", "computerControl", "skills"].includes(activeSection);
  const wideContent =
    activeSection === "mcp" ||
    activeSection === "pluginMarket" ||
    activeSection === "skills" ||
    activeSection === "automation" ||
    activeSection === "archivedSessions";

  return (
    <Modal
      overlayClassName="dialog-overlay settings-overlay"
      className={`settings-dialog${activeSection === "automation" ? " settings-feature-dialog" : ""}${workspaceSection ? " settings-workspace-dialog" : ""}`}
      onClose={onClose}
      closeOnOverlayClick
      ariaLabel={compactSectionHeading ? `${sectionCopy.title}设置` : undefined}
      labelledBy={compactSectionHeading ? undefined : "settings-title"}
    >
        <aside className="settings-nav">
          <button
            className="settings-back"
            type="button"
            aria-label="返回应用"
            onClick={onClose}
          >
            <ArrowLeftIcon size={16} aria-hidden="true" />
            <span>返回应用</span>
          </button>
          <div className="settings-brand">
            <span className="brand-mark" aria-hidden="true">
              <GearSixIcon size={16} weight="regular" />
            </span>
            <span>设置</span>
          </div>
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
                      onClick={() => setActiveSection(item.id)}
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
          {sectionCopy.note && (
            <div className="settings-nav-note">{sectionCopy.note}</div>
          )}
        </aside>

        <div className="settings-content">
          <div
            className={`settings-content-inner${wideContent ? " wide" : ""}${activeSection === "collaboration" ? " collaboration-content" : ""}${activeSection === "sharedMemory" ? " shared-memory-content" : ""}`}
          >
            <div
              className={`dialog-heading settings-heading${compactSectionHeading ? " compact-heading" : ""}`}
            >
              {!compactSectionHeading && (
                <div>
                  <span className="eyebrow">{sectionCopy.eyebrow}</span>
                  <h2 id="settings-title">{sectionCopy.title}</h2>
                  <p>{sectionCopy.description}</p>
                </div>
              )}
              <div className="settings-heading-actions">
                {activeSection === "providers" && (
                  <button className="btn primary" onClick={onNew}>
                    <PlusIcon size={16} weight="bold" aria-hidden="true" />
                    <span>新建模型服务</span>
                  </button>
                )}
                {activeSection === "appearance" &&
                  appearanceTab === "appearance" && (
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
                          {resolvedAppearanceTheme === "light"
                            ? "浅色"
                            : "深色"}
                        </small>
                      </span>
                    </Switch>
                  )}
                <button
                  className="dialog-close"
                  aria-label="关闭设置"
                  onClick={onClose}
                >
                  <XIcon size={16} aria-hidden="true" />
                </button>
              </div>
            </div>

            {activeSection === "providers" ? (
              <div className="settings-provider-list settings-provider-quota-list">
                <p className="settings-provider-quota-note">
                  账户额度每 5 分钟更新；本地用量是 Token 统计，不代表账户额度。
                </p>
                {providers.map((provider) => {
                  const hasKey = keyStates[provider.id] ?? false;
                  const configured = provider.baseUrl.length > 0;
                  const effectiveModel = stripOneM(provider.env.ANTHROPIC_MODEL ?? "");
                  const usage = usageByProvider[provider.id] ?? 0;
                  const tone = providerTone(provider);
                  const logo = PROVIDER_LOGOS[tone];
                  return (
                    <article
                      className={`settings-provider-row settings-provider-quota-row${provider.hidden ? " is-hidden" : ""}${
                        draggedProviderId === provider.id ? " dragging" : ""
                      }${dropTargetId === provider.id ? " drop-target" : ""}`}
                      key={provider.id}
                      draggable={!savingOrder}
                      title="拖动整行调整顺序，单击内容编辑"
                      onPointerDownCapture={(event) => {
                        const target = event.target;
                        blockRowDragRef.current =
                          target instanceof Element &&
                          Boolean(target.closest(".settings-provider-actions, .provider-quota"));
                      }}
                      onPointerUpCapture={() => {
                        blockRowDragRef.current = false;
                      }}
                      onPointerCancel={() => {
                        blockRowDragRef.current = false;
                      }}
                      onDragStart={(event) => {
                        if (blockRowDragRef.current) {
                          event.preventDefault();
                          blockRowDragRef.current = false;
                          return;
                        }
                        setDraggedProviderId(provider.id);
                        setDropTargetId(null);
                        event.dataTransfer.effectAllowed = "move";
                        event.dataTransfer.setData("text/plain", provider.id);
                      }}
                      onDragEnd={resetDragState}
                      onDragOver={(event) => {
                        if (!draggedProviderId || savingOrder) return;
                        event.preventDefault();
                        event.dataTransfer.dropEffect = "move";
                        setDropTargetId(provider.id);
                      }}
                      onDragLeave={(event) => {
                        const relatedTarget = event.relatedTarget;
                        if (
                          !(relatedTarget instanceof Node) ||
                          !event.currentTarget.contains(relatedTarget)
                        ) {
                          setDropTargetId((current) =>
                            current === provider.id ? null : current,
                          );
                        }
                      }}
                      onDrop={(event) => {
                        event.preventDefault();
                        void moveProvider(provider.id);
                      }}
                    >
                      <span
                        className="settings-provider-drag-handle"
                        title="也可以拖动整张卡片"
                        aria-hidden="true"
                      >
                        <DotsSixVertical
                          size={18}
                          weight="bold"
                          aria-hidden="true"
                        />
                      </span>
                      <button
                        className="settings-provider-main"
                        type="button"
                        onClick={() => onEdit(provider)}
                      >
                        <span
                          className={`provider-choice-icon${logo ? " has-logo" : ""}`}
                          data-provider-tone={tone}
                        >
                          {logo ? (
                            <img src={logo} alt="" aria-hidden="true" />
                          ) : (
                            (provider.name || provider.id)
                              .slice(0, 1)
                              .toUpperCase()
                          )}
                        </span>
                        <span className="settings-provider-copy">
                          <span className="settings-provider-title">
                            <strong>{provider.name || provider.id}</strong>
                            {provider.builtin && (
                              <span className="badge builtin">内置</span>
                            )}
                            {provider.hidden && (
                              <span className="badge provider-hidden">
                                已隐藏
                              </span>
                            )}
                          </span>
                          <small title={provider.baseUrl || "尚未配置 API 端点"}>
                            {provider.baseUrl || "尚未配置 API 端点"}
                          </small>
                          <span className="settings-provider-status">
                            <span
                              className={`status-chip ${configured ? "ok" : "warn"}`}
                            >
                              {configured ? "端点已配置" : "缺少端点"}
                            </span>
                            <span className={`status-chip ${hasKey ? "ok" : "warn"}`}>
                              {hasKey ? "Key 已设置" : "缺少 Key"}
                            </span>
                            <span
                              className={`status-chip ${effectiveModel ? "ok" : "warn"}`}
                              title={effectiveModel ? "未匹配角色时实际使用的兜底模型" : "未设置 ANTHROPIC_MODEL，会话将使用 Claude Code 内置默认模型"}
                            >
                              {effectiveModel ? `兜底 ${effectiveModel}` : "兜底模型未配置"}
                            </span>
                          </span>
                        </span>
                      </button>
                      <div className="settings-provider-usage settings-provider-quota-usage">
                        <ProviderQuotaCell provider={provider} hasKey={hasKey} active={activeSection === "providers"} />
                        <small title="该模型服务全部会话的累计 token（含输入/输出/缓存），不代表账户额度">本地用量：{usage > 0 ? `${formatTokens(usage)} tok` : "—"}</small>
                      </div>
                      <span className="settings-provider-actions">
                        <button
                          className="settings-provider-edit"
                          type="button"
                          onClick={() => onEdit(provider)}
                        >
                          <PencilSimpleIcon size={14} aria-hidden="true" />
                          <span>编辑</span>
                        </button>
                        {provider.builtin ? (
                          <>
                            <button
                              className="settings-provider-icon-action"
                              type="button"
                              title={provider.hidden ? "重新显示" : "隐藏"}
                              aria-label={`${provider.hidden ? "重新显示" : "隐藏"} ${provider.name || provider.id}`}
                              onClick={() =>
                                onSetHidden(provider.id, !provider.hidden)
                              }
                            >
                              {provider.hidden ? (
                                <EyeIcon size={14} />
                              ) : (
                                <EyeSlashIcon size={14} />
                              )}
                            </button>
                            <button
                              className="settings-provider-icon-action"
                              type="button"
                              title="恢复默认"
                              aria-label={`恢复 ${provider.name || provider.id} 默认配置`}
                              onClick={() => onRestoreDefault(provider.id)}
                            >
                              <ArrowCounterClockwiseIcon size={14} />
                            </button>
                          </>
                        ) : (
                          <button
                            className="settings-provider-delete"
                            type="button"
                            aria-label={`删除 ${provider.name || provider.id}`}
                            onClick={() => onDelete(provider.id)}
                          >
                            <TrashIcon size={14} aria-hidden="true" />
                          </button>
                        )}
                      </span>
                    </article>
                  );
                })}
              </div>
            ) : activeSection === "collaboration" ? (
              <CollaborationSettingsPanel onClose={onClose} initialTab={initialCollaborationTab} initialDiagnosticGlobal={initialCollaborationDiagnosticGlobal} initialProjectId={initialCollaborationProjectId} />
            ) : activeSection === "sharedMemory" ? (
              <SharedMemorySettingsPanel />
            ) : activeSection === "automation" ? (
              <AutomationPanel
                sessions={sessions}
                providers={selectableProviders}
                embedded
              />
            ) : activeSection === "generalSettings" ? (
              <GeneralSettingsPanel />
            ) : activeSection === "git" ? (
              <GitSettingsPanel />
            ) : activeSection === "browserControl" ? (
              <BrowserControlSettingsPanel />
            ) : activeSection === "computerControl" ? (
              <ComputerControlSettingsPanel />
            ) : activeSection === "pluginMarket" ? (
              <PluginMarketPanel projects={projects} />
            ) : activeSection === "skills" ? (
              <SkillCenterPanel projects={projects} />
            ) : activeSection === "mcp" ? (
              <McpSettingsPanel />
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
                <div
                  className="settings-section-tabs"
                  role="tablist"
                  aria-label="外观设置"
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={appearanceTab === "appearance"}
                    className={appearanceTab === "appearance" ? "active" : ""}
                    onClick={() => setAppearanceTab("appearance")}
                  >
                    外观
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={appearanceTab === "skins"}
                    className={appearanceTab === "skins" ? "active" : ""}
                    onClick={() => setAppearanceTab("skins")}
                  >
                    皮肤
                  </button>
                </div>
                {appearanceTab === "skins" ? (
                  <SkinSettingsPanel
                    appearance={appearance}
                    settings={companionSettings}
                    onChange={onCompanionSettingsChange}
                    onAppearanceChange={(next) => onAppearanceChange(next)}
                  />
                ) : (
                  <AppearanceSettingsPanel
                    appearance={appearance}
                    onChange={onAppearanceChange}
                  />
                )}
              </div>
            )}
          </div>
        </div>
    </Modal>
  );
}
