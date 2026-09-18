# Team Collaboration Corrective M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/001-network-api-authorization-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M001`

Repository baseline reviewed: `80a79599`

Implementation commits or pull requests:

- `cc41e4a4` — feat(server): network API authorization convergence (team-collaboration M001)

## 1. Executive finding

M001 is complete. Every authenticated HTTP compatibility route either reuses
canonical Core/AuthorizationService scope and capability decisions or is
explicitly restricted to LocalOwner compatibility. A personal token for
Project A cannot enumerate, read, mutate, receive events from, or answer
pending control items for Project B. LocalOwner compatibility remains
functional. No denied request produces a project/session/file mutation or
event subscription.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence |
|---|---|
| Single server authorization adapter on `AuthenticatedPrincipal` + canonical scope, delegating to `AuthorizationService`/Core authority; no duplicated role expansion | `src/server/authz.rs` (`authorize_project`, `authorize_session`, `authorize_enumeration`, `filter_visible_projects`, `require_local_owner`, `authorize_control_response`); all handlers call it before side effects |
| Exhaustive route disposition table; static guard so new routes cannot mount without disposition | `route_disposition_table()` (38 entries); `scripts/check_http_route_disposition.py` (4/4 pass) |
| Project/session/workspace reads and mutations use Core-equivalent capabilities | `project.rs` (`project.read`/`project.configure`), `session.rs` (`session.read`/`session.create`/`project.configure`), `workspace.rs` (`project.read`/`project.configure`), `config.rs` messages (`session.read`) |
| Project enumeration filters by visible membership before rows | `list_projects` gates `authorize_enumeration` then `filter_visible_projects`; test `project_enumeration_filters_by_membership` |
| File ops require explicit canonical context + file capability; ambiguous/path-only team requests fail closed | `file.rs` (`file.read`/`file.modify`, scope-free maps to 404); test `file_scope_denies_cross_project_with_zero_side_effect` |
| Permission/question lists require session access; responses require mutation authority now with M004 hook | `permission.rs`/`question.rs` (`session.read` list, `authorize_control_response`/`session.create` respond); test `pending_control_ids_do_not_leak_across_projects` |
| Config/provider/tool/MCP with no safe scope are LocalOwner-only | `require_local_owner` in `config.rs`, `provider.rs`, `tool.rs`, `mcp.rs`; test `sse_and_global_surfaces_are_local_owner_only` |
| Global `/api/event` not available as unfiltered team stream | `event.rs` LocalOwner-only, 404 for team; test SSE denial + LocalOwner stream |
| `AuthenticatedPrincipal` consumed at handler boundaries; no body/query principal/role/capability fields | All handlers take `Extension<AuthenticatedPrincipal>`; request structs carry only locators; static guard `check_no_payload_authority` passes |
| Task-trigger fire stays separate `cggtr_...` capability, never a principal credential | Trigger router untouched outside principal auth; `resolve_bearer_principal` rejects trigger bearers; test `task_trigger_bearer_never_binds_a_principal` |
| `/core` authoritative; REST is adapter; no cwd inference | `/tui`+`/core` are `CoreAdapter`; `scope.rs` unchanged (explicit scope only); router merge fix preserves documented paths |

## 3. Production implementation evidence

- New: `src/server/authz.rs` (adapter + `RouteDisposition` + 38-row
  `route_disposition_table()` + `denial_not_found`/`require_local_owner`/
  `authorize_project`/`authorize_session`/`authorize_enumeration`/
  `filter_visible_projects`/`session_project`/`authorize_control_response`
  with the M004 controller-lease hook).
- Converted: `project.rs` (enumeration filter, `project.read`/`configure`
  gates, creator Owner grant), `session.rs` (all mutations gated,
  privacy-safe scope checks), `workspace.rs`, `file.rs` (capability before
  filesystem mutation), `permission.rs` (canonical + legacy perm-id shapes,
  new optional `perm_id` body field), `question.rs`, `event.rs`
  (LocalOwner-only), `config.rs`/`provider.rs`/`tool.rs`/`mcp.rs`
  (LocalOwner-only), `ws.rs` legacy `/ws` LocalOwner-only gate.
- Fixed: `http.rs` now merges `api_router` instead of nesting under `/api`,
  so documented paths (`/api/...`, `/ws`, `/tui`, `/core`) are served
  instead of doubled `/api/api/...` + `/api/ws` shadows.
- Added `#[derive(Debug)]` to `AxumAppError` for testability (no behavior
  change).
- Static guard: `scripts/check_http_route_disposition.py` (route coverage,
  no orphans, adapter consumption, no payload authority).
- Docs: `architecture/server.md` (authorization-convergence section +
  LocalOwner-only router map), `architecture/authorization.md` (HTTP
  convergence section).

Route-disposition matrix (method, path → disposition, operation, capability):

| Method | Path | Disposition | Operation | Capability |
|---|---|---|---|---|
| GET | `/api/sessions` | shared_authz | session_list | session.read |
| POST | `/api/sessions` | shared_authz | session_create | session.create |
| GET | `/api/sessions/{id}` | shared_authz | session_load | session.read |
| DELETE | `/api/sessions/{id}/archive` | shared_authz | session_archive | session.create |
| POST | `/api/sessions/{id}/fork` | shared_authz | session_fork | session.read |
| POST | `/api/sessions/{id}/share` | shared_authz | session_share | project.configure |
| POST | `/api/sessions/{id}/unshare` | shared_authz | session_unshare | project.configure |
| POST | `/api/sessions/{id}/revert` | shared_authz | session_revert | session.create |
| POST | `/api/sessions/{id}/unrevert` | shared_authz | session_unrevert | session.create |
| GET | `/api/sessions/{id}/messages` | shared_authz | session_messages_load | session.read |
| GET | `/api/project` | shared_authz | project_get | project.read |
| POST | `/api/project` | shared_authz | project_register | none (any active; creator → Owner) |
| GET | `/api/project/list` | shared_authz | project_list | project.read |
| GET | `/api/projects` | shared_authz | project_list | project.read |
| GET | `/api/projects/{id}` | shared_authz | project_get | project.read |
| POST | `/api/projects/{id}/archive` | shared_authz | project_archive | project.configure |
| POST | `/api/projects/{id}/restore` | shared_authz | project_restore | project.configure |
| GET | `/api/workspace` | shared_authz | workspace_snapshot_request | project.read |
| POST | `/api/workspace` | shared_authz | workspace_register | project.configure |
| GET | `/api/workspace/list` | shared_authz | workspace_list | project.read |
| GET | `/api/file/read` | shared_authz | file_read | file.read |
| GET | `/api/file/list` | shared_authz | file_read | file.read |
| POST | `/api/file/write` | shared_authz | file_modify | file.modify |
| DELETE | `/api/file/delete` | shared_authz | file_modify | file.modify |
| GET | `/api/permission/{session_id}` | shared_authz | permission_list | session.read |
| POST | `/api/permission/{session_id}/submit` | shared_authz | permission_respond | session.create |
| GET | `/api/question/{session_id}` | shared_authz | question_list | session.read |
| POST | `/api/question/{session_id}` | shared_authz | question_respond | session.create |
| GET | `/api/config` | local_owner_only | config_read | none |
| GET | `/api/mcp` | local_owner_only | mcp_list | none |
| GET | `/api/providers` | local_owner_only | provider_list | none |
| GET | `/api/tools` | local_owner_only | tool_list | none |
| GET | `/api/event` | local_owner_only | event_subscribe | none |
| GET | `/ws` | local_owner_only | ws_legacy_rpc | none |
| GET | `/tui` | core_adapter | tui_transport | none |
| GET | `/core` | core_adapter | core_transport | none |
| POST | `/api/v1/task-triggers/{trigger_id}/fire` | trigger_capability | work_order_trigger_fire | none |

Intentionally LocalOwner-only: `/api/event`, `/api/config`, `/api/mcp`,
`/api/providers`, `/api/tools`, `/ws`. No authenticated project mutation
path bypasses the canonical authorization service.

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`):

- `cargo test --test identity_m003_daemon_authorization --locked -- --test-threads=1` — 9 passed.
- `cargo test --test team_collaboration_m001_http_auth --features server --locked -- --test-threads=1` — 10 passed:
  `every_authenticated_route_has_disposition`,
  `project_enumeration_filters_by_membership`,
  `cross_project_get_denies_as_not_found`,
  `cross_project_session_denies_with_zero_side_effect`,
  `session_mutations_require_owning_project_grant`,
  `file_scope_denies_cross_project_with_zero_side_effect`,
  `pending_control_ids_do_not_leak_across_projects`,
  `sse_and_global_surfaces_are_local_owner_only`,
  `revocation_applies_on_next_request_without_reconnect`,
  `task_trigger_bearer_never_binds_a_principal`.
- `python3 scripts/check_authorization_matrix.py` — 5/5 pass.
- `python3 scripts/check_http_route_disposition.py --verbose` — 4/4 pass.
- `bash scripts/check-core-boundary.sh` — pass.
- `cargo fmt --all -- --check` — pass; `git diff --check` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy -p codegg --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings` — pass.
- `scripts/verify.sh quick` — pass.

Substituted verification (documented, not weakened): the plan lists
`cargo clippy --workspace --all-targets --all-features`. Per `AGENTS.md`,
workspace sweeps never use `--all-features` (it drags real-server tests);
the two clippy invocations above cover the same code under the supported
feature sets (`verify.sh full` uses
`--features server,plugins,lsp-test-support`). No clippy finding was
suppressed.

CI truthfully: only local evidence is claimed here; CI (`verify` job) will
re-run the canonical guards.

## 5. Invariant review

- Authentication is not authorization: middleware binds principals;
  handlers authorize every project-scoped read/mutation through the
  service. No handler trusts body/query authority.
- LocalOwner broad policy passes through `AuthorizationService`
  (`require_local_owner` constructs a decision); no bespoke bypass.
- Team denials are privacy-safe 404s (`project_not_found` shape for
  project/session/file/permission/question; plain 404 for scope-less
  compatibility routes). No existence oracle.
- Trigger fire remains a separate capability; trigger bearers never bind
  principals and principal bearers never fire triggers.
- `/core` untouched and authoritative; REST is an adapter surface.
- No cwd inference: all scope is explicit and server-resolved.

## 6. Failure and recovery review

- Authorization precedes side effects in every converted handler; denied
  session creates, file writes, archives, shares, and permission/question
  responses leave stores/filesystems/registries unchanged (pinned by
  zero-side-effect assertions).
- Revocation between requests takes effect on the next request (same bound
  principal re-evaluated; pinned by test without reconnect).
- Enumeration races may omit newly granted rows but never include denied
  rows (filter after read, snapshot semantics).
- SSE reconnect reauthorizes per request (team never holds a stream).
- No stale role/capability caching across requests; every request builds a
  fresh `AuthorizationService` evaluation against current `TeamStore`.

## 7. Migration and compatibility review

No storage migration. Global-bearer/LocalOwner clients retain broad
compatibility. Personal-token team clients may newly receive 404s for
routes they were never entitled to use — the intended security
correction. Two intentional compatibility notes:

- Router mount fix: `api_router` is now merged (documented paths served).
  Deployments that accidentally used doubled `/api/api/...` or `/api/ws`
  paths must move to the documented paths.
- `POST /api/permission/{session_id}/submit` now accepts an optional
  `perm_id` body field for the canonical session-scoped shape while still
  accepting the legacy simple-perm-id path; ambiguous multi-pending
  submissions without `perm_id` fail closed.

## 8. Security review

Cross-project matrix proven: enumeration filtering, session GET/mutation
denial, file scope denial, permission/question ID non-leakage and response
denial, SSE non-leakage, revocation, zero side effects, trigger separation.
`eggsentry`/adversarial suites were not re-run beyond the focused
authorization suites; no new network, crypto, or secret-handling surface
was added (secrets never enter logs; only token kind is logged).

## 9. Documentation and operations

- `architecture/server.md`: authorization-convergence section, updated
  router map with LocalOwner-only labels, trigger exception, no-cwd rule.
- `architecture/authorization.md`: HTTP convergence section + adapter
  contract.
- New guard documented in `architecture/server.md` testing notes via the
  disposition table reference.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. Low/note: `/api/event`
team filtering via authorized projection subscriptions is future work;
LocalOwner-only is the intentional stop-condition disposition. M004 will
narrow `authorize_control_response` to the controller lease.

## 11. Roadmap disposition

M001 closes the security gate. Per the addendum dependency graph, M002
(project/channel chat policy) and M004 (shared-session controller lease)
may now proceed; M003 and M005 additionally require M002; M006 requires
M001–M005. The subsystem roadmap stays `active` with M001 marked closed.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/001-network-api-authorization-convergence.md`:
  `ready for handoff` → `implemented` (closure at this record;
  implementation `cc41e4a4`).
- `plans/registry.md`: M001 `ready` → `closed` (closure at this record);
  M002 and M004 audited for unblock (see below) and moved to `ready`;
  M003/M005 remain `blocked` on M002; M006 remains `blocked` on M001–M005.
- Subsystem roadmap `plans/subsystems/team-collaboration-corrective-addendum.md`:
  M001 `ready` → closed in the milestone table via this closure (table edit
  in the registry commit).

Dependency audit: M002 (`002-project-channel-chat-access-policy.md`) lists
only M001 as its hard dependency (consumes ADR-0006, already accepted) —
all hard deps now closed, so it becomes `ready`. M004
(`004-shared-session-controller-lease.md`) lists only M001 (consumes
ADR-0007, already accepted) — becomes `ready`. M003 requires M001+M002
(M002 still open → stays `blocked`). M005 requires M001+M002 (stays
`blocked`). M006 requires M001–M005 (stays `blocked`). No corrective
follow-up is registered: no new defect was found that M001 itself must
fix.
