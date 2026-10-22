# ADR-0044: Open, provider-scoped reasoning control vocabulary

Status: accepted

Date: 2026-10-21

Accepted: 2026-10-21, by the project maintainer, selecting Option B: open
provider-scoped vocabulary with a faithful wire intent, with the selection owned
by the daemon. Accepted as written, without amendment, and authorized for
implementation across the `provider-reasoning-control` roadmap milestones.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — provider connection ownership; CodeGG resolves provider-facing request policy itself.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — protocol operations are versioned and capability-negotiated; new clients tolerate unknown optional fields; security-sensitive capability absence fails closed.
- `plans/000-long-term-specification.md#14-presence-and-real-time-team-awareness` — provider-private hidden reasoning is not an observable product feature.
- `plans/001-terminology-and-domain-model.md` — provider/model identity and authority boundaries.

Affected subsystem roadmaps:

- `plans/subsystems/provider-reasoning-control-roadmap.md`

Related architecture documents:

- `architecture/provider.md` (§"Extended-thinking control (`/reasoning`)")
- `architecture/agent.md`
- `architecture/config.md`

Predecessor roadmaps (closed):

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md` — established `eggpool-wire` as the neutral wire kernel and the one CodeGG semantic-request → target-grammar encoder.
- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md` — established the declarative model-adapter registry and the provider-neutral reasoning-preservation rules.

## Context

CodeGG exposes an extended-thinking control (`/reasoning`) that a user cycles to pick a reasoning depth. At the repository baseline `c93211542` the control is closed, and the closure happens in three separate places, each of which discards a different part of what the wire kernel can express.

**The vocabulary is closed at the protocol layer.** `ReasoningEffort` (`crates/codegg-protocol/src/core.rs`) is a four-variant enum — `Minimal`, `Low`, `Medium`, `High` — and `ReasoningOverride::Effort { level }` carries it. `ReasoningMode::cycle()` therefore reaches only `default → medium → high → off → default`; `Minimal`, `Low`, and `Budget` are unreachable from the TUI command, and no provider-specific label such as `xhigh` or `max` can be expressed at all. The configuration layer is already open (`default_reasoning_effort: Option<String>` in `crates/codegg-config/src/schema.rs`), so a user can write `xhigh` into config today and it is silently discarded on the surfaces that do not accept effort.

**The intent is collapsed at the wire boundary.** `crates/codegg-providers/src/wire.rs` builds the kernel's `ReasoningIntent` from the two nullable request fields and can produce only two shapes: `requested: Some(true)` with mode `Effort` or `FixedBudget`, or the default `requested: None`. Three capabilities of the kernel's own type are therefore unreachable:

- `ReasoningIntent::disabled()` (`requested: Some(false)`, `mode: Toggle`, `explicit_disable: true`) is never constructed;
- `ReasoningMode::Adaptive` is never constructed;
- `ReasoningMode::Toggle` is never constructed.

The consequence is that the request "turn thinking off" and the request "I did not specify" are **the same request on the wire**. The protocol's own doc comment states the opposite — "`Off` explicitly disables thinking, which is distinct from `None` on the request" (`crates/codegg-protocol/src/core.rs`) — and the agent-layer doc comment repeats the false premise: "the wire encoder treats 'neither field set' as thinking not requested … which is the only way to express an explicit disable" (`src/agent/request_preparation.rs`). The kernel distinguishes these cases; the CodeGG bridge does not.

**The control does not survive the surface it is aimed at.** The kernel's per-surface codecs differ in which dimensions they can express. Effort is representable on the OpenAI Chat and OpenAI Responses surfaces; a fixed budget is representable on the Anthropic Messages and Gemini GenerateContent surfaces; an explicit disable is representable on the Anthropic Messages surface. CodeGG's bridge can only ever emit effort or budget, so:

- on an Anthropic-compatible surface, effort is dropped and no `thinking` field is emitted at all, making `/reasoning medium` and `/reasoning high` **complete no-ops**;
- on an OpenAI-compatible surface, budget is dropped, making `default_thinking_budget` a no-op;
- "off" is never an explicit disable on any surface.

**Models that always reason cannot be represented.** MiniMax's published API contract for `MiniMax-M3.1-Flash` accepts `reasoning_effort` values `low`, `medium`, `high`, `xhigh`, `max`; rejects `none` with an explicit *"requires adaptive thinking"* error; and rejects `thinking.type = "disabled"` with the same error, so the model always reasons. CodeGG has no representation for "this model's reasoning cannot be turned off", so `/reasoning off` on such a model sends nothing, the provider default applies, the model still reasons, and the frontend reports that thinking is off.

**The always-on case is additionally unfixable by the current escape hatch.** The one adapter that pins thinking on (the Laguna `enable_thinking` transform) computes its wire value as `request.thinking_budget != Some(0)`, and the only way to produce `Some(0)` would be a zero token budget — which the kernel's own `validate_reasoning` rejects.

**The user's choice does not survive the daemon.** The per-turn override travels on `CoreRequest::TurnSubmit.reasoning` and is honoured on the ordinary turn path, but three daemon paths reconstruct a turn and drop the field to `None`: the user-prompt-queue delivery path, the idle-continuation path, and the `SessionPromptSubmit` path (whose protocol variant carries no reasoning field at all, so the desktop frontend cannot express one). The native client crate also hardcodes `None`. The selection is also **frontend-local**, so a frontend that reconnects to a running daemon loses it entirely.

**The repository already owns the right kind of store for this.** `runtime_preferences` (migration v58) is a daemon-owned, principal-scoped, durable, revision-gated preference table, with an established store (`RuntimePreferenceStore` in `crates/codegg-core/src/approval.rs`), an established protocol seam (`ApprovalPreferenceGet`, `ApprovalModeSet`, `SandboxProfileSet`, `RuntimePolicySet`, `TaskModelPreferenceSet`, `ExecutionPolicyGet` — principal derived server-side, payloads carrying no identity), and an established additive-migration precedent (`migrate_v62` added two nullable columns to the same table with a plain `ALTER TABLE … ADD COLUMN` that kept every existing row). Its documented precedence is already the shape this control needs: *"explicit turn overrides win, then project ceiling, then persisted, then defaults"*. A per-turn-only design would instead require every turn-reconstructing path to thread the selection individually, which is the exact failure class that produced the three dropped-field paths above.

## Decision drivers

- The user-visible contract must be honest: a control that reports "off" while the model reasons is a defect, not a limitation.
- The wire kernel (`eggpool-wire`) is an immutable-pinned shared dependency whose reasoning type is already strictly more expressive than CodeGG's bridge. Widening the bridge is cheaper than widening the kernel, and the kernel is not CodeGG's to change.
- Provider-specific vocabularies are a moving target. `xhigh`, `max`, `none`, and `adaptive` are already in use or documented by different vendors, and more will appear. A closed enum in CodeGG would have to be reopened for each.
- The control must not fabricate. A level or budget the provider does not accept must not be invented, translated, or approximated on CodeGG's behalf.
- The canonical specification requires capability-negotiated protocol behaviour and fail-closed handling of security-sensitive capability absence. A silently ignored control is the opposite of fail-closed.
- Provider-facing request policy is a CodeGG responsibility under the closed provider-wire-kernel roadmap; the kernel is the encoder, not the policy authority.

## Considered options

### Option A — Keep the closed enum and widen it per provider

Add `Xhigh`/`Max`/`None` variants to `ReasoningEffort` as needed, and special-case the always-on models.

Benefits: smallest diff; the protocol stays a closed, self-describing contract.

Costs and failure modes: every new vendor label requires another CodeGG release; the enum becomes a union of unrelated vendor vocabularies with no owner; the "always reasons" property is still not representable, because it is a per-model fact rather than a per-level fact; and the explicit-disable hole in the bridge stays open, so "off" remains a lie for always-on models. This option optimizes the symptom (the four levels we know about) rather than the cause (a closed vocabulary standing in for an open, provider-scoped one).

### Option B — Open, provider-scoped vocabulary with a faithful wire intent

Replace the closed effort enum with an open, provider-scoped label carried as a string, and rebuild the kernel intent faithfully so that every expressible distinction survives to the encoder: explicit disable, effort label, fixed budget, and an adaptive/toggle dimension.

Benefits: arbitrary provider labels (`xhigh`, `max`, `none`) become expressible without a CodeGG release; the kernel's `Adaptive` and `Toggle` modes become reachable, which is what a binary "thinking on/off" provider contract needs; explicit disable becomes distinguishable from unspecified, so "off" can be honest; and the vocabulary is owned by the provider contract rather than by CodeGG's enum.

Costs and failure modes: the protocol type loses its closed-enum exhaustiveness, so a consumer can no longer assume a known label set; the frontend can no longer derive a cycling order from the type, so the available options must come from the model rather than from a static ladder; and an open vocabulary means the "unknown label" path must be handled explicitly rather than by exhaustive match.

### Option C — Defer the whole surface to EggPool

Declare that reasoning control is an EggPool capability, remove CodeGG's control, and require an EggPool proxy.

Benefits: EggPool's runtime already owns a provider-bound thinking-control contract with independent toggle/effort/budget dimensions, an accepted-effort list, and a normalization shape; CodeGG would inherit the vocabulary rather than re-derive it.

Costs and failure modes: it violates the closed provider-wire-kernel roadmap's invariant that CodeGG operates against direct providers without requiring an EggPool process; it removes the capability from every direct-provider connection, which is the majority path; and the capability data is reachable only over HTTP from a proxy CodeGG does not require. It also does not help the always-on models, whose contract is a property of the model rather than of a proxy.

### Option D — Persist a per-model capability table first, and widen the vocabulary later

Add a SQLite table of per-model reasoning capabilities and drive the UI from it, deferring the wire-intent fidelity work.

Benefits: the data model is settled early; the UI can be correct immediately.

Costs and failure modes: it persists a fact that is partly provider-deployment-specific and partly discoverable, and it forces a storage-layout migration before the capability it stores can even be expressed on the wire. It also inverts the dependency order: the honest-"off" defect is a wire-boundary defect and cannot be fixed by a table.

## Decision

**Option B is selected.** CodeGG's reasoning control becomes an open, provider-scoped vocabulary, and CodeGG's bridge to the wire kernel becomes lossless for every dimension the kernel can express.

The decision has four parts.

1. **Open label vocabulary.** The protocol carries a reasoning effort as an open, provider-scoped string rather than a closed enum. CodeGG does not enumerate, validate against a fixed list, or translate vendor labels. A label that the selected model does not accept is handled by the capability policy of ADR-0045, not by the type.

2. **Faithful wire intent.** The CodeGG → `eggpool-wire` bridge constructs `ReasoningIntent` so that each user intent maps to the distinct kernel shape that expresses it:
   - "thinking off" → the kernel's explicit-disable intent (`requested: Some(false)`, `explicit_disable: true`), which is *not* the same as "unspecified";
   - an effort label → mode `Effort` carrying the label verbatim;
   - a fixed token budget → mode `FixedBudget`;
   - an adaptive/always-on toggle → mode `Adaptive` or `Toggle`, which is the shape a binary provider thinking contract accepts.
   The CodeGG request type therefore distinguishes "unspecified" from "explicitly off", and the two no longer share one representation.

3. **No fabrication.** CodeGG never converts an effort label into a token budget, never invents a level, and never coerces an unsupported label into a supported one. Where a surface cannot express the selected dimension, the request is either rejected locally under ADR-0045's policy or the control is reported as inapplicable — never silently dropped.

4. **Surface-awareness is a consequence, not a goal.** The control does not promise that every provider honours every dimension. What it promises is that CodeGG **knows** which dimension it selected, sends the shape that expresses that dimension, and never reports a state the request did not actually express. A provider that rejects a value CodeGG legitimately sends is a provider error, reported as such.

5. **The daemon owns the selection; a turn may override it.** The reasoning selection is a **daemon-owned, principal-scoped, durable preference**, held in `runtime_preferences` alongside the existing approval, sandbox, and last-used-model preferences, with its own store method, its own protocol request, and its own additive nullable column following the `migrate_v62` precedent. It is resolved at turn admission as: an explicit per-turn override on the turn request → the persisted principal preference → the agent/model-profile default. `CoreRequest::TurnSubmit.reasoning` remains the explicit per-turn override and keeps its `#[serde(default)]` tolerance, so an absent field continues to mean "inherit".

   The consequences of daemon ownership are deliberate: every turn-admission path — direct submit, queued delivery, idle continuation, and the `SessionPromptSubmit` path — resolves the selection from the same place, so a path that reconstructs a turn cannot silently drop it; the selection survives a frontend reconnect; and two frontends on the same daemon agree on the selection, because it is daemon state rather than frontend state. The selection carries **no authorization meaning** and grants nothing.

   A path that genuinely cannot express an override MUST report visibly rather than defaulting silently — but under daemon ownership that path reads the persisted preference rather than dropping the selection.

The per-turn override is carried on every daemon turn-admission path, and the protocol is extended so that a path which cannot express an override fails visibly rather than defaulting to "inherit".

## Consequences

### Positive

- `/reasoning off` becomes truthful: the request explicitly disables thinking where the surface supports an explicit disable, and where the surface cannot, the control reports that the model's reasoning cannot be turned off rather than claiming it was.
- Vendor-specific depth labels become reachable without a CodeGG release, because the vocabulary is open and provider-scoped.
- A binary "this model always reasons" contract becomes expressible, which is the precondition for offering a depth ladder without an on/off switch on such models.
- The always-on adapter escape hatch becomes reachable, because an explicit zero is no longer required to express "off" — the explicit-disable intent is.
- The kernel becomes the single authority on which dimensions a surface can express, and CodeGG's bridge stops re-deciding that per surface.
- The selection becomes daemon state rather than frontend state, so the three turn-reconstructing paths that silently dropped it — and the frontend reconnect that silently lost it — are fixed by construction rather than by patching each path.
- Two frontends on the same daemon agree on the selection, because it is resolved from one daemon-owned place.

### Negative

- The protocol's reasoning type is no longer exhaustively matchable. Every consumer of the label must tolerate a value it does not recognise, and the frontend can no longer derive a cycling order from the type.
- The available options for a control become data rather than code, so the frontend needs a source of per-model options before the control can present a truthful choice. That is ADR-0045's subject.
- A bridge that is faithful is also more expressive, so the space of distinct request shapes CodeGG can produce grows; the wire-boundary tests must cover each shape rather than the two the current bridge can build.
- The always-on case may, on some surfaces, be reportable but not adjustable — an honest limitation rather than a hidden one.
- Daemon ownership introduces **one new durable column** and therefore one storage migration, where a per-turn-only design would have introduced none. The migration is additive and follows the established `migrate_v62` precedent on the same table, but it is a real cost and it must satisfy the project-catalog invariants guard.
- A persisted selection is a **shared** selection. Two frontends on the same daemon now write the same preference, so a selection made in one is visible in the other. This is the intent of daemon ownership, but it is a behaviour change for anyone who assumed the control was private to their frontend.

### Neutral or deferred

- The `eggpool-wire` pin is unchanged. This decision widens CodeGG's use of an existing pinned kernel; it does not propose changing the dependency, and no new dependency is introduced.
- Whether per-model capability data is persisted in SQLite is **not** decided here. It is deferred to ADR-0045 and its milestone, and the capability must be expressible without a migration.
- Provider-side reasoning text rendering (the separate `/thinking` display toggle) is unaffected. That toggle governs whether reasoning *summaries* are rendered and is not part of this decision.

## Compatibility and migration

- **Protocol.** The reasoning override's serialized shape changes. The change is additive and backwards-tolerant in the direction that matters: a client that does not know the new shape must still decode old and new requests, and a daemon that receives a request it does not understand must fall back to the configured default rather than failing the turn. `CoreRequest::TurnSubmit.reasoning` is already `#[serde(default)]`, so an absent field continues to mean "inherit". A new client against an old daemon degrades to "the daemon ignores the field" — which must be stated in the operator-visible compatibility notes rather than left implicit.
- **Storage.** One additive, nullable column on `runtime_preferences` — the reasoning selection — with its own store method and its own protocol request. It follows the established `migrate_v62` precedent on the same table (a plain `ALTER TABLE … ADD COLUMN` that keeps every existing row) and requires `STORAGE_LAYOUT_VERSION` to be bumped to match the highest wired migration, as the project-catalog invariants guard enforces. Existing rows read as "no explicit selection", which preserves the current behaviour exactly. The **per-model capability data** is a separate matter and is decided by ADR-0045; this decision does not require it to be persisted.
- **Configuration.** `default_reasoning_effort` and `default_thinking_budget` keep their current types and meaning. A configuration value naming a vendor label is already carried verbatim; under this decision it reaches the wire instead of being discarded. Existing configuration therefore changes behaviour from "silently dropped on some surfaces" to "sent as selected", which is the intent of the change and must be called out in release notes.
- **Provider behaviour.** For providers whose surface cannot express the selected dimension, the request changes from "no reasoning field" to "a field the surface rejects" in some configurations. Under ADR-0045's policy this is resolved locally before dispatch, so the provider should not see a request it will reject; the remaining risk is a provider that rejects a value its own documentation advertises, which is a provider-side defect and is reported as such.
- **Removal criteria.** The legacy closed-effort type and the "unspecified means disabled" behaviour are removed once every consumer reads the open vocabulary. There is no flag day: the removal is internal and complete within the milestone series.

## Security and reliability implications

- **No secret exposure.** Reasoning labels and budgets are request-shaping configuration, not credentials. No label value derived from a provider response may be logged verbatim without redaction rules if a provider ever places user data in a label position; the capability overlay from a provider catalog is treated as untrusted input and is validated as a bounded, typed shape before use.
- **Untrusted discovery input.** Per-model capability data obtained from a provider's catalog response is attacker-influenceable if the endpoint is attacker-controlled. Under ADR-0045 such data may only *narrow* what CodeGG offers, never widen it beyond the adapter-declared baseline, and it may not introduce a budget or label the adapter did not declare.
- **Denial of service.** A provider catalog that advertises an enormous accepted-effort list is bounded before it reaches a UI or a request. The list is capped and deduplicated.
- **Fail-closed on capability absence.** The canonical specification requires security-sensitive capability absence to fail closed. A reasoning control is not itself a security boundary, so the failure mode is a typed refusal at admission with a truthful message rather than an unbounded wait or a provider-side 400 with an opaque body.
- **Cancellation and restart.** The per-turn override is a per-turn value carried on the turn request, so it is naturally re-established on a restart from the same admitted request, and a cancelled turn discards it with the turn. The persisted selection is daemon state with the same restart properties as the existing `runtime_preferences` rows: it reloads on restart, and a write failure is reported rather than silently applying in memory only. No new recovery policy is introduced.
- **Contention.** The capability overlay is read-only for the request path; it is refreshed on the existing catalog-refresh cadence and never mutated in place by a turn. A turn therefore never observes a half-updated capability set. The persisted selection is written through the existing revision-gated preference store, so a concurrent write is a typed conflict rather than a silent last-write-wins.

## Verification

The decision is proven by:

- a wire-boundary test that asserts, for each user intent, the **distinct** kernel `ReasoningIntent` produced, including that "off" and "unspecified" are different shapes and that an explicit disable carries `explicit_disable: true`;
- a per-surface encoding fixture set that asserts the encoded body for each intent on each surface CodeGG uses, so that a surface which cannot express a dimension is *proven* to be unable to rather than assumed;
- a test asserting that a non-enumerated label (`xhigh`, `max`) survives to the encoded body verbatim on an effort-capable surface;
- a test asserting that no CodeGG path converts an effort label into a token budget or into a different label;
- a test asserting that the override reaches the encoder on every daemon turn-admission path (direct submit, queued delivery, idle continuation, and the `SessionPromptSubmit` path), and that a path which cannot carry it reports visibly rather than defaulting silently;
- a static guard asserting that CodeGG's bridge contains no hard-coded `explicit_disable: false` and no closed effort enum, so the collapse cannot be reintroduced;
- a compatibility test decoding both the legacy and the new serialized override shapes.

## Supersession

None. This record is superseded only by a later accepted ADR that changes the reasoning control contract.

Related, and distinct: ADR-0045 (per-model reasoning capability authority) decides where the per-model capability facts come from and how they are applied at admission. ADR-0044 decides the vocabulary and the wire-intent fidelity; ADR-0045 decides the authority and policy for the facts that constrain that vocabulary.
