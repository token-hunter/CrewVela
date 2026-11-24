import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { CanvasAddon } from "@xterm/addon-canvas";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { AppTheme } from "../types";
import { clampFontSize, resolveFontFamily } from "../terminalAppearance";
import { resolveTerminalTheme } from "../terminalPalette";

const samples = ["中文终端", "Aa Zz 0123456789", "┌──┬──┐ {} []", "└──┴──┘ <> @ #"];

function sampleLines(columns: number): string[] {
  const lines = [];
  for (const sample of samples) {
    let line = "";
    let width = 0;
    for (const character of sample) {
      const cells = /[\u4e00-\u9fff]/.test(character) ? 2 : 1;
      if (width + cells > columns) {
        lines.push(line);
        line = "";
        width = 0;
      }
      line += character;
      width += cells;
    }
    lines.push(line);
  }
  return lines;
}

export function TerminalFontPreview({ fontFamily, fontSize, theme }: { fontFamily: string; fontSize: number; theme: AppTheme }) {
  const t = useTranslation();
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);
  const redraw = useRef<() => void>(() => {});
  const size = clampFontSize(fontSize);
  const family = resolveFontFamily(fontFamily);

  useEffect(() => {
    const node = host.current;
    if (!node) return;
    const term = new Terminal({ rows: 5, scrollback: 0, disableStdin: true, allowProposedApi: true, customGlyphs: true });
    const fit = new FitAddon();
    term.loadAddon(fit);
    node.setAttribute("inert", "");
    term.open(node);
    term.textarea?.setAttribute("tabindex", "-1");
    try { term.loadAddon(new CanvasAddon()); }
    catch (error) { console.warn("终端预览使用默认渲染器:", error); }
    terminal.current = term;
    let frame = 0;
    redraw.current = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const cols = fit.proposeDimensions()?.cols;
        if (!cols) return;
        const lines = sampleLines(cols);
        term.resize(cols, lines.length);
        term.write("\x1b[2J\x1b[H\x1b[?25l" + lines.join("\r\n"));
      });
    };
    const observer = new ResizeObserver(redraw.current);
    observer.observe(node);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      terminal.current = null;
      term.dispose();
    };
  }, []);

  useEffect(() => {
    const term = terminal.current;
    if (!term) return;
    term.options.fontFamily = family;
    term.options.fontSize = size;
    term.options.theme = resolveTerminalTheme(theme, false);
    redraw.current();
  }, [family, size, theme]);

  return <section className="terminal-font-preview" aria-label={t("终端字体预览")}>
    <header><span>预览</span><small>保存后应用到终端</small></header>
    <div className="terminal-font-preview-sample">
      <div ref={host} aria-hidden="true" style={{ minHeight: Math.ceil(size * 1.18) * 4 }} />
    </div>
  </section>;
}
