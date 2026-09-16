# Context Continuity and Compaction M002 — Authoritative Intent, Plan, and Frame Projection

Status: ready

Repository baseline: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable ADRs:

- None. M001's accepted continuation-store contract is the hard dependency.

Primary class: correctness / capability

## 1. Objective

Make the continuation state fed into compaction authoritative, bounded, and non-recursive.

M002 connects the M001 checkpoint contract to the state CodeGG already owns:

- active `Goal` and revision;
- plan path/digest/current phase where available;
- persisted/current todo state;
- immutable session-origin task;
- bounded exact user steering from the current context epoch;
- deterministic context-ledger evidence;
- semantic constraints/decisions/open errors/next steps.

It also fixes the current repeated-compaction defects where hybrid frames omit `user_goal`/`current_task`, old compaction frames accumulate as system messages, and stale goal checkpoint prefixes can outrank newer progress.

M002 may prepare/render checkpoint candidates in memory. M004 owns durable rollover sequencing and production activation.

## 2. Why this milestone is dependency-gated

M002 is ready once M001 closes because it needs one stable typed destination for the state it assembles. Building another interim frame/storage type before M001 would recreate the split ownership this roadmap is intended to remove.

No additional external dependency is expected.

## 3. Current implementation evidence

Implementation must re-inspect:

- `src/agent/request_preparation.rs::build_context_frame`
- `src/agent/context_frame.rs`
- `src/context/compaction.rs`
- `src/agent/context_runtime.rs::compact_if_needed`
- `src/agent/loop.rs` initialization of `original_user_prompt`
- `src/agent/turn_runtime.rs` active-goal prompt assembly
- `crates/codegg-core/src/goal/model.rs`
- `crates/codegg-core/src/goal/store.rs`
- `crates/codegg-core/src/goal/checkpoint.rs`
- `crates/codegg-core/src/goal/render.rs`
- `crates/codegg-core/src/session/store.rs` (`TodoStore`, relevant message/event access)
- `architecture/goal.md`
- `architecture/compaction.md`
- `architecture/context-ledger.md`
- M001's final continuation payload/store contract.

Baseline defects this milestone must resolve:

1. `build_context_frame()` sets `user_goal` from `original_user_prompt`, gets current task/next steps from todo state, and then merges ledger facts.
2. `build_programmatic_state()` creates a fresh frame with constraints/files/commands/tests/errors but leaves `user_goal` and `current_task` at default `None`.
3. `semantic_checkpoint()` therefore renders `"unknown"` and `"none"` for those fields and cannot return them because its JSON output schema only owns four semantic fields.
4. `compact_if_needed()` detects `[codegg compacted session state]` and skips its later `build_context_frame()` injection, so the incomplete hybrid frame can suppress the only frame that has the original task/todo state.
5. hybrid/programmatic compilation preserves all system messages, including prior CodeGG compacted-state messages, then adds another one.
6. `ContextLedgerState::artifact_handles` is recorded but omitted from `to_context_frame()`.
7. active goal context is assembled independently at turn start, while compaction later reconstructs state without consulting the active `Goal`.
8. `read_checkpoint_excerpt()` returns the first N characters of an append-only goal checkpoint file. Newer progress appended after that prefix may disappear from model context.
9. `select_retained_messages()` uses the first tool call when finding pair partners for retained multi-tool assistant messages rather than resolving every tool-call ID.

## 4. Invariants

M002 MUST preserve:

- session-origin user prompt as immutable provenance;
- active `Goal.objective` + `Goal.revision` as authoritative long-horizon goal state when an active goal exists;
- later user steering as separate ordered intent entries rather than mutation of the origin task;
- host-owned todo/plan/file/test facts as authoritative over model prose;
- semantic checkpoint output as advisory/enrichment, never an authority for goal revision, file paths, commands, tests, or completion;
- exactly one current CodeGG-owned continuation/compaction frame in compiled history;
- exact tool-call/result invariants, including multiple calls from one assistant message;
- model-visible state within explicit token/byte bounds;
- goal Markdown checkpoint as user-facing historical journal, not canonical current-state authority;
- existing goal tools and `GoalStore` behavior;
- no dependence on a specific provider/model for host state assembly.

## 5. Scope

### In scope

- continuation snapshot assembly;
- source precedence and provenance;
- exact bounded user-intent spine;
- goal/todo/plan state projection;
- semantic baseline carry-forward/update rules;
- current-frame rendering;
- removal/supersession of old CodeGG-owned frames;
- multi-tool retained-message correction;
- goal checkpoint excerpt recency fix;
- bounded artifact-reference projection into continuation state;
- unit/integration tests.

### Out of scope

- persistence/install transaction sequencing (M004);
- new context handle kinds and evidence artifact creation (M003);
- semantic search over old history;
- changing goal completion verification;
- changing TodoStore identity model;
- arbitrary parsing of implementation-plan Markdown into a new task graph;
- model-initiated `new_context`-style rollover.

## 6. Required production changes

### 6.1 Introduce one authoritative continuation snapshot assembler

Add a single host-side assembler under the established context/runtime ownership, for example:

```text
src/context/continuation.rs
```

or another location consistent with M001's final API.

The assembler should accept typed inputs, not reach through `AgentLoop` globals opportunistically. A suitable request shape includes:

```text
session_id
session-origin prompt
current provider-visible messages / current user turn
active Goal (optional)
persisted/current todos
current ContextLedgerState
recent security findings
previous installed ContinuationCheckpoint (optional)
active model/token policy
```

Storage lookups may happen in the agent/turn adapter before calling the pure assembler if that better preserves ownership.

The result should be a typed checkpoint payload/candidate accepted by the M001 store.

### 6.2 Define field ownership and precedence

Document and encode the following precedence:

**Objective / goal**

1. Active durable `Goal.objective` + goal ID/revision when present.
2. Otherwise immutable `original_user_prompt`.
3. Never replace either with an LLM paraphrase.

Keep the session-origin task digest/text as provenance even when an active goal is authoritative.

**Current task**

1. Explicit active goal `current_phase` / `next_action` where it identifies the current unit of work.
2. In-progress todo.
3. Most recent host-owned continuation next action.
4. No invented fallback string.

**Plan**

Record metadata, not an unbounded file body:

- `plan_path` when present;
- stable SHA-256 digest of the plan content when readable under the current workspace;
- current phase/action;
- bounded completed/pending/blocked item descriptors already represented by goal/todos/events;
- an explicit `plan_unavailable` diagnostic if the path cannot be read.

Do not infer project identity from the plan path.

**Deterministic evidence**

Touched files, commands, tests, unresolved errors, security findings, and existing artifact handles come from host/runtime state.

**Semantic fields**

Constraints, decisions, unresolved semantic blockers, and next steps may be enriched by the compaction model, but M002 must merge them against previous installed semantic state plus current-epoch evidence rather than regenerate them solely from a previous generated summary.

### 6.3 Exact user intent spine

Preserve exact user-authored text for the parts of the task most likely to alter trajectory.

At minimum the spine must deterministically include:

- the immutable origin request when no active goal supersedes it as current objective, while still retaining origin provenance;
- the most recent user steering/correction messages since the previous installed checkpoint;
- any current user message that triggers the compaction turn.

Use a bounded budget. A reasonable starting policy is comparable to Codex's separate retained-user budget (order of 20k estimated tokens total), but implementation should use CodeGG token estimation and record truncation diagnostics.

When the exact text exceeds the budget:

1. retain origin/current steering boundaries rather than arbitrary middle slices;
2. store content digest;
3. preserve a recovery reference when M003 later provides one;
4. clearly mark that the inline text is bounded/truncated.

Do not treat every assistant response as intent.

M002 does not require durable MessageStore IDs because the current provider DTO/history path does not expose a reliable role-aware durable message identity. Use checkpoint-owned intent IDs/digests now; M003 may add exact evidence references without inventing false MessageStore provenance.

### 6.4 Eliminate compaction-frame accumulation

Add an explicit recognizer for CodeGG-owned continuation frames. It must identify only CodeGG's own marker/version, not arbitrary user/system text.

Before compiling replacement history:

- remove/supersede earlier CodeGG continuation frames;
- emit exactly one current frame generated from the new typed snapshot;
- preserve unrelated system/developer instructions.

Version the marker, for example:

```text
[codegg continuation state v1]
```

Do not rely indefinitely on an unversioned free-form prefix. Recognize the old `[codegg compacted session state]` marker for migration/cleanup, but render the new versioned form.

Tests must prove 5+ repeated compactions still contain one current frame.

### 6.5 Fix hybrid goal/current-task loss

Change the semantic/checkpoint API so the reduced programmatic state receives the host-owned baseline before semantic enrichment.

A suitable direction is:

```text
build_programmatic_state(messages, config, authoritative_baseline)
semantic_checkpoint(reduced_current_epoch, authoritative_baseline, ...)
```

The semantic model should never receive `"unknown"` for an objective that the host already knows.

Its structured output remains narrowly owned; it must not return/override goal ID, goal revision, plan digest, file list, command list, or test state.

### 6.6 Carry bounded artifact handles

Promote `ContextLedgerState::artifact_handles` into the continuation snapshot with a cap and deterministic recency order.

Do not dump the currently unlimited vector into the prompt.

Use a policy such as:

- retain the most recent N handles (for example 32);
- deduplicate;
- include handle + short host-generated label/summary where already available;
- M003 may replace generic handles with richer exact evidence refs.

### 6.7 Correct multi-tool retention

Replace `.first()`-based pairing in `select_retained_messages()` with all-tool-call resolution.

If an assistant message with N calls is retained, all required result messages for those N calls must be retained together or the whole group must be omitted/replaced by a recoverable summary according to policy.

Keep `validate_message_invariants()` as a backstop rather than using emergency fallback as the normal multi-tool path.

### 6.8 Goal checkpoint journal recency

Stop treating the first 4,000 characters of the append-only goal journal as the best current-state context.

Preferred behavior:

- typed `Goal` fields provide current objective/phase/progress/next action/open questions;
- if the Markdown journal is included, render a bounded **latest** excerpt/tail with UTF-8-safe slicing;
- preserve the plan excerpt/source separately from chronological updates;
- do not parse the whole Markdown journal to rediscover fields already in `Goal`.

Add `read_checkpoint_tail` or a structured helper rather than silently changing `read_checkpoint_excerpt` semantics if existing callers/tests rely on prefix behavior. Update `turn_runtime` to use the new current-state projection.

### 6.9 Prompt block ownership

Introduce/use a distinct prompt block kind/source for continuation state if needed. The continuation state required to resume after an installed compaction should not be an optional low-priority block that a future active packer can silently omit.

Goal context may remain separately useful before the first compaction, but once a continuation checkpoint is present the prompt compiler must not emit contradictory duplicate objective/progress projections.

Document exact precedence between:

- active goal context;
- installed continuation state;
- current turn user input.

## 7. Ordered work packages

### WP1 — Source-precedence contract and snapshot types

Finalize typed fields, ownership annotations, bounds, and merge rules. Add pure tests for precedence with/without active goals and previous checkpoints.

### WP2 — Host snapshot assembly

Wire GoalStore/todo/context-ledger/origin/current-user inputs into one assembler. Keep database reads outside the pure reducer where practical.

### WP3 — Compaction engine baseline integration

Pass the authoritative baseline into programmatic/hybrid compaction. Narrow semantic output ownership and prove host fields cannot be overwritten.

### WP4 — Frame replacement and multi-tool correctness

Version the frame marker, strip old CodeGG frames, emit one current frame, and resolve all multi-tool pair members.

### WP5 — Goal journal/current-state cleanup

Make the turn prompt use typed current goal state plus a bounded latest journal excerpt instead of the stale prefix. Add Unicode-boundary tests.

### WP6 — Documentation and diagnostics

Update architecture docs and add bounded diagnostics describing sources used, intent-spine truncation, prior checkpoint carried forward, and semantic-enrichment success/failure.

## 8. Failure, cancellation, restart, and contention semantics

Snapshot assembly should be deterministic and mostly synchronous over captured state.

- Goal-store lookup failure must not produce a fake goal. Fall back to session-origin provenance and record a diagnostic.
- Unreadable plan path keeps plan metadata/path but marks digest/content unavailable; it must not abort compaction.
- Semantic-provider cancellation/failure uses the host snapshot unchanged.
- Previous installed checkpoint decode/digest failure is surfaced and ignored; do not partially merge it.
- Current user steering always outranks older checkpoint semantic next steps.
- A concurrent goal/todo update during snapshot construction should be bounded by captured revisions/timestamps. M004 will decide whether a stale captured snapshot is still installable; M002 must expose enough provenance to detect staleness.

## 9. Compatibility and migration

No new SQLite migration should be required beyond M001 unless M001's payload schema proves insufficient.

Old `[codegg compacted session state]` messages remain readable and are recognized as superseded CodeGG-owned frames during transition.

Existing goal checkpoint files remain valid. M002 changes which excerpt is supplied to the model; it does not rewrite historical files.

Do not remove `ContextFrame` public/internal APIs mechanically. Either adapt them to render the new snapshot or retain a bounded compatibility conversion until callers migrate.

## 10. Required tests

Focused tests:

- active goal objective outranks session-origin prompt for current goal;
- session-origin prompt remains provenance;
- no-goal session uses origin prompt;
- latest user steering retained after prior checkpoint;
- intent spine truncates deterministically;
- previous decisions/constraints carry forward;
- new semantic fields merge without overwriting host facts;
- semantic provider failure leaves host state intact;
- one and only one continuation frame after repeated compaction;
- old marker is removed/superseded;
- unrelated system messages preserved;
- multi-tool assistant retention keeps all matching results;
- artifact-handle cap/dedup;
- plan digest stable and unreadable-plan diagnostic;
- goal journal latest-tail rendering;
- UTF-8-safe journal truncation.

Integration tests:

- `compact_with_policy` receives known objective/current task rather than `"unknown"`/`"none"`;
- hybrid and programmatic modes render equivalent authoritative host fields;
- a second compaction updates one frame rather than stacking another;
- current user steering after checkpoint changes next action/constraints as expected.

## 11. Verification commands

Expected narrow targets:

```text
cargo test -p codegg --test compaction
cargo test -p codegg-core -- goal
cargo test -p codegg -- context
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use exact package/test names available after implementation.

## 12. Documentation

Update:

- `architecture/compaction.md`
- `architecture/context-ledger.md`
- `architecture/goal.md`
- `architecture/cache-aware-context.md`
- `architecture/agent.md` if prompt-block/source precedence is documented there.

Correct any stale statement that production context artifacts are in-memory; `FileArtifactStore` is the current production store.

## 13. Acceptance criteria

M002 is complete only when:

1. one typed authoritative continuation snapshot is assembled from host-owned sources;
2. an active Goal/revision is represented explicitly and cannot be replaced by transcript inference;
3. session origin and later steering have separate provenance;
4. hybrid semantic checkpointing starts from the known objective/current task;
5. semantic output cannot overwrite deterministic goal/plan/file/command/test facts;
6. exactly one current CodeGG continuation frame survives each compaction;
7. repeated compactions do not accumulate old CodeGG frames;
8. multi-tool call/result grouping is correct without ordinary emergency fallback;
9. artifact handles are retained in bounded continuation state;
10. the active goal prompt no longer depends on the stale head of an append-only checkpoint journal;
11. focused tests and quick verification pass.

## 14. Stop conditions

Stop and create an ADR/corrective plan if:

- authoritative goal/plan state requires parsing arbitrary Markdown as the source of truth;
- the implementation needs to mutate `Goal.objective` from compaction output;
- the provider DTO must gain a public durable message-ID contract across all frontends;
- frame cleanup cannot distinguish CodeGG-owned context from user/system content safely;
- achieving continuity requires persisting hidden reasoning;
- M001's payload contract cannot represent required bounded host state without a storage redesign.

## 15. Closure evidence required

Record:

- final source precedence table;
- checkpoint payload/frame type changes;
- before/after repeated-frame examples;
- goal/current-task regression evidence;
- multi-tool grouping evidence;
- goal journal tail/current-state evidence;
- semantic failure fallback evidence;
- intent-spine bounds;
- docs updated;
- verification commands/outcomes.

## 16. Handoff notes

M003 should consume the bounded recovery-reference slots defined here rather than invent another semantic state model.

M004 should treat the M002 snapshot as an immutable candidate captured for one compaction attempt. It owns checking whether relevant goal/plan revisions changed before installation.
