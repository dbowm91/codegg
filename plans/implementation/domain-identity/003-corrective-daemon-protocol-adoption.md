# Domain Identity Milestone 003 — Corrective Daemon and Protocol Adoption

Status: implemented

Repository baseline: `27a2e54` (`main`; the original Milestone 003 plan was
registered but had no production implementation)

Supersedes the unimplemented handoff:

- `plans/implementation/domain-identity/003-daemon-protocol-adoption.md`

Corrective finding and historical closure record:

- `plans/closure/domain-identity/003-status.md`

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-3--daemon-and-protocol-adoption`

## 1. Objective and boundary

Close the original Milestone 003 finding by making canonical
`ProjectId + WorkspaceId` context authoritative for new daemon session
requests, session writes, snapshots, and compatibility-facing server routes.
Preserve old rows and additive protocol decoding, while ensuring directory-only
compatibility requests perform only bounded lookup of existing canonical
bindings and never manufacture identity from path text.

Project Catalog Milestone 004, Runtime Assets refresh, multi-project TUI, ACLs,
and removal of historical compatibility columns remain out of scope.

## 2. Required corrective work

### Authoritative context

- Add a UI/server/auth-free core context module with bounded `SessionId` and
  directory parsing, typed `ProjectContextRequest`, resolved project/workspace
  context, binding revision/status, and typed resolution failures.
- Resolve explicit IDs through `ProjectStorage`, `ProjectCatalog`, and the
  workspace store; reject missing, archived, mismatched, unresolved, and
  ambiguous contexts.
- Resolve a directory only by finding one existing canonical workspace/project
  binding. Return `project_context_required` for none or ambiguity.

### Atomic storage adoption

- Add `SessionStore::create_with_binding` so a new session, compatibility
  projection, and resolved `session_project_binding` commit together.
- Add canonical-project session listing and use it instead of the historical
  path-valued `session.project_id` projection.
- Add binding-aware import for daemon imports, retaining the legacy storage
  import API for old callers.
- Preserve storage-level legacy fork/import readability; daemon and server
  fork paths resolve context before cloning and bind the child or delete it on
  binding failure.

### Protocol and snapshots

- Add additive `ProjectContextDto` and `SessionBindingDto` values.
- Extend session-create/template requests with optional stable project/workspace
  fields without removing legacy directory fields.
- Add canonical binding to session DTOs and snapshots, with serde defaults for
  old clients and fixtures for old/new payloads.
- Advertise `identity_aware_context` in server capabilities without changing
  the existing protocol version.

### Daemon and server adoption

- Route session create, template create, load/attach hydration, list, fork,
  import, and runtime binding through the resolver.
- Keep legacy sessions readable but do not execute them without a resolved
  context; return bounded actionable diagnostics.
- Make `ProjectInfo.id` a stable catalog `ProjectId`, retain filesystem paths
  only as compatibility locators, and source counts/listing from canonical
  catalog and binding stores.
- Apply the same canonical checks to REST and WebSocket compatibility adapters.

### Guards and documentation

- Expand the identity path guard to daemon/server/protocol authority surfaces
  and include a negative path-derived `ProjectId` fixture.
- Update the eight architecture documents named by the original plan to
  describe resolver ownership, compatibility fields, atomic writes, and the
  Project Catalog 004 boundary.

## 3. Acceptance evidence

- New daemon-created sessions have a resolved canonical binding in the same
  transaction as the session row.
- Explicit and directory compatibility context resolution is deterministic;
  archived, mismatched, unresolved, and ambiguous inputs fail before action.
- Session DTOs/snapshots and capabilities carry stable identity additively.
- Server project IDs and canonical session counts no longer use path text.
- Legacy storage rows remain readable, while daemon/server execution requires
  canonical context.
- Static identity, cwd, core-boundary, project-catalog, and execution-ownership
  guards pass.
- Focused protocol, context, session, migration, and workspace tests pass;
  the capped workspace suite is recorded in the linked closure record.

## 4. Closure handoff

The linked closure record must retain the original corrective finding as
history, list the implementation commit(s), include a requirement matrix and
test log, name remaining legacy fields and removal prerequisites, and state the
dependency disposition for Project Catalog, Runtime Assets, Multi-Project TUI,
and Session Projections.
