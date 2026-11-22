use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_FILE: usize = 128 * 1024;
const MAX_TOTAL: usize = 16 * 1024 * 1024;
const MAX_PATHS: usize = 4_000;
const MAX_DIFF: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnChanges {
    pub(crate) files: Vec<TurnFile>,
    pub(crate) checks: Vec<TurnCheck>,
    pub(crate) limited: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TurnFile {
    path: String,
    patch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnCheck {
    command: String,
    exit_code: Option<i64>,
}

pub(crate) struct WorktreeSnapshot {
    root: PathBuf,
    files: BTreeMap<String, Option<String>>,
    limited: bool,
}

pub(crate) fn is_exclusive(root: &Path, owner: &str, sessions: &crate::sessions::SessionManager) -> bool {
    let Ok(root) = root.canonicalize() else { return false; };
    !sessions.list().iter().any(|session| {
        session.id != owner && Path::new(&session.cwd).canonicalize().is_ok_and(|cwd| cwd.starts_with(&root))
    })
}

impl WorktreeSnapshot {
    pub(crate) fn capture(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let detected = crate::git_source_control::run_git_bytes(
            &root, &["rev-parse", "--show-toplevel"],
        )?;
        ensure!(Path::new(std::str::from_utf8(&detected)?.trim()).canonicalize()? == root,
            "工作树根目录已变化");
        let list = crate::git_source_control::run_git_bytes(
            &root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        )?;
        let mut files = BTreeMap::new();
        let mut total = 0;
        let mut limited = false;
        for raw in list.split(|&byte| byte == 0).filter(|part| !part.is_empty()) {
            if files.len() >= MAX_PATHS || total >= MAX_TOTAL {
                limited = true;
                break;
            }
            let Ok(name) = std::str::from_utf8(raw) else { limited = true; continue; };
            if files.contains_key(name) { continue; }
            let path = Path::new(name);
            if !path.components().all(|part| matches!(part, Component::Normal(_))) {
                limited = true;
                continue;
            }
            let contents = match read_file(&root, path, MAX_FILE.min(MAX_TOTAL - total)) {
                Ok(Some(text)) => { total += text.len(); Some(text) }
                Ok(None) => continue,
                Err(_) => { limited = true; None }
            };
            files.insert(name.to_owned(), contents);
        }
        Ok(Self { root, files, limited })
    }

    pub(crate) fn finish(self, checks: Vec<TurnCheck>) -> Result<TurnChanges> {
        let after = Self::capture(&self.root)?;
        let mut result = TurnChanges { files: Vec::new(), checks, limited: self.limited || after.limited };
        // An incomplete enumeration can make files look spuriously deleted.
        let paths = self.files.keys().chain(after.files.keys()).collect::<BTreeSet<_>>();
        let mut patch_bytes = 0;
        let scratch = std::env::temp_dir().join(format!("crewvela-turn-diff-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&scratch)?;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_file(self.0.join("before"));
                let _ = fs::remove_file(self.0.join("after"));
                let _ = fs::remove_dir(&self.0);
            }
        }
        let scratch = Scratch(scratch);
        for path in paths {
            let before = self.files.get(path);
            let next = after.files.get(path);
            if before == next || matches!(before, Some(None)) || matches!(next, Some(None))
                || (self.limited && before.is_none()) || (after.limited && next.is_none()) {
                continue;
            }
            if result.files.len() == 32 { result.limited = true; break; }
            fs::write(scratch.0.join("before"), before.and_then(Option::as_deref).unwrap_or(""))?;
            fs::write(scratch.0.join("after"), next.and_then(Option::as_deref).unwrap_or(""))?;
            let output = crate::git_source_control::run_git_allow_failure(
                &scratch.0, &["-c", "core.safecrlf=false", "diff", "--no-index", "--no-textconv",
                    "--no-ext-diff", "--no-color", "--unified=3", "--", "before", "after"],
            )?;
            ensure!(matches!(output.status.code(), Some(0 | 1)), "无法读取本轮文件差异");
            let delta = String::from_utf8(output.stdout)?;
            let patch = delta.lines().skip_while(|line| !line.starts_with("@@ "))
                .collect::<Vec<_>>().join("\n");
            if patch_bytes + patch.len() > MAX_DIFF { result.limited = true; break; }
            patch_bytes += patch.len();
            result.files.push(TurnFile { path: path.clone(), patch });
        }
        Ok(result)
    }
}

fn read_file(root: &Path, relative: &Path, cap: usize) -> Result<Option<String>> {
    let mut path = root.to_path_buf();
    for part in relative {
        path.push(part);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(!metadata.is_symlink(), "跳过链接文件");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(metadata.file_attributes() & 0x400 == 0, "跳过目录联接");
        }
    }
    let resolved = path.canonicalize()?;
    ensure!(resolved.starts_with(root), "文件位于工作树外");
    let metadata = fs::metadata(&resolved)?;
    ensure!(metadata.is_file() && metadata.len() <= cap as u64, "跳过非文本或超限文件");
    let mut bytes = Vec::new();
    fs::File::open(&resolved).with_context(|| format!("读取轮次变更文件 {}", relative.display()))?
        .take(cap as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > cap || bytes.contains(&0) { bail!("跳过非文本或超限文件"); }
    Ok(Some(String::from_utf8(bytes)?))
}

pub(crate) fn test_checks(tools: impl Iterator<Item = Value>) -> Vec<TurnCheck> {
    tools.filter_map(|tool| {
        if !matches!(tool["status"].as_str(), Some("completed" | "failed")) { return None; }
        let command = tool["rawInput"]["command"].as_str()?;
        let mut parts = command.split_whitespace();
        let program = parts.next()?;
        let test = match program {
            "npm" | "pnpm" | "yarn" | "cargo" => parts.next() == Some("test"),
            "node" => parts.next() == Some("--test"),
            "pytest" | "vitest" => true,
            _ => false,
        };
        if !test { return None; }
        Some(TurnCheck {
            command: command.chars().take(500).collect(),
            exit_code: tool["rawOutput"]["exitCode"].as_i64()
                .or_else(|| tool["rawOutput"]["exit_code"].as_i64()),
        })
    }).take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_turn_delta_excludes_preexisting_edits_and_reads_test_exit_evidence() {
        let root = std::env::temp_dir().join(format!("crewvela-turn-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        crate::git_source_control::run_git_bytes(&root, &["init", "-q"]).unwrap();
        fs::write(root.join("old.txt"), "old\n").unwrap();
        fs::write(root.join("dirty.txt"), "already dirty\n").unwrap();
        crate::git_source_control::run_git_bytes(&root, &["add", "old.txt"]).unwrap();
        let before = WorktreeSnapshot::capture(&root).unwrap();
        fs::write(root.join("old.txt"), "changed this round\n").unwrap();
        fs::write(root.join("new.txt"), "new this round\n").unwrap();
        let changes = before.finish(test_checks([
            serde_json::json!({"status":"completed","rawInput":{"command":"node --test"},"rawOutput":{"exitCode":0}}),
            serde_json::json!({"status":"completed","rawInput":{"command":"cargo test"}}),
            serde_json::json!({"status":"completed","rawInput":{"command":"git diff"}}),
        ].into_iter())).unwrap();
        assert_eq!(changes.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["new.txt", "old.txt"]);
        assert!(changes.files[1].patch.contains("+changed this round"));
        assert_eq!(changes.checks.iter().map(|check| check.exit_code).collect::<Vec<_>>(), [Some(0), None]);
        assert!(!changes.limited);
        let _ = fs::remove_dir_all(root);
    }
}
