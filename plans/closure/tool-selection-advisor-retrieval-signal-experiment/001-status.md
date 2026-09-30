# Tool-Selection Advisor Retrieval-Signal Experiment M001 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-signal-sufficiency-audit-and-preregistration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m001--signal-sufficiency-audit-and-preregistration`

Planning reconciliation baseline:

- `ce088e9153b821d8473372c7c04786a4d90ab6ae`

Implementation commits or pull requests:

- none accepted for M001;
- the attempted handoff reached the plan's hard-stop condition before a Signal V2 preregistration/implementation artifact was committed.

Recommendation: **blocked by evaluation-target semantics; corrective pass required.**

## 1. Executive finding

M001 did not fail because the retrieval implementation was technically impossible. It reached the exact evidence hard stop defined by the plan.

The frozen corpus mixes at least two distinct meanings of "relevant":

1. tools inferable from the bounded current-state request;
2. plausible downstream/supporting workflow tools that are not inferable from that request.

Examples already present in the frozen repository corpus include:

- "Summarize the quoted specification ..." with `summarize` and `lsp_rename` both labeled relevant;
- "Enable the installed extension ..." with `plugin_enable` plus `write` or `table_filter`;
- "Measure coverage ..." with `coverage` plus `write`;
- "Read the module ... and report its public exports" with `read` plus `table_filter`.

Those secondary labels may be useful workflow hints, but they are not all justified by the model-visible current task. A bounded retriever cannot be required to recover hidden future workflow steps without learning dataset conventions rather than task↔tool semantics.

This is therefore an **evaluation-target defect**, not evidence that Retrieval Signal V2 or a learned projection should proceed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Audit persistent dual-signal misses for inferability | Frozen corpus examples plus predecessor miss attribution for `glob`, `table_filter`, `write`, `lsp_rename` | hard stop | At least some labeled relevance is not inferable from allowed current-state text. |
| Distinguish signal gap from evaluation defect | Direct cases such as `glob` "Locate every test fixture..." coexist with implicit-secondary cases such as `summarize` + `lsp_rename` | pass | The two failure classes are separable and must not be scored identically. |
| Freeze Retrieval Signal V2 representation | not reached | blocked | Plan explicitly forbids preregistration after an evaluation-target hard stop. |
| Freeze learned-projection grid | not reached | blocked | Must wait until retrieval relevance semantics are corrected. |
| Preserve frozen historical corpus | current repository evidence | pass | No corpus rewrite is authorized. |
| Preserve gates rather than lowering them | current plan | pass | No recall gate is relaxed here. |

## 3. Why M001 stops here

The implementation plan states:

> If any gate-critical relevant label is classified `implicit-secondary` or `other-evidence-defect` and materially affects the frontier, close M001 blocked and register a separate retrieval-evaluation corrective.

That branch is now controlling.

Proceeding directly to M002 or M003 would violate the plan because the model would be trained/evaluated against a target that includes non-inferable workflow continuation labels.

## 4. Historical evidence handling

Do not rewrite:

- the 256-case historical corpus;
- order-invariance M001-M004 closures;
- retrieval-architecture M001-M003 closures;
- v2/v3 qualification evidence.

Those records remain valid for the exact targets they measured.

The corrected retrieval evaluation must be a **derived, versioned view** over frozen historical labels, not a mutation of historical examples.

## 5. Downstream impact

M002-M005 remain blocked.

The selected span-packed ranker is not invalidated by this planning reconciliation, but its training/evaluation labels share the same historical corpus. The corrective must therefore quantify how much of the ranker's train/dev/test supervision is classified as current-step, explicit-next-step, or implicit-future before a final end-to-end qualification is authorized.

Do not retrain the ranker in this closure.

## 6. Corrective dependency

Registered corrective:

- `plans/subsystems/tool-selection-advisor-retrieval-evaluation-semantics-corrective-addendum.md`
- `plans/implementation/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-inferable-relevance-target-and-rebaseline.md`

The retrieval-signal workstream may resume only through the separately registered post-corrective M001R plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`

M001 itself must not be reopened.

## 7. Security / authority / compatibility

No runtime, authority, network, model, storage, or protocol change is made by this closure.

The advisor remains optional/default-off and `ResolvedToolSurface` remains the authority boundary.

## 8. Final disposition

M001 is **blocked by evidence**. The hard stop is the correct result.

The next ready handoff is the retrieval-evaluation semantics corrective C001. Nothing downstream is unblocked.
