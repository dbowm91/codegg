# AGENTS.md

## Quick start

Rust 1.89+, edition 2021.

```bash
scripts/verify.sh quick   # canonical sanity: fmt, agent schema, core/client/desktop
                          # boundaries, sandbox, execution-ownership, TUI authority,
                          # http-route-disposition, audit-coverage, scheduler-bypass,
                          # config-merge-coverage, provider wire/catalog/resilience +
                          # OpenAI endpoint, Eggwork target routing, cargo check workspace
scripts/verify.sh full    # quick + clippy (-D warnings) + workspace tests +
                          # cargo test -p codegg --features server,plugins,lsp-test-support
cargo fmt                 # rustfmt: max_width 100, 4-space; non-Rust files use 2-space
```

Both verify modes cap build jobs (`CARGO_BUILD_JOBS=2`); broad test execution
runs under nextest profile `ci` (one process per test — env mutation in
tests is process-global — up to 4 concurrent). `dbg!`/`println!`
are allowed in tests (`clippy.toml`).

## Layout

- Root crate `codegg` (`src/`): TUI, agent loop, tools, scheduler, server, auth.
  `src/lib.rs` re-exports `codegg_protocol as protocol`, `codegg_providers as provider`,
  and `codegg_config as config` — there is no `src/protocol/`, `src/provider/`,
  or `src/config/` implementation directory.
- `crates/`: 11 crates (+ root = 12 workspace members in `Cargo.toml`) — `codegg-core` (domain types: bus, jobs, session,
  storage, workspace; must stay UI/server/plugin/auth-free, enforced by
  `scripts/check-core-boundary.sh`), `codegg-config`, `codegg-protocol`,
  `codegg-providers`, `codegg-git` (typed git ops + risk), `codegg-document`
  (text transaction core), `egglsp` (authoritative LSP;
  `src/lsp/` is a thin shim), `egggit` (read-only git facts), `eggsentry` (security
  scanning), `eggcontext` (tokens), `codegg-client` (frontend-side native client).
  `crates/egglsp-test-server/` has no `Cargo.toml` and is NOT a member; the root
  package builds it as the `codegg-lsp-test-server` bin target behind
  `lsp-test-support`. The `egglsp` package separately builds an
  `egglsp-test-server` bin from `crates/egglsp/src/bin/` — two distinct binaries.
- Workspace ownership: root `Cargo.toml` `[workspace.package]`/`[workspace.dependencies]`
  own shared versions/default policy; members use `*.workspace = true` plus only their
  local features (minimal baseline — member manifests add only what they need;
  `serde` keeps no workspace features). Single-consumer deps stay local —
  including the optional `sdm-core`/`sdm-runtime` git deps, which only the root
  package uses behind `decision-runtime-sdm`.
  `[workspace.lints.rust] unsafe_code = "deny"` is inherited via `[lints] workspace = true`
  (e.g. `codegg-core`); the root package intentionally does NOT inherit it because
  `src/bin/codegg-sandbox-helper.rs` has deliberate reviewed `unsafe` — the lib
  enforces `#![deny(unsafe_code)]` in `src/lib.rs` instead.
- Aliases (`.cargo/config.toml`): `cargo ck` (workspace check),
  `ckroot ckcore ckprotocol ckconfig ckproviders ckgit cksplit`.
- Features: `server` (axum HTTP/WS), `plugins` (wasmtime), `image`,
  `debug-logging` (tracing-based, no file output by default),
  `decision-runtime-sdm` (optional local SDM decision runtime, gates
  `src/decision_sdm.rs`; default builds exclude it),
  `lsp-test-support` (fake-LSP harness), `lsp-real-server-tests` (needs installed
  servers — never in default sweeps), `arboard` (default). `codegg-providers`
  additionally has a dev-only `capture-test-support` feature (the OpenCode Go
  origin override for the capture trajectory) enabled through a
  `[dev-dependencies]` entry, so it is absent from `cargo build` and from the
  shipped binary. Never `--all-features` for
  workspace sweeps; it drags in real-server tests. `verify.sh full` uses
  `--features server,plugins,lsp-test-support` instead.

## Testing

Prefer the narrowest target covering the change; run `verify.sh quick` first.

```bash
cargo test -p codegg-core                                            # single crate
cargo test --test tui_render                                         # single integration test
cargo test -p egglsp --features lsp-test-support --test scenario_engine  # LSP (needs feature)
cargo nextest run --workspace --locked --profile ci  # capped full suite (needs cargo-nextest)
```

- New `#[tokio::test]`s default to `current_thread`; use
  `flavor = "multi_thread", worker_threads = 2` only for real concurrency/subprocesses.
- Storage tests: `isolated_pool()` (migrations run inside; never add extra `migrate()`).
- Test profile strips debuginfo; restore backtraces with `RUSTFLAGS=-C debuginfo=2 cargo test …`.
- Plugin SDKs test separately: `examples/plugins/sdk-rust` (cargo),
  `examples/plugins/sdk-python` (`python3 -m unittest discover`).
- Full taxonomy, pool strategy, nextest profiles: `architecture/testing.md`.

## Change-triggered guards

`verify.sh quick` runs the routine subset. CI (`.github/workflows/ci.yml`) is one bounded
`verify` job: agent schema, core-boundary, sandbox, execution-ownership, tui-authority,
http-route-disposition, audit-coverage, scheduler-bypass, config-merge-coverage, fmt,
clippy, workspace tests.
CI is a strict **subset** of `quick` — it omits the client/desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint, and Eggwork routing guards, so a
green CI run does not imply a green `quick`. Everything outside both is
change-triggered (`ls scripts/check_*` for the full list):

- `codegg-core` or workspace deps → `bash scripts/check-core-boundary.sh`
- Process spawning / execution surfaces → `python3 scripts/check_execution_ownership.py`
  and keep `docs/execution-ownership.toml` in sync; scheduler changes also need
  `check_scheduler_bypass.py`; daemon path handling needs `check_daemon_cwd_usage.py`
  (no `std::env::current_dir()` in workspace-bound daemon code — thread `ExecutionContext`)
- `assets/agents/*.toml` or `assets/prompts/` → `python3 scripts/generate_builtin_agents.py`
  to regenerate `src/agent/builtins/generated.rs` (never edit it); `--check` in CI
- Git risk/policy → `check_git_forbidden_patterns.py`; storage layout →
  `check_project_catalog_invariants.py` (`STORAGE_LAYOUT_VERSION` must track the
  highest migration in `crates/codegg-core/src/session/schema.rs`); projection
  transport → `check_projection_*.py` +
  `check_websocket_bounds.py`; provider lifecycle → `check_provider_connections_*.sh`

## Gotchas

- `PermissionRegistry`/`QuestionRegistry` are sync (`register`/`respond`/`answer_question`
  are `fn`, not `async`); register the responder BEFORE publishing the Pending event.
- `ToolBroker` is the only production tool-call boundary. Heavy work (tests, managed
  processes, subagent dispatch, tool programs) goes `JobSubmissionService` →
  `JobScheduler`, never direct executor calls.
- `egggit` never mutates; mutations live in `src/git_mutations.rs` (+ network/config
  policy in `src/git_network_policy.rs`).
- Daemon is a user-scoped singleton (`flock` on `daemon.lock`; `CODEGG_DAEMON_HOME`
  overrides). Plain `codegg` connects-or-starts (`src/core/instance.rs`); `--standalone`
  is in-process core; the `server` requires `--standalone-core`.
- Command intent defaults to `Observe` (classify only); kill switch
  `CODEGG_ROUTING_DISABLE=1`.
- Human `!cmd` is hidden from the model; `!!cmd` promotes (bounded/redacted) output.
- Slow TUI handlers use `spawn_tui_task` + `finish(request_id)`/`fail(request_id, err)`
  guard with a stale-completion test (see `src/tui/async_cmd.rs`). Note
  `dispatch_tui_command` itself is `pub(crate) async` with five `.await` points in
  the durable-editor arms — dispatch is not purely synchronous
  (`src/tui/runtime/command_dispatch.rs:84`).
- Auth: `ExternalCommand` is unsupported; never log secrets. The config-first
  registration path explicitly registers all 17 built-ins, each resolving its
  own config then its conventional env var, so defining a provider in config
  does NOT disable env-var auto-registration for the others. The
  `if registry.list().is_empty() { register_builtin(registry) }` fallback at
  `crates/codegg-providers/src/provider_core.rs:1088` is a redundant safety
  net that only fires when config-based registration produced zero results
  (`architecture/provider.md:81-85`).
- `AssetRegistry::build` takes each global root as the *parent* dir and appends
  `<vendor>/skills` (`src/skills/registry.rs:250-303`). Pass
  `default_global_discovery_root()` — exactly `dirs::config_dir()` — never an
  already-joined `…/codegg/skills` path, which double-joins to
  `…/codegg/skills/codegg/skills`. Because `resolve_source_roots` skips a
  missing root with no diagnostic, that mistake silently drops every global
  skill. Six sites build their own registry; all are listed in
  `architecture/skills.md`.
- New web-search providers belong in the external `eggsearch` project, not `src/search/`
  (legacy fallback). New deterministic validators go in the `eggsact` crate first. New
  LSP servers go in `crates/egglsp/src/server.rs` + config.
- `codegg upgrade` uses CodeGG-owned release/target policy plus Eggup's
  immutable-pinned core/acquisition crates. Keep its existing Eggfetch trust and
  redirect profile; do not add `eggup-eggfetch` or move archive policy into Eggup.
  When changing the upgrade contract, update `architecture/upgrade.md` and
  `.opencode/skills/upgrade/SKILL.md` together.
- Semantic model routing is opt-in through exact `virtual:<name>` aliases only; concrete
  models bypass it. It never changes durable session/provider-connection selection, only
  picks a compatible model through the already-selected connection. No local affinity
  cache (`sticky`/`affinity_ttl_s` are shared policy fields only). See README +
  `architecture/config.md` for the full contract.

## Pointers

- `architecture/overview.md` is the module map; one doc per module under `architecture/`.
  Its `## Verified counts` table is the single source of truth for counts — verify
  each against the listed `Source` rather than copying a number between docs.
- `plans/registry.md` is the authoritative milestone/roadmap status — check it before
  assuming any roadmap state. `plans/README.md` defines the planning hierarchy and
  status vocabulary; the `planning` skill is the operational guide.
- `README.md` is the user-facing entry point (quickstart) and indexes `docs/`.
  `docs/` is user-facing guidance; `architecture/` is authoritative where the two differ.

### docs/ index

| Scope | Files |
|---|---|
| User guides (linked from the README) | `install.md`, `cli.md`, `configuration.md`, `providers.md`, `daemon.md`, `tui.md`, `agents-skills.md`, `tools.md` |
| Integration notes | `LSP.md`, `MCP.md`, `PLUGINS.md`, `themes.md`, `playwright.md` |
| Reference | `security-semantics.md`, `TROUBLESHOOTING.md`, `execution-ownership.md` (+ `execution-ownership.toml` manifest), `dependency-maintenance.md` |
| Historical evidence | `validation/` (closure records; line numbers are pinned to old SHAs by design — do not "refresh" them) |

`codegg.example.jsonc` is the annotated config example; validate it with
`codegg validate --config codegg.example.jsonc`. Several config keys are untagged
enums whose wrong shape silently discards the whole file, so keep the example valid.

### Skills index

`.opencode/skills/*/SKILL.md` are on-demand module guides (load via the skill
tool). Canonical location is `.opencode/skills/`; `.skills` and
`.agents/skills` are symlinks to it. Load the skill for the module you are
changing; each names its authoritative `architecture/` doc in its intro and
`## See Also`. When a module contract changes, update the skill and its
`architecture/` doc together.

| Skill | Read with it |
|---|---|
| `agent` | `architecture/agent.md` |
| `authorization` | `architecture/authorization.md`, `architecture/audit.md` |
| `bus-projection` | `architecture/bus.md`, `architecture/projection.md` |
| `config` | `architecture/config.md` |
| `context` | `architecture/context-ledger.md`, `architecture/compaction.md` |
| `core` | `architecture/core.md` |
| `documentation` | `architecture/overview.md`, `docs/` (doc-surface maintenance) |
| `git` | `architecture/git.md` |
| `human-shell` | `architecture/human_shell.md` |
| `jobs` | `architecture/jobs.md` |
| `mcp-plugin` | `architecture/mcp.md`, `architecture/plugin.md` |
| `permission` | `architecture/permission.md`, `architecture/approval_reviewer.md` |
| `planning` | `architecture/work_plan.md`, `plans/` |
| `provider-auth` | `architecture/provider.md`, `architecture/auth.md` |
| `scheduler` | `architecture/scheduler.md`, `architecture/jobs.md` |
| `server` | `architecture/server.md`, `architecture/client.md` |
| `session-storage` | `architecture/session.md`, `architecture/storage.md` |
| `skills` | `architecture/skills.md` |
| `testing-ci` | `architecture/testing.md`, `scripts/verify.sh` |
| `tool-execution` | `architecture/agent.md`, `architecture/jobs.md`, `architecture/scheduler.md` |
| `security-hardening` | `architecture/security.md`, `architecture/permission.md`, `architecture/authorization.md` |
| `tool-program-harness` | `architecture/tool_programs.md` |
| `tui` | `architecture/tui.md` |
| `upgrade` | `architecture/upgrade.md` |
| `util` | `architecture/util.md` |
| `architecture-review` | `architecture/overview.md` (how to verify these docs) |
