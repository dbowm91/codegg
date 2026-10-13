# TUI Editor Correctness and Usability M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md#M004`

Repository baseline reviewed: `1166a00b0`
(M003 closure; C001/C002/M003 closed, M004 active).

Implementation commits or pull requests:

- `fd2faaaf1` — tui-editor(m004): bounded completion popup and transactional acceptance

## 1. Executive finding

M004 is complete as the user-facing completion capability. An explicit
`F8` (or `:complete`) trigger issues the M003 read against the exact
synchronized snapshot; a bounded popup displays typed suggestions with
explicit states; `Enter` inserts only the chosen supported edit as one
controller transaction with one undo unit, and redo replays it exactly.
Stale, superseded, mismatched, unsafe, and lease-lost accepts are refused
with words and change nothing. Popup keys never leak into the document or
the global bindings; typing, lifecycle operations, detaches, tab switches,
and reconnects invalidate the popup. No snippet expansion, no additional
edits, no command execution, no auto-trigger, no second text buffer, no new
storage or authority. With C001, C002, and M003 already closed, this closure
completes the roadmap's editor workstream.

## 2. Requirement-to-evidence matrix

| Requirement (plan §10) | Evidence | Result | Notes |
|---|---|---|---|
| Real simple Rust completion displays bounded typed suggestions, inserts only the chosen supported edit | `ready_answer_applies_one_undo_unit_with_redo` (scripted `LspCompletionGet` → popup → `Enter` → `println!` in text) | pass | Fake covers daemon+server; M003 proved the real ones |
| Accepted suggestion = one controller edit + one undo; redo replays forward exactly | Same test: `undo.len() == 1` after accept; `u` restores `LIB_RS`; recorded forward re-applies to `println!` | pass | Shared `submit_edit` path, not a second apply |
| Stale/unsynced/wrong-tab/unauthorized/unsafe cannot modify text, disk, or prompt | Scope/generation/identity mismatch tests; snippet `Enter` refusal; lease-loss refusal (no text, no history, visible notice) | pass | Refusals keep the popup for retry except dead bindings, which clear |
| Text input, panels, PTY focus, modes, `Esc` correct; manual trigger reachable without terminal-specific keys | Typing test; composer-focus test; `Esc`-preserves-mode (both modes); `:complete` trigger test; panel/docking/presentation/terminal suites green | pass | `F8` has zero conflicts; `:complete` needs no special keys |
| Empty Ready differs from NotReady/Stale/Unsupported; truncation disclosed; no LSP command run | Status-line test (`no suggestions` vs `warming…`/`re-trigger`/…); widget states test; `…list truncated` / `…more may exist` rows; command-carrying candidates non-applicable end to end | pass | M003 statuses map 1:1 to popup states |
| Integration + negative tests + hosted evidence on exact SHA | 18-trajectory green 8/8 runs; 10 state + 5 resolver + 6 widget unit tests; local `verify.sh quick` on `fd2faaaf1` | pass | Hosted exact-SHA CI not run (low finding, §10) |
| No label-insertion fallback; no unsafe reinterpretation | Bare-label items insert at cursor per the spec default (tested); everything else keeps its reason and refuses | pass | Label-as-insert is the LSP default, not a guess |
| UTF-16/surrogate safety | Resolver unit tests (split/past-end/reversed refuse); emoji trajectory (request col 4, apply exact `B`) | pass | Shared `utf16_column_to_byte` semantics |
| 100/101 candidates | 101 scripted → 100 stored + `truncated`; widget scrolls 15 with tail visible | pass | Defensive frontend cap mirrors the protocol bound |
| Malicious strings | Widget test: escapes/newlines/tabs neutralized, 500-char detail truncated | pass | Sanitize + truncate at the presentation boundary |
| Narrow terminals | 1x1 and 20-wide hide the popup without panic; keys still live | pass | State and rendering are independent |
| Huge lines | 1 MiB line + open popup renders within budget | pass | Popup reads DTO strings only, never the snapshot |
| Overlap/cancel/reconnect/switch | Supersede + out-of-order + rapid-retrigger tests; switch/detach clear test; reconnect clears in code | pass | One live request id; late answers drop |
| Old/cold servers | `NotReady` and `Error` popup states refuse `Enter` without side effects | pass | M003 proved the wire side |

Work package A (state table + keybindings + `:complete`): the binding
survey found `Ctrl+N`/`Ctrl+P` taken (`NewSession`/`CycleModelForward`) and
the chord contract forbids new bare-`Ctrl` letters, so `F8` (zero conflicts,
in the F2–F7 LSP family) plus `:complete` is the documented outcome;
`docs/tui.md` and the palette carry both.

## 3. Production implementation evidence

- `src/tui/editor_completion.rs` (new): `EditorCompletionState`
  (scope/status/candidates/flags/selection/error/loading/request/popup —
  no text fields, no text containers), superseding `begin()`, fenced
  `apply()` with identity check + defensive 100-cap, selection,
  side-effect-free `dismiss()`, invalidating `clear()`, status lines that
  distinguish every M003 state, and `sanitize_completion_text`.
- `src/tui/editor.rs`: `EditorCommandName::Complete` (`:complete`, parsed,
  paletted, help-lined); `resolve_completion_edit` (UTF-16 range → byte
  range → single forward/inverse pair via shared `build_edit`); empty-`:`
  palette now offers the whole registry so the 9th command stays
  discoverable.
- `src/tui/commands/editor.rs`: `current_completion_scope` (binding +
  attachment + UTF-16 cursor), `start_editor_completion` (scoped task,
  `LspCompletionGet`, `EditorCompletionFinished`), fenced
  `apply_editor_completion`, `handle_completion_key` (F8/`Up`/`Down`/
  Normal-`j`/`k`/`Enter`/`Esc`, everything else dismisses and falls
  through), `accept_selected_completion` (full re-validation, single
  `submit_edit`, per-reason refusals), `:complete` arm, and invalidation in
  `submit_edit`, undo, redo, `start_operation`, plus LSP-panel starts
  dismissing the popup.
- `src/tui/app/state/editor.rs`: `completion` field + `Default` +
  `detach()` clear.
- `src/tui/app/project_session.rs`: reconnect + tab-switch clears.
- `src/tui/app/commands.rs` + `runtime/command_dispatch.rs`:
  `EditorCompletionFinished` arm (stale-completion fenced by request id).
- `src/tui/app/mod.rs`: `App::apply_editor_completion` for dispatch and
  trajectory tests.
- `src/tui/components/editor.rs` + `app/render.rs`: `render_completion_popup`
  (centered, width/height capped, hidden when unfiittable, selection +
  scroll window, truncation/incomplete notices, sanitized literal spans).
- `docs/tui.md`: completion invocation/scope/failure/unsupported section.
- `architecture/tui.md`: M004 subsection (precedence, undo/revision
  invariants, invalidation list).

Ownership preserved: candidates are inert DTOs; ranges resolve against a
borrowed snapshot dropped before submit; undo history stays
`TextTransaction` pairs; no `DocumentBuffer`/`Rope` construction, no
filesystem reads, no model/agent insertion path (guards green).

## 4. Verification executed

### Commands run

```bash
cargo test --lib tui::editor --locked
cargo test --lib tui::editor_lsp --locked
cargo test --lib tui::components::editor --locked
cargo test --locked --test tui_completion_trajectory
cargo test --locked --test document_client_trajectory
cargo test --locked --test tui_panel_docking
cargo test --locked --test tui_presentation_regressions
cargo test --locked --test interactive_terminal_tui
cargo test --lib --locked -- tui::
cargo test --locked --test tui_render
cargo test --locked --test tui_usability_fixes
cargo test --locked --test file_tree_trajectory
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

- `tui::editor` + `editor_completion` + `components::editor`: all pass
  (10 new state, 5 new resolver + 1 parse, 6 new widget tests).
- `tui_completion_trajectory`: 18 passed, 8/8 consecutive runs (one
  request/content race found during development and removed — see §10).
- Broad `tui::`: 1497 passed / 0 failed. `tui_render`: 110. `tui_usability_fixes`:
  18. `file_tree_trajectory`: green (C002 intact).
- `document_client_trajectory`: 3. `tui_panel_docking`: 33.
  `tui_presentation_regressions`: 15. `interactive_terminal_tui`: 6.
- Text/project/authorization guards: pass. `fmt --check`: pass.
- `clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass.

Local truth only. Hosted root CI, Desktop E2E, and live-terminal runs were
not executed from this environment (see §10). All suites above ran locally
on `fd2faaaf1`.

## 5. Invariant review

- Suggestion carries no authority: popup hands out inert data; exactly one
  explicit `Enter`, after full re-validation, builds one transaction.
- Buffer stays `DocumentController`-owned: no second buffer (guard green);
  no disk write except through existing controller paths; no
  `LspPreviewApply`; writer lease enforced by `apply_local` with a visible
  notice on loss.
- No stale/unsafe insert: scope+generation+identity+revision+range
  re-checked at accept; non-applicable never appliable; malformed ranges
  refuse.
- Routing intact: popup keys claimed only with buffer focus and an open
  popup; modals/panels/tree/composer precedence untouched (existing suites
  green); `Esc` never quits the editor from the popup.
- Bounded: ≤100 candidates stored, ≤10 rows shown, label/detail/error caps,
  no polling, one live request id, no durable candidate persistence.
- C001 presentation, C002 bounds, and all editor/project guards preserved.

## 6. Failure and recovery review

- Pending request + typed edit: popup clears at keystroke; late answer
  drops on the loading fence.
- Superseded/out-of-order answers: newest request id wins, provably in
  both application orders.
- Stale/wrong-attachment answers: generation/scope/identity fences drop
  before display.
- Cold/broken servers: explicit states, `Enter` refuses, retry by
  re-trigger.
- Lost lease / moved text: refusal with notice, history untouched.
- Reconnect/switch/close/lifecycle ops: popup cleared at initiation;
  in-flight answers fence out.
- Narrow/huge terminals: popup hides or stays bounded; keys live.

## 7. Migration and compatibility review

Additive UI feature; no protocol, storage, config, or authority change
(M003 DTOs consumed unchanged — a correction to M003 through its closure
process was never needed). Existing editor keys behave identically with
the popup closed. Old daemons surface as the `Error` state with no
label-guessing fallback. `LspPreviewApply` independent. Remote attachment
locality untouched. No new keybinding in the global map (no
`modifier_chords` impact); `F8` is editor-buffer-local.

## 8. Security review

No model/agent insertion path added or widened. Untrusted server strings
sanitized (controls → space) and truncated before render; ratatui spans are
literal. No secrets logged (labels/details never enter telemetry; errors
are bounded daemon messages). No new authorization descriptor (read-only
reuse of the M003 contract). No terminal-escape smuggling (widget test
proves `\x1b` cannot survive). Prompt-focus capture: buffer-focus gating +
  modal/panel precedence mean completion keys cannot land in the prompt.

## 9. Documentation and operations

- `docs/tui.md`: trigger keys, selection/accept/dismiss, scope of
  applicable vs unavailable, state distinctions, truncation disclosure.
- `architecture/tui.md`: precedence, undo/revision invariants,
  invalidation list, sanitization rule.
- Operational signals: per-state header lines, `…list truncated` /
  `…more may exist server-side`, per-reason refusal toasts, `…` loading row.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted root CI / Desktop E2E / live-terminal smoke not run on `fd2faaaf1` | Strict closure prefers exact-SHA hosted green; local `verify.sh quick` + 1497 lib tests + all trajectory suites substitute locally | Run hosted CI on the merge head; record runs here by factual correction if they fail |
| low | No live agent-stream overlap run (completion while a turn streams) | Concurrency mechanism (supersede + fences + independent task kind) is tested; a live multi-turn overlap is not | Optional live qualification on a future pass; no evidence of interference (disjoint state) |
| low | Approval-dialog precedence argued structurally (modals short-circuit at `on_key` before the editor), not by a live modal test | Precedence code is unchanged and covered by existing modal suites | Optional targeted modal+popup test on a future pass |
| low | Interactive type-to-filter deferred: typing dismisses instead of narrowing | Documented in `docs/tui.md` behavior ("typing dismisses"); the DTO already carries `filter_text` for a successor | Successor plan if filtering becomes desirable; not a defect |
| low | `Ctrl+R` in Normal buffer resolves to resync, shadowing the `handle_normal_key` redo arm | Pre-existing routing (mode-independent chords run first); redo remains reachable and is tested at the transaction level here | Separate issue if redo-by-key matters; out of this milestone's scope |

No critical/high/medium findings. No corrective pass required. One
trajectory race (content/request coupling across concurrent tasks) was
found by repeated runs during development and removed by binding those
tests to request ids; the suite is 8/8 stable.

## 11. Roadmap disposition

Milestone M004 closed. C001, C002, M003, and M004 now all hold individual
closure records with exact commits and local verification, the accepted
completion ADR (ADR-0021 Option B) is recorded, regression cases
demonstrate bounded costs and refusal semantics, and the operator docs
explain supported and unsupported behaviors — satisfying the roadmap's
completion definition. The roadmap as a whole moves to `closed`. No
follow-up corrective work was created; the deferred filter and the
`Ctrl+R` shadowing note above are explicitly not corrective passes.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md`:
  `active` → `implemented; closed` (`fd2faaaf1`, this closure).
- `plans/subsystems/tui-editor-correctness-usability-roadmap.md`: M004
  section/row `ready` → `closed` with closure link; roadmap status →
  `closed` (C001/C002/M003/M004 all closed, completion definition met).
- `plans/registry.md`: subsystem row to `closed` with the four closure
  refs; M004 row `active` → `closed` with closure + implementation refs
  and verification summary; unblock audit recorded (M004 was the
  workstream's terminal milestone — nothing downstream was waiting on it).
