# Team Collaboration Corrective M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/004-shared-session-controller-lease.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M004`

Repository baseline reviewed: `1bc967c1`

Implementation commits or pull requests:

- `1bc967c1` — feat(session): shared-session controller lease (team-collaboration M004)

## 1. Executive finding

M004 is complete. Active-turn human control is now explicit per
ADR-0007: the principal that successfully submits a turn controls
steer/cancel and permission/question responses for that turn until
terminal state or an explicit authorized transfer/takeover. A second
Contributor may observe and chat but cannot steer, cancel, or answer
the controller's pending items through any network path (Core or
REST). Transfer is controller-only to an eligible recipient under
CAS revision; forced takeover is Maintainer/Owner-only with a
bounded reason and is audited; terminal transitions release the
lease with the terminal side winning every race; restart
reconciles leases against terminal state and trustworthy origin
attribution, otherwise failing closed until recovery takeover. The
TUI shows the cached controller (`control:<principal>`) and offers
`/control` get/request/transfer/release/takeover without weakening
the observer block.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence |
|---|---|
| Controller record keyed by canonical session+turn with principal, origin client metadata, revision, lifecycle timestamps/provenance | `crates/codegg-core/src/session_control.rs` (`SessionControllerRecord`, `SessionControlRequestRecord`); `session_turn_controller` (PK `session_id`) + `session_control_request` tables; `migrate_v65` (layout 64 → 65); unit tests pin acquire-idempotency, second-turn conflict, CAS transfer/takeover, terminal-wins |
| Atomic acquisition with accepting the turn, rollback-safe | `daemon_turns.rs` TurnSubmit: durable `acquire` before execution starts, in-memory turn rolled back (lease + `active_turn` + `Idle`) on store conflict; per-turn reaper spawned at acceptance; `turn_submit_atomically_acquires_controller` + `concurrent_submits_converge_on_one_controller` (exactly one Ack, winner holds revision-1 lease) |
| Controller authorization after the capability decision for `TurnSteer`, `TurnCancel`, permission/question responses; pending IDs resolve to owning session/turn first | `daemon_control.rs::check_turn_controller` (capability gate already ran; then revocation recheck → lease existence → turn match → principal match → controller eligibility); `check_control_response` (Global `none` gate supplemented with owning-project `session.create` + turn-parsed controller check; outsiders see the no-pending shape); `PermissionRespond`/`QuestionRespond` now parse `turn_id` from the pending id before the check |
| Core operations: `SessionControlGet`, request/suggest, transfer, release, forced takeover; request inert; transfer needs controller + recipient eligibility; takeover needs Maintainer/Owner + bounded reason + audit | 5 new `CoreRequest` variants + `SessionControl`/`SessionControlUpdated` responses + `SessionControlChanged`/`SessionControlRequested` events (`session_control.v1`); `handle_control_request` (transfer checks recipient `agent.invoke`; takeover checks explicit Maintainer/Owner role + recipient eligibility + `validate_reason`; request stores bounded inert rows and never mutates the lease); `operation_descriptor` rows (`session.read` get; `agent.invoke` request/transfer/release; `project.configure` takeover) |
| Controller principal/coarse status in safe projection/presence; no credentials or device secrets | `SnapshotSession.controller_principal/revision` + `SessionSnapshot.controller_principal/revision` (additive `#[serde(default)]`); both control events classified `Safe` with ids/revision/action only; `safe_publication` + `has_safe_origin` extended; audit uses `membership_change` with ids/revision only (secret census in tests) |
| Restart/reconnect/race semantics (revision/CAS, terminal wins, disconnect preserves, ambiguous fails closed) | `spawn_turn_reaper` (first terminal envelope for the exact turn id releases + parks `Idle`); `release_turn_controller` deletes on turn match only; `recover_state` releases interrupted leases then `reconcile_session_controllers` (terminal/orphan leases released; unleased active turns derive only from non-legacy, still-eligible origin attribution); recovery takeover with `expected_revision: 0` when no lease row exists |

Authorization matrix changes: 5 new rows (`session_control_get` →
`via_session/session.read`; `session_control_request/transfer/release` →
`via_session/agent.invoke`; `session_control_takeover` →
`via_session/project.configure`). No existing row changed capability
or scope. Audit instrumentation: 4 new `membership_change` mappings
(request/transfer/release/takeover, post-mutation like team
mutations with pre-emit skip via `is_control_mutation`);
`session_control_get` is explicitly uninstrumented (bounded read).

## 3. Production implementation evidence

- Protocol: `crates/codegg-protocol/src/core.rs`
  (`SESSION_CONTROL_CAPABILITY`/`SESSION_CONTROL_PROTOCOL_VERSION`/
  `SESSION_CONTROL_MAX_LIST_LIMIT`/`SESSION_CONTROL_MAX_REASON_LEN`,
  `SessionControllerDto`, `SessionControlRequestDto`; 5 requests, 2
  responses, 2 events; `CoreResponse::SnapshotSession` +
  `SessionSnapshot` gain additive controller fields;
  `CoreResponse` carries `#[allow(clippy::large_enum_variant)]`
  matching the `CoreEvent` precedent).
- Domain: `crates/codegg-core/src/session_control.rs` (new store:
  `acquire`/`release`/`transfer`/`takeover`/`request_control`/
  `list_requests`/`list_active`, bounded validation, CAS diagnostics,
  per-session request cap 20).
- Authorization: `crates/codegg-core/src/authorization/policy.rs`
  (5 descriptors + 5 representative requests; matrix guard still
  passes).
- Audit: `crates/codegg-core/src/audit_instrumentation.rs` (4
  `membership_change` rows + 1 uninstrumented read).
- Events: `projection_replay/safe_publication.rs` classifies both
  control events `Safe` (ids/revision/action only).
- Daemon: `src/core/daemon_control.rs` (new `turns`-family helper:
  `check_turn_controller`, `check_control_response`,
  `acquire/release_turn_controller`, `spawn_turn_reaper`,
  `handle_control_request`, `control_snapshot`,
  `active_turn_id_for_session`,
  `reconcile_session_controllers`); `src/core/daemon_turns.rs`
  (submit acquisition + rollback + reaper spawn, steer/cancel gates,
  permission/question turn-parsing + gates, snapshot projection);
  `src/core/session_runtime.rs` (`TurnHandle` carries
  controller principal/client/revision); `src/core/daemon.rs`
  (control session-id resolution, gate-denial privacy mapping,
  pre-emit skip for control mutations);
  `src/core/daemon_family.rs` (5 variants → `Turns`);
  `src/core/daemon_projects.rs` (`SessionSnapshot` controller
  projection); `src/core/daemon_bootstrap.rs` (recovery releases +
  reconciles; the reaper also repairs the pre-existing stuck
  `active_turn`, which previously never cleared on completion).
- REST: `src/server/authz.rs`
  (`authorize_control_response_for_turn`: capability gate, then
  lease + turn-match + revocation recheck, privacy-safe 404s;
  LocalOwner passes); `permission.rs`/`question.rs` resolve the
  pending turn before the hook (question fan-out requires one
  unambiguous turn).
- TUI: `src/tui/commands/control.rs` (new `/control`
  get/request/transfer/release/takeover with spawn-and-complete
  tasks, request-id/session/epoch stale guards, secret-free line
  renderer); `TuiCommand::ControlLoaded/ControlMutationFinished` +
  dispatch arms; `DialogState::control_request/control_last`;
  `InfoType::Control` (shares the Team dialog slot);
  `BuiltinSlashAction::Control` + `/control` registry entry
  (`architecture/command.md` 150 → 151); status-bar
  `control:<principal>` indicator; observer allowlist unchanged so
  every `/control` subcommand stays hard-blocked while observing
  (pinned by new blocked-family test rows).
- Tests: `tests/session_control_m004_controller_lease.rs` (23
  boundary tests, see §4); `session_control` unit tests (4);
  `server::authz` control-hook tests (3, server-gated).

Resolver truth (enforced by tests, not prose): controller steers/
cancels/responds; second Contributor denies as
`session_control_not_controller`; same-principal second device
passes; transfer moves control (old controller denies, new one
passes); non-controller/ineligible/stale transfer denies with zero
overwrite; Maintainer takeover passes with audit + event while
Contributor takeover denies at the gate; revocation denies at the
next boundary (gate); terminal release deletes on turn match and
stale transfers observe `NotFound`; submit races converge on one
revision-1 lease; no-lease active turns fail closed for control but
accept recovery takeover at revision 0; legacy attribution derives
nothing.

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`):

- `cargo test --test session_control_m004_controller_lease -- --test-threads=1` — 23 passed:
  matrix classification; second-contributor steer/cancel denial;
  controller steer/cancel; same-principal second-device resume;
  viewer/outsider negatives; chat-granted Viewer still denied;
  permission denial + controller answer; question denial; transfer
  handoff (Bob steers, Alice denied); unauthorized/stale/ineligible
  transfer negatives; release + fail-closed; Maintainer takeover +
  audit + event + secret census; Contributor takeover denial;
  no-lease recovery takeover (revision 0 ok, nonzero conflicts);
  revocation denial; terminal release + stale-transfer race;
  reaper release on `TurnCompleted`; submit acquisition + snapshot
  projection; concurrent-submit convergence; get/request round-trip
  (inert); trustworthy-attribution derivation; legacy fail-closed.
- `cargo test -p codegg-core --lib session_control -- --test-threads=1` — 4 passed.
- `cargo test -p codegg --features server --lib server::authz::tests::control_hook -- --test-threads=1` — 3 passed
  (controller allow / non-controller privacy-safe denial /
  turn-mismatch + missing-lease denial / revocation denial).
- `cargo test --test presence_m003_observation -- --test-threads=1` — 11 passed (no divergence).
- `cargo test --test identity_m003_daemon_authorization -- --test-threads=1` — 9 passed (no divergence).
- Permission/question integration surface: `cargo test -p codegg --lib permission -- --test-threads=1` — 104 passed;
  `cargo test -p codegg --lib question -- --test-threads=1` — 13 passed.
- `cargo test --test collaboration_m002_chat_policy -- --test-threads=1` — 13 passed;
  `cargo test --test collaboration_m002_chat_tui -- --test-threads=1` — 11 passed (M002 intact).
- `cargo test --test team_m003_membership_admin -- --test-threads=1` — 12 passed (M003 intact).
- `cargo test --test storage_migrations -- --test-threads=1` — 4 passed (v65 additive).
- `cargo test -p codegg --lib tui:: -- --test-threads=1` — 960 passed (incl. new control/observer/registry-151 tests).
- `cargo test -p codegg --lib core:: -- --test-threads=1` — 206 passed (daemon turn/auth paths).
- `cargo test -p codegg-core --lib -- --test-threads=1` — 792 passed.
- `python3 scripts/check_authorization_matrix.py` — pass.
- `python3 scripts/check_project_catalog_invariants.py` — 7/7 pass (layout 65 tracks `migrate_v65`).
- `python3 scripts/check_task_trigger_boundaries.py` — ok (stale exact-63 pin relaxed to `>= 63`; trigger shape checks unchanged).
- `bash scripts/check-core-boundary.sh` — pass; `check_execution_ownership.py` — ok;
  `check_tui_project_authority.py` — pass; `check_daemon_cwd_usage.py` — pass;
  `check_git_forbidden_patterns.py` — PASS; `check_http_route_disposition.py` — ok;
  `check_websocket_bounds.py` — ok; projection disclosure/publication/transport guards — ok.
- `cargo fmt --all -- --check` — pass; `git diff --check` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy -p codegg --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings` — pass.
- `scripts/verify.sh quick` — pass (fmt, agent schema, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Substituted verification (documented, not weakened): the plan lists
`cargo clippy --workspace --all-targets --all-features`. Per
`AGENTS.md`, workspace sweeps never use `--all-features` (it drags
real-server tests); the two clippy invocations above cover the same
code under the supported feature sets (`verify.sh full` uses
`--features server,plugins,lsp-test-support`). No finding suppressed.

Pre-existing failures (verified unrelated, not caused by this change):

- `python3 scripts/check_audit_coverage.py` fails `every daemon
  operation is classified` on clean `HEAD` too (verified in a
  detached worktree at `5d278794`): the guard reads
  `OperationDescriptor::new("…")` literals from
  `authorization.rs`, which contains none (descriptors live in
  `policy.rs`). Our 5 operations are still classified in both
  `INSTRUMENTED_OPERATIONS` (mutations) and
  `UNINSTRUMENTED_OPERATIONS` (get).
- `python3 scripts/check_scheduler_bypass.py` flags
  `src/agent/snapshot_capture.rs:181`, a file this change does not
  touch.
- `server::authz::tests::every_shared_authz_row_names_a_capability`
  fails on the untouched `POST /api/project` disposition row
  (`SharedAuthz` + `none`); fails identically without this change.

CI truthfully: only local evidence is claimed here; CI (`verify`
job) will re-run the canonical guards.

## 5. Invariant review

- Lease only narrows: every control path runs the capability gate
  first; `LocalOwner` solo behavior is unchanged (submitter
  controls its turn).
- At most one effective controller per active turn: one PK row per
  session, `acquire` conflicts for any other turn/principal, CAS on
  every mutation (pinned by conflict + stale-revision tests).
- Terminal releases: reaper + `release_turn_controller` delete on
  turn match only; stale writers observe `NotFound` (pinned).
- Observer/chat never grant control: observer TUI blocks every
  `/control` subcommand (pinned); Viewer + explicit chat grant
  still denied at the gate (pinned); REST hook requires the lease
  after the capability gate (pinned).
- Disconnect never transfers: lease rows carry no liveness; the
  reaper only reacts to terminal envelopes; reconnect reuses the
  same principal (second-device test pins client-independence).
- Transfer/takeover explicit, revisioned, audited: CAS + provenance
  fields + `SessionControlChanged` events + `membership_change`
  audit with ids/revision only (pinned, secret census green).
- Revoked/suspended principals ineffective: gate + handler +
  REST-hook rechecks against current team state (pinned for Core
  steer, Core respond, and REST).

## 6. Failure and recovery review

- Stale transfer/takeover revisions conflict with zero overwrite
  (pinned); TUI surfaces the conflict with a reload hint
  (`/control` shows current revision).
- Submit races converge: losers observe `turn_already_active` and
  leave no lease (pinned); the in-memory turn rolls back with the
  store conflict.
- Transfer/completion races: terminal wins; later transfers
  observe `NotFound` (pinned).
- Restart: interrupted turns emit `TurnFailed` and release;
  terminal/orphan leases are released; unleased active turns
  derive only from trustworthy attribution, else fail closed until
  recovery takeover at revision 0 (both pinned).
- Request retries are inert: bounded rows, newest-20 cap, never a
  lease mutation (pinned revision-1 after request).
- Ambiguous question fan-out (pending items across turns) fails
  closed in the REST route before authorization.

## 7. Migration and compatibility review

Additive migration v65 (`IF NOT EXISTS` tables + indexes);
`STORAGE_LAYOUT_VERSION` 64 → 65 in lockstep; pre-lease databases
gain empty tables and fail closed (no backfill, no grant change).
Pre-existing active turns derive controller only from trustworthy
`origin_attribution` (non-legacy, still `agent.invoke`-eligible);
otherwise Maintainer/Owner recovery takeover is required.
Rollback drops the binary: the two tables are inert without the
new handlers, and older clients keep ordinary turn/chat behavior
(new variants are additive; unknown variants fail closed at the
serde layer). REST compatibility keeps its capability shape and
adds only the lease narrowing.

## 8. Security review

Cross-boundary matrix proven: controller allow; second-Contributor
steer/cancel/respond negatives (Core, typed); same-principal
second-device allow; Viewer/outsider gate negatives; chat-granted
Viewer negative (chat ≠ control); transfer allow + three negatives
(non-controller, stale, ineligible recipient); release allow +
non-controller negative; Maintainer takeover allow + audit/event +
secret census; Contributor takeover gate negative; no-lease
recovery takeover (0 ok / nonzero conflict); revocation negatives
(Core + REST); terminal + race negatives; permission/question
turn-parsing negatives. Denials carry no existence signal beyond
the operation/capability names the caller supplied (gate) or the
typed controller codes for authorized members (handler); REST
denials are byte-identical 404s. `eggsentry`/adversarial suites
were not re-run beyond the focused suites; no new network, crypto,
or secret-handling primitive was added (lease DTOs carry ids/
revisions/reasons only).

## 9. Documentation and operations

- `architecture/authorization.md`: M004 lease section + 5 matrix
  rows + REST hook update.
- `architecture/session.md`: active-turn controller lease section
  (v65 tables, acquire/release, projection).
- `architecture/presence.md`: observer matrix rows for
  permission/question + steer/cancel + `/control` blocking.
- `architecture/tui.md`: `/control` surface section (state, flow,
  indicator, observer enforcement).
- `architecture/server.md`: REST hook narrowing update.
- `architecture/collaboration.md`: M004 lease section + test table
  update.
- `architecture/protocol.md`: session-control/permission gating
  entries.
- `architecture/command.md`: built-in total 150 → 151.
- User help: `/control` registry description + usage strings for
  `request`/`transfer`/`release`/`takeover`.
- Operational note: after an ambiguous transport failure on a
  control mutation, refresh with `/control` (current revision)
  before retrying; revocation applies at the next request
  boundary; recovery takeover uses revision 0 with a bounded
  reason.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. Three explicit
non-claims (not findings): `check_audit_coverage.py`,
`check_scheduler_bypass.py`, and the `SharedAuthz` capability test
fail identically without this change (pre-existing, §4); existing
connections after revocation follow the pre-existing caller-owned
disconnect contract (unchanged from M003).

## 11. Roadmap disposition

M004 closes the shared-session controller capability. Per the
addendum dependency graph, M005 (Workspace chat) requires only
M001+M002 (already ready) and is unchanged; M006 still requires
M001–M005 with M005 open, so it stays `blocked`. The subsystem
roadmap stays `active` with M004 marked closed.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/004-shared-session-controller-lease.md`:
  `active` → `implemented` (closure at this record;
  implementation `1bc967c1`).
- `plans/registry.md`: M004 `active` → `closed` (closure at this
  record); M005 stays `ready`; M006 stays `blocked` on M001–M005
  (M005 still open).
- Subsystem roadmap
  `plans/subsystems/team-collaboration-corrective-addendum.md`:
  M004 `active` → closed in the milestone table via this closure.

Dependency audit: M005
(`005-workspace-selected-project-chat-view.md`) requires M001+M002
(both closed; consumes accepted ADR-0006 DTOs) — already `ready`,
unchanged. M006 requires M001–M005 (M005 still open → stays
`blocked`). No corrective follow-up is registered: no new defect
was found that M004 itself must fix.
