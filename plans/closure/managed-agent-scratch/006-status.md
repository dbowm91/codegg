# Managed Agent Scratch Milestone M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Predecessor milestones (all closed):

- `plans/closure/managed-agent-scratch/001-status.md`
- `plans/closure/managed-agent-scratch/002-status.md`
- `plans/closure/managed-agent-scratch/003-status.md`
- `plans/closure/managed-agent-scratch/004-status.md`
- `plans/closure/managed-agent-scratch/005-status.md`

Repository baseline reviewed: `cb0e79c0` on `main`

Implementation commits:

- `9c5ae75f` — `feat(scratch): the read-only scratch usage census and its
  operator surfaces`
- `8e4c2ad5` — `docs(scratch,plans): record the M006 census contract and
  close the milestone`
- `67d88db9` — `fix(scratch): make the shared-rule test's link probe
  Unix-only`

## 1. Executive finding

M006 delivers what it promised and nothing beyond it: a **measurement**, not a
policy.

The daemon measures its own per-user scratch tree and keeps the last completed
measurement in memory. An operator reads it with `codegg daemon scratch-usage`
(human table) or `codegg daemon scratch-usage --json` (machine-readable), and
`codegg daemon status` carries the same totals on one line.

The census is **read-only**. It uses `symlink_metadata` only, so a link is
reported and never traversed, a file is measured and never opened, and nothing
is created, renamed, unlinked, chmodded, or written. It never reclaims and it
never creates a tombstone.

Two properties are load-bearing and are enforced by construction rather than
by convention:

- **The reclaim verdict is the reclaim validator's own verdict.** The rule
  that decides whether an entry is reclaimable was extracted into
  `reclaim_entry_refusal` and is now consulted by *both*
  `validate_reclaim_tree` and the census. A report that disagreed with the
  behaviour it describes would be worse than no report, so the two cannot be
  allowed to drift.
- **A partial walk never claims a tree is reclaimable.** Every dimension of
  the walk is bounded, a bound that is reached is reported as `truncated`, and
  a truncated tree's verdict is `None` (unknown), not `Some(true)`.

The walk runs on the daemon's own cadence, **off the hot path**: no tool call,
permission check, turn admission, or scheduler decision performs a size
traversal. The request carries no path and no scope, so it always measures the
single scratch root the daemon owns. Because it resolves no project scope,
every principal other than the local owner fails closed before the walk is
reached.

**What this milestone deliberately does not do.** It adds no retention policy,
no sweep, no session-scratch lease, and no disposition for an unreclaimable
tree. The growth of a session tree that is never soft-deleted remains
unbounded, and that is unchanged by this milestone. The successor policy
cannot be specified until the census has produced real numbers, and it is
deliberately not registered here.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| A census reports the expected counts, sizes, ages, and reclaim verdict | `scratch::tests::census_reports_counts_sizes_and_a_reclaim_verdict` | pass — 3 files, 2 directories, 22 bytes, the age range is present, and the verdict is `Some(true)` |
| A multiply-linked file is reported as **not reclaimable**, with the reason | `scratch::tests::census_reports_a_multiply_linked_file_as_not_reclaimable`; `scratch_usage_census::a_multiply_linked_tree_is_reported_as_not_reclaimable_with_the_reason` | pass — `multiply_linked_count` is 2, the verdict is `Some(false)`, and the rule `multiply_linked` is named |
| A symlink is reported as not reclaimable, and its target is never followed | `scratch::tests::census_reports_a_symlink_as_not_reclaimable` | pass — `link_count` is 1, the verdict is `Some(false)`, the link target's file name never appears, and `file_count` is 0 (the link is not descended into) |
| A non-regular entry is reported as not reclaimable and is never opened | `scratch::tests::census_reports_a_socket_as_not_reclaimable_without_opening_it` | pass — `non_regular_count` is 1, the rule `not_regular` is named, and the walk completed rather than opening the entry |
| A budget-truncated walk reports `truncated` | `scratch::tests::census_reports_a_truncated_walk_when_the_budget_is_exhausted` | pass — the report and the tree both report truncation, and the verdict is `None` |
| A tree that vanishes mid-walk is reported as vanished and does not fail the walk | `scratch::tests::census_reports_a_vanished_tree_without_failing_the_walk` | pass — `vanished` is set, the verdict is `None`, and the walk continues |
| The census never creates, renames, unlinks, or writes | `scratch::tests::census_leaves_the_tree_untouched`; `scratch_usage_census::the_census_leaves_the_tree_byte_identical` | pass — a before/after manifest of names, sizes, and content bytes is identical across a census |
| The report carries no absolute path, no content, and no session identity | `scratch::tests::census_report_carries_no_absolute_path_no_content_and_no_session_id`; `scratch_usage_census::the_report_discloses_no_absolute_path_no_content_and_no_session_identity` | pass — the rendered report contains neither the host path, nor the file's content, nor the file's name |
| Only a refusing path is named; no other file name appears | `scratch::tests::census_reports_a_refusing_path_but_no_other_file_names` | pass — exactly one refusal path is reported and it is the offending entry |
| The session digest is the one the allocator produces | `scratch::tests::the_session_digest_is_the_name_the_allocator_produces` | pass — `session_digest(session_id)` equals the allocated tree's directory name |
| The census's refusal rule and the reclaim validator agree | `scratch::tests::the_census_refusal_rule_agrees_with_the_reclaim_validator` | pass — for each probed shape, `reclaim_entry_refusal(...).is_some()` equals `validate_reclaim_tree(...).is_err()` |
| The daemon serves the report, and a held report is served without a walk | `scratch_usage_census::a_read_without_a_refresh_serves_the_daemon_held_report` | pass — the held report is empty before the first walk, the refresh measures the tree, and the next held read returns the same measurement |
| The report is served over the real socket | `daemon_socket_integration_tests::the_scratch_usage_census_is_served_over_the_socket` | pass — the client hello handshake and the request travel the real Unix-socket byte stream, and the response carries the measured tree |
| A read creates no job and touches no queue | `scratch_usage_census::a_read_creates_no_job_and_touches_no_queue` | pass — the job list is empty after a census read |
| A team principal fails closed on the missing scope | `scratch_usage_census::a_team_principal_fails_closed_on_the_missing_scope` | pass — the response is `authorization_scope_required`, raised before the walk is reached |
| A failed walk keeps the previous report | `daemon_scratch::tests::the_held_report_carries_its_age`; the refresher's `last_error`/`report` split | pass — the held report and the failure are recorded independently, so a transient error cannot erase the last good measurement |
| Two refreshes do not walk the tree concurrently | `daemon_scratch::tests::two_concurrent_refreshes_do_not_walk_the_tree_at_the_same_time` | pass — the walk lock serialises them and both agree |
| The operator surfaces render the measurement | `daemon_scratch::tests::the_human_table_shows_the_totals_the_verdict_and_the_rule_that_refused_it`; `daemon_scratch::tests::the_summary_line_is_one_line_and_counts_the_blocked_trees` | pass — the table names the rule that refused a tree, and the summary is one line that counts the blocked trees |
| The command parses and its flags are explicit | `codegg` bin test `scratch_usage_subcommand_parses_its_flags` | pass — `--json`, `--refresh`, and `--endpoint` parse; the defaults are the held report and the human table |
| The request and response have a stable wire form | `codegg-protocol` tests `the_scratch_usage_request_has_a_stable_wire_form`, `the_scratch_usage_request_defaults_to_the_held_report`, `the_scratch_usage_response_has_a_stable_wire_form`, `the_reclaim_refusal_has_a_stable_wire_form` | pass — `{"type":"scratch_usage","refresh":true}` and `{"type":"scratch_usage","report":{…}}` round-trip, `refresh` defaults to `false`, and the refusal uses a stable snake_case name |

## 3. Production implementation evidence

| File | Change |
|---|---|
| `src/scratch.rs` | `reclaim_entry_refusal` is the shared reclaim rule, consulted by both `validate_reclaim_tree` and the census; `session_digest_bytes`/`session_digest` are the single session-identity computation, now shared by allocation, reclamation, and the census; `census_scratch_usage`/`census_scratch_usage_at` are the bounded, symlink-safe walker; `ScratchCensusBudget`, `ScratchTreeUsage`, `ScratchUsageReport`, and `ReclaimRefusal` are its types |
| `src/core/daemon_scratch.rs` | `ScratchUsageState` (the held report, the last failure, and the last attempt), `ScratchUsageRefresher` (the walk lock and the refresh), `spawn_scratch_usage_refresh` (the cadence task), `handle_scratch_usage_request` (the request handler), and the operator-facing renderers |
| `src/core/daemon.rs` | the `scratch_usage_refresher` and `_scratch_usage_handle` fields, and the `Scratch` dispatch arm |
| `src/core/daemon_construct.rs` | the refresher is constructed with the daemon's root and the refresh task is spawned |
| `src/core/daemon_shutdown.rs` | the refresh task is aborted with the daemon |
| `src/core/daemon_family.rs` | the `Scratch` request family, its owner module, and its doc-table row |
| `src/core/runtime_deps.rs` | `scratch_usage_root` — the construction-time seam that lets a test measure a fixture tree; a client request can never select a root |
| `crates/codegg-protocol/src/core.rs` | the additive `CoreRequest::ScratchUsage { refresh }` and `CoreResponse::ScratchUsage { report }` variants |
| `crates/codegg-protocol/src/dto.rs` | `ScratchUsageReportDto`, `ScratchTreeUsageDto`, `ScratchCensusBudgetDto`, and `ScratchReclaimRefusalDto` |
| `crates/codegg-core/src/authorization/policy.rs` | the `scratch_usage` operation descriptor: opaque scope, `file.read` capability |
| `crates/codegg-core/src/audit_instrumentation.rs` | `scratch_usage` classified as a read for audit purposes |
| `src/main.rs` | the `codegg daemon scratch-usage` command, its bounded request timeout, and the `daemon status` summary line |

### Authority and disclosure

The operation is `ScopeKind::Opaque` with a capability, so it resolves **no**
project scope. `resolve_authorization_project` returns `None` for it, and the
authorization service refuses a missing scope for every principal other than
the local owner. The consequence is recorded rather than assumed: a team
principal is denied with `authorization_scope_required` before the walk is
reached, which `scratch_usage_census::a_team_principal_fails_closed_on_the_missing_scope`
asserts against the real daemon.

The capability is `file.read` because that is the narrowest existing
capability that describes what the census does: it reads file names and sizes
inside a filesystem tree. It never reads file content. The capability choice
does not widen access — the missing scope is what denies — so the descriptor is
a description of the operation, not a grant.

### Audit classification

`scratch_usage` is in `UNINSTRUMENTED_OPERATIONS`, alongside the other
read-only operator views (`node_telemetry_read`, `memory_list`, and the LSP
reads). The census mutates nothing — no file, no tombstone, no job, no queue —
so emitting an audit event per refresh would record a read of the operator's
own daemon state on a timer. Denial **is** audited upstream by
`authorize_request`, which is the security-relevant half.

## 4. Verification executed

Local (macOS, arm64):

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` | pass — 0 findings |
| `cargo test --locked -p codegg-protocol` | pass — **293 passed**, 0 failed |
| `cargo test --locked -p codegg --lib scratch::` | pass — **31 passed**, 0 failed (12 of them new census tests) |
| `cargo test --locked -p codegg --lib daemon_scratch` | pass — **6 passed**, 0 failed |
| `cargo test --locked -p codegg --lib daemon_socket_integration_tests::the_scratch_usage_census_is_served_over_the_socket` | pass — 1 passed |
| `cargo test --locked --test scratch_usage_census` | pass — **7 passed**, 0 failed |
| `scripts/verify.sh quick` | pass — every guard green, including `check_authorization_matrix.py` and `check_audit_coverage.py`, and `cargo check --workspace --all-targets --locked` clean |

The whole-suite sweep and the hosted CI runs are recorded in §13, with the run
IDs and the exact heads they were taken on.

## 5. Invariant review

| Invariant | Result |
|---|---|
| The census reads and never writes | held — only `symlink_metadata` and `read_dir` are used; a before/after manifest is byte-identical across a census |
| A link is reported and never traversed | held — `reclaim_entry_refusal` refuses a link and the walk `continue`s past it, so a link into another tree is never read |
| A FIFO is reported and never opened | held — a non-regular entry is refused and skipped; opening it would block the walk || The reclaim verdict is the validator's verdict | held — one shared rule, asserted directly against the validator |
| A partial walk never claims a tree is reclaimable | held — a truncated tree's verdict is `None` |
| A vanished tree does not fail the walk | held — the tree is reported as vanished and the walk continues |
| The walk never runs on a request path | held — the cadence task owns the periodic walk; the request serves the held report |
| The request selects no path and no scope | held — the request carries one boolean; the root is a construction-time seam |
| A team principal is refused | held — `authorization_scope_required`, before the walk |
| The report discloses no absolute path, content, or session identity | held — asserted in both the unit and the daemon-level suites |
| A read creates no job and touches no queue | held — asserted against the real daemon |
| A failed walk keeps the last good measurement | held — the report and the failure are held separately |

## 6. Failure and recovery review

The held state is in-memory, so a daemon restart starts with an empty report
until the first walk completes. The cadence task's first tick fires
immediately, so the gap is milliseconds, not a cadence interval. A caller
asking for `refresh: true` gets a walk on demand; the walk lock serialises
it against the cadence task and against any other caller.

A failed walk — a root that became unreadable, a task that panicked — records
the failure in `last_error` and **keeps the previous report**. The operator
therefore sees the last good measurement together with its age, rather than
an empty report that reads as "no scratch". The refresh request attaches the
failure to the anomalies of the report it serves, so the failure is visible
in `--json` as well.

The census itself cannot corrupt the tree: it opens nothing for writing and
removes nothing, so the worst case of a bug in the walker is a wrong number,
not a lost directory.

## 7. Migration and compatibility review

No schema, migration, configuration key, or durable state. `STORAGE_LAYOUT_VERSION`
is unchanged: the census adds no durable row and no table.

The protocol change is **additive**: a new request variant and a new response
variant. `CoreRequest::ScratchUsage { refresh }` defaults `refresh` to
`false`, so a client that omits the field asks for the held report rather
than forcing a walk. An older daemon that does not know the request answers
with its normal "unknown request" error; the CLI reports it rather than
hanging, and the `daemon status` summary line is best-effort and simply
omitted when the daemon cannot answer.

`codegg daemon status` remains a valid command against an older daemon: the
extra line is additive and the command's existing output is unchanged.

## 8. Security review

The census **adds no capability** and **discloses less than it measures**. It
reads metadata the daemon already owns and reports it to the local operator.

The disclosure boundary is narrow and was checked rather than asserted:

- **No absolute host path.** The tree is named by its opaque digest, and the
  only paths in a report are *relative* paths inside one tree, bounded in
  count and in length.
- **No session identity.** The digest is one-way by construction: the census
  reports `hex(sha256(session_id))` and never inverts it.
- **No file content, no transcript body, no credential.** Files are measured
  with `symlink_metadata` and never opened.
- **No other tree's data.** A link out of the tree is refused and not
  followed, so a tree cannot be used to read a sibling tree or an unrelated
  host path.

The request is scoped to no project, so it fails closed for every principal
other than the local owner. That is the same shape as the existing
`node_telemetry_read` descriptor: a daemon-level operator view that a team
principal cannot reach.

One property worth recording: because the census uses `symlink_metadata`
rather than opening entries, a **FIFO cannot block the walk**. A census that
opened entries would hang forever on a FIFO a model created — the kind of
hang that is invisible until the daemon looks stuck. The entry-shape check
happens before any traversal.

## 9. Documentation and operations

Updated together, as the module contract requires:

- `architecture/security.md` — the disclosure boundary of the census.
- `architecture/process-tool-execution-ownership.md` — the census in the
  disposable-cache narrative, with the disposable-cache wording preserved.
- `docs/tools.md` — the user-facing operator paragraph.
- `docs/daemon.md` — the command table, the worked example, and the census
  section.
- `plans/subsystems/managed-agent-scratch-roadmap.md` — the M006 milestone
  section and the §12 status row.
- `plans/registry.md` — the Managed agent scratch spaces row.

No operator configuration key is needed. The census is on by default; it is a
measurement of the daemon's own tree.

## 10. Unresolved findings

| Severity | Finding | Condition and exact future evidence |
|---|---|---|
| — | none open | — |

No high, medium, or low finding is open.

Two **limitations** are recorded rather than findings, because they are
properties of the milestone's scope, not defects:

- The census measures the tree **as it is at the moment of the walk**. A model
  writing into its scratch while the walk runs makes a size that moves
  between two reads. That is inherent to measuring a live tree, and it is why
  the report carries the age of what it shows.
- `vanished_entries` — an entry removed between the directory listing and its
  inspection — is counted but not exercised by a deterministic test. The
  vanished-*tree* branch is exercised directly; the vanished-*entry* branch is
  a benign race whose observable effect is a count.

## 11. Roadmap disposition

M006 is **closed**. The census is implemented, the two operator surfaces
exist, the reclaim verdict is the validator's own verdict, and the milestone's
promised non-goals are honoured: it reclaims nothing, adds no configuration
key, no durable state, and no migration, and claims no Windows containment.

The successor — a retention policy, a session scratch sweep, a session-scratch
lease against a live turn's `TMPDIR`, and a disposition for an unreclaimable
tree — remains **unregistered**, exactly as the plan said it would be. Its
policy cannot be specified until this census has produced real numbers from a
real deployment. M003's Windows Required-sandbox half remains outstanding and
keeps M003 conditionally closed; that block is unrelated to this change and
stays owned by `plans/subsystems/windows-native-sandbox-roadmap.md`.

## 12. Registry updates

- `plans/registry.md`: the Managed agent scratch spaces row records M006 as
  closed, with the closure record path.
- `plans/subsystems/managed-agent-scratch-roadmap.md`: the M006 milestone
  section and the §12 status row move to `closed`.
- The M001/M002/M003/M004/M005 rows are unchanged.
- No downstream plan is unblocked by this milestone: the retention/sweep
  successor is deliberately unregistered, and no registered plan lists M006 as
  a dependency.

## 13. Whole-suite and hosted CI evidence

### Hosted CI

The pull request is [#145](https://github.com/dbowm91/codegg/pull/145).

The final head is **`844a5967`**, and every job on it is green except the
one that is already red on `main`:

| Run | Head | Job | Result |
|---|---|---|---|
| **`39533112385`** | `844a5967` | `CI` | **success** — all 27 guards, workspace Clippy, and the workspace tests: **15643 run, 15643 passed, 7 skipped** (433 s) |
| **`39533112371`** | `844a5967` | `Windows process launcher` | **success** — every Windows step green, including the managed scratch lifecycle step: **26 passed, 0 failed** |
| `39533112394` | `844a5967` | `Desktop E2E` | **failure** — pre-existing on `main`; see below |

The same two jobs were also green on the code head **`67d88db9`**
(`CI` run `39529050947`, **15643 run, 15643 passed, 7 skipped**; Windows run
`39529050572`, **26 passed, 0 failed**). The three commits after `67d88db9`
change only the plans and closure record, so the code is the same in both.

### One hosted-CI flake, and its green re-run of the same SHA

The **first** attempt of `CI` run `39536529955` failed exactly one test:

```
agent::tool_batch::batch_admission_tests::
  admission_deadline_edge_returns_one_terminal_outcome_and_cleans_queue
panicked at src/agent/tool_batch.rs:2967:29:
  permit released before deadline: Timeout("tool 'deadline-edge-tool' timed
  out waiting in the tool batch queue after 101ms (admission limit 100ms;
  effective parallelism 1); it did not start executing")
```

That test releases a held permit at **80 ms** and expects the acquisition to
succeed against a **100 ms** admission deadline — a 20 ms margin on a loaded
hosted runner, and the failure was a **1 ms** overshoot. It is a
deadline-edge timing test, not a behavioural one.

It is **not** a regression of this milestone, and that is established rather
than assumed:

- The branch touches **nothing** under `src/agent/`
  (`git diff --name-only origin/main...HEAD -- src/agent/` is empty).
- The only difference between the green head `844a5967` and the head that
  failed is `plans/closure/managed-agent-scratch/006-status.md` — a
  markdown file that cannot influence a Rust test.
- The same code was green on the previous CI run (`39529050947`, 15643/15643)
  and is green on the re-run.
- Locally the test passed **10 out of 10** consecutive runs, in 0.20 s each.

The **re-run of the same SHA** (`39536529955`, attempt 2) was **fully
green**: **15643 run, 15643 passed, 7 skipped** (460 s).

This is the same flake class the repository already records for
`codegg-client::session_projection_driver::duplicate_events_are_idempotent`
in `plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md`.

### A second hosted-CI flake, and how it was closed

The **first** attempt of the `CI` run on the head `24a74874`
(`39540628447`) failed one test in a package this branch does not touch:

```
codegg_client session_projection_driver::unsupported_daemon_fails_attach_closed
panicked at crates/codegg-client/tests/session_projection_driver.rs:197:10:
  read timeout: Elapsed(())
and then at :1212:
  assertion failed: matches!(error, DriverError::Unsupported)
```

That is the **same runner-sensitive read-timeout flake** the repository
already records, tripping a different test in the same file: the shared read
helper at `:197` timed out on the loaded hosted runner, so the test saw a
timeout where it expected an `Unsupported` error.

The same proof applies, and it is the proof the repository itself uses for
this flake class in
`plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md`:

- `git diff --name-only origin/main...HEAD -- crates/codegg-client/` is
  **empty** — the branch never touched that crate.
- The suite passes **16/16** locally, in 0.97 s.

The **re-run of the same SHA** (`39540628447`, attempt 2) came back
**cancelled**, not green: pushing the next commit started a fresh run set and
GitHub cancelled the in-flight re-run. So this flake was **not** closed by a
re-run of its own SHA, and that is recorded rather than blurred.

It was closed instead by the **final head's** own `CI` run, which ran
byte-identical code: `24a74874` and `d5b519f4` differ **only** in
`plans/closure/managed-agent-scratch/006-status.md`, a markdown file that
cannot influence a Rust test. That run is green — see the table above.

One of this milestone's two `CI` flakes therefore has a green re-run of the
same SHA (`39536529955`) and the other does not (`39540628447`); both were
closed by a green run of the final head's code. The code was green on every
attempt that reached the workspace tests without tripping a flake.

The first Windows run, `39527099234` on the pre-fix head `8e4c2ad5`, failed
exactly one test: `scratch::tests::the_census_refusal_rule_agrees_with_the_reclaim_validator`.
The test was gated `#[cfg(any(unix, windows))]` but only created its symlink
under `#[cfg(unix)]`, so on Windows `symlink_metadata` was called on a path
that did not exist and the `unwrap` panicked with
`Os { code: 2, kind: NotFound }`. Creating a symlink on Windows needs a
privilege the CI runner does not have, so the link shape cannot be probed
there; the test now probes the regular-file and directory shapes on every
platform and adds the link shape only where a link can be created. The
assertion is unchanged, and the re-run on `67d88db9` is green: **26 passed,
0 failed**.

### The Desktop E2E failure is `main`'s own

The `Desktop E2E` job fails on this branch with
`element ("[data-testid="permission-list"]") still not existing after 120000ms`
in `e2e/specs/m004-session.e2e.ts`, the `M004 built-app session vertical
slice` spec. The **identical** failure signature occurs on `main` itself:

| Run | Head | What it is |
|---|---|---|
| `39502828002` | `31a41284` (a `main` head) | same failure, same spec, same message |
| `39463448048` | `a9f70375` (a `main` head) | same failure |
| `39416798574` | `0f7d0d30b` (a `main` head) | same failure |
| `39345242816` | `a5d88eb7a` | green — the last green Desktop E2E |

This branch touches nothing under `apps/` or `e2e/`
(`git diff --name-only origin/main...HEAD -- apps/ e2e/` is empty), and the
failure was already present on `main` before this branch existed. The cause is
`main`'s own PR #134 temp-root redirect offer answering the model instead of
raising a human decision, which was recorded in the M005 closure record. It
is **not** a regression of this milestone and is not repaired here.

### A live observation of the operator surface

The census was exercised against a **live daemon**, not only in a fixture.
The daemon was started **isolated** — its own `CODEGG_DAEMON_HOME` and
`CODEGG_DATA_HOME`, its own socket, and a temporary root — so the
developer's own daemon was not driven. The scratch tree is the per-user
cache tree, which is the tree the milestone is about.

| Command | Observed result |
|---|---|
| `codegg daemon scratch-usage` | `Scratch usage — 301 tree(s), 40.0 KiB, measured 20s ago`, then one row per tree: the 16-character digest prefix, the size, the file and directory counts, the link and multiply-linked counts, and the verdict |
| `codegg daemon scratch-usage --json` | the same measurement as JSON, with the **full** 64-character digest, the sizes, the counts, the modification-time range, `reclaimable`, the refusal rules, and the refusal paths |
| `codegg daemon status` | one added line: `Scratch: 301 tree(s), 40.0 KiB, measured 34s ago` |

What the observation shows about the milestone's purpose: on this host the
tree holds **301 session trees totalling 40 KiB** — almost all of them
holding nothing but their own owner record. They are session trees that were
allocated and never reclaimed. Before this milestone that condition was
invisible; there is no surface that reported it. The census reports it, and
the numbers are what the unregistered retention successor needs in order to
have a policy at all.

Two properties of the observation are worth recording because they are the
milestone's own guarantees, observed rather than asserted:

- The JSON carries the full digest and the ages but **no absolute host
  path, no file content, and no session identity** — the digests are
  one-way and the tree is named by its digest alone.
- The daemon was started, measured, and stopped without the census
  **changing anything**: the tree is the same afterwards.

The 301 trees are a property of this host and of the repository's own test
suite, which allocates real session scratch trees under the per-user cache
root. The pre-existing `scratch::tests` module leaks four trees per run
(measured: 305 → 309 with the milestone's own test skipped, and 305 → 305
with only that test run), so the milestone's new test adds **no** leak.

### The local whole-suite sweep

`cargo nextest run --workspace --locked --profile ci --no-fail-fast` on this
branch: **15616 run, 15613 passed, 2 failed, 1 timed out, 7 skipped**
(678 s).

The three non-green results are **`main`'s own** and are not caused by this
branch:

| Test | Result | Why it is not this milestone's |
|---|---|---|
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | fail | `register workspace: InvalidRootPath("/tmp")` — the test registers `/tmp` as a workspace root and the registry refuses it on this host. Reproduced **identically on `origin/main`** (`cb0e79c0`) in a separate worktree: the same panic at the same line. |
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | fail | the same `InvalidRootPath("/tmp")` panic, reproduced identically on `origin/main`. |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | timeout (300 s) | A load-sensitive race in that test's own `BlockingBuilder`, which parks a thread and is released by an `unpark` from the test. It **constructs no `CoreDaemon`**, so the census refresh task this milestone adds to the daemon construction is provably not in its path. It passes in isolation in 0.01 s. The hosted CI run of the same suite is fully green, which is the stronger evidence. |

The comparison is direct: the **same** sweep was run against `origin/main`
(`cb0e79c0`) in a separate worktree with its own target directory.

| Tree | Sweep result | Non-green results |
|---|---|---|
| this branch | 15616 run, 15613 passed, **2 failed, 1 timed out**, 7 skipped | `daemon_refresh::session_projection_snapshot_includes_in_process_live_child_instances`, `daemon_refresh::session_projection_rebuilds_owner_scoped_live_jobs`, `asset_refresh::same_scope_requests_coalesce_to_one_publication` |
| `origin/main` (`cb0e79c0`) | 15584 run, 15579 passed, **4 failed, 1 timed out**, 7 skipped | the **same three**, plus `permission::tests::temp_root_redirect_offer_only_covers_an_unauthorized_temp_root` and `agent_loop_harness::first_use_apply_patch_create_then_grep_runs_through_agent_broker` |

This branch's non-green set is a **strict subset** of `main`'s, and every
member of it is present on `main` too. The milestone adds no failure.

The hosted `CI` run is the authoritative whole-suite result: **15643 run,
15643 passed, 7 skipped**, with none of these three present.

## 14. Notes on what this milestone does not claim

- It does not claim that the scratch tree is small. It claims that its size
  is now *visible*.
- It does not claim a retention policy. A tree that is never soft-deleted
  still grows without bound.
- It does not claim Windows containment. The census reads metadata; it makes
  no containment claim on any platform.
- It does not claim that a tree the validator refuses is *fixable* by this
  milestone. It claims that such a tree is no longer invisible.
