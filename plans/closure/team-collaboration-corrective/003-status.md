# Team Collaboration Corrective M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/003-team-membership-and-token-administration.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M003`

Repository baseline reviewed: `f7afefb1`

Implementation commits or pull requests:

- `f7afefb1` — feat(team): membership and device-token administration (team-collaboration M003)

## 1. Executive finding

M003 is complete. The existing team identity primitives are now
productized behind canonical Core operations: a project Owner can
inspect and manage memberships and M002 chat overrides inside their
own project through `/team`, while LocalOwner alone can create
principals and issue per-device personal tokens through a secret-safe
surface. Token plaintext is returned exactly once in a copy-once
modal and never enters audit, events, logs, list responses, or chat.
Cross-project administration and deployment-wide token issuance by
project Owners both fail closed. `/collaborators` remains the
ephemeral presence/observe chooser with its behavior pinned unchanged.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence |
|---|---|
| Canonical Core membership operations, `DirectProject + member.manage`; bounded DTOs | `TeamMembershipList/Add/Update/Revoke` (`CoreRequest`), `TeamMembership/TeamMembershipList` (`CoreResponse`), `TeamMembershipDto`; `operation_descriptor` maps all four to `DirectProject + MemberManage`; list bound 200 (`TEAM_MAX_LIST_LIMIT`) |
| LocalOwner-only principal/token operations classified so ordinary team principals fail closed (Opaque, no project-derived widening) | `TeamPrincipalList/Create/StatusSet`, `TeamTokenList/Create/Revoke` mapped to `Opaque + ProjectConfigure`; `AuthorizationService::authorize` returns `MissingScope` for team principals (no resolvable project) while LocalOwner broad policy passes; integration test `project_owner_cannot_issue_global_tokens` pins both denials |
| Token list returns metadata only; create returns plaintext exactly once | `TeamTokenDto` has no digest/plaintext fields; `TeamTokenCreated { token, plaintext }` is the only plaintext carrier; `token_plaintext_absent_from_debug_audit_events_and_chat` asserts list JSON has no plaintext/digest; `TeamTokenCreate` is `is_secret_bearing` (local-only, `secret_operation_remote_denied` remotely) |
| Audit events for principal lifecycle, membership mutation, token create/revoke metadata, chat-policy changes; never token digest/plaintext | `team_membership_add/update/revoke`, `team_principal_create/status_set` → `membership_change` (member/role/revision); `team_token_create/revoke` → `authentication` (method/transport/kind/outcome); post-mutation emit in `daemon_team.rs::after_*`; census test asserts no `cggt_`/plaintext in any stored audit event |
| `/team` becomes a FocusManager-backed project administration surface (members, role/state, chat policy status, add/change/suspend/revoke/reactivate, chat grant/deny/clear); `/collaborators` stays presence | `src/tui/commands/team.rs` (`dispatch_team_command`, `start_show_team`, `apply_team_membership_loaded` with request-id/project/epoch guards, `InfoType::Team` dialog); `BuiltinSlashAction::Team` + `/team` registry entry; `/collaborators` alias `/team` removed (aliases now only `/presence`); presence suite green |
| LocalOwner-only device/principal path with one-time token dialog and explicit copy/close semantics | `TeamTokenCreate` task → `TuiCommand::TeamTokenCreated` → `OneTimeDeviceToken` (redacted `Debug`) → `DeviceSecretDialog` (`Dialog::TeamTokenSecret`); `TeamTokenSecretClose` + dialog-close arm drop the credential; stale routes withhold the credential and toast the reconcile path |
| M002 chat-policy admin integration through `/team` | `/team chat`, `chat-allow/deny/clear [--channel]`, `chat-restrict/inherit` dispatch the existing `ChatPolicyGet/ProjectPolicySet/ChannelPolicySet` operations with revision-safe results; `chat_override_changes_through_team_stay_revision_safe` pins CAS behavior |
| No duplicate identity store | `daemon_team.rs` operates only on `TeamStore` + `PersonalTokenStore` over the daemon pool; no new table, no new migration (storage layout unchanged) |
| Multi-principal end-to-end and revocation/reconnect tests | `tests/team_m003_membership_admin.rs` (12 tests: Owner lifecycle, non-Owner negatives, cross-project negatives, token-issuance negatives, LocalOwner create+issue, secret census, revocation, stale-revision conflict, chat-override CAS, presence separation, matrix classification, event classification) |

Authorization matrix changes: 11 new rows (`team_capabilities` →
`global/none`; `team_membership_list/add/update/revoke` →
`direct_project/member.manage`; `team_principal_list/create/
status_set`, `team_token_list/create/revoke` → `opaque/
project.configure`). No existing row changed capability or scope.

## 3. Production implementation evidence

- Protocol: `crates/codegg-protocol/src/core.rs`
  (`TEAM_CAPABILITY`/`TEAM_PROTOCOL_VERSION`/`TEAM_MAX_LIST_LIMIT`,
  `TeamCapabilitiesDto`, `TeamPrincipalDto`, `TeamMembershipDto`,
  `TeamTokenDto`; 11 requests, 8 responses, 3 events;
  `is_secret_bearing` covers `TeamTokenCreate`; protocol test pins
  create-local-only vs list-remote-safe).
- Authorization: `crates/codegg-core/src/authorization/policy.rs`
  (11 descriptors + 11 representative requests; matrix guard still 5/5).
- Audit: `crates/codegg-core/src/audit_instrumentation.rs` (7 new
  `INSTRUMENTED_OPERATIONS` rows reusing `membership_change` /
  `authentication`; 4 new `UNINSTRUMENTED_OPERATIONS` read rows;
  team mutations skip the pre-side-effect emit and record
  post-mutation with durable ids/revisions).
- Events: `projection_replay/safe_publication.rs` classifies all
  three team events `Safe` (ids/revision only).
- Daemon: `src/core/daemon_team.rs` (new `team` family:
  `is_team_request`/`is_team_mutation`/`handle_team_request` with
  bounded validation, secret-free DTO mapping, privacy-safe error
  codes, post-mutation audit + liveness events);
  `src/core/daemon.rs` (pre-router dispatch, direct-project
  resolution for membership ops, `project_not_found` denial shape
  for membership ops, mutation audit skip);
  `src/core/daemon_family.rs` (`Team` family) + `src/core/mod.rs`
  module registration.
- TUI: `src/tui/commands/team.rs` (dispatch, spawn-and-complete
  tasks, stale route/revision guards, secret modal mounting);
  `src/tui/components/dialogs/device_secret.rs` (redacted-`Debug`
  copy-once dialog); `OneTimeDeviceToken` + `OneTimeBearer::
  new_device_token` transient state; `Dialog::Team` /
  `Dialog::TeamTokenSecret` + `DialogType` round-trip;
  `TuiCommand::Team*` (5 variants) + dispatch arms; `/team`
  registry entry + help entries; `architecture/command.md` 149 → 150.
- Tests: `tests/team_m003_membership_admin.rs` (12 boundary tests,
  see §4); TUI unit tests in `team.rs` (line renderers carry no
  secrets) and `device_secret.rs` (redacted `Debug`, close keys,
  bearer validation); command-registry guards updated for the new
  `/team` command.

Resolver truth (enforced by tests, not prose): Owner in own project
passes; Maintainer/Contributor/Viewer deny as `project_not_found`;
Owner in another project denies as `project_not_found`; project
Owner on principal/token ops denies with the typed Opaque scope
shape; LocalOwner passes everywhere; stale revisions conflict with
zero overwrite; revoked rows conflict on re-add; revocation rejects
new authentications while existing bindings are untouched by the
store itself (callers own the bounded disconnect contract, as
documented on `verify_personal_token`).

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`):

- `cargo test --test team_m003_membership_admin --locked -- --test-threads=1` — 12 passed:
  `owner_can_list_add_update_revoke_membership_in_own_project`,
  `non_owners_cannot_use_member_manage`,
  `owner_cannot_administer_another_project`,
  `project_owner_cannot_issue_global_tokens`,
  `local_owner_creates_principal_and_issues_one_device_token`,
  `token_plaintext_absent_from_debug_audit_events_and_chat`,
  `token_revocation_rejects_new_authentications`,
  `stale_membership_revision_conflicts_without_overwrite`,
  `chat_override_changes_through_team_stay_revision_safe`,
  `collaborators_presence_behavior_remains_unchanged`,
  `authorization_matrix_classifies_team_surface`,
  `team_events_are_safe_and_structural_only`.
- `cargo test -p codegg-core team --locked -- --test-threads=1` — 14 passed (team domain).
- `cargo test -p codegg-core transport_auth --locked -- --test-threads=1` — 15 passed (token lifecycle).
- `cargo test --test identity_m003_daemon_authorization --locked -- --test-threads=1` — 9 passed (no divergence).
- `cargo test --test collaboration_m002_chat_policy --test collaboration_m002_chat_tui --locked -- --test-threads=1` — 13 + 11 passed (M002 intact).
- `cargo test --test presence_m002_collaborators --locked -- --test-threads=1` — 12 passed (presence unchanged).
- `cargo test -p codegg --lib tui:: --locked -- --test-threads=1` — 956 passed (incl. new team/device-secret unit tests and updated command-registry guards).
- `python3 scripts/check_authorization_matrix.py` — 5/5 pass.
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

CI truthfully: only local evidence is claimed here; CI (`verify`
job) will re-run the canonical guards.

## 5. Invariant review

- Project `member.manage` changes membership only inside that
  project: gate resolves the direct project; cross-project calls
  deny as `project_not_found` (pinned).
- Project Owners cannot create deployment-wide credentials:
  principal/token ops are `Opaque`; team principals fail closed
  before any store read (pinned).
- Global principal/token lifecycle is LocalOwner-only in this
  milestone: no other policy passes `Opaque + project.configure`
  (pinned by both gate tests).
- Token plaintext is once-only and secret-safe: single carrier
  variant, local-only transport, redacted transient TUI state,
  census test over audit/events/list/Debug (pinned).
- Membership and principal revision checks are fail-closed: stale
  writers conflict with zero overwrite on both paths (pinned).
- `/collaborators` is ephemeral presence; `/team` is durable
  administration: separate commands, separate dialogs, presence
  suite green, membership detail never renders in the presence
  panel.

## 6. Failure and recovery review

- Stale membership/principal revisions return
  `team_revision_conflict` with zero overwrite (pinned); the TUI
  surfaces the conflict with a reload-and-retry hint.
- Closing the token modal drops the credential (`team_token_secret
  = None` in both the explicit close and the dialog-teardown arm);
  metadata stays listable; rotation is revoke + create.
- Token-create retries never silently mint: each explicit `/team
  token-create` mints one credential; transport failures toast the
  reconcile-first path (`/team tokens <principal>`) and require
  explicit user re-issue. The `idempotency_key` field is
  shape-validated and reserved for a future convergence contract;
  the closure explicitly does not claim convergence retries.
- Membership revocation takes effect at the next authorization
  boundary: the store returns empty capabilities immediately, so
  gate and chat handler rechecks deny; visible admin/chat state
  refreshes on the next `/team` load or reconnect (epoch-guarded).
- Disabled principals fail token issuance (`team_principal_not_found`)
  and fail new verification; existing bindings are unaffected by the
  store (documented caller-owned disconnect contract, unchanged
  from M002 transport auth).

## 7. Migration and compatibility review

No new migration: `daemon_team.rs` reads/writes only the v51
(`principal`, `project_membership`) and v52 (`personal_auth_token`)
tables. Pre-M003 databases open cleanly with existing principals/
memberships/tokens appearing in the new surfaces. Rollback drops
the binary: no new durable state exists to strand. Older clients
keep ordinary chat/presence behavior and simply cannot administer
teams (new variants are additive; unknown variants fail closed at
the serde layer). `/team` behavior changes from collaborator alias
to durable administration; `/collaborators` (`/presence`) remains
the compatibility path for presence.

## 8. Security review

Cross-boundary matrix proven: Owner lifecycle in own project;
Maintainer/Contributor/Viewer `member.manage` negatives;
cross-project Owner negatives (indistinguishable not-found);
project-Owner principal/token negatives (typed Opaque scope
denial, no project oracle); LocalOwner create+issue;
secret census over audit/events/list/Debug; revocation rejects
new authentication; stale-revision conflicts; M002 overlay CAS
through the team path; presence separation. Overrides and tokens
carry ids/labels/revisions only — never message content, digests,
or plaintext. Denials carry no existence signal beyond the
operation/capability names the caller supplied (membership ops) or
the scope requirement (Opaque ops). `eggsentry`/adversarial suites
were not re-run beyond the focused suites; no new network, crypto,
or secret-handling primitive was added (token crypto is the
unchanged M002 `PersonalTokenStore`).

## 9. Documentation and operations

- `architecture/identity.md`: M003 administration section
  (operations, classifications, one-time secret, events/audit,
  TUI surfaces, test commands).
- `architecture/server.md`: operator auth setup now documents the
  `/team` provisioning flow (`principal-create` → `add` →
  `token-create` → Bearer → `token-revoke`).
- `architecture/tui.md`: `/collaborators` alias fixed; new Team
  Administration section (state/flow/surface/LocalOwner flow).
- `architecture/presence.md`: `/team` alias removed; presence vs
  administration boundary stated.
- `architecture/collaboration.md`: M003 administration section +
  test table update.
- `architecture/authorization.md`: team classification section.
- `architecture/protocol.md`: `team.v1` capability section.
- `architecture/command.md`: built-in total 149 → 150.
- User help: `/team` registry description + help entries for
  `/team` (and the narrowed `/collaborators` entry) distinguish
  durable administration from ephemeral presence.
- Operational note: after an ambiguous `token-create` transport
  failure, reconcile with `/team tokens <principal-id>` before
  re-issuing; revocation applies to new authentications and
  existing connections follow the documented caller-owned
  disconnect contract.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. Two explicit
non-claims (not findings): token-create idempotency keys are
shape-validated but do not converge retries (explicit re-issue
contract instead); existing connections after revocation follow
the pre-existing caller-owned disconnect contract.

## 11. Roadmap disposition

M003 closes the team-administration capability. Per the addendum
dependency graph, no new production milestone unblocks on M003
alone: M004 (controller lease) requires only M001 (already ready)
and M005 (Workspace chat) requires M001+M002 (already ready), so
both were already `ready` and are unchanged. M006 still requires
M001–M005 with M004/M005 open, so it stays `blocked`. The
subsystem roadmap stays `active` with M003 marked closed.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/003-team-membership-and-token-administration.md`:
  `ready` → `implemented` (closure at this record;
  implementation `f7afefb1`).
- `plans/registry.md`: M003 `ready` → `closed` (closure at this
  record); M004/M005 stay `ready`; M006 stays `blocked` on
  M001–M005 (M004/M005 still open).
- Subsystem roadmap
  `plans/subsystems/team-collaboration-corrective-addendum.md`:
  M003 `ready` → closed in the milestone table via this closure.

Dependency audit: M004
(`004-shared-session-controller-lease.md`) requires only M001
(closed; consumes accepted ADR-0007) — already `ready`,
unchanged. M005 (`005-workspace-selected-project-chat-view.md`)
requires M001+M002 (both closed; consumes accepted ADR-0006 DTOs)
— already `ready`, unchanged. M006 requires M001–M005 (M004/M005
still open → stays `blocked`). No corrective follow-up is
registered: no new defect was found that M003 itself must fix.
