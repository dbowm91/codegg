# Development Verification and Release Milestone 010 — Hosted Test Reproducibility and Mainline Gate

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md#4-corrective-milestone`

Predecessor roadmap and closure evidence:

- `plans/subsystems/development-verification-release-roadmap.md`
- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md`
- `plans/closure/development-verification-release/009-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- None.

Primary class: invariant / polish corrective work

## 1. Objective

Restore trustworthy equivalence between CodeGG's intentionally minimal local and hosted verification paths by identifying and fixing the exact workspace-test failure affecting current `main`, then prove the existing single-job GitHub Actions workflow green on the exact corrective candidate.

This milestone is complete only when the failure is explained and prevented from recurring. A rerun that happens to pass is evidence to investigate, not closure by itself.

## 2. Why this milestone is ready

The predecessor verification/release subsystem is closed and its intended policy is stable: one bounded routine CI job, manual release ownership, and no verification-framework expansion.

At the repository baseline, GitHub Actions run `34161421807` for exact head `15632a0483a8c4b9d573ff2ce43297b29be8f42a` provides a concrete failing execution. In that run:

- agent TOML schema validation passed;
- codegg-core boundary guard passed;
- sandbox contract guard passed;
- execution ownership guard passed;
- formatting passed;
- workspace Clippy passed;
- `Workspace tests` failed.

The failing step executes exactly:

```bash
cargo test --workspace --locked -- --test-threads=1
```

The baseline commit message, by contrast, records `cargo test --workspace` as passing locally. This is sufficient evidence of a reproducibility defect and gives the implementer a bounded starting point.

No unresolved architecture decision is required to begin diagnosis.

## 3. Current implementation evidence

### Hosted workflow

`.github/workflows/ci.yml` contains one `ubuntu-latest` `verify` job with `CARGO_BUILD_JOBS=1`. It configures a deterministic Git identity, runs the existing source/ownership guards, then formatting, workspace Clippy, and serial locked workspace tests. It has no publication authority and produces no release artifact.

### Failing candidate

- exact candidate: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`;
- hosted run: `34161421807`;
- failed job: `101863878469`;
- failed step: `Workspace tests`;
- preceding candidate `f05fbfcd7f151bb500700f63ede873fd72c99b82` also has a failed hosted CI run, so the problem may predate the latest bug-fix commit.

### Important uncertainty

The planning pass could retrieve job/step metadata but not the raw failing test log through the available GitHub integration. The implementation agent MUST inspect the Actions log directly or reproduce faithfully on Ubuntu before editing code. The plan intentionally does not speculate about which test failed.

### Repository-administration boundary

Branch-protection state could not be read through the planning integration because the GitHub App lacks the required administration permission. Branch protection is therefore an operator evidence item, not an assumed repository fact.

## 4. Invariants that must not regress

- Routine hosted CI remains one bounded Ubuntu verification job.
- The workspace test step remains a hard failure gate.
- No `continue-on-error`, blanket ignore, broad `#[ignore]`, flaky retry loop, or rerun-until-green mechanism may hide the defect.
- A production correctness failure must be fixed in production code, not weakened in its test.
- A test-isolation failure must be fixed at ownership/fixture/state boundaries, not serialized more broadly than necessary unless serialization is itself the correct contract.
- Platform-dependent behavior must be explicit and contractually justified.
- Existing source, sandbox, core-boundary, and execution-ownership guards remain enabled.
- Hosted CI does not publish crates, create releases, choose versions, or acquire release credentials.
- Verification must remain proportionate to this repository; no new CI lane or verification framework is introduced.

## 5. Scope

### In scope

- Recover the exact failure output from run `34161421807` or reproduce the same failure under an equivalent clean Ubuntu environment.
- Narrow the failure to the smallest test command that still demonstrates the defect.
- Determine whether the root cause is production behavior, test isolation/order, process-global/environment state, filesystem/platform assumptions, timeout/resource behavior, spawned-process cleanup, or lockfile/dependency/environment mismatch.
- Correct the root cause.
- Add focused regression evidence proportional to the defect class.
- Re-run the exact hosted test command locally/under Ubuntu where possible.
- Obtain one green existing `CI / verify` run on the exact implementation SHA.
- Check whether `main` requires `CI / verify` before merge when repository administration is available; otherwise document the exact operator action needed.

### Explicitly out of scope

- Reintroducing multi-lane CI.
- New OS matrices.
- Test retry tooling.
- Coverage or benchmark gates.
- Dependency/security bot installation.
- Release automation or fixed release cadence.
- Refactoring unrelated failing subsystems merely because the broad suite traverses them.
- Solving the historical supported-Linux Landlock evidence condition owned by the runtime-safety workstream.

## 6. Required production changes

The exact production edit is deliberately evidence-dependent. The agent MUST diagnose before changing code.

### Core/domain

If the failing test exposes incorrect runtime behavior, change the canonical owner of that behavior only. Do not add a compatibility bypass in the test harness.

If the failure is caused by mutable process-global state, leaking environment mutation, shared temporary paths, or other cross-test coupling, make the smallest ownership change required to make the test deterministic. If the needed change overlaps the newly registered post-audit runtime-context milestone, fix only the concrete blocker here and record broader cleanup for that milestone rather than absorbing it.

### Storage and migrations

No production migration is expected. If a database test reveals ordering, transaction, or cleanup defects, preserve existing schema compatibility and limit changes to the owning transaction/test-fixture semantics.

### Protocol and DTOs

No protocol change is expected. A protocol change requires stopping and registering separate work unless the defect is a direct backward-compatible serialization bug with obvious ownership.

### Runtime and concurrency

Inspect for:

- leaked Tokio tasks or child processes;
- tests depending on ambient environment variables;
- process-global mutable registries or caches;
- temporary path collisions;
- order-dependent singleton initialization;
- wall-clock assumptions;
- port/socket collisions;
- resource starvation under `CARGO_BUILD_JOBS=1` and serial tests.

Fix the actual lifetime/isolation contract rather than increasing timeouts by default.

### Frontend or operator surface

No user-facing UI change is required unless the defect is itself in a user-facing contract under test.

Repository branch protection is an operator setting. If the implementation agent cannot inspect/change it, closure must record: `operator action required: configure main to require the existing CI / verify status check` rather than inventing a workflow workaround.

### Security and authorization

Do not weaken fail-closed behavior, sandbox tests, path canonicalization, credential isolation, or permission tests to make hosted execution pass.

### Documentation and static guards

Update testing/verification documentation only if diagnosis proves that a documented precondition or command is wrong. Add a static guard only if the root cause is a durable source-level ownership invariant that a simple focused test cannot cover more directly.

## 7. Ordered work packages

### Work package A — Recover and classify the failure

Intent:

Establish facts before editing.

Required changes/actions:

1. Open the raw log for run `34161421807`, job `101863878469`, and capture the first causal test/process failure plus relevant preceding output.
2. If raw hosted logs are unavailable, reproduce using a clean Ubuntu environment with the exact workflow prerequisites and command.
3. Re-run only the failing package/test where possible.
4. Determine whether the failure is deterministic. A nondeterministic result still requires an ownership/lifetime hypothesis supported by evidence.
5. Record a root-cause category and the earliest bad invariant.

Acceptance evidence:

- exact failing test/process name and message;
- narrow reproduction command;
- root-cause statement tied to code/test ownership.

### Work package B — Correct the root cause

Intent:

Make local and hosted execution obey the same deterministic contract.

Required changes:

- implement the narrowest production/test-fixture correction that addresses the diagnosed cause;
- preserve all existing security and execution ownership boundaries;
- avoid broad timeout increases or additional serial locks unless the resource truly is process-global by contract.

Acceptance evidence:

- focused regression fails before or demonstrably covers the defect class and passes after;
- narrow reproduction command passes repeatedly enough to demonstrate determinism without adding retry semantics to CI.

### Work package C — Reconcile verification evidence

Intent:

Prove the corrective candidate under the repository's actual verification contract.

Required actions:

Run the focused tests, then the exact hosted workspace test command, formatting, linting, and the existing quick verification path as appropriate.

Acceptance evidence:

- commands and exit status recorded verbatim in closure;
- no skipped failing test or ignored failure introduced.

### Work package D — Exact-head hosted closure and branch gate disposition

Intent:

Close the local/hosted discrepancy rather than assuming it.

Required actions:

1. Push the corrective candidate.
2. Record one green existing `CI / verify` run whose `head_sha` is exactly the implementation candidate.
3. Inspect main-branch protection/ruleset if authorized.
4. If the required status gate is absent and administration is authorized, configure the existing `CI / verify` check as required without adding workflows.
5. If administration is not authorized, record the operator action precisely and leave code closure separate from administrative completion.

Acceptance evidence:

- exact SHA and hosted run/job IDs;
- branch-gate evidence or explicit operator-action disposition.

## 8. Failure, cancellation, restart, and contention semantics

A test process interrupted during diagnosis is not evidence. Re-run the narrow reproduction from a clean state.

If a test leaves child processes, sockets, files, environment variables, global registries, or database locks behind, cleanup must occur on both success and failure paths. Panic/unwind behavior must not poison subsequent tests unless the owning API explicitly documents that behavior.

If the failure is timing-sensitive, favor deterministic event/state synchronization over longer sleeps. If a true external timeout exists, establish the bounded contract and test it directly.

Concurrent callers are out of scope unless concurrency is the demonstrated cause; in that case the fix must preserve production contention semantics, not only serial-test behavior.

## 9. Compatibility and migration

No public compatibility break is expected. Any proposed removal or renaming of a public tool/config/protocol item belongs to the post-audit compatibility milestone, not this corrective pass.

Changing test-only fixtures has no migration requirement. Changing persisted data or protocol shape requires stopping and splitting work unless explicitly demonstrated as a backwards-compatible bug fix.

## 10. Required tests

### Focused unit tests

- The specific failing behavior with deterministic fixture/state ownership.
- Poison/error/cleanup path if relevant to the root cause.

### Integration tests

- The exact failing integration test/package where applicable.
- Any local-vs-clean-environment difference demonstrated during diagnosis.

### Restart and recovery tests

Required only if the failure is caused by daemon/process lifecycle or durable state reopening.

### Contention and cancellation tests

Required only if the failure is caused by task/process contention, cancellation, leaked resources, or process-global mutation.

### Security and negative tests

Preserve all existing sandbox, canonicalization, permission, credential, and ownership tests. Add a negative regression when the defect was fail-open or isolation-related.

### Migration and compatibility tests

Not expected unless diagnosis proves a persisted compatibility issue.

## 11. Required verification commands

Start narrow, then broaden. The closure record MUST report what actually ran.

```bash
# exact narrow reproduction discovered in WP-A
cargo test -p <package> <failing-test> --locked -- --test-threads=1

# exact hosted workspace-test contract
cargo test --workspace --locked -- --test-threads=1

# repository formatting/lint posture
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# normal bounded local regression posture
scripts/verify.sh quick
```

If all-feature Clippy is materially unsupported on the implementation host, record that fact rather than substituting an unreported command. Hosted `CI / verify` on the exact candidate remains required closure evidence.

## 12. Documentation updates

- Update `architecture/testing.md`, `CONTRIBUTING.md`, or `AGENTS.md` only if the root cause changes a real prerequisite or canonical command.
- Do not add a troubleshooting essay for a one-off defect.
- Record branch-protection operator action in the closure record if it cannot be performed by the implementation agent.

## 13. Acceptance criteria

- The exact original hosted failure is identified or faithfully reproduced and causally explained.
- A focused regression protects the corrected invariant where practical.
- `cargo test --workspace --locked -- --test-threads=1` exits 0 on the final candidate in an appropriate clean environment.
- Existing formatting, Clippy, and ownership/sandbox guards are not weakened.
- Existing hosted `CI / verify` passes on the exact candidate SHA.
- No new CI lane, retry mechanism, release workflow, or verification framework is introduced.
- Mainline required-check state is either verified/configured or left as a named operator action because repository administration is unavailable.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the raw hosted failure cannot be recovered and no equivalent failure can be reproduced;
- diagnosis requires a broad subsystem redesign unrelated to the failing invariant;
- the only apparent route to green is skipping/ignoring/retrying the failing test;
- a public protocol/schema migration is required;
- branch protection requires privileges the agent does not possess;
- the failure is the already-known Landlock host-evidence condition owned by another registered workstream;
- repository head changed materially enough that `15632a0` no longer represents the failing contract and the new head must be re-audited.

## 15. Closure evidence required

The later closure record MUST contain:

- implementation commit/PR;
- original run/job IDs and exact failing test/process output;
- root-cause classification;
- narrow reproduction command and outcome before/after where obtainable;
- focused regression test/guard and why it prevents recurrence;
- exact workspace-test command and outcome;
- formatting/lint/quick-verification outcomes actually run;
- exact-head hosted `CI / verify` run/job and success conclusion;
- explicit statement that no tests were ignored and no retries/continue-on-error were added;
- branch-protection/ruleset evidence or exact operator action;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

The main hazard is treating the failed status as a request to edit CI. It is not. CI already failed at the intended contract boundary. Diagnose the code/test/runtime difference first.

Tests are intentionally serial in hosted CI. Do not introduce broader test serialization before proving shared-state ownership requires it.

Preserve unrelated user changes. If another implementation plan lands first and changes the failing test area, rebase the evidence against the new head while retaining the original failing run as historical trigger evidence.
