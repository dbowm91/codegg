# Desktop Frontend and IDE Foundation Corrective C001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/001-desktop-connection-subscription-lifecycle-and-qualification.md`

Source subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#7-milestones` (M003)
- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`

Repository baseline reviewed: `82e289db90c4c7981524b4291986af39a7f881ea`

Implementation commits or pull requests:

- `7b17808f` — Desktop C001: explicit connection/subscription lifecycle with generation fencing (branch `impl/desktop-connection-subscription-lifecycle`)

## 1. Executive finding

The desktop connection/subscription ownership defect is repaired in production, deterministically tested, and qualified live against a real isolated daemon. Every project-event forwarder now has an explicit host owner with cancel/join semantics, renderer cleanup reaches Rust teardown, reconnect/disconnect cannot retain stale `LocalSocketClient` clones, and successful reconnects bump a renderer-visible connection generation that drives re-subscription.

The milestone is **conditionally closed** rather than strictly closed for one narrow, named reason: the visible-WebView interaction slice of the display-backed trajectory (rendered pixels, window-driven reload/close gestures, Tauri IPC `Channel` bytes in flight) was not exercised. All lifecycle semantics the window would exercise — ownership, fencing, cancellation/join, generation-keyed resubscribe, stale-silence, client-count return to baseline, autostart survival — are proven by the live command-level trajectory (same production code paths, same real daemon) plus renderer race tests. No high/medium product finding remains; the outstanding item is operational evidence only.

A live-qualification run additionally exposed and fixed a real daemon-side defect that would have blocked ANY strict closure: hello-time capability downgrade cancelled the connection-level token, so GUI clients truthfully declaring no session-projection support could handshake but never complete a request. The fix is minimal, protocol-neutral, and regression-tested.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4/§10) | Evidence | Result | Notes |
|---|---|---|---|
| WP1 explicit host lifecycle controller | `apps/desktop/src-tauri/src/lifecycle.rs` (`SubscriptionRegistry`); 8 focused unit tests | pass | No detached task without a stored owner; no mutex guard held across join |
| WP2 identity-scoped subscribe/replace | `subscribe_with_sink` + `install_with_id`; replace/resubscribe/stale-clear tests | pass | Old terminal cleanup cannot clear a newer owner; 50-cycle baseline test |
| WP3 explicit unsubscribe + teardown | `desktop_unsubscribe_events`; `disconnect_host`; disconnect/reconnect tests | pass | Idempotent repeat; stale id never disturbs the current owner; failed reconnect preserves the current connection |
| WP4 renderer-visible connection generation | `ConnectionSnapshot.connectionGeneration`; `bindings:check`; renderer resubscribe test | pass | Connected N → N+1 re-subscribes with state unchanged |
| WP5 React/bridge cleanup races | async `bridge.subscribe` + generation-keyed effect; 5 race tests | pass | Unmount-before-resolve, stale-event ignore, no accumulation over 3 reconnects |
| WP6 lifecycle instrumentation seams | registry snapshots/counts in unit tests; daemon `connected_clients` in live run | pass | No new production diagnostics or public protocol |
| WP7 Tauri security/build boundaries | `scripts/check-desktop-boundary.sh` | pass | Empty permissions, local assets/CSP, no new authority; root MSRV 1.89 intact |
| WP8 existing-daemon trajectory | live test vs isolated real daemon (see §4) | pass (command-level) | Counts 1→2→2/1-desktop→1; stale silent, current reacts |
| WP8 TUI coexistence | TUI-kind observer attached throughout live run | pass | Daemon served both clients; no duplicated state |
| WP8 reconnect with state connected | live `install_connection` gen1→gen2 + poll | pass | Zero stale forwarders, exactly one desktop client |
| WP8 project mutation reaction | workspace+project register via observer; sink2 event, sink1 silent | pass | `project_catalog_changed`, version 1, daemon `event_seq` retained |
| WP8 renderer reload | unsubscribe/resubscribe cycle + React cleanup tests | pass (command + unit) | No owner/task accumulation; window-gesture reload not exercised |
| WP8 close | `disconnect_host` + poll to baseline, daemon responsive | pass (command-level) | Window-close gesture not exercised |
| WP8 explicit autostart | kill + autostart via explicit executable + exit-leaves-running | pass | New pid, desktop exit leaves daemon live |
| WP8 visible WebView | — | **not run** | Sole condition for strict closure (see §10) |
| Daemon hello-downgrade defect (found live) | `cleanup_projection_state` guard + `hello_downgrade_cleanup_preserves_live_connection` | pass (check-compiled; runs in CI) | No protocol change; Gui clients can now complete requests |

## 3. Production implementation evidence

Desktop host (`apps/desktop/src-tauri/src/`):

- `lifecycle.rs` (new): `SubscriptionRegistry` owns at most one forwarder (`JoinHandle`) with `allocate_subscription_id`, `install`/`install_with_id` (atomic swap, abort+join previous outside the lock), identity-scoped `unsubscribe` (`Removed`/`AlreadyEnded`/`Stale`), `clear_if_matching` (synchronous compare-by-id for exiting tasks), and `shutdown`. Cancellation is abort-plus-join, which drops the task's `LocalSocketClient` clone with deterministic terminal observation; no `tokio_util` dependency was added.
- `lib.rs`: `HostState` now carries `connection_generation` (renderer-visible, monotonic, zero until first install), a separate `connect_serial` attempt fence, and the registry. `install_connection` cancels the stale subscription BEFORE committing the new client; failures never bump the visible generation. `subscribe_with_sink` is the full production subscribe path behind a `DesktopEventSink` trait so the live trajectory exercises the same ownership/fencing/cleanup as the Tauri command; `desktop_subscribe_events` is a thin `ChannelSink` adapter. New `desktop_unsubscribe_events` maps all outcomes to success (removed, idempotent repeat, stale ignore).
- `bridge.rs`: `ConnectionSnapshot` gains `connection_generation`; new `SubscriptionInfo { subscription_id, connection_generation }` (camelCase DTOs).
- `Cargo.toml`/`Cargo.lock`: `tokio` gains `rt` (production need: `tokio::spawn` for forwarders) and `macros` (test need: `#[tokio::test]`); lockfile +12 lines. Desktop Rust remains 1.90, isolated workspace.
- `gen/schemas/macOS-schema.json`: committed Tauri-generated schema matching the tracked per-platform siblings; no config change behind it.

Renderer (`apps/desktop/src/`):

- `bridge.ts`: `subscribe` is now `async` and returns `{ subscriptionId, connectionGeneration, unsubscribe }`; `unsubscribe` invokes `desktop_unsubscribe_events` (fire-and-forget, error-swallowed); new `unsubscribe(id)` helper.
- `App.tsx`: subscription effect depends on `[connection.state, connection.connectionGeneration]`; effect-local `cancelled` flag plus a `connectionRef` mirror handles mount/unmount races, in-flight subscribe at reconnect, and stale-generation events/refreshes. `connectionRef` is synced synchronously in connect/reconnect handlers so immediate-mock resolutions cannot drop valid refreshes.
- `bridge-types.ts` + `scripts/check-bindings.mjs`: contract extended for the two new fields; guard passes.

Daemon (`src/core/transport/daemon_socket.rs`):

- `cleanup_projection_state` now fires the connection-level cancellation only when it actually removed ≥1 subscription. Previously it fired unconditionally, including on the hello-time capability-downgrade path for fresh connections with zero subscriptions — permanently breaking `bounded_critical_delivery` for that connection. Net effect of the old code: any client declaring `session_projection: false` (the desktop GUI before M004) could complete the handshake but every request failed (`Cancelled` → connection torn down). Each removed subscription is still individually cancelled/aborted/joined as before; teardown already cancels through its own token, so that path is unchanged. No wire, DTO, version, or storage change.

Docs:

- `architecture/desktop.md`: connection/subscription ownership and generation model recorded.
- This record; registry/roadmap reconciliation (see §12).

## 4. Verification executed

All runs below are local on macOS arm64 (MacBook Pro, Darwin 25.6.0). The daemon binary for live runs was built from this worktree with the pinned 1.89 aarch64 toolchain into a fresh target dir (`/tmp/codegg-target-fresh/debug/codegg`, arm64, `codegg 0.1.0`).

### Commands run

```bash
$HOME/.cargo/bin/cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
$HOME/.cargo/bin/cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
CODEGG_DAEMON_EXECUTABLE=/tmp/codegg-target-fresh/debug/codegg \
  $HOME/.cargo/bin/cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml \
    -- --ignored live_daemon_lifecycle_against_real_daemon --nocapture
$HOME/.cargo/bin/cargo test -p codegg-client --lib --locked
export PATH="$HOME/.cargo/bin:$PATH" && bash scripts/verify.sh quick
git diff --check
bash scripts/check-desktop-boundary.sh
npm run typecheck && npm test && npm run bindings:check && npm run build   # from apps/desktop
```

### Results

- Desktop host tests: **15 passed, 0 failed, 1 ignored** (`live_daemon_lifecycle_against_real_daemon`, run explicitly). Includes subscribe/unsubscribe release, no-double-owner replace, stale-unsubscribe safety, disconnect teardown, generation monotonicity, terminal-cleanup fencing, channel-failure clear, and 50-cycle baseline return.
- Live command-level trajectory (3 consecutive green runs, ~2.1s each, isolated `CODEGG_DAEMON_HOME=/tmp/cgdt-<pid>`, TUI-kind observer + GUI-kind desktop, workspace `/tmp/cgws-<pid>`, project `live-lifecycle-probe`): baseline 1 client → desktop connect 2 clients/1 desktop → gen1 subscribe (1 owner) → connected→connected reconnect gen2 (0 owners, still 2 clients/1 desktop) → gen2 resubscribe → register/archive path emits `project_catalog_changed` to the current sink only (stale sink 0 events) → reload cycle (1→0→1 owners, counts stable) → disconnect (1 client, daemon responsive) → SIGTERM + explicit-executable autostart (new pid) → desktop exit leaves daemon running (observer-only count 1).
- Renderer tests: **7 passed** (bounded catalog, connect failure, generation-change resubscribe with `connected` unchanged, unmount unsubscribe, unmount-before-resolve late cleanup, stale-generation event ignore, 3-reconnect no-accumulation). `tsc --noEmit`, `bindings:check`, and `vite build` all pass.
- `scripts/verify.sh quick`: **passed** (fmt, agent schema, core-boundary, sandbox, execution-ownership, tui-authority, http-route-disposition, audit-coverage, scheduler-bypass guards, workspace check). Covers the daemon fix including the new unit test's compilation.
- `scripts/check-desktop-boundary.sh`: **passed**. `git diff --check`: clean.
- `codegg-client --lib`: **3 passed**.
- New daemon unit test `hello_downgrade_cleanup_preserves_live_connection`: check-compiled in this tree; executes in CI (root test binaries cannot link in this worktree — see §10).
- End-to-end regression proof for the daemon fix: before the fix, the same live trajectory failed at observer autostart (`StartupTimeout`) and a minimal probe showed hello-OK but snapshot-`PeerClosed` with daemon-side `response delivery failed: cancelled`; after the fix, the full trajectory passes.

## 5. Invariant review

- One active project-event subscription per renderer/connection generation: enforced by atomic registry swap; proven by replace/reconnect/reload tests plus live counts.
- Every forwarder has cancellation and terminal/join ownership: abort+join on replace/unsubscribe/shutdown; self-exit path clears by id only.
- No stale task retains a superseded client clone: live reconnect shows daemon-side desktop count stays exactly 1 with the new client.
- Renderer cleanup causes Rust cleanup: effect cleanup calls unsubscribe (unit-proven, including late-resolve); Rust joins the task.
- Subscription ids and connection generations are distinct and both checked: forwarder fences on `(id, generation)` before every send; snapshot carries generation.
- Stale unsubscribe cannot cancel a current subscription: `Stale` outcome leaves the owner untouched (tested).
- Stale event delivery cannot mutate renderer state: generation check in handler + refresh fencing (tested).
- Reconnect installs a new visible generation even when state stays `connected`: `install_connection` bumps on success only; renderer resubscribes on the change (tested live + unit).
- Desktop shutdown never stops daemon-owned work: disconnect drops client state only; autostart section proves the daemon outlives the desktop (tested live).
- Least-privilege Tauri posture unchanged: boundary guard green; no new permissions/origins/commands beyond the narrow unsubscribe.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: repeat unsubscribe for the terminated id succeeds without work; superseded ids report stale (both success, never disturbing the owner).
- Cancellation races: mount/unmount, in-flight subscribe across reconnect, and task-exit-vs-replace are all covered (registry compare-by-id + effect cancelled flags + late-unsubscribe).
- Daemon restart: autostart path re-proven live (kill → explicit-executable restart → new pid → desktop exit leaves it running).
- Stale generation/lease: generation mismatch breaks forwarders before send; failed reconnects never fabricate generations (serial-fenced, install-only-on-success).
- Contention/resource release: 50-cycle test plus live reload/close cycles return owners and daemon client counts to baseline.
- Malformed/unauthorized input: bridge surface unchanged in authority (bounded DTOs only); daemon fix changes no auth path.
- Test-hygiene finding (own goal, fixed): piping test output through `head` SIGPIPE-orphaned test binaries whose delayed autostart kills hit recycled pids. Fixed by removing truncating pipes, adding daemon-identity checks before every SIGTERM, and tolerating transport errors in death-wait loops. No recurrence across the final green runs.

## 7. Migration and compatibility review

No storage migration. No CoreFrame version change (protocol v2 both ends). No project/session/projection DTO change. Bridge DTO change is additive (`connectionGeneration`, `SubscriptionInfo`); `bindings:check` pins the mapping. Old renderers cannot talk to the new host (expected — bundled together), and no durable state carries the old shape. Rollback is a clean redeploy of the prior bundle.

## 8. Security review

The renderer capability remains `main`-window-only with an empty permission list; CSP unchanged (local assets/IPC only, no remote origins, no `unsafe-eval`); no generic `core_request`/filesystem/shell/process/network authority added — mechanically guarded by `scripts/check-desktop-boundary.sh` (green). The new unsubscribe command carries only an opaque subscription id and is scoped to the calling window's host state. The daemon fix touches cancellation scope only; authorization, secret handling, and audit paths are untouched. No secrets logged; isolated test homes under `/tmp` were removed by test cleanup.

## 9. Documentation and operations

- `architecture/desktop.md` updated with the ownership/generation model.
- Predecessor `plans/closure/desktop-frontend-ide-foundation/003-status.md` left immutable; reconciliation is additive here and in the roadmap/registry.
- Operator note: desktop `cargo test` requires the staged daemon resource (`npm run stage:daemon` with a built `codegg`; the `binaries/` dir is gitignored). The live trajectory runs explicitly with `CODEGG_DAEMON_EXECUTABLE` (see §4) and never touches the real user daemon home.
- Environment note: this worktree's `target/` contains stale x86_64 artifacts from a Homebrew toolchain; root link-based suites were built/run via an explicit 1.89-aarch64 `RUSTC` plus a fresh `CARGO_TARGET_DIR`. `codegg-client` integration tests fail in this worktree on Unix-socket `SUN_LEN` (deep worktree path) — pre-existing and unrelated (client crate untouched; `--lib` green).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Visible-WebView display evidence outstanding: rendered pixels, window-driven reload/close gestures, and Tauri IPC `Channel` bytes were not exercised; all lifecycle semantics behind them are proven live at command level plus renderer unit tests | Blocks strict M003 closure and M004 unblock; no product defect indicated | Run the app on a display-backed macOS/Linux host: reuse existing daemon, autostart via explicit executable, TUI coexistence, connected→connected reconnect, project mutation with renderer observation, window reload, window close with count return to baseline, autostart survival; record OS/arch/build/commands |
| low | Root link-based suites (`cargo test -p codegg --test single_daemon_lifecycle`, full workspace tests) not runnable in this worktree (stale x86_64 `target/`; `SUN_LEN` in deep path) | CI owns the missing runs; `verify.sh quick` (check) is green here | CI `verify` (full) must stay green; the new daemon unit test executes there |

## 11. Roadmap disposition

- C001 is **conditionally closed** by this record: production complete, deterministic + live evidence green, one named operational evidence item outstanding (§10, medium).
- Predecessor M003 stays **conditionally closed**; strict closure awaits the visible-window run above (no new code expected).
- M004 stays **blocked** on strict M003 closure. M005/M006 stay deferred. No other plan is gated on C001.
- No second corrective is registered: the daemon hello-downgrade defect found live was on C001's critical path (no strict closure possible without it), fixed with a 3-line protocol-neutral change plus regression tests, and documented here as the reason original (build-only) verification missed it.

## 12. Registry updates

- Dependency-ready table: C001 `active` → moved to Recently-closed as **conditionally closed** (this record; implementation `7b17808f`).
- Blocked work: C001 row updated to landed-implementation + remaining visible-window evidence; M004 row remains hard-blocked on strict M003 (now via conditionally-closed C001).
- Active subsystem roadmaps: desktop M003-corrective current milestone `C001 ready` → `C001 conditionally closed`; desktop-foundation row notes C001 conditionally closed with M004 still blocked.
- Desktop gate paragraph: C001 strict-closure control point updated to conditional closure with the §10 evidence item.
- Corrective addendum §11: C001 `ready` → `conditionally closed` (this record).
- Foundation roadmap M003 status: appended C001 conditional closure + live/defect evidence summary.
- Implementation plan status: `active` → `implemented`.
- Unblock audit (same commit, per process): the only registered plan depending on this line is M004 (via strict M003); it remains blocked — nothing else unblocked, nothing silently unblocked.
