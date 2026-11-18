use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::super::{AgentPersona, AgentProfile, AgentRole, AgentRuntimeKind, PermissionProfile};
use super::{
    append_event, ensure_project_exists, get_agent_with, now_millis, required,
    sanitized_persona_custom, MultiAgentStore,
};

pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_library_members (
            id TEXT PRIMARY KEY,
            origin_agent_id TEXT NOT NULL UNIQUE,
            config_json TEXT NOT NULL,
            default_order INTEGER
        );
        CREATE TABLE IF NOT EXISTS ma_library_project_members (
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            member_id TEXT NOT NULL REFERENCES ma_library_members(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL UNIQUE REFERENCES ma_agents(id) ON DELETE CASCADE,
            follows_library INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(project_id, member_id)
        );",
    )?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryProfile {
    name: String,
    runtime_kind: AgentRuntimeKind,
    role: AgentRole,
    description: String,
    persona: AgentPersona,
    persona_custom: String,
    avatar_id: Option<String>,
    enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredProfile {
    profile: LibraryProfile,
    model: Option<String>,
    permission_profile: PermissionProfile,
}

impl From<AgentProfile> for StoredProfile {
    fn from(agent: AgentProfile) -> Self {
        Self {
            profile: LibraryProfile {
                name: agent.name,
                runtime_kind: agent.runtime_kind,
                role: agent.role,
                description: agent.description,
                persona: agent.persona,
                persona_custom: agent.persona_custom,
                avatar_id: agent.avatar_id,
                enabled: agent.enabled,
            },
            model: agent.model,
            permission_profile: agent.permission_profile,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryMember {
    id: String,
    profile: LibraryProfile,
    is_default: bool,
    runtime_locked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibrarySnapshot {
    members: Vec<LibraryMember>,
    auto_apply: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryImport {
    source_project_id: String,
    agent_ids: Vec<String>,
    apply_to_project_id: Option<String>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryApplication {
    added: usize,
    skipped: Vec<String>,
}

pub(super) fn auto_apply(connection: &Connection) -> Result<bool> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value FROM ma_meta WHERE key='member_library_auto_apply'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.as_deref() != Some("0"))
}

fn linked_sessions(connection: &Connection, member_id: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_library_project_members link
            JOIN ma_agents agent ON agent.id=link.agent_id AND agent.removed_at IS NULL
            JOIN ma_agent_sessions session ON session.agent_id=agent.id
         WHERE link.member_id=?1 AND link.follows_library=1)",
        [member_id],
        |row| row.get(0),
    )?)
}

pub(super) fn apply_default(
    connection: &Connection,
    project_id: &str,
) -> Result<LibraryApplication> {
    ensure_project_exists(connection, project_id)?;
    let mut statement = connection.prepare("SELECT id,config_json FROM ma_library_members WHERE default_order IS NOT NULL ORDER BY default_order")?;
    let members = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = LibraryApplication::default();
    for (member_id, json) in members {
        let config: StoredProfile = serde_json::from_str(&json)?;
        let profile = config.profile;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_library_project_members WHERE project_id=?1 AND member_id=?2)
                 OR EXISTS(SELECT 1 FROM ma_agents WHERE project_id=?1 AND lower(name)=lower(?3))",
            params![project_id, member_id, profile.name], |row| row.get(0),
        )?;
        if exists || !profile.enabled || !profile.runtime_kind.is_collaboration_cli() {
            result.skipped.push(profile.name);
            continue;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let capabilities = super::super::runtime::capabilities_for_runtime(profile.runtime_kind);
        connection.execute(
            "INSERT INTO ma_agents(id,project_id,name,runtime_kind,role,description,persona,persona_custom,model,
                permission_profile,max_concurrency,capabilities_json,enabled,created_at,updated_at,avatar_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,1,?12,?12,?13)",
            params![id, project_id, profile.name, profile.runtime_kind.as_str(), profile.role.as_str(), profile.description,
                profile.persona.as_str(), profile.persona_custom, config.model, config.permission_profile.as_str(),
                serde_json::to_string(&capabilities)?, now, profile.avatar_id],
        )?;
        connection.execute("INSERT INTO ma_library_project_members(project_id,member_id,agent_id) VALUES(?1,?2,?3)", params![project_id, member_id, id])?;
        append_event(
            connection,
            project_id,
            "agent",
            &id,
            "agent.created",
            "user",
            None,
            serde_json::json!({"name": profile.name, "runtimeKind": profile.runtime_kind, "role": profile.role, "libraryMemberId": member_id}),
            now,
        )?;
        result.added += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        CreateAgentInput, CreateProjectInput, CreateTaskInput, UpdateAgentInput,
    };
    use super::*;
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("crewvela-default-team-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn input(&self, name: &str) -> CreateProjectInput {
            CreateProjectInput {
                name: name.into(),
                goal: "测试默认团队".into(),
                root_path: self.0.join(name).to_string_lossy().into_owned(),
                default_branch: "main".into(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn agent(
        store: &MultiAgentStore,
        project_id: &str,
        name: &str,
        runtime: AgentRuntimeKind,
        role: AgentRole,
    ) -> AgentProfile {
        store
            .create_agent(CreateAgentInput {
                project_id: project_id.into(),
                name: name.into(),
                runtime_kind: runtime,
                role,
                avatar_id: Some("orbit".into()),
                description: "职责".into(),
                persona: AgentPersona::Concise,
                persona_custom: String::new(),
                model: Some("fixture-model".into()),
                permission_profile: PermissionProfile::Default,
                max_concurrency: 1,
            })
            .unwrap()
    }

    #[test]
    fn default_team_applies_to_existing_and_new_projects_without_changing_origin_identity() {
        let fixture = Fixture::new();
        let db = fixture.0.join("control.db");
        let store = MultiAgentStore::open_at(db.clone()).unwrap();
        let source = store.create_project(fixture.input("原项目")).unwrap();
        let target = store
            .create_project(fixture.input("snake-git-test"))
            .unwrap();
        let existing = [
            agent(
                &store,
                &source.id,
                "前端开发",
                AgentRuntimeKind::ClaudeCode,
                AgentRole::Worker,
            ),
            agent(
                &store,
                &source.id,
                "后端开发",
                AgentRuntimeKind::Codex,
                AgentRole::Worker,
            ),
            agent(
                &store,
                &source.id,
                "前端审查",
                AgentRuntimeKind::Codex,
                AgentRole::Reviewer,
            ),
            agent(
                &store,
                &source.id,
                "后端审查",
                AgentRuntimeKind::ClaudeCode,
                AgentRole::Reviewer,
            ),
            agent(
                &store,
                &source.id,
                "联调集成",
                AgentRuntimeKind::Codex,
                AgentRole::Integrator,
            ),
        ];
        let first = &existing[0];
        store
            .link_agent_app_session(&source.id, &first.id, "origin-session")
            .unwrap();
        let task: CreateTaskInput =
            serde_json::from_value(serde_json::json!({"projectId":source.id,
            "title":"原任务", "assigneeAgentId":first.id}))
            .unwrap();
        let task = store.create_task(task).unwrap();
        let conversation_id = store
            .conversation_snapshot(&source.id)
            .unwrap()
            .conversation
            .id;

        let ids = existing
            .iter()
            .map(|member| member.id.clone())
            .collect::<Vec<_>>();
        let add = || LibraryImport {
            source_project_id: source.id.clone(),
            agent_ids: ids.clone(),
            apply_to_project_id: Some(target.id.clone()),
        };
        assert_eq!(store.import_default_team(add()).unwrap().added, 5);
        assert_eq!(store.import_default_team(add()).unwrap().added, 0);
        let after = store.snapshot(&source.id).unwrap();
        assert_eq!(after.agents.len(), 5);
        assert_eq!(
            after
                .tasks
                .iter()
                .find(|entry| entry.id == task.id)
                .unwrap()
                .assignee_agent_id
                .as_deref(),
            Some(first.id.as_str())
        );
        assert_eq!(
            after.agent_sessions[0].app_session_id.as_deref(),
            Some("origin-session")
        );
        assert_eq!(
            store
                .conversation_snapshot(&source.id)
                .unwrap()
                .conversation
                .id,
            conversation_id
        );
        let target_after = store.snapshot(&target.id).unwrap();
        assert_eq!(target_after.agents.len(), 5);
        assert!(target_after.agent_sessions.is_empty() && target_after.tasks.is_empty());
        let linked = target_after
            .agents
            .iter()
            .find(|entry| entry.name == first.name)
            .unwrap();
        assert_ne!(linked.id, first.id);
        assert_eq!(linked.model, first.model);

        let library = store.library_snapshot().unwrap();
        assert!(library.auto_apply && library.members.iter().all(|member| member.is_default));
        let member = library
            .members
            .iter()
            .find(|member| member.profile.name == first.name)
            .unwrap();
        assert!(member.runtime_locked);
        let mut changed = member.profile.clone();
        changed.role = AgentRole::Reviewer;
        assert!(store.update_library_member(&member.id, changed).is_err());
        let mut changed = member.profile.clone();
        changed.description = "下一轮将使用的新职责".into();
        store
            .update_library_member(&member.id, changed.clone())
            .unwrap();
        assert_eq!(
            store.agent_profile(&first.id).unwrap().description,
            changed.description
        );
        assert_eq!(
            store.agent_profile(&linked.id).unwrap().description,
            changed.description
        );
        drop(store);

        let store = MultiAgentStore::open_at(db).unwrap();
        let next = store
            .create_project_with_git(fixture.input("新项目"), None)
            .unwrap();
        let next = store.snapshot(&next.id).unwrap();
        assert_eq!(next.agents.len(), 5);
        assert!(next
            .agents
            .iter()
            .any(|entry| entry.description == changed.description));
        store.set_library_auto_apply(false).unwrap();
        let empty = store
            .create_project_with_git(fixture.input("空团队"), None)
            .unwrap();
        assert!(store.snapshot(&empty.id).unwrap().agents.is_empty());
    }

    #[test]
    fn applying_preserves_same_name_local_members_and_explicit_project_overrides() {
        let fixture = Fixture::new();
        let store = MultiAgentStore::open_at(fixture.0.join("control.db")).unwrap();
        let source = store.create_project(fixture.input("原项目")).unwrap();
        let target = store.create_project(fixture.input("目标项目")).unwrap();
        let first = agent(
            &store,
            &source.id,
            "开发",
            AgentRuntimeKind::ClaudeCode,
            AgentRole::Worker,
        );
        let second = agent(
            &store,
            &source.id,
            "审查",
            AgentRuntimeKind::Codex,
            AgentRole::Reviewer,
        );
        let local = agent(
            &store,
            &target.id,
            "开发",
            AgentRuntimeKind::Codex,
            AgentRole::Worker,
        );
        let result = store
            .import_default_team(LibraryImport {
                source_project_id: source.id,
                agent_ids: vec![first.id, second.id],
                apply_to_project_id: Some(target.id.clone()),
            })
            .unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(result.skipped, ["开发"]);
        assert_eq!(
            store.agent_profile(&local.id).unwrap().runtime_kind,
            AgentRuntimeKind::Codex
        );
        let audit = store
            .snapshot(&target.id)
            .unwrap()
            .agents
            .into_iter()
            .find(|member| member.role == AgentRole::Reviewer)
            .unwrap();
        store
            .update_agent(UpdateAgentInput {
                agent_id: audit.id.clone(),
                project_id: target.id.clone(),
                avatar_id: audit.avatar_id,
                name: "项目专用审查".into(),
                runtime_kind: audit.runtime_kind,
                role: audit.role,
                description: "项目专长".into(),
                persona: audit.persona,
                persona_custom: audit.persona_custom,
                model: audit.model,
                enabled: true,
            })
            .unwrap();
        let library = store.library_snapshot().unwrap();
        let row = library
            .members
            .iter()
            .find(|entry| entry.profile.role == AgentRole::Reviewer)
            .unwrap();
        let mut profile = row.profile.clone();
        profile.description = "全局改动".into();
        store.update_library_member(&row.id, profile).unwrap();
        assert_eq!(
            store.agent_profile(&audit.id).unwrap().description,
            "项目专长"
        );
        assert_eq!(store.apply_default_team(&target.id).unwrap().added, 0);
        assert_eq!(store.snapshot(&target.id).unwrap().agents.len(), 2);
    }
}

impl MultiAgentStore {
    pub(crate) fn library_snapshot(&self) -> Result<LibrarySnapshot> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT id,config_json,default_order IS NOT NULL FROM ma_library_members ORDER BY default_order IS NULL,default_order,rowid")?;
        let mut rows = statement.query([])?;
        let mut members = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let config: StoredProfile = serde_json::from_str(&row.get::<_, String>(1)?)?;
            members.push(LibraryMember {
                runtime_locked: linked_sessions(&connection, &id)?,
                id,
                profile: config.profile,
                is_default: row.get(2)?,
            });
        }
        Ok(LibrarySnapshot {
            members,
            auto_apply: auto_apply(&connection)?,
        })
    }

    pub(crate) fn set_library_auto_apply(&self, enabled: bool) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO ma_meta(key,value) VALUES('member_library_auto_apply',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if enabled { "1" } else { "0" }],
        )?;
        Ok(())
    }

    pub(crate) fn import_default_team(&self, input: LibraryImport) -> Result<LibraryApplication> {
        if input.agent_ids.is_empty() {
            bail!("请至少选择一位成员");
        }
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE ma_library_members SET default_order=NULL", [])?;
        let mut used = std::collections::HashSet::new();
        for (index, agent_id) in input.agent_ids.iter().enumerate() {
            let agent = get_agent_with(&tx, agent_id)?;
            if agent.project_id != input.source_project_id
                || agent.removed_at.is_some()
                || !agent.enabled
                || !agent.runtime_kind.is_collaboration_cli()
            {
                bail!("成员「{}」已不可导入，请刷新并重新选择", agent.name);
            }
            let linked: Option<String> = tx
                .query_row(
                    "SELECT member_id FROM ma_library_project_members WHERE agent_id=?1",
                    [agent_id],
                    |row| row.get(0),
                )
                .optional()?;
            let existing = match linked {
                Some(id) => Some(id),
                None => tx
                    .query_row(
                        "SELECT id FROM ma_library_members WHERE origin_agent_id=?1",
                        [agent_id],
                        |row| row.get(0),
                    )
                    .optional()?,
            };
            let member_id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            if !used.insert(member_id.clone()) {
                continue;
            }
            let config = StoredProfile::from(agent);
            tx.execute(
                "INSERT INTO ma_library_members(id,origin_agent_id,config_json,default_order) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET default_order=excluded.default_order",
                params![member_id, agent_id, serde_json::to_string(&config)?, index as i64],
            )?;
            // Keep the original project's profile and every existing foreign key untouched.
            tx.execute("INSERT OR IGNORE INTO ma_library_project_members(project_id,member_id,agent_id) VALUES(?1,?2,?3)", params![input.source_project_id, member_id, agent_id])?;
        }
        let result = match input.apply_to_project_id {
            Some(project_id) if project_id != input.source_project_id => {
                apply_default(&tx, &project_id)?
            }
            _ => LibraryApplication::default(),
        };
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn apply_default_team(&self, project_id: &str) -> Result<LibraryApplication> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = apply_default(&tx, project_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn library_linked_agent_ids(&self, member_id: &str) -> Result<Vec<String>> {
        let connection = self.connection.lock().unwrap();
        let mut query = connection.prepare("SELECT agent.id FROM ma_library_project_members link JOIN ma_agents agent ON agent.id=link.agent_id
            WHERE link.member_id=?1 AND link.follows_library=1 AND agent.removed_at IS NULL")?;
        let rows = query
            .query_map([member_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn update_library_member(
        &self,
        member_id: &str,
        mut profile: LibraryProfile,
    ) -> Result<()> {
        profile.name = required(&profile.name, "成员名称")?.to_string();
        profile.description = profile.description.trim().to_string();
        profile.persona_custom =
            sanitized_persona_custom(&profile.persona, &profile.persona_custom);
        super::super::domain::validate_agent_avatar(profile.avatar_id.as_deref())?;
        if !profile.runtime_kind.is_collaboration_cli() {
            bail!("全局成员请选择协作 CLI Runtime");
        }
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let json: String = tx
            .query_row(
                "SELECT config_json FROM ma_library_members WHERE id=?1",
                [member_id],
                |row| row.get(0),
            )
            .optional()?
            .context("成员库中找不到该成员")?;
        let mut stored: StoredProfile = serde_json::from_str(&json)?;
        let runtime_changed = stored.profile.runtime_kind != profile.runtime_kind;
        if runtime_changed && linked_sessions(&tx, member_id)? {
            bail!("该成员已有关联项目会话，请保留当前 Runtime。可以另建不同 Runtime 的成员。");
        }
        if runtime_changed || (!stored.profile.enabled && profile.enabled) {
            profile.runtime_kind.ensure_available()?;
        }
        let blocked: Option<String> = tx.query_row(
            "SELECT project.name FROM ma_library_project_members link
             JOIN ma_agents agent ON agent.id=link.agent_id AND agent.removed_at IS NULL
             JOIN ma_projects project ON project.id=link.project_id
             WHERE link.member_id=?1 AND link.follows_library=1 AND (
                 EXISTS(SELECT 1 FROM ma_runs r WHERE r.agent_id=agent.id AND
                   (r.status IN ('queued','starting','running','waiting_input','waiting_approval') OR json_extract(r.session_locator_json,'$.stopUnverified')=1))
                 OR EXISTS(SELECT 1 FROM ma_conversation_rounds r,json_each(r.data_json,'$.participantAgentIds') p
                    WHERE r.project_id=project.id AND p.value=agent.id AND r.status IN ('queued','running','waiting_approval','stopping','unconfirmed'))
                 OR (?2 AND EXISTS(SELECT 1 FROM ma_tasks t WHERE t.project_id=project.id AND
                      (t.assignee_agent_id=agent.id OR t.reviewer_agent_id=agent.id) AND t.status NOT IN ('done','cancelled','failed')))
             ) LIMIT 1", params![member_id, runtime_changed || stored.profile.role != profile.role || !profile.enabled], |row| row.get(0),
        ).optional()?;
        if let Some(project) = blocked {
            bail!("「{project}」有尚未结束的关联任务或对话，请结束后再保存");
        }
        let collision: Option<String> = tx.query_row(
            "SELECT project.name FROM ma_library_project_members link
             JOIN ma_agents agent ON agent.id=link.agent_id AND agent.removed_at IS NULL
             JOIN ma_projects project ON project.id=link.project_id
             JOIN ma_agents other ON other.project_id=project.id AND other.id<>agent.id AND lower(other.name)=lower(?2)
             WHERE link.member_id=?1 AND link.follows_library=1 LIMIT 1", params![member_id, profile.name], |row| row.get(0),
        ).optional()?;
        if let Some(project) = collision {
            bail!("「{project}」已有同名成员，请换一个名称");
        }
        if runtime_changed {
            stored.model = None;
        }
        let capabilities = super::super::runtime::capabilities_for_runtime(profile.runtime_kind);
        tx.execute(
            "UPDATE ma_agents SET name=?2,runtime_kind=?3,role=?4,description=?5,persona=?6,persona_custom=?7,
             avatar_id=?8,model=?9,capabilities_json=?10,updated_at=?11,enabled=?12
             WHERE removed_at IS NULL AND id IN (SELECT agent_id FROM ma_library_project_members WHERE member_id=?1 AND follows_library=1)",
            params![member_id, profile.name, profile.runtime_kind.as_str(), profile.role.as_str(), profile.description,
                profile.persona.as_str(), profile.persona_custom, profile.avatar_id, stored.model,
                serde_json::to_string(&capabilities)?, now_millis(), profile.enabled],
        )?;
        stored.profile = profile;
        tx.execute(
            "UPDATE ma_library_members SET config_json=?2 WHERE id=?1",
            params![member_id, serde_json::to_string(&stored)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}
