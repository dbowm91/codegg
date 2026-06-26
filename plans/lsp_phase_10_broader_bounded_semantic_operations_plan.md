# LSP Phase 10 Plan: Broader Semantic Operations via Bounded Packets

Status date: 2026-06-26
Phase type: semantic capability expansion / packet design
Prerequisites: Phase 9 lifecycle/status/root/apply-handoff ergonomics substantially complete.

## Purpose

Phase 10 should expand Codegg's semantic LSP capability, but only through bounded, provenance-carrying packet abstractions. The repo already has the canonical `LspContextPacket`, named workflow recipes, hunk/source navigation, security enrichment, preview artifacts, and model-tier rendering. Phase 10 should add higher-level semantic operations that serve concrete workflows without exposing raw LSP sprawl or unbounded prompt expansion.

The theme is: add semantic depth only when the operation can be represented as a bounded packet with clear inputs, budgets, truncation, freshness, and renderer behavior.

## Current baseline

Existing primitives include:

- `LspContextRequest::{File, Hunk, Symbol, Review}`.
- `LspContextPacket` with items, previews, preview IDs, budget/truncation, freshness, source tags, server generation, notes, and operational state.
- Item kinds for diagnostics, definitions, declarations, references, implementations, highlights, hover, signature help, completion summaries, semantic-token summaries, workspace symbols, and operational notes.
- Phase 7 recipes for repair, review, security enrichment, hunk navigation, and preview suggestions.
- Phase 8 preview artifacts and read-only apply-candidate export.
- Phase 9 planned lifecycle/root/apply-handoff ergonomics.

Phase 10 should not create parallel DTOs for each new operation. It should extend the canonical packet model or add narrow typed request variants that lower into `LspContextPacket`.

## Non-goals

Do not add persistent semantic memory/cache. That is Phase 12.

Do not add broad free-form raw LSP request execution.

Do not let models request arbitrary unbounded workspace-wide references.

Do not apply edits directly.

Do not execute server commands.

Do not add operations without render tests and budget tests.

## Candidate operations

Implement only the operations that have clear workflow value and bounded behavior. Recommended order:

1. Impact-analysis packet.
2. Test-failure repair packet.
3. Dependency/interface boundary packet.
4. Cross-file repair packet.
5. Call-neighborhood packet.
6. Optional completion/signature micro-context packet for active edit assistance.

## Workstream 1: packet extension design

### Problem

New semantic operations need richer request shapes, but the repo must avoid creating parallel context models.

### Target files

- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `crates/egglsp/src/bridges.rs`
- `architecture/lsp.md`

### Proposed request additions

Add variants only as needed. Candidate shapes:

```rust
pub enum LspContextRequest {
    // existing variants...
    ImpactAnalysis { symbol: SymbolTarget, changed_files: Vec<PathBuf>, max_depth: u8 },
    TestFailureRepair { test_file: PathBuf, failure_message: String, related_files: Vec<PathBuf> },
    InterfaceBoundary { file: PathBuf, symbol: Option<String>, include_implementations: bool },
    CrossFileRepair { primary_file: PathBuf, related_files: Vec<PathBuf>, ranges: Vec<LineRange> },
    CallNeighborhood { file: PathBuf, line: u32, column: u32, direction: HierarchyDirection, max_depth: u8 },
}
```

Prefer smaller request structs if enum variants become too large.

### Design rules

Every new request must define:

- required inputs,
- optional inputs,
- default budget,
- maximum budget,
- freshness behavior,
- unsupported-capability behavior,
- truncation behavior,
- renderer section layout,
- model-tier behavior,
- fallback behavior when LSP unavailable.

### Acceptance criteria

- New operations extend or lower into `LspContextPacket`.
- No new canonical packet is introduced.
- Each request has budget/truncation tests.

## Workstream 2: impact-analysis packet

### Purpose

Help review refactors, renames, API changes, and security-sensitive symbol changes by collecting bounded evidence around affected references and definitions.

### Inputs

- target symbol or file/line/column,
- changed files/hunks,
- max references,
- max files,
- max depth,
- model tier.

### Evidence to collect

- definition/declaration of target,
- references capped by file and count,
- implementations if supported and explicitly enabled,
- document highlights near changed hunks,
- diagnostics in affected files,
- shallow call hierarchy when supported and tier permits,
- preview IDs if rename/related preview exists, but not by default.

### Budget defaults

- Small: same-file definition + hunk-local references only.
- Workhorse: cross-file references capped at 20, affected files capped at 5.
- Frontier: references capped at 50, files capped at 10, shallow hierarchy depth 1.

### Implementation steps

1. Add `ImpactAnalysisRequest` or enum variant.
2. Collect definition first, then references, then diagnostics for affected files.
3. Rank same-file and hunk-local references higher.
4. Tag source as `AgentContextSource::LspContext` or `Hunk` when reference intersects hunk.
5. Add truncation notes for dropped references/files.
6. Add renderer section `Impact Analysis` with affected symbol and limits.
7. Add tests:
   - symbol with many references truncates,
   - unsupported references produce operational note,
   - hunk-local references are prioritized,
   - no definition does not fail packet,
   - stale evidence renders warning.

### Acceptance criteria

- Impact analysis returns bounded packet evidence.
- It is safe for large repos by default.
- Renderer is concise and source/provenance-aware.

## Workstream 3: test-failure repair packet

### Purpose

Help agents repair failing tests by connecting a failure message to nearby test symbols, implementation definitions, diagnostics, and references.

### Inputs

- test file path,
- failure output/message,
- optional line/column from test harness,
- optional related source files from test runner or user,
- max files/ranges.

### Evidence to collect

- diagnostics in test file and related files,
- symbols in test file,
- likely test function range based on failure message or line,
- definitions/references for named functions/types extracted from failure text,
- hover/signature for target function if positioned,
- implementation links for traits/interfaces if supported,
- operational note if symbol extraction is heuristic.

### Implementation steps

1. Add a conservative failure-message symbol extractor. It must not hallucinate; extract only obvious identifiers, test names, file paths, line numbers, and Rust/Python/TS-style symbols.
2. Add `TestFailureRepair` request/recipe helper.
3. Prefer test-local symbols and diagnostics over global workspace search.
4. Use LSP workspace symbols only under strict caps.
5. Render `Failure-linked evidence` separately from general diagnostics.
6. Add tests with synthetic Rust failure output and Python/TS-style paths if cheap.

### Acceptance criteria

- Test repair packet is useful without scanning the whole repo.
- Heuristic extraction is labeled as heuristic.
- Unsupported/missing LSP produces fallback notes, not false confidence.

## Workstream 4: dependency/interface boundary packet

### Purpose

Support review of API boundary changes: traits/interfaces, public functions, type aliases, exported structs/enums, imports, and dependency-facing symbols.

### Inputs

- file path,
- optional symbol name or line/column,
- changed file/hunks,
- include implementations flag,
- public API focus flag.

### Evidence to collect

- document symbols for exported/public items,
- definitions/declarations for changed boundary symbols,
- implementations for traits/interfaces if supported,
- references outside the changed file capped by file count,
- diagnostics in dependent files,
- hover/signature summaries for boundary functions,
- security risk tags if requested by review/security recipe.

### Implementation steps

1. Add item scoring for public/exported symbols where language info is available.
2. Add packet request or recipe helper for interface boundary review.
3. Keep implementation expansion disabled by default for small tier.
4. Render with sections: `Boundary symbols`, `External references`, `Implementations`, `Diagnostics`.
5. Add tests for Rust trait impls if fake server supports it; otherwise use mock provider.

### Acceptance criteria

- Boundary packet helps review public API impact without global bloat.
- Cross-file references are capped and prioritized.
- Unsupported implementation capability is noted cleanly.

## Workstream 5: cross-file repair packet

### Purpose

Allow repair workflows to gather bounded evidence across a small set of related files, without resorting to wide workspace scans.

### Inputs

- primary file,
- related files,
- line ranges or hunks,
- max files,
- max diagnostics/references/symbols,
- model tier.

### Evidence to collect

- diagnostics in primary and related files,
- symbols in primary ranges,
- definitions/references around changed symbols,
- hunk-local evidence where available,
- root/lifecycle notes from Phase 9.

### Implementation steps

1. Add `CrossFileRepair` request/recipe helper.
2. Enforce `related_files.len() <= max_files` before collection.
3. Require explicit related files; do not infer broad repo scope in Phase 10.
4. Rank primary-file evidence above related-file evidence.
5. Add tests for file cap and ranking.

### Acceptance criteria

- Cross-file repair remains bounded and explicit.
- It cannot accidentally scan the whole workspace.
- Render output clearly separates primary and related evidence.

## Workstream 6: call-neighborhood packet

### Purpose

Provide shallow call hierarchy around a symbol for repair/review/security tasks.

### Inputs

- file,
- line,
- column,
- direction: incoming/outgoing/both,
- max depth,
- max callers/callees,
- model tier.

### Rules

- Default max depth must be 1.
- Depth > 1 must require explicit request and stricter caps.
- Do not recursively expand across the whole graph.
- Unsupported call hierarchy must return operational note.

### Implementation steps

1. Add `CallNeighborhood` request/recipe helper.
2. Use existing call hierarchy operations.
3. Deduplicate cycles by symbol/file/range.
4. Add truncation notes for edge and range caps.
5. Render incoming/outgoing separately.
6. Add tests for caps, unsupported operation, and cycle guard.

### Acceptance criteria

- Call-neighborhood packet is bounded and cycle-safe.
- Security/review recipes can opt into it without uncontrolled expansion.

## Workstream 7: renderer and tier policy updates

### Target files

- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- tests for renderer output

### Implementation steps

1. Add renderer sections for each implemented packet type.
2. Ensure small-tier output stays concise.
3. Ensure frontier-tier output includes richer evidence only under caps.
4. Render unsupported capabilities and stale state prominently.
5. Add snapshot-like tests that verify key section names and omissions.

### Acceptance criteria

- New packets render predictably.
- Model-tier differences are tested.
- Truncation/stale notes are visible.

## Workstream 8: documentation

Update:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `plans/lsp_phase_6_12_roadmap.md` only if roadmap status is actively maintained.

Document:

- each new bounded operation,
- inputs and caps,
- fallback behavior,
- model-tier render behavior,
- explicit non-goals and safety boundary.

## Test matrix

Required focused tests:

```bash
cargo fmt --check
cargo test -p egglsp workflow_recipes
cargo test -p egglsp context_renderer
cargo test -p egglsp evidence_collector
cargo test --test phase5_context_integration lsp
```

Recommended broader checks:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 10 is complete when:

- at least two high-value bounded semantic operations are implemented and documented,
- all new operations return or lower into `LspContextPacket`,
- every new operation has explicit caps and truncation behavior,
- renderer output is tier-aware and tested,
- no raw unbounded LSP execution surface is introduced,
- no mutation-producing LSP operation is executed,
- stale/lifecycle notes from Phase 9 are preserved in packets and rendering.
