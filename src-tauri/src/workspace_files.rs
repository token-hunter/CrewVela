use std::fs;
use std::collections::{HashSet, VecDeque};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_SEARCH_ENTRIES: usize = 50_000;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileEntry {
    pub name: String,
    pub relative_path: String,
    pub directory: bool,
    pub size: u64,
    pub modified_at: Option<u64>,
    pub extension: String,
}

fn canonical_root(root: &str) -> Result<PathBuf> {
    let root = fs::canonicalize(root).with_context(|| format!("工作区不存在: {root}"))?;
    if !root.is_dir() {
        return Err(anyhow!("工作区不是目录: {}", root.display()));
    }
    Ok(root)
}

fn safe_relative_path(relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("文件路径不能超出当前工作区"));
    }
    Ok(path.to_path_buf())
}

pub fn resolve_path(root: &str, relative_path: &str) -> Result<PathBuf> {
    let root = canonical_root(root)?;
    let relative = safe_relative_path(relative_path)?;
    let path = fs::canonicalize(root.join(relative))
        .with_context(|| format!("文件不存在: {relative_path}"))?;
    if !path.starts_with(&root) {
        return Err(anyhow!("文件路径不能超出当前工作区"));
    }
    Ok(path)
}

fn relative_string(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

pub fn list_directory(root: &str, relative_path: &str) -> Result<Vec<WorkspaceFileEntry>> {
    let root = canonical_root(root)?;
    let directory = resolve_path(root.to_string_lossy().as_ref(), relative_path)?;
    if !directory.is_dir() {
        return Err(anyhow!("目标不是目录: {relative_path}"));
    }
    let mut entries = Vec::new();
    let visible_directory = safe_relative_path(relative_path)?;
    for entry in fs::read_dir(&directory).context("读取工作区目录失败")? {
        let Ok(entry) = entry else { continue };
        let Ok(path) = fs::canonicalize(entry.path()) else { continue };
        if !path.starts_with(&root) {
            continue;
        }
        let Ok(metadata) = path.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        entries.push(WorkspaceFileEntry {
            // Keep the path the user opened: aliases must have distinct tree keys,
            // while the canonical path above still enforces the workspace boundary.
            relative_path: relative_string(Path::new(""), &visible_directory.join(entry.file_name()))?,
            extension: entry.path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_lowercase(),
            name,
            directory: metadata.is_dir(),
            size: if metadata.is_file() { metadata.len() } else { 0 },
            modified_at: metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_millis() as u64),
        });
    }
    entries.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn read_text(root: &str, relative_path: &str) -> Result<String> {
    let path = resolve_path(root, relative_path)?;
    let metadata = path.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("目标不是文件: {relative_path}"));
    }
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(anyhow!("文本文件超过 2 MB，请使用系统应用打开"));
    }
    fs::read_to_string(path).context("该文件不是可预览的 UTF-8 文本")
}

/// 覆盖保存工作区内的文本文件。resolve_path 要求目标已存在且在根内，
/// 天然拒绝新建与路径逃逸；写入走临时文件 + 原子替换，避免写一半损坏。
pub fn write_text(root: &str, relative_path: &str, contents: &str, expected_contents: &str) -> Result<u64> {
    // Serialize saves from this application so two editors cannot both validate
    // the same baseline and then overwrite each other.
    let _guard = WRITE_LOCK.lock().map_err(|_| anyhow!("文件保存锁不可用"))?;
    let path = resolve_path(root, relative_path)?;
    let metadata = path.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("目标不是文件: {relative_path}"));
    }
    let bytes = contents.len() as u64;
    if bytes > MAX_TEXT_BYTES {
        return Err(anyhow!("内容超过 2 MB 上限，请使用系统编辑器保存"));
    }
    // 内容必须是合法 UTF-8 由 &str 保证；仍拒绝写入 NUL（Windows 路径语义冲突）。
    if contents.contains('\0') {
        return Err(anyhow!("内容包含非法字符"));
    }
    let check_current = || -> Result<()> {
        let current = fs::read(&path).context("读取当前文件失败")?;
        if current != expected_contents.as_bytes() {
            return Err(anyhow!("文件已被其他程序修改，未覆盖磁盘内容；请保留草稿并重新打开文件后合并修改。"));
        }
        Ok(())
    };
    check_current()?;
    let (temp, mut file) = loop {
        let temp = path.with_extension(format!("crewvela-{}.tmp", uuid::Uuid::new_v4()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("创建临时文件失败"),
        }
    };
    // 原子替换会创建新 inode；先复制权限，避免可执行脚本失去执行位或私密文件权限被放宽。
    if let Err(error) = file.set_permissions(metadata.permissions()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error).context("保留文件权限失败");
    }
    if let Err(error) = file.write_all(contents.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error).context("写入临时文件失败");
    }
    drop(file);
    // External editors do not share WRITE_LOCK. Recheck immediately before
    // replacement to catch changes made while the temporary file was written.
    if let Err(error) = check_current() {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(error).context("保存文件失败");
    }
    Ok(bytes)
}

pub fn search(root: &str, query: &str) -> Result<Vec<WorkspaceFileEntry>> {
    let root = canonical_root(root)?;
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // ponytail: bounded scan keeps huge repos responsive; add an index only if 50k entries becomes a measured limit.
    let mut pending = VecDeque::from([root.clone()]);
    let mut visited_directories = HashSet::from([root.clone()]);
    let mut matched_paths = HashSet::new();
    let mut visited = 0;
    let mut matches = Vec::new();
    while let Some(directory) = pending.pop_front() {
        for entry in fs::read_dir(directory).context("搜索工作区失败")? {
            let Ok(entry) = entry else { continue };
            visited += 1;
            if visited > MAX_SEARCH_ENTRIES || matches.len() >= MAX_SEARCH_RESULTS {
                break;
            }
            let Ok(path) = fs::canonicalize(entry.path()) else { continue };
            if !path.starts_with(&root) {
                continue;
            }
            let Ok(metadata) = path.metadata() else { continue };
            if metadata.is_dir() && visited_directories.insert(path.clone()) {
                pending.push_back(path.clone());
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_lowercase().contains(&query) {
                continue;
            }
            if !matched_paths.insert(path.clone()) {
                continue;
            }
            matches.push(WorkspaceFileEntry {
                relative_path: relative_string(&root, &entry.path())?,
                extension: entry.path().extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase(),
                name,
                directory: metadata.is_dir(),
                size: if metadata.is_file() { metadata.len() } else { 0 },
                modified_at: metadata.modified().ok().and_then(|value| value.duration_since(UNIX_EPOCH).ok()).map(|value| value.as_millis() as u64),
            });
        }
        if visited > MAX_SEARCH_ENTRIES || matches.len() >= MAX_SEARCH_RESULTS {
            break;
        }
    }
    matches.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("crewvela-files-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("README.md"), "# Demo\n").unwrap();
        fs::write(root.join(".env.example"), "SAFE=true\n").unwrap();
        root
    }

    #[test]
    fn lists_complete_tree_and_reads_text_inside_workspace() {
        let root = workspace();
        let entries = list_directory(root.to_string_lossy().as_ref(), "").unwrap();
        assert_eq!(entries[0].name, "src");
        assert!(entries.iter().any(|entry| entry.name == ".env.example"));
        assert_eq!(
            read_text(root.to_string_lossy().as_ref(), "src/main.rs").unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            search(root.to_string_lossy().as_ref(), "main").unwrap()[0].relative_path,
            "src/main.rs"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = workspace();
        assert!(resolve_path(root.to_string_lossy().as_ref(), "../secret.txt").is_err());
        assert!(read_text(root.to_string_lossy().as_ref(), "../secret.txt").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_text_roundtrips_rejects_escape_and_missing_targets() {
        let root = workspace();
        let root_str = root.to_string_lossy().as_ref().to_string();
        fs::write(root.join("README.crewvela-tmp"), "keep this file").unwrap();
        let saved = write_text(&root_str, "README.md", "# 改过的标题\n", "# Demo\n").unwrap();
        assert!(saved > 0);
        assert_eq!(read_text(&root_str, "README.md").unwrap(), "# 改过的标题\n");
        assert_eq!(fs::read_to_string(root.join("README.crewvela-tmp")).unwrap(), "keep this file");
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            name.starts_with("README.crewvela-") && name != "README.crewvela-tmp"
        }));

        assert!(write_text(&root_str, "../secret.txt", "x", "").is_err());
        // 不存在的目标（resolve_path 要求已存在）拒绝新建。
        assert!(write_text(&root_str, "new-file.md", "x", "").is_err());
        // 目录不是可保存目标。
        assert!(write_text(&root_str, "src", "x", "").is_err());
        assert!(write_text(&root_str, "README.md", "bad\0content", "").is_err());
        let oversized = "x".repeat(2 * 1024 * 1024 + 1);
        assert!(write_text(&root_str, "README.md", &oversized, "").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_text_rejects_external_changes_and_allows_a_fresh_retry() {
        let root = workspace();
        let root_str = root.to_string_lossy();
        let original = read_text(&root_str, "README.md").unwrap();
        fs::write(root.join("README.md"), "external change").unwrap();

        let error = write_text(&root_str, "README.md", "my draft", &original).unwrap_err();
        assert!(error.to_string().contains("已被其他程序修改"));
        assert_eq!(read_text(&root_str, "README.md").unwrap(), "external change");
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().contains("crewvela-")
        }));

        write_text(&root_str, "README.md", "merged draft", "external change").unwrap();
        assert_eq!(read_text(&root_str, "README.md").unwrap(), "merged draft");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn directory_links_keep_visible_paths_and_search_does_not_repeat_targets() {
        let root = workspace();
        let link = root.join("src-alias");
        let loop_link = root.join("src").join("back-to-root");
        #[cfg(windows)]
        {
            junction::create(root.join("src"), &link).unwrap();
            junction::create(&root, &loop_link).unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("src"), &link).unwrap();
            std::os::unix::fs::symlink(&root, &loop_link).unwrap();
        }

        let root_str = root.to_string_lossy();
        // Remove links before asserting so even a failing regression leaves no cycle behind.
        let entries = list_directory(&root_str, "").unwrap();
        let nested = list_directory(&root_str, "src-alias").unwrap();
        let results = search(&root_str, "main").unwrap();
        #[cfg(windows)]
        {
            junction::delete(&loop_link).unwrap();
            junction::delete(&link).unwrap();
        }
        #[cfg(unix)]
        {
            fs::remove_file(&loop_link).unwrap();
            fs::remove_file(&link).unwrap();
        }
        fs::remove_dir_all(root).unwrap();

        assert_eq!(results.len(), 1, "directory aliases and cycles must not duplicate search results");
        assert_eq!(results[0].relative_path, "src/main.rs");
        assert_eq!(entries.iter().find(|entry| entry.name == "src-alias").unwrap().relative_path, "src-alias");
        assert_eq!(nested.iter().find(|entry| entry.name == "main.rs").unwrap().relative_path, "src-alias/main.rs");
    }

    #[cfg(unix)]
    #[test]
    fn write_text_preserves_executable_and_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = workspace();
        let path = root.join("README.md");
        for mode in [0o755, 0o600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let original = fs::read_to_string(&path).unwrap();
            write_text(root.to_string_lossy().as_ref(), "README.md", "updated", &original).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, mode);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_text_does_not_follow_existing_legacy_temp_symlink() {
        let root = workspace();
        let outside = root.with_extension("outside.txt");
        fs::write(&outside, "outside stays").unwrap();
        let legacy_temp = root.join("README.crewvela-tmp");
        std::os::unix::fs::symlink(&outside, &legacy_temp).unwrap();
        write_text(root.to_string_lossy().as_ref(), "README.md", "updated", "# Demo\n").unwrap();
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside stays");
        assert!(fs::symlink_metadata(&legacy_temp).unwrap().file_type().is_symlink());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }
}
