# Managed Scratch Merge Reconciliation — Closure Status

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the corrective line and it opens no new milestone. It records
the reconciliation of the scratch workstream with `main` and the qualification
of the merged tree, as §11 of the source addendum requires: "later supplemental
evidence appended/linked rather than its history overwritten".

Source corrective addendum:

- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md`

Historical closure records this record supplements and does **not** rewrite:

- `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md` (C001, closed)
- `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md` (C002, blocked)

Repository baseline reconciled: `b832f66e` on `main`, which is **28 commits**
ahead of this branch's merge-base `61cbe922` (26 non-merge commits plus 2
merges, touching 108 files). The branch head at merge was `56a81432`; the
merge into `main` is `a5d88eb7`.

Implementation pull request:

- PR #128 (`https://github.com/dbowm91/codegg/pull/128`) — "Plan managed agent
  scratch spaces, delegation, and lifecycle", merged into `main` as
  `a5d88eb7` ("Merge pull request #128 from dbowm91/plan/managed-scratch-spaces").

## 1. Executive finding

The scratch authorization boundary survived the reconciliation with `main`
**unchanged**, and the merged tree is qualified. `main` touched no
scratch-owned file across its 28 commits — not `src/scratch.rs`, not
`src/managed_process.rs`, not `crates/codegg-core/src/agent_run.rs`, not
`src/agent/worker.rs`, not `src/tool/bash.rs`, and not the sandbox tree — so
the four conflicts were resolved by keeping both sides' semantics rather than
by negotiating the authorization boundary.

The reconciliation nonetheless found and fixed **four defects**, three of which
were `main`-side and would have left `main` red after the merge. The
load-bearing one is a hard authority gap, described in §3.4: on `main` as it
stood, a delegated child loop reached the tool broker with no workspace
identity and no workspace path policy, and **every** child tool call was
refused. A child could not read, and could not write into the scratch space it
had been provisioned. The same gap took the in-process standalone core's root
turn.

The merged tree is green on `main` in all three existing hosted lanes, and the
end-to-end delegated-scratch behaviour was observed live against a real daemon
and a real model, read from the durable record rather than from the model's own
account of it.

## 2. Reconciliation inventory

Merge commit on the branch: `1b36673b` ("Merge origin/main into
plan/managed-scratch-spaces"). Four conflicts, resolved without weakening an
authorization boundary:

| Conflict | Resolution |
|---|---|
| `src/tool/task.rs` | The scratch actions and the `scratch_alias`, `scratch_id`, and `shared_scratch` properties are kept verbatim; `main`'s clearer `run_id`/`task_id`/`group_id` semantics are appended to the same description. |
| `tests/tool_execution.rs` | Both sides independently rooted the `BashTool` in tests. `main`'s version supersedes this branch's: it roots at a per-test `TempDir` that lives for the test's duration, where this branch rooted at the process cwd. |
| `architecture/overview.md` | The Verified-counts rows are remeasured against their listed `Source` (268 integration test files, 58 CI guard scripts). |
| `plans/registry.md` | The union of both sides' rows, with the scratch row updated to record that C001 closed the Linux half of M003. |

The merged tree is byte-identical to the branch head: `git diff a5d88eb7
56a81432` is empty. The branch changed 88 files against `main` (+7,752/−230).

## 3. Defects found and fixed

### 3.1 Scratch admission could fail with `SQLITE_BUSY` (this branch's own code)

`acquire_scratch_attempt_leases` opened a deferred transaction and read
`agent_scratch_resource` inside it before inserting the leases, so the first
SELECT held a shared read lock and the first INSERT had to upgrade it — which
SQLite refuses without waiting. This is the C002 hazard already documented in
the same file for `create_or_get`; the C002 guard is scoped to the queue store
and never covered `crates/codegg-core/src/agent_run.rs`.

Observed live: **4 of 4** scratch admissions failed with "database is locked".

Fixed in `5acf5a9b` by taking the write lock earlier (`BEGIN IMMEDIATE`). It
changes no grant, lease, or authorization semantics — only the order in which
the transaction takes its lock.

### 3.2 The frozen M003 receipt no longer matched (`main` side)

`main` changed the `task`, `verify`, and `apply_patch` schemas after the
receipt was frozen, without extending the historical-schema reconstruction in
`src/tool_advisor/causal_frontier.rs`. `main` is red at `b832f66e` with the
same assertion and the same fingerprints.

Fixed in `ede10b77` by extending the reconstruction to restore each delta,
asserting the live value first. The frozen receipt asset is **untouched**:
disposition stays **D**, M002 stays the selected runtime frontier, and nothing
is skipped or waived.

### 3.3 The `repo_search` disabled-backend tests asserted stale wording (`main` side)

`main`'s host-scoped managed retrieval refuses a non-Eggsearch backend with
"repo_search requires the eggsearch backend" instead of naming the disabled
setting. The behaviour is unchanged; both assertions now match the current
wording and stay distinct from the eggsearch-but-unavailable message.

Fixed in `bd3bf784`, formatted in `aea3dabf`.

### 3.4 A delegated child had no tool authority at all (`main` side; found live and in CI)

`main`'s authority hardening (`590c713e`) makes every tool invocation carry a
workspace identity and a workspace path policy, and the broker refuses a call
whose execution context has neither. A delegated child loop holds no workspace
services lease, so it reached the broker with neither field and **every** child
tool call was refused with "verified tool authority is missing workspace path
policy". A child could not read, and could not write into the scratch space it
had been provisioned. The same gap took the in-process standalone core's root
turn, which has no lease either.

`origin/main` at `b832f66e` reproduces the child half exactly: it is red on
`a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root` with
the identical refusal. `main`'s own CI log is truncated before that test,
which is why the breakage was not visible there.

Fixed in `dda46fd6`. **The fix does not widen anything.** The per-workspace
path policy is the single-root policy over the workspace's canonical root — the
bundle always builds it that way — so the policy a child needs is the policy
over the root its executor bound it to: the *same* policy, not a stand-in. The
child now inherits the workspace identity the scheduler bound its job to and
installs that policy; the standalone root turn installs the policy for its own
canonical root. Scratch roots stay where they were — the tool-level scratch
scope, not the workspace policy — so a child's private root is still reclaimed
at terminal and a shared resource still outlives it.

Files touched: `crates/codegg-core/src/workspace_services.rs` (a root-only
path-policy constructor), `src/agent/loop.rs` (the loop's path policy),
`src/agent/agent_loop_factory.rs` (the standalone root turn),
`src/agent/worker.rs` (the delegated child), `src/agent/tool_batch.rs` (the
fingerprint source), `src/scheduler/executors.rs` (the workspace identity the
scheduler bound to the job), and the `SubAgentRequest` literals in
`src/tool/task.rs`, `src/job_dispatcher.rs`, `src/agent/snapshot_capture.rs`,
`src/agent/specialized_runtime.rs`, and `tests/subagent.rs`.

## 4. Qualification evidence

### 4.1 Hosted CI, pull-request event, head `56a81432`

| Check | Run | Result |
|---|---|---|
| `CI` — fmt, clippy, agent schema, the change-triggered guards, and the full workspace suite | [39339237406](https://github.com/dbowm91/codegg/actions/runs/39339237406) | pass (attempt 2) — **14,898/14,898 tests passed**, 7 skipped |
| Desktop E2E | [39339237356](https://github.com/dbowm91/codegg/actions/runs/39339237356) | pass |
| Windows process launcher | [39339237384](https://github.com/dbowm91/codegg/actions/runs/39339237384) | pass |

The C001 native-Linux fixture (`tests/linux_native_scratch_containment.rs`)
passed 10/10 in that run. It is a whole-file `#![cfg(target_os = "linux")]`
target, so the qualification does not exist off Linux, and a Linux host
without Landlock **fails** it rather than skipping it.

Attempt 1 of run `39339237406` failed on `codegg-core`'s
`goal::checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates` — an
append-ordering race in `crates/codegg-core/src/goal/checkpoint.rs`, a file
this branch does not touch, in a module unrelated to the scratch workstream.
`main` failed the same test with the same assertion in run `39294747903`, and
the registry already records that run as disqualified for exactly this reason.
The workspace suite runs `nextest` **without** `--no-fail-fast`, so one
unrelated flake cancels the tests that have not run yet. The test passes
40/40 locally. It was not "fixed" here: it is an unrelated pre-existing flake
in another module, and the repo's own precedent is to re-run rather than to
fold an unrelated repair into an unrelated change.

### 4.2 Hosted CI, post-merge `push` event on `main`, commit `a5d88eb7`

| Check | Run | Result |
|---|---|---|
| `CI` | [39345242783](https://github.com/dbowm91/codegg/actions/runs/39345242783) | pass — **14,898/14,898 tests passed**, 7 skipped |
| Windows process launcher | [39345242713](https://github.com/dbowm91/codegg/actions/runs/39345242713) | pass |
| Desktop E2E | [39345242816](https://github.com/dbowm91/codegg/actions/runs/39345242816) | pass (attempt 3) |

The Desktop E2E lane failed attempts 1 and 2 of that run on
`TestingLibraryElementError: Unable to find an element by:
[data-testid="workspace-list"]` — the renderer's 1-second `waitFor` budget in
`apps/desktop/src/App.test.tsx` — and passed attempt 3 on the identical tree.
This is a pre-existing flake in a module this change does not touch:
`git diff b832f66e a5d88eb7 -- apps/desktop/src 'apps/desktop/package.json'
apps/desktop/vite.config.ts apps/desktop/index.html` is empty for the renderer
surface, and the only `apps/desktop` change in the merge is a two-line addition
inside a Rust `mod tests` fixture in `apps/desktop/src-tauri/src/present.rs`.
The same lane passed on the identical tree in the pull-request event. It was
reproduced locally at 5 passes / 1 failure in six `vitest` runs. It was
deliberately **not** "fixed" here — raising the renderer's timeout budget is an
unrelated module, outside this workstream.

### 4.3 Local qualification (macOS, rustc 1.89.0)

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Focused suite — scratch, delegation, scheduler, sandbox, cleanup, permission, subagent, workspace-root authority | **694/694 pass** |
| `cargo nextest run -p codegg --test workspace_root_execution_authority` | pass (includes `a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root`, red on both the branch and `main` before the §3.4 fix) |

## 5. Live delegated-run evidence

The live runs drove a real daemon over ACP v1 (stdio) against a real provider
model, on an isolated `HOME` and `CODEGG_DAEMON_HOME` so the developer's own
daemon was never restarted. The durable record — `agent_run`,
`agent_scratch_resource`, `agent_scratch_attempt_lease`, `core_event_log` — is
what the claims below are read from, not the model's own account of them.

Session `ef01ab0a-5c15-48e7-9d12-97c6a73918e1`, model
`generalcompute/gemma-4-31B-it`:

- `scratch_create` minted shared resource `71a94151-661a-496d-aea7-623b8a569af0`
  under the child-local alias `shared-out`.
- Child A (granted the resource) wrote `child-a-shared.txt` into the **shared**
  root and `child-a-private.txt` into its **own** private root
  `runs/5669871a…`.
- Child B (not granted) wrote `child-b-private.txt` into its **own** private
  root `runs/814d8d7c…` — two distinct private roots, and no shared root
  reachable from the ungranted child.
- The shared artifact **survived** both children's terminal states; the
  parent's `cat` of the shared path returned `from-child-a-shared` (exit 0).
- `scratch_revoke` returned "Shared scratch revoked"; the resource is
  `revoked=1` and the shared directory is reclaimed.
- The private roots were reclaimed at child terminal (`runs/` is empty).

The model's own report section H records "Any tool call that was refused:
None."

**Partial runs, reported as partial rather than as passes.** Session
`26d6a3ad` (`minimax-m2.7`) ended on a provider rate limit part-way through;
session `e0b470e7` (`minimax-m2.7`) died on a rate limit after the children's
writes; `generalcompute/deepseek-v3.2` was rejected by the provider with HTTP
400 ("Streaming request failed"). These are provider-side limits.

## 6. Permission recovery, observed live

Session `3bfab29a`: a write to `/tmp/codegg-recovery-probe.txt` — outside every
authorized root — returned the actionable path suggestion "temporary path is
outside the authorized workspace; retry at
'<session scratch>/codegg-recovery-probe.txt'". Three seconds later the model
retried at exactly the suggested path and the write succeeded. The rejected
path was never created, no command was rewritten, and no grant was added: one
rejection, no repetition.

**Usability observation, reported rather than silently changed.** Under the
**default** permission policy a write outside the authorized roots raises a
*human approval prompt* rather than the tool-level hint, because a required
original location must still use the normal permission path
(`architecture/permission.md`). A model that wants `/tmp` therefore parks on a
person instead of receiving an actionable suggestion. The hint fires only
where policy already allows the write tool and the path boundary refuses it.
Changing the approval-prompt payload is a separate decision and is not made
here.

## 7. Retained external block

**Windows Required containment is not delivered by this workstream, and this
record does not change that.** C002 stopped at its own work-package-A
condition: the Windows Required filesystem backend is still owned by
`plans/subsystems/windows-native-sandbox-roadmap.md`, which keeps M004/C011
blocked. The original M003 remains conditionally closed for its **Windows**
half only; the Linux half is qualified by C001. `002-status.md` stands
unchanged, and this record does not reinterpret the owning roadmap's blocked
findings.

## 8. Known limits and non-claims

- **`--run` and `exec` cannot exercise this workstream.** Both build a reduced
  in-process agent loop with no `task` tool, no durable agent-run store, no
  scheduler, and no workspace path-policy lease. Only a daemon session turn runs
  the full path. The in-process core's *root* turn now installs the workspace
  path policy for its own canonical root, so it is no longer refused with
  "verified tool authority is missing workspace path policy" — but the scratch
  workstream still needs the daemon, because the reduced loop has no `task`
  tool and no durable agent-run store to carry a scratch grant.
- **No CI-level regression test for "the delegated child writes into its own
  private scratch"** was landed. A mutating child requires worktree isolation
  plus the full daemon service stack (submission service, run control, run
  groups, worktree service), which no existing test harness provides. The
  regression coverage that does exist is the red-then-green
  `a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root`
  test, the `src/tool/write.rs` unit tests, and the live run in §5.
- **The Linux native fixture cannot be executed off Linux.** It is a whole-file
  `#![cfg(target_os = "linux")]` target; the evidence in §4.1 and §4.2 is
  hosted-CI evidence, not local evidence.
- **Two pre-existing flakes are recorded, not repaired**: the `codegg-core`
  goal-checkpoint append race (§4.1) and the desktop renderer's `workspace-list`
  wait timeout (§4.2). Both are in modules this change does not touch, and both
  are documented in the registry and in PR #128's description.

## 9. Commands run

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo nextest run --locked --profile ci -p codegg \
  -E 'test(/scratch/) or test(/delegat/) or test(/scheduler/) or test(/sandbox/) \
      or test(/cleanup/) or test(/permission/) or test(/subagent/) or test(/workspace_root/)'
CARGO_BUILD_JOBS=3 cargo nextest run --locked --profile ci -p codegg \
  --test workspace_root_execution_authority
```

Live harness (isolated daemon; the developer's own daemon was never restarted):

```bash
export HOME=/private/tmp/codegg-iso-home CODEGG_DAEMON_HOME=/private/tmp/codegg-iso-home/daemon
python3 register_project.py /tmp/codegg-scratch-live
python3 acp_drive.py /tmp/codegg-scratch-live item3c_prompt.txt <log>.jsonl \
  generalcompute/gemma-4-31B-it 600
python3 dump_session.py <session-id>
```

## 10. Historical records preserved

This record adds to, and does not rewrite, the evidence history:

- `001-status.md` (C001) is unchanged; its native Linux evidence and run ID
  `39298041771` stand as recorded.
- `002-status.md` (C002) is unchanged; its blocked disposition and its
  stop-condition reasoning stand as recorded.
- The frozen M003 receipt asset is unchanged; disposition stays **D** and M002
  stays the selected runtime frontier.
- The M003 historical closure in `plans/closure/managed-agent-scratch/` is
  unchanged.

## 11. Factual correction — §6's two live observations came from different permission policies (2026-10-21)

§6 places two observations next to each other without stating that they came
from **different permission policies**, which lets a reader take them as one
trajectory. This section separates them. The historical §6 text is **not**
rewritten; this record "adds to, and does not rewrite, the evidence history"
(§10).

**The session evidence in §6 ran under a permissive policy.** Session
`3bfab29a` observed the tool-level hint and the model's successful retry at the
suggested scratch path while the isolated configuration carried

```json
"permission": { "default": "allow", "write": "allow" }
```

Under that policy the write to the host temp root was **allowed outright**, and
the actionable message came from the write tool's own boundary refusal, not
from anything the permission layer did.

**The usability observation in §6 describes the default policy.** Under
`permission.default = "ask"` with no explicit rule, the same write instead
raises a *human approval prompt*. That observation was taken in a different
session, `2164790b`, which parked on an unanswered human decision and failed
after eleven minutes.

**So the two halves of §6 are not one story.** The usability observation is a
true and useful statement about the default policy: a model that wants `/tmp`
parks on a person instead of receiving an actionable suggestion. The session
evidence printed immediately above it does **not** illustrate that observation,
because it was collected under a policy where the write was permitted.

**What changed afterwards, and where the corrected evidence now lives.** M005
implemented the redirect offer — the model is answered with the actionable
scratch path *before* the human is asked, and only a repeated identical
attempt reaches the human. M005's closure record states this distinction
explicitly in its §13, and its §4 records the live observation that closes the
gap **under the default policy**: two sessions in which the first attempt was
answered with the guidance and published no `permission_pending`, a third in
which a repeated identical attempt published `permission_pending`, and two
further sessions reproducing the behaviour on a second model and a second wire
surface. See `plans/closure/managed-agent-scratch/005-status.md`.

The behaviour M003's §6 reported is therefore **still accurate as a
description of the pre-M005 product**, and the usability gap it identified is
**closed by M005**. The correction here is to the *pairing*, not to either
observation.
