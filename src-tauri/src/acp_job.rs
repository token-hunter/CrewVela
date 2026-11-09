//! Per-runtime Windows Jobs. No global application Job: unrelated long-lived
//! services (notably ai-memory) deliberately keep their own lifecycle.
use anyhow::{anyhow, Context, Result};
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::*,
    System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
};

pub(crate) struct ProcessJob {
    handle: OwnedHandle,
}
impl ProcessJob {
    pub(super) fn new() -> Result<Self> {
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error()).context("创建受管进程Job失败");
        }
        let job = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(raw) },
        };
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("设置受管Job退出限制失败");
        }
        Ok(job)
    }
    pub(crate) fn as_handle(&self) -> BorrowedHandle<'_> {
        use std::os::windows::io::AsHandle;
        self.handle.as_handle()
    }
    pub(crate) fn contains(&self, process: BorrowedHandle<'_>) -> Result<bool> {
        let mut yes = 0;
        if unsafe {
            IsProcessInJob(
                process.as_raw_handle(),
                self.handle.as_raw_handle(),
                &mut yes,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("核实进程Job归属失败");
        }
        Ok(yes != 0)
    }
    pub(super) fn assign(&self, process: BorrowedHandle<'_>) -> Result<()> {
        #[cfg(test)]
        if TEST_REFUSE_ASSIGNMENT.with(|refuse| refuse.get()) {
            return Err(anyhow!("fixture refused Job assignment"));
        }
        if unsafe { AssignProcessToJobObject(self.handle.as_raw_handle(), process.as_raw_handle()) }
            == 0
        {
            return Err(std::io::Error::last_os_error()).context("受管Job分配失败；进程尚未执行");
        }
        if !self.contains(process)? {
            return Err(anyhow!("分配后无法核实进程Job归属"));
        }
        Ok(())
    }
    pub(crate) fn active_processes(&self) -> Result<u32> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("核实Job活动进程失败");
        }
        Ok(info.ActiveProcesses)
    }
    pub(crate) fn stop(&self) -> Result<()> {
        if unsafe { TerminateJobObject(self.handle.as_raw_handle(), 1) } == 0 {
            return Err(std::io::Error::last_os_error()).context("终止受管Job失败");
        }
        let started = Instant::now();
        while self.active_processes()? != 0 {
            if started.elapsed() >= Duration::from_secs(5) {
                return Err(anyhow!("受管Job仍有活动进程，保留Job身份"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}

/// Stable std lacks spawn_with_attributes/main_thread_handle. The root starts
/// suspended, joins its Job, and only then resumes its verified initial thread.
/// Parent crash between CreateProcess and Assign can leave a suspended resource;
/// no unassigned user code is allowed to execute.
pub(super) fn spawn_suspended(command: &mut Command) -> Result<Child> {
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    command.spawn().context("创建暂停的受管进程失败")
}

pub(super) fn resume_initial_thread(root: BorrowedHandle<'_>) -> Result<()> {
    let pid = unsafe { GetProcessId(root.as_raw_handle()) };
    if pid == 0 {
        return Err(std::io::Error::last_os_error()).context("读取原始进程身份失败");
    }
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error()).context("读取暂停进程线程失败");
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut next = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    let mut threads = Vec::new();
    while next != 0 {
        if entry.th32OwnerProcessID == pid {
            let raw = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                )
            };
            if raw.is_null() {
                return Err(std::io::Error::last_os_error()).context("打开暂停的初始线程失败");
            }
            let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
            if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
                return Err(anyhow!("初始线程归属已变化，拒绝恢复"));
            }
            threads.push(thread);
        }
        next = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(std::io::Error::last_os_error()).context("枚举初始线程失败");
    }
    if threads.len() != 1 {
        return Err(anyhow!("无法唯一核实暂停的初始线程，未恢复执行"));
    }
    if unsafe { ResumeThread(threads[0].as_raw_handle()) } != 1 {
        return Err(anyhow!("初始线程暂停状态异常，未放行未确认的执行"));
    }
    Ok(())
}

/// Only for a root that has NEVER been resumed. It cannot have user descendants.
pub(super) fn cleanup_unresumed(root: &mut Child) -> Result<()> {
    if unsafe { TerminateProcess(root.as_raw_handle(), 1) } == 0
        && unsafe { WaitForSingleObject(root.as_raw_handle(), 0) } != WAIT_OBJECT_0
    {
        return Err(std::io::Error::last_os_error()).context("暂停进程清理失败");
    }
    if unsafe { WaitForSingleObject(root.as_raw_handle(), 5000) } != WAIT_OBJECT_0 {
        return Err(anyhow!("暂停进程仍未退出"));
    }
    root.wait().context("回收暂停进程失败")?;
    Ok(())
}

#[cfg(test)]
thread_local! { static TEST_REFUSE_ASSIGNMENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(super) struct TestJobAssignmentFailureGuard(bool);
#[cfg(test)]
impl TestJobAssignmentFailureGuard {
    pub(super) fn new() -> Self {
        Self(TEST_REFUSE_ASSIGNMENT.with(|refuse| refuse.replace(true)))
    }
}
#[cfg(test)]
impl Drop for TestJobAssignmentFailureGuard {
    fn drop(&mut self) {
        TEST_REFUSE_ASSIGNMENT.with(|refuse| refuse.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_spawn_refuses_assignment_before_user_code_executes() {
        let root = std::env::temp_dir().join(format!("job-refusal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("executed");
        let mut command = Command::new("node");
        command.args(["-e", "require('fs').writeFileSync(process.argv[1],'ran')"]).arg(&marker);
        let refusal = TestJobAssignmentFailureGuard::new();
        assert!(super::super::spawn_owned_command(&mut command).is_err());
        drop(refusal);
        assert!(!marker.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owned_job_stops_grandchild_after_its_ancestors_exit() {
        use std::os::windows::io::AsHandle;
        let root = std::env::temp_dir().join(format!("job-orphan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let grand = root.join("grand.cjs");
        let middle = root.join("middle.cjs");
        let parent = root.join("parent.cjs");
        let marker = root.join("pid");
        std::fs::write(&grand, "require('fs').writeFileSync(process.argv[2],String(process.pid));setInterval(()=>{},1000)").unwrap();
        std::fs::write(&middle, "require('child_process').spawn(process.execPath,[process.argv[2],process.argv[3]],{stdio:'ignore',detached:true}).unref()").unwrap();
        std::fs::write(&parent, "require('child_process').spawn(process.execPath,[process.argv[2],process.argv[3],process.argv[4]],{stdio:'ignore'})").unwrap();
        let mut command = Command::new("node");
        command.arg(&parent).arg(&middle).arg(&grand).arg(&marker);
        let (mut child, identity) = super::super::spawn_owned_command(&mut command).unwrap();
        child.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid: u32 = loop {
            if let Some(pid) = std::fs::read_to_string(&marker).ok().and_then(|value| value.parse().ok()) {
                break pid;
            }
            assert!(Instant::now() < deadline, "grandchild never started");
            std::thread::sleep(Duration::from_millis(5));
        };
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(!raw.is_null());
        let grand_handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let job = identity.identity.job.as_ref().unwrap();
        assert!(job.contains(grand_handle.as_handle()).unwrap());
        assert!(job.active_processes().unwrap() > 0);
        assert_eq!(unsafe { WaitForSingleObject(grand_handle.as_raw_handle(), 0) }, WAIT_TIMEOUT);
        super::super::stop_child_checked(&mut child).unwrap();
        assert_eq!(job.active_processes().unwrap(), 0);
        assert_eq!(unsafe { WaitForSingleObject(grand_handle.as_raw_handle(), 3000) }, WAIT_OBJECT_0);
        drop(identity);
        std::fs::remove_dir_all(root).unwrap();
    }
}
