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
- No production executor exists until M005; scheduler transitions
  to `Blocked` rather than dispatching elsewhere.

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

### Future protocol events (M005+)

When a program executor exists, the following `CoreEvent` variants
will be added:

- `ToolProgramStarted` — program transitions to Running
- `ToolProgramProgress` — heartbeat with budget usage
- `ToolProgramCallStarted` — call dispatched to tool
- `ToolProgramCallCompleted` — call result recorded
- `ToolProgramCompleted` — terminal state reached

These are not implemented in M003 because no executor exists.

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
  retention window (not yet implemented in M003).
- Source/IR content-store GC removes only unreferenced digests via
  `ContentAddressedStore::gc()`.
- The `tool_program` table cascades deletes to `tool_program_call`
  via foreign key.

- Active programs retain source, IR, calls, and artifacts.
- Terminal programs may be garbage-collected after a configurable
  retention window (not yet implemented).
- Source/IR/content-store GC removes only unreferenced digests.

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

### Content-Addressed IR Store

`ProgramStore` (`store.rs`) provides:

- `digest_source(source)` — SHA-256 of source bytes
- `store_ir(source, ir)` — store IR after successful compilation
- `check_cache(source, manifest, limits)` — check for cached IR with matching key
- `get_ir(source)` / `contains_ir(source)` / `remove(source)` — retrieval and cleanup
- `serialize_ir(ir)` / `deserialize_ir(bytes)` — JSON round-trip
- `verify_ir_integrity(ir)` — digest consistency after deserialization

Thread-safe via `Arc<Mutex<...>>`. Concurrent access tested.
