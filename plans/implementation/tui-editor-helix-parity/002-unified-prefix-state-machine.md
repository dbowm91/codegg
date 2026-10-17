# TUI Editor Helix Parity Milestone M002 — Unified prefix state machine

Status: closed — closure `plans/closure/tui-editor-helix-parity/002-status.md`

Outcome: both legacy mechanisms deleted and replaced by
`TuiDocumentPresentation::pending_prefix: EditorPrefix`. `gg` and `space y`
behave identically to the baseline and their existing tests passed
**unmodified**. `[`/`]` armed; a second key disarms and resolves normally.
One deviation from §6, recorded in the closure: the two gestures disagreed about
an unmatched second key (`gg` consumes it, `space y` retries it), so §6's
uniform "disarms and is then resolved normally" would have changed `g j`; the
policy is carried per-prefix by `EditorPrefix::swallows_unmatched` so
acceptance criterion 2 still holds.

Repository baseline: `719cfeb5d` (branch `feat/auto-20261017-80a41db7`)

Source roadmap:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M002)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` (proposed; not a gate
  for this milestone — `[`/`]` are armed here but their leaves arrive in M006)

Primary class: infrastructure

## 1. Objective

Replace the two ad-hoc prefix mechanisms in the editor route with one
pending-prefix state machine, and express the existing `gg` and `space y`
gestures through it, so that Helix's `[`/`]` and `g`-mode grammars can be added
later without each needing their own flag.

## 2. Why this milestone is ready

- Hard dependency M001 closed: the dispatch authority and its reachability guard
  must exist before the dispatch path is restructured, otherwise the
  consolidation is built on an authority model already known to lose bindings.
- ADR-0011 and ADR-0021 accepted. This milestone adds no authority, no
  protocol, and no dependency.
- Both mechanisms being replaced are local frontend presentation state with a
  single consumer each, so the blast radius is bounded.

## 3. Current implementation evidence

### Two independent mechanisms

**`pending_command`** — `src/tui/document_session.rs`, on
`TuiDocumentPresentation`:

```rust
/// ... [`MAX_EDITOR_PENDING_COMMAND`](super::editor::MAX_EDITOR_PENDING_COMMAND).
pub pending_command: String,
```

A free-form `String` bounded by
`MAX_EDITOR_PENDING_COMMAND: usize = 2` (`src/tui/editor.rs`). Written by
`push_pending` in `src/tui/commands/editor.rs`, which refuses to grow past the
bound and clears on overflow. Read by a single resolution branch in
`handle_normal_key` that handles exactly one gesture:

```rust
let pending = session.presentation().pending_command.clone();
if !pending.is_empty() {
    session.presentation_mut().pending_command.clear();
    let KeyCode::Char(ch) = key.code else { return BufferKey::Consumed; };
    if pending.as_str() == "g" && ch == 'g' { ... }
    return BufferKey::Consumed;
}
```

Two `g`-mode entries — `gg` and bare `g` — push the same pending state, and
`gg` is therefore the only multi-key prefix.

**`yank_prefix_pending`** — a `bool` on the same presentation struct, cleared in
its own reset path. Set by the `space` arm in `handle_normal_key` and consumed by
a guard at the top of the same function.

### Render plumbing that must keep working

`pending_command` is threaded from presentation state through the render options
into the editor widget, where it renders as a pending indicator
(`format!(" {}", pending_command)`) truncated to
`MAX_EDITOR_PENDING_COMMAND`. It appears in the render options struct, the
widget's borrowed field, its construction sites, and at least one render test
fixture. This plumbing is load-bearing user-visible feedback: a pending prefix
that is invisible is a prefix the user cannot predict.

The terminal panel has its **own** `prefix_pending` concept for the same `space y`
gesture. This milestone must not silently change the terminal panel; see §9.

## 4. Invariants that must not regress

- **`gg` and `space y` behave exactly as today**, including which keys disarm a
  pending prefix and what a non-matching second key does.
- **Pending state is disarmed on** pane focus change, route change, and document
  open. A stale pending prefix must never apply to a newly opened buffer — that
  is the "relative path resolved against the wrong root" class of bug in gesture
  form.
- **The pending indicator still renders** for a pending prefix, within a bounded
  width.
- Unclaimed-key fallthrough in Normal mode is unchanged.
- Insert-mode printable characters are never consumed by prefix resolution.
- No mutation outside `DocumentController::apply_local`; the buffer keeps
  reporting effects and the App layer keeps performing them.
- `:` and `/` remain outranking everything, including any prefix resolution.

## 5. Scope

### In scope

- A single typed pending-prefix type with explicit arm/disarm/resolve semantics.
- Migrating the `gg` and `space y` gestures onto it.
- Arming `[` and `]` as prefixes that consume correctly (leaves arrive in M006).
- Deleting `pending_command` and `yank_prefix_pending` — not merely bypassing
  them — and updating the render plumbing that referenced `pending_command`.
- Preserving and extending the pending indicator render.
- A documented grammar rule that later milestones add leaves to rather than new
  state.

### Explicitly out of scope

- Any unimpaired leaf binding (`]d`, `[d`, `]p`, …) — M006.
- Any `space`-mode leaf beyond `space y` — M010.
- Any goto-mode leaf beyond `gg` — M005.
- Character-finding motions (`f`/`F`/`t`/`T`, `Alt-.`) — M003. Note `f` and
  `Alt-.` will later consume a following key and therefore register against
  this machine, but are not built here.
- Any new binding for the user.

## 6. Required production changes

### Core/domain

None. Frontend presentation state only.

### Storage, protocol, runtime

None.

### Frontend surface

- Introduce one pending-prefix representation on `TuiDocumentPresentation`
  (replacing both legacy fields) that can express: idle, a pending `g`, a
  pending `space`, and a pending `[` or `]`.
- Each mode registers its leaf table against that representation rather than
  owning a flag. A non-matching key disarms and is then resolved normally,
  matching today's `space y` semantics.
- Preserve the current overflow/bound behavior; a prefix grammar that can grow
  unbounded is not an acceptable foundation, and the render indicator has a
  bounded width.
- Update the render options, the editor widget's pending field, and its
  construction sites to read the new representation.
- Keep the pending indicator behavior: a pending prefix is visible, and its
  displayed width stays bounded.

Design latitude is intentional: a typed prefix enum, a small prefix struct with
a pending `char` plus a kind, and a resolution function that owns arming are all
acceptable. What is **not** acceptable is leaving a per-feature boolean behind —
the deliverable is that adding M006's leaves requires adding a leaf, not a flag.

### Security and authorization

None. No new effect, no new authority.

### Documentation and static guards

- A doc comment on the new type stating the grammar rule: one pending prefix,
  one terminal key consumed, disarm on mismatch or route/focus/document change.
- Add or extend a guard asserting `pending_command` and `yank_prefix_pending` no
  longer exist, so the deletion does not silently regress.

## 7. Ordered work packages

### Work package A — Prefix representation

Intent: one type expressing the pending grammar with explicit semantics.

Required changes: add the type, its arm/disarm/resolve operations, and its bound.
Add the disarm call sites for pane focus change, route change, and document open.

Acceptance evidence: unit tests for arm, resolve-match, resolve-mismatch,
non-`Char` terminal key, and the bound.

### Work package B — Migrate `gg`

Intent: `gg` expressed through the new machine with identical behavior.

Required changes: replace the bespoke `pending_command` branch with prefix
resolution.

Acceptance evidence: existing `gg` behavior tests still pass unchanged. If any
existing test must be rewritten rather than still passing, that is a signal the
behavior changed — stop and report.

### Work package C — Migrate `space y`

Intent: `space y` expressed through the new machine with identical behavior.

Required changes: remove the `yank_prefix_pending` guard and arm the prefix
instead.

Acceptance evidence: existing yank tests pass unchanged.

### Work package D — Render plumbing migration

Intent: the pending indicator keeps working for every pending state.

Required changes: update render options, the widget field, its construction
sites, and render fixtures.

Acceptance evidence: a render test asserting a pending prefix is displayed and a
different pending prefix displays differently. Must be non-vacuous.

### Work package E — Delete the legacy mechanisms

Intent: the two legacy fields are gone, not bypassed.

Required changes: remove `pending_command` and `yank_prefix_pending` and every
reference, including the reset path.

Acceptance evidence: a guard (compile-level plus a grep-style check consistent
with the repo's existing guard scripts) fails if either field is reintroduced.

### Work package F — Arm `[` and `]`

Intent: the unimpaired prefixes are part of the grammar now.

Required changes: arm `[` and `]`; a second key currently disarms and resolves
normally (its leaves arrive in M006).

Acceptance evidence: pressing `[` arms the indicator; a second key with no leaf
disarms without swallowing the key — verify the fallthrough explicitly, because
swallowing here would break typing `[` followed by a character into the
composer.

## 8. Failure, cancellation, restart, and contention semantics

- **Stale prefix across navigation** is the principal hazard. Disarm on document
  open, route change, and pane focus change, and prove each with a test.
- **Overflow** must disarm rather than wedge the state machine; a user must
  always be able to recover by pressing a key.
- **No async, cancellation, or restart surface** is introduced. Restart
  semantics are unchanged: prefix state is not durable.
- **Terminal panel** keeps its own prefix handling in this milestone (§9).

## 9. Compatibility and migration

No storage, protocol, or configuration migration. `TuiDocumentPresentation` is
frontend state, not durable.

**Terminal panel:** `src/tui/commands/interactive_terminal.rs` maintains its own
`prefix_pending`/`YankPrefix` for the same gesture and must keep working
unchanged. Converging the two onto one shared state machine is desirable but is
**not** this milestone; if it appears trivially in scope, note it in the closure
as deliberately deferred rather than silently expanding scope. What must be
proved here is that the terminal panel's behavior is unaffected.

## 10. Required tests

### Focused unit tests

- Prefix arm/resolve/disarm/overflow unit tests (Work package A).
- `gg` and `space y` behavior preserved (B, C) — existing tests must still pass.
- Disarm on document open, route change, and pane focus change.
- `[` then a non-leaf key does not swallow that key.
- Prefix resolution never runs while `search_active` or `command_active`.
- Prefix resolution never runs in Insert mode.

### Integration tests

- `cargo test --test tui_main_buffer` — the editor route regression surface.

### Render tests

- Pending indicator renders for each pending state, non-vacuously (Work package D).

### Restart, contention, migration

Restart and contention are not applicable. Migration is not applicable.
State that explicitly in the closure rather than omitting it.

### Security and negative tests

- A read-only buffer's prefix handling is unchanged.
- No prefix path reaches `DocumentController` or the filesystem directly.

### Non-vacuity proof

The `[`/`]` fallthrough test and the render test must each be shown to fail
against an injected defect before being cited.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg --lib tui::commands::editor
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::components::editor
cargo test -p codegg --test tui_main_buffer
cargo test -p codegg --test tui_render

# terminal panel must be unaffected
cargo test -p codegg --test interactive_terminal_tui

# compile + guards
cargo ck
scripts/verify.sh quick

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Documentation updates

- `architecture/tui.md`: the prefix grammar rule and the disarm points.
- `docs/tui.md` "Editor language features": only if the pending indicator's
  visible behavior changes; it must not change visibly for `gg` and `space y`.
- `CHANGELOG.md` if warranted.

## 13. Acceptance criteria

1. `pending_command` and `yank_prefix_pending` are deleted, with a guard against
   reintroduction.
2. `gg` and `space y` behave identically to the baseline; their existing tests
   pass unmodified.
3. The pending indicator renders correctly for every pending state, and is
   bounded.
4. A pending prefix is disarmed on document open, route change, and pane focus
   change, each proven by a test.
5. `[` and `]` arm and disarm without swallowing the following key.
6. Prefix resolution never intercepts `:`/`/` lines or Insert-mode input.
7. Terminal-panel behavior is unchanged and its tests pass.
8. `scripts/verify.sh quick`, `cargo fmt --check`, and clippy `-D warnings` pass.

## 14. Stop conditions

Stop and report rather than improvise when:

- Preserving `gg` or `space y` behavior exactly appears impossible with the new
  representation. A behavior change in either gesture is a user-visible decision,
  not an implementation detail.
- Converging the terminal panel proves to be a prerequisite rather than a
  follow-up.
- The change appears to require durable state — prefix state must remain
  frontend presentation only.
- A guard cannot be made non-vacuous.
- Repository evidence contradicts the existing terminal-panel dialect contract.

## 15. Closure evidence required

- `plans/closure/tui-editor-helix-parity/002-status.md` containing: the chosen
  representation and why; confirmation that both legacy fields are deleted;
  evidence `gg` and `space y` tests passed **unmodified**; the disarm test
  results; non-vacuity injection results; terminal-panel non-regression evidence;
  exact verification commands and results with local versus hosted labels; the
  explicit not-applicable statements; and the registry update with the unblock
  audit recording that M003–M006 and M010–M012 became `ready`.

## 16. Handoff notes

- **Hazard:** the deliverable is *deletion*. Leaving a legacy field bypassed
  rather than removed satisfies nothing and will be read as incomplete.
- **Hazard:** "existing tests still pass unmodified" is the strongest available
  evidence that `gg` and `space y` did not change. If a test needed rewriting,
  treat that as a behavior change and stop.
- **Hazard:** the `[`-then-key fallthrough is easy to get wrong in the direction
  that swallows a character and breaks composer typing.
- **Hazard:** the render plumbing is threaded through several layers and has
  multiple construction sites and a fixture; missing one shows up as a compile
  error or a blank indicator, and the indicator is user-visible.
- Build jobs are capped (`CARGO_BUILD_JOBS=2`); broad sweeps run under the
  nextest `ci` profile.
- This milestone's closure is the unblock point for most of the roadmap — the
  registry unblock audit in §15 is required, not optional.
