import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { BellRingingIcon, CheckIcon, TrashIcon } from "@phosphor-icons/react";
import { deliverDesktopNotice, normalizeNotificationTitle } from "../notificationCenter";
import { IS_MAC } from "../platform";
import { Switch } from "./ui/Switch";
import { EmptyState } from "./ui/EmptyState";
import {
  clearNotificationHistory,
  loadNotificationHistory,
  markAllNotificationsRead,
  NOTIFICATION_HISTORY_EVENT,
  type NotificationHistoryRecord,
} from "../notificationHistory";
import type { NotificationSettings } from "../notificationSettings";
import type { NotificationSettingsStyle } from "../types";
import "../styles/settings-creative-modes.css";
interface Props {
  settings: NotificationSettings;
  onChange: (settings: NotificationSettings) => void;
  viewStyle?: NotificationSettingsStyle;
}

type NotificationSettingsView = "notifications" | "history";

function ToggleRow({ title, description, checked, disabled, onChange, tone }: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  tone?: "master" | "success" | "danger" | "warn";
}) {
  return (
    <label className={`notification-toggle-row${disabled ? " disabled" : ""}${tone ? ` tone-${tone}` : ""}`}>
      <span className="notification-toggle-copy">
        {tone && tone !== "master" && <i className={`notification-tone-dot ${tone}`} aria-hidden="true" />}
        <span><strong>{title}</strong><small>{description}</small></span>
      </span>
      <Switch wrapper="span" className="pet-switch" checked={checked} disabled={disabled} onChange={onChange} />
    </label>
  );
}

function categoryLabel(category: NotificationHistoryRecord["category"]): string {
  return ({ task: "任务", intervention: "人工处理", terminal: "终端", quota: "额度", test: "测试", general: "通知" })[category];
}

const HISTORY_FILTERS: Array<{ id: "all" | NotificationHistoryRecord["category"]; label: string }> = [
  { id: "all", label: "全部" },
  { id: "intervention", label: "人工处理" },
  { id: "task", label: "任务" },
  { id: "terminal", label: "终端" },
  { id: "test", label: "测试" },
];

function NotificationHistoryView({ records, onReadAll, onClear, onOpenUrl }: {
  records: NotificationHistoryRecord[];
  onReadAll: () => void;
  onClear: () => void;
  onOpenUrl: (url: string) => void;
}) {
  const t = useTranslation();
  const [categoryFilter, setCategoryFilter] = useState<"all" | NotificationHistoryRecord["category"]>("all");
  const filteredRecords = categoryFilter === "all"
    ? records
    : records.filter((item) => item.category === categoryFilter);

  return (
    <section className="notification-history-panel">
      <header>
        <div><strong>{t("通知历史")}</strong><small>完整消息持久化保存在本机，默认保留30天。</small></div>
        <div>
          <button className="btn ghost" disabled={!records.some((item) => !item.read)} onClick={onReadAll}><CheckIcon size={14} />全部已读</button>
          <button className="btn ghost" disabled={records.length === 0} onClick={onClear}><TrashIcon size={14} />清空记录</button>
        </div>
      </header>
      {records.length > 0 && (
        <div className="notification-history-filters" role="group" aria-label="通知分类筛选">
          {HISTORY_FILTERS.map((filter) => {
            const count = filter.id === "all" ? records.length : records.filter((r) => r.category === filter.id).length;
            if (filter.id !== "all" && count === 0) return null;
            return (
              <button
                key={filter.id}
                type="button"
                className={categoryFilter === filter.id ? "active" : ""}
                aria-pressed={categoryFilter === filter.id}
                onClick={() => setCategoryFilter(filter.id)}
              >
                {filter.label}
                <em>{count}</em>
              </button>
            );
          })}
        </div>
      )}
      {records.length === 0 ? (
        <EmptyState className="notification-history-empty" icon={<BellRingingIcon size={24} />} title={t("暂无通知")} description="任务状态和测试通知会显示在这里。" />
      ) : filteredRecords.length === 0 ? (
        <EmptyState className="notification-history-empty" icon={<BellRingingIcon size={24} />} title="该分类下暂无通知" description="切换到“全部”可查看其他类别的历史通知。" />
      ) : (
        <div className="notification-history-list">
          {filteredRecords.map((record) => (
            <article key={record.id} className={record.read ? "" : "unread"} data-category={record.category}>
              <span className="notification-history-dot" aria-hidden="true" />
              <div>
                <header><span className={`notification-category-badge ${record.category}`}>{categoryLabel(record.category)}</span><time>{new Date(record.createdAt).toLocaleString()}</time></header>
                <strong>{normalizeNotificationTitle(record.title)}</strong>
                <p>{record.body}</p>
                {record.url && <button type="button" onClick={() => onOpenUrl(record.url!)}>查看来源</button>}
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

const EQ_BAR_WEIGHTS = [0.46, 0.72, 0.58, 0.92, 0.68, 0.86, 0.52, 0.96, 0.76, 0.62, 0.88, 0.7, 0.54, 0.42];

export function NotificationSettingsPanel({ settings, onChange, viewStyle = "classic" }: Props) {
  const t = useTranslation();
  const [activeView, setActiveView] = useState<NotificationSettingsView>("notifications");
  const [history, setHistory] = useState<NotificationHistoryRecord[]>([]);
  const [feedback, setFeedback] = useState<{ error: boolean; text: string } | null>(null);
  const [eqPlaying, setEqPlaying] = useState(false);
  const animTimer = useRef<number | null>(null);

  useEffect(() => () => {
    if (animTimer.current !== null) window.clearTimeout(animTimer.current);
  }, []);

  const openSource = (url: string) => {
    setFeedback(null);
    void openUrl(url).catch(reason => setFeedback({ error: true, text: `打开来源失败：${String(reason)}` }));
  };
  const updateHistory = async (action: () => Promise<NotificationHistoryRecord[]>) => {
    setFeedback(null);
    try { setHistory(await action()); }
    catch (reason) { setFeedback({ error: true, text: `更新通知历史失败：${String(reason)}` }); }
  };
  const testNotice = async (source: "default" | "mixer" = "default") => {
    setFeedback(null);
    if (source === "mixer") {
      setEqPlaying(true);
      if (animTimer.current !== null) window.clearTimeout(animTimer.current);
      animTimer.current = window.setTimeout(() => setEqPlaying(false), 1500);
    }
    try {
      const shown = await deliverDesktopNotice({ title: "CrewVela · 测试通知", body: "这是一条测试通知，完整内容已保存在通知历史中。", force: true, category: "test" });
      setFeedback(shown ? { error: false, text: "测试通知已发送。" } : { error: true, text: "通知已记录，但系统未显示弹窗。请检查通知权限和系统设置。" });
    } catch (reason) { setFeedback({ error: true, text: `测试通知失败：${String(reason)}` }); }
  };

  const update = (patch: Partial<NotificationSettings>) => onChange({ ...settings, ...patch });
  useEffect(() => {
    void loadNotificationHistory().then(setHistory);
    const historyListener = (event: Event) => setHistory((event as CustomEvent<NotificationHistoryRecord[]>).detail);
    window.addEventListener(NOTIFICATION_HISTORY_EVENT, historyListener);
    return () => {
      window.removeEventListener(NOTIFICATION_HISTORY_EVENT, historyListener);
    };
  }, []);

  const visibleHistory = history.filter((item) => item.category !== "quota");
  const unreadCount = visibleHistory.filter((item) => !item.read).length;

  const mixerActive = settings.enabled && settings.sound !== "none";

  return (
    <div className="notification-settings-panel">
      <div className="notification-view-tabs" role="tablist" aria-label="通知功能">
        <button type="button" role="tab" aria-selected={activeView === "notifications"} className={activeView === "notifications" ? "active" : ""} onClick={() => setActiveView("notifications")}>{t("通知设置")}</button>
        <button type="button" role="tab" aria-selected={activeView === "history"} className={activeView === "history" ? "active" : ""} onClick={() => setActiveView("history")}>{t("通知历史")}{unreadCount > 0 && <span className="notification-unread-count">{unreadCount}</span>}</button>
      </div>

      {feedback && <p role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
      {activeView === "history" ? (
        <NotificationHistoryView
          records={visibleHistory}
          onReadAll={() => void updateHistory(markAllNotificationsRead)}
          onClear={() => void updateHistory(clearNotificationHistory)}
          onOpenUrl={openSource}
        />
      ) : viewStyle === "mixer" ? (
        <section className="noti-mixer-card" aria-label="桌面迷你调音台">
          <div className="noti-mixer-head">
            <div>
              <strong>桌面通知调音台</strong>
              <small>按事件通道调控提醒总闸、提示音推子与专注静默红灯。</small>
            </div>
            <div className="noti-mixer-actions">
              <button
                type="button"
                className={`noti-focus-pill${settings.suppressWhenFocused ? " active" : ""}`}
                disabled={!settings.enabled}
                aria-pressed={settings.suppressWhenFocused}
                onClick={() => update({ suppressWhenFocused: !settings.suppressWhenFocused })}
              >
                <span>{settings.suppressWhenFocused ? "● 专注静默中" : "○ 专注时抑制"}</span>
              </button>
              <button
                type="button"
                className="btn primary"
                disabled={!settings.enabled}
                onClick={() => void testNotice("mixer")}
              >
                <BellRingingIcon size={15} />
                <span>试听测试通知</span>
              </button>
            </div>
          </div>

          <div className="noti-mixer-meter">
            <div className="noti-eq-bars" aria-hidden="true">
              {EQ_BAR_WEIGHTS.map((weight, index) => {
                const effectiveVolume = settings.sound === "system" ? 72 : settings.volume;
                const heightPx = mixerActive
                  ? Math.max(8, Math.round((effectiveVolume / 100) * 44 * weight))
                  : 5;
                return (
                  <span
                    key={index}
                    className={`noti-eq-bar${eqPlaying && mixerActive ? " playing" : ""}`}
                    style={{
                      height: `${heightPx}px`,
                      background: mixerActive
                        ? index > 10
                          ? "var(--accent)"
                          : "var(--success)"
                        : "var(--border-strong)",
                      animationDelay: `${(index * 0.05).toFixed(2)}s`,
                    }}
                  />
                );
              })}
            </div>

            <div className="noti-fader-panel">
              <div className="noti-fader-row">
                <span>提示音通道</span>
                <select
                  aria-label={t("通知声音")}
                  value={settings.sound}
                  disabled={!settings.enabled}
                  onChange={(event) => update({ sound: event.target.value as NotificationSettings["sound"] })}
                >
                  <option value="system">系统默认</option>
                  <option value="soft">柔和双音</option>
                  <option value="bell">清脆铃声</option>
                  <option value="none">静音</option>
                </select>
              </div>
              <div className="noti-fader-row">
                <span>音量推子</span>
                <strong>{settings.sound === "none" ? "静音" : settings.sound === "system" ? t("跟随系统") : `${settings.volume}%`}</strong>
              </div>
              <input
                aria-label="通知音量"
                type="range"
                min="0"
                max="100"
                step="5"
                value={settings.volume}
                disabled={!settings.enabled || settings.sound === "none" || settings.sound === "system"}
                onChange={(event) => update({ volume: Number(event.target.value) })}
              />
            </div>
          </div>

          <div className="noti-channel-grid">
            {([
              {
                key: "enabled" as const,
                title: "总通道",
                sub: IS_MAC ? "系统桌面总闸" : "Windows 通知总闸",
                color: "var(--accent)",
                checked: settings.enabled,
                disabled: false,
                onToggle: (v: boolean) => update({ enabled: v }),
              },
              {
                key: "taskComplete" as const,
                title: "任务完成",
                sub: "会话执行结束提醒",
                color: "var(--success)",
                checked: settings.taskComplete,
                disabled: !settings.enabled,
                onToggle: (v: boolean) => update({ taskComplete: v }),
              },
              {
                key: "taskFailed" as const,
                title: "失败与审核",
                sub: "异常或未通过提醒",
                color: "var(--danger)",
                checked: settings.taskFailed,
                disabled: !settings.enabled,
                onToggle: (v: boolean) => update({ taskFailed: v }),
              },
              {
                key: "waitingForUser" as const,
                title: "等待审批",
                sub: "人工介入暂停提醒",
                color: "var(--warn, #e5a94c)",
                checked: settings.waitingForUser,
                disabled: !settings.enabled,
                onToggle: (v: boolean) => update({ waitingForUser: v }),
              },
            ]).map((channel) => (
              <div
                key={channel.key}
                className={`noti-channel-tile${channel.disabled ? " disabled" : ""}`}
              >
                <span
                  className="noti-channel-led"
                  aria-hidden="true"
                  style={{
                    background: channel.checked && !channel.disabled ? channel.color : "var(--border-strong)",
                    boxShadow: channel.checked && !channel.disabled ? `0 0 9px ${channel.color}` : "none",
                  }}
                />
                <div>
                  <strong>{channel.title}</strong>
                  <small>{channel.sub}</small>
                </div>
                <Switch
                  wrapper="span"
                  className="pet-switch"
                  label={channel.title}
                  checked={channel.checked}
                  disabled={channel.disabled}
                  onChange={channel.onToggle}
                />
              </div>
            ))}
          </div>
        </section>
      ) : <>
        <section className="notification-card notification-master-card">
          <ToggleRow tone="master" title="启用通知" description={`为后台任务和需要人工处理的事件发送${IS_MAC ? "系统" : " Windows "}通知。`} checked={settings.enabled} onChange={(enabled) => update({ enabled })} />
          <div className="notification-divider" />
          <ToggleRow tone="success" title="Agent 任务完成" description="会话执行结束时提醒。" checked={settings.taskComplete} disabled={!settings.enabled} onChange={(taskComplete) => update({ taskComplete })} />
          <ToggleRow tone="danger" title="执行失败与审核失败" description="任务失败或审核未通过时提醒。" checked={settings.taskFailed} disabled={!settings.enabled} onChange={(taskFailed) => update({ taskFailed })} />
          <ToggleRow tone="warn" title="等待审批或人工输入" description="Agent 暂停并等待你处理时提醒。" checked={settings.waitingForUser} disabled={!settings.enabled} onChange={(waitingForUser) => update({ waitingForUser })} />
        </section>

        <section className="notification-card notification-sound-card">
          <div><strong>{t("通知声音")}</strong><small>选择 CrewVela 弹出通知时播放的提示音。</small></div>
          <div className="notification-controls">
            <select value={settings.sound} disabled={!settings.enabled} onChange={(event) => update({ sound: event.target.value as NotificationSettings["sound"] })}>
              <option value="system">系统默认</option><option value="soft">柔和双音</option><option value="bell">清脆铃声</option><option value="none">静音</option>
            </select>
            <input aria-label="通知音量" type="range" min="0" max="100" step="5" value={settings.volume} disabled={!settings.enabled || settings.sound === "none" || settings.sound === "system"} onChange={(event) => update({ volume: Number(event.target.value) })} />
            <span>{settings.volume}%</span>
            <button className="btn secondary" disabled={!settings.enabled} onClick={() => void testNotice()}><BellRingingIcon size={15} />发送测试通知</button>
          </div>
          <div className="notification-divider" />
          <ToggleRow title="专注时抑制" description="CrewVela 位于前台时不显示普通会话通知。" checked={settings.suppressWhenFocused} disabled={!settings.enabled} onChange={(suppressWhenFocused) => update({ suppressWhenFocused })} />
        </section>

      </>}
    </div>
  );
}
