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
#[cfg(target_os = "linux")]
use std::io::Write as _;
#[cfg(target_os = "linux")]
use std::net::TcpListener;
use std::os::fd::AsRawFd;
#[cfg(target_os = "macos")]
use std::os::fd::RawFd;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::unix::net::{SocketAddr as UnixSocketAddr, UnixListener};
use std::os::unix::process::CommandExt;
#[cfg(target_os = "linux")]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    stderr: String,
    success: bool,
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

/// Launch a helper with explicit environment overrides for the target.
///
/// Some developer tools read a credential file unconditionally, even for
/// `--version`. That file is a deliberate deny root, so a probe meant to
/// exercise tool *availability* must point the tool at an empty config
/// directory instead of the operator's real one.
///
/// Gated with its only caller: the Seatbelt developer-tool probe, so the
/// function is not dead code on hosts that compile that probe out.
#[cfg(target_os = "macos")]
fn run_helper_with_env(spec: &SandboxLaunchSpec, overrides: &[(&str, &str)]) -> Run {
    let spec_file = tempfile::NamedTempFile::new().expect("sandbox spec file");
    serde_json::to_writer(spec_file.as_file(), spec).expect("sandbox spec encoding");

    launch_helper_with_env(helper_path(), spec_file.path(), overrides)
}

#[cfg(target_os = "macos")]
fn launch_helper_with_env(helper: PathBuf, spec: &Path, overrides: &[(&str, &str)]) -> Run {
    launch_helper_impl(helper, spec, overrides)
}

/// Fork/exec the helper with its status channel wired to `SANDBOX_STATUS_FD`.
fn launch_helper(helper: PathBuf, spec: &Path) -> Run {
    launch_helper_impl(helper, spec, &[])
}

fn launch_helper_impl(helper: PathBuf, spec: &Path, overrides: &[(&str, &str)]) -> Run {
    let (reader, writer) = nix::unistd::pipe().expect("status pipe");
    let writer_fd = writer.as_raw_fd();
    let mut command = Command::new(helper);
    command
        .arg("--spec")
        .arg(spec)
        .arg("--status-fd")
        .arg(SANDBOX_STATUS_FD.to_string());
    for (name, value) in overrides {
        command.env(name, value);
    }
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
    let Output {
        stdout,
        stderr,
        status: exit_status,
    } = command.output().expect("sandbox helper process");
    // Drop the parent's copy of the write end so the reader sees EOF. This
    // is what lets an `Enforced`-then-EOF stream decode as a clean success.
    drop(writer);
    let mut status = Vec::new();
    std::fs::File::from(reader)
        .read_to_end(&mut status)
        .expect("sandbox status frame");
    Run {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        success: exit_status.success(),
        status,
    }
}

/// Launch a helper while deliberately preserving one already-open descriptor
/// across the sandbox application and target exec. Seatbelt's status report
/// calls this out as a limit; this fixture proves the behavior without
/// touching any real user data.
#[cfg(target_os = "macos")]
fn launch_helper_with_inherited_fd(
    helper: PathBuf,
    spec: &SandboxLaunchSpec,
    source_fd: RawFd,
) -> Run {
    const INHERITED_FD: RawFd = 42;
    let spec_file = tempfile::NamedTempFile::new().expect("sandbox spec file");
    serde_json::to_writer(spec_file.as_file(), spec).expect("sandbox spec encoding");

    let (reader, writer) = nix::unistd::pipe().expect("status pipe");
    let writer_fd = writer.as_raw_fd();
    let mut command = Command::new(helper);
    command
        .arg("--spec")
        .arg(spec_file.path())
        .arg("--status-fd")
        .arg(SANDBOX_STATUS_FD.to_string());
    // SAFETY: pre_exec only duplicates two already-open descriptors and
    // changes close-on-exec flags. It allocates nothing and runs no user code.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(source_fd, INHERITED_FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(INHERITED_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if writer_fd != SANDBOX_STATUS_FD {
                if libc::dup2(writer_fd, SANDBOX_STATUS_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(writer_fd);
            }
            if libc::fcntl(SANDBOX_STATUS_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let Output {
        stdout,
        stderr,
        status: exit_status,
    } = command.output().expect("sandbox helper process");
    drop(writer);
    let mut status = Vec::new();
    std::fs::File::from(reader)
        .read_to_end(&mut status)
        .expect("sandbox status frame");
    Run {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        success: exit_status.success(),
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
    if run.backend().is_none() {
        eprintln!(
            "sandbox qualification: helper did not enforce: {:?}",
            run.outcome()
        );
        None
    } else {
        Some(run)
    }
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

#[test]
fn denied_credential_nested_under_workspace_is_not_read_or_written() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let credentials = workspace.path().join(".config/gh/hosts.yml");
    std::fs::create_dir_all(credentials.parent().expect("credential parent"))
        .expect("credential directory");
    std::fs::write(&credentials, "token: secret-value\n").expect("credential fixture");
    let alias = workspace.path().join("credential-alias");
    std::os::unix::fs::symlink(&credentials, &alias).expect("credential symlink alias");

    let read = probe(
        workspace.path(),
        SandboxMode::ReadOnly,
        std::slice::from_ref(&credentials),
        &format!("cat {}; cat {}", sh_quote(&credentials), sh_quote(&alias)),
    )
    .expect("backend enforced containment");
    assert!(
        !read.stdout.contains("secret-value"),
        "a workspace allowance or symlink alias must not expose a nested credential: {:?}",
        read.stdout
    );
    #[cfg(target_os = "macos")]
    assert_eq!(read.backend(), Some(BackendId::SEATBELT));

    let marker = workspace.path().join("target-ran");
    let mut write_spec = spec_for(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        std::slice::from_ref(&credentials),
    );
    write_spec.args = vec![
        "-c".to_string(),
        format!(
            "printf changed > {}; touch {}",
            sh_quote(&credentials),
            sh_quote(&marker)
        ),
    ];
    let write = run_helper(&write_spec);
    assert_eq!(
        std::fs::read_to_string(credentials).expect("credential remains readable to fixture"),
        "token: secret-value\n",
        "a broad writable workspace root must not override a credential deny; output={:?}",
        write.stdout
    );
    if write.backend().is_none() {
        assert!(
            !marker.exists(),
            "unrepresentable Landlock policy must refuse launch"
        );
    }
}

#[test]
fn missing_deny_path_remains_denied_if_created_before_helper_apply() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let future = workspace.path().join(".config/codegg/new-secret");
    let mut spec = spec_for(
        workspace.path(),
        SandboxMode::ReadOnly,
        std::slice::from_ref(&future),
    );
    std::fs::create_dir_all(future.parent().expect("future deny parent"))
        .expect("create future deny parent after policy preparation");
    std::fs::write(&future, "future-secret").expect("future denied fixture");
    spec.args = vec!["-c".to_string(), format!("cat {}", sh_quote(&future))];

    let run = run_helper(&spec);
    assert!(
        !run.stdout.contains("future-secret"),
        "a deny absent during launch-spec preparation must still apply: {:?}",
        run.stdout
    );
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
    eprintln!("sandbox qualification: helper outcome {outcome:?}");
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
            backend,
            abi,
            guarantees,
            limits,
        } => {
            if backend == BackendId::LANDLOCK {
                eprintln!("sandbox qualification: kernel Landlock ABI {abi:?}");
                assert!(
                    abi.is_some_and(|abi| abi >= 3),
                    "Landlock must enforce ABI 3+: {abi:?}"
                );
                assert!(
                    guarantees
                        .iter()
                        .any(|value| value.contains("no_new_privs")),
                    "Landlock status must name the privilege drop: {guarantees:?}"
                );
            }
            assert!(
                !guarantees.is_empty(),
                "an enforced run must state what it actually guarantees"
            );
            assert!(
                !limits.is_empty(),
                "an enforced run must state its escapes; a backend with no \
                 limits is a backend nobody has audited"
            );
            if backend == BackendId::SEATBELT {
                assert_eq!(abi, None, "Seatbelt has no versioned ABI");
                for reported in [
                    "no network isolation",
                    "no_new_privs equivalent",
                    "open file descriptors",
                    "Apple Events and Launch Services",
                    "reads are broadly allowed",
                ] {
                    assert!(
                        limits.iter().any(|limit| limit.contains(reported)),
                        "Seatbelt must report {reported:?}: {limits:?}"
                    );
                }
            }
        }
        other => panic!("expected enforcement, got {other:?}"),
    }
}

/// Command spelling does not determine filesystem authority. Exercise one
/// outside-root write through shell text, Python, and an independently
/// compiled executable under the same production WorkspaceWrite spec.
#[cfg(target_os = "linux")]
#[test]
fn landlock_denies_equivalent_outside_write_from_shell_python_and_native_binary() {
    require_backend!();
    let temp = tempfile::tempdir().expect("fixture root");
    let workspace = temp.path().join("workspace");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&workspace).expect("workspace directory");
    std::fs::create_dir_all(&outside).expect("outside directory");

    let shell_marker = outside.join("shell-marker");
    let mut shell = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    shell.args = vec![
        "-c".to_string(),
        format!("printf blocked > {}", sh_quote(&shell_marker)),
    ];
    let shell_run = run_helper(&shell);

    let wrapped_marker = outside.join("wrapped-shell-marker");
    let mut wrapped_shell = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    let wrapped_command = format!("printf blocked > {}", sh_quote(&wrapped_marker));
    wrapped_shell.args = vec![
        "-c".to_string(),
        format!("sh -c {}", sh_quote(Path::new(&wrapped_command))),
    ];
    let wrapped_shell_run = run_helper(&wrapped_shell);

    let script_marker = outside.join("script-marker");
    let script = workspace.join("attempt-outside-write.sh");
    std::fs::write(
        &script,
        format!("printf blocked > {}\n", sh_quote(&script_marker)),
    )
    .expect("generated shell script");
    let mut generated_script = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    generated_script.args = vec![script.to_string_lossy().into_owned()];
    let generated_script_run = run_helper(&generated_script);

    let python_marker = outside.join("python-marker");
    let mut python = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    python.target = PathBuf::from("/usr/bin/python3");
    python.args = vec![
        "-c".to_string(),
        "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('blocked')".to_string(),
        python_marker.to_string_lossy().into_owned(),
    ];
    let python_run = run_helper(&python);

    let source = workspace.join("outside_write.c");
    let native_binary = workspace.join("outside_write");
    std::fs::write(
        &source,
        "#include <stdio.h>\nint main(int c,char**v){FILE*f=fopen(v[1],\"w\");if(!f)return 1;fputs(\"blocked\",f);fclose(f);return 0;}\n",
    )
    .expect("native fixture source");
    let compile = Command::new("/usr/bin/cc")
        .arg(&source)
        .arg("-o")
        .arg(&native_binary)
        .output()
        .expect("native fixture compiler");
    assert!(
        compile.status.success(),
        "native fixture compilation failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let native_marker = outside.join("native-marker");
    let mut native = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    native.target = native_binary;
    native.args = vec![native_marker.to_string_lossy().into_owned()];
    let native_run = run_helper(&native);

    // A planted local hook is an alternate executable route through Git.
    // Prepare a repository before confinement, then let the hook attempt the
    // same outside-root write under the exact same WorkspaceWrite spec.
    let git_workspace = workspace.join("git-repo");
    std::fs::create_dir_all(git_workspace.join(".git/hooks")).expect("git hooks directory");
    let git_setup = |args: &[&str]| {
        let result = Command::new("git")
            .arg("-C")
            .arg(&git_workspace)
            .args(args)
            .output()
            .expect("git fixture setup");
        assert!(
            result.status.success(),
            "git fixture setup failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    git_setup(&["init", "-q"]);
    git_setup(&["config", "user.name", "Sandbox Fixture"]);
    git_setup(&["config", "user.email", "sandbox@example.invalid"]);
    std::fs::write(git_workspace.join("tracked.txt"), "fixture\n").expect("git file");
    git_setup(&["add", "tracked.txt"]);
    let git_marker = outside.join("git-hook-marker");
    let hook = git_workspace.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\nprintf blocked > {}\n", sh_quote(&git_marker)),
    )
    .expect("git pre-commit hook");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("executable hook");
    let mut git_commit = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    git_commit.target = PathBuf::from("git");
    git_commit.args = vec![
        "-C".into(),
        git_workspace.to_string_lossy().into_owned(),
        "commit".into(),
        "-m".into(),
        "fixture".into(),
    ];
    let git_run = run_helper(&git_commit);

    for (surface, run) in [
        ("shell", &shell_run),
        ("wrapped shell", &wrapped_shell_run),
        ("generated script", &generated_script_run),
        ("python", &python_run),
        ("native", &native_run),
        ("Git hook", &git_run),
    ] {
        let outcome = run
            .outcome()
            .unwrap_or_else(|| panic!("{surface} did not report an enforced launch"));
        assert!(
            matches!(outcome, SandboxLaunchOutcome::Enforced { .. }),
            "{surface} did not obtain enforcement: {outcome:?}"
        );
        assert!(
            outcome.backend().is_some(),
            "{surface} did not report its backend"
        );
        assert!(
            !run.success,
            "{surface} unexpectedly wrote outside the workspace"
        );
    }
    assert!(!shell_marker.exists(), "shell wrote an outside marker");
    assert!(
        !wrapped_marker.exists(),
        "wrapped shell wrote an outside marker"
    );
    assert!(
        !script_marker.exists(),
        "generated script wrote an outside marker"
    );
    assert!(!python_marker.exists(), "Python wrote an outside marker");
    assert!(
        !native_marker.exists(),
        "native executable wrote an outside marker"
    );
    assert!(!git_marker.exists(), "Git hook wrote an outside marker");
}

/// Landlock ABI 3 added `TRUNCATE`; ordinary write denial is not enough to
/// prove read-only and workspace boundaries protect existing file contents.
#[cfg(target_os = "linux")]
#[test]
fn landlock_truncation_is_confined_to_workspace_write_roots() {
    require_backend!();
    assert!(Command::new("truncate")
        .arg("--version")
        .output()
        .expect("GNU truncate is required for the truncation syscall probe")
        .status
        .success());
    assert!(Command::new("python3")
        .arg("--version")
        .output()
        .expect("python3 is required for the O_TRUNC probe")
        .status
        .success());
    let workspace = tempfile::tempdir().expect("workspace");
    let outside = outside_dir();
    let inside_file = workspace.path().join("inside.txt");
    let outside_file = outside.path().join("outside.txt");
    std::fs::write(&inside_file, "inside-seed").expect("inside fixture");
    std::fs::write(&outside_file, "outside-seed").expect("outside fixture");

    let read_only = probe(
        workspace.path(),
        SandboxMode::ReadOnly,
        &[],
        &format!(
            "(: > {} 2>/dev/null) && echo READONLY-TRUNCATED || echo READONLY-DENIED",
            sh_quote(&inside_file),
        ),
    )
    .expect("read-only truncation probe must run contained");
    assert!(read_only.stdout.contains("READONLY-DENIED"));
    assert_eq!(
        std::fs::read_to_string(&inside_file).unwrap(),
        "inside-seed"
    );

    let workspace_write = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
        &format!(
            "(: > {} 2>/dev/null) && echo INSIDE-TRUNCATED || echo INSIDE-DENIED\n\
             (: > {} 2>/dev/null) && echo OUTSIDE-TRUNCATED || echo OUTSIDE-DENIED\n\
             truncate -s 0 {} 2>/dev/null && echo TRUNCATE-OUTSIDE-LEAK || echo TRUNCATE-OUTSIDE-DENIED\n\
             python3 -c 'open(\"{}\", \"w\").close()' 2>/dev/null && echo PYTHON-OTRUNC-LEAK || echo PYTHON-OTRUNC-DENIED\n\
             sh -c ': > {}' 2>/dev/null && echo CHILD-TRUNCATE-LEAK || echo CHILD-TRUNCATE-DENIED",
            sh_quote(&inside_file),
            sh_quote(&outside_file),
            sh_quote(&outside_file),
            outside_file.display(),
            sh_quote(&outside_file),
        ),
    )
    .expect("workspace-write truncation probe must run contained");
    assert!(workspace_write.stdout.contains("INSIDE-TRUNCATED"));
    assert!(workspace_write.stdout.contains("OUTSIDE-DENIED"));
    assert!(workspace_write.stdout.contains("TRUNCATE-OUTSIDE-DENIED"));
    assert!(workspace_write.stdout.contains("PYTHON-OTRUNC-DENIED"));
    assert!(workspace_write.stdout.contains("CHILD-TRUNCATE-DENIED"));
    assert_eq!(std::fs::read_to_string(&inside_file).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(&outside_file).unwrap(),
        "outside-seed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn landlock_blocks_rename_and_link_from_workspace_into_outside_roots() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let outside = outside_dir();
    let rename_source = workspace.path().join("rename-source.txt");
    let rename_target = outside.path().join("rename-target.txt");
    let link_source = workspace.path().join("link-source.txt");
    let link_target = outside.path().join("link-target.txt");
    std::fs::write(&rename_source, "rename-seed").expect("rename fixture");
    std::fs::write(&link_source, "link-seed").expect("link fixture");

    let run = probe(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[],
        &format!(
            "mv {} {} 2>/dev/null && echo RENAME-OUTSIDE-LEAK || echo RENAME-OUTSIDE-DENIED\n\
             ln {} {} 2>/dev/null && echo LINK-OUTSIDE-LEAK || echo LINK-OUTSIDE-DENIED",
            sh_quote(&rename_source),
            sh_quote(&rename_target),
            sh_quote(&link_source),
            sh_quote(&link_target),
        ),
    )
    .expect("rename/link probe must run contained");

    assert!(run.stdout.contains("RENAME-OUTSIDE-DENIED"));
    assert!(run.stdout.contains("LINK-OUTSIDE-DENIED"));
    assert_eq!(
        std::fs::read_to_string(&rename_source).unwrap(),
        "rename-seed"
    );
    assert_eq!(std::fs::read_to_string(&link_source).unwrap(), "link-seed");
    assert!(!rename_target.exists());
    assert!(!link_target.exists());
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
        match run.backend() {
            Some(BackendId::SEATBELT) => stdout.contains("READ-OUTSIDE-LEAK"),
            Some(BackendId::LANDLOCK) => stdout.contains("READ-OUTSIDE-DENIED"),
            _ => false,
        },
        "outside read outcome must match the reported backend ({:?}): {stdout}",
        run.backend()
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
    // The secret lives under HOME, outside the curated read roots. Its
    // explicit deny is retained on allow-default Seatbelt as well.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("HOME must exist for deny test");
    let secret_dir = tempfile::Builder::new()
        .prefix("codegg-deny-test")
        .tempdir_in(&home)
        .expect("secret dir");
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

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_preserves_and_reports_preopened_denied_file_descriptors() {
    require_backend!();
    assert_eq!(
        platform_sandbox_capability().backend_id(),
        BackendId::SEATBELT
    );

    let workspace = tempfile::tempdir().expect("workspace");
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("HOME must exist for descriptor fixture");
    let outside = tempfile::Builder::new()
        .prefix("codegg-preopened-secret-")
        .tempdir_in(home)
        .expect("outside fixture");
    let secret = outside.path().join("token");
    std::fs::write(&secret, "preopened-secret-sentinel").expect("secret fixture");
    let opened = std::fs::File::open(&secret).expect("pre-open denied file");

    let mut spec = spec_for(
        workspace.path(),
        SandboxMode::WorkspaceWrite,
        &[outside.path().to_path_buf()],
    );
    spec.args = vec!["-c".to_string(), "cat <&42".to_string()];
    let run = launch_helper_with_inherited_fd(helper_path(), &spec, opened.as_raw_fd());
    assert!(run.success, "target failed: {}", run.stderr);
    assert!(run.stdout.contains("preopened-secret-sentinel"));
    match run.outcome().expect("helper status frame") {
        SandboxLaunchOutcome::Enforced {
            backend, limits, ..
        } => {
            assert_eq!(backend, BackendId::SEATBELT);
            assert!(limits
                .iter()
                .any(|limit| limit.contains("open file descriptors")));
        }
        other => panic!("expected Seatbelt enforcement, got {other:?}"),
    }
}

/// Seatbelt's filesystem profile deliberately leaves network operations open.
/// Prove that limit with a loopback-only fixture and require the status frame
/// to disclose it; no external host or service is contacted.
#[cfg(target_os = "macos")]
#[test]
fn seatbelt_loopback_is_reachable_and_reported_as_unrestricted() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("loopback fixture listener");
    listener
        .set_nonblocking(true)
        .expect("bound the fixture wait");
    let port = listener.local_addr().expect("listener address").port();
    let accept = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("loopback probe did not connect: {error}"),
            }
        };
        stream
            .set_nonblocking(false)
            .expect("accepted fixture socket is blocking");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("bound the fixture read");
        let mut byte = [0];
        std::io::Read::read_exact(&mut stream, &mut byte).expect("loopback probe byte");
        byte[0]
    });

    let config = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::ReadOnly)
        .with_allowed_paths(vec![workspace.path().to_string_lossy().into_owned()]);
    let mut spec = config
        .launch_spec("python3", &[], Some(workspace.path()))
        .expect("Python loopback launch spec");
    spec.args = vec![
        "-c".to_string(),
        format!(
            "import socket; s=socket.create_connection(('127.0.0.1',{port}), timeout=5); s.sendall(b'x'); s.close(); print('LOOPBACK-REACHED')"
        ),
    ];
    let run = run_helper(&spec);
    assert!(run.success, "loopback probe failed: {}", run.stderr);
    assert!(run.stdout.contains("LOOPBACK-REACHED"), "{}", run.stdout);
    assert_eq!(accept.join().expect("listener thread"), b'x');
    match run.outcome().expect("helper status frame") {
        SandboxLaunchOutcome::Enforced {
            backend, limits, ..
        } => {
            assert_eq!(backend, BackendId::SEATBELT);
            assert!(limits
                .iter()
                .any(|limit| limit.contains("no network isolation")));
        }
        other => panic!("expected Seatbelt enforcement, got {other:?}"),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn rejected_seatbelt_profile_does_not_execute_the_target() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let marker = workspace.path().join("seatbelt-target-ran");
    let mut spec = spec_for(workspace.path(), SandboxMode::WorkspaceWrite, &[]);
    spec.args = vec!["-c".to_string(), format!("touch {}", sh_quote(&marker))];
    // Preserve a NUL-bearing descendant beneath an existing policy root so it
    // reaches the generated profile. sandbox_init's C-string API must reject
    // it before exec, with no fallback to the unsandboxed shell.
    spec.deny_paths
        .push(workspace.path().join("invalid\0profile-path"));
    let run = run_helper(&spec);
    assert!(!marker.exists(), "target ran after invalid Seatbelt setup");
    assert!(matches!(
        run.outcome(),
        Some(SandboxLaunchOutcome::SetupError { .. })
    ));
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
fn sandboxed_cargo_metadata_uses_workspace_and_scoped_scratch() {
    require_backend!();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("workspace root");
    let scratch = tempfile::tempdir().expect("private scratch");
    let cargo_home = scratch.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).expect("scoped Cargo home");
    std::fs::write(cargo_home.join("config.toml"), "").expect("empty scoped Cargo config");
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".rustup")))
        .expect("Rustup home");
    let command = format!(
        "cd {} && HOME={} CARGO_HOME={} RUSTUP_HOME={} TMPDIR={} cargo metadata --no-deps --offline --format-version 1 --manifest-path {}/Cargo.toml >/dev/null",
        sh_quote(scratch.path()),
        sh_quote(&cargo_home),
        sh_quote(&cargo_home),
        sh_quote(&rustup_home),
        sh_quote(scratch.path()),
        sh_quote(&workspace),
    );
    let mut spec = spec_for(&workspace, SandboxMode::WorkspaceWrite, &[]);
    spec.read_paths.push(scratch.path().to_path_buf());
    spec.write_paths.push(scratch.path().to_path_buf());
    spec.args = vec!["-c".to_string(), command];

    let run = run_helper(&spec);
    assert!(
        run.success,
        "offline Cargo metadata should work with read-only tool inputs and scoped scratch: {}\n{}",
        run.stdout, run.stderr
    );
}

#[test]
fn scratch_space_is_writable_so_tooling_still_works() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    // This explicit scratch root stands in for ManagedProcessService's
    // per-launch directory. The sibling must stay inaccessible.
    let scratch = tempfile::tempdir().expect("scratch dir");
    let other_scratch = tempfile::tempdir().expect("another session scratch dir");
    let scratch_file = scratch.path().join("tmpfile");
    let other_file = other_scratch.path().join("cross-session-write");

    let mut spec = spec_for(workspace.path(), SandboxMode::WorkspaceWrite, &[]);
    spec.write_paths.push(scratch.path().to_path_buf());
    spec.args = vec![
        "-c".to_string(),
        format!(
            "(exec 3> {}) && echo SCRATCH-OK || echo SCRATCH-DENIED\n\
             (echo forbidden > {}) && echo OTHER-SCRATCH-WRITABLE || echo OTHER-SCRATCH-DENIED\n\
             (exec 3> /dev/null) && echo NULL-OK || echo NULL-DENIED\n\
             ls /etc >/dev/null 2>&1 && echo REDIRECT-OK || echo REDIRECT-DENIED\n",
            sh_quote(&scratch_file),
            sh_quote(&other_file)
        ),
    ];
    let run = run_helper(&spec);

    // Scratch space is intentionally allowed: without it every test runner
    // and compiler that writes a temp fixture fails under containment.
    assert!(
        run.stdout.contains("SCRATCH-OK"),
        "temporary scratch space must stay writable: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("OTHER-SCRATCH-DENIED") && !other_file.exists(),
        "a launch must not write another session's scratch: {}",
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

/// Seatbelt currently keeps broad read access for developer-tool compatibility.
/// Exercise representative installed tools through the real helper without
/// allowing any persistent writes or contacting external services.
#[cfg(target_os = "macos")]
#[test]
fn seatbelt_keeps_read_only_developer_tool_probes_operational() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let tools: [(&str, &[&str]); 6] = [
        ("git", &["--version"]),
        ("cargo", &["--version"]),
        ("rustc", &["--version"]),
        ("python3", &["--version"]),
        ("brew", &["--version"]),
        ("gh", &["--version"]),
    ];

    for (program, args) in tools {
        let mut spec = spec_for(workspace.path(), SandboxMode::ReadOnly, &[]);
        spec.target = PathBuf::from(program);
        spec.args = args.iter().map(|arg| (*arg).to_string()).collect();
        // `gh` opens its credential store before it dispatches a command, so
        // a bare `--version` fails on any host where the operator is signed
        // in. That store (`~/.config/gh/hosts.yml`) is a deliberate deny
        // root; pointing the probe at an empty config directory keeps this a
        // tool-availability check rather than a credential-access check.
        let run = if program == "gh" {
            let config = tempfile::tempdir().expect("gh config directory");
            run_helper_with_env(
                &spec,
                &[("GH_CONFIG_DIR", config.path().to_str().expect("utf-8 path"))],
            )
        } else {
            run_helper(&spec)
        };
        assert!(
            run.success,
            "read-only {program} probe failed: {}\n{}",
            run.stdout, run.stderr
        );
        assert!(
            run.outcome()
                .is_some_and(|outcome| outcome.backend() == Some(BackendId::SEATBELT)),
            "{program} probe did not report Seatbelt: {:?}",
            run.outcome()
        );
    }
}

#[test]
fn read_only_still_permits_shell_redirection() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    // ReadOnly denies workspace writes, but denying `/dev/null` too would
    // break redirection without protecting anything.
    let mut spec = spec_for(workspace.path(), SandboxMode::ReadOnly, &[]);
    spec.args = vec![
        "-c".to_string(),
        "ls /etc >/dev/null 2>&1 && echo REDIRECT-OK || echo REDIRECT-DENIED\n\
         (exec 3> /dev/null) && echo NULL-OK || echo NULL-DENIED\n"
            .to_string(),
    ];
    let run = run_helper(&spec);

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

/// Landlock is intentionally filesystem-only. This fixture connects a
/// sandboxed child to same-UID host process, loopback TCP, and abstract UDS
/// endpoints so later namespace work has real, deterministic baseline
/// effects to deny.
#[cfg(target_os = "linux")]
#[test]
fn legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable() {
    require_backend!();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut host_process = Command::new("sleep")
        .arg("30")
        .env("CODEGG_C007_SENTINEL", "fixture-only")
        .spawn()
        .expect("controlled same-UID fixture process");
    let host_pid = host_process.id();

    let tcp = TcpListener::bind(("127.0.0.1", 0)).expect("loopback TCP fixture");
    tcp.set_nonblocking(true).unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    let tcp_server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match tcp.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "TCP fixture accept timed out");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("TCP fixture accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 1];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"T");
        stream.write_all(b"A").unwrap();
    });

    let socket_name = format!(
        "codegg-c007-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    );
    let uds_addr =
        UnixSocketAddr::from_abstract_name(socket_name.as_bytes()).expect("abstract UDS address");
    let uds = UnixListener::bind_addr(&uds_addr).expect("abstract UDS fixture");
    uds.set_nonblocking(true).unwrap();
    let uds_server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match uds.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "UDS fixture accept timed out");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("UDS fixture accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 1];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"U");
        stream.write_all(b"B").unwrap();
    });

    let script = format!(
        r#"import os, signal, socket
try:
    open('/proc/{host_pid}/environ', 'rb').read()
    proc_result = 'HOST_PROC_READ'
except PermissionError:
    proc_result = 'HOST_PROC_DENIED_BY_HOST'
os.kill({host_pid}, 0)
os.kill({host_pid}, signal.SIGTERM)
tcp = socket.create_connection(('127.0.0.1', {tcp_port}), timeout=3)
tcp.sendall(b'T')
assert tcp.recv(1) == b'A'
uds = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
uds.settimeout(3)
uds.connect('\0{socket_name}')
uds.sendall(b'U')
assert uds.recv(1) == b'B'
print(proc_result, 'HOST_PID_VISIBLE HOST_SIGNAL_OK TCP_LOOPBACK_OK ABSTRACT_UDS_OK')"#,
        tcp_port = tcp_addr.port()
    );
    let mut spec = spec_for(workspace.path(), SandboxMode::WorkspaceWrite, &[]);
    spec.target = PathBuf::from("python3");
    spec.args = vec!["-c".to_string(), script];
    let run = run_helper(&spec);
    let tcp_result = tcp_server.join();
    let uds_result = uds_server.join();
    let host_status = match host_process.try_wait().expect("poll fixture process") {
        Some(status) => status,
        None => {
            host_process.kill().expect("clean up fixture process");
            host_process.wait().expect("reap fixture process")
        }
    };

    assert!(run.success, "legacy fixture failed: {}", run.stderr);
    assert!(tcp_result.is_ok());
    assert!(uds_result.is_ok());
    assert!(
        host_status.signal().is_some(),
        "target did not signal fixture process"
    );
    assert!(
        run.stdout
            .contains("HOST_PROC_DENIED_BY_HOST HOST_PID_VISIBLE HOST_SIGNAL_OK TCP_LOOPBACK_OK ABSTRACT_UDS_OK"),
        "legacy Landlock effects changed: {}",
        run.stdout
    );
    match run.outcome().expect("helper status frame") {
        SandboxLaunchOutcome::Enforced {
            backend, limits, ..
        } => {
            assert_eq!(backend, BackendId::LANDLOCK);
            assert!(limits
                .iter()
                .any(|limit| limit.contains("no network isolation")));
        }
        other => panic!("expected the legacy filesystem-only status, got {other:?}"),
    }
}
