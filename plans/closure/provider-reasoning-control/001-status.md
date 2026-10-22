# Provider Reasoning Control Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-reasoning-control/001-faithful-reasoning-intent-at-the-wire-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-001--faithful-reasoning-intent-at-the-wire-boundary`

Source decision:

- `plans/adrs/ADR-0048-open-provider-scoped-reasoning-control-vocabulary.md` (accepted)

Repository baseline reviewed: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5`

Implementation commits or pull requests:

- none — the change is left uncommitted in the working tree, as instructed for
  this milestone.

## 1. Executive finding

Milestone 001 is closed. CodeGG's provider bridge now projects the request's
reasoning intent onto the pinned kernel's canonical `ReasoningIntent`
**losslessly**, so the four dimensions the kernel can express — no preference,
explicit disable, effort label, fixed budget — plus the binary thinking-on
dimension survive to the encoder instead of collapsing into two shapes.

At the baseline the bridge built `ReasoningIntent` from two nullable request
fields alone and hard-coded `explicit_disable: false` at its single
construction site. The kernel's explicit-disable intent was therefore never
constructed, "off" and "no preference" were the same request on the wire, and
`ReasoningMode::Adaptive` / `ReasoningMode::Toggle` were unreachable. Because
only `Effort` and `FixedBudget` were reachable, an effort selection was a
**no-op on Anthropic-compatible surfaces** and a budget selection was a **no-op
on OpenAI-compatible surfaces** — silently, with the request looking honoured.

The landed request type keeps the two existing selection fields (the effort
label stays a `String`, the budget stays a token count, both carried verbatim)
and adds the one state they cannot express: an explicit instruction. The
explicit disable is the **kernel's own constructor**,
`ReasoningIntent::disabled()`; no construction site writes
`explicit_disable: false`. A static guard enforces that, the absence of a
closed CodeGG effort vocabulary in the provider crate, and the continued
presence of every mode in the bridge.

## 2. Requirement-to-evidence matrix

| # | Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | No-preference and explicit-disable produce **different** kernel intents | `wire::tests::no_preference_and_explicit_disable_are_different_kernel_intents` — `assert_ne!(unspecified, disabled)` | pass | the two differ in `requested` (`None` vs `Some(false)`) and in `explicit_disable` (`false` vs `true`) |
| 2 | The explicit-disable intent equals the kernel's `ReasoningIntent::disabled()` | same test — `assert_eq!(disabled, eggpool_wire::ir::ReasoningIntent::disabled())` | pass | the bridge calls the kernel constructor; it never hand-rolls the literal |
| 3 | Effort label and fixed budget each reach the kernel intent verbatim, with no conversion between them | `wire::tests::effort_label_and_budget_reach_the_kernel_intent_verbatim` | pass | effort: `mode: Effort`, `effort: Some("xhigh")`, `budget_tokens: None`; budget: `mode: FixedBudget`, `budget_tokens: Some(2048)`, `effort: None` |
| 4 | Per-surface fixtures assert the encoded body's reasoning content for every intent, including the no-reasoning-field case | `wire::tests::reasoning_intent_encodes_a_per_surface_fixture` (4 surfaces × 5 intents) and `wire::tests::no_reasoning_preference_encodes_no_reasoning_field_on_any_surface` | pass | contents in §3.3. **Strengthened after the independent audit (§11):** a surface recorded as inexpressible must now carry **no reasoning content anywhere in the body**, not merely no field at the one expected path — a differently-spelled field would previously have passed as "inexpressible" |
| 5 | The bridge contains no hard-coded `explicit_disable: false`; the guard proves this and fails on an injected violation | `scripts/check_reasoning_intent_fidelity.py` — rule 1 scans the whole provider crate; `--self-test` injects the literal and the guard rejects it | pass | output in §4. **Hardened after the independent audit (§11):** the scanner is now string-aware, so a literal hidden behind a `"/*"` string literal is no longer invisible, and a new rule rejects a **hand-rolled** disable (`requested: Some(false)`, `explicit_disable: true`, `ReasoningMode::Toggle`), which writes no `explicit_disable: false` at all |
| 6 | The guard is registered in `scripts/verify.sh quick` and the CI `verify` job, and carries a non-vacuous self-test | `scripts/verify.sh` (quick tier, after `check_provider_wire_cutover.py`), `.github/workflows/ci.yml` (`verify` job); `--self-test` injects **seven** distinct violations | pass | no new CI lane; CI remains a subset of quick. **Hardened after the independent audit (§11):** the self-test now also injects a constructor reachable only through a string literal, a literal hidden behind a `"/*"` string literal, a hand-rolled explicit disable, and a closed effort-label ladder |
| 7 | A request with no reasoning preference encodes byte-identically to the baseline | `wire::tests::no_reasoning_preference_encodes_no_reasoning_field_on_any_surface` — the intent is `ReasoningIntent::default()` and no surface carries a reasoning field | pass | the baseline request shape (no control, no selection) is the `Unspecified` default, so the projection is identical |
| 8 | Both contradictory doc comments corrected in place; `architecture/provider.md` describes the landed contract | `crates/codegg-protocol/src/core.rs` (`ReasoningOverride` + `Off`), `src/agent/request_preparation.rs` (`apply_reasoning_override` / `apply_reasoning_choice`), `src/agent/title.rs` (a **third** stale claim, found and corrected by the independent audit), `architecture/provider.md` | pass | see §9 |
| 9 | No protocol serialized shape, storage schema, `STORAGE_LAYOUT_VERSION`, provider-to-surface mapping, credential path, or authorization rule changed | `git diff --stat` over `crates/codegg-protocol` (comment only), no storage file touched, no `STORAGE_LAYOUT_VERSION` edit, provider modules unchanged, no credential or authorization file touched | pass | see §7 |

## 3. Production implementation evidence

### 3.1 The request type that was chosen

`crates/codegg-providers/src/provider_core.rs`:

```rust
/// The explicit reasoning control a provider request carries.
pub enum ReasoningControl {
    /// No explicit instruction: the request's own selection, or the
    /// provider's default, applies.
    #[default]
    Unspecified,
    /// Reasoning was explicitly turned off for this request.
    Disabled,
    /// Reasoning is on with no depth label.
    Enabled,
}

pub struct ProviderRequestContext {
    pub session_id: Option<Arc<str>>,
    pub wire_policy: Option<Arc<ProviderWirePolicy>>,
    pub reasoning: ReasoningControl,
}

pub struct ChatRequest {
    // …
    /// A fixed thinking-token budget, carried to the wire verbatim.
    pub thinking_budget: Option<usize>,
    /// A provider-scoped reasoning effort label, carried to the wire verbatim.
    pub reasoning_effort: Option<String>,
    pub context: ProviderRequestContext,
}
```

Shape notes:

- The **effort label remains a `String`** (`ChatRequest::reasoning_effort`), not
  a `&'static str` and not a closed enum. A provider-scoped label such as
  `xhigh` reaches the wire verbatim.
- The **fixed budget** remains a token count (`ChatRequest::thinking_budget`),
  carried verbatim.
- The **explicit instruction** — the one state the two selection fields cannot
  express — is `ProviderRequestContext::reasoning`. It is deliberately *not*
  merged into `ProviderWirePolicy`: that struct is the model-adapter
  projection (`enable_thinking` is the adapter's own thinking-parameter
  projection), while `ReasoningControl` is the request's own instruction.
- The public path for the type is `codegg_providers::wire::ReasoningControl`
  (re-exported from `provider_core` by the wire module, which owns the
  mapping).
- Existing call sites stay ergonomic: the two selection fields are unchanged, and
  every `ProviderRequestContext` literal in the repository used
  `..Default::default()` **except one** — the exhaustive test literal in
  `src/agent/title.rs`, which the compiler required to be updated (§10).

### 3.2 The intent → kernel mapping (the exact asserted values)

`crates/codegg-providers/src/wire.rs`, `fn reasoning_intent`:

| CodeGG request | kernel `ReasoningIntent` (exact field values) |
|---|---|
| no preference (`Unspecified`, no selection) | `ReasoningIntent::default()` → `requested: None`, `mode: Unspecified`, `effort: None`, `budget_tokens: None`, `explicit_disable: false` |
| explicit disable (`ReasoningControl::Disabled`) | `ReasoningIntent::disabled()` → `requested: Some(false)`, `mode: Toggle`, `effort: None`, `budget_tokens: None`, `explicit_disable: true` |
| effort label (`reasoning_effort = Some("xhigh")`) | `requested: Some(true)`, `mode: Effort`, `effort: Some("xhigh")`, `budget_tokens: None`, `explicit_disable: false` (inherited from the kernel default) |
| fixed budget (`thinking_budget = Some(2048)`) | `ReasoningIntent::fixed(2048)` → `requested: Some(true)`, `mode: FixedBudget`, `effort: None`, `budget_tokens: Some(2048)`, `explicit_disable: false` (inherited from the kernel default) |
| thinking on, no depth (`ReasoningControl::Enabled`) | `requested: Some(true)`, `mode: Adaptive`, `effort: None`, `budget_tokens: None`, `explicit_disable: false` (inherited from the kernel default) |

Two properties are load-bearing and are asserted:

- The explicit disable is the kernel's **own constructor**. Every other case
  inherits the kernel default, so `explicit_disable` is never written as a
  literal at a construction site.
- Effort and budget never convert into one another: the budget branch never
  carries an effort label, and the effort branch never carries a budget.

### 3.3 The per-surface fixture results

The fixture asserts the encoded body's reasoning content for every intent on
every surface CodeGG uses. `(none)` means the surface **cannot express that
dimension and therefore emits no reasoning field at all** — recorded as such,
not silently accepted.

| intent | OpenAI Chat Completions | OpenAI Responses | Anthropic Messages | Gemini GenerateContent |
|---|---|---|---|---|
| no preference | (none) | (none) | (none) | (none) |
| explicit disable | `reasoning_effort: "none"` | `reasoning: {"effort": "none"}` | `thinking: {"type": "disabled"}` | `generationConfig.thinkingConfig: {"thinkingBudget": 0}` |
| effort label `xhigh` | `reasoning_effort: "xhigh"` | `reasoning: {"effort": "xhigh"}` | (none) | (none) |
| fixed budget `2048` | (none) | (none) | `thinking: {"type": "enabled", "budget_tokens": 2048}` | `generationConfig.thinkingConfig: {"thinkingBudget": 2048}` |
| thinking on, no depth | (none) | (none) | `thinking: {"type": "adaptive"}` | (none) |

Which surface expresses which dimension, stated explicitly:

| dimension | OpenAI Chat | OpenAI Responses | Anthropic Messages | Gemini GenerateContent |
|---|---|---|---|---|
| explicit disable | yes (`reasoning_effort: "none"`) | yes (`reasoning: {"effort":"none"}`) | yes (`thinking: disabled`) | yes (`thinkingBudget: 0`) |
| effort label | yes (`reasoning_effort`) | yes (`reasoning.effort`) | **no** | **no** |
| fixed budget | **no** | **no** | yes (`thinking.budget_tokens`) | yes (`thinkingBudget`) |
| thinking on, no depth | **no** | **no** | yes (`thinking: adaptive`) | **no** |

Surface representability is the pinned kernel's authority, not CodeGG's; the
fixtures pin it rather than leaving it implied by the bridge.

### 3.4 The agent layer

`src/agent/request_preparation.rs`:

- `apply_reasoning_choice` now sets `request.context.reasoning`:
  `Off` → `ReasoningControl::Disabled`; `Effort { .. }` and `Budget { .. }` →
  `ReasoningControl::Unspecified`.
- The **precedence rules are unchanged**: `apply_agent_config` →
  `apply_model_profile_defaults` → `apply_reasoning_override`, `Off` still wins
  over agent config and profile defaults, and effort and budget remain
  mutually exclusive.
- `Off` still clears **both** `reasoning_effort` and `thinking_budget`; the
  added explicit-disable mark is what carries "off" to the wire. Clearing the
  fields alone is no longer described as the way to express a disable.

Two focused tests were added:

- `agent::request_preparation::tests::reasoning_off_marks_the_request_as_an_explicit_disable`
  — the disable is marked **and** distinguishable from an unset request.
- `agent::request_preparation::tests::an_explicit_selection_leaves_the_request_unmarked`
  — a concrete effort or budget selection is not an explicit disable.

The pre-existing `reasoning_effort_choice_overrides_a_configured_budget`,
`reasoning_budget_choice_overrides_a_configured_effort`, and
`reasoning_off_clears_both_configured_fields` tests are unchanged and still
pass, so the mutual-exclusivity behaviour is unchanged.

### 3.5 The OpenAI-compatible adapter

`crates/codegg-providers/src/openai_compatible.rs`: the generic
OpenAI-compatible encoder strips `reasoning_effort`, so the thinking toggle
(`chat_template_kwargs.enable_thinking`) is the only field on that surface
that can carry the request's instruction. Its wire value used to be computed
as `request.thinking_budget != Some(0)`, which **could never be false** — a
zero budget is not a request the kernel accepts — so "off" was inexpressible
there. It now reads the request's resolved intent and is `false` only when the
request explicitly disables reasoning. This is not a provider-specific branch
for a capability decision: it is the existing transform reading the request's
own instruction. Whether an always-on model may be told "off" is a separate
authority and is **not** decided here (see §10).

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo test -p codegg-providers
cargo test -p codegg --lib agent::request_preparation
cargo check -p codegg
cargo clippy -p codegg --all-targets -- -D warnings
python3 scripts/check_reasoning_intent_fidelity.py
python3 scripts/check_reasoning_intent_fidelity.py --self-test
```

### Results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass — no diff |
| `cargo clippy -p codegg-providers --all-targets -- -D warnings` | pass — no warnings |
| `cargo test -p codegg-providers` | pass — 280 passed, 0 failed |
| `cargo test -p codegg --lib agent::request_preparation` | pass — 23 passed, 0 failed |
| `cargo check -p codegg` | pass |
| `cargo clippy -p codegg --all-targets -- -D warnings` | pass — no warnings |
| `python3 scripts/check_reasoning_intent_fidelity.py` | pass — 5/5 checks |
| `python3 scripts/check_reasoning_intent_fidelity.py --self-test` | pass — 7 injected violations rejected |

Guard output on the tree:

```text
  ok: the provider crate carries no hard-coded `explicit_disable: false`
  ok: the provider crate carries no closed reasoning-effort enum
  ok: the provider crate carries no closed effort-label ladder
  ok: the bridge reaches the unspecified intent, the explicit disable, and every mode
  ok: the bridge does not hand-roll the explicit disable
check_reasoning_intent_fidelity: all checks pass
```

Guard self-test output (after the independent audit hardened it — see §11):

```text
  ok: injected hard-coded `explicit_disable: false` rejected
  ok: injected closed reasoning-effort enum rejected
  ok: injected dropped explicit-disable constructor rejected
  ok: injected string-literal constructor rejected
  ok: injected string-hidden explicit-disable literal rejected
  ok: injected hand-rolled explicit disable rejected
  ok: injected closed effort-label ladder rejected
  ok: unmodified tree still passes
```

`scripts/verify.sh quick` was **not** run: the milestone assignment
explicitly reserves it for the integrator to avoid cargo target-directory lock
contention. The guards it would run for this change were run individually and
their results are above.

## 5. Invariant review

| Source invariant (roadmap §2 / plan §4) | Evidence | Result |
|---|---|---|
| CodeGG owns the request-side policy; the kernel is the encoder, not the policy authority | `wire.rs` only *projects* CodeGG's intent onto the kernel's canonical intent; no surface decision is made in CodeGG | pass |
| The effort vocabulary is open and provider-scoped | the effort label is a `String` carried verbatim; the guard rejects a closed effort enum in the provider crate | pass |
| No silent surface loss: a dimension a surface cannot express is recorded, not dropped | the per-surface fixture asserts `(none)` for every inexpressible dimension | pass |
| The kernel is the single authority on which dimensions a surface can express | the fixtures read the kernel's own encoding; CodeGG has no per-surface reasoning branch | pass |
| No protocol, storage, pin, or authorization change | §7 | pass |
| The bridge stays the only semantic-request → target-grammar encoder | `wire.rs` remains the only place a `ReasoningIntent` is built | pass |

## 6. Failure and recovery review

- **Malformed input**: the effort label is an opaque `String`; no parsing, no
  validation against a list, and no translation. A label the model rejects is
  a capability question (M003), not a CodeGG-side parse.
- **Zero budget**: `thinking_budget = Some(0)` remains the request's own value.
  The milestone did **not** introduce a zero-budget encoding of "off" — the
  explicit disable is a distinct state. The kernel's own
  `validate_reasoning` continues to reject a zero budget on the adaptation
  path.
- **Bounded behaviour**: the projection is a pure function of the request; it
  allocates one `String` clone for the effort label and nothing else.
- **Duplicate delivery / idempotency**: not applicable — the projection is
  stateless and deterministic.
- **Cancellation, restart, contention, persistence**: not applicable — no
  durable state, no queue, no lease was touched.

## 7. Migration and compatibility review

- **Protocol serialized shape**: unchanged. `ReasoningOverride` keeps
  `Effort { level }`, `Budget { tokens }`, `Off` with
  `#[serde(tag = "mode", rename_all = "snake_case")]`. Only a doc comment
  changed in `crates/codegg-protocol/src/core.rs`.
- **Storage**: no storage file touched; no `STORAGE_LAYOUT_VERSION` change; no
  migration. The new field is a per-request, in-memory value and is never
  serialized or persisted.
- **`eggpool-wire` pin**: unchanged at
  `9ac6a1318e8db3c034b5ab54987317752d5ffea6`. The checkout was read only.
  The kernel's `ReasoningIntent::default()`, `::disabled()`, `::fixed(..)`,
  `ReasoningMode::{Effort, FixedBudget, Adaptive, Toggle}`, and the per-surface
  codec behaviour were read from the pinned revision.
- **Provider-to-surface mapping**: unchanged. MiniMax remains on the
  Anthropic surface; OpenCode Go keeps its multi-surface dispatch; no provider
  module was modified.
- **Credential path / authorization**: unchanged.
- **Backward compatibility**: a request built by an older caller — one that
  never sets the new field — is `Unspecified` by default, which is exactly the
  baseline behaviour. The baseline encoding is byte-identical (§3.3, row 1).

## 8. Security review

- No credential, secret, path, or network handling was added.
- The explicit disable narrows what is sent to a provider; it cannot widen
  authority.
- The guard is a static, read-only check: it reads source files and never
  writes, executes, or mutates the tree. Its self-test operates on in-memory
  copies of the sources.
- No new tool, capability, or privilege was introduced.

## 9. Documentation and operations

| File | Change |
|---|---|
| `architecture/provider.md` — "Extended-thinking control (`/reasoning`)" | the claim that clearing both fields "is the only way to express an explicit disable" is replaced with the landed contract: `Off` clears both fields **and** marks the request as an explicit disable, and the four surface shapes that carry a disable are listed |
| `architecture/provider.md` — new "Reasoning intent at the wire boundary" subsection | the intent → kernel mapping table, the "the kernel's own constructor, not a literal" rule, the guard, the per-surface fixture rule, and the OpenAI-compatible toggle correction |
| `architecture/provider.md` — provider capability section | notes that surface representability is pinned by fixtures rather than implied by the bridge, and that the per-model facts belong to a separate authority |
| `architecture/provider.md` — `ChatRequest` / `ProviderRequestContext` sketches | updated with `ReasoningControl` and the new field, and the note that the wire policy is adapter policy, not request intent |
| `crates/codegg-protocol/src/core.rs` | `ReasoningOverride`'s doc comment and the `Off` variant's doc comment corrected in place. `Off` is now described as distinct from the *absent* override (this enum has no `None` variant), instead of "distinct from `None` on the request" |
| `src/agent/request_preparation.rs` | `apply_reasoning_override`'s and `apply_reasoning_choice`'s comments corrected in place: clearing the fields is not sufficient on its own, and the explicit-disable mark is what reaches the wire |
| `src/agent/title.rs` | the doc comment above `title_request_context` corrected in place: "neither set" is the encoding for *no preference*, not the only encoding for a disable — the explicit disable is the request's own `ReasoningControl::Disabled` mark (found by the independent audit; see §11) |
| `docs/providers.md` | **not changed** — it does not document the thinking control (no occurrence of "reasoning" or "thinking" in the file) |

Static guards added:

- `scripts/check_reasoning_intent_fidelity.py` — five rules: no hard-coded
  `explicit_disable: false` anywhere in the provider crate; no closed
  reasoning-effort enum in the provider crate; no closed effort-label **ladder**
  in match-arm position; the bridge still reaches the unspecified intent, the
  explicit disable, and the effort, budget, and adaptive modes **in code**; and
  the bridge does not hand-roll the explicit disable. The scanner is
  **string-aware** — literals are consumed before comment delimiters are
  recognised, and literal contents can be blanked so that a token appearing
  only inside a string cannot satisfy a "does the code use this?" rule.
  `--self-test` injects **seven** distinct violations and proves each is
  rejected, then proves the unmodified tree still passes. The four injected
  after the independent audit are documented in §11.
- Registered in `scripts/verify.sh` **quick** (guard + `--self-test`) and in
  the CI **`verify`** job in `.github/workflows/ci.yml` (guard + `--self-test`).
  No new CI lane and no coverage/benchmark gate was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The generic OpenAI-compatible adapter now sends `chat_template_kwargs.enable_thinking: false` when the request explicitly disables reasoning, even when the adapter pinned thinking **on** (an always-on model). At the baseline the value was always `true` because the old test could never be false. | Faithful expression of the intent, but an always-on model that rejects "off" would now see a provider-side error instead of a locally-refused selection. | **M003** — the per-model capability record and the admission policy must decide whether an always-on model may be told "off", and refuse locally rather than at the provider. Not absorbed here: that is a model-policy decision. |
| low | `ReasoningControl::Enabled` (the binary thinking-on dimension, projected as kernel mode `Adaptive`) is expressible and reachable at the bridge, but no current agent-layer selection produces it. | The dimension is wired end-to-end at the wire boundary, but the frontend/protocol vocabulary that would select it does not exist yet. | **M002** — the open, daemon-owned selection vocabulary. Deliberately not absorbed. |
| low | The protocol's `ReasoningEffort` is still a closed four-level enum, so the *protocol* vocabulary remains closed even though the *wire* label is an open `String`. | A vendor label such as `xhigh` is expressible on the wire but cannot yet be selected through the protocol. | **M002** — widening the protocol's closed effort enum to an open label set. Deliberately not absorbed; the protocol's serialized shape was not changed in this milestone. |
| low | Azure, OpenRouter, and Zen strip the reasoning projection, so an explicit disable is stripped there too. | Pre-existing, documented adapter divergence; unchanged by this milestone. | None in this milestone; the divergence is intentional and asserted by existing tests. |
| low | The milestone added one field to `ProviderRequestContext`, which forced a one-line mechanical update to the single exhaustive `ProviderRequestContext` literal in the repository (`src/agent/title.rs`, a test literal, now `..Default::default()`). | None; the compiler required it. | None. || low | `architecture/provider.md`'s `provider_core.rs:line` references were **already** stale at the baseline (e.g. `ChatRequest` is documented at `:238` and actually sits at `:264`). This milestone's insertion of `ReasoningControl` shifts the affected items further (e.g. `ChatRequest` is now at `:305`). | The references point at the right *items* but at the wrong lines; a reader following one lands on the neighbouring type. The staleness is not newly introduced by this milestone, and correcting it would mean rewriting unrelated references. | A separate documentation cleanup of `architecture/provider.md`'s `provider_core.rs:line` references. Deliberately not absorbed here. |

## 11. Independent audit and follow-up corrections

An independent verifier audited this milestone against the pinned kernel, the
plan's nine criteria, and the guard itself, and was asked to **refute** this
record rather than confirm it. Its verdict was **partial**: seven criteria pass
cleanly, one has a documentation gap, and the guard's coverage was materially
weaker than this record claimed.

### What the audit confirmed (no defect)

- **The per-surface fixture table is correct — 20 of 20 cells.** Every cell was
  re-derived from the pinned kernel's own encoders, not from this record:
  `codecs.rs:287-298` (OpenAI Chat `reasoning_effort:"none"`),
  `additional_codecs.rs:499-502` (Responses `{"effort":"none"}`),
  `codecs.rs:373-379` (Anthropic `disabled` / `adaptive`),
  `codecs.rs:380-386` (Anthropic `enabled` + budget),
  `additional_codecs.rs:588-593` (Gemini `thinkingBudget` 0 / 2048). Those four
  surfaces are exactly the four `WireSurface` variants used repo-wide.
- **The no-preference path is byte-identical to the baseline.** The diff is four
  hunks; the only behavioural change is `canonical.reasoning =
  reasoning_intent(request)`, and that returns `ReasoningIntent::default()` for
  the no-preference case.
- **The OpenAI-compatible toggle is correct and behaviour-preserving.** The
  record's "could never be false" claim is accurate: the kernel rejects
  `budget_tokens == Some(0)` on the encode path, and the toggle is not reached
  for such a request. A zero budget is a hard error, not a disable.
- **No storage, pin, provider-to-surface, credential, or authorization change.**
  `crates/codegg-core/` is untouched by this milestone, `STORAGE_LAYOUT_VERSION`
  is 83, the `eggpool-wire` pin is unchanged, and `minimax-m3 → AnthropicMessages`
  is intact.
- **The guard's registration is correct**: the quick tier and the existing CI
  `verify` job, with no `continue-on-error` and no new lane.
- **The tests are deterministic and non-tautological**: no async, sleep,
  network, or IO; the expectations are hard-coded literals rather than
  recomputed from the expression under test.

### What the audit found, and what was done about it

| Finding | Severity | Disposition |
|---|---|---|
| The guard's mode check was **token presence**, so a bridge whose only mention of `ReasoningIntent::disabled()` was inside a string literal passed | medium | **Fixed.** The scanner is now string-aware and blanks literal contents, so the check is about code. The self-test injects this bypass. |
| The guard's comment stripper was string-blind, so a file containing `const MARKER: &str = "/*";` made the rest of the file read as a block comment and hid a hard-coded `explicit_disable: false` | medium | **Fixed.** Literals are consumed before comment delimiters are recognised. The self-test injects this bypass. |
| A **hand-rolled** disable — `ReasoningIntent { requested: Some(false), mode: ReasoningMode::Toggle, .. }` — writes no `explicit_disable: false` and so passed the existing rules | medium | **Fixed.** A new rule rejects `requested: Some(false)`, `explicit_disable: true`, and `ReasoningMode::Toggle` in the bridge. The self-test injects this bypass. |
| A closed effort **ladder** in match-arm position (`"low" =>`, `"medium" =>`, `"high" =>`) is the same closed vocabulary as an enum | medium | **Fixed.** A new rule rejects three or more distinct ladder labels in match-arm position. The self-test injects this bypass. |
| `src/agent/title.rs` carried a **third** stale doc comment — it still called "neither set" "the only encoding the bridge supports for a disable" | low | **Fixed.** Reworded in place. |
| The fixture's "(none)" cells were a negative assertion on **one exact JSON path**, so a differently-spelled field would pass as "inexpressible" | low | **Fixed.** A surface recorded as inexpressible must now carry no reasoning content **anywhere in the body**. |
| This record's §3.1 claimed every `ProviderRequestContext` literal already used `..Default::default()`, while §10 said one did not | low | **Fixed.** §3.1 now says one did not. |
| The baseline carried `effort: Some(label)` alongside `mode: FixedBudget`; the new bridge drops it | nit | **Accepted, not changed.** The agent layer can set both fields, and the new bridge keeps them mutually exclusive — which is the contract this milestone states. No wire surface emits `effort` in `FixedBudget` mode, so there is no wire effect. |
| `architecture/provider.md`'s `provider_core.rs:line` references were already stale at the baseline | nit | **Deferred**, as §10 records. Correcting them means rewriting unrelated references; it is a documentation cleanup, not this milestone. |

The guard now has **five** rules and a self-test that injects **seven**
violations. Its output on the tree and its self-test result are in §4.

### One thing this record cannot assert

Criterion 9 ("no protocol serialized shape changed") is true of **this
milestone's** delta, which the audit captured before the next milestone began:
a doc-comment-only change of 13 lines in `crates/codegg-protocol/src/core.rs`.
It is **not** a property of the merged tree, because M002 replaces the closed
`ReasoningEffort` enum with an open provider-scoped label — a deliberate,
separately-recorded protocol change. The two milestones must be read
separately: M001 did not change the wire protocol's shape; M002 did, on purpose,
under ADR-0048's capability-negotiation requirement.

## 12. Roadmap disposition

Milestone closed and the next dependency may proceed. The wire boundary is
faithful for every dimension the pinned kernel can express, the loss is
guarded against recurrence, and the two findings that belong to M002 (the open
protocol vocabulary) and M003 (the per-model capability and admission policy)
are recorded rather than absorbed.

## 13. Registry updates

Required in `plans/registry.md` and in
`plans/subsystems/provider-reasoning-control-roadmap.md` (both owned by the
integrator, not by this milestone):

- record Milestone 001 as closed, with this closure record as its evidence;
- carry forward the two findings in §10 (M002: open protocol vocabulary and
  the `Enabled` selection; M003: the always-on model's admission policy).
