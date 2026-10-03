# Desktop Frontend and IDE Foundation M004 — Closure Status

Status: closed (local `verify.sh quick` GREEN; built-app E2E trajectory
GREEN — lifecycle 5/5 + autostart 2/2 + session 7/7 including the
deterministic live turn; hosted CI disposition recorded in §10)

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/004-desktop-session-control-plane-vertical-slice.md`

Source subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` (M004)
- `plans/registry.md` (M004 ready → closed, §12)

Repository baseline reviewed: `f4c65b9e` (M004 WP G)

Implementation commits (prior session, landed):

- `cab9c7fd` — M004 WP A+D: loss-aware projection driver + TurnSubmit composer in codegg-client
- `d0b36c0a` — M004 WP B: desktop route/session controller with generation fencing
- `ea1b16ec` — M004 WP C: projection presentation + owner lifecycle + renderer session view
- `59bf2445` — M004 WP D: daemon-resolved prompt submit + desktop at-most-once host
- `a7213dfa` — M004 WP E: controller/permission/question interaction through daemon leases
- `fcadbba9` — M004 WP C remainder: artifact excerpts + bounds/coalescing proof
- `56555d1c` — M004 WP F: coexistence, detach-intact, socket-death trajectories
- `f4c65b9e` — M004 WP G: session vertical-slice E2E trajectory

Implementation commit (this closure):

- live-turn bring-up: deterministic mock provider fixture + selection
  wiring + live-turn E2E leg, plus four production fixes found proving
  it (renderer subscribe ordering, socket already-live guard, envelope
  stream-seq rewrite, per-connection SQLite busy_timeout) — single
  commit on `f4c65b9e`.

## 1. Executive finding

M004 lands the desktop session/control-plane vertical slice as
executable, repeatable machinery:

1. The no-provider trajectory stays green and fail-closed
   (`model_unselected`, editable draft preserved).
2. A deterministic live turn runs against an in-repo mock provider
   through the ordinary daemon selection/provider/turn/projection
   machinery — no external model, no production bypass — and converges
   assistant text, tool start, `PermissionPending`, renderer denial,
   and turn completion with final text through canonical projection.
3. Proving the live turn exposed and fixed four production defects
   (§6); none required architecture changes beyond the M004 plan.

## 2. Requirement-to-evidence matrix

| Plan § / WP | Requirement | Evidence |
|---|---|---|
| §7 WP B | Explicit project+workspace+session route, generation fencing, stale-drop | `route.rs`, prior commits; E2E route legs green |
| §8 WP C | Pure bounded presenter, single owner, tail caps, artifact excerpts | `present.rs`, `projection.rs`, `artifact.rs`; unit + E2E green |
| §9 WP D | Daemon-resolved prompt submit, at-most-once, fail-closed codes | `prompt.rs`, daemon `SessionPromptSubmit`; `model_unselected` leg green |
| §10 WP E | ADR-0007 permission/question/controller through daemon leases | `control.rs`; live-turn deny leg green |
| §11 | Reconnect/resume without duplication | Reconnect + reload legs green |
| §12 | Opaque artifact handles | `artifact.rs`; unit green |
| §13 | camelCase bridge sync | `scripts/check-bindings.mjs` green |
| §15 | Deterministic model/provider fixture | `mock_model` in `desktop_e2e_fixture.rs`, `selectMockModel`, temp `CODEGG_TUI_CONFIG`, per-phase HOME isolation |
| §18 | Built-app closure trajectory | `m004-session.e2e.ts` live-turn leg; 7/7 green |
| §23 | Acceptance: text+permission render, deny completes turn | Live-turn leg asserts all three |

## 3. Production implementation evidence

Live-turn bring-up changes (this closure):

- `apps/desktop/src-tauri/src/bin/desktop_e2e_fixture.rs`: `mock_model`
  module (loopback-only hand-rolled HTTP/1.1, counted two-script SSE
  program, 404 otherwise, bodies never logged, dummy credential in
  temp-scoped config), `ensure_mock_model` before daemon spawn,
  `cmd_select_mock_model` via ordinary `ProviderConnectionCreate` +
  `SessionSelectionUpdate`; `tokio` `net` feature (test helper only).
- `apps/desktop/e2e/fixture-client.ts`: `selectMockModel` (90s
  budget for connection probing).
- `apps/desktop/e2e/specs/m004-session.e2e.ts`: live-turn leg
  (select → submit → `permission-list` + text → deny → `completed` +
  final text).
- `apps/desktop/e2e/run-e2e.sh`: per-phase `HOME` redirect into the
  phase home (restored at phase end) so no operator-global config,
  credential store, or provider environment leaks into the isolated
  daemon.
- `apps/desktop/src/App.tsx`: push-subscribe effect subscribes only
  once a projection view exists for the session (removes the
  create-vs-subscribe race), retries bounded with backoff, and surfaces
  exhaustion as a route error instead of a silent swallow.
- `src/core/transport/daemon_socket.rs`: already-live guard —
  replay/continuation on a live owned subscription delivers the batch
  without re-running activation (which tripped
  `InvalidLifecycle(Live)` and destroyed the healthy subscription).
- `crates/codegg-core/src/projection_replay/service.rs`: persisted
  envelopes rewritten to their stream-local row sequence for both
  storage and live delivery (daemon-global log seqs made every
  post-snapshot event look like a history gap).
- `crates/codegg-core/src/storage/mod.rs`: per-connection SQLite
  pragmas via `SqliteConnectOptions` (`busy_timeout` 30s,
  `foreign_keys`, WAL, `synchronous NORMAL`) — a pool-executed
  `PRAGMA` touches one connection only, leaving the rest at
  `busy_timeout=0` (observed as silently dropped publications under
  concurrent turn + subscription load).
- `src/core/daemon.rs`: clippy `useless_conversion` fix in test code
  (pre-existing full-verify blocker).

Regression tests kept:

- `resume_on_live_subscription_keeps_forwarder_alive`
  (daemon_socket; fails pre-guard, passes post-guard).
- `seam_rewrites_envelope_seq_to_stream_row_seq`
  (publication integration; fails pre-rewrite, passes post-rewrite).

## 4. Verification executed

- `scripts/verify.sh quick` — GREEN (fmt, agent schema,
  core-boundary, sandbox, execution-ownership, tui-authority,
  http-route-disposition, audit-coverage, scheduler-bypass, workspace
  check).
- `scripts/verify.sh full` — clippy GREEN; workspace suite 12029
  passed with 12 environment/interaction failures, each dispositioned:
  `scheduler_cancellation` ×3 fail identically on clean HEAD in
  isolation (pre-existing environment failure); `gui_client` ×7 fail
  only on `SUN_LEN` (all 9 pass with `TMPDIR=/tmp`); `causal_observe`
  ×1 and `asset_refresh` timeout ×1 pass in isolation on both trees
  (nextest sweep interaction under parallel load).
- `apps/desktop`: `tsc` + `vite build` GREEN, vitest 18/18 GREEN,
  `check-bindings.mjs` GREEN, tauri lib 73 passed / 1 ignored.
- `codegg-client` package: all suites GREEN with `TMPDIR=/tmp`.
- Official `apps/desktop/e2e/run-e2e.sh` (all three phases):
  lifecycle 5/5 + autostart 2/2 + session 7/7, exit 0 — the session
  phase completes in seconds (no timeout waits).

## 5. Invariant review

- Loss-aware driver: `ClientEvent::{Event,Lagged,Closed}`, pre-request
  receiver install, exact subscription filtering, ack cadence,
  resume/replay/resync, 25ms liveness — unchanged, live-proven.
- Projection replay authority: stream-local row seqs end to end;
  already-live continuations never re-activate.
- SQLite: every pooled connection carries busy_timeout/foreign_keys/
  WAL/synchronous NORMAL from connect options.
- Renderer subscribe: ordered after owner install, bounded retry,
  loud failure. No silent deafness path remains.
- Mock fixture: test-only loopback, no production code, no secrets
  logged, no bridge/bypass.

## 6. Failure and recovery review

Live-turn bring-up failed closed at every stage and each stall was
root-caused with kept-phase evidence (daemon log, store dump, DOM
observables, per-connection delivery probes, in-fixture independent
driver, app-side file logging — all temporary instrumentation fully
reverted):

1. SSE tool-call chunk `id` repeated on the continuation chunk →
   `ConflictingIdentity` ("provider stream tool-call state was
   invalid"). Fixed: opener carries id+name, continuation carries
   index+arguments only.
2. Cursor resume ~1s after subscribe tripped
   `InvalidLifecycle(Live)` and destroyed the healthy subscription →
   already-live guard (§3).
3. Stream rows 1–4 carried daemon-global seqs 3–6 → perpetual
   gap/resync → envelope seq rewrite (§3).
4. Renderer frozen at cursor 1 with a healthy daemon/driver:
   push-subscribe effect won the race with `projectionStart`, the host
   rejected "no session projection attached", the `.catch` swallowed
   it, deps never changed → no watcher ever → renderer deaf while the
   driver converged to cursor 4. Fixed by ordering + retry + loud
   failure (§3). (Also fixed two red herrings ruled out along the way:
   mid-upload mock close and operator-global config leakage, the
   latter fixed for real by HOME isolation.)
5. Concurrent probe attach + turn publication hit `SQLITE_BUSY`
   (`code: 5`) and the publication was silently dropped (message event
   lost, stream renumbered) → per-connection busy_timeout (§3).

## 7. Migration and compatibility review

No storage migration, no `CoreFrame` version bump, no new desktop
database, no protocol change. `busy_timeout` 30s (from effective 0 on
most pooled connections) only extends transient-contention tolerance;
genuine deadlocks still surface as errors. Renderer effect change is
behavior-preserving except ordering/retry/loudness. All additive.

## 8. Security review (plan §20)

- No generic `CoreRequest` bridge: renderer invokes only the named
  `desktop_*` commands; `selectMockModel` is fixture→daemon, never a
  bridge command.
- No renderer filesystem/shell/process/network authority: mock
  interaction stays daemon-side through the ordinary provider stack.
- No renderer path as authority: session/project/workspace identity is
  daemon-resolved; the canned write targets a phase-temp path outside
  the workspace root and is denied in-spec, never executed.
- Session/project reads honor daemon authorization (unchanged paths).
- Projection subscription is connection-owned and session-authorized
  (owner + generations + resubscribe fencing unchanged).
- Fixture observer never answers permissions/questions (no responder
  registered; permission waits out its bounded timeout by design).
- Permission response uses the daemon controller lease path
  (opaque `perm:` id + choice, ADR-0007 unchanged).
- No private/reasoning content rendered (presenter `Public` +
  User/Assistant/Tool filter unchanged; mock emits one benign text).
- Artifact reads unchanged (opaque handles).
- No prompt body/provider secret logged: mock logs only the request
  index; dummy key lives in a temp-scoped config file; no diagnostic
  added logging of bodies, keys, or paths beyond the phase-temp
  denied path already asserted in-spec.

## 9. Documentation and operations

- `architecture/desktop.md`: session slice, push-subscribe ordering
  contract, live-turn leg + mock fixture + HOME isolation.
- `architecture/client.md`: loss-aware events + driver + composer.
- `architecture/projection.md`: stream-local seq rewrite +
  already-live continuation invariant.
- `architecture/storage.md`: per-connection pragma semantics +
  30s busy_timeout.
- No skill updates: no `.opencode/skills/` module covers
  desktop/client; the touched contracts live in the docs above.

## 10. Unresolved findings / hosted disposition

- Hosted `CI / verify` + path-gated `Desktop E2E` runs execute on push;
  local evidence (§4) is the closure basis. No code path is
  hosted-specific beyond the existing C003 workflow.
- `verify.sh full` local delta (§4) is fully dispositioned as
  pre-existing environment/interaction failures; none touch M004
  paths (all pass in isolation on both trees).
- Approval timeout (300s → timeout-deny) is by design; the E2E turn
  resolves via explicit renderer denial in seconds.
- Never `--all-features` for workspace sweeps (real-server LSP tests
  excluded by policy).

## 11. Roadmap disposition

M004 (desktop session/control-plane vertical slice) is CLOSED:
no-provider trajectory green and fail-closed, deterministic live turn
green through canonical projection, four production defects fixed with
regression cover, docs updated. M005–M006 remain deferred per
`plans/registry.md`; no follow-up corrective required.

## 12. Registry updates

- `plans/registry.md`: Desktop frontend and IDE foundation —
  `M001 closed; M002 conditional; M003 closed; M004 closed (004-status);
  M005–M006 deferred`.
