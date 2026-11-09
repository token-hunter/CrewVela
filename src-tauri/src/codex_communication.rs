//! Give the CLI outbox its own permission without making a read-only project writable.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

// This mutates a spawn specification used by PTY (program/args/env/cwd only).
// Configure stdio, Windows creation flags, and other process attributes afterward.
pub(crate) fn configure(
    command: &mut Command,
    executable: &Path,
    cwd: &Path,
    outbox: &Path,
) -> Result<()> {
    let args = normalize_cli_args(command.get_args().map(OsString::from).collect())?;
    if args
        .iter()
        .take_while(|arg| arg.as_os_str() != "--")
        .any(|arg| arg == "-p")
    {
        bail!("当前 Codex config/read 不支持读取 --profile 文件层，无法安全继承通信权限；未变更原命令或用户配置，请使用默认配置启动此会话");
    }
    if args
        .iter()
        .take_while(|arg| arg.as_os_str() != "--")
        .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
    {
        return Ok(());
    }
    let mut probe = crate::multi_agent::command_for_path(executable);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                probe.env(key, value);
            }
            None => {
                probe.env_remove(key);
            }
        }
    }
    let mut explicit_mode = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" {
            break;
        }
        if matches!(arg.as_ref(), "-c" | "--config" | "-p" | "--profile") {
            let value = args
                .get(index + 1)
                .context("Codex 启动配置缺少参数；未变更原权限")?;
            probe.arg(&args[index]).arg(value);
            index += 2;
        } else if matches!(arg.as_ref(), "-s" | "--sandbox") {
            explicit_mode = Some(
                args.get(index + 1)
                    .context("Codex sandbox 缺少参数")?
                    .to_string_lossy()
                    .into_owned(),
            );
            index += 2;
        } else {
            if arg.starts_with("--config=") || arg.starts_with("--profile=") {
                probe.arg(&args[index]);
            }
            if let Some(mode) = arg.strip_prefix("--sandbox=") {
                explicit_mode = Some(mode.into());
            }
            index += 1;
        }
    }
    if explicit_mode.as_deref() == Some("danger-full-access") {
        return Ok(());
    }
    probe.arg("app-server");
    let mut effective = crate::codex_title_summary::read_effective_config(probe, cwd)
        .context("无法安全读取 Codex 原权限，未启动会话、未扩大权限")?;
    if let Some(mode) = explicit_mode {
        effective["default_permissions"] = Value::Null;
        effective["sandbox_mode"] = json!(mode);
    }
    let overrides = permission_overrides(&effective, cwd, outbox)?;
    std::fs::create_dir_all(outbox).context("无法创建应用通信目录")?;
    if overrides.is_empty() {
        // config/read can leave both policies unset even though the interactive
        // CLI resolves them to read-only. In that case --add-dir is fatal after
        // the first TUI frame. Preserve the original invocation and implicit
        // policy instead of guessing a permission profile or adding roots.
        return Ok(());
    }
    // Remove every normalized spelling of the higher-priority CLI sandbox flag.
    // Tokens after -- are prompt/child arguments and must remain uninterpreted.
    let mut retained = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--" {
            retained.extend_from_slice(&args[index..]);
            break;
        }
        if args[index] == "--sandbox" {
            index += 2;
            continue;
        }
        retained.push(args[index].clone());
        index += 1;
    }
    insert_before_separator(
        &mut retained,
        overrides
            .into_iter()
            .flat_map(|value| [OsString::from("-c"), value.into()])
            .collect(),
    );
    replace_arguments(command, retained);
    Ok(())
}

/// Normalize Clap value forms before either probing or rewriting the invocation.
/// Config overrides retain their order; high-level approval flags synthesize their
/// overrides after all -c values, just as Codex 0.167's SharedCliOptions does.
fn normalize_cli_args(args: Vec<OsString>) -> Result<Vec<OsString>> {
    let mut normalized = Vec::new();
    let mut index = 0;
    let mut approve_for_me = false;
    let mut explicit_sandbox = false;
    let mut explicit_approval = false;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" {
            normalized.extend_from_slice(&args[index..]);
            break;
        }
        if matches!(arg.as_ref(), "--approve-for-me" | "--not-so-yolo") {
            approve_for_me = true;
            index += 1;
            continue;
        }
        if arg == "--full-auto" {
            bail!("无法安全继承旧版 --full-auto 权限；未变更原命令，请使用当前 Codex 支持的 sandbox 与审批选项");
        }
        if arg == "--yolo" {
            normalized.push("--dangerously-bypass-approvals-and-sandbox".into());
            index += 1;
            continue;
        }
        let mut option = None;
        for (short, long, canonical) in [
            ("-c", "--config", "-c"),
            ("-p", "--profile", "-p"),
            ("-s", "--sandbox", "--sandbox"),
            ("-a", "--ask-for-approval", "--ask-for-approval"),
        ] {
            if arg == short || arg == long {
                let value = args
                    .get(index + 1)
                    .context("Codex 启动配置缺少参数；未变更原权限")?
                    .clone();
                option = Some((canonical, value, 2));
                break;
            }
            if let Some(value) = arg.strip_prefix(&format!("{long}=")) {
                option = Some((canonical, value.into(), 1));
                break;
            }
            if !arg.starts_with("--") {
                if let Some(value) = arg.strip_prefix(short).filter(|value| !value.is_empty()) {
                    option = Some((
                        canonical,
                        value.strip_prefix('=').unwrap_or(value).into(),
                        1,
                    ));
                    break;
                }
            }
        }
        if let Some((canonical, value, consumed)) = option {
            if value.is_empty() {
                bail!("Codex 启动配置参数为空；未变更原权限");
            }
            explicit_sandbox |= canonical == "--sandbox";
            explicit_approval |= canonical == "--ask-for-approval";
            normalized.extend([canonical.into(), value]);
            index += consumed;
        } else {
            normalized.push(args[index].clone());
            index += 1;
        }
    }
    if approve_for_me {
        if explicit_sandbox
            || explicit_approval
            || normalized
                .iter()
                .take_while(|arg| arg.as_os_str() != "--")
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
        {
            bail!("Codex --approve-for-me 与显式 sandbox/审批选项冲突；未变更原权限");
        }
        insert_before_separator(
            &mut normalized,
            [
                r#"approvals_reviewer="auto_review""#,
                r#"approval_policy="on-request""#,
                r#"sandbox_mode="workspace-write""#,
            ]
            .into_iter()
            .flat_map(|value| [OsString::from("-c"), value.into()])
            .collect(),
        );
    }
    Ok(normalized)
}

fn insert_before_separator(args: &mut Vec<OsString>, additions: Vec<OsString>) {
    let position = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args.splice(position..position, additions);
}

fn replace_arguments(command: &mut Command, args: Vec<OsString>) {
    let mut replacement = Command::new(command.get_program());
    if let Some(directory) = command.get_current_dir() {
        replacement.current_dir(directory);
    }
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                replacement.env(key, value);
            }
            None => {
                replacement.env_remove(key);
            }
        }
    }
    replacement.args(args);
    *command = replacement;
}

fn permission_overrides(effective: &Value, cwd: &Path, outbox: &Path) -> Result<Vec<String>> {
    let outbox = absolute_path(outbox)?;
    let cwd = absolute_path(cwd)?;
    if cwd.starts_with(&outbox) {
        bail!("通信目录覆盖业务工作目录，拒绝扩大 Codex 写权限");
    }
    let parent = effective["default_permissions"]
        .as_str()
        .filter(|name| !name.is_empty());
    let legacy = effective["sandbox_mode"].as_str();
    if parent == Some(":danger-full-access")
        || (parent.is_none() && legacy == Some("danger-full-access"))
    {
        return Ok(Vec::new());
    }
    if parent.is_none() && legacy.is_none() {
        return Ok(Vec::new());
    }
    let profile = format!("crewvela-communication-{}", uuid::Uuid::new_v4().simple());
    let mut entries = Vec::new();
    let mut filesystem = Vec::new();
    let parent = match parent {
        Some(parent) => {
            validate_parent(effective, parent, &cwd, &outbox)?;
            parent
        }
        None => match legacy {
            Some("read-only") => ":read-only",
            Some("workspace-write") => ":workspace",
            _ => bail!("无法安全继承 Codex sandbox 策略；未扩大权限"),
        },
    };
    entries.push(format!(
        "permissions.{profile}.extends={}",
        serde_json::to_string(parent)?
    ));
    if effective["default_permissions"].as_str().is_none() && legacy == Some("workspace-write") {
        let settings = &effective["sandbox_workspace_write"];
        if let Some(roots) = settings["writable_roots"].as_array() {
            let mut workspace_roots = Vec::new();
            for root in roots {
                let root = root.as_str().context("Codex 原可写目录无法安全解析")?;
                workspace_roots.push(format!("{}=true", serde_json::to_string(root)?));
            }
            entries.push(format!(
                "permissions.{profile}.workspace_roots={{{}}}",
                workspace_roots.join(",")
            ));
        }
        for (key, token) in [
            ("exclude_tmpdir_env_var", ":tmpdir"),
            ("exclude_slash_tmp", ":slash_tmp"),
        ] {
            if settings[key] == true {
                filesystem.push(format!("{}=\"read\"", serde_json::to_string(token)?));
            }
        }
        if let Some(network) = settings["network_access"].as_bool() {
            entries.push(format!("permissions.{profile}.network.enabled={network}"));
        }
    }
    let path = outbox.to_string_lossy();
    // Codex treats '?' in a Win32 verbatim prefix as a glob, which cannot grant writes.
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
    };
    if path.contains(['*', '?', '[', ']', '{', '}']) {
        bail!("通信目录含 Codex 权限规则不支持的 glob 字符，无法授予精确路径；未扩大权限");
    }
    filesystem.push(format!("{}=\"write\"", serde_json::to_string(&path)?));
    // Pass a table: Codex's -c dotted-key parser does not unquote path segments.
    entries.push(format!(
        "permissions.{profile}.filesystem={{{}}}",
        filesystem.join(",")
    ));
    entries.push(format!(
        "default_permissions={}",
        serde_json::to_string(&profile)?
    ));
    Ok(entries)
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    // Resolve existing parents too: the new outbox must not hide a link to the business directory.
    if absolute.exists() {
        return absolute.canonicalize().map_err(Into::into);
    }
    let parent = absolute.parent().context("通信目录缺少父目录")?;
    Ok(absolute_path(parent)?.join(absolute.file_name().context("通信目录名称无效")?))
}

fn validate_parent(effective: &Value, initial: &str, cwd: &Path, outbox: &Path) -> Result<()> {
    let mut visited = HashSet::new();
    let mut name = initial;
    let mut roots = vec![cwd.to_path_buf()];
    loop {
        if matches!(name, ":read-only" | ":workspace") {
            return Ok(());
        }
        if !visited.insert(name) {
            bail!("Codex 权限继承存在循环；未扩大权限");
        }
        let profile = effective["permissions"]
            .get(name)
            .filter(|value| value.is_object())
            .context("Codex 原命名权限不可读取；未扩大权限")?;
        if let Some(extra) = profile["workspace_roots"].as_object() {
            for (root, enabled) in extra {
                if enabled == true {
                    roots.push(expand_path(root)?);
                }
            }
        }
        if let Some(rules) = profile["filesystem"].as_object() {
            for (root, access) in rules {
                let bases = match root.as_str() {
                    ":workspace_roots" => roots.clone(),
                    ":root" => vec![outbox
                        .ancestors()
                        .last()
                        .context("无法解析权限根目录")?
                        .to_path_buf()],
                    ":minimal" => vec![outbox.to_path_buf()],
                    ":tmpdir" => vec![std::env::temp_dir()],
                    ":slash_tmp" => vec![PathBuf::from("/tmp")],
                    token if token.starts_with(':') => {
                        bail!("Codex 权限含未知特殊路径；未扩大权限")
                    }
                    _ => vec![expand_path(root)?],
                };
                for base in bases {
                    if access == "deny" && denied_by(&base.to_string_lossy(), outbox) {
                        bail!("Codex 原权限显式拒绝通信目录；保留原策略，未扩大权限");
                    }
                    if let Some(children) = access.as_object() {
                        for (child, access) in children {
                            let child = if child == "." {
                                base.clone()
                            } else {
                                base.join(child)
                            };
                            if access == "deny" && denied_by(&child.to_string_lossy(), outbox) {
                                bail!("Codex 原权限显式拒绝通信目录；保留原策略，未扩大权限");
                            }
                        }
                    }
                }
            }
        }
        let Some(parent) = profile["extends"].as_str() else {
            return Ok(());
        };
        name = parent;
    }
}

fn expand_path(path: &str) -> Result<PathBuf> {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        return Ok(dirs::home_dir()
            .context("无法解析 Codex 权限中的用户目录")?
            .join(path.get(2..).unwrap_or_default()));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        bail!("Codex 权限路径无法安全解析；未扩大权限");
    }
    if !path
        .to_string_lossy()
        .contains(['*', '?', '[', ']', '{', '}'])
    {
        return absolute_path(&path);
    }
    Ok(path)
}

fn denied_by(rule: &str, outbox: &Path) -> bool {
    let normalize = |text: &str| {
        let text = text.trim_start_matches(r"\\?\").replace('\\', "/");
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let rule = normalize(rule);
    outbox.ancestors().any(|ancestor| {
        let path = normalize(&ancestor.to_string_lossy());
        if rule.contains(['[', ']', '{', '}']) {
            // Do not reopen an uncertain deny glob with a narrower write grant.
            let prefix = rule.split(['*', '?', '[', '{']).next().unwrap_or("");
            path.starts_with(prefix)
        } else {
            glob_matches(rule.as_bytes(), path.as_bytes())
        }
    })
}

fn glob_matches(pattern: &[u8], path: &[u8]) -> bool {
    let mut previous = vec![false; path.len() + 1];
    previous[0] = true;
    for &byte in pattern {
        let mut next = vec![false; path.len() + 1];
        if byte == b'*' {
            next[0] = previous[0];
        }
        for index in 1..=path.len() {
            next[index] = if byte == b'*' {
                previous[index] || next[index - 1]
            } else {
                previous[index - 1] && (byte == b'?' || byte == path[index - 1])
            };
        }
        previous = next;
    }
    previous[path.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("crewvela-communication-{}", uuid::Uuid::new_v4()))
    }

    fn values(overrides: &[String]) -> Value {
        let mut document = toml::Value::Table(toml::map::Map::new());
        for entry in overrides {
            let parsed: toml::Value = toml::from_str(entry).unwrap();
            fn merge(left: &mut toml::Value, right: toml::Value) {
                if let (Some(left), Some(right)) = (left.as_table_mut(), right.as_table()) {
                    for (key, value) in right {
                        if let Some(previous) = left.get_mut(key) {
                            merge(previous, value.clone());
                        } else {
                            left.insert(key.clone(), value.clone());
                        }
                    }
                } else {
                    *left = right;
                }
            }
            merge(&mut document, parsed);
        }
        serde_json::to_value(document).unwrap()
    }

    #[test]
    fn cli_spellings_keep_override_order_and_stop_at_literal_separator() {
        let input = [
            "resume",
            "fixture-id",
            "-cdefault_permissions=\":workspace\"",
            "-c=default_permissions=\":read-only\"",
            "-p=readonly",
            "-sread-only",
            "--ask-for-approval=never",
            "--",
            "-cdefault_permissions=\":danger-full-access\"",
            "--full-auto",
        ];
        let actual = normalize_cli_args(input.into_iter().map(OsString::from).collect()).unwrap();
        assert_eq!(
            actual,
            [
                "resume",
                "fixture-id",
                "-c",
                "default_permissions=\":workspace\"",
                "-c",
                "default_permissions=\":read-only\"",
                "-p",
                "readonly",
                "--sandbox",
                "read-only",
                "--ask-for-approval",
                "never",
                "--",
                "-cdefault_permissions=\":danger-full-access\"",
                "--full-auto"
            ]
            .map(OsString::from)
        );
        for (forms, expected) in [
            (["-cvalue=1", "-c=value=1", "--config=value=1"], "-c"),
            (["-preadonly", "-p=readonly", "--profile=readonly"], "-p"),
            (
                ["-sread-only", "-s=read-only", "--sandbox=read-only"],
                "--sandbox",
            ),
        ] {
            let baseline = normalize_cli_args(vec![forms[0].into()]).unwrap();
            assert_eq!(baseline[0], expected);
            for form in forms {
                assert_eq!(normalize_cli_args(vec![form.into()]).unwrap(), baseline);
            }
        }
    }

    #[test]
    fn approve_for_me_keeps_native_approval_overrides_without_erasing_readonly_profile() {
        for flag in ["--approve-for-me", "--not-so-yolo"] {
            let args = normalize_cli_args(vec![
                "-cdefault_permissions=\":read-only\"".into(),
                "-capproval_policy=\"never\"".into(),
                flag.into(),
            ])
            .unwrap();
            assert!(!args.iter().any(|arg| arg == flag));
            let overrides = args
                .chunks_exact(2)
                .map(|pair| pair[1].to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let actual = values(&overrides);
            assert_eq!(actual["default_permissions"], ":read-only");
            assert_eq!(actual["approval_policy"], "on-request");
            assert_eq!(actual["approvals_reviewer"], "auto_review");
            assert_eq!(actual["sandbox_mode"], "workspace-write");
            let root = root();
            let derived = values(
                &permission_overrides(&actual, &root.join("business"), &root.join("outbox"))
                    .unwrap(),
            );
            let name = derived["default_permissions"].as_str().unwrap();
            assert_eq!(derived["permissions"][name]["extends"], ":read-only");
        }
        for args in [
            vec!["--approve-for-me", "-sread-only"],
            vec!["--approve-for-me", "-anever"],
            vec!["--approve-for-me", "--yolo"],
            vec!["--full-auto"],
        ] {
            assert!(normalize_cli_args(args.into_iter().map(OsString::from).collect()).is_err());
        }
    }

    #[test]
    fn literal_bypass_and_legacy_flags_after_separator_are_not_options() {
        let literal = [
            "--dangerously-bypass-approvals-and-sandbox",
            "-cdefault_permissions=\":danger-full-access\"",
            "--full-auto",
        ];
        for prefix in [vec![], vec!["--approve-for-me"]] {
            let args = prefix
                .into_iter()
                .chain(["--"])
                .chain(literal)
                .map(OsString::from)
                .collect();
            let actual = normalize_cli_args(args).unwrap();
            let separator = actual.iter().position(|arg| arg == "--").unwrap();
            assert_eq!(actual[separator + 1..], literal.map(OsString::from));
            assert!(!actual[..separator]
                .iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"));
        }
    }

    #[test]
    fn unsupported_profile_and_glob_outbox_refuse_without_rewriting_command() {
        let root = root();
        for option in [
            vec!["-p", "readonly"],
            vec!["-preadonly"],
            vec!["-p=readonly"],
            vec!["--profile=readonly"],
        ] {
            let mut command = Command::new("must-not-spawn");
            command.args(&option);
            let before = command.get_args().map(OsString::from).collect::<Vec<_>>();
            assert!(configure(
                &mut command,
                Path::new("must-not-spawn"),
                &root.join("business"),
                &root.join("outbox")
            )
            .unwrap_err()
            .to_string()
            .contains("--profile"));
            assert_eq!(
                command.get_args().map(OsString::from).collect::<Vec<_>>(),
                before
            );
        }
        assert!(permission_overrides(
            &json!({"sandbox_mode":"read-only"}),
            &root.join("business"),
            &root.join("[literal]/outbox")
        )
        .unwrap_err()
        .to_string()
        .contains("glob"));
    }

    #[test]
    fn readonly_outbox_grant_does_not_make_workspace_or_network_writable() {
        let root = root();
        let result = values(
            &permission_overrides(
                &json!({"sandbox_mode":"read-only"}),
                &root.join("business"),
                &root.join("outbox"),
            )
            .unwrap(),
        );
        let name = result["default_permissions"].as_str().unwrap();
        let profile = &result["permissions"][name];
        assert_eq!(profile["extends"], ":read-only");
        assert_eq!(profile["filesystem"].as_object().unwrap().len(), 1);
        assert!(profile.get("network").is_none());
        assert!(profile.get("workspace_roots").is_none());
    }

    #[test]
    fn custom_profile_preserves_inherited_restrictions_and_rejects_outbox_denials() {
        let root = root();
        let outbox = root.join("outbox");
        let business = root.join("business");
        let mut original = json!({"default_permissions":"restricted","permissions":{"restricted":{"extends":":read-only","network":{"enabled":false},"filesystem":{":workspace_roots":{"**/*.env":"deny"}}}}});
        let derived = values(&permission_overrides(&original, &business, &outbox).unwrap());
        let name = derived["default_permissions"].as_str().unwrap();
        assert_eq!(derived["permissions"][name]["extends"], "restricted");
        assert!(derived["permissions"][name].get("network").is_none());
        for (path, value) in [
            (outbox.to_string_lossy().into_owned(), json!("deny")),
            (
                root.to_string_lossy().into_owned(),
                json!({"outbox":"deny"}),
            ),
            (outbox.to_string_lossy().into_owned(), json!({".":"deny"})),
            (
                root.join("out*").to_string_lossy().into_owned(),
                json!("deny"),
            ),
        ] {
            original["permissions"]["restricted"]["filesystem"] = json!({path: value});
            assert!(permission_overrides(&original, &business, &outbox)
                .unwrap_err()
                .to_string()
                .contains("显式拒绝"));
        }
        original["permissions"]["restricted"]["filesystem"] = json!({":root":"deny"});
        assert!(permission_overrides(&original, &business, &outbox).is_err());
    }

    #[test]
    fn legacy_workspace_policy_preserves_extra_roots_network_and_tmp_exclusions() {
        let root = root();
        let original = json!({"sandbox_mode":"workspace-write","sandbox_workspace_write":{"writable_roots":[root.join("shared")],"network_access":false,"exclude_tmpdir_env_var":true,"exclude_slash_tmp":true}});
        let derived = values(
            &permission_overrides(&original, &root.join("business"), &root.join("outbox")).unwrap(),
        );
        let name = derived["default_permissions"].as_str().unwrap();
        let profile = &derived["permissions"][name];
        assert_eq!(profile["extends"], ":workspace");
        assert_eq!(profile["network"]["enabled"], false);
        assert_eq!(profile["filesystem"][":tmpdir"], "read");
        assert_eq!(profile["filesystem"][":slash_tmp"], "read");
        assert_eq!(
            profile["workspace_roots"][root.join("shared").to_str().unwrap()],
            true
        );
    }

    #[test]
    fn invalid_or_recursive_profile_and_business_writable_root_are_rejected() {
        let root = root();
        let policy = json!({"default_permissions":"a","permissions":{"a":{"extends":"b"},"b":{"extends":"a"}}});
        assert!(
            permission_overrides(&policy, &root.join("business"), &root.join("outbox")).is_err()
        );
        assert!(permission_overrides(
            &json!({"default_permissions":"missing"}),
            &root.join("business"),
            &root.join("outbox")
        )
        .is_err());
        assert!(permission_overrides(
            &json!({"sandbox_mode":"read-only"}),
            &root.join("business"),
            &root
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed Codex Windows sandbox; no model, account, or user session is used"]
    fn real_readonly_codex_cli_discovers_sends_and_replies_with_only_outbox_writable() {
        use crate::{
            control::SessionActionQueue,
            sessions::{NewRuntimeSession, SessionManager},
        };
        let root = root();
        std::fs::create_dir_all(root.join("business")).unwrap();
        std::fs::create_dir(root.join("codex-home")).unwrap();
        std::fs::write(root.join(".cli-fixture"), "test-only").unwrap();
        let config = "sandbox_mode = \"read-only\"\napproval_policy = \"never\"\n";
        std::fs::write(root.join("codex-home/config.toml"), config).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let sender = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: root.join("business").display().to_string(),
                project_id: None,
            })
            .unwrap();
        let target = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "claude_code".into(),
                cwd: root.join("business").display().to_string(),
                project_id: None,
            })
            .unwrap();
        sessions.attach_runtime(&sender.id, 1).unwrap();
        sessions.attach_runtime(&target.id, 2).unwrap();
        let executable = crate::multi_agent::resolve_runtime_executable(
            crate::multi_agent::AgentRuntimeKind::Codex,
        )
        .unwrap();
        let run = |identity: &str, arguments: Vec<&str>, grant: bool| {
            let mut command = crate::multi_agent::command_for_path(&executable);
            command
                .current_dir(root.join("business"))
                .env("CODEX_HOME", root.join("codex-home"));
            command.arg("sandbox");
            if grant {
                configure(
                    &mut command,
                    &executable,
                    &root.join("business"),
                    &root.join("outbox"),
                )
                .unwrap();
            }
            command
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "messaging::tests::isolated_cli_child_process",
                    "--nocapture",
                ])
                .env("CREWVELA_MESSAGE_TEST_ROOT", &root)
                .env(
                    "CREWVELA_MESSAGE_TEST_ARGS",
                    serde_json::to_string(&arguments).unwrap(),
                )
                .env("CREWVELA_MESSAGE_ASSERT_READONLY", "1")
                .env("CREWVELA_SESSION_ID", identity);
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            command.output().unwrap()
        };
        // The original path must remain red under the real read-only token.
        let denied = run(
            &sender.id,
            vec!["send", "--to", &target.id, "--body", "original denied"],
            false,
        );
        assert!(
            !denied.status.success(),
            "baseline unexpectedly wrote the outbox"
        );
        let baseline_error = format!(
            "{}{}",
            String::from_utf8_lossy(&denied.stdout),
            String::from_utf8_lossy(&denied.stderr)
        );
        assert!(baseline_error.contains("outbox") && baseline_error.contains("os error 5"),
            "baseline must reproduce the actual outbox permission error, not a sandbox startup failure: {baseline_error}");
        let listed = run(&sender.id, vec!["sessions"], true);
        assert!(
            listed.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&listed.stdout),
            String::from_utf8_lossy(&listed.stderr)
        );
        assert!(String::from_utf8_lossy(&listed.stdout).contains(&target.id));
        let body = "Skill CLI 真实沙箱通信\n保留 '引号' $变量 `反引号`";
        let sent = run(
            &sender.id,
            vec!["send", "--to", &target.id, "--body", body],
            true,
        );
        assert!(
            sent.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&sent.stdout),
            String::from_utf8_lossy(&sent.stderr)
        );
        assert!(String::from_utf8_lossy(&sent.stdout)
            .contains("CLI_FIXTURE_BUSINESS_WRITE_DENIED=true"));
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        let message = queue.peek_next(&target.id).unwrap();
        assert_eq!(message.body, body);
        assert_eq!(message.from.as_ref().unwrap().session_id, sender.id);
        let replied = run(
            &target.id,
            vec!["send", "--to", &sender.id, "--body", "ACK:原发送方"],
            true,
        );
        assert!(
            replied.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&replied.stdout),
            String::from_utf8_lossy(&replied.stderr)
        );
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        assert_eq!(queue.peek_next(&sender.id).unwrap().body, "ACK:原发送方");
        assert_eq!(
            std::fs::read_to_string(root.join("codex-home/config.toml")).unwrap(),
            config
        );
        assert!(!root.join("business/forbidden.txt").exists());
        let denied_config = format!("default_permissions = \"restricted\"\n[permissions.restricted]\nextends = \":read-only\"\n[permissions.restricted.filesystem]\n{} = \"deny\"\n", serde_json::to_string(&root.join("outbox").to_string_lossy()).unwrap());
        std::fs::write(root.join("codex-home/config.toml"), &denied_config).unwrap();
        let mut denied_command = crate::multi_agent::command_for_path(&executable);
        denied_command
            .env("CODEX_HOME", root.join("codex-home"))
            .arg("sandbox");
        let original_args = denied_command
            .get_args()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let error = configure(
            &mut denied_command,
            &executable,
            &root.join("business"),
            &root.join("outbox"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("显式拒绝"), "{error:#}");
        assert_eq!(
            denied_command
                .get_args()
                .map(OsString::from)
                .collect::<Vec<_>>(),
            original_args
        );
        assert_eq!(
            std::fs::read_to_string(root.join("codex-home/config.toml")).unwrap(),
            denied_config
        );
        drop(queue);
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed Codex config/read and Windows sandbox; no auth or model"]
    fn real_joined_cli_options_preserve_readonly_config_and_outbox_scope() {
        let root = root();
        let business = root.join("business");
        let outbox = root.join("outbox");
        let home = root.join("codex-home");
        std::fs::create_dir_all(&business).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let base = "default_permissions = \":workspace\"\napproval_policy = \"never\"\n";
        std::fs::write(home.join("config.toml"), base).unwrap();
        std::fs::write(
            home.join("readonly.config.toml"),
            "default_permissions = \":read-only\"\n",
        )
        .unwrap();
        let executable = crate::multi_agent::resolve_runtime_executable(
            crate::multi_agent::AgentRuntimeKind::Codex,
        )
        .unwrap();
        let cases: Vec<Vec<&str>> = vec![
            vec!["-cdefault_permissions=\":read-only\""],
            vec!["-c=default_permissions=\":read-only\""],
            vec!["--config=default_permissions=\":read-only\""],
            vec!["-preadonly"],
            vec!["-p=readonly"],
            vec!["--profile=readonly"],
            vec!["-sread-only"],
            vec!["-s=read-only"],
            vec!["--sandbox=read-only"],
            vec![
                "-cdefault_permissions=\":workspace\"",
                "-cdefault_permissions=\":read-only\"",
            ],
            vec!["-cdefault_permissions=\":workspace\"", "-sread-only"],
            vec!["-cdefault_permissions=\":read-only\"", "--approve-for-me"],
            vec!["-cdefault_permissions=\":read-only\"", "--not-so-yolo"],
        ];
        for args in cases {
            if args
                .iter()
                .any(|arg| arg.starts_with("-p") || arg.starts_with("--profile"))
            {
                let script = format!("$ErrorActionPreference='Stop'; try {{ Set-Content -LiteralPath '{}' -Value 'forbidden' }} catch [UnauthorizedAccessException] {{ exit 0 }}; exit 9", business.join("forbidden.txt").display());
                let mut native = crate::multi_agent::command_for_path(&executable);
                native
                    .current_dir(&business)
                    .env("CODEX_HOME", &home)
                    .args(&args)
                    .arg("sandbox")
                    .arg("--")
                    .args([
                        "powershell.exe",
                        "-NoLogo",
                        "-NoProfile",
                        "-Command",
                        &script,
                    ]);
                use std::os::windows::process::CommandExt;
                let native = native.creation_flags(0x08000000).output().unwrap();
                assert!(
                    native.status.success(),
                    "native profile did not make business readonly: {args:?}\n{}",
                    String::from_utf8_lossy(&native.stderr)
                );
                let mut command = crate::multi_agent::command_for_path(&executable);
                command
                    .current_dir(&business)
                    .env("CODEX_HOME", &home)
                    .args(&args);
                let original = command.get_args().map(OsString::from).collect::<Vec<_>>();
                assert!(configure(&mut command, &executable, &business, &outbox)
                    .unwrap_err()
                    .to_string()
                    .contains("--profile"));
                assert_eq!(
                    command.get_args().map(OsString::from).collect::<Vec<_>>(),
                    original
                );
                continue;
            }
            if !args
                .iter()
                .any(|arg| arg.starts_with("-s") || arg.starts_with("--sandbox"))
            {
                let mut direct = crate::multi_agent::command_for_path(&executable);
                direct
                    .env("CODEX_HOME", &home)
                    .args(&args)
                    .arg("app-server");
                let effective =
                    crate::codex_title_summary::read_effective_config(direct, &business).unwrap();
                assert_eq!(
                    effective["default_permissions"], ":read-only",
                    "native config/read must verify the test input"
                );
            }
            let mut command = crate::multi_agent::command_for_path(&executable);
            command
                .current_dir(&business)
                .env("CODEX_HOME", &home)
                .args(&args);
            configure(&mut command, &executable, &business, &outbox).unwrap();
            let actual = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let generated = actual
                .windows(2)
                .filter(|pair| pair[0] == "-c")
                .map(|pair| pair[1].clone())
                .filter(|value| {
                    value.starts_with("permissions.crewvela-")
                        || value.starts_with("default_permissions=\"crewvela-")
                })
                .collect::<Vec<_>>();
            let derived = values(&generated);
            let name = derived["default_permissions"].as_str().unwrap();
            assert_eq!(
                derived["permissions"][name]["extends"], ":read-only",
                "configure lost a restrictive CLI override: {args:?}"
            );
            if args
                .iter()
                .any(|arg| matches!(*arg, "--approve-for-me" | "--not-so-yolo"))
            {
                let mut verify = Command::new(command.get_program());
                verify
                    .env("CODEX_HOME", &home)
                    .args(command.get_args())
                    .arg("app-server");
                let effective =
                    crate::codex_title_summary::read_effective_config(verify, &business)
                        .unwrap_or_else(|error| {
                            panic!("isolated final config/read failed for {actual:?}: {error:#}")
                        });
                assert_eq!(effective["approval_policy"], "on-request");
                assert_eq!(effective["approvals_reviewer"], "auto_review");
                assert_eq!(effective["permissions"][name]["extends"], ":read-only");
            }
            let script = format!("$ErrorActionPreference='Stop'; Set-Content -LiteralPath {} -Value 'outbox-only'; try {{ Set-Content -LiteralPath {} -Value 'forbidden' }} catch [UnauthorizedAccessException] {{ exit 0 }}; exit 9", format!("'{}'", outbox.join("probe.txt").display()), format!("'{}'", business.join("forbidden.txt").display()));
            command.arg("sandbox").arg("--").args([
                "powershell.exe",
                "-NoLogo",
                "-NoProfile",
                "-Command",
                &script,
            ]);
            use std::os::windows::process::CommandExt;
            let output = command.creation_flags(0x08000000).output().unwrap();
            assert!(
                output.status.success(),
                "actual sandbox did not preserve the restrictive override: {args:?}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!business.join("forbidden.txt").exists());
        }
        // The literal prompt is carried after --. It must neither bypass setup,
        // select a config/profile, nor conflict with a real approval option.
        let literal = [
            "--dangerously-bypass-approvals-and-sandbox",
            "-cdefault_permissions=\":danger-full-access\"",
            "-p=must-not-load",
            "--full-auto",
        ];
        for approve in [false, true] {
            let mut original = crate::multi_agent::command_for_path(&executable);
            original
                .current_dir(&business)
                .env("CODEX_HOME", &home)
                .arg("-cdefault_permissions=\":read-only\"");
            if approve {
                original.arg("--approve-for-me");
            }
            original.arg("--").args(literal);
            configure(&mut original, &executable, &business, &outbox).unwrap();
            let args = original.get_args().map(OsString::from).collect::<Vec<_>>();
            let separator = args.iter().position(|arg| arg == "--").unwrap();
            assert_eq!(args[separator + 1..], literal.map(OsString::from));
            assert!(args[..separator].iter().any(|arg| arg
                .to_string_lossy()
                .starts_with("default_permissions=\"crewvela-")));
            let mut sandbox = Command::new(original.get_program());
            sandbox.current_dir(&business).env("CODEX_HOME", &home).args(&args[..separator]).arg("sandbox").arg("--")
                .arg(which::which("node").unwrap()).args(["-e", "const fs=require('node:fs'),a=require('node:assert/strict');a.deepEqual(process.argv.slice(1),JSON.parse(process.env.CREWVELA_LITERAL_ARGS));fs.writeFileSync(process.env.CREWVELA_LITERAL_OUTBOX,'literal preserved');a.throws(()=>fs.writeFileSync(process.env.CREWVELA_LITERAL_BUSINESS,'forbidden'),e=>e.code==='EACCES'||e.code==='EPERM');", "--"])
                .args(&args[separator + 1..]).env("CREWVELA_LITERAL_ARGS", serde_json::to_string(&literal).unwrap())
                .env("CREWVELA_LITERAL_OUTBOX", outbox.join("literal-probe.txt")).env("CREWVELA_LITERAL_BUSINESS", business.join("literal-forbidden.txt"));
            use std::os::windows::process::CommandExt;
            let output = sandbox.creation_flags(0x08000000).output().unwrap();
            assert!(
                output.status.success(),
                "literal prompt changed effective permissions: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            std::fs::read_to_string(home.join("config.toml")).unwrap(),
            base
        );
        assert!(!home.join("auth.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn actual_original_codex_pty_waits_for_composer_before_receiving_cli_message() {
        use crate::{
            control::SessionActionQueue,
            pty::PtyManager,
            sessions::{NewRuntimeSession, SessionManager},
        };
        use std::time::{Duration, Instant};
        let root = root();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".cli-fixture"), "test-only").unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let sender = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "claude_code".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let target = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let input = root.join("original-codex-input.bin");
        let ready = root.join("show-composer");
        let mut child = Command::new(which::which("node").unwrap());
        child.args(["-e", "const fs=require('node:fs'),[input,ready]=process.argv.slice(1);process.stdin.setRawMode(true);process.stdin.on('data',b=>fs.appendFileSync(input,b));console.log('FIXTURE_STARTED: I have sent the reply through the Skill.');let shown=false;setInterval(()=>{if(!shown&&fs.existsSync(ready)){shown=true;process.stdout.write('\\x1b[2J\\x1b[HWorked for 1s\\n› Ask Codex to do anything\\n');}},10)"])
            .arg(&input).arg(&ready);
        let ptys = PtyManager::new();
        let pty = ptys.open_agent_program(&child, &target.id, false).unwrap();
        struct Stop(PtyManager, u32);
        impl Drop for Stop {
            fn drop(&mut self) {
                let _ = self.0.kill(self.1);
            }
        }
        let stop = Stop(ptys.clone(), pty);
        sessions.attach_runtime(&target.id, pty).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        while !output.contains("FIXTURE_STARTED") {
            let chunks = ptys.poll_all();
            crate::mark_interactive_runtime_prompts(&chunks, &sessions);
            for chunk in chunks.values() {
                output.push_str(&chunk.data);
            }
            assert!(
                Instant::now() < deadline,
                "fake original terminal did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !sessions.is_ready_for_prompt(&target.id),
            "a claim to have replied is not an input composer"
        );
        let mut cli = Command::new(std::env::current_exe().unwrap());
        cli.args([
            "--exact",
            "messaging::tests::isolated_cli_child_process",
            "--nocapture",
        ])
        .env("CREWVELA_MESSAGE_TEST_ROOT", &root)
        .env(
            "CREWVELA_MESSAGE_TEST_ARGS",
            serde_json::to_string(&[
                "send",
                "--to",
                &target.id,
                "--body",
                "original-terminal-message\n第二行",
            ])
            .unwrap(),
        )
        .env("CREWVELA_SESSION_ID", &sender.id)
        .env_remove("CREWVELA_MESSAGE_ASSERT_READONLY");
        use std::os::windows::process::CommandExt;
        let sent = cli.creation_flags(0x08000000).output().unwrap();
        assert!(
            sent.status.success(),
            "{}",
            String::from_utf8_lossy(&sent.stdout)
        );
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        assert!(queue.flush_ready(&sessions, &ptys).is_empty());
        assert!(
            !input.exists(),
            "no keys may be written while only reply text is visible"
        );
        std::fs::write(&ready, "show real fake-runtime composer").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !sessions.is_ready_for_prompt(&target.id) {
            crate::mark_interactive_runtime_prompts(&ptys.poll_all(), &sessions);
            assert!(
                Instant::now() < deadline,
                "fresh composer was not recognized"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(queue.flush_ready(&sessions, &ptys), [target.id.clone()]);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            queue.flush_pending_submits(&sessions, &ptys);
            let bytes = std::fs::read(&input).unwrap_or_default();
            if queue.peek_next(&target.id).is_none() && bytes.ends_with(b"\r") {
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.contains("original-terminal-message\n第二行"));
                assert!(text.contains(&format!("send --to '{}'", sender.id)));
                assert_eq!(bytes.iter().filter(|byte| **byte == b'\r').count(), 1);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "message was not actually submitted to its original PTY"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sessions.get(&target.id).unwrap().pty_id, Some(pty));
        assert!(!sessions.get(&target.id).unwrap().runtime_command_host);
        drop(stop);
        drop(queue);
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }
}
