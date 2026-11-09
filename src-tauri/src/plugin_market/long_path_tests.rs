#![cfg(windows)]
use super::*;

pub(super) struct Fixture {
    pub root: PathBuf,
    pub source: PathBuf,
    pub config: PathBuf,
    pub relative: PathBuf,
    pub revision: String,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!("crewvela-longpath-{}", uuid::Uuid::new_v4()));
        let source = root.join("s");
        let relative = PathBuf::from("plugins/nvidia/skills/accelerated-computing-performance-profiler/references/gpu-kernel-memory-layout-optimization-reference.md");
        std::fs::create_dir_all(source.join(relative.parent().unwrap())).unwrap();
        std::fs::write(source.join(&relative), "long-path-payload").unwrap();
        std::fs::create_dir_all(source.join(".agents/plugins")).unwrap();
        std::fs::write(source.join(".agents/plugins/marketplace.json"), r#"{"name":"openai-curated","owner":{"name":"Test"},"plugins":[{"name":"nvidia","source":{"source":"local","path":"./plugins/nvidia"}}]}"#).unwrap();
        let source_display = source.display().to_string();
        run_git(&["-C", &source_display, "init"]).unwrap();
        run_git(&["-C", &source_display, "-c", "core.longpaths=true", "add", "."]).unwrap();
        run_git(&["-C", &source_display, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "长路径fixture"]).unwrap();
        let revision = run_git(&["-C", &source_display, "rev-parse", "HEAD"]).unwrap().trim().to_string();
        let config = root.join("simulated-profile").join("AppData/Roaming/crewvela");
        Self { root, source, config, relative, revision }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let (Ok(root), Ok(temp)) = (self.root.canonicalize(), std::env::temp_dir().canonicalize()) {
            assert!(root.starts_with(temp));
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn windows_long_path_old_checkout_fails_new_clone_and_rust_io_succeed() {
    let f = Fixture::new();
    let target = f.config.join("plugins/marketplaces/official-01234567-89ab-cdef-0123-456789abcdef");
    // Match the reported case: the repository root is short enough for Git,
    // while checked-out plugin filenames exceed MAX_PATH.
    assert!(target.as_os_str().len() < 230);
    assert!(target.join(&f.relative).as_os_str().len() > 300);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let old = Command::new("git").args(["-c", "core.longpaths=false", "clone", "--depth", "1", &f.source.display().to_string(), &target.display().to_string()]).output().unwrap();
    assert!(!old.status.success(), "传统 checkout 应重现 MAX_PATH 错误");
    assert!(String::from_utf8_lossy(&old.stderr).contains("Filename too long"), "{}", String::from_utf8_lossy(&old.stderr));
    println!("旧 checkout：Filename too long；目标文件 {} 字符", target.join(&f.relative).as_os_str().len());
    clone_repo(&f.source.display().to_string(), Some(&f.revision), &target).unwrap();
    assert_eq!(std::fs::read_to_string(target.join(&f.relative)).unwrap(), "long-path-payload");
    assert_eq!(run_git(&["-C", &target.display().to_string(), "rev-parse", "HEAD"]).unwrap().trim(), f.revision);
    assert_eq!(run_git(&["-C", &target.display().to_string(), "config", "--local", "--get", "core.longpaths"]).unwrap().trim(), "true");
    let copy = f.config.join("plugins/cache/copied-long-package");
    copy_dir_recursive(&target.join("plugins/nvidia"), &copy).unwrap();
    assert_eq!(codex_native::content_hash(&copy).unwrap(), codex_native::content_hash(&target.join("plugins/nvidia")).unwrap());
}
