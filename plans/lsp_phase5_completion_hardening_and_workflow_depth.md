# LSP Phase 5 Completion: Workflow Depth, Canonical Context, and Live Preview Wiring

## Purpose

Complete the remaining Phase 5 work after:

```text
39c0200bdd2ae198f6d8027b47089c77c01ef15c
```

Phase 5 has a strong first implementation pass:

- `egglsp::context` defines context packets, requests, budgets, freshness, provenance, scoring, truncation, and context item kinds.
- `egglsp::evidence_collector` provides a testable provider trait and mock-driven collector.
- `egglsp::context_renderer` renders agent-facing summaries.
- `egglsp::security_context` adds deterministic risk tagging and security evidence summaries.
- `egglsp::preview_registry` defines a preview-only registry.
- `egglsp::tui_summary` defines compact LSP status/summary rendering.
- `egglsp::degradation_policy` defines disabled/opportunistic/required behavior.
- Codegg now injects LSP status/context into the agent system prompt when an `LspService` is available.
- TUI status-bar wiring exists.
- `tests/phase5_context_integration.rs` provides broad mock-driven coverage.

Remaining work is about making Phase 5 deep enough to call complete:

1. Pass real hunk/diff/review/task context into the collector rather than injecting only a generic LSP status section.
2. Reconcile or clearly split the older `SemanticContextPacket` tool output and the new `LspContextPacket` model.
3. Wire live rename/format/code-action preview operations into the preview registry/session artifact path.
4. Replace tuple-heavy collector outputs with richer DTOs or adapters preserving real provenance, version/hash, and capability decisions.
5. Make model-tier rendering active in the real agent path, not only in library/test helpers.
6. Expand TUI/tool summaries beyond one-line status where useful.
7. Add integration tests over the production adapter path, not only the mock provider path.
8. Preserve Phase 2–4 behavior and no-mutation guarantees.

This plan is tailored for a smaller implementation model. Execute the passes in order. Do not add new protocol breadth unless required to adapt existing operations into the Phase 5 context path.

## Completion Definition

Phase 5 is complete when:

1. Agent turns receive task-aware, hunk-aware, or review-aware LSP context packets when relevant context exists.
2. The old semantic-context packet and new Phase 5 packet model have a documented relationship and no confusing duplicate canonical path.
3. Live preview operations register preview artifacts with IDs and original hashes, and rendered context can cite them.
4. Evidence collected from live LSP operations carries server ID, generation, freshness, capability decision, and document hash/version where available.
5. Model-tier rendering is applied in the production agent prompt path.
6. Security review and hunk navigation consume the new packet model or explicitly bridge to it.
7. TUI/tool output can show status, truncation, stale state, and preview artifacts.
8. Opportunistic and required LSP modes are enforced in production pathways.
9. Tests cover live adapter integration, packet rendering, preview registration, no mutation, stale/degraded fallback, and hunk/review specificity.
10. Phase 2–4 regression suites remain green.

## Primary Files

Likely production files:

```text
crates/egglsp/src/context.rs
crates/egglsp/src/context_renderer.rs
crates/egglsp/src/degradation_policy.rs
crates/egglsp/src/evidence_collector.rs
crates/egglsp/src/preview_registry.rs
crates/egglsp/src/security_context.rs
crates/egglsp/src/tui_summary.rs
crates/egglsp/src/lib.rs
src/agent/turn_runtime.rs
src/agent/prompt.rs
src/core/runtime_deps.rs
src/core/daemon.rs
src/tool/lsp.rs
src/tool/lsp_security.rs
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/tui/app/mod.rs
src/tui/components/status_bar.rs
```

Likely test files:

```text
tests/phase5_context_integration.rs
tests/lsp_composite_stdio.rs
tests/security_context_stdio.rs
tests/hunk_nav_stdio.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
```

Documentation:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase5_agent_context_and_workflow_integration.md
```

## Non-Goals

Do not:

- apply LSP previews automatically;
- execute `workspace/executeCommand`;
- add new language-server profiles;
- add new protocol methods beyond existing Phase 4 surface;
- create a persistent semantic index;
- redesign the TUI beyond minimal summaries/details;
- replace all legacy semantic-context tooling in one risky rewrite;
- block normal agent turns when LSP is unavailable unless mode is explicitly `Required`.

# Pass 1 — Define the Canonical Phase 5 Context Boundary

## Current Problem

There are now at least two context packet shapes:

```text
egslp::context::LspContextPacket
src/tool/lsp.rs::SemanticContextPacket
```

The old packet is deeply tied to existing tool output and security/hunk machinery. The new packet is broader, provenance-rich, and designed for Phase 5. Both can coexist, but the repo needs one documented canonical path.

## Required Decision

Choose one of the following policies.

### Preferred Policy: New Packet Is Canonical, Old Packet Is Adapter Output

Use `LspContextPacket` as the internal canonical agent/review packet.

Keep `SemanticContextPacket` as a tool/API compatibility DTO for the existing `semanticContext` command.

Add conversion:

```rust
impl TryFrom<SemanticContextPacket> for LspContextPacket
```

or a dedicated function:

```rust
fn semantic_context_to_lsp_packet(packet: SemanticContextPacket, provenance: LspEvidenceProvenance)
    -> LspContextPacket
```

### Alternative Policy: Old Packet Remains Tool-Specific, New Packet Used Only in Agent Context

If conversion is too much churn, explicitly document:

```text
SemanticContextPacket = legacy/tool-facing detailed source-context response.
LspContextPacket = Phase 5 agent/review packet and rendering unit.
```

Add code comments at both type definitions and architecture docs.

## Required Work

1. Add documentation comments to both packet definitions.
2. Add `architecture/lsp.md` section: “Context packet layers”.
3. Add tests ensuring whichever conversion/bridge exists preserves:

```text
diagnostics
symbols
definitions
references
truncation notes
freshness/provenance where available
```

## Acceptance Criteria

- A future maintainer can tell which packet to use for agent context.
- There is no ambiguous duplicate canonical model.

# Pass 2 — Add Real LSP Evidence Adapter over `LspService` / `LspTool`

## Current Problem

`LspEvidenceProvider` is testable but tuple-heavy and mock-focused. The production path currently injects a compact status/context section via `LspTool::lsp_context_for_agent()` rather than collecting a task-specific packet from live operations.

## Required Adapter

Add a production adapter:

```rust
pub struct ServiceLspEvidenceProvider {
    service: Arc<LspService>,
    allowed_root: PathBuf,
}
```

or, if `LspTool` is the correct boundary:

```rust
pub struct ToolLspEvidenceProvider<'a> {
    tool: &'a LspTool,
}
```

The adapter should implement `LspEvidenceProvider` using existing typed operations and service state.

## Preserve Provenance

The trait currently returns simplified tuples. Either extend the trait or wrap adapter output so the collector can record:

```text
server_id
server_generation
operation
capability decision
freshness
document version or file hash
post_restart
source file
range
```

Preferred extension:

```rust
pub struct EvidenceLocation {
    pub file: PathBuf,
    pub range: Option<LineRange>,
    pub excerpt: Option<String>,
    pub symbol: Option<String>,
    pub provenance: LspEvidenceProvenance,
}

pub struct EvidenceDiagnostic { ... }
pub struct EvidenceCompletion { ... }
pub struct EvidenceSemanticTokenSummary { ... }
```

Then update the provider trait to return DTOs rather than plain tuples.

If this is too large for one pass, add an adapter-local provenance enrichment layer and keep tuple trait for tests.

## Capability Behavior

Every adapter method must:

1. check capability decision first;
2. return a structured unsupported note or empty evidence according to context mode;
3. avoid raw unchecked methods;
4. never call preview-producing operations unless the request explicitly asks for previews;
5. never execute code-action commands.

## Tests

Add fake-service or fake-tool adapter tests:

```text
service_provider_records_server_generation
service_provider_records_capability_unsupported
service_provider_uses_checked_definition
service_provider_uses_checked_references
service_provider_does_not_execute_code_actions
service_provider_marks_retained_diagnostics_stale
```

## Acceptance Criteria

- Production context packets can be assembled from real service/tool state.
- Provenance is not discarded into unstructured strings.

# Pass 3 — Feed Real Task/Hunk/Review Context into Agent Turns

## Current Problem

`DefaultTurnRuntime` appends generic `lsp_context_for_agent()` when an LSP service exists. It does not pass the current task, diff, changed files, hunks, or reviewer mode into the collector.

## Required Inputs

Extend `TurnRunInput` or a narrower context builder with optional workflow metadata:

```rust
pub struct LspAgentContextInput {
    pub mode: LspContextMode,
    pub model_tier: ModelTier,
    pub changed_files: Vec<PathBuf>,
    pub hunks: Vec<HunkDescriptor>,
    pub active_file: Option<PathBuf>,
    pub cursor_position: Option<Position>,
    pub review_mode: bool,
    pub security_review_mode: bool,
    pub budget: LspContextBudget,
}
```

Do not require all fields at first. Start with changed files and hunks if available.

## Source of Hunk/Diff Data

Use existing diff/hunk tracking if available. If not, add a small adapter that can derive hunks from the current pending diff in review workflows.

Do not invent a new diff parser if one already exists.

## Production Behavior

When the turn begins:

```text
if LSP disabled -> no section
if no task/diff metadata -> compact status-only section
if changed files/hunks exist -> collect Review/Hunk packet
if security review mode -> collect Review packet with security evidence
if required mode and collection fails -> explicit failure
if opportunistic mode and collection fails -> partial packet + notes
```

## Model Tier

Use existing model profile or a simple tier mapping:

```text
small -> minimal diagnostics + hunk definitions
workhorse -> diagnostics + hunk refs + hover
frontier/planner -> broader references/implementations/workspace symbols
```

## Tests

```text
turn_runtime_status_only_without_diff
turn_runtime_collects_hunk_packet_when_hunks_present
turn_runtime_uses_model_tier_budget
turn_runtime_security_mode_requests_security_evidence
turn_runtime_opportunistic_lsp_failure_does_not_fail_turn
turn_runtime_required_lsp_failure_fails_before_agent_loop
```

## Acceptance Criteria

- Agent turns receive relevant LSP evidence, not only generic status.

# Pass 4 — Wire Preview Registry into Live Preview Operations

## Current Problem

`PreviewArtifactRegistry` exists, but live `renamePreview`, `formatPreview`, and `sourceActionPreview` tool operations may not register artifacts in session/turn state yet.

## Required Registry Integration

Add a per-session or per-turn registry owner. Preferred locations:

```text
artifact_store
turn runtime state
LspTool state if session-scoped
```

Avoid global mutable state.

## Preview Metadata

Every registered artifact should include:

```text
preview_id
operation
created_at
affected files
original hashes
stale-base state
edit count
capability provenance
server_id/generation
not_applied = true
```

## Tool Output

Update preview-producing tool output to include:

```text
preview_id
not_applied: true
summary
```

## Agent Rendering

When a packet references preview artifacts, render:

```text
Preview artifacts:
- renamePreview#abc123: 2 files, 4 edits, stale=false, not applied
```

## Tests

```text
rename_preview_registers_live_artifact
format_preview_registers_live_artifact
code_action_preview_registers_live_artifact
preview_artifact_contains_original_hashes
preview_artifact_not_applied_true
preview_render_cites_preview_id
preview_operation_does_not_mutate_disk
```

## Acceptance Criteria

- Preview registry is wired into actual tool operations.
- Agents can refer to preview IDs without applying edits.

# Pass 5 — Integrate Security Review with New Context Packet

## Current Problem

Security context has deterministic risk tagging and tests, but the production security review/tool path needs to consume or bridge Phase 5 packets in a stable way.

## Required Work

1. Add a security-specific request builder:

```rust
fn build_security_lsp_context_request(changed_files: &[PathBuf], hunks: &[HunkDescriptor])
    -> LspContextRequest
```

2. Add a converter from `LspContextPacket` to current security packet notes/evidence.
3. Include summary counts:

```text
diagnostics
references
implementations
public API fanout
risk tags
stale evidence
truncated sections
```

4. Ensure code actions are never executed and preview artifacts are included only when explicitly requested.

## Tests

```text
security_review_uses_lsp_context_packet
security_review_public_api_fanout_from_references
security_review_stale_lsp_evidence_marked
security_review_budget_truncation_visible
security_review_no_code_action_execution
security_review_degrades_without_lsp
```

## Acceptance Criteria

- Security review uses Phase 5 evidence rather than a separate ad hoc path, or the bridge is explicit and tested.

# Pass 6 — Integrate Hunk Navigation with New Context Packet

## Current Problem

Hunk context tests exist, but production hunk/source navigation should use or bridge the Phase 5 context packet and carry provenance/truncation consistently.

## Required Work

1. Add hunk request construction from the existing hunk navigation request.
2. Prefer hunk-local diagnostics and references in rank scoring.
3. Add hunk context notes to existing hunk navigation output.
4. Preserve existing hunk nav behavior and stats.
5. Add provenance and freshness labels to hunk evidence summaries.

## Tests

```text
hunk_nav_uses_phase5_packet
hunk_nav_preserves_existing_stats
hunk_nav_hunk_local_items_rank_first
hunk_nav_records_stale_evidence
hunk_nav_reference_cap_visible
hunk_nav_degrades_without_lsp
```

## Acceptance Criteria

- Hunk workflows get Phase 5 provenance-rich evidence without breaking existing hunk nav semantics.

# Pass 7 — Apply Model-Tier Rendering in the Production Agent Path

## Current Problem

The renderer supports model-tier behavior, but the production turn path needs to pass actual model tier and use tier-specific rendering.

## Required Work

1. Map existing `ModelProfile` or resolved model metadata into renderer tier:

```rust
pub enum LspRenderModelTier {
    Small,
    Workhorse,
    Frontier,
}
```

2. Pass this tier into `lsp_context_for_agent()` or directly into `render_lsp_context_for_agent()`.
3. Ensure smaller models receive shorter sections and fewer broad references.
4. Ensure truncation notes remain visible for all tiers.

## Tests

```text
small_model_agent_lsp_context_is_minimal
workhorse_model_agent_lsp_context_includes_references
frontier_model_agent_lsp_context_includes_broader_summary
all_tiers_include_lsp_status_and_truncation_notes
```

## Acceptance Criteria

- The real prompt path uses model-tier-aware rendering.

# Pass 8 — Expand TUI/Tool Summaries Beyond One-Line Status

## Current Problem

A one-line LSP status in the status bar is useful but not enough to inspect stale/truncated/preview state.

## Required Minimal Detail Surface

Expose a compact details string or command/tool output:

```text
LSP: ready rust-analyzer gen=4
Context: 12 items, 3 diagnostics, 4 refs, truncated=true
Freshness: 10 fresh, 2 stale-after-edit
Previews: rename#abc123 stale=false not-applied
Unsupported: implementation unsupported by basedpyright
```

If there is an existing panel/log/event system, emit a structured summary event. If not, add a rendering helper and tests only.

## Tests

```text
tui_summary_lists_context_counts
tui_summary_lists_stale_counts
tui_summary_lists_preview_ids
tui_summary_lists_unsupported_operations
tui_summary_handles_empty_context
```

## Acceptance Criteria

- Users can inspect what LSP added and whether it is stale/truncated.

# Pass 9 — Replace Tuple-Only Mock Tests with Adapter + Production-Seam Tests

## Current Problem

Mock provider tests are good, but completion requires tests proving the production adapter and live tool paths feed Phase 5 packets.

## Required Tests

Add fake-server or fake-service tests for:

```text
production_adapter_collects_diagnostics
production_adapter_collects_definition_and_references
production_adapter_collects_hover_signature_completion
production_adapter_collects_semantic_token_summary
production_adapter_collects_workspace_symbols
production_adapter_records_capability_unsupported
production_adapter_records_generation_and_freshness
production_preview_registers_artifact
agent_turn_injects_hunk_specific_lsp_context
```

Use fake LSP server where possible. Do not require real servers for Phase 5 CI.

## Acceptance Criteria

- Phase 5 is not only tested through tuple mocks.

# Pass 10 — Regression, Docs, and Completion Gate

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

## Documentation Updates

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase5_agent_context_and_workflow_integration.md
```

Document:

- canonical packet boundary;
- production evidence adapter;
- task/hunk/review context input path;
- preview registry integration;
- security review bridge;
- hunk navigation bridge;
- model-tier rendering in real path;
- TUI detail surface;
- adapter/production-seam tests;
- remaining limitations.

## Completion Status Wording

Before all passes:

```text
Phase 5 in progress: core context packet, renderer, preview registry, degradation policy, and initial prompt/TUI integration exist; workflow-depth hardening remains.
```

After all passes:

```text
Phase 5 complete: Codegg assembles bounded, provenance-rich LSP context for agent, hunk, review, and security workflows; preview-producing operations are registered as non-applied artifacts; model-tier rendering is active in production prompts; stale/degraded/unsupported states are explicit; and deterministic tests cover budgets, fallback modes, preview safety, production adapter seams, and UI summaries.
```

# Execution Order for a Smaller Model

1. Define canonical context packet relationship.
2. Add production evidence adapter.
3. Feed real hunk/diff/review context into agent turns.
4. Wire live preview registry into preview-producing operations.
5. Bridge security review to Phase 5 packets.
6. Bridge hunk navigation to Phase 5 packets.
7. Apply model-tier rendering in production prompt path.
8. Expand TUI/tool details summary.
9. Add production adapter/fake-server integration tests.
10. Run regression and update docs.

# Recommended Commit Sequence

```text
1. docs(lsp): define canonical Phase 5 context packet boundary
2. feat(lsp): add production evidence provider adapter
3. feat(agent): collect task-aware LSP context for turns
4. feat(lsp): register live preview artifacts from LSP preview tools
5. feat(security): bridge Phase 5 packets into security review
6. feat(lsp): bridge Phase 5 packets into hunk navigation
7. feat(agent): enable model-tier LSP context rendering
8. feat(tui): expose detailed LSP context and preview summaries
9. test(lsp): add production adapter and preview registry integration tests
10. docs(lsp): close Phase 5 workflow integration
```

# Mandatory Final Checklist

- [ ] Canonical packet boundary is documented.
- [ ] Production adapter exists and uses checked LSP operations.
- [ ] Evidence provenance includes server/generation/freshness/capability where available.
- [ ] Agent turns receive task-aware or hunk-aware context when metadata exists.
- [ ] Generic status-only context remains fallback when metadata is absent.
- [ ] Live preview operations register preview IDs.
- [ ] Preview artifacts include original hashes and `not_applied = true`.
- [ ] Security review consumes or bridges Phase 5 packets.
- [ ] Hunk navigation consumes or bridges Phase 5 packets.
- [ ] Production prompt path uses model-tier rendering.
- [ ] TUI/tool detail summaries show counts, stale/truncated state, and previews.
- [ ] Opportunistic mode degrades without failing normal turns.
- [ ] Required mode fails explicitly.
- [ ] No preview path mutates disk.
- [ ] Production adapter/fake-server tests exist.
- [ ] Phase 2–4 regressions remain green.

# Final Handoff Output

The implementing model must report:

```text
commits created
canonical packet policy
production adapter file and tested operations
agent turn metadata path
hunk/review/security bridge details
preview registry live wiring
model-tier rendering behavior
TUI/detail summary examples
new production-seam tests
no-mutation evidence
fallback mode behavior
workspace check, Clippy, and test results
remaining limitations
```

After this plan passes, Phase 5 can be considered complete as an LSP-informed agent workflow layer rather than just a library-level context abstraction.
