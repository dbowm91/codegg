# Eggsearch Evidence Workflows — M001 Post-Closure Retrieval Contract Corrective

Status: active (C001 closing)
Primary class: correctness / compatibility
Repository baseline: `agent/eggsearch-evidence-workflow-20261013`; implementation `6be801e3a02349630a153f4141e9a69dce789584`; accepted historical closure `plans/closure/eggsearch-evidence-workflows/001-status.md`.

## 1. Authority and purpose

This is a **post-closure corrective** under `plans/003-planning-process.md#7-corrective-passes`. It does not silently rewrite M001's accepted historical closure. The strict downstream M001 retrieval-state fidelity claim needs additional current-contract evidence before M004 consumes it.

Predecessor:
- `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`;
- `plans/implementation/eggsearch-evidence-workflows/001-evidence-fidelity-and-retrieval-state.md`;
- `plans/closure/eggsearch-evidence-workflows/001-status.md`;
- implementation `6be801e3`.

Authoritative upstream contract, verified at the **published v0.4.1 tag** (not just the moving main branch):
- `eggstack/eggsearch@v0.4.1/src/core/retrieval_status.rs` — `ResponseRetrievalSummary.dimensions: Vec<RetrievalDimensionStatus>` with per-dimension `state`, `attempt_outcome`, `truncation_evidence`, provider/role fields and summary flags/counts;
- `eggstack/eggsearch@v0.4.1/architecture/codegg-contract.md#9-retrieval-state-interpretation`;
- `eggstack/eggsearch@v0.4.1/docs/tool-matrix.md`.
CodeGG currently bundles **eggsearch 0.3.9**; use a captured 0.3.9 fixture for backward compatibility and the tagged 0.4.1 contract as the release migration gate.

## 2. Defect and why M001 verification missed it

The M001 `src/research/sources/eggsearch.rs::receipt_from_value` implementation reads `retrieval_state.status` or a scalar `retrieval_summary.status|state|complete`. The actual tagged 0.4.1 diagnostic retrieval summary reports **`retrieval_summary.dimensions[*].state`** and summary-wide `has_failures`, `has_absences`, `has_truncation`, and attempt/completion counters. Its dimension states are `satisfied`, `completed_no_match`, `failed`, `skipped_by_policy`, `capability_unavailable`, `interrupted`, `partial`, `not_applicable`. Exact outcomes and truncation evidence are independently typed.

M001 tests asserted synthetic `retrieval_state: {status: "complete"/"partial", complete: ...}` and direct `providers_failed`; they did not supply a representative `retrieval_summary.dimensions` response. Thus real complete no-match and all-satisfied calls can degrade to `Unknown`, and a provider failure represented only inside dimensions may be lost or misrepresented. This is a **contract/fixture gap**, not evidence that a release binary or network smoke failed.

Preserve M001's successful work: structured receipt transport, nested dates, source IDs, storage compatibility, network budget policy, bounded trust markers and durable synthesis output.

## 3. Invariants

- Only a **positively established completed** retrieval can carry a `Complete` receipt. A zero-card count or `success=true` alone is insufficient.
- `has_failures`, failed dimension states/attempts, policy or capability skips, interruptions, partial results, uncertain limits and confirmed truncation must never turn into negative findings merely because `providers_failed` is absent.
- Do **not** equate `has_absences=true` with failure; `completed_no_match` is a successfully completed outcome for its requested evidence role.
- Do **not** equate `limit_reached_unknown` with *confirmed* truncation; uncertain completeness remains visible.
- `not_applicable` dimensions do not prove requested evidence coverage. Respect `workflow_coverage`/required roles when available and refuse to infer that every requested dimension was covered from result counts alone.
- Full structured MCP result is authoritative over bounded display text. A truncated model-facing projection does not by itself mean internally retained evidence was truncated.
- No model trust or provider authority is conferred by upstream provenance. Preserve existing `external_untrusted` source classification and policy.
- No new providers, network clients, caches, execution paths, background tasks, tool schemas or duplicate search runtime. Shared daemon-owned MCP ownership unchanged.

## 4. Corrective milestone

### C001 — Tagged-retrieval-summary dimension normalization

Status: **closing**. Plan:
`plans/implementation/eggsearch-evidence-workflows-corrective/001-tagged-retrieval-dimension-normalization.md`.

Dependencies:
- M001 implementation and its historical closure: **satisfied**.
- Published eggsearch v0.4.1 retrieval contract: **interface**, established via tagged source and deterministic fixture. **No hard dependency** on CodeGG legacy-retirement M001 or runtime pin adoption.
- A **real installed binary smoke**, if available, is operational/qualification evidence; cannot be replaced with a fabricated claim. A contract-derived offline fixture is required for implementation closure.
- Legacy-retirement M001 provider/version upgrade: separate **parallel** workstream; coordinate edits on common paths and requalify C001 against the newly pinned release when it lands.

Required outcome:
- Current-tagged `retrieval_summary.dimensions` semantics and authoritative attempted/coverage/truncation metadata become CodeGG's receipt basis.
- Fallback handling of older actual release shapes is explicit and conservative.
- Complete zero matches, complete nonempty, partial, all-provider failure, policy skip, capability unavailable, interruption, uncertain limit, confirmed truncation and unknown/malformed states remain distinguishable at the research report/serialized receipt boundary.
- Regression fixtures use the real tagged field paths and protect against later drift.

## 5. Dependency update and closing conditions

M001's historical closure remains recorded **closed**. Until C001 strict closure, **M004 structured-context projection additionally hard-blocks on C001**, because the producer status it would display is not yet verified against the current authoritative upstream shape.

M002/M003 still depend on CodeGG's **separate** legacy-retirement M001 (release pin adoption), not this correction. M005 remains gated by M002/M004. C001 may run in parallel with the separate legacy-retirement M001, but must not cause two agents to edit the same file concurrently.

C001 closes only with:
1. implementation SHA and tagged v0.4.1 fixture/source fingerprint;
2. requirement-to-test matrix including failures and unknowns;
3. `research` coordinator/report/store proof, no-match vs failure, and no false-complete cases;
4. focus/quick verification results plus explicit unrun evidence;
5. independent inspection of conformance to the upstream `ResponseRetrievalSummary` types;
6. a new `plans/closure/eggsearch-evidence-workflows-corrective/001-status.md` record classifying any residual findings.

No automatic closure or change to historical M001 closure is authorized by this planning-only commit.
