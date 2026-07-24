# Tool Programs — Program Domain, Storage, and Call Ledger

Tool Programs introduce a durable, versioned domain model for
agent-submitted bounded programs that invoke approved CodeGG tools
through deterministic control flow.

## Ownership

`crates/codegg-core/src/tool_program/` owns:

- `ToolProgramId`, `ProgramCallId` opaque typed IDs
- `ToolProgramState` lifecycle (submitted → queued → compiling →
  running → waiting → retry_backoff → terminal states)
- `ProgramLanguage` with `RestrictedPython` and forward-compatible
  unknown handling
- `ProgramSourceRef`, `ProgramIrRef` content-addressed immutable
  references
- `ProgramCapabilityManifest` frozen callable-tool contracts and
  authority digest
- `ProgramLimitsSnapshot` every persisted budget
- `ProgramCheckpoint` deterministic interpreter position for restart
  recovery
- `ProgramCallRecord` nested-call ledger with sequence, input hash,
  status, attempts, child job/run, result projection, failure class,
  and replay disposition
- `ProgramResult` terminal type, value/artifacts, failure summary,
  and budget usage
- `ContentAddressedStore` trait + `InMemoryContentStore`
- `ToolProgramStore` trait + `InMemoryToolProgramStore`

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

## State Machine

```
Submitted → Queued → Compiling → Running → Waiting ↔ Running
                                    ↓
                               RetryBackoff → Running
                                    ↓
                 Queued ← Interrupted
                                    ↓
                        Terminal: Completed | Incomplete | Failed |
                                  Cancelled | TimedOut | Blocked
                 Stalled → Running | Failed | TimedOut | Interrupted
```

Terminal states never regress. The `validate_program_transition()`
function enforces the transition table.

## Storage Schema (v33)

```sql
tool_program (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
    language TEXT NOT NULL,
    state TEXT NOT NULL,
    source_json TEXT NOT NULL,
    ir_json TEXT,
    manifest_json TEXT NOT NULL,
    checkpoint_json TEXT,
    result_json TEXT,
    job_id TEXT,
    submission_key TEXT NOT NULL UNIQUE,
    labels_json TEXT NOT NULL DEFAULT '{}',
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER
)

tool_program_call (
    id TEXT PRIMARY KEY,
    program_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    tool_name TEXT NOT NULL,
    tool_contract_hash TEXT NOT NULL,
    normalized_input_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    child_job_id TEXT,
    child_run_id TEXT,
    result_artifacts_json TEXT NOT NULL DEFAULT '[]',
    result_projection TEXT,
    failure_class TEXT,
    error_message TEXT,
    replay_disposition TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER,
    UNIQUE(program_id, sequence),
    FOREIGN KEY(program_id) REFERENCES tool_program(id) ON DELETE CASCADE
)
```

## Content-Addressed Store

Source and IR content is stored by SHA-256 digest in separate
namespaces (`src`, `ir`). Every load verifies digest and length.
The `ContentAddressedStore` trait defines `put`, `get`, `contains`,
and `gc`.

## Scheduler Integration

- `JobKind::ToolProgram` identifies program jobs.
- `JobPayload::ToolProgram` carries `program_id`, `source_digest`,
  `ir_digest`, `authority_digest`, and `submission_key`.
- Submission service verifies referenced records and hashes before
  creating the job.
- `ToolProgramExecutor` (`src/scheduler/tool_program_executor.rs`)
  loads verified IR, creates a `MeteredInterpreter`, and runs it
  through the scheduler's admission-controlled execution path.

## Call Ledger

Each nested call gets a `ProgramCallRecord` with:
- Monotonic `sequence` within the program
- Tool contract hash and normalized input hash for replay
- State machine: Reserved → Running → Completed/Failed/Cancelled/TimedOut
- Replay disposition: Replay (completed), Reexecute (non-idempotent),
  Skip (cancelled)

## Query DTOs

- `ProgramSummary`: compact list view (id, state, language, submission
  key, job_id, timestamps) — canonical in `store.rs`, re-exported from
  `mod.rs`
- `ProgramListQuery`: workspace/session/state filtering with
  pagination (limit, offset)
- `ProgramStoreQuery`: internal store-level query with
  workspace_id, session_id, states, limit, offset

All DTOs derive `Serialize`/`Deserialize` for protocol transport.
Visibility/redaction classification is explicit: `labels` must not
contain source, manifest bodies, credentials, or unbounded output.

## Interpreter Runtime (M005)

The `MeteredInterpreter` (`crates/codegg-core/src/tool_program/interpreter.rs`)
is a stack-machine evaluating verified IR with bounded budgets.

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
| `max_bytes` | Derived (4× value growth) | Total byte budget |
| `max_inflight_calls` | Derived (= max_dynamic_calls) | Concurrent in-flight calls |
| `max_wall_time_ms` | Executor config | Wall-clock deadline (0=unlimited) |
| `max_stall_time_ms` | Executor config | Stall detection threshold |
| `max_per_call_time_ms` | Executor config | Per-call timeout (0=unlimited) |
| `max_retries` | Executor config | Transient retry attempts |
| `retry_base_delay_ms` | Executor config | Base retry delay (exponential) |

### Checkpointing

The `Checkpoint` IR instruction produces an `InterpreterCheckpoint`
containing: PC, steps, iterations, calls completed, bytes used,
parallel groups, locals hash, and completed calls for replay.

### Restart Replay

On restart, the interpreter loads completed calls via
`load_completed_calls()` and re-executes from PC=0. Each
`ExecuteCall` instruction looks up its sequence in the completed
calls map; matched calls are replayed without broker invocation.

### Watchdog and Stall Detection

- Heartbeat emitted via `BrokerCallback::heartbeat()` at each
  instruction milestone, call start/complete, and checkpoint commit.
- Stall detection checks `last_progress_at` against `max_stall_time_ms`.
  If no progress (instruction or call activity) within the threshold,
  the program is marked `Stalled`.

### Transient Retry

`TransientBackend` failures are retried with exponential backoff
(base delay × 2^attempt + random jitter). Non-retryable failure
classes (validation, schema, budget, execution, etc.) fail immediately.

### Result-Schema Validation

When a `result_schema` is provided via `RunConfig`, the `Emit`
instruction validates the output against the JSON Schema before
returning `Completed`. Schema mismatches produce `FailureClass::SchemaMismatch`.

### Protocol events (M005)

The following `CoreEvent` variants are available when a program
executor is active:

- `ToolProgramStarted` — program transitions to Running
- `ToolProgramProgress` — heartbeat with budget usage (emitted at
  instruction milestones, call start/complete, checkpoint commit)
- `ToolProgramCallStarted` — call dispatched to tool
- `ToolProgramCallCompleted` — call result recorded
- `ToolProgramCompleted` — terminal state reached

Heartbeat emission is handled by the `BrokerCallback::heartbeat`
method, called at each meaningful progress boundary in the
interpreter.

## Scheduler Integration (M005)

### Executor Registration

`ToolProgramExecutor` (`src/scheduler/tool_program_executor.rs`)
implements `JobExecutor` for `JobKind::ToolProgram`. The executor:

1. Validates `JobPayload::ToolProgram` fields (program_id,
   source_digest, authority_digest non-empty).
2. Compiles the submitted source to IR (fixture in M005;
   content-addressed store in M006).
3. Verifies IR integrity via `verify_ir_integrity()`.
4. Creates `MeteredInterpreter` with `RuntimeLimits` derived from
   IR bounds plus executor-configured timeouts.
5. Creates `BrokerAdapter` bridging interpreter to real `ToolBroker`.
6. Runs with `CancellationToken` support and typed terminal mapping.

### Timeout Configuration

| Parameter | Default | Source |
|-----------|---------|--------|
| Stall timeout | 60s | `max_stall_time_ms` on `RuntimeLimits` |
| Per-call timeout | 30s | `max_per_call_time_ms` on `RuntimeLimits` |
| Wall deadline | job deadline or `max_wall_time_ms` | `RunConfig.wall_deadline` |
| Retries | 2 | `max_retries` on `RuntimeLimits` |

### Checkpoint Emission

The compiler emits `IrOp::Checkpoint` at five boundaries:

1. **Before nested call reservation** — before `ConstructCall`/`ExecuteCall`
2. **After call completion** — after `ExecuteCall` stores result
3. **At bounded loop intervals** — before `ForLoopNext` in loop body
4. **After parallel convergence** — after `ParallelExecute`
5. **Before terminal publication** — before `Emit` and `Fail`

Checkpoints produce `InterpreterCheckpoint` with: PC, steps,
iterations, calls completed, bytes used, parallel groups, locals
hash, and completed calls for replay.

### Recovery and Restart

On restart, the interpreter:

1. Loads completed calls from the checkpoint via
   `load_completed_calls()`.
2. Re-executes from PC=0.
3. Each `ExecuteCall` looks up its sequence number in the completed
   calls map; matched calls are replayed without broker invocation.
4. Unmatched calls are executed through the broker.

This guarantees completed calls are never re-executed.

## Operator Diagnostics

### Terminal State Classification

| ProgramStatus | Meaning | Operator action |
|---------------|---------|-----------------|
| `Completed` | Program emitted a result | None — inspect output |
| `Failed` | Execution error or validation failure | Check `failure_class` and `error_message` |
| `Cancelled` | User or parent cancelled | None — expected |
| `TimedOut` | Wall-clock or per-call deadline exceeded | Increase timeout or simplify program |
| `Stalled` | No progress within stall threshold | Check broker responsiveness or increase timeout |
| `Incomplete` | Budget exhausted (steps/bytes/iterations/calls) | Increase relevant budget or simplify program |
| `Recoverable` | Transient error, retry-eligible | Daemon will retry automatically |

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

### Restart Recovery

When a daemon restarts mid-execution:

- Completed calls are preserved in the checkpoint and replayed.
- In-flight calls (not yet completed) are lost and must be
  re-executed from scratch.
- Generation recovery marks stale attempts as `Interrupted` and
  requeues if the `RecoveryPolicy` permits.
- Durable checkpoint persistence is implemented in M006 via
  `ContentAddressedStore` integration.

### Incomplete Program Handling

Budget-exhausted programs return `Incomplete` with:

- Partial output value (if any)
- Budget snapshot (steps, bytes, iterations, calls used)
- Error message describing which budget was exhausted
- Recommended narrower continuation parameters

## Storage Migration

### Additive migration v33

`migrate_v33` in `session/schema.rs` adds two tables with no
modifications to existing tables:

```sql
tool_program (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
    language TEXT NOT NULL,
    state TEXT NOT NULL,
    source_json TEXT NOT NULL,
    ir_json TEXT,
    manifest_json TEXT NOT NULL,
    checkpoint_json TEXT,
    result_json TEXT,
    job_id TEXT,
    submission_key TEXT NOT NULL UNIQUE,
    labels_json TEXT NOT NULL DEFAULT '{}',
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER
)

tool_program_call (
    id TEXT PRIMARY KEY,
    program_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    tool_name TEXT NOT NULL,
    tool_contract_hash TEXT NOT NULL,
    normalized_input_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    child_job_id TEXT,
    child_run_id TEXT,
    result_artifacts_json TEXT NOT NULL DEFAULT '[]',
    result_projection TEXT,
    failure_class TEXT,
    error_message TEXT,
    replay_disposition TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER,
    UNIQUE(program_id, sequence),
    FOREIGN KEY(program_id) REFERENCES tool_program(id) ON DELETE CASCADE
)
```

Indexes: workspace, session, state, job, updated_at on `tool_program`;
(program_id, sequence), state, tool_name on `tool_program_call`.

### Compatibility

- **Old daemon opening new DB**: `JobKind::ToolProgram` deserializes to
  `Unsupported` via `#[serde(other)]`; program tables are ignored.
- **New daemon opening old DB**: migration v33 runs automatically; no
  program tables exist until first program is created.
- **Rollback**: migration is additive only (new tables); rolling back
  the daemon simply leaves orphaned tables that are ignored.

### `STORAGE_LAYOUT_VERSION`

Bumped from 32 to 33. The version is stored in `migration_version`
and checked on every database open.

## Retention

- Active programs retain source, IR, calls, and artifacts.
- Terminal programs may be garbage-collected after a configurable
  retention window (not yet implemented).
- Source/IR content-store GC removes only unreferenced digests via
  `ContentAddressedStore::gc()`.
- The `tool_program` table cascades deletes to `tool_program_call`
  via foreign key.

## M004: Restricted-Python Frontend and Static Bounds

### Parse Pipeline (M004)

```
source bytes → parse → normalized AST → validate → static bounds → compile IR → verify IR
```

Ownership: `crates/codegg-core/src/tool_program/` — submodules:

| Module | Purpose |
|--------|---------|
| `ast.rs` | Normalized Codegg-owned AST types (15 node kinds + Range) |
| `parser.rs` | rustpython-parser wrapper (~1100 lines) |
| `validator.rs` | Semantic validator: built-in shadowing, allowed methods, scope |
| `static_bounds.rs` | Bound analyzer: max steps, iterations, calls, parallel width, nesting |
| `ir.rs` | Versioned IR types, 38 opcodes, SHA-256 deterministic digest |
| `compiler.rs` | IR compiler: AST → flat instruction sequence (~620 lines) |
| `ir_verifier.rs` | IR verifier: jump targets, local slots, pool refs, bounds, terminal |
| `diagnostics.rs` | 20 diagnostic codes (TP001–TP018, TP998, TP999), bounded source spans |
| `store.rs` | Content-addressed IR storage, cache key matching, serialize/deserialize |
| `guards.rs` | Compile-time guards: parse-only pipeline, no CPython execution |

### Dependency Inventory

| Dependency | Version | License | MSRV | Purpose | Parse-only |
|------------|---------|---------|------|---------|------------|
| `rustpython-parser` | 0.4.0 | MIT | 1.72.1 | Parse Python source to AST | Yes — no exec |

Features used: `default` (location + malachite-bigint). ~15 transitive crates.
No network, filesystem, or async dependencies. No pyo3 or CPython bindings.

### Agent-Facing Examples

#### Accepted source (Tool Program v1)

```python
# Simple tool call with emit
result = call({"tool": "grep_search", "pattern": "TODO"})
emit({"found": result})

# Bounded loop with parallel calls
reads = parallel(
    {"tool": "read_file", "path": "a.py"},
    {"tool": "read_file", "path": "b.py"},
)
emit({"results": reads})

# Conditional logic with loop
total = 0
for i in range(10):
    total = total + 1
emit({"total": total})
```

#### Rejected source

```python
import os              # TP001 — imports not supported
while True:            # TP001 — while loops not supported
    pass
class Foo:             # TP001 — class definitions not supported
    pass
f = lambda x: x       # TP001 — lambda not supported
x = [i for i in y]    # TP001 — comprehensions not supported
```

### Diagnostics Troubleshooting

| Code | Name | Meaning | Fix |
|------|------|---------|-----|
| TP001 | UnsupportedSyntax | while, try, import, class, lambda, etc. | Rewrite using for/if/assign/emit/fail |
| TP002 | UnboundedLoop | Unknown iteration count | Use literal list or range() |
| TP003 | MaxNestingDepth | Nesting exceeds max (20) | Flatten control flow |
| TP004 | MaxCollectionSize | Literal/collection too large | Reduce element count |
| TP005 | BuiltInShadowing | Shadowed call/parallel/emit/fail | Rename variable |
| TP006 | IllegalAttributeAccess | Disallowed method on object | Use allowed methods only |
| TP007 | MaxParallelWidth | Parallel group too wide | Reduce parallel descriptors |
| TP008 | MaxIrSteps | IR step budget exceeded | Simplify program |
| TP009 | MaxCallSites | Too many tool call sites | Reduce calls |
| TP010 | UnresolvedIdentifier | Unknown variable name | Assign before use |
| TP011 | InvalidCallDescriptor | call() missing descriptor arg | Provide dict to call() |
| TP012 | MaxTotalIterations | Total loop iterations exceeded | Reduce loop bounds |
| TP013 | SourceTooLarge | Source exceeds 1 MB | Split into smaller programs |
| TP014 | MaxAstNodes | AST node count exceeded (10K) | Simplify program |
| TP015 | MaxIdentifierLength | Identifier too long | Shorten name |
| TP016 | UnsupportedVersion | IR/language/compiler version mismatch | Recompile with current version |
| TP017 | DiagnosticSpanTooLarge | Source span exceeds bounds | Reduce source size |
| TP018 | DestructuringMismatch | Assignment target count mismatch | Fix destructuring |
| TP998 | VerificationFailed | IR verification failed | Report bug |
| TP999 | InternalError | Internal compiler error | Report bug |

### Static Guards

Compile-time and module-level guards prevent CPython execution:

- No `pyo3` dependency in `codegg-core/Cargo.toml`
- No `std::process::Command::new("python3")` in `tool_program/` module
- No `eval()`/`exec()`/`compile()` on user source
- `guards.rs` module documents invariants and provides `assert_parse_only!()` macro
- `cargo deny` / `cargo audit` in CI verifies no CPython dependencies

### Fuzz Targets

Located in `crates/codegg-core/fuzz/fuzz_targets/`:

| Target | What it tests |
|--------|--------------|
| `parser_fuzz` | Parser never panics on arbitrary bytes |
| `compiler_fuzz` | Full pipeline never panics on arbitrary input |
| `roundtrip_fuzz` | IR serialize/deserialize round-trip integrity |

Run with: `cargo fuzz run <target> -- -max_total_time=300`

## M006: Read-Only Programmable Tool Palette (Implemented)

M006 delivers the model-facing `tool_program` foreground tool, a
read-only palette of four tools callable from restricted-Python
programs, manifest-based tool eligibility gating, and a content/policy
aware read-only call cache.

### `tool_program` Foreground Model Tool

`src/tool/tool_program.rs` — the model submits a restricted-Python
program via the `tool_program` tool. The tool:

1. Validates `source` (non-empty) and `tools` array (non-empty).
2. Compiles source to IR via `tool_program::compile_program()`.
3. Verifies IR integrity via `verify_ir_integrity()`.
4. Submits the job to the scheduler via `JobSubmissionService`.
5. Returns the `program_id` and submission status.

The tool itself is `DirectOnly` — only the agent loop can call it.
Programs it produces may only call `DirectOrProgrammatic` tools.

### Read-Only Tool Palette

Four tools are eligible for programmatic invocation:

| Tool | Caller Policy | Effect Class | Output Schema | Cache |
|------|--------------|--------------|---------------|-------|
| `read` | `DirectOrProgrammatic` | `ReadOnly` | `path`, `content`, `line_count`, `byte_count`, `truncated` | 300s TTL |
| `glob` | `DirectOrProgrammatic` | `ReadOnly` | `pattern`, `files`, `count`, `truncated` | 60s TTL |
| `grep` | `DirectOrProgrammatic` | `ReadOnly` | `pattern`, `matches` (path/line/content), `total_matches`, `files_searched`, `truncated` | 60s TTL |
| `list` | `DirectOrProgrammatic` | `ReadOnly` | `path`, `entries`, `count`, `truncated` | 30s TTL |

Tools must satisfy all of the following to be callable from programs:

- `caller_policy == DirectOrProgrammatic`
- `effect_class == ReadOnly`
- `output_schema` is `Some(...)`
- Contract passes `validate()` (name non-empty, schema consistent)

### Manifest Resolution

`src/tool/program_manifest.rs` — validates a program's requested
tools against the broker catalog before job creation.

```
resolve_manifest(broker, requested_tools) → ResolvedManifest
```

Rejection reasons:
- `NotFound` — tool not in broker catalog
- `DirectOnly` — tool is `DirectOnly`, not callable by programs
- `NoOutputSchema` — tool has no output schema defined
- `InvalidContract` — contract validation failed

`manifest_is_valid()` returns `true` only when there are zero
rejections. Programs must only use tools in the `allowed_tools` list.

### Tool Contract Guards

At execution time, the `BrokerAdapter` carries a `ToolCaller::Program`
variant into the broker invocation context. The broker enforces:

- Caller policy check: only `DirectOrProgrammatic` tools may be
  called from a program context.
- Effect class check: only `ReadOnly` tools may be called.
- Schema validation: output must conform to the tool's output schema.

### Read-Only Call Cache

`src/tool/program_cache.rs` — caches typed results from read-only
tool calls within a program run.

- **Cache key**: `CacheKey { tool_name, input_hash, workspace_id }`
  incorporates tool identity, serialized arguments, and workspace.
- **TTL per tool**: read=300s, glob=60s, grep=60s, list=30s.
- **Max entries**: 100 per tool, 1000 total.
- **Eviction**: LRU-style — oldest entries evicted when limits reached.
- **Thread-safe**: `RwLock<HashMap<...>>`.

The cache is per-execution and does not persist across daemon restarts.

### Execution Flow

```
Model submits tool_program(source, tools, ...)
    │
    ▼
ToolProgramTool::execute_impl()
    │ 1. Validate source + tools non-empty
    │ 2. compile_program(source) → Compilation { ir, manifest }
    │ 3. verify_ir_integrity(ir)
    │ 4. Submit via JobSubmissionService
    │
    ▼
Scheduler admits job (JobKind::ToolProgram)
    │
    ▼
ToolProgramExecutor::execute()
    │ 1. Validate payload (program_id, source_digest, authority_digest)
    │ 2. Load/compile IR, verify integrity
    │ 3. Create MeteredInterpreter with RuntimeLimits
    │ 4. Create BrokerAdapter (bridges BrokerCallback → real ToolBroker)
    │ 5. Interpreter.run_with_config(broker_adapter, cancellation, run_config)
    │
    ▼
BrokerAdapter::execute_call(request)
    │ 1. Build BrokerInvocationContext (caller=Program, workspace, cwd)
    │ 2. broker.execute(registry, tool_name, input, ctx)
    │ 3. Map StructuredToolResult → CallResult (ProgramValue::ToolResult)
    │
    ▼
MeteredInterpreter steps through IR
    │  - ExecuteCall → BrokerAdapter → ToolBroker → real tool
    │  - CheckCache → ProgramCallCache (skip broker on hit)
    │  - Emit → ProgramResult (terminal)
    │
    ▼
ExecutorCompletion returned to scheduler
    │  - Status: Completed | Failed | Cancelled | TimedOut | ...
    │  - Result projected to model via StructuredToolResult
```

### Content-Addressed IR Store

`ProgramStore` (`store.rs`) provides:

- `digest_source(source)` — SHA-256 of source bytes
- `store_ir(source, ir)` — store IR after successful compilation
- `check_cache(source, manifest, limits)` — check for cached IR with matching key
- `get_ir(source)` / `contains_ir(source)` / `remove(source)` — retrieval and cleanup
- `serialize_ir(ir)` / `deserialize_ir(bytes)` — JSON round-trip
- `verify_ir_integrity(ir)` — digest consistency after deserialization

Thread-safe via `Arc<Mutex<...>>`. Concurrent access tested.

## Source Files (M006)

| File | Purpose |
|------|---------|
| `crates/codegg-core/src/tool_program/` | Domain types, store, interpreter, IR, compiler, verifier |
| `src/tool/tool_program.rs` | Foreground model-facing `tool_program` tool |
| `src/tool/program_manifest.rs` | Manifest resolution — tool eligibility gating |
| `src/tool/program_cache.rs` | Read-only call cache with content/policy-aware keys |
| `src/scheduler/tool_program_executor.rs` | Scheduler executor with `BrokerAdapter` for real pipeline |
