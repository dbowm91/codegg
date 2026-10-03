# Editor Documents

The shared editor document foundation is frontend-neutral. `codegg-document`
owns immutable text snapshots and deterministic edits; the daemon service owns
open document identity, authorization, attachments, revisions, and disk-base
state; `codegg-client` owns an optimistic replica. Disk remains the durable
source of truth, and the LSP registry is a semantic mirror. Agent file tools
remain disk-authoritative.

## Text and transaction contract

`codegg-document` uses `crop` 0.4.3 behind a private backend boundary. It was
selected over `ropey` 1.6.1 because it supports byte-addressed rope edits,
immutable cheap clones, UTF-8 boundary checks, and indexed line lookup, and
declares Rust 1.85 MSRV (below this workspace's Rust 1.89). The crate does not
expose either backend type. Canonical ranges are sorted, disjoint UTF-8 byte
ranges against the pre-transaction snapshot. Edits apply in reverse order;
inverse edits are returned in ascending post-image coordinates. Revisions begin
at zero, empty transactions do not advance revisions, and line endings are
preserved exactly.

The core accepts UTF-8 strings including NUL; editor-service policy owns file
classification and resource bounds. LSP-specific UTF-16/UTF-32 positions remain
owned by `egglsp`; this crate only maps byte offsets and byte columns.

Unsaved editor text is process-local and may be lost on daemon restart. Save
must compare the current disk digest with the document's recorded disk base
under the existing workspace mutation authority. A mismatch preserves dirty
text and becomes a conflict. The client must not automatically replay divergent
local drafts after reconnect.
