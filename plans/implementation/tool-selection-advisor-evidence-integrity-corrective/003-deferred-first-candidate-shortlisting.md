# Tool-Selection Advisor Evidence Corrective C003 — Deferred-First Candidate Shortlisting

Status: implemented (closed; see `plans/closure/tool-selection-advisor-evidence-integrity-corrective/003-status.md`)

Repository baseline: `71460c0cb1421f33a62d57123ac562c8a7c4bf1c`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c003--deferred-first-candidate-shortlisting`

Predecessor closure:

- `plans/closure/tool-selection-advisor-post-closure-corrective/003-status.md`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability/invariant corrective.

## 1. Objective

Ensure pre-turn advisor candidate limits apply to the final eligible deferred universe rather than the first N tools in resolved-surface order.

Preserve the correct M003 authority seam and visibility-only behavior.

## 2. Discovered defect

Current flow:

```text
ResolvedToolSurface.tools
    -> take(max_candidates)
    -> build advisor candidates
    -> filter candidates to deferred names
```

Because `ResolvedToolSurface` is deterministically ordered, a highly relevant deferred tool at position > `max_candidates` cannot be considered even if only a few deferred tools exist.

This is a recall/quality defect, not an authority defect.

The predecessor test used a two-tool surface and therefore could not expose the ordering problem.

## 3. Invariants

- Authority filtering remains upstream in `ResolvedToolSurface`.
- Candidate construction sees only policy-allowed/callable surface entries.
- Required/never-reduce/immediate tools are not consumed as learned promotion candidates unless a future explicit design says otherwise.
- Candidate cap applies after deferred eligibility filtering.
- For eligible deferred sets larger than the neural budget, preselection considers the entire eligible deferred descriptor universe.
- Preselection cannot promote or execute anything; it only chooses which allowed deferred descriptors reach the learned scorer.
- Off/observe/rerank behavior retains existing semantics.
- Failure/empty shortlist leaves provider palette unchanged.
- Determinism is preserved for the same context/surface/config.

## 4. Target flow

```text
ResolvedToolSurface
      |
      v
intersect with current deferred definitions
      |
exclude required/never-reduce/non-promotable entries
      |
      v
eligible deferred descriptors  <--- full allowed deferred universe
      |
      +-- if <= neural max: all candidates
      |
      +-- if > neural max:
             deterministic lexical/BM25 rank over current bounded context
             take neural max
      |
      v
ToolAdvisor contextual scoring
      |
threshold/abstain/promotion/schema budget
      |
provider initial visibility
```

Do not use raw resolved-surface position as a semantic preselector.

## 5. Reuse existing search machinery

Prefer reuse of the existing `ToolCatalog::rank_descriptors`/BM25 descriptor ranking rather than adding another lexical retriever.

Candidate metadata for preselection should use the same textual fields the advisor understands:

- canonical/wire name as appropriate;
- description;
- category;
- disclosure.

Keep the preselector local and deterministic.

If the eligible deferred universe is small enough, bypass BM25 and send all candidates to the advisor.

## 6. API changes

Replace or supplement `candidates_from_surface(surface, max_candidates)` with an API whose ordering makes the contract explicit, for example:

- build all candidates from an already filtered iterable; or
- `candidates_from_deferred_surface(surface, deferred_names, context, max_candidates)`.

Do not silently change unrelated users of `candidates_from_surface` if reactive `tool_search` depends on its current behavior; use a dedicated pre-turn helper where safer.

## 7. Regression matrix

Required tests:

1. construct >max resolved tools where the only relevant deferred tool is after index max — it must reach the advisor;
2. many immediate/core tools before one deferred tool — immediate tools must not consume neural candidate slots;
3. >max deferred tools — BM25/lexical preselector includes a relevant late-position candidate;
4. stable tie ordering;
5. denied/disabled/plan/parent-ceiling tool never enters eligible set;
6. required/never-reduce tool excluded from learned promotion candidate set;
7. MCP/plugin canonical/wire alias mapping survives preselection;
8. unknown textual tool can enter via descriptor relevance;
9. empty/failed preselection leaves palette unchanged;
10. max-candidate and schema budgets remain enforced independently.

Add candidate-recall instrumentation suitable for C004: whether each labeled relevant deferred tool survived preselection into the learned scorer.

## 8. Performance

Record preselection cost for realistic catalog sizes (for example 16, 64, 128, and the largest testable configured surface). BM25/lexical selection should remain well below provider latency and should avoid schema serialization until after candidate selection where practical.

No new model weights or external dependency are expected.

## 9. Ordered work

A. Isolate final eligible deferred surface construction.
B. Build candidates from that set before limiting.
C. Reuse deterministic descriptor ranker when eligible size exceeds neural cap.
D. Add canonical/wire revalidation after prediction as today.
E. Add late-position and large-catalog tests.
F. Add candidate-recall diagnostics for C004.
G. Update architecture docs.

## 10. Verification

```bash
cargo test --locked -p codegg --lib agent::request_preparation
cargo test --locked -p codegg --lib agent::tool_surface
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked -p codegg --lib tool::tool_search
cargo test --locked -p codegg --test tool_surface_minimization
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

## 11. Acceptance criteria

C003 closes when a relevant deferred tool outside the original first-N surface window can be discovered/promoted, candidate caps are applied only after deferred eligibility/preselection, authority-negative tests remain green, and candidate-recall instrumentation is available to C004.

## 12. Stop conditions

Stop if the fix moves scoring before authority resolution, feeds denied definitions to the advisor, expands the provider schema without promotion, or introduces a second independent search implementation when `ToolCatalog` can satisfy the preselection contract.

## 13. Closure evidence

- before/after late-position regression;
- candidate construction ordering;
- authority-negative matrix;
- large-catalog candidate-recall cases;
- preselection latency;
- exact verification output.
