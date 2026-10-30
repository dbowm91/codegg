# Desktop Frontend and IDE Foundation Milestone 022 — The Two Subsystem Moves: A Durable Workflow Owner for Skill Promotion and the Human-Shell Ask

Status: blocked — gated on ADR-0058, which is proposed and not accepted

Repository baseline: `4db5039a0` — `origin/main` merged into
`feat/auto-20261024-6e9f52e4`, carrying **M007-J** closed 2026-10-29 and the
M007-D classification generator's correction.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- The M007-I milestone row: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:570`

Planning basis:

- The M007-I milestone row — *"The two subsystem moves. The skill-promotion
  workflow and the human-shell ask workflow. The git-review diff view is
  **not** among them: it measured as a single synchronous local effect, so it
  is not a subsystem move — and whether it moves at all is ADR-0058's
  decision."*
- The M007-D corrective pass (**plan 017**) and its closure record,
  `plans/closure/desktop-frontend-ide-foundation/017-status.md` §3.1 — the
  six **kind corrections**, which are this plan's structural precedent.
- The projection milestone (**plan 018**) —
  `plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md:245-249`
  — whose demotion of the TUI-local shell store this plan interacts with.

Applicable ADRs:

- **ADR-0058 — proposed, not accepted.** This milestone is gated on it. It
  reserves the ownership decision for the two workflows and for whether the
  diff view moves at all. Nothing in this plan may be implemented until the
  owner accepts it (`plans/adrs/README.md:52`).
- This milestone is bound by **ADR-0010** (the desktop frontend and shared
  client boundary), **ADR-0017** (human-shell promotion admission),
  **ADR-0052** (cross-frontend command execution ownership) and **ADR-0053**
  (the daemon's published projection surfaces), and it revisits none of their
  conclusions.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.4 — a frontend renders projections
  and does not own durable state. The skill-promotion store and the shell-ask
  evidence are durable state.
- `plans/002-long-term-roadmap.md:899` — *"When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap."*

## 1. Objective

The two workflows M007-I names — **skill promotion** (`/skill-promote`) and
the **human-shell ask** (`/shell-ask`) — each get **one** daemon-owned durable
workflow owner, and the **diff view** (`/diff`) is settled as what it
measurably is: a synchronous frontend-local effect whose catalog form is
**reclassified**, not moved.

The milestone's deliverable is the **ownership**, not the surface area. It is
complete when the promotion store has **exactly one** writer, when the ask's
two requests and the record between them are owned end to end by the daemon,
when the catalog stops declaring daemon effects the daemon does not serve,
and when the manifest says who may write each skill root.

**`/diff` is deliberately not a move.** §3.1 measures it; §6.2 reclassifies
it; the desktop's honest notice for it remains.

## 2. Why this milestone is ready

- **The premise is corrected and independently confirmed.** The roadmap row
  now reads "the two subsystem moves"
  (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:570`); the
  generated classification's M007-I row reads "the 2 subsystem moves"
  (`plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md:627`);
  and §3.1 below measures the diff view's shape from the code.
- **The workflows are already implemented and already broken in the recorded
  ways.** The promotion store has two writers (§3.2) and the ask reads a
  demoted store (§3.4). Neither needs a new product surface to be fixed.
- **The precedent exists.** M007-D's corrective established the exact shape
  of a kind correction: three fields on one descriptor and one declared
  `FrontendAction`
  (`plans/closure/desktop-frontend-ide-foundation/017-status.md:103-117`).
- **The manifest already records the surfaces.** `module.skills_promotion`
  (`docs/writer-disposition.toml:714-725`) and `module.skills_publish`
  (`:727-738`) exist, so the change is an **update** to recorded rows, not a
  new surface discovered late.
- **The dependency is a decision, not an implementation.** ADR-0058 is
  proposed; this plan is written so that each work package is executable the
  moment its decision lands, and **no work package silently picks one**.

## 3. Current implementation evidence

### 3.1 The diff view is a single synchronous local effect

`/diff` dispatches at `src/tui/app/mod.rs:8257`; the handler is
`src/tui/app/mod.rs:11268`. The two branches are **mutually exclusive**:

| Branch | Steps | `path:line` |
|---|---|---|
| `/diff <path>` | **one** `git show HEAD:<path>` subprocess; **one** file read | `src/tui/app/mod.rs:11287-11295`; `:11297` |
| bare `/diff` | **one** `git diff` subprocess; no file read | `src/tui/app/mod.rs:11312-11316` |

The git root is found by a pure filesystem walk, not a subprocess
(`crates/codegg-core/src/worktree.rs:126-128` →
`crates/egggit/src/worktree.rs:121`). Both subprocesses are built by the
read-only facts crate's `GitEnvPolicy::apply_sync`
(`crates/egggit/src/process.rs:445`, re-exported at `src/git_mutations.rs:47`)
— **not** by the typed mutating crate `crates/codegg-git`, whose typed
read-only `GitOperation::Diff` (`crates/codegg-git/src/operation.rs:15-21`)
and `::Show` (`:27-29`) this path does not use.

The path is local end to end: `process_msg(TuiMsg::OpenDiffDialog)`
(`src/tui/app/mod.rs:11305`, `:11325`) → `src/tui/app/input.rs:38-48` →
`enqueue_tui_command` (`src/tui/app/mod.rs:10078`) →
`src/tui/runtime/command_dispatch.rs:842-848` →
`src/tui/commands/tasks.rs:717-727`. **No `CoreRequest` is constructed
anywhere on it.** There is no job, no scheduler and no `Run`.

It performs **no** writer effect: `src/tui/app/mod.rs` is in neither
`WRITER_SURFACE_DIRS` (`scripts/check_writer_disposition.py:177-192`), nor
`WRITER_SURFACE_FILES` (`:195-201`), nor `PROCESS_SURFACE_FILES` (`:212-223`),
and the guard's effect primitives (`:165-173`) do not match its read. Both git
subcommands are in the object-reading allowlist
(`crates/egggit/src/process.rs:1434`, `:1445`), so a private object view is
prepared and `GIT_OPTIONAL_LOCKS=0` is set (`:459-462`), which suppresses
git's index-refresh write.

**The catalog does not agree.** `/diff` is declared
`CommandKind::DaemonEffect` with `capability:
Some(CommandCapability::CommandExecute)`
(`crates/codegg-core/src/command_catalog.rs:1499-1515`), and the daemon's
effect table has **no row** for it (`src/core/daemon_commands.rs:185`,
`serves_form` at `:778`; `"diff"` occurs **0** times in that file). The same
holds for `/skill-promote` (`:1391`), `/shell-ask` (`:2363`) and `/revert`
(`:1553`). This violates `architecture/command.md:450-454`: *"the frontend may
never execute a daemon-effect form directly."*

### 3.2 The skill-promotion workflow is five steps, and the store has two writers

`/skill-promote` dispatches inline at `src/tui/app/mod.rs:6907`; there is no
separate handler function. The workflow it starts:

| Step | What happens | `path:line` | Process |
|---|---|---|---|
| 1. Request | `SkillPromotionStore::begin_request` reads the habit candidate and writes a durable, TTL-bounded request record | `src/tui/app/mod.rs:6945-6955` → `src/skills/promotion.rs:306-349` | **TUI** |
| 2. Prompt | composer text set; `send_prompt()` starts a turn | `src/tui/app/mod.rs:6958-6961` | TUI → **daemon** |
| 3. Model drafts | the `skill_proposal` tool writes the proposal durably and attaches collision diagnostics | `src/tool/skill_proposal.rs:117-128`, `:136-148` | **daemon** |
| 4. Human previews | `mark_previewed` mutates the record | `src/skills/promotion.rs:285` | **TUI** |
| 5. Human publishes | `start_skill_publish` → `SkillPublicationService::publish` writes the skill root | `src/tui/commands/memory.rs:779-862` → `src/skills/publish.rs:87-128` | **TUI** |

Publication's filesystem effects: `ensure_directory`
(`src/skills/publish.rs:579-602`), a temp file written and fsynced
(`:299-306`), an atomic no-replace `hard_link` commit (`:332-337`), and a
directory sync (`:345`, `:746-749`), into
`<project>/.codegg/skills/<name>/SKILL.md` or
`<config_dir>/codegg/skills/<name>/SKILL.md`
(`src/skills/publish.rs:412-434`, `:552-566`).

**The two-writer hazard.** The store's root is
`dirs::config_dir()/codegg/memory/skill-promotions`
(`src/skills/promotion.rs:231-243`), created on demand (`:267`). Step 1/4/5
write it from the **TUI process**; step 3 writes it from the **daemon**
(`src/tool/skill_proposal.rs:113-128`). The two are coordinated by an `flock`
on a sidecar lock and an atomic rename
(`src/skills/promotion.rs:629-647`) — a **protocol, not an owner**.

**The identity key is a path string.** The namespace is
`project_namespace(project_identity)`, a SHA-256 of a path string
(`crates/codegg-core/src/memory/mod.rs:36-41`). The TUI passes
`active_workspace_root()` (`src/tui/app/mod.rs:6948`); the daemon tool passes
`ctx.cwd` (`src/tool/skill_proposal.rs:112`). Nothing enforces that the two
strings are equal; if they diverge, the daemon's `submit` fails with "promotion
request not found" (`src/skills/promotion.rs:481`).

**There is no protocol surface for this workflow.** No `CoreRequest` variant
for skill promotion or publication exists, so a second frontend cannot drive
it at all.

### 3.3 The human-shell ask is two daemon requests and one durable record

`/shell-ask` dispatches at `src/tui/app/mod.rs:8971`; the handler is
`src/tui/commands/shell.rs:684-736`; the submission is
`src/tui/commands/shell.rs:738-880`.

1. The shell id resolves against the **TUI-local** store
   (`src/tui/app/mod.rs:9839-9844`).
2. The evidence is assembled from that store (`src/tui/commands/shell.rs:707-724`)
   and redacted (`:915`).
3. **Request 1** — `CoreRequest::HumanShellPromotionStage`
   (`src/tui/commands/shell.rs:764-772`) → `src/core/daemon_turns.rs:1152` →
   `stage_human_shell_promotion` (`:2066-2142`), which writes a **durable**
   `pending` record (`:2117-2119` →
   `crates/codegg-core/src/session/human_shell_promotion.rs:169`, `:181`).
4. **Request 2** — `CoreRequest::SessionPromptSubmit` carrying
   `human_shell_promotion_ids` (`src/tui/commands/shell.rs:773-781`); the turn
   is admitted with the promotions (`src/core/daemon_turns.rs:742`, `:752`).
5. The waiter is released by `apply_shell_ask_submitted`
   (`src/tui/commands/shell.rs:882-910`), matching
   `architecture/human_shell.md:96-97`.

**The seam.** Only request 1 is retried (`src/tui/commands/shell.rs:786-801`).
If request 2 fails after request 1 succeeded, the `pending` row remains and
the turn never runs (`src/tui/commands/shell.rs:834-845`).

### 3.4 The ask reads a store the daemon has demoted

Plan 018 moved `/shell-list` from `app.shell_store.list_recent(10)` to the
daemon's published list and states that "the TUI's local store keeps the
**output bytes** it renders"
(`plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md:245-249`).
`/shell-ask` still reads that local store directly
(`src/tui/commands/shell.rs:694`). The daemon owns the run registry and its
metadata (`src/shell/daemon.rs:60`, owned at `src/core/daemon.rs:81`); the TUI
holds the bytes. A second frontend therefore cannot perform the ask.

### 3.5 Ownership today

| Concern | Owner today | `path:line` |
|---|---|---|
| Diff view | the **TUI process**; no durable state | `src/tui/app/mod.rs:11268-11347` |
| Promotion **request/preview/reject** records | the **TUI process** | `src/tui/app/mod.rs:6945-6955`; `src/skills/promotion.rs:285`, `:379` |
| Promotion **proposal** record | the **daemon** (model tool) | `src/tool/skill_proposal.rs:117-128` |
| Skill **publication** (the write) | the **TUI process** | `src/tui/commands/memory.rs:779-862`; `src/skills/publish.rs:87-128` |
| Skill **catalogue** | the **daemon**, per call | `src/core/daemon_reads.rs:334-364` |
| Skill **registry** the agent sees | the **daemon**, per turn | `src/agent/asset_snapshot_builder.rs:93` |
| A **second** skill registry | the **TUI** | `src/tui/app/mod.rs:6934-6938`, `:7093-7097` |
| Shell **run** registry and metadata | the **daemon** | `src/shell/daemon.rs:60`; `src/core/daemon.rs:81` |
| Shell output **bytes** | the **TUI** | `src/tui/mod.rs:200-205`; `src/tui/runtime/command_dispatch.rs:2139` |
| Ask **durable record** and the answering **turn** | the **daemon** | `src/core/daemon_turns.rs:2117-2119`, `:742`, `:752` |

## 4. Invariants that must not regress

1. **The frontend never executes a daemon-effect form directly**
   (`architecture/command.md:450-454`). Every kind correction in this
   milestone is a correction *toward* truth, not a licence.
2. **A frontend effect creates no `Run`** (`architecture/command.md:12-20`).
   The reclassified diff view must not grow a durable audit record.
3. **The diff read stays read-only.** No checkout mutation, no index write,
   no `Run`. `GIT_OPTIONAL_LOCKS=0` must remain in force under the private
   object view (`crates/egggit/src/process.rs:459-462`).
4. **Exactly one writer of the promotion store** after this milestone.
   Two writers coordinated only by a lock file is not ownership.
5. **The skill roots' race is not widened.** Both roots are fixed names every
   actor writes (`docs/writer-disposition.toml:725`, `:738`); a single owner
   must serialize them, not merely reduce the odds.
6. **The daemon's `daemon_served` publication stays truthful.** A form is
   not advertised as served until its request, authorization and bounded
   result exist (`plans/adrs/ADR-0056-daemon-lsp-command-and-runtime-control-surface.md:66`).
7. **A frontend action may not mutate.** `FrontendActionDisposition` has no
   mutating variant (`crates/codegg-core/src/frontend_action.rs:24-29`) and
   the guard permits only `NonMutating` and `ProcessLaunching`
   (`scripts/check_frontend_action_set.py:65-68`). Skill publication is a
   writer and therefore **cannot** be a frontend action.
8. **The daemon's human-shell execution ownership is untouched**
   (`architecture/human_shell.md:100-101`).

## 5. Scope

### 5.1 In scope

- The **diff view's reclassification**: the catalog form's `kind`, the
  descriptor's `kind` and `capability`, and one declared `FrontendAction`.
- The **skill-promotion workflow's single owner**, its identity key, and its
  publication authority.
- The **human-shell ask's** request shape, its durable record's owner, and
  the store it reads.
- The **manifest update** for `module.skills_promotion` and
  `module.skills_publish`.
- The **generated classification artifact's** M007-I row, if the milestone
  changes any measured figure.

### 5.2 Explicitly out of scope (non-goals and stop conditions)

- **Moving `/diff` to the daemon.** Under D1 Option A it does not move. If
  the owner selects D1 Option B, this plan is **wrong** and must be
  re-planned; it must not be improvised here.
- **`/revert`.** Its writer disposition is M007-H's subject (plan 020 §6.2).
  This plan records the gap; it does not close it.
- **The TUI-local skill registry as a second authority.** §3.5 records that
  the TUI builds its own registry (`src/tui/app/mod.rs:6934-6938`). Whether
  that becomes a projection is a **separate** decision, not this milestone's.
- **A new durable-workflow abstraction.** The repository forbids a second
  one; the owner must name an *existing* facility (the job system, the
  scheduler, the writer permit, the run store — §3.5 of ADR-0058).
- **Any product surface for the desktop.** This milestone delivers
  ownership, not a new desktop panel. The desktop's parity for these forms
  follows the ownership landing.

### 5.3 Decision points pending owner answer

Seven decisions are pending under **ADR-0058**, which is **proposed and not
accepted**. Each work package that depends on one says so at its own point
and is written so it is executable the moment the decision lands. No work
package below silently picks one.

#### D1 — Does the diff view move to the daemon at all?

*Statement:* whether `/diff` becomes a daemon effect or stays a frontend-local
synchronous effect. §3.1 measures it as issuing **no** daemon request.

- **Option A — reclassify to a `FrontendEffect` form.** The descriptor's
  `kind` and the form's `kind` become `FrontendEffect`, the descriptor's
  `capability` becomes `None`, and one `FrontendAction` is declared with the
  `NonMutating` disposition. This is M007-D's kind correction, in the exact
  shape `plans/closure/desktop-frontend-ide-foundation/017-status.md:103-117`
  records. **Cost:** no protocol change, no `Run`, no manifest row; the
  desktop still has no diff view.
- **Option B — make it a daemon effect.** A new request, an effect-table row,
  and a canonical `Run` per invocation
  (`src/core/daemon_commands.rs:1055-1109`). **Cost:** the daemon must reach
  the same working tree the frontend already has, and every diff becomes a
  durable audit record for a read.
- **Option C — leave the kind misdeclared.** **Cost:** the standing
  violation of `architecture/command.md:450-454` persists.

**Recommended: Option A**, because §3.1 measures the effect and Option A is
the only one that matches the measurement. Option B remains available if the
owner decides the desktop's parity is worth a `Run` per diff.

**Consequence.** Option A: work package **A** discharges this and the
milestone's diff-view item closes with **no daemon surface**. Option B: work
package **A** is replaced by a daemon request, an effect-table row and a
canonical `Run` path, and the milestone's scope grows — the plan must be
re-written before that work begins, not improvised inside it.

#### D2 — Which single owner writes the promotion store?

*Statement:* who owns the durable promotion record, given that the TUI process
and a daemon-side model tool both write it today (§3.2).

- **Option A — the daemon's session storage.** The store follows the
  `HumanShellPromotionStore` precedent
  (`crates/codegg-core/src/session/human_shell_promotion.rs:169`); the TUI
  gains request variants and holds no durable state.
  **Cost:** a protocol surface, a storage migration, and a decision about
  in-flight proposals.
- **Option B — the TUI process keeps the file store and the daemon becomes a
  client.** **Cost:** the daemon reaches into a TUI-owned file to serve a
  second frontend, which inverts the ownership the milestone exists to fix.
- **Option C — a third host process owns the store.** **Cost:** a new durable
  owner in a codebase that forbids a second durable-workflow abstraction.

**Recommended: Option A.** The reason is the invariant in §4.4: two writers
coordinated by a lock file is a protocol, and a durable workflow owner must be
one of the four facilities the repository already names.

**Consequence.** Option A: work package **B** moves the store and adds the
request variants; work package **C** re-points the writers. Options B/C: work
package **B** is re-shaped and the closure record must state why the
"durable workflow owner" is not the daemon.

#### D3 — What identity keys the promotion store?

*Statement:* the namespace key, today a SHA-256 of a path string
(`crates/codegg-core/src/memory/mod.rs:36-41`) derived differently on each
side (§3.2).

- **Option A — the canonical `WorkspaceId`.** **Cost:** a namespace
  migration; in-flight proposals must migrate or fail loudly.
- **Option B — a stable `ProjectId`.** **Cost:** the same migration, plus a
  decision about whether a project may be renamed or re-rooted.
- **Option C — keep the path string and enforce an invariant.** **Cost:** a
  recorded invariant that `active_workspace_root()` and `ctx.cwd` are the
  same string, with a guard that proves it. Nothing enforces it today.

**Recommended: Option A**, with Option C as the fallback if the migration is
judged too costly. Option C is only honest if the invariant is **enforced**,
not merely documented.

**Consequence.** Option A: work package **B** migrates the namespace.
Option C: work package **B** adds the invariant's guard, and the closure
record must state that the identity remains a path.

#### D4 — Does the daemon publish skills, or does the TUI keep publication?

*Statement:* who performs the filesystem write of §3.2 step 5.

- **Option A — the daemon publishes to both scopes.** **Cost:** the daemon
  becomes a writer of the skill roots; the manifest's `owner` changes.
- **Option B — the daemon publishes project scope only; the TUI keeps the
  global scope.** **Cost:** two owners for one workflow, which is what
  §4.4 forbids.
- **Option C — the TUI keeps publication.** **Cost:** the desktop cannot
  publish a skill; a recorded product limitation.

**Recommended: Option A.** The global root
(`<config_dir>/codegg/skills`, `src/skills/publish.rs:424`) is outside every
checkout, so a frontend-owned write there is a frontend-owned durable write
outside the workspace — the thing §4.4 of the long-term specification
forbids.

**Consequence.** Option A: work package **C** moves publication.
Option C: the milestone's closure record must record the desktop's
limitation explicitly, as an honest notice.

#### D5 — What mediates a daemon-executed skill publication?

*Statement:* the authority that governs the checkout-scoped write, and
therefore what `module.skills_publish`'s `mediation` says.

- **Option A — keep `shared-sidecar` / `mediation = "none"`.** **Cost:** the
  manifest's race note stays true and the write stays unmediated.
- **Option B — route through the writer permit**
  (`crates/codegg-core/src/writer_permit.rs:652`). **Cost:** the publication
  takes a permit, and its lifetime becomes the permit's.
- **Option C — route through the C004 checkout reservation gate**, with a
  `CheckoutWriterSink` variant, a gate call, and a manifest root claiming the
  marker string. **Cost:** one sink variant, one gate call, one manifest row,
  and the guard must fail if either half of the claim is missing.

**Recommended: Option C**, on the same reasoning plan 020 §5.3 D2 gives for
the daemon-executed `/revert`: a non-broker writer is a C004 sink, and only
that option's refusal is attributable from durable state.

**Consequence.** Option C: work package **C** adds the sink, the gate call,
the manifest row and the refusal audit. Option A: work package **C** adds
the manifest update only, and the closure record must state why the write is
unmediated.

#### D6 — Does the ask's stage-and-submit become one daemon request?

*Statement:* whether the two requests of §3.3 stay two.

- **Option A — one atomic request.** The daemon stages and admits the turn
  together. **Cost:** a new request variant and a transaction boundary
  across the session store; the client's two-call shape changes.
- **Option B — keep two, and reconcile orphan `pending` rows daemon-side.**
  **Cost:** a background reconciler and a status vocabulary for "staged but
  never submitted".
- **Option C — keep two, and let the TUI retry the submit.** **Cost:** a
  durable client-side retry list — the thing this milestone exists to
  remove.

**Recommended: Option A.** A durable workflow owner should not admit a
half-finished workflow, and the orphan row of §3.3 is a state the daemon
cannot currently explain.

**Consequence.** Option A: work package **D** replaces the two-call shape
with one. Option B: work package **D** adds the reconciler and the
vocabulary. Option C: work package **D** is dropped and the milestone's
closure record must record the seam as a known limitation.

#### D7 — Does the ask read the daemon's run bytes or the TUI-local store?

*Statement:* whether `/shell-ask` reads the TUI-local `app.shell_store`
(`src/tui/commands/shell.rs:694`) or the daemon's run read
(`src/shell/daemon.rs:389`).

- **Option A — the daemon serves the bytes.** **Cost:** the TUI stops owning
  the bytes; the desktop gains the ask.
- **Option B — the TUI keeps the bytes.** **Cost:** the desktop cannot
  perform the ask, and the read stays against a store plan 018 demoted.
- **Option C — a separate desktop path.** **Cost:** a second read path, which
  is the thing ADR-0053 forbids.

**Recommended: Option A.** It is the option that makes the ask a
daemon-owned workflow rather than a TUI-local one, and it discharges the
demotion plan 018 recorded.

**Consequence.** Option A: work package **E** re-points the read and the
desktop consumes it. Option B: the closure record must record the desktop's
limitation.

## 6. Required production changes

### 6.1 The per-move table

| Move | Mechanism | Owner of the workflow's state | Authority it runs under | `docs/writer-disposition.toml` change | Invariant it must not break |
|---|---|---|---|---|---|
| **`/diff`** | **Reclassification** — the catalog form's `kind` → `FrontendEffect` (D1 Option A) | **None.** The diff view holds no workflow state; the daemon owns nothing here | The **declared frontend-action set** (`crates/codegg-core/src/frontend_action.rs:37-73`), disposition `NonMutating` | **None.** `/diff` is not a writer surface; `src/tui/app/mod.rs` is in neither `WRITER_SURFACE_DIRS` (`scripts/check_writer_disposition.py:177-192`), nor `WRITER_SURFACE_FILES` (`:195-201`), nor `PROCESS_SURFACE_FILES` (`:212-223`) | The frontend **never** executes a daemon-effect form directly (`architecture/command.md:450-454`); the diff read stays read-only and creates **no** `Run` (`architecture/command.md:12-20`) |
| **`/skill-promote`** | **Daemon-owned durable workflow** | **One** owner of the promotion store — the TUI process (`src/tui/app/mod.rs:6945-6955`, `src/tui/commands/memory.rs:779-862`) and the daemon-side tool (`src/tool/skill_proposal.rs:117-128`) must resolve to **exactly one** (D2) | The daemon's session storage for the durable record; the catalog's `command.execute` for the human steps; the writer permit or the C004 gate for the publication (D5) | `module.skills_promotion` (`docs/writer-disposition.toml:714-725`) and `module.skills_publish` (`:727-738`) **stay**; `owner`, `mediation` and `locator` change under a single owner. `locator` today reads "project-root, under the project-local .codegg state" while the **global** scope writes to `<config_dir>/codegg/skills` (`src/skills/publish.rs:424`), outside every checkout | **Exactly one writer.** Both roots are fixed names every actor writes, so two concurrent publications race on the same directory (`:725`, `:738`) |
| **`/shell-ask`** | **Daemon-owned durable workflow** | The **daemon** — it already owns the durable record (`src/core/daemon_turns.rs:2117-2119`) and the answering turn (`:742`, `:752`) | The daemon's session store; the two requests already exist (`CoreRequest::HumanShellPromotionStage`, `CoreRequest::SessionPromptSubmit`) | **None.** The ask performs no filesystem writer effect; it writes to the session database | The frontend **never** reads a store the daemon has demoted to a projection — plan 018 moved `/shell-list` to the daemon's published list (`018-the-daemon-s-published-projection-surfaces.md:245-249`), but `/shell-ask` still reads the TUI-local `app.shell_store` (`src/tui/commands/shell.rs:694`) |

### 6.2 The diff view's reclassification

Under D1 Option A, one command descriptor changes in three fields and one
`FrontendAction` is added — M007-D's shape exactly
(`plans/closure/desktop-frontend-ide-foundation/017-status.md:103-117`):

- the descriptor's `kind` → `CommandKind::FrontendEffect`;
- the descriptor's `capability` → `None` (a command with no daemon-effect
  form names no capability, 
</task_output
