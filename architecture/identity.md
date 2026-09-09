# Typed Domain Identity Foundation

Personal-local operation has no login ceremony: trusted local transports
resolve the deterministic `LocalOwner` principal without interactive
authentication. Network team listeners fail closed and require a verified
personal token (or, temporarily, the bootstrap global bearer mapped to
`LocalOwner`).

`codegg-core::identity` owns the path-independent identity primitives used by
later project, repository, provider, agent, channel, audit, and distributed
execution work.

## Contract

The following IDs share one lexical contract:

`ProjectId`, `RepositoryId`, `WorktreeId`, `NodeId`, `PrincipalId`,
`AgentRunId`, `AgentTaskId`, `ProviderConnectionId`, `ChannelId`, and
`AuditEventId`.

Each is a private-field, opaque string newtype with:

- UUIDv4 generation through `new()`;
- validated parsing through `parse`, `FromStr`, and `TryFrom<&str>`;
- a 128-byte maximum;
- ASCII alphanumeric, `-`, and `_` characters only;
- rejection of empty, path-like, NUL, control, whitespace, and other invalid
  values before the value is owned by the domain type;
- string serde, `Display`, `AsRef<str>`, ordering, hashing, and
  `into_string` support.

`WorkspaceId` remains owned by `workspace.rs` for compatibility and is
re-exported from this module. It retains its existing `new_unchecked` storage
hydration/test seam, while its new `parse` method uses the shared lexical
contract. Its existing transparent serde behavior remains unchanged for wire
and storage compatibility; new callers should use `parse` at untrusted
boundaries.

## Relations

- `ProjectRepositoryBinding` links a logical project to a repository.
- `ProjectBinding` links a project and optional repository to one workspace,
  with optional future worktree and node identities.
- `SessionBinding` expresses the canonical `ProjectId + WorkspaceId` session
  relation.

One project/repository pair can therefore have multiple workspace bindings.
These values are immutable, cloneable, and serde-friendly; they do not grant
authorization and do not own persistence.

## Compatibility boundary

Current session and protocol `project_id`, `workspace_id`, and `directory`
fields remain strings. They are legacy projections until a later additive
migration. `directory` is a locator, not a project identity, and no path or
Git remote is accepted as an identity source.

`scripts/check_identity_path_usage.py` guards the canonical project-storage
module as well as the identity primitives. It rejects explicit path-derived
`ProjectId` construction; compatibility string projections remain allowed.

## Durable project and repository authority

Domain Identity Milestone 002 adds `codegg_core::project_storage::ProjectStorage`
and additive schema migration v25. `logical_project`, `repository`,
`project_repository`, `workspace_project_binding`,
`session_project_binding`, and `identity_diagnostic` are canonical authority.
The historical `project` table, `session.project_id`, and `session.directory`
remain readable compatibility projections and are never used to infer a
canonical ID.

Repository matching is local-only and bounded. Only a unique normalized Git
remote can establish repository lineage; missing, conflicting, redacted, or
insufficient evidence produces a non-resolved binding and a reason-coded
diagnostic. Rebind operations require the current binding revision.

See [`project_identity_storage.md`](project_identity_storage.md) for the
schema, reconciliation, import, inspection, and operator workflow.

The catalog service in `codegg_core::project_catalog` provides
list/get/register/archive/restore operations on top of the identity
storage layer.

## Daemon context authority

Domain Identity Milestone 003 makes `codegg_core::context::ProjectContextResolver`
the shared authority for executable project/workspace/session context. It validates
typed `ProjectId` and `WorkspaceId` against durable catalog and binding records,
and rejects archived, mismatched, unresolved, or stale contexts. A directory is
only a compatibility locator for a unique existing binding; it never creates an
identity.

## Team principal, membership, role, and capability domain

Identity, authorization, and audit M001 adds `codegg_core::team`, the durable
canonical principal/project-membership domain. Schema migration v51 creates
`principal` and `project_membership` tables; `STORAGE_LAYOUT_VERSION` is 51.

- Principals: `Human`, `ServiceAccount`, `Node`, and explicit `LocalOwner`.
  `TeamStore::ensure_local_owner` bootstraps the deterministic
  `"local-owner"` record idempotently; personal-local daemons resolve the OS
  owner to this record without a login ceremony. Authorization still receives
  an explicit principal; `LocalOwner` is a composition, not a bypass.
- Roles: `Viewer`, `Contributor`, `Maintainer`, `Owner`. `ProjectRole::parse`
  fails closed on unknown input.
- Capabilities: 21 semantic operation verbs (`project.read`,
  `project.observe`, `project.chat`, `session.create`, `session.read`,
  `session.observe`, `agent.invoke`, `agent.delegate`, `file.read`,
  `file.modify`, `command.execute`, `job.submit`, `job.cancel`, `git.read`,
  `git.write`, `worktree.create`, `worktree.remove`, `project.configure`,
  `member.manage`, `audit.read`, `node.target`). `Capability::parse` fails
  closed. Expansion is central and monotonic: Viewer (6) ⊂ Contributor (14)
  ⊂ Maintainer (19) ⊂ Owner (21); see `role_capability_matrix()` and
  `role_capability_rows()` for the executable matrix.
- Memberships: project-scoped `(project_id, principal_id)` rows with role,
  `active`/`suspended`/`revoked` state, and optimistic `revision`. Only
  `active` memberships grant capabilities. Mutations require the current
  revision; stale writers receive `RevisionConflict` instead of silently
  restoring revoked authority. Rows are never physically deleted, so
  re-creation after revocation returns `MembershipConflict` and re-grants go
  through the revision-checked update path.
- Records contain no credential secret, token, or key material. Existing
  provider-credential records are never reinterpreted as human principals.
  Existing string principal fields (`ProjectionPrincipalId` synthetic
  `"local-user"`/`"internal-test"`/`"authenticated-remote"` values) remain
  compatibility projections until M003; `adapt_principal_to_projection_id`
  is a one-way diagnostic adapter and `is_compatibility_projection` marks
  the synthetic values.

This milestone claims no request-time
authorization. Transport authentication is owned by M002
(`codegg_core::transport_auth`, migration v52 `personal_auth_token`,
`AuthenticatedPrincipal`/`RequestAuthorityContext`, `ClientRegistry`
principal binding, HTTP/WS personal-token verification, local-socket
`LocalOwner` binding, and the bootstrap-only disposition of the legacy
global bearer).

## Transport authentication and principal binding (M002)

`codegg_core::transport_auth` resolves every accepted connection to a
canonical principal from trusted transport evidence and carries that
immutable principal through client/request context. Personal-local startup
remains login-free.

- Principals: `AuthenticatedPrincipal` (canonical `PrincipalId` + kind +
  `AuthMethod` + `TransportClass` + owning `client_id`) and
  `RequestAuthorityContext` (principal + correlation). Both are immutable
  after construction; payload DTOs remain locators and never supply
  principals, roles, or capabilities.
- Methods: `local_owner` (trusted Unix-socket/stdio/inproc, no login),
  `personal_token` (verified network token → distinct team principal),
  `bootstrap_global_bearer` (legacy shared secret → `LocalOwner` compat
  only), `internal_test` (harness only).
- Tokens: `PersonalTokenStore::create_personal_token` returns the
  one-time `cggt_<token_id>.<secret>` plaintext plus a durable record;
  only the SHA-256 digest, owner, expiry, and revocation persist (migration
  v52 `personal_auth_token`; `STORAGE_LAYOUT_VERSION` is 52). Verification
  is constant-time, transactional, and restart-safe; revoked/expired or
  disabled-principal tokens fail new authentication immediately.
- Binding: `ClientRegistry::register_with_principal` /
  `set_principal` (immutable once bound) + `principal_for`;
  `CoreDaemon::request_authority_for_client` /
  `projection_access_for_client` resolve the bound principal or fall back
  to `LocalOwner` for in-process/stdio/legacy callers. Projection contexts
  use `from_canonical_principal` with the bound principal string
  (`LocalOwner` → `"local-user"`, others → canonical id); the legacy
  `"authenticated-remote"` synthetic is never produced on this path.
- Compatibility: the global bearer is bootstrap-only and maps to
  `LocalOwner`. Removal condition: delete `server.token` /
  `CODEGG_SERVER_TOKEN` once every operator holds a personal token.
- Trust limits: the Unix-socket file lives in the user-scoped runtime
  directory; per-connection `SO_PEERCRED` UID validation is future
  hardening and is explicitly not claimed. Authentication (who) stays
  separate from M003 authorization (may do what).

## Daemon authorization and originating-principal attribution (M003)

`codegg_core::authorization` enforces project/resource semantic
capabilities at the daemon operation boundary using transport-bound
principals, and propagates immutable originating-principal attribution
into durable work. Full contract and the 135-row operation matrix live in
`architecture/authorization.md`.

- Inventory: `operation_descriptor` maps every native `CoreRequest` to a
  scope (`global`, `direct_project`, `via_session`, `via_job`,
  `enumeration`, `opaque`) plus a semantic `Capability`. The match is
  exhaustive with no wildcard; `scripts/check_authorization_matrix.py`
  pins coverage.
- Service: `AuthorizationService::authorize` evaluates principal +
  project/resource + capability + current membership revision and returns
  a structured `AuthorizationDecision` (or a typed `AuthorizationError`:
  `authorization_denied`, `authorization_scope_required`,
  `authorization_scope_ambiguous`, `authorization_principal_inactive`,
  `authorization_unavailable`). `LocalOwner` decides through the same API
  under the broad local policy (`PolicyKind::LocalOwnerBroad`).
- Boundary: `CoreDaemon::handle_request_with_client` authorizes before
  dispatch; denials reply with zero side effect. `ProjectList` is
  privacy-filtered to `project.read` grants (`visible_projects`);
  `ProjectGet` denies as `project_not_found` so existence is not leaked.
- Attribution: `OriginAttribution` (bound principal + captured decision)
  persists per scope (`session`, `turn`, `job`, `provider`) in migration
  v53 `origin_attribution` (`STORAGE_LAYOUT_VERSION` is 53); first write
  wins. Pre-M003 records use the explicit `legacy-local` provenance
  marker. `ToolExecutionContext` carries `origin_principal` /
  `origin_auth_method` / `origin_decision_id` via `apply_origin`.
- Narrowing: `authorize_child_delegation` fails closed on escalation;
  every turn re-enters the gate with `agent.invoke` on the session
  project. Provider scope follows `authorize_provider_use`
  (personal owner-only, project grant-checked, deployment owner-gated).
- Projection: `team_capabilities_to_projection` plus
  `bounded_resolver_for_principal` converge the projection seam onto team
  grants.
