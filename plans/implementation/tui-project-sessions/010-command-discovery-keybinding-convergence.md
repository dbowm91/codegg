# Multi-Project TUI Frontend Convergence M010 — Command Discovery and Keybinding Convergence

Status: ready for handoff; M009 remains a soft sequencing preference

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#3-work-classification`

Applicable ADRs: none.

Primary class: capability / polish

Hard dependency: M005's final built-in/global/project-scoped command-catalog contract. M009 is a soft dependency so new sidebar/agent-tree actions can share final discovery/help metadata.

Closure record to create: `plans/closure/tui-project-sessions/010-status.md`

## 1. Objective

Make CodeGG's large TUI command and keybinding surface discoverable and internally consistent without merging distinct backend domain models or creating a second command router. Extend canonical command metadata so the palette/help/keybinding views can present coherent user-facing domains, active-project availability, source/provenance, and relevant key hints from one definition surface where practical.

Audit all `InputAction` values against configurable keybinding metadata so a supported bindable action is not accidentally unreachable from customization merely because it was omitted from a parallel enum/table.

## 2. Why this milestone is blocked/readiness condition

The current registry contains more than one hundred built-in slash commands but only three broad `CommandCategory` values (`Session`, `Agent`, `System`). The palette is a small flat fuzzy list. Help, keybinding configuration, `InputAction`, command registry metadata and project/plugin dynamic command additions are partially parallel surfaces.

M005 must first make project-local command catalogs correctly scoped. M010 should enrich and present that final catalog rather than building taxonomy on top of the startup-cwd global registry.

## 3. Current implementation evidence

Before editing, census:

- every `Command::new` built-in in `src/tui/command.rs`, aliases, descriptions, dialogs/templates/process specs and dynamic-command conversion;
- `CommandCategory` and fuzzy ranking/filter behavior;
- `CommandPalette` layout, visible-result cap and selection behavior;
- slash-command parsing/dispatch in `App`/runtime command modules;
- `InputAction`, `ActionKey` or current customizable action representation, default binding construction, custom config parsing and help-line construction;
- direct hard-coded help/key labels in dialogs/status/sidebar/header;
- plugin/project command registration source/provenance and collision rules after M005;
- commands representing related but semantically distinct concepts: goal, plan, todo, jobs/tasks/schedules, runs, agent runs, worktrees, research, LSP, provider connections, collaboration, observation, diagnostics.

Do not infer backend overlap from similar names. Record each command's canonical backend owner and user task domain first.

## 4. Invariants that must not regress

- Slash command strings/aliases remain compatible unless a specific alias is already deprecated and removal is separately justified.
- One command parser/dispatcher remains canonical; palette/help are discovery surfaces, not alternate execution routers.
- Command metadata cannot grant availability/authorization. Daemon/observer/permission gates remain authoritative.
- Project/plugin command precedence/collision semantics from M005 remain deterministic.
- Fuzzy filtering remains bounded and does not perform IO on render/input keystrokes.
- Backend concepts remain semantically distinct even when grouped under one user-facing domain.
- Configurable keybindings remain deterministic and collision-checked.
- Modal/sidebar/prompt focus rules from M007/M009 remain intact.
- Narrow terminals degrade gracefully; no command becomes inaccessible because its description is truncated.

## 5. Scope

### In scope

- Richer command metadata using a small stable set of user-facing domains/categories such as Project, Session, Agent, Execution, Git/Review, Research, Provider, Collaboration, Memory, Diagnostics/System. Exact names follow census.
- Command source metadata: built-in, project-local, plugin/dynamic where already known.
- Optional scope/availability metadata used for display/filtering, with execution-time checks still canonical.
- Optional keybinding/action association for commands/actions where there is a real direct relation.
- Palette grouping/filtering/ranking and source/domain display appropriate to 100+ commands.
- Shared metadata for help/keybinding labels/descriptions where practical.
- `InputAction` <-> configurable action coverage audit and correction.
- Keybinding collision/roundtrip tests and project-catalog switching tests.

### Explicitly out of scope

- New command execution framework/router.
- Backend merger of goal/plan/todo/job/run/schedule/agent-run concepts.
- New fuzzy-search dependency unless existing utility is demonstrably insufficient.
- Interactive command argument forms/wizards for every command.
- Removing power-user slash commands in favor of menus.
- M009 sidebar implementation.

## 6. Required production changes

### Command metadata

Evolve `Command`/registry metadata so every built-in command has a coherent user-facing domain and source. Keep metadata declarative and cheap. A possible shape:

```text
Command {
  name, aliases, description,
  domain,
  source,
  scope_hint,
  dialog/template/process,
  related_input_action?,
  keywords?
}
```

Do not add fields without a consumer. `scope_hint` is presentation/discovery metadata, not authorization. If command families have rich subcommands, concise searchable keywords/examples may be preferable to dozens of extra top-level commands.

### Palette

Use M005's active catalog snapshot. Preserve fuzzy query matching across name/aliases/description and optionally domain/keywords. Display enough source/domain context to disambiguate same-named dynamic commands. Avoid a giant permanent grouped list; empty-query view may show recent/common/domain-grouped commands if current state already tracks history, while typed query should prioritize fuzzy relevance.

Do not add persistent command telemetry unless it already exists. If command history scoring exists, reuse it without creating a new store.

### Keybindings/help

Create a single canonical descriptor for configurable `InputAction` values or add compile-time/table tests proving `ActionKey` and help metadata cover them. Actions that are intentionally hard-coded/non-configurable must be explicitly enumerated with rationale, not omitted accidentally.

Generate or reuse labels/descriptions from canonical metadata where it removes real duplication. Do not force slash commands and arbitrary key actions into one enum if they are not the same concept.

### Storage/protocol/security

No storage/protocol changes expected. Project/plugin command source metadata remains frontend/runtime-asset information. Observer mode and daemon authorization still decide execution.

## 7. Ordered work packages

### Work package A — Command/action census and taxonomy

Create a table of every built-in command: current category, proposed user domain, backend owner, source/scope, aliases, direct key action if any. Separately list every `InputAction` and its configurable/help representation.

Acceptance evidence: closure record includes counts by domain and explicit disposition for every uncovered action.

### Work package B — Canonical metadata model

Implement the smallest metadata changes needed by palette/help/keybinding consumers. Migrate built-ins and M005 dynamic conversion. Add validation tests for nonempty names/descriptions, unique canonical names, alias collision handling, valid domains, and source metadata.

Acceptance evidence: registry construction catches ambiguous duplicate aliases according to documented policy.

### Work package C — Palette usability

Update palette to display/search domain/source and handle a large result set predictably. Keep rendering bounded and resize-safe. Ensure active project switch changes project-local results through M005 catalog state.

Acceptance evidence: representative queries for tests, agents, providers, research, collaboration and project commands surface relevant results within the bounded visible window; A-only project command is absent in active B.

### Work package D — Keybinding coverage convergence

Audit `InputAction` against configurable keys. Add missing configurable actions where sensible (including any current editor/diff/project/sidebar actions discovered), or explicitly mark exceptions. Preserve collision detection and user overrides.

Acceptance evidence: table/exhaustive test fails when a new configurable `InputAction` lacks a descriptor; user config round-trips representative added actions.

### Work package E — Help/docs convergence

Reuse metadata to remove divergent command/action descriptions where that is straightforward. Keep contextual dialog-specific hints local when they truly depend on mode/state.

Acceptance evidence: help/palette/keybinding names for the same action do not contradict one another; no giant generated documentation artifact is required.

## 8. Failure, cancellation, restart, and contention semantics

This is synchronous presentation metadata. It introduces no background task. Project-catalog changes are applied through M005's cached/scoped command snapshot; palette filtering does not rediscover files.

Invalid user keybinding config continues to fail/diagnose according to existing policy. A collision does not silently overwrite two actions unless current documented semantics intentionally choose last-wins; if so, surface the collision clearly in customization UI/tests.

Restart rebuilds command metadata from built-ins/runtime assets/plugins. No new persistent index is needed.

## 9. Compatibility and migration

Preserve command strings, aliases, plugin/project formats, and keybinding config syntax. If a category string is serialized in user config (verify before changing), add compatibility parsing or keep wire/config values while introducing a separate presentation domain.

Do not rename commands merely for taxonomy consistency. Better grouping/keywords should solve discoverability without migration churn.

## 10. Required tests

### Focused unit tests

- every built-in has valid domain/source metadata;
- canonical name/alias collision policy;
- fuzzy ranking includes aliases/domain/keywords as implemented;
- exhaustive configurable-action descriptor coverage;
- keybinding parse/serialize/collision cases.

### Integration tests

- command palette active-project switching using M005 scoped catalog;
- project/plugin/built-in same-name disambiguation according to policy;
- representative command-family searches;
- help and keybinding views render newly covered actions.

### Restart/recovery tests

- rebuilt project catalog after restart/restoration presents only active project's dynamic commands.

### Contention/cancellation tests

None beyond ensuring no filesystem/network task is started by palette keystrokes.

### Security/negative tests

- display availability does not bypass observer/permission/daemon execution gates;
- plugin command metadata is sanitized/bounded for terminal rendering.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- tui::command
cargo test -p codegg --lib -- tui::input
cargo test -p codegg --lib -- tui::components::dialogs::command
cargo test --test tui
# M005 scoped-command integration target
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No new CI lane or command-count gate.

## 12. Documentation updates

- `architecture/command.md`: command domains/source/scope, scoped registry lifecycle and collision policy.
- `architecture/tui.md`: palette/help/keybinding relationship.
- user-facing keybinding/help documentation and `.opencode/skills/tui/SKILL.md` as applicable.

## 13. Acceptance criteria

M010 closes when the active project's full command set remains accessible but is substantially easier to discover by task/domain; palette/help/keybinding metadata no longer diverges unnecessarily; project/plugin/built-in sources are disambiguated; every intended configurable `InputAction` has a tested descriptor or explicit exception; and no backend concept/router/authorization owner was merged or duplicated.

## 14. Stop conditions

Stop if:

- M005 is not closed;
- category/domain values are a serialized public contract and changing them would require an unplanned migration;
- implementation proposes a new command execution router or persistent index;
- taxonomy work starts renaming/removing supported commands rather than improving discovery;
- a new fuzzy/search dependency is proposed without measured need.

## 15. Closure evidence required

Include implementation commits, full command/action census, final domain/source taxonomy, command/alias collision evidence, project-scoped palette test, configurable-action coverage matrix, representative usability search tests, focused/broad verification outcomes, compatibility review, and severity-classified residual findings.

## 16. Handoff notes

Implement after M005. If M009 has closed, include its sidebar/agent-tree actions in the same keybinding/help audit; otherwise keep an explicit soft-dependency note and do not block core command discovery on M009.
