# Linux Trusted Git Cost Corrective C006 — ADR-0044 Live-Read Consistency Review

Status: review complete — ADR-0044 remains **proposed**; this record does not accept it, and it revises clause 1 rather than granting authority

Source review target:

- `plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `6efd5ebdf90f91fc24dcac7f2827f4ca9e355135`

## 1. Executive finding

The live-read consistency model proposed in ADR-0044 clause 1 is **directionally correct and necessary**, and the ADR is right that a sandbox-contained *live-object* read is a different authority class from an immutable-object read. C012's negative finding is unchanged and still valid: a read-only bind mount is not an immutable snapshot, and an in-process lease cannot exclude an external writer.

The model is nevertheless **underspecified in the one place that would make it safe**, and the underspecification is not cosmetic. ADR-0044 requires that a live read's receipt "say mutable/non-snapshot and must not claim reproducible generation identity." A receipt that *says* something is advisory text. The ADR does not require that any **consumer** of a live-read result be structurally prevented from treating it as generation-consistent. A route can satisfy clause 1 literally — print `mutable` in a receipt — and still feed a torn read into a diff, a publish, or a merge-base computation that assumes a stable generation. The clause constrains the *producer's wording*, not the *system's behaviour*.

The concrete second finding is that **there is nothing to review in production**. The live-ODB read path exists only as a test-only pilot behind a dev-only feature, and the consistency class ADR-0044 would introduce **has no carrier in any type**: no consistency field, no live-read receipt dimension, and no producer for the `NetworkEnforcement::Enforced` value that clause 2 treats as a precondition. Clause 1 is therefore unimplementable as written — not because it is wrong, but because it names a receipt that does not exist.

The review recommendation is: **do not accept ADR-0044 as written.** Adopt the revised clause 1 and clause 2 language below, and treat acceptance as blocked behind a named consistency type, a route that cannot be confused with the immutable route, and the C004 production vertical that would supply the publisher.

## 2. Requirement-to-evidence matrix

Clause-by-clause verdict against the code as it stands at the baseline SHA.

| ADR-0044 clause | Current code state | Evidence | Verdict |
|---|---|---|---|
| 1 — introduce an explicit consistency/authority requirement distinguishing immutable from live reads; the live receipt must say mutable/non-snapshot and must not claim reproducible generation identity | No consistency dimension exists in any type. `LiveReadInconsistentPossible` is described in the ADR as an illustrative *name*. `SandboxEnforcement` carries `filesystem`, `network`, `process` only (`crates/codegg-core/src/approval.rs`). No public or protocol DTO carries a consistency claim. | `crates/codegg-core/src/approval.rs`; `crates/codegg-protocol/src/core.rs` | **not implemented — and underspecified.** The clause names a receipt that has no carrier, and constrains wording rather than consumer behaviour. |
| 2 — live read only when the child obtains the full required boundary **before exec**; a probe, unverified helper, legacy Landlock-only route, or hoped-for M005 network claim is insufficient; no fallback to host Git or FullHost on denial | Containment-before-exec is real for the M004/M005 process tier: `SandboxExecutionOutcome::Enforced` carries obtained facts from the child. But the M005 network half has **no producer**: `SandboxEnforcement::for_constrained_enforced` always sets `network: NetworkEnforcement::Unrestricted`, and no code path constructs `NetworkEnforcement::Enforced`. | `src/managed_process.rs`; `crates/codegg-core/src/approval.rs` | **partially satisfied, and the M005 precondition is vacuous.** "Hoped-for M005 network claim" is currently the only kind that exists. |
| 3 — preserve `GitExecutionService` / `GitMutationExecutor` / `ManagedProcessService` and typed approval/lease authority; no uncontained Git with executable repo-controlled configuration | All three owners exist and are unchanged by the candidate work. `GitEnvPolicy::apply`/`apply_sync` strip `GIT_INDEX_FILE` and `GIT_ALTERNATE_OBJECT_DIRECTORIES` and deny `--git-dir`/`--work-tree`. `GitMutationExecutor` has no `Add` arm. | `crates/egggit/src/process.rs`; `src/git_service.rs`; `src/git_mutations.rs` | **satisfied structurally.** `filter.*` is not disabled, so "executable repo-controlled configuration" is not yet fully excluded (see §4). |
| 4 — bind selected worktree, gitdir, common-dir and authorized ODB identity; reject or mediate alternates, replace refs, shallow/grafts, include paths, aliases, symlink/mount changes, cross-worktree references; do not rely on command-spelling allowlists | Alternates and `--git-dir`/`--work-tree` are handled. The test-only pilot rejects linked worktrees by geometry. `filter.*`, `include.path`, and alias mediation are **not** present. | `crates/egggit/src/process.rs`; `tests/linux_m004_process_ipc.rs` | **partially satisfied.** |
| 5 — operation-scoped private index/new-object storage and a narrow **daemon-owned** publisher; validate changed objects/index/refs and allowed paths; reject stale observed state; coordinate Git-compatible locks/CAS; prove recovery after partial failure; the publisher must never execute hooks/filters/helpers with its own host authority; a Codegg lease does **not** exclude external writers | The entire publisher is a **test-local helper** inside `tests/linux_m004_process_ipc.rs`. There is no production publisher, no durable journal, no restart reconciler, and no `Add` dispatch arm in `GitMutationExecutor`. The ADR's own warning that a lease does not fence external writers is correct and is the reason the production route must not be inferred from the prototype. | `tests/linux_m004_process_ipc.rs`; `src/git_mutations.rs`; `src/git_service.rs` | **not implemented in production.** This is the C004 vertical. |
| 6 — network transport/credentials separately mediated under Linux C003/M006; no `RemoteHelper`/Credential permission follows from ordinary contained local Git; destructive reset/clean retains `DestructiveRepositoryChange` authorization | `DestructiveRepositoryChange` exists and is retained; no remote-helper or credential authority follows from a local contained read. | `src/git_mutations.rs`; `crates/codegg-core/src/approval.rs` | **satisfied.** |
| 7 — no new persistent setting, public DTO, production-default switch, M003 backend admission, M005 `NetworkEnforcement::Enforced` claim, or historical closure reversal is authorized by the proposed ADR | None of these exist. The live-ODB pilot is behind a dev-only feature enabled through `[dev-dependencies]`, so it is absent from `cargo build` and from the shipped binary. | `Cargo.toml`; `crates/codegg-providers/Cargo.toml` feature layout precedent | **satisfied and must remain satisfied.** |

## 3. The live-read consistency model specifically

ADR-0044's real subject is the consistency model, and the three findings below are the review's substantive output.

### 3.1 "Immutable" and "live" are not the only classes a reader can observe

The ADR models consistency as a binary: an immutable-object read versus a sandbox-contained live-object read. The current code shows a third case that the binary does not name, and it is the case that actually bites in production.

`GitEnvPolicy::apply` calls `prepare_private_object_view`, which materialises a private object view. The prepare step is guarded by a 4-entry / 2-GiB process cache with fingerprint invalidation, a global preparation mutex, and per-command leases. That machinery is a **best-effort** mechanism: it is a cache with eviction, not a durability boundary. A read served from a cache entry that has not yet been invalidated, concurrently with an external writer, is *neither* a guaranteed immutable generation *nor* a plainly live read — it is a read of bytes that were immutable when they were captured and may now be stale relative to the repository.

That middle case is exactly what C002 measured, and it is why the cost pilot's "advisory cache eviction" caveat exists. ADR-0044's binary does not describe it, so a receipt that says "immutable" can be false under cache eviction, and a receipt that says "live" under-describes a route that is trying to be immutable. The clause needs a third, named class for *captured-but-not-currently-consistent* reads, or it needs to state that "immutable" is a claim about a specific preparation, with an explicit cache-eviction condition, rather than a property of the route.

### 3.2 The receipt constrains wording, not consumption

Clause 1 requires that the live read's receipt "say mutable/non-snapshot and must not claim reproducible generation identity." Nothing in the ADR requires that a consumer treat a live-read result differently from an immutable-read result.

This is the gap that matters, because the consumers are where a torn read turns into a wrong answer rather than a failed command. A concurrent `git gc`/`repack` during a live read can produce a plausible-but-wrong object, or a partially-visible object, or a command failure — and the ADR's security section only asks that "source mutation during object reads" be *tested*, not that any consumer be fenced. An adversarial test proves the route *can* misbehave; it does not prove the misbehavior is *contained*.

The revised clause must require the following, and each is implementable:

- The consistency class must be a **typed value carried in the result**, not a string in a receipt, so that a consumer cannot silently drop it.
- A result carrying the live class must be **inadmissible** to any consumer that assumes generation consistency — diff generation, index publication, merge-base computation, and the C004 publisher. This should be a type-level or explicit admissibility check, not a convention.
- A live-class result must report a **specific** consistency state rather than "not guaranteed", so that a caller can distinguish "observed a torn state" from "served a consistent view that may be stale".

### 3.3 Containment is orthogonal to consistency, and the ADR should say so in the clause rather than only in the drivers

Clause 2 is a containment clause: the child must obtain the boundary before exec. Clause 1 is a consistency clause. The ADR's own Context section makes the orthogonality explicit — "(1) repository-controlled content cannot cause a process to acquire host/daemon capabilities; (2) a Git read must observe one immutable object generation" — and then correctly observes that the existing route requires both.

The revision the review recommends is to make that orthogonality a first-class part of the clause: the live route is defined as **contained but not generation-consistent**, and the live route is **ineligible** for any operation whose contract requires generation consistency. That is stronger and more testable than "report it as mutable", because it converts a labelling duty into a capability restriction that can be enforced by construction and asserted by a test.

The one place this is already true in the code is worth naming as the model to follow: `DestructiveRepositoryChange` is an authorization capability, and the destructive Git operations are gated on it. The live consistency class should be gated the same way — a capability that a consumer must *hold* to use a live result, not a label it must remember to check.

## 4. Additional findings from the clause review

- **Executable repository configuration is not fully excluded.** ADR-0044's first decision driver names clean/smudge/process filters, merge drivers, fsmonitor, external diff/textconv, credential/transport helpers, URL rewrite, executable attributes, and aliases. `GitEnvPolicy` strips `GIT_INDEX_FILE` and `GIT_ALTERNATE_OBJECT_DIRECTORIES` and denies `--git-dir`/`--work-tree`, but `filter.*` is not disabled and `include.path` is not mediated. Under a live read the repository is by definition mutable by an external writer, so the config surface must be treated as hostile at read time, not only at write time. This is a live-read blocker, not a C004 blocker.
- **`NetworkEnforcement::Enforced` has no producer.** `SandboxEnforcement::for_constrained_enforced` always reports `network: NetworkEnforcement::Unrestricted`. Clause 2 treats an M005 network claim as a precondition, and clause 7 forbids the ADR from authorizing such a claim. Both are currently true only because no code produces the value at all. A live-read route that needs a verified offline boundary therefore has nothing to verify against until M005's production owner qualification lands.
- **The C002 pilot is not a production route and must not be read as one.** The live-ODB read pilot is gated behind a dev-only feature enabled through `[dev-dependencies]`, so it is absent from `cargo build` and from the shipped binary. Its measured cost improvement is real evidence about the *mechanism*, and it is not evidence that any production route exists.

## 5. Revised clause language

Proposed replacement for clause 1, and a supporting addition to clause 2. This is a review proposal, not an accepted decision.

> **1. (revised)** Introduce an explicit internal Git **consistency class**, carried as a typed value in the Git result rather than as wording in a receipt. The classes are: *immutable generation* (one observed object generation, with the preparation and cache-eviction conditions that bound the claim), *captured-not-current* (bytes that were immutable when captured and may now be stale), and *live* (bytes read from a concurrently mutable repository). A live read may read the original ODB without copying **only** when it is dispatched through a route that is **structurally incapable** of being mistaken for an immutable read.
>
> A result carrying the live class must (a) be inadmissible to every consumer whose contract requires generation consistency — diff generation, index publication, merge-base computation, and the ADR-0044 publisher — enforced by an explicit admissibility check rather than by convention; and (b) report the specific consistency state observed, distinguishing a detected torn read from a served-but-possibly-stale view.
>
> The live class is a **capability**, not a label. A consumer must hold it to use a live result, on the same footing as `DestructiveRepositoryChange` gates a destructive mutation.

> **2. (addition)** The containment precondition in this clause is independent of the consistency class in clause 1. Obtained containment is evidence about host protection only; it is never evidence of generation consistency, and a route that satisfies this clause does not thereby satisfy clause 1.

## 6. Review outcome and what remains

**ADR-0044 is not accepted by this record.** The recommended next steps, in order:

1. Land the M005 production owner qualification first. Clause 2's M005 precondition has no producer until the offline boundary is both implemented for the durable command route and reportable; a live-read route cannot be qualified against a claim the system does not make.
2. Implement the C004 production vertical — authorized `git add`, private staging, conflict-safe index publication, crash recovery — on the **immutable** route. The publisher is the component that most needs generation consistency, and it is the component ADR-0044 would make ineligible for live reads. Qualifying it against immutable reads first removes the temptation to route it through a live read.
3. Then add the typed consistency class, the live route behind it, and the admissibility check, and re-run this review against the concrete types rather than a proposed name.

**Blocking items for acceptance**, each currently unsatisfied:

- No typed consistency class exists in any DTO or result type.
- No production live-ODB read route exists; the pilot is dev-only and absent from the shipped binary.
- `NetworkEnforcement::Enforced` has no producer, so the clause 2 M005 precondition is vacuous.
- `filter.*` and `include.path` are not mediated, so the clause 1 driver list is not met.
- No consumer-side admissibility fence exists for generation-consistent consumers.
- ADR-0044's own gate requires an independent security review that this record is the first pass of, not the last.

## 7. Verification performed for this review

```bash
# Every clause verdict above was checked against the source at the baseline SHA.
git rev-parse HEAD                                    # 6efd5ebdf90f91fc24dcac7f2827f4ca9e355135
git grep -n "NetworkEnforcement::Enforced" -- '*.rs'  # type only; no producer
git grep -n "prepare_private_object_view" -- '*.rs'  # immutable path is the production path
git grep -n "LiveRead\|Consistency" -- 'crates/*/src/*.rs'   # no consistency type exists
```

No source file was modified for this review, and no new authority is granted by it. The C004 implementation plan, its closure record, and the C005 blocked state are unchanged by this record.
