# Delegated Run Observability and Recovery Corrective Milestone 002 — A Durable Transcript for a Delegated Run

Status: ready for handoff

Repository baseline: `a21b5fffa` on `fix/tui-composer-helix-parity-post-closure-corrective` (2026-10-24). Revalidate the actual implementation head before editing.

Source roadmap: `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md` §7 (C002)

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/001-terminology-and-domain-model.md` — agent run, delegated run, session

Applicable ADRs:

- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` — **accepted (option B) on 2026-10-24; this milestone's gate, now discharged.** C002 implements option B of that ADR: a dedicated durable store keyed by run id, redacted at write, bounded per row and per run.
- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md` (accepted) — unchanged.
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` (accepted) — unchanged; C002 adds a store, not a projection.

Primary class: infrastructure

## 1. Objective

Make a delegated run's conversation a durable, bounded, redacted artifact, keyed by the run id.

One bounded outcome: the messages a delegated run produces are written to a dedicated store, redacted **before** they are stored, bounded per row and per run, and are readable back by run id — with no `session` row created and no change to the session catalog.

This milestone delivers **infrastructure**. It is not a user-visible capability: nothing renders the transcript until C003, and nothing continues a run from it until C004.

## 2. Why this milestone is ready

**It is.** ADR-0051 was **accepted on 2026-10-24 as option B** — a dedicated store keyed by `AgentRunId`, redacted at write, bounded per row and per run. The gate this plan was blocked on is discharged.

- **The dependency was a decision, not an implementation, and it is now made.** ADR-0051's §"Decision" fixes the ownership boundary (a delegated transcript is owned by `codegg-core` and keyed by `AgentRunId`; a delegated run is not a session), the redaction point (at write, never at read), the bounds (per row and per run, with a recorded truncation marker), and what may cross the frontend seam. Option C (a real `session` row) was rejected, so §4's catalog invariant and §9's compatibility statement hold exactly as written.
- **Everything else is in place.** The storage layer, the migration mechanism, the redaction utilities the rest of the repository already uses, the projection seam and the session store's read path all exist.
- **The prerequisite implementation milestone, C001, is not a hard dependency of C002.** C002 may be implemented before or after C001; the two touch different files (`src/tool/task.rs` and `src/scheduler/` versus the store and the child's message path). C001 must nevertheless close before C003, because C003's user-visible claim depends on the orchestrator being told what happened.

## 3. Current implementation evidence

### 3.1 The child's conversation is per-attempt and in-memory

`src/agent/worker.rs:1765-1773` builds the child's messages as `[System, User(prompt)]` and lets the loop append to them. Nothing writes them.

### 3.2 The durable write is a deliberate no-op, for a stated reason

- `crates/codegg-core/src/session/store.rs:2321-2324`: `append_batch` "Returns `Ok(None)` when `session_id` has no `session` row. Subagent and other synthetic transcripts deliberately have no durable session, and `message.session_id` is a foreign key with `ON DELETE CASCADE`, so writing one would fail the constraint rather than create a transcript."
- `src/agent/turn_completion.rs:567-568`: "Synthetic transcripts (subagents, continuation tasks) have no durable session row; their output stays in the run journal."
- `src/agent/worker.rs:1545-1547`: the child's session id is synthetic — `{parent_id}-sub-{task_id}`.

That reason is correct **for the `message` table**. ADR-0051 option C would make it false by creating the `session` row; option B keeps it true and adds a different table.

### 3.3 The projection carries none of it, deliberately

`crates/codegg-protocol/src/projection/dto.rs:545-548` (`AgentRunSummary` "contains no prompt, transcript, mailbox body, hidden reasoning, credentials, or raw logs") and `crates/codegg-core/src/projection_replay/agent_runs.rs:92` (the adapter hardcodes `progress: None`). `src/tui/app/render.rs:1567-1572` states the position in the UI itself.

### 3.4 What the delegated run store already owns

`crates/codegg-core/src/agent_run.rs` already owns the delegated run's identity, its durable result (`agent_run_result.summary`, written by `src/scheduler/executors.rs:1464` and the `finish` call at `src/scheduler/executors.rs:1537-1539`), its status, its worktree and its validation evidence. The transcript is the missing sibling of that result.

### 3.5 Why the existing verification passed

No test asserts that a delegated run's messages are, or are not, persisted. The absence of the artifact was never a failure of a check; it was simply never asked for.

## 4. Invariants that must not regress

- **A delegated run is not a session.** No `session` row is created; the session catalog, project binding, `/sessions`, `Ctrl+T` and session-scoped snapshots are unchanged. A test must prove the catalog is byte-identical before and after a delegated run.
- **Redaction happens at write.** An unredacted body is never durable. A later bug in a reader or a projection cannot expose it.
- **Bounds are enforced at write.** Per row and per run. A transcript is not a log. When the per-run bound is reached, appending stops and a **truncation marker** is recorded — the tail is never silently lost.
- **The store is append-only and ordinal-stable.** A transcript row is never rewritten; a concurrent append for one run receives a distinct ordinal.
- **`codegg-core` stays UI-, server-, plugin- and auth-free** (`scripts/check-core-boundary.sh`). The store, its redaction and its bounds belong in `crates/codegg-core`; the projection and the TUI belong outside it.
- **The child's authority is unchanged.** Writing a transcript grants the child nothing.
- **The existing `message` store is untouched.** No row is written to it for a delegated run, and no existing row is rewritten.
- **The delegated run's result store is untouched.** `agent_run_result` keeps its shape; the transcript is a sibling, not a replacement.

## 5. Scope

### In scope

- One additive migration adding a delegated-transcript store.
- The store's API: append with redaction and bounds; read back by run id; a recorded truncation marker; a bounded page read.
- The redaction rule, expressed once and applied at write.
- The write path: the child's messages are captured as they are produced, from the same place the child's loop already holds them.
- The read path: a bounded, ordered read of a run's transcript.
- Ownership in `codegg-core`, with the daemon as the caller.

### Explicitly out of scope

- **Any projection, protocol DTO or TUI surface.** C003 owns those.
- **Any continuation or resume.** C004 owns those.
- **Any change to the delegated run's identity, status, result store, worktree, sandbox or tool policy.**
- **Any change to the `message` store, the session store's public shape, or the session catalog.**
- **Writing the transcript into the `message` table.** That is ADR-0051 option C, which was not selected.
- **A retention or compaction product.** A bounded per-run transcript with a deletion path is in scope; a retention policy with an operator surface is not.
- **Reading a transcript back into a model.** That is C004.
- **The legacy delegated path's transcript.** The milestone must state whether the legacy path writes one; if it does not, the guarantee is scoped to the durable path and the documentation says so.

## 6. Required production changes

### Core/domain

- **A new store in `crates/codegg-core`**, alongside `agent_run.rs`, with a trait in the shape the repository already uses for `AgentRunStore` (an object-safe trait with a SQL-backed implementation and an in-memory implementation for tests).
- **A transcript row**: `(run_id, ordinal, role, kind, redacted_body, byte_len, created_at)`.
  - `role` distinguishes the orchestrator-visible channel from the child's internal channel.
  - `kind` distinguishes a model message, a tool call, a tool result and a terminal note.
  - `redacted_body` is the **post-redaction** text. `byte_len` is the post-redaction length, so a bound check reads the stored size.
- **An append API** that takes the *un*redacted body, applies redaction, enforces the row bound, assigns the next ordinal atomically, and — when the run's bound is reached — stops appending and records a truncation marker instead.
- **A bounded read API**: `(run_id, from_ordinal, limit)` returning at most `limit` rows and a flag saying whether more exist. No unbounded read exists.

### Storage and migrations

- One additive migration. The table is created; nothing is rewritten and nothing is dropped.
- The migration must be safe to run against a database that already contains delegated runs: those runs simply have no transcript, and the absence is distinguishable from an empty transcript.
- A delegated run recorded **before** this milestone has no transcript. Reading it must yield a typed "no transcript recorded", never an empty list that reads like "the child said nothing".

### Protocol and DTOs

**None in this milestone.** No DTO crosses a wire in C002 — that is C003. The store's read API is a Rust API.

### Runtime and concurrency

- **The capture point.** The child's loop already holds its messages; the write must happen where they are produced, not by re-deriving them afterwards. The milestone must identify that point and prove the captured body is the same text the child acted on.
- **Concurrency.** Two appends for one run must not receive the same ordinal. The store must assign ordinals atomically (a transaction with an update, or an equivalent) rather than by a read-then-write.
- **Bounded work per append.** The redaction and the bound check must be O(body), not O(run), so a long run cannot make each append progressively more expensive.

### Frontend or operator surface

None. No projection, no DTO, no TUI change. The only visible effect of this milestone is that the transcript exists.

### Security and authorization

- **Redaction at write.** The redaction rule must cover at minimum: credentials and tokens in the shapes the repository already recognises, absolute environment values, and unbounded tool output. The repository already has secret-scanning helpers; the milestone should reuse them rather than invent a second recogniser.
- **Tool output is bounded at capture**, not at read. A tool result of unbounded size must be truncated at the point it is captured, and the truncation must be marked in the row.
- **No credentials, environment values or raw provider reasoning** in a stored body.
- **Authorization is unchanged.** The store is written by the daemon on the child's behalf and read by the daemon on a parent's behalf. This milestone adds no new read path across a trust boundary — that is C003, and ADR-0051 §"Security and reliability implications" already requires the projection seam to be treated as one.

### Documentation and static guards

- `architecture/agent.md` — what a delegated transcript is, where it lives, and that it is redacted at write.
- `architecture/codegg_core.md` — the new store's ownership.
- `architecture/session.md` — the explicit statement that a delegated run is **not** a session and does not appear in the catalog.
- **Static guard (required)**: a check that the delegated transcript store's declared bounds and redaction entry points are actually used in production, failing when a declared field is never written. It carries a `--self-test` and is wired into both `scripts/verify.sh` and `.github/workflows/ci.yml`.

## 7. Ordered work packages

### Work package A — Confirm the ADR's option, then implement only that

Intent: the ADR proposed three options. Implementation must follow the accepted one and no other.

Required changes: re-read the accepted ADR; if it was accepted as option B, implement a dedicated store; if it was accepted as option C, implement the `session`-row approach and the session-catalog consequences that option implies — and re-open this plan's §4 invariant about the catalog, because option C changes it by construction.

Acceptance evidence: the closure record names the accepted option and the ADR's acceptance commit.

### Work package B — The store, the migration and the bounds

Intent: a durable, bounded, ordered store.

Required changes: per §6 "Core/domain" and "Storage and migrations".

Acceptance evidence: a test that appends N rows and reads them back in ordinal order; a test that the per-row bound truncates and marks; a test that the per-run bound stops appending and records a truncation marker.

### Work package C — Redaction at write

Intent: nothing unredacted is ever durable.

Required changes: per §6 "Security and authorization".

Acceptance evidence: a test that writes a known-secret-shaped body and asserts the secret is absent from the **stored row** — not merely absent from a rendered view. This distinction is the whole point of redacting at write.

### Work package D — The capture path

Intent: the transcript is the conversation the child actually had.

Required changes: per §6 "Runtime and concurrency".

Acceptance evidence: a test that the stored transcript contains the child's prompt and its final response, and matches the text the child's own turn observed.

### Work package E — Concurrency and restart

Intent: two appends, one ordinal each; a crash mid-append leaves no partial row.

Required changes: per §6 "Runtime and concurrency".

Acceptance evidence: a concurrency test that appends from two tasks and observes distinct ordinals with no lost row; a restart test that a partially written append leaves no readable row.

### Work package F — Docs, guard and closure

Intent: the repository states the truth and cannot drift back.

Required changes: the documentation updates in §6; the guard with its `--self-test`, wired into `scripts/verify.sh` and `.github/workflows/ci.yml`.

Acceptance evidence: `bash scripts/verify.sh quick` exits 0; the guard's `--self-test` proves non-vacuity.

## 8. Failure, cancellation, restart, and contention semantics

- **A crash mid-append.** No partial row is readable; the transcript is short by one message and the run's recorded result is unaffected.
- **Two concurrent appends for one run.** Distinct ordinals, no lost row, no duplicate ordinal.
- **The per-run bound is reached.** Appending stops; a truncation marker is recorded; the transcript states that it is truncated. It must never read as complete.
- **The delegated run is cancelled.** The transcript holds what the run produced before cancellation; the cancellation is a terminal note in the transcript.
- **The delegated run fails.** The transcript holds the conversation that led to the failure — which is precisely what makes the failure diagnosable and, in C004, continuable.
- **A redaction rule changes.** Rows already written are not rewritten; the transcript is append-only. A rule change therefore applies to later runs only, and the closure record must say so.
- **A run has no transcript** (recorded before this milestone, or the legacy path). The read yields a typed "no transcript recorded", distinguishable from an empty transcript.

## 9. Compatibility and migration

- **Additive.** One migration creates one table. No existing table is rewritten; no row is removed; no backfill is required or permitted.
- **No protocol change.** Nothing crosses a wire in this milestone, so an older client is unaffected by construction.
- **No session-catalog change.** No `session` row is created for a delegated run.
- **`agent_run_result` is unchanged.** The transcript is a sibling of the recorded result, not a replacement for it.
- **The `message` store is unchanged.** `append_batch`'s `Ok(None)` behaviour for synthetic sessions is preserved exactly; this milestone does not make synthetic transcripts writable in that table.
- **Degradation.** If the store is unavailable, the delegated run still runs and still records its result. A transcript write failure must not fail the delegated work; it must be logged and must be visible.

## 10. Required tests

### 10.1 Focused unit tests — `crates/codegg-core`

- Append, read back in ordinal order.
- The per-row bound truncates and marks the truncation.
- The per-run bound stops appending and records the truncation marker.
- Redaction removes a known-secret-shaped body from the **stored row**.
- A concurrent append yields a distinct ordinal; no row is lost.
- A read is bounded: `limit` is honoured and the "more exist" flag is correct.

### 10.2 Integration tests — `tests/agent_loop/`, `tests/session_family/`

- A delegated run's transcript contains its prompt and its final response.
- The transcript contains the child's tool calls and tool results, bounded, with truncation marked.
- **The session catalog is unchanged** — a delegated run appears in no session enumeration, and the catalog is identical before and after.
- A run with no transcript reads as "no transcript recorded", not as an empty transcript.
- A transcript write failure does not fail the delegated run.

### 10.3 Restart and recovery tests

- A crash mid-append leaves no partial row and does not corrupt the run's recorded result.
- A daemon restart reads back a previously written transcript intact.

### 10.4 Migration and compatibility tests

- The migration applies cleanly to a database that already contains delegated runs.
- An older client is unaffected (nothing new crosses a wire).

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg-core -- transcript
cargo test --test agent_loop -- <member>
cargo test --test session_family -- <member>

# static guards
bash scripts/check-core-boundary.sh
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

`bash scripts/verify.sh quick` is the canonical gate and must exit 0. Never run `cargo test --workspace`, a bare `cargo test`, or anything with `--all-features`.

## 12. Documentation updates

- `architecture/agent.md` — the delegated transcript: what it is, where it lives, that it is redacted at write and bounded at write.
- `architecture/codegg_core.md` — the new store's ownership.
- `architecture/session.md` — the explicit statement that a delegated run is **not** a session and never enters the catalog.
- `architecture/projection.md` — a forward reference that C003 will extend it; this milestone projects nothing.

## 13. Acceptance criteria

1. A delegated run's messages are durably present, in ordinal order, readable by run id.
2. A known-secret-shaped body is provably absent from the **stored row**.
3. The per-row and per-run bounds are enforced at write, and truncation is recorded rather than silent.
4. No `session` row is created; the session catalog is provably unchanged.
5. A run with no transcript is distinguishable from an empty transcript.
6. A transcript write failure does not fail the delegated run and is visible in the daemon log.
7. The store lives in `codegg-core` and passes `scripts/check-core-boundary.sh`.
8. Nothing crosses a wire in this milestone; no protocol change.
9. `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- **ADR-0051 was accepted as something other than option B** — the whole plan is written for option B and must be renegotiated rather than bent. (Discharged: accepted as option B on 2026-10-24.)
- the redaction rule cannot be expressed against the shapes the repository already recognises without inventing a second secret recogniser;
- the per-run bound cannot be enforced without rewriting existing rows — the store is append-only;
- capturing the transcript would require the child to hold a different conversation than the one it acted on;
- the change would require exposing the transcript across a trust boundary — that is C003's scope and ADR-0051's projection contract;
- the change would expand into continuation or resume — that is C004's scope.

## 15. Closure evidence required

The closure record at `plans/closure/delegated-run-observability-and-recovery-corrective/002-status.md` must contain:

1. The accepted ADR option and its acceptance commit, named in §15 of ADR-0051.
2. The requirement-to-evidence matrix for §13, each row naming a **file:line** and a named test.
3. The **redaction evidence**: the stored-row assertion, quoted.
4. The **bounds evidence**: the truncation tests, quoted, including the per-run case.
5. The **catalog evidence**: the before/after session enumeration comparison.
6. The exact commands run and their real results, labelled local versus CI.
7. The guard's `--self-test` output.
8. An invariant review against §4, item by item.
9. A failure-and-recovery review against §8.
10. Any deviation from this plan, recorded as a deviation.
11. The registry updates and the **unblock audit**: C003 becomes eligible when C002 closes **and** ADR-0051 is accepted; C004 likewise. If ADR-0051 is still unaccepted at C002's closure, the audit says so and both stay blocked.

## 16. Handoff notes

- **Blocked.** Do not begin implementation until ADR-0051 is accepted and the acceptance is recorded in the ADR's own file.
- **Storage shape.** Follow the repository's existing store patterns (`crates/codegg-core/src/agent_run.rs` is the closest sibling). `codegg-core` must stay free of UI, server, plugin and auth dependencies — `scripts/check-core-boundary.sh` enforces it and runs in `verify.sh quick`.
- **Redaction at write, not at read.** This is the load-bearing property of the milestone. A read-time redaction leaves the secret durable and fails the milestone's central acceptance criterion.
- **Append-only.** A transcript row is never rewritten. A redaction-rule change applies to later runs only.
- **Test conventions.** Root integration tests are **families**: `cargo test --test <family> -- <member>`. Storage tests use `isolated_pool()`; migrations run inside it, so do not add an extra `migrate()`.
- **New `#[tokio::test]`s** default to `current_thread`; use `flavor = "multi_thread", worker_threads = 2` only for real concurrency.
- **Assertion quality.** Every new test body must assert a checkable property or delegate to a named helper that does, or be recorded in `docs/test-assertion-quality.toml`. `scripts/check_test_assertion_quality.py` and `scripts/check_test_duplication.py` both run in `verify.sh quick`.
- **Scan Rust with `scripts/rust_test_scan.py`**, never with a regex.
- **Budget.** `CARGO_BUILD_JOBS=3` while iterating; 8 for the final workspace clippy.
- **Comment budget.** `scripts/check_comment_budget.py` is a ratchet: re-bank **after** `cargo fmt --all`, and the bank diff must stay confined to the files this milestone touches plus globals.
- **Guard wiring.** The new guard must appear in **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`.
