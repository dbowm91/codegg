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
use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
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
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, OpenThread,
    ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    THREAD_SUSPEND_RESUME,
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
pub(super) struct WindowsProcessAttributes<'profile> {
    _storage: Vec<usize>,
    raw: *mut std::ffi::c_void,
    // `UpdateProcThreadAttribute` retains this pointer until the attribute
    // list is deleted. Keep the package profile alive for the same interval.
    _profile: Option<&'profile WindowsAppContainerProfile>,
    security_capabilities: Option<Box<SECURITY_CAPABILITIES>>,
    inherited_handles: Option<Box<[HANDLE]>>,
}

impl<'profile> WindowsProcessAttributes<'profile> {
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
            _profile: None,
            security_capabilities: None,
            inherited_handles: None,
        })
    }

    pub(super) fn set_appcontainer(
        &mut self,
        profile: &'profile WindowsAppContainerProfile,
    ) -> io::Result<()> {
        if self.security_capabilities.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "AppContainer security capabilities are already set",
            ));
        }

        // The attribute list borrows lpValue until DeleteProcThreadAttributeList,
        // which happens in Drop below. Keep both the struct allocation and the
        // profile owning AppContainerSid in this owner for that full interval.
        // There are no capabilities in this proof: null pointer and zero count.
        let capabilities = Box::new(SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid(),
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        });
        self._profile = Some(profile);
        self.security_capabilities = Some(capabilities);
        let capabilities = self
            .security_capabilities
            .as_deref()
            .expect("AppContainer capabilities were just stored");
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                (capabilities as *const SECURITY_CAPABILITIES)
                    .cast_mut()
                    .cast(),
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

    pub(super) fn set_handle_list(&mut self, handles: &[HANDLE]) -> io::Result<()> {
        if self.inherited_handles.is_some()
            || handles.is_empty()
            || handles.iter().any(|h| h.is_null())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inherited handle list must be non-empty, valid, and set once",
            ));
        }
        let owned = handles.to_vec().into_boxed_slice();
        let byte_len = std::mem::size_of_val(&*owned);
        self.inherited_handles = Some(owned);
        let handles = self
            .inherited_handles
            .as_deref()
            .expect("handle list was stored");
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast_mut().cast(),
                byte_len,
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

impl Drop for WindowsProcessAttributes<'_> {
    fn drop(&mut self) {
        // This must happen before the owned SECURITY_CAPABILITIES and profile
        // reference are released; the attribute list may still dereference them.
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

/// Pipe ends for one restricted child. Only the three `child_*` handles are
/// placed in the explicit process attribute handle list; the service retains
/// the other ends and drops child ends immediately after creation.
pub(super) struct WindowsStdioPipes {
    pub(super) stdin_writer: Option<OwnedHandle>,
    pub(super) stdout_reader: OwnedHandle,
    pub(super) stderr_reader: OwnedHandle,
    pub(super) child_stdin: OwnedHandle,
    pub(super) child_stdout: OwnedHandle,
    pub(super) child_stderr: OwnedHandle,
}

impl WindowsStdioPipes {
    pub(super) fn create() -> io::Result<Self> {
        let (child_stdin, stdin_writer) = create_pipe_ends()?;
        let (stdout_reader, child_stdout) = create_pipe_ends()?;
        let (stderr_reader, child_stderr) = create_pipe_ends()?;
        clear_inherit(&stdin_writer)?;
        clear_inherit(&stdout_reader)?;
        clear_inherit(&stderr_reader)?;
        Ok(Self {
            stdin_writer: Some(stdin_writer),
            stdout_reader,
            stderr_reader,
            child_stdin,
            child_stdout,
            child_stderr,
        })
    }
}

fn create_pipe_ends() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    let mut security = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    if unsafe { CreatePipe(&mut read, &mut write, &mut security, 0) } == 0 {
        return Err(last_error());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read as RawHandle),
            OwnedHandle::from_raw_handle(write as RawHandle),
        )
    })
}

fn clear_inherit(handle: &OwnedHandle) -> io::Result<()> {
    if unsafe { SetHandleInformation(handle.as_raw_handle() as HANDLE, HANDLE_FLAG_INHERIT, 0) }
        == 0
    {
        Err(last_error())
    } else {
        Ok(())
    }
}

#[cfg(feature = "windows-sandbox-test-support")]
pub(super) fn create_test_status_pipe() -> io::Result<(tokio::fs::File, std::fs::File)> {
    use std::os::windows::io::IntoRawHandle;
    let (reader, writer) = create_pipe_ends()?;
    clear_inherit(&reader)?;
    clear_inherit(&writer)?;
    let reader = unsafe { std::fs::File::from_raw_handle(reader.into_raw_handle()) };
    let writer = unsafe { std::fs::File::from_raw_handle(writer.into_raw_handle()) };
    Ok((tokio::fs::File::from_std(reader), writer))
}

pub(super) fn verify_appcontainer_token(
    process: HANDLE,
    expected_sid: windows_sys::Win32::Security::PSID,
) -> io::Result<()> {
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TokenAppContainerSid, TOKEN_APPCONTAINER_INFORMATION,
        TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let mut bytes = 0u32;
    unsafe {
        let _ = GetTokenInformation(
            token.as_raw_handle() as _,
            TokenAppContainerSid,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
    }
    if bytes < std::mem::size_of::<TOKEN_APPCONTAINER_INFORMATION>() as u32 {
        return Err(io::Error::other(
            "AppContainer token information size was invalid",
        ));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as _,
            TokenAppContainerSid,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(last_error());
    }
    let info = unsafe { &*storage.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>() };
    if info.TokenAppContainer.is_null()
        || unsafe { EqualSid(info.TokenAppContainer, expected_sid) } == 0
    {
        return Err(io::Error::other(
            "created process token AppContainer SID did not match",
        ));
    }
    Ok(())
}

impl WindowsSuspendedAppContainerProcess {
    pub(super) fn create(
        application: &Path,
        command_line: &mut [u16],
        attributes: &WindowsProcessAttributes,
    ) -> io::Result<Self> {
        Self::create_inner(application, command_line, attributes, None, None)
    }

    pub(super) fn create_with_stdio(
        application: &Path,
        command_line: &mut [u16],
        attributes: &WindowsProcessAttributes,
        stdin: HANDLE,
        stdout: HANDLE,
        stderr: HANDLE,
        cwd: &Path,
        environment: &mut [u16],
    ) -> io::Result<Self> {
        Self::create_inner(
            application,
            command_line,
            attributes,
            Some((stdin, stdout, stderr)),
            Some((cwd, environment)),
        )
    }

    fn create_inner(
        application: &Path,
        command_line: &mut [u16],
        attributes: &WindowsProcessAttributes,
        stdio: Option<(HANDLE, HANDLE, HANDLE)>,
        startup: Option<(&Path, &mut [u16])>,
    ) -> io::Result<Self> {
        if !application.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer executable must be an absolute path",
            ));
        }
        if application.as_os_str().encode_wide().any(|unit| unit == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer executable cannot contain NULs",
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
        let mut startup_info = STARTUPINFOEXW::default();
        startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup_info.lpAttributeList = attributes.raw;
        if let Some((stdin, stdout, stderr)) = stdio {
            startup_info.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
            startup_info.StartupInfo.hStdInput = stdin;
            startup_info.StartupInfo.hStdOutput = stdout;
            startup_info.StartupInfo.hStdError = stderr;
        }
        let has_startup = startup.is_some();
        let (cwd, environment) = if let Some((cwd, environment)) = startup {
            if !cwd.is_absolute()
                || cwd.as_os_str().encode_wide().any(|unit| unit == 0)
                || environment.len() < 2
                || environment[environment.len() - 2..] != [0, 0]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid restricted startup cwd or environment block",
                ));
            }
            (wide_os(cwd.as_os_str()), environment.as_mut_ptr())
        } else {
            (Vec::new(), ptr::null_mut())
        };
        let mut process_info = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(stdio.is_some()),
                CREATE_NO_WINDOW
                    | CREATE_SUSPENDED
                    | EXTENDED_STARTUPINFO_PRESENT
                    | if has_startup {
                        CREATE_UNICODE_ENVIRONMENT
                    } else {
                        0
                    },
                environment.cast(),
                if has_startup {
                    cwd.as_ptr()
                } else {
                    ptr::null()
                },
                &mut startup_info.StartupInfo,
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

#[cfg(feature = "windows-sandbox-test-support")]
pub(super) struct WindowsTestRestrictedProcess {
    process: WindowsSuspendedAppContainerProcess,
    job: Option<WindowsJob>,
    pub(super) stdin: Option<tokio::fs::File>,
    pub(super) stdout: Option<tokio::fs::File>,
    pub(super) stderr: Option<tokio::fs::File>,
}

#[cfg(feature = "windows-sandbox-test-support")]
impl WindowsTestRestrictedProcess {
    pub(super) fn launch(
        application: &Path,
        argv: &[std::ffi::OsString],
        profile: &WindowsAppContainerProfile,
        cwd: &Path,
        environment: &mut [u16],
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> io::Result<Self> {
        use std::os::windows::io::IntoRawHandle;

        let pipes = WindowsStdioPipes::create()?;
        let mut attributes = WindowsProcessAttributes::new(2)?;
        attributes.set_appcontainer(profile)?;
        attributes.set_handle_list(&[
            pipes.child_stdin.as_raw_handle() as HANDLE,
            pipes.child_stdout.as_raw_handle() as HANDLE,
            pipes.child_stderr.as_raw_handle() as HANDLE,
        ])?;
        let mut command_line = windows_command_line(application, argv)?;
        let mut child = WindowsSuspendedAppContainerProcess::create_with_stdio(
            application,
            &mut command_line,
            &attributes,
            pipes.child_stdin.as_raw_handle() as HANDLE,
            pipes.child_stdout.as_raw_handle() as HANDLE,
            pipes.child_stderr.as_raw_handle() as HANDLE,
            cwd,
            environment,
        )?;
        drop(attributes);
        drop((pipes.child_stdin, pipes.child_stdout, pipes.child_stderr));

        let mut job = WindowsJob::create()?;
        job.assign_appcontainer_and_resume(&mut child, |process| {
            verify_appcontainer_token(process, profile.sid())?;
            if cancellation.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "restricted launch cancelled before target resume",
                ));
            }
            Ok(())
        })?;

        let stdin = unsafe {
            std::fs::File::from_raw_handle(
                pipes
                    .stdin_writer
                    .expect("stdin writer is present")
                    .into_raw_handle(),
            )
        };
        let stdout =
            unsafe { std::fs::File::from_raw_handle(pipes.stdout_reader.into_raw_handle()) };
        let stderr =
            unsafe { std::fs::File::from_raw_handle(pipes.stderr_reader.into_raw_handle()) };
        Ok(Self {
            process: child,
            job: Some(job),
            stdin: Some(tokio::fs::File::from_std(stdin)),
            stdout: Some(tokio::fs::File::from_std(stdout)),
            stderr: Some(tokio::fs::File::from_std(stderr)),
        })
    }

    pub(super) fn job_established(&self) -> bool {
        let mut in_job = 0;
        self.job.as_ref().is_some_and(|job| unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                self.process.process_handle(),
                job.handle.as_raw_handle() as _,
                &mut in_job,
            ) != 0
                && in_job != 0
        })
    }

    pub(super) async fn wait(&self) -> io::Result<std::process::ExitStatus> {
        use std::os::windows::process::ExitStatusExt;
        loop {
            let result = unsafe { WaitForSingleObject(self.process.process_handle(), 0) };
            if result == windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
                let mut exit_code = 0;
                if unsafe {
                    windows_sys::Win32::System::Threading::GetExitCodeProcess(
                        self.process.process_handle(),
                        &mut exit_code,
                    )
                } == 0
                {
                    return Err(last_error());
                }
                return Ok(std::process::ExitStatus::from_raw(exit_code as i32));
            }
            if result == u32::MAX {
                return Err(last_error());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    pub(super) fn terminate(&self, exit_code: u32) -> io::Result<()> {
        self.job
            .as_ref()
            .ok_or_else(|| io::Error::other("restricted process Job Object already closed"))?
            .terminate(exit_code)
    }

    pub(super) fn close_job(&mut self) {
        self.job.take();
    }
}

#[cfg(feature = "windows-sandbox-test-support")]
impl Drop for WindowsTestRestrictedProcess {
    fn drop(&mut self) {
        self.job.take();
        unsafe {
            let _ = WaitForSingleObject(self.process.process_handle(), u32::MAX);
        }
    }
}

#[cfg(feature = "windows-sandbox-test-support")]
fn windows_command_line(application: &Path, argv: &[std::ffi::OsString]) -> io::Result<Vec<u16>> {
    let mut values = Vec::with_capacity(argv.len() + 1);
    values.push(application.as_os_str());
    values.extend(argv.iter().map(std::ffi::OsString::as_os_str));
    let mut command = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            command.push(b' ' as u16);
        }
        command.extend(quote_windows_argument(value));
    }
    if command.len() + 1 > 32_767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows command line exceeds 32,767 UTF-16 code units",
        ));
    }
    command.push(0);
    Ok(command)
}

#[cfg(feature = "windows-sandbox-test-support")]
fn quote_windows_argument(argument: &std::ffi::OsStr) -> Vec<u16> {
    let input = argument.encode_wide().collect::<Vec<_>>();
    let mut output = vec![b'"' as u16];
    let mut slashes = 0usize;
    for unit in input {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            output.push(unit);
        } else {
            output.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            output.push(unit);
        }
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    output
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
        let mut in_job = 0;
        if unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                process,
                self.handle.as_raw_handle() as _,
                &mut in_job,
            )
        } == 0
            || in_job == 0
        {
            return Err(io::Error::other(
                "suspended child was not verified in its owned Job Object",
            ));
        }
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
        WindowsSuspendedAppContainerProcess, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
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

    #[inline(never)]
    fn churn_stack() {
        let mut scratch = [0x5au8; 32 * 1024];
        for (index, byte) in scratch.iter_mut().enumerate() {
            *byte = index as u8;
        }
        std::hint::black_box(scratch);
    }

    #[test]
    fn appcontainer_attribute_owns_abi_value_and_profile_sid() -> io::Result<()> {
        let profile = WindowsAppContainerProfile::create()?;
        let mut attributes = WindowsProcessAttributes::new(1)?;
        attributes.set_appcontainer(&profile)?;

        let capabilities = attributes
            .security_capabilities
            .as_deref()
            .expect("the attribute owner retains its security capabilities");
        assert_eq!(capabilities.AppContainerSid, profile.sid());
        assert!(capabilities.Capabilities.is_null());
        assert_eq!(capabilities.CapabilityCount, 0);
        assert_eq!(capabilities.Reserved, 0);
        assert!(attributes._profile.is_some());
        assert_eq!(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            0x0002_0009
        );
        assert_eq!(
            size_of::<windows_sys::Win32::Security::SECURITY_CAPABILITIES>(),
            if size_of::<usize>() == 8 { 24 } else { 16 }
        );
        assert_eq!(
            size_of::<windows_sys::Win32::System::Threading::STARTUPINFOEXW>(),
            if size_of::<usize>() == 8 { 112 } else { 72 }
        );
        Ok(())
    }

    #[cfg(feature = "windows-sandbox-test-support")]
    #[test]
    fn restricted_attribute_list_contains_only_three_standard_handles() -> io::Result<()> {
        let profile = WindowsAppContainerProfile::create()?;
        let pipes = super::WindowsStdioPipes::create()?;
        let mut attributes = WindowsProcessAttributes::new(2)?;
        attributes.set_appcontainer(&profile)?;
        attributes.set_handle_list(&[
            pipes.child_stdin.as_raw_handle() as HANDLE,
            pipes.child_stdout.as_raw_handle() as HANDLE,
            pipes.child_stderr.as_raw_handle() as HANDLE,
        ])?;
        let handles = attributes
            .inherited_handles
            .as_deref()
            .expect("the handle-list owner retains its array");
        assert_eq!(handles.len(), 3);
        assert_ne!(handles[0], handles[1]);
        assert_ne!(handles[0], handles[2]);
        assert_ne!(handles[1], handles[2]);
        Ok(())
    }

    #[cfg(feature = "windows-sandbox-test-support")]
    #[tokio::test]
    async fn restricted_job_assignment_failure_never_runs_the_target() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let marker = directory.path().join("restricted-target-ran.txt");
        let executable = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let marker_text = marker.to_string_lossy().replace('\'', "''");
        let args = vec![
            OsString::from("-NoLogo"),
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-Command"),
            OsString::from(format!(
                "Set-Content -LiteralPath '{marker_text}' -Value ran"
            )),
        ];
        let mut request = ManagedProcessRequest::new(
            std::iter::once(executable.clone().into_os_string())
                .chain(args.iter().cloned())
                .collect(),
            PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("System32"),
            ProcessProvenance::new("restricted-failure-test", "attempt"),
        );
        request.sandbox = crate::managed_process::SandboxRequest::Required(
            crate::security::sandbox::SandboxLaunchSpec {
                target: executable,
                args: args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
                read_paths: Vec::new(),
                write_paths: Vec::new(),
                deny_paths: Vec::new(),
            },
        );
        FAIL_NEXT_JOB_ASSIGNMENT.with(|fail| fail.set(true));
        let result = ManagedProcessService::run(request).await;
        assert!(matches!(result, Err(ManagedProcessError::Spawn(_))));
        assert!(
            !marker.exists(),
            "restricted target ran before Job assignment"
        );
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
        let profile = WindowsAppContainerProfile::create()?;
        let mut job = WindowsJob::create()?;
        let mut attributes = WindowsProcessAttributes::new(1)?;
        attributes.set_appcontainer(&profile)?;
        // The previous implementation passed a stack-local capabilities
        // struct. Clobber stack storage before CreateProcessW to exercise the
        // regression while the owner must keep the attribute value stable.
        churn_stack();

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        let cmd = PathBuf::from(system_root).join("System32").join("cmd.exe");
        let mut command_line = wide(&format!("\"{}\" /D /C exit 0", cmd.display()));
        let mut process =
            WindowsSuspendedAppContainerProcess::create(&cmd, &mut command_line, &attributes)
                .map_err(|error| {
                    io::Error::new(error.kind(), format!("CreateProcessW failed: {error}"))
                })?;

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
        attributes.set_appcontainer(&profile)?;

        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        let executable = PathBuf::from(system_root).join("System32").join("cmd.exe");
        let mut command_line = wide(&format!("\"{}\" /D /C exit 0", executable.display()));
        let mut process = WindowsSuspendedAppContainerProcess::create(
            &executable,
            &mut command_line,
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
