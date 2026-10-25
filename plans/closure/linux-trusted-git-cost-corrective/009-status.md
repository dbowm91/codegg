# Linux Trusted Git Cost Corrective C009 — Contained Worktree Grant Precision: Status

Status: closed

Milestone: `plans/implementation/linux-trusted-git-cost-corrective/009-contained-worktree-grant-precision.md`
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work was applied to

## 1. Executive finding

ADR-0048 accepted the writable grant `<common-gitdir>/worktrees/` for the contained managed-worktree creation child. The **code delivered that path correctly**. Five artifacts described a **narrower** grant — "the linked worktree's own administration directory" — and the containment test *locked the narrower description in* with an assertion (`assert_eq!(write, &administration_root)`) that read as though the narrower path were the one granted.

Two consequences followed from that gap.

**The description understated the grant.** The granted root is the **shared** administration root: the parent of every linked worktree's own administration directory, and the directory Git always places inside the common Git directory. A reviewer reading the ADR, the code comments, and the test would conclude the child could touch only one linked worktree's administration directory. It can touch every one of them, because the granted root is their parent.

**The obtained-root check was a substring match.** The baseline's inline check compared the required path against a comma-joined string of obtained roots with `contains`. A prefix-sharing path — `/repo/.git/worktrees-old` — satisfied a required `/repo/.git/worktrees`. The check is now a path equality comparison over the obtained set, in a named function — `obtained_write_root_is_present`.

C009 corrected the descriptions, replaced the substring comparison with equality, and added a guard that keeps the ADR, the code comments, the containment test, and the delivered grant naming **one** path. The narrowing question — should the grant be reduced to a linked worktree's own administration directory? — is **not** answered here: that would need a new ADR, and it is recorded in the amendment as an **open capability question** for the maintainer.

## 2. Requirement-to-evidence matrix

| §13 criterion | Evidence | Result |
|---|---|---|
| 1. ADR-0048, both code comments, the test's name and comment all name `<common-gitdir>/worktrees/` as the **shared** administration root | `scripts/check_contained_worktree_grant.py` asserts the agreement; `git diff` on `plans/adrs/ADR-0048-…md`, `src/git_contained_worktree.rs`, `tests/contained_worktree_c008.rs` | **met** |
| 2. The ADR's amendment records that the child can write another linked worktree's `HEAD`, and records the maintainer's disposition | `## Amendment:` section at the end of ADR-0048, quoted in §3 below | **met in the form §14 requires** — the disposition is recorded as *not chosen*; see §9 for the named open item |
| 3. An obtained-root set containing a prefix-sharing path but not the required path **fails** the obtained-root check | `tests/contained_worktree_c008.rs::the_obtained_root_check_compares_paths_and_not_substrings` | **met** |
| 4. A static guard asserts the grant/ADR agreement and rejects an injected divergence in either direction | `scripts/check_contained_worktree_grant.py --self-test`: four injections, four rejections | **met** |
| 5. Every defect above has a regression test that fails against the baseline and passes after the fix | The guard reports **8 findings** against the baseline tree and **0** against the fixed tree (§4); the obtained-root test fails against the baseline's substring comparison (§4) | **met** |
| 6. The five existing `contained_worktree_c008` tests remain green, and `verify.sh quick` and `cargo clippy --workspace --all-targets -- -D warnings` are green | §4 | **met** — `contained_worktree_c008` is 7 passed / 0 failed, `verify.sh quick` is green across its 86 steps at the closing commit, and the workspace clippy with `-D warnings` is green |

## 3. Production implementation evidence

### 3.1 ADR-0048's amendment

ADR-0048 (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`) now ends with an amendment headed `## Amendment: the shared administration root, and the capability it carries`, with the subsections `### What was wrong`, `### What changes`, `### What does not change`, `### The consequence the shared root carries` and `### Disposition: an open capability question, not an acceptance`. Its operative wording, quoted:

> ### What was wrong
>
> Clause 3 said the child may write `<common-gitdir>/worktrees/` — "the linked
> worktree's own administration directory". The **path** is right; the
> **description** is not. `<common-gitdir>/worktrees/` is the **shared**
> administration root: the **parent** of every linked worktree's own
> administration directory, not one of them.

> ### What changes
>
> **Clause 3** now names `<common-gitdir>/worktrees/` as the **shared**
> administration root, states plainly that it is the parent of every linked
> worktree's own administration directory, and states that the narrower grant —
> the linked worktree's own administration directory alone — is not granted.
>
> The accepted **path** is unchanged: `<common-gitdir>/worktrees/`, and only
> that.

> ### The consequence the shared root carries
>
> The shared root is the **parent** of every linked worktree's administration
> directory, so the child can write **any** of them — including another linked
> worktree's `HEAD`. The path to that write is real: `git worktree add` runs
> the repository's `post-checkout` hook, and the contained child's argv does not
> pin `core.hooksPath`. A repository that declares such a hook therefore runs
> it inside the containment, and the hook can repoint another worktree's branch
> pointer.

> ### Disposition: an open capability question, not an acceptance
>
> **The maintainer has not chosen a disposition for this consequence, so this
> amendment does not record one.** The consequence is recorded here as an **open
> capability question**, and the milestone that corrected the prose stops at
> the documentation correction rather than recording an acceptance that was not
> made.
>
> The question is whether the shared root's reach over **other** linked
> worktrees' `HEAD` files is an acceptable consequence of ADR-0048's accepted
> path. The candidates are listed for the maintainer to choose among; this
> amendment selects none of them:
>
> * **Accept as recorded** — the shared root stays, and the reach over other
>   worktrees' `HEAD` files becomes an accepted, documented consequence of this
>   ADR.
> * **Narrow the grant** to the linked worktree's own administration directory.
>   This **changes the accepted contract** and requires a **new ADR**; it is not
>   an edit to this one, and this milestone does not do it.
> * **Defence in depth** — pin `core.hooksPath` for the contained child, so the
>   repository's `post-checkout` hook does not run inside the containment. This
>   removes the most obvious path to the capability; it does not close it,
>   because the shared root stays writable.

The clause-3 correction is in `plans/adrs/ADR-0048-…md`, and the matching corrections are in `architecture/git.md` (§"Contained managed-worktree creation"), `architecture/security.md` (§"Read-only grants and the Git-pointer exemption"), `architecture/session_launch.md` (§`WorktreeService::create_reserved`), and `src/git_contained_worktree.rs`'s two code comments.

### 3.2 The obtained-root check

`src/git_contained_worktree.rs` — the baseline had **no** `obtained_write_root_is_present`
function at all: the check was inline in the creator, over the comma-joined
string of the backend's obtained roots:

```rust
let obtained_writes = guarantees
    .iter()
    .find_map(|entry| entry.strip_prefix("writable grant roots: "))
    .unwrap_or_default();
for required in [&checkout, &scratch, &administration_root] {
    if !obtained_writes.contains(&required.display().to_string()) {
```

A prefix-sharing path satisfied it, because the test was a substring match over
the joined string. It is now a **named** function that compares whole paths:

```rust
pub fn obtained_write_root_is_present(guarantees: &[String], required: &Path) -> bool {
    obtained_write_roots(guarantees)
        .iter()
        .any(|root| root.as_path() == required)
}
```

The obtained set itself is recovered by splitting the joined entry back into
whole paths:

```rust
fn obtained_write_roots(guarantees: &[String]) -> Vec<PathBuf> {
    guarantees
        .iter()
        .find_map(|entry| entry.strip_prefix("writable grant roots: "))
        .map(|joined| {
            joined
                .split(',')
                .map(str::trim)
                .filter(|element| !element.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}
```

Splitting is what makes the comparison exact: the elements are whole paths, and
a prefix-sharing root is a different element, not a match.

The structural `assert_grant`, which already compared the requested spec for
exact equality, is unchanged.

### 3.3 The new guard

`scripts/check_contained_worktree_grant.py` asserts that ADR-0048's clause 3,
the two code comments, and the containment test all name **one** shared
administration root, and that the grant's writable roots under the repository's
Git directory are exactly that one root. It derives the expected path from the
ADR's own text, so the guard cannot drift from the document it protects.

## 4. Verification executed

All commands were run **locally** on the implementation tree. No hosted lane was consulted, and none is claimed.

```
$ python3 scripts/check_contained_worktree_grant.py
contained-worktree-grant check ok: ADR-0048's clause 3, the two code comments and the grant name one shared administration root, and the Git-directory write root is the only one

$ python3 scripts/check_contained_worktree_grant.py --self-test
  ok: accepted the unmodified tree
  ok: rejected a code comment that names the wrong path
  ok: rejected an ADR that names the wrong path
  ok: rejected a grant that adds a second writable root under the Git directory
  ok: rejected a containment test still named after the worktree's own directory
contained-worktree-grant self-test ok
```

**Non-vacuity against the baseline.** The guard was copied into an extracted
copy of the baseline tree (`git archive f1b83a7a7`) and run there:

```
$ cd <baseline-extract> && python3 scripts/check_contained_worktree_grant.py
contained-worktree-grant check: FAILED
  - ADR-0048's clause 3 describes <common-gitdir>/worktrees/ as the linked worktree's own administration directory; …
  - ADR-0048's clause 3 does not describe <common-gitdir>/worktrees/ as the shared administration root; …
  - ADR-0048's clause 3 does not call <common-gitdir>/worktrees/ an administration root
  - ADR-0048's clause 3 does not state that <common-gitdir>/worktrees/ is the parent of every linked worktree's own administration directory
  - the build_worktree_grant's comment names 0 `<common-gitdir>/...` paths, expected exactly one: []
  - the assert_grant's comment names 0 `<common-gitdir>/...` paths, expected exactly one: []
  - the containment test no longer names a test for the shared administration root; …
  - the containment test still names a test after the linked worktree's own administration directory; …
```

**8 findings against the baseline, 0 against the fixed tree.** The guard is
not vacuous: it fails on the tree it was written for.

**Test evidence.** `tests/contained_worktree_c008.rs` gained
`the_obtained_root_check_compares_paths_and_not_substrings`, which feeds the
obtained-root set `/repo/.git/worktrees-old` — a path that shares a prefix
with, but is not, the required `/repo/.git/worktrees` — and asserts the check
**fails**. It also asserts that the exact path passes, that the order of the
reported roots and extra roots alongside the required one do not matter, and
that a launch which obtained no writable root at all does not pass.

Against the baseline the check was a substring comparison over the joined roots,
so the prefix-sharing root satisfied it —
`"/repo/.git/worktrees-old".contains("/repo/.git/worktrees")` is true — and the
baseline had no `obtained_write_root_is_present` to call at all: the check had
to be extracted into a named function before the assertion could be written at
all. The other test was renamed from
`the_grant_names_only_the_administration_directory` to
`the_grant_names_only_the_shared_administration_root`, its comment was
corrected, and its assertion now checks the **set** of writable roots under the
Git directory rather than one path.

**The five pre-existing `contained_worktree_c008` tests** remain green; the
target's count went from five to **seven** with the two added tests —
`the_grant_writes_exactly_one_path_under_the_git_directory` and
`the_obtained_root_check_compares_paths_and_not_substrings` — and none of the
five was altered except for the renamed test's comment and assertion shape.

**`verify.sh quick` and `cargo clippy --workspace --all-targets -- -D warnings`** are **green** on this tree — see §9 (U2). The host's disk was at 100% during this milestone, so both whole-tree gates were deferred; they have since been run, and the workspace clippy needed four lints the new code introduced to be fixed before it was green.

## 5. Invariant review

§4's eight invariants, reviewed against the implementation tree:

| # | Invariant | Status |
|---|---|---|
| 1 | One operation, in two steps — `git worktree add --detach` then `git symbolic-ref HEAD` | **intact** — the argv is unchanged |
| 2 | The child's writable set is exactly three roots (create) and two (attach) | **intact, and now stated correctly** — the administration root is named as the shared root in the comments that describe it |
| 3 | Every other path under the Git directory is read-only except the administration root | **intact** — the grant's write roots are unchanged; the guard asserts there is exactly one |
| 4 | The source working tree is readable and never writable | **intact** — no grant change |
| 5 | The only `.git` the child may create is the checkout's own pointer file | **intact** — no grant change |
| 6 | Containment is asserted against the built description, not the requested configuration | **intact** — `assert_grant` is unchanged |
| 7 | The branch is owned by the daemon, not by the child | **intact** — the attached form of `worktree add` is still not used |
| 8 | The contained child reaches the launch instead of failing on its own paths | **intact** — the obtained-root comparison now matches on equality, which admits the legitimate single-root case exactly as the old substring match did, and refuses only the prefix-sharing case that was never a legitimate grant |

## 6. Failure, cancellation, restart and contention semantics

No lifecycle changed. The child still runs one operation in two steps; the
grant still describes the same three (create) and two (attach) roots; the
branch is still owned by the daemon. The obtained-root comparison's failure
mode moved from "a prefix-sharing path was accepted" to "a prefix-sharing path
is refused", which is the direction §4 invariant 6 already required.

## 7. Compatibility and migration review

No migration. No schema change. No durable payload change. No public API
change beyond one added `pub fn` on the root crate's contained-worktree module
(`obtained_write_root_is_present`), which is exercised by the integration test.

The **description** change is documentation-only, and that is the point: the
behaviour did not change, so nothing that depended on it can regress.

## 8. Security review

The grant's *behaviour* is unchanged, so this milestone does not narrow any
capability. It makes the existing capability **legible**: a reader of ADR-0048
now learns that the child can write another linked worktree's `HEAD`.

The obtained-root check's direction changed from fail-open (a prefix-sharing
path was accepted as the required root) to fail-closed. The only behaviour a
caller can observe is that a backend reporting the *wrong* root under a
prefix-sharing name is now refused where it was previously admitted. No
legitimate backend reports a prefix-sharing root in place of the required one.

The guard `scripts/check_contained_worktree_grant.py` runs automatically
(`scripts/verify.sh quick` and CI), with its `--self-test`, so a future change
that re-narrows the description — or widens the grant — fails the build rather
than waiting for a reader to notice.

## 9. Unresolved findings

> **U1 discharged on 2026-10-25.** U1 below was closed by a follow-up after this
> record was written; see §14 (A14). Its text is kept as it stood when the record
> was closed, because a reader comparing the record with the tree's history needs
> to see the finding and its discharge, not a rewritten finding. U2 was already
> closed when the record was closed.

**U1 — the maintainer has not chosen a disposition for the "another linked worktree's `HEAD` is writable" consequence.** The amendment records the consequence and records the disposition as *not chosen*. Narrowing the grant would need a new ADR; the milestone explicitly stops at the documentation correction. **This is a decision the maintainer owns**, and it is the only thing standing between this milestone and a narrower grant. Severity: medium. No code depends on it.

**U2 — §13 criterion 6's whole-tree gates were run on this tree after the disk emergency.**
The host's disk was at 100% during this milestone (the shared XDG trash had
accumulated 287 GB of already-trashed build caches, which were purged to make
the tree buildable at all), and the guard and the scoped tests were prioritised
over the whole-tree gates. Both have since been run:

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

## 10. Roadmap disposition

The Linux trusted Git cost corrective line moves from
`active; C001/C002/C003/C004 are bounded test-only handoffs; C005 blocked; C009 ready` to
`active; C001/C002/C003/C004 are bounded test-only handoffs; C005 blocked; C009 closed`. C009 is the last milestone this line had ready.

The line's open item remains **C005** (local Git parity and admission), which is blocked on accepted ADR-0044, the unqualified tool-program child-job route, the machine-readable supported-host policy, and separate Linux C002/M003/C010 decisions. **This milestone does not unblock C005 and does not change C005's conditions.**

**M005's own status is unchanged by this milestone.** M005's condition is C007's closure, on the agent-workstream writer-containment corrective line; the Linux trusted Git line owns a different defect and does not discharge it.

## 11. Registry updates

`plans/registry.md` records, in this same commit:

- the Linux trusted Git cost corrective row's status cell →
  `active; C001/C002/C003/C004 are bounded test-only handoffs; C005 blocked; C009 closed`;
- the "Dependency-ready implementation plans" entry for this plan → moved out of dependency-ready, with its closure record named;
- a new closure-table row for `C009` on the Linux trusted Git cost corrective line.

## 12. Documentation updates

- `plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md` — clause 3 corrected; `## Amendment:` section added.
- `architecture/git.md` — the grant's description corrected; the detector's invocation points named (§ C005's requirement, satisfied here).
- `architecture/security.md` — the read-only-grant paragraph corrected.
- `architecture/session_launch.md` — the contained-creation paragraph corrected.
- `architecture/testing.md` — the containment test's name updated in the platform-gate table.
- `architecture/overview.md` — the two file-count rows re-measured.
- `AGENTS.md` — the new guard's classification recorded.

## 13. Handoff notes

The next reader should know three things.

**The grant is the shared root, and that is deliberate.** The amendment is the record of the decision not yet made. If the maintainer chooses to narrow the grant, that is a **new ADR**, not an edit to this one, and the guard `scripts/check_contained_worktree_grant.py` will need its expected path re-derived from the new ADR's text.

**The guard derives its expectation from the ADR.** It reads the ADR's own wording to learn which path the grant is, so it cannot drift from the document. If the ADR changes, the guard follows it — which is why the guard asserts the ADR *says* the right thing (names it as the shared root, calls it an administration root, states the parent relationship) rather than merely matching a path.

**The baseline tree still contains the narrower description.** The baseline is `f1b83a7a7`; the ADR amendment, the code-comment corrections, the test rename and the obtained-root fix all landed after it.

## 14. Amendment: implementation review

An implementation review of this milestone's change set found two weaknesses
in the milestone's own guard. The record is amended rather than replaced:
the closure stands, and the amendments below are what the review changed.

**A1 — the guard's write-root scan was window-limited.** The guard located
the grant's write roots between two string markers, so a Git-directory-rooted
write root added **after** the window was invisible to it. The scan now
covers the whole `build_worktree_grant` body, so a write root anywhere in
that function is visible.

**A2 — the guard asserted the refusal textually.** The guard searched the
normalised source for the literal condition text, so appending `&& false` to
that condition — which makes the refusal dead — left the guard's expectation
satisfied. The check is now **behavioural**: the refusal's condition is
lifted out of `assert_grant` and evaluated against a synthetic write-root
set, and it must refuse a non-administration Git-directory root while
permitting the administration root and a root outside the Git directory. A
tautology (`&& false`, `|| true`) is rejected independently.

**A3 — the self-test grew from 4 injections to 9.** The new injections cover
a widened allowed path after the configuration, a Git-directory root named
after the configuration, a refusal neutered with `&& false`, a refusal made
vacuous with `|| true`, and a refusal that permits a path **under** the
administration root. Each was verified to be **vacuous against the old
guard** and **rejected by the new one**. The last one was found by the
review: the original three synthetic write roots did not include a path
inside the administration root, so replacing the refusal with
`!write.starts_with(administration_root)` — which permits writes to a
sibling worktree's directory inside the administration root — was not
detected.

**A4 — the baseline/fixed distinction is preserved.** The guard still
reports **8 findings** on the baseline commit `f1b83a7a7` and **0** on the
fixed tree.

**A5 — the `verify.sh quick` step count.** The review removed a duplicated
`check_writer_fence_completeness` invocation from `scripts/verify.sh` (−2
steps) and wired `check_daemon_cwd_usage.py`, which was wired into neither
`verify.sh quick` nor CI (+2 steps). The count is unchanged at **86 steps** over a different guard set — the count
at the branch tip `8225f42dd`; the merged tree's count is in §14 A7.

**A6 — the record's quoted self-test output is milestone-scoped.** The
milestone's own four-injection measurement remains as recorded; the live
count is now **9**.

**A7 — the merged tree's measurements.** The **86 steps** recorded above were
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

`contained_worktree_c008` is still **7** in the merged tree — the merge added
no test to it — so §4's table needed no correction.

**A8 — the fourth reconciliation with `main`.** `origin/main` advanced again
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

**A9 — the fifth reconciliation with `main`.** `origin/main` advanced again
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

**A10 — the sixth reconciliation with `main`.** `origin/main` advanced again
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

**A11 — the seventh reconciliation with `main`.** `origin/main` advanced again, to `4aa07fa72`, which carried PRs #184, #187, #188, #189 and #190 — the delegated-run line's reconciliation, the desktop parity merge, the M007-A desktop gate guard, and the managed-agent-scratch M008 corrective. The branch had to be reconciled with it, and two files conflicted.

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

**A12 — the eighth reconciliation with `main`.** `origin/main` advanced again, to `6e8a6bb60`, which carried PRs #185 and #191 — the desktop chat region and side panel (M007-B) and the panel layer's authority guard. Nothing conflicted textually, but the auto-merge produced **two** copies of the three count rows in `architecture/overview.md` — this branch's and `main`'s — which is the shape a count table takes when both sides moved the same rows without the same values. The duplicate was collapsed to one row set carrying the merged tree's measurement.

Two rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Guard scripts (all) | 94 | **95** | `main` added `scripts/check_desktop_panel_authority.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 57 | **58** | The CI workflow gained the same guard and its self-test. The measure is unchanged. |
| Integration test files | 341 | **341** | Unchanged, and worth stating rather than leaving implicit: `main`'s M007-B work added no integration test file. |

The *quick* guard count moved **71 → 72**, and the *quick runs 14 guards CI does not* difference is unchanged at **14**: the new guard runs in **both** `quick` and CI, so it adds no quick-only entry. The quick lane's `==>` lines moved from 117 to **119** (`run_guards` **115**, `run_quick` **4**). `AGENTS.md`'s published counts moved with them, to **72 quick / 58 CI**, and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow rather than carried over.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment corpus did not move, and the relation to the 124,865 reference did not change.** The merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio; the re-bank moved no per-file ceiling, because `main`'s new commits added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the tenth amendment already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.
**A13 — the ninth reconciliation with `main`.**

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

**A14 — the maintainer's chosen disposition for U1: defence in depth, and the hooks-path pin.**

The maintainer has chosen the disposition U1 left open: **defence in depth**. The shared administration root stays writable and the capability it carries stays open — narrowing the grant is still a **new ADR**, and this milestone did not narrow it. What the disposition removes is the most obvious path to that capability: the contained worktree child's argv now carries `-c core.hooksPath=/dev/null`, so the repository's own hooks cannot run inside the containment at all.

**Why the pin is on the argv, and not in the environment.** This is the one part of the disposition that is not obvious, and it was settled by measurement rather than by preference. Git's command-scope configuration can also be carried by `GIT_CONFIG_COUNT` with `GIT_CONFIG_KEY_0`/`GIT_CONFIG_VALUE_0`, and that mechanism *does* outrank the repository's own configuration file — measured on this host, where the repository's local `core.hooksPath` was overridden by the environment route and the hook stopped running. But those variables are in `egggit`'s `ALWAYS_STRIPPED_ENV_VARS`, so the child's environment policy **hard-denies** them, and `EnvironmentPolicy::with_var` **silently refuses** a denied variable rather than reporting it. A pin carried there would therefore be dropped before the child ever saw it: a disposition that reads like a control and is a no-op. The first draft of the test drove the environment and **failed** for exactly that reason. A `-c` on the argv is the mechanism the daemon's own trusted Git reads already use (`egggit::add_trusted_git_overrides`), it is not on the deny list, and it outranks the repository's local configuration.

**What was implemented.**

* `src/git_contained_worktree.rs` — the constant `WORKTREE_CHILD_HOOKS_PATH_PIN` and the argv builder that applies it. The pin is applied to **both** steps of the operation, ahead of the subcommand, because a Git **global** option only applies before it.
* `scripts/check_contained_worktree_hooks_path.py` — the static guard, wired into `verify.sh quick` and CI, with a `--self-test`. It checks that the constant is declared with the pinned key and a value that names no hook directory, that the argv builder applies it ahead of every step-specific subcommand, that the child's environment carries **no** command-scope pin (the hard-denied route), that ADR-0048 records the disposition as chosen, and that the behavioural proof exists and derives its control from the production pin.
* `tests/contained_worktree_hooks_path.rs` — the behavioural proof, in a standalone target.
* `tests/contained_worktree_c008.rs` — the containment test's hook assertion is now the **negative** one. It used to require the repository's hook to have run and merely be contained; it now requires that it does **not** run at all. That is a *stronger* statement than the one it made before.
* `plans/adrs/ADR-0048-…md` — the disposition section now records the choice, clause 5 no longer lists *hooks* among the repository configuration surfaces that still apply, and a dated amendment records the mechanism and what it does not change.

**What the behavioural proof claims, and how it avoids being vacuous.** The test runs the repository's `post-checkout` hook under the production argv with the **pin pair removed** — the control — and the hook runs; it then removes the control's marker and runs the production argv, and the hook does not run, while the worktree is still created. The control is derived from the production argv rather than written out separately, so it cannot drift from what the child really runs. Without that control the claim would be satisfied by a child that never ran hooks for an unrelated reason.

**Three planted regressions proved the proof load-bearing**, and each was reverted:

| Plant | Where it failed |
|---|---|
| The pin removed from the argv builder | `tests/contained_worktree_hooks_path.rs:200` — "the production argv does not carry the hooks-path pin at all" |
| The pin renamed to a key that does not disable hooks (`core.pager=cat`) | `tests/contained_worktree_hooks_path.rs:237` — "the production argv does not pin core.hooksPath" |
| The pin applied only to the attach step | `tests/contained_worktree_hooks_path.rs:200` — "the production argv does not carry the hooks-path pin at all" |

**What this amendment does not claim.** It does not close the capability: the shared root stays writable, so the child can still write another linked worktree's `HEAD`. It does not claim the pin is the only way to reach it. It does not claim the repository's hooks are the only hostile configuration the child runs — a hostile `filter.*.smudge` still runs, by ADR-0048's own accepted clause 5, and it is the sandbox that contains it. And it does not reopen C009's status: C009 was closed before this amendment and stays closed. What it does is discharge U1, the record's last open finding, so the record now carries no open finding.

**The counts that moved on this tree.** *Guard scripts (all)* **96 → 97** (this line's `scripts/check_contained_worktree_hooks_path.py`), *CI guard scripts* **59 → 60** (the workflow gained the same guard and its self-test), and *Integration test files* **341 → 342** (`tests/contained_worktree_hooks_path.rs`). The *quick* guard count moved **73 → 74**, the *quick runs 14 guards CI does not* difference is unchanged at **14** (the new guard runs in both lanes), `AGENTS.md`'s published counts moved to **74 quick / 60 CI**, and the quick lane's `==>` lines moved from 121 to **123** (`run_guards` **119**, `run_quick` **4**). Every other row was confirmed unchanged by measurement.

**The comment corpus moved, and the movement is accounted for.** The tree now measures **125,249** comment lines, **1,104,302** words and a **0.1337** ratio, against the recorded **125,212** / **1,103,943** / **0.1337**. The increase is the new target's documentation; it is partly offset by two genuine per-file reductions in the same diff — `src/git_contained_worktree.rs` **284 → 280** and `tests/contained_worktree_c008.rs` **171 → 170** — where three comment blocks in the source repeated what the named functions already say and the containment test's obtained-root block repeated the paragraph the source documents. The corpus remains **above** the 124,865 pre-reduction figure, which is a historical reference and not a guard.

**The count table was also repaired.** `architecture/overview.md`'s Verified-counts table carried **two** copies of the three count rows — one set at **341** and one at **340** — a merge artifact from an earlier reconciliation, in which both sides moved the same rows without the same values. The duplicate was collapsed to one row set carrying the tree's measurement.

**Verification on this tree.** `scripts/verify.sh quick` is **green** at **123 steps**; `tests/contained_worktree_hooks_path.rs` passes; `scripts/check_contained_worktree_hooks_path.py` and its `--self-test` pass; `scripts/check_contained_worktree_grant.py` still passes against the reduced comments, which is the check that the ADR, the two code comments and the containment test still name one shared administration root.
