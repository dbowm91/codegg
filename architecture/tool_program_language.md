# Tool Program Restricted-Python Language Specification

Status: normative (M004, closed)

Version: 1

This document specifies the restricted-Python language subset accepted
by the Tool Program compiler. The language is deliberately minimal — it
provides deterministic control flow, bounded iteration, and safe tool
invocation without requiring CPython execution, imports, reflection,
or arbitrary standard-library access.

## Purpose

Define the normative grammar, semantics, bounds, and rejection rules
for the restricted-Python language that Tool Programs compile to IR.
The language is not general Python — the ordinary `python_script` tool
remains unrestricted.

## Design Principles

1. **Parse-only pipeline**: the parser never executes source.
   Compilation produces deterministic IR without loading modules or
   spawning processes.
2. **Fail-closed**: unknown syntax, ambiguous constructs, and
   unbounded operations are rejected at parse or validation time.
3. **Bounded execution**: every accepted program has statically
   provable finite bounds for steps, iterations, calls, parallelism,
   and nesting.
4. **Deterministic output**: the same source, manifest, limits, and
   versions always produce identical IR and deterministic content
   hashes.
5. **Separate from general Python**: Tool Programs are not general
   Python. The ordinary `python_script` tool remains unrestricted.

## Grammar (Version 1)

### Program

```text
program     = statement* EOF
```

### Statements

```text
statement   = assignment
            | if_stmt
            | for_stmt
            | assert_stmt
            | tool_call_stmt
            | parallel_stmt
            | submit_job_stmt
            | emit_stmt
            | fail_stmt
            | pass_stmt

assignment  = target_list '=' expression
if_stmt     = 'if' expression ':' block ('elif' expression ':' block)* ['else' ':' block]
for_stmt    = 'for' target 'in' iterable ':' block
assert_stmt = 'assert' expression [',' expression]
tool_call_stmt = target '=' 'call' '(' arguments ')'
parallel_stmt  = target '=' 'parallel' '(' call_list ')'
submit_job_stmt = target '=' 'submit_job' '(' expression ',' expression ')'
                | 'submit_job' '(' expression ',' expression ')'
emit_stmt   = 'emit' '(' expression ')'
fail_stmt   = 'fail' '(' [expression] ')'
pass_stmt   = 'pass'
```

### Expressions

```text
expression  = boolean_or
boolean_or  = boolean_and ('or' boolean_and)*
boolean_and = comparison ('and' comparison)*
comparison  = bitwise_or (comp_op bitwise_or)*
comp_op     = '==' | '!=' | '<' | '>' | '<=' | '>=' | 'in' | 'not' 'in'
bitwise_or  = bitwise_xor ('|' bitwise_xor)*
bitwise_xor = bitwise_and ('^' bitwise_and)*
bitwise_and = shift ('&' shift)*
shift       = addition (('<<'|'>>') addition)*
addition    = multiply (('+'|'-') multiply)*
multiply    = unary (('*'|'@'|'/'|'%'|'//') unary)*
unary       = ('-'|'+'|'~') unary | power
power       = primary ['**' unary]
primary     = atom trailer*

atom        = 'None' | 'True' | 'False'
            | INTEGER | FLOAT | STRING
            | IDENTIFIER
            | list | tuple | dict
            | '(' expression ')'
            | 'len' '(' expression ')'
            | 'str' '(' expression ')'
            | 'int' '(' expression ')'
            | 'bool' '(' expression ')'

trailer     = '[' expression ']'
            | '[' expression ':' expression ']'
            | '[' expression ':' ']'
            | '[' ':' expression ']'
            | '[' ':' ']'
            | '.' IDENTIFIER '(' arguments ')'

list        = '[' [expression (',' expression)* [',']] ']'
tuple       = '(' [expression (',' expression)* [',']] ')'
dict        = '{' [expression ':' expression (',' expression ':' expression)* [',']] '}'
```

### Call Descriptors

```text
call_list   = call_descriptor (',' call_descriptor)*
call_descriptor = '{' key_value (',' key_value)* '}'
key_value   = STRING ':' expression
```

### Targets

```text
target_list = target (',' target)*
target      = IDENTIFIER
```

Destructuring assignment (`a, b = ...`) is supported for simple target
lists. Target count must match source count (TP018).

## Allowed Built-ins

| Name | Signature | Description |
|------|-----------|-------------|
| `call` | `(tool: dict) -> Any` | Invoke an approved tool through the Tool Broker |
| `parallel` | `(*calls: dict) -> list` | Execute call descriptors concurrently |
| `submit_job` | `(op: str, config: dict) -> dict` | Submit a scheduler-owned child job (M007) |
| `emit` | `(value: Any) -> None` | Emit a structured result value |
| `fail` | `(reason: str) -> None` | Fail the program with a reason |
| `len` | `(collection) -> int` | Collection length |
| `str` | `(value) -> str` | String conversion |
| `int` | `(value) -> int` | Integer conversion |
| `bool` | `(value) -> bool` | Boolean conversion |

Shadowing `call`, `parallel`, `submit_job`, `emit`, or `fail` as a
local variable is rejected by the validator (TP005).

Allowed methods on objects (`validator.rs:17`):
`append`, `items`, `keys`, `values`, `split`, `join`, `strip`,
`lower`, `upper`, `replace`, `get`.

## Value Types

### Primitives

| Type | Representation | Bounds |
|------|---------------|--------|
| `None` | null literal | — |
| `bool` | `True` / `False` | — |
| `int` | Decimal integer literal | Configurable max magnitude (default: 2^63) |
| `float` | Decimal float literal | Configurable max magnitude (default: 2^53 mantissa) |
| `str` | Single/double-quoted string | Configurable max length (default: 10,000 chars) |

### Collections

| Type | Syntax | Element bounds |
|------|--------|---------------|
| `list` | `[a, b, c]` | Configurable max elements (default: 1,000) |
| `tuple` | `(a, b, c)` | Configurable max elements (default: 1,000) |
| `dict` | `{k: v}` | Configurable max entries (default: 1,000) |

### Slicing

Slicing produces the same type as the source collection. Slice
indices must be compile-time integers or `None`.

## Deterministic Evaluation Order

All expressions evaluate left-to-right. Function arguments evaluate
left-to-right. No short-circuit evaluation is performed for `and`/`or`
— both operands are evaluated.

## Truthiness and Equality

- `None` is falsy.
- `0`, `0.0`, `""` are falsy.
- Empty collections `[]`, `()`, `{}` are falsy.
- All other values are truthy.
- Equality follows Python semantics for the supported types.

## Loop and Parallel Bounds

### Static Loop Analysis

The compiler statically analyzes every `for` loop:

- **Literal range**: `for i in range(N)` — N must be a non-negative
  integer constant.
- **Literal collection**: `for x in [a, b, c]` — iteration count is
  the collection length.
- **Bounded variable**: `for x in prior_result` — the variable must
  have a statically known bound from a prior `call`, `len`, or `range`.
- **Range with bounds**: `for i in range(start, stop, step)` — all
  arguments must be static integers, and `stop - start` divided by
  `step` must be finite.

### Parallel Bounds

- Maximum parallel width: configurable (default: 10).
- Maximum nested parallel depth: configurable (default: 2).
- `parallel()` call descriptors must be statically countable.

### Total Loop Budget

An upper bound on total iterations across all loops is computed and
stored in the IR. Programs exceeding the configured maximum total
iterations are rejected (TP012).

## Error Classes

| Code | Name | Description |
|------|------|-------------|
| `TP001` | `UnsupportedSyntax` | while, try, import, class, lambda, etc. |
| `TP002` | `UnboundedLoop` | Unknown iteration count |
| `TP003` | `MaxNestingDepth` | Nesting exceeds max (20) |
| `TP004` | `MaxCollectionSize` | Literal/collection too large |
| `TP005` | `BuiltInShadowing` | Shadowed call/parallel/submit_job/emit/fail |
| `TP006` | `IllegalAttributeAccess` | Disallowed method on object |
| `TP007` | `MaxParallelWidth` | Parallel group too wide |
| `TP008` | `MaxIrSteps` | IR step budget exceeded |
| `TP009` | `MaxCallSites` | Too many tool call sites |
| `TP010` | `UnresolvedIdentifier` | Unknown variable name |
| `TP011` | `InvalidCallDescriptor` | call() missing descriptor arg |
| `TP012` | `MaxTotalIterations` | Total loop iterations exceeded |
| `TP013` | `SourceTooLarge` | Source exceeds 1 MB |
| `TP014` | `MaxAstNodes` | AST node count exceeded (10K) |
| `TP015` | `MaxIdentifierLength` | Identifier too long |
| `TP016` | `UnsupportedVersion` | IR/language/compiler version mismatch |
| `TP017` | `DiagnosticSpanTooLarge` | Source span exceeds bounds |
| `TP018` | `DestructuringMismatch` | Assignment target count mismatch |
| `TP998` | `VerificationFailed` | IR verification failed |
| `TP999` | `InternalError` | Internal compiler error |

## Source-Span Diagnostics

Diagnostics include:

- Error code (e.g., `TP001`)
- Human-readable message
- Source span: byte offset and length (capped at 200 bytes of
  surrounding context)
- Never echo full source bodies or secret-sized content

## IR Versioning and Compatibility Policy

- IR format starts at version 1.
- Each IR is content-addressed with SHA-256 over: source hash,
  manifest hash, limits hash, language version, compiler version,
  parser version, and IR instruction sequence.
- The same source with the same parameters always produces the same
  IR hash.
- Stored IR can be reused only when all version/hash tuples match.
- New IR format changes increment the compiler version and invalidate
  stored IR.

## IR Opcodes (39 total)

The `IrOp` enum defines 39 opcodes in `crates/codegg-core/src/tool_program/ir.rs:93`:

| Category | Opcodes |
|----------|---------|
| Constants | `LoadInt`, `LoadFloat`, `LoadString`, `LoadTrue`, `LoadFalse`, `LoadNone` |
| Locals | `LoadLocal`, `StoreLocal` |
| Collections | `MakeList`, `MakeTuple`, `MakeDict` |
| Operators | `BinOp`, `UnaryOp`, `Compare` |
| Logic | `BoolAnd`, `BoolOr`, `BoolNot` |
| Stack | `Pop`, `Dup`, `Index`, `Slice`, `Len`, `Str`, `Int`, `Bool` |
| Control | `JumpIfFalse`, `Jump`, `ForLoopStart`, `ForLoopNext`, `ForLoopIter` |
| Tool calls | `ConstructCall`, `ExecuteCall` |
| Parallel | `ParallelStart`, `ParallelExecute` |
| Child jobs | `ExecuteChildJob` |
| Terminal | `Emit`, `Fail`, `Checkpoint`, `Return` |

## Runtime Limits (M005)

The compiler computes static bounds (`IrBounds`) that constrain
runtime execution. The interpreter enforces these via `RuntimeLimits`:

| Budget | Source | Description |
|--------|--------|-------------|
| Steps | `max_steps` | Total IR instructions executed |
| Loop iterations | `max_loop_iterations` | Per-loop cap |
| Total iterations | `max_total_iterations` | Aggregate across all loops |
| Dynamic calls | `max_dynamic_calls` | `call()` invocations |
| Parallel width | `max_parallel_width` | Concurrent `parallel()` calls |
| Parallel depth | `max_parallel_depth` | Nested parallel groups |
| Value growth | `max_value_growth` | Aggregate byte size of all live values |
| In-flight calls | `max_inflight_calls` | Concurrent broker calls |
| Wall time | `max_wall_time_ms` | Total execution time (0 = unlimited) |
| Stall time | `max_stall_time_ms` | No-progress timeout (0 = unlimited) |
| Per-call time | `max_per_call_time_ms` | Individual call timeout (0 = unlimited) |
| Retries | `max_retries` | Transient error retry count |
| Retry delay | `retry_base_delay_ms` | Base delay for exponential backoff |

Bounds are computed conservatively at compile time. Runtime limits
add executor-configured timeouts on top of static bounds.

## Examples

### Accepted

```python
results = []
for file in ["a.py", "b.py", "c.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content.split("\n"))
    if lines > 100:
        results.append({"file": file, "size": lines})

summary = {"total": len(results), "files": results}
emit(summary)
```

```python
reads = parallel(
    {"tool": "read_file", "path": "a.py"},
    {"tool": "read_file", "path": "b.py"},
)
a = reads[0]
b = reads[1]
if len(a) != len(b):
    fail("files have different lengths")
emit({"a_len": len(a), "b_len": len(b)})
```

```python
count = 0
for i in range(10):
    result = call({"tool": "grep_search", "pattern": f"TODO {i}"})
    if len(result) > 0:
        count = count + 1
emit({"found": count})
```

```python
build = submit_job("build", {"argv": ["cargo", "build", "--release"]})
emit({"build_status": build["success"]})
```

### Rejected

```python
import os              # TP001: import not allowed
while True:            # TP001: while not allowed
    pass
def foo():             # TP001: function def not allowed
    pass
[x**2 for x in range(10)]  # TP001: comprehension not allowed
lambda x: x + 1       # TP001: lambda not allowed
call({"tool": "exec", "cmd": rm -rf /})  # TP011: dangerous tool not in manifest
result = something.method()  # TP006: arbitrary attribute access
```

## Dependency Review

Parser: `rustpython-parser` 0.4.0

| Property | Value |
|----------|-------|
| License | MIT |
| MSRV | 1.72.1 |
| Features used | `all-nodes-with-ranges`, `malachite-bigint` (default features disabled) |
| Parse-only | Yes — parser produces AST, does not execute |
| Source spans | Yes — `TextRange` and `SourceRange` on all nodes |
| Fuzz posture | Upstream fuzz corpus exists; Codegg adds adversarial corpus |
| Dependency weight | ~15 transitive crates; no network/filesystem/async deps |

No `pyo3` or CPython bindings. The frontend does not use RustPython's
optional location or fold APIs.

## Static Guards

Compile-time and module-level guards prevent CPython execution:

- No `pyo3` dependency in `codegg-core/Cargo.toml`
- No `std::process::Command::new("python3")` in `tool_program/` module
- No `eval()`/`exec()`/`compile()` on user source
- `guards.rs` module documents invariants and provides
  `assert_parse_only!()` macro
- `cargo deny` / `cargo audit` in CI verifies no CPython dependencies

## Fuzz Targets

Located in `crates/codegg-core/fuzz/fuzz_targets/`:

| Target | What it tests |
|--------|--------------|
| `parser_fuzz` | Parser never panics on arbitrary bytes |
| `compiler_fuzz` | Full pipeline never panics on arbitrary input |
| `roundtrip_fuzz` | IR serialize/deserialize round-trip integrity |

Run with: `cargo fuzz run <target> -- -max_total_time=300`

## Testing

```bash
cargo test -p codegg-core --lib tool_program::parser
cargo test -p codegg-core --lib tool_program::compiler
cargo test -p codegg-core --lib tool_program::validator
cargo test -p codegg-core --lib tool_program::static_bounds
cargo test -p codegg-core --lib tool_program::ir_verifier
```

## Related Docs

- `architecture/tool_programs.md` — Domain, storage, execution
- `architecture/tool_broker.md` — Tool Broker pipeline
