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
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("suspended child process has no PID"))?;
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("suspended child process handle is unavailable"))?
            as windows_sys::Win32::Foundation::HANDLE;
        let thread_id = find_suspended_primary_thread(pid)?;
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if thread.is_null() {
            return Err(last_error());
        }
        let thread = unsafe { OwnedHandle::from_raw_handle(thread as RawHandle) };
        self.assign_raw_and_resume(process, thread.as_raw_handle() as _)
    }

    fn assign_raw_and_resume(
        &mut self,
        process: windows_sys::Win32::Foundation::HANDLE,
        thread: windows_sys::Win32::Foundation::HANDLE,
    ) -> io::Result<()> {
        self.assign_suspended(process)?;
        Self::resume_suspended(thread)
    }

    fn assign_suspended(
        &mut self,
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> io::Result<()> {
        #[cfg(test)]
        if FAIL_NEXT_JOB_ASSIGNMENT.replace(false) {
            return Err(io::Error::other("injected Job assignment failure"));
        }

        let assigned =
            unsafe { AssignProcessToJobObject(self.handle.as_raw_handle() as _, process) };
        if assigned == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    fn resume_suspended(thread: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
        let previous_suspend_count = unsafe { ResumeThread(thread) };
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
    use super::{last_error, WindowsJob};
    use crate::managed_process::{
        ManagedProcessError, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    };
    use std::ffi::OsString;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::path::PathBuf;
    use std::ptr;
    use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeleteAppContainerProfile,
    };
    use windows_sys::Win32::Security::{
        EqualSid, FreeSid, GetTokenInformation, OpenProcessToken, TokenAppContainerSid,
        SECURITY_CAPABILITIES, TOKEN_APPCONTAINER_INFORMATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
    };

    struct TestAppContainerProfile {
        name: Vec<u16>,
        sid: windows_sys::Win32::Security::PSID,
    }

    impl TestAppContainerProfile {
        fn create() -> io::Result<Self> {
            let profile_name = format!("Codegg-M004-{}", uuid::Uuid::new_v4());
            let name = wide(&profile_name);
            let display = wide("CodeGG M004 launcher proof");
            let description = wide("Disposable test-only AppContainer profile");
            let mut sid = ptr::null_mut();
            let result = unsafe {
                CreateAppContainerProfile(
                    name.as_ptr(),
                    display.as_ptr(),
                    description.as_ptr(),
                    ptr::null(),
                    0,
                    &mut sid,
                )
            };
            if result < 0 {
                if !sid.is_null() {
                    unsafe {
                        let _ = FreeSid(sid);
                    }
                }
                return Err(io::Error::from_raw_os_error(result));
            }
            if sid.is_null() {
                unsafe {
                    let _ = DeleteAppContainerProfile(name.as_ptr());
                }
                return Err(io::Error::other(
                    "CreateAppContainerProfile returned a null package SID",
                ));
            }
            Ok(Self { name, sid })
        }
    }

    impl Drop for TestAppContainerProfile {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteAppContainerProfile(self.name.as_ptr());
                let _ = FreeSid(self.sid);
            }
        }
    }

    struct TestAttributeList {
        _storage: Vec<usize>,
        raw: *mut std::ffi::c_void,
    }

    impl TestAttributeList {
        fn new() -> io::Result<Self> {
            let mut bytes = 0usize;
            unsafe {
                let _ = InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes);
            }
            if bytes == 0 {
                return Err(io::Error::other(
                    "InitializeProcThreadAttributeList returned no buffer size",
                ));
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let raw = storage.as_mut_ptr().cast();
            if unsafe { InitializeProcThreadAttributeList(raw, 1, 0, &mut bytes) } == 0 {
                return Err(last_error());
            }
            Ok(Self {
                _storage: storage,
                raw,
            })
        }
    }

    impl Drop for TestAttributeList {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.raw) };
        }
    }

    struct TestSuspendedProcess {
        process: OwnedHandle,
        thread: OwnedHandle,
        resumed: bool,
    }

    impl Drop for TestSuspendedProcess {
        fn drop(&mut self) {
            if !self.resumed {
                unsafe {
                    let _ = TerminateProcess(self.process.as_raw_handle() as HANDLE, 1);
                }
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

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

    #[test]
    fn appcontainer_target_is_in_job_and_has_package_sid_before_resume() -> io::Result<()> {
        let profile = TestAppContainerProfile::create()?;
        let mut job = WindowsJob::create()?;
        let attributes = TestAttributeList::new()?;
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            ..SECURITY_CAPABILITIES::default()
        };
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.raw,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                (&capabilities as *const SECURITY_CAPABILITIES).cast(),
                size_of::<SECURITY_CAPABILITIES>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(last_error());
        }

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        let cmd = PathBuf::from(system_root).join("System32").join("cmd.exe");
        let application = wide(&cmd.to_string_lossy());
        let mut command_line = wide(&format!("\"{}\" /D /C exit 0", cmd.display()));
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attributes.raw;
        let mut info = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                ptr::null(),
                (&startup as *const STARTUPINFOEXW).cast(),
                &mut info,
            )
        };
        if created == 0 {
            return Err(last_error());
        }
        let mut process = TestSuspendedProcess {
            process: unsafe { OwnedHandle::from_raw_handle(info.hProcess as RawHandle) },
            thread: unsafe { OwnedHandle::from_raw_handle(info.hThread as RawHandle) },
            resumed: false,
        };

        let mut token = ptr::null_mut();
        if unsafe {
            OpenProcessToken(
                process.process.as_raw_handle() as HANDLE,
                TOKEN_QUERY,
                &mut token,
            )
        } == 0
        {
            return Err(last_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
        let mut appcontainer_info = TOKEN_APPCONTAINER_INFORMATION::default();
        let mut returned = 0u32;
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenAppContainerSid,
                (&mut appcontainer_info as *mut TOKEN_APPCONTAINER_INFORMATION).cast(),
                size_of::<TOKEN_APPCONTAINER_INFORMATION>() as u32,
                &mut returned,
            )
        } == 0
        {
            return Err(last_error());
        }
        if appcontainer_info.TokenAppContainer.is_null()
            || unsafe { EqualSid(appcontainer_info.TokenAppContainer, profile.sid) } == 0
        {
            return Err(io::Error::other(
                "suspended process token does not contain the requested AppContainer SID",
            ));
        }

        job.assign_suspended(process.process.as_raw_handle() as HANDLE)?;
        let mut in_job = 0;
        if unsafe {
            IsProcessInJob(
                process.process.as_raw_handle() as HANDLE,
                job.handle.as_raw_handle() as _,
                &mut in_job,
            )
        } == 0
            || in_job == 0
        {
            return Err(last_error());
        }
        WindowsJob::resume_suspended(process.thread.as_raw_handle() as HANDLE)?;
        process.resumed = true;
        if unsafe { WaitForSingleObject(process.process.as_raw_handle() as HANDLE, 30_000) }
            != WAIT_OBJECT_0
        {
            return Err(last_error());
        }
        let mut exit_code = u32::MAX;
        if unsafe { GetExitCodeProcess(process.process.as_raw_handle() as HANDLE, &mut exit_code) }
            == 0
        {
            return Err(last_error());
        }
        if exit_code != 0 {
            return Err(io::Error::other(format!(
                "AppContainer target exited with code {exit_code}"
            )));
        }
        Ok(())
    }
}
