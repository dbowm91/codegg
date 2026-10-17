# TUI Panel Surface Convergence Corrective Milestone M001 — Panel manager outcome contract and registry reachability

Status: closed

Repository baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(`feat(tui): editor cursor, readable colours, in-file / search, syntax colours`, 2026-10-16)
**plus the uncommitted `file-picker-rework` M001 working-tree changes.** Re-inspect both before
starting — see §2.

Closure record: `plans/closure/tui-panel-surface-convergence-corrective/001-status.md`

Source roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` — §6 target
  architecture, §7 M001

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none required. No identity, ownership, protocol, storage, or authorization
change. `SidePanelId` variants and `config_key` spellings are consumed unchanged and frozen.

Adjacent, not owned here: `plans/subsystems/file-picker-rework-roadmap.md` M001
(conditionally closed, uncommitted — see §2).

Primary class: capability (with polish)

## 1. Objective

Make selecting a panel in `/panels` a single honest step. For every panel the manager either
fills the panel, begins a load and shows a progress state that matches, or states why it
cannot act — and never instructs the user to run a `/` command or to press `Shift+←` to
complete a gesture that is already done. Separately, prove that every panel the manager
lists can actually be opened, and remove the ones that cannot.

## 2. Why this milestone was **blocked**, and how that resolved

This was a sequencing constraint, not a defect in this plan.

`file-picker-rework` M001 was **implemented but uncommitted and unclosed** in
the working tree. It created `Component::keymap_hint()` and
`Component::render_in_panel()` and edited `src/tui/panels/hosted.rs` — both
inside this milestone's boundary. Until it committed, verified, and wrote
`plans/closure/file-picker-rework/001-status.md`:

- both trait methods might still change shape, and this milestone writes a
  `Placeholder` that must override `render_in_panel` correctly against the final
  signature;
- `Placeholder` lives in the same file that milestone was editing, so two agents
  in one worktree would collide.

**How it resolved.** `plans/closure/file-picker-rework/001-status.md` now exists
and records the milestone as *conditionally closed*, recommending that a
downstream milestone proceed "on the two seams as landed" —
`keymap_hint() -> Option<String>` and
`render_in_panel(&mut Frame, Rect, &Arc<Theme>)`, both defaulted. Both were
re-inspected and confirmed to have exactly those shapes. The collision half was
removed by working in a **separate worktree**
(`.worktrees/fork-446e57902e2e-0ae9640ab4bb432f9fa08368b8ae2faf`).

Neither milestone is committed yet, so the condition is met in substance but not
in the letter this section asked for. **M002 should re-inspect both boundary
files at the eventual commit before starting.** The `Placeholder` override was
written against the confirmed signatures; the seams were not re-shaped.

## 3. Current implementation evidence

All statements were read at `b77c8156b` with the uncommitted `file-picker-rework` M001
working-tree changes applied. Re-verify before editing; several symbols may have moved.

### 3.1 The manager's implicit three-way convention

`src/tui/commands/panels.rs`:

- `materialize_hosted_panel` returns `bool` — `true` means "the caller should select `id`",
  `false` means "a live box opened instead and owns the keyboard."
- After calling `open_panel_box`, it re-reads `app.dock_target()` to infer which of three
  things happened: the box mapped to the requested panel, to a different panel, or to none.
  That inference **is** the contract, and it is untyped.

### 3.2 The placeholder copy

`src/tui/panels/hosted.rs`, `Placeholder::render`:

```rust
Line::from(format!("No {} loaded.", self.id.label())),
Line::from(format!("Run  {}  to open it,", load_command(self.id))),
Line::from("then Shift+← to move it here."),
```

The panel is already in place; there is no box to move, and `Shift+←` is
placement-dependent. `Placeholder` also paints its own all-borders `Block` and does **not**
override `render_in_panel`, so it overwrites the host's title and `●` focus marker.

### 3.3 Panels that cannot act, and how they are reached

`open_panel_box` arms that mount no box:

| Panel | Behaviour | Reached when |
|---|---|---|
| `Memory` | toast only: names `/memory-search <query>` | always |
| `ShellShow` | toast only: names `/shell-show <id\|last>` **and** `L` | no recent shells |
| `Security` | warning only: names `/security-review` | no receipt |
| `Plan` | toast only: names `/plan` | no pending `plan_dialog` |
| `Workspace` | toast only: points at `Ctrl+O` | always |
| `Diff` | delegates to `handle_diff_command` | no git root, or no changes, or git failed |
| `Goal` / `Doctor` / `Plugins` | async starter + *"Loading…"* toast | always (Goal also: no session) |

The last three are the contradiction: the toast says a load is running while the placeholder
says nothing was opened. `ShellShow` and `Diff` state the reason and then contradict it with
the instruction on the panel.

### 3.4 The two literal branches

`materialize_hosted_panel` contains
`if matches!(id, SidePanelId::Research | SidePanelId::TaskView)` to toast *"{label} opened —
press L to dock it"* and return `false`, and a sibling `Some(other)` arm toasting *"Opened as
{label} — press L to dock it"*. Both are properties of the widget, expressed as panel ids and
message strings at the call site.

### 3.5 The unreachable panel

`SidePanelId::Workspace` is in `SidePanelId::ALL`. `WorkspaceDashboardDialog::new()` is called
only from `#[cfg(test)]`; the `Dialog::WorkspaceDashboard` mount arm in `src/tui/app/modal.rs`
is an empty comment; `Route::Workspace` owns the view and the modal path is recorded as
obsolete. `App::dock_target`'s `DialogType::WorkspaceDashboard` arm therefore cannot fire.

### 3.6 The silent adoption drop

`dock_open_box` calls `SidePanelHost::adopt(id, widget)` and **discards the returned `bool`**.
`adopt` returns `false` for a bespoke panel. `dock_target` maps `DialogType::Agent` →
`SidePanelId::Agents` and `DialogType::ProjectChat` → `SidePanelId::Chat`, both bespoke — so
that gesture unmounts and discards a box, then focuses a panel showing different content.

### 3.7 Existing coverage

`tests/tui_panel_docking.rs` asserts the shared docking gesture for **seven** boxes. Its own
comment records that the remainder "are produced by `open_info_dialog` / dedicated helpers
rather than by `open_dialog`, so a test cannot mount them without reaching into
`dialog_state`." No test sweeps `SidePanelId::ALL`, so a listed-but-unconstructible panel
passes today.

## 4. Invariants that must not regress

- `Shift+S` + `Enter` opens and focuses the selected panel in one step. This is the
  behaviour `materialize_hosted_panel` exists for; M001 strengthens its honesty and must not
  make it two steps.
- Bespoke panels keep their own `SidePanel` impls and their own content and keymaps. M001
  changes nothing about what they show.
- `SidePanelId` variants, `config_key` spellings, persisted panel bindings, placement, and
  sizing are unchanged.
- A box that is loading must not be adopted onto a frozen spinner. The `Research` /
  `TaskView` guard exists for a real reason and its protection must survive in the new typed
  form.
- If a box is already open when the manager is used, it is left alone. Adopting it would
  steal a box the user opened for another purpose.
- No panel may gain a data read, a daemon call, or a privilege.
- `/agents` continues to route through one `P::Agents` arm, so the slash command and
  `Shift+L` cannot disagree about what the panel shows.

## 5. Scope

**In:**

- `src/tui/commands/panels.rs` — typed outcome from `open_panel_box`; the `Research` /
  `TaskView` literal list; adoption-failure surfacing in `dock_open_box`.
- `src/tui/panels/hosted.rs` — `Placeholder` copy and its `render_in_panel` override.
- `SidePanelId::ALL` and `App::dock_target()` — reachability decision for `Workspace`.
- `tests/` — an exhaustive `SidePanelId::ALL` sweep and rendered-buffer assertions.
- `docs/tui.md`, `architecture/tui.md` — panel list and gesture table.

**Out:** every M002 and M003 item. No `keymap_hint` adoption for boxes other than the
placeholder's own line. No `render_in_panel` work on any dialog except `Placeholder`. No
theming change, no colour token, no shared geometry helper, no `Component` default flip.
No file-picker behaviour. No bespoke panel redesign. No new panel.

## 6. Required production changes

**`src/tui/commands/panels.rs`.** Introduce a typed outcome for `open_panel_box`:

```rust
pub(crate) enum PanelFill {
    /// A box was mounted and adopted into `id`. Select and focus it.
    Filled,
    /// An async load started; the box opens later through the existing
    /// completion path. Select `id` and show a progress state.
    Loading,
    /// This panel cannot act without something the manager cannot supply.
    Cannot(&'static str),
}
```

`materialize_hosted_panel` consumes it instead of re-deriving intent from `dock_target()`.
The two literal branches become properties: the "safe to adopt while loading" case is a
question the outcome answers, and the "opened as a different panel" message is produced by
one formatting site.

A box that is loading must map to `Loading`, never `Filled` — that is the `Research` /
`TaskView` protection, now expressed once.

`dock_open_box` must act on `adopt`'s `false`: either refuse before unmounting, or report
that the panel cannot adopt the box. It must **not** silently discard a mounted component.

**`src/tui/panels/hosted.rs`.** `Placeholder` overrides `render_in_panel` so the host's
border, title, and `●` focus marker survive, and its copy describes **state**:

- `Cannot(reason)` — the reason, in the panel, with no command to run.
- `Loading` — a progress line, matching the toast, never "no <panel> loaded".
- Default — a state line plus the real way to fill it **from inside the manager**, which
  means re-invoking the fill action rather than typing a `/` command or pressing `Shift+←`.

If the panel has no in-panel fill action, say so plainly. Do not invent a gesture.

**`SidePanelId::Workspace`.** Decided during implementation: **option 2**, remove it
from `SidePanelId::ALL` and its hotkey/config registration. Evidence and the
fail-soft verification are in the closure record §2. Of the three:


1. Give it a real entry path to `Route::Workspace` from `open_panel_box`, or
2. Remove it from `SidePanelId::ALL` and its hotkey/config registration, or
3. Keep it only if a live box can be constructed.

An unconstructible row must not stay in the manager. **This is a stop condition, not a
decision to make unilaterally** — see §14.

## 7. Ordered work packages

### W1 — Baseline-failing tests (the acceptance signal)

Land first, failing. As in `file-picker-rework` M001, these are the proof the work landed.

1. An exhaustive `SidePanelId::ALL` sweep asserting every listed panel is either bespoke or
   opens a box through `open_panel_box`, and that the resulting `dock_target()` is consistent
   with the outcome. This is the test that does not exist today.
2. A rendered-buffer assertion that a hosted `Placeholder` shows the host's panel title and
   `●` focus marker.
3. A rendered-buffer assertion that no `Placeholder` text contains `Shift+←` or a
   `/`-command instruction.
4. Per-panel assertions that the three self-contradicting panels (`Goal`, `Doctor`,
   `Plugins`) never render a "nothing loaded" line while their load is in flight.

Do not modify production code in W1.

### W2 — Placeholder rewrite

Implement `render_in_panel` and the state-driven copy. Re-run W1's tests 2–4.

### W3 — Typed outcome and the manager

Introduce `PanelFill`, rewire `materialize_hosted_panel`, collapse the two literal branches,
surface adoption failure. Re-run W1's test 1 and the existing
`tests/tui_panel_docking.rs` in full — the seven-box gesture sweep must not regress.

### W4 — Workspace reachability

Apply the §6 decision. Re-run W1's test 1.

### W5 — Guard and docs

Add the static guard so a future panel cannot be listed without being constructible, and a
guard that a hosted widget's `render` does not paint a `Block`. Any new guard MUST
distinguish hosted from modal rendering — it may not forbid the modal border every box
legitimately draws, and it must be allowed by `scripts/verify.sh quick` only if that file's
own contract supports it; otherwise register it as its own change-triggered check. Update
`docs/tui.md` (panel list, gesture table) and `architecture/tui.md` (panel host module map).

## 8. Failure, cancellation, restart, contention semantics

- A failed async load must leave the panel in `Cannot`-style state, never stuck in `Loading`.
  M001 touches only the toast and placeholder sites; the starter and its completion path are
  unchanged, and the outcome must not convert "load started" into "load succeeded".
- `Cannot` carries a static reason string. It must not carry a command to run — that is the
  defect being removed.
- No new request, task, lease, or generation fence. Concurrency behaviour is unchanged;
  M001 adds no IPC.
- If the manager is used while a box is open, that box is still left alone.

## 9. Compatibility and migration

No persisted format changes. `STORAGE_LAYOUT_VERSION` unchanged. No config key removed
without a deprecation path: if `SidePanelId::Workspace` is removed, its persisted binding
must fail soft as an unknown key rather than breaking config load — check the existing
unknown-key path before assuming it is graceful.

`/agents` routing is unchanged. `Shift+S` + `Enter` remains one step.

## 10. Required tests

- The W1 sweep over `SidePanelId::ALL` (primary acceptance signal).
- Rendered-buffer tests for the placeholder: title and focus marker present; no `Shift+←`,
  no `/`-command instruction; loading state matches the toast.
- Regression sweep proving every `DialogType` with a `dock_target` entry still docks.
- `tests/tui_panel_docking.rs` passes unchanged, including its seven-box gesture cases and
  the comment-documented empty/unbound panel hint cases.
- Focused `cargo test -p codegg <filter>` runs during development; `verify.sh quick` as the
  gate and `verify.sh full` before closure. Never `--all-features`.

## 11. Required verification commands

```bash
cargo test -p codegg panel          # focused
cargo test --test tui_panel_docking
cargo test --test tui_side_panels
scripts/verify.sh quick             # the gate
scripts/verify.sh full              # before closure
```

`verify.sh quick` is a strict subset of nothing here — note that CI runs a subset of `quick`
that omits some guards, so a green CI run does **not** imply a green `quick`. Report local
and CI evidence truthfully and separately.

**Outcome:** `quick` exit 0; clippy `-D warnings` clean. `full` exits 100 on pre-existing
failures only — proven by re-running the 12 non-passing tests with `src/` stashed to
`b77c8156`, where 11 fail identically and the 12th is the known load-sensitive timeout that
passes in isolation. Details and the full table are in the closure record §4.1.

## 12. Documentation updates

- `docs/tui.md` — panel list (Workspace decision), the panel gesture table, and the
  "Moving between the box and the panel" wording that must agree with the new placeholder.
- `architecture/tui.md` — panel host module map; the `Placeholder` description; verified
  counts if any are affected.
- `.opencode/skills/tui/SKILL.md` if its panel guidance names the removed instruction.
- `plans/registry.md` in the same commit as any status change.

## 13. Acceptance criteria (externally observable)

1. Selecting any panel in `/panels` performs exactly one action: the panel fills, a load
   starts and the panel says so, or the panel states why it cannot act.
2. No panel, in any state, displays an instruction to run a `/` command or to press
   `Shift+←`.
3. `Goal`, `Doctor`, and `Plugins` never show a "nothing loaded" line while their load runs.
4. Every hosted placeholder shows the host's panel title and `●` focus marker.
5. Every panel in `SidePanelId::ALL` is provably reachable or is no longer listed; no listed
   panel is unconstructible.
6. No adoption failure is silent.
7. No new command, key, or gesture is introduced; no existing one changes meaning.

## 14. Stop conditions

Stop and report rather than improvise if any of these hold:

- **The `SidePanelId::Workspace` decision needs a capability judgement.** Removing a panel
  from the manager is user-visible surface change; adding a `Route::Workspace` entry path is
  new behaviour. Either needs its own planning pass. Report the three options and the
  evidence for each; do not pick silently.
- **The `Component` seam signatures differ** from those in `file-picker-rework` M001 after it
  closes. Adjust the `Placeholder` override; do not re-shape the seams.
- **W1's exhaustive sweep cannot construct a panel without reaching into private
  `dialog_state`.** The existing suite documents this obstacle for the `InfoDialog` family.
  If a production-only seam is required to test it, that is a design question — report it,
  do not add test-only public doors.
- **Removing a panel would delete the only route to an authorization-gated action.**
- **Any change would alter bespoke panel content or keymaps.**

## 15. Closure evidence required

- W1's tests green, including the exhaustive `SidePanelId::ALL` sweep.
- `scripts/verify.sh quick` green locally; `scripts/verify.sh full` green locally.
- Statement of what was run locally versus what CI alone ran. No unrun verification
  presented as passing.
- `plans/closure/tui-panel-surface-convergence-corrective/001-status.md` written and
  accepted; only then is M001 `closed`.
- Registry audit: record whether M002 became newly eligible. **A closure that unblocks
  nothing is valid and must be recorded as such** — but the audit must be explicit.

## 16. Handoff notes

- §2's unblock condition was met before W1; see that section for what "met"
  meant here and what is still outstanding.
- The five work packages are ordered; W1 must fail before W2 makes it pass.
- `Placeholder` is the only dialog touched for chrome in this milestone. Do not migrate
  `InfoDialog` or any other box — that is M002, and its template depends on this milestone
  landing first.
- M002 and M003 remain unapproved. Nothing here may smuggle in their scope.