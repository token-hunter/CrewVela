import english from "./interfaceLanguage.en.json";
import { invoke } from "@tauri-apps/api/core";

export type InterfaceLanguage = "zh-CN" | "en";
type MessageValues = Record<string, string | number>;

const storageKey = "crewvela-interface-language";
const messages: Readonly<Record<string, string>> = english;
const listeners = new Set<() => void>();

function normalizeLanguage(value: unknown): InterfaceLanguage {
  return value === "en" ? "en" : "zh-CN";
}

function loadLanguage(): InterfaceLanguage {
  try {
    return normalizeLanguage(window.localStorage.getItem(storageKey));
  } catch {
    return "zh-CN";
  }
}

let language = loadLanguage();
let nativeLanguageUpdate: Promise<void> = Promise.resolve();

function applyLanguage(next: InterfaceLanguage) {
  if (typeof document !== "undefined") document.documentElement.lang = next;
  if (next === language) return;
  language = next;
  for (const listener of listeners) listener();
}

if (typeof window !== "undefined") {
  applyLanguage(language);
  window.addEventListener("storage", (event) => {
    if (event.key === storageKey || event.key === null) applyLanguage(loadLanguage());
  });
}

export function getInterfaceLanguage(): InterfaceLanguage {
  return language;
}

export function subscribeInterfaceLanguage(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function setInterfaceLanguage(next: InterfaceLanguage): boolean {
  try {
    window.localStorage.setItem(storageKey, next);
  } catch {
    return false;
  }
  applyLanguage(next);
  return true;
}

export function syncDesktopInterfaceLanguage(next: InterfaceLanguage): Promise<void> {
  const update = nativeLanguageUpdate.catch(() => undefined).then(() =>
    invoke<void>("set_interface_language", { language: next }),
  );
  nativeLanguageUpdate = update;
  return update;
}

export function translate(language: InterfaceLanguage, source: string, values?: MessageValues): string {
  const message = language === "en" && Object.prototype.hasOwnProperty.call(messages, source)
    ? messages[source]
    : source;
  return values ? message.replace(/\{(\w+)\}/g, (placeholder, key: string) =>
    Object.prototype.hasOwnProperty.call(values, key) ? String(values[key]) : placeholder,
  ) : message;
}

export function translateCurrent(source: string, values?: MessageValues): string {
  return translate(language, source, values);
}
