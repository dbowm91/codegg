# Milestone 002a — Durable Queue Storage and Read Projection: Closure Record

Status: **closed**

Milestone: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 2a)

Plan: `plans/implementation/user-prompt-queue/002a-durable-queue-storage-and-read-projection.md`

Closure record: `plans/closure/user-prompt-queue/002a-status.md`

Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted M001 closure
tree. Nothing in this milestone was committed; the whole working tree remains
uncommitted.

## 1. Class and delivered capability

Primary class: **infrastructure**.

**This milestone delivered no user-visible capability. No user can queue a prompt.**

Every mutation operation — add, edit, delete, reorder — is M002b and was out of
scope. The read projection therefore has no producer, and the queue reads empty by
construction. `verify.sh quick` passing is not evidence a user can queue anything,
and this record must not be read as though it were. The first user-visible surface
is M004, and the first user-authorable operation is M002b.

## 2. What shipped

| Area | Delivered |
|---|---|
| Storage | `migrate_v80` creating `user_prompt_queue_entry` + `idx_user_prompt_queue_entry_session`; `STORAGE_LAYOUT_VERSION` 79 → 80; the `if current_version < 80` ladder gate |
| Model | `UserPromptQueueStore`, `UserPromptQueueEntryRecord`, `UserPromptQueueLiveEntry`, `UserPromptQueueStatus`, `UserPromptQueueError` with a `code()` ladder |
| Bounds | 64 KiB per entry, 64 pending, 7-day TTL, 256 retained per session — each a typed refusal **and** a schema `CHECK` backstop |
| Ordering | Daemon-maintained dense ordinal under a schema-enforced `UNIQUE (session_id, ordinal)` |
| Expiry | Lazy, deterministic, single-transaction sweep on the read path — the producer for `expired` |
| Protocol | `CoreRequest::UserPromptQueueList`, `CoreResponse::UserPromptQueue`, `UserPromptQueueEntryDto` (every field `#[serde(default)]`) |
| Authority | `OperationDescriptor` = `ScopeKind::ViaSession` + `Capability::SessionRead`; `UNINSTRUMENTED_OPERATIONS` |
| Frontend | Seeded `SessionState::user_prompt_queue` projection, loaded once on the session-change edge, typed completion command, stale-generation guard |

Shipped DDL:

```sql
CREATE TABLE IF NOT EXISTS user_prompt_queue_entry (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    text TEXT NOT NULL,
    text_bytes INTEGER NOT NULL CHECK (text_bytes >= 0 AND text_bytes <= 65536),
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'delivering', 'delivered', 'failed', 'expired')
    ),
    failure_reason TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    delivered_turn_id TEXT,
    delivered_at_ms INTEGER,
    UNIQUE (session_id, ordinal),
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_user_prompt_queue_entry_session
    ON user_prompt_queue_entry(session_id, status, ordinal);
```

## 3. Acceptance criteria against evidence

| # | Criterion | Evidence |
|---|---|---|
| 1 | v79 pool migrates to v80; table, index, `CHECK`s exist; layout equals 80 and both assertions pass | `user_prompt_queue_v80_migration_creates_table_index_and_layout`; `check_project_catalog_invariants.py` 7/7 including `--self-test` (4/4 cases) |
| 2 | Entries survive restart with byte-identical text, unchanged ordinal and status | `user_prompt_queue_entries_survive_reopen_with_unchanged_fields`; `..._round_trip_is_byte_identical_and_dense_in_ordinal_order` |
| 3 | Count, length, and retention each give their **specific** typed refusal; the schema rejects an over-bound direct row | `..._entry_byte_bound_refuses_with_typed_variant`, `..._pending_count_bound_refuses_with_too_many_entries`, `..._retention_bound_refuses_with_too_many_entries`, `..._schema_check_rejects_over_bound_text_bytes` |
| 4 | A due entry is `expired` and absent from the live list; non-due untouched; repeated reads idempotent | `..._due_entry_expires_and_leaves_the_live_list`, `..._expiry_is_idempotent_across_repeated_reads`, `..._due_entries_expire_across_restart_and_others_do_not` |
| 5 | `UserPromptQueueList` returns dense daemon order and is refused without `SessionRead` | `user_prompt_queue_read_is_refused_without_session_read` (live daemon: outsider → `authorization_denied`; `Viewer` holding `SessionRead` → `CoreResponse::UserPromptQueue`) |
| 6 | Classified in `UNINSTRUMENTED_OPERATIONS`; audit guard passes | `user_prompt_queue_list_is_classified_uninstrumented`; `scripts/check_audit_coverage.py` passed |
| 7 | No `CoreRequest` variant can mutate the queue, guarded by a test | `user_prompt_queue_no_core_request_mutation_variant_exists` |
| 8 | `codegg-core` free of UI, server, plugin, auth | `bash scripts/check-core-boundary.sh` passed |
| 9 | Closure states plainly that no user can queue a prompt | §1 above |

Truncation proof: `user_prompt_queue_text_at_exact_byte_bound_is_accepted_not_truncated`
asserts a boundary-length entry stores byte-identical, and
`..._error_messages_carry_no_entry_text` asserts on the recorded error value using a
distinctive sentinel body rather than a substring that could match coincidentally.

## 4. Verification actually run

All run locally on this machine. **Hosted CI was not run.**

| Command | Result |
|---|---|
| `cargo fmt` | ran; 0 remaining diffs |
| `cargo test -p codegg-core user_prompt_queue` | **26 passed**, 0 failed (4 lib unit + 22 integration) — filter non-zero |
| `cargo test -p codegg-core --test human_shell_promotion` | 4 passed (analogue untouched) |
| `cargo test --test storage_migrations` | 4 passed |
| `cargo test -p codegg-core --test identity` | 2 passed |
| `cargo test --lib user_prompt_queue` | 4 passed (6483 filtered out) |
| `cargo test --test identity_m003_daemon_authorization user_prompt_queue` | 2 passed |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 passed |
| `python3 scripts/check_project_catalog_invariants.py --self-test` | 4/4 self-test cases passed |
| `python3 scripts/check_audit_coverage.py` | passed |
| `bash scripts/check-core-boundary.sh` | passed |
| `python3 scripts/check_no_hardcoded_models.py` | passed |
| `cargo clippy -p codegg-core --all-targets -- -D warnings` | clean |
| `cargo clippy -p codegg --all-targets -- -D warnings` | clean |
| `scripts/verify.sh quick` | **passed** — run independently by the closing agent after the implementation pass, not merely inherited from it; ended `Quick verification passed.` with `cargo check --workspace --all-targets --locked` clean in 1m 45s |

Additionally green, from the guards `verify.sh quick` runs that touch this change:
`check_tui_transport_authority`, `check_http_route_disposition`,
`check_scheduler_bypass`, `check_daemon_cwd_usage`, `check_execution_ownership`,
`check_config_merge_coverage`, `check_sandbox_contract`,
`check_sandbox_policy_wiring`, `generate_builtin_agents.py --check`.

`CARGO_BUILD_JOBS=2` throughout. No `--all-features` was used.

## 5. Where reality contradicted the plan

Three, recorded rather than smoothed over.

1. **The plan specified only the `80 => migrate_v80(&mut tx)` dispatch arm. The real
   migration path also needs a separate `if current_version < 80 { … }` ladder gate**
   at `crates/codegg-core/src/session/schema.rs:262`. Without it `migrate_v80` is
   dead code and every test fails with `no such table`. The plan's §3 evidence quote
   of the dispatch arm list was accurate but incomplete: the ladder at `:253-264` is
   a separate construct that a migration must also join. **This is a plan defect, not
   an implementation choice.**
2. **`UserPromptQueueError::Unavailable` was specified with no producer.** The daemon
   returns a literal when the pool is absent, before any store exists, so the variant
   was unreachable. It was dropped rather than given a contrived producer, because a
   variant with no producer is the same defect class this milestone deliberately
   accepted for the `CHECK` vocabulary — and unlike a `CHECK` value, an enum variant
   *can* mislead a reader. `Storage` now maps to its own
   `user_prompt_queue_storage_error` code, and every remaining variant has a real
   producer asserted by the ladder test.
3. **Row-mapping failure paths need private-type access.** The `text_bytes`
   conversion failure and the status `parse` refusal are unreachable from an
   integration test, because the `CHECK` constraints block the values that would
   trigger them. They were placed as in-module unit tests rather than weakened.

## 6. Recorded divergences and deliberate narrowings

- **`status` vocabulary.** The `CHECK` enumerates all five roadmap values
  (`pending`, `delivering`, `delivered`, `failed`, `expired`) but only `pending` and
  `expired` have producers. `delivering`, `delivered`, and `failed` are
  **unreachable until M003**. Rationale: SQLite cannot `ALTER` a `CHECK`, so
  narrowing now would force a create-copy-drop-rename rebuild of a durable user
  table. A `CHECK` value is a validation constraint, not a code path, and cannot
  execute wrongly. **M003 inherits this decision explicitly: it must either make all
  five reachable or rebuild the table.** Raise it rather than assuming it is cheap.
- **`run_at_ms` omitted.** Nothing here writes it, and a durable column with no
  producer is the claim-without-a-hook defect. M007 adds it as a nullable column by
  in-place `ALTER TABLE … ADD COLUMN`. **The migration version is v82, not v81** —
  accepting ADR-0037 on 2026-10-18 spent v81 on the queue-owner binding.
- **Capability divergence.** `user_prompt_queue_list` uses `Capability::SessionRead`,
  deliberately narrower than `human_shell_promotions_get`'s `AgentInvoke`, because
  ADR-0035 states queue reads are subject to session read authority. Pinned by
  `user_prompt_queue_descriptor_uses_session_read_not_agent_invoke` and
  `user_prompt_queue_descriptor_diverges_from_promotion_read_capability`, so a later
  edit that copies the broader precedent fails a test.
- **`UNIQUE (session_id, ordinal)`** makes the total order schema-enforced.
  **M002b inherits the constraint that reorder must renumber collision-free under one
  transaction**; this slice only appends at `max(ordinal) + 1`.

## 7. Boundaries held

- **No user-visible surface.** No dialog, no `/queue` command, no keybinding, no row 0
  change. `src/tui/app/state/observe.rs` is untouched, so `/queue` was correctly **not**
  added to the fail-closed `is_observer_allowed_command` allowlist; M004 owns that.
- **`is_controller_bypass` was neither extended nor routed through.** Verified by an
  empty diffstat on `src/core/daemon_control.rs`.
- **No prompt text in diagnostics.** Asserted against the recorded error value, not
  the absence of a log line.
- **`codegg-core` stays UI-, server-, plugin-, and auth-free.**
- **Storage changes are additive only.** No existing table altered, no stored data
  rewritten.

## 8. M001 preservation

All 12 M001 working-tree entries remain present and unmodified in content. `cargo
fmt` was verified *before* running that its `--check` output listed only M002a files,
so no M001 content was reformatted. The three M001-owned files M002a also had to
touch (`src/tui/app/commands.rs`, `src/tui/app/mod.rs`,
`src/tui/runtime/command_dispatch.rs`) received additive-only hunks; the
`command_dispatch.rs` change is a clean 7-line insertion. No stash, revert, or
checkout was performed.

## 9. Unblock audit

- **M002b is no longer blocked.** Its blocker, ADR-0037, was **accepted** on
  2026-10-18 selecting Option B (a durable, session-scoped queue-owner binding). Its
  plan was rewritten against that decision and moved `blocked` → `ready`. This is the
  one status that changed as a result of this closure.
- **ADR-0036 remains `proposed`** and still gates M006 and M007's copy boundary. It
  was not touched.
- **M003–M008 remain deliberately unplanned**, each waiting on a closure or an ADR
  acceptance that does not exist.
- `tui-panel-surface-convergence` M004 remains a soft/interface gate for this
  roadmap's own M004.
- **ADR-0035 and ADR-0037 are both accepted.** ADR-0037's acceptance carried a
  recorded pre-acceptance correction: its draft folded the owner binding into v80,
  which this milestone had already spent, so the claim was corrected to **v81** while
  the ADR was still `proposed`, with the draft-versus-accepted difference stated in
  the accepted record's header.

## 10. Unresolved findings carried forward

1. **The M003 `status` `CHECK` decision** (§6) — widen or rebuild a durable table.
2. **`run_at_ms` is v82**, not v81.
3. **The plan's §3 migration evidence was incomplete** (§5.1). Any future migration
   milestone in this repository must join both the dispatch arm *and* the
   `if current_version < N` ladder.
4. **M001's inherited `agents_to_dtos` pre-flight failure still does not release the
   turn.** Unrelated to this milestone; not absorbed here.
5. **The queue has no producer.** The read path is exercised only by tests that
   construct rows directly. The first real producer is M002b.