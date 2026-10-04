# Shared Editor Document Foundation M005-B — Closure Status

Status: closed

Source implementation plan: `plans/implementation/editor-document-foundation/002-daemon-document-service-and-protocol.md`

Source subsystem roadmap: `plans/subsystems/editor-document-foundation-roadmap.md#milestones`

Repository baseline reviewed: `f47bf41b`

Implementation commits:

- `90df4156` — add the daemon document service and native protocol

## 1. Executive finding

M005-B is complete. The daemon now owns process-local canonical open text,
attachments, a single writer lease, bounded recent change-id deduplication,
disk-base digest state, and an additive `document.v1` protocol. Save and reload
remain explicitly not-ready until M005-C installs checked workspace mutation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Additive versioned document protocol and limits | `crates/codegg-protocol/src/document.rs`, `core.rs` | pass | `document.v1`, 8 MiB text, 256 edits, 1 MiB inserted-text bounds. |
| Scoped open identity and read authorization | `DocumentOpen`, authorization matrix, handler workspace/project binding | pass | Reads are authorized before file access; project/workspace bindings are checked. |
| Single writer and connection cleanup | `DocumentService::acquire_writer`, `detach_client`, focused service test | pass | Disconnect releases lease/attachments and retains dirty text. |
| Stable idempotency and stale revision rejection | `DocumentService::change`, focused service test | pass | Same ID/payload returns original revision; collisions and stale bases do not mutate. |
| Path, UTF-8, and resource policy | `validate_relative`, canonical containment, symlink rejection, caps; focused tests | pass | Includes traversal, symlink escape, document/edit/resident/attachment/dirty bounds. |
| Status and snapshot resync | `DocumentStatusGet`, `DocumentSnapshotGet` | pass | Both bind the supplied project ID to the canonical document record and attachment. |
| Checked save/reload handoff | `capture_save`, `commit_save`, `mark_disk_conflict`, `reload_verified` | pass | Save/reload protocol handlers return typed not-ready until M005-C installs the checked writer. |
| Concurrent first open convergence | focused test | pass | Two simultaneous opens produce one canonical document. |

## 3. Production implementation evidence

`CoreDaemon` owns one `DocumentService`; protocol dispatch uses the existing
request-family router and authorization preamble. Document text stays in
`codegg-document`. Documents are keyed by project, workspace, and normalized
relative path. Writer acquisition is a separate `file.modify` request after a
`file.read` open. Client disconnect removes its attachments and writer lease.

The service exports typed save handoff methods. A save snapshot captures the
canonical text, document revision, disk-base digest, workspace/path identity,
and canonical path. A later save commit remains dirty if a newer edit won the
race. Verified reloads advance the monotonic document revision.

The existing `CoreEvent` broadcast is global and has no project access filter.
Publishing document events there would leak document IDs and revisions to
unrelated connected clients. The plan was adjusted to use authorized bounded
`DocumentStatusGet` polling; a future push channel must apply project filtering.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo check -p codegg
rtk python3 scripts/check_authorization_matrix.py
rtk env RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
  rustup run 1.89-aarch64-apple-darwin cargo test --target aarch64-apple-darwin \
  -p codegg document_service::tests --lib
rtk git diff --check
```

### Results

Formatting, root package check, authorization matrix, and diff checks passed.
The focused native Rust 1.89 suite passed five service tests. The test command
ran on arm64 because the default x86_64/Rosetta toolchain cannot link against
the installed arm64 compression and iconv libraries. No hosted CI run was
performed.

## 5. Invariant review

- Daemon owns canonical open text; disk remains the durable source.
- Buffer text and revisions are process-local; shutdown/restart loses them.
- Read and writer capabilities are separated; stale leases fail closed.
- Project ID supplied to every ID-based operation must match the stored key.
- Absolute client paths are never accepted after open.
- Dirty documents are not evicted; document count, dirty count, resident bytes,
  attachment count, change count/bytes, and dedupe history are bounded.
- Global CoreEvent does not carry document events; observers poll authorized
  status and request a bounded snapshot.

## 6. Failure and recovery review

Open uses per-key serialization so concurrent first opens converge. Change
holds the document mutex across revision check, idempotency lookup, transaction
apply, and result recording. Exact retries are idempotent; conflicting ID reuse
and stale revisions fail without mutation. Disconnect releases transport-owned
leases but keeps dirty text available for a newly authorized writer. Restart
discards all in-memory documents, as specified by ADR-0011.

## 7. Migration and compatibility review

The protocol is additive; no database migration or durable text state was
introduced. `DocumentOpen` is read-only; writer acquisition is a separate
operation. Save/reload return a stable typed not-ready error until M005-C.

## 8. Security review

Authorization precedes document file access. Project/workspace bindings are
checked for catalog-backed requests, and ID-based calls recheck the project
scope in the service. Paths reject absolute, traversal, and symlink components;
resolved targets must remain under the canonical workspace and be regular
files. File bodies never appear in document change events. All protocol payloads
and in-memory collections have explicit caps.

## 9. Documentation and operations

Updated `architecture/document.md`, the implementation plan, and the client
qualification plan to document status polling and the staged save transition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M005-B finding | none | none |

## 11. Roadmap disposition

M005-B is closed. M005-C may proceed: the service API, identity scope, revision
contract, and disk-base handoff are stable. The parent M005 remains open.

## 12. Registry updates

- Mark M005-B closed and remove it from dependency-ready plans.
- Mark M005-C ready; its hard dependency M005-B is closed.
- Keep M005-D blocked on M005-C.
