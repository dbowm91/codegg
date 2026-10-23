# Repository Audit Corrective Milestone 003 — Closure Status

Status: implemented; closure record pending

Source implementation plan:

- `plans/implementation/repository-audit-corrective/003-slash-command-argument-contract.md`

Source subsystem roadmap:

- `plans/subsystems/repository-audit-corrective-roadmap.md`

Repository baseline reviewed: `8590c17c5` (branch `fix/c003-audit-corrective`)

Implementation commit:

- `85cfeea3d` — fix(tui): extract the slash-command argument at the registry
  entry point

## 1. Executive finding

The milestone's capability is implemented, and **the plan's defect set was
re-derived from the code and is wider than the plan recorded**: the plan
named six affected commands, and the re-derivation found **twelve**. The
plan's §14 stop condition ("the extraction would change a command's
behaviour beyond the six confirmed commands") therefore fired during
execution. It was handled by re-deriving the scope, recording the delta
below, and shipping the re-derived scope — the mechanism the milestone
mandates (a single extraction point) cannot be delivered without it: any
extraction that leaves `/loop` parsing the command name as the duration is
not "the argument is extracted once".

What shipped:

* **One extraction point.** `Command::argument_from_typed_line`
  (`src/tui/command.rs:421`) removes the argument from the typed line, and
  `App::execute_command` (`src/tui/app/mod.rs:5489`) is the only caller that
  hands it to the built-in dispatcher. The dispatcher parameter is the
  argument, not the line.
* **The spelling the user typed is what is removed** — the entry's
  canonical name *or one of its aliases*. `/agent` and `/agents` each lose
  their own token.
* **No arm reads the palette's query any more.** The palette's `query` is
  the whole typed line; a handler that read it re-derived the argument by
  hand, and that is how `/search foo` searched for the literal
  `"/search foo"`.
* **`Dialog::is_open` covers every variant.** `PanelManager` and `Control`
  were omitted; the predicate now names all 52 variants that can be on
  screen (`Dialog::None` is the absence of a dialog and is deliberately not
  listed).
* **A static guard** (`scripts/check_command_argument_contract.py`) fails
  when an arm reads the palette's query and when a dialog variant is
  missing from the predicate. It is wired into `scripts/verify.sh quick`
  and into CI, and it self-tests.

## 2. Scope discovery — the plan's six, and the six the plan missed

The plan's §3 records six arms that read the palette's query raw. The
re-derivation (WP1) enumerated **all 161 arms** and classified each by the
idiom it used. Twelve arms received an argument that still carried the
command's name; the plan's six are among them.

| # | Arm | Command (spellings) | Defect class | Plan |
|---|---|---|---|---|
| 1 | `B::Search` | `/search` | whole line used as the query | plan's six |
| 2 | `B::TaskDel` | `/task-del` | whole line used as the id | plan's six |
| 3 | `B::MemorySearch` | `/memory-search` | whole line used as the query | plan's six |
| 4 | `B::MemoryRemember` | `/memory-remember` | whole line used as the text | plan's six |
| 5 | `B::MemoryForget` | `/memory-forget` | whole line used as the id | plan's six |
| 6 | `B::HabitDismiss` | `/habit-dismiss` | whole line used as the id | plan's six |
| 7 | `B::OpenDocument` | `/open` | whole line used as the path | **discovery** |
| 8 | `B::MemoryList` | `/memory-list` | whole line used as the query | **discovery** |
| 9 | `B::Habits` | `/habits` | whole line compared against `ready` | **discovery** |
| 10 | `B::SecurityReview` | `/security-review` | whole line handed to the flag parser | **discovery** |
| 11 | `B::Loop` | `/loop` | splits the line and takes the **first** part — the command name — as the duration | **discovery** |
| 12 | `B::Review` | `/review-change` (alias `/preview-review`) | strips `"/review "`, a spelling that is **not its own** | **discovery** |

Two of the plan's claims were falsified by the re-derivation:

* **"`/loop` splits the line and takes the second part"** — it takes the
  second part as the *message* and the first part as the *duration*, and
  the first part is the command name. `parse_duration("/loop")` is `None`,
  so `/loop 5m "check status"` always answered *"Invalid interval"* and
  never scheduled anything.
* **"the confirmed count is six"** — the whole-line class holds ten arms,
  and two further arms are defective in their own idiom (the wrong half of
  the split, and a strip against a spelling that is not the command's).
  The six the plan names are the six whose *visible* defect the earlier
  survey happened to catch; they are not the six that are defective.

`/security-review` is defective by the contract but its **observable result
does not change**: `parse_security_review_args` ignores unknown tokens, so
the leading `/security-review` was already inert. It is migrated because the
contract is about what the handler receives, not about what it happens to
print.

## 3. WP1 — the derivation over all 161 arms

The derivation is reproducible: `scripts/check_command_argument_contract.py`
uses `scripts/rust_test_scan.py` to find the dispatcher and its arms, and
the table below was produced by the same scanner over the baseline commit
(`git show 8590c17c5:src/tui/app/mod.rs`) and over the landed tree. Both
trees carry **161 arms**; the arm set did not change.

| Idiom (baseline) | Arms | Meaning |
|---|---|---|
| `NONE` | 69 | the arm ignores the argument entirely |
| `STRIP` | 49 | the arm strips a literal spelling off the line |
| `TOKEN` | 31 | the arm splits the line at the first space and takes the rest |
| `WHOLE-LINE-PASS` | 2 | the arm hands the line to a helper that strips the name (`SubagentSend`, `Themes`) |
| `WHOLE-LINE-ARG` | 10 | the arm binds the whole line and uses it as the argument |

After the change every arm of the two reading idioms takes the extracted
argument; the `NONE` arms are untouched.

**Alias coverage.** The plan required that every `STRIP` arm strip *every*
spelling of its command. The re-derivation checked each `STRIP` arm's
strip literals against its registry entry's name and aliases. Exactly one arm
failed: `B::Review` strips `"/review "`, while the entry answers to
`/review-change` and `/preview-review` — so the candidate id was **always**
empty and `/review-change <id>` never worked. Every other `STRIP` arm's
literals cover all of its entry's spellings (the multi-spelling arms —
`Approval`/`Sandbox`, the `LspPreview*` family — strip both the canonical
name and the alias).

**Re-derivation after the change.** The landed tree has the same 161 arms
and the same `NONE`/`STRIP`/`TOKEN` split *by construction*; the difference
is that the 92 arms that read an argument now read the extracted one:

| Idiom (landed) | Arms | Meaning |
|---|---|---|
| `NONE` | 69 | unchanged |
| `ARGUMENT` | 92 | the arm consumes the extracted argument |

## 4. Production implementation evidence

### 4.1 The extraction point

`Command::argument_from_typed_line` (`src/tui/command.rs:421`) splits the
trimmed typed line at its first whitespace-delimited token and removes that
token. The token is resolved against the entry's canonical name **and its
aliases** (the method walks `all_names()`), so `/theme` loses `/theme` and
`/themes` loses `/themes` — each entry loses its own spelling. The
comparison is the registry's own (`normalize_name`: a leading slash and the
case are not significant). A token that names no entry of the command is not
an error: the palette executes the highlighted row, which is not always the
token the user typed, and the argument still starts after the token.

`App::execute_command` (`src/tui/app/mod.rs:5489`) is the only caller. It
resolves the typed line to a registry entry, extracts the argument once, and
passes it to `dispatch_builtin_command`. The dispatcher's parameter is
renamed `argument` and is rebound once at the top of the function to the
trimmed argument, so `None` and `Some("")` mean the same thing.

### 4.2 The 92 migrated arms

Every arm that read the palette's query or the `raw_input` parameter now
consumes the extracted argument:

* the ten `WHOLE-LINE-ARG` arms dropped their own copy of the line;
* the 49 `STRIP` arms dropped their `strip_prefix` / `trim_start_matches`
  chain — the spelling is already gone, and dropping the chain is what
  removes the alias-blind strips (`B::Review`) and the wrong-half splits
  (`B::Loop`);
* the 31 `TOKEN` arms dropped their `split_once(' ')`;
* the two `WHOLE-LINE-PASS` arms now pass the argument to their helper, and
  the helpers (`App::handle_theme_command`,
  `commands::subagent_steer::dispatch_addressee_command`) stopped
  re-deriving it.

### 4.3 The commands whose behaviour changed

Eleven commands' behaviour changed; the twelfth (`/security-review`) is
mechanism-only.

| Command | Before | After |
|---|---|---|
| `/search` | searched for `"/search needle"` | searches for `"needle"` |
| `/task-del` | deleted the task `"/task-del 5"` | deletes the task `"5"` |
| `/memory-search` | searched for `"/memory-search needle"` | searches for `"needle"` |
| `/memory-remember` | stored `"/memory-remember …"` | stores `"…"` |
| `/memory-forget` | forgot the id `"/memory-forget habit-7"` | forgets `"habit-7"` |
| `/habit-dismiss` | dismissed the habit `"/habit-dismiss habit-7"` | dismisses `"habit-7"` |
| `/open` | always refused the path as absolute (`/open src/lib.rs`) | opens `src/lib.rs` |
| `/memory-list` | answered *"Unknown memory command: /list"* | answers with the memory summary |
| `/habits` | the `ready` filter never matched | `ready` filters the list |
| `/loop` | always answered *"Invalid interval"* | schedules the interval and the message |
| `/review-change` | always answered *"Usage: /review <preview-id>"* | passes the candidate id to the reviewer |
| `/security-review` | the flags parsed the same | unchanged (the parser ignored the name) |

The set matches the WP1 derivation exactly: the twelve arms of §2, of which
eleven have an observable consequence.

### 4.4 The reconciled call sites

* The dispatcher parameter `raw_input` → `argument` (92 arms).
* `App::handle_theme_command` takes `&str` (the argument) instead of
  `Option<&str>` (the line). Its five test call sites now pass the
  argument (`"list"`, `"use catppuccin-latte"`, `"use nope"`, `"reload"`).
* `commands::subagent_steer::dispatch_addressee_command` takes `&str` (the
  argument); its internal `strip_prefix("/subagent-send")` is gone.
* **Six** policy-selector tests set `command_palette.query` to a full
  command line and called the dispatcher with `None`. That assignment is
  dead under the new contract, so it was removed; the one test that needed
  an argument now passes it (`Some("interactive")`), and the alias test now
  drives the registry (`find_by_name_or_alias("approval-mode")`) so the
  alias is exercised end to end.
* `App::handle_slash_command`'s sync of the palette's query is **kept**: the
  palette's own filter uses that field, and the milestone does not change
  the palette's completion behaviour. No dispatcher arm reads it any more.

### 4.5 `Dialog::is_open`

`src/tui/app/types.rs` — the predicate now lists every variant that can be
on screen, including `PanelManager` and `Control`.

## 5. `Dialog::is_open` — the callers and their dispositions

Every caller of `ui_state.dialog.is_open()` was reviewed before the
predicate was widened. The invariant that carries the review: **`ui_state.
dialog` is a mirror of the focus stack, and it names a dialog only while
the stack holds it** — `push_dialog` sets it only on a successful push, and
`close_dialog` and the render-panic recovery re-derive it from the stack's
top. So `ui_state.dialog ∈ {PanelManager, Control}` implies the focus stack
is non-empty.

| # | Call site | What the predicate means there | Disposition |
|---|---|---|---|
| 1 | `src/tui/app/render.rs:903` (`active_context_indicator`) | "a dialog is on screen" | **no change.** The focus manager's `active_dialog_type()` is consulted first and already reports the panel manager and the control box; the predicate's arm is only reached with an empty focus stack. |
| 2 | `src/tui/app/mod.rs:1447` (`build_remote_snapshot`) | "name the dialog on screen" | **accepted change.** The snapshot's `dialog` field now names `PanelManager` / `Control` when they are on screen. The field is informational — `codegg-client` does not branch on it — and the old value was wrong: it said "no dialog" while a box was on screen. |
| 3 | `src/tui/app/mod.rs:3933` (`on_key`'s debug log) | logging | **no change.** The logged flag becomes accurate. |
| 4 | `src/tui/app/mod.rs:3978` (`on_key`'s modal short-circuit) | "a modal dialog owns the key" | **no change.** The branch is reachable only with an empty focus stack, and the two added variants imply a non-empty one. The branch's own `focus_manager.is_empty()` guard (which logs the inconsistency and clears the dialog) is unchanged. |
| 5 | `src/tui/app/mod.rs:8865` (`on_paste`) | "no box owns the screen" | **accepted change.** A paste no longer reaches the prompt behind an open panel manager or control box. Key routing already sent every key to the focus stack while it was non-empty, so the paste now agrees with the keys. The paste is **not** routed into the box — that would be a new capability, outside this milestone. |
| 6 | `src/tui/app/mod.rs:9743` (`cancel`) | "a dialog is open" | **no change.** Reachable only with an empty focus stack. |
| 7 | `src/tui/app/mod.rs:9784` (`force_quit`) | same | **no change.** Reachable only with an empty focus stack. |
| 8 | `src/tui/app/mod.rs:9850` (`scroll_viewport_up`) | "the dialog owns scrolling" | **no change.** Reachable only with an empty focus stack. |
| 9 | `src/tui/app/mod.rs:9858` (`scroll_viewport_down`) | same | **no change.** Reachable only with an empty focus stack. |
| 10 | `src/tui/app/mod.rs:9866` (`go_to_top`) | "a dialog is open" | **no change.** Reachable only with an empty focus stack. |
| 11 | `src/tui/app/mod.rs:9872` (`go_to_bottom`) | same | **no change.** Reachable only with an empty focus stack. |
| 12 | `src/tui/app/mod.rs:12284` (`on_char`) | "a dialog owns typing" | **no change.** `on_char` is reached from the `TuiMsg::CharInput` path; the key path hands the key to the focus stack first, so the two variants are unreachable here. |

Two dispositions accept a change (2 and 5); both are the predicate becoming
true rather than a behaviour being traded away. Ten are unchanged, each for
a stated reason.

## 6. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One extraction point | `Command::argument_from_typed_line`; `App::execute_command` | pass | the only caller of the extraction is the registry-entry resolution |
| The argument excludes the command name and its alias | `src/tui/command.rs:421`; `resolves_spelling` | pass | `/theme` and `/themes` each lose their own token |
| Handlers receive only the argument | 92 migrated arms; the guard | pass | no arm reads `command_palette.query` |
| The affected set is re-derived from the code | §2, §3 | pass | 12 arms, not 6 |
| One test per affected command | `src/tui/app/command_argument_tests.rs` | pass | 13 behavioural tests (12 fail on the baseline: the 11 affected commands plus the `/preview-review` alias; the `/theme` alias guard passes on the baseline) and 3 extraction tests in `src/tui/command.rs` |
| `Dialog::is_open` covers every variant | `src/tui/app/types.rs:100`; the guard | pass | `PanelManager` and `Control` added |
| The `is_open` callers are reviewed | §5 | pass | 12 callers, 2 accepted changes |
| A guard with a self-test | `scripts/check_command_argument_contract.py` | pass | 9 self-test cases |
| The guard is wired into `verify.sh` and CI | `scripts/verify.sh`; `.github/workflows/ci.yml` | pass | both the self-test and the scan |
| The architecture doc records the contract | `architecture/tui.md` §"The slash-command argument contract" | pass | links the guard; does not restate it |
| No key binding changed | `check_tui_composer_key_authority.py` | pass | 5 chords, unchanged |

## 7. Verification executed

### Commands run

```bash
CARGO_BUILD_JOBS=3 cargo check -p codegg --all-targets
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib command_argument
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib tui::app::command_argument_tests
python3 scripts/check_command_argument_contract.py --self-test
python3 scripts/check_command_argument_contract.py
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
cargo fmt
scripts/verify.sh quick
```

### Results

#### 7.1 The baseline capture — every new test against the unfixed tree

The production files were reverted to the baseline commit (`8590c17c5`) while
the new test file was kept, and the narrow test was run:

```bash
# production files reverted, the new test file kept
git checkout -- src/tui/app/mod.rs src/tui/command.rs src/tui/app/types.rs \
                 src/tui/commands/subagent_steer.rs
# the test module's two-line declaration re-added (test scaffolding, not
# production code) so the new file is compiled and actually runs
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib command_argument
```

Result: **12 failed, 1 passed**. The captured assertion messages are the
production symptoms, verbatim:

| Test | Captured baseline failure |
|---|---|
| `search_receives_the_argument_without_the_command_name` | `assertion left == right failed: the transcript search must run on the argument, not on the typed line` — `left: Some("/search needle")`, `right: Some("needle")` |
| `task_del_receives_the_id_without_the_command_name` | `the delete must carry the id, not the typed line: DeleteTask { id: "/task-del 5" }` |
| `memory_search_receives_the_query_without_the_command_name` | `the memory search must carry the query, not the typed line: MemorySearch { query: "/memory-search needle" }` |
| `memory_remember_receives_the_text_without_the_command_name` | `the memory write must carry the text, not the typed line: MemoryRemember { text: "/memory-remember remember the milk" }` |
| `memory_forget_receives_the_id_without_the_command_name` | `the memory forget must carry the id, not the typed line: MemoryForget { id: "/memory-forget habit-7" }` |
| `memory_list_receives_no_query_argument` | `the list sub-command must be recognised: ["Unknown memory command: /list"]` |
| `habit_dismiss_receives_the_id_without_the_command_name` | `the dismissal must carry the habit id, not the typed line: HabitDismiss { id: "/habit-dismiss habit-7" }` |
| `habits_receives_the_ready_filter_without_the_command_name` | `the ready filter must survive the extraction: HabitList { ready_only: false }` |
| `open_receives_the_path_without_the_command_name` | `the path must not carry the command name: ["Cannot open document: path must be workspace-relative, not absolute"]` |
| `loop_receives_the_interval_without_the_command_name` | `the handler queued a command: Empty` (the baseline parsed `/loop` as the interval and scheduled nothing) |
| `review_change_receives_the_id_without_the_command_name` | `the candidate id must reach the review handler: ["Usage: /review <preview-id>"]` |
| `review_change_alias_receives_the_same_id` | `the alias must resolve to the same command and lose its own token: ["Usage: /review <preview-id>"]` |
| `theme_alias_receives_the_subcommand` | **passed** — `/theme` was already correct; this test is the alias regression guard, not an affected-command test |

The three extraction tests in `src/tui/command.rs` exercise a method that
does not exist on the baseline, so they cannot compile there; the baseline
evidence for the contract is the capture above plus the guard's self-test.

#### 7.2 The landed tree

| Command | Result |
|---|---|
| `CARGO_BUILD_JOBS=3 cargo check -p codegg --all-targets` | pass — no errors (the warnings are pre-existing and untouched by this change) |
| `CARGO_BUILD_JOBS=3 cargo test -p codegg --lib command_argument` | pass — 13 passed, 0 failed |
| `CARGO_BUILD_JOBS=3 cargo test -p codegg --lib argument_from_typed_line` | pass — 3 passed, 0 failed |
| `python3 scripts/check_command_argument_contract.py --self-test` | pass — 9 cases: the real dispatcher is clean; an injected arm that reads the palette query is rejected and named; a synthetic violating dispatcher is rejected; a dispatcher that reads only the argument is accepted; the real `is_open` is complete; a dropped variant is reported; a synthetic unlisted variant is reported; `Dialog::None` is not required |
| `python3 scripts/check_command_argument_contract.py` | pass — the dispatcher reads no palette query and `Dialog::is_open` covers every variant |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | pass — every violation class detected and rejected |
| `python3 scripts/check_tui_composer_key_authority.py` | pass — the claim table is closed, single-owner, and every caret motion is bound the same way in both maps |
| `python3 scripts/check_tui_editor_key_authority.py` | pass — **2 guard-block chord(s), 72 Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm** |
| `cargo fmt` / `cargo fmt --check` | pass — the tree is formatted (rustfmt, max_width 100) |
| `scripts/verify.sh quick` | pass — *"Quick verification passed."* (rustfmt, the two generated-asset checks, the 55 guard scripts including the new one and its self-test, and the workspace check) |

#### 7.3 The key-authority chord counts, confirmed unchanged

Both key-authority guards read files this milestone did not touch, so their
recorded counts are the baseline's by construction — and they were **run**,
not assumed:

* `check_tui_composer_key_authority.py` reads the two binding maps in
  `src/tui/input.rs` (untouched). Its chord model is
  `CHORD_BOUND_ACTIONS` = 4 respelled (`ToggleAgentRun`, `OpenDiff`,
  `FocusPrompt`, `FocusSidebar`) + 1 chord-only (`ExpandAllDisclosures`) =
  **5 chords**, unchanged. The guard passes.
* `check_tui_editor_key_authority.py` reads `src/tui/commands/editor.rs`
  (untouched). It reports **2 guard-block chords, 72 Normal-mode chords,
  8 Insert-mode chords, no shadowed arm** — the same counts as the
  baseline, because the file it reads did not change.

No key binding was added, removed or renamed by this milestone: the
dispatcher is the only thing that changed, and a command's key binding lives
in the input maps.


## 8. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| The palette's own completion and validation behaviour is unchanged | the palette's `query` field keeps its meaning and its sync from `handle_slash_command` is untouched | pass |
| No command is added, removed, renamed or re-aliased | the registry is untouched; `built_in_command_count_matches_release_docs` is unchanged | pass |
| The `/lsp-*` commands' results are unchanged | their arms received the same values: the strip literals covered both spellings, so the extracted argument equals the stripped value | pass |
| A command with no argument behaves as today | the 69 `NONE` arms are untouched; the 12 migrated arms now see an empty argument, which is the fix the milestone exists for | pass |
| A command dispatched with an explicit argument keeps working | `dispatch_builtin_command(action, None)` still compiles and behaves as "no argument"; `Some(argument)` carries one | pass |
| No key binding changes | the composer key-authority guard's chord model is unchanged (5 chords) | pass |
| The dispatcher is not split and `src/tui/app/mod.rs` is not restructured | the dispatcher is still one 161-arm match in one function | pass |
| The dispatcher is not made `async` and does not adopt `spawn_tui_task` | untouched | pass |

## 9. Security review

Plan §6 required an **exhaustive** check that no arm's behaviour depended
on the argument *containing* the command name. The check was the WP1
derivation itself, arm by arm, over all 161 arms — not a sample:

* The ten `WHOLE-LINE-ARG` arms were the only ones that used the name as
  part of the argument. Their behaviour is the change of §4.3, and each is
  pinned by a test.
* The 49 `STRIP` arms depended on the name only as a prefix they removed;
  the extracted argument equals the value they computed.
* The 31 `TOKEN` arms depended on the name only as the token they split
  off; the extracted argument equals the value they computed.
* The two `WHOLE-LINE-PASS` arms handed the line to helpers that stripped
  the name; the helpers now receive the argument.

No arm used the command name for an **authority** decision (a path check, a
permission, a scope). The two that turned out to be security-adjacent are
`/open` (the path reached `normalize_relative_path`, which refused the
absolute path — after the fix the path is the workspace-relative path the
user typed, which is the path the daemon's `DocumentOpen` contains) and
`/review-change` (the candidate id reached the LSP preview staleness check;
before the fix the id was always empty and the review never ran). Neither
widens an authority: `/open` still goes through the same
`normalize_relative_path` containment check and the daemon's own scope, and
`/review-change` still refuses a stale or unknown candidate.

`/security-review` parses flags only; the command name was already inert
there.

## 10. Migration and compatibility review

No schema, protocol, configuration or persisted-state change. The
dispatcher's public shape inside the crate changed (`raw_input` →
`argument`, `Option<&str>` → the argument's meaning), and the two helpers
that took the whole line take the argument now. `dispatch_builtin_command`
is `pub(crate)`, so no external consumer exists. The remote snapshot's
`dialog` field can now name `PanelManager` / `Control`; the field is
additive information in an already-optional string.

## 11. Documentation and operations

* `architecture/tui.md` — new section "The slash-command argument
  contract": where the argument is extracted, that it excludes the command
  name and its alias, that handlers receive only the argument, and a link to
  the guard.
* `scripts/check_command_argument_contract.py` — the new guard.
* `scripts/verify.sh` — the guard and its self-test are in `run_guards()`.
* `.github/workflows/ci.yml` — the guard and its self-test are in the
  guard step.
* No operator diagnostic, recovery instruction or configuration changed.

## 12. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's affected-command list named six; the re-derivation found twelve. | The plan's §14 stop condition fired during execution; the scope was re-derived and recorded here rather than silently widened. | Re-register the scope in `plans/registry.md` and in the roadmap (done in §14). |
| low | `/security-review`'s arm is migrated but its observable result does not change, so it has no baseline-failing behavioural test. | The contract for that command is pinned by the extraction test and by the guard, not by a behaviour test. | none |
| low | The hosted `CI / verify` lane has not run against this commit. | CI evidence is outstanding. | Run the lane against the commit and record the result. |

## 13. Roadmap disposition

**Milestone implemented; closure record pending the hosted CI lane.** The
milestone's capability — a single argument-extraction point, a total
`Dialog::is_open`, and a guard that keeps both — is in the tree. The
roadmap's exit conditions are met:

* *every registered command with an argument receives it* — the 92 arms
  that read an argument consume the extracted one;
* *a single argument-extraction point exists* —
  `Command::argument_from_typed_line`, called from the registry-entry
  resolution;
* *the six known-defective commands are covered by tests that fail on the
  baseline* — the six are covered, and so are the six the re-derivation
  added;
* *the `Dialog::is_open` predicate covers every dialog variant* — it does.

C005 (re-enable the guards) and C006 (the TUI render path) were gated on
C001–C003 being closed; this record closes C003's implementation but the
roadmap's status column is updated in §14.

## 14. Registry updates

* `plans/registry.md` — the C003 row of the "Active implementation plans"
  table moves from **ready for handoff** to **implemented; closure record
  pending**, and names this closure record, the implementation commit, and
  the re-derived affected set (twelve commands, not six).
* `plans/subsystems/repository-audit-corrective-roadmap.md` — the C003
  milestone status moves from *not started* to *implemented*, and the
  affected-command count in the milestone row is corrected from six to
  twelve with the six additions named.
* Unblock audit: nothing else was waiting on C003 inside this workstream.
  C005 waits on C001–C004 and C006 on C001 and C003; this record makes
  C003 available to C006.
