# ADR-0057: Daemon-Owned Plugin Catalogue and Registry Ownership

Status: proposed — not accepted; this record grants no capability

Date: 2026-10-29

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md` §4.4 — "Frontends render projections
  and do not own durable state" (`plans/000-long-term-specification.md:95`)
- `plans/001-terminology-and-domain-model.md` — the `Command` and `Run` terms
- `architecture/command.md` — the command catalogue and cross-frontend
  execution ownership
- `architecture/plugin.md` — the plugin runtime, capability-based registry,
  service and management UX (`architecture/plugin.md:44`, `:88`, `:164`, `:243`)
- `architecture/writer_permit.md` — the checkout writer fence (C003) and the
  shared checkout profile (C006)

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — M007-G
  ("The daemon's plugin catalogue"), which this record gates.

Related **accepted** ADRs: ADR-0010, ADR-0011, ADR-0012, ADR-0052, ADR-0053
and ADR-0056. This record does not rewrite or supersede any of them.

This number was **reserved** for exactly this decision on 2026-10-27:
`plans/registry.md:1165` records that "**ADR-0057** is reserved for the
ownership decision and the record is **not yet written**", and the M007-G
roadmap row (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:568`)
records the same reservation. `scripts/check_adr_integrity.py` checks the ADR
filename pattern (`:14`), that active ADR numbers are unique (`:29-34`), and
that every `plans/adrs/ADR-NNNN-*.md` reference in the repository resolves to
an active record (`:47-58`).

## Context

The daemon owns the command catalogue; frontends issue `CoreRequest` variants
and never compile a second list. The catalogue's `kind` field has exactly two
values, `DaemonEffect` and `FrontendEffect` (`crates/codegg-core/src/command_catalog.rs:36-44`,
whose doc comment reads *"The two command kinds ADR-0052 defines. There are
exactly two."*). The kind lives on the **form**
(`crates/codegg-core/src/command_catalog.rs:138`), and the decision it drives
is made in one place, `route_for`
(`src/tui/command_route.rs:138-162`): a `FrontendEffect` form routes to a
declared `FrontendAction`, and a `DaemonEffect` form routes to the daemon only
if the daemon's effect table serves it, otherwise to `NotServed`.

Seven plugin forms exist in the catalogue. Each has exactly one form, the
empty default form, and **all seven are declared `DaemonEffect`** with
`capability: Some(CommandCapability::CommandExecute)`:

| `command_id` | name | catalogue entry | TUI handler |
|---|---|---|---|
| `plugins` | `/plugins` (aliases `/plugin-list`, `/plugin-ls`) | `crates/codegg-core/src/command_catalog.rs:2633` | `src/tui/app/mod.rs:8720` |
| `plugin_info` | `/plugin-info` | `crates/codegg-core/src/command_catalog.rs:2651` | `src/tui/app/mod.rs:8923` |
| `plugin_enable` | `/plugin-enable` | `crates/codegg-core/src/command_catalog.rs:2669` | `src/tui/app/mod.rs:8933` |
| `plugin_disable` | `/plugin-disable` | `crates/codegg-core/src/command_catalog.rs:2687` | `src/tui/app/mod.rs:8943` |
| `plugin_doctor` | `/plugin-doctor` | `crates/codegg-core/src/command_catalog.rs:2705` | `src/tui/app/mod.rs:8949` |
| `plugin_remove` | `/plugin-remove` | `crates/codegg-core/src/command_catalog.rs:2723` | `src/tui/app/mod.rs:8958` |
| `plugin_install` | `/plugin-install` | `crates/codegg-core/src/command_catalog.rs:2741` | `src/tui/app/mod.rs:8968` |

The daemon serves **none** of them. Its command effect table
(`src/core/daemon_commands.rs:1454`) has **no** plugin row — a grep for
`"plugin` across that file returns zero matches — so `serves_form`
(`src/core/daemon_commands.rs:778-788`) is false for all seven and the TUI
keeps its local arm. The repository asserts this itself: `route_for("plugin_install", "")`
is `CommandRoute::NotServed` at `src/tui/command_route.rs:295`, under the
comment *"The plugin family has no daemon request surface."* The generated
classification artifact records the same for each form
(`plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md:213-219`:
*"daemon → not served (the TUI keeps its local arm)"*), and classifies all seven
as **misclassified** at `:593-599`.

**There is no daemon request surface for plugins at all.** No `CoreRequest`
variant mentions a plugin (`crates/codegg-protocol/src/core.rs` — the only
plugin-named variant is the *event* `CoreEvent::PluginUiEffect` at `:5006`).
No server route mentions plugins (a grep across `src/server/` returns no
matches). The client crate mentions plugins only in capability-flag defaults
(`crates/codegg-client/src/lib.rs:127-134`). The desktop mentions a plugin
only as a skill-source label (`apps/desktop/src/bridge-types.ts:382`).

**The daemon builds a fresh registry per turn, and the TUI holds a second
instance.** `create_default_plugin_service`
(`src/plugin/mod.rs:79`) constructs a brand-new `PluginRegistry`
(`src/plugin/mod.rs:80`) on every call. The daemon calls it **per turn**
(`src/core/daemon_turns.rs:909`, inside the turn-execution path of
`handle_turns_request` at `src/core/daemon_turns.rs:274`) and again on every
asset refresh (`src/core/daemon_refresh.rs:547`). The TUI calls the same
factory at startup and wraps it in its own `PluginManager`
(`src/tui/app/mod.rs:1900-1907`). So there are **two independent in-memory
registries** over one shared durable file: the activation store at
`<daemon root>/plugin-activation.json` (`src/core/instance.rs:186-188`, root
resolved at `src/core/instance.rs:98-101`). The two can disagree, and the
store's own doc comment says *"The daemon singleton makes one process the
normal writer"* (`src/plugin/activation.rs:138-140`) — a singleton that does
not exist.

The plugin registry has a service type of its own, `PluginService`
(`src/plugin/service.rs:24`), which is the plugin analogue of
`egglsp::LspService`. It is not in `codegg-core`, it has no daemon-owned
singleton, and nothing in the daemon holds one.

**The registry's own recorded finding is that "a declared `FrontendAction`
each" is not implementable for the four mutating forms.**
`scripts/check_frontend_action_set.py` admits exactly two dispositions,
`ALLOWED_DISPOSITIONS = {"NonMutating", "ProcessLaunching"}`
(`scripts/check_frontend_action_set.py:68`), and the disposition vocabulary
itself has exactly two members (`crates/codegg-core/src/frontend_action.rs:24-29`),
documented as *"Every action in this set is **non-mutating** with respect to
daemon-owned state"* (`crates/codegg-core/src/frontend_action.rs:18-21`).
The guard rejects any other disposition
(`scripts/check_frontend_action_set.py:284-289`), and its self-test injects a
mutating disposition to prove the rejection is not vacuous (`:451-456`).
`/plugin-enable`, `/plugin-disable`, `/plugin-remove` and `/plugin-install`
are mutating: they rewrite the activation file
(`src/plugin/management.rs:329`, `:354`, `:423`, `:452-473`,
`src/plugin/activation.rs:178`, `:239-255`), copy into the canonical plugins
directory (`src/plugin/management.rs:512`, `src/plugin/install.rs:33-38`), and
delete from it (`src/plugin/management.rs:576`). Their recorded disposition
is therefore fixed: they **can only be carried as daemon effects with a
canonical `Run` under ADR-0052**, never as frontend actions. This is the
registry's own statement at `plans/registry.md:1165`, recorded 2026-10-27.

## Decision drivers

1. **The frontend-action vocabulary is closed, and a mutating form cannot be
   carried by it.** `scripts/check_frontend_action_set.py:68` admits only
   `NonMutating` and `ProcessLaunching`; `:326-330` forbids a declared action
   from serving a daemon-effect form; `:333-345` requires every frontend-effect
   form to be served by exactly one declared action. A mutating form
   therefore **must** be a daemon effect with a canonical `Run` under ADR-0052,
   unless the owner widens the disposition vocabulary — which is itself a
   decision this record would have to make explicitly.
2. **The plugin registry's owner is a new ownership boundary, and ADR-0052
   does not cover it.** ADR-0052 decides cross-frontend *command execution*
   ownership; it does not decide *who owns the plugin registry*. Two
   registries exist today and can disagree. That is why this number was
   reserved (`plans/registry.md:1165`) rather than folded into an existing
   record, and why the milestone row names an ADR
   (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:568`).
3. **One authority, or a recorded reason for two.** ADR-0056's rule is the
   precedent: *"The daemon's command catalogue continues to publish
   `daemon_served` truthfully. When implementing a form, wire the native
   request, authorize it, test it end-to-end and only then advertise it as
   served"* (`plans/adrs/ADR-0056-daemon-lsp-command-and-runtime-control-surface.md:66`).
   A form that is advertised as daemon-served and is not, or advertised as a
   frontend effect and is neither, is a lie the catalogue cannot carry.
4. **The four mutating forms write to user-scoped roots, not to checkouts.**
   The activation file lives under the daemon home
   (`src/core/instance.rs:186-188`); the plugin install directory is
   `dirs::data_local_dir()/codegg/plugins` (`src/plugin/install.rs:33-38`).
   **No plugin form touches a checkout.** The checkout writer permit (C003)
   and the shared checkout profile (C006) are therefore **not** the
   mechanism here; the `Run` record and the writer-disposition manifest are.
   The manifest already has the vocabulary for exactly this shape:
   `scope = "non-checkout"` with `disposition = "out-of-contract"` and
   `mediation = "none"`, as `src/core/instance.rs` demonstrates
   (`docs/writer-disposition.toml:608-619`).
5. **A plugin-contributed command surface stays out of scope.** ADR-0052
   defers it and this record does not revisit that conclusion
   (`plans/registry.md:1165`).

## Options

**A. The split — the read-only three become frontend effects, the mutating
four become daemon effects, and the daemon's registry becomes the single
authority.**

The three read-only forms (`/plugins`, `/plugin-info`, `/plugin-doctor`)
change `kind` to `FrontendEffect` and gain declared `FrontendAction`s with
the `NonMutating` disposition, fed by a plugin catalogue the daemon
publishes. The four mutating forms stay daemon effects and gain daemon
requests under ADR-0052, each producing a canonical `Run`. The TUI's second
`PluginManager` (`src/tui/app/mod.rs:1900-1907`) is retired in daemon mode,
and the daemon's registry becomes a process-lifetime singleton rather than a
per-turn construction.

*Cost to the writer-disposition story:* one new root for the activation store
and one for the install directory, both `scope = "non-checkout"`,
`disposition = "out-of-contract"`, `mediation = "none"` — the shape
`src/core/instance.rs` already uses. No checkout mediation, no C003 fence, no
C004 gate. **Cost to `Run` semantics:** the read-only three produce **no**
`Run` (a frontend action does not), so the daemon's plugin catalogue must
reach the frontend by a channel other than `CommandExecute` — which is a
published-projection question, and ADR-0053 has already decided the
daemon's published projection surfaces (`plans/adrs/ADR-0053-daemon-published-projection-surfaces.md`).
ADR-0056 says *"No other accepted ADR is amended"* (`:85`).

**B. All seven as daemon effects.**

One ownership path: the daemon serves every form through `CommandExecute`,
each with a canonical `Run`, and the TUI and the desktop both render the
daemon's answer. The catalogue's `kind` for all seven stays `DaemonEffect`,
so no `FrontendAction` is declared and `check_frontend_action_set.py`'s
closure rule is untouched.

*Cost to the writer-disposition story:* the same two new roots as A.
*Cost to `Run` semantics:* **every** invocation, including a read-only
`/plugins` list, produces a canonical `Run` — a durable record for a read.
That is heavier than the effect requires, and ADR-0056 explicitly declined
to make keystrokes into durable command runs (`:33`); the analogous concern
here is a poll becoming a `Run`. *Cost to ADR-0053:* none — no new projection
surface is needed, because the daemon's answer arrives as the `CommandExecute`
response.

**C. The frontend keeps its own registry.**

The cheapest: no daemon request, no new capability, no projection, no
manifest root. The catalogue's `kind` for the seven stays as it is, or is
corrected to `FrontendEffect` with declared actions — but the actions would
read the **TUI's own** `PluginManager`.

*Cost:* **two authorities persist and can disagree**, which is the status quo
the milestone exists to end. The daemon's per-turn registry
(`src/core/daemon_turns.rs:909`) and the TUI's long-lived one
(`src/tui/app/mod.rs:1904`) would continue to differ. The desktop has no
plugin surface at all (`apps/desktop/src/bridge-types.ts:382` is a skill
label), so the desktop would have to build its own third registry, or render
nothing. This option does not deliver the milestone's stated goal — *"The
daemon's plugin registry becomes the authority for the catalogue"* — and it
would make the roadmap row's claim of a correction false rather than fix it.

## Decision

**The decision is pending the project maintainer. This record is `proposed`
and grants no capability.**

This record **recommends Option A (the split)**, for these reasons:

- It is the only option that satisfies the closed frontend-action vocabulary
  (driver 1) without widening it: the three read-only forms fit `NonMutating`
  exactly, and the four mutating forms are carried as daemon effects with
  canonical `Run`s under ADR-0052, which is also the disposition the registry
  already recorded on 2026-10-27 (`plans/registry.md:1165`).
- It delivers the milestone's stated goal — one authority — while keeping the
  two writer surfaces on the correct side of the checkout boundary (driver 4).
- It refuses the alternative that would make a read a durable `Run` (Option B)
  unless the owner prefers a single ownership path over that cost.

**What the owner must still decide, and what this record does not decide:**

1. **How the daemon publishes the plugin catalogue** that the three
   read-only frontend actions consume — as a new published projection surface
   (which touches ADR-0053's decided set), as a dedicated request, or not at
   all. Option B sidesteps this by making all seven daemon effects.
2. **Whether the daemon's plugin registry becomes a process-lifetime
   singleton**, and what happens to the TUI's `PluginManager` in standalone
   (non-daemon) mode, where the TUI must still work.
3. **Which capability authorises the four mutating forms.** `CommandCapability`
   has exactly one member, `command.execute`
   (`crates/codegg-core/src/command_catalog.rs`), and the role capability
   vocabulary has 22 members with **no** plugin capability
   (`crates/codegg-core/src/team.rs:249-294`). ADR-0056's precedent for the
   LSP controls was a narrow, explicitly-granted `lsp.control`
   (`plans/adrs/ADR-0056-...:38-40`); the analogue would be a narrow
   `plugin.manage`, granted to named roles and not inferred from
   `command.execute`. Whether to mint one is the owner's decision.
4. **Who is the single writer of `plugin-activation.json`** — the daemon
   becoming the sole writer (so the TUI's activation writes become daemon
   requests), the file being split into daemon-global and frontend-local
   scopes, or an inter-process lock being added.

**Nothing is implemented, and no capability, request, projection, catalogue
change or manifest entry is granted, until the owner accepts this record.**
Acceptance is recorded in the `Status:` line above, in the manner of
`plans/adrs/ADR-0056-...:3` (*"Status: accepted — Option B accepted
2026-10-28 by the project maintainer"*) and ADR-0030's proposed form
(`plans/adrs/ADR-0030-...:3`: *"Status: proposed — **not authorized for
implementation or user-facing selection**"*). Until then the M007-G roadmap
row stands as written: *"not started — ADR-0057 not yet written"*
(`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:568`).

## Consequences

**If Option A is accepted.** The catalogue's `kind` for `/plugins`,
`/plugin-info` and `/plugin-doctor` becomes `FrontendEffect`, their
`capability` becomes `None` (a wholly-frontend command may name no daemon
capability — `scripts/check_command_catalog_ownership.py:456-468`), and
three `FrontendAction` variants are declared with the `NonMutating`
disposition. The four mutating forms keep `DaemonEffect` and gain daemon
requests. The catalogue version is bumped, because it must be bumped whenever
the catalogue's contents change (`crates/codegg-core/src/command_catalog.rs:175-178`).
The TUI's second registry is retired in daemon mode. Benefit: one authority,
honest cross-frontend parity, and the desktop's palette gains the forms
automatically because it consumes the same catalogue. Cost: a new published
surface or a new request; a new capability; two new manifest roots; a
migration for the TUI's standalone mode.

**If Option B is accepted.** All seven forms are daemon effects. The
catalogue's `kind` does not change for them — they are already `DaemonEffect`
— so the milestone's "kind is corrected" phrasing does not describe this
option, and the roadmap row must be corrected. The desktop gains every form
with no new projection. Cost: a canonical `Run` per read-only invocation.

**If Option C is accepted.** Nothing changes. The milestone does not close,
the catalogue's classification stays misleading, and two authorities persist.

**Neutral in all options.** The four mutating forms' filesystem roots do not
change: the activation file and the plugin install directory are user-scoped
and outside every checkout, so the checkout writer permit (C003) and the
shared checkout profile (C006) are not the mechanism, and the TUI's own
`/plugin-install` and `/plugin-remove` arms already write there today
(`src/tui/commands/plugin_management.rs:254`, `:211`).

## Compatibility, security and migration

All catalogue and protocol extensions are additive and typed; an older client
that sees `daemon_served: false` and no `action` for a plugin form renders
the form as unavailable, exactly as it does today
(`crates/codegg-protocol/src/core.rs:1224-1250`: `daemon_served` and
`action` are both `#[serde(default)]`). The catalogue version bump makes any
in-flight `CommandExecute` carrying the old version fail with `StaleCatalog`
rather than execute against a changed classification — the intended
behaviour (`crates/codegg-core/src/command_catalog.rs:175-178`).

**Storage.** `plugin-activation.json` is user-scoped and atomically rewritten
(`src/plugin/activation.rs:250-254` writes a temp file and renames it). No
schema change is proposed. The plugin install directory's layout is unchanged.
**If the owner chooses to make the daemon the single activation writer**, the
TUI's standalone mode needs an honest path — the TUI cannot reach a daemon
that is not running, so the standalone TUI either keeps a local activation
writer (two writers again, and the disagreement returns) or becomes
read-only for plugin activation in standalone mode. That is a consequence to
be decided, not assumed.

**Security.** The four mutating forms change what the machine loads and runs:
a plugin's manifest, its capabilities and its permissions
(`crates/codegg-protocol/src/plugin.rs:10-129`). A daemon that owns the
registry is a daemon that owns the authority to install code — so the
capability that authorises them must be explicit and narrowly granted, and
the `Run` record must name the plugin identity and the effect. The install
copy must remain fenced to the canonical plugins directory
(`src/plugin/management.rs:562-575` documents the `validate_uninstall_target`
check), and an install must never be able to write into a checkout. The
read-only forms expose plugin metadata — names, versions, capabilities,
diagnostics — to the frontend, which is a smaller disclosure than the mutating
four but is still a disclosure the catalogue now attributes to the daemon.

**Reliability.** The daemon's per-turn registry construction
(`src/core/daemon_turns.rs:909`) means a plugin installed between two turns
is not in the next turn's registry, and a plugin *uninstalled* between two
turns still is. A process-lifetime singleton changes that, and the singleton's
own failure mode — a registry that cannot be built, currently logged at
`src/plugin/mod.rs:139` — must keep degrading the same way: *"plugin
activation state is unavailable; plugins disabled"*.

## Required verification

Before acceptance: the owner confirms the four decisions listed in
`## Decision`; a per-form disposition table is produced for all seven forms
in the manner ADR-0056 required of its 26 forms
(`plans/adrs/ADR-0056-...:60`), naming for each form its mechanism, its
authority, and its writer surface; and the implementation that follows must
satisfy:

- The catalogue's `kind` for each of the seven forms matches the accepted
  mechanism, and every frontend-effect form is served by exactly one declared
  action (`scripts/check_frontend_action_set.py`).
- The catalogue version is bumped, and a stale-version `CommandExecute` is
  refused with `StaleCatalog`.
- `daemon_served` is truthful for every plugin form: a form is advertised as
  served only after its request is wired, authorized, tested end to end, and
  its frontend consumer exists (`plans/adrs/ADR-0056-...:66`).
- The daemon's registry is a single instance, and the TUI's second instance
  is retired or its standalone-mode behaviour is recorded.
- The activation file has exactly one writer, proven by a test that shows two
  processes cannot diverge.
- Each of the four mutating forms has an explicit `docs/writer-disposition.toml`
  root with a legal `surface`, `scope`, `disposition`, `mediation` and
  `owner`, and `scripts/check_writer_disposition.py` passes with the plugin
  modules detected.
- Two-client parity: the TUI and the desktop see the same plugin catalogue
  and the same outcomes for the same form.
- A closure record exists, and no other accepted ADR is amended without an
  explicit owner decision.

Supersession: none.
