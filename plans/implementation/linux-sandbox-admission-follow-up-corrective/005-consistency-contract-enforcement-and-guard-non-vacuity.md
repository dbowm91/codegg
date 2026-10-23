# Linux Sandbox Admission C005 — Git Consistency Contract Enforcement and Guard Non-Vacuity

Status: ready for handoff
Repository baseline: `f1b83a7a7` (`origin/main`, 2026-10-23)
Source roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
Long-term requirements: `plans/000-long-term-specification.md` §§9, 16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0044 (contained Git live objects and metadata authority, **proposed — NOT accepted**), ADR-0048 (contained Git execution for managed worktree creation, accepted)
Primary class: invariant

## 1. Objective

Make one claim true end to end:

> **The typed Git read-consistency contract is enforced at every consumer that reads a diff, and the guard that is supposed to protect the hostile-config mediation actually detects the mediation's removal.**

This milestone closes two defects found in the 2026-10-23 implementation review of the blackboard line. One is a major: the guard that checks the hostile-Git-metadata authority **never asserts that the detector is called**, so the entire C003 mediation can be deleted while the guard, its own `--self-test`, `verify.sh quick` and CI all stay green. The other is a minor: one diff consumer discards the consistency class instead of fencing it, so the type is declared but not enforced at that consumer.

The claim is about **enforcement of a contract that already exists**. Neither defect is about what the contract says; both are about whether it is actually applied and whether the machinery that claims to protect it is real.

**This milestone does not depend on ADR-0044's acceptance.** ADR-0044 remains proposed and is not accepted. C003's conditional closure rests on the maintainer's ADR-0044 decision, and that condition is unchanged by this milestone.

## 2. Why this milestone is ready

- **C003 landed the typed consistency contract, the consumer fence and the hostile-config mediation.** `plans/closure/linux-sandbox-admission-follow-up-corrective/003-status.md`; implementation `f00d10e0d`.
- **The defect was found by mutation-testing the guard, not by reading it.** Deleting the detector's two call sites produced **zero** findings from the guard's own checks, with the unmodified tree also producing zero. The guard's non-vacuity claim in its own docstring is therefore not supported.
- **The guard script already exists and has a `--self-test` harness.** The fix is to add the missing obligation and the matching injection, not to build new machinery.
- **The consistency consumer shape is already established.** `src/git_service.rs:804` and `src/tool/commit.rs:189` already call `admit_generation_bound`; the third consumer is the one that does not.

## 3. Current implementation evidence

### 3.1 The guard does not assert the detector's call site (major)

C003's mediation is the refusal of a repository that declares a hostile Git configuration — a `filter.<n>.process|clean|smudge`, a `diff.<n>.command`, and similar. It is implemented by `ensure_repository_config_surface_is_contained`, called from exactly two places:

```rust
let worktree = canonical_worktree(cwd)?;
ensure_repository_config_surface_is_contained(&worktree)?;
```

(`crates/egggit/src/process.rs:428` in `GitEnvPolicy::apply`, and `:453` in `apply_sync`.)

The guard that is supposed to protect this is `check_hostile_git_metadata_authority` (`scripts/check_git_forbidden_patterns.py:813`). Its docstring states that it asserts three things about the trusted read route, and it inspects four functions:

```python
overrides = _function_body(text, "add_trusted_git_overrides")
environment = _function_body(text, "apply_environment")
identity = _function_body(text, "apply_commit_identity")
classification = _function_body(text, "classify_git_request")
```

(`scripts/check_git_forbidden_patterns.py:855-858`.)

`ensure_repository_config_surface_is_contained` is **not among them**, and it is not in `ROUTE_OBLIGATIONS` either — that table asserts argv and environment *patterns* (`:567-603`), none of which is the detector's call. The guard mentions `config_surface` exactly once, in a comment (`:706`).

**Evidence — mutation test.** The guard was mutation-tested at the 2026-10-23 review: `crates/egggit/src/process.rs` was copied to a temporary tree, **both** `ensure_repository_config_surface_is_contained(&worktree)?;` lines were deleted, and the guard's own two relevant checks were run against the mutated tree:

| Tree | `check_hostile_git_metadata_authority` | `check_trusted_git_execution_policy` |
|---|---|---|
| unmodified | 0 findings | 0 findings |
| both detector calls deleted | **0 findings** | **0 findings** |

The guard's 32 self-test injections do not include this case.

**Impact.** The whole C003 mediation — the refusal of a filter-bearing or diff-driver-bearing repository — can be removed, and every automated check stays green. The trusted route would then materialise content through a repository-declared smudge filter, which is precisely the capability C003 was written to deny.

**Contributing factor.** `scripts/check_git_forbidden_patterns.py` is **not** wired into `scripts/verify.sh quick` or into CI. AGENTS.md classifies it as change-triggered. So even a guard that *could* detect the removal would only run when a human remembered to run it. The two defects compound: the guard cannot see the removal, and nothing runs the guard automatically.

### 3.2 One diff consumer discards the consistency class (minor)

`src/security/workflow/report.rs:135-138`:

```rust
let summary = egggit::diff_summary(root, base)
    .await
    .map_err(|e| e.to_string())?
    .into_value();
```

and `:157-160`:

```rust
let file_diff = egggit::file_diff(root, &path, base)
    .await
    .map_err(|e| e.to_string())?
    .into_value();
```

Both call `.into_value()` directly. `admit_generation_bound` — the fence that turns a read *fact* into an admitted or refused read — is never called, so the consistency class the read carries is discarded and the hunks are consumed as if the read were generation-consistent.

The other two diff consumers do fence it: `src/git_service.rs:804` and `src/tool/commit.rs:189` both call `admit_generation_bound(fact, live_read_capability())`.

**Today's behavioural impact: none.** The trusted route yields `Captured`/`Immutable` for `diff`, so the admitted value is the same. The defect is that the type is **declared but not enforced** at this consumer — the exact gap C003 set out to close — and the guard for it does not exist.

**Why it matters beyond tidiness.** `src/security/workflow/report.rs` feeds a **security review** surface. A read that a future consistency class marks as inadmissible — for example a `NoObjectGeneration` read — would have its hunks become security-review targets without ever being fenced. The three consumers are not equivalent in consequence.

## 4. Invariants that must not regress

1. **The trusted route is the only place a CodeGG-owned Git child is constructed**, and it refuses a repository that declares a hostile configuration surface.
2. **`ensure_repository_config_surface_is_contained` is invoked on every trusted-route child construction**, in both the async and the sync form.
3. **"We could not check" is not "we checked and found nothing".** A truncated inspection is reported and refused (`crates/egggit/src/process.rs:498-540`, the `InspectionTruncated` surface maps to `GitExecutionCapability::RepositoryAlias`).
4. **The consistency class is enforced at every consumer that reads a diff**, not merely declared.
5. **`admit_generation_bound` refuses an inadmissible read fact** rather than downgrading it (`src/git_consistency.rs:40`).
6. **The guard's own `--self-test` proves each obligation it claims** — the guard is not evidence until it is shown to fail on an injected violation.
7. **The trusted route's existing containment is unchanged**: hostile clean/smudge filters, `core.fsmonitor`, `core.pager`, `core.sshCommand`, `core.editor`, `alias.*` and `GIT_*`/`GIT_CONFIG_*` injection are all already contained, and none of that is reopened here.

## 5. Scope

**In scope**

- The guard's coverage of the detector's call site: a new route obligation plus a `--self-test` injection that deletes the call and must be rejected.
- The enforcement of the consistency class at `src/security/workflow/report.rs`.
- Wiring `scripts/check_git_forbidden_patterns.py` into `scripts/verify.sh quick` and CI, with `AGENTS.md`'s change-triggered list updated in the same commit.
- Regression tests for both defects, and the guards in §10.

**Out of scope**

- **ADR-0044's acceptance.** It remains proposed. C003's conditional closure and its condition — the maintainer's ADR-0044 decision — are unchanged by this milestone.
- **The trusted route's override table, the env policy and the command classification.** Those were verified separately to be correct and are not touched.
- **Any change to the consistency type's vocabulary** (`crates/egggit/src/consistency.rs`). Only its *enforcement* at a consumer is in scope.
- The contained managed-worktree creation grant — the Linux trusted Git line's C009 owns it.
- The shared checkout's placement and lease lifetime, the mediation authority and the agent-loop fence — the M005 writer-containment corrective line's C007, C008 and C009 own them.

## 6. Required production changes

### 6.1 Make the guard assert the detector's call site

`scripts/check_git_forbidden_patterns.py` must gain an obligation that fails when the detector is not invoked from the trusted route.

The obligation must assert **both** call sites — `GitEnvPolicy::apply` and `GitEnvPolicy::apply_sync` — because removing either one leaves a construction path uncovered. Asserting only one would leave the mutation test half-passing.

The assertion must match the **call**, not a mention. The guard already has machinery for this distinction: it strips line comments before matching (`_strip_line_comments`), so a comment naming the detector must not satisfy the obligation. The new obligation must use the same discipline.

### 6.2 Add the matching `--self-test` injection

The guard's self-test must include a case that **deletes the detector's call** from a mutated `process.rs` and asserts that the guard rejects it. A guard obligation without a matching injection is exactly the defect §3.1 describes.

The injection must be proven to fail the guard, not merely to run. The self-test's own report must say which injection was exercised.

### 6.3 Wire the guard into `verify.sh quick` and CI

`scripts/check_git_forbidden_patterns.py` must run in `scripts/verify.sh quick` and in `.github/workflows/ci.yml`, with its `--self-test` alongside, matching the pattern the other guards in this repo already follow.

`AGENTS.md` documents this script as change-triggered. That statement must be updated in the same commit, and the update must record *why* — that a guard which is not run automatically cannot protect anything.

**This is the change most likely to need the maintainer's agreement**, because it alters a stated repo policy and adds CI time. It is nonetheless the difference between a guard that exists and a guard that works.

### 6.4 Fence the consistency class at the security-review diff consumer

`src/security/workflow/report.rs` must route both reads through the same fence the other two consumers use:

```rust
let admitted = crate::git_consistency::admit_generation_bound(fact, live_read_capability())?;
```

The two reads are `egggit::diff_summary` and `egggit::file_diff`. Both must be admitted before their values are consumed.

The refusal must be a typed error the caller handles, not a panic and not a silent fallback to an unfenced read. The security-review surface's callers must see an admissible read or an error.

## 7. Ordered work packages

### WP1 — The detector call-site obligation

**Intent:** the guard detects the mediation's removal.

**Required changes:** §6.1.

**Acceptance evidence:** deleting either or both detector call sites from a mutated `process.rs` produces a finding from the guard.

### WP2 — The self-test injection

**Intent:** the new obligation is proven non-vacuous.

**Required changes:** §6.2.

**Acceptance evidence:** the guard's `--self-test` includes the detector-call deletion and reports it as rejected; the guard fails when the obligation is removed.

### WP3 — Wire the guard into `verify.sh quick` and CI

**Intent:** the guard runs automatically.

**Required changes:** §6.3, including the `AGENTS.md` update in the same commit.

**Acceptance evidence:** `verify.sh quick` and the CI `verify` job both invoke the guard and its `--self-test`; `AGENTS.md` no longer classifies the script as change-triggered.

### WP4 — Fence the security-review diff consumer

**Intent:** the consistency class is enforced at every diff consumer.

**Required changes:** §6.4.

**Acceptance evidence:** `src/security/workflow/report.rs` admits both reads; a read fact that `admit_generation_bound` refuses produces a typed error at the security-review surface.

### WP5 — Regression suite

**Intent:** both defects are covered by something that runs automatically.

**Required changes:** §10.

**Acceptance evidence:** the tests in §10 fail against the baseline and pass after the fix.

## 8. Failure, cancellation, restart and contention semantics

- **The repository's config surface cannot be inspected.** The detector refuses (`InspectionTruncated` → `RepositoryAlias`), and the trusted route refuses the child. That behaviour is unchanged.
- **The trusted route is constructed for a repository with no Git configuration at all.** The detector finds a clean surface and the route proceeds. Unchanged.
- **The guard is run against a tree where `process.rs` has been restructured.** The obligation must fail with a *finding* (a missing route), not with an exception. A guard that crashes on a refactor is not a guard.
- **The security-review surface's read is refused.** The caller must receive the typed refusal. It must not silently produce an empty review, which would look like "no findings" rather than "the review could not be produced".
- **The daemon restarts.** No durable state changes; both fixes are code-level.

## 9. Compatibility and migration

- **No schema change and no durable-record change.**
- **§6.4 changes a failure mode of the security-review surface.** A diff read that would previously have been consumed is now refused when the consistency class is inadmissible. Today that cannot happen — the trusted route yields `Captured`/`Immutable` for `diff` — so the change is inert for current traffic and becomes load-bearing only when a stricter consistency class is introduced. The surface's callers must handle the typed error.
- **§6.3 adds CI time.** The guard is a static Python check and is fast, but the CI duration must be measured and the measurement recorded, since this repo has an active CI-throughput workstream.
- **`AGENTS.md`'s change-triggered list shrinks by one entry.** That is a documentation change in the same commit as the wiring, and it must state the reason.

## 10. Required tests

**Focused tests**

- `src/security/workflow/report.rs` — a read fact that `admit_generation_bound` refuses produces a typed error; an admissible fact is consumed unchanged.

**Guard tests**

- `scripts/check_git_forbidden_patterns.py --self-test` — the new injection deletes the detector's call from a mutated `process.rs` and must be rejected.
- The same self-test must reject a mutation that deletes only **one** of the two call sites.

**Non-regression**

- The guard's existing 32 injections must all still be rejected.
- The trusted route's existing containment evidence must be unaffected: `crates/egggit`'s own tests and the C003 regressions (`5188e227f`) remain green.
- `codegg --test contained_worktree_c008` and `codegg --test shared_checkout_c006` remain green.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
python3 scripts/check_git_forbidden_patterns.py --self-test
python3 scripts/check_git_forbidden_patterns.py
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test contained_worktree_c008
```

If the guard is wired into `verify.sh quick`, its `--self-test` invocation must be present there too, matching the pattern at `scripts/verify.sh:129-133`.

**Never** run `--all-features` for a workspace sweep.

## 12. Documentation updates

- `scripts/check_git_forbidden_patterns.py` — the docstring must state the detector-call obligation it now asserts.
- `AGENTS.md` — the change-triggered guard list is updated, with the reason recorded.
- `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` — the milestone table gains C005; the status line records it.
- `architecture/` — whichever document describes the trusted Git route's hostile-metadata refusal must name the detector's invocation points. The architecture documents are authoritative where `docs/` differs.
- `plans/closure/linux-sandbox-admission-follow-up-corrective/003-status.md` — **read-only**. C003's closure recorded the mediation as delivered; the correction that it is unguarded belongs in this milestone's closure record, not by editing C003's record.

## 13. Acceptance criteria

1. Deleting either or both `ensure_repository_config_surface_is_contained(&worktree)?;` call sites from `crates/egggit/src/process.rs` produces a finding from `scripts/check_git_forbidden_patterns.py`.
2. The guard's `--self-test` includes that injection and reports it as rejected.
3. `scripts/check_git_forbidden_patterns.py` runs in `scripts/verify.sh quick` and in CI, with its `--self-test`; `AGENTS.md` records the change and its reason.
4. `src/security/workflow/report.rs` admits both of its diff reads through `admit_generation_bound`, and an inadmissible fact produces a typed error rather than an unfenced read.
5. Every defect above has a regression test that fails against the baseline and passes after the fix.
6. The trusted route's existing containment is unchanged: hostile filters, `core.fsmonitor`, `core.pager`, `core.sshCommand`, `core.editor`, `alias.*` and `GIT_*` injection all remain refused.
7. `verify.sh quick`, `cargo clippy --workspace --all-targets -- -D warnings` and the named test targets are green.

## 14. Stop conditions

Report rather than improvise if:

- **§6.3's wiring into `verify.sh quick` would exceed a material CI-time budget.** Measure first and report the measurement; the maintainer decides whether to wire it into CI, into quick, or into a narrower change-triggered path. Do **not** ship a guard that is still only run by hand — report instead of quietly dropping the obligation.
- **The guard cannot locate the detector's call sites after a refactor of `crates/egggit/src/process.rs`.** The obligation must be updated to the new shape; it must not be weakened to a mention-based match.
- **§6.4's refusal would propagate an unhandled error type to a caller.** Report the caller set before changing the error shape.
- **The consistency vocabulary needs a new class** to express what the security-review consumer requires. Report it; adding a class is a contract change, not a wiring change.
- **Any fix would weaken an invariant in §4**, in particular invariant 3 or invariant 6.

## 15. Closure evidence required

1. A closure record at `plans/closure/linux-sandbox-admission-follow-up-corrective/005-status.md`, with the sixteen-section structure the closure template requires.
2. A requirement-to-evidence matrix mapping each of §13's seven criteria to the test or command that demonstrates it.
3. **The mutation test, re-run against the fixed tree and shown to be rejected** — the same mutation that produced zero findings at the baseline must now produce a finding.
4. The exact verification commands and their results, labelled honestly as local or hosted, including the measured CI-time impact of §6.3.
5. An invariant review confirming §4's seven invariants are intact.
6. A registry update in the same commit as the closure.
7. A statement that ADR-0044 remains proposed and that C003's conditional closure condition is unchanged by this milestone.

## 16. Handoff notes

- The two defects were found by a 2026-10-23 review of `origin/main` at `f1b83a7a7`. The guard defect was found by **mutation-testing the guard**, not by reading it — which is the technique to reuse for any guard in this repo.
- The tree at `f1b83a7a7` is **green**: `codegg --test contained_worktree_c008` (5), `codegg --test shared_checkout_c006` (16), `codegg --test composed_two_agent_race_m011` (3), `codegg --lib tool::writer_fence` (3) and `codegg-core --test writer_permit` (21) all pass. These defects are latent, not caught.
- This milestone landed alongside the C003 work (PR #169), so the guard and the mediation are new. The vacuity is not a regression from an older state; it is a gap in the new guard as written.
- The Linux trusted Git line's C009 owns the contained worktree grant. The two milestones are independent and must not be folded together, even though both touch the Git route.
- ADR-0044 remains **proposed**. C003's conditional closure and its condition are unchanged.
