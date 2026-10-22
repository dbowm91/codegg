# AGENTS.md

## Quick start

Rust 1.89+, edition 2021.

```bash
scripts/verify.sh quick   # canonical sanity: fmt, agent schema, core/client/desktop
                          # boundaries, sandbox, execution-ownership, TUI authority,
                          # http-route-disposition, audit-coverage,
                          # queue-delivery-callsites, queue-write-transaction-shape,
                          # scheduler-bypass,
                          # config-merge-coverage, no-hardcoded-models, provider
                          # wire/catalog/resilience +
                          # OpenAI endpoint, Eggwork target routing,
                          # cargo-profile-alignment, test-ownership,
                          # test-target-references, cargo check workspace
scripts/verify.sh full    # quick + clippy (-D warnings) + workspace tests +
                          # cargo test -p codegg --features server,plugins,lsp-test-support
scripts/verify.sh scoped  # quick's guards + only the tests the change can affect
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
                          # give the sweep a private cache root so its result is a
                          # property of the product, not of the host:
                          #   XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci
scripts/verify.sh scoped [base-ref]  # quick's guards + only the tests the change can affect
```

- New `#[tokio::test]`s default to `current_thread`; use
  `flavor = "multi_thread", worker_threads = 2` only for real concurrency/subprocesses.
- Storage tests: `isolated_pool()` (migrations run inside; never add extra `migrate()`).
- Test profile strips debuginfo; restore backtraces with
  `cargo test --config 'profile.dev.strip="none"' …`. **`RUSTFLAGS=-C debuginfo=2`
  does not work** — the profile's `strip` still applies, so the artifact comes
  out byte-identical with no DWARF sections.
- The root package's integration tests are **families** (one executable per
  subject area, under `tests/<family>/`). `cargo test --test <member>` no
  longer selects a single member; use `cargo test --test <family> -- <member>`.
- `scripts/verify.sh scoped` selects tests from `docs/test-ownership.toml`. A
  change in a crate **another crate depends on** widens to every transitive
  dependent's full surface — the root package depends on all eleven other
  crates and re-exports several, so the coupling runs through the root's own
  source and no path rule can see it.
- Plugin SDKs test separately: `examples/plugins/sdk-rust` (cargo),
  `examples/plugins/sdk-python` (`python3 -m unittest discover`).
- Full taxonomy, pool strategy, nextest profiles: `architecture/testing.md`.

## Change-triggered guards

`verify.sh quick` runs the routine subset. CI (`.github/workflows/ci.yml`) is one bounded
`verify` job: agent schema, core-boundary, sandbox, execution-ownership, tui-authority,
http-route-disposition, audit-coverage, queue-delivery-callsites,
queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, fmt,
clippy, workspace tests.
CI is a strict **subset** of `quick` — it omits the client/desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint, and Eggwork routing guards, so a
green CI run does not imply a green `quick`. Everything outside both is
change-triggered (`ls scripts/check_*` for the full list):

- `codegg-core` or workspace deps → `bash scripts/check-core-boundary.sh`
- Daemon-internal admission surfaces → `python3 scripts/check_queue_delivery_callsites.py`
  (M003 queue delivery, ADR-0038 — **three** permitted callers: the post-terminal
  wake, the bounded restart twin, and M007's bounded advance tick, which ADR-0039
  accepted; the guard also **fails when none is wired**, and `--self-test` injects a
  **fourth** caller to prove it is not vacuous).
  Its sibling `check_idle_continuation_callsites.py` covers ADR-0031's delegated
  entry. Both live in `scripts/verify.sh quick` and in CI. Widening either guard's
  caller set is a recorded decision, never a quiet extra call site.
- Queue **store** transaction shape → `python3 scripts/check_queue_write_transaction_shape.py`
  (C002). A queue write path that reads before it writes must open its transaction
  with `begin_immediate` (`BEGIN IMMEDIATE`), not `pool.begin()`. A deferred
  transaction takes no lock, so the first `SELECT` holds a shared read lock and the
  first `UPDATE` has to upgrade it — and SQLite refuses that upgrade **without
  waiting**, so two such callers can both fail with `SQLITE_BUSY` instead of one
  winning and the other waiting out `busy_timeout`. Measured at 204 of 256
  concurrent claim rounds before C002. `--self-test` injects a deferred
  read-then-write path and a closing rollback to prove the guard rejects both. In
  `scripts/verify.sh quick` and CI.
- Process spawning / execution surfaces → `python3 scripts/check_execution_ownership.py`
  and keep `docs/execution-ownership.toml` in sync; scheduler changes also need
  `check_scheduler_bypass.py`; daemon path handling needs `check_daemon_cwd_usage.py`
  (no `std::env::current_dir()` in workspace-bound daemon code — thread `ExecutionContext`)
- `assets/agents/*.toml` or `assets/prompts/` → `python3 scripts/generate_builtin_agents.py`
  to regenerate `src/agent/builtins/generated.rs` (never edit it); `--check` in CI.
  That generator owns `src/agent/builtins/mod.rs` **outright** — it rewrites the
  whole file. `assets/subagent-names/*.toml` is separate: regenerate it with
  `python3 scripts/generate_subagent_names.py` into
  `src/agent/subagent_names/generated.rs`, whose sibling `mod.rs` is hand-written
  and must not be handed to the builtins generator.
- Git risk/policy → `check_git_forbidden_patterns.py`; storage layout →
  `check_project_catalog_invariants.py` (`STORAGE_LAYOUT_VERSION` must track the
  highest migration in `crates/codegg-core/src/session/schema.rs`); adding or
  changing any model list → `check_no_hardcoded_models.py` (CodeGG ships **no**
  compiled-in model catalog; discovery is canonical, config models are
  additive — see `architecture/provider.md` "Model discovery authority");
  projection
  transport → `check_projection_*.py` +
  `check_websocket_bounds.py`; provider lifecycle → `check_provider_connections_*.sh`
- Test bodies → `python3 scripts/check_test_assertion_quality.py` (every body must
  assert a checkable property, delegate to a named helper that does, or be recorded
  in `docs/test-assertion-quality.toml` with the reason it asserts nothing) and
  `python3 scripts/check_test_duplication.py` (no two bodies may assert the same
  thing twice unless `docs/test-duplication.toml` records the survivor). Both
  are in `scripts/verify.sh quick` and in CI. Both are built on
  `scripts/rust_test_scan.py`, the shared Rust-aware scanner — **never** scan
  Rust with a regex: a regex scan misreads lifetimes as character literals and
  undercounts `tests/` by 76%. `rust_test_scan.py --validate` is the ground
  truth; `check_test_assertion_quality.py --self-test` and
  `check_test_duplication.py --self-test` prove each guard is not vacuous.
- Tool Program parse-only invariant → `python3 scripts/check_tool_program_parse_only.py`
  (rejects a `pyo3` dependency, a `pyo3` import, a process spawn, or a Python FFI
  link in `crates/codegg-core/src/tool_program/`). The three test functions that
  used to "enforce" it could not fail; the guard replaced them.

## Gotchas

- A **relative** path in any tool is relative to the session's `workspace_root`,
  never to the process cwd. `validate_path`/`validate_target_path` join the
  `allowed_root` first; `TestTool`/`PythonScriptTool` default `workdir` to it
  and refuse rather than guess when there is none. This matters because turns
  run in a user-scoped **daemon** whose cwd is wherever it was launched — so
  resolving a relative path against it silently retargeted a whole session into
  an unrelated tree. It stayed invisible while you worked in whichever repo the
  daemon happened to start from. Two tell-tales of this class of bug: a tool
  reporting a file missing that its own hint then suggests, and tests/scripts
  executing "in the wrong workspace". **Walk the path you validated** — `glob`
  and `list` both validated a root and then walked the raw request, the same bug
  wearing a different hat.
- **A missing workspace root is a refusal, never a cue to read the process cwd.**
  Every scheduler executor binds `JobExecutionContext.workspace_root` (whose own field docs say
  "executors must use this instead of consulting process-global CWD"), and
  `ToolRegistry::with_options` registers an `UnrootedTool` for each workspace tool when
  `workspace_root: None` rather than calling `X::default()`. The `Default` constructors themselves
  now seed the sentinel `util::unresolved_workspace_root()`, which `validate_path`/
  `validate_target_path` reject with `ToolError::MissingWorkspaceRoot` — so a tool built without
  a root fails typed instead of silently reading an unrelated tree. `SubagentJobExecutor` was
  the last executor deriving its root from the payload's `allowed_paths`, so a legitimate job
  with an empty list was refused while the daemon held the right root.
  `check_daemon_cwd_usage.py` enforces this, has **no** line-pattern allowlist (its predecessor
  matched `allowed_root: std::env::current_dir` and so passed with every violation present), and
  carries `--self-test`; if you change it, prove it fails on an injected violation before citing
  it as evidence.
- `PermissionRegistry`/`QuestionRegistry` are sync (`register`/`respond`/`answer_question`
  are `fn`, not `async`); register the responder BEFORE publishing the Pending event.
- One approval on screen, the rest in `DialogState::permission_queue`. `show_permission_dialog`
  used to assign straight into `permission_dialog`, so each new `PermissionPending` destroyed
  the previous request **together with its `perm_id`** — the daemon waiter stayed parked on a
  human answer no UI pointed at, and the tool timed out denied for an invisible reason. Related:
  `close_dialog` needs its `Dialog::Permission` arm calling `requeue_open_permission()`, because
  anything still mounted at close time was dismissed *unanswered*; every answer path must
  therefore clear the dialog fields **before** calling `close_dialog()` or it requeues what it
  just answered. `CoreEvent::PermissionPending` carries no `args`, so `mount_permission_dialog`
  recovers them from the transcript — approving a command you cannot read is not a decision.
- An advisory must never create an escalation on its own: gate it on a **finding**
  (`preflight_requires_escalation`), never on having run. Command preflight is wired by
  default, so testing `CommandPreflightResult::is_some()` escalated every `bash` call — and
  because that arm is reached from `PermissionResult::Allow` as well as `Ask`, it silently
  discarded stored `AlwaysAllow` grants. That is the "the agent overrode my approval"
  symptom; see `architecture/permission.md` "Command preflight is advisory".
- Remembered bash approvals are **per segment**: `decision_scopes_for_shell_command` splits a
  compound command on control operators outside quotes and combines the segments with
  **intersection** semantics (all must Allow; any Deny denies). Anything whose interior the
  splitter cannot see — `$( )`, `${ }`, heredoc, unbalanced quotes — stays one
  `cmd:exact:` scope. Never resolve the intersection to "any segment is enough".
- `BashTool` has two independent roots: `workspace_root` (default `workdir`, command-intent
  context, every session) and `child_workspace_ceiling` (the isolated child-Git worktree
  ceiling, armed only by `child_git_policy`). Never derive the ceiling from `workspace_root` —
  it rejects *any* absolute path outside its root, including `/dev/null`.
- Never route a TUI answer through `AppMode` alone: `RemoteCore` covers both the
  WS-attach frontend (owns `remote_send_tx`) and the socket-daemon frontend (owns
  `core_client` only). Use `App::is_ws_attach_transport()` — enforced by
  `scripts/check_tui_transport_authority.py`. A mode-only gate drops the answer
  into an absent channel, orphaning the daemon waiter on an unbounded human wait,
  so approving a tool hangs the turn with no visible error.
- Progress recovery keys every incident on the whole `action_key()` (tool **plus** normalized
  arguments), `EquivalentError` included. Two tool-name-only arms got this wrong and each killed
  ordinary turns. `ShortCycle` compared `canonical_tool` alone, so it fired exactly when a model
  reached for the same tool again with *different* arguments — reading a second file, running a
  second grep — and climbed the whole ladder. `EquivalentError` compared tool plus `error_class`,
  so a model bisecting a build failure ran five different commands, saw the same `tool_error` class
  five times, and was stalled for work it had never repeated. Both are gone; do not reintroduce a
  tool-name-only check, and do not let error *wording* into a key either — the arguments already
  identify the work. A successful read-only call the turn has never made is `NewEvidence`: it
  cannot be a repeat, so it must never be escalated. Ladder state belongs to one incident and
  resets when `action_key()` changes, so a model that fixed its problem mid-sequence is not
  terminated for the failures that preceded the fix. A successful mutating or process-class call
  carries **no** progress signal on its own — `state_changed` needs a drained file-change bus
  event, which nothing emits for process execution; do not widen it to "the call succeeded" or the
  ladder goes blind to a loop. The outer run bound is `ExecutionLimits::max_turns`, not the
  recovery ladder. A `ProgressSignal` variant with no producer is a claim, not a hook: `ChildAdvanced`
  sat hardcoded `false` at the only construction site until it was deleted.
- The **working strip row 0 is reserved unconditionally** and is the main buffer's only
  status surface. Its height is subtracted from the transcript (`session_layout` splits
  `prompt_height` against `Min(10)`), so gating it on turn state reflowed the transcript
  on every submit and completion. It reads `idle` when nothing runs. Put turn state here,
  not on the hint line: there is one cadence (`tui::components::spinner`, one frame table,
  100ms) and one `ActivityPhase` enum — a second spinner or a free-form label is how the
  indicator a user watches became ten times slower than the one nobody looked at. Turn
  failures are **latched** in `SessionState::last_error` and cleared only by `TurnStarted`;
  `report_turn_error` is for turn failures alone, not for shell/clipboard/theme feedback.
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
- Human `!cmd` stays out of model context; `!!cmd` and `/shell-include` require
  daemon acknowledgement for bounded/redacted, session-owned staging before
  the next turn can consume them. Turn admission atomically commits promotion
  consumption, the controller lease, and a receipt keyed by envelope
  `request_id`; retry the identical request with that ID. Startup marks any
  nonterminal accepted receipt interrupted and never replays it. `/shell-ask`
  stages its question/evidence before submitting through the bound session.
  Staging expires after 24 hours;
  PTY bytes remain private local output. Remote prompt-only transport keeps an
  explicitly unsubmitted composer draft.
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
- New web-search providers belong in the external `eggsearch` project; CodeGG's
  retired `src/search/` provider tree must not be recreated. New deterministic validators go in the `eggsact` crate first. New
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
| `tool-program-harness` | `architecture/tool_programs.md` |
| `testing-ci` | `scripts/verify.sh`, `architecture/testing.md` |
| `tool-execution` | `architecture/agent.md`, `architecture/jobs.md`, `architecture/scheduler.md` |
| `security-hardening` | `architecture/security.md`, `architecture/permission.md`, `architecture/approval_reviewer.md` |
| `lsp-ide` | `architecture/lsp.md`, `architecture/tui.md`, `architecture/client.md` |
| `tui` | `architecture/tui.md` |
| `upgrade` | `architecture/upgrade.md` |
| `util` | `architecture/util.md` |
| `architecture-review` | `architecture/overview.md` (how to verify these docs) |
