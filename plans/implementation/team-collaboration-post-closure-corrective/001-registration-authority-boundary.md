# Team Collaboration Post-Closure Corrective Milestone 001 — Registration Authority Boundary

Status: ready for handoff

Repository baseline: `626585a1fad449a637e4828777bfa21d222abea0`

Source roadmap: `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m001--daemon-local-registration-and-project-bootstrap-authority`

Long-term requirements:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`
- `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md`

Applicable ADRs: no new ADR. ADR-0006/0007 are unaffected. If a new team project-creation or workspace-ownership authority is required, stop and register a separate ADR/plan.

Primary class: invariant

## 1. Objective

Remove the post-closure authority escalation in daemon-local workspace/project bootstrap. Ordinary team principals must not be able to register arbitrary daemon-local paths, enumerate global workspace roots, or claim a globally registered workspace as a new project. Preserve the existing LocalOwner/proven-local bootstrap and already-project-scoped workspace mutation.

## 2. Why this milestone is ready

The predecessor identity/authorization, project-catalog, multi-project TUI, and collaboration campaigns are closed. The intended local-filesystem boundary is already documented: raw path registration is a local/proven-local operation, and filesystem-locator operations without safe project scope fail closed for team principals.

No new data model is necessary to restore that invariant. The current `WorkspaceRegistry`, `ProjectCatalog`, `AuthorizationService`, transport-bound principal, and HTTP disposition table are stable.

## 3. Current implementation evidence

- `crates/codegg-core/src/authorization/policy.rs` classifies:
  - `WorkspaceRegister` as `Global / none`;
  - `WorkspaceList` as `Global / none`;
  - `ProjectRegister` as `Global / none` with a comment that any active principal may create a project.
- `src/core/daemon_projects.rs`:
  - passes caller `WorkspaceRegister.root` directly to `WorkspaceRegistry::get_or_register`;
  - returns all registered workspaces and their canonical roots from `WorkspaceList`;
  - accepts `ProjectRegister.workspace_id` and registers a local project without a project-scoped authorization relation.
- `src/server/routes/project.rs::create_project`:
  - authorizes as enumeration/global;
  - accepts an arbitrary absolute daemon-local path;
  - creates it when absent;
  - registers workspace + project;
  - grants the caller Owner when not LocalOwner.
- `src/server/authz.rs` labels `POST /api/project` as `SharedAuthz` with capability `none`, contradicting its own shared-authz invariant test.
- `src/server/routes/workspace.rs::create_workspace` already resolves explicit project context, requires `project.configure`, and sanitizes under the authorized root. It is not the defect and must not be widened or made LocalOwner-only.

The M006 trajectory fixture created projects/workspaces during setup and then tested cross-project behavior. It did not adversarially invoke registration endpoints as Contributor/Viewer/outsider principals, so this path escaped the final qualification.

## 4. Invariants that must not regress

- Authentication is not authorization.
- Caller-supplied paths never create project authority for an ordinary team principal.
- Team denials remain privacy-safe and occur before mkdir/workspace/catalog/membership side effects.
- Raw daemon-local path registration requires LocalOwner/proven-local authority.
- Global workspace enumeration cannot disclose canonical roots to ordinary team principals.
- A `workspace_id` is a locator, not proof that the caller owns or may convert that workspace into a project.
- LocalOwner personal-local bootstrap remains functional.
- `POST /api/workspace` remains project-scoped under `project.configure`.
- No frontend-selected state or request payload may supply principal/role/capability authority.

## 5. Scope

In scope:

- Core authorization descriptors and handlers for `WorkspaceRegister`, `WorkspaceList`, and `ProjectRegister`.
- HTTP `POST /api/project` disposition and handler authority.
- Removal/reconciliation of the now-invalid "creator receives Owner" HTTP bootstrap branch.
- Project picker/local TUI compatibility and truthful error behavior for non-local/team clients.
- Authorization, route-disposition, zero-side-effect, and LocalOwner compatibility tests.
- Architecture/security docs and relevant skill docs if their contracts change.

Out of scope:

- New `project.create` capability.
- Workspace ACLs or workspace ownership tables.
- Team invitations, org groups, OIDC/device login, or deployment-admin roles.
- Making arbitrary remote/team users project creators.
- Changing ProjectId/WorkspaceId identity.
- Changing `POST /api/workspace` scoped mutation semantics.
- General project-catalog refactors.

## 6. Required production changes

### A. Canonical Core authority

Make daemon-local/global registration operations fail closed for ordinary team principals using the existing authorization model.

At minimum:

- `WorkspaceRegister` must be LocalOwner/proven-local only.
- `WorkspaceList` must not return the global registry/canonical roots to ordinary team principals. Until a project-filtered workspace enumeration contract exists, make the global form LocalOwner/proven-local only.
- `ProjectRegister` must not allow an ordinary team principal to turn a globally registered/guessed `workspace_id` into a new project. Without a durable workspace ownership relation, the safe corrective is LocalOwner/proven-local only.

Use the existing Opaque/LocalOwner authorization pattern or an equivalent canonical daemon-side decision. Do not introduce a second authorization engine or a payload boolean such as `local=true`.

If the implementation discovers an existing, durable, daemon-verifiable relationship that safely authorizes team project creation from a known WorkspaceId, stop and document it before widening the safe default. Do not infer ownership from knowledge of the ID.

### B. HTTP compatibility

Change `POST /api/project` from `SharedAuthz + none` to an explicit LocalOwner-only disposition, or route it through the same canonical LocalOwner-only Core operation.

Authorization must happen before:

- `create_dir_all`;
- workspace registration;
- project registration; or
- membership creation.

An ordinary personal-token team caller must receive the same bounded privacy-safe denial regardless of whether the supplied path exists.

### C. Bootstrap parity

Remove stale comments/branches claiming that arbitrary active principals receive Owner on project registration. LocalOwner broad policy does not require a project membership row to operate.

If a non-LocalOwner safe project-creation path is desired later, it must atomically define creator membership and workspace authority under a separately reviewed contract; do not preserve the current HTTP-only Owner grant as dead or unreachable policy.

### D. Local TUI compatibility

The local project picker flow (`WorkspaceRegister` -> `ProjectRegister`) must continue for the implicit/proven LocalOwner principal. Remote/team callers must get a bounded actionable denial rather than silently falling back to local path interpretation.

No process `cwd` inference may be reintroduced.

### E. Tests and docs

Add a focused post-closure regression target that covers Core and HTTP registration authority and extend the M001 HTTP suite where appropriate. Update:

- `architecture/authorization.md`;
- `architecture/server.md`;
- `architecture/workspace.md` and/or `architecture/core.md` where registration scope is described;
- `architecture/tui.md` if project-picker transport behavior needs clarification;
- module skills whose authority contract changes.

## 7. Ordered work packages

### Work package A — Reproduce and pin the exploit

Add tests using LocalOwner, Owner/Contributor/Viewer team principals, and an outsider/principal with no relevant project grant.

Prove on the baseline that an ordinary team principal can reach at least one of the unsafe registration paths. Then encode the corrected expectation:

- team `WorkspaceRegister` denied with zero directory/workspace side effect;
- team `WorkspaceList` denied/no root disclosure;
- team `ProjectRegister` against a known or guessed workspace ID denied with zero project/membership side effect;
- team HTTP `POST /api/project` denied before mkdir/catalog/membership;
- LocalOwner equivalents succeed.

### Work package B — Core policy correction

Move the three global registration/enumeration operations to the canonical local-only/opaque authority shape. Keep operation names stable for audit/compatibility where possible.

Ensure denial happens in the daemon authorization preamble before `daemon_projects` mutates stores or touches filesystem state.

### Work package C — REST convergence

Change route disposition and handler gating for `POST /api/project`. Prefer one shared canonical authority path; if compatibility code remains direct, its decision must use the same LocalOwner policy and be pinned by the route-disposition table.

Delete or make unreachable any non-LocalOwner Owner-grant bootstrap branch that no longer represents product semantics.

### Work package D — Compatibility/docs

Verify local picker, personal-local startup, and existing project-scoped workspace operations. Correct comments and architecture docs so "global" never means "any team principal may manipulate daemon-local filesystem."

## 8. Failure, cancellation, restart, contention semantics

- Denied requests produce no mkdir, workspace row, project row, membership row, event, or service activation.
- A failure after LocalOwner workspace registration but before project registration follows existing catalog behavior; do not add a cross-store transaction unless already supported.
- Concurrent LocalOwner duplicate registration keeps existing WorkspaceRegistry/ProjectCatalog convergence semantics.
- Restart does not weaken authority; registered workspaces remain durable but global listing/registration stays LocalOwner-only.
- Revoking or changing a team membership is irrelevant to these local-only operations; no stale membership cache may grant access.

## 9. Compatibility and migration

No storage migration is expected.

Behavioral compatibility change: remote/team callers that were accidentally able to register/list daemon-local workspaces or create projects through global bootstrap will now be denied. This is a security correction, not a supported compatibility guarantee.

LocalOwner personal-local flows remain unchanged.

Do not remove wire variants in this corrective; old clients should receive a normal authorization error rather than a deserialization break.

## 10. Required tests

At minimum:

- new `tests/team_collaboration_postclosure_m001_registration_auth.rs` (or equivalent) covering Core + HTTP;
- existing `tests/team_collaboration_m001_http_auth.rs`;
- existing `tests/team_collaboration_m006_trajectory.rs`;
- local TUI/project-picker tests;
- authorization-policy unit/matrix tests;
- route-disposition tests.

Required assertions include:

- nonexistent path remains nonexistent after denied HTTP/Core attempt;
- workspace count unchanged after denied registration;
- project count unchanged;
- membership count unchanged;
- no global workspace root leaks to team principal;
- LocalOwner registration succeeds;
- `POST /api/workspace` with valid project authority remains unchanged;
- `SharedAuthz` rows all name a capability after `POST /api/project` leaves that class.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test --features server --test team_collaboration_postclosure_m001_registration_auth -- --test-threads=1
cargo test --features server --test team_collaboration_m001_http_auth -- --test-threads=1
cargo test --features server --test team_collaboration_m006_trajectory -- --test-threads=1
cargo test --test tui_project_picker -- --test-threads=1
cargo test -p codegg --features server --lib server::authz::tests -- --test-threads=1
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_http_route_disposition.py --verbose
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

If target names differ at implementation time, use the narrowest existing target that proves the same boundary and record the substitution in closure evidence.

## 12. Documentation updates

Document:

- raw daemon-local workspace registration is LocalOwner/proven-local only;
- global workspace enumeration is not a team surface;
- team project creation is intentionally not provided until a safe workspace/deployment authority contract exists;
- project-scoped workspace mutation remains available under `project.configure`;
- REST is compatibility and cannot widen Core authority.

## 13. Acceptance criteria

- Contributor, Viewer, and otherwise-active team principals cannot create a directory/workspace/project or obtain project ownership through arbitrary daemon-local path registration.
- Knowing a global WorkspaceId does not let a team principal create a project.
- Team principals cannot enumerate global canonical workspace roots.
- LocalOwner can still register a workspace and project through Core and the local TUI.
- HTTP `POST /api/project` is LocalOwner-only and produces zero side effects on denial.
- `server::authz::tests::every_shared_authz_row_names_a_capability` passes.
- Existing chat/team/controller/Workspace behavior remains green.

## 14. Stop conditions

Stop and register a separate design/ADR instead of improvising if:

- ordinary team users must be able to create new projects as a product requirement;
- safe team creation requires workspace ownership/ACLs, deployment roles, or invitation semantics;
- the fix requires changing ProjectId/WorkspaceId identity;
- a new public wire variant is needed solely to encode authority;
- LocalOwner cannot be distinguished canonically at the daemon boundary.

## 15. Closure evidence required

Closure record: `plans/closure/team-collaboration-post-closure-corrective/001-status.md`.

It must include:

- exploit/regression reproduction;
- requirement-to-code matrix;
- zero-side-effect evidence;
- Core/HTTP parity;
- LocalOwner compatibility;
- direct guard outputs;
- explicit statement that no team project-creation capability was invented; and
- audit of whether M003 can be unblocked.

## 16. Handoff notes

This is a security corrective, not a project-onboarding feature. Prefer the narrow safe boundary over preserving accidental remote behavior. Keep the implementation small enough that a later deliberate team project-creation design can build on it without depending on path knowledge as authority.
