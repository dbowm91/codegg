//! Hardened, shell-free Git process construction.
//!
//! This is the generic Git execution boundary. It owns the environment
//! policy and the process plumbing shared by structured read operations and
//! callers that need a synchronous Git command. It does not know about
//! CodeGG sessions, runs, jobs, permissions, or projections.

use crate::EgggitError;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Output as StdOutput, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Default wall-clock bound for a single structured Git read.
///
/// `run` executes a synchronous child inside `spawn_blocking`, and a
/// blocking task cannot be cancelled: wrapping the call in
/// `tokio::time::timeout` would only abandon the task while the Git process
/// kept running and kept holding a blocking thread. The deadline therefore
/// has to be enforced around the child *inside* the blocking closure, where
/// exceeding it kills the process. 30s matches `GitExecutionService`'s
/// default so both halves of a `git_query` call share one budget.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll interval while waiting on the child inside [`run_command`].
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Environment variables restored for local, non-interactive Git commands.
pub const ALLOWED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "LANG",
    "LC_ALL",
    "LC_MESSAGES",
    "TZ",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "LANGUAGE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "GIT_SSL_CAPATH",
];

/// Command-bearing variables never inherited by a CodeGG-owned local Git
/// process. Network callers may explicitly overlay a reviewed allowlist, but
/// these hard-deny entries are always removed afterwards.
pub const ALWAYS_STRIPPED_ENV_VARS: &[&str] = &[
    "GIT_ASKPASS",
    "GIT_SSH_COMMAND",
    "GIT_SSH_VARIANT",
    "GIT_PROXY_COMMAND",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_KEY_1",
    "GIT_CONFIG_KEY_2",
    "GIT_CONFIG_KEY_3",
    "GIT_CONFIG_KEY_4",
    "GIT_CONFIG_KEY_5",
    "GIT_CONFIG_VALUE_0",
    "GIT_CONFIG_VALUE_1",
    "GIT_CONFIG_VALUE_2",
    "GIT_CONFIG_VALUE_3",
    "GIT_CONFIG_VALUE_4",
    "GIT_CONFIG_VALUE_5",
    "GIT_CONFIG_PARAMETERS",
    "SSH_ASKPASS",
    "GIT_TOOL",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_PAGER",
    "PAGER",
];

/// Executable authority that a Git request may obtain from repository data or
/// its transport. Callers use these facts to select a real contained backend;
/// the list is never treated as a reason to manufacture a process error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitExecutionCapability {
    RepositoryAlias,
    CleanSmudgeFilter,
    MergeDriver,
    Hook,
    ExternalDiffOrTextconv,
    RemoteHelper,
    Network,
    Credential,
    DestructiveRepositoryChange,
}

/// Policy classification for one shell-free Git argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitExecutionPolicyOutcome {
    AllowedTrustedNonExecutable,
    RequiresContainedExecution {
        capabilities: Vec<GitExecutionCapability>,
    },
    RequiresAdditionalAuthorization {
        capabilities: Vec<GitExecutionCapability>,
    },
    DeniedExplicitAuthority {
        reason: String,
    },
    InvalidRequest {
        reason: String,
    },
    BackendUnavailable {
        capabilities: Vec<GitExecutionCapability>,
    },
}

impl std::fmt::Display for GitExecutionPolicyOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllowedTrustedNonExecutable => formatter.write_str("trusted non-executable Git"),
            Self::RequiresContainedExecution { capabilities } => {
                write!(
                    formatter,
                    "contained Git execution required: {capabilities:?}"
                )
            }
            Self::RequiresAdditionalAuthorization { capabilities } => {
                write!(
                    formatter,
                    "additional Git authorization required: {capabilities:?}"
                )
            }
            Self::DeniedExplicitAuthority { reason } => write!(formatter, "Git denied: {reason}"),
            Self::InvalidRequest { reason } => write!(formatter, "invalid Git request: {reason}"),
            Self::BackendUnavailable { capabilities } => {
                write!(formatter, "Git backend unavailable for {capabilities:?}")
            }
        }
    }
}

impl std::error::Error for GitExecutionPolicyOutcome {}

/// Base policy for local Git operations.
#[derive(Debug, Clone)]
pub struct GitEnvPolicy {
    pub terminal_prompt_disabled: bool,
    pub pin_editor: bool,
    pub strip_editors: bool,
    pub strip_command_bearers: bool,
}

/// A trusted Git command plus any private read-only object view it needs.
/// Keeping the view beside the command ties its lifetime to spawn/wait and
/// prevents repository alternates created after admission from affecting the
/// command.
#[derive(Debug)]
pub struct GitAsyncCommand {
    command: Command,
    _object_view: Option<PrivateObjectView>,
}

impl Deref for GitAsyncCommand {
    type Target = Command;

    fn deref(&self) -> &Self::Target {
        &self.command
    }
}

impl DerefMut for GitAsyncCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.command
    }
}

#[derive(Debug)]
pub struct GitSyncCommand {
    command: StdCommand,
    _object_view: Option<PrivateObjectView>,
}

impl Deref for GitSyncCommand {
    type Target = StdCommand;

    fn deref(&self) -> &Self::Target {
        &self.command
    }
}

impl DerefMut for GitSyncCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.command
    }
}

#[derive(Debug)]
struct PrivateObjectView {
    lease: Option<Arc<CachedObjectView>>,
    ephemeral: Option<tempfile::TempDir>,
}

impl PrivateObjectView {
    fn path(&self) -> &Path {
        self.lease
            .as_ref()
            .map(|view| view.directory.path())
            .or_else(|| self.ephemeral.as_ref().map(tempfile::TempDir::path))
            .expect("private object view always owns a directory")
    }
}

#[derive(Debug)]
struct CachedObjectView {
    source: PathBuf,
    fingerprint: [u8; 32],
    directory: tempfile::TempDir,
    _cache_root: Arc<tempfile::TempDir>,
    bytes: u64,
    last_used: std::sync::atomic::AtomicU64,
    watcher: Option<ObjectStoreWatcher>,
}

#[derive(Debug)]
struct ObjectStoreWatcher {
    dirty: std::sync::atomic::AtomicBool,
    #[cfg(target_os = "linux")]
    fd: rustix::fd::OwnedFd,
}

#[derive(Debug)]
struct ObjectViewCache {
    root: Arc<tempfile::TempDir>,
    entries: Vec<Arc<CachedObjectView>>,
    bytes: u64,
    clock: u64,
}

fn report_private_object_view_failure(context: &str, error: &dyn std::fmt::Debug) {
    if std::env::var_os("CODEGG_GIT_TEST_DIAGNOSTICS").is_some() {
        eprintln!("private Git object view rejected at {context}: {error:?}");
    }
}

impl ObjectStoreWatcher {
    fn new(root: &Path) -> std::io::Result<Option<Self>> {
        #[cfg(target_os = "linux")]
        {
            use rustix::fs::inotify::{self, CreateFlags, WatchFlags};

            let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)
                .map_err(rustix_io_error)?;
            let flags = WatchFlags::ATTRIB
                | WatchFlags::CLOSE_WRITE
                | WatchFlags::CREATE
                | WatchFlags::DELETE
                | WatchFlags::DELETE_SELF
                | WatchFlags::MODIFY
                | WatchFlags::MOVE_SELF
                | WatchFlags::MOVED_FROM
                | WatchFlags::MOVED_TO;
            inotify::add_watch(&fd, root, flags).map_err(rustix_io_error)?;
            let mut children = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(|entry| entry.file_name());
            for entry in children {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let is_fanout =
                    name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit());
                if name != "pack" && !is_fanout {
                    continue;
                }
                let metadata = std::fs::symlink_metadata(entry.path())?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "unsupported Git object namespace directory",
                    ));
                }
                inotify::add_watch(&fd, entry.path(), flags).map_err(rustix_io_error)?;
            }
            Ok(Some(Self {
                dirty: std::sync::atomic::AtomicBool::new(false),
                fd,
            }))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = root;
            Ok(None)
        }
    }

    /// `Some(true)` means no mutation event is pending. Any event (including
    /// queue overflow or watch loss) invalidates the cache generation.
    fn is_clean(&self) -> std::io::Result<bool> {
        if self.dirty.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        {
            use rustix::fs::inotify;
            use std::mem::MaybeUninit;

            let mut buffer = [MaybeUninit::uninit(); 4096];
            let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
            match reader.next() {
                Err(rustix::io::Errno::AGAIN) => Ok(true),
                Err(error) => Err(rustix_io_error(error)),
                Ok(_) => {
                    self.dirty.store(true, std::sync::atomic::Ordering::Release);
                    Ok(false)
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(false)
        }
    }
}

const OBJECT_VIEW_CACHE_ENTRIES: usize = 4;
const OBJECT_VIEW_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const OBJECT_VIEW_SNAPSHOT_ATTEMPTS: usize = 3;
static OBJECT_VIEW_CACHE: OnceLock<Mutex<Option<ObjectViewCache>>> = OnceLock::new();
#[cfg(all(test, target_os = "linux"))]
static SNAPSHOT_FICLONE_SUCCESS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(all(test, target_os = "linux"))]
static SNAPSHOT_FICLONE_FALLBACK: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Release cached trusted-Git object snapshots during the owning daemon's
/// shutdown. A running child retains its `Arc` lease until it exits; cache
/// directories are then removed in child-before-root order.
pub fn shutdown_object_view_cache() {
    if let Some(cache) = OBJECT_VIEW_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            *cache = None;
        }
    }
}

impl Default for GitEnvPolicy {
    fn default() -> Self {
        Self {
            terminal_prompt_disabled: true,
            pin_editor: true,
            strip_editors: true,
            strip_command_bearers: true,
        }
    }
}

impl GitEnvPolicy {
    /// Construct an async command from a complete argv (`git` included).
    pub fn apply(
        &self,
        argv: &[String],
        cwd: &Path,
    ) -> Result<GitAsyncCommand, GitExecutionPolicyOutcome> {
        ensure_trusted_git_request(argv)?;
        let object_view = prepare_private_object_view(argv, cwd)?;
        let worktree = canonical_worktree(cwd)?;
        let mut cmd = Command::new("git");
        add_trusted_git_overrides(&mut cmd, argv, &worktree);
        cmd.current_dir(cwd);
        apply_environment(&mut cmd, self, &worktree, argv);
        if let Some(view) = &object_view {
            cmd.env("GIT_OBJECT_DIRECTORY", view.path());
            cmd.env("GIT_OPTIONAL_LOCKS", "0");
        }
        cmd.kill_on_drop(true);
        Ok(GitAsyncCommand {
            command: cmd,
            _object_view: object_view,
        })
    }

    /// Construct a synchronous command from a complete argv (`git` included).
    pub fn apply_sync(
        &self,
        argv: &[String],
        cwd: &Path,
    ) -> Result<GitSyncCommand, GitExecutionPolicyOutcome> {
        ensure_trusted_git_request(argv)?;
        let object_view = prepare_private_object_view(argv, cwd)?;
        let worktree = canonical_worktree(cwd)?;
        let mut cmd = StdCommand::new("git");
        add_trusted_git_overrides(&mut cmd, argv, &worktree);
        cmd.current_dir(cwd);
        isolate_child_process_group(&mut cmd);
        apply_environment(&mut cmd, self, &worktree, argv);
        if let Some(view) = &object_view {
            cmd.env("GIT_OBJECT_DIRECTORY", view.path());
            cmd.env("GIT_OPTIONAL_LOCKS", "0");
        }
        Ok(GitSyncCommand {
            command: cmd,
            _object_view: object_view,
        })
    }
}

/// Classify a shell-free Git invocation before it crosses the process
/// boundary. Contained/authorization outcomes are typed handoffs; they never
/// become fabricated executable paths or generic spawn failures.
pub fn classify_git_request(argv: &[String]) -> GitExecutionPolicyOutcome {
    if argv.is_empty() || argv[0] != "git" {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "expected complete argv beginning with git".into(),
        };
    }

    // Global config and repository selectors change the authority context
    // after policy evaluation. CodeGG's typed renderers do not need them.
    if argv
        .iter()
        .skip(1)
        .take_while(|arg| arg.starts_with('-'))
        .any(|arg| {
            arg == "-c"
                || arg.starts_with("-c") && arg.len() > 2
                || arg == "--config-env"
                || arg.starts_with("--config-env=")
                || arg == "--git-dir"
                || arg.starts_with("--git-dir=")
                || arg == "--work-tree"
                || arg.starts_with("--work-tree=")
                || arg == "-C"
                || arg.starts_with("-C") && arg.len() > 2
                || arg == "--exec-path"
                || arg.starts_with("--exec-path=")
        })
    {
        return GitExecutionPolicyOutcome::DeniedExplicitAuthority {
            reason: "caller supplied global Git configuration or repository redirection".into(),
        };
    }

    let Some(subcommand) = argv.get(1).map(String::as_str) else {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "missing Git operation".into(),
        };
    };
    if subcommand.starts_with('-') {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "missing Git operation before operation arguments".into(),
        };
    }

    if matches!(subcommand, "diff" | "diff-tree" | "show" | "log" | "blame")
        && argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "--ext-diff" | "--textconv"))
    {
        return requires_containment(vec![GitExecutionCapability::ExternalDiffOrTextconv]);
    }

    // Operations that only inspect trusted repository state. Diff extension
    // execution paths are disabled explicitly by add_trusted_git_overrides.
    if matches!(
        subcommand,
        "fetch" | "pull" | "push" | "clone" | "ls-remote"
    ) {
        return remote_git_requires_containment();
    }

    if subcommand == "remote" {
        let operation = argv.get(2).map(String::as_str).unwrap_or_default();
        if matches!(operation, "update" | "prune")
            || operation == "show"
                && !argv
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-n" | "--no-fetch"))
        {
            return remote_git_requires_containment();
        }
        return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
    }

    if matches!(subcommand, "reset" | "clean") {
        // A dry-run clean (`-n`/`--dry-run`) changes nothing: it is the
        // read-only preview operators approve against, so it must not
        // require the destructive capability. `reset` has no dry-run mode
        // and a real clean still gates below.
        if subcommand == "clean"
            && argv
                .iter()
                .any(|arg| matches!(arg.as_str(), "-n" | "--dry-run"))
        {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        return GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
            capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange],
        };
    }

    let capabilities = match subcommand {
        "add" => vec![GitExecutionCapability::CleanSmudgeFilter],
        "checkout" | "restore" | "switch" => {
            vec![GitExecutionCapability::CleanSmudgeFilter]
        }
        "merge" | "cherry-pick" | "rebase" | "revert" => vec![
            GitExecutionCapability::CleanSmudgeFilter,
            GitExecutionCapability::MergeDriver,
            GitExecutionCapability::Hook,
        ],
        "commit" => return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable,
        "grep" if argv.iter().any(|arg| arg == "--textconv") => {
            vec![GitExecutionCapability::ExternalDiffOrTextconv]
        }
        "stash" if argv.get(2).is_some_and(|operation| operation == "list") => {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        "stash" => vec![
            GitExecutionCapability::CleanSmudgeFilter,
            GitExecutionCapability::Hook,
        ],
        "worktree" if argv.get(2).is_some_and(|operation| operation == "list") => {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        "worktree" => vec![GitExecutionCapability::CleanSmudgeFilter],
        "am" | "bisect" => vec![GitExecutionCapability::Hook],
        "apply" | "blame" | "branch" | "cat-file" | "check-attr" | "config" | "diff"
        | "diff-tree" | "for-each-ref" | "grep" | "init" | "log" | "ls-files" | "ls-tree"
        | "merge-base" | "mv" | "notes" | "reflog" | "rev-parse" | "rev-list" | "rm" | "show"
        | "status" | "tag" | "update-ref" => {
            if subcommand == "cat-file" && argv.iter().any(|arg| arg == "--filters") {
                return requires_containment(vec![GitExecutionCapability::CleanSmudgeFilter]);
            }
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        _ => vec![GitExecutionCapability::RepositoryAlias],
    };
    requires_containment(capabilities)
}

fn requires_containment(capabilities: Vec<GitExecutionCapability>) -> GitExecutionPolicyOutcome {
    GitExecutionPolicyOutcome::RequiresContainedExecution { capabilities }
}

fn remote_git_requires_containment() -> GitExecutionPolicyOutcome {
    requires_containment(vec![
        GitExecutionCapability::RemoteHelper,
        GitExecutionCapability::Network,
        GitExecutionCapability::Credential,
    ])
}

fn ensure_trusted_git_request(argv: &[String]) -> Result<(), GitExecutionPolicyOutcome> {
    match classify_git_request(argv) {
        GitExecutionPolicyOutcome::AllowedTrustedNonExecutable => Ok(()),
        GitExecutionPolicyOutcome::RequiresContainedExecution { capabilities } => {
            Err(GitExecutionPolicyOutcome::BackendUnavailable { capabilities })
        }
        other => Err(other),
    }
}

fn add_trusted_git_overrides<C>(cmd: &mut C, argv: &[String], worktree: &Path)
where
    C: CommandArguments,
{
    // These options take precedence over repository-local config and prevent
    // Git from promoting config values into child processes. Network routing
    // remains possible through the separately reviewed environment policy.
    for (key, value) in [
        ("core.fsmonitor", "false"),
        ("core.hooksPath", "/dev/null"),
        ("core.pager", "cat"),
        ("core.sshCommand", ""),
        ("credential.helper", ""),
        ("diff.external", ""),
        ("commit.gpgSign", "false"),
        ("tag.gpgSign", "false"),
        ("gpg.program", ""),
        ("core.askPass", ""),
        ("protocol.ext.allow", "never"),
    ] {
        cmd.arg("-c");
        cmd.arg(format!("{key}={value}"));
    }
    // The daemon supplies the repository root as cwd. Pin Git's worktree to
    // that selected root so repository-local `core.worktree` cannot redirect
    // a trusted read into an arbitrary host path. A newly created nested
    // repository is consequently data inside this root; it cannot retarget
    // an operation that was admitted for the root repository.
    cmd.arg(format!("--work-tree={}", worktree.display()));
    cmd.arg("--no-pager");
    if argv.get(1).is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "diff" | "diff-tree" | "show" | "log" | "blame"
        )
    }) {
        cmd.arg(argv[1].as_str());
        cmd.arg("--no-ext-diff");
        cmd.arg("--no-textconv");
        cmd.args(&argv[2..]);
    } else {
        cmd.args(&argv[1..]);
    }
}

trait CommandArguments {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self;
    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>;
}

impl CommandArguments for Command {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self {
        self.arg(arg)
    }

    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.args(args)
    }
}

impl CommandArguments for StdCommand {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self {
        self.arg(arg)
    }

    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.args(args)
    }
}

/// Put commands that may need tree cleanup in a private process group.
///
/// `kill_process_tree` signals the child's process group on Unix. Without
/// making the child its own group leader first, `getpgid(child)` can return
/// the test runner or application process group and timeout cleanup would
/// kill unrelated processes along with the child.
fn isolate_child_process_group(cmd: &mut StdCommand) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

fn apply_environment<C>(cmd: &mut C, policy: &GitEnvPolicy, worktree: &Path, argv: &[String])
where
    C: CommandEnvironment,
{
    cmd.env_clear();
    for key in ALLOWED_ENV_VARS {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    if policy.strip_command_bearers {
        for key in ALWAYS_STRIPPED_ENV_VARS {
            cmd.env_remove(key);
        }
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1".into());
    cmd.env("GIT_CONFIG_GLOBAL", null_config_path().into());
    // Git normally walks parent directories to discover a repository. The
    // typed service has already selected `worktree`; do not let a missing
    // or newly replaced root marker silently select an enclosing repository.
    if let Some(parent) = worktree.parent() {
        cmd.env("GIT_CEILING_DIRECTORIES", parent.as_os_str().to_owned());
    }
    if policy.terminal_prompt_disabled {
        cmd.env("GIT_TERMINAL_PROMPT", "0".into());
    }
    if policy.pin_editor {
        cmd.env("GIT_EDITOR", "true".into());
        cmd.env("GIT_SEQUENCE_EDITOR", "true".into());
    }
    if policy.strip_editors {
        cmd.env_remove("EDITOR");
        cmd.env_remove("VISUAL");
    }
    cmd.env("GPG_TTY", "".into());
    cmd.env("GIT_PAGER", "cat".into());
    cmd.env("PAGER", "cat".into());

    apply_commit_identity(cmd, argv, worktree);
}

/// Preserve Git's user identity preference without loading arbitrary global
/// configuration into the trusted operation. Local values retain normal Git
/// precedence; only missing `user.name`/`user.email` values are copied from
/// global config into command-scoped configuration data. Helpers, hooks,
/// aliases, and other global settings remain disabled.
fn apply_commit_identity<C>(cmd: &mut C, argv: &[String], worktree: &Path)
where
    C: CommandEnvironment,
{
    if !needs_commit_identity(argv) {
        return;
    }

    let mut config = Vec::new();
    for key in ["user.name", "user.email"] {
        let value = read_config_value(worktree, "--local", key)
            .or_else(|| read_config_value(worktree, "--global", key));
        let Some(value) = value.filter(|value| valid_identity_value(value)) else {
            continue;
        };
        config.push((key, value));
    }

    if config.is_empty() {
        return;
    }
    cmd.env("GIT_CONFIG_COUNT", config.len().to_string().into());
    for (index, (key, value)) in config.into_iter().enumerate() {
        let key_env = format!("GIT_CONFIG_KEY_{index}");
        let value_env = format!("GIT_CONFIG_VALUE_{index}");
        cmd.env(&key_env, key.to_owned().into());
        cmd.env(&value_env, value.into());
    }
}

fn needs_commit_identity(argv: &[String]) -> bool {
    match argv.get(1).map(String::as_str) {
        Some("commit" | "merge" | "cherry-pick" | "revert" | "rebase" | "am") => true,
        Some("tag") => argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "-a" | "--annotate")),
        _ => false,
    }
}

fn valid_identity_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value
            .chars()
            .any(|character| character == '\0' || character == '\n' || character == '\r')
}

fn read_config_value(worktree: &Path, scope: &str, key: &str) -> Option<String> {
    let mut command = StdCommand::new("git");
    command
        .env_clear()
        .current_dir(worktree)
        .args(["config", scope, "--null", "--get", key])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat");
    for name in ["PATH", "HOME", "XDG_CONFIG_HOME", "GIT_CONFIG_GLOBAL"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut output = Vec::with_capacity(4097);
    let stdout = child.stdout.take()?;
    stdout.take(4097).read_to_end(&mut output).ok()?;
    if output.len() > 4096 {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    let value = std::str::from_utf8(&output).ok()?;
    let value = value.strip_suffix('\0').unwrap_or(value);
    valid_identity_value(value).then(|| value.to_owned())
}

fn canonical_worktree(cwd: &Path) -> Result<std::path::PathBuf, GitExecutionPolicyOutcome> {
    let worktree = cwd
        .canonicalize()
        .map_err(|_| GitExecutionPolicyOutcome::InvalidRequest {
            reason: "trusted Git worktree could not be canonicalized".into(),
        })?;
    if !worktree.is_dir() || worktree.to_str().is_none() {
        return Err(GitExecutionPolicyOutcome::InvalidRequest {
            reason: "trusted Git worktree must be an existing UTF-8 directory".into(),
        });
    }
    Ok(worktree)
}

/// Resolve the selected repository object directory and reject unsupported
/// aliases. Static alternates are refused for compatibility; read commands
/// additionally receive a private object view that has no alternates file, so
/// creating one after this check cannot redirect the running Git process.
fn trusted_object_directory(
    cwd: &Path,
) -> Result<Option<std::path::PathBuf>, GitExecutionPolicyOutcome> {
    let Some(worktree) = crate::worktree::find_git_root(cwd) else {
        return Ok(None);
    };
    let unavailable = || GitExecutionPolicyOutcome::BackendUnavailable {
        capabilities: vec![GitExecutionCapability::RepositoryAlias],
    };
    let dot_git = worktree.join(".git");
    let metadata = std::fs::symlink_metadata(&dot_git).map_err(|_| unavailable())?;
    let git_dir = if metadata.file_type().is_symlink() {
        return Err(unavailable());
    } else if metadata.is_dir() {
        dot_git.canonicalize().map_err(|_| unavailable())?
    } else if metadata.is_file() && metadata.len() <= 8 * 1024 {
        let pointer = std::fs::read_to_string(&dot_git).map_err(|_| unavailable())?;
        let mut lines = pointer.lines();
        let Some(target) = lines.next().and_then(|line| line.strip_prefix("gitdir: ")) else {
            return Err(unavailable());
        };
        if lines.next().is_some() || target.trim().is_empty() {
            return Err(unavailable());
        }
        let target = Path::new(target.trim());
        let target = if target.is_absolute() {
            target.to_path_buf()
        } else {
            worktree.join(target)
        };
        target.canonicalize().map_err(|_| unavailable())?
    } else {
        return Err(unavailable());
    };

    let mut git_dirs = vec![git_dir.clone()];
    let common_dir_file = git_dir.join("commondir");
    match std::fs::symlink_metadata(&common_dir_file) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= 4 * 1024 => {
            let common = std::fs::read_to_string(&common_dir_file).map_err(|_| unavailable())?;
            let common = common.trim();
            if common.is_empty() || common.lines().count() != 1 {
                return Err(unavailable());
            }
            let common = Path::new(common);
            let common = if common.is_absolute() {
                common.to_path_buf()
            } else {
                git_dir.join(common)
            };
            git_dirs.push(common.canonicalize().map_err(|_| unavailable())?);
        }
        Ok(_) => return Err(unavailable()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(unavailable()),
    }

    let expected_object_dir = git_dirs
        .last()
        .expect("selected git directory is always present")
        .join("objects");
    for git_dir in git_dirs {
        let alternates = git_dir.join("objects/info/alternates");
        match std::fs::symlink_metadata(&alternates) {
            Ok(_) => return Err(unavailable()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(unavailable()),
        }
    }
    let object_metadata =
        std::fs::symlink_metadata(&expected_object_dir).map_err(|_| unavailable())?;
    if object_metadata.file_type().is_symlink() || !object_metadata.is_dir() {
        return Err(unavailable());
    }
    let object_dir = expected_object_dir
        .canonicalize()
        .map_err(|_| unavailable())?;
    Ok(Some(object_dir))
}

fn prepare_private_object_view(
    argv: &[String],
    cwd: &Path,
) -> Result<Option<PrivateObjectView>, GitExecutionPolicyOutcome> {
    if !argv_reads_objects(argv) {
        return Ok(None);
    }
    let unavailable = || GitExecutionPolicyOutcome::BackendUnavailable {
        capabilities: vec![GitExecutionCapability::RepositoryAlias],
    };
    let unavailable_with = |context: &str, error: &dyn std::fmt::Debug| {
        report_private_object_view_failure(context, error);
        unavailable()
    };
    let object_dir = match trusted_object_directory(cwd) {
        Ok(object_dir) => object_dir,
        Err(error) => {
            return Err(unavailable_with("repository metadata", &error));
        }
    };
    let Some(object_dir) = object_dir else {
        return Ok(None);
    };
    let cache = OBJECT_VIEW_CACHE.get_or_init(|| Mutex::new(None));
    let mut cache = cache
        .lock()
        .map_err(|error| unavailable_with("cache lock", &error))?;
    if cache.is_none() {
        let root = Arc::new(
            tempfile::Builder::new()
                .prefix("codegg-git-object-cache-")
                .tempdir()
                .map_err(|error| unavailable_with("cache directory", &error))?,
        );
        *cache = Some(ObjectViewCache {
            root,
            entries: Vec::new(),
            bytes: 0,
            clock: 0,
        });
    }
    let cache = cache.as_mut().expect("initialized object-view cache");
    cache.clock = cache.clock.wrapping_add(1);
    let tick = cache.clock;
    let mut stale = Vec::new();
    for (index, view) in cache.entries.iter().enumerate() {
        if view.source != object_dir {
            continue;
        }
        let clean = if let Some(watcher) = &view.watcher {
            watcher
                .is_clean()
                .map_err(|error| unavailable_with("cached view watcher", &error))?
        } else {
            let current = object_store_fingerprint(&object_dir)
                .map_err(|error| unavailable_with("cached view fingerprint", &error))?;
            current == view.fingerprint
        };
        if clean {
            view.last_used
                .store(tick, std::sync::atomic::Ordering::Relaxed);
            return Ok(Some(PrivateObjectView {
                lease: Some(Arc::clone(view)),
                ephemeral: None,
            }));
        }
        stale.push(index);
    }
    for index in stale.into_iter().rev() {
        if Arc::strong_count(&cache.entries[index]) == 1 {
            let retired = cache.entries.remove(index);
            cache.bytes = cache.bytes.saturating_sub(retired.bytes);
        }
    }

    // The watcher is installed before fingerprinting/copying. A bounded
    // retry accommodates short-lived Git object-store maintenance while every
    // published snapshot still has to pass its own full stability check.
    // Each attempt gets a fresh watcher and source fingerprint; no unstable
    // partial copy can reach a child or the cache.
    let mut stable_snapshot = None;
    for _ in 0..OBJECT_VIEW_SNAPSHOT_ATTEMPTS {
        let watcher = ObjectStoreWatcher::new(&object_dir).ok().flatten();
        let fingerprint = object_store_fingerprint(&object_dir)
            .map_err(|error| unavailable_with("source fingerprint", &error))?;

        // Build inside the daemon-owned cache root. The resulting TempDir
        // remains private and immutable for every child holding a lease.
        let view = tempfile::Builder::new()
            .prefix("view-")
            .tempdir_in(cache.root.path())
            .map_err(|error| unavailable_with("snapshot directory", &error))?;
        let bytes = copy_object_files(&object_dir, view.path())
            .map_err(|error| unavailable_with("snapshot copy", &error))?;
        let stable = if let Some(watcher) = &watcher {
            watcher
                .is_clean()
                .map_err(|error| unavailable_with("snapshot watcher", &error))?
        } else {
            object_store_fingerprint(&object_dir)
                .map_err(|error| unavailable_with("post-copy fingerprint", &error))?
                == fingerprint
        };
        if stable {
            stable_snapshot = Some((view, bytes, fingerprint, watcher));
            break;
        }
    }
    let Some((view, bytes, fingerprint, watcher)) = stable_snapshot else {
        report_private_object_view_failure("source stability", &"source changed during snapshot");
        return Err(unavailable());
    };
    if bytes > OBJECT_VIEW_CACHE_MAX_BYTES {
        return Ok(Some(PrivateObjectView {
            lease: None,
            ephemeral: Some(view),
        }));
    }

    // Retire stale generations of this repository when possible. A child
    // holding an Arc lease keeps its immutable directory alive until exit.
    let mut index = 0;
    while index < cache.entries.len() {
        if cache.entries[index].source == object_dir
            && Arc::strong_count(&cache.entries[index]) == 1
        {
            let retired = cache.entries.remove(index);
            cache.bytes = cache.bytes.saturating_sub(retired.bytes);
        } else {
            index += 1;
        }
    }
    while cache.entries.len() >= OBJECT_VIEW_CACHE_ENTRIES
        || cache.bytes.saturating_add(bytes) > OBJECT_VIEW_CACHE_MAX_BYTES
    {
        let Some((index, _)) = cache
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| Arc::strong_count(entry) == 1)
            .min_by_key(|(_, entry)| entry.last_used.load(std::sync::atomic::Ordering::Relaxed))
        else {
            // All cache slots are leased by running Git children. Keep this
            // snapshot command-scoped rather than exceeding the cache bound.
            return Ok(Some(PrivateObjectView {
                lease: None,
                ephemeral: Some(view),
            }));
        };
        let retired = cache.entries.remove(index);
        cache.bytes = cache.bytes.saturating_sub(retired.bytes);
    }

    let cached = Arc::new(CachedObjectView {
        source: object_dir,
        fingerprint,
        directory: view,
        _cache_root: Arc::clone(&cache.root),
        bytes,
        last_used: std::sync::atomic::AtomicU64::new(tick),
        watcher,
    });
    cache.bytes = cache.bytes.saturating_add(bytes);
    cache.entries.push(Arc::clone(&cached));
    Ok(Some(PrivateObjectView {
        lease: Some(cached),
        ephemeral: None,
    }))
}

/// Hash the admitted object namespace for cache invalidation. Only entries
/// Git can address through loose fanout or packs participate. The selected
/// names and metadata include inode identity, size, and change time so an
/// observed add, replacement, in-place write, or pack rotation cannot reuse
/// an older private view. Object contents are not read during a cache hit.
fn object_store_fingerprint(root: &Path) -> std::io::Result<[u8; 32]> {
    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    let root_metadata = std::fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Git object directory changed type",
        ));
    }
    fingerprint_metadata(&mut digest, b".", &root_metadata);
    let mut directories = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    directories.sort_by_key(|entry| entry.file_name());
    for entry in directories {
        let name = entry.file_name();
        let name_bytes = name.to_string_lossy();
        if name_bytes == "info" {
            continue;
        }
        let is_fanout =
            name_bytes.len() == 2 && name_bytes.bytes().all(|byte| byte.is_ascii_hexdigit());
        if name_bytes != "pack" && !is_fanout {
            continue;
        }
        let directory = entry.path();
        let metadata = std::fs::symlink_metadata(&directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported Git object namespace directory",
            ));
        }
        fingerprint_metadata(&mut digest, name_bytes.as_bytes(), &metadata);
        let mut objects = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        objects.sort_by_key(|object| object.file_name());
        for object in objects {
            let object_name = object.file_name();
            #[cfg(unix)]
            let object_name_bytes = {
                use std::os::unix::ffi::OsStrExt;
                object_name.as_bytes().to_vec()
            };
            #[cfg(not(unix))]
            let object_name_bytes = object_name.to_string_lossy().as_bytes().to_vec();
            if is_fanout {
                let suffix = std::str::from_utf8(&object_name_bytes).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-UTF-8 loose Git object name",
                    )
                })?;
                if !((suffix.len() == 38 || suffix.len() == 62)
                    && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
                {
                    continue;
                }
            }
            let path = object.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unsupported entry in Git object namespace",
                ));
            }
            let mut relative = name_bytes.as_bytes().to_vec();
            relative.push(b'/');
            relative.extend_from_slice(&object_name_bytes);
            fingerprint_metadata(&mut digest, &relative, &metadata);
        }
    }
    Ok(digest.finalize().into())
}

fn fingerprint_metadata(digest: &mut sha2::Sha256, path: &[u8], metadata: &std::fs::Metadata) {
    use sha2::Digest;

    digest.update((path.len() as u64).to_le_bytes());
    digest.update(path);
    digest.update(metadata.len().to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(metadata.dev().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
        digest.update(metadata.ctime().to_le_bytes());
        digest.update(metadata.ctime_nsec().to_le_bytes());
        digest.update(metadata.mtime().to_le_bytes());
        digest.update(metadata.mtime_nsec().to_le_bytes());
    }
    #[cfg(not(unix))]
    if let Ok(modified) = metadata.modified().and_then(|time| {
        time.duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)
    }) {
        digest.update(modified.as_secs().to_le_bytes());
        digest.update(modified.subsec_nanos().to_le_bytes());
    }
}

/// The trusted read surface is deliberately an exact allowlist. Mutating and
/// potentially executable Git routes do not receive this read-only view.
fn argv_reads_objects(argv: &[String]) -> bool {
    let Some(subcommand) = argv.get(1).map(String::as_str) else {
        return false;
    };
    match subcommand {
        // These ref-only forms do not inspect commit/tree/blob contents, so
        // an object-store alias must not make them unavailable. Verbose and
        // formatted listings can render object summaries and retain the
        // private-view requirement.
        "branch" => argv.iter().any(|arg| {
            matches!(arg.as_str(), "-v" | "-vv" | "--verbose")
                || arg == "--format"
                || arg.starts_with("--format=")
                || (arg.starts_with('-') && arg[1..].contains('v'))
        }),
        "apply" => argv.iter().any(|arg| arg == "--3way"),
        "check-attr" => argv
            .iter()
            .any(|arg| arg == "--source" || arg.starts_with("--source=")),
        "config" => argv
            .iter()
            .any(|arg| arg == "--blob" || arg.starts_with("--blob=")),
        "notes" => matches!(argv.get(2).map(String::as_str), Some("list" | "show")),
        "remote" => argv.get(2).is_some_and(|operation| operation == "show"),
        "stash" | "worktree" => argv.get(2).is_some_and(|operation| operation == "list"),
        "tag" => {
            argv.len() == 2
                || argv
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-l" | "--list"))
        }
        _ => matches!(
            subcommand,
            "blame"
                | "branch"
                | "cat-file"
                | "diff"
                | "diff-tree"
                | "for-each-ref"
                | "grep"
                | "log"
                | "ls-files"
                | "ls-tree"
                | "merge-base"
                | "reflog"
                | "rev-list"
                | "rev-parse"
                | "show"
                | "status"
                | "update-ref"
        ),
    }
}

/// Construct a private Git object directory without the repository's
/// `objects/info` metadata. Only regular files in Git's loose-object fanout
/// and pack directories are admitted. The directory is a sibling of the
/// selected object store. Object contents are copied rather than hard-linked:
/// a hard link would let a concurrent workspace writer alter the command's
/// supposedly private view in place. The guard removes it after the command
/// exits or is cancelled.
fn copy_object_files(source: &Path, destination: &Path) -> std::io::Result<u64> {
    #[cfg(unix)]
    {
        copy_object_files_descriptor_relative(source, destination)
    }
    #[cfg(not(unix))]
    {
        copy_object_files_path_based(source, destination)
    }
}

#[cfg(unix)]
fn copy_object_files_descriptor_relative(
    source: &Path,
    destination: &Path,
) -> std::io::Result<u64> {
    use rustix::fs::{open, openat, Dir, FileType, Mode, OFlags};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let before = std::fs::symlink_metadata(source)?;
    if !before.is_dir() || before.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Git object directory changed type",
        ));
    }
    let source_fd = open(
        source,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(rustix_io_error)?;
    let source_file = std::fs::File::from(source_fd);
    let opened = source_file.metadata()?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Git object directory identity changed during snapshot setup",
        ));
    }

    let mut copied = 0usize;
    let mut bytes = 0u64;
    let mut directories = Dir::read_from(&source_file).map_err(rustix_io_error)?;
    for entry in &mut directories {
        let entry = entry.map_err(rustix_io_error)?;
        let raw_name = entry.file_name().to_bytes();
        if raw_name == b"." || raw_name == b".." || raw_name == b"info" {
            continue;
        }
        let name = OsStr::from_bytes(raw_name);
        let name_text = name.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "non-UTF-8 Git object entry",
            )
        })?;
        let is_fanout =
            name_text.len() == 2 && name_text.bytes().all(|byte| byte.is_ascii_hexdigit());
        if name_text != "pack" && !is_fanout {
            // Git may leave temporary entries while another process writes
            // objects. They are outside Git's lookup namespace, so omit them
            // from this immutable view instead of failing trusted reads.
            continue;
        }
        let source_subdir = openat(
            &source_file,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(rustix_io_error)?;
        let target_dir = destination.join(name);
        std::fs::create_dir(&target_dir)?;
        let mut objects = Dir::read_from(&source_subdir).map_err(rustix_io_error)?;
        for object in &mut objects {
            let object = object.map_err(rustix_io_error)?;
            let raw_object_name = object.file_name().to_bytes();
            if raw_object_name == b"." || raw_object_name == b".." {
                continue;
            }
            let object_name = OsStr::from_bytes(raw_object_name);
            if is_fanout {
                let suffix = object_name.to_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-UTF-8 loose Git object name",
                    )
                })?;
                if !((suffix.len() == 38 || suffix.len() == 62)
                    && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
                {
                    // Temporary names and unrelated files cannot be looked
                    // up as loose objects by Git.
                    continue;
                }
            }
            let object_fd = openat(
                &source_subdir,
                object_name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(rustix_io_error)?;
            let object_file = std::fs::File::from(object_fd);
            let object_metadata = rustix::fs::fstat(&object_file).map_err(rustix_io_error)?;
            if FileType::from_raw_mode(object_metadata.st_mode) != FileType::RegularFile {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unsupported entry in Git object fanout",
                ));
            }
            bytes = bytes
                .checked_add(object_metadata.st_size.max(0) as u64)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "object byte count overflow",
                    )
                })?;
            copied = copied.checked_add(1).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "object count overflow")
            })?;
            if copied > 1_000_000 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Git object snapshot exceeds entry bound",
                ));
            }
            let target = destination.join(name).join(object_name);
            let mut snapshot = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            copy_object_snapshot(&object_file, &mut snapshot)?;
        }
    }
    Ok(bytes)
}

#[cfg(unix)]
fn copy_object_snapshot(
    source: &std::fs::File,
    destination: &mut std::fs::File,
) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        if rustix::fs::ioctl_ficlone(&*destination, source).is_ok() {
            #[cfg(test)]
            SNAPSHOT_FICLONE_SUCCESS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(());
        }
        #[cfg(test)]
        SNAPSHOT_FICLONE_FALLBACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        destination.set_len(0)?;
        std::io::Seek::seek(destination, std::io::SeekFrom::Start(0))?;
    }
    std::io::copy(&mut &*source, destination)?;
    Ok(())
}

#[cfg(unix)]
fn rustix_io_error(error: rustix::io::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error.raw_os_error())
}

#[cfg(not(unix))]
fn copy_object_files_path_based(source: &Path, destination: &Path) -> std::io::Result<u64> {
    let mut copied = 0usize;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "info" {
            continue;
        }
        let name_text = name.to_string_lossy();
        let is_fanout =
            name_text.len() == 2 && name_text.bytes().all(|byte| byte.is_ascii_hexdigit());
        if name_text != "pack" && !is_fanout {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported entry in Git object directory",
            ));
        }
        let target_dir = destination.join(&name);
        std::fs::create_dir(&target_dir)?;
        for object in std::fs::read_dir(entry.path())? {
            let object = object?;
            let object_name = object.file_name();
            if is_fanout {
                let suffix = object_name.to_string_lossy();
                if !((suffix.len() == 38 || suffix.len() == 62)
                    && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
                {
                    continue;
                }
            }
            let object_metadata = std::fs::symlink_metadata(object.path())?;
            if !object_metadata.is_file() || object_metadata.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unsupported entry in Git object fanout",
                ));
            }
            bytes = bytes.checked_add(object_metadata.len()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "object byte count overflow",
                )
            })?;
            copied = copied.checked_add(1).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "object count overflow")
            })?;
            if copied > 1_000_000 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Git object snapshot exceeds entry bound",
                ));
            }
            let target = target_dir.join(object.file_name());
            std::fs::copy(object.path(), &target)?;
        }
    }
    Ok(bytes)
}

#[cfg(windows)]
fn null_config_path() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_config_path() -> &'static str {
    "/dev/null"
}

trait CommandEnvironment {
    fn env_clear(&mut self);
    fn env(&mut self, key: &str, value: std::ffi::OsString);
    fn env_remove(&mut self, key: &str);
}

impl CommandEnvironment for Command {
    fn env_clear(&mut self) {
        self.env_clear();
    }

    fn env(&mut self, key: &str, value: std::ffi::OsString) {
        self.env(key, value);
    }

    fn env_remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl CommandEnvironment for StdCommand {
    fn env_clear(&mut self) {
        self.env_clear();
    }

    fn env(&mut self, key: &str, value: std::ffi::OsString) {
        self.env(key, value);
    }

    fn env_remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Run a Git command whose `args` omit the executable name.
///
/// The child runs under [`DEFAULT_READ_TIMEOUT`]; exceeding it kills the
/// process rather than leaking it.
pub async fn run(args: &[String], cwd: &Path) -> Result<StdOutput, EgggitError> {
    run_with_timeout(args, cwd, DEFAULT_READ_TIMEOUT).await
}

/// Run a Git command whose `args` omit the executable name, bounded by an
/// explicit `timeout`.
pub async fn run_with_timeout(
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<StdOutput, EgggitError> {
    if !cwd.exists() {
        return Err(EgggitError::NotARepository(cwd.display().to_string()));
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend_from_slice(args);
    let cwd = cwd.to_path_buf();
    let rendered = argv.join(" ");
    tokio::task::spawn_blocking(move || {
        let mut cmd = GitEnvPolicy::default()
            .apply_sync(&argv, &cwd)
            .map_err(EgggitError::Policy)?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let GitSyncCommand {
            command,
            _object_view,
        } = cmd;
        run_command_with_view(command, _object_view, timeout, &rendered)
    })
    .await
    .map_err(|error| EgggitError::Join(error.to_string()))?
}

/// Spawn `cmd`, drain stdout/stderr on their own threads so a large-output
/// child cannot deadlock on a full pipe, and enforce `timeout` against the
/// running process.
///
/// The deadline is enforced here rather than around the `spawn_blocking`
/// join: a blocking task is not cancellable, so an outer
/// `tokio::time::timeout` would return to the caller while the Git process
/// kept running and kept holding a blocking-pool thread.
#[cfg(test)]
fn run_command(
    cmd: StdCommand,
    timeout: Duration,
    rendered: &str,
) -> Result<StdOutput, EgggitError> {
    run_command_with_view(cmd, None, timeout, rendered)
}

fn run_command_with_view(
    mut cmd: StdCommand,
    _object_view: Option<PrivateObjectView>,
    timeout: Duration,
    rendered: &str,
) -> Result<StdOutput, EgggitError> {
    // `run_command` is also exercised with directly constructed commands in
    // tests, so enforce isolation here as well as in `GitEnvPolicy`.
    isolate_child_process_group(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|error| EgggitError::Io(error.to_string()))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = stdout.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr_handle = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Kill the whole process group: `git` spawns helpers
                    // (pager, credential helper, filter driver) that would
                    // otherwise outlive the direct child.
                    kill_process_tree(&mut child);
                    return Err(EgggitError::Timeout(format!(
                        "git command timed out after {}s: {}",
                        timeout.as_secs(),
                        rendered
                    )));
                }
                std::thread::sleep(CHILD_POLL_INTERVAL);
            }
            Err(error) => {
                kill_process_tree(&mut child);
                return Err(EgggitError::Io(format!("failed to wait on git: {error}")));
            }
        }
    };

    let stdout_bytes = stdout_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr_bytes = stderr_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();

    Ok(StdOutput {
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        status,
    })
}

/// Kill the Git child and any helper processes it spawned.
///
/// `git` frequently delegates to a pager, credential helper, or external
/// diff/merge driver. Killing only the direct child leaves those helpers
/// holding the repository lock, which is what turns one slow read into a
/// cascade of stuck ones.
fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // Signal the child's process group so helpers spawned by `git` die
        // with it. A positive `pid` is required: signalling group 0 would
        // hit our own process group.
        let pid = child.id();
        if pid > 0 {
            #[cfg(target_os = "macos")]
            let pgid = pid as i32;
            #[cfg(not(target_os = "macos"))]
            let pgid = unsafe { libc::getpgid(pid as i32) };
            if pgid > 0 {
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run a read-only Git command with a hard stdout byte limit and the
/// default wall-clock bound. The child is killed as soon as it exceeds
/// either bound; stderr is discarded to avoid an unbounded diagnostic buffer.
pub async fn run_bounded(
    args: &[String],
    cwd: &Path,
    max_stdout_bytes: usize,
) -> Result<StdOutput, EgggitError> {
    run_bounded_with_timeout(args, cwd, max_stdout_bytes, DEFAULT_READ_TIMEOUT).await
}

/// Run a read-only Git command bounded by both a stdout byte limit and an
/// explicit wall-clock timeout.
///
/// Stdout is read under a `Take` bound so an oversized child is killed at
/// the limit instead of being allowed to fill memory. Reading on the calling
/// thread (rather than `wait_with_output`) is deliberate: it lets the byte
/// limit trip before the process exits.
pub async fn run_bounded_with_timeout(
    args: &[String],
    cwd: &Path,
    max_stdout_bytes: usize,
    timeout: Duration,
) -> Result<StdOutput, EgggitError> {
    if !cwd.exists() {
        return Err(EgggitError::NotARepository(cwd.display().to_string()));
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend_from_slice(args);
    let cwd = cwd.to_path_buf();
    let rendered = argv.join(" ");
    tokio::task::spawn_blocking(move || {
        let mut cmd = GitEnvPolicy::default()
            .apply_sync(&argv, &cwd)
            .map_err(EgggitError::Policy)?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|error| EgggitError::Io(error.to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| EgggitError::Io("missing git stdout".into()))?;

        // Read one extra byte so the caller can detect truncation without
        // over-allocating.
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(max_stdout_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| EgggitError::Io(error.to_string()))?;
        if bytes.len() > max_stdout_bytes {
            kill_process_tree(&mut child);
            return Err(EgggitError::OutputTooLarge);
        }

        // stdout is drained, so only the deadline remains: reap promptly
        // instead of blocking until the child exits on its own.
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(StdOutput {
                        status,
                        stdout: bytes,
                        stderr: Vec::new(),
                    })
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_process_tree(&mut child);
                        return Err(EgggitError::Timeout(format!(
                            "git command timed out after {}s: {}",
                            timeout.as_secs(),
                            rendered
                        )));
                    }
                    std::thread::sleep(CHILD_POLL_INTERVAL);
                }
                Err(error) => {
                    kill_process_tree(&mut child);
                    return Err(EgggitError::Io(format!("failed to wait on git: {error}")));
                }
            }
        }
    })
    .await
    .map_err(|error| EgggitError::Join(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    #[test]
    fn private_object_view_omits_temporary_entries() {
        let source = tempfile::tempdir().expect("object source");
        let destination = tempfile::tempdir().expect("object view");
        let fanout = source.path().join("ab");
        std::fs::create_dir(&fanout).expect("fanout directory");
        let valid_object = fanout.join("c".repeat(38));
        std::fs::write(&valid_object, b"object").expect("valid loose object");
        std::fs::write(fanout.join("tmp_obj_in_progress"), b"partial")
            .expect("temporary loose object");
        std::fs::write(source.path().join("tmp_pack_in_progress"), b"partial")
            .expect("temporary object-store entry");

        copy_object_files(source.path(), destination.path()).expect("copy valid object view");

        assert_eq!(
            std::fs::read(destination.path().join("ab").join("c".repeat(38)))
                .expect("copied valid object"),
            b"object"
        );
        assert!(!destination.path().join("ab/tmp_obj_in_progress").exists());
        assert!(!destination.path().join("tmp_pack_in_progress").exists());
    }

    #[test]
    #[ignore = "manual performance qualification; requires CODEGG_OBJECT_VIEW_BENCH_REPOSITORY"]
    fn private_object_view_warm_performance_samples() {
        let repository = std::env::var_os("CODEGG_OBJECT_VIEW_BENCH_REPOSITORY")
            .map(PathBuf::from)
            .expect("set CODEGG_OBJECT_VIEW_BENCH_REPOSITORY to a Git worktree");
        let repository = repository.canonicalize().expect("canonical Git worktree");
        let samples = std::env::var("CODEGG_OBJECT_VIEW_BENCH_SAMPLES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(20);
        assert!((1..=100).contains(&samples), "sample count must be 1..=100");
        let cache_mode =
            std::env::var("CODEGG_OBJECT_VIEW_BENCH_CACHE_MODE").unwrap_or_else(|_| "warm".into());
        assert!(matches!(cache_mode.as_str(), "cold" | "warm"));
        let argv = vec!["git".to_string(), "status".to_string()];
        eprintln!(
            "private Git object-view benchmark: repository={} cache_mode={} samples={}",
            repository.display(),
            cache_mode,
            samples
        );
        if cache_mode == "warm" {
            drop(
                prepare_private_object_view(&argv, &repository).expect("warm private object view"),
            );
            let output = GitEnvPolicy::default()
                .apply_sync(&argv, &repository)
                .expect("prepare warm trusted Git command")
                .output()
                .expect("warm trusted git status");
            assert!(output.status.success());
        }
        let mut preparation_samples = Vec::with_capacity(samples);
        let mut protected_status_samples = Vec::with_capacity(samples);
        let mut baseline_status_samples = Vec::with_capacity(samples);
        for sample in 1..=samples {
            if cache_mode == "cold" {
                let object_dir = trusted_object_directory(&repository).unwrap().unwrap();
                clear_cached_object_view(&object_dir);
                evict_object_file_cache(&object_dir);
            }
            #[cfg(target_os = "linux")]
            {
                SNAPSHOT_FICLONE_SUCCESS.store(0, std::sync::atomic::Ordering::Relaxed);
                SNAPSHOT_FICLONE_FALLBACK.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            let started = Instant::now();
            let view = prepare_private_object_view(&argv, &repository)
                .expect("prepare trusted private object view");
            let preparation = started.elapsed();
            preparation_samples.push(preparation.as_secs_f64() * 1000.0);
            #[cfg(target_os = "linux")]
            eprintln!(
                "sample {sample}: ficlone_success={} ficlone_fallback={}",
                SNAPSHOT_FICLONE_SUCCESS.load(std::sync::atomic::Ordering::Relaxed),
                SNAPSHOT_FICLONE_FALLBACK.load(std::sync::atomic::Ordering::Relaxed),
            );
            drop(view);

            let started = Instant::now();
            let output = GitEnvPolicy::default()
                .apply_sync(&argv, &repository)
                .expect("prepare trusted private object view")
                .output()
                .expect("run trusted git status");
            assert!(output.status.success());
            let protected_status = started.elapsed();
            protected_status_samples.push(protected_status.as_secs_f64() * 1000.0);

            let started = Instant::now();
            let baseline = StdCommand::new("git")
                .args(&argv[1..])
                .current_dir(&repository)
                .output()
                .expect("run paired baseline git status");
            assert!(baseline.status.success());
            let baseline_status = started.elapsed();
            baseline_status_samples.push(baseline_status.as_secs_f64() * 1000.0);
            eprintln!(
                "sample {sample}: {:.3} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        for (name, mut values) in [
            ("preparation", preparation_samples),
            ("protected_status", protected_status_samples),
            ("baseline_status", baseline_status_samples),
        ] {
            values.sort_by(f64::total_cmp);
            let median = if values.len() % 2 == 0 {
                (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
            } else {
                values[values.len() / 2]
            };
            let p95_index = ((values.len() * 95).div_ceil(100) - 1).min(values.len() - 1);
            eprintln!(
                "summary {name}: median_ms={median:.3} p95_ms={:.3} max_ms={:.3}",
                values[p95_index],
                values[values.len() - 1],
            );
        }
    }

    fn clear_cached_object_view(root: &Path) {
        let Some(cache) = OBJECT_VIEW_CACHE.get() else {
            return;
        };
        let mut cache = cache.lock().expect("object-view cache lock");
        let Some(cache) = cache.as_mut() else {
            return;
        };
        let mut index = 0;
        while index < cache.entries.len() {
            if cache.entries[index].source == root {
                assert_eq!(
                    Arc::strong_count(&cache.entries[index]),
                    1,
                    "cannot cold-qualify while a child leases this view"
                );
                let retired = cache.entries.remove(index);
                cache.bytes = cache.bytes.saturating_sub(retired.bytes);
            } else {
                index += 1;
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn evict_object_file_cache(root: &Path) {
        use std::os::fd::AsRawFd;

        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory).expect("read object-store directory");
            for entry in entries {
                let entry = entry.expect("read object-store entry");
                let metadata = entry.metadata().expect("stat object-store entry");
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if metadata.is_file() {
                    let file = std::fs::File::open(entry.path()).expect("open object file");
                    // POSIX_FADV_DONTNEED asks the kernel to discard cached
                    // pages for these source files; it does not mutate repo
                    // contents or globally flush unrelated cache state.
                    let result = unsafe {
                        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED)
                    };
                    assert_eq!(result, 0, "advise cold object cache");
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn evict_object_file_cache(_root: &Path) {
        panic!("cold object-view measurement requires Linux POSIX_FADV_DONTNEED");
    }

    #[test]
    fn policy_tables_are_disjoint_and_unique() {
        let allowed = ALLOWED_ENV_VARS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let stripped = ALWAYS_STRIPPED_ENV_VARS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(allowed.len(), ALLOWED_ENV_VARS.len());
        assert_eq!(stripped.len(), ALWAYS_STRIPPED_ENV_VARS.len());
        assert!(allowed.is_disjoint(&stripped));
    }

    #[test]
    fn default_read_timeout_is_bounded() {
        // The regression that motivated this constant: structured reads used
        // an unbounded `spawn_blocking` wait, so a stuck git could hang a
        // `git_query` call indefinitely.
        assert!(DEFAULT_READ_TIMEOUT <= Duration::from_secs(60));
    }

    #[test]
    fn trusted_git_policy_overrides_execution_bearing_repository_config() {
        let cwd = tempfile::tempdir().expect("non-repository working directory");
        let argv = vec!["git".to_string(), "diff".to_string()];
        let cmd = GitEnvPolicy::default()
            .apply_sync(&argv, cwd.path())
            .unwrap();
        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        for required in [
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "core.sshCommand=",
            "credential.helper=",
            "diff.external=",
            "commit.gpgSign=false",
            "tag.gpgSign=false",
            "--no-pager",
            "--no-ext-diff",
            "--no-textconv",
        ] {
            assert!(
                args.iter().any(|arg| arg == required),
                "missing {required:?}"
            );
        }
        assert!(args.iter().any(|arg| arg.starts_with("--work-tree=")));
    }

    #[test]
    fn git_policy_returns_execution_classes_without_fabricated_spawns() {
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            classify_git_request(&argv(&["git", "status"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "branch", "--show-current"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "add", "file"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![GitExecutionCapability::CleanSmudgeFilter]
            }
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "fetch", "origin"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![
                    GitExecutionCapability::RemoteHelper,
                    GitExecutionCapability::Network,
                    GitExecutionCapability::Credential,
                ]
            }
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "remote", "get-url", "origin"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert!(matches!(
            classify_git_request(&argv(&["git", "remote", "show", "origin"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution { .. }
        ));
        assert!(matches!(
            GitEnvPolicy::default().apply_sync(&argv(&["git", "add", "file"]), Path::new(".")),
            Err(GitExecutionPolicyOutcome::BackendUnavailable { .. })
        ));
        let cwd = tempfile::tempdir().expect("non-repository working directory");
        assert!(GitEnvPolicy::default()
            .apply_sync(&argv(&["git", "status"]), cwd.path())
            .is_ok());
        assert!(matches!(
            classify_git_request(&argv(&["git", "show", "--textconv"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution { .. }
        ));
        assert_eq!(
            classify_git_request(&argv(&["git", "possibly-an-alias"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![GitExecutionCapability::RepositoryAlias]
            }
        );
        assert!(matches!(
            classify_git_request(&argv(&["not-git", "status"])),
            GitExecutionPolicyOutcome::InvalidRequest { .. }
        ));
    }

    #[test]
    fn trusted_git_object_read_routes_require_private_view() {
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        for read in [
            argv(&["git", "cat-file", "-p", "HEAD"]),
            argv(&["git", "config", "--blob=HEAD:.gitconfig", "user.name"]),
            argv(&["git", "check-attr", "--source=HEAD", "filter", "--", "file"]),
            argv(&["git", "reflog", "-n", "1"]),
            argv(&["git", "tag", "--list"]),
            argv(&["git", "notes", "show", "HEAD"]),
            argv(&["git", "apply", "--3way", "patch.diff"]),
            argv(&["git", "branch", "--verbose"]),
            argv(&["git", "remote", "show", "--no-fetch", "origin"]),
            argv(&["git", "stash", "list"]),
            argv(&["git", "worktree", "list"]),
            argv(&["git", "update-ref", "refs/heads/example", "HEAD"]),
        ] {
            assert!(
                argv_reads_objects(&read),
                "object-reading route omitted: {read:?}"
            );
        }
    }

    #[test]
    fn git_clean_preview_is_allowed_but_real_clean_requires_destructive_authorization() {
        // C001 corrective: changing this classification is a policy change,
        // not a test update. Dry-run preview (`-n`/`--dry-run`) is read-only
        // and stays allowed; any real clean gates on an explicit
        // `DestructiveRepositoryChange` capability even inside a temp repo.
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "-n"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "-n", "-d"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "--dry-run"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        for real in [
            argv(&["git", "clean", "-f"]),
            argv(&["git", "clean", "-f", "-d"]),
            argv(&["git", "clean", "-f", "-d", "--", "garbage-no-grant.txt"]),
        ] {
            assert_eq!(
                classify_git_request(&real),
                GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
                    capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange]
                },
                "real clean must gate on DestructiveRepositoryChange: {real:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&real, Path::new(".")),
                Err(GitExecutionPolicyOutcome::RequiresAdditionalAuthorization { .. })
            ));
        }
    }

    #[test]
    fn git_reset_modes_require_destructive_authorization() {
        // Reset/remote-test-contract C001: changing reset gating is a policy
        // change, not a test update. Reset has no dry-run mode: every real
        // reset gates on an explicit `DestructiveRepositoryChange`
        // capability even inside a temp repo.
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        for reset in [
            argv(&["git", "reset", "--hard", "HEAD~1"]),
            argv(&["git", "reset", "--soft", "HEAD~1"]),
            argv(&["git", "reset", "--mixed", "HEAD~1"]),
            argv(&["git", "reset", "--merge", "HEAD"]),
            argv(&["git", "reset", "--keep", "HEAD"]),
            argv(&["git", "reset", "HEAD"]),
        ] {
            assert_eq!(
                classify_git_request(&reset),
                GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
                    capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange]
                },
                "reset must gate on DestructiveRepositoryChange: {reset:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&reset, Path::new(".")),
                Err(GitExecutionPolicyOutcome::RequiresAdditionalAuthorization { .. })
            ));
        }
    }

    #[test]
    fn git_push_and_pull_require_contained_remote_backend() {
        // Reset/remote-test-contract C002: remote gating covers every
        // transport, including local paths — a `file://` remote is not a
        // safety grant. Changing this classification is a policy change
        // owned by Linux C003, not a test update. (`fetch` is already
        // pinned in the execution-classes fixture above.)
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        for remote in [
            argv(&["git", "push", "origin", "main"]),
            argv(&["git", "push", "--force-with-lease", "origin", "main"]),
            argv(&["git", "pull", "origin", "main"]),
            argv(&["git", "pull", "--ff-only", "origin", "main"]),
            argv(&["git", "clone", "/tmp/some-local-remote"]),
        ] {
            assert_eq!(
                classify_git_request(&remote),
                GitExecutionPolicyOutcome::RequiresContainedExecution {
                    capabilities: vec![
                        GitExecutionCapability::RemoteHelper,
                        GitExecutionCapability::Network,
                        GitExecutionCapability::Credential,
                    ]
                },
                "remote Git must require the contained backend: {remote:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&remote, Path::new(".")),
                Err(GitExecutionPolicyOutcome::BackendUnavailable { .. })
            ));
        }
    }

    #[test]
    fn trusted_git_policy_denies_caller_config_and_repository_redirects() {
        let cwd = Path::new(".");
        for extra in [
            vec![
                "-c".to_string(),
                "core.fsmonitor=true".to_string(),
                "status".to_string(),
            ],
            vec![
                "--config-env=core.fsmonitor=GIT_CONFIG_COUNT".to_string(),
                "status".to_string(),
            ],
            vec!["-C/other/repo".to_string(), "status".to_string()],
            vec![
                "--git-dir=/other/repo/.git".to_string(),
                "status".to_string(),
            ],
        ] {
            let mut argv = vec!["git".to_string()];
            argv.extend(extra);
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&argv, cwd),
                Err(GitExecutionPolicyOutcome::DeniedExplicitAuthority { .. })
            ));
        }

        // `-c` is also a valid subcommand option (for example `git grep -c`);
        // only global configuration overrides before the subcommand are denied.
        let grep_count = vec!["git".to_string(), "grep".to_string(), "-c".to_string()];
        assert!(GitEnvPolicy::default().apply_sync(&grep_count, cwd).is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_status_does_not_run_repository_fsmonitor() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(root)
            .output()
            .expect("git init");
        assert!(init.status.success());

        let marker = root.join("fsmonitor-ran");
        let hook = root.join("fsmonitor.sh");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf 'token\\n'\n",
                marker.display()
            ),
        )
        .expect("write fsmonitor hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make fsmonitor executable");
        let config = StdCommand::new("git")
            .args([
                "config",
                "core.fsmonitor",
                hook.to_str().expect("hook path"),
            ])
            .current_dir(root)
            .status()
            .expect("set hostile fsmonitor");
        assert!(config.success());

        let status = run(&["status".to_string()], root)
            .await
            .expect("trusted status");
        assert!(status.status.success());
        assert!(!marker.exists(), "repository fsmonitor must not execute");
    }

    #[tokio::test]
    async fn trusted_git_status_cannot_be_redirected_by_local_core_worktree() {
        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("repo");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).expect("repo root");
        std::fs::create_dir(&outside).expect("outside root");
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&root)
            .output()
            .expect("git init");
        assert!(init.status.success());
        std::fs::write(outside.join("host-secret-name.txt"), "outside\n").expect("outside marker");
        let config = StdCommand::new("git")
            .args(["config", "core.worktree"])
            .arg(&outside)
            .current_dir(&root)
            .status()
            .expect("set hostile worktree");
        assert!(config.success());

        let status = run(&["status".to_string()], &root)
            .await
            .expect("trusted status");
        assert!(status.status.success());
        let stdout = String::from_utf8_lossy(&status.stdout);
        assert!(
            !stdout.contains("host-secret-name.txt"),
            "trusted Git must stay bound to the daemon-selected worktree: {stdout}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_status_neutralizes_included_repository_configuration() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("repo");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).expect("repo root");
        std::fs::create_dir(&outside).expect("outside root");
        assert!(StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&root)
            .status()
            .expect("git init")
            .success());

        std::fs::write(outside.join("host-secret-name.txt"), "outside\n").expect("outside marker");
        let marker = fixture.path().join("included-fsmonitor-ran");
        let fsmonitor = fixture.path().join("included-fsmonitor.sh");
        std::fs::write(
            &fsmonitor,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf 'token\\n'\n",
                marker.display()
            ),
        )
        .expect("write included fsmonitor");
        std::fs::set_permissions(&fsmonitor, std::fs::Permissions::from_mode(0o755))
            .expect("make included fsmonitor executable");

        let included = fixture.path().join("hostile-included.config");
        std::fs::write(
            &included,
            format!(
                "[core]\n\tworktree = {}\n\tfsmonitor = {}\n[c009]\n\tincluded = yes\n",
                outside.display(),
                fsmonitor.display()
            ),
        )
        .expect("write included configuration");
        let include_key = "includeIf.onbranch:main.path";
        let configured = StdCommand::new("git")
            .args(["config", "--local", include_key])
            .arg(&included)
            .current_dir(&root)
            .status()
            .expect("configure conditional include");
        assert!(configured.success());

        let included_value = StdCommand::new("git")
            .args(["config", "--local", "--includes", "--get", "c009.included"])
            .current_dir(&root)
            .output()
            .expect("verify include fixture");
        assert!(
            included_value.status.success(),
            "include fixture did not match; key={include_key}; repo config={}",
            std::fs::read_to_string(root.join(".git/config")).unwrap_or_default()
        );
        assert_eq!(
            String::from_utf8_lossy(&included_value.stdout).trim(),
            "yes"
        );

        let status = run(&["status".to_string()], &root)
            .await
            .expect("trusted status");
        assert!(status.status.success());
        let stdout = String::from_utf8_lossy(&status.stdout);
        assert!(
            !stdout.contains("host-secret-name.txt"),
            "included core.worktree must not redirect trusted Git: {stdout}"
        );
        assert!(
            !marker.exists(),
            "included repository fsmonitor must not execute"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_status_ignores_new_nested_repository_configuration() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("selected-repository");
        let nested = root.join("new-nested-repository");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(&nested).expect("nested repo root");
        std::fs::create_dir(&outside).expect("outside root");
        let raw_git = |cwd: &Path, args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .expect("fixture Git command")
        };

        assert!(raw_git(&root, &["init", "--initial-branch=main"])
            .status
            .success());
        assert!(
            raw_git(&root, &["config", "user.email", "test@example.invalid"])
                .status
                .success()
        );
        assert!(raw_git(&root, &["config", "user.name", "C009 Test"])
            .status
            .success());
        std::fs::write(root.join("selected.txt"), "selected root\n")
            .expect("write selected repository file");
        assert!(raw_git(&root, &["add", "selected.txt"]).status.success());
        assert!(raw_git(&root, &["commit", "-m", "selected root"])
            .status
            .success());

        std::fs::write(outside.join("host-secret-name.txt"), "outside\n")
            .expect("write outside marker");
        let marker = fixture.path().join("nested-fsmonitor-ran");
        let fsmonitor = fixture.path().join("nested-fsmonitor.sh");
        std::fs::write(
            &fsmonitor,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf 'token\\n'\n",
                marker.display()
            ),
        )
        .expect("write nested fsmonitor");
        std::fs::set_permissions(&fsmonitor, std::fs::Permissions::from_mode(0o755))
            .expect("make nested fsmonitor executable");
        assert!(raw_git(&nested, &["init", "--initial-branch=main"])
            .status
            .success());
        assert!(raw_git(
            &nested,
            &[
                "config",
                "core.worktree",
                outside.to_str().expect("outside path"),
            ],
        )
        .status
        .success());
        assert!(raw_git(
            &nested,
            &[
                "config",
                "core.fsmonitor",
                fsmonitor.to_str().expect("fsmonitor path"),
            ],
        )
        .status
        .success());

        let status = run(&["status".to_string()], &root)
            .await
            .expect("trusted outer status");
        assert!(status.status.success());
        let stdout = String::from_utf8_lossy(&status.stdout);
        assert!(
            stdout.contains("new-nested-repository/"),
            "nested repository should remain ordinary untracked workspace content: {stdout}"
        );
        assert!(
            !stdout.contains("host-secret-name.txt"),
            "nested repository configuration must not redirect the selected worktree: {stdout}"
        );
        assert!(
            !marker.exists(),
            "nested repository fsmonitor must not execute on the trusted outer route"
        );
    }

    #[tokio::test]
    async fn trusted_git_refuses_repository_object_alternates() {
        let fixture = tempfile::tempdir().expect("fixture");
        let selected = fixture.path().join("selected-repository");
        let alternate = fixture.path().join("alternate-repository");
        std::fs::create_dir(&selected).expect("selected repository");
        std::fs::create_dir(&alternate).expect("alternate repository");
        let raw_git = |cwd: &Path, args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&selected, &["init", "--initial-branch=main"])
            .status
            .success());
        assert!(raw_git(&alternate, &["init", "--initial-branch=main"])
            .status
            .success());
        std::fs::write(alternate.join("outside.txt"), "alternate object content\n")
            .expect("alternate object content");
        assert!(raw_git(&alternate, &["add", "outside.txt"])
            .status
            .success());
        assert!(raw_git(
            &alternate,
            &[
                "-c",
                "user.name=C009",
                "-c",
                "user.email=c009@example.invalid",
                "commit",
                "-m",
                "alternate"
            ]
        )
        .status
        .success());
        let object = raw_git(&alternate, &["rev-parse", "HEAD:outside.txt"]);
        assert!(object.status.success());
        let object = String::from_utf8_lossy(&object.stdout).trim().to_owned();
        let alternates = selected.join(".git/objects/info/alternates");
        std::fs::create_dir_all(alternates.parent().unwrap()).expect("alternate metadata root");
        std::fs::write(
            &alternates,
            format!("{}\n", alternate.join(".git/objects").display()),
        )
        .expect("write repository alternate object directory");

        let branch = run(&["branch".into(), "--show-current".into()], &selected)
            .await
            .expect("ref-only branch lookup does not read object data");
        assert!(branch.status.success());
        assert_eq!(String::from_utf8_lossy(&branch.stdout).trim(), "main");

        assert!(matches!(
            run(&["cat-file".into(), "blob".into(), object], &selected).await,
            Err(EgggitError::Policy(
                GitExecutionPolicyOutcome::BackendUnavailable { capabilities }
            )) if capabilities == vec![GitExecutionCapability::RepositoryAlias]
        ));
    }

    #[test]
    fn trusted_git_read_view_ignores_alternates_created_after_admission() {
        let fixture = tempfile::tempdir().expect("fixture");
        let selected = fixture.path().join("selected-repository");
        let alternate = fixture.path().join("alternate-repository");
        std::fs::create_dir(&selected).expect("selected repository");
        std::fs::create_dir(&alternate).expect("alternate repository");
        let raw_git = |cwd: &Path, args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&selected, &["init", "--initial-branch=main"])
            .status
            .success());
        assert!(raw_git(&alternate, &["init", "--initial-branch=main"])
            .status
            .success());
        std::fs::write(
            alternate.join("outside.txt"),
            "outside object must stay hidden\n",
        )
        .expect("alternate object content");
        assert!(raw_git(&alternate, &["add", "outside.txt"])
            .status
            .success());
        assert!(raw_git(
            &alternate,
            &[
                "-c",
                "user.name=C009",
                "-c",
                "user.email=c009@example.invalid",
                "commit",
                "-m",
                "alternate"
            ]
        )
        .status
        .success());
        let object = raw_git(&alternate, &["rev-parse", "HEAD:outside.txt"]);
        assert!(object.status.success());
        let object = String::from_utf8_lossy(&object.stdout).trim().to_owned();

        // The policy check and command creation are complete before the
        // hostile metadata arrives. Git must use only the private snapshot
        // created for this command, not the now-present repository alternate.
        let argv = vec!["git".into(), "cat-file".into(), "blob".into(), object];
        let mut command = GitEnvPolicy::default()
            .apply_sync(&argv, &selected)
            .expect("trusted Git admission");
        let view = command
            .get_envs()
            .find_map(|(name, value)| {
                (name == "GIT_OBJECT_DIRECTORY")
                    .then(|| value.map(Path::new).map(Path::to_path_buf))
                    .flatten()
            })
            .expect("private object directory configured");
        assert!(view.is_dir());
        assert!(!view.join("info/alternates").exists());

        let alternates = selected.join(".git/objects/info/alternates");
        std::fs::create_dir_all(alternates.parent().unwrap()).expect("alternate metadata root");
        std::fs::write(
            &alternates,
            format!("{}\n", alternate.join(".git/objects").display()),
        )
        .expect("create alternate after admission");
        let output = command.output().expect("spawn trusted Git");
        assert!(
            !output.status.success(),
            "external object was unexpectedly read"
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("outside object must stay hidden"),
            "trusted Git output exposed an object introduced by a raced alternate"
        );
        drop(command);
        assert!(
            view.is_dir(),
            "the daemon cache retains the immutable view for later trusted reads"
        );
    }

    #[test]
    fn trusted_git_object_view_cache_reuses_and_invalidates_repository_generations() {
        let fixture = tempfile::tempdir().expect("fixture");
        let selected = fixture.path().join("selected-repository");
        std::fs::create_dir(&selected).expect("selected repository");
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&selected)
            .output()
            .expect("initialize repository");
        assert!(init.status.success());
        let argv = vec!["git".into(), "status".into()];

        let first = prepare_private_object_view(&argv, &selected)
            .expect("first private view")
            .expect("repository view");
        let first_path = first.path().to_path_buf();
        let second = prepare_private_object_view(&argv, &selected)
            .expect("cached private view")
            .expect("repository view");
        assert_eq!(
            first_path,
            second.path(),
            "matching generation should reuse its view"
        );

        let loose = selected.join(".git/objects/ab");
        std::fs::create_dir_all(&loose).expect("loose object fanout");
        std::fs::write(loose.join("c".repeat(38)), b"new object generation")
            .expect("new loose object");
        let third = prepare_private_object_view(&argv, &selected)
            .expect("view after object addition")
            .expect("repository view");
        assert_ne!(
            first_path,
            third.path(),
            "an admitted namespace change must not reuse the old object generation"
        );
        assert!(
            first.path().is_dir(),
            "an active lease retains its old view"
        );
        assert!(!first.path().join("ab").join("c".repeat(38)).exists());
        assert!(third.path().join("ab").join("c".repeat(38)).is_file());

        let loose_path = loose.join("c".repeat(38));
        std::fs::write(&loose_path, b"mutated object generation")
            .expect("mutate loose object in place");
        let fourth = prepare_private_object_view(&argv, &selected)
            .expect("view after in-place object change")
            .expect("repository view");
        assert_ne!(third.path(), fourth.path());
        assert_eq!(
            std::fs::read(fourth.path().join("ab").join("c".repeat(38)))
                .expect("mutated private object"),
            b"mutated object generation"
        );

        let pack_dir = selected.join(".git/objects/pack");
        std::fs::create_dir_all(&pack_dir).expect("pack directory");
        let pack_path = pack_dir.join(format!("pack-{}.pack", "d".repeat(40)));
        std::fs::write(&pack_path, b"packed object generation").expect("packed object fixture");
        let fifth = prepare_private_object_view(&argv, &selected)
            .expect("view after pack creation")
            .expect("repository view");
        assert!(fifth
            .path()
            .join("pack")
            .join(pack_path.file_name().unwrap())
            .is_file());
        std::fs::write(&pack_path, b"replaced packed object generation")
            .expect("replace pack contents");
        let sixth = prepare_private_object_view(&argv, &selected)
            .expect("view after pack replacement")
            .expect("repository view");
        assert_ne!(fifth.path(), sixth.path());
        assert_eq!(
            std::fs::read(
                sixth
                    .path()
                    .join("pack")
                    .join(pack_path.file_name().unwrap())
            )
            .expect("replaced private pack"),
            b"replaced packed object generation"
        );
    }

    #[test]
    fn concurrent_trusted_git_view_misses_share_one_cached_snapshot() {
        let fixture = tempfile::tempdir().expect("fixture");
        let selected = fixture.path().join("selected-repository");
        std::fs::create_dir(&selected).expect("selected repository");
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&selected)
            .output()
            .expect("initialize repository");
        assert!(init.status.success());
        let selected = Arc::new(selected);
        let mut workers = Vec::new();
        for _ in 0..8 {
            let selected = Arc::clone(&selected);
            workers.push(std::thread::spawn(move || {
                let argv = vec!["git".into(), "status".into()];
                let view = prepare_private_object_view(&argv, &selected)
                    .expect("private view")
                    .expect("repository view");
                view.path().to_path_buf()
            }));
        }
        let paths = workers
            .into_iter()
            .map(|worker| worker.join().expect("cache worker"))
            .collect::<Vec<_>>();
        assert!(paths.iter().all(|path| path == &paths[0]));
    }

    #[test]
    fn trusted_git_read_view_is_immutable_after_admission() {
        let fixture = tempfile::tempdir().expect("fixture");
        let selected = fixture.path().join("selected-repository");
        std::fs::create_dir(&selected).expect("selected repository");
        let raw_git = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(&selected)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&["init", "--initial-branch=main"]).status.success());
        std::fs::write(selected.join("tracked.txt"), "trusted original object\n")
            .expect("tracked file");
        assert!(raw_git(&["add", "tracked.txt"]).status.success());
        assert!(raw_git(&[
            "-c",
            "user.name=C009",
            "-c",
            "user.email=c009@example.invalid",
            "commit",
            "-m",
            "initial"
        ])
        .status
        .success());
        let object = raw_git(&["rev-parse", "HEAD:tracked.txt"]);
        assert!(object.status.success());
        let object = String::from_utf8_lossy(&object.stdout).trim().to_owned();

        #[cfg(target_os = "linux")]
        {
            SNAPSHOT_FICLONE_SUCCESS.store(0, std::sync::atomic::Ordering::Relaxed);
            SNAPSHOT_FICLONE_FALLBACK.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let mut command = GitEnvPolicy::default()
            .apply_sync(
                &[
                    "git".into(),
                    "cat-file".into(),
                    "blob".into(),
                    object.clone(),
                ],
                &selected,
            )
            .expect("trusted Git admission");
        #[cfg(target_os = "linux")]
        if std::env::var_os("CODEGG_REQUIRE_OBJECT_VIEW_FICLONE").is_some() {
            assert!(
                SNAPSHOT_FICLONE_SUCCESS.load(std::sync::atomic::Ordering::Relaxed) > 0,
                "the qualification filesystem did not provide a successful FICLONE"
            );
        }
        let object_path = selected
            .join(".git/objects")
            .join(&object[..2])
            .join(&object[2..]);
        let mut object_permissions = std::fs::metadata(&object_path)
            .expect("loose object metadata")
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            object_permissions.set_mode(object_permissions.mode() | 0o200);
        }
        #[cfg(not(unix))]
        object_permissions.set_readonly(false);
        std::fs::set_permissions(&object_path, object_permissions)
            .expect("make loose object writable for race fixture");
        std::fs::write(&object_path, b"mutated after admission")
            .expect("mutate original loose object");

        let output = command.output().expect("spawn trusted Git");
        assert!(
            output.status.success(),
            "private snapshot should retain the admitted object: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"trusted original object\n");
    }

    #[tokio::test]
    async fn trusted_git_refuses_common_dir_object_alternates() {
        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("main-worktree");
        let linked = fixture.path().join("linked-worktree");
        std::fs::create_dir(&root).expect("main worktree");
        let raw_git = |cwd: &Path, args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&root, &["init", "--initial-branch=main"])
            .status
            .success());
        std::fs::write(root.join("tracked.txt"), "selected worktree\n").expect("tracked file");
        assert!(raw_git(&root, &["add", "tracked.txt"]).status.success());
        assert!(raw_git(
            &root,
            &[
                "-c",
                "user.name=C009",
                "-c",
                "user.email=c009@example.invalid",
                "commit",
                "-m",
                "initial"
            ]
        )
        .status
        .success());
        let linked_arg = linked.to_str().expect("UTF-8 linked worktree path");
        assert!(
            raw_git(&root, &["worktree", "add", "--detach", linked_arg, "HEAD"])
                .status
                .success()
        );
        let common_dir = raw_git(&linked, &["rev-parse", "--git-common-dir"]);
        assert!(common_dir.status.success());
        let common_dir = String::from_utf8_lossy(&common_dir.stdout);
        let common_dir = linked
            .join(common_dir.trim())
            .canonicalize()
            .expect("common dir");
        let alternates = common_dir.join("objects/info/alternates");
        std::fs::create_dir_all(alternates.parent().expect("objects info directory"))
            .expect("create object metadata directory");
        std::fs::write(&alternates, "/outside/trusted/object-store\n")
            .expect("write common-dir object alternate");

        assert!(matches!(
            run(&["status".into()], &linked).await,
            Err(EgggitError::Policy(
                GitExecutionPolicyOutcome::BackendUnavailable { capabilities }
            )) if capabilities == vec![GitExecutionCapability::RepositoryAlias]
        ));
    }

    #[test]
    fn trusted_git_linked_worktree_read_view_ignores_common_alternate_race() {
        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("main-worktree");
        let linked = fixture.path().join("linked-worktree");
        let alternate = fixture.path().join("alternate-repository");
        std::fs::create_dir(&root).expect("main worktree");
        std::fs::create_dir(&alternate).expect("alternate repository");
        let raw_git = |cwd: &Path, args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&root, &["init", "--initial-branch=main"])
            .status
            .success());
        std::fs::write(root.join("selected.txt"), "selected repository\n").expect("selected file");
        assert!(raw_git(&root, &["add", "selected.txt"]).status.success());
        assert!(raw_git(
            &root,
            &[
                "-c",
                "user.name=C009",
                "-c",
                "user.email=c009@example.invalid",
                "commit",
                "-m",
                "selected"
            ]
        )
        .status
        .success());
        assert!(raw_git(
            &root,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD"
            ]
        )
        .status
        .success());
        assert!(raw_git(&alternate, &["init", "--initial-branch=main"])
            .status
            .success());
        std::fs::write(alternate.join("outside.txt"), "outside common-dir object\n")
            .expect("alternate object content");
        assert!(raw_git(&alternate, &["add", "outside.txt"])
            .status
            .success());
        assert!(raw_git(
            &alternate,
            &[
                "-c",
                "user.name=C009",
                "-c",
                "user.email=c009@example.invalid",
                "commit",
                "-m",
                "alternate"
            ]
        )
        .status
        .success());
        let object = raw_git(&alternate, &["rev-parse", "HEAD:outside.txt"]);
        assert!(object.status.success());
        let object = String::from_utf8_lossy(&object.stdout).trim().to_owned();
        let common = raw_git(&linked, &["rev-parse", "--git-common-dir"]);
        assert!(common.status.success());
        let common = linked
            .join(String::from_utf8_lossy(&common.stdout).trim())
            .canonicalize()
            .expect("common dir");

        let argv = vec!["git".into(), "cat-file".into(), "blob".into(), object];
        let mut command = GitEnvPolicy::default()
            .apply_sync(&argv, &linked)
            .expect("trusted Git admission");
        let alternates = common.join("objects/info/alternates");
        std::fs::create_dir_all(alternates.parent().unwrap()).expect("alternate metadata root");
        std::fs::write(
            &alternates,
            format!("{}\n", alternate.join(".git/objects").display()),
        )
        .expect("create common alternate after admission");
        let output = command.output().expect("spawn trusted Git");
        assert!(!output.status.success(), "external common object was read");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("outside common-dir object"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_diff_does_not_run_repository_external_diff_or_textconv() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let marker = root.join("diff-helper-ran");
        let helper = root.join("diff-helper.sh");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .expect("write hostile diff helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("make diff helper executable");

        let raw_git = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("git command")
        };
        assert!(raw_git(&["init", "--initial-branch=main"]).status.success());
        assert!(raw_git(&["config", "user.email", "test@example.invalid"])
            .status
            .success());
        assert!(raw_git(&["config", "user.name", "C009 Test"])
            .status
            .success());
        assert!(
            raw_git(&["config", "diff.hostile.textconv", helper.to_str().unwrap()])
                .status
                .success()
        );
        assert!(
            raw_git(&["config", "diff.external", helper.to_str().unwrap()])
                .status
                .success()
        );
        std::fs::write(root.join(".gitattributes"), "*.txt diff=hostile\n")
            .expect("write hostile diff attributes");
        std::fs::write(root.join("tracked.txt"), "before\n").expect("write tracked file");
        assert!(raw_git(&["add", ".gitattributes", "tracked.txt"])
            .status
            .success());
        assert!(raw_git(&["commit", "-m", "initial"]).status.success());
        std::fs::write(root.join("tracked.txt"), "after\n").expect("modify tracked file");

        let output = run(&["diff".to_string(), "HEAD".to_string()], root)
            .await
            .expect("trusted diff");
        assert!(
            output.status.success(),
            "trusted diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("-before\n+after"),
            "diff should still return the ordinary content change"
        );
        assert!(
            !marker.exists(),
            "repository external-diff and textconv helpers must not execute"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_filter_routes_require_contained_execution_before_helper_launch() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let marker = root.join("filter-helper-ran");
        let helper = root.join("filter-helper.sh");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'; cat\n", marker.display()),
        )
        .expect("write hostile filter helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("make filter helper executable");

        let raw_git = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("fixture Git command")
        };
        assert!(raw_git(&["init", "--initial-branch=main"]).status.success());
        assert!(
            raw_git(&["config", "filter.hostile.clean", helper.to_str().unwrap()])
                .status
                .success()
        );
        std::fs::write(root.join(".gitattributes"), "*.txt filter=hostile\n")
            .expect("write hostile filter attributes");
        std::fs::write(root.join("tracked.txt"), "untrusted content\n")
            .expect("write filter input");

        let outcome = run(&["add".to_string(), "tracked.txt".to_string()], root)
            .await
            .expect_err("filter use must require contained Git execution");
        assert!(
            matches!(
                outcome,
                EgggitError::Policy(GitExecutionPolicyOutcome::BackendUnavailable {
                    ref capabilities
                }) if capabilities == &vec![GitExecutionCapability::CleanSmudgeFilter]
            ),
            "unexpected filter disposition: {outcome:?}"
        );
        assert!(
            !marker.exists(),
            "repository clean filter must not execute on the trusted route"
        );
        assert!(
            !root.join(".git/index").exists(),
            "refused filter operation must not create or mutate the Git index"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_executable_alias_requires_containment_before_helper_launch() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let marker = root.join("alias-helper-ran");
        let helper = root.join("alias-helper.sh");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .expect("write hostile alias helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("make alias helper executable");

        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(root)
            .output()
            .expect("git init");
        assert!(init.status.success());
        let alias = StdCommand::new("git")
            .args(["config", "alias.c009-hostile"])
            .arg(format!("!{}", helper.display()))
            .current_dir(root)
            .status()
            .expect("configure executable alias");
        assert!(alias.success());

        let outcome = run(&["c009-hostile".to_string()], root)
            .await
            .expect_err("repository executable alias must require containment");
        assert!(
            matches!(
                outcome,
                EgggitError::Policy(GitExecutionPolicyOutcome::BackendUnavailable {
                    ref capabilities
                }) if capabilities == &vec![GitExecutionCapability::RepositoryAlias]
            ),
            "unexpected alias disposition: {outcome:?}"
        );
        assert!(
            !marker.exists(),
            "repository executable alias must not execute on the trusted route"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_commit_does_not_run_repository_hooks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let run = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("git command")
        };
        assert!(run(&["init", "--initial-branch=main"]).status.success());
        assert!(run(&["config", "user.email", "test@example.com"])
            .status
            .success());
        assert!(run(&["config", "user.name", "Test"]).status.success());

        let marker = root.join("hook-ran");
        let hook_dir = root.join(".git/hooks");
        let hook = hook_dir.join("post-commit");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
            .expect("write post-commit hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make hook executable");
        std::fs::write(root.join("tracked.txt"), "content\n").expect("write file");
        assert!(run(&["add", "tracked.txt"]).status.success());

        let output = super::run(
            &[
                "commit".to_string(),
                "-m".to_string(),
                "initial".to_string(),
            ],
            root,
        )
        .await
        .expect("trusted commit");
        assert!(output.status.success());
        assert!(
            !marker.exists(),
            "repository post-commit hook must not execute"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_commit_uses_global_identity_without_loading_global_hooks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("repo");
        let global = dir.path().join("user.gitconfig");
        let hooks = dir.path().join("global-hooks");
        std::fs::create_dir_all(&root).expect("repo dir");
        std::fs::create_dir_all(&hooks).expect("hooks dir");
        let marker = dir.path().join("global-hook-ran");
        let hook = hooks.join("post-commit");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
            .expect("write global post-commit hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make global hook executable");
        std::fs::write(
            &global,
            format!(
                "[user]\n\tname = Safe Test User\n\temail = safe@example.invalid\n[core]\n\thooksPath = {}\n",
                hooks.display()
            ),
        )
        .expect("write isolated global config");

        let previous_global = std::env::var_os("GIT_CONFIG_GLOBAL");
        // The repository's nextest profile isolates environment-mutating
        // tests by process. Restore the setting even if an assertion fails.
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
        let _restore_global = RestoreEnvVar::new("GIT_CONFIG_GLOBAL", previous_global);

        let run = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("git command")
        };
        assert!(run(&["init", "--initial-branch=main"]).status.success());
        std::fs::write(root.join("tracked.txt"), "content\n").expect("write tracked file");
        assert!(run(&["add", "tracked.txt"]).status.success());

        let output = super::run(&["commit".into(), "-m".into(), "initial".into()], &root)
            .await
            .expect("trusted commit with global identity");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!marker.exists(), "global hooks must remain disabled");

        let log = super::run(
            &["log".into(), "-1".into(), "--format=%an <%ae>".into()],
            &root,
        )
        .await
        .expect("read committed identity");
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim(),
            "Safe Test User <safe@example.invalid>"
        );
    }

    struct RestoreEnvVar {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl RestoreEnvVar {
        fn new(name: &'static str, previous: Option<std::ffi::OsString>) -> Self {
            Self { name, previous }
        }
    }

    impl Drop for RestoreEnvVar {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    /// Drive a command through `run_command` with a short deadline.
    ///
    /// `git` itself cannot be made to hang reliably from a test without a
    /// hostile hook, so the deadline logic is exercised directly against a
    /// long-running child.
    fn run_sleeping_command(timeout: Duration) -> Result<std::process::Output, EgggitError> {
        let mut cmd = StdCommand::new("sleep");
        cmd.arg("30");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_command(cmd, timeout, "sleep 30")
    }

    #[test]
    fn run_command_times_out_instead_of_waiting_forever() {
        let started = Instant::now();
        let result = run_sleeping_command(Duration::from_millis(300));

        assert!(
            matches!(result, Err(EgggitError::Timeout(_))),
            "a child outliving its deadline must surface as Timeout, got {:?}",
            result.err()
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "the deadline must be enforced promptly, took {elapsed:?}"
        );
    }

    #[test]
    fn run_command_returns_output_for_a_fast_child() {
        let mut cmd = StdCommand::new("sh");
        cmd.arg("-c").arg("printf hello");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = run_command(cmd, Duration::from_secs(10), "printf hello")
            .expect("a fast child must succeed");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello");
    }

    #[test]
    fn run_command_drains_stderr_without_deadlocking() {
        // A child writing more than a pipe buffer holds must not deadlock the
        // wait loop; stdout/stderr are drained concurrently for that reason.
        let mut cmd = StdCommand::new("sh");
        cmd.arg("-c")
            .arg("i=0; while [ $i -lt 2000 ]; do echo 'x' >&2; i=$((i+1)); done; printf done");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = run_command(cmd, Duration::from_secs(20), "large stderr")
            .expect("draining stderr must not deadlock");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "done");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_with_timeout_rejects_a_missing_directory() {
        let missing = PathBuf::from("/nonexistent-codegg-egggit-test");
        let result =
            run_with_timeout(&["status".to_string()], &missing, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(EgggitError::NotARepository(_))));
    }
}
