//! Spawn-owned POSIX groups. The leader is observed with WNOWAIT and is not
//! reaped until the group has no executable members, preventing PGID reuse.
use super::UnverifiedStop;
use super::process_group_state::{validate_leader_group, verify_exited_group_quiescent, LeaderGroupState};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::process::{Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex,
};
use std::time::{Duration, Instant};

pub(super) struct GroupIdentity {
    pub(super) pid: u32,
    started: Option<u128>,
    // Only PTY/session leaders own a session. ACP groups inherit our session
    // and must never claim or inspect unrelated peers as owned descendants.
    owned_session: bool,
    pub(super) verified: AtomicBool,
    owner_dropped: AtomicBool,
    cleanup_failed: AtomicBool,
    // Identity validation, every signal, and the irreversible verified state
    // share this gate. Once verified is published, the owned Child may reap
    // the leader: no waiter can subsequently signal the released PGID.
    cleanup: Mutex<()>,
}
static GROUPS: LazyLock<Mutex<HashMap<u32, Arc<GroupIdentity>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn peek_process_exit(pid: u32) -> std::io::Result<Option<ExitStatus>> {
    use std::os::unix::process::ExitStatusExt;
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { info.si_pid() } == 0 {
        return Ok(None);
    }
    let status = unsafe { info.si_status() };
    let raw = match info.si_code {
        libc::CLD_EXITED => status << 8,
        libc::CLD_KILLED => status,
        libc::CLD_DUMPED => status | 0x80,
        _ => return Err(std::io::Error::other("unexpected waitid exit event")),
    };
    Ok(Some(ExitStatus::from_raw(raw)))
}

#[cfg(target_os = "linux")]
fn started_at(pid: u32) -> Option<u128> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm can contain spaces and parentheses. Fields after its final ')' start
    // at state (field 3), so starttime (22) is index 19 in this remainder.
    raw.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}
#[cfg(target_os = "macos")]
fn started_at(pid: u32) -> Option<u128> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            std::mem::size_of_val(&info) as i32,
        )
    };
    (count == std::mem::size_of_val(&info) as i32)
        .then_some(u128::from(info.pbi_start_tvsec) * 1_000_000 + u128::from(info.pbi_start_tvusec))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn started_at(_pid: u32) -> Option<u128> {
    None
}

fn observed_group(pid: u32) -> std::io::Result<Option<u32>> {
    let group = unsafe { libc::getpgid(pid as libc::pid_t) };
    if group >= 0 { return Ok(Some(group as u32)); }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) { Ok(None) } else { Err(error) }
}

pub(super) fn register(pid: u32) -> Result<Arc<GroupIdentity>> {
    if pid == 0 || pid == std::process::id() {
        return Err(anyhow!(UnverifiedStop(
            "拒绝登记应用自身或无效进程组".into()
        )));
    }
    // WNOWAIT proves ownership and reserves the PID even when Darwin no longer
    // exposes a completed child through getpgid. Never substitute a reaped PID.
    let observed = observed_group(pid)?;
    let exited = peek_process_exit(pid).with_context(|| format!("无法持有 PID {pid} 的未回收子进程身份"))?.is_some();
    let group_state = validate_leader_group(pid, observed, exited, cfg!(target_os = "macos"))
        .map_err(|error| anyhow!(UnverifiedStop(format!("PID {pid} 不是独立 spawn 进程组首领：{error}"))))?;
    let identity = Arc::new(GroupIdentity {
        pid,
        started: started_at(pid),
        // With a completed Darwin child getsid may also return ESRCH; inspect
        // the possible owned session conservatively rather than assuming none.
        owned_session: group_state == LeaderGroupState::ExitedWithoutLookup
            || unsafe { libc::getsid(pid as libc::pid_t) } == pid as libc::pid_t,
        verified: AtomicBool::new(false),
        owner_dropped: AtomicBool::new(false),
        cleanup_failed: AtomicBool::new(false),
        cleanup: Mutex::new(()),
    });
    let mut groups = GROUPS
        .lock()
        .map_err(|_| anyhow!(UnverifiedStop("进程组身份锁异常".into())))?;
    if groups
        .get(&pid)
        .is_some_and(|current| !current.verified.load(Ordering::Acquire))
    {
        return Err(anyhow!(UnverifiedStop(format!(
            "进程组 {pid} 已登记，拒绝覆盖未清理身份"
        ))));
    }
    groups.insert(pid, Arc::clone(&identity));
    Ok(identity)
}

fn active_members(pgid: u32) -> Result<bool> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "pgid=", "-o", "stat="])
        .output()
        .context("读取POSIX进程组状态失败")?;
    if !output.status.success() {
        return Err(anyhow!(
            "读取POSIX进程组状态失败：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 3
            || fields[0].parse::<u32>().is_err()
            || fields[1].parse::<u32>().is_err()
        {
            return Err(anyhow!("POSIX进程组快照无法核实"));
        }
        if fields[1].parse::<u32>()? == pgid
            && !fields[2].starts_with('Z')
            && !fields[2].starts_with('X')
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn active_other_session_groups(session: u32) -> Result<bool> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "pgid=", "-o", "stat="])
        .output().context("读取PTY会话成员失败")?;
    if !output.status.success() { return Err(anyhow!("读取PTY会话成员失败")); }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 { return Err(anyhow!("PTY会话快照无法核实")); }
        let pid: u32 = fields[0].parse()?;
        let pgid: u32 = fields[1].parse()?;
        if pgid != session && !fields[2].starts_with('Z') && !fields[2].starts_with('X') {
            let sid = unsafe { libc::getsid(pid as libc::pid_t) };
            if sid == session as libc::pid_t { return Ok(true); }
            if sid < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                return Err(anyhow!("无法核实PTY成员 {pid} 的会话身份"));
            }
        }
    }
    Ok(false)
}

pub(super) fn stop_registered(pid: u32) -> Result<()> {
    let identity = GROUPS
        .lock()
        .map_err(|_| anyhow!(UnverifiedStop("进程组身份锁异常".into())))?
        .get(&pid)
        .cloned()
        .ok_or_else(|| {
            anyhow!(UnverifiedStop(format!(
                "PID {pid} 未登记 spawn 进程组，拒绝裸 PID 停止"
            )))
        })?;
    stop(&identity)
}

pub(super) fn stop(identity: &Arc<GroupIdentity>) -> Result<()> {
    stop_serialized(identity, || stop_locked(identity))
}

fn stop_serialized(
    identity: &Arc<GroupIdentity>,
    operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let _cleanup = identity
        .cleanup
        .lock()
        .map_err(|_| anyhow!(UnverifiedStop("进程组清理锁异常，保留身份".into())))?;
    if identity.verified.load(Ordering::Acquire) {
        return Ok(());
    }
    operation()
}

// Caller must hold identity.cleanup; never call this after verified becomes true.
fn stop_locked(identity: &Arc<GroupIdentity>) -> Result<()> {
    let result = (|| -> Result<()> {
        // A still-waitable leader (even a zombie) reserves the original PID/PGID.
        // Reaping is exclusively done by the owned Child AFTER this succeeds.
        let exited = peek_process_exit(identity.pid).context("进程组首领已被提前回收，不能排除PGID复用")?.is_some();
        if identity
            .started
            .zip(started_at(identity.pid))
            .is_some_and(|(expected, current)| expected != current)
        {
            return Err(anyhow!("进程组首领创建时间不匹配，拒绝复用的PGID"));
        }
        let observed = observed_group(identity.pid)?;
        // The leader can exit between the first WNOWAIT observation and getpgid.
        // Recheck ownership/exit after ESRCH rather than rejecting that race.
        let exited = if observed.is_none() { peek_process_exit(identity.pid)?.is_some() } else { exited };
        let group_state = validate_leader_group(identity.pid, observed, exited, cfg!(target_os = "macos"))
            .map_err(|error| anyhow!(error))?;
        if group_state == LeaderGroupState::ExitedWithoutLookup {
            // Do not send any signal based on a missing group lookup. An own,
            // unreaped exited root can finish only if its groups are quiescent.
            verify_exited_group_quiescent(active_members(identity.pid)?,
                identity.owned_session && active_other_session_groups(identity.pid)?)
                .map_err(|error| anyhow!(error))?;
        } else {
            let signal = |signal| -> Result<()> {
                if unsafe { libc::kill(-(identity.pid as libc::pid_t), signal) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.into());
                    }
                }
                Ok(())
            };
            signal(libc::SIGTERM)?;
            let started = Instant::now();
            loop {
                // kill(...,0) includes zombies, which cannot execute or spawn. Keep
                // the leader unreaped while verifying all executable group members.
                if !active_members(identity.pid)? {
                    break;
                }
                if started.elapsed() >= Duration::from_millis(150) {
                    signal(libc::SIGKILL)?;
                }
                if started.elapsed() >= Duration::from_secs(5) {
                    return Err(anyhow!("POSIX进程组仍有活动成员"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // Never signal dynamic PGIDs: their leaders may have exited and their
        // identifiers may be reused between enumeration and kill. Keep the
        // original identity unreaped and report an unverified stop instead.
        if identity.owned_session && active_other_session_groups(identity.pid)? {
            return Err(anyhow!("PTY会话仍有其他进程组的活动成员，停止未验证；保留身份待重试"));
        }
        identity.verified.store(true, Ordering::Release);
        if identity.owner_dropped.load(Ordering::Acquire) {
            retire(identity)?;
        }
        Ok(())
    })();
    if result.is_err() {
        identity.cleanup_failed.store(true, Ordering::Release);
    }
    result.map_err(|error| {
        anyhow!(UnverifiedStop(format!(
            "进程组 {}：{error:#}",
            identity.pid
        )))
    })
}

fn retire(identity: &Arc<GroupIdentity>) -> Result<()> {
    let mut groups = GROUPS.lock().map_err(|_| anyhow!("进程组身份锁异常"))?;
    if groups
        .get(&identity.pid)
        .is_some_and(|current| Arc::ptr_eq(current, identity))
    {
        // Do not reap a newly reused PID. Explicit Child.wait normally already
        // reaped the leader; this handles exceptional owner Drop only when its
        // exact creation fingerprint still matches.
        if identity.started.is_some() && identity.started == started_at(identity.pid) {
            let mut status = 0;
            unsafe {
                libc::waitpid(identity.pid as libc::pid_t, &mut status, libc::WNOHANG);
            }
        }
        groups.remove(&identity.pid);
    }
    Ok(())
}

pub(super) fn owner_dropped(identity: &Arc<GroupIdentity>) -> Result<()> {
    let _cleanup = identity
        .cleanup
        .lock()
        .map_err(|_| anyhow!(UnverifiedStop("进程组清理锁异常，保留身份".into())))?;
    identity.owner_dropped.store(true, Ordering::Release);
    if identity.verified.load(Ordering::Acquire) {
        retire(identity)
    } else if identity.cleanup_failed.load(Ordering::Acquire) {
        Err(anyhow!(UnverifiedStop(format!(
            "进程组 {} 此前停止失败，保留身份待显式重试",
            identity.pid
        ))))
    } else {
        stop_locked(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc;

    #[test]
    fn session_stop_retains_identity_when_job_control_child_survives() {
        let root = std::env::temp_dir().join(format!("pty-session-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ready = root.join("child.pid");
        let mut command = Command::new("/bin/bash");
        command.args(["-c", "trap '' HUP; set -m; sleep 120 & echo $! > \"$1\"; wait", "fixture"]).arg(&ready);
        unsafe { command.pre_exec(|| { if libc::setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) } }); }
        let mut child = command.spawn().unwrap();
        let identity = register(child.id()).unwrap();
        assert!(identity.owned_session);
        let deadline = Instant::now() + Duration::from_secs(5);
        let task: u32 = loop {
            if let Ok(text) = std::fs::read_to_string(&ready) { if let Ok(pid) = text.trim().parse() { break pid; } }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_ne!(unsafe { libc::getpgid(task as i32) }, child.id() as i32);
        let result = stop(&identity);
        assert!(result.as_ref().err().is_some_and(|error| error.downcast_ref::<UnverifiedStop>().is_some()), "{result:?}");
        assert!(!identity.verified.load(Ordering::Acquire));
        assert!(peek_process_exit(child.id()).unwrap().is_some(), "leader identity must remain unreaped");
        assert_eq!(unsafe { libc::getsid(task as i32) }, child.id() as i32);
        unsafe { libc::kill(task as i32, libc::SIGKILL); }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if stop(&identity).is_ok() { break; }
            assert!(Instant::now() < deadline, "could not verify stopped fixture");
            std::thread::sleep(Duration::from_millis(20));
        }
        child.wait().unwrap();
        owner_dropped(&identity).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_stop_cannot_signal_after_verified_leader_is_reaped() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 30"]).process_group(0);
        let mut child = command.spawn().unwrap();
        let identity = register(child.id()).unwrap();
        assert!(!identity.owned_session, "ACP-style group inherits the parent session");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (begin_tx, begin_rx) = mpsc::channel();
        let (verified_tx, verified_rx) = mpsc::channel();
        let (reaped_tx, reaped_rx) = mpsc::channel();
        let first = {
            let identity = Arc::clone(&identity);
            std::thread::spawn(move || {
                stop_serialized(&identity, || {
                    entered_tx.send(()).unwrap();
                    begin_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    stop_locked(&identity)?;
                    verified_tx.send(()).unwrap();
                    reaped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                })
            })
        };
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // This caller has reached the same per-identity gate before the first
        // caller can publish verified or allow the owned leader to be reaped.
        let (waiting_tx, waiting_rx) = mpsc::channel();
        let second = {
            let identity = Arc::clone(&identity);
            std::thread::spawn(move || {
                assert!(identity.cleanup.try_lock().is_err());
                waiting_tx.send(()).unwrap();
                stop_serialized(&identity, || {
                    panic!("stale stop would signal a reusable PGID")
                })
            })
        };
        waiting_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        begin_tx.send(()).unwrap();
        verified_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(identity.verified.load(Ordering::Acquire));
        child.wait().unwrap();
        assert_eq!(
            peek_process_exit(identity.pid).unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
        reaped_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        owner_dropped(&identity).unwrap();
    }
    #[test]
    fn checked_stop_accepts_owned_unreaped_completed_command() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]).process_group(0);
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while peek_process_exit(child.id()).unwrap().is_none() {
            assert!(Instant::now() < deadline, "fixture did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        // Registration itself must also handle the fast-exit race on Darwin.
        let identity = register(child.id()).unwrap();
        stop(&identity).unwrap();
        assert!(identity.verified.load(Ordering::Acquire));
        assert!(child.wait().unwrap().success());
        assert_eq!(peek_process_exit(child.id()).unwrap_err().raw_os_error(), Some(libc::ECHILD));
        owner_dropped(&identity).unwrap();
    }

}
