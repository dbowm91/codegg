# Delegated Run Observability and Recovery Corrective Milestone 002 — Closure Status

Status: **conditionally closed** — the implementation and its evidence are
complete; the canonical gate's remaining failures are the shared
comment-budget ratchet and the `cargo fmt` drift in files this milestone does
not own. Both are named in §10 with the exact outstanding action.

Source implementation plan:

- `plans/implementation/delegated-run-observability-and-recovery-corrective/002-a-durable-transcript-for-a-delegated-run.md`

Source subsystem roadmap:

- `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md`
  (the "C002 — A durable transcript for a delegated run" milestone)

Repository baseline reviewed: `4803e093c` — the commit that **accepted
ADR-0051 as option B**.

Implementation commits or pull requests:

- *none* — this milestone's changes are left in the working tree, uncommitted
  and unpushed, by instruction.

---

## 0. The accepted decision

| Item | Value |
|---|---|
| ADR | `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` |
| Accepted option | **Option B** — a dedicated durable store keyed by `AgentRunId`, redacted **at write**, bounded per row and per run, with a recorded truncation marker |
| Acceptance date | 2026-10-24 |
| Acceptance commit | `4803e093c` |
| Rejected option | Option C (a real `session` row per delegated run) — **not** implemented, so the plan's §4 catalog invariant and §9 compatibility statement hold exactly as written |

Acceptance grants the *authority*, not the implementation. This milestone
produces that implementation and the ADR's evidence.

---

## 1. Executive finding

The delegated run now has a durable transcript, and the transcript's two
load-bearing properties — **redaction at write** and **bounds at write** — are
enforced by the store itself rather than assumed.

The store lives in `codegg-core`
(`crates/codegg-core/src/agent_run_transcript.rs`), is keyed by the run's
durable `AgentRunId`, and is the sibling of the run's existing durable result
rather than a replacement for it. A delegated run is still **not** a session:
no `session` row is created, the catalog is provably unchanged, and the
existing `message` store's `append_batch` → `Ok(None)` behaviour for synthetic
transcripts is preserved exactly.

The child's conversation is captured where the child's messages are produced
(`src/agent/worker.rs`), so the stored body is the text the child acted on.
The capture is **degradable**: a transcript write failure is logged and the
delegated work still completes and still records its result.

The child's **executed tool results** are in the transcript too. They are
sourced from the outcomes the loop actually executed, not from the child's
event stream: no provider adapter constructs a tool-result event, so a
transcript read from the event stream would carry the tool *calls* and no
tool *outputs* at all. The loop now retains the executed outcomes per run,
bounded, and the capture path writes one row per outcome. The capture path
does not touch the child's event vector, so the parent's "a tool call with no
matching result is `Pending`" is unchanged.

Nothing crosses a wire in this milestone. The store's read API is a Rust API
only; the projection and the TUI are C003's scope, and `architecture/projection.md`
carries a forward reference only.

The implementation is complete, and the D5 corrective that closed the one
capability gap the earlier round named is in. The canonical gate is not yet
green for reasons this milestone does not own: the comment-budget ratchet
(which the orchestrator re-banks after all workers land). See §10.

---

## 2. Requirement-to-evidence matrix (plan §13)

| # | Requirement | Evidence (`file:line`, named test) | Result |
|---|---|---|---|
| 1 | A delegated run's messages are durably present, in ordinal order, readable by run id | `crates/codegg-core/src/agent_run_transcript.rs:404` (`SqliteAgentRunTranscriptStore::append`), `:518` (`read`); `crates/codegg-core/src/agent_run_transcript.rs:763` `appended_rows_read_back_in_ordinal_order`; capture at `src/agent/worker.rs:1876` (system + prompt) and `src/agent/worker.rs:1979` (final response) | pass |
| 2 | A known-secret-shaped body is provably absent from the **stored row** | `crates/codegg-core/src/agent_run_transcript.rs:909` `a_known_secret_is_absent_from_the_stored_row` — reads the row with raw SQL (`crates/codegg-core/src/agent_run_transcript.rs:744`), not through the read API | pass |
| 3 | The per-row and per-run bounds are enforced at write, and truncation is recorded rather than silent | `crates/codegg-core/src/agent_run_transcript.rs:805` `the_per_row_bound_truncates_and_marks_the_stored_row`; `crates/codegg-core/src/agent_run_transcript.rs:834` `the_per_run_bound_stops_appending_and_records_a_truncation_marker`; integration: `tests/agent_loop/delegated_transcript.inc:483` `a_delegated_run_records_a_bounded_tool_result_with_truncation_marked` | pass |
| 4 | No `session` row is created; the session catalog is provably unchanged | `tests/session_family/delegated_transcript_catalog.inc:206` `a_delegated_run_appears_in_no_session_enumeration_and_leaves_the_catalog_unchanged`; `src/agent/turn_completion.rs:567` (the corrected comment) | pass |
| 5 | A run with no transcript is distinguishable from an empty transcript | `crates/codegg-core/src/agent_run_transcript.rs:172` (`AgentRunTranscriptRead::Absent` / `::Present`); `crates/codegg-core/src/agent_run_transcript.rs:1046` `a_run_without_a_transcript_reads_as_absent_not_as_an_empty_transcript` | pass |
| 6 | A transcript write failure does not fail the delegated run and is visible in the daemon log | `src/agent/worker.rs:1031` (`capture_transcript` logs at `warn` and returns); `tests/agent_loop/delegated_transcript.inc:616` `a_transcript_write_failure_does_not_fail_the_delegated_run` | pass (log visibility is by construction + `tracing::warn!`; not separately asserted) |
| 7 | The store lives in `codegg-core` and passes `scripts/check-core-boundary.sh` | `crates/codegg-core/src/agent_run_transcript.rs`; `./scripts/check-core-boundary.sh` → `codegg-core boundary check passed` | pass |
| 8 | Nothing crosses a wire in this milestone; no protocol change | No file under `crates/codegg-protocol/` was touched; no DTO, event or snapshot was added. The store's read API is Rust-only (`crates/codegg-core/src/agent_run_transcript.rs:209`) | pass |
| 9 | `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean | clippy: **clean, exit 0**. `verify.sh quick`: **exit 1** at the comment-budget ratchet, whose every remaining violation is named in §10. The `cargo fmt` failure recorded by the earlier round is **resolved** — `cargo fmt --all -- --check` now exits 0 (the concurrent worker formatted their files). The 112 guard commands and the workspace `cargo check` were run directly. See §4 and §10 | **partial** — see §10 |
| 10 | **The child's executed tool results reach the transcript** (the D5 corrective) | The loop retains the executed outcomes (`src/agent/loop.rs:90` `ExecutedToolOutcome`, `:102` `ExecutedToolOutcomes`, `:109` `bound_retained_tool_output`, `:776` `AgentLoop::executed_tool_outcomes`); the capture path writes one `ToolResult` row per outcome (`src/agent/worker.rs:1984`). Integration: `tests/agent_loop/delegated_transcript.inc:420` `a_delegated_run_records_the_tool_result_its_child_executed` (the tool's name **and** its output are in the row), `:483` (the row is bounded and the truncation is marked), `:534` `a_delegated_run_redacts_a_secret_in_the_tool_output` (the secret is absent from the **stored row**), `:572` `a_delegated_run_that_executes_no_tool_records_no_tool_result` | pass |
| 11 | The call and its result are both present, in ordinal order, and pairable | `tests/agent_loop/delegated_transcript.inc:420` — the `ToolCall` row's ordinal is strictly less than the `ToolResult` row's, and both name `tool_call_id=call-1` | pass |
| 12 | The capture path does not change what the events vector holds | `src/agent/worker.rs:2324` `the_delegated_capture_leaves_a_tool_call_without_a_result_pending` (a tool call with no matching result is still stored as `Pending`); `scripts/check_delegated_transcript_store.py` rules `delegated-transcript-events-mutated` and `delegated-transcript-tool-result-constructed`, each proven non-vacuous by the guard's `--self-test` (§4.6) | pass |

---

## 3. Production implementation evidence

### 3.1 The store

| Property | Value | Location |
|---|---|---|
| Trait | `AgentRunTranscriptStore` — object-safe, `Send + Sync` | `crates/codegg-core/src/agent_run_transcript.rs:194` |
| SQL implementation | `SqliteAgentRunTranscriptStore` | `crates/codegg-core/src/agent_run_transcript.rs:301` |
| In-memory implementation | `InMemoryAgentRunTranscriptStore` | `crates/codegg-core/src/agent_run_transcript.rs:550` |
| The row | `(run_id, ordinal, role, kind, redacted_body, byte_len, created_at)` | `:121` (`AgentRunTranscriptEntry`) |
| The table | `agent_run_transcript` | `:323` (the production INSERT) |
| The per-run aggregate table | `agent_run_transcript_state` — next ordinal, stored bytes, truncation flag | `:421`; created by the migration |
| The migration | **v89**, additive | `crates/codegg-core/src/session/schema.rs:3723` (`migrate_v89`), wired at `:291` and `:394` |
| The layout marker | `STORAGE_LAYOUT_VERSION = 89` | `crates/codegg-core/src/storage/mod.rs:53` |
| The row bound | `MAX_TRANSCRIPT_ROW_BYTES = 8 KiB` | `:32` |
| The run bound | `MAX_TRANSCRIPT_RUN_BYTES = 256 KiB` | `:35` |
| The read bound | `MAX_TRANSCRIPT_READ_LIMIT = 256` rows | `:38` |
| The in-row truncation marker | `TRANSCRIPT_ROW_TRUNCATION_MARKER` | `:42` |
| The per-run truncation marker | `TRANSCRIPT_RUN_TRUNCATION_MARKER` | `:46` |
| `role` | `Orchestrator` (orchestrator-visible) / `Internal` (the child's own channel) | `:60` |
| `kind` | `ModelMessage` / `ToolCall` / `ToolResult` / `TerminalNote` | `:87` |

### 3.2 Where redaction happens

**At write, before the body reaches the store.** The single entry point is
`redact_transcript_body` (`crates/codegg-core/src/agent_run_transcript.rs:226`),
reached through the shared preparation helper `prepare_transcript_row`
(`:259`), which **both** store implementations call from `append`.

The recogniser is the repository's existing `ProjectionFieldRedactor` — the
same one the projection seam (`crates/codegg-core/src/projection_replay/redactor.rs`)
and the durable run's failure message already use. **No second secret
recogniser was invented.** Two passes are applied to every body: the kind's
own rule set (`ToolArgument` for a tool call, `ToolOutput` for a tool result,
`Text` for a model message and a terminal note) and the `Environment` rule
set, so an environment assignment has its **value** replaced wherever it
appears rather than only where it is spelled `KEY=value` at the start of a
line. The pass **fails closed**: a redactor refusal or an over-long input
yields a marker, never the original text.

The order of operations is deliberate: redaction runs over a prefix large
enough to hold a credential whole (`scan_prefix`, `:271`), and the row bound
is applied to the **redacted** text (`bound_row_body`, `:285`). A stored
prefix therefore cannot contain a half-redacted secret that the row bound cut
in half.

### 3.3 The capture path

`src/agent/worker.rs`:

| Capture | Location | Channel / kind |
|---|---|---|
| The child's system message | `:1880` | `Internal` / `ModelMessage` |
| The child's prompt | `:1888` | `Orchestrator` / `ModelMessage` |
| The child's tool calls | `:1972` | `Orchestrator` / `ToolCall` |
| The child's tool results | `:1984` | `Orchestrator` / `ToolResult` |
| The child's final response | `:2027` | `Orchestrator` / `ModelMessage` |
| Cancellation (scheduler / shutdown / root lineage) | `:1136`, `:1154`, `:1170` | `Orchestrator` / `TerminalNote` |
| Failure | `:1235` | `Orchestrator` / `TerminalNote` |

The prompt and the system message are captured **where the child's messages
are produced** (`src/agent/worker.rs:1880`), from the same source values that
become the child's `Message::System` and `Message::User`. The final response
is captured as the same `output` string the run records as its result.
`ChatEvent::ReasoningDelta` is deliberately **not** captured: raw provider
reasoning is not part of a transcript.

The tool calls and the tool results come from **two different sources**, and
the difference is the whole of the D5 corrective:

- The **tool calls** are the child's own turn events (`ChatEvent::ToolCall`),
  so the call row names the call and the arguments the child asked with.
- The **tool results** are the outcomes the loop **executed**
  (`src/agent/loop.rs`, `ExecutedToolOutcomes`), reached through
  `AgentLoop::executed_tool_outcomes()` (`src/agent/loop.rs:776`). They are
  **not** read from the event stream, because no provider adapter emits a
  tool-result event: the child's event vector carries the calls, and the
  outputs exist only where the tools ran. A capture that read the outputs
  from the event stream would record a transcript whose tool results are
  empty in production.

The retained output is the **path-redacted** text the loop published and the
model received (`redact_local_paths` over the outcome's `model_text`), kept
as a bounded excerpt: a head, the truncation marker, and a tail
(`src/agent/loop.rs:118`). The tail matters — a tool's conclusion is at the
end of its output (the `CODEGG_VALIDATION_RESULT:` marker the `test` tool
appends is its last line), so a head-only cut would discard exactly the line
a caller reads the output for. The row also carries the loop's own
truncation flag as a field (`output_truncated=`), ahead of the output, so a
later and tighter row bound cannot cut the flag away.

The capture path **never writes into the child's event vector**. That vector
is what the parent's turn transcript is built from, so a tool call the
provider emitted with no matching result must stay `Pending`; a capture path
that appended a synthetic tool-result event would turn that call into a
completed one and claim an outcome that never happened. Two static rules
enforce it (`scripts/check_delegated_transcript_store.py`,
`delegated-transcript-events-mutated` and
`delegated-transcript-tool-result-constructed`), and the behaviour is
asserted by a unit test (§4.2).

`delegated_transcript_store` (`src/agent/worker.rs:1019`) resolves the store
only when the request carries a durable `AgentRunId` **and** a pool is
available. `capture_transcript` (`src/agent/worker.rs:1031`) logs a failed
write at `warn` with the run id and the kind, and returns — the delegated work
is never failed by a transcript write.

`extract_validation_evidence` (`src/agent/worker.rs:2044`) reads the same
retained outcomes, so a delegated run's recorded `agent_run_result.validation`
is now populated from the outputs the child actually saw. It was always empty
before, because the branch it read was the dead event-stream branch.

### 3.4 Concurrency and cost

The ordinal and the per-run byte total are read and advanced in **one**
`BEGIN IMMEDIATE` transaction (`crates/codegg-core/src/agent_run_transcript.rs:417`),
so two concurrent appends for one run cannot receive the same ordinal and
neither row is lost. The per-run aggregate lives in
`agent_run_transcript_state`, so the bound check and the ordinal assignment
are **O(1) per append** rather than O(run): a long run does not make each
append progressively more expensive.

### 3.5 What is *not* implemented here

- **No protocol change, no DTO, no projection.** The projection and the TUI
  are C003's scope. `architecture/projection.md` carries a forward reference
  only.
- **No continuation or resume.** C004's scope.
- **No `session` row, no session enumeration, no project binding, no
  `/sessions`, no `Ctrl+T`, no session-scoped snapshot.**
- **No change to `agent_run_result`'s shape, to the `message` store, or to
  `append_batch`'s `Ok(None)` behaviour.** The D5 corrective changes the
  **contents** of `agent_run_result.validation` — it is now populated from
  the tools the child actually ran, where it was always empty because the
  branch it read was the dead event-stream branch. The durable schema is
  unchanged; the evidence in the record is not (D5c).

---

## 4. Verification executed

All commands were run **locally** on the milestone worktree
(`/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261023-69770c99`),
Rust 1.89, edition 2021, macOS (darwin 25.6.0 arm64). **No CI run was
performed for this milestone**; CI wiring of the new guard is the
orchestrator's (§10).

### 4.1 Commands run

```bash
# the store's own unit tests
CARGO_BUILD_JOBS=3 cargo test -p codegg-core --lib -- agent_run_transcript

# the integration families
CARGO_BUILD_JOBS=3 cargo test --test agent_loop -- delegated_transcript
CARGO_BUILD_JOBS=3 cargo test --test session_family -- delegated_transcript_catalog

# the capture path's own unit tests (the D5 corrective)
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib -- agent::worker

# the new static guard, and its self-test
python3 scripts/check_delegated_transcript_store.py
python3 scripts/check_delegated_transcript_store.py --self-test

# the canonical gate
bash scripts/verify.sh quick

# the gate's steps, run directly because `verify.sh quick` stops at the
# first failing step
cargo fmt --all -- --check
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -oE '\(cd "\$REPO_ROOT" && [^)]*\)' \
  | sed 's/(cd "\$REPO_ROOT" && //; s/)$//' | grep -v "^cargo fmt"   # 112 guard commands
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked

# the lint gate
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=8 cargo clippy -p codegg --all-targets --locked -- -D warnings

# the storage-layout marker
python3 scripts/check_project_catalog_invariants.py
```

### 4.2 Results

| Command | Result |
|---|---|
| `cargo test -p codegg-core --lib -- agent_run_transcript` | **10 passed; 0 failed** (935 filtered out) |
| `cargo test --test agent_loop -- delegated_transcript` | **6 passed; 0 failed** (536 filtered out) — 2 from the original milestone, 4 from the D5 corrective |
| `cargo test --test session_family -- delegated_transcript_catalog` | **1 passed; 0 failed** (97 filtered out) |
| `cargo test -p codegg --lib -- agent::worker` | **16 passed; 0 failed** (7469 filtered out) — includes `the_delegated_capture_leaves_a_tool_call_without_a_result_pending` |
| `python3 scripts/check_delegated_transcript_store.py` | **exit 0** — "the declared bounds and the redaction entry point are live, and every declared field is written" |
| `python3 scripts/check_delegated_transcript_store.py --self-test` | **exit 0** — **6** injected defects each rejected; see §4.6 |
| `bash scripts/verify.sh quick` | **exit 1** at the comment-budget ratchet. The `cargo fmt` failure the earlier round recorded is **resolved**: the gate now passes `cargo fmt --check --all` and runs **every** guard in `run_guards()`; the first and only failing guard is `python3 scripts/check_comment_budget.py --self-test` ("the unmodified tree must pass the guard: R4 the corpus holds 124716 comment lines, over the recorded ceiling of 124310"). The step aborts the script, so the workspace check was run directly (below). |
| `cargo fmt --all -- --check` | **exit 0** — clean. The earlier round's four files (the concurrent worker's) are formatted. |
| 112 guard commands from `run_guards()` (minus `cargo fmt`) | **110 pass; 2 fail** — `scripts/check_comment_budget.py` and its `--self-test` (the self-test fails because the tree is over the recorded ceiling). The failure is the shared ratchet: **23 violations**, of which this milestone contributes **8**, across five files: `crates/codegg-core/src/agent_run_transcript.rs` (105 comment lines, an unlisted file that needs a bank entry), `crates/codegg-core/src/session/schema.rs` (467 vs a ceiling of 464), `crates/codegg-core/src/storage/mod.rs` (103 vs 100), `src/agent/worker.rs` (343 vs 311), `src/agent/turn_completion.rs` (70 vs 67) and `src/agent/loop.rs` (357 vs 306). The concurrent worker contributes the remaining 15, and the global corpus is over its ceiling. `docs/comment-budget.toml` is the orchestrator's to re-bank and was **not** edited. |
| `cargo check --workspace --all-targets --locked` | **exit 0** |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **exit 0**, no errors and no warnings |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | **exit 0** — re-run after the D5 corrective |
| `python3 scripts/check_project_catalog_invariants.py` | **7/7 checks passed** — "storage layout marker tracks highest wired schema migration" |

### 4.3 The redaction evidence — the secret is absent from the **stored row**

`crates/codegg-core/src/agent_run_transcript.rs:909`
(`a_known_secret_is_absent_from_the_stored_row`). The row is read with raw
SQL, **not** through the store's read API, so the assertion is about the
durable bytes and not about a rendered view:

```rust
        let stored = stored_body(&pool, &run_id, 0).await;
        for secret in [api_secret, bearer_secret, env_secret] {
            assert!(
                !stored.contains(secret),
                "the secret must be absent from the stored row, got: {stored:?}"
            );
        }
        assert!(
            stored.contains("[REDACTED"),
            "the stored row must show that a redaction happened, got: {stored:?}"
        );
```

and the reader it uses (`crates/codegg-core/src/agent_run_transcript.rs:744`):

```rust
    async fn stored_body(pool: &SqlitePool, run_id: &AgentRunId, ordinal: i64) -> String {
        sqlx::query_scalar(
            "SELECT redacted_body FROM agent_run_transcript WHERE run_id = ? AND ordinal = ?",
        )
```

The three secret shapes are the repository's own: an `api_key=` assignment, a
`Bearer` token, and an `AWS_SECRET_ACCESS_KEY=` environment assignment.

### 4.4 The bounds evidence

**The per-row bound** — `crates/codegg-core/src/agent_run_transcript.rs:805`
(`the_per_row_bound_truncates_and_marks_the_stored_row`):

```rust
        assert_eq!(outcome.status, TranscriptAppendStatus::RowTruncated);
        assert_eq!(
            outcome.byte_len, MAX_TRANSCRIPT_ROW_BYTES as i64,
            "the stored row must be bounded by the per-row limit"
        );
        let stored = stored_body(&pool, &run_id, 0).await;
        assert!(
            stored.ends_with(TRANSCRIPT_ROW_TRUNCATION_MARKER),
            "a truncated row must carry the truncation marker, got: {stored:?}"
        );
```

**The per-run bound** — `crates/codegg-core/src/agent_run_transcript.rs:834`
(`the_per_run_bound_stops_appending_and_records_a_truncation_marker`):

```rust
        let refused = outcomes.last().expect("the run bound must be reached");
        assert_eq!(
            refused.status,
            TranscriptAppendStatus::RunTruncated,
            "the append past the per-run bound must be refused, not stored"
        );
        let read = page(
            store
                .read(&run_id, 0, MAX_TRANSCRIPT_READ_LIMIT)
                .await
                .unwrap(),
        );
        assert!(
            read.truncated,
            "the transcript must state that it is truncated"
        );
        let last = read.entries.last().expect("a truncation marker row");
        assert_eq!(last.kind, TranscriptKind::TerminalNote);
        assert_eq!(last.redacted_body, TRANSCRIPT_RUN_TRUNCATION_MARKER);
        assert!(
            read.total_bytes <= MAX_TRANSCRIPT_RUN_BYTES as i64,
            "the run must never store more than the per-run bound"
        );
```

A further append after the bound is refused and adds no row:

```rust
        assert_eq!(after.status, TranscriptAppendStatus::RunTruncated);
        assert_eq!(
            reread.entries.len(),
            read.entries.len(),
            "a refused append must not add a row"
        );
```

**Bounded tool output at capture** —
`tests/agent_loop/delegated_transcript.inc:483`
(`a_delegated_run_records_a_bounded_tool_result_with_truncation_marked`). The
child reads a file three times the per-row limit, so the tool's output
exceeds the loop's own retention bound and the row exceeds the store's:

```rust
    assert_eq!(
        tool_result.byte_len, MAX_TRANSCRIPT_ROW_BYTES as i64,
        "the tool result row must be bounded by the per-row limit"
    );
    assert!(
        tool_result
            .redacted_body
            .ends_with(codegg_core::agent_run_transcript::TRANSCRIPT_ROW_TRUNCATION_MARKER),
        "a bounded tool result must carry the truncation marker, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result.redacted_body.contains("output_truncated=true"),
        "a tool output the loop's own bound cut must say so, got: {:?}",
        tool_result.redacted_body
    );
```

The two truncations are **different facts** and both are recorded: the
loop's own retention cut the tool's output (`output_truncated=true`, and the
loop's marker between the retained head and tail), and the store's row bound
cut the row (the store's in-row marker at the end).

### 4.5 The catalog evidence

`tests/session_family/delegated_transcript_catalog.inc:206`
(`a_delegated_run_appears_in_no_session_enumeration_and_leaves_the_catalog_unchanged`):

```rust
    let before = catalog_ids(&session_store).await;
    assert_eq!(
        before.len(),
        1,
        "the catalog must not be empty to begin with"
    );
    ... the delegated run executes ...
    // The delegated run really happened: it wrote its transcript. Without
    // this, "the catalog is unchanged" would be true of a run that never
    // started.
    let store = SqliteAgentRunTranscriptStore::new(pool.clone());
    let transcript = store.read(&run_id, 0, 256).await.unwrap();
    assert!(
        matches!(transcript, AgentRunTranscriptRead::Present(_)),
        "the delegated run must have recorded a transcript"
    );

    let after = catalog_ids(&session_store).await;
    assert_eq!(
        after, before,
        "the session catalog must be identical before and after a delegated run"
    );
    let synthetic = format!("parent-session-sub-{task_id}");
    assert!(
        !after.contains(&synthetic),
        "a delegated run must appear in no session enumeration, found {synthetic}"
    );
```

`catalog_ids` (`:193`) enumerates **every** session the catalog holds
(`SessionStore::list_all_sessions(Some(256))`, sorted). The before/after
comparison is over the whole enumeration, not a sample. The catalog holds one
real session before the delegated run and exactly the same one after; the
delegated run's synthetic id `parent-session-sub-{task_id}` appears in no
enumeration.

### 4.6 The guard's `--self-test` output

```
$ python3 scripts/check_delegated_transcript_store.py --self-test
  ok   the unmodified delegated transcript store produced no finding
  ok   the per-row bound is declared but never read (rejected by delegated-transcript-bound-unused)
  ok   the redaction door is never called (rejected by delegated-transcript-redaction-unreachable)
  ok   a declared transcript field is never written (rejected by delegated-transcript-field-unwritten)
  ok   the migration does not create the transcript table (rejected by delegated-transcript-migration-missing)
  ok   the capture path writes into the child's event vector (rejected by delegated-transcript-events-mutated)
  ok   the capture path constructs a synthetic tool result (rejected by delegated-transcript-tool-result-constructed)
delegated-transcript-store self-test passed: 6 injected defects were each rejected by the guard, and the unmodified store produced no finding
```

The last two cases are the D5 corrective's own rules. They are not vacuous:
the guard rejects a capture path that pushes onto the child's event vector,
and a capture path that constructs a `ChatEvent::ToolResult` — the two ways a
capture path could turn the parent's "a tool call with no matching result is
`Pending`" into a completed call that claims an outcome which never
happened.

The guard is **not vacuous**: it proves the unmodified tree passes, and that
each of six independently injected defects is rejected by the check that owns
it.

### 4.7 The D5 corrective's evidence — the executed tool results

**The tool's name and its output are in the stored row** —
`tests/agent_loop/delegated_transcript.inc:420`
(`a_delegated_run_records_the_tool_result_its_child_executed`). The child's
workspace is a temp directory holding one file, and the child issues a real
`read` of it; the tool **executes** (a child without a workspace identity has
every tool refused, so the test supplies one — see `ChildWorkspace`,
`tests/agent_loop/delegated_transcript.inc:242`):

```rust
    assert!(
        tool_result.redacted_body.contains("tool=read"),
        "the tool result row must name the tool, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result
            .redacted_body
            .contains("the delegated child read this line"),
        "the tool result row must hold the output the tool produced, got: {:?}",
        tool_result.redacted_body
    );
```

**The call and its result are both present, in ordinal order, and pairable** —
the same test:

```rust
    assert!(
        tool_call.ordinal < tool_result.ordinal,
        "the tool call must come before its result, got call {} and result {}",
        tool_call.ordinal,
        tool_result.ordinal
    );
    for row in [tool_call, tool_result] {
        assert!(
            row.redacted_body.contains("tool_call_id=call-1"),
            "the call and its result must name the same call id, got: {:?}",
            row.redacted_body
        );
    }
```

**The stored row is redacted** —
`tests/agent_loop/delegated_transcript.inc:534`
(`a_delegated_run_redacts_a_secret_in_the_tool_output`). The file the child
reads holds `api_key=<secret>`, and the secret is absent from the **stored
row** (read back through the store's read API, which returns what was
written):

```rust
    assert!(
        !tool_result.redacted_body.contains(secret),
        "the secret must be absent from the stored row, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result.redacted_body.contains("[REDACTED"),
        "the stored row must show that a redaction happened, got: {:?}",
        tool_result.redacted_body
    );
```

**A run that executes no tool records no tool result** —
`tests/agent_loop/delegated_transcript.inc:572`
(`a_delegated_run_that_executes_no_tool_records_no_tool_result`):

```rust
    assert!(
        !entries
            .iter()
            .any(|entry| entry.kind == TranscriptKind::ToolResult),
        "a run that executed no tool must record no tool result, got: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == TranscriptKind::ModelMessage),
        "the run must still have recorded its messages, got: {entries:?}"
    );
```

**The events vector is unchanged** —
`src/agent/worker.rs:2324`
(`the_delegated_capture_leaves_a_tool_call_without_a_result_pending`). The
parent's turn transcript is built from the child's event vector, so a tool
call the provider emitted with no matching result must still be stored as
`Pending`:

```rust
        assert_eq!(
            status,
            crate::session::message::ToolStatus::Pending,
            "a tool call with no matching result must stay pending"
        );
        assert!(
            output.is_none(),
            "a pending tool call must carry no output, got: {output:?}"
        );
```

The two static rules in §4.6 are the machine-checked form of the same
invariant: the capture path may not push onto the child's event vector, and
may not construct a `ChatEvent::ToolResult`.

---

## 5. Invariant review (plan §4, item by item)

| §4 invariant | Evidence | Status |
|---|---|---|
| **A delegated run is not a session.** No `session` row; the catalog, project binding, `/sessions`, `Ctrl+T` and session-scoped snapshots are unchanged; the catalog is byte-identical before and after. | §4.5 above (`tests/session_family/delegated_transcript_catalog.inc:206`). No `session` INSERT exists in the migration; the only foreign key the transcript carries points at `agent_run`. `src/agent/turn_completion.rs:567` corrects the stale comment that claimed a delegated run's output "stays in the run journal". | held |
| **Redaction happens at write.** An unredacted body is never durable. | `redact_transcript_body` (`:244`) is reached from `prepare_transcript_row` (`:279`), which both `append` implementations call **before** the INSERT. §4.3 proves the absence in the stored row. | held |
| **Bounds are enforced at write.** Per row and per run; a truncated transcript says so. | §4.4. `prepare_transcript_row` bounds the row; the per-run bound is checked inside the append transaction and records a marker row instead of the body. | held |
| **The store is append-only and ordinal-stable.** A row is never rewritten; a concurrent append receives a distinct ordinal. | No UPDATE or DELETE against `agent_run_transcript` exists in the module. `crates/codegg-core/src/agent_run_transcript.rs:945` `concurrent_appends_receive_distinct_ordinals_and_lose_no_row` — two tasks, 25 appends each, 50 distinct ordinals forming the unbroken sequence 0..49, no row lost. | held |
| **`codegg-core` stays UI-, server-, plugin- and auth-free.** | `./scripts/check-core-boundary.sh` → `codegg-core boundary check passed`. The store imports only `crate::identity` and `crate::projection_replay::redactor`; no `crate::agent*` import (the boundary matcher would reject one). | held |
| **The child's authority is unchanged.** Writing a transcript grants the child nothing. | The capture path only *reads* `request.run_id` and appends rows. No new tool, no permission, no workspace identity, no writer-fence change. The transcript table grants no capability. | held |
| **The existing `message` store is untouched.** | No file under `crates/codegg-core/src/session/store.rs` was modified. `append_batch`'s `Ok(None)` behaviour for synthetic sessions is preserved exactly; the migration adds two new tables and rewrites nothing. | held |
| **The delegated run's result store keeps its shape.** `agent_run_result` is a sibling of the transcript, not a replacement for it. | `crates/codegg-core/src/agent_run.rs` and `crates/codegg-core/src/run_result.rs` were not modified; the transcript is keyed by the same `AgentRunId` and lives beside the result. **The D5 corrective changes the record's *contents*, not its shape**: `agent_run_result.validation` is now populated from the tools the child actually ran, where it was always empty because the branch it read was dead (D5c). | held |

---

## 6. Failure and recovery review (plan §8)

| §8 case | Behaviour | Evidence |
|---|---|---|
| **A crash mid-append.** No partial row is readable; the transcript is short by one message; the run's recorded result is unaffected. | The row INSERT and the aggregate UPDATE share one transaction, so a crash rolls both back. | `crates/codegg-core/src/agent_run_transcript.rs:1074` `a_crash_mid_append_leaves_no_partial_row_and_keeps_the_recorded_result` — a transaction that inserts a row and advances the ordinal is dropped without committing; the transcript still holds exactly one row, the next append receives ordinal **1** (not 2), and `get_result` returns the saved result unchanged. |
| **Two concurrent appends for one run.** Distinct ordinals, no lost row, no duplicate ordinal. | One `BEGIN IMMEDIATE` transaction reads and advances the ordinal. | `crates/codegg-core/src/agent_run_transcript.rs:945` — 50 concurrent appends, 50 distinct ordinals, the unbroken sequence 0..49, 50 rows read back. |
| **The per-run bound is reached.** Appending stops; a truncation marker is recorded; the transcript states that it is truncated. It must never read as complete. | The body is refused and a `TerminalNote` marker row is recorded; the read reports `truncated`. | §4.4; a further append is refused and adds no row. |
| **The delegated run is cancelled.** The transcript holds what the run produced before cancellation; the cancellation is a terminal note. | Three cancellation paths each append a `TerminalNote`. | `src/agent/worker.rs:1136`, `:1154`, `:1170`. |
| **The delegated run fails.** The transcript holds the conversation that led to the failure. | The failure is appended as the last `TerminalNote`. | `src/agent/worker.rs:1235`. |
| **A redaction rule changes.** Rows already written are not rewritten; the rule change applies to later runs only. | The store is append-only; there is no UPDATE against `agent_run_transcript`. | **This closure record states it explicitly, as §8 requires:** a change to the redaction rules does **not** retroactively clean rows written under the old rules. Rows written before the change keep their stored (redacted-at-the-time) body. |
| **A run has no transcript** (recorded before this milestone, or the legacy path). The read yields a typed "no transcript recorded", distinguishable from an empty transcript. | `AgentRunTranscriptRead::Absent` vs `::Present(page)`. | `crates/codegg-core/src/agent_run_transcript.rs:1046` — a run that exists but has no transcript reads `Absent`; the same run after one append reads `Present`. |

### 6.1 Degradation

`capture_transcript` (`src/agent/worker.rs:1031`) never propagates a write
failure: it logs at `warn` with the run id and the kind, and returns. The
delegated run therefore still completes and still records its result.
`tests/agent_loop/delegated_transcript.inc:425` proves it by dropping the
transcript table underneath a running delegated run and asserting that the
task still completes with its result and that no partial row is left behind.

---

## 7. Migration and compatibility review

- **Additive.** One migration, **v89**
  (`crates/codegg-core/src/session/schema.rs:3723`), creates two new tables
  and one index. No existing table is rewritten, no row is removed, no
  backfill is performed or permitted. `STORAGE_LAYOUT_VERSION` moves 88 → 89
  (`crates/codegg-core/src/storage/mod.rs:53`).
- **The migration applies cleanly to a database that already holds delegated
  runs.** `crates/codegg-core/src/agent_run_transcript.rs:1195`
  `the_migration_applies_to_a_database_that_already_holds_delegated_runs` —
  the database is rewound to the pre-v89 state (the transcript tables dropped,
  the layout marker set back to 88), a delegated run is created, the migration
  is applied, and the run survives and the store is usable.
- **No protocol change.** Nothing crosses a wire: no DTO, no event, no
  snapshot, no wire schema. An older client is unaffected **by construction**.
- **No session-catalog change.** No `session` row is created for a delegated
  run.
- **`agent_run_result` is unchanged.**
- **The `message` store is unchanged.** `append_batch`'s `Ok(None)` for
  synthetic sessions is preserved exactly; this milestone does not make
  synthetic transcripts writable in that table.
- **Rollback.** The migration is additive, so removing the two tables
  restores the previous schema exactly; the data it held is discarded.
  There is no downgrade path for a database that has already written
  transcripts, and none is needed: nothing else reads them.
- **Legacy path status.** The legacy, non-durable delegated path has **no**
  `run_id` and therefore writes **no** transcript. The guarantee is scoped to
  the durable delegated path. This is stated in the module's own code
  (`src/agent/worker.rs:1019`) and is a deliberate boundary, not an
  oversight: the legacy path has no durable run identity to key a transcript
  by.

---

## 8. Security review

- **Redaction is at write.** The milestone's central criterion. Proved by
  §4.3 against the stored row.
- **No second secret recogniser.** The recogniser is the repository's
  existing `ProjectionFieldRedactor`; the store adds a *composition* of its
  existing rule sets, not a new pattern set.
- **The redaction pass fails closed.** A redactor refusal, or an input beyond
  the recogniser's own bound, yields a marker rather than the original text.
- **Bounds are a denial-of-service control.** Per row (8 KiB), per run
  (256 KiB), per read (256 rows). A read is always bounded; there is no
  unbounded read. The per-run bound is enforced in the same transaction that
  assigns the ordinal, so it cannot be raced.
- **The aggregate is O(1) per append.** A long run does not make each append
  progressively more expensive, and the append path is bounded by the body
  size, not the run size.
- **Authority.** Writing a transcript grants the child nothing. The store
  carries no permission check of its own, because it confers no capability.
- **Trust boundary.** Nothing about the transcript crosses a wire in this
  milestone. The projection and the TUI are C003's scope.
- **Audit visibility.** A failed transcript write is logged at `warn` with
  the run id and the kind (`src/agent/worker.rs:1043`).

---

## 9. Documentation and operations

### 9.1 Documentation updated

| Document | Change |
|---|---|
| `architecture/codegg_core.md` | New "Delegated-Run Transcript (ADR-0051 option B)" section with the store's shape, its bounds and its three load-bearing properties; the module table gained `agent_run_transcript`; the module count 54 → 55; the layout version 87 → 89. The D5 corrective extended it: a tool result in the transcript is the outcome the **loop executed**, not an event the provider chose to emit; the retained output is a bounded head-and-tail excerpt; and the capture path never writes into the child's event vector. |
| `architecture/session.md` | Invariant 13: a delegated run is not a session; `append_batch`'s `Ok(None)` is correct; the transcript lives in `codegg_core::agent_run_transcript`, keyed by the run id, created by migration v89. |
| `architecture/projection.md` | Forward reference only: the delegated transcript is authoritative durable state and is **not** projected; nothing about it crosses a wire today; C003 extends the projection to carry it. |
| `architecture/overview.md` | The "Verified counts" table rows this change moved: `Database tables` 95/91 → 97/93, `Storage layout version` 88 → 89, `Integration test files` 338 → 342, `Guard scripts (all)` 92 → 94. Measured with `python3 scripts/measure_overview_counts.py`. |

### 9.2 Static guard

`scripts/check_delegated_transcript_store.py` — checks that the delegated
transcript store's declared bounds and its redaction entry point are **live**,
that every declared row field is **written**, that the migration creates the
table, that the production capture path names the store, and that the capture
path **does not change what the child's event vector holds**. The last check
has two rules: `delegated-transcript-events-mutated` (the capture path may
not push onto the event vector) and
`delegated-transcript-tool-result-constructed` (it may not construct a
`ChatEvent::ToolResult`). It carries a `--self-test` (§4.6) that proves it is
not vacuous — six independently injected defects, each rejected by the check
that owns it.

**The guard is wired**: it appears in **both** `scripts/verify.sh`'s
`run_guards()` (with its `--self-test`) and `.github/workflows/ci.yml`
(`scripts/verify.sh:146-150`, `.github/workflows/ci.yml:163-166`). This
milestone did not edit either file — the wiring was made in this round by the
orchestrator, alongside the concurrent worker's own guard.

### 9.3 Operator diagnostics

A delegated run's transcript is read with the store's read API:

```rust
let store = SqliteAgentRunTranscriptStore::new(pool.clone());
match store.read(&run_id, 0, 256).await? {
    AgentRunTranscriptRead::Absent => { /* no transcript recorded */ }
    AgentRunTranscriptRead::Present(page) => {
        // page.entries, page.has_more, page.truncated,
        // page.total_bytes, page.total_entries
    }
}
```

A failed transcript write appears in the daemon log as
`delegated transcript write failed; the delegated run is unaffected`, carrying
the run id and the row kind.

---

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `bash scripts/verify.sh quick` exits 1 at the comment-budget ratchet: `scripts/check_comment_budget.py` reports **23 violations** and its `--self-test` fails because the tree is over the recorded ceiling. This milestone contributes **8**, across six files: `crates/codegg-core/src/agent_run_transcript.rs` (105 comment lines, an unlisted file and needs a bank entry), `crates/codegg-core/src/session/schema.rs` (467 vs a ceiling of 464), `crates/codegg-core/src/storage/mod.rs` (103 vs 100), `src/agent/worker.rs` (343 vs 311) and `src/agent/turn_completion.rs` (70 vs 67), plus `src/agent/loop.rs` (357 vs 306) from the D5 corrective. The concurrent worker contributes the remaining 15, and the global corpus is over its ceiling. | The gate is red. The brief authorises exactly this: `docs/comment-budget.toml` is a ratchet the orchestrator re-banks after all workers land, and it was **not** edited here. | The orchestrator re-banks `docs/comment-budget.toml` with `python3 scripts/check_comment_budget.py --bank` after all workers land. |
| low | `architecture/agent.md` was **not** updated by this milestone: it is not in the milestone's owned file set, and the concurrent worker has it open at the same time. The delegated transcript's capture path belongs in that document. | The agent architecture document does not yet describe the delegated transcript capture path. | The orchestrator (or the concurrent worker, who owns that file in this round) adds the capture-path description. |
| low | The `AgentRunTranscriptStore` read API is not `pub use`d from the root crate's `src/lib.rs` (which re-exports a block of `codegg_core` modules). Consumers reach it as `codegg_core::agent_run_transcript::…`. `src/lib.rs` is not in this milestone's owned file set. | Cosmetic: a consumer needs the full path. | Optional: the orchestrator may add `agent_run_transcript` to the root re-export list. |

The **D5 finding is closed** and is no longer listed here: the child's executed
tool outputs now reach the transcript. See §3.3 and §13.

---

## 11. Roadmap disposition

**Milestone conditionally closed**, with the exact outstanding evidence named
in §10: the shared comment-budget ratchet (the canonical gate's only
remaining failure) and `architecture/agent.md`.

The milestone's own implementation, tests, guard and documentation are
complete, and every requirement the plan states about the *capability* is
satisfied and evidenced — including the D5 corrective, which closed the one
gap the earlier round named in the capability itself. The guard is wired into
`scripts/verify.sh` and CI. What remains is re-banking that the orchestrator
owns and that the brief explicitly reserved.

---

## 12. Registry updates and the unblock audit

### 12.1 Registry updates required (the orchestrator owns `plans/registry.md`)

- Mark milestone **002 — a durable transcript for a delegated run** as closed
  (conditionally closed, per §11) in the
  `delegated-run-observability-and-recovery-corrective` entry of
  `plans/registry.md`.
- Record milestone **002**'s closure record at
  `plans/closure/delegated-run-observability-and-recovery-corrective/002-status.md`.
- Note the guard `scripts/check_delegated_transcript_store.py` in the
  registry's guard inventory if it carries one.
- `plans/registry.md` and `plans/subsystems/**` were **not** edited by this
  milestone: both are the orchestrator's.

### 12.2 Unblock audit

| Milestone | Condition | State at this closure | Eligible? |
|---|---|---|---|
| **C002** (this milestone) | ADR-0051 accepted **and** the milestone closes | ADR-0051 accepted as option B on 2026-10-24, acceptance commit `4803e093c`. The milestone closes (conditionally, §11). | **closed** |
| **C003** (projection + TUI extend the transcript) | ADR-0051 accepted **and** C002 closed | ADR-0051 is accepted; C002 closes with this record. | **eligible to start** |
| **C004** (continuation / resume) | ADR-0051 accepted **and** C002 closed | ADR-0051 is accepted; C002 closes with this record. | **eligible to start** |

Neither C003 nor C004 remains blocked by an unaccepted ADR. Both inherit the
deviations in §13 — in particular C003 must project the transcript **as it is
stored** (redacted at write, bounded, with its truncation marker), and C004's
continuation must read a transcript that may be `Absent` for a run recorded
before this milestone.

One inheritance note for C003: a tool result in the transcript is the
**outcome the loop executed**, not an event the provider emitted (D5), and
the retained output is a bounded head-and-tail excerpt with the loop's own
truncation marker inside it (D5b). A projection that renders the stored row
must therefore show the output as the excerpt it is — head, marker, tail —
rather than as a complete output.

---

## 12.3 The roadmap's exit conditions for C002

`plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md`
states C002's exit conditions as:

> a delegated run's messages are durably present and redacted; a
> known-secret-shaped body is provably absent from the stored row; the per-run
> bound is enforced and truncation is recorded; no `session` row is created and
> the session catalog is unchanged.

Each is met by the evidence in §4: §4.3 (durably present, redacted, secret
absent from the stored row), §4.4 (per-row and per-run bounds enforced, the
truncation recorded), §4.5 (no `session` row; the catalog identical before and
after).

## 13. Deviations

Every deviation from the plan is recorded here.

| # | Deviation | Reason |
|---|---|---|
| **D1** | The store is **two** tables (`agent_run_transcript` + `agent_run_transcript_state`), not one. | The plan requires the per-run bound and the ordinal assignment to be **O(body), not O(run)**. A single table would make the bound check a `SUM(byte_len)` over the run's rows — O(run). The second table carries the per-run aggregate (next ordinal, stored bytes, truncation flag), so both are O(1) per append. The migration is still **one additive migration** and still adds no row to any existing table. The ADR's declared *row* shape `(run_id, ordinal, role, kind, redacted_body, byte_len, created_at)` is unchanged. |
| **D2** | The `kind` values are `model_message`, `tool_call`, `tool_result`, `terminal_note`; the **truncation marker is a `terminal_note` row**, not a fifth kind. | The ADR names exactly four kinds. The marker is a terminal note whose body is the reserved `TRANSCRIPT_RUN_TRUNCATION_MARKER`; the read API reports `truncated: bool` so a consumer never has to parse the body. |
| **D3** | The truncation marker row's `role` is `Orchestrator`. | A truncation marker is a store-level fact about the run, not a message in either conversation channel. `Orchestrator` is the channel a parent reads. |
| **D4** | "Absolute environment values" is implemented as the repository's existing `FieldName::Environment` rule set (whole-value replacement) applied to **every** body, composed with the `FieldName::Text` secret-assignment rule. | The brief requires the repository's existing recogniser, not a second one. A bare environment value with no `KEY=` prefix is not detectable by any shape-based rule the repository has; that limitation is inherent to a shape-based recogniser and is not worked around with an ad-hoc matcher. |
| **D5** | **RESOLVED by the D5 corrective.** The captured tool results no longer come from the child's `ChatEvent` stream. They come from the outcomes the **loop executed**, which the loop now retains per run (`src/agent/loop.rs`, `ExecutedToolOutcomes`) and exposes through `AgentLoop::executed_tool_outcomes()`. The capture path writes one `ToolResult` row per retained outcome (`src/agent/worker.rs:1984`), with the tool's name, the tool's output, the success flag and the loop's own truncation flag. | The original deviation was correct in its diagnosis and wrong in its conclusion. No provider adapter constructs `ChatEvent::ToolResult`, so the capture branch was **dead**: the earlier integration test drove it by having the scripted provider emit the event, which is a thing no production adapter does. A production transcript therefore carried the tool *calls* and the model's messages, and no tool *outputs*. The outputs exist only where the tools ran — in the loop — so the loop is the source. `src/agent/loop.rs` was brought into this milestone's owned file set for the corrective, which is what the earlier round's deviation said would be required. Two properties of the resolution are load-bearing: the retained output is the **path-redacted** text the loop published and the model received (so the transcript leaks no local path and matches what the child acted on), and the capture path **never writes into the child's event vector** — that vector is what the parent's turn transcript is built from, so a tool call with no matching result must stay `Pending`. The `events` vector is unchanged and no `ChatEvent::ToolResult` is emitted into it. |
| **D5b** | The retained output is a bounded **excerpt** — a head, the truncation marker, and a tail — rather than a prefix. The loop's own retention cap is 8 KiB (`MAX_RETAINED_TOOL_OUTPUT_BYTES`), and the retained text is the path-redacted tool output with a `output_truncated` flag beside it. | A tool's conclusion is at the **end** of its output: the `CODEGG_VALIDATION_RESULT:` marker the `test` tool appends is its last line. A head-only cut would discard exactly the line a caller — `extract_validation_evidence`, and any projection that follows — reads the output for. Keeping the tail also puts the loop's truncation marker **between** the head and the tail, so the later and tighter store row bound cannot cut the marker off and leave a truncated output reading as a complete one. The retention is a memory bound, not the transcript's bound: the store bounds the row again at write. |
| **D5c** | `extract_validation_evidence` now reads the same retained outcomes, so a delegated run's recorded `agent_run_result.validation` is **populated** where it was always empty. | The branch it read was the dead event-stream branch (D5). This is a behaviour change, not only an internal one: a delegated run's durable result now carries the validation evidence the child's tools produced. It is the correct behaviour — the evidence describes what the child actually ran — but it is a change in a durable record's contents, so it is recorded here rather than left implicit. |
| **D6** | `architecture/overview.md`'s "Verified counts" table was updated (four rows) although it is outside this milestone's enumerated doc set. | `scripts/check_test_target_references.py` checks one of those rows, and this milestone's two new integration members moved it. The value is a deterministic function of the tree (342), so a concurrent writer measuring the same tree writes the same number. The rows are the only ones this change moved; the prose reconciliation paragraphs were left alone. |
| **D7** | The new guard was **not** wired into `scripts/verify.sh` or `.github/workflows/ci.yml` **by this milestone**. | Both files are the orchestrator's, and this milestone was explicitly instructed not to edit them. **The wiring was made in this round by the orchestrator** (`scripts/verify.sh:146-150`, `.github/workflows/ci.yml:163-166`), so the outstanding action recorded by the earlier round is now discharged. |
| **D8** | `architecture/agent.md` was **not** updated. | It is outside this milestone's owned file set and the concurrent worker has it open in this round. Recorded in §10. |
| **D9** | `crates/codegg-core/src/lib.rs` gained `pub mod agent_run_transcript;` but the root crate's `src/lib.rs` re-export list was not extended. | `src/lib.rs` is outside this milestone's owned file set. Consumers use `codegg_core::agent_run_transcript::…`; the integration tests do exactly that. |
| **D10** | The comment budget was **not** re-banked. | `docs/comment-budget.toml` is the orchestrator's, and the brief says to record the failure rather than edit the bank. This milestone's additions were trimmed to +3 in each of the two schema files it touched, +10 in `src/agent/worker.rs` and +3 in `src/agent/turn_completion.rs`, and the new module's comments are 105 lines (9% comment density, in line with the house range). The D5 corrective added the retention API's documentation in `src/agent/loop.rs` (357 comment lines against a ceiling of 306) and the capture path's in `src/agent/worker.rs` (343 against 311); both were trimmed before the record was written, and the new integration member's own file was brought **under** the 20-line floor an unlisted file is held to. The re-bank is the orchestrator's. |
| **D11** | The delegated transcript's rows carry a foreign key to `agent_run(run_id)` with `ON DELETE CASCADE`. | The ADR requires a "deletion path" for a bounded per-run transcript, and the foreign key makes "a transcript belongs to a delegated run" a storage-enforced invariant. Deleting a run therefore deletes its transcript. |

---

## 14. Closure status

**Conditionally closed.**

The capability is complete and evidenced: the durable store, the redaction at
write, the bounds at write with recorded truncation, the capture path, the
concurrency and crash behaviour, the typed "no transcript recorded" read, the
degradation, the catalog invariant, the static guard with a proven
non-vacuous self-test, and the documentation.

**D5 is closed.** The child's executed tool results now reach the transcript
(§3.3, §13), and the corrective's own tests and guard rules are evidenced in
§4. The earlier round's `cargo fmt` failure is also resolved:
`cargo fmt --all -- --check` now exits 0.

The named outstanding evidence is exactly:

1. the shared comment-budget ratchet, which the orchestrator re-banks — the
   only remaining failure in the canonical gate, and every one of the 110
   guard commands other than the budget check and its self-test passes;
2. `architecture/agent.md`'s capture-path description.

Item 1 is the orchestrator's by the brief's own ratchet rule. Item 2 is a
recorded deviation with its required action named (§10, §13 D8).

The earlier round's two other outstanding items are **discharged**: the
`cargo fmt` failure is gone (`cargo fmt --all -- --check` exits 0), and the
guard's wiring into `scripts/verify.sh` and CI was made in this round by the
orchestrator (§9.2, D7).

Nothing in the outstanding list is a gap in the **capability**. Every
requirement the plan states about the delegated transcript is satisfied by
the evidence in §4.
