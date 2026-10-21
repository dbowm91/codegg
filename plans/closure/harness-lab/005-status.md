# Developer Harness Lab Milestone 005 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/harness-lab/005-optimization-controller-and-agent-pilot.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md`

Repository baseline reviewed: `5155555674bb87cc3b18a4672e5ae332facc3596`

## 1. Executive finding

The bounded multi-iteration assess/propose/build/retest controller, frozen input
binding, append-only hash-linked run log, cumulative wallclock deadline, candidate
resume validation, and additive ACP usage/argument-fingerprint telemetry are
implemented. After OpenCode Go was configured in the current CodeGG installation,
the live catalog exposed both approved model routes and the stored credential was
usable without copying its value. A frozen Task 1 pair was started. LongCat
completed the task and its verifier passed; Muse Spark's turn failed without
terminal usage and left the baseline implementation unchanged. The pilot stopped
before Task 2 under the configured usage stop rule. M005 remains conditionally
closed; the redacted receipts are in
`plans/closure/harness-lab/evidence/live-pilot-20261021.json`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Configurable outer iteration scheduler | `optimize` CLI and `run_optimizer` | implemented | Independent candidates start from the immutable baseline; bounded iterations, compile retries and wallclock are configurable. |
| Acceptance requires expected outcomes, complete pairing and no regression | `qualify_candidate` | implemented | Acceptance also requires at least one measured expected correctness improvement. |
| Hash-linked interrupted-run recovery | `OptimizationCheckpoint`; `test_optimization_checkpoint_is_hash_linked_and_resume_validates_inputs` | pass | Resume binds baseline, suite, prompt digest, scope, retry/iteration bounds and original deadline. Ambiguous partial proposals are preserved and refused. |
| Candidate partial evaluation recovery | `validate_candidate_resume_rows`; `test_candidate_resume_preserves_completed_prefix_and_rejects_changed_suite` | pass | Existing task/repeat rows are reused only after suite, case and group validation; missing pairs remain eligible. |
| ACP usage and tool-selection evidence | `src/acp.rs`, `architecture/acp.md`, Python ledger | implemented | Provider-reported token fields and argument projection hashes are additive; cost is not estimated. |
| Python harness suite | `python3 -m unittest discover -s dev/harness_lab/tests -v` | pass | 15 tests. |
| ACP Rust library coverage | `cargo test -p codegg --lib acp` | pass | 11 passed, 7,165 filtered. |
| ACP Rust integration coverage | `cargo test -p codegg --test acp_stdio` | pass | 1 passed. |
| Rebased quick qualification | `scripts/verify.sh quick` | pass | Formatting, repository/schema/boundary/provider guards and `cargo check --workspace --all-targets --locked` passed. |
| Frozen-baseline live coding pilot on LongCat 2.5 and Muse Spark 1.3 | `evidence/live-pilot-20261021.json`; committed task fixtures at `dev/harness_lab/pilot/baseline` | partial | Both routes were catalogued and Task 1 ran from the same frozen fixture. LongCat passed all 10 tests with provider token counts. Muse Spark ended in a native turn failure, reported no terminal usage, made no task-file changes, and left all 10 baseline tests failing. Cost was unavailable for both; no estimate was substituted. The usage stop rule prevented Task 2 calls. |
| Hosted CI evidence | `CI / verify` run `39470157041` | pass | Green on implementation commit `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`; hosted run completed in 28m17s. The preceding run found the lock-file Clippy issue, which `63bc46b` fixes. |

## 3. Implementation and safety evidence

The `optimize` command creates a candidate worktree per iteration from one frozen
baseline, applies path-scoped proposals, runs independent build and ACP gates,
compares paired outcomes, and stops on verified acceptance or configured bounds.
Run events are append-only and hash-linked; the atomic pointer must match the log
tail. Resume rejects changed inputs, changed candidate patches, changed binaries,
and ambiguous proposal state. The original wallclock deadline survives process
restart. Candidate code cannot control its evaluator, and accepted code is neither
merged nor pushed by the controller.

ACP now returns optional `codeggUsage` fields from provider-reported terminal
usage and includes only a digest of the bounded tool-argument projection. The
ledger labels missing token/cost metrics as unavailable. No price estimate is
reported as actual cost.

## 4. Verification executed

```text
python3 -m unittest discover -s dev/harness_lab/tests -v — 15 passed
cargo fmt --all -- --check — passed as part of scripts/verify.sh quick
cargo test -p codegg --lib acp — 11 passed
cargo test -p codegg --test acp_stdio — 1 passed
scripts/verify.sh quick — passed
Hosted CI — `39470157041` passed on implementation commit `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`
```

The hosted [`CI / verify` run 39470157041](https://github.com/dbowm91/codegg/actions/runs/39470157041)
passed all guards, formatting, workspace Clippy, sandbox-helper build and workspace
tests on `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`. An earlier run on the initial
implementation commit failed Clippy because the persistent lock file did not state
its truncation behavior; `.truncate(false)` was added, local workspace Clippy passed,
and the follow-up hosted run passed.

The live provider pilot is separate from credential-free CI. Its partial result,
including the failed turn and unavailable usage fields, is recorded without
capturing provider response bodies or credential material.

## 5. Remaining conditions

1. Before making another live call, resolve why Muse Spark's failed terminal
   event omitted provider usage or establish a provider-side hard usage ceiling
   that remains enforceable when the agent turn fails. The current pilot stopped
   on that missing usage as required by M005's spend rule.
2. Once usage accounting is safe, run the frozen Task 2 pair from the identical
   committed fixture and record both outcomes. Preserve the existing Task 1
   receipts; do not apply automatic corrections between subjects.
3. Provider-reported cost remains unavailable for both Go routes. Keep it null
   with an explicit reason; do not substitute a price estimate.
4. Capture a valid accepted correctness-fix trace and live compile-correction
   trace before M004 can be strictly closed. No baseline defect is fabricated to
   force an acceptance result.

## 6. Planning audit

M001–M003 remain closed. M004 remains conditionally closed: the deterministic
candidate gate and bounded retry infrastructure remain in place, while its live
proposal/correction and accepted-candidate evidence conditions are still open.
M005 is conditionally closed on the usage-accounting and remaining paired-task
conditions above. The live model routes and secret-safe credential access were
verified in the current installation. The Harness Lab registry contains no later
milestone or blocked successor; therefore this closure unblocks no further
Harness Lab plan. No eligible plan in this line was left unstarted.

## 7. Registry updates

The subsystem roadmap and registry record M005 as conditionally closed and name
the remaining operational blockers. The pilot is partial and the autonomous
acceptance trace remains open; neither is represented as complete.
