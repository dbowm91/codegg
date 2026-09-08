# Post-Audit Maintainability and Surface Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/004-bash-tool-physical-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Repository baseline reviewed: `910f58f9` (pre-change HEAD; M001/M002/M003 closed)

Implementation commits:

- (this closure) feat(tools): decompose bash tool into policy/process/output modules (maintainability M004)

## 1. Executive finding

M004 is complete. `src/tool/bash.rs` fell from 3217 lines (~126 KiB) to
1115 lines (~43 KiB): the file is now the `BashTool` configuration facade
plus the auditable five-step `execute` sequence
(parse/classify → authorize/path/child policy → route → collect/persist →
project/return). Three narrowly named modules carry the moved production
code with their focused tests, following the M003 `impl`-across-files
precedent. No second shell executor, sandbox policy, command router,
scheduler path, permission UX, or protocol/DTO change was introduced.
`BashTool` remains the single model-facing `bash` owner with identical
tool name/input schema/output contract; permission, destructive-command,
workspace/sensitive-path, child-Git, scheduler-admission, timeout/
cancellation/process-tree, output-cap, projection/redaction, and
run/artifact persistence semantics are unchanged. No downstream plan is
unblocked by this closure: M005 was already `ready` (M002 hard + M003
interface dependencies both closed before this milestone began) and is
independent of the Bash file layout.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Responsibility map before movement, no duplicate owners | Section 3 map; `destructive`, scheduler, shell/projector, sandbox, command-intent modules invoked, never copied | pass |
| `BashTool` sequencing remains auditable and canonical | Facade header documents the 5-step sequence; `execute` reads top-to-bottom with no moved helper left inline | pass |
| Policy/classification independently testable | `bash/policy.rs` + 40 moved policy unit tests (patterns, heredoc, allowlist, child ceiling, classify/plan/resolve, kill switches) | pass |
| Supervised execution independently testable | `bash/process.rs` + env-policy/cwd-extraction/synth unit tests; dispatch paths covered by execute-level routing tests | pass |
| Bounded output/result persistence independently testable | `bash/output.rs` + truncation/suffix/backend unit tests; persistence precedence covered by ownership integration tests | pass |
| All pre-spawn checks still precede spawn | Policy chain order preserved verbatim in facade; `pre_spawn_order_rejects_blocked_command_before_any_dispatch` regression test added | pass |
| Timeout/cancel/reap/output-cap preserved | No `Command::new`/`.output()`/unbounded read introduced (execution-ownership guard extended to new files); managed/shell/preflight suites green | pass |
| Scheduler/run-store ownership unchanged | Scheduler submission call sites moved verbatim; `command_routing_execution_ownership` (21) + `git_execution_origin_matrix` (28) green | pass |
| No duplicate executor/sandbox/router | `rg` for new `Command::new`, `SandboxRequest`, `plan_execution` definitions in new modules: none (all calls resolve to existing owners) | pass |
| Docs reflect final ownership; no size gate added | `architecture/tool.md` module table rewritten; guard/manifest coverage extended, not weakened; no CI gate added | pass |

## 3. Production implementation evidence

### 3.1 Responsibility map (WP-A) and extraction units

Pre-extraction inventory of `src/tool/bash.rs` (3217 lines):

| Cluster | Previous home | New owner (one sentence) |
|---|---|---|
| `BashTool` config/builders/`Default`, `Tool` facade, 5-step `execute` | `bash.rs` full file | `tool/bash.rs` — facade only; fields widened private→`pub(crate)` so descendant modules share one struct without accessors |
| Blocked-pattern table/regexes, heredoc sanitizer, risk caps, `RoutingMetric`, intent/plan family adapters, `plan_to_planned_backend`, kill switches, `check_command_security`, child-workspace validator | `bash.rs` top + `impl BashTool` | `tool/bash/policy.rs` — pure decision-making; spawns/submits nothing |
| `DispatchOutcome`, env policy, `-C` cwd recovery, `synth_output`, raw-shell spawn, test/native/python/git/managed/shell dispatch, `dispatch_command_target` | `bash.rs` `impl BashTool` | `tool/bash/process.rs` — process lifetime + scheduler-boundary translation; sole local spawn path is `ManagedProcessService::run` |
| `RoutingMetadata`, `truncate_output`, metadata builder/suffix, `execution_outcome_clone_actual`, caller-owned persistence + ownership decision | `bash.rs` `execute` tail + free fns | `tool/bash/output.rs` — bounded capture/projection/persistence/result shaping |
| Policy unit tests (patterns, heredoc, allowlist, child ceiling, classify/plan, kill switches, family mapping) | `bash.rs` tests | `tool/bash/policy.rs` tests (verbatim move + `field_reassign` allow matching the facade) |
| Process/output unit tests (env policy, `-C` forms, synth shape, truncation, suffix, backend mapping) | new | `tool/bash/process.rs` + `tool/bash/output.rs` tests (additive; one corrected expectation, see §10) |
| Execute/routing integration tests + builder contract + pre-spawn regression guard | `bash.rs` tests | `tool/bash.rs` tests (verbatim except two additive guards) |

Deliberately retained in the facade: the full `execute` orchestration
(~350 lines — sequencing, not a reusable responsibility), all 15 builder
methods, and the `validate_child_workspace_command` re-export path
(`terminal.rs` imports `crate::tool::bash::validate_child_workspace_command`
unchanged).

### 3.2 Before/after sizes (descriptive only)

`bash.rs` 3217→1115 lines (~126 KiB→~43 KiB). New: `policy.rs` 834
(40 policy unit tests), `process.rs` 1121 (3 supervision unit tests),
`output.rs` 441 (5 output unit tests). Total 3511 lines: the increase is
module headers documenting the pre-spawn order and child-ownership
contract plus 10 additive unit/regression tests, not duplicated logic.

### 3.3 No-bypass evidence

- `rg` for new `Command::new`, `plan_execution`, `SandboxRequest`,
  `JobScheduler::submit` definitions in `tool/bash/`: all such tokens
  resolve to pre-existing owners (`managed_process`, `command_intent`,
  `scheduler`); the new modules define no executor, classifier, or
  sandbox constructor.
- `scripts/check_execution_ownership.py` extended (`CANONICAL_FINITE_PATHS`
  + `TYPED_ARGV_PATHS` cover `tool/bash/*.rs`); guard ok.
- `scripts/check_sandbox_contract.py` scans `tool/bash/process.rs` (new
  sandbox call-site home); guard passed.
- `scripts/check_daemon_cwd_usage.py` protects `tool/bash/*.rs`; passed.
- `check_scheduler_bypass.py`, `check_tool_broker_boundary.py`,
  `check_git_forbidden_patterns.py`: all passed.

## 4. Verification executed (commands + results; local unless noted)

- `cargo check -p codegg --all-targets` — clean, zero warnings.
- `cargo test -p codegg --lib tool::bash` — 87 passed (68 pre-split
  execute/policy tests preserved + 19 moved/additive policy/process/
  output unit tests).
- `cargo test -p codegg --lib tool::` — 546 passed (M003 count 534 + 12
  new bash-module tests; no regression).
- `cargo test -p codegg --lib tool::destructive` — 6 passed.
- `cargo test -p codegg --lib shell::` — 379 passed
  (timeout/cancel/reader termination paths).
- `cargo test -p codegg --lib scheduler::` — 71 passed.
- `cargo test -p codegg --lib managed_process` — 13 passed.
- `cargo test --test command_routing_execution_ownership` — 21 passed
  (one-execution-one-record, delegated-ownership, attacker-matrix).
- `cargo test --test git_execution_origin_matrix` — 28 passed (Track U
  bash→git routing origins).
- `cargo test --test tool_execution` — 54 passed (incl. bash timeout
  override).
- `cargo test --test preflight_integration` — 71 passed (bash preflight
  block/warn path).
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  — clean (one justified `too_many_arguments` allow on the extracted
  persistence helper per repo precedent; one `needless_borrow` fixed, not
  suppressed).
- `scripts/verify.sh quick` — passed (generated-agents check, core
  boundary, sandbox contract, execution ownership, locked workspace
  check).
- No hosted `CI / verify` run: behavior-preserving source reorganization
  with no daemon, scheduler, protocol, or release change; local quick +
  all-feature Clippy is the proportionate posture per the roadmap (same
  basis as M001/M002/M003).

## 5. Invariant review

- `BashTool` canonical; tool name/schema/category unchanged.
- Pre-spawn chain order byte-identical in effect: length → blocked/
  allowlist/patterns → preflight → child ceiling → allowed_paths →
  classify/plan → spawn/submit. Security checks remain deterministic and
  pre-spawn; extraction only relocated them.
- Scheduler admission authoritative: all six submission call sites moved
  verbatim; active-route failure still returns without raw-shell retry
  (no-double-execution invariant intact).
- Child-Git ceiling narrower than parent authority: validator moved
  verbatim; bypass-by-spelling regression test added.
- Bounded capture: `OutputPolicy` limits and `truncate_output` moved
  verbatim; no unbounded accumulation introduced.
- Run/artifact attribution: `RunDraft` construction moved verbatim into
  `output.rs`; delegated-run-id suppression rule unchanged; persistence
  failure still cannot rewrite the terminal outcome.
- Env/cwd construction explicit: `bash_environment_policy` and
  canonical-workdir resolution moved verbatim.
- No second parser as security authority: `find_blocked_pattern` moved,
  not forked; canonical classifier remains `command_intent`.

## 6. Failure and recovery review

No new tasks, locks, stores, or recovery paths. No `tokio::spawn` site
was added or moved (the facade's `spawn_blocking` workdir resolution
stays inline in `execute`). Reader/supervisor lifetimes unchanged:
`ManagedProcessService::run` still owns join/cancellation. Timeout maps
to `ToolError::Timeout` after reap in all three direct paths
(raw shell, native tool, git managed argv). Scheduler `wait_for_completion`
timeouts propagate as execution errors without fallback execution.
Restart semantics unchanged: in-flight local processes are not resumable;
durable scheduler/run-store behavior untouched.

## 7. Migration and compatibility review

No user migration. Tool name (`bash`), input schema
(`command`/`workdir`/`timeout`), output contract (result text + exit-code
annotation + optional routing suffix), and provenance shapes unchanged.
Internal paths added (`tool::bash::{policy,process,output}`); documented
`crate::tool::bash::{BashTool, DispatchOutcome,
validate_child_workspace_command}` paths preserved via re-export. The
`execution-ownership.toml` manifest gained one additive `process.rs`
site (guard-verified); the `bash.rs` entry reason now names the facade
role. No config or persisted-data migration.

## 8. Security review

- Blocked-pattern table, blocked-command set, allowlist logic, heredoc
  sanitizer, and child-workspace validator moved byte-identically (only
  visibility `fn`→`pub(crate)` for cross-module calls within the same
  `bash` tree).
- Permission/destructive negative tests green (policy unit tests +
  `command_routing_execution_ownership` attacker matrix + destructive 6).
- Preflight block/warn ordering preserved (block returns before any
  policy/path work beyond the security check; 71 integration tests green).
- Sandbox hooks preserved: `SandboxRequest::Required` construction moved
  verbatim to `process.rs`; sandbox contract guard extended to scan it.
- Auth logging, credential handling untouched.

## 9. Documentation and operations

- `architecture/tool.md`: file tree + bash row rewritten for the four
  responsibility owners; all other sections unchanged and still accurate.
- `docs/execution-ownership.toml`: additive `process.rs` scheduler site;
  `bash.rs` reason narrowed to the facade role. No existing entry changed
  semantics.
- Guard scripts: finite-process/argv-boundary coverage, sandbox file
  list, and daemon-cwd globs extended to the new layout (additive only).
- `AGENTS.md`: no edit — it does not name `bash.rs` as a source-layout
  location (same basis as the M003 closure).
- No user README/operator change: no behavior or public-path change.

## 10. Unresolved findings

No critical/high/medium/low findings requiring a corrective pass. Notes
(intentional, recorded per plan §15):

1. `process.rs` (1121 lines) is the largest new module because the six
   scheduler-submission translators (test/python/shell/managed ×
   request-building) moved verbatim. They stay together because they
   share one reason to change (the `JobSubmissionService` translation
   boundary); splitting per-backend now would create pass-through pairs.
2. `persist_caller_run` carries `#[allow(clippy::too_many_arguments)]`
   per established repo precedent (10+ existing sites). A params struct
   was considered and rejected: it would rename the `RunDraft` fields at
   every call site for no behavior gain.
3. One new unit-test expectation was corrected during implementation:
   `synth_output` packs its integer via `ExitStatus::from_raw`, so only
   the zero status round-trips through `.code()` portably. The test now
   asserts streams plus the zero-code path; nonzero terminal status
   remains covered by the `[exit code: N]` result-string tests. No
   production code was changed for this.
4. Remaining concentration: the facade `execute` (~350 lines) stays
   inline because it is orchestration sequencing, matching the M003
   `run_inner` disposition.

## 11. Roadmap disposition

M004 meets all exit conditions in
`plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`:
command policy/classification, supervised execution/cancellation, and
bounded output/result persistence are independently testable;
`BashTool` remains the single model-facing owner; no second process
executor, sandbox policy, or scheduler path was introduced.

Dependency audit (per planning-skill unblock check): M005
(`005-runtime-service-context-global-state-cleanup.md`) lists a hard
dependency on M002 (closed) and an interface dependency on M003's final
agent/tool construction seams (closed). Both were satisfied before M004
began — M004 is independent per the roadmap dependency graph
(`M004 is independent of M001–M003 except for ordinary shared
verification`) and touches none of M005's seams (`ToolRegistryOptions`,
search/MCP service state). No registered plan lists M004 as a hard or
interface dependency. No new follow-up or corrective plan is required.

Recommendation: closed; M005 remains ready (unchanged).

## 12. Registry updates

- `plans/registry.md`: M004 moved to closed with this closure record;
  subsystem row updated to `M001–M004 closed; M005 ready`; M004 removed
  from the dependency-ready table (M005 + provider-auth M010 retained).
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`:
  M004 → closed with closure link.
- `plans/implementation/post-audit-maintainability-surface/004-bash-tool-physical-decomposition.md`:
  status → closed.
- `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md`:
  status unchanged (`ready for handoff`; dependency audit confirms no
  M004 dependency — see §11).
