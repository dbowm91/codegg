# Team Collaboration — Post-Closure Authority and Lifecycle Corrective Addendum

Status: active

Repository baseline reviewed: `626585a1fad449a637e4828777bfa21d222abea0`

Predecessor work:

- `plans/subsystems/team-collaboration-corrective-addendum.md`
- `plans/closure/team-collaboration-corrective/001-status.md` through `006-status.md`
- `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md`
- `plans/closure/identity-authorization-audit/003-status.md`

Long-term references:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/001-terminology-and-domain-model.md`

Related ADRs:

- `plans/adrs/ADR-0006-project-channel-chat-access-policy.md` — unaffected.
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` — unaffected.
- No new ADR is required. This corrective restores the already-selected rule that daemon-local filesystem locators are not remote/team authority, narrows frontend cancellation to the owning view, and repairs verification guards. If implementation requires a new team project-creation capability or workspace-ownership model, stop and register a separate ADR/plan.

## 1. Purpose and corrective trigger

The M001-M006 collaboration campaign closed at `626585a1` after a cross-cutting trajectory harness. A post-closure review found three defects outside the trajectories exercised by that harness:

1. daemon-local workspace/project bootstrap remains available through authority shapes that are too broad for team principals;
2. leaving the non-modal Workspace view aborts every TUI task classified as `Command` rather than only Workspace-owned work; and
3. several static guards are stale or excluded from canonical quick/CI verification, allowing the first two classes of defect to coexist with green routine verification.

This addendum does not reopen the collaboration feature set. It corrects authority and lifecycle boundaries discovered after closure and makes their regression checks canonical.

## 2. Work classification

### Invariants

- A remote/team principal cannot convert an arbitrary daemon-local path or globally registered workspace into project authority.
- Raw daemon-local filesystem registration is available only to a proven-local/LocalOwner authority until an explicit team workspace-ownership contract exists.
- Project/workspace enumeration never exposes daemon-local roots to a principal lacking an authorized project relationship.
- Leaving one TUI view cancels only work owned by that view; generic command tasks for unrelated features remain live.
- Cheap security/authority guards used as closure evidence must execute from the canonical verification entry point and CI.

### Capabilities

No new user capability is introduced. LocalOwner project registration and the existing Workspace/chat/team behavior remain available.

### Infrastructure

Authorization descriptors/dispositions, TUI task-lifecycle ownership, and static verification scripts.

### Polish

Documentation and diagnostics describing why team project creation is unavailable without an explicit safe authority model.

## 3. Non-goals

- Designing organization-wide project creation, invitation, billing, OIDC, or deployment-admin roles.
- Adding a new `project.create` capability without a durable workspace/project ownership contract.
- Adding a workspace ACL or changing stable Project/Workspace identity.
- Reworking chat policy, membership administration, controller leases, or WorkOrder scheduling.
- Replacing the TUI task registry.
- Broadly moving every change-triggered guard into routine CI.

## 4. Current implementation evidence

### Registration/path authority

- `CoreRequest::WorkspaceRegister { root }` is classified as `ScopeKind::Global` with no capability and calls `WorkspaceRegistry::get_or_register` on the caller-provided daemon-local path.
- `CoreRequest::WorkspaceList` is also global/no-capability and returns `WorkspaceSnapshot` values containing `canonical_root`.
- `CoreRequest::ProjectRegister` is global/no-capability and accepts a globally registered `workspace_id`. The policy comment describes any-active-principal team bootstrap, while the Core handler itself does not establish an owner membership.
- HTTP `POST /api/project` independently accepts an absolute path, creates the directory if missing, registers it, registers a project, and grants a non-LocalOwner caller Owner membership.
- The earlier TUI project-navigation contract explicitly states that raw path registration is disabled over transports that cannot prove daemon-local filesystem context and that remote clients may use only a separately safe known-workspace seam.
- HTTP `POST /api/workspace` is different: it resolves an already-authorized project/workspace context, requires `project.configure`, and sanitizes the requested path under the authorized root. That scoped behavior must remain unchanged.

### Workspace task cancellation

- `leave_workspace_view` calls `task_registry.cancel_kind(TuiTaskKind::Command)`.
- `TuiTaskRegistry::cancel_kind` aborts every active record of that kind.
- `TuiTaskKind::Command` is intentionally shared by chat, team administration, control, project picker, provider connections, session selection, WorkOrders, diagnostics, and other unrelated TUI command flows.
- The Workspace reducer already has request generation/cancellation and stale-completion guards; the broad kind cancellation is not required for correctness.

### Verification

- `scripts/check_audit_coverage.py` extracts authorization operations from `crates/codegg-core/src/authorization.rs` although operation descriptors now live in `authorization/policy.rs`.
- `scripts/check_scheduler_bypass.py` misses the existing `standalone-compat` annotation in `snapshot_capture.rs` because the annotation is outside its narrow scan window.
- `server::authz::tests::every_shared_authz_row_names_a_capability` is red because `POST /api/project` is `SharedAuthz` with capability `none`.
- `scripts/check_http_route_disposition.py`, `check_audit_coverage.py`, and `check_scheduler_bypass.py` are not in `scripts/verify.sh quick` or the routine CI job.

## 5. Target architecture

Daemon-local registration is treated as deployment authority, not project membership authority. Until CodeGG has a durable team workspace-ownership/project-creation contract, team principals fail closed for raw `WorkspaceRegister`, global workspace enumeration, and unscoped Project registration. LocalOwner/proven-local flows remain available. Project-scoped workspace mutation under an already-authorized project remains unchanged.

Workspace view background work has an explicit ownership boundary. Closing the Workspace view invalidates/cancels only dashboard/view-owned work. Chat state and unrelated command tasks continue normally.

The cheap static guards protecting these boundaries become part of canonical quick verification and CI once they are truthful and green.

## 6. Dependency graph

```text
M001 daemon-local registration authority  ─┐
                                          ├─> M003 verification-guard convergence
M002 Workspace task cancellation ownership ┘
```

M001 and M002 are independent and ready. M003 is blocked on both because its canonical guard assertions must encode the corrected production semantics.

## 7. Milestones

### M001 — Daemon-local registration and project-bootstrap authority

Class: invariant/security corrective.

Restrict raw workspace registration, global workspace enumeration, and unscoped project registration to LocalOwner/proven-local authority until a safe team workspace-ownership contract exists. Close the HTTP path-to-Owner escalation and align REST/Core documentation and tests.

Plan: `plans/implementation/team-collaboration-post-closure-corrective/001-registration-authority-boundary.md`.

### M002 — Workspace-owned task cancellation

Class: invariant/frontend lifecycle corrective.

Remove the generic `Command`-kind cancellation from Workspace close and give Workspace-owned async work a precise cancellation boundary while retaining generation/reconnect stale guards.

Plan: `plans/implementation/team-collaboration-post-closure-corrective/002-workspace-task-cancellation-ownership.md`.

### M003 — Verification guard convergence and canonical gating

Class: invariant/verification corrective.

Repair the audit/scheduler guards, make the route-disposition invariant truthful after M001, and add the cheap authority/security guards to canonical quick verification and CI.

Plan: `plans/implementation/team-collaboration-post-closure-corrective/003-verification-guard-convergence.md`.

## 8. Cross-cutting requirements

Denials must be privacy-safe and must precede filesystem/catalog/membership side effects. No authorization decision may trust a caller-supplied role/principal. LocalOwner behavior must still pass through the canonical authorization seam rather than a frontend-only exception.

Frontend cancellation changes must preserve shutdown cancellation, tab/session cancellation, reconnect generation fencing, and bounded task accounting.

Static guards must fail on real violations, not be weakened to silence current code. If repairing a guard exposes a genuine audit/scheduler/authorization defect, implementation must stop and register the defect rather than marking it as an exemption solely to achieve green verification.

## 9. Verification strategy

M001 adds adversarial team-principal tests around raw workspace registration/listing and project registration through Core and HTTP, including zero-side-effect assertions and LocalOwner compatibility.

M002 adds task-registry/view integration tests proving an unrelated `Command` task survives Workspace close while Workspace-owned work is cancelled or rendered stale.

M003 executes each repaired guard directly, then through `scripts/verify.sh quick` and CI-equivalent commands. The final closure must rerun the M006 trajectory suite plus the new M001/M002 regressions.

## 10. Risks and decision points

The primary risk is accidentally designing a new team project-creation model inside a security fix. Do not. The safe stop condition is LocalOwner/proven-local only. A future requirement for ordinary team users to create projects requires explicit workspace/deployment authority and should be designed separately.

The frontend risk is over-scoping a new task kind. Workspace chat uses persistent per-project `ChatState` and need not be aborted merely because the Workspace route closes. Only view/dashboard-owned fetches should be cancelled.

The verification risk is converting a failing guard into a permissive guard. Prefer moving or making annotations precise and parsing the canonical source of truth over expanding ignore windows.

## 11. Completion definition

This corrective closes only when:

- an ordinary team principal cannot register/list arbitrary daemon-local workspaces or register a project from an unowned global workspace/path;
- HTTP `POST /api/project` cannot create a daemon-local path/project/Owner grant for a team principal;
- LocalOwner project/workspace bootstrap remains functional;
- closing Workspace does not abort unrelated `TuiTaskKind::Command` operations;
- Workspace-owned async completions remain safely cancelled or stale-dropped;
- audit coverage, scheduler bypass, route disposition, authorization matrix, quick verification, and routine CI are all green on the corrected semantics; and
- no high/medium authority or lifecycle finding remains.

## 12. Milestone status

| Milestone | Status | Dependencies |
|---|---|---|
| M001 | ready | predecessor M001-M006 closed; existing local-filesystem authority rule is stable |
| M002 | ready | predecessor M005 closed; TUI task registry contract is stable |
| M003 | blocked | hard dependency on M001 + M002 closure |
