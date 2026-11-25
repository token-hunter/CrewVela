import { LocalizedErrorText } from "./LocalizedErrorText";
import { useTranslation } from "../useInterfaceLanguage";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeftIcon, ArrowRightIcon, ArrowUpRightIcon, CaretRightIcon, CheckIcon, CopyIcon, CubeIcon, ChatCircleIcon, DotsThreeIcon, FileTextIcon, FolderIcon, InfoIcon, LinkIcon, MagnifyingGlassIcon, StackIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { AdditionalProjectAccess, ZcodeFinalize } from "./SharedMemoryProjectControls";
import { SharedMemoryQuickSetup } from "./SharedMemoryQuickSetup";
import { EmbeddedMemoryToggle, useEmbeddedMemory } from "./EmbeddedMemoryToggle";
import { sharedMemoryAgents as AGENTS, sharedMemoryCommands } from "../lib/sharedMemoryCommands";
import { pushModal, removeModal } from "./ui/modalStack";
import "../styles/shared-memory-settings.css";

const PREVIEW = typeof window !== "undefined" && new URLSearchParams(window.location.search).get("ui-preview") === "1";
const STATUS_POLL_INTERVAL_MS = 30_000;

interface AiMemoryStatus {
  installed: boolean;
  executable: string | null;
  dataDir: string | null;
  installSource: string | null;
  serverUrl: string;
  serverRunning: boolean;
  healthState?: string;
  healthError?: string | null;
  canStart?: boolean;
  managedInstallAvailable?: boolean;
  version: string | null;
  counts: { pages_latest?: number; sessions?: number; observations?: number } | null;
  webUrl: string | null;
  managedByCrewVela: boolean;
}

const PREVIEW_STATUS: AiMemoryStatus = {
  installed: true,
  executable: "C:\\Users\\demo\\AppData\\Roaming\\crewvela\\ai-memory\\ai-memory.exe",
  dataDir: "C:\\Users\\demo\\AppData\\Roaming\\crewvela\\ai-memory-data",
  installSource: "managed",
  serverUrl: "http://127.0.0.1:49374",
  serverRunning: true,
  version: "2.6.0",
  counts: { pages_latest: 12, sessions: 5, observations: 143 },
  webUrl: "http://127.0.0.1:49374/web",
  managedByCrewVela: true,
};

type View = "overview" | "setup" | "advanced";
type ServiceAction = "ai_memory_install" | "ai_memory_serve" | "ai_memory_stop";
type SetupAction = "hooks" | "mcp";
interface SetupResult { success: boolean; exitCode: number | null; timedOut: boolean; stdout: string; stderr: string; outputTruncated: boolean }
interface CheckItem { state: string; detail: string }
interface CheckReport { agent: string; projectDir: string; mcp: CheckItem; hooks: CheckItem; tools: CheckItem; capture: CheckItem }

function serviceStatusContextChanged(previous: AiMemoryStatus | null, next: AiMemoryStatus): boolean {
  if (!previous) return false;
  return previous.installed !== next.installed
    || previous.serverRunning !== next.serverRunning
    || previous.healthState !== next.healthState
    || previous.serverUrl !== next.serverUrl
    || previous.executable !== next.executable
    || previous.dataDir !== next.dataDir
    || previous.version !== next.version;
}

interface SetupOverflowMenuProps {
  label: string;
  canCopy: boolean;
  hasOutput: boolean;
  commandVisible: boolean;
  outputVisible: boolean;
  onCopy: () => void;
  onToggleCommand: () => void;
  onToggleOutput: () => void;
}

function MemoryOptionsMenu({ label, menuLabel, text, items }: {
  label: string; menuLabel: string; text?: string;
  items: Array<{ label: string; icon?: ReactNode; disabled?: boolean; action: () => void }>;
}) {
  const t = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    // 菜单暂时成为顶层，避免设置弹窗的 capture 阶段先消费 Escape。
    const modalId = pushModal();
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        rootRef.current?.querySelector<HTMLButtonElement>("[aria-haspopup='menu']")?.focus();
      }
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    rootRef.current?.querySelector<HTMLButtonElement>("[role='menuitem']:not(:disabled)")?.focus();
    return () => {
      removeModal(modalId);
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  const closeAndRun = (action: () => void) => {
    setOpen(false);
    window.requestAnimationFrame(() => rootRef.current?.querySelector<HTMLButtonElement>("[aria-haspopup='menu']")?.focus());
    action();
  };
  const handleMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[role='menuitem']:not(:disabled)"));
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    let next = -1;
    if (items.length && event.key === "ArrowDown") next = (current + 1 + items.length) % items.length;
    if (items.length && event.key === "ArrowUp") next = (current - 1 + items.length) % items.length;
    if (items.length && event.key === "Home") next = 0;
    if (items.length && event.key === "End") next = items.length - 1;
    if (next >= 0) {
      event.preventDefault();
      items[next]?.focus();
    }
    if (event.key === "Tab") setOpen(false);
  };

  return <div className={text ? "sm-overflow sm-home-options" : "sm-overflow"} ref={rootRef}>
    <button type="button" className="sm-overflow-trigger" aria-label={label} title={text ?? t("更多操作")} aria-haspopup="menu" aria-expanded={open} aria-controls={open ? menuId : undefined} onClick={() => setOpen(current => !current)}>
      <DotsThreeIcon size={21} weight="bold" aria-hidden="true" />
      {text}
    </button>
    {open && <div className="sm-overflow-menu" id={menuId} role="menu" aria-label={menuLabel} onKeyDown={handleMenuKeyDown}>
      {items.map(item => <button key={item.label} type="button" role="menuitem" disabled={item.disabled} onClick={() => closeAndRun(item.action)}>
        {item.icon}{t(item.label)}{text && <CaretRightIcon size={14} aria-hidden="true" />}
      </button>)}
    </div>}
  </div>;
}

function SetupOverflowMenu({ label, canCopy, hasOutput, commandVisible, outputVisible, onCopy, onToggleCommand, onToggleOutput }: SetupOverflowMenuProps) {
  const t = useTranslation();
  return <MemoryOptionsMenu label={t("{label}更多操作", { label: t(label) })} menuLabel={t("{label}操作", { label: t(label) })} items={[
    { label: "复制命令", disabled: !canCopy, action: onCopy },
    { label: commandVisible ? "隐藏命令" : "查看命令", disabled: !canCopy, action: onToggleCommand },
    { label: outputVisible ? "隐藏执行输出" : "查看执行输出", disabled: !hasOutput, action: onToggleOutput },
  ]} />;
}

export function SharedMemorySettingsPanel() {
  const t = useTranslation();
  const [page, setPage] = useState<"home" | "legacy" | "diagnostics">("home");
  const [legacyOpened, setLegacyOpened] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const previousPage = useRef(page);
  const memory = useEmbeddedMemory(PREVIEW);
  useEffect(() => {
    if (previousPage.current === page) return;
    previousPage.current = page;
    headingRef.current?.focus();
  }, [page]);

  return <div className="sm-page sm-shell">
    <header className="settings-heading sm-shell-header" data-memory-page={page}>
      <div className="sm-shell-title">
        {page !== "home" && <button type="button" className="sm-icon-button" aria-label={t("返回共享记忆")} title={t("返回共享记忆")} onClick={() => setPage("home")}><ArrowLeftIcon size={17} aria-hidden="true" /></button>}
        <h2 id="settings-title" ref={headingRef} tabIndex={-1}>{page === "legacy" ? t("旧版共享记忆") : page === "diagnostics" ? t("诊断信息") : t("共享记忆")}</h2>
      </div>
      {page === "home" && <MemoryOptionsMenu label={t("更多选项")} menuLabel={t("共享记忆更多选项")} text={t("更多选项")} items={[
        { label: "旧版共享记忆", icon: <StackIcon size={17} aria-hidden="true" />, action: () => { setLegacyOpened(true); setPage("legacy"); } },
        { label: "诊断信息", icon: <InfoIcon size={17} aria-hidden="true" />, action: () => setPage("diagnostics") },
      ]} />}
    </header>
    {page === "home" && <EmbeddedMemoryToggle memory={memory} />}
    {page === "diagnostics" && <section className="sm-card sm-advanced" aria-label={t("内置共享记忆诊断")}>
      <header className="sm-card-header"><h3>{t("内置共享记忆")}</h3>
        <button className="sm-button" type="button" disabled={memory.busy || memory.checking} onClick={memory.refresh}>{t("重新检测")}</button>
      </header>
      <dl>
        <div><dt>{t("内置引擎")}</dt><dd>{memory.status ? memory.status.available ? t("可用") : t("当前构建未内置") : t("未知")}</dd></div>
        <div><dt>{t("服务状态")}</dt><dd role="status">{memory.statusText}</dd></div>
      </dl>
      {memory.error && <p role="alert" className="sm-error"><LocalizedErrorText source={memory.error} /></p>}
    </section>}
    {legacyOpened && <div hidden={page !== "legacy"}><LegacySharedMemorySettingsPanel active={page === "legacy"} /></div>}
  </div>;
}

/** 服务运行与智能体接入分别展示；注册配置不等于会话捕获已验证。 */
export function LegacySharedMemorySettingsPanel({ active = true }: { active?: boolean }) {
  const t = useTranslation();
  const [status, setStatus] = useState<AiMemoryStatus | null>(PREVIEW ? PREVIEW_STATUS : null);
  const statusRef = useRef<AiMemoryStatus | null>(PREVIEW ? PREVIEW_STATUS : null);
  const [statusFresh, setStatusFresh] = useState(PREVIEW);
  const [statusReadError, setStatusReadError] = useState<string | null>(null);
  const [manualBusy, setBusy] = useState<string | null>(null);
  const [quickRunning, setQuickRunning] = useState(false);
  const quickRunningRef = useRef(false);
  const busy = manualBusy ?? (quickRunning ? "快速启用中…" : null);
  const pending = useRef(false);
  const statusProbeInFlight = useRef(false);
  const queuedManualRefresh = useRef(false);
  const operationRevision = useRef(0);
  const onQuickRunningChange = useCallback((running: boolean) => {
    if (running && !quickRunningRef.current) operationRevision.current += 1;
    quickRunningRef.current = running;
    setQuickRunning(running);
  }, []);
  const mounted = useRef(true);
  const refreshStatusRef = useRef<((manual?: boolean) => Promise<void>) | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [view, setView] = useState<View>("overview");
  const [agentIndex, setAgentIndex] = useState(1);
  const [expanded, setExpanded] = useState({ hooks: false, mcp: false });
  const [outputExpanded, setOutputExpanded] = useState({ hooks: false, mcp: false });
  const [projectDir, setProjectDir] = useState<string | null>(null);
  const [setupResults, setSetupResults] = useState<Record<string, SetupResult>>({});
  const [checkReport, setCheckReport] = useState<CheckReport | null>(null);
  const faqRef = useRef<HTMLDetailsElement>(null);
  const agent = AGENTS[agentIndex];
  const usesDeepSeekManualSetup = agent.id === "dsh";
  const usesPiExtension = agent.id === "pi";
  const usesAntigravityHooks = agent.id === "antigravity-cli";
  const context = `${status?.executable}|${status?.dataDir}|${status?.serverUrl}|`;
  const setupKey = (action: SetupAction) => `${context}${agent.id}-${action}-${action === "hooks" && agent.id === "claude-code" ? projectDir : "global"}`;
  const completed = setupResults[setupKey("hooks")]?.success && (usesPiExtension || setupResults[setupKey("mcp")]?.success);
  const checked = checkReport?.agent === agent.id && checkReport.projectDir === projectDir ? checkReport : null;
  const checkFailed = checked && [checked.mcp, checked.hooks, checked.tools, checked.capture].some(item => ["failed", "missing", "mismatch"].includes(item.state));
  const configured = checked?.mcp.state === "ok" && checked.hooks.state === "ok";
  const integrationSummary = usesDeepSeekManualSetup ? "手动接入" : checkFailed ? "检查发现问题" : completed || configured ? "配置已完成 · 待验证" : checked ? "已检查 · 会话待验证" : "接入待检查";
  const rowState = (action: SetupAction) => {
    const item = checked?.[action];
    if (item) {
      if (item.state === "ok") return { label: "配置检查通过", tone: "success" };
      if (item.state === "pending") return { label: "待验证", tone: "pending" };
      if (["failed", "missing", "mismatch"].includes(item.state)) return { label: "检查发现问题", tone: "danger" };
      return { label: "检查已完成", tone: "neutral" };
    }
    const result = setupResults[setupKey(action)];
    if (!result) return { label: "尚未检查", tone: "neutral" };
    if (result.success) return { label: "命令已执行 · 待检查", tone: "pending" };
    return { label: result.timedOut ? "执行失败 · 超时" : result.exitCode == null ? "执行失败" : t("执行失败 · 退出码 {code}", { code: result.exitCode }), tone: "danger" };
  };
  const verificationMessage = checked
    ? checkFailed ? "检查发现问题，请查看下方诊断结果。" : "配置检查完成；实际会话捕获仍待验证。"
    : completed ? "配置命令已执行，尚未完成接入检查。" : "尚未验证实际会话捕获。";
  const restartInstruction = agent.id === "codex"
    ? "启用 Codex 原生 hooks，确认信任提示并重启会话。"
    : agent.id === "claude-code"
      ? "从所选项目目录启动 Claude Code，并重启会话以加载项目级钩子。"
      : agent.id === "zcode"
        ? "重启 ZCode 以加载 MCP；会话结束时按需手动收尾。"
        : usesPiExtension
          ? "重启 Pi 以加载记忆扩展，再在所选项目验证保存和检索。"
          : t("重启 {agent} 会话，确认记忆工具已加载并验证会话捕获。", { agent: agent.label });
  const commands = sharedMemoryCommands(status);
  const applyStatus = (next: AiMemoryStatus) => {
    if (serviceStatusContextChanged(statusRef.current, next)) setCheckReport(null);
    statusRef.current = next;
    setStatus(next);
    setStatusFresh(true);
    setStatusReadError(null);
  };
  const chooseProject = async () => {
    if (pending.current) return;
    try {
      if (PREVIEW) { setProjectDir("C:\\example\\crewvela"); setCheckReport(null); return; }
      const directory = await open({ directory: true, multiple: false, title: "选择检查共享记忆接入的项目目录" });
      if (typeof directory === "string") { setProjectDir(directory); setCheckReport(null); }
    } catch (reason) { setError(`选择项目失败：${String(reason)}`); }
  };
  const executeSetup = async (action: SetupAction) => {
    if (usesDeepSeekManualSetup || pending.current || !selected || !statusFresh || !status?.serverRunning || (action === "hooks" && agent.id === "claude-code" && !projectDir)) return;
    operationRevision.current += 1;
    pending.current = true;
    const key = setupKey(action);
    setCheckReport(null);
    setBusy(key);
    setError(null);
    setSetupResults(current => { const next = { ...current }; delete next[key]; return next; });
    try {
      const result = PREVIEW
        ? { success: true, exitCode: 0, timedOut: false, stdout: "预览模拟：配置命令执行成功；重启会话后验证。未修改本机配置。", stderr: "", outputTruncated: false }
        : await invoke<SetupResult>("ai_memory_setup", { agent: agent.id, action, projectDir });
      if (mounted.current) setSetupResults(current => ({ ...current, [key]: result }));
    } catch (reason) {
      if (mounted.current) setSetupResults(current => ({ ...current, [key]: { success: false, exitCode: null, timedOut: false, stdout: "", stderr: String(reason), outputTruncated: false } }));
    }
    finally { pending.current = false; if (mounted.current) setBusy(null); }
  };
  const checkIntegration = async () => {
    if (usesDeepSeekManualSetup || pending.current || !projectDir || !status?.installed) return;
    operationRevision.current += 1;
    pending.current = true;
    setBusy("check-integration"); setError(null); setCheckReport(null);
    try {
      const report = PREVIEW ? { agent: agent.id, projectDir,
        mcp: { state: "ok", detail: "预览模拟：已注册当前服务" },
        hooks: { state: "ok", detail: "预览模拟：发现钩子配置；实际运行待验证" },
        tools: { state: "ok", detail: "预览模拟：记忆服务工具可用；会话加载待验证" },
        capture: { state: "pending", detail: "预览模拟：历史记录不能证明当前会话钩子有效" } }
        : await invoke<CheckReport>("ai_memory_check", { agent: agent.id, projectDir });
      if (mounted.current) setCheckReport(report);
    } catch (reason) { if (mounted.current) setError(`接入检查失败：${String(reason)}`); }
    finally { pending.current = false; if (mounted.current) setBusy(null); }
  };
  const selected = agent.id === "dsh" ? null : commands?.forAgent(agent.id) ?? null;

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(null), 2400);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const copy = async (text: string, key: string) => {
    setCopied(null);
    try {
      if (PREVIEW) await navigator.clipboard.writeText(text);
      else await writeText(text);
      setCopyError(null);
      setCopied(key);
    } catch (reason) { setCopyError(`复制失败：${String(reason)}`); }
  };
  const refreshStatus = useCallback(async (manual = false) => {
    if (PREVIEW || pending.current || quickRunningRef.current) return;
    if (statusProbeInFlight.current) {
      if (manual) {
        // Preserve the user's explicit refresh even if an automatic probe is already in flight.
        queuedManualRefresh.current = true;
        setBusy("检测中…");
      }
      return;
    }
    statusProbeInFlight.current = true;
    const revision = operationRevision.current;
    if (manual) {
      pending.current = true;
      setBusy("检测中…");
    }
    try {
      const next = await invoke<AiMemoryStatus>("ai_memory_status");
      if (!mounted.current || revision !== operationRevision.current) return;
      applyStatus(next);
    } catch (reason) {
      if (!mounted.current || revision !== operationRevision.current) return;
      setStatusFresh(false);
      setStatusReadError(String(reason));
    } finally {
      statusProbeInFlight.current = false;
      if (manual) {
        pending.current = false;
        if (mounted.current) setBusy(null);
      }
      if (queuedManualRefresh.current && mounted.current && !pending.current) {
        queuedManualRefresh.current = false;
        void refreshStatusRef.current?.(true);
      }
    }
  }, []);
  refreshStatusRef.current = refreshStatus;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      queuedManualRefresh.current = false;
      operationRevision.current += 1;
    };
  }, []);
  useEffect(() => { if (active) void refreshStatus(); }, [active, refreshStatus]);
  useEffect(() => {
    if (!active || view !== "setup") return;
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") void refreshStatus();
    };
    refreshWhenVisible();
    const interval = window.setInterval(refreshWhenVisible, STATUS_POLL_INTERVAL_MS);
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [active, view, refreshStatus]);
  const run = async (action: ServiceAction, label: string) => {
    if (pending.current) return;
    operationRevision.current += 1;
    pending.current = true;
    setBusy(`${label}中…`);
    setError(null);
    try {
      if (PREVIEW) {
        applyStatus({ ...(statusRef.current ?? PREVIEW_STATUS), installed: true,
          serverRunning: action !== "ai_memory_stop", managedByCrewVela: true });
      } else {
        const next = await invoke<AiMemoryStatus>(action);
        if (mounted.current) applyStatus(next);
      }
    } catch (reason) {
      // 刷新状态不能抹掉操作失败的原因。
      if (mounted.current && statusProbeInFlight.current) {
        setStatusFresh(false);
        setStatusReadError("服务操作后状态尚未确认，请重新检测。");
        queuedManualRefresh.current = true;
      } else if (mounted.current) try {
        const next = await invoke<AiMemoryStatus>("ai_memory_status");
        if (mounted.current) applyStatus(next);
      }
      catch (statusReason) {
        if (mounted.current) {
          setStatusFresh(false);
          setStatusReadError(String(statusReason));
        }
      }
      if (mounted.current) setError(`${label}失败：${String(reason)}`);
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(null);
      if (queuedManualRefresh.current && mounted.current) {
        queuedManualRefresh.current = false;
        void refreshStatusRef.current?.(true);
      }
    }
  };
  const openWiki = () => {
    if (!statusFresh || !status?.serverRunning || !status.webUrl) return;
    window.dispatchEvent(new CustomEvent("crewvela:browse", { detail: { url: status.webUrl } }));
  };
  const canStart = statusFresh && !!status?.installed && !status.serverRunning && (status.canStart ?? !status.healthError);
  const summary = !statusFresh ? (statusReadError ? "状态检测失败" : "状态未知")
    : !status ? "状态未知" : status.serverRunning ? "服务运行中"
    : status.healthError && status.healthState !== "offline" ? "服务异常"
    : !status.installed ? "未安装" : "服务未运行";
  const setupDisabledReason = (action: SetupAction) => {
    if (busy) return "当前有操作正在执行，请等待完成后重试。";
    if (!statusFresh) return statusReadError
      ? `服务状态检测失败：${statusReadError}。请重新检测后再配置。`
      : "正在确认服务状态；如等待过久，请重新检测。";
    if (!status?.installed) return "ai-memory 尚未安装，请先点击上方“下载安装”。";
    if (!status.serverRunning) return canStart ? "ai-memory 服务未运行，请先点击上方“启动服务”。" : status.healthError || "请检查外部服务后重新检测。";
    if (!selected) return "缺少可执行文件或数据目录，无法生成配置命令；请重新检测或安装 ai-memory。";
    if (action === "hooks" && agent.id === "claude-code" && !projectDir) return "Claude Code 的钩子需要项目目录，请先选择验证项目。";
    return null;
  };
  const source = status?.installSource === "managed" ? "CrewVela 托管" : status?.installSource === "path" ? "PATH 发现" : status?.installSource ? "环境变量" : "—";
  const copyButton = (text: string | null | undefined, key: string, label: string) => <button
    type="button" className="sm-button sm-copy-button" disabled={!text}
    onClick={() => text && void copy(text, key)}>
    {copied === key ? <CheckIcon size={16} aria-hidden="true" /> : <CopyIcon size={16} aria-hidden="true" />}
    {copied === key ? t("已复制") : t(label)}
  </button>;
  const manual = <div className="sm-manual">
    <h4>{t("仅捕获指定项目")}</h4>
    <p>{t("Claude 钩子使用项目级配置（--scope project）；其余钩子及 MCP 默认写入用户级配置，会影响其他项目的新会话。仅捕获指定项目时，需先配置 ai-memory 的 allowlist 捕获模式，并为允许的目录设置")} <code>.ai-memory.toml</code>。</p>
    <h4>{t("如何确认接入生效")}</h4>
    <p>{t("重启后检查智能体是否列出 ai-memory 工具，再实际完成一次会话交接。服务计数和配置注册不能证明捕获成功。CrewVela 只展示状态与待接提示，不代替智能体消费记忆。")}</p>
    <p>{t("Codex 需启用原生 hooks 并确认新的钩子信任提示。CrewVela 内原生聊天是否转发生命周期事件仍需实际验证。")}</p>
    <ZcodeFinalize key={JSON.stringify([projectDir, context, status?.serverRunning])} projectDir={projectDir} status={statusFresh && status?.serverRunning ? status : null} preview={PREVIEW} copy={text => void copy(text, "finalize")} />
    {copied === "finalize" && <p role="status">{t("命令已复制到剪贴板")}</p>}
  </div>;

  return <div className="shared-memory-settings sm-page">
    <nav className="sm-tabs" aria-label={t("共享记忆视图")}>
      {([['overview', t("概览")], ['setup', t("接入配置")], ['advanced', t("高级设置")]] as const).map(([id, label]) =>
        <button type="button" key={id} className={view === id ? "active" : ""} aria-current={view === id ? "page" : undefined} onClick={() => setView(id)}>{t(label)}</button>)}
    </nav>
    {error && <p role="alert" className="sm-error"><LocalizedErrorText source={error} /></p>}
    {statusReadError && <p role="status" className="sm-status-read-error">{t("服务状态检测失败：")} <LocalizedErrorText source={statusReadError} />{t("。当前服务状态未知，请重新检测后再配置。")}</p>}
    {statusFresh && status?.healthError && <p role="status" className="sm-status-read-error"><LocalizedErrorText source={status.healthError} /></p>}
    {status?.managedInstallAvailable === false && !status.installed && <p role="status">{t("请手动安装 ai-memory，并将其加入 PATH 后重新检测。")}</p>}
    {copyError && <p role="alert" className="sm-error"><LocalizedErrorText source={copyError} /></p>}
    <SharedMemoryQuickSetup hidden={view !== "overview"} disabled={PREVIEW || Boolean(manualBusy)} onRunningChange={onQuickRunningChange} onComplete={() => { void refreshStatus(true); }} onVerify={project => { setProjectDir(project); setView("setup"); }} />
    <span className="sm-sr-only" role="status">{copied ? t("已复制到剪贴板") : ""}</span>
    {view === "overview" && <>
      <section className="sm-card sm-service" aria-labelledby="sm-service-title">
        <header className="sm-card-header">
          <h3 id="sm-service-title">{t("服务状态")}</h3>
          <div className="sm-actions">
            <button type="button" className="sm-button" disabled={Boolean(busy)} onClick={() => void refreshStatus(true)}>{busy === "检测中…" ? busy : t("重新检测")}</button>
            {status && !status.installed && status.managedInstallAvailable !== false && <button type="button" className="sm-button sm-primary" disabled={Boolean(busy)} onClick={() => void run("ai_memory_install", "下载安装")}>{busy && busy !== "检测中…" ? busy : t("下载安装（约 20MB）")}</button>}
            {canStart && <button type="button" className="sm-button sm-primary" disabled={Boolean(busy)} onClick={() => void run("ai_memory_serve", "启动服务")}>{busy && busy !== "检测中…" ? busy : t("启动服务")}</button>}
            {statusFresh && status?.managedByCrewVela && <button type="button" className="sm-button" disabled={Boolean(busy)} onClick={() => void run("ai_memory_stop", "停止服务")}>{busy && busy !== "检测中…" ? busy : t("停止服务")}</button>}
          </div>
        </header>
        <div className={`sm-service-status${statusFresh && status?.serverRunning ? " running" : ""}`}><span className="sm-dot" aria-hidden="true" /><strong>{t(summary)}</strong>{statusFresh && status?.version && <span className="sm-badge">v{status.version}</span>}</div>
        <p className="sm-muted">{t("独立进程，退出 CrewVela 后仍会继续运行。")}</p>
        <dl className="sm-stats">{([
          [t("页面"), status?.counts?.pages_latest], [t("会话"), status?.counts?.sessions], [t("观察"), status?.counts?.observations],
        ] as const).map(([label, count]) => <div key={label}><dt>{t(label)}</dt><dd>{statusFresh && status?.serverRunning && count != null ? count.toLocaleString() : "—"}</dd></div>)}</dl>
        <div className="sm-service-footer"><div className="sm-endpoint"><span>{t("本地地址")}</span><code>{status?.serverUrl ?? "—"}</code><button type="button" className="sm-icon-button" aria-label={t("复制服务地址")} title={copied === "endpoint" ? t("已复制") : t("复制服务地址")} disabled={!status?.serverUrl} onClick={() => status && void copy(status.serverUrl, "endpoint")}>{copied === "endpoint" ? <CheckIcon size={16} /> : <CopyIcon size={16} />}</button></div><span>来源&nbsp; {source}</span></div>
      </section>
      <div className="sm-overview-bento-grid">
        <section className="sm-card sm-wiki">
          <div className="sm-bento-head">
            <span className="sm-bento-icon"><CubeIcon size={22} aria-hidden="true" /></span>
            <div>
              <strong>{t("查看与检索共享记忆")}</strong>
              <p className="sm-muted">{t("在浏览器中打开本地 Wiki，查看项目记忆与会话记录。")}</p>
            </div>
          </div>
          <button type="button" className="sm-button sm-primary" disabled={!statusFresh || !status?.serverRunning || !status.webUrl || Boolean(busy)} onClick={openWiki}>{t("打开 Wiki")} <ArrowUpRightIcon size={16} aria-hidden="true" /></button>
        </section>
        <section className="sm-card sm-wiki sm-agent-overview-card" aria-label={t("智能体接入状态")}>
          <div className="sm-bento-head">
            <span className="sm-bento-icon"><ChatCircleIcon size={22} aria-hidden="true" /></span>
            <div>
              <strong>{agent.label} · {t(integrationSummary)}</strong>
              <p className="sm-muted">{t("配置与服务检查不代表当前会话已加载工具或捕获成功。")}</p>
            </div>
          </div>
          <div className="sm-agent-quick-pills" aria-label={t("快速切换智能体接入")}>
            {AGENTS.map((item, idx) => (
              <button
                key={item.id}
                type="button"
                className={`sm-agent-pill${agentIndex === idx ? " active" : ""}`}
                onClick={() => { setAgentIndex(idx); setView("setup"); }}
              >
                {item.label}
              </button>
            ))}
          </div>
          <button type="button" className="sm-button" onClick={() => setView("setup")}>{t("前往接入配置")} <ArrowRightIcon size={16} aria-hidden="true" /></button>
        </section>
      </div>
    </>}
    {view === "setup" && <section className="sm-setup-v2" aria-label={t("接入配置")}>
      <div className="sm-setup-toolbar" role="group" aria-label={t("共享记忆服务状态与操作")}>
        <div className="sm-agent-segment" role="group" aria-label={t("选择智能体")}>
          {AGENTS.map((item, index) => <button type="button" key={item.id} disabled={Boolean(busy)} aria-pressed={agentIndex === index} className={agentIndex === index ? "active" : ""} onClick={() => { setAgentIndex(index); setCheckReport(null); setExpanded({ hooks: false, mcp: false }); setOutputExpanded({ hooks: false, mcp: false }); setCopied(null); }}>{item.label}</button>)}
        </div>
        <div className="sm-setup-service-right">
          <div className="sm-setup-service-summary">
            <span className={`sm-dot${statusFresh && status?.serverRunning ? " running" : ""}`} aria-hidden="true" />
            <strong>{t(summary)}</strong>
            {statusFresh && status?.version && <span className="sm-badge">v{status.version}</span>}
          </div>
          <span className={`sm-badge${checkFailed ? " sm-badge-warning" : ""}`}><span className="sm-dot" aria-hidden="true" />{integrationSummary}</span>
          <div className="sm-setup-service-actions">
            <button type="button" className="sm-button" disabled={Boolean(busy)} onClick={() => void refreshStatus(true)}>{busy === "检测中…" ? busy : t("重新检测")}</button>
            {status && !status.installed && status.managedInstallAvailable !== false && <button type="button" className="sm-button sm-primary" disabled={Boolean(busy)} onClick={() => void run("ai_memory_install", "下载安装")}>{busy === "下载安装中…" ? t("下载安装中…") : t("下载安装")}</button>}
            {canStart && <button type="button" className="sm-button sm-primary" disabled={Boolean(busy)} onClick={() => void run("ai_memory_serve", "启动服务")}>{busy === "启动服务中…" ? busy : t("启动服务")}</button>}
          </div>
        </div>
      </div>

      {usesDeepSeekManualSetup ? <section className="sm-setup-step sm-card" aria-labelledby="sm-dsh-title">
        <div className="sm-step-heading"><div className="sm-step-title-copy"><h4 id="sm-dsh-title">{t("手动接入 DeepSeek Harness")}</h4><p>{t("在所用的 DeepSeek Harness profile 中配置会话捕获和记忆检索。")}</p></div></div>
        <div className="sm-step-content sm-manual">
          <h4>{t("安装会话钩子")}</h4>
          <p>{t("先运行")} <code>dsh plugin --profile PROFILE add @deepseek-ai/dsh-hooks-claude-code</code>{t("，把 PROFILE 换成实际使用的 profile 名称。再运行下方命令生成钩子文件和配置片段。")}</p>
          {copyButton(commands?.deepSeek.hooks, "dsh-hooks", t("复制钩子命令"))}
          <h4>{t("生成 MCP 配置")}</h4>
          <p>在 {commands?.shellLabel ?? t("终端")} {t("中执行命令，将输出的两个")} <code>insert</code> {t("项追加到所用 profile 的")} <code>cordis.patch.yml</code>{t("，保留原有配置。")}</p>
          {copyButton(commands?.deepSeek.mcp, "dsh-mcp", t("复制 MCP 命令"))}
          {!commands && <p className="sm-unavailable">{t("检测到 ai-memory 可执行文件和数据目录后即可复制命令。")}</p>}
          <h4>{t("验证接入")}</h4>
          <p>{t("重启该 profile，确认会话已加载 ai-memory 工具。在同一项目保存一条测试记忆，再从新会话检索它。")}</p>
        </div>
      </section> : <>
      <section className="sm-setup-step sm-card" aria-labelledby="sm-project-title">
        <div className="sm-step-heading"><span className="sm-step-index">01</span><div className="sm-step-title-copy"><h4 id="sm-project-title">{t("选择验证项目")}</h4><p>{t("选择要检查捕获记录的项目目录。")}</p></div></div>
        <div className="sm-step-content">
          <div className="sm-project-picker">
            <div className={`sm-project-path${projectDir ? " selected" : ""}`}><FolderIcon size={20} aria-hidden="true" /><code>{projectDir ?? t("尚未选择项目目录")}</code></div>
            <button type="button" className="sm-button" disabled={Boolean(busy)} onClick={() => void chooseProject()}>{projectDir ? t("更换目录") : t("选择项目目录")}</button>
          </div>
          {projectDir && <AdditionalProjectAccess key={projectDir} projectDir={projectDir} preview={PREVIEW} />}
          <p className="sm-scope-copy"><span><strong>{t("配置范围：")}</strong>{agent.id === "claude-code" ? t("钩子仅写入当前项目，MCP 注册为用户全局。") : usesPiExtension ? t("扩展安装在 Pi 的用户目录，对新启动的 Pi 会话生效。") : t("钩子与 MCP 均写入用户全局配置，可能影响其他项目的新会话。")} {t("仅捕获指定项目需设置 allowlist。")}</span><button type="button" className="sm-inline-link" onClick={() => { if (faqRef.current) { faqRef.current.open = true; faqRef.current.scrollIntoView({ block: "nearest", behavior: "smooth" }); } }}>{t("查看说明")} <ArrowUpRightIcon size={14} aria-hidden="true" /></button></p>
        </div>
      </section>

      <section className="sm-setup-step sm-card" aria-labelledby="sm-config-title">
        <div className="sm-step-heading"><span className="sm-step-index">02</span><div className="sm-step-title-copy"><h4 id="sm-config-title">{t("配置智能体")}</h4><p>{t("启用会话捕获与记忆检索。")}</p></div></div>
        <div className="sm-step-content">
          <div className="sm-config-list">
            {(usesPiExtension ? [
              ["hooks", t("安装 Pi 记忆扩展"), t("同一个扩展提供会话捕获和记忆检索。"), t("安装扩展"), t("重新安装")],
            ] as const : [
              ["hooks", t("安装会话钩子"), usesAntigravityHooks ? t("在会话开始和回合结束时捕获项目记忆。") : t("会话结束后保存项目记忆。"), t("安装钩子"), t("重新安装")],
              ["mcp", t("注册 MCP"), t("允许智能体检索共享记忆。"), t("注册 MCP"), t("重新注册")],
            ] as const).map(([key, title, description, actionLabel, repeatLabel]) => {
              const resultKey = setupKey(key);
              const result = setupResults[resultKey];
              const state = rowState(key);
              const reportItem = checked?.[key];
              const hasConfiguration = reportItem ? reportItem.state === "ok" : Boolean(result?.success);
              const isHooks = key === "hooks";
              const disabledReason = setupDisabledReason(key);
              const disabledReasonId = `sm-disabled-reason-${key}`;
              return <div className={`sm-config-item${result && !result.success ? " failed" : ""}`} key={key}>
                <div className="sm-config-row">
                  <span className="sm-config-icon">{isHooks ? <LinkIcon size={18} aria-hidden="true" /> : <FileTextIcon size={18} aria-hidden="true" />}</span>
                  <div className="sm-config-copy"><h4>{t(title)}</h4><p>{t(description)}</p></div>
                  <span className={`sm-config-state ${state.tone}`}><span className="sm-config-state-dot" aria-hidden="true" />{t(state.label)}</span>
                  <button type="button" className="sm-button sm-config-action" disabled={Boolean(disabledReason)} aria-describedby={disabledReason ? disabledReasonId : undefined} onClick={() => void executeSetup(key)}>{busy === resultKey ? t("执行中…") : t(hasConfiguration ? repeatLabel : actionLabel)}</button>
                  <SetupOverflowMenu
                    label={title}
                    canCopy={Boolean(selected?.[key])}
                    hasOutput={Boolean(result)}
                    commandVisible={expanded[key]}
                    outputVisible={outputExpanded[key]}
                    onCopy={() => selected?.[key] && void copy(selected[key], `${agent.id}-${key}`)}
                    onToggleCommand={() => setExpanded(current => ({ ...current, [key]: !current[key] }))}
                    onToggleOutput={() => setOutputExpanded(current => ({ ...current, [key]: !current[key] }))}
                  />
                </div>
                {disabledReason && <p id={disabledReasonId} className="sm-disabled-reason"><LocalizedErrorText source={disabledReason} fallback="查看原始诊断" /></p>}
                {expanded[key] && selected && <div id={`sm-command-${key}`} className="sm-row-detail"><p>{isHooks && agent.id === "claude-code" ? t("在所选项目目录 {project} 的 {shell} 中执行：", { project: projectDir ?? t("（尚未选择）"), shell: commands?.shellLabel ?? "" }) : t("在 {shell} 中执行（用户级配置）：", { shell: commands?.shellLabel ?? "" })}</p><p>{t("此命令写入 ai-memory 原生配置。如果正在使用项目桥或受限钩子，请保留现有注册。")}</p><pre className="sm-command">{selected[key]}</pre></div>}
                {result && outputExpanded[key] && <div className={`sm-row-detail sm-output-detail${result.success ? "" : " failed"}`} role="status">
                  <strong className="sm-output-title">{result.success ? t("配置命令执行成功；重启会话后验证接入") : result.timedOut ? t("执行失败：命令超时并已终止，配置可能已部分写入") : t("执行失败") + (result.exitCode == null ? t("（退出码未知）") : t("（退出码 {code}）", { code: result.exitCode }))}</strong>
                  {result.outputTruncated && <p className="sm-output-truncated">{t("输出过长，仅显示前 64 KiB。")}</p>}
                  <p className="sm-output-label">{t("退出码：")}{result.exitCode ?? t("未知")}</p>
                  <p className="sm-output-label">{t("标准输出")}</p><pre className="sm-command">{result.stdout || t("（无输出）")}</pre>
                  <p className="sm-output-label">{t("错误输出")}</p><pre className="sm-command">{result.stderr || t("（无输出）")}</pre>
                </div>}
              </div>;
            })}
          </div>
          {!selected && <p className="sm-unavailable">{t("检测到可执行文件和数据目录后显示安装命令。")}</p>}
          <p className="sm-setup-notice"><WarningCircleIcon size={16} aria-hidden="true" /><span>{t("安装或注册会修改配置；ai-memory 会备份已有配置并保留其他 MCP 服务。执行最长等待 60 秒。")}</span></p>
          {copied === `${agent.id}-hooks` || copied === `${agent.id}-mcp` ? <p className="sm-copy-feedback" role="status">{t("命令已复制到剪贴板")}</p> : null}
        </div>
      </section>

      <section className="sm-setup-step sm-card" aria-labelledby="sm-verify-title">
        <div className="sm-step-heading"><span className="sm-step-index">03</span><div className="sm-step-title-copy"><h4 id="sm-verify-title">{t("验证接入")}</h4><p>{t("只读检查配置文件、服务工具和历史捕获记录，不会修改配置。")}</p></div></div>
        <div className="sm-step-content sm-verify-content">
          <div className="sm-verify-main-row">
            <div className="sm-verify-copy">
              <p className={`sm-verification-state${checkFailed ? " failed" : checked ? " checked" : ""}`}><span className="sm-config-state-dot" aria-hidden="true" />{verificationMessage}</p>
              <p className="sm-restart-help">{restartInstruction}</p>
            </div>
            <button type="button" className="sm-button sm-primary sm-verify-btn" disabled={Boolean(busy) || !projectDir || !status?.installed} onClick={() => void checkIntegration()}><MagnifyingGlassIcon size={16} aria-hidden="true" />{busy === "check-integration" ? t("检查中…") : t("检查接入")}</button>
          </div>
          {checked && <div className="sm-check-report sm-check-report-v2" role="status"><div className="sm-check-report-heading"><strong>{agent.label}</strong><code>{checked.projectDir}</code></div>{([[t("MCP 注册配置"), checked.mcp], [t("钩子配置"), checked.hooks], [t("记忆服务工具"), checked.tools], [t("项目捕获记录"), checked.capture]] as const).map(([label, item]) => <div key={label} className={`sm-check-item ${item.state}`}><strong>{label}</strong><p><LocalizedErrorText source={item.detail} fallback="查看原始诊断" /></p></div>)}</div>}
        </div>
      </section>

      <details className="sm-faq" ref={faqRef}><summary>{t("常见问题与手动操作")}</summary>{manual}</details>
      </>}
    </section>}
    {view === "advanced" && <section className="sm-card sm-advanced"><h3>{t("本地服务信息")}</h3><p className="sm-muted">{t("此处展示当前服务路径；智能体接入与捕获范围请前往接入配置。")}</p><dl>{[[t("可执行文件"), status?.executable], [t("数据目录"), status?.dataDir], [t("服务地址"), status?.serverUrl]].map(([label, value]) => <div key={label}><dt>{label}</dt><dd><code>{value ?? t("未检测到")}</code></dd></div>)}</dl><button type="button" className="sm-button" onClick={() => setView("setup")}>{t("前往接入配置")} <ArrowRightIcon size={16} aria-hidden="true" /></button></section>}
  </div>;
}
