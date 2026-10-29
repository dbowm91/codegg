# Desktop Frontend and IDE Foundation Milestone 017 — The Forms the Daemon Serves and the TUI Does Not Perform

Status: ready for handoff

Repository baseline: `32a91c4c4` — `origin/main` (`682ae762a`) merged with the
M007-D branch: the daemon-owned command catalog, the cross-frontend execution
path, and the TUI migration of **39** of the daemon's **51** served forms.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Planning basis:

- M007-D's closure record, `plans/closure/desktop-frontend-ide-foundation/014-status.md`
  §10.6 — "Ten served forms whose TUI arm performs a different effect" — and
  §10.1's list of the **12** served forms the TUI does not perform.
- The classification artifact,
  `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`
  §3 and §6 — the served set (51 rows) and the per-form verdicts.
- M007-D's closure §10.3 — the two decision shapes it settled, which this
  milestone inherits rather than reopens:
  `policy` names **no** session; `share`, `unshare`, `export` and `observe`
  carry the session in the **argument**.

Applicable ADR:

- **ADR-0052** — `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`,
  accepted 2026-10-24. The daemon owns the command catalog and the execution
  path; a frontend may not own one.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.4 — a frontend renders projections
  and does not own durable state. A catalog entry that names a **local** effect
  as a daemon effect is a frontend owning a state it does not have, and the
  correction is a correction of the classification.
- `plans/002-long-term-roadmap.md:899` — "When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap." **This milestone
  changes no ownership boundary**: it corrects the catalog's classification of
  effects that already exist, and it records the two shapes that would.
- `plans/001-terminology-and-domain-model.md` §16 — the canonical set of terms
  the desktop's input model introduced (`Keymap`, `Chord`, `Action`,
  `Binding scope`). A catalog form that is really a frontend action is an
  `Action`, not a `Command`.

## 1. Objective

Make the daemon's catalog **honest about the twelve forms the TUI does not
perform**, and settle the two shapes that need an owner decision.

M007-D migrated **39** of the daemon's **51** served forms. The other **12**
are served by the daemon and are *not* executed by the TUI, because the TUI's
own arm performs a **different effect** — a local read, a local dialog, a
per-keystroke write, or a refusal that depends on the run mode. Migrating
those arms as the rows stand would change what the user sees, which
`plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md`
§14 forbids, and it would open a canonical `Run` per keystroke or per poll
for the two paths that fire continuously.

So the catalog currently **publishes a serve-ability that the TUI does not
exercise**, and the two frontends disagree about those forms: the desktop
palette dispatches them through the daemon, the TUI does something else
entirely. That disagreement is the defect this milestone closes.

## 2. What this milestone must not do

- **It must not change what a user sees.** Each correction is a change to the
  *classification* or to the *row*, never to the effect the user observes.
- **It must not add a new `CoreRequest` variant, a new capability, or a
  generic host passthrough.** Every correction lands inside the catalog, the
  effect table, and the declared action set that already exist.
- **It must not migrate an arm whose effect differs from the row's.** Where
  the arm is genuinely a different effect, the correction is a kind change
  and a declared action — not a rewiring.
- **It must not touch `/lsp-status` and `/lsp-errors`.** Those two are
  **decided** (M007-D closure §10.2): the arm's behaviour differs by mode, so
  migrating it would change behaviour in both. They belong to **M007-F**, the
  daemon's LSP command surface, which needs its own ADR. This milestone
  records that routing and changes nothing.
- **It must not perform the 28 catalog kind changes and the 24 new
  `FrontendAction` declarations wholesale.** Those are recorded, not applied
  (M007-D closure §10.3). This milestone applies **only** the corrections
  that belong to the ten forms below, and names the rest as still recorded.

## 3. The ten forms, and the decision each needs

Each row is a form the daemon serves, the request its row builds, and the
effect the TUI's arm actually performs. The citations are the daemon's effect
table (`src/core/daemon_commands.rs`) and the TUI's own arm.

| form | the daemon's row serves | the TUI's arm performs | class | decision |
|---|---|---|---|---|
| `skills` | `skill_catalogue_list` (`src/core/daemon_commands.rs:256`) | a read of the TUI's own asset snapshot (`src/tui/app/mod.rs:5720`) | **kind** | the arm is a frontend-local read → correct the kind to `FrontendEffect` and declare the action it performs |
| `status` | `session_lifecycle_get` (`src/core/daemon_commands.rs:308`) | a locally assembled status string — session status, token counters, the current model (`src/tui/app/mod.rs:6002`) | **kind** | same |
| `tool_contracts` | `tool_program_list` (`src/core/daemon_commands.rs:269`) | a read of the TUI's **local** tool registry (`src/tui/commands/diagnostics.rs:661`) — tool *contracts*, not tool *programs* | **kind** | same |
| `worktree` | `managed_worktree_list` (`src/core/daemon_commands.rs:262`) | `worktree_list` by project directory — the checkout listing, a different request (`src/tui/commands/tasks.rs:587`) | **kind** | same |
| `shell_list` | `human_shell_run_capabilities` (`src/core/daemon_commands.rs:571`) | a read of the TUI's **local** shell store | **kind** | same |
| `terminal_show` | `interactive_process_list` (`src/core/daemon_commands.rs:599`) | a purely frontend-local action — switch the view to the named terminal (`src/tui/app/mod.rs:8752`) | **kind** | same |
| `shell_show` | `human_shell_run_read` (`src/core/daemon_commands.rs:553`) | a read of the TUI's local shell store | **granularity** | the daemon's request is issued by the `!`-shell path's **polling loop** — one read per poll. Routing it through `command.execute` would open a canonical `Run` per poll. **Owner decision** |
| `shell_expand` | `human_shell_run_read` (`src/core/daemon_commands.rs:553`) | the same local store, expanded | **granularity** | same, and it shares the row with `shell_show` |
| `shell_kill` | `human_shell_run_cancel` (`src/core/daemon_commands.rs:566`) | a local kill for a local run, and a cancel for a remote one (`src/tui/commands/shell.rs:1173`) | **authority** | the cancel's authority is the **run's own** project, workspace and session — the ones the binding carries — not the caller's scope. A scope-built row could address the wrong run. **Owner decision** |
| `terminal_send` | `interactive_process_input` (`src/core/daemon_commands.rs:597`) | the interactive terminal's **input path**, shared by the `/terminal-send` arm and the live typing path (`src/tui/commands/interactive_terminal.rs:1097`, `:2833`) | **granularity** | the path issues one request **per keystroke**; routing it through `command.execute` would open a canonical `Run` per keystroke. **Owner decision** |

### 3.1 The three classes, and what each one means

**Kind (six forms).** The arm performs a **frontend-local** effect and the
catalog says it is a daemon effect. The correction is the one M007-D
recorded and did not apply: correct the form's `kind` to `FrontendEffect` and
declare the `FrontendAction` the arm already performs. The effect table drops
the row, the catalog publishes the form as not daemon-served, and the desktop
palette shows it as a frontend action rather than dispatching it to the daemon
— which is what it actually is. **No user-visible change**: the TUI keeps its
local read, and the desktop gains the action it was missing.

**Granularity (three forms).** The arm performs the daemon's effect, but the
path fires continuously — one poll, one keystroke. Routing it through
`command.execute` would open a canonical `Run` per iteration, and the `Run`
record is an audit of a *user action*, not of a keystroke. The owner decides
which of these is right:

- **(a)** keep the path outside the catalog, with the bypass recorded (M007-D
  closure §10.7 already records the shape for four other paths); or
- **(b)** make the catalog's form cover the path, and accept the `Run`
  granularity — in which case the `Run` record's meaning changes and the
  change needs recording, not just a row; or
- **(c)** give the path a **coarser** command — one form per *session* of
  input rather than per keystroke — which is a new row and a new encoding.

Option (c) is the only one that keeps the catalog honest without an audit
per keystroke, and it is the one this milestone should implement if the owner
chooses it. Options (a) and (b) are also legitimate; **(b) changes what a
`Run` means**, which is an ownership-adjacent decision and is the reason it is
the owner's rather than the implementer's.

**Authority (one form).** `shell_kill`'s cancel is authorized against the
**run's own** binding, not the caller's scope. A row that built the request
from the scope would authorize against the wrong project, workspace or
session. The correction is a row that takes the run's identity from the
**argument** and resolves the authority from the run's own durable binding —
the same shape M007-D settled for `share`, `unshare` and `export`, where the
caller's choice travels in the argument. **This is the implementer's call if
the owner does not object**, and it needs no new decision shape.

### 3.2 The two `/lsp-*` forms are decided and are not this milestone's work

`/lsp-status` and `/lsp-errors` are served (`src/core/daemon_commands.rs:604`
— one row serves both) and are **not** migrated. The arm's behaviour differs
by mode: in the socket configuration `app.lsp_tool` is `None`, so the arm
refuses with `LSP not available`; in local mode the same arm works, against
the TUI's own second, non-authoritative server
(`src/main.rs:4013` attaches the TUI's `LspTool` **only** when the TUI is not
in socket mode). Migrating the arm would change behaviour in **both** modes.

The consequence is named rather than left implicit: **the daemon serves two
forms the TUI does not use**, and the desktop palette dispatches both because
they are published as served. This milestone records that routing to
**M007-F** and changes nothing about the two forms. The architecture record
is `architecture/lsp.md` §"The `/lsp-*` command forms (M007-D)".

## 4. The presence-panel discrepancy

`/collaborators` is **migrated** — its row serves `team_principal_list` and
the principal listing routes through the daemon. But the TUI reaches that
listing through `/team principals`, not through the `/collaborators` arm, and
the `/collaborators` arm shows the **presence** panel, whose request is
`PresenceSnapshotGet` (`src/core/daemon_projection.rs:104`; the TUI issues it
at `src/tui/commands/presence.rs:105`).

`presence_snapshot_get` is **not a catalog form at all**, so the presence
panel's effect is a daemon effect reached outside the catalog — one of the
bypasses M007-D's closure §10.7 records, and the only one whose effect has
no catalog entry to route through. This milestone must either give it a form
or record it as a named, deliberate exception. A form is preferable: the
presence panel is a **user-visible action** with no `Run` today.

## 5. Required changes

1. **Correct the six kind-classified forms** (`skills`, `status`,
   `tool_contracts`, `worktree`, `shell_list`, `terminal_show`): set each
   form's `kind` to `FrontendEffect` in
   `crates/codegg-core/src/command_catalog.rs`, declare the `FrontendAction`
   each arm performs in `crates/codegg-core/src/frontend_action.rs`, and drop
   the now-wrong rows from the daemon's effect table. Each declaration is
   pinned by the frontend-action guard and by the catalog's own closure test.
2. **Settle the three granularity forms** (`shell_show`, `shell_expand`,
   `terminal_send`) per §3.1 — the owner's decision, recorded, and then
   implemented as that decision says.
3. **Settle `shell_kill`** by making its row take the run's identity from the
   argument and resolve the authority from the run's own binding.
4. **Give the presence panel a form**, or record it as a named exception, per
   §4.
5. **Re-baseline the classification artifact.** The artifact's §1 and §6 move
   with the corrections, and the generator's `--check` must pass.
6. **Update `architecture/command.md`, `architecture/command_routing.md` and
   `architecture/lsp.md`** for the corrected counts, and the closure record
   `plans/closure/desktop-frontend-ide-foundation/014-status.md` for the
   resolved findings — the closure's own §10.3 lists the kind changes as
   *recorded, not applied*, and this milestone is where the six above land.
7. **Record the decision shapes** in the closure and in the architecture
   documents, so a later reader finds why a poll loop is outside the catalog
   rather than inferring an oversight.

## 6. Verification

- `scripts/verify.sh quick` — green, including the catalog-ownership guard's
  `--self-test` and the frontend-action guard's.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- The generator's `--check` — the artifact matches the tree.
- `cargo test --test core_daemon -- command_catalog_m007d` — the daemon's
  catalog tests, including the effect table's closure test, pass.
- `cargo test -p codegg --lib command_route` — the TUI's route coverage passes.
- The desktop's own verification: `npm run typecheck`, `npm test`,
  `npm run bindings:check`, and the hosted Desktop E2E lane — because the
  palette's availability for the corrected forms changes.
- The migration instrument (the owner's read-only check) — every form the
  catalog still serves as a daemon effect is routed through the daemon, and
  every form it serves as a frontend effect is not.

## 7. Acceptance criteria

1. **The catalog is honest.** For every form in the catalog, the kind matches
   what the TUI's arm actually performs: a form whose arm is frontend-local is
   `FrontendEffect`, and a form the daemon serves is `DaemonEffect`.
2. **No form is served and unused.** Every daemon-effect form the daemon
   serves is either executed through the daemon by the TUI, or is recorded
   with a named reason — and the recorded set is **empty or owner-decided**,
   not accidental.
3. **The desktop palette agrees with the TUI.** A form the TUI performs
   locally is not dispatchable to the daemon in the palette, and a form the
   daemon owns is.
4. **Every user action that reaches a daemon effect has a `Run`**, except
   where the owner has decided otherwise and the exception is recorded with
   its reason.
5. **The `Run` record keeps its meaning.** No form opens a `Run` per
   keystroke or per poll unless the owner has decided that it should, and the
   decision is recorded.
6. **The authority of a cancel is the run's own.** `shell_kill` cannot
   address a run outside the caller's scope.
7. **The artifact matches the tree**, and the classification's §1, §3 and §6
   counts agree with the catalog and the effect table.
8. **The TUI's commands, names, aliases and descriptions are unchanged**, and
   the 169 registry entries and the 161 `BuiltinSlashAction` variants are
   byte-identical to `origin/main`.
9. **`scripts/verify.sh quick` is green**, including both catalog guards'
   `--self-test`s, and the workspace clippy is clean at `-D warnings`.
10. **The closure record names every decision this milestone made**, and the
    ones it did not make, with their reasons.

## 8. Closure evidence required

- A closure record at `plans/closure/desktop-frontend-ide-foundation/017-status.md`,
  with the same thirteen-section shape as `014-status.md` and `015-status.md`.
- The registry update and the roadmap update, recording the milestone's
  disposition and the residual findings.
- The before-and-after classification counts, and the artifact's `--check`
  passing on the closed tree.
- The verification actually run, named, with its result.

## 9. Stop conditions

- **If the owner has not decided the three granularity forms**, the milestone
  does not implement them. It lands the six kind corrections, the `shell_kill`
  authority correction and the presence form, and records the three as
  awaiting a decision — the same disposition M007-D used for a finding it
  could not decide.
- **If a correction turns out to need a new `CoreRequest` variant or a new
  capability**, the milestone stops and the decision is escalated. ADR-0052
  does not authorise a new capability, and the plan does not need one.
- **If correcting a form's kind makes the desktop palette lose a command the
  user could run**, the milestone stops: that is a user-visible regression,
  and the fix belongs with the decision that caused it.
