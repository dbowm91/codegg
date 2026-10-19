# Agent Tool-Use Operability Corrective C003 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/003-grep-and-patch-first-use-contract.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: closed; C007 remains gated on other plans.

## 1. Finding and disposition

First-use create/search, unique grep paths, precise malformed-patch diagnostics, and workspace containment are implemented and covered. The added nested-file trajectory uses the real AgentLoop/broker path and confirms the grep result returns to the next provider request without a preceding read.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| First-use nested create and search through broker | `first_use_apply_patch_create_then_grep_runs_through_agent_broker` in `tests/agent_loop_harness.rs` | Pass |
| Exact single pathname prefix and multiple files | Grep formatting and multi-file unit tests | Pass |
| Malformed bare/range hunk and no mutation | `patch_util` parser tests and `malformed_update_does_not_change_file` | Pass |
| Traversal, outside-root and symlink containment | `tool::util`/`apply_patch` path validation tests; root policy unchanged | Pass |
| Model-facing format and error guidance | `ApplyPatchTool` descriptions and `docs/tools.md` | Pass |

## 3. Before/after trace

Before: grep output duplicated pathname prefixes and bare `@@` was accepted far enough to yield unhelpful diff feedback; direct first-use tests did not prove the broker route. After: each rendered result has one path prefix, malformed hunk headers identify the line and expected grammar, and the real scripted AgentLoop path creates a nested file and returns its grep hit to the provider.

## 4. Verification

- `cargo test --locked -p codegg --lib tool::grep` — 7 tests passed.
- `cargo test --locked -p codegg --lib tool::util` — 9 tests passed.
- `cargo test --locked -p codegg --lib tool::apply_patch` — 6 tests passed.
- `cargo test --locked -p codegg --lib tool::patch_util` — 6 tests passed.
- `cargo test --locked -p codegg --test agent_loop_harness` — 62 tests passed.
- `scripts/verify.sh quick` — passed, including format, guards, and locked workspace all-target check.

No hosted CI or live-provider run is claimed.

## 5. Compatibility and security

Tool names and `mode=create|update|delete|move` contracts remain. Create still takes file contents; update still takes a unified diff. No symlink or workspace-root relaxation was introduced. Invalid updates remain atomic with no mutation.

## 6. Failure, restart, and contention

Patch failures are returned as bounded tool errors and leave the target unchanged. Grep cancellation remains bounded by the existing search controls. This correction adds no persisted state, scheduler behavior, or restart-sensitive authority.

## 7. Limitations and severity

No known high-severity residual. The real AgentLoop trajectory covers nested create plus grep; delete/move remain covered by their existing tool contracts rather than a separate model trajectory.

## 8. Unblock audit

C003 is strictly closed. It discharges one C007 dependency. After the later C001/C004/C006 closures, C007 remains blocked solely by C002's trusted workspace-to-Eggsearch scope contract. No other registered plan became eligible.
