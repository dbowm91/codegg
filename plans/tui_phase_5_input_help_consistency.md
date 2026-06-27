# TUI Phase 5: Input Mode and Help Text Consistency

## Objective

Make TUI input behavior and help text consistent across insert mode, normal mode, command mode, and dialog mode. The help surface should describe what actually works in the current mode instead of listing shortcuts that are intentionally shadowed by text input.

## Current Problem

The input layer deliberately skips bare printable character bindings in insert mode so users can type freely. This is good behavior. However, the default binding table and help text still advertise bare `/` and `?` shortcuts without clear mode qualification. In insert mode, these keys are normal text in most circumstances, so they can appear broken.

The TUI also has multiple sources of truth for shortcuts: binding tables, help lines in `App::with_config`, tests, docs, and any dialog-local handling. This creates drift as features are added.

## Design Direction

1. Preserve insert-mode typing semantics.
2. Make normal-mode/navigation shortcuts explicit.
3. Make command-mode behavior explicit.
4. Make dialog-local bindings discoverable when dialogs are active.
5. Centralize help metadata close to the binding definitions where practical.

The goal is not to redesign every keybinding. It is to stop help text from lying and to make mode semantics obvious.

## Desired Semantics

### Insert Mode

Insert mode is for prompt editing. Bare printable characters should insert text. Shortcuts should generally require modifiers or be special keys.

Examples:

- `Enter`: send prompt
- `Shift+Enter`: newline if supported by terminal
- `Esc`: cancel/normal mode/close dialog depending state
- `Ctrl+L`: model selector
- `Ctrl+T`: sidebar
- `Ctrl+S`: stash prompt
- `Ctrl+R`: restore prompt
- `Ctrl+P`: cycle model forward
- `Ctrl+Shift+P`: cycle model backward
- `Ctrl+Y`: toggle TTS
- `Ctrl+Shift+Y`: stop TTS
- `Ctrl+Shift+F`: fullscreen

Bare `/` should be documented as slash-command start only when typed at the prompt position that triggers command mode. Bare `?` should not be advertised as global help in insert mode unless implementation changes.

### Normal Mode

Normal mode can expose bare navigation/action keys.

Examples:

- `j/k`: navigate
- `g/G`: top/bottom
- `?`: help
- `q`: quit or close, depending current surface
- `i`: return to insert/focus prompt
- `:`: command mode if enabled

### Command Mode

Command mode should document slash command entry, completion navigation, accept, cancel, and history behavior.

### Dialog Mode

Each dialog should expose local bindings or a compact footer. At minimum, common bindings should be consistent:

- `Esc`: close/cancel
- `Enter`: accept/open/confirm
- arrows or `j/k`: navigate
- `/` or search key only where implemented
- `Tab` only where implemented

## Implementation Steps

### 1. Add keybinding/help metadata types

Create a type near `src/tui/input.rs`:

```rust
pub enum HelpMode {
    Insert,
    Normal,
    Command,
    Dialog,
}

pub struct HelpEntry {
    pub mode: HelpMode,
    pub key: &'static str,
    pub action: &'static str,
    pub condition: Option<&'static str>,
}
```

Add functions:

```rust
pub fn default_help_entries() -> Vec<HelpEntry>;
pub fn help_entries_for_mode(mode: HelpMode) -> Vec<HelpEntry>;
```

Do not require every binding to be generated from this in the first pass. The important part is that help text is not manually hardcoded in `App::with_config` forever.

### 2. Replace static `help_lines` construction

In `App::with_config` and `App::new_for_testing`, replace manually duplicated help lines with output derived from `help_entries_for_mode`. Use the active `vim_mode` config to select normal-mode entries when appropriate.

If changing `help_lines` shape is large, start with a helper that returns `Vec<String>`:

```rust
pub fn build_help_lines(vim_mode: bool, input_mode: InputMode) -> Vec<String>;
```

### 3. Update help dialog to show mode-aware sections

If the existing help dialog supports only lines, generate sections like:

```text
Insert mode
  Enter           Send prompt
  Shift+Enter     New line
  Ctrl+L          Model selector

Normal mode
  j/k             Navigate
  ?               Help
  i               Focus prompt

Command mode
  / at prompt     Slash command
  Tab             Complete
  Esc             Cancel
```

If the active mode can be passed into the help dialog, prioritize active mode at the top.

### 4. Fix misleading default help text

Remove or qualify these generic lines:

- `/              Focus prompt`
- `?              Help`
- `j/k or arrows  Navigate`

Replace with mode-aware forms:

- `/ at prompt start    Start slash command`
- `? in normal mode     Help`
- `j/k in normal mode   Navigate`
- `arrows              Navigate or move cursor depending focus`

### 5. Add tests for advertised behavior

Add tests that compare help entries against input behavior for the most failure-prone cases:

1. Bare `?` in insert mode inserts `?`, not help.
2. Bare `/` in insert mode inserts `/` unless prompt logic separately activates command mode at position zero.
3. Bare `?` in normal mode maps to help.
4. Bare `j/k` in insert mode inserts text if prompt is focused, but in normal mode navigates.
5. `Ctrl+L`, `Ctrl+T`, `Ctrl+P`, and `Esc` are advertised and mapped.

### 6. Dialog footer audit

For the main dialogs, check whether their displayed footer hints match their actual key handling:

- model dialog
- session dialog
- tree dialog
- permission dialog
- question dialog
- command palette
- import dialog
- research browser
- security review dialog

Do not rewrite every dialog in this phase. Fix obvious mismatches and leave notes for Phase 12 if larger UX polish is needed.

## Documentation Updates

Update any TUI architecture or user docs that list keybindings. The docs should mention that insert mode prioritizes text input and that normal/vim mode enables bare navigation keys.

## Testing Plan

Unit tests:

1. Help builder includes insert-mode modifier shortcuts.
2. Help builder qualifies bare printable keys by mode.
3. Input mapping tests for `?`, `/`, `j`, `k`, `Ctrl+L`, `Ctrl+T`, `Ctrl+P`, `Esc` in insert and normal modes.
4. If vim mode is enabled, normal-mode help includes vim-like keys.
5. If vim mode is disabled, help does not imply vim normal-mode-only behavior as global behavior.

Manual verification:

1. Open help in insert mode and confirm it does not advertise bare `?` as globally active.
2. Type `?` and `/` in prompt and confirm behavior matches help.
3. Switch to normal/vim mode and confirm navigation/help bindings match help.
4. Open key dialogs and confirm local footer hints are not misleading.

## Acceptance Criteria

- Help text is generated from a central helper rather than duplicated ad hoc in app constructors.
- Bare printable key behavior is accurately documented by mode.
- Insert-mode typing semantics remain intact.
- Normal-mode navigation/help shortcuts are discoverable.
- Tests cover the formerly misleading `/`, `?`, `j`, and `k` cases.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full keybinding editor redesign.
- Global remapping UI.
- Perfect generated help for every custom user keybind. This phase can show default/mode semantics and leave custom keybind display for a later polish pass.
