# Dependency maintenance

Dependency changes are deliberate maintainer work, not an automatic update
cadence or a release schedule. When an update is useful or a release is being
prepared:

1. Inspect dependency versions and duplicates with `cargo tree -d`; use
   `cargo outdated` when installed. For a specific package, inspect its
   dependency and feature paths with `cargo tree -i <package>` and
   `cargo tree -e features -i <package>`.
2. Check advisories with `cargo audit` when available. Review direct dependency
   changes for MSRV, licensing, maintenance status, API compatibility, and
   feature changes.
3. Update one bounded dependency group at a time. Review both manifest and
   lockfile changes, then run focused consumer tests and
   `scripts/verify.sh quick`.
4. Run package and release checks as part of an actual release. Record material
   compatibility or ownership decisions in an ADR or the owning subsystem plan.

## Workspace policy

The root `Cargo.toml` owns shared package metadata and the version and default
feature policy for dependencies used by multiple workspace members. Members
refer to those dependencies with `workspace = true` and add only the features
they need. Keep single-consumer dependencies in their owning crate rather than
hoisting them for consistency. Inspect the resolved feature graph before
widening a shared dependency's defaults.

Some external Git dependencies are pinned to revisions. Moving a pin requires
review of the resulting API, feature graph, platform support, and security
posture; do not replace a reviewed revision with a floating branch as routine
cleanup.

## YAML compatibility

YAML remains a read-only compatibility format for frontmatter in Markdown-based
agents, commands, and skills. Parsing is centralized in the
`codegg-config` document codec at `crates/codegg-config/src/document.rs`.
Use the subsystem's canonical TOML or JSON/JSON5 format for new configuration
and generated assets. Existing YAML assets are not rewritten automatically.

## Source verification

Checked 2026-11-15 against `Cargo.toml`, workspace member manifests under
`crates/`, `Cargo.lock`, `.cargo/audit.toml`, and
`crates/codegg-config/src/document.rs`.

- Shared dependency versions and baseline features are owned in the root
  workspace manifest; member manifests opt into needed features. Single-use
  dependencies remain local.
- YAML parsing is routed through the `codegg-config` document codec and is
  labeled a compatibility format in `crates/codegg-config/src/document.rs`.
- Removed the point-in-time upstream version and branch-head table previously
  checked on 2026-10-23. Those values change independently of the local
  dependency policy and should be checked when reviewing a specific update.
