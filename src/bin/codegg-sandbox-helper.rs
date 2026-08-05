//! One-shot child sandbox launcher.
//!
//! This binary intentionally has no daemon or public protocol.  The parent
//! supplies one bounded JSON launch description, this process applies the
//! complete Landlock policy in a normal process context, and then replaces
//! itself with the target executable.

#[cfg(unix)]
use codegg::security::sandbox::{
    apply_landlock, probe_landlock, SandboxLaunchSpec, SANDBOX_HELPER_ENFORCED_PREFIX,
    SANDBOX_HELPER_ERROR_PREFIX,
};
#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
const MAX_SPEC_BYTES: u64 = 64 * 1024;

#[cfg(unix)]
fn fail(kind: &str, reason: impl std::fmt::Display) -> ! {
    eprintln!("{SANDBOX_HELPER_ERROR_PREFIX}{kind}: {reason}");
    std::process::exit(125)
}

#[cfg(unix)]
fn main() {
    let mut args = env::args_os();
    let _program = args.next();
    let flag = args.next().unwrap_or_default();
    let spec_path = args.next().map(PathBuf::from);
    if flag != "--spec" || spec_path.is_none() || args.next().is_some() {
        fail("protocol", "usage: codegg-sandbox-helper --spec <file>");
    }
    let spec_path = spec_path.expect("checked above");
    let metadata = fs::metadata(&spec_path).unwrap_or_else(|e| fail("protocol", e));
    if metadata.len() > MAX_SPEC_BYTES {
        fail("protocol", "sandbox specification exceeds 64 KiB");
    }
    let bytes = fs::read(&spec_path).unwrap_or_else(|e| fail("protocol", e));
    let spec: SandboxLaunchSpec =
        serde_json::from_slice(&bytes).unwrap_or_else(|e| fail("protocol", e));

    if let Err(reason) = probe_landlock() {
        fail("unavailable", reason);
    }
    let abi = apply_landlock(&spec).unwrap_or_else(|reason| fail("setup", reason));
    eprintln!("{SANDBOX_HELPER_ENFORCED_PREFIX}{abi}");

    let mut command = Command::new(&spec.target);
    command.args(&spec.args);
    let error = command.exec();
    fail("exec", error);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("codegg-sandbox-helper is unavailable on this platform");
    std::process::exit(125);
}
