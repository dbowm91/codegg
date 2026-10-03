# Shared Editor Document Foundation M005-B — Daemon Document Service and Native Protocol

Status: active

Repository baseline: `43cc33f6e740de33878d78a8fa2de959692cf819`

Parent roadmap:

- `plans/subsystems/editor-document-foundation-roadmap.md`

Accepted decision:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Hard dependency:

- M005-A strict closure.

Primary class: daemon capability / protocol / concurrency

## 1. Objective

Add the daemon-owned ephemeral editor document service and additive native protocol.

The service becomes canonical owner of open unsaved text and document revisions while disk remains durable source authority.

No LSP synchronization is implemented in this milestone beyond explicit seams consumed by M005-C.

## 2. Ownership split

Recommended composition:

```text
codegg-document
  pure text/revision/transactions
        |
        v
root daemon DocumentService
  scope + auth + attachments + disk-base state
        |
        +---- CoreRequest/CoreResponse/CoreEvent document.v1
        |
        +---- M005-C LSP bridge seam
```

Do not move authorization, filesystem path resolution, or transport connection identity into `codegg-document`.

If durable-domain helpers naturally belong in `codegg-core`, keep them dependency-neutral. The root daemon remains orchestration authority.

## 3. Document identity

Define an opaque ephemeral `DocumentId` with a distinct prefix/type.

Canonical key:

```text
project_id
workspace_id
workspace-relative normalized path
```

Open requests may name the scoped relative path. After open, change/save/reload/close requests use `DocumentId`; the server resolves scope/path from the document record.

Never trust an absolute path supplied after open.

Reject:

- empty path;
- absolute path;
- `..` escape;
- symlink escape;
- directory target;
- non-UTF-8 file;
- file above configured editor-size bound.

## 4. Document state

Suggested internal state:

```rust
struct OpenDocument {
    id: DocumentId,
    key: DocumentKey,
    buffer: DocumentBuffer,
    disk_base: DiskBase,
    dirty: bool,
    conflict: Option<DiskConflict>,
    writer: Option<DocumentWriterLease>,
    readers: AttachmentSet,
    recent_changes: BoundedChangeDedupe,
}
```

`DiskBase` contains at least SHA-256 of the disk bytes used to establish/reload/save the document.

mtime/size may be stored as hints only.

Do not persist buffer text/disk-base state to SQLite in M005.

## 5. Single-writer lease

M005 deliberately does not implement collaborative live editing.

Each open document has:

- zero or more read attachments;
- at most one writer lease.

Open may request writable access. The daemon grants it only when:

- caller has `file.modify`;
- no current live writer owns the document.

Other clients can attach read-only with `file.read`.

Writer lease properties:

- transport/client connection owned;
- opaque lease id;
- released on explicit detach or connection death;
- release does not save/discard the document;
- a new authorized writer may acquire an unowned dirty document, preserving unsaved canonical text;
- stale lease ids fail closed.

Do not implement force-takeover in M005 unless necessary for recovery; an explicit acquire after old connection cleanup is sufficient.

## 6. Document lifetime and bounds

The service is process-local.

Policies:

- clean + no frontend attachment + no LSP owner: eligible for immediate/short-TTL eviction;
- dirty + no frontend attachment: retain in memory so accidental frontend disconnect does not discard text;
- dirty documents are never silently LRU-evicted.

Global/service bounds must cap:

- open document count;
- dirty orphan count;
- per-document bytes;
- total resident editor bytes;
- attachments/document;
- recent idempotency records.

When a bound prevents opening/editing, return a typed resource-limit error. Do not evict dirty content to make room.

Daemon shutdown/restart explicitly loses unsaved text per ADR-0011.

## 7. Additive document protocol

Add a namespaced/capability-negotiated `document.v1` surface.

Expected requests:

```text
DocumentCapabilities
DocumentOpen
DocumentSnapshotGet
DocumentWriterAcquire        [optional if not folded into Open]
DocumentChange
DocumentSave
DocumentReload
DocumentClose/Detach
```

Expected responses include:

- document id;
- canonical project/workspace ids;
- relative path;
- revision;
- bounded text snapshot when requested/opened;
- dirty state;
- disk-base digest or opaque revision token;
- conflict state;
- writer/read-only mode;
- limits/capabilities.

An initial implementation of events through the existing global CoreEvent
broadcast was rejected during implementation review: project scoping is not
enforced on that fanout, so it could disclose document IDs and revisions to a
client without access to the document. M005-B therefore exposes an authorized,
bounded `DocumentStatusGet` metadata poll; no document data is sent through the
global event stream. Any future push event must use a project-filtered channel.

The event metadata contract, when a scoped transport exists, contains no text:

```text
DocumentRevisionChanged
DocumentSaved
DocumentConflictChanged
DocumentWriterChanged
DocumentGone
```

A client that needs text after an event requests a snapshot.

## 8. Authorization

Operation policy:

- capabilities: normal authenticated availability response;
- open/snapshot/read attachment: `file.read` scoped to the canonical project;
- writer acquire/change: `file.modify`;
- save/reload destructive mutation: `file.modify`;
- close own attachment: attachment ownership + readable scope; must not require mutation capability merely to detach.

Authorization must happen before reading file content or exposing document existence to unauthorized principals.

Team/project privacy-safe not-found conventions remain unchanged.

## 9. Change request and idempotency

`DocumentChange` carries:

- document id;
- writer lease id;
- `base_revision`;
- stable `change_id`;
- bounded `TextTransaction`.

Server behavior:

1. resolve document/attachment;
2. verify writer lease;
3. check bounded dedupe by `change_id`;
4. verify base revision equals current canonical revision;
5. apply transaction through `codegg-document`;
6. atomically advance revision/dirty state;
7. record dedupe result;
8. emit revision event;
9. return new revision and minimal state.

Duplicate same `change_id` + identical digest returns the original accepted result.

Same `change_id` with different payload is a typed identity collision.

Stale base revision returns current revision/resync-required metadata and performs zero mutation.

## 10. Snapshot/resync

`DocumentSnapshotGet` returns one authoritative bounded snapshot.

It must include enough state for a replica to reset:

- text;
- revision;
- dirty/conflict;
- writer lease relationship for the requesting attachment;
- disk-base opaque/hash metadata as allowed by protocol.

Snapshots are point-in-time consistent under the document's synchronization primitive.

Do not stream an unbounded edit history in M005.

## 11. Open semantics

Open flow:

1. resolve project/workspace from explicit ids;
2. authorize before file read;
3. normalize/validate relative path;
4. if document already open, attach to existing canonical state rather than reread disk over it;
5. otherwise read bounded UTF-8 file from disk;
6. compute disk SHA-256;
7. create canonical buffer revision;
8. optionally grant writer lease;
9. return snapshot.

Two concurrent first opens of the same key must converge to one document id/state.

Use per-key entry serialization or an equivalent race-safe map pattern; do not hold a global service lock across filesystem I/O.

## 12. Save/reload seam

M005-B defines service transitions but does not yet finalize shared safe-write/LSP behavior.

Provide clear internal interfaces consumed by M005-C:

- capture current document snapshot/revision/disk base;
- begin writer-only save under document generation;
- commit new disk base after verified write;
- mark disk conflict;
- reload from an externally supplied verified disk snapshot.

Do not implement an unsafe direct `tokio::fs::write` shortcut merely to make the protocol complete. If shared checked write is not yet ready, `DocumentSave` may remain typed `not_ready` until M005-C only if the protocol tests explicitly encode that staged behavior. Preferred: land B+C in ordered commits with B's save handler wired to the C primitive before B is declared closed.

## 13. Connection cleanup

The native daemon already tracks connection lifecycle.

On disconnect:

- remove that connection's document read attachments;
- release its writer leases;
- do not save;
- do not discard dirty documents;
- emit writer-state changes where another observer is attached;
- clean unattached clean documents according to service policy.

Cleanup must be deterministic and tested against reconnect races.

## 14. Concurrency tests

Required deterministic interleavings:

- two concurrent first opens converge;
- two writable opens => one writer;
- read-only + writer coexist;
- same writer concurrent changes with same base => exactly one revision wins, other stale;
- duplicate `change_id` identical => idempotent;
- duplicate id different payload => collision;
- disconnect vs change commit;
- disconnect vs writer acquire;
- writer release vs new acquire;
- snapshot racing accepted change returns either complete old or complete new state, never torn text/revision;
- dirty orphan survives connection cleanup;
- clean unattached eviction;
- service bounds do not evict dirty documents.

Use barriers/test hooks rather than scheduler-probability loops for key races.

## 15. Protocol compatibility

- additive Core request/response/event variants;
- older clients ignore document capability;
- no storage migration;
- no protocol version bump unless the decoder contract absolutely requires it;
- new DTO fields use existing compatibility conventions.

Add static/exhaustive request-family/authorization mappings so new document mutations cannot bypass policy.

## 16. Security

- text bodies never appear in audit structural metadata or ordinary logs;
- request debug formatting must not dump full document text;
- bounds checked before cloning/allocating where practical;
- unauthorized path probing is privacy-safe;
- handles are not bearer authority without attachment/principal checks;
- writer lease identity is server issued;
- canonical absolute path never becomes renderer authority;
- no remote-origin/Tauri changes.

## 17. Documentation

Update:

- `architecture/document.md`;
- `architecture/protocol.md`;
- `architecture/core.md`;
- `architecture/authorization.md`;
- `architecture/client.md` only for wire capability visibility.

## 18. Verification

Expected minimum:

```bash
cargo test -p codegg-document --locked
cargo test -p codegg --lib core::document
cargo test -p codegg --test document_service_integration --locked
cargo test -p codegg --test identity_m003_daemon_authorization --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Exact target names may follow final module layout.

## 19. Acceptance criteria

M005-B closes when:

1. one daemon service owns canonical open text/revisions.
2. document identity uses explicit project/workspace/relative path.
3. read/write authorization is exhaustive.
4. at most one writer lease exists/document.
5. changes are revision-fenced and idempotent.
6. stale changes perform zero mutation and return resync state.
7. connection cleanup releases leases without autosave/discard.
8. dirty orphan buffers are bounded but never silently evicted.
9. snapshots are consistent and bounded.
10. protocol is additive/no persistence migration.
11. concurrency fixtures pass.
12. M005-C has an explicit save/LSP integration seam.

## 20. Stop conditions

Stop and create an ADR/corrective if:

- collaborative simultaneous writers become a requirement;
- durable unsaved recovery becomes required;
- team principals need a new file-edit capability model;
- a generic file API replacement is proposed;
- safe document identity cannot be expressed without absolute path authority in the client.

## 21. Closure record

Create:

- `plans/closure/editor-document-foundation/002-status.md`

Record service ownership, limits, protocol table, auth matrix, race evidence, restart limitation, and M005-C readiness.
