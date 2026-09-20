# Coding-Agent Tool Surface — Post-Closure Evidence Polish Addendum

Status: closed

Repository audit baseline: `44a1f02476409829a752e860ae0917695501ea7a`

Predecessor work:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md` — M001-M007 closed.
- `plans/closure/coding-agent-tool-surface-corrective/005-status.md` — checked LSP preview application closure.
- `plans/closure/coding-agent-tool-surface-corrective/006-status.md` — LSP runtime/authority seam closure.
- `plans/closure/coding-agent-tool-surface-corrective/007-status.md` — M003/M004 supplemental qualification closure.

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` — controlling and unchanged.
- No new ADR is required. This addendum adds executable evidence for already-selected concurrency and ephemeral-lifetime semantics. If testing exposes a need to change authorization, mutation ownership, locking, preview persistence, or caller boundaries, stop and register a separate corrective/ADR as required.

## 1. Purpose and ownership boundary

The coding-agent tool-surface corrective campaign is closed and its production implementation is coherent. A post-closure review found a narrow evidence gap in M005 rather than a known runtime defect.

The M005 plan explicitly required:

1. two callers racing the same preview must produce at most one successful apply; and
2. restart/teardown must invalidate ephemeral preview IDs.

The M005 closure explains why both properties should hold from the canonical repository lock, original-hash revalidation, zero automatic retry, and turn-local preview-registry ownership. It does not record dedicated adapter-level fixtures for those exact trajectories.

This addendum owns one bounded qualification/polish milestone that turns those two architectural arguments into deterministic executable regression evidence. It does not reopen M005 production ownership or rewrite predecessor closure records.

## 2. Work classification

### Invariants

- One reviewed preview cannot produce two successful checked mutations under concurrent direct callers.
- Duplicate/racing application cannot create more than one durable edit checkpoint for one successful mutation.
- Preview possession does not bypass stale/hash/session/workspace validation.
- A fresh tool-registry lifetime cannot resolve a preview ID from a prior registry lifetime.
- Restart/teardown does not reconstruct previews from transcripts, database state, LSP state, or model output.
- `lsp_preview_apply` remains direct-only, non-idempotent, and non-retryable.
- `src/lsp/mutation.rs::apply_preview()` remains the sole checked mutation owner.

### Capabilities

No new user or model capability is introduced.

### Infrastructure

No new runtime infrastructure is expected. Test-only fixture helpers may be added when they do not change production semantics.

### Polish

Dedicated contention/restart evidence, closure documentation, and any stale test instructions discovered while building the fixtures.

## 3. Non-goals

- Reopening or redesigning M005/M006.
- Persisting preview artifacts across turns or daemon restarts.
- Adding a preview claim database, global registry, generation service, or second lock.
- Changing supported LSP preview kinds.
- Changing ToolBroker retry or permission semantics.
- Expanding Tool Programs or read-only agents to mutation.
- Adding a live-provider/LSP-server CI lane.
- Broad LSP reliability/performance work.
- Rewriting historical M005/M006 closure records.

## 4. Current state and evidence gap

At the audit baseline:

- `LspPreviewApplyTool` exports one host-owned candidate from the shared M006 `LspPreviewRegistryHandle`, rejects stale/already-applied/unsupported candidates, validates the trusted session/workspace binding, and calls `apply_preview()`.
- The tool contract is `DirectOnly`, `NonIdempotent`, with `ToolRetryPolicy::none()`.
- `apply_preview()` acquires the canonical repository lock before reading/checking file pre-state and rejects a second caller after the first write because the original SHA no longer matches.
- The adapter marks the preview applied only after `apply_preview()` returns success.
- The preview registry is an explicit `Arc<Mutex<PreviewArtifactRegistry>>` owned by the session/turn tool-registry lifetime and is not persisted.
- Current unit coverage proves normal apply, stale rejection, already-applied rejection, host-bound request construction, shared-registry visibility, and isolated default registries.
- Current closure evidence asserts concurrent duplicate application and restart expiry from those components, but no dedicated M005 adapter-level test name/trajectory is recorded for either property.

The evidence gap is therefore precise: executable composition tests for contention and lifecycle, not missing architecture.

## 5. Target evidence architecture

The qualification should pin the existing behavior at the public adapter/runtime seam:

```text
same preview ID
      |
      +---- caller A ----> lsp_preview_apply ----+
      |                                         |
      +---- caller B ----> lsp_preview_apply ----+--> same WorkspaceLockTable
                                                  --> canonical apply_preview
                                                  --> exactly one committed checkpoint
                                                  --> one success, one rejection
```

and:

```text
tool registry / turn A
  -> create preview P
  -> P resolvable
  -> teardown/drop

fresh tool registry / turn B
  -> same session/workspace may exist
  -> preview state is a new handle
  -> old P is not resolvable
  -> no file mutation / checkpoint
```

The tests must exercise existing owners. Do not create a production-only coordination API just to make a test convenient.

## 6. Dependency graph

```text
Closed M005 checked adapter
        +
Closed M006 runtime/preview ownership
        +
Accepted ADR-0008
        |
        v
M001 concurrency + restart evidence polish
```

All hard dependencies are closed. M001 is dependency-ready.

## 7. Milestone

### M001 — LSP preview apply concurrency and restart evidence polish

Class: invariant.

Implementation plan:

- `plans/implementation/coding-agent-tool-surface-post-closure-evidence-polish/001-lsp-preview-apply-concurrency-restart-evidence.md`

Objective: add deterministic regression evidence for same-preview contention and preview lifetime expiry, then re-run the bounded M005/M006 verification set without changing production architecture.

Exit conditions:

- a dedicated adapter-level same-preview concurrency test proves exactly one successful mutation and no duplicate checkpoint;
- a dedicated fresh-registry/restart-equivalent test proves an old preview ID is unavailable after the owning registry lifetime ends and causes zero mutation;
- the existing no-retry contract remains pinned;
- broad repository verification remains green;
- if either test reveals a real semantic defect, this evidence milestone does not close around it: a separate corrective implementation plan is registered and the affected predecessor status is reconciled truthfully.

## 8. Cross-cutting requirements

### Storage and migration

No storage migration. Tests may inspect existing checkpoint rows to prove cardinality, but must not add preview persistence.

### Protocol and compatibility

No protocol/schema change expected. Public model input remains preview-ID-only.

### Security and authorization

Tests must use host-bound workspace/session state and the existing ToolBroker/tool contract where practical. No model-supplied authority fields may be introduced for fixture convenience.

### Concurrency and contention

The contention fixture must be deterministic enough to prove single-commit behavior. It may coordinate test tasks around the existing canonical `WorkspaceLockTable`, or use a narrowly `#[cfg(test)]` synchronization aid if necessary. It must not add a production lock/claim path.

### Restart/lifecycle

A newly constructed production-style tool registry with the same durable session/workspace but a fresh preview handle is the minimum deterministic proof of restart/teardown expiry because the preview registry itself is the canonical ephemeral owner. If an existing cheap daemon-restart harness can assert the same property without adding infrastructure, it may be used additionally.

### Observability

Closure evidence records exact success/error outcomes and checkpoint cardinality. Do not infer the result solely from final file contents.

## 9. Verification strategy

Use focused deterministic tests first, then rerun the current M005/M006 regressions and broad local gates.

The concurrency fixture should ensure two direct apply calls target the same preview and canonical workspace lock. It must assert:

- exactly one call succeeds;
- the other returns already-applied or stale/hash rejection;
- final file content matches one application;
- exactly one checked edit checkpoint is durably recorded for the attempt;
- the shared preview entry is marked applied after success;
- zero automatic retry occurs.

The lifecycle fixture should:

- create/register a preview in registry lifetime A;
- prove it is present;
- drop/end lifetime A;
- construct a fresh production-style registry/adapter lifetime B using the same durable session/workspace identities;
- prove the handles/state are distinct;
- invoke or resolve the old ID through B and receive not-found/expired;
- assert file content and checkpoint count are unchanged.

## 10. Risks and decision points

- A naïve concurrent test may pass without ensuring meaningful overlap. Prefer explicit coordination around the existing lock or a test-only barrier rather than sleep-based timing.
- Counting only successes is insufficient if both callers can create checkpoints or side effects. Assert durable checkpoint cardinality.
- A fresh isolated `LspTool::new()` test alone is insufficient for restart evidence; use the production/session registry ownership seam established by M006 where practical.
- If old preview IDs become resolvable through durable state, that is a production defect and architectural change, not evidence polish.
- If two concurrent callers can both succeed or create duplicate checkpoints, stop and register a correctness corrective before claiming closure.

## 11. Completion definition

This post-closure addendum closes when M005's two previously argument-based properties—same-preview at-most-once mutation and restart/teardown preview expiry—have dedicated deterministic executable evidence, the normal M005/M006 contract remains unchanged, and no unresolved medium/high finding remains.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/coding-agent-tool-surface-post-closure-evidence-polish/001-lsp-preview-apply-concurrency-restart-evidence.md` | `plans/closure/coding-agent-tool-surface-post-closure-evidence-polish/001-status.md` | None; M005/M006 closed and ADR-0008 accepted. |
