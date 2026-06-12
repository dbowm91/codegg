# LSP Hunk Source Context Review Integration Plan

## Purpose

`hunkSourceContext` now has a solid first implementation and hardening pass. The next phase should integrate it into Codegg review/edit-planning workflows so agents use hunk-aware evidence by default when reasoning over diffs.

This plan focuses on integration and routing. It should not add broad new LSP primitives or make `hunkSourceContext` mutate files.

## Current State

Recent LSP work established:

- `SemanticContextResponse` as the shared semantic read model.
- `SemanticContextCollector` for generic semantic evidence.
- `hunkSourceContext` as a read-only LSP operation that maps unified-diff hunks to:
  - enclosing symbols;
  - related symbols;
  - intersecting and nearby diagnostics;
  - definitions;
  - references;
  - optional compact hierarchy;
  - source excerpts;
  - diagnostic freshness/unavailable metadata.
- `HunkSourceNavigator` as a pure navigator that consumes `SemanticContextResponse` and hunk descriptors without calling LSP directly.
- Hardening fixes:
  - diagnostic line indexing converted correctly from internal 0-indexed diagnostics to 1-indexed hunk ranges;
  - malformed hunk headers return structured errors;
  - truncation flags use raw pre-cap counts;
  - multi-file patches are rejected for single-file `hunkSourceContext` requests;
  - output/docs accurately state that the full `SemanticContextResponse` is not returned;
  - multiple hunks disclose first-hunk-centered semantic collection.

## Non-Goals

Do not implement automatic patch application.

Do not make `hunkSourceContext` write files.

Do not add multi-file semantic collection in this pass unless needed as a small helper for integration; prefer a later dedicated pass.

Do not add security hunk enrichment yet.

Do not move overlay/source-action ownership into the hunk navigator.

Do not require live LSP servers in new unit tests.

## Target Architecture

Use `hunkSourceContext` as a deterministic evidence-gathering step for diff-aware review and planning:

```text
Diff / patch / changed-file context
        │
        ▼
Review/edit-planning workflow decides hunkSourceContext is useful
        │
        ▼
LSP tool call: hunkSourceContext
        │
        ▼
HunkSourceNavigationResponse
        │
        ▼
Agent-facing context summary / prompt section / reviewer notes
        │
        ├── code review
        ├── edit planning
        └── later: security hunk enrichment
```

The integration should preserve existing tool autonomy/permission boundaries: `hunkSourceContext` is read-only and should be safe to call in planning/review contexts.

## Phase 1 — Inventory Diff-Aware Review Entry Points

Find the current code paths that already know about diffs, patches, or changed files.

Likely search targets:

- code review agent / reviewer prompts;
- edit planning / plan generation;
- patch preview / apply_patch adjacent paths;
- PR review or diff review code if present;
- security review planner if it consumes diffs;
- any prompt builder that already inserts unified diffs into context.

Search terms:

```text
review
diff
patch
hunk
changed file
apply_patch
security review
plan from diff
```

Acceptance criteria:

- Identify the smallest integration point where hunk evidence can be added without broad refactor.
- Document whether the integration point is tool-call based, prompt assembly based, or planner-policy based.
- Avoid adding hunk logic in multiple places for the first pass.

## Phase 2 — Define Hunk Evidence Routing Policy

Add a small policy layer that decides when to call `hunkSourceContext`.

Suggested policy:

Call `hunkSourceContext` when:

- there is a single changed file and a unified diff is available;
- the file extension is likely covered by LSP support;
- the diff has at least one hunk;
- the diff is not too large for configured caps;
- the agent is in review, edit-planning, or pre-edit analysis mode.

Do not call it when:

- no file path is known;
- the patch spans multiple files and no per-file split is available;
- the file is binary or generated;
- the operation is already inside a tool preview path where LSP overlay/source-action semantics would be redundant;
- user/tool settings disable LSP context.

Suggested policy type:

```rust
pub struct HunkSourceContextPolicy {
    pub enabled: bool,
    pub max_patch_bytes: usize,
    pub max_hunks: usize,
    pub include_definitions: bool,
    pub include_references: bool,
    pub include_call_hierarchy: bool,
    pub include_type_hierarchy: bool,
}

pub enum HunkSourceContextDecision {
    Use { file_path: PathBuf, patch: String },
    Skip { reason: String },
}
```

Acceptance criteria:

- The decision is explicit and testable.
- Skip reasons are visible in debug logs or planner notes, not silently swallowed.
- Defaults are conservative: definitions/references on, hierarchy off.

## Phase 3 — Add a Planner/Reviewer Context Adapter

Create an adapter that converts `HunkSourceNavigationResponse` into a compact agent-facing section.

Do not dump raw JSON into prompts by default. Produce a compact, stable summary.

Suggested summary shape:

```text
Hunk Source Context
File: src/foo.rs
Diagnostic evidence: Fresh/PossiblyStale/Stale/Unavailable, age_ms=...
Notes: ...

Hunk src/foo.rs:0:42-48
Focus: lines 38-52
Enclosing symbol: parse_request function lines 31-70
Diagnostics: 2 in hunk, 1 nearby
Definitions: 1 intersecting
References: 3 intersecting
Related symbols: validate_header, parse_body
Truncation: references truncated
```

Rules:

- Always include diagnostic freshness if present.
- Include notes for unavailable/stale diagnostics.
- Include hunk id and focus range.
- Include enclosing symbol and related symbols.
- Include counts and top few messages, not full large payloads.
- Preserve truncation flags.
- Do not claim no diagnostics means clean/safe.

Potential location:

- a new prompt/context helper near existing reviewer prompt assembly;
- or a module under `src/lsp/hunk_nav_prompt.rs` if LSP context formatting is kept local.

Acceptance criteria:

- The adapter is deterministic and unit-tested with static `HunkSourceNavigationResponse` fixtures.
- Summary size is bounded.
- Stale/unavailable diagnostic warnings survive summarization.

## Phase 4 — Wire Into Review/Edit-Planning Flow

Use the policy and adapter in one narrow workflow first.

Preferred first integration:

- diff review / pre-edit planning path where a single-file patch is already available.

Implementation options:

Option A: Tool-call recommendation only

- The planner/reviewer inserts a recommendation or todo to call `hunkSourceContext` when policy says useful.
- The model still performs the tool call.
- Lower automation risk, easier to observe.

Option B: Deterministic prefetch

- The harness calls `hunkSourceContext` before invoking the reviewer/planner and inserts the compact summary into context.
- More deterministic, but must account for latency/failure and avoid surprising tool work.

Recommendation:

- Start with Option A unless Codegg already has a deterministic context prefetch pipeline for review tools.
- If there is an established prefetch layer, use Option B with strict caps and fail-open behavior.

Acceptance criteria:

- At least one review/edit-planning flow can use hunk evidence.
- Failure to gather hunk evidence does not block the workflow.
- The workflow records whether hunk context was used or skipped.

## Phase 5 — Add Fail-Open Error Handling

`hunkSourceContext` should improve context, not make review brittle.

Error handling rules:

- Parse error: show a concise note and continue without hunk evidence.
- Multi-file patch rejected: show a note recommending per-file hunk context or future multi-file mode.
- LSP unavailable: continue with diff-only context.
- Stale diagnostics: include hunk evidence but mark low-confidence.
- Path outside root: continue without hunk evidence and record error.

Acceptance criteria:

- Review/edit-planning flow continues when `hunkSourceContext` fails.
- Failures are visible enough for debugging.
- No error is misrepresented as “no issues found.”

## Phase 6 — Tests

Add tests around policy and formatting, not live LSP behavior.

Required policy tests:

- single-file patch with supported extension selects `Use`;
- multi-file patch selects `Skip` or per-file split later;
- oversized patch selects `Skip`;
- disabled policy selects `Skip`;
- binary/generated file selects `Skip` if detection exists.

Required formatter tests:

- includes hunk id and focus range;
- includes enclosing symbol;
- includes diagnostic freshness;
- includes stale/unavailable diagnostic warning;
- includes truncation flags;
- bounds number of diagnostics/symbols/references emitted.

Required workflow tests:

- hunk context success inserts summary or recommendation;
- hunk context failure continues review path with note;
- multi-file patch is handled according to policy.

Acceptance criteria:

- Tests do not require live LSP servers.
- Tests use static hunk response fixtures.
- Tests fail if freshness/truncation metadata is dropped.

## Phase 7 — Documentation

Update docs after implementation.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only for verified facts

Docs should state:

- where `hunkSourceContext` is used in review/edit-planning;
- whether integration is recommendation-based or deterministic prefetch;
- fail-open behavior;
- default caps;
- known limitation: single-file hunk context only;
- known limitation: semantic collection is first-hunk-centered.

Acceptance criteria:

- Docs do not imply whole-program analysis.
- Docs do not imply security proof.
- Docs accurately state whether the model or harness initiates the tool call.

## Phase 8 — Defer Multi-File Collection to a Separate Pass

Do not solve multi-file semantic collection in this pass unless the integration point requires it.

Future design should be:

```text
multi-file diff
    └── group hunks by file
          └── one SemanticContextResponse per file
                └── one HunkSourceNavigationResponse per file
```

Potential future response:

```rust
pub struct MultiFileHunkSourceNavigationResponse {
    pub files: Vec<HunkSourceNavigationResponse>,
    pub notes: Vec<String>,
    pub truncated: bool,
}
```

Acceptance criteria for this pass:

- Multi-file limitation is explicit.
- No wrong-file hunk evidence is produced.
- Review integration can skip or split multi-file diffs safely.

## Phase 9 — Defer Security Hunk Enrichment to a Separate Pass

Security hunk enrichment should come after review/edit-planning integration proves the context useful.

Future direction:

- prioritize risk markers inside changed hunk ranges;
- distinguish changed-range diagnostics from whole-file diagnostics;
- include changed enclosing symbols in security surface notes;
- preserve stale/unavailable diagnostic warnings;
- never treat absence of hunk diagnostics as evidence of safety.

Acceptance criteria for this pass:

- No security-context output shape changes.
- Existing `securityContext` remains stable.
- The review integration does not overclaim security coverage.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test --lib lsp
cargo test -p egglsp
```

If broader workflow code is touched:

```bash
cargo test --all --workspace
cargo clippy --all-targets --all-features -- -D warnings
```

If full workspace tests or clippy are skipped, record the reason in the implementation summary.

## Review Checklist

Before this integration pass is complete:

- There is a clear policy deciding when hunk context is useful.
- At least one review/edit-planning flow uses or recommends `hunkSourceContext`.
- Hunk evidence is summarized compactly, not dumped as unbounded JSON.
- Diagnostic freshness and truncation metadata survive into the summary.
- Fail-open behavior is implemented and tested.
- Multi-file patches are skipped or handled safely.
- Docs accurately describe the integration point and limitations.

## Expected Follow-Up

After this pass:

1. Add multi-file hunk source collection with one semantic response per file.
2. Add optional security hunk enrichment.
3. Evaluate whether first-hunk-centered semantic collection is sufficient or whether per-hunk targeted enrichment is needed.
4. Consider adding deterministic prefetch if the initial integration is recommendation-based and proves useful.
