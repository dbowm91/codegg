//! macOS Seatbelt backend.
//!
//! Seatbelt evaluates an SBPL (Scheme) profile and, on macOS, is reachable
//! **in-process** through `sandbox_init`. No `sandbox-exec` wrapper is
//! needed: the helper calls `sandbox_init` and then `exec()`s the target,
//! exactly as it does for Landlock, and the restrictions survive the exec.
//!
//! The header is marked deprecated and App Sandbox is the blessed path for
//! shipped applications — but App Sandbox is entitlement-gated and means
//! nothing for a CLI tool. Seatbelt remains the only OS-provided,
//! non-entitlement filesystem confinement available to a terminal process,
//! and it is what Claude Code and the OpenAI Codex CLI ship for exactly this
//! reason.
//!
//! ## Evaluation model
//!
//! SBPL is **last-rule-wins**, which is what makes the tool-compatible
//! policy expressible: start from `(allow default)`, deny reads of sensitive
//! material, then deny *all* writes and re-allow exactly the writable roots.
//! Confinement lands on writes — the operation that damages a host — while
//! reads stay broad enough that the toolchain still runs.
//!
//! ## Honest limits
//!
//! Seatbelt has no `no_new_privs` equivalent, so this backend deliberately
//! reports **fewer** guarantees than Landlock. In particular a setuid binary
//! reachable inside an allowed read root may regain privilege; pre-open file
//! descriptors survive; and Apple Events / Launch Services remain a
//! cross-process channel the profile does not close. These are reported in
//! [`BackendEnforcement::limits`] rather than papered over.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use super::backend::BackendEnforcement;
use super::policy::BackendPolicy;
use super::SandboxLaunchSpec;

/// What Seatbelt actually provides.
const SEATBELT_GUARANTEES: &[&str] = &[
    "writes confined to the workspace and scratch space",
    "credential material denied by path",
    "inherited by exec'd target",
];

/// Escapes that survive a Seatbelt profile.
const SEATBELT_LIMITS: &[&str] = &[
    "no network isolation",
    "no no_new_privs equivalent: a setuid binary in an allowed read root may regain privilege",
    "open file descriptors inherited before the profile apply remain usable",
    "Apple Events and Launch Services remain a cross-process channel",
    "reads are broadly allowed: containment is write-confined, not read-allowlisted",
];

/// Path of the shipped front end for the Seatbelt facility.
#[cfg(target_os = "macos")]
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Availability probe.
///
/// `sandbox_init` is **one-shot per process**: once a profile is applied a
/// second call fails. Probing it in this process would therefore consume the
/// single application the helper has, so the probe deliberately does not call
/// it.
///
/// Instead it checks for `sandbox-exec`, the shipped front end for the same
/// kernel facility. Its absence means the SBPL profile compiler is not
/// serviceable here, which is a stronger statement than "the symbol exists".
/// This is a cheap serviceability signal, not proof that a given profile
/// compiles — [`apply_profile`] is the real gate and fails closed.
#[cfg(target_os = "macos")]
pub fn probe() -> Result<(), String> {
    let front_end = Path::new(SANDBOX_EXEC);
    if front_end.exists() {
        Ok(())
    } else {
        Err(format!(
            "Seatbelt is unavailable: {SANDBOX_EXEC} is not present"
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn probe() -> Result<(), String> {
    Err("Seatbelt is a macOS sandbox and is not available on this platform".to_string())
}

/// Compile a policy into an SBPL profile.
///
/// Last-rule-wins, so the order below is the whole semantics:
/// allow everything, take back sensitive reads, take back *all* writes, then
/// re-allow exactly the writable roots.
pub fn sbpl_profile(policy: &BackendPolicy) -> String {
    let mut profile = String::with_capacity(1024);
    profile.push_str("(version 1)\n");
    // Baseline: read is broadly permitted. Tool compatibility lives here;
    // writes are what we actually take back below.
    profile.push_str("(allow default)\n");

    for path in &policy.denied_roots {
        push_subpath(&mut profile, "deny", "file-read*", path);
        push_subpath(&mut profile, "deny", "file-write*", path);
    }

    if policy.write_roots.is_empty() {
        // ReadOnly: no writes at all, anywhere.
        profile.push_str("(deny file-write*)\n");
    } else {
        profile.push_str("(deny file-write*)\n");
        for path in &policy.write_roots {
            push_subpath(&mut profile, "allow", "file-write*", path);
        }
    }
    profile
}

/// Append one `(op operation* (subpath "path"))` rule.
///
/// Paths are escaped because they are interpolated into a quoted Scheme
/// string: a workspace directory containing a quote or backslash would
/// otherwise produce a profile that parses as a different rule.
fn push_subpath(profile: &mut String, op: &str, operation: &str, path: &Path) {
    // An empty subpath compiles cleanly and matches nothing, so it would
    // silently turn a real allowance into a no-op that reads as if it
    // granted something. A policy can be built by hand, so the renderer
    // defends itself rather than trusting normalization upstream.
    if path.as_os_str().is_empty() {
        return;
    }
    profile.push('(');
    profile.push_str(op);
    profile.push(' ');
    profile.push_str(operation);
    profile.push_str(" (subpath \"");
    for ch in path.to_string_lossy().chars() {
        if ch == '"' || ch == '\\' {
            profile.push('\\');
        }
        profile.push(ch);
    }
    profile.push_str("\"))\n");
}

/// Apply the spec's policy in the current process.
///
/// The `sandbox_init` FFI is `unsafe`, and the root library denies
/// `unsafe_code`; the only place allowed to make that call is the one-shot
/// helper binary. This module therefore owns the profile *text* and the
/// enforcement report, and the helper installs the applier at startup with
/// [`install_apply_hook`] before the helper resolves a backend.
///
/// An uninstalled hook is a hard failure, not a fallback: it can only happen
/// if containment was requested outside the helper, and running the target
/// uncontained there would silently launder a `Required` request.
#[cfg(target_os = "macos")]
pub fn apply_profile(spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    let apply = apply_hook().ok_or_else(|| {
        "Seatbelt containment was requested outside the sandbox helper".to_string()
    })?;
    apply(spec)
}

#[cfg(target_os = "macos")]
type ApplyHook = fn(&SandboxLaunchSpec) -> Result<BackendEnforcement, String>;

#[cfg(target_os = "macos")]
static APPLY_HOOK: std::sync::OnceLock<ApplyHook> = std::sync::OnceLock::new();

/// Install the process-local Seatbelt applier. Called once by the sandbox
/// helper at startup. Idempotent: a second call is ignored rather than
/// panicking, because "already installed" is not an error condition worth
/// failing a launch over.
#[cfg(target_os = "macos")]
pub fn install_apply_hook(apply: ApplyHook) {
    let _ = APPLY_HOOK.set(apply);
}

#[cfg(target_os = "macos")]
fn apply_hook() -> Option<ApplyHook> {
    APPLY_HOOK.get().copied()
}

/// Apply the spec's policy in the current process.
#[cfg(not(target_os = "macos"))]
pub fn apply_profile(_spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    Err("Seatbelt is a macOS sandbox and is not available on this platform".to_string())
}

/// The generated profile for a launch spec. Exposed so the caller can render
/// it and hand the text to the helper.
pub fn profile_for(spec: &SandboxLaunchSpec) -> String {
    sbpl_profile(&BackendPolicy::for_spec(spec))
}

/// Assurance descriptor for a profile this backend applied.
///
/// Seatbelt exposes no versioned policy ABI, so `abi` is `None` — reported
/// as absent rather than invented.
pub fn enforcement() -> BackendEnforcement {
    BackendEnforcement::new(None, SEATBELT_GUARANTEES, SEATBELT_LIMITS)
}

/// Paths a generated profile must never contain empty.
///
/// A `(subpath "")` rule compiles but matches nothing, and would silently
/// turn a real allowance into a no-op — the failure mode that looks like
/// "the sandbox is working" while granting nothing.
#[cfg(test)]
fn malformed_subpaths(profile: &str) -> Vec<String> {
    profile
        .lines()
        .filter(|line| line.contains("(subpath \"\")"))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn policy(dir: &TempDir, writable: bool) -> BackendPolicy {
        let read = vec![dir.path().to_path_buf()];
        let write = if writable {
            vec![dir.path().to_path_buf()]
        } else {
            Vec::new()
        };
        BackendPolicy::new(read, write, vec![PathBuf::from("/etc/ssh")])
    }

    #[test]
    fn read_only_profile_denies_every_write() {
        let dir = TempDir::new().expect("workspace fixture");
        let profile = sbpl_profile(&policy(&dir, false));
        assert!(
            profile.contains("(deny file-write*)"),
            "read-only must deny writes wholesale: {profile}"
        );
        assert!(
            !profile.contains("(allow file-write*"),
            "read-only must re-allow no write path: {profile}"
        );
    }

    #[test]
    fn workspace_write_profile_re_allows_only_the_workspace() {
        let dir = TempDir::new().expect("workspace fixture");
        let profile = sbpl_profile(&policy(&dir, true));
        let canonical = dir.path().canonicalize().expect("canonical root");
        assert!(profile.contains("(deny file-write*)"), "{profile}");
        assert!(
            profile.contains(&format!(
                "(allow file-write* (subpath \"{}\"))",
                canonical.display()
            )),
            "the workspace must be re-allowed: {profile}"
        );
    }

    #[test]
    fn profile_always_starts_from_the_allow_default_baseline() {
        // Last-rule-wins evaluation means a missing baseline silently
        // turns every later deny into a dead rule.
        let dir = TempDir::new().expect("workspace fixture");
        let profile = sbpl_profile(&policy(&dir, true));
        assert!(profile.starts_with("(version 1)"));
        assert!(profile.contains("(allow default)"), "{profile}");
    }

    #[test]
    fn denied_paths_are_removed_from_both_directions() {
        let dir = TempDir::new().expect("workspace fixture");
        let profile = sbpl_profile(&policy(&dir, true));
        // Rendered paths are the *resolved* ones: `/etc` is a symlink to
        // `/private/etc` on macOS and Seatbelt matches on the resolved
        // path, so a rule has to name what the filesystem actually reports.
        let denied = Path::new("/etc/ssh")
            .canonicalize()
            .expect("canonical denied root");
        assert!(
            profile.contains(&format!(
                "(deny file-read* (subpath \"{}\"))",
                denied.display()
            )),
            "{profile}"
        );
        assert!(
            profile.contains(&format!(
                "(deny file-write* (subpath \"{}\"))",
                denied.display()
            )),
            "a denied path must be denied for writes too: {profile}"
        );
    }

    #[test]
    fn deny_rules_precede_allow_rules_so_last_rule_wins() {
        let dir = TempDir::new().expect("workspace fixture");
        let profile = sbpl_profile(&policy(&dir, true));
        let deny_at = profile.find("(deny file-write*)").expect("deny present");
        let allow_at = profile.find("(allow file-write*").expect("allow present");
        assert!(
            deny_at < allow_at,
            "SBPL is last-rule-wins; re-allows must come after the blanket deny:\n{profile}"
        );
    }

    #[test]
    fn quoted_paths_cannot_break_out_of_the_rule() {
        let mut escaped = String::new();
        let weird = PathBuf::from("/tmp/a\") (allow default)");
        push_subpath(&mut escaped, "deny", "file-read*", &weird);
        assert!(
            !escaped.contains("/tmp/a\") (allow default)"),
            "an unescaped quote would inject a second rule: {escaped}"
        );
        assert!(
            escaped.contains("\\\""),
            "the quote must be escaped: {escaped}"
        );
    }

    #[test]
    fn generated_never_contains_an_empty_subpath() {
        let dir = TempDir::new().expect("workspace fixture");
        // Deliberately bypasses `BackendPolicy::new`: a hand-built policy is
        // reachable, and the renderer must never emit a rule that matches
        // nothing while appearing to grant access.
        let mut p = policy(&dir, true);
        p.write_roots.push(PathBuf::new());
        p.denied_roots.push(PathBuf::from(""));
        p.read_roots.push(PathBuf::new());
        let profile = sbpl_profile(&p);
        assert!(
            malformed_subpaths(&profile).is_empty(),
            "an empty subpath compiles but matches nothing: {profile}"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn seatbelt_reports_a_readable_reason_off_macos() {
        let reason = probe().expect_err("Seatbelt must be unavailable off macOS");
        assert!(reason.contains("Seatbelt"), "{reason}");
        let err = apply_profile(&SandboxLaunchSpec {
            target: PathBuf::from("/bin/sh"),
            args: vec![],
            read_paths: vec![],
            write_paths: vec![],
            deny_paths: vec![],
        })
        .expect_err("Seatbelt must be unavailable off macOS");
        assert!(err.contains("Seatbelt"), "{err}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_probe_answers_on_macos() {
        // Either it is usable or it names why; never a silent false.
        if let Err(reason) = probe() {
            assert!(!reason.trim().is_empty(), "{reason}");
        }
    }
}
