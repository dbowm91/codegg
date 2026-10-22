# Provider Reasoning Control Milestone 004 — Frontend reasoning option surface

Status: ready for handoff (depends on M003 closure)

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`)

Source roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-4--frontend-reasoning-option-surface`

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — new clients tolerate unknown optional fields; security-sensitive capability absence fails closed.
- `plans/000-long-term-specification.md#4.4-frontends-render-projections` — frontends render daemon-owned projections rather than privileged local state.

Applicable ADRs:

- `plans/adrs/ADR-0044-open-provider-scoped-reasoning-control-vocabulary.md` (§4 the open vocabulary, §5 the daemon-owned selection).
- `plans/adrs/ADR-0045-per-model-reasoning-capability-authority.md` (§1 the authority order, §2 the record's shape, §3 the unknown semantics).

Primary class: capability

## 1. Objective

Make the reasoning control **truthful in the interface**: offer only the options the selected model actually accepts, show the current selection **persistently** rather than in a transient toast, and describe an always-reasoning model honestly instead of offering a switch that the provider will reject.

## 2. Why this milestone is ready

- **Hard dependency:** M003 closed. M003 produces the effective per-model capability; without it the frontend has nothing truthful to offer.
- **ADRs:** ADR-0044 and ADR-0045 accepted. See §14.
- **Interface dependency — the established model DTOs.** `ProviderModelDto` and `SelectedModelDto` in `crates/codegg-protocol/src/provider.rs` already carry per-model metadata (`context_window`, `max_output_tokens`, `supports_tools`, `supports_vision`), and the same file already establishes the additive-field pattern this milestone needs: `ConnectionHealthDto::credential_status` uses `#[serde(default, skip_serializing_if = "Option::is_none")]` with a doc comment stating that an absent value "means the peer predates the axis, which reads as 'not established'". The new capability payload follows that pattern exactly.
- **Interface dependency — the working strip.** The TUI's working strip **row 0** is reserved unconditionally and is the main buffer's only status surface; its height is subtracted from the transcript, so gating it on turn state reflows the transcript on every submit and completion. That is where turn state belongs, and therefore where a persistent reasoning indicator belongs.

## 3. Current implementation evidence

Repository baseline `c93211542`.

**The mode is frontend-local and transient.** `src/tui/app/mod.rs` holds the current mode in `self.ui_state.reasoning_mode` and cycles it in the `/reasoning` command handler, emitting a transient toast with `self.ui_state.reasoning_mode.detail()`. The mode's `label()` and `detail()` appear **nowhere in the render path** — a search for `reasoning_mode.label` / `reasoning_mode.detail` across `src/tui` matches only that one toast. So the user cannot see the current mode without issuing the command and reading the transient notice.

**The command cycles a static ladder.** `ReasoningMode::cycle()` maps `Inherit → Effort(Medium) → Effort(High) → Off → Inherit`, with `Budget(_)` mapping straight to `Off` — so `Budget` is never *entered* by cycling, and `Minimal` and `Low` are never reached. The cycle is derived from the type, not from what the model accepts.

**The `/reasoning` command and `/thinking` are different controls.** `/reasoning` changes what the model is asked to do; `/thinking` only toggles whether reasoning text is *rendered*. The implementation notes that distinction explicitly and it must be preserved.

**The frontend already has a per-provider warning that fires on the wrong condition.** The `/reasoning` handler checks `selected_provider_strips_reasoning()` and warns that the provider deletes `reasoning_effort` on the wire. That check is per-provider and means "this provider drops the field", not "this model accepts these depths" — so it fires for a reason unrelated to what the user selected, and does not fire for the generic OpenAI-compatible family (M003 corrects that coverage).

**The model channel is a plain string list.** `CoreResponse::ModelsSnapshot` and `ModelsRefresh` carry `Vec<String>` of `provider/model` identifiers. Changing that channel's shape would be a breaking change to the most-used models path, so the capability payload must ride an existing structured DTO rather than a new channel.

**The structured DTOs are the right carrier.** `ProviderModelDto` and `SelectedModelDto` (`crates/codegg-protocol/src/provider.rs`) already carry per-model metadata and a `catalog_revision`. Neither currently has any `#[serde(default)]` on its fields, so a **required** new field would break a new client decoding an old daemon's response. The additive pattern is `#[serde(default, skip_serializing_if = "Option::is_none")]`, as `ConnectionHealthDto::credential_status` already uses in the same file.

**The TUI's status surfaces have established rules.** The working strip row 0 is the main buffer's only status surface and is reserved unconditionally; the hint line is not a status surface; there is one cadence (one spinner, one frame table, 100 ms) and one `ActivityPhase` enum, and a second spinner or a free-form label is the documented cause of a previously-slowed indicator.

## 4. Invariants that must not regress

- **The working strip row 0 stays reserved unconditionally.** Its height is subtracted from the transcript, so gating it on turn state reflows the transcript on every submit and completion. The reasoning indicator is added to it as content, never as a condition on the row's presence.
- **One cadence, one `ActivityPhase` enum.** The reasoning indicator introduces **no** second spinner, no second timer, and no free-form label competing with the existing activity indicator.
- **`/reasoning` and `/thinking` stay distinct.** One shapes the request; the other governs whether reasoning summaries are rendered. Neither absorbs the other.
- **Frontends render projections.** The frontend's displayed mode is a projection of daemon state. A client that has not received the capability MUST NOT present a capability it did not receive.
- **A client without a capability payload offers only what it knows.** It must not present the known-label ladder as the model's full option set.
- **The models channel is unchanged.** `ModelsSnapshot` and `ModelsRefresh` keep their `Vec<String>` shape.
- **The `/reasoning` command's existing behaviour is preserved** where the capability does not change it — including the transient toast, the alias set, and the existing stripping-provider warning.
- **The capability grants nothing.** It is display and selection data, never an authorization input.

## 5. Scope

### In scope

- The optional capability payload on the structured model DTOs, and the daemon response that populates it.
- The frontend option surface: which options `/reasoning` offers for the selected model.
- The persistent presentation of the current selection in the working strip row 0.
- The always-reasons presentation.
- The desktop and native-client surfaces, to the extent they consume the payload.

### Explicitly out of scope

- **The discovery overlay.** M005's subject. The frontend works from the compiled baseline capability.
- **The capability record and the admission policy.** M003's subject.
- **The open vocabulary and the daemon-owned selection.** M002's subject.
- **The wire-intent fidelity.** M001's subject.
- **Changing the models channel.** `ModelsSnapshot` and `ModelsRefresh` keep their shape.
- **A new UI framework, a new dialog system, or a new keymap.** The option surface reuses the existing command and dialog surfaces.
- **The reasoning *content* rendering.** The `/thinking` display toggle is untouched.
- **Any storage change.**

## 6. Required production changes

### Core/domain

- `crates/codegg-protocol/src/provider.rs` — a capability payload type carrying, for the selected model: the accepted effort labels, whether reasoning can be disabled, whether a fixed budget is accepted and its bounds, and whether the model always reasons. It is added to `ProviderModelDto` and `SelectedModelDto` as `Option<…>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, following `ConnectionHealthDto::credential_status`.
- The payload's doc comment states what an absent value means, in the same spirit as the existing `credential_status` comment: an absent payload means the peer predates the capability, which reads as "not established" and **not** as "this model accepts nothing".

### Storage and migrations

None.

### Protocol and DTOs

- The daemon's model-resolution response populates the payload from the effective capability of the model it resolved.
- The payload is **optional**. A new client against an old daemon receives no payload and MUST fall back to offering only the known-label ladder, and MUST NOT present that ladder as the model's full option set.
- An old client against a new daemon ignores the unknown field, which the `skip_serializing_if` and `#[serde(default)]` combination makes safe.
- The models channel (`ModelsSnapshot`, `ModelsRefresh`) is unchanged.

### Runtime and concurrency

- The frontend's option list for `/reasoning` is derived from the effective capability: the accepted labels the model permits, in the capability's order, plus a disable option only when the model permits a disable, plus a budget option only when the model permits a budget and within its bounds.
- When the capability is absent or `Unknown` on a dimension, the frontend offers the known-label fallback for that dimension and does **not** claim the model accepts nothing.
- The working strip's reasoning indicator reads the daemon-owned selection, not local state. When the daemon's selection differs from the frontend's local display, the daemon's value wins and the local display is corrected.

### Frontend or operator surface

- **The persistent indicator lives in working strip row 0.** It shows the current selection compactly (for example the effort label, or "off", or "default"). It is content in the reserved row, not a condition on the row. It adds no second spinner and no second cadence.
- The `/reasoning` command's option set is capability-driven. Cycling moves through the **model's** options rather than a static ladder. When the model accepts no disable, the cycle does not offer one, and the command explains why rather than appearing broken.
- The always-reasons case is stated: the model always reasons, so there is no off.
- The existing transient toast after `/reasoning` is preserved — it confirms the action — but it is no longer the only place the selection is visible.
- The desktop and native-client surfaces show the selection and the option set to the extent they already surface model metadata. A surface that does not consume the payload is unaffected.

### Security and authorization

- The capability payload is display data. It grants nothing and is never used as an authorization input.
- The payload's accepted-label list is bounded at the source (M003), so the frontend's option list cannot be inflated by a hostile payload.
- The frontend's option list is a **subset** of what the capability permits; the frontend never widens it.

### Documentation and static guards

- `architecture/tui.md` — the working strip's contents gain the reasoning indicator, and the section states that it adds no second cadence.
- `architecture/provider.md` — the reasoning-control section records the option surface's derivation from the capability and the always-reasons presentation.
- `architecture/protocol.md` — the DTO documentation records the new optional field and what its absence means.
- `docs/tui.md` — the user-facing guide documents the persistent indicator and the capability-driven option set, if it documents the thinking control today.
- A static guard is added asserting the TUI renders **no** second activity cadence and no second spinner, consistent with the repository's existing one-cadence invariant. It is registered in `scripts/verify.sh quick` **and** in the CI `verify` job, with a `--self-test`.

## 7. Ordered work packages

### Work package A — Expose the capability on the model DTOs

Intent: Carry the effective capability to the frontend, additively and optionally.

Required changes:
- The payload type and its addition to `ProviderModelDto` and `SelectedModelDto`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- The daemon's model-resolution response populating it.

Acceptance evidence:
- A test decoding a response **without** the payload and asserting it decodes successfully and the payload reads as absent.
- A test decoding a response **with** the payload and asserting it round-trips.
- A test asserting the payload's absence is documented as "not established", not as "accepts nothing" — expressed as a behaviour assertion on the frontend's fallback, not only as a comment.
- A test asserting `ModelsSnapshot` and `ModelsRefresh` shapes are unchanged.

### Work package B — Derive the option set from the capability

Intent: Offer only what the model accepts.

Required changes:
- The capability-driven option derivation, including the known-label fallback for an absent or `Unknown` dimension.
- The budget option's bounds enforcement.
- The disable option offered only when the model permits a disable.

Acceptance evidence:
- A test asserting the option set for a model with a full ladder and a disable.
- A test asserting the option set for an always-reasons model contains **no** disable.
- A test asserting the option set for a model with `Unknown` effort falls back to the known labels and does not present them as the model's full set.
- A test asserting a budget option is offered only within the model's bounds.

### Work package C — Present the selection persistently

Intent: Make the current selection visible without issuing a command.

Required changes:
- The working strip row 0 indicator, reading the daemon-owned selection.
- The daemon's selection winning over a divergent local display.

Acceptance evidence:
- A render test asserting the indicator shows the current selection.
- A render test asserting the working strip's height is unchanged and the transcript does not reflow when the indicator appears or disappears.
- A test asserting the indicator adds no second spinner and no second cadence, checked through the repository's existing activity-phase and cadence structures.
- A test asserting the daemon's selection wins over a divergent local display.

### Work package D — Distinguish the two controls and preserve the existing behaviour

Intent: Keep `/reasoning` and `/thinking` distinct, and keep the command's existing affordances.

Required changes:
- The capability-driven cycle, with the always-reasons case explained rather than appearing broken.
- The transient toast preserved.
- The existing stripping-provider warning preserved (its coverage corrected by M003).

Acceptance evidence:
- A test asserting `/thinking` still governs only the rendering of reasoning text and is unaffected by this milestone.
- A test asserting the `/reasoning` alias set is unchanged.
- A test asserting the transient toast still fires after the command.

### Work package E — Extend the other surfaces and reconcile the documentation

Intent: Keep the desktop and native clients consistent, and document the contract.

Required changes:
- The desktop and native-client surfaces to the extent they consume model metadata.
- The documentation updates listed in §6.

Acceptance evidence:
- A test asserting a surface that does not consume the payload is unaffected.
- A test asserting the surfaces that do consume it show the same option set.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A frontend that receives no capability payload falls back to the known-label ladder. It never presents an empty option set, and never claims the model accepts nothing.
- **Cancellation.** The indicator reflects the selection, not the turn state, so a cancelled turn does not change it. The working strip's turn-state content is unaffected by this milestone.
- **Restart.** The selection is daemon state (M002), so it survives a restart and the indicator reappears with the same value. A frontend that reconnects receives it from the daemon rather than from local state.
- **Duplicate delivery.** A repeated model-resolution response updates the option set idempotently.
- **Contention.** A capability change between turns updates the option set for the next turn. It never mutates an option set mid-interaction in a way that loses a selection: the current selection is either still permitted or is visibly refused.
- **Stale generations.** A client whose option set is older than the daemon's still holds a selection it was allowed to make; the daemon's local policy refuses an unsupported one at admission with a typed error (M003), which is the authoritative backstop.

## 9. Compatibility and migration

- **Protocol.** The new field is optional and additive on both model DTOs, with `#[serde(default, skip_serializing_if = "Option::is_none")]`. An old client ignores it. A new client against an old daemon receives nothing and falls back to the known-label ladder, and must say that it is a fallback rather than presenting it as the model's option set.
- **The models channel is unchanged.** `ModelsSnapshot` and `ModelsRefresh` keep their `Vec<String>` shape; no client sees a breaking change on the most-used models path.
- **Storage.** None.
- **Configuration.** Unchanged.
- **User-visible behaviour.** `/reasoning` now offers the model's options rather than a fixed ladder, and the selection is persistently visible. A user who relied on the old ladder's fixed order sees a different cycle — that is the intent, and it belongs in the release notes.
- **No removal criteria.** The known-label ladder survives as the no-capability fallback.

## 10. Required tests

### Focused unit tests

- The option derivation for a full-capability model, an always-reasons model, and an `Unknown` model.
- The budget bounds enforcement.
- The absence semantics: absent payload ≠ "accepts nothing".

### Integration tests

- The DTO round trip with and without the payload.
- A new client against an old daemon's response: decodes, falls back, and does not present the fallback as the model's option set.
- An old client against a new daemon's response: decodes and ignores the unknown field.
- The desktop and native-client surfaces showing the same option set.

### Restart and recovery tests

- The indicator shows the same selection after a frontend reconnect, because the selection comes from the daemon.

### Contention and cancellation tests

- A cancelled turn leaves the indicator and the selection unchanged.
- A capability change between turns updates the option set without losing a still-permitted selection.

### Security and negative tests

- The option list is a subset of what the capability permits; a frontend never widens it.
- A malformed or oversized payload is rejected at the source boundary and never reaches the option list.
- The capability is never used as an authorization input.

### Migration and compatibility tests

- `STORAGE_LAYOUT_VERSION` is unchanged and the project-catalog invariants guard still passes.
- Both model DTOs decode a payload-less response.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-protocol
cargo test --test tui_render
cargo test -p codegg-client

# the new static guard, including its self-test
python3 scripts/check_tui_single_activity_cadence.py
python3 scripts/check_tui_single_activity_cadence.py --self-test

# the guard must be wired into the routine suite
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
cargo test --workspace --locked
```

Do not claim commands that were not actually run in the closure record. The repository's existing verification policy applies: no new CI lane, no coverage or benchmark gate, no permanent external service.

## 12. Documentation updates

- `architecture/tui.md` — the working strip's contents gain the reasoning indicator; the section states that the indicator adds no second cadence and no second spinner, and that the row stays reserved unconditionally.
- `architecture/provider.md` — the reasoning-control section records the option surface's derivation from the capability, the known-label fallback, and the always-reasons presentation.
- `architecture/protocol.md` — the model DTO documentation records the new optional field and what its absence means.
- `docs/tui.md` — the user guide documents the persistent indicator and the capability-driven option set.
- `CHANGELOG.md` — under `### Changed`: `/reasoning` now offers the selected model's options rather than a fixed ladder, and the selection is persistently visible in the working strip.

## 13. Acceptance criteria

1. The capability is carried on the structured model DTOs as an optional field with `#[serde(default, skip_serializing_if = "Option::is_none")]`; an old client decodes a new daemon's response and a new client decodes an old daemon's response.
2. `ModelsSnapshot` and `ModelsRefresh` keep their `Vec<String>` shape — no breaking change to the models channel.
3. The frontend offers only the options the selected model accepts: its accepted labels, a disable only when permitted, and a budget only when permitted and within bounds.
4. An always-reasoning model is offered **no** disable, and the reason is stated rather than the control appearing broken.
5. A client with no capability payload falls back to the known-label ladder and does **not** present that ladder as the model's option set.
6. The current selection is persistently visible in working strip row 0, reading the daemon-owned selection, and the daemon's value wins over a divergent local display.
7. The indicator adds no second spinner, no second cadence, and no free-form competing label; the repository's single-`ActivityPhase` and one-frame-table invariants are intact.
8. The working strip row 0 stays reserved unconditionally, and the transcript does not reflow when the indicator appears or disappears.
9. `/reasoning` and `/thinking` remain distinct; the `/reasoning` alias set and its transient toast are unchanged.
10. The option list is a subset of what the capability permits, and the capability is never used as an authorization input.
11. `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes.
12. `architecture/tui.md`, `architecture/provider.md`, `architecture/protocol.md`, and `docs/tui.md` describe the landed contract.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **M003 has not closed**, or **ADR-0044 / ADR-0045 are not `accepted`**.
- The capability cannot be exposed additively on the existing model DTOs. A **required** field would break a new client decoding an old daemon, so the agent must not add one; if the payload genuinely cannot be optional, the agent reports that and stops.
- The option surface would require changing the `ModelsSnapshot` / `ModelsRefresh` channel shape. That is a breaking change to the most-used models path and is out of scope.
- The persistent indicator would require a second spinner, a second timer, or a second `ActivityPhase`. The one-cadence invariant is not negotiable.
- The indicator would need to be conditional on turn state, or would change the working strip's height and reflow the transcript. Row 0 is reserved unconditionally.
- The reasoning content rendering (`/thinking`) would have to change. The two controls stay distinct.
- A frontend would have to present a capability it did not receive. Frontends render projections; they do not invent authority.
- The desktop or native-client surfaces would need a new framework, a new dialog system, or a new keymap to render the option set.
- Repository evidence contradicts a canonical invariant in §4.
- The work would expand into the discovery overlay, the capability record, the vocabulary, or the wire-intent fidelity — all of which are other milestones.

## 15. Closure evidence required

The closure record (`plans/closure/provider-reasoning-control/004-status.md`) must contain:

- the requirement-to-evidence matrix covering all twelve acceptance criteria;
- the DTO round-trip results for the with-payload and without-payload cases, quoted;
- the option-derivation results for the full-capability, always-reasons, and `Unknown` cases, quoted;
- a render result showing the persistent indicator, and a render result showing the transcript does not reflow when it appears and disappears;
- evidence that the indicator adds no second cadence or spinner, checked against the repository's existing activity structures;
- the compatibility results for a new client against an old daemon and an old client against a new daemon;
- the `scripts/verify.sh quick` result, the Clippy and `cargo fmt` results, and the workspace test result;
- the project-catalog invariants guard's result and an explicit statement that `STORAGE_LAYOUT_VERSION` did not change;
- the documentation changes made, listed by file, and the `CHANGELOG.md` entry;
- any finding the milestone surfaced for M005, recorded rather than absorbed;
- an explicit statement that the models channel shape, the `/thinking` control, and the one-cadence invariant did not change.

A closure that shows only compilation, or that renders the indicator without showing the transcript did not reflow, is not a closure.

## 16. Handoff notes

- **Working strip row 0 is the only status surface, and it is reserved unconditionally.** Its height is subtracted from the transcript. Put the indicator's *content* in the row; never gate the row's presence on anything, and never on turn state — gating it on turn state reflowed the transcript on every submit and completion once already.
- **Do not add a second spinner or a second frame table.** The repository has one cadence and one `ActivityPhase` enum on purpose; a second indicator with its own cadence is how a watched indicator became ten times slower than the unwatched one.
- **`ProviderModelDto` and `SelectedModelDto` have no `#[serde(default)]` today.** Copy the `ConnectionHealthDto::credential_status` pattern from the same file: `#[serde(default, skip_serializing_if = "Option::is_none")]`, with a doc comment saying what absence means.
- **Absence must not read as "accepts nothing".** A client with no payload falls back to the known-label ladder and says it is a fallback. An empty option set would be a different and much worse failure.
- **`/thinking` is not in scope.** It governs whether reasoning summaries are *rendered*. Do not fold the two controls together, and do not change the reasoning-content rendering.
- **The models channel is `Vec<String>` and stays that way.** Put the payload on the structured DTOs.
- **The desktop and native clients are not required to consume the payload.** A surface that does not consume it is unaffected; the requirement is that the surfaces which do consume it agree.
- **The capability is display data.** It grants nothing and is never an authorization input.
