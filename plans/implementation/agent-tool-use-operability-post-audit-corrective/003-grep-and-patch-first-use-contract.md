# Agent Tool-Use Operability Corrective C003 — Correct First-Use Grep and Patch Contracts

Status: ready for handoff
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (2026-10-19 main)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 programmatic authority; ADR-0004 approval routing; ADR-0028 execution authority (only where applicable). No new ADR required if existing ownership remains unchanged.
Primary class: capability

## 1. Objective
Correct First-Use Grep and Patch Contracts. Close only the specified real-model operating contract; do not conflate source review with successful runtime reproduction.

## 2. Why this milestone is ready
No unresolved hard dependency: existing tool, authorization and test seams are present. Interface dependencies are current repository contracts; re-check their SHAs at implementation.

## 3. Current implementation evidence
- `src/tool/util.rs::validate_path` resolves existing workspace-contained files independently of read history; `validate_target_path` allows nonexistent targets and intermediate parents. `src/tool/apply_patch.rs` create uses target validation; update uses existing validation. Agent saw `invalid path` for fresh probe; reason unproven. `src/tool/grep.rs` unambiguously duplicates prefixes: `push_rendered_line` emits path:line:text and execute wraps path again. `patch_util.rs` reports `invalid hunk header: @@` without line/expected grammar; a bare @@ is not a valid unified-diff hunk.

## 4. Invariants that must not regress
- Never relax canonical workspace-root containment, symlink/traversal security or mutation preflight. Error messages cannot disclose secrets/out-of-scope paths. A read is not a prerequisite to legitimate search/create. Patch update must be atomic on validation failure; no partial writes.

## 5. Scope
In: source paths and bounded tool-contract behavior listed below, focused regressions, model-facing diagnostics, and affected docs.
Out: unrelated runtime refactors, new global scheduler, network CI, public release and SBC compilation/startup benchmarks; do not weaken authorization or workspace containment.

## 6. Required production changes
Tool layer: trace an actual first-use `grep` and `apply_patch(mode=create)` via registry/broker, confirm root/cwd and mode; correct only demonstrated path resolution defects. Improve missing/existing target distinctions, path-vs-workspace-root diagnostics and patch mode documentation. Remove outer grep path prefix (maintain structured match schema). Report invalid unified hunk header with expected `@@ -<line>[,<count>] +<line>[,<count>] @@` grammar and location without accepting malformed diffs.
Storage: no new schema or migration expected; any proposed persistence change requires a new documented contract.
Protocol: preserve existing names/fields except the explicit compatibility corrections below; no remote protocol migration.
Security: no model-supplied source may grant privilege; fail closed on an absent required authority.
Documentation/static guards: update only owning source docs and focused tests/guards; do not duplicate existing cross-cutting scanners.

## 7. Ordered work packages
### A — Isolated fresh-workspace matrix
Run `write`→`grep` and patch-create without preceding `read`; then update/delete/move on existing paths. Cover relative vs absolute, nested missing parents, cwd mismatch, missing workspace root and symlinks.
### B — Renderer and parser UX
Deduplicate prefix at single renderer boundary; retain line numbers/context and bounded output. Return human-actionable invalid-hunk position/format, no parser permissiveness increase.
### C — Atomicity / containment tests
Prove malformed patch leaves file unchanged; traversal and out-of-root symlink reject, no parent side effects on denial.

## 8. Failure, cancellation, restart, and contention semantics
Failed validation must not mutate target; cancellations during writes respect existing atomicity/rollback contracts or explicitly report their current limits. Two concurrent modifications cannot bypass normal workspace locks/preflight.

## 9. Compatibility and migration
Preserve existing `apply_patch` `mode=create|update|delete|move` names; clarify `create` takes file contents whereas update takes unified diff. Do not silently reinterpret existing update calls as create.

## 10. Required tests
- File first-use from real broker path (no previous read), nested create and search.
- Grep exact `path:line:content` formatting plus context; same test on multiple files.
- Bare `@@`, malformed range, context mismatch, multihunk and absent patch; no mutation after error.
- Canonical root/symlink/traversal and permission denial matrix.

## 11. Required verification commands
```bash
cargo test -p codegg --lib tool::util
cargo test -p codegg --lib tool::grep
cargo test -p codegg --lib tool::apply_patch
cargo test -p codegg --lib tool::patch_util
scripts/verify.sh quick
```
These are required for implementation closure when applicable, not claims about work run while authoring the plan. Use `--locked` and no `--all-features` unless explicitly justified for a narrow target; no required live-provider or SBC run.

## 12. Documentation updates
`docs/tools.md` and `architecture/tool.md`; update file tool error shape if externally documented.

## 13. Acceptance criteria
- Fresh contained files can be searched and created without extra read roundtrip.
- Invalid files report meaningful, precise cause; paths outside allowed root remain denied.
- Every grep line has exactly one pathname prefix; malformed diff gives expected syntax/position.

## 14. Stop conditions
If reproductions require relaxing symlink rejection/workspace binding or moving permission decisions into tools; treat as security/authority finding requiring separate review.
In all cases stop rather than silently changing accepted ADRs, inventing authority, dropping scope requirements, rewriting historical closure or creating unbounded work.

## 15. Closure evidence required
Exact implementation commit and branch; before/after failure trace; requirement-to-test matrix; tests/guards actually run (and explicitly unrun); negative security cases; contention/restart treatment; known limitations with severity; `plans/closure/agent-tool-use-operability-post-audit-corrective/003-status.md` and registry/roadmap status update and unblock audit.

## 16. Handoff notes
Do not change `read` maximum here; that issue belongs to a separate previous agent report. Do not silently normalize errors by re-reading or granting broader root access.
