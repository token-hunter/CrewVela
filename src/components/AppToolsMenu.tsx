import { useEffect, useLayoutEffect, useRef, useState, type RefObject, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { CaretRight, Question, ShieldCheck, ArrowClockwise, Info, Eye, CornersOut, CornersIn, GearSix } from "@phosphor-icons/react";
import type { AppHelpSection } from "../appHelp";
import type { AppViewMenuProps } from "../appView";
import { AppHelpDialog } from "./AppHelpDialog";
import "../styles/app-help.css";

interface Props {
  trigger: RefObject<HTMLButtonElement>;
  reserveSidebarSpace?: boolean;
  onHelp: (section: AppHelpSection) => void;
  onClose: () => void;
  onSettings?: () => void;
  view?: AppViewMenuProps;
}
type Section = "view" | "help";
const menuItems = (root: HTMLElement | null) => root ? [...root.querySelectorAll<HTMLButtonElement>('button[role^="menuitem"]:not(:disabled)')] : [];

export function AppToolsIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true">
      <path d="M2 4h14 M2 9h11 M2 14h6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export function AppToolsMenu(props: Props) {
  const root = useRef<HTMLDivElement>(null), sectionMenu = useRef<HTMLDivElement>(null);
  const sidebarSpacer = useRef<HTMLDivElement>(null);
  const helpButton = useRef<HTMLButtonElement>(null), viewButton = useRef<HTMLButtonElement>(null);
  const callbacks = useRef(props); callbacks.current = props;
  const [section, setSection] = useState<Section | null>(null);
  const [position, setPosition] = useState({left:8, top:42, sectionLeft:196, sectionTop:42, reservedHeight:0});
  const focusSection = useRef(false);
  useEffect(() => {
    props.view?.onMenuChange(true);
    return () => props.view?.onMenuChange(false);
  }, [props.view?.onMenuChange]);
  const sectionTrigger = () => section === "help" ? helpButton.current : viewButton.current;
  const close = (restore = true) => { if (restore) callbacks.current.trigger.current?.focus(); callbacks.current.onClose(); };
  useLayoutEffect(() => {
    const place = () => {
      const anchor = callbacks.current.trigger.current?.getBoundingClientRect();
      if (!anchor) return;
      const width = root.current?.offsetWidth ?? 184, height = root.current?.offsetHeight ?? 120;
      const left = Math.max(8, Math.min(anchor.left, innerWidth-width-8));
      const top = Math.max(8, Math.min(anchor.bottom+5, innerHeight-height-8));
      const spacerTop = sidebarSpacer.current?.getBoundingClientRect().top;
      const reservedHeight = spacerTop === undefined ? 0 : Math.max(0, top + height - spacerTop);
      const beside = (parentLeft: number, parentWidth: number, target: HTMLElement | null, anchorTop: number) => {
        const childWidth = target?.offsetWidth ?? 224, childHeight = target?.offsetHeight ?? 128;
        const right = parentLeft+parentWidth+4;
        const childLeft = Math.max(8, Math.min(right+childWidth <= innerWidth-8 ? right : parentLeft-childWidth-4, innerWidth-childWidth-8));
        return {left:childLeft, top:Math.max(8,Math.min(anchorTop-5,innerHeight-childHeight-8))};
      };
      // 根菜单的新位置先按 offset 计算，避免滚动/缩放时沿用旧的 bounding rect。
      const sectionOffset = sectionTrigger()?.offsetTop ?? 0;
      const second = beside(left,width,sectionMenu.current,top+sectionOffset);
      setPosition({left,top,sectionLeft:second.left,sectionTop:second.top,reservedHeight});
    };
    place(); window.addEventListener("resize",place); window.addEventListener("scroll",place,true);
    return () => { window.removeEventListener("resize",place);window.removeEventListener("scroll",place,true); };
  }, [section]);
  useEffect(() => { menuItems(root.current)[0]?.focus(); }, []);
  useEffect(() => { if (section && focusSection.current) { menuItems(sectionMenu.current)[0]?.focus(); focusSection.current=false; } }, [section]);
  const back = () => {
    if (section) { setSection(null); sectionTrigger()?.focus(); }
    else close();
  };
  useEffect(() => {
    const key = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();event.stopPropagation();
        back();
      } else if (event.key === "Tab") { close(false); }
    };
    window.addEventListener("keydown",key,{capture:true});return () => window.removeEventListener("keydown",key,{capture:true});
  }, [section]);
  const openSection = (next: Section, focus: boolean) => {
    focusSection.current = focus;
    if (section === next && focus) menuItems(sectionMenu.current)[0]?.focus();
    setSection(next);
  };
  const navigate = (event: KeyboardEvent<HTMLDivElement>, level: 0 | 1 = 0) => {
    const items=menuItems(level === 1 ? sectionMenu.current : root.current), index=items.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null=null;
    if (event.key === "ArrowDown") next=(index+1)%items.length;
    if (event.key === "ArrowUp") next=(index-1+items.length)%items.length;
    if (event.key === "Home") next=0;
    if (event.key === "End") next=items.length-1;
    if (next !== null) {event.preventDefault();event.stopPropagation();items[next]?.focus();}
    else if (event.key === "ArrowRight") {
      if (level === 0 && (event.target === helpButton.current || event.target === viewButton.current)) {
        event.preventDefault(); openSection(event.target === helpButton.current ? "help" : "view", true);
      }
    } else if (event.key === "ArrowLeft" && level > 0) { event.preventDefault(); back(); }
  };
  const choose = (section: AppHelpSection) => { close(); props.onHelp(section); };
  const act = (action: () => void) => { close(); action(); };
  const { view, onSettings } = props;
  const menu = createPortal(<>
    <div className="menu-backdrop app-tools-backdrop" onClick={() => close()} />
    <div className="unified-tools-menu app-tools-menu" role="menu" aria-label="更多工具" ref={root} style={{left:position.left,top:position.top}} onKeyDown={event=>navigate(event)}>
      {view && <button ref={viewButton} type="button" role="menuitem" aria-haspopup="menu" aria-expanded={section === "view"} aria-controls={section === "view"?"app-view-submenu":undefined} className="app-help-root" onMouseEnter={()=>openSection("view",false)} onClick={()=>openSection("view",true)}><Eye size={17} /><span>视图</span><CaretRight size={13} /></button>}
      <button ref={helpButton} type="button" role="menuitem" aria-haspopup="menu" aria-expanded={section === "help"} aria-controls={section === "help"?"app-help-submenu":undefined} className="app-help-root" onMouseEnter={()=>openSection("help",false)} onClick={()=>openSection("help",true)}><Question size={17} /><span>帮助</span><CaretRight size={13} /></button>
      {onSettings && <button type="button" role="menuitem" className="app-help-root" onMouseEnter={() => setSection(null)} onClick={() => act(onSettings)}><GearSix size={17} /><span>设置</span></button>}
    </div>
    {section === "help" && <div ref={sectionMenu} id="app-help-submenu" className="app-help-submenu" role="menu" aria-label="帮助" style={{left:position.sectionLeft,top:position.sectionTop}} onKeyDown={event=>navigate(event,1)}>
      <button type="button" role="menuitem" onClick={()=>choose("privacy")}><ShieldCheck size={17}/><span>隐私政策</span></button>
      <button type="button" role="menuitem" onClick={()=>choose("updates")}><ArrowClockwise size={17}/><span>检查更新…</span></button>
      <button type="button" role="menuitem" onClick={()=>choose("about")}><Info size={17}/><span>关于 CrewVela</span></button>
    </div>}
    {section === "view" && view && <div ref={sectionMenu} id="app-view-submenu" className="app-help-submenu app-view-submenu" role="menu" aria-label="视图" style={{left:position.sectionLeft,top:position.sectionTop}} onKeyDown={event=>navigate(event,1)}>
      <button type="button" role="menuitem" disabled={!view.fullscreenAvailable || view.fullscreenPending} onClick={()=>act(view.onFullscreen)}>{view.fullscreen ? <CornersIn size={17} /> : <CornersOut size={17} />}<span>{view.fullscreen ? "退出全屏" : "全屏"}</span></button>
    </div>}
  </>,document.body);
  return <>
    {props.reserveSidebarSpace && <div ref={sidebarSpacer} aria-hidden="true" style={{ height: position.reservedHeight, flexShrink: 0 }} />}
    {menu}
  </>;
}

/** 左侧栏隐藏后仍保留相同工具菜单，不依赖隐藏栏内的触发按钮。 */
export function CollapsedAppTools({ view, onSettings }: { view?: AppViewMenuProps; onSettings?: () => void }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [help, setHelp] = useState<AppHelpSection | null>(null);
  return <>
    <button ref={trigger} type="button" className="workspace-sidebar-expand" aria-label="更多工具" title="更多工具" aria-haspopup="menu" aria-expanded={open} onClick={()=>setOpen(current=>!current)}><AppToolsIcon /></button>
    {open && <AppToolsMenu trigger={trigger} view={view} onClose={()=>setOpen(false)} onHelp={setHelp} onSettings={onSettings} />}
    {help && <AppHelpDialog key={help} section={help} onClose={()=>setHelp(null)} />}
  </>;
}
