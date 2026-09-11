# Tool Programs — Domain, Storage, and Execution

Status: fully implemented (M006–M020 all closed; M019 strict closure
and M020 corrective disposition accepted via `plans/closure/tool-programs/`)

## Purpose

Tool Programs introduce a durable, versioned domain model for
agent-submitted bounded programs that invoke approved CodeGG tools
through deterministic control flow. Programs are compiled to a
safe IR, executed in a metered interpreter, and produce typed
results with full crash-recovery support.

## Where It Lives

| Path | Purpose |
|------|---------|
| `crates/codegg-core/src/tool_program/` | Domain types, IR, compiler, verifier, interpreter, diagnostics, store, guards |
| `src/tool/tool_program.rs` | Model-facing `tool_program` tool (DirectOnly, foreground/background) |
| `src/tool/program_manifest.rs` | Manifest resolution — tool eligibility gating |
| `src/tool/program_cache.rs` | Read-only call cache with content/policy-aware keys |
| `src/tool/tool_program_ledger.rs` | Call ledger persistence |
| `src/tool/tool_program_source.rs` | Durable source persistence under `.codegg/tool_program_sources/` |
| `src/tool/tool_program_result.rs` | Typed program result store |
| `src/tool/tool_program_context.rs` | Execution context, authority digest, grant construction |
| `src/scheduler/tool_program_executor.rs` | Scheduler executor with `BrokerAdapter` for real pipeline |
| `src/scheduler/tool_program_notifications.rs` | Notification service for background programs |
| `tests/tool_program_*.rs` | Integration tests |
| `tests/hosted_tool_program_*.rs` | Hosted-adapter tests |

## How It Works

```text
Model submits tool_program(source, tools, ...)
    |
    v
ToolProgramTool::execute_impl()
    | 1. Validate source + tools non-empty
    | 2. compile_program(source) -> Compilation { ir, manifest }
    | 3. verify_ir_integrity(ir)
    | 4. Persist source under .codegg/tool_program_sources/
    | 5. Build authority grant and execution context
    | 6. Submit via JobSubmissionService
    |
    v
Scheduler admits job (JobKind::ToolProgram)
    |
    v
ToolProgramExecutor::execute()
    | 1. Validate payload fields
    | 2. Load/compile IR, verify integrity and digest
    | 3. Create MeteredInterpreter with RuntimeLimits
    | 4. Create BrokerAdapter (bridges BrokerCallback -> real ToolBroker)
    | 5. Interpreter.run_with_config(broker_adapter, cancellation, run_config)
    |
    v
BrokerAdapter::execute_call(request)
    | 1. Verify tool in frozen manifest
    | 2. Build BrokerInvocationContext (caller=Program, workspace, cwd)
    | 3. broker.execute(registry, tool_name, input, ctx)
    | 4. Map StructuredToolResult -> CallResult
    |
    v
MeteredInterpreter steps through IR
    |  - ExecuteCall -> BrokerAdapter -> ToolBroker -> real tool
    |  - CheckCache -> ProgramCallCache (skip broker on hit)
    |  - ExecuteChildJob -> BrokerAdapter -> JobSubmissionService
    |  - Emit -> ProgramResult (terminal)
    |
    v
ExecutorCompletion returned to scheduler
    |  - Typed result record persisted to ToolProgramResultStore
    |  - Notification dispatched for background programs
```

## Key Types & APIs

| Type | Location | Purpose |
|------|----------|---------|
| `ToolProgramId` | `crates/codegg-core/src/tool_program/mod.rs` | Opaque typed program ID |
| `ProgramCallId` | `crates/codegg-core/src/tool_program/mod.rs` | Opaque typed call ID |
| `ToolProgramState` | `crates/codegg-core/src/tool_program/mod.rs` | Lifecycle state machine |
| `ProgramLanguage` | `crates/codegg-core/src/tool_program/mod.rs` | `RestrictedPython` + forward-compatible unknown |
| `ProgramSourceRef` | `crates/codegg-core/src/tool_program/store.rs` | Content-addressed immutable source reference |
| `ProgramCapabilityManifest` | `crates/codegg-core/src/tool_program/mod.rs` | Frozen callable-tool contracts and authority digest |
| `ProgramCheckpoint` | `crates/codegg-core/src/tool_program/mod.rs` | Deterministic interpreter position for restart |
| `ProgramCallRecord` | `crates/codegg-core/src/tool_program/mod.rs` | Nested-call ledger entry |
| `ProgramResult` | `crates/codegg-core/src/tool_program/interpreter.rs:228` | Terminal type, value/artifacts, failure class, budget usage |
| `ProgramStatus` | `crates/codegg-core/src/tool_program/interpreter.rs:242` | `Completed` / `Failed` / `Cancelled` / `TimedOut` / `Stalled` / `Incomplete` / `Recoverable` |
| `FailureClass` | `crates/codegg-core/src/tool_program/interpreter.rs:169` | 13 classes: Validation, ManifestDrift, AuthorityNarrowed, SchemaMismatch, TransientBackend, Timeout, Stall, Cancelled, Storage, ReplayDivergence, BudgetExhausted, Execution, InternalPanic |
| `BrokerCallback` | `crates/codegg-core/src/tool_program/interpreter.rs:638` | Trait: `execute_call`, `submit_child_job`, `submit_child_job_with_checkpoint`, `heartbeat`, `call_reserved`, `call_completed`, `checkpoint` |
| `MeteredInterpreter` | `crates/codegg-core/src/tool_program/interpreter.rs` | Stack-machine evaluating verified IR with bounded budgets |
| `RuntimeLimits` | `crates/codegg-core/src/tool_program/interpreter.rs:359` | Derived from `IrBounds` + executor timeouts |
| `InterpreterCheckpoint` | `crates/codegg-core/src/tool_program/interpreter.rs:449` | Serializable state: PC, steps, iterations, locals, stack, pending child wait, semantic digest |
| `ProgramStore` | `crates/codegg-core/src/tool_program/store.rs:29` | Content-addressed IR store with cache key matching |
| `ToolProgramTool` | `src/tool/tool_program.rs:79` | Model-facing tool (DirectOnly, foreground/background) |
| `ToolProgramExecutor` | `src/scheduler/tool_program_executor.rs:119` | `JobExecutor` for `JobKind::ToolProgram` |
| `BrokerAdapter` | `src/scheduler/tool_program_executor.rs:181` | Bridges `BrokerCallback` to real `ToolBroker` |
| `ToolProgramNotificationService` | `src/scheduler/tool_program_notifications.rs` | Durable notification records with claim/ack semantics |

## Invariants

1. Program source and compiled IR are immutable and content-addressed
   (SHA-256).
2. A capability manifest is frozen at submission and cannot expand
   while running.
3. Nested-call arguments/results are bounded, redactable, and
   artifact-backed when large.
4. Storage does not contain credentials or hidden reasoning.
5. Unknown future variants remain inspectable but never execute under
   older code.
6. State transitions are intent-named and validated; generic arbitrary
   state mutation is prohibited.
7. Program storage cannot become a second scheduler or RunStore.
8. Intermediate tool call outputs do NOT enter the parent model
   transcript — only the final program result is projected.

## State Machine

```
Submitted -> Queued -> Compiling -> Running -> Waiting <-> Running
                                    |
                               RetryBackoff -> Running
                                    |
                 Queued <- Interrupted
                                    |
                        Terminal: Completed | Incomplete | Failed |
                                  Cancelled | TimedOut | Blocked
                 Stalled -> Running | Failed | TimedOut | Interrupted
```

Terminal states never regress. `ProgramStatus::Recoverable` is a
non-terminal state indicating transient retry-eligible failure.

## Call Ledger

Each nested call gets a `ProgramCallRecord` with:
- Monotonic `sequence` within the program
- Tool contract hash and normalized input hash for replay
- State machine: Reserved -> Running -> Completed/Failed/Cancelled/TimedOut
- Replay disposition: Replay (completed), Reexecute (non-idempotent),
  Skip (cancelled)

The `BrokerAdapter` implements durable crash-boundary hooks:
- `call_reserved()` — reservation before dispatch
- `call_completed()` — completion record accepted
- `checkpoint()` — interpreter state persisted

## Content-Addressed IR Store

`ProgramStore` (`store.rs`) provides:

- `digest_source(source)` — SHA-256 of source bytes
- `store_ir(source, ir)` — store IR after successful compilation
- `check_cache(source, manifest, limits)` — check for cached IR with matching key
- `get_ir(source)` / `contains_ir(source)` / `remove(source)` — retrieval and cleanup
- `serialize_ir(ir)` / `deserialize_ir(bytes)` — JSON round-trip
- `verify_ir_integrity(ir)` — digest consistency after deserialization

Thread-safe via `Arc<Mutex<...>>`. IR reuse is gated on matching
`CompilationKey`: source hash, manifest hash, limits hash,
language/compiler/parser versions.

## Scheduler Integration

- `JobKind::ToolProgram` identifies program jobs.
- `JobPayload::ToolProgram` carries program_id, source_digest,
  ir_digest, authority_digest, submission_key, source_ref,
  source_length, allowed_tools, authority_grant_json, and
  execution_context_json.
- `ToolProgramExecutor` (`src/scheduler/tool_program_executor.rs`)
  implements `JobExecutor` for `JobKind::ToolProgram`.
- Submission service verifies referenced records and hashes before
  creating the job.
- `BrokerAdapter` carries a `ToolCaller::Program` variant into the
  broker invocation context.

## M006: Read-Only Programmable Tool Palette

### `tool_program` Foreground Model Tool

`src/tool/tool_program.rs` — the model submits a restricted-Python
program via the `tool_program` tool. The tool:

1. Validates `source` (non-empty) and `tools` array (non-empty).
2. Compiles source to IR via `tool_program::compile_program()`.
3. Verifies IR integrity via `verify_ir_integrity()`.
4. Persists source under `.codegg/tool_program_sources/`.
5. Builds authority grant and execution context.
6. Submits the job to the scheduler via `JobSubmissionService`.
7. Returns the `program_id` and submission status.

The tool itself is `DirectOnly` — only the agent loop can call it.
Programs it produces may only call `DirectOrProgrammatic` tools and
the hidden `ProgrammaticOnly` operation-scoped adapters (M003).

### Read-Only Tool Palette

Eight tools are eligible for programmatic invocation:

| Tool | Caller Policy | Effect Class | Output Schema | Cache TTL |
|------|--------------|--------------|---------------|-----------|
| `read` | `DirectOrProgrammatic` | `ReadOnly` | `path`, `content`, `line_count`, `byte_count`, `truncated` | 300s |
| `glob` | `DirectOrProgrammatic` | `ReadOnly` | `pattern`, `files`, `count`, `truncated` | 60s |
| `grep` | `DirectOrProgrammatic` | `ReadOnly` | `pattern`, `matches`, `total_matches`, `files_searched`, `truncated` | 60s |
| `list` | `DirectOrProgrammatic` | `ReadOnly` | `path`, `entries`, `count`, `truncated` | 30s |
| `diff` | `DirectOrProgrammatic` | `ReadOnly` | `path`, `has_changes`, `diff`, `truncated`, `original_bytes`, `current_bytes` | 60s |
| `repo_search` | `DirectOrProgrammatic` | `ReadOnly` | upstream eggsearch JSON object (backend-versioned) | disabled (nondeterministic external read) |
| `git_read` | `ProgrammaticOnly` (hidden) | `ReadOnly` | `operation`, `truncated`, `results` (unwrapped `GitPayload`) | disabled (workspace-version dependent) |
| `lsp_read` | `ProgrammaticOnly` (hidden) | `ReadOnly` | `operation`, `result_count`, `truncated`, `results` (canonical `LspToolOutput`) | disabled (server-state dependent) |

### Manifest Resolution

`src/tool/program_manifest.rs` — validates a program's requested
tools against the broker catalog before job creation.

Rejection reasons:
- `NotFound` — tool not in broker catalog
- `DirectOnly` — tool is `DirectOnly`, not callable by programs
- `NoOutputSchema` — tool has no output schema defined
- `NotReadOnly` — tool effect class is not `ReadOnly` or `ReadValidate`
- `InvalidContract` — contract validation failed

### Read-Only Call Cache

`src/tool/program_cache.rs` — caches typed results from read-only
tool calls within a program run.

- **Cache key**: `CacheKey { tool_name, input_hash, workspace_id }`
- **Default TTL**: 300s (configurable per `ProgramCacheConfig`)
- **Max entries**: 100 per tool, 1000 total
- **Eviction**: oldest-first when limits reached
- **Thread-safe**: `parking_lot::RwLock<HashMap<...>>`
- Per-execution; does not persist across daemon restarts

## Expansion M001: Deterministic Local Read Contract Expansion

Status: implemented (see `plans/closure/tool-program-capability-expansion/001-status.md`).

### Eligibility matrix

Programmatic admission is governed by an executable checklist, not by
`ToolCategory::ReadOnly` alone. A tool is admitted only with all of:

1. explicit `DirectOrProgrammatic` caller policy;
2. read-side effect class (`ReadOnly` / `ReadValidate`);
3. declared output schema (manifest-gated);
4. explicit input/output bounds (file bytes, original bytes, diff display);
5. execution-context-derived workspace/path policy (no process-CWD authority);
6. deterministic `LocalTrusted` semantics (or explicitly nondeterministic);
7. conservative retry/cache declaration (local reads: no broker retry);
8. truthful replay/ledger behavior (only `Success` replays);
9. no hidden mutable globals, network, or mutation surface.

Enforced by `tests/tool_program_diff_palette.rs` (matrix, census, broker,
manifest, cache, ledger tests) plus `tests/tool_contract_guards.rs`.

### `diff` promotion

`src/tool/diff.rs` — the local `diff` operation is `DirectOrProgrammatic` /
`ReadOnly` / `Idempotent` with a 60s cache policy and a structured output
schema (`path`, `has_changes`, `diff`, `truncated`, byte counts):

- `execute()` keeps the direct string behavior (unified diff or
  `(no changes)`).
- `execute_structured()` returns the same display plus a bounded structured
  value with `Native` / `codegg/diff` / `LocalTrusted` provenance.
- Relative paths resolve against the broker-supplied
  `ToolExecutionContext.cwd` (workspace root for program calls); the
  tool-default root remains only for legacy context-free direct callers.
- Path policy rejects escapes, `..` traversal outside the root, and symlink
  components (via `check_path_for_symlinks`); failures are deterministic
  (`Denied` for policy violations, typed errors for missing/invalid input).
- Bounds: 10 MiB file and `original` caps, 256 KiB diff display with an
  explicit truncation marker and `truncated` metadata.
- Contract-hash changes invalidate stale manifests/caches by construction
  (canonical `ContractEntry` digest covers caller policy and schemas).

### Candidate census (M001 disposition)

Admitted: `read`, `glob`, `grep`, `list`, `diff`. Deferred without promotion:
mutations (`write`, `edit`, `apply_patch`, `replace`, `commit`), process
execution (`bash`, `terminal`, `test`, `task`), multiplexed read/mutation
surfaces (`git`, `lsp` — covered since Expansion M003 by the hidden
`git_read`/`lsp_read` adapters; the multiplexed tools stay `DirectOnly`), external network reads (`repo_search`
— M002 scope at the time; since admitted, see Expansion M002 — and other
search/fetch wrappers, `websearch`, `webfetch` — still out of scope),
`tool_program` itself (`DirectOnly`), and workflow/state surfaces (`skill`,
`question`, `security`, `review`, `image`, `research`). The census is encoded
in `tests/tool_program_diff_palette.rs::candidate_census_*` so future
promotions must satisfy the same matrix evidence.

## Expansion M002: External Repository-Search Programmatic Read Seam

Status: implemented (see `plans/closure/tool-program-capability-expansion/002-status.md`).

### Eligibility extension (explicitly nondeterministic external read)

`repo_search` is admitted under the M001 matrix extended by one declared
exception: the network-surface ban is lifted for a single explicitly
classified external read with daemon-owned runtime context,
`ExternalUntrusted` provenance, and truthful nondeterministic
cache/replay semantics. Admission requires all of:

1. explicit `DirectOrProgrammatic` caller policy (or
   `ProgrammaticOnly` for the hidden M003 operation-scoped adapters);
2. read-side effect class (`ReadOnly`);
3. declared output schema (manifest-gated; intentionally permissive
   `object` because the upstream eggsearch JSON shape is
   backend-versioned — display/provenance bounds still apply);
4. explicit input/output bounds (`query` required, `max_results` capped
   at 30 by the eggsearch adapter, display capped at
   `max_repo_search_output_chars`, broker artifact boundary above);
5. daemon-owned `SearchRuntimeContext` threaded through
   `ToolRegistry::with_options` (the same registry serves direct and
   program calls; no isolated default silently replaces the configured
   service; no process-global slot);
6. explicitly nondeterministic `ExternalUntrusted` semantics
   (`Idempotent` means ledger replay serves the recorded
   execution-time result, NOT that a fresh rerun recomputes the same
   results);
7. conservative retry/cache declaration: no broker-side retries and the
   program-call cache DISABLED so repeated identical queries within one
   run re-execute against the backend instead of implying determinism;
8. truthful replay/ledger behavior (only `Success` maps to a
   programmatic `Ok`; restart replay serves the recorded result and
   divergence fails closed via existing ledger semantics);
9. no hidden mutable globals, no credential choice (programs supply only
   query/filters — the input schema admits no `url`, `api_key`, `token`,
   `credential`, or `env` field; provider credentials stay in the
   daemon-owned context), no mutation surface.

Enforced by `tests/tool_program_search_palette.rs` (matrix, network
census, manifest, runtime-threading, broker, bounds, provenance,
cancellation, disabled-cache, ledger, rerun-vs-replay tests) plus the
`src/tool/repo_search.rs` contract unit tests and the
`tests/tool_contract_guards.rs` palette pin.

### `repo_search` promotion

`src/tool/repo_search.rs` — the canonical repository-search operation is
`DirectOrProgrammatic` / `ReadOnly` / `Idempotent` with no broker retry,
disabled program-call cache, and a permissive object output schema:

- `execute()` keeps the direct string behavior (framed
  `external_untrusted` evidence).
- `execute_structured()` returns the same display plus the upstream
  structured value with `Mcp` / `eggsearch/search` /
  `ExternalUntrusted` provenance.
- The tool holds the daemon-owned `SearchRuntimeContext` clone installed
  at registry construction; program calls execute through the same
  broker pipeline (caller policy, input-schema validation, verified
  authority, deadline/cancellation, output validation, artifact
  spillover) as direct calls.
- Bounds: `query` required, `max_results` capped at 30, display capped at
  `max_repo_search_output_chars` (default 15 KiB) with an explicit
  truncation marker and `truncated` metadata.
- Backend/policy failures (disabled, builtin-without-eggsearch,
  unavailable MCP service) surface as typed `InfrastructureError`
  programmatic failures under existing policy; cancellation propagates
  through `BrokerAdapter` before dispatch.
- Contract-hash changes invalidate stale manifests/caches by construction
  (canonical `ContractEntry` digest covers caller policy and schemas).

### Candidate census (M002 disposition)

Admitted: `read`, `glob`, `grep`, `list`, `diff` (M001) plus `repo_search`.
Still deferred without promotion: mutations, process execution,
multiplexed `git`/`lsp` (admitted in M003 only as the hidden
`git_read`/`lsp_read` operation-scoped adapters — the multiplexed
tools themselves stay `DirectOnly`; see Expansion M003), every other
external search/fetch
surface (`websearch`, `webfetch`, `repo_fetch`, `repo_map`,
`codesearch` compat alias, `batch_fetch`, `security_search`,
`research_search`, `evidence_bundle`, `research`), `tool_program` itself,
and workflow/state surfaces. The census is encoded in
`tests/tool_program_search_palette.rs::candidate_census_*`.

## Expansion M003: Operation-Scoped Git/LSP Read Adapters

Status: implemented (see `plans/closure/tool-program-capability-expansion/003-status.md`).

### Why adapters instead of promotion

`git` and `lsp` are multiplexed tools: `ToolContract` is tool-level,
so marking either `DirectOrProgrammatic` would structurally admit
mutation-capable inputs (typed git `mutation`/`recover` actions, LSP
`renamePreview`/`formatPreview`/code-action apply). The M003 adapters
are separate hidden `ProgrammaticOnly` tools whose input schemas
cannot name a mutation at all (strict `deny_unknown_fields` parsing
plus an `operation` enum limited to the read subset).

### Operation allow/deny table

| Surface | Allowed (adapter op) | Denied (structurally unavailable) |
|---|---|---|
| `git_read` | `status`, `diff` (optional `base_ref`), `log` (`max_count` clamped 1–50), `branches` (local only) | `show`, `blame`, tags, remotes, worktrees, stashes, `rev-parse`, every typed `mutation`, `recover`, `operation_state`, raw `subcommand`/`args`, any `workdir` input |
| `lsp_read` | `diagnostics`, `documentSymbol`, `workspaceSymbol`, `hover`, `goToDefinition`, `findReferences` | all previews (`renamePreview`, `formatPreview`, `sourceActionPreview`, `codeActionPreview`, `semanticCheckPreview`), contexts, hierarchies, workflows, capabilities, and every preview field (`new_name`, `action`, `content`, `patch`, …) |

Deferred reads that stay out of the minimal subset (e.g. git
`show`/`blame`, LSP `declaration`/`callHierarchy`) remain available
to direct callers through the multiplexed tools; a future milestone
may admit them through the same adapter pattern with fresh evidence.

### Canonical delegation map

- `git_read` executes typed `codegg_git::GitOperation`s through
  `GitExecutionService` (the canonical read executor, itself
  delegating to `egggit`). No `workdir` input: the broker
  execution-context cwd (program workspace root) is the repository
  root. Display is the canonical service stdout bounded to 64 KiB;
  `results` is the unwrapped typed payload.
- `lsp_read` executes through `LspTool::execute_scoped_read` — the
  exact typed implementation behind the model-facing `lsp` read arms —
  sharing the registry's `LspService` and the program workspace root
  as `allowed_root`. Path validation, per-operation bounds, and
  `egglsp`/`LocalUntrusted` provenance are therefore the canonical
  rules; the adapter implements no LSP protocol logic and never calls
  the `Tool` trait (tool-broker boundary stays intact).

### Eligibility extension (workspace/server-state-dependent reads)

Both adapters are admitted under the M001 matrix with the M002-style
declared exception for state-dependent reads:

1. explicit `ProgrammaticOnly` caller policy (hidden from model
   definitions, `tool_search`, and all disclosure palettes);
2. read-side effect class (`ReadOnly`);
3. declared output schema (manifest-gated);
4. explicit input/output bounds (log-count clamp, `base_ref`
   alphabet/length bound, 1-indexed positions, 200-char symbol
   query, 64 KiB git display cap, broker artifact boundary above);
5. execution-context workspace authority (no `workdir` input; LSP
   paths validated against the program root);
6. explicitly version-dependent semantics (`Idempotent` means ledger
   replay serves the recorded execution-time result, NOT that a
   fresh rerun recomputes the same results — git races with worktree
   mutations, LSP reflects live server state);
7. conservative retry/cache declaration: no broker-side retries and
   the program-call cache DISABLED so reruns re-observe state;
8. truthful replay/ledger behavior (only `Success` maps to a
   programmatic `Ok`; unavailable LSP servers fail closed as typed
   `InfrastructureError`);
9. no hidden mutable globals, no subprocess/shell of its own, no
   mutation surface.

`resolve_contract_snapshot` admits `ProgrammaticOnly` alongside
`DirectOrProgrammatic` (manifest resolution already did); `DirectOnly`
remains rejected everywhere. Contract-hash changes invalidate stale
manifests by construction.

Enforced by `tests/tool_program_git_lsp_palette.rs` (allow/deny
table, delegation equivalence, broker matrix, mutation negatives,
workspace isolation, bounds, rerun-vs-replay, disclosure invariance)
plus the `src/tool/git_read.rs` / `src/tool/lsp_read.rs` contract
unit tests.

## M007: Child-Job Composition

Programs call `submit_job("op", {...})` which compiles to the
`ExecuteChildJob` IR opcode. The broker adapter translates typed
requests into canonical `NewJob` submissions via
`JobSubmissionService` and waits for completion.

### `submit_job()` Language Construct

```python
# Assigned form
result = submit_job("test", {"scope": "workspace", "timeout_secs": 120})

# Expression statement form
submit_job("build", {"argv": ["cargo", "build", "--release"]})
```

`submit_job` is a reserved builtin alongside `call`, `parallel`,
`emit`, and `fail` (`validator.rs:14`).

### Types (`child_job.rs`)

- `ChildJobOp`: `Test` / `Build` / `Lint` / `Format`
- `ChildJobConfig`: per-operation typed config (scope, cwd, argv, timeout_secs)
- `ChildJobRequest`: submission request (op + config)
- `ChildJobResult`: completion result (success, exit_code, duration_ms, details, artifacts, error)

### Operation-to-Scheduler Mapping

| ChildJobOp | JobKind | Default argv |
|------------|---------|--------------|
| `Test` | `Test` | `["cargo", "test"]` |
| `Build` | `Build` | `["cargo", "build"]` |
| `Lint` | `Lint` | `["cargo", "clippy", "--", "-D", "warnings"]` |
| `Format` | `Format` | `["cargo", "fmt", "--", "--check"]` |

Before translation, the broker applies a closed Cargo argv allowlist,
rejects shell metacharacters, and resolves requested cwd values beneath
the parent workspace. The effective child timeout is the minimum of the
requested timeout and the remaining parent deadline.

### `BrokerCallback` Trait

```rust
#[async_trait]
pub trait BrokerCallback: Send + Sync {
    async fn execute_call(&self, request: &CallRequest)
        -> Result<CallResult, InterpreterError>;
    async fn submit_child_job(&self, request: &ChildJobRequest)
        -> Result<ChildJobResult, InterpreterError>;
    async fn submit_child_job_with_checkpoint(
        &self, request: &ChildJobRequest,
        checkpoint: &InterpreterCheckpoint,
    ) -> Result<ChildJobResult, InterpreterError>;
    async fn heartbeat(&self, budget: &BudgetSnapshot);
    async fn call_reserved(&self, sequence: u32, request: &CallRequest)
        -> Result<(), InterpreterError>;
    async fn call_completed(&self, completed: &CompletedCall)
        -> Result<(), InterpreterError>;
    async fn checkpoint(&self, checkpoint: &InterpreterCheckpoint)
        -> Result<(), InterpreterError>;
}
```

The default implementations for `submit_child_job`,
`submit_child_job_with_checkpoint`, `call_reserved`, `call_completed`,
and `checkpoint` are no-ops or return errors. Production code
overrides all of them via `BrokerAdapter`.

## M008: Background Programs and Notifications

### Execution Modes

- **`foreground`** (default): blocks until completion, returns result
- **`background`**: returns a compact `ProgramHandle` immediately;
  terminal notification delivered when the program finishes

### Notification Service

`ToolProgramNotificationService` manages durable notification
records with claim/ack semantics:

- **Record**: created at background submission
- **Claim**: compare-and-set from Pending to Claimed
- **Acknowledge**: transition from Claimed to Delivered
- **Session bound**: max pending per session (default 16)
- **Lease timeout**: 5 min claim lease (default)

### Three-Way Classification

- **`Completed`** — program finished successfully
- **`IncompleteRecoverable`** — incomplete but retry-eligible (timeout, stall, interrupted)
- **`FailedTerminal`** — terminal failure (compile error, policy denial, resource exhaustion)

### Recovery State Machine (M017)

After a daemon restart, the notification service rebuilds from the
job store's terminal state. Every recovery branch uses
`expected_notification_event()` to reconstruct the expected event
and `EventStore::confirm_existing()` to verify before any state
transition. Event existence alone is never sufficient for semantic
confirmation.

## M012: Authority and Failure Semantics

### Grant Scope Verification

Every nested broker call verifies the `ToolAuthorityGrant` against:
validity, integrity, workspace, caller class, effect class, session,
permission mode, principal, path policy, manifest, contract snapshot,
and policy revision (12 dimensions). Missing, stale, mismatched, or
tampered grants fail closed.

### Programmatic Failure Mapping

`BrokerResult::into_programmatic_outcome()` maps terminal statuses:

| `ToolTerminalStatus` | `ProgrammaticOutcome` |
|---|---|
| `Success` | `Ok(ToolValue)` |
| `Denied` | `Err(Denied)` |
| `Cancelled` | `Err(Cancelled)` |
| `TimedOut` | `Err(TimedOut)` |
| `InfrastructureError` | `Err(InfrastructureError)` |
| `Error` | `Err(InfrastructureError)` |

Only `Success` increments `calls_completed` and enters the
replay-completed map.

## M013–M014: Checkpointing and Replay

### Checkpoint Emission

The compiler emits `IrOp::Checkpoint` at five boundaries:
1. Before nested call reservation
2. After call completion
3. At bounded loop intervals
4. After parallel convergence
5. Before terminal publication

### InterpreterCheckpoint

Contains: PC, steps, iterations, calls_completed, bytes_used,
parallel_groups, bounded locals (not just hash), bounded stack,
pending_child_wait identity, original_deadline_millis,
checkpoint_sequence, semantic_digest (SHA-256), and completed
calls for replay.

### Restart Replay

On restart the interpreter loads completed calls via
`load_completed_calls()` and re-executes from PC=0. Each
`ExecuteCall` looks up its sequence in the completed calls map;
matched calls are replayed without broker invocation.

## M014: Child-Job Artifact Recovery

Child job results produce durable `ChildArtifactHandle` records
with SHA-256 content digests. The `BrokerAdapter` tracks
submitted child jobs via `ChildJobTracking` and the executor
persists a canonical summary artifact via `ContextArtifactStore`.

## M019/M020: Strict Closure and Corrective Disposition

M019 accepted independent strict closure via shared hosted run
`30931979689` / job `92084050226`. M020 accepted corrective
disposition for child-artifact recovery (same hosted run). All
milestones in the Tool Programs subsystem are closed.

## Configuration Surface

### Runtime Limits

| Limit | Source | Description |
|-------|--------|-------------|
| `max_steps` | Static bounds | Total IR instruction count |
| `max_loop_iterations` | Static bounds | Per-loop iteration cap |
| `max_total_iterations` | Static bounds | Aggregate iteration cap |
| `max_dynamic_calls` | Static bounds | Total broker calls |
| `max_parallel_width` | Static bounds | Concurrent parallel calls |
| `max_parallel_depth` | Static bounds | Nested parallel groups |
| `max_value_growth` | Static bounds | Aggregate value byte size |
| `max_bytes` | Derived (4x value growth) | Total byte budget |
| `max_inflight_calls` | Derived (= max_dynamic_calls) | Concurrent in-flight calls |
| `max_wall_time_ms` | Executor config | Wall-clock deadline (0=unlimited) |
| `max_stall_time_ms` | Executor config | Stall detection threshold |
| `max_per_call_time_ms` | Executor config | Per-call timeout (0=unlimited) |
| `max_retries` | Executor config | Transient retry attempts |
| `retry_base_delay_ms` | Executor config | Base retry delay (exponential) |

### Timeout Defaults

| Parameter | Default | Source |
|-----------|---------|--------|
| Stall timeout | 60s | `max_stall_time_ms` |
| Per-call timeout | 30s | `max_per_call_time_ms` |
| Wall deadline | job deadline or `max_wall_time_ms` | `RunConfig.wall_deadline` |
| Model timeout | 10 min | `MAX_MODEL_TIMEOUT_MS` in `tool_program.rs` |
| Retries | 2 | `max_retries` |

### Storage Layout

Current storage version is defined by `storage::STORAGE_LAYOUT_VERSION`
(`storage/mod.rs`).
Tool program tables (`tool_program`, `tool_program_call`) were
introduced in migration v33 and are additive.

## Testing

```bash
cargo test -p codegg-core --lib tool_program       # core domain tests
cargo test --test tool_program                      # integration tests
cargo test --test hosted_tool_program               # hosted adapter tests
cargo test --test tool_program_scenario             # scenario tests
```

## Operator Diagnostics

### Terminal State Classification

| ProgramStatus | Meaning | Operator action |
|---------------|---------|-----------------|
| `Completed` | Program emitted a result | Inspect output |
| `Failed` | Execution error or validation failure | Check `failure_class` and `error_message` |
| `Cancelled` | User or parent cancelled | Expected — no action |
| `TimedOut` | Wall-clock or per-call deadline exceeded | Increase timeout or simplify program |
| `Stalled` | No progress within stall threshold | Check broker or increase timeout |
| `Incomplete` | Budget exhausted | Increase relevant budget |
| `Recoverable` | Transient error, retry-eligible | Daemon retries automatically |

### Failure Classes

| Class | Retryable | Typical cause |
|-------|-----------|---------------|
| `Validation` | No | Source/IR/manifest validation error |
| `ManifestDrift` | No | Tool manifest changed after submission |
| `AuthorityNarrowed` | No | Authority reduced after submission |
| `SchemaMismatch` | No | Output doesn't match result schema |
| `TransientBackend` | Yes | Temporary provider/backend error |
| `Timeout` | No | Wall-clock or per-call deadline |
| `Stall` | No | No progress detected |
| `Cancelled` | No | Explicit cancellation |
| `Storage` | No | Persistence failure |
| `ReplayDivergence` | No | Checkpoint replay mismatch |
| `BudgetExhausted` | No | Step/byte/iteration/call budget |
| `Execution` | No | Type, index, division error |
| `InternalPanic` | No | Interpreter invariant violation |

## Retention

- Active programs retain source, IR, calls, and artifacts.
- Terminal programs may be garbage-collected after a configurable
  retention window (not yet implemented).
- Source/IR content-store GC removes only unreferenced digests.
- The `tool_program` table cascades deletes to `tool_program_call`.

## Related Docs

- `architecture/tool_broker.md` — Tool Broker pipeline
- `architecture/tool_program_language.md` — Restricted-Python language spec
- `architecture/tool.md` — Tool trait and registry
## Run groups and background execution

Tool Programs remain the deterministic programmatic tool-loop authority: they
compile restricted Python to verified IR and use their own bounded result and
notification records. A run group is different coordination over independent
agent runs and must not be used to replace the Tool Program interpreter.
Tool Program background mode returns the durable scheduler job/program handle
and notifies the owning session on terminal completion; it does not create an
agent-run group or an unowned task. Use ordinary direct/parallel tools for
short turn-local work, Tool Programs for bounded deterministic tool loops, and
run groups only for independent delegated agent lifetimes.
