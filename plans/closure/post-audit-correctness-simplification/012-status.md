# Post-Audit Correctness, Simplification, and Footprint C003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/post-audit-correctness-simplification/012-daemon-startup-shutdown-and-process-lifecycle-corrective-pass.md`

Source subsystem roadmap: `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md`

Repository baseline reviewed: `2249c0bdd58481316c53207ee5297a3319c0a34f`

Implementation commits:

- `0bb7d5b` — fix daemon startup shutdown and process lifecycle
- `49488e0` — mark daemon lifecycle implementation ready for closure

## 1. Executive finding

C003 is strictly closed. The ordinary daemon-client entrypoint now starts or
reuses one verified user-scoped daemon, keeps an autostarted daemon alive after
the initiating frontend returns, converges concurrent starters on the lock
winner, and shuts down through the same bounded cleanup path for SIGINT,
SIGTERM, and `daemon stop`. Production daemon storage uses the migrated
user-scoped catalog. No critical or high C003 finding remains unresolved.

## 2. Requirement-to-evidence matrix

| Required outcome | Evidence |
|---|---|
| Plain `codegg` reaches the TUI startup boundary from fresh state | `plain_entrypoint_reaches_tui_startup_boundary_without_prestarted_daemon` starts the real binary with private runtime/data roots and observes the deterministic event-loop seam. |
| Autostart lifetime, detachment, and reaping | `connect_or_start_keeps_autostarted_daemon_alive_after_return`; `src/core/instance.rs` removes daemon `kill_on_drop`, starts a new Unix session, and moves the child to a non-killing reaper after readiness. |
| Concurrent startup converges | `concurrent_connect_or_start_calls_converge_on_one_daemon` starts two real helpers against an empty root and asserts both succeed with one daemon identity. |
| Failed helper ownership is bounded | Startup has one deadline; timeout kills and reaps only the helper-owned child, while child-exit handling performs a final verified winner probe before failing. |
| Fresh migrated production catalog | `init_migrated_daemon_catalog` performs the existing single-connection migration then reopens `init_daemon_catalog`; `run_daemon` and stdio bootstrap use it, and lifecycle tests isolate `CODEGG_DATA_HOME`. |
| Legacy project store remains compatibility-only | Production `run_daemon` no longer calls `init_legacy_project_store`; no legacy project database is created as daemon authority by the fresh-root tests. |
| Handshake-gated readiness | `SocketCoreClient::connect` sends ClientHello and waits for validated ServerHello; readiness then requires a bounded `SnapshotDaemon` probe. |
| Startup cannot hang on a dead/non-responsive peer | `verified_connect_until` wraps the handshake and control-plane probe in the remaining startup deadline; connect-or-start polls only until that deadline. |
| Peer death releases pending requests | `core::transport::socket::tests::peer_death_releases_pending_request_with_error` closes a real Unix peer after handshake and verifies the request resolves as an error. Reader EOF/error drains all pending senders. |
| Reconnect is safe | Reconnect resets negotiated identities and performs a fresh handshake; request retry was removed so replacement connections cannot silently reuse stale state. |
| Endpoint precedence and normalization | `core::instance::tests` includes endpoint normalization/path authority coverage; the shared resolver applies CLI endpoint, then `CODEGG_CORE_ENDPOINT`, then the canonical default to TUI, daemon commands, and connect-or-start. |
| SIGINT/SIGTERM graceful path | `run_daemon` listens for both supported Unix signals and cancels the shared shutdown token; `stop_signals_the_current_daemon_after_identity_match` passes through the real SIGTERM path. |
| Bounded connection drain | `run_core_socket_with_listener_with_timeout` applies `shutdown_timeout_ms`, aborts remaining connection tasks after expiry, and the 33-test daemon-socket suite passes its active-writer/shutdown cleanup coverage. |
| Owned artifact cleanup and restart | `stop_signals_the_current_daemon_after_identity_match` verifies socket, PID, metadata cleanup and immediate restart; stop waits for all three artifacts without force-killing an unverified PID. |
| Coherent diagnostics | Autostart redirects stdout/stderr to the user-only canonical daemon log; plain startup asserts that log path exists, and timeout/child-exit errors include it. `daemon logs` resolves the same path. |
| Noisy MCP stderr cannot deadlock a child | `mcp::local::tests::noisy_stderr_does_not_block_initialize` writes beyond normal pipe capacity while serving initialize; the continuously draining task is bounded-memory and joined at shutdown. |

All acceptance criteria in the implementation plan are covered by the
mechanism evidence above or by the focused verification below. No new service
manager, binary, CI lane, scheduler authority, or protocol message was added.

## 3. Production implementation evidence

- `src/core/instance.rs` now owns endpoint normalization, startup deadlines,
  verified readiness, detached autostart, race convergence, log redirection,
  child cleanup, and reaping.
- `src/core/transport/socket.rs` now has explicit handshake state, protocol
  validation, peer-death propagation, pending-request cleanup, and fresh
  reconnect semantics.
- `src/core/transport/daemon_socket.rs` now bounds connection draining and
  aborts only the connection tasks remaining after the configured grace
  interval.
- `src/main.rs` uses the shared paths, migrated daemon catalog, SIGINT/SIGTERM
  cancellation, canonical log path, endpoint-aware TUI launch, and bounded
  stop completion wait.
- `src/mcp/local.rs` continuously drains local-child stderr without retaining
  unbounded output.
- The lifecycle integration suite isolates both runtime and data roots and
  exercises the real binary rather than an in-process substitute.

## 4. Verification executed

All results below are local results; no hosted CI result is being implied.

- `rtk proxy cargo test --test single_daemon_lifecycle -- --nocapture --test-threads=1` — 8 passed.
- `rtk proxy cargo test -p codegg --lib core::transport::socket::tests::peer_death_releases_pending_request_with_error -- --nocapture` — 1 passed.
- `rtk proxy cargo test -p codegg mcp::local::tests::noisy_stderr_does_not_block_initialize -- --nocapture` — 1 passed.
- `rtk proxy cargo test -p codegg core::instance::tests -- --nocapture` — 10 passed.
- `rtk proxy cargo test -p codegg-core storage::tests -- --nocapture` — 9 passed.
- `rtk proxy cargo test -p codegg core::transport::daemon_socket -- --nocapture` — 33 passed.
- `rtk cargo check -p codegg` — passed.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk proxy scripts/verify.sh quick` — passed.
- Static guards passed: daemon CWD usage, scheduler bypass, identity-path usage,
  and WebSocket bounds.
- `rtk git diff --check` — passed before the implementation and closure commits.

The full workspace/all-features test matrix was not run; the focused runtime
coverage and repository quick verification were the change-proportional checks
for this lifecycle pass.

## 5. Invariant review

The singleton lock remains authoritative, metadata remains diagnostic, plain
`codegg` remains daemon-client mode, and no second production persistence
authority was introduced. Daemon lifetime is independent of the initiating
frontend. Peer death, startup, shutdown, endpoint selection, and cleanup all
have finite behavior. Existing standalone/stdio compatibility modes, scheduler
ownership, protocol version, and single-binary topology remain unchanged.

## 6. Failure and recovery review

Startup failure paths include bounded handshake/probe deadlines, actionable log
paths, helper-child kill/reap, and concurrent-winner convergence. Reader EOF or
I/O failure resolves every pending request. Reconnect is explicit and
fresh-handshake based. SIGTERM and SIGINT share the owned cancellation path;
connection drain is bounded and cleanup is performed after the drain attempt.
`daemon stop` retains live identity verification and never force-kills an
unverified PID.

## 7. Migration and compatibility review

No schema or wire migration was added. The existing session schema migration
is run before the normal daemon catalog pool is opened. Project-local legacy
stores remain available to compatibility/import paths. `--standalone`,
`--stdio`, daemon subcommands, endpoint environment overrides, and explicit
log-file selection remain supported.

## 8. Security review

The singleton lock and generation/identity checks remain in force. Stop cannot
signal a process without matching live protocol identity and metadata. The
daemon log is opened with user-only permissions. Endpoint overrides retain the
user-scoped lock and metadata paths rather than creating an untracked authority.
No secret-handling, authorization, sandbox, or network trust boundary was
expanded.

## 9. Documentation and operations

Updated `architecture/core.md`, `architecture/storage.md`, `architecture/client.md`,
`architecture/tui.md`, and `docs/TROUBLESHOOTING.md` to describe daemon
survival, migrated catalog authority, endpoint precedence, bounded startup and
shutdown, graceful Unix signals, and the canonical daemon log path.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Critical: none.
- High: none.
- Medium: none within C003.
- Low/deferred: `StdioCoreClient` remains an explicit compatibility/testing
  transport with the adjacent ownership/request-gating concerns documented in
  the C003 addendum. No directly reproduced production-daemon defect required
  widening this pass, so no separate follow-up was registered.

## 11. Roadmap disposition

The C003 daemon-lifecycle corrective addendum is now closed. The parent
post-audit roadmap remains closed, with C001/C002 preserved as historical
records and not rewritten. The implementation plan is marked `implemented` and
this closure record is the authority for its accepted completion.

## 12. Registry updates

The registry moves C003 from closure review to recently closed and removes it
from dependency-ready work. The blocked-work audit found only Development
Verification and Release M006, which remains blocked on Provider M007 and Tool
Programs M019. No registered plan lists C003 as a hard or interface dependency,
so no future plan was unblocked or status-changed. Tool Programs M019 remains
ready independently; the existing Provider M007 condition and M006 blocker are
unchanged. No new corrective follow-up was required.
