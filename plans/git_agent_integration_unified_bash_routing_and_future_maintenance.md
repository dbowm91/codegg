# Git Agent Integration Follow-Up: Unified Bash Routing and Future Maintenance

## Status

Proposed follow-up plan for the remaining Git agent integration work after completion of Phases A-F, the corrective security closure, and the polish/maintainability/verification pass.

This plan has two tracks:

1. **Track U — Unified Bash mutation routing**: close the remaining functional parity gap where a simple Bash-originated Git mutation is parsed and planned as Git but is conservatively dispatched through raw shell because `GitMutating` is not mapped to an active-routing family.
2. **Track M — Future maintenance**: define objective triggers, safe decomposition boundaries, property/fuzz coverage, and long-term regression controls without blocking functional closure.

Track U is the near-term implementation work. Track M is explicitly non-blocking unless one of its trigger conditions is met.

---

## 1. Background

The Git subsystem now provides:

- a typed `GitOperation` vocabulary in `crates/codegg-git`;
- structured read primitives in `crates/egggit`;
- `ExecutionBackend::Git` and `RoutingDecision::RouteToGit`;
- typed mutation, network, destructive, conflict, and recovery execution;
- canonical Git subprocess hardening;
- audit-safe RunStore argv and secret-lifecycle controls;
- Bash command classification through the same typed parser;
- provenance, projection, permission, and execution-origin verification.

The remaining routing caveat is narrow:

```text
BashTool input: git commit -m "message"
  -> shell shape: SimpleArgv
  -> classifier: GitMutating
  -> planner: ExecutionBackend::Git
  -> family lookup: None
  -> active routing disabled for this family
  -> actual execution: RawShell
```

The command is classified correctly and provenance is honest, but the native Git tool and Bash translation layer do not fully converge for simple mutations.

Complex shell expressions must remain shell-owned:

```bash
git commit -m fix && git push
git diff | sed -n '1,120p'
FOO=bar git status
git show "$(git rev-parse HEAD)"
```

This plan must not weaken that boundary.

---

## 2. Goals

### 2.1 Functional goals

- Route eligible simple Bash-originated Git mutations through `RouteToGit`.
- Preserve the same typed operation, risk set, permission defaults, environment policy, projection, timeout, and RunStore provenance used by native Git tool calls.
- Keep complex shell expressions, low-confidence parses, unresolved permissions, unsupported commands, and critical-risk operations on conservative fallback paths.
- Add independent configuration controls for local mutations, network operations, and destructive operations.
- Ensure routing behavior is deterministic and inspectable in observe mode before activation.

### 2.2 Safety goals

- Never promote a command with shell operators or ambiguous shell shape.
- Never silently auto-allow commit, branch switching, stash application, history integration, network writes, or destructive mutations.
- Preserve `DestructiveFileMutation`, `OutsideWorkspace`, and critical-risk denial rules.
- Preserve canonical `GitEnvPolicy` and audit-safe persistence on all promoted paths.
- Ensure a routing failure does not execute a command twice.
- Ensure fallback provenance records the executor that actually ran.

### 2.3 Maintenance goals

- Avoid reintroducing a separate Git mutation executor inside `BashTool`.
- Keep routing policy declarative and family-based.
- Establish measurable triggers before splitting large parser, renderer, service, or tool modules.
- Add property/fuzz tests only where they produce clear incremental value.

---

## 3. Non-goals

This follow-up does not:

- change human shell semantics;
- route compound shell pipelines through the Git executor;
- auto-resolve Git conflicts;
- add worktree mutation management;
- add submodule mutation delegation;
- add typed `git bisect` or `git am` recovery;
- redesign the complete command-intent configuration system;
- rewrite the Git parser or renderer;
- split large files solely because of line count;
- change default permission policy for dangerous operations;
- add credential storage or authentication management.

---

# Track U — Unified Bash Mutation Routing

## U1. Establish the routing-family model

### Problem

`CommandIntentKind::GitMutating` currently has no family mapping in `intent_kind_to_family()`. Read-only Git has `CommandIntentFamily::GitRead`, while network and destructive routing have separate configuration fields but do not provide a complete local-mutation family path.

### Required design

Introduce explicit command-intent families that distinguish operation policy rather than relying on one broad mutation bucket:

```rust
pub enum CommandIntentFamily {
    // existing families...
    GitRead,
    GitLocalMutation,
    GitNetwork,
    GitDestructive,
}
```

Alternative naming is acceptable, but the semantic split must remain.

### Family assignment

Assign the family from the typed operation and `RiskSet`, not from raw string prefixes:

| Operation class | Family |
|---|---|
| status, diff, log, show, blame, refs | `GitRead` |
| add, unstage, commit, branch create/switch, restore, stash, merge, rebase, cherry-pick, revert | `GitLocalMutation` |
| fetch, pull, push, remote network action | `GitNetwork` |
| reset hard/merge/keep, clean, force push, destructive branch deletion | `GitDestructive` |
| repository-local config mutation | `GitLocalMutation` or dedicated config family if already justified |
| unsupported/ambiguous | no active family; conservative fallback |

If an operation carries multiple classes, select the highest-risk family:

```text
GitDestructive > GitNetwork > GitLocalMutation > GitRead
```

For example, force push must be `GitDestructive`, not merely `GitNetwork`.

### Deliverables

- Add family variants to configuration schema.
- Add a central helper such as:

```rust
fn git_operation_family(operation: &GitOperation, risks: &RiskSet)
    -> Option<CommandIntentFamily>
```

- Update `intent_kind_to_family()` to use typed Git request metadata.
- Remove any raw command-prefix family selection for active routing.
- Add exhaustive table-driven tests covering every `GitOperation` variant.

### Acceptance criteria

- Every typed Git operation maps deterministically to one family or explicit no-route result.
- A new `GitOperation` variant causes an exhaustive match failure or test failure until classified.
- Force-with-lease and plain force push are classified differently where policy requires it.

---

## U2. Add configuration controls and safe defaults

### Configuration

Extend `CommandIntentConfig` with a local mutation route setting:

```rust
pub route_git_local_mutation: Option<RouteLevel>,
```

Retain or reconcile existing fields:

```rust
pub route_git_network: Option<RouteLevel>,
pub route_git_destructive: Option<RouteLevel>,
```

Use the repository’s existing `RouteLevel` semantics:

- `off`: classify and record only through existing general observation, but never promote;
- `observe`: compute the intended Git route and metadata, execute the current fallback path;
- `active`: promote only after all active-routing validation and permission checks pass.

### Defaults

Recommended defaults:

| Family | Default |
|---|---|
| Git read | existing behavior |
| Git local mutation | `observe` initially, then eligible for future default `active` after validation period |
| Git network | `off` |
| Git destructive | `off` |

Do not make network or destructive routing active by default in this pass.

### Configuration compatibility

- Existing configurations must deserialize without changes.
- Missing fields must preserve conservative behavior.
- Add schema and default-value tests.
- Update example configuration and architecture documentation.

### Acceptance criteria

- No existing user receives newly active Bash mutation routing merely by upgrading unless current configuration semantics already imply it.
- Observe mode emits enough metadata to compare planned Git routing with actual shell fallback.

---

## U3. Preserve typed Git requests through Bash planning

### Required invariant

A promoted Bash-originated mutation must use the same `GitExecutionRequest` shape as a native Git tool operation:

```rust
GitExecutionRequest {
    operation,
    argv,
    origin: GitCommandOrigin::BashTranslation,
    risks,
    repo_root,
}
```

Do not reconstruct operations from command strings after planning.

### Work

- Ensure classification stores parsed `GitOperation`, canonical argv, risk set, and origin.
- Ensure planning does not lose repository root or current working directory.
- Ensure routing passes the complete request to `RouteToGit`.
- Ensure execution calls `GitExecutionService` / mutation executor rather than a new Bash-specific helper.
- Remove whitespace-splitting fallback for active Git mutation promotion.

### Parse requirements

Promotion requires:

- `SimpleArgv` shell shape;
- successful `parse_git_argv()`;
- high confidence;
- resolved repository root inside policy scope;
- no shell operators, variable assignment prefix, substitution, redirect, newline, or NUL;
- operation allowed by the selected route level;
- all permissions resolved before dispatch.

### Acceptance criteria

- Native and Bash-originated equivalents produce equal typed operations and risk sets.
- No active Git route uses `command.split_whitespace()`.
- Quoted commit messages and paths with spaces survive argv parsing exactly.

---

## U4. Permission and preflight parity

### Permission generation

Use typed risk classes to produce operation-specific permission requests.

Required defaults remain:

| Operation | Default |
|---|---|
| explicit `git add <paths>` | Allow or current configured policy |
| `git add -A` / stage all | Ask |
| commit / amend | Ask |
| switch / checkout branch | Ask |
| restore worktree | Ask |
| stash push/apply/pop | Ask |
| merge/rebase/cherry-pick/revert | Ask |
| fetch | Ask for network |
| pull | Ask for network + mutation |
| push | Ask for network write |
| force-with-lease | strong confirmation / destructive family |
| plain force push | Deny by default |
| reset hard / clean | Deny by default |

### State-aware prompts

Where repository snapshots are available, permission descriptions should include:

- active branch;
- dirty/clean state;
- number of staged and unstaged files;
- target branch/ref;
- remote/refspec;
- whether hooks may run;
- whether the operation may overwrite worktree state;
- whether the operation is networked or destructive.

### Preflight behavior

- Reuse existing `PreflightService` and mutation executor checks.
- Do not duplicate permission resolution in BashTool.
- If permission is unresolved in active mode, do not promote.
- A denied operation must not be retried through raw shell.

### Critical acceptance criterion

A denied or rejected Git route must yield a rejection result, not silently fall back to executing the same command through raw shell.

Fallback is permitted only for unsupported routing, not for denied policy.

---

## U5. Execution and fallback semantics

### Execution tiers

Maintain three explicit tiers:

1. **Typed Git** — fully parsed and supported operation.
2. **Managed Git argv** — simple argv, Git-owned environment and provenance, unsupported typed feature.
3. **Raw shell** — shell-owned syntax or non-promotable shape.

### Required routing outcomes

| Input | Outcome |
|---|---|
| `git status` | typed Git read |
| `git add src/lib.rs` | typed Git local mutation when active and permitted |
| `git commit -m "fix parser"` | typed Git local mutation when active and permitted |
| `git stash list` | typed Git read |
| `git stash push -m save` | typed Git local mutation |
| `git fetch origin` | typed Git network only when configured |
| `git push --force origin main` | destructive route; denied by default |
| `git rev-list --left-right main...HEAD` | managed Git argv if unsupported by typed parser |
| `git diff | less` | raw shell |
| `git status && git log -1` | raw shell |
| malformed quoting | rejection or raw shell according to current shell policy; never lossy argv promotion |

### No-double-execution invariant

The routing dispatcher must produce one terminal execution decision. If typed execution starts and returns an error, BashTool must return that error/result and must not run the original shell command.

Add explicit tests with marker files or counters proving the child is invoked once.

### Environment and persistence

All typed and managed Git executions must use:

- canonical `codegg_git::process_policy`;
- noninteractive settings;
- audit-safe argv;
- URL/text redaction;
- `PlannedBackend::Git` / `ActualBackend::Git` where Git owns execution;
- correct `RunOwnership::DelegatedBackend` semantics.

Raw shell expressions must retain `ActualBackend::RawShell`.

---

## U6. Output projection and RunStore parity

### Projection

Bash-originated mutations should use the existing Git mutation projectors:

- local mutation projection;
- network mutation projection;
- destructive mutation projection;
- conflict/recovery projection.

Do not pass raw Git output through the generic shell truncator when a typed result exists.

### RunStore

Persist the same structured metadata as native Git tool calls:

- operation kind;
- command origin;
- planned and actual backend;
- risk classes;
- repository root;
- state before/after;
- state delta;
- permission outcome;
- timeout/cancellation status;
- redacted invocation and result output.

### Origin visibility

Retain origin metadata so audits can distinguish:

```text
NativeTool
BashTranslation
ManagedGitFallback
TuiAction
DaemonAction
Replay
```

Origin must not change behavior after the operation reaches the executor, except where user-facing context is useful.

### Acceptance criteria

- Equivalent native and Bash operations differ only in origin metadata and model/tool call envelope.
- Secret sentinel tests pass through Bash promotion and native execution.
- Conflict results contain the same recovery guidance for both origins.

---

## U7. Observe-mode rollout and metrics

Before making local mutation routing active by default, add or confirm metrics for:

- eligible simple Git mutations observed;
- operations promoted;
- operations not promoted and reason;
- permission asks, allows, denials;
- typed parse failures;
- managed fallback counts;
- raw shell fallback counts;
- typed-vs-fallback outcome disagreement;
- timeout/cancellation frequency;
- conflict frequency;
- projection truncation frequency.

Do not persist command secrets in metrics labels.

### Shadow comparison

In observe mode:

- classify and plan the typed Git route;
- execute through the existing actual path;
- compare only non-invasive metadata;
- do not execute a second shadow command;
- record whether the typed route would have been eligible.

### Rollout sequence

1. Land family/configuration model with default `off` or `observe`.
2. Run full tests and local manual matrix.
3. Enable `observe` in development configuration.
4. Review routing metrics and mismatch logs.
5. Enable `active` for local mutation in development.
6. Keep network/destructive off.
7. Consider changing defaults only in a separate decision after evidence.

---

## U8. Test plan

### Unit tests

- operation-to-family exhaustive mapping;
- family risk precedence;
- route-level defaults;
- simple mutation planning;
- permission generation;
- denial does not fallback;
- unsupported typed operation selects managed Git argv;
- complex shell selects raw shell;
- repository root preservation;
- timeout propagation;
- origin metadata preservation.

### Integration tests

Extend `tests/git_execution_origin_matrix.rs` with at least:

1. Bash `git add path` promoted to Git.
2. Bash commit promoted after permission allow.
3. Bash commit denied and not executed.
4. Bash branch switch promoted and state delta recorded.
5. Bash stash push promoted.
6. Bash merge conflict returns typed conflict result.
7. Bash fetch remains off by default.
8. Bash fetch routes when network route is active and permission granted.
9. Bash force push remains denied.
10. Bash reset hard remains denied.
11. Unsupported simple plumbing command uses managed Git argv.
12. Compound shell expression remains raw shell.
13. Quoted path with spaces survives.
14. Quoted commit message survives.
15. Child is executed exactly once on typed failure.
16. cancellation kills promoted Git child.
17. RunStore planned/actual backend agreement.
18. audit-safe argv contains no credential sentinel.
19. tracing contains no credential sentinel.
20. native-vs-Bash output projection equivalence.

### Adversarial tests

- option-looking filenames after `--`;
- filenames containing whitespace, newlines, glob characters, Unicode, and leading colon;
- malformed shell quoting;
- hostile environment variables;
- nested repository and symlinked cwd;
- detached HEAD and unborn branch;
- in-progress merge/rebase state;
- hooks that fail;
- hook attempts to prompt;
- repository changes between planning and execution;
- operation becomes destructive after ref/state movement;
- large stderr and large diff projection.

### Static checks

Extend `scripts/check_git_forbidden_patterns.py` to enforce:

- `GitMutating` has an explicit family mapping;
- no Bash-specific direct `Command::new("git")` path;
- no denied Git route falls back to shell execution;
- no active Git promotion uses whitespace splitting;
- `RouteToGit` remains the only promoted Git execution route.

---

## U9. Documentation and handoff

Update:

- `architecture/command_intent.md`;
- `architecture/command_planner.md`;
- `architecture/command_routing.md`;
- `architecture/git.md`;
- `architecture/git_polish_verification_handoff.md`;
- `AGENTS.md`;
- configuration reference and examples.

Create final handoff artifact:

```text
architecture/git_unified_bash_routing_handoff.md
```

It should contain:

- final family and route-level matrix;
- exact promotion criteria;
- permission defaults;
- fallback rules;
- native/Bash equivalence matrix;
- known limitations;
- rollout status;
- validation commands;
- commit references.

---

# Track M — Future Maintenance

Track M is not a prerequisite for unified Bash routing. It should be executed only when the trigger conditions below are met or when a contributor is already modifying the relevant module substantially.

## M1. Large-module decomposition triggers

Do not split files solely because of line count. Start a decomposition when one or more of these conditions occurs:

- repeated merge conflicts in the same module across three or more feature branches;
- code-review latency materially higher than comparable modules;
- a new feature requires touching unrelated operation families in the same file;
- test ownership is unclear;
- compile times are materially affected by monolithic test modules;
- contributors repeatedly introduce cross-family regressions;
- navigation or ownership feedback is documented in issues/reviews.

Record the evidence in the future plan or PR.

---

## M2. Parser decomposition

### Current candidate

`crates/codegg-git/src/parser.rs`

### Target shape

```text
parser/
  mod.rs
  common.rs
  read.rs
  staging.rs
  branch.rs
  history.rs
  stash.rs
  network.rs
  config.rs
  destructive.rs
  plumbing.rs
```

### Constraints

- preserve public `parse_git_argv()` API;
- preserve exact parse errors and risk classifications;
- preserve deterministic ordering and option precedence;
- no behavior changes in the split commit;
- move tests with their operation family where practical;
- add a top-level cross-family regression module.

### Verification

- parser golden tests before and after;
- complete parse/render round-trip suite;
- diff generated operation snapshots;
- no changes to serialized representations.

---

## M3. Renderer decomposition

### Current candidate

`crates/codegg-git/src/render.rs`

### Target shape

Mirror parser families where practical:

```text
render/
  mod.rs
  read.rs
  staging.rs
  branch.rs
  history.rs
  network.rs
  destructive.rs
```

### Security invariant

`RedactedUrl::expose_secret()` must remain restricted to one clearly documented execution-boundary module. If rendering is split, use a private helper in `render/mod.rs` so multiple family modules do not gain independent secret escape hatches.

Extend the forbidden-pattern script accordingly.

---

## M4. Git tool decomposition

### Current candidate

`src/tool/git.rs`

### Target shape

```text
src/tool/git/
  mod.rs
  schema.rs
  read.rs
  mutation.rs
  network.rs
  destructive.rs
  recovery.rs
  raw.rs
  errors.rs
```

### Constraints

- public tool name and schema remain stable;
- schema snapshot tests remain authoritative;
- dispatcher remains thin;
- operation execution stays in Git services/executors, not tool modules;
- no duplicate permission or policy logic.

---

## M5. Git service decomposition

### Candidate

`src/git_service.rs`

Split only if structured read growth continues. Suggested boundaries:

```text
src/git_service/
  mod.rs
  status.rs
  diff.rs
  history.rs
  refs.rs
  fallback.rs
  payload.rs
```

Keep `egggit` as the read-only fact provider and prevent mutation logic from migrating into this service.

---

## M6. Property and fuzz testing

### Trigger conditions

Add property/fuzz infrastructure when:

- parser or renderer behavior changes materially;
- new pathspec/refspec syntax is introduced;
- a redaction bypass is discovered;
- a security review requests stronger generative coverage;
- CI budget allows bounded scheduled fuzz jobs.

### High-value properties

1. `RedactedUrl` Debug/Display/Serialize never contains userinfo secrets.
2. `AuditSafeArgv` never serializes credential-bearing URL userinfo.
3. Redaction is idempotent.
4. Redaction preserves non-secret URL host/path data.
5. `parse(render(op))` is equivalent for supported operations.
6. `render(parse(argv))` preserves semantic argv for canonical forms.
7. Parser never panics on arbitrary byte-valid strings.
8. Path/ref constructors reject invalid forms without panics.
9. Risk classification is monotonic when destructive flags are added.
10. Sanitization and truncation remain linear within defined bounds.

### Suggested tooling

- `proptest` for deterministic CI properties;
- `cargo-fuzz` or `honggfuzz` for scheduled/manual fuzzing;
- seed corpus built from existing parser and adversarial fixtures.

Do not place unbounded fuzzing in the normal pull-request test path.

---

## M7. Windows and cross-platform completion

Current Unix-focused adversarial tests are acceptable, but future Windows support should add:

- `git.exe` discovery and `PATHEXT` tests;
- `USERPROFILE`, `APPDATA`, and Windows Git config path policy;
- named-pipe SSH agent behavior;
- process-tree cancellation on Windows;
- editor/askpass marker tests using PowerShell or `cmd.exe`;
- path normalization and drive-letter repository roots;
- symlink/junction behavior;
- CRLF-sensitive diff/status fixtures.

Any Windows-specific environment additions must be centralized in `codegg_git::process_policy`, not copied into callers.

---

## M8. Performance and resource monitoring

Retain the existing performance scripts and add regression thresholds only where measurements are stable.

Monitor:

- command classification latency;
- typed parser latency for long argv;
- large diff parsing and projection;
- status_v2 on large repositories;
- URL redaction and argv sanitization scaling;
- RunStore persistence size and latency;
- cancellation cleanup time;
- Bash routing overhead relative to direct native execution.

Avoid brittle microbenchmark gates on shared CI. Prefer documented baselines and alert thresholds with generous variance.

---

## M9. Deprecation and compatibility cleanup

Periodically review:

- deprecated RunStore Git backend variants retained for serialization;
- legacy status APIs superseded by structured status;
- raw Git tool compatibility actions that now have typed equivalents;
- managed Git argv fallbacks that can be promoted safely;
- stale documentation references to removed `GitSession` behavior.

Removal requires:

- migration documentation;
- compatibility window;
- serialization tests for historical records;
- explicit release-note entry.

---

# 4. Recommended implementation sequence

## Commit 1 — Family model and configuration

- add `GitLocalMutation` family;
- central operation-to-family classification;
- add route configuration and defaults;
- exhaustive unit tests;
- documentation skeleton.

## Commit 2 — Bash promotion path

- wire family lookup;
- preserve typed request through planner/router;
- route eligible local mutations to `RouteToGit`;
- enforce no whitespace fallback;
- add no-double-execution guard.

## Commit 3 — Permission and provenance parity

- operation-specific permission descriptions;
- RunStore origin/backend parity;
- native-vs-Bash equivalence tests;
- denial-does-not-fallback tests.

## Commit 4 — Network/destructive gates

- verify separate route levels;
- keep defaults off;
- add force/reset/clean routing tests;
- add credential and environment sentinel coverage.

## Commit 5 — Observe-mode metrics and rollout docs

- routing reason metrics;
- observe-mode comparison metadata;
- configuration examples;
- rollout guidance.

## Commit 6 — Closure verification and handoff

- extend execution-origin matrix;
- static checks;
- full focused and workspace validation;
- final handoff artifact;
- update broader roadmap status.

Track M decomposition or fuzz work should use separate commits and preferably a separate plan when its triggers are met.

---

# 5. Validation commands

Focused validation:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-features
cargo clippy -p codegg-git --all-targets --all-features
cargo clippy -p codegg-core --all-targets --all-features
cargo clippy -p codegg --lib --tests --all-features

cargo test -p codegg-git
cargo test -p codegg-core
cargo test -p egggit
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib tool::git
cargo test -p codegg --lib git_mutations
cargo test -p codegg --lib git_service

cargo test --test git_execution_origin_matrix
cargo test --test command_routing_execution_ownership
cargo test --test command_routing_adversarial
cargo test --test git_mutations_integration
cargo test --test git_network_integration
cargo test --test git_recovery_integration
cargo test --test git_credential_cross_path
cargo test --test git_credential_runstore_sentinel
cargo test --test git_env_attack
cargo test --test git_noninteractive
cargo test --test git_tracing_capture
cargo test --test git_closure_matrix

python3 scripts/check_git_forbidden_patterns.py
```

Full workspace validation:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=8
```

Where full validation is split due to resource limits, record every invocation and result in the handoff artifact.

---

# 6. Definition of done

Track U is complete when all of the following are true:

1. Simple Bash-originated local Git mutations can route through `RouteToGit` when configured active.
2. Native and Bash-originated equivalent operations share the same typed executor, environment policy, permissions, projection, and RunStore semantics.
3. Complex shell expressions remain raw-shell owned.
4. Network and destructive families remain independently gated and conservatively disabled by default.
5. Denied operations never execute through fallback.
6. Typed execution failures never trigger duplicate shell execution.
7. Active promotion never uses whitespace splitting.
8. Planned and actual backend provenance is correct for every execution origin.
9. Credential, environment, timeout, cancellation, conflict, and state-race tests pass.
10. Static checks prevent reintroduction of Bash-specific Git subprocess paths or unsafe fallbacks.
11. Observe-mode rollout evidence is documented.
12. `architecture/git_unified_bash_routing_handoff.md` records the final state.
13. The broader Git Agent Integration roadmap is marked complete with only explicitly deferred maintenance items remaining.

Track M is not required for Track U closure. Deferred maintenance work is considered properly managed when:

- trigger conditions are documented;
- ownership boundaries remain clear;
- static and regression checks remain green;
- no high- or medium-severity finding depends on the deferred refactor or fuzz work.

---

## Final expected state

After Track U, Codegg will have literal execution convergence for eligible Git commands from both agent-native and Bash-translated origins:

```text
Native Git tool ───────┐
                       ├─> GitOperation ─> policy/permission ─> RouteToGit
Bash SimpleArgv git ───┘                                      │
                                                              v
                                                  shared Git executor
                                                              │
                                         structured result + projection
                                                              │
                                               audit-safe RunStore
```

Only shell-owned syntax, unsupported compatibility commands, or policy-denied operations remain outside that shared route. Future maintenance work can then proceed independently based on measured need rather than blocking roadmap closure.
