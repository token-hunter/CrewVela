use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use super::{
    AppendRuntimeEventInput, HeartbeatRunInput, MultiAgentStore, PublishArtifactInput,
    PutMemoryInput, RequestApprovalInput, ReviewVerdict, SearchKnowledgeInput, SearchMemoryInput,
    SendCollaborationMessageInput, SubmitReviewDecisionInput,
};

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CollaborationAccess {
    pub endpoint: String,
    pub token: String,
}

impl CollaborationAccess {
    pub fn prompt_contract(&self) -> String {
        format!(
            "优先使用已注入的 crewvela MCP 工具（cc_task_read、cc_review_submit、cc_artifact_publish、\
             cc_approval_request、cc_heartbeat、cc_knowledge_search、cc_memory_search、cc_memory_put 等）。如果当前 Runtime\
             没有显示这些工具，可回退到：POST {}/v1/command\nAuthorization: Bearer {}\nContent-Type: application/json\n\
             可用 action：task.read、review.submit、artifact.publish、artifact.inspect、\
             approval.request、approval.read、heartbeat、knowledge.search、memory.search、memory.put、runtime.event。\n\
             所有调用仅能访问当前项目、任务、Run、Agent 和工作目录；状态仍以调度器和 SQLite 为准。\n\
             Agent 间会话通信使用 crewvela Skill：通过 CREWVELA_CLI 运行 sessions，再运行 send --to <目标> --body <消息>；MCP/HTTP 用于任务、Review、Artifact、审批、心跳、知识和记忆。\n\
             建议开始时调用 memory.search；涉及既有需求、历史决策、交付或审核时调用 knowledge.search，并保留返回的 source_uri；\
             长任务定期调用 heartbeat；中间成果用 artifact.publish，遇到权限或决策问题用\
             approval.request；请求后使用返回的审批 ID 调用\
             approval.read，等待 approved 后再继续。",
            self.endpoint, self.token
        )
    }
}

#[derive(Debug, Clone)]
struct RunScope {
    project_id: String,
    task_id: String,
    run_id: String,
    agent_id: String,
    cwd: PathBuf,
}

pub struct RunBridgeLease {
    token: String,
    run_id: String,
    endpoint: String,
    scopes: Arc<Mutex<HashMap<String, RunScope>>>,
}

impl RunBridgeLease {
    pub fn access(&self) -> CollaborationAccess {
        CollaborationAccess {
            endpoint: self.endpoint.clone(),
            token: self.token.clone(),
        }
    }
}

impl Drop for RunBridgeLease {
    fn drop(&mut self) {
        // token 按 project:agent 复用，同一 Agent 的后继 Run 可能已重新注册同一 token，
        // 只在自己的 scope 仍有效时才删除，避免误吊销新 Run 的访问权限。
        let mut scopes = self.scopes.lock().unwrap();
        if scopes
            .get(&self.token)
            .is_some_and(|scope| scope.run_id == self.run_id)
        {
            scopes.remove(&self.token);
        }
    }
}

#[derive(Clone)]
pub struct CollaborationBridge {
    endpoint: String,
    scopes: Arc<Mutex<HashMap<String, RunScope>>>,
    agent_tokens: Arc<Mutex<HashMap<String, String>>>,
}

impl CollaborationBridge {
    pub fn start(store: MultiAgentStore) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("启动本地 Agent 协作接口失败")?;
        let address = listener.local_addr()?;
        let endpoint = format!("http://{address}");
        let scopes = Arc::new(Mutex::new(HashMap::new()));
        let agent_tokens = Arc::new(Mutex::new(HashMap::new()));
        let server_scopes = scopes.clone();
        std::thread::Builder::new()
            .name("cc-agent-collaboration-bridge".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let store = store.clone();
                            let scopes = server_scopes.clone();
                            let _ = std::thread::Builder::new()
                                .name("cc-agent-collaboration-request".to_string())
                                .spawn(move || {
                                    let _ = handle_connection(stream, &store, &scopes);
                                });
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(25)),
                    }
                }
            })?;
        Ok(Self {
            endpoint,
            scopes,
            agent_tokens,
        })
    }

    pub fn register_run(
        &self,
        project_id: &str,
        task_id: &str,
        run_id: &str,
        agent_id: &str,
        cwd: &Path,
    ) -> RunBridgeLease {
        let agent_key = format!("{project_id}:{agent_id}");
        let token = self
            .agent_tokens
            .lock()
            .unwrap()
            .entry(agent_key)
            .or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        self.scopes.lock().unwrap().insert(
            token.clone(),
            RunScope {
                project_id: project_id.to_string(),
                task_id: task_id.to_string(),
                run_id: run_id.to_string(),
                agent_id: agent_id.to_string(),
                cwd: cwd.to_path_buf(),
            },
        );
        RunBridgeLease {
            token,
            run_id: run_id.to_string(),
            endpoint: self.endpoint.clone(),
            scopes: self.scopes.clone(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum CollaborationCommand {
    #[serde(rename = "task.read")]
    TaskRead,
    #[serde(rename = "thread.read")]
    ThreadRead { limit: Option<u32> },
    #[serde(rename = "message.send")]
    MessageSend {
        target_agent_id: Option<String>,
        body: String,
    },
    #[serde(rename = "review.submit")]
    ReviewSubmit {
        target_task_id: String,
        verdict: ReviewVerdict,
        summary: String,
    },
    #[serde(rename = "artifact.publish")]
    ArtifactPublish {
        artifact_type: String,
        uri: String,
        summary: String,
        content_hash: Option<String>,
    },
    #[serde(rename = "artifact.inspect")]
    ArtifactInspect { artifact_id: String },
    #[serde(rename = "approval.request")]
    ApprovalRequest {
        approval_type: String,
        summary: String,
    },
    #[serde(rename = "approval.read")]
    ApprovalRead { approval_id: String },
    #[serde(rename = "heartbeat")]
    Heartbeat { lease_seconds: Option<u64> },
    #[serde(rename = "knowledge.search")]
    KnowledgeSearch {
        query: String,
        source_types: Option<Vec<String>>,
        limit: Option<u32>,
    },
    #[serde(rename = "memory.search")]
    MemorySearch {
        query: String,
        namespace: Option<String>,
        limit: Option<u32>,
    },
    #[serde(rename = "memory.put")]
    MemoryPut {
        namespace: String,
        key: String,
        value: String,
        confidence: Option<f64>,
    },
    #[serde(rename = "runtime.event")]
    RuntimeEvent {
        event_type: String,
        payload: Option<serde_json::Value>,
    },
}

fn handle_connection(
    mut stream: TcpStream,
    store: &MultiAgentStore,
    scopes: &Arc<Mutex<HashMap<String, RunScope>>>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            write_json_response(&mut stream, 400, response_error(&error.to_string()))?;
            return Ok(());
        }
    };
    if request.method == "GET" && request.path == "/health" {
        write_json_response(
            &mut stream,
            200,
            serde_json::json!({"ok": true, "data": {"status": "ready"}}),
        )?;
        return Ok(());
    }
    if request.method != "POST" || (request.path != "/v1/command" && request.path != "/mcp") {
        write_json_response(&mut stream, 404, response_error("未找到接口"))?;
        return Ok(());
    }
    let Some(token) = request.authorization.strip_prefix("Bearer ") else {
        write_json_response(&mut stream, 401, response_error("缺少任务访问令牌"))?;
        return Ok(());
    };
    let scope = scopes.lock().unwrap().get(token).cloned();
    let Some(scope) = scope else {
        write_json_response(&mut stream, 403, response_error("任务访问令牌无效或已过期"))?;
        return Ok(());
    };
    if request.path == "/mcp" {
        let is_notification = serde_json::from_slice::<serde_json::Value>(&request.body)
            .ok()
            .is_some_and(|value| value.get("id").is_none());
        let response = handle_mcp_request(store, &scope, &request.body);
        match response {
            Ok(_) if is_notification => write_empty_response(&mut stream, 202)?,
            Ok(response) => write_json_response(&mut stream, 200, response)?,
            Err(error) => write_json_response(
                &mut stream,
                200,
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": serde_json::Value::Null,
                    "error": {"code": -32603, "message": error.to_string()},
                }),
            )?,
        }
        return Ok(());
    }
    let command = match serde_json::from_slice::<CollaborationCommand>(&request.body) {
        Ok(command) => command,
        Err(error) => {
            write_json_response(
                &mut stream,
                400,
                response_error(&format!("命令格式错误：{error}")),
            )?;
            return Ok(());
        }
    };
    match dispatch(store, &scope, command) {
        Ok(data) => write_json_response(
            &mut stream,
            200,
            serde_json::json!({"ok": true, "data": data}),
        )?,
        Err(error) => write_json_response(&mut stream, 422, response_error(&error.to_string()))?,
    }
    Ok(())
}

fn dispatch(
    store: &MultiAgentStore,
    scope: &RunScope,
    command: CollaborationCommand,
) -> Result<serde_json::Value> {
    let profile = store.agent_profile(&scope.agent_id)?;
    if !profile.enabled {
        return Err(anyhow!("该 Agent 已禁用，不能使用协作功能"));
    }
    let _runtime_lease = crate::runtime_preferences::acquire(profile.runtime_kind)?;
    match command {
        CollaborationCommand::TaskRead => {
            let snapshot = store.snapshot(&scope.project_id)?;
            let task = snapshot
                .tasks
                .into_iter()
                .find(|task| task.id == scope.task_id)
                .ok_or_else(|| anyhow!("当前任务不存在"))?;
            let dependencies = snapshot
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.task_id == scope.task_id)
                .collect::<Vec<_>>();
            let artifacts = store.dependency_artifacts(&scope.task_id)?;
            Ok(serde_json::json!({
                "task": task,
                "dependencies": dependencies,
                "dependencyArtifacts": artifacts,
            }))
        }
        CollaborationCommand::ThreadRead { limit } => {
            Ok(serde_json::to_value(store.scoped_messages(
                &scope.project_id,
                &scope.task_id,
                &scope.agent_id,
                limit.unwrap_or(100),
            )?)?)
        }
        CollaborationCommand::MessageSend {
            target_agent_id,
            body,
        } => Ok(serde_json::to_value(store.send_collaboration_message(
            SendCollaborationMessageInput {
                project_id: scope.project_id.clone(),
                task_id: Some(scope.task_id.clone()),
                sender_type: "agent".to_string(),
                sender_id: Some(scope.agent_id.clone()),
                target_agent_id,
                body,
            },
        )?)?),
        CollaborationCommand::ReviewSubmit {
            target_task_id,
            verdict,
            summary,
        } => Ok(serde_json::to_value(store.submit_review_decision(
            SubmitReviewDecisionInput {
                review_run_id: scope.run_id.clone(),
                target_task_id,
                verdict,
                summary,
            },
        )?)?),
        CollaborationCommand::ArtifactPublish {
            artifact_type,
            uri,
            summary,
            content_hash,
        } => {
            let safe_uri = scoped_artifact_uri(&scope.cwd, &uri)?;
            Ok(serde_json::to_value(store.publish_artifact(
                PublishArtifactInput {
                    project_id: scope.project_id.clone(),
                    task_id: scope.task_id.clone(),
                    run_id: scope.run_id.clone(),
                    artifact_type,
                    uri: safe_uri,
                    summary,
                    content_hash,
                },
            )?)?)
        }
        CollaborationCommand::ArtifactInspect { artifact_id } => {
            let snapshot = store.snapshot(&scope.project_id)?;
            let artifact = snapshot
                .artifacts
                .into_iter()
                .find(|artifact| {
                    artifact.id == artifact_id
                        && (artifact.task_id == scope.task_id
                            || snapshot.dependencies.iter().any(|dependency| {
                                dependency.task_id == scope.task_id
                                    && dependency.depends_on_task_id == artifact.task_id
                            }))
                })
                .ok_or_else(|| anyhow!("产物不存在或不在当前任务依赖范围内"))?;
            Ok(serde_json::to_value(artifact)?)
        }
        CollaborationCommand::ApprovalRequest {
            approval_type,
            summary,
        } => Ok(serde_json::to_value(store.request_approval(
            RequestApprovalInput {
                project_id: scope.project_id.clone(),
                task_id: Some(scope.task_id.clone()),
                run_id: Some(scope.run_id.clone()),
                approval_type,
                summary,
            },
        )?)?),
        CollaborationCommand::ApprovalRead { approval_id } => {
            let snapshot = store.snapshot(&scope.project_id)?;
            let approval = snapshot
                .approvals
                .into_iter()
                .find(|approval| {
                    approval.id == approval_id
                        && approval.task_id.as_deref() == Some(scope.task_id.as_str())
                        && approval.run_id.as_deref() == Some(scope.run_id.as_str())
                })
                .ok_or_else(|| anyhow!("审批不存在或不属于当前 Run"))?;
            Ok(serde_json::to_value(approval)?)
        }
        CollaborationCommand::Heartbeat { lease_seconds } => Ok(serde_json::to_value(
            store.heartbeat_run(HeartbeatRunInput {
                run_id: scope.run_id.clone(),
                lease_seconds: lease_seconds.unwrap_or(120),
            })?,
        )?),
        CollaborationCommand::KnowledgeSearch {
            query,
            source_types,
            limit,
        } => Ok(serde_json::to_value(store.search_knowledge_for_agent(
            SearchKnowledgeInput {
                offset: 0,
                project_id: scope.project_id.clone(),
                query,
                source_types: source_types.unwrap_or_default(),
                task_id: None,
                limit: limit.unwrap_or(20),
            },
            &scope.agent_id,
            &scope.task_id,
        )?)?),
        CollaborationCommand::MemorySearch {
            query,
            namespace,
            limit,
        } => Ok(serde_json::to_value(store.search_memory_for_agent(
            SearchMemoryInput {
                project_id: scope.project_id.clone(),
                query,
                namespace,
                current_run_id: Some(scope.run_id.clone()),
                limit: limit.unwrap_or(20),
            },
            &scope.agent_id,
        )?)?),
        CollaborationCommand::MemoryPut {
            namespace,
            key,
            value,
            confidence,
        } => Ok(serde_json::to_value(store.put_memory(PutMemoryInput {
            project_id: scope.project_id.clone(),
            namespace,
            key,
            value,
            source_task_id: Some(scope.task_id.clone()),
            source_run_id: Some(scope.run_id.clone()),
            confidence: confidence.unwrap_or(0.5),
        })?)?),
        CollaborationCommand::RuntimeEvent {
            event_type,
            payload,
        } => Ok(serde_json::to_value(store.append_runtime_event(
            AppendRuntimeEventInput {
                run_id: scope.run_id.clone(),
                event_type,
                payload: payload.unwrap_or_else(|| serde_json::json!({})),
            },
        )?)?),
    }
}

fn handle_mcp_request(
    store: &MultiAgentStore,
    scope: &RunScope,
    body: &[u8],
) -> Result<serde_json::Value> {
    let request: serde_json::Value =
        serde_json::from_slice(body).context("MCP 请求不是有效 JSON")?;
    let id = request
        .get("id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let method = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("MCP 请求缺少 method"))?;
    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": negotiated_protocol_version(&request),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "crewvela-collaboration", "version": "1.0.0"},
            "instructions": "这些工具只访问当前任务范围。SQLite 调度状态是唯一事实源。",
        }),
        "notifications/initialized" | "ping" => serde_json::json!({}),
        "tools/list" => serde_json::json!({"tools": collaboration_tools()}),
        "tools/call" => {
            let params = request
                .get("params")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let name = params
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("tools/call 缺少工具名"))?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let command = collaboration_command_from_tool(name, &arguments)?;
            match dispatch(store, scope, command) {
                Ok(data) => {
                    let structured_content = if data.is_object() {
                        data.clone()
                    } else {
                        serde_json::json!({"data": data})
                    };
                    serde_json::json!({
                        "content": [{"type": "text", "text": serde_json::to_string(&data)?}],
                        "structuredContent": structured_content,
                        "isError": false,
                    })
                }
                Err(error) => serde_json::json!({
                    "content": [{"type": "text", "text": error.to_string()}],
                    "isError": true,
                }),
            }
        }
        _ => {
            return Ok(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("未知 MCP 方法：{method}")},
            }));
        }
    };
    Ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn negotiated_protocol_version(request: &serde_json::Value) -> &str {
    match request
        .pointer("/params/protocolVersion")
        .and_then(serde_json::Value::as_str)
    {
        Some("2024-11-05") => "2024-11-05",
        Some("2025-06-18") => "2025-06-18",
        _ => "2025-03-26",
    }
}

fn collaboration_tools() -> Vec<serde_json::Value> {
    vec![
        mcp_tool(
            "cc_task_read",
            "读取当前任务、依赖和前置产物",
            serde_json::json!({}),
        ),
        mcp_tool(
            "cc_thread_read",
            "读取当前任务可见的协作消息",
            serde_json::json!({"limit": {"type": "integer", "minimum": 1, "maximum": 200}}),
        ),
        mcp_tool(
            "cc_message_send",
            "向项目或指定 Agent 发送持久化消息",
            serde_json::json!({
                "body": {"type": "string"},
                "targetAgentId": {"type": ["string", "null"]}
            }),
        ),
        mcp_tool(
            "cc_review_submit",
            "提交结构化审核结论；Reviewer 每次运行必须且只能提交一次",
            serde_json::json!({
                "targetTaskId": {"type": "string"},
                "verdict": {"type": "string", "enum": ["approved", "changes_requested"]},
                "summary": {"type": "string"}
            }),
        ),
        mcp_tool(
            "cc_artifact_publish",
            "发布当前工作目录内的中间产物",
            serde_json::json!({
                "artifactType": {"type": "string"},
                "uri": {"type": "string"},
                "summary": {"type": "string"},
                "contentHash": {"type": ["string", "null"]}
            }),
        ),
        mcp_tool(
            "cc_artifact_inspect",
            "查看当前任务或直接依赖任务的产物记录",
            serde_json::json!({"artifactId": {"type": "string"}}),
        ),
        mcp_tool(
            "cc_approval_request",
            "暂停当前 Run 并向用户请求审批",
            serde_json::json!({
                "approvalType": {"type": "string"},
                "summary": {"type": "string"}
            }),
        ),
        mcp_tool(
            "cc_approval_read",
            "查询当前 Run 发起的审批状态",
            serde_json::json!({"approvalId": {"type": "string"}}),
        ),
        mcp_tool(
            "cc_heartbeat",
            "续租当前 Run，长任务应定期调用",
            serde_json::json!({"leaseSeconds": {"type": "integer", "minimum": 30, "maximum": 600}}),
        ),
        mcp_tool(
            "cc_knowledge_search",
            "搜索当前项目的任务、对话、运行、交付物、审核和已验证经验，并返回可追溯的原文引用",
            serde_json::json!({
                "query": {"type": "string"},
                "sourceTypes": {
                    "type": ["array", "null"],
                    "items": {
                        "type": "string",
                        "enum": ["project", "task", "conversation", "run", "artifact", "review", "memory"]
                    }
                },
                "limit": {"type": "integer", "minimum": 1, "maximum": 100}
            }),
        ),
        mcp_tool(
            "cc_memory_search",
            "搜索已验证的项目记忆和当前 Run 的候选记忆",
            serde_json::json!({
                "query": {"type": "string"},
                "namespace": {"type": ["string", "null"]},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100}
            }),
        ),
        mcp_tool(
            "cc_memory_put",
            "记录候选经验；只有当前 Run 成功后才会晋升为已验证记忆",
            serde_json::json!({
                "namespace": {"type": "string"},
                "key": {"type": "string"},
                "value": {"type": "string"},
                "confidence": {"type": "number", "minimum": 0, "maximum": 1}
            }),
        ),
        mcp_tool(
            "cc_runtime_event",
            "记录结构化运行事件",
            serde_json::json!({
                "eventType": {"type": "string"},
                "payload": {"type": "object"}
            }),
        ),
    ]
}

fn mcp_tool(name: &str, description: &str, properties: serde_json::Value) -> serde_json::Value {
    let required = match name {
        "cc_message_send" => vec!["body"],
        "cc_review_submit" => vec!["targetTaskId", "verdict", "summary"],
        "cc_artifact_publish" => vec!["artifactType", "uri", "summary"],
        "cc_artifact_inspect" => vec!["artifactId"],
        "cc_approval_request" => vec!["approvalType", "summary"],
        "cc_approval_read" => vec!["approvalId"],
        "cc_knowledge_search" => vec!["query"],
        "cc_memory_search" => vec!["query"],
        "cc_memory_put" => vec!["namespace", "key", "value"],
        "cc_runtime_event" => vec!["eventType"],
        _ => Vec::new(),
    };
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
            "required": required,
        }
    })
}

fn collaboration_command_from_tool(
    name: &str,
    arguments: &serde_json::Value,
) -> Result<CollaborationCommand> {
    let string = |key: &str| -> Result<String> {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("工具参数缺少 {key}"))
    };
    Ok(match name {
        "cc_task_read" => CollaborationCommand::TaskRead,
        "cc_thread_read" => CollaborationCommand::ThreadRead {
            limit: arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
        },
        "cc_message_send" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct MessageArgs {
                body: String,
                target_agent_id: Option<String>,
            }
            let message: MessageArgs = serde_json::from_value(arguments.clone())
                .context("发送参数无效，请使用 body 和可选的 targetAgentId")?;
            CollaborationCommand::MessageSend {
                target_agent_id: message.target_agent_id,
                body: message.body,
            }
        }
        "cc_review_submit" => CollaborationCommand::ReviewSubmit {
            target_task_id: string("targetTaskId")?,
            verdict: serde_json::from_value(
                arguments
                    .get("verdict")
                    .cloned()
                    .ok_or_else(|| anyhow!("工具参数缺少 verdict"))?,
            )
            .context("verdict 必须是 approved 或 changes_requested")?,
            summary: string("summary")?,
        },
        "cc_artifact_publish" => CollaborationCommand::ArtifactPublish {
            artifact_type: string("artifactType")?,
            uri: string("uri")?,
            summary: string("summary")?,
            content_hash: arguments
                .get("contentHash")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        },
        "cc_artifact_inspect" => CollaborationCommand::ArtifactInspect {
            artifact_id: string("artifactId")?,
        },
        "cc_approval_request" => CollaborationCommand::ApprovalRequest {
            approval_type: string("approvalType")?,
            summary: string("summary")?,
        },
        "cc_approval_read" => CollaborationCommand::ApprovalRead {
            approval_id: string("approvalId")?,
        },
        "cc_heartbeat" => CollaborationCommand::Heartbeat {
            lease_seconds: arguments
                .get("leaseSeconds")
                .and_then(serde_json::Value::as_u64),
        },
        "cc_knowledge_search" => CollaborationCommand::KnowledgeSearch {
            query: string("query")?,
            source_types: arguments
                .get("sourceTypes")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect()
                }),
            limit: arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
        },
        "cc_memory_search" => CollaborationCommand::MemorySearch {
            query: string("query")?,
            namespace: arguments
                .get("namespace")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            limit: arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
        },
        "cc_memory_put" => CollaborationCommand::MemoryPut {
            namespace: string("namespace")?,
            key: string("key")?,
            value: string("value")?,
            confidence: arguments
                .get("confidence")
                .and_then(serde_json::Value::as_f64),
        },
        "cc_runtime_event" => CollaborationCommand::RuntimeEvent {
            event_type: string("eventType")?,
            payload: arguments.get("payload").cloned(),
        },
        _ => return Err(anyhow!("未知协作工具：{name}")),
    })
}

fn scoped_artifact_uri(cwd: &Path, uri: &str) -> Result<String> {
    let raw = uri.trim();
    if raw.is_empty() {
        return Err(anyhow!("产物地址不能为空"));
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let canonical_cwd = cwd
        .canonicalize()
        .with_context(|| format!("解析工作目录失败：{}", cwd.display()))?;
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("产物不存在：{}", path.display()))?;
    if !canonical_path.starts_with(&canonical_cwd) {
        return Err(anyhow!("产物必须位于当前任务工作目录内"));
    }
    Ok(canonical_path.display().to_string())
}

struct HttpRequest {
    method: String,
    path: String,
    authorization: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(anyhow!("连接提前关闭"));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(anyhow!("请求头过大"));
        }
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).context("请求头不是 UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or_else(|| anyhow!("缺少请求行"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut content_length = 0_usize;
    let mut authorization = String::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().context("Content-Length 无效")?;
        } else if name.eq_ignore_ascii_case("authorization") {
            authorization = value.trim().to_string();
        }
    }
    if content_length > MAX_BODY_BYTES {
        return Err(anyhow!("请求体过大"));
    }
    while bytes.len().saturating_sub(header_end) < content_length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(anyhow!("请求体不完整"));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(HttpRequest {
        method,
        path,
        authorization,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn response_error(message: &str) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": message})
}

fn write_json_response(
    stream: &mut TcpStream,
    status: u16,
    value: serde_json::Value,
) -> Result<()> {
    let body = value.to_string();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Entity",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    Ok(())
}

fn write_empty_response(stream: &mut TcpStream, status: u16) -> Result<()> {
    let reason = if status == 202 { "Accepted" } else { "OK" };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    use super::{
        collaboration_command_from_tool, dispatch, find_bytes, handle_connection,
        handle_mcp_request, CollaborationCommand, RunScope,
    };
    use crate::multi_agent::{
        AgentRole, AgentRuntimeKind, ClaimTaskInput, CreateAgentInput, CreateProjectInput,
        CreateTaskInput, MultiAgentStore, PermissionProfile, WorkspaceMode,
    };

    #[test]
    fn finds_http_header_separator() {
        assert_eq!(find_bytes(b"GET / HTTP/1.1\r\n\r\n", b"\r\n\r\n"), Some(14));
    }

    #[test]
    fn scoped_commands_can_message_and_record_unverified_memory() {
        let root = std::env::temp_dir().join(format!("crewvela-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let project = store
            .create_project(CreateProjectInput {
                name: "bridge".to_string(),
                goal: "test scoped commands".to_string(),
                root_path: root.display().to_string(),
                default_branch: "main".to_string(),
            })
            .unwrap();
        let agent = store
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Codex".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Worker,
                description: "worker".to_string(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            
            persona: Default::default(),
            persona_custom: String::new(),})
            .unwrap();
        let task = store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "task".to_string(),
                description: String::new(),
                acceptance_criteria: "done".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 3,
                retry_backoff_seconds: 5,
            })
            .unwrap();
        let run = store
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let scope = RunScope {
            project_id: project.id.clone(),
            task_id: task.id.clone(),
            run_id: run.id.clone(),
            agent_id: agent.id.clone(),
            cwd: root.clone(),
        };

        let tools = handle_mcp_request(
            &store,
            &scope,
            br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .unwrap();
        assert!(tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "cc_message_send"));
        assert!(tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "cc_review_submit"));
        assert!(tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "cc_knowledge_search"));
        let message_result = handle_mcp_request(
            &store,
            &scope,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cc_message_send","arguments":{"body":"发现接口约束"}}}"#
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(message_result["result"]["isError"], false);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let scopes = Arc::new(Mutex::new(HashMap::from([(
            "scoped-token".to_string(),
            scope.clone(),
        )])));
        let server_store = store.clone();
        let server_scopes = scopes.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream, &server_store, &server_scopes).unwrap();
        });
        let body = r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"bridge-test","version":"1"}}}"#;
        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer scoped-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            address,
            body.len(),
            body
        )
        .unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("crewvela-collaboration"));
        assert!(response.contains("2025-03-26"));

        dispatch(
            &store,
            &scope,
            CollaborationCommand::MemoryPut {
                namespace: "pattern".to_string(),
                key: "bridge-test".to_string(),
                value: "仅在成功后晋升".to_string(),
                confidence: Some(0.9),
            },
        )
        .unwrap();

        let knowledge = dispatch(
            &store,
            &scope,
            CollaborationCommand::KnowledgeSearch {
                query: "scoped commands".to_string(),
                source_types: Some(vec!["project".to_string()]),
                limit: Some(5),
            },
        )
        .unwrap();
        assert!(!knowledge.as_array().unwrap().is_empty());

        let snapshot = store.snapshot(&project.id).unwrap();
        assert!(snapshot.messages.iter().any(|message| {
            message.sender_id.as_deref() == Some(agent.id.as_str())
                && message.body == "发现接口约束"
        }));
        assert!(snapshot.memories.is_empty());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn http_mcp_direct_messages_cross_tasks_without_leaking_to_other_members() {
        use serde_json::{json, Value};
        use std::io::{BufRead, BufReader};
        use std::time::{Duration, Instant};

        let root = std::env::temp_dir().join(format!("cc-bridge-thread-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let project = store.create_project(CreateProjectInput {
            name: "HTTP thread isolation".into(), goal: "cross-task member messages".into(),
            root_path: root.display().to_string(), default_branch: "main".into(),
        }).unwrap();
        let mut scopes = HashMap::new();
        let mut agents = Vec::new();
        for name in ["alpha", "beta", "observer"] {
            let agent = store.create_agent(CreateAgentInput {
                avatar_id: None, project_id: project.id.clone(), name: name.into(),
                runtime_kind: AgentRuntimeKind::Codex, role: AgentRole::Worker,
                description: String::new(), model: None, permission_profile: PermissionProfile::Default,
                max_concurrency: 1, persona: Default::default(), persona_custom: String::new(),
            }).unwrap();
            let task = store.create_task(CreateTaskInput {
                project_id: project.id.clone(), parent_id: None, stage: 0,
                title: format!("separate task for {name}"), description: String::new(),
                acceptance_criteria: "message arrives only at its recipient".into(),
                verification_command: None, assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None, workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0, required_capabilities: Vec::new(), max_attempts: 1,
                retry_backoff_seconds: 5,
            }).unwrap();
            let run = store.claim_task(ClaimTaskInput {
                task_id: task.id.clone(), agent_id: agent.id.clone(), lease_seconds: 60,
            }).unwrap();
            scopes.insert(format!("token-{name}"), RunScope {
                project_id: project.id.clone(), task_id: task.id, run_id: run.id,
                agent_id: agent.id.clone(), cwd: root.clone(),
            });
            agents.push(agent);
        }
        let initial_message_count = store.snapshot(&project.id).unwrap().messages.len();
        assert!(store.enqueue_cli_message_task(&project.id, &agents[1].id, "bridge-private", "alpha-to-beta private envelope").unwrap());
        store.put_memory(crate::multi_agent::PutMemoryInput {
            project_id: project.id.clone(), namespace: "private".into(), key: "envelope".into(),
            value: "alpha-to-beta private memory".into(), source_task_id: Some("cli-message-bridge-private".into()),
            source_run_id: None, confidence: 1.0,
        }).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let scopes = Arc::new(Mutex::new(scopes));
        let server_store = store.clone();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut accepted = 0;
            while accepted < 17 {
                assert!(Instant::now() < deadline, "HTTP test did not receive all requests");
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Windows inherits the listener's nonblocking mode.
                        // Only accept polling is nonblocking in this fixture;
                        // production incoming() hands the handler blocking IO.
                        stream.set_nonblocking(false).unwrap();
                        handle_connection(stream, &server_store, &scopes).unwrap();
                        accepted += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("HTTP test accept failed: {error}"),
                }
            }
        });
        let request = |token: &str, body: Value| -> (u16, Value) {
            let body = serde_json::to_string(&body).unwrap();
            let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
            client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            client.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            // Exercise the connection-before-payload boundary deliberately.
            std::thread::sleep(Duration::from_millis(20));
            write!(client, "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            // HTTP framing ends at Content-Length. Waiting for TCP EOF can
            // report a Windows connection reset after a complete response.
            let mut reader = BufReader::new(client);
            let mut status_line = String::new();
            assert!(reader.read_line(&mut status_line).unwrap() > 0);
            let status = status_line.split_whitespace().nth(1).unwrap().parse().unwrap();
            let mut content_length = None;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0, "truncated HTTP headers");
                if line == "\r\n" { break; }
                let (name, value) = line.split_once(':').expect("invalid HTTP header");
                if name.eq_ignore_ascii_case("Content-Length") {
                    assert!(content_length.is_none(), "duplicate Content-Length");
                    content_length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut response_body = vec![0; content_length.expect("missing Content-Length")];
            reader.read_exact(&mut response_body).expect("truncated HTTP response body");
            (status, serde_json::from_slice(&response_body).unwrap())
        };
        for token in ["token-alpha", "token-beta", "token-observer"] {
            let (status, body) = request(token, json!({
                "jsonrpc":"2.0", "id":1, "method":"initialize",
                "params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"thread-regression","version":"1"}},
            }));
            assert_eq!(status, 200);
            assert_eq!(body["result"]["serverInfo"]["name"], "crewvela-collaboration");
        }
        let tool = |token: &str, name: &str, arguments: Value| -> Value {
            let (status, body) = request(token, json!({
                "jsonrpc":"2.0", "id":2, "method":"tools/call",
                "params":{"name":name,"arguments":arguments},
            }));
            assert_eq!(status, 200);
            assert_eq!(body["result"]["isError"], false, "{body}");
            serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        tool("token-alpha", "cc_message_send", json!({"targetAgentId":agents[1].id,"body":"alpha-to-beta across tasks"}));
        let received = tool("token-beta", "cc_thread_read", json!({"limit":20}));
        assert!(received.as_array().unwrap().iter().any(|message| message["body"] == "alpha-to-beta across tasks"));
        tool("token-beta", "cc_message_send", json!({"targetAgentId":agents[0].id,"body":"beta-to-alpha reply"}));
        let received = tool("token-alpha", "cc_thread_read", json!({"limit":20}));
        assert!(received.as_array().unwrap().iter().any(|message| message["body"] == "beta-to-alpha reply"));
        let observer = tool("token-observer", "cc_thread_read", json!({"limit":20}));
        assert!(observer.as_array().unwrap().iter().all(|message| {
            message["body"] != "alpha-to-beta across tasks" && message["body"] != "beta-to-alpha reply"
        }), "third member must not read directed messages: {observer}");
        for query in ["", "alpha-to-beta"] {
            for sources in [None, Some(json!(["conversation"]))] {
                let mut arguments = json!({"query":query,"limit":100});
                if let Some(sources) = sources { arguments["sourceTypes"] = sources; }
                let observer = tool("token-observer", "cc_knowledge_search", arguments);
                assert!(observer.as_array().unwrap().iter().all(|result| {
                    !result["content"].as_str().unwrap().contains("alpha-to-beta")
                        && !result["content"].as_str().unwrap().contains("beta-to-alpha")
                }), "knowledge search leaked another member's private conversation: {observer}");
            }
            let recipient = tool("token-beta", "cc_knowledge_search", json!({"query":query,"limit":100}));
            assert!(recipient.as_array().unwrap().iter().any(|result| result["content"] == "alpha-to-beta across tasks"),
                "recipient lost its cross-task message in knowledge search: {recipient}");
        }
        let observer_memory = tool("token-observer", "cc_memory_search", json!({"query":"alpha-to-beta","limit":100}));
        assert!(observer_memory.as_array().unwrap().is_empty(), "private verified memory leaked: {observer_memory}");
        let receiver_memory = tool("token-beta", "cc_memory_search", json!({"query":"alpha-to-beta","limit":100}));
        assert_eq!(receiver_memory.as_array().unwrap().len(), 1);
        let (status, _) = request("incorrect-token", json!({
            "jsonrpc":"2.0", "id":3, "method":"tools/call",
            "params":{"name":"cc_message_send","arguments":{"targetAgentId":agents[1].id,"body":"unauthorized injection"}},
        }));
        assert_eq!(status, 403);
        server.join().unwrap();
        assert_eq!(store.snapshot(&project.id).unwrap().messages.len(), initial_message_count + 2);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_recipient_cannot_fall_back_to_broadcast() {
        use serde_json::json;
        for invalid in [json!({"body":"private","target_agent_id":"b"}),
                        json!({"body":"private","targetAgentId":7})] {
            assert!(collaboration_command_from_tool("cc_message_send", &invalid).is_err());
        }
        for public in [json!({"body":"public"}), json!({"body":"public","targetAgentId":null})] {
            assert!(matches!(collaboration_command_from_tool("cc_message_send", &public).unwrap(),
                CollaborationCommand::MessageSend { target_agent_id: None, .. }));
        }
    }

    #[test]
    fn parses_structured_review_tool_arguments() {
        let command = collaboration_command_from_tool(
            "cc_review_submit",
            &serde_json::json!({
                "targetTaskId": "implementation",
                "verdict": "changes_requested",
                "summary": "缺少边界测试"
            }),
        )
        .unwrap();
        assert!(matches!(
            command,
            CollaborationCommand::ReviewSubmit {
                target_task_id,
                verdict: crate::multi_agent::ReviewVerdict::ChangesRequested,
                summary,
            } if target_task_id == "implementation" && summary == "缺少边界测试"
        ));
    }

    #[test]
    fn parses_knowledge_search_tool_arguments() {
        let command = collaboration_command_from_tool(
            "cc_knowledge_search",
            &serde_json::json!({
                "query": "历史审核结论",
                "sourceTypes": ["review", "conversation"],
                "limit": 12
            }),
        )
        .unwrap();
        assert!(matches!(
            command,
            CollaborationCommand::KnowledgeSearch {
                query,
                source_types: Some(source_types),
                limit: Some(12),
            } if query == "历史审核结论"
                && source_types == vec!["review".to_string(), "conversation".to_string()]
        ));
    }

    #[test]
    fn dropping_a_stale_run_lease_keeps_the_current_run_scope() {
        let root =
            std::env::temp_dir().join(format!("crewvela-bridge-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let bridge = super::CollaborationBridge::start(store.clone()).unwrap();
        let stale = bridge.register_run("p", "t", "run-stale", "agent", &root);
        let token = stale.access().token.clone();
        let current = bridge.register_run("p", "t", "run-current", "agent", &root);
        assert_eq!(
            current.access().token,
            token,
            "同一 Agent 的 Run 复用 token"
        );
        drop(stale);
        let scope = bridge.scopes.lock().unwrap().get(&token).cloned();
        drop(current);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
        let scope = scope.expect("旧 Run 的 lease 释放不应吊销当前 Run 的作用域");
        assert_eq!(scope.run_id, "run-current");
    }
}
