# Managed Agent Scratch Milestone M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Successor milestone:

- `plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md`
  — M007 consumes this milestone's census as its first work package

Repository baseline reviewed: `2a0bae08` on `main`

## 1. Executive finding

M006 delivered the measurement it was scoped to deliver, and it delivered
**nothing else**. The daemon can now measure its own disposable scratch tree
and report the result to an operator, through `codegg daemon scratch-usage`
and one line of `codegg daemon status`.

The measurement is **read-only by construction**. The census creates nothing,
renames nothing, unlinks nothing, opens no file for reading, and never
follows a link: only `symlink_metadata` and directory listing are used. A
before/after manifest of the scratch root taken around a full census is
identical — same names, same sizes, same mtimes, and no reclaim tombstone
appeared.

The report's reclaimability verdict is **the reclaim validator's own verdict**,
not a restatement of it. The rule that decides whether one entry may be
removed was factored into a single function, `classify_reclaim_entry`, and
both `validate_reclaim_tree` and the census consult it. A report therefore
cannot claim a tree is reclaimable when the validator would refuse it. This
was not a paper property: the first implementation of the census had its own
inline classification and the unit test
`census_reports_a_multiply_linked_file_as_a_refusal` caught exactly that
drift, which is why the census now asks the shared rule.

**Nothing is reclaimed by this milestone.** The growth of a session tree that
is never soft-deleted remains **unbounded**, and the absence of a retention
sweep and of a retention policy is unchanged. M006 adds no configuration key,
no durable state, and no storage migration, and it claims no Windows
containment. It exists to produce the numbers that M007's retention policy
needs, and to make a tree that reclamation can never accept visible instead of
silent.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| A census enumerates the daemon's sessions root and reports per-tree counts, sizes, and ages | `scratch::tests::census_measures_a_tree_without_mutating_it` | pass — 2 files, 1 directory, 11 bytes, an age range, and the tree's opaque digest |
| An empty or missing scratch root is an empty report, not an error | `scratch::tests::census_treats_a_missing_root_as_an_empty_report` | pass — zero trees, `truncated` false |
| A planted symlink is counted and never traversed | `scratch::tests::census_reports_a_planted_symlink_as_a_refusal_without_traversing_it` | pass — `symlink_count` 1, refusal `symbolic_link`, the outside file is intact, and its content appears nowhere in the report |
| A planted FIFO is counted and never opened | `scratch::tests::census_reports_a_fifo_without_opening_it` | pass — `non_regular_count` 1, refusal `non_regular_entry`; opening a FIFO would block until a writer appeared |
| A multiply-linked file is counted and the tree reported not reclaimable | `scratch::tests::census_reports_a_multiply_linked_file_as_a_refusal` | pass — `multiply_linked_count` 1, refusal `multiply_linked_file`, `reclaimable` false |
| A tree that vanishes mid-walk is reported as vanished | `scratch::tests::census_reports_a_tree_that_vanished_mid_walk` | pass — `vanished` true, no bytes counted |
| A budget-truncated walk reports `truncated` | `scratch::tests::census_stops_at_its_tree_budget_and_says_so`; `scratch::tests::census_stops_at_its_entry_budget_and_says_so` | pass — the tree budget reports `unreported_trees` 1; the entry budget flags the tree |
| The tree name is the existing digest computation | `scratch::tests::the_digest_names_the_tree_and_cannot_be_inverted` | pass — `session_scratch_digest_hex` matches the tree name and cannot be inverted to the session id |
| The census cannot reclaim | `scratch::tests::a_full_census_leaves_the_root_entry_for_entry_identical`; `scratch_usage_census::the_census_reclaims_nothing` | pass — the before/after manifest is identical, and three refreshed censuses leave the tree and its content untouched with no tombstone |
| The report and the status line disclose no path, content, file name, or session identity | `scratch_usage_census::scratch_usage_reports_the_daemon_s_own_tree_without_leaking_it` | pass — the `--json` form parses, names the tree by digest with its byte count, and contains no absolute path, no file content, no file name, and no session id |
| `codegg daemon scratch-usage` prints the report | `scratch_usage_census::scratch_usage_reports_the_daemon_s_own_tree_without_leaking_it` | pass — the human form prints the header and the tree's digest |
| `codegg daemon status` shows the same usage in one line | `scratch_usage_census::scratch_usage_reports_the_daemon_s_own_tree_without_leaking_it` | pass — the status output carries a `Scratch:` line and no absolute path |
| A client talking to a daemon that cannot answer fails with a bounded error instead of hanging | `transport::socket::tests::a_bounded_request_reports_a_silent_peer_instead_of_hanging` | pass — a peer that completes the handshake and never answers produces `BoundedRequestError::TimedOut` inside the bound |
| The request is served on the local socket only | `daemon_family::tests::scratch_census_answers_from_the_daemon_and_stays_local` | pass — `CoreRequest::ScratchUsage` is `is_local_only()`, and the remote WebSocket transport denies it with `local_operation_remote_denied` |

## 3. Production implementation evidence

| File | Change |
|---|---|
| `src/scratch.rs` | `session_scratch_digest` / `session_scratch_digest_hex` — the one digest computation; `classify_reclaim_entry` — the one entry rule, now shared with `validate_reclaim_tree`; `ReclaimRefusal` — the refusal taxonomy; `ScratchCensusBudget`, `ScratchTreeUsage`, `ScratchUsageReport` — the report types; `scratch_usage_census` — the bounded walk; `enumerate_session_scratch_trees` — the tree enumerator |
| `crates/codegg-protocol/src/scratch.rs` | The wire DTOs: `ScratchUsageRequestDto`, `ScratchUsageReportDto`, `ScratchTreeUsageDto`, `ScratchReclaimRefusalDto` |
| `crates/codegg-protocol/src/core.rs` | `CoreRequest::ScratchUsage`, `CoreResponse::ScratchUsage`, and `CoreRequest::is_local_only` |
| `src/core/daemon_scratch.rs` | The daemon-held snapshot (`ScratchCensus`), the refresh task, and the family handler |
| `src/core/daemon_family.rs` | The `Scratch` family, its routing arm, and its owner-module row |
| `src/core/daemon.rs` | The `scratch_census` field and the family dispatch arm |
| `src/core/daemon_construct.rs` | The census state and the refresh task's spawn, on its own interval and its own handle |
| `src/core/transport/socket.rs` | `SocketCoreClient::request_bounded` and `BoundedRequestError` |
| `src/main.rs` | `codegg daemon scratch-usage` with `--json` and `--refresh`, the bounded fetch, the human formatter, and the `daemon status` summary line |
| `src/server/ws.rs` | The remote gate: a local-only request is denied on the WebSocket transport |
| `crates/codegg-core/src/authorization/policy.rs` | The `scratch_usage` operation descriptor — `Opaque`, no capability |
| `crates/codegg-core/src/audit_instrumentation.rs` | `scratch_usage` listed as explicitly uninstrumented: it is a bounded read of disposable state that mutates nothing |

## 4. Verification executed

Local (macOS, rustc 1.89.0, load-averaged host):

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib scratch::` | pass — 23 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib daemon_family` | pass |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --test scratch_usage_census` | pass |
| `scripts/verify.sh quick` | pass — every guard green, and `cargo check --workspace --all-targets --locked` clean |

Hosted CI on the pull-request head: recorded in this section after the run completes.

## 5. Invariant review

- **The disposable-cache contract is preserved verbatim.** "Cache contents are
  disposable and may be removed by host cache maintenance" is unchanged. The
  census observes the tree; it does not promise the tree's survival, and it
  does not create one.
- **No size traversal on the agent hot path.** The walk runs on a background
  timer (600 s) and, on demand, on an explicit operator request. No tool call,
  permission check, or turn admission consults it.
- **No global scan per tool call.** The census is never invoked from the tool
  path at all.
- **The reclaim path is unchanged.** `validate_reclaim_tree` still refuses a
  symlink, a non-regular entry, and — on Unix — a multiply-linked regular
  file. The existing reclamation tests in `src/scratch.rs` and
  `src/core/daemon_sessions.rs` pass unchanged.
- **The digest computation is now single-sourced.** `session_scratch_dir`,
  `existing_session_scratch_dir`, and `reclaim_deleted_session_scratch_dir`
  all call `session_scratch_digest`. A second copy is how the durable-to-disk
  mapping that M007 depends on silently breaks.
- **The entry rule is now single-sourced.** `validate_reclaim_tree` and the
  census both call `classify_reclaim_entry`.

## 6. Failure and recovery review

- **A failed walk does not clear the previous report.** The refresh task logs
  the failure and leaves the last completed measurement in place, so the
  operator sees the age of the report it is reading rather than an empty
  report that looks like "no scratch".
- **A cancelled walk is not an error.** The refresh task is a spawned task
  with no authority; a daemon shutdown simply ends it.
- **Daemon restart clears the in-memory report.** Nothing durable depends on
  it; the first refresh rebuilds it.
- **Contention with reclamation.** A tree removed mid-walk is reported as
  `vanished`. A tree created mid-walk is reported as observed. The walk and
  the reclaim share only the filesystem, and both use metadata-only
  operations.
- **Contention with a live turn.** A turn writing into its tree changes the
  tree's size mid-walk. The report is a point-in-time observation and says
  so; a size that moves between two reads is not an error.
- **Budget exhaustion** produces a report flagged `truncated`, with the number
  of trees the walk did not reach. A truncated report is never presented as a
  complete measurement.

## 7. Migration and compatibility review

- **No storage change.** No migration, no `STORAGE_LAYOUT_VERSION` bump, no
  schema or `codegg-core` storage change. Existing databases are untouched.
- **No configuration change.** No new key, so no
  `scripts/check_config_merge_coverage.py` obligation is created.
- **The protocol change is additive.** `PROTOCOL_VERSION` stays `2`: the
  request is a new variant of an existing enum, and a client only sends it
  deliberately.
- **Version skew across a daemon upgrade** is the one real compatibility
  case. The socket server silently drops a frame it cannot decode, so a new
  client against an older daemon receives **no response**. The client
  therefore uses a bounded request and reports a typed "the running daemon
  does not support scratch usage; restart it to upgrade" error. Hanging is
  not an acceptable degradation, and
  `a_bounded_request_reports_a_silent_peer_instead_of_hanging` proves the
  bound.
- **The scratch layout, identity rules, and reclaim validator are unchanged**,
  so no existing scratch tree is invalidated.

## 8. Security review

- **The census reads only what the daemon owns.** It resolves the sessions
  root itself and enumerates it; a request cannot name a path, a session, or
  a digest to inspect. The request carries one boolean.
- **No link is followed.** A symlink is counted from its own metadata and
  never traversed; the outside target is neither read nor counted.
- **No content is read.** Only `symlink_metadata` and directory listing are
  used. A regular file is measured by its metadata length and never opened.
- **No special file is opened.** A FIFO is counted from metadata; opening one
  would block until a writer appeared.
- **Disclosure is bounded.** The report carries counts, sizes, ages, and the
  opaque one-way digest. It carries no absolute path, no file name, no file
  content, no session identity, and no transcript body. The digest is
  one-way by design, so a session id cannot be recovered from it.
- **The operation is scoped `Opaque` with no capability.** No project linkage
  exists, so team principals fail closed and only the local-owner broad
  policy authorizes. It is a bounded read of disposable state and mutates
  nothing, so it claims no capability.
- **The operation is local-only.** `CoreRequest::is_local_only` gates the
  remote WebSocket transport: a remote client is authenticated to the
  server, not to this machine's owner, and never reaches the census.
- **The census is not audited as a mutation.** It is listed in
  `UNINSTRUMENTED_OPERATIONS` with its reason: a bounded read that mutates
  nothing does not earn a dedicated audit action, and denial *is* audited
  upstream by `authorize_request`.

## 9. Documentation and operations

- `architecture/security.md` — the scratch census paragraph: what it reports,
  that it is read-only, that it never follows a link, and that the report's
  reclaimability is the validator's own verdict. The disposable-cache wording
  is preserved.
- `architecture/process-tool-execution-ownership.md` — the disposable-cache
  paragraph is preserved verbatim; the census is mentioned as an observation
  that does not change the contract.
- `docs/tools.md` — the scratch paragraph gains one sentence: the tree is
  measurable, and it is **not** yet swept by age or by size.
- `docs/daemon.md` — the lifecycle-command table gains
  `codegg daemon scratch-usage`; `codegg daemon status` gains the summary
  line.
- `architecture/authorization.md` — the operation-to-capability matrix gains
  the `scratch_usage` row.
- `architecture/overview.md` — the `## Verified counts` table is remeasured:
  `CoreRequest` variants **260** (236 struct + 24 unit), `CoreResponse`
  variants **170**, and authorized operations **254**. Each was remeasured
  against its listed `Source` with the counting method the table has always
  used, and each was confirmed to have been correct before this milestone.

## 10. Unresolved findings

None that block the milestone. Two facts are recorded so that the successor
does not have to rediscover them:

- **M007's retention default is still *specified*, not *measured*.** The
  14-day default is derived from the disposal contract. M007's WP0 runs this
  census; only if that run happens may a closure claim the default is
  *validated*.
- **The Unix-only multiply-linked refusal still pins a tree.** The census
  reports it — `multiply_linked_file` — so it is now visible rather than
  silent. M007's WP1 removes the refusal; M006 deliberately does not, because
  removing it is a different milestone's change.

## 11. Roadmap disposition

`plans/subsystems/managed-agent-scratch-roadmap.md` §7's M006 section moves
from `ready` to `closed` and names this record; §12's M006 row carries the
same status and the closure path. The M007 dependency text is reconciled:
M006's census output was a named work-package input, and it is now an
implemented input rather than a pending one.

## 12. Registry updates

`plans/registry.md`: the subsystem row now reads `M001/M002/M004/M005/M006
closed; M003 conditional; M007 ready`; the M006 row moves to **closed** and
names this record; the M007 row's M006 dependency text is reconciled with the
new state.

## 13. Factual corrections

### 2026-10-21 — a partially-read tree could report itself reclaimable

Corrected after the fact by a follow-up change. The historical sections
above are left as they were written; this records what was wrong and what
now holds.

**What was wrong.** §1 states that "a report therefore cannot claim a tree
is reclaimable when the validator would refuse it". That is true of every
entry the census *read* — the rule is shared, so there is no drift. It was
not true of a tree the census did not finish reading. `ScratchTreeUsage`'s
`reclaimable` was a plain `bool`, assigned unconditionally at the end of the
walk as `refusal.is_none()`. A walk that hit the entry budget stopped early,
so the entries past the budget were never classified — but the tree was
reported `reclaimable: true` anyway, on the strength of the entries that
*were* read.

The consequence is a report that overstates what is known: it tells an
operator the reclaim path would accept a tree whose unread part might hold
exactly the entry that would make the reclaim path refuse it. The same
applied to a tree that vanished while it was being read.

This was not a paper property. A probe added to
`census_stops_at_its_entry_budget_and_says_so` — a test that already
asserted `truncated` and the entry count, but said nothing about the
verdict — failed against `main`:

```
PROBE: a truncated tree reported reclaimable=true (truncated=true)
```

**What now holds.** The verdict is tri-state: `Option<bool>`, where `None`
means the verdict could not be established. A truncated tree and a vanished
tree both carry `None`; a tree the walk read in full carries `Some(true)` or
`Some(false)` from the shared rule. A partial walk never yields
`Some(true)`, so the report can no longer claim a tree is reclaimable on the
strength of the part it managed to read.

The wire shape changed with it — `ScratchTreeUsageDto.reclaimable` is now
`Option<bool>`, and `SCRATCH_PROTOCOL_VERSION` moved from 1 to 2. The
operator surface reports the three states as `yes`, `no`, and `unknown`, and
counts the unknown trees separately from the refused ones. The retention
sweep's consumer of the same verdict is fail-closed: only an established
`Some(true)` counts as reclaimable, so an unknown verdict is never a
permission to reclaim.

**What this correction does not change.** The removal path is unaffected
either way: the sweep and the operator reclaim both re-run the authoritative
validator (`reclaim_session_scratch_for_operator`) before anything is
removed, so the census verdict never authorised a removal. This was a
reporting-accuracy defect, not a data-loss one. The property §1 claims —
that the verdict is the validator's own rule rather than a restatement of it
— was and remains true.

## 14. Handoff notes

- **M007's WP0 can now run.** The census exists, is read-only, and is
  reachable through `codegg daemon scratch-usage --json` against a live
  daemon. The census WP0 needs is the *measured* input — the age
  distribution, the byte distribution, the tree count, and the count of trees
  the reclaim validator refuses — over a real scratch tree.
- **The census is not the policy.** M006 publishes no default, no threshold,
  and no gauge. A tree measured by this milestone is still reclaimed only by
  the existing event-driven reclaimers.
- **The shared rule is the extension point.** Anything that wants to change
  what "reclaimable" means changes `classify_reclaim_entry`, and the census
  follows automatically. M007's WP1 does exactly that: it removes the
  multiply-linked refusal from that one function, and the census's verdict
  changes with it.
