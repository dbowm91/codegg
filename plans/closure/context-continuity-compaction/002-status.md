# Context Continuity and Compaction M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/context-continuity-compaction/002-authoritative-intent-plan-and-frame-projection.md`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md#8-ordered-milestones`

Repository baseline reviewed: `e4fa1458`

Implementation commits or pull requests:

- `a96ed0fc` — context-continuity M002: authoritative intent, plan, and frame projection

## 1. Executive finding

M002 is complete. Continuation state fed into compaction is now
authoritative, bounded, and non-recursive: one typed host-side snapshot
assembled from Goal/todo/ledger/origin/plan/previous-checkpoint state,
exact bounded user-intent spine, versioned single-frame projection,
baseline-aware hybrid enrichment with narrow semantic ownership,
all-tool-call multi-tool retention, bounded artifact-handle projection,
and typed-goal-first journal rendering with a UTF-8-safe tail. No
durable rollover sequencing was activated (M004 owns install/restart
production behavior); M002 prepares/renders candidates in memory.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Authoritative snapshot assembler (§6.1) | `src/context/continuation.rs`: `ContinuationAssemblyInput` → `assemble_continuation_snapshot` → `ContinuationSnapshot::{to_payload, to_context_frame, render_frame_text}` + 10 unit tests | pass | Typed inputs, no `AgentLoop` global reach-through; storage reads stay in `agent::context_runtime` adapter; result converts to M001 `ContinuationCheckpointPayload` |
| Field ownership/precedence (§6.2) | Objective (goal→origin, never paraphrase), current task (goal next/phase → in-progress todo → previous continuation → none), plan metadata (path+SHA-256+phase/action+`plan_unavailable`), deterministic evidence from host, semantic as advisory merge | pass | `assemble_continuation_snapshot` + `active_goal_outranks_origin_for_objective`, `no_goal_session_uses_origin_prompt`, `current_task_prefers_goal_next_action_then_todo` tests |
| Exact user intent spine (§6.3) | Origin + steering since previous checkpoint + current triggering message; 20k-token budget via CodeGG estimation; origin/current boundary retention; digests; truncation diagnostics; checkpoint-owned IDs; assistant text never intent | pass | `intent_spine_retains_steering_and_marks_truncation`, `intent_spine_deterministic_for_equivalent_input`, `previous_decisions_carry_forward_and_merge` (steering-after-checkpoint) |
| Frame accumulation eliminated (§6.4) | Versioned `[codegg continuation state v1]` render; explicit recognizer (`is_continuation_frame`/`is_legacy_compaction_frame`/`is_codegg_owned_frame`, start-anchored); strip-then-emit-one in `compile_frame_messages`; legacy-path normalization in `compact_if_needed` | pass | `m002_frame_replacement_keeps_one_current_frame`, `m002_repeated_compaction_does_not_stack_frames` (5×), `m002_legacy_marker_recognized_as_owned_frame`; unrelated system preserved |
| Hybrid goal/task loss fixed (§6.5) | `build_programmatic_state_with_baseline`, `semantic_checkpoint_with_baseline` (baseline objective/task in prompt, never `"unknown"`/`"none"` when known); `parse_semantic_response` reads 4 semantic fields only; `merge_frames` never touches host facts; `CompactionInput.baseline` + `ContextCompactionRequest.baseline` plumbing | pass | `m002_baseline_sets_objective_and_task`, `m002_hybrid_receives_known_objective_not_unknown`, `m002_compact_with_policy_reports_authoritative_fields` (programmatic+hybrid equivalent), `semantic_enrichment_never_overwrites_host_facts` |
| Bounded artifact handles (§6.6) | `bounded_artifact_handles` (most-recent 32, dedup, recency order); `ContextLedgerState::to_context_frame` + `build_context_frame` + snapshot carry it | pass | `artifact_handles_bounded_dedup_recency`; frame renders `Evidence handles:` line |
| Multi-tool retention (§6.7) | All-tool-call resolution in `select_retained_messages` + group atomicity; validator stays backstop | pass | `m002_multi_tool_retention_keeps_all_results` (3-call group, invariants hold without emergency fallback) |
| Journal recency (§6.8) | `read_checkpoint_tail`/`checkpoint_tail_of` (UTF-8-safe char slicing, prefix API untouched); `render_goal_context_with_tail` (typed fields authority + bounded latest tail); `render_goal_context` truncation fixed to char-boundary; `turn_runtime` uses tail | pass | `test_read_checkpoint_tail_returns_latest_updates`, `test_read_checkpoint_tail_utf8_safe`, `test_render_goal_context_with_tail_prefers_typed_state`, `test_render_goal_context_utf8_safe` |
| Prompt block ownership (§6.9) | `PromptBlockKind::ContinuationState` (`required`, `SlowChanging`, order 3) + precedence docs in `prompt.rs`/`agent.md`/`cache-aware-context.md`; goal block remains pre-first-compaction; no contradictory duplicate once installed | pass | Compiler mapping + docs; M004 owns turn-start injection (no premature durable wiring) |
| Failure/cancellation/restart/contention (§8) | Goal lookup failure → origin fallback + diagnostic; unreadable plan → metadata + `plan_unavailable`, never abort; semantic failure → host-only; previous-decode failure → ignore with diagnostic path; steering outranks old next steps; captured revisions exposed via goal id/revision + previous checkpoint lineage for M004 staleness checks | pass | `m002_semantic_failure_leaves_host_state_intact`; `plan_digest_stable_and_unreadable_diagnostic`; no new persistence (M004 owns install) |
| No new migration (§9) | No SQLite migration; `ContextFrame.artifact_handles` additive `#[serde(default)]`; old frames readable as superseded; goal journal files untouched; `build_programmatic_state`/`semantic_checkpoint` retained as compat wrappers | pass | Old JSON decodes; `storage_migrations` suite unaffected (covered by quick verify workspace check) |

## 3. Production implementation evidence

Ownership preserved: `src/context/continuation.rs` is the pure
assembler; `src/context/compaction.rs` remains the single production
reduction owner; `codegg-core` goal/session stores untouched except
additive journal/render helpers; `AgentLoop` sequences
(assemble→compact→normalize) and never persists checkpoints (M004).

Final source precedence table:

```text
objective:        active Goal.objective+id/revision > origin prompt > (empty, never paraphrase)
current task:     goal next_action > goal current_phase > in-progress todo > previous continuation next > none
plan:             path + SHA-256 (workspace-contained read) + phase/action; else plan_unavailable
evidence:         ledger files/commands/tests/errors + bounded artifact handles + security findings
semantic:         previous installed semantic + current-epoch constraints, enriched advisory-only
intent:           origin + steering since previous + current trigger, 20k-token budget, digests
```

Checkpoint payload/frame type changes:

- New `ContinuationSnapshot` (+ intent/plan/goal/todo/semantic/previous/diagnostics types) with `to_payload_body`/`to_payload` (M001 envelope) and `to_context_frame`/`render_frame_text` (≤16 KiB, UTF-8-safe).
- `ContextFrame` gains additive `artifact_handles`; `to_continuation_text` renders `[codegg continuation state v1]`; `to_compaction_control_text` is a compat alias emitting the same versioned form.
- `CompactionInput.baseline` and `ContextCompactionRequest.baseline` thread the snapshot; `build_programmatic_state`/`semantic_checkpoint` remain compat wrappers.

Before/after repeated-frame example:

```text
before (legacy, 3 compactions):
  System "system prompt"
  System "[codegg compacted session state] - Goal: stale one"
  System "[codegg compacted session state] - Goal: stale two"
  System "[codegg compacted session state] - Goal: stale three"

after (M002, 5 forced compactions):
  System "system prompt"
  System "[codegg continuation state v1] - Goal: authoritative objective ..."
```

Goal/current-task regression evidence: `m002_baseline_sets_objective_and_task`,
`m002_hybrid_receives_known_objective_not_unknown` (prompt contains known
objective/task, no `User Goal: unknown`), `m002_compact_with_policy_reports_authoritative_fields`
(both modes report `authoritative goal` / `authoritative current`).

Multi-tool grouping evidence: 3-call assistant group retains
`call_1`/`call_2`/`call_3` results together; compiled history passes
`validate_message_invariants` without emergency fallback.

Goal journal tail evidence: append Phase 1→2→3; head(200) shows
`Goal Checkpoint` prefix while tail(200) shows `Phase 3 newest`;
turn projection renders `Latest journal:` from the tail.

Semantic failure fallback evidence: `M002FailingProvider` hybrid run
returns the host frame (`host objective intact` / `host task intact`).

Intent-spine bounds: 20k estimated tokens total, 8k chars per entry,
digests per entry, `intent_truncated` flag + `intent_spine_truncated`
diagnostic; determinism test proves equal input → equal spine + digest.

Docs updated: `architecture/compaction.md` (M002 section),
`architecture/context-ledger.md` (FileArtifactStore production
correction + bounded projection), `architecture/goal.md`
(tail/current-state), `architecture/agent.md` + `architecture/cache-aware-context.md`
(continuation block precedence), `architecture/context-compaction-ownership.md`
(assembler inventory).

No ADR required: durable authority stayed in `codegg-core`, no public
cross-service contract introduced, no provider-specific semantics added.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy -p codegg --all-targets --locked -- -D warnings
cargo clippy -p codegg-core --all-targets --locked -- -D warnings
cargo test -p codegg --lib -- context::continuation context::compaction
cargo test -p codegg --lib -- m002
cargo test -p codegg --lib -- context
cargo test -p codegg-core --lib -- goal::
cargo test --test compaction --locked -- --test-threads=1
scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg --all-targets --locked -- -D warnings`: pass (two M002 lints fixed: `obfuscated_if_else`, `then_some`).
- `cargo clippy -p codegg-core --all-targets --locked -- -D warnings`: pass.
- `cargo test -p codegg --lib -- context::continuation context::compaction`: 51/51 pass (10 continuation incl. carry-forward/determinism; 8 M002 compaction incl. baseline/multi-tool/frame/hybrid/failure).
- `cargo test -p codegg --lib -- m002`: 10/10 pass (8 M002 + 2 pre-existing convergence M002 namespaced tests).
- `cargo test -p codegg --lib -- context`: 492/492 pass.
- `cargo test -p codegg-core --lib -- goal::`: 40/40 pass (incl. 2 new tail + 2 new render tests).
- `cargo test --test compaction --locked -- --test-threads=1`: 65/65 pass (production behavior preserved; one legacy marker assertion updated to the versioned recognizer).
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Focused §10 coverage: active-goal outranks origin; origin provenance;
no-goal origin; steering retained after checkpoint; deterministic
truncation; decisions/constraints carry-forward; semantic merge without
host overwrite; provider-failure host intact; one frame after repeated
compaction; old marker superseded; unrelated system preserved;
multi-tool all-results; artifact cap/dedup; plan digest + unreadable
diagnostic; journal tail + UTF-8 safety; `compact_with_policy`
authoritative fields in both modes; second-compaction single frame;
steering-after-checkpoint next action/constraints.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Origin prompt immutable provenance | `origin_text`/`origin_digest` always retained; origin entry first in spine |
| Active Goal+revision authoritative when present | Assembler + `build_context_frame` goal lookup; revision in snapshot/payload/provenance footer |
| Later steering as ordered entries, never origin mutation | Intent spine IDs/digests; steering-after-checkpoint test |
| Host todo/plan/file/test facts over model prose | Baseline-first reduction; narrow semantic output; merge never overwrites host |
| Semantic advisory only | 4-field parse + merge guards + failure fallback |
| Exactly one current CodeGG frame | Strip-then-emit-one + legacy-path normalization; 5× test |
| Exact tool-call/result invariants incl. multi-call | All-ID resolution + atomicity + validator backstop |
| Model-visible state bounded | Intent 20k + per-entry caps + evidence caps + 16 KiB frame + 32 KiB prompt block |
| Goal journal as history, not authority | Typed fields first; tail-only journal inclusion |
| Existing goal tools/`GoalStore` unchanged | No store API touched; additive helpers only |
| No provider/model dependence for assembly | Assembler sync/pure; provider only in enrichment with fallback |

## 6. Failure and recovery review

- Goal-store lookup failure → origin provenance + debug diagnostic, no fake goal.
- Unreadable plan → path kept + `plan_unavailable`, compaction continues.
- Semantic cancellation/failure → host snapshot unchanged (tested with failing provider).
- Previous-checkpoint decode failure → empty semantic default, no partial merge; lineage ignored safely.
- Steering outranks old next steps (enrichment merges behind host state).
- Concurrent goal/todo updates bounded by captured goal revision + previous lineage exposed for M004 install checks; M002 performs no installs.
- No new persistence: crash before/after candidate build leaves no durable row (M004 owns prepare/install boundaries).

## 7. Migration and compatibility review

- No SQLite migration; layout version unchanged.
- Old `[codegg compacted session state]` messages recognized as superseded and removed on next compile; unrelated system messages preserved.
- Goal checkpoint files unmodified; only the read projection changed (head→tail for turn context).
- `ContextFrame` additive field with `#[serde(default)]`; old serialized frames decode.
- `build_programmatic_state`, `semantic_checkpoint`, `to_compaction_control_text`, `read_checkpoint_excerpt`, `render_goal_context` retained as compat paths.
- `CompactionInput`/`ContextCompactionRequest` additive `baseline: Option<…>`; existing callers pass `None` (8 integration literals + 2 canonical tests updated).
- `PromptBlockKind::ContinuationState` additive; existing kinds/orders unchanged.

## 8. Security review

- No hidden-reasoning persistence: intent spine takes `User` text only; assembler never reads provider-private reasoning; payload keys avoid the M001 denylist (values only carry user/host text).
- No secret-bearing tool arguments copied: evidence comes from ledger projections, not raw tool-arg JSON; semantic prompt carries reduced ledger + recent user/assistant text only.
- Plan reads workspace-contained only (absolute paths must strip-prefix to workspace; relative joined under workspace; missing/unreadable → diagnostic, never abort/escape).
- Same-session scoping unchanged; handles are bounded strings, no new lookup surface (M003 owns evidence handles).
- Checkpoint bodies never logged; diagnostics carry IDs/digests/counts/sizes only.
- No new network/auth/permission/export surface.

## 9. Documentation and operations

Updated (see §3 list): `architecture/compaction.md`,
`architecture/context-ledger.md`, `architecture/goal.md`,
`architecture/agent.md`, `architecture/cache-aware-context.md`,
`architecture/context-compaction-ownership.md`.

Operator notes: intent budget 20k tokens, per-entry 8k chars, evidence
lists 32×1k chars, artifact handles 32, frame 16 KiB, prompt blocks
32 KiB. Diagnostics to watch: `intent_spine_truncated`,
`plan_unavailable`, `previous_checkpoint(carried_forward)`,
`semantic_enrichment(success|…)`, `baseline(objective_source=…, task_source=…)`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No stop condition triggered (no Markdown-as-truth parsing, no
`Goal.objective` mutation from compaction, no durable message-ID
contract, frame cleanup distinguishes CodeGG-owned content by anchored
marker, no hidden reasoning persisted, M001 payload contract sufficient).

## 11. Roadmap disposition

Milestone closed; dependencies update as follows:

- M002 (authoritative intent, plan, and frame projection): hard
  dependency on M001 satisfied — close.
- M003 (bounded exact context recovery references): already `ready`
  (parallel with M002); unchanged, still unblocked by M001.
- M004 remains `blocked` on M002 **and** M003 accepted closure. M002
  closure alone does not unblock M004; M003 must still close.

## 12. Registry updates

- `plans/registry.md`: M002 `ready` → `closed` with closure link and
  implementation commit `a96ed0fc`; subsystem row current milestone
  M001 closed, M002 closed, M003 ready; execution-order item 1 updated;
  closure-work control row updated; M002 appended to recently-closed
  work.
- `plans/subsystems/context-continuity-compaction-roadmap.md`: status
  line, M002 section (`ready` → `closed` with closure link), M004
  section remains blocked (M003 still open).
- `plans/implementation/context-continuity-compaction/002-*.md`:
  `Status: ready` → `Status: implemented`.
- `plans/implementation/context-continuity-compaction/003-*.md`:
  unchanged (`ready`).
- `plans/implementation/context-continuity-compaction/004-*.md`:
  unchanged (`blocked on M002 and M003 accepted closure`).
