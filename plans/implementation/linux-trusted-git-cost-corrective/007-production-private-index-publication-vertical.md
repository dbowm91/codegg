# C007 — Production Private Index Publication Vertical

Status: planning — implementation not started

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`

Related closure records:

- `plans/closure/linux-trusted-git-cost-corrective/004-status.md` (the test-only prototype this vertical productionises)
- `plans/closure/linux-trusted-git-cost-corrective/006-status.md` (the ADR-0044 live-read consistency review)

Related ADR: `plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md` — **proposed, not accepted**.

Repository baseline reviewed: `6efd5ebdf90f91fc24dcac7f2827f4ca9e355135`

## 1. Scope — one narrow production vertical

C004 closed a **test-only** prototype: a typed `Add` operation staged through
`GitExecutionService` and `ManagedProcessService` while the repository's own
index and objects stayed unchanged. What it did not establish is a production
mutation transaction.

This corrective takes **one** operation from end to end in production:

```text
authorized git add  →  private staging  →  conflict-safe index publication  →  crash recovery
```

and proves it completely, on the **immutable** trusted-read route.

Everything else stays out of scope on purpose. Merge, rebase and checkout are
**not** extended here. The C004 prototype's own advisory was that extending
test-only transaction machinery to those operations prematurely would be worse
than proving one vertical completely, because the hard part of a Git mutation
is not the staging — it is the conflict detection and the recovery. Those are
exactly the parts that a partial, test-only extension leaves unproven. Proving
one operation completely establishes the machinery those other operations would
reuse; a half-proven machinery across three operations establishes nothing.

### 1.1 Why the immutable route, not the live route

ADR-0044 clause 1 proposes a distinct consistency class for live-object reads
and requires that such a result be inadmissible to consumers that need
generation consistency. The publisher is precisely such a consumer: it must
know that the state it observed is the state it publishes against. The C006
review reaches the same conclusion — qualify the publisher against immutable
reads first, so that routing it through a live read is never tempting.

This vertical therefore **depends on nothing in ADR-0044** and does not require
its acceptance. It runs on the existing immutable trusted-read route.

### 1.2 Relationship to M005

The publisher runs the staging child through `ManagedProcessService`, so the
M004 process/IPC tier and the M005 offline boundary apply to it. M005 remains
active and unqualified, so the publisher's own qualification must record which
network boundary it actually obtained, and must not claim an M005
`NetworkEnforcement::Enforced` value — no such producer exists. C014's
recorded finding applies here: the obtained network fact is not reported
anywhere, so the publisher's receipt has to say so rather than imply it.

> **Update 2026-10-19.** The obtained-facts gap this plan was written against is
> closed by
> `plans/implementation/linux-execution-isolation/023-m005-obtained-network-fact-contract.md`
> (closure `plans/closure/linux-execution-isolation/023-status.md`):
> `NetworkEnforcement::Enforced` now has a producer, and the publisher's receipt
> states the boundary its staging actually obtained, read off the launch rather
> than off the request. The passage above is preserved as the plan's own
> constraint at the time it was written.

## 2. What already exists

| Component | Location | State |
|---|---|---|
| Typed `GitOperation` | `crates/codegg-git` / `src/git_mutations.rs` | `Add` exists; the executor has **no `Add` arm** |
| `GitMutationExecutor` | `src/git_mutations.rs` | routes typed mutations; `Add` is not routed |
| `DestructiveRepositoryChange` | `crates/codegg-core/src/approval.rs` | present; `Add` is not destructive and must not require it |
| `GitExecutionService` | `src/git_service.rs` | has `execute_private_git_add_staging_test_only`, feature-gated and test-only |
| `GitEnvPolicy` / `prepare_private_object_view` | `crates/egggit/src/process.rs` | the immutable object view; strips `GIT_INDEX_FILE` and `GIT_ALTERNATE_OBJECT_DIRECTORIES`, denies `--git-dir`/`--work-tree` |
| The publisher | `tests/linux_m004_process_ipc.rs` | **entirely test-local** — the logic this vertical must productionise |
| Durable journal | — | **none exists**; `STORAGE_LAYOUT_VERSION` is 83, so the next migration is 84 |
| Restart reconciler | — | **none exists** |
| External-writer exclusion | — | **none exists**; `WorkspaceLockTable` is daemon-owned and cannot fence an external `git` process |

## 3. The four elements

### 3.1 Authorized `git add`

An ordinary dispatch caller, not a test-only one.

- `GitMutationExecutor::execute` gains an `Add` arm that routes to the
  publisher service.
- Authorization: `Add` is a **workspace-scoped, non-destructive** mutation. It
  must not require `DestructiveRepositoryChange`. It must require the
  workspace lease and the daemon-owned mutation authority that the existing
  executor already requires, and it must validate that every path in
  `GitOperation::Add { paths }` is inside the selected worktree, using the
  existing path validation rather than a new one.
- `filter.*` must be disabled for the staging child. `GitEnvPolicy` does not
  currently disable it, and a clean/smudge filter is repository-controlled
  code that would run inside the publisher's authority. This is the same gap
  the ADR-0044 review records for reads, and it is *more* acute for a write
  path.

### 3.2 Private staging

Reuse the existing immutable object view and add a private index.

- The staging child runs with `GIT_INDEX_FILE` pointing at a private index in
  daemon-owned scratch, and the object view from `prepare_private_object_view`.
- The child is the daemon-pinned Git executable, launched through
  `ManagedProcessService` with the M004 process/IPC tier, so the repository
  cannot influence what the child executes.
- The staging child must not be able to reach the real index. `GitEnvPolicy`
  already strips an inherited `GIT_INDEX_FILE`; the publisher must set it
  explicitly *after* that strip, and the assertion is that the real index file
  is byte-identical before and after staging.

### 3.3 Conflict-safe index publication

This is the part the test-only prototype models and production must own.

- **Compare-and-swap on the real index.** Before publishing, the publisher
  records the identity of the real index it based its decision on (content
  hash plus mtime/size, or a content hash alone). At publication time it
  re-reads the real index; if the identity changed, the publication is
  **rejected as stale** and the operation fails with a typed conflict. A
  stale publication must never overwrite a concurrent writer's index.
- **Git-compatible locking.** The publisher takes Git's own `index.lock` in
  the real gitdir, so a concurrent official Git process contends with it
  rather than racing it. The publisher must create the lock the way Git does,
  and must release it on every exit path.
- **Atomic replacement.** The private index is moved into place only after the
  lock is held and the CAS succeeds. The move itself must be atomic within
  the gitdir filesystem.
- **External writers are not excluded.** The publisher cannot fence an
  external `git` process. The CAS is what makes that safe: a concurrent
  external write is detected and refused, not silently overwritten. This is
  the property the C012 closure and ADR-0044 clause 5 both insist on, and it
  must be stated in the receipt rather than implied.
- **A conflict is a typed result, not a crash.** A stale index, a held
  `index.lock`, or a changed worktree geometry must each produce a distinct
  typed outcome, so the caller can retry deliberately.

### 3.4 Crash recovery

- A **durable journal** records the intent to publish before the publication
  begins: the operation id, the workspace, the private index path, the
  observed real-index identity, and the state. A migration is required
  (`STORAGE_LAYOUT_VERSION` 83 → 84) plus the project-catalog invariant check
  (`scripts/check_project_catalog_invariants.py`).
- On daemon start, a **reconciler** inspects every journal entry that is not
  terminal and resolves it:
  - the publication completed → the entry is closed, and the receipt records
    the outcome;
  - the publication did not start → the entry is aborted and the private
    index is discarded;
  - the publication started but did not finish → the real index's identity is
    compared against the journal's recorded pre-publication identity:
    - unchanged → the real index was never replaced, so the operation is
      aborted and the private index discarded;
    - changed → the identity is compared against the *published* index's
      identity, which the journal also records; if it matches the published
      one, the publication succeeded and the entry is closed as committed; if
      it matches neither, the state is **ambiguous** and the operation is
      failed with a typed "unknown outcome" result and the private index is
      retained for inspection. Ambiguity must never be reported as success.
- Recovery must be **idempotent** and must never replay a committed
  publication.

## 4. Non-goals

- Merge, rebase, checkout, reset, clean, fetch, push, pull — all deferred.
- Any live-ODB read route. This vertical runs on the immutable route.
- Any change to ADR-0044's status, and no reliance on ADR-0044's acceptance.
- Any production `NetworkEnforcement::Enforced` claim.
- Multi-resource atomicity across index, refs and worktree. A single `Add`
  publishes **one** resource — the index — and the receipt must say so. The
  multi-resource transaction remains a separate, larger problem; pretending a
  single-resource vertical proves it would repeat the failure mode this
  corrective exists to avoid.
- Any change to the C004 test-only prototype's behaviour. It stays as the
  oracle the production path is compared against.

## 5. Implementation order

1. **Durable journal** — the migration, the store, and the reconciler, with
   no publisher attached. A journal that can be written, read and reconciled
   is testable before there is anything to publish.
2. **Private index staging** — a production staging service that produces a
   private index and proves the real index is untouched.
3. **CAS + lock + atomic publication** — the publisher, with a typed conflict
   result.
4. **Recovery wiring** — the reconciler resolves the states above, and the
   crash-recovery test kills the daemon at each defined point.
5. **The ordinary dispatch caller** — `GitMutationExecutor` gains the `Add`
   arm, and the test-only method is retired or reduced to a test that
   exercises the production path.

Each step is independently testable, and each step's failure is a typed
result rather than a crash.

## 6. Required proof

- An **authorized** `Add` stages and publishes; an unauthorized one is refused
  **before** any path is accessed and before any child is spawned.
- A non-`Add` mutation is still refused by the same preflight the test-only
  path uses.
- The real index and the repository's objects are byte-identical before and
  after a **failed** staging.
- A **concurrent external writer** between staging and publication produces a
  typed conflict; the external writer's index is not overwritten; the
  publication is not applied.
- A concurrent official Git process holding `index.lock` produces a typed
  conflict, not a crash and not a silent retry.
- **Crash recovery**: killing the daemon before the publication, after the
  lock is taken, and after the move but before the journal is closed yields
  exactly the three reconciled outcomes above — committed, aborted, or
  ambiguous — and never a silent wrong answer.
- Recovery is idempotent across repeated restarts.
- The publisher executes **no** hook, filter, helper or worktree-controlled
  code with its own authority. `filter.*` disabled; hooks are not run by the
  publisher; the staging child's configuration is pinned by the daemon.
- Every receipt states the obtained network boundary truthfully, including
  "not reported" where the M005 obtained fact is unavailable.

## 7. Dependencies and gates

- **Not** blocked by ADR-0044. This vertical runs on the immutable route and
  is consistent with ADR-0044 as proposed.
- Requires the M004 process/IPC tier (closed) for the staging child.
- Does **not** require M005 production qualification, but must not *claim* an
  M005 boundary it cannot report. C014's open items apply: the obtained
  network fact is not reported anywhere in the system.
- Does **not** change C010 no-admission or M003 blocked status.
- C005 remains blocked until this vertical has a production publisher and
  recovery proof.

## 8. Stop conditions

- If the CAS cannot be made reliable against an external writer on the
  supported filesystems, the vertical closes **negative** and the immutable
  snapshot route stays canonical. A publisher that can silently overwrite a
  concurrent writer's index is worse than no publisher.
- If crash recovery cannot distinguish committed from ambiguous, the vertical
  closes negative for the same reason. "Possibly published" must never be
  reported as "published".
- If the real index cannot be published atomically within the gitdir
  filesystem, the vertical closes negative.
- No stop condition is waived by a test that is skipped, feature-gated off, or
  run only against a filter that matches zero tests — the failure mode C014
  found in the hosted M004/M005 job.

## 9. Implementation decisions that differ from the plan text

Three points where the implementation departs from the wording above. Each is
recorded here rather than silently applied, because the plan text is the
commitment the closure is measured against.

### 9.1 `GIT_INDEX_FILE` is not used

Section 3.2 says the staging child runs with `GIT_INDEX_FILE` pointing at a
private index, set explicitly *after* `GitEnvPolicy` strips it.

`EnvironmentPolicy::with_var` **refuses** to set a denied variable, and
`GIT_INDEX_FILE` is in `codegg_git::ALWAYS_STRIPPED_ENV_VARS`. Setting it
would therefore require weakening a deliberate deny rule — a deny rule that
exists precisely so no inherited value can redirect a Git child at a
different index.

The property the requirement protects is obtained without it: the staging
child runs with `--git-dir=<private gitdir>`, so its index is the private
gitdir's own index in daemon-owned scratch, and the repository's real index
is unreachable by it. The mount view additionally makes the repository's
Git metadata read-only to the child, so the child could not reach the real
index even if it tried.

The plan's *assertion* — the real index is byte-identical before and after
staging — is implemented as a publisher check and a test.

### 9.2 The object import is performed by the daemon, not by a child

Section 3.2's private object view puts the staged blob in a private object
database. Publishing the index then requires that blob to reach the
repository's object database.

A child **cannot** do that: the M004/M005 mount view protects the
repository's Git metadata — the whole `.git` directory, object database
included — as read-only to children. That protection is a deliberate
invariant, and a containment profile that let a child write the repository's
object database would contradict it.

The import is therefore performed by the **daemon**, which already owns the
repository (it performs the index replacement itself). The import is
content-addressed and therefore idempotent: an object is copied under its own
content hash, an object already present is left alone, and the copy is
verified byte-for-byte against its source. It cannot corrupt an existing
object, because an existing object with the same name has the same content.

The import runs **under the index lock and after the compare-and-swap
succeeds**, so a conflicting publication writes nothing at all.

### 9.3 The staging step is a trait

The publication half of the vertical — the journal, the compare-and-swap, the
Git index lock, the object import, the atomic replacement, and the crash
recovery — must be provable on a host that cannot provide the M004/M005
tier, because that is the only host a developer has.

`GitIndexStaging` is therefore the trust boundary: production uses
`ContainedGitIndexStaging` (the daemon-pinned Git executable through
`ManagedProcessService` with M004/M005), and a test supplies its own
implementation. Everything above the boundary is the same code in both
cases, so a proof obtained through the test implementation is a proof about
the production publication.

The contained staging step itself remains proven by the hosted Linux runner.
