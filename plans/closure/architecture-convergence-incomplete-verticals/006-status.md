# Architecture Convergence M006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/006-command-pipeline-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `7a9d2ff`

Implementation commits:

- `7a9d2ff` — converge command pipeline dispatch ownership

## 1. Executive finding

M006 production implementation is complete and conditionally closed. Raw
shell-originated command execution now has one production
pipeline: `prepare_command()` performs parse/normalize and classification,
`plan_execution_with_context()` produces the typed plan, and
`CommandPlan::dispatch_target()` produces the one executor-facing target.
Bash consumes that target directly. The former 294-line independent routing
implementation was deleted; `command_routing` remains only as a zero-logic
compatibility facade for existing callers.

The command-family mapping is also plan-owned, including typed Git risk-family
resolution. Authorization remains at the existing daemon/tool boundary;
active-routing validation is only a preflight eligibility check. Outcome and
persistence representations remain separate because they describe different
states: `ExecutionOutcome` records actual execution truth, while RunStore
types remain the durable compatibility boundary.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory supported command families and aliases | Before/after matrix below; classifier tests and existing routing/adversarial suites | pass | Slash-command definition loading remains a separate typed template/process-definition contract, not a second shell classifier |
| One canonical semantic intent representation | `CommandIntent` plus `command_intent::pipeline::prepare_command()` | pass | No UI state or executor authority was added to the intent |
| Planner/router overlap removed | `CommandPlan::dispatch_target()` owns the mapping; `src/command_routing.rs` is a direct alias/delegator | pass | Production Bash no longer calls `resolve_routing()` |
| Typed dispatch boundary preserved | `CommandDispatchTarget` carries typed argv, cwd, timeout, Python mode, and Git request | pass | Existing executor methods and scheduler admission are unchanged |
| Authorization handoff preserved | `CommandPlan::validate_for_active_routing()` remains preflight; existing Bash/tool authorization and adversarial tests remain in their canonical owners | pass | The new pipeline cannot grant authority; the pipeline unit test proves pending Git permission blocks active eligibility |
| Typed outcome/provenance preserved | Existing `ExecutionOutcome`, `ActualExecutor`, `RunOwnership`, and RunStore mappings unchanged; Bash maps actual executor truth | pass | No boolean/string outcome replacement was introduced |
| Compatibility aliases retained safely | `command_planner` re-export retained; `RoutingDecision` alias and `resolve_routing()` shim retained | pass | Both are source-compatible adapters with no independent policy |
| Explicit execution context | `CommandIntentContext::execution_cwd()` and `plan_execution_with_context()`; Bash supplies workspace/workdir context | pass | Managed argv plans no longer infer cwd from `std::env::current_dir()` |
| Focused and broad verification | Compile checks, Clippy, quick verification, and test attempt recorded below | partial / operational condition | Runtime test linking is blocked by the pre-existing host architecture/library mismatch; exact-head hosted workspace tests are still running |
| Documentation updated | `architecture/overview.md`, `command_planner.md`, and `command_routing.md` | pass | Canonical pipeline and compatibility ownership are documented |

### Command-family routing matrix

| Family and representative aliases | Before | After | Disposition |
|---|---|---|---|
| Tests: `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`, package-manager `test`, `make test/check` | classifier → `CommandPlan` → independent `resolve_routing()` → Bash dispatch | `prepare_command()` → plan → `CommandDispatchTarget::RouteToTestRunner` → Bash dispatch | Aliases preserved; argv remains typed and test validation remains deterministic |
| Git read: `status`, `diff`, `log`, `show`, refs/status families | classifier → Git backend → independent routing enum | `prepare_command()` → typed Git plan → `RouteToGit` | Typed `GitExecutionRequest` and projector policy preserved |
| Git mutation/network/destructive: `add`, `commit`, `push`, `reset`, `clean`, etc. | classifier → Git backend → routing enum; Bash separately re-derived family with fallback mapper | plan-owned typed Git target and `CommandPlan::command_family()` | Risk family is derived once from typed operation/risk set; canonical Git mutation executor unchanged |
| Python: `python`, `python3`, `pytest`, `uv run pytest` | classifier → Python backend → independent routing enum | one pipeline → `RouteToPythonScripting` | Analyze/transform/verify mode behavior preserved |
| Search/file read: `rg`, `grep`, `fd`, `find`, `ls`, `cat`, `head`, `tail`, etc. | classifier → managed argv plan → independent routing enum | one pipeline → `RouteToManagedProcess` with explicit cwd | Shell metacharacters and workspace checks remain classifier/security behavior |
| Build/lint/format: Cargo, npm/pnpm, make, rustfmt, prettier, black, linters | classifier → managed argv plan → independent routing enum | one pipeline → `RouteToManagedProcess` with typed argv | Permission/projector/timeout behavior preserved |
| Explicit complex/raw shell: pipes, redirects, expansion, unclassified commands | classifier → raw-shell plan → independent routing enum | one pipeline → `RouteToShell` | Explicit shell remains distinct and is never merged with native argv |
| Invalid/ambiguous input: empty input, unsupported typed Git argv, unsafe search paths | classifier/plan rejection plus routing conversion | one pipeline → `Rejected` or the existing explicit raw-shell classification | No invalid typed target is dispatched |
| Slash command definitions in `src/command/` | command-file parser with template/process definition semantics | unchanged separate contract | These are user-defined command definitions, not raw shell intent classification; no duplicate pipeline was found |

## 3. Production implementation evidence

- Added `src/command_intent/pipeline.rs` as the canonical raw-command entry
  point, with typed result, active-routing validation delegation, and focused
  family/alias/invalid-input/authorization tests.
- Added `CommandDispatchTarget` and `CommandPlan::dispatch_target()` to the
  canonical planning module. This moved executor-facing mapping beside the
  backend selection it consumes.
- Added `plan_execution_with_context()` and explicit cwd selection. The
  legacy `plan_execution()` wrapper remains available for compatibility.
- Moved routing-family selection to `CommandPlan::command_family()` and kept
  only test/compatibility delegates in Bash.
- Rewired Bash to run the canonical pipeline for all configured and
  unconfigured command executions, while preserving Observe vs Active mode,
  kill switches, scheduler dispatch, actual-executor persistence, and
  terminal active-routing failures.
- Reduced `src/command_routing.rs` from an independent enum/match implementation
  to a direct alias/delegator. No public command or protocol surface was
  removed.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check -p codegg --lib
rtk cargo check -p codegg --all-targets
rtk cargo clippy -p codegg --lib -- -D warnings
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_sandbox_contract.py
rtk scripts/verify.sh quick
rtk git diff --check
rtk cargo test -p codegg --lib command_intent
```

### Results

- Formatting, library and all-target compile checks, targeted Clippy,
  workspace all-features Clippy, execution-ownership guard, sandbox guard,
  quick verification, and diff checks passed.
- `scripts/verify.sh quick` passed generated-agent checks, core-boundary
  checks, sandbox and execution-ownership checks, and workspace all-target
  checking.
- The focused `command_intent` test binary compiled through the test-link
  stage but could not link on this host. The existing x86_64 macOS toolchain
  sees arm64 `/opt/local` libraries and cannot resolve x86_64 LZMA symbols;
  the linker also reports the pre-existing mixed native-library/macOS-target
  warnings. This is an environmental limitation, not a changed-path compiler
  error. The all-target compile check still compiled the test code.
- Existing hosted CI run `33912249757` for exact candidate `7a9d2ff` passed
  setup, all static guards, formatting, and workspace Clippy; its workspace
  test phase was still running when this closure record was written. No new CI
  lane or harness was added.

## 5. Invariant review

- The daemon and scheduler remain the execution/admission authorities. The
  pipeline only selects a target; it does not execute or admit work.
- Native argv and explicit raw shell remain separate types and routes.
- Workspace identity and cwd are explicit in the production Bash pipeline;
  the compatibility classifier wrapper still permits legacy callers to use
  process cwd only outside the daemon-owned path.
- Git risk, mutation, provenance, snapshot/delta, and RunStore behavior remain
  in the existing typed Git owners.
- Child-agent authority, tool authorization, sandboxing, cancellation, and
  output bounds were not widened or bypassed.
- Actual execution truth still drives ownership, RunKind, argv, and backend
  persistence mapping; planned routing cannot falsely claim execution.

## 6. Failure and recovery review

Invalid input produces a typed rejected target before dispatch. Active routing
still treats scheduler/admission/executor failures as terminal and does not
retry through raw shell. Observe mode retains the existing raw-shell behavior.
No process lifecycle, cancellation, restart, durable run, or scheduler state
machine was changed; therefore no new recovery race or persistence migration
was introduced. Existing delegated-run-id and no-double-execution semantics
remain in Bash.

## 7. Migration and compatibility review

No storage schema, protocol DTO, config schema, tool schema, or user-visible
command alias was removed. `command_planner` continues to re-export planning
types. `command_routing::RoutingDecision` is now an alias of
`CommandDispatchTarget`, and `resolve_routing()` delegates directly to the
plan for source compatibility. The old independent routing implementation
was removed because it had no distinct invariant or durable/public boundary.

## 8. Security review

Authorization remains at the existing daemon/tool boundary. Plan-level active
routing validation checks typed argv, confidence, backend eligibility, risk,
workspace scope, destructive capability, and pending permission state, but is
not a replacement for canonical authorization. Search/file path containment,
Git typed risk classification, shell-shape separation, scheduler admission,
sandbox checks, output bounds, and provenance persistence remain unchanged.
No secret, credential, or raw authenticated argv was added to durable state.

## 9. Documentation and operations

- Updated `architecture/overview.md` with the canonical typed pipeline and
  authority boundaries.
- Updated `architecture/command_planner.md` with explicit-context planning,
  plan-owned dispatch, and compatibility re-export details.
- Updated `architecture/command_routing.md` to describe the target as a
  compatibility-free production concept and the old module as a shim.
- No new static guard, CI lane, test harness, dependency, or operational
  migration was introduced.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low / operational | Focused runtime tests cannot link on the current mixed-architecture host | Local test execution evidence is incomplete; compile-time test coverage and all existing static/quick checks pass | Rerun `cargo test -p codegg --lib command_intent`, routing/outcome/Bash focused suites, and normal CI on a corrected x86_64-compatible toolchain |
| critical/high/medium | None | — | None |

## 11. Roadmap disposition

M006 is conditionally closed. Its production contract is complete and its sole hard
dependency, M004, was already available. The dependency audit found no
registered blocked plan whose hard or interface dependency is M006:

- M007 remains `ready` from M002's stable process/edit integration contract;
- M008 remains `ready` from the already-closed session-projection contract;
- the only blocked work in the registry remains the unrelated supported-Linux
  Landlock evidence condition.

No corrective implementation plan or ADR is required. The named low-severity
host-link condition and pending hosted test completion are operational
verification debt and do not identify a production correctness defect in M006.

## 12. Registry updates

- Marked the implementation plan `implemented`.
- Marked M006 `conditionally closed` in the subsystem roadmap and registry.
- Removed M006 from dependency-ready and active implementation sections.
- Added M006 to recently closed control points with commit `7a9d2ff`.
- Retained M007 and M008 as dependency-ready; no downstream plan was newly
  unblocked by this closure.
