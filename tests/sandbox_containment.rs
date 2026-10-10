//! End-to-end containment: run the real helper and observe real enforcement.
//!
//! These tests execute `codegg-sandbox-helper` as a separate process with a
//! real launch spec, and assert on what the sandboxed command could actually
//! do. Nothing here trusts the helper's own report: the assertion is always
//! the *observed* behaviour of the target, and the status frame is only
//! checked for consistency with it.
//!
//! Mechanism-agnostic on purpose. The same contract — workspace writes
//! allowed under `WorkspaceWrite`, no writes under `ReadOnly`, writes outside
//! the workspace always denied, denied paths never readable — is asserted
//! against whichever backend the host provides, so a new backend inherits
//! the whole matrix for free.

use codegg::security::sandbox::{
    decode_sandbox_status, platform_sandbox_capability, BackendId, SandboxCapability,
    SandboxConfig, SandboxLaunchOutcome, SandboxLaunchSpec, SandboxMode, SANDBOX_STATUS_FD,
};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn helper_path() -> PathBuf {
    std::env::current_exe()
        .expect("test executable")
        .parent()
        .and_then(|path| path.parent())
        .expect("target debug directory")
        .join("codegg-sandbox-helper")
}

/// Outcome of one helper launch.
struct Run {
    stdout: String,
    status: Vec<u8>,
}

impl Run {
    /// Decode the private status channel, or `None` when the helper produced
    /// no frame (a protocol failure the caller must treat as not-contained).
    fn outcome(&self) -> Option<SandboxLaunchOutcome> {
        decode_sandbox_status(&self.status).ok()
    }

    fn backend(&self) -> Option<BackendId> {
        self.outcome().and_then(|outcome| outcome.backend())
    }
}

fn run_helper(spec: &SandboxLaunchSpec) -> Run {
    let spec_file = tempfile::NamedTempFile::new().expect("sandbox spec file");
    serde_json::to_writer(spec_file.as_file(), spec).expect("sandbox spec encoding");

    launch_helper(helper_path(), spec_file.path())
}

/// Fork/exec the helper with its status channel wired to `SANDBOX_STATUS_FD`.
fn launch_helper(helper: PathBuf, spec: &Path) -> Run {
    let (reader, writer) = nix::unistd::pipe().expect("status pipe");
    let writer_fd = writer.as_raw_fd();
    let mut command = Command::new(helper);
    command
        .arg("--spec")
        .arg(spec)
        .arg("--status-fd")
        .arg(SANDBOX_STATUS_FD.to_string());
    // SAFETY: `pre_exec` runs between fork and exec in a single-threaded
    // child here. It duplicates an already-open descriptor onto a fixed
    // number and clears close-on-exec; it allocates nothing and runs no
    // user code. This mirrors the parent's own status-pipe setup.
    unsafe {
        command.pre_exec(move || {
            if writer_fd != SANDBOX_STATUS_FD {
                if libc::dup2(writer_fd, SANDBOX_STATUS_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(writer_fd);
            }
            // The helper clears FD_CLOEXEC on this descriptor itself once it
            // has written the setup frame.
            if libc::fcntl(SANDBOX_STATUS_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let Output { stdout, .. } = command.output().expect("sandbox helper process");
    // Drop the parent's copy of the write end so the reader sees EOF. This
    // is what lets an `Enforced`-then-EOF stream decode as a clean success.
    drop(writer);
    let mut status = Vec::new();
    std::fs::File::from(reader)
        .read_to_end(&mut status)
        .expect("sandbox status frame");
    Run {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        status,
    }
}

/// Build a real spec through the production path, so the curated
/// tool-compatible allowances are exercised rather than hand-rolled.
fn spec_for(workspace: &Path, mode: SandboxMode, extra_deny: &[PathBuf]) -> SandboxLaunchSpec {
    let mut config = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(mode)
        .with_allowed_paths(vec![workspace.to_string_lossy().into_owned()]);
    config.deny_paths = extra_deny
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    config
        .launch_spec(
            "sh",
            &["-c".to_string(), "true".to_string()],
            Some(workspace),
        )
        .expect("production launch spec")
}

/// Run a probe script under the sandbox and return its stdout.
fn probe(workspace: &Path, mode: SandboxMode, deny: &[PathBuf], script: &str) -> Option<Run> {
    let base = spec_for(workspace, mode, deny);
    let mut spec = base;
    spec.args = vec!["-c".to_string(), script.to_string()];
    let run = run_helper(&spec);
    // A helper that could not establish containment never ran the target.
    run.backend().map(|_| run)
}

fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

fn skip_reason() -> Option<String> {
    match platform_sandbox_capability() {
        SandboxCapability::Available { .. } => None,
        SandboxCapability::Unavailable { reason } => Some(reason),
    }
}

macro_rules! require_backend {
    () => {
        if let Some(reason) = skip_reason() {
            eprintln!("skipped: no containment backend available ({reason})");
            return;
        }
    };
}

// ── The backend actually engages ────────────────────────────────────────

#[test]
fn helper_reports_enforcement_with_a_real_backend_identity() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let run = run_helper(&spec_for(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
    ));

    let outcome = run
        .outcome()
        .expect("a successful helper run must produce a decodable status frame");
    let backend = outcome
        .backend()
        .expect("a contained run must name the backend that enforced it");
    assert!(
        codegg::security::sandbox::backend::backend(backend).is_some(),
        "the reported backend must be one the registry actually ships: {backend}"
    );
    assert_eq!(
        Some(backend),
        platform_sandbox_capability().backend_id().into(),
        "the helper and the parent must agree on which backend is in force"
    );
    match outcome {
        SandboxLaunchOutcome::Enforced {
            guarantees, limits, ..
        } => {
            assert!(
                !guarantees.is_empty(),
                "an enforced run must state what it actually guarantees"
            );
            assert!(
                !limits.is_empty(),
                "an enforced run must state its escapes; a backend with no \
                 limits is a backend nobody has audited"
            );
        }
        other => panic!("expected enforcement, got {other:?}"),
    }
}

// ── Workspace-write contract ─────────────────────────────────────────────

/// A directory that is genuinely outside every allowance: `$HOME` itself is
/// neither a writable root nor a tool cache, unlike `TMPDIR` (which is
/// deliberately writable and must be tested as such, not used as the
/// "outside" control).
fn outside_dir() -> tempfile::TempDir {
    let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME");
    tempfile::Builder::new()
        .prefix("codegg-sandbox-outside-")
        .tempdir_in(home)
        .expect("outside fixture")
}

#[test]
fn workspace_write_allows_workspace_and_refuses_everything_else() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let outside = outside_dir();
    let inside_file = workspace.path().join("inside.txt");
    let inside_new = workspace.path().join("new.txt");
    let outside_new = outside.path().join("escaped.txt");
    let probe_file = outside.path().join("probe.txt");
    std::fs::write(&inside_file, "inside").expect("inside fixture");
    std::fs::write(&probe_file, "seed").expect("seed file");

    let run = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
        &format!(
            "set -e\n\
             (exec 3> {}) && echo WRITE-INSIDE-OK || echo WRITE-INSIDE-DENIED\n\
             (exec 3< {}) && echo READ-INSIDE-OK || echo READ-INSIDE-DENIED\n\
             (exec 3> {}) 2>/dev/null && echo WRITE-OUTSIDE-LEAK || echo WRITE-OUTSIDE-DENIED\n\
             (exec 3< {}) && echo READ-OUTSIDE-LEAK || echo READ-OUTSIDE-DENIED\n\
             ",
            sh_quote(&inside_new),
            sh_quote(&inside_file),
            sh_quote(&outside_new),
            sh_quote(&probe_file),
        ),
    )
    .expect("workspace-write probe must run contained");

    let stdout = &run.stdout;
    assert!(
        stdout.contains("WRITE-INSIDE-OK"),
        "workspace writes must work: {stdout}"
    );
    assert!(
        stdout.contains("WRITE-OUTSIDE-DENIED"),
        "writes outside the workspace must be denied: {stdout}"
    );
    assert!(
        !outside_new.exists(),
        "a denied write must not create the file: {}",
        outside_new.display()
    );
    // Reads are deliberately broad (tool compatibility), so an outside read
    // succeeding is expected and pinned here to keep the trade-off visible.
    assert!(
        stdout.contains("READ-OUTSIDE-LEAK") || stdout.contains("READ-OUTSIDE-DENIED"),
        "the read probe must report a definite outcome: {stdout}"
    );
}

// ── Read-only contract ───────────────────────────────────────────────────

#[test]
fn read_only_denies_even_the_workspace() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let probe_file = workspace.path().join("existing.txt");
    let attempted = workspace.path().join("attempted.txt");
    std::fs::write(&probe_file, "seed").expect("seed file");

    let run = probe(
        workspace.path(),
        SandboxMode::ReadOnly,
        &[],
        &format!(
            "(exec 3> {}) 2>/dev/null && echo WORKSPACE-WRITE-LEAK || echo WORKSPACE-WRITE-DENIED\n\
             (exec 3< {}) && echo READ-OK || echo READ-DENIED\n",
            sh_quote(&attempted),
            sh_quote(&probe_file),
        ),
    )
    .expect("read-only probe must run contained");

    assert!(
        run.stdout.contains("WORKSPACE-WRITE-DENIED"),
        "read-only must deny writes even inside the workspace: {}",
        run.stdout
    );
    assert!(
        !attempted.exists(),
        "a denied write must not create the file: {}",
        attempted.display()
    );
    assert!(
        run.stdout.contains("READ-OK"),
        "read-only still permits reads: {}",
        run.stdout
    );
}

// ── Denied paths ─────────────────────────────────────────────────────────

#[test]
fn explicitly_denied_paths_are_unreachable() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let secret_dir = tempfile::tempdir().expect("secret dir");
    let secret = secret_dir.path().join("token");
    std::fs::write(&secret, "s3cret").expect("secret fixture");

    let run = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[secret_dir.path().to_path_buf()],
        &format!(
            "(exec 3< {}) && echo SECRET-LEAK || echo SECRET-DENIED\n",
            sh_quote(&secret)
        ),
    )
    .expect("deny-list probe must run contained");

    assert!(
        run.stdout.contains("SECRET-DENIED"),
        "a denied path must be unreadable: {}",
        run.stdout
    );
}

// ── Tool compatibility ───────────────────────────────────────────────────

#[test]
fn a_sandboxed_shell_can_still_read_the_system_and_resolve_a_command() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");

    // This is the assertion that keeps "containment" from becoming "denial":
    // a confined command must still start, read its own toolchain, and
    // consult system configuration.
    let run = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
        "(exec 3< /etc/hosts) && echo ETC-OK || echo ETC-DENIED\n\
         command -v ls >/dev/null && echo TOOLCHAIN-OK || echo TOOLCHAIN-DENIED\n\
         echo WORKSPACE-PWD=$(pwd)\n",
    )
    .expect("toolchain probe must run contained");

    assert!(
        run.stdout.contains("ETC-OK"),
        "system configuration must stay readable or DNS resolution breaks: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("TOOLCHAIN-OK"),
        "the toolchain must stay executable or no command runs at all: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("WORKSPACE-PWD="),
        "the probe must actually run: {}",
        run.stdout
    );
}

#[test]
fn scratch_space_is_writable_so_tooling_still_works() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    // TMPDIR is the scratch every compiler and test runner writes to, and it
    // is deliberately outside the workspace. Confined there on purpose: it
    // is scratch space, not host state.
    let scratch = tempfile::tempdir().expect("scratch dir");

    let run = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
        &format!(
            "(exec 3> {}) && echo SCRATCH-OK || echo SCRATCH-DENIED\n\
             (exec 3> /dev/null) && echo NULL-OK || echo NULL-DENIED\n\
             ls /etc >/dev/null 2>&1 && echo REDIRECT-OK || echo REDIRECT-DENIED\n",
            sh_quote(&scratch.path().join("tmpfile"))
        ),
    )
    .expect("scratch probe must run contained");

    // Scratch space is intentionally allowed: without it every test runner
    // and compiler that writes a temp fixture fails under containment.
    assert!(
        run.stdout.contains("SCRATCH-OK"),
        "temporary scratch space must stay writable: {}",
        run.stdout
    );
    // Stateless device nodes stay writable under every profile: without
    // /dev/null every `> /dev/null` and `2>&1` fails outright.
    assert!(
        run.stdout.contains("NULL-OK"),
        "/dev/null must stay writable: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("REDIRECT-OK"),
        "ordinary shell redirection must survive containment: {}",
        run.stdout
    );
}

#[test]
fn read_only_still_permits_shell_redirection() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    // ReadOnly denies workspace writes, but denying `/dev/null` too would
    // break redirection without protecting anything.
    let run = probe(
        workspace.path(),
        SandboxMode::ReadOnly,
        &[],
        "ls /etc >/dev/null 2>&1 && echo REDIRECT-OK || echo REDIRECT-DENIED\n\
         (exec 3> /dev/null) && echo NULL-OK || echo NULL-DENIED\n",
    )
    .expect("redirection probe must run contained");

    assert!(
        run.stdout.contains("NULL-OK"),
        "/dev/null must be writable even under read-only: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("REDIRECT-OK"),
        "read-only must not break shell redirection: {}",
        run.stdout
    );
}

// ── Fail-closed ──────────────────────────────────────────────────────────

#[test]
fn a_malformed_spec_never_produces_an_enforced_status() {
    require_backend!();
    let spec_file = tempfile::NamedTempFile::new().expect("spec file");
    // Not JSON at all: the helper must fail closed, never report a
    // containment it did not establish.
    std::fs::write(spec_file.path(), b"{not json").expect("malformed spec");

    let run = launch_helper(helper_path(), spec_file.path());
    let status = run.status;

    assert!(
        !status.is_empty(),
        "a malformed spec must still report a terminal reason, not vanish"
    );
    let outcome = decode_sandbox_status(&status).expect("terminal frame must decode");
    assert!(
        outcome.backend().is_none(),
        "a malformed spec must never report enforcement: {outcome:?}"
    );
    assert!(
        !matches!(outcome, SandboxLaunchOutcome::Enforced { .. }),
        "fail-closed: setup failure must not read as containment: {outcome:?}"
    );
}
