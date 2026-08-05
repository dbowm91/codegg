//! One-shot child sandbox launcher.
//!
//! This binary intentionally has no daemon or public protocol. The parent
//! supplies one bounded JSON launch description and one private status-pipe
//! descriptor. This process applies the complete Landlock policy in a normal
//! process context, reports setup state through the private pipe, and then
//! replaces itself with the target executable.

#[cfg(unix)]
use codegg::security::sandbox::{
    apply_landlock, encode_sandbox_status, probe_landlock, SandboxLaunchOutcome, SandboxLaunchSpec,
    MAX_SANDBOX_SPEC_BYTES,
};
#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs::{self, File};
#[cfg(unix)]
use std::io::Write;
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

#[cfg(unix)]
fn main() {
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

    if let Err(reason) = probe_landlock() {
        fail(
            status_fd,
            SandboxLaunchOutcome::Unavailable {
                reason: String::new(),
            },
            reason,
        );
    }
    let abi = apply_landlock(&spec).unwrap_or_else(|reason| {
        fail(
            status_fd,
            SandboxLaunchOutcome::SetupError {
                reason: String::new(),
            },
            reason,
        )
    });
    if let Err(reason) = write_status(status_fd, SandboxLaunchOutcome::Enforced { abi }) {
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

#[cfg(not(unix))]
fn main() {
    eprintln!("codegg-sandbox-helper is unavailable on this platform");
    std::process::exit(125);
}
