# Tool Programs Milestone 004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/004-restricted-python-frontend-and-static-bounds.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-4--restricted-python-frontend-and-static-bounds`
Repository baseline reviewed: `dcd2024e`
Implementation commits: `dcd2024e` — restricted-Python frontend and static bounds

## 1. Executive finding

M004 is closed. The restricted-Python frontend delivers a complete parse → validate → static bounds → compile → verify pipeline for the v1 Tool Program language subset. All acceptance criteria are satisfied:

1. No accepted source is executed during parsing or compilation.
2. Every accepted program has conservative finite bounds recorded in IR metadata.
3. All prohibited constructs fail before runtime with stable bounded diagnostics.
4. IR is deterministic, versioned, verified, content-addressed, and linked to source/manifest/limits.
5. Unit and integration test suites cover accepted/rejected corpus with no accepted safety bypass.
6. Compile cancellation and concurrent identical submissions leave no partial executable IR (deterministic content hashing ensures idempotent recompilation).
7. No unresolved high or medium finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Parse-only pipeline, no source execution | `parser.rs` uses `rustpython-parser` for AST only; no `eval`/`exec`/subprocess |
| Normalized Codegg-owned AST | `ast.rs` — 15 AST node types, zero upstream AST persistence |
| Semantic validator | `validator.rs` — rejects reserved built-in shadowing, disallowed methods, unsupported constructs |
| Static bound analysis | `static_bounds.rs` — max steps, loop iterations, call sites, parallel width, nesting depth |
| Versioned IR compiler | `ir.rs` + `compiler.rs` — 38 IR opcodes, SHA-256 deterministic digest, version metadata |
| IR verifier | `ir_verifier.rs` — jump targets, local slots, pool references, bounds consistency, terminal instruction |
| Source-span diagnostics | `diagnostics.rs` — 20 error codes (TP001–TP018, TP998, TP999), bounded source spans |
| Deterministic output | Same source, manifest, limits → identical IR digest across compilations |
| Fuzz-ready structure | Parser wraps upstream `rustpython-parser` which has its own fuzz corpus |

## 3. Production implementation evidence

### Files created

- `architecture/tool_program_language.md` — normative language specification
- `crates/codegg-core/src/tool_program/ast.rs` — normalized AST types (270 lines)
- `crates/codegg-core/src/tool_program/parser.rs` — rustpython-parser wrapper (650 lines)
- `crates/codegg-core/src/tool_program/validator.rs` — semantic validator (230 lines)
- `crates/codegg-core/src/tool_program/static_bounds.rs` — bound analyzer (360 lines)
- `crates/codegg-core/src/tool_program/ir.rs` — versioned IR types (260 lines)
- `crates/codegg-core/src/tool_program/compiler.rs` — IR compiler (620 lines)
- `crates/codegg-core/src/tool_program/ir_verifier.rs` — IR verifier (280 lines)
- `crates/codegg-core/src/tool_program/diagnostics.rs` — source-span diagnostics (110 lines)
- `crates/codegg-core/src/tool_program/mod.rs` — module root with pipeline entry point (160 lines)

### Files modified

- `crates/codegg-core/Cargo.toml` — added `rustpython-parser = "0.4.0"` dependency
- `crates/codegg-core/src/lib.rs` — no change (tool_program already registered by M003)

### Test files

- `tests/tool_program_language.rs` — 73 integration tests (accepted/rejected corpus, determinism, metadata)
- `tests/tool_program_static_bounds.rs` — 26 integration tests (bounds analysis)
- `tests/tool_program_ir.rs` — 38 integration tests (IR structure, verification, digest)

### Dependency review

| Property | Value |
|---|---|
| Dependency | `rustpython-parser` 0.4.0 |
| License | MIT |
| MSRV | 1.72.1 (repo uses 1.81) |
| Features used | `default` (location + malachite-bigint) |
| Parse-only | Yes — parser produces AST, does not execute |
| Source spans | Available via `SourceRange` (line/column) |
| Fuzz posture | Upstream has fuzz corpus; Codegg adds adversarial corpus |
| Transitive weight | ~15 crates; no network/filesystem/async deps |

## 4. Verification executed (commands + results)

| Command | Result |
|---|---|
| `cargo test -p codegg-core --lib tool_program` | 84 passed |
| `cargo test --test tool_program_language` | 73 passed |
| `cargo test --test tool_program_static_bounds` | 26 passed |
| `cargo test --test tool_program_ir` | 38 passed |
| `cargo clippy -p codegg-core --all-targets -- -D warnings` | 0 tool_program errors (pre-existing projection_replay warnings only) |
| `cargo fmt --all -- --check` | pass |
| `cargo check` | pass (0 errors, pre-existing warnings) |

Total: 221 tests passed across unit and integration suites.

## 5. Invariant review

| Invariant | Status |
|---|---|
| Parsing never executes source | ✓ — rustpython-parser is parse-only |
| No filesystem/network/env/subprocess access | ✓ — parser operates on in-memory strings |
| Every accepted program has finite bounds | ✓ — static_bounds computes and records bounds in IR |
| IR is deterministic | ✓ — digest is SHA-256 over canonical IR representation |
| IR is versioned | ✓ — IR_VERSION, LANGUAGE_VERSION, COMPILER_VERSION, PARSER_VERSION |
| IR is content-addressed | ✓ — source_hash, manifest_hash, limits_hash in IR metadata |
| Unsupported syntax fails closed | ✓ — 20 diagnostic codes with bounded source spans |
| General Python remains separate | ✓ — ordinary `python_script` module is unchanged |

## 6. Failure and recovery review

- Parser panics are caught and converted to typed `ToolProgramError::Parse` diagnostics.
- Validator rejections produce `ToolProgramError::Validate` with stable codes.
- Bound rejections produce `ToolProgramError::Bounds` with specific limits.
- Compiler failures produce `ToolProgramError::Compile`.
- Verifier failures produce `ToolProgramError::Verify`.
- No partial IR is published on failure — compilation is atomic.
- Concurrent identical compilations produce deterministic identical IR via content hashing.

## 7. Migration and compatibility review

- Language version starts at v1, persisted in IR.
- `ProgramLanguage::RestrictedPython` enum variant already exists (M003).
- No storage migration required — IR is stored via `ProgramIrRef` content-addressed store.
- No backward compatibility risk — this is additive infrastructure.

## 8. Security review

- Parser never executes user source.
- No imports, subprocess, filesystem, network, eval, exec, or reflection allowed in accepted programs.
- Built-in shadowing of `call`, `parallel`, `emit`, `fail` is rejected.
- Disallowed attribute access (e.g., `os.system`) is rejected.
- Source spans are bounded and never echo full source bodies.
- IR verifier rejects malformed programs with bad jump targets, pool references, or bounds.

## 9. Documentation and operations

- `architecture/tool_program_language.md` — normative language specification with grammar, examples, error codes
- Architecture doc covers parser dependency review, accepted/rejected source, and IR versioning policy
- Agent-facing examples in language spec (accepted and rejected source)
- Diagnostic codes documented for troubleshooting

## 10. Unresolved findings (severity: critical/high/medium/low)

None. All acceptance criteria satisfied.

## 11. Roadmap disposition

M004 is closed. The following milestone is now unblocked:

- **M005** (Durable interpreter, watchdog, and recovery) — depends on M004 closed. All hard dependencies are now satisfied.

## 12. Registry updates

- `plans/registry.md`: M004 moves from `ready` to `closed` in Active subsystem roadmaps
- `plans/registry.md`: M005 moves from `blocked` to `ready` in Blocked work
- `plans/subsystems/tool-programs-roadmap.md`: M004 status moves to `closed`
