# Command Routing Closure Testing and Policy Tightening

## Objective

Close the Phase 06–10 command-intent, Python-scripting, run-store, projection, TUI, and active-routing line of work with a focused validation and policy-hardening pass.

The implementation is now substantial and broadly coherent. The remaining work is not feature expansion. It is to prove that active routing is conservative, that every structured backend produces complete durable evidence, that permission policy matches real command risk, that UI controls only expose supported actions, and that projection/context behavior is single-pass and auditable.

This pass should leave the system in one of two explicit states:

1. active routing is validated for a documented set of intent families and remains opt-in; or
2. any family that does not meet the closure criteria is forced back to Observe or RawShell until corrected.

## Current state to preserve

Preserve these invariants:

- global command-intent mode defaults to `Observe`;
- `CODEGG_ROUTING_DISABLE=1` remains a global kill switch;
- complex shell never enters structured active routing;
- active routing requires `validate_for_active_routing()` success;
- package installation commands remain outside Build/Lint/Format routing;
- raw artifacts remain durable and projections remain bounded;
- Python portable fallback is not described as equivalent to OS-level sandboxing;
- Linux Landlock evidence remains explicit in `PythonRunResult` and run manifests;
- `.codegg/runs/` remains the canonical new run store;
- projection redaction is mandatory and cannot be bypassed by native or RTK projectors.

## Non-goals

- Do not add new command families.
- Do not enable active routing by default.
- Do not broaden Python capabilities.
- Do not add network-enabled or dependency-install Python profiles.
- Do not implement a general rollback engine unless required to remove a misleading UI action.
- Do not replace the shell parser.
- Do not redesign the run-store schema unless a correctness defect requires it.

## Workstream A: End-to-end active-routing validation matrix

### Goal

Prove classify → plan → permission → validate → route → execute → persist → project behavior for every active family.

### Required matrix

For each family, add one success case, one denied/fallback case, one timeout/failure case, and one persistence/projection assertion.

#### Tests

- `cargo test`
- `cargo nextest run`
- `pytest`
- `uv run pytest`
- npm/pnpm/yarn/bun test commands

Assertions:

- routes to TestRunner only for validated simple argv;
- complex shell falls back;
- test report persists into RunStore;
- timeout/failure status is preserved;
- model projection retains failed-test diagnostics.

#### Git read

- status, diff, log, show

Assertions:

- routes to native git/egggit where supported;
- run manifest records `GitRead`;
- projection source spans reference durable artifacts;
- unsupported flags fall back rather than being approximated.

#### Git mutation

- add, commit, stash, checkout/switch/restore
- merge, rebase, cherry-pick, revert
- push, pull, reset --hard, clean -f, branch -D

Assertions:

- safe mutation subset uses managed argv only after permission resolution;
- dangerous mutations remain RawShell or denied;
- conflict-producing commands persist exit status, stdout/stderr, and changed-worktree evidence;
- no mutation silently bypasses permission policy.

#### Search/read

- rg, grep, fd, safe find, cat/head/tail

Assertions:

- explicit workspace root is honored;
- outside-workspace path falls back or denies;
- find mutation flags never active-route;
- output is bounded and persisted.

#### Python

- Analyze read
- Transform write
- Verify subprocess
- denied network/destructive/dependency cases

Assertions:

- policy evidence persists;
- Landlock vs portable fallback is represented accurately;
- changed files and diffs persist;
- denied runs create a durable denied record when appropriate.

#### Build/Lint/Format

- cargo build/check/clippy/fmt
- make/cmake
- npm run build
- mypy/pyright/tsc
- prettier/black

Assertions:

- project-defined scripts receive managed-process supervision, not an implicit safety downgrade;
- package install commands remain RawShell;
- timeout and output limits are enforced;
- active route still requires permission when command semantics may execute arbitrary project code.

### Acceptance criteria

- Every active family has end-to-end tests.
- Every structured execution path writes a complete run manifest.
- Every failed validation path deterministically falls back or denies.
- No test relies only on classifier output; at least one test per family executes through BashTool or the equivalent dispatch seam.

## Workstream B: Tighten permission policy by semantic risk

### Problem

A command can be structurally safe to route while still executing arbitrary code or materially mutating the worktree. Routing safety and permission safety must remain separate.

### Required policy changes

1. Define explicit policy classes:

- read-only native;
- read-only managed process;
- project-code execution;
- reversible workspace mutation;
- conflict-prone git mutation;
- destructive mutation;
- outside-workspace/network/dependency action.

2. Map intent families and subcommands to policy classes.

3. Require `Ask` by default for:

- merge;
- rebase;
- cherry-pick;
- revert;
- checkout/switch when it may discard or overwrite worktree state;
- commit when hooks may execute;
- make/cmake and package scripts;
- npm/pnpm/yarn/bun scripts;
- formatter commands that write files;
- any managed process not in a narrow read-only allowlist.

4. Require `Deny` or RawShell fallback for active routing when:

- destructive capability is present;
- outside-workspace capability is present;
- package/dependency installation is detected;
- unresolved network access is required;
- permission requests remain pending.

5. Ensure `validate_for_active_routing()` does not treat a valid backend as permission approval.

6. Persist permission decisions in `RunManifest.permissions`.

### Acceptance criteria

- Build/Lint/Format routing does not imply automatic permission.
- Git conflict-producing operations are not auto-allowed.
- Permission decisions are durable and visible in the TUI Policy tab.
- Denied and fallback paths are distinguishable in metrics and manifests.

## Workstream C: Complete RunStore coverage

### Problem

Bash and Python persist runs, but test runner and native git/search integration may still use separate or incomplete persistence paths.

### Required changes

1. Wire RunStore into TestRunner:

- begin a `RunKind::Test` run;
- persist raw test logs;
- persist structured test report;
- persist projection and critical diagnostics;
- complete with accurate status and duration.

2. Wire native git/search execution into RunStore:

- use `GitRead`, `GitMutation`, `Search`, or `NativeTool` appropriately;
- persist invocation, backend detail, artifacts, and projection.

3. Avoid duplicate records when BashTool delegates to another backend:

- one logical user command should create one canonical parent run;
- child runs are allowed only when explicitly linked by `parent_run_id`;
- do not create an outer Bash run plus an unrelated inner backend run.

4. Reconcile `.codegg/test-runs/`:

- migrate, adapt, or document it as a compatibility index;
- new structured execution should use `.codegg/runs/` as the authoritative store.

5. Add recovery tests for interrupted/incomplete runs.

### Acceptance criteria

- All active backends persist manifests.
- Artifact hashes and ranged reads work for test/native artifacts.
- No duplicate unlinked runs for one dispatch.
- Incomplete runs are recoverable/listable and do not corrupt the JSONL index.

## Workstream D: TUI and protocol action correctness

### Problem

Run surfaces exist, but rerun, rollback, promotion, artifact viewing, and policy actions must match actual backend support.

### Required changes

1. Compute capabilities from real support, not run kind heuristics.

Replace or tighten fields such as:

- `can_rerun`;
- `can_rollback`;
- `can_promote`;
- `can_view_artifact`.

2. Do not expose rollback for Python solely because changed files exist unless rollback is implemented and validated.

3. If rollback remains deferred:

- set `can_rollback = false`;
- show changed files/diff without an enabled rollback action;
- document the deferred state.

4. Wire artifact content viewing with ranged RunStore reads or clearly mark metadata-only views.

5. Validate protocol lifecycle ordering:

- RunStarted;
- RunProgress/ArtifactCreated;
- ProjectionReady;
- RunCompleted/RunDenied;
- promotion/rerun linkage events.

6. Ensure reconnect/reload can reconstruct run cells and details from RunStore.

### Acceptance criteria

- No enabled UI action leads to an unimplemented handler.
- Protocol events are ordered and idempotent enough for remote frontends.
- Run detail content survives restart.
- Large artifacts are viewed through bounded ranges.

## Workstream E: Projection, redaction, and promotion single-pass guarantees

### Problem

Projection logic now spans shell, Python, test runner, RunStore, RTK, and context promotion. The contract is unified, but duplicate compression/redaction/promotion must be ruled out.

### Required changes

1. Establish one authoritative projection coordinator.

2. Ensure every projection is assigned exactly one `projection_id`.

3. Ensure redaction runs exactly once at the final model-facing boundary:

- native projectors cannot bypass it;
- Python/Test projectors cannot pre-redact and then record misleading offsets;
- RTK output must still pass through redaction;
- persisted raw artifacts retain their correct redaction/safety labels.

4. Enforce `is_already_projected`/equivalent to prevent double compression.

5. Ensure promotion is evaluated once from:

- token budget;
- redaction state;
- source spans;
- critical facts;
- model tier.

6. Verify source spans refer to durable artifact IDs and valid ranges.

7. Add invariants:

- no duplicate projection IDs;
- no overlapping contradictory redaction records;
- no promotion of artifacts marked unsafe for model;
- critical errors remain available after RTK/compaction.

### Acceptance criteria

- One raw artifact set produces one canonical projection record per projection attempt.
- Redaction records correspond to the final projected text.
- RTK cannot erase required error/test-failure spans.
- Context promotion state matches the durable RunStore record.

## Workstream F: Kill switches, fallback behavior, and metrics

### Required changes

1. Test global kill switch:

```text
CODEGG_ROUTING_DISABLE=1
```

All families must execute through legacy/raw behavior without partial routing.

2. Test global modes:

- Observe;
- Active.

3. Test each per-family `RouteLevel`:

- Off;
- Observe;
- Active.

4. Test validation failures:

- complex shell;
- low confidence;
- destructive capability;
- outside workspace;
- unresolved permission;
- rejected backend.

5. Add or verify metrics for:

- classified family;
- attempted active route;
- successful route;
- fallback reason;
- permission denial;
- execution backend;
- sandbox backend;
- projection backend;
- RTK use/fallback.

6. Ensure metrics contain no raw secrets, full commands when sensitive, or unredacted output.

### Acceptance criteria

- Every active-route failure has a stable fallback reason.
- Kill switch takes precedence over all family settings.
- Observe mode never changes execution backend.
- Metrics distinguish policy denial from technical fallback.

## Workstream G: Adversarial and failure-injection expansion

Extend the existing adversarial suites with focused integration cases.

### Command smuggling

- shell operators hidden through quoting/escaping;
- env assignments;
- command substitution;
- heredocs and redirection;
- Unicode/confusable command names;
- option values containing metacharacters;
- git aliases/config-induced behavior.

### Workspace escape

- symlink paths;
- deleted/recreated paths after canonicalization;
- `..` traversal;
- absolute paths;
- worktree root changes;
- artifact path traversal.

### Python sandbox

- subprocess aliasing;
- dynamic import/eval;
- filesystem writes through less common APIs;
- symlink writes;
- `/proc`, `/sys`, home credential reads on Linux;
- portable-fallback evidence correctness;
- timeout and process-tree cleanup.

### Projection poisoning

- ANSI/control characters;
- fake tool/protocol delimiters;
- secret-like content before and after RTK;
- huge lines;
- invalid UTF-8;
- repeated error blocks;
- malicious file names in changed-file lists.

### RunStore failure injection

- disk full/permission denied;
- interrupted atomic rename;
- corrupted index line;
- corrupted artifact hash;
- concurrent completion;
- retention during active run.

### Acceptance criteria

- No adversarial case enters an unsafe structured route.
- Failures produce bounded, non-secret diagnostics.
- RunStore remains recoverable after injected corruption/interruption.

## Workstream H: Validation commands and evidence

Run targeted suites first:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib python_script
cargo test -p codegg --lib shell::projector
cargo test -p codegg --lib test_runner::projection
cargo test -p codegg-core run_store
cargo test -p codegg-protocol
```

Run integration/adversarial suites:

```bash
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Run configuration combinations:

```bash
CODEGG_ROUTING_DISABLE=1 cargo test -p codegg --lib tool::bash
```

Add targeted tests for Observe, Active, and per-family RouteLevel behavior without depending only on environment variables.

Then run the capped full suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

Also run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-features
```

Record in the handoff/commit message:

- exact commands run;
- pass/fail counts;
- platform and sandbox backend;
- skipped environment-gated tests;
- any known flaky or resource-heavy tests.

## Recommended implementation order

1. Tighten semantic permission classes and active-routing policy.
2. Complete RunStore integration for TestRunner and native routes.
3. Remove duplicate/unlinked run creation.
4. Correct TUI capability/action flags.
5. Consolidate projection/redaction/promotion into a single authoritative path.
6. Add kill-switch/fallback metrics and tests.
7. Expand adversarial and failure-injection coverage.
8. Run targeted, integration, and full capped validation.
9. Update architecture docs with the final supported active-routing matrix.

## Closure criteria

This line of work is closed when:

- active routing remains opt-in and defaults to Observe;
- every active family has end-to-end tests;
- every structured backend persists one complete canonical run record;
- permission policy distinguishes structural routability from semantic risk;
- conflict-prone git and arbitrary project-code commands are permission-gated;
- dangerous/package-install/outside-workspace commands never auto-route;
- TUI actions reflect implemented capabilities;
- projection, redaction, RTK, and promotion are single-pass and auditable;
- kill switches and per-family fallbacks work end to end;
- adversarial suites pass;
- the capped full workspace suite, fmt, clippy, and check pass or any exceptions are explicitly documented.
