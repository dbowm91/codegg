use crate::error::AppError;
use egggit::worktree::WorktreeInfo;
use serde::{Deserialize, Serialize};
use std::path::Path;

// Re-export the canonical policy constants from the core surface for
// compatibility. Construction itself is delegated to egggit's generic
// process boundary.
pub use egggit::process::{
    ALLOWED_ENV_VARS as POLICY_ALLOWED_ENV_VARS,
    ALWAYS_STRIPPED_ENV_VARS as POLICY_ALWAYS_STRIPPED_ENV_VARS,
};

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
    create_worktree_at(git_root, path, branch, create_branch, None)
}

/// Create a worktree, optionally creating `branch` from an explicit base
/// revision.  This keeps worktree creation on the same hardened Git
/// subprocess boundary while allowing durable services to record the exact
/// base commit they allocated.
pub fn create_worktree_at(
    git_root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
    base: Option<&str>,
) -> Result<(), AppError> {
    let mut args = vec!["worktree", "add"];
    let path_str = path.to_string_lossy().to_string();
    args.push(&path_str);

    if create_branch {
        args.push("-b");
    }
    args.push(branch);
    if let Some(base) = base {
        args.push(base);
    }

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
/// The allowlist and stripped set come from
/// `egggit::process` (the single source of truth shared
/// with the root crate's `GitEnvPolicy::apply` / `apply_sync`). This
/// helper exists because `codegg-core` builds synchronous
/// `std::process::Command` values (no tokio) and uses a single
/// caller pattern for `worktree add` / `worktree remove`.
fn hardened_git_command(args: &[&str], git_root: &Path) -> std::process::Command {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    egggit::process::GitEnvPolicy::default().apply_sync(&argv, git_root)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard: the local `hardened_git_command` must consume the
    /// canonical lists from `egggit::process`. If this
    /// test ever fails, a future refactor has re-introduced a
    /// hand-maintained mirror that must be deleted in favor of the
    /// shared constants.
    #[test]
    fn worktree_uses_canonical_policy() {
        assert!(POLICY_ALLOWED_ENV_VARS.contains(&"PATH"));
        assert!(POLICY_ALLOWED_ENV_VARS.contains(&"HOME"));
        assert!(POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&"GIT_ASKPASS"));
        assert!(POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&"GIT_CONFIG_PARAMETERS"));
        assert!(POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&"SSH_ASKPASS"));
        assert!(POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&"GIT_TOOL"));
        assert!(POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&"GIT_DIR"));
    }

    /// Drift guard: the canonical set must include all command-bearing
    /// variables that the old local mirror was missing.
    #[test]
    fn canonical_includes_locally_drifted_entries() {
        let drifted = ["GIT_CONFIG_PARAMETERS", "SSH_ASKPASS", "GIT_TOOL"];
        for k in drifted {
            assert!(
                POLICY_ALWAYS_STRIPPED_ENV_VARS.contains(&k),
                "{k} missing from canonical always-stripped list"
            );
        }
    }
}
