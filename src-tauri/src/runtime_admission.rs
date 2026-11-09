//! Application-scoped startup admission. Shutdown closes admission before it
//! snapshots managers, and waits until admitted starts have published ownership.
use anyhow::{bail, Result};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    closing: bool,
    starts: usize,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeAdmission(Arc<Shared>);

pub(crate) struct StartGuard(RuntimeAdmission);

pub(crate) struct ShutdownGuard {
    admission: RuntimeAdmission,
    committed: bool,
}

impl RuntimeAdmission {
    pub(crate) fn admit(&self) -> Result<StartGuard> {
        let mut state = self.0.state.lock().unwrap();
        if state.closing {
            bail!("应用正在退出，不能启动新的 Agent 或终端；若退出失败，请稍后重试");
        }
        state.starts += 1;
        Ok(StartGuard(self.clone()))
    }

    /// Call before acquiring any manager/session lifecycle lock. A start may
    /// need those locks to finish publishing the process that shutdown will stop.
    pub(crate) fn begin_shutdown(&self, timeout: Duration) -> Result<ShutdownGuard> {
        let deadline = Instant::now() + timeout;
        let mut state = self.0.state.lock().unwrap();
        if state.closing {
            bail!("应用退出检查已在进行");
        }
        state.closing = true;
        while state.starts != 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.closing = false;
                self.0.changed.notify_all();
                bail!("Agent 或终端仍在启动，未退出应用；请等待启动完成后重试");
            }
            let (next, _) = self.0.changed.wait_timeout(state, remaining).unwrap();
            state = next;
        }
        Ok(ShutdownGuard {
            admission: self.clone(),
            committed: false,
        })
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        let mut state = self.0 .0.state.lock().unwrap();
        state.starts -= 1;
        self.0 .0.changed.notify_all();
    }
}

impl ShutdownGuard {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.admission.0.state.lock().unwrap().closing = false;
            self.admission.0.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn shutdown_waits_for_publication_and_rejects_late_start() {
        let admission = RuntimeAdmission::default();
        let start = admission.admit().unwrap();
        let other = admission.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let shutdown = std::thread::spawn(move || {
            let guard = other.begin_shutdown(Duration::from_secs(2)).unwrap();
            finished_tx.send(()).unwrap();
            guard.commit();
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while !admission.0.state.lock().unwrap().closing {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(
            admission.admit().is_err(),
            "queued starts must be refused before manager snapshots"
        );
        assert!(
            finished_rx.try_recv().is_err(),
            "shutdown must wait for the admitted owner to publish"
        );
        drop(start);
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        shutdown.join().unwrap();
        assert!(
            admission.admit().is_err(),
            "successful shutdown cannot reopen admission before exit"
        );
    }

    #[test]
    fn failed_shutdown_restores_admission_without_resetting_another_guard() {
        let admission = RuntimeAdmission::default();
        let shutdown = admission.begin_shutdown(Duration::from_secs(1)).unwrap();
        assert!(admission.begin_shutdown(Duration::from_secs(1)).is_err());
        assert!(admission.admit().is_err());
        drop(shutdown);
        let start = admission.admit().unwrap();
        drop(start);
        admission
            .begin_shutdown(Duration::from_secs(1))
            .unwrap()
            .commit();
        assert!(admission.admit().is_err());
    }

    #[test]
    fn timeout_preserves_pending_start_and_allows_a_later_retry() {
        let admission = RuntimeAdmission::default();
        let start = admission.admit().unwrap();
        assert!(admission.begin_shutdown(Duration::from_millis(5)).is_err());
        let another = admission.admit().unwrap();
        drop(another);
        drop(start);
        admission
            .begin_shutdown(Duration::from_secs(1))
            .unwrap()
            .commit();
        assert!(admission.admit().is_err());
    }
}
