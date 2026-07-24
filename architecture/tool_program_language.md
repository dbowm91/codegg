# Tool Program Restricted-Python Language Specification

Status: normative (M004)

Version: 1

This document specifies the restricted-Python language subset accepted by the Tool Program compiler. The language is deliberately minimal — it provides deterministic control flow, bounded iteration, and safe tool invocation without requiring CPython execution, imports, reflection, or arbitrary standard-library access.

## 1. Design Principles

1. **Parse-only pipeline**: The parser never executes source. Compilation produces deterministic IR without loading modules or spawning processes.
2. **Fail-closed**: Unknown syntax, ambiguous constructs, and unbounded operations are rejected at parse or validation time.
3. **Bounded execution**: Every accepted program has statically provable finite bounds for steps, iterations, calls, parallelism, and nesting.
4. **Deterministic output**: The same source, manifest, limits, and versions always produce identical IR and deterministic content hashes.
5. **Separate from general Python**: Tool Programs are not general Python. The ordinary `python_script` tool remains unrestricted.

## 2. Grammar (Version 1)

### 2.1 Program

```text
program     = statement* EOF
```

### 2.2 Statements

```text
statement   = assignment
            | if_stmt
            | for_stmt
            | assert_stmt
            | tool_call_stmt
            | parallel_stmt
            | emit_stmt
            | fail_stmt
            | pass_stmt

assignment  = target_list '=' expression
if_stmt     = 'if' expression ':' block ('elif' expression ':' block)* ['else' ':' block]
for_stmt    = 'for' target 'in' iterable ':' block
assert_stmt = 'assert' expression [',' expression]
tool_call_stmt = target '=' 'call' '(' arguments ')'
parallel_stmt  = target '=' 'parallel' '(' call_list ')'
emit_stmt   = 'emit' '(' expression ')'
fail_stmt   = 'fail' '(' [expression] ')'
pass_stmt   = 'pass'
```

### 2.3 Expressions

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

### 2.4 Call Descriptors

Call descriptors are structured arguments to `call()`:

```text
call_list   = call_descriptor (',' call_descriptor)*
call_descriptor = '{' key_value (',' key_value)* '}'
key_value   = STRING ':' expression
```

### 2.5 Targets

```text
target_list = target (',' target)*
target      = IDENTIFIER
```

Destructuring assignment (`a, b = ...`) is supported for simple target lists.

## 3. Allowed Built-ins

| Name | Signature | Description |
|------|-----------|-------------|
| `call` | `(tool: dict, **kwargs) -> Any` | Invoke an approved tool through the Tool Broker |
| `parallel` | `(*calls: dict) -> list` | Execute call descriptors concurrently |
| `emit` | `(value: Any) -> None` | Emit a structured result value |
| `fail` | `(reason: str) -> None` | Fail the program with a reason |
| `len` | `(collection) -> int` | Collection length |
| `str` | `(value) -> str` | String conversion |
| `int` | `(value) -> int` | Integer conversion |
| `bool` | `(value) -> bool` | Boolean conversion |

Shadowing `call`, `parallel`, `emit`, or `fail` as a local variable is rejected by the validator.

## 4. Value Types

### 4.1 Primitives

| Type | Representation | Bounds |
|------|---------------|--------|
| `None` | null literal | — |
| `bool` | `True` / `False` | — |
| `int` | Decimal integer literal | Configurable max magnitude (default: 2^63) |
| `float` | Decimal float literal | Configurable max magnitude (default: 2^53 mantissa) |
| `str` | Single/double-quoted string | Configurable max length (default: 10,000 chars) |

### 4.2 Collections

| Type | Syntax | Element bounds |
|------|--------|---------------|
| `list` | `[a, b, c]` | Configurable max elements (default: 1,000) |
| `tuple` | `(a, b, c)` | Configurable max elements (default: 1,000) |
| `dict` | `{k: v}` | Configurable max entries (default: 1,000) |

### 4.3 Slicing

Slicing produces the same type as the source collection. Slice indices must be compile-time integers or `None`.

## 5. Deterministic Evaluation Order

All expressions evaluate left-to-right. Function arguments evaluate left-to-right. No short-circuit evaluation is performed for `and`/`or` — both operands are evaluated.

## 6. Truthiness and Equality

- `None` is falsy.
- `0`, `0.0`, `""` are falsy.
- Empty collections `[]`, `()`, `{}` are falsy.
- All other values are truthy.
- Equality follows Python semantics for the supported types.

## 7. Loop and Parallel Bounds

### 7.1 Static Loop Analysis

The compiler statically analyzes every `for` loop:

- **Literal range**: `for i in range(N)` — N must be a non-negative integer constant.
- **Literal collection**: `for x in [a, b, c]` — iteration count is the collection length.
- **Bounded variable**: `for x in prior_result` — the variable must have a statically known bound from a prior `call`, `len`, or `range`.
- **Range with bounds**: `for i in range(start, stop, step)` — all arguments must be static integers, and `stop - start` divided by `step` must be finite.

### 7.2 Parallel Bounds

- Maximum parallel width: configurable (default: 10).
- Maximum nested parallel depth: configurable (default: 2).
- `parallel()` call descriptors must be statically countable.

### 7.3 Total Loop Budget

An upper bound on total iterations across all loops is computed and stored in the IR. Programs exceeding the configured maximum total iterations are rejected.

## 8. Error Classes

| Code | Description |
|------|-------------|
| `TP001` | Unsupported syntax (while, try, import, class, lambda, etc.) |
| `TP002` | Unbounded loop or unknown iteration count |
| `TP003` | Maximum nesting depth exceeded |
| `TP004` | Maximum collection/literal size exceeded |
| `TP005` | Built-in shadowing of reserved name |
| `TP006` | Illegal attribute access on arbitrary object |
| `TP007` | Maximum parallel width exceeded |
| `TP008` | Maximum IR steps exceeded |
| `TP009` | Maximum call sites exceeded |
| `TP010` | Unresolvable identifier |
| `TP011` | Invalid call descriptor structure |
| `TP012` | Maximum total loop iterations exceeded |
| `TP013` | Source too large |
| `TP014` | Maximum AST node count exceeded |
| `TP015` | Maximum identifier length exceeded |
| `TP016` | Unsupported compiler/language version |
| `TP017` | Source body too large for diagnostic span |
| `TP018` | Destructuring assignment target count mismatch |

## 9. Source-Span Diagnostics

Diagnostics include:

- Error code (e.g., `TP001`)
- Human-readable message
- Source span: byte offset and length (capped at 200 bytes of surrounding context)
- Never echo full source bodies or secret-sized content

## 10. IR Versioning and Compatibility Policy

- IR format starts at version 1.
- Each IR is content-addressed with SHA-256 over: source hash, manifest hash, limits hash, language version, compiler version, parser version, and IR instruction sequence.
- The same source with the same parameters always produces the same IR hash.
- Stored IR can be reused only when all version/hash tuples match.
- New IR format changes increment the compiler version and invalidate stored IR.

## 11. Examples

### 11.1 Accepted

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

### 11.2 Rejected

```python
import os                          # TP001: import not allowed
while True:                         # TP001: while not allowed
    pass
def foo():                          # TP001: function def not allowed
    pass
[x**2 for x in range(10)]          # TP001: comprehension not allowed
lambda x: x + 1                     # TP001: lambda not allowed
call({"tool": "exec", "cmd": rm -rf /})  # TP011: dangerous tool not in manifest
result = something.method()         # TP006: arbitrary attribute access
```

## 12. Dependency Review

Parser: `rustpython-parser` 0.4.0

| Property | Value |
|----------|-------|
| License | MIT |
| MSRV | 1.72.1 |
| Features used | `default` (location + malachite-bigint) |
| Parse-only | Yes — parser produces AST, does not execute |
| Source spans | Yes — `TextRange` and `SourceRange` on all nodes |
| Fuzz posture | Upstream fuzz corpus exists; Codegg adds adversarial corpus |
| Dependency weight | ~15 transitive crates; no network/filesystem/async deps |

## 13. Runtime Limits (M005)

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
