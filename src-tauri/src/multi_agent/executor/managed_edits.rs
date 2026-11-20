use super::{
    git_repository_layout, hidden_command, run_piped_command, spawn_passive_stream_reader,
    verify_delivery_worktree, AgentRuntimeKind, MultiAgentStore, RuntimeRequest,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_TEXT: usize = 2 * 1024 * 1024;
const MAX_BATCH: usize = 8 * 1024 * 1024;

pub(super) fn instruction(scopes: &[String]) -> String {
    format!(
        "本轮采用宿主受限文本交付。你可以读取现有代码，由 CrewVela 将你的实际文本修改应用到独立工作树。\n\
        允许写入的相对文件或目录为 {}。路径以当前工作目录为根，使用 /。最后只输出一个 JSON 对象，包含非空 summary 和 edits 数组。\n\
        新建文件用 {{\"operation\":\"create\",\"path\":\"src/example.ts\",\"text\":\"完整内容\"}}。\
        修改用 {{\"operation\":\"replace\",\"path\":\"src/example.ts\",\"before\":\"文件中唯一的原文\",\"after\":\"替换文本\"}}，可以对同一文件依次替换。\
        删除用 {{\"operation\":\"delete\",\"path\":\"src/example.ts\",\"before\":\"完整原文\"}}。\
        edits 必须包含实际修改，空交付不会通过。测试由 CrewVela 按获批的验收命令执行。",
        scopes.join("、")
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    summary: String,
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "lowercase", deny_unknown_fields)]
enum Edit {
    Create {
        path: String,
        text: String,
    },
    Replace {
        path: String,
        before: String,
        after: String,
    },
    Delete {
        path: String,
        before: String,
    },
}

impl Edit {
    fn path(&self) -> &str {
        match self {
            Self::Create { path, .. } | Self::Replace { path, .. } | Self::Delete { path, .. } => {
                path
            }
        }
    }
}

struct FileEdit {
    original: Option<String>,
    next: Option<String>,
    mode: &'static str,
    permissions: Option<fs::Permissions>,
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(
        !relative.is_empty()
            && relative.len() <= 512
            && !relative.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
            && !relative.chars().any(char::is_control),
        "变更路径必须是工作树内的普通相对路径"
    );
    let mut current = root.to_owned();
    for part in relative.split('/') {
        let lower = part.to_lowercase();
        let device = lower.split('.').next().unwrap_or_default().trim_end();
        let serial = device
            .strip_prefix("com")
            .or_else(|| device.strip_prefix("lpt"));
        ensure!(
            !part.is_empty()
                && !part.ends_with(['.', ' '])
                && !matches!(lower.as_str(), ".git" | ".hg" | ".svn")
                && !matches!(
                    device,
                    "con" | "aux" | "nul" | "prn" | "conin$" | "conout$" | "clock$"
                )
                && !serial.is_some_and(|n| [
                    "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"
                ]
                .contains(&n))
                && !part
                    .as_bytes()
                    .windows(2)
                    .any(|s| s[0] == b'~' && s[1].is_ascii_digit()),
            "变更路径包含保留名称、别名或路径穿越"
        );
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(meta.file_attributes() & 0x400 == 0, "变更路径包含重解析点");
                }
                ensure!(
                    !meta.file_type().is_symlink() && (meta.is_dir() || meta.is_file()),
                    "变更路径包含链接或特殊文件"
                );
                if meta.is_dir() {
                    ensure!(
                        !current.join(".git").try_exists()?,
                        "变更路径进入嵌套 Git 工作区"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(current)
}

fn file_state(path: &Path) -> Result<FileEdit> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileEdit {
                original: None,
                next: None,
                mode: "100644",
                permissions: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        meta.is_file() && meta.len() <= MAX_TEXT as u64,
        "只能修改 2 MiB 以内的 UTF-8 普通文件"
    );
    let mut options = fs::File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(meta.file_attributes() & 0x400 == 0, "不能跟随文件重解析点");
    }
    ensure!(meta.is_file(), "变更目标不是普通文件");
    let mut original = String::new();
    file.take(MAX_TEXT as u64 + 1)
        .read_to_string(&mut original)?;
    ensure!(
        original.len() <= MAX_TEXT && !original.contains('\0'),
        "文件不是可编辑文本"
    );
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
    };
    #[cfg(not(unix))]
    let mode = "100644";
    Ok(FileEdit {
        original: Some(original.clone()),
        next: Some(original),
        mode,
        permissions: Some(meta.permissions()),
    })
}

/// Read tools often present CRLF as LF. Preserve the existing file's convention.
fn with_line_endings(value: &str, file: &str) -> String {
    if file.contains("\r\n") && !file.replace("\r\n", "").contains('\n') {
        value.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        value.to_owned()
    }
}

fn prepare(cwd: &Path, scopes: &[String], edits: Vec<Edit>) -> Result<BTreeMap<String, FileEdit>> {
    ensure!(
        !scopes.is_empty() && !edits.is_empty() && edits.len() <= 256,
        "托管任务需要已确认的范围和 1 至 256 项文本修改"
    );
    let root = cwd.canonicalize()?;
    let paths: Vec<_> = edits.iter().map(|edit| edit.path().to_owned()).collect();
    MultiAgentStore::verify_write_paths(scopes, &paths)?;
    let mut files = BTreeMap::<String, FileEdit>::new();
    let mut total_size = 0;
    for edit in edits {
        let relative = edit.path().to_owned();
        let path = checked_path(&root, &relative)?;
        #[cfg(unix)]
        ensure!(
            scopes
                .iter()
                .any(|scope| Path::new(&relative).starts_with(scope.trim().replace('\\', "/"))),
            "文件名大小写不在获准的写入范围"
        );
        if !files.contains_key(&relative) {
            ensure!(
                !files.keys().any(|other| {
                    let first = other.to_lowercase();
                    let second = relative.to_lowercase();
                    first == second
                        || Path::new(&first).starts_with(&second)
                        || Path::new(&second).starts_with(&first)
                }),
                "同批文件路径存在别名或父子冲突"
            );
            let state = file_state(&path)?;
            total_size += state.original.as_ref().map_or(0, String::len);
            ensure!(total_size <= MAX_BATCH, "原文件总计超过 8 MiB");
            files.insert(relative.clone(), state);
        }
        let state = files.get_mut(&relative).unwrap();
        match edit {
            Edit::Create { text, .. } => {
                ensure!(state.next.is_none(), "新建目标已存在：{relative}");
                state.next = Some(text);
            }
            Edit::Replace { before, after, .. } => {
                let current = state
                    .next
                    .as_ref()
                    .with_context(|| format!("替换目标不存在：{relative}"))?;
                let before = with_line_endings(&before, current);
                ensure!(
                    !before.is_empty() && current.match_indices(&before).take(2).count() == 1,
                    "替换原文必须在 {relative} 中唯一匹配，请重新读取文件"
                );
                let after = with_line_endings(&after, current);
                state.next = Some(current.replacen(&before, &after, 1));
            }
            Edit::Delete { before, .. } => {
                let current = state
                    .next
                    .as_ref()
                    .with_context(|| format!("删除目标不存在：{relative}"))?;
                ensure!(
                    with_line_endings(&before, current) == *current,
                    "删除原文与 {relative} 不符"
                );
                state.next = None;
            }
        }
        ensure!(
            state
                .next
                .as_ref()
                .is_none_or(|v| v.len() <= MAX_TEXT && !v.contains('\0')),
            "交付文件不是 2 MiB 以内的 UTF-8 文本"
        );
    }
    files.retain(|_, edit| edit.original != edit.next);
    ensure!(!files.is_empty(), "原生回复没有实际文件变更");
    Ok(files)
}

fn patch_lines(patch: &mut String, text: &str, sign: char) {
    for line in text.split_inclusive('\n') {
        patch.push(sign);
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    }
}

// Build every header and line count from validated data. Model text can only become hunk contents.
fn patch_for(files: &BTreeMap<String, FileEdit>, prefix: &Path) -> Result<String> {
    use std::fmt::Write;
    let mut patch = String::new();
    for (relative, edit) in files {
        let path = prefix
            .join(relative)
            .to_str()
            .context("工作树路径不是 UTF-8")?
            .replace('\\', "/");
        let old_path = serde_json::to_string(&format!("a/{path}"))?;
        let new_path = serde_json::to_string(&format!("b/{path}"))?;
        writeln!(patch, "diff --git {old_path} {new_path}")?;
        if edit.original.is_none() {
            writeln!(patch, "new file mode {}", edit.mode)?;
        }
        if edit.next.is_none() {
            writeln!(patch, "deleted file mode {}", edit.mode)?;
        }
        let old = edit.original.as_deref().unwrap_or("");
        let new = edit.next.as_deref().unwrap_or("");
        if old.is_empty() && new.is_empty() {
            continue;
        }
        writeln!(
            patch,
            "--- {}",
            if edit.original.is_some() {
                &old_path
            } else {
                "/dev/null"
            }
        )?;
        writeln!(
            patch,
            "+++ {}",
            if edit.next.is_some() {
                &new_path
            } else {
                "/dev/null"
            }
        )?;
        let old_count = old.split_inclusive('\n').count();
        let new_count = new.split_inclusive('\n').count();
        writeln!(
            patch,
            "@@ -{},{old_count} +{},{new_count} @@",
            usize::from(old_count != 0),
            usize::from(new_count != 0)
        )?;
        patch_lines(&mut patch, old, '-');
        patch_lines(&mut patch, new, '+');
    }
    Ok(patch)
}

// The target repository may configure external filters or normalize line endings. Apply as raw
// worktree text against an empty, private Git directory so delivery cannot execute those filters.
struct PatchRepository(PathBuf);

impl PatchRepository {
    fn create() -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("crewvela-text-patch-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        let repo = Self(path);
        let initialized = repo
            .command()
            .args(["init", "--bare", "--template=", "-q"])
            .arg(&repo.0)
            .output()?;
        ensure!(initialized.status.success(), "无法准备受限 Git 文本写入");
        fs::create_dir_all(repo.0.join("info"))?;
        fs::write(
            repo.0.join("info/attributes"),
            "* -text -crlf !eol -filter -ident !working-tree-encoding\n",
        )?;
        Ok(repo)
    }

    fn command(&self) -> Command {
        let mut git = hidden_command("git");
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                git.env_remove(key);
            }
        }
        git.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.0.join("no-global-config"))
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.protectNTFS=true",
                "-c",
                "core.protectHFS=true",
            ])
            .arg("-c")
            .arg(format!(
                "core.attributesFile={}",
                self.0.join("no-global-attributes").display()
            ));
        git
    }
}

impl Drop for PatchRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn apply_patch(
    scratch: &PatchRepository,
    repo: &Path,
    patch: String,
    check: bool,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut git = scratch.command();
    git.current_dir(repo)
        .arg("--git-dir")
        .arg(&scratch.0)
        .arg("--work-tree")
        .arg(repo)
        .args(["apply", "--whitespace=nowarn", "--unidiff-zero", "-p1"]);
    #[cfg(unix)]
    {
        // Existing private files must never become world-readable during replacement. Permissions
        // of existing files are restored before publication succeeds. New worktree files are private.
        use std::os::unix::process::CommandExt;
        unsafe {
            git.pre_exec(|| {
                libc::umask(0o077);
                Ok(())
            });
        }
    }
    if check {
        git.arg("--check");
    }
    git.arg("-");
    let output = run_piped_command(
        git,
        Some(patch),
        cancelled,
        Duration::from_secs(60),
        "托管文本变更",
        |stdout, stderr| {
            (
                spawn_passive_stream_reader(stdout),
                spawn_passive_stream_reader(stderr),
            )
        },
    )?;
    ensure!(
        output.status.success(),
        "Git 无法应用本批文本变更：{}",
        output.stderr.trim()
    );
    Ok(())
}

pub(super) fn apply(
    request: &RuntimeRequest,
    reply: &str,
    cancelled: &AtomicBool,
) -> Result<String> {
    ensure!(request.managed_native(), "受限写入仅供独立托管工作树");
    let (repo, prefix) = verify_delivery_worktree(&request.project, &request.cwd)?;
    with_write_scope(
        &request.store,
        &request.run_id,
        &request.agent.id,
        request.agent.runtime_kind,
        &request.cwd,
        |scopes, _| apply_reply(&repo, &prefix, scopes, reply, cancelled),
    )
}

/// The authorization and mutation share the store lock with handoff, pause, and settlement.
fn with_write_scope<T>(
    store: &MultiAgentStore,
    run_id: &str,
    agent: &str,
    kind: AgentRuntimeKind,
    cwd: &Path,
    operation: impl FnOnce(&[String], &rusqlite::Connection) -> Result<T>,
) -> Result<T> {
    let connection = store.connection.lock().unwrap();
    let (raw, saved_cwd): (String, String) = connection.query_row(
        "SELECT s.write_paths_json,json_extract(r.session_locator_json,'$.cwd') FROM ma_runs r
         JOIN ma_tasks t ON t.id=r.task_id JOIN ma_agents a ON a.id=r.agent_id
         JOIN ma_conversation_task_scopes s ON s.task_id=t.id JOIN ma_conversation_plans p ON p.id=s.plan_id
         JOIN ma_projects j ON j.id=r.project_id
         WHERE r.id=?1 AND r.agent_id=?2 AND a.runtime_kind=?3 AND r.status='running' AND t.assignee_agent_id=r.agent_id
           AND t.workspace_mode='worktree' AND a.role IN ('worker','integrator')
           AND json_extract(p.data_json,'$.managed.status')='running' AND j.status='active'
           AND NOT EXISTS(SELECT 1 FROM ma_handoff_locks h WHERE h.project_id=r.project_id)",
        [run_id, agent, kind.as_str()], |r| Ok((r.get(0)?,r.get(1)?)),
    ).context("当前托管运行没有工作树写入授权")?;
    ensure!(
        Path::new(&saved_cwd).canonicalize()? == cwd.canonicalize()?,
        "托管工作树授权发生变化"
    );
    let scopes: Vec<String> = serde_json::from_str(&raw)?;
    operation(&scopes, &connection)
}

pub(in crate::multi_agent) fn file_tool_names(kind: AgentRuntimeKind) -> Result<[&'static str; 2]> {
    match kind {
        AgentRuntimeKind::Pi => Ok(["cc_pi_edit_read", "cc_pi_write_file"]),
        AgentRuntimeKind::KimiCode | AgentRuntimeKind::OpenCode => Ok(["cc_workspace_edit_read", "cc_workspace_write_file"]),
        _ => bail!("该 Runtime 未获托管文件工具授权"),
    }
}

pub(in crate::multi_agent) fn file_operation(
    store: &MultiAgentStore, run_id: &str, agent: &str, kind: AgentRuntimeKind,
    cwd: &Path, writing: bool, args: &serde_json::Value,
) -> Result<serde_json::Value> {
    use super::super::conversation::research;
    use sha2::{Digest, Sha256};
    with_write_scope(store, run_id, agent, kind, cwd, |scopes, connection| {
        let relative = args["path"].as_str().context("缺少文件相对路径")?;
        MultiAgentStore::verify_write_paths(scopes, &[relative.to_owned()])?;
        let read = || -> Result<String> {
            let policy = research::workspace_policy_for_run(connection, run_id, agent, cwd)?;
            let file = fs::File::open(research::readable_path(cwd, &policy.boundary, relative)?)?;
            ensure!(file.metadata()?.is_file(), "仅支持普通文本文件");
            let mut bytes = Vec::new();
            file.take(1_048_577).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 1_048_576 && !bytes.contains(&0), "精确编辑只支持 1 MiB 内的 UTF-8 文本");
            Ok(String::from_utf8(bytes)?)
        };
        if !writing {
            let text = read()?;
            return Ok(serde_json::json!({"revision":format!("{:x}", Sha256::digest(text.as_bytes())), "text":text}));
        }
        if let Some(expected) = args.get("revision") {
            ensure!(expected.as_str() == Some(format!("{:x}", Sha256::digest(read()?.as_bytes())).as_str()),
                "文件在读取后发生变化，未覆盖原文件");
        }
        write_text(cwd, scopes, relative, args["text"].as_str().context("缺少文件内容")?)?;
        Ok(serde_json::json!({"path":relative}))
    })
}

pub(super) fn write_text(cwd: &Path, scopes: &[String], relative: &str, text: &str) -> Result<()> {
    ensure!(
        text.len() <= MAX_TEXT && !text.contains('\0'),
        "工具写入必须是 2 MiB 以内的 UTF-8 文本"
    );
    let (repo, prefix) = git_repository_layout(cwd)?;
    MultiAgentStore::verify_write_paths(scopes, &[relative.to_owned()])?;
    #[cfg(unix)]
    ensure!(
        scopes
            .iter()
            .any(|scope| Path::new(relative).starts_with(scope.trim().replace('\\', "/"))),
        "文件名大小写不在获准的写入范围"
    );
    let path = checked_path(&repo.join(&prefix), relative)?;
    let mut state = file_state(&path)?;
    if state.original.as_deref() == Some(text) {
        return Ok(());
    }
    state.next = Some(text.to_owned());
    write_files(
        &repo,
        &prefix,
        &BTreeMap::from([(relative.to_owned(), state)]),
        &AtomicBool::new(false),
    )
}

/// Match the outermost object ending at the final '}'. Quoted source text inside edits may
/// contain arbitrary nested braces, escaped quotes and Markdown fences.
fn trailing_json_start(body: &str) -> Result<usize> {
    let bytes = body.as_bytes();
    ensure!(
        bytes.last() == Some(&b'}'),
        "原生最终回复缺少末尾 JSON 对象或含尾随内容"
    );
    let (mut depth, mut quoted) = (0usize, false);
    for (i, &byte) in bytes.iter().enumerate().rev() {
        match byte {
            b'"' => {
                let escapes = bytes[..i].iter().rev().take_while(|&&x| x == b'\\').count();
                if escapes % 2 == 0 {
                    quoted = !quoted;
                }
            }
            b'}' if !quoted => depth += 1,
            b'{' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    bail!("原生最终回复的 JSON 对象未完整闭合")
}

fn has_verdict_field(prefix: &str) -> bool {
    let lower = prefix.to_ascii_lowercase();
    lower.match_indices("verdict").any(|(start, word)| {
        let bytes = lower.as_bytes();
        if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            return false;
        }
        lower[start + word.len()..]
            .trim_start_matches(|ch: char| ch.is_whitespace() || "\"'\x60*".contains(ch))
            .starts_with([':', '：', '='])
    })
}

/// Prose and source snippets are evidence. A second standalone JSON document or an explicit
/// machine result in that evidence would make the decision ambiguous.
fn check_reply_prefix(prefix: &str) -> Result<()> {
    ensure!(!has_verdict_field(prefix), "说明中包含额外的 verdict 字段");
    let raw = prefix.as_bytes();
    let mut offset = 0;
    // Bound total parsing work even when an adversarial prefix contains overlapping bad objects.
    let mut budget = raw.len().saturating_mul(4).saturating_add(32);
    while let Some(next) = prefix[offset..].find('{') {
        let start = offset + next;
        offset = start + 1;
        let tail = &raw[start..];
        let first = tail[1..].iter().find(|b| !b.is_ascii_whitespace());
        if !matches!(first, Some(b'"' | b'}')) {
            continue;
        }
        ensure!(budget > 0, "原生回复含有过多重叠 JSON 片段");
        let size = tail.len().min(budget);
        let mut input = &tail[..size];
        let parsed = BTreeMap::<String, serde::de::IgnoredAny>::deserialize(
            &mut serde_json::Deserializer::from_reader(&mut input),
        );
        let spent = size - input.len();
        budget -= spent;
        match parsed {
            Ok(keys) => {
                let end = start + spent;
                let standalone = prefix[..start]
                    .chars()
                    .rev()
                    .find(|ch| *ch == '\n' || !ch.is_whitespace())
                    .is_none_or(|ch| ch == '\n');
                ensure!(
                    !standalone && !keys.contains_key("verdict") && !keys.contains_key("edits"),
                    "原生回复含有多个结构化对象"
                );
                ensure!(prefix.is_char_boundary(end), "JSON 片段边界无效");
                offset = end;
            }
            Err(error) if error.is_eof() => bail!("最终对象前还有一份未闭合的 JSON"),
            Err(_) => {}
        }
    }
    ensure!(
        !prefix.trim_end().ends_with(['[', '{']),
        "最终对象嵌在未闭合的 JSON 结构中"
    );
    Ok(())
}

pub(super) fn structured_reply(raw: &str) -> Result<&str> {
    ensure!(raw.len() <= MAX_BATCH, "原生结构化回复超过 8 MiB");
    const FENCE: &str = "\x60\x60\x60";
    let raw = raw.trim();
    let closed = raw
        .rsplit_once('\n')
        .filter(|(_, last)| last.trim() == FENCE);
    let body = closed.map_or(raw, |(content, _)| content.trim_end());
    let start = trailing_json_start(body)?;
    let object = &body[start..];
    // Callers deserialize the original slice into deny_unknown_fields structs. Duplicate fields
    // remain visible to Serde (conversion to serde_json::Value would silently discard them).
    serde_json::from_str::<serde::de::IgnoredAny>(object)
        .context("原生最终回复末尾不是合法 JSON 对象")?;
    let mut prefix = body[..start].trim_end();
    let last_line = prefix.rsplit('\n').next().unwrap_or("").trim();
    let mut in_fence = false;
    for line in prefix.lines() {
        let line = line.trim();
        if in_fence {
            if line == FENCE {
                in_fence = false;
            }
        } else if line.starts_with(FENCE) {
            in_fence = true;
        }
    }
    let opening = last_line.strip_prefix(FENCE);
    if closed.is_some() {
        ensure!(
            in_fence
                && opening.is_some_and(|lang| lang.is_empty() || lang.eq_ignore_ascii_case("json")),
            "最终 JSON 代码块起止标记不对应"
        );
        prefix = prefix[..prefix.rfind(last_line).unwrap()].trim_end();
    } else {
        ensure!(!in_fence, "最终 JSON 代码块缺少结束标记");
    }
    check_reply_prefix(prefix)?;
    Ok(object)
}

fn apply_reply(
    repo: &Path,
    prefix: &Path,
    scopes: &[String],
    reply: &str,
    cancelled: &AtomicBool,
) -> Result<String> {
    let json = structured_reply(reply)?;
    let delivery: Delivery =
        serde_json::from_str(json).context("原生回复缺少结构化的 summary 和 edits 文本交付")?;
    let summary = delivery.summary.trim();
    ensure!(
        !summary.is_empty() && summary.chars().count() <= 4000,
        "交付摘要为空或过长"
    );
    let cwd = repo.join(prefix);
    let files = prepare(&cwd, scopes, delivery.edits)?;
    write_files(repo, prefix, &files, cancelled)?;
    Ok(summary.to_owned())
}

fn write_files(
    repo: &Path,
    prefix: &Path,
    files: &BTreeMap<String, FileEdit>,
    cancelled: &AtomicBool,
) -> Result<()> {
    let patch = patch_for(files, prefix)?;
    let scratch = PatchRepository::create()?;
    apply_patch(&scratch, repo, patch.clone(), true, cancelled)?;
    for (relative, prepared) in files {
        let path = checked_path(&repo.join(prefix), relative)?;
        ensure!(
            file_state(&path)?.original == prepared.original,
            "应用前 {relative} 已变化，本批修改已停止"
        );
    }
    if cancelled.load(Ordering::Acquire) {
        bail!("托管任务已取消");
    }
    // Do not interrupt a Git worktree transaction halfway through its writes.
    let outcome = (|| {
        apply_patch(&scratch, repo, patch, false, &AtomicBool::new(false))?;
        for (relative, prepared) in files {
            let path = checked_path(&repo.join(prefix), relative)?;
            ensure!(
                file_state(&path)?.original == prepared.next,
                "文件实际内容不符，未确认交付 {relative}"
            );
            if prepared.next.is_some() {
                if let Some(permissions) = &prepared.permissions {
                    fs::set_permissions(path, permissions.clone())?;
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        // Git rejects ordinary hunk conflicts atomically. A filesystem failure can still interrupt
        // its final writes. Restore only observed postimages, preserving any concurrent user's edit.
        let mut undo = BTreeMap::new();
        let mut unknown = Vec::new();
        for (relative, prepared) in files {
            let path = checked_path(&repo.join(prefix), relative)?;
            let mut current = file_state(&path)?;
            if current.original == prepared.original {
                continue;
            }
            if current.original != prepared.next {
                unknown.push(relative.as_str());
                continue;
            }
            current.next = prepared.original.clone();
            current.mode = prepared.mode;
            undo.insert(relative.to_owned(), current);
        }
        if !undo.is_empty() {
            apply_patch(
                &scratch,
                repo,
                patch_for(&undo, prefix)?,
                false,
                &AtomicBool::new(false),
            )
            .with_context(|| format!("部分文件恢复失败，请核对原工作树。原错误：{error:#}"))?;
            for relative in undo.keys() {
                if let Some(permissions) = &files[relative].permissions {
                    fs::set_permissions(
                        checked_path(&repo.join(prefix), relative)?,
                        permissions.clone(),
                    )?;
                }
            }
        }
        ensure!(
            unknown.is_empty(),
            "文件已被其他程序改动，已保留当前内容，请核对 {}。原错误：{error:#}",
            unknown.join("、")
        );
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    struct Repo(PathBuf);
    impl Repo {
        fn new() -> Result<Self> {
            let root = std::env::temp_dir()
                .join(format!("crewvela-managed-edits-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root)?;
            ensure!(
                hidden_command("git")
                    .arg("-C")
                    .arg(&root)
                    .args(["init", "-q"])
                    .status()?
                    .success(),
                "git init"
            );
            fs::create_dir_all(root.join("project/src"))?;
            Ok(Self(root))
        }
        fn run(&self, edits: Value) -> Result<String> {
            apply_reply(
                &self.0,
                Path::new("project"),
                &["src".into()],
                &json!({"summary":"交付","edits":edits}).to_string(),
                &AtomicBool::new(false),
            )
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn managed_edits_structured_reply_accepts_one_terminal_object() {
        let object = r#"{"verdict":"approved","summary":"inspected source"}"#;
        let lead = "审查完成。以下为全部核对要点的最终书面结论（仅输出 JSON）：";
        let fence = "\x60\x60\x60";
        for raw in [
            object.to_owned(),
            format!("{lead}\n\n{object}"),
            format!("{lead}\r\n\r\n{fence}json\r\n{object}\r\n{fence}"),
            format!("例如\n{fence}js\nfetch(url, {{}})\n{fence}\n{object}"),
            format!("[INFO] {{\"event\":\"test\",\"status\":\"passed\"}}\n测试通过。\n{object}"),
            format!("**代码核对**：只读检查通过，判据见下文。\n{}\n- \x60fetch(url, {{}})\x60\n\n{object}", "说明".repeat(250)),
            format!("测试通过，读取了三个关键文件。\n{}\n\n{fence}json\n{object}\n{fence}", "证据".repeat(1000)),
            format!("本次自动化测试未通过，见 summary。\n\n{object}"),
        ] {
            assert_eq!(structured_reply(&raw).unwrap(), object);
        }
        for invalid in [
            format!("{object}\n{object}"),
            format!("{object}\n补充说明"),
            format!("Verdict: changes_requested\n\n{object}"),
            format!("JSON：\n\n{object}\n{object}"),
            format!("{{\"another_document\":true}}\n\n{object}"),
            format!("\x60verdict\x60 = approved\n\n{object}"),
            format!("{lead}\n\n{fence}json\n{object}\n{fence}\n末尾文字"),
            format!("{fence}json\n{object}"),
            format!("{fence}yaml\n{object}\n{fence}"),
            format!("[{object}"),
            format!("{{\"wrapper\":{object}"),
            "普通审查意见".to_owned(),
        ] {
            assert!(structured_reply(&invalid).is_err(), "{invalid}");
        }
        let duplicate = r#"{"verdict":"changes_requested","verdict":"approved","summary":"x"}"#;
        assert!(super::super::parse_managed_review_verdict(duplicate).is_err());
        let duplicate_edit = r#"{"summary":"x","edits":[],"edits":[]}"#;
        assert!(
            serde_json::from_str::<Delivery>(structured_reply(duplicate_edit).unwrap()).is_err()
        );
        let with_code = json!({"summary":"写入后交付","edits":[{"operation":"create","path":"src/x","text":"} {\n\x60\x60\x60\n\"quote\\\"}\""}]}).to_string();
        assert_eq!(
            structured_reply(&format!("确认原文件后，直接提交修改。\n\n{with_code}")).unwrap(),
            with_code
        );
    }

    /// The input contains only final native assistant text for the selected, correlatable turns.
    #[test]
    #[ignore = "set CREWVELA_NATIVE_REPLY_REPLAY to a JSON array of role, accepted and reply for selected real turns"]
    fn managed_edits_replay_selected_native_turns() -> Result<()> {
        #[derive(Deserialize)]
        struct Replay {
            role: String,
            accepted: bool,
            reply: String,
        }
        let path = std::env::var_os("CREWVELA_NATIVE_REPLY_REPLAY").context("缺少回放输入文件")?;
        let cases: Vec<Replay> = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(!cases.is_empty(), "没有可回放的轮次");
        for (i, case) in cases.into_iter().enumerate() {
            let outcome = match case.role.as_str() {
                "review" => super::super::parse_managed_review_verdict(&case.reply).map(|_| ()),
                "write" => structured_reply(&case.reply).and_then(|body| {
                    let delivery: Delivery = serde_json::from_str(body)?;
                    ensure!(
                        !delivery.summary.trim().is_empty() && !delivery.edits.is_empty(),
                        "交付对象为空"
                    );
                    Ok(())
                }),
                _ => bail!("回放的角色无效"),
            };
            ensure!(
                outcome.is_ok() == case.accepted,
                "第 {} 个原生 {} 轮次结果不符：{:?}",
                i + 1,
                case.role,
                outcome.err()
            );
            eprintln!(
                "native reply {} role={} accepted={}",
                i + 1,
                case.role,
                case.accepted
            );
        }
        Ok(())
    }

    #[test]
    fn managed_edits_apply_scoped_changes_in_project_subdirectory() -> Result<()> {
        let repo = Repo::new()?;
        // Checkout filters and newline conversion belong to the user's Git workflow. A native
        // tool's filesystem write must save its own bytes without invoking either one.
        fs::write(
            repo.0.join(".gitattributes"),
            "*.md text eol=crlf filter=must-not-run\n",
        )?;
        for (key, value) in [
            ("filter.must-not-run.smudge", "false"),
            ("filter.must-not-run.required", "true"),
        ] {
            ensure!(
                hidden_command("git")
                    .arg("-C")
                    .arg(&repo.0)
                    .args(["config", key, value])
                    .status()?
                    .success(),
                "git config"
            );
        }
        let root = repo.0.join("project/src");
        fs::write(root.join("old.txt"), "first\r\nlast\r\n")?;
        fs::write(root.join("obsolete"), "old")?;
        let edits = json!([
            {"operation":"create","path":"src/子目录/space name.md","text":"## note\n"},
            {"operation":"replace","path":"src/old.txt","before":"first\nlast","after":"updated\nlast"},
            {"operation":"delete","path":"src/obsolete","before":"old"},
            {"operation":"create","path":"src/empty","text":""},
        ]);
        assert_eq!(repo.run(edits)?, "交付");
        assert_eq!(fs::read(root.join("子目录/space name.md"))?, b"## note\n");
        assert_eq!(fs::read(root.join("old.txt"))?, b"updated\r\nlast\r\n");
        assert!(!root.join("obsolete").exists());
        assert!(root.join("empty").is_file());
        Ok(())
    }

    #[test]
    fn managed_edits_reject_invalid_batch_before_any_write() -> Result<()> {
        let repo = Repo::new()?;
        let root = repo.0.join("project/src");
        fs::write(root.join("exists"), "duplicate duplicate")?;
        for bad in [
            json!({"operation":"replace","path":"src/exists","before":"duplicate","after":"x"}),
            json!({"operation":"delete","path":"src/exists","before":"wrong"}),
            json!({"operation":"create","path":"src/exists","text":"x"}),
            json!({"operation":"create","path":"elsewhere","text":"x"}),
            json!({"operation":"create","path":"src/../outside","text":"x"}),
            json!({"operation":"create","path":"src/./outside","text":"x"}),
            json!({"operation":"create","path":"src/.GIT/config","text":"x"}),
            json!({"operation":"create","path":"src/aux.txt","text":"x"}),
            json!({"operation":"create","path":"src/name:stream","text":"x"}),
            json!({"operation":"create","path":"src/name.","text":"x"}),
            json!({"operation":"create","path":"src/link~1","text":"x"}),
            json!({"operation":"create","path":"src/valid","text":"x","force":true}),
        ] {
            let batch = json!([{"operation":"create","path":"src/early","text":"unpublished"},bad]);
            assert!(repo.run(batch).is_err());
            assert!(!root.join("early").exists());
            assert_eq!(fs::read(root.join("exists"))?, b"duplicate duplicate");
        }
        Ok(())
    }

    #[test]
    fn managed_edits_refuse_directory_links_and_do_not_mutate_hardlink_peers() -> Result<()> {
        let repo = Repo::new()?;
        let root = repo.0.join("project/src");
        let external = repo.0.join("other");
        fs::create_dir(&external)?;
        fs::write(external.join("original"), "one")?;
        fs::hard_link(external.join("original"), root.join("alias"))?;
        repo.run(json!([{"operation":"replace","path":"src/alias","before":"one","after":"two"}]))?;
        assert_eq!(fs::read(root.join("alias"))?, b"two");
        assert_eq!(fs::read(external.join("original"))?, b"one");
        #[cfg(windows)]
        junction::create(&external, root.join("link"))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, root.join("link"))?;
        assert!(repo
            .run(json!([{"operation":"create","path":"src/link/escape","text":"x"}]))
            .is_err());
        assert!(!external.join("escape").exists());
        #[cfg(windows)]
        junction::delete(root.join("link"))?;
        Ok(())
    }

    #[test]
    fn managed_edits_native_tool_write_respects_scope_and_payload_bytes() -> Result<()> {
        let repo = Repo::new()?;
        let cwd = repo.0.join("project");
        let scopes = vec!["src".into()];
        write_text(&cwd, &scopes, "src/nested/one", "1\n2\r\n")?;
        assert_eq!(fs::read(cwd.join("src/nested/one"))?, b"1\n2\r\n");
        write_text(&cwd, &scopes, "src/nested/one", "last")?;
        assert_eq!(fs::read(cwd.join("src/nested/one"))?, b"last");
        assert!(write_text(&cwd, &scopes, "else", "outside").is_err());
        assert!(write_text(&cwd, &scopes, "src/.git/config", "meta").is_err());
        let cancelled = AtomicBool::new(true);
        let reply = json!({"summary":"stop","edits":[{"operation":"create","path":"src/late","text":"late"}]});
        assert!(apply_reply(
            &repo.0,
            Path::new("project"),
            &scopes,
            &reply.to_string(),
            &cancelled
        )
        .is_err());
        assert!(!cwd.join("src/late").exists());
        Ok(())
    }
}
