# Cross-Agent Skill Compatibility M004 — Closure Status

Status: closed

Source plan: `plans/implementation/cross-agent-skills-compatibility/004-scoped-discovery-and-inspection.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: M003 closure at `plans/closure/cross-agent-skills-compatibility/003-status.md`.

Implementation commits: `d0164e95`, `8ef173bf`, `b1e03ce0`, `c1cd9893`, `08b7daa7`, `1b66ea5`, `be873fc`.

## 1. Finding

M004 is closed. Explicit workspace discovery walks a bounded nearest-first ancestor chain and stops at the closest `.git` boundary. Nearest eligible project roots win; sibling roots are never scanned. The registered `/skills [query]` command reads the active cached snapshot and displays effective, shadowed, invalid, resource, and alias-count details without scanning during rendering.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Bounded scope and nearest-root precedence | `nearest_scoped_project_wins_and_sibling_roots_are_not_loaded`; snapshot-builder fingerprint fixture | pass |
| Scope/provenance changes republish | `skill_scope_precedence_changes_fingerprint_without_hashing_absolute_paths`; refresh identity includes skill source/rank and summaries | pass |
| Path-free source identity tracks aliases/diagnostics | `source_summary_changes_refresh_identity_without_hashing_paths` | pass |
| `/skills` is registered and dispatched | command registry test plus `skills_command_renders_the_cached_workspace_snapshot` | pass |
| Active snapshot and resources are displayed | direct TUI dispatch test verifies cached workspace report, precedence, nested resource path, and reload hint | pass |
| Existing runtime snapshots remain pinned | asset refresh and asset snapshot suites | pass |

## 3. Implementation evidence

`workspace_skill_roots` accepts an explicit workspace path, canonicalizes it, walks at most 16 ancestors, and stops at `.git`. Snapshot construction uses these roots. Snapshot fingerprints include effective source kind and scope precedence, shadowed source identities, path-free source counts/alias counts, and skill diagnostic severity/reasons. Refresh diffs identify skill provenance and source-summary changes without embedding absolute paths.

`/skills` dispatch checks that the cached snapshot belongs to the active workspace, filters by query, and opens a read-only information dialog. It does not call the registry builder or inspect the filesystem. `/reload skills` remains the refresh path.

## 4. Verification executed

- `rtk cargo test -p codegg --lib agent::asset_snapshot` — 6 passed.
- `rtk cargo test -p codegg --lib agent::asset_refresh` — 6 passed.
- `rtk cargo test -p codegg --lib agent::asset_snapshot_builder` — 4 passed.
- `rtk cargo test -p codegg --lib tui::command::tests::skills_command_resolves_to_the_snapshot_inspection_dispatch` — 1 passed.
- `rtk cargo test -p codegg --lib tui::app::skills_inspection_tests::skills_command_renders_the_cached_workspace_snapshot` — 1 passed.
- `rtk cargo test --test asset_snapshot` — 8 passed.
- `rtk scripts/verify.sh quick` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.

## 5. Effective/shadowed/invalid view

Effective entries show skill name, source kind, and description. A query adds digest, effective precedence, and relative resource inventory. Shadowed source kinds, canonical alias counts, bounded parse diagnostics, and a `/reload skills` hint are included. Physical paths and resource bodies are not displayed.

## 6. Security and authorization

The command consumes only the immutable snapshot already held by TUI state. It performs no direct file read, core request, protocol addition, permission check, or write. The workspace mismatch path fails closed with an operator warning. TUI project-authority guard passed.

## 7. Refresh, restart, and concurrency

Source kind, precedence, shadow set, source counts, alias counts, and diagnostics participate in path-free fingerprint identity; `diff_snapshots` reports provenance changes. Existing refresh coordinator retains coalescing and failed-candidate behavior. Runtime pin and refresh tests passed; no in-flight snapshot mutation was added.

## 8. Platform findings

Linux temporary-workspace tests passed, including `.git` boundary, nested scope, sibling isolation, source aliases, and TUI dispatch. No Windows/macOS or hosted CI execution is claimed.

## 9. Documentation

Updated command catalog/count, architecture overview and skills/command docs, user Agent Skills guide, and TUI maintenance guidance. The architecture fingerprint description now matches path-free source-aware behavior.

## 10. Deviations and remaining findings

The fingerprint intentionally hashes source summary facts and scope ranks, not physical paths. The active TUI surfaces alias counts and source kinds while withholding locations. No new protocol/DTO/ADR or storage migration was needed. No unresolved M004 finding remains.

## 11. Dependency and registry disposition

M004 closed after M003 and unblocked M005. M005 has since closed. Registry and roadmap record the completed line as closed.

## 12. Final disposition

M004 acceptance criteria are met. Closure accepted.
