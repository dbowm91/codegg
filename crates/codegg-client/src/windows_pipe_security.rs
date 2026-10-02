//! Narrow Windows API boundary for constructing the local pipe DACL.
//!
//! The descriptor grants access only to the current process token user and
//! LocalSystem. Anonymous and Everyone receive no ACE. Tokio passes the
//! descriptor synchronously to `CreateNamedPipeW`, which copies it while
//! creating each server instance.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[doc(hidden)]
pub struct PipeSecurity {
    descriptor: *mut c_void,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    pub fn current_user() -> io::Result<Self> {
        // All raw pointers and handles in this function are returned by
        // Windows APIs and are checked before use. The token buffer is
        // machine-word aligned and remains alive until SID conversion ends.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = TokenHandle(token);

            let mut required = 0_u32;
            let _ = windows_sys::Win32::Security::GetTokenInformation(
                token.0,
                TokenUser,
                null_mut(),
                0,
                &mut required,
            );
            if required == 0 {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            let word_count = (required as usize).div_ceil(std::mem::size_of::<usize>());
            let mut token_data = vec![0_usize; word_count];
            if windows_sys::Win32::Security::GetTokenInformation(
                token.0,
                TokenUser,
                token_data.as_mut_ptr().cast(),
                required,
                &mut required,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let token_user = &*(token_data.as_ptr().cast::<TOKEN_USER>());

            let mut sid_string = null_mut();
            if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut sid_len = 0;
            while *sid_string.add(sid_len) != 0 {
                sid_len += 1;
            }
            let sid = String::from_utf16(std::slice::from_raw_parts(sid_string, sid_len))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            let _ = LocalFree(sid_string.cast());
            let sid = sid?;

            let sddl: Vec<u16> = pipe_sddl_for_sid(&sid)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut descriptor = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            Ok(Self {
                descriptor,
                attributes,
            })
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        (&mut self.attributes as *mut SECURITY_ATTRIBUTES).cast()
    }
}

fn pipe_sddl_for_sid(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)")
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(self.descriptor);
        }
    }
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pipe_sddl_for_sid, PipeSecurity};

    #[test]
    fn pipe_dacl_is_protected_and_excludes_broad_principals() {
        let sddl = pipe_sddl_for_sid("S-1-5-21-1-2-3-1001");
        assert!(sddl.starts_with("D:P"));
        assert!(sddl.contains("S-1-5-21-1-2-3-1001"));
        assert!(sddl.contains(";;;SY)"));
        assert!(!sddl.contains(";;;WD)"));
        assert!(!sddl.contains(";;;AN)"));
        assert!(!sddl.contains(";;;AU)"));
    }

    #[test]
    fn current_user_security_descriptor_can_be_built() {
        let mut security = PipeSecurity::current_user().expect("current-user pipe ACL");
        assert!(!security.as_mut_ptr().is_null());
    }
}
