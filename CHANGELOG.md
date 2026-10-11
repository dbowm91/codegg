# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- New `documentation` skill (`.opencode/skills/documentation/SKILL.md`) covering
  the repo's doc surface: the authority order from source through `architecture/`
  to `README.md`, the verify-before-you-assert discipline, the untagged-enum
  config trap that silently discards a whole config file, and the content that is
  deliberately frozen (`docs/validation/`, `plans/closure/`, archived rounds) and
  must not be "refreshed". Registered in the `AGENTS.md` skills index.

### Fixed

- **Subcommand diagnostics were written to stdout, breaking machine-readable
  output.** `src/main.rs` installed `tracing_subscriber::fmt()` with no writer
  for ordinary subcommands, and `tracing-subscriber` defaults to stdout. A
  fresh machine therefore got 14–17 `WARN codegg_providers::provider_core: NO
  KEY for provider '...'` lines *before* the result, so the documented CI
  pipeline `codegg exec --format json --quiet | jq` failed with a parse error,
  and `codegg providers > file` produced a file polluted with ANSI-coloured
  warnings. The `core-stdio`/`acp` branch already used stderr for exactly this
  reason; the general branch now does too. `codegg providers` stdout is now
  just the provider list, and the `exec` JSON parses.
- **`codegg completions` help text contradicted its own behaviour.** The flag
  was documented as "default: current directory", but with no `--output` the
  script goes to stdout (`cmd_completions`, the `None` arm). `--output`
  additionally requires the directory to already exist and fails otherwise.
  Corrected the help string plus `docs/install.md` and `docs/cli.md`.
- **`docs/install.md` wrongly claimed `cargo install` installs only `codegg`.**
  `codegg` and `codegg-sandbox-helper` are both unconditional `[[bin]]`
  targets, so Cargo installs both — verified by running
  `cargo install --path . --root <tmp>`, which reported
  `Installed package codegg v0.1.0 (executables codegg, codegg-sandbox-helper)`.
  The substantive point stands: the pinned `codegg-eggsearch` sidecar is still
  absent, so a source install is not a managed bundle.
- **`--cwd` is inert but was documented as selecting the workspace.**
  `src/main.rs` parses `cwd: Option<PathBuf>` and never reads it; verified by
  `codegg --cwd /nonexistent-xyz-123 validate`, which succeeds with exit 0
  exactly as a plain `codegg validate` does. The README and `docs/cli.md` no
  longer advertise it as working.
- **The README quickstart's non-interactive examples failed as written.** With
  no `model` configured the agent falls back to the `EMERGENCY_DEFAULT_MODEL`
  of `openai/gpt-4o`, so `codegg --run "..."` and the `codegg exec` payload —
  which follow step 2's credential-only setup — died with
  `Provider not found: openai`. Both examples now pass a model, and step 2
  states the default outright. Verified: with `-m`/a `"model"` field both
  commands resolve the provider and reach the model call.
- Three source comments that contradicted their own code:
  `crates/codegg-config/src/schema.rs` documented
  `daemon.shutdown_timeout_ms` as defaulting to 5000 ms while both call sites
  use 10,000; and `src/plugin/install.rs` stated twice that
  `validate_local_install_source` accepts `..`, which it rejects by default.
- **Doc accuracy sweep against source.** Roughly forty corrections across
  `docs/` and `README.md`, each verified against source and, for CLI claims, by
  running the command. Notable removals: a fabricated
  `SandboxResult::Applied { profile: "workspace_rw" }` in
  `docs/execution-ownership.md` (the real capability is
  `isolation.landlock.workspace-rw.v1`), a non-existent `"Failed to launch
  language server"` error string in `docs/TROUBLESHOOTING.md`, the wrong
  session-database path, and `~/.config/codegg` paths that are wrong on macOS
  (`~/Library/Application Support`). `docs/security-semantics.md` described a
  3-step escalation ladder where the policy resolves in 7 ordered steps.
  `docs/MCP.md` described codegg as only an MCP client, when it also runs an
  MCP server. `docs/providers.md` listed `opencode_zen` as an env-var naming
  exception, when `OPENCODE_ZEN_API_KEY` already follows the convention.

- **`--cwd` was inert.** `src/main.rs` parsed `cwd: Option<PathBuf>` and never
  read it: `codegg --cwd /nonexistent-xyz-123 validate` exited 0 exactly as a
  plain run did. It now calls `std::env::set_current_dir` during CLI bootstrap —
  permitted explicitly by `scripts/check_daemon_cwd_usage.py` — before anything
  reads the filesystem, so config discovery and project selection both resolve
  against it. A nonexistent path now fails loudly. Verified byte-identical
  output between `codegg --cwd <dir> validate` and running from `<dir>`.
- **`config.skills` was dead config** — the same defect class as the
  `merge_configs` drop above. `enabled` and `paths` were parsed and merged but
  read by no production path, so `skills.paths` was a silent no-op. Both now
  work through a single `asset_discovery_config_from` helper that every
  `AssetRegistry` construction site calls:
  - `skills.paths` adds extra skills directories via a new `SourceKind::Configured`
    (rank 90). Each entry is a skills directory **itself**, deliberately unlike
    the global roots which are parent directories that get `<vendor>/skills`
    appended, so `"/opt/skills"` discovers `/opt/skills/<name>/SKILL.md`.
    Ranked below every built-in root so it can never shadow a standard skill
    name; collisions are still recorded as shadowed diagnostics.
  - `skills.enabled = false` clears `enabled_sources`, disabling discovery.
  - `SkillsConfig.urls` was **removed** rather than left accepted-but-inert:
    remote skill fetching is unimplemented, and an accepted-but-dead key is the
    defect being fixed.
  `ProjectAssetSnapshotBuilder::new` now takes only `Arc<Config>` and derives
  its discovery config instead of accepting an injected `SnapshotBuilderConfig`,
  so a key cannot be honoured in one construction path and ignored in another.
  Pinned by four new tests in `src/skills/registry.rs`.
- **`scripts/check_provider_connections_m4_coverage.sh` checked the wrong file.**
  It asserted `ConnectionRotateBegin` appears in `src/server/ws.rs`, but WS only
  maps a handful of JSON-RPC methods onto `CoreRequest`; the Connection
  lifecycle is a CoreDaemon concern dispatched in
  `src/core/daemon_providers.rs`. It now asserts all ten lifecycle handlers are
  implemented there *and* routed to the Providers family in
  `src/core/daemon_family.rs`, which is strictly stronger. Both halves are
  negative-tested.
- **`check_execution_ownership.py` had a dead path entry.** `CANONICAL_FINITE_PATHS`
  listed `src/scheduler/executors.rs`, but the scan loop skipped all of
  `src/scheduler/` before reaching it, so that entry never ran. The skip now
  exempts only the annotation inventory, leaving the boundary checks live for
  paths they are configured to police. Verified the guard still passes and that
  an injected violation in `executors.rs` is now caught.
- **`docs/agents-skills.md` documented the double-join failure as the real
  path.** It listed the Linux global root as `~/.config/codegg/codegg/skills`;
  the global root is the config directory and `<vendor>/skills` is appended
  once, so it is `~/.config/codegg/skills`.
- **`architecture/security.md`** described `Ambient` as "observation only, no
  auto-deny" and showed `prompt_hints = false` / `max_findings_in_prompt = 10`.
  `deny_critical_commands` defaults to true, so Ambient denies critical commands;
  the real defaults are `true` and `5`.

### Known issues (not fixed here)

- `scripts/check_provider_connections_m4_coverage.sh` is still not part of
  `verify.sh quick` or CI, so it only runs when invoked directly.

### Fixed

- **Config layer merge silently dropped 13 sections.** `merge_configs`
  (`crates/codegg-config/src/paths.rs:164`) is an explicit whitelist and is the
  only path from a parsed config file to `Config` — both `Config::load` and
  `ConfigWatcher::reload_config` call it — so a `Config` field with no merge arm
  was discarded for *every* layer, not only multi-layer loads. `provider_connections`,
  `approval_reviewer`, `daemon`, `scheduler`, `tool_deferral`, `security`,
  `research`, `tool_backends`, `human_shell`, `shell`, `deterministic_tools`,
  `preflight`, and `command_intent` had no arm, so those config sections parsed
  without error and then did nothing. The ten all-`Option` sections
  (`approval_reviewer`, `command_intent`, `daemon`, `human_shell`, `preflight`,
  `research`, `scheduler`, `shell`, `tool_backends`, `tool_deferral`) now merge
  field-by-field via a `merge()` impl each; `security`, `provider_connections`,
  and `deterministic_tools` replace wholesale, because their fields are all
  non-`Option` and serde bakes the default into the struct, so "unset" cannot be
  distinguished from "set to the default". **Behaviour change:** those thirteen
  sections now take effect, where previously they were inert.
- Added `scripts/check_config_merge_coverage.py`, wired into
  `scripts/verify.sh quick` and CI, to stop the defect class recurring. It
  derives the `Config` field set from the struct and the merged field set from
  `merge_configs` and fails when they differ; it embeds no field names, so a
  future covered field passes without editing it and an uncovered one fails.
- Three pre-existing verification defects surfaced while validating the above,
  all unrelated to config layering:
  - `installation_m002_qualification::prebuilt_docs_do_not_require_separate_eggsearch_install`
    anchored on `### Prebuilt installer` / `### From source` headings that the
    README restructure removed, so it failed on a missing section rather than on
    the contract. It now anchors on the same headings in `docs/install.md`,
    where the prebuilt path is actually documented, and additionally asserts the
    README routes readers there.
  - `tool_program_m014_daemon_recovery` resolved the daemon binary as
    `current_exe().parent()/codegg`, i.e. `target/<profile>/deps/codegg`, which
    never exists, and relied on `CARGO_BIN_EXE_codegg` being populated (it is
    not for this target/feature combination). A shared `daemon_binary()` helper
    now walks up from `deps/` to `target/<profile>/codegg`.
  - `clippy::nonminimal_bool` in `server`-gated `src/server/http.rs`. The
    routine clippy sweep runs without `--features server`, so the lint was never
    reached; it is now expressed with `Option::is_none_or`.
- Global skill discovery: five registry construction sites
  (`src/tui/app/mod.rs` ×2, `src/tool/skill.rs`, `src/tool/skill_proposal.rs`,
  `src/skills/compat.rs`) passed an already-joined `<config>/codegg/skills` path
  where `AssetRegistry::build` expects the *parent* directory, so the root
  resolved to `<config>/codegg/skills/codegg/skills`. That path never exists and
  `resolve_source_roots` skips missing roots without a diagnostic, so global
  skills were silently dropped everywhere except the daemon. `default_global_skills_root()`
  is replaced by `default_global_discovery_root()` (exactly `dirs::config_dir()`),
  all sites pass the parent, and two regression tests pin both halves of the
  contract.
- `codegg upgrade` now performs native verified replacement of the managed
  three-runfile bundle on supported Linux/macOS targets through Eggup. It
  preserves CodeGG's release/target policy and Eggfetch trust profile,
  verifies the archive checksum before strict extraction, validates all
  candidates, and commits with rollback/recovery receipts. Unsupported
  targets retain pinned fresh-install guidance; normal upgrade never fetches
  or executes `install.sh`. Eggup is pinned to immutable revision
  `66813b3b94de3a9b2f270e0000dc339ef6f0b478`.
- `upgrade()`: retire the network-fetched installer-script execution path (M005 hardening). `upgrade()` no longer spawns external `curl`, never fetches or executes a shell script, acquires no candidate bytes, and attempts no executable replacement; a valid newer tag now fails closed with manual fresh-install guidance (`CODEGG_VERSION=v{latest}` + `install.sh` URL) via the pure `describe_upgrade()` disposition. `check_for_updates()` (Eggfetch, 10s timeout, bounded redirects) and `installer_invocation()` fresh-install pin contract are unchanged.
- `upgrade()`: export the installer version pin as `CODEGG_VERSION` (the name `install.sh` honors) instead of `INSTALL_VERSION`, which the installer ignored — the pin was silently dropped and latest installed. Point the installer at the GitHub-hosted script (`raw.githubusercontent.com/dbowm91/codegg/main/install.sh`; there is no `codegg.ai` domain) and print the full `curl ... | sh` upgrade command. Pin construction lives in the pure `installer_invocation()` helper with a regression test (`tests/upgrade.rs`). `upgrade()` itself remains unwired from `codegg upgrade` (check-only CLI).

### Documented

- Skills and architecture third pass (`.opencode/skills/` + `architecture/` +
  `AGENTS.md`): re-verified all 23 module guides against source and corrected the
  claims that no longer held. The material ones — `architecture/mcp.md` showed a
  phantom `"mcp": {"servers": {...}}` nesting that parses as a single server named
  `servers` (corrected; the example now validates against the real parser);
  `provider-auth` wrongly claimed one config-defined provider suppresses env-var
  auto-registration for the rest; `human-shell` documented a `ShellOrigin::HumanPromoted`
  that is declared but never constructed and missed the `\!` escape hatch,
  `/shell-expand`, and the real warned-pattern set; `authorization` claimed CI
  coverage for three guards that are not wired; `architecture/approval_reviewer.md`
  carried a whole line-reference table shifted by +4; `bus-projection` still
  reported a projection guard failure that now passes; `projection.md` pointed at
  a test binary consolidated into `tests/session_family/`. `AGENTS.md`'s
  `verify.sh quick` guard list omitted eight guards it actually runs, and CI is a
  strict subset of `quick` — both now stated. Added the construction-site table to
  `architecture/skills.md`. `assets/agents/README.md` referenced
  `scripts/check_builtin_agents.py`, which does not exist; corrected to the real
  `generate_builtin_agents.py --check` and documented `runtime_kind`'s accepted
  values. `architecture-review`'s batch table was re-checked and does cover all 85
  docs. No production behavior changed.
- Removed `codeggers.example.jsonc`: it is an OpenCode config (foreign schema,
  `opencode_zen/*` models) left under the old plural branding, referenced by nothing,
  and it failed `codegg validate` with the same untagged-enum defect the canonical
  `codegg.example.jsonc` had. Fixed three broken path references found by a
  whole-surface link scan: `docs/cli.md` pointed at a nonexistent `docs/server.md`,
  `docs/dependency-maintenance.md` cited `src/memory/*.rs` instead of
  `crates/codegg-core/src/memory/*.rs`, and the `permission` skill pointed at a
  nonexistent `.opencode/skills/security-semantics/SKILL.md`. Historical
  cross-references inside `plans/closure/` and `plans/archive/` remain broken by
  design — they are immutable evidence and guessing a replacement would be worse.
- Skills refresh, second pass (`.opencode/skills/`): rewrote `architecture-review` (full 11-batch coverage for all 77 docs, counts defer to `overview.md` Verified Counts, added working-paper archival step); refreshed `skills` (10-variant precedence with Plugin rank 35, portable schema, `/reload` refresh lifecycle, proposal/publication boundary, `promotion.rs`/`publish.rs`); clarified `tool-program-harness` ACP placeholder vs the `codegg acp` frontend; documented the `upgrade()` installer-pin contract (also in `architecture/upgrade.md`); added a new `git` skill (ownership map, hard rules, forbidden-pattern guard). `AGENTS.md` pointers are now an index: Skills Index table plus `docs/` map. `architecture/skills.md` notes the canonical skill-guide location and its symlinks. No production behavior changed.
- Repository-surface housekeeping (M001): repaired `scripts/check_project_catalog_invariants.py` to assert the storage layout marker tracks the highest wired schema migration instead of pinning a volatile exact version; corrected stale storage-layout claims to reference `storage::STORAGE_LAYOUT_VERSION`; removed fragile TUI command-module and LSP server counts in favor of their owning registries; fixed `check-core-boundary.sh` invocations to use `bash`; corrected TUI source comments for multi-tab state and top-modal-only focus; reconciled README/AGENTS/skills/architecture with current source truth. No production behavior, schema, version, or release state changed.
- Skills refresh (`.opencode/skills/`): corrected stale claims in `context`, `core`, `jobs`, `server`, `skills`, `architecture-review`, `tool-program-harness`, and `shell_session` guides (full `CoreRuntimeDeps` field list, `NewJob` lineage fields, `TuiMessage` crate location, `run_server` daemon parameter, `ContextPolicyConfig` tool-palette fields, current storage layout reference). Added a new `tui` skill covering command registration, sync dispatch, the async spawn-and-complete guard pattern, and dialog invariants.
- `AGENTS.md`: fixed the `CoreRuntimeDeps` gotcha to list all 15 fields, documented both fake LSP server binaries (`egglsp-test-server` for egglsp crate tests, `codegg-lsp-test-server` for root-level tests), added a Skills Index mapping each skill to its architecture doc, and indexed the `docs/` directory.
- Moved the completed architecture-docs audit from `architecture/review-findings.md` to `plans/archive/architecture-doc-review-findings.md` with a closure note; updated `architecture/overview.md`.
- `architecture/core.md` and `architecture/workspace.md`: clarified that workspace tables arrived in migration v22; the current layout version is defined by `storage::STORAGE_LAYOUT_VERSION`.

### Added

- Phase 09 projection contract: `ProjectionResult` now carries `projection_id`, `source_spans`, `redaction_records`, and `rtk_metadata`. `ProjectionRecord` in run_store persists full projection metadata with promotion decisions. `evaluate_promotion()` provides budget/redaction/span-aware promotion. `preferred_projector_for_run_kind()` maps run kinds to optimal projectors. `PythonProjector` implements `CommandOutputProjector` for Python script output.

## Phase 13-17 Corrective Verification Pass (2026-06-27)

Docs/roadmap reconciliation plus test hardening for the Phase 13-17 surface. No new LSP protocol operations, no new workflow recipes, and no `workspace/applyEdit` / `workspace/executeCommand` execution were introduced. Plan: `plans/lsp_phase_13_17_corrective_verification_plan.md`. All eight workstreams meet final closure criteria.

### Added (52 new tests)

- `crates/egglsp/src/doctor.rs` (8): doctor scenarios — no service, outside root, unsupported language, no active server, active server with capabilities, observability snapshot, cache enabled/disabled, stale previews.
- `crates/egglsp/src/workflow_recipes.rs` (11): table-driven recipe coverage (all 12 named recipes), invalid-input rejection (missing path, max_depth=0, extreme line/column), no-auto-apply invariant (every composed workflow must NEVER include a preview id), tier-specific caps (Small/Workhorse/Frontier depth differences, security review forces Aggressive risk on every tier), sub-recipe provenance rendering.
- `crates/egglsp/src/context_policy.rs` (8): `LspContextDiagnostics` from empty/truncated/cache-hit packets; `StaleEvidencePolicy` and `LspUnavailablePolicy` behavior tests.
- `crates/egglsp/src/context_renderer.rs` (4): renderer feature-flag propagation (`include_cross_file` / `include_hierarchy` from policy); render-compact output stability.
- `src/tui/app/mod.rs` (6): dispatch tests for `/lsp-doctor` (missing-arg-shows-usage, with-path-produces-toast, without-tool-shows-unavailable) and `/lsp-context-diagnostics` (same three).
- `tests/lsp.rs` (15): tool-level integration tests covering Phase 13-15 surface end-to-end.

### Fixed

- `crates/egglsp/src/workflow_recipes.rs` line 923: `end: request.line + 20` → `end: request.line.saturating_add(20)`. Bare addition could overflow `u32` with extreme line numbers.
- `crates/egglsp/src/workflow_recipes.rs` line 1167: `end: request.line + 10` → `end: request.line.saturating_add(10)`. Same class of bug.

### Documented

- `architecture/lsp.md`: added "Phase 13-17 Corrective Verification Pass (2026-06-27)" section with closure summary table, new test counts, and bug fixes.
- `.opencode/skills/lsp/SKILL.md`: bumped version 1.8.0 → 1.9.0; added "Phase 13-17 Corrective Verification Pass" section near the bottom.
- `AGENTS.md`: added a one-line corrective verification note after Phase 17 describing the 52 new tests and two bug fixes.
- `README.md`: clarified Phase 16/17 are explicitly deferred.
- `plans/lsp_phase_13_17_roadmap.md`: marked Phase 13-17 as verified.

### Verified

- All 12 Phase 13-15 commands are registered and dispatched.
- `/lsp-doctor` is read-only and never starts servers.
- Workflow commands never auto-apply previews.
- `/lsp-context-diagnostics` is on-demand and does NOT bloat normal agent prompts.
- Disk cache remains memory-only (`LspCacheMode::Disabled | Memory` only).
- `/lsp-start` and `/lsp-replay-docs` are NOT registered.
- `mark_preview_applied` is only called after `PreviewApplyWriteReport.all_succeeded == true`.

## Phase 15: Renderer-Policy Unification and Context Diagnostics

- Fixed impact-analysis cap-note bug (inverted comparison emitted note when references were NOT capped)
- Extended `LspContextRenderConfig` and `RecipeSettings` with `include_cross_file` and `include_hierarchy` fields
- Added `LspContextDiagnostics` struct for structured context-shaping diagnostics
- Added `/lsp-context-diagnostics` TUI command
- Added behavior tests for all `StaleEvidencePolicy` and `LspUnavailablePolicy` variants
- Documented renderer/policy feature-flag ownership model

### Added

- Built-in Language Server Protocol (LSP) support with capability gating,
  preview-only semantic edits, semantic context packets, semantic check
  previews, and security/hunk context operations. Authoritative
  implementation in `crates/egglsp/`; 39 language server configurations
  available. Phase 6 added `/lsp-status` command, `counts_from_packet`
  flag for accurate status rendering, support-tier documentation, and
  troubleshooting guide.
- Native crate extraction: `egglsp`, `egggit`, `eggsentry`, `eggcontext`,
  `codegg-config`, `codegg-protocol`, `codegg-providers`, `codegg-core`
  (see `architecture/native_crates.md`).
- Typed `AuthConfig`, `AuthResolver`, and user-level encrypted credential
  store at `~/.config/codegg/credentials.json` with `codegg auth status |
  set-key | logout` CLI.
- Security review workflow (`/security-review`) with diff-based preset
  selection, evidence-based finding synthesis, opt-in LSP enrichment,
  opt-in `hunkSourceContext` evidence, structured `SecurityReviewReceipt`,
  result panel (`/security-review-show`), and cancellation
  (`/security-review-cancel`).
- Theme system with 50 bundled Halloy-format themes and live-preview
  picker; SQLite-persisted active theme.
- Long-horizon goal runtime with four-axis budget enforcement
  (turns, tokens, tool calls, wall-clock), durable wall-clock across
  session restarts, and `codegg goal` / `/goal` surfaces.
- Cache-aware context packing (observe-only layer), hardened gated
  context-policy layer (tool-palette reduction, base-derived, with
  backoff/starvation detection and Warn dry-run), and volatile-tail
  compaction for late-context token reduction of old tool results with
  recovery handles.
- Server mode (Axum) with HTTP REST, WebSocket TUI protocol, SSE event
  stream, session CRUD, and token-based auth (feature-gated).
- MCP (Model Context Protocol) client with local and remote transports,
  exponential-backoff reconnect, OAuth device-flow scaffolding, and DNS
  re-validation on each connect.
- WASM plugin system with hooks (feature-gated).
- TTS module (macOS `say`).
- Goal budget slash command (`/goal budget show|raise <axis> <n>`).
- TUI slash commands: `/help`, `/tree`, `/model`, `/agent`, `/new`,
  `/compact`, `/connect`, `/status`, `/context`, `/cost`, `/usage`,
  `/themes`, `/tui`, `/sessions`, `/goto`, `/share`, `/unshare`,
  `/timeline`, `/undo`, `/redo`, `/export`, `/import`, `/timestamps`,
  `/thinking`, `/models-refresh`, `/variants`, `/mcps`, `/fork`,
  `/worktree`, `/editor`, `/loop`, `/lsp-status`, `/lsp-servers`,
  `/lsp-capabilities`, `/lsp-errors`, `/lsp-root`, `/lsp-restart`,
  `/lsp-stop`, `/lsp-preview-apply`, `/tasks`, `/task-del`, `/memory`,
  `/memory-search`, `/memory-list`, `/memory-remember`,
  `/memory-forget`, `/memory-consolidate`, `/checkpoint`, `/goal`,
  `/plan`, `/state`, `/pr`, `/issue`, `/review`, `/diff`, `/tests`,
  `/revert`, `/research`, `/research-runs`, `/research-open`,
  `/research-show`, `/search`, `/doctor`, `/tool-backends`,
  `/security-review`, `/security-review-show`, `/security-review-cancel`,
  `/commit`, `/init`, `/skills`, plus `/exit` aliases.
  Correction (2026-10-10): skills activate through the model-facing `skill`
  tool; there is no `/skill:<name>` TUI command.
- Phase 9 LSP lifecycle commands: `/lsp-servers` (list active servers
  with root, state, generation, capabilities, and supported features),
  `/lsp-capabilities <key>` (effective capability snapshot for a server),
  `/lsp-errors <key>` (error history and health info),
  `/lsp-root <path>` (diagnose workspace root detection without starting
  servers), `/lsp-restart <key>` (manually restart a server),
  `/lsp-stop [key]` (stop all or a specific server). `/lsp-preview-apply`
  now applies patches directly with hash revalidation instead of
  read-only export. Lifecycle-state warnings in agent context (indexing,
  degraded, restarting, failed states produce explicit notes).

  **Deferred:** `/lsp-start` and `/lsp-replay-docs` commands deferred to
  a future phase. Per-key server stop uses `shutdown_all` fallback (stop
  per-key requires service API changes).

- LSP semantic memory cache (Phase 12): optional bounded in-memory cache
  for LSP-derived evidence packets. Disabled by default; opt-in via
  `[lsp_semantic_cache]` config (`mode = "memory"`, `max_entries = 64`,
  `max_bytes = 4194304`, `ttl_seconds = 300`). Cache keys encode workspace
  root, server ID, operation, request fingerprint, file content hashes,
  capability fingerprint, and budget fingerprint. Cache hits preserve or
  downgrade freshness correctly (e.g., `RetainedAfterRestart` after server
  generation change). `collect_context_cached()` wraps `collect_context()`
  with cache lookup/insert. TUI commands: `/lsp-cache-status`,
  `/lsp-cache-clear [--all|<root>]`. Never caches across workspace roots.
  Disk persistence explicitly deferred.

### Hardening (Phase 9–12 closeout)

- Phase 9 preview apply is gated by `egglsp::tui_summary::validate_preview_apply`,
  a testable boundary that performs all checks (not-found, stale-base,
  no-patches, already-applied, hash mismatch, patch failure) in memory and
  returns a typed `PreviewApplyPlan` without writing to disk. The TUI
  handler performs the actual `std::fs::write` calls and only calls
  `mark_preview_applied` after every write succeeds; failed writes leave
  the preview pending. Write-side hardening via
  `write_preview_apply_plan_atomically_enough()` performs per-file SHA-256
  recheck before each write; `PreviewApplyWriteReport` tracks per-file
  successes/failures; `mark_preview_applied` only called on full success;
  partial failures reported without marking applied. 10 new tests prove the
  write-side invariant.
- Phase 10 known notes-text bug: `crates/egglsp/src/evidence_collector.rs:1633`
  emits the `"references capped"` note when references are **not** capped
  (inverted comparison). Underlying reference count and budget enforcement
  are correct. Tracked as a follow-up.
- Phase 11 known limitation: `LspContextRenderConfig` does not currently
  expose `include_cross_file` / `include_hierarchy` fields, so
  `to_render_config()` does not propagate those policy flags. The
  `RecipeSettings` path (`to_recipe_settings()`) is unaffected.
- Phase 12 production wiring: `LspTool::lsp_context_for_agent_with_input`
  now routes through the cache when enabled, via the sync
  `LspSemanticCache::get` / `insert` API (rather than
  `collect_context_cached`) because the cache guard is `!Send` and cannot
  cross `.await`. Production cache keys now include request-scoped file
  hashes via `collect_cache_file_hashes_for_request()` in
  `src/tool/lsp.rs` (cap of 16 files with debug logging). When the primary
  file is unreadable, cache is bypassed for that request. Pattern: lock,
  lookup, drop lock, await `collect_context` on miss, lock again, insert.
  Unit tests cover `with_cache_config` propagation, `lsp_cache_status`
  reporting, and `clear_semantic_cache` zero-clear behavior in disabled
  mode. Cache eviction is conservative: generation mismatch, file hash
  change, TTL expiry, and capability fingerprint change all remove entries.
- Phase 9–12 safety sweep: all 3 static searches passed with 0 disallowed
  matches. `workspace/applyEdit` is rejected by the dispatcher.
  `workspace/executeCommand` is never invoked. `mark_applied` is only
  called after all writes succeed.

### Security

- SSRF protection with IPv6 ULA/multicast blocking (`fc00::/7`,
  `ff00::/8`) and DNS rebinding protection in MCP client.
- Symlink validation before canonicalization in `security/sandbox.rs`.
- `env_clear()` and minimal safe `PATH` for subprocess invocations.
- AES-256-GCM encryption with Argon2id key derivation for the credential
  store (`src/crypto/mod.rs`).
- Landlock filesystem sandboxing for the bash tool.
- Error redaction (`redact_local_paths()`) so internal paths never
  leak into LLM-facing error messages.
- `#![deny(unsafe_code)]` at the crate root.

## [0.1.0] - 2024-01-01

### Added

- Initial release
- Pure Rust implementation
- Multiple LLM provider support (Anthropic, OpenAI, Google, Azure,
  Bedrock, and more)
- Built-in Language Server Protocol (LSP) support
- WASM-based plugin system
- Terminal user interface (TUI) with syntax highlighting
- Server mode for headless HTTP access
- Persistent session management with SQLite
- Context compaction for long conversations
- Tool system with bash, read, edit, and task capabilities
- MCP (Model Context Protocol) client support
- Security features including SSRF protection and Landlock sandboxing
