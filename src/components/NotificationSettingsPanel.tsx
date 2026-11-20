import { useEffect, useState } from "react";
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
interface Props {
  settings: NotificationSettings;
  onChange: (settings: NotificationSettings) => void;
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
  const [categoryFilter, setCategoryFilter] = useState<"all" | NotificationHistoryRecord["category"]>("all");
  const filteredRecords = categoryFilter === "all"
    ? records
    : records.filter((item) => item.category === categoryFilter);

  return (
    <section className="notification-history-panel">
      <header>
        <div><strong>通知历史</strong><small>完整消息持久化保存在本机，默认保留30天。</small></div>
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
        <EmptyState className="notification-history-empty" icon={<BellRingingIcon size={24} />} title="暂无通知" description="任务状态和测试通知会显示在这里。" />
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

export function NotificationSettingsPanel({ settings, onChange }: Props) {
  const [activeView, setActiveView] = useState<NotificationSettingsView>("notifications");
  const [history, setHistory] = useState<NotificationHistoryRecord[]>([]);
  const [feedback, setFeedback] = useState<{ error: boolean; text: string } | null>(null);
  const openSource = (url: string) => {
    setFeedback(null);
    void openUrl(url).catch(reason => setFeedback({ error: true, text: `打开来源失败：${String(reason)}` }));
  };
  const updateHistory = async (action: () => Promise<NotificationHistoryRecord[]>) => {
    setFeedback(null);
    try { setHistory(await action()); }
    catch (reason) { setFeedback({ error: true, text: `更新通知历史失败：${String(reason)}` }); }
  };
  const testNotice = async () => {
    setFeedback(null);
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

  return (
    <div className="notification-settings-panel">
      <div className="notification-view-tabs" role="tablist" aria-label="通知功能">
        <button type="button" role="tab" aria-selected={activeView === "notifications"} className={activeView === "notifications" ? "active" : ""} onClick={() => setActiveView("notifications")}>通知设置</button>
        <button type="button" role="tab" aria-selected={activeView === "history"} className={activeView === "history" ? "active" : ""} onClick={() => setActiveView("history")}>通知历史{unreadCount > 0 && <span className="notification-unread-count">{unreadCount}</span>}</button>
      </div>

      {feedback && <p role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
      {activeView === "history" ? (
        <NotificationHistoryView
          records={visibleHistory}
          onReadAll={() => void updateHistory(markAllNotificationsRead)}
          onClear={() => void updateHistory(clearNotificationHistory)}
          onOpenUrl={openSource}
        />
      ) : <>
        <section className="notification-card notification-master-card">
          <ToggleRow tone="master" title="启用通知" description={`为后台任务和需要人工处理的事件发送${IS_MAC ? "系统" : " Windows "}通知。`} checked={settings.enabled} onChange={(enabled) => update({ enabled })} />
          <div className="notification-divider" />
          <ToggleRow tone="success" title="Agent 任务完成" description="会话执行结束时提醒。" checked={settings.taskComplete} disabled={!settings.enabled} onChange={(taskComplete) => update({ taskComplete })} />
          <ToggleRow tone="danger" title="执行失败与审核失败" description="任务失败或审核未通过时提醒。" checked={settings.taskFailed} disabled={!settings.enabled} onChange={(taskFailed) => update({ taskFailed })} />
          <ToggleRow tone="warn" title="等待审批或人工输入" description="Agent 暂停并等待你处理时提醒。" checked={settings.waitingForUser} disabled={!settings.enabled} onChange={(waitingForUser) => update({ waitingForUser })} />
        </section>

        <section className="notification-card notification-sound-card">
          <div><strong>通知声音</strong><small>选择 CrewVela 弹出通知时播放的提示音。</small></div>
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
