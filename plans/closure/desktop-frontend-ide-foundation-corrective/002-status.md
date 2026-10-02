# Desktop Frontend and IDE Foundation Corrective C002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/002-atomic-lifecycle-transitions-subscription-arming-window-teardown.md`

Source subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` (M003)

Repository baseline reviewed: `dc32a6ad`

Implementation commits:

- `dc32a6ad` — Desktop C002: atomic lifecycle transitions, subscription arming, window teardown (branch `impl/desktop-c002-provider-recon-c001`)

## 1. Executive finding

The four post-C001 host linearization races plus the missing native window/app
teardown hook are repaired in production, deterministically tested with
barrier-forced interleavings, and qualified live against a real isolated
daemon with an adversarial prepared-connect trajectory. Every lifecycle
transition (connect commit, disconnect, subscription install, native close/app
exit) is now mutually ordered by one lifecycle serialization gate; disconnect
and native close supersede every earlier in-flight connect via the
connect-attempt epoch; forwarders spawn unarmed and are armed only after owner
publication; and window/app teardown reaches the same Rust-host cleanup as
explicit disconnect without stopping the daemon.

The milestone is **conditionally closed** rather than strictly closed for one
named operational reason: hosted `CI / verify` has not run on the
implementation commit yet (local `scripts/verify.sh quick` is green; the
desktop, renderer, client, and daemon-lifecycle suites are green locally).
No high/medium product finding remains; the outstanding item is hosted
confirmation only. The visible-window interaction run remains the final
M003-level gate after C002 and is intentionally not claimed here.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| WP1 lifecycle serialization boundary, linearization points, no I/O under gate (§5, §6) | `HostState.lifecycle: Mutex<()>`; gate holds only serial check + take + install + generation bump; slow connect/autostart/snapshot outside; forwarder abort/join after release where applicable; `architecture/desktop.md` § Lifecycle linearization documents rules 1–8 | pass | Lock order gate → registry → client; forwarders never acquire the gate |
| WP2 atomic connect commit + disconnect supersession (§5) | `allocate_connect_attempt` + `commit_prepared_connection` (check + take + install + bump under one gate); `disconnect_host` advances `connect_serial` under the same gate; `disconnect_supersedes_inflight_connect`, `newer_connect_attempt_prevents_older_commit`, `failed_newer_connect_does_not_allow_stale_commit` | pass | Stale attempts drop the prepared client; failed newer never fabricates a generation; prior connection preserved |
| WP3 arming barrier (§5) | oneshot arm gate: spawn UNARMED → publish via `replace_without_join` under gate → release → `arm_tx.send`; `subscription_is_armed_only_after_owner_publication`, `event_before_install_does_not_leave_finished_owner` | pass | Queued pre-install events cannot self-terminate; no dead handle installed |
| WP4 subscribe/transition mutual ordering (§5) | snapshot + spawn + publish under the gate; `subscribe_racing_disconnect_is_linearizable` (both orderings), `subscribe_racing_reconnect_binds_one_generation_only` (binds G+1 or torn down, one generation only) | pass | No stale owner waits for a future event; host never relies on renderer repair |
| WP5 native window/app teardown (§5) | `handle_native_close`/`handle_app_exit` (same authority as disconnect, daemon never stopped, idempotent); `run()` wires `on_window_event` (`CloseRequested`/`Destroyed`, bounded 2s `block_on`) + `RunEvent::ExitRequested`/`Exit`; `native_close_tears_down_subscription_and_connection` (both orderings + idempotent repeats), `native_close_supersedes_inflight_connect`, `native_window_teardown_is_wired_in_run` | pass | macOS window-close-with-live-app path covered by idempotent host handler; visible-window gesture remains M003-level |
| WP6 deterministic barrier tests, 10 interleavings (§5) | `TestBarrier` (entered/release, no sleeps for correctness); 10 required names plus wiring check; registry `old_terminal_cleanup_cannot_clear_newer_owner` covers old-exit-during-install; `mixed_lifecycle_stress_returns_to_baseline` (100 mixed cycles) | pass | Final-state assertions, not panic-only |
| WP7 adversarial live-daemon trajectory (§5) | ignored `live_daemon_lifecycle_against_real_daemon` extended: TUI baseline 1/0 → connect 2/1 → gen1 subscribe → reconnect gen2 (0 owners, 1 desktop) → resubscribe gen2 → invalidation to current only → reload cycle → `handle_native_close` to 1/0 (idempotent repeat) → prepared-connect superseded without resurrection (counts back to 1/0, generation unchanged) → autostart survival; daemon responsive throughout | pass | 1 passed explicitly; no WebView required for the concurrency proof |
| WP8 renderer + security boundaries (§5) | 8 renderer tests (7 preserved + new host-rejection phantom-handle regression), `typecheck`, `bindings:check`, `vite build`; `scripts/check-desktop-boundary.sh` green; no new Tauri permission | pass | Host rejection on concurrent disconnect yields no phantom handle; generation-mismatch stale cleanup retained |
| Completion 1–7: mutual ordering, supersession, no older commit, armed-only, no stale owner, zero owners after close, one connection after reconnect (§13) | Unit tests above + live counts (2/1 → 1/0, exactly one desktop client, zero old-generation owners) | pass | — |
| Completion 8–10: deterministic tests, live evidence, renderer/guards green (§13) | `cargo test` desktop lib 26 passed + live 1 passed; renderer 8 passed; `codegg-client --lib` 3 passed; `single_daemon_lifecycle` 8 passed; `verify.sh quick` green; boundary guard green; `git diff --check` clean | pass | Hosted CI is the sole outstanding gate (§10) |
| Completion 11: hosted CI green (§13) | — | **not run** | Sole condition for strict closure (see §10) |
| Completion 12: no unresolved high/medium finding (§13) | Review below (§5, §10) | pass | Only the named operational conditions remain |

## 3. Production implementation evidence

Desktop host (`apps/desktop/src-tauri/src/`):

- `lib.rs`: `HostState` gains `lifecycle: Mutex<()>` with documented
  linearization points; `allocate_connect_attempt`/`is_current_attempt`
  fencing; `commit_prepared_connection` (slow prep outside, check + take +
  install + bump under one gate); `disconnect_host` (serial invalidation +
  generation bump + `take_active` + client clear under the gate, join after
  release); `handle_native_close`/`handle_app_exit` (idempotent, daemon never
  stopped); `subscribe_with_sink_and_barriers` (pre-gate + pre-install test
  barriers, spawn UNARMED via oneshot, snapshot + publish via
  `replace_without_join` under the gate, ARM after release, join previous
  outside); `desktop_connect` fast/slow paths both commit atomically;
  `run()` wires `on_window_event` (`CloseRequested`/`Destroyed`, bounded 2s
  `block_on`) and `RunEvent::ExitRequested`/`Exit` to the host teardown.
- `lifecycle.rs`: `take_active` (take without join for gate-friendly
  teardown), `replace_without_join` (swap returning the previous task for
  post-gate join), `cancel_and_join` made `pub(crate)`. Terminal cleanup stays
  compare-by-id and generation-safe.
- Test-only (cfg(test)) mirrors sharing the gate/serial/generation/registry:
  `fake_connected` flag, `TestBarrier`, `test_commit_generation` (takes active
  subscription like production), `test_subscribe_with_fake_stream` (identical
  spawn-unarmed → gate → verify → publish → release → arm ordering with fake
  receiver).

Renderer (`apps/desktop/src/`):

- No architecture change. `App.tsx` generation-keyed effect and
  stale-generation fencing already handle host rejections; `bridge.subscribe`
  creates the handle only after a successful invoke, so a host rejection
  cannot leave a phantom handle. New regression test
  `host rejection from concurrent disconnect leaves no phantom handle` pins
  this timing contract (8/8 renderer tests green).

Docs:

- `architecture/desktop.md`: lifecycle linearization boundary, epoch rules 1–8,
  split slow-prep/atomic-commit, arm-before-run ordering, native teardown
  protocol recorded.
- This record; roadmap/addendum/registry reconciliation (see §12).

## 4. Verification executed

Local on macOS arm64 (Darwin 25.6.0). Desktop toolchain 1.90.0 per
`apps/desktop/src-tauri/rust-toolchain.toml`; root toolchain 1.89.

```bash
rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib
# 26 passed, 0 failed, 1 ignored (live, run explicitly below)
CODEGG_DAEMON_EXECUTABLE=/Users/davidbowman/projects/codegg/target/debug/codegg \
  rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml \
    --locked -- --ignored live_daemon_lifecycle_against_real_daemon --nocapture
# 1 passed (~6.1s, isolated CODEGG_DAEMON_HOME, TUI observer + GUI desktop):
# baseline 1/0 → connect 2/1 → gen1 subscribe → reconnect gen2 (0 owners,
# 1 desktop) → resubscribe gen2 → project invalidation to current only →
# reload cycle → handle_native_close to 1/0 (idempotent repeat) →
# prepared-connect superseded without resurrection (1/0, generation unchanged)
# → autostart survival with desktop exit leaving the daemon live
cargo test -p codegg-client --lib --locked
# 3 passed
cargo test -p codegg --test single_daemon_lifecycle --locked
# 8 passed
bash scripts/verify.sh quick
# passed (fmt, agent schema, core-boundary, sandbox, execution-ownership,
# tui-authority, http-route-disposition, audit-coverage, scheduler-bypass,
# provider guards, workspace check)
bash scripts/check-desktop-boundary.sh
# passed (empty permissions, local assets/CSP, no new authority; root MSRV intact)
git diff --check
# clean
npm ci && npm run typecheck && npm test && npm run bindings:check && npm run build
# typecheck clean; 8/8 renderer tests; bindings match; vite build ok
rustup run 1.90.0 cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo fmt --all -- --check
# clean
```

Daemon binary for the live run was the worktree `target/debug/codegg`
(C002 touches only the desktop crate, so no daemon rebuild was required;
the trajectory asserts daemon responsiveness throughout and autostart
survival at the end).

## 5. Invariant review

- Linearizable host epoch `(generation, connection?, daemon id?, active
  subscription?, connect serial)`: every completed operation leaves a coherent
  tuple; proven by both-orderings race tests plus 100-cycle stress returning
  to baseline.
- No active subscription names a generation different from the installed
  connection: reconnect commits take the old owner; subscribe publishes only
  for the current generation under the gate.
- Disconnect authority: completed disconnect/native close supersedes every
  earlier in-flight connect (serial invalidated under the gate; verified live
  with a real prepared socket).
- Connect commit authority: only the latest still-valid attempt commits;
  check and commit share the gate; slow work stays outside.
- Subscription publication: unarmed spawn → publish → arm; pre-install events
  cannot terminate the task before installation.
- Terminal cleanup compare-by-id/generation-safe, never clears a newer owner
  (registry test retained + host arming tests).
- Window/app teardown reaches `desktop_disconnect` semantics, idempotent,
  daemon never stopped.
- Security/toolchain: no new renderer permission, protocol version, storage
  migration, workspace dependency, or MSRV change (boundary guard green).

## 6. Failure and recovery review

- Duplicate/ilate teardown: repeat unsubscribe/close/exit are idempotent
  success paths; stale ids never disturb the current owner.
- Cancellation races: mount/unmount, in-flight subscribe across reconnect,
  task-exit-vs-replace, close-vs-subscribe, close-vs-commit all forced via
  barriers and asserted on final state.
- Daemon restart: autostart path re-proven live (kill → explicit-executable
  restart → new pid → desktop exit leaves it running).
- Stale generation: mismatch breaks forwarders before send; failed reconnects
  never fabricate generations.
- Contention: 100-cycle mixed stress (commit/subscribe/unsubscribe/disconnect/
  native-close/reconnect-commit) returns owners and connection flags to
  baseline with monotonic generations.
- Malformed/unauthorized input: bridge surface unchanged in authority
  (bounded DTOs only); Tauri callbacks carry no payload.

## 7. Migration and compatibility review

No storage migration. No CoreFrame/protocol version change. No daemon
authorization change. Bridge DTOs unchanged (`ConnectionSnapshot`,
`SubscriptionInfo` shapes preserved; `bindings:check` green). Old renderers
cannot talk to the new host (expected — bundled together); no durable state
carries the changed internals. Rollback is a clean redeploy of the prior
bundle plus desktop crate revert.

## 8. Security review

Renderer capability remains `main`-window-only with an empty permission
list; CSP unchanged; no generic `core_request`/filesystem/shell/process/
network authority added — mechanically guarded (green). Native teardown adds
no Tauri plugin permission (window/app lifecycle events only). New
unsubscribe-adjacent paths carry only opaque subscription ids. No secrets
logged; isolated test homes removed by test cleanup.

## 9. Documentation and operations

- `architecture/desktop.md` updated with the gate, epoch, arming, and native
  teardown contract.
- Predecessor closures (`003-status.md`, C001 `001-status.md`) left
  immutable; reconciliation is additive here and in the roadmap/addendum/
  registry.
- Operator note: desktop `cargo test` requires the staged daemon resource
  (`npm run stage:daemon`; `binaries/` is gitignored). The live trajectory
  runs explicitly with `CODEGG_DAEMON_EXECUTABLE` and never touches the real
  user daemon home.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Hosted `CI / verify` has not run on `dc32a6ad` | Blocks strict C002 closure; no product defect indicated (local quick + all focused suites green) | Hosted `CI / verify` green on the merge commit; record the run ID in a follow-up note if required |
| medium | Visible-window display evidence outstanding (M003-level, not C002 scope) | Blocks strict M003 closure and M004 unblock; C002 concurrency proof does not require a WebView | After C002, run the app on a display-backed macOS/Linux host per C001 §10: reuse existing daemon, explicit-path autostart, TUI coexistence, connected→connected reconnect, project mutation with renderer observation, window reload, window close with count return to baseline, autostart survival; then promote M003 to strict closed and register M004 ready |

## 11. Roadmap disposition

- C002 is **conditionally closed** by this record (production complete,
  deterministic + live evidence green, hosted CI the sole strict-closure
  condition).
- Predecessor M003 stays **conditionally closed**; strict closure awaits the
  visible-window run above (no new code expected for the races closed here).
- M004 stays **blocked** on strict M003 closure. M005/M006 stay deferred.
- No second corrective is registered: all five findings (A–E) are closed
  within C002's scope with regression tests pinning each interleaving.

## 12. Registry updates

- Dependency-ready table: C002 `ready` → moved to Recently-closed as
  **conditionally closed** (this record; implementation `dc32a6ad`).
- Blocked work: M004 row remains hard-blocked on strict M003 (now via
  conditionally-closed C002 + visible-window evidence); C001 strict-closure
  row unchanged (visible-window evidence still the item).
- Active subsystem roadmaps: desktop M003-corrective current milestone
  `C002 ready` → `C002 conditionally closed`; desktop-foundation row notes
  C002 conditionally closed with M004 still blocked.
- Desktop gate paragraph: C002 readiness control point updated to conditional
  closure with the §10 hosted-CI condition.
- Corrective addendum §11: C002 `ready` → `conditionally closed` (this record).
- Foundation roadmap M003 status: appended C002 conditional closure evidence.
- Implementation plan status: `ready for handoff` → `implemented`.
- Unblock audit (same commit, per process): the only registered plan gated on
  this line is M004 (via strict M003, which additionally requires the
  visible-window run); it remains blocked — nothing else unblocked, nothing
  silently unblocked.
