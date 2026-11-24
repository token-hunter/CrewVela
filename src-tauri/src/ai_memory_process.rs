//! Reconnect only the exact local managed service after a launcher restart.
//! Never trust server.pid, enumerate by image name, or terminate by an unheld PID.
use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::{Foundation::*, Security::*, System::Threading::*,
    NetworkManagement::IpHelper::*, Networking::WinSock::{AF_INET, AF_INET6},
    UI::Shell::CommandLineToArgvW};

pub(super) struct VerifiedService {
    handle: OwnedHandle,
    pid: u32,
    created: u64,
    executable: PathBuf,
    data: PathBuf,
    base: String,
}

fn creation(handle: HANDLE) -> Result<u64> {
    let mut times: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    if unsafe { GetProcessTimes(handle, &mut times[0], &mut times[1], &mut times[2], &mut times[3]) } == 0 {
        return Err(std::io::Error::last_os_error()).context("无法核验服务启动时间");
    }
    Ok((times[0].dwHighDateTime as u64) << 32 | times[0].dwLowDateTime as u64)
}

fn user_token(handle: HANDLE) -> Result<(OwnedHandle, Vec<usize>)> {
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut raw) } == 0 {
        bail!("无法核验记忆服务的用户身份");
    }
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut size = 0;
    unsafe { GetTokenInformation(raw, TokenUser, std::ptr::null_mut(), 0, &mut size); }
    if size == 0 || size > 65536 { bail!("记忆服务用户身份长度无效"); }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(raw, TokenUser, buffer.as_mut_ptr().cast(), size, &mut size) } == 0 {
        bail!("无法读取记忆服务用户身份");
    }
    Ok((token, buffer))
}

fn same_user(handle: HANDLE) -> Result<()> {
    let (_a, a) = user_token(handle)?;
    let (_b, b) = user_token(unsafe { GetCurrentProcess() })?;
    let a = unsafe { &*a.as_ptr().cast::<TOKEN_USER>() };
    let b = unsafe { &*b.as_ptr().cast::<TOKEN_USER>() };
    if unsafe { EqualSid(a.User.Sid, b.User.Sid) } == 0 { bail!("记忆服务不属于当前用户，未接管"); }
    Ok(())
}

fn image(handle: HANDLE) -> Result<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) } == 0 {
        bail!("无法读取记忆服务可执行文件身份");
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize])))
}

#[repr(C)]
struct UnicodeString { length: u16, maximum: u16, buffer: *const u16 }
#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(handle: HANDLE, class: i32, information: *mut std::ffi::c_void, length: u32, returned: *mut u32) -> i32;
}

fn arguments(handle: HANDLE) -> Result<Vec<PathBuf>> {
    // Class 60 is available on supported modern Windows. Failure is closed;
    // no fallback to a PID file or unchecked process termination.
    let mut size = 0;
    unsafe { NtQueryInformationProcess(handle, 60, std::ptr::null_mut(), 0, &mut size); }
    if size < std::mem::size_of::<UnicodeString>() as u32 || size > 128 * 1024 {
        bail!("当前系统无法核验服务启动参数，未接管");
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { NtQueryInformationProcess(handle, 60, buffer.as_mut_ptr().cast(), size, &mut size) } < 0 {
        bail!("读取记忆服务启动参数失败，未接管");
    }
    let value = unsafe { &*buffer.as_ptr().cast::<UnicodeString>() };
    let start = value.buffer as usize;
    let low = buffer.as_ptr() as usize;
    if value.length % 2 != 0 || start % 2 != 0 || start < low ||
        start.checked_add(value.length as usize).is_none_or(|end| end > low + buffer.len() * std::mem::size_of::<usize>()) {
        bail!("记忆服务启动参数范围无效");
    }
    let mut command = unsafe { std::slice::from_raw_parts(value.buffer, value.length as usize / 2) }.to_vec();
    command.push(0);
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
    if argv.is_null() { bail!("无法解析记忆服务启动参数"); }
    let result = (|| {
        if !(1..=64).contains(&count) { bail!("记忆服务启动参数数量无效"); }
        let mut args = Vec::new();
        for pointer in unsafe { std::slice::from_raw_parts(argv, count as usize) } {
            let mut len = 0;
            while len < command.len() && unsafe { *pointer.add(len) } != 0 { len += 1; }
            if len == command.len() { bail!("记忆服务启动参数未终止"); }
            args.push(PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(*pointer, len) })));
        }
        Ok(args)
    })();
    unsafe { LocalFree(argv.cast()); }
    result
}

fn matches_launch(args: &[PathBuf], executable: &Path, data: &Path) -> Result<()> {
    if args.len() != 9 || args[1] != Path::new("--data-dir") || args[3] != Path::new("serve") ||
        args[4] != Path::new("--transport") || args[5] != Path::new("http") ||
        args[6] != Path::new("--enable-web") || args[7] != Path::new("--config") {
        bail!("当前监听进程不是可核验的 CrewVela 托管服务，未接管");
    }
    if args[0].canonicalize()? != executable.canonicalize()? || args[2].canonicalize()? != data.canonicalize()? ||
        args[8].canonicalize()? != data.join("config.toml").canonicalize()? {
        bail!("服务的程序、数据目录或配置文件与本次更新不一致，未接管");
    }
    Ok(())
}

pub(super) fn listener_pid(base: &str) -> Result<u32> {
    let address = loopback_socket_address(base).context("仅允许核验本机回环记忆服务")?;
    let family = if address.is_ipv4() { AF_INET } else { AF_INET6 } as u32;
    let mut size = 0;
    unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0); }
    for _ in 0..3 {
        if size < 4 || size > 4 * 1024 * 1024 { bail!("系统监听表大小无效"); }
        let mut bytes = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        let status = unsafe { GetExtendedTcpTable(bytes.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        if status == ERROR_INSUFFICIENT_BUFFER { continue; }
        if status != 0 { bail!("无法核验记忆服务监听进程，系统错误 {status}"); }
        let raw = bytes.as_ptr().cast::<u8>();
        let count = unsafe { raw.cast::<u32>().read_unaligned() } as usize;
        let mut found = std::collections::BTreeSet::new();
        let row_size = if address.is_ipv4() { std::mem::size_of::<MIB_TCPROW_OWNER_PID>() } else { std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>() };
        if count > (size as usize - 4) / row_size { bail!("系统监听表截断，未接管"); }
        for i in 0..count {
            let row = unsafe { raw.add(4 + i * row_size) };
            let pid = match address {
                std::net::SocketAddr::V4(a) => {
                    let row = unsafe { row.cast::<MIB_TCPROW_OWNER_PID>().read_unaligned() };
                    (row.dwLocalAddr.to_ne_bytes() == a.ip().octets() && u16::from_be(row.dwLocalPort as u16) == a.port()).then_some(row.dwOwningPid)
                },
                std::net::SocketAddr::V6(a) => {
                    let row = unsafe { row.cast::<MIB_TCP6ROW_OWNER_PID>().read_unaligned() };
                    (row.ucLocalAddr == a.ip().octets() && row.dwLocalScopeId == a.scope_id() && u16::from_be(row.dwLocalPort as u16) == a.port()).then_some(row.dwOwningPid)
                },
            };
            if let Some(pid) = pid { found.insert(pid); }
        }
        if found.len() != 1 { bail!("未找到唯一的匹配监听进程，未接管"); }
        return Ok(*found.first().unwrap());
    }
    bail!("监听进程持续变化，请稍后重试")
}

impl VerifiedService {
    pub(super) fn reconnect(executable: &Path, data: &Path, base: &str) -> Result<Self> {
        let pid = listener_pid(base)?;
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
        if raw.is_null() { bail!("无法取得本机记忆服务句柄，未接管"); }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let service = Self { created: creation(raw)?, handle, pid, executable: executable.into(), data: data.into(), base: base.into() };
        service.verify()?;
        Ok(service)
    }
    fn verify(&self) -> Result<()> {
        let raw = self.handle.as_raw_handle();
        if unsafe { WaitForSingleObject(raw, 0) } != WAIT_TIMEOUT || creation(raw)? != self.created || listener_pid(&self.base)? != self.pid {
            bail!("记忆服务身份或监听进程已经变化，未停止");
        }
        same_user(raw)?;
        if image(raw)?.canonicalize()? != self.executable.canonicalize()? { bail!("监听进程不是当前托管程序，未停止"); }
        matches_launch(&arguments(raw)?, &self.executable, &self.data)
    }
    pub(super) fn stop(self) -> Result<()> {
        self.verify()?;
        let raw = self.handle.as_raw_handle();
        if unsafe { TerminateProcess(raw, 0) } == 0 { return Err(std::io::Error::last_os_error()).context("停止已核验的旧实例服务失败"); }
        if unsafe { WaitForSingleObject(raw, 5000) } != WAIT_OBJECT_0 { bail!("等待旧实例服务退出失败，未开始备份或替换"); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_and_command_identity_reject_unrelated_process_without_stopping_it() {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            let listener = std::net::TcpListener::bind(address).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            assert_eq!(listener_pid(&base).unwrap(), std::process::id());
            let root = std::env::temp_dir().join(format!("memory-process-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("server.pid"), std::process::id().to_string()).unwrap();
            // Even a matching listener PID and executable cannot replace launch/data checks.
            assert!(VerifiedService::reconnect(&std::env::current_exe().unwrap(), &root, &base).is_err());
            assert_eq!(listener_pid(&base).unwrap(), std::process::id());
            let raw = unsafe { GetCurrentProcess() };
            assert_eq!(arguments(raw).unwrap(), std::env::args_os().map(PathBuf::from).collect::<Vec<_>>());
            same_user(raw).unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn launch_validation_requires_exact_data_config_and_no_extra_arguments() {
        let root = std::env::temp_dir().join(format!("memory-身份 核验-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let exe = root.join("ai-memory.exe");
        std::fs::write(&exe, "fixture").unwrap();
        std::fs::write(data.join("config.toml"), "fixture").unwrap();
        let mut args = vec![exe.clone(), "--data-dir".into(), data.clone(), "serve".into(), "--transport".into(), "http".into(), "--enable-web".into(), "--config".into(), data.join("config.toml")];
        matches_launch(&args, &exe, &data).unwrap();
        args.push("--unknown".into());
        assert!(matches_launch(&args, &exe, &data).is_err());
        args.pop();
        args[2] = root.clone();
        assert!(matches_launch(&args, &exe, &data).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
