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

M004 is conditionally closed. Manual and optional OpenCode proposal paths, version-pinned event parsing, candidate-only worktrees, immutable baseline checks, separate build targets, changelog/scope gates, paired ACP qualification, and fail-closed acceptance are implemented. The controlled candidate loop has no accepted trace because the frozen baseline has no known correctness defect, and the optional authenticated OpenCode pilot was not run. The controller therefore rejects candidates without a verified correctness improvement; it does not fabricate acceptance evidence.

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
| Bounded compile-correction retry and checkpointed attempt identity | `qualify_candidate`; retry transitions are covered by checkpoint resume test | pass | OpenCode-only retries rerun candidate build gates; a live correction attempt was not run. |
| Full interrupted qualification resume and accepted candidate trace | M004 work package D | partial | Pre-evaluation candidate checkpoints resume after immutable-input checks. Partial candidate ledger rows fail closed and require a fresh candidate ID. No baseline correctness defect exists to support a legitimate accepted trace. |
| Authenticated live OpenCode one-cycle pilot | optional manual pilot | not run | No provider call was requested or made. |

## 3. Production implementation evidence

`dev/harness_lab/improve.py` provides checkpointed state transitions and budget checks, ManualProposer and an OpenCode CLI adapter pinned to 1.19.0, local candidate branch/worktree creation, patch hashing including untracked files, path/changelog validation, separate Cargo targets, focused build/test gates, bounded compile-correction retries with new attempt identities, full ACP evaluation, paired comparison, and final acceptance only on measured primary correctness improvement without regression. It never integrates the candidate into the default worktree or pushes a branch. Synthetic manual and out-of-scope rejection paths have receipts.

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

Candidate worktrees must be linked, named, off main, and at the frozen baseline SHA. Candidate patches are scope-checked and hashed including untracked files. Baseline manifests and append-only index prefix must remain unchanged. Acceptance requires correctness improvement and paired non-regression; time/cost are not substituted for correctness because those metrics are unavailable. No automatic merge or push exists.

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
| medium | No end-to-end interrupted qualification resume across candidate trials | The controller resumes immutable candidates before scoring; once candidate rows exist it fails closed and requires a new candidate ID | Before treating the improvement loop as fully restart-qualified, run a controlled interruption and reconcile all partial trial identities without hiding attempts. |
| medium | No accepted candidate trace or live compile-correction trace | OpenCode proposer and a legitimate accepted outcome lack live evidence | In a separately budgeted session, use an independently verified correctness fix, induce one compile correction, and preserve candidate/build/ACP receipts. |
| low | The installed OpenCode version was checked but no authenticated invocation was made | Version-pinned live behavior is supported only by fake executable tests | Keep OpenCode optional; run the plan's manual opt-in pilot before relying on it for autonomous proposals. |

## 11. Roadmap disposition

Milestone conditionally closed with named operational evidence outstanding. The hard M003 dependency is discharged. No later harness-lab milestone is registered, so no additional plan can be unblocked. M004 must not be represented as a fully qualified autonomous improvement loop until the medium findings are discharged with the specified receipts.

## 12. Registry updates

M004 is removed from blocked work and recorded as conditionally closed. The roadmap status remains conditional; any future corrective plan must reference the medium findings above.
