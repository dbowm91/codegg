# TUI Editor Helix-style Keymap and Configuration M001 — Closure Status

Status: closed (implementation and evidence complete; §5 records the two
defects that verification found and this change fixed, and §6 records the
limits of the live evidence rather than claiming more than it shows)

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-keymap-config-roadmap.md#milestone-1--the-helix-style-editor-configuration-the-nested-minor-modes-the-space-q-a-gesture-and-the-completion-popups-three-fixes`

No ADR governs this milestone. ADR-0066 governs the inline assist's
**contract** — the pending region, the approval and the discard — and none
of it moved. The two deviations ADR-0066 already carried are untouched by
this change and are recorded in
`plans/closure/tui-editor-inline-llm-assist/001-status.md` §5.

Repository baseline reviewed: `bab09b789`
(`feat(tui,core): inline editor LLM generation and a cursor-anchored review menu`).

Implementation commits or pull requests: none. The work is uncommitted in
the working tree at the time of this record.

## 1. Executive finding

The editor's inline-generation gesture is now a **Helix-style key sequence**
rather than a chord: `space`, then `q`, then `a`, in Normal mode. Each
level paints the menu it is waiting in, so the reader sees `q` and then
`a` instead of having to remember the path. The `Ctrl+Alt+a` chord is
**removed** — there is one route to the capability, not two — and the
`Ctrl+Alt+u` / `Ctrl+Alt+v` approve and discard chords are unchanged.

The editor now has **its own configuration file**, in Helix's format: a TOML
file with nested key tables, layered **shipped defaults → global → project**,
with the project winning. The shipped default is
`[keys.normal.space.q]` / `a = "llm_insert"`, and the shipped asset is
`crates/codegg-tui/assets/editor.toml`. The global layer is
`<config dir>/codegg/editor.toml`; the project layer is
`.codegg/editor.toml`.

The completion popup now sits **near the cursor**, **does not appear when
there is nothing to complete**, and **can be switched off** with
`completion-popup = false` in the `[editor]` section.

The whole gesture path is verified **live** in a terminal: 14 of 14 checks
pass in an isolated environment (§4).

## 2. Requirement-to-evidence matrix

| Exit condition | Evidence | Result |
|---|---|---|
| `space` opens a menu that names `q` | `the_shipped_defaults_bind_the_space_menu_to_llm_insert`; live receipt step B (`the space menu offers q`, `the space menu is drawn`) | pass |
| `space q` opens a nested menu that names `a` | `the_shipped_defaults_bind_the_space_menu_to_llm_insert`; live receipt step C (`the shipped default binding is listed`) | pass |
| The nested menu also lists what the operator configured | `a_project_config_extends_the_shipped_defaults`; live receipt step C (`the project binding is listed too`) | pass |
| The configured binding runs the command | `space_q_a_runs_the_inline_generation_command`, `a_configured_menu_binds_a_command_the_defaults_do_not`; live receipt step D (`the inline prompt is open`) | pass |
| The shipped default still runs after the operator's file is merged over it | live receipt step E (`the default binding opens the prompt`) | pass |
| The editor's own keymap box names the new gesture | live receipt step F (`the keymap box names space q a`) | pass |
| A broken binding is reported, not silently ignored | `an_unknown_command_is_reported_and_not_bound`; live receipt step G (`the problem is surfaced at startup`, `the broken binding is not bound`) | pass |
| A key the configuration does not claim still reaches the built-in space leaves | live receipt step H (`space f still opens the file picker`) | pass |
| The completion popup sits near the cursor | `completion_popup_renders_below_the_cursor_with_candidates` | pass |
| The completion popup does not appear when there is no completion | `completion_popup_states_render_distinctly` | pass |
| The completion popup can be switched off in the editor config | `the_completion_popup_can_be_switched_off`, `the_completion_request_is_never_issued_when_the_config_disables_the_popup` | pass |
| A file that tries to bind the editor's own Normal-mode chords is told so | `the_editors_own_normal_chords_are_not_configurable`, `a_menu_that_is_not_the_space_menu_is_reported` | pass |
| The global layer is found and loaded | `a_global_editor_config_is_found_and_loaded` | pass |
| The project layer is found and loaded | `a_project_editor_config_is_found_and_loaded`, `a_project_config_extends_the_shipped_defaults` | pass |

## 3. What was built

- `crates/codegg-config/src/editor.rs` — the file format and its discovery:
  `parse_editor_config`, `load_editor_config`,
  `global_editor_config_path`, `find_project_editor_config_from`. The
  crate keeps the key spellings as **text**; it has no key semantics.
- `crates/codegg-tui/src/editor_keymap.rs` — the runtime keymap:
  `NamedKey`, `Modifiers`, `KeyPattern`, `MenuPath` (a bounded `Copy`
  path), `MenuAction`, `MinorMode`, `EditorKeymap`, and the
  `EditorMenuView` the renderer draws.
- `crates/codegg-tui/assets/editor.toml` — the shipped defaults.
- `crates/codegg-tui/src/editor.rs` — `EditorPrefix::SpaceMenu(MenuPath)`,
  so the nested menu rides in the one pending-prefix field and the
  prefix-state guard stays green.
- `crates/codegg-tui/src/commands/editor.rs` — the dispatch: the configured
  menu is consulted **before** the built-in space leaves, an unclaimed key
  falls through to them, and a configured command is **named** rather than
  performed (`EditorPendingEffect::RunCommand`, drained by the App layer).
- `crates/codegg-tui/src/components/editor.rs` — the menu overlay, the
  popup's placement, and the popup's emptiness rule.
- `crates/codegg-tui/src/app/mod.rs` — the keymap is loaded from the
  project directory, and a problem is surfaced at startup rather than
  dropped.

## 4. Live receipt

One isolated environment (`HOME`, `CODEGG_DATA_HOME` and the project
directory all redirected; no provider credentials present). **14 of 14
checks pass.**

| Step | Checks | Result |
|---|---|---|
| A | the composer is ready; a valid config raises no problem; the file opens | pass |
| B | `space` offers `q`; the menu is drawn | pass |
| C | `space q` names the command; the shipped default and the project binding are **both** listed | pass |
| D | `space q b` opens the inline prompt; `Esc` dismisses it | pass |
| E | `space q a` still opens the inline prompt | pass |
| F | the editor's keymap box names `space q a` | pass |
| G | a broken binding is reported at startup and is **not** bound; the shipped default is still bound | pass |
| H | `space f` still opens the file picker: the configured menu is consulted **first**, and a key it does not claim falls through to the built-in leaves | pass |

The receipt script is `/tmp/codegg-receipt-keymap3.sh`; the isolated
directory it leaves behind is named in its own output.

## 5. Defects that verification found, and this change fixed

Both were found by the live receipt, and both are now covered by a test.

### 5.1 The menu was replaced, not merged

`EditorKeymap::fill_menu` built a menu from the configuration it was given
and **replaced** the menu at that path. A project file binding
`space.q.b` therefore **erased** the shipped `space.q.a` rather than
extending it: the operator's file silently took the capability away.

`fill_menu` now resolves the bindings first and then **merges** them into
the menu at that path, replacing an entry only when the same key is bound
again. Covered by `a_project_config_extends_the_shipped_defaults` and by
live receipt steps C and E.

### 5.2 Opening a document discarded the loaded keymap

`App::swap_editor_state` installs the editor state for the tab that is now
on screen. For a tab with no parked state it installed
`EditorState::default()`, and `EditorState::default()` carries
`EditorKeymap::defaults()` — the **shipped defaults**, not the keymap the
application loaded. The project configuration was therefore read, applied,
and then thrown away the moment a file was opened, which is why the
project's own binding did nothing at runtime even though the file was
provably loaded.

The keymap is the **application's configuration**, not one document's, so it
now survives the swap. Covered by
`the_editor_keymap_survives_a_document_opening` (in
`crates/codegg-tui/src/app/mod.rs`) and by live receipt step D.

### 5.3 Two smaller corrections, made with the change

- The global-configuration test set `XDG_CONFIG_HOME`, which
  `dirs::config_dir()` ignores on macOS (it resolves
  `$HOME/Library/Application Support` there). The test now isolates
  `HOME`, `XDG_CONFIG_HOME` and `APPDATA` and writes the file where the
  platform actually looks.
- `an_unknown_command_is_reported_and_not_bound` asserted that an unusable
  binding left the menu empty. That was true of the **replace** semantics;
  under the merge semantics an unusable binding leaves the shipped default
  in place, which is the better behaviour, and the test now says so.

## 6. The limits of this evidence — recorded, not absorbed

- **The completion popup's "there is a completion" case was not reproduced
  live.** The isolated environment has no language server, so there is no
  completion to show; the live receipt can only show that **no** popup
  appears, which is the emptiness rule and not the populated case. The
  populated case is covered by the render test
  `completion_popup_renders_below_the_cursor_with_candidates`.
- **The completion trigger's effect is not observable on the editor route.**
  The trigger's refusal is a **toast**, and toasts are drawn by the main
  buffer's render, which the editor route does not draw. The switch is
  therefore covered by the dispatch test
  `the_completion_request_is_never_issued_when_the_config_disables_the_popup`
  (which asserts both directions — enabled raises the request, disabled
  raises nothing) rather than by a live check.
- **Only the space menu is configurable.** The editor's own Normal-mode
  chords are its own tables, guarded by
  `scripts/check_tui_editor_key_authority.py`; a file that tries to bind
  them is told so. Making them configurable would move the keymap's
  authority and is a separate decision.
- **The other Helix minor modes are not configurable yet.** The grammar
  machinery carries a path for them — a menu path is a bounded list of keys
  — but only `space` is wired, and the configuration file says so.

## 7. What was checked

- `cargo nextest run -p codegg-tui --profile ci --no-fail-fast` —
  **4669 passed**, 1 skipped.
- `cargo nextest run -p codegg-config --profile ci -E 'test(/editor_config/)'`
  — 2 passed.
- `cargo nextest run -p codegg-tui --lib --profile ci -E 'test(/completion/) or test(/editor_keymap/) or test(/center_tab/)'`
  — 129 passed.
- `python3 scripts/check_comment_budget.py` — passed.
- `scripts/check_tui_editor_prefix_state.py`,
  `scripts/check_tui_editor_key_authority.py`,
  `scripts/check_tui_editor_text_authority.py` — passed.
- Live `tui-test` receipt — 14 of 14 (§4).

## 8. The two amendments this change made elsewhere

- `crates/codegg-tui/src/components/editor.rs` — the editor's keymap box
  read `Ctrl+Alt+a`; it reads `space q a`.
- `architecture/tui.md` and `docs/tui.md` — the gesture, the
  configuration file, the layering, the command names, and the completion
  popup's three rules.
