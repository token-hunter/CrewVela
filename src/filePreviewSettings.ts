export type FilePreviewKind =
  | "image"
  | "pdf"
  | "markdown"
  | "html"
  | "code"
  | "binary"
  | "video";

export type FilePreviewSettings = Record<FilePreviewKind, boolean>;

const STORAGE_KEY = "crewvela-file-preview-settings-v1";

export const DEFAULT_FILE_PREVIEW_SETTINGS: FilePreviewSettings = {
  image: true,
  pdf: true,
  markdown: true,
  html: true,
  code: true,
  binary: true,
  video: true,
};

export function loadFilePreviewSettings(): FilePreviewSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<FilePreviewSettings> | null;
    return saved ? { ...DEFAULT_FILE_PREVIEW_SETTINGS, ...saved } : DEFAULT_FILE_PREVIEW_SETTINGS;
  } catch {
    return DEFAULT_FILE_PREVIEW_SETTINGS;
  }
}

export function saveFilePreviewSettings(settings: FilePreviewSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}
