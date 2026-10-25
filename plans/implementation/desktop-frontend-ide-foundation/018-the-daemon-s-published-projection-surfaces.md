# Desktop Frontend and IDE Foundation Milestone 018 — The Daemon's Published Projection Surfaces

Status: ready for handoff

Repository baseline: `02030facc` — `origin/main` at the time of writing: the
merged desktop GUI parity line through **M007-E**, M007-D and its corrective
pass (**plan 017**), and the owner's decision record for plan 017.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Planning basis:

- **ADR-0053** — `plans/adrs/ADR-0053-daemon-published-projection-surfaces.md`,
  accepted 2026-10-25. The daemon publishes the session's **selected model**,
  its **shell command runs** and its **tool contracts**, and it publishes the
  **real** session summary on a session update.
- M007-D's closure record,
  `plans/closure/desktop-frontend-ide-foundation/017-status.md` §3.6 — the
  desktop's action table, and the three actions the desktop carries only as
  an honest notice.
- The classification artifact,
  `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`
  §3 — the served set and the per-form verdicts.

Applicable ADR:

- **ADR-0053** — accepted 2026-10-25. This milestone is that record's
  implementation. It is bound by ADR-0010 (the desktop frontend and shared
  client boundary), ADR-0043 (session-owned subagent lifecycle projection)
  and ADR-0052 (cross-frontend command execution ownership), and it revisits
  none of their conclusions.
- ADR-0053 §"What this ADR decides, and what it leaves to the plan" leaves
  the **mechanism** to this plan. Section 3 below is that mechanism.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.4 — a frontend renders
  projections and does not own durable state. The desktop's
  `show_tool_contracts` and `show_shell_runs` actions today render state the
  **desktop** would have to re-derive, which is the TUI's defect in reverse;
  the daemon publishing the state is what makes the two frontends agree.
- `plans/002-long-term-roadmap.md:899` — "When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap." **This milestone
  changes no terminology and no canonical ownership boundary**: ADR-0053
  already decided the ownership, and this milestone implements it. The
  roadmap's M007 decomposition gains the **M007-J** row the owner's
  direction asked for.
- `plans/001-terminology-and-domain-model.md` — the canonical vocabulary is
  unchanged. The daemon's **projection** is not a *store* and a frontend's
  **view** is not the daemon's *registry*; §3.1 names the distinction this
  milestone relies on.

## 1. Objective

Make the daemon publish the three surfaces the desktop's uncarried actions
need, and publish the **real** session summary, so that the desktop
performs **five** of the six declared frontend actions instead of three.

ADR-0053 decided **what** the daemon publishes. This plan decides **how**,
and that decision is load-bearing: the desktop's action dispatch is
**synchronous** (`apps/desktop/src/App.tsx:1297` →
`apps/desktop/src/commands/frontendActions.ts:124`), and its
`FrontendActionContext` holds only what the renderer already has
(`apps/desktop/src/commands/frontendActions.ts:39-46`). A surface that
arrives by a separate daemon read would have to be fetched **before** the
action runs and held in the context — a second mechanism, and an action
that reports a value the renderer did not itself receive.

So the mechanism is: **all three surfaces ride the canonical session
projection**, which the desktop already receives on connect and on every
event, and which the TUI already holds
(`src/tui/app/state/projection_client.rs:549`).

Two of the three are state that changes rarely and can be event-fed. One —
the shell command runs — changes continuously and needs its own producer.

The milestone also withdraws a defect the measurement found: the daemon's
projection event for a session update fills a **stub** summary
(`crates/codegg-core/src/projection_replay/publication.rs:402`), and the
reducer's `SessionActivated` arm **replaces** `primary_session` wholesale
with it (`crates/codegg-protocol/src/projection/reducer.rs:347-356`). The
wipe is invisible today because no frontend reads `primary_session`. It
becomes visible the moment one does — which is exactly what this milestone
does.

## 2. What this milestone must not do

- **It must not change what a user sees.** Each change is a change to
  *where a value comes from*, never to the effect the user observes. The
  TUI's `/shell-list` and `/tool-contracts` keep answering, from
  daemon-owned state.
- **It must not add a `CoreRequest` variant.** Every change lands inside the
  existing projection and event surfaces. The projection protocol version is
  not bumped (`crates/codegg-protocol/src/projection/caps.rs:22`): the
  fields are additive and `#[serde(default)]`.
- **It must not carry the desktop's terminal surface.** `show_terminal` stays
  uncarried, with the desktop's honest notice, until **M007-J**. The
  daemon's `CoreRequest::InteractiveProcessList`
  (`crates/codegg-protocol/src/core.rs:3566`) is already served
  (`src/core/daemon.rs:4884`) and is not changed by this milestone.
- **It must not migrate the three granularity forms.** `shell_show`,
  `shell_expand` and `terminal_send` remain recorded outside the audited
  path by the owner's decision of 2026-10-25 (plan 017's closure §3.5). This
  milestone changes neither their routing nor their behaviour.
- **It must not perform the 28 catalog kind changes and the 24 new
  `FrontendAction` declarations wholesale.** Those stay recorded
  (plan 017's closure §10). This milestone adds **no** new declared action:
  `show_tool_contracts` and `show_shell_runs` are already declared.
- **It must not change the daemon's command catalog.** No catalog form, row
  or effect-classification is touched, so `architecture/command.md`'s
  asserted counts are unaffected.
- **It must not move the TUI's output bytes.** The daemon already retains
  the run's output chunks and serves them on demand
  (`src/shell/daemon.rs:40`, `:363`); the TUI keeps its bounded local copy
  for rendering (`src/shell/store.rs`). What moves is the **list**.

## 3. The mechanism ADR-0053 leaves to the plan

### 3.1 The four surfaces and where each rides

| surface | carrier | why there |
|---|---|---|
| the session's **selected model** | `SessionSummaryProjection.selected_model` (`crates/codegg-protocol/src/projection/dto.rs:131`), already declared and already reduced (`crates/codegg-protocol/src/projection/reducer.rs:1172`) | it is already a field of the session summary; only the daemon's **producer** is missing |
| the session's **selected agent** | `SessionSummaryProjection.selected_agent`, same shape | same |
| the session's **shell command runs** | a new bounded `shell_runs` collection on the snapshot (`crates/codegg-protocol/src/projection/snapshot.rs:29`) | the runs are the daemon's own registry; the list is state, and state belongs in the projection |
| the session's **tool contracts** | a new bounded `tool_contracts` collection on the snapshot | the contracts are the session's own registry, and the registry changes only at a turn's boundary |

The distinction the plan rests on: the daemon's **projection** is not a
store, and a frontend's **view** is not the daemon's **registry**. The
projection publishes a bounded *summary* of daemon-owned state; the
registry remains the authority, and the projection never becomes a second
copy of it.

### 3.2 The real session summary

`CoreEvent::SessionUpdated` gains an optional, serde-defaulted summary
field, and the daemon fills it with the session's own title, status,
selected model, selected agent and token counts. The publication path then
publishes **that** summary instead of the stub.

Two rules make this safe:

- **The stub is withdrawn, not patched.** An update that carries no summary
  publishes **no** projection event, so the projection is not erased by an
  update that has nothing to say. That is the defect's actual shape: the
  wipe came from publishing an *empty* summary, not from publishing one.
- **The daemon's producers always fill it.** The daemon knows the session's
  runtime state (`src/core/session_runtime.rs:38`, `:88`) and its durable
  record, so every `SessionUpdated` it publishes carries the real summary.

`ProjectionEvent::ModelSelected` and `ProjectionEvent::AgentSelected`
(`crates/codegg-protocol/src/projection/event.rs:384`, `:386`) are today
**claims with no hook** — declared, reduced, never constructed. This
milestone gives them producers, so the model and the agent are live rather
than a snapshot of one moment.

### 3.3 The shell runs need their own producer

The projection's `runs` collection is event-fed
(`ProjectionEvent::RunStarted`), and the daemon's snapshot builder rebuilds
the rest — `jobs`, `worktrees` — on connect and resync only
(`src/core/daemon_refresh.rs:45`, `:200-231`). The shell runs are neither:
they change while a turn is running, and a list rebuilt only on resync would
be stale exactly when a user asks for it.

So the daemon's human-shell service publishes a run's lifecycle, the
publication path maps it to a projection event, and the reducer upserts it
into `shell_runs`. The list is then fresh, the TUI's `/shell-list` stays a
**synchronous local render**, and the desktop's action reads a value the
renderer holds.

This is the one place where the milestone adds a producer on the daemon's
side of the projection boundary, and it is the price of the daemon owning
the list.

### 3.4 What is rejected, and why

- **A separate daemon read per surface.** It would need a protocol version
  bump, a second path to the same state, and — for the desktop — a host
  refresh before every action, which is the mechanism the TUI's own defect
  already models (`src/tui/commands/diagnostics.rs:354` builds a local
  registry). It would also leave the desktop's action reading a value the
  renderer did not receive.
- **Rebuilding the run list on the snapshot only.** Stale by construction,
  as §3.3 says.
- **Publishing the tool contracts from the static default registry.** The
  ADR rejects it explicitly: the contracts must be the ones the session's
  own registry holds, so a plugin-contributed tool is named when it is
  really available. The static default
  (`src/tool/mod.rs:120`) is used by nothing outside its own test.
- **The daemon building a second session registry to publish from.** The
  registry holds live tool instances — plugin adapters, LSP handles. A
  second registry would duplicate them and could open a second LSP
  connection. The session's own registry is the authority; the daemon needs
  a **handle** on its contract summary, not a second registry.

## 4. The work packages

### WP-A — The real session summary and the selected model

1. `CoreEvent::SessionUpdated` (`crates/codegg-protocol/src/core.rs:4703`)
   gains a serde-defaulted summary field carrying the session's own
   summary.
2. The daemon's producers fill it from the session runtime and the session's
   durable record. The daemon already ships the same values on another wire
   in `CoreResponse::SnapshotSession` (`src/core/daemon_turns.rs:1946`), so
   the assembly is a matter of reusing that reading rather than inventing
   one.
3. The publication path (`crates/codegg-core/src/projection_replay/publication.rs:402`)
   publishes the carried summary and publishes **nothing** when no summary
   is carried.
4. The daemon's snapshot builder (`src/core/daemon_refresh.rs:45`) fills
   `primary_session.selected_model`, `selected_agent`, the status and the
   token counts from the session runtime. It already fills
   `active_subagents` (`:72-81`), so this is the same seam.
5. `ProjectionEvent::ModelSelected` and `ProjectionEvent::AgentSelected` get
   producers, through the daemon's own selection paths and the publication
   path's mapping.

### WP-B — The daemon owns the session's shell command runs

1. The daemon's human-shell service (`src/shell/daemon.rs:60`) gains a
   `list` over its own registry, bounded by the bounds it already declares
   (`:18`, `:19`, `:17`). It has none today: its public surface is `create`
   (`:90`), `read` (`:363`), `scope_for` (`:433`) and `cancel` (`:441`).
2. The service publishes a run's lifecycle to the daemon's event log, and
   the publication path maps it to a projection event that upserts into the
   snapshot's new `shell_runs` collection.
3. The run's published metadata gains a start timestamp, so the list can be
   ordered by recency. `HumanShellRunMetadataDto`
   (`crates/codegg-protocol/src/human_shell.rs:63`) carries `expires_at_ms`
   but no start time, and the run id is a UUID rather than a time-ordered
   value.
4. The snapshot's `shell_runs` collection is bounded by a new constant
   beside its siblings (`crates/codegg-protocol/src/projection/limits.rs:40`,
   `:46`, `:116`).
5. The TUI's `/shell-list` reads the daemon's published list instead of
   `app.shell_store.list_recent(10)`
   (`src/tui/commands/shell.rs:1205`). The TUI's local store keeps the
   **output bytes** it renders; the daemon already serves those on demand
   (`src/shell/daemon.rs:363`).
6. The publication carries the **listing, not the bytes**: the run's output
   stays behind the daemon's own read.

### WP-C — The daemon publishes the session's tool contracts

1. The daemon owns a session-keyed **contract summary** — the catalogue the
   session's own registry holds, not the registry itself. The catalogue is
   built from the registry exactly as the broker builds it
   (`src/tool/broker.rs:617`), and the broker is built from the registry
   the turn actually runs against (`src/agent/loop.rs:391`).
2. The turn runtime writes it when the registry is complete — after the
   factory (`src/agent/turn_runtime.rs:583`) and after the plugin tools are
   registered (`src/agent/turn_runtime.rs:646`), because a plugin tool
   registered after the catalogue was built would not be named.
3. The daemon reads it in the snapshot builder and publishes it as the
   snapshot's new `tool_contracts` collection, bounded by a new constant
   beside its siblings.
4. The TUI's `/tool-contracts` reads the published contracts instead of
   building a local registry (`src/tui/commands/diagnostics.rs:351-355`).
   The rendering — the tool name, the caller policy, the effect class and
   the idempotency class — is unchanged; only the **source** of the
   contracts changes.

### WP-D — The desktop carries the actions

1. The presenter maps the three published surfaces into the desktop's
   presentation view (`apps/desktop/src-tauri/src/present.rs:488`), beside
   the collections it already maps.
2. The bridge DTO and its TypeScript mirror stay in step
   (`apps/desktop/src-tauri/src/bridge.rs:788`,
   `apps/desktop/src/bridge-types.ts:284`), and
   `apps/desktop/scripts/check-bindings.mjs:53` pins the field list, so a
   field added on one side and not the other is refused.
3. The desktop's action module gains the two renderers, and the two entries
   leave `UNCARRIED_ACTIONS`
   (`apps/desktop/src/commands/frontendActions.ts:56-61`). `show_terminal`
   **stays**, with its notice naming the desktop's own gap.
4. `show_status` names the model, which it cannot do today
   (`apps/desktop/src/commands/frontendActions.ts:95-101`).

## 5. Required changes

### 5.1 Protocol

- `crates/codegg-protocol/src/core.rs` — `CoreEvent::SessionUpdated` gains a
  serde-defaulted summary field; the model and agent selection gains their
  own events; the human-shell run's lifecycle gains an event.
- `crates/codegg-protocol/src/human_shell.rs` — the run's metadata gains a
  serde-defaulted start timestamp.
- `crates/codegg-protocol/src/projection/dto.rs` — the two new projection
  DTOs.
- `crates/codegg-protocol/src/projection/event.rs` — the two new projection
  events.
- `crates/codegg-protocol/src/projection/snapshot.rs` — the two new bounded
  collections and their upsert helpers.
- `crates/codegg-protocol/src/projection/limits.rs` — the two new bounds.
- `crates/codegg-protocol/src/projection/reducer.rs` — the two new reducer
  arms.

Every added field is `#[serde(default)]` and every added collection is
bounded, so a client that predates the change decodes it as absent.

### 5.2 Daemon

- `src/core/daemon_refresh.rs` — the snapshot builder fills the selected
  model, the selected agent, the status, the token counts, the shell runs
  and the tool contracts.
- `src/core/daemon_human_shell.rs` and `src/shell/daemon.rs` — the run
  list, and the lifecycle publication.
- `src/agent/turn_runtime.rs` and the daemon's turn path — the contract
  summary is written when the session's registry is complete.
- `crates/codegg-core/src/projection_replay/publication.rs` — the real
  summary, and the mappings for the new events.

### 5.3 Frontends

- `src/tui/commands/shell.rs` — `/shell-list` reads the daemon's list.
- `src/tui/commands/diagnostics.rs` — `/tool-contracts` reads the published
  contracts.
- `apps/desktop/src-tauri/src/present.rs`,
  `apps/desktop/src-tauri/src/bridge.rs`, `apps/desktop/src/bridge-types.ts`,
  `apps/desktop/src/commands/frontendActions.ts` — the three surfaces and
  the two renderers.

### 5.4 Constraints every change obeys

- **The comment budget is a strict ratchet with no headroom.** The corpus
  ceiling is `126485` comment lines (`docs/comment-budget.toml`), with
  per-file ceilings, and the scope is `src/`, `crates/` and `tests/`. Every
  new comment in that scope is paid for by compressing pre-existing prose
  in the same file. The manifest and the guard are never edited to make a
  change fit.
- **No new `CoreRequest` variant**, so the core protocol version is not
  bumped.
- **No catalog change**, so no asserted count in `architecture/command.md`
  moves.
- **The desktop stays a separate Cargo workspace.** The root `cargo check`
  does not compile it; the hosted desktop lane and
  `scripts/check-desktop-boundary.sh` are the gate.

## 6. Verification

Scoped, minimal, and aimed at the changed code:

- `cargo test -p codegg-protocol` — the new DTOs, the new events, the new
  reducer arms, and the snapshot's bounds.
- `cargo test -p codegg-core` — the publication path's real summary, and the
  withdrawal of the stub.
- The root package's projection and daemon families, selected by the change:
  the projection replay family and the daemon refresh family, run with the
  narrowest `--test <family> -- <member>` filter that covers the change.
- The desktop's own checks: the bindings guard
  (`apps/desktop/scripts/check-bindings.mjs`), the desktop host's `clippy`,
  and the desktop's TypeScript build.
- `scripts/verify.sh quick` — the canonical sanity sweep.
- `scripts/verify.sh scoped` — the tests the change can affect.

`scripts/check_comment_budget.py` runs as part of the sweep and must stay
green without a manifest edit.

## 7. Acceptance criteria

1. The daemon's projection snapshot carries the session's **selected model**
   and **selected agent**, taken from the session's own runtime.
2. `ProjectionEvent::ModelSelected` and `ProjectionEvent::AgentSelected` have
   producers, and a change of selection is visible to a subscriber without a
   resync.
3. A session update publishes the session's **real** summary, and a
   subscriber that reads `primary_session` keeps its title, its status, its
   model and its token counts across an unrelated session update.
4. The snapshot carries the session's **shell command runs**, the list is
   the daemon's own registry, and the publication carries the listing and
   not the output bytes.
5. The TUI's `/shell-list` answers from the daemon's published list.
6. The snapshot carries the session's **tool contracts**, taken from the
   session's own registry, and a plugin-contributed tool is named when it is
   really registered.
7. The TUI's `/tool-contracts` answers from the published contracts, with
   its rendering unchanged.
8. The desktop's `show_tool_contracts` and `show_shell_runs` are
   **performed**, and `show_status` names the model.
9. The desktop's `UNCARRIED_ACTIONS` holds **one** entry, `show_terminal`,
   with the desktop's own reason, and the notice is not widened.
10. The bridge's field list matches on both sides, and the bindings guard
    is green.
11. The comment budget is green with no manifest edit.
12. `scripts/verify.sh quick` is green.

## 8. Closure evidence required

- A requirement-to-evidence matrix against §7, with the file-and-line
  evidence for each criterion.
- The named test proofs for the projection crate, the publication path, the
  daemon's snapshot builder and the desktop's action module.
- The guard runs and their exact output, including
  `scripts/check_comment_budget.py` and `scripts/check_frontend_action_set.py`.
- The hosted lane identifiers for the code change, and the local
  `scripts/verify.sh quick` result.
- The unresolved findings, each with its reason for staying unresolved.

## 9. Stop conditions

- **If the projection cannot carry the shell runs without a second
  authority**, stop and record the alternative. Two authorities for one list
  is the defect this milestone closes, not a variant of it.
- **If the session's tool registry cannot be summarised without a second
  registry or a second LSP connection**, stop and record it. The contract
  summary is a summary of the session's own registry; it is never a second
  registry.
- **If the TUI's `/shell-list` or `/tool-contracts` cannot keep their
  current rendering**, stop and record the difference. The rendering is not
  this milestone's to change.
- **If publishing the real summary would make the projection carry a value
  the daemon does not own**, stop. The daemon publishes what it owns; it
  does not publish a frontend's state.
- **If the comment budget cannot absorb the change without a manifest
  edit**, stop and compress prose instead. The manifest is not edited.
- **If the desktop's terminal surface appears to be needed for
  `show_terminal`**, stop. That is **M007-J**, and it needs no ADR and no
  daemon work.
