//! 自动化任务：时间触发（一次性/间隔/每天/每周/cron）+ 会话事件触发（完成/失败时），
//! 三种动作（起新会话/发给现有会话/广播）。
//! 调度 tick 由 pty_poll_all 驱动，执行在 lib.rs；本模块只管模型、持久化与调度推进。
//! 护栏：max_runs 限制总触发次数，连续失败 max_consecutive_failures 次自动熔断停用；
//! 每次执行写 recent_runs 历史（最多 20 条），UI 可见——无人值守烧 token 必须可审计。
//! 事件型触发一轮 poll 合并一次（一批会话同时完成只触发一次动作），无时间排程。

use std::path::PathBuf;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
mod execution;
#[cfg(test)]
mod execution_tests;
#[cfg(test)]
mod edit_tests;

pub const MAX_RECENT_RUNS: usize = 20;
pub const MAX_BODY_CHARS: usize = 32_000;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: u32 = 3;
const AUTOMATION_FILE_VERSION: u32 = 4;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    /// Manual request receipts outlive the visible history, preventing replay after trimming.
    #[serde(default)]
    pub request_receipts: std::collections::HashMap<String, String>,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub action: AutomationAction,
    /// None = 不限次数。
    pub max_runs: Option<u32>,
    pub max_consecutive_failures: u32,
    pub run_count: u32,
    pub consecutive_failures: u32,
    pub last_run_at: Option<u64>,
    pub next_run_at: Option<u64>,
    pub created_at: u64,
    #[serde(default)]
    pub recent_runs: Vec<AutomationRun>,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// 毫秒时间戳，到点触发一次。
    Once { at: u64 },
    /// 每隔 N 分钟；错过不补跑，触发后从当前时刻推进下一次。
    Interval { minutes: u32 },
    /// 每天本地时间 HH:MM。
    Daily { hour: u32, minute: u32 },
    /// 每周：weekdays 用 ISO 1=周一..7=周日，可多天（[1,2,3,4,5] 即工作日）。
    Weekly {
        weekdays: Vec<u32>,
        hour: u32,
        minute: u32,
    },
    /// cron 高级模式：5 段（分 时 日 月 周）自动补秒位，6/7 段原样使用。
    Cron { expr: String },
    /// 会话状态事件：任一会话执行完成/失败时触发；一轮 poll 的一批事件合并触发一次。
    OnSessionEvent { event: SessionEventKind },
    #[serde(rename_all = "camelCase")]
    OnSessionEventFiltered { event: SessionEventKind, runtime_kind: Option<crate::multi_agent::AgentRuntimeKind>, project_id: Option<String>, session_id: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    /// 会话执行完成（hook Stop / idle）。
    Completed,
    /// 会话失败（进程非零退出 / 启动失败）。
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationAction {
    #[serde(rename_all = "camelCase")]
    RunAgent { runtime_kind: crate::multi_agent::AgentRuntimeKind, #[serde(skip)] provider_id: Option<String>, #[serde(skip)] model: Option<String>, cwd: String, prompt: String },
    #[serde(rename_all = "camelCase")]
    SendAgent { runtime_kind: crate::multi_agent::AgentRuntimeKind, session_id: String, body: String, project_id: Option<String>, agent_id: Option<String> },
    #[serde(rename_all = "camelCase")]
    BroadcastAgent { runtime_kind: crate::multi_agent::AgentRuntimeKind, body: String },
    /// 用指定 provider 起一个新会话并投初始 prompt（默认模式，非 yolo/auto、无 worktree）。
    // 枚举级 rename_all 只作用于变体名；字段需按变体声明 camelCase 才与前端契约一致。
    #[serde(rename_all = "camelCase")]
    Launch {
        /// 兼容旧 automations.json 里已按 snake_case 持久化的条目。
        #[serde(alias = "provider_id")]
        provider_id: String,
        cwd: Option<String>,
        prompt: String,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
    },
    /// 给现有会话发消息（逐字投递，随主循环 flush 自动提交）。
    #[serde(rename_all = "camelCase")]
    Message {
        #[serde(alias = "session_id")]
        session_id: String,
        body: String,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
    },
    /// 广播给全部运行中的 claude 会话。
    Broadcast { body: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub retry_of: Option<String>,
    #[serde(default)]
    pub targets: Vec<AutomationTarget>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub stage: Option<String>,
    pub at: u64,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTarget {
    pub session_id: String,
    pub message_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub stage: String,
    pub detail: String,
}

pub fn is_active_stage(stage: &str) -> bool {
    matches!(stage, "dispatching" | "queued" | "submitted" | "received" | "running" | "waiting" | "stopping" | "unconfirmed")
}

fn new_run(now: u64, origin: &str, retry_of: Option<String>) -> AutomationRun {
    AutomationRun { id: uuid::Uuid::new_v4().to_string(), origin: origin.into(), retry_of,
        targets: Vec::new(), updated_at: now, stage: Some("dispatching".into()), at: now, ok: true,
        detail: "执行已登记，正在准备派发".into() }
}

fn trim_history(item: &mut Automation) {
    // Active and uncertain executions must remain inspectable even after many skipped ticks.
    while item.recent_runs.len() > MAX_RECENT_RUNS {
        let Some(index) = item.recent_runs.iter().position(|r| !is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed"))) else { break; };
        item.recent_runs.remove(index);
    }
}

fn settle_failure(item: &mut Automation, ok: bool) {
    if ok { item.consecutive_failures = 0; }
    else {
        item.consecutive_failures += 1;
        if item.consecutive_failures >= item.max_consecutive_failures { item.enabled = false; item.next_run_at = None; }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy { Skip, #[default] Allow }

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAutomation {
    pub name: String,
    pub trigger: AutomationTrigger,
    pub action: AutomationAction,
    pub max_runs: Option<u32>,
    pub max_consecutive_failures: Option<u32>,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
}

#[derive(Serialize, Deserialize)]
struct AutomationFile {
    version: u32,
    items: Vec<Automation>,
}

pub struct AutomationManager {
    items: Mutex<Vec<Automation>>,
    path: Option<PathBuf>,
    dispatching: Mutex<std::collections::HashSet<String>>,
}

/// 按本地日历寻找下一次触发；跳过 DST 不存在的时刻，重复时刻选严格晚于基准的最早一次。
fn next_calendar_after(
    weekdays: Option<&[u32]>,
    hour: u32,
    minute: u32,
    after_ms: u64,
    mut date: chrono::NaiveDate,
    mut resolve: impl FnMut(&chrono::NaiveDateTime) -> chrono::LocalResult<i64>,
) -> Option<u64> {
    use chrono::{Datelike, Days, LocalResult, NaiveTime};
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    // 两周覆盖每周单日调度遇到跳时后下一周的合法触发点。
    for _ in 0..15 {
        if weekdays.is_none_or(|days| days.contains(&date.weekday().number_from_monday())) {
            let candidates = match resolve(&date.and_time(time)) {
                LocalResult::Single(value) => [Some(value), None],
                LocalResult::Ambiguous(first, second) => [Some(first), Some(second)],
                LocalResult::None => [None, None],
            };
            if let Some(next) = candidates.into_iter().flatten()
                .filter_map(|value| u64::try_from(value).ok())
                .filter(|value| *value > after_ms).min() {
                return Some(next);
            }
        }
        date = date.checked_add_days(Days::new(1))?;
    }
    None
}

fn next_daily_after(hour: u32, minute: u32, after_ms: u64) -> Option<u64> {
    use chrono::{Local, TimeZone};
    let after = Local.timestamp_millis_opt(i64::try_from(after_ms).ok()?).single()?;
    next_calendar_after(None, hour, minute, after_ms, after.date_naive(), |candidate| {
        Local.from_local_datetime(candidate).map(|dt| dt.timestamp_millis())
    })
}

fn next_weekly_after(weekdays: &[u32], hour: u32, minute: u32, after_ms: u64) -> Option<u64> {
    use chrono::{Local, TimeZone};
    let after = Local.timestamp_millis_opt(i64::try_from(after_ms).ok()?).single()?;
    next_calendar_after(Some(weekdays), hour, minute, after_ms, after.date_naive(), |candidate| {
        Local.from_local_datetime(candidate).map(|dt| dt.timestamp_millis())
    })
}

/// 经典 cron 的星期为 0/7=周日、1=周一；cron crate 则为 1=周日、7=周六。
/// 先在经典编号中展开范围/步长，再映射，避免 */2 与包含 7 的范围改变含义。
fn normalize_standard_cron_weekdays(field: &str) -> Result<String> {
    fn ordinal(value: &str) -> Result<u32> {
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            return value.parse::<u32>().ok().filter(|day| *day <= 7)
                .ok_or_else(|| anyhow!("cron 星期必须在 0-7 之间（0 和 7 均为周日）"));
        }
        // 保留底层解析器支持的英文全称、缩写以及 Tues/Thurs 别名。
        match value.to_ascii_lowercase().as_str() {
            "tues" => Ok(2),
            "thurs" => Ok(4),
            _ => value.parse::<chrono::Weekday>().map(|day| day.num_days_from_sunday())
                .map_err(|_| anyhow!("cron 星期无效: {value}")),
        }
    }

    let mut selected = [false; 7];
    for part in field.split(',') {
        let (base, step) = match part.split_once('/') {
            Some((base, value)) => {
                let step = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
                    value.parse::<u32>().ok().filter(|step| *step > 0)
                } else { None };
                (base, Some(step.ok_or_else(|| anyhow!("cron 星期步长必须是正整数"))?))
            }
            None => (part, None),
        };
        let (start, end) = if matches!(base, "*" | "?") {
            (0, 6)
        } else if let Some((start, end)) = base.split_once('-') {
            (ordinal(start)?, ordinal(end)?)
        } else {
            let start = ordinal(base)?;
            (start, if step.is_some() { 7 } else { start })
        };
        if start > end {
            return Err(anyhow!("cron 星期范围起点不能晚于终点: {base}"));
        }
        for day in start..=end {
            if (day - start) % step.unwrap_or(1) == 0 {
                selected[(day % 7) as usize] = true;
            }
        }
    }
    Ok(selected.iter().enumerate().filter(|(_, selected)| **selected)
        .map(|(day, _)| (day + 1).to_string()).collect::<Vec<_>>().join(","))
}

/// 5 段经典 cron（分 时 日 月 周）补秒并转换星期；6/7 段保持既有 crate 语义。
fn normalize_cron_expr(expr: &str) -> Result<String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {} {}", fields[..4].join(" "), normalize_standard_cron_weekdays(fields[4])?)),
        6 | 7 => Ok(fields.join(" ")),
        _ => Err(anyhow!(
            "cron 表达式必须是 5 段（分 时 日 月 周）或 6/7 段（含秒/年），当前 {} 段",
            fields.len()
        )),
    }
}

fn next_cron_after(expr: &str, after_ms: u64) -> Option<u64> {
    cron_next_runs(expr, after_ms, 1).ok()?.into_iter().next()
}

fn parse_cron_schedules(expr: &str) -> Result<Vec<cron::Schedule>> {
    use std::str::FromStr;
    let normalized = normalize_cron_expr(expr)?;
    let parse = |source: &str| cron::Schedule::from_str(source)
        .map_err(|error| anyhow!("cron 表达式无效: {error}"));
    let schedule = parse(&normalized)?;
    let original: Vec<_> = expr.split_whitespace().collect();
    // 经典 5 段中，日和星期均显式受限时满足任一即可；以 */? 开头的
    // 通配字段（包括 */n）仍与另一个字段取交集。6/7 段沿用原有语义。
    let wildcard = |field: &str| field.starts_with('*') || field.starts_with('?');
    if original.len() != 5 || wildcard(original[2]) || wildcard(original[4]) {
        return Ok(vec![schedule]);
    }
    let mut day_fields: Vec<_> = normalized.split_whitespace().collect();
    let mut weekday_fields = day_fields.clone();
    day_fields[5] = "*";
    weekday_fields[3] = "*";
    Ok(vec![parse(&day_fields.join(" "))?, parse(&weekday_fields.join(" "))?])
}

fn cron_search_start(schedule: &cron::Schedule, after: &chrono::DateTime<chrono::Local>) -> Result<chrono::DateTime<chrono::Local>> {
    use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
    use cron::TimeUnitSpec;
    let Some(year) = schedule.years().iter().find(|year| i64::from(*year) >= i64::from(after.year())) else {
        return Ok(*after);
    };
    if i64::from(year) == i64::from(after.year()) { return Ok(*after); }
    // cron 0.12 跳过未包含的起始年份时不会重置月/日下界。例如从六月
    // 查询明年会漏掉明年一至五月。对齐到第一个可用年，再交给原迭代器。
    let first = NaiveDate::from_ymd_opt(year as i32, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| anyhow!("cron 年份无效"))?;
    // 跨年也可能出现时区跳时；只查找年份边界，不逐秒扫描调度候选。
    // 正常时区第一次即命中；上限覆盖跳过一整天的时区变更。
    for seconds in 0..172_800 {
        let local = first + Duration::seconds(seconds);
        if let Some(start) = Local.from_local_datetime(&local).earliest() {
            return start.checked_sub_signed(Duration::seconds(1))
                .ok_or_else(|| anyhow!("cron 年份边界超出时间范围"));
        }
    }
    Err(anyhow!("无法确定 cron 年份的本地时间边界"))
}

/// cron 预览：返回接下来的 count 个触发点；表达式非法或没有未来触发点时报错（供 UI 实时预览）。
pub fn cron_next_runs(expr: &str, after_ms: u64, count: usize) -> Result<Vec<u64>> {
    use chrono::{Local, TimeZone};
    let schedules = parse_cron_schedules(expr)?;
    let after = Local
        .timestamp_millis_opt(i64::try_from(after_ms).map_err(|_| anyhow!("时间基准无效"))?)
        .single()
        .ok_or_else(|| anyhow!("时间基准无效"))?;
    let mut iterators: Vec<_> = schedules.iter().map(|schedule| {
        cron_search_start(schedule, &after).map(|start| schedule.after(&start).peekable())
    }).collect::<Result<_>>()?;
    let mut runs = Vec::new();
    for _ in 0..count {
        let Some(next) = iterators.iter_mut().filter_map(|iter| iter.peek().cloned()).min() else { break; };
        // 合并两条递增序列；日与星期同一时刻命中时也只执行一次。
        for iter in &mut iterators {
            if iter.peek() == Some(&next) { iter.next(); }
        }
        runs.push(next.timestamp_millis() as u64);
    }
    if runs.is_empty() {
        return Err(anyhow!("该表达式没有未来的触发时间"));
    }
    Ok(runs)
}

/// 触发前的首次调度（创建/重新启用时调用）。事件型无时间调度，返回 None。
fn schedule_initial(trigger: &AutomationTrigger, now: u64) -> Option<u64> {
    match trigger {
        AutomationTrigger::Once { at } => Some(*at),
        AutomationTrigger::Interval { minutes } => Some(now + u64::from(*minutes) * 60_000),
        AutomationTrigger::Daily { hour, minute } => next_daily_after(*hour, *minute, now),
        AutomationTrigger::Weekly {
            weekdays,
            hour,
            minute,
        } => next_weekly_after(weekdays, *hour, *minute, now),
        AutomationTrigger::Cron { expr } => next_cron_after(expr, now),
        AutomationTrigger::OnSessionEvent { .. } | AutomationTrigger::OnSessionEventFiltered { .. } => None,
    }
}

/// 一次触发后的推进；返回 None 表示不再触发（一次性、事件型或已达 max_runs）。
fn advance_after_run(
    trigger: &AutomationTrigger,
    now: u64,
    run_count: u32,
    max_runs: Option<u32>,
) -> Option<u64> {
    if max_runs.is_some_and(|max| run_count >= max) {
        return None;
    }
    match trigger {
        AutomationTrigger::Once { .. } | AutomationTrigger::OnSessionEvent { .. } | AutomationTrigger::OnSessionEventFiltered { .. } => None,
        AutomationTrigger::Interval { minutes } => Some(now + u64::from(*minutes) * 60_000),
        AutomationTrigger::Daily { hour, minute } => next_daily_after(*hour, *minute, now),
        AutomationTrigger::Weekly {
            weekdays,
            hour,
            minute,
        } => next_weekly_after(weekdays, *hour, *minute, now),
        AutomationTrigger::Cron { expr } => next_cron_after(expr, now),
    }
}

/// v1 的下一次 cron 时间可能按旧星期编号/日交集计算。仅迁移待执行时间，
/// 不从 created_at/last_run_at 补算：用户重新启用时可能主动跳过过往排程。
fn migrate_legacy_cron_schedules(items: &mut [Automation], now: u64) {
    for item in items {
        if !item.enabled || item.max_runs.is_some_and(|max| item.run_count >= max) {
            continue;
        }
        let AutomationTrigger::Cron { expr } = &item.trigger else { continue; };
        let Some(next) = item.next_run_at else { continue; };
        // 未到期任务从当前时间重排，避免未来旧时间掩盖更早的新触发点。
        // 已到期任务从原待执行边界重排：合法时间原样保留；错误星期修正到
        // 边界后的首个合法时间，若它也已到期，首轮 tick 仍执行一次。
        let after = if next <= now { next.saturating_sub(1) } else { now };
        item.next_run_at = next_cron_after(expr, after);
    }
}

fn append_skipped(item: &mut Automation, now: u64) {
    let mut run = new_run(now, "schedule", None);
    run.stage = Some("skipped".into());
    run.detail = "上一轮尚未完成，本轮已跳过；未启动或排队新任务".into();
    item.recent_runs.push(run);
    trim_history(item);
}

impl AutomationManager {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
            path: None,
            dispatching: Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        Self::open_at_time(path, now_millis())
    }

    fn open_at_time(path: PathBuf, now: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建自动化配置目录失败: {}", parent.display()))?;
        }
        let (mut items, migrate) = match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let file = serde_json::from_str::<AutomationFile>(&raw)
                    .with_context(|| format!("解析自动化配置失败: {}", path.display()))?;
                if file.version > AUTOMATION_FILE_VERSION {
                    return Err(anyhow!("自动化配置版本 {} 高于当前支持的版本 {}，请升级应用；原文件未修改", file.version, AUTOMATION_FILE_VERSION));
                }
                (file.items, file.version < 2)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), false),
            Err(error) => {
                return Err(error).with_context(|| format!("读取自动化配置失败: {}", path.display()))
            }
        };
        if migrate { migrate_legacy_cron_schedules(&mut items, now); }
        let mut recovered = false;
        for item in &mut items {
            for run in &mut item.recent_runs {
                if run.id.is_empty() { run.id = uuid::Uuid::new_v4().to_string(); recovered = true; }
                if run.updated_at == 0 { run.updated_at = run.at; recovered = true; }
                if run.stage.is_none() && !run.ok {
                    run.stage=Some("failed".into()); recovered=true;
                } else if run.stage.as_deref().is_none_or(is_active_stage) {
                    run.stage = Some("unconfirmed".into());
                    run.detail = "应用已重启或旧记录缺少执行回执，结果未确认；不会自动重发".into();
                    recovered = true;
                }
            }
        }
        let manager = Self {
            items: Mutex::new(items),
            path: Some(path),
            dispatching: Mutex::new(std::collections::HashSet::new()),
        };
        if migrate || recovered { manager.persist(&manager.items.lock().unwrap())?; }
        Ok(manager)
    }

    fn persist(&self, snapshot: &[Automation]) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = AutomationFile {
            version: AUTOMATION_FILE_VERSION,
            items: snapshot.to_vec(),
        };
        let bytes = serde_json::to_vec(&file).context("序列化自动化配置失败")?;
        let temp = path.with_extension(format!("crewvela-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)
                .with_context(|| format!("创建自动化临时文件失败: {}", temp.display()))?;
            file.write_all(&bytes).context("写入自动化配置失败")?;
            drop(file);
            std::fs::rename(&temp, path)
                .with_context(|| format!("保存自动化配置失败: {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temp); }
        result
    }

    pub fn list(&self) -> Vec<Automation> {
        self.items.lock().unwrap().clone()
    }

    fn validate(input: &NewAutomation, now: u64) -> Result<()> {
        if input.name.trim().is_empty() {
            return Err(anyhow!("自动化名称不能为空"));
        }
        if input.name.chars().count() > 60 {
            return Err(anyhow!("自动化名称不能超过 60 个字符"));
        }
        let validate_clock = |hour: u32, minute: u32| -> Result<()> {
            if hour > 23 || minute > 59 {
                return Err(anyhow!("触发时间无效（小时 0-23，分钟 0-59）"));
            }
            Ok(())
        };
        match &input.trigger {
            AutomationTrigger::OnSessionEventFiltered { runtime_kind, project_id, session_id, .. } => {
                let count = usize::from(runtime_kind.is_some()) + usize::from(project_id.is_some()) + usize::from(session_id.is_some());
                if count != 1 || project_id.as_ref().is_some_and(|id| id.trim().is_empty()) || session_id.as_ref().is_some_and(|id| id.trim().is_empty()) {
                    return Err(anyhow!("请选择一个明确的事件来源"));
                }
                if runtime_kind.is_some_and(|kind| kind.is_retired()) { return Err(anyhow!("事件来源 Agent 不受支持")); }
            }
            AutomationTrigger::Interval { minutes } if !(1..=1440).contains(minutes) => {
                return Err(anyhow!("间隔分钟数必须在 1-1440 之间"));
            }
            AutomationTrigger::Daily { hour, minute } => validate_clock(*hour, *minute)?,
            AutomationTrigger::Weekly {
                weekdays,
                hour,
                minute,
            } => {
                validate_clock(*hour, *minute)?;
                if weekdays.is_empty() {
                    return Err(anyhow!("每周触发至少选择一天"));
                }
                if weekdays.iter().any(|day| !(1..=7).contains(day)) {
                    return Err(anyhow!("每周触发的星期值必须在 1（周一）到 7（周日）之间"));
                }
            }
            AutomationTrigger::Cron { expr } => {
                // 必须可解析且存在未来触发点（如「2 月 31 号」这种永不触发的要拦住）。
                cron_next_runs(expr, now, 1)?;
            }
            _ => {}
        }
        match &input.action {
            AutomationAction::RunAgent { runtime_kind, provider_id, model, cwd, .. } => {
                crate::automation_runtime::validate_launch(*runtime_kind, provider_id.as_deref(), model.as_deref(), cwd)?;
            }
            AutomationAction::SendAgent { runtime_kind, session_id, project_id, agent_id, .. } => {
                if runtime_kind.is_retired() { return Err(anyhow!("该 Agent 不受支持")); }
                if project_id.is_some() != agent_id.is_some() || (agent_id.is_none() && session_id.trim().is_empty()) { return Err(anyhow!("请选择有效的会话或项目成员")); }
            }
            AutomationAction::BroadcastAgent { runtime_kind, .. } if runtime_kind.is_retired() => return Err(anyhow!("该 Agent 不受支持")),
            _ => {}
        }
        let body = match &input.action {
            AutomationAction::RunAgent { prompt, .. } => prompt,
            AutomationAction::SendAgent { body, .. } | AutomationAction::BroadcastAgent { body, .. } => body,
            AutomationAction::Launch { prompt, .. } => prompt,
            AutomationAction::Message { body, .. } => body,
            AutomationAction::Broadcast { body } => body,
        };
        if body.trim().is_empty() {
            return Err(anyhow!("自动化的 Prompt/消息内容不能为空"));
        }
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(anyhow!("内容不能超过 {MAX_BODY_CHARS} 个字符"));
        }
        if input.max_runs.is_some_and(|max| max == 0) {
            return Err(anyhow!("最大触发次数至少为 1"));
        }
        if input
            .max_consecutive_failures
            .is_some_and(|max| !(1..=10).contains(&max))
        {
            return Err(anyhow!("熔断阈值必须在 1-10 之间"));
        }
        Ok(())
    }

    pub fn create(&self, input: NewAutomation, now: u64) -> Result<Automation> {
        Self::validate(&input, now)?;
        let automation = Automation {
            request_receipts: Default::default(),
            id: uuid::Uuid::new_v4().to_string(),
            name: input.name.trim().to_string(),
            enabled: true,
            next_run_at: schedule_initial(&input.trigger, now),
            trigger: input.trigger,
            action: input.action,
            max_runs: input.max_runs,
            max_consecutive_failures: input
                .max_consecutive_failures
                .unwrap_or(DEFAULT_MAX_CONSECUTIVE_FAILURES),
            run_count: 0,
            consecutive_failures: 0,
            last_run_at: None,
            created_at: now,
            recent_runs: Vec::new(),
            overlap_policy: input.overlap_policy,
            active_session_ids: Vec::new(),
        };
        let mut items = self.items.lock().unwrap();
        let mut updated = items.clone();
        updated.push(automation.clone());
        self.persist(&updated)?;
        *items = updated;
        Ok(automation)
    }

    /// Updates future configuration without replacing execution receipts or live targets.
    pub fn update(&self, id: &str, input: NewAutomation, now: u64) -> Result<Automation> {
        Self::validate(&input, now)?;
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        let item = snapshot.iter_mut().find(|item| item.id == id)
            .ok_or_else(|| anyhow!("未找到自动化任务: {id}"))?;
        let trigger_changed = serde_json::to_value(&item.trigger)? != serde_json::to_value(&input.trigger)?;
        if trigger_changed && matches!(&input.trigger, AutomationTrigger::Once { at } if *at <= now) {
            return Err(anyhow!("请将一次性任务的时间设为未来时间"));
        }
        let reschedule = trigger_changed || item.max_runs != input.max_runs;
        item.name = input.name.trim().into();
        item.trigger = input.trigger;
        item.action = input.action;
        item.max_runs = input.max_runs;
        item.max_consecutive_failures = input.max_consecutive_failures.unwrap_or(DEFAULT_MAX_CONSECUTIVE_FAILURES);
        item.overlap_policy = input.overlap_policy;
        if reschedule {
            item.next_run_at = if !item.enabled || item.max_runs.is_some_and(|max| item.run_count >= max) {
                None
            } else if matches!(&item.trigger, AutomationTrigger::Once { at } if *at <= now || item.last_run_at.is_some_and(|last| *at <= last)) {
                None
            } else {
                schedule_initial(&item.trigger, now)
            };
        }
        let updated = item.clone();
        self.persist(&snapshot)?;
        *items = snapshot;
        Ok(updated)
    }

    /// 启用时重新排程（避免按陈旧 next_run_at 立即补跑）；禁用即停在下一边界。
    pub fn set_enabled(&self, id: &str, enabled: bool, now: u64) -> Result<Automation> {
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        let updated = {
            let item = snapshot
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow!("未找到自动化任务: {id}"))?;
            item.enabled = enabled;
            item.next_run_at = if enabled {
                // 一次性已跑过的任务重新启用没有意义，保持 None。
                if matches!(item.trigger, AutomationTrigger::Once { at } if item.last_run_at.is_some_and(|last| at <= last)) {
                    None
                } else {
                    schedule_initial(&item.trigger, now)
                }
            } else {
                None
            };
            if enabled {
                item.consecutive_failures = 0;
            }
            item.clone()
        };
        self.persist(&snapshot)?;
        *items = snapshot;
        Ok(updated)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        let before = snapshot.len();
        if snapshot.iter().find(|item| item.id == id).is_some_and(|item| item.recent_runs.iter().any(|r| is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed")))) {
            return Err(anyhow!("仍有执行中或结果未确认的记录，请先核对或停止本轮，再删除计划"));
        }
        snapshot.retain(|item| item.id != id);
        if snapshot.len() == before {
            return Err(anyhow!("未找到自动化任务: {id}"));
        }
        self.persist(&snapshot)?;
        *items = snapshot;
        Ok(())
    }

    /// 到点任务出列：标记已触发并推进调度，返回待执行动作。错过的时间不补跑。
    pub fn collect_due(&self, now: u64) -> Result<Vec<(String, String, AutomationAction)>> {
        self.collect_due_with_busy(now, &std::collections::HashSet::new())
    }
    pub fn collect_due_with_busy(&self, now: u64, busy: &std::collections::HashSet<String>) -> Result<Vec<(String, String, AutomationAction)>> {
        let mut due = Vec::new();
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        let mut changed = false;
        for item in snapshot.iter_mut() {
            if !item.enabled
                || !item.next_run_at.is_some_and(|at| at <= now)
                || item.max_runs.is_some_and(|max| item.run_count >= max)
            {
                continue;
            }
            changed = true;
            if item.recent_runs.iter().filter(|r| is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed"))).count() >= MAX_RECENT_RUNS || (item.overlap_policy == OverlapPolicy::Skip && (busy.contains(&item.id) || self.dispatching.lock().unwrap().contains(&item.id))) {
                item.next_run_at = advance_after_run(&item.trigger, now, item.run_count, item.max_runs);
                append_skipped(item, now);
                continue;
            }
            item.run_count += 1;
            item.last_run_at = Some(now);
            item.next_run_at = advance_after_run(&item.trigger, now, item.run_count, item.max_runs);
            let run = new_run(now, "schedule", None);
            due.push((item.id.clone(), run.id.clone(), item.action.clone()));
            item.recent_runs.push(run); trim_history(item);
        }
        if changed {
            self.persist(&snapshot)?;
            *items = snapshot;
        }
        self.dispatching.lock().unwrap().extend(due.iter().map(|(id, _, _)| id.clone()));
        Ok(due)
    }

    /// 会话事件触发：一轮 poll 的一批同类事件合并为一次触发（10 个会话同时完成
    /// 只执行一次动作，不是 10 次）。max_runs 与熔断护栏与时间型一致。
    pub fn collect_event(
        &self,
        event: SessionEventKind,
        now: u64,
    ) -> Result<Vec<(String, String, AutomationAction)>> {
        self.collect_event_filtered(event, now, &[], &std::collections::HashSet::new())
    }
    pub fn collect_event_filtered(&self, event: SessionEventKind, now: u64, sources: &[crate::sessions::SessionRecord], busy: &std::collections::HashSet<String>) -> Result<Vec<(String, String, AutomationAction)>> {
        let mut changed = false;
        let mut fired = Vec::new();
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        for item in snapshot.iter_mut() {
            let matches = match &item.trigger {
                AutomationTrigger::OnSessionEvent { event: kind } => *kind == event,
                AutomationTrigger::OnSessionEventFiltered { event: kind, runtime_kind, project_id, session_id } => *kind == event && sources.iter().any(|s| {
                    runtime_kind.is_none_or(|kind| crate::automation_runtime::session_runtime(s) == Some(kind)) &&
                    project_id.as_ref().is_none_or(|id| s.multi_agent_project_id.as_ref() == Some(id) || s.project_id.as_ref() == Some(id)) &&
                    session_id.as_ref().is_none_or(|id| &s.id == id)
                }),
                _ => false,
            };
            if !item.enabled || !matches || item.max_runs.is_some_and(|max| item.run_count >= max) {
                continue;
            }
            changed = true;
            if item.recent_runs.iter().filter(|r| is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed"))).count() >= MAX_RECENT_RUNS || (item.overlap_policy == OverlapPolicy::Skip && (busy.contains(&item.id) || self.dispatching.lock().unwrap().contains(&item.id))) { append_skipped(item, now); continue; }
            item.run_count += 1;
            item.last_run_at = Some(now);
            let run = new_run(now, "event", None);
            fired.push((item.id.clone(), run.id.clone(), item.action.clone()));
            item.recent_runs.push(run); trim_history(item);
        }
        if changed {
            self.persist(&snapshot)?;
            *items = snapshot;
        }
        self.dispatching.lock().unwrap().extend(fired.iter().map(|(id, _, _)| id.clone()));
        Ok(fired)
    }

    /// 执行完成后回报结果：写历史（封顶 20 条），连续失败达阈值自动熔断停用。
    pub fn report_result(&self, id: &str, ok: bool, detail: String, now: u64) -> Result<()> {
        self.report_legacy_result(id, ok, detail, now)
    }
    pub fn report_dispatch_result(&self, id: &str, ok: bool, detail: String, now: u64, targets: Vec<String>) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut snapshot = items.clone();
        let Some(item) = snapshot.iter_mut().find(|item| item.id == id) else {
            return Ok(());
        };
        item.active_session_ids = targets;
        let mut run = new_run(now, "legacy", None);
        run.stage = Some(if ok { "queued" } else { "failed" }.into());
        run.ok = ok; run.detail = detail;
        item.recent_runs.push(run);
        trim_history(item);
        if !ok { settle_failure(item, false); }
        self.persist(&snapshot)?;
        *items = snapshot;
        self.dispatching.lock().unwrap().remove(id);
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_fields_deserialize_from_camelCase_contract() {
        // 前端契约（types.ts）：providerId / sessionId（camelCase）。
        let launch: AutomationAction =
            serde_json::from_str(r#"{"kind":"launch","providerId":"glm","cwd":null,"prompt":"hi"}"#)
                .unwrap();
        match launch {
            AutomationAction::Launch { provider_id, .. } => assert_eq!(provider_id, "glm"),
            other => panic!("unexpected variant: {other:?}"),
        }

        let message: AutomationAction =
            serde_json::from_str(r#"{"kind":"message","sessionId":"s1","body":"继续"}"#).unwrap();
        match message {
            AutomationAction::Message { session_id, body, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(body, "继续");
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // 序列化回前端也用 camelCase 键。
        let json = serde_json::to_value(AutomationAction::Launch {
            provider_id: "glm".into(),
            cwd: None,
            prompt: "hi".into(),
            project_id: None,
            agent_id: None,
        })
        .unwrap();
        assert!(json.get("providerId").is_some());
        assert!(json.get("provider_id").is_none());
    }

    #[test]
    fn action_fields_still_load_legacy_snake_case_entries() {
        // 旧 automations.json 按 snake_case 持久化，alias 保证可继续加载。
        let legacy: AutomationAction =
            serde_json::from_str(r#"{"kind":"launch","provider_id":"glm","cwd":null,"prompt":"hi"}"#)
                .unwrap();
        match legacy {
            AutomationAction::Launch { provider_id, .. } => assert_eq!(provider_id, "glm"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    fn input(trigger: AutomationTrigger, action: AutomationAction) -> NewAutomation {
        NewAutomation {
            name: "每日巡检".into(),
            trigger,
            action,
            max_runs: None,
            max_consecutive_failures: None,
            overlap_policy: OverlapPolicy::Allow,
        }
    }

    #[test]
    fn interval_fires_when_due_and_advances_from_now() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::Interval { minutes: 10 },
                    AutomationAction::Broadcast {
                        body: "检查构建".into(),
                    },
                ),
                t0,
            )
            .unwrap();
        assert_eq!(automation.next_run_at, Some(t0 + 600_000));

        assert!(manager.collect_due(t0 + 599_999).unwrap().is_empty());
        let due = manager.collect_due(t0 + 600_000).unwrap();
        assert_eq!(due.len(), 1);
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.run_count, 1);
        // 从触发时刻推进，错过的间隔不补跑。
        assert_eq!(item.next_run_at, Some(t0 + 1_200_000));
    }

    #[test]
    fn once_fires_exactly_once() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::Once { at: t0 + 1000 },
                    AutomationAction::Broadcast {
                        body: "一次性任务".into(),
                    },
                ),
                t0,
            )
            .unwrap();
        assert_eq!(manager.collect_due(t0 + 1000).unwrap().len(), 1);
        assert!(manager.collect_due(t0 + 60_000).unwrap().is_empty());
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.next_run_at, None);
    }

    #[test]
    fn max_runs_caps_total_triggers() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let mut new = input(
            AutomationTrigger::Interval { minutes: 1 },
            AutomationAction::Broadcast {
                body: "限次".into(),
            },
        );
        new.max_runs = Some(2);
        let automation = manager.create(new, t0).unwrap();

        assert_eq!(manager.collect_due(t0 + 60_000).unwrap().len(), 1);
        assert_eq!(manager.collect_due(t0 + 120_000).unwrap().len(), 1);
        assert!(manager.collect_due(t0 + 180_000).unwrap().is_empty());
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.next_run_at, None);
    }

    #[test]
    fn consecutive_failures_trip_circuit_breaker_and_success_resets() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::Interval { minutes: 5 },
                    AutomationAction::Message {
                        session_id: "ghost".into(),
                        body: "会失败".into(),
                        project_id: None,
                        agent_id: None,
                    },
                ),
                t0,
            )
            .unwrap();

        manager
            .report_result(&automation.id, false, "目标不存在".into(), t0)
            .unwrap();
        manager
            .report_result(&automation.id, true, "成功一次".into(), t0 + 1000)
            .unwrap();
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.consecutive_failures, 0); // 成功清零

        for offset in 0..DEFAULT_MAX_CONSECUTIVE_FAILURES {
            manager
                .report_result(
                    &automation.id,
                    false,
                    "目标不存在".into(),
                    t0 + 2000 + u64::from(offset),
                )
                .unwrap();
        }
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert!(!item.enabled); // 熔断停用
        assert_eq!(item.next_run_at, None);
        assert!(manager.collect_due(t0 + 600_000).unwrap().is_empty());

        // 重新启用：失败计数清零并按当前时刻重新排程。
        let re_enabled = manager
            .set_enabled(&automation.id, true, t0 + 700_000)
            .unwrap();
        assert!(re_enabled.enabled);
        assert_eq!(re_enabled.consecutive_failures, 0);
        assert_eq!(re_enabled.next_run_at, Some(t0 + 700_000 + 300_000));
    }

    #[test]
    fn recent_runs_capped_and_newest_kept() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::Interval { minutes: 5 },
                    AutomationAction::Broadcast {
                        body: "历史".into(),
                    },
                ),
                t0,
            )
            .unwrap();
        for index in 0..(MAX_RECENT_RUNS + 5) {
            manager
                .report_result(
                    &automation.id,
                    true,
                    format!("第 {index} 次"),
                    t0 + index as u64,
                )
                .unwrap();
        }
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.recent_runs.len(), MAX_RECENT_RUNS);
        assert_eq!(
            item.recent_runs.last().unwrap().detail,
            format!("第 {} 次", MAX_RECENT_RUNS + 4)
        );
    }

    #[test]
    fn calendar_schedule_skips_dst_gap_without_losing_future_runs() {
        use chrono::{LocalResult, NaiveDate, Timelike};
        let gap = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(); // Sunday spring-forward.
        let millis = |date: chrono::NaiveDate, hour, minute| date.and_hms_opt(hour, minute, 0).unwrap().and_utc().timestamp_millis() as u64;
        let resolve = |candidate: &chrono::NaiveDateTime| {
            if candidate.date() == gap && candidate.hour() == 2 {
                LocalResult::None
            } else {
                LocalResult::Single(candidate.and_utc().timestamp_millis())
            }
        };
        let before = millis(gap, 1, 0);
        assert_eq!(next_calendar_after(None, 2, 30, before, gap, resolve), Some(millis(gap.succ_opt().unwrap(), 2, 30)));
        let next_sunday = gap.checked_add_days(chrono::Days::new(7)).unwrap();
        assert_eq!(next_calendar_after(Some(&[7]), 2, 30, before, gap, resolve), Some(millis(next_sunday, 2, 30)));
    }

    #[test]
    fn calendar_schedule_uses_future_occurrence_in_dst_overlap() {
        use chrono::{LocalResult, NaiveDate};
        let date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let first = date.and_hms_opt(1, 30, 0).unwrap().and_utc().timestamp_millis();
        let second = first + 3_600_000;
        let resolve = |_: &chrono::NaiveDateTime| LocalResult::Ambiguous(first, second);
        assert_eq!(next_calendar_after(None, 1, 30, (first - 1) as u64, date, resolve), Some(first as u64));
        assert_eq!(next_calendar_after(None, 1, 30, (first + 1) as u64, date, resolve), Some(second as u64));
        assert_eq!(next_calendar_after(Some(&[7]), 1, 30, first as u64, date, resolve), Some(second as u64));
    }

    #[test]
    fn daily_schedules_next_local_occurrence() {
        let t0 = now_millis();
        // 目标时间 = 本地现在 + 2 分钟，应落在今天；验证严格晚于 now 且不晚于 24h。
        let next = next_daily_after(0, 0, t0).unwrap(); // 明天 00:00
        assert!(next > t0 && next <= t0 + 86_400_000);
        // 用「本地当前时刻 + 2 分钟」构造一个今天的候选。
        use chrono::{Datelike, Local, Timelike};
        let local_now: chrono::DateTime<Local> = Local::now();
        let candidate_minutes = (local_now.minute() + 2) % 60;
        let candidate_hour = (local_now.hour() + u32::from(local_now.minute() + 2 >= 60)) % 24;
        let now = now_millis();
        let next = next_daily_after(candidate_hour, candidate_minutes, now).unwrap();
        assert!(next > now);
        // 小时+分钟回绕到明天时仍应在 24h+2min 内。
        assert!(next <= now + 86_400_000 + 120_000);
        let _ = local_now.day();
    }

    #[test]
    fn weekly_lands_on_a_listed_weekday_within_eight_days() {
        use chrono::{Datelike, Local, TimeZone, Timelike};
        let now = now_millis();
        let next = next_weekly_after(&[1, 3, 5], 9, 30, now).unwrap();
        assert!(next > now && next <= now + 8 * 86_400_000);
        let dt = Local.timestamp_millis_opt(next as i64).single().unwrap();
        assert!([1, 3, 5].contains(&dt.weekday().number_from_monday()));
        assert_eq!((dt.hour(), dt.minute()), (9, 30));

        // 每天都选时退化为每天触发：明天之内必有一次。
        let daily_like = next_weekly_after(&[1, 2, 3, 4, 5, 6, 7], 0, 0, now).unwrap();
        assert!(daily_like > now && daily_like <= now + 86_400_000);

        // 只选今天但时刻已过 → 落到 7 天后。
        let local_now: chrono::DateTime<Local> = Local::now();
        let today = local_now.weekday().number_from_monday();
        let past_hour = local_now.hour().saturating_sub(2);
        let next_week = next_weekly_after(&[today], past_hour, 0, now).unwrap();
        let gap_days = (next_week - now) / 86_400_000;
        assert!((6..=7).contains(&gap_days));
    }

    #[test]
    fn cron_five_field_weekdays_follow_standard_numbering() {
        use chrono::{Datelike, Local, TimeZone, Weekday};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        assert_eq!(after.weekday(), Weekday::Sun);
        let runs = cron_next_runs("0 13 * * 1-5", after.timestamp_millis() as u64, 6).unwrap();
        let dates: Vec<_> = runs.into_iter().map(|at| {
            let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
            (date.day(), date.weekday())
        }).collect();
        assert_eq!(dates, vec![(22, Weekday::Mon), (23, Weekday::Tue), (24, Weekday::Wed),
            (25, Weekday::Thu), (26, Weekday::Fri), (29, Weekday::Mon)]);
    }

    fn cron_weekday_dates(field: &str, count: usize) -> Vec<(u32, chrono::Weekday)> {
        use chrono::{Datelike, Local, TimeZone};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        cron_next_runs(&format!("0 13 * * {field}"), after.timestamp_millis() as u64, count)
            .unwrap().into_iter().map(|at| {
                let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
                (date.day(), date.weekday())
            }).collect()
    }

    #[test]
    fn cron_five_field_supports_sunday_aliases_and_named_weekdays() {
        use chrono::Weekday::*;
        for field in ["0", "7", "Sun", "Sunday", "sUnDaY", "0,7,SUN"] {
            assert_eq!(cron_weekday_dates(field, 2), vec![(21, Sun), (28, Sun)], "{field}");
        }
        assert_eq!(cron_weekday_dates("MON,Wednesday,5", 4),
            vec![(22, Mon), (24, Wed), (26, Fri), (29, Mon)]);
    }

    #[test]
    fn cron_five_field_ranges_and_steps_follow_standard_weekdays() {
        use chrono::Weekday::*;
        for field in ["*/2", "0-6/2"] {
            assert_eq!(cron_weekday_dates(field, 5),
                vec![(21, Sun), (23, Tue), (25, Thu), (27, Sat), (28, Sun)], "{field}");
        }
        for field in ["1-7/2", "1/2"] {
            assert_eq!(cron_weekday_dates(field, 5),
                vec![(21, Sun), (22, Mon), (24, Wed), (26, Fri), (28, Sun)], "{field}");
        }
        assert_eq!(cron_weekday_dates("MON-FRI/2", 4),
            vec![(22, Mon), (24, Wed), (26, Fri), (29, Mon)]);
        assert_eq!(cron_weekday_dates("5-7", 4),
            vec![(21, Sun), (26, Fri), (27, Sat), (28, Sun)]);
    }

    #[test]
    fn cron_six_and_seven_fields_keep_existing_weekday_numbering() {
        use chrono::{Datelike, Local, TimeZone, Weekday};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        for expr in ["0 0 13 * * 1", "0 0 13 * * 1 2026"] {
            let runs = cron_next_runs(expr, after.timestamp_millis() as u64, 2).unwrap();
            assert_eq!(runs.into_iter().map(|at| {
                let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
                (date.day(), date.weekday())
            }).collect::<Vec<_>>(), vec![(21, Weekday::Sun), (28, Weekday::Sun)]);
        }
        assert!(cron_next_runs("0 0 13 * * 0", after.timestamp_millis() as u64, 1).is_err());
    }

    #[test]
    fn cron_five_field_rejects_invalid_weekdays_without_panicking() {
        let now = now_millis();
        for field in ["8", "-1", "7-1", "MON-SUN", "*/0", "1/0", "1/2/3", "1,,2", "1,", "TURTLE", "1-8"] {
            assert!(cron_next_runs(&format!("0 13 * * {field}"), now, 1).is_err(), "{field}");
        }
    }

    #[test]
    fn cron_five_field_day_and_weekday_restrictions_are_alternatives() {
        use chrono::{Datelike, Local, TimeZone, Weekday};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        let runs = cron_next_runs("0 13 1 * 1", after.timestamp_millis() as u64, 5).unwrap();
        assert_eq!(runs.into_iter().map(|at| {
            let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
            (date.month(), date.day(), date.weekday())
        }).collect::<Vec<_>>(), vec![(6, 22, Weekday::Mon), (6, 29, Weekday::Mon),
            (7, 1, Weekday::Wed), (7, 6, Weekday::Mon), (7, 13, Weekday::Mon)]);

        // 两个条件同日满足只触发一次；调度与预览必须共用语义。
        let after = Local.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).single().unwrap();
        let runs = cron_next_runs("0 13 1 * 1", after.timestamp_millis() as u64, 2).unwrap();
        assert_eq!(next_cron_after("0 13 1 * 1", after.timestamp_millis() as u64), Some(runs[0]));
        let dates: Vec<_> = runs.into_iter().map(|at| Local.timestamp_millis_opt(at as i64).single().unwrap().day()).collect();
        assert_eq!(dates, vec![1, 8]);
    }

    #[test]
    fn cron_five_field_day_alternative_can_be_impossible() {
        use chrono::{Datelike, Local, TimeZone, Weekday};
        let after = Local.with_ymd_and_hms(2026, 1, 31, 12, 0, 0).single().unwrap();
        let runs = cron_next_runs("0 13 31 2 1", after.timestamp_millis() as u64, 2).unwrap();
        assert_eq!(runs.into_iter().map(|at| {
            let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
            (date.month(), date.day(), date.weekday())
        }).collect::<Vec<_>>(), vec![(2, 2, Weekday::Mon), (2, 9, Weekday::Mon)]);
    }

    #[test]
    fn cron_wildcard_days_and_extended_expressions_keep_intersection_semantics() {
        use chrono::{Datelike, Local, TimeZone};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        for (expr, expected) in [
            ("0 13 * * 1", (2026, 6, 22)),
            ("0 13 1 * *", (2026, 7, 1)),
            ("0 13 */2 * 1", (2026, 6, 29)),
            ("0 13 1 * */2", (2026, 8, 1)),
            ("0 0 13 1 * MON", (2027, 2, 1)),
            ("0 0 13 1 * MON 2027", (2027, 2, 1)),
        ] {
            let at = cron_next_runs(expr, after.timestamp_millis() as u64, 1).unwrap()[0];
            let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
            assert_eq!((date.year(), date.month(), date.day()), expected, "{expr}");
        }
    }

    #[test]
    fn cron_explicit_future_year_starts_at_its_earliest_matching_date() {
        use chrono::{Datelike, Local, TimeZone, Timelike};
        let after = Local.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).single().unwrap();
        let runs = cron_next_runs("0 0 0 1 1 * 2027,2030", after.timestamp_millis() as u64, 3).unwrap();
        assert_eq!(runs.into_iter().map(|at| {
            let date = Local.timestamp_millis_opt(at as i64).single().unwrap();
            (date.year(), date.month(), date.day(), date.hour(), date.minute(), date.second())
        }).collect::<Vec<_>>(), vec![(2027, 1, 1, 0, 0, 0), (2030, 1, 1, 0, 0, 0)]);
        assert!(cron_next_runs("0 0 0 1 1 * 2025", after.timestamp_millis() as u64, 1).is_err());
    }

    #[test]
    fn cron_accepts_five_field_expr_and_rejects_garbage() {
        let now = now_millis();
        // 5 段经典格式：每 5 分钟，未来 15 分钟内应有 3 个触发点。
        let runs = cron_next_runs("*/5 * * * *", now, 3).unwrap();
        assert_eq!(runs.len(), 3);
        assert!(runs[0] > now && runs[0] <= now + 300_000);
        assert_eq!(runs[1] - runs[0], 300_000);

        // 6 段（含秒）：每 30 秒。
        let runs = cron_next_runs("*/30 * * * * *", now, 2).unwrap();
        assert_eq!(runs[1] - runs[0], 30_000);

        // 非法与永不触发。
        assert!(cron_next_runs("not a cron", now, 1).is_err());
        assert!(cron_next_runs("61 * * * *", now, 1).is_err());
        assert!(cron_next_runs("0 0 31 2 *", now, 1).is_err()); // 2 月 31 日
    }

    #[test]
    fn create_rejects_invalid_weekly_and_cron() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let mut bad = input(
            AutomationTrigger::Weekly {
                weekdays: vec![],
                hour: 9,
                minute: 0,
            },
            AutomationAction::Broadcast { body: "x".into() },
        );
        assert!(manager.create(bad.clone(), t0).is_err());
        bad.trigger = AutomationTrigger::Weekly {
            weekdays: vec![1, 8],
            hour: 9,
            minute: 0,
        };
        assert!(manager.create(bad.clone(), t0).is_err());
        bad.trigger = AutomationTrigger::Weekly {
            weekdays: vec![1, 2, 3, 4, 5],
            hour: 9,
            minute: 0,
        };
        assert!(manager.create(bad.clone(), t0).is_ok());
        bad.name = "另一个".to_string();
        bad.trigger = AutomationTrigger::Cron {
            expr: "0 9 * * 1-5".into(),
        };
        assert!(manager.create(bad, t0).is_ok());
    }

    #[test]
    fn create_rejects_invalid_input() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let mut bad = input(
            AutomationTrigger::Interval { minutes: 0 },
            AutomationAction::Broadcast { body: "x".into() },
        );
        assert!(manager.create(bad.clone(), t0).is_err());
        bad.trigger = AutomationTrigger::Interval { minutes: 10 };
        bad.name = "  ".into();
        assert!(manager.create(bad.clone(), t0).is_err());
        bad.name = "正常".into();
        bad.action = AutomationAction::Broadcast { body: "  ".into() };
        assert!(manager.create(bad.clone(), t0).is_err());
        bad.action = AutomationAction::Broadcast { body: "正常".into() };
        bad.max_runs = Some(0);
        assert!(manager.create(bad, t0).is_err());
    }

    #[test]
    fn session_event_trigger_fires_on_matching_event_only() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::OnSessionEvent {
                        event: SessionEventKind::Completed,
                    },
                    AutomationAction::Broadcast {
                        body: "有会话完成了".into(),
                    },
                ),
                t0,
            )
            .unwrap();
        // 事件型无时间排程。
        assert_eq!(automation.next_run_at, None);
        assert!(manager.collect_due(t0 + 86_400_000).unwrap().is_empty());

        // 不匹配的事件不触发。
        assert!(manager
            .collect_event(SessionEventKind::Failed, t0 + 1000)
            .unwrap()
            .is_empty());
        // 匹配触发一次，run_count 推进。
        let fired = manager
            .collect_event(SessionEventKind::Completed, t0 + 2000)
            .unwrap();
        assert_eq!(fired.len(), 1);
        let item = manager
            .list()
            .into_iter()
            .find(|item| item.id == automation.id)
            .unwrap();
        assert_eq!(item.run_count, 1);
        assert_eq!(item.last_run_at, Some(t0 + 2000));
    }

    #[test]
    fn session_event_trigger_respects_max_runs() {
        let manager = AutomationManager::new();
        let t0 = now_millis();
        let mut new = input(
            AutomationTrigger::OnSessionEvent {
                event: SessionEventKind::Failed,
            },
            AutomationAction::Broadcast {
                body: "有会话失败".into(),
            },
        );
        new.max_runs = Some(2);
        let automation = manager.create(new, t0).unwrap();

        assert_eq!(
            manager
                .collect_event(SessionEventKind::Failed, t0 + 1)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            manager
                .collect_event(SessionEventKind::Failed, t0 + 2)
                .unwrap()
                .len(),
            1
        );
        assert!(manager
            .collect_event(SessionEventKind::Failed, t0 + 3)
            .unwrap()
            .is_empty());

        // 禁用后不再触发；重新启用不清 run_count，max_runs 仍然生效。
        manager.set_enabled(&automation.id, false, t0 + 4).unwrap();
        assert!(manager
            .collect_event(SessionEventKind::Failed, t0 + 5)
            .unwrap()
            .is_empty());
        let re_enabled = manager.set_enabled(&automation.id, true, t0 + 6).unwrap();
        assert_eq!(re_enabled.next_run_at, None);
        assert!(manager
            .collect_event(SessionEventKind::Failed, t0 + 7)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn automations_survive_reopen() {
        let dir = std::env::temp_dir().join(format!("crewvela-automation-{}", uuid::Uuid::new_v4()));
        let path = dir.join("automations.json");
        let t0 = now_millis();
        let manager = AutomationManager::open_at(path.clone()).unwrap();
        let automation = manager
            .create(
                input(
                    AutomationTrigger::Interval { minutes: 30 },
                    AutomationAction::Broadcast {
                        body: "持久化".into(),
                    },
                ),
                t0,
            )
            .unwrap();
        manager
            .report_result(&automation.id, true, "跑过一次".into(), t0)
            .unwrap();
        drop(manager);

        let reopened = AutomationManager::open_at(path).unwrap();
        let items = reopened.list();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "每日巡检");
        assert_eq!(items[0].recent_runs.len(), 1);
        assert_eq!(items[0].next_run_at, automation.next_run_at);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_poll_and_heartbeat_collect_due_actions_only_once() {
        use std::sync::{Arc, Barrier};
        let manager = Arc::new(AutomationManager::new());
        let automation = manager.create(
            input(AutomationTrigger::Interval { minutes: 1 },
                AutomationAction::Broadcast { body: "exactly once".into() }), 0,
        ).unwrap();
        let due = automation.next_run_at.unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let workers = (0..8).map(|_| {
            let manager = manager.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                manager.collect_due(due).unwrap()
            })
        }).collect::<Vec<_>>();
        let claimed = workers.into_iter().flat_map(|worker| worker.join().unwrap()).collect::<Vec<_>>();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].0, automation.id);
        assert_eq!(manager.list()[0].run_count, 1);
        assert!(manager.collect_due(due).unwrap().is_empty());
    }

    fn cron_migration_time(year: i32, month: u32, day: u32, hour: u32) -> u64 {
        use chrono::{Local, TimeZone};
        Local.with_ymd_and_hms(year, month, day, hour, 0, 0).single().unwrap().timestamp_millis() as u64
    }

    fn legacy_cron_item(expr: &str, next: u64) -> Automation {
        let mut item = AutomationManager::new().create(
            input(AutomationTrigger::Cron { expr: expr.into() },
                AutomationAction::Broadcast { body: "迁移测试".into() }),
            cron_migration_time(2026, 6, 1, 12),
        ).unwrap();
        item.next_run_at = Some(next);
        item
    }

    #[test]
    fn legacy_cron_migration_recomputes_future_runs_once_and_persists_version() {
        let dir = std::env::temp_dir().join(format!("crewvela-cron-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("automations.json");
        let old = vec![
            legacy_cron_item("0 13 * * 1", cron_migration_time(2026, 6, 21, 13)),
            legacy_cron_item("0 13 1 * MON", cron_migration_time(2027, 2, 1, 13)),
            legacy_cron_item("0 0 13 1 * MON 2027", cron_migration_time(2027, 11, 1, 13)),
        ];
        std::fs::write(&path, serde_json::to_vec(&AutomationFile { version: 1, items: old }).unwrap()).unwrap();
        let manager = AutomationManager::open_at_time(path.clone(), cron_migration_time(2026, 6, 20, 12)).unwrap();
        assert_eq!(manager.list().iter().map(|item| item.next_run_at).collect::<Vec<_>>(), vec![
            Some(cron_migration_time(2026, 6, 22, 13)),
            Some(cron_migration_time(2026, 6, 22, 13)),
            Some(cron_migration_time(2027, 2, 1, 13)),
        ]);
        let saved: AutomationFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.version, AUTOMATION_FILE_VERSION);
        drop(manager);

        // 二次启动不能再次从 now 重排，否则会跳过已经到期但尚未执行的任务。
        let later = cron_migration_time(2026, 6, 25, 12);
        let reopened = AutomationManager::open_at_time(path, later).unwrap();
        assert_eq!(reopened.list()[0].next_run_at, Some(cron_migration_time(2026, 6, 22, 13)));
        assert_eq!(reopened.collect_due(later).unwrap().len(), 2);
        assert!(reopened.collect_due(later).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn future_automation_file_version_is_rejected_without_rewriting() {
        let dir = std::env::temp_dir().join(format!("crewvela-cron-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("automations.json");
        let raw = serde_json::json!({
            "version": AUTOMATION_FILE_VERSION + 1,
            "items": [],
            "futureSettings": { "retain": true },
        }).to_string();
        std::fs::write(&path, &raw).unwrap();
        let error = match AutomationManager::open_at_time(path.clone(), cron_migration_time(2026, 6, 25, 12)) {
            Ok(_) => panic!("未来版本不应被当前应用接受"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("请升级应用"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_cron_migration_keeps_valid_overdue_runs() {
        let due = cron_migration_time(2026, 6, 22, 13);
        let now = cron_migration_time(2026, 6, 23, 12);
        let mut items = vec![legacy_cron_item("0 13 * * MON", due)];
        migrate_legacy_cron_schedules(&mut items, now);
        assert_eq!(items[0].next_run_at, Some(due));
        let manager = AutomationManager { items: Mutex::new(items), path: None, dispatching: Mutex::new(std::collections::HashSet::new()) };
        assert_eq!(manager.collect_due(now).unwrap().len(), 1);
        assert!(manager.collect_due(now).unwrap().is_empty());
        assert_eq!(manager.list()[0].next_run_at, Some(cron_migration_time(2026, 6, 29, 13)));
    }

    #[test]
    fn legacy_cron_migration_corrects_overdue_wrong_weekdays_without_losing_valid_due_run() {
        let old_sunday = cron_migration_time(2026, 6, 21, 13);
        let corrected_monday = cron_migration_time(2026, 6, 22, 13);
        for (now, expected_due) in [
            (cron_migration_time(2026, 6, 21, 14), 0),
            (cron_migration_time(2026, 6, 23, 12), 1),
        ] {
            let mut items = vec![legacy_cron_item("0 13 * * 1", old_sunday)];
            migrate_legacy_cron_schedules(&mut items, now);
            assert_eq!(items[0].next_run_at, Some(corrected_monday));
            let manager = AutomationManager { items: Mutex::new(items), path: None, dispatching: Mutex::new(std::collections::HashSet::new()) };
            assert_eq!(manager.collect_due(now).unwrap().len(), expected_due);
            assert!(manager.collect_due(now).unwrap().is_empty());
        }
    }

    #[test]
    fn legacy_cron_migration_preserves_inactive_and_non_cron_schedules() {
        let due = cron_migration_time(2026, 6, 21, 13);
        let mut inactive = legacy_cron_item("0 13 * * 1", due);
        inactive.enabled = false;
        let mut exhausted = inactive.clone();
        exhausted.enabled = true;
        exhausted.max_runs = Some(1);
        exhausted.run_count = 1;
        let mut no_next = inactive.clone();
        no_next.enabled = true;
        no_next.next_run_at = None;
        let mut interval = inactive.clone();
        interval.enabled = true;
        interval.trigger = AutomationTrigger::Interval { minutes: 10 };
        let mut items = vec![inactive, exhausted, no_next, interval];
        let before = serde_json::to_value(&items).unwrap();
        migrate_legacy_cron_schedules(&mut items, cron_migration_time(2026, 6, 25, 12));
        assert_eq!(serde_json::to_value(items).unwrap(), before);
    }

    #[test]
    fn legacy_cron_migration_does_not_backfill_reenabled_schedule() {
        let mut item = legacy_cron_item("0 13 * * 1", cron_migration_time(2026, 6, 28, 13));
        item.last_run_at = Some(cron_migration_time(2026, 6, 8, 13));
        item.run_count = 1;
        let mut items = vec![item];
        let now = cron_migration_time(2026, 6, 25, 12);
        migrate_legacy_cron_schedules(&mut items, now);
        assert_eq!(items[0].next_run_at, Some(cron_migration_time(2026, 6, 29, 13)));
        let manager = AutomationManager { items: Mutex::new(items), path: None, dispatching: Mutex::new(std::collections::HashSet::new()) };
        assert!(manager.collect_due(now).unwrap().is_empty());
        assert_eq!(manager.list()[0].run_count, 1);
    }

    #[test]
    fn concurrent_changes_match_reopened_state() {
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("crewvela-automation-{}", uuid::Uuid::new_v4()));
        let path = dir.join("automations.json");
        let manager = Arc::new(AutomationManager::open_at(path.clone()).unwrap());
        let original = manager
            .create(
                input(
                    AutomationTrigger::Interval { minutes: 1 },
                    AutomationAction::Broadcast {
                        body: "tick".into(),
                    },
                ),
                1,
            )
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|scope| {
            let tick_manager = Arc::clone(&manager);
            let tick_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                tick_barrier.wait();
                for now in 60_001..60_101 {
                    tick_manager.collect_due(now).unwrap();
                }
            });
            let edit_manager = Arc::clone(&manager);
            let edit_barrier = Arc::clone(&barrier);
            let deleted_id = original.id.clone();
            scope.spawn(move || {
                edit_barrier.wait();
                if let Err(error) = edit_manager.delete(&deleted_id) {
                    assert!(error.to_string().contains("执行中或结果未确认"));
                    assert!(edit_manager.list().iter().find(|a| a.id == deleted_id).unwrap().recent_runs.iter().any(|r| is_active_stage(r.stage.as_deref().unwrap())));
                }
                for index in 0..100 {
                    edit_manager
                        .create(
                            input(
                                AutomationTrigger::OnSessionEvent {
                                    event: SessionEventKind::Failed,
                                },
                                AutomationAction::Broadcast {
                                    body: format!("new {index}"),
                                },
                            ),
                            1,
                        )
                        .unwrap();
                }
            });
            barrier.wait();
        });
        let in_memory = manager.list();
        drop(manager);
        let reopened = AutomationManager::open_at(path).unwrap().list();
        assert_eq!(reopened.len(), in_memory.len());
        if let Some(original) = reopened.iter().find(|item| item.id == original.id) {
            assert_eq!(original.recent_runs[0].stage.as_deref(), Some("unconfirmed"));
        }
        assert_eq!(
            reopened.iter().map(|item| &item.id).collect::<Vec<_>>(),
            in_memory.iter().map(|item| &item.id).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_save_does_not_acknowledge_or_mutate() {
        let dir = std::env::temp_dir().join(format!("crewvela-automation-{}", uuid::Uuid::new_v4()));
        let path = dir.join("automations.json");
        let manager = AutomationManager::open_at(path.clone()).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(manager
            .create(
                input(
                    AutomationTrigger::Once { at: 2 },
                    AutomationAction::Broadcast { body: "x".into() },
                ),
                1
            )
            .is_err());
        assert!(manager.list().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn agent_automation_skip_is_durable_without_consuming_runs_or_resetting_failures() {
        let root = std::env::temp_dir().join(format!("automation-agent-{}", uuid::Uuid::new_v4()));
        let path = root.join("automation.json");
        let manager = AutomationManager::open_at(path.clone()).unwrap();
        let mut new = input(AutomationTrigger::Interval { minutes: 1 }, AutomationAction::BroadcastAgent { runtime_kind: crate::multi_agent::AgentRuntimeKind::Codex, body: "inspect".into() });
        new.overlap_policy = OverlapPolicy::Skip;
        let task = manager.create(new, 1_000).unwrap();
        assert_eq!(manager.collect_due(61_000).unwrap().len(), 1);
        assert!(manager.collect_due(121_000).unwrap().is_empty(), "dispatch still in flight");
        manager.report_dispatch_result(&task.id, false, "queue failed".into(), 121_001, vec!["target".into()]).unwrap();
        let busy = std::collections::HashSet::from([task.id.clone()]);
        assert!(manager.collect_due_with_busy(181_000, &busy).unwrap().is_empty());
        let stored = manager.list().remove(0);
        assert_eq!(stored.run_count, 1);
        assert_eq!(stored.consecutive_failures, 1);
        assert_eq!(stored.next_run_at, Some(241_000));
        assert_eq!(stored.recent_runs.last().unwrap().stage.as_deref(), Some("skipped"));
        drop(manager);
        let reopened = AutomationManager::open_at(path).unwrap();
        assert_eq!(reopened.list()[0].active_session_ids, vec!["target"]);
        assert_eq!(reopened.list()[0].overlap_policy, OverlapPolicy::Skip);
        assert_eq!(reopened.collect_due(241_000).unwrap().len(), 1);
        drop(reopened); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_automation_filtered_events_and_contracts() {
        use crate::{multi_agent::AgentRuntimeKind as K, sessions::{SessionManager, NewRuntimeSession}};
        let root = std::env::temp_dir().join(format!("automation-sources-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let source = sessions.create_runtime(NewRuntimeSession { runtime_kind:"codex".into(), cwd:root.display().to_string(), project_id:Some("project-a".into()) }).unwrap();
        let other = sessions.create_runtime(NewRuntimeSession { runtime_kind:"pi".into(), cwd:root.display().to_string(), project_id:Some("project-b".into()) }).unwrap();
        let manager = AutomationManager::new();
        for (runtime_kind, project_id, session_id) in [(Some(K::Codex),None,None), (None,Some("project-a".into()),None), (None,None,Some(source.id.clone()))] {
            manager.create(input(AutomationTrigger::OnSessionEventFiltered { event:SessionEventKind::Completed,runtime_kind,project_id,session_id }, AutomationAction::SendAgent { runtime_kind:K::Pi, session_id:other.id.clone(),body:"hi".into(),project_id:None,agent_id:None }), 1).unwrap();
        }
        let busy = std::collections::HashSet::new();
        assert!(manager.collect_event_filtered(SessionEventKind::Completed, 2, &[other], &busy).unwrap().is_empty());
        assert!(manager.collect_event_filtered(SessionEventKind::Failed, 3, &[source.clone()], &busy).unwrap().is_empty());
        assert_eq!(manager.collect_event_filtered(SessionEventKind::Completed, 4, &[source], &busy).unwrap().len(), 3);
        let invalid = AutomationTrigger::OnSessionEventFiltered { event:SessionEventKind::Failed,runtime_kind:None,project_id:None,session_id:None };
        assert!(manager.create(input(invalid, AutomationAction::Broadcast { body:"x".into() }), 1).is_err());
        let action: AutomationAction = serde_json::from_value(serde_json::json!({"kind":"run_agent","runtimeKind":"codex","providerId":null,"model":null,"cwd":root.display().to_string(),"prompt":"literal"})).unwrap();
        let json = serde_json::to_value(action).unwrap();
        assert_eq!(json["runtimeKind"], "codex");
        assert!(json["providerId"].is_null() && json["model"].is_null());
        drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_automation_loads_v2_without_changing_legacy_schedules_or_overlap() {
        let root = std::env::temp_dir().join(format!("automation-v2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let manager = AutomationManager::new();
        let task = manager.create(input(AutomationTrigger::Cron { expr:"0 9 * * 1".into() }, AutomationAction::Broadcast { body:"legacy".into() }), now_millis()).unwrap();
        let mut old = serde_json::to_value(&task).unwrap();
        old.as_object_mut().unwrap().remove("overlapPolicy");
        old.as_object_mut().unwrap().remove("activeSessionIds");
        let path = root.join("automation.json");
        std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"version":2,"items":[old]})).unwrap()).unwrap();
        let reopened = AutomationManager::open_at_time(path, now_millis() + 60_000).unwrap();
        assert_eq!(reopened.list()[0].next_run_at, task.next_run_at);
        assert_eq!(reopened.list()[0].overlap_policy, OverlapPolicy::Allow);
        assert!(reopened.list()[0].active_session_ids.is_empty());
        drop(reopened); std::fs::remove_dir_all(root).unwrap();
    }

}
