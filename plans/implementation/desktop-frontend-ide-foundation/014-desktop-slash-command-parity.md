# Desktop Frontend and IDE Foundation Milestone M007-D — Cross-Frontend Command Surface and Slash-Command Parity

Status: blocked

**Blocked on the acceptance of `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`.**
ADR-0052 is the controlling decision for this milestone and is **not yet accepted**; its owner is
the repository owner. This plan is written in advance of that acceptance **on purpose**: the
registry must record the planned milestone and its named blocker, and the plan must be executable
the moment ADR-0052 is accepted, without a second round of design. Nothing here may be executed
before ADR-0052 is accepted.

Repository baseline: `c4fa30aeba439894b048768d4829dea0cfa0b84b`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Controlling ADR (unaccepted, blocking): `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`
— **controlling**. The daemon owns the command catalog and executes daemon-effect commands; each
frontend dispatches frontend-effect commands from a bounded, declared set; command identity is
shared while display names may differ; execution is audited as a canonical `Run`; the terminology
document is amended before the protocol lands. This plan **implements** that decision and **does
not re-decide any part of it**. Where this plan would otherwise choose, ADR-0052 chooses.

Accepted ADRs this milestone is bound by: ADR-0010 (desktop frontend and shared client
boundary), ADR-0011 (editor document ownership and frontend replication), ADR-0012 (native LSP
read surface delivery and authorization — binds anything here that reads a file).

Long-term requirements:

- `plans/000-long-term-specification.md:341` — "The TUI **and protocol** SHOULD expose commands
  or views that answer:".
- `plans/000-long-term-specification.md:368-379` — "Manual commands MUST include a unified
  operation and focused aliases", the `/reload` / `/reload project` / `/reload skills` /
  `/agents refresh` examples, and "The final command names MAY differ, but there MUST be one
  discoverable manual command that refreshes all project runtime assets and returns a structured
  report of added, removed, changed, shadowed, and invalid entries."
- `plans/000-long-term-specification.md:217` — `command.execute` is a project capability.
- `plans/000-long-term-specification.md:580` — project authorization MUST be enforced server-side.
- `plans/000-long-term-specification.md:602` — "Authorization is enforced in the daemon, not
  solely in frontends."
- `plans/000-long-term-specification.md:510` — "Free text MUST NOT silently become an unaudited
  privileged command."
- `plans/000-long-term-specification.md:538` — explicit size bounds on payloads.
- `plans/001-terminology-and-domain-model.md:452-459` — the canonical `Run` record; `:759` — the
  "run" vs "command run" ambiguity prohibition; `:5` — terminology is normative for protocol
  types, schemas, docs, tests and UI labels.
- `plans/002-long-term-roadmap.md:17` — cross-phase rule 3: typed protocol and storage
  representations BEFORE frontend-only state; `:897-899` — terminology and the long-term
  specification are updated first, then the roadmap.
- `plans/003-planning-process.md:29-36` — when canonical documents may be amended; `:204-210` —
  authority order.

Applicable architecture: `architecture/command.md` (the TUI command surface and its count
claims), `architecture/command_routing.md` (the CLI's routing — a different surface),
`architecture/protocol.md`, `architecture/tui.md`, `architecture/desktop.md`,
`architecture/overview.md`, `architecture/authorization.md`, `architecture/lsp.md`.

Primary class: capability

> The command surface is a cross-frontend capability. It is not a UI feature and not a plugin
> framework, and the daemon-side catalog plus its guard are the deliverable, not a palette.

## 1. Objective

Deliver the command surface as a **daemon-owned catalog** that the TUI and the desktop both use:

- one catalog — in the daemon's own code — defining every command's identity, display name,
  aliases, one-line description, kind, required capability and argument shape (ADR-0052 item 1);
- a bounded protocol family that returns the catalog and executes a daemon-effect command
  (ADR-0052 item 2);
- a daemon executor that authorizes daemon-effect commands under `command.execute`, executes them
  under the daemon's existing authority, and records a canonical `Run` for each (ADR-0052 item 5);
- a finite, declared, reviewable **frontend-action set** that frontends dispatch locally for
  frontend-effect commands, guarded so it cannot grow an undeclared or daemon-mutating action
  (ADR-0052 item 3);
- the TUI migrated so daemon-effect commands execute in the daemon and frontend-effect commands
  dispatch against the declared action set, with **no change in observable outcome**;
- the desktop rendering the daemon's catalog — not a hand-written list — completing on `/`,
  dispatching frontend effects locally and daemon effects to the daemon, and showing the result.

One bounded outcome: **a user typing `/` in the desktop sees the daemon's catalog, and a
daemon-effect command typed in either frontend is authorized by the daemon and leaves a `Run`
behind — while the TUI's `/` surface behaves exactly as it does today.**

## 2. Why this milestone is ready

**This milestone is not ready.** It waits on exactly one thing: the acceptance of
`plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`. Everything the milestone
needs from that ADR — the two kinds, the shared identity, the `Run` audit, the terminology
ordering, the deferral of plugin and MCP commands — is already decided there. The milestone must
not begin catalog design, protocol naming, or TUI migration while that decision is open, because
a second decision about command ownership would put a plan-level invention above the accepted
ADR. §14 carries the stop conditions that follow.

Closed hard dependencies (satisfied): M001…M006-E are closed; the M006-A editor vertical slice is
the house template for a milestone of this shape
(`plans/implementation/desktop-frontend-ide-foundation/005-editor-buffer-vertical-slice.md`);
ADR-0010, ADR-0011 and ADR-0012 are accepted; the `document.v1` native surface is complete and
additive (`crates/codegg-protocol/src/core.rs:2317-2392`) and is the pattern for a bounded
additive protocol family.

The **asset refresh** family is the working precedent for a daemon-owned, bounded, audited
command effect: `AssetRefresh` / `AssetRefreshStatus` / `AssetRefreshCapabilities`
(`crates/codegg-protocol/src/core.rs:1273-1279`, `:2407`), with `AssetRefreshReportDto` already
carrying `added`, `removed`, `changed`, `shadowed` and `invalid`
(`crates/codegg-protocol/src/core.rs:124-152`) — exactly the structured report
`plans/000-long-term-specification.md:379` demands. `/reload` already reaches it
(`src/tui/app/mod.rs:5683` → `src/tui/commands/agents.rs:21` → `CoreRequest::AssetRefresh`).

Hard dependencies on other milestones (not yet satisfied):

- **M007-A — the desktop gate.** The desktop must be a working client with a composer before it
  can consume a command surface. M007-A's plan is
  `plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md` —
  authored against the same baseline `c4fa30aeba439894b048768d4829dea0cfa0b84b` and landing
  concurrently with this plan. Work package G cannot start until M007-A lands.
- **M007-C — the keymap.** The desktop's `/` completion and palette bind to M007-C's chord
  (`plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md`,
  landing concurrently). M007-C **reserves** the palette chord without implementing command
  execution — `mod+shift+p` is recorded as reserved for M007-D, and M007-C states that "the
  palette's command contents are M007-D's, blocked on ADR-0052" and that "binding the chord to an
  empty surface would be a dead chord"
  (`plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md:421`,
  `:591`). This milestone owns the command **contents** and **execution**; M007-C owns the chord.
  The palette must therefore land with this milestone's catalog, not before it.

Stable interface dependencies:

- `codegg_client::LocalSocketClient::request` takes a `RequestEnvelope<CoreRequest>` and returns
  `Result<CoreResponse, ClientError>` (`crates/codegg-client/src/local.rs:188-213`) — the request
  pattern the new family follows.
- `codegg_core::run_store` is the canonical `Run` store: `RunKind`, `RunStatus`, `RunManifest`,
  `RunDraft`, `RunHandle`, `RunCompletion` (`crates/codegg-core/src/run_store.rs:212-694`), with
  `FsRunStore` (`:1514`) and `MemRunStore` (`:1969`).
- `codegg_core::Capability::CommandExecute` already exists and is serialized as
  `command.execute` (`crates/codegg-core/src/team.rs:269-270`, `:331`, `:358`). **No new
  capability is invented by this milestone.**
- The desktop's Tauri host already has a named-command pattern with route fencing
  (`apps/desktop/src-tauri/src/lib.rs:888-901` for `desktop_prompt_submit`, `:919-926` for
  `desktop_control_refresh`), and the desktop's request pattern is `codegg-client`'s
  `LocalSocketClient` plus `SessionProjectionDriver` (`apps/desktop/src-tauri/src/lib.rs:15-18`).

## 3. Current implementation evidence

- **The command surface is entirely TUI-local.** `BuiltinSlashAction` occurs **nowhere outside
  `src/tui/`**:

  ```bash
  $ grep -rn "BuiltinSlashAction" --include="*.rs" . | grep -v "^./src/tui/"   # no output
  $ awk '/pub enum BuiltinSlashAction/,/^}/' src/tui/command.rs | grep -cE '^\s+[A-Z][A-Za-z0-9]*'
  161
  $ sed -n '5661,8895p' src/tui/app/mod.rs | grep -cE '^\s+B::[A-Za-z0-9]+'
  161
  ```

  The enum is declared at `src/tui/command.rs:112` and has **161** variants. The TUI's dispatcher
  `dispatch_builtin_command` (`src/tui/app/mod.rs:5661`) matches it **exhaustively**: the arm set
  and the variant set are identical, with no gap and no extra arm.

  **Recorded discrepancy.** ADR-0052's Context says "the dispatcher handles **86** of them by name
  (`src/tui/app/mod.rs:5661`); the remainder are reached through other TUI paths"
  (`plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md:35-37`). That is **not what
  the source says at this baseline**: the dispatcher has one arm per variant, all 161, and the
  remaining 75 variants are *not* reached by another path — they have no other route. The
  repository's own documentation agrees with the source: `architecture/command.md:15` records "169
  built-in commands", `architecture/command.md:169` records "161 variants,
  `src/tui/command.rs:112`", and `architecture/command.md:482-485` records both counts as
  re-measured. This plan cites the measured numbers. The discrepancy must be reconciled in
  ADR-0052 before acceptance; it makes the milestone's migration surface **larger** than
  ADR-0052's Context implies, not smaller.
- **There is no `CoreRequest` variant that executes a command.** `CoreRequest`
  (`crates/codegg-protocol/src/core.rs`) has **266** variants; none is a command execution.
  `CoreResponse` (`:1202`) has **174**.
- **The registry is 169 entries, not 161.** `CommandRegistry::built_in_commands`
  (`src/tui/command.rs:577-997`) yields **169** entries: **162** bound to a `BuiltinSlashAction`
  (one variant, `TuiStats`, carries two names, `/stats` and `/tui-stats`), plus **7** non-built-in
  entries — `/models`, `/mcps`, `/files`, `/keybinds`, `/review` are `CommandAction::Dialog`, and
  `/pr`, `/issue` are `CommandAction::Template`. Those 7 are **not** in scope here: a `Dialog`
  entry is a frontend presentation choice and a `Template` entry is a config/project template,
  both covered by ADR-0052's deferral of dynamic command sources.
- **The registry also carries dynamic sources.** `CommandRegistry::append_dynamic_commands`
  (`src/tui/command.rs:999-1043`) appends config commands and project `.command` files, with
  `CommandSource::{BuiltIn, Config, Project, Plugin { id }}` (`src/tui/command.rs:76-88`). These are
  the dynamic sources ADR-0052 defers (`…ADR-0052…md:297-301`).
- **The renderer has no command surface at all.** `apps/desktop/src/App.tsx` is 407 lines; the
  prompt is a plain `<textarea data-testid="prompt-input">` with a `Submit prompt` button
  (`apps/desktop/src/App.tsx:392`). There is **no** `onKeyDown` handler anywhere in
  `apps/desktop/src/App.tsx`, no completion overlay, no palette, and no `/` dispatch.
- **The TUI's `/` recognition is in three places.** `src/tui/app/prompt_turn.rs:36`, `:71` and
  `:116` each test `trimmed_text.starts_with('/')`. The typed path resolves the name through
  `CommandRegistry::find_by_name_or_alias` (`src/tui/command.rs:1212`) in `handle_slash_command`
  (`src/tui/app/mod.rs:9744-9773`), which returns `false` for an unrecognized name — the free-text
  rule already holds in the TUI.
- **The TUI has the two surfaces to mirror.** The completion overlay is
  `src/tui/components/completion_overlay.rs` (`CompletionOverlay`, `CompletionItem`, `:28-225`);
  the command dialog is `src/tui/components/dialogs/command.rs` (`CommandPalette`, `:15-101`).
  Both stay TUI-local and are not moved by this milestone.
- **Most daemon-effect commands already reach the daemon.** The TUI's command channel `TuiCommand`
  (`src/tui/app/commands.rs:52`) is consumed by `src/tui/runtime/command_dispatch.rs`, whose
  handlers submit ordinary `CoreRequest`s — `TuiCommand::RefreshAssets` → `start_refresh_assets` →
  `CoreRequest::AssetRefresh` (`src/tui/runtime/command_dispatch.rs:153`,
  `src/tui/commands/agents.rs:21-51`); the terminal family submits
  `CoreRequest::InteractiveProcess*` (`src/tui/commands/interactive_terminal.rs:203`, `:404`); the
  plugin family submits plugin requests and receives `TuiCommand::Plugin*Finished`
  (`src/tui/commands/plugin_management.rs:33-295`). The migration is therefore mostly a **uniform,
  audited route over existing daemon handlers**, not a rewrite of 130 command implementations.
- **A small, verified set is not daemon-routed today.** Six commands are frontend-local while
  their effect is daemon-owned, and they seed the Work package F inventory: `Compact` (the arm
  only toasts at `src/tui/app/mod.rs:5958`; `TuiCommand::CompactSession` is declared at
  `src/tui/app/commands.rs:282` and handled at `src/tui/runtime/command_dispatch.rs:773` but **has
  no sender**), `Status` (formats from frontend state at `src/tui/app/mod.rs:5971`), `State`
  (formats from `self.session_state_derived` at `src/tui/app/mod.rs:7389`), `Logs` (reads the
  daemon's log file directly from the frontend's filesystem at `src/tui/app/modal.rs:124-144`),
  `Skills` (formats from the projection snapshot at `src/tui/app/mod.rs:5689`), and the bare
  `/agents` form (opens a dialog at `src/tui/app/mod.rs:5824`, while its `show` / `--all` /
  `diff` / `validate` / `reload` / `rebuild` subcommand forms are daemon-routed).
- **The LSP family's frontend tool is absent in the default mode.** `app.lsp_tool` is `None` in
  socket mode (`src/tui/commands/change_review.rs:659`, `architecture/lsp.md:929-930`, `:987`), so
  every `/lsp-*` arm already refuses in the daemon-connected configuration. The authoritative LSP
  service is daemon-owned (`architecture/lsp.md:81-84`).
- **The desktop boundary guard constrains the host bridge's shape.**
  `scripts/check-desktop-boundary.sh:23-26` rejects `core_request`, `read_file`, or
  `invoke.*shell` anywhere in `apps/desktop/src` or `apps/desktop/src-tauri/src` — a **generic
  machine-authority bridge is forbidden**. `:19-22` rejects `shell|filesystem|fs:|http:default|
  process:|updater|clipboard|global-shortcut` in the renderer capability, `:92` asserts
  `capability["permissions"] == []`, and `:9-12` pins the root MSRV at 1.89. The new host surface
  must therefore be **named commands**, not a generic request passthrough.
- **The desktop already has a Rust↔TS bindings check to follow.**
  `apps/desktop/scripts/check-bindings.mjs` is the established pattern for a generated mirror of a
  Rust DTO into TypeScript; ADR-0052 item 2 names it as the model for a generated catalog mirror.
- **`RunKind` has no `Command` variant.** `codegg_core::run_store::RunKind`
  (`crates/codegg-core/src/run_store.rs:212-220`) is `#[serde(rename_all = "snake_case")]` with
  eight variants (`raw_shell`, `managed_process`, `test`, `git_read`, `git_mutation`, `search`,
  `python`, `native_tool`). A daemon-effect command's `Run` needs a ninth, `command`.
- **The run store is not SQLite.** `FsRunStore` writes manifests to the filesystem
  (`crates/codegg-core/src/run_store.rs:1514-1968`); the SQLite session schema
  (`crates/codegg-core/src/session/schema.rs`) has `agent_run` (`:2020`) and
  `runtime_asset_refresh` (`:1885`) but no generic `run` table. `STORAGE_LAYOUT_VERSION` is **88**
  (`crates/codegg-core/src/storage/mod.rs:50`) and the highest wired migration is `migrate_v88`
  (`crates/codegg-core/src/session/schema.rs:3699`).

## 4. Invariants that must not regress

1. **Authorization is daemon-side.** Every daemon-effect command is authorized in the daemon
   under the capability the catalog names, and the daemon re-checks it server-side
   (`plans/000-long-term-specification.md:602`, `:580`; ADR-0052 items 3 and 5). The renderer
   cannot widen a command's capability.
2. **Free text never becomes a privileged command.** A composer line that is not a recognized
   command is prompt text. No implicit dispatch, no fuzzy fall-through, no "the frontend guessed"
   (`plans/000-long-term-specification.md:510`; ADR-0052 item 6). The TUI's existing behaviour at
   `src/tui/app/mod.rs:9744-9773` is the reference.
3. **The TUI's command names, aliases and observable outcomes are unchanged.** ADR-0052 item 4:
   "The TUI's existing `/` names are unchanged by this ADR; it keeps its aliases and its
   dispatcher." The TUI's registry keeps its 169 entries, aliases and descriptions.
4. **One catalog.** The daemon's catalog is the only definition of what commands exist. No
   frontend compiles a second list of commands, and the renderer holds no catalog of its own.
5. **The frontend-action set is the trust boundary.** It is finite, declared and reviewable; it
   contains nothing that mutates daemon state; it is not extensible at runtime by a plugin or an
   untrusted source (ADR-0052, "Security and reliability implications").
6. **The `Run` record stays canonical.** A daemon-effect command's audit record is the canonical
   `Run` (`plans/001-terminology-and-domain-model.md:452-459`), not a bespoke command log, and
   "command" and "Run" are not interchangeable (`plans/001-terminology-and-domain-model.md:759`).
7. **The desktop boundary guard keeps its meaning.** `scripts/check-desktop-boundary.sh` is not
   weakened, reworded, or given an exemption for the new host commands.
8. **`codegg-core` stays UI-free.** The catalog is plain data
   (`scripts/check-core-boundary.sh`); it must not reference `crate::tui`, ratatui, or any
   frontend type.
9. **One bounded protocol surface.** The catalog response and the execution response are bounded
   in command count, form count and payload size (`plans/000-long-term-specification.md:538`;
   ADR-0052 item 2). An over-bound catalog is **refused**, not silently truncated.
10. **The frontend never silently retries a daemon-effect command.** A failed or refused command
    reports a typed outcome to the user; it is not re-sent behind their back.
11. **Existing TUI behaviour outside the command surface is unchanged.** The editor, the
    permission dialog, the composer, the projection and the scheduler are untouched by this
    milestone except where a command's effect moves.

## 5. Scope

### In scope

- **The classification** of every one of the 161 `BuiltinSlashAction` variants — and every
  argument form of those variants — as a daemon effect or a frontend effect, recorded in a
  committed, reviewable artifact, with the review and the misclassification catch named (§7 WP-A).
- **The daemon-owned catalog**: identity, display name, aliases, one-line description, kind,
  required capability, argument shape, and forms. One definition, in the daemon's own code.
- **The protocol family**: a bounded request that returns the catalog, and a bounded request that
  executes a daemon-effect command, with their DTOs and bounds.
- **The daemon executor**: authorization under `command.execute`, execution through the daemon's
  existing authority, and a canonical `Run` record per execution.
- **The declared frontend-action set** and the guard that fails closed when an undeclared or
  daemon-mutating action appears.
- **The TUI migration**: daemon-effect commands route their execution to the daemon;
  frontend-effect commands dispatch against the declared action set. Observable outcomes
  unchanged, with regression coverage.
- **The desktop consumption**: the renderer shows the daemon's catalog, completes on `/`,
  dispatches frontend effects locally and daemon effects to the daemon, and shows the result.
- **Auditing**: a `Run` per daemon-effect execution; a typed refusal reason for a refusal.
- **Guards with `--self-test`**, and tests on both frontends.

### Explicitly out of scope

- **Plugin- and MCP-contributed commands.** ADR-0052 defers them explicitly: "A dynamic command
  source has different trust properties from a built-in one and must not be folded into this
  decision by analogy" (`…ADR-0052…md:297-301`). The TUI's config commands, project `.command`
  files and plugin commands (`src/tui/command.rs:999-1043`, `CommandSource::{Config, Project,
  Plugin}`) stay **TUI-local and outside the daemon catalog**; the desktop does not expose them.
  This milestone **stops** rather than folding them in — whether a dynamic command may be a
  daemon effect at all is a separate owner decision.
- **The command palette's visual design.** M007-C owns the keymap and the chord; ADR-0052's
  "Neutral or deferred" section says the palette's appearance "belongs to the frontend milestone
  that consumes this decision". This milestone owns the command **contents** and **execution**.
- **Any new authorization model.** `command.execute` is the canonical capability
  (`plans/000-long-term-specification.md:217`; `crates/codegg-core/src/team.rs:269-270`). No new
  capability, scope, or policy.
- **Any change to the TUI's command names or aliases.**
  `plans/000-long-term-specification.md:379` allows the final names to differ, but the TUI's
  existing names and aliases are preserved (ADR-0052 item 4). The desktop may display a
  different name for the same identity.
- **A second command catalog.** One catalog — not one in `codegg-core` plus one in the TUI plus
  one in the renderer. The TUI's registry becomes a **cache of the daemon's catalog** plus its own
  local dynamic sources (§7 WP-F).
- **The editor's own commands** (M007-E); **a third-party or bundled command framework** of any
  kind; **the ACP boundary**, which ADR-0052 leaves untouched.
- **Any change to the daemon's scheduler, authorization model, or provider ownership.** The
  command executor reuses them; it does not modify them.
- **The 7 non-built-in registry entries** (`/models`, `/mcps`, `/files`, `/keybinds`, `/review` as
  dialogs; `/pr`, `/issue` as templates). They are neither daemon-effect nor frontend-effect
  commands in ADR-0052's sense, and the desktop does not gain them here.

## 6. Required production changes

### Core/domain

- A new pure-data module `crates/codegg-core/src/command_catalog.rs` holding `CommandKind {
  DaemonEffect, FrontendEffect }` (exactly two kinds, ADR-0052 item 3); `CommandId` (a stable
  identity string, not a display name); `CommandDescriptor { command_id, name, aliases,
  description, domain, kind, capability, forms }`; `CommandForm { form, kind, argument_hint,
  argument_required }` — the classification unit is the **form**, because a command's argument can
  select a different effect (WP-A); `CommandCapability`, resolved from the existing `Capability`
  enum, where `command.execute` is the only capability any daemon-effect command may name in this
  milestone; and `the_catalog() -> &'static CommandCatalog` — the single definition.
- `codegg-core` gains a `RunKind::Command` variant
  (`crates/codegg-core/src/run_store.rs:212-220`), serialized as `command`. This is the only
  domain addition.
- `codegg-core` gains the **declared frontend-action set**: a finite `FrontendAction` enum plus a
  checked-in declaration of each action's non-mutating disposition.

### Storage and migrations

- **No new table, no migration, no `STORAGE_LAYOUT_VERSION` change.** The `Run` record is the
  existing `codegg_core::run_store` manifest (`RunManifest`, `RunDraft`, `RunHandle`,
  `RunCompletion`), written by `FsRunStore` (`crates/codegg-core/src/run_store.rs:1514-1968`) or
  `MemRunStore` (`:1969`). `RunKind` is a `serde`-serialized enum inside that manifest, so adding
  `RunKind::Command` is additive to a filesystem artifact and touches no SQLite schema.
- `STORAGE_LAYOUT_VERSION` stays **88** (`crates/codegg-core/src/storage/mod.rs:50`). The rule
  enforced by `scripts/check_project_catalog_invariants.py:292-295` — the marker must track the
  highest wired migration in `crates/codegg-core/src/session/schema.rs` (`migrate_v88`, `:3699`) —
  is therefore satisfied trivially. **If any implementation adds a SQLite table for command
  audit, the layout version must move with it; that is a change of plan, not a detail.**
- The audit trail beyond the `Run` — the principal that issued it (ADR-0052 item 5) — rides in the
  `Run` manifest, not in a new table.

### Protocol and DTOs

New variants in `crates/codegg-protocol/src/core.rs`:
`CoreRequest::CommandCatalog { scope: CommandCatalogScopeDto }`,
`CoreRequest::CommandExecute { request: CommandExecuteRequestDto }`,
`CoreResponse::CommandCatalog { catalog: CommandCatalogDto }`,
`CoreResponse::CommandExecute { result: CommandExecuteResultDto }`.

| DTO | Shape | Bound |
|-----|-------|-------|
| `CommandCatalogScopeDto` | `{ project_id, workspace_id }` | mirrors `AssetRefreshScopeDto` (`core.rs:89-93`) |
| `CommandCatalogDto` | `{ catalog_version: u64, commands: Vec<CommandDescriptorDto> }` | ≤ 256 commands, ≤ 512 forms, ≤ 256 KiB total; **over-bound is refused, not truncated** |
| `CommandDescriptorDto` | `{ command_id, name, aliases, description, domain, kind, capability, forms }` | description ≤ 200 bytes; ≤ 8 aliases; `command_id` ≤ 64 bytes |
| `CommandFormDto` | `{ form, kind, argument_hint, argument_required }` | ≤ 32 forms per command |
| `CommandKindDto` | `{ DaemonEffect, FrontendEffect }` | exactly two values |
| `CommandExecuteRequestDto` | `{ scope, command_id, form, argument, catalog_version }` | `argument` ≤ 4 KiB |
| `CommandExecuteResultDto` | `{ run_id, command_id, status, summary, lines, report, started_at_ms, completed_at_ms }` | ≤ 256 lines and ≤ 64 KiB |
| `CommandOutcomeStatusDto` | `{ Completed, Refused, Failed, Cancelled }` | — |
| `CommandReportDto` | `{ added, removed, changed, shadowed, invalid }` | mirrors `AssetRefreshReportDto` (`core.rs:124-152`) so `/reload` satisfies `plans/000-long-term-specification.md:379` unchanged |

`catalog_version` is a monotonically increasing `u64` bumped whenever the catalog's contents
change. A `CommandExecute` carrying a `catalog_version` different from the daemon's current one
is **refused** with `StaleCatalog` and the current version — the frontend refetches and re-issues,
and never guesses.

### Runtime and concurrency

- A new daemon request family module `src/core/daemon_commands.rs`, following the existing
  per-family shape (`src/core/daemon_assets.rs`, `src/core/daemon_documents.rs`,
  `src/core/daemon_jobs.rs`).
- `src/core/daemon_family.rs` gains a `DaemonFamily::Commands` arm mapping `CoreRequest::
  CommandCatalog` and `CoreRequest::CommandExecute`, and the family must appear in the
  project-scoping map in `src/core/daemon.rs` beside the other families.
- The executor's dispatch is **table-driven**: each daemon-effect form maps to an existing daemon
  handler (or a small adapter over one). The executor authorizes under `command.execute`, opens a
  `Run` via `RunStore::begin_run` (`crates/codegg-core/src/run_store.rs:1515`), executes, and
  closes the run via `RunCompletion`.
- **Concurrency:** one `CommandExecute` is one `Run`; two concurrent executions of the same
  command identity are two `Run`s and are not serialized against each other unless the command's
  own authority serializes it. The executor holds no lock of its own and introduces no scheduler
  or admission change.
- **Ownership:** a command's execution is daemon work; the frontend waits for the result. The
  TUI's slow-handler discipline applies to the new round trip: it goes through the TUI's existing
  async-task completion path, not a blocking call on the UI thread.

### Frontend or operator surface

**Tauri host** (`apps/desktop/src-tauri/src/lib.rs`) gains **two** named `#[tauri::command]`
functions, taking the host from 20 to 22: `desktop_command_catalog(scope) -> CommandCatalogView`
and `desktop_command_execute(scope, command_id, form, argument, catalog_version) ->
CommandExecuteView`. Both are **named** commands, never a generic request passthrough, because
`scripts/check-desktop-boundary.sh:23-26` rejects `core_request` in the host source. Both go
through the same route-token fencing the other 20 commands use (`apps/desktop/src-tauri/src/route.rs`,
`apps/desktop/src-tauri/src/control.rs`).

**Renderer** (`apps/desktop/src/App.tsx`, `apps/desktop/src/bridge.ts`,
`apps/desktop/src/bridge-types.ts`): `bridge.ts` gains `commandCatalog()` and
`commandExecute(...)` wrappers; `bridge-types.ts` gains the DTO types, **generated** from the Rust
DTO and checked by `apps/desktop/scripts/check-bindings.mjs` — a hand-written mirror is forbidden;
`App.tsx` gains a `/` completion overlay fed by the daemon's catalog, a command palette that lists
the daemon's catalog, and a submit path that resolves the typed line to a catalog entry —
dispatching a frontend effect locally and a daemon effect to the daemon. The renderer holds **no**
command list, **no** command names, and **no** command kind table of its own. Free text that is
not a recognized command stays prompt text; there is no fall-through.

**TUI** (`src/tui/command.rs`, `src/tui/app/mod.rs`, `src/tui/commands/**`): the built-in registry
becomes a **cache of the daemon's catalog**; the daemon's catalog defines identity, name, aliases,
description, kind and argument shape, while the TUI's local dynamic sources
(`append_dynamic_commands`) stay local and out of the catalog. Daemon-effect commands stop
executing locally: their arms resolve the typed line to a catalog entry and issue
`CoreRequest::CommandExecute`, then render the returned structured result with their **existing**
presentation code. Frontend-effect commands dispatch against the **declared frontend-action set**
rather than an open enum (ADR-0052, Compatibility: "now against the declared frontend-action set
rather than an open enum").

### Security and authorization

- The daemon authorizes every `CommandExecute` under `command.execute` and re-checks the
  capability server-side. The catalog's `capability` field is a **property of the catalog entry**,
  never a client-supplied widening — a request that names a capability is rejected.
- The frontend-action set is declared, finite, and reviewed. Its guard **fails closed**: an action
  that is not in the declared set, a declared action with no implementation, or a declared action
  whose catalog entry says daemon-effect, is a build failure.
- The daemon refuses to execute a frontend-effect command id: `CommandExecute` against a
  frontend-effect form returns a typed `NotDaemonEffect` refusal and produces **no** `Run`.
- The renderer holds no credentials and no durable state. The desktop's capability grants no plugin
  permissions (`scripts/check-desktop-boundary.sh:92`).
- A command's argument is bounded (≤ 4 KiB) and is treated as untrusted input; the daemon's
  existing path validation applies unchanged.

### Documentation and static guards

| Guard | Proves |
|-------|--------|
| `scripts/check_command_catalog_ownership.py` | the daemon's catalog and the TUI's command surface are the same set; no `CoreRequest` variant executes a command without going through the catalog; no frontend holds a second catalog |
| `scripts/check_frontend_action_set.py` | the frontend-action set is finite, declared, closed, and contains nothing daemon-mutating |

## 7. Ordered work packages

The order is load-bearing. WP-B (terminology) precedes C (protocol), which precedes D (the
daemon), which precedes E (the guard), which precedes F (the TUI), which precedes G (the
desktop). `plans/002-long-term-roadmap.md:17` requires the typed protocol representation before
frontend-only state; `:897-899` requires terminology and the long-term specification before the
roadmap; ADR-0052 items 7 and 8 require the terminology amendment before the protocol variant
lands.

### Work package A — Classify the catalog

**Intent.** Produce the complete, reviewable classification of every command form, and the record
that makes a misclassification detectable.

**Required changes.**

1. The classification unit is the **command form**, not the bare enum variant: a command's
   argument can select a different effect. `/agents` is the confirmed case — the bare form opens
   the agent dialog (frontend effect) while `show`, `--all`, `diff`, `validate`, `reload` and
   `rebuild` are daemon effects (`src/tui/app/mod.rs:5798-5950`).
2. The **rule**, applied uniformly:

   > A form is a **daemon effect** when invoking it requires the daemon to produce content or
   > perform an effect: it mutates daemon-owned state, reads daemon-owned state as its result, or
   > drives a daemon-mediated process, store, or service. A form is a **frontend effect** when its
   > entire observable result is a change to frontend presentation state — a panel or dialog opens
   > or closes, focus moves, a local display flag toggles, a local edit is undone or redone, a
   > disclosure expands, a view scrolls. Opening a surface whose *contents* are daemon-owned does
   > not by itself make the command a daemon effect: the contents arrive over that surface's
   > existing, separate data path, which this milestone does not change. **Tie-break:** when the
   > command itself performs the fetch — rather than the surface it opens — the form is a daemon
   > effect.

3. The classification is committed as
   `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`.
   The table below is that artifact's content: **161 commands → 166 forms → 134 daemon-effect
   forms, 32 frontend-effect forms** (129 commands are wholly daemon-effect, 31 wholly
   frontend-effect, and `/agents` carries one frontend-effect form and five daemon-effect forms).

| # | Command | Action | Kind | Authority the effect lives in | Dispatcher arm |
|---|---------|--------|------|------------------------------|-----------------|
| 1 | `/init` | `ProjectInit` | **D** | the daemon's project-init draft | `src/tui/app/mod.rs:5670` |
| 2 | `/projects` | `Projects` | **F** | opens the project picker; the list arrives over the picker's own path | `src/tui/app/mod.rs:6490` |
| 3 | `/workspace` | `Workspace` | **F** | opens the Workspace view; the dashboard fills over its own path | `src/tui/app/mod.rs:6770` |
| 4 | `/workspaces` | `Workspaces` | **F** | baseline advisory toast naming `/sessions` | `src/tui/app/mod.rs:6680` |
| 5 | `/tree` | `Tree` | **F** | opens the file-tree dialog | `src/tui/app/mod.rs:5770` |
| 6 | `/editor` | `Editor` | **F** | launches the user's `$EDITOR`; frontend-local, no daemon state | `src/tui/app/mod.rs:6711` |
| 7 | `/open` | `OpenDocument` | **D** | the daemon's `DocumentService` | `src/tui/app/mod.rs:6714` |
| 8 | `/worktree` | `Worktree` | **D** | daemon-owned git worktrees | `src/tui/app/mod.rs:6685` |
| 9 | `/agent` | `AgentSelect` | **D** | the session's agent selection | `src/tui/app/mod.rs:5774` |
| 10 | `/agents` | `Agents` | **F**/**D** | bare form opens the agent dialog; `show`/`--all`/`diff`/`validate`/`reload`/`rebuild` are daemon-routed | `src/tui/app/mod.rs:5798` |
| 11 | `/new` | `New` | **D** | creates a session in the daemon | `src/tui/app/mod.rs:5951` |
| 12 | `/compact` | `Compact` | **D** | compaction is daemon-owned — **recorded baseline discrepancy**, §14 | `src/tui/app/mod.rs:5958` |
| 13 | `/sessions` | `Sessions` | **F** | opens the session dialog; the list arrives over the dialog's own path | `src/tui/app/mod.rs:6487` |
| 14 | `/share` | `Share` | **D** | a daemon-owned share URL | `src/tui/app/mod.rs:6499` |
| 15 | `/unshare` | `Unshare` | **D** | revokes a daemon-owned share URL | `src/tui/app/mod.rs:6525` |
| 16 | `/export` | `Export` | **D** | daemon-owned session state | `src/tui/app/mod.rs:6558` |
| 17 | `/import` | `Import` | **D** | session state, through the daemon | `src/tui/app/mod.rs:6572` |
| 18 | `/rename` | `Rename` | **F** | baseline advisory toast naming `/sessions` | `src/tui/app/mod.rs:6536` |
| 19 | `/timeline` | `Timeline` | **F** | shows the timeline view | `src/tui/app/mod.rs:6541` |
| 20 | `/undo` | `Undo` | **F** | undoes a local message-list edit | `src/tui/app/mod.rs:6544` |
| 21 | `/redo` | `Redo` | **F** | redoes a local message-list edit | `src/tui/app/mod.rs:6551` |
| 22 | `/status` | `Status` | **D** | daemon-resolved session status and effective policy | `src/tui/app/mod.rs:5971` |
| 23 | `/state` | `State` | **D** | the daemon-resolved session, goal and plan state | `src/tui/app/mod.rs:7389` |
| 24 | `/start` | `StartTurn` | **D** | resumes a daemon-owned stopped turn | `src/tui/app/mod.rs:6496` |
| 25 | `/stop` | `Stop` | **D** | cancels the daemon's running turn | `src/tui/app/mod.rs:6493` |
| 26 | `/loop` | `Loop` | **D** | schedules a recurring daemon-owned prompt | `src/tui/app/mod.rs:6727` |
| 27 | `/schedules` | `Schedules` | **D** | daemon-owned background schedules | `src/tui/app/mod.rs:6777` |
| 28 | `/task-del` | `TaskDel` | **D** | deletes a daemon-owned background schedule | `src/tui/app/mod.rs:6784` |
| 29 | `/tasks` | `Tasks` | **D** | daemon-owned work-order tasks | `src/tui/app/mod.rs:6758` |
| 30 | `/task` | `Task` | **D** | a daemon-owned task | `src/tui/app/mod.rs:6767` |
| 31 | `/goal` | `Goal` | **D** | the daemon-owned goal | `src/tui/app/mod.rs:7133` |
| 32 | `/plan` | `Plan` | **D** | the daemon-owned plan | `src/tui/app/mod.rs:7275` |
| 33 | `/tests` | `Tests` | **D** | daemon-owned test results | `src/tui/app/mod.rs:8164` |
| 34 | `/test` | `Test` | **D** | runs a daemon-owned test | `src/tui/app/mod.rs:8169` |
| 35 | `/diff` | `Diff` | **D** | a daemon-owned workspace diff | `src/tui/app/mod.rs:8155` |
| 36 | `/revert` | `Revert` | **D** | mutates the workspace through the daemon | `src/tui/app/mod.rs:8182` |
| 37 | `/review-change` | `Review` | **D** | daemon-owned change previews | `src/tui/app/mod.rs:7579` |
| 38 | `/edit-undo` | `EditUndo` | **D** | undoes a daemon-owned edit | `src/tui/app/mod.rs:8191` |
| 39 | `/edit-reapply` | `EditReapply` | **D** | reapplies a daemon-owned edit | `src/tui/app/mod.rs:8237` |
| 40 | `/edit-checkpoints` | `EditCheckpoints` | **D** | daemon-owned edit checkpoints | `src/tui/app/mod.rs:8283` |
| 41 | `/connect` | `Connect` | **F** | opens the connect dialog | `src/tui/app/mod.rs:5963` |
| 42 | `/connections` | `Connections` | **F** | opens the connection-selection dialog | `src/tui/app/mod.rs:5967` |
| 43 | `/models-refresh` | `ModelsRefresh` | **D** | the daemon/provider model catalog | `src/tui/app/mod.rs:6658` |
| 44 | `/variants` | `Variants` | **F** | baseline advisory toast naming the model's base | `src/tui/app/mod.rs:6661` |
| 45 | `/approval` | `Approval` | **D** | the daemon's approval mode | `src/tui/app/mod.rs:6006` |
| 46 | `/sandbox` | `Sandbox` | **D** | the daemon's sandbox profile | `src/tui/app/mod.rs:6030` |
| 47 | `/network` | `Network` | **D** | the daemon's child network intent | `src/tui/app/mod.rs:6056` |
| 48 | `/policy` | `Policy` | **D** | the daemon's effective policy | `src/tui/app/mod.rs:5991` |
| 49 | `/tool-backends` | `ToolBackends` | **F** | the local search-backend configuration | `src/tui/app/mod.rs:8134` |
| 50 | `/tool-contracts` | `ToolContracts` | **D** | the daemon's tool contracts | `src/tui/app/mod.rs:7477` |
| 51 | `/chat` | `Chat` | **F** | focuses the chat panel or opens the chat dialog | `src/tui/app/mod.rs:6215` |
| 52 | `/chat-send` | `ChatSend` | **D** | the daemon-owned project chat | `src/tui/app/mod.rs:6227` |
| 53 | `/chat-reply` | `ChatReply` | **D** | a daemon-owned chat message | `src/tui/app/mod.rs:6240` |
| 54 | `/chat-history` | `ChatHistory` | **D** | daemon-owned chat history | `src/tui/app/mod.rs:6271` |
| 55 | `/chat-sync` | `ChatSync` | **D** | daemon-owned chat state | `src/tui/app/mod.rs:6291` |
| 56 | `/chat-read` | `ChatRead` | **D** | a daemon-owned chat message | `src/tui/app/mod.rs:6303` |
| 57 | `/chat-edit` | `ChatEdit` | **D** | edits a daemon-owned chat message | `src/tui/app/mod.rs:6315` |
| 58 | `/chat-redact` | `ChatRedact` | **D** | redacts a daemon-owned chat message | `src/tui/app/mod.rs:6345` |
| 59 | `/chat-composing` | `ChatComposing` | **D** | the daemon-owned composing state | `src/tui/app/mod.rs:6370` |
| 60 | `/chat-action-task` | `ChatActionTask` | **D** | a daemon-owned chat action task | `src/tui/app/mod.rs:6393` |
| 61 | `/chat-action-review` | `ChatActionReview` | **D** | a daemon-owned chat action | `src/tui/app/mod.rs:6429` |
| 62 | `/chat-action-list` | `ChatActionList` | **D** | daemon-owned chat actions | `src/tui/app/mod.rs:6464` |
| 63 | `/collaborators` | `Collaborators` | **D** | daemon-owned presence state | `src/tui/app/mod.rs:6126` |
| 64 | `/team` | `Team` | **D** | daemon-owned team membership | `src/tui/app/mod.rs:6142` |
| 65 | `/control` | `Control` | **D** | the daemon's control plane | `src/tui/app/mod.rs:6154` |
| 66 | `/observe` | `Observe` | **D** | a daemon-owned observation lease | `src/tui/app/mod.rs:6167` |
| 67 | `/stop-observing` | `StopObserving` | **D** | releases the daemon-owned observation lease | `src/tui/app/mod.rs:6197` |
| 68 | `/subagent-send` | `SubagentSend` | **D** | addresses a prompt to a running subagent | `src/tui/app/mod.rs:5680` |
| 69 | `/memory` | `Memory` | **D** | the daemon-owned memory store | `src/tui/app/mod.rs:6800` |
| 70 | `/memory-search` | `MemorySearch` | **D** | searches the daemon-owned memory store | `src/tui/app/mod.rs:6807` |
| 71 | `/memory-list` | `MemoryList` | **D** | lists the daemon-owned memory store | `src/tui/app/mod.rs:6815` |
| 72 | `/memory-remember` | `MemoryRemember` | **D** | writes the daemon-owned memory store | `src/tui/app/mod.rs:6823` |
| 73 | `/memory-forget` | `MemoryForget` | **D** | deletes from the daemon-owned memory store | `src/tui/app/mod.rs:6831` |
| 74 | `/memory-consolidate` | `MemoryConsolidate` | **D** | consolidates the daemon-owned memory store | `src/tui/app/mod.rs:6839` |
| 75 | `/habits` | `Habits` | **D** | the daemon-owned habit store | `src/tui/app/mod.rs:6842` |
| 76 | `/habit-dismiss` | `HabitDismiss` | **D** | dismisses a daemon-owned habit | `src/tui/app/mod.rs:6852` |
| 77 | `/skills` | `Skills` | **D** | the daemon's effective skill registry | `src/tui/app/mod.rs:5689` |
| 78 | `/skill-promote` | `SkillPromote` | **D** | the daemon-owned skill store | `src/tui/app/mod.rs:6866` |
| 79 | `/skill-proposals` | `SkillProposals` | **D** | daemon-owned skill proposals | `src/tui/app/mod.rs:6928` |
| 80 | `/skill-proposal` | `SkillProposal` | **D** | publishes or resolves a daemon-owned skill proposal | `src/tui/app/mod.rs:6965` |
| 81 | `/research` | `Research` | **D** | starts a daemon-owned research run | `src/tui/app/mod.rs:8313` |
| 82 | `/research-runs` | `ResearchRuns` | **D** | daemon-owned research runs | `src/tui/app/mod.rs:8324` |
| 83 | `/research-open` | `ResearchOpen` | **D** | opens a daemon-owned research run | `src/tui/app/mod.rs:8328` |
| 84 | `/research-show` | `ResearchShow` | **D** | shows a daemon-owned research run | `src/tui/app/mod.rs:8339` |
| 85 | `/search` | `Search` | **D** | daemon-owned workspace content | `src/tui/app/mod.rs:7439` |
| 86 | `/doctor` | `Doctor` | **D** | the daemon's diagnostic pass | `src/tui/app/mod.rs:7460` |
| 87 | `/logs` | `Logs` | **D** | the daemon's log tail, which the frontend reads off disk today | `src/tui/app/mod.rs:7473` |
| 88 | `/nodes` | `Nodes` | **D** | daemon-owned node telemetry | `src/tui/app/mod.rs:7494` |
| 89 | `/lsp-status` | `LspStatus` | **D** | the daemon's LSP service status | `src/tui/app/mod.rs:7486` |
| 90 | `/lsp-servers` | `LspServers` | **D** | the daemon's LSP servers | `src/tui/app/mod.rs:7668` |
| 91 | `/lsp-capabilities` | `LspCapabilities` | **D** | daemon-owned LSP capabilities | `src/tui/app/mod.rs:7683` |
| 92 | `/lsp-errors` | `LspErrors` | **D** | daemon-owned LSP diagnostics | `src/tui/app/mod.rs:7705` |
| 93 | `/lsp-root` | `LspRoot` | **D** | a daemon-owned LSP root | `src/tui/app/mod.rs:7726` |
| 94 | `/lsp-doctor` | `LspDoctor` | **D** | the daemon's LSP diagnostic pass | `src/tui/app/mod.rs:7740` |
| 95 | `/lsp-context-diagnostics` | `LspContextDiagnostics` | **D** | daemon-owned LSP context diagnostics | `src/tui/app/mod.rs:7756` |
| 96 | `/lsp-repair-local` | `LspRepairLocal` | **D** | mutates the workspace through the daemon's LSP | `src/tui/app/mod.rs:7773` |
| 97 | `/lsp-repair-hunk` | `LspRepairHunk` | **D** | mutates the workspace through the daemon's LSP | `src/tui/app/mod.rs:7803` |
| 98 | `/lsp-review-file` | `LspReviewFile` | **D** | reviews a daemon-owned file | `src/tui/app/mod.rs:7832` |
| 99 | `/lsp-review-diff` | `LspReviewDiff` | **D** | reviews a daemon-owned diff | `src/tui/app/mod.rs:7859` |
| 100 | `/lsp-security-review` | `LspSecurityReview` | **D** | the daemon's LSP security review | `src/tui/app/mod.rs:7880` |
| 101 | `/lsp-impact` | `LspImpact` | **D** | daemon-owned LSP impact | `src/tui/app/mod.rs:7908` |
| 102 | `/lsp-test-repair` | `LspTestRepair` | **D** | mutates the workspace through the daemon's LSP | `src/tui/app/mod.rs:7938` |
| 103 | `/lsp-interface` | `LspInterface` | **D** | daemon-owned LSP interface detail | `src/tui/app/mod.rs:7969` |
| 104 | `/lsp-cross-repair` | `LspCrossRepair` | **D** | mutates the workspace through the daemon's LSP | `src/tui/app/mod.rs:8000` |
| 105 | `/lsp-call-neighbors` | `LspCallNeighbors` | **D** | daemon-owned LSP call neighbours | `src/tui/app/mod.rs:8032` |
| 106 | `/lsp-restart` | `LspRestart` | **D** | restarts a daemon-owned LSP server | `src/tui/app/mod.rs:8070` |
| 107 | `/lsp-stop` | `LspStop` | **D** | stops a daemon-owned LSP server | `src/tui/app/mod.rs:8086` |
| 108 | `/lsp-cache-status` | `LspCacheStatus` | **D** | the daemon's LSP cache status | `src/tui/app/mod.rs:8104` |
| 109 | `/lsp-cache-clear` | `LspCacheClear` | **D** | clears the daemon's LSP cache | `src/tui/app/mod.rs:8113` |
| 110 | `/lsp-previews` | `LspPreviews` | **D** | daemon-owned LSP previews | `src/tui/app/mod.rs:7501` |
| 111 | `/lsp-preview` | `LspPreview` | **D** | a daemon-owned LSP preview | `src/tui/app/mod.rs:7510` |
| 112 | `/lsp-preview-clear` | `LspPreviewClear` | **D** | clears daemon-owned LSP previews | `src/tui/app/mod.rs:7527` |
| 113 | `/lsp-preview-refresh` | `LspPreviewRefresh` | **D** | refreshes a daemon-owned LSP preview | `src/tui/app/mod.rs:7552` |
| 114 | `/lsp-preview-apply` | `LspPreviewApply` | **D** | applies a daemon-owned preview to the workspace | `src/tui/app/mod.rs:7584` |
| 115 | `/security-review` | `SecurityReview` | **D** | the daemon's security review | `src/tui/app/mod.rs:8377` |
| 116 | `/security-review-show` | `SecurityReviewShow` | **D** | a daemon-owned security-review result | `src/tui/app/mod.rs:8452` |
| 117 | `/security-review-cancel` | `SecurityReviewCancel` | **D** | cancels a daemon-owned security review | `src/tui/app/mod.rs:8462` |
| 118 | `/shell-include` | `ShellInclude` | **D** | stages daemon-owned shell output | `src/tui/app/mod.rs:8466` |
| 119 | `/shell-rerun` | `ShellRerun` | **D** | reruns a daemon-owned shell command | `src/tui/app/mod.rs:8510` |
| 120 | `/shell-kill` | `ShellKill` | **D** | kills a daemon-owned shell command | `src/tui/app/mod.rs:8526` |
| 121 | `/shell-list` | `ShellList` | **D** | lists daemon-owned shell commands | `src/tui/app/mod.rs:8542` |
| 122 | `/shell-show` | `ShellShow` | **D** | shows a daemon-owned shell command | `src/tui/app/mod.rs:8574` |
| 123 | `/shell-expand` | `ShellExpand` | **D** | expands a daemon-owned shell command's output | `src/tui/app/mod.rs:8590` |
| 124 | `/shell-ask` | `ShellAsk` | **D** | asks about a daemon-owned shell command | `src/tui/app/mod.rs:8869` |
| 125 | `/plugins` | `Plugins` | **D** | lists daemon-owned plugins | `src/tui/app/mod.rs:8617` |
| 126 | `/plugin-info` | `PluginInfo` | **D** | daemon-owned plugin state | `src/tui/app/mod.rs:8814` |
| 127 | `/plugin-enable` | `PluginEnable` | **D** | enables a daemon-owned plugin | `src/tui/app/mod.rs:8824` |
| 128 | `/plugin-disable` | `PluginDisable` | **D** | disables a daemon-owned plugin | `src/tui/app/mod.rs:8834` |
| 129 | `/plugin-doctor` | `PluginDoctor` | **D** | diagnoses daemon-owned plugins | `src/tui/app/mod.rs:8844` |
| 130 | `/plugin-remove` | `PluginRemove` | **D** | removes a daemon-owned plugin | `src/tui/app/mod.rs:8849` |
| 131 | `/plugin-install` | `PluginInstall` | **D** | installs a daemon-owned plugin | `src/tui/app/mod.rs:8859` |
| 132 | `/terminal` | `Terminal` | **D** | opens a daemon-owned interactive process | `src/tui/app/mod.rs:8665` |
| 133 | `/terminal-create` | `TerminalCreate` | **D** | creates a daemon-owned interactive process | `src/tui/app/mod.rs:8620` |
| 134 | `/terminal-list` | `TerminalList` | **D** | lists daemon-owned interactive processes | `src/tui/app/mod.rs:8634` |
| 135 | `/terminal-attach` | `TerminalAttach` | **D** | attaches to a daemon-owned interactive process | `src/tui/app/mod.rs:8637` |
| 136 | `/terminal-show` | `TerminalShow` | **D** | shows a daemon-owned interactive process | `src/tui/app/mod.rs:8650` |
| 137 | `/terminal-clear` | `TerminalClear` | **D** | clears a daemon-owned interactive-process buffer | `src/tui/app/mod.rs:8699` |
| 138 | `/terminal-focus` | `TerminalFocus` | **F** | moves focus to a terminal panel | `src/tui/app/mod.rs:8726` |
| 139 | `/terminal-send` | `TerminalSend` | **D** | sends input to a daemon-owned interactive process | `src/tui/app/mod.rs:8735` |
| 140 | `/terminal-resize` | `TerminalResize` | **D** | resizes a daemon-owned interactive process | `src/tui/app/mod.rs:8752` |
| 141 | `/terminal-resume` | `TerminalResume` | **D** | resumes a daemon-owned interactive process | `src/tui/app/mod.rs:8778` |
| 142 | `/terminal-detach` | `TerminalDetach` | **D** | detaches from a daemon-owned interactive process | `src/tui/app/mod.rs:8787` |
| 143 | `/terminal-terminate` | `TerminalTerminate` | **D** | terminates a daemon-owned interactive process | `src/tui/app/mod.rs:8796` |
| 144 | `/terminal-remove` | `TerminalRemove` | **D** | removes a daemon-owned interactive process | `src/tui/app/mod.rs:8805` |
| 145 | `/exit` | `Exit` | **F** | stops the frontend's event loop | `src/tui/app/mod.rs:5759` |
| 146 | `/help` | `Help` | **F** | opens the help dialog | `src/tui/app/mod.rs:5763` |
| 147 | `/panels` | `Panels` | **F** | opens the panel manager | `src/tui/app/mod.rs:6203` |
| 148 | `/context` | `Context` | **F** | opens the context dialog | `src/tui/app/mod.rs:6102` |
| 149 | `/cost` | `Cost` | **F** | opens the cost dialog | `src/tui/app/mod.rs:6106` |
| 150 | `/usage` | `Usage` | **F** | opens the usage dialog | `src/tui/app/mod.rs:6110` |
| 151 | `/queue` | `Queue` | **F** | opens the queue box; its contents arrive over the box's own path | `src/tui/app/mod.rs:5677` |
| 152 | `/reload` | `Reload` | **D** | the daemon's runtime-asset registry | `src/tui/app/mod.rs:5683` |
| 153 | `/reasoning` | `Reasoning` | **D** | the daemon-owned reasoning preference | `src/tui/app/mod.rs:6602` |
| 154 | `/timestamps` | `Timestamps` | **F** | toggles the local timestamp display | `src/tui/app/mod.rs:6579` |
| 155 | `/thinking` | `Thinking` | **F** | toggles the local reasoning display | `src/tui/app/mod.rs:6588` |
| 156 | `/expand-all` | `ExpandAllDisclosures` | **F** | expands local disclosures | `src/tui/app/mod.rs:6597` |
| 157 | `/themes` | `Themes` | **F** | changes the frontend's theme | `src/tui/app/mod.rs:6114` |
| 158 | `/tui` | `Tui` | **F** | toggles the frontend's fullscreen mode | `src/tui/app/mod.rs:6117` |
| 159 | `/stats`, `/tui-stats` | `TuiStats` | **F** | opens the stats dialog | `src/tui/app/mod.rs:6120` |
| 160 | `/tts` | `Tts` | **F** | toggles the frontend's text-to-speech | `src/tui/app/mod.rs:6123` |
| 161 | `/fork` | `Fork` | **F** | baseline advisory toast naming the CLI `--fork` flag | `src/tui/app/mod.rs:6668` |

4. **Who reviews it.** The classification is reviewed by the repository owner before Work package
   D begins, because a wrong kind is a security-relevant defect (ADR-0052, Consequences, Negative).
   The review asks two questions of every form: *does this form change daemon-owned state or
   consume daemon-owned content as its result?* and *would the desktop be able to run this form
   with the authority it has?* A form that answers "no" to both must be a frontend effect; a form
   that answers "yes" to either must be a daemon effect.
5. **How a misclassification is caught.** Three mechanisms, all mechanical:
   - the **frontend-action-set guard** (`scripts/check_frontend_action_set.py`) fails closed if a
     declared frontend action maps to a catalog form the catalog calls a daemon effect, or if a
     catalog form calls itself a frontend effect but has no declared action;
   - the **catalog-ownership guard** (`scripts/check_command_catalog_ownership.py`) fails if the
     catalog and the TUI's command surface disagree, and if any `CoreRequest` variant executes a
     command without going through the catalog;
   - the **daemon refuses** a `CommandExecute` against a frontend-effect form with a typed
     `NotDaemonEffect` refusal, so a misclassification that slips past the static guards still
     cannot execute as a daemon effect.
6. **Recorded discrepancies** are listed in the classification artifact with an owner-visible note.
   At the baseline there is one confirmed: `/compact` toasts "Compaction triggered" but never
   reaches the daemon (`TuiCommand::CompactSession` has no sender). The artifact must also record
   the ADR-0052 "86" count discrepancy (§3) and its resolution.

**Acceptance evidence.** The classification artifact exists, covers all 161 commands and 166
forms, is reviewed, and the three misclassification guards exist with passing `--self-test`s.

### Work package B — Amend the terminology document

**Intent.** Add the cross-frontend `Command` definition to the canonical vocabulary **before** the
protocol variant or any UI label using the concept lands.

**Required changes.** `plans/001-terminology-and-domain-model.md` gains a `Command` definition
covering the catalog entry, its two kinds, its identity (shared across frontends, distinct from
the display name), its relationship to `Run`, its relationship to the `command.execute` capability,
and the rule that "command" and "Run" are not interchangeable
(`plans/001-terminology-and-domain-model.md:759`). The amendment is an **addition of
definitions**, not a change to an existing one (ADR-0052, Compatibility). It follows
`plans/002-long-term-roadmap.md:897-899` — terminology and the long-term specification first, then
the roadmap — and is the owner-directed amendment `plans/003-planning-process.md:29-36` permits,
contingent on ADR-0052's acceptance (ADR-0052 item 7).

**Acceptance evidence.** The terminology document contains the `Command` definition; a reviewer
confirms no existing canonical term was redefined; the amendment's commit precedes the protocol
variant's commit.

### Work package C — The protocol variants, DTOs and bounds

**Intent.** Add the bounded, additive protocol family.

**Required changes.** The variants and DTOs in §6 "Protocol"; the `DaemonFamily::Commands` mapping
in `src/core/daemon_family.rs`; the project-scoping entry in `src/core/daemon.rs` beside the other
families; the `codegg-client` request wrapper following
`crates/codegg-client/src/local.rs:188-213`; and the generated TypeScript mirror checked by
`apps/desktop/scripts/check-bindings.mjs`. The bounds are enforced **server-side** — an over-bound
catalog is refused with a typed reason, never truncated silently.

**Acceptance evidence.** `cargo test -p codegg-protocol --locked` is green; the round-trip serde
tests pass; the bound tests pass; `apps/desktop/scripts/check-bindings.mjs` is green against the
generated mirror.

### Work package D — The daemon catalog and the daemon-effect execution path

**Intent.** Give the daemon the catalog and the audited executor.

**Required changes.** `crates/codegg-core/src/command_catalog.rs` (the single definition);
`src/core/daemon_commands.rs` (the family handler: the catalog request, the execute request, the
authorization step, the `Run` open and close, the refusal reasons); the executor's dispatch table
mapping each daemon-effect form to its daemon handler; and `RunKind::Command`
(`crates/codegg-core/src/run_store.rs:212-220`).

The **authorization** step is unconditional and server-side: the executor checks the principal
against `command.execute` (`crates/codegg-core/src/team.rs:269-270`) before touching the handler.
The catalog's `capability` field is what is checked; a request cannot widen it.

The **audit** step is unconditional for a daemon-effect execution: a `Run` is opened before the
handler runs and closed with its outcome after. A refusal opens **no** `Run` and returns a typed
reason; a handler that fails closes the `Run` as failed and returns a typed failure.

**Acceptance evidence.** A test proves a daemon-effect execution produces a `Run` with the command
identity and the issuing principal; a test proves an unauthorized execution is refused with a
reason and produces no `Run`; a test proves the catalog response is bounded and an over-bound
catalog is refused.

### Work package E — The frontend-effect declared action set and its guard

**Intent.** Make the frontend-action set finite, declared, reviewable, and enforced.

**Required changes.** A `FrontendAction` enum in `codegg-core` with **29** variants covering the
32 frontend-effect forms (the four advisory forms — `/rename`, `/fork`, `/workspaces`,
`/variants` — share one `ShowAdvisory` action whose text is a frontend-local string keyed by the
command identity): `Quit`, `OpenHelpDialog`, `OpenFileTreeDialog`, `OpenConnectDialog`,
`OpenConnectionSelectionDialog`, `OpenContextDialog`, `OpenCostDialog`, `OpenUsageDialog`,
`OpenPanelManager`, `OpenProjectPicker`, `OpenSessionDialog`, `OpenWorkspaceDashboard`,
`OpenQueueBox`, `OpenAgentsDialog`, `FocusChat`, `ShowTimeline`, `UndoLocalMessages`,
`RedoLocalMessages`, `ToggleTimestamps`, `ToggleThinking`, `ExpandAllDisclosures`, `SetTheme`,
`ToggleFullscreen`, `OpenStatsDialog`, `ToggleTts`, `OpenExternalEditor`, `FocusTerminal`,
`ShowToolBackends`, `ShowAdvisory`.

Each action's declaration records its non-mutating disposition.
`scripts/check_frontend_action_set.py` fails closed when: a `FrontendAction` variant is not in the
declared set; a declared entry has no variant; a catalog form marked frontend-effect has no
declared action; a declared action's catalog form is marked daemon-effect; or a declared action is
marked mutating. `OpenExternalEditor` is the one declared action that launches a process
(`src/tui/app/mod.rs:6711`); it is already recorded in `docs/execution-ownership.toml:189`, and
the declaration must carry that disposition explicitly.

**Acceptance evidence.** The guard's `--self-test` injects each of the five failure classes and
proves the guard rejects each; the declared set and the enum agree; the 32 frontend-effect forms
all map to a declared action.

### Work package F — The TUI migration, with regression coverage

**Intent.** Move daemon-effect execution to the daemon and dispatch frontend effects against the
declared action set, with **no change in observable outcome**.

**Required changes.**

1. The TUI's built-in registry becomes a cache of the daemon's catalog. The TUI's local dynamic
   sources (`src/tui/command.rs:999-1043`) stay local and out of the catalog.
2. The dispatcher resolves a typed line to a **catalog form**, then: daemon effect →
   `CoreRequest::CommandExecute`; frontend effect → the declared `FrontendAction`.
3. **The central migration rule that makes outcome-preservation mechanical:**

   > The daemon owns the effect and the authoritative data; the frontend keeps the formatting. A
   > daemon-effect command's TUI arm keeps its existing presentation code and replaces only its
   > *effect* call with a `CommandExecute` round trip whose structured result feeds the same
   > presentation code.

   This is why `/skills` still prints the same report, why `/status` still prints the same status
   line, and why `/reload` still shows the same report — the daemon supplies the fields, the TUI
   formats them.
4. The **inventory** of which daemon-effect forms already have a daemon route and which do not is
   committed as part of the classification artifact. The verified seeds are the six in §3:
   `Compact`, `Status`, `State`, `Logs`, `Skills`, and the bare `/agents` form.
5. **Regression coverage**: for each migrated command, a test asserts the documented outcome. The
   coverage is organized by command family, and the family list is a closure artifact.

**Acceptance evidence.** For every migrated command, a test asserts the same observable outcome as
the baseline; the TUI's existing command tests remain green; the new regression tests are listed in
the closure record with their commands.

### Work package G — The desktop consumption

**Intent.** Give the desktop the daemon's catalog, a `/` completion, a palette, and a dispatch
path.

**Required changes.** The two named Tauri commands (§6 "Frontend"); the `codegg-client` request
wrappers; the generated TypeScript DTO mirror; the renderer's `/` completion overlay and palette;
the dispatch split (frontend effect locally, daemon effect to the daemon); and the result
rendering. The renderer holds no catalog, no command names, and no kind table. M007-A must have
landed (the desktop must be a working client), and M007-C's reserved `mod+shift+p` chord must be
bound, before the palette is wired — M007-C records that chord as reserved for M007-D and does
not bind it.

**Acceptance evidence.** A renderer test proves the completion list is exactly what the daemon
returned; a renderer test proves a daemon-effect command issues a host command and shows the
result; a renderer test proves a frontend-effect command does **not** reach the host; a renderer
test proves free text that resembles a command is submitted as prompt text.

### Work package H — The guards and their `--self-test`s

**Intent.** Make the two new invariants mechanically enforced.

**Required changes.** `scripts/check_command_catalog_ownership.py` and
`scripts/check_frontend_action_set.py`, each with a `--self-test` that injects the defect and
proves the guard rejects it. Both are wired into `scripts/verify.sh`'s `run_guards()` and into
`.github/workflows/ci.yml`; the wiring is additive and does not change which other guards CI runs.

The `--self-test` classes for `check_command_catalog_ownership.py`: a catalog entry with no TUI
counterpart; a TUI command with no catalog entry; a `CoreRequest` variant that executes a command
without consulting the catalog; a renderer or host source containing a hand-written command list.

The `--self-test` classes for `check_frontend_action_set.py`: an undeclared `FrontendAction`
variant; a declared entry with no variant; a catalog form marked frontend-effect with no declared
action; a declared action whose catalog form is daemon-effect; a declared action marked mutating.

**Acceptance evidence.** Each guard's `--self-test` is demonstrated to fail on an injected
violation before being reverted; both guards are green on the real tree; both are in
`scripts/verify.sh quick`.

### Work package I — The tests

**Intent.** Cover the behaviours §10 names, on both frontends.

**Required changes.** The test surfaces in §10, including the TUI's family-level regression tests
and the desktop's renderer and host tests.

**Acceptance evidence.** The §11 command list is green; the §10 behaviours each have a named test.

## 8. Failure, cancellation, restart, and contention semantics

- **A command fails.** The daemon closes the `Run` as failed, returns
  `CommandOutcomeStatusDto::Failed` with a typed reason and the `run_id`. The frontend shows the
  reason. **The frontend does not retry** — a retry would be a second privileged effect the user
  did not ask for.
- **A command is refused for authorization.** The daemon returns
  `CommandOutcomeStatusDto::Refused` with a typed reason (`Unauthorized`,
  `CapabilityRequired`, `NotDaemonEffect`, `StaleCatalog`, `UnknownCommand`, `ArgumentTooLong`).
  **No `Run` is opened** for a refusal — a refusal is not an execution. The frontend shows the
  reason; it does not downgrade the refusal to a local execution.
- **A command times out.** The daemon's request timeout produces a typed failure with the
  `run_id` if a `Run` was opened. The frontend reports "the daemon did not answer" and leaves the
  `Run` to the daemon's own recovery. It does **not** re-send.
- **A command is submitted against a stale catalog version.** The daemon refuses with
  `StaleCatalog` and returns the current `catalog_version`. The frontend refetches the catalog and
  re-issues **once**, and only if the user did not type anything new in the meantime. This is the
  one bounded, user-visible retry, and it is a retry of the *catalog lookup*, not a second
  execution of the command.
- **A command arrives during a projection resync.** The catalog is fetched over the request
  channel, not over the projection, so a resync does not invalidate it. A command's *result* that
  depends on projected state is answered from the daemon's authoritative state, not from the
  frontend's last snapshot; the frontend then re-renders from the projection as usual.
- **A command arrives while the daemon is restarting.** The client's existing reconnect path
  applies (`crates/codegg-client/src/local.rs:131-151`). A command submitted during the outage
  fails with a typed connection error; it is **not** queued for later execution, because a
  privileged effect must not be deferred silently.
- **Two frontends run the same command concurrently.** Two executions, two `Run`s. The daemon's
  existing authority fences whatever the command's own handler already fences; the command
  executor adds no serialization of its own and does not touch the scheduler.
- **The frontend restarts mid-execution.** The `Run` lives in the daemon; the frontend's restart
  does not cancel it and does not lose it. The frontend reconnects and sees the run through the
  existing run listing.
- **A frontend effect cannot fail against the daemon**, because it does not reach it. If a
  frontend action is dispatched with no daemon connection, it still runs — it changes only what
  the frontend is showing.

## 9. Compatibility and migration

- **The protocol family is additive.** Two new `CoreRequest` variants and two new `CoreResponse`
  variants. No existing variant changes shape. A daemon that does not implement the family
  answers with its existing unsupported-variant error, and a client asking for the catalog degrades
  to the command list it already holds (ADR-0052, Compatibility).
- **The TUI migrates its execution without changing observable outcomes.** Identities, names,
  aliases and descriptions are unchanged (ADR-0052 item 4). The migration changes *where* the
  effect happens, not what the user sees.
- **The desktop gains a surface it never had.** There is nothing to migrate there; its composer
  stays a plain text input until this milestone lands, and becomes a command-aware composer after.
- **The terminology amendment is additive.** New definitions; no existing canonical term is
  redefined.
- **No storage migration.** `STORAGE_LAYOUT_VERSION` stays 88.
- **A daemon built before this milestone still serves a TUI built after it**, because the TUI
  degrades to the unsupported-variant error and to the command list it already holds. The TUI
  must therefore keep a local fallback list for the case where the catalog request is unsupported
  — **and that fallback must be derived from the same catalog definition, not hand-written**, or
  it becomes the second catalog this milestone forbids.
- **A TUI built before this milestone still works against a daemon built after it**, because the
  new variants are additive and the old TUI never asks for them.

## 10. Required tests

### Focused unit tests

- The catalog's shape: exactly two kinds; every command has at least one form; every form has a
  kind; the declared capability of every daemon-effect form is `command.execute` and nothing else.
- The classification artifact and the catalog agree, entry for entry and form for form.
- The bound tests: catalog count, form count, description length, alias count, argument length,
  response line count and response byte size.
- The frontend-action set: every frontend-effect form maps to exactly one declared action; every
  declared action maps to at least one form.
- The protocol round trip: every DTO serializes and deserializes without loss; unknown fields are
  tolerated; the `catalog_version` monotonicity rule holds.

### Integration tests

- A daemon-effect command typed in the TUI is authorized in the daemon, produces a `Run`, and the
  TUI's rendered outcome equals the baseline outcome.
- A frontend-effect command typed in the TUI still works and produces **no** `Run`.
- Free text that resembles a command (`/reloadl`, `/ reload`, `reload`, `/Reload` with a trailing
  space and a wrong argument) is treated as prompt text and produces no `Run`.
- An unauthorized command is refused with a typed reason and produces no `Run`.
- `CommandExecute` against a frontend-effect form is refused with `NotDaemonEffect` and produces
  no `Run`.
- The catalog response is bounded, and an over-bound catalog is **refused** rather than
  truncated.
- A misclassified command — a catalog form marked frontend-effect whose effect is daemon-mutating
  — is caught by the guard (via the guard's `--self-test`).

### Restart and recovery tests

- The daemon restarts between the catalog fetch and the command execution: the command fails with
  a typed connection error and is not re-queued.
- The daemon restarts after a `Run` was opened: the `Run` survives and is still listed.
- The frontend restarts mid-execution: the `Run` is still there, and the frontend does not
  re-issue the command.

### Contention and cancellation tests

- Two concurrent executions of the same command produce two `Run`s.
- A command submitted while the daemon is reconnecting fails with a typed error and is not
  silently retried.
- A stale `catalog_version` is refused with `StaleCatalog`, and the frontend refetches and
  re-issues exactly once.

### Security and negative tests

- A `CommandExecute` that names a capability other than the catalog entry's is rejected.
- An argument over the bound is rejected without reaching the handler.
- A command identity that is not in the catalog is rejected with `UnknownCommand`.
- The renderer cannot execute a daemon effect without going through the host's named command;
  `scripts/check-desktop-boundary.sh` still passes unchanged.

### Migration and compatibility tests

- A TUI talking to a daemon that does not implement the family degrades to the unsupported-variant
  error and the local fallback list.
- The TUI's existing command tests remain green, unmodified, except where a command's
  implementation moved.

## 11. Required verification commands

```bash
# protocol and client
cargo test -p codegg-protocol --locked
cargo test -p codegg-client --locked

# the catalog and the guards
cargo test -p codegg-core --locked
python3 scripts/check_command_catalog_ownership.py --self-test
python3 scripts/check_frontend_action_set.py --self-test
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_frontend_action_set.py

# the TUI
cargo test --lib tui::command --locked
cargo test --lib tui::app --locked

# the desktop renderer and host
cd apps/desktop && npm run typecheck && npm test && npm run bindings:check
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical sweep
scripts/verify.sh quick
```

**`scripts/verify.sh quick` does NOT compile the desktop.** The desktop is a separate workspace
(`apps/desktop/src-tauri/Cargo.toml` declares `[workspace] members = ["."]`, so it is not a member
of the root workspace), and `scripts/check-desktop-boundary.sh:4-8` asserts that the desktop
package has not leaked into the root Cargo metadata. The desktop's `npm run typecheck`,
`npm test`, `npm run bindings:check` and `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml`
are therefore **separate, mandatory** commands — a green `quick` says nothing about the desktop.

Do not claim commands that were not actually run in the closure record. If a test needs a feature
flag or fixture that does not exist yet, create it as part of the work package and record that in
the closure record rather than dropping the test.

## 12. Documentation updates

**Required by ADR-0052** (the agent may not perform these silently; each is an owner-directed
amendment, sequenced per `plans/002-long-term-roadmap.md:897-899`):

- `plans/001-terminology-and-domain-model.md` — the cross-frontend `Command` definition: the
  catalog entry, its two kinds, its identity, its relationship to `Run` and to
  `command.execute`, and the "command" vs "Run" ambiguity prohibition (ADR-0052 item 7;
  `plans/001-terminology-and-domain-model.md:5`, `:452-459`, `:759`).
- `plans/000-long-term-specification.md` — the desktop-frontend clause: a desktop client is a
  first-class frontend under §4.4's constraints, rendering frontend-neutral snapshots and owning
  no durable state (ADR-0052 item 8). This follows the owner-directed amendment path of
  `plans/003-planning-process.md:29-36` and is contingent on ADR-0052's acceptance.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — the M007-D row only: the
  milestone's status and its decomposition. The roadmap is adjusted **after** terminology and the
  long-term specification.
- `plans/registry.md` — register this plan and record its blocked state and its named blocker.

**Required by this milestone** (ordinary architecture documentation):

- `architecture/protocol.md` — the new request family, its DTOs, its bounds, and the
  `DaemonFamily::Commands` mapping.
- `architecture/command.md` — the catalog's new owner. This document currently records the TUI
  registry as the catalog (`architecture/command.md:11-15`, `:23`, `:256`, `:398`); it must record
  the daemon's catalog as the single definition and the TUI's registry as a cache plus its local
  dynamic sources. Its count claims (169 built-in commands, 161 `BuiltinSlashAction` variants —
  `architecture/command.md:482-485`, `:512-515`) must stay accurate and must not be edited to hide
  the migration.
- `architecture/command_routing.md` — the CLI's command routing is a **different** surface from
  slash commands; this document must gain a short section saying so, so a future reader does not
  conflate `codegg <verb>` with `/verb`.
- `architecture/tui.md` — the TUI's command surface after the migration: the catalog cache, the
  dispatch split, the declared frontend-action set, and the `/`-recognition sites at
  `src/tui/app/prompt_turn.rs:36`, `:71`, `:116`.
- `architecture/desktop.md` — the desktop's command surface: the two named host commands, the
  generated DTO mirror, the renderer's zero-catalog rule, and the completion and palette surfaces.
- `architecture/overview.md` — the command catalog's place in the ownership map.
- `architecture/authorization.md` — `command.execute` as the capability that authorizes a
  daemon-effect command, and the fact that the catalog entry names it and the daemon re-checks it.
- `architecture/lsp.md` — a note that the `/lsp-*` forms are daemon-effect commands whose
  execution is the daemon's authoritative LSP service, and that ADR-0052 does not re-decide LSP
  ownership.

**Explicitly not edited by this milestone's implementer:** ADR-0052 itself, ADR-0010, ADR-0011,
ADR-0012, and the M007-A / M007-C / M007-E plan files.

## 13. Acceptance criteria

1. A user typing `/` in the desktop sees the daemon's catalog — the same set the TUI exposes —
   and not a hand-written list; the renderer's source contains no command names.
2. A user running a daemon-effect command in the desktop has it authorized in the daemon under
   `command.execute` and sees the result; a canonical `Run` record exists for it.
3. A user running a daemon-effect command in the TUI sees exactly the outcome they saw before the
   migration — the same text, the same report, the same side effects — and a `Run` record exists
   for it.
4. A user running a frontend-effect command in either frontend sees the same local effect, and
   **no** `Run` record is created.
5. A user typing free text that merely resembles a command has it submitted as prompt text; it is
   never dispatched, and no `Run` record is created.
6. Exactly one command catalog exists in the product; a guard fails the build if a second one
   appears in the daemon, the TUI, the desktop host, or the renderer.
7. The frontend-action set is finite and declared, and the build fails if an undeclared action is
   added or if a declared action turns out to be daemon-mutating.
8. The catalog response is bounded in command count and payload size, and an over-bound catalog is
   refused with a typed reason rather than silently truncated.
9. A command that is refused for authorization reports a typed reason and leaves no `Run` record.
10. The TUI's command names, aliases and descriptions are unchanged; the 169 registry entries and
    the 161 `BuiltinSlashAction` variants still exist with the same identities.
11. `scripts/verify.sh quick` is green, including both new guards and their `--self-test`s;
    `cargo fmt --all -- --check` and workspace Clippy with `-D warnings` are clean.
12. The desktop's own verification is green and separately recorded: `npm run typecheck`,
    `npm test`, `npm run bindings:check`, and
    `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml`.
13. The terminology document carries the cross-frontend `Command` definition, and it landed before
    the protocol variant.
14. `scripts/check-desktop-boundary.sh` is unchanged and green; the desktop gained no generic
    machine-authority bridge.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **ADR-0052 is not accepted.** This is the milestone's blocker; nothing here may proceed on an
  unaccepted decision.
- The classification of a command is **contested**, or appears to change an ownership contract
  beyond ADR-0052's decision — for example if classifying a command as a frontend effect would
  take a daemon-mutating effect out of the daemon's authority.
- A **plugin or MCP command** appears to need inclusion. ADR-0052 defers dynamic command sources;
  this milestone stops and reports.
- A command appears to need an **authorization capability other than `command.execute`**. No new
  capability is invented here.
- The **terminology amendment** appears to need a canonical change the owner has not directed — a
  redefinition, a removal, or a change to an existing term.
- A **new `CoreRequest` variant** beyond the two named families (`CommandCatalog`,
  `CommandExecute`) appears necessary. More variants means the protocol surface is wider than
  ADR-0052 authorized.
- The **TUI's observable behaviour** would change. The migration's contract is that a command that
  worked before produces the same observable outcome after its execution moves. A command that did
  *not* work before — `/compact` is the confirmed case — is a **recorded discrepancy requiring an
  owner-visible decision**, not a licence to change behaviour silently.
- The work appears to require the **renderer to hold durable state or credentials**, or a generic
  request passthrough in the host — both are forbidden by `scripts/check-desktop-boundary.sh`.
- A **second command catalog** appears to be needed — in the TUI, in the host, or in the renderer.
- The **bounds** in §6 cannot be met without truncating silently.
- The implementation appears to require a **change to the daemon's scheduler, authorization model,
  or provider ownership**, or a change to the TUI's command names and aliases.
- The milestone would expand into the **editor's own commands** (M007-E) or into the **command
  palette's design** (M007-C).

## 15. Closure evidence required

The closure record at `plans/closure/desktop-frontend-ide-foundation/014-status.md` must contain:

- **ADR-0052's acceptance**, named as a precondition, with the acceptance commit or decision
  recorded. A closure without it is void.
- The **full 161-entry classification table** as an artifact of the closure, or a pointer to where
  it is committed, together with the per-form breakdown (166 forms → 134 daemon-effect, 32
  frontend-effect) and the reviewer's sign-off.
- The **resolved count discrepancy** — ADR-0052's "86" against the measured 161 dispatcher arms —
  and where it was reconciled.
- The **recorded discrepancies** list, with `/compact`'s missing daemon route named explicitly and
  the owner-visible decision recorded.
- **Proof the TUI's outcomes are unchanged**: the per-command regression evidence, the family list
  it covers, and the before/after rendered outcome for each family.
- **Proof the guards bite**: both `--self-test`s demonstrated to fail on an injected violation
  before being reverted, with the injected construct and the observed rejection recorded.
- **Proof the `Run` records are produced**: the test that shows a `Run` per daemon-effect
  execution, with its identity and principal, and the proof that a refusal produces none.
- The **catalog bound evidence**: the measured catalog size and the over-bound refusal.
- The **desktop evidence**: `npm run typecheck`, `npm test`, `npm run bindings:check`, and
  `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml` outcomes, recorded separately
  because `scripts/verify.sh quick` does not compile the desktop.
- The **full command list from §11** with actual outcomes, including any command that failed and
  its classification.
- The **documentation updates actually made**, with an explicit statement for each of the four
  ADR-0052-mandated canonical edits: made, or made-and-deferred with the owner's decision
  recorded.
- Unresolved findings classified by severity, with any medium or high finding keeping M007-D open.
- A recommendation of closed, conditionally closed, corrective pass required, or blocked.

Per `plans/003-planning-process.md` §2.5, a commit message asserting closure is not closure
evidence. A build that compiles is not closure.

## 16. Handoff notes

- **Authority order.** ADR-0052 outranks this plan. The terminology amendment outranks the
  protocol variant. The classification artifact outranks the implementation's per-command choices.
  Where repository evidence conflicts with this plan, preserve the canonical invariant, record the
  discrepancy, and make the smallest coherent adjustment.
- **Do not edit** ADR-0052, ADR-0010, ADR-0011, ADR-0012, the M007-A / M007-C / M007-E plans, the
  scheduler, the authorization model, or the provider layer. If one of them looks wrong, report
  it.
- **The daemon owns the effect; the frontend owns the formatting.** That sentence is the
  migration's load-bearing rule and the reason "the observable outcome is unchanged" is
  mechanical rather than aspirational.
- **Environment.** The root workspace is Rust **1.89** (`Cargo.toml:12`), and
  `scripts/check-desktop-boundary.sh:9-12` asserts the root MSRV has not moved. The desktop's own
  workspace declares `rust-version = "1.90"` (`apps/desktop/src-tauri/Cargo.toml:6`) and is a
  **separate workspace** (`[workspace] members = ["."]`), so the desktop's Rust toolchain may
  differ from the root's; verify the desktop with a toolchain that satisfies 1.90.
- **The desktop is not covered by `scripts/verify.sh quick`.** Its typecheck, tests, bindings
  check and build are separate commands and must be run and recorded separately.
- **Serial tests.** Repository tests mutate process-global environment; use the `ci` nextest
  profile (one process per test) for broad sweeps — `XDG_CACHE_HOME="$(mktemp -d)" cargo nextest
  run --workspace --locked --profile ci`. New `#[tokio::test]`s default to `current_thread`.
- **Storage tests.** Use `isolated_pool()` where a pool is needed; migrations run inside it.
- **The storage-layout version rule.** If any implementation adds a SQLite table for command
  audit, `STORAGE_LAYOUT_VERSION` (`crates/codegg-core/src/storage/mod.rs:50`) must move with the
  highest wired migration in `crates/codegg-core/src/session/schema.rs`, and
  `scripts/check_project_catalog_invariants.py` will enforce it. The planned implementation adds no
  table.
- **No `--all-features`.** It drags in real-server tests. Broad sweeps use
  `scripts/verify.sh full`, which pins `--features server,plugins,lsp-test-support`.
- **Preserve unrelated user changes.** Do not revert anything this milestone's author did not
  write.
- **Length.** This plan is longer than the house template because §7 carries the **161-entry
  classification table** — the milestone's central artifact — and because the 16 required
  sections each carry their own anchors. The table is inlined rather than referenced so the
  reviewer can ratify the classification without leaving the plan; §15 permits the closure to
  point at the committed artifact instead.
- **A commit message saying "closed" is not closure evidence.**
