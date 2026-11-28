export type AppHelpSection = "privacy" | "updates" | "about";
export const APP_REPOSITORY_URL = "https://github.com/token-hunter/CrewVela";
export const APP_RELEASES_URL = `${APP_REPOSITORY_URL}/releases`;
export const APP_GUIDE_URL = `${APP_REPOSITORY_URL}/blob/main/docs/user-guide.md`;
export type AppUpdateCheck = {
  currentVersion: string;
  releaseNotes: string;
  canInstall?: boolean;
} & (
  | { status: "available" | "current" | "ahead"; latestVersion: string; releaseUrl: string }
  | { status: "unavailable"; latestVersion: null; releaseUrl: null }
);

const RELEASE_TAG_PATH = "/token-hunter/CrewVela/releases/tag/";
const RELEASE_TAG = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][\da-zA-Z-]*))*)?(?:\+[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/;

/** Only the configured repository's Release pages may be opened as update results. */
export function trustedReleaseUrl(raw: string): boolean {
  try {
    const url = new URL(raw);
    return url.origin === "https://github.com" && !url.username && !url.password &&
      url.pathname.startsWith(RELEASE_TAG_PATH) && !url.search && !url.hash &&
      RELEASE_TAG.test(url.pathname.slice(RELEASE_TAG_PATH.length));
  } catch { return false; }
}
