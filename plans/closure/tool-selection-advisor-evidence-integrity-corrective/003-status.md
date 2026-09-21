# Tool-Selection Advisor Evidence Corrective C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/003-deferred-first-candidate-shortlisting.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c003--deferred-first-candidate-shortlisting`

Repository baseline reviewed: `e4f7805c`

Implementation commits or pull requests:

- C003 implementation (this closure batch) — deferred-first shortlisting

## 1. Executive finding

C003 is closed. Pre-turn advisor candidate limits now apply to the final
eligible deferred universe rather than the first N tools in
resolved-surface order. The corrected flow builds every policy-allowed
deferred descriptor first, applies a deterministic BM25 preselection only
when the universe exceeds the neural budget, and then scores. The M003
authority seam is untouched: `ResolvedToolSurface` remains the only
authority owner, promotion stays visibility-only, and required/never-reduce
tools are excluded before scoring with promotion-time revalidation intact.
A relevant deferred tool outside the original first-N window is now
discovered and promoted; all authority-negative tests remain green; and the
candidate-recall instrumentation C004 needs is in place.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Cap applies after deferred eligibility | `candidates_from_deferred_surface` (full universe, no cap) + `preselect_candidates` in `project_preturn_promotions` | pass | Old `take(max)` path removed |
| Large universes use deterministic lexical preselection | BM25 via `ToolCatalog::rank_descriptors` over name/description/category/disclosure | pass | No second retriever added |
| Late-position relevant deferred tool reaches advisor | `late_position_deferred_tool_reaches_the_advisor` (9 immediate + target, budget 4) | pass | Fails on the old first-N code by construction |
| Immediate tools don't consume neural slots | `immediate_tools_do_not_consume_neural_candidate_slots` | pass | Advisor input contains no immediate names |
| Large-catalog BM25 recall | `large_deferred_catalog_preselects_the_relevant_late_candidate` (11 deferred, budget 3) | pass | |
| Stable tie ordering | `preselection_tie_ordering_is_stable` + mod.rs determinism test | pass | Name tiebreak, repeated runs identical |
| Authority negatives stay out | `denied_and_disabled_tools_never_enter_the_eligible_set` (deny + disable variants, adversarial ranking) | pass | Plan/ceiling omissions flow through the same `surface.tools` seam |
| Required/never-reduce excluded | `required_tools_are_excluded_from_learned_promotion_candidates` (natural `read` path) | pass | Excluded pre-score and revalidated at promotion |
| Canonical/wire aliases survive | `plugin_wire_alias_mapping_survives_preselection` (`mcp__docs_search` → `docs_search`) | pass | |
| Unknown textual tools enter via descriptors | `unknown_textual_tool_enters_via_descriptor_relevance` (novel name, relevant description) | pass | |
| Empty/failed preselection leaves palette unchanged | `empty_and_failed_preselection_leave_the_palette_unchanged` (empty context never calls advisor; injected failure returns empty) | pass | |
| Budgets independent | `candidate_and_schema_budgets_apply_independently` (neural cap 1 of 2; starved schema blocks) | pass | |
| Recall instrumentation for C004 | `PreselectionReport.shortlisted_names` + `preselection_recall` (+ unit test) | pass | |
| Preselection cost below provider latency | `preselection_latency_is_well_below_provider_latency`: 16→0.36 ms, 64→4.6 ms, 128→3.4 ms | pass | Schema serialization still happens after selection |

## 3. Production implementation evidence

- `src/tool_advisor/mod.rs`:
  - Removed `candidates_from_surface` (first-N truncation; single caller,
    no other consumers in `src/`, `tests/`, or `examples/`).
  - Added `candidates_from_deferred_surface(surface, deferred_names)`:
    full eligible deferred universe from authority-filtered surface tools,
    canonical-or-wire deferred matching preserved, required/never-reduce
    excluded, `synthetic_identity` flagging preserved.
  - Added `preselect_candidates(candidates, context, max)` returning the
    shortlist plus a `PreselectionReport` (eligible count, shortlisted
    count, truncation flag, elapsed millis, shortlisted names). Universes
    within budget pass through untouched; oversized universes go through
    BM25 (positive-score rows only); empty BM25 results fall back to stable
    eligible order rather than dropping the universe. No promotion,
    execution, or authority logic in the preselector.
  - Added `preselection_recall(report, relevant)` → `CandidateRecall`
    (total/hit/rate/missing) for C004's large-catalog fixture.
- `src/agent/request_preparation.rs`:
  - `project_preturn_promotions` builds the eligible universe first, then
    preselects, then scores; promotion-side required/never-reduce
    revalidation, threshold/abstain handling, Observe/Rerank semantics,
    schema-budget accounting, and max-promotion caps are unchanged.
  - Added a debug span line recording eligible/shortlisted/truncated/
    preselect-millis per turn.
  - Tests migrated from the fixed-output `PreturnAdvisor` to the
    input-recording `RecordingAdvisor` (plus `tool_definition`,
    `deferred_from_surface`, and `promote_config` helpers); the pre-existing
    disclosure test keeps its assertions under the new harness.
- `architecture/tool-advisor.md`: pre-turn paragraph documents the
  deferred-first contract and measured preselection cost.

Before/after late-position regression: 9 immediate tools plus one deferred
target with `max_candidates = 4`. Before: `take(4)` admitted only immediate
tools, deferred filtering emptied the set, and the advisor never saw the
target (no promotion possible). After: the eligible universe holds the
target, the advisor scores it, and it promotes at threshold. The new test
encodes exactly this geometry.

## 4. Verification executed

### Commands run

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

### Results

- `agent::request_preparation`: 16 passed, 0 failed (10 new C003 tests +
  migrated disclosure test + 5 pre-existing).
- `agent::tool_surface`: 5 passed. `tool::tool_search`: 4 passed.
  `tool_advisor` lib: 30 passed (5 new preselection tests). Training-feature
  lib `tool_advisor`: 49 passed (no C003 regressions in trainer paths).
- `tool_surface_minimization` integration: 13 passed.
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  clean.
- `scripts/verify.sh quick`: passed.
- Local-only verification; no hosted CI for this batch (same standing
  limitation as C001/C002; C004 requires hosted CI per its plan).

## 5. Invariant review

- Authority filtering remains upstream in `ResolvedToolSurface`: eligible
  construction iterates `surface.tools` only; deny/disable/plan/ceiling
  omissions never enter (tested for deny + disable with an adversarial
  ranking; plan/ceiling share the same `surface.tools` seam).
- Candidates see only policy-allowed/callable entries: unchanged surface
  contract; no new producer of candidates.
- Required/never-reduce/immediate tools are not learned-promotion
  candidates: excluded pre-score, revalidated at promotion; immediate tools
  never enter the eligible set (not deferred).
- Cap applies after deferred eligibility: structural (universe built
  before `preselect_candidates`).
- Oversized eligible sets use deterministic BM25 over the full universe:
  tested, tie-stable, latency-measured.
- Preselection cannot promote or execute: pure ordering function returning
  descriptors; promotion still requires threshold + budgets + revalidation.
- Off/observe/rerank semantics unchanged: migrated test asserts Off and
  Observe return empty; Rerank short-circuits before candidacy as before.
- Failure/empty shortlist leaves palette unchanged: tested both paths.
- Determinism for same context/surface/config: repeated-run equality test.

## 6. Failure and recovery review

- Advisor scoring failure or panic-guard trip: palette unchanged
  (pre-existing catch_unwind path preserved and now covered by an injected
  failure test).
- Empty eligible universe or zero budget: empty promotion set, no advisor
  call on empty context, no crash on zero budget.
- BM25 returning no rows (e.g. empty context with non-empty universe):
  bounded stable fallback inside the eligible set; palette unchanged when
  the context itself is empty.
- Schema-budget overflow: promotion skipped per definition as before.

## 7. Migration and compatibility review

- `candidates_from_surface` removed (was `pub` but had exactly one
  in-tree caller; no `tests/` or `examples/` users). Replacement APIs make
  the deferred-first contract explicit in their names and docs.
- New additive APIs: `candidates_from_deferred_surface`,
  `preselect_candidates`, `PreselectionReport`, `preselection_recall`,
  `CandidateRecall`. No storage, protocol, configuration, or artifact
  changes.

## 8. Security review

- No authorization, permission, broker, or sandbox change. The eligible set
  is a subset of the previously scored set intersected with deferred
  eligibility minus required/never-reduce — strictly narrower authority
  exposure than before for oversized surfaces, identical for small ones.
- No secret handling, path validation, or network change. BM25 runs on the
  same in-memory descriptor text the advisor already received.

## 9. Documentation and operations

- `architecture/tool-advisor.md` pre-turn paragraph updated.
- Per-turn debug span (`preturn_disclosure`) now records eligible count,
  shortlisted count, truncation flag, and preselection millis for operator
  diagnosis.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No hosted CI run for this batch | Same standing limitation as C001/C002 | C004 requires hosted CI per its plan |

No critical/high/medium findings.

## 11. Roadmap disposition

C003 closed. C004's hard dependency on C003 is satisfied (deferred-first
shortlisting landed with recall instrumentation). C004 is now unblocked on
all three predecessors and may proceed to clean offline requalification.

## 12. Registry updates

- `plans/registry.md`: move C003 `ready` → `closed` (closure:
  `plans/closure/tool-selection-advisor-evidence-integrity-corrective/003-status.md`);
  move C004 `blocked` → `ready`.
- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md`:
  C003 `ready` → `closed`; C004 `blocked on C003` → `ready`.
- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/003-…md`:
  status `ready for handoff` → `implemented`.
- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/004-…md`:
  status `blocked` → `ready for handoff`.
