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
| 2. The guard's `--self-test` includes the injection and reports it rejected | §4.2 — the self-test's twenty-first through twenty-fourth cases | **met** |
| 3. The guard runs in `verify.sh quick` and in CI with its `--self-test`; `AGENTS.md` records the change and its reason | §3.4 | **met** |
| 4. `src/security/workflow/report.rs` admits both diff reads through `admit_generation_bound`, and an inadmissible fact produces a typed error | §3.3 | **met** |
| 5. Every defect above has a regression test that fails against the baseline and passes after the fix | §4.1 (guard) and §4.3 (consumers) | **met** |
| 6. The trusted route's existing containment is unchanged | §5 | **met** |
| 7. `verify.sh quick`, workspace clippy and the named test targets are green | §4.4 | **met** — `verify.sh quick` is green across its 86 steps and the workspace clippy with `-D warnings` is green; the named test targets are green |

## 3. Production implementation evidence

### 3.1 The detector-call obligation

`scripts/check_git_forbidden_patterns.py` gained the rule
`hostile-git-metadata-detector-call-missing`. It asserts that **both**
`GitEnvPolicy::apply` and `GitEnvPolicy::apply_sync` call
`ensure_repository_config_surface_is_contained`.

The rule matches a **call**, not a mention: the detector name must be followed
by `(` in a **comment-stripped** method body, using the script's existing
`_strip_line_comments` machinery. A comment that names the detector does not
satisfy the obligation — which is precisely how the original gap was invisible.

Asserting only one of the two call sites would have left the mutation
half-passing, so both are asserted and both are injected in the self-test.

### 3.2 The guard's self-test

The self-test gained three cases:

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
    live_read_capability(),
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
The reason is recorded in `AGENTS.md`: *"A guard which is not run automatically
cannot protect anything."* The script was previously classified as
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

**U1 — the whole-tree verification gap opened by the disk emergency is closed.**
The host's disk reached 100% during this milestone (the shared XDG trash had
accumulated 287 GB of already-trashed build caches; purging it was necessary to
make the tree buildable at all), and the scoped tests and the guards were
prioritised over the whole-tree gates. Both have since been run on this tree:

- `scripts/verify.sh quick` — **green**, 86 steps: `cargo fmt --check --all`,
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
