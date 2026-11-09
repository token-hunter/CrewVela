use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

const LEGACY_TOKEN_ACCOUNT: &str = "codex-quota-x-bearer-token";
const PRIMARY_SOURCE: &str = "codexreset.org";
const SECONDARY_SOURCE: &str = "codexresets.com";
const TIBO_SOURCE: &str = "x.com/thsottiaux";
const PRIMARY_URL: &str = "https://codexreset.org/";
const SECONDARY_URL: &str = "https://codexresets.com/";
const TIBO_URL: &str = "https://x.com/thsottiaux";
const TIBO_FALLBACK_URL: &str = "https://r.jina.ai/https://x.com/thsottiaux";
const X_BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/138 Safari/537.36";

#[derive(Clone)]
pub struct CodexQuotaMonitor {
    connection: Arc<Mutex<Connection>>,
    client: Client,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub cid_installed: bool,
    pub tracked_accounts: i64,
    pub public_sources_online: i64,
    pub public_sources_parsed: i64,
    pub last_checked_at: Option<i64>,
    pub last_event_id: Option<String>,
    pub last_error: Option<String>,
    pub matched_events: i64,
    pub source_diagnostics: Vec<PublicSourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSourceDiagnostic {
    pub source: String,
    pub url: String,
    pub http_ok: bool,
    pub parse_ok: bool,
    pub checked_at: Option<i64>,
    pub latest_event_id: Option<String>,
    pub latest_text: Option<String>,
    pub latest_created_at: Option<String>,
    pub latest_url: Option<String>,
    pub event_kind: Option<String>,
    pub decision: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaEvent {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub created_at: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredQuotaNotification {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub created_at: Option<String>,
    pub notified_at: i64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReceipt {
    #[serde(flatten)]
    pub status: MonitorStatus,
    pub initialized: bool,
    pub events: Vec<QuotaEvent>,
}

#[derive(Debug, Clone)]
struct PublicReset {
    source: &'static str,
    event_id: String,
    text: String,
    created_at: Option<String>,
    url: String,
    kind: &'static str,
}

#[derive(Debug, Clone)]
struct ObservedPost {
    event_id: String,
    text: String,
    created_at: Option<String>,
    url: String,
    kind: Option<&'static str>,
}

impl From<&PublicReset> for ObservedPost {
    fn from(reset: &PublicReset) -> Self {
        Self {
            event_id: reset.event_id.clone(),
            text: reset.text.clone(),
            created_at: reset.created_at.clone(),
            url: reset.url.clone(),
            kind: Some(reset.kind),
        }
    }
}

#[derive(Debug)]
struct TiboTimeline {
    latest: ObservedPost,
    relevant: Option<PublicReset>,
}

#[derive(Debug)]
struct SourceFetch {
    source: &'static str,
    http_ok: bool,
    reset: Option<PublicReset>,
    latest: Option<ObservedPost>,
    error: Option<String>,
}

impl SourceFetch {
    fn http_error(source: &'static str, error: String) -> Self {
        Self {
            source,
            http_ok: false,
            reset: None,
            latest: None,
            error: Some(error),
        }
    }
}

impl CodexQuotaMonitor {
    pub fn new() -> Result<Self> {
        // X API 监控已废弃；顺手清理旧版本留在系统凭据管理器中的 Token。
        let _ = crate::secrets::delete(LEGACY_TOKEN_ACCOUNT);
        let path = crate::providers::config_dir()?.join("notifications.db");
        let connection = Connection::open(&path)
            .with_context(|| format!("打开通知数据库失败: {}", path.display()))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS codex_quota_monitor_state (
               id INTEGER PRIMARY KEY CHECK(id=1),
               last_checked_at INTEGER,
               last_error TEXT,
               public_sources_online INTEGER NOT NULL DEFAULT 0,
               public_sources_parsed INTEGER NOT NULL DEFAULT 0
             );
             INSERT OR IGNORE INTO codex_quota_monitor_state(id) VALUES(1);
             CREATE TABLE IF NOT EXISTS codex_quota_account_state (
               account TEXT PRIMARY KEY,
               used_percent REAL,
               reset_label TEXT,
               checked_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS codex_quota_public_state (
               source TEXT PRIMARY KEY,
               last_event_id TEXT,
               checked_at INTEGER NOT NULL,
               last_error TEXT,
               http_ok INTEGER NOT NULL DEFAULT 0,
               parse_ok INTEGER NOT NULL DEFAULT 0,
               latest_text TEXT,
               latest_created_at TEXT,
               event_kind TEXT,
               decision TEXT,
               event_url TEXT,
               latest_seen_id TEXT
             );
             CREATE TABLE IF NOT EXISTS codex_quota_free_notifications (
               event_id TEXT PRIMARY KEY,
               source TEXT NOT NULL,
               event_text TEXT NOT NULL,
               created_at TEXT,
               notified_at INTEGER NOT NULL,
               event_kind TEXT,
               event_url TEXT
             );
             DROP TABLE IF EXISTS codex_quota_state;
             DROP TABLE IF EXISTS codex_quota_notifications;",
        )?;
        add_column_if_missing(
            &connection,
            "codex_quota_monitor_state",
            "public_sources_parsed INTEGER NOT NULL DEFAULT 0",
        )?;
        for column in [
            "http_ok INTEGER NOT NULL DEFAULT 0",
            "parse_ok INTEGER NOT NULL DEFAULT 0",
            "latest_text TEXT",
            "latest_created_at TEXT",
            "event_kind TEXT",
            "decision TEXT",
            "event_url TEXT",
            "latest_seen_id TEXT",
        ] {
            add_column_if_missing(&connection, "codex_quota_public_state", column)?;
        }
        add_column_if_missing(
            &connection,
            "codex_quota_free_notifications",
            "event_kind TEXT",
        )?;
        add_column_if_missing(
            &connection,
            "codex_quota_free_notifications",
            "event_url TEXT",
        )?;
        let client = crate::general_settings::configure_http_client(Client::builder())
            .timeout(Duration::from_secs(18))
            .user_agent("CrewVela/0.1 Codex quota monitor")
            .build()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            client,
        })
    }

    pub fn status(&self) -> Result<MonitorStatus> {
        let connection = self.connection.lock().unwrap();
        let (last_checked_at, last_error, public_sources_online, public_sources_parsed): (Option<i64>, Option<String>, i64, i64) = connection.query_row(
            "SELECT last_checked_at,last_error,public_sources_online,public_sources_parsed FROM codex_quota_monitor_state WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let tracked_accounts = connection.query_row(
            "SELECT COUNT(*) FROM codex_quota_account_state",
            [],
            |row| row.get(0),
        )?;
        let matched_events = connection.query_row(
            "SELECT COUNT(*) FROM codex_quota_free_notifications",
            [],
            |row| row.get(0),
        )?;
        let last_event_id = connection.query_row(
            "SELECT event_id FROM codex_quota_free_notifications ORDER BY notified_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        ).ok();
        Ok(MonitorStatus {
            cid_installed: crate::codex_accounts::is_installed(),
            tracked_accounts,
            public_sources_online,
            public_sources_parsed,
            last_checked_at,
            last_event_id,
            last_error,
            matched_events,
            source_diagnostics: load_source_diagnostics(&connection)?,
        })
    }

    pub fn notifications(&self) -> Result<Vec<StoredQuotaNotification>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT event_id,source,event_text,created_at,notified_at,event_kind,event_url
             FROM codex_quota_free_notifications ORDER BY notified_at DESC LIMIT 200",
        )?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let source: String = row.get(1)?;
            let kind: Option<String> = row.get(5)?;
            let stored_url: Option<String> = row.get(6)?;
            let fallback_kind = if source == "cid" {
                "account_reset"
            } else {
                "public_confirmed"
            };
            let url = stored_url
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    id.strip_prefix("public:")
                        .map(|status_id| format!("https://x.com/thsottiaux/status/{status_id}"))
                        .unwrap_or_default()
                });
            Ok(StoredQuotaNotification {
                id,
                source,
                kind: kind.unwrap_or_else(|| fallback_kind.into()),
                text: row.get(2)?,
                created_at: row.get(3)?,
                notified_at: row.get(4)?,
                url,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub async fn check(&self) -> Result<CheckReceipt> {
        let now = now_millis();
        let account_baseline_exists = {
            let connection = self.connection.lock().unwrap();
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_quota_account_state)",
                [],
                |row| row.get::<_, bool>(0),
            )?
        };
        let source_baseline_exists = {
            let connection = self.connection.lock().unwrap();
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_quota_public_state)",
                [],
                |row| row.get::<_, bool>(0),
            )?
        };
        let initialized = !account_baseline_exists && !source_baseline_exists;
        let mut events = Vec::new();
        let mut errors = Vec::new();

        let account_snapshot = tokio::task::spawn_blocking(crate::codex_accounts::list)
            .await
            .context("等待 cid 额度检查失败")?;
        match account_snapshot {
            Ok(snapshot) if snapshot.installed => {
                self.check_accounts(snapshot, now, &mut events)?;
            }
            Ok(_) => errors.push("本机未安装 cid，无法检测账户实际额度".to_string()),
            Err(error) => errors.push(format!("cid 检查失败: {error}")),
        }

        let (primary, secondary, tibo) = tokio::join!(
            self.fetch_primary_source(),
            self.fetch_secondary_source(),
            self.fetch_tibo_source()
        );
        let mut sources_online = 0_i64;
        let mut sources_parsed = 0_i64;
        for result in [primary, secondary, tibo] {
            if result.http_ok {
                sources_online += 1;
            }
            if result.http_ok && result.error.is_none() {
                sources_parsed += 1;
            }
            if let Some(error) = result.error.as_ref() {
                errors.push(format!("{}: {error}", result.source));
            }
            self.record_public_result(result, now, &mut events)?;
        }

        let last_error = (!errors.is_empty()).then(|| errors.join("；"));
        self.connection.lock().unwrap().execute(
            "UPDATE codex_quota_monitor_state SET last_checked_at=?1,last_error=?2,public_sources_online=?3,public_sources_parsed=?4 WHERE id=1",
            params![now, last_error, sources_online, sources_parsed],
        )?;
        Ok(CheckReceipt {
            status: self.status()?,
            initialized,
            events,
        })
    }

    fn check_accounts(
        &self,
        snapshot: crate::codex_accounts::CodexAccountSnapshot,
        now: i64,
        events: &mut Vec<QuotaEvent>,
    ) -> Result<()> {
        let previous = {
            let connection = self.connection.lock().unwrap();
            let mut statement =
                connection.prepare("SELECT account,used_percent FROM codex_quota_account_state")?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?))
                })?
                .filter_map(Result::ok)
                .collect::<HashMap<_, _>>();
            rows
        };
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        for account in snapshot.accounts {
            // cid 输出的是“剩余额度百分比”。数据库列名 used_percent 为兼容旧版本保留，
            // 其中存储的原始数值无需迁移，只需按剩余额度语义判断。
            let remaining = crate::codex_accounts::account_remaining(&account);
            if let (Some(Some(before)), Some(after)) = (previous.get(&account.account), remaining) {
                if is_account_reset(*before, after) {
                    let event_id = format!("cid:{}:{now}", stable_account_id(&account.account));
                    let text = format!(
                        "Codex 账号额度已恢复：剩余额度从 {:.0}% 升至 {:.0}%",
                        before, after
                    );
                    let inserted = transaction.execute(
                        "INSERT OR IGNORE INTO codex_quota_free_notifications(event_id,source,event_text,created_at,notified_at,event_kind,event_url) VALUES(?1,'cid',?2,NULL,?3,'account_reset','')",
                        params![event_id, text, now],
                    )?;
                    if inserted > 0 {
                        events.push(QuotaEvent {
                            id: event_id,
                            source: "本机 cid".into(),
                            kind: "account_reset".into(),
                            text,
                            created_at: None,
                            url: String::new(),
                        });
                    }
                }
            }
            transaction.execute(
                "INSERT INTO codex_quota_account_state(account,used_percent,reset_label,checked_at) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(account) DO UPDATE SET used_percent=excluded.used_percent,reset_label=excluded.reset_label,checked_at=excluded.checked_at",
                params![account.account, remaining, account.total_reset.or(account.next_reset), now],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    async fn fetch_primary_source(&self) -> SourceFetch {
        self.fetch_public_source(PRIMARY_SOURCE, PRIMARY_URL, parse_primary_source)
            .await
    }

    async fn fetch_secondary_source(&self) -> SourceFetch {
        self.fetch_public_source(SECONDARY_SOURCE, SECONDARY_URL, parse_secondary_source)
            .await
    }

    async fn fetch_tibo_source(&self) -> SourceFetch {
        let direct = self
            .client
            .get(TIBO_URL)
            .header(reqwest::header::USER_AGENT, X_BROWSER_USER_AGENT)
            .send()
            .await;
        let direct_error = match direct {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.text().await {
                    Ok(html) => {
                        if let Some(timeline) = parse_tibo_profile_html(&html) {
                            return source_fetch_from_tibo_timeline(timeline);
                        }
                        Some("X 主页可访问，但时间线结构无法识别".to_string())
                    }
                    Err(error) => Some(error.to_string()),
                },
                Err(error) => Some(error.to_string()),
            },
            Err(error) => Some(error.to_string()),
        };

        let fallback = self.client.get(TIBO_FALLBACK_URL).send().await;
        match fallback {
            Ok(response) => match response.error_for_status() {
                Ok(response) => match response.text().await {
                    Ok(markdown) => match parse_tibo_jina_markdown(&markdown) {
                        Some(timeline) => source_fetch_from_tibo_timeline(timeline),
                        None => SourceFetch {
                            source: TIBO_SOURCE,
                            http_ok: true,
                            reset: None,
                            latest: None,
                            error: Some(format!(
                                "直接抓取失败（{}）；备用页面结构无法识别",
                                direct_error.unwrap_or_else(|| "未知错误".into())
                            )),
                        },
                    },
                    Err(error) => SourceFetch::http_error(
                        TIBO_SOURCE,
                        format!(
                            "直接抓取失败（{}）；备用正文读取失败：{error}",
                            direct_error.unwrap_or_else(|| "未知错误".into())
                        ),
                    ),
                },
                Err(error) => SourceFetch::http_error(
                    TIBO_SOURCE,
                    format!(
                        "直接抓取失败（{}）；备用请求失败：{error}",
                        direct_error.unwrap_or_else(|| "未知错误".into())
                    ),
                ),
            },
            Err(error) => SourceFetch::http_error(
                TIBO_SOURCE,
                format!(
                    "直接抓取失败（{}）；备用连接失败：{error}",
                    direct_error.unwrap_or_else(|| "未知错误".into())
                ),
            ),
        }
    }

    async fn fetch_public_source(
        &self,
        source: &'static str,
        url: &'static str,
        parser: fn(&str) -> Option<PublicReset>,
    ) -> SourceFetch {
        let response = match self.client.get(url).send().await {
            Ok(response) => response,
            Err(error) => return SourceFetch::http_error(source, error.to_string()),
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) => return SourceFetch::http_error(source, error.to_string()),
        };
        let html = match response.text().await {
            Ok(html) => html,
            Err(error) => return SourceFetch::http_error(source, error.to_string()),
        };
        match parser(&html) {
            Some(reset) => {
                let latest = Some(ObservedPost::from(&reset));
                SourceFetch {
                    source,
                    http_ok: true,
                    reset: Some(reset),
                    latest,
                    error: None,
                }
            }
            None if page_structure_recognized(source, &html) => SourceFetch {
                source,
                http_ok: true,
                reset: None,
                latest: None,
                error: None,
            },
            None => SourceFetch {
                source,
                http_ok: true,
                reset: None,
                latest: None,
                error: Some("网页可访问，但页面结构无法识别".into()),
            },
        }
    }

    fn record_public_result(
        &self,
        result: SourceFetch,
        now: i64,
        events: &mut Vec<QuotaEvent>,
    ) -> Result<()> {
        let source = result.source;
        let previous = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT last_event_id FROM codex_quota_public_state WHERE source=?1",
                [source],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        let current_id = result.reset.as_ref().map(|item| item.event_id.as_str());
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut decision = if !result.http_ok {
            "http_error"
        } else if result.error.is_some() {
            "parse_error"
        } else if result.reset.is_none() {
            "no_relevant_event"
        } else if previous.is_none() {
            "baseline"
        } else {
            "unchanged"
        };
        if let Some(item) = result.reset.as_ref() {
            let event_id = format!("public:{}", item.event_id);
            let already_notified = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_quota_free_notifications WHERE event_id=?1)",
                [&event_id],
                |row| row.get::<_, bool>(0),
            )?;
            let changed = should_notify_public_reset(previous.as_ref(), &item.event_id);
            let recover_fresh_tibo_baseline =
                source == TIBO_SOURCE && !already_notified && is_recent_public_event(item, now);
            if changed || recover_fresh_tibo_baseline {
                let inserted = transaction.execute(
                    "INSERT OR IGNORE INTO codex_quota_free_notifications(event_id,source,event_text,created_at,notified_at,event_kind,event_url) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![event_id, item.source, item.text, item.created_at, now, item.kind, item.url],
                )?;
                if inserted > 0 {
                    decision = "notified";
                    events.push(QuotaEvent {
                        id: event_id,
                        source: item.source.into(),
                        kind: item.kind.into(),
                        text: item.text.clone(),
                        created_at: item.created_at.clone(),
                        url: item.url.clone(),
                    });
                } else {
                    decision = "duplicate";
                }
            }
        }
        if !matches!(decision, "notified" | "duplicate")
            && result
                .latest
                .as_ref()
                .is_some_and(|latest| latest.kind.is_none())
        {
            decision = "no_relevant_event";
        }
        let parse_ok = result.http_ok && result.error.is_none();
        let latest = result.latest.as_ref();
        let latest_seen_id = latest.map(|item| item.event_id.as_str());
        let latest_text = latest.map(|item| item.text.as_str());
        let latest_created_at = latest.and_then(|item| item.created_at.as_deref());
        let event_kind = latest.and_then(|item| item.kind);
        let event_url = latest.map(|item| item.url.as_str());
        transaction.execute(
            "INSERT INTO codex_quota_public_state(source,last_event_id,checked_at,last_error,http_ok,parse_ok,latest_text,latest_created_at,event_kind,decision,event_url,latest_seen_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(source) DO UPDATE SET
               last_event_id=COALESCE(excluded.last_event_id,codex_quota_public_state.last_event_id),
               checked_at=excluded.checked_at,last_error=excluded.last_error,http_ok=excluded.http_ok,
               parse_ok=excluded.parse_ok,
               latest_text=CASE WHEN excluded.latest_seen_id IS NOT NULL THEN excluded.latest_text ELSE codex_quota_public_state.latest_text END,
               latest_created_at=CASE WHEN excluded.latest_seen_id IS NOT NULL THEN excluded.latest_created_at ELSE codex_quota_public_state.latest_created_at END,
               event_kind=CASE WHEN excluded.latest_seen_id IS NOT NULL THEN excluded.event_kind ELSE codex_quota_public_state.event_kind END,
               decision=excluded.decision,
               event_url=CASE WHEN excluded.latest_seen_id IS NOT NULL THEN excluded.event_url ELSE codex_quota_public_state.event_url END,
               latest_seen_id=COALESCE(excluded.latest_seen_id,codex_quota_public_state.latest_seen_id)",
            params![source, current_id, now, result.error, result.http_ok, parse_ok, latest_text, latest_created_at, event_kind, decision, event_url, latest_seen_id],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn add_column_if_missing(connection: &Connection, table: &str, definition: &str) -> Result<()> {
    let column = definition.split_whitespace().next().unwrap_or_default();
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !exists {
        connection.execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), [])?;
    }
    Ok(())
}

fn load_source_diagnostics(connection: &Connection) -> Result<Vec<PublicSourceDiagnostic>> {
    [
        (PRIMARY_SOURCE, PRIMARY_URL),
        (SECONDARY_SOURCE, SECONDARY_URL),
        (TIBO_SOURCE, TIBO_URL),
    ]
    .into_iter()
    .map(|(source, url)| -> Result<PublicSourceDiagnostic> {
        Ok(connection
            .query_row(
                "SELECT http_ok,parse_ok,checked_at,COALESCE(latest_seen_id,last_event_id),latest_text,latest_created_at,event_url,event_kind,decision,last_error
                 FROM codex_quota_public_state WHERE source=?1",
                [source],
                |row| {
                    Ok(PublicSourceDiagnostic {
                        source: source.into(),
                        url: url.into(),
                        http_ok: row.get(0)?,
                        parse_ok: row.get(1)?,
                        checked_at: row.get(2)?,
                        latest_event_id: row.get(3)?,
                        latest_text: row.get(4)?,
                        latest_created_at: row.get(5)?,
                        latest_url: row.get(6)?,
                        event_kind: row.get(7)?,
                        decision: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "waiting".into()),
                        last_error: row.get(9)?,
                    })
                },
            )
            .optional()
            .map(|value| {
                value.unwrap_or(PublicSourceDiagnostic {
                    source: source.into(),
                    url: url.into(),
                    http_ok: false,
                    parse_ok: false,
                    checked_at: None,
                    latest_event_id: None,
                    latest_text: None,
                    latest_created_at: None,
                    latest_url: None,
                    event_kind: None,
                    decision: "waiting".into(),
                    last_error: None,
                })
            })?)
    })
    .collect()
}

fn source_fetch_from_tibo_timeline(timeline: TiboTimeline) -> SourceFetch {
    SourceFetch {
        source: TIBO_SOURCE,
        http_ok: true,
        reset: timeline.relevant,
        latest: Some(timeline.latest),
        error: None,
    }
}

fn parse_tibo_profile_html(html: &str) -> Option<TiboTimeline> {
    let ids = extract_tibo_timeline_ids(html);
    let posts = ids
        .iter()
        .filter_map(|id| extract_tibo_html_post(html, id))
        .collect::<Vec<_>>();
    build_tibo_timeline(posts)
}

fn extract_tibo_timeline_ids(html: &str) -> Vec<String> {
    let marker = "entries:tweet-";
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find(marker) {
        let start = cursor + offset + marker.len();
        let id = html[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        // The marker already advances cursor; an empty ID must not skip a byte
        // past EOF or into a multi-byte character.
        cursor = start + id.len();
        if !id.is_empty() && seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    ids
}

fn extract_tibo_html_post(html: &str, event_id: &str) -> Option<ObservedPost> {
    let marker = format!("rest_id:\"{event_id}\"");
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find(&marker) {
        let start = cursor + offset;
        let end = ceil_char_boundary(html, (start + 20_000).min(html.len()));
        let block = &html[start..end];
        let created_at_ms = extract_ascii_number(block, "created_at_ms:");
        let text = extract_js_string(block, "full_text:\"");
        if let (Some(created_at_ms), Some(text)) = (created_at_ms, text) {
            let created_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(created_at_ms)
                .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
            return Some(ObservedPost {
                event_id: event_id.to_string(),
                kind: classify_tibo_reset_text(&text),
                text,
                created_at,
                url: format!("{TIBO_URL}/status/{event_id}"),
            });
        }
        cursor = start + marker.len();
    }
    None
}

fn extract_ascii_number(text: &str, marker: &str) -> Option<i64> {
    let start = text.find(marker)? + marker.len();
    let value = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!value.is_empty()).then(|| value.parse().ok()).flatten()
}

fn extract_js_string(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let value = &text[start..];
    let mut escaped = false;
    for (offset, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            let quoted = format!("\"{}\"", &value[..offset]);
            return serde_json::from_str(&quoted).ok();
        }
    }
    None
}

fn parse_tibo_jina_markdown(markdown: &str) -> Option<TiboTimeline> {
    let marker = "https://x.com/thsottiaux/status/";
    let mut posts = Vec::new();
    let mut seen = HashSet::new();
    for line in markdown.lines() {
        let Some(offset) = line.find(marker) else {
            continue;
        };
        let id_start = offset + marker.len();
        let event_id = line[id_start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        if event_id.is_empty() || !seen.insert(event_id.clone()) {
            continue;
        }
        let url_end = id_start + event_id.len();
        let text_start = line[url_end..]
            .find(')')
            .map(|position| url_end + position + 1)
            .unwrap_or(url_end);
        let text = normalize_visible_text(&line[text_start..]);
        if text.is_empty() {
            continue;
        }
        posts.push(ObservedPost {
            event_id: event_id.clone(),
            kind: classify_tibo_reset_text(&text),
            text,
            created_at: None,
            url: format!("{TIBO_URL}/status/{event_id}"),
        });
    }
    build_tibo_timeline(posts)
}

fn normalize_visible_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_tibo_timeline(posts: Vec<ObservedPost>) -> Option<TiboTimeline> {
    let latest = posts.first()?.clone();
    let relevant = posts.iter().find_map(|post| {
        post.kind.map(|kind| PublicReset {
            source: TIBO_SOURCE,
            event_id: post.event_id.clone(),
            text: post.text.clone(),
            created_at: post.created_at.clone(),
            url: post.url.clone(),
            kind,
        })
    });
    Some(TiboTimeline { latest, relevant })
}

fn parse_primary_source(html: &str) -> Option<PublicReset> {
    parse_latest_reset_signal(html, PRIMARY_SOURCE)
}

fn parse_secondary_source(html: &str) -> Option<PublicReset> {
    parse_latest_reset_signal(html, SECONDARY_SOURCE)
}

fn parse_latest_reset_signal(html: &str, source: &'static str) -> Option<PublicReset> {
    let mut candidates = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find("https://x.com/") {
        let start = cursor + offset;
        let rest = &html[start..];
        let end = rest
            .find(['\"', '\'', '<', ' ', '\\'])
            .unwrap_or(rest.len());
        let url = rest[..end].replace("&amp;", "&");
        cursor = start + end.max(1);
        if !url.contains("/status/") {
            continue;
        }
        let Some(event_id) = status_id(&url) else {
            continue;
        };
        let (block, relative_url_start) = surrounding_event_block(html, start);
        let visible = strip_html(block);
        let text = extract_signal_text(block, relative_url_start)
            .filter(|value| classify_reset_text(value).is_some())
            .or_else(|| extract_relevant_excerpt(&visible))
            .unwrap_or_else(|| truncate_text(&visible, 360));
        let Some(kind) = classify_reset_text(&text).or_else(|| classify_reset_text(&visible))
        else {
            continue;
        };
        let created_at = extract_attribute(block, "dateTime=\"")
            .or_else(|| extract_attribute(block, "data-datetime=\""));
        candidates.push(PublicReset {
            source,
            event_id,
            text,
            created_at,
            url,
            kind,
        });
    }
    candidates.into_iter().max_by(|left, right| {
        numeric_status_id(&left.event_id)
            .cmp(&numeric_status_id(&right.event_id))
            .then_with(|| left.event_id.cmp(&right.event_id))
    })
}

fn numeric_status_id(value: &str) -> u128 {
    value.parse().unwrap_or_default()
}

fn surrounding_event_block(html: &str, url_start: usize) -> (&str, usize) {
    let before = &html[..url_start];
    let start = before
        .rfind("<article")
        .or_else(|| before.rfind("<li"))
        .filter(|position| url_start.saturating_sub(*position) <= 3_000)
        .unwrap_or_else(|| floor_char_boundary(html, url_start.saturating_sub(1_200)));
    let after = &html[url_start..];
    let end = after
        .find("</article>")
        .map(|offset| url_start + offset + "</article>".len())
        .or_else(|| {
            after
                .find("</li>")
                .map(|offset| url_start + offset + "</li>".len())
        })
        .unwrap_or_else(|| ceil_char_boundary(html, (url_start + 1_200).min(html.len())));
    (&html[start..end], url_start - start)
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn extract_signal_text(block: &str, url_start: usize) -> Option<String> {
    let before_url = &block[..url_start.min(block.len())];
    let paragraph_start = before_url.rfind("<p")?;
    let paragraph = &before_url[paragraph_start..];
    let content_start = paragraph.find('>')? + 1;
    let content_end = paragraph[content_start..]
        .find("</p>")
        .map(|offset| content_start + offset)
        .unwrap_or(paragraph.len());
    let text = strip_html(&paragraph[content_start..content_end]);
    (!text.is_empty()).then_some(text)
}

fn extract_relevant_excerpt(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let position = [
        "limits have been reset",
        "usage limits have been reset",
        "nice reset",
        "landing in the next hour",
        "reset incoming",
        "reset is coming",
        "reset coming",
        "about to reset",
        "reset soon",
    ]
    .iter()
    .filter_map(|keyword| lower.find(keyword))
    .min()?;
    let start = floor_char_boundary(text, position.saturating_sub(140));
    let end = ceil_char_boundary(text, (position + 260).min(text.len()));
    let excerpt = text[start..end]
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(character, ',' | ':' | '"' | '\'' | '{' | '}' | '[' | ']')
        })
        .to_string();
    (!excerpt.is_empty()).then_some(excerpt)
}

fn page_structure_recognized(source: &str, html: &str) -> bool {
    let lower = html.to_lowercase();
    match source {
        PRIMARY_SOURCE => {
            lower.contains("latest reset evidence")
                || lower.contains("latest from monitored accounts")
                || lower.contains("codex reset monitor")
        }
        SECONDARY_SOURCE => {
            lower.contains("last codex reset")
                || lower.contains("temporary signal")
                || lower.contains("codex resets")
        }
        _ => false,
    }
}

fn status_id(url: &str) -> Option<String> {
    url.split("/status/")
        .nth(1)?
        .split(['?', '#', '/'])
        .next()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn extract_attribute(text: &str, marker: &str) -> Option<String> {
    let rest = text.split(marker).nth(1)?;
    Some(rest.split('\"').next()?.to_string())
}

fn decode_html(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#x27;", "'")
}

fn strip_html(text: &str) -> String {
    let decoded = decode_html(text);
    let mut visible = String::with_capacity(decoded.len());
    let mut inside_tag = false;
    for character in decoded.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => {
                inside_tag = false;
                visible.push(' ');
            }
            _ if !inside_tag => visible.push(character),
            _ => {}
        }
    }
    visible.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut characters = text.chars();
    let mut output = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        output.push('…');
    }
    output
}

fn classify_reset_text(text: &str) -> Option<&'static str> {
    let text = text.to_lowercase();
    let confirmed = [
        "limits have been reset",
        "usage limits have been reset",
        "we reset usage",
        "reset usage limits",
        "global codex quota reset",
        "reset has landed",
        "reset is live",
        "reset completed",
    ];
    if confirmed.iter().any(|keyword| text.contains(keyword)) {
        return Some("public_confirmed");
    }
    let announced = [
        "nice reset",
        "landing in the next hour",
        "reset incoming",
        "reset is coming",
        "reset coming",
        "about to reset",
        "reset soon",
    ];
    announced
        .iter()
        .any(|keyword| text.contains(keyword))
        .then_some("public_announcement")
}

fn classify_tibo_reset_text(text: &str) -> Option<&'static str> {
    if let Some(kind) = classify_reset_text(text) {
        return Some(kind);
    }
    let text = text.to_lowercase();
    if text.contains("occasional resets") {
        return None;
    }
    let suspected = [
        "new reset button",
        "fancy reset button",
        "gifted a reset button",
        "time for a reset",
        "ready to reset",
        "trigger a reset",
        "another reset today",
        "reset tonight",
        "reset tomorrow",
        "quota refill",
        "credits refill",
        "refill codex",
        "more codex credits",
        "extra codex credits",
        "boost codex limits",
        "raise codex limits",
        "increase codex limits",
    ];
    suspected
        .iter()
        .any(|keyword| text.contains(keyword))
        .then_some("public_suspected")
}

fn is_account_reset(before: f64, after: f64) -> bool {
    (before <= 25.0 && after >= 75.0) || after - before >= 50.0
}

fn should_notify_public_reset(previous: Option<&Option<String>>, current: &str) -> bool {
    matches!(previous, Some(value) if value.as_deref() != Some(current))
}

fn is_recent_public_event(item: &PublicReset, now: i64) -> bool {
    const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1_000;
    const CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
    let Some(created_at) = item.created_at.as_deref() else {
        return false;
    };
    let Ok(created_at) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return false;
    };
    let age = now - created_at.timestamp_millis();
    (-CLOCK_SKEW_MS..=MAX_AGE_MS).contains(&age)
}

fn stable_account_id(account: &str) -> u64 {
    account
        .bytes()
        .fold(14_695_981_039_346_656_037_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_timeline_markers_preserve_utf8_boundaries_and_later_ids() {
        for input in ["entries:tweet-", "entries:tweet-中文", "entries:tweet-🐳"] {
            assert!(extract_tibo_timeline_ids(input).is_empty());
            assert_eq!(extract_tibo_timeline_ids(&format!("{input} entries:tweet-123 entries:tweet-123")), vec!["123"]);
        }
        assert_eq!(extract_tibo_timeline_ids("entries:tweet-entries:tweet-456"), vec!["456"]);
    }

    #[test]
    fn account_reset_requires_both_quota_windows_to_recover() {
        let monitor = in_memory_public_monitor();
        monitor.connection.lock().unwrap().execute_batch(
            "CREATE TABLE codex_quota_account_state(account TEXT PRIMARY KEY, used_percent REAL, reset_label TEXT, checked_at INTEGER NOT NULL);"
        ).unwrap();
        let account = |weekly, short| crate::codex_accounts::CodexAccountSnapshot {
            installed: true, warnings: vec![], message: None,
            accounts: vec![crate::codex_accounts::CodexAccount {
                index: 1, account: "fixture@example.com".into(), plan: None,
                next_used_percent: Some(short), next_reset: None,
                total_used_percent: Some(weekly), total_reset: None,
                current: true, local_status: None, refresh_error: None,
            }],
        };
        let mut events = Vec::new();
        monitor.check_accounts(account(0.0, 0.0), 1, &mut events).unwrap();
        monitor.check_accounts(account(100.0, 0.0), 2, &mut events).unwrap();
        assert!(events.is_empty(), "one exhausted window still blocks the account");
        monitor.check_accounts(account(100.0, 95.0), 3, &mut events).unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].text.contains("95%"));
        monitor.check_accounts(account(100.0, 95.0), 4, &mut events).unwrap();
        assert_eq!(events.len(), 1, "unchanged quotas must not notify again");
    }

    fn in_memory_public_monitor() -> CodexQuotaMonitor {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE codex_quota_public_state(
                   source TEXT PRIMARY KEY,last_event_id TEXT,checked_at INTEGER NOT NULL,last_error TEXT,
                   http_ok INTEGER NOT NULL DEFAULT 0,parse_ok INTEGER NOT NULL DEFAULT 0,
                   latest_text TEXT,latest_created_at TEXT,event_kind TEXT,decision TEXT,event_url TEXT,latest_seen_id TEXT
                 );
                 CREATE TABLE codex_quota_free_notifications(
                   event_id TEXT PRIMARY KEY,source TEXT NOT NULL,event_text TEXT NOT NULL,created_at TEXT,
                   notified_at INTEGER NOT NULL,event_kind TEXT,event_url TEXT
                 );",
            )
            .unwrap();
        CodexQuotaMonitor {
            connection: Arc::new(Mutex::new(connection)),
            client: Client::new(),
        }
    }

    fn reset_item(source: &'static str, event_id: &str, kind: &'static str) -> PublicReset {
        PublicReset {
            source,
            event_id: event_id.into(),
            text: format!("reset {event_id}"),
            created_at: None,
            url: format!("https://x.com/thsottiaux/status/{event_id}"),
            kind,
        }
    }

    fn reset_fetch(source: &'static str, event_id: &str, kind: &'static str) -> SourceFetch {
        let reset = reset_item(source, event_id, kind);
        SourceFetch {
            source,
            http_ok: true,
            latest: Some(ObservedPost::from(&reset)),
            reset: Some(reset),
            error: None,
        }
    }

    fn reset_fetch_at(
        source: &'static str,
        event_id: &str,
        kind: &'static str,
        created_at: String,
    ) -> SourceFetch {
        let mut fetch = reset_fetch(source, event_id, kind);
        fetch.reset.as_mut().unwrap().created_at = Some(created_at.clone());
        fetch.latest.as_mut().unwrap().created_at = Some(created_at);
        fetch
    }

    #[test]
    fn parses_tibo_profile_timeline_without_confusing_user_id_for_tweet_id() {
        let html = r#"
          entries:tweet-2089941380336644295
          entries:tweet-2089149255382438340
          rest_id:"1953337039510003712",created_at_ms:1787121600000,full_text:"profile"
          rest_id:"2089941380336644295",created_at_ms:1787125428000,full_text:"I was gifted a very fancy new reset button today"
          rest_id:"2089149255382438340",created_at_ms:1786926971000,full_text:"Codex\\n\\nAlmost 100% reliable. Occasional resets."
        "#;
        let timeline = parse_tibo_profile_html(html).unwrap();
        assert_eq!(timeline.latest.event_id, "2089941380336644295");
        let reset = timeline.relevant.unwrap();
        assert_eq!(reset.event_id, "2089941380336644295");
        assert_eq!(reset.kind, "public_suspected");
        assert!(reset.text.contains("reset button"));
    }

    #[test]
    fn tibo_classifier_ignores_generic_reliability_copy() {
        assert_eq!(
            classify_tibo_reset_text("Codex is reliable with occasional resets"),
            None
        );
        assert_eq!(
            classify_tibo_reset_text("I was gifted a very fancy new reset button today"),
            Some("public_suspected")
        );
        assert_eq!(
            classify_tibo_reset_text("Usage limits have been reset for paid Codex users"),
            Some("public_confirmed")
        );
    }

    #[test]
    fn parses_tibo_jina_markdown_as_fallback() {
        let markdown = r#"
          * [Tibo](https://x.com/thsottiaux) [1h](https://x.com/thsottiaux/status/2089941380336644295) I was gifted a very fancy new reset button today 1.6K 229
          * [Tibo](https://x.com/thsottiaux) [Aug 17](https://x.com/thsottiaux/status/2089500941842342287) What should we do with Codex next?
        "#;
        let timeline = parse_tibo_jina_markdown(markdown).unwrap();
        assert_eq!(timeline.latest.event_id, "2089941380336644295");
        assert_eq!(timeline.relevant.unwrap().kind, "public_suspected");
    }

    #[test]
    fn tibo_source_baselines_deduplicates_and_preserves_relevant_cursor() {
        let monitor = in_memory_public_monitor();
        let mut events = Vec::new();
        monitor
            .record_public_result(
                reset_fetch(TIBO_SOURCE, "100", "public_suspected"),
                1,
                &mut events,
            )
            .unwrap();
        assert!(events.is_empty(), "首次检查只建立基线");

        monitor
            .record_public_result(
                reset_fetch(TIBO_SOURCE, "101", "public_confirmed"),
                2,
                &mut events,
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "public:101");

        monitor
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO codex_quota_public_state(source,last_event_id,checked_at,decision) VALUES(?1,'99',1,'baseline')",
                [SECONDARY_SOURCE],
            )
            .unwrap();
        monitor
            .record_public_result(
                reset_fetch(SECONDARY_SOURCE, "101", "public_confirmed"),
                3,
                &mut events,
            )
            .unwrap();
        assert_eq!(events.len(), 1, "相同帖子跨来源只通知一次");
        let duplicate_decision: String = monitor
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT decision FROM codex_quota_public_state WHERE source=?1",
                [SECONDARY_SOURCE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(duplicate_decision, "duplicate");

        let latest = ObservedPost {
            event_id: "102".into(),
            text: "A normal Codex update".into(),
            created_at: None,
            url: "https://x.com/thsottiaux/status/102".into(),
            kind: None,
        };
        monitor
            .record_public_result(
                SourceFetch {
                    source: TIBO_SOURCE,
                    http_ok: true,
                    reset: Some(reset_item(TIBO_SOURCE, "101", "public_confirmed")),
                    latest: Some(latest),
                    error: None,
                },
                4,
                &mut events,
            )
            .unwrap();
        let state: (Option<String>, Option<String>, Option<String>, String) = monitor
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT last_event_id,latest_seen_id,event_kind,decision FROM codex_quota_public_state WHERE source=?1",
                [TIBO_SOURCE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(state.0.as_deref(), Some("101"));
        assert_eq!(state.1.as_deref(), Some("102"));
        assert_eq!(state.2, None);
        assert_eq!(state.3, "no_relevant_event");
    }

    #[test]
    fn tibo_fresh_baseline_is_notified_once_but_stale_history_is_not() {
        const HOUR_MS: i64 = 60 * 60 * 1_000;
        let now = 1_787_124_004_172_i64;
        let fresh_created_at =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now - 2 * HOUR_MS)
                .unwrap()
                .to_rfc3339();
        let monitor = in_memory_public_monitor();
        let mut events = Vec::new();
        monitor
            .record_public_result(
                reset_fetch_at(
                    TIBO_SOURCE,
                    "2089941380336644295",
                    "public_suspected",
                    fresh_created_at.clone(),
                ),
                now,
                &mut events,
            )
            .unwrap();
        assert_eq!(events.len(), 1, "24小时内的首次 Tibo 信号应立即通知");
        monitor
            .record_public_result(
                reset_fetch_at(
                    TIBO_SOURCE,
                    "2089941380336644295",
                    "public_suspected",
                    fresh_created_at,
                ),
                now + 1,
                &mut events,
            )
            .unwrap();
        assert_eq!(events.len(), 1, "相同帖子不能重复通知");

        let stale_monitor = in_memory_public_monitor();
        let stale_created_at =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now - 25 * HOUR_MS)
                .unwrap()
                .to_rfc3339();
        let mut stale_events = Vec::new();
        stale_monitor
            .record_public_result(
                reset_fetch_at(
                    TIBO_SOURCE,
                    "2089000000000000000",
                    "public_suspected",
                    stale_created_at,
                ),
                now,
                &mut stale_events,
            )
            .unwrap();
        assert!(stale_events.is_empty(), "超过24小时的历史帖子只建立基线");
    }

    #[test]
    fn detects_real_account_reset_only() {
        assert!(is_account_reset(0.0, 95.0));
        assert!(is_account_reset(20.0, 82.0));
        assert!(!is_account_reset(30.0, 55.0));
        assert!(!is_account_reset(0.0, 15.0));
    }

    #[test]
    fn parses_primary_confirmed_reset() {
        let html = r#"Latest reset evidence <article data-testid="x-evidence-post"><p data-testid="confirmed-source-text">Usage limits have been reset for Codex.</p><a href="https://x.com/thsottiaux/status/12345">View original source</a><time dateTime="2026-08-12T01:02:03.000Z">"#;
        let reset = parse_primary_source(html).unwrap();
        assert_eq!(reset.event_id, "12345");
        assert!(reset.text.contains("Usage limits"));
    }

    #[test]
    fn primary_prefers_latest_announcement_over_stale_confirmation() {
        let html = r#"
          <section><h2>Latest from monitored accounts</h2>
            <article><p>Enjoy a nice reset everyone. Landing in the next hour or so.</p>
            <a href="https://x.com/thsottiaux/status/2087706104814023111">Open post</a>
            <time dateTime="2026-08-13T01:01:00.000Z"></time></article>
          </section>
          <section><h2>Latest reset evidence</h2>
            <article><p>Usage limits have been reset for paid users.</p>
            <a href="https://x.com/thsottiaux/status/2086972933566857393">Open post</a></article>
          </section>"#;
        let reset = parse_primary_source(html).unwrap();
        assert_eq!(reset.event_id, "2087706104814023111");
        assert_eq!(reset.kind, "public_announcement");
        assert!(reset.text.contains("Landing in the next hour"));
    }

    #[test]
    fn secondary_rejects_prediction_and_accepts_confirmation() {
        let prediction = r#"LAST CODEX RESET FROM TIBO <p class="hero-tweet">Little surprise for you tomorrow.</p><a href="https://x.com/thsottiaux/status/111">"#;
        assert!(parse_secondary_source(prediction).is_none());
        let reset = r#"LAST CODEX RESET FROM TIBO <p class="hero-tweet">The usage limits have been reset for all paid users.</p><a href="https://x.com/thsottiaux/status/222"><span data-datetime="2026-08-12T01:02:03.000Z">"#;
        assert_eq!(parse_secondary_source(reset).unwrap().event_id, "222");
    }

    #[test]
    fn secondary_reads_temporary_reset_signal_without_legacy_class_name() {
        let html = r#"
          <section><h2>TEMPORARY SIGNAL / UNCONFIRMED</h2>
            <article><p>Enjoy a nice reset everyone. Landing in the next hour or so.</p>
            <a href="https://x.com/thsottiaux/status/2087706104814023111">Original</a>
            <time data-datetime="2026-08-13T01:01:00.000Z"></time></article>
          </section>
          <section><h2>LAST CODEX RESET FROM TIBO</h2>
            <p>The usage limits have been reset for all paid users.</p>
            <a href="https://x.com/thsottiaux/status/2086972933566857393">Old</a>
          </section>"#;
        let reset = parse_secondary_source(html).unwrap();
        assert_eq!(reset.event_id, "2087706104814023111");
        assert_eq!(reset.kind, "public_announcement");
    }

    #[test]
    fn first_public_check_builds_baseline_without_notification() {
        assert!(!should_notify_public_reset(None, "100"));
        assert!(should_notify_public_reset(Some(&None), "100"));
        assert!(!should_notify_public_reset(
            Some(&Some("100".into())),
            "100"
        ));
        assert!(should_notify_public_reset(Some(&Some("99".into())), "100"));
    }

    #[test]
    fn migrates_existing_notification_database_without_losing_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE codex_quota_monitor_state(id INTEGER PRIMARY KEY,last_checked_at INTEGER,last_error TEXT,public_sources_online INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE codex_quota_public_state(source TEXT PRIMARY KEY,last_event_id TEXT,checked_at INTEGER NOT NULL,last_error TEXT);
             CREATE TABLE codex_quota_free_notifications(event_id TEXT PRIMARY KEY,source TEXT NOT NULL,event_text TEXT NOT NULL,created_at TEXT,notified_at INTEGER NOT NULL);
             INSERT INTO codex_quota_free_notifications VALUES('public:old','codexreset.org','old event',NULL,1);",
        ).unwrap();
        add_column_if_missing(
            &connection,
            "codex_quota_monitor_state",
            "public_sources_parsed INTEGER NOT NULL DEFAULT 0",
        )
        .unwrap();
        add_column_if_missing(
            &connection,
            "codex_quota_public_state",
            "parse_ok INTEGER NOT NULL DEFAULT 0",
        )
        .unwrap();
        add_column_if_missing(
            &connection,
            "codex_quota_public_state",
            "latest_seen_id TEXT",
        )
        .unwrap();
        add_column_if_missing(
            &connection,
            "codex_quota_free_notifications",
            "event_kind TEXT",
        )
        .unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM codex_quota_free_notifications",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT public_sources_parsed FROM codex_quota_monitor_state WHERE id=1",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .optional()
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    #[ignore = "requires public internet access"]
    async fn live_public_sources_remain_parseable() {
        let client = Client::builder()
            .user_agent("CrewVela parser check")
            .build()
            .unwrap();
        let primary = client
            .get(PRIMARY_URL)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let secondary = client
            .get(SECONDARY_URL)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let tibo = client
            .get(TIBO_URL)
            .header(reqwest::header::USER_AGENT, X_BROWSER_USER_AGENT)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let primary = parse_primary_source(&primary).unwrap();
        let secondary = parse_secondary_source(&secondary).unwrap();
        let tibo = parse_tibo_profile_html(&tibo).unwrap();
        assert!(numeric_status_id(&primary.event_id) >= 2_087_706_104_814_023_111);
        assert!(numeric_status_id(&secondary.event_id) >= 2_087_706_104_814_023_111);
        assert!(matches!(
            primary.kind,
            "public_announcement" | "public_confirmed"
        ));
        assert!(matches!(
            secondary.kind,
            "public_announcement" | "public_confirmed"
        ));
        assert!(!primary.text.is_empty());
        assert!(!secondary.text.is_empty());
        assert!(!tibo.latest.text.is_empty());
        assert!(numeric_status_id(&tibo.latest.event_id) > 2_089_000_000_000_000_000);
    }
}
