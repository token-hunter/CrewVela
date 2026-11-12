use crate::win::psuedocon::HPCON;
use anyhow::{ensure, Error};
use std::io::Error as IoError;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::um::processthreadsapi::*;

const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x00020016;
// Windows 10+: PROC_THREAD_ATTRIBUTE_JOB_LIST, as defined by WinBase.h.
const PROC_THREAD_ATTRIBUTE_JOB_LIST: usize = 0x0002000D;

pub struct ProcThreadAttributeList {
    data: Vec<u8>,
    job_handles: Vec<winapi::um::winnt::HANDLE>,
}

impl ProcThreadAttributeList {
    pub fn with_capacity(num_attributes: DWORD) -> Result<Self, Error> {
        let mut bytes_required: usize = 0;
        unsafe {
            InitializeProcThreadAttributeList(
                ptr::null_mut(),
                num_attributes,
                0,
                &mut bytes_required,
            )
        };
        // Exposing uninitialized bytes as a Vec<u8>/slice is undefined behavior,
        // even when the Windows API will subsequently initialize the buffer.
        let mut data = vec![0u8; bytes_required];

        let attr_ptr = data.as_mut_slice().as_mut_ptr() as *mut _;
        let res = unsafe {
            InitializeProcThreadAttributeList(attr_ptr, num_attributes, 0, &mut bytes_required)
        };
        ensure!(
            res != 0,
            "InitializeProcThreadAttributeList failed: {}",
            IoError::last_os_error()
        );
        Ok(Self { data, job_handles: Vec::new() })
    }

    pub fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.data.as_mut_slice().as_mut_ptr() as *mut _
    }

    pub fn set_jobs(&mut self, jobs: &[winapi::um::winnt::HANDLE]) -> Result<(), Error> {
        self.job_handles = jobs.to_vec();
        let res = unsafe {
            UpdateProcThreadAttribute(self.as_mut_ptr(), 0, PROC_THREAD_ATTRIBUTE_JOB_LIST,
                self.job_handles.as_mut_ptr() as *mut _,
                mem::size_of::<winapi::um::winnt::HANDLE>() * self.job_handles.len(),
                ptr::null_mut(), ptr::null_mut())
        };
        ensure!(res != 0, "setting Windows Job attribute failed: {}", IoError::last_os_error());
        Ok(())
    }

    pub fn set_pty(&mut self, con: HPCON) -> Result<(), Error> {
        let res = unsafe {
            UpdateProcThreadAttribute(
                self.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                con,
                mem::size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        ensure!(
            res != 0,
            "UpdateProcThreadAttribute failed: {}",
            IoError::last_os_error()
        );
        Ok(())
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}
