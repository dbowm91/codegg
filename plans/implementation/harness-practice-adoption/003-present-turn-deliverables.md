# Harness Practice Adoption Milestone 003 — Present: Declared Turn Deliverables

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M003, §8

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — bounded payloads; a frontend displays, the daemon owns state
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/003-planning-process.md`

Applicable ADRs:

- **None as written.** The deliverable record is the tool's own structured
  result, which the session transcript already persists and the projection
  already carries. §14 makes a stop condition of the case where it is not: if
  the frontend cannot recognize the result without a new projection field or a
  new `CoreRequest` variant, that is an ADR.

Primary class: capability

## 1. Objective

The model can declare files as a turn's deliverables, and the harness **verifies
each one before recording it** — the path is inside the workspace, it exists, it
is a regular file, and this turn wrote it — so "I created `src/foo.rs`" becomes a
checked claim rather than a sentence.

## 2. Why this milestone is ready

- No hard dependency. The roadmap records the line as free of them.
- The tool contract vocabulary exists: effect class, structured result, schema,
  and category, all enforced by `ToolBroker`'s pipeline.
- Path containment already has one authority: `validate_path` joins the turn's
  `workspace_root` and refuses a missing root with a typed error.
- Durable per-turn write evidence exists and is already scoped correctly: the
  edit checkpoint captures affected paths per `(workspace, session, turn,
  batch)` and is explicitly *not* derived from the global file-change stream.
- The frontend already renders a turn's tool results; a card is presentation,
  not a protocol change.

## 3. Current implementation evidence

**There is no deliverable tool.** A workspace-wide search for `deliverable`
finds only the user-prompt queue's delivery vocabulary. No tool declares a file
as a turn's output, and no durable record distinguishes "the turn delivered
this" from "the repository happens to contain this".

**The TUI's changed-file list is a different claim.** `src/tui/app/state/session.rs`
holds `ChangedFile { path, action, diff_preview, diff_state }` per session,
driven by `AppEvent::FileChanged` and an async diff-stat worker
(`src/tui/file_diff.rs`). That answers "what did this session change", is
frontend state, and is not a model declaration.

**The global file-change stream is not durable evidence — by decision.**
`src/agent/tool_batch.rs` states that durable checkpoint capture is scoped to
workspace/session/turn/batch and "does not depend on unscoped global
FileChanged draining", and it drains that stream "for hygiene but do not use for
durability". Any milestone that used `FileChanged` to prove authorship would
contradict a decision the tree already made.

**Per-turn write evidence exists.** `src/agent/tool_batch.rs` builds a private
`CheckpointContext { paths, pre_states, workspace_id, session_id, turn_id,
batch_seq }` and hands it to `crate::snapshot::checkpoint::EditCheckpointManager`.
The paths are the batch's affected files, and the record is durable and scoped.

**Tool results are already durable and projected.** A tool call and its result
are persisted in the session transcript and carried on the projection, so a
structured deliverable result needs no new table and no new field.

**A writer-surface guard exists.** `scripts/check_writer_disposition.py`
requires a recorded disposition for any tool that can mutate a checkout, launch
a process, or perform a filesystem effect. `present` does none of those; the
guard is run to confirm that classification rather than to record one.

## 4. Invariants that must not regress

- **`present` holds no write authority.** It reads metadata and the checkpoint
  record. It does not create, modify, delete, or move anything, and it takes no
  writer permit.
- **Path containment has one authority.** Every model-supplied path goes through
  `validate_path` against the turn's `workspace_root`. A path outside it, an
  absolute path, a `..` escape, and a symlink whose target escapes are refused
  with a typed error. The tool never resolves a path itself.
- **Authorship evidence is the durable checkpoint, never the global stream.**
  Using `AppEvent::FileChanged` here would contradict `tool_batch.rs`'s recorded
  decision.
- **The declaration cannot make a false claim.** A path that does not exist, is
  not a regular file, or was not written by this turn is refused with a reason
  naming the path. The tool must not "fix up" a claim by dropping a bad entry
  silently; a partial declaration is refused, not trimmed.
- **The record is the tool result.** No second store, no second authority, no
  duplicate row that can disagree with the transcript.
- **The tool is bounded.** A declaration has a maximum entry count and a
  maximum path length; exceeding either is a typed refusal.
- **Restricted subagents do not get it.** A delegated child's result is
  machine-derived from Git state; letting it also declare deliverables would
  create a second, weaker claim about the same work.

## 5. Scope

### In scope

- One read-only tool, `present`, with a bounded schema and a typed structured
  result.
- Verification against the turn's checkpoint record, with a bounded read added
  to the checkpoint manager if it does not already expose one.
- A typed refusal per failure shape.
- A TUI card at the turn tail that renders the verified declarations.
- Denial by default in restricted agents.

### Explicitly out of scope

- A new table, a migration, a `CoreRequest` variant, or a projection field.
  §14.
- Copying, moving, or publishing a deliverable. The tool declares; it does not
  transfer.
- Verifying content (a hash, a size, a line count). The claim is "this turn
  delivered this file", not "this file is correct".
- A desktop card. `apps/desktop/` has its own renderer and its own lane; the
  result shape is frontend-neutral, so parity is a later milestone rather than
  an assumption.
- Changing the existing changed-file panel. It answers a different question and
  keeps doing so.

## 6. Required production changes

### Core/domain

- A `PresentedFile { path, description }` value and a `PresentResult
  { delivered: Vec<PresentedFile>, refused: Vec<...> }` shape carried as the
  tool's structured result.
- A bounded read on the checkpoint manager: the set of paths written for
  `(workspace_id, session_id, turn_id)`. If the manager already answers this,
  use it; do not add a second index.

### Storage and migrations

None. The result rides the transcript row the tool call already produces. This is
the milestone's main compatibility property and §16 asks the agent not to add a
table for convenience.

### Protocol and DTOs

None. The structured result is a tool result; the projection already carries
tool results. If the frontend cannot render it without a new field, §14 applies.

### Runtime and concurrency

- The tool executes inside the batch, so it sees the turn's current batch's
  checkpoint context. Paths written by an **earlier** batch of the same turn are
  found through the checkpoint manager's read, not through the batch context.
- The read is bounded: at most the turn's own paths, capped.

### Frontend or operator surface

- A turn-tail card in the TUI listing the delivered files, each with its
  description and a way to open the file in the editor.
- The card is rendered from the tool result; it does not introduce a second
  store of what was delivered.
- A refused declaration renders as a failure, with the reason, at the tool
  call's position — not as a silent absence.

### Security and authorization

- `validate_path` is the only resolver.
- The tool is registered as read-only in the contract and hidden from restricted
  agents by the same mechanism that hides other denied tools.
- No capability, role, or principal change.

### Documentation and static guards

- `architecture/tool.md` gains `present`: its contract, its four refusals, and
  its authorship evidence.
- `architecture/tui.md` gains the turn-tail deliverable card and states that it
  renders a tool result.
- No new guard is required; `check_writer_disposition.py` is run to confirm the
  read-only classification.

## 7. Ordered work packages

### Work package A — The tool and its refusals

Intent: the claim is checkable before anything renders it.

Required changes:

- Schema: `{ files: [ { path: string, description?: string } ] }`, bounded to
  `MAX_PRESENTED_FILES` entries and `MAX_PRESENTED_PATH_LEN` per path.
- Each path: `validate_path` against the turn's root; must exist; must be a
  regular file (a symlink is resolved and its target must be inside the root);
  must appear in the turn's checkpoint paths.
- Four typed refusals, each naming the path:
  `outside_workspace`, `missing`, `not_a_regular_file`, `not_written_this_turn`;
  plus `too_many_files` and `path_too_long`.
- A declaration containing one bad entry is refused in full. The tool does not
  report a partial success.

Acceptance evidence:

- One test per refusal shape, each asserting the typed variant and that the
  path appears in the message.
- A test proving a declaration of a file written in an **earlier batch of the
  same turn** succeeds.
- A test proving a declaration of a file written by a **different turn**
  fails with `not_written_this_turn`.

### Work package B — The checkpoint read

Intent: authorship comes from the durable record, not from a stream.

Required changes:

- If `EditCheckpointManager` does not expose it, add one bounded query: paths
  written for a `(workspace_id, session_id, turn_id)`, capped, ordered
  deterministically.
- The query reads the durable checkpoint store. It does not subscribe to
  `AppEvent::FileChanged`.
- A missing `turn_id` is a refusal (`not_written_this_turn` cannot be proven), not
  a permissive default.

Acceptance evidence:

- A test proves the query returns the batch's paths for the turn and nothing
  from another turn.
- A test proves the query is bounded: a turn with more paths than the cap
  returns the cap, deterministically ordered.

### Work package C — The structured result and the card

Intent: the user sees what the turn delivered, from the same record the harness
verified.

Required changes:

- The tool returns a structured result carrying the verified files.
- The TUI renders a turn-tail card from that result; opening a row opens the
  file in the editor through the existing editor-open path.
- A refused declaration renders as a failed tool call with its reason.

Acceptance evidence:

- A `tui_render` test asserts the card lists exactly the declared files in the
  declared order and renders nothing for an absent declaration.
- A test asserts a refusal renders the reason and no card.

### Work package D — Restricted-agent denial and classification

Intent: the tool is not a second claim channel for a delegated child.

Required changes:

- Denied by default for restricted agents, using the existing denial mechanism.
- Confirm the contract's effect class is read-only and that
  `check_writer_disposition.py` reports no required disposition.

Acceptance evidence:

- A test proves a restricted agent's registry does not offer `present`.
- `check_writer_disposition.py` is green with no new entry.

## 8. Failure, cancellation, restart, and contention semantics

- **A file written and then deleted in the same turn.** `missing` — the claim is
  about a file the user can open now.
- **A file written, then the turn cancelled.** The declaration never runs; no
  card. The checkpoint remains, which is correct: the turn did write it.
- **A path that is a directory.** `not_a_regular_file`.
- **A symlink inside the workspace pointing outside.** `outside_workspace`,
  because the resolver checks the resolved target.
- **A turn with no checkpoint record.** Every entry is
  `not_written_this_turn`. The tool does not fall back to a weaker check.
- **Restart.** The checkpoint store is durable, so a declaration in a later
  process still resolves. The card renders from the transcript, which is also
  durable.
- **Contention.** The tool reads; it does not take a permit and cannot deadlock
  with a writer. Two `present` calls in one turn are independent.

## 9. Compatibility and migration

- No storage, protocol, configuration, or dependency change.
- The tool is additive: an older transcript without a `present` call renders
  exactly as it does today.
- The result shape is frontend-neutral so a second frontend can render it
  without a protocol change.
- If a later milestone wants the declaration in the canonical projection, that
  is a separate decision under ADR-0053, not a widening of this one.

## 10. Required tests

### Focused unit tests

- Schema bound: too many entries and an over-long path are refused.
- Each of the four path refusals, with the typed variant.
- A partial declaration is refused in full.

### Integration tests

- A turn writes a file and declares it: the result lists it.
- A file written in an earlier batch of the same turn is accepted.
- A file written by another turn is refused.
- A restricted agent is not offered the tool.

### Restart and recovery tests

- A declaration resolves after a store reopen.

### Contention and cancellation tests

- Two declarations in one turn are independent.
- A cancelled turn produces no declaration.

### Security and negative tests

- An absolute path, a `..` escape, and an escaping symlink are each refused.
- The tool holds no writer permit, proven by asserting the permit store is
  untouched.

### Migration and compatibility tests

- Not applicable: no migration. A test asserts the result renders from an
  existing transcript row.

## 11. Required verification commands

```bash
# the tool
cargo test -p codegg --lib -- tool::present
cargo test -p codegg --lib -- snapshot::checkpoint

# the loop and the frontend
cargo test --test agent_loop -- present
cargo test --test tui_render -- present_deliverable

# classification guards
python3 scripts/check_writer_disposition.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and linting
cargo fmt --check
cargo clippy -p codegg --lib --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

The closure record states which commands ran.

## 12. Documentation updates

- `architecture/tool.md` — `present`'s contract, its refusals, and why its
  authorship evidence is the checkpoint rather than the file-change stream.
- `architecture/tui.md` — the turn-tail card and the fact that it renders a tool
  result.
- `architecture/agent-tool-surface.md` — the tool appears in the generated
  catalog once M004 lands; until then, in the hand-written list.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. A declaration of a file this turn wrote succeeds and is rendered at the
   turn's tail.
2. A path outside the workspace, a missing path, a directory, and a path written
   by another turn each fail with a distinct typed error naming the path.
3. A declaration containing one bad entry produces no partial success.
4. The result survives a store reopen and renders from the transcript.
5. A restricted agent's registry does not offer the tool.
6. No table, migration, `CoreRequest` variant, or projection field is added,
   proven by the diff.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the frontend cannot render the declaration from the existing tool result
  without a new projection field or a `CoreRequest` variant — that is an ADR;
- the checkpoint manager cannot answer "paths written in this turn" without a
  schema change, and the alternative would be reading the global file-change
  stream — which `tool_batch.rs` already refused as durable evidence;
- authorship cannot be established at all; then drop the authorship claim, keep
  containment and existence, and record the reduction in the closure record
  rather than weakening the wording of the tool's description;
- the tool would need write authority to verify a claim;
- rendering the card requires a second store of what was delivered.

## 15. Closure evidence required

- One test result per refusal shape, with the message text.
- The earlier-batch and other-turn tests.
- The `tui_render` card test.
- The restricted-agent denial test.
- `check_writer_disposition.py` output confirming no new disposition.
- A statement, with the diff, that no migration or protocol change was made.
- The named non-claim: the tool does not verify content, only authorship,
  existence, and containment.

## 16. Handoff notes

- **Do not add a table.** The result is the record. A second store will disagree
  with the transcript eventually, and the transcript is what the user reads.
- **Do not read `AppEvent::FileChanged`.** `tool_batch.rs` already records why:
  it is unscoped and not durable.
- **A partial declaration is a refusal.** Trimming the bad entry silently is how
  a tool becomes a liar.
- **Resolve symlinks.** A link inside the workspace pointing outside it is an
  escape, and the resolver is the place to catch it.
- **The description is user-facing text, not a claim.** Do not let it become a
  second, unverified summary of the file's contents.
