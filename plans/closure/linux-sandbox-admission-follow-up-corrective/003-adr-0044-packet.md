# ADR-0044 Maintainer Packet — Consistency Class, Live-Read Capability, and Hostile Metadata

Status: **prepared for maintainer review — ADR-0044 remains `proposed` and is not accepted by this packet**
Prepared by: milestone C003 of `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/003-git-live-read-consistency-and-metadata-review.md`
Closure record: `plans/closure/linux-sandbox-admission-follow-up-corrective/003-status.md`
ADR under review: `plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md`
Prior review that produced the proposed amendment: `plans/closure/linux-trusted-git-cost-corrective/006-status.md` §8

## 1. What this packet is

ADR-0044 is **proposed**. Nothing in this packet accepts it, changes its
status, or authorizes a production live-ODB read. This packet does three
things and nothing else:

1. it states the **proposed decision text** that C003's implementation would
   support, as a concrete amendment to clause 1;
2. it records a **tested-versus-untested table**, so a reviewer can see what
   the evidence covers and what it does not; and
3. it records an **owner yes/no** on the preconditions ADR-0044 names.

The prior ADR review (`006-status.md` §8) listed six blocking items. This
packet answers each of them, including the two that remain open.

## 2. Why clause 1 needed amending

The proposed clause 1 read:

> Introduce an explicit internal Git **consistency/authority requirement**,
> distinguishing an immutable-object read from a sandbox-contained
> *live-object* read. Only the latter may read the original ODB without
> copying; its receipt must say mutable/non-snapshot and must not claim
> reproducible generation identity.

The review found three defects in that text:

1. **Two classes, three realities.** The private object view's preparation
   cache is a best-effort mechanism with eviction, not a durability boundary.
   A cache-served read is a *captured* generation that may already be stale,
   not an immutable one. A two-class vocabulary cannot say so truthfully.
2. **A receipt that "says" is not a type.** "Its receipt must say
   mutable/non-snapshot" is prose. Nothing stopped a consumer from treating a
   possibly-torn read as a stable generation, because the class was a string
   in a receipt rather than a value in the result.
3. **The authority was implicit.** Nothing in the text said *who* may consume
   a live-class read, or how that differs from a consumer that merely knows
   the class.

The review's own proposed fix was: *"a typed consistency value, inadmissibility
of live results to generation-consistent consumers, and capability gating the
way `DestructiveRepositoryChange` gates a destructive mutation."* That is
what C003 built, and this packet states it as decision text.

## 3. Proposed decision text (amendment to clause 1)

> **1. Consistency class.** Every Git read the daemon performs carries a typed
> consistency class describing the object generation it observed:
>
> * `ImmutableGeneration` — the read served objects from one materialized
>   generation. The claim is bounded by the preparation that produced it.
> * `CapturedNotCurrent` — the read served objects from a capture that was
>   immutable when it was taken and may now be stale relative to the
>   repository. This is the class of a cache-served read, and the preparation
>   cache's eviction is precisely why it is not `ImmutableGeneration`.
> * `LiveMutable` — the read served objects directly from a concurrently
>   mutable repository. The observation may be torn or invalidated by an
>   outside writer.
> * `NoObjectGeneration` — the read consulted no object store, so it observed
>   no object generation at all.
>
> The class is a **typed value carried in the read result**, never a string
> in a receipt, so a consumer cannot receive the bytes and silently drop what
> it was allowed to assume about them. The class describes the **object
> store** only: the private object view covers the object database, and refs,
> the index and the worktree are always read live. A read that is assembled
> from several Git invocations carries the **weakest** class among them.
>
> A consumer whose contract requires one stable object generation — diff
> generation, index publication, merge-base computation — accepts a read only
> through an explicit admissibility check. A live-class read is **refused**,
> not warned about, unless the consumer holds the live-read capability. A
> read that observed no object generation cannot satisfy such a consumer.
>
> **1a. Live-read capability.** The live-read capability is an **authority**,
> on the same footing as `DestructiveRepositoryChange` gates a destructive
> mutation: a consumer must *hold* it to use a live-class read, and holding it
> is a reviewed decision rather than a label. A production build of CodeGG
> cannot obtain it — its only constructor is compiled out unless a
> separately reviewed test-support feature is enabled — so the live route
> remains test-only and no product build can consume a live-class read. The
> capability authorises the use of a live read; it does not rewrite the
> class, so an admitted live read still reports `LiveMutable`.
>
> **1b. Hostile configuration metadata.** A trusted Git read must not execute
> repository-controlled configuration. The route neutralises the
> configuration **keys** it can name by pinning them with `-c` overrides, and
> it refuses the operations that consult filters and merge drivers. Neither
> reaches a surface that exists only in the **content** of a configuration
> file: a `filter.<name>.clean` / `.smudge` / `.process` command is reachable
> from an ordinary `git status` or `git diff`, because both compare the
> working tree against the index *through the clean filter*, and the filter's
> name is the repository's to choose. The route therefore inspects the
> repository's configuration surface — following `include.path` and
> `includeIf.<condition>.path`, within bounded depth, file count and size —
> before it spawns, and refuses when the surface declares a command the route
> cannot neutralise. The inspection is a pre-flight: it cannot close the
> window between the check and the spawn, and it does not claim to.

### 3.1 What the amendment does *not* change

* It does not enable a production live-ODB read. The live route stays behind
  the test-only support feature.
* It does not alter the C007 publisher's authority boundary, the
  `GitExecutionService` / `GitMutationExecutor` / `ManagedProcessService`
  ownership, or the typed approval and lease model.
* It does not admit any M003 backend, claim any `NetworkEnforcement::Enforced`
  boundary, or reopen C010 / C012.
* It does not reinterpret an existing `Enforced` flag or any other persisted
  field.

## 4. Tested versus untested

Every row is either **tested** (a named, committed test asserts it) or
**untested** (no test asserts it, or the property cannot be asserted on this
host). Nothing in the untested column is claimed as proven.

### 4.1 The consistency contract

| Property | Status | Evidence |
|---|---|---|
| The class is a closed, typed, serializable value with distinct names | **tested** | `crates/egggit/src/consistency.rs` — `GitReadConsistency`, its `as_str`, and the class-name unit test |
| The class travels with the read result and survives `map` | **tested** | `GitReadFact::map` and its unit test |
| A live-class read is refused for a generation-bound consumer | **tested** | `admit_generation_consistent` returns `LiveClassWithoutCapability`; asserted in `consistency.rs` and in `tests/git_read_consistency.rs` |
| A read with no object generation is refused | **tested** | `admit_generation_consistent` returns `NoObjectGeneration`; asserted in `consistency.rs` and in `tests/git_read_consistency.rs` |
| A captured class is admitted, and the admitted value keeps its class | **tested** | `consistency.rs`; `tests/git_read_consistency.rs` |
| A composite read carries the weakest contributing class | **tested** | `GitReadConsistency::weaker`; the `repo_status` / `list_branches` / `diff_summary` weakest-class tests in `crates/egggit/src/` |
| A production build cannot obtain the live-read capability | **tested** | `LiveReadCapability`'s only constructor is `#[cfg(feature = "git-live-read-test-support")]`; the crate's default build has no constructor, and the root crate's `live_read_capability()` returns `None` in every build |
| The capability authorises a live read without rewriting its class | **tested** | the capability-gated admission test, under the test-only feature |
| The refusal happens **before** mutation and leaves refs and the index unchanged | **tested** | the publisher refusal test and the producer/consumer-mismatch test in `tests/git_read_consistency.rs` |
| The publisher records the class of the generation it published from | **tested** | `GitIndexPublicationReceipt::consistency`; the publication receipt summary in `src/git_mutations.rs` |
| The publisher refuses a live-class staging before it writes anything | **tested** | `publish_prepared_index` returns `StagingNotCaptured` before the lock and before the journal write |

### 4.2 Hostile configuration metadata

| Property | Status | Evidence |
|---|---|---|
| A repository-declared filter command is refused on the **read** route | **tested** | `trusted_git_read_route_refuses_a_repository_that_declares_a_filter_command` in `crates/egggit/src/process.rs` |
| A filter command declared **only** in an `include.path` file is refused | **tested** | `trusted_git_read_route_refuses_a_filter_declared_only_in_an_included_file`; the detector's include tests |
| The filter command does not execute | **tested** | the marker assertion in both read-route tests, plus the host-positive control in `tests/git_hostile_metadata.rs` |
| An ordinary repository still reads | **tested** | `trusted_git_read_route_allows_a_repository_without_a_hostile_surface`; the detector's benign-config test |
| The inspection is bounded (depth, file count, file size, total bytes) | **tested** | the detector's bound tests in `crates/egggit/src/config_surface.rs` |
| An include cycle terminates and is reported | **tested** | the detector's include-cycle test |
| A malformed configuration is reported without panicking | **tested** | the detector's malformed-config tests |
| The report never carries a configuration **value** | **tested** | the detector's secret-absence test; the report carries surface kinds and key templates only |
| The trusted route pins the configuration keys it can name | **tested** | `scripts/check_git_forbidden_patterns.py` — the hostile-metadata authority check, with 32 injected defects each asserted rejected |
| The route disables replace refs | **tested** | `GIT_NO_REPLACE_OBJECTS=1` in `crates/egggit/src/process.rs`; asserted by the guard's route-obligation check and its injected defect |
| The route pins `core.attributesFile`, `core.alternateRefsCommand` and `core.gitProxy` | **tested** | the `-c` override table in `crates/egggit/src/process.rs`; asserted by the guard |
| The route refuses the filter-bearing and remote operations | **tested** | the guard's independent route obligations |
| A URL-scoped credential helper is detected | **tested** | the detector's classification test |
| An out-of-bounds `include.path` that is an ordinary regular file is still inspected | **tested** | the detector's out-of-bounds include test |
| An `include.path` that is **not** a regular file is never opened | **tested** | the detector's non-regular-file test |
| The inspection cannot see a configuration that changes between the check and the spawn | **untested — residual** | The pre-flight is a check-then-spawn sequence. A repository that rewrites its own configuration between the two steps is not covered by a test. This is stated as a residual risk, not closed. |
| The `includeIf` conditions are evaluated faithfully | **untested — overstated by design** | The detector follows `includeIf.<condition>.path` **unconditionally**, because `onbranch:`, `gitdir:` and `hasconfig:` cannot be evaluated faithfully at inspection time. The detector therefore over-reports relative to what the child will actually read. Over-reporting is the safe direction, but it means the detector's report is not a faithful simulation of Git's include resolution. |
| The private object view's preparation is atomic with respect to a concurrent `git gc` | **untested — partial** | The preparation re-checks that the object store did not change while it was copied, and reports the failure rather than serving a torn view. That check is exercised by the view's own tests. A *concurrent writer that starts after the check and before the read* is not covered by a test, and the read is served the captured generation — which is why the class says `CapturedNotCurrent` and not `ImmutableGeneration`. |
| The live route's consistency observation distinguishes "torn" from "stale" | **untested — not implemented** | The class vocabulary distinguishes `CapturedNotCurrent` from `LiveMutable`, which is the distinction the prior review asked for at the class level. A finer-grained "a torn read was detected" state is **not** implemented, because no live route exists to produce it. The live route stays test-only. |
| The Windows and macOS platforms satisfy the same contract | **untested** | This milestone was executed and reviewed on Linux. No Windows or macOS run is claimed for the code in this milestone. |

## 5. Owner yes/no on the named preconditions

ADR-0044's acceptance gate requires more than this milestone can supply. Each
precondition is answered with an owner decision, not with a test result.

| Precondition | Answer | Why |
|---|---|---|
| **C002** — the maintainer-owned production-route condition | **No** | C002's own status is *conditionally closed* with the condition owned by the maintainer. C003 does not discharge it and does not attempt to. |
| **C023** — the M005 `NetworkEnforcement::Enforced` producer | **No** | C003 does not produce an enforced-network claim and does not need one: the live route it introduces is test-only and its containment is asserted by the M004/M005 tier, not by C003. |
| **M003** — backend admission | **No** | C003 introduces **no** new backend and admits none. The trusted read route's behaviour changed — it now refuses a repository that declares a filter command — but that is a refusal, not an admission. |
| **C010** — no-admission closure for the immutable-view-based route | **No** | C010 remains authoritative. C003's change is a *narrowing* of the trusted route's admissibility, which is compatible with C010's negative closure; it does not reopen it. |
| **M008** | **No** | Untouched by this milestone. |
| **Independent security review of ADR-0044** | **No — this packet is an input to it, not a substitute for it** | A second review pass over this packet is still required before ADR-0044 can be accepted. |

## 6. What this packet does not claim

* It does not claim ADR-0044 is accepted, or that its status changed.
* It does not claim a production live-ODB read exists, is qualified, or is
  safe to enable.
* It does not claim the check-then-spawn window is closed.
* It does not claim the detector's include resolution matches Git's exactly.
* It does not claim any Windows or macOS result.
* It does not claim the four C007 publication invariants were *newly*
  implemented — they were implemented by C007. C003 adds the regression
  tests that were missing and the consistency fence on top of them.
* It does not claim the live-read capability is unavailable in a build that
  enables the test-only support feature. It is unavailable in the **shipped**
  binary, which is the claim that matters.

## 7. The recommendation this packet supports

C003's implementation makes the **proposed** clause 1 enforceable rather than
descriptive: the class is typed, carried, and checked; the capability is an
authority a production build cannot obtain; and the hostile-configuration
gap the prior review named is closed on the read route.

That is evidence for the maintainer to *consider* accepting ADR-0044. It is
not evidence that ADR-0044 *is* accepted, and the preconditions in §5 are
unmet by design. The live route stays test-only until a maintainer decision
says otherwise.
