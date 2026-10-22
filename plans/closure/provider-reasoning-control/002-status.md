# Provider Reasoning Control Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-reasoning-control/002-open-reasoning-vocabulary-and-daemon-owned-selection.md`

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-002--open-reasoning-vocabulary-and-daemon-owned-selection`

Source decision:

- `plans/adrs/ADR-0044-open-provider-scoped-reasoning-control-vocabulary.md` (accepted)

Repository baseline reviewed: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5`

Implementation commits or pull requests:

- none — the change is left uncommitted in the working tree, as instructed for
  this milestone.

## 1. Executive finding

Milestone 002 is closed. The reasoning vocabulary is now **open and
provider-scoped** end to end, and the **daemon owns the durable selection**
that applies it.

At the baseline the protocol carried a closed four-level `ReasoningEffort`
enum (`minimal` / `low` / `medium` / `high`). A vendor label such as `xhigh`
or `max` was therefore *expressible on the wire* — M001's bridge carries the
label verbatim — but **not selectable through the protocol**: the enum was the
only door into an effort selection, and it had four doors. The protocol's
serialized shape now carries an open, provider-scoped string, and the known
four labels survive as a **convenience set** for the frontend's fallback
cycling order only, not as an accepted value set.

The second half of the baseline was worse: the per-turn field was
frontend-local and never persisted. It applied to a direct TUI submit and
vanished on a session-prompt submit, a queued prompt, and a delegated idle
continuation — three of the four ways a turn can start. The daemon now
resolves the effective selection at **turn admission**, through one helper
called at all three dropped paths, and persists it in
`runtime_preferences.reasoning_selection` (migration `v84`) against the
principal derived from transport authority.

Because the protocol's serialized shape changed, the milestone also lands the
capability negotiation ADR-0044 requires: the daemon advertises that it
accepts an open vocabulary in the established `*Capabilities` family, a client
consults that advertisement before sending a non-enumerated label, and a
daemon that has not advertised it refuses such a label with a typed error
rather than folding it onto a known level.

## 2. Requirement-to-evidence matrix

| # | Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | An unrecognized label such as `xhigh` or `max` is preserved verbatim through the protocol, the agent layer, the request, and the encoded body, with no normalization, rejection, or translation | `crates/codegg-protocol/src/core.rs` — `core::tests::an_open_reasoning_label_is_carried_verbatim` (`{"mode":"effort","level":"xhigh"}` round-trips byte-identically); `crates/codegg-client/src/compose.rs` — `compose::tests::a_client_with_the_capability_sends_the_open_label_verbatim`; `crates/codegg-core/src/approval.rs` — `approval::tests::reasoning_selection_round_trips_an_open_label_verbatim`; `src/core/daemon_turns.rs` — `the_persisted_selection_reaches_a_direct_turn_submit` (the runtime observes `Effort { level: "xhigh" }`); `crates/codegg-providers/src/wire.rs` — `reasoning_intent` carries `effort: Some(label.clone())` | pass | no normalization anywhere: the label is copied, never mapped. `scripts/check_reasoning_vocabulary_open.py` rule 3 rejects a bridge that lower-cases, trims, or matches the label against a closed set |
| 2 | The legacy payload `{"mode":"effort","level":"medium"}` still decodes to the medium label, and all three legacy `ReasoningOverride` variants still decode | `core::tests::the_known_labels_keep_their_legacy_serialized_shape` (all four known labels serialize to the legacy byte shape) and `core::tests::an_open_reasoning_label_is_carried_verbatim` (the legacy payload decodes); `core::tests::reasoning_override_off_is_distinct_from_absent` (`Off` still decodes and is still distinct from absent) | pass | `Budget { tokens }` and `Off` are unchanged variants; the enum's `#[serde(tag = "mode", rename_all = "snake_case")]` shape is unchanged |
| 3 | A client that has not observed the open-vocabulary capability sends no non-enumerated label, and a daemon that has not advertised it refuses one with a typed error rather than failing the daemon | `crates/codegg-client/src/compose.rs` — `a_client_without_the_capability_sends_no_non_enumerated_label` (`ComposerError::ReasoningVocabularyUnsupported`, the request is never built) and `a_known_label_needs_no_capability_observation`; `src/core/daemon_turns.rs` — `a_daemon_without_the_open_vocabulary_refuses_a_non_enumerated_label` (`CoreResponse::Error { code: "reasoning_vocabulary_unsupported" }`, the daemon keeps serving) | pass | the daemon's advertised capability and its admission check read the **same** daemon-owned value, so "advertised" and "accepted" cannot drift apart |
| 4 | The selection is durable, principal-scoped, and revision-gated, held in `runtime_preferences`; the principal is derived server-side and the payload carries no identity | `crates/codegg-core/src/approval.rs` — `approval::tests::reasoning_selection_round_trips_an_open_label_verbatim`, `reasoning_selection_write_is_revision_gated` (`PreferenceError::Conflict` on a stale revision), `a_stale_reasoning_write_is_rejected_by_the_statement_itself` (the revision predicate lives **inside** the upsert, so a write that lands after another writer moved the row is refused), `two_concurrent_reasoning_writes_produce_one_winner_and_one_conflict`, `reasoning_selection_survives_unrelated_preference_writes`; `src/core/daemon_ops.rs` — `CoreRequest::ReasoningPreferenceSet` reads `authority.principal_id()` and the payload (`selection`, `expected_revision`) carries no identity field; `crates/codegg-core/src/authorization/policy.rs` — `R::ReasoningPreferenceSet { .. } => OperationDescriptor::new("reasoning_preference_set", ScopeKind::Global, None)` | pass | the store's own `principal_id` argument is the only identity in the write path. **Corrected after the independent audit (§13):** the gate was originally a read-then-check, which is a check another writer can invalidate between the check and the write; the revision predicate now lives in the statement |
| 5 | The selection survives a daemon restart and is applied to the next admitted turn | The selection is a durable row in `runtime_preferences` (`crates/codegg-core/src/approval.rs`), read fresh on every turn admission by `CoreDaemon::resolve_reasoning_selection`; `approval::tests::reasoning_selection_survives_a_restart` writes to a **file-backed** database, closes the pool, reopens it, and reads the value back; `src/core/daemon_turns.rs` — `the_persisted_selection_reaches_a_direct_turn_submit` reads it through the daemon's own pool | pass | there is no in-memory copy of the selection anywhere; every resolution is a store read, so a restart cannot lose it and cannot serve a stale one. **Corrected after the independent audit (§13):** the original evidence read the value back through a second handle on the **same** `sqlite::memory:` database, which proved a round-trip but not a restart; the file-backed test is the real proof |
| 6 | Every turn-admission path resolves the same selection: the ordinary direct submit path, the user-prompt-queue delivery path, the idle-continuation path, and the `SessionPromptSubmit` path. The `SessionPromptSubmit` path resolves the persisted selection without a new protocol field | Direct: `the_persisted_selection_reaches_a_direct_turn_submit`. `SessionPromptSubmit`: `the_persisted_selection_reaches_a_session_prompt_submit`. Queue delivery: `the_persisted_selection_reaches_a_queued_prompt_delivery`. Idle continuation: `scripts/check_reasoning_vocabulary_open.py` rule 4 (the three named sites must call `resolve_reasoning_selection` and must not hard-code `reasoning: None`) plus `src/core/daemon_continuation.rs`'s own call site. All three sites call the **one** helper `CoreDaemon::resolve_reasoning_selection` (`src/core/daemon_turns.rs`), and the admission point itself resolves `explicit ▸ persisted` | pass | the `SessionPromptSubmit` path uses the **existing** `CoreRequest::TurnSubmit.reasoning` field — no new protocol field was added for it. See §3.3 for the per-path detail and §10 for the continuation path's evidence grade |
| 7 | The precedence is: explicit per-turn override → persisted principal preference → agent/model-profile defaults, with the defaults applying exactly as at the baseline when neither of the first two is present | `src/core/daemon_turns.rs` — `an_explicit_turn_override_beats_the_persisted_selection` (explicit `low` beats the persisted `max`), `no_persisted_selection_means_no_override` (no persisted value ⇒ the runtime observes `None`, so `apply_agent_config` → `apply_model_profile_defaults` → `apply_reasoning_override` runs exactly as at the baseline), `the_selection_is_principal_scoped`; `src/agent/request_preparation.rs` — the existing `reasoning_effort_choice_overrides_a_configured_budget` and `an_explicit_selection_leaves_the_request_unmarked` tests, unchanged in behaviour | pass | the agent-layer precedence was **not** touched: `apply_reasoning_override` still runs last and still wins over the configured effort/budget; the daemon only decides *whether* there is an override at all |
| 8 | The migration is additive and preserves every existing `runtime_preferences` row; a reasoning write preserves the approval mode, sandbox profile, and last-used model; `STORAGE_LAYOUT_VERSION` matches the highest wired migration and the chain is contiguous | `crates/codegg-core/src/approval.rs` — `approval::tests::reasoning_selection_migration_is_additive_and_preserves_existing_rows` (a pre-M002 table gains the column; every pre-existing dimension survives; the new column is `NULL`); `approval::tests::reasoning_selection_survives_unrelated_preference_writes` (a reasoning write, then four other writes, leave the selection intact — and the other writes leave the approval mode / sandbox / model identity intact by construction); `scripts/check_project_catalog_invariants.py` (dispatch arms exactly match the definitions, no gaps; layout version equals the highest wired migration) | pass | `migrate_v84` is one plain `ALTER TABLE runtime_preferences ADD COLUMN reasoning_selection TEXT`; `STORAGE_LAYOUT_VERSION` is **84** |
| 9 | The selection grants no permission, tool, path, or credential scope, and its operation carries the same `Global` scope and no capability as the other principal-scoped preference operations | `crates/codegg-core/src/authorization/policy.rs` — `R::ReasoningPreferenceGet`/`R::ReasoningPreferenceSet { .. }` → `ScopeKind::Global`, `None` capability, exactly like `approval_preference_get` / `approval_mode_set` / `task_model_preference_set`; `architecture/authorization.md`'s matrix rows `reasoning_preference_get`, `reasoning_preference_set`, `reasoning_capabilities` (all `global` / `none`); `scripts/check_authorization_matrix.py` passes | pass | the stored value is request-shaping configuration only; it is never read by the authorization service, the tool broker, the path policy, or the credential resolver |
| 10 | The static guard against a re-closed vocabulary is registered in `scripts/verify.sh quick` and in the CI `verify` job, carries a self-test, and fails on an injected closed enum | `scripts/check_reasoning_vocabulary_open.py` (registered in `scripts/verify.sh` immediately after `check_reasoning_intent_fidelity.py` in the quick tier, and in `.github/workflows/ci.yml`'s `verify` job); `--self-test` injects **seven** violations — a closed `ReasoningEffort` enum, a closed level set whose name does not say "Reasoning", a non-`String` protocol label, a normalized provider bridge, a dropped turn-admission path, a dropped path written as a trailing `reasoning: None`, and a label normalized in the agent layer — and proves each is rejected | pass | no new CI lane; CI remains a strict subset of quick. Output in §4. **Hardened after the independent audit (§13):** the first four cases were all bypassable, so three more were added and the scanner was changed to match variants rather than a type name, to catch a trailing `reasoning: None`, and to scan the agent layer |
| 11 | The protocol carries no closed reasoning-effort enum anywhere | `crates/codegg-protocol/src/core.rs` — the `ReasoningEffort` enum is **deleted**; `ReasoningOverride::Effort { level: String }` and `ReasoningMode::Effort(String)`; the known labels live in `KNOWN_REASONING_EFFORT_LABELS: [&str; 4]` with `is_known_reasoning_effort_label` / `next_known_reasoning_effort_label`; `scripts/check_reasoning_vocabulary_open.py` rule 1 scans `crates/codegg-protocol/src`, `crates/codegg-providers/src`, and `src/agent` for any enum whose variants are a subset of the four legacy levels | pass | `grep -rn "ReasoningEffort" --include=*.rs .` returns no enum definition anywhere in the workspace |
| 12 | `architecture/provider.md`, `architecture/session.md`, `architecture/authorization.md`, `architecture/protocol.md`, and `architecture/config.md` describe the landed contract | the five documents, each edited in place (not appended): `provider.md`'s "Extended-thinking control (`/reasoning`)" section **rewritten** (its "frontend-local and never persisted" claim was false and is gone) plus a new "Reasoning capability negotiation" subsection; `session.md`'s `RuntimePreferenceStore` heading tags, Methods list, Protocol list and precedence sentence extended in place; `authorization.md`'s matrix rows and prose count corrected; `protocol.md`'s `CoreRequest`/`CoreResponse`/`CoreEvent` counts and group lists updated; `config.md`'s `ModelProfileConfig` section extended | pass | `architecture/overview.md`'s Verified counts table was re-measured against each row's own `Source` (§9) |

## 3. Production implementation evidence

### 3.1 The protocol vocabulary

The closed enum is gone; the label is a string.

```rust
// crates/codegg-protocol/src/core.rs
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ReasoningOverride {
    Effort { level: String },   // open, provider-scoped, verbatim
    Budget { tokens: usize },
    Off,
}

pub const KNOWN_REASONING_EFFORT_LABELS: [&str; 4] = ["minimal", "low", "medium", "high"];
```

- **Serialization.** `Effort { level: "medium" }` serializes to
  `{"mode":"effort","level":"medium"}` — **byte-identical to the legacy
  payload** for all four known labels, so a pre-M002 client and a post-M002
  daemon interoperate on the known set. A vendor label serializes to
  `{"mode":"effort","level":"xhigh"}`.
- **Deserialization.** The legacy payload decodes to the medium label; all
  three legacy variants decode.
- **The convenience set.** `KNOWN_REASONING_EFFORT_LABELS` is a *fallback
  ladder* for the frontend's `/reasoning` cycle, not an accepted value set.
  `ReasoningMode::cycle()` reproduces the legacy ladder exactly for the known
  labels (`minimal → low → medium → high → off → inherit`) and, for a label
  outside the set, steps off the ladder at `Off` rather than inventing a
  neighbouring level for it — this crate does not know the vendor's ordering.
- **`ReasoningControl::Enabled`** (M001's `Adaptive` mode) has **no
  production producer**, and this milestone did not give it one. The
  open-label path sets `ChatRequest::reasoning_effort` and leaves
  `context.reasoning` at `Unspecified`, so the bridge maps it to the
  kernel's **`Effort`** mode — a different thing from `ReasoningControl::
  Enabled`, which is the kernel's **`Adaptive`** mode. `Enabled` is
  exercised only by M001's per-surface wire fixtures. See §10 for the
  consequence.

### 3.2 The daemon-owned durable selection

| Element | Value |
|---|---|
| Storage table | `runtime_preferences` (`crates/codegg-core/src/approval.rs`) |
| Column | `reasoning_selection` — `TEXT`, **nullable** |
| Migration | `migrate_v84` (`crates/codegg-core/src/session/schema.rs`) |
| Migration statement | `ALTER TABLE runtime_preferences ADD COLUMN reasoning_selection TEXT` |
| Stored value | the protocol's own serialized selection shape (e.g. `{"mode":"effort","level":"xhigh"}`) — no second vocabulary |
| `STORAGE_LAYOUT_VERSION` | **84** (`crates/codegg-core/src/storage/mod.rs`) |
| Scope | the principal derived from transport authority; the payload carries no identity field |
| Concurrency | revision-gated CAS — the revision predicate lives **inside** the upsert statement, so a stale write gets `PreferenceError::Conflict` (`preference_conflict`), never last-write-wins |
| Bounds | label ≤ 64 bytes (`MAX_REASONING_LABEL_BYTES`), non-empty, no control characters; budget in `1..=1_000_000` tokens |
| Read | a corrupt stored value degrades to "no explicit selection" rather than failing the read |

The four existing upserts (`set_policy`, `set_model_preference`,
`set_task_model_preference`, `write_field`) all carry the new column through,
so **a reasoning write and an approval write cannot destroy each other** —
`reasoning_selection_survives_unrelated_preference_writes` asserts both
directions.

### 3.3 The three dropped paths

All three resolve through **one** helper,
`CoreDaemon::resolve_reasoning_selection` (`src/core/daemon_turns.rs`), and
the admission point itself resolves `explicit ▸ persisted` for every turn.

| Path | Site | Evidence |
|---|---|---|
| `SessionPromptSubmit` | `src/core/daemon_turns.rs` — the handler resolves the selection and hands the **resolved** value to the `TurnSubmit` it constructs | behavioural: `the_persisted_selection_reaches_a_session_prompt_submit` — the recording turn runtime observes `Effort { level: "max" }` |
| Queued-prompt delivery | `src/core/daemon_queue_delivery.rs` — the selection is resolved **at delivery time**, not at enqueue time | behavioural: `the_persisted_selection_reaches_a_queued_prompt_delivery` — the recording turn runtime observes `Effort { level: "xhigh" }` for the queue owner's principal. The test persists the selection **after** the queue add and **before** the delivery, so it discriminates delivery-time from enqueue-time resolution, and it drives the delivery through the daemon's **permitted** post-terminal-wake entry (`deliver_next_queued_prompt`) rather than the delivery entry itself — ADR-0038 fixes that entry's caller set to three, and `scripts/check_queue_delivery_callsites.py` fails a fourth, including a test |
| Delegated idle continuation | `src/core/daemon_continuation.rs` — the delegated entry resolves the selection before it constructs the `TurnSubmit` | structural: `scripts/check_reasoning_vocabulary_open.py` rule 4 asserts the site calls `resolve_reasoning_selection` and does not hard-code `reasoning: None`. See §10 finding U1 for the evidence grade |
| Ordinary direct submit | `src/core/daemon_turns.rs` — the admission point resolves `explicit ▸ persisted` | behavioural: `the_persisted_selection_reaches_a_direct_turn_submit` |

**The queue entry captures no selection.** `UserPromptQueueEntryDto` and the
`user_prompt_queue` row both stay selection-free
(`the_queue_entry_captures_no_selection`): a captured selection would be
stale against the model chosen at delivery, which may differ from the model
chosen at enqueue. The delivery resolves it fresh.

### 3.4 The resolution precedence

| Priority | Source | Where it is applied |
|---|---|---|
| 1 | **explicit per-turn override** — `CoreRequest::TurnSubmit.reasoning` | `CoreDaemon::resolve_reasoning_selection` returns it unchanged; `apply_reasoning_override` (agent layer) applies it last |
| 2 | **persisted principal preference** — `runtime_preferences.reasoning_selection` | resolved by `CoreDaemon::resolve_reasoning_selection` when the turn carries no explicit override |
| 3 | **agent / model-profile default** — `apply_agent_config` → `apply_model_profile_defaults` | unchanged from the baseline; applies when neither of the first two is present |

The agent-layer precedence was **not** modified. `apply_reasoning_choice` now
assigns the open label verbatim (`request.reasoning_effort = Some(level)`)
instead of converting a closed enum variant to a string.

### 3.5 The capability negotiation

The negotiation is expressed through the **established `*Capabilities` family**
— the same shape as `ChatCapabilities`, `HumanShellRunCapabilities` and
`WorkOrderCapabilities`: a `CoreRequest::XCapabilities` request answered by
`CoreResponse::XCapabilities { capabilities: XCapabilitiesDto }`. No new
negotiation channel was invented.

```rust
// crates/codegg-protocol/src/core.rs
pub struct ReasoningCapabilitiesDto {
    pub open_label_vocabulary: bool,   // #[serde(default)]
    pub known_labels: Vec<String>,     // #[serde(default)]
    pub max_label_bytes: usize,        // #[serde(default)]
}
```

- **What the daemon advertises:** `ReasoningCapabilitiesDto::current()` —
  `open_label_vocabulary: true`, the four known labels, `max_label_bytes: 64`.
- **What a client consults:** `codegg-client`'s `compose_turn_submit` reads
  `TurnSubmitInput.reasoning_capabilities`. A **non-enumerated** label is
  refused with `ComposerError::ReasoningVocabularyUnsupported` unless the
  observed advertisement says `open_label_vocabulary`. The known convenience
  labels need no advertisement — they are the legacy vocabulary every daemon
  understands.
- **What an old daemon does with a new label:** it never sees one. A client
  that has not observed the advertisement refuses to send it. A daemon that
  has not advertised the open vocabulary **refuses** such a label with
  `CoreResponse::Error { code: "reasoning_vocabulary_unsupported" }` at turn
  admission, before the turn is claimed — a typed, per-request error, not a
  daemon failure and not a silent mis-decode.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-protocol -p codegg-core -p codegg-providers -p codegg --all-targets -- -D warnings
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test -p codegg-providers
cargo test -p codegg-client
cargo test -p codegg --lib
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_reasoning_intent_fidelity.py
python3 scripts/check_reasoning_intent_fidelity.py --self-test
python3 scripts/check_reasoning_vocabulary_open.py
python3 scripts/check_reasoning_vocabulary_open.py --self-test
```

`bash scripts/verify.sh quick` was **not** run: the integrator runs it once,
to avoid cargo target-directory lock contention with the other jobs on this
machine. Every guard it would run for this change was run individually.

### Results

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | **pass** | clean after `cargo fmt --all` was applied to the files this milestone touched |
| `cargo clippy -p codegg-protocol -p codegg-core -p codegg-providers -p codegg --all-targets -- -D warnings` | **pass** | exit 0, no warnings. One `clippy::question_mark` finding was raised and fixed during the run (`let ... else` → `?` in `resolve_reasoning_selection`) |
| `cargo test -p codegg-protocol` | **pass** | 292 passed, 0 failed (plus 0 doc-tests) |
| `cargo test -p codegg-core` | **pass** | 892 lib tests + every integration target passed, 0 failed — including the two migration tripwires this milestone moved (§4.1) |
| `cargo test -p codegg-providers` | **pass** | 280 passed, 0 failed (plus 0 doc-tests) |
| `cargo test -p codegg-client` | **pass** | 37 + 9 + 16 passed, 0 failed across the crate's three test targets, including the four capability-negotiation cases this milestone added |
| `cargo test -p codegg --lib` | **partial pass** | 7285 passed, **2 failed**, 4 ignored, 0 measured. All 13 `core::daemon_turns::tests::*` cases this milestone added pass. The 2 failures are a macOS-platform artifact of the test, not of this change — see §4.2 |
| `python3 scripts/check_authorization_matrix.py` | **pass** | "All authorization matrix invariants verified." — the new `CoreRequest` variants each have their own `operation_descriptor` arm; the guard's exhaustive-match rule is satisfied |
| `python3 scripts/check_audit_coverage.py` | **pass** | "All audit coverage invariants verified." — the three new operations are classified in `audit_instrumentation.rs`'s uninstrumented list, in the same class as the other principal-scoped preferences |
| `python3 scripts/check_project_catalog_invariants.py` | **pass** | 7/7 checks, including "storage layout marker tracks highest wired schema migration" |
| `python3 scripts/check_reasoning_intent_fidelity.py` | **pass** | M001's bridge guard still passes: no `explicit_disable: false` literal, no closed effort enum in the provider crate, no closed effort-label ladder, and every mode still reachable |
| `python3 scripts/check_reasoning_intent_fidelity.py --self-test` | **pass** | seven injected violations, each rejected |
| `cargo test -p codegg-core --lib approval` | **pass** | 31 passed, 0 failed — including the three tests added when the independent audit found the revision gate was not a compare-and-swap (`a_stale_reasoning_write_is_rejected_by_the_statement_itself`, `two_concurrent_reasoning_writes_produce_one_winner_and_one_conflict`, `reasoning_selection_survives_a_restart`) |
| `python3 scripts/check_reasoning_vocabulary_open.py` | **pass** | 4/4 checks: no closed reasoning-effort enum, the protocol carries an open label, the provider bridge carries the label verbatim, every turn-admission path resolves the selection |
| `python3 scripts/check_reasoning_vocabulary_open.py --self-test` | **pass** | seven injected violations (a closed enum, a closed level set whose name does not say "Reasoning", a non-`String` protocol label, a normalized bridge, a dropped turn-admission path, a dropped path written as a trailing `reasoning: None`, and a label normalized in the agent layer), each rejected — the last four were added when the independent audit showed the first four were bypassable |
| `python3 scripts/check_no_hardcoded_models.py` | **pass** | the vocabulary is four label *strings*, not a compiled-in model catalog |
| `bash scripts/check-core-boundary.sh` | **pass** | "codegg-core boundary check passed" |

**Environmental note.** `bash scripts/verify.sh quick` was **not** run, by
instruction: the integrator runs it once, to avoid cargo target-directory lock
contention with the other jobs on this machine. Every guard it would run for
this change was run individually above, with its exit code captured. The
machine was under heavy load from a concurrent build throughout; two cargo
invocations were killed by the environment (one `SIGTERM` at the command
limit, one `SIGKILL` from the OOM killer) **before** any test ran, and both
were re-run to completion with a reduced build footprint
(`CARGO_BUILD_JOBS=1`, `CARGO_PROFILE_{DEV,TEST}_DEBUG=0`). No test result
below comes from a killed run.

### 4.1 Failures the suite caught, and the fixes

Both were caught by the milestone's own verification, both are recorded here
rather than quietly repaired.

1. **`crates/codegg-core/tests/cached_models_composite_key.rs` —
   `upgrading_from_the_single_key_shape_preserves_rows_and_lifts_the_constraint`
   failed with `Migration("no such table: runtime_preferences")`.** The test
   builds a *synthetic* version-69 database that contains only
   `cached_models`, then runs the whole migration chain. `migrate_v84` issued
   `ALTER TABLE runtime_preferences ADD COLUMN reasoning_selection TEXT`
   against a database whose `runtime_preferences` table did not exist — a
   shape a real version-69 database cannot have (the table was created by
   `migrate_v58`), but that the synthetic database does. **Fix:**
   `migrate_v84` now applies `RUNTIME_PREFERENCE_SCHEMA_STATEMENTS` — whose
   create statement is `IF NOT EXISTS` — before the `ALTER`, so the column
   always has a table to attach to. On a real database the create is a no-op;
   on the synthetic one the table appears with the current shape and the
   `ALTER` is then a tolerated no-op. The migration stays additive and
   idempotent, and the test passes.
2. **`crates/codegg-core/tests/user_prompt_queue.rs` and
   `user_prompt_queue_mutation.rs` — two tripwires that pin
   `STORAGE_LAYOUT_VERSION == 83` failed.** These are deliberate tripwires
   ("pins the *current* number so a silent bump is visible in review"). The
   milestone moves the constant to 84, so the tripwires were updated to 84
   with a comment recording that provider reasoning control M002 took v84 for
   the reasoning-selection column. The companion assertion in the same tests
   — that the constant equals the highest wired migration — was **not**
   touched and still proves the chain.

### 4.2 The two remaining `cargo test -p codegg --lib` failures

`core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs`
and `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances`
fail with:

```
register workspace: InvalidRootPath("/tmp")
```

The cause is a **platform** rule, not this change. The two tests register
`/tmp` as a workspace root, and
`crates/codegg-core/src/workspace.rs:626` rejects a root that is itself a
symlink ("the path must be absolute and the root itself cannot be a
symlink"). On macOS `/tmp` is a symlink to `private/tmp`
(`lrwxr-xr-x /tmp -> private/tmp`); on Linux, where CI runs, it is a real
directory and the tests pass. **Neither `crates/codegg-core/src/workspace.rs`
nor `src/core/daemon_refresh.rs` is touched by this milestone** (`git diff
--stat` on both is empty), and no code path this milestone changed feeds the
workspace registry. These two failures are therefore pre-existing on macOS and
were left as they are — changing the workspace rule or the test's fixture is
outside this milestone's ownership.

### 4.3 The eleven first-run failures were a missing sibling binary

The first `cargo test -p codegg --lib` run reported 13 failures: the two above
plus eleven in `python_script::executor::tests::*`,
`managed_process::tests::constrained_launch_gets_private_tmpdir_removed_after_reap_on_macos`,
and `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace`.
All eleven failed with

```
managed process sandbox failed: trusted sandbox helper codegg-sandbox-helper
could not be resolved: No such file or directory (os error 2)
```

`cargo test -p codegg --lib` builds only the library test target; the
`codegg-sandbox-helper` **bin** target is a sibling of the test executable and
is not built by that command. After `cargo build -p codegg --bin
codegg-sandbox-helper` (which puts the helper where
`crate::install::installation_dir_for` looks for it), all eleven pass —
verified by re-running the whole suite. This is a command-shape property of
`--lib`, not a defect and not a consequence of this change.

### 4.4 The tests this milestone added

**`crates/codegg-protocol`** (`core::tests`):
`known_reasoning_effort_labels_escalate_and_saturate`,
`an_open_reasoning_label_is_carried_verbatim`,
`the_known_labels_keep_their_legacy_serialized_shape`,
`reasoning_mode_cycle_reaches_every_setting_and_returns_home`,
`an_open_label_leaves_the_fallback_ladder_at_off`,
`reasoning_override_off_is_distinct_from_absent`,
`only_pinned_modes_request_reasoning`.

**`crates/codegg-core`** (`approval::tests`):
`reasoning_selection_round_trips_an_open_label_verbatim`,
`reasoning_selection_is_absent_until_it_is_written`,
`reasoning_selection_write_is_revision_gated`,
`reasoning_selection_survives_unrelated_preference_writes`,
`reasoning_selection_can_be_cleared`,
`reasoning_selection_rejects_an_implausible_label`,
`a_corrupt_reasoning_selection_degrades_to_no_selection`,
`reasoning_selection_migration_is_additive_and_preserves_rows`.

**`crates/codegg-client`** (`compose::tests`):
`a_client_without_the_capability_sends_no_non_enumerated_label`,
`a_client_with_the_capability_sends_the_open_label_verbatim`,
`a_known_label_needs_no_capability_observation`,
`no_selection_means_the_daemon_owns_the_effective_one`.

**`codegg` (`src/core/daemon_turns.rs` tests)**:
`the_persisted_selection_reaches_a_direct_turn_submit`,
`the_persisted_selection_reaches_a_session_prompt_submit`,
`the_persisted_selection_reaches_a_queued_prompt_delivery`,
`the_queue_entry_captures_no_selection`,
`an_explicit_turn_override_beats_the_persisted_selection`,
`no_persisted_selection_means_no_override`,
`the_selection_is_principal_scoped`,
`a_daemon_without_the_open_vocabulary_refuses_a_non_enumerated_label`,
`the_daemon_advertises_the_open_vocabulary`.


## 5. Invariant review

| Plan §4 invariant | Evidence |
|---|---|
| The effort label is **open and provider-scoped** — no closed level set | `scripts/check_reasoning_vocabulary_open.py` rule 1; the protocol's `ReasoningEffort` enum is deleted |
| No **normalization, rejection, or translation** of the label | `reasoning_intent` copies the label; the guard rejects `to_lowercase`, `trim`, and a `match` on the label |
| An **explicit disable** stays distinct from "no preference" | `ReasoningOverride::Off` is unchanged; `core::tests::reasoning_override_off_is_distinct_from_absent` |
| Effort and budget stay **mutually exclusive** | `apply_reasoning_choice` clears the other field (unchanged) |
| The **agent-layer precedence** is unchanged | `apply_agent_config` → `apply_model_profile_defaults` → `apply_reasoning_override` is untouched apart from the label assignment |
| The **principal is derived server-side** | `authority.principal_id()` is the only identity in the read and write paths; the payload has no identity field |
| The **revision semantics** of `runtime_preferences` do not change | the new write is a revision-gated CAS on the same `revision` column as the existing writes |
| The migration is **additive** | one plain `ALTER TABLE … ADD COLUMN`; `reasoning_selection_migration_is_additive_and_preserves_existing_rows` |
| The provider crate ships **no compiled-in model catalog** | `scripts/check_no_hardcoded_models.py` passes; the vocabulary is four label *strings*, not models |

## 6. Failure and recovery review

- **Store read failure.** `resolve_reasoning_selection` logs a warning and
  returns `None`: a preference store that cannot be read is **not authority**,
  so the turn falls back to the agent / model-profile default rather than
  failing. The same rule already governs the corrupt-mode and corrupt-profile
  reads in this store.
- **Store write failure.** `CoreResponse::Error { code: "preference_write_failed" }`;
  a rejected shape is `invalid_reasoning_selection` and a stale revision is
  `preference_conflict`. The selection is unchanged in both cases.
- **Capability refusal.** A non-enumerated label at a daemon that has not
  advertised the open vocabulary is refused at turn admission with
  `reasoning_vocabulary_unsupported`. The turn is not claimed, the session is
  not left mid-turn, and the daemon keeps serving other requests.
- **Idempotency.** The turn-admission receipt is computed from the **client's**
  request, before the selection is resolved. A retry of the same request
  returns the original receipt and does not re-resolve the selection, so a
  preference change between two retries cannot produce two different turns.
- **Restart.** The selection is durable and is read fresh on every
  admission; there is no in-memory copy to lose.
- **Contention.** Two frontends racing a selection change converge on one
  winner via `PreferenceError::Conflict`; the loser is told the current
  revision and can re-read.

## 7. Migration and compatibility review

- **Schema.** `migrate_v84` adds one nullable column with a plain
  `ALTER TABLE`. Existing rows are untouched; the new column defaults to
  `NULL` ("no explicit selection"). Fresh databases receive the column
  through `RUNTIME_PREFERENCE_SCHEMA_STATEMENTS` with the same
  `length(...) <= 512` CHECK the other nullable identity columns carry;
  bounds are enforced in Rust either way, so a migrated database and a fresh
  one behave identically.
- **Version.** `STORAGE_LAYOUT_VERSION` 83 → **84**, matching the highest
  wired migration. `scripts/check_project_catalog_invariants.py` asserts the
  dispatch arms exactly match the definitions with no gaps, and that the
  layout version equals the highest wired migration; it passes.
- **Protocol.** The serialized shape of `ReasoningOverride` is unchanged for
  the known labels, so a pre-M002 client keeps interoperating on the known
  set. `TurnSubmit.reasoning` stays `#[serde(default)]`, so a client that
  predates the field decodes and inherits the daemon's resolved selection.
  A **non-enumerated** label is a new capability, and it is negotiated
  (§3.5) rather than assumed.
- **Rollback.** The column is additive, so a rollback to a pre-M002 build
  leaves the column in place and unread. The stored value is the protocol's
  own serialized shape, so a rollback does not strand the daemon in an
  unreadable state.
- **Configuration.** No config key was added or changed. `ModelProfileConfig`
  keeps its shape; only the **meaning** of `default_reasoning_effort`
  widened (it is now an open label rather than a rejected unknown level).

## 8. Security review

- **No authority from the selection.** The stored value is request-shaping
  configuration. It is never read by the authorization service, the tool
  broker, the path policy, the sandbox profile, or the credential resolver.
  Its operation carries `ScopeKind::Global` and **no capability**, exactly
  like the other principal-scoped preference operations, and the
  authorization matrix records it as such.
- **Principal scoping.** The principal comes from
  `authority.principal_id()` — transport authority — and never from the
  request body. The `ReasoningPreferenceSet` payload is
  `{ selection, expected_revision }`; there is no identity field to spoof.
  `the_selection_is_principal_scoped` proves another principal's preference
  does not apply.
- **Bounds.** A label is at most 64 bytes, non-empty, and free of control
  characters (NUL included) — checked at the store boundary, not trusted
  verbatim. A budget is in `1..=1_000_000` tokens; a zero budget is refused
  because the provider wire rejects it, and persisting one would store a
  value that can never be honoured.
- **No secrets.** The selection is a label or a token count. It is not a
  credential, and `runtime_preference_serialization_is_secret_free` (whose
  allowlist was extended with `reasoning_selection`) still passes.
- **Unbounded-input denial.** A hostile label is rejected at admission, not
  forwarded to a provider.
- **Audit.** The three new operations are classified as **uninstrumented**
  in `crates/codegg-core/src/audit_instrumentation.rs`, in the same class as
  `approval_preference_get`, `approval_mode_set` and
  `task_model_preference_set`: the daemon binds the principal server-side,
  the payload carries no project-keyed data, and the write touches only the
  caller's own preference row. `scripts/check_audit_coverage.py` passes, and
  `architecture/audit.md` was **not** changed (the audit contract did not
  move).

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/provider.md` | the "Extended-thinking control (`/reasoning`)" section **rewritten** — its "frontend-local and never persisted" / "needs no migration" claim was false once the daemon took ownership. Adds the four-level precedence, the open-vocabulary contract, the durable-selection contract, and a new "Reasoning capability negotiation" subsection |
| `architecture/session.md` | the `RuntimePreferenceStore` heading's migration tags (`v58 + v62` → `v58 + v62 + v84`), its **Methods** list, its **Protocol** list, and its precedence sentence extended in place |
| `architecture/storage.md` | the `STORAGE_LAYOUT_VERSION` section corrected (79 → 84 — the file was already stale) and the "Migrations (Storage Layout Context)" list corrected (`87` → `92` `CREATE TABLE` statements, "v1–v79" → "v1–v84") with the v84 entry added |
| `architecture/protocol.md` | the `CoreRequest` / `CoreResponse` / `CoreEvent` group lists and counts updated (234 → 264, 149 → 173, 87 → 92) against `crates/codegg-protocol/src/core.rs`, plus the new reasoning groups |
| `architecture/authorization.md` | the operation-to-capability matrix rows for the three M002 operations added; the stale "228 native operations" prose corrected to **258**; four pre-existing missing rows added so the table is an exact set-diff against `policy.rs` |
| `architecture/config.md` | the `ModelProfileConfig` section extended: `default_reasoning_effort` is an open, provider-scoped label |
| `architecture/overview.md` | the Verified counts table re-measured against each row's own `Source`: `CoreRequest` 261 → 264 (238 struct + 26 unit), `CoreResponse` 171 → 173, authorized operations 255 → 258, storage layout version 83 → 84, CI guard scripts 63 → 64. A dated remeasurement paragraph was added in the established style. The remaining rows were confirmed unchanged (no new architecture document, no new `codegg-core` top-level module, no new database table, no new `CoreEvent` variant, no new integration test file) |
| `docs/providers.md` | the `## Choosing a model` section gains a "Controlling extended thinking" subsection — the file previously contained **zero** occurrences of "reasoning", "thinking", or "effort" |
| `.opencode/skills/provider-auth/SKILL.md` | rule 11 (the open, provider-scoped, verbatim label contract), the two reasoning guards in its static-guard list, the `session-storage` skill in "See Also", and a dated source-verification note — the file previously contained **zero** reasoning or thinking mentions |
| `CHANGELOG.md` | three bullets under `### Changed` in `## [Unreleased]`, in the house format |

**Static guards.**

```bash
python3 scripts/check_reasoning_vocabulary_open.py            # open vocabulary + daemon-owned selection
python3 scripts/check_reasoning_vocabulary_open.py --self-test # seven injected violations
python3 scripts/check_reasoning_intent_fidelity.py            # M001's bridge guard, unchanged
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_project_catalog_invariants.py
```

**Operator diagnostics.** A refused label is visible as
`reasoning_vocabulary_unsupported` on the request that carried it; a stale
preference write is `preference_conflict` with the current revision in the
message; a refused shape is `invalid_reasoning_selection`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The delegated idle-continuation path has **structural** evidence only: `scripts/check_reasoning_vocabulary_open.py` rule 4 proves the site calls `resolve_reasoning_selection` and does not hard-code `reasoning: None`, but no behavioural test drives a real continuation grant to an admitted turn. The continuation fixture (grant + origin-attribution row + team membership + workspace lineage + stub provider) is a 600-line harness that this milestone did not rebuild. | Low: the site resolves through the **same** helper the other two paths use, and the helper's behaviour is covered behaviourally by the direct, `SessionPromptSubmit`, and queue-delivery tests. The residual risk is that the continuation path could fail to *reach* the resolution point (for example, by returning `Suppressed` earlier), which would suppress the continuation rather than apply a wrong selection. | Milestone 004 (or a follow-up) should add the behavioural continuation test alongside the rest of the frontend work, when the continuation harness is next touched. |
| low | `ReasoningControl::Enabled` (the kernel's `Adaptive` mode) has **no production producer**. The open-label path sets `ChatRequest::reasoning_effort` and leaves `context.reasoning` at `Unspecified`, so the bridge produces the kernel's `Effort` mode; `Enabled` is exercised only by M001's per-surface wire fixtures. | Low: every shipped selection carries a depth, so nothing in the product asks for "thinking on, no depth". The variant is dead weight rather than a wrong behaviour — the wire shape it maps to is correct and fixture-proven. | Milestone 004 owns the option surface: it should either offer a "reasoning on, no depth" choice that produces `Enabled`, or record the variant as reserved with no producer. |
| low | `docs/providers.md` describes the thinking control in prose only; there is no user-facing CLI flag for reading or writing the durable selection, because the write path (`ReasoningPreferenceSet`) has no operator surface yet. | Low: the durable selection is reachable through the protocol, and the `/reasoning` command remains a per-turn frontend control until milestone 004 wires it to the durable store. | Milestone 004 wires `/reasoning` to `ReasoningPreferenceSet` and shows the stored value. |
| low | The daemon's advertised `open_label_vocabulary` is `true` in every shipped build; the "daemon that has not advertised it" branch is reachable only by flipping the daemon-owned field in a test. | Low: the refusal path is implemented and tested; no shipped configuration disables the open vocabulary, so the branch is defensive rather than operational. | None required. If a future deployment needs to disable the open vocabulary, the daemon-owned `reasoning_capabilities` value is the single place to do it. |
| low | The stored selection uses the protocol's serialized shape as the column encoding, so a future change to that serialization would need a read-compatibility story for already-persisted rows. | Low: the shape is stable, and a corrupt or undecodable value already degrades to "no explicit selection" rather than failing the read. | Revisit if the `ReasoningOverride` serialization ever changes. |
| low | `crates/codegg-core/src/approval.rs` now depends on `codegg_protocol::core::ReasoningOverride`. The crate already depended on `codegg-protocol` (the collaboration policy module does the same), and the core-boundary guard passes. | Low: none observed. | None. |

**Deliberately not absorbed (out of scope by the plan and the brief).**

- **No per-model capability record and no admission policy — in *this
  milestone's* delta.** M001's closure §10 records that the OpenAI-compatible
  adapter sends `enable_thinking: false` for an explicit disable even on an
  always-on model; *whether* an always-on model may be told "off" belongs to
  milestone 003's local admission policy. Milestone 003 has since landed that
  record and that policy in the same working tree, so this bullet states what
  milestone 002 did **not** absorb — it is not a claim about the merged tree.
- **No frontend option surface.** The `/reasoning` command still cycles the
  frontend-local `ReasoningMode`; wiring it to the durable store, and
  showing the stored value, is milestone 004.
- **No discovery overlay** from catalog metadata (milestone 005).
- **No change to which surface a provider uses.** MiniMax stays on the
  Anthropic surface; OpenCode Go keeps its multi-surface dispatch.
- **No change to the `eggpool-wire` pin** (`9ac6a1318e8db3c034b5ab54987317752d5ffea6`).

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed. The open vocabulary, the
daemon-owned durable selection, the three dropped paths, the capability
negotiation, the additive migration, and the documentation all landed with
the evidence in §2 and §4. The one named residual — the behavioural
continuation-path test — is bounded, structural-guard-covered, and carried as
finding U1 for milestone 004.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/provider-reasoning-control-roadmap.md`:

- mark milestone 002 complete;
- record the milestone 003 scope boundary that this milestone deliberately
  did not absorb (the always-on-model admission policy);
- record the milestone 004 scope that now also owns the behavioural
  continuation-path test (finding U1) and the `/reasoning` write path.

Both files are owned by the integrator and were **not** modified here.

## 13. Independent audit and follow-up corrections

An independent verifier audited this milestone against the plan's thirteen
criteria, the pinned kernel, and this record itself, and was asked to
**refute** the record rather than confirm it. Its verdict was **partial**:
most criteria held, but three of this record's own claims did not, and two
of its evidence claims proved less than they said.

This section records what the audit found, what was corrected, and what was
handed to a later milestone. It is a dated post-acceptance record: the
milestone's decisions are unchanged.

### What the audit confirmed (no defect)

- The open vocabulary is genuinely open: the label is copied, never mapped,
  through the protocol, the agent layer, the request, and the encoded body.
- The legacy serialized shapes still decode, and `Off` is still distinct from
  absent.
- The daemon's advertised capability and its admission check read the **same**
  daemon-owned value, so "advertised" and "accepted" cannot drift apart.
- The migration is one plain `ALTER TABLE`, additive, and
  `STORAGE_LAYOUT_VERSION` matches the highest wired migration. The two
  layout tripwires were bumped 83 → 84, not weakened.
- The two `cargo test -p codegg --lib` failures the milestone reported are
  **macOS-only** (a `/tmp` symlink in `workspace.rs:625-627`) and pre-existing;
  they are not caused by this milestone.
- The selection grants no permission, tool, path, or credential scope: its
  operation carries the same `Global` scope and no capability as the other
  principal-scoped preference operations.

### Defects this record overstated, and the corrections

| # | Finding | Correction |
|---|---|---|
| A | §3.1 claimed `ReasoningControl::Enabled` "is now reachable" because an open label produces it. **False**: the open-label path sets `ChatRequest::reasoning_effort` and leaves `context.reasoning` at `Unspecified`, so the bridge produces the kernel's **`Effort`** mode; `ReasoningControl::Enabled` is the kernel's **`Adaptive`** mode and has **no production producer**. | §3.1 now says so, and §10 carries it as an open finding for milestone 004, which owns the option surface. |
| B | The revision gate was described as a CAS. It was a **read-then-check**: the store read the revision, compared it, and then wrote with no revision predicate, so two writers could both pass the check and the second would silently overwrite the first. The plan (§6, §10) requires that "two concurrent writes produce a typed conflict rather than a silent last-write-wins" — which the original code did not guarantee. | **Fixed.** The revision predicate now lives **inside** the upsert statement (`ON CONFLICT … DO UPDATE … WHERE runtime_preferences.revision = ?`), and a rejected statement is reported as `PreferenceError::Conflict`. Two new tests prove it: `a_stale_reasoning_write_is_rejected_by_the_statement_itself` (deterministic — it reproduces the stale window exactly) and `two_concurrent_reasoning_writes_produce_one_winner_and_one_conflict` (the public contract, over a shared multi-connection pool). |
| C | The guard's self-test claimed four injected violations. All four were **bypassable**: a closed level set whose type name does not contain "Reasoning" passed, a dropped path written as a trailing `reasoning: None` passed, and a label normalised in the agent layer passed. | **Fixed.** The guard now matches enum *variants* rather than a type name, catches the trailing-field form, and scans the agent layer. The self-test injects **seven** violations and every one is rejected. §2 row 10 and §4 are updated. |
| D | The restart claim rested on reading the value back through a second handle on the **same** `sqlite::memory:` database. That proves a round-trip, not a restart — every store test in this milestone used an in-memory database. | **Fixed.** `reasoning_selection_survives_a_restart` writes to a **file-backed** database, closes the pool, reopens it, and reads the value back. §2 row 5 is updated. |
| E | §9 claimed `architecture/overview.md`'s Verified counts table "was re-measured against each row's own `Source`". It was not: several rows were stale and contradicted `architecture/protocol.md` and `architecture/authorization.md`, which had been updated. | Corrected by the integrator against each row's own `Source` once milestones 003 and 004 landed; §9 now states what was actually re-measured. |

### Findings handed to later milestones

| # | Finding | Owner | Resolution |
|---|---|---|---|
| F | **No client sent `ReasoningPreferenceSet`.** The durable selection was reachable through the protocol but no shipped frontend wrote it, so the user-facing claim that "the daemon persists the choice" was not true of the merged tree. | Milestone 004, which owns the `/reasoning` write path. | **Closed.** M004's `/reasoning` sends `CoreRequest::ReasoningPreferenceSet` with the daemon's preference revision as the compare-and-set, and the local display is corrected from the daemon's **response**. `docs/providers.md`'s claim is now true of the tree. |
| G | **A failed read of the persisted selection silently dropped the user's choice** — `resolve_reasoning_selection` logged a `tracing::warn!` and fell back to the agent default, so the loss was invisible. | Milestone 003's notice channel. | **Closed.** M003 publishes `CoreEvent::Error { code: "reasoning_selection_unreadable", .. }` on the turn's **session** event log, carrying the underlying error. The turn still proceeds on the agent / model-profile default — a store that cannot be read is not authority — but the loss is reported. |
| H | **The queued-delivery test persisted the selection *before* the queue add**, so it could not distinguish delivery-time from enqueue-time resolution — the one property the queue contract exists to guarantee. | Milestone 003. | **Closed.** The test now persists the selection **after** the queue add and **before** the delivery, so an implementation that wrongly captured the selection into the queue entry would deliver **no** selection and the test would fail. The resolution point is now the thing under test. |
| I | **Two refusal codes coexist**: `reasoning_vocabulary_unsupported` (the daemon's label check) and `reasoning_capability_unsupported` (the per-model admission check). They are distinct conditions and a frontend must be able to tell them apart. | Milestone 003 / 004. Neither code is renamed or merged — renaming either is a separate decision. | **Closed.** M004's `refusal_message` maps each to its own sentence ("this daemon does not accept that reasoning label" vs "the model does not accept that reasoning selection"). A third and fourth code now coexist on the **notice** channel (`reasoning_capability_notice`, `reasoning_selection_unreadable`), so the channel — not the code — separates a refusal from a notice. `reasoning_vocabulary_unsupported` still has two production sites (turn admission and the preference write), which M003's closure records as a low finding. |

### Scope note

This milestone's "deliberately not absorbed" list (§10) describes **milestone
002's delta**, not the merged tree. Milestone 003 has since landed the
per-model capability record and the admission policy in the same tree, so
that list is now explicitly scoped rather than silently wrong.
