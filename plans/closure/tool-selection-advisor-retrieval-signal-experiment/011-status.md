# Tool-Selection Advisor Retrieval-Signal Experiment M011 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/011-train-partition-inferability-audit.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Repository baseline reviewed: `3fa18576`

Implementation commit:

- (this closure batch) — add fail-closed train-label audit harness and regressions

## 1. Executive finding

M011 reaches its preregistered stop condition before producing an optimizer-input receipt. The frozen M001 classifier reports a primary grade-3 training label as `other-evidence-defect`: `read` in `filesystem-semantic-014-variant-1`. Its allowed current-step query says “Open dep_lock … then list the sibling entries”; the candidate descriptor is “Read bounded file contents.” The target is inferable by ordinary generic paraphrase, but M001's frozen audit classifier has no open/read relation and therefore cannot approve this optimizer label. M003 remains blocked until an audit-only classifier coverage corrective resolves this taxonomy gap and reruns the complete train audit.

No corpus, labels, partitions, M003 grid, dev outcomes, or learned weights changed. The audit stopped before writing either the committed receipt or an accepted optimizer-input file.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Derive the frozen train partition and check corpus/dev fingerprints | `audited_optimizer_cases`; frozen dataset SHA-256 `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582`; dev SHA-256 `b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9` | Pass | Supplied optimizer indexes must exactly equal `partition_cases().train_cases`. |
| Apply M001 inferability and M006 current-step filtering to train labels | M011 focused audit test | Blocked | The first highest-grade pair is classified `other-evidence-defect` by the incomplete frozen paraphrase table. |
| Exclude implicit-secondary labels and freeze exact optimizer input | Receipt `assets/tool-advisor/retrieval-signal-m003-train-audit.json` | Not produced | Hard stop occurs before output, as required. |
| Keep dev/test outside optimizer inputs | `optimizer_view_rejects_non_train_indexes` | Pass | Explicit dev index injection is rejected. |
| Pin M006 decision and avoid corpus mutation | `m006_decision_fingerprint_is_frozen` | Pass | M006 decision fingerprint and `current-step-only` value are asserted. |

## 3. Production implementation evidence

M011 adds `src/tool_advisor/retrieval_signal_m003_audit.rs`, feature-gated with the encoder-training experiment code. It verifies the corpus and dev fingerprints, requires exact train indexes, classifies each train pair with M001's taxonomy and sibling adjudication, filters implicit-secondary pairs, and is designed to emit a compact receipt plus exact optimizer view only after all validation passes. The failed audit raises before writing either output. No runtime advisor path consumes this module.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m003_audit
git diff --check
```

### Results

- Focused feature-gated tests: passed, 3 passed / 0 failed. One test asserts that the full frozen training audit stops on the named unsupported-primary-label finding; the other tests reject dev indexes and pin M006's receipt hash.
- The audit generated no `assets/tool-advisor/retrieval-signal-m003-train-audit.json` and no accepted optimizer view.
- Full `scripts/verify.sh quick` and workspace Clippy were not rerun for this bounded blocked pass; M002 already passed both at its closure. They remain required after the corrective produces a positive train audit.
- `git diff --check`: pass.

## 5. Invariant review

- Corpus bytes, labels, split assignments, and M006 decision remain unchanged.
- No dev/test/v2/v3 example entered an optimizer view.
- No M003 model, candidate grid, retrieval gate, hard-negative set, or learned artifact was inspected or changed.
- The unsupported primary label failed closed before producing a receipt.

## 6. Failure and recovery review

The deterministic offline audit halts at the first `other-evidence-defect`; the partial classification is diagnostic only and cannot become optimizer input. The corrective must resolve the generic inferability taxonomy issue and rerun the full audit from frozen source assets. No recovery or restart state is persisted.

## 7. Migration and compatibility review

No data migration, protocol, runtime configuration, or durable advisor artifact changed.

## 8. Security review

The audit reads only repository-owned synthetic corpus data and bounded tool schema cues. It adds no network, subprocess, secret, or execution surface.

## 9. Documentation and operations

The registry and roadmap identify M012 as the owner of the generic audit-classifier coverage defect. The M011 code and tests remain as regression evidence for the fail-closed stop. M003 must not begin training until a later completion record supplies a positive optimizer-input receipt.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | M001's audit classifier lacks a generic `open` ↔ `read` paraphrase and rejects an inferable primary training label. | M003's M001-approved optimizer input cannot yet be frozen; training is blocked. | M012 adds a bounded, tool-agnostic audit-only paraphrase coverage corrective with regressions, then reruns M011's full train audit before any projection fitting. |

## 11. Roadmap disposition

M011 is blocked at its explicit stop condition. M003 remains blocked. The bounded classifier-coverage corrective M012 is dependency-ready and registered; if it closes positively, M003 returns to ready. No other plan is newly unblocked.

## 12. Registry updates and dependency audit

- M011: active → blocked (stop condition met; no optimizer receipt).
- M003 remains blocked pending a positive M011 train audit after M012 corrects the audit taxonomy coverage.
- M012: registered ready; it owns the generic audit-classifier coverage correction and full train-only re-audit.
- M004 and M005 remain blocked on positive retrieval/operating-point evidence.
- Registry scan found no independent ready implementation plans outside this workstream.
