# ADR-0052: Cross-Frontend Command Execution Ownership

Status: proposed

Date: 2026-10-24

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/000-long-term-specification.md#88-daemon-owned-provider-connections`
- `plans/000-long-term-specification.md#124-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#21-project-communication`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`

Related ADRs:

- ADR-0010 (accepted) — desktop frontend and shared client boundary. This ADR is
  bound by ADR-0010's conclusions and does not revisit them.
- ADR-0011 (accepted) — editor document ownership and frontend replication.
- ADR-0012 (accepted) — native LSP read surface, delivery and authorization.

## Context

The TUI exposes a large manual command surface. At the repository baseline the
catalog is **161** `BuiltinSlashAction` variants
(`src/tui/command.rs`), and the dispatcher `dispatch_builtin_command`
(`src/tui/app/mod.rs:5661-8895`) has an arm for **every one** of them. Every
one of those names is invocable from the composer with a leading `/`.

That catalog is **entirely TUI-local**. `BuiltinSlashAction` occurs nowhere
outside `src/tui/`, and no `CoreRequest` variant executes a command. The
daemon's protocol has **266** request variants
(`crates/codegg-protocol/src/core.rs`), and none of them is a command
execution.

The owner's 2026-10-24 direction for the desktop is that it expose **the same
`/` commands** the TUI exposes, alongside a main chat region, a side panel,
and an editor. That direction cannot be met by the desktop today, and the
reason is structural rather than incidental.

Two canonical requirements already point at the shape of the answer, and
neither is currently satisfied:

- `plans/000-long-term-specification.md:341` — "The TUI **and protocol**
  SHOULD expose commands or views that answer:". The protocol half of that
  sentence has no implementation.
- `plans/000-long-term-specification.md:368-379` — "Manual commands MUST
  include a unified operation and focused aliases", with `/reload`,
  `/reload project`, `/reload skills`, `/agents refresh` given as examples,
  and then: "The final command names MAY differ, but there MUST be one
  discoverable manual command that refreshes all project runtime assets and
  returns a structured report of added, removed, changed, shadowed, and
  invalid entries."

The canonical documents therefore already contemplate a command surface that
is not the TUI's private property, while leaving the ownership question
open. They also fix three constraints that bear directly on the answer:

- `plans/000-long-term-specification.md:97` — "TUIs, ACP clients, web
  clients, and future frontends MUST render frontend-neutral snapshots and
  events. They MUST NOT become independent owners of provider credentials,
  project authorization, global scheduling, or durable session state."
- `plans/000-long-term-specification.md:602` — "Authorization is enforced in
  the daemon, not solely in frontends."
- `plans/000-long-term-specification.md:510` — "Free text MUST NOT silently
  become an unaudited privileged command."

There is also a vocabulary problem. `plans/001-terminology-and-domain-model.md:5`
makes the terminology document normative for "plans, protocol types, storage
schemas, architecture documents, tests, UI labels". The canonical
terminology has **no** cross-frontend `Command` concept: "command" appears
only as the project capability `command.execute`
(`plans/000-long-term-specification.md:217`) and as the `Run` record of "one
command, script, test, or tool execution"
(`plans/001-terminology-and-domain-model.md:452-459`). A desktop that
exposes a command surface under a vocabulary the canonical set does not
define would put a plan-level invention above canonical text.

## Decision drivers

1. **One authority.** A command that mutates anything must be authorized and
   executed where authorization is enforced — the daemon
   (`plans/000-long-term-specification.md:602`, `:580`). Two executors means
   two authorization stories, and the second one is not the daemon.
2. **One catalog.** If the TUI and the desktop each own a command list, they
   drift. Drift is not a cosmetic problem: a command present in one frontend
   and absent from the other is a capability difference users notice and
   report as a bug.
3. **Auditable.** `plans/000-long-term-specification.md:510` forbids free
   text silently becoming an unaudited privileged command. A command that
   runs must produce an auditable record — the canonical `Run`
   (`plans/001-terminology-and-domain-model.md:452-459`).
4. **Canonical-friendly.** The canonical set already says the *protocol*
   should expose commands, and it explicitly allows the final command names
   to differ (`:379`). The answer must make command **identity** shared
   while leaving **presentation** to each frontend, which is what the
   canonical text actually requires.
5. **Vocabulary discipline.** A new cross-frontend `Command` concept must be
   written into `plans/001-terminology-and-domain-model.md` **before** the
   protocol and the UI labels that use it, per
   `plans/002-long-term-roadmap.md:897-899` ("update the terminology document
   and long-term specification first, then adjust the roadmap").
6. **Bounded.** The protocol surface this adds must be bounded in payload,
   in command count, and in what a single execution may do
   (`plans/000-long-term-specification.md:538`).

## Considered options

### Option A — The desktop reimplements a look-alike command subset

**Benefit:** no protocol change; no daemon work; fastest to a demo.

**Cost:** two catalogs that diverge. The desktop's `/` would be a different
set of commands wearing the same prefix, and every divergence would be a
user-visible inconsistency. The desktop could not serve a command the daemon
does not implement, because there is no daemon half at all.

**Failure mode:** the desktop ships a `/model` that lists the TUI's models
from a stale snapshot, and a `/permissions` that shows nothing, because the
authority for both is the daemon and the desktop has no path to it. This is
the option that makes "the same commands" a claim the code cannot support.

### Option B — The daemon owns the catalog and executes daemon effects; each frontend dispatches frontend-local effects

**Benefit:** one catalog, one authority, one audit trail. The daemon can
answer "what commands exist" for every frontend, so a renderer never holds a
stale copy of the command surface. Daemon-effect commands are authorized and
audited exactly like any other daemon operation.

**Cost:** a new protocol surface; a canonical vocabulary addition; a split
between command kinds that the protocol must be able to express.

**Failure mode:** a frontend-effect command that is actually a daemon
mutation in disguise — for example a "reconnect the session" command that
looks like UI state but really reconfigures a subscription — would escape
daemon authorization. The mitigation is that the **kind** of each command is
declared in the catalog, and a command whose kind is wrong is a catalog
defect, not a frontend choice.

### Option C — The catalog lives in a shared crate; each frontend executes commands locally

**Benefit:** one source list; no protocol round trip for UI-only commands.

**Cost:** the executor is the frontend, so authorization moves into the
frontend for every command that touches state — directly against
`plans/000-long-term-specification.md:602`. It also requires the renderer to
consume a Rust catalog, which it cannot: the renderer's TypeScript layer
"does not speak the core protocol" and a Rust crate is not a wire format.

**Failure mode:** a command that reads session state renders whatever the
frontend last saw, so two frontends attached to the same session show
different answers to the same command.

### Option D — The desktop embeds or drives the TUI

**Benefit:** the command surface is the TUI's by construction.

**Cost:** the TUI is a terminal application built on crossterm and ratatui; it
cannot be embedded in a WebView. A pseudo-terminal hosting a TUI inside a
desktop app is a different product with a different security boundary and
would be rejected by `scripts/check-desktop-boundary.sh`.

**Failure mode:** it introduces a second renderer and a second input model,
which is precisely what this ADR exists to avoid.

## Decision

**Option B is selected.**

1. **The daemon owns the command catalog.** A single catalog is the source of
   truth for every command's identity, its display name, a one-line
   description, its kind, and the capability it requires. The catalog is
   defined once, in the daemon's own code, and is the only definition of
   "what commands exist".

2. **The protocol exposes the catalog.** A new request returns the catalog
   in a bounded form — identity, name, description, kind, required
   capability, and argument shape — with an explicit bound on both the
   command count and the payload size
   (`plans/000-long-term-specification.md:538`). The renderer's TypeScript
   layer consumes that response; it does not hold a hand-written copy of the
   catalog. Where a static mirror is required for ergonomics, it is
   **generated** from the Rust DTO and a guard proves it matches, following
   the pattern `apps/desktop/scripts/check-bindings.mjs` already establishes
   for the bridge DTOs.

3. **Every command declares its kind.** There are exactly two kinds:

   - **Daemon effect** — the command's result is a change to daemon-owned
     state. The frontend submits it; the daemon authorizes it under the
     capability the catalog names (`command.execute` is the canonical
     capability, `plans/000-long-term-specification.md:217`), executes it, and
     records a `Run`. The frontend never executes it locally.
   - **Frontend effect** — the command changes only what the frontend is
     showing: which region is focused, which panel is open, which view is
     active. The frontend dispatches it locally, from a **bounded, declared
     set** of frontend actions that the catalog names explicitly. A frontend
     effect never mutates daemon-owned state, and its set is finite and
     reviewable.

   The kind is a property of the command, not a choice the frontend makes at
   call time. A command whose kind is wrong is a catalog defect.

4. **Command identity is shared; command names may differ.** A frontend may
   present a different display name for the same command identity, which is
   what `plans/000-long-term-specification.md:379` already allows ("The final
   command names MAY differ"). What is shared is the identity, the
   description, the kind, and the required capability. The TUI's existing
   `/` names are unchanged by this ADR; it keeps its aliases and its
   dispatcher.

5. **Command execution is audited.** A daemon-effect command that runs
   produces a canonical `Run` record
   (`plans/001-terminology-and-domain-model.md:452-459`) carrying the command
   identity, the principal that issued it, and its result. Frontend-effect
   commands are not audited as `Run`s — they change nothing durable — but a
   frontend effect that requests a daemon-effect command's precondition
   (asking the daemon to attach a session, for instance) goes through the
   daemon like any other request.

6. **Free text never becomes a command.** A line in the composer that is not
   a recognized command is prompt text. There is no implicit dispatch, no
   fuzzy fall-through, and no "the frontend guessed what you meant". This is
   `plans/000-long-term-specification.md:510` made explicit at the protocol
   boundary rather than assumed.

7. **The terminology document is amended first.** Before the protocol
   variant or any UI label using the new concept lands,
   `plans/001-terminology-and-domain-model.md` gains a cross-frontend
   `Command` definition — the catalog entry, its two kinds, its identity, its
   relationship to `Run` and to the `command.execute` capability, and the
   rule that "command" and "Run" are not interchangeable
   (`plans/001-terminology-and-domain-model.md:759`). The amendment follows
   `plans/002-long-term-roadmap.md:897-899`, which requires terminology and
   the long-term specification to be updated before the roadmap is adjusted.

8. **The canonical long-term documents gain a desktop-frontend clause.** The
   canonical set is presently silent on a desktop GUI: `desktop` as a
   product, `GUI`, `graphical`, `Monaco`, `palette`, `keymap`, `mouse`,
   `sidebar` and `dock` have no occurrence in
   `plans/000-long-term-specification.md`,
   `plans/001-terminology-and-domain-model.md` or
   `plans/002-long-term-roadmap.md`. The GUI is neither required nor
   forbidden there. Because the owner has directed a desktop GUI line, the
   canonical specification gains an explicit statement that a desktop client
   is a first-class frontend under §4.4's constraints — rendering
   frontend-neutral snapshots, not owning durable state — and the long-term
   roadmap gains the desktop GUI as a named consumer of Phase 5's canonical
   session view. These amendments are **contingent on this ADR's
   acceptance** and are the owner-directed amendment that
   `plans/003-planning-process.md:29-36` permits.

## Consequences

### Positive

- The desktop's `/` surface is the TUI's `/` surface, by construction rather
  than by imitation, and the two cannot drift silently.
- Every command that touches state is authorized by the daemon and audited
  as a `Run`, satisfying `plans/000-long-term-specification.md:602` and
  `:510` without a per-frontend audit implementation.
- The TUI gains a benefit it did not have: its command catalog becomes
  discoverable over the protocol, which is what
  `plans/000-long-term-specification.md:341` already asks for.
- The renderer holds no command catalog of its own, so the "stale command
  list" class of bug is structurally absent rather than tested for.

### Negative

- The protocol grows a new request family and a new DTO, and the daemon
  grows a catalog to maintain. Every command the TUI adds becomes
  something the daemon must expose.
- The TUI's dispatcher is affected: commands that are daemon effects must
  move their execution to the daemon, which changes TUI behaviour and
  therefore requires its own regression coverage.
- Two kinds of command mean the catalog is the place where a mistake becomes
  a security-relevant defect. The catalog therefore needs its own review and
  its own guard.

### Neutral or deferred

- Command *display names* remain a frontend concern. The TUI's aliases
  (`src/tui/command.rs`) are untouched.
- A command palette as a distinct UI affordance is **not** decided here. This
  ADR decides who owns command identity and execution; the palette's
  appearance belongs to the frontend milestone that consumes this decision.
- Plugin and MCP-contributed commands (`src/tui/commands/plugins.rs`,
  `src/tui/commands/plugin_management.rs`) are **deferred**. A dynamic
  command source has different trust properties from a built-in one and must
  not be folded into this decision by analogy. The deferred question is
  whether a dynamic command may be a daemon effect at all.
- The ACP boundary is untouched. ACP is the canonical editor-to-agent
  boundary (`plans/000-long-term-specification.md:18`, `:526`); if a future
  ACP client is to expose the same commands, this ADR's shape is the one it
  would reuse.

## Compatibility and migration

- **Protocol.** The new request family is **additive**. A daemon that does
  not implement it answers with its existing unsupported-variant error, and
  a client that asks for the catalog degrades to a command list it already
  holds. The existing `PROTOCOL_VERSION` negotiation
  (`plans/000-long-term-specification.md:536-538`) is the mechanism; no
  existing variant changes shape and no stored record is migrated.
- **TUI.** The TUI's command catalog keeps its identities, aliases and
  descriptions. Commands whose kind is *daemon effect* move their execution
  to the daemon in the milestone that implements this ADR; commands whose
  kind is *frontend effect* keep executing in the TUI, now against the
  declared frontend-action set rather than an open enum. The TUI's own
  behaviour is unchanged in observable outcome — the migration is where the
  execution happens, not what the user sees.
- **Desktop.** The desktop gains the command surface; it has none to
  migrate. Its existing composer remains a plain text input until the
  implementing milestone lands.
- **Terminology and canonical documents.** Amended before the protocol, per
  the Decision above. An amendment to `plans/001-terminology-and-domain-model.md`
  is an addition of definitions, not a change to existing ones, so no
  existing canonical term is redefined.

## Security and reliability implications

- **Authorization.** Every daemon-effect command is authorized in the daemon
  under the capability the catalog names. The renderer cannot widen a
  command's capability, because the capability is a property of the catalog
  entry and the daemon re-checks it server-side
  (`plans/000-long-term-specification.md:580`, `:602`).
- **The frontend-action set is the new trust boundary.** A frontend-effect
  command's handler set is finite, declared, and reviewable. It must not
  contain anything that mutates daemon state, and it must not be
  extensible at runtime by a plugin or an untrusted source. The guard for
  this must fail closed if a frontend action is added that is not in the
  declared set.
- **Free text.** Unrecognized input stays prompt text
  (`plans/000-long-term-specification.md:510`). There is no path by which
  typed text becomes a privileged operation without a recognized command
  identity.
- **Audit.** Daemon-effect commands produce `Run` records. A command that is
  refused is a refusal with a reason, not a silent no-op.
- **Boundedness.** The catalog response is bounded in command count and
  payload size, and a command execution response is bounded by the same
  rules that bound every other protocol payload
  (`plans/000-long-term-specification.md:538`).
- **Failure semantics.** A daemon-effect command that fails must report a
  typed failure, not a timeout that the frontend retries silently. A
  frontend-effect command cannot fail against the daemon, because it does not
  reach it.

## Verification

- A guard proving the protocol DTO's command kinds and the renderer-facing
  mirror agree, carrying a `--self-test` that injects each disagreement class
  and proves the guard rejects it.
- A guard proving the frontend-action set is finite, declared, and contains
  no daemon-mutating effect, with a `--self-test`.
- A guard proving no `CoreRequest` variant executes a command without
  going through the catalog, with a `--self-test`.
- Tests proving the TUI's daemon-effect commands still produce their
  documented outcomes after their execution moves to the daemon, and that
  the TUI's frontend-effect commands still work.
- Tests proving the desktop's palette lists the daemon's catalog, that a
  command issued from the desktop is authorized in the daemon and produces a
  `Run`, and that an unauthorized command is refused with a reason.
- A test proving that free text which resembles a command is treated as
  prompt text.
- A test proving the catalog response is bounded and that an oversized
  catalog is refused rather than truncated silently.

## Supersession

None. This ADR extends ADR-0010 and ADR-0011 without revisiting them, and
is bound by ADR-0012's authorization conclusions for anything that reads a
file.

If the owner decides the desktop should expose only a **subset** of the
TUI's commands and that subset should be frontend-owned rather than
daemon-owned, this ADR is superseded by that decision and M007-D is rescoped
accordingly. The driving constraint — that the TUI and the desktop must not
drift — survives either choice; only the ownership moves.
