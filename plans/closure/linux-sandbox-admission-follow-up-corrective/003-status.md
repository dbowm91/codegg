# Linux Sandbox Admission Follow-up Corrective — Milestone C003 — Closure Status

Status: **conditionally closed** — the slice's own exit criteria are met; the condition is a maintainer-owned decision that C003 cannot make
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/003-git-live-read-consistency-and-metadata-review.md`
Source subsystem roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` (C003)
Repository baseline: `f4019a410` (`main` at the start of the milestone), merged forward with `3ee32e839` (`main`) when the branch was prepared for review
Landed: `f1b83a7a7` (merge commit on `main`, 2026-10-23); the landing was repaired by `a3b6116e8` (PR #173) — see §8.5 and §12.5
Branch: `plans/linux-sandbox-c003-git-consistency-metadata-review`
ADR packet: `plans/closure/linux-sandbox-admission-follow-up-corrective/003-adr-0044-packet.md`
Predecessor review: `plans/closure/linux-trusted-git-cost-corrective/006-status.md` §8 (the blocking items this milestone answers)

## 1. Executive finding

C003 was registered as "ready for non-promoting interface/review only". It is
now **conditionally closed**: everything the plan put inside C003's own scope
is delivered and evidenced, and the one thing it deliberately does not do —
accept ADR-0044 — is recorded as a maintainer-owned condition rather than
papered over.

The milestone closed five gaps, none of them by relaxing anything:

1. **There was no typed consistency class.** The prior ADR review's blocking
   item was exactly this: *"No typed consistency class exists in any DTO or
   result type."* A Git read could be served from the private object view's
   cache, from a fresh preparation, or — on the test-only contained live
   route — from the original object database of a concurrently mutable
   repository, and **nothing in the type system told a consumer which**. The
   class now exists, is carried in the read result, and is checked.
2. **A live read was not inadmissible to a generation-bound consumer.** The
   review asked for *"a type-level or explicit admissibility check, not a
   convention"*, and for the live class to be *"a capability, not a label"*,
   gated the way `DestructiveRepositoryChange` gates a destructive mutation.
   Both now exist, and the capability **cannot be obtained in a production
   build**.
3. **The publication boundary did not record what it published from.** The
   C007 publisher compare-and-swaps against the fingerprint of the index
   generation it captured, but the receipt did not say which class that
   generation was, and a staging that had read the live index would have been
   published anyway. It is now recorded and refused.
4. **`filter.*` and `include.path` were not mediated.** The review found that
   a repository could cause code execution during an ordinary **read**. This
   was not a reading of the code — it was **reproduced**: `git diff`, on the
   trusted read route, executed a filter command declared only in a file
   reached through `include.path`. That gap is closed.
5. **Three C007 publication invariants were claimed but untested.** They now
   have regression tests, including a host-positive control for the filter
   case.

## 2. The plan's six packages, and where each landed

| # | Package | Where | State |
|---|---|---|---|
| 1 | Inventory read consumers and guarantees | §3.1 below, and the ADR packet §4 | delivered |
| 2 | Typed consistency contract and consumer fence | §3.2, §3.3 | delivered |
| 3 | Hostile-metadata probes | §4 | delivered, with two stated residuals |
| 4 | C007 publication invariant regressions | §5 | delivered |
| 5 | ADR-0044 maintainer packet | §6, and `003-adr-0044-packet.md` | delivered |
| 6 | Accurate docs and guards | §7 | delivered |

## 3. The typed consistency contract

### 3.1 The read-consumer inventory

The plan's first package was an inventory, because the fence can only be
placed where the consumers are. The inventory is recorded in the ADR packet
§4. Its two findings shape everything else:

* **The private object view covers the object database only.** Refs, the
  index and the worktree are always read live. So the class describes the
  **object generation** a read observed, and says nothing about the other
  surfaces. A read that consults no object store therefore reports
  `NoObjectGeneration` rather than borrowing a class it did not earn.
* **There is no typed merge-base consumer.** `merge-base` appears in the
  trusted route's read-objects allowlist, but the repository has no typed
  merge-base function and no consumer. The plan names merge-base as a
  generation-bound consumer; the honest inventory records that there is
  nothing to fence yet, rather than inventing one.

### 3.2 The class

`crates/egggit/src/consistency.rs` is new and owns the vocabulary:

| Item | Purpose |
|---|---|
| `GitReadConsistency` | the closed class: `ImmutableGeneration`, `CapturedNotCurrent`, `LiveMutable`, `NoObjectGeneration` |
| `GitReadConsistency::weaker` | the composite rule: a read assembled from several Git invocations carries the **weakest** class among them |
| `GitReadFact<T>` | a read result **together with** the class it was obtained under |
| `GitConsistencyRefusal` | a typed refusal with a stable machine name |
| `GenerationConsistent<T>` | the only shape a generation-bound consumer may accept |
| `admit_generation_consistent` | the explicit admissibility check |
| `LiveReadCapability` | the authority a consumer must **hold** to use a live-class read |

Three properties of this design are worth stating, because each one is a
place where the obvious alternative was wrong:

* **The class is carried, not announced.** `GitReadFact<T>` holds the value
  and the class together. A consumer cannot receive the bytes and forget
  what it was allowed to assume about them.
* **The refusal is a value, not a warning.** A live-class read handed to a
  generation-bound consumer returns `GitConsistencyRefusal::LiveClassWithoutCapability`.
  There is no "advisory" mode that lets a caller proceed anyway.
* **The capability is not mintable in a product build.** `LiveReadCapability`'s
  only constructor is `#[cfg(feature = "git-live-read-test-support")]`, a
  feature the shipped binary does not enable. This is the reason the fence
  cannot be relaxed by accident: there is nothing to relax.

`egggit` gained its **first** optional feature for this. The crate's manifest
previously stated that it defined no optional features; that statement is now
false and the manifest says so, with the reason.

### 3.3 Where the class is produced and consumed

**Produced.** `crates/egggit/src/process.rs` carries the class from the
private object view into the command and out with the result:

* `PrivateObjectView` records the class of the bytes it serves. A view
  materialized from a fresh, stability-checked copy of the object store
  reports `ImmutableGeneration`; a view **reused from the preparation cache**
  reports `CapturedNotCurrent`.
* `GitAsyncCommand::read_consistency` and `GitSyncCommand::read_consistency`
  expose it.
* `GitCommandOutput` — which replaces the raw `std::process::Output` the
  route returned — carries it, so the class travels with the bytes.

The cache case is the one that matters. The prior review recorded that the
4-entry/2-GiB preparation cache is *"a best-effort mechanism, not a
durability boundary"*, which means a cache-served read is **not** an
immutable current generation. Reporting it as one would be the exact
overclaim the review objected to. It now reports `CapturedNotCurrent`, and
`CapturedNotCurrent` is still admissible to a generation-bound consumer —
because what such a consumer needs is the *internal consistency* of the
bytes, and currency is a separate question that the publisher's
compare-and-swap answers.

**Carried.** Eighteen typed read functions in `egggit` now return
`GitReadFact<T>` rather than the bare value: `repo_status`,
`rich_repo_status`, `diff_summary`, `diff_text`, `changed_files`,
`file_diff`, `log_commits`, `blame_file`, `resolve_commit`,
`resolve_base_ref`, `list_branches`, `list_tags`, `list_remotes`,
`list_worktrees`, `capture_git_source_subject`,
`capture_git_source_subject_excluding`, `detect_operation_state_for_root`,
and `detect_repository_operation_state`.

**Consumed.** The root crate's fence lives in `src/git_consistency.rs`, which
re-exports the vocabulary and adds `admit_generation_bound` — the fence that
maps a refusal onto the typed `GitServiceError::Consistency` variant, so a
caller that ignores the fence still cannot turn a refusal into a payload.
`src/git_service.rs` stamps the class on `GitExecutionResult`, and the diff
path — a generation-bound consumer — goes through the fence before it
returns a payload.

**The live route.** The test-only contained live-read pilot now returns a
`GitReadFact` with the class `LiveMutable`. That is the truthful class: the
route reads the **original** object database of a repository that is
concurrently mutable. A production build cannot mint the capability, so that
live-class read cannot be admitted to a generation-bound consumer.

### 3.4 The publication boundary

`src/git_publication/` now records and enforces the class:

* `PreparedPrivateIndex::consistency` — the class of the index generation the
  publication is based on. The publisher captures the repository's index bytes
  into the private gitdir, so the generation it observed is a **captured**
  one; the staging's own class is folded in with `weaker`.
* `GitIndexPublicationReceipt::consistency` — the class the publication was
  committed from, so a caller never has to infer it from the fingerprint.
* `GitIndexPublicationRefusal::StagingNotCaptured` — a live-class staging is
  refused **before** the lock is taken and **before** the journal is written.
  A live read may be torn by an outside writer, and a torn generation has no
  fingerprint to compare and swap against.

`src/git_mutations.rs` surfaces the class in the publication receipt summary,
so the tool output states it rather than leaving it implicit.

### 3.5 One judgment call, recorded

`rich_repo_status` embeds a repository operation state that is read from the
filesystem (`.git/` sentinels) rather than from a Git invocation. That read
consults no objects, so it does not weaken the invocation's class, and the
function reports the invocation's class. The alternative reading — that any
no-object component makes the whole composite `NoObjectGeneration` — would
make almost every read unusable as a generation-bound input. The choice is
recorded here because it is a choice.

## 4. Hostile metadata

### 4.1 What the gap was, and how it was established

The prior review recorded that *"`filter.*` and `include.path` are not
mediated, so the clause 1 driver list is unmet"*. That was reproduced
directly, on this host, before anything was changed:

> In a scratch repository whose `filter` was declared **only** in a file
> reached through `include.path` from `.git/config`, `git diff` — a trusted
> read route — executed the filter command. The marker file appeared.

The reason the existing `-c` override table could not prevent it is precise
and worth stating, because it drives the whole design: **`-c` overrides beat
the keys they name, in every configuration file the child reads.** They
cannot beat a key they do not name, and `filter.<name>.clean` is a key whose
name the *repository* chooses. The same holds for
`diff.<name>.command`.

### 4.2 The detector

`crates/egggit/src/config_surface.rs` is new. It is a bounded, read-only
detector that parses the repository's configuration **itself** — it never
shells out to `git config`, because `git config --list` would honour includes
and could run helpers — and reports the dangerous surfaces it finds.

* **It follows includes.** `include.path` and `includeIf.<condition>.path` are
  followed, so a dangerous key that lives only inside an included file is
  still detected. That is the case the prior review named.
* **It is bounded.** Include depth, file count, per-file size and total bytes
  are all bounded by named constants with a stated reason. Exceeding a bound
  stops the walk and reports the inspection as truncated — never an unbounded
  read.
* **It follows an out-of-bounds include when the target is an ordinary
  regular file.** This is a deliberate change from the first draft, and it is
  the correction that matters: refusing to look at an included file is
  precisely the hole the detector exists to close. A target that is *not* a
  regular file — a fifo, a device, a socket — is never opened, and the report
  is truncated.
* **It never leaks a value.** The report carries surface *kinds* and key
  *templates*, never a configuration value. A credential helper whose value
  embeds a password cannot leak through it.
* **It is empirical about Git's grammar.** The parser was derived by probing
  the real `git config`, not from memory: inline `#`/`;` comments, quoted
  values, `\n \t \b \\ \"` escapes with an unknown escape treated as an error,
  odd/even trailing-backslash continuation, comment lines that do not
  continue, section-less keys, dot-form subsections, and Git's
  section/key lower-casing. A differential check against real `git config
  --file --list` over 46 samples matched on every one.

### 4.3 The mediation

The trusted route now inspects the configuration surface **before** it spawns,
and refuses when the surface declares something the route cannot neutralise:

| Surface | Disposition |
|---|---|
| `filter.<name>.process` / `.clean` / `.smudge` | **refused** — reachable from an ordinary `git status` or `git diff` |
| `diff.<name>.command` | **refused** — a repository-chosen diff driver |
| an inspection that was truncated | **refused** — "we could not check" is not "we checked and found nothing" |
| `core.attributesFile`, `core.alternateRefsCommand`, `core.gitProxy`, `core.pager`, `core.editor`, `sequence.editor` | neutralised by the pinned override table |
| `url.*.insteadOf`, `credential.<url>.helper`, `core.sshCommand` | reachable only from a remote operation, which the trusted route refuses outright |
| `merge.<name>.driver` | reachable only from a merge operation, which the trusted route refuses outright |

The refusal uses the existing typed vocabulary: `RequiresContainedExecution`
with `CleanSmudgeFilter` or `ExternalDiffOrTextconv`, which the route's
existing gate turns into `BackendUnavailable`. No new outcome shape was
invented, and no Git authorization was widened — the trusted read route can
now do **less**, not more.

### 4.4 The overrides that were added

The trusted route's `-c` table gained three keys and the environment gained
one:

* `core.attributesFile=/dev/null` — a user-level attributes file can select
  a filter or a diff driver.
* `core.alternateRefsCommand=` — a repository-configured alternate-refs
  command runs a program Git invokes itself.
* `core.gitProxy=` — a repository-configured Git proxy routes Git's own
  protocol traffic.
* `GIT_NO_REPLACE_OBJECTS=1` — a repository-declared `refs/replace/*` makes
  Git substitute one object for another on read, so a read that honoured it
  would not describe the generation the repository actually holds.

### 4.5 The guard

`scripts/check_git_forbidden_patterns.py` gained a hostile-metadata
authority check and a `--self-test`. The check asserts that each dangerous
key is neutralised, that no dangerous key is carried in an executable form,
that the `GIT_CONFIG_COUNT` injection path carries only the commit identity,
and that five route obligations hold **independently** — because a key
override cannot stand in for a route refusal and a route refusal cannot stand
in for a key override; they defend against different surfaces.

The self-test injects **32** defects and asserts each is rejected, and
additionally asserts that the unmodified route produces **no** finding, so a
guard that rejected everything would not be mistaken for one that
discriminates.

Two defects were found in the guard while the milestone's own overrides were
being added, and both were fixed rather than worked around:

* **A last-wins bug.** Git applies `-c` options in order, so the **last**
  occurrence of a key is the one that takes effect. The check tested "is any
  occurrence neutralising", which would let a later hostile value win over
  an earlier safe one unnoticed. It now tests the effective value and also
  reports a duplicated key.
* **Four protections were only implied.** `--no-ext-diff`, `--no-textconv`,
  the remote-operation refusal and the filter-bearing-operation refusal were
  each detected only as *evidence for another key*, so neutralising that key
  silently disarmed them. They are now asserted as independent obligations,
  each with its own injected defect.

## 5. The C007 publication invariants

Three invariants the C007 closure record claimed had no test. They now do,
in `tests/command_git/git_index_publication.inc`:

1. **The real `Add` dispatch preflight** runs before the publisher touches
   the repository, and a failing precondition leaves the index and refs
   byte-identical with no publication recorded.
2. **Scoped scratch** — a scratch root inside the repository is refused
   before the repository is modified.
3. **No source filter invocation** — the publisher never invokes a
   repository-declared clean/smudge filter, and the ordinary publication
   still commits.

The filter test carries a **host-positive control**: the same filter command
is run directly and asserted to produce its sentinel. A sentinel-absence
assertion without that control would pass even if the filter could never
have run, and would prove nothing.

A fourth test covers the new fence: a live-class prepared index is refused
before any mutation, the real index is byte-identical afterwards, no journal
row is written, and the ordinary captured-class path still commits.

## 6. The ADR-0044 maintainer packet

`plans/closure/linux-sandbox-admission-follow-up-corrective/003-adr-0044-packet.md`
states the proposed amendment to clause 1, a **tested-versus-untested** table
covering every property the milestone claims, and an **owner yes/no** on the
preconditions ADR-0044 names.

The prior review's six blocking items are answered there. Two of them are
answered **no**, deliberately:

* **M003 backend admission** — this milestone introduces no backend and
  admits none.
* **C010** — C010's negative closure remains authoritative. This milestone's
  change is a *narrowing* of the trusted route's admissibility, which is
  compatible with it.

ADR-0044 itself remains **proposed**. Nothing in this milestone accepts it,
and no production live-ODB read is enabled or qualified.

## 7. The guards and the docs

* `scripts/check_git_forbidden_patterns.py` — the hostile-metadata authority
  check and its self-test (§4.5).
* `architecture/git.md` — the read-consistency contract, the class
  vocabulary, the fence, and the hostile-configuration mediation.
* `architecture/security.md` — the hostile-configuration surface and what
  the trusted route refuses.
* `crates/egggit/Cargo.toml` — the crate's first optional feature, with the
  reason it exists and the fact that it is absent from the shipped binary.

## 8. Verification executed

All of it on this Linux host. The counts in §8.1–§8.4 were first measured on
the milestone's own head and then **re-measured on the merged head**, after
`main` consolidated the command-git test family into `tests/command_git/*.inc`
and this milestone's two new suites were moved into that family. The numbers
below are the merged head's.

### 8.1 Compile and static gates

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check -p codegg --locked --all-targets` | clean, no errors and no warnings |
| `cargo check -p codegg --locked --tests` | clean — every integration test target **this command enables** compiles. It does not enable `linux-git-test-support`, so the `linux_git_test_support` target is **not** among them; see §12.5 |
| `git diff --check` | clean |

### 8.2 The focused test targets

| Target | Result |
|---|---|
| `cargo nextest run -p egggit` | **164 passed**, 1 skipped (165 total) |
| `cargo nextest run -p codegg --test command_git` | **478 passed**, 1 skipped (479 total) |
| `cargo nextest run -p codegg-core` | **1105 passed**, 0 skipped |

The `command_git` target is the consolidated command-git family that `main`
introduced while this milestone was in flight: one Cargo test executable
whose members are `tests/command_git/*.inc`. This milestone's two new suites
were **moved into that family** (`git_read_consistency.inc` and
`git_hostile_metadata.inc`) rather than left as standalone executables, so
the consolidation's purpose — N executables replaced by 1 — is preserved.
The family's 478 include this milestone's 26 new and adapted members:
16 in `git_read_consistency`, 6 in `git_hostile_metadata`, and 21 in
`git_index_publication` of which 4 are C003's.

The member-level counts are smaller than the pre-consolidation per-file
counts for the members that share the family's `common` helper, because the
5 shared secret-scan tests live once in `secret_scan_self_tests.inc` and are
counted there rather than in each member. That is a counting artefact, not a
loss: the shared tests still run.

#### 8.2.1 What the new tests assert, by name

*Fence and contract* (`tests/command_git/git_read_consistency.inc`): a live-class read is
inadmissible to a generation-bound consumer; a read with no object generation
is refused; captured classes are admitted and round-trip their class; the
class cannot be dropped from a read fact; a read fact is not a
generation-consistent value; a live read is admitted only when the capability
is held; the capability cannot be obtained in a production build; the weaker
class wins for a composite read; a mixed-snapshot read reports the weakest of
its contributing spawns; a read after an outside mutation still reports a
captured generation; a repeat read of an unchanged repository is served from
the capture cache; a concurrent outside writer never yields an over-claiming
class; a repository with an alternate object database is refused by the
trusted route; the trusted read route does not honour replace refs; a
producer/consumer mismatch is refused before any mutation; a failed read is a
typed error and cannot be mistaken for a result; the operation-state sentinel
read reports no object generation.

*Hostile configuration* (`tests/command_git/git_hostile_metadata.inc`): a filter process
declared only through `include.path` is detected; a filter clean command
declared only through `include.path` is detected; the filter sentinel command
produces the sentinel when run directly (the **host-positive control**); an
ordinary repository reports no hostile surface; a URL-scoped credential helper
is detected and the plain form is not; a credential secret never appears in
the report.

*C007 invariants* (`tests/command_git/git_index_publication.inc`): the real `Add` dispatch
preflight refuses before the publisher is consulted; a scratch root inside the
repository is refused before the repository changes; the publisher never
invokes a repository-declared clean filter; a live-class prepared index is
refused before any mutation.

#### 8.2.2 The host-positive control

The filter cases carry a control that makes the absence assertion mean
something: the same filter command is run **directly** and asserted to
produce its sentinel file. Without that control, "the sentinel was not
created" would also hold on a host where the filter could never have run, and
would prove nothing. The control runs in this milestone's own suite, and it
passes.

### 8.3 The guard

| Command | Result |
|---|---|
| `python3 scripts/check_git_forbidden_patterns.py` | `forbidden-pattern checks: PASS (0 findings)`, exit 0 |
| `python3 scripts/check_git_forbidden_patterns.py --self-test` | `32 injected defects were each rejected by the guard, and the unmodified trusted Git route produced no finding`, exit 0 |

### 8.4 The canonical quick gate

| Command | Result |
|---|---|
| `scripts/verify.sh quick` | **`Quick verification passed.`** — the agent schema, core/client/desktop boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, queue-delivery-callsites, queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards, the filter-execution guard (20 registered filters) and `cargo check --workspace --all-targets --locked` all passed |

The gate was run **four** times, and it failed twice. Both failures are
recorded here, because a first run's result is a real result and not a
footnote.

1. **Failed on `cargo fmt --check`** — five one-line call-site adaptations
   exceeded the 100-column limit. The formatting was fixed and the gate was
   re-run.
2. **Passed** on the milestone's own head.
3. **Failed on `check_test_target_references`** after `main`'s consolidation
   was merged in: `architecture/overview.md` recorded 319 integration test
   files and the tree held 321, because C003's two new members are two more
   files. The count row was re-measured and corrected — 321 — and the
   correction is recorded in `architecture/overview.md`'s own count history,
   with the reason and the note that the **test-target** count did not move
   because both new members live inside the consolidated family.
4. **Passed** on the merged head.

### 8.5 The landing

| Item | Result |
|---|---|
| PR | [#169](https://github.com/dbowm91/codegg/pull/169) — "C003: typed Git read-consistency contract, consumer fence, and hostile-config mediation" |
| Merge commit | `f1b83a7a7` — a **merge commit**, not a squash. The four milestone SHAs (`f00d10e0d`, `86a9b372f`, `5188e227f`, `3f1ca3323`) and the merge-forward `f69622924` are all reachable from `main`. |
| Hosted `CI` job on the merge commit | `verify` **passed** |
| Hosted Linux M004 qualification on the merge commit | `process-ipc` and `restricted-userns-fails-closed` **failed to compile** — see §12.5 |

The landing therefore needed a follow-up repair: PR #173
(`fix/c003-linux-git-test-support-call-sites`), merged as `a3b6116e8`, which
adapted the three call sites in the feature-gated `linux-git-test-support`
family to C003's typed live-read result.

| Hosted lane on PR #173 | Result |
|---|---|
| `verify` | **passed** (26m28s) |
| `process-ipc` | **passed** (33m20s) — including the step "C007 private-index publication under containment", which is the step that failed to compile on the merge commit |
| `restricted-userns-fails-closed` | **passed** (9m30s) |

Both jobs that failed on the merge commit pass on the repair, and the step
that failed to compile now runs and passes.

The repair does not change this milestone's disposition: C003 remains
**conditionally closed** on the ADR-0044 decision (§11).

## 9. What this milestone does not establish

1. **ADR-0044 is not accepted.** Its status is unchanged.
2. **No production live-ODB read exists.** The live route stays behind the
   test-only support feature, and the capability a consumer would need to use
   a live-class read cannot be obtained in a product build.
3. **The check-then-spawn window is not closed.** The configuration-surface
   inspection is a pre-flight. A repository that rewrites its own
   configuration between the check and the spawn is not covered.
4. **The detector does not reproduce Git's include resolution exactly.**
   `includeIf` conditions cannot be evaluated faithfully at inspection time,
   so the detector follows them unconditionally. It therefore **over-reports**
   relative to what the child will read, which is the safe direction, but the
   report is not a simulation of Git's behaviour.
5. **The class describes the object store only.** Refs, the index and the
   worktree are still read live, and a read that consulted them has not
   observed one repository generation in the whole.
6. **No merge-base consumer exists to fence**, and none was invented.
7. **No Windows or macOS result is claimed.** The milestone was executed and
   reviewed on Linux.
8. **Git C005, Linux C002, M003/C010 and M008 are untouched.** This
   milestone does not close them and does not attempt to.

## 10. Residual risks

| Risk | Status |
|---|---|
| A repository rewrites its configuration between the pre-flight and the spawn | open, stated in §9.3 |
| `includeIf` conditions are over-reported | open, stated in §9.4; the safe direction |
| The private object view serves a captured generation that an outside writer has since replaced | inherent to the preparation cache; the class says `CapturedNotCurrent` rather than claiming currency, and the publisher's compare-and-swap is what protects a mutation |
| The trusted read route now refuses repositories that declare a filter command | **intended** — a narrowing, not a regression; repositories using LFS-style filters need the contained tier, which is a separate owner decision |

## 11. The condition on this closure

C003 is **conditionally closed**. The condition is maintainer-owned and is not
something this milestone can discharge:

> **The ADR-0044 owner decision.** Until a maintainer accepts (or revises)
> ADR-0044, the live-ODB route stays test-only and the capability stays
> unobtainable in a product build. Nothing downstream may treat this
> milestone's typed contract as an acceptance of ADR-0044.

The slice's own exit criteria are met: the fence cannot be bypassed by
ordinary typed Git execution, the hostile-config cases and the
external-writer negatives are non-vacuous and documented, the C007 and
immutable-route regressions are green, and the ADR packet identifies the
still-unmet evidence.

## 12. Factual corrections made to this record

Recorded here so that a later reader can see what was corrected rather than
finding a silently rewritten document.

1. **The filter class was widened during the milestone.** The detector's
   first draft classified only `filter.<name>.process`. The reproduction
   showed `git diff` executing a **clean** filter, so the class was widened
   to `process`, `clean` and `smudge` and the variant was renamed
   `FilterCommand` to say so.
2. **The out-of-bounds include behaviour was reversed during the milestone.**
   The first draft refused to follow an include that resolved outside the
   bounded root set and reported the inspection as truncated. That was
   correct as a bound and wrong as a security property: refusing to look is
   the hole. The behaviour was changed to follow an ordinary regular file,
   and the truncated report was kept for a target that is not a regular file.
3. **The guard's last-wins bug was found by the milestone's own overrides.**
   Adding `core.attributesFile` to the override table made two of the
   self-test's injected defects stop being detected. The cause was a real
   correctness bug in the guard, not a defect in the injection.
4. **Two test counts were corrected against the consolidated family.** This
   milestone's two new suites were first measured as standalone executables
   (11 and 16 tests). The 11 counted the shared `common` helper's 5
   secret-scan tests as well, because a standalone file with `mod common;`
   compiles that module's tests into its own binary. Inside the consolidated
   `command_git` family the shared tests run once, in their own member, so the
   member's own count is 6. The `git_index_publication` count moved the same
   way (26 standalone → 21 as a member, with the 5 shared tests counted
   separately). The `architecture/git.md` table now records member-level
   counts and says so.

   The historical closure records that name `tests/git_index_publication.rs`
   are **not** rewritten: `main` relocated that file into the consolidated
   family while this milestone was in flight, and those records are pinned to
   their own evidence. This note is the correction's home instead.

5. **The landing broke the feature-gated Linux qualification, and this
   milestone's own baseline did not see it.** C003 changed
   `GitExecutionService::execute_contained_live_read_test_only` to return
   `GitReadFact<ManagedProcessResult>`, so that the contained live pilot's
   consistency class travels with the value instead of being a separate fact.
   The three call sites in the feature-gated `linux-git-test-support` family
   were not adapted, so the `linux_git_test_support` test target no longer
   compiled. On merge commit `f1b83a7a7` the hosted Linux M004 lane failed
   **both** of its jobs with that compile error.

   The cause is structural, not incidental. `scripts/verify.sh:347-349`
   records that the `windows-sandbox-test-support` and `linux-git-test-support`
   families are qualified by the platform-specific workflows and not by the
   routine gates, and neither `cargo check --workspace --all-targets` nor the
   CI `verify` job enables those features. **The routine gates cannot see this
   class of change.** §8.1's "`cargo check -p codegg --locked --tests` — every
   integration test target compiles" was therefore **false** for the gated
   family, and is corrected above. The Linux workflow caught the break, which
   is that safety net working as designed; the fault was that this milestone
   did not compile the gated target locally before landing.

   The repair is PR #173, merged as `a3b6116e8`. It is not a weakening: no test
   was skipped, excluded, or timed out, and the two repaired helpers now
   assert `GitReadConsistency::LiveMutable`, so the live pilot's honesty is
   observable at its own call sites rather than being discarded at the
   unwrap. A second commit in the same PR fixed five pre-existing
   `needless_borrow` findings that `cargo clippy` reports in the same file;
   they predate C003 and were found only because the repair is the first time
   the gated target has been linted at all.

   **What is still not established:** the routine gates still do not compile
   the gated families. Putting the gated targets inside a routine compile
   check is a separate decision, and it is not made here.
