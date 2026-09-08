# Development Verification and Release — CI Reproducibility Corrective Addendum

Status: active

This addendum reopens only the correctness/evidence boundary of the closed Development Verification and Release workstream. It does not reopen the earlier CI-expansion debates or authorize new verification infrastructure.

Source roadmap and predecessor evidence:

- `plans/subsystems/development-verification-release-roadmap.md`
- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md`
- `plans/closure/development-verification-release/009-status.md`

Long-term references:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs: none.

## 1. Trigger

Repository baseline `15632a0483a8c4b9d573ff2ce43297b29be8f42a` has contradictory verification evidence:

- the commit message records `cargo test --workspace` as passing locally;
- GitHub Actions run `34161421807` for that exact SHA failed in the single `CI / verify` job;
- generated-agent, core-boundary, sandbox, execution-ownership, formatting, and workspace-Clippy steps passed;
- the `Workspace tests` step, which runs `cargo test --workspace --locked -- --test-threads=1`, failed;
- the preceding mainline run at `f05fbfcd7f151bb500700f63ede873fd72c99b82` also failed in hosted CI.

The repository therefore cannot currently claim that the intentionally minimal local/hosted verification contract is reproducible.

## 2. Ownership boundary

This corrective work owns only:

- reproduction and diagnosis of the exact hosted workspace-test failure;
- correcting the production or test defect that causes local/hosted disagreement;
- making any genuinely required test environment precondition explicit and deterministic;
- proving the existing single-job CI contract green on the exact implementation candidate;
- documenting the required mainline gate as an operator action when repository administration permits it.

It does not own product feature redesign, broader test-suite restructuring, release automation, retry infrastructure, additional CI matrices, coverage gates, audit scanners, or scheduled verification.

## 3. Invariants

- The ordinary hosted path remains one bounded Ubuntu job.
- `cargo test --workspace --locked -- --test-threads=1` remains a real failure gate; the corrective pass MUST NOT mask, ignore, retry-until-green, or delete a failing test merely to restore status.
- A platform-specific assumption must be encoded as an explicit fixture/precondition or corrected in the implementation, not hidden behind conditional test skipping unless the capability is genuinely unsupported on the platform.
- Static ownership/sandbox guards and strict Clippy remain intact.
- Hosted CI remains non-authoritative for release cadence and publication.
- No new required CI lane is introduced.

## 4. Corrective milestone

### M010 — Hosted workspace-test reproducibility and mainline gate

Class: invariant / polish corrective work.

Dependency state: ready. The prior DVR workstream is closed and the exact failing hosted candidate is known.

Deliverable boundary:

1. identify the exact test(s) or process failure in run `34161421807` from the Actions log or an equivalent faithful Ubuntu reproduction;
2. reproduce the failure with the narrowest command that still demonstrates it;
3. classify the cause as production correctness, test isolation/order, process-global/environment state, filesystem/platform assumption, timeout/resource behavior, or dependency/lockfile behavior;
4. fix the root cause without weakening the tested contract;
5. add a focused regression test or static guard when the defect class can recur;
6. run the exact hosted workspace-test command and the repository's normal bounded verification commands on the final candidate;
7. obtain a green existing `CI / verify` run for that exact candidate;
8. verify whether `main` is protected by a required `CI / verify` status check. If repository administration is unavailable to the implementation agent, record the required operator action rather than changing workflow policy to compensate.

## 5. Explicit non-goals

- Adding test retries, flaky-test plugins, rerun wrappers, or `continue-on-error`.
- Reintroducing the previously removed multi-job CI matrix.
- Adding Windows/macOS CI to diagnose an Ubuntu-only failure.
- Expanding `scripts/verify.sh` into another orchestration framework.
- Changing release cadence or publication ownership.
- Treating branch protection configuration as production code.

## 6. Closure authority

M010 may close only when the closure record contains:

- the exact failing test/process evidence from the original or reproduced hosted failure;
- a root-cause classification;
- the implementation/regression evidence that fixes it;
- exact commands and outcomes for focused reproduction and `cargo test --workspace --locked -- --test-threads=1`;
- formatting and workspace-Clippy outcomes;
- one green existing hosted `CI / verify` run tied to the exact implementation SHA;
- branch-protection status if observable, or a named operator action if repository administration is unavailable.

If the failure cannot be reproduced or the hosted log cannot identify the failing test, M010 remains blocked/conditionally closed; it must not be declared complete on a speculative code change.

## 7. Deferred work preserved

This addendum does not activate release automation, fixed release cadence, automatic dependency updates, additional support-tier CI, package signing, SBOM/provenance generation, or general CI expansion.
