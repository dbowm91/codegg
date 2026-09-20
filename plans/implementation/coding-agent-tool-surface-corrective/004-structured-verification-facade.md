# Coding-Agent Tool Surface Corrective M004 — Structured Verification Facade

Status: implemented

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (production baseline)

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m004--structured-verification-facade`

Dependency:

- M002 must close first so verification exposure follows the corrected coding-profile contract.

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs: none. Existing command-intent, scheduler, managed-process, and test ownership must be reused.

Primary class: capability

## 1. Objective

Add one bounded model-facing verification tool for routine compile/check/lint/typecheck/format-check workflows without creating another command executor.

The facade should let a coding agent request semantic verification such as:

```text
check
build
lint
typecheck
format_check
```

and receive a compact structured result containing command family, resolved argv/cwd, terminal status, diagnostic summary, bounded output/artifact handle, elapsed time, and next-action hints where deterministic.

The tool must compose the existing command-intent classifier/planner, scheduler-owned job submission, managed process service, run/audit provenance, and existing language/project detection. `test` remains the canonical test runner and should be invoked/delegated for test semantics rather than copied.

## 2. Why this milestone is blocked

M002 must first establish that normal coding profiles expose a complete verify loop and settle the immediate/deferred place for verification tools. M004 is then an additive ergonomic capability.

The underlying execution stack is already mature; no architecture decision is pending.

## 3. Current implementation evidence

The baseline already contains:

- `CommandIntentKind::Build`, `Lint`, `Format`, test and Python-analysis classifications;
- shell-shape parsing and managed argv planning;
- canonical managed-process/scheduler dispatch;
- a dedicated supervised `test` tool with structured scheduler ownership and bounded reports;
- run-store/provenance/audit infrastructure;
- LSP diagnostics that can supplement but must not replace process/toolchain truth.

Routine coding agents currently tend to use `bash` for commands such as `cargo check`, Clippy, `ruff check`, `mypy`, `tsc --noEmit`, formatting checks, etc. This works but requires the model to construct command syntax and parse heterogeneous output.

## 4. Invariants that must not regress

- No second process executor, scheduler, retry owner, or command policy.
- Explicit user-supplied/custom commands remain subject to existing shell/command policy; the verification facade does not become a generic shell escape.
- `test` remains canonical for test execution.
- Verification executes on the workspace-owning node/cwd.
- Output is bounded; full logs use existing artifact/run mechanisms.
- Permission/sandbox/approval semantics remain those of the resolved underlying command operation.
- A verification failure is reported truthfully and never converted to a successful tool result because parsing failed.
- Format-check is read-only; auto-format mutation is not part of this milestone.
- Network/package installation is never inferred as part of verification.

## 5. Scope

### In scope

- A `verify` tool or repository-conventional equivalent.
- Semantic action enum: at minimum `auto`, `check`, `build`, `lint`, `typecheck`, `format_check`.
- Project/language-aware resolution through existing command-intent/project metadata.
- Optional package/path/workspace scope when existing planners can express it safely.
- Structured result DTO and output schema.
- Delegation to `test` when action explicitly requests tests only if the contract is cleaner than excluding tests; do not copy TestTool.
- Deterministic fixtures for Rust, Python, JS/TS, Go, and generic Make-style projects where current command-intent support exists.
- Profile/disclosure/docs integration.

### Explicitly out of scope

- New package installation or toolchain bootstrap.
- Generic arbitrary command execution.
- Auto-fixing lint/format errors.
- Replacing LSP diagnostics.
- Replacing CI.
- New language-specific verification framework.
- Benchmark/performance testing.
- Remote CI/execution backends.

## 6. Required production changes

### Tool contract

Prefer a compact schema similar to:

```json
{
  "action": "auto|check|build|lint|typecheck|format_check",
  "scope": "workspace|package|path",
  "package": "...",
  "path": "...",
  "timeout": 300,
  "max_report_bytes": 20000
}
```

Exact fields follow repository conventions. Avoid a raw `command` field in the primary contract. If an escape hatch is required for unsupported projects, use existing `bash` rather than growing `verify` into shell.

### Resolver

Build on existing command-intent/project command resolution. One semantic action may map to more than one serial command only when the project already defines a canonical bounded sequence. Do not invent language conventions when no existing resolver/config supports them.

For `auto`, choose the safest high-value non-test verification family based on project evidence. Return the resolved action/commands so the model knows what actually ran.

### Execution

Submit through existing scheduler/managed command path with the current workspace/session/turn attribution. Preserve timeout/cancellation/audit/run-store semantics.

### Structured result

Return a typed JSON result with at least:

- requested/effective action;
- project/language detector result if available;
- commands/argv actually run, with safe bounded representation;
- terminal status;
- diagnostic counts/classes where parsable;
- bounded textual summary;
- artifact/log handle for omitted output when available;
- elapsed time;
- whether any command was skipped/unavailable.

Parsers may extract well-known diagnostics, but raw terminal status remains authoritative.

## 7. Ordered work packages

### Work package A — Resolver census

Inventory existing command-intent resolution for Cargo/Rust, Python, JS/TS, Go, Make/generic projects. Identify which semantic verification actions already have deterministic command mappings.

Acceptance evidence: no invented command family is presented as canonical.

### Work package B — Verify contract and resolver

Add the bounded semantic tool and route each supported action to canonical command plans.

Acceptance evidence: resolved argv/cwd is the same machinery used by Bash command-intent routing rather than a new subprocess builder.

### Work package C — Scheduler execution and structured projection

Run through scheduler-owned execution and project results into the typed bounded schema.

Acceptance evidence: cancellation/timeouts/audit/run provenance match existing managed execution.

### Work package D — Language/project fixtures

Add loopback/local project fixtures for supported families, including success and diagnostic failure.

### Work package E — Exposure/docs

Place `verify` in the M002-corrected coding profile according to schema cost; likely immediate for Curated coding profiles and discoverable for Minimal if M002 matrix supports it.

## 8. Failure, cancellation, restart, and contention semantics

A resolver miss returns `unsupported`/actionable diagnostics and suggests `bash` rather than guessing.

Scheduler queueing, cancellation, timeout, process-tree cleanup, and restart semantics are unchanged. A daemon restart may interrupt an active verification according to current job recovery policy; the tool must report existing job state rather than relaunch automatically unless the scheduler already classifies replay as safe.

Parallel verification respects scheduler admission/resource classes. Do not launch unbounded per-package fan-out.

## 9. Compatibility and migration

Additive tool only; no storage/protocol migration.

Existing Bash and TestTool behavior remains unchanged. Model profiles may gain a new available tool but existing `disabled_tools` should be able to disable it normally.

## 10. Required tests

### Focused unit tests

- semantic action schema/validation;
- resolver mappings for each supported project family;
- unsupported project/action behavior;
- structured result projection/truncation;
- no raw arbitrary-command field/path.

### Integration tests

- Rust `cargo check`/Clippy/format-check fixture;
- Python lint/typecheck where configured/available in deterministic fixture;
- JS/TS type/lint fixture;
- Go check/lint/build fixture where existing support exists;
- cancellation/timeout through scheduler;
- failed diagnostics remain failed.

### Security/negative tests

- verify cannot execute arbitrary shell syntax;
- no implicit package install/network fetch;
- workspace root/path scoping enforced;
- read-only format check does not write files.

## 11. Required verification commands

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib tool::test
cargo test --test command_routing_adversarial
cargo test --test tool_execution
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Add the new focused verify tests to this list during implementation.

## 12. Documentation updates

- `architecture/tool.md`
- `architecture/command_intent.md`
- scheduler/managed-process docs only as needed to reference the facade
- agent tool-surface/profile docs

## 13. Acceptance criteria

M004 closes when agents can request routine semantic verification through one bounded structured facade that uses the existing resolver/scheduler/process owners, returns truthful structured status, keeps output bounded, and cannot be used as an arbitrary shell or mutation bypass.

## 14. Stop conditions

Stop if:

- M002 is not closed;
- command-intent lacks a stable resolution contract for enough actions to justify the facade;
- implementation requires a second subprocess/scheduler path;
- project detection would require speculative language/package-manager logic outside existing ownership;
- test execution must be copied instead of delegated to TestTool.

## 15. Closure evidence required

Include resolver support matrix, command-owner trace, structured result examples for success/failure/timeout, negative arbitrary-command/network/mutation evidence, profile exposure disposition, exact verification results, and residual unsupported project families.

## 16. Handoff notes

This is a semantic projection over installed machinery. Keep the tool deliberately smaller than Bash: once it starts accepting arbitrary commands or auto-fixes, the ownership boundary has been lost.
