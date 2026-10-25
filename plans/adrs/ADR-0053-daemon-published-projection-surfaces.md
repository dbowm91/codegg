# ADR-0053: The Daemon's Published Projection Surfaces

Status: accepted

Date: 2026-10-25

Accepted: 2026-10-25, by the project maintainer, on three directions given
together. First, that the daemon's published projection surfaces be decided
in **one** record rather than three, covering the session's selected model,
the session's shell command runs and the session's tool contracts, and that
the three then be planned and implemented. Second, that the defect the
implementation's own measurement found — the daemon's projection event for a
session update carrying an **empty** session summary — be fixed inside the
same milestone, with the daemon publishing the **real** session summary.
Third, that the desktop's terminal surface be registered as its **own**
milestone (M007-J) and that the `show_terminal` action stay uncarried until
that milestone lands.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`

Related ADRs:

- ADR-0010 (accepted) — desktop frontend and shared client boundary. This ADR
  is bound by ADR-0010's conclusions and does not revisit them.
- ADR-0043 (accepted) — session-owned subagent lifecycle projection. The
  subagent projection this ADR's sibling surfaces sit beside is not revisited.
- ADR-0052 (accepted) — cross-frontend command execution ownership. This ADR
  is the projection-side counterpart of ADR-0052 and is bound by it: the
  daemon still owns the catalog, and a frontend still performs a
  frontend-effect command from data the daemon publishes.

## Context

The line anchors in this section are as of **2026-10-25**, before plan 018
(`plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md`)
implemented this record. They describe the state that motivated the
decision. The closure for plan 018 carries the current anchors.

ADR-0052 decided that the daemon owns the command catalog and that each
frontend performs a frontend-effect command from a bounded, declared set.
Plan 017 applied that decision: six catalog forms were corrected to
`FrontendEffect`, six `FrontendAction` variants were declared for them, and
the action identity was published on the wire so a second frontend could
know what to perform
(`crates/codegg-core/src/frontend_action.rs:67-72`,
`crates/codegg-protocol/src/core.rs:1248`).

The desktop's palette now dispatches on that published action
(`apps/desktop/src/App.tsx:1297` →
`apps/desktop/src/commands/frontendActions.ts:124`). It performs three of
the six — `show_skills`, `show_worktrees`, and `show_status` in part — and
carries the other three only as an honest notice
(`apps/desktop/src/commands/frontendActions.ts:56-61`). The reason is the
same in each case, and it is not a wiring gap: **the daemon does not publish
the data the action needs**.

The three surfaces were measured, and each fails differently.

**The session's selected model.** The projection's session summary already
carries the field — `pub selected_model: Option<String>`
(`crates/codegg-protocol/src/projection/dto.rs:131`) — and the reducer
already applies it
(`crates/codegg-protocol/src/projection/reducer.rs:1172`). The daemon has
the value: `SessionRuntime::selected_model`
(`src/core/session_runtime.rs:38`), read into the runtime snapshot
(`src/core/session_runtime.rs:88`), and already shipped on another wire in
`CoreResponse::SnapshotSession` (`src/core/daemon_turns.rs:1946`). But the
daemon's projection publish path never fills it: `projection_snapshot_for_session`
(`src/core/daemon_refresh.rs:45`) sets `primary_session.active_subagents`
(`src/core/daemon_refresh.rs:72-81`) and nothing else on the summary. The
projection event that would carry a *change* has **no producer**:
`ProjectionEvent::ModelSelected`
(`crates/codegg-protocol/src/projection/event.rs:384`) appears in exactly
three places in the tree — its own declaration, its reducer arm, and the
`is_turn_scoped` arm (`crates/codegg-protocol/src/projection/event.rs:453`) —
and is never constructed. `ProjectionEvent::AgentSelected`
(`crates/codegg-protocol/src/projection/event.rs:386`) is in the same state.
A field that is declared, reduced and never produced is a claim, not a
hook.

**The session's shell command runs.** The daemon holds a human-shell run
service (`src/shell/daemon.rs:60`) whose records carry a scope, a metadata
DTO and a handle (`src/shell/daemon.rs:40`). Its public surface has
`create`, `read`, `scope_for` and `cancel` (`src/shell/daemon.rs:73-441`) and
**no `list`**, and the protocol's human-shell requests are `Create`,
`Capabilities`, `Read` and `Cancel`
(`crates/codegg-protocol/src/core.rs:3617-3626`) — there is no list
request. But the list the TUI's own `/shell-list` shows is not the daemon's
registry at all: the TUI reads its **local** output store
(`src/tui/commands/shell.rs:1205`, `app.shell_store.list_recent(10)`), which
the TUI itself fills when it issues a command
(`src/tui/runtime/command_dispatch.rs:2037`). So the daemon does not own
this state today, and cannot publish what it does not own.

**The session's tool contracts.** The daemon's broker builds a contract
catalogue from a registry (`src/tool/broker.rs:617`,
`src/tool/broker.rs:608`), and a `ToolContract` carries a caller policy, an
effect class, retry and cache policies, and an input and an output schema
(`src/tool/contract.rs:233`). But the registry is **turn-scoped**: it is
built per turn by `build_session_tool_registry`
(`src/tool/factory.rs:175`, called at `src/agent/turn_runtime.rs:583`) and
held by the agent loop's services
(`src/agent/coordinator.rs:53`). `CoreDaemon` has no registry or broker
field. The only daemon-global registry is the static default
(`src/tool/mod.rs:120`), which is currently used by nothing outside its own
test. The TUI's `/tool-contracts` arm builds a **local** registry
(`src/tui/commands/diagnostics.rs:354`), so the desktop's counterpart has
nothing to read.

**A second defect, found while measuring the first.** The daemon's
projection event for a session update is built from a `CoreEvent::SessionUpdated`
that carries only a session id, and it fills a **stub** summary —
`selected_model: None`, `selected_agent: None`, an empty title, a zero
status, no token counts
(`crates/codegg-core/src/projection_replay/publication.rs:402`). The
reducer's `SessionActivated` arm then **replaces** `primary_session`
wholesale with it (`crates/codegg-protocol/src/projection/reducer.rs:347-356`).
No frontend reads `primary_session` today, so the wipe is invisible. It
becomes visible the moment a frontend reads the model from it: any session
update erases it, along with the title, the status and the token counts.

**What is already served.** The interactive-process list is **not** part of
this problem: `CoreRequest::InteractiveProcessList`
(`crates/codegg-protocol/src/core.rs:3566`) and its response
(`crates/codegg-protocol/src/core.rs:1997`) already exist and are served
(`src/core/daemon.rs:4884`, `src/interactive_process_attach.rs:682`), and
`InteractiveProcessMetadata` carries a handle, a state and the PTY geometry
(`crates/codegg-protocol/src/interactive_process.rs:234`). The desktop's
blocker for `show_terminal` is that it has **no terminal surface at all** —
no analogue of the TUI's attach, input and resize wiring — which is a
desktop feature and not a projection question.

## Decision drivers

1. **A frontend may only perform what the daemon can tell it.** ADR-0052
   already decided that a frontend-effect command is performed by the
   frontend. That is only coherent if the daemon publishes the state the
   effect reads; where it does not, the desktop and the TUI disagree about
   what a command does, which is the defect plan 017 was written to remove.
2. **The daemon may not publish state it does not own.** A projection is a
   claim about daemon-owned state. Publishing a value the daemon merely
   mirrors from a frontend would invert the ownership ADR-0052 established.
3. **One decision, not three.** The three surfaces share a root: which
   daemon-owned state a frontend may read, and where the daemon's boundary
   with the agent layer sits. Deciding them separately would let one of
   them be answered without the other two, and the agent-layer coupling
   they share is the part that actually needs a judgement.
4. **A frontend that cannot perform an action must say so honestly.** The
   desktop's existing behaviour — an honest notice naming the desktop's own
   gap — is correct and stays. This ADR does not license faking an effect.
5. **The projection is a published contract.** Whatever the daemon adds is
   read by every frontend, including the TUI, and is bounded by the
   projection's own limits and version discipline
   (`crates/codegg-protocol/src/projection/caps.rs:22`).

## Considered options

### Option A — The desktop keeps a local registry of its own

The desktop builds its own shell-run list, its own tool registry and its
own model, as the TUI does today, and the daemon publishes nothing.

**Rejected.** It reproduces the defect plan 017 was written to remove: the
two frontends would hold different state and disagree about what a command
shows, and the desktop's list would be empty wherever the daemon ran the
command. It also leaves ADR-0052's premise intact but useless — a
frontend-effect command whose data the frontend must re-derive is a command
whose effect the frontend owns in fact.

### Option B — The daemon publishes the three surfaces; the desktop reads them

The daemon owns the session's shell command runs and its tool contracts,
publishes them alongside the selected model, and the desktop renders them.
The desktop's terminal surface stays out of scope, as its own milestone.

**Selected.** It makes the three actions performable, it keeps the daemon
the owner of the state it publishes, and it gives the three surfaces one
decision instead of three. It costs the daemon two ownership moves, which
is why it needs a record rather than a plan.

### Option C — Three separate records, one per surface

Each surface is decided in its own ADR.

**Rejected.** The three share the agent-layer coupling: the shell-run
registry and the tool registry both live behind the daemon's boundary with
the agent loop, and deciding one without the others invites a second
boundary question in the next ADR. The owner directed one record.

## Decision

1. **The daemon publishes the session's selected model.** The canonical
   session projection carries it, and the projection's own
   `SessionSummaryProjection.selected_model` field is the carrier
   (`crates/codegg-protocol/src/projection/dto.rs:131`). The daemon's
   publish path fills it, and a change to the selection is published as
   `ProjectionEvent::ModelSelected`
   (`crates/codegg-protocol/src/projection/event.rs:384`), so the field is
   live rather than a snapshot of one moment.
2. **The daemon owns the session's shell command runs and publishes
   them.** The run list the TUI's `/shell-list` shows becomes
   daemon-owned state, and the TUI's local output store becomes a
   projection of it rather than a second copy. The daemon publishes the
   list to every frontend.
3. **The daemon publishes the tool contracts available to the session.**
   The contracts are the ones the session's own tool registry holds — the
   registry the turn actually runs against — not a static default, so a
   plugin- or LSP-contributed tool is named when it is really available.
4. **The daemon publishes the real session summary on a session update.**
   The stub summary is withdrawn: a session update carries the session's
   own title, status, selected model, selected agent and token counts, so
   that a frontend reading the projection is not silently erased by an
   unrelated update.
5. **The desktop's terminal surface is out of scope.** `show_terminal`
   stays uncarried, with the desktop's own honest notice, until **M007-J**
   — the desktop terminal surface — lands. The daemon's
   `InteractiveProcessList` is already served and is not changed by this
   ADR.
6. **A frontend that cannot perform an action continues to say so.** The
   desktop's `UNCARRIED_ACTIONS` notice
   (`apps/desktop/src/commands/frontendActions.ts:56`) is the contract:
   the reason names what the **desktop** is missing, never what the command
   means.

### What this ADR decides, and what it leaves to the plan

This ADR decides **what** the daemon publishes. The **mechanism** — whether
a surface rides the session projection or a separate daemon read, how the
run list is bounded, how the contract catalogue is bounded, and how the
session summary is assembled — belongs to the implementation plan, which is
`plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md`.

## Consequences

### Positive

- The desktop performs **five** of the six frontend actions instead of
  three, so the two frontends agree about what each command does. The
  sixth, `show_terminal`, stays uncarried until **M007-J**.
- The projection's `selected_model` and `selected_agent` fields stop being
  claims with no hook: `ModelSelected` and `AgentSelected` gain producers.
- The session summary stops being wiped by a session update, so a frontend
  that reads the projection keeps the title, the status and the token
  counts.
- The daemon becomes the owner of the shell command-run list, which is
  where the TUI's list already had to come from in substance.

### Negative

- The daemon takes ownership of two things it does not own today. The shell
  command-run list moves out of the TUI's local store, and the daemon needs
  a handle on the session's tool registry that it does not have.
- The projection grows. Every published surface is read by every frontend,
  including the TUI, and is bounded by the projection's limits.
- The TUI's `/shell-list` changes shape: its local store becomes a
  projection of daemon-owned state rather than the TUI's own record. That is
  a behaviour change in a command the TUI already answers.

### Neutral or deferred

- The daemon's `InteractiveProcessList` is unchanged.
- The two `/lsp-*` command forms remain served and unmigrated, routed to
  **M007-F**.
- **M007-F**, **M007-G** and **M007-I** each remain recorded as needing
  their own ADR. This ADR does not decide the daemon's LSP command surface,
  its plugin catalogue, or the three subsystem moves; it decides the
  projection surfaces the desktop's three uncarried actions need.
- The 28 catalog kind changes and 24 new `FrontendAction` declarations the
  classification artifact records are untouched by this ADR.

## Compatibility and migration

- The projection's additive fields are `#[serde(default)]`, so a client that
  predates them decodes them as absent and the frontends keep their current
  behaviour until they read the new fields.
- The protocol version is not bumped by adding projection fields
  (`crates/codegg-protocol/src/projection/caps.rs:22`); a **new** request
  variant would bump it.
- The desktop's bridge DTO and its TypeScript mirror stay in step, and
  `apps/desktop/scripts/check-bindings.mjs:53` pins the field list, so a
  field added on one side and not the other is refused.
- The TUI's `/shell-list` keeps answering, but from daemon-owned state. The
  migration is the TUI's local store becoming a projection of the daemon's
  registry; the plan states the order.

## Security and reliability implications

- **Nothing crosses the bridge that the daemon does not own.** A published
  value is daemon-owned state, read-only on the frontend side, and a
  frontend cannot supply it.
- **The tool contracts are a schema, not a secret.** A `ToolContract`
  carries an input and an output schema (`src/tool/contract.rs:233`), which
  the desktop already needs in order to render the action's effect; it
  carries no credential.
- **The shell command-run list is bounded and carries no transcript.** The
  run's metadata carries its command, its scope, its status and its expiry
  (`crates/codegg-protocol/src/human_shell.rs:63`); the command's output
  stays behind the daemon's own read, and the publication carries the
  listing, not the bytes.
- **A projection that a frontend cannot fill is not faked.** Where a
  surface cannot be published, the action stays uncarried and the notice
  says so.

## Verification

- The daemon's projection publish path is proved to carry the selected
  model, and a session update is proved to leave the summary intact.
- The shell command-run list is proved to be daemon-owned and to survive the
  TUI reading it.
- The published tool contracts are proved to be the session's own — a
  plugin- or LSP-contributed tool is named when it is really present.
- The desktop's **five** carried frontend actions are each proved
  performable, and the bridge's field list is proved to match on both
  sides. `show_terminal` remains uncarried, with **M007-J** named as its
  milestone.

## Supersession

This ADR supersedes nothing. It is bound by ADR-0010, ADR-0043 and
ADR-0052, and it does not revisit their conclusions.
