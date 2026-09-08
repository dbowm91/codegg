//! Canonical model-visible tool disclosure classification (M002).
//!
//! This module is the single authoritative source for *prompt disclosure*:
//! which registered tools are advertised immediately on an ordinary coding
//! turn, which remain registered but deferred/discoverable via
//! `tool_search`, which are immediate only for specific agent roles, and
//! which are never model-visible.
//!
//! Registration remains owned by `ToolRegistry::with_options`; invocation
//! remains owned by the broker/tool contracts and scheduler boundaries.
//! Disclosure never widens authority: a deferred or profile-specific tool
//! is still filtered through agent/session/broker/permission policy, and
//! `tool_search` only reveals tools the current policy allows.
//!
//! States (see `plans/.../002-model-visible-tool-surface-minimization.md`):
//!
//! - `Core`: ordinary coding turn immediate (direct edit-loop primitives,
//!   plus `tool_search` itself).
//! - `Deferred`: registered and callable, discoverable via `tool_search`,
//!   deferred from the initial definitions (specialist/research/evidence
//!   variants, overlapping helpers).
//! - `ProfileSpecific`: deferred by default, immediate only for named
//!   specialist agent roles that require it.
//! - `Hidden`: never model-visible (diagnostic/internal catch-alls).
//!
//! Runtime-unavailable tools (disabled backends, task without a functional
//! spawner) are a separate state owned by `expose_in_definitions()` and
//! `has_functional_backend()` plus the resolved surface omissions. They
//! must not be advertised as merely deferred.

/// Canonical disclosure state for one tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolDisclosure {
    /// Ordinary coding turn immediate.
    Core,
    /// Registered, discoverable via `tool_search`, deferred initially.
    Deferred,
    /// Deferred by default; immediate only for specific agent roles.
    ProfileSpecific,
    /// Never model-visible.
    Hidden,
}

impl ToolDisclosure {
    /// Stable string label for diagnostics and `tool_search` metadata.
    pub fn as_str(self) -> &'static str {
        match self {
            ToolDisclosure::Core => "core",
            ToolDisclosure::Deferred => "deferred",
            ToolDisclosure::ProfileSpecific => "profile_specific",
            ToolDisclosure::Hidden => "hidden",
        }
    }
}

/// Classify one tool name.
///
/// Unknown names (MCP `mcp__*`, plugin tools, future natives) default to
/// `Core` so disclosure never accidentally hides a capability it does not
/// own. Only the explicit match arms below participate in minimization.
pub fn disclosure_for(name: &str) -> ToolDisclosure {
    match name {
        // Internal catch-all: never a model-called tool.
        "invalid" => ToolDisclosure::Hidden,
        // Specialist synthesis / evidence / fetch variants: deferred,
        // discoverable via `tool_search`. `repo_search` stays core as the
        // single canonical repo-inspect primitive; `codesearch` remains
        // registered as the M001 compatibility alias but deferred.
        "research" | "research_search" | "repo_fetch" | "repo_map" | "security_search"
        | "batch_fetch" | "evidence_bundle" | "codesearch" | "review" | "image" | "terminal"
        | "skill_proposal" | "security" | "replace" | "commit" | "python_script"
        | "tool_program" => ToolDisclosure::Deferred,
        // Eggsact deferred validators (already `defer_loading = true`).
        "text_inspect"
        | "config_preflight"
        | "identifier_inspect"
        | "structured_data_compare"
        | "text_fingerprint" => ToolDisclosure::Deferred,
        // Deferred by default; immediate for the specialist roles below.
        // (Currently all profile-specific tools are also in the Deferred
        // match above; this arm documents the role-override contract for
        // future tools that need it. Kept for M002 interface stability
        // and M005 construction-seam consumption.)
        _ if is_profile_specific_name(name) => ToolDisclosure::ProfileSpecific,
        _ => ToolDisclosure::Core,
    }
}

/// Names that have a role-specific immediate override.
///
/// These are a subset of the deferred set: they remain deferred for
/// ordinary coding but become immediate for the agent named in
/// [`immediate_for_agent`]. Listed separately so the override contract is
/// explicit even though the default disclosure is `Deferred`.
fn is_profile_specific_name(name: &str) -> bool {
    matches!(
        name,
        "research"
            | "research_search"
            | "repo_fetch"
            | "repo_map"
            | "batch_fetch"
            | "evidence_bundle"
            | "codesearch"
            | "security"
            | "security_search"
    )
}

/// Whether a tool is deferred from initial definitions by default.
///
/// Backed by [`disclosure_for`]; per-tool `defer_loading()` overrides
/// should delegate here so classification has one owner.
pub fn is_deferred_by_default(name: &str) -> bool {
    matches!(
        disclosure_for(name),
        ToolDisclosure::Deferred | ToolDisclosure::ProfileSpecific
    )
}

/// Whether a tool is never model-visible.
pub fn is_hidden(name: &str) -> bool {
    matches!(disclosure_for(name), ToolDisclosure::Hidden)
}

/// Whether a tool is immediate on an ordinary coding turn.
pub fn is_core(name: &str) -> bool {
    matches!(disclosure_for(name), ToolDisclosure::Core)
}

/// Canonical core palette for ordinary coding turns.
///
/// Direct primitives for the common edit loop (inspect files/repo,
/// edit/write/apply patch, controlled shell/test/Git/task/delegation),
/// plus `tool_search` itself. Specialist synthesis/evidence/fetch variants
/// are intentionally absent: they remain registered and discoverable.
///
/// This list owns the curated/minimal exposure subsets below; loop-level
/// filters must consume these constants rather than re-declaring names.
pub const CORE_PALETTE: &[&str] = &[
    "bash",
    "read",
    "edit",
    "write",
    "glob",
    "grep",
    "list",
    "diff",
    "apply_patch",
    "task",
    "test",
    "git",
    "question",
    "skill",
    "todoread",
    "todowrite",
    "plan_enter",
    "plan_exit",
    "tool_search",
    "websearch",
    "webfetch",
    "repo_search",
    "lsp",
    // Deterministic pre-edit validators (eggsact always-visible).
    "text_equal",
    "text_diff_explain",
    "text_replace_check",
    "validate_json",
    "validate_toml",
    "command_preflight",
    "path_normalize",
    "text_security_inspect",
    // `context_read` is core when registered (artifact expansion).
    "context_read",
];

/// Curated exposure for frontier/reasoning profiles.
///
/// Subset of [`CORE_PALETTE`] preserving the pre-M002 curated shape but
/// advertising the canonical `repo_search` instead of the `codesearch`
/// alias. The alias remains registered and discoverable.
pub const CURATED_PALETTE: &[&str] = &[
    "read",
    "list",
    "grep",
    "glob",
    "repo_search",
    "edit",
    "apply_patch",
    "bash",
    "git",
    "diff",
    "todoread",
    "todowrite",
    "question",
    "tool_search",
    "skill",
    "websearch",
];

/// Minimal exposure for fast/fragile/local profiles.
///
/// Subset of [`CORE_PALETTE`]; deferred capability remains discoverable
/// via `tool_search`, which is always retained.
pub const MINIMAL_PALETTE: &[&str] = &[
    "read",
    "list",
    "grep",
    "repo_search",
    "edit",
    "apply_patch",
    "bash",
    "question",
    "todowrite",
    "todoread",
    "tool_search",
    "websearch",
];

/// Canonical plan-mode allowed surface.
///
/// Read-only inspection plus planning/state operations. Includes both the
/// canonical `repo_search` and the retained `codesearch` alias (M001) so
/// existing plan-mode configs keep working, plus `tool_search` so deferred
/// capability remains discoverable in plan mode. Mutating tools remain
/// excluded.
pub const PLAN_ALLOWED: &[&str] = &[
    "read",
    "glob",
    "grep",
    "list",
    "codesearch",
    "repo_search",
    "webfetch",
    "lsp",
    "skill",
    "todoread",
    "todowrite",
    "bash",
    "plan_enter",
    "plan_exit",
    "tool_search",
];

/// Whether a tool is allowed in plan mode.
pub fn plan_allowed(name: &str) -> bool {
    PLAN_ALLOWED.contains(&name)
}

/// Deferred tools that become immediate for the `research` specialist role.
///
/// The research agent denies mutation/shell but needs both primitive
/// search and synthesis/evidence tools immediately; ordinary coding keeps
/// them deferred.
pub const RESEARCH_AGENT_IMMEDIATE: &[&str] = &[
    "research",
    "research_search",
    "repo_search",
    "repo_fetch",
    "repo_map",
    "batch_fetch",
    "evidence_bundle",
    "codesearch",
];

/// Deferred tools that become immediate for the `security-review` role.
pub const SECURITY_AGENT_IMMEDIATE: &[&str] = &["security", "security_search"];

/// Deferred tools that become immediate for the hidden `verifier` role.
pub const VERIFIER_AGENT_IMMEDIATE: &[&str] = &["evidence_bundle"];

/// Whether a deferred tool is immediate for a specific agent role.
///
/// Ordinary coding agents receive `false` (deferred); the named
/// specialist roles receive `true` for their role-appropriate palette.
/// This never widens authority: the tool must still pass agent deny,
/// plan-mode, model-disable, backend-availability, and parent-ceiling
/// checks in the resolved surface.
pub fn immediate_for_agent(tool_name: &str, agent_name: &str) -> bool {
    match agent_name {
        "research" => RESEARCH_AGENT_IMMEDIATE.contains(&tool_name),
        "security-review" => SECURITY_AGENT_IMMEDIATE.contains(&tool_name),
        "verifier" => VERIFIER_AGENT_IMMEDIATE.contains(&tool_name),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn core_palette_contains_edit_loop_primitives_and_search() {
        let set: BTreeSet<&str> = CORE_PALETTE.iter().copied().collect();
        for required in [
            "read",
            "glob",
            "grep",
            "list",
            "edit",
            "write",
            "apply_patch",
            "bash",
            "test",
            "git",
            "task",
            "tool_search",
            "websearch",
            "webfetch",
            "repo_search",
        ] {
            assert!(set.contains(required), "core palette missing {required}");
        }
        // Canonical repo inspect is core; the compatibility alias is not.
        assert!(is_core("repo_search"));
        assert!(!is_core("codesearch"));
    }

    #[test]
    fn research_evidence_specialists_are_deferred_by_default() {
        for name in [
            "research",
            "research_search",
            "repo_fetch",
            "repo_map",
            "security_search",
            "batch_fetch",
            "evidence_bundle",
            "codesearch",
            "review",
            "image",
            "terminal",
            "skill_proposal",
            "security",
            "replace",
            "commit",
            "python_script",
            "tool_program",
        ] {
            assert!(
                is_deferred_by_default(name),
                "{name} should be deferred by default"
            );
            assert!(
                !is_core(name),
                "{name} should not be in the core immediate set"
            );
        }
    }

    #[test]
    fn invalid_is_hidden_not_deferred() {
        assert_eq!(disclosure_for("invalid"), ToolDisclosure::Hidden);
        assert!(is_hidden("invalid"));
        assert!(!is_deferred_by_default("invalid"));
    }

    #[test]
    fn unknown_future_tools_default_to_core() {
        // MCP/plugin/future natives must not be accidentally hidden.
        assert_eq!(disclosure_for("mcp__server__tool"), ToolDisclosure::Core);
        assert_eq!(disclosure_for("some_future_tool"), ToolDisclosure::Core);
    }

    #[test]
    fn curated_and_minimal_are_core_subsets_with_canonical_search() {
        let core: BTreeSet<&str> = CORE_PALETTE.iter().copied().collect();
        for name in CURATED_PALETTE.iter().chain(MINIMAL_PALETTE.iter()) {
            assert!(core.contains(name), "{name} must be a core subset");
        }
        assert!(CURATED_PALETTE.contains(&"repo_search"));
        assert!(MINIMAL_PALETTE.contains(&"repo_search"));
        assert!(!CURATED_PALETTE.contains(&"codesearch"));
        assert!(!MINIMAL_PALETTE.contains(&"codesearch"));
        // `tool_search` itself is always retained where deferral exists.
        assert!(CURATED_PALETTE.contains(&"tool_search"));
        assert!(MINIMAL_PALETTE.contains(&"tool_search"));
    }

    #[test]
    fn plan_mode_allows_inspection_and_search_but_not_mutation() {
        for allowed in ["read", "glob", "grep", "list", "repo_search", "tool_search"] {
            assert!(plan_allowed(allowed), "{allowed} should be plan-allowed");
        }
        for denied in ["edit", "write", "apply_patch", "task", "commit", "research"] {
            assert!(!plan_allowed(denied), "{denied} must stay plan-denied");
        }
    }

    #[test]
    fn specialist_roles_receive_role_appropriate_immediate_palette() {
        assert!(immediate_for_agent("research", "research"));
        assert!(immediate_for_agent("evidence_bundle", "research"));
        assert!(immediate_for_agent("codesearch", "research"));
        assert!(!immediate_for_agent("research", "build"));
        assert!(!immediate_for_agent("security", "build"));
        assert!(immediate_for_agent("security", "security-review"));
        assert!(immediate_for_agent("security_search", "security-review"));
        assert!(!immediate_for_agent("research", "security-review"));
        assert!(immediate_for_agent("evidence_bundle", "verifier"));
        assert!(!immediate_for_agent("evidence_bundle", "build"));
    }
}
