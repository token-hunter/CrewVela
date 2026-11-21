use super::*;

struct Repo {
    temp: PathBuf,
    root: PathBuf,
    state: PathBuf,
    tree: SessionWorktree,
    id: String,
}

impl Repo {
    fn new() -> Self {
        let temp =
            std::env::temp_dir().join(format!("crewvela-worktree-lifecycle-{}", Uuid::new_v4()));
        let root = temp.join("source");
        fs::create_dir_all(&root).unwrap();
        git_text(&root, &["init", "-b", "main"]).unwrap();
        git_text(&root, &["config", "user.email", "fixture@invalid.test"]).unwrap();
        git_text(&root, &["config", "user.name", "Fixture"]).unwrap();
        git_text(&root, &["config", "core.autocrlf", "false"]).unwrap();
        fs::write(root.join("tracked.bin"), b"\0\x01\r\noriginal").unwrap();
        fs::write(root.join(".gitignore"), "secret.env\n").unwrap();
        git_text(&root, &["add", "--all"]).unwrap();
        git_text(&root, &["commit", "-m", "fixture"]).unwrap();
        let id = Uuid::new_v4().to_string();
        let tree =
            crate::worktree::create_session_worktree_in(&root, &id, &temp.join("managed")).unwrap();
        Self {
            state: temp.join("state"),
            temp,
            root,
            tree,
            id,
        }
    }

    fn path(&self) -> &Path {
        Path::new(&self.tree.path)
    }
    fn project(&self) -> PathBuf {
        project_dir(&self.state, &self.root).unwrap()
    }
    fn snapshot(&self) -> Result<RecoverySnapshot> {
        snapshot_in(&self.state, &self.tree, &self.id)
    }
    fn remove_checkout(&self) {
        git_text(&self.root, &["worktree", "remove", "--", &self.tree.path]).unwrap();
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        // Each test owns this freshly generated OS-temp directory, including its local Git data.
        if self.temp.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = fs::remove_dir_all(&self.temp);
        }
    }
}

#[test]
fn real_checkout_round_trip_preserves_files_empty_directories_history_and_binding() {
    let repo = Repo::new();
    fs::create_dir_all(repo.path().join("empty").join("nested")).unwrap();
    fs::write(repo.path().join("tracked.bin"), b"\0\x01\r\ncommitted").unwrap();
    git_text(repo.path(), &["add", "--", "tracked.bin"]).unwrap();
    git_text(repo.path(), &["commit", "-m", "independent work"]).unwrap();
    // A fixed sub-second time catches restoring only whole tar seconds, or just the bytes.
    let file = repo.path().join("tracked.bin");
    let timestamp = std::time::UNIX_EPOCH + Duration::new(1_690_000_000, 123_456_700);
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(timestamp)
        .unwrap();
    let original_time = fs::metadata(&file).unwrap().modified().unwrap();
    let before = git_text(repo.path(), &["reflog", "--format=%H", "HEAD"]).unwrap();
    let snapshot = repo.snapshot().unwrap();
    verify_snapshot_in(&repo.state, &snapshot).unwrap();
    assert_eq!(
        snapshot.commit,
        git_text(repo.path(), &["rev-parse", "HEAD"]).unwrap()
    );
    repo.remove_checkout();
    // Even if the retained ordinary branch is manually removed, the snapshot reference survives.
    git_text(&repo.root, &["branch", "-D", &repo.tree.branch]).unwrap();
    restore_in(&repo.state, &snapshot).unwrap();
    crate::worktree::bind_restored_checkout(&repo.tree, &repo.id).unwrap();
    assert!(repo.path().join("empty").join("nested").is_dir());
    assert_eq!(
        fs::read(repo.path().join("tracked.bin")).unwrap(),
        b"\0\x01\r\ncommitted"
    );
    assert_eq!(
        fs::metadata(file).unwrap().modified().unwrap(),
        original_time
    );
    assert_eq!(
        git_text(repo.path(), &["reflog", "--format=%H", "HEAD"]).unwrap(),
        before
    );
    assert_eq!(
        git_text(repo.path(), &["status", "--porcelain"]).unwrap(),
        ""
    );
    verify_managed_worktree(&repo.tree, &repo.id).unwrap();
}

#[test]
fn cleanup_refuses_ignored_dirty_locked_and_index_hidden_changes() {
    let repo = Repo::new();
    assert!(
        !trusted_policy(&repo.project(), &repo.root)
            .unwrap()
            .auto_cleanup
    );
    fs::write(repo.path().join("secret.env"), "never discard").unwrap();
    assert!(repo.snapshot().is_err());
    fs::remove_file(repo.path().join("secret.env")).unwrap();
    fs::write(repo.path().join("new.txt"), "untracked").unwrap();
    assert!(repo.snapshot().is_err());
    fs::remove_file(repo.path().join("new.txt")).unwrap();
    git_text(
        repo.path(),
        &["update-index", "--assume-unchanged", "--", "tracked.bin"],
    )
    .unwrap();
    fs::write(repo.path().join("tracked.bin"), b"hidden edit").unwrap();
    assert_eq!(
        git_text(repo.path(), &["status", "--porcelain"]).unwrap(),
        ""
    );
    assert!(repo.snapshot().is_err());
    git_text(
        repo.path(),
        &["update-index", "--no-assume-unchanged", "--", "tracked.bin"],
    )
    .unwrap();
    assert!(repo.snapshot().is_err());
    git_text(repo.path(), &["restore", "--", "tracked.bin"]).unwrap();
    git_text(
        &repo.root,
        &["worktree", "lock", "--reason", "fixture", &repo.tree.path],
    )
    .unwrap();
    assert!(repo.snapshot().is_err());
    assert!(repo.path().is_dir());
}

#[test]
fn external_file_metadata_and_unconfirmed_preparation_protect_the_checkout() {
    let repo = Repo::new();
    let link = repo.temp.join("external-hard-link");
    fs::hard_link(repo.path().join("tracked.bin"), &link).unwrap();
    assert!(repo.snapshot().is_err());
    assert!(repo.path().join("tracked.bin").is_file());
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    {
        let stream = repo.path().join("tracked.bin:private-metadata");
        fs::write(&stream, b"stream bytes").unwrap();
        assert!(repo.snapshot().is_err());
        assert_eq!(fs::read(&stream).unwrap(), b"stream bytes");
        fs::remove_file(stream).unwrap();
    }
    let log = PreparationLog {
        state: "unverified".into(),
        updated_at_ms: 1,
        lines: vec!["fixture".into()],
    };
    save_json(&run_path(&repo.project(), &repo.id), &log).unwrap();
    assert!(repo.snapshot().is_err());
    assert!(prepare(
        &repo.project(),
        &repo.tree,
        &repo.id,
        &LifecyclePolicy::default()
    )
    .is_err());
}

#[test]
fn setup_commands_are_bound_to_the_original_local_repository() {
    let repo = Repo::new();
    let policy = LifecyclePolicy {
        command: "echo explicit-trust".into(),
        files: vec![],
        auto_cleanup: true,
    };
    save_json(
        &repo.project().join("policy.json"),
        &TrustedPolicy {
            repository: root_identity(&repo.root).unwrap(),
            policy: policy.clone(),
        },
    )
    .unwrap();
    assert!(
        trusted_policy(&repo.project(), &repo.root)
            .unwrap()
            .auto_cleanup
    );
    save_json(
        &repo.project().join("policy.json"),
        &TrustedPolicy {
            repository: "replaced-repository".into(),
            policy,
        },
    )
    .unwrap();
    assert!(trusted_policy(&repo.project(), &repo.root).is_err());
}

#[test]
fn changes_after_snapshot_and_reused_targets_never_get_removed_or_overwritten() {
    let repo = Repo::new();
    let snapshot = repo.snapshot().unwrap();
    fs::write(repo.path().join("secret.env"), "late").unwrap();
    assert!(verify_snapshot_in(&repo.state, &snapshot).is_err());
    fs::remove_file(repo.path().join("secret.env")).unwrap();
    repo.remove_checkout();
    fs::create_dir(repo.path()).unwrap();
    fs::write(repo.path().join("keep.txt"), "occupied").unwrap();
    assert!(restore_in(&repo.state, &snapshot).is_err());
    assert_eq!(
        fs::read_to_string(repo.path().join("keep.txt")).unwrap(),
        "occupied"
    );
    fs::remove_file(repo.path().join("keep.txt")).unwrap();
    fs::remove_dir(repo.path()).unwrap();
    git_text(&repo.root, &["commit", "--allow-empty", "-m", "new work"]).unwrap();
    let later = git_text(&repo.root, &["rev-parse", "HEAD"]).unwrap();
    git_text(
        &repo.root,
        &[
            "update-ref",
            &format!("refs/heads/{}", repo.tree.branch),
            &later,
        ],
    )
    .unwrap();
    assert!(restore_in(&repo.state, &snapshot).is_err());
    assert_eq!(
        git_text(&repo.root, &["rev-parse", &repo.tree.branch]).unwrap(),
        later
    );
    let file = snapshot_dir(&repo.project(), &snapshot.id)
        .unwrap()
        .join("checkout.tar.gz");
    fs::write(file, b"damaged").unwrap();
    assert!(load_snapshot_in(&repo.state, &repo.root, &snapshot.id).is_err());
    assert!(!repo.path().exists());
}

#[test]
fn trusted_preparation_copies_only_explicit_files_and_retries_without_overwriting() {
    let repo = Repo::new();
    let root = repo.project();
    fs::create_dir(repo.root.join("config")).unwrap();
    fs::write(repo.root.join("config/local.env"), "private").unwrap();
    let mut policy = LifecyclePolicy {
        files: vec!["config/local.env".into(), "absent".into()],
        command: "echo prepared>prepared.txt".into(),
        auto_cleanup: false,
    };
    assert!(prepare(&root, &repo.tree, &repo.id, &policy).is_err());
    assert_eq!(
        fs::read_to_string(repo.path().join("config/local.env")).unwrap(),
        "private"
    );
    assert!(!repo.path().join("prepared.txt").exists());
    assert!(ready(&root, &repo.id).is_err());
    let log: PreparationLog = read_optional(&run_path(&root, &repo.id)).unwrap().unwrap();
    assert_eq!(log.state, "failed");
    assert!(log.lines.iter().any(|line| line.contains("absent")));
    fs::write(repo.root.join("absent"), "created later").unwrap();
    prepare(&root, &repo.tree, &repo.id, &policy).unwrap();
    assert!(ready(&root, &repo.id).is_ok());
    assert_eq!(
        fs::read_to_string(repo.path().join("prepared.txt"))
            .unwrap()
            .trim(),
        "prepared"
    );
    fs::write(repo.path().join("config/local.env"), "manual edit").unwrap();
    assert!(prepare(&root, &repo.tree, &repo.id, &policy).is_err());
    assert_eq!(
        fs::read_to_string(repo.path().join("config/local.env")).unwrap(),
        "manual edit"
    );
    policy.files.clear();
    policy.command = if cfg!(windows) {
        "echo failed diagnostic & exit /b 17"
    } else {
        "echo failed diagnostic; exit 17"
    }
    .into();
    assert!(prepare(&root, &repo.tree, &repo.id, &policy).is_err());
    let log: PreparationLog = read_optional(&run_path(&root, &repo.id)).unwrap().unwrap();
    assert!(log
        .lines
        .iter()
        .any(|line| line.contains("failed diagnostic")));
    assert!(!repo.root.join("prepared.txt").exists());
}

#[test]
fn required_file_links_and_unsafe_paths_are_refused() {
    for name in [
        "../outside",
        "folder/../file",
        ".GiT/config",
        "C:/outside",
        "file:stream",
        "/outside",
        "folder//file",
        "git~1/config",
        "NUL.txt",
    ] {
        assert!(relative_file(name).is_err(), "{name}");
    }
    let repo = Repo::new();
    let outside = repo.temp.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("private"), "external").unwrap();
    #[cfg(windows)]
    {
        junction::create(&outside, repo.root.join("linked")).unwrap();
        junction::create(&outside, repo.path().join("dest")).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, repo.root.join("linked")).unwrap();
        std::os::unix::fs::symlink(&outside, repo.path().join("dest")).unwrap();
    }
    assert!(copy_required(&repo.root, repo.path(), "linked/private").is_err());
    fs::create_dir(repo.root.join("dest")).unwrap();
    fs::write(repo.root.join("dest/new"), "new").unwrap();
    assert!(copy_required(&repo.root, repo.path(), "dest/new").is_err());
    assert!(!outside.join("new").exists());
}

#[test]
fn optional_fetch_updates_only_tracking_refs_from_the_configured_local_remote() {
    let repo = Repo::new();
    assert!(fetch_worktree_base(&repo.root, "refs/heads/main").is_err());
    let remote = repo.temp.join("upstream");
    fs::create_dir(&remote).unwrap();
    git_text(&remote, &["init", "--bare", "-b", "main"]).unwrap();
    git_text(
        &repo.root,
        &["remote", "add", "up", remote.to_str().unwrap()],
    )
    .unwrap();
    git_text(&repo.root, &["push", "-u", "up", "main"]).unwrap();
    let local_head = git_text(&repo.root, &["rev-parse", "HEAD"]).unwrap();
    git_text(
        repo.path(),
        &["commit", "--allow-empty", "-m", "remote-only"],
    )
    .unwrap();
    let remote_head = git_text(repo.path(), &["rev-parse", "HEAD"]).unwrap();
    // Pushing by URL avoids advancing the source repository's remote-tracking ref.
    git_text(
        repo.path(),
        &["push", remote.to_str().unwrap(), "HEAD:refs/heads/main"],
    )
    .unwrap();
    assert_eq!(
        git_text(&repo.root, &["rev-parse", "up/main"]).unwrap(),
        local_head
    );
    fetch_worktree_base(&repo.root, "refs/remotes/up/main").unwrap();
    assert_eq!(
        git_text(&repo.root, &["rev-parse", "up/main"]).unwrap(),
        remote_head
    );
    assert_eq!(
        git_text(&repo.root, &["rev-parse", "HEAD"]).unwrap(),
        local_head
    );
}
