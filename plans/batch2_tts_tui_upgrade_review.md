# TTS & TUI & Upgrade Architecture Review

## Verified Claims

### TTS Module (`src/tts/mod.rs`)
- **Location correct**: `src/tts/`
- **Tts struct**: Uses `Mutex<AtomicBool>` for thread-safe interior mutability - CORRECT
- **Tts::new()**: Creates with `speaking: Mutex::new(AtomicBool::new(false))` - CORRECT
- **Tts::init()**: Only handles `TtsProvider::None` as a no-op - CORRECT
- **Tts::speak()**: Validates non-empty text, returns `AppError::Io` for empty strings - CORRECT
- **Tts::stop()**: Uses `pkill say`, checks speaking state first, returns early if not speaking - CORRECT
- **is_speaking()**: Returns `bool` (not `Result<bool, AppError>`) - CORRECT
- **TtsProvider**: Has `#[default] None` variant only - CORRECT
- **TtsEngine trait**: `send + sync`, `async fn speak/stop`, `fn is_speaking` - CORRECT
- **macOS only**: Hardcoded `say` command via `tokio::process::Command` - CORRECT
- **stop() error handling**: Returns `Err(AppError::Io)` on `pkill` failure - CORRECT (known issue per AGENTS.md is fixed)

### TUI Module

#### Directory Structure (`src/tui/`)
- **app/mod.rs**: 6003 lines (doc says ~5978, slight discrepancy but close)
- **app/types.rs**: Contains `Dialog`, `TuiMsg`, `SessionStatus`, etc. - CORRECT
- **State domains**: `agent.rs`, `dialog.rs`, `messages.rs`, `prompt.rs`, `session.rs`, `ui.rs` - CORRECT

#### UiState (`src/tui/app/state/ui.rs`)
- **Field count**: 26 fields - CORRECT
- **Fields verified**:
  - `theme: Arc<Theme>` (line 29) ✓
  - `layout: TuiLayout` (line 31) ✓
  - `sidebar_visible: bool` (line 33) ✓
  - `auto_scroll: bool` (line 35) ✓
  - `show_thinking: bool` (line 37) ✓
  - `show_timestamps: bool` (line 39) ✓
  - `routes: RouteManager` (line 41) ✓
  - `dialog: Dialog` (line 43) ✓
  - `command_mode: bool` (line 45) ✓
  - `input_mode: InputMode` (line 47) ✓
  - `shutdown_tx: Option<broadcast::Sender<()>>` (line 49) ✓
  - `help_lines: Vec<String>` (line 51) ✓
  - `bindings: HashMap<...>` (line 53) ✓
  - `keybinds: Option<KeybindConfig>` (line 55) ✓
  - `remote_mode: bool` (line 57) ✓
  - `remote_status: Option<String>` (line 58) ✓
  - `running: bool` (line 60) ✓
  - `timeline_visible: bool` (line 62) ✓
  - `timeline_selected: usize` (line 63) ✓
  - `render_panic_count: usize` (line 64) ✓
  - `last_render_error: Option<String>` (line 65) ✓
  - `tts: Tts` (line 67) ✓
  - `tts_enabled: bool` (line 69) ✓
  - `fullscreen: bool` (line 71) ✓
  - `dirty_regions: Vec<Rect>` (line 73) ✓
  - **`resize_debounce: Option<std::time::Instant>` (line 75)** - present in code but NOT documented

#### Dialog variants (`types.rs`)
- `Dialog` enum has 23 variants (including None) - verified at lines 1-26
- Actual variant order: None, Model, Agent, Session, Help, Tree, Theme, Question, Permission, Mcp, Keybind, Share, Import, Template, Connect, Context, Cost, Usage, **Stats**, Goto, Plan, Diff, Confirm

#### DialogType (`component.rs`)
- `DialogType` enum has 23 variants (lines 21-46), including `Stats` at line 41
- `From<DialogType> for Dialog` conversion (lines 54-82) includes `Stats` mapping - CORRECT

#### Component trait (`component.rs`)
- `pub trait Component: Send + Any` (line 84) - doc says just `Send` but code uses `Send + Any`

#### InputMode
- `Insert` (default), `Normal` - CORRECT

#### FocusManager
- `VecDeque<Box<dyn Component>>` stack - CORRECT
- `push`, `pop`, `top`, `top_mut`, `handle_key`, `active_dialog_type` - CORRECT

#### Theme count
- **31 themes** verified (26 dark + 5 light) - doc says 31 - CORRECT

#### Routes
- `Route::Home`, `Route::Session(String)` - CORRECT

#### Keyboard shortcuts
- `Ctrl+Y` for TTS toggle, `Ctrl+Shift+Y` for stop - CORRECT

#### SpinnerWidget
- Frames: `["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"]` - CORRECT

### Upgrade Module (`src/upgrade/mod.rs`)
- **Location correct**: `src/upgrade/`
- **VersionInfo struct**: `current`, `latest`, `needs_update` - CORRECT
- **check_for_updates()**: 10s timeout, GitHub API query, User-Agent header - CORRECT
- **upgrade() function**: Defined but not called by CLI - CORRECT
- **`INSTALL_VERSION` env var** set with `v{latest}` - CORRECT

## Incorrect/Stale Claims

### TUI Documentation Issues

1. **UiState missing `resize_debounce` field**: The actual `UiState` struct has a 26th field `resize_debounce: Option<std::time::Instant>` (line 75) that is NOT documented in the architecture file.

2. **Component trait bounds**: `architecture/tui.md` line 284 shows `pub trait Component: Send` but actual code at `component.rs:84` shows `pub trait Component: Send + Any`.

3. **Dialog enum missing Stats in documentation**: The documented list of Dialog variants in `tui.md` (lines 189-196) omits `Stats` - the actual Dialog enum does include `Stats` at type.rs line 21.

4. **`app/mod.rs` line count**: Documentation says "App struct (~5978 lines)" but actual file has 6003 lines.

## Bugs Found

No critical bugs found. Code is consistent with itself.

## Improvements Identified

1. **Update tui.md UiState struct documentation** to include `resize_debounce` field (line 75 of ui.rs).

2. **Update tui.md Component trait documentation** to include `+ Any` bound.

3. **Update tui.md Dialog enum documentation** to include `Stats` variant.

4. **Update app/mod.rs line count** from "~5978 lines" to "6003 lines" in documentation.

## Stale References

1. **UiState field `resize_debounce` not documented**: This field exists in actual code but is absent from architecture docs.

2. **upgrade.md**: No stale information found; documentation is accurate.

## Recommendations

1. Add `resize_debounce: Option<std::time::Instant>` to the UiState documentation after `dirty_regions`.

2. Update Component trait docs: `pub trait Component: Send + Any` (not just `Send`).

3. Add `Stats` to the Dialog enum example in tui.md (line 193 area).

4. Update app/mod.rs line count reference from "~5978" to "6003".

5. Consider whether the `/stats` command (command.rs:147) with `Dialog::Stats` triggers a working Stats dialog or if this is dead code. No StatsDialog implementation exists in `src/tui/components/dialogs/`.
