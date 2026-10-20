# Closure record — TUI editor Helix parity Milestone 017

Milestone: **M017 — the code-action read: `space a` previews instead of stating**
Plan: `plans/implementation/tui-editor-helix-parity/017-code-action-read.md`
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M017,
Amendments A1 and A2)
Closed: 2026-10-19
Baseline: `9b04b59a` (`feat/auto-20261018-12255c45`)

## 1. The finding, in five points

1. **`space a` was the last leaf that named a thing it could not do.** Its
   reason was *true* — the protocol had no operation that asks a server what
   code actions it would offer — and the reason said so rather than implying
   a preview existed and was merely unapplied. This milestone gave it the
   operation.
2. **The read is a *preview*, and the read family stays read-only by
   construction.** What travels is a summary and a diff, never an edit. The
   protocol's own note — that `CodeAction` is absent "because its answers are
   edits" — is about the **raw** action, and it was corrected in place, not
   deleted.
3. **One request, many previews.** The alternative — summaries first, then a
   second request naming the chosen action — carries a hazard the model-facing
   `codeActionPreview` already has: an action's *index* is only meaningful for
   the list the server returned in that moment, and a second request may
   return a different list. The index would then name a different action than
   the user picked. One request removes the index entirely.
4. **A code action is a *choice*, so the panel is the picker.** `j`/`k` move
   the selection, `Enter` opens the change review on the **selected**
   action's candidate, and the review does **not** open by itself — opening it
   on the first entry would be choosing for the reader.
5. **The candidate travels M016's way: carried, never registered.** The read
   result carries it, `candidate_of` deliberately does *not* lift a
   code-action candidate out of the payload, and the review reaches the apply
   through M016's one builder.

## 2. What was built

| Work package | Where |
|---|---|
| W1 — the protocol's code-action surface | `crates/codegg-protocol/src/lsp.rs`: `LspReadOperation::CodeActionPreview`, `LspReadPayloadDto::CodeActions`, `LspCodeActionDto`, `LspCodeActionUnavailable`, `MAX_LSP_CODE_ACTIONS`, `MAX_LSP_READ_CANDIDATE_BYTES` |
| W2 — the total-candidate bound | the same file's bounds, and the daemon's running total |
| W3 — `egglsp`'s warm-only entry point | `crates/egglsp/src/operations/code_actions.rs`: `code_action_previews_for_warm_client`, `code_actions_for_key`, `CodeActionEntry`, `code_action_entry` |
| W4 — the daemon's arm | `src/core/daemon_lsp.rs`: the `CodeActionPreview` arm, `mint_code_action_candidates`, `code_action_patches`, `code_action_exceeds_candidate_bounds` |
| W5 — the leaf, the panel, and the review | `src/tui/editor.rs`: the `space a` leaf; `src/tui/commands/editor.rs`: `open_selected_code_action`; `src/tui/components/editor.rs`: the panel's action list; `src/tui/editor_lsp.rs`: `code_action_unavailable_reason` |
| W6 — the wording, the docs, the guard | the seven documents; the new guard is M018's, and this milestone did not need one |

## 3. The model

### 3.1 The DTO

```rust
pub struct LspCodeActionDto {
    pub title: String,
    pub kind: Option<String>,
    pub preferred: bool,
    pub disabled_reason: Option<String>,
    pub has_edit: bool,
    pub has_command: bool,
    pub candidate: Option<LspPreviewCandidateDto>,
    pub unavailable: Option<LspCodeActionUnavailable>,
}
```

`LspCodeActionUnavailable` names the cause: `NoEdit`, `Command`, `Disabled`,
`IncompletePreview`, `TooLarge` or `BoundReached`. It is present **exactly**
when `candidate` is not.

**The reason is per action, not per result.** One action in a list can be
command-only while the one beside it is perfectly applicable, and the daemon
stops minting at the bound — so the actions after the bound are cut for that
reason while an action earlier in the list may have had its own. A
result-wide flag could only say "something was cut" and the reader would not
know which.

### 3.2 The panel is the picker

| Key | What it does |
|---|---|
| `space a` | ask the server for the code actions it would offer at the cursor |
| `j` / `k` | move the selection |
| `Enter` | open the change review on the selected action |
| `Esc` | close the panel |

The panel's row count is the action count, and each row says what would
happen: *ready to apply*, or the reason it cannot be. The reason comes from
**one** helper (`code_action_unavailable_reason`), so the panel and the
`Enter` handler cannot drift apart — a reader who reads the panel and then
presses `Enter` is given the same words in the same place.

**The window follows the selection.** The panel is at most twelve rows tall
and a server can offer more actions than that, so the code-action arm renders
a window of eight that **ends on the selected action**, and the panel's
scroll offset becomes that window's offset. This is the one payload that
overrides the panel's standing scroll, and it does so for a reason the others
do not share: here `Enter` applies a *change*, so an off-screen selection
would offer to apply something the reader could not read. The other pickers
in the panel (diagnostics, locations, symbols, highlights, signature help,
rename files) keep the standing behaviour, where a selection beyond the
visible rows is invisible — a **pre-existing** panel property that M017
inherits rather than introduces. Changing it for the other payloads is a
separate concern with its own milestone; it is recorded in §9.

### 3.3 The one-request proof

The claim is **measured**, not asserted. The egglsp integration test drives
the fake LSP server and then reads the server's **own transcript**:

```rust
let requests = harness.code_action_request_count().await;
assert_eq!(
    requests, 1,
    "one request, and the extra previews were computed locally\n{}",
    harness.transcript()
);
```

The count is of what the *server received*. The claim is about the wire, not
about the client's intention.

### 3.4 A command-bearing action is refused whole

The LSP specification has the client apply an action's edit **and then** its
command. This surface never runs commands. So an action that carries a
`Command` payload gets **no** candidate — even one that also carries an edit,
because applying the edit alone would apply half of what the action means.

This matches what `egglsp`'s single-action `preview_code_action` already did
(it rejected any action with `command: Some(_)`), so the list and the single
agree. The mixed case is a **sharpening** of the plan's wording, recorded in
the plan's §17.3.

### 3.5 The bound

`MAX_LSP_READ_CANDIDATE_BYTES` is 2 MiB, against the per-candidate bound of
1 MiB. A result-wide bound at or below the per-candidate bound would let
exactly one large candidate through and refuse the second of two ordinary
ones, so the result bound is set above the per-candidate one. The protocol
test asserts the relation that makes this sound: a candidate that passes its
own bound can always be minted in a result.

The bound is a **parameter** of the daemon's minting function
(`mint_code_action_candidates(root, entries, max_total_bytes)`), so the
enforcement is proved without materialising megabytes of patch text.

## 4. The guards

| Guard | What it says | Result |
|---|---|---|
| `scripts/check_tui_editor_key_authority.py` | the chords' attribution | pass (2 guard-block / 71 Normal / 8 Insert) |
| `scripts/check_tui_editor_leaf_authority.py` | the leaf tables | pass (5 tables / 44 glyphs / 12 bindings) |
| `scripts/check_tui_editor_completion_authority.py` | the completion triggers (M018) | pass (2 triggers) |
| `scripts/check_tui_editor_prefix_state.py` | the prefix state | pass |
| `scripts/check_tui_editor_text_authority.py` | the text authority | pass |
| `scripts/check_tui_editor_apply_path.py` | one apply path | pass |

**The counts did not move.** `space a` keeps the glyph it had; the leaf table
gains no row and the Normal-mode chord count is unchanged. What changed is
what the leaf *does*.

## 5. Verification

The sweep ran against the final tree. `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`
throughout — the incremental cache in this worktree is unreliable
(`dep-graph.part.bin` went missing twice during the M011–M015 work).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo build --locked --bin codegg-sandbox-helper` | exit 0 |
| `cargo test -p codegg-protocol --locked` | **259 passed / 0 failed** (M016: 255) |
| `cargo test -p egglsp --features lsp-test-support --locked --test code_action_preview` | **2 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked` | **1012 passed / 0 failed** |
| `cargo test -p codegg --lib --locked --no-fail-fast` | **6838 passed / 2 failed / 4 ignored** (1 filtered out) — the two failures are the load-sensitive `interactive_terminal` class; see §5.2 |
| `cargo test -p codegg --lib --locked agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | **1 passed / 0 failed** (the one test the whole-suite run filters out — see §5.3) |
| `cargo test -p codegg --lib --locked interactive_terminal` | **94 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::interactive_terminal` | **37 passed / 0 failed** |
| `cargo test -p codegg --lib --locked -- tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop tui::commands::interactive_terminal::tests::a_live_terminal_surface_repaints_with_no_further_user_input` | **2 passed / 0 failed** (the two that failed in the whole-suite run, re-run alone) |
| `cargo test -p codegg --lib --locked tui::` | **2312 passed / 0 failed** (M016: 2308) |
| `cargo test -p codegg --lib --locked tui::commands::editor::tests::` | **226 passed / 0 failed** (M016: 224) |
| `cargo test -p codegg --lib --locked tui::components::editor::tests::` | **57 passed / 0 failed** (M016: 52) |
| `cargo test -p codegg --lib --locked tui::commands::change_review::tests::` | **9 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::editor_lsp::tests::` | **12 passed / 0 failed** |
| `cargo test -p codegg --lib --locked core::daemon_lsp::` | **17 passed / 0 failed** (M016: 12) |
| `cargo test -p codegg --locked --test change_review_trajectory` | **7 passed / 0 failed** |
| `cargo test -p codegg --features lsp-test-support --locked --test lsp_read_delivery_m013 --test lsp_read_surface_trajectory --test lsp_completion_trajectory` | **7 + 14 + 12 = 33 passed / 0 failed** |
| `python3 scripts/check_tui_editor_key_authority.py` | pass — **2 guard-block / 71 Normal / 8 Insert** |
| `python3 scripts/check_tui_editor_leaf_authority.py` | pass — **5 tables / 44 glyphs / 12 bindings** |
| `python3 scripts/check_tui_editor_leaf_authority.py --self-test` | pass — 9 cases |
| `python3 scripts/check_tui_editor_completion_authority.py` | pass — **2 triggers** (M018) |
| `python3 scripts/check_tui_editor_completion_authority.py --self-test` | pass — **9 cases** (M018) |
| `python3 scripts/check_tui_editor_prefix_state.py` | pass |
| `python3 scripts/check_tui_editor_text_authority.py` | pass |
| `python3 scripts/check_tui_editor_apply_path.py` | pass — one builder, one construction |
| `python3 scripts/check_tui_editor_apply_path.py --self-test` | pass — 7 cases |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

The sweep script is `/tmp/m017_verify.sh`; the log is `/tmp/m017_verify.log`.

**The sweep was not wholly green, and this record does not say it was.** The
whole-`codegg`-lib step failed with two failures, and §5.2 says exactly which
two and what they are. Every other step — fmt, clippy, the sandbox-helper
build, the protocol and `egglsp` suites, every focused subset, the three LSP
integration suites, the six guards and `verify.sh quick` — passed.

### 5.1 The chord and leaf counts did not move

The key-authority guard still reports **2 guard-block / 71 Normal / 8 Insert**
and the leaf guard still reports **5 tables / 44 glyphs / 12 bindings**. M017
added no chord and no leaf: `space a` keeps the glyph it had, and what changed
is what it *does* with the answer. A milestone that changed a count would
have said so here.

### 5.2 The two failures, and what they are

```
failures:
    tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop
    tui::commands::interactive_terminal::tests::a_live_terminal_surface_repaints_with_no_further_user_input
```

Both are in `src/tui/commands/interactive_terminal.rs`, which **this milestone
does not touch** — the M017 diff contains no line of it. They are the
load-sensitive class the M012, M013 and M016 closure records already name:
the M012 record lists both test names among its whole-suite failures, and the
M016 record reports the class as *94 passed in isolation* while the
whole-suite run can fail it.

The evidence that they are load-sensitive and not M017's:

| Run | Result |
|---|---|
| the whole `codegg` lib suite (6845 tests, machine load average **12.88**) | **2 failed** |
| `cargo test -p codegg --lib --locked interactive_terminal` | **94 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::interactive_terminal` | **37 passed / 0 failed** |
| the two failing tests, run alone | **2 passed / 0 failed** |

The failure is `expect("the box is showing a terminal")` — the PTY-backed
terminal's detail handle was not set when the dialog was drained, which is
what a starved spawn looks like. The two tests, re-run alone **in the same
tree**, pass. Nothing in M017's diff is on that path.

**This is registered, not discharged.** The class is pre-existing and was
already listed as such before M017; M017 neither caused it nor fixed it.

### 5.3 The one filtered test

`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
is filtered out of the whole-suite run and run alone instead. It is not
M017's: it is in the asset-refresh path, which this milestone does not touch,
and it **passes in isolation in 0.00s** while the whole-suite run — with the
machine's load average above 7 — can block in it for minutes. That is the
load-sensitive class the M016 closure record already names. The number
reported here is the whole suite's, with that one test out and its own result
beside it.

### 5.3 The new tests

| Where | What it proves |
|---|---|
| protocol | the code-action operation is a **point** operation, and the request refuses to be asked without a position |
| protocol | the code-actions payload round-trips |
| protocol | a command-only action carries **no candidate** and says why |
| protocol | one candidate always fits in one read result — the result-wide bound is at least the per-candidate bound |
| egglsp | **one request, many previews** — the server's own transcript says it saw exactly one `textDocument/codeAction` |
| egglsp | a command-only action produces no preview |
| egglsp | a **mixed** action (an edit *and* a command) produces no preview — it is refused whole |
| daemon | the minted candidate's digest is **recomputable from its own fields** |
| daemon | a command-bearing action gets no candidate; a disabled action gets none and keeps the server's reason; an action with no edit gets none |
| daemon | the daemon **stops minting** when the result reaches its bound, and says so — and it stops rather than skipping ahead |
| daemon | a file outside the workspace leaves the action without a candidate |
| TUI | the `space a` leaf issues the code-action read, and states no reason for not doing it |
| TUI | `Enter` on the selected action opens the review on **that action's** candidate — and the review does not open before a choice is made |
| TUI | a command-only action cannot be applied, and the toast names the action and the reason |
| components | the panel lists the offer, says which action can be applied, and says why the other cannot |
| components | the panel keeps the **selected** action visible when the server offered more actions than the panel is tall |

### 5.4 The pre-existing test that changed

One test changed meaning:
`space_a_states_that_no_code_action_request_exists` asserted that the leaf
*states a reason*. The claim became false when the read landed, so the test
was **replaced**, not deleted: it now asserts what the leaf *does* — it
issues the code-action read, and it states no reason for not doing it.

## 6. Exit conditions

| Condition | Result |
|---|---|
| `space a` asks the server what code actions it would offer | met — one `textDocument/codeAction` at the cursor, measured |
| The panel lists them, with their kind and whether they can be applied | met — §3.2 and the components test |
| `Enter` on one opens the change review on that action's candidate | met — the TUI test |
| The candidate reviewed is the candidate applied | met — M016's carried path, unchanged |
| A command-only or disabled action cannot be applied, and says why | met — and a **mixed** action is refused whole too (§3.4) |
| The read result's candidates are bounded, and the bound is visible | met — 2 MiB, per-action `BoundReached` |
| The **one apply path** is structural | met — unchanged, and the guard still passes |
| The chords **did not move** | met — 2 / 71 / 8 and 5 / 44 / 12, unchanged |
| The change is recorded in **eight places** | met — the roadmap, this record, `CHANGELOG.md`, `docs/tui.md`, `docs/LSP.md`, `architecture/tui.md`, `architecture/lsp.md`, and the two skill files |
| The docs and their skills moved **together** | met — one commit |

**None of the plan's §14 stop conditions fired.** The fourth — *"the
total-candidate bound would have to be enforced by dropping a candidate
silently"* — came closest, and the per-action reason is what discharged it:
the bound is never silent, because the action that was cut says it was cut.

## 7. Recorded divergences

The plan's §17 holds the nine deviations, from the plan file itself. The
three that changed the design rather than the wording:

- **§17.1 and §17.2 — the DTO carries two more fields, and the reason is per
  action.** The plan's DTO had six fields; it has eight, and the reason a
  candidate is missing is a per-action enum rather than a result-wide flag.
  The reason is spelled out in §3.1.
- **§17.3 — a command-bearing action is refused whole.** The plan's W4 said
  "a candidate per action that has an edit"; the implementation refuses any
  action carrying a `Command` payload, edit or not. §3.4 gives the reason.
- **§17.5 — the bound is a parameter.** The production call passes
  `MAX_LSP_READ_CANDIDATE_BYTES`; the test passes a small bound, so the
  enforcement is proved without megabytes of patch text.

## 8. The documents that moved

| Document | What changed |
|---|---|
| `docs/tui.md` | `space a` out of the table of leaves that cannot run; a new "Code actions: the picker is the panel" section with the per-action reasons |
| `docs/LSP.md` | the code action became a preview; the warm-only entry point |
| `architecture/lsp.md` | the entry point in the `LspOperations` listing, the preview-only table, the invariants' count |
| `architecture/tui.md` | the leaf's row, the picker, the per-action reason, and the one-request proof |
| `CHANGELOG.md` | the `space a` bullet under `### Added` |
| `.opencode/skills/lsp-ide/SKILL.md` | the leaf table and the command rule |
| `.opencode/skills/tui/SKILL.md` | the preview bullet and the picker's rule |

## 9. Carried forward

- **The code-action read asks without a diagnostics context.** The editor's
  read passes no diagnostics to the server, so a server that keys its quick
  fixes to the diagnostics it was told about will offer fewer of them here
  than it would with the context attached. Recorded, not hidden; the plan's
  §17.8 names it.
- **A mixed action is refused whole**, which is safe but may refuse an action
  a user could have applied as an edit. The panel says why, so the refusal is
  never a mystery.
- **The other pickers in the LSP panel still show the first rows only.** A
  selection beyond the visible rows is invisible in the diagnostics, locations,
  symbols, highlights, signature-help and rename-file panels. That is the
  panel's standing behaviour, not something M017 introduced, and M017's
  code-action list is the one that overrides it — because there `Enter`
  applies a change. Fixing the others is a separate concern with its own
  milestone.
- **M014** — the syntax-tree slice — remains blocked on ADR-0034's
  acceptance, which is a decision the implementation is not free to make.
- **The change quartet's missing data source** — it needs its own milestone
  and its own plan.
- **The dirty-buffer merge** — unchanged and still deferred.

## 10. The residual

| Residual | Why it is not this milestone's |
|---|---|
| The `Ctrl-s`/`Ctrl+r` Insert-mode conflict | M011 decided it; both keep their mode-independent meaning |
| The key-authority guard's `Ctrl-x` blind spot | M018 — closed 2026-10-19 |
| The 69 failing `tests/` integration binaries | Pre-existing; registered, not discharged |
| The code-action read's missing diagnostics context | A narrowing of this milestone's own scope, recorded in §9 |
| The two `interactive_terminal` failures in the whole-lib run | The load-sensitive class of §5.2 — pre-existing, in a file this milestone does not touch, and passing in every focused run |
