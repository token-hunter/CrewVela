//! Count native turns and their reported usage, scoped to a single user request.
use super::{AgentRuntimeKind, AppendRuntimeEventInput, MultiAgentStore};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod history;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    #[serde(rename = "inputTotal")]
    input: Option<u64>,
    cache_read: Option<u64>,
    output: Option<u64>,
}

impl Tokens {
    const ZERO: Self = Self {
        input: Some(0),
        cache_read: Some(0),
        output: Some(0),
    };

    fn add(self, other: Self) -> Self {
        let add = |a: Option<u64>, b: Option<u64>| a.zip(b).and_then(|(a, b)| a.checked_add(b));
        Self {
            input: add(self.input, other.input),
            cache_read: add(self.cache_read, other.cache_read),
            output: add(self.output, other.output),
        }
    }

    // Codex already includes cached input. Other providers report the three
    // input categories separately. A missing category makes their total unknown.
    fn codex(value: &Value) -> Self {
        Self {
            input: value["input_tokens"].as_u64(),
            cache_read: value["cached_input_tokens"].as_u64(),
            output: value["output_tokens"].as_u64(),
        }
    }

    fn claude(value: &Value) -> Self {
        Self::separate_input(
            &value["input_tokens"],
            &value["cache_read_input_tokens"],
            &value["cache_creation_input_tokens"],
            &value["output_tokens"],
        )
    }

    fn pi(value: &Value) -> Self {
        Self::separate_input(
            &value["input"],
            &value["cacheRead"],
            &value["cacheWrite"],
            &value["output"],
        )
    }

    fn separate_input(input: &Value, read: &Value, write: &Value, output: &Value) -> Self {
        let total = || {
            input
                .as_u64()?
                .checked_add(read.as_u64()?)?
                .checked_add(write.as_u64()?)
        };
        Self {
            input: total(),
            cache_read: read.as_u64(),
            output: output.as_u64(),
        }
    }
}

/// Only terminal protocol records cover the whole CLI invocation. Never parse nested
/// tool output, message fragments, or the text that was printed for the user.
fn cli_usage(kind: AgentRuntimeKind, raw: &str) -> Option<Tokens> {
    let mut result = None;
    let mut session = false;
    let mut started = false;
    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim().trim_start_matches('\u{feff}'))
        else {
            continue;
        };
        let valid_id = |value: &Value| {
            value
                .as_str()
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
        };
        let usage = match (kind, event["type"].as_str()) {
            (AgentRuntimeKind::Codex, Some("thread.started")) if valid_id(&event["thread_id"]) => {
                if session {
                    return None;
                }
                session = true;
                continue;
            }
            (AgentRuntimeKind::Pi, Some("session")) if valid_id(&event["id"]) => {
                if session {
                    return None;
                }
                session = true;
                continue;
            }
            (AgentRuntimeKind::Codex, Some("turn.started"))
            | (AgentRuntimeKind::Pi, Some("agent_start"))
                if session =>
            {
                if started {
                    return None;
                }
                started = true;
                continue;
            }
            (AgentRuntimeKind::Codex, Some("turn.completed")) if started => {
                Tokens::codex(&event["usage"])
            }
            (AgentRuntimeKind::ClaudeCode, Some("result"))
                if valid_id(&event["session_id"])
                    && event["is_error"].is_boolean()
                    && event["subtype"].is_string() =>
            {
                started = true;
                Tokens::claude(&event["usage"])
            }
            (AgentRuntimeKind::Pi, Some("agent_end")) if started => {
                let mut assistants = event["messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|m| m["role"] == "assistant");
                match assistants.next() {
                    Some(first) => assistants.fold(Tokens::pi(&first["usage"]), |sum, m| {
                        sum.add(Tokens::pi(&m["usage"]))
                    }),
                    None => Tokens::default(),
                }
            }
            _ => continue,
        };
        // A one-shot invocation has one terminal record. A merged/replayed capture
        // cannot safely attribute several terminal results to one launch.
        if result.replace(usage).is_some() {
            return None;
        }
    }
    started.then(|| result.unwrap_or_default())
}

pub(super) fn record_cli_capture(
    store: &MultiAgentStore,
    run: &str,
    kind: AgentRuntimeKind,
    path: &std::path::Path,
) {
    use std::io::Read;
    if !matches!(
        kind,
        AgentRuntimeKind::Codex | AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Pi
    ) {
        return;
    }
    // The visible command wrapper already owns and bounds this per-run file.
    // If it cannot be read, only the original evidence from the store is used.
    const LIMIT: u64 = 6 * 1024 * 1024;
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let mut raw = String::new();
    if file.take(LIMIT + 1).read_to_string(&mut raw).is_ok() && raw.len() as u64 <= LIMIT {
        record_cli_usage(store, run, kind, &raw);
    }
}

pub(super) fn record_cli_usage(
    store: &MultiAgentStore,
    run: &str,
    kind: AgentRuntimeKind,
    raw: &str,
) {
    if !matches!(
        kind,
        AgentRuntimeKind::Codex | AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Pi
    ) {
        return;
    }
    if let Some(usage) = cli_usage(kind, raw) {
        if let Err(error) = store.append_runtime_event(AppendRuntimeEventInput {
            run_id: run.into(),
            event_type: "turn_token_usage".into(),
            payload: serde_json::json!(usage),
        }) {
            eprintln!("[crewvela] 无法保存本次运行的 token 用量：{error:#}");
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoundMetrics {
    totals: Metrics,
    tasks: Vec<TaskMetrics>,
    unstarted_members: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskMetrics {
    task_id: String,
    member_id: String,
    title: String,
    member: String,
    metrics: Metrics,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Metrics {
    starts: Option<u64>,
    repeat_dispatches: Option<u64>,
    tokens: Tokens,
}

struct TaskRun {
    task_id: String,
    title: String,
    agent: String,
    name: String,
    phase: Option<String>,
    reply_to: Option<String>,
    header: String,
    runtime: String,
    run: Option<String>,
    status: Option<String>,
    started: Option<i64>,
    finished: Option<i64>,
    signal: bool,
    usage: Option<Tokens>,
    locator: Option<Value>,
}

struct RoundEvidence {
    runs: Vec<TaskRun>,
    members: Vec<(String, String)>,
}

fn read_evidence(db: &Connection, project: &str, round: &str) -> Result<RoundEvidence> {
    let recipients: String = db.query_row(
        "SELECT json_extract(r.data_json,'$.participantAgentIds') FROM ma_conversation_rounds r
         JOIN ma_conversations c ON c.project_id=r.project_id AND c.id=json_extract(r.data_json,'$.conversationId')
         WHERE r.project_id=?1 AND r.id=?2", params![project, round], |r| r.get(0),
    ).optional()?.context("这条请求不属于当前项目会话")?;
    let mut member_query = db.prepare(
        "SELECT j.value,COALESCE(a.name,'历史成员') FROM json_each(?2) j
        LEFT JOIN ma_agents a ON a.id=j.value AND a.project_id=?1 ORDER BY j.key",
    )?;
    let members = member_query
        .query_map(params![project, recipients], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut query = db.prepare(
        "WITH scoped(task_id,phase,reply_to,pos) AS (
            SELECT t.task_id,json_extract(t.data_json,'$.phase'),json_extract(m.data_json,'$.replyToMessageId'),t.ordinal
            FROM ma_conversation_turns t LEFT JOIN ma_conversation_messages m ON m.id=t.id AND m.project_id=?1
            WHERE t.round_id=?2
            UNION ALL
            SELECT s.task_id,NULL,NULL,NULL FROM ma_conversation_task_scopes s
            JOIN ma_conversation_plans p ON p.id=s.plan_id AND p.project_id=?1 WHERE p.round_id=?2
            UNION ALL
            SELECT 'cli-message-'||x.message_id,x.purpose,x.message_id,NULL FROM ma_member_messages x
            JOIN ma_messages m ON m.id=x.message_id AND m.project_id=?1
            WHERE x.round_id=?2 AND x.purpose IN ('question','handoff','direct')
         )
         SELECT t.id,t.title,COALESCE(r.agent_id,t.assignee_agent_id,''),COALESCE(a.name,'历史成员'),s.phase,s.reply_to,
            CASE WHEN s.pos IS NOT NULL THEN substr(t.description,1,min(512,instr(t.description,char(10))-1)) ELSE '' END,
            COALESCE(a.runtime_kind,''),r.id,r.status,r.started_at,r.finished_at,r.session_locator_json,
            EXISTS(SELECT 1 FROM ma_runtime_events e WHERE e.run_id=r.id AND (
              e.event_type IN ('conversation_prompt_received','agent_activity','turn_token_usage')
              OR (e.event_type='managed_turn' AND json_extract(e.payload_json,'$.phase') IN ('accepted','completed','failed','interrupted')))),
            (SELECT e.payload_json FROM ma_runtime_events e WHERE e.run_id=r.id AND e.event_type='turn_token_usage'
                ORDER BY e.seq DESC LIMIT 1)
         FROM scoped s JOIN ma_tasks t ON t.id=s.task_id AND t.project_id=?1
         LEFT JOIN ma_runs r ON r.task_id=t.id AND r.project_id=?1
         LEFT JOIN ma_agents a ON a.id=COALESCE(r.agent_id,t.assignee_agent_id) AND a.project_id=?1
         ORDER BY s.pos IS NULL,s.pos,t.created_at,t.id,r.attempt")?;
    let rows = query.query_map(params![project, round], |row| {
        Ok((
            TaskRun {
                task_id: row.get(0)?,
                title: row.get(1)?,
                agent: row.get(2)?,
                name: row.get(3)?,
                phase: row.get(4)?,
                reply_to: row.get(5)?,
                header: row.get(6)?,
                runtime: row.get(7)?,
                run: row.get(8)?,
                status: row.get(9)?,
                started: row.get(10)?,
                finished: row.get(11)?,
                signal: row.get(13)?,
                usage: None,
                locator: None,
            },
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<String>>(14)?,
        ))
    })?;
    let mut runs = Vec::new();
    for row in rows {
        let (mut run, locator, usage) = row?;
        run.locator = locator.map(|v| serde_json::from_str(&v)).transpose()?;
        run.usage = usage.map(|v| serde_json::from_str(&v)).transpose()?;
        runs.push(run);
    }
    Ok(RoundEvidence { runs, members })
}

#[derive(Default)]
struct LaunchCounts {
    confirmed: u64,
    possible: u64,
}

impl LaunchCounts {
    fn exact(&self) -> Option<u64> {
        (self.confirmed == self.possible).then_some(self.confirmed)
    }
    fn repeats(&self) -> Option<u64> {
        let minimum = self.confirmed.saturating_sub(1);
        (minimum == self.possible.saturating_sub(1)).then_some(minimum)
    }
}

fn measure(evidence: RoundEvidence, round: &str) -> RoundMetrics {
    let mut positions = HashMap::new();
    let mut work: Vec<(TaskMetrics, LaunchCounts)> = Vec::new();
    let mut possible_members = HashSet::new();
    for run in evidence.runs {
        let subject = run.reply_to.as_deref().unwrap_or(&run.task_id);
        let key = (subject.to_owned(), run.phase.clone(), run.agent.clone());
        let index = *positions.entry(key).or_insert_with(|| {
            let index = work.len();
            let title = match run.phase.as_deref() {
                Some("chat") => "群聊回复",
                Some("exchange") => "成员接力",
                Some("plan") => "分工规划",
                Some("question") => "成员提问",
                Some("handoff") => "成员交接",
                Some("direct") => "成员消息",
                _ => &run.title,
            };
            work.push((
                TaskMetrics {
                    task_id: run.task_id.clone(),
                    member_id: run.agent.clone(),
                    title: title.into(),
                    member: run.name.clone(),
                    metrics: Metrics {
                        starts: Some(0),
                        repeat_dispatches: Some(0),
                        tokens: Tokens::ZERO,
                    },
                },
                LaunchCounts::default(),
            ));
            index
        });
        let (task, launches) = &mut work[index];
        if run.run.is_none() {
            continue;
        }
        let usage = match run.usage {
            Some(known)
                if known.input.is_some()
                    && known.cache_read.is_some()
                    && known.output.is_some() =>
            {
                Some(known)
            }
            recorded => match history::usage(&run, round) {
                Some(history) => {
                    let known = recorded.unwrap_or_default();
                    Some(Tokens {
                        input: known.input.or(history.input),
                        cache_read: known.cache_read.or(history.cache_read),
                        output: known.output.or(history.output),
                    })
                }
                None => recorded,
            },
        };
        let confirmed = run.signal || usage.is_some() || run.status.as_deref() == Some("succeeded");
        // Claim/heartbeat/start preparation alone does not prove that a provider
        // turned. An old failed/interrupted attempt with no start evidence can
        // have consumed tokens, whereas an unstarted claim cannot.
        let possible = confirmed || run.started.is_some();
        if possible {
            possible_members.insert(run.agent);
            launches.possible += 1;
            launches.confirmed += u64::from(confirmed);
            task.metrics.tokens = task.metrics.tokens.add(usage.unwrap_or_default());
        }
    }
    let mut totals = Metrics {
        starts: Some(0u64),
        repeat_dispatches: Some(0u64),
        tokens: Tokens::ZERO,
    };
    let add = |a: Option<u64>, b: Option<u64>| a.zip(b).map(|(a, b)| a + b);
    let tasks = work
        .into_iter()
        .map(|(mut task, launches)| {
            task.metrics.starts = launches.exact();
            task.metrics.repeat_dispatches = launches.repeats();
            totals.starts = add(totals.starts, task.metrics.starts);
            totals.repeat_dispatches =
                add(totals.repeat_dispatches, task.metrics.repeat_dispatches);
            totals.tokens = totals.tokens.add(task.metrics.tokens);
            task
        })
        .collect();
    let unstarted_members = evidence
        .members
        .into_iter()
        .filter(|(id, _)| !possible_members.contains(id))
        .map(|(_, name)| name)
        .collect();
    RoundMetrics {
        totals,
        tasks,
        unstarted_members,
    }
}

impl MultiAgentStore {
    pub(super) fn conversation_round_metrics(
        &self,
        project: &str,
        round: &str,
    ) -> Result<RoundMetrics> {
        // Release the project database before optional native transcript I/O.
        let evidence = read_evidence(&self.connection.lock().unwrap(), project, round)?;
        Ok(measure(evidence, round))
    }
}

#[tauri::command]
pub(crate) async fn ma_conversation_round_metrics(
    project_id: String,
    round_id: String,
    state: tauri::State<'_, MultiAgentStore>,
) -> Result<RoundMetrics, String> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.conversation_round_metrics(&project_id, &round_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[cfg(test)]
pub(super) fn codex_test_output(usage: Value) -> String {
    format!(
        "{}\n{}\n{}",
        serde_json::json!({"type":"thread.started","thread_id":uuid::Uuid::new_v4().to_string()}),
        serde_json::json!({"type":"turn.started"}),
        serde_json::json!({"type":"turn.completed","usage":usage})
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_cli_receipts_require_totals_and_preserve_missing_and_zero_fields() {
        let pi = json!({"type":"agent_end","messages":[
            {"role":"user","content":"irrelevant"},
            {"role":"assistant","usage":{"input":17,"cacheRead":0,"cacheWrite":0,"output":3}},
            {"role":"toolResult","content":[{"text":"printed usage is not evidence"}]},
            {"role":"assistant","usage":{"input":8,"cacheRead":5,"cacheWrite":2,"output":4}}
        ]});
        assert_eq!(
            cli_usage(
                AgentRuntimeKind::Pi,
                &format!(
                    "{}\n{}\n{pi}",
                    json!({"type":"session","id":uuid::Uuid::new_v4().to_string()}),
                    json!({"type":"agent_start"})
                )
            ),
            Some(Tokens {
                input: Some(32),
                cache_read: Some(5),
                output: Some(7)
            })
        );
        assert_eq!(
            cli_usage(
                AgentRuntimeKind::Pi,
                r#"{"type":"message_end","message":{"usage":{"input":6}}}"#
            ),
            None
        );
        let partial = codex_test_output(json!({"input_tokens":20,"output_tokens":0}));
        assert_eq!(
            cli_usage(AgentRuntimeKind::Codex, &partial),
            Some(Tokens {
                input: Some(20),
                cache_read: None,
                output: Some(0)
            })
        );
        assert_eq!(
            cli_usage(AgentRuntimeKind::Codex, &format!("{partial}\n{partial}")),
            None
        );
        assert_eq!(
            cli_usage(
                AgentRuntimeKind::Codex,
                r#"{"sessionUpdate":"usage_update","used":80,"size":128}"#
            ),
            None
        );
        let claude = json!({"type":"result","subtype":"success","is_error":false,"session_id":uuid::Uuid::new_v4().to_string(),"usage":{"input_tokens":23,"cache_read_input_tokens":11,"cache_creation_input_tokens":7,"output_tokens":4}}).to_string();
        assert_eq!(
            cli_usage(AgentRuntimeKind::ClaudeCode, &claude),
            Some(Tokens {
                input: Some(41),
                cache_read: Some(11),
                output: Some(4)
            })
        );
        assert_eq!(
            Tokens::pi(&json!({"input":17,"cacheRead":5,"output":2})),
            Tokens {
                input: None,
                cache_read: Some(5),
                output: Some(2)
            }
        );
    }
}
