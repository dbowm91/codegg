# C008 — Contained Managed Worktree Creation — Status

Status: **closed** — the grant, the seam and the filesystem containment are
proven locally, and the **hosted** M004 lane obtained the production tiers this
workstation cannot (Landlock ABI 6 needs Linux 6.12+; this host is 6.8). The
`linux-m004-native.yml` `process-ipc` job passed on the corrected head
`383b9ca19` (run `39829806588`, step "C008 contained managed-worktree creation
under containment"), so the obtained enforcement is no longer a pending claim.

Plan: `plans/implementation/linux-trusted-git-cost-corrective/008-contained-worktree-creation.md`
Authority: **ADR-0048** (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`),
**accepted 2026-10-22** and **amended 2026-10-22**.

## 1. Executive finding

`WorktreeService::create_reserved` used to fail closed on every host: it
reaches `git worktree add`, `egggit::process::classify_git_request` classifies
that as requiring `CleanSmudgeFilter` containment, and `GitEnvPolicy` maps every
containment-requiring request to `BackendUnavailable`. C007 proved the
*staging* case; the *worktree* case was unbuilt.

C008 builds it. `src/git_contained_worktree.rs` runs the daemon-pinned Git
executable through the canonical managed-process owner with a sandbox
description that grants the child **exactly three writable roots**: the new
checkout, the daemon scratch, and `<common-gitdir>/worktrees/` — the linked
worktree's own administration directory. The repository's Git directory is
readable and immutable to the child, `refs` included, so the child cannot
create, move or repoint a branch.

Two findings changed the design, and both are recorded rather than papered
over:

- **The operation is two steps, not one.** Measured on Git 2.43.0 and 2.55.0:
  `git worktree add <path> <existing-branch>` **locks and rewrites**
  `refs/heads/<branch>` even though the value is unchanged. The attached form
  therefore cannot run inside this grant at all — it fails with `cannot lock
  ref 'HEAD'`. ADR-0048's Fact 2 was wrong on this point (it measured the
  ref's *value*, not whether Git writes it), and the ADR was **amended** rather
  than the implementation improvised. The operation is now
  `git worktree add --detach <path> <commit>` followed by
  `git symbolic-ref HEAD refs/heads/<branch>`, both inside the same
  containment. The child writes **no ref at all**, which is stronger than the
  original clause 2 required.
- **The grant is per step.** The protected mount view *follows* an existing
  `.git` pointer and mounts the administration directory it names read-only.
  A launch that granted the checkout as a write root would therefore find the
  administration directory read-only under the child — a silent narrowing of
  the grant. The attach step does not need the checkout, so it is granted a
  **narrower** set: the scratch and the administration directory only.

## 2. Requirement-to-evidence matrix

Local evidence is from this workstation (Linux 6.8, Git 2.43.0). Hosted
evidence is from the `linux-m004-native.yml` `process-ipc` job.

| # | Acceptance criterion | Test | Local | Hosted |
|---|---|---|---|---|
| 1 | The writable set is exactly {checkout, scratch, `<common-gitdir>/worktrees/`}; the Git directory is read-only; nothing under it is writable except the administration root; the source working tree is not writable | `the_grant_names_only_the_administration_directory` | **pass** | pass (same test) |
| 2 | The `.git` pointer exemption names only the top-level `.git` of a granted write root; a pointer beyond a write root and a nested `.git` are refused when the launch description is built | `the_grant_names_only_the_administration_directory` | **pass** | pass (same test) |
| 3 | The argv is one operation, in two steps, and never names a ref or creates a branch | `the_grant_names_only_the_administration_directory` | **pass** | pass (same test) |
| 4 | A configured creator is authoritative: a real linked worktree on the recorded branch; a refusing creator orphans the reservation with `GitError` health; a nested repository is rejected; a service without a creator fails closed with the Git policy's own typed refusal | `the_seam_creates_a_real_checkout_and_fails_closed` | **pass** | pass (same test) |
| 5 | The creator resolves the paths it creates itself and reaches the launch; a host that cannot obtain the tiers fails at the **containment**, not at the path | `the_creator_reaches_the_launch_instead_of_failing_on_its_own_paths` | **pass** | pass (same test) |
| 6 | A hostile smudge filter and a hostile `post-checkout` hook both run and both fail to reach the host; the worktree is still created; the branch is unmoved | `the_contained_child_cannot_write_outside_its_grant` | **pass** (filesystem grant) | **pass** (production tiers) |
| 7 | Every other containment-requiring Git request keeps its classification | `the_git_execution_policy_is_unchanged` | **pass** | pass (same test) |
| 8 | The production launch obtains the M004 process/IPC tier and the M005 offline network | `the_contained_child_cannot_write_outside_its_grant` | **not obtainable here** | **pass** (run `39829806588`) |

Criterion 6 is the containment claim. Criterion 8 is the only part this
workstation cannot reach, and it is the part that needs Landlock ABI 6 — which
is exactly why it is a hosted claim and why the hosted result settles it.

## 3. The hostile-configuration evidence

The fixture is a repository whose **own configuration** is hostile:

- `filter.hostile.smudge` — a script, committed through `.gitattributes` so
  that Git actually applies it to a checked-out `*.txt` file. The script writes
  its positive control into the daemon scratch (inside the grant) and then
  tries to write `refs/heads/hostile-filter` (outside the grant).
- `core.hooksPath` — a **directory** inside the daemon scratch holding an
  executable `post-checkout`, because `core.hooksPath` names a directory. The
  hook writes its positive control into the scratch and then tries to write
  `refs/heads/hostile-hook`.

The observed write set, from the child's own stderr:

```text
Preparing worktree (detached HEAD 822c9f2)
scratch/hostile-filter.sh: 3: cannot create .../repo/.git/refs/heads/hostile-filter: Read-only file system
scratch/hooks/post-checkout: 3: cannot create .../repo/.git/refs/heads/hostile-hook: Read-only file system
```

Both hostile surfaces **ran** — that is what the positive controls prove, and
without them the negative assertions would be vacuous — and both were refused
with `EROFS`. Neither marker exists on the host. The branch list after the run
is exactly the daemon's branch and `main`; the daemon's branch still points at
the commit the daemon put it at.

The refused writes are tolerated by the scripts (`|| true`) so that the
*operation* completes and the checkout is still created. The claim under test
is that the escape does not reach the host, not that the hostile code aborts
the checkout.

## 4. The obtained-enforcement evidence

The grant is asserted against the **built** launch description, not against the
configuration that was requested: the write roots, the read-only Git
directory, the pointer exemption and the tier flags are all checked on the
spec the child runs with.

The **obtained** enforcement is read from the launch's own result. The
outcome's `guarantees` carry the scope the launch actually obtained, and the
creator requires the administration directory to be in the writable scope it
obtained — for the creation step and for the attach step. A launch that
reports a request but not an obtained fact is a failure.

**The production tiers themselves are not locally obtainable.** This host's
kernel is 6.8, so Landlock offers ABI 4, and the M004 process/IPC tier needs
ABI 6 (Linux 6.12+):

```text
Landlock ABI 6 scope request failed: fully incompatible access-rights: AbstractUnixSocket | Signal
```

So the local native proof ran with the **filesystem grant alone** — the
protected mount view, which is the dimension this milestone's grant is about.
The test says so on stderr when it does this, and it requests the **production**
tiers automatically on a host that can obtain them.

**The hosted lane obtained them.** `linux-m004-native.yml`'s `process-ipc` job
passed on the corrected head `383b9ca19` (run `39829806588`), and its step
"C008 contained managed-worktree creation under containment" is a pass. The
job's own step list records the tiers it enabled — "Enable user namespaces for
isolated native qualification", "Build trusted sandbox helper" — before that
step ran, so the hosted result is the **obtained** enforcement and not a
rerun of the filesystem-only proof. The sibling job
`restricted-userns-fails-closed` also passed, so the qualification keeps its
negative arm.

## 5. Invariant review

- The child writes **no ref**. The branch is created by the daemon before the
  child runs, through the existing typed `git branch` path.
- `GitEnvPolicy`'s classification is unchanged. `symbolic-ref` is not run on
  the ordinary process boundary; it runs inside the containment, where the
  daemon's argv is the authority.
- No other Git operation is affected: `git add`, `git commit`, `git merge`,
  `git cherry-pick`, `git rebase`, `git stash`, `git am`, `git bisect`,
  `git push` and `git pull` keep their current classification, and
  `git worktree add` keeps its own.
- No new module, no new `GitOperation` variant, no schema migration, no new
  tool, no new event variant, no new guard script.
- ADR-0044 stays **proposed** and is not superseded. This milestone grants
  neither the mutable object-read nor the metadata-promotion class.

## 6. Failure and recovery review

- A creator that fails orphans the reservation and records `GitError` health.
  No half-made checkout is reported as created.
- A creator that plants a nested repository instead of a pointer file is
  rejected by the post-create verification: the result must be a **file** whose
  contents name the expected administration directory.
- A host that cannot obtain the tiers refuses with a typed reason. There is no
  uncontained fallback and no downgrade.

## 7. Stop conditions

The plan's stop condition — *"`git worktree add` proves to write a ref under
the grant on the pinned Git version. **Stop** and report the observed write
set."* — **fired on 2026-10-22**. The observed write set was
`<common-gitdir>/refs/heads/<branch>.lock` and `<common-gitdir>/refs/heads/<branch>`.

The plan's second stop condition — *"ADR-0048's clause 1 turns out to authorize
something this implementation needs beyond the single operation shape. **Stop**
and propose an ADR amendment rather than widening the implementation."* — also
fired, and was answered with ADR-0048's amendment. The writable set did not
widen and no clause was weakened.

## 8. Verification executed

Local, this workstation:

```
cargo fmt --all -- --check                                              pass
cargo clippy --workspace --all-targets --locked -- -D warnings          clean
python3 scripts/check_execution_ownership.py                             ok
python3 scripts/check_adr_integrity.py                                  ok
bash scripts/check-core-boundary.sh                                     ok
python3 scripts/check_scheduler_bypass.py                               ok
python3 scripts/check_queue_delivery_callsites.py                      ok
python3 scripts/check_idle_continuation_callsites.py                    ok
python3 scripts/check_queue_write_transaction_shape.py                  ok
python3 scripts/check_daemon_cwd_usage.py                              ok
python3 scripts/check_git_forbidden_patterns.py                        ok
python3 scripts/check_no_hardcoded_models.py                            ok
python3 scripts/check_sandbox_contract.py                              ok
python3 scripts/check_tui_transport_authority.py                       ok
cargo test -p codegg-core --lib worktree                                 12 passed, 0 failed
cargo test -p codegg --test contained_worktree_c008                      5 passed, 0 failed
CODEGG_RUN_M005_OFFLINE_NATIVE=1 cargo test -p codegg --test contained_worktree_c008
                                                                     5 passed, 0 failed
cargo test -p codegg --test contained_worktree_c008 --test sandbox_landlock \
  --test sandbox_containment --test linux_bubblewrap_first_hop \
  --test git_index_publication --test worktree --test session_launch_service \
  --test shared_checkout_c006                                           87 passed, 0 failed
bash scripts/verify.sh quick                                            "Quick verification passed."
```

The eight regression binaries are the ones the change could plausibly move:
the sandbox's own suites, the publication suite, the worktree and
session-launch suites, and C006's shared-checkout carrier. The C006 carrier
still passes **unchanged**, which is the point: M011 will add the composed race
beside it, not inside it.

The native run's stderr records the tier it could obtain:

```text
note: this host cannot obtain the M004 process/IPC tier (Landlock ABI 6); proving the filesystem grant alone
```

Hosted: the `linux-m004-native.yml` `process-ipc` job carries a dedicated
"C008 contained managed-worktree creation under containment" step with its own
ten-minute budget, and the C008 paths are in both the `pull_request` and the
`push` path filters. It **passed** on the corrected head `383b9ca19`:

```
run 39829806588 — Linux M004 process and IPC qualification — success
  job process-ipc — success
    C008 contained managed-worktree creation under containment — success
  job restricted-userns-fails-closed — success
```

Every other lane on the same head also passed: `CI` (the workspace suite),
`Linux C006 protected mount qualification`, `Windows native probe contract`,
`Windows process launcher` and `Shared checkout macOS qualification`. The
Windows lanes are the repository's own contract checks; they are **not**
Windows evidence for this milestone, and none is claimed.

## 9. Non-claims

- **No Windows evidence, and none is claimed.** Full Windows support is not a
  target on this line. The platform gate disables Windows with a named reason,
  which is what discharges M005's Q4.
- **No claim that a hostile configuration was neutralized.** It is contained by
  the sandbox. The repository's configuration is left in place deliberately.
- **No claim that a hostile filter's *output* is safe.** The checkout's
  contents are untrusted content.
- **No claim about any other Git operation.** No mutable object reads, no
  metadata promotion, no remote transport, no credential mediation.
- **No claim that the M004/M005 tiers were obtained locally.** They were not
  obtainable here, and that is stated rather than papered over. The tiers were
  obtained on the hosted M004 lane, and that is recorded as a **hosted** result
  in §2 and §8, not as a local one.
- **No claim that the whole `create` path is contained.** Only
  `create_reserved` / `create_reserved_contained`, the managed linked-worktree
  path.

## 10. The unblock audit

C008 unblocked the blackboard line's composed two-agent daemon race, which is
**M011** in that line, written against C008's shipped API. M005's only
remaining non-claim was that race, and **M011 closed it on 2026-10-22**
(`plans/closure/agent-workstream-blackboard/011-status.md`), so M005 closed
strictly.

M011 also found a production defect this milestone's scope did not cover: every
mutating tool's writer-permit idempotency key was over the permit store's
128-byte bound, so every write on a shared checkout was refused. The fix is
recorded in M011's closure and in `architecture/writer_permit.md`.

Nothing else became newly eligible. C005's parity/benchmark/admission scope is
untouched and stays blocked on its own named blockers.
