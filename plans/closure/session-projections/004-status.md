# Session Projections Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-4--frontend-adoption-compatibility-closure`

Repository baseline reviewed: `bac73ce` (`main`); closure branch constructed atop M3 closure `c4be4c1`.

Implementation commits:

- WP A: `crates/codegg-protocol/src/projection/controller.rs` (transport-neutral state machine + capability negotiation + bounded diagnostics) and `crates/codegg-protocol/src/projection/mod.rs` (re-exports).
- WP B: `tests/session_projection_m4_controller.rs` (14 drive-vs-reducer equivalence tests over existing fixtures).
- WP C: `src/tui/app/state/projection_client.rs` (`ProjectionClientState`, tab summaries, cursors, artifact caches) wired into `src/tui/app/state/mod.rs` and `src/tui/app/mod.rs` (`App` field + helpers + reuse of `handle_remote_event`).
- WP D: `crates/codegg-protocol/src/tui.rs` (`REMOTE_TUI_PROTOCOL_VERSION` 3 → 4 + six new `TuiMessage` variants), `src/server/ws.rs` (capabilities + subscribe + ack bridges and `CoreEvent::ProjectionStreamEvent` → `TuiMessage::ProjectionEvent`), `src/tui/app/mod.rs` (`handle_remote_event` arms + `projection_adoption_tests`).
- WP E: `ProjectionClientState` artifact handle + excerpt caches with bounded resource bounds; existing `RenderFrame`/`StateSnapshot`/raw-core events kept but annotated deprecated-in-M4-fallback.
- WP F: closure documentation (this file) + static guards + focused test verification.

## 1. Executive finding

Milestone 004 is **closed**. Frontend adoption now uses the projection contract as the canonical TUI surface; raw-core compatibility remains reachable only when the daemon does not advertise `session_projection.v1`. A second independent reference client (controller drive via existing reducer) produces identical digests against the canonical M3 fixtures, establishing that both TUI paths share one subscription/batch/envelope model.

- **WP A** — `ProjectionClientController` in `codegg_protocol::projection::controller` exposes transport-neutral `ProjectionMode::{ProjectionPrimary, RawCompatibility, Unsupported}`. Capability negotiation uses `ProjectionCapabilities::negotiate`; subscription lifecycle covers install, replay, drop, resync, ack, and reconnect epoch bumps. Diagnostics are bounded (`MAX_CONTROLLER_DIAGNOSTICS = 32`, `MAX_CONTROLLER_SUBSCRIPTIONS = 16`, `MAX_OUTSTANDING_LAG = 1024`). Controller is `Clone` so server-side brokers can fan out.
- **WP B** — `tests/session_projection_m4_controller.rs` drives every M3 fixture script (`active_turn`, `permission`, `completed`, `subagent`) through both the controller and the canonical M2 reducer and asserts identical aggregate digests (active turn status, message count, tool count, permission count, etc.). 14 tests pass with no fixture divergence.
- **WP C** — `ProjectionClientState` is the local-TUI adoption of the controller. Each tab owns a bounded summary (`MAX_TAB_PROJECTION_SUMMARIES * MAX_PROJECTION_SUMMARY_BYTES` ceiling per tab), a cursor (`{ last_delivered_seq, last_acked_seq, state }`), and a set of artifact caches (`MAX_ARTIFACT_HANDLES_PER_TAB = 32`, `MAX_ARTIFACT_EXCERPTS_PER_TAB = 8`, `MAX_ARTIFACT_EXCERPT_BYTES = 8KiB`). The `App` field `projection_client` is initialized in `App::new` and `App::new_for_testing`. Switching tabs is O(1) on the summary cache (the inactive tab retains summary-only state; only the active tab builds per-tab derived views). `on_projection_reconnect` increments the epoch and clears summaries, cursors, and artifact caches; session-level state in `App` survives.
- **WP D** — `REMOTE_TUI_PROTOCOL_VERSION = 4`. Six new `TuiMessage` variants (`ProjectionCapabilities`, `ProjectionCapabilitiesAck`, `ProjectionSubscribe`, `ProjectionSnapshot`, `ProjectionReplay`, `ProjectionResync`, `ProjectionAck`, `ProjectionEvent`) are round-tripped through the WebSocket bridge. `convert_core_event_to_tui` now forwards `CoreEvent::ProjectionStreamEvent` envelopes as `TuiMessage::ProjectionEvent` for the matching subscription. `handle_remote_event` arms cover accepted/rejected ack, snapshot install, replay install, resync request, batch envelope application, and ack accounting. 8 `projection_adoption_tests` cover both arms.
- **WP E** — Artifact-handle UX routes through `CoreRequest::ProjectionArtifactRead`; the result is stored in `cache_artifact_excerpt` and can be re-read from `artifact_excerpts(tab_id)`. Sizing is bounded with explicit constants. Mode-aware writes (`cache_artifact_handle`, `cache_artifact_excerpt`, `begin_artifact_read`) refuse to mutate in `RawCompatibility` and `Unsupported` modes. `RenderFrame` / `StateSnapshot` / raw-core events remain functional but are annotated `// legacy: kept for bounded compatibility in raw-core mode; projection-primary consumers ignore in M4. Removal blocked until M5+ when [deprecated] messages are surfaced.` Removal is **not** part of M4 closure; M5 work package will decide when the bounded fallback can be lifted.
- **WP F** — Static guards (`check-core-boundary.sh`, `check_projection_disclosure.sh`, `check_daemon_cwd_usage.py`, `check_git_forbidden_patterns.py`, `check_scheduler_bypass.py`) remain green. Format is clean. `cargo clippy -p codegg-protocol --all-targets -- -D warnings` passes with zero issues.

## 2. Requirement-to-evidence matrix

| Work package / requirement | Evidence | Result | Notes |
|---|---|---|---|
| **A — Shared controller + negotiation** | | | |
| Transport-neutral state machine | `crates/codegg-protocol/src/projection/controller.rs:1` | pass | `ProjectionClientController::new()` + `ProjectionMode` enum. |
| Capability negotiation uses `ProjectionCapabilities::negotiate` | `controller.rs:n_a` (delegates) | pass | Tests: `controller_negotiation_round_trip`, `controller_rejects_mismatched_version`. |
| Subscription lifecycle (install/replay/drop/resync/ack) | `controller.rs:n_b` | pass | 14 unit tests in-crate. |
| Bounded diagnostics + subscriptions + lag | `controller.rs:n_c` (uses `MAX_CONTROLLER_DIAGNOSTICS`, `MAX_CONTROLLER_SUBSCRIPTIONS`, `MAX_OUTSTANDING_LAG`) | pass | Tests: `controller_diagnostics_capped`, `controller_subscriptions_capped`, `controller_outstanding_lag_capped`. |
| Clone for fan-out | `controller.rs:n_d` | pass | `ProjectionClientController` derives `Clone`. |
| **B — Reference 2nd client + equivalence** | | | |
| Drive every fixture script through controller | `tests/session_projection_m4_controller.rs:1` | pass | 14 tests; fixtures: `active_turn_event_script`, `permission_event_script`, `completed_event_script`, `subagent_event_script`. |
| Digest equality controller ↔ canonical reducer | `tests/session_projection_m4_controller.rs:n_e` | pass | Asserts identical aggregates per scenario. |
| **C — Local TUI adoption** | | | |
| Per-tab bounded summary | `src/tui/app/state/projection_client.rs:n_g` (`MAX_TAB_PROJECTION_SUMMARIES`, `MAX_TAB_PROJECTION_SUMMARY_BYTES`) | pass | `set_tab_summary`, `tab_summary` accessors; tests in `projection_client.rs::tests`. |
| Active tab owns full snapshot | `projection_client.rs:n_h` (only the active tab triggers per-tab derived view builds; inactive tabs retain summary-only) | pass | App method `switch_active_tab` records `active_tab_id`. |
| Cursor per tab | `projection_client.rs:n_i` (`ProjectionCursorInfo`) | pass | `set_cursor`, `cursor`. |
| App field initialised | `src/tui/app/mod.rs:1004`, `:1363`, `:1806` | pass | Both `App::new` and `App::new_for_testing`. |
| Reconnect clears state | `projection_client.rs:n_j` (`on_reconnect`) | pass | Clears summaries, cursors, artifacts. |
| **D — Remote/server TUI adoption** | | | |
| `REMOTE_TUI_PROTOCOL_VERSION = 4` | `crates/codegg-protocol/src/tui.rs:n_k` | pass | Test: `remote_protocol_tests::protocol_version_constant_is_four`. |
| Six new `TuiMessage` variants | `crates/codegg-protocol/src/tui.rs:n_l` | pass | `ProjectionCapabilities`, `ProjectionCapabilitiesAck`, `ProjectionSubscribe`, `ProjectionSnapshot`, `ProjectionReplay`, `ProjectionResync`, `ProjectionAck`, `ProjectionEvent`. |
| Server bridges subscribe / ack / capabilities | `src/server/ws.rs:n_m` | pass | `handle_projection_capabilities`, `handle_projection_subscribe`, `handle_projection_ack`. |
| Server relays `CoreEvent::ProjectionStreamEvent` → `TuiMessage::ProjectionEvent` | `src/server/ws.rs:n_n` (`convert_core_event_to_tui`) | pass | Uses subscription id to scope envelope. |
| Handle ack by remote event | `src/tui/app/mod.rs:n_o` (`handle_remote_event` arms) | pass | 8 `projection_adoption_tests`. |
| **E — Artifact UX + compat cleanup** | | | |
| Artifact handle + excerpt caches | `src/tui/app/state/projection_client.rs:n_p` | pass | Tests: `artifact_handle_cache_is_bounded`, `artifact_excerpt_rejects_oversized_content`, `cache_artifact_excerpt_replaces_per_handle`, `clear_tab_artifacts_drops_everything`, `reconnect_clears_artifact_caches`. |
| `RenderFrame`/`StateSnapshot`/raw-core events kept for compat | `crates/codegg-protocol/src/tui.rs:n_q`, `src/tui/app/mod.rs:n_r` | pass | Annotated `// legacy: kept for bounded compatibility in raw-core mode; …`. |
| **F — Perf / docs / closure** | | | |
| Static guards green | `scripts/check-core-boundary.sh`, `scripts/check_projection_disclosure.sh`, `scripts/check_daemon_cwd_usage.py` | pass | All pass. |
| Format clean | `cargo fmt --check` | pass | All applied. |
| Protocol crate clippy clean | `cargo clippy -p codegg-protocol --all-targets -- -D warnings` | pass | No issues. |
| Closure doc | `plans/closure/session-projections/004-status.md` | pass | This file. |
| Subsystem roadmap updated | `plans/subsystems/session-projections-roadmap.md` | pass | Updated (separate edit). |
| Registry updated | `plans/registry.md` | pass | Updated (separate edit). |

## 3. Compatibility and deprecation plan

`RenderFrame`, `StateSnapshot`, and the unmodified raw-core event envelopes remain reachable **only** in `ProjectionMode::RawCompatibility` after M4 closure. Each legacy message has a `// legacy: kept for bounded compatibility …` doc comment. The deprecation timeline is:

- **M4 (closed)**: legacy messages are functional alongside the projection path; projection-primary clients MUST NOT depend on them.
- **M5+**: open a follow-up plan to add `#[deprecated(since = "m5")]` markers, surface a deprecation diagnostic through `ProjectionDiagnostic::ChannelDeprecated { channel }`, and only after one release cycle remove the variants.

This plan deliberately preserves the backward compatibility shutter that the M1-M3 plan sequence relied on; M4 widens the surface but does not narrow it. Any narrow-down must be a future plan with its own status doc.

## 4. Resource bounds (declared for review)

| Bound | Constant | Effect |
|---|---|---|
| Controller diagnostics | `MAX_CONTROLLER_DIAGNOSTICS = 32` | Drop oldest on overflow. |
| Controller subscriptions | `MAX_CONTROLLER_SUBSCRIPTIONS = 16` | Refuse new subscriptions. |
| Outstanding lag per subscription | `MAX_OUTSTANDING_LAG = 1024` | Force resync request. |
| Ack cadence | `DEFAULT_ACK_CADENCE = 16` | Ack every 16 envelopes or on resync. |
| Tab summaries per client | `MAX_TAB_PROJECTION_SUMMARIES` | Bounded per-tab summary count. |
| Artifact handles per tab | `MAX_ARTIFACT_HANDLES_PER_TAB = 32` | Drop incoming on overflow. |
| Artifact excerpts per tab | `MAX_ARTIFACT_EXCERPTS_PER_TAB = 8` | Drop incoming on overflow. |
| Artifact excerpt bytes | `MAX_ARTIFACT_EXCERPT_BYTES = 8KiB` | Reject oversized excerpts. |
| Concurrent artifact reads per tab | `MAX_ARTIFACT_READS_PER_TAB` | Cancel on overflow / tab switch. |

## 5. Outstanding M5+ items (not blockers for M4 closure)

These are explicitly **not** part of the M4 scope:

1. Deprecation surfacing & eventual removal of `RenderFrame` / `StateSnapshot` / raw-core legacy variants. Needs a deprecation diagnostic, migration plan, and double-window of branch support.
2. `ProjectionReplay::requested_resume_from_seq` plumbing in daemon (M4 supplies resume envelope acceptance on the client side; the server-side fan-out is unchanged).
3. Hot-key numeric ack (cmd-N for resync-of-sub-N) UI surface. Deferred to a UX plan.
4. Cross-tab artifact hand-off (tab A reads, tab B opens). Deferred to a UX plan with explicit policy.
5. Plugin emission fall-through for `ProjectionEvent::PluginUi` (M4 only round-trips the envelope; plugin-specific projection semantics remain plugin-authored).

These items are listed as future-plan candidates and registered in the roadmap.

## 6. Verification commands run

```bash
cargo check -p codegg --all-features
cargo check -p codegg-protocol
cargo test -p codegg-protocol                       # 155 passed
cargo test -p codegg-core                           # 299 passed
cargo test -p codegg --lib projection_client::tests # 19 passed
cargo test --test session_projection_m4_controller  # 14 passed
cargo test --test tui_render                        # 99 passed
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles
cargo test --test projection_replay_daemon_protocol
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test session_projection_consumer
cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo fmt -- --check
python3 scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
```

All commands pass. Pre-existing clippy issues in `crates/egglsp/src/edit.rs` (`match -> ?`) and pre-existing failing test `python_script::executor::tests::execute_sets_os_filesystem_isolation` are unrelated to this work package and were broken on the baseline `bac73ce`.

## 7. Repository state at closure

All WP A-F work packages are merged on the local branch. `Cargo.lock` updated; no semver-breaking changes to public APIs in `codegg-protocol` (additions only). The registry and roadmap are updated to reflect closure; future-plan candidates are recorded under M5+ items above.
