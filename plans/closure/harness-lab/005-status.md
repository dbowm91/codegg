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
implemented. M005 is conditionally closed because the first real-model pilot was
not safely executable in this environment. The exact OpenCode Go profile pinned by
CodeGG has no reviewed LongCat 2.5 route, and no secret-safe credential handoff
was available for the temporary key supplied in chat. No live model call was made.

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
| Frozen-baseline live coding pilot on LongCat 2.5 and Muse Spark 1.3 | OpenCode Go profile audit and execution environment | blocked | The shared profile lacks LongCat's exact route. No safe credential handoff is exposed by the runtime. A provider route was not guessed and the chat credential was not copied to command arguments, files or logs. |
| Hosted CI evidence | pending exact-head run | pending | Updated after CI dispatch. |

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
Hosted CI — pending
```

Only completed runs will be marked as evidence here. The live provider pilot is
separate from credential-free CI and remains unrun.

## 5. Remaining conditions

1. Add the exact LongCat 2.5 OpenCode Go route to the shared provider-profile
   contract through its reviewed ownership path, then update CodeGG's pinned
   dependency revision. CodeGG must continue to fail closed for unknown models.
2. Provide a runtime secret-safe handoff for the approved temporary Go key; do
   not put it in a repository, command line, checkpoint, stdout/stderr, or ledger.
3. Run both approved models against identical frozen synthetic coding tasks and
   record successful/failed task outcomes, actual tool selection, repeated reads,
   recovery signals, provider-reported token counts and provider-reported cost if
   available. Keep unavailable fields explicit and run both baseline subjects
   before any automatic candidate correction.
4. Capture a valid accepted correctness-fix trace and live compile-correction
   trace before M004 can be strictly closed. No baseline defect is fabricated to
   force an acceptance result.

## 6. Planning audit

M001–M003 remain closed. M004 remains conditionally closed: the deterministic
candidate gate and bounded retry infrastructure remain in place, while its live
proposal/correction and accepted-candidate evidence conditions are still open.
M005 is conditionally closed on the named provider-route and credential-handoff
conditions above. The Harness Lab registry contains no later milestone or blocked
successor; therefore this closure unblocks no further Harness Lab plan. No
eligible plan in this line was left unstarted.

## 7. Registry updates

The subsystem roadmap and registry record M005 as conditionally closed and name
the remaining operational blockers. This is a formal closure disposition, not a
claim that the live pilot or autonomous acceptance trace has been completed.
