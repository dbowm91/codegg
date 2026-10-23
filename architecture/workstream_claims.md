# Workstream Claims

Agent Workstream Blackboard M003 stores advisory responsibility claims and
dependency edges for existing `AgentRunGroup` members. The claims table is an
intent ledger. It does not participate in tool authorization, path policy,
Git policy, sandbox grants, scheduler admission, or run creation.

## Ownership

- `crates/codegg-core/src/workstream_claim.rs` owns typed claim, actor,
  prerequisite, state, CAS, target-normalization, and SQLite store contracts.
- `crates/codegg-core/src/session/schema.rs::migrate_v79` owns the additive
  schema. That migration is **version 79 of the chain**, not the current
  layout: `STORAGE_LAYOUT_VERSION` is **87**, because the writer permit (v86)
  and the session launch (v87) landed after it.
- `crates/codegg-protocol/src/projection/workstream.rs` joins authorized
  claim summaries to the existing session projection. It does not access
  storage or authorize a caller.
- `WorkstreamClaimStore::project_group` is the storage-backed M002 consumer:
  it authorizes the group actor, reads bounded claims and dependency edges,
  then invokes the pure protocol projector with the caller's authorized
  source snapshot.

## Scope and bounds

Every claim is tied to a group, project, and workspace. Run actors must be
active group members or the group owner run. Principal actors must match the
origin principal for the turn-owned group session. Task targets must identify
a task represented by a group member. These checks run again during each
mutation. The caller still has to pass the daemon's project authorization
gate before invoking the store.

Each group retains at most 128 claims and 4096 append-only claim events. Each
claim has at most 32 prerequisite edges. Claim labels are bounded at 256
bytes, reject traversal, absolute paths, backslashes, bidi controls, and
unsupported broad glob syntax, and are normalized to NFC. Path/interface
overlap uses a conservative case-folded fixed-prefix check; it may report
false positives and is not a file lock.

`task` claims use exact typed task identity and one active owner. `path` and
`interface` claims describe intended scope only. A claim never expands
`allowed_paths` or changes effective Git, shell, process, or filesystem
capability. The store does not resolve linked worktrees, symlinks, or
junctions and cannot establish filesystem case-fold behavior; such labels stay
advisory and all actual access remains subject to existing execution
containment and permission reductions.

## Transactions and recovery

Claim creation, lifecycle changes, dependency insertion, and corresponding
provenance rows use SQLite `BEGIN IMMEDIATE`. Active exact-target uniqueness
is also enforced by a partial unique index. CAS transitions require the
expected row version and lease generation. Handoff creates a versioned offer
for the new owner, who must accept it; offers participate in overlap
arbitration. Expired claims are released, provenance-recorded, and
generation-bumped before replacement; old handles cannot renew the new
owner's claim. A run actor that has become terminal cannot mutate or renew a
claim. No worker polls or replays agents after restart; durable rows remain
available through the bounded group inspection API.

Dependencies reference a claim or an existing group-member run, never both.
The store rejects self-edges, cross-group/project/workspace references,
duplicate edges, cycles, and fanout above 32. Adding an edge is revision-CAS
guarded. These edges do not auto-submit work or assert that a prerequisite's
agent-authored report is true.

## Projection and audit

Transition events store only actor kind/ID, action, claim ID, version,
generation, and timestamp; they contain no prompts or free-form agent text.
The read-only workstream projector accepts these bounded claim and dependency
summaries from the authorized group-scoped reader and filters them to the
source session's project/workspace/group. Callers must preserve the source
projection's event cursor for peer observation.

## Verification

```bash
cargo test -p codegg-core workstream_claim::tests
cargo test --test core_daemon -- workstream_claim
python3 scripts/check_project_catalog_invariants.py
```

The integration coverage lives in the `core_daemon` family at
`tests/core_daemon/workstream_claim_m003.inc`; the root package's integration
tests are families, so `cargo test --test workstream_claim_m003` selects zero
tests and still exits `0` — `workstream_claim_m003` is a family *member*, not a
target. The member's own test functions carry descriptive names, so filter on
the function prefix (`-- workstream_claim`) rather than the file name.

## Source verification

Verified 2026-10-23 against source. Corrected:

- `STORAGE_LAYOUT_VERSION` "79" → **87** (`crates/codegg-core/src/storage/mod.rs:48`).
  The value 79 is the *migration* that adds the claims schema
  (`schema.rs::migrate_v79`), not the current layout.
- The integration test target `workstream_claim_m003` → the `core_daemon`
  family member `tests/core_daemon/workstream_claim_m003.inc`.

Confirmed unchanged: the owning module
(`crates/codegg-core/src/workstream_claim.rs`); the protocol projector
(`crates/codegg-protocol/src/projection/workstream.rs`); every published
bound — `MAX_CLAIMS_PER_GROUP` 128, `MAX_CLAIM_EVENTS_PER_GROUP` 4096,
`MAX_CLAIM_DEPENDENCIES` 32, `MAX_CLAIM_TARGET_BYTES` 256; the `workstream_claim`
table's own `CHECK` constraints (`target` 1–256, `owner_id` 1–128,
`idempotency_key` 1–128, `claim_kind` in task/path/interface, `state` in
offered/claimed/released/blocked/completed); the `BEGIN IMMEDIATE` transaction
shape; and the advisory-only scope statement.
