# LSP Phase 13-17 Roadmap

Status date: 2026-06-26
Scope: post-Phase-12 LSP productization and future integration work.

## Purpose

Phases 6-12 closed the core LSP subsystem: status and docs, workflow recipes, preview artifact lifecycle, lifecycle/root/server-health commands, bounded semantic operations, model-tier-aware context policy, and optional bounded semantic cache.

The next LSP roadmap should not add raw protocol surface by default. It should productize the subsystem around real-world validation, workflow UX, renderer/policy diagnostics, optional disk-cache evaluation, and manual lifecycle controls only where real usage justifies them.

## Current baseline

The current repo state includes:

- Canonical `egglsp::LspContextPacket` as the packet boundary.
- Read-only semantic operations through `LspTool`.
- Preview-only semantic edits with explicit apply path and SHA-256 validation.
- TUI lifecycle commands: `/lsp-status`, `/lsp-servers`, `/lsp-capabilities`, `/lsp-errors`, `/lsp-root`, `/lsp-restart`, `/lsp-stop`.
- Preview commands: `/lsp-previews`, `/lsp-preview`, `/lsp-preview-refresh`, `/lsp-preview-clear`, `/lsp-preview-apply`.
- Bounded workflow recipes for repair, review, security, impact analysis, test failure repair, interface boundary review, cross-file repair, and call-neighborhood evidence.
- Central `LspContextPolicy` for model-tier/workflow/risk/stale/unavailable decisions.
- Optional memory-only semantic cache with request-scoped file hashes.

## Architectural invariants

Preserve these invariants across all future phases:

- `LspTool` remains read-only for LSP-originated semantic operations.
- Mutation-producing LSP requests remain preview-only until explicitly applied by a higher-level user-approved path.
- Do not execute LSP `workspace/executeCommand` or server command-only code actions from the model-facing path.
- Any future mutation path must revalidate file hashes immediately before write and must report partial failures without marking previews applied.
- New context expansion must flow through `LspContextPacket`, named workflow recipes, or narrow bridge functions with explicit budgets and provenance.
- Stale evidence must be visible; never render stale or generation-mismatched evidence as clean current fact.
- Cache behavior must remain opt-in, bounded, root-scoped, and conservative unless a future phase explicitly proves a safer broader mode.

## Phase overview

### Phase 13: real-world LSP validation and `/lsp-doctor`

Goal: validate the subsystem against real server profiles and provide a user-facing diagnostic command for server/root/capability issues.

Deliverables:

- Pinned smoke-test matrix for rust-analyzer, pyright/pylsp, TypeScript, Go, and one fallback/no-server profile.
- Fixture workspaces that exercise root detection, diagnostics, definitions, references, formatting previews, rename previews, and lifecycle states.
- `/lsp-doctor [path]` command that explains server availability, root selection, executable discovery, capabilities, health state, recent errors, and recommended remediation.
- Observability metrics for startup time, initialization time, request latency, restart count, degraded count, and cache hit/miss counts if enabled.

Exit condition: a contributor can run a bounded validation suite and a user can run `/lsp-doctor` to diagnose common LSP failures without reading logs.

### Phase 14: workflow composition UX

Goal: make existing recipes easy to invoke from TUI and agent workflows without exposing raw semantic plumbing.

Deliverables:

- TUI commands or palette actions for common recipe flows: repair local, repair hunk, review diff, security review enriched, impact analysis, test failure repair, interface boundary, cross-file repair, call-neighborhood.
- Consistent recipe result panels or toasts that show evidence summary, truncation, stale notes, preview IDs, and recommended next action.
- Agent-facing recipe intent mapping so smaller models can ask for high-level workflows rather than manually composing tool arguments.
- Command dispatch tests and renderer tests for each surfaced workflow.

Exit condition: users and agents can invoke workflows by intent, not by raw LSP operation sequence.

### Phase 15: renderer-policy unification and context diagnostics

Goal: align `LspContextPolicy`, `RecipeSettings`, and `LspContextRenderConfig`, then make context inclusion/omission decisions inspectable.

Deliverables:

- Decide whether `LspContextRenderConfig` should expose `include_cross_file` and `include_hierarchy`, or document `RecipeSettings` as the only feature-flag path.
- Add context diagnostics that explain model tier, workflow, risk, stale policy, unavailable policy, included item counts, omitted item counts, truncation, cache hit, and policy source.
- Add debug/TUI surfaces for context diagnostics without bloating normal agent prompts.
- Fix known Phase 10 notes-text bug around impact-analysis reference cap messaging.

Exit condition: policy decisions are testable and visible, and rendered context no longer contains misleading truncation/cap notes.

### Phase 16: optional disk-cache evaluation

Goal: evaluate whether disk-backed semantic cache is worth the complexity. Implement only if measured latency or startup wins justify privacy, invalidation, and schema risks.

Deliverables:

- Benchmark memory-cache vs no-cache and simulated disk-cache cases.
- Privacy/security design for source-derived cached evidence.
- Explicit opt-in disk cache config if implemented.
- Root-scoped cache storage, schema versioning, TTL, size caps, manual clear, and drop-on-version-mismatch behavior.
- Documentation warning that disk mode stores source-derived semantic evidence.

Exit condition: either disk cache is explicitly rejected/deferred based on evidence, or a safe opt-in implementation lands with tests and docs.

### Phase 17: manual lifecycle controls for start/replay — DEFERRED

Status: **Deferred** (2026-06-27). No evidence of lifecycle control failures. Decision note: `plans/lsp_phase_17_decision_note.md`.

Auto-start via `get_or_create_client()` handles server startup on demand. Document replay is handled internally by the restart coordinator. Per-key stop uses `shutdown_all()` fallback. `/lsp-start` and `/lsp-replay-docs` were evaluated and deferred.

May be reconsidered if: auto-start fails in common scenarios, document replay is unreliable, or multi-root sessions require finer-grained stop control.

## Recommended order

Proceed in this order:

1. Phase 13: validate reality before adding user-facing workflows.
2. Phase 14: expose workflows once real server behavior is known.
3. Phase 15: unify renderer/policy diagnostics after workflows are visible.
4. Phase 16: evaluate disk cache only after metrics show whether it is useful.
5. Phase 17: add manual start/replay controls only if Phase 13 evidence shows user need.

Do not move Phase 16 earlier. Disk cache before validation and policy diagnostics would increase stale-evidence and privacy risk.

Do not move Phase 17 earlier unless users hit real lifecycle-control failures. Manual start/replay can easily become a parallel lifecycle model if service APIs are not clean.

## Risk register

Real server behavior may diverge across platforms and versions. Keep smoke tests pinned and mark profiles as pinned/CI-verified or best-effort.

Workflow commands can become thin wrappers around raw operations if not designed around user intent. Use named recipes and recipe outcomes as the public surface.

Context diagnostics can bloat prompts if injected by default. Prefer debug panels, TUI details, or opt-in diagnostic sections.

Disk cache can leak source-derived evidence or retain stale packets. Keep disabled by default and require explicit opt-in.

Manual lifecycle controls can introduce race conditions with auto-start/restart. Add them only behind clean service APIs.

## Completion definition for the whole roadmap

The Phase 13-17 roadmap is complete when Codegg's LSP subsystem is validated against real server profiles, exposed through workflow-first UX, diagnosable from TUI/debug surfaces, and still preserves the read-only LSP boundary, bounded context, explicit stale evidence, and conservative cache semantics established in Phases 6-12.

## Status update — corrective verification pass (2026-06-27)

Verification plan: `plans/lsp_phase_13_17_corrective_verification_plan.md`. The roadmap is now considered **verified** with Phases 13-15 implemented and Phases 16-17 explicitly deferred.

| Phase | Status | Evidence |
|-------|--------|----------|
| 13 — Real-world validation + `/lsp-doctor` | Implemented | `crates/egglsp/src/doctor.rs` + `/lsp-doctor` dispatch + 8 new doctor tests + 6 new dispatch tests |
| 14 — Workflow composition UX | Implemented | 10 `/lsp-*` workflow commands + `LspWorkflowDisplay` + 11 new workflow_recipes tests + 15 tool-level tests |
| 15 — Renderer-policy unification + context diagnostics | Implemented | `LspContextDiagnostics` + `/lsp-context-diagnostics` + 12 new policy/renderer tests |
| 16 — Optional disk-cache evaluation | Deferred | `plans/lsp_phase_16_disk_cache_decision.md`; memory-only mode remains the only active mode |
| 17 — Manual lifecycle controls | Deferred | `plans/lsp_phase_17_decision_note.md`; `/lsp-start` and `/lsp-replay-docs` NOT registered |

Closure criteria met for all eight workstreams in the verification plan. 52 new tests added total (8 doctor + 11 workflow + 8 policy + 4 renderer + 6 dispatch + 15 tool). Two saturating-arithmetic bug fixes in `crates/egglsp/src/workflow_recipes.rs` (lines 923 and 1167). Static safety sweep confirmed 0 disallowed matches.
