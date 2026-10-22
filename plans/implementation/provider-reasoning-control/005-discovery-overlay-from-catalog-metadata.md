# Provider Reasoning Control Milestone 005 — Discovery overlay from provider catalog metadata

Status: closed (M003 closed; ADR-0049 accepted 2026-10-21; closure record `plans/closure/provider-reasoning-control/005-status.md`; the overlay machinery has **no live source** at this baseline and the daemon-side install site is deliberately not wired)

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`)

Source roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-5--discovery-overlay-from-provider-catalog-metadata`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — provider connection ownership; CodeGG operates against direct providers without requiring an EggPool process.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — capability-negotiated protocol.

Applicable ADRs:

- `plans/adrs/ADR-0049-per-model-reasoning-capability-authority.md` (§1 the authority order — the overlay is the third source and may only narrow; §5 no storage change).
- `plans/adrs/ADR-0048-open-provider-scoped-reasoning-control-vocabulary.md` — the open vocabulary the overlay constrains.

Primary class: infrastructure

Amendment (2026-10-21, after ADR-0049's acceptance): the milestone's **decision**
is unchanged — a provider-published overlay may only narrow the compiled baseline,
it is applied at M003's single resolution point, malformed content degrades to
`Unknown` per dimension, and the overlay is absent for every direct provider
connection. The **premise** is corrected: the
`eggpool.capabilities.thinking.control_contract.accepted_efforts` JSON path named
in §2 and §3 is a design document in the pinned upstream, not an implemented wire
contract, and the pinned `/v1/models` response carries no capability block at all.
The deliverable is therefore forward-compatible narrowing machinery with **no live
source** at this baseline, and §13 now carries the disclosure criterion that says
so. See §3 and ADR-0049's amendment note.

## 1. Objective

Use provider-published capability metadata, **where the provider publishes it**, to narrow the compiled adapter baseline per deployment — so a deployment whose provider advertises a narrower label set is offered exactly that set, without a CodeGG release.

This is **infrastructure**, not a user-visible capability: M003's compiled baseline is the capability, and M004's option surface is the user-visible part. Neither depends on this milestone. Its class is infrastructure because a consumer path already exists (M003's resolution function accepts an overlay) and this milestone supplies the overlay's data.

## 2. Why this milestone is ready

- **Interface dependency, not a hard one:** M003 closed. M003's resolution function already accepts an absent overlay — the case for every direct provider connection — so this milestone is not on M003's critical path and M003 does not wait for it. It needs M003's record shape and its narrowing rule, which are stable.
- **ADRs:** ADR-0049 accepted (its §1 overlay position and its "may only narrow" rule). See §14.
- **Interface dependency — the catalog probe and its digest.** `crates/codegg-providers/src/eggpool.rs` holds the bounded catalog probe: `parse_compatible_models_response` returns `Vec<EggpoolModelSummary>`, and `digest_models` computes the catalog revision by hashing each model's `id` and `name` only.
- **The upstream contract does not exist yet — verified 2026-10-21.** The
  `eggpool.capabilities.thinking.control_contract.accepted_efforts` path this
  milestone was written against appears in the pinned upstream **only as a design
  document** (`docs/thinking.md`, `plans/…`), never in its runtime code. The
  pinned `/v1/models` handler emits a flat four-key model object (`id`, `object`,
  `owned_by`, `name`) with **no capability block at all**, and the same holds for
  the newer `d70b596` revision CodeGG pins for `eggpool-model-routing`. The one
  capability shape upstream exposes at runtime is the **flat** `thinking`
  capability (`supported_efforts`, `toggle`, `effort`, `budget`,
  `budget_tokens_min`/`_max`), and it is reachable over an integrations-profile
  route that CodeGG does not call — not over `/v1/models`. This milestone
  therefore lands the CodeGG-side narrowing machinery against **both** real
  shapes, and its closure must disclose that the overlay has **no live source**
  at this baseline. See the amendment note below and §3.

## 3. Current implementation evidence

Repository baseline `c93211542`.

**The upstream contract this milestone was written against is not implemented.**
Verified 2026-10-21 against both available upstream checkouts (the pinned
`eggpool-wire` revision `9ac6a131` and the pinned `eggpool-model-routing`
revision `d70b596`):

- `accepted_efforts` and `control_contract` appear **only** in upstream's
  `docs/` and `plans/` prose. No runtime module constructs, serialises, or
  consumes them.
- The `/v1/models` handler emits a flat four-key model object — `id`, `object`,
  `owned_by`, `name` — with no capability block. There is nothing at the parse
  boundary for CodeGG to retain today.
- The capability shape upstream *does* expose at runtime is the **flat**
  `thinking` capability — `supported_efforts`, `toggle`, `effort`, `budget`,
  `budget_tokens_min`, `budget_tokens_max` — and it is carried by the
  integrations-profile route, not by `/v1/models`.

**What this changes and what it does not.** The milestone's *decision* is
unchanged and still sound: a provider-published overlay may only **narrow** the
compiled baseline, it is applied at M003's single resolution point, malformed
content degrades to `Unknown` per dimension, and the overlay is absent for every
direct provider connection. What the verification changes is the milestone's
**deliverable**: it lands forward-compatible, bounded, narrowing-only machinery
that is **inert at this baseline** — no deployment's effective capability changes,
because no upstream publishes the contract CodeGG would read. The milestone's
acceptance criteria are written to test that machinery against fixtures, and its
closure must say plainly that there is no live source.

**The answer is already arriving and is discarded at the parse boundary.** `crates/codegg-providers/src/eggpool.rs`:

```rust
/// A bounded, stable projection of one discovered model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolModelSummary {
    pub id: String,
    pub name: String,
}
```

and the parse reduces each catalog entry to exactly that pair before the probe summary is built. The published thinking-control contract in the same response is therefore dropped before it can inform anything. The discovery path is not missing the data; it is discarding it.

**The catalog revision is computed from `id` and `name` only.** `digest_models` hashes each model's `id` and `name`, and its doc comment states the intent: "Callers may reuse this helper during refresh without changing the catalog revision when the provider returns the same content in another order." The revision is the stable identity of a catalog's *content*.

**The catalog revision is load-bearing for stored selections.** The daemon reuses a principal's last-used connection/model preference only when the connection, model, and **current revisions** all match; an invalid preference leaves the session unselected (`src/core/daemon_sessions.rs`). `provider_connection_models` is a **revision-scoped** table keyed by `(connection_id, revision, model_id)`. So a change to the catalog digest is not a cosmetic change: it can invalidate stored model preferences and split the revision-scoped model rows.

**This creates a specific trap.** Folding the capability into the existing `digest_models` would make every existing deployment's catalog revision change once, at upgrade. That is a one-time invalidation of stored last-used-model preferences and a split of the revision-scoped model rows — a user-visible consequence of a change that only adds metadata. The alternative — adding the capability to the summary but **not** to the digest — is worse in a quieter way: a capability-only change in the provider's response would not move the revision, so a client holding a cached revision would never learn that the model's options changed.

**The resolution function must therefore accept an overlay that is separate from the catalog revision.** A separate **capability revision** — a digest over the overlay content alone — gives clients and caches a way to detect a capability change without disturbing the model catalog's revision semantics, and leaves the stored model preferences keyed by the catalog revision untouched.

**The overlay is untrusted input.** It arrives from a provider endpoint that may be misconfigured, stale, or hostile. It may only **narrow** the compiled baseline; it may never add a label, introduce a bound the adapter did not declare, or change the always-reasons property.

**Direct provider connections are unaffected.** Every connection that is not an EggPool-compatible proxy has no published contract, so the overlay is simply absent — which is the state M003 already supports.

## 4. Invariants that must not regress

- **CodeGG runs against direct providers without requiring an EggPool process.** This milestone adds an optional overlay; no connection requires it.
- **The overlay may only narrow.** It can never add a label, a bound, or a capability the compiled baseline does not declare, and it can never change the always-reasons property.
- **Untrusted input is bounded and typed before use.** Malformed, oversized, or unparseable overlay content degrades to `Unknown` and never reaches a request or a UI surface.
- **The model catalog revision keeps its current meaning and its current inputs.** The digest continues to hash `id` and `name` only, so the upgrade does not invalidate stored last-used-model preferences and does not split the revision-scoped model rows.
- **Provider/model-specific transforms have one CodeGG authority.** The overlay is applied at the single resolution point M003 established, not at a second site.
- **The `eggpool-wire` pin is unchanged.**
- **No storage change.** The overlay is held in the existing discovery layer, and the roadmap explicitly defers any persistence of it to a separate recorded decision.

## 5. Scope

### In scope

- Retaining a published thinking-control contract from the catalog response instead of discarding it — in either shape a provider may publish it (the flat `thinking` capability, or the documented `control_contract.accepted_efforts` shape), with the retained value held apart from the model summary and the digest.
- The bounded, typed overlay shape and its parsing.
- The overlay's application at M003's resolution point, as a narrowing-only source.
- The overlay's refresh on the existing catalog-refresh cadence, replaced atomically.
- A separate capability revision, so a capability change is detectable without disturbing the catalog revision.

### Explicitly out of scope

- **Persisting the overlay.** ADR-0049 §5 defers it to a separate decision with its own evidence requirement. This milestone adds no column, no table, and no migration.
- **Requiring an EggPool connection.** Direct provider connections keep the compiled baseline and no overlay.
- **The frontend option surface.** M004's subject; the overlay only changes what the daemon resolves.
- **The capability record and the admission policy.** M003's subject.
- **Widening the model catalog's revision inputs.**
- **Adding a new discovery cadence.** The existing catalog-refresh cadence is used.
- **Supporting provider-specific capability metadata formats other than the published contract.** One format, parsed defensively; an unknown format is simply no overlay.

## 6. Required production changes

### Core/domain

- `crates/codegg-providers/src/eggpool.rs` — the catalog parse retains the published thinking-control contract alongside `id` and `name`, in a **separate** field on the probe output, so the model summary and the digest keep their current shape and inputs.
- A bounded, typed overlay type: the accepted effort labels (deduplicated, length-capped list of length-capped labels), whether reasoning can be disabled, whether a fixed budget is accepted and its bounds, and whether the model always reasons. Parsing is defensive: a field that is missing, of the wrong type, or out of bounds yields `Unknown` for that dimension rather than a parse failure for the whole catalog.
- A **capability revision**: a digest over the overlay content alone, computed separately from `digest_models`.

### Storage and migrations

None. No column, no table, no migration, and `STORAGE_LAYOUT_VERSION` is unchanged.

### Protocol and DTOs

None. The overlay is daemon-internal; the capability it narrows reaches frontends through M004's existing optional payload.

### Runtime and concurrency

- The overlay is applied at M003's single resolution point, as the third source in the order: operator override → adapter baseline → overlay → unknown. Its application is **narrowing-only**, and an overlay that claims something the baseline does not declare is **discarded**, not merged.
- The overlay is refreshed on the existing catalog-refresh cadence and replaced atomically, so a turn never observes a partially updated overlay and never sees a half-narrowed capability.
- The overlay is held per connection, alongside the existing catalog state, and is read-only for the request path.

### Frontend or operator surface

None directly. The overlay changes what the daemon resolves, which M004's option surface already reflects.

### Security and authorization

- **The overlay is untrusted input.** It is parsed into a bounded, typed shape before any use: a capped label count, a capped label length, a bounded budget range. Content that violates a bound is discarded, and the affected dimension becomes `Unknown`.
- **The overlay may only narrow.** A label the baseline does not declare is discarded. A budget bound the baseline does not declare is discarded. An always-reasons claim in the overlay is discarded — the compiled baseline owns that property, because a provider's advertisement is not an authority over the model's behaviour.
- **The overlay grants nothing.** It carries no authorization meaning and is never used to widen any permission, tool, path, or credential scope.
- **No arbitrary provider text is copied into the capability.** Only the bounded typed fields are retained, so a provider cannot smuggle text into a UI surface or a log.

### Documentation and static guards

- `architecture/provider.md` — the capability model section records the overlay's position, its narrowing-only rule, its bounds, and the separate capability revision.
- `architecture/storage.md` — no change is required, and the closure must state that none occurred.
- A static guard is added asserting that the discovery overlay's write path **cannot** reach the compiled adapter baseline — the narrowing property enforced structurally, not only by a test. It is registered in `scripts/verify.sh quick` **and** in the CI `verify` job, with a `--self-test`.

## 7. Ordered work packages

### Work package A — Retain the published contract without disturbing the catalog revision

Intent: Keep the metadata the provider already publishes, without changing what the catalog revision means.

Required changes:
- The catalog parse retains the published thinking-control contract in a separate field on the probe output.
- `digest_models` continues to hash `id` and `name` only.
- A separate capability revision over the overlay content.

Acceptance evidence:
- A test asserting the existing catalog digest is **unchanged** for a catalog response that also carries a thinking-control contract — that is, adding the contract does not move the model catalog revision.
- A test asserting the capability revision **does** change when the contract's content changes, and does not change when only the response ordering changes.
- A test asserting a catalog response without the contract produces no overlay and an unchanged catalog revision.

### Work package B — Parse the overlay defensively

Intent: Turn untrusted provider metadata into a bounded, typed shape.

Required changes:
- The overlay type and its defensive parsing, with per-dimension `Unknown` on anything malformed, oversized, or of the wrong type.
- The bound enforcement: label count, label length, budget range.

Acceptance evidence:
- A test asserting a malformed contract yields no overlay and does **not** fail the catalog probe.
- A test asserting an oversized label list or an over-long label is discarded and the affected dimension becomes `Unknown`.
- A test asserting a contract for one model does not leak into another model's record.

### Work package C — Apply the overlay as a narrowing-only source

Intent: Narrow the baseline; never widen it.

Required changes:
- The overlay's application at M003's resolution point.
- The discard rules for a label, bound, or always-reasons claim the baseline does not declare.

Acceptance evidence:
- A test asserting an overlay naming a label the baseline does not declare is **discarded**, and the effective capability is unchanged.
- A test asserting an overlay narrowing the baseline's label set is applied.
- A test asserting an overlay claiming a model does **not** always reason is discarded when the baseline says it does.
- A test asserting the resolution still produces the same effective capability when the overlay is absent.

### Work package D — Refresh atomically and keep the request path read-only

Intent: Ensure a turn never observes a half-updated overlay.

Required changes:
- The overlay's refresh on the existing catalog-refresh cadence, replaced atomically.
- The overlay's read-only use on the request path.

Acceptance evidence:
- A test asserting a refresh in progress is not observable as a partial overlay.
- A test asserting a turn admitted during a refresh resolves against a consistent overlay.
- A test asserting the request path never mutates the overlay.

### Work package E — Add the structural guard and reconcile the documentation

Intent: Make the narrowing property structural, and the contract documented.

Required changes:
- The static guard asserting the overlay's write path cannot reach the compiled adapter baseline, with a `--self-test`.
- The documentation updates listed in §6.

Acceptance evidence:
- The guard passes on the tree and rejects an injected path that would let the overlay write the baseline.
- The documentation names the overlay's position, its narrowing rule, its bounds, and the separate capability revision.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A malformed contract produces no overlay; the catalog probe still succeeds and the baseline stands. The overlay is never half-applied, because it is parsed into a complete typed value before it is installed.
- **Cancellation.** The overlay is not turn-scoped. A cancelled turn leaves it untouched.
- **Restart.** The overlay is re-derived by the existing catalog-refresh path. Because it is not persisted, a restart leaves the overlay absent until the next refresh, and the compiled baseline stands meanwhile. That is the intended behaviour, and it is why the roadmap defers persistence to a separate decision.
- **Duplicate delivery.** A repeated refresh producing identical content produces an identical capability revision and installs an equivalent overlay.
- **Contention.** The overlay is replaced atomically, so a turn never sees a partially updated capability. A refresh that races a turn does not change the turn's already-resolved capability.
- **Stale generations.** An overlay older than the current catalog is still narrow-only, so staleness can only under-offer, never over-offer. An overlay that becomes stale after a provider *widens* its set is corrected at the next refresh.

## 9. Compatibility and migration

- **Protocol.** Unchanged. No request or response shape changes, and no new frontend payload is added (M004 already carries the capability).
- **Storage.** None. No migration, no `STORAGE_LAYOUT_VERSION` change. The overlay is not persisted, and its persistence is explicitly deferred to a separate recorded decision.
- **The model catalog revision is unchanged.** This is the compatibility point that matters most: the digest keeps hashing `id` and `name` only, so upgrading to this milestone does not invalidate stored last-used-model preferences and does not split the revision-scoped `provider_connection_models` rows. A capability change is detectable through the **separate** capability revision instead.
- **Configuration.** Unchanged.
- **Provider behaviour.** A selection the effective capability supports is unaffected. A selection the overlay narrows away is refused locally by M003's policy, with a truthful message instead of a provider 400.
- **Build-time.** Unchanged. No adapter asset is affected; the overlay is runtime data.

## 10. Required tests

### Focused unit tests

- The overlay's defensive parsing: a malformed, oversized, or wrong-typed field yields `Unknown` for that dimension, not a parse failure.
- The overlay's bounds: label count, label length, and budget range.
- The capability revision's stability under response reordering and its sensitivity to content change.

### Integration tests

- A catalog response carrying a thinking-control contract producing an overlay, and one without producing none.
- The overlay narrowing the effective capability, and the effective capability being unchanged when the overlay is absent.
- The overlay's discard rules for a label, a bound, and an always-reasons claim the baseline does not declare.
- The catalog digest being unchanged by the presence of the contract.

### Restart and recovery tests

- A daemon restart leaving the overlay absent until the next refresh, with the compiled baseline standing in the meantime.

### Contention and cancellation tests

- A refresh in progress not being observable as a partial overlay.
- A cancelled turn leaving the overlay untouched.
- The request path never mutating the overlay.

### Security and negative tests

- A hostile overlay claiming a label the baseline does not declare is discarded.
- A hostile overlay claiming a model does not always reason is discarded.
- An oversized overlay being discarded rather than inflating a UI surface.
- The overlay carrying no authorization meaning.
- The static guard rejecting an injected path that would let the overlay write the compiled baseline.

### Migration and compatibility tests

- `STORAGE_LAYOUT_VERSION` is unchanged and the project-catalog invariants guard still passes.
- The model catalog digest is byte-identical to the baseline for the same catalog content.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-providers eggpool
cargo test -p codegg-core model_profile

# the new static guard, including its self-test
python3 scripts/check_reasoning_overlay_narrowing.py
python3 scripts/check_reasoning_overlay_narrowing.py --self-test

# the guard must be wired into the routine suite
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
cargo test -p codegg-providers -p codegg-core
```

Do not claim commands that were not actually run in the closure record. The repository's existing verification policy applies: no new CI lane, no coverage or benchmark gate, no permanent external service, and no live-network CI test.

## 12. Documentation updates

- `architecture/provider.md` — the capability model section records the overlay's position in the authority order, its narrowing-only rule, its bounds, the separate capability revision, and the fact that the model catalog revision is unchanged.
- `architecture/storage.md` — the closure states that no storage change occurred and that overlay persistence is deferred.
- `docs/providers.md` — if it documents provider capability discovery, reconcile it with the landed contract.

## 13. Acceptance criteria

1. The catalog parse **retains** a published thinking-control contract instead of discarding it, in whichever shape a provider publishes it — the flat `thinking` capability (`supported_efforts`, `toggle`, `effort`, `budget`, `budget_tokens_min`/`_max`) and the documented `control_contract.accepted_efforts` shape — and the retained value is kept in a **separate** field so the model summary and the digest keep their current shape and inputs. When neither shape is present, the overlay is absent and the effective capability is unchanged.
2. The model catalog digest is **unchanged** by the presence of the contract — the digest continues to hash `id` and `name` only, so the upgrade does not invalidate stored last-used-model preferences and does not split the revision-scoped model rows.
3. A separate capability revision changes when the overlay's content changes and does not change when only the response ordering changes.
4. The overlay is applied at M003's single resolution point as the third source, and the effective capability is unchanged when the overlay is absent.
5. The overlay may only narrow: a label, a bound, or an always-reasons claim the baseline does not declare is **discarded**, and the narrowing property is enforced structurally by a static guard with a self-test.
6. Malformed, oversized, or wrong-typed overlay content degrades to `Unknown` for that dimension and does not fail the catalog probe.
7. The overlay is refreshed on the existing catalog-refresh cadence and replaced atomically; a turn never observes a partial overlay, and the request path never mutates it.
8. Direct provider connections are entirely unaffected — no overlay, compiled baseline only, and no requirement to run an EggPool process.
9. The overlay grants no permission, tool, path, or credential scope, and no arbitrary provider text is retained.
10. `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes.
11. The documentation records the overlay's position, its narrowing rule, its bounds, the separate capability revision, and the unchanged catalog revision.
12. The closure record states plainly that the overlay has **no live source** at this baseline — the upstream contract is unimplemented in every available revision, and the capability shape upstream does expose is reachable only over a route CodeGG does not call — so the milestone is forward-compatible machinery, **not** a shipped behaviour change. The closure also names the route that would become the source if a deployment ever publishes the contract.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **M003 has not closed**, or **ADR-0049 is not `accepted`**.
- Folding the capability into the existing catalog digest would be the only way to expose it, and the implementation cannot keep the digest's inputs unchanged. The agent reports the digest conflict rather than invalidating stored model preferences.
- The published contract's shape cannot be parsed defensively — that is, a malformed contract cannot be made to yield `Unknown` per dimension without failing the whole catalog probe. The agent reports that rather than making a malformed provider response break discovery.
- The overlay would need to be persisted for the milestone to be meaningful. Persistence is explicitly deferred to a separate decision; the agent records the limitation instead.
- **The overlay's source would require a new provider endpoint.** The only capability shape upstream exposes at runtime rides the integrations-profile route, which CodeGG does not call. Adding a provider surface is a different roadmap's business; the agent records the gap and the route in the closure record instead of adding an endpoint call.
- The overlay would need to **widen** the baseline to be useful. That is forbidden by ADR-0049; the agent records the gap and leaves the baseline standing.
- The always-reasons property would need to come from the provider's advertisement rather than the compiled baseline. The baseline owns it.
- Direct provider connections would be required to gain an overlay, or an EggPool process would become a runtime requirement.
- Repository evidence contradicts a canonical invariant in §4.
- The work would expand into the frontend option surface, the capability record, the vocabulary, or the wire-intent fidelity — all of which are other milestones.

## 15. Closure evidence required

The closure record (`plans/closure/provider-reasoning-control/005-status.md`) must contain:

- the requirement-to-evidence matrix covering all eleven acceptance criteria;
- the catalog-digest stability result, quoted — the same catalog content producing a byte-identical digest before and after the change;
- the capability-revision sensitivity and stability results, quoted;
- the overlay's discard results for the label, bound, and always-reasons cases, quoted;
- the malformed- and oversized-overlay results, quoted, including that the catalog probe still succeeds;
- the atomic-refresh result, quoted, showing a turn never observes a partial overlay;
- the static guard's pass result on the tree **and** its rejection of an injected baseline-writing path, plus its `--self-test` result;
- the `scripts/verify.sh quick` result, the Clippy and `cargo fmt` results, and the `cargo test -p codegg-providers -p codegg-core` result;
- the project-catalog invariants guard's result and an explicit statement that `STORAGE_LAYOUT_VERSION` did not change;
- an explicit statement that direct provider connections are unaffected and that no EggPool process is required;
- the documentation changes made, listed by file;
- an explicit statement that the `eggpool-wire` pin, the model catalog digest's inputs, the provider-to-surface mapping, and the protocol shape did not change.

A closure that shows only compilation, or that reports the overlay without showing the digest is unchanged, is not a closure.

## 16. Handoff notes

- **Do not change what the catalog digest hashes.** `digest_models` hashes `id` and `name` and its doc comment is explicit about why. Folding the capability in changes every deployment's catalog revision once, which can invalidate stored last-used-model preferences (`src/core/daemon_sessions.rs`) and split the revision-scoped `provider_connection_models` rows. Use a separate capability revision.
- **The probe summary is a "bounded, stable projection"** — the word *stable* is load-bearing. The capability is a different kind of fact from a model's identity, which is why it gets a separate revision rather than a place in the existing digest.
- **Parse defensively or not at all.** A malformed provider response must not break catalog discovery. Per-dimension `Unknown` is the right failure, and the whole probe must still succeed.
- **The overlay can only narrow.** If an implementation cannot express "narrow" without a way to widen, the narrowing rule is not implemented. Prove it with the injected hostile overlay before the guard.
- **The always-reasons property comes from the compiled baseline**, never from a provider's advertisement. A provider's self-description is not an authority over its own behaviour, and a proxy's advertisement is not even the upstream provider's.
- **No persistence.** The overlay is re-derived on the existing refresh cadence and is absent after a restart until the next refresh. That is intended here; persistence is a separate decision.
- **Do not add a new discovery cadence** and do not add a live-network CI test. The repository's verification policy forbids both.
- **Direct provider connections must be provably unaffected.** That is the case most users are in, and a milestone that only helps EggPool-backed connections is infrastructure for a narrow slice.
