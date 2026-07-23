# Tool Programs Milestone 004 — Restricted-Python Frontend and Static Bounds

Status: closed

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-4--restricted-python-frontend-and-static-bounds`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, job, artifact

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: infrastructure

## 1. Objective

Implement an in-process parser, validator, static-bound analyzer, and deterministic compiler for the documented restricted-Python Tool Program language. Accepted source must compile to a versioned CodeGG IR without executing Python or loading user modules.

This milestone is compile-only. It must not execute real tools or claim runtime capability.

## 2. Readiness boundary

Hard dependency: M003 closure, including stable source/IR references, limits, manifest schema, and checkpoint version seams.

Before implementation, perform a bounded dependency review for the selected maintained Rust Python parser. The parser must parse only; it must not execute source or require a persistent Python subprocess. Record dependency size, license, MSRV, feature flags, and fuzz posture in closure evidence.

## 3. Current implementation evidence

- The ordinary Python subsystem invokes `python3 -I` for AST-oriented risk analysis and falls back to string scanning.
- That analyzer detects broad risk categories but does not define a safe executable language subset or produce replayable IR.
- Existing model tools accept arbitrary Python source in analyze/transform/verify modes.
- No language grammar, source-span diagnostics, static loop bound, call graph, or IR version currently exists for Tool Programs.

## 4. Invariants that must not regress

- Parsing and validation never execute source.
- Accepted syntax cannot access filesystem, network, environment, subprocesses, imports, reflection, dynamic evaluation, threads, signals, or native code.
- Every loop and parallel collection has a finite enforced upper bound.
- Compiler output is deterministic for the same source, manifest, language version, and limit snapshot.
- Source spans and diagnostics are bounded and do not echo secret-sized source bodies.
- Unknown syntax or parser ambiguity fails closed.
- General Python scripting remains a separate tool and is not silently routed through this subset.

## 5. Scope

### In scope

Version 1 syntax sufficient for deterministic tool orchestration:

- constants: null/None, booleans, integers, bounded strings;
- lists, tuples, and dictionaries;
- local assignment without destructuring complexity;
- `if`/`else`, boolean operators, comparisons, membership checks;
- bounded `for` loops over literal/bounded collections, prior bounded results, and bounded `range`;
- safe indexing, slicing, length, and selected pure collection/string helpers;
- explicit built-ins `call`, `parallel`, `emit`, and `fail`;
- structured call descriptors and JSON-compatible values;
- source-span diagnostics and deterministic IR.

### Explicitly out of scope

- `while`, recursion, user-defined functions, lambdas, generators, async/await, comprehensions in version 1, classes, decorators, context managers, exceptions, imports, attributes on arbitrary objects, `eval`, `exec`, `compile`, globals/locals, mutation of external state, or arbitrary standard-library functions.
- CPython bytecode execution.
- Real Tool Broker invocation.
- Automatic source repair by a model inside the compiler.

## 6. Required production changes

### Language specification

Add a normative `architecture/tool_program_language.md` containing:

- grammar/subset table;
- allowed built-ins and value types;
- numeric/string/container size behavior;
- deterministic evaluation order;
- truthiness and equality semantics;
- loop and parallel bounds;
- error classes and source-span format;
- IR versioning and compatibility policy;
- examples of accepted and rejected source.

### Parser and AST normalization

- Parse in-process through a maintained Rust parser with minimal features.
- Convert upstream parser nodes immediately into Codegg-owned normalized AST types; do not persist third-party AST structures.
- Reject unsupported statements/expressions before semantic analysis.
- Bound source bytes, AST nodes, nesting depth, literal bytes, collection elements, identifier length, and diagnostics.
- Normalize identifiers and disallow confusable or reserved built-in shadowing where it could alter safety.

### Static bound analysis

Compute or conservatively cap:

- maximum IR steps;
- maximum loop iterations per loop and total;
- maximum syntactic call sites and dynamic call count upper bound;
- maximum parallel width and nested parallel depth;
- maximum collection/value growth;
- maximum emitted result shape;
- maximum nested block/condition depth.

Reject programs whose bound cannot be proven within configured limits. A runtime limit remains mandatory later even when a static bound exists.

### IR compilation

Define a versioned IR with explicit instructions for:

- load/store local;
- construct bounded values;
- comparison/boolean operations;
- branch and bounded iterator frames;
- construct call request;
- sequential call;
- bounded parallel call group;
- emit/fail;
- checkpoints at call and loop boundaries.

IR must include source-span mapping, language version, manifest hash, limit hash, compiler version, and deterministic digest.

### Program store integration

- Load source by immutable reference and verify digest.
- Store compiled IR only after successful validation and hash it.
- Persist compile diagnostics and terminal blocked/failed state without creating a runtime attempt.
- Reuse an existing matching IR only when source, manifest, limits, language/compiler version, and parser version identity match.

## 7. Ordered work packages

### Work package A — Parser dependency and normalized AST

- Complete dependency review and lock the parser behind a narrow module.
- Add normalized AST types and bounded parse API.
- Add corpus tests for supported Python syntax and fail-closed unsupported forms.

### Work package B — Semantic validator

- Enforce identifier, built-in, type, attribute, statement, expression, and scope restrictions.
- Reject shadowing of `call`, `parallel`, `emit`, and `fail`.
- Produce stable diagnostic codes and bounded source spans.

### Work package C — Static cost and termination analysis

- Implement conservative abstract size/call/iteration analysis.
- Require finite collection/range bounds.
- Detect nested amplification and reject overflow/unknown bounds.
- Property-test calculated bounds against a reference metered evaluator for generated safe programs.

### Work package D — Versioned IR compiler

- Compile normalized AST into deterministic CodeGG IR.
- Add verifier that checks jump targets, stack/local bounds, call metadata, checkpoint placement, and terminal instruction.
- Store and reload IR through M003 content references.

### Work package E — Fuzzing, docs, and guards

- Add parser/validator/compiler fuzz targets and adversarial corpus.
- Add static guards preventing use of CPython execution in the Tool Program compiler/runtime modules.
- Publish language docs and agent-facing examples.

## 8. Failure, cancellation, restart, and contention semantics

- Compilation is bounded synchronous/managed CPU work and must respect a scheduler or blocking-work budget; it cannot monopolize the async runtime.
- Cancellation before or during compilation aborts without publishing partial IR.
- Atomic IR publication occurs only after full verification and digest calculation.
- Restart may reuse only a complete verified IR with matching version/hash tuple.
- Parser panic is converted to a typed internal failure and covered by fuzz/regression tests; no partial state becomes executable.
- Concurrent compilation of the same fingerprint coalesces or safely produces one canonical content-addressed IR.

## 9. Compatibility and migration

- Language version starts at v1 and is persisted.
- Future syntax requires a new language/compiler version; old accepted source remains reproducible with the recorded version or fails blocked if support is removed.
- Third-party parser upgrades must not silently change stored IR semantics; parser/compiler version participates in the cache key.
- Ordinary `python_script` remains unrestricted according to its separate sandbox/mode policy and is not source-compatible by implication.

## 10. Required tests

### Focused unit tests

- every allowed and denied AST node;
- scope and built-in shadowing;
- literal/container/depth limits;
- deterministic IR and source-span mapping;
- IR verifier negative cases.

### Property and fuzz tests

- parser never panics on arbitrary bytes;
- accepted generated programs terminate under calculated bounds in a reference evaluator;
- mutation fuzzing cannot turn rejected import/attribute/eval constructs into accepted IR;
- deterministic digest across repeated compilation.

### Integration tests

- load source/manifest/limits, compile, store IR, reload, verify hashes;
- compile cancellation and concurrent coalescing;
- unsupported compiler/language version blocks safely.

### Security and negative tests

- imports and alias tricks;
- dunder/reflection access;
- huge integers/strings/collections;
- deep nesting and exponential loop/call amplification;
- Unicode/confusable identifiers;
- malformed AST and forged IR.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool_program
cargo test -p codegg --test tool_program_language
cargo test -p codegg --test tool_program_static_bounds
cargo test -p codegg --test tool_program_ir
cargo fuzz run tool_program_parser -- -max_total_time=300
cargo fuzz run tool_program_validator -- -max_total_time=300
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

Use the repository’s actual fuzz mechanism if cargo-fuzz is not already adopted; do not claim unavailable fuzz evidence.

## 12. Documentation updates

- new `architecture/tool_program_language.md`
- `architecture/tool_programs.md`
- dependency/license inventory
- agent prompt examples for bounded programs
- troubleshooting for diagnostic codes

## 13. Acceptance criteria

1. No accepted source is executed during parsing or compilation.
2. Every accepted program has conservative finite bounds recorded in IR metadata.
3. All prohibited constructs fail before runtime with stable bounded diagnostics.
4. IR is deterministic, versioned, verified, content-addressed, and linked to source/manifest/limits.
5. Fuzz/adversarial suites show no accepted safety bypass or panic.
6. Compile cancellation and concurrent identical submissions leave no partial executable IR.
7. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if the selected parser executes source, materially violates dependency/MSRV/licensing policy, cannot preserve source spans, or requires accepting syntax whose bounds cannot be proven. Do not replace static rejection with a promise that runtime timeouts will catch it.

## 15. Closure evidence required

Create `plans/closure/tool-programs/004-status.md` with dependency review, grammar coverage matrix, accepted/rejected corpus, static-bound property evidence, fuzz commands/durations/crashes, deterministic IR evidence, cancellation/concurrency tests, and residual findings.

## 16. Handoff notes

Keep the v1 subset deliberately small. It is easier to add syntax later than to revoke an unsafe accepted construct. No real tools should be invoked in this milestone; use compile fixtures only.
