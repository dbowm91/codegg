# Tool Program Capability Expansion Milestone 001 — Deterministic Local Read Contract Expansion

Status: implemented — closed by `plans/closure/tool-program-capability-expansion/001-status.md`

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/tool-program-capability-expansion-roadmap.md#M001--deterministic-local-read-contract-expansion`

Long-term requirements: `plans/000-long-term-specification.md#42-explicit-ownership`, `#47-correctness-before-transparent-magic`, `#29-system-invariants`.

Applicable ADRs: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`. Primary class: capability.

## 1. Objective

Establish an executable programmatic-tool eligibility matrix and safely promote the local `diff` operation to Tool Programs after giving it a structured bounded contract and execution-context-derived workspace/path policy. Admit no other production tool without the same evidence.

## 2. Why this milestone is ready

Tool Programs are strictly closed through M020; broker/contract/manifest/cache/replay/authority foundations are stable. `read/glob/grep/list` prove the local-read pattern. `diff` is already non-mutating and bounded in file size but lacks the required contract/context shape.

## 3. Current implementation evidence

`read`, `glob`, `grep`, and `list` return `ToolContract` with `DirectOrProgrammatic` and `ReadOnly`. `DiffTool` is categorized read-only, canonicalizes a path against its `allowed_root`, compares provided original text to current file, and returns a unified diff string; it does not override `contract()` and defaults its root from process cwd.

## 4. Invariants that must not regress

Program calls go through broker; captured principal/workspace/path policy controls file access; no process cwd as program authority; output/input bounds explicit; effect `ReadOnly`; cache/retry/replay metadata truthful; contract/manifests version/invalidate correctly; no mutation/network promotion.

## 5. Scope

In: eligibility matrix/check helper or tests, `diff` structured output/schema/provenance, execution-context workspace policy, programmatic caller contract, manifest/cache/replay tests, audit of nearby local read candidates with disposition. Out: repo_search (M002), LSP/Git (M003), write/edit/apply_patch/Bash/terminal, broad refactor of Tool trait.

## 6. Required production changes

Tool contract: define `diff` caller/effect/input/output/retry/cache/projection semantics. Execution: use canonical `ToolExecutionContext`/workspace policy rather than cwd/default root for program calls; direct compatibility may retain existing constructor behavior if safe. Result: structured bounded diff metadata/content with truncation/provenance. Program manifest/runtime: ensure contract is discoverable and hash/cache/replay correctly. Docs: eligibility rules.

## 7. Ordered work packages

A — encode eligibility checklist in tests/matrix: authority, effects, bounds, context, determinism/trust, schemas, retry/cache/replay, globals.

B — refactor `diff` to structured execution and explicit workspace/path context while preserving direct behavior.

C — set `DirectOrProgrammatic` + `ReadOnly` and conservative cache/retry policy; update manifest/catalog as necessary.

D — program fixture invoking diff, replay/cache/ledger/provenance and path-escape negatives.

E — census other local read tools and record eligible/deferred reasons without adding unrelated promotions.

## 8. Failure, cancellation, restart, and contention semantics

Missing/stale path or invalid original input fails deterministically. Program cancellation stops pending call through broker/runtime. Replay follows existing ledger contract; no second filesystem read is claimed deterministic if runtime replay semantics say use recorded result. Concurrent reads do not mutate workspace.

## 9. Compatibility and migration

Direct `diff` input remains compatible. Add structured result without breaking string-facing direct callers through existing structured-tool adapter conventions. Contract hash changes must naturally invalidate stale manifests/cache.

## 10. Required tests

Eligibility matrix; direct diff compatibility; program diff success; path escape/symlink/root policy; size/output truncation; structured schema; broker caller denial/allow; manifest hash; cache/replay/ledger; mutation tools remain unavailable.

## 11. Required verification commands

```bash
cargo test --test tool_program_read_palette
cargo test --test tool_contract_guards
cargo test --test tool_program_runtime
cargo test --workspace diff --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

`architecture/tool_programs.md`, `architecture/tool_broker.md`, tool contract docs and AGENTS/skills if palette is enumerated.

## 13. Acceptance criteria

A Tool Program can call `diff` through broker against its authorized workspace and receive bounded structured output; path escape and mutation attempts fail; contract/replay/cache metadata is truthful; existing direct callers continue to work.

## 14. Stop conditions

ADR-0001 would need weakening; `diff` cannot derive authority from existing execution context; structured compatibility requires a new tool runtime; proposed additional candidate introduces network/mutation/hidden globals.

## 15. Closure evidence required

Eligibility matrix, before/after diff contract, authority/path negatives, program runtime/replay/cache/manifest tests, direct compatibility, candidate disposition, exact commands/results.

## 16. Handoff notes

Do not equate `ToolCategory::ReadOnly` with safe programmatic admission. The contract evidence is the gate.
