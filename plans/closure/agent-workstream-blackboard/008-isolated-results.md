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
