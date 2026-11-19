//! Explicit live regression for the managed Codex boundary used by group chat.
//! The caller supplies a fresh temporary CODEX_HOME containing only its own
//! authentication copy and inert model settings; production history is never read.
use super::*;

fn session_models(root: &Path, session_id: &str) -> Result<Vec<String>> {
    let mut models = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            models.extend(session_models(&entry.path(), session_id)?);
        } else if entry.file_name().to_string_lossy().ends_with(&format!("-{session_id}.jsonl")) {
            for line in std::fs::read_to_string(entry.path())?.lines() {
                let frame: Value = serde_json::from_str(line)?;
                if frame["type"] == "turn_context" {
                    if let Some(model) = frame.pointer("/payload/model").and_then(Value::as_str) {
                        models.push(model.into());
                    }
                }
            }
        }
    }
    Ok(models)
}

#[test]
#[ignore = "requires CREWVELA_RUN_REAL_CODEX_MODEL=1 and an isolated CODEX_HOME; at most two new short model calls"]
fn real_managed_codex_inherits_model_for_new_and_resumed_sessions() {
    assert_eq!(std::env::var("CREWVELA_RUN_REAL_CODEX_MODEL").as_deref(), Ok("1"));
    let root = PathBuf::from(std::env::var_os("CREWVELA_CODEX_MODEL_PROBE_ROOT").expect("isolated root required"));
    let code_home = PathBuf::from(std::env::var_os("CODEX_HOME").expect("isolated CODEX_HOME required"));
    let canonical_root = root.canonicalize().unwrap();
    assert_eq!(code_home.canonicalize().unwrap(), root.join("codex-home").canonicalize().unwrap());
    assert_eq!(std::fs::read_to_string(root.join(".isolated-model-probe")).unwrap(), "crewvela-codex-model-probe-v1\n");
    assert!(canonical_root.starts_with(std::env::temp_dir().canonicalize().unwrap()), "live acceptance must use OS temporary storage");
    crate::test_profile::initialize(&root.join("app")).unwrap();
    let cwd = root.join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let source = std::fs::read_to_string(code_home.join("config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&source).unwrap();
    let expected_model = config["model"].as_str().expect("explicitly copied current model");
    let command = codex_adapter_command().unwrap();
    let cli = command.get_envs().find_map(|(key, value)|
        (key == "CODEX_PATH").then_some(value).flatten().map(|value| value.to_string_lossy().into_owned()));
    let mut evidence = json!({"requestedModel":expected_model,"modelOverride":null,"codexPath":cli,
        "boundary":"production execute_managed_acp; isolated application profile, CODEX_HOME and workspace; no Tauri GUI",
        "calls":[]});
    let outcome = (|| -> Result<()> {
        let mut resume = None;
        for (index, marker) in ["CREWVELA_CODEX_MODEL_NEW_OK", "CREWVELA_CODEX_MODEL_RESUME_OK"].into_iter().enumerate() {
            let cancelled = Arc::new(AtomicBool::new(false));
            let tool_seen = Arc::new(AtomicBool::new(false));
            let permission_cancel = cancelled.clone();
            let permission_seen = tool_seen.clone();
            let event_cancel = cancelled.clone();
            let event_seen = tool_seen.clone();
            let done = AtomicBool::new(false);
            let prompt = format!("Reply with exactly {marker}. Do not call any tool, read any file, or contact any service.");
            let result = std::thread::scope(|scope| {
                scope.spawn(|| {
                    let started = Instant::now();
                    while !done.load(Ordering::Acquire) {
                        if started.elapsed() >= Duration::from_secs(90) { cancelled.store(true, Ordering::Release); break; }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                });
                let result = execute_managed_acp(ManagedAcpKind::Codex, &format!("isolated-model-probe-{index}"),
                    cwd.to_str().unwrap(), resume.as_deref(), &prompt, "", "", ManagedAcpMode::Inherit,
                    &cancelled, None,
                    Some(Box::new(move |_, _| { permission_seen.store(true,Ordering::Release); permission_cancel.store(true,Ordering::Release); Ok(false) })),
                    Some(Box::new(move |update| {
                        if matches!(update["sessionUpdate"].as_str(), Some("tool_call"|"tool_call_update")) {
                            event_seen.store(true,Ordering::Release); event_cancel.store(true,Ordering::Release);
                        }
                    })), None, &[]);
                done.store(true, Ordering::Release);
                result
            });
            let result = match result {
                Ok(ManagedAcpOutcome::Completed(result)) => result,
                Ok(ManagedAcpOutcome::Unavailable(reason)) => return Err(anyhow!(reason)),
                Err(error) => return Err(error),
            };
            let models = session_models(&code_home.join("sessions"), &result.session_id)?;
            evidence["calls"].as_array_mut().unwrap().push(json!({"sessionId":result.session_id,
                "resumed":resume.is_some(),"protocolModels":models,"output":result.text,"toolSeen":tool_seen.load(Ordering::Acquire)}));
            if result.text.trim() != marker { return Err(anyhow!("unexpected acceptance marker")); }
            if tool_seen.load(Ordering::Acquire) { return Err(anyhow!("live probe attempted a tool")); }
            if models.is_empty() || models.iter().any(|model| model != expected_model) {
                return Err(anyhow!("native turn_context did not preserve the configured model"));
            }
            if resume.as_ref().is_some_and(|session| session != &result.session_id) {
                return Err(anyhow!("resume silently opened a different native session"));
            }
            resume = Some(result.session_id);
        }
        Ok(())
    })();
    evidence["passed"] = json!(outcome.is_ok());
    evidence["error"] = json!(outcome.as_ref().err().map(|error|format!("{error:#}")));
    let path = root.join("acceptance.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    eprintln!("CODEX_MODEL_ACCEPTANCE={} evidence={}",outcome.is_ok(),path.display());
    assert!(outcome.is_ok(),"{}",outcome.unwrap_err());
}
