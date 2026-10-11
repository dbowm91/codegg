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
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;

use tokio::process::Child;
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{FreeSid, SECURITY_CAPABILITIES};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, OpenThread,
    ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW, THREAD_SUSPEND_RESUME,
};

/// A uniquely named AppContainer package SID whose lifetime is tied to the
/// launch setup that owns it. ACL leases must outlive this value when recovery
/// is required; dropping it only removes the profile after its process has
/// exited and its leases have been restored.
pub(super) struct WindowsAppContainerProfile {
    name: Vec<u16>,
    sid: windows_sys::Win32::Security::PSID,
}

impl WindowsAppContainerProfile {
    pub(super) fn create() -> io::Result<Self> {
        let profile_name = format!("Codegg-{}", uuid::Uuid::new_v4());
        let name = wide(&profile_name);
        let display = wide("CodeGG managed process");
        let description = wide("Per-launch CodeGG AppContainer");
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

    pub(super) fn sid(&self) -> windows_sys::Win32::Security::PSID {
        self.sid
    }
}

impl Drop for WindowsAppContainerProfile {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteAppContainerProfile(self.name.as_ptr());
            let _ = FreeSid(self.sid);
        }
    }
}

/// Owns the aligned storage required by `STARTUPINFOEXW`'s attribute list.
/// Keep this value alive through `CreateProcessW`.
pub(super) struct WindowsProcessAttributes {
    _storage: Vec<usize>,
    raw: *mut std::ffi::c_void,
}

impl WindowsProcessAttributes {
    pub(super) fn new(attribute_count: u32) -> io::Result<Self> {
        let mut bytes = 0usize;
        unsafe {
            let _ =
                InitializeProcThreadAttributeList(ptr::null_mut(), attribute_count, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::other(
                "InitializeProcThreadAttributeList returned no buffer size",
            ));
        }
        let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let raw = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(raw, attribute_count, 0, &mut bytes) } == 0 {
            return Err(last_error());
        }
        Ok(Self {
            _storage: storage,
            raw,
        })
    }

    pub(super) fn set_appcontainer(
        &mut self,
        sid: windows_sys::Win32::Security::PSID,
    ) -> io::Result<()> {
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: sid,
            ..SECURITY_CAPABILITIES::default()
        };
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                (&capabilities as *const SECURITY_CAPABILITIES).cast(),
                std::mem::size_of::<SECURITY_CAPABILITIES>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(())
    }
}

impl Drop for WindowsProcessAttributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.raw) };
    }
}

/// A process created with the requested AppContainer token and still
/// suspended. The caller must assign it to the managed Job and verify the
/// token before resuming it.
pub(super) struct WindowsSuspendedAppContainerProcess {
    process: OwnedHandle,
    thread: OwnedHandle,
    resumed: bool,
}

impl WindowsSuspendedAppContainerProcess {
    pub(super) fn create(
        application: &Path,
        command_line: &mut [u16],
        current_directory: &Path,
        attributes: &WindowsProcessAttributes,
    ) -> io::Result<Self> {
        if !application.is_absolute() || !current_directory.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer executable and working directory must be absolute paths",
            ));
        }
        if application.as_os_str().encode_wide().any(|unit| unit == 0)
            || current_directory
                .as_os_str()
                .encode_wide()
                .any(|unit| unit == 0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer executable and working directory cannot contain NULs",
            ));
        }
        if command_line.len() < 2
            || command_line.last() != Some(&0)
            || command_line[..command_line.len() - 1].contains(&0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer command line must be NUL-terminated without interior NULs",
            ));
        }
        let application = wide_os(application.as_os_str());
        let current_directory = wide_os(current_directory.as_os_str());
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attributes.raw;
        let mut process_info = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                current_directory.as_ptr(),
                &mut startup.StartupInfo,
                &mut process_info,
            )
        };
        if created == 0 {
            return Err(last_error());
        }
        Ok(Self {
            process: unsafe { OwnedHandle::from_raw_handle(process_info.hProcess as RawHandle) },
            thread: unsafe { OwnedHandle::from_raw_handle(process_info.hThread as RawHandle) },
            resumed: false,
        })
    }

    pub(super) fn process_handle(&self) -> HANDLE {
        self.process.as_raw_handle() as HANDLE
    }

    pub(super) fn thread_handle(&self) -> HANDLE {
        self.thread.as_raw_handle() as HANDLE
    }

    pub(super) fn mark_resumed(&mut self) {
        self.resumed = true;
    }
}

impl Drop for WindowsSuspendedAppContainerProcess {
    fn drop(&mut self) {
        if !self.resumed {
            unsafe {
                let _ = TerminateProcess(self.process_handle(), 1);
                // Do not let the AppContainer profile be deleted while the
                // failed, unresumed process is still tearing down.
                let _ = WaitForSingleObject(self.process_handle(), u32::MAX);
            }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

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
        self.assign_raw_and_resume(process, thread.as_raw_handle() as _, |_| Ok(()))
    }

    pub(super) fn assign_appcontainer_and_resume(
        &mut self,
        child: &mut WindowsSuspendedAppContainerProcess,
        verify_before_resume: impl FnOnce(HANDLE) -> io::Result<()>,
    ) -> io::Result<()> {
        self.assign_raw_and_resume(
            child.process_handle(),
            child.thread_handle(),
            verify_before_resume,
        )?;
        child.mark_resumed();
        Ok(())
    }

    fn assign_raw_and_resume(
        &mut self,
        process: windows_sys::Win32::Foundation::HANDLE,
        thread: windows_sys::Win32::Foundation::HANDLE,
        verify_before_resume: impl FnOnce(windows_sys::Win32::Foundation::HANDLE) -> io::Result<()>,
    ) -> io::Result<()> {
        self.assign_suspended(process)?;
        verify_before_resume(process)?;
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
    use super::{
        last_error, wide, WindowsAppContainerProfile, WindowsJob, WindowsProcessAttributes,
        WindowsSuspendedAppContainerProcess,
    };
    use crate::managed_process::{
        ManagedProcessError, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    };
    use std::ffi::OsString;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::path::PathBuf;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TokenAppContainerSid, TOKEN_APPCONTAINER_INFORMATION,
        TOKEN_QUERY,
    };
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
    };

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
        let profile = WindowsAppContainerProfile::create()?;
        let mut job = WindowsJob::create()?;
        let mut attributes = WindowsProcessAttributes::new(1)?;
        attributes.set_appcontainer(profile.sid())?;

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        let cmd = PathBuf::from(system_root).join("System32").join("cmd.exe");
        let working_directory = cmd
            .parent()
            .ok_or_else(|| io::Error::other("cmd.exe path has no parent directory"))?;
        let mut command_line = wide(&format!("\"{}\" /D /C exit 0", cmd.display()));
        let mut process = WindowsSuspendedAppContainerProcess::create(
            &cmd,
            &mut command_line,
            working_directory,
            &attributes,
        )
        .map_err(|error| io::Error::new(error.kind(), format!("CreateProcessW failed: {error}")))?;

        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(process.process_handle(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
        let mut returned = 0u32;
        let query_succeeded = unsafe {
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenAppContainerSid,
                ptr::null_mut(),
                0,
                &mut returned,
            )
        };
        let query_error = unsafe { GetLastError() };
        if query_succeeded != 0
            || query_error != ERROR_INSUFFICIENT_BUFFER
            || returned < size_of::<TOKEN_APPCONTAINER_INFORMATION>() as u32
        {
            return Err(io::Error::from_raw_os_error(query_error as i32));
        }
        let mut token_info_storage = vec![0usize; (returned as usize).div_ceil(size_of::<usize>())];
        let appcontainer_info = token_info_storage
            .as_mut_ptr()
            .cast::<TOKEN_APPCONTAINER_INFORMATION>();
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenAppContainerSid,
                appcontainer_info.cast(),
                returned,
                &mut returned,
            )
        } == 0
        {
            return Err(last_error());
        }
        let appcontainer_info = unsafe { &*appcontainer_info };
        if appcontainer_info.TokenAppContainer.is_null()
            || unsafe { EqualSid(appcontainer_info.TokenAppContainer, profile.sid()) } == 0
        {
            return Err(io::Error::other(
                "suspended process token does not contain the requested AppContainer SID",
            ));
        }

        let job_handle = job.handle.as_raw_handle() as _;
        job.assign_appcontainer_and_resume(&mut process, |process_handle| {
            let mut in_job = 0;
            if unsafe { IsProcessInJob(process_handle, job_handle, &mut in_job) } == 0 {
                return Err(last_error());
            }
            if in_job == 0 {
                return Err(io::Error::other(
                    "AppContainer target was not in its owner Job before resume",
                ));
            }
            Ok(())
        })
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Job assignment, verification, or resume failed: {error}"),
            )
        })?;
        if unsafe { WaitForSingleObject(process.process_handle(), 30_000) } != WAIT_OBJECT_0 {
            return Err(last_error());
        }
        let mut exit_code = u32::MAX;
        if unsafe { GetExitCodeProcess(process.process_handle(), &mut exit_code) } == 0 {
            return Err(last_error());
        }
        if exit_code != 0 {
            return Err(io::Error::other(format!(
                "AppContainer target exited with code {exit_code}"
            )));
        }
        Ok(())
    }

    #[test]
    fn appcontainer_verification_failure_keeps_target_suspended() -> io::Result<()> {
        let profile = WindowsAppContainerProfile::create()?;
        let mut job = WindowsJob::create()?;
        let mut attributes = WindowsProcessAttributes::new(1)?;
        attributes.set_appcontainer(profile.sid())?;

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        let executable = PathBuf::from(system_root).join("System32").join("cmd.exe");
        let working_directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("cmd.exe path has no parent directory"))?;
        let mut command_line = wide(&format!("\"{}\" /D /C exit 0", executable.display()));
        let mut process = WindowsSuspendedAppContainerProcess::create(
            &executable,
            &mut command_line,
            working_directory,
            &attributes,
        )?;
        let process_handle = process.process_handle();

        let error = job
            .assign_appcontainer_and_resume(&mut process, |_| {
                Err(io::Error::other("injected token verification failure"))
            })
            .expect_err("verification failure must prevent resume");
        assert!(error
            .to_string()
            .contains("injected token verification failure"));
        assert_eq!(
            unsafe { WaitForSingleObject(process_handle, 0) },
            windows_sys::Win32::Foundation::WAIT_TIMEOUT,
            "verification failure must leave the target suspended until cleanup"
        );

        // Dropping the suspended process synchronously terminates it before
        // the profile and its package SID are released.
        drop(process);
        drop(attributes);
        drop(job);
        drop(profile);
        Ok(())
    }
}
