import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { FolderSimpleIcon } from "@phosphor-icons/react/dist/csr/FolderSimple";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { PaletteIcon } from "@phosphor-icons/react/dist/csr/Palette";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { TerminalIcon } from "@phosphor-icons/react/dist/csr/Terminal";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import {
  loadRightSidebarBreezeOn,
  loadRightSidebarLampOn,
  saveRightSidebarBreezeOn,
  saveRightSidebarLampOn,
} from "../appearance";
import { shellLabel, terminalShells } from "../platform";
import type { RightSidebarStartStyle } from "../types";
import type { SidebarTool } from "./RightSidebarActivityBar";

interface StartToolEntry {
  key: "files" | "terminal" | "browser" | "git" | "multi_agent";
  tool: SidebarTool;
  label: string;
  code: string;
  num: string;
  title: string;
  disabled: boolean;
  accentColor: string;
  icon: ReactNode;
}

const START_STYLE_MENU: ReadonlyArray<{ id: RightSidebarStartStyle; label: string }> = [
  { id: "minimal", label: "极简列表" },
  { id: "lamp", label: "深夜拉绳台灯" },
  { id: "clothesline", label: "微风晾衣架" },
  { id: "cassette", label: "复古磁带架" },
];

export function RightSidebarStartStyleCorner({
  startStyle,
  onStartStyleChange,
  onOpenAppearanceSettings,
}: {
  startStyle: RightSidebarStartStyle;
  onStartStyleChange: (style: RightSidebarStartStyle) => void;
  onOpenAppearanceSettings?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="rs-start-diy-corner" ref={rootRef}>
      {open && (
        <div className="rs-start-diy-popover" role="menu" aria-label="切换开始页模式">
          <div className="rs-start-diy-popover-title">开始页模式</div>
          {START_STYLE_MENU.map((option) => {
            const active = startStyle === option.id;
            return (
              <button
                key={option.id}
                type="button"
                role="menuitemradio"
                aria-checked={active}
                className={`rs-start-diy-option${active ? " active" : ""}`}
                onClick={() => {
                  onStartStyleChange(option.id);
                  setOpen(false);
                }}
              >
                <span>{option.label}</span>
                {active && <CheckIcon size={13} weight="bold" aria-hidden="true" />}
              </button>
            );
          })}
          {onOpenAppearanceSettings && (
            <>
              <div className="rs-start-diy-divider" />
              <button
                type="button"
                role="menuitem"
                className="rs-start-diy-option rs-start-diy-settings-link"
                onClick={() => {
                  setOpen(false);
                  onOpenAppearanceSettings();
                }}
              >
                <span>前往「外观」设置</span>
                <span aria-hidden="true">↗</span>
              </button>
            </>
          )}
        </div>
      )}
      <button
        type="button"
        className="rs-start-diy-trigger"
        aria-label="切换开始页模式"
        title="切换开始页模式"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <PaletteIcon size={13} aria-hidden="true" />
        <span>模式</span>
      </button>
    </div>
  );
}

export function RightSidebarStartCreative({
  root,
  onOpenTool,
  startStyle,
  onStartStyleChange,
  onOpenAppearanceSettings,
}: {
  root: string | null;
  onOpenTool: (tool: SidebarTool) => void;
  startStyle: Exclude<RightSidebarStartStyle, "minimal">;
  onStartStyleChange?: (style: RightSidebarStartStyle) => void;
  onOpenAppearanceSettings?: () => void;
}) {
  const defaultShell = terminalShells()[0];
  const tools: StartToolEntry[] = [
    {
      key: "files",
      tool: "files",
      label: "文件",
      code: "FILES",
      num: "01",
      title: "浏览工作区文件",
      disabled: !root,
      accentColor: "#d97757",
      icon: <FolderSimpleIcon size={17} />,
    },
    {
      key: "terminal",
      tool: defaultShell,
      label: "终端",
      code: "SHELL",
      num: "02",
      title: `在当前工作区运行 ${shellLabel(defaultShell)}`,
      disabled: !root,
      accentColor: "#5fbf77",
      icon: <TerminalIcon size={17} />,
    },
    {
      key: "browser",
      tool: "browser",
      label: "浏览器",
      code: "WEB",
      num: "03",
      title: "打开网页或搜索内容",
      disabled: false,
      accentColor: "#5ca0d3",
      icon: <GlobeIcon size={17} />,
    },
    {
      key: "git",
      tool: "git",
      label: "Git",
      code: "GIT",
      num: "04",
      title: "查看 Git 更改与提交记录",
      disabled: false,
      accentColor: "#d9a13b",
      icon: <GitBranchIcon size={17} weight="regular" />,
    },
    {
      key: "multi_agent",
      tool: "multi_agent",
      label: "多智能体",
      code: "CREW",
      num: "05",
      title: "协调智能体任务与协作进度",
      disabled: false,
      accentColor: "#9b7af0",
      icon: <RobotIcon size={17} />,
    },
  ];

  // 拉绳台灯默认关灯，切换后记住开关状态
  const [lampOn, setLampOn] = useState<boolean>(loadRightSidebarLampOn);
  const [cordStretch, setCordStretch] = useState(0);
  const [cordDragging, setCordDragging] = useState(false);
  const [beamShift, setBeamShift] = useState(0);
  const cordDragRef = useRef({ active: false, startY: 0, pulled: false });
  const lampStageRef = useRef<HTMLDivElement>(null);

  // 晾衣架微风开关与阵风动画
  const [breezeOn, setBreezeOn] = useState<boolean>(loadRightSidebarBreezeOn);
  const [gustingKeys, setGustingKeys] = useState<Record<string, boolean>>({});
  const gustTimersRef = useRef<number[]>([]);

  // 复古磁带架当前装入项
  const [loadedTape, setLoadedTape] = useState<StartToolEntry["key"]>("git");

  useEffect(() => {
    return () => {
      gustTimersRef.current.forEach((id) => window.clearTimeout(id));
      gustTimersRef.current = [];
    };
  }, []);

  const toggleLamp = () => {
    setLampOn((current) => {
      const next = !current;
      saveRightSidebarLampOn(next);
      return next;
    });
  };

  const toggleBreeze = () => {
    setBreezeOn((current) => {
      const next = !current;
      saveRightSidebarBreezeOn(next);
      return next;
    });
  };

  const triggerWindGust = () => {
    gustTimersRef.current.forEach((id) => window.clearTimeout(id));
    gustTimersRef.current = [];
    setGustingKeys({});
    tools.forEach((item, index) => {
      const startId = window.setTimeout(() => {
        setGustingKeys((current) => ({ ...current, [item.key]: true }));
        const endId = window.setTimeout(() => {
          setGustingKeys((current) => ({ ...current, [item.key]: false }));
        }, 1350);
        gustTimersRef.current.push(endId);
      }, index * 85);
      gustTimersRef.current.push(startId);
    });
  };

  const handleCordPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    cordDragRef.current = { active: true, startY: event.clientY, pulled: false };
    setCordDragging(true);
  };

  const handleCordPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!cordDragRef.current.active) return;
    const delta = Math.max(0, Math.min(36, event.clientY - cordDragRef.current.startY));
    setCordStretch(delta);
    if (delta > 16) cordDragRef.current.pulled = true;
  };

  const finishCordDrag = (clientY?: number) => {
    if (!cordDragRef.current.active) return;
    const { startY, pulled } = cordDragRef.current;
    cordDragRef.current.active = false;
    setCordDragging(false);
    setCordStretch(0);
    const isClick = typeof clientY === "number" && Math.abs(clientY - startY) < 6;
    if (pulled || isClick) toggleLamp();
  };

  const handleLampPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (lampOn || !lampStageRef.current) return;
    const rect = lampStageRef.current.getBoundingClientRect();
    lampStageRef.current.style.setProperty("--rs-flash-x", `${event.clientX - rect.left}px`);
    lampStageRef.current.style.setProperty("--rs-flash-y", `${event.clientY - rect.top}px`);
    lampStageRef.current.dataset.flashActive = "true";
  };

  const handleLampPointerLeave = () => {
    setBeamShift(0);
    if (lampStageRef.current) {
      delete lampStageRef.current.dataset.flashActive;
    }
  };

  const corner = onStartStyleChange ? (
    <RightSidebarStartStyleCorner
      startStyle={startStyle}
      onStartStyleChange={onStartStyleChange}
      onOpenAppearanceSettings={onOpenAppearanceSettings}
    />
  ) : null;

  if (startStyle === "lamp") {
    return (
      <nav
        className={`right-sidebar-start right-sidebar-start--lamp${lampOn ? " is-lights-on" : " is-lights-off"}`}
        aria-label="工作区工具"
        data-workspace-root={root ?? undefined}
        data-start-style="lamp"
      >
        <div
          className="rs-lamp-stage"
          ref={lampStageRef}
          onPointerMove={handleLampPointerMove}
          onPointerLeave={handleLampPointerLeave}
        >
          <div
            className="rs-lamp-beam"
            aria-hidden="true"
            style={{ "--rs-beam-shift": `${beamShift}` } as CSSProperties}
          />
          <div className="rs-lamp-flashlight" aria-hidden="true" />
          <span className="rs-lamp-dust rs-lamp-dust-1" aria-hidden="true" />
          <span className="rs-lamp-dust rs-lamp-dust-2" aria-hidden="true" />
          <span className="rs-lamp-dust rs-lamp-dust-3" aria-hidden="true" />

          <div className="rs-lamp-fixture" aria-hidden="true">
            <svg width="154" height="96" viewBox="0 0 154 96" fill="none">
              <line x1="77" y1="0" x2="77" y2="38" stroke="var(--border-strong)" strokeWidth="2.2" />
              <rect x="68" y="36" width="18" height="9" rx="3" className="rs-lamp-cap" />
              <path d="M33 82 C37 54, 54 44, 77 44 C100 44, 117 54, 121 82 Z" className="rs-lamp-shade" />
              <ellipse cx="77" cy="82" rx="43" ry="6.5" className="rs-lamp-inner" />
              <circle cx="77" cy="84" r="8.5" className="rs-lamp-bulb" />
              {lampOn && <circle cx="77" cy="84" r="16" className="rs-lamp-halo" />}
            </svg>
          </div>

          <div
            className={`rs-lamp-cord-hit${cordDragging ? " is-dragging" : ""}`}
            role="button"
            tabIndex={0}
            aria-pressed={lampOn}
            aria-label={lampOn ? "拉绳熄灯" : "拉绳开灯"}
            title={lampOn ? "点击或下拉熄灯" : "点击或下拉开灯"}
            onPointerDown={handleCordPointerDown}
            onPointerMove={handleCordPointerMove}
            onPointerUp={(event) => finishCordDrag(event.clientY)}
            onPointerCancel={() => finishCordDrag()}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                toggleLamp();
              }
            }}
          >
            <div className="rs-lamp-cord-string" style={{ height: `${66 + cordStretch}px` }} />
            <div className="rs-lamp-cord-bead" />
          </div>

          <div className="right-sidebar-start-tools rs-lamp-tools">
            {tools.map((item, index) => (
              <button
                key={item.key}
                type="button"
                disabled={item.disabled}
                title={item.title}
                onMouseEnter={() => setBeamShift((index - 2) * 3.2)}
                onClick={() => onOpenTool(item.tool)}
              >
                <i className="right-sidebar-start-icon" aria-hidden="true">{item.icon}</i>
                <span>{item.label}</span>
                <i className="right-sidebar-start-arrow-pill" aria-hidden="true">
                  <CaretRightIcon className="right-sidebar-start-arrow" size={14} />
                </i>
              </button>
            ))}
          </div>

          <div className="rs-lamp-status" aria-live="polite">
            {lampOn ? "已开灯 · 下拉拉绳熄灯" : "已关灯 · 下拉拉绳开灯"}
          </div>
        </div>
        {corner}
      </nav>
    );
  }

  if (startStyle === "clothesline") {
    const renderHanger = (item: StartToolEntry, index: number, dropPx = 0) => (
      <div
        key={item.key}
        className={`rs-hanger-wrap${gustingKeys[item.key] ? " is-gusting" : ""}`}
        style={
          {
            "--rs-sway-dur": `${(3.9 + index * 0.42).toFixed(2)}s`,
            "--rs-sway-delay": `${(-index * 0.65).toFixed(2)}s`,
            marginTop: `${dropPx}px`,
          } as CSSProperties
        }
      >
        <button
          type="button"
          className="rs-hanger-btn"
          disabled={item.disabled}
          title={item.title}
          onClick={() => onOpenTool(item.tool)}
        >
          <svg className="rs-hanger-svg" width="74" height="40" viewBox="0 0 74 40" fill="none" aria-hidden="true">
            <rect x="33" y="-6" width="8" height="15" rx="2" className="rs-clothespin" />
            <path d="M37 9 C37 15, 42 17, 37 19" stroke="var(--text-faint)" strokeWidth="1.7" strokeLinecap="round" />
            <path d="M37 19 L7 34 C4 36, 6 39, 10 39 L64 39 C68 39, 70 36, 67 34 L37 19 Z" stroke="var(--border-strong)" strokeWidth="1.6" fill="transparent" strokeLinejoin="round" />
          </svg>
          <span className="rs-hanger-tag">
            <i className="right-sidebar-start-icon rs-hanger-icon" style={{ color: item.accentColor }} aria-hidden="true">{item.icon}</i>
            <span className="rs-hanger-label">{item.label}</span>
            <em className="rs-hanger-code">{item.disabled ? "LOCKED" : item.code}</em>
          </span>
        </button>
      </div>
    );

    return (
      <nav
        className={`right-sidebar-start right-sidebar-start--clothesline${breezeOn ? " is-breeze-on" : ""}`}
        aria-label="工作区工具"
        data-workspace-root={root ?? undefined}
        data-start-style="clothesline"
      >
        <div className="rs-clothesline-stage">
          <div className="rs-clothesline-caption">DAILY GEAR LINE · 顺手摘下一件开工</div>
          <div className="right-sidebar-start-tools rs-clothesline-rows">
            <div className="rs-clothes-row">
              <svg className="rs-clothes-wire" viewBox="0 0 400 34" preserveAspectRatio="none" aria-hidden="true">
                <circle cx="8" cy="11" r="3" fill="var(--border-strong)" />
                <circle cx="392" cy="11" r="3" fill="var(--border-strong)" />
                <path d="M8 11 Q200 30 392 11" stroke="var(--border-strong)" strokeWidth="1.5" fill="none" />
              </svg>
              <div className="rs-hanger-grid">
                {tools.slice(0, 3).map((item, index) => renderHanger(item, index, index === 1 ? 4 : 0))}
              </div>
            </div>
            <div className="rs-clothes-row">
              <svg className="rs-clothes-wire" viewBox="0 0 400 34" preserveAspectRatio="none" aria-hidden="true">
                <circle cx="28" cy="11" r="3" fill="var(--border-strong)" />
                <circle cx="372" cy="11" r="3" fill="var(--border-strong)" />
                <path d="M28 11 Q200 28 372 11" stroke="var(--border-strong)" strokeWidth="1.5" fill="none" />
              </svg>
              <div className="rs-hanger-grid rs-hanger-grid-bottom">
                {tools.slice(3, 5).map((item, index) => renderHanger(item, index + 3))}
              </div>
            </div>
          </div>
          <div className="rs-clothesline-controls">
            <button type="button" className="rs-mini-toy-btn" onClick={triggerWindGust}>
              吹一阵风
            </button>
            <button type="button" className="rs-mini-toy-btn ghost" aria-pressed={breezeOn} onClick={toggleBreeze}>
              微风：{breezeOn ? "开" : "关"}
            </button>
          </div>
        </div>
        {corner}
      </nav>
    );
  }

  const activeTape = tools.find((item) => item.key === loadedTape) ?? tools[3];
  return (
    <nav
      className="right-sidebar-start right-sidebar-start--cassette"
      aria-label="工作区工具"
      data-workspace-root={root ?? undefined}
      data-start-style="cassette"
    >
      <div className="rs-cassette-stage">
        <div className="rs-cassette-header">
          <span>TAPE ARCHIVE // 5 MIXTAPES</span>
          <small>悬停抽拉 · 点击装入</small>
        </div>

        <div className="right-sidebar-start-tools rs-tape-rack">
          {tools.map((item) => {
            const playing = loadedTape === item.key && !item.disabled;
            return (
              <button
                key={item.key}
                type="button"
                className={`rs-tape-item${playing ? " is-playing" : ""}`}
                style={{ "--rs-tape-accent": item.accentColor } as CSSProperties}
                disabled={item.disabled}
                title={item.title}
                onClick={() => {
                  setLoadedTape(item.key);
                  onOpenTool(item.tool);
                }}
              >
                <i className="rs-tape-spine" aria-hidden="true" />
                <i className="right-sidebar-start-icon rs-tape-icon" style={{ color: item.accentColor }} aria-hidden="true">
                  {item.icon}
                </i>
                <span className="rs-tape-copy">
                  <strong>
                    {item.label}
                    <em>A·{item.num}</em>
                  </strong>
                  <small>{item.disabled ? "打开工作区后可用" : item.title}</small>
                </span>
                <span className="rs-tape-spools" aria-hidden="true">
                  <i />
                  <i />
                </span>
              </button>
            );
          })}
        </div>

        <div className="rs-walkman-deck" style={{ "--rs-tape-accent": activeTape.accentColor } as CSSProperties}>
          <div className="rs-walkman-reels" aria-hidden="true">
            <svg className="rs-walkman-reel" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">
              <circle cx="12" cy="12" r="9" opacity="0.35" />
              <circle cx="12" cy="12" r="3.5" />
              <path d="M12 3v5M12 16v5M3 12h5M16 12h5" />
            </svg>
            <span className="rs-walkman-window"><i /></span>
            <svg className="rs-walkman-reel rs-walkman-reel-slow" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">
              <circle cx="12" cy="12" r="9" opacity="0.35" />
              <circle cx="12" cy="12" r="3.5" />
              <path d="M12 3v5M12 16v5M3 12h5M16 12h5" />
            </svg>
          </div>
          <div className="rs-walkman-readout">
            <em>▶ DECK LOADED</em>
            <strong>{activeTape.label} · {activeTape.code}</strong>
          </div>
        </div>
      </div>
      {corner}
    </nav>
  );
}
