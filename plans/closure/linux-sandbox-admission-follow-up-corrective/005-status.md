# Linux Sandbox Admission Follow-Up Corrective C005 — Git Consistency Contract Enforcement and Guard Non-Vacuity: Status

Status: closed

Milestone: `plans/implementation/linux-sandbox-admission-follow-up-corrective/005-consistency-contract-enforcement-and-guard-non-vacuity.md`
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work was applied to

## 1. Executive finding

Two defects, on opposite sides of the same contract.

**The guard did not assert the thing it was written to protect.** `scripts/check_git_forbidden_patterns.py` inspects the trusted Git route's argv and environment. It never asserted that the route calls `ensure_repository_config_surface_is_contained` — the detector that refuses a repository declaring a hostile configuration surface. The detector's function name appeared nowhere in the guard's obligations, only in a comment. The review **mutation-tested** this: deleting both detector calls from `crates/egggit/src/process.rs` produced **0 findings**, and the unmodified tree produced **0 findings** too. A guard that cannot see the removal of the mediation it exists to protect cannot protect anything.

**One diff consumer discarded the consistency class.** `src/security/workflow/report.rs` consumed two diff reads with `.into_value()` — taking the payload without passing the read fact through `admit_generation_bound`. The other two diff consumers, `src/git_service.rs` and `src/tool/commit.rs`, fence their reads. The security-review surface was therefore reading a diff whose generation-bound consistency had not been established.

C005 fixed both: the guard now asserts the detector's **call** — in both the async and the sync path, and a comment naming the detector does not satisfy it — and the security-review surface now fences both of its reads and turns an inadmissible fact into a typed error. It also wired the guard into `scripts/verify.sh quick` and CI, because a guard that is not run automatically protects nothing.

## 2. Requirement-to-evidence matrix

| §13 criterion | Evidence | Result |
|---|---|---|
| 1. Deleting either or both detector call sites produces a finding | §4.1 — three separate mutations, three rejections | **met** |
| 2. The guard's `--self-test` includes the injection and reports it rejected | §4.2 — the self-test's thirty-third through thirty-sixth cases | **met** |
| 3. The guard runs in `verify.sh quick` and in CI with its `--self-test`; `AGENTS.md` records the change and its reason | §3.4 | **met** |
| 4. `src/security/workflow/report.rs` admits both diff reads through `admit_generation_bound`, and an inadmissible fact produces a typed error | §3.3 | **met** |
| 5. Every defect above has a regression test that fails against the baseline and passes after the fix | §4.1 (guard) and §4.3 (consumers) | **met** |
| 6. The trusted route's existing containment is unchanged | §5 | **met** |
| 7. `verify.sh quick`, workspace clippy and the named test targets are green | §4.4 | **met** — `verify.sh quick` is green across its 86 steps at the closing commit and the workspace clippy with `-D warnings` is green; the named test targets are green |

## 3. Production implementation evidence

### 3.1 The detector-call obligation

`scripts/check_git_forbidden_patterns.py` gained the rule
`hostile-git-metadata-detector-call-missing`. It asserts that **both**
`GitEnvPolicy::apply` and `GitEnvPolicy::apply_sync` call
`ensure_repository_config_surface_is_contained`.

The rule matches a **call**, not a mention: the detector name must be followed
by `(` in a **masked** method body — block comments, line comments, string
literals, raw strings and char literals are masked before the call is looked
for, using the script's `_mask_non_code` helper. A comment that names the
detector does not satisfy the obligation — which is precisely how the original
gap was invisible.

Asserting only one of the two call sites would have left the mutation
half-passing, so both are asserted and both are injected in the self-test.

### 3.2 The guard's self-test

The self-test gained four cases:

```
  ok   the trusted route stops calling the hostile-config detector in the async path (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route stops calling the hostile-config detector in the sync path (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route stops calling the hostile-config detector at all (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route only mentions the hostile-config detector in a comment (rejected by hostile-git-metadata-detector-call-missing)
```

The fourth is the case that distinguishes a call from a mention, and it is the
case that would have caught the original defect.

### 3.3 The diff consumers

`src/security/workflow/report.rs` — both diff reads now go through the fence
the other consumers use:

```rust
let admitted = crate::git_consistency::admit_generation_bound(
    fact,
    crate::git_consistency::live_read_capability(),
)?;
```

An inadmissible fact produces a typed error (`SecurityReviewReadError`'s
`Consistency` arm, via `GitServiceError::Consistency`) rather than an
unfenced payload. The caller's error set was checked: the security-review
surface's callers already handle a typed error from this module, so the
refusal reaches a caller that can act on it — no new error shape reached an
unhandled site.

### 3.4 The automatic wiring

`scripts/check_git_forbidden_patterns.py` now runs in `scripts/verify.sh`'s
`quick` path and in `.github/workflows/ci.yml`, each **with** its `--self-test`.
The reason is recorded in `AGENTS.md`: *"A guard that runs only when somebody
remembers it protects nothing."* (The sentence was rewritten when this line
reconciled with `origin/main`, which replaced the guard section; the reason is
the same.) The script was previously classified as
change-triggered; that classification is gone, and `AGENTS.md` now lists it
among the automatic guards with the reason.

Two further guards were added and wired in the same commit, for the same
reason:

- `scripts/check_git_consistency_consumers.py` — asserts that **every** diff
  consumer fences its read through `admit_generation_bound`, so no consumer can
  silently drop the class again.
- `scripts/check_contained_worktree_grant.py`, `scripts/check_writer_fence_completeness.py`,
  `scripts/check_checkout_mediation_authority.py` — the guards the other three
  corrective milestones added, wired so that they run automatically too.

## 4. Verification executed

All commands were run **locally**. No hosted lane was consulted, and none is claimed.

### 4.1 The guard is non-vacuous

Three mutations of a copy of the baseline tree, each run against
`scripts/check_git_forbidden_patterns.py`:

**Baseline, calls present:**
```
$ python3 scripts/check_git_forbidden_patterns.py
forbidden-pattern checks: PASS (0 findings)
```

**The async call deleted:**
```
$ python3 scripts/check_git_forbidden_patterns.py
forbidden-pattern checks: FAIL (1 finding(s))

  rule: hostile-git-metadata-detector-call-missing  (1 finding(s))
    crates/egggit/src/process.rs:420: the trusted Git route method apply() no longer calls ensure_repository_config_surface_is_contained(); the detector is the mediation that refuses a repository-controlled configuration surface, and a comment naming it is not a call
```

**Both calls deleted:**
```
$ python3 scripts/check_git_forbidden_patterns.py
forbidden-pattern checks: FAIL (2 finding(s))

  rule: hostile-git-metadata-detector-call-missing  (2 finding(s))
    crates/egggit/src/process.rs:420: … method apply() no longer calls …
    crates/egggit/src/process.rs:444: … method apply_sync() no longer calls …
```

Each injection produces its own finding, naming the method that lost the
call. This is the evidence the original defect lacked.

### 4.2 The guard's self-test

```
$ python3 scripts/check_git_forbidden_patterns.py --self-test
  …
  ok   the trusted route stops calling the hostile-config detector in the async path (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route stops calling the hostile-config detector in the sync path (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route stops calling the hostile-config detector at all (rejected by hostile-git-metadata-detector-call-missing)
  ok   the trusted route only mentions the hostile-config detector in a comment (rejected by hostile-git-metadata-detector-call-missing)
git-forbidden-patterns self-test passed: 36 injected defects were each rejected by the guard, and the unmodified trusted Git route produced no finding
```

The full self-test covers **36** injected defects, each rejected.

### 4.3 The consistency-consumer guard

```
$ python3 scripts/check_git_consistency_consumers.py
git-consistency-consumers check ok: every diff consumer fences its read through admit_generation_bound

$ python3 scripts/check_git_consistency_consumers.py --self-test
  ok: accepted the real tree
  ok: rejected a consumer that reads a diff fact without fencing it
  …
git-consistency-consumers self-test ok
```

### 4.4 Scoped tests and the workspace check

The tree compiles green under `cargo check --locked --workspace --all-targets`.
The scoped test targets are recorded in §9.

`scripts/verify.sh quick` and `cargo clippy --workspace --all-targets -- -D warnings`
are **green** on this tree — see §9 (U1).

### 4.5 The measured guard cost

The CI-time impact was measured, because the plan requires it:

| Command | Wall clock |
|---|---|
| `python3 scripts/check_git_forbidden_patterns.py` | **0.31 s** |
| `python3 scripts/check_git_forbidden_patterns.py --self-test` | **1.24 s** |

Both are trivial next to a workspace build. The guard's cost is not what kept
it from running automatically; its being classified as change-triggered was.

## 5. Invariant review

§4's seven invariants, reviewed against the implementation tree:

| # | Invariant | Status |
|---|---|---|
| 1 | The trusted route is the only place a CodeGG-owned Git child is constructed, and it refuses a hostile configuration surface | **intact** — no route construction changed |
| 2 | `ensure_repository_config_surface_is_contained` is invoked on every trusted-route child construction, in both the async and the sync form | **intact and now asserted** — the guard checks both call sites, and the self-test proves the assertion fails when either is removed |
| 3 | "We could not check" is not "we checked and found nothing" — a truncated inspection is reported and refused | **intact** — the `InspectionTruncated` surface is untouched |
| 4 | The consistency class is enforced at every consumer that reads a diff, not merely declared | **restored** — the security-review surface was the one consumer that did not enforce it, and it now does; the new consumer guard keeps the set closed |
| 5 | `admit_generation_bound` refuses an inadmissible read fact rather than downgrading it | **intact** — the fence is unchanged; the consumer now goes through it |
| 6 | The guard's own `--self-test` proves each obligation it claims | **met** — 36 injections, each rejected, including the four new ones |
| 7 | The trusted route's existing containment is unchanged — hostile filters, `core.fsmonitor`, `core.pager`, `core.sshCommand`, `core.editor`, `alias.*`, `GIT_*` injection | **intact** — §4.2's other thirty-two self-test cases still cover each of those, and the guard's own scan reports 0 findings on the tree |

## 6. Failure, cancellation, restart and contention semantics

No lifecycle changed. The guard is a build-time check; the diff-consumer change
is a read-path change that turns a silent un-fenced read into a typed refusal.

The consumer guard's own failure mode is worth naming: a diff consumer that
calls `.into_value()` directly on a diff fact is **rejected by the guard**, not
merely flagged. The guard scans the consumers and asserts the fenced shape, so
a future consumer that bypasses the fence fails the build.

## 7. Compatibility and migration review

No migration. No schema change. No durable payload change. No public API
change: `admit_generation_bound` and `live_read_capability` already existed and
are already public in `src/git_consistency.rs`.

The one behaviour change visible to a caller is that a security-review diff
read over an inadmissible fact now **errors** where it previously returned the
payload. That is the point of the contract, and the error is typed.

## 8. Security review

This milestone closes a **defence-in-depth gap**: the mediation that refuses a
repository-controlled configuration surface was present in the code but absent
from the guard that was supposed to protect it. A future change could have
removed either detector call and the build would have stayed green.

After this milestone, removing either call — or replacing the call with a
comment that names the detector — fails
`scripts/check_git_forbidden_patterns.py`, which runs in `verify.sh quick` and
in CI.

The diff-consumer change is a second, independent closure: the security-review
surface was reading a diff without establishing its generation-bound
consistency, and now it cannot.

## 9. Unresolved findings

> **U2 discharged on 2026-10-24.** U2 below was closed by a follow-up after
> this record was written; see §14 (A9). Its text is kept as it stood when the
> record was closed, because a reader comparing the record with the tree's
> history needs to see the finding and its discharge, not a rewritten finding.
> U1 was already closed when the record was closed.

**U1 — the whole-tree verification gap opened by the disk emergency is closed.**
The host's disk reached 100% during this milestone (the shared XDG trash had
accumulated 287 GB of already-trashed build caches; purging it was necessary to
make the tree buildable at all), and the scoped tests and the guards were
prioritised over the whole-tree gates. Both have since been run on this tree:

- `scripts/verify.sh quick` — **green**, 86 steps on the implementation tree:
  `cargo fmt --check --all`,
  every guard with its `--self-test`, and
  `cargo check --workspace --all-targets --locked`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **green**
  after four lints the new code introduced were fixed.

Severity: **none**. The finding is kept in the record because the gap was real
when the milestone was written, and a reader comparing the record with the
tree's history needs to see that it was closed by measurement rather than by
assumption.

**U2 — the diff-consumer fix has no dedicated integration test in a standalone target.** The consumer guard `scripts/check_git_consistency_consumers.py` covers the *static* contract (every consumer fences its read), and its `--self-test` injects an unfenced consumer and proves the rejection. The *runtime* half — that an inadmissible fact produces a typed error at the security-review surface — is exercised by the guard's static assertion plus the fence's own existing tests, not by a new integration test in this milestone. Severity: low. The static guard is the durable protection; the runtime path is the fence's, which is already tested where the fence lives.

## 10. Roadmap disposition

The Linux sandbox admission follow-up corrective line moves from `active planning; C001 closed, C002 conditionally closed, C003 conditionally closed, C004 closed, C005 ready` to `active planning; C001 closed, C002 conditionally closed, C003 conditionally closed, C004 closed, C005 closed`.

**This milestone does not change C003's condition.** C003's condition is the maintainer's ADR-0044 decision, and ADR-0044 remains **proposed**. C005's work is orthogonal to it: C005 made the C003 *Git consistency* contract's enforcement auditable, and did not decide the ADR-0044 question.

**C002 remains conditionally closed**, maintainer-owned: the host-admission manifest's `native_negative_proven` entry and the `maintainer_admitted` decision. C005 does not touch either.

## 11. Registry updates

`plans/registry.md` records, in this same commit:

- the Linux sandbox admission follow-up corrective row's status cell → `C005 closed`;
- the "Dependency-ready implementation plans" entry for this plan → moved out of dependency-ready, with its closure record named;
- a new closure-table row for `C005` on the Linux sandbox admission follow-up corrective line.

## 12. Documentation updates

- `AGENTS.md` — the guard's classification changed from change-triggered to automatic, with the reason recorded; the `quick` summary lists the new guards; the `CI` job's guard list updated.
- `architecture/git.md` — §"Where the detector is invoked" added: it names the two invocation points, states that the guard matches a **call** rather than a mention, and records that the self-test deletes each call in turn and proves the rejection.
- `architecture/overview.md` — the two file-count rows re-measured.

## 13. Handoff notes

The next reader should know two things.

**The guard's `--self-test` is the evidence, not the guard.** The original defect was a guard that could not see the removal of what it protected. Every obligation the guard now claims has a matching injection, and the injection count is printed: **36 injected defects, each rejected**.

**`architecture/git.md` now names the detector's invocation points.** That is the durable half of the fix: a reader who wants to know where the mediation happens does not have to grep, and the guard asserts the same thing the document says.

## 14. Amendment: implementation review

An implementation review of this milestone's change set found that the
milestone's own guard was still vacuous against one evasion form. The record
is amended rather than replaced: the closure stands, and the amendments below
are what the review changed.

**A1 — the detector-call rule was vacuous against a Rust block comment.** The
rule asserted that the trusted Git route's `apply` and `apply_sync` methods
call `ensure_repository_config_surface_is_contained(...)`. It searched the
method body with a helper that removed `//` line comments and left `/* ... */`
block comments intact, and the call pattern's lookbehind only excluded a
preceding quote — so a call that appeared **only inside a block comment**
satisfied it.

Reproduced before the fix: replacing both real call sites in
`crates/egggit/src/process.rs` with

```rust
        /* ensure_repository_config_surface_is_contained(&worktree)?; */
```

made the guard report `PASS (0 findings)` and exit 0. A string-literal
evasion was already caught, because the lookbehind excludes a preceding
quote; the block-comment form was not.

The guard now masks block comments, line comments, string literals, raw
strings and char literals before it looks for the call, using the technique
`scripts/check_git_consistency_consumers.py` already used. The shared
`_strip_line_comments` helper was **not** changed in place — it is still used
by the override-table parsing — so the override rules are unaffected.

**A2 — the self-test grew from 36 cases to 40.** The four new injections
are the block-comment evasion, a multi-line block-comment evasion, a
string-literal evasion and a raw-string-literal evasion. Each was verified
to be **vacuous against the old guard** (0 findings) and **rejected by the
new one** (1 finding each), so the self-test discriminates rather than
merely passing.

**A3 — the `verify.sh quick` step count.** The review removed a duplicated
`check_writer_fence_completeness` invocation from `scripts/verify.sh` (−2
steps) and wired `check_daemon_cwd_usage.py`, which was wired into neither
`verify.sh quick` nor CI (+2 steps). The count is unchanged at **86 steps** over a different guard set — the count
at the branch tip `8225f42dd`; the merged tree's count is in §14 A5.

**A4 — the record's quoted self-test output is milestone-scoped.** The
"36 injected defects, each rejected" line in §13 remains the milestone's own
measurement; the live count is now **40**.

**A5 — the merged tree's measurements.** The **86 steps** recorded above were
measured on the branch tip `8225f42dd`, where the quick lane's `==>` lines were
83 in `run_guards` plus 3 in `run_quick`. Reconciling with `origin/main`
`3b29c5b88` adds **nine** — `origin/main`'s four reasoning-control guards
(eight lines: each runs once with `--self-test` and once for real) and the
harness-lab unittest step (one) — so the tree merged at `d5b7efc02` runs
**95**
`==>` lines. The count is measured, not incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 91
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

That tree's quick lane was re-run after the merge: **green**, 95 steps.

This milestone's own scope is the two guards and the diff-consumer fix; the
review pass added **no** test to it, so §4.4 and §9 need no correction. The
tests the review pass added belong to the sibling M005 records — the fence
factory's unreadable-store refusal and the delegated-child probe — and are
counted there.

**A6 — the fourth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **repository audit corrective** workstream landed
at `6e097dd3a`, and the branch tip `d5b7efc02` had to be reconciled with it.
The quick lane's `==>` lines moved again, from 95 to **109** — `run_guards` is
**105** and `run_quick` is **4** — because `origin/main` added its own guards
and its own generated-asset checks. The count is measured, not incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 105
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 109
steps.

**A7 — the fifth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **file-picker surface corrective** workstream
landed at `c2d781319` (PR #182), and the branch tip `a5c00ef10` had to be
reconciled with it. The quick lane's `==>` lines moved again, from 109 to
**111** — `run_guards` is **107** and `run_quick` is **4** — because
`origin/main` added its own `/files` keymap-drift guard, which runs twice
(once with `--self-test` and once for real). The count is measured, not
incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 107
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 111
steps.

The merge's only code-level interaction was additive: this branch touched
no `src/tui/` source, so `main`'s file-picker changes are orthogonal to
every corrective on this line. The counts that moved — *Integration test
files* **338**, *Guard scripts (all)* **91**, *CI guard scripts* **54** — are
recorded in `architecture/overview.md` and
`architecture/count_audit_history.md`, and the comment budget was re-banked
on the merged tree at **123,685** lines, still below the 124,865
pre-reduction baseline.

**A8 — the sixth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **composer's second post-closure corrective**
workstream landed at `a4bbba81e` (PR #183), and the branch tip `28ebe7163` had
to be reconciled with it. The quick lane's `==>` lines moved again, from 111 to
**113** — `run_guards` is **109** and `run_quick` is **4** — because
`origin/main` added its own composer keymap-drift guard, which runs twice
(once with `--self-test` and once for real). The count is measured, not
incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 109
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 113
steps.

The merge's only code-level interaction was additive: `main` changed no file
outside `src/tui/`, and this branch changed no file under `src/tui/`, so the
two sides' code is disjoint and `main`'s composer changes are orthogonal to
every corrective on this line. The counts that moved — *Guard scripts (all)*
**92** and *CI guard scripts* **55** — are recorded in
`architecture/overview.md` and `architecture/count_audit_history.md`.
*Integration test files* is **338** and did **not** move: `main`'s composer
corrective extended the existing members `tests/tui_render/tui_composer_claims.inc`
and `tests/tui_render/tui_composer_motions.inc` and rewrote the already-existing
`tests/tui_render/tui_composer_selection.inc`, so it added no new integration
test file. The comment budget was re-banked on the merged tree at **124,310**
lines / **1,095,646** words / ratio **0.1330**, still below the 124,865
pre-reduction baseline.
**A9 — U2 is discharged: the diff-consumer fix's runtime half now has a dedicated integration test in a standalone target.** U2 recorded that the runtime half — that an inadmissible fact produces a typed error at the security-review surface — was covered by the static guard's assertion and by the fence's own unit tests, but not by a new integration test. That gap is closed by `tests/git_consistency_consumers_c005.rs`, a standalone target that drives the property from **outside** the crate, where the unit tests next to the fence cannot reach.

The target asserts three properties, and each is stated so that a reader can tell what it does and does not prove.

1. **The real route admits.** A real repository's diff read is admitted by the fence, and `discover_targets_from_diff` returns the target, the hunks and the patch built from it. This is the *admitting* half: a fence wired to refuse everything would fail this, so the fence is load-bearing in the admitting direction as well as the refusing one. It does **not** prove the fence is *reached* — a surface that bypassed the fence would return the same content — and the target deliberately does not claim to. That is the static guard's property, and the guard can see a call site this target cannot.
2. **An inadmissible read is refused, and the refusal is typed.** The refusal reaches the caller as the surface's own `SecurityReviewReadError::Consistency` variant, carrying the consistency refusal's stable machine name — `live_class_without_capability` for a live-class read, `no_object_generation` for a read that observed no generation. The capability the target passes is `crate::git_consistency::live_read_capability()`, the **production** accessor, which returns `None` in every build: this is the production capability state, not a test-only posture.
3. **A read failure is not a consistency refusal.** On the real path — a directory that is not a repository — the surface's Git read fails before anything can be admitted, and the caller sees `SecurityReviewReadError::Read`, not `Consistency`. This is the property a flattened error would destroy.

**The target is load-bearing, and that was proved by mutation rather than asserted.** Two planted regressions were run against it, and each was reverted afterwards.

**Regression 1 — the typed refusal is flattened.** `From<GitServiceError> for SecurityReviewReadError`'s `Consistency` arm was replaced with an unconditional `Read` arm — the mistake this record's own §3.3 warns against. The target then fails, and it fails on **exactly** the test that owns the property, with the other two still passing:

```
FAIL [   0.005s] (1/3) codegg::git_consistency_consumers_c005 an_inadmissible_diff_read_is_refused_with_a_typed_error
  stderr ───
  thread 'an_inadmissible_diff_read_is_refused_with_a_typed_error' panicked at
  tests/git_consistency_consumers_c005.rs:156:13:
  the refusal must stay typed, not become a read failure: git read failed
  PASS [   0.017s] (2/3) codegg::git_consistency_consumers_c005 a_failed_diff_read_is_not_a_consistency_refusal
  PASS [   0.064s] (3/3) codegg::git_consistency_consumers_c005 the_security_review_surface_admits_a_real_repositorys_diff
  Summary: 3 tests run: 2 passed, 1 failed, 0 skipped
```

**Regression 2 — the fence admits everything.** `admit_generation_bound` was made to downgrade the read's class to an always-admissible one before admitting it, so the fence never refuses anything. The same test then fails, because the inadmissible fact is admitted instead of refused:

```
FAIL [   0.050s] (1/3) codegg::git_consistency_consumers_c005 an_inadmissible_diff_read_is_refused_with_a_typed_error
  stderr ───
  thread 'an_inadmissible_diff_read_is_refused_with_a_typed_error' panicked at
  tests/git_consistency_consumers_c005.rs:127:6:
  a live-class read must not be admitted without the live-read capability:
  GenerationConsistent { value: "torn diff", class: CapturedNotCurrent }
  PASS [   0.234s] (2/3) codegg::git_consistency_consumers_c005 a_failed_diff_read_is_not_a_consistency_refusal
  PASS [   1.437s] (3/3) codegg::git_consistency_consumers_c005 the_security_review_surface_admits_a_real_repositorys_diff
  Summary: 3 tests run: 2 passed, 1 failed, 0 skipped
```

Both regressions were reverted and the target is green on the restored tree. The two failures land on the same test for two different reasons, which is the shape that matters: the target detects a fence that refuses *incorrectly* and a fence that refuses *not at all*, and it does not fire on the two properties that are still true under either regression.

**The gates run on this tree.**

| Gate | Result |
|---|---|
| `scripts/verify.sh quick` | **green**, 113 steps |
| `cargo fmt --check --all` | green |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | green |
| `cargo nextest run --locked --profile ci -p codegg --test git_consistency_consumers_c005` | **3 passed**, 0 failed |
| `cargo nextest run --locked --profile ci -p codegg --lib` | **7,512 passed**, 4 skipped |
| `cargo nextest run --locked --profile ci -p codegg-core` | **1,141 passed**, 0 skipped |
| `cargo nextest run --locked --profile ci -p codegg --test shared_checkout_c006 --test writer_fence_c009 --test contained_worktree_c008 --test composed_two_agent_race_m011` | **37 passed**, 0 failed |
| `python3 -m unittest discover -s dev/harness_lab/tests` | **329 passed**, 1 skipped |
| `python3 scripts/check_test_ownership.py` | green |
| `python3 scripts/check_comment_budget.py` | green |

One run of the `--lib` and `codegg-core` sweeps reported **timeouts** rather than failures: `tool::git::schema_tests::a_read_only_agent_may_still_inspect` and two `codegg-core` migration tests exceeded the profile's 120-second bound, and one of them had already **passed** at 119.641 s. This host was running other sessions' builds at the time, and the tests are not slow by construction. The affected test passes in isolation in 4.97 s, and both sweeps are green when re-run — so the timeouts are load, not a defect, and nothing in this change touches `tool::git` or the migration suite.

**The manifest and the counts moved with it.** `docs/test-ownership.toml` gained the `root_git_consistency_consumers_c005` group. Without it the ownership guard fails with *"test target codegg/git_consistency_consumers_c005 exists but no group owns it"* — an unowned target can never be selected by a scoped run, so the guard's failure is the point rather than an obstacle. *Integration test files* moved **338 → 339**, re-measured on the tree with `python3 scripts/measure_overview_counts.py` rather than incremented, and recorded in `architecture/overview.md` and `architecture/count_audit_history.md`.

**What this amendment does not claim.** It does not reopen C005's status: C005 was closed before this amendment and stays closed. It discharges U2, the record's last open finding, so the record now carries no open finding. It does not claim the static guard's coverage is now redundant — the guard still owns the *reach* of the fence at every consumer, which this target cannot see. And it does not claim the runtime half was previously untested: the fence's own unit tests cover the fence; what was missing was a test that could observe the property from outside the crate, where the surface's error type is the surface's own.

**A10 — the seventh reconciliation with `main`.** `origin/main` advanced again, to `4aa07fa72`, which carried PRs #184, #187, #188, #189 and #190 — the delegated-run line's reconciliation, the desktop parity merge, the M007-A desktop gate guard, and the managed-agent-scratch M008 corrective. The branch had to be reconciled with it, and two files conflicted.

Three rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 340 (main) / 339 (branch) | **341** | The union. `main` added `tests/frontend_parity/tui_reconstruction.inc` and the managed-agent-scratch tree's `tests/scheduler_jobs/scratch_record_less_tree.inc`; this branch added `tests/git_consistency_consumers_c005.rs`. |
| Guard scripts (all) | 93 (main) / 92 (branch) | **94** | The union. `main` added `scripts/check_scratch_reclaim_invariants.py` and `scripts/check_desktop_gate_wiring.py`; this branch added none. |
| CI guard scripts | 56 (main) / 55 (branch) | **57** | The union of the two workflows' distinct `scripts/check_*` names. Both of `main`'s new guards run in **both** `quick` and CI, so the *quick runs 14 guards CI does not* difference is unchanged at **14**, and the *quick* guard count is **71**. |

`AGENTS.md`'s published counts moved with them — **71 quick / 57 CI** — and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow, not carried over. The quick lane's `==>` lines moved from 113 to **117** (`run_guards` **113**, `run_quick` **4**), because `main` added guard steps and their self-tests.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment budget's relation to the pre-reduction baseline changed, and it is worth stating plainly.** The corpus is a ratchet — no recorded ceiling may be exceeded — and the merged tree measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio, which the re-bank records exactly as measured.

The **124,865** figure is the pre-reduction corpus from the C007 comment-reduction workstream. It is a **historical reference, not a guard**: no script, no entry in `docs/comment-budget.toml`, and no CI step enforces it. The corpus crossed it before this merge — `main`'s own tree banked **125,161** before this change, already **296 lines above** the reference — so this merge did not cross it and did not cause it. The fourth, fifth, sixth and ninth amendments each recorded the corpus *below* the baseline because it *was* below on those trees; that relation simply no longer holds now that `main`'s subsequent lines have spent more comment lines than the reduction freed. Recording it as still-below would be false, so it is recorded as measured.

**`main` and this branch share the fence.** `main`'s `2222697d6` — *"carry a git read's consistency class to the client"* — was written against the guard this line merged, and it **propagates** the Phase 3 git read's class rather than admitting it, because the desktop's git read is a display read rather than a generation-bound consumer. Its commit message notes that the desktop's status and branch reads consult refs and the index rather than the object store, so their composite class is `NoObjectGeneration` — a class the fence refuses outright. That is the same refusal the new standalone integration target asserts from outside the crate, and the two agree.

**A11 — the eighth reconciliation with `main`.** `origin/main` advanced again, to `6e8a6bb60`, which carried PRs #185 and #191 — the desktop chat region and side panel (M007-B) and the panel layer's authority guard. Nothing conflicted textually, but the auto-merge produced **two** copies of the three count rows in `architecture/overview.md` — this branch's and `main`'s — which is the shape a count table takes when both sides moved the same rows without the same values. The duplicate was collapsed to one row set carrying the merged tree's measurement.

Two rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Guard scripts (all) | 94 | **95** | `main` added `scripts/check_desktop_panel_authority.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 57 | **58** | The CI workflow gained the same guard and its self-test. The measure is unchanged. |
| Integration test files | 341 | **341** | Unchanged, and worth stating rather than leaving implicit: `main`'s M007-B work added no integration test file. |

The *quick* guard count moved **71 → 72**, and the *quick runs 14 guards CI does not* difference is unchanged at **14**: the new guard runs in **both** `quick` and CI, so it adds no quick-only entry. The quick lane's `==>` lines moved from 117 to **119** (`run_guards` **115**, `run_quick` **4**). `AGENTS.md`'s published counts moved with them, to **72 quick / 58 CI**, and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow rather than carried over.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment corpus did not move, and the relation to the 124,865 reference did not change.** The merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio; the re-bank moved no per-file ceiling, because `main`'s new commits added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the tenth amendment already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.
**A12 — the ninth reconciliation with `main`.**

`origin/main` advanced again, to `cb4ceed9d`, which carried PRs #192 and #194 — the desktop keyboard-first input model (M007-C) and the desktop editor surface (M007-E). One file conflicted: `architecture/overview.md`, in the three count rows.

Two rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Guard scripts (all) | 95 (branch) / 95 (main) | **96** | The union: `main` added `scripts/check_desktop_editor_surface_authority.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 58 (branch) / 58 (main) | **59** | The union of the two workflows' distinct `scripts/check_*` names. The measure is unchanged. |
| Integration test files | 341 (branch) / 340 (main) | **341** | The union: this branch's `tests/git_consistency_consumers_c005.rs`. `main`'s two desktop milestones added no integration test file. |

The *quick* guard count moved **72 → 73**, and the *quick runs 14 guards CI does not* difference is unchanged at **14**: the new guard runs in **both** `quick` and CI, so it adds no quick-only entry. The quick lane's `==>` lines moved from 119 to **121** (`run_guards` **117**, `run_quick` **4**). `AGENTS.md`'s published counts moved with them, to **73 quick / 59 CI**, and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow rather than carried over.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment corpus did not move.** The merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio; the re-bank moved no per-file ceiling, because `main`'s two desktop milestones added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the tenth amendment already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.
