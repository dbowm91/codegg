# Provider Reasoning Control Milestone 004 — Closure Status

Status: closed

> **Closed 2026-10-21 by the integrator.** The milestone was first recorded as
> *conditionally closed* with two named conditions (§11). Both are discharged:
>
> - the capability payload and the shared option-derivation rule are now
>   documented in `architecture/protocol.md` and `architecture/provider.md`
>   (the condition named in §10 finding F-1); and
> - the WS-attach transport's inability to carry a core request is **accepted
>   explicitly** in the protocol documentation — `architecture/protocol.md` and
>   `architecture/tui.md` now say that `/reasoning` there reports that the
>   selection cannot be stored and leaves the display alone, rather than
>   falling back to a local value the daemon would ignore. That is the second
>   resolution §10 finding F-2 offered ("or accept the limitation explicitly
>   in the protocol documentation"); adding a `TuiMessage` variant that
>   carries a core request remains a separate protocol decision.
>
> `scripts/check_tui_reasoning_surface.py` is registered in
> `scripts/verify.sh quick` and in the CI `verify` job. The low findings in
> §10 are carried forward and are not conditions on this closure.

Source implementation plan:

- `plans/implementation/provider-reasoning-control/004-frontend-reasoning-option-surface.md`

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md`

Repository baseline reviewed: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5`
(branch `feat/auto-20261021-d6b5ffc5`; changes left uncommitted in the working
tree, as instructed)

Implementation commits or pull requests:

- none — uncommitted working-tree changes on `feat/auto-20261021-d6b5ffc5`.

## 1. Executive finding

The frontend reasoning option surface is **complete**, and the `/reasoning`
**write path** — the high-severity defect an independent audit of Milestone
002 found — is closed.

Before this milestone, `src/tui/app/mod.rs`'s `/reasoning` handler mutated a
local field (`ui_state.reasoning_mode`) and showed a transient toast. It
never wrote the selection to the daemon: `CoreRequest::ReasoningPreferenceSet`
existed in `crates/codegg-protocol/src/core.rs` and was handled in
`src/core/daemon_ops.rs`, but **no client sent it**. The TUI also sent its
local mode as an *explicit* per-turn override, so the local state always beat
a persisted selection — the opposite of what `docs/providers.md` and
`CHANGELOG.md` claimed ("the choice is kept by the daemon against your own
account, so it applies to every turn that starts afterwards").

Now:

- `/reasoning` writes the selection through `CoreRequest::ReasoningPreferenceSet`
  with the daemon's preference revision as the compare-and-set, and the local
  display is updated from the daemon's **response**. The daemon's value wins
  over a divergent local display; a refused write restores the daemon's last
  known value and surfaces the typed refusal.
- The turn carries **no** explicit reasoning override, so the daemon's
  persisted selection is what actually applies.
- The option set is derived from the model's published capability, and a
  model with no published capability falls back to the known-label ladder and
  says so.
- The selection is persistently visible in working strip row 0, reading the
  daemon-owned selection, with no second spinner and no second cadence.

The milestone is closed **conditionally** for two reasons that are outside
this milestone's ownership, both recorded in §10:

1. `architecture/provider.md` and `architecture/protocol.md` do not yet
   describe the landed payload (owned by M003/M005 and the integrator).
2. The WS-attach frontend has no channel for a core request, so `/reasoning`
   there reports that the selection cannot be stored rather than changing a
   local value the daemon would ignore.

## 2. Requirement-to-evidence matrix

Every acceptance criterion of plan §13, with the evidence that discharges it.

| # | Requirement | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | Capability carried on the model DTOs as an optional field with `#[serde(default, skip_serializing_if = "Option::is_none")]`; old client decodes new daemon, new client decodes old daemon | `crates/codegg-protocol/src/provider.rs` — `ProviderModelDto::reasoning_capability` and `SelectedModelDto::reasoning_capability`; test `provider::tests::response_without_capability_payload_decodes_as_absent` decodes a response with **no** payload field and asserts it reads as `None`; test `provider::tests::response_with_capability_payload_round_trips` round-trips the payload through both DTOs | pass | The field is `Option<ReasoningCapabilityDto>`, defaulted and skipped when `None`, exactly like `ConnectionHealthDto::credential_status` in the same file. No `deny_unknown_fields` on the DTOs, so an old client ignores the new field and a new client ignores its absence. |
| 2 | `ModelsSnapshot` and `ModelsRefresh` keep their `Vec<String>` shape | `crates/codegg-protocol/src/core.rs` is **unchanged** by this milestone; test `provider::tests::models_snapshot_and_refresh_shapes_are_unchanged` asserts the snapshot serialises to a JSON array of model strings and that `ModelsRefresh` still round-trips as `{"type":"models_refresh"}` | pass | The models channel is untouched. The capability rides on the model DTOs, never on the snapshot. |
| 3 | The frontend offers only the options the model accepts: its labels, a disable only when permitted, a budget only when permitted and within bounds | `crates/codegg-protocol/src/provider.rs::reasoning_options`; tests `provider::tests::established_capability_offers_the_capabilitys_labels_in_its_order`, `provider::tests::capability_without_disable_offers_no_off_option`, `provider::tests::budget_outside_the_models_bounds_is_not_offered`, `provider::tests::unsupported_and_unknown_budget_dimensions_offer_no_budget`; TUI test `the_cycle_moves_through_the_models_options` | pass | Effort: the published labels **in the capability's order**. Disable: only when the disable axis is `Supported` and the model does not always reason. Budget: only when the budget axis is `Supported` and the value is inside the model's range. |
| 4 | An always-reasoning model is offered **no** disable, and the reason is stated | Test `provider::tests::always_reasons_model_is_stated_and_offers_no_off_option`; TUI test `an_always_reasons_model_is_stated_and_has_no_off`; `src/tui/commands/reasoning.rs::option_note` returns `"this model always reasons, so there is no off"` | pass | A model whose `always_reasons` is `Some(true)` gets no `Off` option even when the disable axis claims `Supported`, and the command states the reason instead of appearing broken. |
| 5 | A client with no capability payload falls back to the known-label ladder and does **not** present it as the model's option set | `reasoning_options(None)` returns the four `KNOWN_REASONING_EFFORT_LABELS` plus the historic off option with `fallback == true`; tests `provider::tests::absent_capability_falls_back_to_the_known_label_ladder` and TUI `an_absent_capability_falls_back_to_the_known_label_ladder` assert the ladder is present, `fallback` is set, and the note says `"known labels"` | pass | The fallback carries no budget: the known-label vocabulary declares no budget contract. The fallback is flagged, so a surface that shows it says so. |
| 6 | The selection is persistently visible in working strip row 0, reading the daemon-owned selection, and the daemon's value wins over a divergent local display | `src/tui/app/render.rs` — row 0 appends `" · reasoning: {detail}"` from `ui_state.reasoning_mode`; TUI tests `the_indicator_shows_the_daemon_owned_selection`, `a_daemon_side_selection_wins_over_a_stale_local_display`, `the_command_writes_the_selection_through_the_daemon` | pass | The indicator reads `UiState::reasoning_mode`, which is the projection of the daemon-owned selection. The daemon's response overwrites the local display; when they differ, the daemon's value is applied and a correction toast is raised. |
| 7 | The indicator adds no second spinner, no second cadence, and no free-form competing label; the single-`ActivityPhase` and one-frame-table invariants are intact | `scripts/check_tui_reasoning_surface.py` — five checks, all pass; `--self-test` injects four violations and proves each is rejected. TUI test `the_indicator_adds_no_second_spinner_or_cadence` asserts two renders are byte-identical and that row 0 carries no `SPINNER_FRAMES` glyph | pass | The indicator is a static string in a row the one 100 ms frame table already redraws. `ActivityPhase` and `tui::components::spinner` are untouched. The pre-existing tool-call-header frame table is recorded in §10. |
| 8 | Working strip row 0 stays reserved unconditionally, and the transcript does not reflow when the indicator appears or disappears | TUI test `the_indicator_does_not_reflow_the_transcript` asserts `working_strip_rows()` and `session_layout(...)` are identical before and after the selection changes; `scripts/check_tui_reasoning_surface.py` fails when `working_strip_rows` mentions the reasoning state | pass | The indicator is a suffix on the reserved row, in the same `" · "` chain as the queued count — never an extra row and never a condition on row 0's existence. |
| 9 | `/reasoning` and `/thinking` remain distinct; the `/reasoning` alias set and its transient toast are unchanged | `src/tui/command.rs:588` is unchanged: `/reasoning` with aliases `reasoning-effort`, `thinking-budget`. `/thinking` is untouched. The transient toast is still emitted synchronously by the `/reasoning` handler | pass | The toast is preserved but is no longer the only place the selection is visible. |
| 10 | The option list is a subset of what the capability permits, and the capability is never used as an authorization input | Test `provider::tests::option_set_is_a_subset_of_what_the_capability_permits` asserts every derived option is permitted by the capability. The payload is display/selection data only; the daemon's admission path (`src/core/daemon_turns.rs`, owned by M003) is unchanged and still refuses a selection the model does not accept | pass | The frontend derives options; it never widens them, and it never fabricates a label. |
| 11 | `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes | `crates/codegg-core/src/storage/mod.rs:43` — `STORAGE_LAYOUT_VERSION: u32 = 84` (unchanged by this milestone). No migration was added. `python3 scripts/check_project_catalog_invariants.py` → 7/7 checks passed | pass | No storage change of any kind. |
| 12 | `architecture/tui.md`, `architecture/provider.md`, `architecture/protocol.md`, and `docs/tui.md` describe the landed contract | `architecture/tui.md` — new sections "The reasoning indicator (provider reasoning control M004)" and "The `/reasoning` write path (M004)", plus the updated `UiState` snippet. `docs/tui.md` — new user-facing "## Reasoning control" section. `architecture/provider.md` and `architecture/protocol.md` are **not** updated by this milestone | **partial** | `architecture/provider.md` and `architecture/protocol.md` are outside this milestone's ownership (M003/M005 and the integrator edit them). Recorded as finding F-1 in §10. |

## 3. Production implementation evidence

### Files changed

| File | Change |
|---|---|
| `crates/codegg-protocol/src/provider.rs` | The capability payload, the axis enum, the shared option derivation, and the optional field on both model DTOs; the protocol tests |
| `src/core/session_selection.rs` | `reasoning_capability_dto` and the payload on the four model-resolution sites |
| `src/core/eggpool.rs` | The payload on the provisioner's and the catalog's model lists |
| `src/tui/commands/reasoning.rs` | **New** — the option set, the cycle, the write path, the read path, the refusal messages, and the tests |
| `src/tui/app/mod.rs` | The `/reasoning` handler, the capability lookup and cache, `TurnSubmit` no longer sending an explicit override, `dispatch_builtin_command` made crate-visible, and the updated pre-existing `/reasoning` tests |
| `src/tui/app/state/ui.rs` | `reasoning_capabilities`, `reasoning_preference_revision`, `reasoning_request`; `reasoning_mode` redefined as the daemon-owned projection |
| `src/tui/app/commands.rs` | `TuiCommand::ReasoningPreferenceUpdated` and `TuiCommand::ReasoningPreferenceLoaded` |
| `src/tui/app/render.rs` | The working strip's row-0 reasoning indicator, and the render tests |
| `src/tui/runtime/command_dispatch.rs` | The dispatch arms, and the capability cache update on the model-resolution completion |
| `src/tui/runtime/event_loop.rs` | The start-up read of the daemon-owned selection |
| `src/tui/commands/mod.rs` | The `reasoning` module registration |
| `src/tui/commands/session_selection.rs` | The capability carried through the TUI's own `ProviderModelDto` → `SelectedModelDto` re-projection |
| `src/tui/components/dialogs/connection_selection.rs` | The new optional field on the test fixture's `SelectedModelDto` |
| `crates/codegg-client/src/compose.rs`, `crates/codegg-client/src/lib.rs` | The shared derivation re-export and the client's "not affected by the payload" tests |
| `scripts/check_tui_reasoning_surface.py` | **New** — the one-cadence guard |
| `architecture/tui.md`, `docs/tui.md` | The landed contract |
| `tests/connect_tui_restoration.rs`, `tests/model_preference_convergence.rs` | The new optional field on the fixtures' model DTOs |

### The payload's exact shape

`crates/codegg-protocol/src/provider.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningCapabilityAxis {
    #[default]
    Unknown,      // nobody has published a contract for this dimension
    Supported,    // the dimension is accepted
    Unsupported,  // the dimension is refused
    Mixed,        // accepted by some model variants, refused by others
    Conflicting,  // two sources disagree
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReasoningCapabilityDto {
    pub effort: ReasoningCapabilityAxis,
    pub accepted_effort_labels: Vec<String>,
    pub disable: ReasoningCapabilityAxis,
    pub budget: ReasoningCapabilityAxis,
    pub min_budget_tokens: Option<u64>,
    pub max_budget_tokens: Option<u64>,
    pub always_reasons: Option<bool>,
}
```

It is carried on both model DTOs as
`#[serde(default, skip_serializing_if = "Option::is_none")] pub
reasoning_capability: Option<ReasoningCapabilityDto>` — the same optionality
discipline as `ConnectionHealthDto::credential_status` in the same file.

**What an absent payload means.** An absent payload means the peer predates
the capability. It reads as **"not established"** — never as "this model
accepts nothing", and never as "this model accepts everything". The
distinction the payload makes internally is between *not established* and
*refused*: `Unknown` (and `Mixed`, `Conflicting`) is not established, while
`Unsupported` is a published refusal. A frontend without a payload therefore
falls back to the known-label ladder and **says that it is a fallback**.

A present payload whose every axis is `Unknown` is a different statement
from an absent one: it says "this peer publishes capabilities, and this model
has none established".

### Where the daemon populates it

`src/core/session_selection.rs::reasoning_capability_dto(config,
provider_kind, model_id)` resolves the **effective** capability of one model
and returns the payload:

- the provider segment is the connection's provider kind
  (`ProviderKind::as_str()`);
- the effective record comes from
  `codegg_core::model_profile::ModelProfileResolver::new(config).resolve(model)`,
  which is the compiled adapter baseline **narrowed by the operator's config
  override** (`apply_config_override` already merges the override into the
  profile's `reasoning_capability`);
- the discovery overlay is deliberately **not** applied — it has no live
  source at this baseline, and a frontend that offered a label the admission
  path would refuse is worse than one that offers the known-label fallback.

The payload is populated at every model-resolution site:

- `src/core/session_selection.rs` — `resolve_selection` (the durable
  selection's `SelectedModelDto`), the session-selection summary, the
  model list for a connection (`ProviderModelDto`), and the selection-update
  path;
- `src/core/eggpool.rs` — the provisioner's model list (`ProviderModelDto`)
  and the catalog model list.

A connection that is not the vendor itself — an OpenAI-compatible endpoint, an
EggPool pool, an Azure deployment — matches no adapter contract, so its
capability is **not established** on any dimension. That is the honest
answer: whether such an endpoint honours `reasoning_effort` is a deployment
fact, not a compiled-in universal.

### The option-derivation rule

`crates/codegg-protocol/src/provider.rs::reasoning_options(capability)` —
one rule per dimension. The result is always a **subset** of what the model
accepts.

| Dimension | Established (`Supported`) | Refused (`Unsupported`) | Not established (`Unknown`/`Mixed`/`Conflicting`) |
|---|---|---|---|
| effort | the accepted labels, **in the capability's order** | no effort option | the known-label ladder (`minimal`, `low`, `medium`, `high`), flagged as a fallback |
| disable | an off option — **unless** the model always reasons | no off option | the historic off option (the known-label vocabulary carried one before the capability existed) |
| budget | a fixed-budget option, and only when the offered value is inside the model's accepted range | no budget option | no budget option — the known-label vocabulary declares no budget contract |

The always-reasons property is a status *and* a value: `always_reasons ==
Some(true)` is the property, and it removes the off option whatever the
disable axis says. The budget value offered is the model's **smallest**
accepted budget (its `min_budget_tokens`, else its `max_budget_tokens`),
which is inside the model's range by construction;
`reasoning_budget_option(capability, tokens)` returns `None` for a value
outside the range.

`ReasoningOptionSet` carries `options`, `fallback` (the effort options are
the known-label ladder rather than the model's published labels), and
`always_reasons`.

The frontend's cycle moves through **the model's** options
(`src/tui/commands/reasoning.rs::next_mode`), not a static ladder. A
selection that is not one of the model's options restarts the cycle at the
model's first option rather than inventing a neighbouring level.

### The working-strip presentation, and the proof it adds no second cadence

`src/tui/app/render.rs` — row 0 of the working strip gains one suffix, in
the same left-aligned `" · "` chain as the queued count:

```
idle · reasoning: high
idle · reasoning: off
idle · reasoning: default
```

It is **content in the reserved row**, never a condition on the row's
presence. Row 0's height is subtracted from the transcript
(`session_layout` splits `prompt_height` against `Min(10)`), so gating the row
on the selection reflowed the transcript on every submit and completion. The
guard fails when `working_strip_rows` mentions the reasoning state at all.

It adds **no second spinner, no second cadence, and no second frame table**:

- there is one frame table (`tui::components::spinner`), one `ActivityPhase`
  enum, and one 100 ms frame table; the indicator is a static string in a row
  that row 0's frame already redraws;
- `scripts/check_tui_reasoning_surface.py` fails on a second animation frame
  table anywhere under `src/tui/` (beyond the one documented pre-existing
  exemption), on a timer in the reasoning surface, on the reasoning surface
  reaching for the frame table or the frame chooser, and on the strip's row
  count depending on the reasoning state;
- the behavioural test `the_indicator_adds_no_second_spinner_or_cadence`
  asserts two renders of the same selection are byte-identical and that row 0
  carries no `SPINNER_FRAMES` glyph.

### The `/reasoning` write path

`src/tui/commands/reasoning.rs` (new module) and the `/reasoning` handler in
`src/tui/app/mod.rs`.

Flow:

1. The command derives the option set from the model's published capability.
2. It advances the cycle to the next option **the model accepts**.
3. It updates the local display optimistically and raises the existing
   transient toast (preserved), including the existing per-provider stripping
   warning, plus the capability note (always-reasons / no-off / fallback).
4. It sends `CoreRequest::ReasoningPreferenceSet { selection,
   expected_revision }` — `expected_revision` is the daemon's preference
   revision, used as the compare-and-set. `Inherit` maps to `selection:
   None`, which clears the selection.
5. The completion lands as `TuiCommand::ReasoningPreferenceUpdated { request_id,
   preference, previous, error }`, dispatched through
   `src/tui/runtime/command_dispatch.rs` into
   `apply_preference_updated`, which drops stale completions through the
   request generation guard.
6. On success the local display is set from the daemon's **response** — the
   daemon's value wins. If it differs from the optimistic value, a correction
   toast says so. On a refusal the display falls back to the daemon's last
   known value and the refusal is surfaced.

The read path (`CoreRequest::ReasoningPreferenceGet` →
`TuiCommand::ReasoningPreferenceLoaded` → `apply_preference_loaded`) runs at
TUI start-up (`src/tui/runtime/event_loop.rs`), so a frontend that starts or
reconnects reads the daemon-owned selection instead of showing a local
default.

The refusals surfaced, with their typed code carried in the message:
`preference_conflict`, `invalid_reasoning_selection`,
`reasoning_vocabulary_unsupported`, `reasoning_capability_unsupported`, plus
`preference_unavailable` and the generic case.

**The turn carries no override.** `src/tui/app/mod.rs`'s `TurnSubmit` now
sends `reasoning: None`. Sending the frontend's copy as an explicit override
made the local value beat the persisted one; the daemon resolves the
selection at every turn-admission path (explicit override ▸ persisted
principal preference ▸ agent/model-profile default), so `None` is what makes
the daemon's persisted selection authoritative.

### The capability cache in the frontend

`UiState::reasoning_capabilities: HashMap<String, ReasoningCapabilityDto>`,
keyed by model id. The daemon's model-resolution response is the only
source: `TuiCommand::SessionSelectionLoaded` records the selected model's
capability and each model in the accompanying list. `App::current_reasoning_capability()`
looks the current model's id up in that map. A model with no entry has **no
published capability**, which reads as "not established" — never as "this
model accepts nothing". The TUI's own re-projection of `ProviderModelDto` into
`SelectedModelDto` carries the payload through.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-protocol -p codegg-client -p codegg --all-targets -- -D warnings
cargo test -p codegg-protocol
cargo test -p codegg-client
cargo test -p codegg --lib
python3 scripts/check_tui_reasoning_surface.py
python3 scripts/check_tui_reasoning_surface.py --self-test
python3 scripts/check_project_catalog_invariants.py
```

`scripts/verify.sh quick` was **not** run: the integrator runs it once at the
end, and the machine is shared.

### Results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass — no diff |
| `cargo clippy -p codegg-protocol -p codegg-client -p codegg --all-targets -- -D warnings` | pass — no warnings, no errors |
| `cargo test -p codegg-protocol` | pass — 302 passed, 0 failed |
| `cargo test -p codegg-client` | pass — 65 passed, 0 failed (40 lib + 9 + 16 integration) |
| `cargo test -p codegg --lib` | **7302 passed, 3 failed, 4 ignored** — all three failures are pre-existing and environmental; none touches reasoning. See §4.2 |
| `python3 scripts/check_tui_reasoning_surface.py` | pass — 5/5 checks |
| `python3 scripts/check_tui_reasoning_surface.py --self-test` | pass — 4/4 injected violations rejected |
| `python3 scripts/check_project_catalog_invariants.py` | pass — 7/7 checks |

### 4.1 The three `cargo test -p codegg --lib` failures

None of the three is caused by this milestone, and none touches reasoning.
Each was re-run in isolation to establish that:

| Failing test | Isolated result | Cause |
|---|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | **fails in isolation** | `register workspace: InvalidRootPath("/tmp")` — the sandbox rejects `/tmp` as a workspace root on this macOS host (`/tmp` is a symlink to `/private/tmp`). Environmental; `src/core/daemon_refresh.rs` is not touched by this milestone. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | **fails in isolation** | the same `InvalidRootPath("/tmp")`. |
| `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` | **passes in isolation** (8/8 in the module) | a timing-sensitive worker-count assertion that trips under load in a single-process run. `AGENTS.md` records that broad runs must use the nextest `ci` profile (one process per test) precisely because env mutation in tests is process-global; `cargo test --lib` runs every lib test in one process. |

An earlier full-lib run showed a **different** set of seven failures
(`tui::commands::interactive_terminal::tests` and the same two
`daemon_refresh` tests); re-running those in isolation passed
(36/37 in the module, and the one remaining case
`a_hidden_terminal_surface_does_not_arm_the_loop` passes when run with
`--exact`). The changing failure set across runs is itself the evidence that
these are interference/environment failures rather than regressions.

### 4.2 The guard's own results

```
  [ok] the TUI has one activity frame table and no undocumented second
  [ok] the reasoning surface defines no animation frame table
  [ok] the reasoning surface creates no timer of its own
  [ok] the reasoning surface draws no frame of its own
  [ok] the reasoning indicator never gates the working strip's rows

5/5 checks passed.
```

```
  [PASS] self-test: a second animation frame table is rejected
  [PASS] self-test: a timer in the reasoning surface is rejected
  [PASS] self-test: the reasoning surface reaching for the frame table is rejected
  [PASS] self-test: the strip's row count gated on the reasoning state is rejected
All guard self-test cases passed.
```

Each self-test case copies the tree to a temporary directory, injects one
violation into the copy, and proves the guard rejects it — so a green run is
not vacuous.

## 5. Invariant review

| Plan §4 invariant | Evidence | Result |
|---|---|---|
| A client without a capability payload offers only what it knows, and does not present the known-label ladder as the model's full option set | `reasoning_options(None)` sets `fallback = true`; the note says `"known labels only — this model publishes no reasoning capability"`; tests `absent_capability_falls_back_to_the_known_label_ladder` (protocol and TUI) | holds |
| The frontend's option list is a subset of what the capability permits; the frontend never widens it | `option_set_is_a_subset_of_what_the_capability_permits` asserts every option against the capability | holds |
| The capability is never an authorization input | The payload is display/selection data only; the daemon's admission path is unchanged and still refuses a selection the model does not accept | holds |
| An advisory never creates an escalation on its own | Unchanged — the capability is read on the selection surface only and never feeds a permission decision | holds |
| The models channel keeps its `Vec<String>` shape | `crates/codegg-protocol/src/core.rs` untouched; `models_snapshot_and_refresh_shapes_are_unchanged` | holds |
| The working strip row 0 is reserved unconditionally; its height is subtracted from the transcript | The indicator is a suffix; `the_indicator_does_not_reflow_the_transcript`; the guard fails when `working_strip_rows` mentions the reasoning state | holds |
| One cadence, one `ActivityPhase` enum, one frame table (100 ms) | `tui::components::spinner` and `ActivityPhase` untouched; the guard's frame-table census and the behavioural render test | holds |
| The hint line is not a status surface | The indicator is in the working strip, not the hint line | holds |
| `/reasoning` and `/thinking` stay distinct | `/thinking` untouched; `/reasoning`'s alias set unchanged | holds |
| The `/reasoning` command's existing behaviour is preserved where the capability does not change it — the transient toast, the alias set, the stripping-provider warning | The toast and the stripping warning are still emitted synchronously by the handler; the alias set in `src/tui/command.rs` is unchanged | holds |
| `STORAGE_LAYOUT_VERSION` is unchanged; no migration | 84, unchanged; no migration added | holds |
| The daemon remains the owner of the selection | The turn carries `None`; the write goes through `ReasoningPreferenceSet`; the display is a projection | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** The write is a compare-and-set on
  the daemon's preference revision; a repeated write of the same selection is
  idempotent at the daemon. The frontend's request generation guard drops a
  stale completion, so a late response cannot overwrite a newer one.
- **Cancellation races.** A stale completion is dropped by
  `AsyncUiRequestState::finish`/`fail`; the display only changes on the
  current generation.
- **Daemon restart.** The selection is daemon state (M002), so it survives a
  restart; the frontend re-reads it at start-up through
  `ReasoningPreferenceGet`.
- **Partial persistence failure.** A refused write leaves the daemon's value
  unchanged; the frontend restores the daemon's last known value and shows
  the typed refusal.
- **Malformed or unauthorised input.** The selection is bounded by the
  daemon's own validation (`invalid_reasoning_selection`,
  `reasoning_vocabulary_unsupported`); the frontend never fabricates a label
  the capability did not publish.
- **No daemon connection.** `/reasoning` reports that the selection cannot be
  stored and leaves the display alone, rather than showing a value the daemon
  would ignore.

## 7. Migration and compatibility review

- No schema migration was added; `STORAGE_LAYOUT_VERSION` stays 84.
- The payload is an additive optional field on two DTOs. A client that
  predates it ignores the field (the DTOs do not use `deny_unknown_fields`),
  and a client that requires it sees `None` — "not established".
- The models channel is unchanged, so an old frontend keeps working against a
  new daemon and vice versa.
- The turn's `reasoning` field is unchanged in shape; the **value** the TUI
  sends changed from "the frontend's local mode" to "no override", which is
  the behaviour `docs/providers.md` and `CHANGELOG.md` already documented.
- Rollback: reverting this milestone restores the previous local-only
  behaviour. The daemon's stored preference survives and would simply be
  ignored by the reverted frontend.

## 8. Security review

- The capability payload grants nothing and authorizes nothing. It is
  presentation data; the daemon's admission path remains the authority and
  still refuses a selection the model does not accept.
- The payload carries no secret material: model ids, label names, and token
  bounds only.
- The write path carries no secret: the request is a selection and a
  revision.
- The refusal messages surface the daemon's typed code and message; the
  daemon's messages carry no secret (they are the daemon's own bounded
  rejection text).
- No new path, file access, or process spawning was introduced.

## 9. Documentation and operations

Updated:

- `architecture/tui.md` — the reasoning indicator section (content in the
  reserved row; no second spinner or cadence; the write path; the option
  rules), and the updated `UiState` snippet.
- `docs/tui.md` — a user-facing "## Reasoning control" section: what
  `/reasoning` does, the model-derived options, the fallback, the two things
  that can go wrong, and the daemon's value always winning.

Not updated by this milestone (owned elsewhere): `architecture/provider.md`,
`architecture/protocol.md`, `architecture/overview.md`, `CHANGELOG.md`.

Static guard added: `scripts/check_tui_reasoning_surface.py`.

**The guard is deliberately NOT registered in `scripts/verify.sh` or
`.github/workflows/ci.yml`.** Registration is the integrator's decision and
was explicitly out of scope for this milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `architecture/provider.md` and `architecture/protocol.md` do not yet describe the landed capability payload and the shared option derivation (acceptance criterion 12 is therefore only partially met) | The protocol contract for the new DTO field is documented only in `architecture/tui.md` and in the payload's own doc comments | M003/M005 or the integrator to add the payload and the option-derivation rule to `architecture/protocol.md` and `architecture/provider.md` |
| medium | The WS-attach frontend has no channel for a core request (`TuiMessage` carries no `CoreRequest` variant), so `/reasoning` there reports that the selection cannot be stored and does not change the display | A WS-attach user cannot change the reasoning selection; the indicator still shows the daemon's value, which the frontend reads at start-up only if it has a core client | Either add a `TuiMessage` variant that carries a core request, or accept the limitation explicitly in the protocol documentation. Out of scope for M004: `crates/codegg-protocol/src/tui.rs` was not in this milestone's ownership |
| low | `src/tui/components/messages.rs` carries a **second** animation frame table (`TOOL_SPINNER_FRAMES`, 8 block glyphs on its own 120 ms tick) for the running glyph on a tool-call header. It predates M004, is a per-row transcript affordance rather than the activity indicator, and is recorded as an exact, documented exemption in the guard | The repository's one-frame-table invariant is not literally true of the tree today, although M004 adds nothing of the kind | The tool-call header's glyph could be moved onto `tui::components::spinner`'s frame table, or the exemption removed once it is. Deliberately out of scope for M004: it is an unrelated behaviour change to the tool-call header |
| low | The daemon resolves the capability through the **connection's provider kind**, while turn admission resolves it through the turn's `provider/model` string. They agree for the common configurations (a config provider id and the connection's kind name the same vendor), but a connection whose kind differs from the config provider id can be resolved against a different adapter | In the divergent case the frontend may offer the fallback ladder while admission would accept a narrower set — conservative, not unsafe. The daemon's admission still has the final say | A future milestone could publish the capability for the exact turn model string. Out of scope for M004: `src/core/daemon_turns.rs` is owned by M003 |
| low | The `codegg-client` crate does not consume the per-model capability payload; it only re-exports the shared derivation | None — the plan states that a surface that does not consume the payload is unaffected, and a test asserts it | none |
| low | `cargo test -p codegg --lib` reports three failures in this environment (two `core::daemon_refresh::tests` that fail in isolation with `InvalidRootPath("/tmp")`, and one timing-sensitive `tui::file_tree_coordinator::tests` case that passes in isolation). The integrator's parallel `cargo nextest run --workspace --profile ci --no-fail-fast` finished at **15,797 of 15,800 passed** (2 leaky), and the feature-gated CI lane `cargo nextest run -p codegg --features server,plugins,lsp-test-support --profile ci` at **12,478 of 12,481 passed**, with exactly the same three non-passes: the two `daemon_refresh` failures above (both `InvalidRootPath("/tmp")`, a macOS symlink issue, in files this milestone did not touch) and a **300 s timeout** on `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`, which passes in **0.95 s** in isolation and touches nothing in this subsystem (`asset_context.rs` and `asset_refresh.rs` contain no reasoning code and are unmodified). The machine was running 30+ concurrent cargo builds from other sessions at the time. None of these is caused by this milestone | The milestone's own lib tests all pass; the suite is not green as a whole in this environment for pre-existing reasons | Run the broad suite under the nextest `ci` profile on a host where `/tmp` is a valid workspace root, as `AGENTS.md` prescribes |

## 11. Roadmap disposition

**Milestone closed.** The capability-driven option surface, the persistent
working-strip indicator, and the `/reasoning` daemon write path are
implemented and verified. The two conditions this record first carried —
the missing protocol documentation and the WS-attach transport limitation —
are discharged above (see the note under `Status`): the documentation is
written, and the transport limitation is accepted explicitly rather than left
implicit. Neither was a core correctness or safety gap in the landed path.

## 12. Registry updates

Required in `plans/registry.md` and `plans/subsystems/provider-reasoning-control-roadmap.md`
(the integrator owns both):

- record Milestone 004 as **closed** (it was first recorded as conditionally
  closed; both of its conditions are discharged above);
- carry findings F-1 and F-2 (§10) forward — F-1 is discharged, F-2's
  documentation resolution is taken and its protocol change remains a
  separate decision;
- note that `scripts/check_tui_reasoning_surface.py` exists and is **not yet
  registered** in `scripts/verify.sh` or `.github/workflows/ci.yml`.
