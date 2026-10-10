# ADR-0014: Project Initialization Publication Contract

Status: superseded

Date: 2026-10-10

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Affected subsystem roadmaps:

- `plans/subsystems/repository-initialization-roadmap.md`

## Context

Repository Initialization M002 needs an explicit create/update operation for
the selected workspace root's `AGENTS.md`. The existing document protocol
opens only existing regular files and saves against an existing disk digest.
The LSP apply protocol is bound to reviewed LSP candidates and likewise
requires every affected file to exist. Neither is an adequate authority for
creating this file from `/init`.

## Decision drivers

- Bind each draft to the daemon-resolved project and workspace identity.
- Keep the target fixed to root `AGENTS.md`; do not accept a model- or
  client-selected path.
- Require a visible TUI preview and an explicit user action before publish.
- Let daemon authorization, not the UI alone, gate read and modify operations.
- Reject stale files, target symlinks, invalid file types, and path changes.
- Avoid persistent draft storage and avoid changing normal document editing.

## Considered options

### Option A — Reuse `DocumentSave`

It already has project authorization, writer leases, and revision checking.
It cannot open an absent file, its CAS observes an existing document revision,
and it is designed for editor buffers. Adding initialization semantics would
couple unrelated lifecycles and still require protocol changes.

### Option B — Reuse LSP preview apply

It provides daemon-side project mutation and checked file state, but accepts
only LSP preview kinds and requires an existing pre-state. Extending it for
initialization would make the LSP proposal contract own a non-LSP capability.

### Option C — Add a narrow project initialization draft/publish pair

The daemon creates a bounded draft from the explicit workspace root and
returns a short-lived opaque token plus preview data. Publish consumes that
token once and rechecks target state under the canonical workspace lock.
This adds a typed request family, but gives the capability an explicit,
auditable owner and can enforce a fixed target and exact CAS contract.

## Decision

Choose Option C. Add daemon-owned, ephemeral draft state and two typed
operations: a project-scoped draft read and a project-scoped publish mutation.
The draft request names project and workspace IDs; the daemon resolves their
authoritative workspace root, invokes the M001 read-only analyzer, and stores
the complete bounded candidate under an unpredictable one-use token. The
response includes the operation, evidence, diagnostics, candidate, target
digest/absent marker, and token. Tokens are bound to the authenticated client,
project, and workspace, expire after a short fixed interval, and are discarded
on daemon restart.

The publish request contains only the token. The daemon revalidates the
project/workspace binding and authorization, locks the canonical workspace,
requires the fixed relative path `AGENTS.md`, rejects symlinks and non-regular
targets, compares the current exact digest or absence marker with the draft,
and atomically publishes only on equality. The token is consumed on success or
CAS conflict. No arbitrary path or replacement content is accepted from the
client at publish time.

The TUI must render the full candidate and a readable diff before enabling an
explicit approve action. Cancel discards the local token; expiry, cancellation,
or connection loss never writes. Automatic agent approval modes cannot
approve this operation, and no model tool or agent loop may invoke publish.
After successful publication, the TUI requests the existing scoped asset
refresh. This affects future turns only; active turns retain their pinned
snapshot.

Authorization uses `file.read` for draft creation and `file.modify` for
publication, scoped to the direct project. The operation descriptor must be
exhaustive and included in the authorization matrix. These operations do not
grant broader filesystem authority.

## Consequences

### Positive

- Creation and update use one explicit project-owned write contract.
- A client cannot change the target or candidate after preview.
- Restart loses only an uncommitted draft; on-disk state remains authoritative.

### Negative

- The typed core protocol, authorization matrix, daemon dispatcher, client, and
  TUI all gain initialization-specific cases.
- Clients that do not know the new request family cannot use `/init` and must
  report unsupported capability rather than claim success.

### Neutral or deferred

- Drafts are not resumable after daemon restart; the user can regenerate them.
- No model-assisted enrichment is included.

## Compatibility and migration

No database migration is needed. New request/response variants are
version-tolerant through the existing typed protocol; older peers must receive
an explicit unsupported response. Existing document and LSP contracts remain
unchanged.

## Security and reliability implications

Draft evidence remains bounded and does not include file contents or global
instructions. The token is unpredictable, client-bound, short-lived, and
single-use. Publish rechecks project membership, direct-project `file.modify`,
workspace identity, fixed path, file type, symlink components, and exact CAS
state while holding the existing workspace mutation lock. Atomic replacement
must preserve the original file on failure. No command, hook, script, or
provider executes. Refresh failure is reported separately from successful
disk publication because a later refresh can recover from disk.

## Verification

Tests must cover authorization denial, client/project/workspace token binding,
expiry and replay, create/update CAS, stale edit/create/delete, symlink and
file-type refusal, atomic failure, cancellation, client disconnect, concurrent
publish, selected-project change, restart behavior, and post-write asset
refresh with in-flight snapshot pinning.

## Supersession

Superseded by `plans/adrs/ADR-0015-project-init-direct-scope-publish.md`, which
clarifies that the publish request carries the project/workspace IDs required
for direct-project authorization, in addition to its one-use draft token.
