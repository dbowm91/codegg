# Phase 04: Test, Git, and Search Intent Routing MVP

## Objective

Begin using the command intent/planner/projection substrate for safe, high-confidence command families. This phase should route common natural commands into existing structured codegg subsystems while preserving raw shell fallback for complex or unsupported cases.

The MVP routing families are tests, read-only git, and search/list/read commands. Python routing is intentionally deferred to Phase 05 except for recognizing and preserving Python intent metadata.

## Relationship to previous phases

Phase 01 adds command intent classification. Phase 02 adds planning and backend route metadata. Phase 03 unifies projections and RTK policy. Phase 04 turns on selected routing for commands where codegg already has mature subsystems or low-risk native equivalents.

## Guiding constraints

- Route only high-confidence simple commands.
- Do not route complex shell.
- Do not silently route destructive mutations.
- Preserve raw command/source artifacts and projection handles.
- Keep raw shell fallback available.
- Make routing visible in logs/TUI/protocol metadata so users can tell when a command was upgraded to a structured backend.

## Routing family 1: tests

The test runner is already the canonical supervised process subsystem for tests. It resolves test scopes, streams output, parses failures, captures raw logs, formats bounded reports, indexes previous failures, and publishes lifecycle events. Phase 04 should make common test-like shell commands route into `src/test_runner/` when safe.

### Commands to route

Route these simple argv forms to `ExecutionBackend::TestRunner`:

- `cargo test ...`
- `cargo nextest ...` or `cargo nextest run ...` if resolver/custom validation supports it;
- `pytest ...`;
- `python -m pytest ...`;
- `uv run pytest ...`;
- `npm test ...`;
- `pnpm test ...`;
- `yarn test ...`;
- `bun test ...`;
- `go test ...`;
- `zig build test ...`;
- `make test`;
- `make check`.

Commands should pass through the existing strict custom command validator or equivalent argv validation. If the command contains shell metacharacters, quotes, redirection, pipes, command substitution, or env expansion, it must not route to the test runner.

### Test runner mapping

Add conversion from `TestIntent` to one of:

- `TestScope::Workspace`, `TestScope::Package`, `TestScope::File`, `TestScope::Changed`, or `TestScope::CustomCommand` if exactly representable;
- a validated custom command using `test_runner::custom`;
- fallback `ManagedArgv` with test projection if not representable but still safe.

Prefer existing `TestRunRequest` fields:

- `scope` from intent;
- `workdir` from command cwd;
- `timeout_secs` from planner/default policy;
- `stall_timeout_secs` from test runner defaults;
- `session_id` if available.

### Test projection

Use `TestReport` directly to build `ProjectionResult`. Do not reparse raw logs if a structured report already exists. Preserve links/handles to `.codegg/test-runs/` logs and report JSON.

## Routing family 2: read-only git

Read-only git commands should route to native `egggit` or native projectors where practical. This improves safety, output stability, and context pressure.

### Commands to route

Initial routes:

- `git status`
- `git status --short`
- `git diff`
- `git diff --staged`
- `git log`
- `git log --oneline`
- `git show` for simple refs;
- `git branch --show-current`.

### Routing rules

- Prefer native `egggit` where exact semantic equivalents exist.
- Use managed argv or raw shell with git projector for flags not yet supported but still read-only.
- Do not route commands with shell syntax.
- Do not route git mutations in this phase.
- `git add`, `git commit`, `git restore`, `git checkout`, `git reset`, `git clean`, and `git push` should remain raw shell or explicit ask/reject according to policy.

### Git projection

Use existing git projectors or adapt them into the unified projection pipeline. Git diff projection should preserve exact file paths, hunk headers, and selected changed lines. Long diffs should be RTK eligible, but exact hunk/file spans must remain available.

## Routing family 3: search/list/read

Many agent shell commands are simple repo navigation or file search commands. Route the safe subset into native tools/projectors to reduce raw output volume.

### Commands to route

Initial candidates:

- `pwd`;
- `ls` with simple path/flag forms;
- `rg` with simple patterns and path args;
- `fd` with simple pattern/path args;
- `find` only for simple read/list forms, explicitly excluding `-exec`, deletion, redirection, or complex predicates;
- `cat <single-file>` only within workspace and only when size policy allows;
- `sed -n <range>p <single-file>` only for simple range reads.

### Routing rules

- Prefer existing `read`, `glob`, `grep`, or search backend tools when the command maps cleanly.
- If a command includes unsupported flags but remains simple argv and low risk, run as `ManagedArgv` and project output.
- If a command includes shell syntax, fallback to raw shell.
- If a command attempts outside-workspace reads, defer to permission policy.

### Projection

Search/list/read projections should produce structured file references with paths, line numbers where available, match snippets, truncation metadata, and raw output handles. Avoid dumping large grep output into model context.

## Agent-facing bash integration

Phase 04 should update `BashTool::execute` or a wrapper around it so command calls pass through the planner when routing is enabled.

Recommended behavior:

1. Receive model-facing bash command.
2. Classify into `CommandIntent`.
3. Plan route.
4. If routing is enabled and backend is safe/high confidence, execute structured backend.
5. If routing is disabled, unsupported, low confidence, or complex shell, execute existing bash path.
6. Return a stable model-facing projection with metadata indicating backend used.

Expose a config flag or internal feature gate:

```toml
[command_intent]
route_safe_commands = true
route_tests = true
route_git_read = true
route_search = true
route_python = false
```

If config schema changes are deferred, use constants or private toggles during development but document the intended final controls.

## Human shell integration

Human `!` and `!!` behavior should retain the existing invariant: `!` output is not model context unless promoted, and `!!` output is promoted. Routing should not silently change that context policy.

For human shell:

- classify and plan for metadata;
- optionally route safe commands if the user has enabled routing;
- preserve shell cells/output handles;
- make the backend route visible in detail view if practical.

## Permission behavior

Low-risk read-only routes can use existing allow behavior. Anything that writes, deletes, mutates git state, uses network, or escapes workspace should ask or reject according to existing permission policy. This phase should not expand mutating capabilities.

Do not bypass `PermissionChecker` or existing shell policy. Command-level permission requests from Phase 02 should bridge into existing permission flows only where well tested.

## RTK behavior

Use Phase 03 RTK policy:

- long test logs: eligible;
- long git diffs: eligible;
- long search output: eligible;
- short structured reports: not eligible;
- exact compiler/test/file/line/diff spans preserved.

RTK absence or failure must not fail execution.

## Tests

Add routing tests with execution mocked where possible:

- `cargo test -p codegg-core` routes to test runner.
- `pytest tests` routes to test runner.
- `cargo test && rm -rf .` does not route to test runner.
- `git status --short` routes to git read backend.
- `git commit -m x` does not route as read-only.
- `rg Foo src` routes to search/list projection.
- `find . -exec rm {} \;` does not route to safe search.
- `cat src/main.rs` routes to file read only if within workspace.
- complex shell fallback still reaches raw shell path.
- context promotion policy remains correct for human `!` and `!!`.

Add integration tests only for stable routes. Do not make tests depend on global git state unless using temp repos.

## Acceptance criteria

- Safe command routing can be enabled without breaking raw shell fallback.
- Common test commands route to `test_runner` and produce `TestReport`-based projections.
- Read-only git commands route to native/projected backends where supported.
- Search/list/read commands produce structured projections and raw handles.
- Complex shell and mutating commands do not silently route to safe backends.
- RTK eligibility metadata is attached for long outputs without requiring RTK.
- Existing `/test`, bash, and shell projection tests still pass.

## Suggested validation commands

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib test_runner::custom
cargo test -p codegg --lib tool::test
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

Broader fallback:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Risks and mitigations

The main risk is user surprise when a shell command is routed to a native subsystem. Mitigate with clear backend metadata and an easy raw-shell fallback. The second risk is accepting shell-smuggled test/search commands. Mitigate by using strict argv validation and treating shell syntax as complex shell, never as a safe route.
