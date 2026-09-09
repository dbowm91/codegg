# Identity, Authorization, and Audit Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-authorization-audit/003-daemon-authorization-and-attribution.md`

Source subsystem roadmap:

- `plans/subsystems/identity-authorization-audit-roadmap.md#M003--daemon-authorization-and-originating-principal-attribution`

Repository baseline reviewed: `439f0c3d653e91793af35fb0a5f62a27ed3faaad`

Implementation commits:

- `439f0c3d` — M003 daemon authorization and attribution, migration v53, executable request/capability matrix, revocation/privacy/provider/child matrices, restart/attribution tests, authorization architecture docs.

## 1. Executive finding

M003 is closed. Every native project-scoped operation now receives a
canonical server-side capability decision built only from the
transport-bound principal plus request locators, enforced at the daemon
boundary before any side effect. Project enumeration is privacy-filtered
and single-project denials are indistinguishable from absence. Durable
sessions, turns, jobs, and provider selections capture immutable
originating-principal attribution; child/tool/provider authority can only
narrow; `LocalOwner` decides through the same policy API under an
explicit broad local policy. No unresolved high, medium, or low M003
finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory every native `CoreRequest` with scope + capability (work package A) | `crates/codegg-core/src/authorization.rs::operation_descriptor`: exhaustive match over 135 variants, no wildcard; `operation_capability_matrix()` derived from representative requests; `scripts/check_authorization_matrix.py` coverage check | pass | Adding a `CoreRequest` variant is a compile error until classified; guard asserts every protocol variant appears. |
| Centralized authorization service over M001 state with LocalOwner policy and structured denials (work package B) | `AuthorizationService::authorize` over `TeamStore`; `AuthorizationRequest` (principal + descriptor + resolved project + correlation, no payload-authority constructor); `AuthorizationDecision` (policy, membership revision, decision id); `AuthorizationError` codes `authorization_denied`/`authorization_scope_required`/`authorization_scope_ambiguous`/`authorization_principal_inactive`/`authorization_unavailable` | pass | `LocalOwner` receives `PolicyKind::LocalOwnerBroad` through `authorize`, never around it. |
| Daemon boundary + projection/project-listing integration; DTOs cannot bypass context (work package C) | `CoreDaemon::authorize_request` + gate in `handle_request_with_client` (denial replies before dispatch); `resolve_authorization_project` (direct/session-row/job-session/projection-scope); `filter_projects_for_principal` in `ProjectList`; `denial_as_not_found` in `ProjectGet`; `team_capabilities_to_projection` + `bounded_resolver_for_principal` projection adapter | pass | `check_no_role_checks_in_daemon_dispatch` guard pins capability-only dispatch. |
| Origin principal into sessions/turns/runs/jobs/provider/tool receipts and child-authority narrowing (work package D) | `OriginAttribution::from_authority` + `OriginAttributionStore` (migration v53 `origin_attribution`, first-write-wins); capture in session-create x3, turn-start, job-submit, rerun-child, provider-selection arms; `ToolExecutionContext::origin_principal/auth_method/decision_id` + `apply_origin`; `narrow_authority`/`authorize_child_delegation`; `authorize_provider_use` | pass | Run-row attribution joins via turn attribution; agent-loop stamping of tool receipts is M005 instrumentation input (see §10). |
| Membership-removal/concurrent-request and privacy matrices; docs reconciled (work package E) | Revocation-race, role allow/deny, enumeration-privacy, provider-scope, child-escalation, spoof, restart tests (unit + `tests/identity_m003_daemon_authorization.rs`); `architecture/authorization.md` (full 135-row matrix); `architecture/identity.md` M003 section; both static guards | pass | Opaque-scope fail-closed rule documented for surfaces without project linkage. |
| Complete request/capability table coverage test | `operation_matrix_has_no_duplicate_operations` (135 rows, unique), `every_representative_request_maps_to_a_named_operation`, guard coverage check | pass | Compile-time exhaustiveness plus executable matrix. |
| Viewer/Contributor/Maintainer/Owner allow/deny matrix | `role_allow_deny_matrix_matches_m001_expansion` (13 role×capability cases) + `owner_observer_control_matrix_at_session_scope` (observer reads, never invokes) | pass | Matches M001 expansion sizes 6/14/19/21. |
| Project enumeration privacy | `enumeration_is_privacy_filtered` + `daemon_project_privacy_list_filters_and_get_hides_existence` (member sees 1/2, outsider sees 0, get-denial equals absent-project code) | pass | `denial_as_not_found` pinned secret/existence-free by guard. |
| Session owner vs observer/control | Observer (Viewer) `session.read` allow + `agent.invoke` deny; controller (Contributor) both allow; boundary `TurnSubmit` denies Viewer with zero side effect | pass | Structural: every turn re-enters the gate on the session project. |
| Provider scope | `provider_scope_checks_enforce_ownership` + `deployment_scope_owner_grant_allows` + boundary `child_and_provider_authority_cannot_widen` (personal owner-only, project grant-checked, deployment owner-gated, LocalOwner composes) | pass | — |
| Membership removal race | `membership_removal_race_fails_new_authorization` + `membership_revocation_takes_effect_at_boundary` (decision binds revision; post-revoke authorize denies; stale re-grant conflicts; boundary denies) | pass | Concurrent second attribution writer cannot rewrite origin (first-write-wins test). |
| Child authority escalation negative | `child_authority_narrows_and_escalation_fails` (`member.manage` outside Contributor parent denies; intersection narrows) | pass | — |
| Spoofed payload principal | `request_dtos_carry_no_authority` (wire shape has no principal/role/capability keys) + `spoofed_payload_project_cannot_grant_authority` (attacker denied for victim project) | pass | No constructor accepts caller-supplied authority by construction. |
| Restart/migration attribution | `attribution_survives_restart` + `restart_preserves_membership_and_attribution` (file-DB close/reopen/remigrate preserves memberships, revisions, decisions, attribution) | pass | Migration v53 additive/idempotent (`CREATE TABLE IF NOT EXISTS` + indexes). |

## 3. Production implementation evidence

- `crates/codegg-core/src/authorization.rs` (new, ~2600 lines with
  tests): operation inventory + `AuthorizationService` + denials +
  `OriginAttribution`/`OriginAttributionStore` + child narrowing +
  provider scope + projection adapter + privacy filter + 19 focused
  tests.
- `crates/codegg-core/src/lib.rs`: `pub mod authorization;`
  (boundary-clean: only `team`, `transport_auth`, `provider_connections`,
  `projection_replay`, `identity`, `error`, `session::schema`, `sqlx`,
  `serde`, `thiserror`, `codegg-protocol` used).
- `crates/codegg-core/src/session/schema.rs`: additive `migrate_v53`
  creating `origin_attribution` (scope-kind CHECK, `(scope_kind,
  scope_id)` primary key, principal/policy/decision/correlation columns,
  JSON body, principal + decision indexes); wired into the version
  dispatcher (`52 → 53`).
- `crates/codegg-core/src/storage/mod.rs`: `STORAGE_LAYOUT_VERSION` 52 → 53.
- `src/core/daemon.rs`: `authorize_request`,
  `resolve_authorization_project`, `session_id_for_request`,
  `session_project`, `job_session_project`, `authorization_denial`,
  `filter_projects_for_principal`, `record_origin_with_decision`; gate at
  the top of `handle_request_with_client` (boxed preamble future);
  enumeration filtering in `ProjectList`; attribution capture in
  session-create (x3), turn-start, job-submit, rerun-child, and
  provider-selection arms.
- `src/tool/backend.rs`: `origin_principal` / `origin_auth_method` /
  `origin_decision_id` on `ToolExecutionContext` plus `apply_origin` /
  `origin_principal_id`; `principal_identity` retained as the
  compatibility projection. Call-site initializers updated
  (`broker.rs`, `agent/worker.rs` via `tool_batch.rs`, four tool-program
  fixtures).
- `scripts/check_authorization_matrix.py` (new, 5/5 green): matrix
  coverage, capability-only dispatch, denial-shape, additive migration,
  doc presence.
- `scripts/check_project_catalog_invariants.py`: expected
  `STORAGE_LAYOUT_VERSION` 52 → 53.
- Docs: `architecture/authorization.md` (decision flow, LocalOwner
  composition, privacy, scopes, attribution, narrowing, provider scope,
  projection adapter, failure/restart, full 135-row matrix, verification);
  `architecture/identity.md` M003 section.
- `tests/identity_m003_daemon_authorization.rs` (new, 9 boundary tests,
  no required-features gate): matrix breadth/duplicates, DTO spoof
  negative, observer/controller matrix, gate denial with zero side
  effect, privacy list/get, session attribution capture, revocation at
  the boundary, child/provider non-widening, restart preservation.

Distinguished as absent (downstream milestones, not M003 scope): audit
persistence and instrumentation (M004/M005), presence/chat, OIDC, node
authorization, project linkage for opaque run/job/schedule/worktree
listings (fail closed by design, see §10), agent-loop stamping of tool
receipts from turn attribution (M005 input).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib authorization
cargo test --test identity_m003_daemon_authorization
cargo test -p codegg-core --lib
cargo test --test storage_migrations
cargo test --lib
cargo test --test tool_program_m015_authority_contract --test tool_program_m015_daemon_failpoints --test tool_program_m016_notification_replay --test tool_program_m017_notification_confirmation
cargo test --features server --test identity_m002_transport_auth
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
bash scripts/verify.sh quick
```

The plan's literal `cargo test --workspace authorization|projection|agent`
spell non-existent workspace crates; the narrowest-owner invocations
above (`-p codegg-core authorization`, the new boundary suite, full
`--lib` suites) are the justified substitutes and are recorded here
without concealment.

### Results

- `cargo test -p codegg-core --lib authorization`: pass, 21 passed
  (matrix breadth/uniqueness/spot-checks, role matrix, LocalOwner broad
  policy, missing-scope negative, enumeration activeness, spoof
  negative, revocation race, disabled principal, child narrowing +
  escalation negative, personal/project/deployment provider scope,
  privacy filter, projection mapping, attribution round-trip +
  first-write-wins + scope validation, legacy provenance, restart,
  secret-negative errors).
- `cargo test --test identity_m003_daemon_authorization`: pass, 9
  passed (matrix, DTO authority absence + spoof, observer/controller,
  gate denial + zero side effect + granted-principal pass-through,
  privacy list/get, session attribution capture with `local-owner`
  origin, revocation at boundary, child/provider non-widening,
  file-DB restart).
- `cargo test -p codegg-core --lib`: pass, 579 passed, 0 failed.
- `cargo test --test storage_migrations`: pass, 4 passed.
- `cargo test --lib`: pass, 4342 passed, 0 failed.
- Tool-program touched suites: pass, 5 + 8 + 2 + 2 passed
  (authority-contract, daemon-failpoints, notification-replay,
  notification-confirmation).
- `cargo test --features server --test identity_m002_transport_auth`:
  pass, 7 passed (no M002 regression).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, 0 warnings (one M003 test-only lint,
  `cloned_ref_to_slice_refs`, found during development and fixed before
  the final run).
- `python3 scripts/check_authorization_matrix.py --verbose`: pass, 5/5.
- `python3 scripts/check_project_catalog_invariants.py --verbose`: pass,
  7/7 (including `STORAGE_LAYOUT_VERSION is 53`).
- `bash scripts/check-core-boundary.sh`: pass.
- `bash scripts/verify.sh quick`: pass (`==> Quick verification passed.`).

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

Implementation note (recorded, not concealed): the new boundary
integration suite initially overflowed the debug test-thread stack
because unoptimized test futures inline the giant dispatch machine once
per daemon await. Dispatch call sites in the test file box the future
(`Box::pin(daemon.handle_request…)`); production dispatch is unchanged
apart from the boxed authorization preamble, and all in-lib daemon
tests (31/31) pass unmodified.

## 5. Invariant review

- Authorization occurs server-side: the gate precedes dispatch; the
  request type has no authority constructor; the DTO wire shape carries
  no principal/role/capability keys (tested).
- Project enumeration is protected: listings filter to `project.read`
  grants; outsiders observe empty lists, never errors that confirm
  absence versus denial.
- Request payload cannot grant capability: spoof test denies an attacker
  naming the victim project; role expansion lives only in `team.rs`
  (guard rejects role interpretation in daemon dispatch).
- LocalOwner uses the same API: broad local policy decided through
  `AuthorizationService::authorize` with an explicit policy marker and
  decision id; legacy pool-less daemons decide identically.
- Agent effective authority only narrows: contract intersection plus
  per-turn gate on the session project; escalation fails closed.
- Membership revocation is race safe: revision-bound decisions, stale
  re-grant conflicts, first-write-wins attribution, post-revoke
  authorization denies.
- Structured denials leak nothing: operation + capability names only;
  `ProjectGet` denial is byte-shape-identical to absence (guard-pinned).

## 6. Failure and recovery review

- Denied requests have zero side effect: gate replies before the
  dispatch match (boundary test asserts denial code plus absent
  attribution).
- Revocation races: new authorization re-reads membership rows, so a
  revoked grant denies immediately; in-flight work keeps its immutable
  origin while continuation consumes captured policy/revision.
- Restart: memberships, revocations, and attribution rows persist across
  file-DB close/reopen/remigrate; sequence/revision state reconverges
  (`ensure_local_owner` idempotent; v53 remigration safe).
- Storage failure: `OriginAttributionStore` errors warn without failing
  the operation (explicit M004 hardening point, §10); authorization
  store failures surface `authorization_unavailable`, never anonymous
  success.
- Malformed input: unknown project/session/job ids resolve to `None`
  scope and fail closed for team principals; malformed attribution
  scopes are rejected with typed storage errors.
- Bounded behavior: decision ids are UUIDs, attribution JSON is a fixed
  small struct, enumeration filtering is linear in the listed set, no
  new unbounded retention.

## 7. Migration and compatibility review

- Additive migration v53 (`CREATE TABLE IF NOT EXISTS` + `IF NOT
  EXISTS` indexes) inside the existing transactional
  `migrate_and_record` harness; forward and restart safe.
- `STORAGE_LAYOUT_VERSION` 52 → 53. No existing table altered; no data
  backfill; no rollback beyond the standard SQLite restore story.
- Existing records with absent principal are never silently
  fabricated: `OriginAttribution::legacy_local` marks them explicitly;
  `is_legacy()` distinguishes them from team grants.
- Projection-local principal wrappers remain adapters: canonical IDs
  own semantics; `local-user`/`internal-test`/`authenticated-remote`
  stay compatibility-classified for historical contexts.
- Typed denial codes (`authorization_*`) travel in the existing
  `CoreResponse::Error { code, message }` shape, so older clients
  degrade to generic error display without a wire break.
- `ToolExecutionContext` additions are `Option` fields defaulting to
  `None`; existing receipts and fixtures are unaffected.

## 8. Security review

- Fail-closed defaults on every path: missing/ambiguous scope denies
  team principals; unknown/disabled principals deny; opaque operations
  (filesystem locators, credential-adjacent connection operations,
  unlinkable run/job/schedule listings) deny remote team use while
  `LocalOwner` personal-local flows are unchanged.
- Denial messages contain no secret material and no existence signal
  (serialization scan test + guard).
- Constant-time token handling unchanged from M002; digests still
  omitted from `Debug`; attribution JSON carries identity metadata
  only.
- Privilege boundaries: only `Active` principals authorize; only
  `Active` memberships grant; `LocalOwner` broad policy is an explicit
  composition visible in every decision; bootstrap compat still maps to
  `LocalOwner` only.
- Denial-of-service bounds: bounded scope ids (128), bounded labels,
  indexed attribution lookups, boxed authorization preamble so the
  dispatch future does not grow the hot-path stack frame unboundedly,
  no new network or spawn surface (execution-ownership guard green).
- `codegg-core` boundary guard passes; no UI/server/plugin/auth imports
  or dependencies added (`codegg-protocol` was already a dependency).

## 9. Documentation and operations

Updated:

- `architecture/authorization.md` — decision flow, LocalOwner
  composition, privacy, scope kinds, attribution, narrowing, provider
  scope, projection adapter, failure/restart, full 135-row executable
  matrix, verification commands.
- `architecture/identity.md` — M003 section pointing at the
  authorization architecture.
- `crates/codegg-core/src/authorization.rs` module docs — design notes
  and the M004/M005 transport contract.
- `scripts/check_authorization_matrix.py` — executable matrix/ownership
  guard (5/5).
- `scripts/check_project_catalog_invariants.py` — version expectation
  52 → 53.
- `plans/implementation/identity-authorization-audit/003-daemon-authorization-and-attribution.md`
  — marked closed, linking this record.
- `plans/implementation/identity-authorization-audit/004-append-only-audit-foundation.md`
  — unblocked to ready for handoff.
- `plans/implementation/presence-observation/001-project-presence-leases.md`
  — unblocked to ready for handoff.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — M003
  closed, M004 ready.
- `plans/subsystems/presence-observation-roadmap.md` — M001 ready.
- `plans/registry.md` — Identity row advanced to M004 ready; M004 and
  presence-M001 registered dependency-ready; M003/presence-M001 blocker
  rows removed; M003 recorded under closure evidence.

Operator note: after upgrading, existing databases migrate to v53
automatically on next daemon start. Team operators should grant new
members a project role (or register a project, which grants the creator
Owner) before remote personal-token use: project-scoped operations
without a resolvable grant now fail closed for distinct team
principals, while personal-local `LocalOwner` flows are unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | — | — |

There are no unresolved critical, high, medium, or low M003 findings.
The following are explicit non-claims with named consumers, not
defects:

- Opaque-scope listings (run/job/schedule/managed-worktree stores)
  fail closed for remote team principals until project linkage lands;
  consumer: future project-linkage follow-up, not M004/M005.
- Agent-loop stamping of `ToolExecutionContext` origin fields from turn
  attribution; consumer: M005 audit instrumentation (tool-receipt
  chain).
- Run-row origin joins via turn attribution rather than a dedicated
  run column; consumer: M005 end-to-end chains.
- Attribution write failures warn instead of failing the operation;
  consumer: M004 failure/backpressure policy.

## 11. Roadmap disposition

Milestone closed and two downstream dependencies may proceed. The
registry audit found exactly two registered plans whose sole hard
blocker was M003 closure:

- Identity M004 (append-only audit foundation), blocked on M003 →
  `ready` in the same commit. M005 remains blocked on M004.
- Presence M001 (project-scoped presence leases), blocked on identity
  M003 → `ready` in the same commit. Presence M002 remains blocked on
  presence M001; presence M003 on M001/M002; collaboration M001 remains
  blocked on identity M005 + presence M003.

No corrective pass is required and no new dependency-ready plan was
created beyond the two unblocks.

## 12. Registry updates

Included in the closure commit alongside this record:

- M003 source plan marked closed, linking this record.
- Roadmap milestone table: M003 `closed` with closure link; M004
  `blocked` → `ready`.
- M004 implementation plan: `blocked` → `ready for handoff` (sole
  blocker M003 now closed; actor/decision/attribution semantics are
  canonical in `codegg_core::authorization`).
- Presence M001 implementation plan: `blocked` → `ready for handoff`
  (sole blocker identity M003 now closed; team membership,
  transport-bound principals, and project authorization it consumes are
  landed and tested).
- Presence roadmap milestone table: M001 `blocked` → `ready`.
- Registry active-subsystem row: Identity current milestone M003 ready
  → M004 ready; Presence row notes M001 ready.
- Registry dependency-ready table: M003 row replaced by the M004 row
  (audit foundation; M003 closure is the satisfied dependency); presence
  M001 row added (presence leases; identity M003 closure is the
  satisfied dependency).
- Registry blocked-work table: identity M004 and presence M001 rows
  removed; M005, presence M002/M003, and collaboration rows retained
  unchanged.
- Registry closure-evidence table: Identity M003 row added pointing at
  this record.
