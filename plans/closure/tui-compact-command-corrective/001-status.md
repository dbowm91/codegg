# TUI Compact Command Corrective — Milestone C001 — Closure Status

Status: closed

**Milestone:** C001 — `/compact` must have a sender.

**Plan:** `plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md`.

**Roadmap:** `plans/subsystems/tui-compact-command-corrective-roadmap.md`.

**Branch:** `feat/c001-compact-command-truthful`, baseline `544327d54`.

**Date:** 2026-10-31.

---

## 1. Executive finding

`/compact` no longer lies. The command used to toast **"Compaction triggered
- reducing context"** and send nothing at all, so every invocation narrated an
effect the product never performed. It now reports what compaction **is** and
what this TUI has actually observed, and it sends no request.

The command reaches **one** handler, and both of its surfaces reach it. The
TUI's own palette entry (`/compact` → `BuiltinSlashAction::Compact`,
`src/tui/command.rs:628`) and the daemon's catalog form (`compact` →
`FrontendAction::CompactSession`,
`crates/codegg-core/src/frontend_action.rs:393`) now land on the same
function, so the two cannot drift apart again.

**Shape B was taken, and Shape A was not.** Shape A needs a new
`CoreRequest` variant, and that authority question is the owner's (§10,
finding 1). The proposed variant, its shape and what the daemon would do
with it are reported in §10.1 rather than implemented.

**One High finding is recorded rather than hidden** (§10): the count the TUI
keeps is the TUI's **observed total**, not a session's, so the message claims
no session scope.

---

## 2. Requirement-to-evidence matrix

| # | Plan requirement | Where | Evidence |
|---|---|---|---|
| 1 | `/compact` never reports an effect that did not happen | §3.1 | `src/tui/commands/tasks.rs:710` — the handler's only outcomes are the policy's shape and the observed count |
| 2 | The command's outcome is derived from what the product actually did | §3.1 | the count is `session_state.compaction_count`, incremented only by a real `AppEvent::CompactionTriggered` (`src/tui/runtime/app_events.rs:531`) |
| 3 | The daemon's automatic compaction path, threshold and tracker are unchanged | §3.3 | no file under `src/agent/` or `src/context/` is touched; `compact_if_needed` is untouched (`src/agent/context_runtime.rs:865`) |
| 4 | The TUI's handling of `AppEvent::CompactionTriggered` is unchanged | §3.3 | `src/tui/runtime/app_events.rs` is untouched |
| 5 | No new protocol variant lands without the owner's decision | §3.2 | `crates/codegg-protocol/` is untouched; the variant is reported in §10.1 |
| 6 | `scripts/verify.sh quick` is green, and fmt and workspace clippy with `-D warnings` are clean | §4 | the runs in §4.1 |
| 7 | Every surface that reaches the command is identified and corrected | §3.4 | the three surfaces are named; the two TUI surfaces are corrected and the desktop's is recorded as non-performing |
| §2.1 | The floor — no false report, on any path | §3.1 | the handler has two paths, and both are true of what the product did |
| §2.3 | The toast is not fired before the outcome is known | §3.1 | there is no outcome to wait for: the command performs nothing, so there is nothing to announce |
| §2.3 | The command does not reach into the daemon's policy | §3.1 | it sends no request at all — `the_compact_command_sends_no_request_to_the_daemon` |
| §3 | No change to the catalog's shape | §3.3 | `crates/codegg-core/src/command_catalog.rs` is untouched |
| §6.1 | A test that the message is true, never the false toast | §4.2 | `the_compact_command_reports_what_compaction_is_and_never_claims_to_triggered_one` |
| §6.3 | A test that the reported count matches the count the TUI keeps | §4.2 | `the_compact_command_reports_the_count_the_tui_keeps` |
| §6.4 | A test that the command does not reach the daemon's policy | §4.2 | `the_compact_command_sends_no_request_to_the_daemon` |
| §8.7 | Every surface that reaches the command is identified and corrected | §3.4 | `every_surface_that_reaches_the_compact_command_reports_the_same_truth` |

---

## 3. What was delivered

### 3.1 One truthful handler

`src/tui/commands/tasks.rs:710` — `handle_compact_session` — is the whole
change. It has two paths and both are true:

- **A session is bound** (`session_state.session.is_some()`): "Compaction is
  automatic: the daemon compacts when its context threshold is crossed.
  Compactions so far: N."
- **No session is bound**: "Compaction is automatic: the daemon compacts when
  its context threshold is crossed. No session is attached."

Before, the same command had **three** outcomes across **two** functions, and
two of the three were false or unguaranteed:

| Surface | Before | After |
|---|---|---|
| the TUI's palette entry (`B::Compact`) | "Compaction triggered - reducing context" — **false**: nothing was triggered | one of the two outcomes above |
| the catalog's declared action (`TuiCommand::CompactSession`) | "Compaction will occur at end of current turn" — **unguaranteed**: the daemon compacts only when its threshold is crossed (`src/agent/context_runtime.rs:896`) | the same handler, the same outcome |
| the desktop | "`/compact` is a frontend effect; the desktop does not implement it yet" — **honest**, and unchanged | unchanged (§3.4) |

The second surface was not obviously a false report, but it promised an
effect the daemon does not guarantee: `compact_if_needed` returns without
compacting unless `needs_context_compaction(...)` holds
(`src/agent/context_runtime.rs:896`), and the turn's end is not a threshold
crossing. Both surfaces now land on one function.

### 3.2 No new protocol variant

`crates/codegg-protocol/` is untouched. The variant Shape A would need is
described in §10.1 and is the owner's decision.

### 3.3 Nothing else moved

The change is one function and one arm:

- `src/tui/commands/tasks.rs` — `handle_compact_session` rewritten; the now
  unused `SessionStatus` import dropped.
- `src/tui/app/mod.rs:6046` — the `B::Compact` arm calls the handler instead
  of toasting.

Not touched: `crates/codegg-core/src/command_catalog.rs`,
`crates/codegg-core/src/frontend_action.rs`, `crates/codegg-protocol/`,
`src/agent/context_runtime.rs`, `src/context/compaction.rs`,
`src/tui/runtime/app_events.rs`.

### 3.4 Every surface that reaches the command

| Surface | Path | Disposition |
|---|---|---|
| the TUI's palette | `/compact` → `BuiltinSlashAction::Compact` (`src/tui/command.rs:628`) → `B::Compact` (`src/tui/app/mod.rs:6046`) | **corrected** — it reaches the handler |
| the catalog's declared action | `compact` → `FrontendAction::CompactSession` (`crates/codegg-core/src/frontend_action.rs:393`) → `TuiCommand::CompactSession` → `handle_compact_session` | **corrected** — it already reached the handler, and it now produces the same truthful outcome |
| the desktop | the catalog's form → `apps/desktop/src/commands/frontendActions.ts:320` — the action is **not** among the nine the desktop carries, so it falls to the default arm | **not a false-reporting surface** — it reports "`/compact` is a frontend effect; the desktop does not implement it yet" (`apps/desktop/src/App.tsx:1510`). Recorded in §10.3 |

The TUI's two surfaces are pinned to agree by
`every_surface_that_reaches_the_compact_command_reports_the_same_truth`,
which asserts the catalog's classification, the declared action, the
registry's builtin mapping, and that both paths produce the same message.

---

## 4. Verification executed

### 4.1 The plan's commands (§7)

| Command | Outcome |
|---|---|
| `cargo test --locked -- compact` | **PASS** — **137 passed, 0 failed** across the workspace's matching targets; see §4.2 |
| `cargo fmt --all -- --check` | **PASS** — clean |
| `rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings` | **PASS** — exit 0. The CI lane installs `dtolnay/rust-toolchain@stable` (clippy **1.99.0**) and the local default is 1.89, so clippy is run at the CI toolchain |
| `scripts/verify.sh quick` | **PASS** — "Quick verification passed.", with the harness-lab suite at **412 tests, 1 skipped** |

### 4.2 The new tests

`src/tui/app/compact_command_tests.rs` — four members, all green:

- `the_compact_command_reports_what_compaction_is_and_never_claims_to_triggered_one`
  — dispatches the typed `/compact` through the registry, the way a user
  reaches it, and asserts the toast is non-empty, does not say "Compaction
  triggered", does not promise "will occur", and does name the automatic
  policy.
- `the_compact_command_reports_the_count_the_tui_keeps` — binds a session,
  sets the count to 3, and asserts the toast reports 3.
- `the_compact_command_sends_no_request_to_the_daemon` — installs a
  recording `CoreClient`, dispatches `/compact`, and asserts **no** request
  reached the daemon. The threshold is the daemon's, and the command cannot
  reach it because it asks for nothing.
- `every_surface_that_reaches_the_compact_command_reports_the_same_truth` —
  the catalog's classification and declared action, the registry's builtin
  mapping, and the two surfaces' identical messages.

### 4.3 The guards that bite this change

| Guard | Outcome |
|---|---|
| `python3 scripts/check_comment_budget.py` | **PASS** — after the sanctioned bank (§5) |
| `python3 scripts/check_test_assertion_quality.py` | **PASS** |
| `python3 scripts/check_test_duplication.py` | **PASS** |
| `python3 scripts/check_command_catalog_ownership.py` | **PASS** |
| `python3 scripts/check_frontend_action_set.py` | **PASS** |
| `python3 scripts/check_adr_integrity.py` | **PASS** |

### 4.4 The hosted receipts

PR `#225` carries the milestone. The hosted lanes on head
`d64e58455`:

| Lane | Run | Outcome |
|---|---|---|
| `CI / verify` | `41221259491` | **green** |
| `Desktop E2E` | `41222463559` | **green** |
| `Windows process launcher` | `41222466035` | **green** |
| `Linux M004 process and IPC qualification` | `41222468217` | **green** |

### 4.5 The daemon's automatic compaction still fires

The daemon's own compaction family is untouched and green — **73 members of
`tests/agent_loop/compaction.inc` match `compact` and all pass**, including:
`test_compaction_trigger_under_threshold`,
`test_compaction_trigger_over_threshold`, `test_auto_compact_prunes_long_outputs`,
`test_auto_compact_no_prune_no_compact`, `test_auto_compact_with_prune`,
`test_auto_compact_async_fallback_when_no_provider`,
`llm_compaction_uses_supplied_session_context`,
`test_programmatic_compaction_preserves_tool_pairs`. The TUI's own handling
of the daemon's report is `src/tui/runtime/app_events.rs:525`, untouched.

---

## 5. Invariant review

- **The daemon's authority over when compaction fires is unchanged.** The
  threshold, the tracker and the loop are not touched. The command reads
  nothing from the policy and asks for nothing (§3.1).
- **No frontend decides compaction.** The command does not lower a
  threshold, and it cannot: it sends no request, which
  `the_compact_command_sends_no_request_to_the_daemon` proves against a
  recording client rather than by inspection.
- **One command, one handler.** Before this change the command had two
  functions with two different messages. It now has one function, and the
  surfaces are pinned to agree (§3.4).
- **The catalog did not move.** `/compact` is still a `FrontendEffect` with
  the declared action `CompactSession`, and the catalog's version is
  unchanged. The corrective changes what the TUI *says*, not what the
  daemon *serves*.
- **The comment budget was re-banked, and only the corpus moved.** The
  corpus was exactly at its recorded ceiling before this change, so any new
  comment needed a bank. The new prose was trimmed to what carries the
  reason, and `python3 scripts/check_comment_budget.py --bank` moved
  `docs/comment-budget.toml` by exactly the corpus delta — **+12 comment
  lines and +114 words** — with **no** per-file ceiling changed. The
  manifest was not hand-edited.

---

## 6. Failure and recovery

- **A future change could make the command lie again.** The guard against it
  is the test that asserts the false toast is absent, plus the one that
  asserts no request is sent. A change that reintroduces "Compaction
  triggered" fails the first.
- **A future change could split the two surfaces apart again.** The
  every-surface test asserts they produce the same message, so a second
  handler fails it.
- **The desktop's `/compact` does nothing.** It reports that it does not
  implement the action. If the desktop ever carries the action, it must
  reach the daemon's own report — not a local toast — which is §10.3's
  follow-up.

---

## 7. Migration and compatibility

- **No protocol change.** No `CoreRequest`, no `CoreResponse`, no DTO.
- **No durable-state change.** No migration, no schema version, no store.
- **No configuration change.** The compaction config is untouched.
- **The visible change is the toast's text.** A user who typed `/compact`
  used to see a claim that something happened; they now see what compaction
  is and how many times this TUI has seen it. That is the milestone's
  point: the old text was false.

---

## 8. Security

None. The change is a toast's text and the arm that shows it. It reads no
file, starts no process, opens no socket, and adds no dependency. The
recording client in the test exists to **prove** that no request is sent.

---

## 9. Documentation and operations

- This record is the closure: `plans/closure/tui-compact-command-corrective/001-status.md`.
- The registry's subsystem row and its dependency-ready row carry the
  milestone's new state, and the roadmap's C001 row carries its closure.
- The plan's own anchors are stale — they were written against baseline
  `b25439def`, before M007-H kind-corrected `/compact` — and §10.4 records
  the corrected ones.
- No operational change: no flag, no runbook step, no rollout.

---

## 10. Unresolved findings

### 10.1 Shape A needs a new `CoreRequest` variant — the owner's decision (medium, **open**)

Plan §4's **stop condition 1** was hit: Shape A needs a new protocol
variant, and the authority question — *may a frontend ask the daemon to
compact now, and does the daemon retain the right to refuse?* — is the
owner's. Nothing was added under this milestone, and this is the report the
stop condition asks for.

**The proposed variant, its shape, and what the daemon would do with it:**

| Question | The proposal, for the owner to accept or reject |
|---|---|
| the variant | `CoreRequest::SessionCompact { session_id }` — one request, no arguments beyond the session it names |
| what the daemon would do | resolve the session's own execution policy, take the session's message history, and run **the same** `compact_if_needed` path (`src/agent/context_runtime.rs:865`) with the threshold check forced satisfied — not a second compaction engine |
| the daemon's right to refuse | the daemon **retains** it. It refuses with a typed reason when the session is unknown, when the compaction config disables it, when the session is mid-turn, or when no summarization model is available |
| what the frontend would show | the toast is derived from the daemon's `AppEvent::CompactionTriggered` report, and a refusal is shown as a refusal — never as a success |
| what would have to be decided | whether a frontend may ask at all; whether forcing bypasses the configured `auto` flag; whether a mid-turn request is refused or queued; the typed refusal's shape |
| what it would cost | a new `CoreRequest` variant, a daemon handler, an effect-table row, and the TUI's own arm changing from "report" to "ask" |

**Disposition:** reported, not decided. Shape B applies until the owner
answers.

### 10.2 The count the TUI keeps is the TUI's observed total, not a session's (medium, **recorded**)

`session_state.compaction_count` is incremented by every
`AppEvent::CompactionTriggered` the TUI receives
(`src/tui/runtime/app_events.rs:531`) and is **not** reset when the bound
session changes — the `/new` path resets the token counters but not this one
(`src/tui/app/mod.rs:10792`). So "this session has compacted N times" would
be **false** after a session change.

The message therefore reports the count **without** claiming a session
scope: "Compactions so far: N." The `/context` panel renders the same number
under the same scope (`src/tui/app/mod.rs:14750`), so the two agree.

Making the count session-scoped would change what the panel renders, which
is outside this milestone's scope. It is a follow-up.

### 10.3 The desktop does not carry the `compact_session` action (low, **recorded**)

The desktop carries nine declared frontend actions and not `compact_session`
(`apps/desktop/src/commands/frontendActions.ts:302`), so `/compact` in the
desktop falls to the default arm and reports "`/compact` is a frontend
effect; the desktop does not implement it yet"
(`apps/desktop/src/App.tsx:1510`).

That is **honest** — the desktop neither lies nor performs — so it is not
this milestone's defect. But the desktop cannot perform the command, and
when it gains the action it must derive its answer from the daemon's report
rather than a local toast, exactly as Shape A would require.

### 10.4 The plan's anchors were stale (low, **closed**)

Plan 001 was written against baseline `b25439def`. The anchors it names have
moved, and one of its premises moved with them: M007-H kind-corrected
`/compact` to a `FrontendEffect` with the declared action `CompactSession`,
so the command's two surfaces are now a builtin arm **and** a catalog form.
The corrected anchors are §3's; the plan itself is not amended.

### 10.5 `scripts/verify.sh full` was not run (medium, **open**)

The plan's §7 names `quick`, `fmt` and workspace clippy. `verify.sh full`
adds the nextest workspace sweep and a bounded feature pass. The **clippy**
half was run separately, at the CI toolchain, and is clean. The nextest
workspace sweep and the bounded feature pass
(`--features server,plugins,lsp-test-support --profile subset`) were **not
run** on this machine, which is shared and loaded. Recorded rather than
claimed.

---

## 11. Roadmap disposition

**C001 is closed.** The corrective line's single milestone is implemented,
and the line's closure boundary — §4 of its roadmap, "This line's closure
evidence is C001's closure record" — is discharged by this record.

The line's scope is met: the command no longer reports an effect that did
not happen, the outcome is derived from what the product did, the daemon's
automatic compaction is untouched, and no new protocol variant landed
without the owner's decision.

The one question the line deliberately did not answer is the owner's:
whether a user may **force** a compaction. It is reported with its evidence
in §10.1, and it stays open until the owner answers it.
