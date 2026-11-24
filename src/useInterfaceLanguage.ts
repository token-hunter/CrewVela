import { useCallback, useSyncExternalStore } from "react";
import { getInterfaceLanguage, subscribeInterfaceLanguage, translate } from "./interfaceLanguage";

export function useInterfaceLanguage() {
  return useSyncExternalStore(subscribeInterfaceLanguage, getInterfaceLanguage);
}

export function useTranslation() {
  const language = useInterfaceLanguage();
  return useCallback((source: string, values?: Parameters<typeof translate>[2]) =>
    translate(language, source, values), [language]);
}
