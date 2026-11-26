# TUI editor Helix-style keymap configuration — roadmap

Status: **closed** — M001 implemented and closed; closure record
`plans/closure/tui-editor-helix-keymap-config/001-status.md`.

Repository baseline reviewed: `bab09b789`
(`feat(tui,core): inline editor LLM generation and a cursor-anchored review menu`).

This line adds the **editor's own configuration file**, in the format Helix
uses, and the nested minor modes it makes expressible. It does not reopen any
closed milestone: not the editor Helix-parity, Helix-capability-parity,
Helix-UX, IDE-presence, editor-document-foundation, editor correctness or
inline LLM assist lines.

## 1. What the operator asked for

Three things, in one report:

1. The inline-generation gesture was a chord (`Ctrl+Alt+a`) that did not
   reach the editor, because the chord was already taken. It should be a
   **Helix-style sequence** instead: `space`, then `q`, then `a`, with the
   menu on screen at each level.
2. The editor should adopt **Helix's configuration format**, with shipped
   defaults, so the keys are the operator's to change.
3. The language-server completion popup should sit **near the cursor**, should
   **not appear when there is no completion**, and should be **switchable off**
   in the editor configuration.

## 2. Scope

### In scope

- A TOML configuration file for the editor, layered over shipped defaults:
  global (`<config dir>/codegg/editor.toml`) then project
  (`.codegg/editor.toml`).
- Helix's nested table grammar: `[keys.normal.space.q]` with
  `a = "llm_insert"`.
- The space menu and its nested menus as **configurable** minor modes, with
  the menu drawn beside the cursor.
- The inline-generation command reachable as `space q a`, and named
  `llm_insert` so a configuration file can bind it.
- The completion popup's position, its emptiness rule, and its
  `completion-popup` switch.

### Out of scope — and why

- **The editor's own Normal-mode chords are not configurable.** They are the
  editor's own tables, they are guarded by
  `scripts/check_tui_editor_key_authority.py`, and making them configurable
  would give the configuration file a second claim on the whole keymap. A file
  that tries is **told so**, not silently ignored.
- **No second apply path.** The configuration names commands; the editor's
  existing command registry resolves them, so `llm_insert` is the same
  `open_inline_prompt` the chord used to reach.
- **No new chord family.** `{CONTROL, ALT}` only, and this change removes a
  chord rather than adding one.
- **No new pending-prefix field.** The nested menu rides in the
  `EditorPrefix` variant, so the prefix-state guard stays green.
- **No change to the inline assist's contract.** ADR-0066 governs the
  pending region, the approval and the discard, and none of them moved.

## 3. Milestones

| Milestone | Status | Closure |
|---|---|---|
| M001 the Helix-style editor configuration, the nested minor modes, the `space q a` gesture, and the completion popup's three fixes | closed | `plans/closure/tui-editor-helix-keymap-config/001-status.md` |

## 4. What a later milestone would owe

- Making the editor's own Normal-mode chords configurable — a separate
  decision, because it would move the keymap's authority.
- The other minor modes Helix has (`g`, `z`, `m`, the `"` register
  grammar) as configurable menus. The grammar machinery already carries a path
  for them; the configuration file's mode table is where they would be
  declared.
