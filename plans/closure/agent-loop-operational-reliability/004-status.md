# Agent Loop Operational Reliability M004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-operational-reliability/004-preflight-and-shell-authorization-semantics.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md`

Repository baseline reviewed: `f37840ef5fca36d44b35daec5da6b26cd049a3ee`

Implementation commit: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

Verified implementation head: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

## 1. Executive finding

Preflight's definite caller-policy block is distinct from advisory findings. Batch authorization consumes the explicit block policy while explicit permission decisions remain authoritative.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Caller block and advisory outcomes are distinct | `agent::tool_batch::invocation_tests` | pass | Six invocation-policy tests passed. |
| Authorization decisions remain explicit | authorization and permission regression tests; quick guards | pass | No advisory finding creates escalation by itself. |
| Cross-workspace sanity | workspace nextest | pass | 14,614 default-feature tests passed. |

## 3. Production implementation evidence

`PreflightDecision` carries a distinct blocked outcome. Tool batch applies caller policy for definite blocks separately from advisory findings, preserving existing Allow/Ask/Deny and stored approvals.

## 4. Verification executed

```bash
rtk cargo test -p codegg --lib agent::tool_batch::invocation_tests --locked
rtk cargo nextest run --workspace --locked --profile ci --no-fail-fast
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
```

Results: tool-batch invocation tests (6 passed); workspace 14,614 passed; quick and clippy passed. The full feature-configured run's unrelated test failure is documented in M007.

## 5. Invariant review

Advisories remain non-authoritative. Explicit Deny and sandbox constraints remain independent, and remembered approvals are not discarded by the new block field.

## 6. Failure and recovery review

Definite blocks return bounded denial information; non-blocking findings remain advisory. No retry or durable state was added.

## 7. Migration and compatibility review

No migration or wire change. The new decision field is internal and defaults through existing preflight construction.

## 8. Security review

No authority escalation path was introduced. Explicit hard Deny and caller permission policy remain controlling.

## 9. Documentation and operations

Updated `architecture/permission.md` to distinguish advisories from a caller-policy block.

## 10. Unresolved findings

None within M004.

## 11. Roadmap disposition

Milestone closed; M007 has completed its dependency audit and qualification.

## 12. Registry updates

M004 is closed. No successor in this line remains blocked.
