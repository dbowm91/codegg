# C004 — C001 Remaining Evidence, Filter Controls and Review Pack

Status: **closed** — closure `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`
Source: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`; C001 closure `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md` (§10.1–§10.5)
Baseline: `main` `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`
Closure target: `plans/closure/linux-sandbox-admission-follow-up-corrective/004-status.md`
Class: infrastructure (evidence and review tooling; no production admission)

## 1. Objective

Close the four findings C001's closure review left open, so that C001 can be
closed rather than conditionally held:

1. the complete test-fixture isolation audit for the named Git-routing and
   security-review tests;
2. the LSP load-sensitivity characterization;
3. execution controls that prove a security test filter actually ran the
   tests it names;
4. the per-boundary review pack.

## 2. Why this milestone is ready

C001's integration and evidence objective is delivered and merged. The
remaining work is bounded evidence and tooling work with no production
behaviour to design, no ADR to settle, and no host admission involved.

## 3. Current implementation evidence

- Two fixed-path Git fixtures were made process-unique in the C001
  corrective pass. The rest of the named test set was not audited.
- The three live-language-server tests sit behind `lsp-real-server-tests`,
  keep their functional assertions, and keep their 120-second bound.
- No filter-execution control exists.
- The merge baseline and CI qualification matrix exist in
  `plans/closure/branch-and-ci-stabilization/001-status.md`.

## 4. Invariants that must not regress

- No Git subcommand authorization may be widened to make a test pass.
- No timeout may be lowered, and no test may be disabled or silently
  excluded to obtain a green run.
- A filter that matches zero tests is not a pass, and a self-skip marker is
  not a skip-free run.
- A closure may not claim more than the evidence supports.

## 5. Scope

**In:** the four items above, plus the branch-wide hosted evidence for the
corrective head.

**Out:** any Linux security profile or default decision, any M003/C010
admission, any ADR acceptance, any Git semantic change, any reopening of
M004 or of the closed C023, and any change to unrelated runtime timeouts.

## 6. Ordered work packages

1. **Fixture isolation audit.** For each named Git-routing and
   security-review test, either own a temp Git worktree or bind a scoped
   repository cwd. Record the per-test disposition. Reproduce with two
   concurrent processes running the same fixture and show they do not
   interfere.
2. **LSP characterization.** Add per-test service/workspace fixtures for the
   three live-language-server tests and record the elapsed phase under
   contention. If the cause stays unknown, record **conditional** — do not
   lower the timeout and do not widen the exclusion.
3. **Filter execution controls.** For each security filter: assert the
   executed count is non-zero, assert no self-skip marker was emitted, and
   provide a positive and a negative control. A control that cannot fail is
   not a control.
4. **Review pack.** Produce a per-boundary changed-file/commit ledger, the
   exact `main` merge baseline, the migration and ADR integrity result, the
   CI qualification matrix, and a proposed review chunking.
5. **Hosted evidence.** Run the branch-wide hosted sweep on the corrective
   head and record the run link. Await the native and macOS jobs only for
   the boundaries the ledger marks as needing them.

## 7. Required verification

`git diff --check`; `cargo fmt --all -- --check`;
`cargo check --workspace --all-targets --locked`; `scripts/verify.sh quick`;
the named static guards; the named focused test targets; and the branch-wide
hosted sweep. Capture executed counts for every filter, and do not infer
platform behaviour that was not run.

## 8. Acceptance criteria

- Every named Git-routing and security-review test has a recorded fixture
  disposition, and two concurrent processes running the same fixture do not
  interfere.
- The three LSP tests have a recorded characterization, and either a
  diagnosed cause or an explicit conditional with the exact future evidence.
- Every security filter proves a non-zero executed count, an absent
  self-skip marker, and a positive/negative control pair.
- The review pack exists and names a per-boundary owner.
- The branch-wide hosted sweep on the corrective head is recorded with its
  run link and executed count.

## 9. Stop conditions

Stop and report rather than improvise if the fixture audit would require
widening Git authorization, if the LSP characterization would require
lowering a timeout or widening an exclusion, if a filter control cannot be
made to fail on an injected defect, or if the review pack would require
reopening a closed milestone.

## 10. Closure evidence required

A requirement-to-evidence matrix, the per-test fixture disposition table,
the concurrency reproduction, the LSP characterization, the filter-control
results with their non-vacuity proofs, the review pack, the hosted run
links with executed counts, and an explicit list of anything still open with
its owner.
