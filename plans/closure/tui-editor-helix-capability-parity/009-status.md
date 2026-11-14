# TUI Editor Helix Capability Parity Milestone 009 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/009-undo-tree.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-009--undo-tree`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — **235 changed or untracked paths** at the time of
review (measured, `git status --porcelain | wc -l`)

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same tree carries the other Helix
  milestones and the TUI-modularity line, so no statement below is attributable
  to a SHA.

Provenance labels used throughout: **measured** means this record's author read
it out of the tree or ran the command at the stated tree state; **parent-run**
means the parent session ran it on this same tree and supplied the transcript,
which this record did not repeat; **source plan** means the statement is the
plan's requirement, not a measurement.

**Line numbers are from the working tree at the time of this review, not from
HEAD.** Function and test **names** are the authoritative reference; the numbers
are a convenience for a reader of this tree.

## 1. Executive finding

The capability is delivered as a tree rather than a relabelled list. `HistoryId`
is a stable node identity (`src/tui/editor.rs:1905`); an entry carries `created`,
`parent` and `last_child` (`:1940-1948`); `push_undo` (`:6758`) makes an edit
after an undo a **sibling** of the branch it did not take (`:6754-6777`) instead
of truncating a tail; the cursor lives in `document_session.rs` as a `HistoryId`
(`:341-343`); and traversal has two named axes — `HistoryWalk::Branch` for
`u`/`U` and `HistoryWalk::CreationOrder` for `Alt-u`/`Alt-U`
(`src/tui/commands/editor.rs:2862`, `:2908`).

Four properties are structural rather than conventional:

1. **Nothing is truncated.** `push_undo` appends to the arena and re-points the
   parent's `last_child`; the pre-tree `truncate_history_tail` is **deleted** —
   zero occurrences remain anywhere under `src/` or `tests/` (measured).
2. **The arena is creation order.** Identity is monotonic and the arena is kept
   sorted, so `node_index` is a binary search and pruning never renumbers the
   survivors (`editor.rs:6794-6803`).
3. **Redo is deterministic.** `chosen_child` (`:6831`) trusts `last_child` only
   after checking it still names a child of that parent, so a link a prune forgot
   to clear cannot send the reader outside the node's own children.
4. **The cursor moves only after the text accepted the change.**
   `traverse_history` applies each edge first and moves the cursor on the node the
   text actually reached (`commands/editor.rs:2990-3013`), restoring that node's
   recorded cursor and selection set (`:3016-3029`). A refusal leaves the history
   and cursor where they were, and a path refused part-way leaves the cursor on
   the node the text reached (`:2900-2907`).

**Two deviations from the plan are recorded rather than smoothed over, and one of
them needs an owner decision.**

- **Pruning a single unbranched chain drops the oldest node and re-parents its
  one child to the root.** `prune_victim` (`editor.rs:7136-7157`) prefers the
  oldest leaf and falls back to `history.first()`, and `drop_node`
  (`:7164-7191`) re-parents the removed node's child and `debug_assert`s that at
  most one child exists in that case. This **contradicts the plan's work package
  D** — "An interior node is never dropped while a child survives" — but it is
  what keeps a 257-edit session from either clearing its whole history or failing
  the `len == MAX_EDITOR_UNDO_DEPTH` linear-equivalence gate. It does not strand a
  child: the survivor is re-parented to the removed node's parent, so every
  surviving node's parent chain stays intact. It is a deliberate third option
  between the plan's two (§7 WP D: leaf-first; §14: clear and report), and it is
  traced to an owner decision in §10/§11 rather than treated as settled.
- **Only the fallback clear is disclosed.** `trim_history` returns `true` only
  when it cannot prune enough and clears the nodes (`editor.rs:7117-7134`), and
  that boolean is stored as `history_bound_cleared` (`:6791`), which paints
  `history cleared at its bound` (`src/tui/components/editor.rs:759-762`).
  Ordinary leaf-on-leaf pruning is **silent**. The plan's §5 says pruning
  "operates on whole branches, oldest leaf first, and discloses"; the
  implementing lane chose silence on the stated ground that a latched "pruned"
  flag is permanent noise after the first 256 edits. This is a divergence from
  the plan's wording, recorded as a finding rather than a summary.

The capability is evidenced well: the model suite `tui::editor::tests` is **236
passed, 0 failed** (measured), the two dispatch-level tree tests pass
individually (measured), the render corpus is **1227 passed, 0 failed**
(parent-run on this tree), `scripts/verify.sh quick` **passed** (parent-run,
`QUICK_RC=0`), and the four plan-required read-only guards this record ran all
pass (measured).

Status is conditional, not closed, for three reasons:

1. **Nothing is committed.** Every figure is a property of a working tree with
   235 dirty or untracked paths, not of a SHA.
2. **`scripts/verify.sh full` had not run when this record was written; it ran
   on 2026-11-13 and passed.** The plan's §11 names `cargo fmt --check` and the
   routine gate; `quick` covers formatting but not `clippy --workspace
   --all-targets -- -D warnings`, the nextest workspace sweep or the bounded
   feature pass. That run supplied all of them on this tree — 17,377 workspace
   tests and 8,441 feature-pass tests, 0 failures, formatting and clippy clean —
   so the gate reason no longer holds.
3. **The pruning deviation needs ratification.** It is a memory-bound policy
   decision the plan reserved to the owner (§10, medium).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| An entry has a stable identity and a parent; the history is a tree with one root | `HistoryId` (`editor.rs:1905`); `EditorUndoEntry.created/parent/last_child` (`:1940-1948`); root id `0` documented at `document_session.rs:341-343` | pass | `push_undo` sets `parent = history_cursor` (`:6766`). |
| An entry never moves | arena in creation order; `node_index` binary-searches by `created` (`:6798-6803`); identities are monotonic and a pruned identity is never reused (`:6767-6775`) | pass | No renumbering on prune. |
| Both directions retained per entry | every node stores both `forward` and `inverse` (`:6779-6788`); `undo_entry`/`redo_entry` read the same node | pass | The linear model's "both directions" argument survives in shape. |
| The selections observed before the transaction are restored | `selections_before` stored (`:6783`); restored at `commands/editor.rs:3016-3029`; model test suite 236 passed | pass | |
| An edit after an undo creates a sibling, not a truncation | `push_undo` appends and re-points `last_child`; `truncate_history_tail` has **zero** occurrences in `src/` and `tests/` | pass | The parity gap the milestone exists for. |
| `u`/`U` walk parent / last-visited child | `editor_undo` → Branch/Earlier (`commands/editor.rs:3056`), `editor_redo` → Branch/Later (`:3066`); keys at `:6201`, `:6214`; `redo_follows_the_branch_the_reader_last_walked` (`editor.rs:11126`) | pass | |
| `Alt-u`/`Alt-U` walk creation order | `editor_history_earlier`/`_later` (`:3078`, `:3086`); keys at `:6230`, `:6234`, `:6238`; `creation_order_crosses_the_fork_to_the_other_branch` (`editor.rs:11154`) | pass | Alt-Shift-u is also `later` (`:6238`). |
| The two axes are not merged, and each names its own boundary | `HistoryWalk` (`:2862`); `history_boundary` emits `no earlier state` / `no later state` / `no parent state` / `no branch state` (`:2882-2888`); `the_two_axes_name_different_boundaries` (`:13732`) | pass | The creation-order gestures report; `u`/`U` stay silent (`u_and_u_stay_silent_at_their_boundaries`, `:13752`). |
| An open insert session is committed before any traversal | `traverse_history` commits before reading the history (`:2909-2919`) | pass | The comment states why: the reader's own typing must not be invisible to the `u` they just pressed. |
| A history with no branch behaves byte-identically to the list for all four verbs | `a_branchless_history_walks_exactly_as_the_list_did` (`editor.rs:11059`) | pass | The plan's regression gate. |
| Undo, edit, `Alt-u` reaches the pre-branch state; `Alt-U` returns to the new branch | `an_edit_after_an_undo_keeps_the_abandoned_branch` (`editor.rs:11101`); `alt_u_crosses_the_fork_to_the_branch_the_edit_abandoned` (`commands/editor.rs:13670`, passed) | pass | |
| `U` follows the branch the reader last walked | `redo_follows_the_branch_the_reader_last_walked` (`editor.rs:11126`); `u_walks_the_branch_and_never_crosses_the_fork` (`commands/editor.rs:13710`, passed) | pass | |
| Pruning drops leaf branches and every surviving parent chain is intact | `pruning_drops_the_oldest_branch_and_keeps_every_parent_chain` (`editor.rs:11216`); `drop_node` re-parents the unique child (`:7183-7190`) | pass | The **mechanism** deviates on a single chain — §1, §10. |
| Depth and byte budgets still bound the whole tree | `trim_history` tests both caps (`:7124-7125`); `undo_history_drops_the_oldest_entry_past_the_depth_cap` (`:11007`), `..._past_the_byte_cap` (`:11024`) | pass | |
| A history that cannot be pruned is cleared and disclosed | `a_history_that_cannot_be_pruned_is_cleared_and_disclosed` (`:11258`); `clear_history_nodes` (`:7197`) | pass | Ordinary leaf pruning is silent — §1, §10. |
| The banner still reports a position | `history_bound_cleared` paints `history cleared at its bound` (`components/editor.rs:759-762`); `the_banner_names_a_history_cleared_at_its_bound` (`:4856`) and `the_history_position_is_disclosed_only_when_the_cursor_is_not_at_the_tip` both pass in the history-filtered run | pass | |
| No second text buffer | `check_tui_editor_text_authority.py`: **pass** — "no retained text buffer and no direct filesystem reads in the editor path"; one advisory borrow note at `components/editor.rs:214` | pass | The guard file is modified in this tree by work not attributable to M009; it has **no allowlist** for the diff to have edited (§10). |
| `check_tui_editor_doc_drift.py` passes | **pass** — "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound" | pass | 100 chords, not the 98 the M005 record recorded; later milestones moved the map. |
| No migration, protocol variant, crate or new authority | `Cargo.lock` **unmodified** (`git status`); `crates/codegg-document/**` unmodified; `crates/codegg-protocol/src/document.rs` unmodified; the only modified protocol file, `dto.rs`, contains **0** `history` matches in its diff | pass | `Cargo.toml` is modified by M005's `syntect` `metadata` feature, not here. |
| The render ratchet holds | `check_tui_render_invariant_baseline.py` — 61 advertised-key, 2495 contrast, 0 hint-truncation; "every one reproduces and none was added unbanked" | pass for M009's contribution | M009 adds no advertised key or contrast pair. |
| The render corpus is green | 1227 passed, 0 failed (parent-run on this tree) | pass | The family count is unchanged by M009; it adds no `tui_render` member. |
| The comment ratchet holds | **banked**: 1,523 files / 138,518 comment lines / 1,233,740 words / ratio 0.1360; the guard and its `--self-test` pass (parent-run) | pass | Not an outstanding item for this milestone. |
| `scripts/verify.sh quick` is green | **pass** (parent-run): `==> Quick verification passed.` / `QUICK_RC=0`; 94 guards including their `--self-test`s, 479 harness tests `OK (skipped=4)`, `cargo check --workspace --all-targets --locked` in 2m43s with no warnings | pass | |
| `cargo fmt --check`, `clippy -D warnings`, the nextest sweep | **discharged 2026-11-13** by `scripts/verify.sh full` | pass | `quick` is not `full`; §10, §11. The `full` run supplied all three: formatting and clippy clean, 17,377 workspace tests at 0 failures. |

## 3. Production implementation evidence

- `src/tui/editor.rs:1905` — `pub type HistoryId = usize`, the stable identity.
- `src/tui/editor.rs:1922-1948` — `EditorUndoEntry` with `created`, `parent`,
  `last_child`; the doc comment preserves the "both directions" argument and
  extends it with the parent link.
- `src/tui/editor.rs:6747-6792` — `push_undo`: the sibling branch, the monotonic
  identity, the parent's `last_child` update, and
  `history_bound_cleared = trim_history(presentation)`.
- `src/tui/editor.rs:6794-6850` — `node_index`, `history_node`, `history_depth`,
  `chosen_child` (the recorded child checked against the tree).
- `src/tui/editor.rs:6873`, `:6889` — `move_history_cursor_back` /
  `move_history_cursor_forward` (branch movement, recording `last_child`).
- `src/tui/editor.rs:6908-6952` — `HistoryEdge`, `creation_order_back`,
  `creation_order_forward`, `history_edges` (the multi-edge path a time jump
  takes over a fork).
- `src/tui/editor.rs:7117-7203` — `trim_history`, `prune_victim`, `has_child`,
  `drop_node`, `clear_history_nodes`.
- `src/tui/document_session.rs:332-349` — the arena in creation order,
  `history_cursor: HistoryId`, `history_next`, `history_bound_cleared`.
- `src/tui/commands/editor.rs:2862-2888` — `HistoryWalk`, `HistoryStep`, and the
  four boundary sentences.
- `src/tui/commands/editor.rs:2908-3033` — `traverse_history`: commit the insert
  session, resolve the target, apply every edge, then move the cursor and restore
  the last edge's cursor and selection set.
- `src/tui/commands/editor.rs:3051-3088` — the four verbs and the doc comments
  that state which axis each walks and which boundary speaks.
- `src/tui/commands/editor.rs:6201`, `:6214`, `:6230`, `:6234`, `:6238` — the key
  wiring for `u`, `U`, `Alt-u`, `Alt-U`/`Alt-Shift-u`.

No core/domain, storage, protocol or runtime change: the plan's §6 marks all four
as none, `Cargo.lock` is unchanged, and the history is frontend-local state.

Nothing in the plan's "explicitly out of scope" list was implemented: there is no
undo-graph surface, history is still in memory (cleared by the same transitions),
only one document is attached, the insert-session rule is unchanged, and an
LSP-applied change is undone exactly like a local one.

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`,
with `CARGO_BUILD_JOBS=2`:

```bash
cargo test -p codegg --lib -- tui::editor::tests::
cargo test -p codegg --lib -- alt_u_crosses_the_fork
cargo test -p codegg --lib -- u_walks_the_branch
cargo test -p codegg --lib -- history          # broad filter, 42 tests

python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_editor_key_authority.py

git status --porcelain Cargo.lock Cargo.toml crates/codegg-protocol crates/codegg-document
grep -rn "truncate_history_tail" src/ tests/
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib -- tui::editor::tests::` | **pass** — **236 passed, 0 failed**, 7946 filtered out, 0.92 s |
| `cargo test -p codegg --lib -- alt_u_crosses_the_fork` | **pass** — 1 passed, 0 failed (`commands::editor::tests::alt_u_crosses_the_fork_to_the_branch_the_edit_abandoned`) |
| `cargo test -p codegg --lib -- u_walks_the_branch` | **pass** — 1 passed, 0 failed (`commands::editor::tests::u_walks_the_branch_and_never_crosses_the_fork`) |
| `cargo test -p codegg --lib -- history` | **pass** — 42 passed, 0 failed; a broad substring filter, cited only as corroboration, not as the model-suite count |
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path"; one advisory borrow note |
| `check_tui_editor_doc_drift.py` | **pass** — "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound" |
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and 0 hint-truncation entries recorded; every one reproduces and none was added unbanked" |
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 92 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" |
| `truncate_history_tail` grep | **0 occurrences** in `src/` and `tests/` — the deletion the plan requires |
| `git status` on manifest/protocol/document | `Cargo.lock` clean; `crates/codegg-document` clean; `crates/codegg-protocol/src/document.rs` clean; `Cargo.toml` and `crates/codegg-protocol/src/dto.rs` modified by other work; the `dto.rs` diff has 0 `history` matches |

### Parent-run evidence

| Command | Result |
|---|---|
| `cargo test --test tui_render` | **1227 passed; 0 failed** on this tree |
| `scripts/verify.sh quick` | **pass** — `==> Quick verification passed.` / `QUICK_RC=0`; 94 guard scripts including each `--self-test`, `python3 -m unittest discover -s dev/harness_lab/tests` → `Ran 479 tests` / `OK (skipped=4)`, `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked` finished with no warnings |

### Not run

| Command | Why |
|---|---|
| `scripts/verify.sh full` | Not run on this tree. `full` adds clippy `-D warnings`, the nextest workspace sweep and the bounded feature pass; a green `quick` is not evidence for those. §10, §11. |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Not run individually; `quick` ran formatting, but lint evidence is `full`'s. |
| `cargo test --test tui_render` | Not re-run by this record; the parent's 1227/0 measurement on the same tree is cited as parent-run. |
| The plan's `cargo test -p codegg --lib tui::editor` | Superseded by the narrower `tui::editor::tests::` filter (236 passed) plus the two individually run dispatch tests. |

**Every result above is a property of this working tree at the time it was read.**
The tree carried 235 changed or untracked paths; later lanes move these files.

## 5. Invariant review

- **An entry never moves.** `push_undo` appends; `node_index` searches the
  creation-ordered arena; identities are monotonically assigned and never
  reissued (`editor.rs:6767-6775`). Holds.
- **Both directions are retained per entry, so undo and redo are the same
  movement in opposite directions.** Every node stores `forward` and `inverse`;
  branch movement selects a node and applies the matching side. Holds.
- **The selections observed before the transaction are restored with it.**
  `selections_before` is stored and restored on the landed node
  (`commands/editor.rs:3016-3029`). Holds.
- **The byte budget still holds, and now bounds the whole tree.**
  `trim_history` loops on both `history.len()` and `history_bytes`
  (`editor.rs:7124-7125`), and `drop_node` subtracts the evicted node's bytes
  (`:7173`). Holds.
- **`u`/`U` walk within a branch; `Alt-u`/`Alt-U` traverse creation order.** Two
  named variants, four verbs, four distinct boundary sentences. Holds and is
  asserted (`the_two_axes_name_different_boundaries`).
- **An open insert session is committed before any traversal.** Holds
  (`commands/editor.rs:2909-2919`).
- **No second text buffer.** The text-authority guard passes; entries hold
  transactions and positions. Holds.
- **Pruning never strands a child.** Holds — including on the deviating
  single-chain path, where the unique child is re-parented to the removed node's
  parent (`editor.rs:7183-7190`). **The plan's stronger wording — an interior
  node is never dropped while a child survives — does not hold**; it is the
  recorded deviation in §1 and §10.

## 6. Failure and recovery review

- **Partial failure.** `traverse_history` applies edges in order and moves the
  cursor only after the applies are accepted; a refusal leaves the history and
  cursor unchanged, and a path refused part-way leaves the cursor on the node the
  text actually reached (`commands/editor.rs:2900-2907`, `:2990-3013`). No
  half-applied traversal can claim a node it did not reach.
- **Duplicate delivery.** Not applicable — synchronous local gestures, no queue
  and no request.
- **Restart.** The history is **in-memory by design**; nothing here is durable
  and no startup path reads it. A restart begins with an empty history, as the
  linear model did.
- **Cancellation races.** None; no async work is added. The gesture reads the
  current presentation through the session.
- **Stale generations.** Cleared by the same reconciliation transitions as
  before: `clear_history` (`editor.rs:7230-7231`) calls `clear_history_nodes`;
  a divergent draft is surfaced, never replayed or discarded on the user's
  behalf.
- **Contention and resource release.** No lock, lease or permit is taken.
- **Malformed or unauthorized input.** The only inputs are stored transactions
  and node identities. A stale `last_child` is validated against the tree rather
  than trusted (`chosen_child`, `:6831-6850`), and a removed node's links are
  repaired on drop (`:7183-7190`).
- **Bound reached.** Depth and byte caps prune; a history that cannot be pruned
  enough is cleared **and disclosed**; ordinary pruning is silent by design (§1).

## 7. Migration and compatibility review

**No storage, protocol, configuration or dependency migration.** `Cargo.lock` is
unchanged, so no crate, version or feature edge was added; `crates/codegg-document/**`
and `crates/codegg-protocol/src/document.rs` are unmodified. The two modified
manifest/protocol files in this tree (`Cargo.toml`, `codegg-protocol/src/dto.rs`)
are other work — the `dto.rs` diff contains no `history`.

**The compatibility surface is the history model's own documentation, and the
supersession statement the plan requires is:** the predecessor's
`plans/closure/tui-editor-helix-parity/012-status.md:13` states that M012
delivered "one ordered list with a cursor" — that **remains historically true of
M012**. This milestone supersedes the model rather than contradicting that
record: the roadmap is the mechanism for superseding a closed milestone's model,
and M012 is not reopened.

**No user-visible regression.** `u`/`U`/`Alt-u`/`Alt-U` keep their meanings; the
only observable difference is that an abandoned branch is now reachable. There is
no transitional state, so there is no removal criterion.

## 8. Security review

No authorization surface changes. A traversal applies a stored transaction
through the same edit funnel every writing verb uses, so a read-only attachment
is refused before any text changes, and a controller rejection is reported with
the cursor unmoved.

Two bounded-input properties hold and are the ones a tree could lose:

- **Byte and depth bounds still bind the whole structure**
  (`MAX_EDITOR_UNDO_DEPTH` 256, `MAX_EDITOR_UNDO_BYTES` 4 MiB), so a branching
  workload cannot grow memory without a ceiling.
- **A forged or stale link cannot walk outside the node's children.**
  `chosen_child` checks the recorded `last_child` against `parent` before using
  it, and falls back to the newest real child.

The undo tree stores transactions and positions, never document text, so it does
not become a second buffer and does not widen what the text-authority guard
permits. No path, process, secret or network surface is touched.

## 9. Documentation and operations

- `docs/tui.md` "History gestures" — rewritten by the implementing lane to
  describe the tree, the two traversal axes and pruning.
- `architecture/tui.md` — the editor history block was rewritten by the parent
  session from the linear list/count-cursor model to the tree, because the old
  text was false (`architecture/tui.md:5651-5728` names the arena in creation
  order, the sibling branch, and the branch reachable by creation order).
- `src/tui/editor.rs` — `EditorUndoEntry`'s doc comment carries the "both
  directions" argument and the parent link; `push_undo`, `prune_victim` and
  `drop_node` each state their rule and the single-chain case.
- `src/tui/commands/editor.rs` — the four verb doc comments state which axis is
  walked and which boundary speaks, including the recorded CodeGG divergence
  that creation-order gestures report and branch gestures do not.
- Operator diagnostics: the only user-visible signals are the banner position and
  `history cleared at its bound`; there is no new command, flag or log line.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **Pruning a single unbranched chain drops the oldest node and re-parents its one child**, contradicting the plan's work-package-D rule that an interior node is never dropped while a child survives (`editor.rs:7136-7191`). | It is what keeps a 257-edit session from clearing the whole history and preserves the `len == MAX` linear-equivalence gate; no child is stranded. But it is a memory-bound policy choice the plan reserved to the owner, made in the implementation rather than recorded as a decision. | The owner ratifies the rule and the plan/roadmap records it, or the implementing lane reverts to the plan's two options (clear-and-disclose, or a different budget). One paragraph in the plan is enough; the code already states the rule. |
| low | **Ordinary leaf pruning is silent**; only the fallback clear paints `history cleared at its bound` (`editor.rs:7117-7134`, `components/editor.rs:759-762`). | The plan's §5 says pruning "and discloses". A reader who loses the oldest branch is not told. The implementing lane's stated reason — a latched flag would be permanent noise after the first 256 edits — is reasonable but is a divergence from the written plan. | Correct the plan's §5 wording to "discloses the fallback clear", or add a bounded, non-latching disclosure. Documentation-only if the owner accepts the reason. |
| low | **Nothing is committed.** HEAD is `7642b9e38`; 235 paths are dirty or untracked. | Every number is a working-tree property and cannot be reproduced from HEAD; the record is not attributable to a SHA. | Land the milestone as its own commit and re-label this record's transcripts with that commit. |
| low | **Discharged 2026-11-13.** **`scripts/verify.sh full` had not run** when this record was written; `quick` was green (parent-run). | The plan's §11 names `cargo fmt --check` and the routine gate; `quick` covers formatting and a workspace check, but not clippy `-D warnings`, the nextest sweep or the feature pass. | None: the 2026-11-13 `scripts/verify.sh full` ran all of them on this tree — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures, `cargo fmt --check --all` and clippy `-D warnings` clean. |
| low | **`scripts/check_tui_editor_text_authority.py` is modified in this tree** (274 insertions, a `--self-test` and a `Report` type). | The acceptance criterion is "passes with no allowlist edit". The guard has **no allowlist at all**, and the diff adds no exemption, so the criterion holds in substance; but "unmodified" cannot be claimed, and the modification is not attributable to M009 from the tree alone. | Attribute the hardening to its lane (it is guard work, not editor work) when the tree is split into commits. |
| low | **Line numbers in this record are from a moving tree.** | The M005 record already recorded ~290-line drift in the same file while it was being written. | Function and test names are authoritative; treat the numbers as navigation aids. |

No high or critical finding. No migration, protocol variant, crate or new
authority was added.

## 11. Roadmap disposition

**Milestone closed (owner decision, 2026-11-14).** The capability is complete and each of the
plan's acceptance criteria has evidence: the linear-equivalence gate, the sibling
branch, the last-visited-child redo, the creation-order axis across a fork, the
parent-chain-preserving prune, the banner position, the two guards and the
documentation. The conditions are:

- **Condition 1 — owner ratification of the pruning rule remains open.** The
  single-chain fallback (drop the oldest node, re-parent its child) must be
  either ratified in the plan/roadmap or replaced (§10, medium). This is the one
  finding that is a decision, not a task, and this record does **not** present
  it as settled: the plan's work package D forbids dropping an interior node
  while a child survives, and the implementation chose a third option the plan
  did not enumerate.
- **Condition 2 — `scripts/verify.sh full` green on the merged tree; met
  2026-11-13.** `quick` was green (parent-run) and `full` has since run on this
  tree: 17,377 workspace tests and 8,441 feature-pass tests, 0 failures,
  `cargo fmt --check --all` clean, clippy `-D warnings` clean. The plan's `fmt
  --check` plus routine-gate requirement is discharged by that one run, because
  `full` calls `quick`, which calls `run_guards()`.
- **Condition 3 — a commit carrying the milestone**, still open. Every figure
  here is a working-tree property of an uncommitted tree.

- **Risk.** Low on capability: the model suite is 236/236, the two dispatch tests
  pass individually, the corpus is 1227/0, and `quick` is green. Medium on the
  pruning rule, which is the one place the implementation chose a third option
  the plan did not enumerate.
- **Exact future evidence.** The `full` transcript labelled with the commit that
  carries M009, and the ratified pruning paragraph in the plan or roadmap.

M009 has no dependants — the roadmap records it as the lowest-value, droppable
milestone and the last in this line — so no later milestone is blocked by these
conditions.

## 12. Registry updates

- `plans/registry.md` line 223 (the Helix M009 row): status
  `**ready**` → `**conditionally closed**`, and add
  `plans/closure/tui-editor-helix-capability-parity/009-status.md` as the record.
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` line 603
  (the M009 row): `**ready for handoff**` → `**conditionally closed**`, closure
  record `plans/closure/tui-editor-helix-capability-parity/009-status.md`,
  blockers "owner ratification of the single-chain pruning rule; a commit
  carrying the milestone" (the green-`full` blocker was discharged 2026-11-13).
- `plans/registry.md` line 30 (the subsystem row): "M001–M005 implemented, M006
  in flight, M007–M009 pending" must become a status that names M009's record and
  its condition.
- `plans/implementation/tui-editor-helix-capability-parity/009-undo-tree.md`
  header: `Status: ready for handoff` → `Status: conditionally closed`. **This
  record did not make that edit**: `plans/closure/README.md` does not require it,
  and this session was scoped to `plans/closure/**` only. The M005 record set the
  precedent of recommending the edit rather than making it.
- Record the guard figures at this tree state: text authority **pass** with one
  advisory, doc drift **52 leaf gestures / 100 chords**, editor key authority
  **2 guard-block / 92 Normal-mode / 8 Insert-mode**, render baseline **61 / 2495
  / 0**. Doc drift and key authority moved since the M005 record (98 → 100
  chords, 90 → 92 Normal-mode); the delta is not attributable from this
  uncommitted tree, so do not record it against M009.
- Do **not** mark M009 closed on the strength of this record: conditions 1 and 3
  in §11 stand between conditional closure and closure.

## Owner decision (2026-11-14)

**Decision 7 of the seven, taken as A + C.**

- **A — the single-chain pruning rule is ratified as implemented.** Pruning an
  unbranched chain drops the **oldest node** and re-parents its one child
  (`prune_victim` at `editor.rs:7136`, `drop_node` at `:7164`). The plan's work
  package D said "an interior node is never dropped while a child survives"; that
  wording predates the `len == MAX_EDITOR_UNDO_DEPTH` linear-equivalence gate, and
  the implemented rule is the only one that keeps both the gate and a 257-edit
  history. The structural consequence is stated rather than hidden: the oldest
  surviving undo point is re-parented, so the structure above it survives and its
  identity does not.
- **C — the prune is disclosed.** The row now paints
  `· history trimmed at its bound` while the last edit pruned. It is **derived**,
  not latched: the prune sets `history_trimmed`, an edit that does not prune
  clears it, and `clear_history` clears it with the fallback flag. A latch was
  the failure mode the implementing lane rejected — a notice that keeps reporting
  a prune after the history stopped pruning — and silence was the other: a reader
  losing an undo point without being told. `a_prune_is_disclosed_while_it_happens_and_not_after`
  pins both directions.

## Correction log (2026-11-13)

- **§2, the `full`-only rows** — "`cargo fmt --check`, `clippy -D warnings`, the
  nextest sweep | not run" → discharged 2026-11-13 by `scripts/verify.sh full`,
  which ran all three.
- **§1, status reason 2** — "`scripts/verify.sh full` has not run" → it ran on
  2026-11-13 and passed: 17,377 workspace tests and 8,441 feature-pass tests, 0
  failures, `cargo fmt --check --all` and clippy `-D warnings` clean. The
  repository's "a green `quick` does not imply a green `full`" doctrine is still
  true; it is simply no longer the state of this tree.
- **§10, low finding (`full` not run)** — "**`scripts/verify.sh full` has not
  run.** `quick` is green (parent-run)" → discharged by the 2026-11-13 `full`
  run, which includes the clippy step, the nextest sweep and the bounded feature
  pass.
- **§11, condition 2** — "`full` has not run" → marked met 2026-11-13 with the
  discharge named.
- **§11, condition 1** — strengthened, not changed: the single-chain pruning
  rule is stated as **open and unratified**, with the plan's work-package-D rule
  named as the thing it contradicts. This record does not present it as settled.
- **§12, the roadmap blockers cell** — "green `full` on the merged tree" removed
  from the blocker list because it was discharged 2026-11-13; ratification and
  the commit remain.
- **Verified, not corrected — the traversal axes.** §1, §2 and §5 already state
  that `u`/`U` walk the branch and `Alt-u`/`Alt-U` walk creation order, and name
  the two tests that prove it: `u_walks_the_branch_and_never_crosses_the_fork`
  (`commands/editor.rs`) and
  `alt_u_crosses_the_fork_to_the_branch_the_edit_abandoned`
  (`commands/editor.rs`). Both exist in the tree. Nothing was changed for it.
- **Owner decision, 2026-11-14** — the single-chain prune ratified and the prune disclosed (`history_trimmed`, derived rather than latched); the record is `closed`.
