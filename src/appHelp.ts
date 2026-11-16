export type AppHelpSection = "privacy" | "updates" | "about";
export const APP_REPOSITORY_URL = "https://github.com/token-hunter/CrewVela";
export const APP_RELEASES_URL = `${APP_REPOSITORY_URL}/releases`;
export const APP_GUIDE_URL = `${APP_REPOSITORY_URL}/blob/main/docs/user-guide.md`;
export interface AppUpdateCheck {
  currentVersion: string;
  latestVersion: string;
  status: "available" | "current" | "ahead";
  releaseUrl: string;
  releaseNotes: string;
}

/** Only the configured repository's Release pages may be opened as update results. */
export function trustedReleaseUrl(raw: string): boolean {
  try {
    const url = new URL(raw);
    return url.origin === "https://github.com" && !url.username && !url.password &&
      url.pathname.startsWith("/token-hunter/CrewVela/releases/tag/") && !url.search && !url.hash &&
      /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\+[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/.test(url.pathname.slice("/token-hunter/CrewVela/releases/tag/".length));
  } catch { return false; }
}
