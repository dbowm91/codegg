# Interactive Terminal Liveness Post-Closure Corrective C002 — Honest Transport Refusal and Capability Boundary

Status: ready for handoff
Repository baseline: `8b6191140` (`feat/auto-20261017-8835f26d`)

Source roadmap:

- `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c002--interactive-terminal-on-the-remaining-frontend-transports`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#44-frontends-render-projections`

Applicable ADRs: none required. This milestone deliberately delivers **no** transport and
therefore introduces no protocol, no authorization change, and no new authority. The routing
question that would have required an ADR is recorded as a stop condition (§14) and assigned to a
separate registered workstream (§5, out of scope).

Primary class: **capability** (user-visible: an operator on a remote frontend is told the truth
about what the transport supports instead of being sent to debug a healthy daemon).

## 1. Objective

On a frontend transport that has no interactive-process support — today the WS-attach `/tui`
frontend — every terminal entry point must produce a **typed, accurate refusal that names the
transport and the real cause**, and must do so without mutating any UI state, opening a request
slot, or starting a background task. No user-visible message may blame a daemon that is healthy,
and no terminal operation may fail silently.

This milestone delivers the second half of K2's exit condition ("or fail with a typed, honest
refusal that names the transport"). It does **not** deliver terminal support on any additional
transport.

## 2. Why this milestone is ready

No hard dependency is open.

- C001 is closed at `plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md`,
  so the socket-daemon frontend contract this milestone must not disturb is settled and tested.
- `App::is_ws_attach_transport()` is the authoritative, guard-enforced discriminator between the
  WS-attach frontend and the socket-daemon frontend
  (`src/tui/app/mod.rs:2294`, enforced by `scripts/check_tui_transport_authority.py`).
- The `CoreClient` trait seam and all nine terminal command sites already exist and already
  branch on `core_client`.

There is no design left to do: the refusal sites exist, the discriminator exists, and the fix is
to make the existing refusal tell the truth.

## 3. Current implementation evidence

**The refusal exists but is factually wrong.** All nine terminal entry points guard on
`app.core_client` and emit one of two shapes of the same message:

```text
src/tui/commands/interactive_terminal.rs:82   "Daemon connection unavailable; cannot create a terminal"
                                          :288  "Daemon connection unavailable; cannot list terminals"
                                          :394  "Daemon connection unavailable; cannot attach to the terminal"
                                          :581  "Daemon connection unavailable; cannot resume terminal output"
                                          :919  "Daemon connection unavailable; terminal input dropped"
                                          :1026 "Daemon connection unavailable; terminal resize dropped"
                                          :1117 "Daemon connection unavailable; cannot detach the terminal"
                                          :1201 "Daemon connection unavailable; cannot terminate the terminal"
                                          :1289 "Daemon connection unavailable; cannot remove the terminal"
```

On the WS-attach frontend the daemon is present, reachable, and correctly serving every other
surface. `app.core_client` is `None` because that frontend was never given a `CoreClient` — the
TUI-attach client (`src/client/attach.rs`) opens a `/tui` WebSocket and speaks
`protocol::tui::TuiMessage`, never `CoreFrame`, and never constructs a `CoreClient`. The message
therefore sends the operator to debug the wrong component: restarting the daemon, checking
`CODEGG_DAEMON_HOME`, or blaming connectivity — none of which can ever help.

This is the same defect class C001 closed. C001's root cause was a surface that silently showed
nothing; this milestone's root cause is a surface that **actively misreports why**. A refusal
that names a healthy daemon is worse than no message, because it sends the reader somewhere the
answer is not.

**Supporting evidence.**

- The discriminator already exists and is authoritative:
  `App::is_ws_attach_transport()` binds on `AppMode::RemoteCore` **and** an owned
  `remote_send_tx` (`src/tui/app/mod.rs:2294-2299`). Mode alone is deliberately insufficient, and
  `scripts/check_tui_transport_authority.py` fails on a mode-only gate.
- Refusal ordering is already correct at `:79-84`: the `core_client` check precedes
  `dialog_state.terminal_request.begin()` and precedes `spawn_registered_tui_task`, so no request
  slot is consumed and no task is registered on the refusal path. This must be preserved, not
  assumed.
- C001's `service_terminal_output_pull` gates on a mounted surface holding a live attachment. On
  the WS-attach frontend no attachment can exist, so the pull no-ops. That is correct and must be
  pinned by a test rather than left to inference.
- `/core` is a separate endpoint and is **not** in scope. It accepts
  `CoreFrame::Request(RequestEnvelope<CoreRequest>)` and routes arbitrary requests, so an
  interactive-process request can already be sent there. C002 must not change, gate, or claim
  anything about `/core` (§5, §14, J5).

## 4. Invariants that must not regress

- **J1 — The refusal names the real cause.** On a transport with no interactive-process support,
  the message must say the transport does not support interactive terminals. On a frontend that
  *does* support them but has no live daemon connection, the existing connection-lost wording
  remains correct and must still be used. These two causes are different and must never share one
  string.
- **J2 — No silent failure.** Every terminal entry point either performs its operation or emits a
  typed refusal. No path may return without an outcome.
- **J3 — No state mutation before refusal.** The refusal must be evaluated before
  `dialog_state.terminal_request.begin()`, before `spawn_registered_tui_task`, and before any
  view, dialog, or panel state changes. A refused command must leave the app byte-identical.
- **J4 — C001 liveness is untouched.** Socket-daemon behaviour, the single cadence, bounded
  cursor-tracked reads, per-attachment read state, and both surfaces' rendering must be
  byte-for-byte unchanged. The pull must not spin on an unsupported transport.
- **J5 — Claim precision.** This milestone closes the **frontend** half only. It must never be
  described as making interactive processes unreachable for remote principals. `/core` routes
  `InteractiveProcess*` today; that exposure is a separate registered workstream
  (`plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md`) and is
  explicitly out of scope here.
- **J6 — No transport work.** No new `CoreRequest`/`CoreResponse` variant, no `ServerCapabilities`
  change, no `/core` or `/tui` handler change, no `crates/codegg-client` change, no authorization
  or capability-policy change.

## 5. Scope

### In scope

- The nine refusal sites in `src/tui/commands/interactive_terminal.rs` (and any further terminal
  entry point discovered during implementation) becoming transport-aware and accurate.
- One shared helper that classifies "interactive process unsupported on this transport" versus
  "no daemon connection", so the wording is defined once.
- Pinned tests for: the refusal text and its cause, refusal-before-mutation (J3), and the pull
  not spinning on an unsupported transport (J4).
- Documentation that states the transport boundary accurately, in `architecture/tui.md`,
  `docs/tui.md`, and `.opencode/skills/tui/SKILL.md`.

### Explicitly out of scope

- **Any transport implementation.** No `CoreClient` over `/core` for the TUI-attach frontend, no
  `TuiMessage` variants for interactive processes, no HTTP route, no `/core` or `/tui` handler
  edit. Option A was evaluated and rejected for this milestone (§10).
- The authorization posture of `InteractiveProcess*` on `/core`, including the inverted
  remote/local create-versus-terminate capability. Registered as a separate workstream; see
  `plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md`.
- A new `ServerCapabilities.interactive_processes` flag. **Considered and deliberately rejected:**
  `src/client/attach.rs` performs no `ClientHello`/`ServerHello` handshake on `/tui` (it sends
  `ProjectionCapabilities` then `Resume`), so a `ServerCapabilities` field would never reach the
  WS-attach TUI. Adding one would advertise a capability the frontend cannot observe. The
  discriminator that is actually available — `App::is_ws_attach_transport()` — is local and
  authoritative.
- C001's terminal surface work, the box viewport-plumbing gap (C003), and any polish.
- `codegg-core`, storage, schema, provider, model, scheduler, permission, and `crates/codegg-client`.

## 6. Required production changes

### Core/domain

None. `crates/codegg-core` is untouched; `scripts/check-core-boundary.sh` must stay green.

### Storage and migrations

None.

### Protocol and DTOs

None (J6). No variant, no capability flag, no wire change.

### Runtime and concurrency

All changes are in `src/tui/commands/interactive_terminal.rs` and, if needed for shared
presentation helpers, the existing TUI message/toast module.

1. **One refusal classifier.** A single helper that takes the app (or the two facts needed:
   whether a `CoreClient` exists and whether this is the WS-attach transport) and returns the
   correct refusal for the operation. Its whole purpose is to make the two causes impossible to
   confuse again — one string per operation is the defect, so the classification must live in one
   place and all nine sites must go through it.
2. **Wording.** On an unsupported transport the message must name the transport and the fact that
   it does not carry interactive terminals, and must not assert a daemon or connection problem.
   On a supported frontend with no live connection, retain the existing connection wording.
3. **Ordering preserved (J3).** The classifier runs where the current `let Some(core_client) =
   app.core_client.clone() else { ... }` runs — before `begin()`, before task spawn, before any
   state write. Verify per site rather than in aggregate.
4. **Key routing.** Typed keys on an unsupported transport must still be classified and refused
   the same way, not silently swallowed by a transport branch added in the wrong layer. The dock
   gesture and `terminal_owns_keyboard` rules from C001 are unchanged.
5. **Pull service (J4).** `service_terminal_output_pull` must continue to no-op on the WS-attach
   frontend. No branch, timer, or request may be added for it.

### Frontend or operator surface

- The toast is the surface. It must be actionable: name the transport and what it supports, so an
  operator's next move ("use the local `codegg` TUI against the daemon", or "this frontend is a
  viewer") is obvious.
- `/terminal-resume` remains the manual recovery affordance on the socket-daemon frontend and is
  untouched.

### Security and authorization

No change. This milestone must not add identity, grant, capability, or scope; must not touch
`crates/codegg-core/src/authorization/policy.rs`, `InteractiveAuthority`, or `can_terminate`;
must not gate or un-gate any `/core` request; and must not log terminal bytes (I2 of the
roadmap).

Its one security-relevant duty is negative: **the closure must not overstate what was fixed** (J5).

### Documentation and static guards

- `architecture/tui.md` §"Interactive Terminals (M003)": state the transport boundary precisely —
  interactive terminals are delivered on the socket-daemon frontend; the WS-attach frontend
  refuses with a named transport; `/core` is a separate endpoint whose interactive-process
  posture is tracked elsewhere and is not claimed here.
- `docs/tui.md`: the same in user-facing terms, replacing the existing "until C002" placeholder
  with the delivered refusal behaviour.
- `.opencode/skills/tui/SKILL.md`: add the hazard — a refusal must never name a component that is
  not at fault; classify the cause, do not guess it.
- No new guard is required. `scripts/check_tui_transport_authority.py` already enforces that the
  discriminator is `is_ws_attach_transport()`. If a new cheap guard can pin J1, prefer an existing
  guard over a new script; a new guard must be proven to fail on an injected violation before
  being cited as evidence.

## 7. Ordered work packages

### Work package A — Falsify the premise first

Intent: prove the current behaviour is wrong before changing it.

Required changes: write a test asserting that a terminal command on an app whose transport is
WS-attach produces a refusal that names the transport and does not blame the daemon. Demonstrate
it **failing on baseline `8b6191140`** with the assertion showing the current "Daemon connection
unavailable" text. Additionally pin J3: assert a refused command leaves `terminal_request`
un-consumed and registers no task.

Acceptance evidence: the failing run with its assertion message, plus the refusal-before-mutation
test.

### Work package B — The refusal classifier

Intent: define the two causes once.

Required changes: add the shared classifier; route all nine sites (plus any discovered) through
it; keep the per-operation wording ("cannot create", "input dropped", …) while replacing the
misattributed cause.

Acceptance evidence: the Work package A test passes; a test proving a supported frontend with no
`CoreClient` still gets the connection wording (so J1's second half is real, not vacuous).

### Work package C — No spin, no silent swallow on an unsupported transport

Intent: prove the C001 machinery is inert where it cannot work.

Required changes: pin that `service_terminal_output_pull` issues no request on the WS-attach
frontend; pin that a typed key on that transport is classified and refused rather than dropped by
a transport branch in the wrong layer.

Acceptance evidence: both tests, with the pull test asserting zero outbound requests rather than
merely "no crash".

### Work package D — Documentation and the boundary claim

Intent: make the transport boundary legible, and stop the docs from implying more than is true.

Required changes: update `architecture/tui.md`, `docs/tui.md`, and the TUI skill per §6. State
what is delivered, what is refused, and that `/core`'s interactive-process posture is tracked by a
separate workstream.

Acceptance evidence: the three documents updated; a reviewer reading them cannot conclude that
remote principals are prevented from creating interactive processes.

### Work package E — Scope proof

Intent: demonstrate no transport work happened.

Required changes: show `src/server/**`, `crates/codegg-client/**`,
`crates/codegg-protocol/**`, and `crates/codegg-core/**` are untouched; show the diff contains no
new protocol variant and no authorization change; confirm `/core` behaviour is bit-identical.

Acceptance evidence: the file-level diff summary, recorded in the closure record.

### Work package F — Qualify and close

Intent: prove nothing regressed, then close.

Required changes: run §11 in full; write
`plans/closure/interactive-terminal-liveness-post-closure-corrective/002-status.md` with the J1–J6
invariant review and an explicit statement of the J5 boundary.

Acceptance evidence: §11 results, labelled local vs CI truthfully, plus the closure record.

## 8. Failure, cancellation, restart, and contention semantics

- **Classifier cannot classify:** fail loudly in the test suite, not by falling back to the old
  string. There is no third cause to guess at.
- **A refused command mid-turn:** the turn continues unaffected; nothing about the refusal may
  disturb the prompt, the transcript, or the session state (J3).
- **Daemon genuinely absent on the socket-daemon frontend:** unchanged behaviour — the existing
  connection-lost toast. C002 must not make this path quieter or louder.
- **Repeated refusals:** a user hammering `/terminal` on an unsupported transport may produce a
  toast per attempt, matching current behaviour. Do not add a rate limiter in this milestone; note
  it as a C003 polish candidate if it proves annoying.
- **Restart:** no persisted state is introduced, so there is nothing to migrate or reconcile.

## 9. Compatibility and migration

No database, wire, config, schema, or API change. No migration. No legacy path removed. The
change is confined to user-visible message text plus one helper. Users on the socket-daemon
frontend see no behavioural change.

## 10. Considered alternatives (and why they were rejected here)

| Option | Disposition |
|---|---|
| **A — route terminal bytes over `/core`/`/tui`** | Rejected for this milestone by decision. It is a genuine capability, but it requires answering whether a remote principal may obtain a host shell. That is an authorization-contract change and needs a new ADR plus the separate workstream in §5. Shipping it would also require giving the TUI-attach frontend a `CoreClient`, which affects every `core_client`-based command, not just terminal — far beyond a bounded terminal milestone. |
| **A — `TuiMessage` variants for interactive processes** | Rejected even under Option A. `/core` already routes arbitrary `CoreRequest`; a parallel `TuiMessage` request family would duplicate the dispatch table and bypass the `CoreFrame` handshake. |
| **B — typed refusal naming the transport** | **Selected.** Cheap, independently correct, and it is the correct end state for any transport that does not support terminals. It is not a deferral dressed as closure: the exit condition in the roadmap's §11 is explicitly "delivered **or** recorded as an explicit, typed transport refusal". |
| **Capability advertisement** | Rejected and explained in §5: `/tui` has no `ClientHello`/`ServerHello`, so `ServerCapabilities` cannot reach this frontend. |

## 11. Required verification commands

```bash
# focused
cargo test -p codegg --lib tui::commands::interactive_terminal
cargo test -p codegg --lib tui::interactive_terminal
cargo test -p codegg --lib tui::panels
cargo test -p codegg --test tui_panel_docking
cargo test -p codegg --test interactive_terminal_tui

# guards touched by transport selection
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_execution_ownership.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy -p codegg --all-targets --locked -- -D warnings

# broader suite
bash scripts/verify.sh quick
```

Report locally run commands truthfully. CI is a strict subset of `quick` (see `AGENTS.md`), so a
green local `quick` is not a green CI run and must not be described as one.

## 12. Documentation updates

- `architecture/tui.md` §"Interactive Terminals (M003)": the transport boundary (delivered on
  socket-daemon; refused by name elsewhere), and an explicit pointer that `/core`'s
  interactive-process authorization is a separate workstream.
- `docs/tui.md`: user-facing statement of what happens if you try to open a terminal on a remote
  frontend, replacing the C001 placeholder text.
- `.opencode/skills/tui/SKILL.md`: hazard — never attribute a failure to a component that is not
  at fault; classify transport-vs-connection explicitly.
- Do **not** edit `plans/closure/interactive-process-sessions/003-status.md` or
  `plans/closure/human-execution-corrective/003-status.md`, and do not edit
  `plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md`. Any
  correction is additive, in this milestone's own closure record.

## 13. Acceptance criteria

- On the WS-attach frontend, every terminal command and key produces a typed refusal that names
  the transport and does not state or imply that the daemon or the connection is at fault.
- On the socket-daemon frontend with no live connection, the existing connection-lost wording is
  unchanged — the two causes are distinguishable in the product, not only in the code (J1).
- No terminal operation on an unsupported transport fails silently (J2).
- A refused terminal command leaves the app state unchanged: no request slot consumed, no task
  registered, no view or dialog mutated (J3).
- `service_terminal_output_pull` issues no request on the WS-attach frontend (J4).
- All 157 previously-green C001 terminal/panel tests still pass unmodified, and the C001 closure
  record is unchanged.
- `src/server/**`, `crates/codegg-client/**`, `crates/codegg-protocol/**`, and
  `crates/codegg-core/**` are untouched; no protocol variant and no authorization change exists
  in the diff (J6).
- The closure record states plainly that this milestone closed the frontend half only, and that
  `/core` still routes `InteractiveProcess*` with the posture tracked by the separate workstream
  (J5).
- The remote-PTY workstream is registered in `plans/registry.md` and linked from both this plan
  and the roadmap.

## 14. Stop conditions

Stop and report rather than improvise when:

- anyone proposes routing terminal bytes over `/core` or `/tui` in this milestone (that is Option
  A, requires a new ADR, and belongs to a new milestone);
- anyone proposes adding a `ServerCapabilities` flag or a `TuiMessage` variant to solve this
  (both rejected in §5 and §10, with reasons);
- work expands into `src/server/**`, `crates/codegg-client/**`, `crates/codegg-protocol/**`, or
  `crates/codegg-core/**` (J6);
- the refusal cannot be made accurate without changing a message that a passing test asserts —
  report the test and the conflict instead of editing the assertion to match;
- the C001 pull or either terminal surface changes behaviour on the socket-daemon frontend (J4);
- anyone proposes gating, re-authorizing, or otherwise touching `/core`'s interactive-process
  handling (J5, separate workstream);
- the docs cannot be made accurate without overstating the remote posture (J5).

## 15. Closure evidence required

- The Work package A falsification run on baseline `8b6191140`, with the assertion message.
- Both halves of J1 proven by distinct tests (unsupported transport vs no connection).
- J3 proof per refusal site, not in aggregate.
- J4 proof that the pull is inert on the WS-attach frontend, asserting zero outbound requests.
- The Work package E file-level diff summary.
- Full §11 command results, labelled local vs CI truthfully.
- An explicit J5 statement in the closure record's security review, with a link to the separate
  workstream.
- An unblock audit of `plans/registry.md` (an explicit "nothing newly unblocked" is valid, but it
  must be stated).
- Unresolved findings with severity, including low findings.

## 16. Handoff notes

- **Start at Work package A.** Prove the current message is wrong before rewriting it.
- The defect is a **wrong reason**, not a missing feature. Resist the pull to "just make the
  terminal work over WS" — that is Option A, and it is ADR-gated and out of scope here.
- All nine strings share one cause; fix the classification once. Do not patch the nine sites
  independently, which would recreate the same duplication that caused this.
- `is_ws_attach_transport()` is the only correct discriminator. A mode-only check is a known
  historical defect in this codebase and `scripts/check_tui_transport_authority.py` will fail on
  it.
- Keep the existing connection-lost wording for the genuinely disconnected case. Removing it
  would trade one lie for another.
- Do not run `--all-features`; `verify.sh full` uses
  `--features server,plugins,lsp-test-support`.