# Managed Agent Scratch Milestone M006 — Scratch usage census and operator visibility

Status: ready for handoff
Repository baseline: `b7e28698` on `main`

Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`
Source predecessor: `plans/implementation/managed-agent-scratch/005-temp-root-redirect-offer-before-human-approval.md`
Source predecessor closure: `plans/closure/managed-agent-scratch/005-status.md`
Long-term requirements: `plans/000-long-term-specification.md` (agent operability and
authority), `plans/002-long-term-roadmap.md`
Applicable ADRs: None required — this milestone adds a **read-only** measurement
and an operator diagnostic. It creates no authority, grants no access, and
reclaims nothing, so it changes no ownership or authorization boundary.
Primary class: infrastructure (usage accounting), with an operator-visible
diagnostic surface. It is **not** a reclamation capability; see §5.

## 1. Objective

Give the operator a **measured** view of the per-user scratch tree — total
bytes, tree count, age range, and which trees reclamation would refuse — so the
growth of a tree that nothing currently reclaims stops being invisible.

The deliverable is two operator surfaces:

- `codegg daemon scratch-usage` — a full per-tree report, human-readable by
  default and `--json` for scripting.
- `codegg daemon status` — a one-line scratch usage summary, because that is
  the command an operator already runs.

**Nothing is deleted, moved, renamed, or modified.** This milestone observes
the tree; it does not change what the tree is, what may reclaim it, or what
host cache maintenance may purge.

## 2. Why this milestone is ready

Hard dependencies, all closed:

- **M004** (`plans/closure/managed-agent-scratch/004-status.md`) — the
  reclamation primitives this milestone observes are implemented and
  qualified: the owner-validated tombstone path, the link-free tree walk, and
  the crash-resumable tombstone recovery.
- **M005** (`plans/closure/managed-agent-scratch/005-status.md`) — closed, with
  the live observation recorded. The temp-root redirect offer depends on a
  scratch root being real and administered, which is what this milestone
  measures.
- **C001** (`plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`)
  — the Linux native containment qualification is closed, so the tree this
  milestone walks is a tree whose grants are already evidence-qualified on
  Linux.

Interface dependencies, stable:

- The scratch tree layout and the identity rules are closed by M001/M002 and
  are not being changed here.
- The daemon socket request/response surface
  (`crates/codegg-protocol/src/core.rs`) accepts an additive request variant.
- The CLI lifecycle-command surface (`enum DaemonCommand` in `src/main.rs`)
  accepts an additive subcommand.

No new durable state, no storage migration, and no configuration key are
required. The census is a function of what is already on disk.

## 3. Current implementation evidence

**The tree exists and is administered.** `src/scratch.rs` owns it:

- The session root is `<cache>/codegg/scratch/sessions/<hex(sha256(session_id))>`
  (`src/scratch.rs:136-144`); the run root is
  `<session_root>/runs/<hex(sha256("agent-run:<run_id>"))>`
  (`src/scratch.rs:149-158`); shared resources are
  `<session_root>/shared/<hex(sha256("shared-scratch:<scratch_id>"))>`
  (`src/scratch.rs:162-169`).
- The root is `dirs::cache_dir()/codegg/scratch`, mode `0700` on Unix, with an
  owner record per session tree (`.codegg-owner`,
  `src/scratch.rs:481-540`).
- The documented contract: "Cache contents are disposable and may be removed
  by host cache maintenance" (`architecture/process-tool-execution-ownership.md:39-41`,
  `architecture/security.md:295-297`).

**Reclamation is event-driven, not age-driven.** The only three things that
remove a tree today are a revoked shared resource, a terminal delegated run,
and a soft-deleted session (`architecture/security.md:310-316`,
`docs/tools.md:313-318`). There is no sweep, no TTL, and no size ceiling.
A session that is closed but never soft-deleted keeps its tree indefinitely.

**This is measurable, and the measurement is missing.** Against the isolated
daemon used for the M005 live observation, on 2026-10-20: 15 durable session
rows, **none** soft-deleted, 14 session trees still on disk totalling 132 KB.
Nothing had reclaimed them, and nothing could have observed them.

**The reclaim validator is the right model for a census, and it has a
Unix-specific refusal.** `validate_reclaim_tree` (`src/scratch.rs:384-411`)
rejects any entry that is a symlink, a non-regular file, or — on Unix — a
regular file whose link count is not 1. On Windows the link count is not
readable at this crate's MSRV, so Windows unlinks names instead
(`src/scratch.rs:405-409`). The consequence on Linux and macOS is that a
single `ln a b` inside a model's own scratch gives both names `nlink == 2` and
makes the **whole tree** unreclaimable; the tree is quarantined under a
tombstone and every later retry renames that tombstone and fails again
(exercised at `src/scratch.rs:832-850`). A census that reports reclaimability
makes that condition visible instead of silent.

**That refusal is reachable under the strongest containment, so it is a real
defect and not a theoretical one.** The Landlock backend requests
`write_access = AccessFs::from_all(abi)` for every writable root
(`src/security/sandbox/landlock.rs:80`, applied at `:113-115`), and
`from_all` includes **REFER** — the right that governs linking. A hard link
whose source and destination are the *same* directory therefore needs REFER
only on that one directory, which a writable root has. The Linux native
qualification proves only the **cross-boundary** case is denied (C001);
the same-directory case is permitted. macOS Seatbelt re-allows `file-write*`
for the writable roots (`src/security/sandbox/seatbelt.rs:105-109`), so the
same is permitted there. A model can run `ln a b` inside its own scratch under
Landlock Required and permanently pin that session's whole tree against
reclamation, while Windows is immune to this because it has no link-count
accessor at this crate's MSRV.

This is the reason the census reports the **reclaimability verdict** and not
only sizes and counts. A pinned tree is invisible to any measurement that
reports only totals, and the disposition for a tree reclamation can never
accept belongs to the successor retention milestone — not to this one, which
reclaims nothing.

**No measurement surface exists.**

- There is no `scratch` key in `crates/codegg-config/src/schema.rs` and no
  scratch metric anywhere in `src/util/`.
- `src/util/metrics.rs` provides a generic counter/gauge/histogram registry,
  but **it has no consumer anywhere in the workspace** — a gauge published
  into it would be an unobservable claim. This milestone therefore does not
  publish into it (see §5).
- There is no CLI or protocol surface for scratch usage.

**The road map already asks for this.** `plans/subsystems/managed-agent-scratch-roadmap.md` §8
requires "bounded path-safe operator diagnostics, usage and stale lease
accounting", and constrains the implementation with "no global scan per tool
call; no size traversal on agent hot path; lazy creation and batched or
periodic cleanup". M006 is that requirement, scoped to measurement only.

**Protocol and CLI facts the implementation needs.**

- `CoreRequest` / `CoreResponse` are typed enums in
  `crates/codegg-protocol/src/core.rs:2090` and `:1050`; the daemon
  dispatches on them in `src/core/daemon.rs`.
- `PROTOCOL_VERSION` is `2` (`crates/codegg-protocol/src/core.rs:29`).
- The socket server decodes each frame with
  `if let Ok(frame) = serde_json::from_str::<CoreFrame>(trimmed)`
  (`src/core/transport/daemon_socket.rs:445`) and has **no `else` branch**:
  a frame that does not deserialize is **silently dropped**. A new client
  talking to an older daemon therefore gets no reply at all, which is why
  §9 requires a bounded client timeout rather than an unbounded wait.
- The daemon CLI registry is `enum DaemonCommand` in `src/main.rs:618-665`
  (`Start`, `Stop`, `Status`, `Logs`, `Attach`); the operator-facing table is
  `docs/daemon.md:114-128`.
- `codegg daemon status` already reports "active sessions, and connected
  clients" (`docs/daemon.md:120`), and reaches the daemon through
  `SocketCoreClient::connect` (`src/main.rs:1584-1597`).

## 4. Invariants that must not regress

- **The census is read-only.** It must not create, rename, unlink, chmod,
  truncate, or open-for-write anything under the scratch root, and it must
  not create a tombstone. A before/after manifest of the root must be
  identical across a census.
- **The census never follows a link and never reads content.** Only
  `symlink_metadata` is used. A symlink is reported, never traversed; a
  regular file is measured by `metadata().len()`, never opened.
- **No file content, file name outside the tree, transcript body, credential,
  or absolute host path appears in any report.** Relative paths within a tree
  may appear, bounded in count and length (§7 WP1). The session identity does
  not appear: it is not recoverable from the tree by design, and this
  milestone must not invert the digest to manufacture one.
- **Reclamation behaviour is unchanged.** A tree the census reports as
  blocked is still blocked; the census does not quarantine, unquarantine, or
  otherwise influence the reclaim path.
- **No size traversal on the agent hot path.** No tool call, permission
  check, turn admission, or scheduler decision may walk the tree.
- **One digest computation.** The tree name remains
  `hex(sha256(session_id))` and the run/shared names remain
  `hex(sha256("<scope>:<id>"))`. The census must reuse the existing
  computation; a second, divergent copy would silently break the mapping
  that M007 depends on.
- **Daemon-bound code does not consult the process CWD**
  (`scripts/check_daemon_cwd_usage.py` scans `src/core`).
- **`codegg-core` stays free of UI, server, plugin, and auth concerns**
  (`scripts/check-core-boundary.sh`). The filesystem walk belongs in
  `src/scratch.rs`; the durable side belongs in `codegg-core`. This
  milestone needs no durable side, so no `codegg-core` change is expected.
- **The disposable-cache contract is preserved verbatim.** The census is
  added *around* "may be removed by host cache maintenance"; it does not
  narrow or widen what host cache maintenance may do.

## 5. Scope

### In scope

- A read-only, bounded, symlink-safe census of the scratch tree in
  `src/scratch.rs`, reporting per tree: opaque digest, bytes, file/dir
  counts, symlink count, multiply-linked-file count, non-regular-file count,
  oldest and newest modification time, and a reclaimability verdict
  computed with the same rules the reclaim validator applies.
- A daemon-held report, refreshed **off the hot path** on a fixed internal
  cadence, with a bounded per-refresh budget.
- An additive `CoreRequest`/`CoreResponse` pair and the daemon dispatch arm
  that serves the report.
- `codegg daemon scratch-usage` (human table by default, `--json`), and a
  one-line scratch usage summary added to `codegg daemon status`.
- Documentation: `architecture/security.md`, `docs/daemon.md`, `docs/tools.md`,
  and `architecture/process-tool-execution-ownership.md`.
- Focused unit and integration tests, including the negative cases (planted
  symlink, planted FIFO, multiply-linked file, vanished tree, budget
  truncation).

### Explicitly out of scope

- **Any deletion, reclamation, sweep, tombstone creation, or retention
  enforcement.** That is M007. M006 must not reclaim a byte.
- **Any configuration key.** A `scratch` config section is deliberately
  deferred to M007, which is the milestone that needs a policy. Adding one
  here would mean a schema struct, a `merge_configs` arm, and the
  `scripts/check_config_merge_coverage.py` obligation — for a knob with no
  consumer yet.
- **Publishing into `src/util/metrics.rs`.** That registry has no consumer
  in the workspace; a gauge with no reader is an unobservable claim. The two
  operator surfaces in §1 are the observable deliverable. If a metrics
  consumer is added later, M007 may publish to it.
- **Any durable-to-disk identity mapping.** The census reports the opaque
  digest. Deciding whether a digest belongs to a live session requires the
  durable store, and that is M007's seam.
- **Windows native sandbox qualification**, which the operator has
  explicitly deferred to a separate workstream. The census is portable and
  the report states per-platform reclaimability; it claims no Windows
  containment.
- **Changing the scratch layout, the identity rules, or the reclaim
  validator.**
- **Any change to what a model, a tool, or a human may do.** M006 is
  observation only.

## 6. Required production changes

### Core/domain

None expected. The census is a filesystem capability and belongs to
`src/scratch.rs`, which already owns the tree, its identity rules, and the
reclaim validator. If an owner check, a size helper, or a reclaimability
predicate is already derivable from the existing code, reuse it rather than
introducing a parallel notion of "safe to remove".

### Storage and migrations

None. No schema change, no `ALTER TABLE`, no `STORAGE_LAYOUT_VERSION` bump —
`scripts/check_project_catalog_invariants.py` is therefore not triggered.
The census reads only the filesystem.

### Protocol and DTOs

- Add one `CoreRequest` variant that asks for the scratch usage report. The
  request carries no path, no session identity, and no scope: the daemon
  always reports the single per-user scratch root it owns, so a caller cannot
  select which tree is measured.
- Add one `CoreResponse` variant carrying the report DTO.
- The DTOs carry counts, sizes, ages, the opaque digest, and bounded relative
  paths only. They carry no absolute path, no session id, and no content.
- Keep the request **additive** and the response typed. Do not widen an
  existing request.

### Runtime and concurrency

- The refresh runs on a fixed internal interval owned by the daemon
  construction, modelled on the existing 300 s projection maintenance tick
  (`src/core/daemon_construct.rs:279-291`). It is **not** the projection
  tick's business: that owner is the projection retention service, and
  ADR-0039 already rejected coupling unrelated concerns to it.
- The refresh is a plain `tokio::spawn`ed task holding the last report in an
  `Arc<RwLock<Option<Report>>>`; it never blocks a request path.
- The report request returns the last completed report. A caller may ask for
  a fresh walk; the daemon serialises walks so two callers cannot walk the
  tree concurrently.
- The walk is bounded: a maximum number of entries visited, a maximum depth,
  and a maximum number of reported paths per tree. When a bound is hit the
  report says so explicitly (`truncated`) rather than reporting a partial
  total as if it were complete.
- A tree that vanishes mid-walk (a concurrent reclamation removed it) is
  reported as vanished and does not fail the walk. A tree that appears
  mid-walk is reported with its size as observed. Both are normal outcomes of
  a read-only walk over a live tree.

### Frontend or operator surface

- `codegg daemon scratch-usage` — human table by default, `--json` for
  scripting, `--endpoint` to match the other lifecycle commands.
- `codegg daemon status` — one added line summarising scratch usage.
- The daemon must answer the request even when the scratch root does not
  exist (a fresh install): an empty report, not an error.

### Security and authorization

- The report is served over the **local daemon socket only**, exactly like
  the other daemon lifecycle commands, and it is not exposed on the remote
  server surface.
- The daemon reports the tree **it** owns. A request cannot name a path, a
  session, or a digest to inspect.
- The walk reads metadata only and never opens a file for reading, so a
  hostile or malformed tree cannot cause content disclosure.
- The walk must tolerate a hostile tree: a symlink loop, a FIFO, a device
  node, a directory without read permission, and a file that grows during the
  walk must all degrade to a reported anomaly rather than hang or crash the
  daemon. A FIFO must not be opened, because opening it would block.

## 7. Ordered work packages

### WP1 — The census walker in `src/scratch.rs`

Intent: one read-only function that describes the scratch tree.

Required changes: a census entry point that enumerates
`<cache>/codegg/scratch/sessions/`, and for each tree reports the fields
listed in §5. It must reuse the existing digest computation and the existing
owner check. It must implement the reclaimability verdict with the *same*
rules as `validate_reclaim_tree` — best expressed by sharing that rule with
the validator rather than restating it, so the report cannot drift from the
behaviour it describes.

Acceptance evidence: unit tests covering an empty root, a populated tree, a
planted symlink (not followed, counted), a planted FIFO (not opened, counted,
does not hang), a multiply-linked file (counted, tree reported not
reclaimable), a tree that vanishes mid-walk, and a budget-truncated walk
that reports `truncated`.

### WP2 — The daemon-held report

Intent: keep the last completed census available without a walk on the
request path.

Required changes: a daemon-held report refreshed on the internal cadence,
with the bounded-walk budget, and a request handler that returns the last
report. An empty or missing scratch root yields an empty report.

Acceptance evidence: the daemon reports a census that matches a directly
computed census of the same tree; a walk never runs inside a tool call or a
permission check (asserted by a test that exercises the tool path and
observes no walk).

### WP3 — The protocol variant and dispatch

Intent: let a client ask for the report over the local socket.

Required changes: the `CoreRequest` and `CoreResponse` variants and their
DTOs in `crates/codegg-protocol`, and the dispatch arm in `src/core/daemon.rs`.

Acceptance evidence: a socket round-trip test against a live daemon returns
the report; an unknown or malformed request still yields the daemon's normal
error rather than a hang.

### WP4 — The operator surfaces

Intent: make the measurement reachable by an operator.

Required changes: the `codegg daemon scratch-usage` subcommand in
`src/main.rs` (modelled on `codegg daemon status`), its `--json` form, and
the one-line summary added to `codegg daemon status`. The client must apply
a **bounded** timeout: because the socket server drops an undecodable frame
silently (`src/core/transport/daemon_socket.rs:445`), a client talking to a
daemon that predates this request must receive a clear "this daemon does not
support scratch usage" error rather than hanging.

Acceptance evidence: a CLI test against a running daemon prints the report;
the `--json` form parses as JSON and contains no absolute path and no file
content; a client against a daemon without the request fails with the
bounded error.

### WP5 — Documentation and registry reconciliation

Intent: keep the contract and the planning surface truthful.

Required changes: `architecture/security.md` (the observability paragraph and
the reclaimability reporting), `docs/daemon.md` (the lifecycle-command
table), `docs/tools.md` (the disposable-cache paragraph, extended to say the
tree is measurable but not yet swept), and
`architecture/process-tool-execution-ownership.md` (the disposable-cache
wording, preserved). The subsystem roadmap gains the M006 row and its
milestone section; `plans/registry.md` gains the M006 row.

Acceptance evidence: `codegg validate --config codegg.example.jsonc` still
passes; the documentation skill's doc-surface review is satisfied; no
document claims a reclamation capability that M006 does not deliver.

## 8. Failure, cancellation, restart, contention semantics

- **A failed walk** must not clear the previous report. The daemon keeps the
  last successful report and records the failure; the operator sees the age
  of the report it is reading.
- **A cancelled walk** (daemon shutdown) leaves the previous report in
  place and is not an error.
- **Daemon restart** clears the in-memory report; the first refresh rebuilds
  it. Nothing durable depends on it.
- **Contention with reclamation**: a tree removed mid-walk is reported as
  vanished. A tree created mid-walk is reported as observed. The walk never
  blocks a reclaim and a reclaim never blocks the walk; the only shared
  resource is the filesystem, and both use metadata-only operations.
- **Contention with a live turn**: a turn writing into its scratch tree
  changes the tree's size mid-walk. The report is a point-in-time
  observation and says so; a size that moves between two reads is not
  treated as an error.
- **Budget exhaustion** produces a report flagged `truncated`, with the
  budget that was hit. A truncated report is never presented as a complete
  measurement.

## 9. Compatibility and migration

- **No storage change.** No migration, no `STORAGE_LAYOUT_VERSION` bump, no
  schema or `codegg-core` change. Existing databases are untouched.
- **No configuration change.** No new key, therefore no
  `scripts/check_config_merge_coverage.py` obligation is created.
- **The protocol change is additive.** `PROTOCOL_VERSION` stays `2`: the
  request is a new variant of an existing enum, and a client only sends it
  deliberately. `PROTOCOL_VERSION` must be bumped **only** if the
  implementation cannot keep the request decodable by an older peer — the
  plan's default is to keep it at 2 and make the client degrade gracefully.
- **Version skew across a daemon upgrade** is the one real compatibility
  case. Because the socket server silently drops a frame that does not
  deserialize (`src/core/transport/daemon_socket.rs:445`), a new client
  against an older daemon receives **no response**. The client must therefore
  use a bounded request timeout and report a typed "the running daemon does
  not support scratch usage; restart it to upgrade" error. Hanging is not an
  acceptable degradation.
- **The scratch layout, identity rules, and reclaim validator are unchanged**,
  so no existing scratch tree is invalidated and no migration of on-disk
  state is implied.

## 10. Required tests

- `src/scratch.rs` unit tests: empty root; populated tree; symlink not
  followed and counted; FIFO not opened and counted, and the walk does not
  hang; multiply-linked file counted and the tree reported not reclaimable;
  non-regular file counted; tree vanished mid-walk; budget truncation sets
  `truncated`; the digest used for the tree name matches the existing
  computation.
- `src/core/daemon.rs` (or the transport integration tests): socket
  round-trip of the new request; the request cannot select a path or
  identity; a missing scratch root yields an empty report, not an error; a
  daemon that fails a walk keeps the previous report and reports its age.
- `tests/`: a CLI-level test of `codegg daemon scratch-usage` against a live
  daemon, asserting the human table and the `--json` form; a test that the
  `--json` form contains no absolute path and no file content; a test that a
  client against a daemon without the request fails with the bounded error
  rather than hanging.
- **Negative tests that matter most**: a planted symlink pointing outside the
  tree is not traversed and the outside file's content does not appear
  anywhere in the report; a planted FIFO does not block the daemon; a tree
  with a multiply-linked file is reported as not reclaimable.
- **Regression**: the existing reclamation tests in `src/scratch.rs` and
  `src/core/daemon_sessions.rs` (`scratch_cleanup_tests`) still pass
  unchanged — the census must not disturb the reclaim path.
  `tests/linux_native_scratch_containment.rs` is unaffected and remains a
  Linux-only native qualification.
- **Negative control**: a test that proves the census cannot reclaim — assert
  that a full census leaves the tree byte-for-byte and entry-for-entry
  identical.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/worktree/codegg
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg --lib -E 'test(/scratch/)'
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg --test agent_loop_harness
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg -E 'test(/scratch/) or test(/reclaim/) or test(/daemon/)'
scripts/verify.sh quick
```

`scripts/verify.sh quick` is required, not optional: it carries the
`check_daemon_cwd_usage`, `check-core-boundary`, `check_execution_ownership`,
`check_config_merge_coverage`, and `check_project_catalog_invariants` guards
that a change in this area can trip. Note that CI is a **subset** of
`quick`; a green CI run does not imply a green `quick`.

## 12. Documentation updates

- `architecture/security.md` — the observability paragraph: what the census
  reports, that it is read-only, that it never follows a link, and that
  reclaimability in the report is the validator's own verdict. The
  disposable-cache wording is preserved.
- `architecture/process-tool-execution-ownership.md` — the disposable-cache
  paragraph is preserved verbatim; the census is mentioned as an observation
  that does not change the contract.
- `docs/tools.md` — the scratch paragraph gains one sentence: the tree is
  measurable with `codegg daemon scratch-usage`, and it is **not** yet swept
  by age or size.
- `docs/daemon.md` — the lifecycle-command table gains
  `codegg daemon scratch-usage`; `codegg daemon status` gains the summary
  line.
- `plans/subsystems/managed-agent-scratch-roadmap.md` — the M006 milestone
  section and the §12 status row.
- `plans/registry.md` — the M006 row.

## 13. Acceptance criteria

These are externally observable and are what the closure record must be able
to point at.

1. `codegg daemon scratch-usage` prints a report naming the total bytes, the
   tree count, the age range, and — per tree — the opaque digest, the size,
   the counts, and whether reclamation would accept that tree.
2. A tree containing a multiply-linked file is reported as **not
   reclaimable**, with the reason, and the report says which rule refused it.
3. `codegg daemon status` shows the same usage in one line.
4. A before/after manifest of the scratch root taken around a full census is
   **identical**: no file created, renamed, removed, or changed, and no
   tombstone appeared.
5. The report and the status line contain no absolute path, no file content,
   no session identity, and no transcript body.
6. A planted symlink pointing outside the tree is counted and not traversed,
   and the outside file's content appears nowhere in the output.
7. A planted FIFO does not hang the daemon and does not appear as a
   hang.
8. No tool call, permission check, or turn admission performs a size
   traversal; a test observes that the tool path performs no walk.
9. `scripts/verify.sh quick` and the focused suites pass, and the hosted CI
   run on the pull-request head is green.
10. The closure record states plainly that **nothing is reclaimed by this
    milestone**, and that the growth of a never-deleted session's tree is
    still unbounded.

## 14. Stop conditions

The implementing agent must **stop and report** rather than improvise if:

- Meeting an acceptance criterion would require following a symlink, reading
  file content, or opening a file that is not a regular file.
- The census would need to mutate the tree to answer a question.
- A question can only be answered by mapping a digest to a session
  identity — that is M007's seam, not this milestone's.
- The report would have to carry an absolute host path, a session identity,
  or unbounded relative paths to be useful.
- A reclamation behaviour would have to change for the report to be
  truthful.
- Any claim would require Windows native sandbox qualification. This
  milestone claims no Windows containment.
- A configuration key appears to be necessary. Defer it to M007 rather than
  adding a knob with no consumer.
- The metrics registry would need a consumer for a gauge to be observable.
  The operator surfaces are the deliverable; a gauge with no reader is not.

## 15. Closure evidence required

- The `codegg daemon scratch-usage` output on a real tree, including a tree
  that reclamation would refuse, and the `--json` form.
- The `codegg daemon status` summary line.
- A before/after manifest proving the census changed nothing.
- The focused test results, including every negative test in §10.
- `scripts/verify.sh quick` green, and the hosted CI run ID on the
  pull-request head and on the merged head.
- An explicit statement that no reclamation capability was delivered.
- The registry and roadmap reconciled with the closure record's status.

## 16. Handoff notes

- The single most important property is that this milestone **cannot
  reclaim**. If an implementation path appears to need to delete, quarantine,
  or rename anything to produce its report, that is a scope error: stop.
- The reclaimability verdict must be **shared** with `validate_reclaim_tree`
  rather than restated, so the report cannot drift from the behaviour it
  describes. A report that says "reclaimable" for a tree the validator would
  refuse is worse than no report.
- The one-way digest is a deliberate property, not an obstacle to work
  around. The census reports the opaque digest; M007 maps digests to live
  sessions by hashing the durable session ids, in the other direction.
- M007 is the successor this milestone exists to enable: a retention policy
  and a session scratch sweep, sized from real measurements, with a
  session-scratch lease to close the race against a live turn's `TMPDIR`, and
  a disposition for a tree that reclamation can never accept. M006 must not
  pre-empt any of that.
- The measured evidence for this milestone is the isolated daemon's scratch
  tree (15 durable sessions, none soft-deleted, 14 trees, 132 KB, nothing
  reclaimed). A closure record that quotes a different number is quoting a
  different environment and must say so.
