# Provider Reasoning Control Milestone 003 — Per-model capability record and local admission policy

Status: active (M002 closed; ADR-0049 accepted 2026-10-21; implementation in progress)

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`)

Source roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-3--per-model-reasoning-capability-record-and-local-admission-policy`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — provider-facing request policy is CodeGG-owned; EggPool routing authority stays outside CodeGG.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — capability-negotiated protocol; security-sensitive capability absence fails closed.

Applicable ADRs:

- `plans/adrs/ADR-0049-per-model-reasoning-capability-authority.md` — the layered authority and the admission policy.
- `plans/adrs/ADR-0048-open-provider-scoped-reasoning-control-vocabulary.md` — the open vocabulary this capability constrains.

Primary class: capability

## 1. Objective

Give the daemon a per-model statement of **which reasoning the model accepts** and **whether its reasoning can be turned off**, and use it to **refuse an unsupported selection locally and typed** at turn admission — before any provider request is dispatched — instead of letting the provider reject it with an opaque 400.

## 2. Why this milestone is ready

- **Hard dependency:** M002 closed. M002 makes the label open and gives the daemon a durable selection; this milestone validates that selection against a model.
- **ADRs:** ADR-0049 accepted (its §1 authority order, §2 record shape, §3 unknown semantics, §4 admission policy, §5 no-migration decision). ADR-0048 accepted, since this milestone constrains the vocabulary it defines.
- **Interface dependency — the kernel's capability machinery, already public in the pinned revision.** `eggpool-wire`'s `adaptation` module provides `NeutralThinkingCapability` (independent `status`, `toggle`, `effort`, `budget` dimensions, each `Supported | Unsupported | Unknown | Mixed | Conflicting`), `ReasoningCapabilityPolicy` (a `CapabilityDisposition` per capability class), `CapabilityDisposition` (`Reject | WarnDrop | AllowWithWarning | Allow`), and the pure decision function `reasoning_capability_notices_neutral`. The kernel's default policy is fail-closed: unsupported → `Reject`, unknown → `Reject`, mixed → `WarnDrop`. **The kernel supplies the decision function; CodeGG supplies the facts.**
- **Interface dependency — the model-adapter registry, already the single compiled model-policy authority.** `crates/codegg-core/src/model_profile/adapter.rs` defines `AdapterProfile` with `#[serde(deny_unknown_fields)]`, and the resolved profile already carries `default_reasoning_effort` and `default_thinking_budget`. The capability record is an extension of that record, not a second table.
- **Predecessor roadmaps:** the closed provider-wire-kernel consolidation roadmap (one CodeGG authority for model policy; provider modules must not re-resolve model names into a second table) and the closed model-adaptation roadmap (the declarative adapter registry). Both are preserved.

## 3. Current implementation evidence

Repository baseline `c93211542`.

**There is no per-model reasoning capability anywhere.** The only reasoning-related provider fact is `ProviderCapabilities::strips_reasoning_effort` in `crates/codegg-providers/src/provider_core.rs`, a **per-provider** boolean whose documented meaning is narrow — the provider **deletes the `reasoning_effort` field** — and whose own doc comment says so explicitly: *"This is deliberately NOT the inverse of 'supports reasoning': most providers simply pass the field through without declaring anything, so `false` means 'does not strip', never 'cannot reason'."* Its declared provider list is narrower than reality: the generic OpenAI-compatible adapter omits `reasoning_effort` from the encoded body (`crates/codegg-providers/src/openai_compatible.rs`), yet that adapter family is not in the `for_provider` list, so the frontend's existing "drops this setting" warning never fires for it.

**The generic compatible contract's stripping is intentional, and must stay.** The omission is annotated in the adapter itself: *"The generic compatible contract intentionally did not forward generation controls; preserve that per M001's provider matrix."* So the defect here is **the warning's coverage, not the adapter's behaviour**. The fix is to the capability flag's provider list, so the TUI tells the truth about a control that does nothing on those connections. Changing what the generic adapter forwards is a different decision and is out of scope — and would be a behaviour change to a contract that was deliberately narrowed.

**The adapter registry is the right home and already carries reasoning defaults.** `AdapterProfile` (`crates/codegg-core/src/model_profile/adapter.rs`):

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProfile {
    /* … */
    pub default_reasoning_effort: Option<String>,
    pub default_thinking_budget: Option<usize>,
    pub max_parallel_tools: Option<usize>,
}
```

The declarative assets live in `crates/codegg-core/assets/model-adapters/*.toml` and compile into the binary through `crates/codegg-core/build.rs`. The resolved profile (`crates/codegg-core/src/model_profile/types.rs`) carries `default_reasoning_effort: Option<String>` and `default_thinking_budget: Option<usize>` through to `ResolvedModelProfile`.

**Adding a profile field is a five-place change, and the build enforces it.** `deny_unknown_fields` is set on both the runtime `AdapterProfile` and the build-time `Profile` mirror in `crates/codegg-core/build.rs`, so a field present in one and absent from the other is a hard build error — good, but it means the mirror must be updated in the same change. The five places are: the runtime `AdapterProfile`; the `build.rs` `Profile` mirror; the adapter merge/inheritance list in `adapter.rs`; `ResolvedModelProfile` in `types.rs`; and `ModelProfileConfig` in `crates/codegg-config/src/schema.rs`.

**The build.rs mirror is already drifting.** The runtime `AdapterProfile` has `orchestration_tier` (and the runtime's `merge_adapter` has an inherit list that must match); the `build.rs` `Profile` mirror has **no** `orchestration_tier` field. No shipped TOML sets it, so it is latent — but it demonstrates the exact failure mode a five-place change must avoid: a TOML author who tries it gets a `deny_unknown_fields` error naming the build mirror rather than a statement that the field is unsupported. This milestone's capability field must be added to **both** sides in the same change, and the drift should be reported.

**The capability dimensions are genuinely independent.** Providers differ along several axes at once: a model may accept a depth ladder but not a disable; a model may accept a fixed budget but no ladder; a model may always reason and accept no disable at all. A single boolean cannot express any of these, which is why the record carries independent dimensions rather than one flag.

**MiniMax is the worked example that proves the need.** MiniMax's published contract for `MiniMax-M3.1-Flash` accepts `reasoning_effort` values `low`, `medium`, `high`, `xhigh`, `max`; rejects `none` with an explicit *"requires adaptive thinking"* error; and rejects `thinking.type = "disabled"` with the same error — the model always reasons. CodeGG routes MiniMax through the **Anthropic-compatible** surface (`crates/codegg-providers/src/additional.rs` constructs an `AnthropicProvider` for MiniMax), and `reasoning_effort` is an **OpenAI-surface** parameter — so on the Anthropic Messages surface an effort selection produces **no reasoning field at all**, while a budget selection is the only dimension that surface expresses. The depth ladder is therefore currently unreachable for MiniMax on the surface CodeGG uses, and "off" is a claim the provider will reject with a 400.

**The kernel's capability types are status-only.** `NeutralThinkingCapability` carries a *status* per dimension, not an accepted-value list. The accepted-label set is CodeGG's to carry, in its own record; the kernel supplies the per-dimension decision. This is the division the kernel's own documentation describes: *"Capability outcomes are caller-owned facts, never inferred by codecs."*

**There is no storage change here.** ADR-0049 §5 decides that the capability baseline is compiled and the overlay is held on the existing discovery layer, so this milestone adds no column, no table, and no migration.

## 4. Invariants that must not regress

- **One model-policy authority.** Provider modules must not independently re-resolve model names into a second reasoning-capability table. The adapter registry remains the single compiled authority.
- **Provider-facing request policy stays in CodeGG.** The kernel's capability machinery is used; the kernel's runtime is not required and CodeGG does not run an EggPool process.
- **No fabrication.** CodeGG never converts a depth label into a token budget, never invents a label, and never coerces an unsupported label into a supported one.
- **Untrusted data may only narrow.** A discovery-supplied capability overlay may remove an option the adapter baseline offers; it may never add one, introduce a label or bound the adapter did not declare, or change the always-reasons property.
- **Unknown is neither unsupported nor supported.** An absent capability record is preserved as `Unknown`.
- **The capability grants nothing.** It carries no authorization meaning and is never derived from, or grants, any permission, tool, path, or credential scope.
- **The kernel decides representability, not policy.** The kernel supplies the per-surface encoding and the per-dimension status decision; CodeGG supplies the facts and the operator policy.
- **The existing `ProviderCapabilities::strips_reasoning_effort` flag keeps its narrow meaning.** It means "the provider deletes the field". It is not the inverse of "supports reasoning" and is not replaced by this milestone's record. Its **provider list coverage is corrected** as a defect fix inside this milestone, because the generic OpenAI-compatible adapter family omits the field too and is currently unflagged. **The adapters' behaviour is not changed** — the generic compatible contract's omission is intentional per M001's provider matrix.
- The existing agent-layer precedence order is unchanged.

## 5. Scope

### In scope

- The capability record's shape and its compiled adapter baseline.
- The resolution order: operator override → adapter baseline → discovery overlay → unknown.
- The kernel-policy application at turn admission and the typed refusal.
- The always-reasons property as a first-class capability value.
- The correction of the `strips_reasoning_effort` provider list's coverage.
- Populating the adapter baseline for the model families whose published contracts are known.

### Explicitly out of scope

- **The discovery overlay.** M005's subject. This milestone must work with the overlay **absent** — that is the case for every direct provider connection.
- **The frontend option surface.** M004's subject. This milestone makes the capability available to the daemon; M004 presents it.
- **The protocol payload that exposes the capability to frontends.** M004's subject.
- **Any change to which surface a provider uses.** MiniMax stays on the Anthropic-compatible surface; the provider-to-surface mapping is another roadmap's business. This milestone makes the consequence *visible* rather than changing it.
- **Any storage change.**
- **Any change to the `eggpool-wire` pin.**
- **Effort-to-budget conversion in any direction.**
- **A general model-capability framework.** The record is scoped to reasoning.

## 6. Required production changes

### Core/domain

- `crates/codegg-core/src/model_profile/adapter.rs` — `AdapterProfile` gains the capability record. The record's shape carries, **independently**:
  - the accepted **effort labels** — an open, provider-scoped string set, not a fixed enumeration;
  - whether reasoning can be **disabled**;
  - whether a **fixed budget** is accepted, and its **minimum and maximum**;
  - whether the model **always reasons**.

  Each dimension carries an explicit status using the kernel's vocabulary (`Supported | Unsupported | Unknown | Mixed | Conflicting`), so that an undeclared dimension is `Unknown` rather than `Unsupported`.
- `crates/codegg-core/src/model_profile/types.rs` — `ResolvedModelProfile` carries the resolved record.
- `crates/codegg-core/src/model_profile/adapter.rs` — the merge/inheritance list is extended so the capability record participates in adapter merging exactly as the other profile fields do. **The merge rule for the record is the resolution order of ADR-0049 §1**: the more specific adapter's *narrowing* wins, and an overlay may only narrow.
- `crates/codegg-core/build.rs` — the `Profile` mirror gains the same field in the same change. The `orchestration_tier` drift is reported and, if it is a one-line safe fix, corrected in the same change with a test.
- `crates/codegg-config/src/schema.rs` — `ModelProfileConfig` gains the same record so an operator can narrow or override it from configuration.
- `crates/codegg-core/assets/model-adapters/*.toml` — the shipped adapters are populated for the model families whose published contracts are known, including the always-reasons property for the families that publish it.

### Storage and migrations

None. No column, no table, no migration, and `STORAGE_LAYOUT_VERSION` is unchanged.

### Protocol and DTOs

None in this milestone. The capability is daemon-internal here; M004 adds the optional frontend payload.

### Runtime and concurrency

- A **single capability resolution function**, invoked once per turn admission, which produces the effective capability for the model the turn will use, in the order: operator/config override → adapter baseline → discovery overlay (absent in this milestone) → unknown.
- The effective capability is applied to the resolved selection through the kernel's `reasoning_capability_notices_neutral` and `ReasoningCapabilityPolicy`, producing one of `Reject`, `WarnDrop`, `AllowWithWarning`, `Allow`.
- A `Reject` produces a **typed admission refusal** naming the model and the dimension, and **no provider request is dispatched**.
- A `WarnDrop` drops the unsupported dimension and proceeds with a recorded notice; the notice must say what was dropped, because a silently dropped dimension is the defect this roadmap exists to remove.
- The resolution is per turn and read-only: it never mutates the adapter baseline, the configuration, or any in-flight turn state.

### Frontend or operator surface

None directly in this milestone. The refusal is surfaced to the user through the turn-admission error path, which already exists.

### Security and authorization

- The capability record carries **no authorization meaning** and grants nothing. It is never derived from, or used to grant, a permission, tool, path, or credential scope.
- The capability's accepted-label set is **bounded** — a length cap on the list and on each label — so a hostile or corrupt asset cannot inflate a request or a UI surface.
- The operator/config override is validated as a bounded, typed shape at load; a malformed override is rejected at load rather than trusted.
- The capability is derived from CodeGG's own compiled assets and configuration, not from a provider response, in this milestone. The untrusted-overlay property is M005's subject, but the resolution function is written so that an overlay can only narrow — proven by a test even before an overlay exists.

### Documentation and static guards

- `architecture/provider.md` — the capability model section records the new record's shape, the resolution order, and the admission policy; the reasoning-control section records which dimensions each surface can express and that an unsupported selection is refused locally.
- `architecture/config.md` — the model-profile section records the capability override.
- `architecture/agent.md` — the request-preparation section records the capability check's position in the chain.
- A static guard is added asserting that the adapter profile and its `build.rs` mirror declare **the same field set**, so a one-sided change is caught at build-check time rather than by a confusing `deny_unknown_fields` error. It is registered in `scripts/verify.sh quick` **and** in the CI `verify` job, and carries a `--self-test` that injects a one-sided field and proves the guard rejects it.

## 7. Ordered work packages

### Work package A — Define the capability record and add it to the adapter registry

Intent: Give the adapter registry a capability record, and land it in every place that must agree.

Required changes:
- The record type in `codegg-core`, with independent dimensions and per-dimension status.
- The field in `AdapterProfile`, the `build.rs` `Profile` mirror, the merge/inheritance list, `ResolvedModelProfile`, and `ModelProfileConfig` — all in the same change.
- The `build.rs` mirror's existing `orchestration_tier` drift reported, and corrected if it is a safe one-line fix with a test.

Acceptance evidence:
- A build test asserting the runtime profile type and the `build.rs` mirror declare the same field set, so a one-sided change fails the build rather than producing a confusing asset error.
- A test asserting the shipped adapter assets parse and that every capability field they declare is well-formed (bounded label list, consistent dimension statuses, a budget range whose minimum does not exceed its maximum).
- A test asserting an undeclared dimension resolves as `Unknown`, not `Unsupported`.
- A test asserting the merge rule narrows: a more specific adapter's narrowing wins, and a merge can never widen a dimension's accepted set beyond the baseline.

### Work package B — Populate the adapter baseline

Intent: Give the model families whose contracts are published an accurate baseline.

Required changes:
- Populate the capability record in the shipped adapter assets for the families whose published contracts are known, including the always-reasons property for the families that publish it, and the accepted effort labels where the vendor documents them.
- Leave a family with no published contract **unpopulated** — `Unknown`, not `Unsupported`.

Acceptance evidence:
- A test asserting the always-reasons family is recorded as always-reasoning, that its accepted labels are present, and that its disable dimension is not `Supported`.
- A test asserting an unpopulated family resolves as `Unknown` on every dimension, and that a selection on such a family is refused at admission with a typed error rather than forwarded.
- A test asserting the accepted labels of a populated family are exactly the published set, with no invented label and no effort-to-budget conversion.

### Work package C — Resolve the effective capability and apply the policy

Intent: Produce one effective capability per turn, and refuse an unsupported selection locally.

Required changes:
- The resolution function, with the ADR-0049 §1 order.
- The kernel-policy application and the typed refusal.
- The bounded validation of the accepted-label set and of any operator override.

Acceptance evidence:
- A test asserting the resolution order, including that an operator override narrows the baseline and that a configuration override beats the compiled baseline.
- A test asserting an unsupported label is refused at admission with a typed error naming the model and the dimension, and that **no provider request is dispatched**.
- A test asserting `Unknown` is preserved as `Unknown` and that a selection on an `Unknown` dimension with no baseline is refused rather than forwarded.
- A test asserting a `WarnDrop` outcome records a notice naming the dropped dimension.
- A test asserting the resolution never mutates the adapter baseline, the configuration, or in-flight turn state.

### Work package D — Correct the stripping-flag coverage

Intent: Stop the existing warning from missing the providers that actually strip the field.

Required changes:
- Extend `ProviderCapabilities::strips_reasoning_effort`'s provider coverage to the generic OpenAI-compatible adapter family, which omits `reasoning_effort` from the encoded body today.
- **The adapters' behaviour does not change.** The generic compatible contract's omission is intentional per M001's provider matrix and is annotated as such in the adapter. This work package changes only the *capability flag's* coverage, so the TUI warns about a control that genuinely does nothing on those connections.
- Keep the flag's meaning narrow: "this provider deletes the field", **not** "this provider does not support reasoning".

Acceptance evidence:
- A test asserting the flag's coverage matches the adapters that actually omit the field, so the two cannot drift again — and specifically that the generic OpenAI-compatible provider is covered.
- A test asserting the generic compatible adapter still omits `reasoning_effort` (the intentional behaviour is preserved by this milestone, not silently changed).
- A test asserting the flag is still not used as a "supports reasoning" signal anywhere.

### Work package E — Add the mirror guard and reconcile the documentation

Intent: Make the one-sided-change failure mode mechanical, and the contract documented.

Required changes:
- The static guard asserting the runtime profile type and the `build.rs` mirror declare the same field set, with a `--self-test`.
- The documentation updates listed in §6.

Acceptance evidence:
- The guard passes on the tree and rejects an injected one-sided field.
- The documentation names the record's dimensions, the resolution order, and the admission policy, and states that a rejected selection is refused before dispatch.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** The resolution is a pure function over compiled assets, configuration, and (later) the overlay. There is no partial state. A configuration override that fails to load is rejected at load, and the compiled baseline stands.
- **Cancellation.** The capability check happens at admission, before the turn runs. A cancelled turn's refusal is discarded with the turn, exactly as a cancelled turn's other admission failures are.
- **Restart.** The compiled baseline is in the binary, so it survives restart with no state. A configuration override is re-read from configuration on the next turn. No new durable state means no new recovery policy.
- **Duplicate delivery.** A redelivered turn re-resolves the capability and re-applies the policy. The resolution is deterministic for a given model, configuration, and overlay, so the same turn produces the same verdict.
- **Contention.** The resolution is read-only. An overlay refresh (M005) replaces the overlay atomically, so a turn never observes a partial update and never sees a half-narrowed capability.
- **Stale generations.** A turn admitted against a capability snapshot uses that snapshot. A capability change between turns affects the next turn; it never mutates a running one.

## 9. Compatibility and migration

- **Protocol.** Unchanged in this milestone. No request or response shape changes, and no capability is exposed to frontends yet (M004 does that, additively and optionally).
- **Storage.** None. No migration, no `STORAGE_LAYOUT_VERSION` change, no schema change.
- **Configuration.** `ModelProfileConfig` gains an optional capability override. It is additive and absent by default, so existing configuration behaves exactly as it does at the baseline. A malformed override is rejected at load rather than silently ignored, and the rejection is reported.
- **Adapter assets.** The shipped assets are repository files, so populating them is a normal change. An asset whose capability claim is wrong is a repository defect, not a runtime migration.
- **Provider behaviour.** A selection the effective capability says is supported but the provider rejects anyway is a provider-side defect and is reported as such; it is not masked by the local check. A selection the effective capability says is unsupported is refused locally, so the provider should not see it.
- **Build-time.** The `build.rs` mirror change is a build-time concern only; it does not change the runtime asset format beyond the added field.

## 10. Required tests

### Focused unit tests

- The record's per-dimension status defaults to `Unknown` for an undeclared dimension.
- The record's bounds are enforced: an over-long label list, an over-long label, and an inverted budget range are all rejected.
- The merge rule narrows and never widens.
- The operator override narrows the baseline and is validated at load.

### Integration tests

- The resolution order, exercised end to end for a populated family, an unpopulated family, and an always-reasoning family.
- The admission refusal: an unsupported label produces a typed error naming the model and dimension, and the provider is **not** called.
- The `WarnDrop` path recording a notice naming the dropped dimension.
- The `strips_reasoning_effort` coverage matching the adapters that actually omit the field.

### Restart and recovery tests

- The compiled baseline resolves identically after a daemon restart, and a configuration override is re-read.

### Contention and cancellation tests

- A cancelled turn discards its refusal with the turn.
- The resolution never mutates the baseline or an in-flight turn.

### Security and negative tests

- A capability record grants nothing: asserted as a negative over the resolution path and the operation surface.
- A configuration override carrying an over-long or malformed value is rejected at load and does not reach the resolution.
- An injected overlay claiming a label the baseline does not declare is **rejected**, proving the narrowing property before M005 introduces a real overlay.

### Migration and compatibility tests

- `STORAGE_LAYOUT_VERSION` is unchanged and the project-catalog invariants guard still passes.
- The shipped adapter assets all parse, and an asset with a malformed capability record fails the build rather than reaching runtime.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-core model_profile
cargo test -p codegg-core build
cargo test -p codegg-providers provider_core

# the new static guard, including its self-test
python3 scripts/check_model_adapter_profile_mirror.py
python3 scripts/check_model_adapter_profile_mirror.py --self-test

# the guard must be wired into the routine suite
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
cargo test -p codegg-core -p codegg-providers
```

Do not claim commands that were not actually run in the closure record. The repository's existing verification policy applies: no new CI lane, no coverage or benchmark gate, no permanent external service.

## 12. Documentation updates

- `architecture/provider.md` — the capability model section (the record's shape, the resolution order, the admission policy) and the reasoning-control section (which dimensions each surface expresses, and that an unsupported selection is refused locally before dispatch).
- `architecture/config.md` — the model-profile section records the capability override and its validation.
- `architecture/agent.md` — the request-preparation section records the capability check's position in the precedence chain.
- `architecture/storage.md` — no change is required, and the closure must state that none occurred.
- `docs/providers.md` — if it documents the thinking control, reconcile it with the landed contract.

## 13. Acceptance criteria

1. The adapter registry carries a capability record with **independent** effort-label, disable, budget, and always-reasons dimensions, each with an explicit status in the kernel's vocabulary.
2. The record, the `build.rs` mirror, the merge list, `ResolvedModelProfile`, and `ModelProfileConfig` are all updated in one change, and a one-sided change is caught mechanically by a static guard with a self-test.
3. An undeclared dimension resolves as `Unknown`, never as `Unsupported`.
4. The effective capability is resolved per turn, in the order operator override → adapter baseline → discovery overlay → unknown, and the resolution never widens a dimension beyond the baseline.
5. A selection the effective capability does not support is refused at turn admission with a typed error naming the model and the dimension, and **no provider request is dispatched**.
6. A selection on an `Unknown` dimension with no baseline is refused rather than forwarded.
7. The always-reasons property is a first-class capability value, and the recorded family that publishes it is never offered a disable.
8. The accepted labels of a populated family are exactly the published set — no invented label, no effort-to-budget conversion, and no normalization of a vendor label.
9. `ProviderCapabilities::strips_reasoning_effort`'s coverage matches the adapters that actually omit the field, and the flag is still not used as a "supports reasoning" signal.
10. The capability grants no permission, tool, path, or credential scope, and configuration overrides are bounded and validated at load.
11. `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes.
12. The documentation names the record's dimensions, the resolution order, and the admission policy.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **M002 has not closed**, or **ADR-0049 is not `accepted`**.
- The pinned `eggpool-wire` revision does not expose the capability machinery this milestone depends on — specifically `NeutralThinkingCapability`, `ReasoningCapabilityPolicy`, `CapabilityDisposition`, and `reasoning_capability_notices_neutral` — or exposes a different shape. The agent reports the actual pinned shape rather than writing a local reimplementation of the policy.
- The capability record cannot be added to the adapter registry without changing the **meaning** of an existing profile field. The record is additive.
- The five-place change cannot be made consistently. The agent reports which place resists rather than adding a second path around the registry, because a second path is exactly the "second policy table" the closed provider-wire-kernel roadmap forbids.
- Populating a family's capability record would require a claim the vendor has not published. The agent leaves that family `Unknown` and records the gap; it does not infer a contract from a model name.
- The always-reasons property would require changing which surface a provider uses, or would require sending a value the kernel rejects. That is a provider-mapping or wire-kernel matter, not this milestone's.
- The local refusal would need to dispatch a provider request in order to detect the problem. The refusal is local by construction; a design that requires a round trip is out of scope.
- Repository evidence contradicts a canonical invariant in §4.
- The work would expand into the frontend option surface, the discovery overlay, or the wire-intent fidelity — all of which are other milestones.

## 15. Closure evidence required

The closure record (`plans/closure/provider-reasoning-control/003-status.md`) must contain:

- the requirement-to-evidence matrix covering all twelve acceptance criteria;
- the resolution-order results, quoted, for a populated family, an unpopulated family, and an always-reasoning family;
- the admission-refusal result, quoted, together with the evidence that **no provider request was dispatched**;
- the always-reasons result, quoted, showing that the family is recorded as always-reasoning and that no disable is offered;
- the published-label comparison for a populated family, showing the recorded set against the vendor's published set;
- the `Unknown`-preservation result, quoted;
- the narrowing proof, quoted, from the injected-overlay test;
- the stripping-flag coverage result;
- the static guard's pass result on the tree **and** its rejection of an injected one-sided field, plus its `--self-test` result;
- the `scripts/verify.sh quick` result, the Clippy and `cargo fmt` results, and the `cargo test -p codegg-core -p codegg-providers` result;
- the project-catalog invariants guard's result and an explicit statement that `STORAGE_LAYOUT_VERSION` did not change;
- the `build.rs` mirror drift finding and its disposition;
- the documentation changes made, listed by file;
- any finding the milestone surfaced for M004 or M005, recorded rather than absorbed;
- an explicit statement that the `eggpool-wire` pin, the provider-to-surface mapping, the agent-layer precedence order, and the protocol shape did not change.

A closure that shows only compilation, or that asserts a refusal without showing that no provider request was dispatched, is not a closure.

## 16. Handoff notes

- **The five-place change is the mechanical risk of this milestone.** `AdapterProfile`, the `build.rs` `Profile` mirror, the merge/inheritance list, `ResolvedModelProfile`, and `ModelProfileConfig`. Miss the merge list and a more specific adapter silently drops the record. Miss the mirror and the build fails with a `deny_unknown_fields` error that names the *asset*, not the missing mirror field.
- **The mirror already drifts on `orchestration_tier`.** Expect to find it. Correct it if it is a safe one-line fix with a test; report it either way. The new guard is what stops the next drift.
- **`deny_unknown_fields` on both sides is a feature.** Do not relax it to make the change easier.
- **Do not infer a contract from a model name.** A family with no published contract stays `Unknown`. The kernel's own rule is that capability outcomes are caller-owned facts, never inferred.
- **The MiniMax case is a *record* problem here, not a *surface* problem.** MiniMax's depth ladder is unreachable on the Anthropic-compatible surface because `reasoning_effort` is an OpenAI-surface parameter. Recording the capability makes that visible and lets the local policy refuse an unreachable selection with a truthful message. Changing which surface MiniMax uses is a different roadmap's decision — do not make it here.
- **Do not let the local check depend on a provider round trip.** The whole point is to refuse before dispatch.
- **`WarnDrop` is not "silently drop".** If a dimension is dropped, the notice must name it. A silently dropped dimension is the defect this roadmap exists to remove.
- **Leave the discovery overlay out.** M005 introduces it. Write the resolution function so an overlay can only narrow, and prove that with the injected-overlay test — but do not build the overlay.
