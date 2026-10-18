# Milestone 002b — Queue Mutation Operations and Mutation Authority: Closure Record

Status: **closed**

Milestone: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 2b, mutation slice)

Plan: `plans/implementation/user-prompt-queue/002b-queue-mutation-operations-and-authority.md`

Authority: `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` — **accepted**
2026-10-18, Option B (durable session-scoped queue-owner binding) with Option D
(the M002a/M002b split) adopted in the same decision.

Closure record: `plans/closure/user-prompt-queue/002b-status.md`

Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted M001 closure
tree plus M002a closed. Nothing in this milestone was committed; the whole working
tree remains uncommitted.

## 1. Class and delivered capability

Primary class: **infrastructure**.

**This milestone still delivers no user-visible capability. No user can queue a
prompt.** There is no command, no dialog, no key, no row-0 change, and no
projection field added here. Six protocol operations and a durable store exist;
nothing a person can press reaches them. M004/M005 own the first user-visible
surface, and M003 owns delivery into turns.

What changed relative to M002a is that the queue is no longer permanently empty:
the four mutations have producers and an authority, so the durable substrate is
exercised. That is infrastructure, not a capability, and this record must not be
read as though a user could reach it.

## 2. Work package A evidence — the authority was accepted before work began

`plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` status line reads
`Status: accepted`, dated 2026-10-18, selecting Option B. Verified by reading the
file before any code was written, not inferred from the plan's claim. The
ADR-0030/ADR-0034 "do not start on a proposed ADR" rule therefore no longer
blocks this milestone. It still applies to ADR-0036, which gates M006.

## 3. What shipped

| Area | Delivered |
|---|---|
| Storage | `migrate_v81` creating `user_prompt_queue_owner`; `STORAGE_LAYOUT_VERSION` 80 → 81; the `if current_version < 81` ladder gate and the `81 =>` dispatch arm |
| Ownership | Three-valued `UserPromptQueueOwnership` (`Absent` / `Owned` / `Unowned`), `UserPromptQueueOwnerRecord`, in-transaction ownership gate |
| Operations | `add`, `edit`, `delete`, `reorder` (collision-free), `adopt`, `transfer` (CAS on `revision`) |
| Predicate | `ControllerScope` enum generalizing `check_turn_controller` into one named predicate with three call shapes |
| Protocol | Six `CoreRequest` variants; `CoreResponse::UserPromptQueueOwner`; existing `UserPromptQueue` reused for the resulting projection |
| Authority | Six `OperationDescriptor`s = `ViaSession` + `AgentInvoke`; six `INSTRUMENTED_OPERATIONS` entries → `prompt_submit` |
| Audit | `audit_instrumentation::user_prompt_queue_mutation_event` — identity and byte count, never text |
| Frontend | **Nothing.** No command, dialog, key, status, or projection field. |

Shipped DDL:

```sql
CREATE TABLE IF NOT EXISTS user_prompt_queue_owner (
    session_id TEXT PRIMARY KEY,
    owner_principal TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    adopted_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
```

v81 is additive and carries **only** this table. v80 is closed and was not edited.

## 4. The adoption policy ADR-0037 deferred to this milestone

ADR-0037's "Neutral or deferred" section required M002b to state, with an audit
decision, whether the queue owner is implicitly seeded from the session creator.

**Decision: the calling principal becomes the owner.** Not the session creator,
and not the local interactive owner. The binding records who actually authored
the queue, which is the fact ADR-0037's Option B is about. Recorded in
`architecture/core.md` and pinned by
`user_prompt_queue_first_add_adopts_the_calling_principal`.

The distinction from `Unowned` is load-bearing: first-add adopts **only** on an
`Absent` queue (creation), never on an `Unowned` one (repair), or an invalidated
queue would quietly become owned by whoever touched it next.

## 5. Acceptance criteria against evidence

| # | Criterion | Evidence |
|---|---|---|
| 1 | Mutation works **while idle**, no turn, no lease row | `user_prompt_queue_mutation_is_authorized_while_the_session_is_idle` (store) and `user_prompt_queue_add_is_authorized_while_the_session_is_idle` (daemon, asserts zero lease rows first) |
| 2 | Unauthorized principal refused for every mutation, idle and mid-turn, typed, no partial write | `user_prompt_queue_non_owner_with_agent_invoke_is_refused_for_every_mutation` — all six operations, both states, then asserts the row count is unchanged |
| 3 | Mid-turn four-row mismatch table exactly as specified | `user_prompt_queue_mid_turn_mismatch_table_refuses_with_typed_reasons` — one scenario per row, each asserting its exact code |
| 4 | Reorder produces a dense daemon-owned order; concurrent reorders converge | `user_prompt_queue_reorder_is_collision_free_from_a_sparse_order`, `user_prompt_queue_reorder_refuses_a_partial_entry_list`, `user_prompt_queue_concurrent_reorder_converges_on_one_order` |
| 5 | Edit preserves position and identity, refuses rather than truncating, never resurrects | `user_prompt_queue_edit_preserves_identity_and_position`, `..._edit_refuses_rather_than_truncating`, `..._edit_never_resurrects_an_expired_entry` |
| 6 | Every mutation audited with identity and no text; coverage guard passes | `user_prompt_queue_mutation_operations_are_instrumented_as_prompt_submit`, `user_prompt_queue_no_refusal_carries_entry_text`; `check_audit_coverage.py` green |
| 7 | Ownership survives restart; concurrent transfers converge on one winner | `user_prompt_queue_ownership_and_transfer_survive_restart` (reopens a real file-backed DB), `..._two_transfers_from_one_revision_converge_on_one_winner`, `..._concurrent_transfers_never_produce_two_owners` |
| 8 | `TurnCancel` / `TurnSteer` authorization tests pass **unmodified** | `cargo test --test session_family` — 100 passed, 2 failed for an unrelated environmental cause (§6). The controller lease suite is untouched by this diff. |
| 9 | `is_controller_bypass` unchanged, carries no queue authority, proven by a test | `user_prompt_queue_controller_bypass_carries_no_queue_authority` |
| 10 | v80 rows survive v81; `STORAGE_LAYOUT_VERSION == 81` equals the highest wired migration | `user_prompt_queue_v80_rows_survive_v81_intact`, `user_prompt_queue_v81_migration_creates_the_owner_table`, plus both layout assertions and the guard `--self-test` |
| 11 | **No user-visible surface exists**, and this record says so | §1 above, stated in words. |

## 6. Verification actually run

Counts are real. Filters are test-name filters; every one below was checked to
select a non-zero number of tests.

| Command | Result |
|---|---|
| `cargo fmt` | ran; `cargo fmt --check` reports no remaining diff |
| `cargo test -p codegg-core user_prompt_queue` | **51 passed** (4 lib unit + 22 M002a integration + 25 M002b integration) |
| `cargo test -p codegg-core --test user_prompt_queue` | 22 passed (M002a's storage slice) |
| `cargo test -p codegg-core session_control` | 4 passed |
| `cargo test --test storage_migrations` | 4 passed |
| `cargo test --test identity_m003_daemon_authorization user_prompt_queue` | 10 passed |
| `cargo test --test session_family` | 100 passed, **2 failed** — see below |
| `cargo clippy -p codegg-core --all-targets -- -D warnings` | clean |
| `cargo clippy -p codegg --all-targets -- -D warnings` | clean |
| `scripts/verify.sh quick` | **passed** — `cargo check --workspace --all-targets --locked` clean in 2m 09s |
| `python3 scripts/check_audit_coverage.py` | passed — "All audit coverage invariants verified" |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 checks passed |
| `python3 scripts/check_project_catalog_invariants.py --self-test` | 4/4 self-test cases passed |
| `bash scripts/check-core-boundary.sh` | passed |

Clippy caught four real findings during this milestone, all fixed rather than
allowed: a 9-argument audit builder (grouped into `UserPromptQueueAudit`), an
inverted `if let` over an array of `Result`s, two `let…else` arms rewritten with
`?`, and one unused test binding. `UserPromptQueueAudit` is the better shape
regardless of the arity lint.

### The two `session_family` failures are environmental, not regressions

`control_m004_controller_lease::turn_submit_atomically_acquires_controller` and
`::concurrent_submits_converge_on_one_controller` both panic with the identical
message:

```
workspace register failed: Error { code: "workspace_register_failed",
  message: "workspace root '/tmp' must be an absolute, non-symlink path" }
```

On this host `/tmp` is a symlink to `/private/tmp`, and workspace registration
rejects symlinked roots. Both tests panic at their first `WorkspaceRegister`
call, before any controller-lease or queue code executes.

This was **measured, not assumed**: the same test was run in a clean detached
worktree at `HEAD` (`18c3dca92`) with none of these changes present, and it fails
there with the same error at the same line. Neither the test file nor any
workspace-registration code is in this diff (`git status --porcelain
tests/session_family/` is empty). It is a host-configuration artifact and is
reported here rather than presented as a green suite. The probe worktree and its
build directory were removed afterwards.

This is the same class of pre-existing environmental failure M001's closure
recorded (`InvalidRootPath("/tmp")`), which is why the check was worth running
rather than reasoning about.

### Commands the plan named that do not exist

`cargo test -p codegg --test turn_submit` (plan §11) fails with `no test target
named 'turn_submit' in 'codegg' package`. There is no such test target in this
repository. The area it was meant to cover — turn-admission authority after the
predicate refactor — is covered by `cargo test --test session_family`
(`tests/session_family/control_m004_controller_lease.inc`), which is what was
actually run for criterion 8. Recorded rather than silently substituted.

### Not run

Hosted CI was **not** run and is not claimed.

## 7. Where reality contradicted the plan

**The plan's `ControllerScope` had two variants; it has three.** Adoption repairs
a queue that has *no* owner row, so the queue-owner term is absent by definition
and cannot be the requirement — but routing `adopt` through `QueueMutation` would
refuse it with `QueueUnowned`, making the ADR's mandated repair path unreachable.
The added `QueueAdoption` variant asks the question adoption actually asks, and
its scope rule is "the turn lease, if any, is the only standing available".

This was found by a failing test, not by review: the mid-turn non-owner sweep
expected `NotQueueOwner` for `adopt` and got `ControllerMismatch`. The code was
wrong, not the test — a queue that *does* have an owner is a different situation
from one that does not, and reporting "you are not the turn controller" to a
caller who is simply not the owner answers a different question. `QueueAdoption`
now resolves ownership first and only falls through to the turn term when there
is genuinely no owner.

The named-variant requirement — which is what the plan was actually protecting by
rejecting `Option<&str>` — is satisfied more completely by three variants than by
two.

**Two real bugs the new tests caught**, both in code written earlier in this same
milestone:

1. `delete` read the surviving projection but never renumbered, so deleting a
   middle entry left a permanent gap (`[e1@0, e3@2, e4@3]`). A later `add`'s
   `max(ordinal) + 1` would keep growing past the survivors.
2. `is_permutation` had an inverted duplicate check — `!seen.insert(...)` accepts
   a duplicate instead of rejecting it, because `HashSet::insert` returns `true`
   when the value was *new*. Every reorder would have been refused.

Both are fixed. Neither reached a closure record as shipped code.

## 8. Recorded divergences and deliberate narrowings

- **Audit action divergence.** Every queue operation, including adoption and
  transfer, maps to `prompt_submit`. `session_control_transfer` maps to
  `membership_change`. The divergence is directed by ADR-0037, not an oversight,
  and is recorded in `architecture/authorization.md` rather than "corrected".
- **Read unchanged.** `user_prompt_queue_list` stays `Capability::SessionRead` and
  `UNINSTRUMENTED_OPERATIONS`. M002b does not widen it.
- **No prompt digest in audit records.** The plan said "identity, never text".
  The implementation records a byte *count* and deliberately no digest: a length
  is bounded, a digest of user prompt text is a content-correlation oracle.
- **`QueueAdoption` mid-turn requires the turn controller.** Slightly narrower
  than "any `agent.invoke` holder may repair an unowned queue", so a
  non-controller contributor cannot seize an unowned queue while someone else's
  turn runs.
- **`Absent` passes the daemon predicate** so first-add may create; the store still
  refuses `edit`/`delete`/`reorder` on an absent queue with `NotFound`.
- **A refused mutation rolls back its expiry sweep too.** That is the intended
  "no partial write" property, and it is why
  `user_prompt_queue_edit_never_resurrects_an_expired_entry` asserts the
  projection hides the entry rather than asserting the status column changed.

## 9. Boundaries held

- `is_controller_bypass` gained **no** queue authority. Its early return is
  guarded by `matches!(scope, ControllerScope::Turn { .. })`. Proven by execution.
- The turn controller lease keeps its meaning for turns; ADR-0007 and ADR-0035
  are unmodified.
- No queue mutation acquires a scheduler permit, executor ownership, or
  workspace execution context.
- No mutation is a second submission path. A queued entry becomes a turn only
  through ordinary `TurnSubmit` (M003).
- No refusal discloses an owner identity to a non-owner.
- No observer-allowed command reaches a mutation handler; the allowlist is
  fail-closed and `/queue` is deliberately absent (M004 owns registration).
- No shipped migration was edited. v80 is untouched.

## 10. M001 and M002a preservation

M001's uncommitted tree was not stashed, reverted, or reformatted. `cargo fmt`
was run only after M002a's files were known to be the only ones needing it, and
`cargo fmt --check` reports no remaining diff. M002a's 26 tests still pass
unchanged; its one edit was the layout assertion `80` → `81`, which is the
intended consequence of this milestone and is annotated in place.

## 11. Unresolved findings carried forward

1. **The v80 `status` `CHECK` is still unreachable for three values.**
   `delivering`, `delivered`, and `failed` have no producer until M003. SQLite
   cannot `ALTER` a `CHECK`, so M003 must either make all five reachable or
   rebuild the table. This milestone did not widen or rebuild it, by the plan's
   own stop condition.
2. **`run_at_ms` is now v82.** ADR-0037's pre-acceptance correction moved the
   owner binding out of v80 into v81, so M007's delayed-delivery column takes
   **v82**. `architecture/session.md` was corrected from v81 to v82 accordingly.
3. **The local interactive owner's queue authority is unexercised in practice.**
   In a solo local session the local owner is the only principal and therefore
   always the owner, so the bypass guard is only distinguishable in a team
   fixture. The test constructs that fixture deliberately; a future change that
   removed the guard would pass every solo test and fail only there.
4. **`Unowned` remains unreachable through product code.** It is covered only by
   tests that build the state directly. That is per ADR-0037, but it means the
   repair path has no production caller yet.
5. **Concurrent-transfer coverage tolerates a SQLite busy refusal.** The
   deterministic fence proof is the sequential two-transfer test sharing one
   observed revision; the concurrent test asserts the invariant (never two
   owners) rather than an exact winner count.

## 12. Unblock audit

The registry's Blocked work section for this workstream was:

> M002b — queue mutation operations and their authority. Blocked on ADR-0037
> (idle-session mutation authority), which was `proposed` and confers no
> implementation authorization.

ADR-0037 was accepted with Option B on 2026-10-18 before work began. This
milestone is therefore unblocked and closed, and that Blocked-work row is
retired in favour of an M003–M008 fresh-planning row.

Still blocked, and untouched here:

- **ADR-0036** remains `proposed` and gates M006 (steer frontend / send-now) and
  M007's copy boundary. Accepting ADR-0035 and ADR-0037 did not touch it.
- **M003** was gated on M002b's closure and is now unblocked.
- `tui-panel-surface-convergence` M004's shared-chrome helper remains a soft gate
  on the queue's own M004.
- Desktop parity (M008) remains explicitly deferrable.

## 13. Next milestone

**M003 — delivery into turns.** It inherits finding 1 (the v80 `status` `CHECK`
vocabulary), which is now a decision M003 must make rather than an observation.