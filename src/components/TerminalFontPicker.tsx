import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CaretDown } from "@phosphor-icons/react";
import { missingTerminalFonts, replacePrimaryTerminalFont, splitTerminalFontFamilies, terminalFontName, type InstalledTerminalFont } from "../terminalFonts";

const sampleFonts: InstalledTerminalFont[] = [
  { family: "Cascadia Code", aliases: [], monospace: true },
  { family: "Consolas", aliases: [], monospace: true },
  { family: "MesloLGS NF", aliases: [], monospace: true },
  { family: "Microsoft YaHei", aliases: ["微软雅黑"], monospace: false },
];
const byName = new Intl.Collator("zh-CN", { sensitivity: "base", numeric: true });

export function TerminalFontPicker({ value, onChange, preview }: { value: string; onChange: (value: string) => void; preview: boolean }) {
  const t = useTranslation();
  const listId = useId();
  const hintId = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [fonts, setFonts] = useState<InstalledTerminalFont[] | null>(null);
  const [error, setError] = useState("");
  const [placement, setPlacement] = useState({ above: false, height: 220 });
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const options = useRef<HTMLDivElement>(null);
  const ensureCatalog = () => setAttempt(value => value || 1);

  useEffect(() => {
    if (!attempt) return;
    let disposed = false;
    setError("");
    const read = preview ? Promise.resolve(sampleFonts) : invoke<InstalledTerminalFont[]>("terminal_fonts_list");
    void read.then(result => {
      if (disposed) return;
      setFonts([...result].sort((a, b) => Number(b.monospace) - Number(a.monospace) || byName.compare(a.family, b.family)));
    }).catch(reason => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; };
  }, [attempt, preview]);

  useLayoutEffect(() => {
    const anchor = root.current;
    if (!open || !anchor) return;
    const scroller = anchor.closest(".settings-content");
    const place = () => {
      const rect = anchor.getBoundingClientRect();
      const viewport = scroller?.getBoundingClientRect();
      const below = Math.min(window.innerHeight, viewport?.bottom ?? Infinity) - rect.bottom - 8;
      const above = rect.top - Math.max(0, viewport?.top ?? 0) - 8;
      const displayAbove = below < 220 && above > below;
      setPlacement({ above: displayAbove, height: Math.max(54, Math.min(220, displayAbove ? above : below)) });
    };
    place();
    scroller?.addEventListener("scroll", place, { passive: true });
    window.addEventListener("resize", place);
    return () => {
      scroller?.removeEventListener("scroll", place);
      window.removeEventListener("resize", place);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    search.current?.focus({ preventScroll: true });
    const onPointer = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointer, true);
    return () => document.removeEventListener("pointerdown", onPointer, true);
  }, [open]);

  const shownFonts = useMemo(() => {
    const term = query.trim().toLowerCase();
    return (fonts ?? []).filter(font => [font.family, ...font.aliases].some(name => name.toLowerCase().includes(term)));
  }, [fonts, query]);
  const missing = useMemo(() => fonts ? missingTerminalFonts(value, fonts) : [], [fonts, value]);
  const primary = terminalFontName(splitTerminalFontFamilies(value)[0] ?? "").toLowerCase();

  function select(family: string) {
    onChange(replacePrimaryTerminalFont(value, family));
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  }

  return <div className="terminal-font-control">
    <div ref={root} className={"model-combobox terminal-font-combobox" + (open ? " open" : "")}
      onBlur={event => {
        // Native scrollbar interaction can move focus to the containing dialog.
        // Outside pointer presses are handled separately from focus changes.
        const next = event.relatedTarget;
        if (next instanceof Node && !event.currentTarget.contains(next) && !next.contains(event.currentTarget)) setOpen(false);
      }}
      onKeyDown={event => {
        if (!open || event.nativeEvent.isComposing) return;
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          setOpen(false);
          trigger.current?.focus({ preventScroll: true });
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          const buttons = Array.from(options.current?.querySelectorAll("button") ?? []);
          const index = buttons.findIndex(button => button === event.target);
          if (event.target === search.current || index >= 0) {
            event.preventDefault();
            if (event.key === "ArrowUp" && index === 0) search.current?.focus();
            else buttons[Math.max(0, Math.min(buttons.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]?.focus();
          }
        }
      }}>
      <input id="general-terminal-font" aria-describedby={missing.length || (error && !open) ? hintId : undefined}
        value={value} placeholder={t("默认字体")} autoComplete="off" spellCheck={false}
        onBlur={() => { if (value.trim()) ensureCatalog(); }}
        onChange={event => onChange(event.target.value)} />
      <button type="button" ref={trigger} aria-expanded={open} aria-controls={open ? listId : undefined} onClick={() => {
        if (!open) { setQuery(""); ensureCatalog(); }
        setOpen(!open);
      }}>{t("本机字体")} <CaretDown size={13} aria-hidden="true" /></button>
      {open && <div className="model-combobox-list terminal-font-catalog" id={listId}
        data-above={placement.above} style={{ maxHeight: placement.height }}>
        <input ref={search} type="search" value={query} onChange={event => setQuery(event.target.value)}
          aria-label={t("搜索本机字体")} placeholder={t("搜索本机字体")} autoComplete="off" />
        <div ref={options} className="terminal-font-options" aria-label={t("可选字体")} role="group" tabIndex={-1}>
          <button type="button" className="model-combobox-option" aria-pressed={!value.trim()} onClick={() => select("")}>{t("默认字体")}</button>
          {shownFonts.map(font => <button type="button" className="model-combobox-option"
            key={font.family} aria-pressed={[font.family, ...font.aliases].some(name => name.toLowerCase() === primary)}
            onClick={() => select(font.family)}>
            <span>{font.family}</span>{font.monospace && <em>{t("等宽")}</em>}
          </button>)}
          {error ? <p className="terminal-font-empty" role="status">{t("无法读取本机字体。")}
            <button type="button" className="conversation-text-button" onClick={() => {
              search.current?.focus();
              setAttempt(value => value + 1);
            }}>{t("重试")}</button>
          </p> : !fonts ? <p className="terminal-font-empty" role="status">{t("正在读取本机字体…")}</p>
            : shownFonts.length === 0 && <p className="terminal-font-empty">{t("没有匹配的字体")}</p>}
        </div>
      </div>}
    </div>
    {missing.length > 0 && <small id={hintId} className="terminal-font-hint" role="status">{t("本机列表中未找到")} {missing.join("、")}{t("，将使用备用字体。")}</small>}
    {error && !open && <small id={hintId} className="terminal-font-hint" role="status">{t("无法读取本机字体列表，可以继续手动填写。")}</small>}
  </div>;
}
