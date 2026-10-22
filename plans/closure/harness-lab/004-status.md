# Developer Harness Lab Milestone 004 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/harness-lab/004-opencode-proposer-and-candidate-gates.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md#milestones`

Repository baseline reviewed: `7a1b9d9a1d5a28e7919e3d69851a3ae710b0afbf`

Implementation commits or pull requests:

- `d64da3d8951d743b6aae8a29f6fa3a390e6d4cb5` — bounded proposer adapter, candidate worktree and qualification gates
- `5c263920988d4ba8ece2c2b5d992243b50db5722` — budgeted OpenCode compile-correction retry and checkpointed retry attempts

## 1. Executive finding

M004 is conditionally closed. Manual and optional OpenCode proposal paths, version-pinned event parsing, candidate-only worktrees, immutable baseline checks, separate build targets, changelog/scope gates, paired ACP qualification, and fail-closed acceptance are implemented. The controlled candidate loop now has **both** candidate traces the plan asks for: an **accepted** trace, in which a genuine harness defect was remediated and the paired score accepted it, and a **rejected** trace, in which a well-formed candidate that changed no measured outcome was rejected on the score with every build gate green.

The second trace is what exposed a defect in the score contract itself. The rejected candidate relabelled its failure instead of fixing it, and the first contract accepted it — with an empty correctness-improvement list. The contract was corrected (version 2): an acceptance now rests on a substantive improvement, and in the default contract that is a correctness improvement alone. The accepted candidate's verdict was re-verified from the same recorded trials under the corrected contract and is unchanged; the rejected candidate is now rejected with the primary reason `no_substantive_criterion_improved`. The controller therefore rejects candidates without a verified correctness improvement; it does not fabricate acceptance evidence, and it no longer accepts one that merely got faster.

The remaining operational condition is discharged: the **live** compile-correction trace exists. A candidate carrying a real compile failure was run through the qualification path; the compiler's own diagnostic was handed to an authenticated OpenCode proposer; its correction was re-gated, compiled, tested and scored. Producing that trace exposed a defect in the harness itself — the focused test gate ran *before* the build gate, so a compile failure in library code surfaced as a test failure and the proposer was never given the diagnostic to correct. The gate order is now explicit and asserted by a test.

M004 is closed on the evidence recorded here: an **accepted** trace (a genuine defect remediated, scored `correctness_fix`), a **rejected** trace (a well-formed candidate that changed nothing, rejected on the score with every build gate green), an **interrupted** qualification resumed without a duplicate receipt, and a **compile-correction** trace whose correction was authored by a live proposer from the compiler's own diagnostic. The candidate code remains in its candidate worktree, unmerged and unpushed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Manual proposer and dry-run with no provider calls | `improve --proposer manual --dry-run` | pass | Returns a recorded synthetic rejection. |
| OpenCode adapter version and event contract | installed `opencode --version` 1.19.0; fake executable tests | pass | Adapter pins 1.19.0; live proposal was not invoked. |
| JSON parsing, permission event, malformed output, timeout/process cleanup | fake OpenCode fixtures | pass | Python test suite includes process timeout and cleanup. |
| Exact-baseline candidate branch/worktree | candidate worktree unit test | pass | Verifies named branch and exact requested base SHA. |
| Patch scope, untracked-file digest and changelog enforcement | qualification gates and tests | pass | Forbidden path rejection was exercised and checkpoint recorded REJECTED. |
| Isolated compile/test and full frozen ACP suite | qualification implementation | pass | Separate target directory, focused ACP test, build, then paired suite; no accepted candidate was available to run through success. |
| Immutable baseline and paired regression gates | qualification implementation | pass | Baseline manifests/index prefix are checked; candidate acceptance requires paired non-regression plus a primary correctness improvement. |
| Bounded compile-correction retry and checkpointed attempt identity | `qualify_candidate`; retry transitions are covered by checkpoint resume test | pass | OpenCode-only retries rerun candidate build gates. The **live** correction ran: see the compile-correction trace below. |
| A live compile-correction trace | `plans/closure/harness-lab/evidence/compile-correction-20261022.json`; §3.1 | pass | A real compiler diagnostic from the candidate's own library code was handed to an authenticated OpenCode proposer, whose correction was re-gated and scored. |
| Full interrupted qualification resume and accepted candidate trace | M004 work package D; `plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §5.4 and §5.5 | pass | A real `SIGKILL` to the qualifying process group after a persisted trial; the resume reused the prior row with no second request and no duplicate, the denominator stayed 13, and the accepted candidate's paired score is `accepted` with the objective `correctness_fix` and one improvement and zero regressions. |
| A rejected candidate trace | `plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §5.7; receipt preserved at `plans/closure/harness-lab/evidence/rejected-candidate-20261022.json` | pass | A second, incomplete candidate passed all three build gates and was **rejected** on the score: `no_substantive_criterion_improved`, `correctness_improvements: []`, the defective case still unmatched. It was rejected for doing nothing, not for failing to build. |
| Authenticated live OpenCode one-cycle pilot | optional manual pilot | not run | No provider call was requested or made. |

## 3. Production implementation evidence

`dev/harness_lab/improve.py` provides checkpointed state transitions and budget checks, ManualProposer and an OpenCode CLI adapter pinned to 1.19.0, local candidate branch/worktree creation, patch hashing including untracked files, path/changelog validation, separate Cargo targets, focused build/test gates, bounded compile-correction retries with new attempt identities, full ACP evaluation, paired comparison, and final acceptance only on measured primary correctness improvement without regression. It never integrates the candidate into the default worktree or pushes a branch. Synthetic manual and out-of-scope rejection paths have receipts.

The acceptance rule is the versioned score contract in `dev/harness_lab/scoring.py`, which now carries an explicit *substantive improvement* rule: only a criterion that declares `improvement_accepts` may carry an acceptance, and in the default contract that is the correctness criterion alone. A reliability or latency movement is measured and reported in the receipt's reasons, but it cannot *be* the acceptance. The contract's version moved from 1 to 2 when this rule was added, so a version-1 contract is refused rather than reinterpreted.

The candidate gates run in a fixed order that the compile-correction mechanic depends on: format, **build**, focused test. The build gate precedes the focused test gate on purpose. The focused test gate compiles the same crate, so a test-first order surfaced a compile failure as a *test* failure and the proposer was never given the compiler's diagnostic to correct — the mechanic could only ever fire for a binary the focused test does not build. The order is named in `CANDIDATE_GATE_SEQUENCE` and asserted by a test, so a future reordering cannot silently close that path again.

### 3.1 The live compile-correction trace

The plan asks for a *live* compile correction: the OpenCode proposer answers a
compiler error and the corrected build is re-gated. The receipt is preserved at
`plans/closure/harness-lab/evidence/compile-correction-20261022.json`.

**What was induced.** The candidate is the accepted workspace-binding patch with
one deliberate compile fault, so the mechanic is exercised against a real
compiler error rather than a mock. The fault is one token — a borrowed path
where an owned one is expected:

```
src/exec.rs:172:13: error[E0308]: mismatched types: expected `PathBuf`, found `&PathBuf`
error: could not compile `codegg` (lib) due to 1 previous error
```

The candidate's own changelog records the induction, so the artifact is not
misleading about it.

**What the proposer did.** The build gate failed, the harness handed the
compiler's own diagnostic to the authenticated OpenCode proposer on
`opencode-go/muse-spark-1.3-contributor`, and the proposer removed the `&`.
Nothing else changed: after the correction the four source files are
**byte-identical** to the accepted candidate's, and the changelog — including
the note about the induction — was preserved. The correction prompt carried the
authorised path scope and the instruction not to claim tests passed.

**What was re-gated.** The corrected build compiled, the focused ACP test
passed, and the corrected candidate was then qualified on the frozen suite:

| Item | Value |
|---|---|
| Status | **accepted** |
| Objective | `correctness_fix` |
| `compile_correction_retries` | **1** |
| Evidence pairs | 13/13 |
| Correctness improvements | `headless-exec-workspace-write` (`execution_failed` → `passed`) |
| Regressions | none |
| Gates | `cargo fmt --all -- --check` → 0, `cargo build --bin codegg --bin codegg-sandbox-helper` → 0, `cargo test -p codegg --test acp_stdio` → 0 |
| Binary | `e5c4afdc133f4ec843e61243c2820744d06e1f53a1c38b8b1901cfe54ffa9f0e` |
| Patch | `a736e9e5ce8f0108504d9089e84b30784d70e2dd316a301453323b1492718571` |
| Budget | 1 provider request, 83,417 tokens, `cost_usd` 0.00199 — inside the admitted caps |

**The trace exposed a harness defect of its own.** The first attempt at this
trace did not produce a receipt: the candidate compiled after the correction and
the suite ran, but one unrelated trial ended in `infrastructure_error` — the
known load-dependent flake — so the pairing was incomplete and the comparison
refused to score it. That is the fail-closed behaviour working, and the resume
re-ran only the missing pair. The deeper finding was the gate order: the
focused test gate compiled the same crate as the build gate and ran *first*, so
a compile failure in library code surfaced as a test failure, and the correction
mechanic — whose entire purpose is to correct a compile failure — was never
reached. The order is now format → **build** → focused test, named in
`CANDIDATE_GATE_SEQUENCE` and asserted by a test.

## 4. Verification executed
### Commands run

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 -m dev.harness_lab improve --proposer manual --dry-run
python3 -m dev.harness_lab candidate-worktree --help
python3 -m dev.harness_lab qualify --help
opencode --version
python3 -m dev.harness_lab doctor --binary target/debug/codegg
scripts/verify.sh quick
```

### Results

Python suite passed 12 tests, including bounded retry transition and checkpoint reload coverage. Manual dry-run passed with zero provider calls. Candidate-worktree and qualification command help and doctor passed; OpenCode reported version 1.19.0. Quick verification passed locally. The forbidden-scope rejection was exercised on a linked candidate worktree and recorded `REJECTED`. No live OpenCode proposal or authenticated provider request was run. No hosted CI run is claimed.

## 5. Invariant review

Candidate worktrees must be linked, named, off main, and at the frozen baseline SHA. Candidate patches are scope-checked and hashed including untracked files. Baseline manifests and append-only index prefix must remain unchanged. Acceptance requires correctness improvement and paired non-regression; time/cost are not substituted for correctness because those metrics are unavailable. A candidate that improves nothing measurable, or that improves only an advisory criterion, is rejected rather than accepted on a millisecond-scale artifact. No automatic merge or push exists.

## 6. Failure and recovery review

OpenCode process groups are bounded and reaped on timeout; malformed or permission-related events fail closed. Checkpoint state and immutable inputs are validated on resume. If candidate evaluation has already appended partial trial rows, the controller refuses to silently retry under the same candidate ID. Compile corrections consume a bounded retry and proposer attempt and rerun the build gates. Interruption/resume after partial candidate trials remains fail-closed and is listed below.

## 7. Migration and compatibility review

No Rust production dependency, database migration, or forced OpenCode installation. Manual mode remains available. The pinned CLI version is optional and absence does not affect the manual path.

## 8. Security review

OpenCode is constrained by a candidate worktree, changed-path allowlist, candidate-local changelog, bounded process lifetime and separate build output. The candidate runs on a trusted workstation; no VM/container boundary is claimed. Provider secrets are not written to receipts. Explicit permission/Ask events are not automatically approved.

## 9. Documentation and operations

The harness README documents proposer modes, candidate worktrees, gates, checkpoints and limitations. The source plan and roadmap retain the conditional operational gates.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The **live** compile-correction trace was missing | The OpenCode proposer and a legitimate accepted outcome had live evidence; the correction *loop* had only its labelled fault-injection evidence | **Discharged 2026-10-22** — §3.1. An authenticated proposer was handed a real compiler diagnostic, corrected the candidate, and the corrected build was re-gated and accepted. The trace also exposed the gate-order defect, which is fixed and tested. |
| low | The installed OpenCode version was checked but no authenticated invocation was made | Version-pinned live behavior was supported only by fake executable tests | **Discharged 2026-10-22** — the live proposal and the live correction both ran against the installed `opencode` 1.19.3, and the version gate is a distinct nameable refusal that is not reported as a transport fault. |
| low | The qualification path has no cumulative deadline across a resume | A resumed qualification restarts `--max-wallclock` per invocation; the resume is instead bound to the base revision, suite digest, patch digest and binary fingerprint | Carry the optimization checkpoint's `deadline_unix` into the qualification path if a long qualification must not outlive its original budget. |
| low | The harness still has a load-dependent flake | A live tool trial can lose its ACP tool notifications under heavy load and end as `infrastructure_error`; it is treated as unmeasured and re-run, but the loss itself is unfixed | Drain the notification stream under load and make the trial's observation atomic. It bit the live compile-correction trace once: the pairing was incomplete, the comparison refused to score, and the resume re-ran only the missing pair. |

## 11. Roadmap disposition

Milestone closed. The hard M003 dependency is discharged, and every evidence
condition M004 names is now in the record: an accepted candidate trace, a
rejected candidate trace, an interrupted qualification resumed without a
duplicate receipt, and a live compile-correction trace. The two low findings
that remain are not conditions of closure: the missing cumulative deadline
across a resume is a known limitation stated in the C003 closure, and the
load-dependent notification flake is a harness defect tracked here and in the
C003 closure. No later harness-lab milestone is registered, so no additional
plan can be unblocked.

The accepted candidate code is **not** merged and **not** pushed: it stays in
its candidate worktree, as the plan requires.

## 12. Registry updates

M004 is removed from blocked work and recorded as conditionally closed. The roadmap status remains conditional; any future corrective plan must reference the medium findings above.
