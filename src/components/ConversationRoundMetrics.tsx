import { useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CaretDown, CaretRight } from "@phosphor-icons/react";
import { useTranslation } from "../useInterfaceLanguage";
import "../styles/conversation-round-metrics.css";

type Count = number | null;
interface Metrics {
  starts: Count;
  repeatDispatches: Count;
  tokens: { inputTotal: Count; cacheRead: Count; output: Count };
}
interface Report {
  totals: Metrics;
  tasks: { taskId: string; memberId: string; title: string; member: string; metrics: Metrics }[];
  unstartedMembers: string[];
}

function MetricValues({ metrics }: { metrics: Metrics }) {
  const t = useTranslation();
  const fields: [string, Count][] = [
    ["实际启动", metrics.starts], ["重复派发", metrics.repeatDispatches],
    ["输入 token", metrics.tokens.inputTotal], ["缓存读取 token", metrics.tokens.cacheRead],
    ["输出 token", metrics.tokens.output],
  ];
  return <dl className="conversation-round-metric-values">{fields.map(([label, value]) =>
    <div key={label}><dt>{t(label)}</dt><dd>{value == null ? t("未知") : value.toLocaleString()}</dd></div>
  )}</dl>;
}

export function ConversationRoundMetrics({ projectId, roundId, updatedAt }: {
  projectId: string; roundId: string; updatedAt: number;
}) {
  const t = useTranslation();
  const region = useId();
  const [expanded, setExpanded] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [result, setResult] = useState<{ key: string; report?: Report; error?: string } | null>(null);
  const key = JSON.stringify([projectId, roundId, refresh]);
  useEffect(() => {
    if (!expanded) return;
    let current = true;
    invoke<Report>("ma_conversation_round_metrics", { projectId, roundId }).then(report => {
      if (current) setResult({ key, report });
    }, error => {
      if (current) setResult({ key, error: String(error) });
    });
    return () => { current = false; };
  }, [expanded, key, projectId, roundId, updatedAt]);
  const shown = result?.key === key ? result : null;
  const report = shown?.report;
  return <div className="conversation-round-metrics">
    <button className="conversation-round-metrics-toggle" type="button" aria-expanded={expanded} aria-controls={region}
      onClick={() => setExpanded(value => !value)}>
      {expanded ? <CaretDown size={12} /> : <CaretRight size={12} />}{t("本轮用量")}
    </button>
    {expanded && <section id={region} aria-label={t("本轮用量")}>
      {shown?.error ? <p role="alert">{t("无法读取本轮用量")} · {shown.error}</p>
        : report ? <>
          <MetricValues metrics={report.totals} />
          {report.tasks.length > 0 && <details className="conversation-round-metric-tasks">
            <summary>{t("按任务查看")}</summary>
            <ul>{report.tasks.map(task =>
              <li key={JSON.stringify([task.taskId, task.memberId])}><strong>{task.member} · {t(task.title)}</strong><MetricValues metrics={task.metrics} /></li>
            )}</ul>
          </details>}
          {report.unstartedMembers.length > 0 && <p>{t("本轮未启动的成员")} · {report.unstartedMembers.join("、")}</p>}
          <p>{t("输入包含缓存读写。重复派发按同一成员再次处理同一消息或任务计数，包含重试。缺少运行时用量时显示未知。")}</p>
        </> : <p role="status">{t("正在读取用量…")}</p>}
      {shown && <button type="button" className="conversation-round-metrics-toggle" onClick={() => setRefresh(value => value+1)}>{t("刷新用量")}</button>}
    </section>}
  </div>;
}
