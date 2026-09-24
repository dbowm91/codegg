# Tool-Selection Advisor Retrieval-Signal Experiment M001 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-signal-sufficiency-audit-and-preregistration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m001--signal-sufficiency-audit-and-preregistration`

Repository baseline reviewed: `f351d014`

Implementation commits:

- None. No production changes or preregistration artifact were made.

## 1. Executive finding

M001 is blocked by its §4 hard stop. A bounded corpus review found gate-critical
secondary labels for `table_filter` whose relevance is not supported by the visible
current-step request. The existing evaluation contract does not specify whether such
labels represent current-step tools, inferable future workflow tools, or graded workflow
recall. M002 cannot be unblocked without silently choosing an evaluation target.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce the four persistent miss families | Predecessor M003 closure records `glob`, `table_filter`, `write`, `lsp_rename` at ranks outside the candidate pool | partial | Upstream aggregate/per-tool ranks reproduced from committed closure evidence; M001 case-level attribution receipt was not generated. |
| Determine label inferability from allowed query text | `shell-semantic-186-variant-1` asks only to list running processes but includes `table_filter` grade 2; `filesystem-semantic-013-variant-1` asks to read a module/report exports and includes `table_filter` grade 2 | fail / hard stop | Neither visible task supports filtering tabular data. These are gate-critical because `table_filter` is one of the four persistent misses. |
| Freeze Retrieval Signal V2 representation and conditional learned grid | No receipt or implementation | not run | Correctly withheld while the evaluation target is disputed. |
| Preserve gates and avoid training | No gates changed; no model trained | pass | No experiment arm was run. |
| Schema plumbing audit | Not completed | not run | Downstream work is blocked before representation design. |

The examples establish a material evaluation defect possibility; they are not claimed to
be a complete adjudication of every relevant occurrence. The review did not inspect v3
for selection or tuning.

## 3. Production implementation evidence

No production code, model, or runtime path changed. The only planning change is a new
corrective plan, C001, which owns the evaluation-target decision and immutable relabeling
if required.

## 4. Verification executed

### Commands run

```text
Read-only inspection of the M001-M005 plans, the retrieval-signal roadmap and registry,
the retrieval-architecture M003 closure, and the named JSONL corpus rows.
```

### Results

The two named rows were confirmed directly in `assets/tool-advisor/corpus.jsonl`.
No tests or verification suites were run because no implementation was made. This is
not closure evidence for the positive M001 acceptance criteria.

## 5. Invariant review

- No training or scoring changes occurred.
- Historical corpus and predecessor artifacts remain untouched.
- Retrieval gates were not weakened.
- v3 was not used for classification, selection, or tuning.
- No runtime authority or candidate eligibility path changed.

## 6. Failure and recovery review

Not applicable to production behavior: no production code changed. Recovery requires
resolving the evaluation target and then proceeding on a newly fingerprinted evaluation
version where labels change; historical evidence must remain immutable.

## 7. Migration and compatibility review

No schema, model artifact, storage, or runtime compatibility changes were made. Any
corrected labels must be published as a new version with explicit provenance and split
fingerprints.

## 8. Security review

No authority, execution, secret-handling, network, or security surface changed.

## 9. Documentation and operations

- Registered `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/006-retrieval-evaluation-target-corrective.md` as the blocked owner of the unresolved target decision.
- Updated the roadmap and registry to keep all dependent milestones blocked.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Existing grade-2 `table_filter` labels in current-step requests about process listing and module reading lack supporting query evidence under the apparent contract | The four-tool retrieval frontier and any learned target may be invalid; M002-M005 cannot proceed safely | Decide the relevance target and adjudicate labels under C001; create a new immutable evaluation version if necessary |

## 11. Roadmap disposition

M001 is blocked, not positively closed. The next plan cannot be unblocked because the
hard-stop evaluation issue affects one of the four gate-critical miss families. C001
is registered but itself awaits an explicit relevance-target decision. Stop this
workstream here pending reassessment.

## 12. Registry updates

- M001 status changed from ready to blocked with the hard-stop evidence recorded here.
- M002-M005 remain blocked; the dependency graph has not been satisfied.
- C001 is registered as blocked pending a target decision; it owns any new immutable
  evaluation version and the subsequent dependency audit.
- No other registered downstream plan lists this M001 as a satisfied dependency; none
  is unblocked.
