import { useEffect, useLayoutEffect, useRef, useState, type RefObject, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { CaretRight, Question, ShieldCheck, ArrowClockwise, Info } from "@phosphor-icons/react";
import type { AppHelpSection } from "../appHelp";
import "../styles/app-help.css";

interface Props {
  trigger: RefObject<HTMLButtonElement>;
  onHelp: (section: AppHelpSection) => void;
  onClose: () => void;
}
const menuItems = (root: HTMLElement | null) => root ? [...root.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)')] : [];
export function AppToolsMenu(props: Props) {
  const root = useRef<HTMLDivElement>(null), help = useRef<HTMLDivElement>(null), helpButton = useRef<HTMLButtonElement>(null);
  const callbacks = useRef(props); callbacks.current = props;
  const [helpOpen, setHelpOpen] = useState(false);
  const [position, setPosition] = useState({left:8, top:42, helpLeft:196, helpTop:42});
  const focusHelp = useRef(false);
  const close = (restore = true) => { if (restore) callbacks.current.trigger.current?.focus(); callbacks.current.onClose(); };
  useLayoutEffect(() => {
    const place = () => {
      const anchor = callbacks.current.trigger.current?.getBoundingClientRect();
      if (!anchor) return;
      const width = root.current?.offsetWidth ?? 184, height = root.current?.offsetHeight ?? 120;
      const left = Math.max(8, Math.min(anchor.left, innerWidth-width-8));
      const top = Math.max(8, Math.min(anchor.bottom+5, innerHeight-height-8));
      const helpWidth = 224, helpHeight = help.current?.offsetHeight ?? 128;
      const right = left+width+4;
      const helpLeft = Math.max(8, Math.min(right+helpWidth <= innerWidth-8 ? right : left-helpWidth-4, innerWidth-helpWidth-8));
      const offset = (helpButton.current?.offsetTop ?? 0);
      setPosition({left,top,helpLeft,helpTop:Math.max(8,Math.min(top+offset-5,innerHeight-helpHeight-8))});
    };
    place(); window.addEventListener("resize",place); window.addEventListener("scroll",place,true);
    return () => { window.removeEventListener("resize",place);window.removeEventListener("scroll",place,true); };
  }, [helpOpen]);
  useEffect(() => { menuItems(root.current)[0]?.focus(); }, []);
  useEffect(() => { if (helpOpen && focusHelp.current) { menuItems(help.current)[0]?.focus(); focusHelp.current=false; } }, [helpOpen]);
  useEffect(() => {
    const key = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();event.stopPropagation();
        if (helpOpen) {setHelpOpen(false);helpButton.current?.focus();} else close();
      } else if (event.key === "Tab") { close(); }
    };
    window.addEventListener("keydown",key,{capture:true});return () => window.removeEventListener("keydown",key,{capture:true});
  }, [helpOpen]);
  const openHelp = (focus: boolean) => {
    focusHelp.current = focus;
    if (helpOpen && focus) menuItems(help.current)[0]?.focus();
    setHelpOpen(true);
  };
  const navigate = (event: KeyboardEvent<HTMLDivElement>, submenu = false) => {
    const items=menuItems(submenu ? help.current : root.current), index=items.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null=null;
    if (event.key === "ArrowDown") next=(index+1)%items.length;
    if (event.key === "ArrowUp") next=(index-1+items.length)%items.length;
    if (event.key === "Home") next=0;
    if (event.key === "End") next=items.length-1;
    if (next !== null) {event.preventDefault();event.stopPropagation();items[next]?.focus();}
    else if (event.key === "ArrowRight" && !submenu && event.target===helpButton.current) {event.preventDefault();openHelp(true);}
    else if (event.key === "ArrowLeft" && submenu) {event.preventDefault();setHelpOpen(false);helpButton.current?.focus();}
  };
  const choose = (section: AppHelpSection) => { close(); props.onHelp(section); };
  return createPortal(<>
    <div className="menu-backdrop app-tools-backdrop" onClick={() => close()} />
    <div className="unified-tools-menu app-tools-menu" role="menu" aria-label="更多工具" ref={root} style={{left:position.left,top:position.top}} onKeyDown={event=>navigate(event)}>
      <button ref={helpButton} type="button" role="menuitem" aria-haspopup="menu" aria-expanded={helpOpen} aria-controls={helpOpen?"app-help-submenu":undefined} className="app-help-root" onMouseEnter={()=>openHelp(false)} onClick={()=>openHelp(true)}><Question size={17} /><span>帮助</span><CaretRight size={13} /></button>
    </div>
    {helpOpen && <div ref={help} id="app-help-submenu" className="app-help-submenu" role="menu" aria-label="帮助" style={{left:position.helpLeft,top:position.helpTop}} onKeyDown={event=>navigate(event,true)}>
      <button type="button" role="menuitem" onClick={()=>choose("privacy")}><ShieldCheck size={17}/><span>隐私政策</span></button>
      <button type="button" role="menuitem" onClick={()=>choose("updates")}><ArrowClockwise size={17}/><span>检查更新…</span></button>
      <button type="button" role="menuitem" onClick={()=>choose("about")}><Info size={17}/><span>关于 CrewVela</span></button>
    </div>}
  </>,document.body);
}
