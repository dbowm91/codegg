# ADR-0059: Research Runtime Ownership

Status: accepted — Option B (the runtime moves to the daemon) accepted 2026-10-30 by the project maintainer

Date: 2026-10-30

Decision owners: project maintainers

Related specification sections:
- plans/000-long-term-specification.md
- architecture/research.md
- architecture/search_backend.md
- architecture/writer_permit.md

Affected subsystem roadmaps:
- plans/subsystems/desktop-frontend-ide-foundation-roadmap.md (gates M007-H's research item)

## Context

The research workflow is one workflow with two owners today, and the seam runs
through a **security surface**.

**The TUI owns the runtime and the network policy.** `App::research_service`
(`src/tui/app/mod.rs:14286`) builds a `ResearchService` and attaches the App's
own search context to it; `App::set_search_runtime`
(`src/tui/app/mod.rs:14306`) installs that context. The context is what lets
`ResearchService::with_search_runtime` derive `allow_network` from
`[search].backend` (`src/research/service.rs:113-122`), so `/research` obeys
the same kill-switch as `websearch`. With no context installed the service stays
local-only by design. The command handler is `handle_research_command`
(`src/tui/app/mod.rs:11636`), and it builds its service through that same
`App::research_service` path (`src/tui/app/mod.rs:11692-11694`).

**The setting is a configuration surface.** `[search].backend` is
`SearchConfig::backend` (`crates/codegg-config/src/schema.rs:1335`), whose
value is `SearchBackendConfig` (`crates/codegg-config/src/schema.rs:1420`) —
`Eggsearch` or `Disabled`. It is a resolved config field, read by the code
that builds the `SearchRuntimeContext`.

**The daemon owns a research store but has no research request.** The store is
`ResearchStore` (`src/research/store.rs:10`), and it writes under a declared
writer root: `module.research_store`, `path = "src/research/store.rs"`,
`scope = "checkout-writer"`, `disposition = "namespaced-state"`
(`docs/writer-disposition.toml:702-712`). The daemon's side of the domain is
the *tool* — `ResearchTool` is registered in the daemon's tool registry
(`src/tool/mod.rs:781`, built at `src/tool/mod.rs:536`) — and the tool's own
runtime context is attached the same way
(`src/tool/research.rs:63-70`). But the daemon has **no research request**:
there is no research variant in `CoreRequest`, no research row in the daemon's
command effect table, and no research family in the daemon's request routing.

**The catalog form is declared a daemon effect and the daemon serves none of
it.** `/research` is `CommandKind::DaemonEffect` with
`capability: Some(CommandCapability::CommandExecute)`
(`crates/codegg-core/src/command_catalog.rs:1570-1581`), and the same is true of
its four siblings — `/research-runs`
(`crates/codegg-core/src/command_catalog.rs:1589`),
`/research-open` (`crates/codegg-core/src/command_catalog.rs:1607`),
`/research-show` (`crates/codegg-core/src/command_catalog.rs:1625`) and
`/search` (`crates/codegg-core/src/command_catalog.rs:1644`). All five are
`DaemonEffect`, and the daemon serves none of them: the daemon's command effect
table (`src/core/daemon_commands.rs:185`) has no research row, so
`serves_form` (`src/core/daemon_commands.rs:778`) is false for every one of
them. The TUI's arms carry the whole workflow today (`B::Research` at
`src/tui/app/mod.rs:8415`, `B::ResearchRuns` at
`src/tui/app/mod.rs:8426`, `B::ResearchOpen` at `src/tui/app/mod.rs:8430`,
`B::ResearchShow` at `src/tui/app/mod.rs:8441`).

**The desktop has no research surface at all.** Nothing under `apps/desktop/src`
names research, so a frontend that is not the TUI cannot run the workflow
today.

**ADR-0052 does not cover this boundary.** ADR-0052
(`plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`) decides
who *executes a command form*. It does not decide **which process holds a
workflow's runtime** when the runtime is a network-capable subsystem and the
catalog form is declared a daemon effect the daemon does not serve. Moving the
runtime is an ownership boundary in its own right.

## Decision drivers

1. **The network policy is a security surface and must have one owner.** Today
   `[search].backend` decides whether the research workflow may reach the
   network, and the decision is made in the **frontend** process
   (`src/tui/app/mod.rs:14286-14306`). A daemon that owns the research *turn*
   while a frontend owns the network *policy* means the authority to reach the
   network is not the authority that runs the work.
2. **The search backend setting is a configuration surface and must not be
   forked.** `[search].backend` (`crates/codegg-config/src/schema.rs:1335`) is
   one resolved configuration field. A move must not produce two settings, two
   resolution paths, or a frontend-only interpretation of the same value.
3. **The daemon already owns a research store, so the split is two authorities
   over one domain.** `module.research_store`
   (`docs/writer-disposition.toml:702-712`) is the daemon's declared writer
   root for research artifacts, while the runtime that writes them is the TUI's
   (`src/tui/app/mod.rs:14286`).
4. **A moved runtime must keep its bounded behaviour.** The runtime's
   behaviour is bounded today — a bounded tail, a bounded set of sources, a
   local-only default with no context installed
   (`src/research/service.rs:113-122`). A move that widened any of those
   bounds would be a different decision, not this one.

## Options

### Option A — the runtime stays in the TUI and the catalog form is corrected to a `FrontendEffect`

`/research` and its four siblings become `FrontendEffect` forms served by
declared `FrontendAction`s (`crates/codegg-core/src/frontend_action.rs:16-32`),
and the TUI keeps the runtime it already owns.

- **Writer dispositions:** no change. `module.research_store`
  (`docs/writer-disposition.toml:702-712`) already declares the writer root the
  runtime uses; the runtime does not move, so its row does not widen.
- **`Run` semantics:** the forms become frontend effects, so no daemon `Run`
  is created and no daemon authority is claimed. The catalog's
  `daemon_served` classification for five forms would be false.
- **What the desktop can then do:** it gains the *forms* — the desktop can
  invoke `/research` through the declared action set — but it does **not** gain
  the workflow's outputs, because the results stay in the TUI process and in
  the frontend's own local state.

### Option B — the runtime moves into the daemon and the daemon gains a research request surface

The daemon owns the research runtime, the search backend it runs against and
the network policy that governs it. The TUI's research arms become daemon
requests, and the catalog's five research forms become daemon effects the
daemon actually serves.

- **Writer dispositions:** `module.research_store`
  (`docs/writer-disposition.toml:702-712`) is re-owned by the daemon-side
  research module, and the daemon's research request handler becomes a declared
  writer root of its own (`surface = "daemon"`, `scope = "checkout-writer"`,
  `disposition = "namespaced-state"`). The network capability the runtime
  holds moves with it, so the daemon's research request needs an explicit
  authorization decision and a durable audit mapping like every other daemon
  request.
- **`Run` semantics:** each research form becomes a canonical daemon `Run` with
  its own inner request, audited like the rest of the daemon's command
  surfaces. The daemon's effect table gains rows for the five forms, so
  `serves_form` (`src/core/daemon_commands.rs:778`) becomes true for them and
  the catalog's `daemon_served` classification becomes truthful.
- **What the desktop can then do:** the desktop gains the whole workflow — the
  run, its result and its artefacts — because every one of them is a daemon
  answer rather than a frontend-local value.

### Option C — the research surface is deferred to its own milestone and M007-H corrects the kind only

M007-H corrects the five research forms' `kind` to `FrontendEffect` with
declared actions (Option A's shape), and the runtime's move into the daemon is
a **later** milestone's job.

- **Writer dispositions:** no change in M007-H; the move's dispositions land
  with the later milestone.
- **`Run` semantics:** the forms are frontend effects for the duration, so no
  daemon `Run` exists and the daemon serves none of them.
- **What the desktop can then do:** the desktop gains the *forms* and nothing
  else, and the workflow's outputs stay frontend-local — the same outcome as
  Option A, reached by a different route and with the boundary still open.

## Decision

**Option B is accepted.** The research runtime moves into the daemon, and the
daemon gains a research request surface. The owner accepted this on **2026-10-30**.

Concretely:

- **Who owns the runtime afterwards:** the daemon. `App::research_service`
  (`src/tui/app/mod.rs:14286`) and `App::set_search_runtime`
  (`src/tui/app/mod.rs:14306`) cease to be the research runtime's owners; the
  daemon-side research module owns the `ResearchService`, the `ResearchStore`
  (`src/research/store.rs:10`) and the coordinator that drives them.
- **Where the network policy lives:** in the daemon, and it is still derived
  from `[search].backend`. The derivation in
  `ResearchService::with_search_runtime` (`src/research/service.rs:113-122`)
  is preserved verbatim — the same setting, the same `allow_network`
  derivation, the same local-only default with no context installed. What
  changes is which process evaluates it: the daemon, not a frontend.
- **What happens to the TUI's `[search].backend` setting:** it is not forked
  and not duplicated. `[search].backend` remains one configuration field
  (`crates/codegg-config/src/schema.rs:1335`), and the daemon resolves it. The
  TUI's role in the research workflow changes from *deciding* whether the
  workflow may reach the network to *asking the daemon* — the TUI no longer
  derives `allow_network` for research at all.
- **What the desktop gains:** the whole research workflow. The five forms
  (`/research`, `/research-runs`, `/research-open`, `/research-show`,
  `/search`) become daemon effects the daemon serves, so every frontend —
  the TUI and the desktop alike — reads the same daemon-owned run, result and
  artefacts.
- **The move keeps its bounds.** The bounded behaviour the runtime has today
  is carried across unchanged: a bounded tail, a bounded source set, and the
  local-only default when no search context is installed.

**This ADR does not implement the move.** It decides the ownership. The
implementation is a separate task, and the M007-H plan
(`plans/implementation/desktop-frontend-ide-foundation/020-the-daemon-s-missing-command-surfaces.md`)
**defers** its `/research` item to this ADR's implementation rather than
implementing the move itself.

## Consequences

- **The TUI's research arms become daemon requests.** `B::Research`
  (`src/tui/app/mod.rs:8415`), `B::ResearchRuns`
  (`src/tui/app/mod.rs:8426`), `B::ResearchOpen`
  (`src/tui/app/mod.rs:8430`) and `B::ResearchShow`
  (`src/tui/app/mod.rs:8441`) stop owning the workflow and issue daemon
  requests instead, on the same pattern as the commands M007-D already
  migrated.
- **`serves_form` becomes truthful for five forms.** The daemon's command
  effect table (`src/core/daemon_commands.rs:185`) gains rows for the five
  research forms, and `serves_form` (`src/core/daemon_commands.rs:778`)
  becomes true for them.
- **The writer-disposition manifest changes shape.** `module.research_store`
  (`docs/writer-disposition.toml:702-712`) is re-owned by the daemon-side
  research module, and the daemon's research request handler is a new declared
  writer root. Both need a legal `surface`, `scope`, `disposition`,
  `mediation` and `owner`, and `scripts/check_writer_disposition.py` must pass
  with the research modules detected.
- **The daemon gains an authorization decision and an audit mapping** for the
  research request, because the request now carries the network capability
  the runtime held.
- **`/research` leaves M007-H's scope.** The M007-H plan's `/research` item
  is deferred to this ADR's implementation; M007-H does not implement the
  research move and does not add a research request of its own.
- **The desktop gains the workflow** it could not reach before: nothing under
  `apps/desktop/src` names research today, and after this move the research
  forms are daemon answers that any frontend can read.

## Compatibility, security and migration

- **Security — the network policy keeps one owner.** The `[search].backend`
  kill-switch (`crates/codegg-config/src/schema.rs:1420`) is evaluated by the
  daemon after the move, exactly as it is evaluated by the TUI before it, and
  the local-only default when no search context is installed
  (`src/research/service.rs:113-122`) is preserved. No frontend gains the
  ability to make the research workflow reach the network, and the daemon
  gains no path that reaches it without the setting's consent.
- **Security — the request that carries the capability is authorized.** The
  daemon's research request is a canonical `Run` with its own authorization
  decision and durable audit mapping, like every other daemon request. A
  research turn is attributable to the actor that dispatched it.
- **Compatibility — the catalog's five forms keep their identities.** Their
  `command_id`s, names and argument hints are unchanged; only who serves them
  changes. No form's identity, kind or capability changes (M007-H's I1).
- **Compatibility — the `[search].backend` setting does not fork.** One
  configuration field, resolved once. A configuration that set
  `search.backend = "disabled"` keeps disabling the research workflow's
  network access after the move, because the derivation is unchanged.
- **Migration — the TUI's arms are removed, not kept as a fallback.** When the
  daemon serves a form, the TUI's bespoke arm is removed and the TUI issues
  the daemon request, exactly as M007-D migrated the commands it took over.
  A second implementation of one workflow is the defect this move exists to
  remove.
- **Migration — the move is a separate implementation task.** This ADR decides
  the ownership; the M007-H plan defers its `/research` item to the
  implementation that follows this record.

## Required verification

Before acceptance of the implementation:

- The daemon's command effect table (`src/core/daemon_commands.rs:185`) serves
  all five research forms, and `serves_form` (`src/core/daemon_commands.rs:778`)
  is true for each of them and false for every other unserved form.
- The daemon's research request has a `DaemonRequestFamily` routing
  (`src/core/daemon_family.rs`), a daemon handler, an authorization decision
  and an audit mapping.
- `docs/writer-disposition.toml` carries the re-owned
  `module.research_store` root (`docs/writer-disposition.toml:702-712`) and
  the daemon's research request root, each with a legal `surface`, `scope`,
  `disposition`, `mediation` and `owner`, and
  `python3 scripts/check_writer_disposition.py` passes with the research
  modules detected and its `--self-test` still failing on an injected defect.
- A test proves the network policy still follows `[search].backend`: with the
  backend disabled the research runtime does not reach the network, and with
  no search context installed the runtime stays local-only — the derivation
  asserted at `src/research/service.rs:113-122`.
- A test proves the daemon's research request is refused without its
  authorization decision, and that the refusal is audited.
- The TUI's research arms (`src/tui/app/mod.rs:8415`,
  `src/tui/app/mod.rs:8426`, `src/tui/app/mod.rs:8430`,
  `src/tui/app/mod.rs:8441`) no longer own the workflow: each issues a daemon
  request, and no frontend-local research execution path remains.
- Two-client parity: the TUI and the desktop see the same research run,
  result and artefacts for the same form.
- The generated classification artifact is regenerated by its own generator and
  its counts are derived from its rows; no count is hand-written.
- `python3 scripts/check_adr_integrity.py` passes with this record present.
- A closure record exists for the implementation, and no other accepted ADR is
  amended without an explicit owner decision.

Supersession: none. This ADR is bound by ADR-0010, ADR-0043, ADR-0052 and
ADR-0053, and it does not revisit their conclusions.
