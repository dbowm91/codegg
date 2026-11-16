# TUI Editor Helix Capability Parity Milestone 009 — Undo Tree

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-009--undo-tree`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Replace the editor's linear history and its cursor with a **tree**, so an edit
made after an undo creates a sibling branch instead of destroying the redo tail,
while keeping every invariant the linear model established: both directions
retained, an entry never moves, selections restored, and a bounded byte budget.

## 2. Why this milestone is ready

No hard dependency, and it is the **lowest-value milestone in this roadmap** —
the roadmap's §10 risk 4 records that the owner may drop it without affecting
M001–M008. It is planned rather than hidden because leaving a known parity gap
unrecorded is worse than recording a low-value one.

Everything it needs is closed:

- `EditorUndoEntry` retains both directions, the cursor and the selection set
  observed before the transaction, and a byte count. The tree adds a parent
  identity and changes what the cursor names; it does not change what an entry
  *is*.
- `MAX_EDITOR_UNDO_DEPTH` (256) and `MAX_EDITOR_UNDO_BYTES` (4 MiB) are the
  established bounds, and the pruning loop that enforces them is one place.
- `traverse_history` is the single traversal function, and `editor_undo`,
  `editor_redo`, `editor_history_earlier` and `editor_history_later` all route
  through it with a `HistoryStep` and a `report` flag. That is the seam the tree
  replaces.
- The insert-session commit path (`commit_insert_session`) already produces one
  entry per typing burst, so the tree's node count does not change for ordinary
  typing.

## 3. Current implementation evidence

At the baseline:

- `TuiDocumentPresentation` holds `history: Vec<EditorUndoEntry>`,
  `history_bytes: usize`, and `history_cursor: usize`.
- `EditorUndoEntry`'s own documentation states the model: "The entry never moves.
  The history is one ordered list and a cursor names the position in it, so
  'undo' and 'redo' are the same movement in opposite directions rather than a
  transfer between two stacks. That is what makes the 'both directions'
  invariant structural: there is no direction in which an entry can lose its
  other half."
- The pruning loop trims while the length exceeds `MAX_EDITOR_UNDO_DEPTH` or the
  bytes exceed `MAX_EDITOR_UNDO_BYTES`, so it drops from one end of the list.
- `traverse_history` commits an open insert session first, then reads
  `undo_entry(presentation)` or `redo_entry(presentation)`, applies the entry's
  inverse or forward transaction, and restores the cursor and selection set.
- An edit made after an undo **truncates** the tail: the abandoned branch is
  unreachable. That is the parity gap.
- Helix's `earlier`/`later` traverse **time**, which the linear cursor already
  does; the difference appears only after an undo-then-edit.

## 4. Invariants that must not regress

- **An entry never moves.** A node's identity is stable for its lifetime.
- **Both directions are retained per entry**, so undo and redo are the same
  movement in opposite directions.
- **The selections observed before the transaction are restored** with it.
- **The byte budget still holds**, and it now bounds the whole tree.
- **`u`/`U` walk within a branch; `Alt-u`/`Alt-U` traverse creation order.**
  The four verbs keep their distinct meanings, and the boundary disclosure still
  names which boundary was hit.
- **An open insert session is committed before any traversal**, so the reader's
  own typing is never invisible to the `u` they just pressed.
- **No second text buffer**; entries hold transactions and positions only.
- **Pruning never strands a child.** An interior node a child still needs cannot
  be dropped.

## 5. Scope

### In scope

- Entries gain a parent identity; the history becomes a tree with one root.
- The cursor becomes a node identity plus a "last visited child" record per node.
- `u`/`U` walk parent / last-visited child; `Alt-u`/`Alt-U` walk creation order.
- Pruning operates on whole branches, oldest leaf first, and discloses.
- The banner row's history position remains observable.
- `docs/tui.md` updated where it describes the history model.

### Explicitly out of scope

- An undo **graph** view or any UI beyond the existing banner position.
- Persisting history across a document close, a reload or a reconnect. History is
  cleared by the same transitions it is cleared by today.
- Sharing history between documents (one document is attached at a time).
- Changing what constitutes one entry (the insert-session rule stays).
- Undoing an LSP-applied change differently from a local one.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Traversal is synchronous.

### Frontend or operator surface

- `src/tui/editor.rs`: the tree — nodes, parent links, last-visited child, the
  root, and the pruning rule; `undo_entry`/`redo_entry` become node navigation;
  the creation-order index for `Alt-u`/`Alt-U`.
- `src/tui/document_session.rs`: the history field's shape and the cursor's
  meaning.
- `src/tui/commands/editor.rs`: `traverse_history` reads the tree; the four verbs
  keep their `HistoryStep` + `report` shape.
- `src/tui/app/render.rs`: the banner's position disclosure, if it prints an
  index.

### Security and authorization

No change. Traversal applies a stored transaction through `apply_local`.

### Documentation and static guards

- `docs/tui.md`: the history model described as a tree, with what `u`/`U` and
  `Alt-u`/`Alt-U` each do and what pruning drops.
- No guard change is expected; the text-authority guard's rules are unaffected
  because the entries hold transactions, not text.

## 7. Ordered work packages

### Work package A — The node and the cursor

Intent: give an entry a stable identity and a parent, and make the cursor name a
node.

Required changes: a node identity type; a parent link; the root; a
`last_visited_child` per node so `U` knows which child to follow when a node has
several. The "entry never moves" property becomes "node never moves", and the
existing doc comment's argument is preserved verbatim in shape.

Acceptance evidence: a linear sequence of edits produces a chain whose traversal
reproduces today's `u`/`U` behaviour exactly; the doc comment's invariant is
still true of the new structure.

### Work package B — Branching

Intent: the whole point.

Required changes: a new edit after an undo creates a child of the current node
rather than truncating anything. `last_visited_child` is updated when a branch is
taken, so `U` returns to the branch the reader last walked.

Acceptance evidence: undo, edit, undo, `Alt-u` reaches the pre-branch state and
`Alt-U` returns to the new branch; undo, edit, `U` goes to the **new** branch, not
the abandoned one; undo, edit, `u`, `u`, `Alt-U` returns along the abandoned
branch's creation order.

### Work package C — Creation order for `Alt-u`/`Alt-U`

Intent: keep `earlier`/`later` meaning time.

Required changes: a creation-order index over the nodes. `Alt-u` steps to the
previous node in creation order, `Alt-U` to the next. This is *not* the same as
walking the tree, and the plan states the difference so a later reader does not
merge them.

Acceptance evidence: after a branch, `Alt-u` walks back through creation order
across both branches; `Alt-U` walks forward; the boundary disclosure names the
boundary.

### Work package D — Pruning

Intent: keep the budget without stranding a child.

Required changes: pruning drops **whole leaf branches**, oldest first, until the
byte budget and the depth budget both hold. An interior node is never dropped
while a child survives. If a single branch cannot be pruned enough, the whole
history is cleared and that is disclosed — a stated fallback, not a silent one.

Acceptance evidence: a workload that exceeds the depth budget prunes leaf branches
and keeps every surviving node's parent chain intact; a workload that exceeds the
byte budget does the same; the fallback path is reachable in a test and discloses.

### Work package E — Documentation

Required changes: describe the tree, the two traversal kinds, and the pruning
rule in `docs/tui.md`; keep the banner's position disclosure honest.

Acceptance evidence: the doc-drift guard passes; the banner still reports a
position.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A traversal applies one transaction; a controller rejection
  is reported and the cursor does not move.
- **Duplicate delivery.** None; synchronous.
- **Restart.** History is empty after a restart, as today.
- **Cancellation races.** None; no async work added.
- **Stale generations.** Cleared by the same transitions as today.
- **Bound reached.** Pruning drops leaf branches and discloses; the
  cannot-prune-enough fallback clears the history and discloses.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**The compatibility surface is the history model's own documentation.** The
predecessor's M012 closure states "the history is one list with a cursor in it".
That remains historically true of M012; this milestone supersedes the model, and
the roadmap is the mechanism for superseding a closed milestone's model rather
than reopening it. The plan must state that explicitly so a later reader does not
read the two documents as a contradiction.

There is no user-visible regression: `u`/`U`/`Alt-u`/`Alt-U` keep their meanings,
and the only observable difference is that an abandoned branch is now reachable.

Removal criteria for a transitional state: none; there is no transitional state.

## 10. Required tests

### Focused unit tests

- The chain: a linear sequence traverses exactly as today.
- Branching: undo, edit, and the resulting sibling structure.
- `last_visited_child`: updated on branch entry, and `U` follows it.
- Creation order: `Alt-u`/`Alt-U` across a branch.
- Pruning: leaf-first, parent chains intact, the fallback path.

### Integration tests

- `u`, `U`, `Alt-u`, `Alt-U` through the real dispatch path, including the
  boundary notices.
- An open insert session committed before traversal.

### Restart and recovery tests

None beyond the clearing assertion; the closure record must say the history is
in-memory by design.

### Contention and cancellation tests

- A branching workload that exceeds the depth budget prunes without stranding a
  node — asserted by walking every surviving node's parent chain.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A traversal whose transaction the controller rejects leaves the cursor unmoved.

### Migration and compatibility tests

- The linear-equivalence test: for a history with no branch, the tree produces
  byte-identical behaviour to the list. This is the regression gate.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

## 12. Documentation updates

- `docs/tui.md`: the history model as a tree; `u`/`U` walk a branch; `Alt-u`/`Alt-U`
  walk creation order; pruning drops oldest leaf branches and clears the history
  when it cannot prune enough.
- The `EditorUndoEntry` doc comment's argument is preserved in shape and extended
  with the parent link, so the "both directions" invariant still reads as
  structural.

## 13. Acceptance criteria

- A history with no branch behaves byte-identically to today for `u`, `U`,
  `Alt-u` and `Alt-U`.
- Undo, edit, then `Alt-u` reaches the pre-branch state; `Alt-U` returns to the
  new branch.
- `U` follows the branch the reader last walked, not the abandoned one.
- Pruning drops leaf branches and every surviving node's parent chain is intact.
- The banner still reports a position, and the boundary disclosure names the
  boundary.
- The text-authority guard passes with no allowlist edit; the doc-drift guard
  passes.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Pruning cannot keep the byte budget without stranding a child and the fallback
  (clear the history) is judged unacceptable. That is an owner decision about a
  memory bound, not a detail.
- `Alt-u`/`Alt-U`'s creation-order semantics cannot be implemented without a
  second ordering structure that could disagree with the tree. Report the shape.
- The work appears to require persisting history or sharing it across documents.
- A reviewer asks for an undo **graph** UI. That is a surface, not this
  milestone.
- The change would make one entry per keystroke rather than per insert session.
  That would multiply the node count and is a different model.

## 15. Closure evidence required

- The linear-equivalence test's output, which is the regression gate.
- The branching traversal evidence for each of the cases in §10.
- The pruning evidence, including the parent-chain walk and the fallback path.
- The boundary disclosure evidence.
- A statement that the M012 closure's claim remains historically true and that
  this milestone supersedes the model rather than contradicting it.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **This is the droppable milestone.** The roadmap says so. If the owner
  deprioritizes it, record the drop rather than leaving it `ready` forever.
- **Pruning is the hard part, not branching.** A tree whose interior nodes can be
  dropped while children survive is corrupt. Leaf-first, or clear everything.
- **`last_visited_child` is what makes `U` deterministic.** Without it, a node
  with several children has no defined redo target and `U` would pick arbitrarily.
- **Creation order is not tree order.** `Alt-u`/`Alt-U` are time; `u`/`U` are
  structure. Do not merge them.
- **The linear-equivalence test is the regression gate.** A tree that changes
  today's behaviour for a branchless history is a regression regardless of what it
  adds.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
