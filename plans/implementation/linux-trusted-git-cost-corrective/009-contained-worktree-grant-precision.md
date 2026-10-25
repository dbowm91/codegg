# Linux Trusted Git Cost Corrective C009 — Contained Worktree Grant Precision

Status: closed — C009 closed (`plans/closure/linux-trusted-git-cost-corrective/009-status.md`). ADR-0048, the two code comments and the containment test now all name the shared administration root; the obtained-root check compares paths for equality; and the new guard rejects an injected divergence in either direction. The amendment records the consequence that the child can write another linked worktree's `HEAD`, and the maintainer's disposition for it is now **chosen**: **defence in depth** — the contained child's argv pins `core.hooksPath`, so the repository's own hooks cannot run inside the containment. The grant is unchanged, so narrowing it would still need a new ADR.
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work is applied to
Source roadmap: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md` §§9, 16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0048 (contained Git execution for managed worktree creation, **accepted**), ADR-0002 (agent run worktree isolation and control, accepted)
Primary class: invariant

## 1. Objective

Make one claim true end to end:

> **The writable set the contained worktree child receives is exactly the set ADR-0048 accepted — and every document that describes it says the same thing, and the check that proves the child obtained it cannot be satisfied by a different path.**

This milestone closes two defects found in the 2026-10-23 implementation review of the blackboard line. One is a major: the accepted ADR, two code comments, a test's name and a test's own comment all describe the grant as "the linked worktree's **own** administration directory", while the code grants the **shared** `<common-gitdir>/worktrees/` root — the parent of every linked worktree's administration directory. The other is a minor: the check that asserts the child *obtained* the required writable root is a substring match over a comma-joined list, so a root that merely shares a prefix satisfies it.

The claim is about the **contained managed-worktree creation grant** that C008 introduced under ADR-0048. The Git consistency contract and its guard are the Linux sandbox admission line's C005.

## 2. Why this milestone is ready

- **C008 is closed.** `plans/closure/linux-trusted-git-cost-corrective/008-status.md` landed the contained managed-worktree creation path under ADR-0048, and the hosted `linux-m004-native.yml` `process-ipc` job passed on that head (run `39829806588`).
- **ADR-0048 is accepted.** The grant's *path* — `<common-gitdir>/worktrees/` — is the maintainer's recorded choice (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`, acceptance record: the writable grant is "**`<common-gitdir>/worktrees/` only**", with the branch owned by the daemon). This milestone does not revisit that choice.
- **The defect is in the description, not in the code's behaviour.** The code matches the accepted path. Five artifacts describe a narrower grant than the code delivers, and one of them is the test whose job is to assert containment.
- **No new ADR is required for the work in scope.** Correcting an accepted ADR's *description* to match the path it already accepted is a factual amendment, not a new decision. Narrowing the grant further **would** require a new ADR and is explicitly out of scope (§5).

## 3. Current implementation evidence

### 3.1 Five artifacts describe a narrower grant than the code delivers (major)

The code computes **both** paths and grants the shared one:

```rust
let administration_root = canonical_pending(&gitdir.join("worktrees"))?;
…
let administration_dir = administration_root.join(
    checkout.file_name()… .to_string_lossy().to_string(),
);
```

(`src/git_contained_worktree.rs:282`, `:292-302`.)

The grant uses the **shared root**:

```rust
let (write_roots, expects_pointer) = match step {
    WorktreeChildStep::Create => (
        vec![
            paths.checkout.clone(),
            paths.scratch.clone(),
            paths.administration_root.clone(),
        ],
        true,
    ),
    WorktreeChildStep::AttachBranch => (
        vec![paths.scratch.clone(), paths.administration_root.clone()],
        false,
    ),
};
```

(`src/git_contained_worktree.rs:581-594`.) `WorktreeChildPaths::administration_root` is documented as "`<common-gitdir>/worktrees/` — the grant's administration root" (`:114-115`) and `administration_dir` as "the linked worktree's own administration directory" (`:116-117`) — the latter is computed and then **not granted**.

The five overstated descriptions:

1. **ADR-0048 clause 3** — *"the child may write exactly three things: the new worktree directory, the daemon-owned scratch root, and `<common-gitdir>/worktrees/` — **the linked worktree's own administration directory**"* (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md:164-170`).
2. **`src/git_contained_worktree.rs:561-565`** — *"The grant is exactly ADR-0048 clause 3: the child may write the new checkout, the daemon scratch, and the linked worktree's **own** administration directory."*
3. **`src/git_contained_worktree.rs:662-664`** — *"The child may write exactly three things: the new checkout, the daemon scratch, and the linked worktree's **own** administration directory."*
4. **The test's name** — `the_grant_names_only_the_administration_directory` (`tests/contained_worktree_c008.rs:165`).
5. **The test's own comment** — *"Nothing under the Git directory is writable except the administration directory"* (`tests/contained_worktree_c008.rs:237-238`), while the assertion it guards permits the whole shared root:

```rust
for write in &spec.write_paths {
    if write.starts_with(&gitdir) {
        assert_eq!(
            write, &administration_root,
            "the child was granted {write:?} inside the repository's Git directory"
        );
    }
}
```

(`tests/contained_worktree_c008.rs:239-244`.)

**Consequence.** The granted capability is systematically overstated in the artifacts whose job is to assert it. The concrete capability is: **the child can write `<common-gitdir>/worktrees/<other>/HEAD`** for any other linked worktree in the repository — that is, it can repoint another worktree's branch.

The path to that write is real. `git worktree add` **runs the repository's `post-checkout` hook**, and the contained child's argv does not pin `core.hooksPath`:

```rust
WorktreeChildStep::Create => vec![
    format!("--git-dir={}", paths.gitdir.display()),
    "worktree".to_string(),
    "add".to_string(),
    "--detach".to_string(),
    paths.checkout.display().to_string(),
    paths.base_commit.clone(),
],
```

(`src/git_contained_worktree.rs:641-648`.) A repository that declares a `post-checkout` hook therefore runs that hook inside the containment, and the hook can write the other worktree's `HEAD` because the whole `worktrees/` tree is writable.

**What is unaffected.** The child's own `refs` remain read-only: `refs/heads/main` is under `<common-gitdir>/refs`, not under `<common-gitdir>/worktrees`. ADR-0048 clause 3's literal "refs is read-only" holds, and the child cannot create a branch.

**Why the review's severity assessment is prose-versus-effect rather than code-drift.** The accepted ADR names the path `<common-gitdir>/worktrees/`, and the acceptance record names it explicitly. The code matches. What is wrong is that the ADR's prose, the code's own comments and the containment test all describe a **different, narrower** grant — and the ADR's own decision driver rejected exactly the capability the shared root permits (a child that can write another worktree's branch pointer).

### 3.2 The "obtained enforcement" check is a substring match (minor)

The structural assertion `assert_grant` (`src/git_contained_worktree.rs:669-720`) does exact equality on the **requested** spec — that part is correct. The check that the child **obtained** the required writable root is not:

```rust
if !obtained_writes.contains(&required_path) { … }
```

(`src/git_contained_worktree.rs:390-401`, `:449-458`), where `obtained_writes` is a comma-joined string built by `join_paths` (`src/security/sandbox/linux_mount_view.rs:540-546`).

**Trigger:** a backend that obtained a root merely **sharing a prefix** with the required one — for example `/repo/.git/worktrees-old` — satisfies `obtained_writes.contains("/repo/.git/worktrees")`.

**Consequence.** The evidence check meant to prove the child obtained the administration root can be satisfied by a different root. ADR-0048 clause 4 is *"Containment is asserted, not requested"*, and this is the half of that assertion that can be satisfied by a prefix-sharing path.

## 4. Invariants that must not regress

1. **One operation, in two steps.** The child runs `git worktree add --detach <path> <commit>` and then `git symbolic-ref HEAD refs/heads/<branch>` (`src/git_contained_worktree.rs:641-654`). No other command, and no widening of the capability.
2. **The child's writable set is exactly three roots** in the create step (checkout, scratch, administration root) and two in the attach step (scratch, administration root) (`src/git_contained_worktree.rs:581-594`).
3. **Every other path under the repository's Git directory is read-only to the child** — `refs`, `objects`, `index`, `config`, `hooks`, `packed-refs` — except the administration root.
4. **The source working tree is readable and never writable.**
5. **The only `.git` the child may create is the checkout's own pointer file.**
6. **Containment is asserted against the built description, not the requested configuration** — a request is a hope and the description is what the child runs with (`src/git_contained_worktree.rs:658-668`).
7. **The branch is owned by the daemon**, not by the child.
8. **The contained child reaches the launch instead of failing on its own paths** — the property C008's closure recorded.

## 5. Scope

**In scope**

- The five overstated descriptions in §3.1, corrected to name the grant that is actually delivered.
- The obtained-root check in §3.2, so it compares paths rather than substrings.
- A guard that asserts the granted writable root under the repository's Git directory is exactly the ADR-named path, and that the ADR's prose and the code's comments agree with it.
- The ADR-0048 amendment recording the consequence in §3.1 explicitly.
- Regression tests for both defects, and the guards in §10.

**Out of scope**

- **Narrowing the grant to `administration_dir`.** ADR-0048 accepted `<common-gitdir>/worktrees/` and this milestone does not revisit that. Narrowing would change the accepted contract and requires a **new ADR**; it is recorded as a decision point in §14, not as work.
- Pinning `core.hooksPath` for the contained child. That is a defence-in-depth change to the same surface; it belongs with a narrowing decision, because with the shared root granted it does not close the capability, it only removes the most obvious path to it.
- The Git consistency contract, the hostile-config mediation and their guard — the Linux sandbox admission line's C005 owns them.
- Any change to the command, the step structure, the sandbox tiers, or the containment backend.
- The trusted route's own overrides and env policy (`crates/egggit/src/process.rs`), which were verified separately to contain hostile smudge filters, `core.fsmonitor`, `core.pager`, `core.sshCommand`, `core.editor`, `alias.*` and `GIT_*` injection.

## 6. Required production changes

### 6.1 Correct the five descriptions to name the delivered grant

Each of the five artifacts in §3.1 must name `<common-gitdir>/worktrees/` — the shared administration root — and must say plainly that it is the **parent** of every linked worktree's administration directory, not the worktree's own.

The ADR amendment must use the house form: a `## Amendment:` section at the end of the accepted ADR, recording the date, the reason, and what changed. The ADR's accepted **path** is not changed; only its description and the recorded consequence are.

The amendment must also record the consequence that the shared root permits: **the child can write another linked worktree's `HEAD`**, and therefore a repository's `post-checkout` hook — which `git worktree add` runs — can repoint another worktree's branch. That consequence is currently absent from the ADR, and its absence is why the defect read as a documentation nit rather than a capability question.

**Constraint.** The amendment must not present the consequence as accepted-without-qualification unless the maintainer says so. If the maintainer has not chosen to accept it, the amendment records it as an **open capability question** and the milestone's §14 stop condition applies.

### 6.2 Compare the obtained root as a path, not a substring

The obtained-root check must split the obtained set into its elements and compare each for equality, rather than matching a joined string. The structural `assert_grant` (`:669-720`) already does exact equality on the requested spec; the obtained check must have the same precision.

The check must work for every backend that reports its obtained roots, including one that reports them in a different order.

### 6.3 Add a guard that the delivered grant matches the ADR's named path

A static guard must assert that:

- the writable roots the grant produces under the repository's Git directory are **exactly one** path, and it is the ADR-named administration root;
- ADR-0048's clause 3 names that same path, and does **not** describe it as the linked worktree's own administration directory;
- the two code comments that describe the grant name the same path.

This guard is what prevents the descriptions and the code from diverging again.

## 7. Ordered work packages

### WP1 — Amend ADR-0048 and correct the descriptions

**Intent:** every artifact names the grant that is delivered, and the consequence is recorded.

**Required changes:** §6.1.

**Acceptance evidence:** ADR-0048 carries an `## Amendment:` section naming `<common-gitdir>/worktrees/` as the shared administration root, recording the "another linked worktree's `HEAD` is writable" consequence, and stating the maintainer's disposition of it. Both code comments and the test's name and comment agree.

### WP2 — Make the obtained-root check exact

**Intent:** a prefix-sharing root cannot satisfy the obtained check.

**Required changes:** §6.2.

**Acceptance evidence:** an obtained set containing `/repo/.git/worktrees-old` but not `/repo/.git/worktrees` **fails** the check.

### WP3 — The grant/ADR agreement guard

**Intent:** the descriptions and the code cannot diverge again.

**Required changes:** §6.3.

**Acceptance evidence:** the guard is wired into `scripts/verify.sh quick` or CI and its `--self-test` rejects an injected divergence — both a code comment that names the wrong path and a grant that adds a second writable root under the Git directory.

### WP4 — Regression suite

**Intent:** both defects are covered.

**Required changes:** §10.

**Acceptance evidence:** the tests in §10 fail against the baseline and pass after the fix.

## 8. Failure, cancellation, restart and contention semantics

- **The backend cannot obtain the administration root.** The launch fails closed, exactly as it does today. The obtained-root check's precision change must not turn a *partial* obtain into a pass.
- **The backend reports the obtained roots in a different order or with extra roots.** The check must still pass when the required root is present and fail when it is absent. §6.2's change must not make it order-sensitive.
- **The repository has no other linked worktree.** Nothing changes; the shared root and the own directory coincide for the single-worktree case.
- **The contained child fails mid-step.** Unchanged. The grant's per-step narrowing (`src/git_contained_worktree.rs:581-594`) and the two-step structure are not touched.
- **The daemon restarts.** The grant is built per launch; no durable state changes.

## 9. Compatibility and migration

- **No schema change and no durable-record change.** The grant is computed per launch from the repository's Git directory.
- **The ADR amendment does not change the accepted path**, so no implementation behaviour changes and no previously-qualified evidence is invalidated. The hosted `linux-m004-native.yml` `process-ipc` run `39829806588` remains the evidence for C008's containment tiers.
- **The obtained-root check's precision change** can only turn a false pass into a false. If it turns a previously-passing backend into a failing one, that backend was not obtaining the required root and must be reported, not accommodated.
- **The consequence recorded in the amendment** narrows nothing in the implementation. It makes a previously-undocumented capability explicit. If the maintainer decides the consequence is unacceptable, the disposition is a new ADR and §14's stop condition applies.

## 10. Required tests

**Focused tests**

- `tests/contained_worktree_c008.rs` — the obtained-root check rejects a prefix-sharing path (`/repo/.git/worktrees-old` against a required `/repo/.git/worktrees`), and accepts the exact path.
- `tests/contained_worktree_c008.rs` — the grant's writable roots under the Git directory are exactly one path, and it is the administration root.

**Guard** (new, with a `--self-test`)

- The grant/ADR agreement guard of §6.3, with injected-divergence cases for: a code comment naming the wrong path, the ADR naming the wrong path, and a grant with a second writable root under the Git directory.

**Non-regression**

- `tests/contained_worktree_c008.rs` — the existing five tests remain green: `the_contained_child_cannot_write_outside_its_grant`, `the_grant_names_only_the_administration_directory` (renamed per §6.1), `the_git_execution_policy_is_unchanged`, `the_creator_reaches_the_launch_instead_of_failing_on_its_own_paths`, `the_seam_creates_a_real_checkout_and_fails_closed`.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test contained_worktree_c008
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test shared_checkout_c006
```

The hosted native-containment lane (`linux-m004-native.yml`, job `process-ipc`) is the evidence for the containment tiers. If the grant's construction changes at all — including a rename — the lane must be re-run, and the result recorded as hosted evidence rather than inferred.

**Never** run `--all-features` for a workspace sweep.

## 12. Documentation updates

- `plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md` — the `## Amendment:` section of §6.1.
- `src/git_contained_worktree.rs` — the two comments in §6.1.
- `tests/contained_worktree_c008.rs` — the test's name and the comment in §6.1.
- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md` — the milestone table gains C009; the status line records it.
- `architecture/` — if any architecture document names the grant, it must name the shared administration root. The architecture documents are authoritative where `docs/` differs, so both must be checked.
- `plans/closure/linux-trusted-git-cost-corrective/008-status.md` — **read-only**. C008's closure recorded the grant's shape; if it names the wrong path, the correction belongs in this milestone's closure record, not by editing C008's record.

## 13. Acceptance criteria

1. ADR-0048, both code comments, the test's name and the test's comment all name `<common-gitdir>/worktrees/` as the **shared** administration root, and none describes it as the linked worktree's own administration directory.
2. The ADR's amendment records that the child can write another linked worktree's `HEAD`, and records the maintainer's disposition of that consequence.
3. An obtained-root set containing a prefix-sharing path but not the required path **fails** the obtained-root check.
4. A static guard asserts the grant/ADR agreement and rejects an injected divergence in either direction.
5. Every defect above has a regression test that fails against the baseline and passes after the fix.
6. The five existing `contained_worktree_c008` tests remain green, and `verify.sh quick` and `cargo clippy --workspace --all-targets -- -D warnings` are green.

## 14. Stop conditions

Report rather than improvise if:

- **The maintainer has not chosen a disposition for the "another worktree's `HEAD` is writable" consequence.** The amendment must then record it as an open question and this milestone must stop after WP1's documentation correction, rather than recording an acceptance that was not made. Narrowing the grant is a **new ADR** and is out of scope. *(This condition was met: the milestone stopped at WP1. The maintainer chose the disposition afterwards — **defence in depth**, a pinned `core.hooksPath` — and it is recorded in the closure record's §14 (A14) and in ADR-0048.)*
- **The obtained-root check's precision change causes a backend that previously passed to fail.** That backend was not obtaining the required root; report it rather than loosening the check.
- **Renaming the test changes a filter used by CI or by another lane's path filter.** Report the coupling before renaming; the hosted lane's path filters are a real dependency.
- **The guard cannot determine the ADR's named path** from the ADR's text. Report it — the guard must compare against the ADR, not against a copy of the path.
- **Any fix would weaken an invariant in §4**, in particular invariant 3 or invariant 6.

## 15. Closure evidence required

1. A closure record at `plans/closure/linux-trusted-git-cost-corrective/009-status.md`, with the sixteen-section structure the closure template requires.
2. A requirement-to-evidence matrix mapping each of §13's six criteria to the test or command that demonstrates it.
3. The exact verification commands and their results, labelled honestly as local or hosted. If the grant's construction changed at all, the hosted `linux-m004-native.yml` `process-ipc` run must be quoted with its run identifier.
4. The ADR amendment, quoted in the closure record, with the maintainer's disposition of the recorded consequence.
5. An invariant review confirming §4's eight invariants are intact.
6. A registry update in the same commit as the closure.
7. A disposition for C008's closure record if it named the wrong path — recorded here, not by editing C008's immutable record.

## 16. Handoff notes

- The two defects were found by a 2026-10-23 review of `origin/main` at `f1b83a7a7`, by clause-by-clause reading of ADR-0048 against the implementation.
- The tree at `f1b83a7a7` is **green**: `codegg --test contained_worktree_c008` (5) and `codegg --test shared_checkout_c006` (16) both pass. These defects are latent, not caught.
- This milestone is **documentation-and-guard work against an already-accepted contract**. It does not change what the child can write. The consequence it records is real and is the thing a maintainer should look at first.
- The Linux sandbox admission line's C005 owns the hostile-config guard and the Git consistency contract. The two milestones are independent and must not be folded together, even though both touch the Git route.
- C008 of this line is closed and its hosted containment evidence stands.
