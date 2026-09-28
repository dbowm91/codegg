# Tool-Selection Advisor Retrieval-Signal Experiment M002 — Blocked Closure

Status: blocked

Source implementation plan: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`

Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m002--deterministic-retrieval-signal-v2`

Repository baseline reviewed: `0c663820`

Implementation commits: None. M002 measurement and scoring implementation did not begin.

## 1. Executive finding

M002 is blocked at its explicit §10 stop condition: the M001 preregistration cannot reproduce the scoring contract M002 requires. M006 positively resolved the relevance target and made M002 eligible on a re-derived current-step-only denominator, but it did not supply missing M001 scoring parameters. No lexical or semantic variant was measured, no dev outcome was inspected, and no scoring values were invented.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Use M006 current-step-only decision | `assets/tool-advisor/retrieval-signal-m006-decision.json`; closure `006-status.md` | Satisfied; M002 eligibility confirmed |
| Reproduce fixed lexical weighting | M002 §3 requires fixed weights from M001; M001 plan §5 says freeze weighting strategy, while M001 receipt has only variant names and field order | Blocked: exact strategy/weights absent |
| Reproduce semantic encodings/pooling | M001 receipt lists broad variant and pooling names but no exact per-arm text assembly/scoring contract | Blocked: semantics underspecified |
| Preserve gates, labels, and authority | No scoring code, corpus, labels, thresholds, or runtime behavior changed | Preserved |
| Run the preregistered frontier | Stop condition prohibits choices when preregistration cannot be reproduced | Not run by design |

## 3. Production implementation evidence

None. This is an evidence/infrastructure experiment; no production advisor behavior was changed.

## 4. Verification executed

- Read and compared M001 §§5, 10, and 11; the M001 preregistration receipt; M002 §§2-5 and 10; and the M006 decision/closure.
- Confirmed the repository had no M002 implementation, receipt, or prior M002 closure at the reviewed baseline.
- No tests or retrieval measurements were run because the stop condition was reached before implementation.

## 5. Invariant review

Historical M001 and M006 evidence remains unchanged. Current-step-only labels and derived denominators remain authoritative. Frozen corpus and gates were not modified. No query context or user data was introduced into descriptor cache inputs.

## 6. Failure and recovery review

No runtime or persistent state was introduced. The recovery path is a preregistration-only corrective before any measurement.

## 7. Migration and compatibility review

No schema, protocol, configuration, artifact reader, or production runtime change. M001 and M006 receipts remain immutable.

## 8. Security review

No authorization or execution surface changed. No model download, network request, subprocess, or user-data capture was performed.

## 9. Documentation and operations

M007 is registered ready to make the M002 variant contract deterministic and independently reproducible before any sweep.

## 10. Unresolved findings

| Severity | Finding | Required action |
|---|---|---|
| medium | M001 omitted the exact lexical weighting and semantic encoding details assumed fixed by M002 | Close M007 with an outcome-independent preregistration receipt before resuming M002 |

## 11. Roadmap disposition

M002 is blocked pending M007. M003 remains conditional on a valid M002 negative result; M004 and M005 remain blocked. M001 remains historically blocked as recorded; M006 remains positively closed.

## 12. Registry updates

The registry records M002 as blocked on M007 and M007 as ready. Dependency audit: no plan is newly unblocked by this blocked closure. M003, M004, and M005 retain their existing conditions.
