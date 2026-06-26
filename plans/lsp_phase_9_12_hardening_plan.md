# LSP Phase 9-12 Hardening Plan

Status date: 2026-06-26
Scope: corrective closure pass after the large Phase 9, 10, 11, and 12 implementation wave.

## Purpose

The repo now appears to contain implementation work for all four planned LSP phases:

- Phase 9: lifecycle/status/root/server-health commands, restart/stop controls, cache commands, and preview apply wording.
- Phase 10: broader bounded semantic operations through new `LspContextRequest` variants.
- Phase 11: centralized model-tier-aware context policy.
- Phase 12: optional bounded in-memory semantic cache.

This plan exists because the implementation landed as a large cross-cutting change. Before any new LSP feature work, harden the production seams, verify safety invariants, reconcile documentation with actual behavior, and add regression tests around the highest-risk paths.

## Current repo shape

Known implemented areas include:

- New TUI command registrations: `/lsp-servers`, `/lsp-capabilities`, `/lsp-errors`, `/lsp-root`, `/lsp-restart`, `/lsp-stop`, `/lsp-cache-status`, `/lsp-cache-clear`, and a changed `/lsp-preview-apply` command that now claims disk application with hash revalidation.
- `egglsp::tui_summary` server detail/root diagnosis renderers.
- `egglsp::context` broader request variants: `ImpactAnalysis`, `TestFailureRepair`, `InterfaceBoundary`, `CrossFileRepair`, and `CallNeighborhood`.
- `egglsp::evidence_collector` dispatch and collection helpers for the new request variants.
- `egglsp::context_policy` with centralized tier/workflow/risk/stale/unavailable policy.
- `egglsp::cache` with disabled-by-default or memory-mode bounded cache, TTL, file-hash checks, root/server/request/budget keying, and stats.
- `LspTool` semantic-cache ownership and cache status/clear helpers.

## Hardening priorities

The hardening pass should be ordered by safety risk:

1. Preview apply path safety.
2. Command dispatch and user-visible lifecycle/root controls.
3. Phase 10 semantic operation bounds and fallback behavior.
4. Phase 11 policy integration correctness and non-bloat guarantees.
5. Phase 12 cache integration truthfulness, invalidation, and documentation.
6. CI/test evidence and doc reconciliation.

## Non-goals

Do not add new LSP protocol features.

Do not add additional request variants.

Do not expand cache to disk mode.

Do not execute `workspace/executeCommand` or `workspace/applyEdit`.

Do not make `LspTool` itself mutate files. If preview application mutates files, it must go through the existing mutating apply path and approval semantics.

Do not start Phase 13 or broader semantic memory work.

## Workstream 1: preview apply path safety audit

### Problem

`/lsp-preview-apply` now advertises applying preview patches to disk with hash revalidation. This is the highest-risk Phase 9 change because it crosses from preview-only LSP output into actual file mutation.

### Required invariant

LSP remains preview-only. File mutation must happen through Codegg's existing mutating patch/apply path with explicit approval and final hash validation.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `src/tool/mod.rs`
- existing patch/apply implementation files
- `crates/egglsp/src/tui_summary.rs`
- `crates/egglsp/src/preview_registry.rs`
- tests that cover preview registry and TUI command dispatch

### Audit checklist

Verify `/lsp-preview-apply <id>` does all of the following:

1. Resolves the preview ID from the shared `PreviewArtifactRegistry`.
2. Refreshes stale-base status immediately before apply.
3. Revalidates original file hashes immediately before mutation.
4. Blocks stale previews by default.
5. Blocks previews with no patches.
6. Blocks missing preview IDs with a clear error.
7. Blocks already-applied previews unless there is an explicit existing reapply confirmation flow.
8. Converts `PreviewApplyCandidate` into the existing patch-apply input shape.
9. Uses the existing mutating apply path and approval model.
10. Does not call any LSP mutation method.
11. Marks a preview applied only after successful patch application.
12. Leaves failed apply attempts pending and reports the error.
13. Produces clear user-facing text for success, stale blocked, no patch, missing ID, and apply failure.

### Required tests

Add tests for:

- fresh preview with valid patch routes to apply path and marks applied only after success,
- stale preview is blocked before apply,
- no-patch preview is blocked,
- missing preview ID returns clear error,
- already-applied preview is not silently re-applied,
- failed apply does not mark preview applied,
- direct LSP mutation paths remain absent (`workspace/applyEdit`, `workspace/executeCommand`, command-only actions).

If the actual existing approval/apply path is not easily callable from TUI tests, add a small testable boundary function that performs validation and returns a typed `PreviewApplyPlan` without mutating. Then separately test the handoff into the mutating apply path.

### Acceptance criteria

- Applying a preview cannot bypass hash validation or approval.
- Stale and no-patch previews are safely blocked.
- Registry applied state only changes after successful mutation.
- Tests prove LSP does not directly write files.

## Workstream 2: command dispatch verification

### Problem

Command registration is visible, but registration alone does not prove dispatch handlers are implemented correctly. Each new command must have a tested path from slash command to `LspTool`/service behavior.

### Target commands

- `/lsp-servers`
- `/lsp-capabilities <server-key>`
- `/lsp-errors <server-key>`
- `/lsp-root <path>`
- `/lsp-restart <server-key>`
- `/lsp-stop <server-key|--all>`
- `/lsp-cache-status`
- `/lsp-cache-clear [--all|root]`
- `/lsp-preview-apply <id>`

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- `crates/egglsp/src/root.rs`
- TUI command tests or command parser tests

### Implementation steps

1. Build a command dispatch table checklist mapping command name to handler function.
2. Verify every command handles missing arguments with a clear usage message.
3. Verify invalid server keys do not panic.
4. Verify read-only commands do not start LSP servers or mutate state.
5. Verify restart/stop commands are scoped and report scheduled/stopped outcomes.
6. Verify cache commands handle disabled cache gracefully.
7. Add tests or lightweight handler-level unit tests for every command.

### Acceptance criteria

- Every new registered command has a real handler.
- Every handler has missing-arg and invalid-arg coverage.
- Read-only commands are side-effect-free.
- Mutating lifecycle commands are scoped and explicit.

## Workstream 3: lifecycle/root/status correctness

### Problem

Phase 9 added lifecycle and root ergonomics. These must be correct in degraded and edge-case states, not just happy path.

### Target files

- `crates/egglsp/src/tui_summary.rs`
- `crates/egglsp/src/root.rs`
- `crates/egglsp/src/health.rs`
- `src/tool/lsp.rs`
- docs

### Required checks

1. `LspServerStatusDetail` includes root, state, generation, pending requests, open docs, restart attempts, last error, stderr tail, usability, and capabilities.
2. Capability rendering does not claim `hover: yes` unless hover support is actually known or intentionally modeled as always available with a doc comment.
3. Root diagnosis does not start a server.
4. Root diagnosis handles:
   - file outside allowed root,
   - unknown language,
   - no root markers,
   - nested markers,
   - missing server profile,
   - relative and absolute paths.
5. Restart/stop visibility includes generation changes and state labels.

### Required tests

- server list rendering: ready, indexing, degraded, failed, restarting,
- capability rendering with initialized and uninitialized snapshots,
- error rendering with and without stderr tail,
- root diagnosis no-root and outside-allowed-root,
- root diagnosis nested-root preference,
- restart state/generation visibility if service APIs support it.

### Acceptance criteria

- Users can diagnose why LSP is absent/degraded without reading logs.
- Root diagnosis is deterministic and read-only.
- Status output does not overclaim capabilities.

## Workstream 4: bounded semantic operation hardening

### Problem

Phase 10 added several broad operations. The safety value of Phase 10 depends on caps, ranking, truncation, and fallback being correct.

### Target request variants

- `ImpactAnalysis`
- `TestFailureRepair`
- `InterfaceBoundary`
- `CrossFileRepair`
- `CallNeighborhood`

### Target files

- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- tests for collector and renderer

### Required hardening checks

For each request variant, verify:

1. Inputs are bounded before collection.
2. Collection does not scan the whole workspace by default.
3. Max refs/files/depth/callers/callees are enforced.
4. Budget truncation notes are emitted when data is dropped.
5. Unsupported capabilities produce operational notes, not panics.
6. Stale/degraded lifecycle freshness is preserved.
7. Ranking favors same-file and hunk-local evidence.
8. Render output identifies operation type and truncation.
9. Small-tier policy cannot include broad cross-file expansion.

### Suspicious item to check

In impact analysis, confirm truncation note logic compares original reference count against cap. Avoid notes such as `references capped` when the result was not actually capped.

### Required tests

- `ImpactAnalysis` caps references and files.
- `ImpactAnalysis` ranks same-file and changed-file references first.
- `TestFailureRepair` extracts only obvious identifiers and labels extraction as heuristic.
- `InterfaceBoundary` handles unsupported implementation capability.
- `CrossFileRepair` enforces related-file cap.
- `CallNeighborhood` enforces depth and caller/callee caps, including cycle guard if represented.
- Each operation renders a recognizable section and truncation note.
- Required mode fails for unusable server; opportunistic mode returns notes.

### Acceptance criteria

- New semantic operations are bounded by construction.
- Renderer output is useful but not bloated.
- No operation introduces raw arbitrary LSP execution.

## Workstream 5: context policy hardening

### Problem

Phase 11 introduced central policy. The danger is partial integration: policy exists, but old paths may still use ad-hoc defaults, or policy may not actually control budgets/features.

### Target files

- `crates/egglsp/src/context_policy.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `src/agent/turn_runtime.rs`
- `src/tool/lsp.rs`
- config schema if model-tier overrides exist

### Required checks

1. Identify every model-tier branch in LSP context code.
2. Route all tier/workflow/risk decisions through `LspContextPolicy` or document exceptions.
3. Verify `policy.to_render_config()` controls actual byte caps and section inclusion.
4. Verify `policy.to_recipe_settings()` or equivalent controls recipe breadth.
5. Verify stale policy is actually applied, not just stored.
6. Verify unavailable policy is actually applied, not just stored.
7. Verify token budget hint changes max bytes predictably.
8. Verify security-sensitive workflows use stricter stale/unavailable behavior where intended.
9. Verify policy summary is not noisy in final prompts or can be toggled/debug-scoped if needed.

### Required tests

- small/workhorse/frontier policy defaults,
- workflow-specific feature flags,
- token-budget low/normal/high behavior,
- stale policy include/omit/require-fresh behavior,
- unavailable policy note/omit/fail behavior,
- security-sensitive workflow policy,
- production agent path appends or logs the correct policy summary.

### Acceptance criteria

- There is one authoritative LSP context policy path.
- Tier and workflow behavior is predictable and tested.
- Policy fields are not dead configuration.

## Workstream 6: semantic cache hardening

### Problem

Phase 12 added a substantial cache implementation. The cache is currently safe-looking because it is memory-only and disabled by default, but production integration and documentation must be exact.

### Target files

- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `src/tool/lsp.rs`
- config schema
- docs

### Required checks

1. Confirm default mode is disabled unless config explicitly enables memory mode.
2. Confirm `LspTool` respects config and does not enable memory cache accidentally.
3. Confirm cache keys include root, server, operation, request fingerprint, input hashes, capability fingerprint, and budget fingerprint.
4. Confirm cache entry estimated byte size is bounded and eviction works by count and byte budget.
5. Confirm TTL expiry increments miss/stale counters correctly.
6. Confirm file hash mismatch removes entry.
7. Confirm server generation mismatch removes or downgrades entry. Current conservative remove behavior is acceptable, but docs must say that rather than claiming retained-after-restart cache hits.
8. Confirm cache is either integrated into collection through an explicit cached collection path or documented as implemented but not production-enabled yet.
9. Confirm cache status/clear commands work when cache is disabled.
10. Confirm root-specific clear cannot clear unrelated roots.
11. Confirm no disk cache is implied in docs/config.

### Required tests

- disabled mode always misses and stores nothing,
- memory mode hit/miss,
- TTL expiry,
- file hash mismatch,
- server generation mismatch,
- max entries eviction,
- max bytes eviction,
- clear all,
- clear by root,
- stats correctness,
- config mapping from `codegg-config` to `LspCacheConfig`, if present.

### Acceptance criteria

- Cache behavior is conservative and tested.
- Production integration status is explicit.
- Docs match actual invalidation behavior.

## Workstream 7: documentation reconciliation

### Problem

Docs were updated heavily. They may now overstate completion, cache behavior, preview apply semantics, or CI status.

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md`
- `AGENTS.md`
- `CHANGELOG.md`
- `plans/lsp_phase_6_12_roadmap.md`

### Required doc checks

1. Phase 9 docs accurately state whether preview apply is fully integrated or still partially export/approval mediated.
2. Phase 10 docs list only implemented bounded operations.
3. Phase 11 docs distinguish policy implemented from policy fully applied everywhere.
4. Phase 12 docs state memory-only and disabled-by-default unless config enables it.
5. Cache docs state generation mismatch currently invalidates/removes entries if that is the implementation.
6. Command docs match actual command names and argument syntax.
7. CI/test claims cite actual local command results or avoid overclaiming.
8. Roadmap status labels distinguish `implemented`, `hardened`, and `closed`.

### Acceptance criteria

- Docs do not claim unimplemented cache or apply behavior.
- Command documentation matches handler behavior.
- Roadmap reflects hardening status rather than broad completion claims.

## Workstream 8: CI and test evidence

### Problem

GitHub status metadata may not show workflow results. Commit messages mention passing tests, but closure should leave clear reproducible evidence.

### Required focused commands

Run at minimum:

```bash
cargo fmt --check
cargo test -p egglsp cache
cargo test -p egglsp context_policy
cargo test -p egglsp context_renderer
cargo test -p egglsp evidence_collector
cargo test -p egglsp workflow_recipes
cargo test -p egglsp tui_summary
cargo test --test phase5_context_integration lsp
```

Recommended broader checks:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If the root workspace has known unrelated failures, document exact failures and prove focused LSP tests pass.

### Acceptance criteria

- Focused LSP tests pass.
- Formatting passes.
- Clippy is either clean or known unrelated failures are documented precisely.
- Any known root-crate failures are not hidden behind vague language.

## Workstream 9: static safety sweep

### Problem

The LSP safety boundary must remain intact despite preview apply integration and broad semantic operations.

### Required static sweep

Search `src/` and `crates/egglsp/src/` for:

- `workspace/applyEdit`
- `workspace/executeCommand`
- `executeCommand`
- `applyEdit`
- `apply_workspace_edit`
- `apply_preview`
- direct `std::fs::write` or equivalent inside LSP tool paths
- command-only code action execution

### Expected result

Allowed matches:

- comments/docs explaining rejection,
- capability advertisement/metadata,
- tests proving rejection,
- mutating non-LSP apply path outside `LspTool` with approval semantics.

Disallowed matches:

- LSP server command execution,
- direct application of LSP workspace edits,
- file writes inside `egglsp` preview operations,
- file writes inside `LspTool` except through an explicit existing mutating tool boundary.

### Acceptance criteria

- Static sweep results are documented in commit message or docs.
- Any suspicious match is either removed or justified in comments/tests.

## Suggested implementation order

1. Preview apply validation/handoff tests.
2. Command dispatch coverage for all new Phase 9/12 commands.
3. Lifecycle/root renderer tests and capability overclaim cleanup.
4. Phase 10 request cap/fallback tests.
5. Phase 11 policy dead-field audit and tests.
6. Phase 12 cache tests and docs reconciliation.
7. Static safety sweep.
8. Focused test matrix and changelog/roadmap status update.

## Final closure criteria

Phases 9-12 may be marked closed only when:

- preview apply path is proven safe and approval-mediated,
- every registered command has a tested dispatch handler,
- lifecycle/root/status commands are read-only where intended and scoped where mutating,
- all Phase 10 operations are bounded and tested under cap/fallback/stale cases,
- Phase 11 policy is actually applied to rendering/recipe behavior, not just defined,
- Phase 12 cache is conservative, optional, tested, and documented accurately,
- docs match implementation rather than aspiration,
- focused LSP tests and formatting pass,
- static safety sweep confirms no LSP mutation boundary regression.
