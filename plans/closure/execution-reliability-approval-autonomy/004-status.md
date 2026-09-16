# Execution Reliability, Approval, and Autonomy M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/004-selected-model-and-runtime-preference-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `c12d633f`

Implementation commits or pull requests:

- `2842a25a` — execution-reliability M004: selected-model and runtime-preference convergence

## 1. Executive finding

M004 is complete. All user-facing model selection converges on the
durable daemon-owned `SelectionService`/`update_selection` path, and the
principal's last-used provider connection/model preference is a
convenience default for otherwise unselected sessions only. `ModelSelect`
no longer has a runtime-only authoritative effect: it resolves the
`provider/model` string through the read-only legacy resolver, performs
a CAS update against the current connection/catalog revision, projects
the canonical `provider/model` string into the runtime cache only after
durable success, and remembers the preference best-effort. New sessions
(`SessionCreate`, `SessionCreateFromTemplate`) and opens
(`SessionLoad`/`SessionAttach`) reuse the exact valid preference with
current revisions; explicit bindings always win; unavailable/unknown/stale
preferences leave the session unselected with a bounded diagnostic and
never silently reroute. `SnapshotSession` repairs the runtime cache from
the durable row, and the TUI manifest hint loses to the daemon snapshot
via `reconcile_tab_model_with_daemon`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Preference fields/resolution (WP-A) | `codegg-core/src/approval.rs`: `RuntimePreference::has_model_preference`, `PreferenceApplicationOutcome::{Applied,ExplicitSelectionPresent,NoPreference,UnavailableConnection,UnknownModel,StaleCatalog}` with stable `code()`/`message()`; unit `preference_application_outcome_codes_are_stable`, `runtime_preference_has_model_preference_gate`, `runtime_preference_serialization_is_secret_free` | pass | M003 columns reused, no migration; catalog revision re-resolved at use, never stored as authority |
| ModelSelect convergence (WP-B) | `src/core/daemon_turns.rs` `ModelSelect` arm: `resolve_model_select_target` → current-revision CAS `service.update` → cache projection via `durable_selected_runtime_model` → best-effort `set_model_preference`; `SessionSelectionUpdate` success arm projects cache + remembers preference; guard `scripts/check_model_select_convergence.py` | pass | Old `*selected = Some(model.clone())` runtime-only pattern removed; failures return typed codes without touching cache |
| New-session restore (WP-C) | `src/core/daemon_sessions.rs`: `apply_model_preference_best_effort` + calls in `SessionCreate`, `SessionCreateFromTemplate`, `SessionLoad`/`SessionAttach` (reload row for DTO after apply) | pass | Best-effort: invalid preferences keep unselected/legacy; project/admin restrictions remain the override hook |
| TUI/projection reconciliation (WP-D) | `src/core/daemon_turns.rs` `SnapshotSession`: durable `Selected` wins + cache repair; `src/tui/app/state/restore.rs::reconcile_tab_model_with_daemon` + manifest doc; `daemon_model_wins_over_manifest_hint` test | pass | Manifest stays additive/display-only; next write persists daemon value |
| Tests/docs (WP-A-D) | `tests/model_preference_convergence.rs` (16 M004 tests + 4 shared harness); `architecture/session.md` convergence section; `architecture/protocol.md`/`core.md` notes; guard script | pass | See §4 |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
Preference domain (codegg-core/src/approval.rs)
  has_model_preference: both last_* present and non-blank
  PreferenceApplicationOutcome + code()/message():
    preference_applied / explicit_selection_present / no_preference /
    preference_connection_unavailable / preference_unknown_model /
    preference_catalog_stale

Selection convergence (src/core/session_selection.rs)
  has_explicit_selection(session): both durable columns present
  canonical_runtime_model(kind, model) -> "provider/model"
  durable_selected_runtime_model(Selected) -> Some(canonical); else None
  resolve_model_select_target(store, "provider/model"):
    Unset/empty -> model_not_specified; provider-only -> model_required;
    unknown -> unknown_provider; ambiguous -> ambiguous_provider;
    disabled/credential-missing -> connection_not_selectable; never fallback
  apply_last_used_preference(session/connection/preference stores):
    explicit -> ExplicitSelectionPresent; absent/incomplete -> NoPreference;
    bad/missing/inactive connection -> UnavailableConnection;
    model absent from bounded catalog -> UnknownModel;
    CAS update with current revs; concurrent bump -> StaleCatalog
  apply_preference_error_code: preference_invalid/conflict/ceiling/unavailable

Daemon (daemon_turns.rs / daemon_sessions.rs)
  ModelSelect: service-required (pool-less fails closed
    session_selection_unavailable); resolve -> CAS update ->
    cache projection + set_model_preference (warn-only on failure) ->
    SessionUpdated + Ack; all failures typed, cache untouched
  SessionSelectionUpdate Updated: cache projection + preference remember
    (warn-only), then origin/audit as before
  SessionCreate/Template/Load/Attach: apply_model_preference_best_effort
    then reload row for DTO; runtime sync when already bound
  SnapshotSession: durable Selected wins + in-place cache repair;
    Unselected/legacy keeps cache (turn-level override safe)

TUI (restore.rs / manifest.rs / snapshot.rs)
  PersistedProjectTab.selected_model_id documented as display hint;
  reconcile_tab_model_with_daemon overwrites on daemon snapshot;
  snapshot_from_tabs unchanged (hint writer)
```

Model-selection ownership before/after:

```text
Before (baseline c12d633f):
  CoreRequest::ModelSelect -> bind_runtime -> runtime.selected_model.write
    -> SessionUpdated -> Ack (durable row untouched)
  SessionSelectionUpdate -> durable update only (no cache, no preference)
  SessionCreate/Load -> no preference application
  SnapshotSession -> runtime cache verbatim (stale after restart)
  TUI manifest hint applied as tab.model with no daemon-wins rule
After (2842a25a):
  CoreRequest::ModelSelect -> resolve_model_select_target ->
    service.update(CAS) -> cache projection + set_model_preference -> Ack
  SessionSelectionUpdate -> durable update -> cache projection +
    set_model_preference -> SessionSelectionUpdated
  SessionCreate/Template/Load/Attach -> apply_model_preference_best_effort
    (explicit wins, invalid stays unselected)
  SnapshotSession -> durable Selected wins + cache repair
  TUI restore -> hint first, daemon wins via reconcile function
Guard: scripts/check_model_select_convergence.py passes.
```

Durable selection/preference precedence table:

```text
explicit durable selection present -> preference not consulted (ExplicitSelectionPresent)
no explicit + valid preference + active connection + known model -> Applied (current revs)
no explicit + no/incomplete preference -> NoPreference (unselected)
no explicit + missing/inactive/unparseable connection -> UnavailableConnection (unselected)
no explicit + model absent from current catalog -> UnknownModel (unselected)
concurrent connection/catalog bump during apply -> StaleCatalog (unchanged)
preference write fails after durable success -> selection stands; warn
  "persisted but last-used preference was not saved"; no rollback
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test model_preference_convergence
cargo test --test session_selection
cargo test -p codegg-core --lib approval
cargo test --test approval_router
cargo test -p codegg --lib tui::app::state::restore
cargo test --test presence_m003_observation
cargo test --test agent_loop_harness -- model
python3 scripts/check_model_select_convergence.py
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

### Results

- `cargo test --test model_preference_convergence`: 20/20 pass (16 M004: applied, explicit-wins, no-preference, removed-model, disabled-connection, 4 resolve-model-select, 2 projection helpers, restart, CAS contention, crafted-ID, pre-M004 rows; plus 4 shared `secret_scan` harness tests).
- `cargo test --test session_selection`: 21/21 pass (existing durable selection contract intact, including stale-revision/catalog, disabled, unknown-model, CAS conflict).
- `cargo test -p codegg-core --lib approval`: 13/13 pass (10 pre-existing + outcome codes, preference gate, secret-free serialization).
- `cargo test --test approval_router`: 16/16 pass (M003 router/preference contract unregressed).
- `cargo test -p codegg --lib tui::app::state::restore`: 15/15 pass (including new `daemon_model_wins_over_manifest_hint`).
- `cargo test --test presence_m003_observation`: 11/11 pass (plan-listed observation gate).
- `cargo test --test agent_loop_harness -- model`: 2/2 pass (plan-listed model gate).
- `check_model_select_convergence.py`: pass (ModelSelect delegates, no runtime-only write, SelectionUpdate projects + remembers, helpers present).
- `check-core-boundary.sh`: pass (new core helper respects boundary; approval domain stays UI-free).
- `check_execution_ownership.py`: pass (no new spawn/scheduler surface).
- `check_tui_project_authority.py`: pass (restore helper is pure state, no daemon authority).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Restart and stale-catalog evidence: `selection_and_preference_survive_daemon_restart` closes and reopens a file DB and asserts both the durable selection and the last-used preference survive; `concurrent_selection_updates_conflict_on_catalog_revision` bumps the health catalog between writes and asserts `StaleCatalog` instead of last-write-wins; TUI `daemon_model_wins_over_manifest_hint` asserts daemon overwrite + idempotence + hint retention without a snapshot.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Explicit existing selection authoritative over preference | `has_explicit_selection` gate first in `apply_last_used_preference`; `explicit_selection_wins_over_preference` keeps `gpt-4o` despite `claude-3` preference |
| No silent switch to a different connection | Resolve path returns `UnknownProvider`/`AmbiguousProvider`/`ConnectionNotSelectable`; preference path returns `UnavailableConnection`; both leave the row unchanged; `crafted_connection_id_is_rejected…` |
| Stale catalog yields diagnostic, not silent fallback | CAS update with current revs; `concurrent_…_conflict_on_catalog_revision` asserts `StaleCatalog`; preference race maps to `StaleCatalog` |
| Credentials never enter preferences/manifest | `runtime_preference_serialization_is_secret_free` allowlists 7 identifier/metadata keys and scans for token/secret/api_key/bearer/password; preference stores only connection/model IDs; TUI manifest carries display hint only |
| Frontends request selection, never construct providers | `SelectionService` remains the only writer; TUI calls `SessionSelectionGet/List/Models/Update` (unchanged) plus daemon-wins reconcile; guard pins the boundary |
| Model changes never alter approval/sandbox | `set_model_preference` touches only `last_*` columns (M003 `write_field` separation preserved); no approval/sandbox code path touched |
| Legacy compatibility readable | `resolve_legacy_model_string` reused read-only; pre-M004 rows yield `NoPreference` (`pre_m004_preference_rows_default_safely`); `ModelSelect` wire shape unchanged (adapter) |

## 6. Failure and recovery review

- Preference write failure after durable success does not roll back the selection; both `ModelSelect` and `SessionSelectionUpdate` log `persisted but last-used preference was not saved` and still acknowledge.
- Selection CAS conflict reloads current state and leaves the stored selection unchanged (`StaleRevision`/`StaleCatalog` arms in both `update_selection` and `apply_last_used_preference`).
- Disabled/deleted connection or removed model yields `connection_not_selectable` / `UnknownModel` / `UnavailableConnection` with the session left unselected; no fallback connection is chosen.
- Restart reconstructs the runtime cache from the durable row (`SnapshotSession` repair + file-DB restart test); the persisted preference is re-resolved against the current catalog on next use, never trusted as revision authority.
- Concurrent model selections resolve through `SelectionService` revision checks; the preference path passes the just-read revisions as CAS expectations so a mid-apply bump surfaces `StaleCatalog`.
- Cancellation during catalog refresh/selection leaves the prior durable selection unchanged (read-only resolve before the single `update` write; no partial writes).

## 7. Migration and compatibility review

- Session selection columns remain canonical; no schema change (M003 v58 reused).
- Preference table fields additive; pre-M004 rows without `last_*` read as `NoPreference` (test).
- `ModelSelect` wire request unchanged (`{session_id, model}` → `Ack`/`Error`); legacy `provider/model` strings resolve through the existing read-only resolver, so old clients keep working via the adapter.
- Legacy `model` strings remain readable via `legacy_resolution` (no semantics changed; `get_selection` untouched).
- TUI manifest schema unchanged (additive `Option` hint already present); only documentation + reconcile behavior changed.

## 8. Security review

- Preference fields are identifiers only (`last_provider_connection_id` ≤512, `last_model_id` ≤512, both NUL/blank-sanitized); no tokens/endpoints/secrets (serialization test).
- Principal is derived server-side (`authority.principal_id()`) in all four new write/read seams (`ModelSelect`, `SessionSelectionUpdate`, create/template/load preference apply); payloads carry no identity, so cross-principal reads/writes are denied by construction (personal-mode `LocalOwner` semantics inherited from M003).
- Crafted connection IDs fail parse or miss the store and yield `UnavailableConnection`, never another principal's scope (test).
- No silent provider fallback anywhere: every invalid state is a typed error/diagnostic with the row unchanged.
- Authorization matrix untouched (selection ops keep their existing `via_session` descriptors); no new capability added.

## 9. Documentation and operations

Updated:

- `architecture/session.md` — selection/preference convergence section (authority, precedence table pointer, guard, TUI reconcile).
- `architecture/protocol.md` — `ModelSelect` noted as the M004 compatibility adapter.
- `architecture/core.md` — `core::session_selection` row notes the M004 adapter/preference/cache.
- `src/tui/app/state/manifest.rs` + `restore.rs` — hint-vs-authority docs on the persisted field and the reconcile function.
- Guard: `scripts/check_model_select_convergence.py` (focused ownership lint; not a new CI lane).

Operator notes: watch `model selection persisted but last-used preference was not saved` (warn: durable selection stands, convenience default skipped) vs `applied last-used model preference to unselected session` (info) vs `model preference left session unselected` (info with `preference_connection_unavailable` / `preference_unknown_model` / `preference_catalog_stale` codes) vs `model preference application failed` (warn with `preference_*` code). `SnapshotSession` cache repairs are silent (debug on selection-read failure only).

No new CI lane: the guard is a focused ownership lint run locally (plan §6 allowance); `verify.sh quick` lanes unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | No open items | — | — |

No stop condition triggered (no frontend credential resolution, no silent fallback, no revision-check bypass, no TUI-manifest authority).

## 11. Roadmap disposition

Milestone closed with a dependency audit (no new `ready` moves):

- M004 (selected-model/runtime-preference convergence): hard dependency was the M003 RuntimePreferenceStore contract. Contract consumed as designed. **Close.**
- M005 (production sandbox policy wiring): hard dependency was the M003 execution-policy contract only; M004 does not gate it. Remains **ready** (unchanged).
- M006 remains **blocked** on M005 (M003+M004 closed).
- M007 remains **blocked** on M005+M006 (M003+M004 closed; was M004-M006).
- M008 remains **blocked** on M005-M007 (M001-M004 closed; was M004-M007).
- No corrective pass required; no deferred product work registered.

## 12. Registry updates

- `plans/registry.md`: M004 `active` → `closed` with closure link and implementation `2842a25a`; subsystem row `M001+M002+M003 closed; M004+M005 ready` → `M001+M002+M003+M004 closed; M005 ready`; dependency-ready table M004 row → `closed`; execution-order item 2 rewritten (M004 closed, M005 ready, M006-M008 still blocked); M004 appended to recently-closed work; blocked-work M007/M008 rows narrowed to remaining gates.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: M004 section `active` → `closed` with closure link; M007 blocker `M004-M006` → `M005-M006`; M008 blocker `M004-M007` → `M005-M007`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/004-selected-model-and-runtime-preference-convergence.md`: `Status: active` → `Status: implemented`.
