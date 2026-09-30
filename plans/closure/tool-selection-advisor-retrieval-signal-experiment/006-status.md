# Tool-Selection Advisor Retrieval-Signal Experiment M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/006-retrieval-evaluation-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m006--retrieval-evaluation-corrective`

Repository baseline reviewed: `7259292ef371ecd116e8911f9c2ddca7032e2a2e`

Implementation commits or pull requests:

- (this closure batch) — `RelevanceTarget` decision + `gate_critical_set` re-derivation in `src/tool_advisor/retrieval_signal.rs`, 5 new regression tests, receipt `assets/tool-advisor/retrieval-signal-m006-decision.json` with `m002_unblocked=true`

## 1. Executive finding

M006 closes **positively**: the relevance target is decided as
**current-step-only** with a recorded product rationale, the gate-critical
set is re-derived mechanically over the frozen M001 rows, and the re-derived
set is fully inferable — so **M002 is unblocked** to run the deterministic
Signal V2 sweep on the re-derived denominators.

No gate was lowered, no frozen label was rewritten, no projection was
trained, and v3 never entered the decision. The three implicit-secondary
rows leave the set with per-row citations; the surviving gate-critical miss
(`glob`, query-paraphrase) is inferable and defines the precise Signal V2
gap M002 is designed to test. `AllWorkflow` and `GradedRecall` were
evaluated and correctly keep M002 blocked; the decision records why neither
was selected.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Relevance-target decision among all-workflow / current-step-only / graded recall with rationale (plan §5) | `RelevanceTarget::{AllWorkflow, CurrentStepOnly, GradedRecall}` + `decide_relevance_target` requiring non-empty rationale; receipt `decision.target=current-step-only` with full rationale (sibling-explicit proof, hidden-state argument, graded-weight rejection) | pass | Product judgment recorded, not inferred from recall |
| Mechanical re-derivation over frozen M001 rows, no relabeling (plan §5) | `gate_critical_set` pure filter over `run_miss_audit().occurrences`; no corpus write; every exclusion cites occurrence + rule | pass | Frozen rows untouched |
| Re-computed denominators + M001 classifications under the decision (plan §5) | `rederived_eligible_counts` (72→69 per universe, 3 distinct exclusions); `required_hits` 69/68/66 for 0.99/0.98/0.95; `m002_unblocked` re-check over included rows | pass | Gates unchanged, counts recomputed |
| Verdict: M002 unblocked iff fully inferable, else negative close (plan §5) | Receipt `m002_unblocked=true`, `verdict` authorizing the Signal V2 sweep; `AllWorkflow`/`GradedRecall` both verdict blocked (tested) | pass (positive) | M002 unblocked; no negative close |
| No retriever/scorer/projection/threshold/K change; no frozen-corpus edits; no v4 (plan §5 out-of-scope) | Code inspection: additive enum/filter/receipt only; no catalog/ranker/promotion/protocol change | pass | M002/M003 own all scoring work |
| Regression tests (plan §10) | 5 new tests in `retrieval_signal::tests` (variants/round-trip, exact-3 exclusion, denominators/hits, v3 absence, unblock matrix) | pass | 16/16 in module, 60/60 in `tool_advisor` base suite |
| No frozen corpus byte change; v3 absent from inputs (plan §10) | `dataset_fingerprint`/`EXPECTED_DEV_PARTITION_FINGERPRINT` re-checked in receipt builder + test; `prereg_fingerprint`-style v3 rejection mirrored in receipt-content test | pass | Corpus `06da7e53…`/dev `b804b7d8…` unchanged |
| Decision log + updated denominators in M006 receipt (plan §12) | `assets/tool-advisor/retrieval-signal-m006-decision.json` (4.6 KiB, no embeddings) | pass | Content-identical to generator output |

## 3. Production implementation evidence

- `src/tool_advisor/retrieval_signal.rs` (+~230 lines incl. tests, additive):
  `RelevanceTarget` (exhaustive 3-variant enum, `as_str`,
  `exclusion_rule`), `RelevanceDecision` + `decide_relevance_target`
  (non-empty rationale enforced), `GateCriticalExclusion`,
  `gate_critical_set` (pure filter), `occurrence_weight` (graded 0.5 vs 1.0
  explicit), `rederived_eligible_counts` (distinct-pair subtraction),
  `required_hits` (ceil), `m002_unblocked` (zero-defect rule),
  `M006DecisionReceipt` + `m006_decision_receipt` (frozen-input builder with
  dev-tripwire, gate table, verdict), constants `M006_PROTOCOL`,
  `M006_SCHEMA_VERSION`. No changes to runtime advisor behavior, catalog
  scoring, durable artifacts, thresholds, or promotion logic. No production
  behavior change by design (evidence/infrastructure class).
- Receipt `assets/tool-advisor/retrieval-signal-m006-decision.json`:
  `current-step-only`, 1 included (`glob` query-paraphrase), 3 excluded
  (each with rule citation), rederived `{64:69,128:69,256:69}`,
  `required_hits_{64:69,128:68,256:66}`, `m002_unblocked=true`.
- No catalog, ranker, promotion, protocol, storage, or config change.

## 4. Verification executed

### Commands run

```bash
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal
cargo test --locked -p codegg --lib -- tool_advisor
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

The plan-literal feature-gated suite
(`--features tool-advisor-encoder-training`) was not run: the M006 module
is ungated and dependency-free by design (same justification as M001), and
`candle-core v0.11.0` does not compile on aarch64-darwin/rustc-1.89
(pre-existing NEON interaction). Linux CI remains the qualified host for
the future M002 sweep.

### Results

- `tool_advisor::retrieval_signal`: 16 passed / 0 failed (11 pre-existing +
  5 new M006 tests).
- `tool_advisor` (base features): 60 passed / 0 failed.
- `check_execution_ownership.py`: guard ok (no subprocesses, no Git
  invocation).
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, eggwork routing, workspace check).
- `cargo fmt --all -- --check` and `git diff --check`: clean.
- Receipt generation: `cargo run --locked --bin m006_gen` (temporary
  generator, since removed) emitted valid JSON; committed receipt is
  content-identical to generator output; fingerprints match frozen M001
  references (`06da7e53…`/`b804b7d8…`).

## 5. Invariant review

- Historical train/dev/test/v2/v3 immutable: no corpus file modified;
  dev fingerprint re-checked; v3 never loaded (asserted by receipt-content
  test).
- Relevance decision recorded explicitly; gates re-derived, never lowered:
  `GATE_RECALL_*` constants untouched; required hits recomputed via ceil.
- No projection trained to memorize hidden labels before the decision: no
  encoder use, no optimizer, no threshold edits.
- Advisor locality/authority (ADR-0009) unchanged: no runtime path modified;
  `ResolvedToolSurface` untouched; span-packed ranker frozen.
- Descriptor caches hold static metadata only: untouched.
- No remote model/telemetry/weight download: none.

## 6. Failure and recovery review

Pure offline computation over frozen inputs; no daemon, persistence, or
concurrency surface. Corpus/partition drift fails closed on the existing
tripwires (re-checked in the receipt builder). Empty rationale fails closed
in the constructor (tested). Missing fixture candidates error instead of
defaulting (inherited from `run_miss_audit`). A decision made without
rationale is invalid output and cannot be constructed.

## 7. Migration and compatibility review

Additive only: new enum/structs/functions (all `pub` for M002 reuse), 5 new
tests, one new JSON receipt under `assets/tool-advisor/`. No existing
command, artifact format reader, or report field changed meaning. The M001
receipt format is untouched (M006 receipt is a separate file; readers must
tolerate absence per plan §9 — satisfied by keeping files distinct). No
config change. Rollback is a revert of this batch with no data implications.

## 8. Security review

No authorization surface touched. No secrets logged; receipt contains only
static tool metadata, benchmark contexts (already public in M001 receipt),
and the decision text. Execution-ownership guard passes. Query redaction and
schema value-exclusion properties inherited from M001 remain tested and
green.

## 9. Documentation and operations

- Receipt: `assets/tool-advisor/retrieval-signal-m006-decision.json`.
- Closure: this record.
- Registry: M006 `active` → `closed`; M002 `blocked` → `ready` (see §12).
- Roadmap: M006 `ready` → `closed (positive)`; M002 `blocked` → `ready`
  with the blocker cleared (re-derived set fully inferable).
- Plan file: `006-...md` status `active` → `implemented`.
- No operator diagnostics or static guards beyond the in-code tripwires
  (dev fingerprint, rationale enforcement, v3 absence).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `tool-advisor-encoder-training` profile does not compile on aarch64-darwin/rustc-1.89 (candle-core NEON f16) | Plan-literal feature-gated suite unrunnable on this host class; Linux CI unaffected | Future M002 implementer must run the feature-gated sweep on Linux CI or a compatible host before closing |
| low | Re-derived u64 gate needs 69/69 (0.99 on 69 eligible) while best fused recovers 68/69 on the re-derived set | M002 must still prove Signal V2 clears all three re-derived gates; unblocking is not passing | M002 sweep must report the re-derived frontier honestly; no gate relaxation permitted |
| low | Graded-recall weights (0.5) are explicit here but were not preregistered in M001 | GradedRecall cannot unblock M002 without a new preregistration | Documented to prevent re-litigation; CurrentStepOnly is the decided target |

No critical/high findings. No findings indicate a defect in the shipped code.

## 11. Roadmap disposition

- M006: **closed (positive)** — decision recorded, set re-derived,
  M002 unblocked.
- M002: **ready** — hard dependency (positive M001 re-audit via M006) now
  satisfied; frozen Signal V2 contract + conditional M003 grid from M001
  stand for reuse; deterministic sweep authorized on re-derived
  denominators (69 per universe; required 69/68/66).
- M003: remains **blocked/conditional** — requires M002 outcome (valid
  below-gates or positive); unchanged.
- M004/M005: remain **blocked** — require positive M002/M003/M004 chain;
  unchanged.
- Order-invariance M005 and post-closure M004 stay blocked historical
  evidence, untouched by this workstream.

## 12. Registry updates

- `plans/registry.md` dependency-ready table: M006 row `active` → `closed`
  with closure link; new/updated M002 row `blocked` → `ready` with the
  M006 receipt as the unblock evidence.
- `plans/registry.md` active-roadmaps row for the retrieval-signal
  experiment: current milestone M006 ready → M006 closed (positive), M002
  ready, M003-M005 blocked.
- `plans/registry.md` retrieval-signal gate paragraph: M001-blocked text →
  M006-decided text with M002-ready pointer.
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  M006 `ready` → `closed (positive)`; M002 `blocked` → `ready`; dependency
  graph annotated (006 → unblocks M002).
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/006-*.md`:
  status `active` → `implemented`.
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-*.md`:
  status `blocked` → `ready` (re-derived denominators apply).
- Unblock audit: M002 is the only plan whose blocker is cleared by this
  closure; no other registered plan lists M006 as a dependency. Order-
  invariance M005, post-closure M004, and Eggwork M003 remain blocked as
  recorded in their own closures this batch.
