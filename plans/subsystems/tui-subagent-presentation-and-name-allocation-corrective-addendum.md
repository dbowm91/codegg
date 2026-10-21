# TUI Subagent Presentation and Name Allocation — Corrective Addendum

Status: active

Source roadmaps (neither is reopened by this addendum):

- `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md` — M001–M005 landed but were never closed; superseded for corrective work by the convergence addendum.
- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md` — C001–C007 **all closed**; their closure records are immutable and this addendum cites them as history, not as reopened scope.

Repository baseline: `31a412844fbbf36055b29245075121035eefc385`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles` — §4.4 *Frontends render projections*, §4.6 *Progressive disclosure*, §4.7 *Correctness before transparent magic*.
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs: **none required.** This addendum changes no identity, ownership boundary, protocol, storage layout, or authorization semantics. If an implementation pass discovers that one must change, that milestone **stops** and requests a separate ADR and plan.

Milestone numbering: **P001–P004**, local to this addendum and deliberately distinct from the parent roadmap's `M` series and the convergence addendum's `C` series, so a milestone reference is unambiguous across the three documents.

---

## 1. Purpose and ownership boundary

The parent workstream made delegated runs **visible**. This addendum makes them **legible** and correctly **named**.

Two independent defects are in scope, and they are not the same defect:

1. **Presentation.** The subagent surfaces render, but they render the wrong thing in the wrong place. The `SESSIONS` side panel's subagent rows spend their single column on a prose descriptor instead of the three facts a reader scans for (name, agent type, status). Keyboard navigation lands on todo rows that have no action behind them. The subagent tab's body is drawn **over** the main transcript instead of **instead of** it, so the reader sees the main agent's conversation showing through a subagent's own view. The tab strip paints foreground only over a row carved out of the viewport, so it is the one surface in the TUI with no opaque background. The main entry reads `Chat` rather than `Main`, has no tab glyph, and the strip states none of the three keys that operate it.
2. **Naming.** A subagent that collides with a held name takes a numeric variant (`Sally-2`) even though roughly forty-five other pool names are free. The suffix is a fallback; today it is the primary mechanism.

**Ownership boundary.** The TUI owns viewport pixels, keyboard routing, and the presentation of already-projected state. The daemon owns run identity, delegation, and the projection. This addendum changes **no** scheduler, job, storage, projection, or execution authority: `scripts/check_execution_ownership.py` and `scripts/check_scheduler_bypass.py` must stay green and `docs/execution-ownership.toml` must not change. The one production change outside `src/tui/` is the **name allocation policy** in `src/agent/subagent_names/registry.rs`, which is a pure selection rule over an in-memory active-lease set — it introduces no persistence, no protocol field, and no new identity.

**This addendum does not add a transcript.** See P001 §3.2: no transcript exists to show, and creating one is a different, larger, out-of-scope workstream.

## 2. Work classification

### Invariants

- **I1 — Projection-derived rows.** Every subagent row rendered in the TUI originates in a `SessionProjectionSnapshot` field. No client-side fabrication, no client-side name minting, no frontend pool lookup. Enforced by `scripts/check_tui_subagent_name_pool_isolation.py`.
- **I2 — A surface owns the pixels it covers.** When the subagent tab is active, the viewport rect contains the subagent view and nothing else. No surface may be legible through another.
- **I3 — Every TUI surface paints an opaque background.** A carved-out row is still a row: it must be filled before it is decorated.
- **I4 — Panel input authority.** Panel keys resolve through `SidebarActivation` → `PanelKey::Consumed` / `PanelKey::Intent(TuiMsg)` → `src/tui/app/input.rs`. Enforced by `scripts/check_tui_transport_authority.py`.
- **I5 — A focusable row has an action.** Keyboard focus never rests on a row whose activation is a no-op. (New here; it is the invariant P003 restores.)
- **I6 — Tab ordering is not an execution boundary.** A subagent tab cancels nothing.
- **I7 — The name is drawn once, at spawn.** The draw site stays in `src/agent/worker.rs`; the reducer and the TUI never draw a name. Replay determinism depends on this and is not negotiable.

### Capabilities

- **K1 — The row says what a reader scans for.** A subagent row shows name, agent type, and status.
- **K2 — Navigation moves between things you can act on.** `j`/`k`/arrows skip rows with no action; section headers stay selectable.
- **K3 — The tab is the subagent's own surface.** Opening a subagent tab replaces the transcript; the reader sees the subagent's view, not a ghost of the main conversation.
- **K4 — The tab strip states its own keys.** With tabs open, the strip shows `⇧P`/`⇧N`/`⇧X` on the row it already occupies.
- **K5 — The persona has one home.** The tab bar carries the persona for the active tab; the detail surfaces keep it; the summary row does not.
- **K6 — A new subagent gets a new name.** A free pool name is preferred over a numeric variant of a taken one.

### Infrastructure

- **F1 (carried, landed)** — `SubagentNameRegistry` and the 46-entry curated pool (`assets/subagent-names/names.toml` → `src/agent/subagent_names/generated.rs`, regenerated by `scripts/generate_subagent_names.py`). Verified in sync at baseline.

### Polish

- **P1** — `MAIN_TAB_LABEL` reads `Main`, and the main entry carries a tab glyph so it reads as a tab rather than as plain text.

## 3. Non-goals

- **A transcript surface.** See P001 §3.2.
- **Re-implementing the parent roadmap's M001–M005 or the convergence addendum's C001–C007.** Both landed; this addendum re-verifies and corrects.
- **A second list of subagents.** The side panel, the `Ctrl+T` overlay, the `/sessions` dialog, the working strip, and the tab all read the **same** derived `SubagentRow` vector. Presentation changes; the derivation does not.
- **Changing what a persona is.** The pool's `profile`/`usefulness` text, its byte bound, and its projection path are unchanged. Only where it is *displayed* changes.
- **Widening the persona to every tab.** One row cannot carry one persona per tab; only the active tab's persona is shown. See P002 §4.
- **Moving the persona off the detail surfaces.** The `Ctrl+T` overlay, the `/sessions` dialog, the working strip, and the tab body keep it.
- **Rebinding `Shift+W`.** It is `OpenWorkspaceDashboard` (`/workspace`), deliberately aligned under the parent's P1, and recorded as withdrawn decision **D-C3** in the convergence addendum. The close key is `Shift+X` and already exists.
- **Changing `MAX_OPEN_SUBAGENT_TABS`, the tab-capacity refusal, or the detached-tab liveness model.**
- **Any scheduler, job, storage, projection, or authorization change.**
- **Changing the curated pool's contents.** P004 changes the *selection order* over the existing 46 names, not the names.

## 4. Current state (repo evidence at `31a4128`)

### 4.0 A naming note: which "sessions" surface this addendum means

Two different surfaces are called "sessions", and the parent workstream was misled by exactly this before:

| Surface | Identity | Title | Subagent rows |
|---|---|---|---|
| Session **side panel** | `SidePanelId::Session` (`SidebarWidget`) | `SESSIONS` | **yes** — this is the one this addendum means |
| Session **switcher dialog** | `Dialog::Session` | `Sessions` | no |

The convergence addendum's C002 closed the gap by **also** adding subagent and job sections to the `/sessions` dialog, so both surfaces now list live work — but they are still different widgets with different selection models.

Everything below that says "the side panel", "the `SESSIONS` panel", or "the sessions panel" means the **side panel** (`src/tui/components/sidebar.rs`), where the todo rows live and where `j`/`k`/arrows navigate. Where the `/sessions` **dialog** is meant, it is named explicitly. The requested navigation change is a side-panel change: the `/sessions` dialog's own cursor model is out of scope and untouched.

### 4.1 The subagent tab is drawn **over** the transcript, not instead of it

> **Closed by P001.** The defect described below was fixed; the viewport rect
> now has exactly one owner. This section is kept as the **baseline record**:
> its line numbers are pinned to `31a4128`, and `src/tui/app/render.rs` has
> moved since. The closure record is
> `plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md`.

`src/tui/app/render.rs:331` renders the viewport and `:337` then renders the subagent view **into the same rect**:

```rust
self.render_viewport(frame, viewport_chunk);
// An open subagent buffer replaces the transcript in the
// same rect rather than opening a modal over it: Esc then has
// exactly one meaning — return to the main buffer — instead of
// being ambiguous between "close a dialog" and "go back".
if self.ui_state.subagents.active_tab().is_some() {
    self.render_subagent_view(frame, viewport_chunk);
}
```

The comment states the intent; the code does not implement it. `render_subagent_view` (`render.rs:1502-1618`) renders a ~13-line paragraph into `Rect { width: area.width - 1, ..area }` (`render.rs:1613-1617`) — a foreground-only paragraph, one column short of the rect's width. The transcript painted by `render_viewport` remains visible **beneath** the paragraph and in the **final column**. That is the reported "ghosting".

The same overlay defect has a second instance: `render_viewport` (`render.rs:896-915`) hands the viewport to the **change review** when one is open (`render.rs:901-908`) — a modal decision surface — and the subagent view is then drawn over *that*. Two surfaces that each believe they own the viewport is the bug class, not two unrelated bugs.

### 4.2 There is no transcript to show

The tab cannot become a transcript, because nothing carries one:

- `AgentRunSummary` (`crates/codegg-protocol/src/projection/dto.rs:544-550`) documents that it "contains no prompt, transcript, mailbox body, hidden reasoning, credentials, or raw logs".
- `RunDetailView` (`crates/codegg-core/src/run_store.rs:967-977`) is a **manifest** — cell, invocation, permissions, and the canonical ids needed to reach durable run detail — not a conversation.

The tab is therefore, and can only be, a **run inspector**. Three documents claim otherwise and are wrong today:

| Location | Claim | Status |
|---|---|---|
| `docs/tui.md:671` | "One subagent's **transcript** per tab, in the main buffer" | false |
| `docs/tui.md:739` | "A subagent tab shows that subagent's **transcript**." | false |
| `docs/tui.md:273` | "`Enter` opens a session or a subagent **transcript**" | false |
| `docs/tui.md:687` | "`Enter` opens a child **transcript** or job detail" | false |
| `docs/tui.md:785` | "a **subagent** row opens that subagent's **transcript** as a tab" | false |

`architecture/tui.md:772-776` is *correct* on the rect replacement ("Clicking a row replaces the transcript in the viewport rect") and correct that the tab is the full-fidelity surface for `Task`/`Agent` — it is the `docs/tui.md` capability claims that overreach.

### 4.3 The tab strip is the one transparent surface in the TUI

`render_subagent_tab_strip` (`render.rs:1631-1668`) paints **foreground only**. There is no `.bg(...)` anywhere in it. The row it paints is **carved out of** the viewport chunk (`render.rs:307-319`), and the transcript's opaque background block is painted only over the viewport chunk (`render.rs:1235-1236`; the home renderer at `render.rs:1189-1190` carries the same convention with the comment *"Render a background block first to ensure the viewport isn't transparent"*). The tab row is therefore the one region of the main buffer with no opaque fill — the reported "transparent line".

The project tab strip shows the intended treatment: `render_tab_strip` (`render.rs:2150`) sets `.bg(theme.selection)` / `.bg(theme.background)` on its entries.

### 4.4 The strip's content

> **Closed by P002.** The label is `Main`, the main entry wears `▣`, the active
> tab's persona is on the row in italics, and the row states
> `⇧P/⇧N switch · ⇧X close`. The section below is the **baseline record** at
> `31a4128`: its citations are pinned to that commit and are deliberately not
> refreshed.

- `MAIN_TAB_LABEL = "Chat"` (`src/tui/app/state/subagent_tabs.rs:58`), used by `bar_tabs()` (`:394-397`).
- The main entry carries `glyph: ""` (`subagent_tabs.rs:397`) while every subagent entry carries `●`/`○`/`✕` (`SubagentTabState::glyph()`, `subagent_tabs.rs:165-170`). `Main` would read as plain text, not as a tab.
- The strip row already exists: `subagent_tab_strip_height` (`src/tui/app/mod.rs:15113-15115`) reserves exactly one row, and it is reserved **only** while tabs are open.
- The strip states none of `⇧P` / `⇧N` / `⇧X`.

### 4.5 The close key exists; it is only undiscoverable

> **Closed by P002.** The strip row now states the three keys. The section
> below is the **baseline record** at `31a4128`.

`InputAction::CloseSubagentTab` is bound to `Shift+X` (`src/tui/input.rs:249-252`, mapped at `input.rs:797`), handled at `src/tui/app/mod.rs:15173-15179`, and named in the tab-capacity toast (`app/mod.rs:15141`). `Shift+W` is `OpenWorkspaceDashboard` (`input.rs:1401-1407`), with a standing warning at `input.rs:955-962` and `app/mod.rs:19381-19394`; the convergence addendum recorded the close-key request as withdrawn decision **D-C3** ("`Shift+X` already closes a subagent tab"). **The gap is discoverability, not a binding.**

The only place a hint like this may go is the strip row itself. The composer hint line **must not** be reused: `hint_rows = u16::from(self.composer_hint_text().is_some())` (`render.rs:175`) — that row is reserved only when present, so appending tab hints there would reserve a **new** row and reflow the transcript on every submit and completion.

### 4.6 The side panel's subagent row spends its column on prose

> **Closed by P002.** The row now renders `icon name (agent) status` and nothing
> else; the persona lives on the tab strip. The section below is the **baseline
> record** at `31a4128`.

`src/tui/components/sidebar.rs:1396-1427` renders `icon name (agent) status`, then appends `" — {profile}"` when the column has at least `MIN_SIDEBAR_DESCRIPTOR_CHARS` (12) columns left (`sidebar.rs:205`). The persona is 1–3 sentences (pool max 223 B, per the C005 closure record), so what survives the budget is a prose stub, and the three scannable facts are pushed left. Two tests pin the current behavior and must change with it: `a_subagent_row_shows_its_descriptor_when_the_column_has_room` (`sidebar.rs:2296`) and `a_subagent_row_drops_the_descriptor_when_the_column_is_too_narrow` (`sidebar.rs:2319`).

### 4.7 Keyboard focus lands on todo rows that have no action

> **Closed by P003.** Todo rows left the focus list, the
> `SidebarFocusTarget::Todo` variant was removed with them, and a hovered todo
> now resolves to the **Todos header** — never to a target the list cannot
> produce, and never to nothing, which would clear the focus and make the next
> navigation key jump to index 0. The section below is the **baseline record**
> at `31a4128`.

`focus_targets()` (`sidebar.rs:658-743`) extends `SidebarFocusTarget::Todo(i)` for every todo while the section is expanded (`sidebar.rs:670-675`). `activate_focused()` (`sidebar.rs:619-647`) maps `SidebarFocusTarget::Todo(_)` to `SidebarActivation::None` (`sidebar.rs:640-644`): Enter on a todo row does nothing at all. `with_row_style` (`sidebar.rs:2015-2021`) paints the selection highlight on the focused row, so the highlight lands on a row with no action behind it. `panels/session.rs:66-67` maps `NavigateUp`/`NavigateDown` to `focus_prev`/`focus_next`, so `j`/`k`/arrows walk straight through them.

Two traps follow from the fix, not from the current code:

- `hovered_to_focus` (`sidebar.rs:792-799`, Todo at `:795`) maps a hovered todo to `SidebarFocusTarget::Todo`. If todo rows leave the focus list but that mapping survives, `move_focus` (`sidebar.rs:745-762`) cannot find the current target, falls through to `None`, and jumps to **index 0** — the focus yanks to the top of the panel on every mouse move over a todo.
- `ensure_focused_visible`'s `SidebarFocusTarget::Todo` arm (`sidebar.rs:504`) becomes dead.

`line_targets()` (`sidebar.rs:1011+`, Todo at `:1084`) is the **mouse** hit-test and tooltip map, not the keyboard focus map, and must keep its todo entries.

**A deliberate divergence.** The `Ctrl+T` overlay's `selectable_indices` (`src/tui/app/state/tasks.rs:402-408`) excludes headers — "a label, not something to open" — but deliberately **keeps** todo rows selectable (`tasks.rs:415`), because in the overlay a todo row *is* actionable (it carries the task detail). The side panel's todo row has no action. The two surfaces therefore differ on purpose, and this must be recorded rather than treated as an inconsistency.

### 4.8 A colliding subagent takes a number while forty-five names sit free

> **Closed by P004.** The draw is two passes now: every free pool name first,
> in a random order, and only then the numeric ladder. A new spawn therefore
> receives a bare pool name whenever one is free, and a `-N` suffix means
> every pool name was held at spawn. The section below is the **baseline
> record** at `31a4128`; its citations are pinned to that commit and are
> deliberately not refreshed.

`SubagentNameRegistry::draw` (`src/agent/subagent_names/registry.rs:149-176`) walks the 46-entry pool from a random start and, for each base name, calls `first_free(base, active)` (`registry.rs:182-193`), which returns `base` if free and otherwise the first free `base-2`, `base-3`, … up to `MAX_SUFFIX = 64`.

So the **first** occupied base name consumes the numeric suffix even though the remaining ~45 names are free. The pool holds 46 entries (`assets/subagent-names/names.toml`) against a projection cap of `MAX_PROJECTION_SUBAGENTS = 16` (`crates/codegg-protocol/src/projection/limits.rs:67`), so the free-name pass essentially always succeeds.

The registry is **active-scoped**: `SubagentNameLease`'s `Drop` releases the name (`registry.rs:268-272`), so "already used by an active agent" is exactly the existing lease notion and needs no new state. `pool_entry()` (`registry.rs:279-290`) strips a numeric suffix, so `Sally-2` still resolves to the Sally entry and keeps its persona — the change cannot orphan a descriptor. The name is drawn **once at spawn** and travels inside the event (`registry.rs:9-19`; the only publisher is `src/agent/worker.rs`), so replay reproduces the recorded name and the policy change affects new spawns only.

**The policy is documented in four places**, and one of them is generated:

- `src/agent/subagent_names/registry.rs:21-27` — the module's "Collision policy" section.
- `assets/subagent-names/names.toml:10` — the TOML source's header comment.
- `scripts/generate_subagent_names.py:124-125` — the text the generator emits into `generated.rs`.
- `architecture/tui.md:752-754` — "the registry appends numeric variants (`Sally-2`)".

`src/agent/subagent_names/generated.rs` is **generated**; it must be regenerated with `python3 scripts/generate_subagent_names.py` and never hand-edited.

## 5. Target architecture

Three surfaces, three clear jobs:

| Surface | Job | Persona | Notes |
|---|---|---|---|
| `SESSIONS` side panel, subagent row | **Scan**: name, agent type, status | no | one line, no prose |
| Subagent tab strip | **Identity**: which run am I looking at, and how do I operate this | yes, for the **active** tab, italic | one row; label + persona + hints share **one** width budget |
| Subagent tab body | **Detail**: task, agent description, identity, liveness | yes | the run inspector |

And one rule for naming: **a free pool name always beats a numeric variant of a taken one.**

## 6. Dependency graph

```text
        (none — starts from a clean base)
                        |
                        v
  P001  the tab owns its pixels: opaque viewport,
        opaque strip row, no transcript ghosting,
        the "transcript" claim corrected             [invariant]
                        |
                        v
  P002  the tab bar is the identity surface:
        "Main" + glyph + persona on the active tab
        + ⇧P/⇧N/⇧X on the strip row; the side-panel
        row drops the descriptor                      [capability]
                        |
                        v
  P003  the side panel navigates actionable rows only;
        todo rows leave the keyboard focus list      [capability]

  P004  a free pool name beats a numeric variant    [capability]
        (independent of P001–P003)
```

- **P002 → P003 is hard.** P002 moves the persona onto the tab; P003 removes it from the side-panel row. Landing P003 first would leave the persona on no summary surface at all. The dependency is about *product consistency*, not code coupling — the two milestones touch different files and could not otherwise conflict.
- **P001 → P002 is soft.** Both touch `render_subagent_tab_strip`; serializing them avoids a merge conflict in one function. P002 is not blocked on P001's correctness.
- **P004 is independent** of all three.

## 7. Milestones

### P001 — The subagent tab owns its pixels

Class: **invariant** (I2, I3). Dependencies: none. Implementation plan: `plans/implementation/tui-subagent-presentation-and-name-allocation/001-tab-surface-ownership.md`.

Objective: with a subagent tab open, the viewport rect contains the subagent view and nothing else; the strip row is opaque; the tab body is described as what it is.

Exit conditions: no transcript byte is visible in the viewport; the strip row's cells carry the theme background; the tab body fills the rect's width; the change review still takes precedence over the tab; `docs/tui.md` no longer claims a transcript.

### P002 — The tab bar is the identity surface

Class: **capability** (K4, K5) + **polish** (P1). Dependencies: P001 (soft). Implementation plan: `plans/implementation/tui-subagent-presentation-and-name-allocation/002-tab-bar-identity-persona-and-hints.md`.

Objective: the strip names the main buffer `Main`, gives it a glyph, carries the active tab's persona in italics, and states `⇧P`/`⇧N`/`⇧X` — all on the row it already owns, with one shared width budget. The side-panel subagent row drops the descriptor.

Exit conditions: `Main` with a distinct glyph; the persona is visible, italic, truncated rather than overflowing; the three key hints are present exactly while tabs are open; the side-panel row reads name, agent type, status.

### P003 — The side panel navigates actionable rows only

Class: **capability** (K2) delivering invariant I5. Dependencies: **P002 (hard)**. Implementation plan: `plans/implementation/tui-subagent-presentation-and-name-allocation/003-sessions-panel-navigation-skips-todo-rows.md`.

Objective: `j`/`k`/arrows in the `SESSIONS` side panel move between rows that have an action; the "Todos" header stays selectable so the section still folds.

Exit conditions: the focus highlight never rests on a todo row; the Todos header remains focusable and toggles on Enter; hovering a todo does not yank the focus to index 0; mouse hover and tooltips on todo rows still work.

### P004 — A free pool name beats a numeric variant

Class: **capability** (K6). Dependencies: none. Implementation plan: `plans/implementation/tui-subagent-presentation-and-name-allocation/004-subagent-name-allocation-two-pass.md`.

Objective: a newly spawned subagent receives a **different pool name** whenever any pool name is free.

Exit conditions: with at least one free pool name, a new spawn never receives a `-N` suffix; with the pool exhausted, the suffix walk still applies; the empty-pool and all-suffixes-exhausted fallbacks still hold; the documented policy is updated in all four places, with the generated file regenerated by its generator.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema change, no `STORAGE_LAYOUT_VERSION` churn, no durable name record — names remain active-scoped and ephemeral by design.

### Protocol and compatibility

None. The persona is carried on the projection already (`SubagentRow::profile`, from `AgentRunSummary`'s descriptor fields). No DTO changes. Old snapshots without descriptors keep falling back, exactly as they do today.

### Security and authorization

None. Every surface continues to render only what the projection vouches for. No secret, credential, prompt, or raw log is introduced into any surface — a persona is a curated pool string, bounded at 223 B.

### Concurrency, cancellation, and recovery

The name registry's mutex is unchanged; the new pass holds it no longer than the current one does. The active set is still the lease set, and release-on-`Drop` still bounds it. Detached tabs still outlive their subject.

### Observability and audit

None required. The tab body already states when a run has left the projection.

### Documentation and static guards

- `docs/tui.md` — P001 corrects the transcript claims; P002 corrects the `Chat` wording and describes the strip hints and the persona.
- `architecture/tui.md` — P002 corrects the name-policy sentence; P004 corrects it too. §5's "one row, five surfaces" paragraph must be reconciled with the new placement rule.
- `src/tui/input.rs:242` — P002 corrects the "Chat → subagent tabs" doc comment.
- `src/agent/subagent_names/registry.rs:21-27`, `assets/subagent-names/names.toml:10`, `scripts/generate_subagent_names.py:124-125` — P004.
- `scripts/check_tui_subagent_name_pool_isolation.py` must stay green: the persona on the tab comes from the **projection**, never from a frontend pool lookup.

## 9. Verification strategy

- **Focused unit tests** for each milestone's changed module, named in its plan.
- **Render tests** for the tab surfaces (P001, P002) — the ghosting defect is only observable in a rendered buffer, so a unit test on a helper is not sufficient evidence.
- **Guard runs**: `scripts/verify.sh quick`, which includes `check_tui_subagent_name_pool_isolation.py`, `check_tui_transport_authority.py`, `check_execution_ownership.py`, and `check_scheduler_bypass.py`.
- **`scripts/verify.sh full`** at closure for the workspace suite, Clippy, and formatting.
- **No claim of a runtime observation** unless the TUI was actually launched. The parent workstream's closures are careful about this ("no CI receipt and no runtime observation — the TUI was never launched"), and that honesty is the standard here.

## 10. Risks and decision points

- **The persona on a one-row strip is a width-arithmetic hazard.** A persona is up to 223 B; a strip is 80–200 columns wide and must also carry the labels and the hints. The budget must be computed **once**, and the persona truncated against what remains. If the persona cannot be shown at a usable width, it is dropped rather than allowed to push the labels off the row. P002 §5 states the precedence order.
- **Removing todo rows from the focus list changes an existing, pinned test.** `focus_walks_todo_then_subagent_then_job_rows_without_collapsing` (`sidebar.rs:2343-2372`) asserts the *old* order. It must be **updated to the new order**, not weakened or deleted — the walk it protects (subagent → job, without collapsing a section) is still real.
- **The `Ctrl+T` overlay's opposite rule must not be "fixed" by accident.** The overlay keeps todos selectable on purpose. P003 records the divergence.
- **Two surfaces claiming the viewport.** P001 must establish the precedence between the subagent view and the change review, or the ghosting defect reappears in a new place.
- **P004's change is visible in recorded runs.** Sessions recorded before the change keep their `Sally-2` names; that is correct — the name is drawn once and travels in the event.
- **The generated file.** Editing `src/agent/subagent_names/generated.rs` by hand is forbidden; the generator owns it, and `scripts/generate_subagent_names.py --check` must pass.

## 11. Completion definition

This addendum closes when P001–P004 each have a closure record under `plans/closure/tui-subagent-presentation-and-name-allocation/` and each closure maps its acceptance criteria to exact evidence — tests run, guards run, the implementation SHA, and an honest statement of what was **not** run. A milestone whose only evidence is "it compiles" is not closed.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blocker |
|---|---|---|---|---|
| P001 | closed | `plans/implementation/tui-subagent-presentation-and-name-allocation/001-tab-surface-ownership.md` | `plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md` | none |
| P002 | closed | `plans/implementation/tui-subagent-presentation-and-name-allocation/002-tab-bar-identity-persona-and-hints.md` | `plans/closure/tui-subagent-presentation-and-name-allocation/002-status.md` | none (P001 is a soft dependency, and P001 is now closed, so the two no longer collide) |
| P003 | closed | `plans/implementation/tui-subagent-presentation-and-name-allocation/003-sessions-panel-navigation-skips-todo-rows.md` | `plans/closure/tui-subagent-presentation-and-name-allocation/003-status.md` | none — P002 closed first, so the hard dependency was satisfied: the persona existed on the tab before the side-panel row dropped it |
| P004 | closed | `plans/implementation/tui-subagent-presentation-and-name-allocation/004-subagent-name-allocation-two-pass.md` | `plans/closure/tui-subagent-presentation-and-name-allocation/004-status.md` | none |

P001 closed at baseline `31a4128`: the viewport rect now has exactly one
owner, the subagent view fills the rect it owns, the tab strip's row is
opaque, and the `docs/tui.md` transcript claims are corrected. **Two of the
plan's five required tests were vacuous as written** and were caught by
running them against the unfixed code; both were rewritten and now fail
against the baseline. The closure record's §4.2 holds the evidence. The
column is `—` for the milestones that have not closed, rather than a path to
a file that has not been written. Each closure record is written at the
milestone's own closure, under
`plans/closure/tui-subagent-presentation-and-name-allocation/`, named with the
milestone number per `plans/closure/README.md` — `001-status.md` for P001, and
so on.

P002 closed at the same baseline: the strip's main entry reads `Main` and
carries a marker of its own, the **active** tab's persona is on the strip in
italics, the row states `⇧P/⇧N switch · ⇧X close`, and the side panel's
subagent row carries name, agent type, and status with no prose. All four
share one width budget computed once in `plan_strip`, and the strip's
arithmetic now counts the status glyphs it paints — it did not before, which
is how a marker would have ended up under the hints. **The plan's §5
precedence list and its §8 degradation order contradicted each other**; §10's
required test ("labels are preferred over the persona") was followed, and the
consequence is recorded in the closure record's §3.1 rather than absorbed
silently. Five render tests and two sidebar tests fail against the pre-P002
code, and three negative controls pass on both sides; the evidence is in the
closure record's §4.2.

P003 closed at the same baseline: the side panel's keyboard walk stops only on
rows that **do something** — todo rows left the focus list, and the
`SidebarFocusTarget::Todo` variant was removed with them, because a variant
the list cannot produce is a variant that lies (invariant I5). The `Todos`
**header** stays focusable and still folds the section. Both traps the plan
named were closed as traps: a hovered todo resolves to the **header** (never
to nothing, which would clear the focus and make the next key jump to index
0), and the now-dead `ensure_focused_visible` arm went with the variant. The
mouse map keeps its todo entries, and the `Ctrl+T` overlay's selection model
was **not** touched — there a todo row *is* actionable, so it stays selectable
there. The proof includes a **partial fix** — the todo rows out of the list
with the old hover mapping left in place — against which the new hover test
**fails**, which is what shows the trap is real rather than imagined; the
evidence is in the closure record's §4.2.

P004 closed at the same baseline: the name registry now draws in **two passes**
over the same pool in the same random order — every **free bare** name first,
and only when every base name is held the `-N` suffix ladder. A new spawn
therefore receives a pool name rather than a numeric variant of a taken one,
which is what the capability K6 asked for; the suffix is now the fallback it
was always meant to be, and the pool's 46 names against a projection cap of 16
live subagents means the free-name pass essentially always succeeds. The plan's
own §10.3 test could only be written **probabilistically** ("this cannot be
pinned without seeding the RNG"), so the draw was split into `draw` and
`draw_from(active, start)` — a behaviour-neutral parameterisation — and the
rule is now pinned **deterministically** at every start index. Three of the
four new tests **fail** against the pre-change rule and the fourth — the one
asserting the suffix ladder still applies once the pool is exhausted — passes
on both sides, which is the shape a negative control must have. The lease, the
release, the event, the draw site, and `pool_entry`'s suffix stripping are all
untouched, so replay determinism and attribution are unchanged and a recorded
`Sally-2` stays `Sally-2`; the policy is stated identically in four places
and the generated file's diff is a doc comment and nothing else. The evidence
is in the closure record's §4.2.

## 13. Handoff order

**All four milestones in this addendum are closed.** P001–P004 each have a
closure record under `plans/closure/tui-subagent-presentation-and-name-allocation/`,
and the addendum's §11 completion definition is met: every milestone maps its
acceptance criteria to exact evidence, including an honest statement of what
was **not** run.

Nothing here reopens either parent workstream. The convergence addendum's
C001–C007 stay closed, and the parent roadmap's M001–M005 remain landed-but-
unclosed; neither is reopened by this addendum, and no milestone in it
required a change to an identity, ownership boundary, protocol, storage layout,
or authorization semantic.

The one thing that changed outside `src/tui/` is the **name allocation
policy** in `src/agent/subagent_names/registry.rs` — a pure selection rule
over an in-memory active-lease set, with no persistence, no protocol field,
and no new identity. `scripts/check_execution_ownership.py` and
`scripts/check_scheduler_bypass.py` stayed green throughout, and
`docs/execution-ownership.toml` did not change.
