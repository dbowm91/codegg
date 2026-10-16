# Agent Workstream Blackboard M008 — Isolated S0/S1 Results

Evidence status: exploratory partial result; M008 remains active and is not
strictly closed.

Source plan: `plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md`
CodeGG source: `a0d7acef8e428d55971514addf685741a25ce047`
Fixture baseline: `f5e27d388255726b0b85226a144fdcce2e91b73a`
Machine-readable measurements: `plans/closure/agent-workstream-blackboard/008-isolated-results.json`

## 1. Protocol

The fixed task implemented the same capped exponential retry-delay contract in
a dependency-free Rust crate and Python package. Each package had five frozen
contract tests. No trial changed tests or dependencies. S0 used one CodeGG
`build` agent for both packages. S1 used two CodeGG `build` invocations at the
same baseline, each in its own linked Git worktree, one package per child;
changes were integrated sequentially and tested again.

All runs used CodeGG 0.1.0 built from the source above with Rust 1.89 for
`aarch64-apple-darwin`, provider/model `opencode_go/gpt-6-luna`, yolo approval,
and a requested `workspace-write` sandbox. The per-invocation output budget
was the same; CodeGG `exec` offered no aggregate token-budget field. There was
no provider seed. Trial order was S0-A, S0-B, S1-A, an S2 availability probe,
then S1-B. This is a small, unrandomized measurement.

The process environment was macOS 26.6.2, 24 GiB RAM, running x86_64 under
Rosetta; the fixture tests used Cargo/Rust 1.98.1. Each headless CodeGG run
logged that no workspace root was available for the requested
`workspace-write` profile. The fixture edits were confined to `/tmp` in the
observed Git diffs, but effective OS sandbox containment was not established.
This makes the runs exploratory evidence rather than a complete M008
qualification.

## 2. Trajectory results

| Trial | Outcome | Agent tokens | Agent/process time | Peak RSS | Integrated result |
|---|---|---:|---:|---:|---|
| S0-A | One agent completed both modules | 14,921 | 71.20 s | 119,635,968 B | Rust 5/5 and Python 5/5 passed; evaluator reran both suites |
| S0-B | One agent completed both modules | 14,784 | 47.67 s | 119,603,200 B | Rust 5/5 and Python 5/5 passed; evaluator reran both suites |
| S1-A | Rust child completed; Python CLI did not return within 300 s | 12,626 reported; Python unknown | 308.37 s until Python client termination | 125,681,664 B Rust; 117,850,112 B Python | Evaluator recovered the staged Python edit, committed it, and explicitly integrated both children; integrated Rust 5/5 and Python 5/5 passed |
| S1-B | Both isolated children completed | 27,140 total | 79.01 s to slower child; about 1.2 s for merge and integrated tests | 221,085,696 B, sum of child individual peaks | Both child suites passed; sequential integration had no conflict; integrated Rust 5/5 and Python 5/5 passed |

S0-A and S0-B results are commits `91c2179f` and `32c48d8d`. S1-A's Rust
child committed `c0672fa7`; the Python client timed out without a final JSON
result after staging its edit. The evaluator committed that edit as
`e1d58a52` after the timeout and produced integrated commit `f906bccd`. S1-B
child commits are `32d565ed` (Rust) and `3518ed73` (Python), integrated at
`0748ecc8`. Full SHAs and per-child measurements are in the JSON file.

The completed S1-B trial used 82.7% more reported tokens than the mean of the
two S0 trials. Its approximately 80.2 s total was about 35% longer than the
S0 mean of 59.4 s. This is one successful S1 repetition against two S0
repetitions, and S1-A required timeout recovery. It does not establish a
repeatable performance difference. Peak memory for S1 is the sum of the two
children's separate maxima, not a sampled concurrent process-tree peak.

The S2 headless probe returned without starting children because tool
discovery exposed no `task.spawn_many` or `wait_group` action. No substitute
coordination was used. S3 was therefore not run. A separate TUI availability
probe failed before any M008 prompt was submitted and is excluded from the
measured trials.

## 3. Cargo target-directory contention

Two cold, offline `cargo check --locked -p codegg-core` processes were run
with two Cargo jobs each against the exact CodeGG source above. Both arms
passed:

| Target layout | Wall | User CPU | System CPU | Time-reported max RSS | Target disk |
|---|---:|---:|---:|---:|---:|
| Two Cargo processes, one shared target | 211.23 s | 286.09 s | 44.25 s | 1,088,335,872 B | 1,159,948 KiB |
| Two Cargo processes, separate targets | 233.69 s | 630.37 s | 94.17 s | 1,435,226,112 B | 1,167,808 KiB each |

The shared-target process waited on Cargo's build-directory lock and reused
the first process's completed artifacts. Separate targets compiled
independently, took 10.6% longer end to end, used 2.19 times the combined CPU
time, and occupied about twice the target disk. These are single cold runs;
the time-reported RSS is not a sampled sum of concurrent process memory.
This measurement does not qualify a shared checkout, does not establish a
general Cargo policy, and does not change M005's gate.

## 4. Limits and next work

This task only exercised independent files and clean sequential integration.
There is no live evidence here for S2 peer observations, S3 claims, coupled
Rust API/schema changes, overlapping-path recovery, prerequisite chains,
branch drift, reviewer/producer coordination, interrupted build recovery, or
S4 shared-checkout execution. The existing M009 and M010 closures remain
deterministic correctness evidence; they do not replace the missing live
comparison. The previous S0/S1 pilot in `008-pilot-results.md` remains a
separate experiment and is not pooled with these trials.

No effectiveness improvement is established. M008 remains active. A valid
S2/S3 run needs an execution context that exposes the production TaskTool and
its group-scoped peer/claim surfaces, plus a verified workspace root for the
requested sandbox. Complete the remaining task strata and repeat S1 after the
timeout before writing the M008 strict closure record. S4 remains unavailable
pending M005. No downstream plan is unblocked by these partial results.

## 5. Live headless and Darwin terminal sandbox smoke (2026-10-16)

Using the source-built arm64 binary, an isolated daemon/data root, a resolved
temporary project registration, and a managed worktree session, a harmless
provider-backed `headless` turn completed. Provider metadata and the model
catalog were staged into the trial database by opaque reference only; no
credential material was read or copied.

The first terminal-tool smoke wrote a unique probe under `/tmp` despite the
session's `workspace_write` policy. Source tracing found that the legacy
`terminal` tool did not propagate the tool execution authorization or build a
required sandbox request. The file was removed. After the corrective change
at source commit `473579066803469e130b21d3daf2bf645658a115`, focused terminal
tests passed 5/5 and the sandbox-policy and execution-ownership guards passed.
A fresh daemon-backed terminal call then wrote/read/deleted a temporary file
in the managed worktree and received `Operation not permitted` for the `/tmp`
write; a host check confirmed the outside probe was absent. The observed limitation is
Darwin Seatbelt plus the tested terminal path/session root only. It is not
shared-checkout or all-writer evidence.

No provider-backed comparative trial has yet started against this corrected
source. S0 and a multi-agent isolated arm must still use the frozen
Rust/Python shared-interface task. The pilot and earlier S0/S1 results remain
separate and are not pooled.

## 6. Frozen shared-interface workload v1 (2026-10-16)

The new fixture at
`plans/closure/agent-workstream-blackboard/008-shared-interface-fixture/`
contains 17 frozen files. Its relative-path/content SHA-256 stream is
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1`. The
Rust starter baseline is 0/7 tests and the Python starter baseline is 0/7
tests, with failures reaching the intentional TODO implementation points.
These local evaluator baselines are not coding-agent trials. No provider-backed
trial has started. Recompute the tree digest before each trial and reject any
copy whose digest differs.

## 7. Provider-backed shared-interface follow-up (2026-10-16)

Fixture v1 was copied to an isolated repository with a resolved repository
binding and launched through the source-built daemon-backed `headless` path.
The S0 `build` agent completed both packages in 171.14 seconds. Its final
Rust suite passed 7/7, Python suite passed 7/7, and Rust-produced JSON passed
the Python consumer's golden check. The agent initially had a Python UTC
validation failure, corrected the implementation, and reran the evaluator
successfully. This is a completed S0 outcome, with the initial failure retained
as part of the trajectory. It does not provide a comparison without a
completed multi-agent arm.

S1 orchestration attempts after the M011 repository-identity correction
created children with separate managed worktree IDs and the resolved
repository ID. The latest group (`7fa853fe-0c67-4fcd-968f-93ecb923d8db`)
contained one Rust producer and one Python consumer. Both remained before the
provider invocation boundary: no heartbeat, journal, or usage progress was
observed after admission. The parent headless turn returned before the child
group completed. Both child jobs were cancelled through normal scheduler
`JobCancel`, and no changes were integrated. These are incomplete
orchestration/provider attempts, excluded from success and performance
denominators. Session/run/worktree identifiers and trial data are in
`008-isolated-results.json`.

M011 (`plans/closure/agent-workstream-blackboard/011-status.md`) records the
corrective that enabled child worktree allocation: root `TurnRunInput` now
carries the resolved repository identity from the session workspace binding.
The S1 stalls remain unresolved and do not demonstrate whether multi-agent
work is effective. M008 remains active; S2/S3 and the other task strata remain
open. No effectiveness improvement is claimed.
