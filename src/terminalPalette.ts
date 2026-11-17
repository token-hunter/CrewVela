import type { AppTheme } from "./types";

const TERMINAL_THEMES = {
  dark: {
    background: "#161615",
    foreground: "#e8e6df",
    cursor: "#d97757",
    selectionBackground: "#5a3a2e",
    black: "#1f1f1e",
    red: "#f06b75",
    green: "#62cf8b",
    yellow: "#d7ab5e",
    blue: "#55aef2",
    magenta: "#ba91e6",
    cyan: "#54c8df",
    white: "#c9d2df",
    brightBlack: "#627286",
    brightRed: "#ff8790",
    brightGreen: "#7de3a2",
    brightYellow: "#ebc071",
    brightBlue: "#73c0f7",
    brightMagenta: "#cca8f1",
    brightCyan: "#74d9ec",
    brightWhite: "#faf9f5",
  },
  light: {
    background: "#ffffff",
    foreground: "#2b2a27",
    cursor: "#c6613f",
    selectionBackground: "#f2dccf",
    black: "#2b2a27",
    red: "#c43d56",
    green: "#4f7d37",
    yellow: "#9a651f",
    blue: "#315fa8",
    magenta: "#7c4d9e",
    cyan: "#187184",
    white: "#5f687b",
    brightBlack: "#687084",
    brightRed: "#dc5870",
    brightGreen: "#66994c",
    brightYellow: "#b77b2d",
    brightBlue: "#4a78bf",
    brightMagenta: "#9365b3",
    brightCyan: "#29899e",
    brightWhite: "#273043",
  },
} as const;

export function resolveTerminalTheme(theme: AppTheme, transparent: boolean) {
  if (!transparent) return TERMINAL_THEMES[theme];
  return {
    ...TERMINAL_THEMES[theme],
    background: "rgba(0, 0, 0, 0)",
  };
}
