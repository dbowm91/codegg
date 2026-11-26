# Harness Practice Adoption Milestone 003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/003-present-turn-deliverables.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m003--present-declared-turn-deliverables`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. `src/tool/present.rs`,
  `src/tui/app/present_card.rs` and `tests/tui_render/present_deliverable.inc`
  are untracked additions; `src/tool/mod.rs`, `src/tool/factory.rs`,
  `src/permission/mod.rs`, `src/tui/app/render.rs` and
  `crates/codegg-core/src/snapshot/checkpoint.rs` carry uncommitted edits.
  Nothing below is attributable to a SHA.

The source plan and the roadmap's §12 status table have since been updated by
the parent: both now read `conditionally closed` for M003
(`plans/implementation/harness-practice-adoption/003-present-turn-deliverables.md:3`,
`plans/subsystems/harness-practice-adoption-roadmap.md:502`). This record states
the evidence; it does not edit either.

## 1. Executive finding

The tool is delivered and the claim it makes is **checked**, not asserted. A
declaration is verified against four facts — the path is contained in the
turn's workspace, the file exists, it is a regular file, and this turn wrote it
— and authorship comes from the durable edit-checkpoint record, never from the
global file-change stream and never from the model's own arguments.

`cargo test -p codegg --lib tool::present` runs **18 tests, all passing**, in
2.62 s. They cover every refusal the enum defines, the earlier-batch acceptance,
the different-turn refusal, the in-full refusal of a partial declaration, the
read-only classification, the restricted-child absence, and the writer-permit
store being untouched. `cargo test --test tui_render -- present_deliverable`
runs **4 tests, all passing**, in 0.06 s: the card lists exactly the declared
files in declared order, an absent declaration draws nothing, a refusal is a
failed call with no card, and the card is bounded.

Three things keep this from an unqualified `closed`, and all three are gaps
against the plan's own scope rather than doubts about the capability:

1. **Criterion 4's reopen half is untested.** The plan's §10 asks for "a
   declaration [that] resolves after a store reopen". Durability is structural —
   the checkpoint is a SQL table and `paths_written_for_turn` is a query — but
   no test reopens a store and declares. `src/tool/present.rs` contains no
   reopen or file-backed pool.
2. **The open-in-editor affordance is not built.** The plan's work package C
   requires "opening a row opens the file in the editor through the existing
   editor-open path". The card lists files; there is no row action.
3. **No `agent_loop` integration test exists**, though the plan's §11 names
   `cargo test --test agent_loop -- present`. The tool is tested in isolation and
   in the registry, not through a real turn.

The one deliberate reduction — `write` + `present` in the *same* batch yields
`not_written_this_turn` — made the tool **stricter** than the plan, not weaker.
It is recorded in §5 with the alternative it rejected.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A read-only tool with a bounded schema | `src/tool/present.rs`; `MAX_PRESENTED_FILES = 32`, `MAX_PRESENTED_PATH_LEN = 512`, `MAX_PRESENTED_DESCRIPTION_LEN = 200` | pass | Bounds appear in the JSON schema too (`maxItems`, `maxLength`) |
| Registered for the daemon's per-turn registry | `src/tool/factory.rs:357`; test `the_daemon_session_registry_registers_present` | pass | |
| Classified read-only | `src/permission/mod.rs:178` `"present" => ToolCategory::ReadOnly`; test `the_tool_is_read_only_and_absent_from_the_child_registry` | pass | |
| `outside_workspace` refusal | `a_path_outside_the_workspace_is_refused`; also `a_symlink_that_escapes_the_workspace_is_refused` | pass | Containment via `validate_path`, the single resolver |
| `missing` refusal | `a_missing_path_is_refused` | pass | |
| `not_a_regular_file` refusal | `a_directory_is_refused_as_not_a_regular_file` | pass | |
| `not_written_this_turn` refusal | `a_path_written_by_another_turn_is_refused`, `a_path_this_turn_did_not_write_is_refused`, `a_declaration_with_no_bound_turn_is_refused` | pass | |
| `too_many_files` refusal | `too_many_files_is_refused` | pass | |
| `path_too_long` refusal | `an_over_long_path_is_refused` | pass | |
| A partial declaration is refused **in full** | `a_partial_declaration_is_refused_in_full` — `delivered` is empty and the output does not contain `Presented` | pass | Criterion 3 |
| A file written in an **earlier batch of the same turn** is accepted | `a_path_written_in_an_earlier_batch_of_the_same_turn_is_accepted` | pass | |
| Authorship is the durable checkpoint, not the file-change stream | `paths_written_for_turn` in `crates/codegg-core/src/snapshot/checkpoint.rs:347-388`; the module doc at `src/tool/present.rs:12-20` states the reason | pass | |
| Authorship requires **`pre != post`**, not checkpoint membership | SQL predicate `(files.value ->> '$.pre') <> (files.value ->> '$.post')`; test `turn_write_evidence_names_the_turns_changed_paths_and_no_other_turns` uses a `captured_only("b.txt")` sibling and asserts only `a.txt` is returned | pass | The batch records every captured path, including siblings it did not change |
| A missing turn id is a refusal, not a permissive default | `turn_write_evidence` returns `Err("no turn is bound to this call, so authorship cannot be proven")`; test asserts the reason contains `no turn is bound` | pass | |
| A missing checkpoint store is a refusal | same function, `Err("no durable checkpoint store is attached to this call")` | pass | Fails closed on every arm |
| No table, migration, `CoreRequest` variant or projection field | No migration in `schema.rs` names `present`; the result rides the transcript row | pass | Criterion 6 |
| The TUI renders a turn-tail card from the tool result | `src/tui/app/present_card.rs`, `render.rs:260-272` | pass | |
| The card lists declared files in declared order and nothing for an absent declaration | `present_deliverable::the_card_lists_the_declared_files_in_declared_order`, `present_deliverable::a_session_with_no_declaration_renders_no_card` | pass | |
| A refusal renders as a failed call with its reason and no card | `present_deliverable::a_refused_declaration_is_a_failed_call_with_no_card` | pass | |
| The card is bounded and names how many it hides | `present_deliverable::the_card_is_bounded_and_says_how_many_files_it_hides`; `MAX_PRESENT_CARD_ROWS = 8` | pass | |
| A restricted agent is not offered the tool | `the_tool_is_read_only_and_absent_from_the_child_registry` | pass | Criterion 5 |
| The tool takes no writer permit | `a_declaration_leaves_the_writer_permit_store_untouched` | pass | |
| `check_writer_disposition.py` reports no new disposition | guard run: "writer-disposition check ok: 63 declared roots, 18 writer tools, 35 writer modules" | pass | No entry was added for `present` |
| The description is user-facing text, not a second claim | `MAX_PRESENTED_DESCRIPTION_LEN` bound plus the display round-trip tests (`the_display_round_trips_through_the_one_format`, `a_refusal_message_does_not_parse_as_a_declaration`) | pass | |

## 3. Production implementation evidence

**The tool.** `src/tool/present.rs` (46,825 bytes) implements `Tool` with
`ToolCategory::ReadOnly`, an `IdempotencyClass`/`ToolCachePolicy`/`ToolContract`
matching a read, and a structured result. It is constructed in
`src/tool/factory.rs:357` with the turn's workspace root, workspace id, session
id, an optional turn id and an optional `Arc<EditCheckpointManager>`.

**The three outcomes.** `verify` distinguishes `Verified`, `Refused` and
`Malformed` (`present.rs:487-502`) because they have different terminal
statuses and different structured values. A malformed declaration names no path
to refuse; a refused one carries typed per-path refusals.

**The refusal set.** `PresentRefusalKind` has exactly **six** kinds —
`outside_workspace`, `missing`, `not_a_regular_file`, `not_written_this_turn`,
`too_many_files`, `path_too_long` — each with a stable wire spelling persisted in
the tool result. The briefing for this record said "seven typed refusals were
observed"; I could not reproduce a count of seven from the tree. The enum has
six, and the suite exercises all six plus two refusals that are *not*
`PresentRefusalKind` values: a malformed/empty declaration, and an unbound turn
(which surfaces as `not_written_this_turn` with the reason `no turn is bound`).

**The authorship read.** `EditCheckpointManager::paths_written_for_turn`
(`crates/codegg-core/src/snapshot/checkpoint.rs:347`) projects the per-file
fields in SQL — so the pre/post content blobs are never materialised — and
filters with `(files.value ->> '$.pre') <> (files.value ->> '$.post')`. That
predicate is the whole point: a batch persists **every affected path it
captured**, including siblings whose state did not change, so membership in a
checkpoint is not authorship. Order is `(batch_seq, created_at, id)` then
position within the batch; `LIMIT` is `max_paths + 1` so the truncation flag can
be set without a second query.

**The structured result.** `PresentResult { delivered: Vec<PresentedFile>,
refused: Vec<PresentRefusal> }`, where a `PresentedFile` is `{ path,
description? }`. `render_present_display` emits a canonical text form and
`parse_present_display` reads it back, returning `None` for anything that is not
exactly that format — so a refusal message, a truncated capture or a
pre-`present` transcript renders no card rather than a guessed one.

**The frontend.** `src/tui/app/present_card.rs` owns `latest_declaration`,
`card_height` and `MAX_PRESENT_CARD_ROWS`; `src/tui/app/render.rs:260-272`
derives the card from the transcript on the `Route::Session` arm only, with the
comment "on the home, workspace and editor routes there is no turn tail to draw
it under". There is no second store of what was delivered.

**A defect the render test caught.** The card was originally drawn on every
route, including `Home`, where no transcript is painted — so the card appeared
over a surface that had nothing under it. The fix is the `Route::Session` gate
above. This is the milestone's clearest case of a test failing on a real defect
rather than on a style preference.

**Deliberate reduction, stricter than the plan.** `write` + `present` in the
**same** batch yields `not_written_this_turn`. The alternative was to treat the
model's own `write` arguments in the same batch as authorship evidence, which
would make the declaration check itself. The cost is that a model that writes
and presents in one batch is refused and must present in a later batch; the
benefit is that authorship is never a claim about a claim.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib tool::present
cargo test --test tui_render -- present_deliverable
python3 scripts/check_writer_disposition.py
python3 scripts/check_comment_budget.py
```

### Results

- `cargo test -p codegg --lib tool::present` — **18 passed, 0 failed**, 7,996
  filtered out, 2.62 s. The eighteen: `a_declaration_of_a_file_this_turn_wrote_is_recorded`,
  `a_path_written_in_an_earlier_batch_of_the_same_turn_is_accepted`,
  `a_path_written_by_another_turn_is_refused`,
  `a_path_this_turn_did_not_write_is_refused`,
  `a_path_outside_the_workspace_is_refused`, `a_missing_path_is_refused`,
  `a_directory_is_refused_as_not_a_regular_file`,
  `a_symlink_that_escapes_the_workspace_is_refused`, `too_many_files_is_refused`,
  `an_over_long_path_is_refused`, `a_partial_declaration_is_refused_in_full`,
  `a_declaration_with_no_bound_turn_is_refused`,
  `an_empty_declaration_is_malformed`,
  `a_declaration_leaves_the_writer_permit_store_untouched`,
  `the_tool_is_read_only_and_absent_from_the_child_registry`,
  `the_daemon_session_registry_registers_present`,
  `the_display_round_trips_through_the_one_format`,
  `a_refusal_message_does_not_parse_as_a_declaration`.

  This run initially failed to compile — twice — with
  `no method named trim found for struct ratatui::prelude::Line` at
  `src/tui/components/dialogs/connect.rs:809,961,1333,1485`. That file is not
  this milestone's; it was last modified at 15:52 by a sibling lane and
  compiled on the retry at ~15:55.
- `cargo test --test tui_render -- present_deliverable` — **4 passed, 0 failed**,
  1,217 filtered out, 0.06 s, after an 8 m 23 s build:
  `the_card_lists_the_declared_files_in_declared_order`,
  `a_session_with_no_declaration_renders_no_card`,
  `a_refused_declaration_is_a_failed_call_with_no_card`,
  `the_card_is_bounded_and_says_how_many_files_it_hides`.

  This run first failed to compile in a **different** sibling file —
  `tests/tui_render/tui_editor_cost.inc:83`,
  `error[E0063]: missing field count_label in initializer of EditorWidget` — and
  then spent most of its wall time in
  `Blocking waiting for file lock on build directory` while sibling agents
  rebuilt the same workspace. Both are the TUI lane's in-flight work, not this
  milestone's, and both cleared.
- `check_writer_disposition.py` — green: "writer-disposition check ok: 63
  declared roots, 18 writer tools, 35 writer modules". `present` required no new
  entry, which is the classification the plan predicted.
- `check_comment_budget.py` — **green in the 2026-11-13 `scripts/verify.sh full`
  run**; none of the 44 violations this bullet recorded were in
  `src/tool/present.rs` or `src/tui/app/present_card.rs`, and a standalone re-run
  shows no violation in either file. The corpus total is banked at 139,245
  lines. (At correction time the guard's remaining red traces to WP D's
  in-flight `diff.rs`/`baseline.rs`, including the corpus totals — not to this
  milestone's files.)

### Not run

- `cargo test -p codegg --lib -- snapshot::checkpoint`: the relevant tests live
  in `codegg-core`, and the one that matters
  (`turn_write_evidence_names_the_turns_changed_paths_and_no_other_turns`) was
  read and confirmed by inspection, not executed here.
- `cargo test --test agent_loop -- present`: **not run**, and — see §10 — the
  agent-loop integration test does not exist yet.
- A store-reopen declaration test: **not run, because it does not exist.** The
  plan's §10 asks for it; the suite creates one in-memory pool per `Turn` and
  never reopens one.
- `cargo fmt --check`, `cargo clippy -D warnings`, `scripts/verify.sh quick`: not
  run in the authoring session. **Superseded 2026-11-13:** `scripts/verify.sh
  full` ran on this tree and passed — fmt clean, clippy `-D warnings` clean, the
  workspace sweep green; `full` includes `quick`, which includes every guard.
  The 44-violation comment-budget state and the M001 corpus-guard failure
  recorded here are gone.

## 5. Invariant review

- **`present` holds no write authority.** It reads metadata and the checkpoint
  record; `a_declaration_leaves_the_writer_permit_store_untouched` asserts the
  permit store is untouched, and the tool's contract class is `ReadOnly`.
- **Path containment has one authority.** Every model-supplied path goes through
  `validate_path` against the turn's workspace root; `normalise_relative` refuses
  `ParentDir`, `RootDir` and `Prefix`, so a `..` escape or an absolute path never
  reaches the resolver. The symlink test covers a link inside the workspace
  pointing outside.
- **Authorship evidence is the durable checkpoint, never the global stream.**
  `paths_written_for_turn` queries `edit_checkpoint`; no code path touches
  `AppEvent::FileChanged`.
- **The declaration cannot make a false claim.** One bad entry refuses the whole
  declaration (`verify`, `present.rs:529-534`); the tool never reports a partial
  success.
- **The record is the tool result.** No table, no second store, no projection
  field. The card reads the transcript.
- **The tool is bounded.** 32 files, 512-byte paths, 200-character descriptions;
  `too_many_files` and `path_too_long` are typed refusals, and the empty
  declaration is `Malformed` rather than a silent success.
- **Restricted subagents do not get it.**
  `the_tool_is_read_only_and_absent_from_the_child_registry` asserts the child
  registry does not offer it.
- **`pre != post`, not checkpoint membership.** Tested in `codegg-core`, and
  the reason is documented on the method.

## 6. Failure and recovery review

- **A file written and then deleted in the same turn** — `missing`: the claim is
  about a file the user can open now.
- **A file written, then the turn cancelled** — the declaration never runs and no
  card is drawn; the checkpoint remains, which is correct because the turn did
  write it.
- **A path that is a directory** — `not_a_regular_file`.
- **A symlink inside the workspace pointing outside** — `outside_workspace`,
  because the resolver checks the resolved target.
- **A turn with no checkpoint record** — every entry is `not_written_this_turn`;
  the tool does not fall back to a weaker check.
- **An unreadable store** — `turn_write_evidence` returns `Err`, and each entry
  is refused; the tool never treats "could not read" as "nothing was written".
- **Restart** — the checkpoint store is durable, so a declaration in a later
  process still resolves; the card renders from the durable transcript.
- **Contention** — the tool reads and takes no permit, so it cannot deadlock with
  a writer. Two `present` calls in one turn are independent; the test suite does
  not exercise two calls explicitly.
- **`write` + `present` in one batch** — `not_written_this_turn`, as designed.
  **No test pins this**, because the tests persist their checkpoint rows
  directly (`Turn::record` → `persist_checkpoint`) rather than going through the
  real `tool_batch` boundary. The behaviour follows from the code, and this
  record states it as a consequence rather than as a measured result.

## 7. Migration and compatibility review

- **No migration, no schema change, no protocol change, no configuration key,
  no dependency.** The result rides the transcript row the tool call already
  produces.
- The tool is additive: an older transcript with no `present` call renders
  exactly as it does today, because `parse_present_display` returns `None` for
  anything that is not the canonical form.
- The result shape is frontend-neutral, so a second frontend can render it
  without a protocol change; there is no desktop card, which the plan excluded
  on purpose.
- `PresentResult`, `PresentRefusalKind` and `PresentRefusal` derive
  `Serialize`/`Deserialize`, so the structured value a stored row carries
  decodes back to the same type — that is a *tool-result* contract, not a
  protocol variant.

## 8. Security review

- Containment is `validate_path` plus a lexical normaliser that refuses anything
  that is not a plain relative descent. An absolute path, a `..` escape and an
  escaping symlink are each refused.
- The tool requires no capability beyond reading the metadata of a file inside
  the workspace it was constructed for, and it holds no writer permit.
- Bounds: 32 entries, 512-byte paths, 200-character descriptions, and a bounded
  checkpoint read (`MAX_TURN_WRITE_EVIDENCE`) whose truncation is reported.
- The description is rendered as text after escaping line breaks, so a path with
  a newline cannot forge a second entry in the display; `parse_present_display`
  requires the declared count to match the parsed rows.
- No secret surface: the tool prints paths and model-supplied descriptions, and
  never file content.

## 9. Documentation and operations

- `architecture/tool.md` — modified in the working tree (it is in `git status`);
  `present`'s contract, its refusals and its authorship evidence are the
  reported additions.
- `architecture/tui.md` — modified in the working tree; the turn-tail card and
  the fact that it renders a tool result.
- `architecture/tool-catalog.md` — `present` is **not** a row, because the
  generator's four declared `ToolRegistryOptions` matrices cannot construct it:
  the catalog's "Non-claims" section names `src/tool/present.rs` explicitly as a
  tool the daemon's per-turn registry adds and a `ToolRegistryOptions` value
  cannot express. That is a recorded exclusion, not an omission.
- No new guard. `check_writer_disposition.py` was run to confirm the read-only
  classification, as the plan required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The open-in-editor affordance the plan's work package C required (`opening a row opens the file in the editor through the existing editor-open path`) is not delivered | The card lists files but a reader cannot open one from it — a plan-required, user-visible part of the card is absent | Implement the row action, or amend the plan to drop it |
| medium | No store-reopen test exists, though the plan's §10 requires one | Criterion 4's durability half is structural, not proven: a regression that read a non-durable cache would pass every present test | Add a test with a file-backed pool that declares after dropping and reopening the manager |
| medium | No `agent_loop` integration test exists | The tool is tested in isolation and in the registry; no test proves a real turn's `write` → checkpoint → `present` sequence end to end | Add one member to `tests/agent_loop` driving a batch that writes and then presents in a later batch |
| low | `write` + `present` in one batch yields `not_written_this_turn` and **no test pins it** | A future change to the batch's checkpoint timing could silently make the behaviour succeed, without a test objecting | Add a test at the real `tool_batch` boundary for the same-batch case |
| low | The `tool_catalog` family cannot see `present`, so adding this tool moved no catalog row | A reader of the catalog alone would not learn the tool exists | Either extend the catalog matrix with the daemon per-turn surface, or keep the non-claim (currently recorded) |
| resolved (was low) | The roadmap §12 table and the plan's own `Status:` line said `not started` / `ready for handoff` | The roadmap understated delivered work | Parent updated both: the roadmap row now reads `conditionally closed` (`plans/subsystems/harness-practice-adoption-roadmap.md:502`) and the plan's line 3 matches |

## 11. Roadmap disposition

**Conditionally closed.** Five of the six acceptance criteria are met outright;
the sixth is met in part:

1. A declaration of a file this turn wrote succeeds **and is rendered at the
   turn's tail** — met, by `a_declaration_of_a_file_this_turn_wrote_is_recorded`
   and the four `present_deliverable` card tests.
2. Outside/missing/directory/another-turn each fail with a distinct typed error
   naming the path — met, with a dedicated test per shape.
3. A declaration with one bad entry produces no partial success — met.
4. The result survives a store reopen and renders from the transcript — the
   "renders from the transcript" half is met (`a_session_with_no_declaration_renders_no_card`
   and the display round-trip); **the reopen half is untested**.
5. A restricted agent's registry does not offer the tool — met.
6. No table, migration, `CoreRequest` variant or projection field — met, by the
   absence of any such change in `schema.rs` and in the protocol crates.

Conditions, each with the exact future evidence:

- **A store-reopen test.** Expected shape: a file-backed pool, a declaration, drop
  the manager, reopen, declare again, assert the file is still authored. Command:
  `cargo test -p codegg --lib tool::present`.
- **The open-in-editor affordance**, or an owner decision to drop it from the
  plan.
- **An `agent_loop` integration test.** Command after it exists:
  `cargo test --test agent_loop -- present`.

None of the three leaves the tool able to make a false authorship claim, which is
the property the milestone exists to establish. The 2026-11-13 green
`scripts/verify.sh full` does not discharge them: it re-ran the existing unit
and card members, and no store-reopen test, editor row action or `agent_loop`
`present` member exists to run.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M003 row: `not started` → `conditionally closed`, closure record
  `plans/closure/harness-practice-adoption/003-status.md`, blocker column naming
  the missing store-reopen test and the two unbuilt follow-ups. **Done by the
  parent:** the roadmap row now reads `conditionally closed`
  (`plans/subsystems/harness-practice-adoption-roadmap.md:502`).
- The registry's milestone table entry for M003, if `plans/registry.md` carries
  one.
- **Not updated by this record:** the source plan's `Status:` line; the parent
  has since changed it to `conditionally closed`
  (`plans/implementation/harness-practice-adoption/003-present-turn-deliverables.md:3`).

## Correction log (2026-11-13)

The gate-class discharge cited below is the 2026-11-13 `scripts/verify.sh full`
run on this tree: 17,377 workspace tests and 8,441 feature-pass tests, 0
failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean. `full`
includes `quick`, which includes the guards.

- **Preamble** — "the source plan still reads `ready for handoff`, and the
  roadmap … still reads `not started`": superseded by the parent's updates; both
  now read `conditionally closed` (plan:3, roadmap:502).
- **§4 results, comment budget** — "red, exit 1, 44 violations": the guard was
  green in the 2026-11-13 `full` run; none of the former violations were in this
  milestone's two files, and the corpus is banked at 139,245 lines.
- **§4 "Not run" (fmt, clippy, `quick`)** — superseded by the 2026-11-13 `full`
  run, which ran all three and passed; the 44-violation comment-budget state and
  the M001 corpus-guard failure named there are gone.
- **§10 roadmap/plan row** — parent updated both, retained above.
- **§12 registry bullets** — corrected to record the parent's update.

**Residue kept open, deliberately:** the store-reopen test (§10, medium), the
open-in-editor affordance (§10, medium), the `agent_loop` integration test
(§10, medium), the unpinned same-batch behaviour (§10, low), and the catalog
non-claim (§10, low). Verified still open on 2026-11-13: `src/tool/present.rs`
has no reopen/file-backed-pool test, `src/tui/app/present_card.rs` has no editor
action, and `tests/agent_loop/` has no `present` member. None is discharged by
the 2026-11-13 run.
