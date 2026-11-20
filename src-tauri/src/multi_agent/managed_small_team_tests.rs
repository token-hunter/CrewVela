use super::*;
use crate::multi_agent::{
    executor::{commit_task_worktree, git_head, prepare_workspace_at, verify_task_acceptance},
    project_preview::ProjectPreviews, AgentRun, ClaimTaskInput, MultiAgentTask, ReviewVerdict,
    SubmitReviewDecisionInput,
};
use std::path::{Path, PathBuf};

fn small_plan(pair: bool) -> (Fixture, ConversationExecutionPlan) {
    let (fixture, mut input, draft) = managed_plan_fixture();
    let mut draft: Value = serde_json::from_str(&draft).unwrap();
    let tasks = draft["tasks"].as_array_mut().unwrap();
    tasks.truncate(if pair { 2 } else { 1 });
    tasks[0]["writePaths"] = json!(["src/owned", "README.md"]);
    input.participant_agent_ids = tasks.iter().map(|t| t["assigneeAgentId"].as_str().unwrap().into()).collect();
    let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let result = fixture.run_input(input, &ScriptedReplies::new(vec![draft.to_string()]));
    let plan = &result.execution_plans[0];
    let grant = &plan.managed.as_ref().unwrap().target;
    assert_eq!(grant.mode, if pair { ManagedTeamMode::Pair } else { ManagedTeamMode::Solo });
    assert_eq!(grant.planner, fixture.agents[0]);
    fixture.store.apply_conversation_plan_in(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id.clone(), client_request_id: format!("managed-{}", plan.id),
    }, true).unwrap();
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Active).unwrap();
    let plan = fixture.store.managed_plan_running(&fixture.project).unwrap().unwrap();
    assert_eq!(plan.managed.as_ref().unwrap().final_task_id.as_ref(), Some(&plan.task_ids[0]));
    (fixture, plan)
}

#[test]
fn selection_preserves_required_review_and_legacy_grants() -> Result<()> {
    use crate::multi_agent::managed_delivery::{requires_review, ManagedTarget};
    let (fixture, mut input, draft) = managed_plan_fixture();
    let draft: Value = serde_json::from_str(&draft)?;
    let reviewer = draft["tasks"][1]["assigneeAgentId"].as_str().unwrap().to_owned();
    let integrator = draft["tasks"][2]["assigneeAgentId"].as_str().unwrap().to_owned();
    let gate = |input: &ConversationSendInput| -> String {
        fixture.store.conversation_plan_preflight(input).unwrap().into_iter().map(|issue| issue.message).collect::<Vec<_>>().join(" ")
    };
    input.participant_agent_ids = vec![reviewer.clone()];
    assert!(gate(&input).contains("单人托管需要"));
    input.participant_agent_ids = vec![fixture.agents[0].clone(), integrator];
    assert!(gate(&input).contains("双人托管需要"));
    input.participant_agent_ids.truncate(1);
    input.body = "请独立审查后交付".into();
    assert!(gate(&input).contains("要求独立审查"));
    assert!(fixture.store.begin_conversation(input.clone()).is_err());
    input.body = "新增审查记录功能，本轮无需独立审查".into();
    assert!(gate(&input).is_empty());
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_projects SET goal='所有交付必须独立审查' WHERE id=?1", [&fixture.project])?;
    assert!(gate(&input).contains("要求独立审查"));
    input.participant_agent_ids.push(reviewer);
    assert!(gate(&input).is_empty());
    assert!(requires_review("Independent code review is required before merging."));
    assert!(!requires_review("Add an independent review history screen."));
    let legacy: ManagedTarget = serde_json::from_value(json!({"root":"old","base":"old","branch":"main","planner":"p"}))?;
    assert_eq!(legacy.mode, ManagedTeamMode::Team);
    Ok(())
}

#[test]
fn generated_plan_requires_one_writer_and_the_selected_reviewer() -> Result<()> {
    for pair in [false, true] {
        let (fixture, plan) = small_plan(pair);
        let round = fixture.store.conversation_round(&plan.round_id)?;
        let connection = fixture.store.connection.lock().unwrap();
        let check = |tasks| validate_plan(&connection, &round, &PlanDraft { summary: "分工".into(), tasks });
        assert!(check(plan.tasks.clone()).is_ok());
        let mut missing_readme = plan.tasks.clone();
        missing_readme[0].write_paths = vec!["src/owned".into()];
        assert!(check(missing_readme).is_err());
        let mut extra = plan.tasks.clone();
        let mut extra_write = extra[0].clone();
        extra_write.key = "extra".into();
        extra_write.depends_on = vec![extra[0].key.clone()];
        extra.push(extra_write);
        assert!(check(extra).is_err());
        if pair {
            assert!(check(plan.tasks[..1].to_vec()).is_err());
            let mut detached_review = plan.tasks.clone();
            detached_review[1].depends_on.clear();
            assert!(check(detached_review).is_err());
        }
    }
    Ok(())
}

fn claim(fixture: &Fixture, plan: &ConversationExecutionPlan, index: usize) -> Result<(MultiAgentTask, AgentRun)> {
    let task = fixture.store.snapshot(&fixture.project)?.tasks.into_iter().find(|t| t.id == plan.task_ids[index]).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput { task_id: task.id.clone(),
        agent_id: task.assignee_agent_id.clone().unwrap(), lease_seconds: 120 })?;
    fixture.store.update_run(UpdateRunInput { run_id: run.id.clone(), status: RunStatus::Running,
        error: None, output_summary: None, session_locator: None })?;
    Ok((task, run))
}

fn record_run(fixture: &Fixture, run: &AgentRun, cwd: &Path, sha: &str, tested: bool) -> Result<()> {
    let runtime = fixture.store.agent_profile(&run.agent_id)?.runtime_kind;
    let locator = json!({"cwd":cwd,"adapter":runtime,"threadId":id(),"managedTurn":"completed"});
    fixture.store.finalize_successful_run(&run.id, cwd.to_str().unwrap(), "scripted member", tested, Some(sha), Some(&locator))
}

fn implement(fixture: &Fixture, plan: &ConversationExecutionPlan, revision: usize) -> Result<(PathBuf, String)> {
    let (task, run) = claim(fixture, plan, 0)?;
    let project = fixture.store.project(&fixture.project)?;
    let root = crate::providers::config_dir()?.join("worktrees");
    let cwd = prepare_workspace_at(&root, &project, &task, &run.id, &[], None)?;
    std::fs::create_dir_all(cwd.join("src/owned"))?;
    std::fs::write(cwd.join("README.md"), format!("Use Node. Revision {revision}."))?;
    let marker = serde_json::to_string(&fixture.root.join("fail-final-test"))?;
    std::fs::write(cwd.join("src/owned/check.test.js"), format!(
        "require('node:test')('original acceptance',()=>require('node:assert/strict').equal(require('node:fs').existsSync({marker}),false));"))?;
    assert!(verify_task_acceptance(&task, &cwd, &AtomicBool::new(false), Some((&fixture.store, &run.id)))?);
    let sha = commit_task_worktree(&cwd, &task.title)?;
    record_run(fixture, &run, &cwd, &sha, true)?;
    Ok((cwd, sha))
}

fn review(fixture: &Fixture, plan: &ConversationExecutionPlan, sha: &str, verdict: ReviewVerdict) -> Result<String> {
    let (task, run) = claim(fixture, plan, 1)?;
    let root = crate::providers::config_dir()?.join("worktrees");
    let cwd = prepare_workspace_at(&root, &fixture.store.project(&fixture.project)?, &task, &run.id, &[], Some(sha))?;
    assert_eq!(git_head(&cwd)?, sha);
    fixture.store.submit_review_decision(SubmitReviewDecisionInput { review_run_id: run.id.clone(),
        target_task_id: plan.task_ids[0].clone(), verdict, summary: "scripted review".into() })?;
    record_run(fixture, &run, &cwd, sha, false)?;
    Ok(run.id)
}

#[test]
fn final_acceptance_merge_rework_and_restart() -> Result<()> {
    // Recovery checks the normal application's worktree ownership on disk. Give the
    // subprocess an isolated application profile so it exercises the production paths.
    const ENV: &str = "CREWVELA_SMALL_TEAM_TEST_PROFILE";
    let Some(root) = std::env::var_os(ENV) else {
        let root = std::env::temp_dir().join(format!("cvs-{}", &id()[..8]));
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["managed_small_team::final_acceptance_merge_rework_and_restart", "--nocapture", "--test-threads=1"])
            .env(ENV, &root).output()?;
        std::fs::remove_dir_all(&root)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        ensure!(output.status.success() && stdout.contains("1 passed"), "{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
        return Ok(());
    };
    crate::test_profile::initialize(Path::new(&root))?;
    for case in ["solo", "pair-rework", "final-fails", "stale-review", "dirty", "unaudited-merge", "restart-after-merge"] {
        let pair = matches!(case, "pair-rework" | "stale-review" | "restart-after-merge");
        let (fixture, plan) = small_plan(pair);
        let store = &fixture.store;
        let project = store.project(&fixture.project)?;
        let base = git_head(Path::new(&project.root_path))?;
        let target = &plan.managed.as_ref().unwrap().target;
        let (mut cwd, mut sha) = implement(&fixture, &plan, 1)?;
        let mut reviewed = None;
        if pair {
            if case == "pair-rework" {
                review(&fixture, &plan, &sha, ReviewVerdict::ChangesRequested)?;
                let revised = implement(&fixture, &plan, 2)?;
                assert_ne!(revised.1, sha);
                (cwd, sha) = revised;
            }
            reviewed = Some(review(&fixture, &plan, &sha, ReviewVerdict::Approved)?);
        }
        assert_eq!(git_head(Path::new(&project.root_path))?, base);
        let previews = ProjectPreviews::default();
        let settle = |store: &MultiAgentStore, stop: bool| -> Result<()> {
            let saved = store.managed_plan_running(&project.id)?.unwrap();
            store.settle_idle_managed_plan(&store.snapshot(&project.id)?, &saved, &previews, &AtomicBool::new(stop))
        };
        if case == "unaudited-merge" {
            target.merge(&project, &sha)?;
            assert!(store.recover_managed_deliveries_at_startup()?.is_empty());
            assert_eq!(store.project(&project.id)?.status, ProjectStatus::Paused);
            continue;
        }
        assert_eq!(store.recover_managed_deliveries_at_startup()?, [project.id.clone()], "{case}");
        if case == "stale-review" {
            store.connection.lock().unwrap().execute("UPDATE ma_artifacts SET content_hash=?1 WHERE run_id=?2 AND artifact_type='review'",
                params![base,reviewed])?;
        } else if case == "dirty" {
            std::fs::write(cwd.join("README.md"), "unapproved edit")?;
        } else if case == "final-fails" {
            std::fs::write(fixture.root.join("fail-final-test"), "fail only the final host test")?;
        } else if case == "solo" {
            settle(store, true)?;
            assert_eq!(git_head(Path::new(&project.root_path))?, base);
            assert!(store.managed_plan_running(&project.id)?.is_some());
        } else if case == "restart-after-merge" {
            store.connection.lock().unwrap().execute_batch("CREATE TRIGGER interrupt_final_registration BEFORE UPDATE ON ma_conversation_plans
                WHEN json_extract(NEW.data_json,'$.managed.status')='completed' BEGIN SELECT RAISE(ABORT,'interrupt registration'); END;")?;
            assert!(settle(store, false).is_err());
            assert_eq!(git_head(Path::new(&project.root_path))?, sha);
            assert!(store.managed_plan_running(&project.id)?.unwrap().managed.unwrap().final_acceptance.is_some());
            store.connection.lock().unwrap().execute_batch("DROP TRIGGER interrupt_final_registration")?;
        }
        let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
        if case == "restart-after-merge" {
            assert_eq!(reopened.recover_managed_deliveries_at_startup()?, [project.id.clone()]);
        }
        settle(&reopened, false)?;
        let result = reopened.conversation_snapshot(&project.id)?.execution_plans.remove(0).managed.unwrap();
        if matches!(case, "stale-review" | "dirty" | "final-fails") {
            assert_eq!(result.status, "failed", "{case}: {:?}", result.summary);
            assert_eq!(reopened.project(&project.id)?.status, ProjectStatus::Paused);
            assert_eq!(git_head(Path::new(&project.root_path))?, base);
        } else {
            assert_eq!(result.status, "completed", "{case}: {:?}", result.summary);
            assert_eq!(git_head(Path::new(&project.root_path))?, sha);
            let receipt = serde_json::to_value(result.final_acceptance.unwrap())?;
            assert_eq!(receipt["commit"], sha);
            assert_eq!(receipt["verificationCommand"], "node --test src/owned/check.test.js");
            assert_eq!(receipt["reviewRunId"], json!(reviewed));
            let recorded: i64 = reopened.connection.lock().unwrap().query_row(
                "SELECT COUNT(*) FROM ma_run_logs WHERE run_id=?1 AND content LIKE '最终候选提交验收：%'", [receipt["runId"].as_str().unwrap()], |r| r.get(0))?;
            assert_eq!(recorded, 1);
        }
    }
    Ok(())
}
