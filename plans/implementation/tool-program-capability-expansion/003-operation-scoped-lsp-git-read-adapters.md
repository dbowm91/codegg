# Tool Program Capability Expansion Milestone 003 — Operation-Scoped LSP and Git Read Adapters

Status: ready for handoff (unblocked by M002 closure at `plans/closure/tool-program-capability-expansion/002-status.md`; M001 closed at `plans/closure/tool-program-capability-expansion/001-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/tool-program-capability-expansion-roadmap.md#M003--operation-scoped-GitLSP-read-adapters`

Long-term requirements: `plans/000-long-term-specification.md#42-explicit-ownership`, `#46-progressive-disclosure`, `#29-system-invariants`.

Applicable ADRs: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`. Primary class: capability.

## 1. Objective

Expose a small selected subset of high-value Git and LSP read operations to Tool Programs through hidden `ProgrammaticOnly` (or equivalently narrow) contracts that delegate canonical Git/LSP owners, without making the broad mutation-capable `git` or `lsp` tools program-callable.

## 2. Why this milestone is ready

Unblocked by M002 closure (`plans/closure/tool-program-capability-expansion/002-status.md`; M001 closed at `plans/closure/tool-program-capability-expansion/001-status.md`) so eligibility/trust/replay policy is stable. Git/worktree ownership and LSP mutation-preview/application correctness are already closed; these are read adapters only.

## 3. Current implementation evidence

`git` and `lsp` are multiplexed tools. LSP exposes diagnostics/symbols/hover/definition/reference and also preview/mutation-related operations; Git surfaces reads and mutations. `ToolContract` is tool-level, so marking the multiplexed tool `DirectOrProgrammatic` would structurally admit mutation-capable inputs. Existing canonical backend functions/crates must remain execution owners.

## 4. Invariants that must not regress

No Git write/worktree mutation/LSP edit or preview-apply operation is programmatically available; adapter code contains no independent backend logic or subprocess shell fallback; workspace/execution context is explicit; output bounded/structured; hidden adapters do not expand ordinary model-visible tool surface; broker/manifest/effect authority remains canonical.

## 5. Scope

In: choose a minimal read subset such as Git status/diff/log/branch metadata and LSP diagnostics/symbols/hover/definition/references where existing canonical APIs provide bounded reads; hidden narrow contracts/adapters; program manifests/tests/docs. Out: commit/stage/checkout/merge/rebase/worktree writes, LSP rename/format/code-action apply, arbitrary executeCommand, new Git/LSP backend.

## 6. Required production changes

Contracts: create programmatic-only operation names or a contract mechanism that cannot express mutation inputs. Implementations: delegate typed `codegg-git`/egglsp/tool backend reads with same path/workspace/provenance rules. Disclosure: hidden from direct model palettes/tool_search unless diagnostics intentionally show program-only contracts. Cache/replay: classify each read; LSP state may be workspace-version dependent and must not claim stable determinism beyond ledger semantics.

## 7. Ordered work packages

A — inventory multiplexed Git/LSP operations and produce explicit allow/deny effect table.

B — select bounded high-value read subset and identify canonical backend methods; stop any candidate requiring duplicated logic.

C — implement hidden programmatic-only adapters/contracts with structured schemas and workspace context.

D — broker/program fixtures for allowed reads and exhaustive mutation-name/input negatives.

E — cache/replay/state-version/provenance tests and docs; verify ordinary model disclosure unchanged.

## 8. Failure, cancellation, restart, and contention semantics

Reads fail typed when workspace/LSP unavailable or Git state races. No adapter takes a mutation lock except canonical read consistency requirements. Program cancellation propagates through broker/backend. Rerun may see changed repository/LSP state; ledger replay semantics remain explicit.

## 9. Compatibility and migration

Additive hidden contracts; direct `git`/`lsp` unchanged. Historical program manifests without adapters remain valid; new contract hashes/versioning apply normally.

## 10. Required tests

Allowed operation matrix; exhaustive denied mutation operations; workspace/path/project isolation; LSP unavailable/restart; Git concurrent state change; structured bounds; broker caller policy; manifest/cache/replay; ordinary model tool-definition count/disclosure unchanged; no shell/subprocess duplicate path.

## 11. Required verification commands

```bash
cargo test --workspace lsp --no-fail-fast
cargo test --workspace git --no-fail-fast
cargo test --test tool_contract_guards
cargo test --test tool_program_runtime
cargo test --test tool_surface_minimization
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Tool Programs, tool broker, Git, LSP and agent tool-surface docs; callable operation matrix.

## 13. Acceptance criteria

Programs can perform the selected bounded Git/LSP reads through canonical owners; every mutation-capable operation is structurally unavailable, not merely prompt-discouraged; ordinary model-visible tool surface does not grow; no duplicate backend exists.

## 14. Stop conditions

Dependencies not closed; per-tool contract architecture cannot safely isolate operations without broad rewrite; selected read requires mutation-capable backend path; adapter would duplicate Git/LSP execution.

## 15. Closure evidence required

Operation allow/deny table, canonical delegation map, mutation negative suite, disclosure count, workspace/provenance/cache/replay tests, exact verification results.

## 16. Handoff notes

Prefer a few high-value reads over broad parity. `ProgrammaticOnly` is specifically useful here to avoid re-expanding the prompt tool surface closed by maintainability M002.
