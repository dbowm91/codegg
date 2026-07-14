use crate::error::AppError;
use egggit::worktree::WorktreeInfo;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}

fn into_legacy(info: WorktreeInfo) -> Worktree {
    Worktree {
        path: info.path,
        branch: info.branch,
        is_current: info.is_current,
        is_detached: info.is_detached,
    }
}

pub async fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError> {
    let infos = egggit::worktree::list_worktrees(git_root)
        .await
        .map_err(|e| AppError::Worktree(format!("failed to list worktrees: {e}")))?;
    Ok(infos.into_iter().map(into_legacy).collect())
}

pub fn create_worktree(
    git_root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
) -> Result<(), AppError> {
    let mut args = vec!["worktree", "add"];
    let path_str = path.to_string_lossy().to_string();
    args.push(&path_str);

    if create_branch {
        args.push("-b");
    }
    args.push(branch);

    let output = hardened_git_command(&args, git_root)
        .output()
        .map_err(|e| AppError::Worktree(format!("failed to create worktree: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Worktree(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

pub fn remove_worktree(git_root: &Path, path: &Path, force: bool) -> Result<(), AppError> {
    let path_str = path.to_string_lossy().to_string();
    let mut args = vec!["worktree", "remove", &path_str];
    if force {
        args.push("--force");
    }
    let output = hardened_git_command(&args, git_root)
        .output()
        .map_err(|e| AppError::Worktree(format!("failed to remove worktree: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Worktree(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Build a `git` subprocess with a hardened environment: explicitly
/// allowed vars only, command-bearing `GIT_*` vars stripped, pin
/// `GIT_TERMINAL_PROMPT=0` to block credential-prompt hangs, and
/// pin `GIT_PAGER=cat`/`PAGER=cat` to avoid paginator stalls.
///
/// This mirrors the root crate's `GitEnvPolicy::apply_sync` shape, but
/// kept local because `codegg-core` cannot depend on root-crate
/// helpers. The two lists are kept in sync manually.
fn hardened_git_command(args: &[&str], git_root: &Path) -> std::process::Command {
    const ALLOWED_ENV_VARS: &[&str] = &[
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
    ];
    const STRIPPED_ENV_VARS: &[&str] = &[
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
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PAGER",
        "PAGER",
    ];
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(git_root).env_clear();
    for key in ALLOWED_ENV_VARS {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    for key in STRIPPED_ENV_VARS {
        cmd.env_remove(key);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_EDITOR", "true");
    cmd.env("GIT_SEQUENCE_EDITOR", "true");
    cmd.env_remove("EDITOR");
    cmd.env_remove("VISUAL");
    cmd.env("GPG_TTY", "");
    cmd.env("GIT_PAGER", "cat");
    cmd.env("PAGER", "cat");
    cmd
}

pub fn find_git_root(start: &Path) -> Option<std::path::PathBuf> {
    egggit::worktree::find_git_root(start)
}

pub fn is_git_file(git_path: &Path) -> bool {
    egggit::worktree::is_git_file(git_path)
}

pub fn is_git_worktree(dir: &Path) -> bool {
    egggit::worktree::is_git_worktree(dir)
}
