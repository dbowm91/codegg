# Provider Reasoning Control Roadmap

Status: closed

> **Closed 2026-10-21** with all five milestones closed and their closure records
> accepted (`plans/closure/provider-reasoning-control/001-status.md` … `005-status.md`). Two
> residuals are carried forward as **named, accepted limitations** rather than blockers: the
> discovery overlay is inert machinery (the pinned upstream `/v1/models` response publishes no
> capability block, so its daemon-side install is deliberately unwired), and the WS-attach
> frontend has no channel for a core request, so `/reasoning` there reports that the selection
> cannot be stored. Both are documented in `architecture/provider.md`,
> `architecture/protocol.md`, and `architecture/tui.md`.

Long-term references:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — provider connection ownership; CodeGG resolves provider-facing request policy itself and does not require an EggPool process.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — protocol operations are versioned and capability-negotiated; new clients tolerate unknown optional fields; security-sensitive capability absence fails closed.
- `plans/000-long-term-specification.md#14-presence-and-real-time-team-awareness` — provider-private hidden reasoning is not an observable product feature; the control governs the *request*, not the rendering of reasoning text.
- `plans/000-long-term-specification.md#26-reliability-and-recovery` — daemon restart recovers or deterministically interrupts sessions and turns.
- `plans/001-terminology-and-domain-model.md` — provider/model identity; provider connection identity; the authority boundary between CodeGG and a shared wire kernel.
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections` — provider connection and EggPool integration direction.
- `plans/003-planning-process.md` — bounded handoff and closure rules.

Related ADRs:

- `plans/adrs/ADR-0049-open-provider-scoped-reasoning-control-vocabulary.md` — the open, provider-scoped vocabulary and the faithful wire intent.
- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` — the layered per-model capability authority and the local admission policy.

Related architecture documents:

- `architecture/provider.md` (§"Extended-thinking control (`/reasoning`)", §"Provider capability model")
- `architecture/agent.md` (§request preparation and precedence)
- `architecture/config.md` (§model profile configuration)
- `architecture/tui.md`

Predecessor roadmaps (both closed; this roadmap builds on their established contracts):

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md` — established `eggpool-wire` as the single neutral wire kernel and the one CodeGG semantic-request → target-grammar encoder, and fixed the ownership rule that provider-facing request policy stays in CodeGG.
- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md` — established the declarative model-adapter registry (the one authority for model-specific policy) and the provider-neutral reasoning-preservation rules.

## 1. Purpose and ownership boundary

This workstream makes CodeGG's extended-thinking control **honest and model-aware**: a user can select a reasoning depth the selected model actually accepts, and the control never reports a state the outgoing request did not express.

It owns:

- the reasoning control's **vocabulary** — an open, provider-scoped label set rather than a closed enum;
- the **wire-intent fidelity** of CodeGG's bridge to the `eggpool-wire` kernel, so every distinguishable user intent reaches the encoder as the distinct kernel shape that expresses it;
- the **per-model reasoning capability record** — which depth labels a model accepts, whether its reasoning can be disabled, whether a fixed budget is accepted and within what bounds, and whether it always reasons;
- the **resolution order** that combines an operator override, the compiled adapter baseline, and a discovery overlay from provider catalog metadata;
- the **local admission policy** that refuses an unsupported selection at turn admission with a typed, actionable error instead of forwarding it to a provider 400;
- the **per-turn threading** of the selected control through every daemon turn-admission path;
- the **frontend option surface** that offers the selected model's actual options and states the current selection persistently.

It consumes:

- the immutable-pinned `eggpool-wire` kernel, whose `ReasoningIntent`, `ThinkingFacts`, `NeutralThinkingCapability`, `ReasoningCapabilityPolicy`, and `CapabilityDisposition` are the wire and policy vocabulary;
- the existing declarative model-adapter registry in `crates/codegg-core`, and its build-time validation in `crates/codegg-core/build.rs`;
- the existing provider catalog/discovery layer and its refresh cadence;
- the existing `Provider`, `ChatRequest`, provider-connection, and turn-admission machinery;
- the existing frontend protocol DTOs and the TUI's command/status surfaces.

It does **not** own:

- provider transport, credentials, account routing, quota, health, or fallback — those remain CodeGG's existing provider-connection concern and EggPool's, outside this roadmap;
- the private-reasoning preservation rules established by the closed model-adaptation roadmap;
- the separate `/thinking` **display** toggle, which governs whether reasoning summaries are *rendered* and is a different control from the one that shapes the *request*;
- provider-side reasoning content, summarisation quality, or any claim about a model's internal reasoning;
- model discovery, catalog storage layout, or the project-catalog invariants beyond the capability record this roadmap adds to the existing adapter registry.

## 2. Work classification

### Invariants

- **The control never lies.** CodeGG never reports a reasoning state that the outgoing request did not express. "Off" means an explicit disable was sent, or the model is recorded as always-reasoning and the UI says so — never "we sent nothing and hope".
- **Unspecified is not disabled.** The request distinguishes "no reasoning preference" from "explicitly disable reasoning", and the wire intent preserves that distinction.
- **No fabrication.** CodeGG never converts a depth label into a token budget, never invents a level, and never coerces an unsupported label into a supported one. Discovery never fabricates effort-to-budget values.
- **Untrusted discovery may only narrow.** A capability overlay from a provider catalog can remove an option the adapter baseline offers; it can never add one, introduce a label or bound the adapter did not declare, or change the always-reasons property.
- **Unknown is not unsupported and not supported.** An absent capability record is preserved as `Unknown` and is never collapsed into a verdict that enables or forbids a working control.
- **One model-policy authority.** Provider modules must not independently re-resolve model names into a second reasoning-capability table; the adapter registry remains the single compiled authority, as the closed provider-wire-kernel roadmap requires.
- **The kernel decides representability, not policy.** The `eggpool-wire` kernel supplies the per-surface encoding and the per-dimension status decision; CodeGG supplies the facts and the operator policy.
- **The selection is daemon-owned; a turn may override it.** The selection is a daemon-owned, principal-scoped, durable preference resolved at turn admission as: explicit per-turn override → persisted principal preference → agent/model-profile default. A turn-reconstructing path cannot silently drop it, and a frontend reconnect cannot silently lose it. The selection carries no authorization meaning and grants nothing.
- **Capability absence fails closed where it matters.** A selection on a dimension whose effective status is `Unknown` and which no baseline declares is refused at admission with a typed error, never forwarded optimistically.

### Capabilities

- A user can select a reasoning depth from the set the **selected model actually accepts**, including provider-specific labels such as `xhigh` or `max` that are outside any fixed enumeration.
- A user can turn reasoning off, and the request says so explicitly; on a model whose reasoning cannot be disabled, the control reports that instead of pretending.
- A user can set a fixed thinking token budget where the model accepts one, and the selection is bounded by the model's declared budget range.
- A selection the model does not accept is refused **locally at turn admission** with a typed error naming the model and the dimension, before any provider request is dispatched.
- The selected reasoning state is **persistently visible** in the frontend, not only in a transient toast after a command.
- The reasoning selection survives every daemon turn-admission path — direct submit, queued delivery, idle continuation, and the desktop `SessionPromptSubmit` path — rather than silently reverting to the configured default on the paths that reconstruct a turn.

### Infrastructure

- The compiled adapter-baseline capability record and its build-time validation in `crates/codegg-core/build.rs`.
- The capability resolution order that combines operator override, adapter baseline, and discovery overlay into one effective per-turn capability.
- The discovery overlay carried on the existing provider catalog/discovery layer, refreshed on the existing cadence and replaced atomically.
- A narrow static guard that prevents the wire-intent collapse from being reintroduced.
- The additive, optional protocol payload that exposes a model's reasoning capability to frontends.

### Polish

- A compact, readable presentation of the current reasoning state in the frontend's working strip.
- Terse, actionable wording for each refusal (which model, which dimension, what the model accepts).

## 3. Non-goals

- **Not** a second provider abstraction, a second model-policy table, or a new wire encoder. The `eggpool-wire` kernel remains the single encoder and the adapter registry remains the single compiled model-policy authority.
- **Not** a requirement to run an EggPool process. Every capability milestone must work for direct provider connections. The EggPool-published thinking contract is an optional discovery overlay, never a prerequisite.
- **Not** a change to provider transport, credential, quota, health, or fallback behaviour.
- **Not** a claim about a model's internal reasoning, and not a change to the private-reasoning preservation rules.
- **Not** a change to the `/thinking` display toggle, which is a rendering concern rather than a request-shaping one.
- **Not** a storage-layout change in the core capability. The adapter baseline is compiled and the overlay is held on the existing discovery layer; a persisted capability column is explicitly deferred and requires its own recorded decision.
- **Not** a change to the provider wire-kernel dependency pin.
- **Not** a general model-metadata expansion. The capability payload added here is scoped to reasoning; widening the models channel to carry arbitrary per-model metadata is a separate concern.
- **Not** an effort-to-budget conversion feature, in either direction.

## 4. Current state

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`).

Evidence at that baseline:

- **The vocabulary is closed at the protocol layer.** `ReasoningEffort` is a four-variant enum (`Minimal`, `Low`, `Medium`, `High`) in `crates/codegg-protocol/src/core.rs`, and `ReasoningOverride::Effort { level }` carries it. `ReasoningMode::cycle()` reaches only `default → medium → high → off → default`, so `Minimal`, `Low`, and `Budget` are unreachable from the TUI command and no provider-specific label is expressible. The configuration layer is already open (`default_reasoning_effort: Option<String>` in `crates/codegg-config/src/schema.rs`), so a configured vendor label is carried verbatim into the request and then discarded downstream.
- **The wire intent is collapsed.** `crates/codegg-providers/src/wire.rs` builds the kernel's `ReasoningIntent` from the two nullable request fields and can produce only `requested: Some(true)` with mode `Effort` or `FixedBudget`, or the default `requested: None`. The kernel's explicit-disable intent, `ReasoningMode::Adaptive`, and `ReasoningMode::Toggle` are never constructed. The protocol's doc comment claims "`Off` explicitly disables thinking, which is distinct from `None` on the request", and the agent layer's comment claims "neither field set … is the only way to express an explicit disable" — two comments that contradict each other, and the implementation matches the second one, so "off" and "unspecified" are the same request.
- **The control does not survive its target surface.** The kernel's per-surface codecs differ in representable dimensions: effort is representable on the OpenAI Chat and Responses surfaces, a fixed budget on the Anthropic Messages and Gemini GenerateContent surfaces, an explicit disable on the Anthropic Messages surface. Because the bridge can only emit effort or budget, `/reasoning medium` and `/reasoning high` are complete no-ops on an Anthropic-compatible surface, and `default_thinking_budget` is a no-op on an OpenAI-compatible surface.
- **Always-on models are not representable.** MiniMax's published contract for `MiniMax-M3.1-Flash` accepts `low`, `medium`, `high`, `xhigh`, `max` for `reasoning_effort`, rejects `none`, and rejects `thinking.type = "disabled"` with an explicit *"requires adaptive thinking"* error — the model always reasons. CodeGG has no way to say so, and the only adapter that pins thinking on (Laguna's `enable_thinking` transform) computes its wire value as `request.thinking_budget != Some(0)`, a value the kernel's own `validate_reasoning` rejects.
- **There is no per-model reasoning capability.** The only reasoning-related provider fact is `ProviderCapabilities::strips_reasoning_effort`, a per-provider boolean meaning "the provider deletes the field", with a declared list that omits the generic OpenAI-compatible adapter family even though that adapter omits `reasoning_effort` from the encoded body — so the frontend's existing "drops this setting" warning never fires for those connections. The generic compatible contract's omission is *intentional* per M001's provider matrix, so the gap is in the **warning's coverage**, not in the adapter's behaviour.
- **Discovery already sees the answer and discards it.** The EggPool catalog probe parses a `/v1/models` response and keeps only `id` and `name`; the published thinking-control contract in the same response is dropped at the parse boundary.
- **The selection does not survive every daemon path.** The override travels on `CoreRequest::TurnSubmit.reasoning` and is honoured on the ordinary turn path, but the user-prompt-queue delivery path, the idle-continuation path, and the `SessionPromptSubmit` path each reconstruct a turn with the field set to `None`, and the `SessionPromptSubmit` protocol variant carries no reasoning field at all, so the desktop frontend cannot express one. The native client crate hardcodes `None`. The selection is also frontend-local, so a frontend that reconnects to a running daemon loses it entirely.
- **The repository already owns the right store for a daemon-owned selection.** `runtime_preferences` (migration v58) is a daemon-owned, principal-scoped, durable, revision-gated preference table with an established store (`RuntimePreferenceStore` in `crates/codegg-core/src/approval.rs`), an established protocol seam whose principal is derived server-side, and an established additive-migration precedent (`migrate_v62` added two nullable columns to the same table with a plain `ALTER TABLE … ADD COLUMN`). Its documented precedence — *"explicit turn overrides win, then project ceiling, then persisted, then defaults"* — is already the shape this control needs.
- **The selection is not persistently visible.** The current mode is surfaced only in the transient toast emitted after `/reasoning`; the mode's label and detail appear nowhere in the render path.
- **The kernel already supplies the machinery.** `eggpool-wire`'s `adaptation` module is public in the pinned revision and provides `NeutralThinkingCapability` with independent per-dimension statuses, `ReasoningCapabilityPolicy`, `CapabilityDisposition` (`Reject | WarnDrop | AllowWithWarning | Allow`), and a fail-closed default policy. The kernel's own documentation states that capability outcomes are caller-owned facts, never inferred by codecs.

## 5. Target architecture

```
   frontend (TUI / desktop / native client)
        │
        │  CoreRequest::TurnSubmit { reasoning: Option<ReasoningOverride> }   ← explicit per-turn override
        │  CoreRequest::ReasoningPreferenceSet { … }                          ← daemon-owned selection
        │  CoreResponse::… { reasoning_capability: Option<ModelReasoningCapability> }
        ▼
   daemon turn admission  ──►  selection resolution
        │                        explicit turn override ▸ persisted principal preference ▸ defaults
        │                        └─► capability resolution (per turn, per model)
        │                             operator override ▸ adapter baseline ▸ discovery overlay
        │                             └─► kernel capability policy → Reject | WarnDrop | AllowWithWarning | Allow
        ▼
   AgentLoop request preparation
        agent config ▸ model-profile defaults ▸ resolved selection
        ▼
   ChatRequest { reasoning_effort: Option<String>, thinking_budget: Option<u64> }
        ▼
   CodeGG → eggpool-wire bridge  ──►  ReasoningIntent (lossless)
        │                            explicit disable | Effort(label) | FixedBudget | Adaptive | Toggle
        ▼
   per-surface codec (OpenAI Chat / OpenAI Responses / Anthropic Messages / Gemini)
```

Four properties of the target architecture carry the invariants:

- **The bridge is lossless.** Every distinguishable user intent maps to the distinct kernel shape that expresses it, so representability is decided by the kernel's per-surface codec rather than by CodeGG's bridge.
- **The selection is daemon-owned.** It lives in `runtime_preferences` beside the existing approval, sandbox, and last-used-model preferences, so every turn-admission path resolves it from the same place and a frontend reconnect does not lose it. A turn may still override it explicitly.
- **The capability is resolved per turn against the model the turn uses.** It is not a property of the frontend's selection, and it is not a property of the process. Two sessions on the same daemon with different models resolve different capabilities.
- **The frontend offers a subset of what the capability permits.** The frontend never offers a label the effective capability does not accept, and never presents the selection as an absolute when the capability is `Unknown`.

## 6. Dependency graph

```text
        ADR-0049 (accepted)            ADR-0050 (accepted)
              │                              │
              ▼                              │
        Milestone M001 ◄──────────────────────┤
        (faithful wire intent)               │
              │ (hard)                       │
              ▼                              │
        Milestone M002                       │
        (open vocabulary +                   │
         turn-admission threading)           │
              │ (hard)                       │
              └──────────┬───────────────────┘
                         ▼
                   Milestone M003
                   (capability record +
                    local admission policy)
                         │ (hard)              │ (interface)
                         ▼                      │
                   Milestone M004              ▼
                   (frontend option      Milestone M005
                    surface)            (discovery overlay)
```

| Dependency | Class | Note |
|---|---|---|
| ADR-0049 accepted | hard (decision) | M001–M003 implement it; the roadmap is written against it. |
| ADR-0050 accepted | hard (decision) | M003 implements it; M005 extends it. |
| M001 → M002 | hard | M002's new vocabulary values have no wire representation until M001 restores them. |
| M002 → M003 | hard | M003's admission policy validates a selection value that must be expressible and carried. |
| M003 → M004 | hard | The frontend can only offer the effective capability's options once that capability exists. |
| M003 → M005 | interface | M005 supplies the overlay input; M003's resolution order already accommodates an absent overlay, so M005 is not on M003's critical path. |
| Closed provider-wire-kernel consolidation roadmap | interface | Established `eggpool-wire` as the single encoder and the ownership rule this roadmap preserves. |
| Closed model-adaptation roadmap | interface | Established the adapter registry as the single compiled model-policy authority. |

## 7. Milestones

### Milestone 1 — Faithful reasoning intent at the wire boundary

Class: capability

Objective: Restore the distinction between "no reasoning preference" and "explicitly disable reasoning", and make every user intent reach the encoder as the distinct kernel shape that expresses it.

Dependencies: ADR-0049 accepted. No hard code dependency.

Deliverable boundary: `crates/codegg-providers` — the CodeGG → `eggpool-wire` bridge constructs `ReasoningIntent` losslessly; the request type distinguishes unspecified from explicit disable; per-surface encoding fixtures cover each intent; a narrow static guard prevents the collapse from being reintroduced.

User or operator value: `/reasoning off` stops being a no-op that merely omits a field. On a surface that supports an explicit disable, the request says "disable" instead of "unspecified", and on a surface that cannot, the difference is visible rather than silent.

Exit conditions: each user intent produces a distinct, asserted kernel intent; "off" and "unspecified" are proven different shapes; the static guard is registered in `scripts/verify.sh quick` and the CI verify job; per-surface fixtures assert the encoded body for each intent.

Deferred work: the open vocabulary (M002), the capability record (M003), the frontend surface (M004).

### Milestone 2 — Open reasoning vocabulary and daemon-owned selection

Class: capability

Objective: Replace the closed effort enum with an open, provider-scoped label set, and move the selection from frontend-local state into a daemon-owned, principal-scoped, durable preference so every turn-admission path resolves it from one place.

Dependencies: M001 closed (hard); ADR-0049 accepted.

Deliverable boundary: `crates/codegg-protocol` (open label type; the selection request/response pair), `crates/codegg-core` (the `runtime_preferences` column, its store method, and the additive migration), `src/core` (the daemon handler and the turn-admission resolution point), `src/agent` (the precedence and the selection-to-request mapping), `crates/codegg-client` (native client composition). One additive nullable column and one migration; no other storage change.

User or operator value: a vendor label such as `xhigh` or `max` is selectable and reaches the provider; the selection survives a frontend reconnect; a prompt queued for later delivery, an idle continuation, and a desktop-app submission all run with the selection the user made rather than reverting to the configured default; and two frontends on the same daemon agree on the selection.

Exit conditions: a non-enumerated label survives to the encoded body verbatim; the selection is durable and principal-scoped with a revision-gated write; every turn-admission path is proven to resolve the same selection, including the paths that reconstruct a turn; the legacy closed enum is removed with no consumer left reading it; the additive migration preserves every existing `runtime_preferences` row.

Deferred work: the capability record (M003), the frontend surface (M004).

### Milestone 3 — Per-model reasoning capability record and local admission policy

Class: capability

Objective: Give the daemon a per-model statement of what reasoning the model accepts and whether it can be turned off, and refuse an unsupported selection locally and typed instead of forwarding it to a provider 400.

Dependencies: M002 closed (hard); ADR-0050 accepted.

Deliverable boundary: `crates/codegg-core` — the adapter-baseline capability record, its `build.rs` validation, and the resolved capability; `crates/codegg-providers` — the capability resolution order and the kernel-policy application; `src/core` — the typed admission refusal.

User or operator value: selecting an effort the model does not accept fails immediately with a message naming the model and the dimension, instead of a provider error several seconds later. A model whose reasoning cannot be disabled is described as such.

Exit conditions: the authority order is implemented and tested, including that a discovery overlay claiming a label the baseline does not declare is rejected; `Unknown` is preserved and a selection on an `Unknown` dimension with no baseline is refused at admission with a typed error; the always-reasons property is a first-class capability value; no provider request is dispatched for a refused selection.

Deferred work: the frontend option surface (M004), the discovery overlay (M005).

### Milestone 4 — Frontend reasoning option surface

Class: capability

Objective: Present the selected model's actual reasoning options, and make the current selection persistently visible.

Dependencies: M003 closed (hard).

Deliverable boundary: `crates/codegg-protocol` — the additive optional capability payload on the model DTOs; `src/tui` — the option surface driven by the effective capability, and the persistent working-strip presentation; the desktop and native-client surfaces to the extent they consume the payload.

User or operator value: the user sees only the depths the model accepts, sees the current selection without issuing a command, and sees an always-reasoning model described honestly rather than being offered a switch that would fail.

Exit conditions: the capability payload is optional and tolerant of absence; a client without a payload does not present a capability it did not receive; the working strip shows the current selection; the control offers no disable for an always-reasoning model; the existing surface-count and key-authority guards are unaffected.

Deferred work: the discovery overlay (M005).

### Milestone 5 — Discovery overlay from provider catalog metadata

Class: infrastructure

Objective: Use provider-published capability metadata, where it exists, to narrow the adapter baseline per deployment.

Dependencies: M003 closed (interface — the resolution order already accommodates an absent overlay); ADR-0050 accepted.

Deliverable boundary: `crates/codegg-providers` — the catalog probe retains a published thinking-control contract instead of discarding it; the discovery overlay is bounded, typed, refreshed on the existing cadence, and applied only to narrow.

User or operator value: a deployment whose proxy advertises a narrower label set is offered exactly that set, without a CodeGG release. **At this baseline the value is not yet live**: the upstream contract this milestone reads is a design document, not an implemented wire shape — the pinned `/v1/models` response carries no capability block, and the capability shape upstream does expose rides a route CodeGG does not call. The milestone lands the narrowing machinery, forward-compatible and inert, and its closure says so explicitly. See the milestone plan's §3 and ADR-0050's amendment note.

Exit conditions: the overlay is bounded and deduplicated; malformed input degrades to `Unknown`; the overlay never widens the baseline, proven by an injected hostile overlay; the overlay is replaced atomically so a turn never observes a partial update; direct provider connections are entirely unaffected.

Deferred work: persistence of the overlay in SQLite, which requires its own recorded decision and is explicitly not a prerequisite for M001–M004.

## 8. Cross-cutting requirements

### Storage and migration

**M002 introduces the roadmap's only storage change**: one additive, nullable column on `runtime_preferences` holding the reasoning selection, following the established `migrate_v62` precedent on the same table (a plain `ALTER TABLE … ADD COLUMN` that keeps every existing row). It requires a new migration and a `STORAGE_LAYOUT_VERSION` bump to match the highest wired migration, as the project-catalog invariants guard enforces. Existing rows read as "no explicit selection", which preserves current behaviour exactly. The selection's own store method is separate from the approval/sandbox methods, so the existing preference revision semantics are untouched.

**M001, M003, M004, and M005 require no storage change.** The adapter capability baseline is compiled into the binary; the discovery overlay is held on the existing catalog/discovery layer. If M005 proposes persisting the overlay, it must add its own migration, bump `STORAGE_LAYOUT_VERSION`, satisfy the project-catalog invariants guard, and must not become a prerequisite for a capability milestone.

### Protocol and compatibility

The protocol changes in this roadmap are **additive and optional**. The per-turn selection rides on a field that is already `#[serde(default)]`, so an absent field continues to mean "inherit". The capability payload is optional; a client that does not receive one must not present a capability it did not receive, and a new client against an old daemon degrades to the configured default without claiming the model's full option set. Any DTO field added to the existing model DTOs must carry `#[serde(default)]` and a `skip_serializing_if` so that a new client can decode an old daemon's response and an old client can decode a new daemon's.

### Security and authorization

A capability record describes a model's reasoning options and carries **no authorization meaning**. It must never be derived from, or grant, any permission, tool, path, or credential scope. Discovery-supplied capability data is untrusted input: it is parsed into a bounded, typed shape before use, malformed input degrades to `Unknown`, and it may only narrow the adapter baseline. A capability absence that matters fails closed at admission with a typed refusal.

### Concurrency, cancellation, and recovery

The per-turn selection travels on the turn request, so it is re-established on restart from the same admitted request and discarded with a cancelled turn. The discovery overlay is refreshed on the existing catalog-refresh cadence and replaced atomically; a turn never observes a partial update, and a capability change between turns affects the next turn rather than mutating a running one.

### Observability and audit

A refused selection is a typed admission failure with a reason naming the model and the dimension. The reason must be user-visible and must not leak provider credentials or unbounded provider text. Capability data is not sensitive, but a provider's free-text fields must not be copied into the capability record or into logs beyond the bounded typed shape.

### Performance and resource use

Capability resolution happens once per turn admission and must be O(1) in the size of the label set, with that set bounded. A hostile catalog advertising an enormous label list must not inflate a UI option surface or a request.

### Documentation and static guards

`architecture/provider.md` (§"Extended-thinking control (`/reasoning`)"), `architecture/config.md`, `architecture/agent.md`, and `architecture/tui.md` must be reconciled with the landed contract; the two mutually contradictory doc comments about explicit disable are corrected in place. A narrow static guard prevents the wire-intent collapse from being reintroduced and is registered in `scripts/verify.sh quick` and the CI verify job. No new CI lane, scanner, coverage gate, benchmark gate, or release automation is introduced.

## 9. Verification strategy

Subsystem-level evidence:

- **Wire-intent fidelity** — for each user intent, the distinct kernel `ReasoningIntent` is asserted, including that "off" and "unspecified" differ and that an explicit disable carries `explicit_disable: true`.
- **Per-surface encoding** — for each intent and each surface CodeGG uses, the encoded body is asserted, so a surface that cannot express a dimension is *proven* unable to rather than assumed.
- **Vocabulary openness** — a non-enumerated label (`xhigh`, `max`) survives to the encoded body verbatim; no path converts a label into a budget or into a different label.
- **Capability authority order** — operator override, adapter baseline, and discovery overlay are applied in order; an overlay claiming a label the baseline does not declare is rejected; `Unknown` is preserved as `Unknown`.
- **Local refusal** — an unsupported selection is refused at admission with a typed error naming the model and dimension, and **no provider request is dispatched**.
- **Always-reasons** — an always-reasoning model is offered no disable option and an attempted disable is refused locally.
- **Turn-admission threading** — every daemon turn-admission path carries the selection, proven per path.
- **Frontend tolerance** — a client with no capability payload presents no capability; a client receiving one presents only its options.
- **Restart and recovery** — the selection is re-established from the admitted request after a restart, and discarded with a cancelled turn.
- **Static guard** — the bridge contains no hard-coded `explicit_disable: false` and no closed effort enum.

Verification follows the repository's existing posture: focused deterministic tests, the existing `scripts/verify.sh quick` suite, and the existing Clippy configuration. No live-provider CI, no new CI lane, no benchmark or coverage gate, and no permanent external service is introduced.

## 10. Risks and decision points

| Risk / decision | Handling |
|---|---|
| The protocol's reasoning type loses exhaustive matchability, so a consumer may mishandle an unrecognised label | The capability policy refuses an unsupported selection at admission, and the frontend only offers capability-permitted labels. Both are covered by explicit tests. |
| A stale discovery overlay offers a label a model no longer accepts | Bounded consequence: the provider rejects it and the error is reported as a provider error. The overlay refresh cadence is the existing one; no new staleness is introduced. |
| The capability record and the adapter's existing reasoning defaults could disagree | The precedence rule is defined in ADR-0050 and is implemented and tested as a single resolution function, not as two independent reads. |
| The `SessionPromptSubmit` protocol variant gains a field, which is a wire-visible change | Additive and optional, consistent with the protocol's existing tolerance rules; a new client against an old daemon degrades to "inherit". |
| Effort-vs-budget precedence can invert when an agent sets an effort and a profile sets a budget | Resolved by the mutual-exclusivity rule: the effective selection resolves to exactly one dimension, and the resolution function is the single place that decides. |
| The kernel's neutral capability type is status-only and carries no accepted-value list | CodeGG carries the accepted-label set in its own capability record and uses the kernel for the per-dimension status decision — the division the kernel's own documentation describes. |
| Scope drift into general per-model metadata | Explicitly a non-goal; the added payload is scoped to reasoning. |
| A milestone attempts to persist the capability in SQLite | Not permitted without a separate recorded decision; the core capability is expressible without migration. |
| ADR-0049 or ADR-0050 is not accepted | M001–M005 are written against those decisions. If either is amended, the affected milestone plans are corrected before handoff rather than the roadmap being rewritten silently. |

## 11. Completion definition

This roadmap is closed when:

- every distinguishable reasoning intent reaches the encoder as the distinct kernel shape that expresses it, and "off" is distinguishable from "unspecified";
- the reasoning vocabulary is open and provider-scoped, and a vendor-specific label reaches the provider verbatim;
- the selection is daemon-owned, principal-scoped, and durable, and every daemon turn-admission path resolves it from that one place;
- the per-turn selection is carried on every daemon turn-admission path;
- the daemon holds a per-model capability record with independent effort / disable / budget / always-reasons dimensions, resolves it per turn in the agreed authority order, and refuses an unsupported selection locally with a typed error before dispatching a provider request;
- the frontend offers only the selected model's accepted options, shows the current selection persistently, and does not offer a disable for an always-reasoning model;
- discovery-supplied capability data, where present, narrows but never widens the adapter baseline;
- the static guard preventing the wire-intent collapse is registered in `scripts/verify.sh quick` and the CI verify job;
- `architecture/provider.md`, `architecture/config.md`, `architecture/agent.md`, and `architecture/tui.md` describe the landed contract, with the contradictory explicit-disable comments corrected in place;
- each milestone has an accepted closure record with the evidence this roadmap defines, and no unresolved critical or high finding remains.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 — Faithful reasoning intent at the wire boundary | closed | `plans/implementation/provider-reasoning-control/001-faithful-reasoning-intent-at-the-wire-boundary.md` | `plans/closure/provider-reasoning-control/001-status.md` | — (ADR-0049 accepted 2026-10-21) |
| 2 — Open reasoning vocabulary and daemon-owned selection | closed | `plans/implementation/provider-reasoning-control/002-open-reasoning-vocabulary-and-daemon-owned-selection.md` | `plans/closure/provider-reasoning-control/002-status.md` | — (M001 closed; ADR-0049 accepted 2026-10-21) |
| 3 — Per-model capability record and local admission policy | closed | `plans/implementation/provider-reasoning-control/003-per-model-capability-record-and-admission-policy.md` | `plans/closure/provider-reasoning-control/003-status.md` | — (M002 closed; ADR-0050 accepted 2026-10-21) |
| 4 — Frontend reasoning option surface | closed | `plans/implementation/provider-reasoning-control/004-frontend-reasoning-option-surface.md` | `plans/closure/provider-reasoning-control/004-status.md` | — (M003 closed) |
| 5 — Discovery overlay from provider catalog metadata | closed | `plans/implementation/provider-reasoning-control/005-discovery-overlay-from-catalog-metadata.md` | `plans/closure/provider-reasoning-control/005-status.md` | — (M003 closed; ADR-0050 accepted 2026-10-21) |

### Merge reconciliation with `main` (2026-10-22, and again 2026-10-23)

`main` was reconciled with the closed milestone chain **six times**, because it
advanced five times while the merged tree was being qualified: first 439 commits to
`8cee65934` (eight conflicts), then six more to `47f5572b1` — the
agent-workstream blackboard's M008 — with one conflict, then 79 more to
`f1b83a7a7` — the CI/test-throughput recovery workstream and its M004
root-test-target family consolidation — with two conflicts, then five more to
`3e43d4144` (the landing of PR #170 and PR #171) with one conflict, and finally five more to
`12999178a` (PR #173, which is `main`'s own fix for the Linux-gated test-support compile
error this line's pull request had inherited, and PR #174) with **no conflict at all**, and
finally nine more to `ff82c68da` (a documentation-surfaces pass that changed **two rows'
`Source`** and added a third) with **seven** conflicted architecture documents, every one
resolved by union.
The reasoning contract survived intact; three allocations yielded to `main` and one surface moved:

- **ADR-0044 → ADR-0048 → ADR-0049** (the open-vocabulary record) and
  **ADR-0045 → ADR-0049 → ADR-0050** (the per-model capability record). On
  2026-10-22 `main` had already published ADR-0044 and ADR-0045, so both
  numbers yielded; on 2026-10-23 `main` published its own ADR-0048, so both
  yielded again. Main wins every number, and the same rule applies each
  time: the decision content, `Status: accepted`, the acceptance date and
  the accepting authority are unchanged; only the identifier and the file
  name move, with a dated renumbering note;
- the reasoning-selection migration **v84 → v88**, with
  `STORAGE_LAYOUT_VERSION` 87 → 88; and
- the M004 indicator moved from the **deleted working strip** to the **footer
  status bar**, with its guard's fourth rule rewritten accordingly.

The second merge's single conflict was `architecture/overview.md`'s
Verified-counts table, and its two file-count rows were **re-measured in the
newly merged tree** rather than picked: **300** integration test files and
**71** CI guard scripts, the union of both sides' contributions.

The **third** merge is the reconciliation against the `origin/main` the pull
request is opened against. Its two conflicts were `architecture/overview.md`'s
Verified-counts table and `plans/registry.md` (both sides inserted a subsystem
row at the same slot — **both rows were kept**). The counts table needed
re-measurement on **two** axes, because `main`'s consolidation changed a row's
**Source**: `ls tests/*.rs` alone now reports 4 because the root integration
test *modules* moved into family directories as `.inc` includes, so the row
counts `tests/*.rs` + `tests/*/*.inc` with `tests/common/` excluded and reads
**321**; the CI guard scripts row reads **80**. A third row was **stale on
`main`** and corrected rather than incremented — the tool source files row
read 83 against a tree of 84, because `main` added
`src/tool/bash/tests/git_fixture.rs` without re-measuring it.

The **fourth** merge had one conflict — `plans/registry.md`, both sides inserting a
subsystem row at the same slot — and both rows were kept, with `main`'s current row
first (`main`'s own PR #171 had updated that row's text). Every Verified-counts row
was re-measured against its own listed `Source` and **none moved**, because PR #171's
family consolidation moved the `.inc` files rather than removing them. All 51 guards
still pass, with this line's `reasoning_overlay_narrowing` ownership entry intact
through `main`'s repair of the ownership and duplication guards.

Recorded in
`plans/closure/provider-reasoning-control/006-merge-reconciliation-status.md`.

## 13. Verification

The milestone chain was verified as one tree on 2026-10-21, after the
independent audits of M001 and M002 and the post-audit corrections recorded
in each closure record.

| Check | Result |
|---|---|
| `bash scripts/verify.sh quick` | **pass** — every guard, the agent-schema and boundary checks, and `cargo check --workspace --all-targets --locked` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **pass** |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | **15,797 of 15,800 passed** (2 leaky, 7 skipped). The three non-passes are environmental and pre-existing: two `core::daemon_refresh::tests` cases fail on macOS with `InvalidRootPath("/tmp")`, and `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` hit the 300 s slow-timeout under a machine running 30+ concurrent cargo builds and passes in 0.95 s in isolation. All three live in files this chain did not modify. |
| `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast` | **12,478 of 12,481 passed** — the same three pre-existing non-passes, none in a file this chain modified |
| `cargo test -p codegg --lib core::daemon_turns` | **22 passed** — the reasoning admission, notice, and queue-delivery tests |
| `cargo test -p codegg-core --lib` | **907 passed** — including the compare-and-swap, concurrency, and restart tests |
| `cargo test -p codegg-providers --lib eggpool` | **30 passed** |
| `cargo test -p codegg-core --test reasoning_overlay_narrowing` | **6 passed** |
| `codegg validate --config codegg.example.jsonc` | **valid** |
| The four new static guards | each passes, and each `--self-test` rejects every injected violation |

**A defect the integration pass caught.** The queued-delivery behavioural test
called the queue delivery entry directly, which `scripts/
check_queue_delivery_callsites.py` fails: ADR-0038 fixes that entry's caller
set to three recorded callers, and a test is a fourth. The test now drives
the delivery through the permitted post-terminal-wake entry, so the guard
describes the tree it is supposed to police. The guard would otherwise have
been silently narrowed by a test rather than the other way round.

### The merged tree was qualified again, separately

The table above qualifies the **milestone chain on the branch**, before the
reconciliation. The merged tree was qualified again afterwards, because a
merge can break things that each side had proved on its own — and one such
breakage was found and fixed. `main` advanced a further six commits while
that qualification was running, so the qualification was repeated after the
second merge; the second merge touched only the counts, the CI matrix, the
guard registry and the plan registry, and **no guard invocation was lost** —
a set comparison of `scripts/verify.sh` before and after it shows one
addition (`main`'s blackboard guard) and no removal.

| Check on the merged tree | Result |
|---|---|
| `cargo check --workspace --all-targets --locked` | **exit 0**, zero errors |
| `cargo fmt --check --all` | **clean** |
| All 42 guards invoked by `scripts/verify.sh quick` | **all pass**, and the four new guards' `--self-test`s still reject every injected violation |
| `cargo clippy -p codegg-core -p codegg-config -p codegg-protocol -p codegg-providers -p codegg-client --all-targets --locked -- -D warnings` | **exit 0**, zero findings — these are the crates this chain changed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **seven findings, all pre-existing on `main`** — see below |
| `cargo test -p codegg-core --lib` | **928 passed** |
| `cargo test -p codegg-core --test reasoning_overlay_narrowing` | **6 passed** |
| `cargo test -p codegg --lib core::daemon_turns` | **22 passed** |
| `cargo test -p codegg --lib tui::commands::reasoning` | **7 passed** |
| `cargo test -p codegg-providers --lib eggpool` | **30 passed** |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | **16,060 of 16,067 passed** — six failures and one timeout, **all seven reproduced identically on clean `main`** (see below) |
| `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast` (run on `main` after the fast-forward) | **12,710 of 12,716 passed** — the **same five** consistent failures and the **same** host-load timeout, and nothing else |
| `codegg validate --config codegg.example.jsonc` | **valid** |

**The seven workspace-clippy findings are `main`'s, measured rather than
assumed.** They are all in `main`'s own Linux-execution-isolation code —
`src/interactive_process.rs` (two), `src/lsp/m004.rs`,
`src/managed_process.rs` (three) and `src/security/sandbox.rs` — and none is
in a file this chain touched. Running the same clippy invocation on a **clean
checkout of `main` at `8cee65934`** produced the **identical seven findings
at the identical seven locations**, so the merge introduces **zero** new
findings. They are visible here and not in CI because they sit in
`#[cfg(target_os = "linux")]` / `#[cfg(not(target_os = "linux"))]` pairs that
Linux never compiles both sides of. This is `main`'s pre-existing macOS gap
and is recorded, not fixed, by this line.

**The suite's six failures and one timeout are `main`'s too, measured the
same way.** All seven were run **in isolation on a clean checkout of `main`
at `8cee65934`**, and the result is identical — five fail and two pass, the
same five and the same two. The five consistent failures (two
`daemon_refresh` cases, one `managed_process` case, two
`scratch_cache_root_contract` cases) are in files this chain never touched.
The two that differ between the whole-suite run and the isolated run are
**host-load artifacts** of a machine running thirty-plus concurrent cargo
builds: `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
hit the profile's 300-second slow-timeout in the suite and **passes in 0.15 s
in isolation on both trees**, and
`tool_program_m015_daemon_failpoints::active_child_is_reattached_from_pending_wait_checkpoint`
failed at 62 s in the suite and **passes in under ten seconds in isolation on
both trees**. `main`'s own closure record
(`plans/closure/managed-agent-scratch/007-status.md`) already proves the two
`daemon_refresh` cases the same way, in a clean worktree at `origin/main`,
with the same `InvalidRootPath("/tmp")` cause — on macOS `/tmp` is a symlink
to `private/tmp`, which the workspace registration rightly refuses. **The
merge introduces zero new test failures.**


**A merge-induced defect the merged tree's own tests caught.** The
`approval::tests::reasoning_selection_migration_is_additive_and_preserves_rows`
fixture built a `runtime_preferences` table that predated `main`'s v84
network-intent column, so the merged store's `get()` — which now selects that
column — failed with `no such column: network_intent`. The fixture was
corrected to state the **real** migration order: the reasoning migration is
**v88** in the merged tree and runs *after* main's v84, so a database in the
state it expects already has `network_intent`. The test was strengthened
rather than weakened — it now also asserts that main's column and its value
survive the reasoning migration.

Two more merge artifacts were caught: a `network_intent` field duplicated in
one struct literal in `src/core/daemon_ops.rs`, and a `VALUES` clause in
`crates/codegg-core/src/approval.rs` that ended the merge with 11 columns, 10
placeholders and 11 binds — a **runtime** defect the compiler cannot see.
Both are fixed. Details: the merge-reconciliation record's §7.

