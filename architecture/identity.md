# Typed Domain Identity Foundation

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
