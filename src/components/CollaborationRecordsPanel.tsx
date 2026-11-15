import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EnvelopeIcon } from "@phosphor-icons/react/dist/csr/Envelope";
import { ListBulletsIcon } from "@phosphor-icons/react/dist/csr/ListBullets";
import { PlayCircleIcon } from "@phosphor-icons/react/dist/csr/PlayCircle";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";
import { buildCollaborationRecords } from "../collaborationRecords";
import type { DeliveryRecord, MultiAgentSnapshot } from "../types";
import "../styles/collaboration-records.css";

function dayLabel(at: number): string {
  const date = new Date(at);
  const label = date.toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
  return date.toDateString() === new Date().toDateString() ? `今天 · ${label}` : date.toLocaleDateString("zh-CN");
}

export function CollaborationRecordsPanel({ snapshot, preview = false }: { snapshot: MultiAgentSnapshot; preview?: boolean }) {
  const [deliveries, setDeliveries] = useState<DeliveryRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!preview);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("全部");
  const [expanded, setExpanded] = useState(() => new Set(buildCollaborationRecords(snapshot, [])[0]?.id ? [buildCollaborationRecords(snapshot, [])[0].id] : []));
  const alive = useRef(false);
  const inFlight = useRef(false);
  const loadDeliveries = useCallback(async () => {
    if (preview || inFlight.current) return;
    inFlight.current = true;
    setLoading(true);
    try {
      const next = await invoke<DeliveryRecord[]>("list_delivery_records");
      if (alive.current) { setDeliveries(next); setError(null); }
    } catch (reason) {
      if (alive.current) setError(`投递记录加载失败：${String(reason)}`);
    } finally {
      inFlight.current = false;
      if (alive.current) setLoading(false);
    }
  }, [preview]);
  useEffect(() => {
    alive.current = true;
    if (!preview) void loadDeliveries();
    const timer = preview ? undefined : window.setInterval(() => void loadDeliveries(), 3000);
    return () => { alive.current = false; window.clearInterval(timer); };
  }, [loadDeliveries, preview]);

  const records = useMemo(() => buildCollaborationRecords(snapshot, deliveries), [snapshot, deliveries]);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filtered = records.filter(item => (kind === "全部" || item.kind === kind)
    && `${item.title}\n${item.body}\n${item.details ?? ""}\n${item.kind}`.toLocaleLowerCase().includes(normalizedQuery));
  const groups = new Map<string, typeof filtered>();
  for (const item of filtered) {
    const day = dayLabel(item.at);
    const group = groups.get(day) ?? [];
    group.push(item); groups.set(day, group);
  }
  return <section className="collab-records" aria-label="协作记录">
    <div className="collab-records-controls">
      <input type="search" aria-label="搜索记录" placeholder="搜索记录" value={query} onChange={event => setQuery(event.target.value)} />
      <label>类型：<select aria-label="记录类型" value={kind} onChange={event => setKind(event.target.value)}>
        {["全部", "消息", "事件", "运行", "直通", "直通·广播", "系统", "交付", "经验"].map(value => <option key={value}>{value}</option>)}
      </select></label>
    </div>
    {error && <div className="collab-records-error" role="alert">{error}<button type="button" disabled={loading} onClick={() => void loadDeliveries()}>重试</button></div>}
    {loading && !deliveries.length && <p className="collab-records-loading" role="status">正在读取投递记录…</p>}
    {[...groups].map(([day, items]) => <section className="collab-records-day" key={day}>
      <h3>{day}</h3>
      <div className="collab-records-timeline">
        {items.map(item => {
          const Icon = item.kind === "运行" ? PlayCircleIcon : ["消息", "直通", "直通·广播"].includes(item.kind) ? EnvelopeIcon : ListBulletsIcon;
          const open = expanded.has(item.id);
          return <article className="collab-records-item" key={item.id}>
            <button type="button" className="collab-records-row" aria-expanded={open} onClick={() => setExpanded(current => {
              const next = new Set(current); if (next.has(item.id)) next.delete(item.id); else next.add(item.id); return next;
            })}>
              <time dateTime={new Date(item.at).toISOString()}>{new Date(item.at).toLocaleTimeString("zh-CN", { hour12: false })}</time>
              <Icon size={18} aria-hidden="true" /><span className="collab-records-kind">{item.kind}</span>
              <span className="collab-records-text"><strong>{item.title}</strong><span>{item.body}</span></span>
              <CaretDownIcon size={14} aria-hidden="true" className={open ? "expanded" : ""} />
            </button>
            {open && <pre className="collab-records-body">{item.body || "此记录无正文"}{item.details ? `\n\n${item.details}` : ""}</pre>}
          </article>;
        })}
      </div>
    </section>)}
    {!filtered.length && <p className="collab-empty">{records.length ? "没有符合条件的记录" : "暂无协作记录"}</p>}
  </section>;
}
