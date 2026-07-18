# Project Catalog Milestone 003 — Lazy Activation and Health

Status: closed; see `plans/closure/project-catalog/003-status.md`

Repository baseline: `972c286` (Runtime Assets Milestone 003 closed; explicit
activation refresh seam available)

Source roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-3--lazy-activation-and-health`

Related closure evidence:

- `plans/closure/project-catalog/002-status.md`
- `plans/closure/runtime-assets/003-status.md`

Primary class: infrastructure

## 1. Objective

Activate workspace service bundles only for selected catalog projects and
workspaces, expose bounded project health, and connect activation to the
daemon-owned runtime-asset refresh seam without making catalog listing eager.

## 2. Dependencies and boundaries

Hard dependencies Project Catalog Milestones 001–002 are closed. The Runtime
Assets Milestone 003 interface dependency is closed: use
`CoreDaemon::refresh_project_activation(project_id, workspace_id)` and the
bounded refresh status/report types. Do not create a second asset refresh
coordinator or infer identity from process cwd.

This plan must not implement the Project Catalog Milestone 004 protocol/server
migration, multi-project TUI tabs, remote execution, team authorization, or
semantic monorepo discovery.

## 3. Scope

### In scope

- Project/workspace activation leases with explicit ownership and bounded
  lifetime.
- Lazy construction of workspace services for the selected concrete scope;
  catalog list/get paths remain probe-free.
- Idle eviction and clean release of workspace service bundles.
- Explicit project/workspace selection and activation/rebind diagnostics.
- Asset refresh on activation through the Runtime Assets M003 seam.
- Bounded health aggregation for catalog, workspace, asset, and service state,
  including stale/unavailable and contention outcomes.
- Concurrent same-workspace activation coalescing and restart/hydration tests.
- Architecture and operator documentation for activation/eviction ownership.

### Explicitly out of scope

- Full project DTO/request/event protocol and REST/WS migration (M004).
- TUI tab lifetime/navigation, remote SSH execution, watchers, providers,
  LSP/index/build startup during catalog listing, and authorization.

## 4. Required invariants

- Listing many catalog records retains no workspace-service leases and does no
  expensive repository/service probing.
- Activation is explicit, project/workspace-scoped, idempotent, and bounded.
- Only the selected workspace's services are started; inactive scopes can be
  evicted without deleting durable catalog/session history.
- Asset activation refresh is transactional and reports published/retained/
  invalid/coalesced outcomes through the existing coordinator.
- Health never exposes secrets or unbounded paths and distinguishes catalog,
  workspace, asset, and service failures.

## 5. Verification and handoff

Add focused lease/eviction/health tests, two-project isolation tests,
activation/refresh contention tests, restart hydration tests, and negative
tests proving catalog listing cannot start external services. Run the capped
workspace command and all project-catalog, runtime-asset, daemon-CWD,
execution-ownership, and core-boundary guards before creating the M003 closure
record.
