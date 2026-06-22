# LSP Phase 5 Final Closeout and Stabilization

## Purpose

Close the last Phase 5 gaps after:

```text
142b31882aa1d70a346ccaa452a634d640c0b2e9
```

Phase 5 is now functionally implemented: context packets, production evidence adapter, task-aware turn input, security/hunk bridges, model-tier rendering, preview registry, TUI summaries, and production-seam tests exist. The remaining work is stabilization and closure discipline:

1. remove or quarantine the known timing flake;
2. verify live preview IDs flow through actual `renamePreview`, `formatPreview`, and `sourceActionPreview` outputs;
3. tighten or contain the provenance side-channel in `ServiceLspEvidenceProvider`;
4. prevent context-model drift now that `LspContextPacket` is canonical and `SemanticContextPacket` is tool-facing;
5. add a small number of true fake-server or tool-dispatch tests beyond tuple mocks;
6. make final docs/status match actual validated behavior;
7. run and record full regression evidence.

This plan is intentionally narrow. Do not add Phase 6 features here.

## Final Phase 5 Closure Definition

Phase 5 is complete when:

1. All deterministic tests for Phase 5 pass without relying on known flakes.
2. The `agent_loop_harness` timing flake is fixed, isolated, or explicitly excluded from the Phase 5 closure gate with an issue/plan reference.
3. Live LSP preview tool operations expose preview IDs and register artifacts in the real session/turn artifact path.
4. Preview artifacts preserve `not_applied = true`, affected files, original hashes, stale-base state, capability provenance, and server generation when available.
5. `ServiceLspEvidenceProvider` provenance cannot be mismatched with the wrong request under concurrent collection.
6. `LspContextPacket` is the documented canonical packet for agent/review context; `SemanticContextPacket` remains only a tool DTO/adapter shape.
7. Production-seam tests exercise actual tool dispatch or fake-server-backed paths for at least one context collection and one preview registration path.
8. Model-tier rendering is verified in the production turn path, not only in renderer unit tests.
9. Hunk/security bridges preserve existing behavior while carrying Phase 5 provenance/truncation metadata.
10. Final docs describe the exact completed behavior and remaining limitations.

## Primary Files

```text
crates/egglsp/src/evidence_adapter.rs
crates/egglsp/src/evidence_collector.rs
crates/egglsp/src/context.rs
crates/egglsp/src/context_renderer.rs
crates/egglsp/src/preview_registry.rs
crates/egglsp/src/security_context.rs
crates/egglsp/src/tui_summary.rs
src/tool/lsp.rs
src/agent/turn_runtime.rs
src/security/lsp_executor.rs
src/lsp/hunk_nav_collector.rs
src/tui/components/status_bar.rs
tests/phase5_context_integration.rs
tests/lsp_composite_stdio.rs
```

Documentation:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase5_completion_hardening_and_workflow_depth.md
```

## Non-Goals

Do not:

- add new LSP operations;
- add new language-server profiles;
- execute `workspace/executeCommand`;
- apply preview artifacts automatically;
- replace all legacy semantic-context tool output;
- create a persistent semantic index;
- redesign the TUI;
- broaden to Phase 6 planning in this closeout.

# Pass 1 — Stabilize or Isolate the `agent_loop_harness` Timing Flake

## Current Problem

The latest commit reports:

```text
1 pre-existing flake (agent_loop_harness timing test)
```

A final phase cannot be called closed if the default test suite has a known red/flaky timing test with no explicit containment.

## Required Investigation

Find the flaky test and record:

```text
test name
file path
failure mode
timeout/delay assumptions
runtime/task scheduling assumptions
whether it fails on local, CI, or both
whether it is related to Phase 5 changes
```

Likely locations:

```text
tests/*agent*/*
src/agent/*
agent_loop_harness
```

## Valid Resolutions

### Preferred: Fix the test deterministically

Replace sleeps/timing assumptions with explicit synchronization:

```text
watch channel
oneshot ready signal
barrier
event-log wait helper
bounded polling with explicit condition
```

### Acceptable: Mark as ignored with issue/plan reference

Only acceptable if the flake is proven pre-existing and unrelated to LSP Phase 5.

Requirements:

```rust
#[ignore = "pre-existing timing flake; tracked in <issue/plan>"]
```

or a feature-gated quarantine suite.

### Not acceptable

Do not leave it intermittently failing while claiming full suite closure.

## Required Tests

Run the fixed or isolated test repeatedly:

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test <exact-test-filter> || exit 1
done
```

## Acceptance Criteria

- Phase 5 closure has no untracked flaky default test.

# Pass 2 — Verify Live Preview ID Propagation Through Actual Tool Outputs

## Current Problem

`PreviewArtifactRegistry` invariants are tested, but the full live tool path must prove preview IDs are returned from actual preview operations, not only registry unit tests.

## Required Tool Paths

Audit and update:

```text
src/tool/lsp.rs renamePreview
src/tool/lsp.rs formatPreview
src/tool/lsp.rs sourceActionPreview / codeActionPreview
```

Each preview-producing path should:

1. obtain preview result from checked typed LSP operation;
2. register the preview artifact in the per-session/per-turn registry or artifact store;
3. return `preview_id` in tool output;
4. return `not_applied = true` in tool output;
5. preserve affected files and original hashes;
6. include stale-base state;
7. include server/capability provenance where available.

## Required Output Shape

Add or extend the DTO:

```rust
pub struct LspPreviewToolOutput<T> {
    pub operation: String,
    pub file_path: Option<String>,
    pub preview_id: String,
    pub not_applied: bool,
    pub edit_count: usize,
    pub affected_files: Vec<String>,
    pub stale_base: bool,
    pub result: T,
}
```

If existing tool output schema cannot change without broader impact, add these fields behind `preview_metadata`.

## Registry Ownership

The registry should be scoped to session or turn. Do not use a process-global mutable registry unless the rest of artifact storage already does so safely.

Preferred ownership:

```text
artifact_store / session tool registry / turn runtime state
```

## Tests

Add tool-dispatch or fake-server-backed tests:

```text
lsp_tool_rename_preview_returns_preview_id
lsp_tool_format_preview_returns_preview_id
lsp_tool_code_action_preview_returns_preview_id
lsp_tool_preview_output_not_applied_true
lsp_tool_preview_registry_entry_contains_original_hashes
lsp_tool_preview_registry_marks_stale_base_when_file_changes
lsp_tool_preview_does_not_mutate_disk
```

Use the existing fake LSP server where possible.

## Acceptance Criteria

- Users and agents can cite preview IDs from actual tool outputs.
- Preview registry is not merely a library-level construct.

# Pass 3 — Contain the Evidence Adapter Provenance Side-Channel

## Current Problem

`ServiceLspEvidenceProvider` keeps tuple-shaped trait compatibility by recording provenance in `last_provenance` and exposing `take_provenance()` / `last_provenance()`. This is workable but fragile under concurrent calls or future collector parallelization.

## Required Decision

Choose one of two policies.

### Preferred: Typed DTO Trait V2

Introduce a richer trait beside the existing tuple trait:

```rust
#[async_trait]
pub trait LspTypedEvidenceProvider {
    async fn diagnostics_for_file_typed(&self, file: &Path) -> Result<Vec<EvidenceDiagnostic>, LspError>;
    async fn references_typed(&self, file: &Path, line: u32, column: u32) -> Result<Vec<EvidenceLocation>, LspError>;
    // etc.
}
```

Then adapt collector internals to prefer typed provider when available.

### Acceptable: Sequential Side-Channel Contract

If V2 is too much churn, explicitly enforce the side-channel contract:

- provider calls are sequential within `collect_context`;
- no `join!` / concurrent provider calls may use the same adapter instance;
- immediately consume `take_provenance()` after each call;
- add debug assertions that provenance operation matches the expected operation;
- document this at the trait and adapter.

## Required Guard Tests

If side-channel retained:

```text
collector_consumes_provenance_immediately_after_call
provenance_operation_mismatch_is_detected
collector_does_not_parallelize_provider_calls
adapter_take_provenance_clears_slot
adapter_records_provenance_on_error
```

If typed DTO V2 added:

```text
typed_provider_preserves_provenance_without_side_channel
typed_collector_prefers_typed_provider
tuple_provider_path_still_supported_for_tests
```

## Acceptance Criteria

- Provenance cannot accidentally attach to the wrong context item.

# Pass 4 — Add Guardrails Against Context Model Drift

## Current Problem

The repo now has:

```text
LspContextPacket = canonical Phase 5 agent/review packet
SemanticContextPacket = semanticContext tool DTO
SecurityEvidenceSummary = security bridge DTO
TuiSummary = UI DTO
```

This is acceptable, but future work could add more parallel packet shapes.

## Required Guardrails

1. Add docs to `context.rs` and `src/tool/lsp.rs` stating:

```text
LspContextPacket is canonical for agent/review workflows.
SemanticContextPacket is a tool-facing DTO for semanticContext compatibility.
```

2. Add adapter functions in one place:

```rust
semantic_context_to_lsp_items(...)
lsp_packet_to_security_summary(...)
lsp_packet_to_tui_summary(...)
```

3. Avoid ad hoc conversion scattered across `src/tool/lsp.rs`, `security_context.rs`, and hunk modules.

4. Add tests:

```text
semantic_context_bridge_preserves_diagnostics
semantic_context_bridge_preserves_truncation_notes
lsp_packet_security_bridge_preserves_counts
lsp_packet_tui_bridge_preserves_preview_ids
```

## Acceptance Criteria

- There is one canonical model and named bridges, not implicit duplication.

# Pass 5 — Verify Model-Tier Rendering in the Actual Turn Path

## Current Problem

Tests prove tier rendering at the renderer and helper level. The production turn path must prove it passes the resolved tier into the renderer when workflow metadata exists.

## Required Tests

Add a turn-runtime-level or builder-level test:

```text
turn_runtime_small_model_uses_small_lsp_render_tier
turn_runtime_frontier_model_uses_frontier_lsp_render_tier
turn_runtime_unknown_family_defaults_workhorse
turn_runtime_truncation_notes_visible_for_small_model
```

This can use a fake `TurnRuntime` dependency seam if directly running the agent loop is heavy.

## Required Assertions

For small model:

```text
cross-file broad references absent
hunk-local diagnostics/definitions present
truncation notes present
```

For frontier:

```text
references/implementations/workspace symbols present when budget permits
```

## Acceptance Criteria

- Model-tier rendering is proven in the production prompt assembly path.

# Pass 6 — Strengthen Hunk and Security Bridge Production Tests

## Current Problem

Production-seam tests exist, but many still use the mock provider. The bridges should be proven against at least one fake-server or production adapter path where practical.

## Required Tests

Add or upgrade tests:

```text
hunk_bridge_with_service_adapter_preserves_hunk_source_tag
hunk_bridge_with_service_adapter_records_truncation
hunk_bridge_with_service_adapter_degrades_without_lsp
security_bridge_with_lsp_packet_preserves_public_api_fanout
security_bridge_with_lsp_packet_omits_preview_mutations
security_bridge_with_service_adapter_marks_stale_evidence
```

Use existing fake server support if possible. If that is too expensive, use `ServiceLspEvidenceProvider` with a test `LspService` seam or a thin fake adapter that exercises the same bridge code.

## Acceptance Criteria

- Hunk/security bridges are not only mock-packet transformations.

# Pass 7 — Final No-Mutation and Command-Execution Sweep

## Goal

Reassert the central safety property at the final Phase 5 boundary.

## Required Static Audit

Search for:

```text
executeCommand
workspace/executeCommand
applyEdit
workspace/applyEdit
apply_preview
apply_workspace_edit
```

Ensure Phase 5 paths do not call mutation execution.

## Required Runtime Tests

```text
phase5_agent_context_collection_does_not_apply_rename
phase5_agent_context_collection_does_not_apply_formatting
phase5_agent_context_collection_does_not_execute_code_action_command
phase5_preview_registration_does_not_apply_workspace_edit
```

If these are covered indirectly, add a test that explicitly hashes affected files before and after Phase 5 context collection and preview registration.

## Acceptance Criteria

- Phase 5 remains evidence/preview-only.

# Pass 8 — Final Regression and CI Evidence

## Required Commands

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --test phase5_context_integration
cargo test --workspace --all-features
```

If `agent_loop_harness` remains isolated, document the exact command that excludes it and the issue/plan tracking it.

## Required Evidence

Final handoff must include:

```text
commit SHA
commands run
pass/fail output summary
known ignored tests
reason for any ignored test
whether CI status is available
```

## Acceptance Criteria

- The repo has a reproducible Phase 5 closure test story.

# Pass 9 — Documentation and Status Closeout

## Documentation Updates

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase5_agent_context_and_workflow_integration.md
plans/lsp_phase5_completion_hardening_and_workflow_depth.md
```

## Required Content

Document:

- canonical packet policy;
- live production adapter behavior;
- side-channel or typed DTO provenance policy;
- task-aware agent context path;
- preview registry live tool output path;
- hunk/security bridge behavior;
- model-tier rendering in turn runtime;
- TUI/detail summary capabilities;
- no-mutation/executeCommand safety sweep;
- test commands and remaining ignored/flaky tests.

## Final Status Wording

Only after all closure gates pass:

```text
Phase 5 complete: Codegg assembles bounded, provenance-rich LSP context for agent, hunk, review, and security workflows. Live read-only LSP evidence is collected through the production adapter, preview-producing operations register non-applied artifacts with stable preview IDs, model-tier rendering is active in the turn runtime, stale/degraded/unsupported states are explicit, and deterministic tests cover budgets, fallback modes, preview safety, production adapter seams, hunk/security bridges, and UI summaries.
```

If any closeout item remains:

```text
Phase 5 implementation complete; final stabilization remains: <specific item>.
```

# Exact Execution Order

1. Stabilize or isolate the timing flake.
2. Verify live preview ID output through actual tools.
3. Decide and enforce provenance side-channel or typed DTO policy.
4. Add canonical model drift guardrails.
5. Prove model-tier rendering in the turn path.
6. Strengthen hunk/security bridge production tests.
7. Run no-mutation/executeCommand sweep.
8. Run full regression commands.
9. Update docs and status.

# Recommended Commit Sequence

```text
1. test(agent): stabilize or isolate agent loop timing flake
2. feat(lsp): return preview IDs from live LSP preview tools
3. refactor(lsp): harden evidence adapter provenance contract
4. docs(lsp): guard canonical Phase 5 context packet boundary
5. test(agent): verify model-tier LSP rendering in turn runtime
6. test(lsp): harden hunk and security bridge production seams
7. test(lsp): assert Phase 5 no-mutation safety end to end
8. docs(lsp): close Phase 5 with regression evidence
```

# Mandatory Final Checklist

- [ ] Timing flake fixed or quarantined with tracking reference.
- [ ] Live `renamePreview` returns a preview ID.
- [ ] Live `formatPreview` returns a preview ID.
- [ ] Live `sourceActionPreview` / `codeActionPreview` returns a preview ID.
- [ ] Preview registry entries include original hashes and `not_applied = true`.
- [ ] Provenance side-channel is guarded or replaced by typed DTOs.
- [ ] Canonical packet boundary is documented in code and architecture docs.
- [ ] Production turn path test verifies small/workhorse/frontier model-tier rendering.
- [ ] Hunk bridge production-seam tests pass.
- [ ] Security bridge production-seam tests pass.
- [ ] No Phase 5 path calls `workspace/executeCommand` or applies edits.
- [ ] Full regression commands pass or tracked quarantine is documented.
- [ ] Docs use final Phase 5 status only after gates pass.

# Final Handoff Output

The implementing model must report:

```text
commits created
timing flake resolution
preview ID output examples for rename/format/code-action
preview registry storage location
provenance policy chosen
canonical packet bridge functions/tests
turn-runtime model-tier tests
hunk/security bridge tests
no-mutation audit results
commands run and results
remaining ignored tests or limitations
documentation updates
```

After this plan passes, Phase 5 can be called fully closed rather than merely implemented.
