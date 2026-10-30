# Desktop Frontend and IDE Foundation Milestone 022 — The Two Subsystem Moves: A Durable Workflow Owner for Skill Promotion and the Human-Shell Ask

Status: ready for handoff — ADR-0058 accepted on 2026-10-30 (Option B, all three
moves); the decision is recorded in §5 and the milestone's implementation is
`plans/closure/desktop-frontend-ide-foundation/022-status.md`

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

- **ADR-0058 — accepted Option B (all three moves) on 2026-10-30**
  (`plans/adrs/ADR-0058-durable-workflow-ownership.md`). It reserves the
  ownership decision for the two workflows **and** decides that the git-review
  diff view **moves** as well, so that both frontends see the same diff and the
  diff's bytes are read under the daemon's authority rather than the
  frontend's. The record had recommended the other option, on the measurement
  that `/diff` issues no daemon request; that measurement stands and is
  recorded in §3.1, and the decision is the owner's.
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
workflow owner, and the **diff view** (`/diff`) **moves to the daemon** as the
third move, under accepted ADR-0058 Option B.

The milestone's deliverable is the **ownership**, not the surface area. It is
complete when the promotion store has **exactly one** writer, when the ask's
two requests and the record between them are owned end to end by the daemon,
when the catalog stops declaring daemon effects the daemon does not serve,
and when the manifest says who may write each skill root.

**`/diff` moves, even though it measures as a single synchronous local
effect.** §3.1 measures that shape and the measurement stands; §6.2 moves it
anyway, because the accepted decision is that both frontends see the same diff
and the diff's bytes are read under the daemon's authority. The cost is a
canonical `Run` per diff — a durable audit record for a read — and that cost is
recorded rather than hidden.

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
- **The dependency has landed.** ADR-0058 was accepted Option B on 2026-10-30,
  and §5 now carries a `**Resolution:**` line on each of this plan's seven
  decision points. **No work package below silently picks one.**

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
   The invariant still holds — but it no longer reaches the diff view, which
   under D1 Option B is a **daemon** effect and therefore carries a canonical
   `Run`. What the invariant forbids is a *frontend* effect growing one.
3. **The diff read stays read-only.** No checkout mutation, no index write.
   `GIT_OPTIONAL_LOCKS=0` must remain in force under the private object view
   (`crates/egggit/src/process.rs:459-462`). The move changes **who** reads, not
   **what** the read does.
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

- The **diff view's move** to the daemon: a new request variant, an
  effect-table row, a canonical `Run`, the daemon's ownership of the `git`
  reads, and the removal of the TUI's bespoke arm.
- The **skill-promotion workflow's single owner**, its identity key, and its
  publication authority.
- The **human-shell ask's** request shape, its durable record's owner, and
  the store it reads.
- The **manifest update** for `module.skills_promotion` and
  `module.skills_publish`.
- The **generated classification artifact's** M007-I row, if the milestone
  changes any measured figure.

### 5.2 Explicitly out of scope (non-goals and stop conditions)

- **A diff view that mutates.** The move is a read. A mutating git surface is
  a different milestone.
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

### 5.3 Decision points — resolved

**Seven decision points were put to the project maintainer and answered on
2026-10-30**, by the acceptance of **ADR-0058 Option B** (all three moves).
Each subsection below now carries a `**Resolution:**` line stating the choice,
the date and the reason, and the **Consequence** line under it states the shape
the milestone now takes. **The implementer executes the resolutions, not the
recommendations.** The options are retained unchanged as the record of what was
asked.

**Only D1 differs from this plan's own recommendation**, and it differs because
the owner chose against it: the record recommended reclassifying `/diff`
precisely because §3.1 measures it as issuing no daemon request, and the
accepted decision moves it anyway. The measurement stands and is recorded in
§3.1; the decision is the owner's.

#### D1 — Does the diff view move to the daemon at all?

**Resolution:** **It moves — the owner's Option B, which is this plan's Option B
("make it a daemon effect").** `/diff` becomes a daemon effect: its catalog form
gains a daemon route, the daemon owns the `git` reads it performs, and every
diff becomes a canonical `Run`. **Resolved by the project maintainer on
2026-10-30.** The reason: moving the diff view gives both frontends one
authority for it, and the diff's bytes are read under the daemon's authority
rather than the frontend's. **The measurement that `/diff` issues no daemon
request, holds no workflow state and performs no writer effect stands** — §3.1
records it, and the corrected classification records it — and the cost of the
decision is a durable `Run` per diff, which is a read the audit trail now
records.

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

**Consequence (resolved).** Work package **A** is the daemon move, not a
reclassification: a new request variant, an effect-table row and a canonical
`Run` path. The milestone's diff-view item therefore carries a daemon surface
it would not otherwise have carried, and the **desktop's honest notice** for
the diff view is replaced by the desktop consuming the daemon's answer.

#### D2 — Which single owner writes the promotion store?

**Resolution:** **The daemon's session storage — the owner's Option A, which is
this plan's Option A.** The store follows the `HumanShellPromotionStore`
precedent (`crates/codegg-core/src/session/human_shell_promotion.rs:169`); the
TUI gains request variants and holds no durable state. **Resolved by the project
maintainer on 2026-10-30.** The reason: after this milestone the store has
**one** owner, and the decision fixes which — the daemon.

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

**Consequence (resolved).** Work package **B** moves the store and adds the
request variants; work package **C** re-points the writers. The "durable
workflow owner" **is** the daemon, so no closure finding is owed on that point.

#### D3 — What identity keys the promotion store?

**Resolution:** **The canonical `WorkspaceId` — the owner's Option A, which is
this plan's Option A.** **Resolved by the project maintainer on 2026-10-30.**
The reason: the store's namespace is a SHA-256 of a path string derived
differently on each side (§3.2), and only a canonical identity removes the
divergence that makes the daemon's `submit` fail with "promotion request not
found". A namespace migration follows, and in-flight proposals must migrate or
fail loudly.

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

**Consequence (resolved).** Work package **B** migrates the namespace. The
identity is a canonical `WorkspaceId`, **not** a path, so the closure record
owes no "the identity remains a path" finding — and the option that would have
owed it (Option C, a documented-but-unenforced invariant) is not taken.

#### D4 — Does the daemon publish skills, or does the TUI keep publication?

**Resolution:** **The daemon publishes to both scopes — the owner's Option A,
which is this plan's Option A.** **Resolved by the project maintainer on
2026-10-30.** The reason: the global root (`<config_dir>/codegg/skills`,
`src/skills/publish.rs:424`) is outside every checkout, so a frontend-owned
write there is a frontend-owned durable write outside the workspace — the thing
§4.4 of the long-term specification forbids. Two owners for one workflow is
what §4.4 also forbids, so the split of Option B is not available either.

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

**Consequence (resolved).** Work package **C** moves publication. The desktop
gains publication, so **no** desktop limitation is owed on this point.

#### D5 — What mediates a daemon-executed skill publication?

**Resolution:** **The C004 checkout reservation gate — the owner's Option C,
which is this plan's Option C.** A `CheckoutWriterSink` variant, a gate call, a
manifest root claiming the marker string, and the guard must fail if either
half of the claim is missing. **Resolved by the project maintainer on
2026-10-30.** The reason: a non-broker writer is a C004 sink, and only that
option's refusal is attributable from durable state.

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

**Consequence (resolved).** Work package **C** adds the sink, the gate call,
the manifest row and the refusal audit. The write is **mediated**, so the
closure record owes no "the write is unmediated" finding.

**Note the contrast with M007-H.** That milestone's `/revert` takes the **C003
writer fence** and adds **no** sink, because a typed git mutation under the
model's fence already refuses before its first effect. This milestone's
publication is a **non-broker writer**, which C004 mediates, so it takes the
gate and the sink. The two are different writer classes and the difference is
deliberate, not an inconsistency.

#### D6 — Does the ask's stage-and-submit become one daemon request?

**Resolution:** **One atomic request — the owner's Option A, which is this
plan's Option A.** The daemon stages and admits the turn together. **Resolved
by the project maintainer on 2026-10-30.** The reason: a durable workflow owner
should not admit a half-finished workflow, and the orphan `pending` row of §3.3
is a state the daemon cannot currently explain.

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

**Consequence (resolved).** Work package **D** replaces the two-call shape
with one. The client's two-call shape changes, and the seam of §3.3 is closed
rather than reconciled or retried.

#### D7 — Does the ask read the daemon's run bytes or the TUI-local store?

**Resolution:** **The daemon serves the bytes — the owner's Option A, which is
this plan's Option A.** **Resolved by the project maintainer on 2026-10-30.**
The reason: it is the option that makes the ask a daemon-owned workflow rather
than a TUI-local one, and it discharges the demotion plan 018 recorded.

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

**Consequence (resolved).** Work package **E** re-points the read and the
desktop consumes it. The desktop therefore **gains** the ask, so no desktop
limitation is owed on this point.

## 6. Required production changes

### 6.1 The per-move table

| Move | Mechanism | Owner of the workflow's state | Authority it runs under | `docs/writer-disposition.toml` change | Invariant it must not break |
|---|---|---|---|---|---|
| **`/diff`** | **Daemon effect** — a new `CoreRequest` variant, an effect-table row in `build_inner_request` (`src/core/daemon_commands.rs:185`) and a canonical `Run` per invocation (D1 Option B) | **None.** The diff view holds no workflow state; the **daemon** now owns the `git` reads it performs | The daemon's own read-only `egggit` facts crate (`crates/egggit/src/process.rs:445`), under the canonical `Run` | **None.** `/diff` is not a writer surface: `src/tui/app/mod.rs` is in neither `WRITER_SURFACE_DIRS` (`scripts/check_writer_disposition.py:177-192`), nor `WRITER_SURFACE_FILES` (`:195-201`), nor `PROCESS_SURFACE_FILES` (`:212-223`), and the daemon's read is read-only | The frontend **never** executes a daemon-effect form directly (`architecture/command.md:450-454`); the diff read stays read-only; and it now creates a canonical `Run` (`architecture/command.md:12-20`) — the cost the accepted decision accepts |
| **`/skill-promote`** | **Daemon-owned durable workflow** | **One** owner of the promotion store — the TUI process (`src/tui/app/mod.rs:6945-6955`, `src/tui/commands/memory.rs:779-862`) and the daemon-side tool (`src/tool/skill_proposal.rs:117-128`) must resolve to **exactly one** (D2) | The daemon's session storage for the durable record; the catalog's `command.execute` for the human steps; the writer permit or the C004 gate for the publication (D5) | `module.skills_promotion` (`docs/writer-disposition.toml:714-725`) and `module.skills_publish` (`:727-738`) **stay**; `owner`, `mediation` and `locator` change under a single owner. `locator` today reads "project-root, under the project-local .codegg state" while the **global** scope writes to `<config_dir>/codegg/skills` (`src/skills/publish.rs:424`), outside every checkout | **Exactly one writer.** Both roots are fixed names every actor writes, so two concurrent publications race on the same directory (`:725`, `:738`) |
| **`/shell-ask`** | **Daemon-owned durable workflow** | The **daemon** — it already owns the durable record (`src/core/daemon_turns.rs:2117-2119`) and the answering turn (`:742`, `:752`) | The daemon's session store; the two requests already exist (`CoreRequest::HumanShellPromotionStage`, `CoreRequest::SessionPromptSubmit`) | **None.** The ask performs no filesystem writer effect; it writes to the session database | The frontend **never** reads a store the daemon has demoted to a projection — plan 018 moved `/shell-list` to the daemon's published list (`018-the-daemon-s-published-projection-surfaces.md:245-249`), but `/shell-ask` still reads the TUI-local `app.shell_store` (`src/tui/commands/shell.rs:694`) |

### 6.2 The diff view's move

Under D1 Option B, `/diff` becomes a **daemon effect**. Its catalog form keeps
`kind: DaemonEffect` and its `capability` (`CommandCapability::CommandExecute`,
`crates/codegg-core/src/command_catalog.rs:1499-1515`) — the catalog is
**right** today and the daemon is the part that is missing, which is the
opposite of the kind correction M007-D made elsewhere
(`plans/closure/desktop-frontend-ide-foundation/017-status.md:103-117`). What
changes:

- a **new `CoreRequest` variant** carries the diff request, with the path as
  its single argument. The **empty** argument is the bare `/diff` branch.
- an **effect-table row** in `build_inner_request` (`src/core/daemon_commands.rs:185`)
  maps the form to that variant. The row's inner request must be the scope plus
  at most one argument — the path — or the form does not fit the mechanical
  shape the other rows follow.
- a **canonical `Run`** records each invocation, so the diff is an audited
  effect rather than an unrecorded read. This is the cost §6.1's row records:
  the accepted decision puts a durable audit record on a read.
- the **daemon performs the git reads**, under the read-only facts crate
  (`crates/egggit/src/process.rs:445`, re-exported at `src/git_mutations.rs:47`),
  which is the crate the current TUI path already uses (`src/tui/app/mod.rs:11287-11295`,
  `:11312-11316`). The move changes **who** reads, **not** how: the two
  branches stay mutually exclusive, the object-reading allowlist
  (`crates/egggit/src/process.rs:1434`, `:1445`) and `GIT_OPTIONAL_LOCKS=0`
  (`:459-462`) are unchanged, and the typed mutating crate `crates/codegg-git`
  is still not involved.
- the **TUI's arm is removed**. The frontend never executes a daemon-effect
  form directly (`architecture/command.md:450-454`), so the bespoke
  `handle_diff_command` path (`src/tui/app/mod.rs:11268-11347`) is replaced by
  the daemon request and the daemon's answer is rendered. No fallback arm is
  kept, for the reason M007-H's D8 gives: a fallback that reports a stale
  answer is not a fallback but a lie.

The **desktop gains the diff view**, which it did not have while the form was
misdeclared as a daemon effect the daemon did not serve.

### 6.3 The skill-promotion move

The workflow of §3.2 is five steps with two writers. Under D2, D3, D4 and D5 it
becomes one workflow with one owner.

1. **The store moves to the daemon's session storage** (D2), following the
   `HumanShellPromotionStore` precedent
   (`crates/codegg-core/src/session/human_shell_promotion.rs:169`). The TUI
   gains request variants and holds no durable state. The TUI's steps 1, 4 and
   5 and the daemon tool's step 3 become requests against the daemon's store.
2. **The namespace key becomes the canonical `WorkspaceId`** (D3). The
   SHA-256-of-a-path-string namespace
   (`crates/codegg-core/src/memory/mod.rs:36-41`), derived differently on each
   side (`src/tui/app/mod.rs:6948` vs `src/tool/skill_proposal.rs:112`), is
   replaced. The divergence that produces the daemon's "promotion request not
   found" failure (`src/skills/promotion.rs:481`) is closed by construction.
   In-flight proposals **migrate or fail loudly** — they are not silently
   dropped.
3. **The daemon publishes to both scopes** (D4). The publication's filesystem
   effects — `ensure_directory` (`src/skills/publish.rs:579-602`), the fsynced
   temp file (`:299-306`), the atomic no-replace `hard_link` commit
   (`:332-337`) and the directory sync (`:345`, `:746-749`) — move from
   `src/tui/commands/memory.rs:779-862` to a daemon path, for both the
   project scope (`<project>/.codegg/skills/<name>/SKILL.md`) and the global
   scope (`<config_dir>/codegg/skills/<name>/SKILL.md`,
   `src/skills/publish.rs:412-434`, `:552-566`).
4. **The publication is mediated by the C004 checkout reservation gate** (D5).
   A new `CheckoutWriterSink` variant
   (`src/core/checkout_mediation.rs:41-70`), a gate call in the same place the
   existing sinks are gated (`src/core/daemon.rs:1121-1122`), and a manifest
   root claiming the marker string. The guard fails on **both** directions if
   either half is missing (`scripts/check_writer_disposition.py:714-737`): a
   declared sink no root claims, and a claimed marker no sink declares.

The manifest's two rows **stay** — `module.skills_promotion`
(`docs/writer-disposition.toml:714-725`) and `module.skills_publish`
(`:727-738`) — and their `owner`, `mediation` and `locator` change under the
single owner. `locator` today reads "project-root, under the project-local
.codegg state" while the **global** scope writes outside every checkout
(`src/skills/publish.rs:424`), so the row is corrected rather than deleted.

### 6.4 The ask's move

The ask of §3.3 is two requests and a durable record between them.

1. **Stage-and-submit becomes one request** (D6). `CoreRequest::HumanShellPromotionStage`
   (`src/tui/commands/shell.rs:764-772`) and the `human_shell_promotion_ids`
   payload on `CoreRequest::SessionPromptSubmit` (`:773-781`) become a single
   variant that stages the promotions and admits the turn together, under the
   daemon's transaction boundary. The seam of §3.3 — a `pending` row that
   survives a failed second request (`:834-845`) — is closed rather than
   reconciled or retried.
2. **The evidence comes from the daemon's run bytes** (D7). The read moves off
   the TUI-local `app.shell_store` (`src/tui/commands/shell.rs:694`) to the
   daemon's own run read (`src/shell/daemon.rs:389`), which the daemon already
   owns (`src/core/daemon.rs:81`). The redaction of the assembled evidence
   (`src/tui/commands/shell.rs:915`) moves with it. This discharges the
   demotion plan 018 recorded
   (`plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md:245-249`).

The ask performs **no** filesystem writer effect, so `docs/writer-disposition.toml`
grows **no** row for it.

### 6.5 The desktop

The desktop consumes all three moves, and for the first time in this milestone
it performs work the TUI cannot:

- the **diff view** — the desktop's honest notice is replaced by the daemon's
  answer;
- **skill promotion** — the desktop can begin a request, preview it and publish
  it, because the workflow is daemon-owned;
- **the ask** — the desktop can perform it, because the bytes are the daemon's.

The desktop's frontend-action set is closed over the catalog
(`scripts/check_frontend_action_set.py`), so any new declared action is
recorded in the guard's declared set. The desktop is a **separate Cargo
workspace** (`scripts/check-desktop-boundary.sh`); it is built with
`rustup run 1.90.0`.

## 7. Ordered work packages

Work packages are ordered by dependency. **No package below waits on an
answer** — each names the resolution it implements.

### Work package A — The diff view's move

The new request variant, the effect-table row, the daemon handler, the
authorization decision, the audit mapping, the canonical `Run`, and the TUI's
arm removal. §6.2.

**Depends on:** D1 (resolved as **Option B**, the move) and the D8-shaped
rule that a served form's arm is removed rather than kept as a fallback.

### Work package B — The promotion store moves and its identity changes

The store's relocation to the daemon's session storage (D2), the namespace
migration to the canonical `WorkspaceId` (D3), the request variants the TUI
needs, and the re-pointing of both writers — the TUI's steps 1, 4 and 5 and the
daemon tool's step 3 (§3.2).

**Depends on:** D2 and D3, both resolved as **Option A**.

### Work package C — Publication moves and is mediated

The publication's move to the daemon (D4), the C004 gate and its
`CheckoutWriterSink` variant (D5), the manifest rows' `owner`, `mediation` and
`locator` updates, and the refusal audit.

**Depends on:** D4 (Option A) and D5 (Option C).

### Work package D — The ask's atomic request

The single stage-and-submit request (D6), its transaction boundary, and the
removal of the orphan-`pending` seam.

**Depends on:** D6, resolved as **Option A**.

### Work package E — The ask's read

The evidence read moves to the daemon's run bytes (D7), with the redaction
moving with it.

**Depends on:** D7, resolved as **Option A**.

### Work package F — The desktop consumes all three

The desktop's frontend actions for the diff view, the promotion workflow and
the ask, and the guards' declared-set update.

**Depends on:** A, B, C, D and E.

### Work package G — The generated artifact and the guards

The M007-D classification artifact is **regenerated** by its own generator
(`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`),
never hand-edited. The generator's `--check` and `--self-test` both run.

**Depends on:** A.

## 8. Failure, cancellation, restart and contention semantics

1. **The diff read fails.** The daemon returns the failure in the canonical
   `Run`'s response; the TUI renders it. The read is read-only, so a failure
   leaves the working tree untouched.
2. **A promotion publication is refused by the C004 gate.** The refusal is
   durable and attributable (`crates/codegg-core/src/audit.rs:175`) and names
   the checkout's reservation state. The promotion record is **not** marked
   published; it stays in the state it was in, so the human can act on it.
3. **The ask's atomic request fails.** Either the promotions are staged and the
   turn is admitted, or neither is. There is no state in which the promotions
   are staged and the turn was not admitted — that is the whole point of D6.
4. **The namespace migration meets an in-flight proposal.** The proposal
   **migrates or fails loudly**. A proposal that cannot be migrated is
   reported, not silently dropped.
5. **Two publications race.** The C004 gate serializes them: the second is
   refused rather than racing on the same fixed directory name
   (`docs/writer-disposition.toml:725`, `:738`).

## 9. Compatibility and migration

1. **The catalog's `/diff` descriptor does not change.** It is already a
   `DaemonEffect` with `command.execute`; this milestone makes the daemon
   serve it. The catalog's **version** moves by the number of forms whose
   `kind` changes — which for this milestone is **zero**, unlike M007-G's
   three.
2. **The promotion store's on-disk layout changes.** The store moves from
   `dirs::config_dir()/codegg/memory/skill-promotions`
   (`src/skills/promotion.rs:231-243`) to the daemon's session storage, and its
   namespace changes from a path digest to a canonical `WorkspaceId`. Existing
   records migrate or fail loudly.
3. **The `flock` sidecar protocol is retired.** The two writers become one
   (`src/skills/promotion.rs:629-647`), so the coordination protocol is no
   longer needed. The plan does **not** keep it "just in case": two owners of
   one workflow is the defect.
4. **The TUI keeps its standalone mode.** Where no daemon is attached, the TUI
   still runs the workflow against its own store — the same shape M007-G
   established for the plugin registry.
5. **The desktop is not migrated by this milestone's own code.** It consumes
   the moved workflows through work package F; the desktop's separate Cargo
   workspace is built with `rustup run 1.90.0`.

## 10. Required tests

Every test body must assert a checkable property, delegate to a named helper
that does, or be recorded in `docs/test-assertion-quality.toml` with the reason
it asserts nothing (`scripts/check_test_assertion_quality.py`). No two bodies
may assert the same thing twice unless `docs/test-duplication.toml` records the
survivor (`scripts/check_test_duplication.py`).

1. The daemon's diff request returns the same bytes the TUI's read returned
   for both branches — `/diff <path>` and bare `/diff`.
2. Each diff invocation creates a canonical `Run` that can be read back from
   the durable run store.
3. The TUI's diff arm is gone: the catalog form is daemon-served and the
   frontend no longer constructs the read.
4. The promotion store has **exactly one** writer after the move: a test that
   drives the TUI path and the daemon-tool path and asserts the record is
   written once, in one place.
5. The namespace is keyed on the canonical `WorkspaceId`: a test that stages a
   promotion through the TUI and submits it through the daemon tool **succeeds**
   — the §3.2 divergence that produced "promotion request not found" is closed.
6. A proposal that cannot be migrated **fails loudly** rather than vanishing.
7. The daemon publishes to **both** scopes, and the global scope's write
   carries the daemon as `owner` in the manifest.
8. The C004 gate **refuses** a publication into a reserved checkout, and the
   refusal is durable and attributable.
9. The sink enum and the manifest agree in **both** directions: the guard's
   self-test still rejects an injected defect.
10. The ask's single request leaves **no** orphan `pending` row when the turn
    is not admitted, and admits the turn with its promotions when it is.
11. The ask's evidence read comes from the daemon's run bytes, and the
    redaction still applies.

## 11. Required verification commands

Run each, or record its omission with the reason:

```bash
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
cargo fmt --check --all
cargo test -p codegg-core
cargo test -p codegg-protocol
cargo test -p codegg --lib <module>
cargo test --test core_daemon -- <member>
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_frontend_action_set.py
python3 scripts/check_writer_disposition.py
python3 scripts/check_writer_disposition.py --self-test
python3 scripts/check_adr_integrity.py
python3 scripts/check_comment_budget.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_desktop_panel_authority.py
bash scripts/check-desktop-boundary.sh
python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check
python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --self-test
scripts/verify.sh quick
```

**`cargo test --test scheduler_jobs` is excluded outright** under M008 §16,
which bars running that family against a developer's real scratch root. It is
reported as **not run** with that reason.

**`scripts/verify.sh full`** (clippy `-D warnings` and the nextest workspace
sweep) is a known gap if it is not run, and the closure record says so.

## 12. Documentation updates

- `architecture/overview.md` — the counts the ownership table reports.
- `architecture/command.md` — the diff view's ownership, once it moves.
- `architecture/human_shell.md` — the ask's single request and the daemon's
  ownership of the evidence bytes.
- `architecture/writer_permit.md` — the new `CheckoutWriterSink` variant and
  the publication's mediation.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — the M007-I
  row.
- `plans/registry.md` — the M007-I bullet and a `## Source verification`
  entry.

## 13. Acceptance criteria

Each criterion names a checkable property. A criterion satisfied by a comment
is not satisfied.

1. `/diff` is served by the daemon, its catalog form is `daemon_served`, and
   the frontend no longer constructs the read itself.
2. Each diff invocation creates a canonical `Run` that is readable back from
   the durable run store.
3. The daemon's diff answer carries the same bytes the TUI's read produced, for
   both the `/diff <path>` branch and the bare `/diff` branch.
4. The promotion store has exactly **one** writer, and it is the daemon.
5. The promotion namespace is keyed on the canonical `WorkspaceId`, and a
   promotion staged through the TUI is submitted successfully by the daemon
   tool.
6. The daemon publishes a skill to **both** the project scope and the global
   scope, and the manifest's `owner` names the daemon for both roots.
7. A daemon-executed publication is **refused** by the C004 gate when the
   checkout is reserved by a live run, and the refusal is durable and
   attributable.
8. The sink enum and the manifest agree in both directions: a declared sink no
   root claims fails, and a claimed marker no sink declares fails.
9. The ask's stage-and-submit is **one** request, and a failed admission
   leaves **no** orphan `pending` row.
10. The ask's evidence comes from the daemon's run bytes, and the redaction
    still applies.
11. The frontend never executes a daemon-effect form directly, and no form's
    `kind` changed except through the daemon serving it — the catalog's
    **version** moves by **zero** for this milestone.
12. The generated artifact is regenerated by its own generator and its §1
    counts are derived from its §3 rows; no count in the artifact or in any
    prose about it is hand-written.
13. Every command in §11 is run, or its omission is recorded with the reason —
    in particular the `scheduler_jobs` family, which is excluded outright under
    M008 §16 and is reported as **not run**.

## 14. Stop conditions

1. **A second owner of the promotion store appears.** The milestone's premise
   is one owner; two owners is the defect it exists to fix. The work stops.
2. **The writer-disposition guard cannot be satisfied by a manifest row, or a
   sink and a marker disagree in either direction.** The work stops; the
   guard is **not** edited.
3. **The namespace migration would silently drop an in-flight proposal.** The
   work stops until the proposal fails loudly or migrates.
4. **The diff view would need a writer effect.** The move is a read; a
   mutation is a different milestone.
5. **The generator's parallel correction is not landed** when the artifact is
   regenerated. The regeneration waits; the artifact is never hand-edited.

## 15. Closure evidence required

- The **implementation commit**, and the baseline it was taken against.
- A **requirement-to-evidence matrix** over §13's thirteen criteria, each row
  naming a `path:line` anchor or a named test.
- **Every §11 command**, with its outcome: passed, failed, or **not run with
  the reason**. The `scheduler_jobs` family is reported as not run, with
  M008 §16 as the reason.
- The **guard runs**, with their exact output — including
  `check_writer_disposition.py`'s "N declared roots, N writer tools, N writer
  modules" line **before and after** the manifest changes.
- The **writer-disposition manifest's before and after**: the two changed rows
  in full, and the sink enum's before and after.
- The **generated artifact's regenerated §1 table**, with the note that every
  count in it is derived from the §3 rows by the generator.
- The **known limitations, stated as limitations**.
- **Unresolved findings by severity.**
- A **recommendation**: closed, conditionally closed, corrective pass required,
  or blocked.

## 16. Handoff notes

- The **surveys behind this plan** are §3's four evidence sections. Every
  anchor was re-verified against the tree at the baseline in the header.
- **The seven decisions in §5.1 have landed.** All seven were answered by the
  project maintainer on **2026-10-30**, by the acceptance of **ADR-0058 Option
  B**, and each subsection carries a `**Resolution:**` line with the choice,
  the date and the reason. **The implementer executes the resolutions, not the
  recommendations** — the options and the recommendations are retained as the
  record of what was asked.
- **Only D1 differs from this plan's own recommendation**, and it differs
  because the owner chose against it: the diff view moves, at the cost of a
  canonical `Run` per read. The measurement that `/diff` issues no daemon
  request stands, and is recorded in §3.1.
- **The plan was completed after the decision landed.** It was written against
  ADR-0058 while it was still `proposed`, and the sections that depended on
  the decision — §5's resolutions, §6.1's `/diff` row, §6.2, §7, §8, §9, §10,
  §11, §12, §13, §14, §15 and §16 — were written against the accepted
  decision. A reader holding the pre-acceptance text should take this section
  and §5.1 as the record of what changed and why.
