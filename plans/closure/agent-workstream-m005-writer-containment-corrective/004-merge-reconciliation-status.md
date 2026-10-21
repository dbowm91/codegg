# M005 Writer-Containment Corrective — Merge Reconciliation with `main`

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the corrective line and it opens no new milestone. It records
the reconciliation of the M005 writer-containment branch with `main` and the
qualification of the merged tree, as §11 of the source addendum requires:
"later supplemental evidence appended/linked rather than its history
overwritten".

It now records **three** reconciliations: the first against `origin/main`
`a9f70375` (§1–§8), the second against `origin/main` `9c9ffb763` (§9,
2026-10-21) and the third against `origin/main` `2c1953347` (§10, 2026-10-21).
§9 and §10 are appended, not folded in — the earlier reconciliations'
inventories and defect lists stand as they were written.

Source corrective addendum:

- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`

Source subsystem roadmap:

- `plans/subsystems/agent-workstream-blackboard-roadmap.md`

Historical closure records this record supplements and does **not** rewrite:

- `plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md` (C001, closed)
- `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md` (C002, closed)
- `plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md` (C003, closed)

Repository baseline reconciled: `origin/main` at `a9f70375` ("Merge pull
request #139 from dbowm91/plan/tui-live-work-visibility-corrective"), which is
**104 commits** ahead of this branch's merge-base `fb84bee6` ("docs(plans):
close managed scratch M004"). The branch head at merge was `bdb65361`. The
merge commit on the branch is `87fb57f7`.

Changed-file surface: the branch carried 210 changed files against the
merge-base, `main` carried 309, and **108 files were changed on both sides**.

## 1. Executive finding

**The writer-fence boundary survived the reconciliation unchanged.** Across
`main`'s 104 commits, not one file of the fence contract
(`src/tool/writer_fence.rs`), the daemon adapter (`src/core/writer_fence.rs`),
the permit authority (`src/core/writer_permit.rs`,
`crates/codegg-core/src/writer_permit.rs`), the disposition manifest
(`docs/writer-disposition.toml`) or the guard
(`scripts/check_writer_disposition.py`) was touched. `main` did reach two
fenced effect sites — `src/tool/write.rs` (the temp-root redirect offer) and
`src/tool/apply_patch.rs` — and the broker; all three auto-merged, and every
fence acquire/settle call is still in place in the merged tree.

The reconciliation found and fixed **seven defects**, of which two would have
left the merged tree red:

- **The ADR numbers collided.** Both sides minted an `ADR-0042`: `main` added
  `ADR-0042-composer-normal-mode-key-ownership.md`, and this branch had
  carried `ADR-0042-restricted-agent-shared-checkout-writer-contract.md`
  since it forked. The ADR integrity guard failed on the duplicate *and* on
  every reference to the branch's ADR, because only one path can be canonical
  for a number. The branch's writer contract is renumbered **`ADR-0044`** and
  every reference to it is updated (§3.1). A **second** collision with `main`
  later renumbered it again, to **`ADR-0047`** — see §10.1.
- **`main`'s new scheduler-lifecycle publisher did not carry the M007 scope
  fields.** The merged tree therefore did not compile (§3.2).

The remaining five are documentation-consistency defects (§3.3–§3.7). The
merged tree is green on the local routine set, and the M005 line's own state
is unchanged by the merge: **shared mutation remains unavailable**, and C004
is the next milestone.

## 2. Reconciliation inventory

Merge commit on the branch: `87fb57f7` ("Merge origin/main into
work/agent-workstream-m005-writer-audit"). Eleven files conflicted, in 35
hunks. Every resolution keeps both sides' authority semantics; none of them
negotiates a boundary.

| Conflict | Resolution |
|---|---|
| `src/tool/broker.rs` | The doc comment on `BROKER_WATCHDOG_GRACE_MS`. `main`'s is a superset — it adds the head-start rationale to the same statement — and the constant's value was never in conflict. |
| `src/agent/tool_batch.rs` | `workspace_path_policy` (`main`) replaces `workspace_service_lease` (branch) as the path-policy fingerprint source. Both fields survive in `AgentLoop`: the lease still owns the lock table and the eviction guarantee, while the policy is the fingerprint. `main`'s field is the correct source because it is populated for **both** parent turns and delegated children — a child holds no lease, so the branch's source was `None` exactly where `main`'s fix put the authority. |
| `src/tool/grep.rs` | `result.contains(..)` over the branch's `result.find(..).is_some()`. The two are the same assertion; the branch's spelling is `clippy::manual_contains` and would fail the `-D warnings` gate. |
| `crates/codegg-protocol/src/projection/adapters.rs` (3 hunks) | The event's own scope field wins, with the envelope's session/turn as the fallback: `session_id: session_id.clone().or_else(\|\| env.session_id.clone())`, `turn_id: env.turn_id.clone()`. The branch's M007 added the optional scope fields to the lifecycle events precisely so that the job's *own* scope travels with them; `main` reaches the same facts through the envelope and the merge-preserving upsert. Preferring the event's value is what `main`'s own `JobCreated` arm already does, so the two mechanisms agree. |
| `crates/codegg-protocol/src/projection/snapshot.rs` | `main`'s `upsert_job` merge, a strict superset of the branch's: it adds stale-attempt and terminal-state protection and preserves the summary, the active attempt and the error class as well. |
| `architecture/overview.md` | The Verified-counts table was **re-measured**, not copied from either side (§3.6). |
| `plans/registry.md` (4 hunks) | A row union with per-row ownership: the branch's writer-line and agent-workstream rows, `main`'s rows for every other subsystem. No row was lost and no row was invented (§3.7). |
| `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md` | `main`'s row, which adds the C008 follow-up requalification pointer. |
| `scripts/test-eggsearch-pair.sh` | `main`'s Eggsearch pin (`13fcac0b7ac0125d38e4e9775ebe1d62028fda72`), the C008 `v0.4.4` requalification. Both sides applied the same upstream feature commit independently; `main`'s pin is the later requalification. |
| `tests/agent_loop_harness.rs` (9 hunks) | The branch's extra `AgentLoop::new` argument — the C003 writer fence — which the merged constructor still takes. One hunk took `main`'s `Vec::new()` spelling. |
| `tests/tool_execution.rs` (12 hunks) | The branch's per-test `TempDir`-rooted `BashTool` over `main`'s process-cwd `BashTool::new()`. The rooted form is the one this repository prefers: it is what the earlier scratch reconciliation recorded as the surviving spelling, and a tool rooted at the process cwd is exactly the "relative path resolved against the wrong workspace" shape AGENTS.md warns about. |

## 3. Defects found and fixed

### 3.1 The ADR numbers collided (`main` side and branch side; blocking)

`main` added `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md`
(the TUI composer's normal-mode key ownership). This branch had independently
filed a writer-contract ADR under the number `ADR-0042` (the restricted
shared-checkout writer contract) while that number was still free on the
branch's fork point.

`scripts/check_adr_integrity.py` enforces that an active ADR number resolves
to exactly one path, and that every `plans/adrs/…` reference in the tree
points at a canonical path. With two `ADR-0042` files, the guard reported the
duplicate **and** nine dangling references — every reference to the branch's
ADR — because only the alphabetically first path is canonical.

The branch's ADR is renumbered **`ADR-0044`** (the next free number:
`main` holds 0042 and 0043, and 0044 was referenced nowhere). The file was
renamed, its own three mentions updated, and **68 references** across 21
documents updated: the writer-contract ADR, `plans/adrs/README.md`,
`architecture/writer_permit.md`, `docs/writer-disposition.toml`, the agent-
workstream roadmap and the M005 corrective addendum, every
implementation/evidence/closure document of the corrective line, and the four
writer-line rows of `plans/registry.md`.

`main`'s composer ADR and every reference to it were left untouched. The
remaining `ADR-0042` mentions in the merged tree are all the composer ADR's.

### 3.2 The scheduler-lifecycle publisher did not carry the M007 scope fields (`main` side; compile-blocking)

`main`'s TUI live-work workstream added a scheduler-lifecycle publisher in
`src/core/daemon_construct.rs` that constructs `CoreEvent::JobStarted`,
`JobCompleted` and `JobFailed`. Those three events are the ones this branch's
M007 extended with the optional `session_id`, `workspace_id` and `kind` scope
fields, so the merged tree did not compile: five initializers were missing
`kind`, `session_id` and `workspace_id`.

The publisher already holds the resolved `JobRecord`, which carries exactly
those three facts, so the fix is to publish what the publisher knows rather
than to leave the fields unset: the lifecycle event now carries the job
record's `session_id`, `workspace_id` and `kind`.

This **adds** information and removes none. The projection's `upsert_job`
merge only fills an empty field from the earlier slot, so a populated field
is simply the authoritative scope of the job itself — and it makes M007's
mechanism live rather than inert, which is what M007's closure record claims:
"`CoreEvent::JobStarted` / `JobCompleted` / `JobFailed` now carry
`session_id`, `workspace_id` and `kind`."

Two test fixtures in `crates/codegg-core/src/projection_replay/publication.rs`
(origin/main's, exercising the trusted-envelope owner) were given explicit
`None` scope fields: those fixtures assert that the **envelope** is the
trusted owner, and the event deliberately carries no scope of its own.

### 3.3 C003's status label disagreed across four documents (branch side)

The branch's C003 closure record and its plan header both read "closed", and
the registry's subsystem row read "C001–C003 closed" — but the registry's
milestone row and the roadmap addendum's C003 row still read **"ready"**. A
reader of the milestone table would have been told that C003 was still to be
built, while the closure record said it was shipped.

All four now read "closed", and the roadmap row points at the closure record.

### 3.4 C004's readiness disagreed across four documents (branch side)

C003's closure record states that C004 is "**unblocked** by this closure"
and the registry's subsystem row reads "C004 ready" — but C004's own plan
header read "Status: blocked — C003 strict closure", and the registry's
blocked-work row still listed C004 as blocked on "C003 strict closure".

All four now agree that C004 is ready, and the plan header names the closure
that discharged its gate. C004's *scope* is unchanged: it still owns the
non-broker sinks, the durable audit-event surface for them, and the five
`fenced-native` manifest roots that are not native tool routes.

### 3.5 Six registry rows still said "C007 remains blocked on C002" (both sides)

The agent-tool-use operability corrective line's rows carried the sentence
"C007 remains blocked on C002" on both sides of the merge, although C007 was
closed and its follow-up closure record exists. `main`'s corrected wording —
"C007 was then blocked on C002; it is now closed in
`plans/closure/agent-tool-use-operability-post-audit-corrective/007-follow-up-status.md`" —
is the one that survives.

### 3.6 The Verified-counts table was stale on both sides (both sides)

`architecture/overview.md`'s Verified-counts table is the single source of
truth for the repository's counts, and the two sides had each measured it
against their own tree. Two rows were not merely out of date — they
**disagreed with each other on identical source sets**:

- *Distinct production tool names*: the branch said 59, `main` said 61. Both
  trees contain the **same 70 `impl Tool for` blocks** and the **same 60
  literal production names**, so both numbers were wrong. The merged tree
  re-measures to **60** under the method now stated in the row's `Source`.
- *Tool registrations in `with_options()`*: the branch said 63, `main` said
  65. The count is 52 `registry.register(` call sites **plus** 13
  `registry.register_boxed(` call sites in `src/tool/mod.rs`; the branch's
  row omitted the `register_boxed` sites. The merged tree measures **65**,
  and the row now states both terms.

Every other row was re-measured against its own `Source` on the merged tree
rather than taken from either side: architecture docs 90, tool source files
81, LSP server definitions 39, `codegg-core` modules 49, `AppEvent` variants
61, `CoreRequest` variants 259, `CoreResponse` variants 169, `CoreEvent`
variants 92, `ProjectionEvent` variants 47, authorized operations 259,
identity newtypes 19, built-in slash commands 166, built-in agents 10,
bundled themes 50, TUI state modules 33, database tables 94, storage layout
version 85, git operation variants 54, git risk classes 11, workspace member
crates 11 (+ root), integration test files 279, CI guard scripts 63.

The built-in slash-command row also moved from the branch's stale 164 to
**166**, the number the `built_in_command_count_matches_release_docs`
tripwire asserts — the branch's number would have failed that tripwire.

### 3.7 The row union duplicated two blocks (merge-introduced)

Taking the union of two independently edited registry tables produced two
duplicated blocks: a six-row agent-tool-use block, and the agent-tool-use
rows appearing in both the "Dependency-ready implementation plans" table and
the "Recently closed" table. The merged registry is now the union of both
sides' rows with no row duplicated, no row lost, and every row in the place
`main` keeps it. A key-by-key comparison of the merged registry against the
union of the two sides' rows found **zero** missing rows and **zero** rows
that exist on neither side; the three rows that appear "missing" are the
three that one side **renamed** (the agent-workstream M005 row, the TUI
editor Helix M006 row, and that line's subsystem summary).

## 4. Qualification evidence

All runs are on the merged tree at `87fb57f7`, in the branch's own worktree.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | **clean, exit 0** |
| `cargo check --workspace --all-targets` | clean |
| `scripts/verify.sh quick` | **passed, exit 0** |
| `scripts/check_adr_integrity.py` | pass — "active ADR IDs are unique and local paths resolve" |
| `scripts/check_writer_disposition.py` | pass — "49 declared roots, 17 writer tools, 30 writer modules" |
| `scripts/check_writer_disposition.py --self-test` | pass |
| `tests/writer_fence_c003.rs` | 10/10 pass |
| `tests/writer_permit_service.rs` | 12/12 pass |
| `tests/tool_broker_integration.rs` | 25/25 pass |
| `tests/writer_disposition_guards.rs` | 5/5 pass |
| `crates/codegg-core/tests/writer_permit.rs` | 21/21 pass |
| `cargo test -p codegg --lib tool::` (the fenced tools) | **711 passed, 0 failed** |
| `cargo nextest run --profile ci -p codegg --test agent_loop_harness` | **69 passed, 1 skipped** |

**The full workspace suite did not complete on this host.** The machine was
carrying other sessions' cargo and nextest builds (load average 8–11, and
the shared package cache lock contended), so the verification above is
`verify.sh quick`, workspace clippy and scoped suites rather than the whole
suite. No hosted CI run was made for this merge either.

## 5. What this record does not change

- **M005 remains active and shared mutation remains unavailable.** The merge
  changed no shared-mutation behaviour: the work-order `shared`/`serialized`
  placement is still refused at the coordinator, and the scheduler still gives
  every delegated run its own isolated worktree. C003's fence is live as a
  *containment* rule; the permit-backed shared profile still has no
  production caller.
- **C001, C002 and C003 closures stand as recorded.** Their evidence, their
  commit references and their unresolved findings are unchanged. The only
  edit to those records is the ADR renumbering (§3.1), which changes an
  identifier, not a finding.
- **C004–C006 keep their scope and their order.** C004 is ready; C005 and
  C006 remain blocked on their ordered predecessors.
- **No ADR content was rewritten.** The renumbering changed the file name
  and the references to it; the decision, the option-B acceptance record and
  the date are untouched. The acceptance date stays 2026-10-19.

## 6. Known limits and non-claims

- **No hosted CI evidence.** The merged tree was not pushed and no CI run
  was made. The local evidence in §4 is what exists.
- **The full workspace test suite was not run to completion** on this host
  (§4).
- **One harness test fails under the legacy single-process runner, and it is
  inherited from `main` rather than introduced by this merge.** Running
  `cargo test -p codegg --test agent_loop_harness` (one process, tests in
  parallel threads) fails
  `temp_root_write_following_the_offer_needs_no_human` with one
  `PermissionPending` counted where the test expects zero. The cause is a
  **cross-test leak through the global event bus**: that test subscribes to
  `GlobalEventBus` and counts every `PermissionPending` it receives, while
  another test in the same binary publishes one (`args=None`) concurrently.

  The evidence that this is not a merge regression: the test **passes when run
  alone**; it passes under the repository's canonical runner,
  `cargo nextest run --profile ci`, which gives each test its own process
  (69 passed, 1 skipped); and this branch added **no** tests to that binary
  at all — the harness test inventory of the merged tree is `main`'s, with
  both temp-root tests belonging to `main`. AGENTS.md already names this
  hazard: "broad test execution runs under nextest profile `ci` (one process
  per test — env mutation in tests is process-global)". The failure is
  reported here rather than silenced, because the merged tree inherits it
  and a future run under `cargo test` will hit it again.
- **The three `agent_loop_harness` tests that fail environmentally because
  `~/.cache` is mode 777** remain an environment condition of this host, not
  a regression of this merge; they were already failing on the branch before
  the merge.
- **No live-daemon or real-model observation** was made for this
  reconciliation. The merge is a source-level reconciliation; it claims
  nothing about runtime behaviour beyond what the tests above exercise.
- **The Eggsearch pair qualification was not re-run.** Its pin moved to
  `main`'s `v0.4.4` revision, but the pair script needs a pinned Eggsearch
  checkout and a network-free build; that is a separate, expensive run.

## 7. Commands run

```bash
git fetch origin main
git merge --no-commit --no-ff origin/main          # 11 files, 35 hunks
cargo fmt --all
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
python3 scripts/check_adr_integrity.py
python3 scripts/check_writer_disposition.py
python3 scripts/check_writer_disposition.py --self-test
cargo test -p codegg --test writer_fence_c003 --test writer_permit_service \
  --test tool_broker_integration --test writer_disposition_guards
cargo test -p codegg-core --test writer_permit
cargo test -p codegg --lib tool::
cargo test -p codegg --test agent_loop_harness
```

## 8. Historical records preserved

This record adds to, and does not rewrite, the evidence history:

- `001-status.md` (C001) is unchanged apart from the ADR renumbering.
- `002-status.md` (C002) is unchanged apart from the ADR renumbering.
- `003-status.md` (C003) is unchanged apart from the ADR renumbering.
- The branch's writer-contract ADR is preserved under its new number, as
  `plans/adrs/ADR-0047-restricted-agent-shared-checkout-writer-contract.md`.
  Its original filename carried the number `ADR-0042`, which `main` had
  already taken (§3.1), and it then carried `ADR-0044`, which `main` took in a
  later advance (§10.1); its content, its decision and its acceptance record
  are unchanged.
- `main`'s `ADR-0042-composer-normal-mode-key-ownership.md` and `ADR-0043`
  are untouched.

## 9. Second reconciliation — `origin/main` `9c9ffb763` (2026-10-21)

The first reconciliation (§1–§8) reconciled the branch with `origin/main`
`a9f70375`. `main` then moved on — 71 commits — and this section records the
second reconciliation, appended rather than folded into the earlier one, per
this record's own kind.

### 9.1 Inventory

Repository baseline reconciled: `origin/main` at `9c9ffb763`. The branch head
at merge was `2cddbff3c` (the commit that qualifies macOS for the shared
checkout profile). The merge-base was `a9f70375` — the same baseline the
first reconciliation used, so this reconciliation is `main`'s next 71 commits
and the branch's next 77. The merge commit on the branch is `42707515b`.

**20 files conflicted in 72 hunks.** Every resolution keeps both sides'
authority semantics; none silently dropped one.

The merge commit's own message says 81 hunks. The measured count is **72**,
and the per-file table below is that measurement — it sums to 72. The
commit message is left as published rather than rewriting published
history over a count; this record carries the corrected figure.

| File | Hunks | Resolution |
|---|---|---|
| `crates/codegg-core/src/jobs/mod.rs` | 13 | Union of `ResourceRequest`'s fields (M007's `keyed_quotas`, `main`'s `remote`); the `Build`/`Format` arms keep M007's removal of the legacy global exclusivity key (§9.2.4) |
| `crates/codegg-core/src/session/user_prompt_queue.rs` | 8 | `main`'s shared `begin_immediate` + `commit_with`; the branch's local wrapper removed (§9.2.3) |
| `crates/codegg-core/src/storage/immediate.rs` | 7 | `main`'s version, a strict superset (§9.2.3) |
| `src/scheduler/admission.rs` | 6 | Union of M007's quota errors and held-quota state with `main`'s remote-node errors and per-node state |
| `tests/scheduler_phase5.rs` | 6 | Union of the reservation-dimension fields |
| `scripts/check_queue_write_transaction_shape.py` | 5 | `main`'s version, a strict superset; the branch's C002 rationale comment kept (§9.2.3) |
| `crates/codegg-core/src/audit.rs` | 5 | Union of the audit actions — 30 in the merged tree (§9.2.5) |
| `plans/registry.md` | 4 | Union deduplicated by subsystem (§9.2.2) |
| `architecture/audit.md` | 2 | Action list union; the count re-measured to 30 |
| `architecture/overview.md` | 2 | The Verified-counts table re-measured in the merged tree (§9.2.6) |
| `src/scheduler/permit.rs` | 2 | Union of `PermitDimensions`' fields and the test helper |
| `src/scheduler/config.rs` | 2 | Union of M007's `quota: QuotaPolicy` with `main`'s `remote_node_capacity` |
| `tests/scheduler_contention.rs` | 3 | Union of the reservation-dimension fields, plus §9.2.7 |
| `src/core/mod.rs`, `src/interactive_process.rs`, `src/scheduler/scheduler.rs`, `tests/common/mod.rs`, `tests/scheduler_restart_recovery.rs`, `src/core/daemon_construct.rs`, `crates/codegg-protocol/src/core.rs` | 1 each | Unions of distinct additions (module lists, struct fields, and `main`'s scratch-census wiring kept alongside the branch's C005 human-shell wiring) |

### 9.2 Defects found and fixed

#### 9.2.1 `main`'s new schedule materializer did not know the branch's process refusal (main side; compile-blocking)

`main`'s Eggwork M004 added `src/scheduler/schedule_materializer.rs`, which
maps a refused submission to a stable, secret-free reason code with an
**exhaustive** match on `JobSubmissionError`. The branch's C005 added
`JobSubmissionError::ProcessDenied(String)`, so the merged tree did not
compile:

```
error[E0004]: non-exhaustive patterns:
  `&submission::JobSubmissionError::ProcessDenied(_)` not covered
   --> src/scheduler/schedule_materializer.rs:203:11
```

The arm maps it to the slug `process_denied`. The refusal's own `Display` can
name a path, so only the slug reaches a log line or a durable field — the
same rule the rest of that function follows. The refusal's *retry* semantics
are the store's existing ones: a refused occurrence is **not recorded**, so
the occurrence stays retryable once the agent-run group holding the checkout
finishes. A checkout that is reserved for the moment is not a failed
occurrence.

#### 9.2.2 The registry union duplicated rows (merge-introduced)

Both sides had added a row for the same subsystem, and a blind union would
have produced two rows for each. The merged registry keeps **one** row per
subsystem: `main`'s newer row where `main` is newer (the managed-scratch row,
which records M006 and M007 closed), and the branch's row where the branch is
newer — the blackboard roadmap row, which carries M005's C006 conditional
closure and the macOS qualification. Every M005 row the branch contributed is
carried over; none is lost.

#### 9.2.3 Three files where `main`'s version was a strict superset

- `crates/codegg-core/src/storage/immediate.rs` — `main`'s version adds the
  free `begin_immediate`, the error-mapping `begin_with`/`commit_with`, and
  the shared-primitive rationale. The branch's API is a subset.
- `scripts/check_queue_write_transaction_shape.py` — `main`'s version keeps
  the branch's identical `PERMITTED_DEFERRED` allowlist and its identical
  queue-store check, merges the immediate and rollback checks into one, and
  adds the scratch-sweep check. The branch's C002 rationale comment was kept
  on the shared-module constant.
- `crates/codegg-core/src/session/user_prompt_queue.rs` — the branch's local
  `begin_immediate` wrapper was removed in favour of `main`'s shared one,
  which also removed the now-unused `ImmediateWrite` import.

Taking a superset is only safe when the superset is *verified* to be one, so
each pair was compared before the choice rather than assumed: the allowlists
were diffed, the check functions were diffed, and the immediate-transaction
APIs were listed side by side.

#### 9.2.4 The legacy global exclusivity key (branch side; contract-bearing)

`main`'s `ResourceRequest::for_kind` still attaches
`exclusive:workspace-mutation` to the `Build` and `Format` profiles. The
branch's M007 **removed** that key deliberately, replacing it with the
counted, workspace-scoped build quota. `main` had simply not received M007:
the key came from the common ancestor (`367aa8603`) and the branch removed it
in `def70ffc1`.

The merged tree keeps the branch's removal and the branch's M007-correct test
(`the_build_class_replaces_the_legacy_global_exclusivity_key`), which asserts
the key is *absent*. `main`'s copy of that test asserted the opposite, and
was replaced by the branch's.

#### 9.2.5 The audit action count moved (both sides)

The merged `AuditAction` carries the branch's `CheckoutWriterRefusal` and
`ProcessDenial` **and** `main`'s `ScratchReclaim`: **30** actions, not the
branch's 29 or `main`'s 28. The `ALL` array's declared length, its entries,
the `as_str` arms, the parse arms and `architecture/audit.md`'s count were
all re-measured to 30 against the merged enum rather than copied from either
side.

#### 9.2.6 The Verified-counts table was re-measured, not merged (both sides)

`architecture/overview.md`'s table is the single source of truth for counts,
so it was **re-measured in the merged tree** — all 24 rows, each against its
own listed `Source` — instead of unioned. Six rows moved against the
branch's last measurement: distinct production tool names 60 → **61**,
`CoreRequest` variants 259 → **262**, `CoreResponse` variants 169 → **172**,
authorized operations 259 → **262**, built-in slash commands 166 → **167**,
and integration test files 284 → **290**.

The tool-names row moved because the branch's count missed a **fully
qualified** `impl crate::tool::Tool for ContextReadTool`, whose literal name
`context_read` the branch's own catalog lists. The merged tree's 61
cross-checks against that catalog: all 59 catalogued names are present in the
code, and the code adds exactly two the catalog omitted — `ci_watch` and
`job` — which are now in it. The slash-command row moved because `main`'s
Eggwork M002 `/nodes` moved the tripwire's own pin from 166 to 167; the row
now agrees with the pin that asserts it.

#### 9.2.7 A pre-existing test failure was caused by M007's contract, not by C006 (branch side)

`scheduler_contention::different_exclusivity_keys_run_in_parallel` failed on
this branch before the merge — reproduced at the branch's pre-session head
`6767bb61e` — and failed again in hosted CI run `39616654598` and
`39627444733`. It is **not** C006's: it measures M007's build admission.

The test submits two `JobKind::Build` jobs into **one** workspace with
different exclusivity keys and asserts they run in parallel. Under M007 they
cannot: the submission boundary unconditionally binds the build-class quota
to the canonical workspace (`KeyedQuotaRequest::for_workspace`), and the
default bound is **one build slot per workspace**
(`scheduler.quota.max_concurrent_builds_per_workspace`, default `1`). The two
jobs are therefore serialized by the *quota* — a different admission
dimension — and the test failed while asserting nothing about exclusivity at
all.

The fix widens the quota bound **in that test only**, so the test measures
the dimension its name names. The quota's own contract is unchanged and is
covered by the build-admission suite. The alternative — dropping the default
to admit two builds per workspace — would have changed the shipped contract
to satisfy a test, which is the wrong direction.

### 9.3 Drift absorbed after the reconciliation

`origin/main` moved twice more while the reconciliation's own hosted runs
were in flight — to `239f06e7f`. Both commits are **plans-only**
(`plans/registry.md` and
`plans/subsystems/managed-agent-scratch-roadmap.md`), so absorbing them
changed no code and needed no re-verification. The merge was textually
clean, and it **reduced** the registry's true duplicate rows from six to
three, because one of `main`'s two commits was itself a registry
de-duplication.

The three that remain are `main`'s own pre-existing pattern, not a merge
artefact and not this line's to restructure: each pairs an
implementation-plan row with its **closure** row for the same milestone, so
the two point at distinct artifacts. They are present on `origin/main`
unchanged.

### 9.4 What this reconciliation does not change

- **The writer-fence boundary is untouched again.** `main`'s 71 commits
  changed no file of the fence contract (`src/tool/writer_fence.rs`), the
  daemon adapter (`src/core/writer_fence.rs`), the permit authority, the
  disposition manifest or the writer-disposition guard. The C002–C006
  contracts are exactly as §1 recorded them.
- **The M005 corrective's closure status is unchanged.** C006 remains
  *conditionally* closed, for the reasons §5.1 of the C006 record gives: the
  Windows native backend is absent rather than unmeasured, and a *live*
  two-agent race inside one daemon process is not claimed.
- **The earlier reconciliation's history is not rewritten.** §1–§8 stand as
  the record of the first reconciliation against `a9f70375`.

## 10. Third reconciliation — `origin/main` `2c1953347` (2026-10-21)

The second reconciliation (§9) reconciled the branch with `origin/main`
`9c9ffb763`. `main` then moved on again — **323 commits** — and this section
records the third reconciliation, appended rather than folded into the earlier
ones, per this record's own kind.

`main` at `2c1953347` is the Linux sandbox admission and ADR cleanup (PR #149).
Its advance is the largest this branch has had to absorb, and it is not
count-neutral: it added a `CoreRequest` variant, a `CoreResponse` variant,
`codegg-core` modules, tool registrations, an integration test file, a storage
migration and two ADRs.

### 10.1 Inventory

Merge-base `239f06e7f` ("docs(plans): record the census's tri-state verdict in
the M006 rows"). `main` carried **323** commits above that base; the branch
carried **88**. Eight files conflicted, in **16 hunks**. The merge commit on
the branch is `35ea128ec`. The writer-fence
boundary was untouched again: none of `src/tool/writer_fence.rs`,
`src/core/writer_fence.rs`, `crates/codegg-core/src/writer_permit.rs`,
`docs/writer-disposition.toml` or `scripts/check_writer_disposition.py`
conflicted.

| Conflict | Resolution |
|---|---|
| `crates/codegg-core/src/error.rs` | Additive union: the branch's `ToolError::ProcessDenied` (C005) and `main`'s `ToolError::BackendUnavailable`. Neither side replaced the other. |
| `src/managed_process.rs` (4 hunks) | Additive union of four request fields: the branch's `process_admission` / `process_kind` (C005) and `main`'s `required_sandbox_profile` / `required_network_intent`. Both destructuring sites bind each side's fields. |
| `src/scheduler/executors.rs` | Union: the branch's C005 process-admission refusal runs *before* the payload is destructured, and `main`'s extended destructuring (`sandbox_profile`, `network_intent`) is kept. |
| `crates/codegg-core/tests/user_prompt_queue.rs` | The branch's durable **floor** wins over `main`'s hard-coded `assert_eq!(STORAGE_LAYOUT_VERSION, 85)`. The floor reads the version out of the database, so it is a test; a comparison between two constants is folded away by the compiler, so it is not. `main`'s literal was also already wrong after the renumbering in §10.2. |
| `crates/codegg-core/tests/user_prompt_queue_mutation.rs` | The same floor form, for the same reason. |
| `tests/job_background_test_slice.rs` | Union: `main`'s authorization context (a real receipt) and the branch's `process_admission: None` — this fixture is not under the restricted shared-checkout profile, so the launch carries no admission policy. |
| `tests/tool_execution.rs` (4 hunks) | `main`'s root at the marker's *own* directory wins for the allowlist test, so the denial it asserts comes from the allowlist rather than from a path outside the tool's workspace root. The branch's `rooted_bash_tool()` helper — used by fifteen other tests in the same file — is kept everywhere else. |
| `architecture/overview.md` (3 hunks) | The Verified-counts table was **re-measured**, not copied from either side (§10.4). |

### 10.2 Two identity collisions the auto-merge did not report

The first reconciliation's ADR collision was *reported* by the guard. These two
were not: both sides allocated the same durable identities independently, and
git's auto-merge produced a tree that no side had ever asserted.

**The storage migration numbers collided.** Both sides allocated v84 and v85.
`main` published v84 (the additive network-intent preference) and v85 (the
private index publication journal); this branch's writer permit and session
launch took v84 and v85 as well. Because the two sides' migrations were added
at different anchors in `crates/codegg-core/src/session/schema.rs`, the
auto-merge **concatenated both** rather than conflicting: the merged tree held
two `migrate_v84` functions and two `migrate_v85` functions, which is a
duplicate-definition compile error.

The branch yields the lower numbers to `main` — the same rule its ADR
renumbering follows, and the same rule the branch already used when `main`
took ADR-0042 (§3.1). The writer permit becomes **v86** and the session launch
**v87**, and `STORAGE_LAYOUT_VERSION` moves 85 → **87**. The renumbering is
additive and changes the meaning of no published database: every version
`main` can produce is still below the branch's two.

**The ADR numbers collided again.** `main` published
`ADR-0044-contained-git-live-objects-and-metadata-authority.md`; this branch's
writer-contract ADR was still numbered `ADR-0044` from the first
reconciliation. `scripts/check_adr_integrity.py` failed on the duplicate and
on all eleven references to the branch's ADR. The branch's ADR is renumbered
**`ADR-0047`** (§10.4): the file is renamed, its own mentions updated, and
every reference to it updated — **24 documents**, including
`plans/adrs/README.md`, `architecture/writer_permit.md`,
`docs/writer-disposition.toml`, `src/tool/process_admission.rs`, the agent-
workstream roadmap, the M005 corrective addendum, every
implementation/evidence/closure document of the corrective line, and the
M005 rows of `plans/registry.md`. `main`'s ADR-0044 and every reference to it
are untouched.

### 10.3 Three defects the merged tree did not compile or pass its guards under

**The merged tree did not compile.** `main` added two fields to four
`codegg_core::jobs::JobPayload` variants — `Test`, `ManagedArgv`, `Shell` and
`Python` — namely `sandbox_profile` and `network_intent`, and the branch's own
code constructs those payloads without them. Three construction sites are
filled with `sandbox_profile: None` and
`network_intent: NetworkIntent::LegacyUnrestricted`, which is what a legacy
payload decodes to when the fields are absent — the same spelling `main`'s own
tests use: the two M007 build-class binding tests in
`src/scheduler/submission.rs` and the C005 suite's shell job in
`tests/process_denial_c005.rs`. The remaining `JobPayload` constructions in
the tree are match patterns, not constructions, and needed no change.

**The reviewed read-only palette no longer matched the live catalog.** The
branch's C001 gate asserts that the live catalog's read-only set equals a
reviewed palette of tool *names*. `main` changed how the LSP tools are
registered: a profile that requires filesystem containment attaches a
**disabled stub** for `lsp` and `lsp_read` unless the daemon supplies an
M004-scoped service, and a disabled stub carries no contract of its own, so
its effect class is the conservative legacy default rather than `ReadOnly`.
The palette is therefore read from a registry built at the `FullHost` profile,
where the read-only tools are live; reading it from a constrained registry
would make the reviewed set flip with the profile and would report `lsp_read`
as "no longer read-only" when it is exactly as read-only as it was. The
palette itself is unchanged — a reviewed tool is not dropped to make a test
pass, and a live read-only tool is not un-reviewed.

**The writer-disposition guard failed on a module that does not write.** The
guard strips test-only module bodies before it scans a file for write
primitives, so a module whose only writes live in its unit tests is not
counted as a production writer root. `main` added `src/lsp/m004.rs`, whose
test module is gated by `#[cfg(all(test, target_os = "linux"))]` — a predicate
the stripper did not recognise, so the module's test-only `std::fs::write`
calls were read as production writes, and the guard demanded a disposition row
for a module that has none.

The fix is in the **guard**, not in the manifest: a disposition row would have
recorded a false positive as truth. The stripper now recognises *any* test-only
`cfg` predicate — `test` alone, `all(...)` with any test-only member, and
`any(...)` whose every member is test-only — and keeps a body whose predicate
can select it in a production build (`any(test, feature = …)`, `not(test)`, a
bare target). The self-test covers both directions: a test-only module is
stripped under each predicate shape, and a production-reachable module is kept
under each production shape. The guard reports 58 declared roots, 17 writer
tools and 35 writer modules on the merged tree.

### 10.4 The ADR renumbering, second time

The branch's writer-contract ADR has now been renumbered twice, and both times
for the same reason — `main` took a number the branch had already used:

| Number | Taken by `main` | The branch's ADR became |
|---|---|---|
| ADR-0042 | `ADR-0042-composer-normal-mode-key-ownership.md` | ADR-0044 (§3.1) |
| ADR-0044 | `ADR-0044-contained-git-live-objects-and-metadata-authority.md` | **ADR-0047** (§10.2) |

The ADR's *content* is untouched: the decision, the option-B acceptance record
and the 2026-10-19 acceptance date are unchanged. Only the identifier moved.
ADR-0044 is now `main`'s, and the number must not be reused for this line.

### 10.5 The Verified-counts table was re-measured, and one row changed measure

The table was **re-measured**, never merged. Every row was counted against its
own listed `Source` in the merged tree, and the counting method was first
calibrated by reproducing both sides' published values exactly on their own
trees — 290 test files, 83 tool source files, 90 architecture docs and 65
guard scripts on the branch; 286 / 80 / 87 / 62 on `main`.

Twelve rows moved: `codegg-core` modules 53 → **54**, `CoreRequest` variants
262 → **263** (239 struct + 24 unit), tool registrations 65 → **67**, distinct
production tool names **61** (unchanged), authorized operations → **257**, built-in
slash commands 167 → **168**, database tables 94 statements / 90 tables →
**95 statements / 91 tables**, storage layout version 85 → **87**, integration
test files 290 → **296**, and the directory tree's "Integration tests (286
files)" becomes 296.

The **authorized-operations** row changed its measure, and the change is a
correction rather than a drift. The two sides' published numbers had been
measuring two different things: this branch's 262 was the count of `CoreRequest`
variants reachable in `operation_descriptor` — the same number as the
`CoreRequest` row, which is why it tracked that row — while `main`'s 246 was
the length of `representative_requests()`. The latter is a *sample* of
operations, not the operation set: its doc comment claims "one representative
request per `CoreRequest` operation", but ten operations have no representative
request at all (`document_read`, `document_create`, `document_delete`,
`document_modify`, `document_capabilities`, `project_delete`,
`project_init_draft`, `project_init_publish`, `user_prompt_queue_send_now`,
`workspace_directory_list`). The row now names the measure it actually means —
the distinct operation names in `operation_descriptor`, which
`architecture/authorization.md` already calls the source of truth for the
operation set — and publishes **257**, with the representative list's 247
recorded beside it.

### 10.6 Registry and status-word hygiene

The merge also carried a drift the audit surfaced, and it is corrected here
rather than landed:

- **Closed work was listed as blocked.** The M005 C004, C005 and C006 rows sat
  in `## Blocked work` with the status words `Closed` and `Conditionally
  closed`, while the same three milestones also appeared in `## Recently closed
  or conditionally closed work`. The three blocked-table rows are removed; the
  recently-closed table is the single home for them.
- **C003 was missing from the recently-closed table.** It appears in the
  dependency-ready table as `closed` but had no row in the recently-closed
  table. It is added there, between C002 and C004.
- **The blackboard M005 row still claimed the shared profile was disabled.**
  It said `active; shared mutation disabled` and "Q1–Q5 remain unrun and
  shared-write unavailable", which C006's closure contradicted. The row now
  says the corrective closed C001–C006 with C006 conditional, the shared
  profile is opt-in and platform-gated (Linux and macOS qualified, Windows
  disabled), and the WorkOrder `shared`/`serialized` mutation policy still
  fails closed — so no WorkOrder agent is placed on a shared checkout.
- **The C003 row's note still said "C004–C006 remain blocked on their own
  predecessors."** They are closed on their own predecessors, C006
  conditionally.
- **The implementation plan's status line still said "Q1–Q5 and shared
  mutation remain unqualified".** Updated to the same truth.
- **Two roadmap rows still said "Linux-qualified"** while the roadmap's own
  header said "Linux and macOS qualified". Both now say Linux and macOS.
- **`architecture/writer_permit.md` still said the shared profile "is not live
  yet"** in its C003 section, while its C006 section said the opposite. The
  earlier paragraph now states that the C003-era boundary has since moved and
  points at the C006 section.
- **M008's shared-checkout arm was still recorded as blocked on "M005
  closure."** C006 discharged that gate: the shared checkout is a real opt-in
  path on the platforms the table qualifies. The M008 plan, the roadmap and the
  registry now record S4 as eligible on **Linux and macOS** and **unrun on
  Windows**, where the profile is disabled for a capability reason rather than
  an evidence reason. The two claims that carry into S4 unchanged are stated
  there: the three C005 process surfaces that are not refused pre-effect keep
  their recorded compensating controls, and a shared checkout still admits no
  unqualified process, so S4's Rust build arm is unavailable by contract.

### 10.7 What this reconciliation does not change

- **The M005 line's own state is unchanged.** C001–C005 stay closed, C006 stays
  conditionally closed, and the shared checkout stays **opt-in** and
  **platform-gated**. The Windows backend remains a capability gap, not an
  evidence gap: no filesystem-enforcement backend is registered for Windows at
  all, so no CI run could produce the missing evidence.
- **The composed two-agent race is still claimed at the carrier level only.**
  The re-measurement of the platform evidence did not change it, and a *live*
  two-agent race inside one daemon process is still not claimed — it needs a
  live model and the contained Git backend that does not exist in this tree.
- **No product behaviour changed beyond the migration renumbering.** The
  renumbering changes which version number a table is created at, not what is
  created: both migrations are `CREATE TABLE IF NOT EXISTS`.
