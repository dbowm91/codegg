//! Windows process-tree ownership for `ManagedProcessService`.
//!
//! The target is created suspended, assigned to a kill-on-close Job Object,
//! and resumed only after the assignment succeeds. The Job handle remains
//! owned by the managed-process call until the root exits and its inherited
//! output pipes have been released. This is process supervision only; it does
//! not claim filesystem or network containment.

#[cfg(test)]
use std::cell::Cell;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr;

use tokio::process::Child;
use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_JOB_ASSIGNMENT: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct WindowsJob {
    handle: OwnedHandle,
}

impl WindowsJob {
    pub(super) fn create() -> io::Result<Self> {
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(last_error());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(last_error());
        }
        Ok(Self { handle })
    }

    pub(super) fn assign_and_resume(&mut self, child: &Child) -> io::Result<()> {
        #[cfg(test)]
        if FAIL_NEXT_JOB_ASSIGNMENT.replace(false) {
            return Err(io::Error::other("injected Job assignment failure"));
        }

        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("suspended child process has no PID"))?;
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("suspended child process handle is unavailable"))?
            as windows_sys::Win32::Foundation::HANDLE;
        let assigned =
            unsafe { AssignProcessToJobObject(self.handle.as_raw_handle() as _, process) };
        if assigned == 0 {
            return Err(last_error());
        }

        let thread_id = find_suspended_primary_thread(pid)?;
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if thread.is_null() {
            return Err(last_error());
        }
        let thread = unsafe { OwnedHandle::from_raw_handle(thread as RawHandle) };
        let previous_suspend_count = unsafe { ResumeThread(thread.as_raw_handle() as _) };
        if previous_suspend_count == u32::MAX {
            return Err(last_error());
        }
        if previous_suspend_count != 1 {
            return Err(io::Error::other(format!(
                "suspended child primary thread had unexpected suspend count {previous_suspend_count}"
            )));
        }
        Ok(())
    }

    pub(super) fn terminate(&self, exit_code: u32) -> io::Result<()> {
        let result = unsafe { TerminateJobObject(self.handle.as_raw_handle() as _, exit_code) };
        if result == 0 {
            Err(last_error())
        } else {
            Ok(())
        }
    }
}

fn find_suspended_primary_thread(pid: u32) -> io::Result<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(last_error());
    }
    let _snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as RawHandle) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    if unsafe { Thread32First(snapshot, &mut entry) } == 0 {
        return Err(last_error());
    }

    let mut primary = None;
    loop {
        if entry.th32OwnerProcessID == pid {
            if primary.replace(entry.th32ThreadID).is_some() {
                return Err(io::Error::other(
                    "suspended child exposed more than one thread before resume",
                ));
            }
        }
        if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
            let error = unsafe { GetLastError() };
            if error != windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            break;
        }
    }
    primary.ok_or_else(|| io::Error::other("suspended child primary thread was not found"))
}

fn last_error() -> io::Error {
    let error = unsafe { GetLastError() };
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(test)]
mod tests {
    use super::FAIL_NEXT_JOB_ASSIGNMENT;
    use crate::managed_process::{
        ManagedProcessError, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    };
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_job_assignment_never_runs_the_suspended_target() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let marker = directory.path().join("must-not-run.txt");
        let powershell = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let quoted_marker = format!("'{}'", marker.to_string_lossy().replace('\'', "''"));
        let script = format!("[IO.File]::WriteAllText({quoted_marker}, 'ran')");
        let request = ManagedProcessRequest::new(
            vec![
                powershell.into_os_string(),
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from(script),
            ],
            std::env::current_dir().expect("current directory"),
            ProcessProvenance::new("windows-job-assignment-test", "attempt"),
        );

        FAIL_NEXT_JOB_ASSIGNMENT.set(true);
        let error = ManagedProcessService::run(request)
            .await
            .expect_err("injected Job assignment failure must fail launch");
        assert!(matches!(error, ManagedProcessError::Spawn(_)));
        assert!(!marker.exists(), "target ran before Job setup completed");
    }
}
