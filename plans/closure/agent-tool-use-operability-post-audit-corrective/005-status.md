# Agent Tool-Use Operability Corrective C005 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/005-project-compatible-verify-defaults.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: closed.

## 1. Finding and disposition

Routine lint no longer forces every optional feature. Verification now validates package and feature selection against the workspace manifest, builds typed Cargo argv, binds execution to the configured workspace root, and keeps workspace-wide all-features unavailable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Default offline lint without `--all-features` | Exact resolver argv regression | Pass |
| Package and optional feature selection; explicit all-features policy | Temporary workspace tests, positive bounded package opt-in, invalid package/feature and injection negatives | Pass |
| Unsupported project/scope rejection | Resolver tests | Pass |
| CWD isolation | Bound-root test proves unrelated context cwd is ignored and fallback/missing-root behavior is typed | Pass |
| Bounded timeout/output and truthful result | `exit_status_and_report_are_bounded`; tool execution suite | Pass |
| Documentation and policy coherence | `docs/tools.md`, `architecture/testing.md`, registry policy | Pass |

## 3. Verification

- `cargo test --locked -p codegg --lib tool::verify` — 6 tests passed.
- `cargo test --locked -p codegg --test tool_execution` — 61 tests passed.
- `scripts/verify.sh quick` — passed, including locked workspace all-target check.

The verifier was tested with a temporary Cargo workspace containing multiple packages and an optional feature. No full `verify.sh full`, workspace-wide all-features sweep, or hosted CI run is claimed.

## 4. Compatibility and security

Existing `verify(action=lint)` remains valid. Package/feature values are validated and passed as process arguments, not shell text. Workspace all-features is rejected because it enables real-server tests; explicit all-features is limited to a selected package. No workspace escape or arbitrary command surface is added.

## 5. Failure, restart, and contention

Timeout and output limits remain bounded, and command execution continues through the existing tool/scheduler ownership path. No durable state or restart behavior changes.

## 6. Limitations and severity

No known high-severity residual. The broad full verification mode was not run; the required quick suite and focused targets passed.

## 7. Unblock audit

C005 is strictly closed. It discharges one C007 dependency. After the later C001/C004/C006 closures, C007 remains blocked solely by C002's trusted workspace-to-Eggsearch scope contract; no other registered plan became eligible.
