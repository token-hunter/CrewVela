export const NOTIFICATION_SETTINGS_STORAGE_KEY = "crewvela-notification-settings-v1";
export const NOTIFICATION_SETTINGS_EVENT = "crewvela:notification-settings";

export type NotificationSound = "system" | "soft" | "bell" | "none";

export interface NotificationSettings {
  enabled: boolean;
  taskComplete: boolean;
  taskFailed: boolean;
  waitingForUser: boolean;
  sound: NotificationSound;
  volume: number;
  suppressWhenFocused: boolean;
}

export const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  enabled: true,
  taskComplete: true,
  taskFailed: true,
  waitingForUser: true,
  sound: "soft",
  volume: 65,
  suppressWhenFocused: true,
};

export function loadNotificationSettings(): NotificationSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(NOTIFICATION_SETTINGS_STORAGE_KEY) ?? "null") as Partial<NotificationSettings> | null;
    if (!saved) return DEFAULT_NOTIFICATION_SETTINGS;
    return {
      enabled: saved.enabled ?? DEFAULT_NOTIFICATION_SETTINGS.enabled,
      taskComplete: saved.taskComplete ?? DEFAULT_NOTIFICATION_SETTINGS.taskComplete,
      taskFailed: saved.taskFailed ?? DEFAULT_NOTIFICATION_SETTINGS.taskFailed,
      waitingForUser: saved.waitingForUser ?? DEFAULT_NOTIFICATION_SETTINGS.waitingForUser,
      suppressWhenFocused: saved.suppressWhenFocused ?? DEFAULT_NOTIFICATION_SETTINGS.suppressWhenFocused,
      sound: saved.sound ?? DEFAULT_NOTIFICATION_SETTINGS.sound,
      volume: Math.max(0, Math.min(100, Number(saved.volume ?? 65))),
    };
  } catch {
    return DEFAULT_NOTIFICATION_SETTINGS;
  }
}

export function saveNotificationSettings(settings: NotificationSettings): void {
  localStorage.setItem(NOTIFICATION_SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  window.dispatchEvent(new CustomEvent(NOTIFICATION_SETTINGS_EVENT, { detail: settings }));
}

export function playNotificationSound(settings = loadNotificationSettings()): void {
  if (!settings.enabled || settings.sound === "none" || settings.sound === "system") return;
  try {
    const AudioContextClass = window.AudioContext ?? (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextClass) return;
    const context = new AudioContextClass();
    const gain = context.createGain();
    gain.gain.setValueAtTime(Math.max(0.01, settings.volume / 100) * 0.16, context.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, context.currentTime + 0.42);
    gain.connect(context.destination);
    const notes = settings.sound === "bell" ? [880, 1174] : [523, 659];
    notes.forEach((frequency, index) => {
      const oscillator = context.createOscillator();
      oscillator.type = settings.sound === "bell" ? "sine" : "triangle";
      oscillator.frequency.value = frequency;
      oscillator.connect(gain);
      oscillator.start(context.currentTime + index * 0.09);
      oscillator.stop(context.currentTime + 0.34 + index * 0.09);
    });
    window.setTimeout(() => void context.close(), 700);
  } catch {
    // 声音播放失败不影响系统通知。
  }
}
