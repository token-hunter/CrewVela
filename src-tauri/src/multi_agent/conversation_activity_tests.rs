use super::*;
use crate::multi_agent::{AppendRuntimeEventInput,HeartbeatRunInput};

#[test]
fn activity_survives_output_flood_and_recovers_with_exact_conversation_run() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(&fixture.store,&round.id,0,"chat",0,&fixture.agents[0]).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput {task_id:turn.task_id.clone(),agent_id:fixture.agents[0].clone(),lease_seconds:120}).unwrap();
    for (event_type,payload) in [
        ("analysis_activity",json!({})),
        ("tool_protocol",json!({"toolCallId":"read-1","kind":"read","status":"in_progress","locations":[{"path":"src/api/auth.ts"}],"content":["private tool result"]})),
        ("tool_protocol",json!({"toolCallId":"read-1","status":"completed"})),
    ] { fixture.store.append_runtime_event(AppendRuntimeEventInput {run_id:run.id.clone(),event_type:event_type.into(),payload}).unwrap(); }
    for index in 0..520 { fixture.store.append_runtime_event(AppendRuntimeEventInput {run_id:run.id.clone(),event_type:"output".into(),payload:json!({"stream":"stderr","content":format!("transport startup {index}")})}).unwrap(); }
    fixture.store.heartbeat_run(HeartbeatRunInput {run_id:run.id.clone(),lease_seconds:120}).unwrap();
    assert!(!fixture.store.snapshot(&fixture.project).unwrap().runtime_events.iter().any(|event| event.event_type=="agent_activity"));
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let chat = reopened.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(chat.activity_events.len(),3);
    assert!(chat.activity_events.iter().all(|event|event.run_id==run.id && event.agent_id==fixture.agents[0] && event.project_id==fixture.project));
    assert!(chat.messages.iter().any(|message|message.run_id.as_deref()==Some(&run.id)));
    let encoded=serde_json::to_string(&chat.activity_events).unwrap();
    assert!(!encoded.contains("private tool result")); assert!(!encoded.contains("transport startup"));
    let other = Fixture::new();
    assert!(other.store.conversation_snapshot(&other.project).unwrap().activity_events.is_empty());
}
