//! Narrow Windows process-liveness probe used before stale daemon recovery.

use std::io;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

#[doc(hidden)]
pub fn is_process_alive(pid: u32) -> io::Result<bool> {
    // The process handle is validated before use and is always closed below.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            let error = GetLastError();
            if error == windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER {
                return Ok(false);
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let wait = WaitForSingleObject(process, 0);
        let _ = CloseHandle(process);
        match wait {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => {
                let error = GetLastError();
                Err(io::Error::from_raw_os_error(error as i32))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_process_alive;

    #[test]
    fn reports_current_process_alive() {
        assert!(is_process_alive(std::process::id()).unwrap());
    }

    #[test]
    fn reports_invalid_pid_not_alive() {
        assert!(!is_process_alive(u32::MAX).unwrap());
    }
}
