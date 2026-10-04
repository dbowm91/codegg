# Shared Editor Document Foundation M005-A — Text Core and Transaction Contract

Status: implemented

Repository baseline: `43cc33f6e740de33878d78a8fa2de959692cf819`

Parent roadmap:

- `plans/subsystems/editor-document-foundation-roadmap.md`

Accepted decision:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: foundational library / invariant

## 1. Objective

Create a small frontend-neutral `codegg-document` leaf crate that owns CodeGG editor text snapshots and deterministic text-transaction semantics.

This milestone must be usable by:

- daemon `DocumentService`;
- `codegg-client` optimistic replicas;
- the future TUI IDE;
- a later graphical IDE.

It must not depend on any of those consumers.

## 2. Existing problem

The repo currently has:

- raw `String`/file content in filesystem tools;
- an LSP-only `OpenDocumentRegistry`;
- LSP position conversion helpers;
- frontend text widgets for chat/prompts;
- no reusable editor buffer abstraction.

Without a pure core, daemon/client/TUI implementations would each invent their own:

- revision type;
- byte/range validation;
- transaction ordering;
- snapshot behavior;
- line mapping;
- inverse edit behavior.

M005-A closes that gap only.

## 3. Non-goals

- Filesystem reads/writes.
- Daemon service/protocol.
- LSP process/client calls.
- Ratatui widgets.
- Monaco/Tauri.
- Syntax highlighting/tree-sitter.
- CRDT/OT.
- Persistent undo history.
- Grapheme-aware cursor movement.
- Collaborative editing.
- Agent tool semantics.

## 4. Crate boundary

Create:

```text
crates/codegg-document/
  Cargo.toml
  src/
    lib.rs
    buffer.rs
    edit.rs
    position.rs
    error.rs
```

Exact internal file names may differ.

Dependency direction:

```text
codegg-document
  -> rope implementation
  -> small Unicode/index helper dependencies only when justified

codegg-protocol      may depend on exported scalar/DTO-compatible concepts only if needed later
codegg-client        -> codegg-document (M005-D)
root codegg daemon   -> codegg-document (M005-B)
TUI                  -> codegg-client/document through later adapter
```

The public API must not expose `crop::Rope`, `ropey::Rope`, or another selected backend type.

## 5. Rope qualification

Qualify, in order:

1. `crop` 0.4.3 — preferred candidate.
2. `ropey` 1.6.1 — fallback/control.

Selection criteria:

- compiles under Rust 1.89;
- correct arbitrary UTF-8 text;
- canonical operations can be expressed in byte offsets without O(n) conversion on every normal edit;
- cheap snapshot/clone behavior;
- deterministic line lookup;
- reasonable dependency/compile footprint;
- no backend-specific type leaks into public CodeGG types.

A small temporary benchmark/qualification fixture may compare:

- initial load: 1 KiB / 1 MiB / 10 MiB;
- random localized insert/delete/replace;
- clone/snapshot then divergent edit;
- line lookup;
- Unicode-heavy content.

Do not add a permanent benchmark gate. Record the selection in the closure and `architecture/document.md`.

If `crop` passes all criteria, use it. Do not choose a custom rope.

## 6. Canonical public types

At minimum define:

```rust
pub struct DocumentRevision(u64);

pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

pub struct TextEdit {
    pub range: TextRange,
    pub insert: String,
}

pub struct TextTransaction {
    pub edits: Vec<TextEdit>,
}

pub struct DocumentSnapshot {
    // opaque backing storage
    revision: DocumentRevision,
    // immutable text view/snapshot API
}

pub struct AppliedTransaction {
    pub revision: DocumentRevision,
    pub snapshot: DocumentSnapshot,
    pub inverse: TextTransaction,
}
```

Exact field visibility/type details may differ.

Rules:

- revision 0 may represent initial/open state or be rejected; choose one convention and document it;
- revision increment uses checked arithmetic;
- no wraparound;
- snapshots are immutable;
- transactions do not themselves contain daemon/project/path identity;
- protocol/client idempotency IDs are not part of the pure text crate.

## 7. Transaction semantics

Canonical transaction coordinates are UTF-8 byte offsets against the pre-transaction snapshot.

A transaction must reject:

- start > end;
- out-of-bounds ranges;
- offset inside a UTF-8 code point;
- overlapping edits;
- duplicate ambiguous ranges;
- edit-count overflow/policy violation when supplied limits say so;
- total inserted byte limit violation.

Choose and document one deterministic edit-order convention:

- callers provide ranges in ascending pre-image order;
- the implementation validates ascending/non-overlap then applies from highest offset to lowest, or performs an equivalent deterministic transformation.

Do not let implementation order alter externally visible semantics.

Empty transaction:

- either typed no-op with unchanged revision, or rejected;
- choose once and test it.

Preferred: explicit no-op result without revision advance.

## 8. Snapshot API

The snapshot API should provide bounded/predictable primitives needed by daemon/client/TUI adapters:

- `len_bytes()`;
- `len_lines()`;
- whole-text export with explicit size responsibility;
- byte slice/range read;
- line range lookup;
- byte offset -> line;
- line -> byte offset;
- validity check for byte boundary;
- content SHA-256 helper may remain outside this crate unless it is naturally useful to all consumers.

Do not expose mutable rope handles from a snapshot.

## 9. Position helpers

M005 uses byte offsets as canonical edit coordinates.

Add helpers required to bridge frontend/LSP positions without forcing an LSP dependency:

- byte offset -> `(line, byte_column)`;
- `(line, byte_column)` -> byte offset;
- optional scalar/UTF-16 unit conversion helper only if it can be implemented without duplicating `egglsp` incorrectly.

Preferred ownership:

- keep LSP encoding-specific conversion in `egglsp::position`;
- M005-C composes document line slices with the existing negotiated encoding helpers.

Do not create a second LSP position implementation in `codegg-document`.

## 10. Line endings and text class

M005-A supports UTF-8 text only.

Preserve exact bytes for:

- LF;
- CRLF;
- mixed line endings.

Do not normalize line endings on load/edit/export.

NUL-containing UTF-8 text:

- decide whether to permit in the pure crate;
- daemon M005-B may reject it as an editor-document policy.
- Prefer pure crate permissiveness and service policy separation.

Binary/non-UTF-8 rejection belongs to the daemon open path, not the rope API.

## 11. Inverse transactions

Provide an inverse transaction from every successful non-noop apply.

Purpose:

- frontend-owned undo/redo can use deterministic core primitives later;
- tests can prove `apply(inverse, apply(tx, S)) == S`.

M005 does not define an undo stack, grouping policy, or persistence.

Inverse edits must be expressed against the post-transaction snapshot and remain deterministic for multi-edit transactions.

## 12. Bounds

The pure crate should accept a small configurable `DocumentLimits`/transaction-limits input rather than hardcoding product policy if practical.

At minimum support bounds for:

- max document bytes;
- max edits/transaction;
- max inserted bytes/transaction.

The daemon will choose product defaults in M005-B.

All limit arithmetic uses checked operations.

## 13. Deterministic/property tests

Required:

- ASCII insert/delete/replace;
- Unicode BMP and supplementary-plane characters;
- invalid mid-codepoint offsets;
- beginning/end/empty-file edits;
- multi-edit non-overlap ordering;
- overlapping rejection;
- duplicate range rejection;
- no-op semantics;
- revision overflow test seam;
- LF/CRLF/mixed preservation;
- inverse round trip;
- snapshot immutability after divergent edit;
- random transaction sequence against a simple `String` reference implementation;
- line mapping after edits;
- large document limits;
- serialization only for explicitly serializable public scalar types.

Property tests may use a dev dependency but must stay bounded/deterministic in normal CI.

## 14. MSRV and dependency guard

- root MSRV stays Rust 1.89;
- selected rope dependency must compile on 1.89;
- no Tauri/Ratatui/tokio/sqlx/LSP dependency;
- no unsafe added by CodeGG merely for the rope wrapper;
- add a static/dependency guard if needed to prevent upward dependency creep.

## 15. Documentation

Create:

- `architecture/document.md` with the pure document model and ownership diagram.

Update as needed:

- `architecture/native_crates.md`;
- workspace Cargo/dependency docs.

Do not document daemon/protocol behavior until M005-B lands.

## 16. Verification

Expected minimum:

```bash
cargo test -p codegg-document --locked
cargo clippy -p codegg-document --all-targets --locked -- -D warnings
cargo check -p codegg-document --locked
rustup run 1.89.0 cargo check -p codegg-document --locked
scripts/verify.sh quick
git diff --check
```

Record temporary rope-qualification commands/results in closure evidence.

## 17. Acceptance criteria

M005-A closes when:

1. `codegg-document` exists as a leaf/shared crate.
2. rope dependency is selected on recorded qualification evidence.
3. public API hides rope implementation.
4. byte-coordinate transaction semantics are explicit and tested.
5. snapshots are immutable/cheap enough for intended use.
6. inverse transaction round-trip is proven.
7. Unicode/line-ending behavior is deterministic.
8. configurable bounds fail before unbounded work/allocation where practical.
9. Rust 1.89 passes.
10. no daemon/LSP/frontend dependency entered the crate.
11. `architecture/document.md` records the model.

## 18. Stop conditions

Stop and register a narrower prerequisite if:

- neither qualified rope works under Rust 1.89;
- byte-index semantics require a pervasive public backend leak;
- inverse transactions cannot be represented without introducing frontend-specific cursor state;
- selected dependency requires unsafe/codegen/platform runtime not acceptable for a core leaf crate.

## 19. Closure record

Create:

- `plans/closure/editor-document-foundation/001-status.md`

Record:

- dependency selection;
- public types;
- transaction rules;
- property-test summary;
- MSRV;
- dependency graph;
- unresolved findings;
- M005-B readiness.
