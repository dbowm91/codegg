# Desktop Frontend and IDE Foundation Milestone 020 — The Daemon's Missing Command Surfaces

Status: ready for handoff

Repository baseline: `d4c7cf2d7` — the merged desktop GUI parity line through
**M007-E**, M007-D and its corrective pass (**plan 017**), the projection
milestone (**plan 018**), the reconciliation pass that closed M007-F under
accepted ADR-0056, the landed M007-J terminal surface (**plan 019**), and the
owner's acceptance of ADR-0057 and ADR-0058. **All eight owner decisions in
§5.3 were answered on 2026-10-30** and are recorded as `**Resolution:**` lines
in that section; each work package that depended on one has been rewritten
against the resolved shape. The one remaining external gate is **ADR-0059's
implementation** (`plans/adrs/ADR-0059-research-runtime-ownership.md`), for the
research item only. Every `path:line` anchor below was re-verified against this
baseline; a rebase that moves them must re-derive them.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:558` — the M007
  decomposition table
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:569` — the
  M007-H milestone row

Planning basis:

- The M007-H milestone row, verbatim: *"The commands the daemon does not yet
  serve — compaction, subagent steering, the plan list, the test suite, a git
  revert, the research turn, the security-review and change-review workflows,
  opening a document, re-running a human-shell run, and a log tail: **13**
  catalog forms across **11** items."* The row also records: *"**No ADR** —
  each item extends a request family the daemon already has, **except**
  compaction, which has **no** request family to extend and whose TUI side is
  already owned by the registered TUI compact command corrective"*, and names
  two owner decisions that come first — the compaction overlap and the writer
  disposition a daemon-executed git revert needs.
- The **generated** classification artifact
  `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`
  and its generator
  `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`.
  **This artifact is generated and is being corrected in parallel.** Its
  per-row numbers may move; this plan therefore cites the generator's rows and
  names the source of every count it states, and it never copies the
  artifact's current counts as if they were stable.
- The registered TUI compact command corrective,
  `plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md`
  (status ready for handoff; reconciled 2026-10-29 against this baseline).
- The four confirmed defects §3.3 records.

Applicable ADRs:

- **One ADR is created by this milestone's decision record, not by this plan:
  ADR-0059** (`plans/adrs/ADR-0059-research-runtime-ownership.md`), *accepted
  — Option B (the runtime moves to the daemon), 2026-10-30*. It decides the
  research runtime's ownership (§3.4, D5). This plan **defers** the
  `/research` item to that ADR's implementation and does not implement the
  research move itself.
- This milestone is bound by **ADR-0045** (command syntax and execution
  authority), **ADR-0052** (cross-frontend command execution ownership),
  **ADR-0053** (the daemon's published projection surfaces), **ADR-0056** (the
  daemon's LSP command and runtime control surface), **ADR-0041** (subagent run
  addressing and human interaction authority), **ADR-0036** (mid-turn prompt
  steering delivery contract), **ADR-0008** (LSP preview apply authorization
  and runtime ownership), **ADR-0011** (editor and document ownership and
  frontend replication) and **ADR-0017** (human-shell promotion admission),
  and it revisits none of their conclusions.

Long-term requirements:

- `plans/000-long-term-specification.md:95` §4.4 — a frontend renders
  projections and does not own durable state. Every surface this milestone
  moves to the daemon stops being frontend-owned state: the plan list, the
  test state, the research run, the security-review result and the log tail
  become daemon answers.
- `plans/002-long-term-roadmap.md:900` — *"When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap."* **This milestone
  changes no terminology.** It implements surfaces the roadmap already placed
  inside the daemon's ownership.
- `plans/001-terminology-and-domain-model.md` — the canonical vocabulary is
  unchanged. A **command form** is an argument shape in the daemon's catalog;
  a **daemon effect** is executed by the daemon; a **frontend effect** is
  executed locally against the declared action set. This milestone moves
  forms between those two worlds and does not invent a third.

## 1. Objective

The daemon gains the **command forms** it does not yet serve, so that a frontend
asking for one of them receives the daemon's authoritative answer instead of a
local approximation. **Two of the thirteen do not land as a new daemon
surface**: `/research` is deferred to ADR-0059's implementation (D5), and
`/compact` stays frontend-owned under the registered compact corrective with a
truthful report (D1) — the milestone's only share of it is the catalog's kind
correction.

The thirteen forms are the ones the generated inventory names as M007-H's
surface. The generator names them explicitly, one per `_missing(...)` entry —
the first is `_missing("/compact", "M007-H — the daemon's missing
command surfaces", …)` and the last is `_missing("/logs", "M007-H — the
daemon's missing command surfaces", …)` in
`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py` (`:1410` and `:1473` against this baseline); the thirteenth,
`/security-review-show`, is the `_missing("/security-review-show", "M007-H —
the daemon's missing command surfaces", …)` entry in the same file
(`:1450`) — *"the arm shows the TUI's cached review result, and the daemon has
no request that reads one — the same missing surface as `/security-review`
(M007-H)"*. **That is the source of
the thirteen; the number is not hand-written in this plan.**

The eleven items are the roadmap's own grouping. The decomposition is:

| item | forms | count |
|---|---|---|
| compaction | `/compact` | 1 |
| subagent steering | `/subagent-send` | 1 |
| the plan list | `/plan` | 1 |
| the test suite | `/tests` | 1 |
| a git revert | `/revert` | 1 |
| the research turn | `/research` | 1 |
| the security-review workflow | `/security-review`, `/security-review-show`, `/security-review-cancel` | 3 |
| the change-review workflow | `/review-change` | 1 |
| opening a document | `/open` | 1 |
| re-running a human-shell run | `/shell-rerun` | 1 |
| a log tail | `/logs` | 1 |
| | | **13** |

**The extra two forms are the security-review workflow's `show` and `cancel`
arms, not aliases.** All thirteen catalog descriptors carry exactly one
`CommandForm` (`form: ""`); the two aliases in play — `summarize` on
`/compact` (`crates/codegg-core/src/command_catalog.rs:589`) and
`/preview-review` on `/review-change`
(`crates/codegg-core/src/command_catalog.rs:1825`) — live in the catalog's
separate `aliases` field, which the generator parses (the
`aliases = re.findall(r'"([^"]+)"', _slice(block, "aliases:", "]"))` line in
`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`) and does **not** count as a
form. The milestone is therefore *thirteen forms across eleven items*, and the
roadmap row's figure is correct.

The milestone's deliverable is the **daemon's command surface**, not a
redesign of the TUI. It is complete when each of the thirteen forms has a
route that answers with authoritative data — the daemon's where the daemon owns
the effect, and the frontend's own honest answer where D1 and D6 left the form
frontend-owned — and when the TUI's local approximation for that form is either
gone or honest about what it does.

## 2. Why this milestone is ready

- **The catalog side is done.** Every one of the thirteen forms is already in
  the daemon's catalog with a `DaemonEffect` kind and
  `capability: Some(CommandCapability::CommandExecute)`
  (`crates/codegg-core/src/command_catalog.rs:594`, `:984`, `:1092`, `:1470`,
  `:1524`, `:1560`, `:1578`, `:1686`, `:1830`, `:2226`, `:2244`, `:2262`,
  `:2334`). Nothing about their *identity* changes; this milestone changes
  who serves them.
- **The daemon's executor is ready.** `build_inner_request`
  (`src/core/daemon_commands.rs:185`) is the table-driven dispatch from a
  catalog form to a daemon request, `serves_form`
  (`src/core/daemon_commands.rs:778`) is the serve-ability predicate, and
  `handle_command_request` (`src/core/daemon_commands.rs:805`) audits a
  command execution as a canonical `Run`. A form the daemon serves becomes
  reachable by a frontend through one request.
- **Four of the thirteen already have their specific request.** `/open` →
  `CoreRequest::DocumentOpen` (`crates/codegg-protocol/src/core.rs:2543`),
  `/subagent-send` → `CoreRequest::TurnSteer` (`crates/codegg-protocol/src/core.rs:3016`),
  `/security-review-cancel` → `CoreRequest::TurnCancel` (`crates/codegg-protocol/src/core.rs:3012`), and
  `/shell-rerun` → `CoreRequest::HumanShellRunCreate` (`crates/codegg-protocol/src/core.rs:3660`). For
  those the milestone's work is an **effect-table row and a TUI route**, not a
  protocol change. §7.1 says which.
- **The TUI's migration pattern is proven.** `send_command`
  (`src/tui/command_route.rs:218`) sends `CoreRequest::CommandExecute` with the
  scope and the argument and unwraps the daemon's inner response verbatim;
  `src/tui/runtime/command_dispatch.rs:3057` and
  `src/tui/commands/goals.rs:110` are two production call sites of that
  pattern. The milestone has a template, not a design problem.
- **The writer-disposition manifest exists for exactly this.** The guard
  (`scripts/check_writer_disposition.py`) is the mechanism that makes the git
  revert governable, and it is already wired into `scripts/verify.sh` (`:378-382`).
- **The compaction overlap is documented, not discovered.** The corrective's
  stop condition 1 names the decision this milestone must take, so the
  milestone does not have to re-derive it.

## 3. Current implementation evidence

### 3.1 The thirteen forms, as the tree stands

Catalog anchors are `crates/codegg-core/src/command_catalog.rs`; the TUI arm
is the `B::…` arm in `src/tui/app/mod.rs` that implements the form today.

| form | catalog descriptor | catalog form | TUI arm | the TUI's effect today |
|---|---|---|---|---|
| `/compact` | `:586` | `:597` | `:5989` | a toast, and nothing else |
| `/open` | `:976` | `:987` | `:6745` | `editor::open_editor` → `DocumentOpen` |
| `/subagent-send` | `:1084` | `:1095` | `:5711` | arms a composer badge; the steer goes out later |
| `/plan` | `:1462` | `:1473` | `:7316` | edits `session_state_derived.plan` |
| `/tests` | `:1516` | `:1527` | `:8266` | reads `session_state_derived.test_state` |
| `/revert` | `:1552` | `:1563` | `:8284` | spawns `git checkout HEAD -- <path>` |
| `/research` | `:1570` | `:1581` | `:8415` | runs a TUI-owned `ResearchService` |
| `/logs` | `:1678` | `:1689` | `:7514` | reads the daemon's `daemon.log` off the filesystem |
| `/review-change` | `:1822` | `:1833` | `:7624` | refuses: the TUI's `LspTool` is absent |
| `/security-review` | `:2218` | `:2229` | `:8479` | runs a TUI-owned review, no LSP context |
| `/security-review-show` | `:2236` | `:2247` | `:8554` | shows the TUI's cached result |
| `/security-review-cancel` | `:2254` | `:2265` | `:8564` | cancels the TUI's own tokio task |
| `/shell-rerun` | `:2326` | `:2337` | `:8612` | re-dispatches `TuiCommand::RunHumanShell` |

### 3.2 The four forms whose specific request already exists

- **`/open`** → `CoreRequest::DocumentOpen`. The TUI's arm
  (`src/tui/app/mod.rs:6745`) delegates to `crate::tui::commands::editor::open_editor`
  (`src/tui/commands/editor.rs:140`) → `open_editor_in` (`src/tui/commands/editor.rs:153`) → a
  spawned TUI task (`src/tui/commands/editor.rs:241-252`) → `TuiDocumentSession::open`
  (`src/tui/document_session.rs:594`) → `DocumentController::open` →
  **`CoreRequest::DocumentOpen`** (`crates/codegg-client/src/document.rs:337`).
  Daemon side: `src/core/daemon_documents.rs:387`; family `Documents`
  (`src/core/daemon_family.rs:89`).
- **`/subagent-send`** → `CoreRequest::TurnSteer`. The arm
  (`src/tui/app/mod.rs:5711`) only arms a badge
  (`src/tui/commands/subagent_steer.rs:119`, `:96-98`); the request leaves
  later, on the user's next message (`src/tui/app/prompt_turn.rs:112` →
  `src/tui/commands/subagent_steer.rs:255-262`). Daemon side: `src/core/daemon_turns.rs:1584`;
  family `Turns` (`src/core/daemon_family.rs:189`). The request carries the
  **parent session's** turn id, never a subagent's task id
  (`src/tui/commands/subagent_steer.rs:1-24`) — ADR-0041 *Indirect*.
- **`/security-review-cancel`** → `CoreRequest::TurnCancel`
  (`crates/codegg-protocol/src/core.rs:3012`), family `Turns`
  (`src/core/daemon_family.rs:188`), and the effect table already carries a
  `turn_cancel` row for another form (`src/core/daemon_commands.rs:387-396`).
- **`/shell-rerun`** → `CoreRequest::HumanShellRunCreate`
  (`crates/codegg-protocol/src/core.rs:3660`), family `HumanShell`
  (`src/core/daemon_family.rs:386`), issued by
  `src/tui/commands/shell.rs:124`.

### 3.3 The four confirmed defects this milestone carries

1. **`/review-change` and `/security-review` are dead in production.**
   `app.lsp_tool` is `Option<Arc<LspTool>>` (`src/tui/app/mod.rs:512`),
   initialised to `None` at **both** construction sites
   (`src/tui/app/mod.rs:1249` and `:1841`), and assigned **only** by the test
   helper `sample_lsp_tool()` (`src/tui/app/mod.rs:17837-17841`) and its call
   sites, all inside the test module. Consequences: `/review-change` refuses
   with *"The LSP tool is unavailable in this session."*
   (`src/tui/commands/change_review.rs:136-141`), and `/security-review` runs
   with `security_executor = None` and `lsp_available = false`
   (`src/tui/app/mod.rs:8503` → `src/security/workflow/report.rs:1102-1113`).
   The production LSP tool is built independently inside the agent
   (`src/agent/turn_runtime.rs:1539`), so the TUI's copy is never installed.
2. **`/revert` mutates a checkout with no disposition, no fence and no
   permit.** The arm (`src/tui/app/mod.rs:8284`) calls `handle_revert_command`
   (`src/tui/app/mod.rs:11423`), which builds the argv **by hand**
   (`src/tui/app/mod.rs:11433-11439`) and runs it through
   `crate::git_mutations::GitEnvPolicy::default().apply_sync`
   (`src/tui/app/mod.rs:11440`; `src/git_mutations.rs:47` re-exports
   `egggit::process::GitEnvPolicy`, the environment allowlist only). It takes
   no writer permit, no `CheckoutWriterSink`
   (`src/core/checkout_mediation.rs:41-70`) and no path validation against the
   workspace root, and it does not use the typed `GitOperation::Checkout`
   (`crates/codegg-git/src/operation.rs:94`) nor the model's fenced `git` tool
   (`src/tool/git.rs:570-601`, `:796`). **The guard cannot see it**:
   `scripts/check_writer_disposition.py` lists no `src/tui/**` path in
   `WRITER_SURFACE_DIRS` (`:177`), `WRITER_SURFACE_FILES` (`:195`) or
   `PROCESS_SURFACE_FILES` (`:212`), and `src/git_mutations.rs` has no
   `fs::write`-style primitive, so `has_effect_primitive` (`:263`) is false for
   it. Moving the mutation to the daemon is what makes it governable.
3. **`/plan` and `/tests` read state that is permanently empty.**
   `TuiSessionState::from_events` (`crates/codegg-core/src/session/state.rs:146`)
   has **no production caller**: every call site is at or after line 356, and
   the file's `#[cfg(test)] mod tests` begins at
   `crates/codegg-core/src/session/state.rs:336-337`. A repo-wide grep over
   `src/`, `crates/` and `apps/` finds no other caller. Therefore
   `session_state_derived.plan` is only ever what `/plan add` wrote
   (`src/tui/app/mod.rs:7344`), and `session_state_derived.test_state` is
   permanently `TestState::Unknown`, so `/tests` always prints *"Test state:
   unknown (no tests run yet)"* (`src/tui/app/mod.rs:11354`).
4. **`/logs` is an out-of-band frontend read of a daemon-owned file.** The TUI
   resolves the daemon's log path (`src/tui/app/modal.rs:127`;
   `crates/codegg-client/src/paths.rs:125` → `root.join("daemon.log")`) and
   reads a bounded tail off the filesystem (`src/tui/app/modal.rs:130`) with
   no request and no authority.

### 3.4 Corrections to the roadmap row

These are carried into the plan because the plan's shape depends on them.

1. **The decomposition.** Thirteen forms across eleven items, the extra two
   being the security-review workflow's `show` and `cancel` arms. The roadmap
   row's figures are **correct**; §1 states the derivation so a later reader
   does not have to re-derive it.
2. **"each item extends a request family the daemon already has, except
   compaction."** True of the **family**; four items already have the
   **specific request** (§3.2). For those, the mechanism is an effect-table
   row, and a new protocol variant would be wrong: it would be a second
   request for an effect the daemon already serves.
3. **The roadmap's "No ADR" claim does not survive for `/research`.** The TUI
   owns the research *runtime* today — `ResearchService`
   (`src/tui/app/mod.rs:14286`), the search runtime (`:14306`) and the
   `[search].backend` network policy. A daemon that owns the research *turn*
   while the frontend owns the research *runtime* splits one workflow across
   two owners, which is a new ownership boundary of exactly the kind ADR-0052
   does not cover. This is D5, resolved on 2026-10-30: the runtime moves into
   the daemon, and the decision is recorded in
   `plans/adrs/ADR-0059-research-runtime-ownership.md`. This milestone defers
   the item to that ADR's implementation (§14, stop condition 1).
4. **The generated artifact's `requests` column under-reported `/open` and
   `/review-change`; that defect is corrected.** The generator's resolver used
   to index only the TUI tree (`TUI_DIR = "src/tui"` and the resolver's
   `rglob("*.rs")` walk in `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`)
   and so could not follow a call into `crates/codegg-client/`. **The parallel
   correction has landed** (`SOURCE_TREES = ("src/tui", "crates/codegg-client/src")`),
   and `/open` now correctly reports `DocumentOpen` and `/review-change`
   correctly reports `LspPreviewApply`. This plan does not edit the artifact or
   the generator.

   **The generator's own line numbers are moving** as it is corrected. The
   citations to `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py` in this
   plan therefore name each row by its **literal source text** — the
   `_missing("/compact", …)` and `_missing("/logs", …)` entries, the
   `_missing("/security-review-show", …)` entry, the `TUI_DIR` constant, the
   resolver's `rglob("*.rs")` walk, the `aliases = re.findall(…)` line and the
   *mechanical* definition — rather than by a line number alone. A line number
   for that file is a fact about one commit; the literal is the durable
   anchor, and a rebase that moves the generator must not invalidate this
   plan's reasoning.
5. **The daemon's effect table can only encode "scope plus at most one
   argument" effects.** That is the generator's own definition of a
   *mechanical* row (the `* **mechanical** — the effect request's fields are
   the scope's plus at most one argument` definition in `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`), and
   it is the mechanism boundary this plan works to: a form whose effect needs
   free text, a list, a flag set, a second effect or a capability with no
   request at all needs a **new `CoreRequest` variant**, not a row.
6. **The compact corrective's §3 bullet is stale.** It says *"`/compact` is a
   TUI-local builtin today; whether it becomes a catalog entry is ADR-0052's
   business and belongs to M007-D, not here"*. `/compact` has been a catalog
   entry since M007-D landed, and the corrective has been reconciled against
   this baseline. The corrective's stop condition 1 — *"Shape A needs a new
   `CoreRequest` variant … **do not** add it under this milestone"*
   (`plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md:157-161`)
   — is the decision this milestone must take, and it must be taken **here**,
   not in the corrective: the corrective itself says so
   (`plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md:148` — *"M007-H is where this
   corrective's stop condition 1 … would be discharged, and it must not be
   discharged twice"*).

## 4. Invariants that must not regress

- **I1 — The catalog's command identities do not change.** No command id,
  name, alias, description, domain, kind, capability or argument form is
  edited by this milestone. The only catalog change this milestone may make
  is a **kind correction** under D1/D6, and that changes a *kind*, not an
  identity.
- **I2 — A daemon effect is executed by the daemon.** A form the daemon
  serves runs as a daemon effect, audited as a canonical `Run`
  (`src/core/daemon_commands.rs:817-827`). A frontend effect never reaches
  the daemon (`scripts/check_frontend_action_set.py`).
- **I3 — The frontend never supplies an inner request.** The daemon builds
  the inner request from the scope and the argument
  (`src/core/daemon_commands.rs:185`); the frontend supplies only the
  command id, the form, the argument and the catalog version
  (`src/tui/command_route.rs:189-209`).
- **I4 — A checkout write is fenced or gated, always.** Every checkout
  mutation this milestone adds takes the C003 writer fence (the model's
  writers) or the C004 checkout reservation gate (the non-broker writers)
  (`architecture/writer_permit.md:278-292`, `:346-358`). A write with no
  authority is refused before its first effect.
- **I5 — A daemon-disclosed log is bounded and daemon-owned.** The log tail is
  the daemon's own file; a daemon-side log request returns a bounded tail and
  never an unbounded read. Under D6 the daemon discloses **no** log contents
  to a frontend at all, so this invariant has nothing to bound on the
  `/logs` path.
- **I6 — The daemon's automatic compaction trigger stays the only authority
  that decides *when* compaction fires.** `compact_if_needed`
  (`src/agent/context_runtime.rs:865`), the threshold
  (`src/agent/policy.rs:16`) and the decision
  (`src/agent/context_runtime.rs:896-904` → `src/context/compaction.rs:931`)
  are untouched. A user-requested compaction asks the daemon to compact
  *now*; it never changes the threshold.
- **I7 — The agent's LSP instance stays authoritative.** The TUI's `LspTool`
  is a second, non-authoritative instance
  (`plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`,
  the M007-F follow-up row). `/review-change` and `/security-review` move to
  the daemon's instance; they do not get a second one.
- **I8 — The frontend's authority does not widen.** No frontend gains the
  ability to reach an effect the daemon did not already serve. A form that
  stays frontend-local keeps a non-mutating disposition
  (`crates/codegg-core/src/frontend_action.rs:16-32`).
- **I9 — A guard's failure is a defect, not an obstacle.**
  `scripts/check_writer_disposition.py` may be **satisfied by a new manifest
  row**. It may not be **edited to pass**, and the guard's self-test
  (`--self-test`) must still fail on an injected defect.

## 5. Scope

### 5.1 In scope

- The **thirteen** catalog forms listed in §1, in the daemon's effect table,
  in the daemon's handlers, and in the TUI's routing — except `/research`,
  which is deferred to ADR-0059's implementation (D5).
- The **new `CoreRequest` variants** the forms that have no request need, in
  `crates/codegg-protocol/src/core.rs`, with their daemon handlers, their
  `DaemonRequestFamily` routing (`src/core/daemon_family.rs`), their
  authorization and their audit. Under D1, D3, D4 and D6 no variant is needed
  for `/compact`, `/plan`, `/tests` or `/logs`.
- The **new writer-disposition manifest row** the git revert needs — one row,
  `fenced-native` under the C003 writer fence (D2). **No** `CheckoutWriterSink`
  variant is added.
- The TUI's local arms for the thirteen forms: replaced by a `send_command`
  call where the daemon serves the form, and made **honest** where the form
  stays local.
- The **four confirmed defects** of §3.3, each closed or explicitly bounded.
- The regeneration of the generated classification artifact, **by its own
  generator**, once the code changes land.

### 5.2 Explicitly out of scope

- The TUI's terminal (plan 019 / M007-J), the **desktop GUI**, and every
  already-closed milestone. The desktop's action table is not touched; the
  desktop's parity with these thirteen forms is a later milestone's business,
  and the artifact's inventory — not this plan — is what tells it what
  changed.
- **The catalog's command identities** (I1), and the catalog's `CommandKind`
  of any form except under D1/D6.
- **Any frontend's authority** (I8).
- **The M007-F LSP forms** (`/lsp-*`), M007-G's plugin forms and M007-I's
  three subsystem moves. They are named in the generator's follow-up table and
  belong to their own milestones.
- **The daemon's compaction policy**, its threshold, its tracker and the
  TUI's rendering of `AppEvent::CompactionTriggered`
  (`src/tui/runtime/app_events.rs:525-531`) — the corrective's §3
  non-goals, carried forward unchanged.
- **Editing `scripts/check_writer_disposition.py`** (I9).
- **Editing the generated artifact or its generator by hand.** Both are
  regenerated or left alone.
- **The research runtime move itself.** ADR-0059
  (`plans/adrs/ADR-0059-research-runtime-ownership.md`) decides it; this
  milestone **defers** the `/research` item to that ADR's implementation and
  adds no research request of its own.

### 5.3 Decision points — resolved

Eight decisions were put to the project maintainer and **answered on
2026-10-30**. Each subsection below now carries a `**Resolution:**` line
stating the owner's choice, the date and the reason, and the **Consequence**
line under it states the shape the milestone now takes. **Every work package
that depended on a decision has been rewritten against the resolved shape**, so
no package below waits on an answer any more. The options are retained
unchanged as the record of what was asked; the resolution, not the
recommendation, is what the milestone implements.

**The option letters below are this section's own** — they are the letters the
question was asked with. Where the owner's ballot labelled the same choice
with a different letter, each `**Resolution:**` line states **both**: the
owner's label and the letter this section uses, so a reader holding either
record can follow the mapping.

**The one remaining external gate is ADR-0059's implementation**
(`plans/adrs/ADR-0059-research-runtime-ownership.md`), for the research item
only (D5). Every other decision is settled, and no other item in this
milestone waits on anything outside it.

#### D1 — Does M007-H add a `CoreRequest` compaction variant?

**Resolution:** **Exclude — the owner's Option A, which is this plan's Option B
("no variant").** `/compact` does **not** get a new `CoreRequest` variant in
M007-H; compaction stays automatic daemon policy, and the TUI's `/compact`
command stays owned by the registered TUI compact command corrective
(`plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md`).
**Resolved by the project maintainer on 2026-10-30.** The reason: the catalog
form `/compact` is corrected to a `FrontendEffect` with a declared action, and
the TUI's truthful report (the corrective's **Shape B**) is that corrective's
work, not M007-H's — so M007-H adds no daemon surface for compaction and does
**not** discharge the corrective's stop condition 1.

*Statement:* whether `/compact` gets a daemon request at all, and therefore
whether this milestone discharges the compact corrective's stop condition 1.

- **Option A — a new `CoreRequest` variant** (e.g. `SessionCompact`). The
  daemon compacts the named session on request, under its own authority, and
  reports the outcome; the TUI renders the daemon's answer. **This is the
  corrective's Shape A.**
- **Option B — no variant.** `/compact` stays a TUI-local command and becomes
  *honest*: it reports that compaction is automatic, that it fires at the
  daemon's threshold, and how many times the session has compacted (the count
  the TUI already keeps at `src/tui/runtime/app_events.rs:531`). **This is the
  corrective's Shape B**, and it is always available.
- **Option C — a variant the daemon may refuse.** As A, but the daemon
  declines when the session's context is below the threshold, and the TUI
  reports the refusal rather than a silent no-op.

**Recommended: Option A**, with Option C's refusal semantics folded in. The
reason is that the roadmap's item is *the daemon's missing command surfaces*,
and a command whose only truthful behaviour is to narrate a daemon-internal
policy is not a command the daemon is missing — it is a command the frontend
should not have. Option B remains the honest floor, and it is what the
milestone falls back to if the owner declines A.

**Consequence (resolved).** Work package **B1** is dropped, work package **E**
carries the milestone's only share — the catalog kind correction — and the
corrective's stop condition 1 is **not** discharged by this milestone. The
milestone's compaction item closes with **no daemon surface**, which is a scope
reduction the roadmap row must then record.

#### D2 — Which writer path does a daemon-executed `/revert` take?

**Resolution:** **The daemon executes it, fenced — the owner's Option A, which is
this plan's Option B ("a new daemon handler issuing the typed `GitOperation`
directly", under the C003 fence).** A new daemon path issues
`codegg_git::GitOperation::Checkout { target: Some("HEAD"), paths }` under
the **C003 writer fence** (the same fence the model's typed git tool takes at
`src/tool/git.rs:570-601`), with a new `docs/writer-disposition.toml` root
recording it (`scope = "checkout-writer"`, `disposition = "fenced-native"`,
`mediation = "writer-fence"`). **No new `CheckoutWriterSink` variant is added**,
so no root claims a sink marker. **Resolved by the project maintainer on
2026-10-30.** The reason: the C003 fence already refuses a checkout the run
does not own before its first effect, so the daemon path needs the fence and a
manifest root and nothing else.

*Statement:* which authority governs the checkout mutation a daemon-side
`/revert` performs, and therefore what the manifest must say.

- **Option A — the model's typed Git tool.** Reuse the tool's typed
  `restore_worktree` mutation (`src/tool/git.rs:796`), which already takes the
  C003 fence (`src/tool/git.rs:570-601`). **Cost:** the daemon handler is
  still a new writer surface under `src/core/`, so it needs its own manifest
  root; but the fence is the **model's** fence, and ADR-0041/§`architecture/writer_permit.md:348-350`
  scopes C003 to *"the **model's** writers: the native file tools and the
  typed Git mutations"*. A user-invoked slash command is not the model's
  writer, so the C003 actor attribution (`architecture/writer_permit.md:306-321`)
  does not apply cleanly. **Manifest cost:** one root row, no new sink.
- **Option B — a new daemon handler issuing the typed `GitOperation`
  directly** (`crates/codegg-git/src/operation.rs:94`, rendering at
  `crates/codegg-git/src/render.rs:220-243`), under the C003 fence. **Cost:**
  one root row, no new sink; the same actor-attribution objection as A.
- **Option C — the same handler, mediated by the C004 checkout reservation
  gate.** A new `CheckoutWriterSink::GitRevert` variant in
  `src/core/checkout_mediation.rs:41-70`, the gate call in `src/core/daemon.rs`
  (the same place the existing sinks are gated, `src/core/daemon.rs:1121-1122`),
  and a manifest root claiming the marker string. **Cost:** one root row **and**
  one sink variant **and** the marker claim — and the guard fails on **both**
  directions if either half is missing
  (`scripts/check_writer_disposition.py:714-737`): a declared sink no root
  claims, and a claimed marker no sink declares.

**Recommended: Option C.** The reason is the authority table, not the cost:
C003 mediates the **model's** writers and C004 mediates **CodeGG's
non-broker writers** (`architecture/writer_permit.md:348-350`), and a
user-invoked `/revert` is a non-broker writer — the same class as the human
change-review LSP applier, the document service and the HTTP file routes,
every one of which is a C004 sink today
(`architecture/writer_permit.md:394-397`). Option C is also the only option
whose refusal is *attributable*: the C004 gate answers "is this checkout
reserved, and who may write it?" from durable state
(`architecture/writer_permit.md:356-374`) and records a durable
`checkout_writer_refusal` (`crates/codegg-core/src/audit.rs:175`).

**Consequence (resolved).** Work package **C** adds the manifest root only —
one row, `fenced-native` under the C003 writer fence — and the sink enum is
untouched, so no root claims a sink marker. The closure record must state
which authority governs the write (the C003 writer fence) and why the actor
attribution resolves.

#### D3 — Is `/plan` the agent's plan or the todo list?

**Resolution:** **Wire the daemon's state in — the owner's Option A, which is
this plan's Option B ("the daemon publishes the plan as a projection").** `/plan`
means **the agent's plan**, not the todo list. The daemon publishes it and the
TUI reads the daemon's published plan. **Resolved by the project maintainer on
2026-10-30.** The reason: the daemon already owns the plan, so the TUI's
permanent emptiness is a consumption defect (§3.3 defect 3) and not a missing
protocol capability.

*Statement:* what the plan list is, and therefore whether the daemon's answer
is a new read request or a projection of something it already publishes.

- **Option A — a new plan-read request.** The daemon reads the session's plan
  and returns it. **Cost:** a new `CoreRequest` variant in the `Goals` family
  and a new handler.
- **Option B — the daemon publishes the plan as a projection.** The plan is
  already a session event (`crates/codegg-core/src/session/events.rs:108`,
  `PlanUpdatedEvent`), so the daemon's published projection surfaces
  (ADR-0053) may already carry it and the TUI simply does not consume it.
  **Cost:** a TUI-side consumption change and possibly no protocol change at
  all — but only if the projection already carries it, which the implementer
  must establish before committing to this option.
- **Option C — the todo list.** `/plan` becomes `CoreRequest::TodoList`
  (`crates/codegg-protocol/src/core.rs:3249`, family `Goals`,
  `src/core/daemon_family.rs:290`, handler `src/core/daemon_goals.rs:635`),
  which the TUI already issues elsewhere (`src/tui/commands/goals.rs:335`).
  **Cost:** the smallest — an effect-table row — but it changes what the
  command *means*, and the catalog's description does not say "todos".

**Recommended: Option B, with Option A as the fallback.** The reason is that
the TUI's plan list is empty for a structural reason (§3.3 defect 3): the
projection that would carry the plan is never consumed, and
`TuiSessionState::from_events` has no production caller. Fixing the
consumption restores the agent's real plan without a protocol change; adding a
request would paper over a projection that is already broken. The implementer
must establish whether the daemon's published projection already carries
`PlanUpdatedEvent` before choosing; if it does not, Option A follows.

**Consequence (resolved).** Work package **D3** is a TUI consumption change:
the daemon publishes the plan and the TUI reads it. Work package **B2** adds
nothing. The meaning of `/plan` is unchanged — it is the **agent's plan**, not
the todo list — so §12 records no meaning change.

#### D4 — What is the authoritative test state?

**Resolution:** **The daemon's state is authoritative and is published — the
owner's Option A, which is this plan's Option B ("a projection").** The daemon
publishes the session's test state and the TUI reads
it, instead of the permanently-`Unknown` local field. **Resolved by the project
maintainer on 2026-10-30.** The reason: the daemon owns the test runner, so
its state is the authoritative answer and the TUI's permanently-`Unknown` field
is the defect (§3.3 defect 3).

*Statement:* what `/tests` reports, and where that state lives.

- **Option A — a new test-state read request.** The daemon reports the
  session's test-run state. **Cost:** a new `CoreRequest` variant in the
  `Jobs` family; the daemon already owns the test runner
  (`src/test_runner/runner.rs`, declared root `module.test_runner_runner`,
  `docs/writer-disposition.toml:754`).
- **Option B — a projection.** The test state is a session event
  (`TestRunStartedEvent` / `TestRunFinishedEvent`,
  `crates/codegg-core/src/session/events.rs`), so the daemon publishes it and
  the TUI consumes it — the same structural fix as D3 Option B, and the same
  question: does the published projection already carry it?
- **Option C — the job list.** `/tests` reports the daemon's jobs
  (`CoreRequest::JobList`, `crates/codegg-protocol/src/core.rs:3349`, family
  `Jobs`, `src/core/daemon_family.rs:225`). **Cost:** an effect-table row.

**Recommended: Option B, with Option A as the fallback**, for the same
structural reason as D3: the derived state exists
(`crates/codegg-core/src/session/state.rs:209-236`) and is never populated,
so the fix is consumption, not invention. Option C is the fallback if the
owner wants `/tests` to be a job listing rather than a test-suite state.

**Consequence (resolved).** Work package **D3** covers it with D3's: the
daemon publishes the session's test state and the TUI reads it. Work package
**B2** adds nothing. `/tests` reports the session's test state, not the job
list, so §12 records no meaning change.

#### D5 — Does the research runtime move into the daemon, or does the daemon
gain a request the TUI still runs?

**Resolution:** **The research runtime moves into the daemon — the owner's Option
B, which is this plan's Option A ("the runtime moves into the daemon") and
ADR-0059's Option B.** This
is the one item where the roadmap's "**No ADR**" premise does not survive, so
the decision is recorded in a new ADR: `plans/adrs/ADR-0059-research-runtime-ownership.md`
(**accepted — Option B (the runtime moves to the daemon), 2026-10-30**). **Resolved
by the project maintainer on 2026-10-30.** The reason: the network policy is a
security surface and the search backend setting is a configuration surface, and
neither may have two owners.

*Statement:* who owns the research workflow's runtime — and, with it, whether
this milestone needs an ADR.

- **Option A — the runtime moves into the daemon.** The daemon owns the
  research turn, the search backend and the network policy. **Cost:** the
  largest of the three; `src/research/` is a declared writer surface
  (`module.research_store`, `docs/writer-disposition.toml:702`) whose row
  would need widening, and the search backend's network authority moves with
  it. **This is a new ownership boundary and needs an ADR.**
- **Option B — the daemon owns the turn; the frontend keeps the runtime.** The
  daemon runs the research as an agent turn and the TUI's `ResearchService`
  remains a frontend-local execution surface the daemon calls into. **Cost:**
  the smallest, but it splits one workflow across two owners, and the ADR
  question does not go away — it gets smaller.
- **Option C — the daemon gains a thin research request** and the TUI's
  existing `ResearchService` call is replaced by it, with the search backend
  staying where it is and the daemon reaching it through the same policy
  object. **Cost:** moderate; no ADR if the daemon's request is the only
  caller, but the boundary is still "the daemon decides, the frontend
  executes".

**Recommended: Option A**, because it is the only one in which the daemon's
answer is authoritative end to end and the TUI holds no durable research
state — which is what §4's long-term requirement asks for. **The owner chose
Option A** (2026-10-30), so here the recommendation and the resolution agree.
Option A carries an ADR, the roadmap row's "No ADR" claim does not survive it
(§3.4 item 3), and the decision is recorded in
`plans/adrs/ADR-0059-research-runtime-ownership.md` (accepted 2026-10-30). This
milestone **defers** the item to that ADR's implementation (§5.2) rather than
implementing the research move — see §14, stop condition 1.

**Consequence (resolved).** Work package **B3** is **deferred**: the milestone's
`/research` item is not implemented here and no research request is added. The
decision is recorded in `plans/adrs/ADR-0059-research-runtime-ownership.md`,
and that ADR's implementation is the milestone's only remaining external
gate (§5.3). The roadmap row's "No ADR" claim is corrected by that record,
not by this plan.

#### D6 — Is the log tail a daemon request, or a permanently frontend-owned
read?

**Resolution:** **Permanently frontend-local — the owner's Option B, which is
this plan's Option B ("a kind correction").** `/logs` is corrected
to a `FrontendEffect` with a declared action; the daemon grows **no** log
family and discloses no log contents to a frontend. **Resolved by the project
maintainer on 2026-10-30.** The reason: a log file is the daemon's diagnostic
output rather than durable state, so a bounded local tail read is not reaching
a daemon-owned effect.

*Statement:* whether `/logs` becomes a daemon request or a kind correction.

- **Option A — a new log-tail request.** The daemon returns a bounded tail of
  its own log. **Cost:** a new `CoreRequest` variant and a handler; the daemon
  must decide what it discloses about itself (I5).
- **Option B — a kind correction.** `/logs` becomes a `FrontendEffect` with a
  declared `FrontendAction` of the `NonMutating` disposition
  (`crates/codegg-core/src/frontend_action.rs:16-32`), and the TUI keeps the
  filesystem read it already does (`src/tui/app/modal.rs:127-130`). **Cost:**
  the smallest; the catalog's kind changes, which is the **only** catalog
  change this milestone may make (I1). The guard's rule 4 — no declared
  action serves a form the catalog classifies as a daemon effect — is
  satisfied because the kind changes with it.

**Recommended: Option B.** The reason is that a log file is not durable
*state*: it is the daemon's diagnostic output, and a frontend that reads a
bounded tail of a file it already has a path to is not reaching a
daemon-owned effect. Option A becomes the right answer the moment the log
becomes a **queryable** surface (a filter over a session's own log, say),
which is not this milestone. If the owner chooses Option A, this milestone
must not also choose Option B: the two are mutually exclusive.

**Consequence (resolved).** Work package **D6** is a catalog kind correction
plus a declared action, and the generator's `_missing("/logs", …)` entry in
`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py` is retired by
the generator's own re-run. Work package **B4** adds nothing: the daemon grows
**no** log family and discloses no log contents to a frontend.

#### D7 — Is the security-review workflow one item or three?

**Resolution:** **One item, three surfaces — the owner's Option A, which is this
plan's Option A ("one item, three forms, designed together").** The workflow is
designed as one unit with three surfaces (run, show, cancel) — a daemon-side
run produces a result the daemon can read back and cancel. **Resolved by the
project maintainer on 2026-10-30.** The reason: the three forms are one
workflow's start, read and cancel, and a daemon that can start a run nobody
can read is not a workflow.

*Statement:* whether the three security-review forms are designed and landed
together, and whether the daemon owns the run.

- **Option A — one item, three forms, designed together.** The daemon owns the
  review run; `/security-review` starts it, `/security-review-show` reads its
  result back, `/security-review-cancel` cancels it. **Cost:** the largest
  single item, but the three surfaces are one workflow and splitting them
  produces a daemon that can start a run nobody can read.
- **Option B — three independent items.** Each form lands on its own.
  **Cost:** the smallest per item, but `/security-review-show` has nothing to
  show until `/security-review` has run daemon-side, and
  `/security-review-cancel` has nothing to cancel.

**Recommended: Option A.** The reason is that the three forms are one
workflow's start, read and cancel, and that the daemon must own the run for
any of them to mean anything: the TUI's current cancel
(`src/tui/app/mod.rs:2933-2945`) cancels a **TUI tokio task**, which is not
the same thing as cancelling a daemon turn, and the TUI's current show
(`src/tui/app/mod.rs:8554-8563`) reads a **cached in-memory result**
(`src/tui/app/mod.rs:538`). Landing `/security-review` alone would leave two
forms reading and cancelling a frontend-local artefact of a daemon-owned run.

**Consequence (resolved).** Work package **B5** adds the start, the read and
the cancel variants together, and the milestone's item count stays eleven.

#### D8 — Is a served form's TUI arm removed, or kept as a fallback?

**Resolution:** **Remove it — the owner's Option A, which is this plan's Option A
("the arm is removed").** When a form becomes daemon-served, the
TUI's bespoke arm is removed and the TUI issues the daemon request, exactly as
M007-D migrated the 39 commands it took over. No fallback arm is kept.
**Resolved by the project maintainer on 2026-10-30.** The reason: a fallback
that reports "unknown (no tests run yet)" or "Compaction triggered" is not a
fallback but a lie that survives the daemon being unavailable.

*Statement:* what the TUI does with a local arm whose form the daemon now
serves.

- **Option A — the arm is removed.** The form's effect is the daemon's; the
  TUI's dispatcher arm for it is deleted and the daemon's answer is rendered
  in its place. **Cost:** the cleanest; the TUI holds no second
  implementation.
- **Option B — the arm is kept as an offline fallback.** When no daemon is
  attached, the TUI falls back to the local behaviour. **Cost:** two
  implementations of one command, and the fallback is the defect this
  milestone exists to remove (the `/compact` toast, the `/tests` "unknown").

**Recommended: Option A.** The reason is defect 3 and the `/compact` toast:
a fallback that reports "unknown (no tests run yet)" or "Compaction
triggered" is not a fallback, it is a lie that survives the daemon being
unavailable. The existing migrated commands already follow Option A — their
dispatcher arms call into a module that calls `send_command`
(`src/tui/runtime/command_dispatch.rs:3057`).

**Consequence (resolved).** Work package **D** removes the local effect from
each of the thirteen arms and replaces it with the daemon call. No fallback
arm is kept, so §13's acceptance criteria need no "the fallback is honest"
clause.

## 6. Required production changes

### 6.1 The mechanism, per form

Three mechanisms exist.

- **M1 — an effect-table row.** A new row in `build_inner_request`
  (`src/core/daemon_commands.rs:185`) mapping `(command_id, form)` to an inner
  request the daemon already serves. Expressible **only** when the inner
  request's fields are the scope's plus at most one argument — the
  generator's *mechanical* definition
  (`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py:1487-1491`).
- **M2 — a new `CoreRequest` variant.** A new variant in
  `crates/codegg-protocol/src/core.rs`, its `DaemonRequestFamily` routing
  (`src/core/daemon_family.rs`), its daemon handler, its authorization and
  its audit. Required when the effect needs free text, a list, a flag set, a
  second effect, or a capability with no request at all.
- **M3 — a kind correction.** The form's kind becomes `FrontendEffect` and a
  declared `FrontendAction` serves it
  (`crates/codegg-core/src/frontend_action.rs`), with a non-mutating
  disposition. Available to `/compact` (D1) and `/logs` (D6) — the two forms
  whose owner decisions left them frontend-owned.

| form | mechanism | the daemon request it reaches | authority | writer disposition | invariant it must not break | resolved by |
|---|---|---|---|---|---|---|
| `/open` | **M1** | `DocumentOpen` (`crates/codegg-protocol/src/core.rs:2543`; `Documents`, `src/core/daemon_family.rs:89`) | daemon | none — `DocumentOpen` is not a `CheckoutWriterSink` | the open takes a writer *lease* and writes nothing | — |
| `/subagent-send` | **M1** | `TurnSteer` (`crates/codegg-protocol/src/core.rs:3016`; `Turns`, `src/core/daemon_family.rs:189`) | daemon | none | ADR-0041 Indirect: the frontend steers the **parent** turn; the addressee badge stays frontend-local | — |
| `/security-review-cancel` | **M1**, once the daemon owns the run | `TurnCancel` (`crates/codegg-protocol/src/core.rs:3012`; `Turns`, `src/core/daemon_family.rs:188`) | daemon | none | the cancel resolves the run's turn id daemon-side; cancelling a TUI task is not cancelling a turn | D7 — **Option A** |
| `/shell-rerun` | **M2** for the run-history read, **M1** for the re-run | new read (`HumanShell`); `HumanShellRunCreate` (`crates/codegg-protocol/src/core.rs:3660`) | daemon | none new — the create is governed by `module.shell_runtime` (`docs/writer-disposition.toml:915`) | the rerun's source of truth becomes the daemon's run store, not the TUI's `ShellOutputStore` (`src/tui/app/mod.rs:539`) | — |
| `/compact` | **M3** — a kind correction, no daemon request | none — the daemon has no compaction request | frontend-local, by the registered compact corrective | none | I6: the automatic trigger stays the only authority that decides *when* | D1 — **Option B** (exclude) |
| `/revert` | **M2** | new git-write request (`Git` family) | daemon | **YES** — one new manifest root, `fenced-native` under the C003 writer fence; **no** new `CheckoutWriterSink` | I4: the write is fenced; the TUI's unfenced subprocess is gone | D2 — **Option B** (fenced) |
| `/plan` | a consumed projection (`Goals`) | the daemon's published plan | daemon | none | the daemon's plan is the authority; the TUI's local checklist is not | D3 — **Option B** |
| `/tests` | a consumed projection (`Jobs`) | the daemon's published test state | daemon | none | the daemon's job state is the authority | D4 — **Option B** |
| `/research` | **deferred to ADR-0059's implementation** | none in this milestone | — | a possible edit to `module.research_store` (`docs/writer-disposition.toml:702`) | one workflow, one owner | D5 — **Option A**, deferred |
| `/logs` | **M3** — a kind correction | none — the daemon grows no log family | frontend-local | none (read-only) | I5 | D6 — **Option B** |
| `/review-change` | **M2** for the preview read, **M1** for the apply | new preview-read request; `LspPreviewApply` (`crates/codegg-protocol/src/core.rs:3539`; `Goals`, `src/core/daemon_family.rs:289`) | daemon | none new — the apply is already declared (`docs/writer-disposition.toml:400`, `:407`) | I7: the daemon's LSP instance is authoritative; the apply stays the canonical checked apply | — |
| `/security-review` | **M2** | new review-start request (a turn) | daemon | none — the report is in memory; the only `std::fs::write` in the workflow is a test fixture (`src/security/workflow/report.rs:1573`, inside the `#[cfg(test)]` module at `:1531`) | I7 | D7 — **Option A** |
| `/security-review-show` | **M2** | new review-read request | daemon | none | the daemon owns the result it produced | D7 — **Option A** |

*The option letters in the **resolved by** column are **§5.3's own** letters.
Where the owner's ballot used a different letter for the same choice, §5.3's
`**Resolution:**` line states both.*

### 6.2 The writer-disposition entry the git revert needs

`docs/writer-disposition.toml` is the manifest the guard
(`scripts/check_writer_disposition.py`) reads. A new entry needs:

```toml
[[root]]
id = "<stable identifier>"            # required (guard :608)
path = "src/core/<module>.rs"          # required for a module root (:599-604); the file must exist
surface = "daemon"                     # one of SURFACES (guard :90-98)
scope = "checkout-writer"              # one of SCOPES (guard :83-88)
disposition = "fenced-or-denied"       # one of DISPOSITIONS (guard :57-70)
mediation = "checkout-gate"            # one of MEDIATIONS (guard :74-81)
mediation_site = "src/core/daemon.rs"  # required iff mediation == "checkout-gate" (:678-692)
mediation_markers = "CheckoutWriterSink::GitRevert"  # required iff mediation == "checkout-gate" (:687-691)
owner = "<final effect owner>"         # required, non-empty (:644-645)
locator = "<how the root is located>"
lifetime = "<scope of the effect>"
kind = "module"
note = "<why>"
```

The vocabulary the guard enforces, in full:

- `DISPOSITIONS` (`:57-70`): `allowed-read`, `fenced-native`, `denied-process`,
  `fenced-or-denied`, `mediated-gate`, `namespaced-state`, `shared-sidecar`,
  `compatibility-store`, `new-root-only`, `unsupported`, `out-of-contract`.
- `MEDIATIONS` (`:74-81`): `process-gate`, `writer-fence`, `checkout-gate`,
  `none`.
- `SCOPES` (`:83-88`) and `SURFACES` (`:90-98`).
- `scope = "checkout-writer"` with `disposition = "mediated-gate"` **requires**
  `mediation = "checkout-gate"` (`:670-673`); `checkout-writer` with
  `fenced-native` requires `writer-fence` (`:674-677`); `read-only` requires
  `allowed-read` (`:630-633`).
- Every entry must declare a `mediation` (`:656-660`).
- Under `checkout-gate`, the `mediation_site` file must exist and **every**
  comma-separated marker string must still be present in it (`:693-707`) —
  removing the mediation call from the source fails the guard.
- Every sink declared in `CheckoutWriterSink` must be claimed by **exactly
  one** manifest root (`:714-737`). A new sink variant with no claiming root
  fails, and a root claiming a marker no sink declares fails.

Under D2's **resolved Option B** the manifest grows by **one root row** and the
sink enum is **untouched** — the row is `fenced-native` under `writer-fence`, so
it declares no `mediation_site` and no `mediation_markers`. Option C, which was
**not** chosen, would have added one sink variant and one marker claim on top
of the same root row.

### 6.3 The TUI's arms

For each form the daemon serves, the TUI's local effect is replaced by the
daemon's answer:

- The dispatcher arm calls into a command module that calls
  `crate::tui::command_route::send_command(&core_client, scope, command_id,
  form, argument)` (`src/tui/command_route.rs:218`) and renders the daemon's
  inner response verbatim — the pattern at
  `src/tui/runtime/command_dispatch.rs:3057` and `src/tui/commands/goals.rs:110`.
- The scope is `crate::tui::command_route::scope_for(app)`
  (`src/tui/command_route.rs:176`), so the daemon learns the project and the
  workspace from the attached session's canonical binding and never from the
  frontend's guess.
- A form the daemon **refuses** renders the daemon's typed refusal; the TUI
  never substitutes a local approximation (D8 Option A).

## 7. Ordered work packages

Work packages are ordered by dependency, not by the roadmap's item order. The
eight owner decisions have landed (§5.3), so **no package below waits on an
answer**; each one names the resolution it now implements.

### Work package A — The effect-table rows for the forms that need no new
request

Add rows to `build_inner_request` (`src/core/daemon_commands.rs:185`) for
`("open_document", "")` → `DocumentOpen` and `("subagent_send", "")` →
`TurnSteer`, and route the TUI's `/open` and `/subagent-send` arms through
`send_command`.

**Two properties are load-bearing.** First, the inner request's fields must be
the scope's plus at most one argument, or the row is not mechanical and the
form belongs in work package B. `DocumentOpen` needs project id, workspace id
and a path: the first two are the scope, the third is the argument.
`TurnSteer` needs session id, turn id and text: the first two are the scope
(when a session is attached), the third is the argument — and the turn id is
the **parent** turn's id, which the frontend already knows
(`src/tui/app/mod.rs` `active_turn_id`). Second, the TUI's addressee badge
stays frontend-local: ADR-0041 *Indirect* is a decision about **who steers**,
not about where the selection happens.

**Depends on:** D8, resolved as **Option A** — the arm is removed rather than
kept as a fallback, so this package replaces the local arm with the daemon
call.

### Work package B — The new `CoreRequest` variants

**B1 — the compaction variant: dropped.** D1 resolved as **Option B** (the
owner's Option A — exclude), so M007-H adds **no** compaction `CoreRequest`
variant, no compaction handler and no compaction authority. Compaction stays
automatic daemon policy (I6), and work package **E** carries what remains.

**B2 — the plan and test-state reads: dropped.** D3 and D4 both resolved as
**publish the daemon's state and read it** (this plan's Option B, "the daemon
publishes it as a projection"), so this package adds **nothing** and the work
lands in package **D3**. Neither form needs a new `CoreRequest` variant: the
daemon already holds both pieces of state, and the defect is that the TUI never
reads them (§3.3 defect 3).

**B3 — the research request: deferred, not implemented here.** D5 resolved as
**Option A — the research runtime moves into the daemon** (the owner's Option
B; ADR-0059's Option B), recorded in
`plans/adrs/ADR-0059-research-runtime-ownership.md` (**accepted 2026-10-30**).
**This milestone does not implement the research move and adds no research
request**: the item is deferred to ADR-0059's implementation, which is the
milestone's only remaining external gate (§5.3).

**B4 — the log-tail request: dropped.** D6 resolved as **Option B**
(permanently frontend-local), so `/logs` is corrected to a `FrontendEffect`
with a declared action (package **D6**) and the daemon grows **no** log
family.

**B5 — the security-review trio.** D7 resolved as **Option A** (one item, three
surfaces), so the three variants land **together**: the start (a turn), the
read, and the cancel. The cancel routes to `CoreRequest::TurnCancel` (M1) once
the daemon owns the run; the read returns the daemon's own result, so a
daemon-side run produces a result the daemon can read back and cancel. The
workflow must run against the daemon's LSP instance, not a frontend one (I7).

**B6 — the preview read for `/review-change`.** The apply is
`CoreRequest::LspPreviewApply` (`crates/codegg-protocol/src/core.rs:3539`), already declared
(`docs/writer-disposition.toml:400`, `:407`) and already gated
(`src/core/daemon.rs:1121-1122`). The **read** of a preview by id has no
request today, so this package adds one. It must not duplicate M007-F's
surface: M007-F is closed under accepted ADR-0056, and `/review-change` is
M007-H's form.

**B7 — the shell-rerun read.** Add a request that lists the recent human-shell
runs with the command and the working directory each ran under, so the rerun
does not read the TUI's `ShellOutputStore`
(`src/tui/app/mod.rs:539`).

### Work package C — The git revert's authority and manifest

D2 resolved as **Option B — the daemon executes it, fenced** (the owner's
Option A). A new daemon path issues
`codegg_git::GitOperation::Checkout { target: Some("HEAD"), paths }`
(`crates/codegg-git/src/operation.rs:94`, rendering at
`crates/codegg-git/src/render.rs:220-243`) under the **C003 writer fence** —
the same fence the model's typed git tool takes
(`src/tool/git.rs:570-601`, `acquire_writer_permit` at
`src/tool/writer_fence.rs:280`) — and the manifest grows the root of §6.2 with
`scope = "checkout-writer"`, `disposition = "fenced-native"`,
`mediation = "writer-fence"`.

**No `CheckoutWriterSink` variant is added, so no root claims a sink marker.**
The `mediation_site` and `mediation_markers` fields the guard requires under
`checkout-gate` (`:678-692`, `:687-691`) are **not** declared: the guard
rejects a root that declares a mediation site under a non-`checkout-gate`
mediation (`:705-711`), and `checkout-writer` with `fenced-native` requires
`writer-fence` (`:674-677`). The manifest therefore grows by **one root row
and nothing else**, and the sink enum is untouched.

**The guard's two-sided check is the reason this package is not optional:**
`scripts/check_writer_disposition.py:714-737` fails both when a declared sink
has no claiming root and when a claimed marker names no declared sink. Adding
no sink and claiming no marker keeps both directions satisfied.

The TUI's unfenced subprocess (`src/tui/app/mod.rs:11433-11440`) is **deleted**
in the same change. Leaving it would leave two write paths, one of which no
guard can see — the exact defect §3.3 records.

### Work package D — The TUI's derived state and its dead arms

**D3 — the projection consumption.** `TuiSessionState::from_events`
(`crates/codegg-core/src/session/state.rs:146`) has no production caller
(§3.3 defect 3). D3 and D4 both resolved as **publish the daemon's state and
read it**, so the TUI consumes the daemon's published session events and
`session_state_derived.plan` and `session_state_derived.test_state` carry the
daemon's answers. This is the change that makes `/plan` and `/tests` stop
reporting empty state. `/plan` means **the agent's plan**, not the todo list,
and `/tests` reports the session's test state — the daemon's state is
authoritative and is published.

**D4 — the dead arms.** `/review-change` and `/security-review` are dead
because `app.lsp_tool` is never installed (§3.3 defect 1). The daemon-side
move (B5, B6) removes the dependency on the TUI's copy entirely; the TUI's
`lsp_tool` field (`src/tui/app/mod.rs:512`) becomes unused for these two
forms and is removed if nothing else uses it.

**D6 — the kind correction.** D6 resolved as **Option B (permanently
frontend-local)**, so `/logs`'s kind changes to `FrontendEffect` in the
catalog and the frontend action is declared
(`crates/codegg-core/src/frontend_action.rs`). This is a **kind correction**,
which is the only kind of catalog change this milestone may make (I1). The
daemon grows **no** log family and discloses no log contents to a frontend.

### Work package E — The compaction overlap with the corrective

D1 resolved as **Option B** (exclude), so M007-H adds **no** compaction
request and **does not discharge the compact corrective's stop condition 1**
(§5.3, D1). Compaction stays automatic daemon policy, and the TUI's
`/compact` command stays owned by the registered TUI compact command
corrective (`plans/implementation/tui-compact-command-corrective/001-compact-command-has-a-sender.md`).

**The corrective's own delivery is Shape B** — the command reports the truth
about what compaction is — and that is the corrective's work, not M007-H's.
M007-H's only share is the **catalog** side the corrective does not touch:
`/compact`'s form is corrected to a `FrontendEffect` with a declared action,
which is a **kind correction** and so within I1. The corrective's non-goal that
it makes no catalog change stands; this milestone owns the catalog.

**The corrective's non-goals are carried forward unchanged** (its §3): no
change to the daemon's compaction policy, threshold or tracker; no change to
the TUI's rendering of `AppEvent::CompactionTriggered`; no change to the
catalog's shape *by the corrective*.

### Work package F — The generated artifact and the guards

After the code lands, re-run the generator
(`python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`),
which rewrites the artifact and its counts from the rows. **This plan does
not edit the artifact or the generator by hand**, and the parallel correction
to the generator's resolver is landed independently.

The guard set that must be green (§11) includes the three this milestone
touches directly: `check_command_catalog_ownership.py` (the catalog's
identities did not change and the daemon's effect table names only daemon
effects), `check_frontend_action_set.py` (a kind correction is closed over the
declared actions), and `check_writer_disposition.py` (the new manifest root
for the fenced daemon revert).

## 8. Failure, cancellation, restart, contention semantics

- **A form the daemon refuses is refused with a typed reason, never with a
  silent no-op.** The effect table's refusal vocabulary is unchanged
  (`no_session`, `argument_unreadable`, `not_daemon_effect`,
  `src/core/daemon_commands.rs:51`).
- **A served form's TUI arm renders the daemon's refusal.** Under D8
  (resolved as Option A) there is no local fallback (§5.3, D8).
- **Cancellation.** A cancelled `/security-review` cancels the daemon's turn
  through `CoreRequest::TurnCancel`, and the daemon's turn-cancellation path
  is the one that already exists (`src/core/daemon_turns.rs`) — not a TUI task
  cancellation (`src/tui/app/mod.rs:2933-2945`). Compaction is not
  user-requestable in this milestone (D1), so there is no user-initiated
  compaction cancellation to report; the automatic path's own outcome is
  unchanged.
- **Restart.** The daemon's session and run state are durable; a restarted
  daemon re-derives the compaction count from the session's state, not from
  the TUI's in-memory counter (`src/tui/runtime/app_events.rs:531`), so the
  TUI's count becomes a display of the daemon's answer rather than a local
  tally. A restarted daemon's outstanding security-review run is either
  resumed or reported gone; the frontend reports what the daemon says.
- **Contention.** A `/revert` that finds the checkout reserved by a live
  agent-run group is **refused before its first effect** by the **C003 writer
  fence** — the same fence the model's typed git tool takes
  (`src/tool/git.rs:570-601`) — and the refusal is the fence's own typed
  refusal (`WriterFenceError`, `src/tool/writer_fence.rs:120-138`), not a
  silent no-op. No index, worktree or ref changes.
- **A log tail is bounded.** Under D6 the log tail is permanently
  frontend-local: the daemon discloses no log contents to a frontend, and the
  frontend's read of the daemon's own file stays the bounded read it already
  is (`src/tui/app/modal.rs:127-130`).

## 9. Compatibility and migration

- **Protocol.** New `CoreRequest` variants are additive. The protocol version
  (`PROTOCOL_VERSION`, `crates/codegg-protocol/src/core.rs`) is bumped only
  if the crate's own rule requires it; the plan does not assume a bump.
- **The catalog's identities are unchanged** (I1), so no frontend's stored
  command identity, keybinding or palette entry moves.
- **The artifact's numbers move.** Every count the artifact publishes is
  derived from its rows (the generator's own *"The summary table in §1 is
computed from the §3 inventory the same script writes, so the two cannot
drift"* note in `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`),
  so re-running the generator after this milestone changes §1's measured
  shape. Any prose elsewhere that quotes the artifact's counts must be
  re-derived from the regenerated rows, not adjusted by hand.
- **The daemon and the TUI must not disagree about a form.** The route is
  computed from the catalog's own classification and the daemon's own effect
  table (`src/tui/command_route.rs:138-163`), so a form the daemon serves is
  served for every frontend at once. A frontend that keeps a local arm for a
  served form would be a second implementation — D8 (Option A) removes it.
- **The desktop is not migrated by this milestone.** It gains nothing and
  loses nothing; the desktop's action table and parity are a later
  milestone's business (§5.2).

## 10. Required tests

The repository's rule is the narrowest target that covers the change, and a
comment that is not run is not evidence.

- **The effect table.** Extend `tests/core_daemon/command_catalog_m007d.inc`
  with a member that proves each new row is served: `serves_form` is true for
  the new `(command_id, form)` pairs, and a command execution over them is
  audited as a canonical `Run`.
- **The TUI's route.** Extend `src/tui/command_route_tests.rs`'s family
  coverage — the file's own contract is that *"a migrated command with no
  test fails the family it belongs to"* — with a member per migrated form:
  the route is a daemon effect, the TUI issues a `CoreRequest::CommandExecute`
  carrying the command's identity and its argument, and the response the
  presentation receives is the daemon's inner response verbatim.
- **The TUI's arms.** Extend `tests/tui_render/change_review_trajectory.inc`
  and `tests/tui_render/tui_logs_window.inc` for the two arms whose behaviour
  changes shape.
- **The git revert.** A member under `tests/command_git/` that drives the
  daemon's revert and asserts: the write is refused before its first effect
  when the checkout is reserved (no index, worktree or ref change), the
  refusal is the writer fence's typed refusal, and the write succeeds on an
  ordinary checkout.
- **The writer disposition.** `python3 scripts/check_writer_disposition.py`
  and `python3 scripts/check_writer_disposition.py --self-test` (§11). The
  self-test must still fail on an injected defect after the manifest grows.
- **The kind corrections** (D1, D6): a member proving the action set stays
  closed, that `/logs`'s corrected kind is served by the declared action, and
  that `/compact`'s corrected kind is served by the declared action.
- **The projection consumption** (D3, D4): a member proving
  `session_state_derived.plan` and `.test_state` carry the daemon's answer
  and no longer the local default.
- **The compaction overlap**: a member proving the automatic trigger is
  unchanged (I6) — the threshold still fires the daemon's own compaction — and
  that M007-H adds no user-requested compaction path.

## 11. Required verification commands

Run as written. `scripts/verify.sh quick` runs the guards, the
harness-lab unittests and a workspace `cargo check`; `full` adds clippy and
the capped nextest sweep. Both cap build jobs at `CARGO_BUILD_JOBS=2`.

```bash
# ── The guards this milestone touches ──────────────────────────────
python3 scripts/check_writer_disposition.py --self-test
python3 scripts/check_writer_disposition.py
python3 scripts/check_command_catalog_ownership.py --self-test
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_frontend_action_set.py --self-test
python3 scripts/check_frontend_action_set.py
python3 scripts/check_checkout_mediation_authority.py

# ── The generated artifact, regenerated by its own generator ───────
python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check

# ── Narrow tests ───────────────────────────────────────────────────
cargo test -p codegg-core
cargo test -p codegg-protocol
cargo test --test core_daemon -- command_catalog_m007d
cargo test --test command_git -- <the new revert member>
cargo test --test tui_render -- change_review_trajectory
cargo test --test tui_render -- tui_logs_window

# ── The workspace sweep ────────────────────────────────────────────
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci

# ── The canonical local entry point ────────────────────────────────
scripts/verify.sh quick
scripts/verify.sh full
scripts/verify.sh scoped origin/main
```

**Two rules bind these commands.** First, the root package's integration tests
are **families**: `cargo test --test <family> -- <member>` selects one member,
and `cargo test --test <family>` alone runs the whole family. A test id is
`<family>::<member>::<test>`. Second, the **`scheduler_jobs` family must never
be run locally**: M008 §16 forbids running a sweep against the developer's
own scratch root, and the family's members are excluded outright rather than
run and failed (`plans/closure/desktop-frontend-ide-foundation/014-status.md:324-325`,
`:352-353`). A closure record that reports the family as "not run" with that
reason is complete; one that reports it as passing is not.

`scripts/verify.sh scoped` computes the affected members from
`docs/test-ownership.toml`. A change to `src/core/daemon_commands.rs` or to
`crates/codegg-core/src/command_catalog.rs` widens the scope — the root
package depends on all eleven other crates and re-exports several, so the
coupling runs through the root's own source and no path rule can see it.
The scope widens to the whole workspace; that is expected and is not a defect.

## 12. Documentation updates

- `architecture/command.md` — the **Where It Lives** block
  (`architecture/command.md:33`, repeated at `:683`) names
  `build_inner_request()` as `:189` and `serves_form()` as `:638`; both
  anchors are stale against this baseline (`:185` and `:778`). This milestone
  corrects them and adds the forms it serves to the catalog's served surface.
- `architecture/writer_permit.md` — the mediated-sink inventory
  (`:394-397`) is **unchanged** under D2: no sink is added, so the count and
  the inventory stand. The C003 section records that the daemon's git revert
  is a fenced writer.
- `architecture/context-compaction-ownership.md` and `architecture/compaction.md`
  — the production-path inventory gains **no** requested-compaction row: under
  D1 M007-H adds no user-requested compaction path.
- `architecture/git.md` — the daemon's git surface gains the revert write
  under D2.
- `architecture/command_routing.md` — the served/unserved form list.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:569` — the
  M007-H row is updated when the milestone closes, **not** before; the row
  records the decisions, and this plan does not edit it.
- `docs/writer-disposition.toml` — the new root row for the fenced daemon
  revert. **No** marker claim: no sink is added.
- The **compact corrective**'s closure record — noting that the corrective's
  **Shape B** is its own deliverable and that its stop condition 1 was **not**
  discharged by this milestone (D1).

## 13. Acceptance criteria

Each criterion names a checkable property. A criterion satisfied by a
comment is not satisfied.

1. `serves_form` is true for every form the daemon serves, and false for
   every other unserved form — no form's route changes silently. The forms
   D1 and D6 left frontend-owned are served by their declared frontend
   action, not by the daemon; the forms D3 and D4 resolved to are served by
   the daemon's published projection, not by a declared action; and
   `/research` is not served at all (deferred to ADR-0059).
2. Each form the daemon serves has exactly one daemon route, and the daemon
   builds the inner request itself (I3): no frontend supplies one.
3. Every new `CoreRequest` variant has a `DaemonRequestFamily` routing, a
   daemon handler, an authorization decision and an audit mapping.
4. The TUI's local effect for each form the daemon serves is gone (D8,
   Option A), and no TUI arm for a served form remains reachable. The forms
   that stay frontend-owned carry a declared, non-mutating action.
5. `git status --porcelain` shows no `src/tui/**` file that mutates a
   checkout: the raw `git checkout` subprocess at
   `src/tui/app/mod.rs:11433-11440` is deleted.
6. The git revert's write is refused **before its first effect** when the
   checkout is reserved by a live agent-run group, and the refusal is the
   **C003 writer fence's** typed refusal — not a silent no-op, and not an
   unmediated write.
7. `docs/writer-disposition.toml` contains the new root, and
   `python3 scripts/check_writer_disposition.py` is green with its
   `--self-test` still failing on an injected defect.
8. `app.lsp_tool` is no longer on the path of `/review-change` or
   `/security-review`, and neither command is refused with "The LSP tool is
   unavailable in this session." in a daemon-connected session.
9. `session_state_derived.plan` and `session_state_derived.test_state` carry
   the daemon's answer, and `/tests` no longer prints "Test state: unknown
   (no tests run yet)" in a session that has run tests.
10. The daemon's automatic compaction trigger is unchanged: the same
    threshold (`src/agent/policy.rs:16`), the same decision
    (`src/agent/context_runtime.rs:896-904`), the same
    `AppEvent::CompactionTriggered` (`src/agent/context_runtime.rs:1645`) — and
    M007-H adds **no** user-requested compaction path (D1).
11. The generated artifact is regenerated by its own generator and its §1
    counts are derived from its §3 rows; no count in the artifact or in any
    prose about it is hand-written.
12. Every command in §11 is run, or its omission is recorded with the
    reason — in particular the `scheduler_jobs` family, which is excluded
    outright under M008 §16 and must be reported as **not run**, with the
    reason.
13. The `/research` form is **not** served by this milestone and **no**
    research request was added: the item is deferred to ADR-0059's
    implementation, and the closure record says so.

## 14. Stop conditions

1. **The research item's ADR implementation is not in place.** D5 is resolved:
   the research runtime moves into the daemon (ADR-0059, accepted 2026-10-30).
   **This milestone does not implement the research move** — it **defers** the
   `/research` item to ADR-0059's implementation, which is a separate task.
   If the implementation of this milestone cannot proceed for any other form
   without that move, the work on the remaining forms stops and reports, and
   the `/research` item stays deferred; it is never implemented here on the
   side.
2. **The writer-disposition guard cannot be satisfied by a manifest row.**
   The work stops; the guard is **not** edited (I9).
3. **A new `CheckoutWriterSink` variant is added without a claiming root,**
   or a root claims a marker no sink declares. The guard fails on both
   directions (`scripts/check_writer_disposition.py:714-737`); the work stops
   until both halves agree. Under D2 the manifest grows **one row** and the
   sink enum is **untouched**, so neither direction can fail.
4. **The compact corrective's stop condition 1 is discharged twice** — once
   here and once in the corrective. The work stops until one of them owns it.
   Under D1 this milestone does **not** discharge it at all: the corrective
   owns it, and this milestone's only share is the catalog kind correction.
5. **The generator's parallel correction is not landed** when the artifact is
   regenerated. The regeneration waits; the artifact is never hand-edited.
6. **A form's identity, kind or capability would have to change** outside
   D1/D6 (I1). The work stops; a catalog change beyond a kind correction is
   a different milestone.

## 15. Closure evidence required

- The **implementation commit**, and the baseline it was taken against.
- A **requirement-to-evidence matrix** over §13's thirteen criteria, each row
  naming a `path:line` anchor or a named test.
- **Every §11 command**, with its outcome: passed, failed, or **not run with
  the reason**. The `scheduler_jobs` family is reported as not run, with
  M008 §16 as the reason.
- The **guard runs**, with their exact output — including
  `check_writer_disposition.py`'s "N declared roots, N writer tools, N writer
  modules" line before and after the manifest grows.
- The **writer-disposition manifest's before and after**: the added root in
  full, and the sink enum's before and after — **unchanged** under D2, since
  no sink is added.
- The **generated artifact's regenerated §1 table**, with the note that every
  count in it is derived from the §3 rows by the generator.
- The **known limitations, stated as limitations** — in particular: the forms
  that remain frontend-owned under D1/D6, the fact that `/research` is
  deferred to ADR-0059's implementation, and the fact that the desktop was
  not migrated.
- **Unresolved findings by severity.**
- A **recommendation**: closed, conditionally closed, corrective pass
  required, or blocked.

## 16. Handoff notes

- The **surveys behind this plan** are the M007-H inventory and the four
  confirmed defects in §3.3. Every anchor in this plan was re-verified
  against the tree at the baseline in the header; one correction to the
  earlier survey is recorded in §3.4.
- **The eight decisions in §5.3 have landed.** All eight were answered by the
  project maintainer on **2026-10-30**, and each subsection carries a
  `**Resolution:**` line with the choice, the date and the reason. Six of them
  (D1, D2, D3, D4, D6, D7) changed the shape of a work package; D8 settled
  whether a served form keeps a fallback arm, and D5 settled whether the
  milestone can proceed on the research item at all. **The implementer executes
  the resolutions, not the recommendations** — the options and the
  recommendations are retained as the record of what was asked, and each
  resolution names the option letter **in §5.3's own lettering** alongside the
  owner's own label.
- **The one item that needed an ADR is `/research`** (D5), and the roadmap
  row's "No ADR" claim does not survive it (§3.4). The decision is recorded
  in `plans/adrs/ADR-0059-research-runtime-ownership.md` (**accepted — Option
  B, 2026-10-30**), and this milestone **defers** the item to that ADR's
  implementation rather than implementing the research move.
- **The generated artifact is not this plan's to edit.** Its counts move when
  the code moves; the generator moves them.
- **The compact corrective owns the TUI's compaction truthfulness.** This
  milestone does **not** discharge the corrective's stop condition 1 (D1); the
  two must not both implement it.
