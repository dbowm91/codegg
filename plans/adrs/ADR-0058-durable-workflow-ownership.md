# ADR-0058: Durable Workflow Ownership for the Skill-Promotion and Human-Shell Ask Workflows

Status: proposed — not accepted; this record grants no capability

Date: 2026-10-29

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md`
- `architecture/command.md`
- `architecture/jobs.md`
- `architecture/skills.md`
- `architecture/human_shell.md`
- `architecture/writer_permit.md`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` (gates M007-I)

## Context

M007-I was reserved as "the three subsystem moves". Its premise has since been
corrected — twice — and the correction is what this record decides on. The
milestone's row now names **two** subsystem moves, `/skill-promote` and
`/shell-ask`, and states that the git-review diff view "measured as a single
synchronous local effect, so it is not a subsystem move — and whether it moves
at all is ADR-0058's decision"
(`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:570`).

### 1. The diff view is a single synchronous local effect

`/diff` dispatches at `src/tui/app/mod.rs:8257`; the handler is
`src/tui/app/mod.rs:11268`. Its two branches are **mutually exclusive**:

- `/diff <path>` runs **one** `git show HEAD:<path>` subprocess
  (`src/tui/app/mod.rs:11287-11295`) and reads **one** file
  (`src/tui/app/mod.rs:11297`).
- bare `/diff` runs **one** `git diff` subprocess
  (`src/tui/app/mod.rs:11312-11316`) and reads **no** file.

Both are built by the read-only facts crate's `GitEnvPolicy::apply_sync`
(`crates/egggit/src/process.rs:445`, re-exported at `src/git_mutations.rs:47`),
not by the typed mutating crate `crates/codegg-git` — whose typed read-only
`GitOperation::Diff` (`crates/codegg-git/src/operation.rs:15-21`) and `::Show`
(`:27-29`) this path does not use. The git root is found by a pure filesystem
walk (`crates/codegg-core/src/worktree.rs:126-128` →
`crates/egggit/src/worktree.rs:121`), not a subprocess.

The whole path is local and synchronous. It reaches the UI through
`process_msg(TuiMsg::OpenDiffDialog)` (`src/tui/app/mod.rs:11305`, `:11325`) →
`src/tui/app/input.rs:38-48` → `enqueue_tui_command`
(`src/tui/app/mod.rs:10078`) → `src/tui/runtime/command_dispatch.rs:842-848` →
`src/tui/commands/tasks.rs:717-727`. **No `CoreRequest` is constructed
anywhere on it.** There is no job, no scheduler, no `Run`, and no writer
surface: `src/tui/app/mod.rs` is in neither `WRITER_SURFACE_DIRS`
(`scripts/check_writer_disposition.py:177-192`), `WRITER_SURFACE_FILES`
(`:195-201`) nor `PROCESS_SURFACE_FILES` (`:212-223`), and the guard's effect
primitives (`:165-173`) do not match its read.

### 2. The catalog misdeclares all three forms, and a fourth

`/diff` (`crates/codegg-core/src/command_catalog.rs:1499`),
`/skill-promote` (`:1391`), `/shell-ask` (`:2363`) and `/revert` (`:1553`) are
each declared `CommandKind::DaemonEffect` with
`capability: Some(CommandCapability::CommandExecute)`. **The daemon's effect
table has no row for any of them**: `build_inner_request`
(`src/core/daemon_commands.rs:185`) and `serves_form` (`:778`) contain zero
occurrences of `"diff"`, `"skill_promote"`, `"shell_ask"` or `"revert"`.

This is a standing violation of the repository's own rule:

> **A kind is never inferred.** A form's kind comes from the catalog. The
> daemon may never execute a frontend-effect form, and the frontend may never
> execute a daemon-effect form directly — a daemon effect goes through
> `CoreRequest::CommandExecute`, and the daemon authorizes it under
> `command.execute` and audits it as a canonical `Run`.
> — `architecture/command.md:450-454`

`/revert` is the same defect with a writer surface attached: the TUI runs
`git checkout HEAD -- <path>` in the **TUI process**
(`src/tui/app/mod.rs:11433-11443`), and `docs/writer-disposition.toml` records
**no** revert-like surface (`grep -c revert` on the manifest returns `0`).
`/revert` is M007-H's subject (plan 020), not this record's; it is named here
because it is the repository's clearest existing example of a checkout mutation
with no owner.

### 3. The skill-promotion workflow is five steps across two processes

`/skill-promote` dispatches inline at `src/tui/app/mod.rs:6907`; there is no
separate handler function. The workflow it starts has five steps:

1. **Request** — `src/tui/app/mod.rs:6945-6955` calls
   `SkillPromotionStore::begin_request` (`src/skills/promotion.rs:306-349`),
   which reads the habit candidate and writes a durable, TTL-bounded request
   record (`REQUEST_TTL_MS`, `src/skills/promotion.rs:26`).
2. **Prompt** — `src/tui/app/mod.rs:6958-6961` sets the composer text and
   `send_prompt()`s a **daemon** turn.
3. **Model drafts** — the daemon-side model tool `skill_proposal`
   (`src/tool/skill_proposal.rs:85-157`) writes the proposal durably via
   `store.submit(...)` (`src/tool/skill_proposal.rs:117-128` →
   `src/skills/promotion.rs:429`), then attaches collision diagnostics
   (`src/tool/skill_proposal.rs:136-148`).
4. **Human previews** — `/skill-proposal <id>` at `src/tui/app/mod.rs:7007+`;
   `mark_previewed` (`src/skills/promotion.rs:285`) mutates the record.
5. **Human publishes** — `/skill-proposal publish <id> project|global`
   (`src/tui/app/mod.rs:7007-7037`) → `TuiCommand::SkillPublish` (`:7033`) →
   `start_skill_publish` (`src/tui/commands/memory.rs:779-862`) →
   `SkillPublicationService::publish` (`src/skills/publish.rs:87-128`).

Publication writes the skill root: `ensure_directory`
(`src/skills/publish.rs:579-602`), a temporary file written and fsynced
(`:299-306`), an atomic no-replace commit by `hard_link` (`:332-337`), and a
directory `sync` (`:345`, `:746-749`), into
`<project>/.codegg/skills/<name>/SKILL.md` or
`<config_dir>/codegg/skills/<name>/SKILL.md` (`src/skills/publish.rs:412-434`).
The TUI then asks the daemon to refresh (`src/tui/commands/memory.rs:864-873`).

### 4. The promotion store has two writers and no owner

The store's root is `dirs::config_dir()/codegg/memory/skill-promotions`
(`src/skills/promotion.rs:231-243`), created on demand
(`src/skills/promotion.rs:267`). **Both processes write it**: the TUI process
(request, preview, reject, publish) and the daemon-side model tool
(`src/tool/skill_proposal.rs:117-128`). The two are coordinated by an `flock`
on a sidecar lock file and an atomic temp-file rename
(`src/skills/promotion.rs:629-647`) — a protocol, not an owner.

The store's namespace key is `project_namespace(project_identity)`, a SHA-256
of a **path string** (`crates/codegg-core/src/memory/mod.rs:36-41`). The TUI
passes `active_workspace_root()` (`src/tui/app/mod.rs:6948`); the daemon tool
passes `ctx.cwd` (`src/tool/skill_proposal.rs:112`). These are derived through
different paths, and nothing enforces that they are the same string.

There is **no `CoreRequest` variant for skill promotion or publication** — the
protocol has none, so a second frontend cannot drive this workflow at all.

### 5. The human-shell ask is two daemon requests with a durable record between them

`/shell-ask` dispatches at `src/tui/app/mod.rs:8971`; the handler is
`src/tui/commands/shell.rs:684-736`; the submission is
`src/tui/commands/shell.rs:738-880`.

The ask resolves the shell id against the **TUI-local** store
(`src/tui/app/mod.rs:9839-9844`), assembles the evidence
(`src/tui/commands/shell.rs:707-724`), redacts it
(`src/tui/commands/shell.rs:915`), then issues **two** daemon requests:

- `CoreRequest::HumanShellPromotionStage` (`src/tui/commands/shell.rs:764-772`)
  → `src/core/daemon_turns.rs:1152` → `stage_human_shell_promotion`
  (`:2066-2142`), which writes a **durable** `pending` record
  (`:2117-2119` → `crates/codegg-core/src/session/human_shell_promotion.rs:169`,
  `:181`).
- `CoreRequest::SessionPromptSubmit` carrying
  `human_shell_promotion_ids` (`src/tui/commands/shell.rs:773-781`) → the turn
  is admitted with the promotions
  (`src/core/daemon_turns.rs:742`, `:752`).

The waiter is released by `apply_shell_ask_submitted`
(`src/tui/commands/shell.rs:882-910`), which marks the evidence promoted and
clears the composer — matching `architecture/human_shell.md:96-97` ("`/shell-ask`
waits for both stage acknowledgement and session turn acceptance").

There is a **non-atomic seam**: only the *stage* request is retried
(`src/tui/commands/shell.rs:786-801`). If the *submit* fails after the stage
succeeded, the `pending` promotion row remains and the turn never runs
(`src/tui/commands/shell.rs:834-845`).

### 6. The ask reads a store the daemon has already demoted

Plan 018 moved `/shell-list` from `app.shell_store.list_recent(10)` to the
daemon's published list, and states that "the TUI's local store keeps the
**output bytes** it renders"
(`plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md:245-249`).
`/shell-ask` still reads that local store directly
(`src/tui/commands/shell.rs:694`). The daemon owns the run registry and its
metadata (`src/shell/daemon.rs:60`, owned at `src/core/daemon.rs:81`); the TUI
holds the bytes.

### 7. The governing rule

The repository already draws the line this record needs:

> **The catalog's owner is the daemon.** … A command's *kind* decides who runs
> it: a **daemon effect** is executed by the daemon under `command.execute` and
> audited as a canonical `Run`; a **frontend effect** is executed locally by the
> frontend against the declared frontend-action set and creates **no** `Run**.
> — `architecture/command.md:12-20`

A **synchronous local effect** is a frontend effect: one local operation, no
daemon request, no durable record, no `Run`. A **durable workflow** is a
daemon-owned sequence whose state outlives the request that began it, and the
repository already names the four facilities such an owner must provide:

- the **job system** — `codegg_core::jobs` (`JobId`,
  `crates/codegg-core/src/jobs/mod.rs:380`; `JobKind`, `:506`; `JobState`,
  `:1007`; `JobPayload`, `:1174`; `JobRecord`, `:1416`;
  `SqliteJobStore`, `crates/codegg-core/src/jobs/store.rs:1201`),
- the **scheduler** — `JobScheduler` (`src/scheduler/scheduler.rs:233`) and
  the daemon-owned submission boundary `JobSubmissionService`
  (`src/scheduler/submission.rs:122`),
- the **writer permit** — `codegg_core::writer_permit`
  (`crates/codegg-core/src/writer_permit.rs:102`, `:164`, `:238`, `:278`,
  `:502`, `acquire` at `:652`),
- the **run store** — `codegg_core::run_store` (`RunManifest`,
  `crates/codegg-core/src/run_store.rs:536`; `RunStore` trait, `:1145`;
  `FsRunStore`, `:1236`), which `architecture/jobs.md:3-8` names the
  "artifact and execution-provenance authority".

**ADR-0052 does not cover this.** It decided cross-frontend *command
execution* ownership (the catalog is the daemon's; a frontend dispatches
commands and the daemon runs them). It did not decide who owns a
**multi-request workflow's durable state**, and it did not reach these three
forms: the catalog declares them daemon effects, yet the daemon serves none of
them, so nothing ADR-0052 governs is in play. That is why ADR-0058's number
was reserved.

## Decision drivers

1. **One owner per durable workflow.** A durable workflow owner must provide
   the four facilities named in §7. The promotion store currently has **two**
   writers and no owner, coordinated only by an `flock` protocol.
2. **The two writers must resolve to one.** After this milestone, the
   promotion store has **exactly one** writer, and the record names it.
3. **`/shell-ask` reads a demoted store.** The ask reads the TUI-local shell
   store that plan 018 demoted to a projection
   (`018-the-daemon-s-published-projection-surfaces.md:245-249`); a second
   frontend cannot perform it.
4. **The catalog must stop lying.** All three forms are declared
   `DaemonEffect` and executed locally, violating `architecture/command.md:450-454`.
   M007-D's corrective established the precedent for the fix: a kind
   correction changes three fields on one descriptor — the descriptor's
   `kind`, its `capability` (to `None`) and the form's `kind` — and adds a
   declared `FrontendAction`
   (`plans/closure/desktop-frontend-ide-foundation/017-status.md:103-117`).
5. **`/diff` is a reclassification question, not a move.** It issues no
   daemon request and holds no workflow state. Whether the desktop gets a
   diff view at all is a product question the code cannot settle.
6. **The manifest must stay honest.** Both skill surfaces are already
   recorded (`module.skills_promotion`, `docs/writer-disposition.toml:714-725`;
   `module.skills_publish`, `:727-738`), both `shared-sidecar` with
   `mediation = "none"`. A single owner changes what those rows say, and the
   global scope writes outside the stated `locator`
   (`src/skills/publish.rs:424`).
7. **Nothing may be implemented on a pending record.** This record is
   proposed; the owner accepts it.

## Options

### Option A — The two workflows move; the diff view is reclassified

`/skill-promote` and `/shell-ask` each get a daemon-owned durable workflow
owner. `/diff` stays a frontend-local synchronous effect, and its catalog form
is reclassified to `FrontendEffect` with a declared `FrontendAction`.

- **Writer dispositions.** `module.skills_promotion` and
  `module.skills_publish` remain, with `owner`, `mediation` and `locator`
  updated to name the single writer and both roots. `/diff` needs no manifest
  row at all: it is not a writer surface, and `src/tui/app/mod.rs` is not in
  the guard's surface lists.
- **`Run` semantics.** The two workflows gain canonical `Run`s through the
  daemon's `command.execute` path
  (`src/core/daemon_commands.rs:1055-1109`); `/diff` continues to create
  **no** `Run`, which is exactly what `architecture/command.md:12-20` says a
  frontend effect does.
- **Desktop.** The desktop gains the skill-promotion and shell-ask
  workflows; the diff view remains **desktop-absent** and the desktop says so
  honestly, as it does today for `UNCARRIED_ACTIONS`.
- **Cost.** Two workflow owners, two protocol surfaces, one manifest update,
  one kind correction.

### Option B — All three move to the daemon

`/diff` also becomes a daemon effect, so both frontends see the same diff.

- **Writer dispositions.** Unchanged for `/diff` (still not a writer
  surface), but a **new daemon writer surface** appears for the git read,
  which the guard's `WRITER_SURFACE_FILES`/module sweep will detect.
- **`Run` semantics.** Every `/diff` becomes a canonical `Run` — a durable
  audit record for a read that is currently synchronous and transient.
  `architecture/command.md:12-20` says a daemon effect is "audited as a
  canonical `Run`"; a diff view would produce one per invocation.
- **Desktop.** The desktop gains a diff view.
- **Cost.** Moving two `git` subprocesses and one file read into the daemon,
  plus a `Run` per diff, to buy a read the frontend can already do. The
  daemon would reach the same working tree the frontend already has.

### Option C — None move

The premise is corrected, the milestone closes as not-applicable, and the two
workflows stay TUI-local.

- **Writer dispositions.** No change; the two existing rows stand as written.
- **`Run` semantics.** Unchanged; the workflows create no `Run` and the
  catalog's misdeclaration stands.
- **Desktop.** The desktop gains neither workflow; the promotion store keeps
  two writers, and `/shell-ask` keeps reading a store the daemon has demoted.
- **Cost.** The two-writer hazard and the demoted-store read are **preserved
  as defects**, and the catalog keeps declaring three daemon effects the
  daemon does not serve.

## Decision

**Pending the owner. This record proposes Option A and does not select it.**

Option A is recommended because it is the only option the measurements
support: `/diff` issues **no** daemon request, holds no workflow state and
performs no writer effect, so it is a **reclassification**, not a subsystem
move (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:570`;
`plans/registry.md:1167`). The two workflows are a different matter — one has
two writers and no owner, the other reads a store the daemon has demoted —
and both need a daemon-owned durable workflow owner.

**No capability is granted by this record, and nothing may be implemented
until the owner accepts it.** The decision that is pending is recorded in the
implementation plan's §5.3 as seven owner decisions (D1–D7), each of which
names the work packages it gates. The recommended shape, for the owner's
consideration:

- **D1** — reclassify `/diff`'s form to `FrontendEffect` (Option A), rather
  than moving it.
- **D2–D5** — give the skill-promotion workflow **one** owner (the daemon),
  key its store on a canonical identity, let the daemon publish to both
  scopes, and mediate publication through the checkout reservation gate.
- **D6–D7** — make the ask's stage-and-submit **one** daemon request, and
  have the ask read the daemon's run bytes.

Acceptance of this record by the project maintainer is what selects an option;
this record, in its proposed state, selects none.

## Consequences

### Positive

- The promotion store has **exactly one** writer, so the `flock` protocol
  (`src/skills/promotion.rs:629-647`) stops being the de-facto owner.
- `/shell-ask` stops reading a store the daemon has demoted, and the desktop
  can perform it.
- The catalog stops declaring three daemon effects the daemon does not serve,
  closing a standing violation of `architecture/command.md:450-454`.
- `/diff` keeps its honest shape: synchronous, local, no `Run`, no writer
  surface, no new durable state.

### Negative

- Two durable workflow owners and two protocol surfaces are new work, with
  their own migration and verification.
- The skill-promotion store's on-disk format changes if it moves into the
  daemon's session storage, and in-flight proposals must be migrated or
  discarded.
- The desktop still has no diff view under Option A.

### Neutral or deferred

- `/revert`'s missing writer disposition is **M007-H's** subject (plan 020,
  §6.2) and is named here only as evidence of the same defect class.
- ADR-0057 (M007-G) is a separate reservation and is untouched.
- ADR-0052, ADR-0053 and ADR-0056 are **accepted** and are not amended.

## Compatibility, security and migration

**Storage.** If the promotion store moves into the daemon's session storage
(D2 Option A), the existing per-namespace JSON files under
`<config_dir>/codegg/memory/skill-promotions` must be migrated or explicitly
discarded; the namespace key changes from a path-string SHA-256
(`crates/codegg-core/src/memory/mod.rs:36-41`) to a canonical identity. In-flight
proposals that do not migrate must fail loudly, not silently vanish. The
daemon's existing `HumanShellPromotionStore`
(`crates/codegg-core/src/session/human_shell_promotion.rs:169`) is the
in-tree precedent for a session-scoped durable record and should be followed
rather than a new store invented.

**Protocol.** New request variants are additive and typed; an older client
must receive an explicit refusal for a form it cannot drive, never a silent
no-op. The `daemon_served` publication must not advertise a form before its
request, authorization and bounded result exist
(`plans/adrs/ADR-0056-daemon-lsp-command-and-runtime-control-surface.md:66`).

**Security.** A daemon-executed skill publication writes into a skill root
that the agent's own context reads — the same trust direction as the model's
typed Git mutations. Under D5 Option C, the publication takes a
`CheckoutWriterSink` gate and records a durable refusal
(`crates/codegg-core/src/audit.rs:175`); under Option B it takes the writer
permit (`crates/codegg-core/src/writer_permit.rs:652`). Under no option may a
publication bypass the mediation its manifest row declares. The
`frontend-action` guard permits only `NonMutating` and `ProcessLaunching`
(`scripts/check_frontend_action_set.py:65-68`) and `FrontendActionDisposition`
has **no** mutating variant (`crates/codegg-core/src/frontend_action.rs:24-29`),
so publication **cannot** be a frontend action without a policy change beyond
this record.

**Migration.** The TUI and the desktop must reach parity: until the daemon
serves the workflow, the desktop's honest notice stands. No form may be
marked served before it is.

## Required verification

1. **The promotion store has exactly one writer.** A test that fails when a
   second writer appears — a static check over the writer sites
   (`src/tui/app/mod.rs:6945-6955`, `src/tui/commands/memory.rs:779-862`,
   `src/tool/skill_proposal.rs:117-128`) plus the moved owner.
2. **The catalog declares what the daemon serves.** `python3
   scripts/check_command_catalog_ownership.py` and its `--self-test`, plus the
   generated inventory
   (`python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check`),
   which must show `/diff` issuing **no** daemon request and the M007-I row
   reading "the 2 subsystem moves".
3. **The kind correction is applied correctly.** The descriptor's `kind`, its
   `capability` (set to `None`) and the form's `kind` all change, and the new
   `FrontendAction` is declared with the `NonMutating` disposition
   (`scripts/check_frontend_action_set.py`).
4. **The manifest is honest.** `python3 scripts/check_writer_disposition.py`
   and its `--self-test`; `module.skills_promotion` and `module.skills_publish`
   name the single owner and both roots, and a declared-but-absent path fails
   the guard (`scripts/check_writer_disposition.py:596-600`).
5. **The ask's seam is closed.** A test that a failed submit leaves no orphan
   `pending` promotion, or that the orphan is reconciled by a named daemon
   reconciler.
6. **The ADR's own integrity.** `python3 scripts/check_adr_integrity.py` and
   its `--self-test` — the record's number is unique and every ADR path it
   names resolves.
7. **No acceptance is implied.** The record's status line reads `proposed`
   until the owner changes it; the roadmap and registry rows that say "not yet
   written" are updated only at acceptance.

Supersession: none.
