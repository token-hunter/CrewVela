use std::path::PathBuf;

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
            if let Some(parent) = directory.parent() { roots.push(parent.to_owned()); }
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
