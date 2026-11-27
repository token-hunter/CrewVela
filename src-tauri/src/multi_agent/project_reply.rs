use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::MultiAgentStore;

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_project_reply_preferences (
            project_id TEXT PRIMARY KEY REFERENCES ma_projects(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE
        );",
    )?;
    Ok(())
}

pub(super) fn preferred_member(conn: &Connection, project: &str) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT a.id FROM ma_project_reply_preferences p
         JOIN ma_agents a ON a.id=p.agent_id AND a.project_id=p.project_id
         WHERE p.project_id=?1 AND a.removed_at IS NULL",
        [project], |row| row.get(0),
    ).optional()?)
}

impl MultiAgentStore {
    pub(crate) fn set_project_reply_preference(&self, project: &str, agent: Option<&str>) -> Result<()> {
        let candidate = agent.map(|id| self.agent_profile(id)).transpose()?;
        if let Some(member) = &candidate {
            ensure!(member.project_id == project && member.enabled && member.removed_at.is_none(), "所选答复成员当前不可用");
            ensure!(member.runtime_kind.supports_existing_collaboration(), "所选成员的 Runtime 尚未接入群聊");
            member.runtime_kind.ensure_available()?;
        }
        let mut conn = self.connection.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_projects WHERE id=?1)", [project], |row| row.get(0))?;
        ensure!(exists, "协作项目不存在");
        if let Some(member) = candidate {
            let saved = tx.execute(
                "INSERT INTO ma_project_reply_preferences(project_id,agent_id)
                 SELECT a.project_id,a.id FROM ma_agents a
                 WHERE a.id=?2 AND a.project_id=?1 AND a.runtime_kind=?3 AND a.enabled=1 AND a.removed_at IS NULL
                   AND NOT EXISTS (SELECT 1 FROM ma_agent_sessions s WHERE s.project_id=a.project_id
                                   AND s.agent_id=a.id AND s.control_mode='manual')
                 ON CONFLICT(project_id) DO UPDATE SET agent_id=excluded.agent_id",
                params![project, member.id, member.runtime_kind.as_str()],
            )?;
            ensure!(saved == 1, "所选答复成员当前不可用");
        } else {
            tx.execute("DELETE FROM ma_project_reply_preferences WHERE project_id=?1", [project])?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_agent::{AgentRuntimeKind, CreateProjectInput};
    use serde_json::json;

    #[test]
    fn project_reply_preference_survives_reopening_and_tracks_member_lifecycle() -> Result<()> {
        let root = std::env::temp_dir().join(format!("crewvela-project-reply-{}", uuid::Uuid::new_v4()));
        let path = root.join("team.db");
        let result = (|| -> Result<()> {
            let store = MultiAgentStore::open_at(path.clone())?;
            let create = |name: &str| store.create_project(CreateProjectInput {
                name: name.into(), goal: "team chat".into(), root_path: root.display().to_string(), default_branch: "main".into(),
            });
            let first = create("First")?.id;
            let other = create("Other")?.id;
            let agent = store.create_agent(serde_json::from_value(json!({
                "projectId": first, "name": "Responder", "runtimeKind": "codex", "role": "worker"
            }))?)?;
            assert_eq!(store.snapshot(&first)?.default_reply_agent_id, None);
            store.set_project_reply_preference(&first, Some(&agent.id))?;
            assert!(store.set_project_reply_preference(&other, Some(&agent.id)).is_err());
            assert_eq!(store.snapshot(&other)?.default_reply_agent_id, None);
            drop(store);

            let reopened = MultiAgentStore::open_at(path)?;
            assert_eq!(reopened.snapshot(&first)?.default_reply_agent_id.as_deref(), Some(agent.id.as_str()));
            crate::runtime_preferences::with_disabled_for_test(AgentRuntimeKind::Codex, || {
                assert!(reopened.set_project_reply_preference(&first, Some(&agent.id)).is_err());
            });
            reopened.set_project_reply_preference(&first, None)?;
            assert_eq!(reopened.snapshot(&first)?.default_reply_agent_id, None);
            reopened.set_project_reply_preference(&first, Some(&agent.id))?;
            let mut edit = serde_json::to_value(&agent)?;
            edit["agentId"] = json!(agent.id);
            edit["enabled"] = json!(false);
            reopened.update_agent(serde_json::from_value(edit.clone())?)?;
            assert!(reopened.set_project_reply_preference(&first, Some(&agent.id)).is_err());
            assert_eq!(reopened.snapshot(&first)?.default_reply_agent_id.as_deref(), Some(agent.id.as_str()));
            edit["enabled"] = json!(true);
            reopened.update_agent(serde_json::from_value(edit)?)?;
            assert!(reopened.remove_agent(&first, &agent.id, true)?.is_empty());
            assert_eq!(reopened.snapshot(&first)?.default_reply_agent_id, None);
            assert!(reopened.set_project_reply_preference(&first, Some(&agent.id)).is_err());
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }
}
