# Presence and Observation Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/presence-observation/003-authorized-read-only-observation.md`

Source subsystem roadmap:

- `plans/subsystems/presence-observation-roadmap.md#M003--authorized-read-only-observation`

Repository baseline reviewed: `2a6403d8f5a411d69c7cded7f22f493e7ca8e403`

Implementation commits or pull requests:

- `2a6403d8` — feat(presence): M003 authorized read-only session observation (canonical session.observe, observer TUI, read-only policy, 7 unit + 11 integration tests, observer docs).

## 1. Executive finding

M003 is closed. An authorized project member can select another session
(`/observe <session-id>`) and follow its canonical projection
snapshot/replay/live stream in an explicitly read-only TUI mode.
Session-scope observation requires canonical `session.observe` on the
owning project, enforced through the existing daemon gate plus a
team-derived projection access context (no synthetic allow-all
authority remains on the observation path); authorization is rechecked
on every resume and artifact read/list. Ordinary observer input —
prompt submit, permission/question answers, steering, cancels,
model/agent/provider changes, file/worktree/Git/job mutations — is
rejected centrally with no mutation or control transfer. Unauthorized
callers learn nothing (denials are `project_not_found`,
indistinguishable from absent). Redaction before durable replay is
unchanged and proven end-to-end on the observer path. No unresolved
high, medium, or low M003 finding remains; two pre-existing
environmental/test-maintenance findings are documented in §10 with
in-tree resolution or a documented workaround.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Map projection capabilities to canonical `session.observe`; remove synthetic authority (package A) | `ProjectionAccessContext::authorize_scope` now requires `ObserveSessionProjection` for session scope (`crates/codegg-core/src/projection_replay/context.rs`); `CoreDaemon::session_observe_allowed` (session-row project + `session.observe`); `canonical_observe_access_for_project` (team expansion via `team_capabilities_to_projection` + resolver bounded to target project); subscribe/resume/artifact arms enforce the canonical context; `authorization_denial` maps subscribe/artifact denials to `project_not_found` | pass | `operation_descriptor` matrix unchanged (no new wire variant); `check_authorization_matrix.py` green. |
| Observe/stop-observing frontend flow on existing subscription/replay APIs (package B) | `ObserverState` reducer (`src/tui/app/state/observe.rs`); `src/tui/commands/observe.rs` (capabilities → subscribe → resume → unsubscribe on registered tasks); `TuiCommand::StartObserve/ObserveSubscribed/ObserveResumed/StopObserving/ObserveUnsubscribed` + dispatch arms; `/observe` (`/watch`) with active-tab project resolution; `/stop-observing` (`/unwatch`); collaborator panel `/observe` hint seam; stop returns the observer-owned id for authoritative unsubscribe | pass | No new streaming stack; single observed session; no chat storage (placeholder only). |
| Central read-only command/input policy + negatives for every control family (package C) | `is_observer_allowed_command` narrow allowlist (fail closed); `execute_command` top guard; `send_prompt` reroute to collaboration placeholder; `submit_permission_response` / `on_permission_confirm` / `submit_question_answers` rejections; `👁 OBSERVING … (read-only)` header banner (locator + coarse status only) | pass | Denied/unsupported shells stay input-blocked until explicit stop. |
| Reconnect/lag/resync/multiple-observer/load/privacy tests (package D) | `on_reconnect` epoch bump + cursor resume; revocation resume-denial + transient cleanup; 3-observer independence; 32-per-client + 256-daemon bounds; outsider non-enumeration; owner-disconnect control negative — all in `tests/presence_m003_observation.rs` (11 tests) | pass | Real-transport replay/resync evidence via `projection_transport_real` (58 passed). |
| Observer UX/docs + collaboration seam placeholder (package E) | Banner, toasts, help entries, collaborator hint; `collaboration_input_placeholder` stable seam; `architecture/presence.md` M003 section with visibility/control matrix; `tui.md`, `protocol.md`, `authorization.md` updates | pass | Project chat explicitly not implemented (stays blocked behind collaboration M001). |

## 3. Production implementation evidence

- `crates/codegg-core/src/projection_replay/context.rs`: session scope
  requires `ObserveSessionProjection` (canonical `session.observe`);
  project scope still requires `ObservePublicProjection`. Local users
  hold both; team roles that may observe hold both; single-capability
  principals reach only the matching scope. Core lib 611 green.
- `src/core/daemon.rs`:
  `session_observe_allowed(client, session)` (session-row project +
  `session.observe`; LocalOwner/pool-less preserved);
  `canonical_observe_access_for_project` (membership → team caps →
  bounded resolver; empty caps fail closed);
  `observe_scope_project_boxed` / `observe_access_boxed` (boxed team
  lookups so the near-limit dispatch future holds only the box);
  subscribe arm (session `session.observe` + canonical policy;
  project-scope revocation recheck; binding-vs-canonical mismatch fails
  closed; binding-absent fallback to the authorized session-table
  project); resume arm (per-stream-kind recheck on every resume +
  transient owned-subscription cleanup on denial); artifact read
  (canonical context) and artifact list (canonical scope check);
  `authorization_denial` maps subscribe/artifact denials to
  `project_not_found`.
- `src/tui/app/state/observe.rs` (new, ~750 lines with 7 tests):
  `ObserverState` (single target, request/epoch stale guards,
  `begin_observe` locator validation, subscribe/resume/deny/error
  applies, resync coalescing, `begin_resume`, `on_reconnect`,
  `stop`/`clear`, content-free banner, collaboration placeholder,
  `blocks_command`/`blocks_prompt_submit`/
  `blocks_permission_response`), `is_observer_allowed_command`
  allowlist, `observer_blocked_message`.
- `src/tui/commands/observe.rs` (new): `start_observe` (replace-only,
  capability negotiation + session subscribe),
  `apply_observe_subscribed`, `resume_observe` (cursor resume with
  resubscribe fallback on resync), `apply_observe_resumed`,
  `stop_observing` (observer-owned unsubscribe best-effort),
  `check_observer_block` guard.
- `src/tui/app/mod.rs`: `App.observer` (both constructors);
  `TuiCommand::{StartObserve, ObserveSubscribed, ObserveResumed,
  StopObserving, ObserveUnsubscribed}`; `/observe`/`/watch` +
  `/stop-observing`/`/unwatch` arms (project from active tab);
  `execute_command` top guard; `send_prompt` reroute;
  permission/question negatives; header observer banner;
  `on_projection_reconnect` observer epoch + resume;
  `start_observe`/`apply_observe_subscribed`/`resume_observe`/
  `stop_observing`/`is_observing` façades.
- `src/tui/runtime/command_dispatch.rs`: five observer arms (async
  dispatch stays `fn`-sync; handlers spawn-and-complete).
- `src/tui/command.rs`: `/observe` (`/watch`), `/stop-observing`
  (`/unwatch`) registry entries (built-in count 114 → 127; the count
  was already stale at 125 on the clean tree — see §10).
- `src/tui/input.rs`: `/observe`, `/stop-observing` help entries.
- `src/tui/app/state/presence.rs`: collaborator panel gains the
  bounded `/observe <session-id>` hint on Ready branches (M002
  identical-rendering invariants untouched).
- `tests/presence_m003_observation.rs` (new, 11 tests): allow/deny +
  absent-indistinguishability, outsider non-enumeration, daemon
  steer/cancel/model/file negatives, observer-only teardown with
  surviving observer + history survival, owner-disconnect control
  negative, revocation resume-denial + transient cleanup,
  multi-observer + 32-per-client/256-daemon bounds, snapshot+replay
  secret-redaction proof (`api_key` → `[REDACTED]`, no reasoning leak),
  artifact project scope, TUI lifecycle/banner/policy/reconnect,
  collaborator observe-hint seam.
- Docs: `architecture/presence.md` M003 section (authz, matrix,
  failure/reconnect, compatibility, testing),
  `architecture/tui.md` observer-mode section,
  `architecture/protocol.md` observation-reuse note,
  `architecture/authorization.md` session-observation subsection.

Distinguished as absent (downstream, not M003 scope): project chat
storage/actions (collaboration M001), control transfer/suggestions,
raw PTY observation, remote node replication.

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test -p codegg --lib tui::app::state::observe
cargo test --test presence_m003_observation
cargo test --test presence_m001_leases
cargo test --test presence_m002_collaborators
cargo test -p codegg-core --lib
cargo test -p codegg --lib
cargo test --test projection_replay_daemon_protocol --test projection_replay_subscription --test projection_replay_resume --test projection_replay_restart_recovery --test projection_replay_transport_isolation --test projection_disclosure_invariants --test projection_artifact_handles
cargo test --test tui --test tui_render --test tui_project_tabs --test tui_project_routing
cargo test --test identity_m003_daemon_authorization
RUST_MIN_STACK=16777216 cargo test --features server --test projection_transport_real -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
```

### Results

- `tui::app::state::observe`: 7 passed (locator validation, stale
  guards, deny/input block, full control-family matrix, reconnect,
  stop, banner content-freedom).
- `presence_m003_observation`: 11 passed (matrix §2, package D row).
- `presence_m001_leases`: 15 passed; `presence_m002_collaborators`:
  12 passed (M001/M002 regression green incl. panel-hint change).
- `codegg-core --lib`: 611 passed (incl. tightened `authorize_scope`
  semantics).
- `codegg --lib`: 4408 passed, 0 failed (run with
  `RUST_MIN_STACK=16777216`; see §10 for why).
- Projection replay/disclosure/artifact suites:
  13/13/9/8/7/16/13 passed.
- `tui` 164 / `tui_render` 99 / `tui_project_tabs` 20 /
  `tui_project_routing` 27 passed.
- `identity_m003_daemon_authorization`: 9 passed (gate unchanged).
- `projection_transport_real` (server): 58 passed with
  `RUST_MIN_STACK=16777216 --test-threads=1` (see §10).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- All six static guards: pass (authorization matrix, core boundary,
  projection disclosure, TUI project authority, execution ownership,
  scheduler bypass).
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace projection/observation/tui`
as bare filters; no such workspace crates exist, so the justified
substitutes above were used (same convention as the M001 closure).
No verification was skipped without a substitute.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| No permission/question answers, steering, cancels, mutations, or settings changes via observation | TUI central guards + daemon `agent.invoke`/`file.modify` denials pinned by unit + integration tests | pass |
| Redaction before durable replay remains authoritative | Unchanged pipeline; observer snapshot+replay proof (`AKIA…` → `[REDACTED]`, reasoning absent) | pass |
| Provider-hidden reasoning never exposed | Reasoning deltas stay `InternalNotSerializable`-denied; replay assertion pins absence | pass |
| Raw terminal frames are not observation protocol | Observation uses subscribe/resume/replay only; no PTY/screen path added | pass |
| Unauthorized callers learn nothing | `project_not_found` for missing and denied alike (subscribe, resume, artifact list/read, presence, project get); empty enumeration | pass |
| Presence never authorizes; observation reuses projection backpressure | No new stream/queue; 32/256 caps + bounded locators/errors; `PresenceUpdated` unchanged | pass |

## 6. Failure and recovery review

- Duplicate delivery: subscribe/resume applies are idempotent renewals
  keyed by `(request_id, reconnect_epoch, session_id)`; resync flags
  coalesce while loading (pinned).
- Cancellation races: stop clears the target and unsubscribes the owned
  id best-effort; stale completions with old request ids or epochs drop
  (pinned by stale + reconnect tests).
- Daemon restart: projection store is durable; observer resumes from
  the authoritative cursor or resyncs to a fresh snapshot; nothing is
  fabricated in the reducer.
- Revocation: resume recheck denies as `project_not_found` and cleans
  transient owned subscriptions (pinned).
- Owner disconnect: watcher subscription survives; no control transfers
  (pinned).
- Contention: per-client/daemon subscription caps enforced
  (`projection_subscribe_failed` at 33, pinned); multiple observers
  share no mutable ownership (pinned).
- Malformed input: overlong/NUL/control locators reject before any I/O
  (`begin_observe` → `None`, pinned); unknown sessions deny as
  not-found; binding/project mismatch fails closed.
- Bounded event/artifact behavior: artifact handles remain
  project-scoped opaque ids; reads capped by the 64 KiB protocol
  window; banners/placeholders/errors carry no content or secrets.

## 7. Migration and compatibility review

- No durable migration: no schema change, no `STORAGE_LAYOUT_VERSION`
  bump.
- Protocol is additive-reuse only: no new `CoreRequest` variant (matrix
  unchanged), no `PROTOCOL_VERSION` bump. Older daemons answer
  capability `supported: false` and render the generic unavailable
  panel; project tabs keep working (pinned by unsupported-path tests).
- Config: no new settings surface.
- Rollback: dropping the binary clears observer state (ephemeral
  reducer) and daemon restart preserves only durable projection
  history per existing M012 semantics.

## 8. Security review

- Authorization precedes lookup on every observation path: gate
  (`project.observe`) + handler (`session.observe` via session row) +
  canonical context policy on subscribe; per-stream recheck on resume;
  team-derived recheck on artifact read/list.
- Principal derived from connection: `request_authority_for_client`
  supplies the principal; `session_observe_allowed` and
  `canonical_observe_access_for_project` take only transport-bound
  identity plus locators (pinned by DTO-authority tests carried over
  from M001/M003 identity suites).
- Privacy: single-session/project denials use `project_not_found` for
  subscribe (both scopes), resume, and artifact list/read; outsider ==
  absent pinned at subscribe, resume-after-revocation, artifact, and
  presence/project-list levels; stop/clear drops cached observed state.
- Secrets: snapshot/replay proof shows `api_key` redacted with marker
  surviving and no reasoning leakage; banners/placeholders/panels
  carry locators + coarse status only (pinned); errors truncate to 256
  bytes; locators bounded to 128 bytes.
- Path validation: session/project locators are opaque (never
  interpreted as paths); typed `ProjectId` parsing gates the
  project-scope rechecks.
- DoS bounds: 32/client + 256/daemon subscriptions, 512-event/64 KiB
  replay caps (unchanged), 128-byte locators, 256-byte errors, no
  per-hint task (`note_resync` coalesces; resume is explicit).
- Audit: observation reads remain liveness projections (no audit
  events), consistent with M001; gate denials still flow through the
  existing denial-audit path.

## 9. Documentation and operations

- Updated: `architecture/presence.md` (M003 section: authz, full
  visibility/control matrix, failure/reconnect, compatibility,
  testing), `architecture/tui.md` (observer-mode surface/dispatch/
  enforcement), `architecture/protocol.md` (observation-reuse note),
  `architecture/authorization.md` (session-observation subsection).
- Help/registry: `/observe` (`/watch`), `/stop-observing` (`/unwatch`)
  in `src/tui/command.rs` + `execute_command` + `src/tui/input.rs`
  help entries; collaborator panel `/observe` hint seam for the
  project-collaboration roadmap.
- Operator diagnostics: `👁 OBSERVING <session> (read-only) · <state>`
  header banner; `Observation resynced — live` /
  `Session not found or not authorized for observation` /
  `Observer mode is read-only — …` toasts; `projection_subscribe_failed`
  on queue saturation.
- Static guards: authorization-matrix, core-boundary,
  projection-disclosure, TUI-project-authority, execution-ownership,
  scheduler-bypass all green; no new CI lane added per verification
  policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Debug-build 2 MiB stack overflow in `projection_transport_real::real_core_100_cycle_churn_with_baseline` and `codegg --lib core::daemon::tests::durable_schedule_tui_display_token_…` | Test-harness only: default-stack debug runs abort these binaries. Verified pre-existing on the clean tree (`13be1831`, via `git stash`) for the churn test; the daemon-test family is already recorded as clean-tree-overflowing by the interactive-process M002 closure. No production impact (release stacks + the 16 MiB test runtime are unaffected). | No code change in M003 (dispatch additions were additionally boxed via `observe_scope_project_boxed`/`observe_access_boxed` to avoid growing the near-limit dispatch future). Functional evidence gathered with the repo-precedent workaround: `RUST_MIN_STACK=16777216` → 58/58 transport-real and 4408/4408 lib green. Re-check if MSRV/debug frame budgets change materially. |
| low | `built_in_command_count_matches_release_docs` was stale (114 asserted, 125 actual on the clean tree) | Test-maintenance only: 11 commands from intervening work (interactive terminals, skills, habits) landed without updating the count; M003 adds 2 more. | Fixed in-tree to 127 in this milestone; noted here so the drift is not mistaken for an M003 regression. No further action. |

There are no unresolved critical, high, or medium M003 findings.

## 11. Roadmap disposition

Milestone closed and the Presence and Read-Only Observation subsystem
is complete: M001 (leases) + M002 (collaborator surface) + M003
(authorized observation) are all accepted, satisfying the roadmap
completion definition (authorized teammates see bounded presence and
can observe another session read-only via canonical projections;
unauthorized callers learn nothing; presence stays ephemeral; no
control authority leaks).

Dependency audit: project-collaboration M001 hard-depends on
presence-observation M003 and identity/audit M005 (both now closed),
with no other open hard dependency — it is unblocked to `ready` in
this closure. Collaboration M002/M003 remain blocked on collaboration
M001 (unchanged). No other registered plan lists M003 as a blocker.
No corrective follow-up is required; no new dependency-ready plan was
created.

## 12. Registry updates

- `plans/registry.md`: Presence row `active / M003 ready` → `closed /
  M003 closed`; dependency-ready table: remove the M003
  authorized-observation row, add project-collaboration M001
  channel/message-protocol row (unblocked by this closure);
  execution order §3 reworded (M001–M003 closed, subsystem complete);
  execution order §4 reworded (collaboration M001 may proceed);
  blocked work: remove the collaboration-M001-on-M003 blocker row
  (keep M002/M003-on-M001 rows); closure control points: add M003
  closed row.
- `plans/subsystems/presence-observation-roadmap.md`: Status `active`
  → `closed`; M003 `ready` → `closed` with closure link.
- `plans/implementation/presence-observation/003-authorized-read-only-observation.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/project-collaboration/001-project-channel-message-protocol.md`:
  `blocked` → `ready for handoff` with unblocked-by-M003 note.
- `plans/subsystems/project-collaboration-roadmap.md`: M001 `blocked`
  → `ready` with plan link (M002/M003 stay blocked on M001).
