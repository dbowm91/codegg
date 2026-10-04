# Shared Editor Document Foundation M005-A — Closure Status

Status: closed

Source implementation plan: `plans/implementation/editor-document-foundation/001-text-core-and-transaction-contract.md`

Source subsystem roadmap: `plans/subsystems/editor-document-foundation-roadmap.md#milestones`

Repository baseline reviewed: `53e336e4`

Implementation commits:

- `92e66782` — add the `codegg-document` text core and document contract

## 1. Executive finding

M005-A is complete. The new leaf crate provides revisioned UTF-8 byte-range
transactions, immutable snapshots, inverse edits, line mapping, and caller
provided resource limits. It has no daemon, LSP, protocol, storage, or frontend
dependencies.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Private rope backend with Rust 1.89 compatibility | `crates/codegg-document/Cargo.toml`; crop 0.4.3 declares Rust 1.85; `cargo check -p codegg-document` | pass | `ropey` 1.6.1 was not selected; crop provides byte-indexed rope operations and cheap immutable clones. |
| Revisioned snapshots and deterministic transactions | `crates/codegg-document/src/{buffer,edit}.rs` | pass | Revisions start at zero; empty transactions are no-ops. |
| UTF-8 boundary, overlap, ordering, and size validation | `crates/codegg-document/tests/transactions.rs` | pass | Includes Unicode, invalid boundaries, and overlapping edits. |
| Inverse edit round-trip | integration/property tests | pass | Multi-edit plus generated insertion cases. |
| Line mapping and exact line ending preservation | integration tests | pass | Includes CRLF and LF. |

## 3. Production implementation evidence

`codegg-document` was added as a workspace leaf crate. `crop::Rope` remains
private. Snapshots expose byte reads, line ranges, byte/line mapping, and whole
text export; transactions validate the pre-image ranges and apply from highest
offset to lowest. Inverses use ascending post-image coordinates.

## 4. Verification executed

### Commands run

```bash
rtk cargo check -p codegg-document
rtk cargo fmt --all
rtk cargo test -p codegg-document
rtk git diff --check
```

### Results

All commands passed locally. The focused suite reported five tests passed across
three suites, including a proptest round-trip. No hosted CI run was performed.

## 5. Invariant review

- Text is UTF-8 and byte offsets must be character boundaries: enforced by validation.
- Transactions are sorted, disjoint, and defined against the pre-image: enforced and tested.
- Snapshots do not expose mutable backend handles: public API only exposes copied text/ranges.
- Exact line endings are retained: tested with mixed LF/CRLF.
- Revisions do not wrap: checked increment returns `RevisionExhausted`.
- NUL is permitted in the pure crate; file policy remains a service responsibility.
- LSP encoding conversion remains outside this crate.

## 6. Failure and recovery review

Invalid transaction input fails before mutation. Revision exhaustion is a typed
error. Empty transactions preserve the same snapshot revision. There is no
durable state or async cancellation boundary in this pure crate.

## 7. Migration and compatibility review

No persisted schema or existing protocol changed. The new workspace crate is
additive. Rust 1.89 remains supported by the selected crop release.

## 8. Security review

The crate performs no I/O and handles no paths or authorization. Document and
transaction sizes are caller bounded through `DocumentLimits`; service defaults
are deferred to M005-B.

## 9. Documentation and operations

Added `architecture/document.md` describing ownership and text semantics.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M005-A finding | none | none |

## 11. Roadmap disposition

M005-A is closed. M005-B may proceed: its only hard dependency, M005-A, is
closed, and the public document transaction contract is stable.

## 12. Registry updates

- Mark M005-A closed and remove it from dependency-ready plans.
- Mark M005-B ready and remove its blocker.
- M005-C remains blocked on M005-B; M005-D remains blocked on M005-C.
