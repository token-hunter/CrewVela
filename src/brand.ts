export const PRODUCT_FULL_NAME = "CrewVela · 协帆";
export const PRODUCT_SUBTITLE = "多Agent协作工作台";

const NOTIFICATION_BRAND_PREFIX = /^CrewVela(?:\s*·\s*协帆)?(?:\s*·\s*|\s+|$)/i;

export function normalizeNotificationTitle(title: string): string {
  let normalized = title.trim();
  while (NOTIFICATION_BRAND_PREFIX.test(normalized)) {
    normalized = normalized.replace(NOTIFICATION_BRAND_PREFIX, "").trim();
  }
  return normalized || "通知";
}
