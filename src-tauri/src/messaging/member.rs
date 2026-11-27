use super::{resolve_session, sessions, SendArgs};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{path::Path, process::Command, time::Duration};

pub(crate) fn configure_member_cli(command: &mut Command, endpoint: &str, token: &str, run: &str) {
    command
        .env("CREWVELA_MEMBER_ENDPOINT", endpoint)
        .env("CREWVELA_MEMBER_TOKEN", token)
        .env("CREWVELA_MEMBER_RUN", run);
}

fn command_at(endpoint: &str, token: &str, payload: Value) -> Result<Value> {
    let address = endpoint
        .strip_prefix("http://")
        .and_then(|p| p.parse::<std::net::SocketAddr>().ok())
        .filter(|address| address.ip().is_loopback())
        .context("成员通信端点必须是本机任务协作桥")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()?;
        let response: Value = client
            .post(format!("http://{address}/v1/command"))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .context("无法连接当前任务协作桥")?
            .json()
            .await?;
        ensure!(
            response["ok"] == true,
            "{}",
            response["error"].as_str().unwrap_or("成员通信未获授权")
        );
        Ok(response["data"].clone())
    })
}

fn credentials() -> Result<(String, String, String)> {
    let get = |key| {
        std::env::var(key).context(
            "该操作需要当前项目任务的协作权限，请在原任务中使用 cc_message_send 或 cc_thread_read",
        )
    };
    Ok((
        get("CREWVELA_MEMBER_ENDPOINT")?,
        get("CREWVELA_MEMBER_TOKEN")?,
        get("CREWVELA_MEMBER_RUN")?,
    ))
}

pub(super) fn send_if_bound(
    path: &Path,
    identity: Option<&str>,
    args: &SendArgs,
) -> Result<Option<Value>> {
    ensure!(
        args.handoff.is_none() || args.purpose == Some(super::Purpose::Handoff),
        "交接字段需要明确的 handoff 目的"
    );
    if args.purpose.is_none() && std::env::var_os("CREWVELA_MEMBER_ENDPOINT").is_none() {
        return Ok(None);
    }
    let (endpoint, token, run) = credentials()?;
    let records = sessions::load_records(path)?;
    let sender = records
        .iter()
        .find(|s| {
            Some(s.id.as_str()) == identity && !s.retired_agent_runtime && s.archived_at.is_none()
        })
        .context("缺少有效的来源项目会话")?;
    let target = resolve_session(&records, &args.to)?;
    ensure!(
        sender.multi_agent_project_id.is_some()
            && sender.multi_agent_project_id == target.multi_agent_project_id
            && sender.id != target.id
            && target.multi_agent_agent_id.is_some()
            && target.archived_at.is_none(),
        "项目任务消息只允许发送给同一项目内的另一位指定成员"
    );
    if args.purpose.is_none() && !target.runtime_command_host {
        return Ok(None);
    }
    ensure!(
        args.reply_to.is_none(),
        "原会话回执参数 replyTo 不适用于项目消息"
    );
    let data = command_at(
        &endpoint,
        &token,
        json!({"action":"message.send","expected_run_id":run,
        "body":args.body,"target_agent_id":target.multi_agent_agent_id,"purpose":args.purpose,
        "handoff":args.handoff,"request_id":args.request}),
    )?;
    Ok(Some(data))
}

pub fn run_thread_args(mut args: impl Iterator<Item = String>) -> Result<()> {
    let (mut message, mut offset) = (None, 0usize);
    while let Some(key) = args.next() {
        match key.as_str() {
            "--message" => message = Some(args.next().context("--message 缺少消息 ID")?),
            "--offset" => {
                offset = args
                    .next()
                    .context("--offset 缺少位置")?
                    .parse()
                    .context("offset 必须是非负整数")?
            }
            _ => anyhow::bail!("未知参数 {key}"),
        }
    }
    let id = message
        .filter(|id| !id.trim().is_empty())
        .context("用法：crewvela thread --message <消息 ID> [--offset <字符位置>]")?;
    let (endpoint, token, run) = credentials()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&command_at(
            &endpoint,
            &token,
            json!({"action":"thread.read","message_id":id,"offset":offset,"expected_run_id":run})
        )?)?
    );
    Ok(())
}
