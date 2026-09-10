# Residual Runtime Consolidation Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/residual-runtime-consolidation/003-core-daemon-construction-lifecycle-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/residual-runtime-consolidation-roadmap.md#M003--CoreDaemon-construction-and-lifecycle-decomposition`

Repository baseline reviewed: `05f06d82` (M002 closure; plan baseline
`9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6` plus the M001 retirement and the
M002 request-family decomposition, which touched no lifecycle ordering)

Implementation commits or pull requests:

- Implementation, closure record, and registry updates land in a single
  commit titled "plans: close residual-runtime M003, decompose CoreDaemon
  construction/lifecycle" (this record's own commit; locate with
  `git log --oneline --grep="close residual-runtime M003"`) — construction/
  bootstrap/refresh/shutdown extraction, lifecycle ordering tests, doc
  reconciliation, M003 closure with the residual roadmap closed

## 1. Executive finding

M003 is complete. `src/core/daemon.rs` went from 6,287 lines to 4,550
lines by moving all construction, bootstrap/recovery, refresh, and
shutdown implementations verbatim into four coherent lifecycle modules
behind the same `CoreDaemon` state, with one canonical in-process bootstrap
helper. `CoreDaemon` remains the single composition/lifecycle authority:
every moved helper is a boring `impl CoreDaemon` method operating on the
same daemon-owned state, and no new store, scheduler, state machine,
service bus, actor/DI framework, supervisor, or authority was introduced.
All lifecycle ordering and ownership semantics are preserved: full root lib
suite (4,426 tests, +19 new lifecycle pins), core suites (185, +19), and the
real-transport projection suite (58) pass; `cargo fmt --check`, workspace
`clippy --all-targets --all-features -D warnings`, and
`scripts/verify.sh quick` pass.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Work package A: capture current construction and lifecycle ordering as tests/diagram | `construction_phase_names` (11 phases), `bootstrap_phase_names` (4), `refresh_coordinator_names` (6), `shutdown_phase_names` (3) plus module-header ASCII diagrams in `daemon_construct.rs`, `daemon_bootstrap.rs`, `daemon_refresh.rs`, `daemon_shutdown.rs`; ordering tests fail on silent reorder | pass | Docs record both bootstrap shapes (in-process hydrate->bridge->recover vs socket/daemon bridge->recover) as pre-existing; no reorder performed |
| Work package B: extract dependency construction/bootstrap helpers without changing concrete ownership | `daemon_construct.rs` (526 lines: `SeamProjectionSink`, `with_deps`, `with_deps_and_identity`, `new` moved verbatim + 5 tests); `InprocCoreClient::initialize_recovery` now delegates to `CoreDaemon::initialize_recovery_sequence` (same 4 calls, same order) | pass | Single assembly point preserved; no partial publish; supplied scheduler/services reuse preserved |
| Work package C: extract recovery/refresh and shutdown/join helpers; keep daemon entry points canonical | `daemon_bootstrap.rs` (732 lines: hydrate, asset-metadata hydrate/persist, subscribe, bridge, recover_state, recover_jobs, replay, event bridge + 5 tests); `daemon_refresh.rs` (949 lines: projection snapshot, asset context, 6 refresh coordinators, binding resolvers + 4 tests); `daemon_shutdown.rs` (126 lines: `Drop` + `abort_background_handles` + 4 tests); all entry paths still `CoreDaemon::method` | pass | `persist_asset_refresh_metadata` widened `private` -> `pub(crate)` (cross-file caller); two join-handle fields widened `private` -> `pub(crate)` (construction/shutdown ownership); no public API change |
| Work package D: remove dead glue/imports and reconcile docs | `daemon.rs` top-import cleanup (7 unused imports removed; `EventFilter` moved into tests); `cargo fix`-equivalent manual pass + `ckroot` zero warnings; docs updated (§9) | pass | No new static guard: ordering tests enforce the invariant instead, per registry verification policy |
| Acceptance: construction, startup/recovery, runtime refresh and shutdown ownership findable without scanning request handlers | §3 module table; `architecture/core.md` inventory + `core` skill lifecycle table | pass | — |
| Acceptance: behavior and ownership remain identical | §5–§7: owner-by-owner review; 4,426 lib + 185 core + 58 projection-transport tests pass; error codes/envelopes untouched | pass | — |
| Acceptance: no new framework or duplicate lifecycle exists | §3 landed changes; `shutdown_has_no_second_bootstrap_path` test; `check_scheduler_bypass.py` + `check_execution_ownership.py` pass | pass | — |

## 3. Production implementation evidence

### Before/after ownership map

Before: `src/core/daemon.rs` (6,287 lines) containing the struct, `Drop`,
`SeamProjectionSink`, full `with_deps_and_identity` constructor (321
lines), all refresh coordinators (453 + 365 lines), all bootstrap/recovery
(114 + 437 lines), plus dispatch/auth/chat/interactive/audit and tests.

After (`wc -l` post-`cargo fmt`):

| Module | Lines | Lifecycle responsibility |
|---|---|---|
| `src/core/daemon.rs` | 4,550 | `CoreDaemon` struct + thin family router + auth/audit preamble + chat handler + interactive runner + shared `pub(crate)` request helpers + existing tests |
| `src/core/daemon_construct.rs` | 526 | `with_deps`, `with_deps_and_identity`, `new`, `SeamProjectionSink`, 11-phase order helper + 5 tests |
| `src/core/daemon_bootstrap.rs` | 732 | `hydrate_workspace_registry`, asset-metadata hydrate/persist, `subscribe`, `bridge_app_event`, `recover_state`, `recover_jobs`, `replay_from`, `start_event_bridge`, `initialize_recovery_sequence` + 5 tests |
| `src/core/daemon_refresh.rs` | 949 | `projection_snapshot_for_session`, asset context, `refresh_project_context`, `refresh_project_activation`, `activate_project_workspace`, `project_health`, `evict_project_activation_leases`, `refresh_runtime_assets`, DTOs, binding resolvers + 4 tests |
| `src/core/daemon_shutdown.rs` | 126 | `Drop`, `abort_background_handles`, 3-phase order helper + 4 tests |
| `src/core/daemon_family.rs` | 339 | Unchanged M002 routing table (166 variants) |
| `src/core/mod.rs` | 1,114 | 4 new `pub mod` declarations + `InprocCoreClient::initialize_recovery` delegate (was 1,117) |

### Landed changes

- New: `src/core/daemon_construct.rs`, `daemon_bootstrap.rs`,
  `daemon_refresh.rs`, `daemon_shutdown.rs`; declared in `src/core/mod.rs`.
- `src/core/daemon.rs`: 1,737 lifecycle lines removed verbatim; struct
  retained with two join-handle fields widened `private` -> `pub(crate)`
  (construction/shutdown ownership, no public API change); `Drop` and
  `SeamProjectionSink` moved; pointer comment records the M003 seam.
- `daemon_bootstrap.rs`: `persist_asset_refresh_metadata` widened `private`
  -> `pub(crate)` so `daemon_refresh` calls the same canonical
  implementation (no logic change); new `initialize_recovery_sequence`
  (`&Arc<Self>` hydrate -> bridge -> recover_state -> recover_jobs) used by
  `InprocCoreClient::initialize_recovery` with byte-equivalent semantics.
- Socket/daemon paths (`src/main.rs` daemon start + server standalone) left
  untouched: they run bridge -> recover without hydrate (pre-existing
  shape, documented in `daemon_bootstrap.rs`; changing it would be a
  behavior change out of scope for this polish milestone).
- Docs: `architecture/core.md` (next-target note, module inventory,
  implementation notes, new lifecycle test coverage),
  `.opencode/skills/core/SKILL.md` (lifecycle ownership table +
  maintenance rules).
- Tests: 19 new lifecycle pins (5 construct + 5 bootstrap + 4 refresh + 4
  shutdown) covering wiring, pool-less shape, identity, service reuse,
  hydrate/bridge/recover order, missing-pool/table tolerance, health/
  activation fail-closed behavior, and abort-on-drop.

Deliberate non-change: the giant constructor was moved verbatim, not
refactored into sub-helpers that could reorder initialization; helper
decomposition beyond file-level grouping belongs to a future
correctness-neutral pass, not to this milestone. No raw line-count guard
was added per the plan's explicit prohibition and the registry
verification policy.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib -- core::
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib --locked -- --test-threads=4
cargo test --features server --test projection_transport_real
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_tool_broker_boundary.py
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib -- core::` | pass — 185 passed, 0 failed (166 pre-existing + 19 new lifecycle pins) |
| `cargo test -p codegg --lib --locked` (full root suite) | pass — 4426 passed, 0 failed (4407 pre-existing + 19 new) |
| `cargo test --features server --test projection_transport_real` | pass — 58 passed, 0 failed (transport/cancellation/publication regression evidence) |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass — zero warnings |
| `scripts/verify.sh quick` | pass (builtin-agents check, core-boundary, sandbox contract, execution-ownership, workspace `cargo check --all-targets`) |
| `check_daemon_cwd_usage.py` | pass — no `current_dir` in protected modules |
| `check_scheduler_bypass.py` | pass |
| `check_execution_ownership.py` | pass |
| `check_tool_broker_boundary.py` | pass |

The plan's literal `cargo test --workspace daemon --no-fail-fast` and
`cargo test --workspace recovery --no-fail-fast` have no matching packages
in this repo (no workspace members named `daemon`/`recovery`); the
equivalent coverage above (full `core::` suite plus the named
projection-transport suite) was run instead and recorded here, matching the
M002 closure precedent. No `codegg-core` code changed, so no core-crate
test sweep was required; `check-core-boundary.sh` passes via
`verify.sh quick`. All verification is local; no `CI / verify` hosted
evidence is claimed.

## 5. Invariant review

| Plan invariant | Evidence it remains true |
|---|---|
| Singleton daemon authority | `CoreDaemon` struct, fields, and `DaemonInstanceGuard` untouched; lifecycle modules are `impl CoreDaemon` methods on the same state; no second daemon type |
| Deterministic initialization order | 11-phase construction order + 4-phase bootstrap order pinned by tests; `with_deps_and_identity` body verbatim; `initialize_recovery_sequence` preserves hydrate->bridge->recover_state->recover_jobs |
| Recovery before new conflicting work | `recover_state`/`recover_jobs` bodies verbatim; scheduler loop still spawned during construction before recovery admits work, as before |
| Scheduler/replay/asset/provider ownership | No scheduler/replay/asset/provisioner code touched beyond moves; job submit still crosses `JobSubmissionService`; projection seam still installed once in construction |
| Joined shutdown | `Drop` body verbatim (`abort` projection-maintenance then worktree-reconcile); scheduler loop still detached by design; socket/pid/metadata cleanup in `main.rs` untouched |
| No leaked tasks/processes | `abort_background_handles` takes-then-aborts (at-most-once); `drop` tests assert `None` handles post-abort; full suites pass with no hang |
| Same configured runtime dependencies | `with_deps`/`with_deps_and_identity`/`new` signatures unchanged; `InprocCoreClient::with_deps`/`new` unchanged; `CoreRuntimeDeps` untouched |
| No second bootstrap path | `initialize_recovery_sequence` is a delegate, not a second implementation; `shutdown_has_no_second_bootstrap_path` test; `check_scheduler_bypass.py` passes |

## 6. Failure and recovery review

This milestone changes no production failure, cancellation, restart, or
contention behavior (plan §8). Every moved helper keeps its original
cancellation source (turn tokens flow through the same `TurnSubmit` body in
family modules), scheduler admission path (job submit through the same
submission facade built in construction), idempotency keys, lock ordering
(workspace-service acquire order inside the same refresh bodies), and
replay/publication semantics (bootstrap arms call the same seam). The
boxed-per-family router and both pre-routers are untouched. Construction
failures unwind without publishing a partially ready daemon (single
`Self { .. }` assembly; covered by pool-less/invalid-shape tests).
Recovery failures retain typed behavior (missing pool returns early;
missing tables warn-and-skip; job failure returns `None`). Shutdown
cancellation precedes joins and process cleanup as today. Stop conditions
never triggered: no split required new durable state, protocol semantics,
scheduler authority, DTO-derived authority, or a coordination framework.

## 7. Migration and compatibility review

- Schema migrations: none (no tables touched; `STORAGE_LAYOUT_VERSION`
  unchanged).
- Wire protocol: unchanged — all 166 `CoreRequest` variants, envelopes,
  error codes, and capability responses are byte-identical.
- Configuration: no keys added, removed, or renamed (`load_config_or_default`
  call site unchanged, still in construction).
- Stored runs: no format change; historical-name readers untouched.
- Public paths: `CoreDaemon`, `with_deps`, `with_deps_and_identity`, `new`,
  `hydrate_workspace_registry`, `recover_state`, `recover_jobs`,
  `refresh_project_activation`, `activate_project_workspace`,
  `project_health`, `handle_request`, `handle_request_for_client`
  unchanged; new modules are crate-internal lifecycle handling (`pub mod`
  declaration, `pub(crate)` helpers) with no new public API. Field widening
  (`private` -> `pub(crate)` for two join handles) is crate-internal only.
- Rollback: revert the single M003 commit; construction/bootstrap/refresh/
  shutdown land together so there is no intermediate mixed-lifecycle state.

## 8. Security review

- No authorization seam changed: the M003 gate (`authorize_request` +
  denial/audit emitters) stays in `daemon.rs` before the router exactly as
  before; denied requests still return with zero side effect.
- No new filesystem, network, or process-spawn site: pure code motion
  plus `pub(crate)` visibility; `check_execution_ownership.py` passes.
- Transport-derived authority preserved: lifecycle helpers cannot mint
  principals; presence/projection artifact paths receive the same boxed
  canonical contexts.
- No secret, credential, or redaction boundary touched; rotation/refresh
  arms untouched in family modules with identical error mapping.
- No denial-of-service surface change: same bounded tasks, same queue
  bounds, same permits; background task lifetimes identical (abort-on-drop
  in same order).

## 9. Documentation and operations

- Updated: `architecture/core.md` (next-target note, lifecycle module
  inventory, implementation notes with widening rationale, new test
  coverage), `.opencode/skills/core/SKILL.md` (lifecycle ownership table +
  maintenance rules).
- New: `daemon_construct.rs`, `daemon_bootstrap.rs`, `daemon_refresh.rs`,
  `daemon_shutdown.rs` module headers carry the canonical before/after
  ownership table and ASCII lifecycle diagrams.
- Historical closure records untouched (no rewrite of M001/M002
  conclusions).
- No new operator diagnostics, static guards, or recovery instructions
  required. The deliberate non-addition of a line-count guard is
  recorded: ordering tests enforce the ownership invariant instead.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No critical/high/medium/low findings remain. The two `pub(crate)`
widenings (§3) are accepted, recorded consequences of physical
decomposition — not findings: they change no public API and no runtime
behavior. The pre-existing socket/daemon bootstrap shape (bridge->recover
without hydrate, §3) is documented as observed behavior, not a finding:
changing it would be a correctness-plan decision, explicitly out of scope
per the plan handoff notes.

## 11. Roadmap disposition

Milestone closed and the subsystem roadmap closes with it. Per the roadmap
completion definition (M001–M003 accepted closures, stranded surfaces gone,
docs truthful, `CoreDaemon` materially less concentrated without a new
orchestration abstraction), all three exit conditions now hold:

- M001: `closed` (stranded team/shell-session surfaces retired).
- M002: `closed` (request-family decomposition).
- M003: `closed` (this record).

Unblock audit: no registered implementation plan lists residual M003 as a
hard or interface dependency. The registry `Blocked work` section (arch
M009, runtime C002, TP expansion M002/M003) is unaffected — none is gated
on residual lifecycle decomposition, so the audit moves nothing to
`ready`. No corrective follow-up is registered; future daemon work proceeds
under ordinary module ownership without a new roadmap.

## 12. Registry updates

- `plans/registry.md`: residual-roadmap row `M001 closed, M002 closed, M003
  active` → `closed` with `M001–M003 closed`; M003 row removed from the
  dependency-ready table and recorded in the control-points table;
  execution-order gate 1 updated (M001–M003 closed, roadmap closed);
  blocked-work rows unchanged (unblock audit moves nothing; §11).
- `plans/subsystems/residual-runtime-consolidation-roadmap.md`: Status
  `active` → `closed`; M003 row `active` → `closed` with closure-record
  link.
- `plans/implementation/residual-runtime-consolidation/003-core-daemon-construction-lifecycle-decomposition.md`:
  status `active` → `implemented` (closed via this record).
