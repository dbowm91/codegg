# ADR-0049: Per-model reasoning capability authority and admission policy

Status: accepted

Date: 2026-10-21

Accepted: 2026-10-21, by the project maintainer, selecting Option B: layered
authority (adapter baseline, narrowing-only discovery overlay, local typed
admission policy). Accepted as written, without amendment, and authorized for
implementation across the `provider-reasoning-control` roadmap milestones.

Renumbering note (2026-10-22, merge reconciliation — the decision is unchanged):
this record was **ADR-0045** on the `provider-reasoning-control` branch.
`main` had already published ADR-0045 (the command syntax and execution
authority decision), so the branch yields the number to `main` and this record
moves to **ADR-0049**. Main wins the number. The title, the decision,
`Status: accepted`, the acceptance date and the accepting authority are
unchanged — only the identifier and the file name moved. The same collision
also moved the open-vocabulary record from ADR-0044 to **ADR-0048**.

Amendment note (2026-10-21, post-acceptance — the decision is unchanged): a
verification of the pinned upstream established that the
`eggpool.capabilities.thinking.control_contract.accepted_efforts` JSON path named
in §Context is a **design document, not an implemented wire contract**. The pinned
`/v1/models` handler emits a four-key model object (`id`, `object`, `owned_by`,
`name`) with no capability block at all, and `accepted_efforts` appears in no
available EggPool revision's runtime code — only in that revision's `docs/` and
`plans/` prose. The decision's authority order (operator override ▸ adapter
baseline ▸ discovery overlay ▸ unknown), its narrowing-only rule, its `Unknown`
semantics, its always-reasons treatment, and its local admission policy all stand
unchanged. What the verification changes is the overlay's **source**: milestone 005
implements the CodeGG-side narrowing machinery against the capability shape
upstream actually exposes, and records explicitly that the overlay has **no live
source** at this baseline. See
`plans/implementation/provider-reasoning-control/005-discovery-overlay-from-catalog-metadata.md`.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — CodeGG resolves provider-facing request policy; EggPool routing/quota/health remain outside CodeGG.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — capability-negotiated protocol; unknown optional fields tolerated; security-sensitive capability absence fails closed.
- `plans/001-terminology-and-domain-model.md` — provider/model identity; the model-adapter registry's ownership of model-specific policy.

Affected subsystem roadmaps:

- `plans/subsystems/provider-reasoning-control-roadmap.md`

Related architecture documents:

- `architecture/provider.md` (§"Extended-thinking control (`/reasoning`)", §"Provider capability model")
- `architecture/config.md`

Related ADRs:

- `plans/adrs/ADR-0048-open-provider-scoped-reasoning-control-vocabulary.md` — decides the open vocabulary and the faithful wire intent. This record decides **where the per-model facts come from** and **how they are applied at admission**.

## Context

ADR-0048 makes the reasoning vocabulary open and provider-scoped. An open vocabulary creates a new obligation: CodeGG must know, for the model a turn will actually use, **which reasoning dimensions that model accepts** and **whether it can reason at all**. Without that knowledge, CodeGG can only forward whatever the user selected and let the provider reject it — which is precisely the failure mode the decision exists to remove.

At the repository baseline `c93211542` CodeGG has almost none of this knowledge.

**There is no per-model reasoning capability anywhere.** The only reasoning-related provider fact is `ProviderCapabilities::strips_reasoning_effort` (`crates/codegg-providers/src/provider_core.rs`), a **per-provider** boolean whose documented meaning is narrow: it means the provider **deletes the `reasoning_effort` field** from the request. It explicitly does **not** mean "this provider does not support reasoning", and it cannot express "this model always reasons" or "this model accepts only `low`, `medium`, `high`, `xhigh`, `max`". Its declared provider list is also narrower than reality: the generic OpenAI-compatible adapter omits `reasoning_effort` from the encoded body — an omission the adapter annotates as *intentional*, per M001's provider matrix — yet that adapter family (mistral, groq, xai, deepinfra, together, cohere, local, and custom OpenAI-compatible connections) is not in the list, so the frontend's existing "drops this setting" warning never fires for it. The gap is in the **warning's coverage**, not in the adapter's behaviour.

**The model-adapter registry is the existing home for model-specific policy, and it already carries reasoning defaults.** The declarative adapter assets under `crates/codegg-core/assets/model-adapters/` compile into the binary through `crates/codegg-core/build.rs`, and the resolved profile already carries `default_reasoning_effort` and `default_thinking_budget`. The registry's own invariant is that provider modules must not independently re-resolve model names into a second policy table — that is precisely the authority this decision needs. What it does not yet carry is a *capability* record: what a model accepts, whether it can be turned off, and what its bounds are.

**Discovery already sees the answer and throws it away.** For an EggPool-backed connection, the provider's `/v1/models` response publishes a thinking-control contract (EggPool's `eggpool.capabilities.thinking.control_contract.accepted_efforts`, with independent toggle/effort/budget dimensions and budget bounds). CodeGG's catalog probe parses that response and keeps only `id` and `name` — the capability block is dropped at the parse boundary, before it can inform anything.

**The shared kernel already provides the policy machinery.** `eggpool-wire`'s `adaptation` module is public in the pinned revision and provides `NeutralThinkingCapability` (independent `status`, `toggle`, `effort`, `budget` dimensions, each `Supported | Unsupported | Unknown | Mixed | Conflicting`), `ReasoningCapabilityPolicy` (a disposition per capability class), and `CapabilityDisposition` (`Reject | WarnDrop | AllowWithWarning | Allow`). Its default policy is fail-closed: unsupported → `Reject`, unknown → `Reject`, mixed → `WarnDrop`. The kernel's own documentation states the principle CodeGG needs: *"Capability outcomes are caller-owned facts, never inferred by codecs."* The kernel supplies the decision function; the facts are CodeGG's to own.

**Always-on models are a first-class case, not an edge case.** MiniMax's published contract for `MiniMax-M3.1-Flash` is that the model always reasons: `thinking.type = "disabled"` is rejected with an explicit *"requires adaptive thinking"* error, and `reasoning_effort = "none"` is rejected with the same error. A model whose reasoning cannot be disabled needs a capability record that says so, or the control will keep offering an "off" that the provider refuses.

## Decision drivers

- The capability facts must be **owned by CodeGG**. The kernel explicitly refuses to infer them, and the provider-wire-kernel roadmap forbids moving provider/routing authority out of CodeGG.
- A fact that changes per deployment must not be compiled into the binary as if it were universal, and a fact that is universal must not be re-discovered on every refresh.
- Discovery data is **untrusted**: it arrives from a provider endpoint that may be misconfigured, stale, or hostile.
- **Unknown is not unsupported.** A missing capability record must not be read as "this model cannot reason", and must not be read as "this model accepts everything" without saying so.
- No fabricated values: no effort-to-budget conversion, no invented levels, no coercion of an unsupported label into a supported one.
- The policy must be **local and typed**, so an unsupported selection is refused at admission with a truthful message instead of producing a provider 400 that the user reads as a generic failure.
- The capability must be expressible **without a storage migration**, so the honest-"off" fix is not gated on a schema change.

## Considered options

### Option A — Adapter-declared capability only

Extend the model-adapter assets with the capability record (accepted effort labels, whether reasoning can be disabled, budget bounds) and resolve it purely from the resolved model profile.

Benefits: one authority, consistent with the existing adapter registry and its "no second policy table" invariant; compiled into the binary, so it works for every connection including direct providers with no catalog; no storage change; fully deterministic and testable.

Costs and failure modes: the record is per **family**, resolved by provider id and model pattern, so a per-model or per-deployment difference cannot be expressed; a vendor that changes its accepted labels silently invalidates a compiled record; and there is no path for a value that is genuinely discoverable (an operator's proxy advertising a narrower set) to narrow CodeGG's offer.

### Option B — Layered authority: adapter baseline, discovery overlay, local policy

Keep the adapter-declared record as the **baseline and upper bound**. Allow a **discovery overlay** from provider catalog metadata to *narrow* that baseline — never to widen it — where the provider publishes capability data. Resolve the effective capability per turn, and apply a **local, typed policy** at admission using the kernel's capability machinery.

Benefits: the adapter record is the compiled, deterministic floor and can never be exceeded by untrusted input; the discovery overlay adds per-deployment accuracy where it exists and is simply absent where it does not; the effective capability is resolved per turn against the model the turn actually uses, which is what the control needs to be truthful about; and the local policy turns an unsupported selection into a typed admission refusal rather than a provider-side 400.

Costs and failure modes: two sources must be reconciled, and the reconciliation order is a correctness-critical detail that must be specified and tested; the overlay is only as good as the catalog refresh cadence, so a stale overlay could offer an option a model no longer accepts; and a discovery overlay that disagrees with the adapter baseline needs a defined, non-silent resolution.

### Option C — Require EggPool's runtime contract as the authority

Make the EggPool provider's published thinking-control contract the single source of truth for reasoning capability.

Benefits: EggPool's contract is already the canonical provider-bound model with independent toggle/effort/budget dimensions, an accepted-effort list, and budget bounds; CodeGG would inherit a maintained vocabulary rather than re-derive it.

Costs and failure modes: it violates the closed provider-wire-kernel roadmap's invariant that CodeGG operates against direct providers without an EggPool process, and the majority of connections are direct; the capability would be unavailable exactly where the control is most needed; and it conflates a *proxy's* view of a model with the model's own contract, which differs across deployments of the same upstream model.

### Option D — Persist a per-model capability table as the primary authority

Add a SQLite table of per-model reasoning capabilities, populate it from discovery, and drive both the UI and admission policy from it.

Benefits: per-model granularity, which no other option offers natively; queryable and inspectable; survives restarts without re-discovery.

Costs and failure modes: it requires a storage-layout migration (`STORAGE_LAYOUT_VERSION` and a new migration, with the contiguous-chain invariant enforced by the project-catalog guard) before the capability can even be expressed on the wire — the dependency order is inverted; it stores a fact that is partly derived and partly stale, which invites drift; and it does not help the direct-provider connections that have no catalog to populate it from, so the adapter baseline is still required anyway.

## Decision

**Option B is selected.** The per-model reasoning capability is a **layered, ordered, CodeGG-owned** fact, and an unsupported selection is refused **locally and typed** at turn admission.

The decision has five parts.

1. **Authority order.** The effective capability for a turn is resolved against the model that turn will actually use, from these sources in order, each able to narrow but never widen the previous:
   1. an explicit operator/user configuration override (highest precedence — an operator who knows their deployment's limits is authoritative);
   2. the **adapter-declared baseline** from the resolved model profile — the compiled, deterministic record of what the model family accepts;
   3. a **discovery overlay** from provider catalog metadata, where the provider publishes it (EggPool-compatible connections publish a thinking-control contract with accepted effort labels and budget bounds), applied only to narrow the baseline;
   4. **unknown** — the absence of a record.

   A discovery overlay may remove an option the baseline offers. It may never add one, introduce a label or a bound the adapter did not declare, or change the always-on property.

2. **The capability record's shape.** It carries, independently:
   - the accepted **effort labels** (an open, provider-scoped string set — not a fixed enumeration);
   - whether reasoning can be **disabled** at all (distinct from "no effort control available" and from "always reasons");
   - whether a **fixed budget** is accepted, and its **minimum and maximum**;
   - an **always-reasons** property for models whose reasoning cannot be turned off.

   These are independent dimensions because providers genuinely differ along them, and a single boolean cannot express "no effort control but a binary on/off switch" or "effort control but no disable".

3. **Unknown is a distinct state.** The capability carries an explicit status per dimension, using the kernel's own vocabulary (`Supported | Unsupported | Unknown | Mixed | Conflicting`). Unknown is never collapsed into Unsupported (which would refuse a working control) nor into Supported (which would send an unsupported value). When the status is Unknown, the control proceeds using the adapter baseline's own dimensions and, for dimensions the baseline also does not declare, the selection is **rejected at admission with a typed refusal** rather than forwarded optimistically.

4. **Local admission policy.** The kernel's `reasoning_capability_notices_neutral` and `ReasoningCapabilityPolicy` are the decision function. CodeGG supplies the facts; the kernel's default fail-closed policy is the baseline disposition set. The permitted dispositions are `Reject`, `WarnDrop`, `AllowWithWarning`, and `Allow`. A selection the effective capability does not support is **refused at admission with a typed error naming the model and the dimension** — not forwarded to produce a provider 400, and not silently dropped. The refusal is a turn-admission failure the user can act on, not a provider failure.

5. **No storage migration is required.** The adapter baseline is compiled in. The discovery overlay is held in the existing catalog/discovery layer and is refreshed on the existing catalog-refresh cadence. **A new SQLite column or table is out of scope for the core capability** and requires a separate recorded decision with evidence that persistence is necessary (a later milestone may propose it; it is not a prerequisite for any capability milestone).

The always-reasons property is a **first-class capability value**, not a special case. A model that always reasons is offered the depth ladder it accepts and is **not** offered a disable, and the UI states that the model's reasoning cannot be turned off.

## Consequences

### Positive

- An unsupported selection is refused locally, with a message naming the model and the dimension, instead of surfacing as an opaque provider 400.
- A model whose reasoning cannot be disabled is described truthfully, and the control stops offering a switch that the provider will reject.
- A model's accepted labels are available per deployment where a provider publishes them, without a CodeGG release.
- The adapter registry remains the single model-policy authority, consistent with its existing invariant.
- The capability is expressible without a storage migration, so the honest-control work is not gated on schema change.
- The always-on case stops depending on a `thinking_budget == Some(0)` escape hatch that the kernel rejects.

### Negative

- Two sources must be reconciled on every turn's capability resolution, and the reconciliation order is correctness-critical and must be tested rather than assumed.
- The discovery overlay can be stale between refreshes, so a recently-changed provider label set may briefly be offered. The consequence is bounded — an invalid value is rejected by the provider and reported as a provider error — but it is a real behaviour.
- A model family whose labels vary per deployment is described accurately only when the deployment publishes capability data; otherwise the family baseline applies.
- The capability record is a second thing to maintain alongside the adapter's existing reasoning defaults, and the two must not disagree silently. The precedence rule is defined above for exactly this reason.

### Neutral or deferred

- The `eggpool-wire` pin is unchanged. The kernel's capability machinery is used through its already-public `adaptation` module; no new dependency and no kernel change are proposed.
- Whether the discovery overlay is persisted in SQLite is deferred to a later milestone with its own evidence requirement. The core capability does not depend on it.
- Extending the kernel's capability model to carry an *accepted-values list* (the kernel's own neutral capability type is status-only) is not proposed. CodeGG carries the accepted-label set in its own capability record and uses the kernel for the per-dimension status decision, which is the division the kernel's documentation describes.
- The existing `ProviderCapabilities::strips_reasoning_effort` flag and its provider list are **not** removed by this decision. Correcting that list's coverage is a defect fix inside a milestone, and the flag's meaning stays narrow.

## Compatibility and migration

- **Protocol.** The effective capability is exposed to frontends as a new, additive, optional capability payload. New clients must tolerate its absence (an old daemon returns no capability, and the frontend falls back to offering the configured default without claiming the model's full option set). A new client against an old daemon must not present a capability it did not receive. The payload carries the same `#[serde(default)]` tolerance the protocol already requires of optional fields.
- **Storage.** None for the core capability. No migration, no `STORAGE_LAYOUT_VERSION` change.
- **Configuration.** An operator override for the capability is additive and optional. It must be able to express "this deployment's model accepts only these labels" and "this model always reasons", and it must be validated as a bounded, typed shape at load rather than trusted verbatim.
- **Existing adapters.** The adapter baseline is populated for the model families whose contracts are known and published. A family with no baseline record is `Unknown` per dimension, not `Unsupported`, so the control degrades to the configured default rather than becoming unusable.
- **Provider behaviour.** A selection that the effective capability says is supported but the provider rejects anyway is a provider-side defect and is reported as such; it is not masked by CodeGG's local check.

## Security and reliability implications

- **Discovery data is untrusted.** A capability overlay originates from a provider endpoint. It is parsed into a bounded, typed shape — a deduplicated, length-capped label list; a bounded budget range; a boolean — before any use, and malformed input degrades to `Unknown` rather than propagating.
- **Untrusted data may only narrow.** Because the overlay can never widen the adapter baseline, a hostile catalog cannot cause CodeGG to send a label or a budget the model family does not accept. This is the single most important security property of the layering, and it must be enforced by a test that injects an overlay claiming a label the baseline does not declare.
- **No capability may be used to widen authority.** A capability record describes a model's reasoning options. It carries no authorization meaning and must never be derived from, or grant, any permission, tool, path, or credential scope.
- **Secret handling.** Labels and budgets are not secrets. A provider catalog's free-text fields are not copied into the capability record beyond the bounded typed shape, so a provider cannot smuggle arbitrary text into a UI surface or a log.
- **Fail-closed on absence.** When a dimension's status is `Unknown` and no baseline declares it, the selection is refused at admission. The failure is typed and actionable, not an unbounded wait and not a silent degradation to a wrong value.
- **Contention and refresh.** The overlay is refreshed on the existing catalog-refresh cadence and replaced atomically; a turn never observes a partially updated overlay. Refresh does not mutate in-flight turn state, so a capability change mid-session affects the next turn and does not mutate a running one.
- **Restart and recovery.** The adapter baseline is compiled in, so it survives restart without state. The overlay is re-derived by the existing refresh path. No new durable state means no new recovery policy and no new failure mode at restart.
- **Denial of service.** A catalog advertising an enormous label list is bounded before it reaches a UI or a request, so a hostile endpoint cannot inflate the frontend's option surface.

## Verification

The decision is proven by:

- a test asserting the authority order, including that a discovery overlay claiming a label the adapter baseline does not declare is **rejected** rather than accepted;
- a test asserting that `Unknown` is preserved as `Unknown` and that a selection on an `Unknown` dimension with no baseline is refused at admission with a typed error naming the model and the dimension;
- a test asserting that a model declared always-reasons is offered no disable option, and that an attempted disable is refused locally rather than forwarded;
- a test asserting that the always-on model family's depth labels survive to the encoded body verbatim;
- a test asserting that a malformed or oversized discovery overlay degrades to `Unknown` and does not widen the baseline;
- a test asserting that an unsupported label produces a local admission refusal and that **no provider request is dispatched**;
- a static guard asserting that the capability record's discovery overlay path cannot write to the adapter baseline.

## Supersession

None. This record is superseded only by a later accepted ADR that changes the capability authority model.
