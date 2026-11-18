use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn chat_install_root() -> anyhow::Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("runtime-installations/chat-dependencies"))
}

pub(crate) fn active_component(root: &Path, entry: &Path) -> Option<PathBuf> {
    let pointer = fs::read_to_string(root.join("current")).ok()?;
    let id = uuid::Uuid::parse_str(&pointer).ok()?;
    let versions = root.join("versions");
    let executable = versions.join(id.to_string()).join(entry);
    (executable.is_file()
        && executable
            .canonicalize()
            .ok()?
            .starts_with(versions.canonicalize().ok()?))
    .then_some(executable)
}

pub(crate) fn managed_node() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    active_component(
        &chat_install_root().ok()?.join("nodejs"),
        Path::new("node.exe"),
    )
}

pub(crate) fn node_executable() -> anyhow::Result<PathBuf> {
    managed_node()
        .map(Ok)
        .unwrap_or_else(|| which::which("node").map_err(Into::into))
}

pub(crate) fn with_managed_node(command: &mut Command) {
    if let Some(node) = managed_node() {
        prepend_path(command, &node);
    }
}

pub(crate) fn prepend_path(command: &mut Command, executable: &Path) {
    let Some(directory) = executable.parent() else {
        return;
    };
    let inherited = std::env::var_os("PATH");
    let paths = std::iter::once(directory.to_path_buf()).chain(
        inherited
            .as_deref()
            .into_iter()
            .flat_map(std::env::split_paths),
    );
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
}

/// Only development builds may refer to the build machine's checkout. Release
/// discovery is based on the installed executable and the user's runtime PATH.
pub(crate) fn node_modules_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(debug_assertions)]
    if let Some(project) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        roots.push(project.join("node_modules"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(directory) = exe.parent() {
            roots.push(directory.join("node_modules"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        roots.extend(node_modules_from_path(std::env::split_paths(&path)));
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join("npm/node_modules"));
    }
    let mut seen = std::collections::HashSet::new();
    roots.retain(|root| seen.insert(root.clone()));
    roots
}

fn node_modules_from_path(directories: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for directory in directories {
        if directory.file_name().is_some_and(|name| name == ".bin") {
            if let Some(parent) = directory.parent() {
                roots.push(parent.to_owned());
            }
        }
        roots.push(directory.join("node_modules"));
        if let Some(prefix) = directory.parent() {
            roots.push(prefix.join("lib/node_modules"));
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_paths_cover_npm_exec_global_and_node_installations() {
        let roots = node_modules_from_path([
            PathBuf::from("/cache/npm/node_modules/.bin"),
            PathBuf::from("/opt/node/bin"),
            PathBuf::from("/portable/node"),
        ]);
        assert!(roots.contains(&PathBuf::from("/cache/npm/node_modules")));
        assert!(roots.contains(&PathBuf::from("/opt/node/lib/node_modules")));
        assert!(roots.contains(&PathBuf::from("/portable/node/node_modules")));
    }
}
