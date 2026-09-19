# Team Collaboration Post-Closure Corrective M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-post-closure-corrective/001-registration-authority-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m001--daemon-local-registration-and-project-bootstrap-authority`

Repository baseline reviewed: `02971be8`

Implementation commits or pull requests:

- `02971be8` — feat(security): local-owner-only daemon registration authority (team-collaboration post-closure M001)

## 1. Executive finding

M001 is complete. Raw daemon-local workspace/project bootstrap is
LocalOwner/proven-local deployment authority again: ordinary team
principals (Owner/Contributor/Viewer on an unrelated project, plus an
outsider with no grants) fail closed with zero filesystem/catalog/
membership side effects through both Core and HTTP. Global workspace
enumeration discloses no canonical roots to team principals. LocalOwner
personal-local bootstrap and project-scoped `POST /api/workspace`
mutation remain functional. `POST /api/project` is `LocalOwnerOnly`
compatibility and cannot widen Core authority. No team project-creation
capability was invented. `M003` remains blocked on `M002`.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence | Result | Notes |
|---|---|---|---|
| A. `WorkspaceRegister` LocalOwner/proven-local only via canonical Opaque/LocalOwner pattern | `crates/codegg-core/src/authorization/policy.rs`: `workspace_register` → `opaque` + `project.configure`; `operation_descriptor` exhaustive match, no wildcard | pass | Team resolves to `MissingScope`; LocalOwner broad policy passes through the same `AuthorizationService` API |
| A. `WorkspaceList` global form LocalOwner/proven-local only, no root disclosure | `policy.rs`: `workspace_list` → `opaque` + `project.read`; daemon gate denies before `daemon_projects` lists | pass | Until a project-filtered enumeration contract exists; project-filtered `ProjectList`/`WorkspaceDashboard` unchanged |
| A. `ProjectRegister` LocalOwner/proven-local only; `workspace_id` locator never proves ownership | `policy.rs`: `project_register` → `opaque` + `project.configure`; `resolve_authorization_project` returns `None` for these DTOs so team fails with `MissingScope` | pass | No durable workspace-ownership relation was found; safe default kept per stop conditions |
| B. `POST /api/project` → explicit LocalOwner-only disposition, same canonical policy | `src/server/authz.rs`: `POST /api/project` → `LocalOwnerOnly` (`project_register`/`none`) | pass | `every_shared_authz_row_names_a_capability` now green without weakening the test |
| B. HTTP authorization precedes `create_dir_all`/workspace/catalog/membership; same bounded denial whether or not the path exists | `src/server/routes/project.rs::create_project`: `require_local_owner(.., "project_register")` first; team denial is `local_owner_denial()` (`not_found`) for both missing and existing paths | pass | Pinned by `http_post_project_denied_before_side_effects` (missing + existing probes) |
| C. Remove stale "creator receives Owner" branch; LocalOwner needs no membership row | `project.rs`: non-LocalOwner `create_membership(Owner)` branch deleted; comment states future team creation needs a separately reviewed atomic contract | pass | No dead/unreachable bootstrap policy retained |
| D. Local picker (`WorkspaceRegister` → `ProjectRegister`) continues for implicit/proven LocalOwner; team gets bounded denial, no `cwd` inference | `src/core/daemon.rs::request_authority_for_client`: unregistered/in-process/stdio clients resolve to `LocalOwner`; TUI `project_picker.rs` surfaces `CoreResponse::Error { code, message }` as picker error toast/phase; no `current_dir()` reintroduced | pass | Verified by `local_owner_core_bootstrap_succeeds`, `http_post_project_local_owner_succeeds`, `tui_project_picker` suite |
| E. Focused post-closure regression target covering Core + HTTP; extend M001 HTTP suite where appropriate | New `tests/team_collaboration_postclosure_m001_registration_auth.rs` (10 tests); existing `team_collaboration_m001_http_auth`, `team_collaboration_m006_trajectory` rerun unchanged | pass | Target names match plan §10 (`team_collaboration_postclosure_m001_registration_auth`) |
| E. Docs: `authorization.md`, `server.md`, `workspace.md`/`core.md`, `tui.md`, skills | Updated (see §9) | pass | `POST /api/workspace` scoped semantics explicitly preserved |
| No new `project.create` capability, workspace ACLs, invitations/OIDC/admin roles, identity changes, or `POST /api/workspace` semantic changes | No new Core variants, capabilities, migrations, or ACL tables; `POST /api/workspace` handler untouched except docs | pass | Out-of-scope list from plan §5 verified by `git diff --stat` |

## 3. Production implementation evidence

- `crates/codegg-core/src/authorization/policy.rs`: `workspace_register`
  (`opaque`/`project.configure`), `workspace_list`
  (`opaque`/`project.read`), `project_register`
  (`opaque`/`project.configure`) with comments stating raw
  registration/global enumeration are deployment authority and a
  `workspace_id` never proves ownership. Operation names stable for
  audit/compatibility; no second authorization engine, no payload
  `local=true` boolean.
- `crates/codegg-core/src/authorization.rs`: matrix spot-checks updated
  to `opaque`/`project.configure` (`project_register`,
  `workspace_register`) and `opaque`/`project.read` (`workspace_list`).
- `src/server/authz.rs`: `POST /api/project` disposition
  `SharedAuthz + none` → `LocalOwnerOnly + none`.
- `src/server/routes/project.rs::create_project`:
  `authorize_enumeration` → `require_local_owner(.., "project_register")`
  before absolute-path validation, `create_dir_all`, workspace
  registration, project registration; deleted the non-LocalOwner Owner
  grant (with comment pointing future team creation to a separately
  reviewed atomic contract); removed now-unused `TeamStore`/`ProjectRole`
  imports.
- `Cargo.toml`: registered
  `team_collaboration_postclosure_m001_registration_auth` with
  `required-features = ["server"]` so default workspace sweeps stay clean.
- Tests: new `tests/team_collaboration_postclosure_m001_registration_auth.rs`
  (see §4); no wire variants removed; old clients receive a normal
  authorization error.
- What was deliberately not built: no `project.create` capability, no
  workspace ACL/ownership tables, no invitation/org/OIDC/deployment-admin
  roles, no team project-creation path, no `ProjectId`/`WorkspaceId`
  identity changes.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --features server --test team_collaboration_postclosure_m001_registration_auth -- --test-threads=1
cargo test --features server --test team_collaboration_m001_http_auth -- --test-threads=1
cargo test --features server --test team_collaboration_m006_trajectory -- --test-threads=1
cargo test --test tui_project_picker -- --test-threads=1
cargo test -p codegg --features server --lib server::authz::tests -- --test-threads=1
cargo test -p codegg-core --lib authorization -- --test-threads=1
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_http_route_disposition.py --verbose
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
cargo test --test workspace_m005_selected_project_chat -- --test-threads=1
cargo test --test tui_project_routing -- --test-threads=1
```

### Results

- `cargo fmt --all -- --check`: pass.
- New `team_collaboration_postclosure_m001_registration_auth`: 10/10 pass
  (`core_descriptors_are_local_owner_only`,
  `team_workspace_register_denied_with_zero_side_effect`,
  `team_workspace_list_denied_without_root_disclosure`,
  `local_owner_core_bootstrap_succeeds`,
  `team_project_register_denied_with_zero_side_effect`,
  `http_post_project_denied_before_side_effects`,
  `http_post_project_local_owner_succeeds`,
  `http_post_workspace_scoped_mutation_unchanged`,
  `route_disposition_leaves_no_shared_authz_without_capability`,
  `team_project_picker_lists_only_granted_projects`).
- Existing `team_collaboration_m001_http_auth`: 10/10 pass.
- Existing `team_collaboration_m006_trajectory`: 13/13 pass.
- `tui_project_picker`: 22/22 pass.
- `server::authz::tests`: 5/5 pass, including
  `every_shared_authz_row_names_a_capability` (was red on baseline with
  `POST /api/project` as `SharedAuthz + none`, now green unchanged).
- `codegg-core authorization`: 21/21 pass.
- `check_authorization_matrix.py --verbose`: 5/5 pass.
- `check_http_route_disposition.py --verbose`: 4/4 pass.
- `check-core-boundary.sh`: pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, TUI authority, `cargo check` workspace).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `git diff --check`: pass.
- Extra compatibility: `workspace_m005_selected_project_chat` 9/9 pass;
  `tui_project_routing` 27/27 pass.

Exploit/regression reproduction (baseline `0251ba31`):

- `server::authz::tests::every_shared_authz_row_names_a_capability`
  failed on baseline: `shared_authz route must name a capability: POST
  /api/project` (`left: "none"`). This is the checked-in proof that the
  route disposition contradicted its own invariant.
- Baseline code inspection (recorded in the plan and confirmed before
  editing): `WorkspaceRegister`/`WorkspaceList`/`ProjectRegister` were
  `Global / none` (any active principal passes), `daemon_projects.rs`
  passed caller `root`/`workspace_id` straight to registration/listing,
  and `create_project` gated with `authorize_enumeration` (any active
  principal), then `create_dir_all` + workspace + project + Owner grant.
- The new regression encodes the corrected expectation and passes on the
  fixed tree; the four team principals (Owner/Contributor/Viewer on the
  seed project + outsider) are denied on all three Core ops and on HTTP
  `POST /api/project`, while LocalOwner succeeds.

Zero-side-effect evidence:

- Core `WorkspaceRegister`: denied paths remain nonexistent;
  `daemon.workspaces.list` count unchanged per principal.
- Core `WorkspaceList`: denial carries no canonical root in its debug
  rendering; LocalOwner list still returns rows.
- Core `ProjectRegister` (known + guessed `workspace_id`): project count
  (`ProjectCatalog::list_projects`) and membership count
  (`TeamStore::list_memberships_for_project`) unchanged per probe.
- HTTP `POST /api/project`: missing-path probe remains nonexistent;
  workspace/project/membership counts unchanged per probe; missing-path
  and existing-path probes yield the identical bounded `not_found`
  shape.

Core/HTTP parity:

- Core gate: `AuthorizationService` with `opaque` + capability and no
  project locator → `MissingScope` (`authorization_scope_required`) for
  team, `LocalOwnerBroad` allow for LocalOwner.
- HTTP gate: `require_local_owner(.., "project_register")` through the
  same canonical service (`Global/none` decision for LocalOwner,
  `local_owner_denial()` 404 for team) before any side effect.
- Route table and unit matrix agree: `project_register` is
  `opaque`/`project.configure` in Core and `LocalOwnerOnly`/`none` on
  HTTP; `every_shared_authz_row_names_a_capability` passes unchanged.

LocalOwner compatibility:

- `local_owner_core_bootstrap_succeeds` (Core `WorkspaceRegister` →
  `ProjectRegister` via `handle_request`/LocalOwner) and
  `http_post_project_local_owner_succeeds` (`CREATED`, directory created)
  pass; `team_project_picker_lists_only_granted_projects` preserves
  enumeration filtering; `POST /api/workspace` with valid
  `project.configure` still returns `CREATED` while outsiders deny.

Direct guard outputs: see Results above; all local truth (no CI run in
this environment).

Explicit statement: no team project-creation capability was invented. No
new Core variant, capability, migration, ACL/ownership table,
invitation/OIDC/deployment-admin role, or `POST /api/workspace`
semantic change landed. The deleted HTTP Owner-grant branch was not
preserved as dead policy.

## 5. Invariant review

- Authentication is not authorization: team bearers authenticate but fail
  at the daemon/HTTP gates; payloads supply locators only (no new
  principal/role/capability fields; `check_http_route_disposition`
  `check_no_payload_authority` passes).
- Caller-supplied paths never create project authority for an ordinary
  team principal: Core and HTTP gates precede `create_dir_all`/
  workspace/catalog/membership; tests assert counts and nonexistence.
- Team denials remain privacy-safe and occur before side effects: HTTP
  denials are bounded `not_found`; Core denials are typed
  `authorization_scope_required` with no root/project leakage.
- Raw daemon-local path registration requires LocalOwner/proven-local
  authority: `opaque` descriptors + `require_local_owner`; unregistered
  transports resolve to `LocalOwner` so personal-local startup stays
  login-free.
- Global workspace enumeration cannot disclose canonical roots to team:
  `WorkspaceList` denied; denial rendering asserted to contain no root.
- A `workspace_id` is a locator, not ownership proof: team
  `ProjectRegister` against the known seed workspace denies; guessed IDs
  deny or fail input validation with zero creation.
- LocalOwner personal-local bootstrap remains functional: Core + HTTP
  LocalOwner tests pass.
- `POST /api/workspace` remains project-scoped under
  `project.configure`: `http_post_workspace_scoped_mutation_unchanged`
  passes; handler untouched.
- No frontend-selected state or request payload supplies authority:
  TUI picker surfaces the daemon denial as a picker error; no `cwd`
  inference reintroduced (`check_tui_project_authority` passes via
  `verify.sh quick`).

## 6. Failure and recovery review

- Denied requests produce no mkdir, workspace row, project row,
  membership row, event, or service activation (asserted per probe).
- LocalOwner workspace-then-project partial failure follows existing
  catalog behavior; no cross-store transaction was added.
- Concurrent LocalOwner duplicate registration keeps existing
  `WorkspaceRegistry`/`ProjectCatalog` convergence semantics (untouched).
- Restart does not weaken authority: workspaces remain durable, but
  global listing/registration stays LocalOwner-only (policy is static,
  no stale membership cache can grant access; revocation is irrelevant
  to these local-only ops).
- Malformed `workspace_id` guesses fail input validation with zero
  creation; unknown IDs never authorize.
- HTTP absolute-path validation still rejects relative paths for
  LocalOwner after the gate (`project_context_required`).

## 7. Migration and compatibility review

- No storage migration (layout version untouched; `STORAGE_LAYOUT_VERSION`
  invariant unaffected).
- Behavioral compatibility change (intentional security correction):
  remote/team callers that accidentally could register/list daemon-local
  workspaces or create projects through global bootstrap are now denied
  with a normal authorization error. No wire variants removed, so old
  clients receive `authorization_scope_required` (Core) or bounded 404
  (HTTP) rather than a deserialization break.
- LocalOwner personal-local flows unchanged.
- Rollback: reverting `02971be8` restores the vulnerable behavior; no
  data migration to unwind.

## 8. Security review

- Authority narrowing only: three `Global/none` operations moved to
  `Opaque` + capability; one `SharedAuthz/none` route moved to
  `LocalOwnerOnly`. No widening anywhere.
- Denial shapes: Core `authorization_scope_required` (no secret, no
  existence oracle beyond already-known operation names); HTTP bounded
  `not_found` identical for missing/existing paths.
- Path handling: absolute-path check retained for LocalOwner;
  `create_dir_all` + `canonicalize` only after the gate; scoped
  `POST /api/workspace` still sanitizes under the authorized root.
- Membership: deleted HTTP-only Owner grant removes the
  path-to-Owner escalation; LocalOwner needs no row; no new grant path
  added.
- Audit: denied Core requests still emit `authorization_denied` terminal
  events through the existing preamble; no secret material in errors
  (existing `authorization_errors_carry_no_secrets` coverage still green).
- DoS bounds: enumeration stays bounded (`MAX_PROJECT_LIST_ITEMS`);
  static guards remain offline/deterministic.

## 9. Documentation and operations

- `architecture/authorization.md`: `project_register`/`workspace_register`
  → `opaque`/`project.configure`, `workspace_list` →
  `opaque`/`project.read`; HTTP convergence notes raw bootstrap as
  LocalOwner-only deployment authority, `workspace_id` never proves
  ownership, team creation unavailable until a safe contract exists,
  `POST /api/project` as `LocalOwnerOnly`, REST cannot widen Core.
- `architecture/server.md`: router map marks `POST /api/project` as
  LocalOwner-only and global register/list as LocalOwner-only with scoped
  `POST /api/workspace` preserved; convergence section documents
  pre-side-effect gating and identical missing/existing denial.
- `architecture/workspace.md`: protocol authority note (raw
  registration/global enumeration LocalOwner-only, `ProjectRegister`
  from `workspace_id` LocalOwner-only, scoped mutation preserved).
- `architecture/core.md`: protocol line notes the three ops are
  LocalOwner/proven-local `opaque` scope.
- `architecture/tui.md`: project-picker transport note (local
  `WorkspaceRegister` → `ProjectRegister` under LocalOwner; team
  transports get the bounded daemon denial as a picker error; no `cwd`
  inference).
- Skills: `.opencode/skills/server/SKILL.md` (`POST /api/project`
  LocalOwner-only; workspace routes scoped vs global), 
  `.opencode/skills/core/SKILL.md` (raw/global/unscoped ops
  LocalOwner-only `opaque` scope). TUI skill carries no authority
  contract and needed no change.
- Operator diagnostics: team `POST /api/project` denials present as
  bounded 404; Core denials as `authorization_scope_required`; no new
  runbooks needed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | — | — | — |

No high/medium/low findings remain for M001. `M002` (Workspace task
cancellation) and `M003` (guard convergence) are separate milestones,
not M001 findings.

## 11. Roadmap disposition

Milestone M001 closed. `M002` (Workspace task-cancellation ownership)
remains `ready` and independent. `M003` (verification-guard
convergence) remains `blocked`: its M001 hard dependency is now
satisfied, but its M002 hard dependency is still outstanding, so it is
not yet dependency-ready. No new ADR or corrective pass is required by
this closure.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: post-closure row now
  reads `M001 closed; M002 ready; M003 blocked (M001 satisfied, needs
  M002)`.
- `plans/registry.md` execution-gate paragraph: M001 marked closed.
- `plans/registry.md` Blocked work: M003 blocker now reads `M001 closed;
  hard dependency remaining on M002 Workspace task cancellation
  ownership; guards must encode corrected semantics.`
- `plans/registry.md` Recently closed: M001 row → `closed`, closure
  record `plans/closure/team-collaboration-post-closure-corrective/001-status.md`,
  implementation `02971be8`.
- `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md`:
  milestone table M001 → `closed`.
- `plans/implementation/team-collaboration-post-closure-corrective/001-registration-authority-boundary.md`:
  status → implemented with closure pointer.
- Unblock audit: no registered plan becomes fully dependency-ready on
  this closure alone. `M003` stays `blocked` (needs `M002`). `M002`
  was already `ready` and is unaffected. No other registry rows list
  post-closure M001 as a hard/interface dependency.
