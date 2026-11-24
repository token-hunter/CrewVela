import { useTranslation } from "../useInterfaceLanguage";
import { createPortal } from "react-dom";
import { RuntimeManagementDialog } from "./RuntimeManagementDialog";
import { open as openDirectory } from "@tauri-apps/plugin-dialog";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { FolderSimpleIcon } from "@phosphor-icons/react/dist/csr/FolderSimple";
import { FolderSimplePlusIcon } from "@phosphor-icons/react/dist/csr/FolderSimplePlus";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { useRuntimeAvailability } from "../runtimeAvailability";
import {
  NEW_SESSION_CHAT_ORDER_STORAGE_KEY,
  NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY,
  useRuntimeOrder,
} from "../runtimePreferences";
import type { Project, SessionDirectory } from "../types";
import { CHAT_AGENTS, TERMINAL_AGENTS, type NewSessionAgent } from "../newSessionAgents";
import { resolveSessionDirectory } from "../sessionDirectory";

interface Props {
  projects: Project[];
  directory: SessionDirectory | null;
  onDirectoryChange: (directory: SessionDirectory) => void;
  onLaunch: (runtime: NewSessionAgent, directory: SessionDirectory | null) => void;
}

/**
 * 空工作区：参照 Claude Code 桌面版新会话页——目录卡片提供工作目录，
 * 先勾选工作目录，再从「选择 Agent」下拉里选一个，选完即在该目录开始会话。
 */
export function EmptyWorkspaceLauncher({ projects, directory, onDirectoryChange, onLaunch }: Props) {
  const t = useTranslation();
  const { isRuntimeEnabled, ready, error } = useRuntimeAvailability();
  // 与启动弹窗和 Runtime 管理共用排序。
  const terminalPreferences = useRuntimeOrder(TERMINAL_AGENTS, NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY);
  const chatPreferences = useRuntimeOrder(CHAT_AGENTS, NEW_SESSION_CHAT_ORDER_STORAGE_KEY);
  const terminalOrder = terminalPreferences.order;
  const chatOrder = chatPreferences.order;
  const [managingAgents, setManagingAgents] = useState(false);

  const chosenId = directory?.id ?? null;
  const [picking, setPicking] = useState(false);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const pickingRef = useRef(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);
  const pickDirectory = async () => {
    if (pickingRef.current) return;
    pickingRef.current = true;
    setPicking(true);
    setDirectoryError(null);
    try {
      const selected = await openDirectory({ directory: true, multiple: false, title: "选择工作目录", defaultPath: directory?.path });
      if (!mountedRef.current || typeof selected !== "string" || !selected.trim()) return;
      onDirectoryChange(resolveSessionDirectory(selected, projects));
    } catch (reason) {
      if (mountedRef.current) setDirectoryError(`选择目录失败：${String(reason)}`);
    } finally {
      pickingRef.current = false;
      if (mountedRef.current) setPicking(false);
    }
  };

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [listMaxHeight, setListMaxHeight] = useState<number | null>(null);
  const cardRef = useRef<HTMLElement>(null);
  const pillRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const keyword = query.trim().toLowerCase();
  const groups = [
    { title: "CLI 终端", items: terminalOrder },
    { title: "原生聊天", items: chatOrder },
  ]
    .map((group) => ({
      ...group,
      items: group.items.filter(
        (runtime) => isRuntimeEnabled(runtime) && RUNTIME_LABELS[runtime].toLowerCase().includes(keyword),
      ),
    }))
    .filter((group) => group.items.length > 0);
  const firstMatch = groups[0]?.items[0];

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    setQuery("");
    if (restoreFocus) pillRef.current?.focus();
  };

  const launch = (runtime: NewSessionAgent) => {
    close(false);
    if (!pickingRef.current) onLaunch(runtime, directory);
  };

  useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (!cardRef.current?.contains(event.target as Node)) close(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close(true);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  // 下拉往下展开：列表高度跟随空状态区域里剩下的空间。放不下时在列表里滚动，
  // 并截在某一行的中间，让「下面还有」一眼可见（macOS 的滚动条默认隐藏）。
  const optionCount = groups.reduce((sum, group) => sum + group.items.length, 0);
  useLayoutEffect(() => {
    if (!open) return;
    const fit = () => {
      const popover = popoverRef.current;
      const list = listRef.current;
      if (!popover || !list) return;
      const host = popover.closest(".empty-state")?.getBoundingClientRect();
      const bottom = host ? host.bottom : window.innerHeight;
      const chrome = popover.offsetHeight - list.offsetHeight;
      const room = Math.floor(bottom - popover.getBoundingClientRect().top - chrome - 12);
      if (list.scrollHeight <= room) {
        setListMaxHeight(null);
        return;
      }
      // 至少留约 3 行的高度；截断点始终落在某一行的中间。
      const limit = Math.max(room, 96);
      let cut = limit;
      for (const option of list.querySelectorAll<HTMLElement>("[data-agent-option]")) {
        const middle = option.offsetTop + option.offsetHeight / 2;
        if (middle > limit) break;
        cut = middle;
      }
      setListMaxHeight(Math.floor(cut));
    };
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [open, keyword, optionCount]);

  const onPopoverKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const options = [...(popoverRef.current?.querySelectorAll<HTMLButtonElement>("[data-agent-option]") ?? [])];
    if (!options.length) return;
    event.preventDefault();
    const current = options.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      event.key === "ArrowDown"
        ? (current + 1) % options.length
        : current <= 0
          ? options.length - 1
          : current - 1;
    options[next].focus();
  };

  const emptyMessage = !ready
    ? error
      ? `Agent 配置加载失败：${error}`
      : "正在加载 Agent 配置…"
    : keyword
      ? "没有匹配的 Agent"
      : "没有已启用的 Agent，请先在设置中启用。";

  return (
    <div className="empty-state">
      <section ref={cardRef} className="ews-card" aria-label={t("新建会话")}>
        <div className="ews-projects" role="radiogroup" aria-label={t("工作目录")}>
          {projects.map((project) => (
            <label key={project.id} className="ews-row" title={project.path}>
              <input
                type="radio"
                name="ews-project"
                checked={project.id === chosenId}
                disabled={picking}
                onChange={() => { setDirectoryError(null); onDirectoryChange(project); }}
              />
              <FolderSimpleIcon size={16} aria-hidden="true" />
              <span className="ews-row-label">{project.name}</span>
              {project.id === chosenId && <CheckIcon className="ews-check" size={15} weight="bold" aria-hidden="true" />}
            </label>
          ))}
          {directory && !directory.id && (
            <label className="ews-row" title={directory.path}>
              <input type="radio" name="ews-project" checked disabled={picking} onChange={() => {}} />
              <FolderSimpleIcon size={16} aria-hidden="true" />
              <span className="ews-row-label">{directory.name}</span>
              <CheckIcon className="ews-check" size={15} weight="bold" aria-hidden="true" />
            </label>
          )}
          <button type="button" className="ews-row is-other" disabled={picking} onClick={() => void pickDirectory()}>
            <FolderSimplePlusIcon size={16} aria-hidden="true" />
            <span className="ews-row-label">{picking ? "正在选择目录…" : t("选择其他目录…")}</span>
          </button>
        </div>
        {directoryError && <div role="alert">{directoryError}</div>}
        <div className="ews-divider" />
        <button
          ref={pillRef}
          type="button"
          className="ews-pill"
          disabled={picking || !directory}
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-controls={open ? "ews-agent-popover" : undefined}
          onClick={() => (open ? close(false) : setOpen(true))}
        >
          <span>{t("选择 Agent")}</span>
          <CaretDownIcon size={12} weight="bold" aria-hidden="true" />
        </button>
        {open && (
          <div
            ref={popoverRef}
            id="ews-agent-popover"
            className="ews-popover"
            role="dialog"
            aria-label={t("选择 Agent")}
            onKeyDown={onPopoverKeyDown}
          >
            <label className="ews-search">
              <MagnifyingGlassIcon size={14} aria-hidden="true" />
              <input
                ref={searchRef}
                aria-label={t("搜索 Agent")}
                placeholder={t("搜索 Agent")}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" || !firstMatch) return;
                  event.preventDefault();
                  launch(firstMatch);
                }}
              />
            </label>
            <div ref={listRef} className="ews-popover-list" style={{ maxHeight: listMaxHeight ?? undefined }}>
              {groups.length > 0 ? (
                groups.map((group) => (
                  <div key={group.title} className="ews-group" role="group" aria-label={group.title}>
                    <div className="ews-group-label" aria-hidden="true">{group.title}</div>
                    {group.items.map((runtime) => (
                      <button
                        key={runtime}
                        type="button"
                        className="ews-option"
                        data-agent-option
                        onClick={() => launch(runtime)}
                      >
                        <img className="ews-logo" src={RUNTIME_LOGOS[runtime]} data-runtime={runtime} alt="" aria-hidden="true" />
                        <span className="ews-row-label">{RUNTIME_LABELS[runtime]}</span>
                      </button>
                    ))}
                  </div>
                ))
              ) : (
                <div className="ews-popover-empty" role="status">{emptyMessage}</div>
              )}
            </div>
            <div className="ews-divider" />
            <button
              type="button"
              className="ews-option is-footer"
              onClick={() => {
                close(false);
                setManagingAgents(true);
              }}
            >
              <GearSixIcon size={15} aria-hidden="true" />
              <span className="ews-row-label">{t("管理 Agent")}</span>
            </button>
          </div>
        )}
      </section>
      {managingAgents && createPortal(
        <RuntimeManagementDialog
          order={[...terminalOrder, ...chatOrder]}
          orderError={terminalPreferences.error || chatPreferences.error}
          onReorder={(kind, target, position) => {
            const preferences = terminalOrder.some(runtime => runtime === kind) ? terminalPreferences : chatPreferences;
            const source = preferences.order.find(runtime => runtime === kind);
            const destination = preferences.order.find(runtime => runtime === target);
            if (source && destination) preferences.reorder(source, destination, position);
          }}
          onClose={() => { setManagingAgents(false); setOpen(true); }}
        />,
        document.body,
      )}
    </div>
  );
}
