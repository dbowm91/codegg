//! One-shot child sandbox launcher.
//!
//! This binary intentionally has no daemon or public protocol. The parent
//! supplies one bounded JSON launch description and one private status-pipe
//! descriptor. This process applies the complete containment policy in a
//! normal process context, reports setup state through the private pipe, and
//! then replaces itself with the target executable.
//!
//! Containment is **backend-neutral**: the policy is a list of paths, and
//! the registry in [`codegg::security::sandbox::backend`] decides which OS
//! facility enforces it (Landlock on Linux, Seatbelt on macOS). Adding a
//! backend changes this file not at all.
//!
//! This is the only place in the sandbox that uses `unsafe`, and only for
//! two reasons: the status-pipe descriptor plumbing, and the `sandbox_init`
//! FFI that the root library cannot make itself (`#![deny(unsafe_code)]`).

#[cfg(unix)]
use codegg::security::sandbox::{
    apply_backend, encode_sandbox_status, platform_sandbox_capability, SandboxCapability,
    SandboxLaunchOutcome, SandboxLaunchSpec, MAX_SANDBOX_SPEC_BYTES,
};
#[cfg(unix)]
use std::env;
#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::{self, File};
#[cfg(unix)]
use std::io::{Seek, Write};
#[cfg(unix)]
use std::os::fd::{FromRawFd, RawFd};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
fn write_status(fd: RawFd, outcome: SandboxLaunchOutcome) -> Result<(), String> {
    let frame = encode_sandbox_status(outcome)?;
    // The descriptor is owned by this helper. It is marked close-on-exec
    // only after the setup-success frame so the target cannot retain it.
    let mut writer = unsafe { File::from_raw_fd(fd) };
    writer
        .write_all(&frame)
        .map_err(|error| format!("write sandbox status: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("flush sandbox status: {error}"))?;
    if unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(format!(
            "mark sandbox status descriptor close-on-exec: {}",
            std::io::Error::last_os_error()
        ));
    }
    std::mem::forget(writer);
    Ok(())
}

#[cfg(unix)]
fn fail(fd: RawFd, outcome: SandboxLaunchOutcome, reason: impl std::fmt::Display) -> ! {
    let reason = reason.to_string();
    let outcome = match outcome {
        SandboxLaunchOutcome::Unavailable { .. } => SandboxLaunchOutcome::Unavailable {
            reason: reason.clone(),
        },
        SandboxLaunchOutcome::ExecError { .. } => SandboxLaunchOutcome::ExecError {
            reason: reason.clone(),
        },
        _ => SandboxLaunchOutcome::SetupError {
            reason: reason.clone(),
        },
    };
    let _ = write_status(fd, outcome);
    eprintln!("sandbox helper failure: {reason}");
    std::process::exit(125)
}

#[cfg(unix)]
fn parse_args() -> Result<(PathBuf, RawFd), String> {
    let mut args = env::args_os();
    let _program = args.next();
    let mut spec_path = None;
    let mut status_fd = None;
    while let Some(flag) = args.next() {
        match flag.to_string_lossy().as_ref() {
            "--spec" => spec_path = args.next().map(PathBuf::from),
            "--status-fd" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing --status-fd value".to_string())?;
                let parsed = value
                    .to_string_lossy()
                    .parse::<RawFd>()
                    .map_err(|_| "invalid --status-fd value".to_string())?;
                if parsed < 3 {
                    return Err("status fd must not be stdin, stdout, or stderr".to_string());
                }
                status_fd = Some(parsed);
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok((
        spec_path.ok_or_else(|| "missing --spec path".to_string())?,
        status_fd.ok_or_else(|| "missing --status-fd".to_string())?,
    ))
}

/// Apply the Seatbelt policy in this process.
///
/// The one `unsafe` block outside the status-pipe plumbing: `sandbox_init`
/// takes a raw `*const c_char` and returns an allocated error string.
///
/// `sandbox_init` is **one-shot per process** — a second call fails — which
/// is why availability is probed in `seatbelt::probe` rather than by calling
/// this speculatively. `SANDBOX_NAMED` is passed as `0` deliberately: with
/// flags set, `profile` names a built-in Apple constant rather than being
/// parsed as SBPL.
///
/// The profile is generated in the library (pure, testable) and applied
/// here, so this function does no policy work of its own.
#[cfg(target_os = "macos")]
fn apply_seatbelt(
    spec: &SandboxLaunchSpec,
) -> Result<codegg::security::sandbox::BackendEnforcement, String> {
    extern "C" {
        fn sandbox_init(
            profile: *const std::os::raw::c_char,
            flags: u64,
            error: *mut *mut std::os::raw::c_char,
        ) -> std::os::raw::c_int;
        fn sandbox_free_error(error: *mut std::os::raw::c_char);
    }

    let profile = codegg::security::sandbox::seatbelt::profile_for(spec);
    let c_profile = CString::new(profile.clone())
        .map_err(|_| "generated SBPL profile contains an interior NUL".to_string())?;

    let mut error: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { sandbox_init(c_profile.as_ptr(), 0, &mut error) };
    if rc != 0 {
        let reason = if error.is_null() {
            format!("sandbox_init rejected the generated SBPL profile: {profile}")
        } else {
            let message = unsafe { std::ffi::CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { sandbox_free_error(error) };
            message
        };
        return Err(reason);
    }
    Ok(codegg::security::sandbox::seatbelt::enforcement())
}

#[cfg(unix)]
fn main() {
    // Install the Seatbelt applier before any backend is resolved. On
    // non-macOS hosts this is a no-op the backend registry never reaches.
    #[cfg(target_os = "macos")]
    codegg::security::sandbox::seatbelt::install_apply_hook(apply_seatbelt);

    let (spec_path, status_fd) = parse_args().unwrap_or_else(|reason| {
        // There is no trustworthy channel until the fd argument itself has
        // been parsed. The parent treats this as a missing status frame.
        eprintln!("sandbox helper protocol failure: {reason}");
        std::process::exit(125);
    });
    let metadata = fs::metadata(&spec_path).unwrap_or_else(|error| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            error,
        )
    });
    if !metadata.file_type().is_file() {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            "sandbox specification is not a regular file",
        );
    }
    if metadata.len() > MAX_SANDBOX_SPEC_BYTES as u64 {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            "sandbox specification exceeds 64 KiB",
        );
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            "sandbox specification is not owner-only",
        );
    }
    let bytes = fs::read(&spec_path).unwrap_or_else(|error| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            error,
        )
    });
    let spec: SandboxLaunchSpec = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            error,
        )
    });

    #[cfg(target_os = "linux")]
    if spec.linux_protected_mount_view {
        exec_linux_protected_mount_view(&spec, status_fd);
    }

    // A host with no OS containment reports the canonical capability reason.
    // This stays a hard failure: the helper is only ever launched from a
    // `Required` request, and losing containment between decision and launch
    // must fail closed rather than degrade silently.
    if let SandboxCapability::Unavailable { reason } = platform_sandbox_capability() {
        fail(
            status_fd,
            SandboxLaunchOutcome::Unavailable {
                reason: String::new(),
            },
            reason,
        );
    }
    let outcome = apply_backend(&spec).unwrap_or_else(|reason| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            reason,
        )
    });
    if let Err(reason) = write_status(status_fd, outcome) {
        eprintln!("sandbox helper status failure: {reason}");
        std::process::exit(125);
    }

    let mut command = Command::new(&spec.target);
    command.args(&spec.args);
    let error = command.exec();
    // The setup frame is non-terminal; report the terminal exec failure on
    // the same private channel. The parent rejects any other duplicate shape.
    fail(
        status_fd,
        SandboxLaunchOutcome::ExecError {
            reason: String::new(),
        },
        error,
    );
}

/// Replace the helper with Bubblewrap after compiling and pinning every
/// source mount by descriptor. Bubblewrap's block-fd barrier prevents target
/// execution until the daemon acknowledges its child identity; Bubblewrap
/// reads that byte only after completing mount setup.
#[cfg(target_os = "linux")]
fn exec_linux_protected_mount_view(spec: &SandboxLaunchSpec, status_fd: RawFd) -> ! {
    let bubblewrap = codegg::security::sandbox::linux_launch::trusted_bubblewrap_path()
        .unwrap_or_else(|reason| {
            fail(
                status_fd,
                SandboxLaunchOutcome::Unavailable {
                    reason: String::new(),
                },
                reason,
            )
        });
    let cwd = env::current_dir().unwrap_or_else(|error| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            format!("resolve sandbox cwd: {error}"),
        )
    });
    let mut pinned_sources = Vec::<File>::new();
    let (mount_args, mut inherited_fds) =
        codegg::security::sandbox::linux_mount_view::compile(spec, &cwd, |path| {
            let fd = open_mount_source(path, &spec.path_identities)?;
            let raw = fd.as_raw_fd();
            pinned_sources.push(fd);
            Ok(raw)
        })
        .unwrap_or_else(|reason| {
            fail(
                status_fd,
                SandboxLaunchOutcome::SetupError {
                    reason: String::new(),
                },
                reason,
            )
        });

    let seccomp_filter = if spec.linux_process_isolation {
        let filter = codegg::security::sandbox::linux_seccomp::compile_filter()
            .and_then(|bytes| seccomp_filter_file(&bytes))
            .unwrap_or_else(|reason| {
                fail(
                    status_fd,
                    SandboxLaunchOutcome::Unavailable {
                        reason: String::new(),
                    },
                    reason,
                )
            });
        inherited_fds.push(filter.as_raw_fd());
        Some(filter)
    } else {
        None
    };

    if spec.linux_process_isolation {
        if let Err(reason) = codegg::security::sandbox::landlock::apply_process_scopes() {
            fail(
                status_fd,
                SandboxLaunchOutcome::Unavailable {
                    reason: String::new(),
                },
                reason,
            );
        }
    }

    let mut command = Command::new(bubblewrap);
    command
        .env_clear()
        .arg("--clearenv")
        .arg("--setenv")
        .arg("PATH")
        .arg("/usr/bin:/bin:/usr/local/bin")
        .arg("--setenv")
        .arg("HOME")
        .arg("/tmp")
        .arg("--setenv")
        .arg("TMPDIR")
        .arg("/tmp")
        .arg("--setenv")
        .arg("GIT_CONFIG_NOSYSTEM")
        .arg("1")
        .arg("--setenv")
        .arg("GIT_CONFIG_GLOBAL")
        .arg("/dev/null")
        .arg("--setenv")
        .arg("GIT_CONFIG_SYSTEM")
        .arg("/dev/null")
        .args(mount_args)
        .args(
            seccomp_filter
                .iter()
                .flat_map(|fd| ["--seccomp".to_string(), fd.as_raw_fd().to_string()]),
        )
        .arg("--block-fd")
        .arg(codegg::security::sandbox::SANDBOX_ACK_FD.to_string())
        .arg("--json-status-fd")
        .arg(status_fd.to_string());
    let target_env = validated_target_environment(&spec.env).unwrap_or_else(|reason| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            reason,
        )
    });
    for (name, value) in target_env {
        command.arg("--setenv").arg(name).arg(value);
    }
    command.arg("--").arg(spec.target.as_os_str());
    command.args(&spec.args);
    unsafe {
        command.pre_exec(move || {
            for fd in &inherited_fds {
                if libc::fcntl(*fd, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if libc::fcntl(status_fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(codegg::security::sandbox::SANDBOX_ACK_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let error = command.exec();
    drop(pinned_sources);
    fail(
        status_fd,
        SandboxLaunchOutcome::SetupError {
            reason: String::new(),
        },
        format!("exec trusted Bubblewrap: {error}"),
    )
}

#[cfg(target_os = "linux")]
fn validated_target_environment(env: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    use std::collections::BTreeSet;

    if env.len() > 64 {
        return Err("sandbox target environment exceeds 64 entries".into());
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    let mut validated = Vec::with_capacity(env.len());
    for (name, value) in env {
        if name.is_empty()
            || name.contains('=')
            || name.contains('\0')
            || value.contains('\0')
            || name.len() > 32 * 1024
            || value.len() > 32 * 1024
            || !names.insert(name.as_str())
        {
            return Err("sandbox target environment contains an invalid or duplicate entry".into());
        }
        total = total
            .checked_add(name.len())
            .and_then(|length| length.checked_add(value.len()))
            .ok_or_else(|| "sandbox target environment size overflowed".to_string())?;
        if total > MAX_SANDBOX_SPEC_BYTES {
            return Err("sandbox target environment exceeds the launch-spec bound".into());
        }
        // The helper owns these variables as part of its fixed contained
        // runtime; caller policy may choose PATH, but cannot widen home,
        // temporary, or Git configuration roots.
        if matches!(
            name.as_str(),
            "HOME" | "TMPDIR" | "GIT_CONFIG_NOSYSTEM" | "GIT_CONFIG_GLOBAL" | "GIT_CONFIG_SYSTEM"
        ) {
            continue;
        }
        validated.push((name.clone(), value.clone()));
    }
    Ok(validated)
}

#[cfg(target_os = "linux")]
fn seccomp_filter_file(bytes: &[u8]) -> Result<File, String> {
    use std::ffi::CString;

    if bytes.is_empty() || bytes.len() > 32 * 1024 || !bytes.len().is_multiple_of(8) {
        return Err("compiled seccomp filter has an invalid size".into());
    }
    let name = CString::new("codegg-m004-seccomp").expect("static memfd name");
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "create private seccomp filter descriptor: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)
        .map_err(|error| format!("write private seccomp filter: {error}"))?;
    file.flush()
        .map_err(|error| format!("flush private seccomp filter: {error}"))?;
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|error| format!("rewind private seccomp filter: {error}"))?;
    Ok(file)
}

#[cfg(target_os = "linux")]
fn open_mount_source(
    path: &std::path::Path,
    identities: &[codegg::security::sandbox::SandboxPathIdentity],
) -> Result<File, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }

    const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
    const RESOLVE_NO_SYMLINKS: u64 = 0x04;
    let path_c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "mount source contains a NUL byte".to_string())?;
    let flags = if path == std::path::Path::new("/dev/null") {
        libc::O_RDONLY | libc::O_CLOEXEC
    } else {
        libc::O_PATH | libc::O_CLOEXEC
    };
    let how = OpenHow {
        flags: flags as u64,
        mode: 0,
        resolve: RESOLVE_NO_MAGICLINKS | RESOLVE_NO_SYMLINKS,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            path_c.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        ) as RawFd
    };
    if fd < 0 {
        return Err(format!(
            "cannot pin mount source {} without following aliases: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if let Some(identity) = identities.iter().find(|identity| identity.path == path) {
        let metadata = file
            .metadata()
            .map_err(|error| format!("cannot verify pinned mount source: {error}"))?;
        if metadata.dev() != identity.device || metadata.ino() != identity.inode {
            return Err("mount source changed after the workspace lease was captured".into());
        }
    }
    Ok(file)
}

#[cfg(not(unix))]
fn main() {
    eprintln!("codegg-sandbox-helper is unavailable on this platform");
    std::process::exit(125);
}

#[cfg(all(test, target_os = "linux"))]
mod linux_mount_source_tests {
    use super::{open_mount_source, validated_target_environment};
    use codegg::security::sandbox::SandboxPathIdentity;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn open_mount_source_rejects_replacement_after_lease_snapshot() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let path = directory.path().join("leased-root");
        std::fs::create_dir(&path).expect("leased directory");
        let original = std::fs::metadata(&path).expect("leased metadata");
        let identity = SandboxPathIdentity {
            path: path.clone(),
            device: original.dev(),
            inode: original.ino(),
        };

        let displaced = directory.path().join("displaced-root");
        std::fs::rename(&path, &displaced).expect("replace lease path");
        std::fs::create_dir(&path).expect("replacement directory");

        let error = open_mount_source(&path, &[identity])
            .expect_err("replacement must not be accepted as the leased mount source");
        assert!(
            error.contains("mount source changed"),
            "unexpected identity failure: {error}"
        );
    }

    #[test]
    fn open_mount_source_keeps_original_inode_pinned_after_rename() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let path = directory.path().join("leased-root");
        std::fs::create_dir(&path).expect("leased directory");
        let original = std::fs::metadata(&path).expect("leased metadata");
        let identity = SandboxPathIdentity {
            path: path.clone(),
            device: original.dev(),
            inode: original.ino(),
        };
        let pinned = open_mount_source(&path, &[identity]).expect("pin leased directory");

        let displaced = directory.path().join("displaced-root");
        std::fs::rename(&path, &displaced).expect("rename leased directory");
        std::fs::create_dir(&path).expect("replacement directory");

        let metadata = pinned.metadata().expect("pinned handle metadata");
        assert_eq!(metadata.dev(), original.dev());
        assert_eq!(metadata.ino(), original.ino());
    }

    #[test]
    fn target_environment_is_bounded_unique_and_keeps_helper_owned_roots_fixed() {
        let env = vec![
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("HOME".to_string(), "/host/home".to_string()),
            ("TMPDIR".to_string(), "/host/tmp".to_string()),
            (
                "GIT_CONFIG_GLOBAL".to_string(),
                "/host/gitconfig".to_string(),
            ),
            ("TERM".to_string(), "xterm-256color".to_string()),
        ];
        assert_eq!(
            validated_target_environment(&env).expect("bounded target env"),
            vec![
                ("PATH".to_string(), "/usr/bin:/bin".to_string()),
                ("TERM".to_string(), "xterm-256color".to_string()),
            ]
        );
        assert!(validated_target_environment(&[
            ("TERM".to_string(), "xterm".to_string()),
            ("TERM".to_string(), "screen".to_string()),
        ])
        .is_err());
    }
}
