import { Fragment, useId, useState, type ReactNode } from "react";
import { ArrowSquareOut, CaretDown, CaretUp, CheckCircle, CircleNotch, Clock, ListBullets, PauseCircle, Stop, WarningCircle, XCircle } from "@phosphor-icons/react";
import { activityDuration, secondsSince, type MemberActivity } from "../conversationActivity";

interface Props { activity: MemberActivity; now: number; children: ReactNode; onOpen?: () => void; onStop?: () => void; stopDisabled: boolean; agentName: string; completedLabel?: string; compactHeading?: ReactNode; compactActions?: ReactNode }
export function ConversationMemberActivity({activity,now,children,onOpen,onStop,stopDisabled,agentName,completedLabel,compactHeading,compactActions}:Props) {
  const [expanded,setExpanded] = useState(false);
  const id = useId();
  const {phase,live,stale} = activity;
  const active = live && !stale && ["analysis","reply","read","search","web","command","edit","tool"].includes(phase);
  const Icon = phase === "completed" ? CheckCircle : phase === "failed" ? XCircle : ["approval","unconfirmed"].includes(phase) ? WarningCircle : phase === "cancelled" ? PauseCircle : active ? CircleNotch : Clock;
  const compact = Boolean(compactHeading);
  const Summary = compact ? "header" : "div";
  const toggleLabel = expanded ? "收起活动" : "查看活动";
  const toggle = (live || expanded || activity.lastActivityAt != null || onOpen) && (
    <button type="button" className="member-activity-toggle" aria-label={compact ? toggleLabel : undefined} title={compact ? toggleLabel : undefined} aria-expanded={expanded} aria-controls={id} onClick={()=>setExpanded(value=>!value)}>
      {compact ? <ListBullets size={17} aria-hidden="true" /> : <>{toggleLabel}{expanded ? <CaretUp size={16}/> : <CaretDown size={16}/>}</>}
    </button>
  );
  return <div className="member-activity" data-phase={phase} data-stale={stale} data-compact={compact || undefined}>
    <Summary className="member-activity-summary" aria-live="off">
      {compactHeading}
      <div className="member-activity-state" title={compact ? activity.label : undefined}><Icon size={compact ? 16 : 21} className={active ? "member-activity-spin" : undefined} aria-hidden="true"/><span className={compact ? "member-activity-status-label" : undefined}><span role="status" aria-live="polite">{stale && ["analysis","reply","read","search","web","command","edit","tool"].includes(phase) ? "等待新活动" : phase === "completed" ? completedLabel ?? activity.label : activity.label}</span>{live && <span aria-live="off"> · {activityDuration(activity.elapsed)}</span>}</span></div>
      {live && <Fragment><span className="member-activity-divider" aria-hidden="true"/><span className="member-activity-recency">{stale ? "45秒未收到新活动，需核实" : activity.lastActivityAt == null ? "尚未收到活动反馈" : `最近活动：${activityDuration(secondsSince(now,activity.lastActivityAt))}前`}</span></Fragment>}
      {compact ? <div className="member-activity-compact-actions">{toggle}{compactActions}</div> : toggle}
    </Summary>
    {expanded && <div className="member-activity-details" id={id}>
      <ol aria-label="成员活动时间线">{activity.steps.map(step=><li key={step.id} data-status={step.status}><span className="member-activity-track" aria-hidden="true"/>{step.status === "running" && active ? <CircleNotch className="member-activity-spin" size={20}/> : step.status === "failed" ? <XCircle size={20}/> : ["pending","stopped","running"].includes(step.status) ? <Clock size={20}/> : <CheckCircle weight="fill" size={20}/>}<time dateTime={new Date(step.at).toISOString()}>{new Date(step.at).toLocaleTimeString("zh-CN",{hour12:false,hour:"2-digit",minute:"2-digit",second:"2-digit"})}</time><span className="member-activity-step-label">{step.label}</span>{step.id.startsWith("tool:") && <small>{{completed:"已完成",running:phase === "approval" ? "等待确认" : active ? "进行中" : live ? "待核实" : "已结束",pending:live ? "等待执行" : "已结束",failed:"失败",stopped:"已结束"}[step.status]}</small>}</li>)}</ol>
      <div className="member-activity-foot" aria-live="off"><span>{phase === "unconfirmed" ? "原进程停止状态待核实" : live ? activity.heartbeat === "normal" ? "调度心跳正常" : activity.heartbeat === "stale" ? "调度心跳较久未更新" : "尚无调度心跳记录" : phase === "approval" ? "Agent 已返回，等待你确认" : "本次运行已结束"} · {activity.lastActivityAt == null ? "尚未收到 Agent 活动" : `Agent 最近活动 ${activityDuration(secondsSince(live ? now : activity.steps[activity.steps.length-1].at,activity.lastActivityAt))}前`}</span><div>{onOpen && <button type="button" aria-label={`查看${agentName}的原会话`} onClick={onOpen}><ArrowSquareOut size={17}/>查看原会话</button>}{onStop && <button type="button" className="member-activity-stop" aria-label="停止本轮团队回复" title="停止本轮团队回复，后续成员不再继续发言" disabled={stopDisabled} onClick={onStop}>停止</button>}</div></div>
    </div>}
    {children}
  </div>;
}
