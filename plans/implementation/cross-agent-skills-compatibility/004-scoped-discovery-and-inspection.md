# Cross-Agent Skill Compatibility Milestone 004 — Scoped workspace discovery and skill inspection

Status: closed
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; re-audit before execution)
Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`, M004
Closure: `plans/closure/cross-agent-skills-compatibility/004-status.md`
Long-term references: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`
Applicable ADRs: existing Runtime Assets contracts; ADR required only if public authority boundary changes.
Primary class: capability

## 1. Objective

Scoped workspace discovery and skill inspection, with complete verification and documentation for the named contract.

## 2. Why this milestone is not yet ready

Hard dependency: accepted M003 closure. The package/source APIs must already provide stable provenance, dedup and compatibility diagnostics.

## 3. Current implementation evidence

`src/agent/instructions.rs::ProjectInstructionResolver` already traverses workspace-root ancestors up to a trusted Git root under depth bounds. Skill scanning in `src/skills/registry.rs` currently examines only project root locations and supplied global roots. `src/tui/command.rs` has `/reload skills` but no dedicated `/skills` effective/shadow/skip view. Existing runtime asset summaries and command catalog should remain the query and UI foundation.

## 4. Invariants that must not regress

- Asset discovery/inspection is bound to the explicitly selected project/workspace and stable source provenance.
- Existing project/global precedence, passive discovery, foreign read-only policy and plugin namespacing remain intact.
- Permission grants, tool scripts and vendor execution metadata are not authorized by skills.
- Refresh remains transactional and pins in-flight turns to their captured snapshot/digests; no reads from ambient CWD.

## 5. Scope

**In scope:** only the production interfaces and fixtures below.

**Out of scope:** writing into foreign harness dirs, installing remote skills, general-purpose code search, a second plugin/tool runtime, live-provider CI and new CI lanes.

## 6. Required production changes

**Core:** introduce bounded project ancestor skills discovery matching the selected workspace scope and existing instruction resolver trust boundaries; do not scan all children, sibling checkouts or above a trusted git/worktree root. Define nested workspace priority: nearest eligible project skill overrides parent eligible skill, Codegg project override ordering otherwise stays deterministic, global lowest. **Refresh:** wire source summaries and skip reasons through the existing asset reporting and pinned-generation path, avoiding mutating in-flight asset views. **TUI:** expose an actual registered read-only `/skills` command (list, inspect, source/precedence, ignored/shadowed/invalid, refresh hints); tests must show command invokes a real path, not just an enum entry. Use existing core/client read surfaces wherever possible; if new DTO is unavoidable, establish explicit authorization and an ADR for new contract. **Security:** no filesystem read from generic renderer or palette, and no absolute global path/secret body exposure in shared projection.

**Storage/migrations:** avoid persistent schema changes; any new serialized diagnostic/source fields must preserve existing consumers. **Protocol:** reuse existing project-asset DTO, refresh and query services. Add no unaudited file access via the TUI. **Runtime/concurrency:** no scan at render/keystroke time; maintain coalesced refresh and generation pinning. **Security:** bound all path, content, metadata and diagnostic output, redact potentially sensitive physical paths in public logs.

## 7. Ordered work packages

**A — Scoping rules:** fixture repository/worktree with root, nested selected package, sibling and parent dirs; test root stop, depth and permissions. **B — Snapshot builder:** include resolved scoped roots in canonical identity/fingerprint/refresh; diagnose conflicts, invalid and disabled sources with stable bounded output. **C — Operator:** add typed `BuiltinSlashAction` plus command registry/dispatcher/presentation, scoped read-only report, no imaginary `/skill:` invocation; preserve `/reload` behavior. **D — Cross-project and restart qualification:** switch tabs during request, refresh while turn active, restart, symlink/redaction test.

## 8. Failure, cancellation, restart, and contention semantics

Missing optional skill dirs are not errors. A malformed candidate cannot eliminate a valid lower-precedence candidate. Races, cancellation and restart must preserve the last valid generation or return an actionable stale-generation status, never partially publish. Concurrent projects must not share active scoped assets or report another project's paths.

## 9. Compatibility and migration

Support existing native and portable skills, `skills.paths`, renamed/aliased global roots and plugin-contributed skills; document all new source ordering and any intentionally excluded vendor-specific behavior. No auto-migration or copying foreign files.

## 10. Required tests

**Unit/fixtures:** ancestor root boundary, selected subdir precedence, sibling isolation, nested .git directory/worktree gitfile, no git repo bounded behavior, identical symlink aliases. **Integration:** two tabs/workspaces, stale source diagnostics, reloading before next turn, stable old generation for active turn. **TUI:** `/skills` discoverable in palette and help, callback/response non-inert, skipped/shadowed readable, command collision policy. **Security:** global home path redaction and authorization.

## 11. Required verification commands

~~~bash
cargo test --lib skills::
cargo test --lib tui::
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
~~~

Select exact focused integration test binaries after inspecting current Cargo targets; record actual exit statuses and broader qualification if protocol/frontend changes.

## 12. Documentation updates

- `architecture/skills.md`, `architecture/command.md`, `docs/agents-skills.md`, `architecture/asset_refresh.md` if existing, and command/help documentation.

## 13. Acceptance criteria

- Selecting nested project resolves eligible own+ancestor skills without importing sibling workspaces.
- `/skills` reports meaningful effective/duplicate/skip provenance and references a real asset snapshot.
- Source changes are visible after refresh to subsequent turns, never to already running turns.

## 14. Stop conditions

Stop if scope traversal requires crossing the trusted repo boundary, TUI needs raw daemon filesystem access, or read-only reporting requires a public protocol change without an ADR.

## 15. Closure evidence required

Create `plans/closure/cross-agent-skills-compatibility/004-status.md` with exact commits, effective-vs-shadow matrix, fixture/platform/security results, concurrency and pinned-generation checks, documents changed, commands actually run, unresolved findings and updated registry/roadmap gate. Do not close on compile-only evidence.

## 16. Handoff notes

Rebase/check current branch and preserve unrelated work; never modify the historical Runtime Assets M001–M004 closure records. This is a deliberately additive maintenance workstream.
