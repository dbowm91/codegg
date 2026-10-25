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
- **20** TUI commands execute their effect through `CoreRequest::CommandExecute`
  and render the daemon's inner response verbatim.
- The two guards bite, with **9** injected defect classes rejected by
  `check_command_catalog_ownership.py --self-test`.
- The desktop palette renders the daemon's catalog and refuses an unserved
  form with a typed reason.

**The milestone is not closed.** Criterion 3 — *"a user running a daemon-effect
command in the TUI sees exactly the outcome they saw before the migration"* —
is proved for the **20** migrated commands and **not** for the remaining
**31** served-but-unmigrated forms. The gap is a *migration* gap, not a design
gap: the daemon's effect table already has a row for each of the 31, and each
row's request shape is decided. What is missing is the TUI-side rewiring, and
for five of them the argument's encoding.

§10 records the outstanding work form by form, with the two shapes of
decision it needs.

## 2. Requirement-to-evidence matrix

Plan 014 §13's fourteen acceptance criteria, against the evidence in this
branch.

| # | criterion | status | evidence |
|---|---|---|---|
| 1 | the desktop shows the daemon's catalog, and the renderer holds no command names | **met** | `apps/desktop/src/commands/CommandPalette.tsx` renders the catalog the host serves; `scripts/check_command_catalog_ownership.py` fails the build on a command-name literal in the renderer, in any quoting style (9 defect classes) |
| 2 | a desktop daemon-effect command is authorized under `command.execute` and produces a `Run` | **met** | `tests/core_daemon/command_catalog_m007d.inc::command_execute_audits_a_daemon_effect_as_a_run`; the executor is fail-closed without a run store (`src/core/daemon_commands.rs`, `REFUSAL_RUN_STORE_UNAVAILABLE`) |
| 3 | a TUI daemon-effect command keeps its outcome, and produces a `Run` | **partial — 20 of 51** | `src/tui/command_route_tests.rs` — the per-family regression coverage, one pinned encoding test per migrated command; `src/core/daemon_commands.rs` — the daemon-side encoding pins; §10 lists the 31 outstanding forms |
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

**20** commands execute through `CoreRequest::CommandExecute`:
`/memory`, `/memory-list`, `/memory-search`, `/memory-remember`,
`/memory-forget` (Memory); `/reload`, `/nodes` (System); `/goal`,
`/task-del` (System/Execution); `/edit-undo`, `/edit-reapply` (Session);
`/stop`, `/chat-action-list`, `/stop-observing`, `/chat-history`,
`/chat-sync`, `/chat-read` (Collaboration); `/terminal-attach`,
`/terminal-resume`, `/test` (Execution).

Each one's argument encoding is **pinned by a test** — the daemon-side test
names what the daemon builds from a given argument, and the TUI-side test
names what the frontend sends. That is the evidence criterion 3 rests on: a
frontend that encoded the argument differently would address the wrong turn,
channel, schedule or attachment, and the pinned encoding makes that a red
build rather than a silent mis-targeting.

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
| `cargo test --locked -p codegg --lib command_route` | **19 passed**, 0 failed |
| `cargo test --locked -p codegg --lib daemon_commands` | **8 passed**, 0 failed |
| `cargo test --locked --test core_daemon -- command_catalog_m007d` | **11 passed**, 0 failed |
| `cargo test --locked --test tui_render` | **1094 passed**, 0 failed |
| `cargo test --locked -p codegg-core -p codegg-protocol` | passed |
| `python3 scripts/check_command_catalog_ownership.py --self-test` | `self-test passed: 9 injected defect classes were all rejected` |
| `python3 scripts/check_command_catalog_ownership.py` | `command catalog ownership: clean` |
| `python3 scripts/check_frontend_action_set.py` | `frontend-action set: clean` |
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` (after `--bank`) |
| `python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check` | `check passed: … matches the tree` |
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
| The TUI's observable behaviour | **yes for the 20 migrated commands** | the daemon's inner response travels verbatim; the pinned encoding tests are the evidence. For the 31 outstanding forms the TUI still runs its own arm, so its behaviour is unchanged there too |
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
would change"*. Migrating the 31 outstanding forms is where that risk lives —
two shapes of it are already visible (§10.2). The migration of the 20
commands is done; the remaining 31 are the work this milestone still owes.

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

- The TUI's own arm is kept for every command that is not migrated: **31**
  served-but-unmigrated forms and the 55 no-request forms all keep their
  local execution, so the TUI's behaviour is unchanged for all of them.
- The desktop refuses an unserved form rather than dispatching it, so a
  desktop user sees a typed refusal where the TUI still works locally. That
  asymmetry is recorded in the artifact's §8.

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

### 10.1 The 31 served-but-unmigrated forms

The daemon's effect table has a row for each of these; the TUI's arm still
issues the request itself. Migrating them is the outstanding work.

**26 forms whose row reads only the scope** — the rewiring carries no
encoding decision, so the argument is empty and the scope supplies
everything:

```
project_init  policy  approval  reasoning  skills  models_refresh  worktree
tool_contracts  share  unshare  export  status  tasks  task  schedules  state
collaborators  team  control  observe  edit_checkpoints  shell_list
terminal_list  terminal_show  lsp_status  lsp_errors
```

**5 forms whose row consumes the argument** — each needs its encoding decided
and pinned:

```
network  shell_show  shell_expand  shell_kill  terminal_send
```

`tasks`/`task`, `shell_show`/`shell_expand` and `terminal_list`/`terminal_show`
are **shared arms**: one row serves two forms.

### 10.2 Two shapes of decision the scope-only group needs

"Scope-only" is not automatically behaviour-preserving, and two shapes of it
are visible in the rows. Both are *recorded*, not guessed at:

- **A row that carries the session, where the TUI's request carries none.**
  The `policy` and `approval` rows build `execution_policy_get` with the
  scope's session, while the TUI's own sites pass `session_id: None`
  (`src/tui/commands/policy.rs`). Migrating them as the rows stand would
  change *which* policy the user is shown. The row must either pass no
  session, or the scope must carry the session the TUI meant — a decision,
  not a rewiring.
- **A row that acts on a session the caller chose, not the attached one.**
  The `share`, `unshare` and `export` rows act on the scope's session, while
  the TUI's arms act on a session the user selected in a dialog
  (`src/tui/commands/sessions.rs`). Migrating them as the rows stand would
  share the *wrong* session. The session id must travel in the argument.

### 10.3 The other recorded findings

| finding | severity | disposition |
|---|---|---|
| The 28 catalog kind changes and the 24 new `FrontendAction` declarations are **not applied** — a frontend-effect form needs a declared action, and one declaration may cover several forms of a command | medium | recorded as a follow-up; the corrected kind is named per form in the artifact's §10 |
| 36 daemon-effect forms have **no daemon surface at all** — 24 `/lsp-*` (M007-F, ADR required), and compaction, subagent steer, the plan list, the test suite, a git revert, the research turn, the security-review and change-review workflows, opening a document, re-running a human-shell run, and a log tail (M007-H) | medium | recorded as M007-F and M007-H; neither is built inside this milestone |
| The three **subsystem moves** — the git-review diff view, the skill-promotion workflow and the human-shell ask workflow — are workflows of dozens of daemon requests each | medium | recorded as M007-I, with an ADR required: each is a new ownership boundary ADR-0052 does not cover |
| `/compact` keeps its current behaviour; the daemon has no compaction route | low | a recorded discrepancy requiring an owner-visible decision (plan 014 §14) |
| The 19 forms the verdict calls misclassified are still classified as daemon effects in the catalog | low | folded into the 28 kind changes above |
| The 9 argument-consuming forms need their encoding decided and pinned | medium | §10.1 — the outstanding migration work |

**Any medium finding keeps M007-D open.** Six are listed above, so the
milestone's recommendation is **corrective pass required**, not closed.

## 11. Roadmap disposition

- **M007-D — the daemon-owned command catalog and the cross-frontend
  execution path.** The mechanism, the two namespaces, the serve-ability
  publication, the guards and the desktop consumption are done. The **TUI
  migration is 20 of 51**; the remainder is §10.1's 31 forms.
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
  *conditionally closed*, with the 31 outstanding migration forms named and
  the six medium findings carried.
- The registry gains a corrective-pass entry for M007-D naming the 31 forms
  and the two owner-directed corrective milestones (the scratch cache-root
  contract and the TUI `/compact` sender).

## 13. Handoff notes

- **The pinned encoding test is the evidence for criterion 3**, and it is
  findable: every migrated command appears in the `MIGRATED` constant in
  `src/tui/command_route_tests.rs`, and each is asserted in its family's test
  with the exact argument it must carry. The daemon-side half of each pin is
  in `src/core/daemon_commands.rs`'s test module — `the_identifier_rows_carry_their_identifier_in_the_argument`,
  `the_collaboration_rows_carry_their_identifiers_in_the_argument` and
  `the_cursor_rows_carry_their_identifier_and_cursor_in_the_argument`.
- **`serves_form` is presence-based now.** A row that returns
  `CommandEffect::UnreadableArgument` still counts as served, and the
  catalog publishes `daemon_served: true` for it. A caller that got the
  argument wrong is told the argument was wrong.
- **The next batch's shape is settled**: 26 rows read only the scope, 5 read
  the argument, and §10.2's two shapes need a decision before those rows are
  rewired. The decision is the owner's or the implementer's to make; the
  milestone's own rule is that a guessed encoding is worse than a recorded
  one.
