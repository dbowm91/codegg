# Provider Reasoning Control Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-reasoning-control/003-per-model-capability-record-and-admission-policy.md`

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-003--per-model-capability-record-and-admission-policy`

Source decision:

- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` (accepted)

Repository baseline reviewed: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5`

Implementation commits or pull requests:

- none — the change is left uncommitted in the working tree, as instructed for
  this milestone.

## 1. Executive finding

Milestone 003 is closed. A **per-model reasoning capability record** now
exists, it lands in every place that must agree with it, the daemon
**refuses an unsupported reasoning selection locally**, and the refusal
names the model and the dimension.

At the baseline the reasoning vocabulary was open and provider-scoped
(M001/M002), so *any* label could be sent — but nothing said whether the
model behind the connection accepted it. A selection the model refused was
forwarded anyway and came back as **silence**: the request looked honoured
and the model reasoned at whatever depth it liked. The local policy is
also **fail-closed**, which the ADR decided twice, including §"Fail-closed
on absence": a dimension nobody has published a contract for is refused at
admission rather than forwarded optimistically.

The record is deliberately *independent per dimension*. Each of the four
dimensions — the accepted effort labels, whether reasoning can be
disabled, whether a fixed thinking budget is accepted (and its bounds),
and whether the model always reasons — carries its own status in the
kernel's vocabulary (`Supported | Unsupported | Unknown | Mixed |
Conflicting`), so an **undeclared** dimension stays `Unknown` and is never
collapsed into `Unsupported`. The kernel's neutral capability type is
**status-only**: it never validates a label, so the accepted-label set
stays in CodeGG's own record and the kernel supplies the per-dimension
decision.

The shipped baseline is deliberately partial. Three families carry a
published contract (`minimax`, `anthropic`, `openai`); four do not
(`google`, `local`, `laguna`, `generic`) and stay `Unknown`, because their
published contracts vary materially across one family and a family-scoped
record that guessed at them would be a fabrication. §10 records what that
costs.

Two audit corrections from the independent review of milestone 002 landed
with it, both inside the same admission and notice surface:

- A **persisted selection that cannot be read** was reported only through
  `tracing::warn!`, which no client ever sees. The person who chose a
  label got a different model behaviour with no indication that their
  choice was gone. The loss is now published as a typed notice.
- The queued-delivery test could not distinguish **delivery-time** from
  **enqueue-time** resolution, because it persisted the selection before
  the queue add. The ordering now makes the test discriminate.

## 2. Requirement-to-evidence matrix

| # | Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | The adapter registry carries a capability record with **independent** effort-label, disable, budget, and always-reasons dimensions, each with an explicit status in the kernel's vocabulary | `crates/codegg-config/src/schema.rs` — `ReasoningCapabilityStatus` (`Supported \| Unsupported \| Unknown \| Mixed \| Conflicting`) and `ReasoningCapabilityRecord` (eight fields across four dimensions); `crates/codegg-core/src/model_profile/capability.rs` — `merge_capability`, `resolve_reasoning_capability`, `overall_status` | pass | The record type lives in `codegg-config` because `ModelProfileConfig` (the same schema crate) must carry it, and `codegg-core/src/model_profile/capability.rs` re-exports it — the same pattern `model_profile/types.rs` already uses for the rest of the profile schema |
| 2 | The record, the `build.rs` mirror, the merge list, `ResolvedModelProfile`, and `ModelProfileConfig` are all updated in one change, and a one-sided change is caught mechanically by a static guard with a self-test | `crates/codegg-core/src/model_profile/adapter.rs` (`AdapterProfile.reasoning_capability` + the narrowing merge + `the_runtime_profile_and_the_build_rs_mirror_declare_the_same_fields`); `crates/codegg-core/build.rs` (`Profile.reasoning_capability` + the `ReasoningCapability` mirror); `crates/codegg-core/src/model_profile/types.rs` (`ResolvedModelProfile.reasoning_capability`); `crates/codegg-config/src/schema.rs` (`ModelProfileConfig.reasoning_capability`); `scripts/check_model_profile_capability_mirror.py` | pass | The merge list was **not** left as an `inherit!` arm: the capability record merges by **narrowing** (`merge_capability`), because `inherit!` would let a more specific adapter *widen* the baseline |
| 3 | An undeclared dimension resolves as `Unknown`, never as `Unsupported` | `crates/codegg-core/src/model_profile/capability.rs` — `an_undeclared_dimension_resolves_as_unknown_not_unsupported`; `crates/codegg-core/assets/model-adapters/{google,local,laguna,generic}.toml` declare **no** record at all | pass | The status vocabulary's `Unknown` is the status of an undeclared dimension; the default `ReasoningCapabilityRecord` is all-`Unknown`, never all-`Unsupported` |
| 4 | The effective capability is resolved per turn, in the order operator override → adapter baseline → discovery overlay → unknown, and the resolution never widens a dimension beyond the baseline | `crates/codegg-core/src/model_profile/capability.rs` — `resolve_reasoning_capability`, `an_operator_override_narrows_the_baseline_and_is_reported_when_refused`, `a_hostile_overlay_cannot_widen_the_baseline`; `src/core/daemon_turns.rs` — `admit_reasoning_capability` | pass | The overlay is absent in this milestone (M005 owns it) but the parameter exists and is proven: a hostile overlay is **rejected in full** and the rejection is reported. The operator override is applied **exactly once** — `apply_config_override` folds it into the profile, so the daemon does not pass it a second time |
| 5 | A selection the effective capability does not support is refused at turn admission with a typed error naming the model and the dimension, and **no provider request is dispatched** | `src/core/daemon_turns.rs` — `an_unsupported_reasoning_selection_is_refused_before_any_provider_request`, `an_explicit_disable_is_refused_for_an_always_reasons_model`; the `RecordingTurnRuntime.called` flag stays `false` | pass | The refusal is `Ok(CoreResponse::Error { code: "reasoning_capability_unsupported", .. })` — a successful protocol exchange carrying a typed error, not a transport failure — and it is distinguishable from M002's `reasoning_vocabulary_unsupported` |
| 6 | A selection on an `Unknown` dimension with no baseline is refused rather than forwarded | `src/core/daemon_turns.rs` — `a_selection_on_an_unknown_dimension_is_refused_at_admission`; `crates/codegg-core/src/model_profile/capability.rs` — `an_undeclared_dimension_resolves_as_unknown_not_unsupported` | pass | ADR-0050's fail-closed-on-absence rule. The runtime was not called |
| 7 | The always-reasons property is a first-class capability value, and the recorded family that publishes it is never offered a disable | `crates/codegg-core/assets/model-adapters/minimax.toml` — `always_reasons_status = "supported"`, `always_reasons = true`, `disable_status = "unsupported"`; `crates/codegg-core/src/model_profile/capability.rs` — `the_always_reasons_family_records_its_always_reasoning_property`, `the_always_reasons_property_stays_one_consistent_fact`; `src/core/daemon_turns.rs` — `an_explicit_disable_is_refused_for_an_always_reasons_model` | pass | The record's own validation refuses `always_reasons = true` together with `disable_status = "supported"` — the contradiction cannot be written |
| 8 | The accepted labels of a populated family are exactly the published set — no invented label, no effort-to-budget conversion, and no normalization of a vendor label | `crates/codegg-core/assets/model-adapters/minimax.toml` — `accepted_effort_labels = ["low", "medium", "high", "xhigh", "max"]`; `crates/codegg-core/src/model_profile/capability.rs` — `the_always_reasons_family_records_its_always_reasoning_property` asserts the set verbatim and asserts that `none` is **not** in it | pass | The set is stored verbatim; the record carries no translation table and the kernel carries the label verbatim (`wire::reasoning_intent`) |
| 9 | `ProviderCapabilities::strips_reasoning_effort`'s coverage matches the adapters that actually omit the field, and the flag is still not used as a "supports reasoning" signal | `crates/codegg-providers/src/provider_core.rs` — `OPENAI_COMPATIBLE_STRIPPING_PROVIDERS`, `strips_reasoning_effort_flag_matches_the_declared_provider_list`, `the_generic_openai_compatible_contract_also_strips_reasoning_effort` | pass | Coverage only: the generic OpenAI-compatible family was added to the flag's list; **no adapter's behaviour changed**, and the omission stays intentional |
| 10 | The capability grants no permission, tool, path, or credential scope, and configuration overrides are bounded and validated at load | `crates/codegg-config/src/schema.rs` — `ReasoningCapabilityRecord::validate` (bounded label list, ≤ 32 labels, ≤ 64 bytes per label, no duplicates, budget range ordering, dimension consistency); `Config::validate` validates every `model_profile.*.reasoning_capability`; `crates/codegg-core/src/model_profile/capability.rs` — `a_malformed_override_or_overlay_is_refused_rather_than_trusted` | pass | The record carries **no** authority: it is data about a model, read by a pure function. A malformed override or overlay is refused, not trusted |
| 11 | `STORAGE_LAYOUT_VERSION` is unchanged, no migration was added, and the project-catalog invariants guard still passes | `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION` is **84**, unchanged by this milestone (M002 owns the `v84` reasoning-selection migration); no migration file was added; `python3 scripts/check_project_catalog_invariants.py` passes | pass | The record lives in the compiled profile and the configuration, not in storage |
| 12 | The documentation names the record's dimensions, the resolution order, and the admission policy | `architecture/model-adapters.md` — "Reasoning Capability Record"; `architecture/provider.md` — "Per-model reasoning capability model (ADR-0050)"; `architecture/config.md` — the model-profile section; `architecture/agent.md` — "Per-Turn Reasoning Capability"; `.opencode/skills/provider-auth/SKILL.md` — rule 12 | pass | The skill that owns `architecture/provider.md` moved with the doc |

## 3. Production implementation evidence

### The record

`ReasoningCapabilityRecord` (`crates/codegg-config/src/schema.rs`),
re-exported by `crates/codegg-core/src/model_profile/capability.rs`:

| Dimension | Fields | Status vocabulary |
|---|---|---|
| effort | `effort_status`, `accepted_effort_labels` | `Supported \| Unsupported \| Unknown \| Mixed \| Conflicting` |
| disable | `disable_status` | same |
| budget | `budget_status`, `min_thinking_budget`, `max_thinking_budget` | same |
| always-reasons | `always_reasons_status`, `always_reasons` | same |

The status and its value are **one fact** for the always-reasons dimension:
`supported` ⇒ `true`, `unsupported` ⇒ `false`, and `unknown` / `mixed` /
`conflicting` leave the value absent. A merge can never produce a record
that says `unsupported` and `true` at once, and
`the_always_reasons_property_stays_one_consistent_fact` proves it.

### Where it lands

`AdapterProfile.reasoning_capability`, the `build.rs` `Profile` mirror, the
`build.rs` `ReasoningCapability` mirror, the adapter merge list,
`ResolvedModelProfile.reasoning_capability`,
`ModelProfileConfig.reasoning_capability`, and the shipped adapter assets.

### The merge rule

`merge_capability(baseline, specific)` narrows and only narrows:

- the merged status of a dimension is the more restrictive of the two
  declared statuses;
- the merged accepted-label set is the **intersection** of the two declared
  sets, so a merge can never widen a dimension's accepted set beyond the
  baseline;
- the merged thinking-budget range is the intersection of the two declared
  ranges;
- the always-reasons property is the narrower claim.

A dimension a side does not declare imposes no constraint, so the generic
baseline — which declares nothing — never erases what a specific adapter
declares.

### The resolution order and the kernel-policy application

`resolve_reasoning_capability(baseline, operator_override, overlay)` resolves
in the order **operator/config override ▸ adapter baseline ▸ discovery
overlay ▸ unknown**. The operator's override is authoritative for the
dimensions it declares and may only narrow the compiled baseline. The
overlay may only narrow: an overlay that declares a label, a bound, or an
always-reasons value the effective record does not already carry is
**rejected in full**, and the rejection is reported in
`CapabilityResolution.rejected` — never silent.

`evaluate_reasoning_selection(model, selection, capability, policy, target)`
applies the effective capability through the kernel's
`reasoning_capability_notices_neutral` and `ReasoningCapabilityPolicy`,
producing exactly one of `Reject`, `WarnDrop`, `AllowWithWarning`, `Allow`.
The kernel's default policy is **fail-closed** — `unsupported → Reject`,
`unknown → Reject`, `mixed → WarnDrop` — confirmed against the pinned
`eggpool-wire` revision
(`rust/crates/eggpool-wire/src/adaptation.rs`, `impl Default for
ReasoningCapabilityPolicy`).

### The admission wiring

The call site is `src/core/daemon_turns.rs`, the `CoreRequest::TurnSubmit`
arm, immediately after M002's `resolve_reasoning_selection` — so the
**resolved** selection (explicit override ▸ persisted principal preference ▸
agent / model-profile default) is what gets judged, never the raw override.
The bound `model` is not reassigned anywhere in the arm before dispatch
(re-verified: the only occurrences are reads), so it is the model the turn
will use.

The helper is `CoreDaemon::admit_reasoning_capability`. It resolves the
model profile with `ModelProfileResolver::new(config).resolve(model)` —
which already carries the operator's configuration override, folded in by
`apply_config_override` — and then applies the **single** resolution
function. The override is therefore applied exactly once, not twice. The
discovery overlay is absent in this milestone; the call is shaped so one
can be passed without changing the gate.

The decision is acted on as follows:

| Decision | Action |
|---|---|
| `Reject` | `Ok(CoreResponse::Error { code: "reasoning_capability_unsupported", .. })` naming the model and the dimension. **No provider request is dispatched.** |
| `WarnDrop` | The turn proceeds with the **narrowed** selection (`ReasoningCapabilityDecision::effective_selection`) and a notice is recorded on the turn's event log. |
| `AllowWithWarning` | The turn proceeds with the selection and an uncertainty notice is recorded. |
| `Allow` | The turn proceeds with the selection unchanged. |

**The notice path** is the daemon's turn event log
(`self.event_log.publish(session_id, turn_id, CoreEvent::Error { code:
"reasoning_capability_notice", .. })`). It is the daemon's own
session-scoped channel, it is the only one that carries a typed code
alongside the message, and the clients already read it. The global app
event bus was not used: its `AppEvent::Error` bridge hardcodes the code
`agent_error`, which cannot distinguish this notice from any other
message.

**A second notice, for a selection that could not be read.** A
preference store that cannot be read is not authority, so the turn falls
back to the agent / model-profile default — but the loss was previously
reported only through `tracing::warn!`, which no client ever sees. A
person who chose a label would get a different model behaviour with no
indication that their stored choice was gone. The read failure is now
published the same way, as
`CoreEvent::Error { code: "reasoning_selection_unreadable", .. }` on the
session's event channel, carrying the underlying error text.

The notice is **session-scoped**, and that is a deliberate choice rather
than a convenience: the daemon's event log delivers a session-less event
to a client only as a global event, and the WS filter
(`event_matches_raw_filter` in `src/server/ws.rs`) **drops** a global
`CoreEvent::Error`. A session-less notice would therefore never reach the
client that asked for the turn. `resolve_reasoning_selection` therefore
takes the session id and every call site passes it.

Four typed codes now coexist, and they arrive on **two different
channels** — which is the distinction that matters to a frontend:

| Code | Channel | Meaning | The turn |
|---|---|---|---|
| `reasoning_vocabulary_unsupported` | `CoreResponse::Error` | the daemon has not advertised the open vocabulary, so the label itself is refused | refused |
| `reasoning_capability_unsupported` | `CoreResponse::Error` | the model's capability record refuses the selection | refused |
| `reasoning_capability_notice` | `CoreEvent::Error` (session event) | a dimension was dropped | proceeds |
| `reasoning_selection_unreadable` | `CoreEvent::Error` (session event) | the persisted selection could not be read | proceeds |

The two refusals are told apart by `CoreResponse::Error.code` and the two
notices by `CoreEvent::Error.code`; the **channel** separates a refusal
from a notice. Neither refusal code was merged or renamed.

**The target surface** is resolved best-effort with
`codegg_providers::provider_profile::resolve_route(provider_id, model_id)`
— the codebase's only wire-routing authority. The daemon therefore passes
**no** surface when that resolution fails, rather than guessing one: the
kernel reads the surface only to label the notice it returns, and
CodeGG's notice text names the model and the dimension instead. The
providers wrapper strips the surface label when the caller supplies none,
so no unverified surface claim leaves the module. See §10 for the gap
this leaves.

### The second admission surface

`CoreRequest::ReasoningPreferenceSet` (the durable preference write,
`src/core/daemon_ops.rs`) carries **no model**: the request names the
principal and the selection, not the model the selection will be used
with. A capability policy cannot be applied to a value whose model is
unknown, and guessing one would refuse a preference that is valid for the
model the user actually selects later. **That surface is left alone**, and
the gap is recorded in §10. The refusal there today is M002's
`reasoning_vocabulary_unsupported`, which is a *vocabulary* check and does
not depend on the model.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-core -p codegg-config -p codegg-providers -p codegg --all-targets -- -D warnings
cargo test -p codegg-core
cargo test -p codegg-providers
cargo test -p codegg --lib
python3 scripts/check_model_profile_capability_mirror.py
python3 scripts/check_model_profile_capability_mirror.py --self-test
python3 scripts/check_reasoning_intent_fidelity.py
python3 scripts/check_reasoning_vocabulary_open.py
python3 scripts/check_project_catalog_invariants.py
```

`bash scripts/verify.sh quick` was **not** run: the integrator runs it once
at the end, and the machine's cargo target-directory lock makes concurrent
sweeps a bottleneck.

### Results

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | **pass** | clean. `cargo fmt --all` was applied to the files this milestone touched; the check is clean afterwards |
| `cargo clippy -p codegg-core -p codegg-config -p codegg-providers -p codegg --all-targets -- -D warnings` | **pass** | exit 0, no warnings on any of the four crates |
| `cargo test -p codegg-core` | **pass** | 907 lib tests plus every integration target, 0 failed |
| `cargo test -p codegg-providers` | **pass** | 305 passed, 0 failed |
| `cargo test -p codegg --lib` | **pass for this milestone** | 7298 passed, 7 failed, 4 ignored. All 22 `core::daemon_turns::tests::*` cases pass, including the eight this milestone added. The 7 failures are 2 pre-existing macOS artifacts (reproduced at baseline), 3 load-sensitive PTY flakes, and 2 in a concurrently-added test module that is not this milestone's — see §4.2 |
| `python3 scripts/check_model_profile_capability_mirror.py` | **pass** | `all checks pass` |
| `python3 scripts/check_model_profile_capability_mirror.py --self-test` | **pass** | 5/5 injected violations rejected, including the string-literal bypass |
| `python3 scripts/check_reasoning_intent_fidelity.py` | **pass** | 5/5 checks — M001's bridge guard still passes |
| `python3 scripts/check_reasoning_vocabulary_open.py` | **pass** | 4/4 checks — M002's vocabulary guard still passes |
| `python3 scripts/check_project_catalog_invariants.py` | **pass** | 7/7 checks, including "storage layout marker tracks highest wired schema migration" |

### 4.1 The quoted evidence

**Resolution order — a populated family.** The MiniMax baseline is
resolved from the shipped adapter, and its published ladder is carried
verbatim:

```text
EVIDENCE_ALWAYS_REASONS_RECORD=ReasoningCapabilityRecord { effort_status: Supported, accepted_effort_labels: Some(["low", "medium", "high", "xhigh", "max"]), disable_status: Unsupported, budget_status: Unknown, min_thinking_budget: None, max_thinking_budget: None, always_reasons_status: Supported, always_reasons: Some(true) }
EVIDENCE_ALWAYS_REASONS_LABELS=["low", "medium", "high", "xhigh", "max"]
```

**Resolution order — an unpopulated family.** A family with no shipped
adapter carries **no record at all**, rather than a record of
`Unsupported`:

```text
EVIDENCE_UNDECLARED_FAMILY_RECORD=None
```

**Resolution order — the always-reasons family.** The same MiniMax
record above carries `always_reasons: Some(true)` together with
`disable_status: Unsupported`, so the family is never offered a disable.
The two facts are consistent by construction: the record's own
validation refuses `always_reasons = true` together with
`disable_status = "supported"`.

**`Unknown` is preserved, never collapsed into `Unsupported`.** With no
record at all, every dimension resolves as `Unknown`:

```text
EVIDENCE_UNKNOWN_RESOLUTION=ReasoningCapabilityRecord { effort_status: Unknown, accepted_effort_labels: None, disable_status: Unknown, budget_status: Unknown, min_thinking_budget: None, max_thinking_budget: None, always_reasons_status: Unknown, always_reasons: None }
```

**The narrowing proof — an injected overlay.** A hostile overlay that
widens the baseline is **rejected in full**, and the effective accepted
set stays the narrowed one:

```text
EVIDENCE_OVERLAY_EFFECTIVE_LABELS=Some(["low", "medium"])
EVIDENCE_OVERLAY_REJECTED=[RejectedCapabilitySource { source: DiscoveryOverlay, reason: "the overlay introduces the effort label 'turbo', which the effective capability does not accept" }]
```

**The admission refusal, and that no provider request was dispatched.**
The runtime's `called` flag stays `false` — the refusal happens at
admission, before the turn is claimed:

```text
EVIDENCE_UNSUPPORTED_MESSAGE=model 'minimax/minimax-m3' does not support the reasoning effort dimension: the model does not accept the reasoning-effort label 'turbo'; the turn was refused
```

**The always-reasons result.** An explicit disable on the always-reasons
family is refused on the **disable** dimension:

```text
EVIDENCE_DISABLE_MESSAGE=model 'minimax/minimax-m3' does not support the reasoning disable dimension: the disable dimension is not supported (kernel field 'reasoning', reason UnsupportedSemanticFeature)
```

**The `Unknown`-preservation result at admission.** A selection on a
family with no baseline is refused rather than forwarded:

```text
EVIDENCE_UNKNOWN_MESSAGE=model 'opencode_go/a-model-without-a-capability-baseline' does not support the reasoning effort dimension: the effort dimension is not supported (kernel field 'reasoning.effort', reason UnsupportedSemanticFeature)
```

**The drop notice.** A partly-supported dimension is dropped, and the
notice says what was dropped:

```text
EVIDENCE_NOTICE=model 'minimax/minimax-m3' does not support the reasoning effort dimension; the effort selection was dropped
```

**The unreadable-selection notice.** A stored selection that cannot be
read is reported with what was lost and the underlying error:

```text
EVIDENCE_UNREADABLE_NOTICE=the stored reasoning selection could not be read; the agent default applies: preference storage error: error returned from database: (code: 1) no such table: runtime_preferences
```

**The published-label comparison.** The recorded set against the
vendor's published set for the MiniMax family — identical, with no
invented label and no translation:

```text
recorded: ["low", "medium", "high", "xhigh", "max"]
published: ["low", "medium", "high", "xhigh", "max"]
```

`none` is **not** in the recorded set: MiniMax-M3.1 refuses an explicit
disable, so the label is not in the published ladder either.

**The stripping-flag coverage.** `ProviderCapabilities::strips_reasoning_effort`
lists exactly the adapters that omit the field — `OPENAI_COMPATIBLE_STRIPPING_PROVIDERS`
plus the generic OpenAI-compatible family — and the flag is still not
used as a "supports reasoning" signal.

**The static guard — pass, injected-field rejection, and self-test.**

```text
$ python3 scripts/check_model_profile_capability_mirror.py
check_model_profile_capability_mirror: all checks pass

$ python3 scripts/check_model_profile_capability_mirror.py --self-test
check_model_profile_capability_mirror --self-test
  ok: injected one-sided runtime field rejected
  ok: injected one-sided mirror field rejected
  ok: injected one-sided capability-record field rejected
  ok: a field declaration inside a string literal is not counted
  ok: unmodified tree still passes
```

The injected-field rejection is what proves the guard is not vacuous: a
one-sided change to the runtime struct, to the `build.rs` mirror, or to
the capability record is refused, and a field name that appears only
inside a string literal does **not** satisfy the check.

**The project-catalog guard.**

```text
$ python3 scripts/check_project_catalog_invariants.py
  [PASS] Catalog module file exists
  [PASS] attach_locator only binds workspace for Local variant
  [PASS] No unwrap_or_default PathBuf anti-pattern
  [PASS] catalog/discovery migrations create tables
  [PASS] v28 migration adds 5 columns to logical_project
  [PASS] storage layout marker tracks highest wired schema migration
  [PASS] lib.rs re-exports project_catalog

7/7 checks passed.
```

**`STORAGE_LAYOUT_VERSION` did not change.** It is **84**, and it was
84 before this milestone: M002 owns the `v84` reasoning-selection
migration. This milestone added **no** migration and no storage file.

**The `build.rs` mirror drift finding, and its disposition.** The
finding is that `crates/codegg-core/build.rs` keeps a **hand-written
serde mirror** of the runtime profile types, and the mirror is
`deny_unknown_fields`. Adding `reasoning_capability` to the runtime
`AdapterProfile` without adding it to the mirror would have made every
adapter asset that declares the field **fail to parse at build time** —
a loud failure, but a confusing one, because the error would name a
TOML key rather than the mirror. The disposition is that the mirror is
now kept honest mechanically by
`scripts/check_model_profile_capability_mirror.py`, which compares the
**field sets** of the runtime struct, the `build.rs` mirror, and the
capability record, and rejects a one-sided change in either direction.
The guard's `--self-test` injects each of the three one-sided shapes
and proves each is rejected.

### 4.2 The failing tests, and why they are not this milestone's

The last full run was `7298 passed, 7 failed, 4 ignored`. The seven
failures are three distinct causes, none of them this milestone's.

**Two — pre-existing, and reproduced at the baseline.**
`core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs`
and `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances`
both panic with:

```text
register workspace: InvalidRootPath("/tmp")
```

`/tmp` is a **symlink** on macOS (`/private/tmp`), and
`canonicalize_workspace_root` refuses a workspace root that is itself a
symlink. The two tests hardcode `/tmp`, so they cannot pass on this
platform. Neither file was touched by this milestone — `git diff` against
the baseline is empty for both — and the same two failures are recorded
in **M002's** closure record for the same reason. The failure was
reproduced at the baseline commit with this milestone's changes stashed,
which is the direct proof that it is pre-existing.

**Three — load-sensitive PTY flakes.**
`tui::commands::interactive_terminal::tests::*`. They spawn a real PTY
and assert the terminal came up, and a fully parallel 7 000-test sweep
can outrun the spawn. The failing subset is **different in every run**
(`a_live_terminal_surface_repaints_with_no_further_user_input` and two
others in the first run; `a_hidden_terminal_surface_does_not_arm_the_loop`,
`panel_rendered_buffer_shows_live_pty_output` and
`reporting_the_mode_never_moves_the_keyboard` in the second), and the whole
module passes in isolation (37 passed, 0 failed).

**Two — a concurrently-added test module that is not this milestone's.**
`tui::app::reasoning_strip_warning_tests::*`. The module
`reasoning_strip_warning_tests` is **new** in `src/tui/app/mod.rs` in this
run's diff, and it belongs to the frontend milestone (M004), which is
explicitly outside this milestone's scope. `turning_reasoning_off_never_warns`
fails in isolation too, with `expected the cycle to land on Off, left:
Effort("medium")`: the test's own `EchoingDaemon` fixture never
advertises the model's reasoning options (its `subscribe` yields
nothing), so the option set the cycle walks is empty. Nothing in this
milestone's code path is involved — this milestone touches the daemon's
turn admission, the preference store read, and the queue delivery, none
of which the TUI's reasoning cycle reads.

### 4.3 The tests this milestone added

`src/core/daemon_turns.rs`:

| Test | What it pins |
|---|---|
| `an_unsupported_reasoning_selection_is_refused_before_any_provider_request` | A label the model does not accept is refused at admission, and the runtime is never called |
| `an_explicit_disable_is_refused_for_an_always_reasons_model` | The always-reasons family refuses an explicit disable on the disable dimension |
| `a_supported_reasoning_selection_reaches_the_turn_unchanged` | A supported selection reaches the request byte-for-byte |
| `a_selection_on_an_unknown_dimension_is_refused_at_admission` | ADR-0050's fail-closed-on-absence rule, at the daemon |
| `a_turn_without_a_reasoning_selection_is_not_gated` | A turn that asks for nothing is never gated by the capability check |
| `a_dropped_reasoning_dimension_is_recorded_as_a_notice` | A partly-supported dimension is dropped, and the notice names what was dropped |
| `the_capability_check_does_not_mutate_the_profile_or_the_config` | The check is read-only |
| `an_unreadable_reasoning_selection_is_reported_as_a_notice` | A persisted selection that cannot be read is **reported**, not silently dropped |

`the_persisted_selection_reaches_a_queued_prompt_delivery` was also
corrected so that it can distinguish the two resolution points: the entry
is enqueued while **no** selection is persisted, the stored entry is
asserted to carry no selection of its own, and only then is the
selection written, before the delivery. An implementation that captured
the selection into the queue entry at enqueue time would deliver **no**
selection, so the test now discriminates instead of passing under
either behaviour.

## 5. Invariant review

| Plan §4 invariant | Evidence | Result |
|---|---|---|
| The record is additive and changes the meaning of no existing profile field | `AdapterProfile` gained one optional field; `ResolvedModelProfile` gained one optional field; `ModelProfileConfig` gained one optional field; no existing field's semantics changed | pass |
| No second policy table | The per-dimension decision comes from the kernel's `reasoning_capability_notices_neutral` and `ReasoningCapabilityPolicy`; CodeGG supplies facts only. The accepted-label set is the *one* place a label list exists, and it lives in the record | pass |
| The kernel's `eggpool-wire` is reached through one declaration | `crates/codegg-providers/Cargo.toml` is the only manifest that declares `eggpool-wire`; no `Cargo.toml` was changed by this milestone; `codegg-core` reaches the kernel through `codegg_providers::capability` | pass |
| The resolution is read-only | `resolve_reasoning_capability` takes `Option<&…>` and returns a new record; the daemon's `admit_reasoning_capability` mutates nothing; `the_capability_check_does_not_mutate_the_profile_or_the_config` | pass |
| No fabricated contract | Four families carry **no** record; the three that do are backed by a published contract (§3, and the table in §3 of this record) | pass |
| The always-reasons property never changes which surface a provider uses | MiniMax still routes through the Anthropic-compatible surface; the milestone records the model contract, not the route | pass |
| The local refusal never depends on a provider round trip | The refusal is computed from the compiled record before dispatch; `runtime.called` stays `false` in the refusal tests | pass |
| A dropped dimension is never silent | `WarnDrop` records a notice on the turn's event log naming the dimension; `a_dropped_reasoning_dimension_is_recorded_as_a_notice` | pass |
| A lost stored selection is never silent either | A preference store that cannot be read is not authority, so the turn falls back to the agent / model-profile default — but the loss is published as `reasoning_selection_unreadable` on the session's event channel; `an_unreadable_reasoning_selection_is_reported_as_a_notice` | pass |

## 6. Failure and recovery review

- **Duplicate delivery / idempotency** — the admission check is a pure
  function of the profile, the override, and the selection; it runs once per
  turn submit and writes nothing.
- **Cancellation races** — not applicable: the check runs before the turn is
  claimed and dispatches no request.
- **Daemon restart** — not applicable: nothing is persisted by the check.
- **Partial persistence failure** — not applicable: the check persists
  nothing. The `WarnDrop` notice is an event-log write, which the event
  log's own persistence owns.
- **Malformed or unauthorized input** — a malformed capability record is
  refused by `validate()` at load (a configuration warning) and refused
  again by the resolution (a reported rejection); a malformed selection
  never reaches the check, because M002's vocabulary check runs first.
- **Bounded event and artifact behavior** — the notice is one
  session-scoped event per dropped dimension.
- **A store that cannot be read** — the turn proceeds on the agent /
  model-profile default, because a preference that cannot be read is not a
  preference. The loss is published as one session-scoped
  `reasoning_selection_unreadable` event carrying the underlying error
  text, so the person who chose a label learns that it was not applied.

## 7. Migration and compatibility review

- **Schema migration** — none. `STORAGE_LAYOUT_VERSION` is **84**, unchanged
  by this milestone.
- **Protocol negotiation** — unchanged. The protocol payload does not expose
  the capability; that is M004.
- **Configuration behavior** — `ModelProfileConfig.reasoning_capability` is
  optional and defaults to absent. An absent override leaves the compiled
  baseline standing. A malformed override produces a configuration
  validation warning and is refused by the resolution.
- **Rollback limitations** — the compiled baseline and the shipped assets
  are code; reverting the milestone removes the record and the refusal
  together. A configuration override that is still present after a revert is
  ignored, because nothing reads it.
- **Legacy-path status** — a turn with **no** reasoning selection is never
  gated by the capability check, so ordinary turns are unaffected
  (`a_turn_without_a_reasoning_selection_is_not_gated`).

## 8. Security review

- **Authorization** — the capability record grants nothing. It is data
  about a model, read by a pure function; it carries no permission, tool,
  path, or credential scope.
- **Secret handling** — the record holds model facts only; no credential,
  token, or path ever enters it.
- **Path validation** — not applicable: no path is read or written.
- **Privilege boundaries** — the refusal happens in the daemon, before the
  turn is claimed and before a provider request exists.
- **Denial-of-service bounds** — the accepted-label list is bounded (≤ 32
  labels, ≤ 64 bytes each, no duplicates) and the thinking-budget range is
  bounded by the record's own validation.
- **Redaction** — the refusal and the notice name the model and the
  dimension. They carry no request body, no credential, and no tool
  arguments.
- **Audit behavior** — a refusal is a typed `CoreResponse::Error` with the
  code `reasoning_capability_unsupported`; a drop is an event with the code
  `reasoning_capability_notice`; an unreadable stored selection is an event
  with the code `reasoning_selection_unreadable`. None of them is an
  authorization decision and none of them is recorded in the authorization
  audit trail, which would be misleading: a capability refusal is a
  model-contract fact, not a permission outcome.

## 9. Documentation and operations

| File | Change |
|---|---|
| `architecture/model-adapters.md` | The record's shape, the merge rule, the resolution order, the admission policy, and the shipped baseline (with the families deliberately left `Unknown`) |
| `architecture/provider.md` | "Per-model reasoning capability model (ADR-0050)": the record, the resolution order, the admission policy, which dimensions each wire surface can express, and the table of the **four typed codes** a frontend has to tell apart (with the channel each arrives on) |
| `architecture/config.md` | The model-profile section records the `reasoning_capability` override and its narrowing rule |
| `architecture/agent.md` | "Per-Turn Reasoning Capability": the read-only per-turn resolution and the local admission check |
| `architecture/overview.md` | The `CI guard scripts` row re-measured (63 → 65) and the stale `Storage layout version` row corrected (83 → 84), each against its own `Source` |
| `CHANGELOG.md` | The `### Changed` entry under `## [Unreleased]` |
| `.opencode/skills/provider-auth/SKILL.md` | Rule 12 (the capability contract), the new guard in the static-guard list, `architecture/model-adapters.md` in *See Also*, and the source-verification entry. **No skill names `architecture/model-adapters.md` as its authoritative document** — it is referenced only from the `architecture-review` skill's review-order table — so the skill that owns the provider contract moved with `architecture/provider.md` |

**Static guards**

| Guard | Rule |
|---|---|
| `scripts/check_model_profile_capability_mirror.py` | The runtime `AdapterProfile` and the `build.rs` `Profile` mirror declare the same field set, and so do the runtime `ReasoningCapabilityRecord` and the `build.rs` `ReasoningCapability` mirror. The scanner is string-aware: a field declaration inside a string literal does not count |
| `scripts/check_reasoning_intent_fidelity.py` | The provider bridge keeps the reasoning intent lossless (M001's guard; re-run because this milestone touches the same crate) |
| `scripts/check_reasoning_vocabulary_open.py` | The open vocabulary and the daemon-owned selection (M002's guard; re-run because this milestone touches the daemon's turn path) |

**Operator diagnostics**

- A refusal names the model and the dimension: `model 'minimax/minimax-m3'
  does not support the reasoning effort dimension: the model does not accept
  the reasoning-effort label 'turbo'; the turn was refused`. The outcome
  clause is **disposition-aware**: a refusal says the turn was refused, and
  only a drop says the selection was dropped. An earlier wording put "the
  effort selection was dropped" inside a refusal, which claimed a forward
  that never happened.
- A drop names what was dropped on the turn's event log under the code
  `reasoning_capability_notice`.
- An unreadable stored selection is reported on the turn's event log under
  the code `reasoning_selection_unreadable`, carrying the underlying error
  text.
- `model_profile.<model>.reasoning_capability` in the configuration is the
  operator's lever: it narrows the compiled baseline, and it is the only
  way to establish a dimension an adapter leaves undeclared.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | **The fail-closed default refuses every explicit reasoning selection for a family left `Unknown`** — `google`, `local`, `laguna`, `generic`. A model with no capability record has every dimension `Unknown`, and ADR-0050's fail-closed-on-absence rule refuses such a selection at admission. Those selections are forwarded today and work. This is a **behaviour change the ADR decided**, not a defect in the implementation, and it is the milestone's most visible consequence. | Users of Gemini, local open-weight models, Poolside Laguna, and the generic fallback lose the ability to select an effort or a thinking budget once the check is live: the turn is refused with `reasoning_capability_unsupported`. A turn with **no** selection is unaffected, and the durable preference path is unaffected, because a selection that is never made is never judged. | Two ways to reduce it, both recorded rather than silently applied: **(a) populate those families' baselines** with their published contracts, the way `minimax`, `anthropic` and `openai` are populated here — the honest route, and the one that makes the record more truthful as well; or **(b) a recorded decision to change the policy** — which needs a new ADR, because ADR-0050 states the fail-closed default twice and this milestone deliberately did not soften it. The rollout order belongs to the integrator. |
| medium | **The daemon cannot cheaply know the turn's target wire surface.** The shared provider profile is the codebase's only wire-routing authority, and it adapts one provider (`opencode_go`) with exact reviewed model hints; the direct adapters each own their surface inline and expose no accessor. The check therefore resolves the surface best-effort and passes **none** when it cannot, and the kernel's surface label is stripped rather than guessed. | The kernel's notice carries no `target_surface` for a turn whose surface the daemon cannot resolve. No user-visible effect: the notice text names the model and the dimension, and the disposition does not depend on the surface. | Give the providers crate a provider→surface accessor owned by the adapters themselves, so the daemon can label the notice with the real surface. A second map in the daemon would be the second policy table the provider roadmap forbids. |
| low | The durable preference write (`CoreRequest::ReasoningPreferenceSet`) cannot apply the capability policy: the request carries no model. | A preference that the model would refuse is refused at every subsequent turn instead of at write time. The refusal is truthful and names the model, so the cost is one failed turn rather than a wrong setting. | Give the preference write the model it applies to, or apply the policy when the preference is *used* — which is what this milestone does. |
| low | **The four reasoning codes now coexist across two channels** — `reasoning_vocabulary_unsupported` and `reasoning_capability_unsupported` as `CoreResponse::Error` (refusals), `reasoning_capability_notice` and `reasoning_selection_unreadable` as `CoreEvent::Error` (notices). A frontend that reads `code` on each channel can tell all four apart, and the channel separates a refusal from a notice. `reasoning_vocabulary_unsupported` has **two** production sites — the turn admission and the preference write — so one code carries two meanings. | A frontend that collapses the codes into a generic "turn failed" loses the distinction, and cannot tell the person that their *stored* choice was dropped rather than that the model refused a label they just typed. | M004 owns the frontend surface. Neither code was merged or renamed here: `reasoning_vocabulary_unsupported` is already documented in `architecture/provider.md`, and renaming either is a separate decision. |
| low | **A provider segment with no shipped model adapter has no capability record either** — `opencode_go` is the live example: it matches no adapter, so it resolves to the generic adapter, whose record is absent. | Every explicit reasoning selection for such a model is refused at admission with `reasoning_capability_unsupported`, exactly as for the four families named above. The refusal is truthful — no contract is published for the model — but the operator has no lever short of a configuration override. | The same route as the fail-closed row above: populate the family's baseline, or give the operator a configuration override. |
| low | The provider→surface gap and the second admission surface are recorded here rather than absorbed into the milestone. | — | M004/M005, or a follow-up plan, per §15's "recorded rather than absorbed". |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The capability record, its five-place landing, the shipped baseline, the
resolution function, the local admission policy, the turn-admission call
site, and the static guard all landed and are verified. The discovery
overlay (M005), the frontend option surface and the protocol payload
exposing the capability (M004) remain open and are untouched here.

## 12. Registry updates

Required in `plans/registry.md` and `plans/subsystems/provider-reasoning-control-roadmap.md`
(the integrator owns both):

- record Milestone 003 as closed, with the unresolved findings in §10
  carried forward — in particular the fail-closed rollout consequence for
  the `google`, `local`, `laguna` and `generic` families.
- carry the two findings that belong to **M004** forward: the four
  coexisting reasoning codes and the two channels they arrive on, and the
  fact that `reasoning_vocabulary_unsupported` has two production sites.
