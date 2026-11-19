use super::*;
use serde_json::json;

pub(super) fn supports(kind: AgentRuntimeKind, usage: ManagedUse) -> bool {
    kind == AgentRuntimeKind::ZCodeCli
        && matches!(
            usage,
            ManagedUse::Plan | ManagedUse::Review | ManagedUse::Write
        )
}

pub(super) fn execute(request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
    let planner = request.managed_planner();
    let review = !planner && request.task.workspace_mode == WorkspaceMode::ReadOnly;
    let policy = if planner { "text-only" } else { "read-only" };
    let run = |prompt: &str| -> Result<RuntimeResult> {
        // Planning and review have a fresh context. Each review inspects the current delivery.
        let previous = request
            .resume_locator
            .as_ref()
            .filter(|_| !planner && !review);
        let resume = previous.and_then(|locator| locator["sessionId"].as_str());
        if let (Some(locator), Some(_)) = (previous, resume) {
            anyhow::ensure!(
                locator["adapter"] == "zcode_cli"
                    && locator["engine"] == "zcode-v4"
                    && locator["toolPolicy"] == policy,
                "ZCode CLI 原会话的引擎或权限无法核实"
            );
        }
        let output = super::super::zcode::execute_managed_turn(
            &request.cwd,
            prompt,
            resume,
            request.agent.model.as_deref(),
            planner,
            cancelled,
            &mut |phase, session, command, turn| {
                let mut patch = json!({"engine":"zcode-v4","transport":"zcode-protocol","sessionId":session,
                    "commandId":command,"permissionMode":"plan","toolPolicy":policy});
                if let Some(turn) = turn {
                    patch["turnId"] = json!(turn);
                }
                request.checkpoint_managed_turn(phase, patch)
            },
        )?;
        let locator = request
            .store
            .latest_session_locator(&request.task.id)?
            .context("找不到本轮 ZCode CLI 接收记录")?;
        anyhow::ensure!(
            locator["sessionId"] == output.session_id
                && locator["commandId"] == output.command_id
                && locator["turnId"] == output.turn_id
                && locator["managedTurn"] == "completed",
            "ZCode CLI 完成回执与本次任务不符"
        );
        Ok(RuntimeResult {
            summary: output.text,
            session_locator: locator,
        })
    };
    if planner {
        request.log_launch("无工具规划", "ZCode CLI v4")?;
        return run(&render_task_prompt(request));
    }
    if review {
        request.log_launch("只读审查", "ZCode CLI v4")?;
        return managed_static_review(request, cancelled, run);
    }
    anyhow::ensure!(
        request.managed_native(),
        "ZCode CLI 托管开发需要独立 Git 工作树"
    );
    let scopes = request.store.managed_write_paths(&request.task.id)?;
    request.log_launch("只读工具和计划范围内文本写入", "ZCode CLI v4")?;
    let prompt = format!(
        "{}\n\n{}",
        render_task_prompt(request),
        managed_edits::instruction(&scopes)
    );
    let mut output = run(&prompt)?;
    output.summary = managed_edits::apply(request, &output.summary, cancelled)?;
    Ok(output)
}
