# AGENTS.md

## Quick start

Rust 1.81+, edition 2021.

```bash
scripts/verify.sh quick   # canonical sanity: fmt, agent schema, core-boundary, sandbox,
                          # execution-ownership, tui-authority guards, cargo check workspace
scripts/verify.sh full    # quick + clippy (-D warnings) + workspace tests +
                          # cargo test -p codegg --features server,plugins,lsp-test-support
cargo fmt                 # rustfmt: max_width 100, 4-space; non-Rust files use 2-space
```

Both verify modes cap resources (`CARGO_BUILD_JOBS=1`, `--test-threads=1`). `dbg!`/`println!`
are allowed in tests (`clippy.toml`).

## Layout

- Root crate `codegg` (`src/`): TUI, agent loop, tools, scheduler, server, auth.
  `src/lib.rs` re-exports `codegg_protocol as protocol` and providers as `codegg::provider`
  — there is no `src/protocol/` or `src/provider/` implementation directory.
- `crates/`: 9 workspace members — `codegg-core` (domain types: bus, jobs, session,
  storage, workspace; must stay UI/server/plugin/auth-free, enforced by
  `scripts/check-core-boundary.sh`), `codegg-config`, `codegg-protocol`,
  `codegg-providers`, `codegg-git` (typed git ops + risk), `egglsp` (authoritative LSP;
  `src/lsp/` is a thin shim), `egggit` (read-only git facts), `eggsentry` (security
  scanning), `eggcontext` (tokens). `crates/egglsp-test-server/` is NOT a member; it
  builds the `codegg-lsp-test-server` binary behind `lsp-test-support`.
- Aliases (`.cargo/config.toml`): `cargo ck` (workspace check),
  `ckroot ckcore ckprotocol ckconfig ckproviders ckgit cksplit`.
- Features: `server` (axum HTTP/WS), `plugins` (wasmtime), `image`,
  `lsp-test-support` (fake-LSP harness), `lsp-real-server-tests` (needs installed
  servers — never in default sweeps), `arboard` (default). Never `--all-features` for
  workspace sweeps; it drags in real-server tests. `verify.sh full` uses
  `--features server,plugins,lsp-test-support` instead.

## Testing

Prefer the narrowest target covering the change; run `verify.sh quick` first.

```bash
cargo test -p codegg-core                                            # single crate
cargo test --test tui_render                                         # single integration test
cargo test -p egglsp --features lsp-test-support --test scenario_engine  # LSP (needs feature)
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1  # capped full suite
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
fmt, clippy, workspace tests. Everything else is change-triggered (`ls scripts/check_*`):

- `codegg-core` or workspace deps → `bash scripts/check-core-boundary.sh`
- Process spawning / execution surfaces → `python3 scripts/check_execution_ownership.py`
  and keep `docs/execution-ownership.toml` in sync; scheduler changes also need
  `check_scheduler_bypass.py`; daemon path handling needs `check_daemon_cwd_usage.py`
  (no `std::env::current_dir()` in workspace-bound daemon code — thread `ExecutionContext`)
- `assets/agents/*.toml` or `assets/prompts/` → `python3 scripts/generate_builtin_agents.py`
  to regenerate `src/agent/builtins/generated.rs` (never edit it); `--check` in CI
- Git risk/policy → `check_git_forbidden_patterns.py`; storage layout →
  `check_project_catalog_invariants.py` (`STORAGE_LAYOUT_VERSION` must track the highest
  migration in `session/schema.rs`); projection transport → `check_projection_*.py` +
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
  guard with a stale-completion test (see `src/tui/async_cmd.rs`).
- Auth: `ExternalCommand` is unsupported; never log secrets; adding any config-defined
  provider disables all env-var auto-registration.
- New web-search providers belong in the external `eggsearch` project, not `src/search/`
  (legacy fallback). New deterministic validators go in the `eggsact` crate first. New
  LSP servers go in `crates/egglsp/src/server.rs` + config.

## Pointers

- `architecture/overview.md` is the module map; one doc per module under `architecture/`.
- `plans/registry.md` is the authoritative milestone/roadmap status — check it before
  assuming any roadmap state.
- `.opencode/skills/*/SKILL.md` are on-demand module guides (load via the skill tool).
  Canonical location is `.opencode/skills/`; `.skills` and `.agents/skills` are symlinks
  to it. When a module contract changes, update the skill and its `architecture/` doc together.
- `docs/`: `execution-ownership.md` (+ `.toml` manifest), `security-semantics.md`,
  `LSP.md`/`MCP.md`/`PLUGINS.md` (user integration notes; `architecture/` is authoritative),
  `TROUBLESHOOTING.md`, `dependency-maintenance.md`, `validation/` (historical closure records).

## Skills Index

| Skill | Covers | Primary doc |
|---|---|---|
| `architecture-review` | Verifying `architecture/` against code (counts, paths, batches for all 77 docs) | `architecture/overview.md` (Verified Counts) |
| `context` | Artifact storage, projection, `context_read`, packer, tool-palette policy, volatile-tail | `architecture/context-compaction-ownership.md` |
| `core` | Core facade, daemon families/lifecycle, transports, workspace registry | `architecture/core.md` |
| `git` | Typed ops + risk, guarded mutations/network/recovery, forbidden-pattern guard | `architecture/git.md` |
| `human-shell` | `!`/`!!` promotion model, safety policy, bounded output store | `architecture/human_shell.md` |
| `jobs` | Durable jobs/schedules/recovery/idempotency (`codegg-core`) | `architecture/jobs.md` |
| `planning` | `plans/` lifecycle: roadmaps, handoff plans, closure, registry, ADRs, archive | `plans/003-planning-process.md`, `plans/README.md` |
| `scheduler` | Admission control, fair queue, executors, `JobSubmissionService` | `architecture/scheduler.md` |
| `server` | Axum HTTP/WS server, routes, `/tui` protocol, auth/rate limits | `architecture/server.md` |
| `skills` | Skill discovery/precedence, portable schema, proposal/publication boundary | `architecture/skills.md` |
| `tool-program-harness` | Tool Program scenario/chaos/resource evaluation across harness modes | `architecture/tool_programs.md` |
| `tui` | TUI commands, sync dispatch, async spawn-and-complete, dialogs, project scope | `architecture/tui.md` |
| `upgrade` | Self-upgrade check (`codegg upgrade` is check-only; pin via `CODEGG_VERSION`) | `architecture/upgrade.md` |
| `util` | Clipboard, fuzzy, truncate, metrics, interner, pricing | `architecture/util.md` |

No skill exists yet for agent-loop, provider/auth, MCP/plugin, session/storage,
or bus/projection — use the `architecture/` doc directly for those.
