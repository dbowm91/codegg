# Tool-Selection Advisor Retrieval-Evaluation Semantics Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-inferable-relevance-target-and-rebaseline.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-evaluation-semantics-corrective-addendum.md`

Repository baseline reviewed: `8613575d`

Implementation commits:

- `8613575d` — tool-advisor C001: inferable relevance target and derived view

Disposition: **B — real inferable retrieval gap (unblock M001R only).**

## 1. Executive finding

C001 defines an explicit, deterministic, auditable retrieval target over the
frozen corpus, remeasures unchanged retrieval against it, and proves a genuine
inferable gap remains. All 72 dev positives are adjudicated (53 current-step,
0 explicit-next-step, 19 implicit-future, 0 evidence-defect). Unchanged BM25
reproduces the M004 52/72 broad tripwire exactly and scores 46/53 inferable
(0.8679) flat across 64/128/256 × K16/24/32 with zero authority violations,
failing all gates (0.99/0.98/0.95). The prior normalized-union broad ceiling
68/72 misses `glob` (current-step, dual-signal-miss per R001 §5); corrected
recall is therefore capped at 52/53 = 0.981 and cannot clear the 64 ≥ 0.99
gate. No ranker-label blocker exists: every grade-3 and preferred-first label
in train/dev/test is current-step. M001R is unblocked to ready; M002-M005,
live M004, order-invariance M005, and v4 remain blocked; no ranker corrective
is registered.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned retrieval-evaluation contract (Current/Explicit/Implicit/Defect) | `src/tool_advisor/retrieval_relevance.rs`: `RetrievalRelevanceClass`, `parse_class`, eligibility | pass | One authoritative serializer/validator; `ExplicitNextStep` requires `next_steps` |
| Complete dev adjudication (every positive, stable order, 2 static checks) | `build_derived_view`, `validate_derived_view`; 72/72 dev covered once, sorted by case+ candidate | pass | `ImplicitFuture` with non-empty support fails; `CurrentStep` without evidence fails |
| Derived view asset + manifest/fingerprint, frozen-corpus-preserving | `assets/tool-advisor/retrieval-relevance-v1.json`: 280 entries, fp `23edef17…`, corpus `06da7e53…`, dev `b804b7d8…` | pass | No corpus JSONL change; `committed_asset_matches_classifier` |
| Corpus-wide consumer-impact audit + frozen ranker diagnostic | `impact_by_split`; grade-3/preferred-first audit; per-family table §4 | pass | Diagnostic only, no retrain; no material ranker delta (all top labels current) |
| Unchanged retrieval rebaseline (64/128/256 × 16/24/32, BM25 + semantic + fusion) | `expand_universe_local` + `bm25_ordering_local` (parity with M004 paths); BM25 52/72 reproduced; union ceiling lemma §4 | pass | Semantic live re-run absent here (assets/toolchain, see §4/§10); disposition proven via BM25 + prior dual-signal evidence |
| Stop conditions (defect, nondeterminism, untraceable, retrieval change, violations) | 0 defects, deterministic fp, corpus-bound validation, BM25 tripwire held, 0 violations | pass | No stop triggered |
| Regression tests (§10) | 19 `retrieval_relevance` tests + 63 `tool_advisor` tests | pass | See §4 |
| Disposition A/B/C/E | B (see §11) | pass | M001R unblocked only |

## 3. Production implementation evidence

- `src/tool_advisor/retrieval_relevance.rs` (new, ~1550 lines): contract,
  cue lexicon with stale-suffix stripping (`effective_context`), classifier,
  derived-view build/fingerprint/validation, `impact_by_split`, local
  expansion/BM25 ordering in parity with `operating_point::expand_universe`
  and `retrieval_architecture::bm25_ordering`, 19 regression tests plus
  ignored asset/printing harnesses.
- `src/tool_advisor/mod.rs`: `pub mod retrieval_relevance;` only.
- `assets/tool-advisor/retrieval-relevance-v1.json`: 280 entries
  (219 current-step eligible, 61 implicit-future excluded, 0 explicit, 0
  defect), fingerprint `23edef17d0e4f4ac71d4e4b65349585cfc9b3c08d49cd4f7ac47e3bad12b5479`,
  corpus `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582`,
  dev `b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9`.
- Class counts by split: train 113/0/25/0 (cases 132, 25 with implicit);
  dev 53/0/19/0 (cases 62, 19 with implicit); test 53/0/17/0 (cases 62, 17
  with implicit). Grade-3 by split all current (66/32/36, 0 implicit).
  Preferred-first all current (train 102, dev 52, test 53).
- Per-family (tool_family, current/implicit): filesystem 11/4 + 7/2 + 2/1;
  search 10/4 + 7/2 + 6/2; git 10/1 + 10/3 + 3/0; lsp 11/4 + 5/2 + 8/1;
  verification 17/1 + 3/3 + 5/2; research 11/1 + 8/3 + 3/2; context 9/1 +
  6/1 + 6/2; plugin 14/2 + 1/1 + 8/3; shell 11/4 + 2/0 + 6/2; structured
  9/3 + 4/2 + 6/2 (train/dev/test order per family).
- Ranker impact: every grade-3 and preferred-first label is current-step, so
  the M003 span-packed selection (dev MRR 0.6255, consistency 0.951) and its
  train/dev supervision are not materially affected. No ranker-label-semantics
  corrective is registered (not disposition C).
- No historical corpus file changed (`git diff` shows only `mod.rs` + new
  files); no BM25/semantic/fusion/K/pooling change; no aliases/descriptors;
  no projection training; no ranker retrain; no v4; no gate or authority
  change.

## 4. Verification executed

### Commands run (local)

```bash
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_relevance
cargo test --locked -p codegg --lib -- tool_advisor
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_relevance::tests::print_bm25_frontier --ignored --nocapture
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
cargo clippy -p codegg --lib
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_relevance::tests::print_bm25_frontier --ignored --nocapture
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Results

- `retrieval_relevance`: 19 passed, 0 failed (2 ignored: asset generator +
  frontier printer). Covers exactly-once dev coverage, support invariants,
  deterministic fp, corpus-mismatch failure, corpus immutability,
  broad/inferable distinguishability, implicit exclusion/current inclusion,
  explicit field binding, zero defect, impact counts, grade-3/preferred
  preservation, per-family, BM25 identity, zero violations, expanded BM25
  frontier (broad 52/72 tripwire + inferable 53 denominator).
- `tool_advisor` (default features): 63 passed, 0 failed.
- BM25 frontier printer (ignored, 7.0 s): broad 52/72 = 0.7222 flat and
  inferable 46/53 = 0.8679 flat at every universe/K, 0 violations.
  Inferable misses (all current-step): `git_blame` 2
  (`git-semantic-055-variant-1`, `git-semantic-061-variant-1`), `git_log` 3
  (`git-semantic-047-variant-1`, `050`, `051`), `glob` 1
  (`filesystem-semantic-003-variant-1`), `read` 1
  (`filesystem-semantic-001-variant-1`).
- Old vs corrected frontier (BM25 measured now; union broad from
  M004/R001 closures):

| universe | K | broad (measured now / M004) | inferable (measured now) | gate | verdict |
|---|---|---|---|---|---|
| 64 | 16/24/32 | 52/72 = 0.7222 | 46/53 = 0.8679 | ≥ 0.99 | fail |
| 128 | 16/24/32 | 52/72 = 0.7222 | 46/53 = 0.8679 | ≥ 0.98 | fail |
| 256 | 16/24/32 | 52/72 = 0.7222 | 46/53 = 0.8679 | ≥ 0.95 | fail |

  Prior broad best (unchanged retrieval): normalized-union 68/72 = 0.9444
  flat (M004 §4, R001 §4); BM25-only 52/72; semantic-only ≤ 65/72. R001 §5
  attributes the four union misses as dual-signal-miss with BM25 rank
  last and semantic last/near-last: `glob`, `table_filter`, `write`,
  `lsp_rename`. Our adjudication makes `glob`
  (`filesystem-semantic-003-variant-1`, "Locate every test fixture…")
  current-step. Corrected union recall is therefore capped at 52/53 =
  0.981 < 0.99 even if the other three misses are implicit, so the 64 gate
  still fails with an inferable tool outside K ≤ 32. Semantic/fusion live
  re-run was not performed in this environment (reference MiniLM assets
  absent under `target/tool-advisor/`, and `tool-advisor-encoder-training`
  does not compile here: `candle-core 0.11` `stdarch_neon_f16` nightly
  errors, unrelated toolchain state); the verdict uses the measured BM25
  frontier plus the cited prior dual-signal evidence, which is sufficient
  for disposition B. M001R/M002 must remeasure semantic/fusion with encoder
  assets before operating-point selection.
- `check_execution_ownership.py`: ok. `verify.sh quick`: passed (fmt,
  agents, core-boundary, sandbox, execution-ownership, tui-authority,
  http-route, audit, scheduler-bypass, workspace check).
- `cargo fmt --check`, `git diff --check`: clean.
- Focused `cargo clippy -p codegg --lib`: no `retrieval_relevance` warnings
  (one unused-import fixed). Full
  `cargo clippy --workspace --all-targets --all-features -- -D warnings` is
  blocked by unrelated state: `egglsp` `nonminimal_bool`/`ptr_arg` errors
  plus the `candle-core` nightly `stdarch_neon_f16` errors above; recorded
  here with canonical hosted CI required before M001R close. Focused
  `tool_advisor` tests plus `verify.sh quick` are the local truth for this
  closure.
- `cargo test --locked --features tool-advisor-encoder-training …` is
  blocked by the same unrelated `candle-core` toolchain failure; the C001
  BM25 frontier intentionally runs in the default profile without the
  encoder feature.

## 5. Invariant review

- Frozen corpus unchanged (full fp `06da7e53…`, dev `b804b7d8…` tripwires
  held; `historical_corpus_files_are_unchanged`).
- No v3/v4 tuning or reads for selection; v3 never selects.
- No retrieval/model algorithm change: BM25 broad 52/72 reproduced exactly;
  expansion/BM25 paths are parity copies, originals untouched.
- No gate relaxation (0.99/0.98/0.95 at K ≤ 32 enforced literally).
- No tool authority widening: deferred-only at every entry, 0 violations
  across 9 points.
- No implicit label promoted to current to improve recall: cues are generic
  task entailments with stale-suffix exclusion; all singles current, all
  two-step explicit cases both current, all stale-only secondaries implicit.
- No user/private runtime context in derived assets (corpus text only,
  bounded excerpts ≤ 160 chars, supporting text ≤ 512).
- Default-off/local-only posture unchanged; no telemetry, download, network,
  or runtime-path change. `#![deny(unsafe_code)]` holds via lib root.

## 6. Failure and recovery review

- Missing dev positive, duplicate, unknown case/candidate, grade mismatch,
  empty rationale, oversized supporting text, eligibility inversion,
  implicit claiming support, explicit without `next_steps`, unordered
  entries, fp mismatch, corpus/dev drift: all fail closed in
  `validate_derived_view` (unit-tested via mismatch tests).
- Expansion losing a relevant tool or breaking universe size fails closed in
  `expand_universe_local`.
- Authority outsiders fail closed (counted as violations; C001 requires
  zero).
- Checkpoint/resume not needed: BM25 frontier is ~7 s deterministic; no
  partial state.

## 7. Migration and compatibility review

Additive module + asset only; no storage/protocol/config migration, no
artifact format change, no rollback concern. Derived-view schema v1 is
forward-only; any future adjudication bump invalidates M001R binding by
fingerprint.

## 8. Security review

No authorization, secret, network, or privilege surface touched. Cache keys
hold descriptor hashes only (existing invariant intact). Derived asset holds
dev-fixture tool names/ranks/rationales only, no user context. No new tool
or promotion path.

## 9. Documentation and operations

- Implementation plan `001-…md` moves to implemented (disposition B).
- Derived asset at `assets/tool-advisor/retrieval-relevance-v1.json`
  (fingerprint-gated; regenerate via ignored
  `generate_derived_asset` test only).
- Frontier printer is the ignored `print_bm25_frontier` test (evidence
  above).
- Roadmap addendum C001 ready → closed; retrieval-signal roadmap M001R
  blocked → ready (see §11).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Semantic/fusion live remeasurement not run here (MiniLM assets absent; encoder-training feature blocked by unrelated `candle-core` nightly toolchain) | Disposition B proven via BM25 + prior dual-signal ceiling lemma, but M004 operating-point selection still needs a live semantic/fusion remeasure | M001R/M002 must remeasure semantic/fusion with encoder assets and record the corrected frontier before any operating-point freeze |
| low | Full all-feature Clippy + encoder-training tests blocked by unrelated `egglsp` lints and `candle-core` nightly errors | No C001 code affected (focused clippy clean, default tests green) | Canonical hosted CI required before M001R close; do not treat local all-feature red as C001 evidence |

No high/medium finding. No stop-condition violation.

## 11. Roadmap disposition

Disposition **B — real inferable retrieval gap**: at least one gate still
fails because current-step tools (`glob` plus six others in BM25; `glob`
provably dual-signal-missing for union) remain outside K ≤ 32.

- Unblock M001R only:
  `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`
  blocked/conditional → ready.
- M001 remains blocked/closed (must not be reopened).
- M002-M005 remain blocked (M002 on positive M001R; M003 conditional on
  M002; M004 on positive M002/M003; M005 on positive M004).
- No ranker-label-semantics corrective (not disposition C: all grade-3 and
  preferred-first labels are inferable).
- No M004 unblock or fresh-v4 plan (not disposition A).
- No evidence failure (not disposition E).
- Order-invariance M005 stays blocked as historical evidence; this
  workstream does not unblock it. Retrieval-architecture workstream stays
  closed.

## 12. Registry updates

- `plans/registry.md`: evaluation-semantics subsystem row active → closed
  (C001 closed, disposition B); C001 plan row ready → closed
  (implementation `8613575d`, this closure); M001R row blocked/conditional
  → ready (hard dependency C001/B satisfied, no ranker blocker); M002-M005
  rows unchanged blocked; retrieval-signal gate paragraph updated to name
  C001 closure and M001R ready.
- `plans/subsystems/tool-selection-advisor-retrieval-evaluation-semantics-corrective-addendum.md`:
  active → closed (C001 closed, disposition B).
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  M001R blocked → ready (disposition B, no ranker blocker); M002-M005 stay
  blocked; workstream stays active (blocked on M001R now ready).
- `plans/implementation/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-inferable-relevance-target-and-rebaseline.md`:
  ready → implemented (disposition B, see this closure).
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`:
  unchanged file, now ready via registry (no content edit needed beyond
  status line if present; status remains blocked-on-C001 text until M001R
  activates it).
