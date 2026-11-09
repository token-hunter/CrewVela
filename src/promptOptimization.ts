export interface PromptOptimization {
  optimized: string;
  notes: string;
}

/** 草稿为空或与优化稿实质一致时不需要展示对比。 */
export function optimizationIsUseful(draft: string, optimization: PromptOptimization): boolean {
  const normalized = draft.trim();
  if (!normalized) return false;
  if (!optimization.optimized.trim()) return false;
  if (optimization.notes === "无实质改动") return false;
  return optimization.optimized.trim() !== normalized;
}

export const PROMPT_OPTIMIZER_MAX_CHARS = 8_000;

/** 输入长度校验；返回中文错误或 null。与后端限制一致。 */
export function validatePromptDraft(draft: string): string | null {
  if (!draft.trim()) return "草稿为空，无需优化";
  if (draft.length > PROMPT_OPTIMIZER_MAX_CHARS) {
    return `草稿过长（最多 ${PROMPT_OPTIMIZER_MAX_CHARS} 字）`;
  }
  return null;
}

/** UI 预览（?ui-preview=1）假数据。 */
export const PROMPT_OPTIMIZATION_PREVIEW: PromptOptimization = {
  optimized:
    "目标：为登录页补充表单校验。\n要求：\n1. 邮箱与密码为必填，格式错误时在字段下方提示；\n2. 提交失败保留用户输入。\n验收：手动提交空表单与错误格式均出现提示，正确凭据可登录。",
  notes: "补齐了交付物与验收标准，把两条并列要求拆分编号",
};
