# Desktop Frontend and IDE Foundation Milestone 014 — Closure Status

Status: **open — corrective pass required** (see §1 and §10)

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md`
  (milestone **M007-D**)

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADR (controlling):

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — accepted
  2026-10-24. It is this milestone's governing decision and the precondition
  below.

Repository baseline reviewed: `077655a49` — `origin/main` plus M007-A through
M007-C's closures and the M007-D work to that point.

Implementation commits on this milestone's branch:

- `1501e3b68` — the daemon command catalog, the executor, the two protocol
  variants, the two guards, the desktop host contract and the palette.
- `e51e6a7b4` — the effect table extended to 39 rows; `daemon_served`
  published; the classification artifact created.
- `410929a31` — the first TUI migration (seven commands), the serve-ability
  publication and M007-E's three integration points.
- `7b72ba4d2` — the settled items recorded; the description bound re-baselined
  from 200 to 256 bytes.
- `47b6171d7` — the Collaboration family's `/stop` and `/chat-action-list`.
- `c7cd10b18` — the identifier rows (`/stop-observing`, `/task-del`) and the
  verdict for the forms the daemon does not serve.
- `077655a49` — the kind-change count corrected to per form, and the LSP
  refusal recorded.
- `6f14cdba6` — the six queued mechanical forms, and serve-ability separated
  from one call's argument.

Owner-side commits on the same branch (path-scoped to `architecture/`):
`62ba2731b`, `03f018355`, `4c154d556`, `92335d20d`.

## 1. Executive finding

The daemon now owns the command catalog, and the milestone's **shape** is
delivered and proved:

- **161** commands, **166** argument forms, **134** daemon-effect, **32**
  frontend-effect, in one catalog: `crates/codegg-core/src/command_catalog.rs`.
- **51** daemon-effect forms are served by the daemon's effect table
  (`src/core/daemon_commands.rs`), and the daemon publishes that serve-ability
  per form, so any frontend can route statically.
- **39** TUI commands execute their effect through `CoreRequest::CommandExecute`
  and render the daemon's inner response verbatim.
- The two guards bite, with **9** injected defect classes rejected by
  `check_command_catalog_ownership.py --self-test`.
- The desktop palette renders the daemon's catalog and refuses an unserved
  form with a typed reason.

**The milestone is not closed.** Criterion 3 — *"a user running a daemon-effect
command in the TUI sees exactly the outcome they saw before the migration"* —
is proved for the **39** migrated commands and **not** for the **12** served
forms the TUI does not perform. The gap is a *migration* gap, not a design
gap: the daemon's effect table already has a row for each of the 51 served
forms, and each row's request shape is decided. What is missing for the
remaining twelve is not a rewiring — it is a **decision** per form, because
each one's TUI arm performs a *different* effect from the one its row serves.
Six are a kind correction (§10.6), three are a granularity decision the owner
owns (§10.6), one is an authority correction, and the two `/lsp-*` forms are
already decided (§10.2). The corrective pass that carries them is
`plans/implementation/desktop-frontend-ide-foundation/017-the-forms-the-daemon-serves-and-the-tui-does-not-perform.md`.

§10 records the outstanding work form by form, with the shapes of decision it
needed — including the two now settled (§10.3) and the one already taken
(§10.2: the two served `/lsp-*` forms stay unmigrated, and why).

## 2. Requirement-to-evidence matrix

Plan 014 §13's fourteen acceptance criteria, against the evidence in this
branch.

| # | criterion | status | evidence |
|---|---|---|---|
| 1 | the desktop shows the daemon's catalog, and the renderer holds no command names | **met** | `apps/desktop/src/commands/CommandPalette.tsx` renders the catalog the host serves; `scripts/check_command_catalog_ownership.py` fails the build on a command-name literal in the renderer, in any quoting style (9 defect classes) |
| 2 | a desktop daemon-effect command is authorized under `command.execute` and produces a `Run` | **met** | `tests/core_daemon/command_catalog_m007d.inc::command_execute_audits_a_daemon_effect_as_a_run`; the executor is fail-closed without a run store (`src/core/daemon_commands.rs`, `REFUSAL_RUN_STORE_UNAVAILABLE`) |
| 3 | a TUI daemon-effect command keeps its outcome, and produces a `Run` | **partial — 39 of 51** | `src/tui/command_route_tests.rs` — the per-family regression coverage, one pinned encoding test per migrated command; `src/core/daemon_commands.rs` — the daemon-side encoding pins; §10.1 lists the 19 forms migrated in this pass and §10.6 the 12 the TUI does not perform |
| 4 | a frontend-effect command has the same local effect and **no** `Run` | **met** | `tests/core_daemon/command_catalog_m007d.inc::command_execute_refuses_a_frontend_effect`; `crates/codegg-core/src/frontend_action.rs` declares the finite action set |
| 5 | free text that resembles a command is submitted as prompt text | **met** | the TUI recognises a command only on its registry; `src/tui/command_route.rs::route_for` returns `NotServed` for an unknown command or form |
| 6 | exactly one catalog; a guard fails on a second | **met** | `scripts/check_command_catalog_ownership.py` — clean, and `--self-test` rejects 9 injected defect classes |
| 7 | the frontend-action set is finite and declared | **met** | `crates/codegg-core/src/frontend_action.rs`; `scripts/check_frontend_action_set.py` — clean; the catalog's 32 frontend-effect forms are covered 32/32 |
| 8 | the catalog response is bounded, and an over-bound catalog is refused with a typed reason | **met** | `REFUSAL_CATALOG_OVER_BOUND` and `REFUSAL_RESULT_OVER_BOUND` in `src/core/daemon_commands.rs`; the description bound is 256 bytes (`crates/codegg-core/src/command_catalog.rs`) |
| 9 | an authorization refusal reports a typed reason and leaves no `Run` | **met** | `REFUSAL_CAPABILITY_REQUIRED`, and the executor's refusal path returns `run_id: String::new()` (`src/core/daemon_commands.rs::refusal`) |
| 10 | the TUI's command names, aliases and descriptions are unchanged | **met** | `git diff --name-only 5ff91228d..HEAD -- src/tui/command.rs src/tui/app/commands.rs` is empty; the 169 registry entries and the 161 `BuiltinSlashAction` variants are unchanged |
| 11 | `verify.sh quick` green, rustfmt clean, workspace clippy clean | **met** | §4 |
| 12 | the desktop's own verification green and separately recorded | **met** | §4 — `npx tsc --noEmit`, `npx vitest run`, `node scripts/check-bindings.mjs`, and the desktop host's own `cargo check` |
| 13 | the terminology document carries the cross-frontend `Command` definition, and it landed before the protocol variant | **met** | plan 016's terminology amendment landed in `1501e3b68`'s series, before the protocol variant was published |
| 14 | `check-desktop-boundary.sh` unchanged and green; no generic machine-authority bridge | **met** | `scripts/check-desktop-boundary.sh` is unchanged in this branch; the host exposes two named command routes only |

## 3. What was delivered

### 3.1 The daemon's catalog and its effect table

- `crates/codegg-core/src/command_catalog.rs` — **161** commands, **166**
  forms, with the description bound re-baselined to 256 bytes and the
  per-form `kind`.
- `crates/codegg-core/src/frontend_action.rs` — the declared frontend-action
  set, **32** forms.
- `src/core/daemon_commands.rs` — the effect table: **51** rows, one per
  served form, each row building the request that serves it from the scope
  and the argument.

**The invariant the table holds**: serve-ability is a property of the
**form**, not of one call's argument. `serves_form` asks whether a row
exists. A row whose argument the caller got wrong refuses *that call* with
`argument_unreadable` — its own outcome, never `command_not_served`, which
would misreport the daemon's capability. And every row *builds*: the table's
key set carries the argument each row reads, so the closure test proves a row
builds for the argument it declares rather than for whatever a global probe
happens to be.

### 3.2 The protocol

Two variants only: `CoreRequest::CommandCatalog` and
`CoreRequest::CommandExecute`, plus the `CommandExecuteResultDto` that
carries the daemon's inner response verbatim. `CommandFormDto.daemon_served`
publishes the daemon's own serve-ability per form.

### 3.3 The TUI migration

**39** commands execute through `CoreRequest::CommandExecute`:
`/memory`, `/memory-list`, `/memory-search`, `/memory-remember`,
`/memory-forget` (Memory); `/reload`, `/nodes`, `/init`, `/policy`,
`/approval`, `/network`, `/team` (System); `/goal`, `/task-del`,
`/terminal-attach`, `/terminal-resume`, `/terminal-list`, `/test`, `/tasks`,
`/task`, `/state` (Execution); `/schedules` (Agent); `/models-refresh`
(Provider); `/edit-undo`, `/edit-reapply`, `/edit-checkpoints`, `/share`,
`/unshare`, `/export`, `/reasoning`, `/control` (Session); `/stop`,
`/chat-action-list`, `/stop-observing`, `/chat-history`, `/chat-sync`,
`/chat-read`, `/observe`, `/collaborators` (Collaboration).

Each one's argument encoding is **pinned by a test** — the daemon-side test
names what the daemon builds from a given argument, and the TUI-side test
names what the frontend sends. That is the evidence criterion 3 rests on: a
frontend that encoded the argument differently would address the wrong turn,
channel, schedule or attachment, and the pinned encoding makes that a red
build rather than a silent mis-targeting.

The **19 forms migrated in this pass** and the **12 recorded findings** they
left behind are §10.1's table and §10.6's table, and the **two decision
shapes** the rewiring needed are §10.3's settled rulings.

### 3.4 The classification artifact

`plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`
is **generated**, by the committed
`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`.
Its `--check` mode re-derives the artifact and reports drift instead of
writing over it, and the check mode is proved non-vacuous (absent → 1, stale
→ 1, current → 0).

The accounting is complete: **134** daemon-effect forms = **51** served +
**28** mechanical-unserved + **55** with no daemon request, every form named.

## 4. Verification executed

Commands run against this branch's head, with their results.

| command | result |
|---|---|
| `scripts/verify.sh quick` | **Quick verification passed** |
| `cargo fmt --all` | clean (rustfmt is part of `quick`) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean, no warnings |
| `cargo test --locked -p codegg --lib command_route` | **29 passed**, 0 failed |
| `cargo test --locked -p codegg --lib daemon_commands` | **12 passed**, 0 failed |
| `cargo test --locked --test core_daemon -- command_catalog_m007d` | **11 passed**, 0 failed |
| `cargo test --locked --test tui_render` | **1094 passed**, 0 failed |
| `cargo test --locked -p codegg-core -p codegg-protocol` | passed |
| `python3 scripts/check_command_catalog_ownership.py --self-test` | `self-test passed: 9 injected defect classes were all rejected` |
| `python3 scripts/check_command_catalog_ownership.py` | `command catalog ownership: clean` |
| `python3 scripts/check_frontend_action_set.py` | `frontend-action set: clean` |
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` (after `--bank`) |
| `python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check` | `check passed: … matches the tree` |
| the owner's read-only migration instrument, `/tmp/m007d_verify_migration.py` | 51 effect-table rows, **39** migrated commands, **40** routed call sites; the **12** served forms the TUI does not perform are exactly the ones §10.1 and §10.6 record, and the paths that still construct a migrated form's request are the ones §10.4 and §10.7 record |
| `cd apps/desktop && npx tsc --noEmit` | clean |
| `cd apps/desktop && node scripts/check-bindings.mjs` | `desktop bridge DTO fields match` |
| `cd apps/desktop && npx vitest run` | **29** files, **218** tests, all passed |
| `cd apps/desktop/src-tauri && cargo check --locked` (after `bash apps/desktop/scripts/stage-daemon.sh`) | clean |

**Not run in this branch's final state**, and required before closure:

- The hosted lanes (`verify`, `e2e`, `restricted-userns-fails-closed`,
  `process-ipc`). The lanes reported green at `077655a49`; they do not
  include this branch's later commits, so the final head needs its own run.

## 5. Invariant review

Plan 014 §4's invariants that must not regress, and §14's stop conditions.

| invariant | held? | evidence |
|---|---|---|
| The mechanism is unchanged: `command.execute` carries a scope, a command id, a form and an argument | **yes** | `crates/codegg-protocol/src/core.rs` — two variants only; no third |
| The two namespaces are never joined | **yes** | the declared frontend-action set is keyed by `(command_id, form)`; the desktop keymap holds gestures; `action_for_form` is the only resolver |
| Serve-ability is published, and an unserved form is refused with a typed reason | **yes** | `REFUSAL_COMMAND_NOT_SERVED`; `CommandPalette.tsx:177` refuses an unserved form |
| The effect table's shape is unchanged | **yes** | the table remains the match arms of `build_inner_request`; the key set carries the probe argument for the closure test only |
| The 256-byte description bound | **yes** | `crates/codegg-core/src/command_catalog.rs` — the bound, re-baselined with the evidence plan 014's protocol table records |
| The TUI's command names, aliases and descriptions | **yes** | no change to `src/tui/command.rs` or `src/tui/app/commands.rs` |
| The TUI's observable behaviour | **yes for the 39 migrated commands** | the daemon's inner response travels verbatim; the pinned encoding tests are the evidence. For the 12 served forms the TUI does not perform, and for the 55 no-request forms, the TUI still runs its own arm, so its behaviour is unchanged there too |
| No new authorization capability | **yes** | `command.execute` is the only capability the catalog names |
| No second catalog, no generic host passthrough | **yes** | `scripts/check_command_catalog_ownership.py`; `scripts/check-desktop-boundary.sh` unchanged |

**Stop conditions that were honoured**: ADR-0052 was accepted before any of
this work; the contested classifications were **recorded** rather than
resolved (the 28 kind changes and the 24 new actions are a follow-up, not
this milestone); no plugin or MCP command needed inclusion; no capability
other than `command.execute` was needed; no third `CoreRequest` variant was
introduced; `/compact` keeps its current behaviour and its missing daemon
route is a recorded discrepancy.

**One stop condition is not yet discharged**: *"the TUI's observable behaviour
would change"*. Migrating a served form is where that risk lives — the two
shapes of it are now **settled and pinned** (§10.3), and the twelve forms
whose arms perform a different effect are **recorded** rather than rewired
(§10.6). The migration of the 39 commands is done; the remaining twelve are
the work this milestone still owes, and each one needs an owner-visible
decision rather than a rewiring.

## 6. Failure, cancellation, restart, and contention semantics

- A daemon-effect execution without a run store is **refused**
  (`REFUSAL_RUN_STORE_UNAVAILABLE`), not executed unaudited.
- An authorization refusal leaves **no** `Run`: the refusal's `run_id` is
  empty (`src/core/daemon_commands.rs::refusal`).
- A refusal for an unreadable argument leaves **no** `Run`, and is reported
  as `argument_unreadable`.
- The inner response travels verbatim in the command's result, so a
  cancellation, a resync and an out-of-band refusal each reach the frontend
  as the daemon's own response.

## 7. Migration and compatibility review

- The TUI's own arm is kept for every command that is not migrated: the **12**
  served forms the TUI does not perform (§10.6) and the 55 no-request forms
  all keep their local execution, so the TUI's behaviour is unchanged for all
  of them.
- The desktop refuses an unserved form rather than dispatching it, so a
  desktop user sees a typed refusal where the TUI still works locally. That
  asymmetry is recorded in the artifact's §8.
- The two served `/lsp-*` forms are the one place where the daemon serves a
  form the TUI does not use: the desktop dispatches them through the daemon,
  and the TUI's own arms remain a recorded discrepancy (§10.2). §10.6's ten
  forms are the same shape, and the table names each one's reason.

## 8. Security review

- The inner request is re-authorized by the daemon on **its own operation**,
  not on the frontend's claim: `command.execute` is the capability to *run* a
  command, and the daemon authorizes the effect it builds.
- The scope carries the project, the workspace and the session; the daemon
  resolves what it can from its own durable binding and refuses a session it
  was not given (`REFUSAL_NO_SESSION`).
- The argument is bounded (`REFUSAL_ARGUMENT_TOO_LONG`).
- The renderer holds no command names, no durable state and no credentials;
  the host exposes two named routes.

## 9. Documentation and operations

- The eight ADR-0052-mandated architecture documents were written by the
  owner: `architecture/command.md`, `command_routing.md`, `protocol.md`,
  `tui.md`, `desktop.md`, `overview.md`, `authorization.md`, `lsp.md`.
- The terminology amendment is plan 016's, landed before the protocol
  variant.
- The classification artifact is generated and committed, with a `--check`
  mode so a reader can verify it without a build.

## 10. Unresolved findings

### 10.1 The 31 served-but-unmigrated forms: **19 migrated, 12 recorded**

The daemon's effect table has a row for each of these. **19** of them now
execute through `CoreRequest::CommandExecute`; the remaining **12** are
recorded findings, each with its reason named below and in §10.6.

**19 forms migrated.** The first line is the encoding the argument carries;
a scope-only row carries none.

| command | argument encoding | note |
|---|---|---|
| `project_init` | — | the scope names the project and the workspace the preview is taken in |
| `policy` | — | **decision shape one**, §10.3 |
| `approval` | — | a unit read; the scope's session cannot change what it answers |
| `reasoning` | — | a unit read, for the same reason |
| `network` | `<intent> [<revision>]` | the revision is the compare-and-swap the policy read published |
| `models_refresh` | — | a unit read |
| `share` | `<session-id>` | **decision shape two**, §10.3 |
| `unshare` | `<session-id>` | **decision shape two**, §10.3 |
| `export` | `<session-id>` | **decision shape two**, §10.3 |
| `observe` | `<session-id>` | the session the user named, not the attached one |
| `tasks` | `[<limit>]` | the listing bound belongs to the caller |
| `task` | `[<limit>]` | the same bound, under the id the user typed |
| `schedules` | — | the scope names the workspace the schedules are read in |
| `state` | — | the scope names the session the goal is read in |
| `collaborators` | — | the principal listing; see §10.6 for the `/collaborators` arm |
| `team` | — | the scope names the project the membership is read in |
| `control` | — | the scope names the session the control surface is read in |
| `edit_checkpoints` | — | the scope names the workspace and the session |
| `terminal_list` | — | the scope names the workspace the terminals are read in |

`tasks`/`task` are a **shared arm**: one row serves two forms, and each is
executed under the id the user typed — the same rule `/memory` and
`/memory-list` already follow.

**12 forms recorded, not migrated.** Each is a form the daemon *serves* and
the TUI's own arm does *not* perform, so migrating the arm would change what
the user sees:

```
lsp_status  lsp_errors  skills  status  tool_contracts  worktree
shell_list  shell_show  shell_expand  shell_kill  terminal_show  terminal_send
```

The reasons are in §10.2 (`/lsp-*`) and §10.6 (the other ten).

### 10.2 The two served `/lsp-*` forms: **decided — not migrated in this milestone**

`/lsp-status` and `/lsp-errors` are in the **served** set: their row builds
`lsp_diagnostics_get`, and the daemon's `egglsp` instance is authoritative.
The TUI's own arms for those two are **not** migrated, and the decision is
recorded here rather than taken silently.

The arm's behaviour differs by mode, and that is the reason:

- In the **socket** (daemon-connected) configuration the arm refuses with the
  toast `LSP not available`, because `app.lsp_tool` is `None` there —
  `src/main.rs:4014` attaches the TUI's own `LspTool` only when the TUI is
  **not** in socket mode.
- In **local** mode the same arm works, against that second, non-authoritative
  language server.

Migrating the arm would therefore change behaviour in **both** modes: in
socket mode a refusal becomes real diagnostics, and in local mode the arm
would need a daemon the local configuration does not have. Plan 014 §14 makes
the first shape explicit — *"a command that did **not** work before is a
recorded discrepancy requiring an owner-visible decision, not a licence to
change behaviour silently"* — and `/lsp-status` in the socket configuration
is exactly the `/compact` shape. The second shape is a regression the
milestone's contract forbids outright.

So the two forms stay unmigrated, and the consequence is named rather than
left implicit: **the daemon serves two forms the TUI does not use.** The
desktop palette dispatches both through the daemon, because they are
published as served — so the daemon's capability is used, by the desktop —
and the TUI's own arms remain a recorded discrepancy for the owner to
decide.

This supersedes two statements that were published on the strength of a
narrower reading of the tree, and both are corrected in the artifact:

- `architecture/lsp.md` (the owner's) says the TUI's `/lsp-*` arms refuse.
  That is true of the **socket** configuration and false of local mode, where
  the TUI attaches its own `LspTool`. The owner is re-pinning the document.
- The artifact's M007-F row said the TUI "never builds a second `LspTool` in
  production". That was wrong — the earlier reading missed `src/main.rs`. The
  row now names the mode split.

### 10.3 The two shapes of decision: **both settled and pinned**

"Scope-only" is not automatically behaviour-preserving, and two shapes of it
were visible in the rows. Both are now **decided**, each with a pinned test on
both sides.

#### Shape one — a row that carried the session where the TUI's request carried none

The `policy` row built `execution_policy_get` with the scope's session, while
every TUI `ExecutionPolicyGet` site passes `session_id: None`
(`src/tui/commands/policy.rs`). Migrating the row as it stood would have
changed *which* policy the user is shown.

**The decision: the `policy` row names no session.** The effective runtime
policy is a *global* reading, and the TUI asks for it with no session; a
session-scoped read answers a different question. So the row now builds
`execution_policy_get { session_id: null }` and the scope's session cannot
change the answer. The scope still names the project and the workspace the
reading is authorised against.

`approval` and `reasoning` turned out **not** to have this problem, and the
reason is structural rather than incidental: `approval_preference_get` and
`reasoning_preference_get` are **unit variants** — their requests carry no
session field at all, so the scope's session cannot change what they answer.
The daemon-side pin asserts that, so a future change that gave one of them a
session field would fail rather than pass unnoticed.

Pinned by `the_policy_rows_name_no_session`
(`src/core/daemon_commands.rs`) on the daemon side and
`the_policy_reads_are_executed_by_the_daemon`
(`src/tui/command_route_tests.rs`) on the TUI side.

#### Shape two — a row that acted on a session the caller chose

The `share`, `unshare` and `export` rows acted on the scope's session, while
the TUI's arms act on a session the user selected in a dialog
(`src/tui/commands/sessions.rs`). Migrating the rows as they stood would have
shared the *wrong* session.

**The decision: the session id travels as the argument.** The rows now read it
from the argument's first token and build `session_share` / `session_unshare`
/ `session_export` from it; a call that names no session is refused with
`no_session` — the daemon serves the form, it just cannot guess which session
the caller meant. This is the same place the turn id travels for `stop`, and
it is the only shape that is correct for a surface that can name a session
other than the attached one. **No new row was needed**: the three existing
rows carry the session now.

`observe` has the same shape and the same decision: `/observe <session-id>`
names a session that need not be the attached one, so its row reads the
session from the argument too.

Pinned by `the_session_named_rows_carry_their_session_in_the_argument` and
`the_observe_command_carries_the_session_in_the_argument` on the daemon side,
and by `the_session_named_commands_carry_their_session_in_the_argument` and
`the_observe_command_carries_the_session_in_the_argument` on the TUI side.

### 10.4 Four daemon-owned requests still bypass the catalog

(The 19 forms this pass migrated added three more paths of the same kind;
they are §10.7's table.)

These are **not** gaps in the 39 migrated commands' arms: every migrated arm
routes through the daemon and no longer constructs its own request. These are
*other* paths in the TUI that reach a daemon-owned request directly, outside
the catalog, and therefore open **no `Run`**. Criterion 3's letter does not
reach them — they are not commands — but ADR-0052's ownership boundary does.

| request | sites | path | disposition |
|---|---|---|---|
| `ProjectionUnsubscribe` | `src/tui/commands/primary_projection.rs:110`, `:172`, `:281`, `:327` | the projection subscription's own teardown and replacement | **lifecycle, recorded.** The subscription is opened and dropped by the projection client, not by a user command; the `/stop-observing` *command* does route through the daemon. A `Run` per teardown would be noise, so this is recorded as lifecycle rather than migrated |
| `AssetRefresh` | `src/tui/commands/agents.rs:96`, `src/tui/commands/project_init.rs:116` | the asset refresh inside the `/agents` and `/init` flows — a *different* command's side effect | **recorded for the owner.** `/reload` is the catalog's asset-refresh command and its arm routes through the daemon, so the daemon's capability is audited when the user asks for it; here it happens as a side effect of another command, unaudited. Making it the `reload` command's effect is a decision about what `/agents` and `/init` *are*, and it is not this milestone's to take |
| `ChatHistoryOlder` | `src/tui/commands/chat.rs:1551` | the "load older" **key binding** (`src/tui/app/input.rs:135`), not the `/chat-history` command | **recorded for the owner.** This is the sharpest of the four: `chat_history_older` is **not a catalog command id at all**, so the catalog is not merely bypassed here — it is absent. The effect needs an *anchored* cursor, and the `chat_history` row builds the *unanchored* newest window. The fix is a second form of `chat_history` whose argument carries the anchor, which is a product decision about the catalog's shape |
| `InteractiveProcessResume` | `src/tui/commands/interactive_terminal.rs:957` | the terminal output pump (`service_terminal_output_pull`), not the `/terminal-resume` command | **lifecycle, recorded.** The pump reads the daemon's retained output on a cadence; the `/terminal-resume` *command* does route through the daemon |

None of the four is a defect in what was migrated. All four are recorded here
so that the closure does not read as a claim that every daemon-owned effect
in the TUI is audited.

### 10.5 The other recorded findings

| finding | severity | disposition |
|---|---|---|
| The 28 catalog kind changes and the 24 new `FrontendAction` declarations are **not applied** — a frontend-effect form needs a declared action, and one declaration may cover several forms of a command | medium | recorded as a follow-up; the corrected kind is named per form in the artifact's §10 |
| 36 daemon-effect forms have **no daemon surface at all** — 24 `/lsp-*` (M007-F, ADR required), and compaction, subagent steer, the plan list, the test suite, a git revert, the research turn, the security-review and change-review workflows, opening a document, re-running a human-shell run, and a log tail (M007-H) | medium | recorded as M007-F and M007-H; neither is built inside this milestone |
| The three **subsystem moves** — the git-review diff view, the skill-promotion workflow and the human-shell ask workflow — are workflows of dozens of daemon requests each | medium | recorded as M007-I, with an ADR required: each is a new ownership boundary ADR-0052 does not cover |
| `/compact` keeps its current behaviour; the daemon has no compaction route | low | a recorded discrepancy requiring an owner-visible decision (plan 014 §14) |
| The 19 forms the verdict calls misclassified are still classified as daemon effects in the catalog | low | folded into the 28 kind changes above |
| The 5 argument-consuming forms need their encoding decided and pinned | medium | **settled** — `network`, `share`, `unshare`, `export` and `observe` are migrated with pinned encodings; `shell_show`, `shell_expand`, `shell_kill` and `terminal_send` are recorded in §10.6 |

### 10.6 Ten served forms whose TUI arm performs a different effect

These ten forms are **served** by the daemon and **not** migrated, and the
reason is the same in every case: the daemon's row serves a request that is
*not* the one the TUI's own arm performs. Migrating the arm would therefore
change what the user sees, which plan 014 §14 forbids.

| form | the daemon's row serves | the TUI's arm performs | disposition |
|---|---|---|---|
| `skills` | `skill_catalogue_list` — the daemon's resolved skill catalogue | a read of the TUI's own asset snapshot (`app.agent_state.snapshot`) | **recorded.** Two different sources; the daemon's is authoritative and the TUI's is not, so the arm would start showing a different list |
| `status` | `session_lifecycle_get` | a locally assembled status string (session status, token counters, the policy status line) | **recorded.** The daemon's request carries no part of what the arm shows |
| `tool_contracts` | `tool_program_list` — the daemon's *tool programs* | a read of the TUI's **local** tool registry (`ToolBroker::new(&ToolRegistry::with_defaults())`) | **recorded.** Different things: tool contracts are not tool programs |
| `worktree` | `managed_worktree_list` | `worktree_list` by project directory — a different request | **recorded.** The daemon's managed-worktree listing and the TUI's checkout listing are different surfaces |
| `shell_list` | `human_shell_run_capabilities` | a read of the TUI's **local** shell store | **recorded.** The daemon's request is the human-shell capability probe the `!`-shell path issues, not the `/shell-list` listing |
| `shell_show` | `human_shell_run_read` | a read of the TUI's local shell store | **recorded, and structurally so.** The daemon's request is issued by the `!`-shell path's **polling loop**, one read per poll; routing it through `command.execute` would open a canonical `Run` per poll |
| `shell_expand` | `human_shell_run_read` | the same local store, expanded | **recorded**, for the same reason |
| `shell_kill` | `human_shell_run_cancel` | a local kill for a local run, and a cancel for a remote one | **recorded.** The cancel's authority is the **run's own** project, workspace and session — the ones the binding carries — not the caller's scope, so a scope-built row could address the wrong run |
| `terminal_show` | `interactive_process_list` | a purely frontend-local action: switch the view to the named terminal | **recorded.** The arm performs no daemon request at all |
| `terminal_send` | `interactive_process_input` | the interactive terminal's **input path**, shared by the `/terminal-send` arm and the live typing path | **recorded, and structurally so.** The path issues one request per keystroke, so routing it through `command.execute` would open a canonical `Run` per keystroke |

None of the ten is a defect in what was migrated. All ten are recorded here so
that the closure does not read as a claim that every served form is used by
the TUI: **the daemon serves 51 forms and the TUI now executes 39 of them.**

**One more recorded discrepancy, specific to `/collaborators`.** The
`collaborators` row serves `team_principal_list`, and that request is now
executed through the daemon — but the TUI reaches it through
`/team principals`, not through the `/collaborators` arm. The
`/collaborators` arm shows the **presence** panel, whose request is
`presence_snapshot_get` — a request the catalog does not name. So the
`collaborators` form is migrated, and the presence panel is a separate,
unnamed effect recorded here.

### 10.7 The bypasses the 19 migrations added

The 19 migrations removed the TUI's own construction of the requests they
serve. The paths that still reach a daemon-owned request directly are
recorded, and none of them is a command arm:

| request | sites | disposition |
|---|---|---|
| `ApprovalPreferenceGet` | `src/tui/commands/work_orders.rs:100`, `:378` | **recorded.** These are the Task composer's **model prefetch**, not the `/approval` arm. The `/approval` arm's own read routes through the daemon; routing a prefetch as well would open a `Run` per prefetch for a read that is a side effect of another surface |
| `ProjectionSubscribe` | `src/tui/commands/observe.rs:446`, `src/tui/commands/primary_projection.rs:202` | **lifecycle, recorded.** The first is the *resume* path's fresh-subscribe fallback and the second is the primary projection's own subscription — neither is the `/observe` arm, which routes through the daemon |
| `RuntimePolicySet` | `src/tui/commands/policy.rs:538` | **recorded for the owner.** The catalog's `policy` form is the *read*; the **write** has no catalog form at all, so the confirmed change still issues its own request. Making the write a catalog command is a decision about the catalog's shape |

**Any medium finding keeps M007-D open.** Six are listed above, so the
milestone's recommendation is **corrective pass required**, not closed.

## 11. Roadmap disposition

- **M007-D — the daemon-owned command catalog and the cross-frontend
  execution path.** The mechanism, the two namespaces, the serve-ability
  publication, the guards and the desktop consumption are done. The **TUI
  migration is 39 of the 51 served forms**; the **12 forms the TUI does not
  perform** are §10.1's and §10.6's recorded findings.
- **M007-E — the editor's own commands.** The three integration points were
  closed at `410929a31`.
- **M007-F — the daemon's LSP command surface.** Recorded; ADR required.
- **M007-G — the daemon's plugin catalogue.** Recorded; ADR required.
- **M007-H — the daemon's missing command surfaces.** Recorded; no ADR needed,
  because each extends a request family that already exists.
- **M007-I — the three subsystem moves.** Recorded; ADR required.

## 12. Registry updates

The registry, the subsystem roadmap and the registration of the two
owner-directed corrective milestones are the owner's to write, after this
milestone merges. This record describes what they should say:

- The subsystem roadmap's M007-D entry moves from *in progress* to
  *conditionally closed*, with the **12 forms the TUI does not perform** named
  and the six medium findings carried.
- The registry gains a corrective-pass entry for M007-D naming the 12 recorded
  forms and the two owner-directed corrective milestones (the scratch
  cache-root contract and the TUI `/compact` sender).
- The corrective pass is **plan 017**,
  `plans/implementation/desktop-frontend-ide-foundation/017-the-forms-the-daemon-serves-and-the-tui-does-not-perform.md`,
  and it is registered under `## Dependency-ready implementation plans`. It
  is *ready for handoff*, and its plan header says so: the three
  granularity forms (`shell_show`, `shell_expand`, `terminal_send`) are the
  owner's decision and are recorded as awaiting it, and the plan's own stop
  condition says the milestone lands the rest without them.

## 13. Handoff notes

- **The pinned encoding test is the evidence for criterion 3**, and it is
  findable: every migrated command appears in the `MIGRATED` constant in
  `src/tui/command_route_tests.rs`, and each is asserted in its family's test
  with the exact argument it must carry. The daemon-side half of each pin is
  in `src/core/daemon_commands.rs`'s test module — `the_identifier_rows_carry_their_identifier_in_the_argument`,
  `the_collaboration_rows_carry_their_identifiers_in_the_argument`,
  `the_cursor_rows_carry_their_identifier_and_cursor_in_the_argument`,
  `the_policy_rows_name_no_session`,
  `the_session_named_rows_carry_their_session_in_the_argument`,
  `the_network_row_carries_the_intent_and_the_revision` and
  `the_task_rows_carry_their_bound_in_the_argument`.
- **`serves_form` is presence-based now.** A row that returns
  `CommandEffect::UnreadableArgument` still counts as served, and the
  catalog publishes `daemon_served: true` for it. A caller that got the
  argument wrong is told the argument was wrong.
- **The next batch's shape is settled**: §10.3's two decision shapes are
  decided and pinned, and §10.6's ten forms are recorded with their reasons.
  The rule that produced both is the milestone's own: a guessed encoding is
  worse than a recorded one.
