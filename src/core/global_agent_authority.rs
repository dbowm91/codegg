//! The user-level agent directory as a **distinct** document authority.
//!
//! A global agent config lives outside every workspace, so workspace
//! containment can never reach it. Rather than weaken that containment —
//! which would make `..` or an absolute path meaningful against a file that
//! is by definition not project data — this module establishes a second,
//! separately named root with its own write gate and a **narrower** allowed
//! filename set.
//!
//! ## The global scope is stricter than the workspace scope
//!
//! `DocumentScope::Workspace` accepts any workspace-relative path that stays
//! under the workspace root. `DocumentScope::GlobalAgent` accepts exactly
//! one shape: a bare `<name>.toml` directly inside the agent directory.
//! No subdirectories, no traversal, no absolute paths, no other extension,
//! and a bounded name. The narrowness is enforced by
//! [`validate`], which runs *before* the shared filesystem checks — the
//! shared checks then confirm the result really is inside the root.
//!
//! Every disk write for a global agent still goes through
//! [`crate::lsp::mutation`]'s checked create/write/delete, exactly as a
//! workspace write does. This module adds the root and the gate; it never
//! replaces a containment check with a cheaper one.
//!
//! ## Why there is no admin authorization yet
//!
//! Today one daemon serves one OS user, so "which user owns this file" and
//! "who is asking" are the same question and an explicit authorization tier
//! would be theatre. The forward contract is recorded here deliberately:
//! resolving a global root is the seam where an admin capability must be
//! required once one daemon serves multiple agents. Every caller below
//! resolves through [`ensure_root`] or [`existing_root`], so adding that
//! check later is additive rather than a rewrite of the containment rules.
//!
//! ## Future: one daemon, multiple agents
//!
//! Under a multi-agent daemon the global root is *shared* state that one
//! agent's edit changes for every other agent and for the user. The
//! authorization requirement is therefore per-request, not per-document: an
//! admin capability must be present on `DocumentOpen`, `DocumentCreate`,
//! `DocumentDelete`, and on the save/reload path, and the audit trail must
//! distinguish a global mutation from a workspace one so an operator can
//! attribute it. The `DocumentScope` carried on every request and snapshot is
//! the discriminator both of those checks need, and it is deliberately
//! present on the wire rather than inferred server-side.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use codegg_protocol::document::DocumentScope;

/// Maximum directory components created when materializing the root.
///
/// The root is a fixed, shallow, platform-derived location. A bound this
/// small means a misconfigured `config_dir` cannot be turned into broad
/// directory construction.
const MAX_ROOT_DEPTH: usize = 8;

/// Process-wide write gate for the global agent directory.
///
/// The root is process-global by construction — it comes from
/// `dirs::config_dir()` and is not project-scoped — so a process-global
/// gate is the correct granularity. Workspace writes use the per-workspace
/// repository lock instead; taking that lock for a global path would
/// serialize unrelated projects against a file none of them own.
static GLOBAL_AGENT_GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// The gate that serializes every global agent mutation.
pub(crate) fn write_gate() -> &'static tokio::sync::Mutex<()> {
    GLOBAL_AGENT_GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// The user-level agent root, or `None` when the platform has no config dir.
///
/// Delegates to the agent subsystem's own resolver so there is exactly one
/// definition of where global agents live; a second copy would drift.
pub(crate) fn root() -> Option<PathBuf> {
    crate::agent::asset_context::default_global_agents_root()
}

/// The configured root without touching the filesystem.
pub(crate) fn configured_root() -> Result<PathBuf, String> {
    root().ok_or_else(|| {
        "this platform has no user config directory, so there are no global agents".to_string()
    })
}

/// Canonical root, created if missing.
///
/// The root itself is created here rather than by the shared create helper
/// because that helper creates *parents under an existing root* and refuses
/// to walk above one — correct for a workspace, but it would reject
/// materializing a root that does not exist yet. Only the root is created;
/// every file inside it still goes through the shared checked helpers.
pub(crate) async fn ensure_root() -> Result<PathBuf, String> {
    let configured = configured_root()?;
    create_root(&configured)?;
    let canonical = tokio::fs::canonicalize(&configured)
        .await
        .map_err(|error| format!("global agent directory: {error}"))?;
    // Refuse a root reached through a symlink. The shared write checks
    // resolve every component of a document path and would reject the
    // document anyway; failing here instead makes the cause obvious at the
    // boundary rather than at the first save.
    crate::tool::util::check_path_for_symlinks(&canonical)
        .map_err(|error| format!("global agent directory: {error}"))?;
    Ok(canonical)
}

/// Canonical root, or an error when it does not exist.
///
/// Used by the read paths (open/reload) so "you have no global agents" is a
/// clear message rather than a missing-file IO error from deeper down.
pub(crate) async fn existing_root() -> Result<PathBuf, String> {
    let configured = configured_root()?;
    let canonical = tokio::fs::canonicalize(&configured)
        .await
        .map_err(|_| "the global agent directory does not exist yet".to_string())?;
    crate::tool::util::check_path_for_symlinks(&canonical)
        .map_err(|error| format!("global agent directory: {error}"))?;
    Ok(canonical)
}

/// Whether `root` is shallow enough to materialize.
///
/// Separate from [`create_root`] so the bound is a pure predicate that can be
/// tested without writing to the filesystem.
pub(crate) fn within_depth_bound(root: &Path) -> bool {
    root.components().count() <= MAX_ROOT_DEPTH
}

/// Create `root` and any missing ancestors, at most [`MAX_ROOT_DEPTH`] deep.
///
/// Each level is created with `create_dir` (which fails rather than
/// descending) and then canonicalized, so a directory that is a symlink is
/// detected at the level it appears instead of being trusted because its
/// parent was fine. A concurrent creator winning the race is success.
fn create_root(root: &Path) -> Result<(), String> {
    if !within_depth_bound(root) {
        return Err(format!(
            "global agent directory is deeper than {MAX_ROOT_DEPTH} components"
        ));
    }
    if root.is_dir() {
        return Ok(());
    }
    if let Some(parent) = root.parent() {
        create_root(parent)?;
    }
    match std::fs::create_dir(root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!("global agent directory: {error}")),
    }
}

/// Validate a document path against the global-agent rule.
///
/// Thin, total wrapper over [`DocumentScope::validate_global_agent_filename`]
/// so the daemon has one named seam for "is this even an agent config?"
/// and the rule itself stays with the protocol contract it travels in.
pub(crate) fn validate(relative_path: &str) -> Result<String, String> {
    DocumentScope::GlobalAgent
        .validate_global_agent_filename(relative_path)
        .map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_gate_is_a_single_instance() {
        // Two acquisitions of "the gate" that were actually two locks would
        // silently permit concurrent writes to the same file.
        let a = write_gate() as *const tokio::sync::Mutex<()>;
        let b = write_gate() as *const tokio::sync::Mutex<()>;
        assert_eq!(a, b);
    }

    #[test]
    fn only_a_flat_agent_filename_is_accepted() {
        assert!(validate("reviewer.toml").is_ok());
        // Everything below is refused by name, before any filesystem work.
        assert!(validate("nested/reviewer.toml").is_err());
        assert!(validate("../reviewer.toml").is_err());
        assert!(validate("/etc/passwd.toml").is_err());
        assert!(validate("reviewer.txt").is_err());
        assert!(validate("").is_err());
    }

    #[test]
    fn a_root_deeper_than_the_bound_is_refused() {
        // A platform root of the expected shape is inside the bound.
        assert!(within_depth_bound(Path::new(
            "/Users/someone/Library/Application Support/codegg/agents"
        )));
        // One level deeper is not, so a misconfigured `config_dir` cannot be
        // turned into broad directory construction.
        let too_deep: PathBuf = (0..=MAX_ROOT_DEPTH)
            .map(|index| format!("d{index}"))
            .collect();
        assert!(!within_depth_bound(&too_deep));
    }
}
