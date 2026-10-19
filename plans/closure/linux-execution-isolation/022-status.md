# Linux Execution Isolation C014 — M005 Production Owner Qualification Corrective — Closure Status

Status: implemented on branch — hosted exact-source qualification pending; M005 remains active and unqualified

Source implementation plan:

- `plans/implementation/linux-execution-isolation/022-m005-production-owner-qualification-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `6efd5ebdf90f91fc24dcac7f2827f4ca9e355135`

## 1. Executive finding

The M005 milestone was recorded as "active; internal candidate only" with the
remaining work described as qualification. A review against the source found
that the remaining work was **not only qualification**: two production owners
did not carry the network intent at all, and one of them is the primary command
route. Under that route the M005 refusal could not fire at all, because no
owner was ever told the principal asked for offline execution.

This corrective closes the owner-coverage gap: the durable payload now carries
the intent and the accepted profile ceiling, the bash dispatch threads them
from the authorization snapshot, the scheduler's shell executor applies them
and rebuilds the accepted containment, and the daemon stamps the principal's
persisted policy at job admission and at rerun submission. Three negative
owner-path unit tests and the missing native restricted-userns proof are added;
the hosted job's path triggers are extended to the M005 owner surface; and a
guard now protects the wiring.

M005 is **not** closed by this record. The obtained network fact is still never
reported, the tool-program child-job route is still unqualified, there is still
no machine-readable supported-host policy, and the hosted exact-source run has
not executed against this source. No production `NetworkEnforcement::Enforced`
claim is authorized, and C010 no-admission and M003 blocked status are
unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The durable command route carries the authenticated network intent | `JobPayload::Shell` / `JobPayload::ManagedArgv` in `crates/codegg-core/src/jobs/mod.rs`; `src/tool/bash/process.rs` dispatch | pass — the intent travels with the payload, `#[serde(default)]` keeps legacy payloads readable |
| The accepted profile ceiling travels with the payload | same; `src/scheduler/executors.rs` | pass |
| The authorization snapshot is the ceiling, not the request | `offline_required_…` unit tests in `src/managed_process.rs` | pass — a request-carried `LegacyUnrestricted` cannot widen an `OfflineRequired` authorization |
| The shell executor applies the intent and rebuilds the accepted containment | `src/scheduler/executors.rs` | pass — a constrained job whose host cannot provide containment fails closed instead of running uncontained |
| The daemon applies the principal's policy to payloads submitted without one | `CoreDaemon::stamp_principal_execution_policy` in `src/core/daemon_jobs.rs`, at `CoreRequest::JobSubmit` and at the rerun submission | pass — the stamp narrows only |
| `OfflineRequired` is refused under an unconstrained profile | `offline_required_refuses_unconstrained_profile_before_spawn` | pass |
| `OfflineRequired` is refused without a required sandbox launch | `offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | pass |
| A required process tier is refused before target exec on a restricted-userns host | `required_process_tier_fails_closed_when_user_namespaces_are_restricted` in `tests/linux_m004_process_ipc.rs` | pass (host-gated; the test previously did not exist and the CI filter matched zero tests) |
| The hosted qualification job can see the M005 owner surface | `.github/workflows/linux-m004-native.yml` path triggers | pass — the intent type, the payloads, and every consuming owner are now trigger paths |
| The wiring cannot be dropped silently | `check_network_intent_owner_wiring` in `scripts/check_sandbox_policy_wiring.py` | pass — proven non-vacuous by injecting a widening and observing the failure |
| The obtained network fact is reported | `SandboxEnforcement::for_constrained_enforced`; `LinuxObtainedEnforcement` | **not run — open.** `network` is only ever `ObtainedState::NotRequested`, `SandboxEnforcement.network` is always `NetworkEnforcement::Unrestricted`, and no code path constructs `NetworkEnforcement::Enforced` |
| The tool-program child-job route carries the intent | `src/scheduler/tool_program_executor.rs`; `ChildJobRequest` | **not run — open.** `ChildJobRequest` carries no intent and no ceiling; those payloads bypass the daemon stamp |
| A machine-readable supported-host policy exists | host capability probe | **not run — open.** The refusal is a runtime probe, not a declared capability set |
| Hosted exact-source M005 owner qualification | `process-ipc` job of `.github/workflows/linux-m004-native.yml` | **pending.** Has never run the M005 owner probe against this source |

## 3. Production implementation evidence

### 3.1 The gap that was closed

`src/tool/bash.rs:667-671` disables active routing when the sandbox is enabled.
A constrained profile with a workspace root therefore takes the direct route,
which was already M005-wired. A **FullHost** profile, or a constrained profile
with **no workspace root**, takes the durable route — and that route carried
neither the intent nor the profile. The managed-process owner's guard could
not fire, because the owner received an unconstrained request with no
`execution_authorization`.

The same gap removed containment: a constrained profile with no workspace root
was accepted and then executed with no sandbox at all.

### 3.2 What the changes do

- `crates/codegg-core/src/jobs/mod.rs` — `JobPayload::Shell` and
  `JobPayload::ManagedArgv` gain `sandbox_profile` and `network_intent`,
  matching the fields `Test` and `Python` already carried.
- `src/tool/bash/process.rs` — the dispatch computes the intent and the
  ceiling from the authorization snapshot and passes them into the payload. The
  background route already carried both on `BashBackgroundCtx` and now
  forwards them.
- `src/scheduler/executors.rs` — the shell executor reads the ceiling and the
  intent from the payload, sets them on the request, and rebuilds the sandbox
  spec through `sandbox_config_for_profile` when the ceiling requires
  containment, failing closed when the host cannot provide it.
- `src/core/daemon_jobs.rs` — `stamp_principal_execution_policy` applies the
  persisted per-principal preference to any payload that arrives without one.
  The effective intent is offline when either side asks for it; the effective
  profile is the more contained of the two, via the new
  `SandboxProfile::narrow` helper in `crates/codegg-core/src/approval.rs`.
- `crates/codegg-core/src/approval.rs` — `SandboxProfile::narrow` returns the
  more contained of two profiles.

### 3.3 CI defects corrected

- The `restricted-userns-fails-closed` job filtered on
  `required_process_tier_fails_closed_when_user_namespaces_are_restricted`,
  which did not exist. The filter matched zero tests, libtest exited zero, and
  the step was vacuously green. That test now exists, asserts the host really
  has `user.max_user_namespaces=0`, and asserts the target did not run.
- The job's path triggers covered none of the M005 owner surface, so a change
  to the intent type, the payload that carries it, or any owner that consumes
  it could not trigger its own qualification job. The trigger list now covers
  `crates/codegg-core/src/approval.rs`,
  `crates/codegg-core/src/authorization/**`, `crates/codegg-core/src/jobs/**`,
  `crates/codegg-core/src/session/schema.rs`,
  `crates/codegg-protocol/src/core.rs`, `src/agent/**`, `src/core/**`,
  `src/interactive_process_attach.rs`, `src/python_script/**`,
  `src/scheduler/**`, `src/tool/bash.rs`, `src/tool/bash/**`,
  `src/tool/test.rs` and `src/tool/python_script.rs`.

## 4. Verification executed

### Commands run

```bash
cargo check --workspace --locked --all-targets          # green
cargo test -p codegg --lib managed_process::               # 30 passed, 0 failed
cargo nextest run --profile ci --locked -p codegg --lib   # 4135 passed, 3 timed out, 4 skipped
python3 scripts/check_sandbox_policy_wiring.py             # pass
python3 scripts/check_execution_ownership.py               # pass
python3 scripts/check_scheduler_bypass.py                  # pass
python3 scripts/check_daemon_cwd_usage.py                 # pass
python3 scripts/check_queue_write_transaction_shape.py     # pass
bash scripts/check-core-boundary.sh                        # pass
cargo fmt --all -- --check                                # clean
scripts/verify.sh quick                                    # "Quick verification passed."
gh run list --workflow=linux-m004-native.yml               # hosted qualification
```

### The hosted run against this source

Run `39313802793` (2026-10-19, push, commit `d03b6267`) exercised this
branch's exact source on the hosted Linux runner. Its two qualification jobs
produced **different** results, and the difference is recorded rather than
flattened:

- `restricted-userns-fails-closed`: **success** (17m36s of a 20-minute
  budget). The required M004 launch refuses before the target under a host
  userns policy, and the constrained PTY and constrained LSP routes refuse the
  same way. This is the native restricted-userns proof, and it is now
  evidence.
- `process-ipc`: **cancelled** at the job's 20-minute timeout with the
  `linux_m004_process_ipc` binary still running. The cold-cache workspace
  build inside that job took 5m14s before the suite started, so the job's
  budget was spent on setup plus part of the suite. The cancelled step's log
  was truncated by the cancellation, so it records neither a pass nor a
  failure for the M005 netns proof in that suite.

The budget defect is corrected in the same branch: `process-ipc` 20 → 45
minutes, `restricted-userns-fails-closed` 20 → 35 (it passed with only
2m24s of headroom), and the C007 contained-staging proof is moved into its
own step with its own 10-minute budget ahead of the wider suite.

### The three new negative owner-path tests

```text
test managed_process::tests::offline_required_refuses_unconstrained_profile_before_spawn ... ok
test managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn ... ok
test managed_process::tests::request_carried_network_intent_cannot_widen_an_offline_authorization ... ok

test result: ok. 3 passed; 0 failed
```

### The `managed_process::` suite

The first run of `cargo test -p codegg --lib managed_process::` reported 25 passed
and 5 failed, every failure being:

```text
panicked at src/managed_process.rs:…:
  Spawn(Os { code: 2, kind: NotFound, message: "No such file or directory" })
```

Those five locate the sandbox helper at `target/debug/codegg-sandbox-helper`
(for example `src/managed_process.rs:3138-3143`). `cargo test --lib` builds the
library test binary and **not** the `codegg-sandbox-helper` bin target, so the
helper was absent and the spawn failed with `NotFound` — an absent executable,
not an assertion failure. Building the helper and re-running the same command
gives:

```text
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 6664 filtered out
```

### The nextest run, and the three timeouts

`cargo nextest run --profile ci --locked -p codegg --lib` reported
`4138/6690 tests run: 4135 passed (1 slow), 3 timed out, 4 skipped`, then
stopped at the first failure. The three timeouts are all in
`tool::lsp::tests::security_context_*`. The `ci` profile's
`slow-timeout = { period = "60s", terminate-after = 2 }` caps a test at 120 s,
and the `security_context_*` module takes **158.41 s** to run serially on this
host. Run in isolation the whole module passes:

```text
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 6656 filtered out; finished in 158.41s
```

These are load-related timeouts on a machine running several other builds
concurrently, not assertion failures, and they are in LSP code this corrective
does not touch.

### The plain parallel run, and the six TUI failures

A plain `cargo test -p codegg --lib` (one process, all tests in parallel)
reported `6684 passed; 6 failed`, all six in
`tui::commands::interactive_terminal::tests`. The same six failed identically
on a second run, and **all 37** `interactive_terminal` tests pass when the
module is run alone with `--test-threads=1`. Under the sanctioned
`nextest --profile ci` runner those six **pass** — and `.config/nextest.toml`
already grants whole-binary exclusivity to `interactive_terminal_tui` and the
other live-PTY binaries precisely because they "drive real" subprocesses and
"cannot contend on process resources". These are the parallel-interference
failures the repo's own runner configuration exists to avoid, in TUI terminal
code this corrective does not touch.

### The integration sweep

```bash
cargo nextest run --profile ci --locked -p codegg \
  --test scheduler_protocol_consistency --test scheduler_phase5 \
  --test durable_jobs_phase4 --test work_plan_eggplan_differential \
  --test bash_background_m002_routes --test scheduler_cancellation \
  --test scheduler_contention --test scheduler_restart_recovery \
  --test scheduler_authority_matrix --test scheduler_permit_lifecycle \
  --test eggwork_remote_execution --test eggwork_remote_execution_live \
  --test managed_process_descendants

Summary [  15.956s] 225 tests run: 225 passed, 0 skipped
```

These are the thirteen integration binaries whose job payloads this corrective
changed.

### The one failure that was not a regression, and its root cause

The first sweep failed on
`eggwork_remote_execution_live::live_derived_workspace_reuse_under_required_isolation`:

```text
eggwork: node 'live-node-1' does not satisfy required isolation capability
'isolation.landlock.workspace-rw.v1'; refusing before workspace upload
```

The refusal is produced by `src/scheduler/eggwork.rs:1509-1514` from the
node's **advertised** features, and this corrective's diff to that file is only
the two new payload fields plus a `..` in a destructure.

The node advertises the capability only when
`verify_trusted_helper(...).is_ok() && probe_landlock_ruleset()`
(`eggwork-runner/src/lib.rs:483-485`). The trust check walks the **entire
ancestor chain** of the helper and requires every ancestor to be owned by root
or the effective uid and to be **not group- or other-writable**
(`eggwork-runner/src/lib.rs:587-631`). This machine's worktree path is
group-writable:

```text
775 sugarwookie /home/sugarwookie/.codex/worktrees/032b/codegg
775 sugarwookie /home/sugarwookie/.codex/worktrees/032b
775 sugarwookie /home/sugarwookie/.codex/worktrees
```

The main checkout at `/home/sugarwookie/projects/codegg` is `775` as well, and
`/home/sugarwookie/projects` is `777`, so the same failure would occur there.
Tightening the chain to `755` makes the test pass unchanged:

```text
test live_derived_workspace_reuse_under_required_isolation ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out
```

The failure is a filesystem-permission property of the checkout path, in code
that lives in the external `eggwork` dependency, on a route this corrective
does not touch.

### Guard non-vacuity proof

The new guard section was proven non-vacuous by injecting a widening into
`src/tool/bash/process.rs` — replacing the payload's carried intent with
`network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted` —
and observing the guard fail with:

```text
sandbox-policy-wiring guard failed:
  src/tool/bash/process.rs:1306: production owner hardcodes the unrestricted network intent
```

The file was restored and the guard returned green. The guard also lists the
four sites that legitimately default to the unrestricted intent, each with the
reason it is allowed, so the allowlist cannot grow silently.

## 5. Open items carried forward

1. **The obtained network fact is never reported.** The M005 netns is obtained
   in reality; nothing reports it. `SandboxEnforcement::for_constrained_enforced`
   still sets `network: NetworkEnforcement::Unrestricted`, and
   `NetworkEnforcement::Enforced` still has no producer. The audit record
   therefore understates what was obtained. This is deliberately not changed
   here: the M005 plan defers the production `Enforced` claim, and emitting one
   belongs to the corrective that owns the obtained-facts contract. It also
   makes ADR-0044 clause 2's M005 precondition vacuous until then.
2. **The tool-program child-job route is unqualified.**
   `ChildJobRequest` carries no network intent and no profile ceiling, so
   those payloads default to unrestricted and bypass the daemon stamp.
   Closing it requires `ChildJobRequest` to carry the intent.
3. **There is no machine-readable supported-host policy.** The development
   host is Linux 6.8, whose Landlock ABI is below the 6 the M004/M005 scope
   requires, so native execution is unsupported there and the evidence has to
   come from the hosted runner. The refusal is a runtime probe, not a declared
   capability set.
4. **The hosted run has now executed against this source, but only half of
   it.** Run `39313802793` (2026-10-19, push, commit `d03b6267`) exercised this
   branch's exact source. Its `restricted-userns-fails-closed` job **passed**:
   the required M004 launch refuses before the target under a host userns
   policy, and the constrained PTY and constrained LSP routes refuse the same
   way. That is the native restricted-userns proof, and it is now evidence
   rather than an open item.

   The sibling `process-ipc` job in the same run was **cancelled** — not
   failed — when it reached the workflow's 20-minute job timeout with the
   `linux_m004_process_ipc` binary still running. The job's own budget was
   the cause, not a test result: the cold-cache workspace build inside that
   job took 5m14s before the suite even started, and the suite was still
   running 14 minutes later. The cancelled step's log was truncated by the
   cancellation, so it records **no** pass and **no** failure for the suite.

   The CI defect is corrected in the same branch: the `process-ipc` budget is
   raised from 20 to 45 minutes to cover the cold build plus the full suite,
   the `restricted-userns-fails-closed` budget is raised from 20 to 35
   minutes because it passed with only 2m24s of headroom, and the C007
   contained-staging proof is moved into its own step with its own 10-minute
   budget so a long suite cannot hide it. A `workflow_dispatch` on this branch
   still runs only the C002 cost pilot: the workflow's dispatch condition is
   `github.event_name != 'workflow_dispatch' || github.ref_name != '<this branch>'`.
5. **The M005 netns has no hosted end-to-end proof yet.** The hosted
   `process-ipc` job has still not completed a run of
   `managed_process_owner_applies_authenticated_offline_network_intent` against
   this branch's source, for the time-budget reason in item 4. The proof is
   written and the job now has the budget to reach it; it has not been
   observed passing, so it is recorded as **not run**.

## 6. Downstream effect

M005 remains **active and unqualified**. C010 no-admission and C012's
rejection of the in-process lock as a no-writer proof are unchanged, and no
Linux host is admitted. The C005 handoff of the Linux trusted Git cost
corrective remains blocked: it waits on accepted ADR-0044, M005 production
owner and host qualification, and the production C004 publisher and recovery
proof. The ADR-0044 live-read consistency review at
`plans/closure/linux-trusted-git-cost-corrective/006-status.md` does not
accept ADR-0044 and does not change any of those gates.
