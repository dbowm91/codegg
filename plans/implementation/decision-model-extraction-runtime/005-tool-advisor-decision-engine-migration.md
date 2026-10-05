# Decision-Model Extraction and Runtime Milestone 005 — Tool-Advisor DecisionEngine Migration

Status: active

Repository baseline: `533be5941ac48334743de555cda96d8695cc6527`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m005--tool-advisor-migration-to-decisionengine`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability

Hard dependencies:

- M003 closed positively;
- M004 interface/contract stable and closed or conditionally closed with no unresolved semantic/security defect affecting `DecisionEngine` (satisfied by `plans/closure/decision-model-extraction-runtime/004-status.md`).

## 1. Objective

Migrate learned tool-selection advice onto the backend-neutral `DecisionEngine` path while keeping CodeGG as the sole owner of tool candidate construction, deterministic fallback, disclosure/rerank/promotion policy, authority checks, and downstream qualification.

This milestone proves backend substitutability in the real agent loop. It does not qualify a previously failed model or turn learned promotion on by default.

## 2. Current implementation evidence

Current tool-advisor behavior is spread across `src/tool_advisor/`, request preparation, candidate preselection/retrieval, configuration modes, telemetry/capture, and qualification tooling.

Historical work established these durable semantics:

- `off` is the baseline;
- `observe` scores without behavior change;
- rerank preserves the candidate set;
- promotion can only make an already-allowed/deferred tool immediately visible and is budgeted;
- failure returns to deterministic discovery;
- telemetry/capture consent is independent;
- historical learned qualification is negative/unqualified for live promotion;
- causal-frontier active disclosure is a separate deterministic CodeGG-owned path.

M005 changes only how learned decisions are obtained.

## 3. Invariants that must not regress

- Existing tool authority/policy tests remain authoritative.
- The decision engine receives a projection of the already-admitted candidate universe.
- Backend output cannot add a tool not present in the request.
- BM25/deterministic fallback remains available.
- Learned `rerank` preserves candidate membership exactly.
- Learned `promote` remains opt-in/qualification-gated and cannot become default through migration.
- Existing negative qualification dispositions remain negative.
- Causal-frontier observe/active modes remain behaviorally independent and are not routed through the learned engine.
- Capture/telemetry settings do not follow backend selection automatically.
- There is one learned actuation owner in request preparation.

## 4. Scope

### In scope

- Replace direct learned scorer/runtime invocation with `DecisionEngine`.
- Convert policy-allowed tool context/candidates into Rank requests via M001 adapter.
- Convert supported Rank responses into existing advisor diagnostics/rerank/promotion input.
- Explicit fallback on Unsupported/Unavailable/Degraded/timeout.
- Backend selection among off/local/System One or future backend; since M004 does not support Rank, System One must fall back/decline learned tool ranking unless a later backend profile explicitly advertises a lossless Rank capability.
- Preserve existing mode/config compatibility where feasible.
- Backend-swap/shadow tests in real request-preparation paths.
- Update tool-advisor diagnostics to identify decision backend separately from CodeGG policy mode.
- Remove production routing to model-specific legacy learned runtimes, leaving their source only for historical/retirement work until M006.

### Explicitly out of scope

- New learned architecture.
- Changing retrieval relevance labels/gates.
- Re-running historical final-test qualifications to seek a better verdict.
- Modifying causal-frontier contract/state.
- Deleting legacy training/model source (M006).
- Making a remote backend default.
- Tool argument generation.

## 5. Required production changes

### Runtime ownership

At the learned-scoring seam, replace architecture-specific scorer selection with an injected/resolved `DecisionEngine` snapshot.

The agent loop should know:

- advisor policy mode (off/observe/rerank/promote);
- selected decision backend state/capabilities;
- decision deadline/bounds;
- resulting scores/status.

It should not know encoder/head/tokenizer classes.

### Candidate construction

Reuse the real resolved/deferred candidate path. Do not let the backend fetch or discover tools.

Build stable Rank candidate ids from CodeGG canonical identities and bounded descriptors.

For large catalogs, any deterministic shortlist remains CodeGG-owned. If backend scoring requires a bounded candidate count, overflow behavior is explicit and deterministic.

### Response application

Validate the response against the request fingerprint/candidate ids.

Then:

- Observe: diagnostics only.
- Rerank: reorder only existing shortlist candidates, with deterministic tie/fallback.
- Promote: only if the current mode/artifact/backend remains explicitly qualified by CodeGG policy; otherwise remain observe/rerank-only as historical status requires.

Backend confidence is not automatically a promotion threshold. Existing calibrated promotion policy must be retained or requalified separately.

### Backend selection

Resolve configuration once per turn/request-preparation snapshot.

If selected backend lacks Rank capability, return to deterministic behavior; do not fan out repeated Choice calls.

### Telemetry/capture

Training/capture events should record backend/model/runtime provenance generically.

Do not upload/send the decision request merely because a remote backend exists. Runtime remote state transmission is governed by M004 configuration; training telemetry is separately governed by existing consent.

### Historical CLI

Commands that exist solely to train architecture-specific models remain historical until M006. Production-facing advisor inspect/status should present the new backend state.

## 6. Ordered work packages

### A — Adapter integration in observe mode

Wire the M001 tool Rank request through `DecisionEngine` in observe-only path.

Acceptance: off-vs-observe provider definitions remain identical; diagnostics identify backend/status.

### B — Rerank migration

Route learned reranking through generic Rank response.

Acceptance: candidate set equality property holds over current policy fixtures; unsupported backend falls back to deterministic ordering.

### C — Promotion gate preservation

Connect generic Rank scores only to the existing promotion policy where qualification permits.

Acceptance: no historical negative/unqualified model becomes active merely because the backend changed; promotion authority/budget tests remain green.

### D — Backend swap/failure matrix

Exercise off/local and an unsupported Rank backend profile through the same live request-preparation path.

Acceptance: one CodeGG policy path consumes all; no backend-specific actuation branch bypasses it.

### E — Diagnostics/capture migration

Replace model-specific provenance fields where necessary with generic backend/model/artifact provenance while maintaining backward-readable stored events if applicable.

Acceptance: old local event versions remain inspectable or have an explicit migration reader; consent remains unchanged.

### F — Production legacy routing removal

Remove production callers of architecture-specific learned scorer types. Keep source/tests only where needed for M006 inventory/history.

Acceptance: static/code search proves agent request preparation reaches learned inference only through `DecisionEngine`.

## 7. Failure, cancellation, restart, contention

Decision execution is bounded by the turn/request-preparation lifetime. A backend timeout/error does not trigger an advisor retry loop.

Backend/config/model snapshot is immutable for one preparation. Mid-turn config changes apply next preparation.

Repeated backend failure may mark backend degraded, but CodeGG deterministic discovery remains available.

## 8. Security

- Policy filtering happens before request construction.
- Response ids are matched against the request; unknown ids reject the response.
- No backend can alter permissions/tool schemas/tool arguments.
- Remote-state privacy from M004 remains explicit.
- Capture/telemetry redaction and consent remain separately enforced.
- Diagnostics do not dump raw decision state.

## 9. Compatibility and migration

No database migration expected unless existing captured-event structs require a new version; if so, version additively and retain readers for existing event files.

Legacy advisor config should continue to produce safe behavior. Model-specific config fields not understood by the new backend emit actionable diagnostics and fall back.

## 10. Required tests

- request-preparation off equivalence;
- observe no behavior delta;
- rerank candidate-set equality;
- promotion cannot widen authority;
- unsupported Rank backend fallback;
- local backend swap;
- stale/mismatched response fingerprint rejection;
- backend timeout/cancel;
- concurrent config/artifact snapshot;
- telemetry independence;
- causal-frontier regression suite unchanged;
- static search/guard for one learned actuation owner if warranted.

## 11. Required verification commands

At minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked --test <resolved-tool-surface-or-agent-loop-target>
cargo test --locked --workspace
scripts/verify.sh quick
git diff --check
```

Run feature-specific local-backend tests and M004 fake-server tests if their code is touched.

## 12. Documentation updates

- `architecture/tool-advisor.md`
- advisor configuration/reference docs
- decision runtime/backend diagnostics docs
- roadmap/registry
- architecture diagrams that currently show a model-specific scorer in the agent loop.

## 13. Acceptance criteria

M005 closes only when:

1. learned advisor calls in the real agent loop go through `DecisionEngine`;
2. CodeGG still owns candidate authority and actuation;
3. off/observe/rerank/promotion invariants remain proven;
4. unsupported backend semantics safely fall back;
5. backend swap does not require agent-loop architecture changes;
6. causal-frontier behavior is unchanged;
7. telemetry/capture consent remains independent;
8. no historical negative model is silently promoted.

## 14. Stop conditions

Stop if:

- backend code must query CodeGG permissions/catalog directly;
- M005 requires changing frozen qualification gates/results;
- System One Rank is approximated by repeated Choice;
- two production learned-actuation paths remain;
- migration changes causal-frontier semantics;
- remote backend selection implicitly enables telemetry/training capture.

## 15. Closure evidence required

- production call-path inventory before/after;
- backend-swap matrix;
- off/observe/rerank/promotion invariant tests;
- authority-negative evidence;
- telemetry/causal regression evidence;
- static/code-search proof of one learned inference seam;
- verification results;
- unresolved findings;
- M006 retirement inventory.
