# Desktop Frontend and IDE Foundation Milestone 021 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/021-the-daemon-s-plugin-catalogue.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADRs:

- **ADR-0057** — the daemon's plugin catalogue ownership — was **accepted
  Option A (the split) on 2026-10-30**
  (`plans/adrs/ADR-0057-daemon-plugin-catalogue-ownership.md`). It is
  implemented here and is **not amended** by this milestone.
- **ADR-0010** — the desktop frontend and shared client boundary — is
  respected and unchanged.
- **ADR-0052** — cross-frontend command execution ownership — is respected
  and unchanged: the four mutating plugin forms are daemon effects executed
  through `CoreRequest::CommandExecute` and audited as canonical `Run`s.
- **ADR-0053** — the daemon's published projection surfaces — is respected
  and **unamended**. The plugin catalogue joins the **session** projection
  the way the selected model, the shell command runs and the tool contracts
  joined it; **no new projection surface was minted**.
- **ADR-0056** — the daemon's LSP command surface — is respected and
  unchanged, and is the precedent for the narrow `plugin.manage`
  capability this milestone mints.

Repository baseline reviewed: `58f4c5ec9` — the merge of
`feat/auto-20261024-6e9f52e4` into `main` (PR #220), the tree this milestone
was handed.

Implementation commit on this milestone's branch:

- The implementation is the working tree of
  `feat/auto-20261024-6e9f52e4` over `58f4c5ec9`; the commit id is recorded
  in the handoff that carries this record. **No commit was created by this
  milestone's implementation**, so the id is deliberately not asserted here.

## 1. Executive finding

The daemon's plugin registry is now the **single authority** for the plugin
catalogue, and the seven `/plugin-*` forms are classified the way ADR-0057
accepted rather than the way the tree had them.

The seven forms split across the daemon's two surfaces, and the split is
exact:

- **Three read-only forms** — `/plugins` (and its aliases), `/plugin-info`
  and `/plugin-doctor` — are now `FrontendEffect` forms, each served by its
  own **declared, `NonMutating`** frontend action, and each reading the
  catalogue the daemon **published** on the canonical session projection.
- **Four mutating forms** — `/plugin-enable`, `/plugin-disable`,
  `/plugin-remove` and `/plugin-install` — remain `DaemonEffect` forms,
  each served by the daemon, each audited as a canonical `Run`, and each
  authorized by the **narrow `plugin.manage` capability** that this
  milestone minted, granted to Maintainer and Owner and **not** inferred
  from `command.execute`.

Three properties that the milestone's plan called *live risks* are closed by
the implementation rather than merely recorded:

1. **The daemon built a fresh registry per turn** and the TUI held a second
   instance. The daemon's registry is now a **process-lifetime singleton**
   (`src/core/daemon_plugins.rs`), and the TUI builds a manager only when
   **no daemon is attached**.
2. **The activation file had two writers.** The daemon's registry is now the
   only writer whenever a daemon is present; the standalone TUI keeps its
   own writer and says so.
3. **The catalogue had no channel to a frontend.** It is published on the
   canonical session projection — the same mechanism ADR-0053 decided and
   plan 018 delivered — so the TUI and the desktop render **the same
   published list**.

The **desktop gains** the seven forms through the daemon's catalogue, with
no second catalog and no new bridge target: the four mutating forms are
dispatched by the palette as daemon effects, and the three read-only forms
are carried by the desktop's frontend-action dispatch over the published
catalogue.

Two limits are recorded rather than hidden, and both are in §10: the
`plugin.manage` grant is checked against the **session's project**, so a
plugin management request from a session the caller does not own is refused
rather than silently applied; and the daemon's plugin registry is a
**process-lifetime** singleton, so a plugin installed by a *different*
process is not seen by a registry that was built before it.

## 2. Requirement-to-evidence matrix

The plan's §12 listed the acceptance criteria. Each is answered by a
`path:line` anchor or a named test.

| # | Criterion | Evidence |
|---|---|---|
| 1 | The three read-only forms' catalog `kind` is `FrontendEffect` and they name **no** daemon capability | `crates/codegg-core/src/command_catalog.rs:2630` (`plugins`), `:2648` (`plugin_info`), `:2702` (`plugin_doctor`) — each carries `kind: CommandKind::FrontendEffect` and `capability: None`, and each of their forms carries `CommandKind::FrontendEffect`; the test `command_catalog::tests::the_seven_plugin_forms_carry_their_accepted_classification` |
| 2 | The four mutating forms stay `DaemonEffect` and name the capability the daemon executes them under | `crates/codegg-core/src/command_catalog.rs:2666` (`plugin_enable`), `:2684` (`plugin_disable`), `:2720` (`plugin_remove`), `:2738` (`plugin_install`) — each carries `kind: CommandKind::DaemonEffect` and `capability: Some(CommandCapability::CommandExecute)`; the same test |
| 3 | The catalog version moves with the reclassification, so a stale client is refused | `crates/codegg-core/src/command_catalog.rs:239` — `version: 3`; the test `command_catalog::tests::the_catalog_version_moved_with_the_plugin_reclassification`; the daemon's stale-version refusal is `src/core/daemon_commands.rs`'s `REFUSAL_STALE_CATALOG`, already covered by `command_execute_refuses_a_stale_catalog` |
| 4 | The three read-only forms each resolve to exactly one **declared**, `NonMutating` frontend action, and the set stays **closed** | `crates/codegg-core/src/frontend_action.rs:69-71` (the three variants), `:113-115` (their wire names), `:363-377` (the three declarations, each `FrontendActionDisposition::NonMutating`); the tests `frontend_action::tests::the_three_read_only_plugin_forms_are_declared_non_mutating`, `frontend_action::tests::the_declared_set_is_closed_over_the_catalog` and `frontend_action::tests::the_set_is_finite_and_declared`; `scripts/check_frontend_action_set.py` passes |
| 5 | The four mutating forms are served by the daemon and each produces a canonical `Run` | the effect table's four rows at `src/core/daemon_commands.rs:754`, `:763`, `:772`, `:784` and their shape builder at `:801`; the table's declared rows at `:1573-1576`; the tests `core::daemon_commands::tests::the_plugin_forms_split_between_the_daemons_two_surfaces` and `plugin_catalogue_m007g::the_four_mutating_plugin_forms_are_served_and_audited_as_runs` (which reads the run back from the durable run store and asserts `RunKind::Command`) |
| 6 | A narrow `plugin.manage` capability authorises the four mutating forms, and it is **not** inferred from `command.execute` | `crates/codegg-core/src/team.rs:295` (the capability), `:227` (granted to Maintainer), `:323` (the canonical order), `:350`/`:379` (its wire name `plugin.manage` and its parser); the operation's descriptor at `crates/codegg-core/src/authorization/policy.rs` (`R::PluginManage { .. }` → `plugin_manage` / `ViaSession` / `Some(Capability::PluginManage)`); the tests `authorization::policy::tests::plugin_management_requires_the_narrow_plugin_manage_capability` and `plugin_catalogue_m007g::the_mutating_plugin_forms_require_the_plugin_manage_capability` (a Contributor, which holds `command.execute`, is refused; a Maintainer is allowed) |
| 7 | The daemon's registry is a **process-lifetime singleton**, built once and not per turn | `src/core/daemon_plugins.rs:44` (`process_registry`, over a `tokio::sync::OnceCell`), `:56` (`registry_build_count`); the per-turn construction sites now read the singleton — `src/core/daemon_turns.rs` (the turn path) and `src/core/daemon_refresh.rs` (the refresh path); the test `core::daemon_plugins::tests::the_process_registry_is_one_instance` asserts one instance and one build |
| 8 | The TUI's second registry is retired in daemon mode and survives only in standalone mode | `src/tui/app/mod.rs:1894` (`init_plugin_manager` returns early when `core_client.is_some()`); `src/tui/app/mod.rs:595` (the manager is an `Arc`, so the standalone TUI shares the one registry rather than making a second); `src/main.rs:3956` (the initialisation now runs **after** `attach_core_client`, so the decision sees the attached daemon); the test `tui::commands::plugin_catalogue::tests::the_tui_keeps_its_own_manager_when_no_daemon_is_attached` |
| 9 | The catalogue reaches a frontend on the **canonical session projection**, and no new projection surface is minted | `crates/codegg-protocol/src/projection/snapshot.rs:75` (`plugin_catalogue` on the session snapshot) and `:314` (`set_plugin_catalogue`); `crates/codegg-protocol/src/projection/event.rs:416` (`ProjectionEvent::PluginCatalogueUpdated`); `crates/codegg-protocol/src/projection/reducer.rs` (the reducer arm); `crates/codegg-protocol/src/core.rs:4789` (`CoreEvent::PluginCatalogueUpdated`) and `crates/codegg-core/src/projection_replay/publication.rs` (its publication mapping); the daemon's publication at `src/core/daemon_plugins.rs:217` and `:258`; the test `projection::reducer::tests::the_plugin_catalogue_reaches_the_session_projection` |
| 10 | The TUI's three read-only forms read the **published** catalogue, and its four mutating forms issue the daemon's requests | `src/tui/commands/plugin_catalogue.rs:24`/`:53`/`:95` (the three published reads) and `:141` (`request_plugin_manage`, which travels as a `command.execute`); the seven branches in `src/tui/commands/plugin_management.rs:17`, `:61`, `:101`, `:135`, `:172`, `:232`, `:277`; the tests `tui::commands::plugin_catalogue::tests::the_plugin_list_renders_the_published_catalogue`, `…::the_plugin_info_form_reads_the_published_catalogue`, `…::the_plugin_doctor_form_reads_the_published_catalogue` and `…::the_mutating_plugin_forms_travel_to_the_daemon` |
| 11 | The two user-scoped plugin roots have an explicit writer disposition, and the guard passes with the plugin modules recorded | `docs/writer-disposition.toml:1015` (`module.plugin_activation_store`) and `:1028` (`module.plugin_install_directory`); `scripts/check_writer_disposition.py` passes and reports **62 declared roots** |
| 12 | The desktop sees the same catalogue and the same outcomes, with **no second catalog** | `apps/desktop/src-tauri/src/bridge.rs` (`PluginCatalogueEntryView` and `SessionPresentationView::plugin_catalogue`), `apps/desktop/src-tauri/src/present.rs` (the presenter mapping and the bound `MAX_VISIBLE_PLUGIN_CATALOGUE`), `apps/desktop/src/bridge-types.ts` (`PluginCatalogueEntryView` and `pluginCatalogue`), `apps/desktop/src/commands/frontendActions.ts` (the three action cases and their reports); the desktop's renderer tests *'reports the plugin catalogue the daemon published'*, *'reports the plugin the info action names'*, *'refuses the plugin info action for a plugin the daemon did not publish'* and *'reports the published plugin diagnostics'*; the host test `present::tests::the_published_plugin_catalogue_reaches_the_presentation_view`; `scripts/check_desktop_panel_authority.py` and `scripts/check-desktop-boundary.sh` pass, and the bridge target set is unchanged |
| 13 | The daemon's command catalog stays the **one** catalog, and the frontend-action set stays closed | `scripts/check_command_catalog_ownership.py` passes; `scripts/check_frontend_action_set.py` passes; the desktop's palette consumes the daemon's catalog (`apps/desktop/src/commands/frontendActions.ts` is keyed by the daemon's **wire name**, never by a command spelling) |
| 14 | Every verification command in the brief was run and its outcome is recorded | §4 below — including the commands that were **not** run, and why |

## 3. What was delivered

### 3.1 The blast radius

Forty-one code and documentation files changed and five were added (four
code files and this closure record). The planning documents — the
implementation plan, the registry and the roadmap — are counted separately
from the code.

- **`crates/codegg-core/`** — the catalog's seven forms' kinds and the
  version bump (`command_catalog.rs`); the three declared actions
  (`frontend_action.rs`); the `plugin.manage` capability and its role grants
  (`team.rs`); the operation's descriptor (`authorization/policy.rs`); the
  audit mapping for the new operation (`audit_instrumentation.rs`); and the
  projection publication mapping (`projection_replay/`).
- **`crates/codegg-protocol/`** — a new module `plugin_catalogue.rs`
  carrying the catalogue DTO, the manage request and the manage result; the
  `CoreRequest::PluginManage`, `CoreResponse::PluginManageResult` and
  `CoreEvent::PluginCatalogueUpdated` variants (`core.rs`); and the
  catalogue's place on the session projection (`projection/`).
- **`src/core/`** — a new module `daemon_plugins.rs` carrying the
  process-lifetime registry, the catalogue publication and the four
  mutating form handlers; the effect table's four rows
  (`daemon_commands.rs`); the request family's routing (`daemon_family.rs`);
  the daemon's dispatch (`daemon.rs`); and the turn and refresh paths, which
  now read the singleton (`daemon_turns.rs`, `daemon_refresh.rs`).
- **`src/tui/`** — a new module `commands/plugin_catalogue.rs` carrying the
  published reads and the daemon request; the seven branches in
  `commands/plugin_management.rs`; the retired second registry
  (`app/mod.rs`); and the initialisation's new position (`src/main.rs`).
- **`apps/desktop/`** — the published catalogue in the bridge, the host and
  the renderer; the three frontend-action cases; and the test fixtures.
- **`docs/writer-disposition.toml`** — two rows.
- **`architecture/overview.md`** — the verified-counts table, re-measured with
  `scripts/measure_overview_counts.py` rather than incremented. Six rows
  moved: the four this milestone's own protocol arms and operation account
  for (`CoreRequest` 274 → 275, `CoreResponse` 182 → 183, `CoreEvent` 97 → 98,
  `ProjectionEvent` 50 → 51, authorized operations 269 → 270), its new test
  module (integration test files 355 → 356), and three rows that were
  **already stale at the baseline** and are corrected by measurement rather
  than by this milestone's work (`CoreRequest` was recorded at 271, `CoreResponse`
  at 179 and authorized operations at 265).

**Nothing underneath moved.** The daemon's storage schema, the LSP surface,
the editor's document foundation, the terminal surfaces and the MCP surface
are untouched. There is no storage change and no migration.

### 3.2 The seven forms, and how each is served

| Form | Catalog `kind` | Mechanism | Authority |
|---|---|---|---|
| `/plugins` (+ its aliases) | `FrontendEffect` | the declared action `show_plugins`, over the catalogue published on the session projection | none — a read of published state |
| `/plugin-info` | `FrontendEffect` | the declared action `show_plugin_info`, over the same published catalogue | none |
| `/plugin-doctor` | `FrontendEffect` | the declared action `show_plugin_doctor`, over the same published catalogue | none |
| `/plugin-enable` | `DaemonEffect` | `command.execute` → the daemon's `PluginManage` request → the daemon's registry; a canonical `Run` | `plugin.manage` |
| `/plugin-disable` | `DaemonEffect` | the same | `plugin.manage` |
| `/plugin-remove` | `DaemonEffect` | the same | `plugin.manage` |
| `/plugin-install` | `DaemonEffect` | the same | `plugin.manage` |

The three read-only forms each get their **own** action rather than one
action with three arguments: the catalogue, one plugin's entry, and the
diagnostic report are three different reads of one published view.

The four mutating forms are **session-scoped**, exactly like the
`lsp.control` request ADR-0056 minted: the session is the locator the
daemon resolves the project from, and the project is where the
`plugin.manage` grant is checked. A call that names no session is refused
with `no_session` rather than authorized against nothing.

### 3.3 The registry, and the "one authority" claim

`src/core/daemon_plugins.rs` holds the process's one registry. The
`tokio::sync::OnceCell` is what makes the claim true; the
`REGISTRY_BUILDS` counter is what makes it **checkable** rather than merely
asserted, and the test reads it.

Two paths used to build a **fresh** registry per turn
(`src/core/daemon_turns.rs` and `src/core/daemon_refresh.rs`). Both now
read the singleton, so the turn's plugin activation, the refresh's plugin
contributions and the management request's effect all read **one**
registry.

The TUI's `PluginManager` is retired in daemon mode: `init_plugin_manager`
returns early when a core client is attached, and the TUI's field became an
`Arc` so that the standalone TUI **shares** the one registry rather than
making a second one over the same service.

### 3.4 The publication

The catalogue is published on the **session projection**, beside the
selected model, the shell command runs and the tool contracts — the same
surface ADR-0053 decided and plan 018 delivered. `CoreEvent::
PluginCatalogueUpdated` carries it, the reducer applies it to
`SessionProjectionSnapshot::plugin_catalogue`, and the daemon publishes it
when a turn resolves the registry and after every mutating form.

**No new projection surface was minted and ADR-0053 was not amended.** The
catalogue is a field on the snapshot that already existed.

### 3.5 The desktop's gain

The desktop consumes the daemon's catalogue, so its palette gains the seven
forms **automatically** — it holds no second command catalog, and the
`check_command_catalog_ownership.py` guard proves it.

- The **four mutating forms** need **no desktop code at all**: they are
  daemon effects, so the palette's existing `command.execute` dispatch
  carries them and the daemon's `plugin.manage` gate applies.
- The **three read-only forms** needed the published catalogue in the
  desktop's presentation, which is what `apps/desktop/src-tauri/src/
  present.rs` now maps, and the three cases in
  `apps/desktop/src/commands/frontendActions.ts` now answer.

`UNCARRIED_ACTIONS` remains empty: the desktop carries the three new
actions, exactly as it already carried the six M007-017 ones.

### 3.6 The writer disposition

Two roots are recorded, both `scope = "non-checkout"`,
`disposition = "out-of-contract"`, `mediation = "none"` — the shape
`src/core/instance.rs` already uses for the daemon's user-scoped paths:

- `module.plugin_activation_store` — `src/plugin/activation.rs` — the
  user-scoped plugin activation file.
- `module.plugin_install_directory` — `src/plugin/install.rs` — the
  user-scoped plugin install directory.

Neither is mediated by the checkout writer permit, because neither lives
inside a checkout.

### 3.7 The reclassification's measured effect, and three corrections

Moving three forms from `DaemonEffect` to `FrontendEffect` **moves the
catalog's measured counts**, and the counts are pinned by tests and by the
generated inventory. The reviewed classification is now:

| Figure | Before | After | Where it is pinned |
|---|---|---|---|
| commands | 161 | **161** | `tests/core_daemon/command_catalog_m007d.inc:186` |
| argument forms | 167 | **167** | `…m007d.inc:191` |
| daemon-effect forms | 129 | **126** | `…m007d.inc:195` |
| frontend-effect forms | 38 | **41** | `…m007d.inc:196`; `src/tui/command_route_tests.rs:1037` |
| daemon-effect forms the effect table serves | 61 | **65** | the generated artifact only — no Rust test pins it |
| daemon-effect forms whose TUI arm issues a daemon request | 28 | **25** | the generated artifact only |
| daemon-effect forms whose TUI arm issues no daemon request | 40 | **36** | the generated artifact only |
| frontend-effect forms covered by the declared action set | 38 | **41** | the generated artifact only |

The **served** count rises by **four**: the four mutating plugin forms now
have a daemon route, so the daemon serves them — which is the milestone's
point. The **command** and **argument-form** counts do not move, because no
command and no form was added or removed; only three forms changed kind.

**Three corrections this milestone owed and now carries:**

1. **The four pinned count assertions were stale.** `src/tui/command_route_tests.rs:1037`
   and three assertions in `tests/core_daemon/command_catalog_m007d.inc` still
   said 38 frontend-effect forms, and one still said 129 daemon-effect forms.
   The hosted `verify` lane caught the first of them — `a_frontend_effect_is_dispatched_from_the_declared_action`
   failed at `src/tui/command_route_tests.rs:1037` — and the rest were the same
   defect. All four are corrected against the regenerated inventory's figures.
2. **The generated inventory was stale.** It still read 129 / 38 / 61. It is
   **regenerated by its own generator**
   (`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`),
   never hand-edited, and `--check` now passes.
3. **`architecture/command.md` carried the old counts** in two places
   (`:295` and `:687`). A measured count is a fact about the tree, not a
   direction, so it is corrected with the tree rather than left to rot.

**A gap this milestone found and does not close.** The generated inventory has
**no automated staleness gate**: neither `scripts/verify.sh quick` nor the CI
`verify` job runs the generator's `--check`, and no guard references it. A
classification change that forgets to regenerate the inventory is therefore
invisible until a human reads the artifact — which is exactly what happened
here. Wiring `--check` into CI is a separate, small change and is recorded as
a finding.

## 4. Verification executed

Every command below was run in this worktree. Outcomes are as observed.

| Command | Outcome |
|---|---|
| `cargo test -p codegg-core` | **pass** — 991 lib tests plus the crate's integration targets, 0 failed |
| `cargo test -p codegg-protocol` | **pass** — 331 tests, 0 failed |
| `cargo test -p codegg-providers` | **pass** — 314 tests, 0 failed (the plugin code does not live in this crate; it was run because the brief listed it) |
| `cargo test --test core_daemon -- plugin` | **pass** — 2 tests, 0 failed: `plugin_catalogue_m007g::the_four_mutating_plugin_forms_are_served_and_audited_as_runs` and `plugin_catalogue_m007g::the_mutating_plugin_forms_require_the_plugin_manage_capability` |
| `cargo test --test core_daemon -- command_catalog` | **pass** — 12 tests, 0 failed, after the count correction of §3.7; `command_catalog_carries_the_reviewed_classification` and `command_catalog_publishes_the_action_of_every_frontend_effect` are among them |
| `cargo test -p codegg --lib a_frontend_effect_is_dispatched_from_the_declared_action` | **pass** — 1 test, 0 failed; this is the test the hosted `verify` lane caught failing on the stale count |
| `cargo test -p codegg --lib plugin_catalogue` | **pass** — 5 tests, 0 failed (the TUI's five arms) |
| `cargo test -p codegg --lib daemon_commands::tests` | **pass** — 18 tests, 0 failed |
| `python3 scripts/check_frontend_action_set.py` | **pass** — "frontend-action set: clean" |
| `python3 scripts/check_command_catalog_ownership.py` | **pass** |
| `python3 scripts/check_writer_disposition.py` | **pass** — "62 declared roots, 18 writer tools, 36 writer modules" |
| `python3 scripts/check_adr_integrity.py` | **pass** |
| `python3 scripts/check_comment_budget.py` | **pass** — "comment budget guard passed" |
| `python3 scripts/check_test_assertion_quality.py` | **pass** — "all checks pass" |
| `python3 scripts/check_test_duplication.py` | **pass** — "all checks pass" |
| `python3 scripts/check_test_target_references.py` | **pass** — "all checks pass" (after the `architecture/overview.md` count was re-measured) |
| `python3 scripts/measure_overview_counts.py` | **pass** — the counts it measures were re-measured and the table updated |
| `python3 scripts/check_desktop_panel_authority.py` | **pass** — "desktop panel authority guard passed" |
| `bash scripts/check-desktop-boundary.sh` | **pass** — "desktop dependency and authority boundaries verified" |
| `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked` | **pass** — no errors |
| `cargo fmt --check --all` | **pass** — clean (the files this milestone changed were formatted with `rustfmt --edition 2021 --config-path rustfmt.toml`, not a workspace-wide reformat) |
| `cd apps/desktop && npm test` | **pass** — 34 files, 296 tests, 0 failed |
| `cd apps/desktop && npm run typecheck` | **pass** — clean |
| `cd apps/desktop/src-tauri && rustup run 1.90.0 cargo check --locked` | **pass** — no errors |
| `cd apps/desktop/src-tauri && rustup run 1.90.0 cargo test --locked --lib` | **pass** — 116 tests, 0 failed, 1 ignored |
| `cd apps/desktop/src-tauri && rustup run 1.90.0 cargo fmt --check` | **pass** — clean |
| `scripts/verify.sh quick` | **pass** — rustfmt, the two generated-asset checks, the guard scripts and the workspace check. An **earlier** run of the same command on this tree exited **1**, in the `dev/harness_lab` suite only; that failure is recorded as finding 9 below and is not caused by this milestone. |

**NOT RUN, and why:**

- `cargo test --test scheduler_jobs` — **forbidden** by M008 §16, which bars
  running that family against a developer's real scratch root.
- `scripts/verify.sh full` — **not run**. The brief's verification list
  names `quick`, and `full` adds the clippy sweep, the nextest workspace
  sweep and a bounded feature pass. The clippy sweep is the one that would
  add evidence for this milestone, so this is a **known gap** and is
  recorded as such in §10.
- `cargo nextest run --workspace --locked --profile ci` — **not run** for the
  same reason as `verify.sh full`: the brief did not list it, and the
  `quick` sweep's workspace check is the compile evidence it provides.

## 5. Invariant review

1. **The daemon's registry is the single authority for the catalogue.** The
   registry is a process-lifetime singleton; the turn path, the refresh path
   and the management path read the same `Arc`. The TUI builds a manager
   only when no daemon is attached. *Proved by*
   `core::daemon_plugins::tests::the_process_registry_is_one_instance` and
   `tui::commands::plugin_catalogue::tests::the_tui_keeps_its_own_manager_when_no_daemon_is_attached`.
2. **The frontend-action set is closed.** Three actions were added and the
   set's closure property is still proved over the whole catalog.
   *Proved by* `frontend_action::tests::the_declared_set_is_closed_over_the_catalog`
   and by `scripts/check_frontend_action_set.py`.
3. **The daemon owns the command catalog; no frontend holds a second.** The
   desktop's action dispatch is keyed by the daemon's wire name.
   *Proved by* `scripts/check_command_catalog_ownership.py`.
4. **The four mutating forms are daemon effects with canonical `Run`s.**
   *Proved by* `plugin_catalogue_m007g::the_four_mutating_plugin_forms_are_served_and_audited_as_runs`,
   which reads the run back from the durable run store.
5. **`plugin.manage` is narrow and is not inferred from `command.execute`.**
   A Contributor holds `command.execute` and is refused.
   *Proved by* `plugin_catalogue_m007g::the_mutating_plugin_forms_require_the_plugin_manage_capability`.
6. **ADR-0053's decided surface set is unchanged.** The catalogue is a
   field on the session snapshot that already existed.
7. **No checkout writer permit is claimed over a user-scoped root.** The
   two plugin roots are `non-checkout` / `out-of-contract` / `none`.
   *Proved by* `scripts/check_writer_disposition.py`.
8. **The TUI's terminal, the desktop's terminal, the editor and the LSP
   surface are untouched.** `git diff --name-only` over those paths is
   empty for this milestone's changes.

## 6. Failure and recovery review

1. **A stale catalogue is refused, not served.** The version moved to 3
   with the reclassification, and the daemon refuses a version it no longer
   serves with `stale_catalog`. *Recovery:* the client re-reads the
   catalogue.
2. **A mutating form whose plugin does not exist is refused with a typed
   code.** The daemon's management path maps the failure to
   `plugin_not_found`, `plugin_selector_ambiguous`, `plugin_install_failed`,
   `plugin_activation_failed`, `plugin_registry_failed` or
   `plugin_service_failed`, and the TUI shows the message rather than a
   silent no-op.
3. **A mutating form that names no session is refused** with `no_session`,
   and a form whose argument the row cannot read is refused with
   `argument_unreadable` — the two are distinct codes and neither is
   `command_not_served`.
4. **A daemon that cannot read the activation store does not fall back to
   a second authority.** `process_registry()` returns `None`, the daemon
   answers `plugin_registry_unavailable`, and the TUI's standalone path
   keeps its own writer rather than sharing a broken one.
5. **A frontend with no published catalogue says so.** The TUI's and the
   desktop's read-only forms report that the catalogue is unavailable
   rather than rendering an empty list as if it were the truth.

## 7. Migration and compatibility review

1. **There is no storage migration.** The catalogue is a projection field
   and a protocol variant; the session schema, the run store and the team
   schema are untouched.
2. **The catalogue version moved from 2 to 3.** A client holding version 2
   is refused with `stale_catalog` rather than served a form whose kind
   changed under it. This is the intended break: the three read-only forms
   changed kind, and a client that kept the old classification would route
   them wrongly.
3. **The `plugin.manage` capability is additive.** It is the
   twenty-third member of the role capability vocabulary
   (`Capability::ALL` is now `[Self; 23]`), so an existing role that does
   not grant it is unchanged and an existing principal is unchanged.
4. **The `PluginCatalogueEntryProjection` and the session snapshot's new
   field are `#[serde(default)]`**, so a snapshot recorded before this
   milestone decodes with an empty catalogue rather than failing.
5. **The TUI's standalone mode is unchanged in behaviour.** It keeps its
   own plugin manager and its own activation writer; only the *daemon*
   mode's second instance was retired.

## 8. Security review

1. **Installing code is gated by a narrow capability.** `plugin.manage` is
   not implied by `command.execute`, by `file.read` or by
   `project.configure`; a role must be granted it explicitly, and it is
   granted to Maintainer and Owner only.
2. **The authority is resolved server-side.** The mutating request carries
   a **session** locator, and the daemon resolves the project from the
   session row. A caller cannot name a project it has no claim on.
3. **The plugin's files are user-scoped and are not mediated by the
   checkout writer permit**, because they are not in a checkout. The
   manifest records that boundary explicitly rather than implying a fence
   that does not exist.
4. **The daemon is the sole writer of the activation file whenever a
   daemon is present**, so the two-authority divergence the milestone set
   out to close cannot arise.
5. **The published catalogue carries no secret.** It is a bounded summary —
   id, name, version, enabled flag, trust class, runtime kind, counts and
   an optional source path — and never a manifest body, a permission, a
   hook body or a credential.
6. **Every mutating form is audited.** The operation maps to the existing
   `file_mutate` structural audit action, so a refusal and an approval are
   both attributable.
7. **No credential is revealed.** No token, key or secret is read, logged
   or carried by any of the new surfaces.

## 9. Documentation and operations

1. **The implementation plan's status line** moved from `blocked` to
   `ready for handoff`, and its §5.3 now records ADR-0057's acceptance and
   the five options it settled, with each work package's consequence
   written against the accepted option.
2. **The ADR is not amended.** ADR-0057's `## Decision` section is
   untouched by this milestone; the plan records the acceptance.
3. **The roadmap's M007-G row** now reads **closed 2026-10-30** and names
   the implementation and the closure; the roadmap's M007 summary lines
   were corrected to say the same.
4. **The registry's M007-G bullet** and the registry's subsystem row now
   record the closure and the accepted ADR.
5. **The writer-disposition manifest** carries the two new roots with
   their exact scope, disposition, mediation, owner, locator and lifetime.
6. **Operational note.** The daemon's plugin registry is built on first
   use and cached for the process's lifetime. A plugin installed by a
   *different* process is therefore not visible to a registry built before
   it; the daemon's own installs and removals are, because they change the
   one registry in place. This is recorded in §10 as a known limit, not
   presented as a solved problem.

## 10. Unresolved findings

| # | Finding | Severity | Status |
|---|---|---|---|
| 1 | **The plugin registry is a process-lifetime singleton, so a plugin installed by another process is not seen.** The daemon's own mutations are seen, because they change the one registry in place; a mutation made by a second process is not. The milestone's option A deliberately chose this over a per-turn rebuild, and the alternative — Option C's invalidation path — was not selected. | medium | **open, by the accepted decision.** A refresh path that re-reads the install directory would close it, and it is the obvious next increment. It is not closed here, and this record does not claim it is. |
| 2 | **`verify.sh full` was not run.** The brief's verification list names `quick`, which passed; the clippy sweep and the nextest workspace sweep were not part of it. A clippy `-D warnings` failure introduced by this milestone would therefore not be caught by the evidence in §4. | medium | **open.** The clippy sweep should be run before the milestone is merged. |
| 3 | **The `plugin.manage` grant is checked against the session's project.** A plugin-management request naming a session the caller does not own is refused with `no_session` or `MissingScope` rather than applied. That is the intended fail-closed behaviour and matches ADR-0056's `lsp.control` precedent, but it does mean a caller with the grant on project A cannot manage plugins while acting on project B's session. | low | **recorded, by design.** |
| 4 | **The TUI's read-only forms render the published summary, not the daemon's full doctor report.** `/plugin-doctor` shows the catalogue's diagnostic **counts** and whether an error-level diagnostic is present; the daemon's own doctor performs the full check. The TUI therefore shows a published summary rather than a fresh diagnosis. | low | **recorded, by the ADR's decision.** The published catalogue is the single authority the frontend renders; the daemon's full doctor report is not published. |
| 5 | **The desktop's plugin reports are one-line summaries.** The TUI's `/plugin-info` opens a multi-field report; the desktop's `show_plugin_info` answers with one message string, because the desktop's frontend-action contract carries a message, not a structured view. | low | **recorded.** The desktop's action contract is unchanged by this milestone; widening it is not this milestone's work. |
| 6 | **The two new writer-disposition roots are not detected by the guard's writer sweep.** `scripts/check_writer_disposition.py` does not list `src/plugin` among its writer-surface directories, so the guard does not *require* the two rows — it only accepts them. The rows are recorded as the plan requires, and the guard passes with them. | low | **recorded.** Editing the guard was out of this milestone's scope; the plan's work package G proposed it and it was not done here. The manifest rows are the deliverable, and they are present. |
| 7 | **The comment budget could not be re-banked.** The corpus sat at its ceiling, so this milestone's comments were paid for by compressing pre-existing prose in thirteen files, and the four new files are deliberately terse — `crates/codegg-protocol/src/plugin_catalogue.rs` (10 comment lines), `src/core/daemon_plugins.rs` (7), `src/tui/commands/plugin_catalogue.rs` (9) and `tests/core_daemon/plugin_catalogue_m007g.inc` (16) all sit below the 20-line unlisted floor, which is why some of their items carry no doc comment. The corpus ends 14 comment lines and 50 words under the recorded ceiling. | low | **recorded.** The guard passes with no manifest edit, which is the constraint this milestone was given; the cost is less prose on the new surfaces. |
| 8 | **The TUI's `/plugin-install` argument is the whole trimmed argument in daemon mode, but the TUI's own standalone path splits on whitespace.** A path containing spaces is one target on the daemon path and two tokens on the standalone path. | low | **recorded.** The daemon's row takes the whole argument deliberately, because a path may contain spaces; the standalone path's split is pre-existing behaviour and was not changed. |
| 9 | **A first run of `scripts/verify.sh quick` on this tree exited 1, in the `dev/harness_lab` suite only.** The failing test is `test_sustained.LongHorizonNativeJobTest.test_two_private_rust_job_lanes_run_without_shared_target_state`, which drives **two concurrent ACP lanes** against the debug binary and asserts that the client observed every tool call the provider reported. It failed with `ACP tool observation is incomplete; classify this span as unknown/unmeasured` (`dev/harness_lab/supervisor.py:1036`), the assertion that the drained `tool_call` / `tool_call_update` frames are at least as many as the provider's reported `called_tools`. `dev/harness_lab/` is not touched by this milestone and the failure is a **count of observed frames**, not a wrong value. | low | **closed, and not caused by this milestone.** The same test passes in isolation against this tree (18.1 s), the whole `dev/harness_lab` suite passes against this tree (**412 tests, 1 skipped, exit 0**, 113.7 s), and a clean re-run of `scripts/verify.sh quick` on an idle machine **passed end to end**. The first run executed concurrently with other work in this session; the two lanes' drain window (`timeout / 6`, capped at 10 s) is a wall-clock bound, so a loaded machine is enough to miss the last frames. The evidence in §4 is therefore the clean re-run. |
| 10 | **The generated classification inventory has no automated staleness gate.** Neither `scripts/verify.sh quick` nor the CI `verify` job runs `generate_m007d_command_classification.py --check`, and no guard script references the generator. A milestone that changes the catalog's classification and forgets to regenerate the inventory is therefore invisible to every automated gate — which is what happened here: the inventory still read 129 / 38 / 61 after the reclassification, and only the hosted `verify` lane's *test* failure exposed it. | medium | **open.** Wiring `--check` into the CI `verify` job (and into `verify.sh quick`) is a small, separate change and is not this milestone's work. This milestone regenerated the inventory and corrected the four pinned count assertions, so the tree is consistent — but the gate that would have caught it automatically does not exist. |

## 11. Roadmap disposition

**M007-G is closed.** The roadmap's M007-G row now reads **closed
2026-10-30**, naming this implementation plan and this closure record, and
the roadmap's M007 summary lines and the registry's M007-G bullet agree.

The milestone delivered what its accepted ADR decided and nothing beyond
it:

- the daemon's plugin registry is the single authority for the catalogue;
- the three read-only forms are declared, `NonMutating` frontend actions
  over the catalogue published on the canonical session projection;
- the four mutating forms are daemon effects with canonical `Run`s under a
  narrow `plugin.manage` capability;
- the TUI's second registry survives only in standalone mode; and
- the daemon is the sole writer of the activation file whenever a daemon is
  present.

**Deliberately not delivered, and still owned elsewhere:**

- A **plugin- or MCP-contributed command surface** stays deferred by
  ADR-0052 and is out of scope.
- **M007-H** (the daemon's missing command surfaces) and **M007-I** (the
  two subsystem moves, under accepted ADR-0058) are untouched.
- The **comment-budget re-banking** and the **writer-guard's writer-surface
  sweep** are recorded in §10 and belong to whichever milestone owns the
  guards.
