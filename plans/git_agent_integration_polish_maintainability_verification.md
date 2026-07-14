# Git Agent Integration Polish, Maintainability, and Verification Plan

## Status

Proposed post-closure pass for the Git agent integration roadmap.

This plan follows the completion of Phases A-F and the corrective security closure commits:

- `cb192e9` — `RedactedUrl` and hardened Git subprocess environment policy.
- `c2e806f` — adversarial credential, RunStore, tracing, and environment verification.
- `53b2beb` — URL-flow inventory, render/persistence boundary test, and final closure cleanup.

The security findings that triggered the corrective pass are considered resolved. This pass is not intended to expand Git capability. Its purpose is to reduce long-term maintenance risk, make the remaining secret-handling assumptions explicit, verify that the implementation matches its documentation, and leave a compact, durable closure artifact for future contributors.

## Objectives

1. Verify the lifecycle and storage guarantees of credential-bearing rerun descriptors.
2. Eliminate or mechanically guard duplicated Git environment policy definitions.
3. Reduce navigation and review cost in the largest Git implementation files without behavior changes.
4. Verify that native tool, Bash translation, managed Git fallback, raw Git fallback, projection, tracing, and RunStore paths remain semantically consistent.
5. Confirm that security-sensitive documentation reflects code rather than intended behavior.
6. Produce a final maintainability and verification handoff with evidence, known limitations, and deferred work.

## Non-goals

This pass must not add new Git operations, introduce a new Git library backend, change default permission policy, redesign RunStore, add automatic credential storage, add worktree mutation features, or broadly refactor unrelated command-routing infrastructure.

Behavior-preserving cleanup is allowed only when guarded by tests and when the resulting code has a materially clearer ownership boundary.

## Current architecture to preserve

The following boundaries are treated as invariants:

- `crates/egggit` remains read-only.
- `crates/codegg-git` owns typed Git operations, parsing, rendering, risk classification, and secret-aware value types.
- Codegg-owned Git subprocesses use the hardened Git environment policy.
- Simple Git argv from the Bash translation layer can route to the Git backend.
- Shell-owned expressions remain raw-shell executions with raw-shell provenance.
- `render_argv()` is an execution boundary, not a persistence or logging boundary.
- Audit-facing RunStore fields contain redacted argv and command values.
- Structured outputs, projections, errors, tracing, and model-visible output must not expose embedded credentials.
- Deprecated `GitMutating` RunStore variants remain serialization-compatibility shims only.

# Workstream A — Rerun descriptor secret lifecycle

## Problem

The security closure intentionally preserves raw URL credentials in the rerun descriptor so an operation can be replayed. The current documentation states that rerun data is not model-visible and is separated from audit surfaces. That does not by itself prove that the raw value is non-durable, access-controlled, excluded from exports, or deleted on a bounded schedule.

This is the highest-priority verification item in this pass.

## A1. Inventory rerun descriptor construction and storage

Trace every path that creates, clones, serializes, persists, exports, logs, displays, or replays a rerun descriptor.

At minimum inspect:

- `crates/codegg-core/src/run_store.rs`
- `src/git_run_store.rs`
- all `RunCompletion`, `RunManifest`, `RunInvocation`, rerun, replay, and `can_rerun` structures
- memory and filesystem RunStore implementations
- export, backup, diagnostic, and support-bundle code
- TUI or daemon endpoints that expose run metadata
- tests that deserialize historical RunStore records

Create a table in `docs/validation/git-rerun-secret-lifecycle.md` with columns:

| Stage | Type/function | Raw secret possible | Durable | User/model visible | Exported | Protection |

The table must distinguish audit argv from execution/rerun argv.

## A2. Choose and document the accepted lifecycle

Select one explicit policy. Preferred order:

### Option 1: secret-free rerun descriptors

Do not persist embedded credentials. Persist a redacted URL plus a replay requirement stating that credentials must be reacquired from the current credential helper, environment, or user input.

This is the preferred design because credentials embedded in URLs are inherently unsuitable for durable replay records.

### Option 2: ephemeral in-memory raw descriptor

Persist only redacted audit metadata. Retain the raw descriptor in memory for the current process/session, with no filesystem serialization and a bounded lifetime.

### Option 3: protected durable secret storage

Use only if the repository already has a suitable secret-storage abstraction. The raw descriptor must be encrypted at rest, excluded from ordinary exports and logs, access-controlled, and deletable independently from audit records.

Do not invent weak reversible obfuscation or repository-local encryption keys.

## A3. Enforce the chosen policy in types

Avoid representing audit argv and replay argv as interchangeable `Vec<String>` values.

Introduce distinct types or wrappers such as:

```rust
pub struct RedactedAuditArgv(Vec<String>);
pub struct EphemeralReplayArgv(SecretVec<String>);
```

or an equivalent design that prevents accidental serialization and logging.

Requirements:

- raw replay values must not implement leaking `Debug`, `Display`, or ordinary `Serialize`;
- persistence APIs must accept only audit-safe types;
- replay APIs must explicitly consume a secret-bearing type;
- conversion from raw to audit-safe representation must be one-way at ordinary call sites;
- compiler-visible boundaries should make misuse difficult.

## A4. Add rerun-specific sentinel tests

Extend the credential sentinel suite to inspect:

- in-memory manifests;
- filesystem manifests;
- artifact trees;
- index files;
- exported run metadata;
- serialized replay descriptors, if they remain durable;
- diagnostic output;
- `Debug` and JSON representations;
- run listing and detail views.

Use a unique credential sentinel and fail on any durable or user-visible occurrence not explicitly permitted by the selected policy.

Add a positive replay test proving replay still works under the new lifecycle.

## A5. Acceptance criteria

- The lifecycle is documented with code-level references.
- Raw credentials are either absent from durable RunStore data or protected by an existing approved secret-storage mechanism.
- Audit records remain useful after secret removal.
- Replay failure due to unavailable credentials is explicit and actionable, not silent.
- Sentinel tests cover all durable RunStore surfaces.

# Workstream B — Shared Git subprocess policy

## Problem

The root crate and `codegg-core` currently maintain parallel environment allowlists and stripped-variable lists because of dependency boundaries. Manual synchronization is a drift risk, particularly for certificate variables, prompt controls, repository redirection variables, and future Git environment features.

## B1. Extract a dependency-neutral policy crate or module

Preferred design: create a small workspace crate, for example `crates/codegg-git-process-policy`, containing only dependency-light constants and policy construction helpers usable by both the root crate and `codegg-core`.

The crate may expose:

```rust
pub const ALLOWED_ENV_VARS: &[&str];
pub const ALWAYS_STRIPPED_ENV_VARS: &[&str];
pub struct GitProcessPolicy;
```

It must not depend on TUI, providers, RunStore, command routing, or the root binary crate.

If a new crate is disproportionate, generate both lists from one checked-in source file and add a staleness check. Do not leave manual synchronization as the only guard.

## B2. Normalize async and sync application

Ensure `apply()` and `apply_sync()` share one policy definition and differ only in process-command type.

Verify consistent handling of:

- `PATH` and executable discovery;
- `HOME` and XDG variables;
- locale and timezone;
- SSH agent variables;
- certificate variables;
- `GIT_TERMINAL_PROMPT=0`;
- editor and sequence-editor disabling;
- pager disabling;
- GPG behavior;
- all `GIT_CONFIG_*` injection forms;
- `GIT_DIR`, `GIT_WORK_TREE`, index, object, and alternate-object variables;
- askpass and SSH/proxy command variables.

## B3. Add policy drift tests

Add tests that compare every consumer’s effective environment against the canonical policy.

Required cases:

- root async Git execution;
- root sync Git execution;
- `codegg-core` worktree execution;
- TUI Git probes;
- daemon snapshot Git commands;
- managed/raw fallback.

A future policy change must fail tests if one consumer is not updated.

## B4. Platform-specific policy verification

Document and test platform differences:

- Windows `USERPROFILE`, `HOMEDRIVE`, and `HOMEPATH`;
- `PATHEXT` and `git.exe` discovery;
- Windows SSH agent named pipes;
- macOS keychain credential helper behavior;
- Linux and macOS certificate stores;
- non-UTF-8 environment values.

Unix-only adversarial tests may remain guarded, but pure policy composition tests must run cross-platform.

## B5. Acceptance criteria

- One canonical policy source exists.
- Root and core consumers cannot silently drift.
- Certificate and SSH-agent functionality remains intact.
- No direct `Command::new("git")` call remains outside an explicitly documented exception.

# Workstream C — Large-file maintainability cleanup

## Problem

Several Git implementation files have become large enough to increase navigation, review, and merge-conflict cost. The goal is not arbitrary file-size reduction; it is to establish coherent module ownership and isolate security-sensitive boundaries.

Candidate files include:

- `crates/codegg-git/src/parser.rs`
- `crates/codegg-git/src/render.rs`
- `crates/codegg-git/src/operation.rs`
- `src/git_service.rs`
- `src/git_network_ops.rs`
- `src/tool/git.rs`

## C1. Measure before refactoring

Record:

- file line counts;
- public items per file;
- test modules per file;
- major operation families;
- cyclic or cross-family dependencies;
- frequently changed sections from recent commits.

Only split files where ownership boundaries are clear.

## C2. Decompose parser and renderer by operation family

Preferred structure:

```text
crates/codegg-git/src/parser/
  mod.rs
  read.rs
  staging.rs
  branch.rs
  history.rs
  network.rs
  destructive.rs
  config.rs

crates/codegg-git/src/render/
  mod.rs
  read.rs
  staging.rs
  branch.rs
  history.rs
  network.rs
  destructive.rs
  config.rs
```

Requirements:

- preserve the public `parse_git_argv()` and `render_argv()` interfaces;
- keep parser/render round-trip tests centralized;
- keep `RedactedUrl::expose_secret()` use tightly localized and searchable;
- avoid duplicating option parsing helpers;
- do not change supported syntax during the split.

## C3. Decompose model-facing Git tool dispatch

Split schema definition, read dispatch, mutation dispatch, recovery dispatch, and raw compatibility execution.

Suggested structure:

```text
src/tool/git/
  mod.rs
  schema.rs
  reads.rs
  mutations.rs
  recovery.rs
  raw.rs
```

Keep one registration point and one externally visible tool name.

## C4. Clarify service versus executor ownership

Review `git_service.rs`, `git_mutations.rs`, `git_mutations_ops.rs`, and `git_network_ops.rs` for overlapping responsibilities.

The intended distinction should be explicit:

- service: route typed request to the appropriate implementation;
- executor: run process with snapshots, policy, timeout, and outcome capture;
- operation helper: validate and construct one operation family;
- projector: render already-sanitized typed results;
- persistence adapter: store audit-safe records.

Move helpers only where this reduces ambiguity. Avoid introducing generic abstraction layers without multiple real consumers.

## C5. Acceptance criteria

- Public interfaces remain stable unless a migration is explicitly documented.
- No behavior changes are mixed into mechanical moves.
- Security-sensitive escape hatches remain easier, not harder, to audit.
- Focused tests pass after each family split.
- Documentation points to the new module ownership boundaries.

# Workstream D — Execution and provenance parity verification

## D1. Build a path matrix

Create a table covering each execution origin:

| Origin | Example | Planned backend | Actual backend | Environment policy | Redaction boundary | RunStore ownership |

Include:

- native typed read;
- native typed mutation;
- native raw Git subcommand;
- Bash simple Git read;
- Bash simple Git mutation;
- managed Git argv fallback;
- raw shell with a Git-leading command;
- TUI Git action;
- daemon Git action;
- replay/rerun.

## D2. Add invariant tests

For representative commands, assert:

- planned and actual backend identity;
- repository root selection;
- permission request generation;
- timeout selection;
- environment policy application;
- sanitized audit argv;
- expected projection route;
- RunStore ownership;
- fallback reason when structured execution is not used.

## D3. Verify fallback honesty

Adversarial commands must remain shell-owned:

```bash
git status | cat
git push && echo done
FOO=bar git status
git show "$(git rev-parse HEAD)"
git diff > patch.txt
```

Simple quoted argv must continue to route correctly where safe:

```bash
git diff -- "file with spaces.rs"
git commit -m "message with spaces"
```

Add regression coverage for malformed quotes, newlines, NULs, pathspec magic, option-like filenames, and revision/path ambiguity.

## D4. Verify repository resolution

Cover:

- repository root;
- nested directory;
- nested independent repository;
- linked worktree;
- bare repository where supported;
- non-repository directory;
- symlinked working directory;
- path outside active project.

Ensure the selected repository is surfaced in output and provenance.

# Workstream E — Security verification refresh

## E1. Re-run the threat model against current code

Update `docs/validation/git-security-review.md` only after inspecting current implementations.

Re-evaluate:

- URL credentials;
- command-bearing environment variables;
- repository redirection variables;
- credential helpers;
- SSH command injection;
- proxy command injection;
- config injection;
- pager/editor hangs;
- process timeout and cancellation;
- pathspec and revision ambiguity;
- tracing and error leakage;
- RunStore and export leakage;
- rerun secret lifecycle;
- shell-boundary provenance.

## E2. Add static checks for forbidden patterns

Add a validation script or test that identifies:

- direct `Command::new("git")` outside approved modules;
- direct logging of `render_argv()` output;
- serialization or `Debug` derivation on secret-bearing types;
- `expose_secret()` calls outside approved execution boundaries;
- RunStore persistence using unsanitized argv;
- duplicated environment policy tables.

The check should have a small explicit allowlist with rationale.

## E3. Property and fuzz-style tests

Extend property testing around URL redaction and parser/render behavior.

URL cases should include:

- HTTPS user/password;
- percent-encoded credentials;
- IPv6 hosts;
- ports;
- query strings and fragments;
- SCP-like SSH syntax;
- malformed URLs;
- multiple URLs in one stderr line;
- ANSI escape sequences around URLs;
- Unicode usernames and hostnames.

Parser/render properties should include:

- parse-render-parse stability for supported operations;
- no raw secret in `Debug` or serialization;
- path arguments remain separated after `--`;
- dangerous flags retain risk classification;
- unsupported syntax does not silently downgrade risk.

## E4. Acceptance criteria

- No open high- or medium-severity finding remains in the refreshed review.
- Any accepted low-severity limitation has an owner, rationale, and regression test where possible.
- Security checks run in CI or the standard validation script.

# Workstream F — Performance and resource verification

## F1. Re-run focused measurements

Measure at least:

- rich status on clean and dirty repositories;
- diff projection for small and large diffs;
- parser/render overhead;
- mutation snapshot overhead;
- RunStore persistence overhead;
- TUI sidebar refresh;
- operation-state detection;
- credential sanitization on large stderr/stdout;
- closure tests under configured test-thread limits.

## F2. Guard against accidental quadratic behavior

Inspect parsers and redactors for repeated whole-string scans or cloning.

Add benchmarks or size-scaled tests for:

- large diff output;
- many changed files;
- many refs/remotes;
- long credential-bearing stderr;
- large argv vectors;
- deeply nested RunStore artifact scans.

## F3. Preserve bounded output behavior

Confirm truncation occurs after redaction, not before, so credentials cannot survive at truncation boundaries.

Verify that exact spans required by diff projection and RTK are preserved.

# Workstream G — Documentation and final handoff

## G1. Reconcile architecture documentation

Update:

- `architecture/git.md`
- `architecture/git_phase_f_handoff.md`
- `architecture/command_intent.md`
- `architecture/command_planner.md`
- `architecture/command_routing.md`
- `docs/validation/git-security-review.md`
- `docs/validation/git-cross-platform.md`
- `AGENTS.md`

Remove obsolete references to pre-unified routing and clearly mark compatibility-only paths.

## G2. Produce final verification artifact

Create `architecture/git_polish_verification_handoff.md` containing:

- final crate/module map;
- execution-origin matrix;
- permission and risk matrix;
- environment policy ownership;
- secret lifecycle decision;
- durable versus ephemeral RunStore fields;
- supported and fallback operation matrix;
- validation commands and results;
- performance summary;
- cross-platform status;
- known limitations;
- deferred work with rationale;
- closure commit references.

## G3. Update roadmap status

Add a final post-closure note to `plans/git_agent_integration_roadmap.md` linking the corrective closure and polish verification handoff. Do not rewrite historical phase completion records.

# Validation matrix

Run focused validation before the full workspace suite.

```bash
cargo fmt --all -- --check
cargo check --workspace --all-features
cargo clippy --workspace --all-features --all-targets -- -D warnings

cargo test -p codegg-git
cargo test -p egggit
cargo test -p codegg-core
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib tool::git
cargo test -p codegg --lib git_mutations
cargo test -p codegg --lib git_network_policy
cargo test -p codegg --lib git_mutation_projector

cargo test --test git_credential_cross_path
cargo test --test git_credential_runstore_sentinel
cargo test --test git_env_attack
cargo test --test git_noninteractive
cargo test --test git_tracing_capture
cargo test --test git_network_integration
cargo test --test git_mutations_integration
cargo test --test git_recovery_integration
cargo test --test git_closure_matrix

CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=8
```

Also run the static forbidden-pattern validation and the Git performance script.

# Commit strategy

Prefer small, reviewable commits:

1. rerun secret-lifecycle inventory and tests;
2. rerun lifecycle/type enforcement;
3. canonical subprocess policy extraction;
4. policy drift and platform tests;
5. parser/renderer family split;
6. Git tool/service maintainability split;
7. execution/provenance matrix tests;
8. refreshed security and performance verification;
9. documentation and final handoff.

Mechanical file moves must not be combined with behavior changes.

# Definition of done

This pass is complete when all of the following are true:

- credential-bearing rerun data has an explicit, tested lifecycle;
- no raw credential is present in ordinary durable RunStore artifacts, exports, logs, projections, tracing, or model-visible output;
- the Git subprocess environment policy has one canonical source or a mechanically enforced generated source;
- all Codegg-owned Git subprocesses use the canonical policy;
- parser, renderer, tool dispatch, and service ownership are easier to navigate without changing behavior;
- native, Bash-translated, managed fallback, raw fallback, TUI, daemon, and rerun paths have provenance and policy parity tests;
- direct execution and persistence boundaries for secret-bearing values are statically and dynamically guarded;
- security, cross-platform, and performance validation documents reflect current code;
- the full workspace suite and focused security suites pass;
- `architecture/git_polish_verification_handoff.md` records the final verified state and remaining limitations.
