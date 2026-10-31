# TUI Compact Command Corrective — Subsystem Roadmap

Status: closed (opened 2026-10-25 by the owner's direction; C001 closed 2026-10-31)

## 1. What this line is

A **corrective** line for one defect: the TUI's `/compact` command **claims
work it never did**.

`src/tui/app/mod.rs:5958` handles `B::Compact` by showing one toast —
"Compaction triggered - reducing context" — and sending **nothing**. No
request leaves the TUI, the daemon is not asked, and no compaction happens.
The message is false on every invocation.

Compaction itself is real and works: it is **automatic and policy-driven** in
the daemon (`src/agent/policy.rs:16` carries `compaction_threshold`;
`src/context/compaction.rs:23` is the `ContextTracker`;
`src/agent/loop.rs:913` sets the tracker's threshold, and the daemon applies
the policy in `src/agent/context_runtime.rs:827` (`compact_if_needed`) — the
threshold is read at `:836` and the compaction decision is made at `:858`), and
the daemon reports each one back as `AppEvent::CompactionTriggered`, which the TUI
already counts (`src/tui/runtime/app_events.rs:531`). **The defect is not
that compaction is missing — it is that the command lies about having caused
one.**

This line therefore owns the command's truthfulness, and records the separate
product question of whether a user may *force* a compaction.

The defect was **already recorded** before this line existed, by M007-D's own
plan: `plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md`
§14 — "A command that did *not* work before — `/compact` is the confirmed case
— is a **recorded discrepancy requiring an owner-visible decision**, not a
licence to change behaviour silently." The same plan classifies `/compact` as
a **daemon effect**. This line is that decision's disposition.

## 2. Why this is a corrective and not a feature

The defect is a **false report**, and that is what must close. Whether a user
*should* be able to force a compaction is a product decision about the
daemon's authority over when compaction fires, and the daemon's authority is
not this line's to widen.

The line therefore separates two things that are currently conflated in one
toast:

- **the defect** — the command reports an effect that did not happen; and
- **the question** — should `/compact` cause a compaction, and if so, who
  decides that it may?

The first is closed here. The second is recorded for the owner, with the
evidence, and is not decided by the implementer.

## 3. Milestone decomposition

| Milestone | Scope | Dependency | Status |
|---|---|---|---|
| C001 | `/compact` reports only what happened; the command's outcome is truthful, and the manual-compaction question is recorded with its evidence | none — the defect is self-contained | **closed 2026-10-31** — plan `plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md`, closure `plans/closure/tui-compact-command-corrective/001-status.md`. Shape B was taken: the command reports what compaction is and what this TUI has observed, and sends no request. The manual-compaction question is **recorded, not decided** — the proposed `CoreRequest` variant and the daemon's right to refuse are in the closure's §10.1 |

## 4. Closure boundary

This line's closure evidence is C001's closure record at
`plans/closure/tui-compact-command-corrective/001-status.md`.

**Discharged 2026-10-31.** C001 closed with **Shape B**: the command's
outcome is truthful on every path, and both of its surfaces reach one
handler. The **manual-compaction question is the owner's** and stays open —
the proposed `CoreRequest` variant, its shape, and the daemon's right to
refuse are reported in the closure's §10.1.

The command had **two** surfaces, and one of them was also wrong: the
palette's builtin arm toasted "Compaction triggered" and the catalog's
declared action promised "Compaction will occur at end of current turn",
which the daemon does not guarantee (`compact_if_needed` returns without
compacting unless the threshold is crossed,
`src/agent/context_runtime.rs:896`). Both now reach
`src/tui/commands/tasks.rs:710`.

The desktop is the line's **third** surface: it does not carry the
`compact_session` action, so `/compact` there reports that it does not
implement it. That is honest, and it is recorded as a follow-up rather than
changed here.

## 5. What this line must not do

- **No change to the daemon's compaction policy.** The threshold, the
  tracker and the loop's behaviour are not in scope; the automatic path works.
- **No new protocol variant without the owner's decision.** A
  "compact now" request is a new capability on the daemon's surface, and ADR-
  shaped authority questions are the owner's, not the implementer's.
- **No change to the TUI's rendering of `AppEvent::CompactionTriggered`.** The
  daemon's report is already handled truthfully.
