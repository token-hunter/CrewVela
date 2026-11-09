type PasteShortcutEvent = Pick<
  KeyboardEvent,
  "type" | "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey"
>;

export function isTerminalPasteShortcut(event: PasteShortcutEvent): boolean {
  if (event.type !== "keydown") return false;

  const key = event.key.toLowerCase();
  const commandPaste =
    (event.ctrlKey || event.metaKey) && !event.altKey && key === "v";
  const insertPaste =
    event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey && key === "insert";

  return commandPaste || insertPaste;
}

/**
 * Claude Code 2.1.226 在 Windows ConPTY 中启用了 kitty keyboard protocol。
 * Alt+V 必须使用 kitty 编码（ESC [ 118 ; 3 u）；旧式 ESC v 会被直接忽略。
 */
export function imagePasteSequence(platform: string): number[] {
  return platform.startsWith("Win")
    ? [0x1b, 0x5b, 0x31, 0x31, 0x38, 0x3b, 0x33, 0x75]
    : [0x16];
}

type ImageFileLike = Pick<File, "name" | "type">;

const SUPPORTED_IMAGE_MIME_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
]);

export function isSupportedImageFile(file: ImageFileLike): boolean {
  if (SUPPORTED_IMAGE_MIME_TYPES.has(file.type.toLowerCase())) return true;
  return /\.(png|jpe?g|gif|webp)$/i.test(file.name);
}

export function terminalDropIntent(
  files: readonly ImageFileLike[],
): "pane" | "image" | "reject" {
  if (files.length === 0) return "pane";
  return files.some(isSupportedImageFile) ? "image" : "reject";
}
