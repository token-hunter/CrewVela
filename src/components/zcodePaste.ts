/** ZCode 输入框粘贴/拖放附件的纯分流逻辑（后端系统剪贴板直读 + 字节兜底命令配合）。 */

export type PasteFileLike = { name: string; size: number };

/**
 * 粘贴分流决策：
 * - "files"：剪贴板带文件（截图、资源管理器复制、浏览器图片）→ 走附件通路，文件优先于同贴文本；
 * - "text"：纯文本 → 交原生插入，不拦截；
 * - "system"：文件与文本皆空 → 可能是 WebView 未暴露的位图/文件，交后端直读系统剪贴板。
 */
export function classifyComposerPaste(files: PasteFileLike[], text: string): "files" | "text" | "system" {
  if (files.length > 0) return "files";
  return text ? "text" : "system";
}

/** 附件路径合并去重：既有顺序保持、新增在后；数量与大小上限由后端 prepare_attachments 统一校验。 */
export function mergeAttachmentRefs(existing: string[], added: string[]): string[] {
  const merged = [...existing];
  for (const path of added) if (!merged.includes(path)) merged.push(path);
  return merged;
}
