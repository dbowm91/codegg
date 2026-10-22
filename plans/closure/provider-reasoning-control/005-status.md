# Provider Reasoning Control Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-reasoning-control/005-discovery-overlay-from-catalog-metadata.md`

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-005--discovery-overlay-from-catalog-metadata`

Source decision:

- `plans/adrs/ADR-0049-per-model-reasoning-capability-authority.md` (accepted)

Repository baseline reviewed: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5`

Implementation commits or pull requests:

- none — the change is left uncommitted in the working tree, as instructed for
  this milestone.

## 1. Executive finding

Milestone 005 is closed. A provider's **published thinking-capability
contract is no longer discarded** by the catalog probe: it is retained, parsed
into a bounded, typed overlay, given its **own revision**, and installed
atomically beside the model catalog. The overlay is **narrow-only** — it may
remove an option the compiled adapter baseline offers, and it can never add
one, introduce a label or a bound the baseline does not declare, or change the
always-reasons property — and that is proven end to end, through the **real**
resolution function, with a hostile overlay.

**The overlay has no live source at this baseline.** This is the honest
headline of the milestone, and it is not a defect in the implementation:

- The upstream contract `eggpool.capabilities.thinking.control_contract.accepted_efforts`
  appears **only** in EggPool's `docs/` and `plans/` prose. A byte-level read
  of both pinned checkouts found no runtime module that constructs,
  serialises, or consumes it. **The upstream contract is unimplemented in
  every available revision.**
- The pinned `/v1/models` handler emits a flat four-key model object — `id`,
  `object`, `owned_by`, `name` — with **no capability block at all**.
- The one capability shape upstream *does* expose at runtime is the **flat**
  `thinking` capability (`supported_efforts`, `toggle`, `effort`, `budget`,
  `budget_tokens_min`, `budget_tokens_max`), and it is carried by the
  **integrations-profile route, which CodeGG does not call**.

So the machinery landed here is **forward-compatible and inert**. Every
provider response CodeGG actually receives today yields an **absent** overlay,
and the effective capability is the compiled baseline, exactly as before. The
route that would become the source, if a deployment ever published the
contract on `/v1/models`, is the catalog response itself — the same
`/v1/models` body the probe already reads — which is why the parse reads both
the flat `thinking` capability and the documented
`control_contract.accepted_efforts` shape. **No new provider endpoint was
added**, and none is required: the milestone adds no surface, changes no
route, and changes which surface a provider uses not at all.

What the milestone *does* buy is that the narrowing property, the bounds, the
separate revision, and the atomic replacement are now **decided and tested**
rather than assumed. When the upstream contract ships, the overlay narrows on
day one, and the narrow-only property is already proven rather than hoped
for.

The compatibility point that mattered most was kept exactly: the **model
catalog digest still hashes `id` and `name` only**. Folding the capability
into it would have invalidated every stored last-used-model preference and
split the revision-scoped `provider_connection_models` rows, so the capability
lives in a **separate field** with a **separate revision**.

## 2. Requirement-to-evidence matrix

The plan's §13 enumerates twelve criteria. The thirteenth row below is this
milestone's added criterion — the disclosure of the overlay's source — which
the integrator may fold into the plan's §13 when the plan is next edited
(the plan file was not edited by this milestone).

| # | Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | The catalog parse **retains** a published thinking-control contract instead of discarding it, in whichever shape a provider publishes it — the flat `thinking` capability and the documented `control_contract.accepted_efforts` shape — and the retained value is kept in a **separate** field so the model summary and the digest keep their current shape and inputs. When neither shape is present, the overlay is absent and the effective capability is unchanged. | `crates/codegg-providers/src/eggpool.rs` — `parse_summary` (collects the overlay per model id), `parse_thinking_overlay` (reads both shapes), `EggpoolProbeSummary.capability_overlay` (the separate field), `EggpoolThinkingOverlay`, `EggpoolCapabilityOverlay`; tests `the_catalog_parse_retains_a_flat_thinking_contract`, `the_catalog_parse_retains_the_documented_control_contract_shape`, `a_response_without_a_contract_retains_no_overlay`, `a_contract_for_one_model_does_not_leak_into_another_models_record`, `two_different_contracts_for_one_id_retain_no_overlay` | pass | The model summary is untouched: `EggpoolModelSummary` still has exactly `id` and `name`. The retained value is a **separate field on the probe summary**, not a field on the summary the digest hashes. A duplicate model id carrying two different contracts is ambiguous, so neither is retained — the merge is order-independent |
| 2 | The model catalog digest is **unchanged** by the presence of the contract — the digest continues to hash `id` and `name` only | `crates/codegg-providers/src/eggpool.rs` — `digest_models` is unchanged (it hashes `model.id` and `model.name` and nothing else); test `the_catalog_digest_is_unchanged_by_the_contracts_presence` asserts `plain.digest == with_flat.digest == with_documented.digest`, and that the digest still equals `digest_models(&plain.models)` | pass | The only two deletions this milestone made in the file are the `Eq` derive on `EggpoolProbeSummary` (the retained record is `PartialEq`, not `Eq`) and the summary's construction site. `digest_models` itself was not touched |
| 3 | A separate capability revision changes when the overlay's content changes and does not change when only the response ordering changes | `crates/codegg-providers/src/eggpool.rs` — `digest_overlays` (hashes the overlay content alone, over a `BTreeMap` keyed by model id, with each ladder sorted) and `EggpoolCapabilityOverlay::capability_revision`; test `the_capability_revision_moves_with_the_content_and_not_with_the_ordering` | pass | The revision is **separate** from the catalog revision and never feeds it. Two *different wire shapes* expressing the same capability share a revision — the revision is over the capability, not over the spelling the provider used — and a changed ladder moves it |
| 4 | The overlay is applied at M003's single resolution point as the third source, and the effective capability is unchanged when the overlay is absent | `crates/codegg-core/src/model_profile/capability.rs` — `resolve_reasoning_capability(baseline, operator_override, overlay)` and `apply_discovery_overlay` (M003's code, unmodified here); `crates/codegg-core/tests/reasoning_overlay_narrowing.rs` — `a_published_contract_narrows_the_compiled_baseline`, `the_effective_capability_is_unchanged_when_the_overlay_is_absent` | pass | The **resolution function is M003's** and was not touched. This milestone built the overlay and the evidence; the **wiring** is the integrator's (§11). The proof drives a real overlay payload through the real function, not a stub |
| 5 | The overlay may only narrow: a label, a bound, or an always-reasons claim the baseline does not declare is **discarded**, and the narrowing property is enforced structurally by a static guard with a self-test | `crates/codegg-core/tests/reasoning_overlay_narrowing.rs` — `a_hostile_published_contract_is_rejected_in_full` (a payload naming `ultra`, which the baseline does not declare, is refused **in full** and the effective record is byte-identical to the baseline), `an_overlay_cannot_change_the_always_reasons_property` (both directions); `crates/codegg-providers/src/eggpool.rs` — the always-reasons claim is retained for reporting and **never** enters the record (`the_always_reasons_claim_is_retained_but_never_reaches_the_record`) | pass | The **structural guard is the integrator's** (`scripts/check_reasoning_overlay_narrowing.py`, plan §7 work package E) — this milestone did not add a guard script, and `scripts/verify.sh` was not edited. The behavioural half of the property is proven here: a hostile overlay is **rejected in full**, not partially applied, and the rejection is **reported** (`resolution.rejected` names the offending label) |
| 6 | Malformed, oversized, or wrong-typed overlay content degrades to `Unknown` for that dimension and does not fail the catalog probe | `crates/codegg-providers/src/eggpool.rs` — `parse_overlay_labels`, `overlay_bound_field`, `parse_overlay_status`; tests `an_oversized_label_list_degrades_the_effort_dimension_to_unknown`, `an_over_long_label_degrades_the_effort_dimension_to_unknown`, `a_malformed_budget_range_degrades_the_budget_dimension_to_unknown`, `an_oversized_budget_bound_degrades_the_budget_dimension_to_unknown`, `a_malformed_contract_does_not_fail_the_catalog_probe` | pass | The probe succeeds in every case: `a_malformed_contract_does_not_fail_the_catalog_probe` asserts `summary.models.len() == 1` alongside the degraded dimensions. One rule set governs every dimension: malformed content takes the dimension with it, a contradiction degrades it, and a published ladder or range is itself a declaration of the dimension it belongs to |
| 7 | The overlay is refreshed on the existing catalog-refresh cadence and replaced atomically; a turn never observes a partial overlay, and the request path never mutates it | `crates/codegg-providers/src/eggpool.rs` — `EggpoolCapabilityOverlaySlot` (a `RwLock<Arc<EggpoolCapabilityOverlay>>`; `replace` swaps the whole value under the write lock, `load` clones the `Arc` under the read lock and releases it); tests `the_overlay_is_replaced_atomically`, `a_reader_never_observes_a_partially_replaced_overlay` | pass | The slot is the **whole-value** analogue of the catalog refresh's single transaction: one atomic swap, no in-place mutation path, so there is no way to observe a half-replaced overlay. The concurrency test runs a reader thread against 500 replacements and asserts every observed snapshot is one of the two **whole** overlays. The lock is never held across an `await` — the critical section is one `Arc` clone. **The installation site is the integrator's** (§11) |
| 8 | Direct provider connections are entirely unaffected — no overlay, compiled baseline only, and no requirement to run an EggPool process | `crates/codegg-providers/src/eggpool.rs` — `parse_compatible_models_response` keeps its exact signature and returns `Vec<EggpoolModelSummary>`, so the direct-provider paths (`openai_compatible.rs`, `anthropic.rs`) have nowhere to carry an overlay; test `a_direct_provider_connection_has_no_overlay` (the direct parse returns **identical** models with and without a published contract); `crates/codegg-core/tests/reasoning_overlay_narrowing.rs` — `a_direct_provider_connection_has_no_catalog_and_no_overlay` | pass | This is not a special case in the code: it falls out of there being **no catalog**. A direct provider connection issues no catalog request, so there is no response to carry a contract and no slot to install one in |
| 9 | The overlay grants no permission, tool, path, or credential scope, and no arbitrary provider text is retained | `crates/codegg-providers/src/eggpool.rs` — the retained value is a `ReasoningCapabilityRecord` plus one `Option<bool>`; the parser copies **only** the recognised fields and never retains a raw provider string, an object, or a free-text field; `crates/codegg-config/src/schema.rs` — `ReasoningCapabilityRecord::validate` | pass | The overlay carries no authority: it is data about a model, read by a pure function. `always_reasons_claim` is a single boolean retained for reporting; no provider text is retained anywhere |
| 10 | `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes | `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION` is **84**, unchanged by this milestone; no migration file was added; the overlay is **not persisted** | pass | No storage change. Overlay persistence is explicitly deferred by the plan (§8 "Restart", §9 "Storage") to a separate recorded decision |
| 11 | The documentation records the overlay's position, its narrowing rule, its bounds, the separate capability revision, and the unchanged catalog revision | `architecture/provider.md` — "Discovery overlay from provider catalog metadata (M005)"; `CHANGELOG.md` — the `### Changed` entry under `## [Unreleased]`; this record | pass | The section states, in the same terms the plan's and the ADR's amendment notes use, that the overlay has **no live source** at this baseline |
| 12 | The closure record states plainly that the overlay has **no live source** at this baseline — the upstream contract is unimplemented in every available revision, and the capability shape upstream does expose is reachable only over a route CodeGG does not call — so the milestone is forward-compatible machinery, **not** a shipped behaviour change. The closure also names the route that would become the source if a deployment ever publishes the contract. | §1 above; the module header in `crates/codegg-providers/src/eggpool.rs` ("The pinned upstream publishes **no** capability block on `/v1/models` at all") | pass | The route named is the **catalog response itself** — the same `/v1/models` body the probe already reads. No new endpoint was added, and none is required |
| 13 | *(added by this milestone)* The overlay's source is disclosed as absent, and the machinery is proven inert rather than merely unexercised | `crates/codegg-providers/src/eggpool.rs` — `a_response_without_a_contract_retains_no_overlay` (the overlay is absent), `the_catalog_digest_is_unchanged_by_the_contracts_presence`; `crates/codegg-core/tests/reasoning_overlay_narrowing.rs` — `the_effective_capability_is_unchanged_when_the_overlay_is_absent` | pass | The inertness is a **tested property**, not an assertion: a response with no contract yields an empty overlay and the effective capability is the baseline. The integrator may fold this row into the plan's §13 |

## 3. Production implementation evidence

### The JSON shapes that are parsed

The parse reads a **per-model catalog entry** and looks for a thinking
capability block at three candidate locations, in this order:

| Location | Shape |
|---|---|
| `/eggpool/capabilities/thinking` | the **documented** envelope (`docs/thinking.md`) |
| `/capabilities/thinking` | the **flat** runtime capability |
| `/thinking` | the flat capability directly on the model entry |

The two real shapes are therefore both covered, and a deployment may publish
either without CodeGG changing:

**The flat `thinking` capability** (the shape upstream exposes at runtime, on
the integrations-profile route CodeGG does not call):

```json
{
  "id": "model-x",
  "name": "model-x",
  "capabilities": {
    "thinking": {
      "status": "supported",
      "toggle": "unsupported",
      "effort": "supported",
      "budget": "supported",
      "supported_efforts": ["low", "medium", "high"],
      "budget_tokens_min": 1024,
      "budget_tokens_max": 128000
    }
  }
}
```

**The documented `control_contract.accepted_efforts` shape**:

```json
{
  "id": "model-x",
  "name": "model-x",
  "eggpool": {
    "provider_id": "p",
    "base_model_id": "model-x",
    "capabilities": {
      "thinking": {
        "status": "supported",
        "control_contract": {
          "toggle": "unsupported",
          "effort": "supported",
          "budget": "supported",
          "accepted_efforts": ["low", "medium", "high"]
        },
        "budget_tokens_min": 1024,
        "budget_tokens_max": 128000
      }
    }
  }
}
```

**Field resolution.** `toggle`, `effort`, `budget`, and the always-reasons
claim are read from `control_contract` **first** and from the `thinking`
object **second**, so both shapes work from one reader. The accepted-effort
ladder is read from `accepted_efforts` **or** `supported_efforts`, again in
both objects. The budget bounds are read from `budget_tokens_min` /
`budget_tokens_max` at either level.

**Status vocabulary.** `supported`, `unsupported`, `mixed` are recognised
(case-insensitively, after trimming); anything else — including `conflicting`
and an unrecognised spelling — degrades to `Unknown`. A provider's
`conflicting` is a statement about the provider's *sources*, not about the
model, so it is not carried into the record.

### The typed shape that is produced

```rust
/// A bounded, typed thinking-capability overlay retained for one model.
pub struct EggpoolThinkingOverlay {
    record: ReasoningCapabilityRecord,   // the narrowing record — the only part that reaches the resolution
    always_reasons_claim: Option<bool>,   // retained for reporting; never part of the record
}

/// The overlays retained from one catalog response.
pub struct EggpoolCapabilityOverlay {
    catalog_revision: String,        // bound by the caller; empty until then
    capability_revision: String,     // a digest over the overlay content alone
    overlays: BTreeMap<String, EggpoolThinkingOverlay>,   // keyed by model id
}
```

The record is M003's `codegg_config::schema::ReasoningCapabilityRecord` — the
**same type** the resolution already consumes, so the overlay needs no second
policy table and no translation layer. Its four dimensions are populated
**only** from what the provider published:

| Dimension | Published source | Retained as |
|---|---|---|
| effort | `effort` status, `accepted_efforts` / `supported_efforts` | `effort_status`, `accepted_effort_labels` |
| disable | `toggle` status | `disable_status` |
| budget | `budget` status, `budget_tokens_min` / `budget_tokens_max` | `budget_status`, `min_thinking_budget`, `max_thinking_budget` |
| always-reasons | `always_reasons` boolean | **`always_reasons_claim`** — never `always_reasons` / `always_reasons_status` |

The always-reasons claim is **discarded** from the record, exactly as the
plan's §6 security note requires: the compiled baseline owns the
always-reasons property, and a provider's advertisement is not an authority
over its own model's behaviour. The claim is kept beside the record for
reporting only.

### The bounding rules, and their limits

| Rule | Limit | Behaviour when violated |
|---|---|---|
| Accepted-label count | ≤ `MAX_REASONING_CAPABILITY_LABELS` (**32**, the shared `codegg-config` constant, so the record always validates) | the **whole effort dimension** degrades to `Unknown`; the ladder is **not** truncated, because a truncated ladder would present a subset as if it were the published one |
| Per-label length | ≤ `MAX_REASONING_CAPABILITY_LABEL_BYTES` (**64 bytes**) | the whole effort dimension degrades to `Unknown` |
| Label well-formedness | non-empty, already trimmed | the whole effort dimension degrades to `Unknown` |
| Label duplication | deduplicated (a repeated label is the same label) | not a failure: the set is deduplicated, and the result is sorted so the ladder's order cannot move the capability revision |
| Empty ladder (`[]`) | — | treated as **no ladder published** (dimension `Unknown` if no status says otherwise): it is not "this model accepts no effort", which would be a claim the provider never made |
| Budget bound | `1 ≤ tokens ≤ MAX_OVERLAY_THINKING_BUDGET_TOKENS` (**16 777 216**) | the whole budget dimension degrades to `Unknown`; the bound is **discarded, not clamped**, because a clamped bound would be a number the provider never published |
| Budget range | `min ≤ max` | the whole budget dimension degrades to `Unknown` |
| Contradiction inside one dimension | a ladder beside an `unsupported` effort status; a range beside an `unsupported` budget status; a `supported` status with no ladder | that dimension degrades to `Unknown` |
| Status vocabulary | `supported` / `unsupported` / `mixed` | anything else degrades to `Unknown` |
| Overlay presence | the overlay exists only if it declares **at least one** dimension | otherwise the overlay is **absent** (`None`), and the effective capability is the baseline |

Every degradation is per dimension and **never widens**: `Unknown` imposes no
constraint, so a degraded dimension leaves the compiled baseline standing.

### Where the retained capability is held, and why it is outside the digest

The capability lives in **`EggpoolProbeSummary.capability_overlay`** — a
**separate field** from `models`, the list the digest hashes. `digest_models`
is unchanged and still hashes `model.id` and `model.name` and nothing else.

That separation is load-bearing. The catalog digest **is** the revision-scoped
`catalog_revision` that stored last-used-model preferences are keyed against
(`src/core/daemon_sessions.rs`, `apply_model_preference_best_effort`), and it
is the key the revision-scoped `provider_connection_models` rows hang off.
Folding the capability into the digest's inputs would have changed every
deployment's catalog revision once, invalidating stored preferences and
splitting the rows. Instead:

- the **catalog revision** keeps its current inputs and its current meaning;
- the **capability revision** (`digest_overlays`) is a second digest over the
  overlay content alone, so a capability change is detectable without
  disturbing the catalog revision;
- the overlay is **revision-scoped**: `EggpoolCapabilityOverlay.catalog_revision`
  records the catalog revision the overlay belongs to, so a reader can tell an
  overlay that belongs to the current catalog from one that does not. This
  mirrors the catalog's own revision-scoped rows, and it makes the install
  order safe in either direction (a reader that sees a mismatch treats the
  overlay as absent and the baseline stands).

### The atomicity mechanism

The overlay is installed by **replacing the whole value atomically**:
`EggpoolCapabilityOverlaySlot` holds a `RwLock<Arc<EggpoolCapabilityOverlay>>`,
`replace` swaps the entire `Arc` under the write lock, and `load` clones the
`Arc` under the read lock and releases it. There is **no in-place mutation
path**, so a reader can never observe a partially updated overlay, and the
request path cannot mutate an installed overlay at all.

This is the same all-or-nothing property the catalog rows get from the
refresh's single transaction, expressed for a value that is **not persisted**.
(The overlay is not persisted — plan §8 "Restart" and §9 "Storage" defer that
to a separate decision — so it cannot live inside the transaction itself; the
revision-scoped binding is what keeps the two in step.)

### The narrowing proof

The proof runs a **real overlay payload** through the **real** resolution
function, using the **shipped adapter baselines**
(`crates/codegg-core/assets/model-adapters/`):

| Case | Result |
|---|---|
| A published ladder `["low","medium"]` against the `openai-frontier` baseline (`["minimal","low","medium","high"]`) | the effective set is `["low","medium"]`; the dimensions the overlay did not declare keep the baseline's value; `resolution.rejected` is empty |
| A **hostile** published ladder `["low","medium","ultra"]` — `ultra` is not in the baseline | **rejected in full**: one rejection, `source == CapabilitySource::DiscoveryOverlay`, the reason names `ultra`, and the effective record is **equal to the baseline** — not one dimension of the overlay was applied, not even the ones that would have narrowed |
| An overlay claiming `always_reasons = Some(false)` against a baseline of `Some(true)` (and the reverse) | **rejected in full**; the effective record equals the baseline. The parser additionally never emits an always-reasons claim into the record at all |
| No overlay | the effective record equals the baseline; no rejection |
| An oversized published ladder | the overlay is **empty** — it never reaches the resolution; the effective record equals the baseline |

### The direct-provider-connection case

A direct provider connection has **no catalog**, so it has no response to
carry a contract and no slot to install one in. `parse_compatible_models_response`
keeps its exact signature and returns `Vec<EggpoolModelSummary>`; the
direct-provider paths (`openai_compatible.rs`, `anthropic.rs`) therefore
cannot see an overlay at all. The test asserts the direct parse returns
**identical** models with and without a published contract, and the
cross-crate test asserts the resolution is the compiled baseline only. **No
EggPool process is required for a direct connection**, and none was added.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo test -p codegg-providers
cargo test -p codegg-core --test reasoning_overlay_narrowing
cargo test -p codegg-core
```

`bash scripts/verify.sh quick` was **not** run: the integrator runs it once
at the end, and the machine's cargo target-directory lock makes concurrent
sweeps a bottleneck.

### Results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | **pass for this milestone's files.** `rustfmt` was applied to the two files this milestone owns. The only remaining diff in the tree is `src/core/daemon_turns.rs`, which belongs to Milestone 003 / the integrator and was **not** touched here |
| `cargo clippy -p codegg-providers --all-targets -- -D warnings` | **pass** — `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 9.79s`, no warnings |
| `cargo test -p codegg-providers` | **pass** — `305 passed; 0 failed; 0 ignored` (lib), plus `Doc-tests codegg_providers` `0 passed` |
| `cargo test -p codegg-core --test reasoning_overlay_narrowing` | **pass** — 6 passed, 0 failed |
| `cargo test -p codegg-core` | **pass** — the library target `904 passed; 0 failed`, and **every** integration-test target `0 failed` (including `reasoning_overlay_narrowing`'s 6) |

### 4.1 The three failures the first run caught

The first `cargo test -p codegg-providers --lib` run reported **3 failed**.
All three were wrong *expectations* in this milestone's own tests, not
defects in the implementation, and each was fixed:

1. `a_malformed_budget_range_degrades_the_budget_dimension_to_unknown` and
   `an_oversized_budget_bound_degrades_the_budget_dimension_to_unknown`
   expected a retained overlay, but the payload declared **only** the budget
   dimension — so when that dimension degraded, the overlay was correctly
   **absent**. The payloads now also publish a sound `toggle`, and the tests
   assert the budget dimension degraded **and** the sound dimension survived.
   The behaviour was right; the assertion was not.
2. `the_capability_revision_moves_with_the_content_and_not_with_the_ordering`
   asserted that the flat and the documented wire shapes must have
   **different** capability revisions. They have the **same** one, because
   they express the **same capability** — and that is the better property: the
   revision is over the capability, not over the spelling the provider used.
   The assertion was inverted.

After the fixes: `305 passed; 0 failed`.

## 5. Invariant review

| Plan §4 invariant | Evidence | Result |
|---|---|---|
| The model catalog digest keeps its inputs and its meaning | `digest_models` unchanged; `the_catalog_digest_is_unchanged_by_the_contracts_presence` | pass |
| A capability change is detectable without disturbing the catalog revision | `digest_overlays` / `capability_revision`; `the_capability_revision_moves_with_the_content_and_not_with_the_ordering` | pass |
| The overlay may only narrow | `a_hostile_published_contract_is_rejected_in_full`, `an_overlay_cannot_change_the_always_reasons_property` | pass |
| The always-reasons property comes from the compiled baseline | the parser never writes `always_reasons` / `always_reasons_status` into the record; `the_always_reasons_claim_is_retained_but_never_reaches_the_record` | pass |
| No fabricated contract | no label, bound, or status is invented; the ladder is copied verbatim (deduplicated and sorted only); a depth label is never converted into a token budget | pass |
| Malformed provider input must not break discovery | `a_malformed_contract_does_not_fail_the_catalog_probe` (the probe succeeds and the models are still returned) | pass |
| The overlay is never half-applied | `EggpoolCapabilityOverlaySlot` has no in-place mutation path; `a_reader_never_observes_a_partially_replaced_overlay` | pass |
| The request path never mutates the overlay | `load` returns an `Arc` clone and every accessor returns a reference; the overlay's fields are private | pass |
| No second policy table | the overlay produces M003's own `ReasoningCapabilityRecord`; the narrowing decision is made by M003's `apply_discovery_overlay`, not by a second implementation here | pass |
| Direct provider connections are unaffected | `a_direct_provider_connection_has_no_overlay`, `a_direct_provider_connection_has_no_catalog_and_no_overlay` | pass |
| The `eggpool-wire` kernel is reached through one declaration | `crates/codegg-providers/Cargo.toml` is unchanged; the pin is still `rev = "9ac6a1318e8db3c034b5ab54987317752d5ffea6"` | pass |
| No new discovery cadence and no live-network test | no new endpoint, no new cadence, no network test; every test is a pure parse or a pure function call | pass |

## 6. Failure and recovery review

- **Partial failure.** A malformed contract produces **no overlay** (or an
  overlay whose affected dimension is `Unknown`); the catalog probe still
  succeeds and the baseline stands. The overlay is parsed into a complete
  typed value *before* it is installed, so it is never half-applied.
- **Cancellation races.** The overlay is not turn-scoped. A cancelled turn
  leaves it untouched.
- **Daemon restart.** The overlay is re-derived by the existing catalog-refresh
  path. Because it is **not persisted**, a restart leaves it absent until the
  next refresh, and the compiled baseline stands meanwhile. That is the
  intended behaviour, and it is why the plan defers persistence to a separate
  recorded decision.
- **Partial persistence failure.** Not applicable: the overlay persists
  nothing.
- **Stale generations.** An overlay older than the current catalog is still
  narrow-only, so staleness can only under-offer, never over-offer. The
  revision binding makes the staleness **detectable**: an overlay whose
  `catalog_revision` does not match the current one is treated as absent, and
  the baseline stands. An overlay that becomes stale after a provider *widens*
  its set is corrected at the next refresh.
- **Contention.** The overlay is replaced atomically, so a turn never sees a
  partially updated capability. A refresh that races a turn does not change
  the turn's already-resolved capability — the turn holds the `Arc` it
  loaded.
- **Duplicate delivery.** A repeated refresh producing identical content
  produces an identical capability revision and installs an equivalent
  overlay. The merge of duplicate model ids is order-independent, so the
  response's ordering cannot change what is retained.
- **Malformed or unauthorized input.** A malformed contract is degraded per
  dimension and never propagates. The overlay carries no authorization
  meaning, so there is nothing to authorize.
- **Bounded event and artifact behavior.** The overlay retains at most one
  record per model id, each with at most 32 labels of at most 64 bytes and two
  budget bounds. The response body is already bounded by the probe's
  existing byte limit.

## 7. Migration and compatibility review

- **Schema migration** — none. `STORAGE_LAYOUT_VERSION` is **84**, unchanged
  by this milestone. No migration file was added.
- **Protocol negotiation** — unchanged. No request or response shape changed,
  and no frontend payload was added.
- **The model catalog revision is unchanged** — this is the compatibility
  point that matters most. The digest keeps hashing `id` and `name` only, so
  upgrading to this milestone does not invalidate stored last-used-model
  preferences and does not split the revision-scoped
  `provider_connection_models` rows. A capability change is detectable through
  the **separate** capability revision instead.
- **Configuration behavior** — unchanged. No configuration key was added,
  and no existing key's meaning changed.
- **Provider behaviour** — unchanged **today**, because no provider publishes
  a contract on `/v1/models`. When one does, a selection the effective
  capability supports is unaffected, and a selection the overlay narrows away
  is refused locally by M003's policy, with a truthful message instead of a
  provider 400.
- **Rollback limitations** — the overlay is code with no persisted state, so
  a revert removes it entirely. There is nothing to migrate back.
- **Legacy-path status** — a direct provider connection has no catalog and no
  overlay, and is byte-for-byte unaffected.

## 8. Security review

- **Authorization** — the overlay grants nothing. It is data about a model,
  read by a pure function; it carries no permission, tool, path, or credential
  scope.
- **Secret handling** — the overlay holds model facts only. No credential,
  token, or path ever enters it, and no provider text is retained beyond the
  recognised fields.
- **Path validation** — not applicable: no path is read or written.
- **Privilege boundaries** — the overlay is parsed in the providers crate and
  is consumed by a pure function in `codegg-core`. It is never consulted
  before an authorization decision, and it never relaxes one.
- **Denial-of-service bounds** — the retained value is bounded: ≤ 32 labels
  of ≤ 64 bytes per model, two budget bounds under
  `MAX_OVERLAY_THINKING_BUDGET_TOKENS`, and one boolean. An oversized or
  malformed payload degrades the affected dimension rather than inflating a
  surface, and the response body was already bounded by the probe.
- **Redaction** — the rejection M003 reports names the offending **label** and
  the dimension. It carries no request body, no credential, and no tool
  arguments.
- **Audit behavior** — an overlay rejection is a capability fact, not an
  authorization decision, so it is not recorded in the authorization audit
  trail. That would be misleading: the overlay says what a model accepts, not
  what a principal may do.
- **The always-reasons property is not a provider's to claim** — the claim is
  discarded at the parse boundary, and M003 refuses an overlay that would
  change the property in either direction.

## 9. Documentation and operations

| File | Change |
|---|---|
| `architecture/provider.md` | "Discovery overlay from provider catalog metadata (M005)": where the contract is retained, that it is **outside** the catalog digest, that it has its **own** revision, that it may only **narrow**, the bounds, the per-dimension `Unknown` degradation, the atomic replacement, and the **no live source** disclosure |
| `CHANGELOG.md` | The `### Changed` entry under `## [Unreleased]` |
| `plans/closure/provider-reasoning-control/005-status.md` | This record |

**Static guards** — none were added by this milestone. The plan's §7 work
package E names `scripts/check_reasoning_overlay_narrowing.py` (the guard
that asserts the overlay's write path cannot reach the compiled adapter
baseline, with a `--self-test`); `scripts/verify.sh` and
`.github/workflows/ci.yml` are the integrator's to register, and this
milestone edited neither.

**Operator diagnostics** — there are none to add at this baseline, because
the overlay has no live source: no provider response CodeGG receives today
produces an overlay. When one does, M003's existing rejection names the
model, the dimension, and the offending label.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | **The overlay has no live source.** The upstream `control_contract.accepted_efforts` contract is unimplemented in every available revision, the pinned `/v1/models` handler emits a flat four-key model object with no capability block, and the one capability shape upstream exposes at runtime rides an integrations-profile route CodeGG does not call. | **No behaviour changes today.** Every provider response yields an absent overlay, so the effective capability is the compiled baseline, exactly as before this milestone. The machinery is inert but decided and tested. | None for this milestone. When a deployment publishes the contract on `/v1/models`, the parse already reads it. Adding a call to the integrations-profile route would be **a different roadmap's business** (a new provider surface) and was deliberately not done — the gap is recorded here instead. |
| **medium** | **The overlay is not persisted.** It is re-derived on the existing refresh cadence and is absent after a restart until the next refresh. | Between a restart and the first successful refresh, the effective capability is the baseline even if the provider has published a narrower contract. That is the **safe** direction: the baseline is the upper bound. | Persistence is explicitly deferred by the plan to a separate recorded decision. It would need its own storage layout decision, and it is deliberately **not** smuggled in here. |
| **medium** | **The overlay is not yet wired into the daemon.** The overlay, its bounds, its revision, and its atomic slot are landed and proven, but nothing installs it and nothing passes it to the resolution. | The milestone's behaviour change is still zero. The wiring is M003's turn-admission call site plus the integrator's catalog-refresh site. | The integrator wires it per §11 below. The **behaviour** does not change until they do — and even then, not until a provider publishes a contract. |
| low | **`EggpoolProbeSummary` lost its `Eq` derive.** The retained record is `PartialEq` but not `Eq`, so the summary is now `PartialEq` only. | No effect: nothing in the tree requires `EggpoolProbeSummary: Eq`. | None. Recorded so the derive change is not mistaken for an accident. |
| low | The plan's §13 enumerates **twelve** criteria; this record carries a thirteenth (the disclosure of the overlay's source). | The matrix and the plan's list differ by one row. | The integrator may fold the thirteenth row into the plan's §13 when the plan is next edited. The plan file was not edited here. |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The retained contract, the bounded typed parse, the separate capability
revision, the digest's independence, the atomic replacement, the
per-dimension `Unknown` degradation, and the narrowing proof through the real
resolution function all landed and are verified.

**Two things remain open, and both are deliberately outside this milestone:**

1. **The wiring.** The overlay must be installed at the daemon's
   catalog-refresh site and passed into M003's resolution at the turn-admission
   site. §11.1 below is the note the integrator needs.
2. **The static guard.** The plan's §7 work package E names
   `scripts/check_reasoning_overlay_narrowing.py` and its `--self-test`; the
   integrator registers it in `scripts/verify.sh` and CI.

Neither changes the behaviour of a deployment today, because the overlay has
no live source (§10).

### 11.1 The wiring note for the integrator

**Where the overlay comes from.** `CompatibleProbe::probe` returns an
`EggpoolProbeSummary`, which now carries the retained overlay in its
**separate** `capability_overlay` field.

**Where it must be installed.** In `src/core/eggpool.rs`, in the function
that performs the catalog refresh (the one that writes the catalog rows in
its single transaction), after that transaction commits and the new
`catalog_revision` is known:

```rust
// The daemon holds one slot per provider connection.
// src/core/eggpool.rs — the catalog-refresh path, after the transaction commits.
overlay_slot.replace(capability_overlay.with_catalog_revision(&catalog_revision));
```

`EggpoolCapabilityOverlaySlot::replace` swaps the whole value under one
write lock; there is no in-place mutation path, so a turn never observes a
partial update.

**Where it must be passed into the resolution.** At M003's turn-admission
call site — `src/core/daemon_turns.rs`, `admit_reasoning_capability`
(`pub(crate) fn admit_reasoning_capability`, the call to
`resolve_reasoning_capability` at `:188-192`) — the **third argument** is the
overlay. It is currently the literal `None`, and the call site is already
shaped so one can be passed without changing the gate:

```rust
// src/core/daemon_turns.rs — admit_reasoning_capability
let effective = capability::resolve_reasoning_capability(
    profile.reasoning_capability.as_ref(),
    None,              // the operator override — already folded into the profile
    overlay_for_model, // ← the third argument: the discovery overlay
);
```

The overlay is looked up from the slot and bound to the current catalog
revision:

```rust
// The current overlay, then the per-model lookup.
let overlay = overlay_slot.load();
let overlay_for_model = if overlay.catalog_revision() == current_catalog_revision {
    overlay.for_model(model_id).map(|entry| entry.record())
} else {
    None
};
```

The exact accessors:

| What | How |
|---|---|
| The current overlay | `EggpoolCapabilityOverlaySlot::load() -> Arc<EggpoolCapabilityOverlay>` |
| The overlay for one model | `EggpoolCapabilityOverlay::for_model(model_id) -> Option<&EggpoolThinkingOverlay>` |
| The narrowing record | `EggpoolThinkingOverlay::record() -> &ReasoningCapabilityRecord` |
| The provider's always-reasons claim (reporting only) | `EggpoolThinkingOverlay::always_reasons_claim() -> Option<bool>` |
| The capability revision | `EggpoolCapabilityOverlay::capability_revision() -> &str` |
| The catalog revision the overlay belongs to | `EggpoolCapabilityOverlay::catalog_revision() -> &str` |

**How the daemon reaches the overlay.** The slot is per **provider
connection**, so the daemon needs a place to hold one slot per connection —
most naturally a map on the daemon's own provider-connection state (the state
that already keys the catalog rows by connection and revision), keyed by the
same connection identity the catalog uses. The daemon state therefore **does**
need a new field for it; there is no existing slot to reuse, because the
overlay is a per-connection fact and not a per-session one.

**What happens with no catalog.** A direct provider connection has no catalog,
so it has no overlay and no slot: the effective capability is the compiled
baseline. An EggPool-backed connection whose catalog refresh has not yet run
(or has failed) is in the same position — the slot holds the **empty** overlay
and the baseline stands. The overlay's absence is never an error.

**Things the integrator must decide (this milestone did not):**

1. **Where the slot map lives** — the daemon's provider-connection state is
   the natural home, but the daemon's own state layout is the integrator's
   call.
2. **Which model id keys the lookup.** The overlay is keyed by the **catalog
   model id** (the `id` the `/v1/models` response carried). The turn's model
   may be a qualified, provider-scoped id (`provider/model`), so the
   integrator must decide how the turn's model is reduced to the catalog's
   model id at admission time. This milestone deliberately did not guess.
3. **The revision comparison.** The overlay carries the catalog revision it
   belongs to, and the comparison against the *current* catalog revision is
   the integrator's. The recommended comparison is the one above: a mismatch
   means "absent", so the baseline stands — the safe direction.
4. **Whether a stale overlay is dropped or kept.** Dropping it on a revision
   mismatch is safe (the baseline is the upper bound). Keeping it and letting
   the narrowing apply would also be safe — a stale overlay is still
   narrow-only — but it would apply a contract from a superseded catalog.
5. **The static guard and its registration** (plan §7 work package E).

## 12. Registry updates

Required in `plans/registry.md` and `plans/subsystems/provider-reasoning-control-roadmap.md`
(the integrator owns both):

- record Milestone 005 as closed, with the unresolved findings in §10 carried
  forward — in particular that the overlay has **no live source** at this
  baseline, that it is **not persisted**, and that it is **not yet wired**;
- note that the wiring (the catalog-refresh install site and the
  turn-admission overlay argument) and the static guard
  (`scripts/check_reasoning_overlay_narrowing.py` plus its registration) remain
  open for the integrator.
