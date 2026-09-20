# Coding-Agent Tool Surface Post-Closure Evidence Polish M001 — LSP Preview Apply Concurrency and Restart Evidence

Status: ready for handoff

Repository baseline: `44a1f02476409829a752e860ae0917695501ea7a`

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-post-closure-evidence-polish-addendum.md#m001--lsp-preview-apply-concurrency-and-restart-evidence-polish`

Predecessor evidence:

- `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md`
- `plans/closure/coding-agent-tool-surface-corrective/005-status.md`
- `plans/closure/coding-agent-tool-surface-corrective/006-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`

Applicable ADRs:

- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted, unchanged)

Primary class: invariant

## 1. Objective

Add dedicated deterministic regression evidence for the two M005 semantics that are currently justified primarily by composition reasoning:

1. racing the same preview through `lsp_preview_apply` produces exactly one successful checked mutation and one durable checkpoint; and
2. ending the owning turn/tool-registry lifetime invalidates that preview ID so a fresh registry cannot replay it.

No production behavior change is expected. If either fixture exposes a real runtime defect, stop the evidence pass and register the smallest correctness corrective instead of repairing architecture opportunistically inside this milestone.

## 2. Why this milestone is ready

All hard dependencies are closed:

- M005 implemented the direct-only preview-ID adapter and passed focused/broad verification.
- M006 established the daemon-owned LSP service, shared turn-local preview registry, canonical workspace locks, and reusable session/workspace binding.
- ADR-0008 fixes the caller-specific authorization and ephemeral preview-lifetime contract.

The remaining issue is evidence completeness. The M005 plan explicitly requested same-preview race and restart invalidation trajectories. The closure explains why the current code satisfies them but does not record dedicated adapter-level fixtures for those exact paths.

## 3. Current implementation evidence

`src/tool/lsp_preview_apply.rs` currently:

- accepts only `preview_id` with `deny_unknown_fields`;
- exports the candidate from the shared M006 registry;
- rejects absent, stale, unsupported, empty, and already-applied candidates before mutation;
- binds host-owned workspace/session/turn state;
- validates durable session/workspace binding;
- calls only `src/lsp/mutation.rs::apply_preview()`;
- marks the shared preview applied only after successful mutation;
- advertises `DirectOnly`, `NonIdempotent`, and zero broker retries.

`src/lsp/mutation.rs::apply_preview()`:

- validates request shape/digest/containment;
- acquires `WorkspaceLockTable::acquire_repository()`;
- captures file pre-state under that lock;
- rejects original-hash mismatch;
- applies/rolls back writes;
- persists one edit checkpoint after successful post-state capture;
- emits file-change/LSP synchronization only after the checked write path.

`ToolRegistry` owns a turn-local `LspPreviewRegistryHandle`; no preview row is persisted to SQLite and no process-global registry exists.

Existing tests prove normal supported-kind apply, stale/already-applied rejection, shared-handle visibility, registry isolation, contract/retry classification, and canonical mutation failure behavior. They do not name a dedicated concurrent same-preview adapter test or a fresh-production-registry old-ID expiry test.

## 4. Invariants that must not regress

- At most one successful checked mutation per preview under concurrent direct calls.
- At most one durable edit checkpoint for that successful same-preview race.
- The losing call performs no committed file mutation.
- The registry is marked applied only after the successful checked mutation.
- No automatic ToolBroker retry is introduced.
- Preview lifetime remains bounded to the owning tool-registry/turn lifetime.
- A fresh registry cannot resolve or reconstruct an old preview from durable session/checkpoint/LSP state.
- `apply_preview()` remains the single digest/hash/write/rollback/checkpoint owner.
- The test must not require model-supplied workspace/session/patch/digest material.
- Read-only profile and Tool Program exclusions remain unchanged.

## 5. Scope

### In scope

- Focused adapter-level same-preview contention test.
- Durable checkpoint cardinality assertion for the race.
- Fresh-registry/turn-lifetime preview expiry test.
- Explicit zero-side-effect assertions for the expired ID.
- Existing no-retry contract assertion if the new contention fixture does not already exercise it.
- Minimal test helpers needed to construct production-style session registries/tools.
- Documentation/closure evidence updates if test names or canonical restart proxy semantics need to be described.
- Broad regression verification.

### Out of scope

- Production preview claim/in-flight state unless a test proves it is required for correctness.
- Preview persistence.
- New runtime locks, queues, fencing tokens, generations, or scheduler paths.
- Changes to ADR-0008.
- New LSP operations or WorkspaceEdit classes.
- Tool Program exposure.
- Provider/live-LSP tests.
- General test-suite cleanup.

## 6. Required production changes

None expected.

Permitted changes are tests, test fixtures, and documentation. A narrowly `#[cfg(test)]` synchronization helper is acceptable only if existing public seams cannot make the race deterministic; it must not change release behavior or public API.

If the new tests fail because both calls can commit, duplicate checkpoints can occur, or old preview IDs survive a fresh registry lifetime, stop and create a separate production corrective plan. Do not hide such a defect as "test polish."

## 7. Ordered work packages

### Work package A — Same-preview concurrency fixture

Create one preview whose patch changes a real temporary file and whose session/workspace rows are valid.

Use one shared:

- `LspPreviewRegistryHandle`;
- `WorkspaceLockTable`;
- durable test pool/session/workspace;
- `LspPreviewApplyTool` runtime binding.

Launch two direct `lsp_preview_apply` calls against the same preview ID.

Preferred deterministic coordination:

- use the existing canonical repository lock or a test-only barrier so both calls are genuinely in flight against the same preview before the first completion;
- avoid sleeps, wall-clock assumptions, random retries, or iteration-count probability.

Acceptance evidence:

- exactly one `Ok`;
- exactly one error classified as already-applied or stale/original-hash rejection;
- final file equals the single intended patched content;
- the preview registry records `applied = true`;
- exactly one `edit_checkpoint` row exists for the successful same-preview mutation;
- no broker retry path is used.

### Work package B — Fresh-registry lifetime/restart fixture

Construct production-style registry/tool lifetime A using the M006 session factory seam.

- create one preview and capture its opaque ID;
- prove the preview is visible through A's shared handle;
- release/drop A.

Construct lifetime B with:

- the same durable session/workspace IDs and pool where practical;
- the same daemon-style LSP service or a valid replacement instance;
- a fresh tool registry and therefore a fresh preview-registry handle.

Assert:

- A and B preview registry handles are not the same allocation/state;
- B cannot find A's preview ID;
- invoking `lsp_preview_apply` with the old ID returns the canonical not-found/expired error before mutation;
- target file content is unchanged;
- checkpoint count is unchanged.

This is the minimum canonical restart proof because preview lifetime is owned by the registry, not by a durable daemon store. If an existing daemon restart harness can exercise the same property cheaply and deterministically, add it as supplemental evidence, not as a new runtime requirement.

### Work package C — Contract and negative reconciliation

Reassert:

- public schema remains preview-ID-only;
- contract remains direct-only/non-idempotent/zero-retry;
- stale/already-applied behavior remains green;
- unsupported preview kinds/empty patches remain rejected;
- read-only agents and Tool Programs remain excluded through existing tests/guards.

Do not duplicate every M005 test; run the canonical existing suites.

### Work package D — Broad qualification and closure

Run focused tests first, then all required M005/M006 regression and repository gates.

Write:

- `plans/closure/coding-agent-tool-surface-post-closure-evidence-polish/001-status.md`

with exact concurrency outcomes, checkpoint count, registry-lifetime identity evidence, and current-baseline commands.

On green evidence, close this addendum without changing the historical M005/M006 closure records.

On a semantic failure, stop; mark this milestone corrective-pass-required/blocked as appropriate, reconcile the predecessor campaign status if the defect invalidates strict closure, and register a separate implementation corrective.

## 8. Failure, cancellation, restart, and contention semantics

### Concurrent apply

The intended behavior is schedule-independent:

- if caller B exports after caller A marks applied, B fails early as already applied;
- if both export before A completes, the canonical repository lock serializes mutation and B must fail original-hash/stale revalidation after A commits;
- neither schedule permits two successful checkpoints.

The fixture should accept either losing error class while requiring exactly one success and one checkpoint.

### Failed first caller

Do not broaden this milestone into exhaustive fault injection. Existing mutation tests own rollback/checkpoint failure. The new race fixture should fail loudly if the first caller unexpectedly errors and neither call succeeds.

### Restart/teardown

Dropping the registry destroys preview reachability. Reusing durable session/workspace/checkpoint state must not reconstruct the preview.

### Cancellation

No new cancellation semantics are introduced. If a deterministic race helper requires spawned tasks, ensure tasks are joined/aborted cleanly and no test leaks locks/background work.

## 9. Compatibility and migration

No runtime, storage, wire, model-schema, or configuration migration is expected.

Test-only helpers must not become a public compatibility contract.

## 10. Required tests

Add focused tests with stable descriptive names, for example:

- `same_preview_concurrent_apply_commits_once`;
- `fresh_tool_registry_expires_prior_preview_id`.

Exact placement is implementation-defined, but at least one test must exercise `LspPreviewApplyTool` rather than only `apply_preview()`.

The concurrency test must query durable checkpoint state, not just count successful futures.

The lifetime test must use a fresh M006-style preview handle/tool-registry construction, not merely call `PreviewArtifactRegistry::new()` in isolation.

Retain existing tests for:

- supported rename/formatting/code-action application;
- stale and already-applied rejection;
- shared/default registry isolation;
- session/workspace binding;
- edit checkpoint integration;
- contract guards;
- disclosure/read-only profile restrictions.

## 11. Required verification commands

Use current exact target names if repository targets move and record substitutions.

```bash
cargo test -p codegg --lib tool::lsp_preview_apply -- --test-threads=1
cargo test -p codegg --lib tool::lsp -- --test-threads=1
cargo test -p codegg --lib lsp::mutation -- --test-threads=1
cargo test --test tool_registry -- --test-threads=1
cargo test --test edit_checkpoint_integration -- --test-threads=1
cargo test --test tool_contract_guards -- --test-threads=1
cargo test --test tool_execution -- --test-threads=1
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

If the new fixtures are placed in a dedicated integration target, run that target explicitly in addition to the commands above.

## 12. Documentation updates

Expected:

- this plan's closure record;
- source roadmap status;
- `plans/registry.md`.

Update `architecture/lsp.md` only if a short testing/semantics note is needed to describe the now-executable same-preview and lifetime guarantees. Do not rewrite architecture merely to add test names.

Historical M005/M006 closure records remain unchanged.

## 13. Acceptance criteria

M001 closes only when:

1. a dedicated adapter-level concurrent same-preview fixture is deterministic and green;
2. exactly one call succeeds;
3. exactly one durable edit checkpoint is recorded;
4. the losing call has no committed file side effect;
5. the preview ends marked applied;
6. a fresh production-style registry lifetime cannot resolve or apply the old preview ID;
7. expired-ID invocation leaves file and checkpoint state unchanged;
8. direct-only/non-idempotent/zero-retry contract remains green;
9. all focused and broad verification commands pass;
10. no medium/high correctness finding remains;
11. predecessor M005/M006 closure records are not rewritten.

## 14. Stop conditions

Stop and report/register a separate correctness corrective if:

- both concurrent calls can succeed;
- more than one checkpoint can be recorded for the same preview race;
- a losing race caller can overwrite/modify files after the winner;
- an old preview ID survives a fresh registry lifetime through durable/global state;
- proving the race requires adding production synchronization or a new claim protocol;
- fixing a failure would change ADR-0008 authorization or preview-lifetime semantics;
- the test requires making preview IDs durable;
- broad verification exposes an unrelated production defect.

Do not weaken the race, use probabilistic sleeps, or accept "eventually usually one success" as closure evidence.

## 15. Closure evidence required

Record:

- baseline/final SHA;
- exact new test names;
- how concurrency was deterministically coordinated;
- both caller outcomes;
- final file content;
- pre/post checkpoint counts and the resulting checkpoint ID;
- shared preview entry terminal state;
- fresh-registry handle identity/isolation evidence;
- old-ID error and zero-side-effect evidence;
- contract/retry evidence;
- complete command/result table;
- unresolved findings;
- registry/roadmap disposition;
- explicit statement that historical M005/M006 closure records remain immutable.

## 16. Handoff notes

This is deliberately a proof pass. The current architecture already has the right ingredients: host-owned preview state, canonical repository locking, stale hash validation, one checkpoint owner, and ephemeral registry lifetime. The implementation agent should turn those arguments into deterministic tests, not invent new runtime machinery. If the arguments prove false under the new fixtures, that is valuable evidence and must become a separate correctness corrective.
