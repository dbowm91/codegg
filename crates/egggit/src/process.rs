//! Hardened, shell-free Git process construction.
//!
//! This is the generic Git execution boundary. It owns the environment
//! policy and the process plumbing shared by structured read operations and
//! callers that need a synchronous Git command. It does not know about
//! CodeGG sessions, runs, jobs, permissions, or projections.

use crate::EgggitError;
use std::path::Path;
use std::process::{Command as StdCommand, Output as StdOutput};
use tokio::process::Command;

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

/// Base policy for local Git operations.
#[derive(Debug, Clone)]
pub struct GitEnvPolicy {
    pub terminal_prompt_disabled: bool,
    pub pin_editor: bool,
    pub strip_editors: bool,
    pub strip_command_bearers: bool,
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
    pub fn apply(&self, argv: &[String], cwd: &Path) -> Command {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        apply_environment(&mut cmd, self);
        cmd.kill_on_drop(true);
        cmd
    }

    /// Construct a synchronous command from a complete argv (`git` included).
    pub fn apply_sync(&self, argv: &[String], cwd: &Path) -> StdCommand {
        let mut cmd = StdCommand::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        apply_environment(&mut cmd, self);
        cmd
    }
}

fn apply_environment<C>(cmd: &mut C, policy: &GitEnvPolicy)
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
pub async fn run(args: &[String], cwd: &Path) -> Result<StdOutput, EgggitError> {
    if !cwd.exists() {
        return Err(EgggitError::NotARepository(cwd.display().to_string()));
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend_from_slice(args);
    let cwd = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        GitEnvPolicy::default()
            .apply_sync(&argv, &cwd)
            .output()
            .map_err(|error| EgggitError::Io(error.to_string()))
    })
    .await
    .map_err(|error| EgggitError::Join(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
