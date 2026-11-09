//! One desktop owns a configuration directory's sessions, hooks and mailbox.
use anyhow::{Context, Result};
use std::{fs::{File, OpenOptions}, path::Path};

pub(crate) struct AppInstanceGuard { _file: File }

impl AppInstanceGuard {
    pub(crate) fn acquire(directory: &Path) -> Result<Option<Self>> {
        std::fs::create_dir_all(directory)?;
        let path = directory.join("desktop-instance.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(None),
            Err(error) => return Err(error).context("无法锁定桌面会话数据，未启动后台任务"),
        };
        #[cfg(unix)] {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock { return Ok(None); }
                return Err(error).context("无法锁定桌面会话数据，未启动后台任务");
            }
        }
        // Keep the lock file: unlinking it could let another process lock a new inode.
        Ok(Some(Self { _file: file }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_guard_child() {
        let Some(root) = std::env::var_os("CREWVELA_GUARD_TEST_ROOT") else { return; };
        let root = std::path::PathBuf::from(root).canonicalize().unwrap();
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert_eq!(std::fs::read_to_string(root.join(".guard-fixture")).unwrap(), "test-only");
        let acquired = AppInstanceGuard::acquire(&root).unwrap().is_some();
        assert_eq!(acquired, std::env::var("CREWVELA_GUARD_EXPECT").as_deref() == Ok("free"));
    }

    #[test]
    fn direct_communication_desktop_guard_excludes_duplicates_and_releases_on_exit() {
        let root = std::env::temp_dir().join(format!("crewvela-instance-{}", uuid::Uuid::new_v4()));
        let guard = AppInstanceGuard::acquire(&root).unwrap().unwrap();
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::write(root.join(".guard-fixture"), "test-only").unwrap();
        let probe = |expected: &str| std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app_instance::tests::isolated_guard_child", "--nocapture"])
            .env("CREWVELA_GUARD_TEST_ROOT", &root).env("CREWVELA_GUARD_EXPECT", expected)
            .output().unwrap();
        let blocked = probe("locked");
        assert!(blocked.status.success(), "{}", String::from_utf8_lossy(&blocked.stderr));
        assert!(AppInstanceGuard::acquire(&root).unwrap().is_none());
        let other = AppInstanceGuard::acquire(&root.join("other-config")).unwrap().unwrap();
        drop(other); drop(guard);
        let released = probe("free");
        assert!(released.status.success(), "{}", String::from_utf8_lossy(&released.stderr));
        assert!(AppInstanceGuard::acquire(&root).unwrap().is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_communication_desktop_guard_reports_io_errors() {
        let path = std::env::temp_dir().join(format!("crewvela-instance-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "fixture").unwrap();
        assert!(AppInstanceGuard::acquire(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
