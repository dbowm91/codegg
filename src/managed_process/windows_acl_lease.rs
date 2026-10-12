//! Journaled ACL leases for daemon-owned Windows directories.
//!
//! This module is only compiled with `windows-sandbox-test-support`. It is
//! not a production sandbox backend. Its test caller stores journals below
//! the per-user daemon root and only leases fresh disposable roots.

#![cfg(all(windows, feature = "windows-sandbox-test-support"))]
#![allow(unsafe_code)]

use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, DENY_ACCESS, EXPLICIT_ACCESS_W,
    SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, TokenOwner, TokenUser, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_OWNER, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const JOURNAL_VERSION: u32 = 2;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024;
const MAX_RUN_ID_BYTES: usize = 256;
const MAX_SID_BYTES: usize = 68;

#[cfg(test)]
thread_local! {
    static FAIL_AFTER_ACL_WRITE: Cell<bool> = const { Cell::new(false) };
}
const FILE_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LeaseAccess {
    Read,
    ReadWrite,
    Deny,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalRecord {
    version: u32,
    lease_id: String,
    run_id: String,
    appcontainer_sid: Vec<u8>,
    canonical_path: PathBuf,
    owner_sid: Vec<u8>,
    volume_serial: u32,
    file_index: u64,
    original_dacl: Vec<u8>,
    leased_dacl: Vec<u8>,
}

/// Owns a path lock and its durable before/after record until release.
pub(crate) struct WindowsAclLease {
    record_path: PathBuf,
    record: JournalRecord,
    _lock: File,
    released: bool,
}

impl WindowsAclLease {
    pub(crate) fn acquire(
        path: &Path,
        journal_root: &Path,
        run_id: &str,
        appcontainer_sid: windows_sys::Win32::Security::PSID,
        access: LeaseAccess,
    ) -> io::Result<Self> {
        if appcontainer_sid.is_null() || run_id.is_empty() || run_id.len() > MAX_RUN_ID_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ACL lease requires a bounded run id and AppContainer SID",
            ));
        }
        let sid_len = unsafe { GetLengthSid(appcontainer_sid) } as usize;
        if sid_len == 0 || sid_len > MAX_SID_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ACL lease AppContainer SID has an invalid length",
            ));
        }
        let appcontainer_sid_bytes =
            unsafe { std::slice::from_raw_parts(appcontainer_sid.cast::<u8>(), sid_len).to_vec() };
        fs::create_dir_all(journal_root)?;
        validate_journal_root(journal_root)?;
        let (canonical_path, owner_sid, volume_serial, file_index) =
            identify_owned_directory(path)?;
        let key = path_key(&canonical_path);
        let lock_path = journal_root.join(format!("{key}.lock"));
        let record_path = journal_root.join(format!("{key}.json"));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        acquire_file_lock(&lock)?;
        if record_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "an unrecovered ACL lease already exists for this directory",
            ));
        }

        let original_dacl = get_dacl(&canonical_path)?;
        let leased_dacl = add_appcontainer_ace(&original_dacl, appcontainer_sid, access)?;
        let record = JournalRecord {
            version: JOURNAL_VERSION,
            lease_id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            appcontainer_sid: appcontainer_sid_bytes,
            canonical_path: canonical_path.clone(),
            owner_sid,
            volume_serial,
            file_index,
            original_dacl,
            leased_dacl: leased_dacl.clone(),
        };
        write_new_record(&record_path, &record)?;

        // The complete before/expected-after image is durable before the ACL
        // changes. Recovery can distinguish an unapplied lease, an applied
        // lease, and concurrent descriptor drift without a mutable journal
        // state transition.
        if let Err(error) = apply_lease_dacl(&canonical_path, &leased_dacl) {
            if restore_record(&record).is_ok() {
                let _ = fs::remove_file(&record_path);
            }
            return Err(error);
        }
        Ok(Self {
            record_path,
            record,
            _lock: lock,
            released: false,
        })
    }

    pub(crate) fn release(mut self) -> io::Result<()> {
        self.restore()?;
        self.released = true;
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        restore_record(&self.record)?;
        fs::remove_file(&self.record_path)?;
        self.released = true;
        Ok(())
    }

    #[cfg(test)]
    fn simulate_process_crash(mut self) {
        // Keep the durable record/ACL but release the OS lock as process exit
        // would. Drop then sees released=true and does not restore anything.
        self.released = true;
    }
}

impl Drop for WindowsAclLease {
    fn drop(&mut self) {
        if !self.released {
            if let Err(error) = self.restore() {
                tracing::error!("Windows ACL lease cleanup failed; journal retained: {error}");
            }
        }
    }
}

/// Reconcile every journal in an exclusively owned lease directory.
/// Unknown versions and descriptor/identity drift are preserved as conflicts.
pub(crate) fn recover_all(journal_root: &Path) -> io::Result<Vec<PathBuf>> {
    if !journal_root.exists() {
        return Ok(Vec::new());
    }
    validate_journal_root(journal_root)?;
    let mut conflicts = Vec::new();
    for entry in fs::read_dir(journal_root)? {
        let entry = entry?;
        if entry.path().extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        let record_path = entry.path();
        let metadata = fs::metadata(&record_path)?;
        if metadata.len() > MAX_JOURNAL_BYTES {
            conflicts.push(record_path);
            continue;
        }
        let bytes = fs::read(&record_path)?;
        let record: JournalRecord = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(_) => {
                conflicts.push(record_path);
                continue;
            }
        };
        if record.version != JOURNAL_VERSION {
            conflicts.push(record_path);
            continue;
        }
        let lock_path = record_path.with_extension("lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        if !try_file_lock(&lock)? {
            conflicts.push(record_path);
            continue;
        }
        match restore_record(&record) {
            Ok(()) => fs::remove_file(&record_path)?,
            Err(_) => conflicts.push(record_path),
        }
    }
    Ok(conflicts)
}

pub(crate) fn dacl_snapshot(path: &Path) -> io::Result<Vec<u8>> {
    get_dacl(path)
}

fn validate_journal_root(path: &Path) -> io::Result<()> {
    identify_owned_directory(path).map(|_| ())
}

fn identify_owned_directory(path: &Path) -> io::Result<(PathBuf, Vec<u8>, u32, u64)> {
    let link_metadata = fs::symlink_metadata(path)?;
    if !link_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ACL leases support only non-reparse directories",
        ));
    }
    let canonical = fs::canonicalize(path)?;
    let name = wide(&canonical);
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle() as _, &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ACL lease path resolves to a reparse point",
        ));
    }
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    let owner_sid = owner_sid(&canonical)?;
    if owner_sid != current_user_sid()? && owner_sid != current_token_owner_sid()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ACL lease root is not owned by the current daemon user or its token owner",
        ));
    }
    Ok((
        canonical,
        owner_sid,
        information.dwVolumeSerialNumber,
        file_index,
    ))
}

fn restore_record(record: &JournalRecord) -> io::Result<()> {
    let (canonical, owner_sid, volume_serial, file_index) =
        identify_owned_directory(&record.canonical_path)?;
    if canonical != record.canonical_path
        || owner_sid != record.owner_sid
        || volume_serial != record.volume_serial
        || file_index != record.file_index
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ACL lease path identity changed; journal retained for operator repair",
        ));
    }
    let current = get_dacl(&canonical)?;
    if current == record.original_dacl {
        return Ok(());
    }
    if current != record.leased_dacl {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ACL lease descriptor changed concurrently; refusing restoration",
        ));
    }
    set_dacl(&canonical, &record.original_dacl)
}

fn owner_sid(path: &Path) -> io::Result<Vec<u8>> {
    let name = wide(path);
    let mut owner = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _descriptor = LocalAllocation(descriptor.cast());
    sid_bytes(owner, "ACL lease root has no owner SID")
}

fn current_token_owner_sid() -> io::Result<Vec<u8>> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let mut bytes = 0u32;
    unsafe {
        let _ = GetTokenInformation(
            token.as_raw_handle() as _,
            TokenOwner,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
    }
    if bytes < std::mem::size_of::<TOKEN_OWNER>() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "current process token owner information is invalid",
        ));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as _,
            TokenOwner,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let owner = unsafe { &*storage.as_ptr().cast::<TOKEN_OWNER>() };
    sid_bytes(owner.Owner, "current process token has no owner SID")
}

fn current_token_owner_sid() -> io::Result<Vec<u8>> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let mut bytes = 0u32;
    unsafe {
        let _ = GetTokenInformation(
            token.as_raw_handle() as _,
            TokenOwner,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
    }
    if bytes < std::mem::size_of::<TOKEN_OWNER>() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "current process token owner information is invalid",
        ));
    }
    let mut storage = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as _,
            TokenOwner,
            storage.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let owner = unsafe { &*storage.as_ptr().cast::<TOKEN_OWNER>() };
    sid_bytes(owner.Owner, "current process token has no owner SID")
}

fn sid_bytes(sid: windows_sys::Win32::Security::PSID, message: &str) -> io::Result<Vec<u8>> {
    if sid.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, message));
    }
    let length = unsafe { GetLengthSid(sid) } as usize;
    if length == 0 || length > MAX_SID_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows SID length is outside the supported bound",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec())
}

fn write_new_record(path: &Path, record: &JournalRecord) -> io::Result<()> {
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ACL lease journal record exceeds its size bound",
        ));
    }
    let temporary_path = path.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    use std::io::Write;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

fn path_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().to_lowercase();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn acquire_file_lock(file: &File) -> io::Result<()> {
    let start = Instant::now();
    loop {
        if try_file_lock(file)? {
            return Ok(());
        }
        if start.elapsed() >= FILE_LOCK_TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "timed out waiting for the path's ACL lease lock",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn try_file_lock(file: &File) -> io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

fn get_dacl(path: &Path) -> io::Result<Vec<u8>> {
    let name = wide(path);
    let mut dacl = ptr::null_mut::<ACL>();
    let mut descriptor = ptr::null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _descriptor = LocalAllocation(descriptor.cast());
    if dacl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "null or absent DACL is not safe to lease",
        ));
    }
    let size = unsafe { (*dacl).AclSize as usize };
    if size < std::mem::size_of::<ACL>() || size > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid ACL size",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), size) }.to_vec())
}

fn add_appcontainer_ace(
    original: &[u8],
    sid: windows_sys::Win32::Security::PSID,
    access: LeaseAccess,
) -> io::Result<Vec<u8>> {
    let mut aligned = vec![0usize; original.len().div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        ptr::copy_nonoverlapping(
            original.as_ptr(),
            aligned.as_mut_ptr().cast(),
            original.len(),
        );
    }
    let (access_mask, access_mode) = match access {
        LeaseAccess::Read => (FILE_GENERIC_READ, SET_ACCESS),
        LeaseAccess::ReadWrite => (FILE_GENERIC_READ | FILE_GENERIC_WRITE, SET_ACCESS),
        LeaseAccess::Deny => (FILE_GENERIC_READ | FILE_GENERIC_WRITE, DENY_ACCESS),
    };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access_mask,
        grfAccessMode: access_mode,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut new_acl = ptr::null_mut::<ACL>();
    let result = unsafe { SetEntriesInAclW(1, &entry, aligned.as_ptr().cast(), &mut new_acl) };
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let _new_acl = LocalAllocation(new_acl.cast());
    let size = unsafe { (*new_acl).AclSize as usize };
    Ok(unsafe { std::slice::from_raw_parts(new_acl.cast::<u8>(), size) }.to_vec())
}

fn set_dacl(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = wide(path);
    let mut aligned = vec![0usize; bytes.len().div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
    }
    let result = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            aligned.as_ptr().cast(),
            ptr::null_mut(),
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

fn apply_lease_dacl(path: &Path, bytes: &[u8]) -> io::Result<()> {
    set_dacl(path, bytes)?;
    #[cfg(test)]
    if FAIL_AFTER_ACL_WRITE.replace(false) {
        return Err(io::Error::other("injected failure after DACL mutation"));
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct LocalAllocation(*mut core::ffi::c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_process::windows::WindowsAppContainerProfile;

    #[test]
    fn windows_acl_lease_restores_the_exact_before_image() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let profile = WindowsAppContainerProfile::create().unwrap();
        let before = get_dacl(&root).unwrap();
        let lease = WindowsAclLease::acquire(
            &root,
            &journal,
            "test-run",
            profile.sid(),
            LeaseAccess::ReadWrite,
        )
        .unwrap();
        assert_ne!(get_dacl(&root).unwrap(), before);
        lease.release().unwrap();
        assert_eq!(get_dacl(&root).unwrap(), before);
        assert!(fs::read_dir(journal).unwrap().all(|entry| {
            entry.unwrap().path().extension().and_then(OsStr::to_str) != Some("json")
        }));
    }

    #[test]
    fn windows_acl_lease_rolls_back_after_failure_following_acl_mutation() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let profile = WindowsAppContainerProfile::create().unwrap();
        let before = get_dacl(&root).unwrap();
        FAIL_AFTER_ACL_WRITE.with(|fail| fail.set(true));
        assert!(WindowsAclLease::acquire(
            &root,
            &journal,
            "test-run",
            profile.sid(),
            LeaseAccess::ReadWrite,
        )
        .is_err());
        assert_eq!(get_dacl(&root).unwrap(), before);
        assert!(fs::read_dir(journal).unwrap().all(|entry| {
            entry.unwrap().path().extension().and_then(OsStr::to_str) != Some("json")
        }));
    }

    #[test]
    fn windows_acl_lease_recovers_after_simulated_parent_crash() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let profile = WindowsAppContainerProfile::create().unwrap();
        let before = get_dacl(&root).unwrap();
        WindowsAclLease::acquire(
            &root,
            &journal,
            "test-run",
            profile.sid(),
            LeaseAccess::Read,
        )
        .unwrap()
        .simulate_process_crash();
        assert_ne!(get_dacl(&root).unwrap(), before);
        assert!(recover_all(&journal).unwrap().is_empty());
        assert_eq!(get_dacl(&root).unwrap(), before);
    }

    #[test]
    fn windows_acl_lease_refuses_descriptor_drift_and_keeps_journal() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let profile = WindowsAppContainerProfile::create().unwrap();
        let lease = WindowsAclLease::acquire(
            &root,
            &journal,
            "test-run",
            profile.sid(),
            LeaseAccess::Read,
        )
        .unwrap();
        let current = get_dacl(&root).unwrap();
        let drift = add_appcontainer_ace(&current, profile.sid(), LeaseAccess::ReadWrite).unwrap();
        set_dacl(&root, &drift).unwrap();
        assert!(lease.release().is_err());
        assert_eq!(recover_all(&journal).unwrap().len(), 1);
    }

    #[test]
    fn windows_acl_lease_refuses_a_reused_path_identity() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let moved = sandbox.path().join("moved-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let profile = WindowsAppContainerProfile::create().unwrap();
        let lease = WindowsAclLease::acquire(
            &root,
            &journal,
            "test-run",
            profile.sid(),
            LeaseAccess::Read,
        )
        .unwrap();
        lease.simulate_process_crash();
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        let replacement_before = get_dacl(&root).unwrap();
        assert_eq!(recover_all(&journal).unwrap().len(), 1);
        assert_eq!(get_dacl(&root).unwrap(), replacement_before);
    }

    #[test]
    fn windows_acl_lease_serializes_same_path_leases_across_threads() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("owned-root");
        let journal = sandbox.path().join("journal");
        fs::create_dir(&root).unwrap();
        let first_profile = WindowsAppContainerProfile::create().unwrap();
        let first = WindowsAclLease::acquire(
            &root,
            &journal,
            "first-run",
            first_profile.sid(),
            LeaseAccess::Read,
        )
        .unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let root_for_thread = root.clone();
        let journal_for_thread = journal.clone();
        let second = std::thread::spawn(move || {
            let profile = WindowsAppContainerProfile::create().unwrap();
            started_tx.send(()).unwrap();
            WindowsAclLease::acquire(
                &root_for_thread,
                &journal_for_thread,
                "second-run",
                profile.sid(),
                LeaseAccess::Read,
            )
            .unwrap()
            .release()
            .unwrap();
        });
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        first.release().unwrap();
        second.join().unwrap();
        assert!(fs::read_dir(journal).unwrap().all(|entry| {
            entry.unwrap().path().extension().and_then(OsStr::to_str) != Some("json")
        }));
    }
}
