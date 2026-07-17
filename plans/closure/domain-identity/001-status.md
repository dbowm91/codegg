# Domain Identity Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/domain-identity/001-typed-identity-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-1--typed-identity-primitives-and-relation-contracts`

Repository baseline reviewed: `f203ed9` (`main`, synchronized with `origin/main`)

Implementation commits or pull requests:

- `f203ed9` — add the typed identity foundation, relation contracts, compatibility annotations, architecture documentation, tests, and opt-in path-identity guard.

## 1. Executive finding

Milestone 1 is closed. The core boundary now provides the ten requested typed
identity primitives, a shared bounded parser/generator contract, and typed
project/repository/workspace/session relation values. Existing workspace,
session, job, daemon, and protocol behavior remains compatible because storage
and wire DTOs were not migrated and `WorkspaceId` retained its existing
transparent serde representation and compatibility constructor.

The milestone remains intentionally infrastructure-only. Project/repository
tables, migration of legacy session rows, server route adoption, and broad
path-derived identity enforcement remain deferred to later milestones.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Ten requested typed IDs exist in a stable core boundary | `crates/codegg-core/src/identity.rs`; `crates/codegg-core/src/lib.rs`; `src/lib.rs` re-export | pass | `ProjectId`, `RepositoryId`, `WorktreeId`, `NodeId`, `PrincipalId`, `AgentRunId`, `AgentTaskId`, `ProviderConnectionId`, `ChannelId`, and `AuditEventId` are public from `codegg_core::identity`. |
| One shared validation and generation contract | `validate_identity`; macro-generated `new`, `parse`, `FromStr`, `TryFrom<&str>`, serde, `Display`, `AsRef<str>`, ordering/hash, and `into_string` implementations | pass | Values are bounded to 128 UTF-8 bytes and allow only ASCII alphanumeric, `-`, and `_`. |
| Invalid, empty, path-like, control, whitespace, and oversized values fail | `crates/codegg-core/src/identity.rs` unit tests | pass | Tests cover empty, separators, NUL, control, whitespace, unsupported characters, and overlong input. |
| Project/repository/workspace/session relations are represented | `ProjectRepositoryBinding`, `ProjectBinding`, and `SessionBinding` plus `crates/codegg-core/tests/identity.rs` | pass | Multiple workspace bindings can share one project/repository pair; sessions carry project and workspace separately. |
| IDs round-trip as strings through serde and database-compatible conversions | Identity serde tests; `AsRef<str>` and `into_string`; unchanged SQLite string fields and workspace store binding | pass | No schema migration or SQLx identity trait was needed; future stores can bind the explicit string conversion without changing the wire form. |
| Existing `WorkspaceId` behavior remains compatible | `WorkspaceId` remains defined in `workspace.rs`, keeps transparent serde and `new_unchecked`, and passes `cargo test -p codegg-core workspace` and `tests/workspace_isolation.rs` | pass | Strict `WorkspaceId::parse` is additive and uses the shared lexical validator. |
| No new durable identity is derived from paths | `ProjectId` has no path constructor; path rejection tests; `scripts/check_identity_path_usage.py` | pass | The guard is intentionally opt-in and narrow for later project-storage adoption. |
| Focused tests, documentation, and guards are present | Identity integration tests; `architecture/identity.md`; workspace/session/protocol/core docs; static guard results | pass | No user-visible UI or protocol version change was introduced. |

## 3. Production implementation evidence

### Core/domain

- Added `codegg_core::identity` with the ten new private-field identity
  newtypes, `IdentityParseError`, shared validation, UUIDv4 generation, and
  relation contracts.
- Re-exported the existing `WorkspaceId` from the identity boundary without
  relocating or rewriting its storage owner.
- Added strict `WorkspaceId::parse`, `FromStr`, `TryFrom<&str>`, `AsRef<str>`,
  `into_string`, ordering, and generation conveniences while preserving the
  existing compatibility constructor and serde behavior.

### Storage and migration

- No production schema migration, table, store authority, or session-row
  migration was added.
- New IDs expose explicit string conversions suitable for existing SQLite
  columns and future additive stores.

### Protocol and DTOs

- Existing protocol and session DTO shapes remain string-backed.
- Added only rustdoc compatibility annotations identifying `project_id`,
  `workspace_id`, and `directory`; no protocol version bump was required.

### Runtime, frontend, and operations

- No daemon execution path, frontend, route, or user-visible behavior changed.
- The opt-in `scripts/check_identity_path_usage.py` guard provides a narrow
  future enforcement seam without claiming that later route migration is
  complete.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p codegg-core identity
rtk cargo test -p codegg-core workspace
rtk cargo test -p codegg-protocol
rtk cargo test --test workspace_isolation
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

### Results

- Formatting: pass.
- `codegg-core identity`: 8 passed.
- `codegg-core workspace`: 13 passed.
- `codegg-protocol`: 75 passed.
- `workspace_isolation`: 6 passed.
- Workspace-wide clippy with all targets/features and `-D warnings`: pass;
  no issues found.
- Core boundary guard: pass.
- Identity path guard seam: pass.
- Daemon cwd guard: pass.
- Diff whitespace check: pass.
- Broader workspace test execution was not required for this core-only,
  additive milestone and was not claimed as closure evidence.

## 5. Invariant review

- Existing valid `WorkspaceId` serialization and registry behavior remain
  intact; the workspace and isolation suites pass.
- New parsed identities reject path separators and cannot be constructed from
  `Path` values or Git remotes. No new production path-to-ID conversion was
  added.
- Parsing is bounded and rejects empty, malformed, control, whitespace, and
  unsupported-character input before the identity type owns the value.
- Identity values are opaque data types only; no authorization or principal
  trust is inferred from successful parsing.
- No process-global cwd dependency was introduced; the existing cwd guard
  passes.
- Existing sessions, jobs, daemon startup, and protocol consumers continue to
  compile and pass their focused compatibility suites without migration.

## 6. Failure and recovery review

- Generation has no partial persistent state and uses UUIDv4 without a global
  mutable registry or lock.
- A concurrent eight-thread sample generated 2,048 unique project IDs and
  revalidated every generated value.
- Serialization/reconstruction tests prove that valid IDs remain valid across
  deserialization; restart recovery does not apply because this milestone
  introduces no persisted authority.
- Malformed and oversized untrusted strings return typed, kind-aware errors
  without echoing the rejected value.
- Cancellation, duplicate delivery, daemon-generation recovery, and lease
  semantics are not applicable to these immutable primitives and remain owned
  by the existing job/session subsystems.

## 7. Migration and compatibility review

- No schema version or database migration changed.
- Existing string-backed `project_id`, `workspace_id`, and `directory` fields
  remain available; `directory` remains a locator and is not reinterpreted as
  a project identity.
- Protocol DTO JSON shapes and protocol version remain unchanged; the protocol
  suite passes.
- `WorkspaceId` remains source-compatible at its existing module path and
  retains transparent string serde. Callers crossing untrusted boundaries
  should use `WorkspaceId::parse`.
- Legacy path/string projections intentionally left for Milestones 2–3:
  session/project storage fields, server project routes, compatibility DTOs,
  and any existing path-valued `project_id` data.

## 8. Security review

- Identity parsing imposes a 128-byte bound and rejects empty, path-like,
  NUL/control, whitespace, non-ASCII, and unsupported-character values.
- IDs contain no credentials, usernames, filesystem paths, or authorization
  decisions.
- Debug/display output renders only the opaque identity string; no path is
  transformed into an ID.
- The future path-identity guard is documented as opt-in and currently checks
  only explicit forbidden constructor patterns.

## 9. Documentation and operations

Updated:

- `architecture/identity.md` — authoritative typed-ID and relation contract;
- `architecture/core.md` — core module ownership and re-export;
- `architecture/workspace.md` — project/workspace/session distinction and
  guard seam;
- `architecture/session.md` — compatibility-field clarification;
- `architecture/protocol.md` — typed-internal/string-wire guidance.

Operational/static evidence is recorded in section 4. No new runtime command
or recovery procedure is required for this additive milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `WorkspaceId` retains permissive transparent serde for compatibility even though `parse` is strict. | Callers that deserialize untrusted legacy workspace values must explicitly validate them. | Use `WorkspaceId::parse` at new untrusted boundaries; revisit serde tightening only in a compatibility-scoped later milestone. |
| medium | Existing path-valued `project_id` handling in legacy storage/server routes remains. | Later catalog and daemon adoption must migrate authority without silently reinterpreting paths. | Address through Domain Identity Milestones 2–3; no new path-derived identity path was added here. |

No critical or high-severity finding remains. These findings are explicit,
bounded, and within the source plan's deferred scope; they do not prevent
closure of Milestone 1.

## 11. Roadmap disposition

Milestone 1 is closed and the next dependency may proceed to planning/handoff.
The domain-identity roadmap remains active for project/repository storage and
later daemon/protocol adoption. Existing Runtime Assets and Project Catalog
plans remain blocked where they require those later domain milestones.

## 12. Registry updates

- Mark Domain Identity Milestone 001 closed and link this closure record from
  `plans/subsystems/domain-identity-roadmap.md`.
- Move Domain Identity Milestone 001 from the dependency-ready registry table
  to the recently closed table.
- Advance the Domain Identity roadmap's current milestone to Milestone 2 and
  remove Milestone 1 closure as its blocker.
- Unblock Provider Connections Milestone 001, which depends directly on the
  now-closed typed-ID foundation; update its plan and roadmap to `ready`.
- Keep Runtime Assets and Project Catalog blocked on their explicitly named
  later domain-identity context/storage dependencies.
