# Desktop Frontend and IDE Foundation Milestone 017 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/017-the-forms-the-daemon-serves-and-the-tui-does-not-perform.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Source closure of the predecessor:

- `plans/closure/desktop-frontend-ide-foundation/014-status.md`

Applicable ADR:

- **ADR-0052** — `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`,
  accepted 2026-10-24. The daemon owns the command catalog and the execution
  path; a frontend may not own one.

Repository baseline reviewed: `cb316f75` — `origin/main` with M007-D's
closure, plan 017's registration, and the two owner-directed corrective
milestones landed.

Implementation commits on this milestone's branch:

- `e1c07de88` — the desktop's frontend-action dispatch, the action identity
  on the bridge, and the palette's mouse route.
- `caa219c6b` — the six kind corrections, the action identity on the wire,
  `shell_kill`'s authority, and the presence panel's form.

## 1. Executive finding

**The catalog is now honest about every form whose classification M007-D
recorded, except the three the owner owns, and the desktop can now act on a
frontend-effect form at all.**

M007-D left **12** served forms the TUI did not perform. This milestone
resolves **nine** of them and records the remaining **three** as deliberate
bypasses:

- **Six kind corrections** are applied. `skills`, `status`, `tool_contracts`,
  `worktree`, `shell_list` and `terminal_show` are frontend effects: the
  TUI's arms perform their own effects and the daemon's rows served
  *different* ones. Each command's kind and its form's kind become
  `FrontendEffect`, each `capability` becomes `None`, and the six rows leave
  the effect table in **both** of its views.
- **The presence panel has a form.** `presence_snapshot_get` was a daemon
  effect with no catalog entry at all — a bypass with nothing to route
  through. `collaborators` gains a `presence` form whose row produces it.
- **`shell_kill` is authorized against the run's own binding**, not the
  caller's scope, which is what plan 017 §3.1's *authority* class required.
- **Three granularity forms stay recorded**, not migrated: `shell_show`,
  `shell_expand` and `terminal_send` are reached by a polling loop and by
  the per-keystroke input path, and routing either through `command.execute`
  would open a canonical `Run` per poll or per keystroke. The owner's
  decision on them has not been made, so — per plan 017 §9's stop
  condition — they are recorded and not implemented.
- **The two `/lsp-*` forms are not this milestone's work.** They are decided
  and route to **M007-F**.

The daemon now serves **46** forms and the TUI executes **40** of them
through `CoreRequest::CommandExecute` (M007-D's figures were 51 and 39).

**One thing the milestone could not do, and the reason it matters.** The
desktop palette had **no** frontend-action implementation at all: every
frontend effect was answered with *"is a frontend effect; the desktop does
not implement it yet"*. The action identity was also **not on the wire** —
`CommandFormDto` carried a form's `kind` but not the action that serves it,
so a second frontend could not know what to perform. Both are fixed here:
`CommandFormDto` gains `action: Option<String>` carrying the action's
stable wire name, and the desktop dispatches on it. **Three of the six
actions the desktop cannot yet carry**, because the daemon does not publish
the data they need; §10.1 names them, and each is reported honestly rather
than faked.

## 2. Requirement-to-evidence matrix

Plan 017 §5's seven required changes, against what this milestone did.

| # | plan 017 §5 | status | evidence |
|---|---|---|---|
| 1 | Correct the six kind-classified forms; declare the `FrontendAction` each arm performs; drop the wrong effect-table rows | **done** | `crates/codegg-core/src/command_catalog.rs` — six descriptors (`status:317`, `skills:821`, `shell_list:2273`, `terminal_show:2489`, `tool_contracts:3144`, `worktree:3162`); `crates/codegg-core/src/frontend_action.rs:67-72` (variants), `:108-113` (wire names), `:326-355` (declarations); `src/core/daemon_commands.rs` — the six rows dropped from the `build_inner_request` arms **and** from `EFFECT_TABLE` |
| 2 | Settle the three granularity forms per §3.1 | **not implemented — recorded** | §3.5 here; the owner's decision has not been made, and plan 017 §9 sanctions landing the rest without it |
| 3 | Settle `shell_kill`: the row takes the run's identity from the argument and resolves the authority from the run's own binding | **done** | `src/core/daemon_commands.rs:566` (the row), `:86` (`CommandContext.authority`), `:98` (`argument_identifier`), `:751` (`resolve_command_authority`); `src/shell/daemon.rs:433` (`scope_for`), `:24` (`HumanShellRunScope`) — §3.3 |
| 4 | Give the presence panel a form, or record it as a named exception | **done — a form** | `crates/codegg-core/src/command_catalog.rs:2776` (the `presence` form); `src/core/daemon_commands.rs:344` (the row); `src/tui/commands/presence.rs:22` (`PresenceRefresh`) — §3.4 |
| 5 | Re-baseline the classification artifact; the generator's `--check` must pass | **done** | `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md` regenerated from the tree; `--check` and `--self-test` both pass (§4) |
| 6 | Update `architecture/command.md`, `architecture/command_routing.md` and `architecture/lsp.md`, and closure 014 | **done** | `architecture/command.md:294-296` and `:686-688` carry the new counts. `command_routing.md` and `lsp.md` carry **no** catalog counts, so no numeric change is needed there — recorded in §9 |
| 7 | Record the decision shapes in the closure and the architecture documents | **done** | §3.3, §3.4, §3.5 and §10 of this record |

## 3. What was delivered

### 3.1 The six kind corrections

The kind lives on the **form**, with a mirrored command-level value
(`crates/codegg-core/src/command_catalog.rs`: the form's `kind` and the
descriptor's `kind`, documented as "the kind of its **default** form").
Each of the six is the default form of a single-form command, so each
conversion changed three fields on one descriptor: the descriptor's `kind`,
its `capability` (`None` — a command with no daemon-effect form names no
capability, and `scripts/check_command_catalog_ownership.py` rejects one
that does), and the form's `kind`.

Six new `FrontendAction` variants serve them: `ShowSkills`, `ShowStatus`,
`ShowToolContracts`, `ShowWorktrees`, `ShowShellRuns` and `ShowTerminal`
(`crates/codegg-core/src/frontend_action.rs:67-72`), with the wire names at
`:108-113` and one declaration each at `:326-355`.

**`terminal_show` does not reuse `FocusTerminal`.** `/terminal-show` calls
`handle_terminal_show` (`src/tui/commands/interactive_terminal.rs:1612`),
which **opens the terminal box on a named handle and makes it the active
terminal**; `/terminal-focus` calls `focus_terminal` (`:1752`), which
**only takes the PTY's keyboard** of a terminal already on screen — it opens
no box and sets no active terminal. One action for both would tell a second
frontend to open a box where the TUI only focuses. The two effects differ,
so the two actions differ.

### 3.2 The action identity on the wire

`CommandFormDto` gains `action: Option<String>`
(`crates/codegg-protocol/src/core.rs:1248`): the action's stable wire name
for a frontend-effect form, `None` for a daemon-effect form. The field is
**additive** and `#[serde(default)]`, so an older client still decodes the
DTO. It is populated from `FrontendAction::as_str()`
(`src/core/daemon_commands.rs:734`), so the string is generated from the
enum and **cannot be a typo**.

The daemon's catalog DTO is the only place the action is published. The
desktop's Rust host forwards the field rather than deriving it
(`apps/desktop/src-tauri/src/route.rs:650`; the struct's own field at
`apps/desktop/src-tauri/src/bridge.rs:688-695`), and the renderer mirrors
it (`apps/desktop/src/bridge-types.ts:434-438`).

**No new `CoreRequest` variant, no new capability, and no generic host
passthrough** were added — plan 017 §2 forbids all three, and the action
identity is a field on an existing DTO, published by the daemon that owns
the catalog.

### 3.3 `shell_kill`: the run's own binding is the authority

The cancel's authority is the **run's own** project, workspace and session,
not the caller's scope: a row built from the caller's scope could address
the wrong run.

The obstacle was that `build_inner_request` is a synchronous, pure free
function with no access to daemon services, and `CommandContext` carried
only the caller's scope. The resolution is that the daemon **resolves the
run's scope before the row is built**:

- `HumanShellRunService::scope_for(run_id)` (`src/shell/daemon.rs:433`)
  answers the run's own binding from the daemon's registry. The runs are in
  memory, so it answers `None` for a run the daemon does not hold.
- `CoreDaemon::resolve_command_authority` (`src/core/daemon_commands.rs:751`)
  resolves it before the row is built and carries it in
  `CommandContext.authority` (`:86`).
- `argument_identifier` (`:98`) is shared by the row and the resolver, so
  they cannot read different identities out of one argument.
- The row (`src/core/daemon_commands.rs:566`) builds
  `human_shell_run_cancel` from **the authority's** project, workspace and
  session.

A run the daemon does not hold has no binding, so the row **refuses the
call** rather than building a scope-built request — and the form stays
served, because serve-ability is a property of the form
(`serves_form`, `src/core/daemon_commands.rs:629`), not of one call's
argument.

**The behaviour this changes, named.** The old row could cancel only a run
in the caller's *exact* session, because the daemon's `cancel` compares the
whole scope. The new row cancels the named run whenever the caller holds
authority on the run's project and is the run's creator principal. That is
the disagreement plan 017 names: the TUI's own `/shell-kill` arm already
sent the run's own binding, so the row now agrees with it.

**The TUI's `/shell-kill` arm is unchanged.** It still issues
`CoreRequest::HumanShellRunCancel` directly — a pre-existing bypass, not
this milestone's (§3.5).

### 3.4 The presence panel has a form

`presence_snapshot_get` was a daemon effect the catalog did not name, so
the presence panel reached it outside the catalog — a bypass with no entry
to route through.

`collaborators` gains a `presence` form
(`crates/codegg-core/src/command_catalog.rs:2776`), and the row
(`src/core/daemon_commands.rs:344`) produces `presence_snapshot_get`. The
panel reads a **named** project, so the project travels in the argument.

**The hint path is a named bypass, and the reason is the heartbeat.** The
daemon publishes the `PresenceUpdated` liveness hint in answer to a presence
**heartbeat** (`src/core/daemon_projection.rs:26-81`), and a client
heartbeats on a `lease_ttl / 3` cadence — the default lease of 90 s makes
that every 30 s (`crates/codegg-core/src/presence.rs:148`). A
hint-triggered read is therefore a read **per heartbeat**, and routing it
through `command.execute` would open a canonical `Run` per poll — exactly
what plan 017 §3.1's granularity rule forbids.

So the two are distinguished explicitly, by a type rather than by a
convention (`src/tui/commands/presence.rs:22`):

| trigger | path | audited? |
|---|---|---|
| a **user** action — panel open, explicit refresh, reconnect, tab or project switch | the `presence` form, through `command.execute` | **yes** — a canonical `Run` |
| the daemon's `PresenceUpdated` **hint** | `CoreRequest::PresenceSnapshotGet` directly | **no** — a named bypass |

The call sites are in `src/tui/runtime/command_dispatch.rs` (both arms),
`src/tui/app/mod.rs`, `src/tui/app/project_session.rs` and
`src/tui/commands/project_picker.rs`.

**Two presence requests stay outside the catalog**, and both are named:
`PresenceCapabilities` (the negotiation — a capability probe, not an
effect) and `PresenceSnapshotGet` on the hint path only.

### 3.5 The three granularity forms: recorded, not migrated

`shell_show` and `shell_expand` are issued by the `!`-shell path's
**polling loop**, one read per poll; `terminal_send` is the interactive
terminal's **input path**, one request per keystroke. Routing either
through `command.execute` would open a canonical `Run` per iteration, and
the `Run` record is an audit of a *user action*, not of a keystroke.

Plan 017 §3.1 put three options to the owner: keep the path outside the
catalog with the bypass recorded; make the catalog's form cover the path and
accept the `Run` granularity; or give the path a coarser command. **The
owner has not decided.** Plan 017 §9's stop condition says the milestone
lands the rest and records these three as awaiting a decision, and that is
what happens here. Nothing about these three forms changed: their rows stay
in the effect table, their arms keep their behaviour, and no `Run` per poll
or per keystroke was introduced.

### 3.6 The desktop gained a frontend-action surface

The desktop had **no** frontend-action implementation: every frontend effect
was answered with *"is a frontend effect; the desktop does not implement it
yet"* (`apps/desktop/src/App.tsx`). It now dispatches on the action the
daemon publishes (`apps/desktop/src/App.tsx:1297` →
`apps/desktop/src/commands/frontendActions.ts:124`), and every palette row
is a `<button>`, so every catalog command is reachable by mouse as well as
by keyboard (`apps/desktop/src/commands/CommandPalette.tsx:253-281`).

| form | action | the desktop can run it? | where the data comes from |
|---|---|---|---|
| `skills` | `show_skills` | **yes** | `SkillCatalogueView` — the daemon's own catalogue read, which the desktop already makes for the route-selected project |
| `status` | `show_status` | **partial** | `SessionPresentationView.state` and the turn's token counters. The **current model is not available** — no model field exists in the desktop's projection DTO |
| `worktree` | `show_worktrees` | **yes, session-scoped** | `SessionPresentationView.worktrees` — the **managed** worktrees the daemon's projection already carries. The TUI's own arm is a *project-directory* listing, and no path crosses the bridge |
| `tool_contracts` | `show_tool_contracts` | **no** | the projection carries the tools a turn *used* and the background *programs*, not the contract registry |
| `shell_list` | `show_shell_runs` | **no** | the projection publishes no human-shell runs |
| `terminal_show` | `show_terminal` | **no** | the desktop has no terminal surface, and the projection publishes no interactive-process list |

The three it cannot carry are listed in `UNCARRIED_ACTIONS`
(`apps/desktop/src/commands/frontendActions.ts:56`) with the **desktop's
own** reason — what the desktop is missing, never what the command means.
They are reported honestly, not faked, and the dispatch degrades safely: an
action the desktop does not carry falls through to the honest notice rather
than guessing.

### 3.7 The catalog version moved to 2

`THE_CATALOG.version` is **2** (`crates/codegg-core/src/command_catalog.rs:242`),
because the catalog's contents changed — six forms reclassified and one
added. The catalog's own documentation says the version is bumped whenever
its contents change. Every client's cached palette is therefore stale, and
the daemon refuses a stale catalogue with `REFUSAL_STALE_CATALOG`
(`src/core/daemon_commands.rs:53`).

### 3.8 The comment budget was at zero headroom, and was paid for

The comment budget is a strict ratchet with **zero headroom**: the corpus
sat at exactly its recorded ceiling, and every file at its own. The first
`verify.sh quick` on the changed tree failed —
`R4 the corpus holds 126718 comment lines, over the recorded ceiling of
126485`.

**The guard and the manifest were not edited.** The new comments were paid
for by compressing pre-existing prose **in the same twelve files the change
already touched**, and by removing one dead no-op block in
`refresh_collaborators`. The compressions are: `daemon_commands.rs`'s
module-doc refusal table (which duplicated the `REFUSAL_*` constants' own
docs) and its `build_inner_request` JSON paragraph (a verbatim duplicate of
`json_request`'s own doc); `presence.rs`'s module and function docs; the
protocol DTO field docs; `frontend_action.rs`'s module doc;
`command_catalog.rs`'s plain-data paragraph; `shell/daemon.rs`'s
`process_admission` doc; and two test module docs.

## 4. Verification executed

Every command below was run on this milestone's tree, and every one exited
**0**.

| command | result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `python3 …/generate_m007d_command_classification.py --check` | `check passed: … matches the tree` |
| `python3 …/generate_m007d_command_classification.py --self-test` | `self-test passed: the generator counts constructions, not patterns` |
| `python3 scripts/check_command_catalog_ownership.py` | `command catalog ownership: clean` |
| `python3 scripts/check_command_catalog_ownership.py --self-test` | `self-test passed: 9 injected defect classes were all rejected` |
| `python3 scripts/check_frontend_action_set.py` | `frontend-action set: clean` |
| `python3 scripts/check_frontend_action_set.py --self-test` | `self-test passed: 5 injected defect classes were all rejected` |
| `cargo test --locked --test core_daemon -- command_catalog_m007d` | 12 passed; 0 failed |
| `cargo test --locked -p codegg --lib command_route` | 29 passed; 0 failed |
| `cargo test --locked -p codegg --lib presence` | 13 passed; 0 failed |
| `cargo test --locked -p codegg --lib shell` | 518 passed; 0 failed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0, no warnings |
| `scripts/verify.sh quick` | `==> Quick verification passed.` |
| `cd apps/desktop && npm run typecheck` | exit 0, no diagnostics |
| `cd apps/desktop && npm test` | 31 files, 233 tests, all passed |
| `cd apps/desktop && npm run bindings:check` | `desktop bridge DTO fields match` |

The implementation's own runs, on the same tree and independent of the
above, add: `cargo test --locked --test tui_render -- presence_m002_collaborators`
(12 passed), `cargo test --locked -p codegg --lib daemon_commands` (15
passed), `cargo test --locked -p codegg-core` (every binary ok), and
`cargo test --locked --test frontend_parity` (6 passed).

**Not run locally, and why.** The whole-workspace `nextest` sweep was not
run: this milestone's changes are confined to the command catalog, the
effect table, the presence arm and the desktop's palette, and the scoped
targets above cover each of them. The `scheduler_jobs` family was not run at
all — M008 §16 forbids running a sweep against the developer's own scratch
root. The hosted Desktop E2E lane is the parent's to trigger.

### 4.1 The counts, before and after

| measure | before | now | pinned in |
|---|---|---|---|
| commands | 161 | **161** | `tests/core_daemon/command_catalog_m007d.inc:187` |
| argument forms | 166 | **167** | `tests/core_daemon/command_catalog_m007d.inc:192` |
| daemon-effect forms | 134 | **129** | `tests/core_daemon/command_catalog_m007d.inc:195` |
| frontend-effect forms | 32 | **38** | `…m007d.inc:196`; `src/tui/command_route_tests.rs` |
| daemon-effect forms the effect table serves | 51 | **46** | the generated artifact only (`m007-d-command-classification.md:40`) — no Rust test pins it |
| frontend-effect forms covered by the declared action set | 32 | **38** | the generated artifact (`:43`) |
| daemon-effect forms the TUI arm executes through `command.execute` | 39 | **40** | `src/tui/command_route_tests.rs:43` — the `MIGRATED` list, 40 entries, the presence form at `:90` |

**The artifact's hardcoded action count is now derived.** The generator
carried the literal "29 `FrontendAction` variants"; it now counts the
parsed set (`generate_m007d_command_classification.py:1103`), so the
figure cannot go stale.

## 5. Invariant review

Plan 017 §7's acceptance criteria.

| # | criterion | held? | evidence |
|---|---|---|---|
| 1 | **The catalog is honest** — a form whose arm is frontend-local is `FrontendEffect`; a form the daemon serves is `DaemonEffect` | **yes, for the six** | §3.1. The two `/lsp-*` forms remain served and unmigrated by decision (§3.2 of the predecessor's closure), and the three granularity forms remain served because their rows are correct — it is the *auditing* that does not apply to them (§3.5) |
| 2 | **No form is served and unused** — every daemon-effect form the daemon serves is executed through the daemon by the TUI, or recorded with a named reason | **yes** | 40 of the 46 served forms are executed. The six unexecuted are: the three granularity forms, recorded in §3.5 with the poll/keystroke reason; the two `/lsp-*` forms, decided and routed to M007-F; and the presence form's *hint* path, a named bypass in §3.4 |
| 3 | **The desktop palette agrees with the TUI** — a form the TUI performs locally is not dispatchable to the daemon in the palette, and a form the daemon owns is | **yes** | §3.6: the desktop dispatches on the published action, and the six corrected forms are no longer dispatchable to the daemon |
| 4 | **Every user action that reaches a daemon effect has a `Run`**, except where the owner has decided otherwise and the exception is recorded | **yes** | The presence panel's user-triggered refreshes are audited (§3.4). The hint path is not a user action and is recorded. The three granularity forms are not implemented, so they open no `Run` at all |
| 5 | **The `Run` record keeps its meaning** — no form opens a `Run` per keystroke or per poll unless the owner has decided that it should | **yes** | §3.5: the three granularity forms are not migrated precisely so that no `Run` per poll or per keystroke is introduced |
| 6 | **The authority of a cancel is the run's own** — `shell_kill` cannot address a run outside the caller's scope | **yes** | §3.3: the row authorizes against the run's own binding, resolved from the daemon's registry before the row is built |
| 7 | **The artifact matches the tree**, and its §1, §3 and §6 counts agree with the catalog and the effect table | **yes** | §4.1; the generator's `--check` passes |
| 8 | **The TUI's commands, names, aliases and descriptions are unchanged** | **yes** | no change to `src/tui/command.rs`'s registry or `src/tui/app/commands.rs`; the 169 cached built-in rows and the 161 `BuiltinSlashAction` variants are unchanged (`architecture/command.md:683-685`) |
| 9 | **`scripts/verify.sh quick` is green**, including both catalog guards' `--self-tests`, and the workspace clippy is clean at `-D warnings` | **yes** | §4 |
| 10 | **The closure names every decision this milestone made, and the ones it did not make, with their reasons** | **yes** | §3.3, §3.4, §3.5 and §10 |

## 6. Failure, cancellation, restart, and contention semantics

- **A `shell_kill` for a run the daemon does not hold is refused**, with the
  form still served: the refusal is about the argument, not about the form.
- **The presence panel's user refresh is audited; its hint refresh is not.**
  A hint refresh opens no `Run`, so the count of presence `Run`s is the
  count of *user* refresh attempts, not of refreshes.
- **A `Run` is opened before the effect runs**, so a refused refresh still
  leaves a `Run` — the count is of attempts, not of successes. That is the
  pre-existing audit shape, unchanged here.
- **The catalog version moved to 2**, so a client holding a version-1 palette
  is refused with `REFUSAL_STALE_CATALOG` rather than shown a catalogue
  whose classification is stale.

## 7. Migration and compatibility review

- **The protocol change is additive.** `CommandFormDto.action` is
  `#[serde(default)]` and last in the struct: an older client decodes a
  version-2 DTO and simply sees no action, which is the same "the desktop
  does not implement it yet" answer it gives today.
- **No `CoreRequest` variant was added**, and no capability was added. The
  action identity is a field on an existing DTO, published by the daemon
  that owns the catalog.
- **The TUI's own behaviour is unchanged** for every form. The six
  kind-corrected forms keep their local arms; the presence panel keeps its
  panel; `/shell-kill` keeps its own arm and its direct request.
- **The desktop's behaviour changes in three places**, all of them
  corrections: the six corrected forms are no longer dispatchable to the
  daemon, the palette's rows are now buttons (so every command is
  mouse-reachable), and an action the desktop cannot carry is reported with
  the desktop's own reason rather than a generic notice.
- **The catalog version bump makes every client's cached palette stale**,
  which is the intended effect of the bump and is refused with a typed
  reason rather than a wrong catalogue.

## 8. Security review

- **The `shell_kill` correction narrows authority.** The row authorizes
  against the run's own binding, so a caller cannot cancel a run it does
  not own by naming it in a scope it controls.
- **The presence form carries the project in the argument**, and the daemon
  re-authorizes the inner request on its own operation — the frontend never
  supplies the request itself. The project is therefore not trusted from
  the caller's scope.
- **The action identity is not a capability.** It names an action the
  frontend performs; it grants the frontend nothing the declared action set
  did not already grant.
- **The hint path opens no `Run`**, so it is not audited as a user action.
  It is a read of the presence snapshot, and it is recorded as a bypass in
  §3.4 rather than left implicit.
- **No secret, credential, or path leaves the daemon** through the new
  field: the action's name is a static string from an enum.

## 9. Documentation and operations

- **`architecture/command.md` carries the new counts** at `:294-296` and
  `:686-688`. ⚠️ `src/tui/command.rs:1428-1462` asserts that **every**
  number in that document matching `(\d+)\s+(hardcoded|built-in|total)` or
  `[Cc]ount is (\d+)` equals **169**. The new counts are phrased so that
  they do not match those patterns, and
  `cargo test --locked -p codegg --lib command_docs_count_matches_registry`
  passes.
- **`architecture/command_routing.md` carries no catalog counts.** It
  documents the TUI's routing to subsystems, not the command catalog, so
  plan 017 §6's "update it for the corrected counts" is satisfied by
  **no change** — recorded here rather than left as a silent omission.
- **`architecture/lsp.md` carries no catalog counts** either, and the two
  `/lsp-*` forms are untouched by this milestone. Its M007-D section
  (§"The `/lsp-*` command forms (M007-D)") remains accurate: the daemon
  still classifies 26 `/lsp-*` forms as daemon effects, still serves 2, and
  still has no request for 24. No change is needed, and none was made.
- **The classification artifact is generated, never hand-edited.** It is
  regenerated from the tree and its `--check` passes.
- **The two owner-directed corrective milestones** are registered and
  re-pinned to the post-M007-D main by PR #197; this milestone does not
  touch them.

## 10. Unresolved findings

| finding | severity | disposition |
|---|---|---|
| The desktop cannot carry **three** of the six corrected actions: `show_tool_contracts` (the projection carries the tools a turn used, not the contract registry), `show_shell_runs` (the projection publishes no human-shell runs) and `show_terminal` (the desktop has no terminal surface) | medium | **recorded, and reported honestly in the product.** Each is listed in the desktop's `UNCARRIED_ACTIONS` with the desktop's own reason (`apps/desktop/src/commands/frontendActions.ts:56-61`), and the dispatch falls through to the honest notice rather than guessing. Before this milestone the desktop **dispatched these three to the daemon** — where the daemon ran a *different* effect (the tool *programs*, the shell *capabilities*, an interactive-process *listing*). The desktop now reports what it does not have instead of running the wrong thing. Carrying them needs the **daemon to publish** the data, which is a new projection surface and out of this milestone's scope |
| `show_status` **omits the current model**: the desktop's projection publishes no model field, and the TUI's arm reads its own `agent_state.current_model` | low | **recorded.** The report carries the session state and the turn's token counters, which the projection does publish, and names the model's absence rather than showing a stale one |
| `show_worktrees` reports **session-scoped managed worktrees**, where the TUI's arm is a *project-directory* listing (`CoreRequest::WorktreeList { project_dir }`) | low | **recorded.** No path crosses the bridge by design, so the desktop cannot do that listing. The two surfaces are related but not the same, and the daemon's own old row served the managed-worktree family |
| The **three granularity forms** (`shell_show`, `shell_expand`, `terminal_send`) are still not migrated | medium | **awaiting the owner's decision.** Plan 017 §3.1's three options; §3.5 records why the milestone did not implement them and what each option would cost |
| The **two `/lsp-*` forms** remain served and unmigrated | low | **decided and routed to M007-F**, which needs its own ADR for the daemon's LSP command surface |
| The **28 remaining catalog kind changes and 24 new `FrontendAction` declarations** M007-D recorded are still recorded | low | **unchanged.** This milestone applied the six that belonged to it; the rest are a follow-up |
| `command_routing.md` and `lsp.md` were **not changed**, though plan 017 §6 names them | low | **recorded, and the reason given in §9**: neither carries a catalog count. A document that does not state the number cannot drift from it |
| The **comment budget has zero headroom** — the corpus sat at exactly its ceiling, and this milestone's own comments had to be paid for by compressing pre-existing prose in twelve files | low | **recorded.** §3.8 names every compression. The next change in this area will hit the same ceiling, and the budget is a ratchet by design |

## 11. Roadmap disposition

- **M007-D — conditionally closed.** Its closure record is
  `plans/closure/desktop-frontend-ide-foundation/014-status.md`. This
  milestone carries the **12** forms its §10.6 recorded: six corrected, one
  given a form, one settled on its authority, three recorded, two routed to
  M007-F.
- **M007-F — the daemon's LSP command surface.** Recorded; ADR required.
  The two `/lsp-*` forms belong to it.
- **M007-G — the daemon's plugin catalogue.** Recorded; ADR required.
- **M007-H — the daemon's missing command surfaces.** Recorded; no ADR
  needed.
- **M007-I — the three subsystem moves.** Recorded; ADR required.
- **The two owner-directed corrective milestones** — the scratch cache-root
  contract and the TUI `/compact` sender — are registered and ready, and
  are not this milestone's work.

## 12. Registry updates

The registry and the subsystem roadmap are the owner's to write. This record
describes what they should say:

- The subsystem roadmap's M007-D entry gains: **plan 017 is implemented**,
  with the six kind corrections applied, the presence panel given a form,
  `shell_kill`'s authority settled, the three granularity forms recorded as
  awaiting the owner's decision, and the desktop's new frontend-action
  surface named — including the three actions the desktop cannot carry and
  why.
- The registry's M007-D corrective-pass row moves from *ready for handoff*
  to **implemented**, with the closure record cited and the residual
  findings of §10 carried.
- The **catalog's own figures** change everywhere they are stated: 166 →
  **167** argument forms, 134 → **129** daemon-effect forms, 32 → **38**
  frontend-effect forms, 51 → **46** served forms, and the TUI's 39 → **40**
  executed forms. The closure 014's figures describe **M007-D's** tree and
  are left as they are — they are dated statements of what that milestone
  measured, not live counters.

## 13. Handoff notes

- **The six corrected forms' pinned evidence is the `MIGRATED` list's
  complement.** The migrated forms are pinned in
  `src/tui/command_route_tests.rs:43` (40 entries; the presence form at
  `:90`), and each is asserted in its family's test with the exact argument
  it must carry. The daemon-side half of each pin is in
  `src/core/daemon_commands.rs`'s test module.
- **The six corrected forms' action names are the contract between the
  daemon and any frontend**: `show_skills`, `show_status`,
  `show_tool_contracts`, `show_worktrees`, `show_shell_runs` and
  `show_terminal`, from `FrontendAction::as_str()`
  (`crates/codegg-core/src/frontend_action.rs:108-113`). They are generated
  from the enum, so a frontend that switches on them cannot drift from the
  daemon — and a frontend that meets an unknown name degrades to the
  honest notice rather than to a guess.
- **`serves_form` is presence-based**, and stays so: a row that returns
  `CommandEffect::UnreadableArgument` still counts as served, and the
  catalog publishes `daemon_served: true` for it.
- **The next batch's shape is settled**: the six kind corrections are
  applied and proved, `shell_kill`'s authority is settled and pinned, and
  the presence panel's form is in. What remains for this line is the
  **owner's** three granularity decisions, the **owner's** M007-F ADR, and
  the **owner's** decision on the three desktop actions the daemon does not
  yet publish the data for.
