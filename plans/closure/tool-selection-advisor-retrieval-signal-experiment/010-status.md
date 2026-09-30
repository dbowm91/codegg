# Tool-Selection Advisor Retrieval-Signal Experiment M010 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/010-current-step-label-projection-corrective.md`

Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m010--current-step-label-projection-corrective`

Repository baseline reviewed: `0ff67b44`

Implementation commit: `1636e5b` — validate current-step retrieval experiment inputs

Plan-boundary correction: `d85ca86` — keep M010 closure independent of M002 sweep

## 1. Executive finding

M010 closes. M006's three current-step exclusions are now applied consistently to both `relevance` and `preferred_order` in the derived M002 dev view. Projected cases are validated before they are returned. The complete 64/128/256 universe fingerprint is computed before encoder manifest loading, using deterministic serialization compatible with the existing case fingerprint for valid cases and the expanded-fixture validator for the 256-candidate case size.

No historical corpus, partition, M001/M002/M006 receipt, relevance target, gate, K value, retrieval arm, or production behavior changed. The previous full M002 attempt ran 47,110 seconds and failed at final receipt fingerprinting when the derived `filesystem-semantic-013-variant-1` case still had `table_filter` in `preferred_order`. It wrote no frontier receipt and yielded no retrieval outcomes. A bounded regression test also exposed the separate 256-candidate serialization limit before another full run; M010 now covers that preflight failure before model loading.

The M010 implementation plan was narrowed before closure so its gate is deterministic projection and fail-fast preflight evidence. The full MiniLM frontier remains M002's work after this closure unblocks M002.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Remove each M006 exclusion from both derived label fields | `current_step_dev_cases()` filters `relevance` and `preferred_order` for exactly the named `(case_id, tool)` rows | Complete |
| Reject incoherent projected cases before M002 work proceeds | `ToolAdvisorCase::validate()` runs on every projected dev case | Complete |
| Preserve frozen corpus and eligible denominator | `m002_current_step_projection_removes_excluded_labels_consistently_without_mutating_corpus`; 69 eligible rows; source dataset fingerprint unchanged | Complete |
| Preserve non-excluded grade and preferred-order data | Regression compares every projected case to its frozen source with only the three exact exclusions applied | Complete |
| Support deterministic fingerprints for all preregistered universes | Regression computes 64/128/256 expansion fingerprints twice; valid-size fingerprints match `dataset_fingerprint`; size 256 passes the dedicated expanded-fixture validator | Complete |
| Detect projection/fingerprint defects before model cost | `run_sweep()` computes the full universe fingerprint before reading/loading the encoder manifest | Complete |
| Preserve preregistration and historical receipts | Corpus, M001 preregistration, M002 preregistration, and M006 decision assets unchanged; hashes recorded below | Complete |

## 3. Production implementation evidence

Changes are confined to experiment-owned M002 code in `src/tool_advisor/retrieval_signal_m002.rs`:

- M006 exclusions are removed from both the relevance map and preferred order while preserving remaining order.
- Each projected case is validated before returning the derived view.
- Universe fingerprinting serializes cases in case-ID order. This preserves the existing canonical bytes for valid cases and allows the 256-candidate experiment universe, which intentionally exceeds the generic persisted-case validation limit.
- The full universe fingerprint is computed before loading MiniLM, so deterministic expansion or fingerprint failures cannot waste another multi-hour run.

## 4. Verification executed

- `rustup run 1.98.1 cargo test --locked -p codegg --features tool-advisor-encoder-training --lib tool_advisor::retrieval_signal_m002::tests` — passed: 6 passed, 0 failed, 1 ignored (the full sweep).
- `scripts/verify.sh quick` — passed, including formatting, schema and architecture guards, and workspace check.
- `cargo fmt --all` — passed.
- `git diff --check` — passed.
- The feature-gated local test emitted the existing linker warning that the `__eh_frame` section exceeds compact-unwind encoding size; all focused tests passed.
- Full ignored M002 sweep — not rerun under M010. The failed pre-corrective attempt took 47,110 seconds, failed before writing output, and is not a valid measurement. M002 owns the retry after it returns to ready status.

Frozen artifact hashes observed during closure review:

| Artifact | SHA-256 |
|---|---|
| `assets/tool-advisor/corpus.jsonl` | `a4540cb95194d8628058d897e1f093f8d497a81d538b8d12b954034e36de6010` |
| `assets/tool-advisor/retrieval-signal-m001-preregistration.json` | `bb2a3cf9e635b01a5f181029ebcdc22194cad0070342c7638d3e1fc4ec74cc17` |
| `assets/tool-advisor/retrieval-signal-m002-preregistration.json` | `2ab3d0e38de58e16f43536acd923ec430e104b858939031625324601677fd8bb` |
| `assets/tool-advisor/retrieval-signal-m006-decision.json` | `24e3783f1cc1c3d8934bac1592607702966112554db8ca1215087f90cfd09756` |

## 5. Invariant and compatibility review

The source corpus and its historical supervision are unchanged. The derived M002 view now consistently applies M006's existing `current-step-only` decision; it does not rewrite the target or change its 69-label denominator. Scoring, candidate authority, runtime catalog behavior, and advisor production behavior are unchanged.

## 6. Failure and recovery review

The failed full run produced no frontier artifact or partial metric accepted as evidence. M010 adds a fail-fast projected-case validation and moves complete universe fingerprint computation ahead of encoder loading. The full offline sweep remains single-process and must not be duplicated. A future failure before a complete receipt is a stop condition for M002, not a partial result.

## 7. Migration and compatibility review

No persisted data, schema, or protocol migration. For valid cases, the new fingerprint helper uses the same compact serialized bytes and case-ID order as the existing dataset fingerprint. The 256-candidate expanded cases use the already-defined expanded-fixture size and authority validation.

## 8. Security review

No production authority or execution surface changed. Retrieval scoring still ranks only the expanded deferred candidate set. The input checks and fingerprints operate on local, frozen experiment assets.

## 9. Documentation and operations

M010 plan, roadmap, and registry now describe the corrective and its closure boundary. M002 remains ready to run the original frozen sweep. The prior failed attempt remains historical failure evidence only; no frontier receipt was created.

## 10. Unresolved findings

None in M010 scope. M002 retrieval sufficiency remains unmeasured.

## 11. Roadmap disposition

M010 is closed. M002 is returned to ready status with its M006-M009 dependencies intact and may now run the full preregistered frontier. M003 remains conditional on a valid negative M002 result. M004 remains blocked pending a positive M002 or M003 result; M005 remains blocked pending positive M004.

## 12. Registry updates and dependency audit

- M010: closing → closed.
- M002: blocked on M010 → ready; M006-M009 remain closed and the input contract is now validated before model load.
- M003: remains blocked/conditional because no valid negative M002 result exists.
- M004: remains blocked because there is no positive M002 or M003 result.
- M005: remains blocked because M004 has no positive operating point.
- No other registered plan has M010 as a hard or interface dependency; no unrelated plan is newly unblocked. Other ready workstreams remain eligible for the broader registry scan after M002's measurement.
