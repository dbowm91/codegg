# Provider Reasoning Control Milestone 001 — Faithful reasoning intent at the wire boundary

Status: closed (ADR-0048 accepted 2026-10-21; closure record `plans/closure/provider-reasoning-control/001-status.md`)

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`)

Source roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-1--faithful-reasoning-intent-at-the-wire-boundary`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — CodeGG owns provider-facing request policy.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — capability-negotiated protocol; fail-closed on capability absence.

Applicable ADRs:

- `plans/adrs/ADR-0048-open-provider-scoped-reasoning-control-vocabulary.md`

Primary class: capability

## 1. Objective

Make CodeGG's bridge to the `eggpool-wire` kernel **lossless** for every reasoning intent, so that "explicitly disable reasoning" is a request the encoder can express and is distinct from "no reasoning preference".

This milestone changes no vocabulary, no protocol shape, and no user-visible surface. It repairs the layer at which distinguishable user intents collapse into one request.

## 2. Why this milestone is ready

- **Hard dependencies:** none. No other milestone in this roadmap depends on this one for correctness of a *later* concern; it is the foundation the rest build on.
- **ADR:** ADR-0048 is the decision this milestone implements, and it is `proposed` — see §14, which requires the agent to confirm acceptance before starting.
- **Interface dependency:** the `eggpool-wire` kernel is already an immutable-pinned dependency of `crates/codegg-providers`, and its `ir` module is public in the pinned revision. No new dependency, no pin change, and no kernel change is required.
- **Predecessor roadmaps:** the closed provider-wire-kernel consolidation roadmap established that `eggpool-wire` is the single encoder and that provider-facing request policy stays in CodeGG. This milestone preserves both.

## 3. Current implementation evidence

Repository baseline `c93211542`.

**The bridge collapses the intent.** `crates/codegg-providers/src/wire.rs`, in the canonical-request builder:

```rust
canonical.reasoning = if request.thinking_budget.is_some() || request.reasoning_effort.is_some() {
    eggpool_wire::ir::ReasoningIntent {
        requested: Some(true),
        mode: if request.thinking_budget.is_some() {
            ReasoningMode::FixedBudget
        } else {
            ReasoningMode::Effort
        },
        effort: request.reasoning_effort.clone(),
        budget_tokens: request.thinking_budget.map(|value| value as u64),
        explicit_disable: false,
    }
} else {
    eggpool_wire::ir::ReasoningIntent::default()
};
```

Three consequences:

1. `explicit_disable` is hard-coded `false`. The kernel's explicit-disable intent — `requested: Some(false)`, `mode: ReasoningMode::Toggle`, `explicit_disable: true`, available as `ReasoningIntent::disabled()` in the pinned `eggpool-wire` revision — is never constructed.
2. `ReasoningMode::Adaptive` and `ReasoningMode::Toggle` are never constructed. A binary provider thinking contract has no representation.
3. "Neither field set" produces `ReasoningIntent::default()`, whose `requested` is `None` — i.e. *unspecified*, not *disabled*.

**The request type cannot express "disable".** `crates/codegg-providers/src/provider_core.rs` defines `ChatRequest` with `reasoning_effort: Option<String>` and `thinking_budget: Option<usize>`. There is no third field that could carry an explicit disable, so the bridge cannot distinguish the two cases even if it wanted to.

**The two doc comments disagree, and the implementation sides with the wrong one.** `crates/codegg-protocol/src/core.rs` documents `ReasoningOverride::Off` as "Explicitly disable thinking for this turn", with the type-level comment "`Off` explicitly disables thinking, which is distinct from `None` on the request (inherit the configured default)". `src/agent/request_preparation.rs` documents the opposite: "`Off` clears both fields: the wire encoder treats 'neither field set' as thinking not requested (`wire.rs`), which is the only way to express an explicit disable". The implementation clears both fields, so the behaviour matches the agent-layer comment and contradicts the protocol comment. Both are wrong about the kernel, which distinguishes `requested: None` from `requested: Some(false)`.

**The kernel can already express everything this milestone needs.** In the pinned revision (`eggpool-wire` `ir.rs`), `ReasoningIntent` carries `requested: Option<bool>`, `mode: ReasoningMode`, `effort: Option<String>`, `budget_tokens: Option<u64>`, and `explicit_disable: bool`; `ReasoningMode` has the variants `Unspecified`, `Effort`, `FixedBudget`, `Adaptive`, and `Toggle`; and `ReasoningIntent::disabled()` is a provided constructor for the explicit-disable case. `ReasoningIntent::to_thinking_facts()` projects the intent into the kernel's `ThinkingFacts`, which carries `requested_toggle: Option<bool>` for the binary toggle dimension.

**Per-surface representability differs, and CodeGG's bridge decides it by accident.** The kernel's per-surface codecs accept different dimensions:

- OpenAI Chat and OpenAI Responses encode an effort label (`reasoning_effort` / `reasoning.effort`);
- Anthropic Messages encodes a fixed budget (`thinking: {type: "enabled", budget_tokens}`), an adaptive switch (`thinking: {type: "adaptive"}`), and an explicit disable (`thinking: {type: "disabled"}`), but **not** an effort label;
- Gemini GenerateContent encodes a fixed budget (`thinkingConfig.thinkingBudget`), but **not** an effort label.

Because the bridge can only emit `Effort` or `FixedBudget`, an effort selection produces **no reasoning field at all** on an Anthropic-compatible surface, and a budget selection produces **no reasoning field at all** on an OpenAI-compatible surface.

**Existing test surface.** `crates/codegg-providers/src/wire.rs` has an in-module `mod tests` with roughly thirty test functions, including `anthropic_and_gemini_surface_projection_is_structural_and_provider_native` and `request_bridge_covers_system_image_tool_history_schema_and_policy`. That module is the established home for bridge and per-surface encoding assertions. `crates/codegg-providers/tests/` is not a tracked test directory.

## 4. Invariants that must not regress

- CodeGG runs against direct providers without requiring an EggPool process.
- Provider credentials and endpoint/session-affinity policy remain CodeGG-owned and never enter `eggpool-wire`.
- One CodeGG semantic request is encoded once into the selected target grammar; provider payloads are never translated through another provider format.
- Private reasoning history is never returned to a provider unless the resolved CodeGG model/wire policy explicitly allows that provider-private round trip.
- Provider/model-specific transforms have one CodeGG authority; provider modules must not independently re-resolve model names into a second policy table.
- The shared-kernel dependency is immutable-pinned and cannot silently track EggPool `main`.
- Tool calls, usage, finish/error outcomes, cancellation, and retry classification preserve CodeGG semantics.
- The existing effort/budget mutual-exclusivity behaviour at the agent layer is unchanged by this milestone — this milestone changes the **wire bridge**, not the precedence rules (which are M002's concern).

## 5. Scope

### In scope

- The CodeGG → `eggpool-wire` reasoning-intent construction in `crates/codegg-providers`.
- The `ChatRequest` reasoning fields, insofar as an explicit-disable signal must be representable.
- The per-surface encoding fixtures proving which dimensions each surface can express.
- A narrow static guard preventing the collapse from being reintroduced.
- Correcting the two contradictory doc comments in place (protocol and agent layer) to describe the landed behaviour.

### Explicitly out of scope

- **The open vocabulary.** The effort label stays a closed enum at the protocol layer in this milestone; widening it is M002.
- **The capability record and admission policy.** Whether a selection is supported is not decided here; M003 owns that.
- **The frontend surface.** No TUI, desktop, or client change; M004 owns the user-visible surface.
- **Any change to the `eggpool-wire` pin or to the kernel itself.**
- **Any storage change.**
- **Any change to the agent-layer precedence rules** (`apply_agent_config` → `apply_model_profile_defaults` → `apply_reasoning_override`), except where a new request field must be threaded.
- **Changing which surface a provider uses.** MiniMax's Anthropic-compatible surface, OpenCode Go's multi-surface dispatch, and every other provider-to-surface mapping stay exactly as they are; the provider-surface mapping roadmap owns those.

## 6. Required production changes

### Core/domain

- `crates/codegg-providers/src/provider_core.rs` — the `ChatRequest` reasoning fields must be able to express three distinct states: no preference, an explicit disable, and a concrete selection. The shape is the implementation agent's choice, but it MUST be able to represent "explicitly disabled" as distinct from "unset", and it MUST remain `Copy`-free-safe for the string-valued effort label (a `String` field, not `&'static str`).
- `crates/codegg-providers/src/wire.rs` — the bridge must construct `ReasoningIntent` so that each state maps to the distinct kernel shape that expresses it:
  - no preference → the kernel's unspecified intent (`requested: None`);
  - explicit disable → the kernel's explicit-disable intent (`requested: Some(false)`, `mode: Toggle`, `explicit_disable: true`) — i.e. the kernel's own `ReasoningIntent::disabled()` constructor, or an equivalent literal that is asserted equal to it;
  - an effort label → mode `Effort` carrying the label verbatim;
  - a fixed budget → mode `FixedBudget` carrying the budget;
  - the adaptive/toggle-on dimension → mode `Adaptive` (or `Toggle` with `requested: Some(true)`), chosen so that a binary "thinking on" provider contract is expressible. Which of the two the bridge emits is the implementation agent's choice, but it MUST be asserted per intent, because the two are not interchangeable on every surface.
- `crates/codegg-providers/src/wire.rs` — the `explicit_disable: false` literal MUST NOT remain as a hard-coded constant in the bridge. Any construction site that sets it MUST set it from the actual intent.

### Storage and migrations

None. This milestone adds no column, no table, and no migration, and does not change `STORAGE_LAYOUT_VERSION`.

### Protocol and DTOs

- `crates/codegg-protocol/src/core.rs` — the `ReasoningOverride::Off` doc comment is corrected in place to describe the landed behaviour. The serialized shape of `ReasoningOverride` does NOT change in this milestone; the protocol change that introduces the open vocabulary is M002's.
- The protocol's `#[serde(default)]` tolerance is preserved: an absent `reasoning` field on a turn request continues to mean "inherit".

### Runtime and concurrency

None. The bridge is a pure function; the request fields are per-request values with no shared mutable state.

### Frontend or operator surface

None in this milestone. The user-visible consequence arrives through M002 (vocabulary) and M004 (surface).

### Security and authorization

- The explicit-disable signal is request-shaping only. It MUST NOT be derived from, or grant, any permission, tool, path, or credential scope.
- The bridge MUST NOT copy provider free-text beyond the effort label into any other field.

### Documentation and static guards

- `architecture/provider.md` — the "Extended-thinking control (`/reasoning`)" section records the distinction between an unspecified reasoning preference and an explicit disable, and which dimensions each surface can express. The two contradictory statements are reconciled.
- `src/agent/request_preparation.rs` — the comment claiming that clearing both fields "is the only way to express an explicit disable" is corrected in place to describe the landed behaviour.
- A narrow static guard is added that asserts the bridge contains no hard-coded `explicit_disable: false` and no closed reasoning-effort enum, so the collapse cannot be reintroduced silently. The guard is registered in `scripts/verify.sh quick` **and** in the CI `verify` job, consistent with `AGENTS.md`'s change-triggered-guard rules. The guard MUST carry a `--self-test` that injects a violating bridge and proves the guard rejects it, matching the established convention of the repository's other guards.

## 7. Ordered work packages

### Work package A — Make the explicit-disable state representable

Intent: Give the request a way to say "explicitly disable reasoning" that is distinct from "no preference".

Required changes:
- Extend the provider-facing request type so the three states are distinguishable.
- Thread the new signal from the agent layer's existing `ReasoningOverride::Off` into that request field. The agent layer's precedence rules are unchanged: `Off` still wins over agent config and model-profile defaults, and effort and budget remain mutually exclusive.

Acceptance evidence:
- A focused unit test asserts that an explicit-disable request and an unset request are distinguishable at the request level, before any encoding.
- A focused unit test asserts that an `Off` selection still overrides a configured `default_reasoning_effort` and a configured `default_thinking_budget`, and that an effort selection still clears the budget (the existing mutual-exclusivity behaviour is unchanged).

### Work package B — Build the kernel intent losslessly

Intent: Replace the collapsed bridge with a faithful projection.

Required changes:
- Implement the intent-to-kernel mapping described in §6.
- Ensure the "no preference" case remains the kernel's unspecified intent, so a turn that expresses no preference behaves exactly as it does at the baseline.

Acceptance evidence:
- A focused unit test asserts, per intent, the **exact** kernel `ReasoningIntent` produced, including that the explicit-disable intent equals the kernel's own `ReasoningIntent::disabled()`.
- A focused unit test asserts that the unspecified intent and the explicit-disable intent are **not equal**, so the distinction is proven rather than assumed.
- A focused unit test asserts that an effort label is carried verbatim into `ReasoningIntent::effort` and that a budget is carried verbatim into `budget_tokens`.
- A regression test asserts that the no-preference path produces a request byte-identical to the baseline's, so a turn that expresses no preference is unaffected.

### Work package C — Pin per-surface representability with fixtures

Intent: Prove, rather than assume, which dimensions each surface CodeGG uses can express — so that a later milestone can decide what to do when a selected dimension is inexpressible on a surface.

Required changes:
- Add per-surface encoding fixtures covering each intent on each surface CodeGG uses: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, and Gemini GenerateContent. Surfaces CodeGG reaches through another provider (for example OpenCode Go's multi-surface dispatch) are covered through the same shared encoder.
- Record, in the fixtures, the observable consequence of a dimension a surface cannot express.

Acceptance evidence:
- Per-surface fixtures assert the encoded body for each intent, including the case where a surface's encoded body contains **no** reasoning field for a dimension that surface cannot express. The fixture is written so that a future change to the bridge that makes an inexpressible dimension silently present is caught.
- A focused test asserts that the explicit-disable intent reaches the Anthropic Messages surface as an explicit `thinking: {type: "disabled"}` and that the effort intent on that surface yields no reasoning field — the difference between "the control cannot express this here" and "the control expressed it" is what the fixtures make visible.

### Work package D — Add the static guard and reconcile the documentation

Intent: Make the collapse non-reintroducible and the documentation truthful.

Required changes:
- Add the narrow static guard, register it in `scripts/verify.sh quick` and in the CI `verify` job, and give it a `--self-test` that injects a violating bridge and proves the guard rejects it.
- Correct the two contradictory doc comments in place (protocol and agent layer), and update `architecture/provider.md`.

Acceptance evidence:
- The guard passes on the tree and fails on an injected violation, with the injected case recorded.
- `scripts/verify.sh quick` passes.
- The documentation states the distinction between unspecified and explicit disable, and names the dimensions each surface can express.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** The bridge is a pure projection; there is no partial state and no failure mode beyond a construction that does not typecheck. If the kernel's intent type changes shape in a future pin, the build fails rather than silently mis-encoding — which is the intended outcome.
- **Cancellation.** Unchanged. The reasoning fields are per-request values; a cancelled turn discards them with the request.
- **Restart.** Unchanged. The reasoning fields are carried on the turn request, so a restarted daemon re-establishes them from the same admitted request. No new durable state is introduced, so no recovery policy changes.
- **Duplicate delivery.** Unchanged. A redelivered turn request carries the same fields and encodes identically.
- **Contention.** None. No shared mutable state is introduced.
- **Stale generations.** Not applicable; the fields are request-scoped values, not generation-scoped.

## 9. Compatibility and migration

- **Protocol.** No serialized shape changes in this milestone. `CoreRequest::TurnSubmit.reasoning` keeps its `#[serde(default)]` tolerance, so an absent field continues to mean "inherit" and an old client against a new daemon behaves exactly as at the baseline.
- **Provider behaviour.** This is the one behavioural change with outward visibility: on surfaces that support an explicit disable, a turn that selects "off" now sends an explicit disable where it previously sent nothing. That is the intent of the change and is the fix for the "off means unspecified" defect. On surfaces that cannot express an explicit disable, the encoded body is unchanged, and M003's capability policy is what will later make that state reportable rather than silent.
- **Storage.** None.
- **Configuration.** None. `default_reasoning_effort` and `default_thinking_budget` keep their meaning and their types.
- **No removal criteria are triggered by this milestone.** The legacy behaviour is replaced in place; there is no flag day and no deprecation window.

## 10. Required tests

### Focused unit tests

- The request level distinguishes explicit disable from unset.
- The kernel intent is asserted exactly, per intent, including the explicit-disable intent equalling `ReasoningIntent::disabled()`.
- The unspecified intent and the explicit-disable intent are asserted **not** equal.
- An effort label is carried verbatim; a budget is carried verbatim.
- The no-preference path is byte-identical to the baseline.
- The agent-layer precedence is unchanged: `Off` still beats agent config and profile defaults, and effort and budget remain mutually exclusive.

### Integration tests

- Per-surface encoding fixtures for OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, and Gemini GenerateContent, covering each intent and asserting the full encoded body's reasoning-related content.
- A regression fixture asserting that a request with no reasoning preference produces the same encoded body as the baseline.

### Restart and recovery tests

- None required by this milestone. The reasoning fields are request-scoped and introduce no durable state; the restart behaviour is unchanged and is covered by the existing suite.

### Contention and cancellation tests

- None required. No shared mutable state is introduced.

### Security and negative tests

- A test asserting that the explicit-disable signal cannot be produced by, or influence, any authorization or permission path — expressed as a negative assertion over the request construction, not as an authorization test.
- A test asserting that the bridge copies no provider free-text into any field other than the effort label.

### Migration and compatibility tests

- A test decoding a legacy `ReasoningOverride` serialization (all three existing variants) and asserting it is handled as at the baseline.
- A test asserting that an absent `reasoning` field on a turn request is treated as "inherit".

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-providers wire::tests

# the new static guard, including its self-test
python3 scripts/check_reasoning_intent_fidelity.py
python3 scripts/check_reasoning_intent_fidelity.py --self-test

# the guard must be wired into the routine suite
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets -- -D warnings

# broader suite appropriate to the change
cargo test -p codegg-providers
```

Do not claim commands that were not actually run in the closure record. The repository's existing verification policy applies: no new CI lane, no coverage or benchmark gate, no permanent external service.

## 12. Documentation updates

- `architecture/provider.md` — the "Extended-thinking control (`/reasoning`)" section: record the distinction between an unspecified reasoning preference and an explicit disable, and record which dimensions each surface can express. The section's current statement that the three configuration sites are applied in a fixed order is preserved.
- `architecture/provider.md` — the provider capability section: note that surface representability is now pinned by fixtures rather than implied by the bridge.
- `crates/codegg-protocol/src/core.rs` — the `ReasoningOverride::Off` doc comment is corrected in place.
- `src/agent/request_preparation.rs` — the comment claiming that clearing both fields "is the only way to express an explicit disable" is corrected in place.
- `docs/providers.md` — if it documents the thinking control, reconcile it with the landed behaviour.

## 13. Acceptance criteria

1. A request that expresses no reasoning preference and a request that expresses an explicit disable produce **different** kernel `ReasoningIntent` values, proven by an assertion that they are not equal.
2. The explicit-disable intent equals the kernel's own `ReasoningIntent::disabled()`, proven by assertion.
3. An effort label and a fixed budget each reach the kernel intent verbatim, with no conversion between the two and no substitution of a different label.
4. Per-surface fixtures assert, for every surface CodeGG uses, the encoded body's reasoning content for every intent — including the case where a surface cannot express a dimension and therefore emits no reasoning field.
5. The bridge contains no hard-coded `explicit_disable: false`; the static guard proves this and fails on an injected violation.
6. The static guard is registered in `scripts/verify.sh quick` and in the CI `verify` job, and carries a self-test proving it is not vacuous.
7. A request with no reasoning preference encodes byte-identically to the baseline.
8. The two contradictory doc comments about explicit disable are corrected in place, and `architecture/provider.md` describes the landed contract.
9. No protocol serialized shape, storage schema, `STORAGE_LAYOUT_VERSION`, provider-to-surface mapping, credential path, or authorization rule changed.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **ADR-0048 is not `accepted`.** This milestone implements that decision. If the ADR is still `proposed`, or if it was amended in a way that changes the intent-to-kernel mapping, the agent reports that instead of implementing a decision nobody has made.
- The pinned `eggpool-wire` revision does not expose the `ReasoningIntent` shape this milestone depends on (in particular `explicit_disable`, the `Adaptive`/`Toggle` modes, or `ReasoningIntent::disabled()`). The agent reports the actual pinned shape rather than working around it with a local approximation.
- The faithful mapping cannot be expressed without changing the `eggpool-wire` pin. A pin change is a separate decision and is not this milestone's to make.
- The explicit-disable signal cannot be represented without a protocol serialized-shape change. That change belongs to M002; the agent reports the collision rather than absorbing M002's scope.
- Any surface's encoder would need a provider-specific special case in CodeGG to express a dimension. That is a model-policy decision and belongs to M003's capability record; the agent records the finding and does not build a per-provider branch.
- Repository evidence contradicts a canonical invariant in §4.
- The work would expand into the frontend surface, the capability record, or the vocabulary — all of which are other milestones.

## 15. Closure evidence required

The closure record (`plans/closure/provider-reasoning-control/001-status.md`) must contain:

- the requirement-to-evidence matrix covering all nine acceptance criteria;
- the exact kernel `ReasoningIntent` values asserted per intent, quoted from the test output or the closure's own reproduction;
- the per-surface fixture results for every surface CodeGG uses, with the encoded reasoning content for each intent;
- the static guard's pass result on the tree **and** its rejection of an injected violation, plus its `--self-test` result;
- the `scripts/verify.sh quick` result;
- the Clippy and `cargo fmt` results for the touched crate;
- the `cargo test -p codegg-providers` result;
- an explicit statement of which surfaces can express which dimensions, and that a surface which cannot express a dimension is recorded as such rather than silently accepted;
- a statement that the no-preference path is byte-identical to the baseline;
- the documentation changes made, listed by file;
- any finding the milestone surfaced for M002 or M003, recorded rather than absorbed;
- an explicit note that no storage, protocol-shape, pin, or authorization change occurred.

A closure that shows only compilation, or that reports the fixtures without their contents, is not a closure.

## 16. Handoff notes

- **The narrowest useful target is `crates/codegg-providers`.** The reasoning bridge, the request type, and the per-surface fixtures all live there, and its in-module `mod tests` is the established home for bridge assertions.
- **`crates/codegg-providers/tests/` is not a tracked test directory** — it exists only as an untracked local artifact. Put new tests in `crates/codegg-providers/src/wire.rs`'s `mod tests`, alongside `anthropic_and_gemini_surface_projection_is_structural_and_provider_native`.
- **The kernel's constructors are the reference.** `ReasoningIntent::disabled()` in the pinned revision is the authority for the explicit-disable shape; assert against it rather than against a hand-written literal, so a future kernel change is caught as a test failure rather than as a silent divergence.
- **Do not edit the pinned kernel checkout.** It lives in the cargo git cache and is shared with other crates; the checkout at `~/.cargo/git/checkouts/eggpool-2a2610ca2de951f5/9ac6a13/` is read-only for this work.
- **Watch the two doc comments.** They are the clearest statement of the current defect, and a mechanical reader may "fix" one without the other. Both must land together with the behaviour change.
- **The `explicit_disable: false` literal may appear in more than one construction site.** Search the whole crate for it, not just the canonical-request builder.
- **Do not widen scope to the vocabulary.** If implementing an open label feels like it requires changing the protocol enum, that is M002 — report it and stop.
