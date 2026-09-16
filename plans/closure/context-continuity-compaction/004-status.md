# Context Continuity and Compaction M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/context-continuity-compaction/004-transactional-rollover-and-multi-compaction-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md#8-ordered-milestones`

Repository baseline reviewed: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Implementation commits or pull requests:

- `a06b7164` — context-continuity M004: transactional rollover and multi-compaction qualification

## 1. Executive finding

M004 is complete. The M001-M003 continuation contracts are now integrated
into the real compaction lifecycle with transactional ordering, and one
logical coding task stays coherent through repeated reductions and restart
boundaries. Replacement history is never installed before a verified
prepared checkpoint exists; install and the durable `ContextCompacted`
event commit atomically; latest installed state restores on later
turns/restart; prepared/aborted rows are never resume authority; stale
candidates cannot install; semantic/storage failures degrade explicitly
without false continuity; the production strategy matches the resolved
policy; recursive stacked summaries are eliminated; and the required
eight-compaction trajectory plus restart/cancellation/contention/security
matrices pass. No new memory/history service or live-provider CI was added.
This closes the context-continuity workstream (M001-M004).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed continuation candidate plumbing (§6.1) | `src/context/compaction.rs`: `ContextCompactionRequest.proposed_checkpoint_id`, `ContinuationCandidate{snapshot,evidence,frame,semantic_outcome}`, `ContextCompactionResult.continuation_candidate`, `CompactionOutput.continuation_candidate`; no SQL in pure policy | pass | Candidate carries final M002 merge + raw M003 index; handles attached only after verify |
| Transactional rollover ordering A-J (§6.2) | `src/context/rollover.rs::prepare_candidate` (C-F+D) + `install_prepared` (I) + `AgentLoop::compact_if_needed` (A-B-G-H-J) + `finish_prepared_install`; pre-allocated ID via `ContinuationCheckpointStore::prepare_with_id` | pass | H never precedes verification; abandoned candidates stay Prepared/Aborted |
| Source-revision revalidation (§6.3) | `RolloverSourceRevisions{goal_id,goal_revision,plan_digest,todo_revision,previous_installed_id}` + `is_stale_against`/`stale_reason` + one bounded rebuild in `compact_if_needed` | pass | Unrelated telemetry never rejects; one rebuild, no livelock |
| Post-install durable + in-process events (§6.4) | `install_with_compaction_event` (M001 atomic) + `AppEvent::CompactionTriggered` derived from installed result + `session.compacted` plugin hook | pass | Bus publish never substitutes for durable commit |
| Restart/turn-start recovery (§6.5) | `load_usable_installed_checkpoint` + `validate_installed_for_restart` + `render_installed_projection` + `inject_installed_continuation_for_turn` (called in `AgentLoop::run_inner` before provider loop); newer goal merges with M002 precedence | pass | Prepared/Aborted ignored; corrupt falls back with diagnostic; missing evidence degrades per-ref |
| Hard-capacity fallback (§6.6) | `rollover::is_hard_capacity` + `degraded_fallback` (emergency pair-safe + host frame, no install) + degraded `ContextCompacted` event with `checkpoint_id=None` + `continuity_degraded_reason` | pass | Ordinary failure defers unchanged; hard capacity degrades explicitly, never falsely installs |
| Strategy reconciliation (§6.7) | `resolve_effective_strategy` + `production_strategy_matrix`; `compact_context` unified to resolved policy when `auto=true` (Hybrid default, deterministic without provider/model); `auto=false` keeps DropMiddle compat; legacy `auto_compact_*` retained compat-only | pass | No silent billable call; compat helpers retained, docs classify them |
| No recursive summary dependence (§6.8) | `compile_frame_messages` strip-then-emit-one + `strip_prior_frames` + enriched snapshot from typed prior fields (never rendered text); `assert_single_frame` invariant | pass | Free-form legacy summaries never stack |
| Trajectory harness (§6.9) | `tests/context_continuity_m004.rs::m004_eight_compaction_trajectory` (8 epochs, steering, decisions, multi-tool, pass/fail tests, missing artifact, store recreation, ghost prepared, semantic failure) + `m004_stable_checkpoint_digests` | pass | Small limits, no live provider; per-epoch frame/goal/provenance/steering/decision/todo/file/test/evidence/pair/budget asserts |
| Observability (§6.10) | `RolloverDiagnostics{session,checkpoint,sequence,prev,tokens_before/after,bytes,intent_tokens,ref_count,semantic,continuity,reason}` + `bounded_line()` (IDs/digests/sizes only) | pass | Bodies/evidence never logged |

## 3. Production implementation evidence

Final transactional sequence (implemented in `context_runtime.rs` + `rollover.rs`):

```text
A. capture revisions (goal id/rev, plan digest, todo rev, parent, history digest)
   + load latest installed for lineage + assemble baseline with previous
   + allocate checkpoint UUID
B. compact_context(baseline + proposed ID) -> candidate + replacement
   [cancel check after candidate: no row yet]
C. select/persist/verify evidence (reuse ctx://tool, else bounded ctx://evidence)
D. validate replacement (pairs, one frame, user visible, send budget)
E. prepare_with_id as Prepared
   [cancel check after prepare: mark aborted, non-resumable]
F. get + verify digest/schema/parent
G. revalidate revisions; stale -> abort + one bounded rebuild or keep history
   [cancel check before replacement: abort, history unchanged]
H. replace in-memory/provider-visible messages
I. install_with_compaction_event atomically (+ ContextCompacted marker)
   [cancel check before install: abort; install failure after H -> degraded]
J. reset tracker / bounded diagnostics + CompactionTriggered + plugin hook
```

Exact production strategy/default matrix (`production_strategy_matrix()`):

```text
explicit mode=programmatic|agent|hybrid honored; auto=false -> DropMiddleMessages (compat); auto=true + mode omitted -> resolved default Hybrid (deterministic programmatic frame when no provider/model, hybrid enrichment when model-backed auto configured); legacy auto_compact helpers retained for compat/tests only
```

M001-M003 accepted closure references:

- M001 `fde6c2e3` (`plans/closure/context-continuity-compaction/001-status.md`)
- M002 `a96ed0fc` (`plans/closure/context-continuity-compaction/002-status.md`)
- M003 `3ea77e9f` (`plans/closure/context-continuity-compaction/003-status.md`)

No ADR was required: durable authority stayed in `codegg-core`, no public
cross-service contract was introduced, and no provider-specific semantics
were added. Default reconciliation uses the deterministic programmatic
frame when no provider/model is configured, so no silent cost change.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --test compaction --locked -- --test-threads=1
cargo test -p codegg-core --lib session::continuation --locked
cargo test -p codegg-core --test continuation_checkpoint --locked -- --test-threads=1
cargo test --test context_continuity_m004 --locked -- --test-threads=1
cargo test -p codegg --lib -- agent
cargo test -p codegg --lib -- context::compaction context::rollover context::continuation context::evidence
cargo fmt --all -- --check
cargo clippy -p codegg --all-targets --locked -- -D warnings
cargo clippy -p codegg-core --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

The plan's suggested workspace `--all-features` clippy sweep was deliberately
not used per repo no-`--all-features` policy; the narrowest affected crates
plus `verify.sh quick`'s workspace check is the justified substitute (same
convention as M001/M002/M003 closures).

### Results

- `cargo test -p codegg --test compaction`: 65/65 pass.
- `cargo test -p codegg-core --lib session::continuation`: 13/13 pass.
- `cargo test -p codegg-core --test continuation_checkpoint`: 11/11 pass.
- `cargo test --test context_continuity_m004`: 15/15 pass (10 M004:
  eight-compaction trajectory, stable digests, transaction ordering,
  stale-parent/contention, storage-vs-hard-capacity, cancellation matrix,
  restart matrix, strategy matrix, repeated invariants, security; plus 5
  shared `common::` harness tests).
- `cargo test -p codegg --lib -- agent`: 342/342 pass.
- `cargo test -p codegg --lib -- context::compaction context::rollover context::continuation context::evidence`: 64/64 pass (incl. 2 new rollover unit tests).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg --all-targets --locked -- -D warnings`: pass
  (one `unnecessary_unwrap` + two test `useless_vec` findings fixed).
- `cargo clippy -p codegg-core --all-targets --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority, workspace
  check).

Eight-compaction trajectory command/output:

```text
cargo test --test context_continuity_m004 m004_eight --locked -- --test-threads=1
test m004_eight_compaction_trajectory ... ok
```

Crash/restart matrix results (`m004_restart_matrix`): no checkpoint absent;
installed usable + projection renders objective/task; prepared-only ignored;
installed+newer prepared still returns installed; corrupt digest fails
closed; unsupported schema fails closed; session mismatch fails closed;
missing optional evidence still renders; newer goal merges with precedence.
All pass.

Cancellation injection results (`m004_cancellation_matrix`): token-cancelled
`compact_context` returns `Cancelled` without mutation; prepared-then-
cancelled aborts to `Aborted` with no install; pre-replacement cancel keeps
history; pre-install cancel aborts with no false install. All pass.

Stale-parent/contention evidence (`m004_stale_parent_and_contention`):
stale prepare fails closed; same-parent contenders cannot both install;
revision helper flags parent advancement. Pass.

Hard-capacity degraded fallback evidence
(`m004_storage_failure_vs_hard_capacity`): ordinary prepare failure leaves
history unchanged; hard-capacity `degraded_fallback` keeps the turn operable
with host frame in memory and an explicit no-install reason. Pass.

Provider semantic-failure fallback evidence: epoch 5 of the trajectory uses
a failing provider with an explicit hybrid model config; candidate outcome
is `fallback`, host objective/task preserved, epoch still installs. Pass.

No-stacked-frame assertion: `assert_single_frame` + `count_continuation_frames
== 1` after every installed epoch and in repeated-compaction invariants.
Pass.

Tool-pair invariant assertion: `validate_message_invariants` after every
installed epoch, in repeated invariants, and for emergency fallback. Multi-
tool 3-call groups retain all results atomically. Pass.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| No replacement before verified prepared checkpoint | `prepare_candidate` verifies before caller replaces; `*messages` assigned only after `get`+digest+parent checks |
| Only `Installed` is resume authority | `latest_installed` filters; `validate_installed_for_restart` rejects non-installed; ghost-prepared test |
| One current continuation frame | Strip-then-emit-one in engine + `strip_prior_frames` + per-epoch `== 1` asserts |
| Active current input visible | `validate_replacement_messages(expect_user_visible)` + re-append fallback + trajectory steering asserts |
| Goal/Todo/plan revisions authoritative | Capture/revalidate + newer-goal merge at turn start; host facts never overwritten by semantic merge |
| Tool-call/result invariants | Validator + all-ID multi-tool retention + per-epoch asserts |
| Provider session/request propagation | `ProviderRequestContext` threaded through `compact_context` unchanged |
| Cancellation | Five-point checks: before semantic (no row), after candidate (no row), after prepare (abort), before replacement (abort, unchanged), before install (abort; post-replacement install failure degrades) |
| Hard-capacity availability | `is_hard_capacity` gate; degraded path tested |
| Permissions/tool/workspace/Git/assets unchanged | No authority change; boundary guards pass |
| No model-specific private API | No new provider endpoint; deterministic tests only |
| Short sessions pay minimal cost | No pool → in-memory only, no checkpoint row; no checkpoint when `Ready` |
| Compaction state not frontend-owned | No `CoreEvent`/ACP/projection change; checkpoint bodies never in events |

## 6. Failure and recovery review

- Persistence/verify failure at ordinary threshold: history unchanged,
  candidate aborted when a row exists, bounded warning, retry later.
- Hard capacity: emergency pair-safe + host frame in memory,
  `continuity_degraded_reason` set, durable degraded `ContextCompacted`
  event with `checkpoint_id=None`, in-process warning; never installs.
- Semantic failure: host-only candidate (`fallback`/`disabled`), install
  proceeds with deterministic state.
- Stale source/contention: parent/revision check fails closed; one bounded
  rebuild from fresh state; no livelock.
- Restart: file-backed reopen proves installed recovery and prepared
  ignored (trajectory epoch 3 + M001 restart tests).
- Corrupt installed: diagnostic + fallback to goal/todo/session state,
  never partial parse or session failure unless no safe context exists.

## 7. Migration and compatibility review

- No new SQLite migration; `prepare_with_id` is additive API on the v57
  `continuation_checkpoint` table. Layout version unchanged (57).
- Existing sessions without continuation rows work unchanged; no backfill.
- Existing explicit compaction modes continue to parse; omitted-mode
  `auto=true` now uses the resolved Hybrid engine (deterministic without a
  model). Before/after matrix documented in `architecture/compaction.md`
  and `production_strategy_matrix()`; no silent billable call (model
  enrichment requires an explicit model-backed configuration).
- Legacy `auto_compact_*`/`compact_messages_*` helpers retained for
  compatibility/tests, classified as compat-only in code and docs.
- Old `[codegg compacted session state]` history remains consumable (recognized
  as superseded and cleaned to the single versioned frame on next rollover).
- No frontend/ACP/projection protocol change; handles are model/tool-internal.

## 8. Security review

- Checkpoint/event/log bodies never include payload or evidence content;
  `diagnostic_summary` + `bounded_line` carry IDs/digests/sizes only (tested).
- `ToolCall` args never persisted (selection skips, persist refuses).
- Reasoning excluded (`Text`-only extraction; fixture test).
- Secret assignment/token + Git URL redaction before hashing/storage.
- Same-session enforcement at parse/persist/verify/read; cross-session
  substring attacks rejected (tested for both handle forms).
- No directory enumeration or path disclosure; store paths never exposed.
- Continuation records remain excluded from exports pending an explicit
  contract (roadmap §10 rule preserved).
- No new network/auth/permission surface.

## 9. Documentation and operations

Updated:

- `architecture/compaction.md` — epoch/checkpoint model, A-J sequence,
  strategy matrix, degraded/recovery behavior, trajectory test target.
- `architecture/context-compaction-ownership.md` — rollover ownership
  (`rollover.rs` vs `AgentLoop` sequencing) and resolved-default rule.
- `architecture/context-ledger.md` — checkpoint evidence handle bounds and
  verify/degrade contract.
- `architecture/goal.md` — Goal/Todo revision authority through rollover
  and newer-goal merge.
- `architecture/session.md` — `prepare_with_id` additive API note.
- `architecture/agent.md` — turn-start injection + A-J sequencing note.
- `architecture/config.md` — omitted-mode resolved-default note.
- `src/context/rollover.rs` + `src/context/compaction.rs` module docs —
  ownership, bounds, and M004 call order.

Operator notes: watch `rollover(session=..., checkpoint=..., seq=...,
tokens=...->..., bytes=..., refs=..., semantic=success|fallback|disabled,
continuity=installed|degraded|deferred|prepared, reason=...)` lines;
`evidence_persist/evidence_verify` diagnostics; `plan_unavailable`,
`intent_spine_truncated`, stale-rebuild warnings, and degraded-continuity
warnings. Checkpoint bodies and evidence contents are never logged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Orphan candidate evidence files may accumulate when installation later fails or candidates are abandoned (inherited M003 note) | Inert same-session files under `.codegg/context_artifacts`; bounded by per-checkpoint caps | If accumulation is measured, write a bounded retention follow-up; do not widen M004 |
| Low | Turn-start injection loads the checkpoint per turn (one indexed `latest_installed` query) | One extra indexed read per turn; negligible vs provider call | No action; do not add a cache without measured need |
| — | No other open items | — | — |

No stop condition triggered (no daemon/session ownership change needed,
restart unambiguous via installed checkpoint + current turn state, no hidden
reasoning persisted, deterministic continuity path viable for non-model
configs, M001 install composes with event storage, M002/M003 closures left
no medium-or-higher defects).

## 11. Roadmap disposition

Workstream closed; dependencies update as follows:

- M004 (transactional rollover and multi-compaction qualification): hard
  dependencies M002 and M003 accepted closure satisfied — close.
- No active milestone depends on M004. The context-continuity subsystem
  (M001-M004) is complete; no M005 is registered. Future semantic history
  search, distributed replication, or user-facing epoch management require
  separate product justification per the roadmap and are not unblocked by
  this closure.
- Dependency-security M005 remains independently blocked on the generalized
  external updater interface (unchanged).
- Architecture convergence M009 and Runtime Safety C002 remain
  conditionally closed on their outstanding operational evidence (unchanged).

## 12. Registry updates

- `plans/registry.md`: M004 `ready` → `closed` with closure link and
  implementation commit `a06b7164`; subsystem row `active` → `closed`,
  current milestone M001+M002+M003 closed, M004 ready → M001-M004 closed;
  execution-order item 1 rewritten to reflect workstream closure;
  closure-work control row updated; M004 appended to recently-closed work.
- `plans/subsystems/context-continuity-compaction-roadmap.md`: status
  line `active; M001-M003 closed, M004 ready` → `closed; M001-M004 closed`;
  M004 section (`ready` → `closed` with closure link).
- `plans/implementation/context-continuity-compaction/004-*.md`:
  `Status: ready` → `Status: implemented`.
