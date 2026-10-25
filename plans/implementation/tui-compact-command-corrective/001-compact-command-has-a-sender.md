# TUI Compact Command Corrective — Milestone C001 — `/compact` Must Have a Sender

Status: ready for handoff

Repository baseline: `6e8a6bb60` — `origin/main` with M007-A, M007-B and M007-C
landed.

Source roadmap:

- `plans/subsystems/tui-compact-command-corrective-roadmap.md`

Planning basis:

- The owner's direction of 2026-10-25: register a corrective milestone for the
  `/compact` missing-sender defect, after the M007 desktop GUI parity line
  closes. The defect is **pre-existing** and is untouched by that line.
- **The defect was already recorded**, before this line existed, by M007-D's
  own plan: `plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md`
  §14 — "A command that did *not* work before — `/compact` is the confirmed
  case — is a **recorded discrepancy requiring an owner-visible decision**, not
  a licence to change behaviour silently." The same plan classifies `/compact`
  as a **daemon effect** (its WP-A table, row 12: "compaction is
  daemon-owned"). This line is the disposition of that recorded discrepancy:
  the owner-visible decision has been made, and it is that the command must
  stop lying before anything may change it.
- **M007-D must not close the discrepancy by accident.** While M007-D is in
  flight, its WP-F may route `/compact` — and plan 014 §14 forbids changing
  the command's behaviour silently. The owner-visible decision is *this*
  milestone's, so until it lands, `/compact`'s behaviour stays exactly as it
  is.
- `plans/002-long-term-roadmap.md:899` — "Avoid accumulating incompatible local
  meanings in phase plans." A command that reports a local effect it cannot
  produce is that failure in the command surface.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.4 — frontends render
  frontend-neutral snapshots and events. A frontend's own toast is
  presentation; it must not narrate an effect the daemon did not perform.
- The daemon's authority over **when compaction fires** is unchanged by this
  milestone.

Applicable architecture:

- `architecture/tui.md` — the TUI's command dispatch and its rendering of
  daemon events.

Predecessor closure:

- None. The defect is self-contained.

Primary class: capability

---

## 1. Objective

Make `/compact` **truthful**, and give the command a sender.

### The defect, exactly

`src/tui/app/mod.rs:5958`:

```rust
B::Compact => {
    self.messages_state
        .toasts
        .info("Compaction triggered - reducing context");
}
```

The arm shows a toast and returns. **No request leaves the TUI.** The command
is a message with no sender: it narrates an effect that never happened.

### What is already true, and must not be broken

- Compaction is **automatic and policy-driven**: `src/agent/policy.rs:16`
  carries `compaction_threshold`; `src/context/compaction.rs:23` is the
  `ContextTracker`; `src/agent/loop.rs:913` sets the tracker's threshold, and
  the daemon applies the policy in `src/agent/context_runtime.rs:827`
  (`compact_if_needed`) — the threshold is read at `:836` and the compaction
  decision is made at `:858`.
- The daemon reports each compaction back as `AppEvent::CompactionTriggered`,
  and the TUI already renders and counts it
  (`src/tui/runtime/app_events.rs:531`).

So the daemon's own report is already truthful. Only the **command** lies.

## 2. What the corrective must do

### 2.1 The floor — no false report

After this milestone, `/compact` MUST NOT report an effect that did not
happen. Every path the command can take ends in a message that is true of
what the product actually did.

### 2.2 The two acceptable shapes

The implementer takes **one** of these and records which, and why:

**Shape A — the command asks, and the daemon decides.**
The TUI sends a real request; the daemon performs a compaction under its own
authority and reports it back the way every other compaction is reported —
`AppEvent::CompactionTriggered`. The toast the user sees is derived from the
daemon's report, not from the command's own optimism.

This shape requires a **new protocol variant** and therefore a decision the
implementer does not make: **§4's stop condition 1**. If the owner has not
authorized the variant, Shape A is off the table and Shape B applies.

**Shape B — the command reports the truth about what compaction is.**
`/compact` stops claiming to have triggered anything and instead reports what
is actually true: compaction is automatic, it fires when the daemon's policy
threshold is crossed, and the session has compacted **n** times so far (the
count the TUI already keeps). If the session has no compaction capability at
all, the command says so.

Shape B is the **floor**, and it is always available. It is not a
workaround: a command that reports the product's actual behaviour truthfully
is a correct command.

### 2.3 What must not happen

- The toast must not be fired **before** the outcome is known. A command that
  announces its own success has the same defect in a different shape.
- The command must not reach into the daemon's policy. The threshold is the
  daemon's; a frontend does not get to lower it.
- The automatic path must not change. `AppEvent::CompactionTriggered` handling
  is already correct and is not touched.

## 3. Out of scope — explicit non-goals

- **No change to the daemon's compaction policy, threshold or tracker.**
- **No change to the TUI's rendering of `AppEvent::CompactionTriggered`.**
- **No change to the `Command` catalog's shape.** `/compact` is a TUI-local
  builtin today; whether it becomes a catalog entry is ADR-0052's business
  and belongs to M007-D, not here.
- **No broad sweep.** The change touches the TUI's command dispatch arm and
  whatever the chosen shape needs.

## 4. Stop conditions

The implementer **stops and reports** rather than deciding any of these:

1. **Shape A needs a new `CoreRequest` variant.** That is a new capability on
   the daemon's surface, and the authority question — *may a frontend ask the
   daemon to compact now, and does the daemon retain the right to refuse?* —
   is the owner's. Report the proposed variant, its shape, and what the daemon
   would do with it; **do not** add it under this milestone.
2. **The daemon already has a way to be asked to compact that this milestone
   has not found.** Report where it is. If it exists, Shape A needs no new
   variant and the question is only whether using it is in scope.
3. **Making the command honest would change what the TUI's tests assert.**
   Report the affected assertions. A test that asserts the *false* toast is
   part of the defect, not a contract to preserve.
4. **The command is reachable from more than one surface** — the desktop
   palette, a keymap chord, or an ACP client. Report every surface that
   reaches `B::Compact`, because each one inherits the same false report.

## 5. Required production changes

| File | Change |
|---|---|
| `src/tui/app/mod.rs` | the `B::Compact` arm: a truthful outcome, per §2 |
| the chosen shape's supporting files | whatever Shape A or Shape B requires, and no more |

## 6. Required tests

1. A test that `/compact` produces a message that is **true** of what
   happened — never the false "Compaction triggered" toast on a path where
   nothing was triggered.
2. If Shape A is taken: a test that the request reaches the daemon, and that
   the toast is derived from the daemon's `CompactionTriggered` report.
3. If Shape B is taken: a test that the reported compaction count matches the
   count the TUI already keeps.
4. A test that the command does **not** reach the daemon's policy — the
   threshold is the daemon's.

## 7. Required verification commands

```bash
cargo test --locked -- compact
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

## 8. Acceptance criteria

1. `/compact` never reports an effect that did not happen, on any path.
2. The command's outcome is derived from what the product actually did.
3. The daemon's automatic compaction path, its threshold and its tracker are
   unchanged.
4. The TUI's handling of `AppEvent::CompactionTriggered` is unchanged.
5. No new protocol variant lands without the owner's decision.
6. `scripts/verify.sh quick` is green, and fmt and workspace clippy with
   `-D warnings` are clean.
7. Every surface that reaches the command is identified and corrected.

## 9. Closure evidence required

1. The command's behaviour before and after, quoted.
2. The chosen shape, and why the other was not taken.
3. If Shape B: the owner's decision record for the manual-compaction
   question, or the evidence that it is still open.
4. If a stop condition was hit: the report that was made, and the owner's
   answer.
5. The commands in §7 with their real outcomes.
6. Evidence that the daemon's automatic compaction still fires — the
   `AppEvent::CompactionTriggered` path's own tests, named.
