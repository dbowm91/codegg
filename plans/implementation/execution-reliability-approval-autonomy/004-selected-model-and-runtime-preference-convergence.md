# Execution Reliability, Approval, and Autonomy M004 — Selected-Model and Runtime-Preference Convergence

Status: active

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: capability / infrastructure

Hard dependency: M003 RuntimePreferenceStore contract.

## 1. Objective

Converge all user-facing model selection on the durable daemon-owned provider/session selection service and use the RuntimePreferenceStore to remember the principal's last valid provider connection/model for new or otherwise unselected sessions. Eliminate runtime-only `ModelSelect` state as an authoritative path while preserving explicit session binding and catalog revision safety.

## 2. Why this milestone is blocked

M003 must first land the principal-scoped RuntimePreferenceStore and precedence contract. Existing durable session selection is otherwise already available.

## 3. Current implementation evidence

- `src/core/session_selection.rs` owns durable provider connection/model selection with connection revision, catalog revision, active-state validation, stale update diagnostics, and no silent credentialed endpoint substitution.
- session rows contain `provider_connection_id`, `provider_connection_revision`, `model_catalog_revision`, and `selected_model_id` plus legacy model/provider strings.
- `CoreRequest::ModelSelect` in `src/core/daemon_turns.rs` currently only mutates `runtime.selected_model` and publishes `SessionUpdated`.
- TUI manifest persists `selected_model_id` as a display-only restoration hint and explicitly states that manifest preferences do not influence daemon authority.
- snapshot/projection paths can show runtime selected model separately from durable session data, creating potential divergence after restart.

## 4. Invariants that must not regress

- an explicit existing session selection is authoritative over last-used preference;
- preference cannot silently choose a different provider connection when its remembered connection/model is unavailable;
- stale catalog revision produces a diagnostic/selection flow, not silent fallback to another model;
- credentials remain daemon/provider-store owned and never enter preferences/TUI manifest;
- frontends request selection but do not construct providers or resolve secrets;
- model selection changes do not alter approval/sandbox mode except the separate preference record fields explicitly updated by user action;
- legacy session/model compatibility remains readable.

## 5. Scope

### In scope

- route `CoreRequest::ModelSelect` or replace it compatibly with durable SessionSelection update;
- update runtime selected-model cache only after durable selection succeeds;
- write principal last-used connection/model preference after successful explicit selection;
- restore preference for new/unselected session only after current catalog validation;
- frontend-neutral get/effective/fallback diagnostics;
- TUI restoration reconciliation so display hints cannot override daemon state;
- tests/docs.

### Explicitly out of scope

- provider credential storage changes;
- automatic semantic model routing changes;
- silent provider fallback;
- changing provider connection lifecycle;
- Automatic reviewer model choice (M006 can consume the model-selection infrastructure separately);
- TUI-only preference authority.

## 6. Required production changes

### Core/domain

Extend M003 RuntimePreferenceStore with stable `last_provider_connection_id`/`last_model_id` fields if not already present. Store no catalog revision as lasting authority; catalog revision is re-resolved at use.

Define resolution result types for preference application: `Applied`, `UnavailableConnection`, `UnknownModel`, `StaleCatalog`, `ExplicitSelectionPresent`, `NoPreference` or equivalent.

### Durable selection convergence

Refactor simple `ModelSelect` handling so it invokes the daemon `SelectionService`/`update_selection` semantics. If protocol lacks required expected revisions, either:

- resolve the current connection/catalog revision first and perform a CAS update; or
- add an explicit newer selection request and retain `ModelSelect` as a compatibility adapter that delegates to it.

Do not update `runtime.selected_model` before durable success.

### New-session preference application

At session creation/open when there is no explicit durable selection:

1. load principal preference;
2. resolve exact remembered provider connection;
3. verify connection active and credential state through existing selection service;
4. verify remembered model exists in current bounded catalog;
5. apply durable selection with current revisions;
6. if invalid, keep session unselected/legacy state and return bounded diagnostic rather than selecting another endpoint/model.

Project/admin/model restrictions, if present, override preference.

### Runtime cache/projection

Make runtime `selected_model` a projection/cache of durable selection or turn submission, not an independent source. SnapshotSession should not contradict durable session selection after restart.

### Frontend/TUI

On restore, treat manifest selected_model_id only as display hint until daemon selection snapshot arrives. If mismatch, daemon value wins and manifest can be refreshed on next write.

### Security

Preference fields are identifiers only; no tokens/endpoints/secrets. Cross-principal preference reads/writes are denied in future team mode, with LocalOwner explicit in personal mode.

### Documentation/static guards

Document selection-preference distinction. Add tests/static guard preventing new daemon ModelSelect handlers from mutating only runtime cache without durable store update.

## 7. Ordered work packages

### Work package A — Preference fields/resolution

Add last connection/model fields and bounded validation/result types to RuntimePreferenceStore.

### Work package B — ModelSelect convergence

Delegate existing protocol path to SelectionService, preserve compatibility responses, update cache after success.

### Work package C — New-session restore

Apply preference only for unselected sessions and only after exact connection/catalog validation.

### Work package D — TUI/projection reconciliation

Make daemon selection authoritative in snapshot/restore and expose bounded invalid-preference diagnostics.

## 8. Failure, cancellation, restart, and contention semantics

- preference write failure after a successful session selection does not roll back the explicit session selection; report that convenience preference was not saved;
- selection CAS conflict reloads current selection and does not overwrite another frontend's choice;
- connection disabled/deleted or model removed causes explicit diagnostic, no fallback;
- restart reconstructs runtime cache from durable session selection;
- concurrent model selections resolve through SelectionService revision checks;
- cancellation during catalog refresh/selection leaves prior durable selection unchanged.

## 9. Compatibility and migration

- existing session selection columns remain canonical;
- preference table fields are additive;
- existing `ModelSelect` wire request remains supported through adapter if practical;
- legacy `model` strings remain readable by existing legacy resolution rules;
- TUI manifest schema need not change unless display-hint cleanup requires an additive field.

## 10. Required tests

### Focused unit tests

- preference resolution states;
- explicit-session-selection precedence;
- removed model/disabled connection behavior;
- no secret fields in preference serialization.

### Integration tests

- ModelSelect persists selection and runtime projection matches;
- new session reuses last valid model/connection;
- explicit session selection is not overwritten by preference;
- removed remembered model leaves session unselected/diagnostic rather than fallback.

### Restart and recovery tests

- daemon restart preserves existing session selection and last-used preference;
- TUI restart manifest mismatch resolves to daemon state.

### Contention and cancellation tests

- simultaneous selection updates produce CAS conflict rather than last-write-wins;
- connection/catalog revision changes during selection.

### Security and negative tests

- no credential material in preference/protocol;
- invalid/crafted connection ID cannot access another scope.

### Migration and compatibility tests

- pre-M004 preference rows default fields safely;
- legacy ModelSelect client path still works.

## 11. Required verification commands

```bash
cargo test --test session_selection
cargo test -p codegg-core -- session
cargo test --test presence_m003_observation
cargo test --test agent_loop_harness -- model
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/session.md`
- provider connection/selection architecture docs;
- TUI restoration documentation;
- core protocol docs.

## 13. Acceptance criteria

- `ModelSelect` no longer has a runtime-only authoritative effect;
- successful explicit selection is durable and updates last-used preference;
- new/unselected session reuses exact valid preference;
- existing explicit session selection always wins;
- unavailable/stale preference never silently reroutes to another provider/model;
- restart/TUI restore shows consistent daemon-owned selection.

## 14. Stop conditions

Stop if implementation would require frontend credential resolution, silent provider fallback, bypassing SelectionService revision checks, or making TUI manifest authoritative.

## 15. Closure evidence required

- before/after model-selection ownership trace;
- durable selection/preference precedence table;
- restart and stale-catalog tests;
- TUI reconciliation evidence;
- exact verification commands and residual compatibility notes.

## 16. Handoff notes

Treat “last used model” strictly as a convenience default for otherwise unselected work. It is not a reason to mutate an established session's explicit provider/model binding.
