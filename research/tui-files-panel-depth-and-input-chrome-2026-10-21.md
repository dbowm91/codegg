# TUI Files Panel Depth, Panel Escape Semantics, Ctrl+C Exit, and the Status Line — Research

Date: 2026-10-21
Scope: **research only** — no product code changed. (One throwaway probe under `tests/`
was executed and deleted; `git status` is clean.)
Method: direct reading of the CodeGG source at `76690a912`, **one executed probe**
through the real walker and the real `FileTreeState`, plus two delegated sweeps
over the key-routing and layout surfaces.

Every claim below is anchored `path:line` in the CodeGG tree.

> **Outcome.** The open questions in §6.4 were answered on 2026-10-21 and the
> work was taken forward as
> `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md`
> (M001–M004). That roadmap's §2 records the decisions, and §2.1 names the
> contracts it supersedes. This document remains the research record behind
> them; where the two differ, the roadmap and the code are the truth.

---

## 0. Summary — the six asks

| # | Ask | Verdict | Primary site |
|---|---|---|---|
| 1 | `/files` must browse to infinite depth; `l`/Enter moves right; at the window edge the scope **slides** | **Partly built, and it is not the surface the user is on.** The slide exists and fires — but it lands **one level deeper** than the requested rule; `Enter` is a *different verb by documented contract* and never reaches the slide; `l` on a file is a dead key | `src/tui/components/dialogs/files.rs`, `src/tui/app/state/file_tree.rs` |
| 2 | No "maximum depth" noise at the bottom of the panel | **Present and permanent.** The walk always hits the depth cap, so `… stopped at the maximum depth of 3` is on screen at all times | `src/tui/components/dialogs/files.rs:211-222` |
| 3 | `/files` must enter INS mode and filter; Esc keeps the filter | **Not implemented in the `/files` box.** It *is* implemented in the Ctrl+T pane. The box has no `i` arm and never renders the filter field | `src/tui/components/dialogs/files.rs:318-365` |
| 4 | Ctrl+C = two presses within 3 s; first press clears the input; indicator above the main input | **Not implemented.** Today one Ctrl+C exits. A 3-second two-press mechanism already exists in the codebase and can be mirrored | `src/tui/app/mod.rs:9790-9804` |
| 5 | Remove the whole "idle" status line | **Two different rows exist.** The "idle" line is the working strip's reserved row 0, which is *not* the row directly above the input; removing it has a documented reflow cost | `src/tui/app/render.rs:1686-1695` |
| 6 | Esc on any panel must not return the selection to the main LLM | **Three Esc paths exist**; the side-panel one calls `blur()`. `Shift+H/J/K/L` already exists as the panel-to-panel gesture | `src/tui/panels/mod.rs:1912-1929` |

The single most important structural finding: **the `/files` panel is one component
(`FilesDialog`) rendered in three surfaces, and only one of them was migrated to the
shared panel contract.** Every one of asks 1–3 lands on that component.

---

## 1. The `/files` panel: one component, three surfaces, one half-migrated contract

### 1.1 The surface map

| Surface | Where | Key handler | Peek/slide | `i` filter | Filter field | Bottom-of-panel chrome |
|---|---|---|---|---|---|---|
| `/files` **modal box** (`Dialog::Files`) | `src/tui/components/dialogs/files.rs` | `FilesDialog::handle_key` (`:318-365`) | `l` → `FilesDescend` → the shared peek; **`Enter` → the retired toggle** | **none** | **never drawn** | hotkey `HINT` row (`:385-408`) **and** the truncation notice |
| `/files` **docked side panel** | same component, `render_in_panel` (`:308-316`) | same | same | none | never drawn | truncation notice only (no hotkey row) |
| **Ctrl+T tree pane** | `src/tui/components/file_tree.rs` + `src/tui/commands/file_tree.rs` | `handle_panel_key` (`src/tui/commands/file_tree.rs:632-706`) | yes, full | **yes** (`:700-703`) | yes (`src/tui/components/file_tree.rs:126-134`) | truncation notice + filter field |

All three share one `FileTreeState` (`src/tui/app/state/file_tree.rs:179-263`) and one
walker. `architecture/tui.md:2714-2716` claims this parity explicitly — *"the box, the
hosted panel, and the editor-side Ctrl+T pane all run the same `FileTreeState`
navigation, so a fix in one is a fix in all three"* — and it is **false for the filter**
and **false for `Enter`**.

### 1.2 Defect 1 — `Enter` on a directory never descends (this is the "empty directory")

`src/tui/components/dialogs/files.rs:121-138`:

```rust
    /// A directory is **peeked into** — the same motion `l` makes, and the
    /// same one the Ctrl+T pane's `l` makes — so the two surfaces cannot
    /// disagree about what `Enter` does. ...
    fn activate(&mut self) -> Option<TuiMsg> {
        let row = self.state.selected_row()?;
        match row.kind {
            TreeNodeKind::Directory => {
                self.state.toggle_selected();     // ← the retired verb
                self.reveal();
                None
            }
            TreeNodeKind::File => Some(TuiMsg::FilesOpenSelected { path: row.path }),
        }
    }
```

The doc comment states the contract; the code does the opposite. `toggle_selected`
(`src/tui/app/state/file_tree.rs:440-459`) expands/collapses **in place** and never
moves the scope. At the listing window's edge the selected directory has no children
in the listing, so the in-place expansion renders an expanded directory with nothing
under it — **exactly the reported "attempting to open a directory results in just
indicating an 'empty' directory (nothing happens)"**.

**This is a deliberate documented contract, not a bug.** `architecture/tui.md:2721`
says *"`Enter` on a directory — expand/collapse **in place** — the cursor does not
move"*, and the same file says *"Enter and `l` are deliberately different verbs"*
(`:2730`). `.opencode/skills/tui/SKILL.md:375` repeats it: *"**`Enter` expands, `l`
peeks, `h` goes up a level.** `Enter` reveals a directory's contents in place and
leaves the cursor alone."* The **request supersedes that contract** — the user wants
`l` and `Enter` to be "effectively the same thing" and to move right. So §1.2 is a
**contract change**, and the two doc sites above must change with the code. (The
`activate()` doc comment at `files.rs:121-127` is the one place that is simply wrong
today: it claims Enter is "the same motion `l` makes" while the code, the
architecture doc and the skill all say otherwise.)

`l` on the same row does reach the shared peek (`files.rs:350-356` → `TuiMsg::FilesDescend`
→ `src/tui/app/input.rs:951-957` → `commands::file_tree::descend_to`). So the two keys
the user expects to be "effectively the same thing" are **not** the same thing today.

### 1.3 Defect 2 — the scope slide lands one level deeper than the requested rule

The requested rule, worked through the user's own example: the workspace root is
`~/projects/codegg`; the user browses `~/projects/codegg/crates/codegg-client` and
presses `l` on `src`; the panel should refresh with `~/projects/codegg/crates` as the
new visual working directory.

**The window.** `MAX_LISTING_DEPTH = 3` (`src/directory_listing.rs:52`). The walker
seeds the scope at depth 0 and refuses to descend at `depth >= MAX_LISTING_DEPTH`
(`src/directory_listing.rs:230-236`), emitting nodes at `depth + 1`
(`:332-338`). So a listing rooted at a scope contains rows at scope-relative depths
**1, 2 and 3**, and **depth-3 rows are leaves** — their children were never walked.

**The implemented rule.** `src/tui/app/state/file_tree.rs:481-497`:

```rust
        if self.relative_depth(&row) >= 2 {           // row.depth >= 3
            return PeekStep::Slide {
                scope: parent_path(&row.path),          // ← the target's PARENT
                directory: row.path,
            };
        }
```

`relative_depth` is `row.depth - 1` (`:702-704`), so only a **leaf** (depth 3) slides,
and the new scope is the target's **parent**.

**The requested rule.** For the target's children to be inside the window the target
must land at scope-relative depth ≤ 2, so the *shallowest* scope that still works is
the target's **grandparent**:

| Target (absolute) | Target's parent | Target's grandparent | Window requirement | New scope (requested) | New scope (implemented) |
|---|---|---|---|---|---|
| `crates/codegg-client/src` (depth 3) | `crates/codegg-client` | `crates` | target at rel depth ≤ 2 | **`crates`** ✓ matches the user's example | `crates/codegg-client` ✗ one level deeper |

Both rules are unbounded (§2), but they differ in how much context survives: the
requested rule keeps the target's **parent** visible beside the target's siblings;
the implemented rule discards it and shows the target's siblings at the top level.

**Executed probe** (real walker, real `FileTreeState`, throwaway test, since deleted):

```
listing.truncated = true
listing.truncated_reason = Some("stopped at the maximum depth of 3")
--- root scope (scope="")
    depth=1 Directory crates expanded=false
    depth=1 File README.md expanded=false
peek(crates) = Enter { directory: "crates" }
--- after enter crates
    depth=1 Directory crates expanded=true
    depth=2 Directory crates/codegg-client expanded=false
    depth=2 Directory crates/codegg-core expanded=false
peek(codegg-client) = Enter { directory: "crates/codegg-client" }
--- after enter codegg-client
    depth=2 Directory crates/codegg-client expanded=true
    depth=3 Directory crates/codegg-client/src expanded=false
    depth=3 Directory crates/codegg-client/tests expanded=false
peek(src) = Slide { scope: "crates/codegg-client", directory: "crates/codegg-client/src" }
            (row depth=3, relative=2)
```

Three facts fall out of this that the source alone did not make obvious:

1. **The slide does fire**, and it fires exactly on the leaf row the user pressed `l`
   on. It is not dead code.
2. **It lands on `crates/codegg-client`**, one level deeper than the requested
   `crates`.
3. **`crates/codegg-client/src` has no children in the listing at all** — depth-3 rows
   are leaves — which is why an in-place expansion of it (what `Enter` still does)
   shows nothing.

### 1.4 Defect 3 — the `/files` box has no insert mode and no filter field

`TreePanelMode` (`src/tui/app/state/file_tree.rs:115-123`), the `filter` field
(`:242-247`), `rows()`' narrowing (`:283-292`), `filter_matches` (`:299-304`),
`reanchor_selection` (`:311-325`) and `TreeFilterField`
(`src/tui/file_tree.rs:235-271`) all exist and are correct.

But the `/files` box never reaches them:

- `FilesDialog::handle_key` (`src/tui/components/dialogs/files.rs:318-365`) has arms
  for `Ctrl+d`/`Ctrl+u`, `Esc`, `j`/`k`, `l`/`h`, `Enter`, `r` — **and no `i`**.
- `FilesDialog` never draws `TreeFilterField`; only the Ctrl+T renderer does
  (`src/tui/components/file_tree.rs:126-134`).
- `FilesDialog`'s own doc says so: *"A plain viewer box: no composer, no live filter,
  no mode of its own"* (`src/tui/components/dialogs/files.rs:277-283`).

The Ctrl+T pane has the whole thing already: `i` → `TreePanelMode::Insert`
(`src/tui/commands/file_tree.rs:698-703`), and the insert handler where `Esc` leaves
insert mode **keeping the filter** (`:713-744`) — which is exactly the requested
semantics.

The hard constraint on the fix: the filter must narrow the **already-loaded** rows.
`scripts/check_daemon_listing_authority.py:46-64` puts
`tui/components/dialogs/files.rs` and `tui/app/state/file_tree.rs` on
`PROTECTED_FILES` and rejects `read_dir(`, `build_tree(`, `build_tree_with_cancel(`,
`local_walk_root(` in them. `FileTreeState::rows()` already satisfies this.

### 1.5 Defect 4 — the permanent "maximum depth" notice, and the duplicated hotkey row

At the bottom of the listing, the box appends the truncation reason
(`src/tui/components/dialogs/files.rs:211-222`):

```rust
            if let Some(reason) = self.state.truncated.as_deref() {
                lines.push(Line::from(Span::styled(
                    format!("… {reason}"),
                    Style::default().fg(theme.warning),
                )));
```

Because the walk **always** hits the depth cap in any repository with a third level
(the probe printed `truncated = true` for a two-directory sample), this row is
permanently on screen and always reads `… stopped at the maximum depth of 3`. Under
the requested contract this notice is *misleading*: the depth cap is no longer a user
limit, because the scope slides.

Separately, the **modal** box reserves a row for a hotkey line
(`src/tui/components/dialogs/files.rs:385-408`) rendering `HINT`
(`:47-48`), and `keymap_hint()` (`:288-293`) hands **the same string** to the panel
host, which prints it above the composer
(`src/tui/panels/hosted.rs:158-178`: `"{panel} · {keys} · Shift+← back"`). So the
user's "the key mapping for the panel already exists above the main input" is exactly
right, and the box's own bottom row is the duplicate. The hosted variant already
omits it (`render_in_panel`, `:308-316`) — only the modal box still prints it.

### 1.6 What the wire budget does and does not do

`MAX_DIRECTORY_LISTING_RESPONSE_BYTES = 512 KiB`
(`crates/codegg-protocol/src/document.rs:105`) is enforced on the **complete
serialized response** (`src/core/daemon_documents.rs:1181-1211`), and the policy is to
**trim trailing whole top-level nodes** — never to prune a retained node's children
(`:1184-1190`). So the budget can remove *sibling roots* from a listing; it cannot
make a directory empty. It is **not** the cause of the reported symptom, and it is
not the constraint on the fix. The depth window is.

---

## 2. The requested "infinite depth" contract, stated precisely

**Invariant that must hold:** the workspace root never changes. The scope is a *view*
onto the workspace, carried as `WorkspaceDirectoryListRequestDto::prefix`
(`src/tui/commands/file_tree.rs:217-226`) — "a visual trick to accommodate infinite
directory depth, not actually shifting working directory" ✓. The daemon re-resolves
the canonical root on every request (`src/core/daemon_documents.rs:1086-1104`), so the
scope can never escape the workspace.

**The rule, in one sentence:** *`l`/Enter on a directory expands it in place and
moves the cursor into it; if the directory's children fall outside the three-level
listing window, the scope slides out to the shallowest ancestor that puts those
children back inside the window, the directory is expanded there, and the cursor
enters it.*

**Why it is unbounded.** Each slide moves the scope **one level inward** (the target's
grandparent is one level deeper than the scope that made the target a depth-3 leaf),
and after the slide the target sits at scope-relative depth 2 with its children at
depth 3 — inside the window. The next edge is therefore one level further in. Repeat
indefinitely: depth is bounded per listing, not per session.

**Two candidate models, and which one the request implies.**

- **Model A — peek (implemented).** `l` expands in place; the scope moves only at the
  window edge. The window is measured from the scope.
- **Model B — browser.** `l` re-roots the scope onto the selected directory.

Model B makes the slide *unnecessary*: after a re-root the target's children are always
at scope-relative depth 1. But the user's example — the scope becoming `crates` rather
than the target itself — only reconciles with **Model A plus a grandparent slide**.
So the request is Model A, and the change is the slide target, not the motion.

**The one genuinely new verb:** the user allows `l` to open a file in the editor
("l could also open a file in file editor"). Today `peek_into` returns
`PeekStep::None` for a file (`src/tui/app/state/file_tree.rs:485-487`), so `l` on a
file is a **dead key**. Under the requested contract `l` on a file should be the same
as `Enter` on a file.

**What must not change:** the daemon owns the listing (`check_daemon_listing_authority.py`),
the scope travels with the request, and the panel's filter is never a scope
(`src/tui/app/state/file_tree.rs:37-44`).

---

## 3. Ctrl+C — two presses within three seconds

### 3.1 What happens today

- The binding: `src/tui/input.rs:809-814` maps `(CONTROL, Char('c'))` to
  `InputAction::ForceQuit`; dispatched at `src/tui/app/mod.rs:4326`.
- `App::force_quit` (`src/tui/app/mod.rs:9790-9804`):

```rust
    fn force_quit(&mut self) {
        self.cancel();
        if self.ui_state.dialog.is_open() || self.prompt_state.show_completions {
            return;
        }
        self.shutdown_app();
    }
```

- `App::cancel` (`src/tui/app/mod.rs:9760-9783`) unwinds one layer: close a dialog →
  hide completions → `Insert`→`Normal` → else **clear a non-empty draft**.

So today: **one Ctrl+C exits the program**, and the "clear the draft" behaviour in
`cancel` is unreachable on that path (the Insert→Normal step fires first, and
`shutdown_app` runs regardless). Inside a modal dialog Ctrl+C never reaches
`force_quit` at all — the dialog consumes the key before the binding map
(`src/tui/app/mod.rs:3927-3939`). One exception already exists: Ctrl+C **copies a
live text selection** and is swallowed by that (`:3897-3907`).

### 3.2 The 3-second two-press mechanism already exists — copy it

The stop-agent confirmation is the same protocol the user is asking for:

| Piece | Site |
|---|---|
| the arm | `src/tui/app/state/prompt.rs:76` — `pub stop_armed_at: Option<std::time::Instant>` |
| the window | `src/tui/app/state/prompt.rs:108` — `STOP_ARM_WINDOW = Duration::from_secs(3)` |
| arm / expire / predicate | `src/tui/commands/turn_control.rs:207`, `:237`, `:248` |
| the "press again" wording | `src/tui/app/mod.rs:13325-13327` — `llm_buffer_key_hints` returns `"press {key} again to stop agent"` |

The Ctrl+C arm should be a sibling `Option<std::time::Instant>` on `PromptState` with
the same three functions, and the indicator should be produced in the same place —
**the hint row directly above the main input** — which already renders the analogous
stop-agent confirmation. That is the requested location and it needs no new surface.

### 3.3 The blocking constraint: the event loop is not awake when idle

`src/tui/runtime/event_loop.rs:67-89`:

```rust
pub(crate) fn animation_interval(app: &app::App) -> Option<Duration> {
    if app.streaming_active || matches!(app.session_state.session_status, SessionStatus::Working) {
        Some(...)
    } else if !app.messages_state.toasts.is_empty() {
        Some(TOAST_RENDER_INTERVAL)
    } else if crate::tui::commands::interactive_terminal::terminal_pull_armed(app) {
        Some(...)
    } else {
        None
    }
}
```

`None` means the loop parks on `pending()` and **stops waking**. The stop-agent arm
survives that only because a turn is `Working` (80 ms tick) and it posts a toast. A
Ctrl+C arm raised while the session is **idle** has neither: **unless `animation_interval`
gains an arming contributor, the 3-second expiry never fires and the indicator stays on
screen forever.** This is the single most important implementation constraint in §3,
and it has exactly two candidate fixes:

1. add a contributor to `animation_interval` for an armed Ctrl+C (or for the stop arm
   too, which has the same latent hole), **or**
2. arm the toast as well (the stop arm does — `turn_control.rs:228-230`), which keeps
   the loop ticking at `TOAST_RENDER_INTERVAL` and gives `ToastManager::tick` a reason
   to re-render.

### 3.4 The precedence questions that must be answered before coding

| Situation | Today | Question |
|---|---|---|
| Live text selection | Ctrl+C copies (`:3897-3907`) | Must stay: a copy must not arm or exit |
| A modal dialog is open | Ctrl+C never reaches `force_quit` | Should the arm work inside a dialog? |
| Command (`/`) mode | `handle_command_key` has no `ForceQuit` arm — Ctrl+C is inert | Same question |
| Draft has text | `cancel()` clears it (in Normal mode) | The requested rule: first Ctrl+C **clears the field** and arms; second within 3 s exits |
| Draft is empty | — | The requested rule: first Ctrl+C **arms only**, with the same indicator |
| Lapsed arm | — | The indicator must disappear after 3 s and the program must not exit |

Note the requested rule is a **strict superset** of the current `cancel()` behaviour
("the first ctrl+c on any input field should clear it"), so `cancel()`'s draft-clearing
arm should move to the *new* protocol rather than remain a separate path.

---

## 4. Removing the "idle" line — three rows, not one

The layout of the main buffer (`src/tui/app/render.rs:354-359`, carve arithmetic
`:226-261`, `session_layout` at `src/tui/layout.rs:353-365`):

```
[header]
[transcript                Min(10)]
[subagent tab bar]
[working strip row 0   ← "idle" / "working…" / "press Ctrl+Alt+X to stop"]   :354
[hint row               ← panel keymap / composer verbs / confirmations]      :355
[todo strip]                                                                   :356
[tasks strip]                                                                  :357
[prompt / input box]                                                           :358
[footer status bar]                                                            :359
```

**The "idle" line is the working strip's reserved row 0. It is NOT the row directly
above the input** — the hint row is. The user's two asks are therefore compatible, and
the natural home for the Ctrl+C indicator is the **hint row**, which is exactly where
the stop-agent confirmation already lives (`src/tui/app/mod.rs:13319-13331`).

### 4.1 What row 0 carries

`src/tui/app/render.rs:1735-1845`. Its body precedence is `last_error` → `stopped_turn`
→ working phase → `awaiting N task(s)` → `idle` (`:1797-1801` prints `idle`), with
trailing counters appended (`:1804-1841`). It also carries the **stop-agent hint**
(`:1742-1755`) and the clickable subagent rows below it (`:1864-1872`, whose hit
rects assume `area.y + 1 + index`).

### 4.2 The reflow constraint and the options

`working_strip_rows()` (`src/tui/app/render.rs:1686-1695`) returns `1 + shown +
overflow`. The leading `1` is the reservation, and it exists for a documented reason
(AGENTS.md: *"The working strip row 0 is reserved unconditionally … Its height is
subtracted from the transcript"*; `architecture/tui.md:643-651`;
`.opencode/skills/tui/SKILL.md:993-1001`): without it the transcript resizes every
time a subagent starts or stops. `tests/tui_main_buffer.rs:228-264`
(`the_status_row_is_reserved_in_every_state_so_the_transcript_never_reflows`) pins
this, and `tests/tui_main_buffer.rs:266-284` pins the literal `idle` text.

| Option | Frees the line? | Cost |
|---|---|---|
| **A.** Delete row 0 and let the strip be content-driven | yes | The strip's height changes when subagents appear → **the transcript reflows**, the exact defect the reservation prevents |
| **B.** Keep the height, print nothing | no | The line is still consumed; the user's goal ("wasting a line of vertical space") is unmet |
| **C.** Move the subagent rows into the existing subagent tab bar and drop the whole strip | yes, permanently | More work; the tab bar and the strip are different widgets today |
| **D.** Render row 0 only when it has something to say | partly | Same reflow as A, on a different trigger |

**A + C together** is the only shape that both frees the row and keeps the transcript
stable. That is a decision, not an implementation detail, and it belongs to the user.

### 4.3 One landmine

`expire_pending_stop_armed(self)` is called **from inside** `render_working_strip`
(`src/tui/app/render.rs:1710`). If the strip is removed, that call site disappears and
**the stop-agent confirmation would never expire**. It must be relocated (to the
render pump, or to the same `animation_interval` contributor §3.3 needs).

---

## 5. Esc on a panel must not return the selection to the main LLM

### 5.1 The three Esc paths

| Surface | Site | Effect today |
|---|---|---|
| Focused **side panel** | `src/tui/panels/mod.rs:1916-1929` | `is_exit_key` → `self.blur()` → focus returns to the composer |
| **Ctrl+T tree pane** | `src/tui/commands/file_tree.rs:663-666` | `set_focus(app, false)` → `tree_focused = false` |
| **`/files` modal box** | `src/tui/components/dialogs/files.rs:337` | `TuiMsg::CloseDialog` |

### 5.2 What "return the selection to the main LLM" is in code

For the side panel it is `SidePanelHost::blur()` plus
`restore_composer_mode_if_unfocused` (`src/tui/commands/panels.rs:713`,
`:1277-1283`): the keyboard goes back to the composer, the composer's parked
NOR/INS mode is restored, and — per `architecture/tui.md:3315-3317` — the derived
`[BUILD]`/`[CHAT]` target is re-armed. It is a focus move, not a message.

### 5.3 The gesture that replaces it already exists

`Shift+H/J/K/L` is the panel-to-panel and panel-to-buffer gesture:
`src/tui/input.rs:731-750` (bindings), `src/tui/panels/mod.rs:400-425` (`direction_of`),
`src/tui/commands/panels.rs:761-801` (`move_directionally` → `focus_main_buffer`).
It is resolved by the host **before** the panel sees the key
(`src/tui/panels/mod.rs:1895-1908`), so it is unaffected by a panel swallowing Esc.

The host already has the seam this change needs: `owns_exit_key`
(`src/tui/panels/mod.rs:1022`), which lets a panel take first refusal of the exit key
before the host blurs.

### 5.4 The decisions this needs

1. Does "any panel" include the **modal `/files` box**? If Esc must not close it,
   what does — `q`? `Ctrl+T`? (Today `is_exit_key` covers **both** `Esc` and `q`
   (`src/tui/panels/mod.rs:1968-1973`); only `Esc` is named in the request.)
2. Should Esc in a panel become a **no-op**, or should it do something else (close the
   panel's own overlay, e.g. the completion popup)?
3. The Ctrl+T tree pane's Esc → `set_focus(false)` is the same shape of behaviour and
   should probably move with it.
4. `direction_of` matches `Char('H')` **without requiring the SHIFT modifier**
   (`src/tui/panels/mod.rs:419-422`), while the composer binds both encodings
   (`src/tui/input.rs:731-750`). A terminal that reports `Shift+H` as `Char('h') +
   SHIFT` is missed by the panel path — worth fixing in the same change, since the
   gesture becomes the *only* way out of a panel.

---

## 6. Change surface, guard impact, and the open decisions

### 6.1 Files that would change

| File | Why |
|---|---|
| `src/tui/components/dialogs/files.rs` | `Enter` → descend (`:128-138`); add `i` + insert handling (`:318-365`); draw `TreeFilterField`; drop the bottom `HINT` row (`:385-408`); the truncation notice (`:211-222`) |
| `src/tui/app/state/file_tree.rs` | the slide target rule (`:481-497`); `l` on a file → open (`:485-487`) |
| `src/tui/commands/file_tree.rs` | `descend_to` (`:495-517`) to carry the new slide target; `l` on a file |
| `src/tui/app/mod.rs` | the Ctrl+C arm (`force_quit` `:9790-9804`, `cancel` `:9760-9783`); the indicator in `llm_buffer_key_hints` (`:13319-13331`) |
| `src/tui/app/state/prompt.rs` | the arm field + window (`:69-108`) |
| `src/tui/commands/turn_control.rs` (or a sibling module) | the arm / expire / predicate trio (`:207-254`) |
| `src/tui/runtime/event_loop.rs` | the wake contributor (`:67-89`) |
| `src/tui/app/render.rs` | the "idle" row (`:1686-1695`, `:1735-1845`, `:1864-1872`); relocating `expire_pending_stop_armed` (`:1710`) |
| `src/tui/panels/mod.rs` | Esc no longer blurs (`:1916-1929`); possibly `direction_of`'s SHIFT requirement (`:400-425`) |
| `src/tui/commands/file_tree.rs` | the tree pane's Esc (`:663-666`) |

### 6.2 Guards

- `scripts/check_daemon_listing_authority.py` — **binding on the filter work.** The
  filter must narrow loaded rows (`FileTreeState::rows()`), never re-walk; the four
  protected files include `tui/components/dialogs/files.rs` and
  `tui/app/state/file_tree.rs`.
- `scripts/check_tui_composer_key_authority.py` — binds the composer claim table
  (`src/tui/composer_claims.rs`) and `src/tui/input.rs`. `H/J/K/L` are unclaimed
  today, so a panel-local `Shift+H/J/K/L` is outside its scope — but any edit to
  `insert_panel_direction_bindings` must keep the two encodings consistent.
- The eight `check_tui_editor_*` guards — none of them constrains the changes above
  (they read `src/tui/editor.rs` and `src/tui/commands/editor.rs`).
- No guard references the working strip, row 0, `idle`, or Ctrl+C.

### 6.3 Documentation that must move with the code

- `architecture/tui.md:2714-2716` — the "a fix in one is a fix in all three" parity
  claim (currently false for the filter and for `Enter`).
- `architecture/tui.md:2721` and `:2730` — the `/files` key table's **"Enter expands in
  place, `l` peeks"** contract, and the sentence that calls them "deliberately
  different verbs". Both are superseded by the request (§1.2).
- `architecture/tui.md:2726` — the `/files` `i` row (documents a verb the box does
  not implement) and `:2728` — the `Esc | close` row.
- `architecture/tui.md:643-651` — the working strip's reserved row 0.
- `architecture/tui.md:4107`, `:4133-4142` — the keymap tables for `Ctrl+C → ForceQuit`
  and "a dialog needs two presses to leave".
- `.opencode/skills/tui/SKILL.md:364-368` — *"Esc never reaches a hosted widget (the
  host claims Esc/q as its own exit and blurs first)"* — the contract §5 changes.
- `.opencode/skills/tui/SKILL.md:375-384` — the `/files` navigator rules.
- `.opencode/skills/tui/SKILL.md:993-1001` — the reserved row 0.
- `AGENTS.md` — the working strip row 0 convention.
- `docs/tui.md` — the user-facing "The bottom row" section added by the previous
  workstream, and the panel keymap description.

### 6.4 The open decisions, each with a recommended default

| # | Decision | Recommended default |
|---|---|---|
| D1 | Does `l`/Enter on a directory **re-root** the scope (browser) or **expand in place** (peek)? | **Peek**, with the scope sliding at the edge — the user's example only reconciles with this |
| D2 | Where does the slide land — the target's **parent** (implemented) or **grandparent** (requested example)? | **Grandparent** — it matches the user's stated outcome and keeps one more ancestor visible |
| D3 | Does `l` on a **file** open it? | **Yes** — the user explicitly allows it, and it removes a dead key |
| D4 | The bottom-of-panel rows: drop the hotkey `HINT` row, the truncation notice, or both? | **Both** — the hotkey row duplicates the composer hint row; the notice is permanently on screen and becomes misleading once the depth is not a limit |
| D5 | Does the truncation notice disappear entirely, or only when the scope has slid past the edge? | Keep a notice **only** when the listing was cut for a reason the user cannot slide past (node/examined budget), not for the depth window |
| D6 | "Remove the idle line": delete the strip row (reflow) or drop the whole strip into the subagent tab bar? | **Drop the whole strip** (option C) — the only shape that frees the row without reflowing the transcript |
| D7 | Where does the Ctrl+C indicator live? | The **hint row** above the input — the same row that already carries "press Ctrl+Alt+X again to stop agent" |
| D8 | Does the Ctrl+C arm work inside a modal dialog and in `/`-command mode? | **No** in command mode (it is a text field); **yes** in a dialog, if the dialog is not consuming Esc-like keys for itself |
| D9 | Does "any panel" include the modal `/files` box, and does `q` still exit a panel? | **Esc stops exiting; `q` keeps working** — the request names Esc only, and `q` is the typed fallback |
| D10 | Is the scope slide's daemon round-trip acceptable (a `l` at the edge costs one listing)? | Yes — it is already the implementation, and the daemon owns the listing |

### 6.5 Suggested work packages (for a roadmap, if this is taken forward)

1. **Panel verbs** — `Enter` descends; `l` opens a file; the slide lands on the
   grandparent; `l`/`Enter` parity across the box, the hosted panel and the Ctrl+T pane.
2. **Panel filter** — `i` in the box, `TreeFilterField` rendered, Esc keeps the filter,
   the filter narrows loaded rows only.
3. **Panel chrome** — remove the bottom hotkey row and the depth notice.
4. **Ctrl+C protocol** — the arm, the indicator, the expiry, the wake contributor.
5. **Panel escape** — Esc no longer blurs; `Shift+H/J/K/L` becomes the only way out;
   the tree pane's Esc moves with it.
6. **Status line** — remove the working strip's reserved row 0 without reflowing the
   transcript.
7. **Docs + guards** — the `architecture/tui.md` and skill sections in §6.3, plus the
   `check_daemon_listing_authority` self-test if the filter moves.

Each is independently shippable; 1–3 are one surface and should land together, and
4–6 are independent of the panel work.
