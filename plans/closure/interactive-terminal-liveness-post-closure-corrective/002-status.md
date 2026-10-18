# Interactive Terminal Liveness Post-Closure Corrective C002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/002-honest-transport-refusal-and-capability-boundary.md`
Source subsystem roadmap: `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c002--interactive-terminal-on-the-remaining-frontend-transports`
Repository baseline reviewed: `8b6191140` (`feat/auto-20261017-8835f26d`)
Implementation commits: none — the working tree at closure time contains the change; it is **not committed** at the time of writing. See §10 finding 3.

## 1. Executive finding

The defect was a **wrong reason**, not a missing feature. All nine interactive-terminal
entry points guard on `app.core_client`, and on the `/tui` WebSocket attach frontend
that guard fires because the frontend was **never given a `CoreClient`** —
`src/client/attach.rs` opens `/tui`, speaks `protocol::tui::TuiMessage`, and never
constructs one. The daemon behind that frontend is present, reachable, and correctly
serving every other surface. Each site answered with
`"Daemon connection unavailable; <operation>"`, which sends the operator to restart a
daemon that is working, check `CODEGG_DAEMON_HOME`, or debug connectivity — none of
which can ever help. A refusal that names a healthy component is worse than silence,
because it points at the wrong place with confidence.

This is the same defect class C001 closed, moved from the render path to the
diagnostic path. C001's root cause was a surface that silently showed nothing; this
one is a surface that actively misreports why.

**A second defect was found during implementation, in the same function family and
the same silent class.** `flush_terminal_input` performed the `core_client` check
*after* `take_pending_input` and `begin_input`. On the refusal path it therefore
dropped the queued keystrokes **and** left `input_in_flight` set with nothing in
flight to release it. Every later keystroke then parked behind a flag nobody would
ever clear: one visible toast, followed by permanent silence for the life of the
attachment. The refused resize had a smaller version of the same shape — the drained
size was consumed by the refusal rather than returned to the coalescing queue. Both
are fixed here, and both are covered by tests that were proven failing on baseline.

**No transport was delivered, and none is claimed.** This milestone closed the
*frontend* half of K2 only. The `/core` WebSocket endpoint still routes
`InteractiveProcess*` requests; see §8 and J5.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| Work package A: premise falsified on baseline `8b6191140` | `ws_attach_refusal_names_the_transport_not_the_daemon` failed on baseline with `toasts: ["Daemon connection unavailable; cannot create a terminal"]` | pass |
| J3 pinned at every reachable refusal site on baseline | `a_refused_terminal_command_mutates_nothing` covers create/list/attach on both frontends | pass (passed pre-change; the ordering was already right at those sites) |
| One shared classifier; all nine sites routed through it | `TerminalOperation` (`:71`), `TerminalRefusal` (`:111`), `TerminalRefusal::classify` (`:124`), `TerminalRefusal::message` (`:137`), `terminal_core_client` (`:158`); nine call sites at `:188,391,494,678,1012,1123,1215,1296,1381` | pass |
| J1 both halves proven by distinct tests | `ws_attach_refusal_names_the_transport_not_the_daemon` and `socket_daemon_without_a_connection_still_blames_the_connection` | pass |
| J2 no silent failure | `!toasts.is_empty()` in the refusal test; `the_pull_is_inert_on_the_unsupported_transport` asserts no per-tick toast storm | pass |
| J3 refusal before any mutation | refusal runs where the old `let Some(core_client) = app.core_client.clone()` ran; the input and resize sites additionally moved *earlier* (see finding 1) | pass |
| J4 pull inert on an unsupported transport | `the_pull_is_inert_on_the_unsupported_transport` mounts a terminal surface, runs 50 cadence passes, and asserts `false` every time, `active_count() == 0`, no request generation consumed, and no toast | pass |
| No silent key swallow at the wrong layer | `the_unsupported_transport_does_not_swallow_typed_keys` | pass |
| No transport work (J6) | §3 diff proof: `src/server/**`, `crates/codegg-client/**`, `crates/codegg-protocol/**`, `crates/codegg-core/**` untouched; no `CoreRequest`/`CoreResponse`/`TuiMessage`/`ServerCapabilities` variant added or changed | pass |
| No existing test weakened or deleted | no test was removed or relaxed; the 157 previously-green C001 tests in the four terminal/panel suites all still pass | pass |
| I2: no terminal bytes on the bus | no bus event, projection, transcript, or session-observation change; the change adds no data path at all, only a message string | pass |

## 3. Production implementation evidence

One production file changed: `src/tui/commands/interactive_terminal.rs`
(+371/−38 at closure time, of which the test module accounts for the largest share).

```
$ git diff --stat 8b6191140 -- src crates
 src/tui/commands/interactive_terminal.rs | 409 +++++++++++++++++++++-----
 1 file changed, 371 insertions(+), 38 deletions(-)

$ git diff --name-only 8b6191140 -- src/server crates/codegg-client \
      crates/codegg-protocol crates/codegg-core
(no output — untouched)
```

The change is three things:

1. **`TerminalOperation` (`:71`)** — the nine operation clauses
   (`cannot create a terminal`, … `terminal input dropped`), defined once.
   Nine copies of one sentence was nine places to get the cause wrong.
2. **`TerminalRefusal` (`:111`) with `classify` (`:124`) and `message` (`:137`)**
   — the two causes, made distinct and given distinct wording. `classify` keys on
   `App::is_ws_attach_transport()`, never on `AppMode`, because the socket-daemon
   frontend is also `RemoteCore` *and does* own a `CoreClient`.
3. **`terminal_core_client` (`:158`)** — the single seam every entry point now goes
   through. It returns the client or reports the classified refusal and returns
   `None`, so the existing `let Some(core_client) = … else { return }` shape — and
   therefore the existing refusal-before-`begin()`/before-spawn ordering — is
   preserved at each site rather than re-established by hand.

The rendered refusal on the attach frontend:

> This frontend is attached over a remote WebSocket, which does not carry
> interactive terminals; cannot create a terminal. Open one with `codegg` or
> `codegg daemon attach` on the machine running the daemon.

On a terminal-capable frontend with no connection the wording is byte-identical to
before: `Daemon connection unavailable; cannot create a terminal`.

Two ordering repairs sit inside the same seam rather than beside it:

- `flush_terminal_input` now classifies **before** `begin_input`, and returns the
  drained bytes to the queue on refusal (`:1005-1016`).
- `send_terminal_resize` returns the drained size to the coalescing queue on
  refusal instead of losing it (`:1123-1129`).

## 4. Verification executed

All commands run **locally** in this worktree. **Hosted CI has not run this change**
(§10 finding 2).

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tui::commands::interactive_terminal` | 23 passed, 0 failed |
| `cargo test -p codegg --lib tui::interactive_terminal` | 48 passed, 0 failed |
| `cargo test -p codegg --lib tui::panels` | 40 passed, 0 failed |
| `cargo test -p codegg --test tui_panel_docking` | 52 passed, 0 failed |
| `cargo test -p codegg --test interactive_terminal_tui` | 6 passed, 0 failed |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_audit_coverage.py` | pass |
| `python3 scripts/check_execution_ownership.py` | pass |
| `cargo fmt --all -- --check` | pass (after `cargo fmt --all`; two hunks in this file were reformatted) |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | pass |
| `bash scripts/verify.sh quick` | **Quick verification passed** |

169 tests across the five terminal/panel suites, of which 6 are new. The 157
previously-green C001 tests in the four suites the plan names are unmodified and
still pass. Per `AGENTS.md`, CI is a strict **subset** of `quick`, so this green
local `quick` is not evidence of a green CI run.

The baseline falsification (before any production change), for the record:

```
test ...::ws_attach_refusal_names_the_transport_not_the_daemon ... FAILED
test ...::a_refused_flush_keeps_the_bytes_and_releases_the_input_slot ... FAILED

  the daemon is healthy on this frontend; blaming it sends the reader to restart
  something that is working. toasts: ["Daemon connection unavailable; cannot
  create a terminal"]

  a refused flush must not leave the input slot claimed; nothing was sent, so
  nothing will ever complete and release it
```

## 5. Invariant review

- **J1 — the refusal names the real cause.** Proven by two tests that fail each
  other: the attach frontend no longer sees `Daemon connection unavailable`, and the
  socket-daemon frontend without a connection still does. Neither half is vacuous.
- **J2 — no silent failure.** Every entry point either performs its operation or
  emits a typed refusal. The pull deliberately stays *quiet* on an unsupported
  transport — it reads nothing, so it has nothing to report, and a toast per cadence
  tick would be a storm. That is an explicit, tested exception, not a silent one.
- **J3 — no state mutation before refusal.** The classifier runs at the old guard
  position at all nine sites, and at two of them (input, resize) it had to move
  *earlier* than the old guard because the old guard was already too late.
- **J4 — C001 liveness untouched.** No change to the pull's request, its cadence, its
  gating, or either surface's rendering. The existing client check inside
  `service_terminal_output_pull` is deliberately left as a silent `return false`: on
  an unsupported transport there is no PTY to read, and a refusal toast on every
  tick would be the storm C001's single-cadence design exists to avoid.
- **J5 — claim precision.** See §8.
- **J6 — no transport work.** See §3. No protocol variant, no capability flag, no
  `/core` or `/tui` handler change, no client change, no authorization change.

## 6. Failure and recovery review

- **Classifier cannot classify:** it cannot return a third state. There are exactly
  two causes and both are enumerated; an unhandled combination is a compile error,
  not a runtime fallback.
- **A refused command mid-turn:** nothing else is touched. The turn, the prompt, the
  transcript, and session state are unaffected; the refusal is one toast.
- **Daemon genuinely absent on the socket-daemon frontend:** unchanged. Same
  wording, same severity, same frequency as before this milestone.
- **Repeated refusals:** one toast per attempt, exactly as before. No rate limiter
  was added — the plan explicitly declines one.
- **Restart:** no persisted state, nothing to migrate or reconcile.

## 7. Migration and compatibility review

No database, wire, config, schema, or API change. No migration. No legacy path
removed. The change is confined to one message string on one frontend and two
ordering repairs. Users on the socket-daemon frontend see **no behavioural change**:
the wording they already had is preserved byte-for-byte, and the input/resize repairs
only alter the refusal path, which previously destroyed state.

## 8. Security review

No change to identity, authorization, capability, or scope. This milestone adds no
grant, no principal, and no authority; it touches no file in
`crates/codegg-core/src/authorization/`, no `InteractiveAuthority`, no
`can_terminate`, and no `/core` request handling. Audit coverage for
`interactive_process_create` and `interactive_process_input` is unchanged and
`scripts/check_audit_coverage.py` still passes. No terminal byte is logged; the
change introduces no data path at all (I2 holds trivially — there is nothing but a
message string).

**J5, stated explicitly because this is the claim this milestone must not make:**

> C002 closed the **frontend** half only. The `/core` WebSocket endpoint still
> accepts `CoreFrame::Request(RequestEnvelope<CoreRequest>)` and routes arbitrary
> requests, `InteractiveProcess*` included. Nothing here prevented a remote
> principal from obtaining a PTY, and nothing here claims to have. The remote
> interactive-process authorization posture — including the inverted posture in
> which a remote principal may create a PTY and run arbitrary commands but may not
> terminate it — is owned by R001 in
> `plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md`,
> which requires a new ADR before any implementation opens.

The docs written for this milestone were checked against that statement, and none of
them asserts the contrary.

## 9. Documentation and operations

Updated:

- `architecture/tui.md` — the transport boundary under *Interactive Terminals
  (M003)*, with the two-cause table and an explicit statement that `/core` is a
  separate surface whose posture is tracked elsewhere; and the "Keystrokes are
  ordered" consequence, extended with the refusal-before-`begin_input` obligation.
- `docs/tui.md` — the user-facing boundary: terminals work on plain `codegg` /
  `codegg daemon attach`, do not work through the remote WebSocket attach, and
  everything else on that frontend is unaffected.
- `.opencode/skills/tui/SKILL.md` — the hazard "never attribute a refusal to a
  component that is not at fault", with the ordering corollary.

No predecessor closure record was edited. `plans/closure/interactive-process-sessions/003-status.md`,
`plans/closure/human-execution-corrective/003-status.md`, and this workstream's own
`001-status.md` are untouched; corrections are additive here.

## 10. Unresolved findings

**1. The input-path refusal ordering was a latent silent stall (resolved here,
originally invisible).** Found during Work package B, not in the plan. On any
frontend that lost its daemon connection while a terminal was attached, one refused
flush lost the queued keystrokes *and* permanently pinned `input_in_flight`, so every
later keystroke queued silently forever. It is not a regression from this milestone —
it is older, and it was unreachable on the attach frontend (which cannot have an
attachment), which is presumably why it survived. Fixed and pinned.

**2. CI evidence is outstanding (medium, operational).** Everything in §4 is local.
Hosted CI has not run this change. Per `AGENTS.md`, CI is a strict subset of `quick`,
so the green local `quick` above is not a CI result.

**3. The change is not committed (operational).** `src/tui/commands/interactive_terminal.rs`
and three documents are modified in the working tree at closure time, alongside the
planning documents written earlier in this phase. No commit was made, because none
was requested.

**4. The plan referenced a "until C002" placeholder in `docs/tui.md` that does not
exist (low, informational).** No such text was present on baseline. The user-facing
boundary was therefore written into the section introduction rather than replacing a
placeholder; the resulting document states the delivered behaviour either way.

**5. `/terminal` on the attach frontend still enters its normal preflight before
refusing (low).** `/terminal-create`, `/terminal-list` and `/terminal-attach` refuse
with the classified message. A user who runs `/terminal` with no arguments on that
frontend may first see an unrelated workspace-binding message, because
`resolve_terminal_workspace` runs before the classifier in `start_terminal_create`.
That ordering is intentional (a missing workspace is the more specific fault) and
was left alone; it is not a false statement about the daemon.

**6. The remote-PTY authorization gap is unresolved by design (owned by R001).**
Recorded, not fixed. See §8.

## 11. Roadmap disposition

C002 is **closed**. The K2 exit condition is met on its second half — "delivered
**or** recorded as an explicit, typed transport refusal" — so this is closure, not
a deferral dressed as one. Option A (routing terminal bytes over `/core` or `/tui`)
was evaluated and rejected for this milestone; it remains ADR-gated and is not
scheduled here.

C003 remains deliberately unopened. The remote-interactive-process authorization
workstream remains `proposed` and blocked behind its own ADR (R001). Neither blocks
anything here.

## 12. Registry updates

`plans/registry.md` updated for this workstream: the subsystem row moved to
`active (C001 and C002 closed; C003 deliberately unopened)`, the C002 row moved from
Dependency-ready to the closed-implementation-plans record with this closure record
named, the execution-order paragraph gained the C002 outcome, and a dated correction
note records the two defects found during implementation.

`plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md`
updated: status line, §7's C002 section, and the §12 milestone table.

**Unblock audit: nothing newly unblocked.** R001 remains gated behind a new ADR and
C003 remains unopened; neither was waiting on C002, and C002 had no open dependency
when it started. The only status that changed anywhere is this milestone's own.