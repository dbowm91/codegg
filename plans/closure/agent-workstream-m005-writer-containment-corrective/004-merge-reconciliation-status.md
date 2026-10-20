# M005 Writer-Containment Corrective — Merge Reconciliation with `main`

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the corrective line and it opens no new milestone. It records
the reconciliation of the M005 writer-containment branch with `main` and the
qualification of the merged tree, as §11 of the source addendum requires:
"later supplemental evidence appended/linked rather than its history
overwritten".

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
  every reference to it is updated (§3.1).
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
filed `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md`
(the restricted shared-checkout writer contract) while its number was still
free on the branch's fork point.

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
- `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md`
  is preserved as `plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md`,
  with its content, its decision and its acceptance record unchanged.
- `main`'s `ADR-0042-composer-normal-mode-key-ownership.md` and `ADR-0043`
  are untouched.
