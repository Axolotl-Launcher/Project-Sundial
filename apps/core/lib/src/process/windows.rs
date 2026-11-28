//! A non-inheritable Job Object contains every writer until it has stopped.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};

pub(super) struct Tree {
    job: OwnedHandle,
    attached: bool,
    terminated: bool,
}

impl Tree {
    #[allow(
        unsafe_code,
        reason = "A fresh unnamed job with null attributes is private and non-inheritable."
    )]
    pub fn new() -> io::Result<Self> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                size_of_val(&limits) as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            job,
            attached: false,
            terminated: false,
        })
    }

    #[allow(
        unsafe_code,
        reason = "Both job and child handles are live; attachment precedes the first request."
    )]
    pub fn attach(&mut self, child: &Child) -> io::Result<()> {
        let assigned =
            unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), child.as_raw_handle()) };
        if assigned == 0 {
            return Err(io::Error::last_os_error());
        }
        self.attached = true;
        Ok(())
    }

    pub fn poll_exit(&mut self, child: &mut Child, stop: bool) -> io::Result<bool> {
        if !self.attached {
            return super::single_exit(child, true);
        }
        let exited = child.try_wait()?.is_some();
        if (exited || stop) && !self.terminated {
            self.stop_job()?;
        }
        Ok(exited && self.empty()?)
    }

    fn stop_job(&mut self) -> io::Result<()> {
        if !matches!(self.empty(), Ok(true)) {
            self.terminate()?;
        }
        self.terminated = true;
        Ok(())
    }

    #[allow(
        unsafe_code,
        reason = "Terminates only the private job owned by this reaper."
    )]
    fn terminate(&self) -> io::Result<()> {
        let result = unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[allow(
        unsafe_code,
        reason = "The job accounting query writes into a correctly sized live buffer."
    )]
    fn empty(&self) -> io::Result<bool> {
        let mut stats = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let result = unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut stats).cast(),
                size_of_val(&stats) as u32,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(stats.ActiveProcesses == 0)
    }
}
