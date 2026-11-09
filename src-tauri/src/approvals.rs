//! 会话权限审批：Ask/Allow/Deny 三档策略与自动代答。
//!
//! `Ask`（默认）的等待会话经 `collect_approval_cards` 汇总到统一干预中心出卡片，人工决定；
//! `Allow` / `Deny` 的等待会话由 `ApprovalAutoResponder` 在 pty_poll_all 轮询中直接代答。
//! 同一轮等待（waiting_since 不变）只处理一次：观察到即登记，即使当时策略是 Ask——
//! 之后把策略改成 Allow/Deny 也不会追溯代答已在卡片里挂着的那次等待。

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::pty::PtyManager;
use crate::sessions::{ApprovalPolicy, SessionKind, SessionManager, SessionStatus};

/// 对权限提示的决定：允许选提示的第 1 项（Yes），拒绝用 Esc 退回。
/// `1\r` 与 pty.rs trust 对话框自动确认同一先例（选 Yes + 回车，末尾回车落在
/// 输入框上是空提交，claude 视为 no-op）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

impl ApprovalDecision {
    pub fn keys(self) -> &'static [u8] {
        match self {
            Self::Allow => b"1\r",
            Self::Deny => b"\x1b",
        }
    }
}

/// 待人工审批的会话卡片（统一干预中心「会话审批」区的数据源）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalCard {
    pub session_id: String,
    pub pty_id: u32,
    pub title: String,
    pub provider_name: String,
    pub cwd: String,
    pub project_id: Option<String>,
    pub multi_agent_project_id: Option<String>,
    pub waiting_since: u64,
    pub waiting_reason: Option<String>,
    pub approval_policy: ApprovalPolicy,
}

pub fn collect_approval_cards(sessions: &SessionManager) -> Vec<ApprovalCard> {
    sessions
        .list()
        .into_iter()
        .filter(|session| {
            session.status == SessionStatus::Waiting
                && session.kind == SessionKind::Claude
                && session.pty_id.is_some()
        })
        .map(|session| ApprovalCard {
            session_id: session.id,
            pty_id: session.pty_id.unwrap_or_default(),
            title: session.title,
            provider_name: session.provider_name,
            cwd: session.cwd,
            project_id: session.project_id,
            multi_agent_project_id: session.multi_agent_project_id,
            waiting_since: session.waiting_since.unwrap_or(session.updated_at),
            waiting_reason: session.waiting_reason.or(session.last_error),
            approval_policy: session.approval_policy,
        })
        .collect()
}

/// Allow/Deny 策略的自动代答器。内存态去重：session_id -> 已处理的 waiting_since。
#[derive(Default)]
pub struct ApprovalAutoResponder {
    handled_since: Mutex<HashMap<String, u64>>,
}

impl ApprovalAutoResponder {
    /// pty_poll_all 每轮调用：对策略非 Ask 且本轮等待尚未处理过的会话写键。
    pub fn flush(&self, pty: &PtyManager, sessions: &SessionManager, queue: &crate::control::SessionActionQueue) {
        let snapshot = sessions.list();
        for session in snapshot.iter().filter(|session| session.status == SessionStatus::Waiting) {
            queue.hold_pending_submit_for_blocker(&session.id);
        }
        // Keep the same snapshot, and acquire no queue locks while the session
        // approval gate is held (handoff acquires those locks in reverse order).
        let _ = self.flush_snapshot_with(sessions, snapshot, &mut |session_id, pty_id, keys| {
            pty.write_for_app_session_bounded(pty_id, session_id, keys, std::time::Duration::from_secs(2))
        });
    }

    #[cfg(test)]
    fn flush_with(
        &self,
        sessions: &SessionManager,
        writer: &mut dyn FnMut(&str, u32, &[u8]) -> Result<()>,
    ) -> Vec<(u32, ApprovalDecision)> {
        self.flush_snapshot_with(sessions, sessions.list(), writer)
    }

    fn flush_snapshot_with(&self, sessions: &SessionManager, snapshot: Vec<crate::sessions::SessionRecord>, writer: &mut dyn FnMut(&str, u32, &[u8]) -> Result<()>) -> Vec<(u32, ApprovalDecision)> {
        let mut responses = Vec::new();
        for session in snapshot {
            if session.status != SessionStatus::Waiting
                || session.kind != SessionKind::Claude
                || session.yolo
            {
                continue;
            }
            let Some(since) = session.waiting_since else {
                continue;
            };
            {
                let mut handled = self.handled_since.lock().unwrap();
                if handled.get(&session.id) == Some(&since) { continue; }
                handled.insert(session.id.clone(), since);
            }
            let decision = match session.approval_policy {
                ApprovalPolicy::Allow => ApprovalDecision::Allow,
                ApprovalPolicy::Deny => ApprovalDecision::Deny,
                // Ask：登记「已见」即可，本轮等人工卡片决定；
                // 策略中途切换不追溯已在卡片里挂着的这次等待。
                ApprovalPolicy::Ask => continue,
            };
            let Some(pty_id) = session.pty_id else {
                continue;
            };
            if let Err(error) = sessions.respond_waiting_approval(&session.id, pty_id, since, decision == ApprovalDecision::Allow, |pty| writer(&session.id, pty, decision.keys())) {
                eprintln!(
                    "[crewvela] 审批自动代答写入失败 session={} pty={pty_id} err={error:#}",
                    session.id
                );
                continue;
            }
            eprintln!(
                "[crewvela] 审批自动代答 session={} decision={decision:?}",
                session.id
            );
            responses.push((pty_id, decision));
        }
        // 等待已结束（状态离开 Waiting 或新一轮等待换了时间戳）的登记项不再保留。
        self.handled_since.lock().unwrap().retain(|session_id, since| {
            sessions
                .get(session_id)
                .and_then(|session| session.waiting_since)
                .is_some_and(|current| current == *since)
        });
        responses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{NewSession, SessionRecord};
    use std::cell::RefCell;

    fn waiting_session(name: &str) -> (SessionManager, String) {
        let root = std::env::temp_dir().join(format!(
            "crewvela-approval-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 7).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
                "message": "Claude needs your permission to use Bash"
            }))
            .unwrap();
        let since = manager.get(&created.id).unwrap().waiting_since;
        assert!(since.is_some());
        (manager, root.display().to_string())
    }

    fn cleanup(root: &str) {
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn automatic_stale_snapshot_cannot_write_to_a_new_pty_or_waiting_generation() {
        let (manager, root) = waiting_session("auto-stale-snapshot");
        let id = manager.list()[0].id.clone();
        manager.set_approval_policy(&id, ApprovalPolicy::Allow).unwrap();
        let old = manager.list();
        let old_since = old[0].waiting_since.unwrap();
        manager.mark_terminal_submission(7).unwrap();
        manager.attach_runtime(&id, 8).unwrap();
        manager.apply_hook_value(&serde_json::json!({"session_id":old[0].claude_session_id,"hook_event_name":"Notification","notification_type":"permission_prompt"})).unwrap();
        assert!(manager.get(&id).unwrap().waiting_since.unwrap() > old_since);
        let responder = ApprovalAutoResponder::default();
        let writes = std::cell::RefCell::new(Vec::new());
        let mut writer = |owner: &str, pty: u32, keys: &[u8]| { writes.borrow_mut().push((owner.to_string(), pty, keys.to_vec())); Ok(()) };
        assert!(responder.flush_snapshot_with(&manager, old, &mut writer).is_empty());
        assert!(writes.borrow().is_empty());
        assert_eq!(manager.get(&id).unwrap().status, SessionStatus::Waiting);
        let current = manager.list();
        assert_eq!(responder.flush_with(&manager, &mut writer).len(), 1);
        assert_eq!(*writes.borrow(), vec![(id.clone(), 8, b"1\r".to_vec())]);
        assert!(responder.flush_snapshot_with(&manager, current, &mut writer).is_empty());
        assert_eq!(writes.borrow().len(), 1);
        cleanup(&root);
    }

    #[test]
    fn automatic_failed_input_consumes_the_generation_without_replaying() {
        let (manager, root) = waiting_session("auto-input-uncertain");
        let id = manager.list()[0].id.clone();
        manager.set_approval_policy(&id, ApprovalPolicy::Deny).unwrap();
        let responder = ApprovalAutoResponder::default();
        let mut attempts = 0;
        let mut writer = |_: &str, _: u32, _: &[u8]| { attempts += 1; Err(anyhow::anyhow!("fixture uncertain pipe")) };
        assert!(responder.flush_with(&manager, &mut writer).is_empty());
        assert!(responder.flush_with(&manager, &mut writer).is_empty());
        assert_eq!(attempts, 1);
        let after = manager.get(&id).unwrap();
        assert!(after.waiting_since.is_none());
        assert!(after.last_error.unwrap().contains("当前代次已消费"));
        cleanup(&root);
    }

    #[test]
    fn allow_policy_answers_once_per_waiting_round() {
        let (manager, root) = waiting_session("allow-once");
        manager
            .set_approval_policy(
                &manager.list()[0].id,
                crate::sessions::ApprovalPolicy::Allow,
            )
            .unwrap();
        let responder = ApprovalAutoResponder::default();
        let writes = RefCell::new(Vec::new());
        let mut writer = |_: &str, _: u32, keys: &[u8]| {
            writes.borrow_mut().push(keys.to_vec());
            Ok(())
        };

        let responses = responder.flush_with(&manager, &mut writer);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].1, ApprovalDecision::Allow);
        assert_eq!(*writes.borrow(), vec![b"1\r".to_vec()]);

        // 同一轮等待不重复代答。
        assert!(responder.flush_with(&manager, &mut writer).is_empty());

        // 新一轮等待（新时间戳）重新代答。
        std::thread::sleep(std::time::Duration::from_millis(2));
        let session_id = manager.list()[0].id.clone();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": manager.get(&session_id).unwrap().claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
                "message": "Claude needs your permission to use Bash"
            }))
            .unwrap();
        assert_eq!(responder.flush_with(&manager, &mut writer).len(), 1);
        assert_eq!(writes.borrow().len(), 2);

        cleanup(&root);
    }

    #[test]
    fn deny_policy_sends_escape() {
        let (manager, root) = waiting_session("deny-esc");
        manager
            .set_approval_policy(&manager.list()[0].id, crate::sessions::ApprovalPolicy::Deny)
            .unwrap();
        let responder = ApprovalAutoResponder::default();
        let writes = RefCell::new(Vec::new());
        let mut writer = |_: &str, _: u32, keys: &[u8]| {
            writes.borrow_mut().push(keys.to_vec());
            Ok(())
        };

        let responses = responder.flush_with(&manager, &mut writer);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].1, ApprovalDecision::Deny);
        assert_eq!(*writes.borrow(), vec![b"\x1b".to_vec()]);

        cleanup(&root);
    }

    #[test]
    fn ask_round_seen_before_policy_switch_is_not_retro_answered() {
        let (manager, root) = waiting_session("ask-no-retro");
        let session_id = manager.list()[0].id.clone();
        let responder = ApprovalAutoResponder::default();
        let writes = RefCell::new(Vec::new());
        let mut writer = |_: &str, _: u32, keys: &[u8]| {
            writes.borrow_mut().push(keys.to_vec());
            Ok(())
        };

        // Ask 时先观察到这轮等待：只登记，不代答。
        assert!(responder.flush_with(&manager, &mut writer).is_empty());

        // 切到 Allow 不追溯本轮；卡片仍由人工处理。
        manager
            .set_approval_policy(&session_id, crate::sessions::ApprovalPolicy::Allow)
            .unwrap();
        assert!(responder.flush_with(&manager, &mut writer).is_empty());
        assert!(writes.borrow().is_empty());

        // 下一轮新等待开始自动代答。
        std::thread::sleep(std::time::Duration::from_millis(2));
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": manager.get(&session_id).unwrap().claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt"
            }))
            .unwrap();
        assert_eq!(responder.flush_with(&manager, &mut writer).len(), 1);

        cleanup(&root);
    }

    #[test]
    fn cards_collect_only_waiting_claude_sessions() {
        let (manager, root) = waiting_session("cards");
        let cards = collect_approval_cards(&manager);
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].waiting_reason.as_deref(),
            Some("Claude needs your permission to use Bash")
        );
        assert_eq!(cards[0].approval_policy, ApprovalPolicy::Ask);

        // 回答后（状态翻回 Running）不再出卡片。
        manager.mark_terminal_submission(cards[0].pty_id).unwrap();
        assert!(collect_approval_cards(&manager).is_empty());

        cleanup(&root);
    }

    #[test]
    fn session_record_defaults_keep_ask_policy_after_roundtrip() {
        let (manager, root) = waiting_session("roundtrip");
        let session: SessionRecord = manager.list()[0].clone();
        assert_eq!(session.approval_policy, ApprovalPolicy::Ask);
        assert!(session.waiting_reason.is_some());
        assert!(session.waiting_since.is_some());
        cleanup(&root);
    }
}
