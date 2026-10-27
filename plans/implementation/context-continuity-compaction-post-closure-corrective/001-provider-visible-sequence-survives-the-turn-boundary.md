# Context Continuity Post-Closure Corrective — Milestone C001 — The Provider-Visible Sequence Survives the Turn Boundary

Status: implemented

Repository baseline: `957fcbf00e17bf108429f0d75dcbf760736ae511` — `origin/main`
(the reviewing worktree `feat/auto-20261026-7c94ea67` was already at this
commit; its rebase was a no-op)

Source roadmap:

- `plans/subsystems/context-continuity-compaction-post-closure-corrective-addendum.md`

Planning basis:

- **ADR-0054 (accepted 2026-10-26)** —
  `plans/adrs/ADR-0054-daemon-owned-provider-visible-history-across-turn-boundaries.md`
  — the daemon owns the provider-visible sequence across a turn boundary; a
  frontend-supplied prefix the daemon has already compacted is not
  authoritative and may be dropped; the comparison is exact and fail-closed.
- **The closed predecessor line** —
  `plans/subsystems/context-continuity-compaction-roadmap.md` and
  `plans/closure/context-continuity-compaction/001-status.md` …
  `004-status.md`. M004 owns the durable contracts this milestone reuses: the
  continuation checkpoint store, the transactional ordering, the one-frame
  invariant, and §6.5's turn-start injection rule. **M001–M004 are not
  reopened**; their closure records are inputs here, and this plan states
  precisely which of their delivered behaviours this milestone corrects.
- **The ownership contract** — `architecture/context-compaction-ownership.md`,
  "durable session history is never deleted; only the in-memory/provider-visible
  sequence is replaced after verification."

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable architecture:

- `architecture/context-compaction-ownership.md`
- `architecture/compaction.md`
- `architecture/context-ledger.md`
- `architecture/session.md`

Primary class: **invariant** (a compaction that does not survive the turn
boundary is a false claim about the product's own behaviour), with a
**capability** consequence (a long trajectory stops failing on capacity).

---

## 1. Objective

One bounded outcome: **after a turn that compacts, the next turn's
provider-visible sequence is the daemon's compacted sequence — not the
frontend's pre-compaction body re-sent beside it.**

Concretely, the milestone must make three things true:

1. **The superseded prefix is dropped.** The daemon recognises the prefix of
   the frontend-supplied message list that it has already compacted and
   replaced, drops it, and continues from its own compacted sequence plus the
   checkpoint's bounded projection.
2. **The projection is placed before the current user turn**, on every model
   profile — restoring M004 §6.5.
3. **The "current user message" read is immune to daemon-injected rows**, so
   that neither a control instruction nor a peer observation can be mistaken
   for the human's turn.

## 2. The defect, in the code

### 2.1 The frontend re-sends everything, and the daemon has no durable copy

| Step | Site |
|---|---|
| the TUI renders its whole transcript into provider messages | `src/tui/app/mod.rs:3382` `App::build_provider_context` |
| the TUI attaches it to the turn submit | `src/tui/app/mod.rs:3113` `dispatch_turn_submit_request` → `:3138` |
| the daemon takes the list as given | `src/core/daemon_turns.rs:312` (TurnSubmit arm) → `:985` `messages_dto` |
| the agent's request is built from it | `src/agent/turn_runtime.rs:1074` `ChatRequest { messages: dtos_to_provider_messages(messages_dto), .. }` |

The daemon does **not** re-read durable session history for the agent's
request. `MessageStore::list` is reached only from
`src/core/daemon_sessions.rs:96` and `:274` (the frontend's session-message
load), `src/core/daemon_turns.rs:1830` (`SnapshotSession`) and
`src/core/daemon_goals.rs:643` (goal history) — none of which feeds the
provider request.

`build_provider_context` emits only `Message::User` / `Message::Assistant` /
`Message::Tool`. It never emits a `Message::System`, so the daemon's system
prompt and every daemon-injected frame are absent from the frontend's list by
construction.

### 2.2 The compaction replaces only the daemon's copy

| Step | Site |
|---|---|
| the loop decides to compact | `src/agent/loop.rs:1457` (pre-provider) and `:2269` (post-tool-results) |
| the compaction owner replaces the sequence | `src/agent/context_runtime.rs:827` `compact_if_needed` → `src/context/compaction.rs:950` `compact_context` |
| the replacement is installed in memory only | `src/agent/context_runtime.rs:1640` `finish_prepared_install` (Prepared → Installed + durable `ContextCompacted` marker) |
| the turn ends; the replaced list is discarded | the daemon's per-turn `request.messages` is a local of `AgentLoop::run_inner` (`src/agent/loop.rs:1142`) |
| the frontend's transcript is untouched | the only TUI reaction to a compaction is a toast (`src/tui/runtime/app_events.rs:525`) |

`architecture/context-compaction-ownership.md` states the intended behaviour:
"durable session history is never deleted; only the in-memory/provider-visible
sequence is replaced after verification." The replacement is replaced and then
re-asserted by the next turn's request.

### 2.3 The projection is re-injected on every later turn

`src/agent/context_runtime.rs:773` `inject_installed_continuation_for_turn`
(called from `src/agent/loop.rs:1330`):

```text
:778  load_usable_installed_checkpoint()          # the latest Installed row
:809  render_installed_projection(&checkpoint, goal_override)
:816  let (stripped, _) = rollover::strip_prior_frames(messages);
:817  *messages = stripped;
:818  push_control_instruction(messages, model_profile, &block);
```

`strip_prior_frames` (`src/context/rollover.rs:653` →
`src/context/compaction.rs:2478` `strip_codegg_owned_frames`) removes only
`Message::System` rows whose content is a CodeGG-owned frame
(`src/context/compaction.rs:2466`). The frontend's transcript contains none, so
the strip is a **no-op on the TUI path** and the projection is pushed again.

`render_installed_projection` (`src/context/rollover.rs:472`) renders the
checkpoint's goal, current task, constraints, **decisions**, commands, tests,
errors, next steps and recovery handles (`src/context/rollover.rs:507`). A
pending question or a permission verdict is a decision — so the agent's own
earlier questions and permissions come back, beside the raw copy of themselves.

### 2.4 The projection lands after the new user message

`crates/codegg-core/src/model_profile/policy.rs:29`
`push_control_instruction`:

| Profile | `should_avoid_late_system_messages` | Placement |
|---|---|---|
| **default** (`supports_late_system_messages: true`, `prefers_user_control_messages: false` — `crates/codegg-core/src/model_profile/resolve.rs:160`) | `false` (`policy.rs:25`) | **appended as `Message::System` at the tail** (`policy.rs:56`) — after the new user message |
| minimax / local (`supports_late_system_messages: false` — `resolve.rs:183`, `:199`) | `true` | merged into `messages[0]`, the daemon's system prompt (`policy.rs:39`) — before the new user message |
| any profile where `messages.first()` is not a System | `true` | appended as **`Message::User("Instruction: …")`** (`policy.rs:48`) — reachable only from the `exec` path (`src/exec.rs:219`, `system: None`) |

M004 §6.5 requires the projection "**before the current user turn**". The
default profile does not meet that.

### 2.5 The "current user message" reads

`src/agent/context_runtime.rs:11` `latest_user_prompt_from_messages` scans the
live `request.messages` in reverse for the last `Message::User`. It is read
**after** the injection (`src/agent/loop.rs:1330`) and after peer observations
(`:1353`), at compaction time:

| Read | Site | Uses it for |
|---|---|---|
| `current_user` | `src/agent/context_runtime.rs:964` | the checkpoint snapshot's `current_user_message` |
| `original_user` | `src/agent/context_runtime.rs:1018` | the "active current user input must remain visible" guarantee (`src/context/rollover.rs:760`, `:781`) |
| fresh-baseline retry | `src/agent/context_runtime.rs:1398` | the same, on the degraded path |
| fresh-epoch objective fallback | `src/agent/context_runtime.rs:1810` | the objective when there is no active goal |
| fresh-epoch current user | `src/agent/context_runtime.rs:1946` | the epoch's handoff message |
| fresh-epoch original user | `src/agent/context_runtime.rs:1972` | the visibility guarantee, epoch path |

`src/agent/follow_up.rs:331` (`inject_peer_observations`) pushes a
`Message::User` at the tail, so on the TUI path this read can already return a
peer observation rather than the human's turn. On the `exec` path the
`Message::User("Instruction: …")` branch makes it return a control instruction.

For contrast, the two prompt reads that are **not** exposed:
`AgentLoop::latest_user_prompt` (`src/agent/request_preparation.rs:890`) is
read at `src/agent/loop.rs:1150` and `:1289` — both **before** the injection —
and `latest_user_question` (`src/agent/turn_runtime.rs:1265`) operates on the
original frontend DTOs. The milestone must not regress these.

## 3. Why this milestone is ready

- **ADR-0054 is accepted** (2026-10-26). The ownership question is settled and
  is not this implementer's to decide.
- **The durable machinery exists.** The checkpoint store, its lifecycle, its
  payload, its digest, the transactional ordering and the projection renderer
  are all delivered and closed under M001–M004. This milestone adds two
  additive bounded fields and one turn-construction step; it does not build a
  compaction engine.
- **The interfaces are stable.** `ContinuationCheckpoint`'s shape
  (`crates/codegg-core/src/session/continuation.rs:319`) is closed and is
  extended additively with `#[serde(default)]`, so older rows stay readable.
- **No hard dependency is outstanding.** The frontend, the daemon and the
  protocol are all present at this baseline.

## 4. Invariants that must not regress

1. `src/context/compaction.rs` remains the single production owner of context
   reduction policy. This milestone adds no second owner.
2. `codegg-core` remains the durable owner of session-scoped continuation
   records; the new durable fields live on the existing checkpoint payload.
3. The checkpoint payload stays within
   `CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES = 128 * 1024`
   (`crates/codegg-core/src/session/continuation.rs:51`).
4. Durable session history is never deleted. The `MessageStore` transcript
   remains the uncompacted record of what happened.
5. The daemon never drops a prefix it cannot identify exactly. An
   unidentifiable prefix is **kept**.
6. Tool-call/result pairing, ordering and IDs remain valid, including
   assistant messages with multiple tool calls
   (`src/context/compaction.rs` `validate_message_invariants`).
7. Exactly **one** CodeGG-owned continuation frame is present in the
   provider-visible sequence (`src/context/compaction.rs` `count_continuation_frames`).
8. The durable session transcript's user row remains the human's prompt
   (`src/agent/turn_completion.rs:532` gates the whole write on
   `self.current_user_prompt`, which is captured at `src/agent/loop.rs:1150`).
9. No permission, tool authority, workspace, Git, job, budget or runtime-asset
   semantics change.
10. Hidden reasoning, credentials, secret-bearing tool arguments and
    unredacted sensitive output are never copied into continuation state.
11. The frontend's transcript is not truncated, rewritten or emptied.
12. The `TurnSubmit` contract and
    `crates/codegg-client`'s non-empty-messages requirement are unchanged.
13. Short sessions that never compact pay no extra persistence or model cost.
14. The compaction's own policy, thresholds and tracker are unchanged.

## 5. Scope

### In scope

- The durable identity of the history a checkpoint replaced, and a comparable
  projection over the **incoming** frontend-supplied list.
- The durable form of the replacement the daemon re-installs at the next turn
  (today the in-memory replacement does not survive the turn boundary).
- Turn construction: drop the superseded prefix, fail-closed on a mismatch,
  and emit a bounded diagnostic.
- The continuation projection's placement, per ADR-0054 rule 4 and M004 §6.5.
- The "current user message" reads listed in §2.5.
- The two-turn regression test that does not exist today, plus the in-crate
  test that can reach `inject_installed_continuation_for_turn`.
- Architecture and documentation updates.

### Out of scope — explicit non-goals

- **No change to the `TurnSubmit` protocol** or to any frontend's compose path.
- **No truncation, rewrite or emptying of the frontend's display transcript.**
- **No bounding of the wire payload.** The daemon still receives the
  superseded prefix before dropping it. Recorded as a known cost in ADR-0054.
- **No new durable store, vector index, or history service.**
- **No change to compaction policy, thresholds, the tracker, the semantic
  enrichment contract, or the compaction engine's internals.**
- **No reopening of the closed `context-continuity-compaction` M001–M004
  milestones** or their closure records.
- **No new CI lane, benchmark gate, live-provider test, or permanent
  verification framework.**
- **No change to the fresh-epoch policy's supported profiles**
  (`crates/codegg-core/src/work_plan/epoch_policy.rs:258`) unless §7's stop
  condition 2 forces it.
- **No change to `AppEvent::CompactionTriggered`'s payload or the TUI's toast.**

## 6. Required production changes

The implementer chooses the mechanism inside ADR-0054's boundary. The following
are the **obligations**, not a design.

### 6.1 A durable, comparable identity for the replaced history

ADR-0054 rule 5 requires the checkpoint to carry a comparable identity for the
history it replaced.

The existing candidates, and why each is insufficient alone:

- `rollover::source_history_digest` (`src/context/rollover.rs:252`) covers
  **only** `Message::System` content and `Message::User` text parts
  (`rollover.rs:256`–`:266`); it excludes Assistant and Tool messages, so it
  cannot detect duplication of assistant or tool material. It is captured at
  `src/agent/context_runtime.rs:673` over the **daemon's** message list (which
  includes the daemon's system prompt and injected frames), it is **never
  persisted** (`build_checkpoint_payload_body`,
  `src/context/rollover.rs:357`, writes no digest), and it is **never
  compared** — `is_stale_against` (`rollover.rs:156`) does not consider it, and
  the existing test records it as "diagnostic, not a reject reason"
  (`src/context/rollover.rs:935`).
- The checkpoint's `intent_spine` entries (`src/context/continuation.rs:189`,
  built at `:299`) already carry a SHA-256 digest of the exact user text per
  entry (`continuation.rs:281`), and they **are** durable
  (`src/context/continuation.rs:966` `to_payload_body`). But they cover
  **user-authored turns only** — not assistant text, not tool results — and
  they are **truncated**, dropping middle entries
  (`continuation.rs:376`, `intent_truncated` flag). Absence from the spine
  therefore does **not** prove absence from the transcript.

So the milestone must add a new identity that is:

- computed over a **stated common projection** of the message list — not over
  "the messages", because the daemon's set and the frontend's set genuinely
  differ (the daemon's has the system prompt, injected frames and peer
  observations; the frontend's has none of those and re-serialises tool output
  at `src/tui/app/mod.rs:3419`);
- **exact**, so that a mismatch means "not mine" and the daemon keeps the
  list;
- **durable**, on the existing checkpoint payload, additive and
  `#[serde(default)]`;
- **bounded**, and carrying no body-level content that could leak a secret
  (invariant 10).

The prefix identity is **not** the existing `source_history_digest`, and the
plan does not leave its safety mechanism to the implementer. Use a versioned,
canonical projection shared by the compaction-source capture and the
`TurnSubmit` ingress comparison:

1. Project only rows representable by the frontend transcript (User,
   Assistant, Tool); exclude daemon-only System rows, continuation/control
   frames, and peer observations. Keep the original frontend transcript
   boundary/order. Do not attempt to match a daemon-only row to a frontend row.
2. Preserve every identity-bearing field and boundary: role, ordered content
   parts and their kinds/text, assistant tool-call IDs/names/arguments, and
   tool-result call ID/content. Do not concatenate text or omit assistant/tool
   messages. The projection must match the exact normalized representation
   emitted by the shared client/TUI serializer; if a field cannot be compared
   losslessly, a match is unavailable and the daemon keeps the incoming list.
3. Canonically encode each projected message with explicit variant tags and
   length-delimited fields, then compute a domain-separated cryptographic hash
   (SHA-256). Persist the projection version, covered message count, and digest
   on the existing checkpoint as additive `#[serde(default)]` data. No message
   bodies are persisted in this identity. At ingress, project the first N
   incoming rows using the same version and compare the digest, where N is the
   checkpoint's covered count. The count identifies the exact prefix boundary;
   a digest without that count is insufficient.
4. Reject unsupported versions, absent identities, invalid counts, and any
   conversion/canonicalization error as non-matches. Never use a text-only,
   truncated, role-blind, or heuristic comparison to authorize a drop.

The covered count and digest must be captured from the exact source sequence
that the installed checkpoint replaces, using the common projection above;
they must not be inferred later from `intent_spine` or the lossy diagnostic
digest. A canonical compaction may retain a contiguous opening anchor in its
immediate provider request; the checkpoint frame still supersedes the source
prefix across the turn boundary when the retained suffix gives an exact cut
point. Only such a contiguous opening anchor is permitted; any later,
non-contiguous retained row makes the identity unavailable. Keep the serialized
checkpoint within its existing 128 KiB limit. Add unit tests proving that role
changes, content-part boundaries, reordered parts, tool-call names/arguments/IDs,
and tool-result changes alter identity, that an opening anchor plus exact suffix
is accepted, and that non-contiguous retention fails closed. If the TUI's
existing serialization cannot support this common projection without ambiguity,
stop and report rather than weakening the comparison or changing the public
`TurnSubmit` contract.

### 6.2 The replacement must survive the turn boundary

The in-memory replacement does not survive the turn boundary, so at turn N+1
the daemon has nothing to install in place of the dropped prefix. The
milestone must therefore give the daemon a **durable, bounded** form of what it
re-installs.

The bounded continuation state the checkpoint already carries
(`src/context/continuation.rs:966`–`:1039`: objective, current task,
`intent_spine`, `touched_files`, `commands`, `tests`, `errors`, `security`,
`artifact_handles`, `semantic{constraints, decisions, unresolved_blockers,
next_steps}`) plus the M003 recovery references are the designed carriers of
continuity. ADR-0053's companion mechanism — the fresh-context epoch, rebuilt
from "immutable prompt/runtime instructions, current Goal/WorkPlan/Todo
projections, installed continuation state, bounded recent user steering, and
recovery handles" (`plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`
§13; `src/context/epoch.rs:264` `build_fresh_epoch_messages`) — is the
**existing** mechanism for exactly this shape.

The implementer must decide, and record, which of these the daemon re-installs:

- **the durable bounded checkpoint state** (plus the retained tail it already
  keeps), or
- **a fresh-epoch rebuild** from authoritative host state, using the existing
  `build_fresh_epoch_messages` and `try_start_fresh_epoch`
  (`src/agent/context_runtime.rs:1752`), or
- **a hybrid** the milestone introduces.

A fresh-epoch rebuild is attractive because it is already implemented and
already has its own transactional ordering. But note the recorded constraint:
`epoch_supported_for_profile` (`crates/codegg-core/src/work_plan/epoch_policy.rs:258`)
opts in only `LongContextPlanner`, `FrontierReasoning` and `FrontierExecutor`,
and the policy is disabled by default. Widening either is a **behaviour change
for every profile** and is stop condition 2.

Whatever the choice, the daemon must be able to answer "what do I install in
place of the dropped prefix?" **without** the frontend's list and **without**
inventing continuity. If the honest answer is "the bounded checkpoint state
plus recovery handles", then the raw material it does not carry is reachable
only through `context_read` — which is the M003 design and must be stated
plainly in the closure, not glossed.

### 6.3 Turn construction: drop the superseded prefix

At turn construction — alongside the existing
`inject_installed_continuation_for_turn` (`src/agent/loop.rs:1330`) — the
daemon must:

1. Load the installed checkpoint (already done at
   `src/agent/context_runtime.rs:778`).
2. Compare the recorded source identity against the **incoming frontend-supplied
   list** using §6.1's stated projection.
3. On an exact match, drop **only** the N matched rows and compose §6.2's
   durable replacement with any unmatched incoming suffix after that boundary;
   do not discard or reorder that suffix.
4. On a mismatch, or when there is no usable checkpoint, **keep the list
   unchanged** and emit a bounded diagnostic naming the outcome. Never drop a
   prefix that cannot be identified. A matching digest with an invalid or
   out-of-range covered count is a mismatch.

The comparison must be over the frontend-supplied list **before** the daemon's
own rows are mixed in — i.e. over the list as it arrived, not over
`request.messages` after the system prompt, the injected projection and the
peer observations have been added. The daemon must therefore keep the
frontend's list identifiable at that point. The implementer chooses how
(`src/agent/turn_runtime.rs:1074` is the natural seam, where the DTO list is
still unconverted and unmodified).

### 6.4 The projection's placement

Per ADR-0054 rule 4 and M004 §6.5, the continuation projection must be
injected **before the current user turn** on **every** model profile.

Today the default profile appends a trailing `Message::System`
(`crates/codegg-core/src/model_profile/policy.rs:56`) and the
late-system-avoiding profile merges into `messages[0]` (`policy.rs:39`).
Merging into the leading system prompt satisfies "before"; appending at the
tail does not. The milestone must make the placement correct for every
profile, and must not regress the profiles that are correct today.

`push_control_instruction` is shared with the todo reminder
(`src/agent/loop.rs:1450`) and the max-steps control instruction
(`src/agent/loop.rs:1371`). The implementer must not change those callers'
semantics by accident; if the shared helper's placement must change, the
change must be safe for all of its callers or the projection must use a
separate path.

### 6.5 The "current user message" reads

`latest_user_prompt_from_messages` (`src/agent/context_runtime.rs:11`) must
identify the **human's** turn, not a daemon-injected row. The reads listed in
§2.5 must be immune to:

- a peer observation (`src/agent/follow_up.rs:331`);
- a control instruction (`crates/codegg-core/src/model_profile/policy.rs:48`);
- the continuation projection, in whichever role it is placed.

The obligation is that these reads return the human's prompt, or a **typed
absence** — never a daemon-injected row's text. "No user message" and "the
last user message is an injected row" must be distinguishable.

The two unexposed prompt reads (`src/agent/request_preparation.rs:890`,
`src/agent/turn_runtime.rs:1265`) must keep their current behaviour, and the
durable transcript's user row (`src/agent/turn_completion.rs:532`) must remain
the human's prompt.

## 7. Stop conditions

The implementer **stops and reports** rather than deciding any of these:

1. **The identity cannot be made exact.** If no stated projection of the
   incoming list can be compared exactly against the checkpoint's recorded
   source, the milestone stops. A heuristic comparison would risk dropping live
   material, which is worse than the duplication it removes.
2. **The only workable replacement is a fresh-epoch rebuild, and that requires
   widening `epoch_supported_for_profile` or the epoch policy's default.**
   That is a behaviour change for every model profile and belongs to the
   maintainer. Report the profile set, the policy default, and the observed
   effect; do not widen either.
3. **Carrying the replacement durably would exceed the checkpoint payload
   bound** (`continuation.rs:51`) or would require raising it. Report the
   measured size and the alternatives; do not raise the bound.
4. **The correction requires a new `CoreRequest` variant, a new `AppEvent`
   variant, or any change to the `TurnSubmit` shape.** ADR-0054 explicitly
   excludes these. Report the required variant and why; do not add it.
5. **The frontend's transcript would have to be rewritten or emptied.** Report
   why; the correction is daemon-side by decision.
6. **The durable session transcript's user row would change.** It must remain
   the human's prompt.
7. **The drop would have to remove tool results or permission records the
   checkpoint does not cover.** Report the affected material. Only history the
   checkpoint covers may be dropped.

## 8. Required tests

The central gap is that **no test spans the TUI→daemon boundary across a
compaction**. This milestone must close it.

### 8.1 The two-turn test (the regression that does not exist)

A test that drives **two turns on one session with a compaction between them**
and asserts:

1. the second turn's provider-visible sequence does **not** contain the first
   turn's pre-compaction body;
2. it contains **exactly one** continuation projection
   (`src/context/compaction.rs` `count_continuation_frames`);
3. the projection is positioned **before** the current user message;
4. the human's prompt is present and identifiable as the current user turn.

**Location:** `tests/agent_loop/context_continuity_m004.inc` has the store and
session fixtures (`seed_session` `:179`, `test_goal` `:101`, `test_todos` `:126`)
and the checkpoint-store setup used by `m004_restart_matrix` (`:998`); the
sibling file `tests/agent_loop/context_epoch_handoff.inc` has the
"restart reads the installed checkpoint" shape
(`restart_after_fresh_epoch_preserves_lineage` `:531`). The implementer picks
the better fit and records it.

**Constraint:** the test must supply a **frontend-shaped** message list —
User/Assistant/Tool only, **no `Message::System`** — because that is what
`build_provider_context` produces (`src/tui/app/mod.rs:3382`). A test that
supplies a daemon-shaped list would pass on the broken baseline and prove
nothing. This is precisely the mistake
`tests/agent_loop/context_continuity_m004.inc:443` makes.

### 8.2 The in-crate test (the only way to reach the injection)

`inject_installed_continuation_for_turn` is `pub(super)`
(`src/agent/context_runtime.rs:773`), so it is **not reachable from `tests/`**.
A test asserting its exact placement per profile branch must live in
`src/agent/context_runtime.rs`'s inline `mod tests` (from `:2084`), which
currently holds only context-packer and cache-stat tests.

### 8.3 The fail-closed test

An incoming prefix that does **not** match the recorded source is **kept**, and
a bounded diagnostic is emitted. The test must fail on a baseline where the
prefix is dropped unconditionally.

### 8.4 The prompt-identity test

`latest_user_prompt_from_messages` returns the human's prompt when the list
ends with a peer observation or a control instruction, and returns a typed
absence — not an injected row's text — when there is no human turn.

### 8.5 The bounds and secrecy test

The enlarged checkpoint payload stays within
`CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES`, and the new durable fields carry
no body-level content that could carry a secret or hidden reasoning.

### 8.6 The placement matrix test

`crates/codegg-core/src/model_profile/policy.rs`'s inline `mod tests` (from
`:61`) already holds four placement tests
(`test_push_control_avoids_late_system` `:66`,
`test_push_control_allows_late_system` `:83`,
`test_dedup_push_control_skips_duplicate` `:100`,
`test_dedup_different_instructions_not_skipped` `:115`). If the helper's
placement changes, the matrix belongs there — covering the default profile,
the late-system-avoiding profile, and the "first row is not a System" case.

### 8.7 The existing suites must stay green and unmodified in intent

`tests/agent_loop/context_continuity_m004.inc`,
`tests/agent_loop/context_epoch_handoff.inc`,
`tests/agent_loop/long_horizon_trajectory_qualification.inc`, and the
continuity tests in `src/context/compaction.rs`
(`m002_frame_replacement_keeps_one_current_frame` `:3850`,
`m002_repeated_compaction_does_not_stack_frames` `:3881`).

## 9. Required verification commands

```bash
# the new two-turn regression, by name
cargo test --locked -- context_continuity

# the continuity families
cargo test --locked --test agent_loop -- context_continuity
cargo test --locked --test agent_loop -- context_epoch_handoff
cargo test --locked --test agent_loop -- long_horizon_trajectory

# the in-crate placement tests
cargo test --locked -p codegg --lib -- context_runtime

# the projection placement matrix
cargo test --locked -p codegg-core --lib -- model_profile

# the workspace and the guards
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

`scripts/verify.sh quick` is the canonical sanity gate (rustfmt, the
generated-asset checks, the guard scripts, and a workspace check). If the
change reaches the desktop tree, the hosted desktop qualification is also
required — see `scripts/check_desktop_gate_wiring.py`.

## 10. Acceptance criteria

1. After a turn that compacts, the next turn's provider-visible sequence does
   not contain the pre-compaction body the daemon already replaced.
2. The compaction is **effective across the turn boundary**: the sequence that
   crosses the wire on the next turn is the compacted one.
3. Exactly one CodeGG-owned continuation frame is present in the
   provider-visible sequence.
4. The continuation projection is positioned **before** the current user
   message on **every** model profile.
5. The "current user message" reads return the human's prompt, or a typed
   absence — never a daemon-injected row's text.
6. A prefix the daemon cannot identify exactly is **kept**, and the outcome is
   a bounded diagnostic.
7. The durable session transcript is unchanged: it still holds the uncompacted
   record, and its user row is still the human's prompt.
8. The frontend's transcript is not truncated, rewritten or emptied, and the
   user still sees the full scrollback.
9. `CoreRequest::TurnSubmit` and `crates/codegg-client`'s compose contract are
   unchanged; no new protocol variant lands.
10. The checkpoint payload stays within its 128 KiB bound and carries no
    secret-bearing or hidden-reasoning body.
11. The compaction engine's policy, thresholds, tracker and invariants are
    unchanged.
12. The two-turn regression test of §8.1 **fails on the baseline** and passes
    after the change.
13. `scripts/verify.sh quick` is green, and fmt and workspace clippy with
    `-D warnings` are clean.

## 11. Documentation updates

- `architecture/context-compaction-ownership.md` — correct the "Transactional
  rollover (M004)" and "Fresh context epochs" sections to describe the
  turn-boundary behaviour as it now is, and to name the new durable fields.
  The document's "Source Verification" section must be re-verified against the
  landed code, and any claim that has drifted corrected in the same change.
- `architecture/compaction.md` — the turn-boundary rule, if it describes the
  sequence's ownership.
- `architecture/core.md` — if it states who owns the provider-visible history.
- `plans/closure/context-continuity-compaction-post-closure-corrective/001-status.md`
  — the closure record (written at closure, not by the implementer of this
  plan).
- `plans/registry.md` — the status transitions and the unblock audit.

## 12. Closure evidence required

1. The two-turn request, **before and after**, quoted — the message list as
   it crosses the provider boundary, showing the pre-compaction body absent
   after the change and present before it.
2. The projection's placement, per model profile, with the branch that changed.
3. The fail-closed case: the mismatched prefix kept, and the diagnostic.
4. The mechanism the daemon re-installs in place of the dropped prefix, and
   **what it does not carry** — stated plainly, including whether the omitted
   material is reachable only through `context_read`.
5. The measured checkpoint payload size against the 128 KiB bound.
6. The commands in §9 with their real outcomes, labelled local vs CI.
7. The evidence that the baseline fails the §8.1 test — a recorded failing run
   at the pre-change commit, not an assertion that it "would" fail.
8. The existing continuity suites, named and green, unmodified in intent.
9. The registry update and the unblock audit, including the statement that
   nothing newly unblocked if that is the outcome.

## 13. Handoff notes

- **Read the ADR first.** ADR-0054 fixes the contract and the authority. The
  mechanism is yours to choose inside it; the *obligations* in §6 are not.
- **The baseline is `origin/main` at `957fcbf00`.** If the tree has moved,
  re-derive the citations in §2 before acting on them, and record any drift.
- **Do not "fix" this by weakening the injection.** The cheapest-looking change
  — suppressing the re-injection when the transcript already carries the
  material — leaves the compaction ineffective and the request growing without
  bound. It is stop condition 1's cousin; it is not a solution.
- **`agentfindings.md` is the reviewer's working notes** for this defect and is
  git-ignored. It is evidence, not a contract: the ADR and this plan are the
  contract.
- The milestone is **one ownership boundary**. If it starts to look like three
  milestones, the ordering in §6 is the split point: §6.1–§6.3 are the
  authority change and belong together; §6.4 and §6.5 are the ordering and
  identity corrections that ride with it.
