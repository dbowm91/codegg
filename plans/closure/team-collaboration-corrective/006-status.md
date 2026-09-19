# Team Collaboration Corrective M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/006-multi-user-trajectory-security-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M006`

Repository baseline reviewed: `9b30150c`

Implementation commits or pull requests:

- harness + test-maintenance commit (this closure): new
  `tests/team_collaboration_m006_trajectory.rs` (13 tests) with
  `required-features = ["server"]` registration in `Cargo.toml`, plus a
  test-only repair to the M001 HTTP suite for the accepted M004
  controller-lease semantic (no production delta; see §3).

## 1. Executive finding

M006 is complete. The reusable multi-principal fixture (Alice Owner of
Project A+B with two device tokens, Bob Contributor in A only, Carol
Viewer in A only with a project chat grant, Mallory outsider with no
membership, plus a restricted channel in A where Bob is denied) exercises
the full team trajectory across Core and HTTP surfaces: parity-checked
reads/enumeration, the complete chat policy matrix, live
administration/revocation while clients stay connected, explicit
Alice→Bob controller handoff with stale-revision and ineligible-recipient
negatives, reconnect/restart convergence, Workspace dashboard filtering
plus rapid selected-project chat switching without cross-routing,
idempotent-retry/CAS contention convergence, a secret-negative census,
pre-corrective compatibility, LocalOwner solo mode, and cross-project
mutation probes with zero side effects. Every privilege-escalation and
cross-project probe fails closed; no high/medium authorization, privacy,
controller, migration, or routing defect remains.

Required production changes: none. Qualification exposed one stale
M001-suite expectation (pre-M004 control-response behavior), repaired
with a test-only update that encodes the accepted M004 semantic — no
production file changed. Larger architectural discoveries: none, so no
new corrective plan is registered.

## 2. Requirement-to-evidence matrix

| Plan requirement (§7 work package / §10 scenario) | Evidence |
|---|---|
| A. Reusable multi-principal fixture: LocalOwner, Owner, Contributor, Viewer, outsider, multiple device tokens, two projects/channels/sessions | `world()` in `tests/team_collaboration_m006_trajectory.rs`: catalog-registered A/B (M001 path), Alice Owner A+B + `client-alice-2` second device, Bob Contributor A-only, Carol Viewer A-only + A project chat grant, Mallory member nowhere, default + restricted (Bob denied) channels in A, default channel in B, seeded sessions; `install_alice_turn` for controller tests (M004 pattern) |
| B. HTTP/Core parity + event isolation before collaboration flows | `http_core_parity_for_reads_and_enumeration` (Core `ProjectGet`/`SessionLoad` allow/deny == `authorize_project`/`authorize_session` allow/deny; Bob sees exactly A, Mallory sees nothing via `filter_visible_projects`); `global_event_surface_stays_local_owner_only` (disposition table pins `/api/event` LocalOwner-only; `require_local_owner` denies Bob, allows LocalOwner) |
| C. Chat policy matrix: Viewer grant, Contributor deny, restricted channel, structured-action negatives | `chat_policy_matrix_end_to_end`: Carol sends in A / denied in B; Bob allowed general / denied restricted (privacy-safe) with denied history indistinguishable from absence; Mallory denied at list/history/sync; Carol `TurnSubmit` denied (chat ≠ execution); Carol `ChatActionSubmit` denied with zero side effect; Bob→B probe `project_not_found` |
| D. Team administration + token/membership revocation while connected | `team_administration_and_revocation_while_connected`: Bob/Carol cannot use `member.manage`; Bob cannot self-add to B; stale `team_revision_conflict` with zero overwrite; Alice revokes Bob mid-connection → next-boundary denial of chat, `SessionLoad`, and `SessionControlGet`; revocation row retained non-active (monotonic) |
| E. Observe/chat/control request/transfer/takeover, permission/question, reconnect/restart | `controller_transfer_handoff_between_alice_and_bob` (Bob steer `session_control_not_controller`; Alice second device steers; stale transfer conflicts; transfer moves control — Bob steers, Alice denied; ineligible-recipient transfer fails); `permission_question_and_control_follow_revocation_and_reconnect` (unknown pending ids fail closed for Bob and Mallory; revoked Bob stays denied after client re-registration); `restart_preserves_policy_and_revocation` (fresh daemon on same pool: Carol grant holds, Bob denials hold, history durable) |
| F. Workspace selection: ordinary composer + side-panel chat across rapid project changes | `workspace_dashboard_filters_by_membership_and_revocation` (Alice sees A+B, Bob A-only, Mallory none; revoked Bob sees none; A chat never surfaces in B sync); `workspace_rapid_switch_never_cross_routes_chat_or_prompt` (TUI App + fake Core: A→B→A→B selection flapping always panels the selected project's chat, per-project drafts never cross) |
| G. Stress/race + negative secret/content census with exact evidence | `idempotent_retries_and_cas_contention_converge` (duplicate `ChatSend` key returns original message with `duplicate`, seq count stays 1; 3× control requests leave lease revision untouched; policy CAS winner Deny stands, stale loser `chat_revision_conflict`, grant restored at current revision); `secret_negative_census_across_trajectory` (`cggt_…` plaintext only in `TeamTokenCreated`, absent from token/member lists, history, sync, composing, control views; chat body marker absent from content-free composing snapshot) |
| §9 compatibility: pre-corrective DB fixture, role-default chat compat | `pre_corrective_role_defaults_and_local_owner_solo`: no-overlay database keeps Viewer default deny + Contributor default allow |
| §9 LocalOwner solo without team setup | Same test: fresh catalog → workspace/project/session create + chat as LocalOwner with zero team rows |
| §10 Alice/Bob/Carol/Mallory end-to-end scenario | Covered across the matrix tests above on one shared world shape: Carol chats only in granted A; Bob denied restricted; Mallory learns no identities (empty enumeration, 404 everywhere); Alice→Bob turn transfer; non-controller mutation fails; revocation terminates chat/control/event access; rapid Workspace switching never cross-routes |

## 3. Production implementation evidence

No production file changed (`git status` shows only the new harness,
its `Cargo.toml` target registration, the M001 test repair, and planning
docs). `STORAGE_LAYOUT_VERSION` unchanged (65, from M004); no migration,
no protocol change, no new Core operation, no authorization-matrix row
change, no new durable identity.

Test-only repair (stale M001 expectation vs accepted M004 semantic):
`pending_control_ids_do_not_leak_across_projects` in
`tests/team_collaboration_m001_http_auth.rs` registered legacy pending
items with `turn_id=None` and no controller lease, then expected the
owner's respond to succeed. M004 (accepted closure
`plans/closure/team-collaboration-corrective/004-status.md`, resolver
truth: "no-lease … fail closed … until recovery takeover") deliberately
narrowed `authorize_control_response_for_turn` to fail closed without a
lease — a narrowing M001 itself anticipated ("M004 will narrow
`authorize_control_response` to the controller lease", M001 §10). The
test now registers both items under one owning turn and acquires the
matching durable owner lease, preserving every original assertion
(outsider/viewer 404s, owner respond Ok, registry cleanup). All 10 M001
tests pass unmodified in intent.

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`, `--test-threads=1`):

- `cargo test --features server --test team_collaboration_m006_trajectory` — 13 passed (new harness).
- `cargo test --features server --test team_collaboration_m001_http_auth` — 10 passed (incl. repaired test).
- `cargo test --test identity_m003_daemon_authorization` — 9 passed.
- `cargo test --test presence_m003_observation` — 11 passed.
- `cargo test --test collaboration_m001_chat` — 12 passed.
- `cargo test --features server --test collaboration_m002_chat_policy --test collaboration_m002_chat_tui --test collaboration_m003_chat_actions` — 13 + 11 + 10 passed.
- `cargo test --features server --test team_m003_membership_admin --test session_control_m004_controller_lease --test workspace_m005_selected_project_chat` — 12 + 23 + 9 passed.
- `python3 scripts/check_authorization_matrix.py` — pass (matrix unchanged, all invariants verified).
- `python3 scripts/check_http_route_disposition.py` — pass.
- `python3 scripts/check_project_catalog_invariants.py` — 7/7 pass (layout 65 tracks `migrate_v65`; no new migration).
- `bash scripts/check-core-boundary.sh` — pass; `check_execution_ownership.py` — ok; `check_tui_project_authority.py` — pass (via `verify.sh quick`).
- `cargo fmt --all -- --check` — pass; `git diff --check` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy -p codegg --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings` — pass.
- `scripts/verify.sh quick` — pass (fmt, agent schema, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Substituted verification (documented, not weakened): the plan lists
`cargo clippy --workspace --all-targets --all-features`. Per `AGENTS.md`,
workspace sweeps never use `--all-features` (drags real-server tests);
the two clippy invocations above cover the same code under the supported
feature sets (`verify.sh full` uses
`--features server,plugins,lsp-test-support`). No finding suppressed.
The new harness target carries `required-features = ["server"]` in
`Cargo.toml` (same pattern as the M001 HTTP suite) so default workspace
sweeps skip it exactly like its predecessor.

CI truthfully: only local evidence is claimed here; CI (`verify` job)
will re-run the canonical guards.

## 5. Invariant review

- No REST/Core authorization divergence: parity test pins identical
  allow/deny on both surfaces for reads and enumeration; disposition
  guard green.
- No chat-to-execution escalation: Viewer/Contributor chat grants never
  confer `TurnSubmit` or structured-action capabilities (pinned with
  zero-side-effect assertions).
- No implicit shared-turn control: second-Contributor steer/cancel
  denied with lease untouched; handoff only via explicit CAS transfer;
  same-principal second device still works (client-independent
  principal control, per ADR-0007).
- No cross-project Workspace/chat routing: dashboard rows filtered by
  membership, revocation clears rows, per-project chat histories and
  drafts never cross under rapid selection flapping (daemon + TUI).
- Immediate revocation at new request boundaries: chat, reads, control,
  and dashboard all deny post-revoke without reconnect; restart
  preserves revocation; reconnect re-evaluates (pinned).
- Secret hygiene: one-time plaintext never reappears in any list,
  history, sync, composing, control, or membership view (census pinned).

## 6. Failure and recovery review

- Disconnect between request and response: revocation + client
  re-registration test proves reconnect converges to deny; duplicate
  idempotency keys converge without double effects (chat seq, control
  revision pinned).
- Duplicate/stale requests: CAS conflicts (`team_revision_conflict`,
  `chat_revision_conflict`, stale transfer revision) change nothing.
- Controller transfer vs turn completion: terminal-wins and
  stale-transfer `NotFound` semantics are M004-pinned and untouched;
  M006 adds the live-handoff success path plus ineligible-recipient
  negative.
- Policy change vs chat send: CAS race test proves the winner stands
  and the loser conflicts; reconnect/restart re-evaluate policy.
- Membership revoke vs event delivery: revoked principals hold no event
  stream (LocalOwner-only global surface pinned) and lose control/chat
  at the next boundary.
- Workspace selection vs async completions: M005 stale-generation
  guards untouched and green; M006 adds rapid-flap routing proof.

## 7. Migration and compatibility review

No migration (layout stays 65). Pre-corrective-shaped databases keep
role-default chat compatibility (Viewer deny, Contributor allow pinned).
LocalOwner solo mode works end to end with no team rows. Rollback drops
the harness binary target: no production behavior depends on it. The
M001 test repair is forward-compatible (turn-scoped pending items are
the production shape since M004).

## 8. Security review

Adversarial matrix executed: outsider enumeration empty + 404 at every
entry point; cross-project reads/mutations/admin/policy/control probes
fail closed with zero side effects (membership-row and chat-history
counts pinned before/after); restricted-channel denial
indistinguishable from absence; structured-action and turn-submit
negatives for chat-granted Viewers; controller-transfer negatives
(non-controller, stale, ineligible); revocation negatives across
chat/read/control/dashboard/event surfaces. Secret census green (see
§4/G). No new network, crypto, or secret-handling primitive was added.
`eggsentry`/adversarial suites were not re-run beyond the focused
authorization suites; no new exfiltration surface exists (harness-only
change).

## 9. Documentation and operations

- `architecture/collaboration.md`: M006 qualification section to be
  added only if it documents harness facts (no production semantic
  changed, so no architecture correction is owed; see note below).
- Operational note: after an ambiguous transport failure on a chat
  send or control request, retry with the same idempotency key (chat)
  or refresh revision first (`SessionControlGet`, `ChatPolicyGet`,
  membership list); revocation applies at the next request boundary;
  recovery takeover after upgrade uses revision 0 with a bounded reason
  (M004 operational note, unchanged).

Note: no `architecture/` doc required factual correction — every
production behavior the harness pins was already documented by
M001–M005. The roadmap status table (§11) is the documentation update.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. One explicit non-claim:
`cargo test` with `lsp-real-server-tests` / `--all-features` was not
run per `AGENTS.md` workspace policy (never in default sweeps); the
supported feature sets above are the canonical substitute used by every
prior closure in this campaign.

## 11. Roadmap disposition

M006 was the final cross-cutting qualification gate (hard dependency
M001–M005, all closed). With M006 now closed, the corrective campaign
is complete: a Viewer can be granted one project/channel chat surface
without write authority; a Contributor can be denied chat without losing
unrelated capabilities; membership is administered through CodeGG;
active shared turns have explicit controller ownership; Workspace
provides selected-project chat alongside the ordinary composer; and no
authenticated network compatibility route bypasses canonical
authorization. The subsystem roadmap moves to `closed` with M006 marked
closed.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/006-multi-user-trajectory-security-qualification.md`:
  `ready` → `implemented` (closure at this record; harness-only, no
  production delta).
- `plans/registry.md`: M006 `ready` → `closed` (closure at this
  record); subsystem row `active` → `closed` (M001–M006 all closed;
  final gate satisfied); gate paragraph + blocked-work + recently-closed
  rows updated.
- Subsystem roadmap
  `plans/subsystems/team-collaboration-corrective-addendum.md`: Status
  `active` → closed; M006 `ready` → closed in the milestone table.

Dependency audit: M006 lists M001–M005 as hard dependencies (all
closed; consumes accepted ADR-0006/ADR-0007 DTOs) — satisfied. No other
registered plan lists M006 (or any team-collaboration milestone) as a
hard/interface dependency: the registry Blocked-work section contains
only dependency-security M005 (external updater interface),
architecture-convergence M009 (compatible-host evidence), and runtime
C002 (Landlock evidence), none of which is unblocked by this closure —
all stay `blocked` with unchanged blockers. No corrective follow-up is
registered: the one discrepancy found (stale M001 expectation) was
repaired test-only in this same commit and needs no further track.
