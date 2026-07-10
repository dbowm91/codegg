# Phase 10: Expanded Safe Routing and Adversarial Activation Gate

## Objective

Expand structured command routing beyond the initial test/git/search/Python MVP and establish the adversarial validation gate required before active routing can be enabled for real agent workflows. This phase should convert the current observe-first substrate into a selectively active system for high-confidence, low-risk command families while preserving raw-shell fallback for complex or uncertain commands.

The goal is not to eliminate shell. The goal is to activate routing only where classification, planning, permissions, sandboxing, persistence, projection, and rollback semantics are demonstrably stronger than raw shell.

## Scope

This phase covers:

- broader safe command-family support;
- active-routing configuration and rollout gates;
- package manager/build/lint/format/type-check routes;
- selected git mutation workflows;
- route-specific permission and rollback policy;
- command-smuggling and Python-escape adversarial tests;
- output-poisoning/context-promotion tests;
- shadow/observe comparison metrics;
- activation criteria and fallback behavior.

## Existing substrate to reuse

Reuse:

- command intent classifier and `ShellShape` parser;
- explicit workspace-root context;
- planner and `ExecutionBackend`;
- Phase 06 Python capability/sandbox profiles;
- Phase 07 run store;
- Phase 08 TUI/protocol run surfaces;
- Phase 09 projection/redaction/context-promotion pipeline;
- test runner command validation;
- native git/search tooling;
- permission planner and security policy;
- deterministic test monitoring.

## Design principles

1. Active routing is opt-in by route family and confidence threshold.
2. Unknown or complex commands fall back to raw shell, never to guessed structured execution.
3. Route activation requires differential tests against current behavior.
4. Mutations need explicit permission and recovery semantics.
5. A route must preserve user/model intent and argv exactly.
6. Projection and artifact persistence are mandatory for active routes.
7. Security failures disable a route rather than weakening policy.

## Workstream A: Define active-routing configuration

Replace ambiguous booleans with an explicit rollout model if not already complete:

```toml
[command_intent]
mode = "observe" # off | observe | active
minimum_confidence = "high"

[command_intent.routes]
tests = "active"
git_read = "active"
search = "active"
python = "active"
build = "observe"
format = "observe"
lint = "observe"
typecheck = "observe"
package_manager = "off"
git_mutation = "off"
```

Each route should support:

- off;
- observe;
- active.

Defaults should remain observe/off. Active mode should require the shared run/projection infrastructure.

## Workstream B: Add routing decision gates

Before active routing, require all of:

- `ShellShape::SimpleArgv`;
- recognized executable/subcommand;
- confidence at or above configured threshold;
- workspace/cwd containment;
- route enabled as active;
- permission decision complete;
- backend available;
- run store available or explicitly degraded under tested policy;
- projector available;
- no classifier ambiguity/adversarial flags.

If any condition fails, route to raw shell or reject according to existing shell policy. Record the fallback reason.

## Workstream C: Activate proven MVP routes

Activate only after differential validation:

### Tests

- `cargo test`;
- `cargo nextest ...` supported forms;
- `pytest`;
- `uv run pytest`;
- `go test`;
- npm/pnpm/yarn/bun test forms;
- configured strict custom test commands.

Route to test runner with deterministic progress and structured reports.

### Git read

- status;
- diff;
- log;
- show;
- safe branch/tag listing;
- stash list;
- remote read-only forms.

Route to `egggit`/native projector where semantic parity is established; otherwise managed argv with strict validation.

### Search/read

- `rg`, `grep`, `fd` supported forms;
- safe `find` without action flags;
- workspace-contained `cat`, `head`, `tail`;
- `ls`, `pwd`, `wc` where useful.

Route through native search/read tools or managed argv with bounded output.

### Python

Route simple `python -c`, Python script invocations, and explicit model Python tool requests into the Phase 06 backend when source extraction and mode selection are unambiguous. Complex heredocs remain raw shell until separately designed.

## Workstream D: Expand build/lint/format/type-check families

Add strict validators and backend policies for common ecosystems.

### Rust

- `cargo build`;
- `cargo check`;
- `cargo clippy`;
- `cargo fmt --check`;
- `cargo fmt` as a mutation requiring permission/diff capture;
- optional configured `cargo deny`, `cargo audit` if dependencies exist.

### Python

- `ruff check`;
- `ruff format --check`;
- `ruff format` mutation;
- `mypy`;
- `pyright`;
- `python -m compileall` where bounded;
- package installation remains excluded.

### JavaScript/TypeScript

- npm/pnpm/yarn/bun script execution only for known project scripts;
- lint/typecheck/build script names from package manifest;
- formatter mutations require permission and changed-file capture;
- prohibit install/add/remove/update commands in safe routing.

### Go

- `go test`;
- `go vet`;
- `go build`;
- `gofmt -d` read-only;
- `gofmt -w` mutation with permission/diff capture.

### General

- configured make targets only when explicitly allowlisted;
- CMake configuration/build with workspace-contained build directories;
- unknown flags/subcommands fall back.

## Workstream E: Selected git mutation routing

Do not broadly route all git mutations. Start with recoverable, local-only operations:

- `git add` selected workspace paths;
- `git restore --staged` selected paths;
- branch creation without checkout;
- tag creation only if explicitly enabled;
- local commit only under explicit user/agent permission policy.

Requirements:

- no push/pull/fetch/network operations;
- no reset --hard, clean -f, destructive checkout/restore, rebase, merge, or force operations in initial active mutation set;
- capture pre-run git/worktree facts;
- expose exact staged/working-tree changes;
- create run records and recovery guidance;
- request explicit permission for every mutation family unless trusted policy says otherwise.

## Workstream F: Package-manager safety boundary

Package managers are high-impact and frequently networked. In this phase:

- permit read-only metadata commands only where validated (`cargo metadata`, package script listing, lockfile inspection);
- permit project-defined script execution for known non-install targets;
- deny install/add/remove/update/publish/login commands from safe routing;
- keep dependency installation outside Python profiles;
- classify network/package mutations explicitly and require raw shell plus permissions, or reject according to policy.

## Workstream G: Observe-vs-active differential harness

Build a harness that executes fixture commands through:

1. current raw shell path in a disposable workspace;
2. proposed structured route in an equivalent disposable workspace.

Compare:

- exit status;
- stdout/stderr semantic content;
- changed files and hashes;
- git state;
- test results;
- environment/cwd behavior;
- timeout/cancellation behavior;
- projection/actionable diagnostics.

Allow expected differences only through explicit normalization rules.

No route should move from observe to active without passing its differential fixture set.

## Workstream H: Adversarial command-smuggling tests

Add fixtures for:

- pipes, semicolons, `&&`, `||`, backgrounding;
- redirects and fd manipulation;
- command substitution/backticks;
- variable expansion and env prefixes;
- escaped/newline injection;
- Unicode/confusable executable names;
- aliases/wrappers such as `env`, `command`, `xargs`, `sh -c`, `bash -c`;
- `find -exec/-delete/-ok` variants;
- flags containing embedded shell metacharacters;
- path traversal and symlink escape;
- executable shadowing via PATH;
- malicious project-local binaries named like trusted tools;
- argument forms that turn read-only commands into mutation.

The classifier must conservatively raw-route or reject these cases.

## Workstream I: Adversarial Python tests

Add fixtures for:

- alias and dynamic import bypasses;
- `getattr`/reflection-based dangerous calls;
- `shell=True` and indirect subprocess construction;
- filesystem access through pathlib/os/shutil/tempfile;
- symlink escapes;
- `/proc`/device/credential access;
- socket/network attempts;
- package installation and runtime downloads;
- ctypes/native loading;
- pickle/marshal unsafe loading;
- fork/process-tree escape;
- writes hidden from simplistic mtime snapshots;
- race conditions during snapshot/diff;
- output flooding and binary output.

OS sandbox tests should prove denial where supported; fallback platforms should fail closed for known unsupported capabilities.

## Workstream J: Output poisoning and context safety

Test outputs containing:

- prompt-injection instructions;
- fake tool messages/system text;
- terminal escape sequences;
- extremely long lines;
- repeated stack traces;
- forged file/line references;
- secret-like strings;
- invalid UTF-8;
- misleading success/failure markers.

Projection must treat output as untrusted data, retain exact raw spans, strip/escape unsafe terminal sequences, redact secrets, and never convert output text into tool/control instructions.

## Workstream K: Activation metrics and telemetry

Record locally:

- classifier family/confidence;
- observe decision vs active decision;
- fallback reason;
- backend success/failure;
- differential mismatch count;
- permission prompts;
- projection bytes/tokens;
- RTK usage/failure;
- user rerun/rollback actions;
- route disable events.

Avoid collecting sensitive command content beyond local run records unless explicitly configured.

Provide a route-health summary for development/debug mode.

## Workstream L: Kill switches and degradation

- global active-routing off switch;
- per-family off switch;
- automatic route disable after repeated backend invariant failures;
- raw-shell fallback when backend unavailable and policy permits;
- reject rather than fallback when raw shell would violate stronger requested policy;
- preserve run record of failed routing attempt/fallback reason.

## Workstream M: Validation and activation gate

A route family can be marked production-active only when:

- strict argv validator exists;
- adversarial fixture set passes;
- differential fixture set passes;
- permission policy is defined;
- sandbox/backend behavior is defined;
- persistent run artifacts are produced;
- projector/redaction behavior is tested;
- cancellation/timeouts are tested;
- fallback behavior is tested;
- documentation lists supported/unsupported forms.

Suggested commands:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib python_script
cargo test -p codegg --lib test_runner
cargo test -p codegg-core run_store
```

Adversarial/integration suites:

```bash
cargo test --test command_routing_differential
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
```

Full capped suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Acceptance criteria

- active routing is explicit and per-family;
- proven MVP routes can execute through structured backends;
- uncertain/complex commands preserve raw-shell fallback;
- build/lint/format/type-check routes have strict validators;
- package installation/network mutations are not silently routed;
- selected git mutations are permissioned and recoverable;
- differential and adversarial suites gate activation;
- outputs are treated as untrusted data;
- kill switches and fallback reasons are operational;
- run/projection records make every routing decision auditable;
- unsupported route forms are documented.
