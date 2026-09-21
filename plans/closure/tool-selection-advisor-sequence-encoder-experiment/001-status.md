# Tool-Selection Advisor Sequence-Encoder Experiment P001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/001-evidence-preregistration-polish.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#p001--evidence-state-and-preregistration-polish`

Repository baseline reviewed: `1cd3276`

Implementation commits:

- `1cd3276` — advisor: polish preregistration evidence contract

## 1. Executive finding

P001 is complete. Planning state now distinguishes the historical hashed
contextual scorer from the new sequence-encoder experiment, records later
hosted verification without rewriting C004 history, and enforces an optional
schema-v2 preregistration hash before evaluation begins.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Preserve historical C004 closure timing | `plans/closure/tool-selection-advisor-evidence-integrity-corrective/005-hosted-ci-supplemental-evidence.md` | pass | Hosted run is additive; C004 closure is unchanged. |
| Correct stale C004 planning status | `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md` | pass | C004 subsection now says closed, disposition B. |
| Record framework demotion | `architecture/tool-advisor-framework-spike.md` | pass | Historical selection and current qualification state are explicit. |
| Add preregistration provenance | `src/tool_advisor/requalify.rs` | pass | Protocol hash, model/config hashes, declared gates, dataset/splits, and commit provenance are represented and echoed. |
| Reject schema-v2 manifest drift | `protocol_hash_is_stable_and_excludes_operator_commit_provenance` | pass | Canonical hash changes when protocol content changes and ignores operator commit metadata. |
| Keep legacy C004 reproducible | `load_preregistration` schema default and existing manifest | pass | Schema v1 remains accepted without retroactive edits. |

## 3. Production implementation evidence

`Preregistration` now has a backward-compatible schema version and optional
schema-v2 provenance envelope. `protocol_hash_for` canonicalizes the manifest
with only the mutable hash and operator commit provenance removed. Schema-v2
loads fail on missing or mismatched hashes and on an empty declared gate set.
`RequalificationReport` echoes the provenance values so closure evidence can
tie a result to the exact frozen protocol.

The architecture and subsystem documents explicitly state that
`contextual-embedding-v2` is research-only after C004 disposition B and that
no learned architecture is currently qualified.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk proxy cargo test --locked --features tool-advisor-training -p codegg requalify::tests::protocol_hash_is_stable_and_excludes_operator_commit_provenance -- --nocapture
rtk scripts/verify.sh quick
rtk git diff --check
```

### Results

- Focused protocol test passed.
- `scripts/verify.sh quick` passed, including formatting, guards, and locked workspace check.
- `git diff --check` passed.
- The focused test command was interrupted after the matching unit test passed while Cargo continued launching unrelated zero-filter integration binaries; no failure was observed in the matching test.

## 5. Invariant review

- Advisor remains optional/default-off; no runtime authority or model-selection path changed.
- Historical C004 artifacts and closure records remain loadable/immutable.
- Protocol validation is offline and occurs before dataset evaluation.
- Provenance is descriptive and does not grant permission, execute tools, or widen the candidate universe.

## 6. Failure and recovery review

Malformed schema-v2 manifests fail closed on missing/incorrect protocol hash or
missing declared gates. Schema-v1 manifests retain the prior compatibility
path. No persistence, restart, scheduler, or concurrency behavior changed.

## 7. Migration and compatibility review

The new fields use serde defaults, so the historical C004 JSON remains valid.
No storage or wire migration is required. Future positive qualification must
freeze the manifest in a separate commit before final-test evaluation.

## 8. Security review

The change adds only hash verification and metadata echoing. It introduces no
network access, secret handling, execution authority, or authorization bypass.

## 9. Documentation and operations

Updated the framework spike, corrective roadmap status, sequence-experiment
registry state, and hosted-CI supplemental evidence. Operators can inspect the
protocol hash and provenance fields in machine-readable requalification output.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Exact hosted-run completion timestamp was not exposed by the local evidence source | No correctness impact; run/job/conclusion are exact | Retain the hosted provider record if a later closure audit needs the timestamp. |

## 11. Roadmap disposition

P001 is closed. M001 and M002 were already independently dependency-ready;
M003 remains correctly blocked on both. No new downstream plan became ready
from closing P001 alone.

## 12. Registry updates

- P001 moved from active to closed and was recorded under recently closed work.
- The sequence-encoder roadmap remains active with M001 and M002 ready.
- Existing live advisor M004 remains blocked; no positive qualification exists.
