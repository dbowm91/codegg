# AGENTS.md

## Quick start

Rust 1.89+, edition 2021.

```bash
scripts/verify.sh quick   # canonical sanity: rustfmt, the generated-asset
                          # checks, 70 guard scripts, and a workspace check
scripts/verify.sh full    # quick + clippy (-D warnings) + workspace tests +
                          # cargo test -p codegg --features server,plugins,lsp-test-support
scripts/verify.sh scoped  # quick's guards + only the tests the change can affect
cargo fmt                 # rustfmt: max_width 100, 4-space; non-Rust files use 2-space
```

The guard list is not written out here: it lives in `scripts/verify.sh`
(`run_guards()`), so it cannot drift from what actually runs. Read that
function, or `ls scripts/check_*`, for the current set.

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
  servers — never in default sweeps), `arboard` (default). Two more are
  test-only compile gates that keep a capability out of shipped builds:
  `windows-sandbox-test-support` and `linux-git-test-support` (the latter
  pulls `egggit/git-live-read-test-support`). `codegg-providers`
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
cargo test --test tui_render -- tui_render                           # one member of one family
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
  The `--test` name is the **family** (the Cargo target, declared in the root
  `Cargo.toml`), and the trailing filter is the member. A member name can
  coincide with a family name — `tui_render` is both — so `--test tui_render`
  alone runs the whole 41-member family, and `--test tui_render -- tui_render`
  runs just that member. A test id is `<family>::<member>::<test>`.
- `scripts/verify.sh scoped` selects tests from `docs/test-ownership.toml`. A
  change in a crate **another crate depends on** widens to every transitive
  dependent's full surface — the root package depends on all eleven other
  crates and re-exports several, so the coupling runs through the root's own
  source and no path rule can see it.
- Plugin SDKs test separately: `examples/plugins/sdk-rust` (cargo),
  `examples/plugins/sdk-python` (`python3 -m unittest discover`).
- Full taxonomy, pool strategy, nextest profiles: `architecture/testing.md`.

## Change-triggered guards

`verify.sh quick` runs the routine subset: rustfmt, the two generated-asset
checks, 70 guard scripts, and a workspace `cargo check`. CI
(`.github/workflows/ci.yml`) is one bounded `verify` job: the same generated-asset
checks, 56 guard scripts, fmt, clippy, and the workspace test sweep.

CI is a strict **subset** of `quick` — it runs nothing that quick does not.
As measured on 2026-10-24, quick runs **14 guards CI omits**: the client and
desktop boundaries, `check_daemon_listing_authority`, `check_first_party_skills`,
`check_network_intent_owner_wiring`, `check_no_hardcoded_selection_colours`,
`check_eggwork_target_routing`, `check_openai_endpoint_composition`, and the
six provider guards (`check_provider_catalog_consistency`,
`check_provider_multi_surface_dispatch`, `check_provider_qualification`,
`check_provider_resilience_ownership`, `check_provider_wire_boundary`,
`check_provider_wire_cutover`). So a green CI run does **not** imply a green
`quick`. Re-derive the difference instead of trusting this list:

```bash
diff <(awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' | sort -u) \
     <(grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' .github/workflows/ci.yml | sort -u)
```

Everything outside both is change-triggered (`ls scripts/check_*` for the full
list):

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
  `check_scheduler_bypass.py`
- `assets/agents/*.toml` or `assets/prompts/` → `python3 scripts/generate_builtin_agents.py`
  to regenerate `src/agent/builtins/generated.rs` (never edit it); `--check` in CI.
  That generator owns `src/agent/builtins/mod.rs` **outright** — it rewrites the
  whole file. `assets/subagent-names/*.toml` is separate: regenerate it with
  `python3 scripts/generate_subagent_names.py` into
  `src/agent/subagent_names/generated.rs`, whose sibling `mod.rs` is hand-written
  and must not be handed to the builtins generator.
- Storage layout →
  `check_project_catalog_invariants.py` (`STORAGE_LAYOUT_VERSION` must track the
  highest migration in `crates/codegg-core/src/session/schema.rs`); adding or
  changing any model list → `check_no_hardcoded_models.py` (CodeGG ships **no**
  compiled-in model catalog; discovery is canonical, config models are
  additive — see `architecture/provider.md` "Model discovery authority");
  adding a **writer surface** (a tool that can mutate a checkout, a daemon
  module that performs a filesystem effect, or a CodeGG-managed process
  launcher) → `check_writer_disposition.py`, which requires an explicit
  disposition in `docs/writer-disposition.toml`. The launcher list lives in
  the guard's `PROCESS_SURFACE_FILES`: a new launcher is a deliberate
  addition there *and* in the manifest, never a silent omission;
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
- A test that **re-executes its own binary** → `python3 scripts/check_test_self_reexec_paths.py`
  (every `--exact` argument must be the full path of a test function defined in that same
  file). `--exact` matches the whole test path, and that path is `<member>::<test_fn>` for a
  family member but a bare `<test_fn>` for a standalone target — so moving a file into a
  family silently invalidates the bare name it used to carry. A name that matches nothing
  selects **zero** tests and still exits **0**, so the child does no work and the parent
  reads an empty success as evidence: the fixture prints nothing, the interrupted publisher
  never creates its lock, and the recovery scenario is never exercised while the test stays
  green. In `scripts/verify.sh quick` and in CI.
- A binder of the **shared integration-test helper tree** → `python3 scripts/check_test_common_binding.py`
  (every binder of the root package's `tests/common/` must bind it with `pub mod common;`). The
  tree's items are `pub` and it carries **no** blanket `#[allow(dead_code)]` — that suppression was
  removed on purpose — so a *private* `mod common;` makes them unreachable from the binding crate
  root and rustc reports every one of them as dead code: a hosted clippy failure under
  `-D warnings`, visible only on a **Linux** runner, because this tree's macOS build fails earlier
  on unrelated findings. In `scripts/verify.sh quick` and in CI.
- Tool Program parse-only invariant → `python3 scripts/check_tool_program_parse_only.py`
  (rejects a `pyo3` dependency, a `pyo3` import, a process spawn, or a Python FFI
  link in `crates/codegg-core/src/tool_program/`). The three test functions that
  used to "enforce" it could not fail; the guard replaced them.
- A **skill guide** (`.opencode/skills/<name>/SKILL.md`) → `python3
  scripts/check_first_party_skills.py` (frontmatter `name` must equal the
  directory name and be unique and ≤ 64 chars, the description must be
  non-empty, every relative link in the guide must resolve, and the guide must
  have a `` | `name` | `` entry in the **Skills index** below — dropping that
  row fails the guard). `testing-ci`, `tool-execution`, `security-hardening`, and
  `lsp-ide` must always exist. In `quick`; **not** in CI.


## Automatic guards

A guard that runs only when somebody remembers it protects nothing, so the
guards whose defects fail **silently** are wired into `scripts/verify.sh quick`
and CI rather than left change-triggered. Each carries a `--self-test` that
injects the defect and proves the guard rejects it:

- `check_git_forbidden_patterns.py` — hostile Git metadata: the pinned override
  table, the route obligations, and the bounded hostile-config detector's *call*
  from both trusted child-construction paths. It was change-triggered, and the
  review that mutation-tested it found that deleting **both** detector call sites
  produced **zero** findings, because the guard did not assert the calls in the
  first place.
- `check_git_consistency_consumers.py` — every diff consumer fences its read
  through `admit_generation_bound`, so no consumer discards the consistency class
  with a bare `.into_value()`.
- `check_contained_worktree_grant.py` — the contained worktree child's writable
  grant is the **shared** administration root `<common-gitdir>/worktrees/`, and
  ADR-0048, the code comments and the containment test must all name that same
  single path. The guard derives the expected path from the ADR's own text, so
  it cannot drift from it.
- `check_writer_fence_completeness.py` — every mutating tool has a `None` arm
  that **refuses** (the `commit` tool was the one that did not), the daemon's
  team-policy principal fallback is derived from the team state rather than a
  literal `true`, the nested-subagent dispatch forwards the parent's fence
  instead of silently downgrading to a containment-only one, and the fence's
  **durable authority reads** are never answered with "absent" — an unreadable
  run store is an unresolved lease, not a missing one, and answering it with
  "absent" produced a writer with no fence at all.
- `check_checkout_mediation_authority.py` — the reservation authority must not
  resolve a path through a **capped** list (a 512-row oldest-first list fails
  open once the catalog grows), the HTTP routes must refuse rather than skip the
  gate when no daemon is attached, and the gate's workspace-identifier rule must
  be at least as permissive as **every** effect family's — a gate stricter than
  an effect refuses identifiers the effect would accept, so the mediation never
  engages.
- `check_daemon_cwd_usage.py` — no `std::env::current_dir()` in workspace-bound
  daemon code; the root is threaded through `ExecutionContext`. It covers
  `src/core/**`, the turn-code surfaces and the `codegg-core` crate. It was
  wired into neither `quick` nor CI before the C008 corrective extended it.

Re-derive the **automatic** set — every guard that runs in *both* `quick` and
CI — rather than trusting a hand-copied list:

```bash
comm -12 <(awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' | sort -u) \
         <(grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' .github/workflows/ci.yml | sort -u)
```

The six named above are the guards whose classification was **decided** in this
line's work; the derived set is larger, and the command is what says which.

## Gotchas

- A **relative** path in any tool is relative to the session's `workspace_root`,
  never to the process cwd. `validate_path`/`validate_target_path` join the
  `allowed_root` first; `TestTool`/`PythonScriptTool` default `workdir` to it
  and refuse rather than guess when there is none. Turns run in a user-scoped
  **daemon** whose cwd is wherever it was launched, so resolving a relative path
  against it silently retargets a whole session into an unrelated tree — and it
  stays invisible while you work in whichever repo the daemon happened to start
  from. Two tell-tales of this class of bug: a tool reporting a file missing
  that its own hint then suggests, and tests/scripts executing "in the wrong
  workspace". **Walk the path you validated** — `glob` and `list` both validated
  a root and then walked the raw request, the same bug wearing a different hat.
  See `architecture/tool.md` and `architecture/workspace.md`.
- **A missing workspace root is a refusal, never a cue to read the process cwd.**
  Every scheduler executor binds `JobExecutionContext.workspace_root` (whose own field docs say
  "executors must use this instead of consulting process-global CWD"), and
  `ToolRegistry::with_options` registers an `UnrootedTool` for each workspace tool when
  `workspace_root: None` rather than calling `X::default()`. The `Default` constructors themselves
  now seed the sentinel `tool::util::unresolved_workspace_root()`, which `validate_path`/
  `validate_target_path` reject with `ToolError::MissingWorkspaceRoot` — so a tool built without
  a root fails typed instead of silently reading an unrelated tree. (`SubagentJobExecutor` was
  the last executor deriving its root from the payload's `allowed_paths`, so a legitimate job
  with an empty list was refused while the daemon held the right root.)
  `check_daemon_cwd_usage.py` enforces this, has **no** line-pattern allowlist (its predecessor
  matched `allowed_root: std::env::current_dir` and so passed with every violation present), and
  carries `--self-test`; if you change it, prove it fails on an injected violation before citing
  it as evidence. See `architecture/tool.md` and `architecture/workspace.md`.
- `PermissionRegistry`/`QuestionRegistry` are sync (`register`/`respond`/`answer_question`
  are `fn`, not `async`); register the responder BEFORE publishing the Pending event.
  See `architecture/permission.md`.
- One approval on screen, the rest in `DialogState::permission_queue`. `show_permission_dialog`
  must not assign straight into `permission_dialog`: that destroyed the previous request
  **together with its `perm_id`**, leaving the daemon waiter parked on a human answer no UI
  pointed at, so the tool timed out denied for an invisible reason. Related: `close_dialog` needs
  its `Dialog::Permission` arm calling `requeue_open_permission()`, because anything still mounted
  at close time was dismissed *unanswered*; every answer path must therefore clear the dialog
  fields **before** calling `close_dialog()` or it requeues what it just answered.
  `CoreEvent::PermissionPending` carries no `args`, so `mount_permission_dialog` recovers them
  from the transcript — approving a command you cannot read is not a decision.
  See `architecture/permission.md`.
- An advisory must never create an escalation on its own: gate it on a **finding**
  (`preflight_requires_escalation`), never on having run. Command preflight is wired by default,
  so testing `CommandPreflightResult::is_some()` escalated every `bash` call — and because that
  arm is reached from `PermissionResult::Allow` as well as `Ask`, it silently discarded stored
  `AlwaysAllow` grants. That is the "the agent overrode my approval" symptom; see
  `architecture/permission.md` "Command preflight is advisory".
- Remembered bash approvals are **per segment**: `decision_scopes_for_shell_command` splits a
  compound command on control operators outside quotes and combines the segments with
  **intersection** semantics (all must Allow; any Deny denies). Anything whose interior the
  splitter cannot see — `$( )`, `${ }`, heredoc, unbalanced quotes — stays one
  `cmd:exact:` scope. Never resolve the intersection to "any segment is enough".
  See `architecture/permission.md`.
- `BashTool` has two independent roots: `workspace_root` (default `workdir`, command-intent
  context, every session) and `child_workspace_ceiling` (the isolated child-Git worktree
  ceiling, armed only by `child_git_policy`). Never derive the ceiling from `workspace_root` —
  it rejects *any* absolute path outside its root, including `/dev/null`.
  See `architecture/tool.md`.
- Never route a TUI answer through `AppMode` alone: `RemoteCore` covers both the WS-attach
  frontend (owns `remote_send_tx`) and the socket-daemon frontend (owns `core_client` only).
  Use `App::is_ws_attach_transport()` — enforced by `scripts/check_tui_transport_authority.py`.
  A mode-only gate drops the answer into an absent channel, orphaning the daemon waiter on an
  unbounded human wait, so approving a tool hangs the turn with no visible error.
  See `architecture/tui.md` and `architecture/core.md`.
- Progress recovery keys every incident on the whole `action_key()` (tool **plus** normalized
  arguments), `EquivalentError` included. Do not reintroduce a tool-name-only check, and do not
  let error *wording* into a key either — the arguments already identify the work. (Two such
  arms shipped once and each killed ordinary turns: `ShortCycle` fired exactly when a model
  reached for the same tool again with *different* arguments, and `EquivalentError` stalled a
  model bisecting a build failure over five genuinely different commands.) A successful read-only
  call the turn has never made is `NewEvidence`: it cannot be a repeat, so it must never be
  escalated. Ladder state belongs to one incident and resets when `action_key()` changes, so a
  model that fixed its problem mid-sequence is not terminated for the failures that preceded the
  fix. A successful mutating or process-class call carries **no** progress signal on its own —
  `state_changed` needs a drained file-change bus event, which nothing emits for process
  execution; do not widen it to "the call succeeded" or the ladder goes blind to a loop. The outer
  run bound is `ExecutionLimits::max_turns`, not the recovery ladder. A `ProgressSignal` variant
  with no producer is a claim, not a hook: `ChildAdvanced` sat hardcoded `false` at the only
  construction site until it was deleted. See `architecture/agent.md`.
- The main buffer has **no status row of its own**. The working strip that used to
  reserve one (reading `idle` most of the time, with a clickable row per advertised
  subagent under it) is deleted, and **nothing is reserved** between the transcript and
  the composer, so the transcript cannot reflow when the turn state changes. The task
  state lives in the `Ctrl+T` tasks overlay (todos, active subagents, live jobs), and
  the row above the composer is the buffer's one *situational* surface: the `space help`
  chord, a latched turn failure, and an armed exit confirmation. It names **no verb
  keys** — the help one keypress away is where the verbs are described. Turn failures
  are **latched** in `SessionState::last_error` and cleared only by `TurnStarted`;
  `report_turn_error` is for turn failures alone, not for shell/clipboard/theme
  feedback. There is one cadence (`tui::components::spinner`, one frame table, 100ms)
  and one `ActivityPhase` enum — a second spinner or a free-form label is how the
  indicator a user watches became ten times slower than the one nobody looked at.
- **Leaving the program takes two deliberate presses.** `q` and `Ctrl+C` each need a
  second press of the same key within three seconds (`EXIT_ARM_WINDOW`); the first
  `Ctrl+C` clears the input and arms the exit, and the row above the input names the
  key to press again. `Esc` is the **mode switch** (Insert ↔ Normal) and never returns
  a panel's selection to the main LLM — `Shift+Q` is the panel exit. A timed arm needs
  `animation_interval` to keep the loop awake, or the confirmation outlives its window
  and the second press is read as a first press.
- The **checkout writer permit** is a *third* authority, and it is neither of the
  two it resembles. The *worktree lease* says who owns a checkout; the
  *scheduler* says whether a job may run; the permit says **who may write to this
  checkout right now** (`codegg_core::writer_permit` + `src/core/writer_permit.rs`).
  The scheduler's exclusivity keys cannot carry it — the writer-bearing job kinds
  (`Shell`/`ManagedProcess`/`Test`/`Lint`/`Python`/`ToolProgram`) have **empty**
  key sets and only `exclusive:`-prefixed keys conflict at all — so the permit
  has its own durable store, its own compare-and-swap, and its own bounded
  per-checkout waiter. Three rules that are easy to get wrong: a permit is never
  freed without **positive** quiescence evidence (unproven evidence quarantines
  it, and a lease deadline never frees a live writer); **dropping** a
  `WriterPermitGuard` quarantines the permit rather than releasing it; and the
  permit's presence enables no shared mutation by itself. Routing: C003 routes
  the model's native file and typed Git writers through it, C004 mediates the
  non-broker writers, C005 refuses the process surfaces pre-effect, and C006
  makes the shared checkout an opt-in, platform-gated path.
  See `architecture/writer_permit.md` §"The checkout writer fence (C003)".
- The **shared checkout profile** is opt-in and **never inferred**
  (`codegg_core::run_placement::RunPlacement`, `architecture/writer_permit.md`
  §"The shared checkout profile (C006)"). The default is `Auto` — an isolated
  managed checkout — and the durable payload field is `#[serde(default)]`, so
  an older payload and an older client keep the behaviour they had. Two rules
  are load-bearing: the group is the reservation (a run outside the live group is
  refused whatever scope it is handed, because the fence resolves the run's scope from
  the run's **own durable record**, never from a caller-supplied path), and the profile
  is **platform-gated** by a compile-time table rather than a runtime flag. The two
  platforms are not symmetric: macOS is qualified on a hosted lane's evidence (Seatbelt
  obtained, and a confined process unable to write outside its own roots), while Windows
  lacks a *backend* — no filesystem-enforcement backend is registered for it at all, so
  no CI run could produce the evidence.
- The **build quota** is admission, and it is not the writer permit. M007
  replaced the legacy *global* `exclusive:workspace-mutation` key on the
  `Build`/`Format` profiles with a **counted, workspace-scoped** quota
  (`codegg_core::jobs::quota`, `architecture/build_admission.md`). Three
  properties are load-bearing: the scope is the canonical `WorkspaceId`, so
  unrelated workspaces never share a bucket; the limit is
  **configuration-owned** (`scheduler.quota.max_concurrent_builds_per_workspace`,
  default 1), so a job can ask for slots but never widen the bound it is measured
  against; and the scope is bound at the *submission boundary* **unconditionally**, so a
  caller that supplies its own resource request cannot opt out. An unknown quota
  namespace is **refused**, not ignored — an unenforced bound that was silently dropped
  would be a lie. A waiting job holds **no** permit: the quota is checked before any
  dimension is reserved, and the slots come back when the permit guard drops, after the
  process lifetime ends. The queue caps are not execution quotas.
- A **checkout is a second working directory of a project, not a new
  project.** The catalog's auto-provisioning rule gives every previously
  unseen workspace root **its own logical project**
  (`ProjectStorage::reconcile_workspace_path` → `resolve_project_and_repository`),
  which is right for a directory the user opens and wrong for a launch
  (M006, `architecture/session_launch.md`). A launch that auto-provisions the checkout's
  workspace binds the session to a project nobody asked for — and then fails at the *last*
  step, because `SessionStore::create_with_binding` requires the workspace's binding to
  name the same project as the session, with the checkout already on disk.
  `ProjectStorage::bind_checkout_workspace` is the explicit path: the checkout's workspace
  joins the launch's project, shares its repository, and names the managed checkout. It is
  refused when the workspace is already bound to a *different* project.
- A **launch does not take the worktree lease.** The lease is what fences writers, and the
  writer that works in the checkout is the session's own agent run. A lease held by a
  synthetic owner would fence that run out (`acquire_lease` refuses a checkout that already
  has an owner) and nothing would ever release it, because a synthetic owner has no lifetime.
  The checkout stays `ready` and unleased until a run takes it — and a checkout another run
  *holds* is refused as a placement, because sharing it is shared-checkout execution, which
  M006 does not enable.
- `ToolBroker` is the only production tool-call boundary. Heavy work (tests, managed
  processes, subagent dispatch, tool programs) goes `JobSubmissionService` →
  `JobScheduler`, never direct executor calls. See `architecture/tool_broker.md`.
  `scripts/check_tool_broker_boundary.py` enforces this in `verify.sh quick` and
  CI: a **registry dispatch** (`ToolRegistry::execute_capture`) is a crossing
  everywhere outside the registry's own module, and a **tool-instance dispatch**
  (`Tool::execute`/`Tool::execute_structured`) is intra-tool delegation inside
  `src/tool/` and a crossing outside it. One **recorded exception** exists — the
  M006 Automatic approval reviewer's investigation call, which cannot be routed
  through the broker because the broker requires a verified authority grant the
  reviewer cannot hold without approving its own investigation. The exception is
  scoped to one call in one function, is recorded in the guard with its owner,
  and widening it is a deliberate edit to that guard.
- `egggit` never mutates; mutations live in `src/git_mutations.rs` (+ network/config
  policy in `src/git_network_policy.rs`). See `architecture/git.md`.
- Daemon is a user-scoped singleton (`flock` on `daemon.lock`; `CODEGG_DAEMON_HOME`
  overrides). Plain `codegg` connects-or-starts (`src/core/instance.rs`); `--standalone`
  is in-process core; the `server` requires `--standalone-core`.
- Command intent defaults to `Observe` (classify only); kill switch
  `CODEGG_ROUTING_DISABLE=1`. See `architecture/command_intent.md`.
- Human `!cmd` stays out of model context; `!!cmd` and `/shell-include` require
  daemon acknowledgement for bounded/redacted, session-owned staging before
  the next turn can consume them. Turn admission atomically commits promotion
  consumption, the controller lease, and a receipt keyed by envelope
  `request_id`; retry the identical request with that ID. Startup marks any
  nonterminal accepted receipt interrupted and never replays it. `/shell-ask`
  stages its question/evidence before submitting through the bound session.
  Staging expires after 24 hours;
  PTY bytes remain private local output. Remote prompt-only transport keeps an
  explicitly unsubmitted composer draft. See `architecture/human_shell.md`.
- Slow TUI handlers use `spawn_tui_task` + `finish(request_id)`/`fail(request_id, err)`
  guard with a stale-completion test (see `src/tui/async_cmd.rs`). Note
  `dispatch_tui_command` itself is `pub(crate) async` with five `.await` points in
  the durable-editor arms — dispatch is not purely synchronous
  (`src/tui/runtime/command_dispatch.rs:87`). See `architecture/tui.md`.
- Auth: `ExternalCommand` is unsupported; never log secrets. The config-first
  registration path explicitly registers all 17 built-ins, each resolving its
  own config then its conventional env var, so defining a provider in config
  does NOT disable env-var auto-registration for the others. The
  `if registry.list().is_empty() { register_builtin(registry) }` fallback at
  `crates/codegg-providers/src/provider_core.rs:1416` is a redundant safety
  net that only fires when config-based registration produced zero results
  (`architecture/provider.md` §"17 Config+Env Providers").
- `AssetRegistry::build` takes each global root as the *parent* dir and appends
  `<vendor>/skills` (`src/skills/registry.rs:250-303`). Pass
  `default_global_discovery_root()` — exactly `dirs::config_dir()` — never an
  already-joined `…/codegg/skills` path, which double-joins to
  `…/codegg/skills/codegg/skills`. Because `resolve_source_roots` skips a
  missing root with no diagnostic, that mistake silently drops every global
  skill. Seven sites build their own registry (across six files — `src/tui/app/mod.rs`
  holds two); all are listed in `architecture/skills.md`.
- New web-search providers belong in the external `eggsearch` project; CodeGG's
  retired `src/search/` provider tree must not be recreated — the live tree is
  `src/search_backend/` (`architecture/search_backend.md`). New deterministic
  validators go in the `eggsact` crate first (`architecture/deterministic_tools.md`).
  New LSP servers go in `crates/egglsp/src/server.rs` + config
  (`architecture/lsp.md`).
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
  Re-measure with `python3 scripts/measure_overview_counts.py`; the method behind
  each row and the reconciliation history that pinned it live in
  `architecture/count_audit_history.md`.
- `plans/registry.md` is the authoritative milestone/roadmap status — check it before
  assuming any roadmap state. `plans/README.md` defines the planning hierarchy and
  status vocabulary; the `planning` skill is the operational guide.
- `README.md` is the user-facing entry point (quickstart) and indexes `docs/`.
  `docs/` is user-facing guidance; `architecture/` is authoritative where the two differ.

### Documentation surfaces

Four doc surfaces, each with one authority. Change a contract in **one** place and
point the other three at it — do not restate a count or a contract in all four.

| Surface | Audience | Authority | Index |
|---|---|---|---|
| `architecture/*.md` | Implementers | **Authoritative** for contracts and counts. | `architecture/overview.md` |
| `docs/*.md` | Users | Derived from `architecture/`; user-facing guidance only. | `README.md` |
| `.opencode/skills/*/SKILL.md` | Agents | On-demand module guides; each names its `architecture/` doc. | **Skills index** below |
| `plans/**` | Maintainers | Milestone and roadmap status. | `plans/registry.md` |

Rules that keep the surfaces honest:

- A **count** belongs in `architecture/overview.md`'s `## Verified counts` table and
  nowhere else. Every other doc names the symbol or path it counted rather than
  repeating a number.
- A **contract change** updates the `architecture/` doc and the matching skill
  guide together (`AGENTS.md` → "Skills index" gives the pairing).
- A **user-facing claim** (a command, a flag, a config key) belongs in `docs/`
  first, then the `architecture/` doc for the contract behind it.
- Every doc that states a fact carries a dated `## Source verification` note
  naming what was checked and when. Refresh it when you re-verify; do not
  re-stamp a doc you did not check.
- `docs/validation/` closure records are **historical evidence**. Their line
  numbers are pinned to old SHAs by design — never "refresh" them.

### docs/ index

| Scope | Files |
|---|---|
| User guides (linked from the README) | `install.md`, `cli.md`, `configuration.md`, `providers.md`, `daemon.md`, `tui.md`, `agents-skills.md`, `tools.md` |
| Integration notes | `LSP.md`, `MCP.md`, `PLUGINS.md`, `themes.md`, `playwright.md`, `repository-init.md` |
| Reference | `security-semantics.md`, `TROUBLESHOOTING.md`, `execution-ownership.md` (+ `execution-ownership.toml` manifest), `dependency-maintenance.md`, `windows-acl-leases.md` |
| Guard manifests (machine-read; do not hand-edit) | `test-ownership.toml`, `test-assertion-quality.toml`, `test-duplication.toml`, `security-test-filters.toml`, `writer-disposition.toml` |
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

A skill earns its place by being **loadable at the moment it is needed** and
**verifiable against source**. When you touch one:

- Every file path, symbol, and line reference must resolve. A reference more
  than ~5 lines off is wrong; fix it or replace it with the symbol name.
- Every `cargo test` example must use the **family/member** form
  (`cargo test --test <family> -- <member>`) — see **Testing** above.
- Do not restate a contract that the linked `architecture/` doc already owns;
  link it instead. A duplicated contract drifts and then lies.
- End with a dated `## Source verification` note naming what you checked.
- Do not add a skill for a module that does not exist, and do not remove one
  whose module still exists — a skill is pruned only with its subject.
- `scripts/check_first_party_skills.py` enforces the frontmatter, the
  uniqueness, the relative links, and the presence of the row below. It is in
  `quick` but **not** in CI, so run it yourself.

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
