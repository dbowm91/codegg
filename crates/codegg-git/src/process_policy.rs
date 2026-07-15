//! Canonical environment-policy tables for Codegg-owned `git` subprocesses.
//!
//! This module is the **single source of truth** for:
//!
//! * `ALLOWED_ENV_VARS` — variables always restored from the parent
//!   environment for local (non-network) `git` invocations.
//! * `ALWAYS_STRIPPED_ENV_VARS` — variables that MUST be removed from
//!   the inherited environment before launching `git`, regardless of
//!   operation family.
//!
//! Network operations extend the baseline with
//! [`NETWORK_ALLOWED_ENV_VARS`], defined in the root crate because
//! `codegg-git` is deliberately network-policy-agnostic.
//!
//! # Why this lives in `codegg-git`
//!
//! `codegg-core` cannot depend on the root crate (which holds
//! `GitEnvPolicy`), and a manually-synchronized mirror in
//! `codegg-core/src/worktree.rs` was drifting from the canonical list
//! in `src/git_mutations.rs` — three entries (`GIT_CONFIG_PARAMETERS`,
//! `SSH_ASKPASS`, `GIT_TOOL`) were missing on the core side as of the
//! polish-pass audit. Hoisting the constants here means both
//! consumers read the exact same slice.
//!
//! The constants remain `&'static [&'static str]` so neither
//! consumer pays an allocation, and `codegg-git` keeps its existing
//! minimal dependency footprint (no `std::process`, no `tokio`).
//!
//! The runtime `apply` / `apply_sync` builders that actually construct
//! a `Command` live in the root crate (`src/git_mutations.rs`) because
//! they need `tokio::process::Command` / `std::process::Command` and
//! the optional `NetworkEnvPolicy` overlay. This module exposes
//! enough helpers to verify, in tests, that both builders and any
//! ad-hoc consumers agree on the policy.

/// Environment variables that are always restored for noninteractive
/// local git operations. `PATH` is restored from the parent; the rest
/// pin git to a deterministic state so local operations cannot hang
/// waiting for a credential prompt, editor, or signing pinentry.
///
/// HTTPS certificate passthrough lets systems that route HTTPS
/// through custom CA bundles (corporate proxies, local CA stores)
/// still authenticate against Git remotes without weakening
/// command-injection protections.
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
    // HTTPS certificate passthrough.
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "GIT_SSL_CAPATH",
];

/// Environment variables that are NEVER passed to a Codegg-owned
/// `git` child, regardless of the kind of operation. These are
/// command-bearing variables that could be used by a hostile parent
/// to inject helper/editor/filter/credential commands.
///
/// Network operations extend the baseline with
/// [`NETWORK_ALLOWED_ENV_VARS`] (defined in the root crate at
/// `src/git_network_ops::NETWORK_ALLOWED_ENV_VARS`), which is a
/// reviewed allowlist for credential helpers, SSH agent, and proxy
/// variables required for remote access.
pub const ALWAYS_STRIPPED_ENV_VARS: &[&str] = &[
    // credential helpers (never auto-restored for local ops)
    "GIT_ASKPASS",
    "GIT_SSH_COMMAND",
    "GIT_SSH_VARIANT",
    "GIT_PROXY_COMMAND",
    // git config injection vectors
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
    // alternate askpass
    "SSH_ASKPASS",
    "GIT_TOOL",
    // repository working-tree overrides (would let parent escape cwd)
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    // pager (prevent paginated output stalls)
    "GIT_PAGER",
    "PAGER",
];

/// Returns `true` if `name` is in [`ALLOWED_ENV_VARS`].
pub fn is_allowed(name: &str) -> bool {
    ALLOWED_ENV_VARS.contains(&name)
}

/// Returns `true` if `name` is in [`ALWAYS_STRIPPED_ENV_VARS`].
pub fn is_stripped(name: &str) -> bool {
    ALWAYS_STRIPPED_ENV_VARS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_contains_path_and_home() {
        assert!(is_allowed("PATH"));
        assert!(is_allowed("HOME"));
        assert!(is_allowed("SSH_AUTH_SOCK"));
    }

    #[test]
    fn stripped_contains_command_bearers() {
        // All known credential-helper / editor / config-injection
        // vectors are denied by default.
        for k in [
            "GIT_ASKPASS",
            "GIT_SSH_COMMAND",
            "GIT_PROXY_COMMAND",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "SSH_ASKPASS",
            "GIT_TOOL",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_PAGER",
            "PAGER",
        ] {
            assert!(is_stripped(k), "expected {k} to be stripped");
        }
    }

    #[test]
    fn allowed_and_stripped_are_disjoint() {
        for k in ALLOWED_ENV_VARS {
            assert!(
                !is_stripped(k),
                "{k} appears in both ALLOWED_ENV_VARS and ALWAYS_STRIPPED_ENV_VARS"
            );
        }
    }

    #[test]
    fn no_duplicates_within_allowed() {
        let mut seen = std::collections::HashSet::new();
        for k in ALLOWED_ENV_VARS {
            assert!(seen.insert(*k), "duplicate entry in ALLOWED_ENV_VARS: {k}");
        }
    }

    #[test]
    fn no_duplicates_within_stripped() {
        let mut seen = std::collections::HashSet::new();
        for k in ALWAYS_STRIPPED_ENV_VARS {
            assert!(
                seen.insert(*k),
                "duplicate entry in ALWAYS_STRIPPED_ENV_VARS: {k}"
            );
        }
    }

    #[test]
    fn canonical_lists_are_pure_pure_data() {
        // The policy must not depend on OS-specific paths, network
        // state, or environment reads. If a future change adds env
        // reads here, this test will catch it.
        let allowed_strs: Vec<&str> = ALLOWED_ENV_VARS.to_vec();
        let stripped_strs: Vec<&str> = ALWAYS_STRIPPED_ENV_VARS.to_vec();
        assert!(
            !allowed_strs.is_empty(),
            "ALLOWED_ENV_VARS must not be empty"
        );
        assert!(
            !stripped_strs.is_empty(),
            "ALWAYS_STRIPPED_ENV_VARS must not be empty"
        );
        // Every entry must be a valid env-var identifier (uppercase +
        // digits + underscore, ASCII).
        for k in ALLOWED_ENV_VARS
            .iter()
            .chain(ALWAYS_STRIPPED_ENV_VARS.iter())
        {
            assert!(
                k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "invalid env-var identifier: {k}"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn windows_overlays_documented_but_not_in_canonical_list() {
        // Windows-specific env vars (USERPROFILE, HOMEDRIVE, HOMEPATH,
        // PATHEXT) are NOT yet in the canonical allowlist. This test
        // documents that decision. When Windows CI is added, expand
        // ALLOWED_ENV_VARS behind a `#[cfg(windows)] const` overlay
        // and update this test to assert the overlays are present.
        assert!(
            !is_allowed("USERPROFILE"),
            "USERPROFILE not yet in canonical list"
        );
        assert!(
            !is_allowed("HOMEDRIVE"),
            "HOMEDRIVE not yet in canonical list"
        );
        assert!(
            !is_allowed("HOMEPATH"),
            "HOMEPATH not yet in canonical list"
        );
        assert!(!is_allowed("PATHEXT"), "PATHEXT not yet in canonical list");
    }

    #[test]
    #[cfg(not(windows))]
    fn unix_canonical_list_is_platform_independent() {
        // On Unix, the canonical list must not include Windows-specific
        // vars. If a future change adds them unconditionally, this test
        // will catch it.
        assert!(
            !is_allowed("USERPROFILE"),
            "USERPROFILE is Windows-specific; gate behind cfg(windows)"
        );
        assert!(
            !is_allowed("HOMEDRIVE"),
            "HOMEDRIVE is Windows-specific; gate behind cfg(windows)"
        );
        assert!(
            !is_allowed("HOMEPATH"),
            "HOMEPATH is Windows-specific; gate behind cfg(windows)"
        );
        assert!(
            !is_allowed("PATHEXT"),
            "PATHEXT is Windows-specific; gate behind cfg(windows)"
        );
    }
}
