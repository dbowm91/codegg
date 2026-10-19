# ADR-0042: Composer Normal-mode key ownership contract

Status: accepted

Date: 2026-10-19

Amended: 2026-10-19 — accepted with one significant change. See
[§ Amendment: the `space` chord and the copy verb](#amendment-the-space-chord-and-the-copy-verb).

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Affected subsystem roadmaps:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md`
- `plans/subsystems/tui-editor-helix-parity-roadmap.md` (interface dependency only — the editor owns a
  different buffer and a different key authority boundary)

## Context

The main composer — the "LLM buffer" the user types into — currently has no selection, no undo, and
no Helix-style navigation. It is a plain `String` plus a byte cursor held by
`PromptWidget` (`src/tui/components/prompt.rs`). The user has asked for Helix keybindings in that
field so that text manipulation inside it behaves like the editor
(`src/tui/editor.rs`) and the interactive terminal panel
(`src/tui/commands/interactive_terminal.rs`).

Two established surfaces already implement the requested verb set. The editor implements it over a
daemon-authorized document under `Route::Editor`, with its own dispatch authority
(`src/tui/commands/editor.rs`). The terminal panel implements it over a scrollback buffer, with a
narrower and simpler table (`src/tui/commands/interactive_terminal.rs`). Neither is a model for the
composer, because the composer's text is a local frontend string, not a document, and it is
delivered to the daemon only on submit.

Implementing Helix navigation in the composer forces a question the other two surfaces never had to
answer: **which surface owns a key in the composer's Normal mode?** Today the composer has no
Normal-mode verbs of its own, so every bare letter in the main buffer resolves to an app-level
action through the global binding maps in `src/tui/input.rs`. Introducing composer verbs means
introducing a second claimant for the same letters, and the codebase already contains a recorded
instance of that collision going wrong.

The terminal panel's dispatch records the failure mode precisely: *"a focused panel is consulted
before Normal-mode actions, so an unclaimed `d` would silently become `OpenDiff` and never delete
anything."* The panel solved it by claiming `d` unconditionally — even with an empty selection —
so the key can never leak to the app-level verb. The composer needs the same rule, and it needs it
written down before the verb table grows, because the same rule applied to the editor is what the
editor's key-authority guard now enforces statically.

There is a second, prior question: **what does "Normal mode" mean for the composer?** The app has
exactly one mode axis, `InputMode` (`src/tui/input.rs`), held as a single global field on
`UiState` (`src/tui/app/state/ui.rs`). It is documented as a keymap gate: in `Insert`, bare
characters are text; in `Normal`, bare characters are claimable app actions. The composer badge
already renders `[INS]` / `[NOR]` from the resolved mode, and `App::composer_input_mode()`
(`src/tui/app/mod.rs`) already resolves *which surface actually holds the keyboard* — including
the terminal box and a focused side panel that claims insert keys. That resolution exists because
the badge was wrong: a focused terminal panel was reporting the composer's mode.

So the composer already has a well-defined "who holds the keyboard" derivation, and a mode badge
that reports it. What it lacks is a verb set for Normal mode and a rule for who owns a letter when
the composer and the app both want it.

This decision is required now because the workstream
(`plans/subsystems/tui-composer-helix-parity-roadmap.md`) has a first milestone that must ship
before any Helix verb exists, and the second milestone must not create a second keymap without a
recorded ownership rule.

## Decision drivers

- The composer is the app's primary text-entry surface. Making it navigable is the user-visible
  goal; the ownership rule is the means.
- Two existing surfaces already implement Helix navigation. A third implementation that invents a
  different ownership model is how keymaps drift apart. The shared selection state machine
  (`src/tui/helix_selection.rs`) already exists precisely to stop that drift.
- A key must have exactly one owner in a given surface and mode. Two owners means the behaviour of
  a key depends on unstated context, which is the class of defect the editor's key-authority guard
  was written to prevent.
- A claimed key must not silently fall through to an unrelated app verb when the composer's own
  verb is a no-op. "I pressed `d` and a diff opened" is worse than "I pressed `d` and nothing
  happened".
- The app's mode badge and the cursor must agree. A cursor that disagrees with the badge teaches
  the user a keymap that is not the one in force.
- The app has one mode axis and one documented cadence. A second mode axis or a second redraw
  cadence is a structural cost paid for a convenience feature.
- Existing app-level verbs in the main buffer are user-visible behaviour. Any that change meaning
  must be an explicit, recorded decision, not a side effect of adding a table.

## Considered options

### Option A — The composer claims letters in the app's existing Normal mode

The app's single `InputMode` axis is retained. When the composer holds the keyboard and the mode
resolves to `Normal`, the composer claims a defined set of letters; the app's global binding map
is consulted only for letters the composer does not claim.

*Benefits.*

- The existing `[NOR]` / `[INS]` badge becomes literally true: Normal mode *is* a navigation mode
  over the composer buffer, so the badge, the cursor, and the key behaviour are one story.
- No second mode axis. The editor already had to argue itself out of a two-axis model
  (`src/tui/editor.rs`); adding a third axis in the composer repeats that cost.
- The composer's derived "holds the keyboard" helper (`App::composer_input_mode()`) already
  exists, so the surface-selection rule is inherited rather than invented.
- Mirrors the terminal panel, which is the closest existing precedent: a non-document buffer with
  a selection, claiming the same verbs.

*Costs.*

- Letters that already mean something in the main buffer change meaning. The list is small and
  enumerable (see the conflict table in the Decision section), but it is a user-visible change.
- `InputMode::Normal` stops meaning only "the keymap gate" and starts meaning "the composer is in
  navigation mode". Other surfaces that consult the mode must be checked so they do not
  accidentally inherit composer semantics.

*Failure modes.*

- If the composer's claim set is not exhaustive, a claimed-but-unimplemented letter leaks to the
  app verb. The static guard must therefore police the *whole* claim set, not just the verbs that
  are implemented.
- If a surface holds the keyboard but the mode field still says `Insert` (or vice versa), the
  composer claims letters while the user believes they are typing. This is why the resolved mode
  — not the raw field — must be the gate.

### Option B — A separate composer mode axis

The app's `InputMode` keeps its current meaning, and the composer gains its own
`ComposerMode::{Typing, HelixNormal}`, entered by a dedicated key.

*Benefits.*

- The app's Normal mode keeps exactly its current meaning, so no existing app-level verb changes
  behaviour.
- The composer can be in navigation mode while the app is in Insert mode, which is a strictly
  larger expressible set.

*Costs.*

- Two mode axes for one keyboard is exactly the model the editor's documentation argues against.
- The badge can only show one of them, so the composer would need a second indicator, or the badge
  would have to change meaning. Either way the user is asked to track two mode concepts.
- The user-facing request is explicitly "in NOR mode". Option B answers a different question.
- Every consumer of `InputMode` has to be re-examined to decide which axis it meant.

*Failure modes.*

- The two axes can disagree, and the disagreement is invisible unless both are surfaced.
- Entering and leaving Helix normal mode becomes a per-surface affair, so the same key can mean
  different things depending on which axis happens to be set.

### Option C — Prefix-gated Helix only

Helix verbs are reachable only inside a composer-entered modal line (a `:`-style command line),
never in the main buffer at large.

*Benefits.*

- Zero conflict with existing app-level verbs outside the modal line.
- Smallest possible change.

*Costs.*

- Does not deliver the requested capability. "Basic helix navigability" in the main buffer is the
  whole point; a modal line is a different feature.
- The editor already has a prefix-gated modal line, and it does not satisfy the composer request.

*Failure modes.*

- Users would reasonably read a `:` line as a command palette, not a text-editing mode.

### Option D — Helix only in a dedicated composer sub-mode, entered from the app's Normal mode

The app's Normal mode is retained, and the composer enters Helix navigation only when a second,
explicit gesture is performed (for example, a dedicated chord), leaving a second mode again.

*Benefits.*

- Preserves all existing app-level verbs by default.
- Still delivers Helix navigation, just behind a gesture.

*Costs.*

- Same two-axis objection as Option B, with an extra gesture.
- The badge question returns, and the answer is worse: the composer would be in Normal mode, not
  navigating, for most of the time the user is in the composer.

*Failure modes.*

- Users discover the gesture slowly or not at all, so the capability is under-used.

## Decision

**Option A is selected**, and this record is **accepted** by the maintainers, with the
amendment recorded in [§ Amendment](#amendment-the-space-chord-and-the-copy-verb).

The decision has seven parts.

1. **The composer reuses the app's single mode axis.** `InputMode::Normal` in the main-buffer
   surface means *Helix navigation over the composer buffer*. The resolved mode — the one
   `App::composer_input_mode()` returns — is the gate. The raw `UiState::input_mode` field is
   never used as the gate, because it is forced to `Normal` whenever a side panel takes focus.

2. **The composer claims a closed, enumerated set of letters, and claims them
   unconditionally.** The claim set is exhaustive: every letter in it has exactly one owner in the
   composer. A claimed letter whose verb is currently a no-op — `d` with no selection, `x` with
   no selection — is still consumed by the composer and does **not** fall through to the app-level
   verb. This is the rule the terminal panel already follows, and the rule the editor's key
   authority guard now enforces.

3. **Unclaimed letters fall through to the app's existing binding map unchanged.** A letter the
   composer does not claim behaves exactly as it does today in the main buffer. This preserves
   the app's Normal-mode verbs rather than reinterpreting them.

4. **The conflict table is explicit and exhaustive.** At the baseline recorded in this record, the
   following letters are already owned by app-level Normal-mode actions in the main buffer. Each
   must be resolved explicitly by the implementing milestone; none may be resolved by accident.

   | Letter | Existing owner (Normal mode) | Required resolution |
   |---|---|---|
   | `h` | `InputAction::Left` → `TuiMsg::CursorLeft` → prompt caret | **No conflict.** The existing action already moves the composer caret, which is Helix `h`. The composer keeps it. |
   | `l` | `InputAction::Right` → `TuiMsg::CursorRight` → prompt caret | **No conflict.** Same as `h`. |
   | `j` | `InputAction::NavigateDown` (transcript scroll) | The composer claims `j` **only when the composer buffer has more than one line**. On a single-line buffer the key must fall through to transcript scroll unchanged. |
   | `k` | `InputAction::NavigateUp` (transcript scroll) | Mirror of `j`. |
   | `x` | `InputAction::ToggleAgentRun` (stop/resume the turn) | The composer claims `x` as Helix line-select. **Stop/resume must be re-spelled to a non-letter chord**, and the new spelling must be added to both binding maps and to the help surface in the same milestone. The turn-stopping capability is not removed. |
   | `d` | `InputAction::OpenDiff` (vim map only) | The composer claims `d` as Helix delete-selection, unconditionally. Open-diff must be reachable by a chord that is not a bare letter, in both maps. |
   | `space` | `InputAction::Help` (both maps) | **No conflict, and no re-spelling.** `space` keeps opening the help menu, in both maps, exactly as it does today. The composer does **not** claim `space`. See part 7 and [§ Amendment](#amendment-the-space-chord-and-the-copy-verb). |
   | `g` | `InputAction::GoToTop` | Not in the composer's initial claim set. `GoToTop` keeps `g` unless the composer later claims `g` for a goto prefix, in which case `GoToTop` must be re-spelled then. |
   | `i` | `InputAction::FocusPrompt` (vim map) | The composer claims `i` as Helix insert-before-cursor. Focus-prompt must be re-spelled; the capability is not removed. |

   Letters with no existing Normal-mode owner — including `e`, `b`, `v`, `o`, `u`, `U`, `p`, `w` —
   are free for the composer to claim. `y` is not among them: part 7 gives the copy gesture to the
   help box the `space` chord opens, and one gesture has one owner.

5. **A claimed uppercase letter accepts both key encodings.** The app binds some capital letters
   under two encodings — `Char('H')` with `SHIFT`, and `Char('H')` with `NONE` — because some
   terminals report a shifted key without the `SHIFT` modifier. The composer follows the same
   convention: a claim on an uppercase letter, notably `U` for redo, must match **both** the
   shifted and the unshifted encoding, or the verb works on one terminal and not another. The
   Insert-mode text guarantee is unaffected, because a built-in binding on a text key is skipped
   in Insert mode rather than consumed.

6. **The claim set is policed by a static guard, not by review.** A guard must fail when a letter
   in the composer's claim set has no owner, when a claimed letter can fall through to an
   app-level action, or when a letter is claimed by two owners. The guard must ship with the
   milestone that establishes the claim set, before the verb milestones add to it. The guard's
   self-test must inject a leak and prove the guard rejects it.

7. **`space` stays the help chord, and the copy verb lives in the box it opens.** The composer does
   not claim `space`; `InputAction::Help` keeps it in both binding maps, and `space` keeps opening
   the help menu. The Helix yank gesture is preserved as a **two-step** gesture rather than a
   prefix: `space` opens the help menu, and **`y` in the help menu the `space` chord opened copies
   the composer's selection to the system clipboard**. The help menu opened by the `/help`
   **command** does not carry that verb — a `/help` box is opened to type a query, and `y` there
   is a filter character.

   Three consequences are load-bearing:

   - The verb is a **Normal-mode** verb of that box, like `j`/`k`/`q`. A box opened by `space` is
     in Normal, because `space` is a Normal-mode chord; a `/help` box is in Insert, because the
     user typed it. The mode is what keeps `y` a verb in one box and a character in the other.
   - The box must know **how it was opened**. `open_dialog(Dialog::Help)` cannot answer that on
     its own, so the opening route is carried into the widget. This is a caller decision, exactly
     like the starting mode: the widget never hardcodes it.
   - With no composer selection, `y` reports that there is nothing to copy and changes nothing.
     A silent no-op would teach the user that the key is dead, which is the failure the claim
     table exists to prevent.

   The composer therefore does **not** claim `y` either. One gesture, one owner: the copy verb is
   the help box's, reached through `space`, and the composer has no second spelling of it.

## Consequences

### Positive

- The `[NOR]` badge, the cursor, and the key behaviour become one consistent story.
- The composer joins the two surfaces that already share `src/tui/helix_selection.rs`, so selection
  semantics do not drift into a third implementation.
- App-level verbs that must move (stop-turn, open-diff, focus-prompt) become explicit, recorded,
  and chord-based rather than positional accidents.
- The Helix yank gesture survives without costing the app its most reachable help chord: `space`
  still opens the help menu, and the yank is the second step of the same two-key gesture Helix
  uses.
- The claim/guard model generalises: the terminal panel and the editor can be brought under the
  same guard family later without changing their behaviour.

### Negative

- Three app-level chords change: stop-turn, open-diff, and focus-prompt. Users who learned them
  will need the new spelling, and the help surface must say so. Help does **not** move — that was
  the sharpest of the four, and it is not paid.
- The composer's Normal mode is a second Helix implementation in the app, and it must track Helix
  semantics deliberately rather than accidentally.
- A static guard for the composer's claim set is new machinery that must be kept in step with the
  verb table.
- The copy verb is two keys and a menu, not one chord. A user who wants to copy has to read a
  help box on the way, which is more friction than Helix's `space y` — and it is the price of not
  spending `space` on a prefix.

### Neutral or deferred

- The editor's own key authority remains separate; the composer does not extend the editor's
  guard, it gains a sibling guard.
- Paste (`p`) has no precedent in any surface. Whether the composer introduces it alone, or the
  editor and terminal panel gain it too, is a roadmap decision and is **not** settled by this
  record.
- Whether the blink for insert mode is the terminal's native cursor or an app-driven cadence is a
  roadmap decision and is **not** settled by this record. The single-cadence rule in `AGENTS.md`
  is a constraint on that choice, not a decision itself.
- The scope of composer undo — the prompt text only, or the wider composer state including slash
  commands and staged `!cmd` input — is a roadmap decision and is **not** settled by this record.
- Multi-selection is out of scope for this record. The composer gets a single selection, matching
  the terminal panel.

## Compatibility and migration

- No storage migration. The composer's text is a frontend-local string and is not persisted by
  this workstream.
- No protocol change. No daemon message, no session state, and no wire format is affected.
- No configuration change. The new chords are not user-configurable in this milestone; making the
  composer's claim set configurable is explicitly deferred.
- **User-visible key change.** Stop-turn, open-diff, and focus-prompt move to new chords in the
  main buffer. This is a deliberate compatibility break, accepted here so that it is recorded
  rather than discovered. The help surface and `docs/tui.md` must state the new spellings in the
  same change that removes the old ones. Help itself does **not** move: `space` keeps opening it.
- The editor's and the terminal panel's existing keymaps are unchanged by this record.

## Security and reliability implications

- **No authority change.** The composer is a frontend buffer. Nothing in this workstream changes
  what the daemon accepts, how turns are admitted, or what a tool may do. The composer's
  `u`/`U` undo history is local presentation state and never reaches the daemon.
- **No new input path to the daemon.** Composer text still reaches the daemon only through the
  ordinary submit path.
- **Clipboard exposure.** The copy verb copies the selected composer text to the system clipboard.
  This is the same exposure the editor's yank already has, and it is user-initiated. It must not
  copy text the user did not select, and it must not log clipboard contents.
- **Undo must not resurrect submitted text.** The undo stack is bound to the composer's current
  buffer and must be cleared when the composer is submitted or replaced, so that `u` cannot
  re-materialize a prompt that was already sent.
- **Reliability.** The claim set is a closed table; an unclaimed letter must behave exactly as it
  does today. A partially-implemented claim set is a correctness defect, which is why the static
  guard lands with the claim set rather than after the verbs.

## Verification

- A static guard proves the composer's claim set is closed, single-owner, and non-leaking, with a
  self-test that injects a leak and proves rejection.
- Dispatch-reachability tests press every claimed letter as a real key event through the app's key
  path and assert the composer owned it. A verb that is only reachable by calling a helper
  directly does not count as reachable.
- Negative tests assert that, in the main buffer with the composer holding the keyboard: `d` does
  not open the diff, `x` does not stop the turn, and `j`/`k` still scroll the transcript when the
  composer buffer is a single line.
- Positive tests assert the re-spelled chords still reach stop-turn, open-diff, and focus-prompt.
- A help-chord test asserts that `space` still opens the help menu, that `y` in the menu the
  `space` chord opened copies the composer's selection, and that the `/help` command's box does
  **not** carry that verb.
- A cursor test asserts the composer draws a cursor in Insert and Normal modes, and draws none
  when another surface holds the keyboard.
- An encoding test asserts that a claimed uppercase letter is reachable in both the shifted and
  the unshifted encoding, because a terminal that reports a shifted key without the `SHIFT`
  modifier must still reach the verb.
- The help audit (`documented_keys_resolve_in_the_active_map` and the modifier-chord audit) must
  pass with the new claim set and the re-spelled chords.

## Amendment: the `space` chord and the copy verb

**Amended 2026-10-19, at acceptance.**

The decision as first drafted made `space` the composer's Helix prefix and re-spelled help to a
non-letter chord. The maintainers **rejected that entry** and amended the record:

- **`space` keeps its current behaviour: it opens the help menu.** Both binding maps keep
  `InputAction::Help` on `space`, and nothing about the help chord changes. It is the most
  reachable chord in the app, and spending it on a prefix was not a trade the app was willing to
  make.
- **The copy verb becomes the second step of the same gesture.** What the help menu is brought up
  with is `space`; after that gesture, `y` acts as **copy to clipboard**. The help menu opened by
  the `/help` **command** does not carry the verb.

The reasoning the amendment records: this keeps the app closer to current Helix behaviour on this
gesture than either alternative would. In Helix, `space` is a prefix and `space y` yanks; here
`space` is a chord that opens a surface, and `y` inside that surface yanks. The two-key shape of
the gesture survives — what changes is what the first key lands on, not whether the second key
follows.

What the amendment changes in this record:

| Section | Before | After |
|---|---|---|
| Decision part 4, `space` row | The composer claims `space`; help is re-spelled | `space` is not claimed; help is not re-spelled |
| Decision part 7 | *(absent)* | The copy verb is a Normal-mode verb of the `space`-opened help box |
| Consequences, Negative | Four chords move (help, stop-turn, open-diff, focus-prompt) | Three move (stop-turn, open-diff, focus-prompt) |
| Consequences, Negative | — | The copy gesture costs a menu, and that is the recorded price |
| Compatibility | Help moves | Help does not move |
| Verification | `space` does not open help | `space` opens help; `y` in that box copies; `/help`'s box does not |

What the amendment does **not** change: Option A, the single mode axis, the unconditional-claim
rule, the fall-through rule, the dual-encoding rule for uppercase letters, and the static-guard
requirement. The claim table still governs the composer's letters; the copy verb simply is not one
of them.

One consequence worth stating plainly, because it is easy to get wrong in the implementation: the
help box must know **how it was opened**. `open_dialog(Dialog::Help)` does not answer that, so the
opening route is carried into the widget by the caller — the same way the starting mode already
is. A box that inferred the route from its own mode would be right today and wrong the day a
`/help` typed from Normal mode opens the same mode, which is exactly the kind of accidental
coupling this record exists to prevent.

## Supersession

None. If this decision is superseded, `plans/subsystems/tui-composer-helix-parity-roadmap.md`
milestone M002 is re-planned against the superseding record, and the composer claim-set guard is
re-scoped to it.
