# LSP Phase 5: Agent Context and Workflow Integration

## Purpose

Begin Phase 5 after the Phase 4 compatibility and evidence work through:

```text
0dc550c91d1d6301584d6bd81afa75ac6750a14f
```

Phase 4 established the LSP substrate:

- normalized capability snapshots and observed runtime capability overlays;
- Tier 1 and Tier 2 compatibility profiles;
- typed read-only operations;
- preview-only rename, formatting, and code-action surfaces;
- real-server smoke fixtures and operation matrices;
- position-encoding-aware semantic token and signature handling;
- shutdown traces and matrix artifact infrastructure;
- fail-closed capability decisions for model-facing tools.

Phase 5 should make this substrate useful to Codegg workflows. The goal is not to add more protocol breadth; it is to turn LSP evidence into bounded, explainable, and safe agent context.

Phase 5 should wire LSP evidence into:

```text
agent context assembly
hunk/source navigation
security review packets
change planning
review/repair loops
TUI-visible summaries
model budget policy
```

The key safety rule remains unchanged:

```text
read-only semantic operations may be executed directly;
mutation-producing LSP operations remain preview-only;
no workspace/executeCommand is executed by Codegg automatically.
```

## Phase 5 Completion Definition

Phase 5 is complete when:

1. Agent prompts can request bounded LSP context packets through a typed internal API.
2. Context packets include diagnostics, definitions/declarations, references, implementations, highlights, hover summaries, signature/completion summaries, and semantic-token excerpts where useful.
3. Context packets are budgeted, deduplicated, source-ranked, and explicitly marked with freshness and capability provenance.
4. Diff/hunk workflows can request LSP context focused on changed symbols and changed ranges.
5. Security review packets can include relevant call/reference/diagnostic evidence without flooding the model.
6. Preview-producing operations return structured artifacts that can be cited in reviews but never auto-applied.
7. TUI surfaces can display concise operation summaries, warnings, and stale/unsupported states.
8. The system degrades cleanly when LSP is unavailable, stale, initializing, degraded, or capability-unsupported.
9. Existing Phase 2–4 regression suites remain green.
10. New Phase 5 integration tests cover context budgets, deduplication, stale evidence, hunk focus, unsupported capability behavior, and preview safety.

## Primary Files

Expected production touch points:

```text
crates/egglsp/src/context.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/capability.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/service.rs
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/lsp/security_context.rs
src/tool/lsp.rs
src/tool/security.rs
src/agent/context.rs
src/agent/review.rs
src/agent/repair.rs
src/tui/*
```

Expected test touch points:

```text
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_semantic_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
tests/lsp_composite_stdio.rs
tests/security_context_stdio.rs
tests/hunk_nav_stdio.rs
```

Documentation:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

If actual file names differ, preserve the intent and adapt locally.

## Non-Goals

Do not implement during Phase 5:

- automatic rename application;
- automatic formatting application;
- automatic code-action application;
- `workspace/executeCommand` execution;
- arbitrary dynamic registration support;
- new language-server profiles beyond the current matrix;
- semantic indexing storage beyond bounded request-time context;
- full IDE UI redesign;
- background autonomous LSP crawling;
- new plugin architecture.

# Design Principles

## 1. LSP Evidence Is Context, Not Authority

LSP outputs should improve review and repair quality, but they must be treated as evidence with provenance:

```text
server_id
server_version if known
operation
capability decision
document version / file hash
generation
freshness
stale/degraded notes
```

The model should see uncertainty when the LSP state is stale or degraded.

## 2. Bounded by Default

Every Phase 5 context API must accept explicit budgets:

```rust
pub struct LspContextBudget {
    pub max_files: usize,
    pub max_ranges_per_file: usize,
    pub max_diagnostics: usize,
    pub max_references: usize,
    pub max_symbols: usize,
    pub max_completion_items: usize,
    pub max_semantic_tokens: usize,
    pub max_bytes: usize,
}
```

Hard default budgets should be conservative.

## 3. Freshness Must Be Visible

Every context item should include:

```text
Fresh
StaleAfterEdit
RetainedAfterRestart
ServerGenerationMismatch
Unknown
```

Do not silently merge stale diagnostics with fresh diagnostics.

## 4. Preview-Only Mutation Evidence

Rename/format/code-action previews may be included in context as proposed changes. They must not be applied unless a later user-approved path explicitly applies them.

## 5. Degrade Without Blocking the Agent

When LSP is unavailable, unsupported, initializing, or degraded, the context assembler should return:

```text
partial packet + notes
```

rather than failing the entire agent workflow, unless the caller explicitly requires LSP evidence.

# Phase 5 Data Model

## LSP Context Packet

Add or refine a stable packet type:

```rust
pub struct LspContextPacket {
    pub workspace_root: PathBuf,
    pub generated_at: SystemTime,
    pub server_id: Option<String>,
    pub server_generation: Option<u64>,
    pub operational_state: LspOperationalStateSummary,
    pub request: LspContextRequest,
    pub budget: LspContextBudget,
    pub items: Vec<LspContextItem>,
    pub previews: Vec<LspPreviewArtifact>,
    pub notes: Vec<String>,
    pub truncation: LspContextTruncation,
}
```

## Context Request

```rust
pub enum LspContextRequest {
    File {
        file: PathBuf,
        line_ranges: Vec<LineRange>,
        include_symbols: bool,
        include_diagnostics: bool,
    },
    Hunk {
        file: PathBuf,
        hunks: Vec<HunkRange>,
        include_references: bool,
        include_definitions: bool,
        include_security_evidence: bool,
    },
    Symbol {
        file: PathBuf,
        position: Position,
        include_references: bool,
        include_implementations: bool,
        include_call_like_context: bool,
    },
    Review {
        changed_files: Vec<PathBuf>,
        hunks: Vec<HunkDescriptor>,
        risk_mode: LspRiskMode,
    },
}
```

## Context Item

```rust
pub enum LspContextItemKind {
    Diagnostic,
    Definition,
    Declaration,
    Reference,
    Implementation,
    DocumentHighlight,
    Hover,
    SignatureHelp,
    CompletionSummary,
    SemanticTokenSummary,
    WorkspaceSymbol,
    OperationalNote,
}

pub struct LspContextItem {
    pub kind: LspContextItemKind,
    pub file: Option<PathBuf>,
    pub range: Option<LineRange>,
    pub symbol: Option<String>,
    pub severity: Option<String>,
    pub message: String,
    pub excerpt: Option<String>,
    pub provenance: LspEvidenceProvenance,
    pub score: LspContextScore,
}
```

## Preview Artifact

```rust
pub enum LspPreviewArtifact {
    Rename(RenamePreview),
    Formatting(FormattingPreview),
    CodeAction(CodeActionPreview),
}
```

Preview artifacts are included only when explicitly requested.

# Pass 0 — Baseline Audit and Stabilization

## Goal

Record current state and identify existing context assembly paths before adding Phase 5 APIs.

## Required Audit

Inspect and document current flows for:

```text
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/tool/lsp.rs
src/tool/security.rs
agent context assembly files
review/repair workflows
```

Map current inputs and outputs:

```text
what context is currently included
what is deterministic
what is model-generated
what is LSP-backed
what is stale/fresh
what is hunk-aware
what is security-review-specific
```

## Required Baseline Tests

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test -p egglsp --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

Do not proceed if baseline does not build.

## Deliverable

Add a short internal note to the Phase 5 plan or `architecture/lsp.md` summarizing current context pathways.

# Pass 1 — Context Packet Core Types and Budgeting

## Goal

Create stable types and budget enforcement independent of live LSP calls.

## Work Items

1. Add `LspContextPacket`, `LspContextRequest`, `LspContextBudget`, `LspContextItem`, `LspEvidenceProvenance`, and `LspContextTruncation`.
2. Add deterministic budget enforcement helpers:

```rust
fn enforce_context_budget(packet: &mut LspContextPacket) -> LspContextTruncation
```

3. Add sorting/dedup helpers:

```rust
fn dedup_context_items(items: Vec<LspContextItem>) -> Vec<LspContextItem>
fn rank_context_items(items: &mut [LspContextItem], request: &LspContextRequest)
```

4. Add explicit default budgets.
5. Ensure all truncation is reflected in `packet.truncation` and `packet.notes`.

## Dedup Key

Use a stable key:

```text
kind + file + normalized range + symbol + message hash
```

Do not deduplicate diagnostics solely by message because the same diagnostic can occur at multiple sites.

## Ranking Rules

Prefer, in order:

```text
changed/hunk ranges
errors over warnings
same file before external files
definitions/declarations before broad references
security-sensitive diagnostics before style diagnostics
fresh evidence before stale evidence
short excerpts before long excerpts
```

## Tests

```text
budget_limits_total_bytes
budget_limits_files
budget_limits_ranges_per_file
budget_limits_diagnostics
budget_truncation_notes_are_recorded
dedup_preserves_distinct_ranges
ranking_prefers_hunk_local_items
ranking_prefers_fresh_items
```

## Acceptance Criteria

- Core packet logic is testable without LSP servers.
- Budget enforcement is deterministic.

# Pass 2 — LSP Evidence Collector API

## Goal

Add a collector that converts existing typed LSP operations into context packet items.

## API Sketch

```rust
pub struct LspEvidenceCollector {
    service: Arc<LspService>,
}

impl LspEvidenceCollector {
    pub async fn collect(&self, request: LspContextRequest, budget: LspContextBudget)
        -> Result<LspContextPacket, LspContextError>;
}
```

If `LspService` is not the right dependency boundary, use the existing service/client abstraction.

## Collection Rules

For each operation:

- call capability decision before request;
- if unsupported, add an operational note rather than failing;
- if unknown, fail closed only for `require_lsp = true` callers;
- capture errors as notes with structured provenance;
- never invoke mutation-producing operations unless the request explicitly asks for preview artifacts;
- never execute commands.

## Evidence Mapping

Map:

```text
diagnostics -> Diagnostic items
definition/declaration -> location items + excerpts
references -> ranked location items
implementation -> implementation items
hover -> concise hover summary
signatureHelp -> active signature summary
completion -> top bounded candidates
semanticTokens -> semantic-token summary only, not giant raw token list
workspaceSymbols -> bounded symbol summary
```

## Excerpt Policy

For each location, include at most:

```text
small range around symbol/hunk
line numbers
trimmed content
hash/version metadata
```

Do not include entire files through this collector.

## Tests

Use fake/composite LSP server tests:

```text
collector_returns_diagnostics_with_provenance
collector_returns_definition_excerpt
collector_limits_references
collector_records_unsupported_notes
collector_handles_unknown_capability_fail_closed_when_required
collector_does_not_execute_code_actions
collector_does_not_apply_rename_or_formatting
```

## Acceptance Criteria

- The collector produces bounded context packets from existing typed operations.

# Pass 3 — Hunk-Focused Context Integration

## Goal

Wire the evidence collector into hunk/source navigation and review workflows.

## Work Items

1. Extend hunk descriptors with symbol positions when available.
2. For each changed hunk, request hunk-local LSP context:

```text
diagnostics overlapping hunk
symbols enclosing hunk
definitions for changed call sites
references for changed symbols, capped
implementations for changed interfaces/classes/traits, capped
semantic-token role summary
```

3. Merge this with existing hunk nav output.
4. Add provenance to hunk source context summary lines.
5. Ensure stale LSP state is visible in hunk summaries.

## Hunk Focus Algorithm

For each hunk:

```text
1. identify changed line range
2. expand by small context window
3. query document symbols / highlights if supported
4. identify best symbol position
5. collect definition/reference/diagnostic evidence under budget
6. rank hunk-local evidence first
```

## Tests

```text
hunk_context_prefers_changed_range_diagnostics
hunk_context_includes_enclosing_symbol
hunk_context_caps_references
hunk_context_marks_stale_evidence
hunk_context_degrades_without_lsp
hunk_context_does_not_include_unrelated_file_flood
```

## Acceptance Criteria

- Hunk navigation receives useful LSP evidence without bloating prompts.

# Pass 4 — Security Review Context Integration

## Goal

Use LSP evidence to improve security review packets without turning security review into an unbounded static analyzer.

## Security-Relevant Evidence

Include only bounded evidence relevant to risk:

```text
diagnostics in changed files
references to changed public functions/classes
implementations of changed interfaces/traits
call-site-like reference clusters
hover/type summaries for changed symbols
semantic-token role summary for changed ranges
rename/code-action/format previews only when explicitly requested
```

## Risk Scoring Inputs

Add deterministic risk tags:

```text
changed public API
changed auth/security-sensitive names
changed unsafe/ffi/network/fs/process code
changed function has broad references
diagnostics introduced in changed hunk
implementation hierarchy affected
```

LSP should contribute evidence, not final security verdicts.

## Packet Shape

Extend security packet notes with:

```text
LSP evidence summary: X diagnostics, Y references, Z definitions, stale=false, truncated=true
```

Add item-level provenance for the reviewer/model.

## Tests

```text
security_packet_includes_changed_diagnostics
security_packet_caps_reference_clusters
security_packet_marks_public_api_reference_fanout
security_packet_degrades_without_lsp
security_packet_never_executes_code_actions
security_packet_budget_truncation_visible
```

## Acceptance Criteria

- Security review gets better semantic context without hidden side effects.

# Pass 5 — Agent Context Assembly and Prompt Integration

## Goal

Expose Phase 5 packets to manager/reviewer/repair agent loops in a controlled way.

## Work Items

1. Add a context source enum:

```rust
pub enum AgentContextSource {
    RepositorySearch,
    Diff,
    Hunk,
    Diagnostics,
    LspContext,
    SecurityContext,
    UserProvided,
}
```

2. Add LSP packet rendering to the existing prompt/context assembler.
3. Render concise sections:

```text
LSP status
Relevant diagnostics
Definitions/declarations
References/implementations
Hover/signature/completion summaries
Preview artifacts
Truncation notes
```

4. Ensure every LSP section is bounded and can be disabled.
5. Add model-budget policy:

```text
small model -> diagnostics + hunk-local definitions only
workhorse -> diagnostics + references + hover
frontier/planner -> broader references/implementation summaries
```

## Prompt Rendering Rules

Do not dump raw JSON. Render stable, readable text:

```text
- file.rs:12-18 [definition, fresh, rust-analyzer gen=3]
  fn validate_token(...)
```

Include notes when unsupported:

```text
LSP note: implementation unsupported by current server profile.
```

## Tests

```text
agent_context_renders_lsp_status
agent_context_respects_small_model_budget
agent_context_includes_truncation_notes
agent_context_omits_disabled_lsp_section
agent_context_does_not_render_raw_large_payloads
```

## Acceptance Criteria

- Agent context can include LSP evidence predictably.

# Pass 6 — Preview Artifact Workflow Integration

## Goal

Make rename/format/code-action previews useful to agents and reviewers while preserving non-mutation.

## Work Items

1. Add a preview artifact registry for the current turn/session if one does not already exist.
2. Store preview metadata:

```text
preview_id
operation
file edits
original hashes
stale-base state
capability provenance
created_at
```

3. Allow context packets to reference preview IDs.
4. Render preview summaries in review output.
5. Provide explicit user-facing language:

```text
Preview only; not applied.
```

6. Keep application outside Phase 5 unless an existing user-approved apply path exists.

## Tests

```text
rename_preview_registers_artifact
format_preview_registers_artifact
code_action_preview_registers_artifact
preview_artifact_records_original_hashes
preview_artifact_marks_stale_base
preview_context_render_says_not_applied
no_preview_operation_mutates_disk
```

## Acceptance Criteria

- Mutation-producing LSP outputs are usable but safe.

# Pass 7 — TUI and User-Facing Summaries

## Goal

Expose LSP evidence to users without flooding the UI.

## Minimal UI Surface

Add or refine a compact LSP status/summary area:

```text
LSP: ready | degraded | initializing | unavailable
server: rust-analyzer gen=3
context: 4 diagnostics, 2 refs, 1 definition, truncated
preview: rename-preview #abc123, 2 files, stale=false
```

If TUI files are not ready for direct integration, expose the summary through existing command/tool output first.

## Required States

Show:

```text
unsupported capability
stale diagnostics
retained diagnostics after restart
server degraded
context truncated
preview stale base
```

## Tests

If UI snapshot tests exist, add them. Otherwise add serialization/rendering tests for the summary model:

```text
lsp_summary_ready
lsp_summary_degraded
lsp_summary_truncated
lsp_summary_preview_stale
```

## Acceptance Criteria

- Users can tell what LSP contributed and whether it is stale or truncated.

# Pass 8 — Degradation and Fallback Policy

## Goal

Ensure agent workflows remain useful when LSP is absent or degraded.

## Fallback Modes

Define:

```rust
pub enum LspContextMode {
    Disabled,
    Opportunistic,
    Required,
}
```

Rules:

```text
Disabled -> no LSP calls, note omitted or disabled
Opportunistic -> return partial packet + notes on failure
Required -> return structured error on unavailable/unsupported required operation
```

Default agent workflows should use `Opportunistic` unless a command explicitly requires LSP.

## Tests

```text
opportunistic_returns_partial_when_server_unavailable
required_fails_when_server_unavailable
unsupported_capability_records_note
initializing_state_records_note
stale_state_records_note
```

## Acceptance Criteria

- LSP issues do not derail normal agent work unless explicitly required.

# Pass 9 — Composite Integration Tests

## Goal

Prove Phase 5 behavior end to end through fake/composite servers.

## Add Tests

At root or egglsp integration layer:

```text
phase5_context_packet_for_diff_hunk
phase5_security_packet_with_lsp_evidence
phase5_agent_context_render_budgeted
phase5_preview_artifact_non_mutating
phase5_degraded_lsp_opportunistic_context
phase5_required_lsp_failure
```

Use the fake LSP server where possible. Real-server tests should remain opt-in.

## Assertions

Each test should verify:

```text
bounded output
provenance present
freshness visible
unsupported/degraded notes visible
no mutation
stable rendering
```

## Acceptance Criteria

- Phase 5 is covered by deterministic CI tests.

# Pass 10 — Documentation and Handoff

## Documentation Updates

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- Phase 5 context-packet model;
- budget and truncation policy;
- hunk-focused LSP context;
- security review LSP evidence;
- agent prompt rendering rules;
- preview artifact safety;
- TUI/user-facing summary semantics;
- fallback modes;
- exact tests added.

## Status Wording

Before completion:

```text
Phase 5 in progress: LSP evidence is being integrated into agent context, hunk review, security review, and preview workflows.
```

After completion:

```text
Phase 5 complete: Codegg can assemble bounded, provenance-rich LSP context packets for hunk, symbol, review, security, and agent workflows. Mutation-producing LSP operations remain preview-only, unsupported/stale/degraded states are explicit, and deterministic tests cover budget, fallback, preview safety, and rendering behavior.
```

# Execution Order for a Smaller Model

1. Audit current context pathways.
2. Add core context packet/budget types and tests.
3. Add evidence collector over existing typed LSP operations.
4. Integrate hunk-focused context.
5. Integrate security review context.
6. Integrate agent prompt rendering.
7. Add preview artifact registry/summaries.
8. Add minimal TUI/tool-output summary model.
9. Add degradation/fallback policy.
10. Add composite integration tests.
11. Update docs and handoff notes.

Do not proceed to preview artifact application in this phase.

# Recommended Commit Sequence

```text
1. feat(lsp): add bounded context packet model
2. feat(lsp): collect typed LSP evidence into context packets
3. feat(lsp): add hunk-focused semantic context assembly
4. feat(security): include bounded LSP evidence in security packets
5. feat(agent): render LSP context packets in prompt assembly
6. feat(lsp): register preview-only LSP artifacts
7. feat(tui): summarize LSP context and preview state
8. feat(lsp): add opportunistic and required context modes
9. test(lsp): add Phase 5 composite context integration tests
10. docs(lsp): document Phase 5 agent context integration
```

# Mandatory Final Checklist

- [ ] Context packet types exist and are documented.
- [ ] Budgets are enforced deterministically.
- [ ] Deduplication preserves distinct ranges.
- [ ] Ranking prefers hunk-local fresh evidence.
- [ ] Evidence collector handles diagnostics, locations, hover, signature, completion, semantic tokens, and workspace symbols.
- [ ] Unsupported capabilities produce notes, not silent omissions.
- [ ] Hunk context includes relevant diagnostics and symbols.
- [ ] Security packets include bounded LSP evidence.
- [ ] Agent context renderer is stable and budget-aware.
- [ ] Preview artifacts are stored and marked preview-only.
- [ ] No LSP preview mutates disk.
- [ ] TUI/tool summaries expose status, truncation, stale state, and preview state.
- [ ] Opportunistic mode degrades gracefully.
- [ ] Required mode fails explicitly.
- [ ] Composite tests cover the full Phase 5 path.
- [ ] Phase 2–4 regression suites remain green.

# Final Handoff Output

The implementing model must report:

```text
commits created
context packet type locations
budget defaults
dedup/ranking policy
evidence collector operation coverage
hunk integration summary
security packet integration summary
agent renderer output example
preview artifact safety evidence
TUI/tool summary example
fallback mode behavior
new tests and results
workspace check, Clippy, and test results
remaining known limitations
```

After this plan passes, Codegg will have moved from an LSP-compatible substrate to a usable LSP-informed agent workflow layer.
